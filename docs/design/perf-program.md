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
