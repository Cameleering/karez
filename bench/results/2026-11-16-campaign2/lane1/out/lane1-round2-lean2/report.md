# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R1 pipelined, 8 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane1/2 | 60140 | 815 | 2900 | 4489 | 48.33 | 12 | 3 |
| vertx-pg-client | lane1/2 | 32000 | 259 | 2073 | 8225 | 29.27 | 476 | 3 |

## R1 unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/2 | 60140 | 346 | 2343 | 4751 | 32.45 | 11 | 3 |
| deadpool-postgres | lane1/2 | 60140 | 313 | 2703 | 5931 | 36.96 | 8 | 3 |
| pgx | lane1/2 | 56454 | 293 | 2277 | 4112 | 40.53 | 27 | 3 |
| karez-shared | lane1/2 | 56260 | 774 | 2179 | 3391 | 67.62 | 14 | 3 |
| vertx-pg-client | lane1/2 | 44620 | 285 | 2146 | 4145 | 24.94 | 430 | 3 |
| sqlx-tuned | lane1/2 | 41400 | 449 | 2245 | 3752 | 61.06 | 9 | 3 |
| psycopg3 | lane1/2 | 4469 | 578 | 2310 | 2703 | 221.04 | 41 | 3 |

## R1 unpipelined, 512 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/2 | 46000 | 346 | 2310 | 5865 | 39.91 | 30 | 3 |
| deadpool-postgres | lane1/2 | 44620 | 317 | 1499 | 4014 | 43.39 | 23 | 3 |
| karez-shared | lane1/2 | 37182 | 659 | 2769 | 67633 | 91.70 | 84 | 3 |
| pgx | lane1/2 | 36860 | 289 | 2638 | 6128 | 51.66 | 50 | 3 |
| vertx-pg-client | lane1/2 | 31000 | 257 | 995 | 8585 | 29.01 | 454 | 3 |
| sqlx-tuned | lane1/2 | 29000 | 354 | 1319 | 2540 | 67.71 | 22 | 3 |
| psycopg3 | lane1/2 | 4250 | 586 | 1860 | 4227 | 231.50 | 50 | 3 |

## R2 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/2 | 10820 | 938 | 5210 | 10289 | 170.21 | 14 | 3 |
| karez-shared | lane1/2 | 10820 | 1335 | 6128 | 9765 | 256.30 | 19 | 3 |
| deadpool-postgres | lane1/2 | 10820 | 823 | 5014 | 9634 | 213.25 | 9 | 3 |
| sqlx-tuned | lane1/2 | 10185 | 2441 | 7569 | 10945 | 313.92 | 10 | 3 |
| pgx | lane1/2 | 8853 | 1036 | 6980 | 18481 | 297.62 | 34 | 3 |
| vertx-pg-client | lane1/2 | 6456 | 1012 | 3031 | 4555 | 160.86 | 435 | 3 |
| psycopg3 | lane1/2 | 823 | 2802 | 5669 | 38011 | 1124.02 | 46 | 3 |

## R4 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| deadpool-postgres | lane1/2 | 32000 | 700 | 6652 | 11076 | 72.81 | 9 | 3 |
| karez-blocking | lane1/2 | 31991 | 676 | 6521 | 12124 | 64.13 | 15 | 3 |
| karez-shared | lane1/2 | 31031 | 1122 | 4882 | 8225 | 111.47 | 20 | 3 |
| pgx | lane1/2 | 30070 | 995 | 7963 | 12255 | 89.08 | 35 | 3 |
| sqlx-tuned | lane1/2 | 25555 | 774 | 4424 | 6521 | 110.66 | 9 | 3 |
| vertx-pg-client | lane1/2 | 15500 | 545 | 1810 | 3097 | 69.46 | 456 | 3 |
| psycopg3 | lane1/2 | 2125 | 1286 | 4948 | 7176 | 392.79 | 44 | 3 |

## S1 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/2 | 97854 | 354 | 1696 | 2540 | 26.20 | 10 | 3 |
| deadpool-postgres | lane1/2 | 94090 | 330 | 1794 | 3359 | 31.33 | 8 | 3 |
| pgx | lane1/2 | 93120 | 272 | 1958 | 3195 | 26.95 | 26 | 3 |
| karez-shared | lane1/2 | 71102 | 473 | 1286 | 2114 | 49.58 | 13 | 3 |
| sqlx-tuned | lane1/2 | 51644 | 350 | 1745 | 3162 | 57.39 | 8 | 3 |
| vertx-pg-client | lane1/2 | 44000 | 247 | 1057 | 3424 | 24.17 | 426 | 3 |
| psycopg3 | lane1/2 | 6250 | 432 | 1532 | 3391 | 159.21 | 41 | 3 |

## S2 unpipelined, 1 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| deadpool-postgres | lane1/2 | 5881 | 220 | 1368 | 5145 | 40.12 | 5 | 3 |
| karez-blocking | lane1/2 | 5750 | 210 | 522 | 2343 | 25.59 | 5 | 3 |
| karez-shared | lane1/2 | 5578 | 220 | 1237 | 6390 | 46.81 | 5 | 3 |
| vertx-pg-client | lane1/2 | 5410 | 228 | 971 | 2277 | 48.45 | 280 | 3 |
| pgx | lane1/2 | 4792 | 823 | 1729 | 6324 | 70.06 | 22 | 3 |
| sqlx-tuned | lane1/2 | 2877 | 243 | 1729 | 6062 | 69.72 | 6 | 3 |
| psycopg3 | lane1/2 | 2750 | 432 | 1860 | 2736 | 191.74 | 40 | 3 |

## S2 unpipelined, 8 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/2 | 38800 | 245 | 864 | 2089 | 25.43 | 6 | 3 |
| deadpool-postgres | lane1/2 | 36860 | 255 | 963 | 2146 | 34.10 | 6 | 3 |
| pgx | lane1/2 | 35754 | 259 | 1106 | 2310 | 41.58 | 24 | 3 |
| karez-shared | lane1/2 | 32980 | 276 | 1020 | 2179 | 53.55 | 6 | 3 |
| vertx-pg-client | lane1/2 | 32980 | 257 | 1401 | 3949 | 27.85 | 426 | 3 |
| sqlx-tuned | lane1/2 | 19400 | 251 | 782 | 2802 | 53.05 | 6 | 3 |
| psycopg3 | lane1/2 | 5250 | 498 | 1663 | 4112 | 188.56 | 40 | 3 |

## S2 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/2 | 63741 | 322 | 913 | 2310 | 31.04 | 10 | 3 |
| karez-shared | lane1/2 | 62080 | 643 | 1729 | 3097 | 58.54 | 13 | 3 |
| deadpool-postgres | lane1/2 | 62062 | 301 | 995 | 2408 | 37.32 | 8 | 3 |
| pgx | lane1/2 | 62062 | 276 | 1712 | 3260 | 36.45 | 26 | 3 |
| sqlx-tuned | lane1/2 | 43808 | 408 | 1679 | 2834 | 61.31 | 8 | 3 |
| vertx-pg-client | lane1/2 | 43315 | 268 | 1647 | 4489 | 23.93 | 428 | 3 |
| psycopg3 | lane1/2 | 5645 | 528 | 1597 | 1942 | 186.78 | 41 | 3 |

## S2 unpipelined, 512 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/2 | 50440 | 317 | 946 | 2998 | 36.33 | 29 | 3 |
| deadpool-postgres | lane1/2 | 48500 | 305 | 1434 | 4293 | 42.92 | 23 | 3 |
| pgx | lane1/2 | 42680 | 268 | 1942 | 4882 | 45.70 | 48 | 3 |
| vertx-pg-client | lane1/2 | 40000 | 261 | 1106 | 7635 | 24.75 | 452 | 3 |
| karez-shared | lane1/2 | 38000 | 518 | 1401 | 2605 | 84.41 | 86 | 3 |
| sqlx-tuned | lane1/2 | 34000 | 354 | 1860 | 4047 | 65.50 | 21 | 3 |
| psycopg3 | lane1/2 | 4750 | 553 | 1614 | 3817 | 210.95 | 50 | 3 |

## S7 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/2 | 50440 | 338 | 1843 | 3326 | 35.51 | 12 | 3 |
| karez-shared | lane1/2 | 48927 | 502 | 1270 | 2802 | 66.57 | 16 | 3 |
| pgx | lane1/2 | 47459 | 285 | 1384 | 3031 | 42.58 | 29 | 3 |
| sqlx-tuned | lane1/2 | 31991 | 362 | 1516 | 2638 | 70.92 | 11 | 3 |
| vertx-pg-client | lane1/2 | 20000 | 276 | 522 | 2966 | 40.12 | 470 | 3 |
| psycopg3 | lane1/2 | 4850 | 569 | 1794 | 2179 | 211.15 | 48 | 3 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane1/2 | 1000000 | 0.582 | 1,717,033 | 0.86 | 7 | 3 | ok |
| pgx (pgx v5) | lane1/2 | 1000000 | 0.633 | 1,580,904 | 0.74 | 17 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/2 | 1000000 | 0.683 | 1,464,129 | 0.42 | 4 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/2 | 1000000 | 0.686 | 1,456,876 | 0.35 | 4 | 3 | ok |
| vertx-pg-client (vertx-pg-client) | lane1/2 | 1000000 | 1.206 | 829,463 | 2.76 | 1711 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/2 | 1000000 | 2.284 | 437,733 | 2.24 | 826 | 3 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-mrow (karez-pg 0.0.0 (row view)) | lane1/2 | 100000 | 0.409 | 244,559 | 0.60 | 10 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/2 | 100000 | 0.721 | 138,735 | 1.10 | 5 | 3 | ok |
| pgx (pgx v5) | lane1/2 | 100000 | 0.974 | 102,673 | 4.40 | 16 | 3 | ok |
| vertx-pg-client (vertx-pg-client) | lane1/2 | 100000 | 0.974 | 102,669 | 5.40 | 313 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/2 | 100000 | 1.927 | 51,905 | 19.20 | 88 | 3 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/2 | 3000000 | 4.288 | 699,594 | 0.89 | 4 | 3 | ok |
| pgx (pgx v5) | lane1/2 | 3000000 | 5.471 | 548,347 | 1.96 | 17 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/2 | 3000000 | 9.139 | 328,249 | 2.94 | 40 | 3 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane1/2 | 13069067 | 9.670 | 1,351,548 | 1.28 | 7 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/2 | 13069067 | 11.870 | 1,101,035 | 0.62 | 4 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/2 | 13069067 | 13.039 | 1,002,329 | 0.56 | 5 | 3 | ok |
| pgx (pgx v5) | lane1/2 | 13069067 | 15.990 | 817,310 | 1.39 | 19 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/2 | 13069067 | 115.552 | 113,101 | 8.45 | 81 | 3 | ok |
