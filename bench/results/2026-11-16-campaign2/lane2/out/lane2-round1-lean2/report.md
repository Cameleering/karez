# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R1 pipelined, 8 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane2/1 | 58200 | 741 | 2834 | 4424 | 50.27 | 12 | 3 |
| vertx-pg-client | lane2/1 | 38000 | 214 | 1892 | 5014 | 27.35 | 511 | 3 |

## R1 unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| deadpool-postgres | lane2/1 | 60140 | 276 | 2900 | 5997 | 39.37 | 8 | 3 |
| karez-blocking | lane2/1 | 58200 | 322 | 1729 | 3555 | 35.63 | 11 | 3 |
| pgx | lane2/1 | 54572 | 245 | 2073 | 4686 | 42.28 | 27 | 3 |
| karez-shared | lane2/1 | 51347 | 725 | 2900 | 4227 | 72.95 | 14 | 3 |
| vertx-pg-client | lane2/1 | 42000 | 220 | 2572 | 5079 | 25.94 | 452 | 3 |
| sqlx-tuned | lane2/1 | 40740 | 453 | 2310 | 4293 | 62.55 | 8 | 3 |
| psycopg3 | lane2/1 | 4250 | 453 | 1352 | 1663 | 210.84 | 41 | 3 |

## R1 unpipelined, 512 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 38800 | 309 | 897 | 2310 | 46.73 | 30 | 3 |
| karez-shared | lane2/1 | 34000 | 659 | 2540 | 78119 | 99.90 | 84 | 3 |
| deadpool-postgres | lane2/1 | 34000 | 285 | 1810 | 3981 | 53.28 | 23 | 3 |
| pgx | lane2/1 | 31000 | 281 | 2966 | 6062 | 58.75 | 50 | 3 |
| vertx-pg-client | lane2/1 | 30000 | 198 | 1466 | 10945 | 30.91 | 452 | 3 |
| sqlx-tuned | lane2/1 | 23000 | 305 | 815 | 1827 | 66.04 | 22 | 3 |
| psycopg3 | lane2/1 | 4234 | 528 | 2703 | 3457 | 229.04 | 50 | 3 |

## R2 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane2/1 | 10185 | 1319 | 7766 | 12386 | 280.60 | 20 | 3 |
| deadpool-postgres | lane2/1 | 10185 | 799 | 4751 | 9634 | 217.75 | 9 | 3 |
| karez-blocking | lane2/1 | 10000 | 1020 | 6652 | 9372 | 186.78 | 14 | 3 |
| sqlx-tuned | lane2/1 | 9409 | 2310 | 6849 | 10158 | 317.59 | 10 | 3 |
| pgx | lane2/1 | 8468 | 1057 | 6914 | 17433 | 313.59 | 34 | 3 |
| vertx-pg-client | lane2/1 | 5754 | 799 | 2056 | 3752 | 165.56 | 434 | 3 |
| psycopg3 | lane2/1 | 823 | 2310 | 3654 | 6586 | 1109.32 | 46 | 3 |

## R4 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane2/1 | 30070 | 1384 | 6193 | 9110 | 119.45 | 20 | 3 |
| deadpool-postgres | lane2/1 | 30070 | 635 | 5538 | 11207 | 75.39 | 9 | 3 |
| karez-blocking | lane2/1 | 29100 | 594 | 7700 | 12911 | 73.23 | 15 | 3 |
| pgx | lane2/1 | 27160 | 774 | 7111 | 11731 | 95.69 | 29 | 3 |
| sqlx-tuned | lane2/1 | 24250 | 766 | 6193 | 8847 | 112.71 | 10 | 3 |
| vertx-pg-client | lane2/1 | 17000 | 545 | 3654 | 6521 | 59.42 | 460 | 3 |
| psycopg3 | lane2/1 | 2125 | 1221 | 6717 | 11338 | 389.43 | 44 | 3 |

## S1 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 96000 | 338 | 1270 | 3097 | 27.45 | 10 | 3 |
| deadpool-postgres | lane2/1 | 86563 | 261 | 1860 | 4293 | 33.82 | 8 | 3 |
| pgx | lane2/1 | 86563 | 202 | 1548 | 3097 | 28.86 | 26 | 3 |
| karez-shared | lane2/1 | 71508 | 502 | 1810 | 2966 | 50.79 | 13 | 3 |
| sqlx-tuned | lane2/1 | 54572 | 313 | 1991 | 3326 | 54.94 | 8 | 3 |
| vertx-pg-client | lane2/1 | 45163 | 191 | 1909 | 4555 | 25.56 | 426 | 3 |
| psycopg3 | lane2/1 | 6116 | 362 | 1188 | 1974 | 155.60 | 41 | 3 |

## S2 unpipelined, 1 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 8939 | 155 | 420 | 1466 | 23.80 | 5 | 3 |
| vertx-pg-client | lane2/1 | 7760 | 185 | 569 | 2089 | 49.10 | 404 | 3 |
| karez-shared | lane2/1 | 7750 | 173 | 449 | 4620 | 53.98 | 5 | 3 |
| deadpool-postgres | lane2/1 | 7750 | 171 | 403 | 1221 | 44.88 | 6 | 3 |
| pgx | lane2/1 | 7057 | 700 | 1303 | 2703 | 72.57 | 22 | 3 |
| sqlx-tuned | lane2/1 | 4792 | 173 | 528 | 3424 | 68.37 | 6 | 3 |
| psycopg3 | lane2/1 | 3879 | 305 | 561 | 2179 | 180.86 | 40 | 3 |

## S2 unpipelined, 8 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 46560 | 204 | 522 | 2245 | 29.21 | 6 | 3 |
| deadpool-postgres | lane2/1 | 45163 | 212 | 1712 | 3850 | 32.37 | 6 | 3 |
| pgx | lane2/1 | 44000 | 185 | 1057 | 1942 | 40.76 | 24 | 3 |
| karez-shared | lane2/1 | 39518 | 243 | 1434 | 2040 | 56.77 | 6 | 3 |
| vertx-pg-client | lane2/1 | 34000 | 200 | 561 | 4620 | 26.57 | 430 | 3 |
| sqlx-tuned | lane2/1 | 25555 | 206 | 651 | 2540 | 58.83 | 6 | 3 |
| psycopg3 | lane2/1 | 5410 | 412 | 1810 | 2023 | 180.92 | 40 | 3 |

## S2 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 62080 | 276 | 807 | 1729 | 33.13 | 10 | 3 |
| deadpool-postgres | lane2/1 | 60218 | 261 | 1417 | 2572 | 39.21 | 8 | 3 |
| pgx | lane2/1 | 58200 | 216 | 1450 | 3097 | 38.35 | 26 | 3 |
| karez-shared | lane2/1 | 56586 | 528 | 1516 | 3326 | 63.10 | 13 | 3 |
| sqlx-tuned | lane2/1 | 42680 | 367 | 1663 | 2802 | 60.85 | 8 | 3 |
| vertx-pg-client | lane2/1 | 36000 | 191 | 428 | 4358 | 25.03 | 428 | 3 |
| psycopg3 | lane2/1 | 4940 | 432 | 1352 | 1991 | 191.63 | 41 | 3 |

## S2 unpipelined, 512 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 40740 | 276 | 807 | 2114 | 43.41 | 29 | 3 |
| deadpool-postgres | lane2/1 | 38800 | 281 | 1188 | 3424 | 50.77 | 22 | 3 |
| pgx | lane2/1 | 34000 | 245 | 1483 | 4227 | 52.27 | 48 | 3 |
| karez-shared | lane2/1 | 32632 | 494 | 1417 | 2245 | 94.90 | 85 | 3 |
| vertx-pg-client | lane2/1 | 30000 | 189 | 412 | 5538 | 29.07 | 455 | 3 |
| sqlx-tuned | lane2/1 | 27160 | 313 | 1499 | 4178 | 66.69 | 21 | 3 |
| psycopg3 | lane2/1 | 4750 | 453 | 1647 | 2073 | 199.47 | 50 | 3 |

## S7 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 48500 | 309 | 1729 | 3916 | 40.12 | 12 | 3 |
| pgx | lane2/1 | 46560 | 241 | 1647 | 3523 | 44.56 | 29 | 3 |
| karez-shared | lane2/1 | 42000 | 416 | 1057 | 2040 | 74.05 | 16 | 3 |
| sqlx-tuned | lane2/1 | 31040 | 313 | 1384 | 2605 | 71.84 | 11 | 3 |
| vertx-pg-client | lane2/1 | 20000 | 202 | 578 | 3621 | 39.01 | 472 | 3 |
| psycopg3 | lane2/1 | 4750 | 506 | 1876 | 2408 | 213.50 | 48 | 3 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane2/1 | 1000000 | 0.537 | 1,860,465 | 0.72 | 7 | 3 | ok |
| pgx (pgx v5) | lane2/1 | 1000000 | 0.704 | 1,420,979 | 0.79 | 17 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/1 | 1000000 | 0.727 | 1,375,516 | 0.40 | 4 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane2/1 | 1000000 | 0.822 | 1,216,841 | 0.36 | 4 | 3 | ok |
| vertx-pg-client (vertx-pg-client) | lane2/1 | 1000000 | 1.186 | 842,957 | 2.53 | 1748 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane2/1 | 1000000 | 2.414 | 414,302 | 2.27 | 826 | 3 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-mrow (karez-pg 0.0.0 (row view)) | lane2/1 | 100000 | 0.401 | 249,252 | 0.70 | 10 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/1 | 100000 | 0.720 | 138,966 | 1.10 | 5 | 3 | ok |
| vertx-pg-client (vertx-pg-client) | lane2/1 | 100000 | 0.936 | 106,883 | 5.10 | 312 | 3 | ok |
| pgx (pgx v5) | lane2/1 | 100000 | 0.984 | 101,594 | 4.20 | 16 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane2/1 | 100000 | 1.986 | 50,352 | 19.70 | 88 | 3 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/1 | 3000000 | 4.320 | 694,380 | 0.89 | 5 | 3 | ok |
| pgx (pgx v5) | lane2/1 | 3000000 | 5.411 | 554,390 | 1.91 | 17 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane2/1 | 3000000 | 8.945 | 335,398 | 2.91 | 40 | 3 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane2/1 | 13069067 | 9.669 | 1,351,646 | 1.07 | 7 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/1 | 13069067 | 13.462 | 970,833 | 0.68 | 4 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane2/1 | 13069067 | 15.496 | 843,361 | 0.58 | 4 | 3 | ok |
| pgx (pgx v5) | lane2/1 | 13069067 | 16.134 | 810,039 | 1.39 | 18 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane2/1 | 13069067 | 115.655 | 113,000 | 8.45 | 81 | 3 | ok |
