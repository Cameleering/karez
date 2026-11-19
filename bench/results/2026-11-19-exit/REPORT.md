# Exit measurements of M1 step 9: S6 against PgBulkInsert and bulk reads over TLS

Run of 2026-11-19, one lane, **shared-CPU droplets** (see "Conditions"). Every number is `[measured here]`. The tables the harness wrote are in
`lane1/out/lane1-round1-exit/report.md`; the raw cells are in `cells.jsonl.gz` next to it (unpack with `gunzip -k`). The cells carry the round label `x1`.
This run is **not comparable with the `c-4` campaigns** (campaign 2, round 3 and its tail run): another droplet class, another database version
(PostgreSQL 18.7), n=3 passes per cell and one lane. Do not rank its numbers against theirs.

## Conditions

| | |
|---|---|
| droplets | `s-4vcpu-8gb` (shared CPU, 4 vCPU, 8 GB), two, in `blr1`, one client and one PostgreSQL 18.7 host on the private network, 77 minutes; Ubuntu 24.04, kernel 6.8 |
| why not `c-4` | DigitalOcean reported `c-4` as unavailable in `blr1` that day (`Size is not available in this region`); the owner chose the shared class in chat |
| tree | commit `2d3ee6b` (the `bench`, `crates` and root manifests were shipped from the working tree, which had no other change there); adapters built on the client droplet: Rust 1.97.1, Go 1.27.4, Java 21, libpq 18 |
| method | closed loop, one untimed warm-up pass and 3 timed passes per cell, median of the 3; S4 1,000,000 rows x 10 columns, S5 100,000 rows in one transaction, S6 COPY of 10,000,000 rows; TLS cells use `sslmode=verify-full` with the database's throwaway CA |
| correctness | every cell's row count and (S4) checksum matched; the open-loop `verify` of S1 to S3 matched on 8 clients (its taxi scenarios failed because the taxi table was not loaded here: it is not part of this run) |
| database CPU | 20% to 32% of the four vCPUs during the S6 cells: about one backend busy out of four, so S6 is limited by the single backend that does the insert |

## S6: karez against PgBulkInsert (the plan's gate: at least 1.5x)

PgBulkInsert 8.1.4 writes **binary** COPY through pgjdbc. The `karez-blocking` adapter writes **text** COPY (the format of the harness's row recipe; karez has no
binary COPY writer yet). The contents of the target table after a run were compared with `md5(string_agg(t::text, ',' order by id))` against the pgjdbc text COPY on a local server: identical.

| client | rows/s (median of 3) | passes (s) | CPU-s per M rows | peak RSS MB |
|---|---:|---|---:|---:|
| PgBulkInsert (binary COPY) | 357,862 | 27.8, 27.9, 28.0 | 2.23 | 1,935 |
| karez-blocking (text COPY) | 318,889 | 29.3, 31.4, 32.5 | 1.48 | 6 |
| sqlx | 318,873 | 31.3, 31.3, 33.3 | 1.61 | 8 |
| libpq | 304,512 | 29.8, 32.8, 38.5 | 2.40 | 10 |
| pgx | 304,094 | 30.8, 32.9, 35.0 | 3.20 | 17 |
| tokio-postgres | 303,380 | 31.2, 33.0, 36.4 | 2.35 | 7 |
| pgjdbc (defaults) | 296,378 | 32.9, 33.7, 41.9 | 2.74 | 1,962 |
| pgjdbc (tuned) | 295,926 | 33.1, 33.8, 38.3 | 2.80 | 1,953 |
| asyncpg | 257,472 | 38.3, 38.8, 39.2 | 3.40 | 31 |
| psycopg3 | 171,758 | 55.6, 58.2, 59.2 | 5.24 | 41 |

- **karez / PgBulkInsert = 0.89x in rows per second** (the fastest karez pass, 29.3 s, is slower than the slowest PgBulkInsert pass, 28.0 s: the passes do not overlap). **The plan's gate of at least 1.5x is not met on this droplet class.**
- karez uses **0.66x the CPU** of PgBulkInsert per million rows and about 1/300 of its memory.
- Eight of the ten clients are between 296,000 and 358,000 rows/s, and the database had one busy backend. That points at the server's insert rate as the limit on these droplets, not at the clients. It does **not** show that the gate is out of reach: in
  campaign 2 on `c-4` droplets (2026-11-16, another droplet pair) karez wrote 3,000,000 rows at 736,214 rows/s, more than twice the rate here, so the server was not the limit there. The gate needs a rerun on `c-4` droplets with PgBulkInsert next to karez. **Open.**
- A binary COPY writer in karez would be the first thing to try for S6: binary COPY costs the server less parsing, which is the likely reason PgBulkInsert leads here. Not built.

## S6 over TLS (`sslmode=verify-full`)

karez 307,412 rows/s against PgBulkInsert 322,547 (**0.95x**; the PgBulkInsert passes were 28.1, 31.0 and 34.1 s, so the two overlap). TLS cost karez 4% of its plain S6 rate, and the others between a gain of 4% and a loss of 14%: S6 is
server-bound, so TLS hardly shows.

## Bulk reads (S4) over TLS against plain, same droplets

| client | plain rows/s | TLS rows/s | TLS / plain | CPU-s per M rows, TLS / plain |
|---|---:|---:|---:|---:|
| karez-blocking (row view) | 642,343 | 488,043 | 0.76 | 1.16 |
| karez-blocking-chunk (chunk path) | 578,269 | 546,538 | 0.95 | 1.04 |
| tokio-postgres | 676,773 | 535,504 | 0.79 | 1.13 |
| pgx | 713,770 | 565,949 | 0.79 | 1.44 |
| libpq | 621,234 | 488,830 | 0.79 | 1.19 |
| sqlx | 623,558 | 402,139 | 0.64 | 1.16 |
| pgjdbc (tuned) | 504,796 | 490,436 | 0.97 | 1.07 |
| asyncpg | 280,355 | 212,481 | 0.76 | 1.34 |
| psycopg3 | 232,997 | 193,043 | 0.83 | 1.08 |

- Over TLS every client loses between 3% and 36% of its S4 rate. karez's row view loses 24%, in the middle of the others; its chunk path loses 5%.
- **karez is not ahead on S4 here.** On the plain scan pgx and tokio-postgres are 11% and 5% above karez's row view; over TLS pgx, the karez chunk path and tokio-postgres are above it. The earlier `c-4` campaigns found karez level with tokio-postgres, pgx and sqlx on single-connection scans; this
  run agrees on "level, within a few percent" and on nothing more (n=3, a shared CPU, the spread of single passes is up to 30%: libpq 29.8 to 38.5 s on S6).

## S5 (batch insert), plain and TLS

karez-blocking 51,602 rows/s plain and 42,669 over TLS; sqlx 88,261 and 86,445; libpq 61,740 and 56,357; asyncpg 67,467 and 67,399; tokio-postgres 25,002 and 27,608. **karez is fifth of eight on S5 in this run**, behind sqlx, pgjdbc (tuned), asyncpg and libpq. The `Connection::insert_batch`
multi-row path (adapter `karez-blocking-mrow`) was not part of this run. S5 is the one bulk scenario where karez is not at the top or level with it; earlier campaigns showed the same direction.

## Open loop (S1, S2, S3, S8, plain and over TLS): no result

The open-loop part of the `exit` profile **found no throughput that meets the 2 ms p99 bound on these droplets**: for `karez-blocking`, `tokio-postgres` and `sqlx` on S1, S1 over TLS and S2 at 64 connections (and for `pgx`, which had only started) the p99 of the probes stayed between 3.4 and 36 ms from
2,000 requests a second down to 16 requests a second (the harness steps the rate down until it passes, and never did). The harness then gave up on each client after three configurations in a row without a result, as designed, and the
run was stopped there to save time. The cause is the droplet class (a shared CPU and a shared network show a latency tail an order of magnitude above the dedicated `c-4` pair, which met the same bound at tens of thousands of requests a second); it was not investigated further.
**S1 to S3 and S8 over TLS are therefore not measured. They need the `c-4` class.** The harness part is built and was run locally (WSL) to check that the labels and the TLS switch work (`s1t`, `s3t`, `s8`, `s8t` in profile `exit`).

## What this run supports and what it does not

Supports: TLS bulk reads and COPY writes work in all nine bulk clients and cost karez about the same as the others; on S6 on shared droplets karez is at 0.89x of PgBulkInsert's rows per second with two thirds of its CPU and almost none of its memory, with the database as the limit.
Does not support: any speed claim, the plan's S6 gate in either direction, S1 to S3 and S8 over TLS, the S8 wait-time distribution against the Erlang C prediction (not attempted; it needs the open loop), or anything about `c-4`.

Cost: two `s-4vcpu-8gb` droplets for 77 minutes, at DigitalOcean's list price of $0.0714 an hour each about $0.18. Both were deleted by the campaign script at the end; the safety net had been armed for 4 hours and was disarmed.
