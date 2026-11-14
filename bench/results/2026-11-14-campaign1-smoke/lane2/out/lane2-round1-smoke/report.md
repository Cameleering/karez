# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R4 unpipelined, 8 connections (p99 bound 20.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 500 | 442 | 14483 | 436208 | 86.72 | 6 | 2 |
| karez-shared | lane2/1 | 500 | 475 | 11469 | 356516 | 119.90 | 7 | 2 |
| deadpool-postgres | lane2/1 | 500 | 455 | 14189 | 358613 | 96.94 | 6 | 2 |
| pgx | lane2/1 | 500 | 1024 | 13763 | 369099 | 188.80 | 21 | 2 |
| vertx-pg-client | lane2/1 | 500 | 422 | 15270 | 398459 | 137.74 | 205 | 2 |
| psycopg3 | lane2/1 | 500 | 623 | 14287 | 392167 | 390.16 | 41 | 2 |
| zero-postgres | lane2/1 | did not meet the bound | | | | | | 0 |

## S2 unpipelined, 8 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 45600 | 213 | 1866 | 2912 | 31.08 | 5 | 2 |
| deadpool-postgres | lane2/1 | 38000 | 219 | 807 | 1720 | 38.70 | 6 | 2 |
| zero-postgres | lane2/1 | 38000 | 214 | 672 | 2126 | 31.01 | 6 | 2 |
| karez-shared | lane2/1 | 32000 | 259 | 1806 | 2642 | 68.51 | 6 | 2 |
| pgx | lane2/1 | 32000 | 186 | 538 | 1929 | 48.00 | 22 | 2 |
| vertx-pg-client | lane2/1 | 32000 | 200 | 944 | 3736 | 28.38 | 431 | 2 |
| psycopg3 | lane2/1 | 4000 | 410 | 655 | 1001 | 199.00 | 40 | 2 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/1 | 1000000 | 0.720 | 1,389,854 | 0.47 | 4 | 2 | ok |
| pgx (pgx v5) | lane2/1 | 1000000 | 0.860 | 1,163,143 | 0.94 | 17 | 2 | ok |
| vertx-pg-client (vertx-pg-client) | lane2/1 | 1000000 | 1.147 | 871,764 | 2.75 | 1334 | 2 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane2/1 | 1000000 | 3.153 | 317,163 | 3.09 | 826 | 2 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/1 | 20000 | 0.158 | 126,462 | 1.00 | 5 | 2 | ok |
| pgx (pgx v5) | lane2/1 | 20000 | 0.219 | 91,304 | 5.00 | 16 | 2 | ok |
| vertx-pg-client (vertx-pg-client) | lane2/1 | 20000 | 0.335 | 59,737 | 44.00 | 212 | 2 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane2/1 | 20000 | 0.536 | 37,313 | 26.50 | 50 | 2 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/1 | 100000 | 0.150 | 664,673 | 1.00 | 4 | 2 | ok |
| pgx (pgx v5) | lane2/1 | 100000 | 0.247 | 404,563 | 2.45 | 16 | 2 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane2/1 | 100000 | 0.470 | 212,925 | 4.30 | 40 | 2 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| pgx (pgx v5) | lane2/1 | 13069067 | 17.438 | 749,455 | 1.49 | 17 | 2 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane2/1 | 13069067 | 124.922 | 104,618 | 9.25 | 81 | 2 | ok |

_failed: karez-blocking s8: rustbench: unknown scenario s8 (s1 to s6)
_
