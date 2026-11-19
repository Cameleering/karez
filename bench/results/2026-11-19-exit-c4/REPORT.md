# Exit measurements of M1 step 9, rerun on `c-4` droplets in New York

Run of 2026-11-19, one lane, **two `c-4` droplets (dedicated CPU, 4 vCPU, Intel Xeon Platinum 8358, 8 GB) in `nyc1`**, PostgreSQL 18.7 on one and the clients on the other, private network, Ubuntu 24.04. Every number is `[measured here]`.
The tables the harness wrote are in `lane1/out/lane1-round1-exit/report.md`; the raw cells are in `cells.jsonl.gz` next to it (`gunzip -k`). Round label `x1`.
This follows the run on shared-CPU droplets in `blr1` ([../2026-11-19-exit/REPORT.md](../2026-11-19-exit/REPORT.md)): `c-4` was not available in `blr1` that day and was found in `nyc1` and `sfo2`. **It is not comparable with the earlier `c-4` campaigns in `blr1`**
(another region and another droplet pair; PostgreSQL 18.7 in both). n=3 timed passes per bulk cell and 3 confirm runs per open-loop cell; the SLO rates are steps of a search, so read differences under about 10% as noise.

| | |
|---|---|
| tree | commit `1096739` plus the `setup-db.sh` and `lane-exit.sh` changes of the next commit; adapters built on the client droplet (Rust 1.97.1, Go 1.27.4, Java 21, libpq 18) |
| method | open loop for S1 to S3 and S8 (p99 bound 2 ms, S3 3 ms), plain and over TLS (`sslmode=verify-full`); closed loop for S4 (1,000,000 rows), S5 (100,000 rows) and S6 (10,000,000 rows), plain and over TLS; median of 3 passes |
| correctness | every row count and S4 checksum matched; the `verify` of S1 to S3 matched on 8 clients (the taxi scenarios fail there: the taxi table is not part of this run) |
| database CPU | 16% to 29% of four vCPUs during S6 cells: about one backend busy; S6 is limited by the single backend that does the insert |
| cost | 199 minutes, two droplets at $0.125 an hour: about $0.83; deleted by the campaign script; the safety net (4 hours) was disarmed |

## S6, COPY of 10,000,000 rows: the plan's gate (at least 1.5x PgBulkInsert)

| client | rows/s (median of 3) | passes (s) | CPU-s per M rows | peak RSS MB |
|---|---:|---|---:|---:|
| **karez, binary COPY** (`Connection::copy_in_binary`) | **899,596** | 10.7, 11.1, 11.7 | 0.22 | 6 |
| sqlx | 703,180 | 14.1, 14.2, 14.4 | 0.76 | 8 |
| pgx | 690,955 | 14.4, 14.5, 15.8 | 1.52 | 17 |
| tokio-postgres | 678,956 | 14.5, 14.7, 14.9 | 0.81 | 8 |
| **PgBulkInsert** (binary COPY on pgjdbc) | **661,398** | 15.1, 15.1, 16.6 | 1.40 | 1,934 |
| karez, text COPY | 654,553 | 15.1, 15.3, 16.4 | 0.68 | 6 |
| libpq | 617,803 | 15.5, 16.2, 16.6 | 0.89 | 10 |
| pgjdbc (tuned) | 616,147 | 14.4, 16.2, 16.3 | 1.16 | 1,961 |
| pgjdbc (defaults) | 597,161 | 14.7, 16.7, 17.0 | 1.11 | 1,959 |
| asyncpg | 552,798 | 17.9, 18.1, 19.4 | 1.66 | 31 |
| psycopg3 | 366,549 | 26.7, 27.3, 27.5 | 2.56 | 40 |

Over TLS: karez binary COPY **1,010,121** rows/s (9.2, 9.9, 10.3 s) against PgBulkInsert **708,105** (13.7, 14.1, 15.4 s); karez text COPY 785,953.

- **karez binary COPY / PgBulkInsert = 1.36x plain and 1.43x over TLS in rows per second** (medians; pass by pass the ratio runs from 1.29x to 1.55x plain, and 1.33x to 1.67x over TLS). **The gate of at least 1.5x is not met in either case.**
- karez's text COPY is level with PgBulkInsert (0.99x), as in the shared-droplet run (0.89x there): binary COPY is what moves karez ahead, 1.37x its own text COPY.
- karez binary COPY uses **0.16x the CPU** of PgBulkInsert per million rows (6.4 times less) and about 1/300 of its memory.
- The database showed one busy backend in every cell (24% of the CPU for karez's binary run, the same as the others), and the two fastest results, 0.9 and 1.0 million rows a second, are the highest of any client. A gate of 1.5x on PgBulkInsert's 661,000 rows/s would need about 992,000 rows/s plain (and 1.06 million over TLS, where PgBulkInsert wrote 708,000), against the 900,000 and 1,010,000 karez reached. So the server's insert rate is probably the limit that keeps the ratio under 1.5x on this database, not the client. **That is a reading, not tested** (it would need a faster server or a second target table to see where the rate ends).

## Open loop (SLO rate at p99 under the bound, 64 connections unless said; median of 3 confirm runs)

| scenario | karez-blocking | tokio-postgres | pgx | libpq | pgjdbc (tuned) | sqlx | asyncpg |
|---|---:|---:|---:|---:|---:|---:|---:|
| S1 `SELECT 1` | 54,000 | 54,959 | 54,959 | 52,000 | 50,809 | 23,018 | 19,000 |
| S1 over TLS | 42,680 | 44,000 | 42,680 | 42,000 | 37,182 | 17,000 | 7,518 |
| S2 point select | 40,740 | 37,636 | 34,000 | 37,636 | 37,636 | 13,500 | 17,341 |
| S2 over TLS (S7) | 31,871 | 34,000 | 29,100 | 29,197 | 29,100 | 13,279 | 7,250 |
| S3 12 rows | 44,000 | 50,440 | 48,000 | 46,000 | 48,000 | 18,000 | 16,490 |
| S3 over TLS | 37,636 | 38,000 | 38,800 | 31,000 | 38,800 | 16,490 | 7,292 |
| S8 512 workers, pool of 32 | **10,000** | not run (no pool) | 41,400 | **42,000** | 27,471 | 17,000 | 5,932 |
| S8 over TLS | **9,500** | not run | 31,000 | **34,682** | 21,000 | 16,490 | 3,274 |

- On S1 to S3 and their TLS variants **karez, tokio-postgres, pgx, libpq and tuned pgjdbc are level within the noise of the search** (karez is 0.98x of the best on S1, 1.08x on S2, 0.87x on S3). sqlx and asyncpg are far below them, as in earlier campaigns. No ranking among the five follows.
- TLS costs every client about 20% of its S1 rate (karez 0.79x, tokio-postgres 0.80x, pgx 0.78x, libpq 0.81x).
- karez uses about the same CPU per request as the others at its rate (28.7 to 32 CPU-s per million requests on S1 and S2, against 28 to 38 for tokio-postgres, libpq and pgx).

## S8: karez's pool collapses under 512 workers (a finding)

With 512 workers sharing a pool of 32 connections, `karez-blocking` held 10,000 requests a second at a p99 of 0.28 ms and **collapsed above that**: at 10,500 to 16,000 requests a second only 1,100 to 6,800 of the 31,500 to 48,000 requests finished and the p99 was 7 to 8 seconds (S8 over TLS the same, collapsing above 9,500)
(`bench/results/2026-11-19-exit-c4/lane1/campaign.log`, the S8 probes). libpq's pool and pgx's held 41,000 to 42,000. The cause was **not investigated here**. The likely one was thought to be the pool's first-come-first-served hand-off (a released connection is reserved for the longest waiter, which must be scheduled before the connection is used again). **That reading was wrong:** the cause turned out to be a wake-up of every waiter on every release. **The pool needed a fix and a test at this load; it was not in the plan's gate list, and this run found it. Fixed after the run (commit after a90bca2): the cause was `notify_all` on every release (a thundering herd), not the hand-off order; see `docs/design/m1-plan.md` step 9. The numbers above are from before the fix; S8 has not been rerun on `c-4`.**

## Bulk reads (S4) and batch insert (S5)

| client | S4 plain rows/s | S4 TLS rows/s | S5 plain rows/s | S5 TLS rows/s |
|---|---:|---:|---:|---:|
| karez, chunk path | **1,677,571** | 942,418 | | |
| tokio-postgres | 1,618,909 | 1,441,545 | 48,075 | 47,526 |
| pgx | 1,469,023 | 1,375,188 | 118,183 | 116,073 |
| libpq | 1,340,123 | 1,125,619 | 159,795 | 152,393 |
| pgjdbc (tuned) | 1,316,309 | 1,143,380 | 189,897 | 183,050 |
| karez, row view | 1,300,728 | 1,424,096 | 145,730 | 128,337 |
| sqlx | 1,089,562 | 1,082,368 | 245,158 | 259,202 |
| asyncpg | 563,730 | 448,089 | 167,842 | 159,363 |
| psycopg3 | 491,352 | 488,377 | 56,189 | 46,162 |

- S4 plain: karez's chunk path is first, 1.04x tokio-postgres (passes 0.58, 0.60 and 0.91 s against 0.62, 0.62 and 0.68 s: they overlap); its row view is 0.80x tokio-postgres. Over TLS karez's row view is level with tokio-postgres (0.99x).
- **The chunk path over TLS is a finding: 942,418 rows/s, 44% below its plain rate** (three passes of 1.06 s against 0.58 to 0.91 s plain), while the row view shows no loss (its plain passes varied from 0.62 to 0.83 s) and the others lose 1% to 21%. Not investigated.
- S5: karez's batch insert is fifth of eight (145,730 rows/s; sqlx 245,158, pgjdbc tuned 189,897, asyncpg 167,842, libpq 159,795), as on the shared droplets.

## What this run supports and what it does not

Supports: karez's binary COPY is the fastest S6 client here and uses 0.16x of PgBulkInsert's CPU, **but misses the plan's 1.5x gate (1.36x plain, 1.43x over TLS)**, with the server's insert rate the probable limit; karez is level with the best Rust, Go, C and JVM clients on S1 to S3 plain and over TLS; TLS costs karez about what it costs the others on point queries.
Does not support: a "fastest" claim; any statement about other regions or instance types; a ranking among clients within about 10% of each other; any S8 claim for karez except the collapse above; the S8 wait-time distribution against Erlang C (not attempted: it needs a working pool first); taxi scenarios (not loaded).
