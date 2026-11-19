# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R1 pipelined, 8 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane2/1 | 49400 | 725 | 2736 | 4948 | 61.39 | 11 | 3 |
| vertx-pg-client | lane2/1 | 36000 | 230 | 1761 | 9765 | 31.58 | 524 | 3 |

## R1 unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 49400 | 305 | 1368 | 2638 | 40.08 | 11 | 3 |
| deadpool-postgres | lane2/1 | 49400 | 281 | 1745 | 3949 | 46.36 | 8 | 3 |
| karez-shared | lane2/1 | 46930 | 881 | 2998 | 4424 | 81.40 | 14 | 3 |
| pgx | lane2/1 | 46930 | 276 | 2507 | 5603 | 49.08 | 27 | 3 |
| vertx-pg-client | lane2/1 | 30000 | 214 | 537 | 4751 | 34.25 | 430 | 3 |
| psycopg3 | lane2/1 | 3750 | 528 | 1368 | 8847 | 245.14 | 41 | 3 |

## R1 unpipelined, 512 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 32000 | 342 | 2146 | 4227 | 48.89 | 30 | 3 |
| deadpool-postgres | lane2/1 | 28500 | 326 | 1319 | 3162 | 49.38 | 24 | 3 |
| pgx | lane2/1 | 28500 | 326 | 2998 | 6062 | 59.61 | 50 | 3 |
| vertx-pg-client | lane2/1 | 28000 | 210 | 553 | 5603 | 36.25 | 464 | 3 |
| karez-shared | lane2/1 | 26064 | 553 | 1991 | 40108 | 120.36 | 82 | 3 |
| psycopg3 | lane2/1 | 3750 | 651 | 2245 | 2802 | 271.77 | 50 | 3 |

## R2 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 9500 | 954 | 5407 | 11338 | 195.65 | 14 | 3 |
| deadpool-postgres | lane2/1 | 9500 | 782 | 3850 | 7504 | 216.74 | 9 | 3 |
| karez-shared | lane2/1 | 9000 | 1044 | 3588 | 9503 | 274.00 | 19 | 3 |
| pgx | lane2/1 | 8550 | 1106 | 7635 | 17170 | 308.31 | 34 | 3 |
| vertx-pg-client | lane2/1 | 5500 | 635 | 1368 | 2474 | 186.03 | 449 | 3 |
| psycopg3 | lane2/1 | 500 | 5472 | 7307 | 7897 | 1967.54 | 46 | 3 |

## R3 unpipelined, 8 connections (p99 bound 60.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 0 | 6390 | 1216348 | 1551892 | 36.46 | 5 | 3 |
| karez-shared | lane2/1 | 0 | 6914 | 1149239 | 1384120 | 87.81 | 6 | 3 |
| deadpool-postgres | lane2/1 | 0 | 6586 | 1132462 | 1619001 | 83.57 | 6 | 3 |
| pgx | lane2/1 | 0 | 6783 | 1132462 | 1468006 | 122.30 | 11 | 3 |
| vertx-pg-client | lane2/1 | did not meet the bound | | | | | | 0 |
| psycopg3 | lane2/1 | 0 | 6652 | 1149239 | 1484784 | 329.63 | 40 | 3 |

## R4 unpipelined, 64 connections (p99 bound 20.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane2/1 | 445 | 537 | 17170 | 367002 | 280.65 | 15 | 3 |
| karez-blocking | lane2/1 | did not meet the bound | | | | | | 0 |
| deadpool-postgres | lane2/1 | did not meet the bound | | | | | | 0 |
| pgx | lane2/1 | did not meet the bound | | | | | | 0 |
| vertx-pg-client | lane2/1 | did not meet the bound | | | | | | 0 |
| psycopg3 | lane2/1 | did not meet the bound | | | | | | 0 |

## S1 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 79420 | 281 | 979 | 1647 | 33.40 | 10 | 3 |
| pgx | lane2/1 | 76000 | 206 | 1696 | 3359 | 33.86 | 25 | 3 |
| zero-postgres | lane2/1 | 76000 | 465 | 1909 | 2900 | 32.36 | 7 | 3 |
| deadpool-postgres | lane2/1 | 75449 | 264 | 1597 | 2933 | 39.12 | 8 | 3 |
| karez-shared | lane2/1 | 52000 | 391 | 1004 | 2507 | 63.90 | 13 | 3 |
| vertx-pg-client | lane2/1 | 32000 | 189 | 848 | 2441 | 28.07 | 423 | 3 |
| psycopg3 | lane2/1 | 5000 | 399 | 676 | 1761 | 195.36 | 41 | 3 |

## S2 unpipelined, 1 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| deadpool-postgres | lane2/1 | 8000 | 181 | 1303 | 4358 | 37.67 | 6 | 3 |
| vertx-pg-client | lane2/1 | 7600 | 185 | 1991 | 4227 | 50.40 | 401 | 3 |
| karez-blocking | lane2/1 | 7500 | 171 | 436 | 2040 | 34.41 | 5 | 3 |
| zero-postgres | lane2/1 | 7500 | 183 | 528 | 2638 | 41.94 | 5 | 3 |
| karez-shared | lane2/1 | 7000 | 191 | 741 | 2966 | 61.06 | 5 | 3 |
| pgx | lane2/1 | 6650 | 717 | 1303 | 2769 | 76.55 | 22 | 3 |
| psycopg3 | lane2/1 | 3750 | 395 | 791 | 3654 | 209.95 | 40 | 3 |

## S2 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 54150 | 305 | 1974 | 3850 | 38.16 | 10 | 3 |
| zero-postgres | lane2/1 | 53200 | 268 | 1319 | 2867 | 35.86 | 7 | 3 |
| deadpool-postgres | lane2/1 | 52000 | 276 | 1122 | 2933 | 44.90 | 8 | 3 |
| karez-shared | lane2/1 | 49400 | 610 | 1581 | 2736 | 73.13 | 13 | 3 |
| pgx | lane2/1 | 48000 | 239 | 1286 | 2703 | 45.07 | 26 | 3 |
| vertx-pg-client | lane2/1 | 30000 | 210 | 395 | 2507 | 31.82 | 432 | 3 |
| psycopg3 | lane2/1 | 4287 | 528 | 1942 | 2245 | 234.49 | 41 | 3 |

## S2 unpipelined, 512 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 34200 | 301 | 954 | 2540 | 45.15 | 29 | 3 |
| pgx | lane2/1 | 32000 | 297 | 1827 | 4178 | 53.75 | 48 | 3 |
| vertx-pg-client | lane2/1 | 32000 | 208 | 1335 | 7766 | 31.82 | 450 | 3 |
| zero-postgres | lane2/1 | 30400 | 305 | 963 | 2736 | 36.37 | 15 | 3 |
| deadpool-postgres | lane2/1 | 30000 | 305 | 864 | 2474 | 49.80 | 23 | 3 |
| karez-shared | lane2/1 | 27436 | 498 | 1876 | 73925 | 110.61 | 83 | 3 |
| psycopg3 | lane2/1 | 4061 | 473 | 1909 | 3883 | 212.51 | 50 | 3 |

## S7 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/1 | 41800 | 313 | 1221 | 3097 | 43.24 | 12 | 3 |
| karez-shared | lane2/1 | 41800 | 569 | 1942 | 3260 | 81.50 | 16 | 3 |
| pgx | lane2/1 | 39710 | 268 | 1827 | 3457 | 50.83 | 35 | 3 |
| vertx-pg-client | lane2/1 | 15000 | 230 | 387 | 3097 | 60.29 | 469 | 3 |
| psycopg3 | lane2/1 | did not meet the bound | | | | | | 0 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/1 | 1000000 | 0.722 | 1,384,658 | 0.61 | 4 | 3 | ok |
| pgx (pgx v5) | lane2/1 | 1000000 | 0.782 | 1,278,921 | 0.87 | 18 | 3 | ok |
| vertx-pg-client (vertx-pg-client) | lane2/1 | 1000000 | 1.170 | 854,628 | 2.84 | 1710 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane2/1 | 1000000 | 2.817 | 355,038 | 2.70 | 826 | 3 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/1 | 100000 | 0.790 | 126,566 | 1.40 | 5 | 3 | ok |
| vertx-pg-client (vertx-pg-client) | lane2/1 | 100000 | 0.968 | 103,316 | 5.50 | 318 | 3 | ok |
| pgx (pgx v5) | lane2/1 | 100000 | 1.044 | 95,790 | 5.30 | 17 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane2/1 | 100000 | 3.312 | 30,195 | 33.00 | 88 | 3 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/1 | 3000000 | 4.596 | 652,713 | 1.09 | 5 | 3 | ok |
| pgx (pgx v5) | lane2/1 | 3000000 | 6.925 | 433,218 | 2.47 | 18 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane2/1 | 3000000 | 13.055 | 229,802 | 4.27 | 40 | 3 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/1 | 13069067 | 13.903 | 940,018 | 1.01 | 4 | 3 | ok |
| pgx (pgx v5) | lane2/1 | 13069067 | 18.120 | 721,255 | 1.55 | 18 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane2/1 | 13069067 | 132.692 | 98,492 | 9.85 | 81 | 3 | ok |
