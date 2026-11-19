# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R1 pipelined, 8 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane3/3t | 56260 | 750 | 2900 | 4882 | 50.85 | 13 | 3 |

## R1 unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/3t | 48927 | 293 | 1073 | 2408 | 34.37 | 12 | 3 |
| karez-shared | lane3/3t | 48500 | 586 | 2245 | 3916 | 72.56 | 16 | 3 |
| sqlx-tuned | lane3/3t | 34920 | 481 | 1958 | 3457 | 65.65 | 10 | 3 |

## R1 unpipelined, 512 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane3/3t | 32980 | 586 | 2089 | 12124 | 98.51 | 87 | 3 |
| karez-blocking | lane3/3t | 32000 | 334 | 2966 | 7635 | 43.04 | 32 | 3 |
| sqlx-tuned | lane3/3t | 20000 | 338 | 987 | 2703 | 71.48 | 23 | 3 |

## R2 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/3t | 8468 | 913 | 7307 | 11993 | 175.59 | 15 | 3 |
| karez-shared | lane3/3t | 8214 | 1057 | 6849 | 17170 | 260.62 | 21 | 3 |
| sqlx-tuned | lane3/3t | 7518 | 1892 | 5931 | 11731 | 327.03 | 12 | 3 |

## R4 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/3t | 24463 | 610 | 7569 | 13697 | 64.99 | 16 | 3 |
| karez-shared | lane3/3t | 24463 | 799 | 5145 | 10945 | 116.01 | 20 | 3 |
| sqlx-tuned | lane3/3t | 20700 | 799 | 6062 | 9241 | 117.81 | 11 | 3 |
| psycopg3 | lane3/3t | 2182 | 1466 | 7373 | 11469 | 412.97 | 44 | 3 |

## S1 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/3t | 81480 | 276 | 1614 | 2834 | 27.65 | 12 | 3 |
| karez-shared | lane3/3t | 73014 | 494 | 1548 | 2900 | 49.63 | 14 | 3 |
| sqlx-tuned | lane3/3t | 44654 | 293 | 1155 | 2703 | 59.76 | 10 | 3 |

## S2 unpipelined, 1 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/3t | 7750 | 165 | 371 | 1122 | 27.18 | 6 | 3 |
| karez-shared | lane3/3t | 7275 | 179 | 741 | 2376 | 50.71 | 6 | 3 |
| sqlx-tuned | lane3/3t | 4365 | 189 | 889 | 2998 | 72.52 | 8 | 3 |

## S2 unpipelined, 8 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/3t | 44620 | 212 | 1761 | 6652 | 28.01 | 7 | 3 |
| karez-shared | lane3/3t | 37636 | 249 | 1319 | 2474 | 56.51 | 7 | 3 |
| sqlx-tuned | lane3/3t | 23522 | 226 | 1909 | 4227 | 62.91 | 8 | 3 |

## S2 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/3t | 52935 | 276 | 1909 | 3785 | 31.90 | 12 | 3 |
| karez-shared | lane3/3t | 46000 | 358 | 913 | 1696 | 62.94 | 14 | 3 |
| sqlx-tuned | lane3/3t | 36860 | 379 | 1614 | 2834 | 63.70 | 10 | 3 |

## S2 unpipelined, 512 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane3/3t | 34920 | 537 | 1696 | 2900 | 88.38 | 87 | 3 |
| karez-blocking | lane3/3t | 33872 | 301 | 1221 | 2605 | 39.84 | 31 | 3 |
| sqlx-tuned | lane3/3t | 22000 | 346 | 1106 | 2572 | 70.29 | 23 | 3 |

## S7 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/3t | 41400 | 317 | 1270 | 2671 | 37.50 | 14 | 3 |
| karez-shared | lane3/3t | 41400 | 436 | 1352 | 2474 | 69.41 | 17 | 3 |
| sqlx-tuned | lane3/3t | 26190 | 334 | 963 | 2114 | 76.11 | 13 | 3 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane3/3t | 1000000 | 0.585 | 1,709,402 | 0.89 | 8 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/3t | 1000000 | 0.760 | 1,316,482 | 0.53 | 6 | 3 | ok |
| pgx (pgx v5) | lane3/3t | 1000000 | 0.846 | 1,181,664 | 0.90 | 17 | 3 | ok |
| diesel (diesel 2 (libpq bundled, single-row mode)) | lane3/3t | 1000000 | 0.925 | 1,080,847 | 0.91 | 8 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane3/3t | 1000000 | 1.130 | 884,799 | 0.51 | 6 | 3 | ok |
| vertx-pg-client (vertx-pg-client) | lane3/3t | 1000000 | 1.467 | 681,803 | 3.07 | 1711 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane3/3t | 1000000 | 2.953 | 338,616 | 2.76 | 826 | 3 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-mrow (karez-pg 0.0.0 (row view)) | lane3/3t | 100000 | 0.420 | 238,322 | 0.70 | 11 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/3t | 100000 | 0.788 | 126,968 | 1.10 | 7 | 3 | ok |
| vertx-pg-client (vertx-pg-client) | lane3/3t | 100000 | 1.033 | 96,834 | 6.20 | 329 | 3 | ok |
| pgx (pgx v5) | lane3/3t | 100000 | 1.051 | 95,187 | 4.50 | 16 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane3/3t | 100000 | 2.077 | 48,137 | 20.70 | 88 | 3 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/3t | 3000000 | 4.741 | 632,738 | 0.89 | 6 | 3 | ok |
| pgx (pgx v5) | lane3/3t | 3000000 | 5.716 | 524,822 | 2.01 | 17 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane3/3t | 3000000 | 9.632 | 311,449 | 3.06 | 40 | 3 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane3/3t | 13069067 | 9.665 | 1,352,220 | 1.22 | 8 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane3/3t | 13069067 | 16.134 | 810,053 | 0.69 | 6 | 3 | ok |
| pgx (pgx v5) | lane3/3t | 13069067 | 16.618 | 786,459 | 1.43 | 18 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/3t | 13069067 | 21.296 | 613,684 | 0.82 | 6 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane3/3t | 13069067 | 121.089 | 107,930 | 8.85 | 81 | 3 | ok |
