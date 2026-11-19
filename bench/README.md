# karez benchmark harness

The harness that measures karez against the PostgreSQL clients listed in
[plan.md section 6.3](../docs/design/plan.md). It follows the plan's method (section 6) and the
campaign design of the simoom project ([bench/cloud](cloud) is adapted from simoom's scripts).

**Status (2026-11-16): built and tested in WSL; a reduced campaign (2026-11-14) and a two-round campaign (2026-11-16) have run on DigitalOcean** (the section "Campaign log" at the end says what it covers and what it does not). The numbers
anyone may quote come from the campaign on DigitalOcean, not from WSL. The WSL runs only prove that
every adapter works. They share one CPU between the client and the database and say nothing about
speed.

## What is measured

| scenario | what | harness |
|---|---|---|
| S1 | `SELECT 1` | open loop, `run.py` |
| S2 | point select by primary key, one 10-column mixed row | open loop |
| S3 | 12-row select with a string column (Fortunes-like) | open loop |
| S7 | S2 over TLS (`sslmode=verify-full`) | open loop, `--url-tls` |
| S8 | S2 at 512 workers with a pool of 32 | open loop, `--pool 32` |
| R1 | **real data**: point select by primary key on `taxi_trips`, 17 columns, NULLs, two timestamps | open loop |
| R2 | real data: 100 consecutive rows by primary-key range | open loop |
| R3 | real data: aggregate (`count`, `max`, `max`) over one pickup zone, server-bound | open loop |
| R4 | real data: mixed workload, 80% R1 and 20% R2, per request at random (campaign 1 had 5% R3 in the mix) | open loop |
| S4 | scan of 1,000,000 rows x 10 columns (karez: row view and chunk path) | closed loop, `run.py bulk` |
| S5 | batch insert of 100,000 rows, prepared, one transaction | closed loop; each client's own best path |
| S6 | COPY of 10,000,000 rows (text, binary where the client's fast path is binary) | closed loop |
| S9, S10 | partitioned parallel scan (ConnectorX), ADBC and JDBC exports | **not built yet** (see "Not built") |

Per configuration the harness searches the **SLO throughput**: the highest arrival rate at which the
p99 latency stays under the scenario's bound (S1, S2, S7: 2 ms; S3: 3 ms; fixed before measuring,
the same for every client). It then runs `repeats` full-length runs at that rate and reports the
median p50, p99, p99.9, CPU-seconds per million requests and peak RSS. A configuration whose median
p99 is over the bound is stepped down 3% until it passes. Nothing is best-of-N.

## The real dataset

`taxi_trips` is loaded by [data/load_taxi.py](data/load_taxi.py) from the public NYC Taxi and Limousine
Commission yellow-taxi trip records (monthly parquet files from
https://www.nyc.gov/site/tlc/about/tlc-trip-record-data.page): January to April 2024, about 13 million
rows, 17 columns (int2, int4, int8, float8, text, two `timestamp`), with the source's NULLs and its odd
values (negative fares, 2002 timestamps) kept. `trip_id` numbers the rows 1..N in file order, with a
primary key and an index on `pu`. The data is downloaded on the database droplets; it is not stored in
this repository. S1 to S8 use the synthetic tables of [sql/schema.sql](sql/schema.sql); R1 to R4 use the
real table, so every client decodes the same real values, NULLs and timestamps.

**Correctness check before any number is taken:** `python3 run.py verify` runs every client for one
worker on the same request sequence (every adapter uses the same generator and seed) and compares the
checksum of all values read. On PostgreSQL 18 in WSL, with 2,964,624 rows of January 2024 loaded, every client that
implements a scenario produces the identical checksum: 15 clients on S2 and S3, 14 on R1 to R4 (zero-postgres
has no taxi adapter: its tuple decoder stops at 12 columns) (2026-11-14, `[measured here]`). The campaign runs
`verify` first on every lane and keeps its output (`verify.txt`).

## Method (what makes the numbers comparable)

- **Open loop.** A generator thread issues arrivals at a fixed rate on a fixed schedule. Latency is
  measured from the *intended* start time, so a slow server cannot hide its queueing (coordinated
  omission corrected). Overdue arrivals are emitted in a burst, not skipped.
- **Same arrival model for every client.** Rust and Go adapters take arrivals from a queue that
  one generator thread feeds. Python, Java and C adapters do the same in their language.
  `--dispatch shared` (default): all workers draw from one queue, like a pool. `--dispatch perconn`:
  one queue per connection, which is what `pgbench -R` does; used by `crosscheck.py`.
- **Unpipelined and pipelined are separate tables.** `--mode unpipelined`: one request at a time per
  connection. `--mode pipelined`: several requests in flight on one connection (karez-rt,
  tokio-postgres, vertx-pg-client). TechEmpower banned cross-request pipelining for a reason;
  readers must see both.
- **Prepared statements, native types.** Every adapter opens its connections and prepares its
  statement before the clock starts, reads every column of every row in its native type, folds it
  into a checksum, checks the row count (and for S2 the row id), and counts a mismatch as an error.
  An adapter may not skip decoding.
- **Latency histogram.** 64 buckets per power of two (at most 1.6% error), identical in every
  language ([hist.rs](rustbench/src/hist.rs)); adapters print the sparse bucket list, the
  orchestrator computes the percentiles.
- **CPU is measured inside the window.** The orchestrator reads `/proc/<pid>/stat` when the adapter
  prints `T0` on stderr plus the warm-up, and again just before the window closes, so connection
  setup and warm-up are not counted. With `--db-host` it samples the database host's `/proc/stat`
  over ssh the same way.
- **Warm-up.** 3 s by default; JIT clients (`warmup_min` in `clients.json`) get 30 s in final runs
  and 15 s in probes (the plan asks for 60 s on the JVM; this is the budgeted value and is stated
  in the report).
- **Servers.** PostgreSQL 18 on its own host, the clients on another, same VPC, the same
  configuration and dataset for every client ([setup-db.sh](cloud/setup-db.sh)). PostgreSQL 18
  because zero-postgres 0.9 requires protocol 3.2.
- **Lanes and rounds.** Three lanes, each a database host and a client host. karez runs in every
  lane and every round; the other clients rotate across lanes, so each client meets each pair of
  droplets. A ratio is only ever computed between rows of the same lane and round. Absolute numbers
  are shown with their lane.
- **Defaults and tuned.** Where a client's defaults cost something it can switch off (pgJDBC
  prepare threshold, sqlx's ping on acquire), both variants run and both are published
  (`pgjdbc`/`pgjdbc-tuned`, `sqlx`/`sqlx-tuned`).
- **Raw data first.** Every cell appends its full adapter output (including both histograms) to
  `cells.jsonl`. `run.py report` builds the tables from the raw lines; nothing is averaged away
  or overwritten.

## Clients

| client | language | modes | pool | TLS | notes |
|---|---|---|---|---|---|
| karez-blocking | Rust | unpipelined | karez `Pool` | yes | the blocking `Connection` |
| karez-shared | Rust | both | no | yes | `karez-rt` shared connection, runtime-free |
| tokio-postgres | Rust | both | no | yes | prepared once per connection; pipelined = one client, many tasks |
| deadpool-postgres | Rust | unpipelined | yes | not yet | `prepare_cached`, recycling `Fast` |
| sqlx, sqlx-tuned | Rust | unpipelined | yes | yes | tuned: no ping on acquire |
| zero-postgres | Rust | unpipelined | no | not yet | tokio flavour, needs PostgreSQL 18 |
| libpq | C | unpipelined | ad-hoc list | yes | binary format, `PQexecPrepared` |
| pgx | Go | unpipelined | pgxpool | yes | statement cache |
| asyncpg | Python | unpipelined | yes | yes | on uvloop |
| psycopg3 | Python | unpipelined | yes | yes | binary, prepared, `psycopg_pool` |
| pgjdbc, pgjdbc-tuned | Java | unpipelined | HikariCP | yes | tuned: `prepareThreshold=1`, statement cache |
| vertx-pg-client | Java | both | yes | yes | pipelining limit 1 or 256 |
| r2dbc-postgresql | Java | unpipelined | r2dbc-pool | yes | |

## Not built (be honest in any report)

- S9 (ConnectorX-style partitioned scan) and S10 (ADBC and JDBC exports): karez has no exports yet.
- Bulk adapters for deadpool-postgres and zero-postgres; S6 for vertx-pg-client (no COPY support)
  and r2dbc-postgresql; S5 for r2dbc-postgresql. The report lists failures and gaps.
- The S4 columnar comparison against a real Arrow consumer: the "chunk path" folds columns straight
  over karez's own Arrow-layout buffers; no other client has an equivalent, so its row is shown next
  to the row-view rows of the same lane and labelled as a different access path.
- Competitors from plan 6.3 without an adapter: wtx, diesel (and diesel-async), ConnectorX, MySQL,
  SQL Server and SQLite clients (karez has no driver for them yet), vertx pipelining variants beyond
  the one above, a GraalVM native build of the JVM clients.
- Rust size and dependency floor of diesel and rusqlite hello-worlds (M0 item). tokio-postgres, sqlx and karez were measured on 2026-11-16 (docs/design/plan.md section 1.3).
- Allocations per row, bytes on the wire, `perf stat`.
- A JVM warm-up of 60 s and 300 s runs (shortened for the budget, see above).
- `deadpool-postgres` and `zero-postgres` over TLS.

## Run it

Local (Linux or WSL), against a throwaway PostgreSQL:

```sh
docker run -d --name karez-test-bench-pg -p 127.0.0.1:55432:5432 -e POSTGRES_USER=bench \
  -e POSTGRES_PASSWORD=bench_pw_local -e POSTGRES_DB=bench postgres:18 -c max_connections=1000
docker exec -i karez-test-bench-pg psql -U bench -d bench -v ON_ERROR_STOP=1 < sql/schema.sql
cargo build --release                                   # in bench/, builds rustbench
(cd adapters/go && go build -o karez-bench-go .)        # Go 1.22 or newer
(cd adapters/java && mvn -q -B package -DskipTests)     # JDK 21
(cd adapters/c && cc -O2 -o libpq_adapter libpq_adapter.c -I$(pg_config --includedir) -L$(pg_config --libdir) -lpq -lpthread)
python3 -m venv ~/venv && ~/venv/bin/pip install asyncpg "psycopg[binary,pool]" uvloop
export BENCH_URL='postgresql://bench:bench_pw_local@127.0.0.1:55432/bench?sslmode=disable'
python3 run.py campaign --profile smoke --out /tmp/out        # a few minutes, proves the machinery (OLTP and bulk)
python3 run.py bulk --out /tmp/out --s6-rows 1000000          # the bulk scenarios on their own
python3 run.py cell --client karez-blocking --scenario s2 --conns 8 --rate 5000 --out /tmp/out
python3 crosscheck.py --pgbench pgbench --rates 2000,8000     # method check against pgbench -R
```

`karez-test-*` is the naming rule for throwaway containers in this repository. WSL stops idle
containers when the last `wsl.exe` session ends, so run long jobs inside one `wsl.exe` command.

### Adapter contract

Every adapter takes the same flags (`--client --url --scenario s1|s2|s3 --mode --conns --pool --rate
--duration --warmup --dispatch --rows --rt-threads --root-cert`), prints `T0` on stderr when its
clock starts, and prints one JSON line on stdout with `client version scenario mode conns pool rate
duration_s warmup_s sent ok errors dropped checksum first_error wall_s cpu_user_s cpu_sys_s rss_kb
hist service_hist`. `hist` is the latency from the intended start, `service_hist` from the actual
send, both as `[[bucket, count], ...]`. A new client is a new adapter plus an entry in `clients.json`.

## The campaign on DigitalOcean

Scripts in [cloud/](cloud): `droplets.sh` (create and delete, by tag `karez-bench` only),
`safety-net.sh` (deletes everything at a hard deadline, even if the operator is gone; adapted from
simoom, which lost droplet-hours twice without it), `setup-db.sh`, `setup-client.sh`, `lane-run.sh`
and `ctl.sh` (the operator). Token: a file read only by these scripts, never printed
(`DO_TOKEN_FILE`). Six `c-4` droplets in `blr1` (4 dedicated vCPU, 8 GB), $0.125 per hour each,
`[vendor, DigitalOcean API, checked 2026-11-14]`, so $0.75 per hour for the set.

```sh
bash bench/cloud/ctl.sh up              # create, arm the safety net, set up every node
bash bench/cloud/ctl.sh run 1 full      # round 1 on all lanes
bash bench/cloud/ctl.sh status ; bash bench/cloud/ctl.sh tail 1
bash bench/cloud/ctl.sh pull            # results to bench/results/<date>-campaign1/
bash bench/cloud/ctl.sh down            # final pull, delete droplets and key
```

## Reporting rules

1. Every number carries its evidence tag and its lane. WSL and droplet numbers never share a table.
2. One table per scenario with every client in it, losses included, and the ratio to karez computed
   per lane and round (the median of the per-round ratios is the published figure).
3. The owner chose "both" (2026-11-14): raw files and a script-built report always, and headline
   numbers may be published after the campaign. A headline in the README or STATUS appears only
   next to a link to the full table, with its tag, lane count and the losses stated in the same
   place. No headline on the About line or in images. "Fastest" and similar words are never
   written; the table says it or it does not.

## Campaign log

### 2026-11-14, campaign 1 (aborted) and the lean round

Results: `bench/results/2026-11-14-lean/` (raw cells and `REPORT.md`). Setup: three lanes, each a database droplet and a
client droplet, DigitalOcean `c-4` (4 vCPU, 8 GB, Intel Xeon Platinum 8280) in `blr1`, PostgreSQL 18.7 with TLS
available, private network between the two droplets. `[measured here, 2026-11-14]`.

What happened, in order:

1. The smoke round passed on all three lanes.
2. The first real round (profile `full`) was cut short. The kernel's out-of-memory handler killed PostgreSQL on every
   database host (04:22Z, 05:05Z and 05:10Z) because up to about 1000 backends each mapped the 2 GB shared buffer with
   4 kB pages. The clients kept running against dead servers. Everything from those moments is invalid. The partial raw
   data is kept in `bench/results/2026-11-14-campaign1-aborted-oom/` and is not used.
3. Fix: huge pages on the database hosts (`vm.nr_hugepages=1300`, `huge_pages = on`). Nothing else in the database
   configuration changed. No further kill happened.
4. The time left did not fit a full round, let alone three. The profile `lean` (run.py) was run once: 11
   configurations, 3 repeats, 10 s runs, no 256-connection mix, S6 with 3M rows. **One lane per client, one round.**
   There was no rotation of clients across lanes and no repeat rounds, so every competitor row except the two karez rows
   has n=1. The karez rows have n=3 (one per lane). The droplets lived 657 minutes and were deleted by the campaign script.

What the lean round does not cover or cannot support:

- Lane 3 was still in its bulk phase when the time limit hit: S8 is missing for sqlx, sqlx-tuned, pgjdbc-tuned and
  r2dbc-postgresql; pgjdbc (defaults) failed S8 as expected (it buffers the whole result). **The sqlx S8 slowness is explained (2026-11-16):** the
  adapter fetched through the pool (`fetch(&pool)`), and sqlx then held about 5.9 GB of the 2.3 GB result in memory (23.6 s per pass in
  WSL); on an 8 GB client droplet that means paging. Fetching on one acquired connection streams it with 5 MB of memory in 17.5 to
  19.1 s with the same checksum. The adapter now acquires a connection for S4 and S8; the campaign numbers of 2026-11-14 predate that
  and sqlx's S4 row is unaffected in kind (127 MB fits in memory), but its memory column was high.
- R3 (aggregate over 13M rows) met no p99 bound for any client, and R4 (mix with 5% aggregates) is dominated by that
  query. Neither tells the clients apart.
- Many rows are at the same rate (for example 46,930 or 32,000 per second): the search moves in steps and the database
  host is likely the limit there. Differences under about 5% are inside one step.
- One run per client per lane is noisy at one connection: tokio-postgres and deadpool-postgres share a driver and gave
  5,000 and 8,000 per second in S2 at one connection on different lanes. Read the per-lane ratio column and treat n=1 as
  indicative only.
- The pgbench cross-check (`crosscheck.py`) was not run on the droplets. It was run in WSL on 2026-11-16 (S1 and S2, 8 connections,
  warm-up dropped from the pgbench log, pgbench `-j` 2, 4 and 8): at low load (1,000 to 2,000 requests per second) **pgbench reports a p50
  of 300 to 345 us against 255 to 265 us for the libpq adapter, a steady offset of 50 to 70 us (about 20%)**, the same for S1
  (one tiny column) and S2 (ten columns, so not the text-against-binary format) and independent of pgbench's thread count. p99 agrees
  within about 25% (S1: 520 to 550 against 395 to 465 us). At 8,000 per second both tools show spikes on the shared WSL CPUs and the
  comparison is not usable. So the adapter does not inflate latency; the offset is probably pgbench's own event loop and timing (not
  proven). It is a constant that cancels when clients are compared on the same harness. Rerun on a droplet pair when the next run happens.
- No client other than the karez rows ran twice, so no confidence interval exists.

Reading the tables in `REPORT.md` as they are: karez-blocking is level with the best clients in S1/S2/R1/R2 at 64 and 512
connections (differences within one step), ahead of pgjdbc, sqlx, vertx-pg-client, r2dbc-postgresql, asyncpg and
psycopg3 there, and not ahead of tokio-postgres, libpq, pgx or deadpool-postgres. In the bulk scans S4 it is level with
pgx and sqlx; in S5 sqlx (205k rows/s), asyncpg and libpq are ahead of karez (123k); in S6 and S8 karez is first by a
small margin. These are single-lane figures and not a ranking; no speed claim for karez follows from them.


The raw cells are stored gzipped (`cells.jsonl.gz`); gunzip them next to the file to rerun `python bench/report.py bench/results/2026-11-14-lean`.


### 2026-11-16, campaign 2

Results: `bench/results/2026-11-16-campaign2/` (gzipped raw cells, `REPORT.md`, campaign logs). Setup as in campaign 1 (three lanes of two
`c-4` droplets, `blr1`, PostgreSQL 18.7 with huge pages on) plus the harness fixes of 2026-11-16. A smoke round (07:40Z) and then two
rounds of the profile `lean2` (08:14Z to 16:15Z), 14 clients with the karez variants in every lane and the other clients rotated across
lanes. Six droplets lived about 8.6 hours and were deleted by `campaign.sh` (the safety net was disarmed). `[measured here, 2026-11-16]`.

What it supports: the karez rows have n=6 and every other row n=2 (two lane-rounds), ratios are per lane and round and shown with their
range, all bulk checksums match, no database restart, no lane aborted, no `MISMATCH`. The numbers and their reading are in
[docs/design/perf-program.md](../docs/design/perf-program.md) section 17. In short: level with the best clients on point queries (about 1.2x
tuned pgJDBC at 64 connections); level on single-connection bulk reads; the partitioned scan 1.3x (1M rows) and 1.5x (13M rows) the best
other client on a 1.9 Gbit/s link; `insert_batch` ahead of every other client; the shared pipelined connection behind the blocking client.

Limits: default pgjdbc ran out of memory on S8 in both rounds (it buffers the result; a result, not a harness fault); competitors have n=2,
so the ranges are wide; the S8 rate of the partitioned scan is the same in all lanes, which points at the link, not proven here; the
pgbench cross-check was not rerun on droplets; diesel and wtx were not in the client set (adapters added 2026-11-18, perf-program.md section 18); TLS ran only as S7 (S2 over TLS, 64 connections), not for bulk reads; one profile only.
Rerun the report with `gunzip` of each `cells.jsonl.gz` and `python bench/report.py bench/results/2026-11-16-campaign2`.


### 2026-11-18, round 3 (partial) and the combined report

Results: `bench/results/2026-11-18-round3/` (gzipped raw cells for each lane, `REPORT.md` that combines campaign 2 and round 3: nine lane-rounds, 10,645 raw cells, and `smoke-checks/` with the
`run.py verify` and pgbench cross-check outputs of the smoke round). Same setup as campaign 2 (three lanes of two `c-4`, `blr1`, PostgreSQL 18.7), one more `lean2` round with the client groups
rotated again, plus diesel and wtx in every lane. Droplets up 02:22Z, round 3 03:21Z to 07:45Z, deleted 07:46Z (about 5.4 hours, about $4). `[measured here, 2026-11-18]`.

What it supports: karez n=9, rotated competitors n=3 (95% t interval shown from n=3), diesel and wtx n=3, all `verify` checks and checksums matched. Numbers and reading:
[docs/design/perf-program.md](../docs/design/perf-program.md) section 19. In short: wtx is level with karez-blocking on S1 and S2; diesel is 0.4 to 0.7 of it; the partitioned scan is 1.31x the best other client on S4
(range 0.96 to 1.61, one loss) and 1.52x on S8; the pipelined shared connection is about 9% behind tokio-postgres, with an interval that includes parity.

Limits: the round was **cut at its stop time** (4 h 24 min), so bulk scenarios ran only on lane 2 and the last client of each lane has fewer scenarios; the pgbench cross-check ran only in the smoke round (two rates, 20 s per
cell); `sqlx-tuned` and `zero-postgres` stay at n=2; no TLS bulk reads; diesel has no R2/R3/R4 or S5/S6/S8; wtx has S1 and S2 only. Rerun: gunzip each `cells.jsonl.gz`, then
`python bench/report.py bench/results/2026-11-16-campaign2 bench/results/2026-11-18-round3` (the report is written next to the first root; the stored copy of the combined one is in the round 3 folder).



### 2026-11-18, the round 3 tail and the combined report of 11 lane-rounds

Results: `bench/results/2026-11-18-round3-tail/` (gzipped raw cells for lanes 1 and 3, the `run.py verify` output and pgbench cross-checks of each lane, and `REPORT.md` that combines campaign 2, round 3 and the tail:
11 lane-rounds, 11,776 raw cells; it supersedes the report in the round 3 folder). Run by `bench/cloud/lane-tail.sh` (the `tail` profile of `lane-run.sh`; `campaign.sh` takes `LANES="1 3"`): four `c-4` droplets in `blr1`
(lanes 1 and 3, PostgreSQL 18.7), the part of round 3 that the stop time cut off, with its own karez baselines (round label `3t`). DigitalOcean had no `c-4` in `blr1` from 10:19Z to 10:47Z, so the start waited.
Droplets up 10:47Z, run 11:00Z to 12:40Z, deleted 12:41Z (about 1.9 hours, about $1; the safety net was not needed). `[measured here, 2026-11-18]`.

What it supports: bulk scenarios on lanes 1 and 3, so karez has n=9 on bulk and n=11 on point queries, every rotated competitor n=3 (asyncpg S8 n=2); all `verify` checks and checksums matched; the pgbench cross-check
now has 20 cells (pgbench's p50 is 54 to 115 µs above the adapter's in every cell). Numbers and reading: [docs/design/perf-program.md](../docs/design/perf-program.md) section 20. In short: the partitioned scan is
1.33x the best other client on S4 (0.96 to 1.61, one loss in nine) and 1.65x on S8 (1.42 to 1.90); 1.9x to 2.0x tuned pgJDBC (n=3); single-connection scans are level with tokio-postgres, pgx and sqlx; the earlier
diesel S4 figure of 0.39 was a low outlier (three runs: 0.82).

Limits: no TLS bulk reads; diesel has no R2/R3/R4 or S5/S6/S8; wtx has S1 and S2 only; one profile (`lean2`); one region and one droplet class (the 1.9 Gbit/s link is the likely limit of the bulk reads, untested).
Rerun: gunzip each `cells.jsonl.gz`, then `python bench/report.py bench/results/2026-11-18-round3-tail bench/results/2026-11-18-round3 bench/results/2026-11-16-campaign2` (written next to the first root, so it
lands in the tail folder; do not give campaign 2 first, the report would overwrite that folder's `REPORT.md`).

### 2026-11-19, exit measurements of M1 step 9 (one lane, shared-CPU droplets)

Results: `bench/results/2026-11-19-exit/` (gzipped raw cells, `verify.txt`, the generated `report.md` and `REPORT.md`, which reads them). Run by `bench/cloud/lane-exit.sh` (the `exit` profile of `lane-run.sh`; `LANES=1 campaign.sh "exit"`): two `s-4vcpu-8gb` droplets in `blr1`
(PostgreSQL 18.7; DigitalOcean had no `c-4` in `blr1` that day), 08:53Z to 09:58Z, deleted by the script (77 minutes, about $0.18). Round label `x1`. `[measured here, 2026-11-19]`.

What it supports: S6 (10,000,000 rows) with a PgBulkInsert adapter, and S4 to S6 over TLS (`run.py bulk --bulk-tls`), n=3 passes per cell, all row counts and S4 checksums matched. In short: karez 318,889 rows/s and PgBulkInsert 357,862 on S6 (0.89x; karez at 0.66x the CPU), the
database as the likely limit; TLS costs every client's S4 rate 3% to 36% (karez row view 24%).

Limits: the shared droplet class (not comparable with the `c-4` campaigns); one lane; n=3; the open-loop S1 to S3 and S8, plain and over TLS, found no throughput under the 2 ms bound (p99 3.4 to 36 ms down to 16 requests a second) and were stopped, so they are **not measured**; the plan's S6 gate (1.5x) is
neither met nor ruled out and needs `c-4`.
Rerun: `gunzip -k` the cells, then `python bench/run.py report <folder>` on the folder that holds them.
After the run: client `karez-blocking-binary` (S6 through `Connection::copy_in_binary`, same rows) and the `pgbulkinsert` client are in `clients.json`; the binary client has not run on droplets.
