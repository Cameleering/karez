# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R1 pipelined, 8 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane1/3t | 58000 | 651 | 2802 | 4555 | 49.35 | 13 | 3 |

## R1 unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/3t | 51110 | 289 | 1483 | 3031 | 32.86 | 12 | 3 |
| karez-shared | lane1/3t | 50809 | 602 | 2507 | 4817 | 70.35 | 16 | 3 |

## R1 unpipelined, 512 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/3t | 32000 | 317 | 1417 | 3949 | 43.00 | 32 | 3 |
| karez-shared | lane1/3t | 31040 | 522 | 1696 | 48497 | 101.52 | 87 | 3 |

## R2 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/3t | 8468 | 864 | 5210 | 9110 | 159.86 | 15 | 3 |
| karez-shared | lane1/3t | 8468 | 1004 | 4620 | 9110 | 248.06 | 20 | 3 |
| r2dbc-postgresql | lane1/3t | 1353 | 1401 | 6390 | 9503 | 1089.71 | 473 | 3 |

## R4 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/3t | 25404 | 578 | 5276 | 11993 | 62.50 | 16 | 3 |
| karez-shared | lane1/3t | 24642 | 717 | 4080 | 9241 | 112.71 | 20 | 3 |
| r2dbc-postgresql | lane1/3t | 5500 | 1270 | 7438 | 12517 | 394.02 | 478 | 3 |

## S1 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/3t | 82799 | 255 | 897 | 3359 | 26.31 | 12 | 3 |
| zero-postgres | lane1/3t | 81480 | 253 | 1319 | 2638 | 25.08 | 8 | 3 |
| karez-shared | lane1/3t | 79036 | 469 | 1499 | 2736 | 46.54 | 14 | 3 |

## S2 unpipelined, 1 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/3t | 8939 | 157 | 424 | 2146 | 23.68 | 6 | 3 |
| karez-shared | lane1/3t | 7760 | 171 | 1303 | 4014 | 44.70 | 6 | 3 |
| zero-postgres | lane1/3t | 7750 | 181 | 717 | 2114 | 32.65 | 7 | 3 |

## S2 unpipelined, 8 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/3t | 47459 | 200 | 1729 | 3097 | 25.51 | 7 | 3 |
| zero-postgres | lane1/3t | 47045 | 206 | 905 | 2540 | 22.02 | 7 | 3 |
| karez-shared | lane1/3t | 38800 | 237 | 897 | 3097 | 51.68 | 7 | 3 |

## S2 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/3t | 56586 | 285 | 1860 | 3162 | 30.62 | 12 | 3 |
| karez-shared | lane1/3t | 53118 | 416 | 1597 | 3097 | 60.18 | 15 | 3 |
| zero-postgres | lane1/3t | 52690 | 247 | 1044 | 2572 | 25.73 | 8 | 3 |

## S2 unpipelined, 512 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane1/3t | 38800 | 553 | 1843 | 3654 | 82.52 | 88 | 3 |
| karez-blocking | lane1/3t | 36860 | 309 | 1860 | 4424 | 38.97 | 30 | 3 |
| zero-postgres | lane1/3t | 31040 | 293 | 1188 | 2900 | 31.18 | 17 | 3 |

## S7 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/3t | 42494 | 309 | 1122 | 2277 | 36.01 | 14 | 3 |
| karez-shared | lane1/3t | 40723 | 391 | 1057 | 2114 | 69.12 | 17 | 3 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane1/3t | 1000000 | 0.564 | 1,773,679 | 0.82 | 8 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/3t | 1000000 | 0.774 | 1,291,990 | 0.39 | 6 | 3 | ok |
| diesel (diesel 2 (libpq bundled, single-row mode)) | lane1/3t | 1000000 | 0.827 | 1,209,629 | 0.83 | 8 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/3t | 1000000 | 0.898 | 1,113,214 | 1.07 | 1731 | 3 | ok |
| sqlx (sqlx 0.8) | lane1/3t | 1000000 | 1.038 | 963,020 | 1.01 | 7 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/3t | 1000000 | 1.224 | 816,860 | 0.53 | 6 | 3 | ok |
| r2dbc-postgresql (r2dbc-postgresql 1.0) | lane1/3t | 1000000 | 3.034 | 329,544 | 3.61 | 1967 | 3 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-mrow (karez-pg 0.0.0 (row view)) | lane1/3t | 100000 | 0.417 | 239,521 | 0.60 | 11 | 3 | ok |
| sqlx (sqlx 0.8) | lane1/3t | 100000 | 0.455 | 219,684 | 1.10 | 9 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/3t | 100000 | 0.651 | 153,633 | 9.20 | 314 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/3t | 100000 | 0.764 | 130,959 | 1.10 | 7 | 3 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/3t | 3000000 | 4.575 | 655,795 | 0.83 | 6 | 3 | ok |
| sqlx (sqlx 0.8) | lane1/3t | 3000000 | 4.661 | 643,680 | 0.86 | 8 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/3t | 3000000 | 5.735 | 523,122 | 1.84 | 1958 | 3 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane1/3t | 13069067 | 9.680 | 1,350,138 | 1.10 | 8 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/3t | 13069067 | 15.732 | 830,758 | 0.74 | 6 | 3 | ok |
| sqlx (sqlx 0.8) | lane1/3t | 13069067 | 16.579 | 788,290 | 1.18 | 7 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/3t | 13069067 | 18.343 | 712,467 | 0.55 | 6 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/3t | 13069067 | 23.968 | 545,276 | 0.83 | 3803 | 3 | ok |
