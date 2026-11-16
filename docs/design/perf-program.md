# Performance program

_Written 2026-11-15, after the first DigitalOcean measurements. Owner's direction: keep improving until the gain is
significant; if it cannot be reached there is no point in building the library. This file says where a gain can exist,
what to try, and when to stop. Numbers are `[measured here]` from `bench/results/2026-11-14-lean/` (one lane per
competitor, so indicative) or derived from them, and say which._

## 1. What the first measurements say about where a margin can exist

The database host's CPU use was recorded in every final run (`db_cpu_busy_frac`):

| scenario | database CPU busy (median) | meaning |
|---|---:|---|
| S1, S2, R1, R2 at 64 connections | 0.83 to 0.85 | **server-bound.** Every good client lands at 76k to 79k/s (S1); no client can show a margin here. |
| S2 at 1 connection | 0.05 | **latency-bound**: client overhead plus the network round trip; the server idles. |
| R1 pipelined, 8 connections | 0.66 | client-bound; there is room. karez-shared 56.0k/s against tokio-postgres 53.2k/s. |
| S8 scan of 13M rows | not recorded | to be recorded (see 3.1). |

Derived (`[derived from the above]`): in S1 the server spends about 4 vCPUs x 0.85 / 79k/s = 43 us of CPU per request,
so the ceiling of this database host is about 93k/s. The best client reaches 79k/s, 85% of it. **On unpipelined OLTP the
most any client can gain is about 1.17x, and only by making the server do less per request** (fewer packets per request).
karez is already level with the best clients there. The plan said so in advance (plan section 1.3: OLTP asks for parity).
Chasing a large margin on single-row OLTP is chasing the server's own limit. It will not be found, and promising it would
be a claim the data contradicts.

Where a client can still make a large difference:

1. **Bulk and columnar reads** (S4, S8). One backend produces a result at about 1.4 us per row (S8: 733k rows/s,
   `[measured here]`), and karez's own CPU is 0.55 CPU-s per million rows, about 40% of the pass. The server's single
   backend is the limit; the way past it is to use more backends for one result (3.1).
2. **Bulk writes** (S5). karez inserts 123k rows/s with per-row binds. sqlx reaches 205k/s with multi-row `VALUES`
   statements (`[measured here]`, read from `bulk_async.rs`). That is a real gap, and the cause is known (3.3).
3. **Client CPU and memory per request**, and the single-connection latency floor. These are where the zero-dependency,
   sans-io design should show (3.2, 3.4). They decide cost and tail latency in production even when throughput is
   server-bound.

**Gap in the measurements to close first.** The plan's stop-rule scenario (S4 through the chunk path, at least 2x the best
Rust row API) was *not measured* in campaign 1: `karez-blocking-chunk` was not in any lane's client list. It is in the
karez group now (`bench/cloud/ctl.sh`). Until it runs, the central claim of the plan is untested.

## 2. Rules for this program

- A lever is kept only if a measurement on a droplet shows the gain against the best competitor in the same lane, at
  n >= 2, with the losses in the same table.
- Each lever has a kill criterion fixed before the work. If it fails, it is dropped and written down here.
- **Stop rule (plan section 1.3, unchanged):** if the chunk-path S4 is under 2x the best Rust row API after the levers
  of 3.1, or OLTP is more than 10% behind the fastest Rust client, the architecture is not worth a new crate family.
  The plan's fallback is option C (a thin standards-shaped API over existing drivers). This is a possible outcome and
  the owner should expect it to be stated, not hidden.

## 3. Levers, with the reasoning and the test

### 3.1 Partitioned parallel reads (the main candidate)

*Idea.* A query is executed by one backend, so one connection cannot use more than one database core. Split one logical
scan into k disjoint ranges (primary-key bisection, or ranges of `ctid`), run them on k connections, and stream the chunks
back in order or unordered. All k connections read one consistent snapshot: `pg_export_snapshot()` on the first, then
`SET TRANSACTION SNAPSHOT` on the others (the mechanism `pg_dump -j` uses). ConnectorX partitions the same way and
reports 21x less time than pandas on a 10x scan (`docs/research/consumers-and-bridges.md`, `[vendor]`; its comparison is
against pandas, not against the Rust drivers, so it is not our baseline).

*Model.* With k connections the scan rate is about min(k, c_db) x r1 with an upper bound from the client side of
k x (1 / 0.4) x r1 (one decoding thread per connection keeps up while the server is the limit), and from the network.
For S8, r1 = 733k rows/s and the database host has 4 vCPUs: **the bound is about 2.5x to 3x** (the kernel and the
network take part of the cores). The network: S8 moves about 1.8 GB in 17.8 s, about 100 MB/s; the DigitalOcean limit
for a `c-4` is `[unverified]` and must be measured with iperf before the plan counts on it.

*Test.* (1) Record `db_cpu_busy_frac` and bytes for S8 and S4 (small change in `run_bulk`). (2) Prototype
`scan_partitioned(query, key, k)` in the bench adapter only, with k in 1, 2, 4, 8, and measure on a droplet. *Kill
criterion:* under 1.8x at k=4 on the 13M-row table, or any result mismatch (the checksum is the guard).

### 3.2 The latency floor ("floor adapter")

*Idea.* The best a client can do for one request is send one pre-built message and read one reply. Write a minimal adapter
that sends pre-encoded `Bind/Execute/Sync` bytes on a blocking socket and reads until `ReadyForQuery`, with no decoding.
Its p50 at one connection is the floor for every client, including karez.

*Test.* Run it in the same lanes as S2 at one connection. If karez-blocking is within 3% of the floor, **there is no
latency gain to find** and the single-connection work stops. If it is 10% or more above, profile the gap
(`perf`, syscall counts with `strace -c`; a request should cost one `sendto` and one `recvfrom`). Campaign 1's p50 was
222 us for karez-blocking at one connection against 181 us for deadpool-postgres on another lane, which could be lane
noise; the floor adapter in the same lane settles it.

### 3.3 Bulk writes: multi-row and array binding

*Idea.* Per-row `Bind/Execute` costs a message pair per row on the wire and a plan execution per row on the server. A
multi-row `INSERT ... VALUES (...),(...)` or `INSERT ... SELECT ... FROM UNNEST($1::int8[], $2::text[], ...)` costs one
statement per batch. PostgreSQL limits one statement to 65,535 bind parameters, so 10 columns allow 6,553 rows per
statement with `VALUES`; `UNNEST` has one parameter per column and no such limit. COPY (S6, 638k rows/s) stays the
fastest path for pure loads; this lever is for `INSERT` semantics (defaults, `ON CONFLICT`, triggers).

*Test.* Add `execute_many_columnar` (UNNEST) and a multi-row path to the S5 adapter; compare with sqlx's 205k/s and
libpq's 128k/s. *Kill criterion:* under 1.3x the current 123k/s.

### 3.4 Client CPU and memory per request

*Idea.* karez-blocking spends 23.5 CPU-s per million S1 requests, libpq 24.8, tokio-postgres 27.4 (`[measured here]`);
sqlx 72; asyncpg 69 in S2 (it did not meet the S1 bound). The floor adapter (3.2) gives the kernel share. Reductions beyond that: reuse of encode buffers per
connection (already), `writev` of header and body, avoiding a second `read` when the reply arrives in one segment, and
for pipelining `MSG_MORE`/batched sends. `io_uring` would remove syscalls entirely but needs `unsafe` and a dependency, so
it could only be an optional crate outside the core, and only if the floor adapter shows the kernel share is large.

*Test.* `perf stat` and `strace -c` on S1 at 64 connections, per client. *Kill criterion:* under 10% CPU per request saved.

### 3.5 Pipelined OLTP (client-bound, database 66% busy)

karez-shared leads tokio-postgres by 5% in R1 pipelined at n=1 lane. Depth and batching policy (how many requests per
write, when to flush) is a small search space: the model is throughput = min(depth / (RTT + s_server), 1 / s_server_cpu),
so the optimum depth is the one that just saturates the server. Test depths 2, 4, 8, 16, 32 in the same lane. Gain is
bounded by the database's 34% idle: at most about 1.5x, in practice less.

## 4. Order of work and cost

| step | where | cost |
|---|---|---|
| a. Record database CPU and bytes in bulk runs; floor adapter; S5 multi-row and UNNEST; COPY BINARY versus DataRow for S8 | WSL, then one droplet pair for a few hours | WSL free; droplets about $1 |
| b. Partitioned scan prototype (3.1) | WSL for correctness, droplet pair for the numbers | about $1 |
| c. Decide against the kill criteria; write the outcome in this file | | |
| d. Campaign 2 (two rounds, [bench/CAMPAIGN2-PLAN.md](../../bench/CAMPAIGN2-PLAN.md)) with the surviving levers, the chunk client and the floor adapter | six droplets, up to 12 h | up to about $9, owner's go-ahead needed |

Running steps a and b on one lane (two droplets, a few hours, about $0.25 per hour) before campaign 2 means the big
campaign measures an improved library once instead of measuring the old one twice. This needs the owner's go-ahead for
that smaller spend too.

## 5. What would count as "significant"

By scenario, against the best competitor in the same lane, at n >= 2 and with the same checksum: S4 and S8 through the
chunk path at least 2x (the plan's number); S5 at least 1.3x the best `INSERT` path; S2 at one connection within 5% of
the floor adapter; OLTP at 64 connections level (server-bound, so a tie is the correct outcome); client CPU per request
10% or more below the best Rust client. If only the first two hold, karez is a bulk and columnar library with parity on
OLTP and the standards exports, and the README should say exactly that.

## 6. Results of the first experiments (2026-11-16, WSL)

**Read this first.** These ran on a laptop under WSL: 12 logical CPUs, PostgreSQL 18 in a container, loopback, the client and
the server sharing the CPUs. They are `[measured here, WSL]` and prove correctness and give ratios inside one machine.
They are not evidence of speed on real hardware and no number below may be quoted outside this file. Each needs the same
test on a droplet pair before it counts (section 4, steps a and b). Each pass is the median of 5, same checksum for every
variant. Data: the 1M-row, 10-column `bench_wide` table; `bench_ins` for S5.

| experiment | result | kill criterion | outcome |
|---|---|---|---|
| **S4 on one connection:** row view, chunk path, tokio-postgres | 0.57 s, 0.56 s, 0.56 s per 1M rows. Client CPU: karez 0.27 s (row) and 0.25 s (chunk), tokio-postgres 0.57 s. | the plan's gate asks chunk at least 2x the best Rust row API | **cannot be met on one connection.** The single server backend produces the rows at about 0.55 us each; the chunk path saves client CPU (about half of tokio-postgres), not wall time. |
| **3.1 partitioned scan** (`karez-blocking-part`, k connections, chunk path, id ranges, same checksum) | k=1: 0.66 s; k=2: 0.39 s (1.7x); k=4: 0.30 s (2.2x); k=8: 0.20 s (3.3x). Against tokio-postgres on one connection (0.56 s): 1.9x at k=4, 2.8x at k=8. | under 1.8x at k=4 | **passes** (2.2x). The bound I derived (2.5x to 3x on 4 database vCPUs) is not yet tested; WSL has 12 CPUs. No shared snapshot in the prototype (the table is static). |
| **3.3 S5 multi-row insert** (`karez-blocking-mrow`, 1000 rows per statement, 10,000 parameters) | per-row path 0.53 s per 100k rows; multi-row 0.27 s (1.9x); sqlx 0.31 s to 0.33 s; tokio-postgres 1.86 s. Batch 200: 0.41 s, 500: 0.35 s, 3000: 0.30 s, 6000: 0.31 s. | under 1.3x | **passes** (1.9x), and 1.15x to 1.2x ahead of sqlx's multi-row on the same machine. The best batch is 1000 to 3000. `UNNEST` was not tried: karez-pg has no array parameters yet. |
| **3.2 latency floor** (`bin/floor`, one connection, closed loop, 100k requests, 3 runs) | karez-blocking p50 over the raw pre-encoded floor: S1 0.93, 1.02, 1.04; S2 1.03, 1.01, 1.02. p99 within 6%. | within 3% = nothing to gain; 10% or more = profile | **within 3% (about 2 to 5 us of 150 us).** Single-connection latency work stops here: the client is at the floor, the rest is the server and the network. |
| **COPY binary versus row messages for S4** (`bin/copyvs`, nothing decoded) | wall 0.60 s for both. Client CPU 0.16 s (COPY) against 0.28 s (rows). All-CPU busy 1.2 s against 1.3 s. 127 MB for 1M rows. | a lower wall time | **killed as a speed lever.** The server's cost per row is the same. It lowers client CPU only; keep it in mind for the Arrow path where the format is already columnar-friendly. |

### What this changes

1. **The plan's S4 gate (2x the best Rust row API) is reachable only with partitioned reads.** On one connection every
   client is server-bound and karez's win is client CPU (about half of tokio-postgres) and memory. The honest claim that the
   data supports so far: *with partitioning, 2x to 3x the one-connection scan on one machine class; on a droplet pair this
   is unmeasured.* A competitor could partition too (ConnectorX does), so the report must show karez's partitioned scan
   next to their single-connection scan and say that it is a different access path. The comparison that matters for users
   is "what do I get from the library without writing the partitioning myself".
2. **Bulk insert has a clear fix:** a multi-row path (and `UNNEST` once arrays exist) is 1.9x the current `execute_many`
   on this machine. It belongs in the library API (an `insert_batch`), not only in the benchmark.
3. **Stop work on single-connection latency and on COPY binary for speed.** They cost time and the data says nothing is there.

### Next (needs a droplet pair, about $0.25 per hour, owner's go-ahead for the smaller spend)

- Repeat the table above on one lane (database and client droplets) with `db_cpu_busy_frac` recorded for bulk runs
  (`run.py` now records it) and k in 2, 4, 8, 16: this tests the 4-vCPU bound and the network limit (iperf first).
- Add a shared snapshot (`pg_export_snapshot`, `SET TRANSACTION SNAPSHOT`) to the partitioned scan and check the cost.
- Partitioned S8 on the real taxi table (primary-key ranges on `trip_id`), checksum against the single scan.
- Move the partitioned scan and the multi-row insert from the benchmark into `karez-pg` as API, with tests.
- `UNNEST` insert once karez-pg has array parameters.

## 7. Results on a droplet pair (2026-11-16, one lane, 24 minutes)

`[measured here]` on DigitalOcean `c-4` in `blr1` (4 vCPU, 8 GB, Intel Xeon Platinum 8280), PostgreSQL 18.7 with huge
pages, private network, one database droplet and one client droplet. Raw data: `bench/results/2026-11-16-perf1/` (gzipped
cells). One lane, one run per variant, median of 5 timed passes (S8: 3), the same checksum for every variant of a
scenario. **Indicative, not a ranking**: n=1 and one lane. The droplets were deleted at 02:30Z (24 minutes, about $0.10).

**The network is the limit for bulk reads on this machine class.** `iperf3` between the two droplets: 1.90 Gbit/s with one
stream and 1.85 Gbit/s with four (about 237 MB/s), round trip 0.6 ms average (ping). The S4 result is 127 MB on the wire,
so no client can finish S4 in less than about 0.54 s here.

| scenario | variant | seconds | rows/s | client CPU s | database CPU busy |
|---|---|---:|---:|---:|---:|
| S4 (1M rows, 10 columns, 127 MB) | karez row view | 1.031 | 970k | 0.35 | 0.24 |
| | karez chunk path | 0.781 | 1.28M | 0.36 | 0.24 |
| | sqlx | 0.756 | 1.32M | 0.81 | 0.21 |
| | pgx | 0.788 | 1.27M | 0.72 | 0.24 |
| | tokio-postgres | 0.814 | 1.23M | 0.66 | 0.24 |
| | libpq | 0.929 | 1.08M | 0.54 | 0.21 |
| | karez partitioned, k=2 | 0.727 | 1.38M | 0.53 | 0.46 |
| | karez partitioned, k=4 | **0.574** | 1.74M | 0.73 | 0.71 |
| | karez partitioned, k=8 | 0.581 | 1.72M | 0.63 | 0.88 |
| | karez partitioned, k=16 | 0.796 | 1.26M | 0.59 | 0.92 |
| | k=4 and k=8 with one shared snapshot | 0.560 and 0.602 | | | |
| S8 (13M rows, 17 columns, NULLs) | karez chunk path, one connection | 16.85 | 776k | 6.4 | 0.27 |
| | pgx | **13.85** | 943k | 15.5 | 0.25 |
| | tokio-postgres | 17.24 | 758k | 12.0 | 0.26 |
| | libpq | 26.4 | 494k | 16.2 | 0.19 (6.5 GB RSS) |
| | karez partitioned, k=2 / 4 / 8 / 16 | 11.3 / 10.1 / 9.7 / 9.6 | 1.15M / 1.29M / 1.35M / 1.36M | 7.5 / 12.6 / 14.2 / 14.8 | 0.46 / 0.73 / 0.81 / 0.82 |
| | k=4 and k=8 with one shared snapshot | 9.97 and 9.63 | | | |
| S5 (100k inserts) | karez per-row `execute_many` | 0.817 | 122k | 0.12 | 0.21 |
| | karez multi-row, 500 / 1000 / 3000 per statement | 0.504 / 0.449 / 0.445 | 198k / 223k / 225k | 0.07 to 0.10 | 0.17 to 0.19 |
| | sqlx (multi-row) | 0.480 | 208k | 0.11 | 0.16 |
| | asyncpg, libpq, pgx, tokio-postgres | 0.70, 0.75, 1.04, 1.71 | 143k, 133k, 97k, 59k | | |

Latency floor (one connection, 100k requests, 3 runs, trust role): karez-blocking p50 over the raw floor was
0.88, 0.93, 0.91 for S1 and 1.03, 0.92, 1.07 for S2. **At the floor.** COPY binary: 0.89 to 1.14 s against 0.85 to 1.06 s
for row messages, client CPU half (0.16 s against 0.35 s): no wall-clock gain.

### Reading the results against the criteria of section 5

| criterion | result on this machine | verdict |
|---|---|---|
| S4 and S8 at least 2x the best competitor | S4: partitioned 0.574 s against sqlx 0.756 s = **1.32x**; chunk path on one connection 0.97x of sqlx. S8: partitioned 9.6 s against pgx 13.85 s = **1.43x**; karez on one connection is **22% slower than pgx**. | **not met**, and the network (1.9 Gbit/s) is near the wall: S4 k=4 already moves 127 MB in 0.57 s = 1.77 Gbit/s |
| S5 at least 1.3x the best `INSERT` path | multi-row 223k against sqlx 208k = **1.07x**; 1.82x our own per-row path | **not met** (level with sqlx) |
| S2 at one connection within 5% of the floor | 0.88 to 1.07 | **met** |
| OLTP at 64 connections level | campaign 1: level with the best, server-bound | **met** |
| client CPU per request 10% lower than the best Rust client | S1 at 64 connections: karez 23.5 against tokio-postgres 27.4 (campaign 1) = 14% lower; bulk S8 single connection: 6.4 CPU-s against 12.0 (tokio-postgres) | **met** |

**The plan's stop rule (section 1.3: S4 under 2x the best Rust row API, or OLTP more than 10% behind) is triggered for
S4 on this hardware.** I do not draw the final conclusion from it because: (1) it is one lane and n=1 per competitor;
(2) the network class caps the result, and a competitor could not exceed 0.54 s either. The WSL run, where there is no
network limit, reached 3.3x at k=8 against one connection. The question the data leaves open is whether the 2x gate is
physically reachable on any hardware the owner cares about, or only on a faster network than DigitalOcean's `c-4`. **That
needs one measurement on a larger droplet class (a faster network), about $0.50, and it decides whether the bulk path
claim stands.** The network limit of larger classes is `[unverified]` here; measure it with iperf3 first.

### Things found that cost nothing to fix and should be fixed regardless

1. **Single-connection S8: karez was 22% behind pgx in one run (16.85 s against 13.85 s), not yet confirmed** with the server about 100% busy on one
   backend in both. karez reads and decodes in one thread, so the server stalls while karez decodes; pgx overlaps. A reader
   thread with a small ring of buffers (std only) should bring karez to pgx's level or past it. This is the most solid
   gain in this list; expected 1.2x on S8, to be proven.
2. **Correction (2026-11-16, same day): the "row view 32% slower" reading is not supported.** The client CPU of the row view and the chunk path was the same (0.35 s and 0.36 s) and the first cell of a lane is often slow (cold caches, a noisy minute). One run per variant cannot separate that from a real difference. The same doubt applies to "karez is 22% behind pgx on S8". The next droplet run interleaves three rounds of the single-connection clients (A B C A B C) before any code is changed for either.
3. **Partitioning gets worse beyond the point where the server is saturated** (S4 k=16: 0.80 s, database 92% busy): the
   API should pick k from a measured probe or a default of about the database core count, not a fixed large number.
4. **A partitioned scan with a `WHERE` range is slower per row than the plain scan** (`part1` 1.01 s against 0.78 s on S4):
   range predicates on a sequential scan cost the server per-row work. Prefer `ctid` ranges or a table sample; test.
5. **The multi-row insert is a library feature, not a benchmark trick**: 1.8x our own `execute_many`. It only equals sqlx.
   The remaining gain is `COPY` (S6) for loads and `UNNEST` with array parameters.

### Decision needed from the owner

The honest state: **OLTP parity is reached, client CPU is clearly lower, and the bulk-read margin the plan needs (2x) is
not reached on DigitalOcean `c-4` hardware.** Options, in the order I would take them:

1. Fix items 1 and 2 above (cheap, local, testable in WSL for correctness), then measure again on a **larger droplet class**
   with iperf3 first, one lane, about $0.50. This decides whether the 2x gate is reachable at all.
2. If it is not reached there either, follow the plan's stop rule: option C (thin standards-shaped API over existing drivers),
   keeping what was learned (the benchmark harness, the multi-row and partitioned-scan ideas) as contributions.
3. Do not widen the claim from "lower client CPU, level OLTP, and a partitioned scan that is 1.3x to 1.4x faster here" until
   a measurement says more.

## 8. Repeat on a second droplet pair, interleaved (2026-11-16, 25 minutes, about $0.10)

**Larger droplet classes are not available:** creating a `c-8` failed with "This size is currently restricted, please open a
ticket to increase your account tier", and the sizes API lists nothing above `c-4` for this account in `blr1`. Raising the
tier is a request to DigitalOcean that only the owner can make. So the "does a faster network change the result" question
stays open. What was done instead: a second `c-4` pair (a different physical placement: the one-connection round trip was
75 to 83 us here against 150 us on the first pair), with the single-connection clients **interleaved three times** (A B C A B C)
and the partitioned scans repeated. Raw data: `bench/results/2026-11-16-perf3/`. `[measured here]`, one lane, indicative.

The network is the same: iperf3 1.90 Gbit/s (one stream) and 1.86 Gbit/s (four), ping average 0.59 ms.

| scenario | single connection, median of three rounds (range) | partitioned karez, best |
|---|---|---|
| S4, 1M rows (127 MB) | karez chunk 0.693 s (0.663 to 0.737); karez row view 0.788 (0.686 to 0.810); pgx 0.657 (0.600 to 0.804); tokio-postgres 0.705 (0.579 to 0.772); libpq 0.744 (0.664 to 0.793); sqlx 0.750 (0.741 to 0.886) | k=4: **0.565 s**; k=2: 0.591; k=8: 0.599; k=16: 0.688; k=32: 1.019 |
| S8, 13M rows | karez chunk 14.09 s (13.45 to 14.29); pgx 14.22 (13.70 to 14.45); **tokio-postgres 12.35 (12.15 to 12.91)** | k=2: 9.89 s; k=4: 9.72; k=8: 9.72; k=16: 9.66; k=32: 9.65; k=8 with a shared snapshot: 9.68 |
| S5, 100k inserts | karez per-row 0.726 s (138k/s); sqlx 0.457 s (219k/s) | multi-row 1000: 0.419 s (239k/s) |
| client CPU, S8 | karez 6.2 to 6.6 CPU-s; tokio-postgres 12.4 to 13.0; pgx 15.5 to 16.2 | partitioned (k=4): 11.3 |
| one-connection latency | karez/floor p50: S1 1.03, 1.01, 1.03; S2 1.00, 1.05, 0.99 | |

**What this settles**

1. **The "karez is 22% behind pgx" and "row view is 32% slower" readings were noise.** With three interleaved rounds the
   run-to-run spread of a single client is about +-15% (S4) and +-5% (S8), and karez, pgx, libpq and sqlx overlap on S4.
   On S8 the best single-connection client is **tokio-postgres, 12% faster than karez** (12.35 s against 14.09 s) with twice
   the CPU; karez and pgx are level. No code was changed for either claim. A read-ahead thread could still close the 12%
   on S8; it is a possible gain of about 1.1x, not the 1.2x guessed, and it would break the library's "one thread, no channel"
   design for the blocking client. Not worth it unless a bigger gain appears.
2. **The partitioned scan is flat at the network limit:** S8 stays at 9.65 s from k=2 (9.89) up to k=32, which is 2.3 GB in
   9.65 s = 1.9 Gbit/s, the iperf number. S4 k=4 moves 127 MB in 0.565 s = 1.8 Gbit/s. **The database CPU is not the limit
   (0.60 to 0.67 busy at S8); the network is.** Against the best single-connection competitor the partitioned scan is
   **1.16x on S4 (0.565 against pgx 0.657) and 1.28x on S8 (9.65 against tokio-postgres 12.35)**. Against karez's own single
   connection it is 1.2x and 1.46x.
3. **The 2x gate (plan section 1.3) is not reached on this network class, and cannot be: even an infinitely fast client
   cannot finish S8 in less than about 9.6 s here.** On a network without that ceiling the WSL run reached 2.8x over
   tokio-postgres at k=8 (`[measured here, WSL]`, loopback, 12 CPUs). Whether a real cloud network close to the
   database (10 Gbit/s or more) reaches 2x is the open question; a competitor could also partition.
4. **Too many partitions hurt:** S4 k=32 is 1.02 s (database 93% busy, 32 backends on 4 vCPUs). The API default should be
   about the database's core count, bounded by the measured bandwidth.
5. **What holds:** the one-connection latency is at the floor (0.99 to 1.05); client CPU is about half of tokio-postgres and
   a third of pgx on S8; the multi-row insert is 1.7x karez's own per-row path and level with sqlx (1.09x, n=1).

## 9. Where this leaves the decision (owner)

The plan's stop rule said: if S4 is under 2x the best Rust row API, or OLTP is more than 10% behind, the architecture is not
worth a new crate family. Measured on the hardware available: **OLTP level (met), one-connection latency at the floor (met),
client CPU about half of the best Rust client (met), bulk reads 1.16x to 1.28x with a partitioned scan and about level on one
connection (not met), bulk inserts level with sqlx (not met).** Three honest options:

1. **Test a network without the 1.9 Gbit/s ceiling** (ask DigitalOcean to raise the account tier, or use another provider or
   a single large machine with the database and the client side by side). One lane, under $1. Decides whether the 2x gate is
   reachable at all. The owner must request the tier or approve another provider; I cannot.
2. **Apply the stop rule: option C** (a thin standards-shaped API over existing drivers). Keep the harness, the partitioned
   scan and multi-row insert ideas, and the finding that single-row OLTP cannot be beaten (server-bound).
3. **Change what "worth it" means.** The measured advantages are real but of another kind than a 2x speed claim: half the CPU
   per request and per scanned row (a cost saving at scale), zero dependencies, a small binary (not yet measured), and the
   standards parity and exports no Rust client offers (not built). If the owner values those, the speed claim in the README
   stays "level on OLTP, lower CPU, partitioned scans up to the network limit", with nothing stronger.

## 10. The same comparison with no network limit (2026-11-16, WSL, loopback)

The owner asked to test a faster network. A faster two-machine network cannot be had with this DigitalOcean account (section 8:
`c-8` and larger are restricted; raising the tier is a support ticket only the owner can open). What can be done now is the
other end of the range: **no network limit at all**. PostgreSQL 18 in a Docker container in WSL (1 GB shared buffers), the
client on the same machine over loopback, 12 logical CPUs shared by both, the full 13,069,067-row taxi table and the 1M-row S4
table. `[measured here, WSL]`; summary in `bench/results/2026-11-16-wsl-nonet/summary.txt`. Single-connection clients
interleaved three times; partitioned runs once each (median of 5 passes for S4, 3 for S8). libpq did not run (shared library
missing in WSL). The client and the server compete for the CPUs, which hurts the partitioned runs; competitors were not
partitioned.

| scenario | best single connection (median of 3, range) | karez partitioned | ratio |
|---|---|---|---|
| S4, 1M rows | karez chunk 0.625 s (0.584 to 0.671); pgx 0.632 (0.605 to 0.736); sqlx 0.751; tokio-postgres 0.794 (0.656 to 0.812) | k=8: **0.263 s**; k=12: 0.302; k=4: 0.340; k=16: 0.421; k=2: 0.616 | **2.4x** over the best single connection (karez chunk and pgx), 3.0x over tokio-postgres |
| S8, 13M rows | tokio-postgres 12.95 s (12.75 to 14.61); karez chunk 13.28 (12.91 to 14.62); pgx 16.21 (13.56 to 19.44) | k=12: **3.79 s**; k=8: 4.13; k=16: 4.33; k=4: 6.03; k=2: 10.17 | **3.4x** over the best single connection |
| client CPU, S8 | karez 5.0 to 5.6 CPU-s; tokio-postgres 11.0 to 12.4; pgx 13.3 to 19.6 | 7.6 to 9.0 | |

**What this shows.** Without the network ceiling the partitioned scan clears the plan's 2x gate: 2.4x on S4 and 3.4x on S8 over
the best single-connection client, with about half the CPU of tokio-postgres even when partitioned. With the DigitalOcean
`c-4` network (1.9 Gbit/s) the same code gives 1.16x and 1.28x (section 8). **The result is therefore decided by the
bandwidth between the client and the database, and by how many cores the database has**: the speed-up is bounded by
min(k, database cores, bandwidth / (bytes per row x single-connection rate)). On this laptop the bound was about 12 cores; on
`c-4` it is the 1.9 Gbit/s wire.

**Caveats that matter.**

1. **Competitors can partition too.** ConnectorX does. The honest framing is "karez ships a partitioned, snapshot-consistent
   scan as one call, and a single-connection client does not". A user who writes the partitioning on top of tokio-postgres
   would get most of the same speed-up (their CPU use would be about twice as high).
2. **`k=2` on S8 (10.17 s) is far slower than the model predicts (about 6.6 s).** The range predicate probably makes the planner
   choose an index scan for a half-range. This is a planner effect of `WHERE trip_id > lo AND trip_id <= hi` and must be
   handled: use `ctid` ranges or `TABLESAMPLE`-free block ranges (`WHERE ctid >= '(n,0)' AND ctid < '(m,0)'`, PostgreSQL 14+
   has a TID range scan), and check the plan with `EXPLAIN`.
3. **Loopback is not a real network and 12 shared CPUs are not a real server.** The WSL number says the code has no hidden
   ceiling below roughly 3x. It does not say a real 10 Gbit/s link gives 3x. That needs a two-machine run on a faster link.
4. The 1M-row S4 table stays inside PostgreSQL's cache in both settings, so storage speed is not part of either result.

## 11. Updated state of the decision

The plan's stop rule is **not** simply triggered: the 2x gate is reachable (2.4x to 3.4x with no network limit) and not reachable
on the one network class available (1.16x to 1.28x). Whether karez's bulk-read claim holds depends on the customer's network and
database size, which the owner knows better than the benchmark does. To close the question with evidence:

- Ask DigitalOcean to raise the account tier (or use another provider the owner already has) and run one lane on droplets
  with a faster network (`perf-lane.sh` is ready; about $1). That turns the laptop number into a real one.
- Meanwhile, three things are worth doing in the code regardless, all local and testable: (a) a `ctid`-range partitioner and
  an `EXPLAIN` check (finding 2); (b) the partitioned scan as a real API (`scan_partitioned`) with a shared snapshot and a
  default `k` taken from the database's core count; (c) the multi-row insert as `insert_batch`.

## 12. The two features are in the library (2026-11-16)

- **`karez_pg::scan_partitioned(&Config, &Scan, init, on_chunk)`** (`crates/karez-pg/src/scan.rs`): splits an ordinary table into
  ranges of heap blocks (`ctid >= '(a,0)' AND ctid < '(b,0)'`, bound as parameters, PostgreSQL 14+), reads each range on its own
  connection and thread, and by default makes all of them read one snapshot (`pg_export_snapshot`, `SET TRANSACTION SNAPSHOT`).
  Per-partition state, chunks delivered to a callback, first error stops the others. Default partitions: the client's CPUs, at
  most 8. A table with no heap blocks is read on one connection.
- **`Connection::insert_batch(table, columns, rows)`** (`crates/karez-pg/src/batch.rs`): multi-row `INSERT ... VALUES`, up to 1,000
  rows and 65,535 parameters per statement, atomic (own transaction when none is open).
- **Invariant kept:** user data is bound server-side. Identifiers are quoted. The one string put into SQL text is the
  server-generated snapshot id (`SET TRANSACTION SNAPSHOT` takes only a literal), checked to be hex digits and dashes.
- **Tests:** 7 unit tests (block ranges cover the table exactly once, snapshot-id check, identifier quoting, statement text,
  parameter limit) and 4 real-server tests (`insert_batch` atomicity and rollback, every row read once for k in 1, 2, 3, 5, 8 with
  and without snapshot, a snapshot that holds while another connection inserts and deletes, empty table, missing table and
  column, callback error, usage errors). They pass on PostgreSQL 14, 16 and 17; the whole `karez-pg` suite (46 integration tests)
  passes on 16; `cargo fmt` and `cargo clippy` are clean. The MSRV (1.85) toolchain was not installed in WSL, so it was **not**
  rechecked for the new code.
- **Through the bench adapter** (`KAREZ_API=1`, WSL, 12 shared CPUs, `[measured here, WSL]`, noisy: the same configuration varied
  by up to 40% between runs): the checksums of S4 and S8 are identical to the single-connection scan for every k. S4 k=8: 0.26 to
  0.39 s; S8 k=4 / 8 / 12: 9.1 / 5.4 / 5.2 s against 13.3 s on one connection. The shared snapshot showed no consistent cost
  (S4 k=8: 0.31 to 0.34 s with it, 0.36 to 0.39 s without, in one back-to-back pair).
- **Open:** (1) S8 with k=2 is no faster than one connection (12.3 to 15.8 s), also with ctid ranges, so my earlier planner
  explanation (an index scan for a half-range) is **not** confirmed; the cause is unknown (possibly parallel query on the plain
  scan, possibly WSL). The default of 8 or the client's CPU count avoids it. (2) Each call opens its connections; with SCRAM that
  is a few milliseconds each, which is noticeable on small tables. A pool-taking variant is the obvious next step. (3) The
  droplet numbers of sections 7 and 8 came from the prototype (id ranges); the API (ctid ranges) has not been run on a droplet.

## 13. The library API on a droplet pair (2026-11-16, 23 minutes, about $0.10)

`[measured here]`, DigitalOcean `c-4` pair in `blr1` (a third pair), PostgreSQL 18.7, one lane, indicative. Raw data:
`bench/results/2026-11-16-perf4/`. This run used the library's `scan_partitioned` (ctid ranges, shared snapshot) and
`insert_batch`, not the prototype. Every S4 and S8 result has the same checksum as the single-connection scan. iperf3: 1.90 Gbit/s
(one stream), 1.85 (four), round trip 0.56 ms: the same network as before.

| scenario | single connection (two interleaved rounds) | `scan_partitioned` | best ratio |
|---|---|---|---|
| S4, 1M rows | karez chunk 0.588 / 0.616 s; tokio-postgres 0.595 / 0.608; pgx 0.648 / 0.684; sqlx 0.841 / 0.876 | k=2: 0.535 s; k=4: 0.541; k=8: 0.530 (no snapshot: 0.554); k=16: 0.548 | **1.12x** over tokio-postgres |
| S8, 13M rows | tokio-postgres 12.04 / 11.79 s; karez chunk 13.96 / 12.03; pgx 16.48 / 17.06 | k=2: 11.98 s; k=4: 9.61; k=8: 9.65 and 9.69 (no snapshot: 9.60); k=16: 9.74 | **1.23x** over tokio-postgres |
| S5, 100k inserts | karez per-row 0.744 / 0.753 s (133k to 134k rows/s); sqlx 0.449 / 0.472 (212k to 223k) | `insert_batch`: 0.438 / 0.426 s (228k to 235k) | **1.75x** over karez per-row; **1.05x to 1.08x** over sqlx |
| latency floor | karez/floor p50: S1 0.97, 1.02, 1.03; S2 1.02, 1.04, 1.05 | | at the floor |

**What this shows.** The API reproduces the prototype: S4 and S8 stop improving at k=4 because the 1.9 Gbit/s link is full (S8 9.6 s
is 2.3 GB at 1.9 Gbit/s), and a shared snapshot costs nothing measurable (S8 k=8: 9.65 s with it, 9.60 s without). The partitioned
scan is 1.12x (S4) and 1.23x (S8) over the best single-connection client on this network class. `insert_batch` is 1.75x karez's own per-row path and
level with sqlx. k=2 on S8 is again no better than one connection (11.98 s against 12.0 s for tokio-postgres): it was seen on a third pair and in WSL; section 14 investigates it and finds no systematic problem.

**State against the criteria of section 5**, unchanged by this run: OLTP level, latency at the floor, client CPU about half on
one connection (S8: karez 7.0 to 7.7 CPU-s, tokio-postgres 14.0 to 14.7, pgx 18.7 to 19.3; the partitioned scan uses 11 to 18 CPU-s because
more connections do more work in total), bulk reads 1.1x to 1.25x on this network and 2.4x to 3.4x with no network limit, bulk
inserts level with sqlx. The deciding measurement is still a network faster than 1.9 Gbit/s, which needs the DigitalOcean tier
raised (section 9, option 1).

## 14. The k=2 question (2026-11-16)

Question: why did two partitions give no gain over one connection on S8 in some runs? The checks, in the order done:

1. **Plans (PostgreSQL 18, WSL).** The plain scan is a `Seq Scan`; each half by `ctid` is a `Tid Range Scan` (cost about half of the
   whole), with the TID condition applied. No index scan, so the first hypothesis (a half-range turning into an index scan) is
   **wrong**. `max_parallel_workers_per_gather` is 2, but neither query uses a parallel plan: `parallel_tuple_cost` (0.1) keeps a
   `SELECT` of 13M rows serial. So one connection is one backend, and the second hypothesis (the plain scan is already parallel) is
   also **wrong**.
2. **The cache state decides the WSL numbers.** The same plain scan took 24.3 s cold and 6.5 s warm in `EXPLAIN ANALYZE` (the table is
   1.8 GB, `shared_buffers` 512 MB, WSL has 5.9 GB of RAM): every earlier WSL run that moved by tens of percent between repeats was
   mixing cold and warm pages.
3. **A controlled probe** (`bench/rustbench/src/bin/partprobe.rs`; warmed cache; whole table, each half alone, both halves at once, quarters;
   three rounds; `[measured here, WSL]`): whole table on one connection 15.8 to 16.0 s; each half alone 7.9 to 9.6 s; **both halves on
   two connections 10.1 to 11.1 s (1.5x to 1.6x)**; quarters on four connections 5.6 to 6.2 s (2.6x to 2.9x). Scaling from one to two to
   four connections is close to proportional; the second connection costs about 10% over the ideal.
4. **On droplets the wall explains the rest.** One connection already moves 2.3 GB in about 12 s = 1.5 Gbit/s, and the link carries
   1.9 Gbit/s (9.65 s for 2.3 GB). The most two connections can gain there is 12 / 9.65 = 1.24x. The k=2 results on droplets were:
   9.89 s (second pair, prototype), 11.3 s (first pair, prototype) and 11.98 s (third pair, API), against single connections of
   12 to 14 s: gains from 1.0x to 1.4x, around the possible 1.24x and inside the run-to-run spread (about 15%).

**Conclusion: there is no k=2 defect.** The "anomaly" was cold-cache noise in WSL and a ceiling of 1.24x on the droplets. I withdraw
the claim that it is real and unexplained. It is still sensible that the default partition count is the client's CPU count capped
at 8: on the 1.9 Gbit/s link a second connection adds little, and from four on the link is full.

## 15. Pool variant and MSRV recheck (2026-11-16)

- **`scan_partitioned_pool(&Pool, &Scan, init, on_chunk)`** takes its connections from a `Pool` and gives them back, so a warm pool costs no
  connection set-up per scan. The partition count is capped at `pool.max_size()` (a scan needs that many connections at once); a
  connection that is not free within 30 seconds fails the scan with an error, not a deadlock. The scan code treats an owned and a
  pooled connection alike through a private trait; `Pool::max_size()` is new. A connection that fails is discarded by the pool as
  before; one left inside the snapshot transaction is rolled back by the pool's drop.
- **Test** (`scan_partitioned_pool_reuses_connections_and_leaves_them_clean`, real server): 8 partitions asked for on a pool of 4 reads
  every row once with 4; the pool stays at 4 connections over three scans (with and without snapshot) and all are idle afterwards; a missing
  table (42P01) and a missing column (42703) return the connections, which are not in a transaction; a pool of one scans on its one
  connection. It passes on PostgreSQL 14, 16 and 17.
- **MSRV 1.85 rechecked.** Rust 1.85.0 installed in WSL (`rustup toolchain install 1.85.0 --profile minimal`): `cargo +1.85.0 build` of the three core
  crates, the whole `karez-chunk`, `karez-proto-pg` and `karez-pg` test suites against PostgreSQL 16 (47 integration tests among them) and
  `cargo +1.85.0 check --workspace --all-targets` all pass. `cargo fmt` and `cargo clippy` (on the current stable) are clean.
- **Not measured:** the saving from the pool on a droplet. In WSL a SCRAM connection takes a few milliseconds, so it only shows on
  tables that scan in tens of milliseconds; the number belongs in the next droplet run, not here.

## 16. Closing the open items (2026-11-16, owner: go ahead with the current numbers, park the faster network)

| item | result |
|---|---|
| top-level `karez` crate | already re-exports everything of `karez-pg` (`pub use karez_pg::*`); added doc examples for `scan_partitioned` and `insert_batch` (compile-checked doctests, pass on stable and on 1.85.0) |
| `execute_many` deadlock fix on Linux | the mock-server test `execute_many_does_not_deadlock_when_the_server_stops_reading` passes on Linux (WSL), six runs in a row, and the other seven mock-server tests pass |
| sqlx S8 slowness | **explained**: the adapter fetched through the pool, and sqlx held about 5.9 GB for a 2.3 GB result (23.6 s per pass in WSL); on one acquired connection it streams in 5 MB, 17.5 to 19.1 s, same checksum. Adapter fixed (S4 and S8). The 2026-11-14 campaign's sqlx S8 never finished for that reason (an 8 GB droplet paging) |
| pgbench cross-check | run in WSL: pgbench shows a steady 50 to 70 us (about 20%) higher p50 than the libpq adapter at low load, for S1 and S2 alike and for any pgbench thread count; p99 within about 25%; at 8,000 per second the shared WSL CPUs make both useless. The adapter does not inflate latency; the offset is pgbench's own overhead (probably, not proven) and cancels between clients. `crosscheck.py` got `--scenario`, `--jobs` and a warm-up drop. Droplet rerun parked with the faster-network test |
| binary size and dependencies (plan 1.3 row) | **met**: 441 KiB (size-optimised) / 483 KiB (`opt-level=3`) against the 600 KB gate; zero third-party crates; tokio-postgres 824 / 940 KiB with 68 crates, sqlx 1,060 KiB with 147 crates |
| plan, README, STATUS | updated to the measured state: plan section 1.3 has the gate table (stop rule neither triggered nor cleared), the README status no longer says "nothing is measured", STATUS records the owner's decision |
| MSRV, pool variant | done earlier today (section 15) |

**Still open** (none blocks the rest): the faster two-machine network test (parked by the owner); the pgbench check and the pool-saving
number on a droplet; diesel and rusqlite hello-world sizes; a full campaign 2 (needs the owner's go-ahead); crate-name reservation and the
audit budget (owner decisions); pushing the branch (not done: nothing is pushed unless the owner asks).
