# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R1 pipelined, 8 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane3/1 | 46000 | 518 | 1745 | 4080 | 52.96 | 10 | 3 |

## R1 unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/1 | 54320 | 375 | 2540 | 6128 | 30.51 | 11 | 3 |
| karez-shared | lane3/1 | 48927 | 586 | 2343 | 4293 | 68.70 | 14 | 3 |
| pgjdbc-tuned | lane3/1 | 46560 | 362 | 2245 | 5669 | 43.93 | 425 | 3 |
| sqlx | lane3/1 | 23280 | 602 | 2638 | 6521 | 83.62 | 8 | 3 |
| r2dbc-postgresql | lane3/1 | 18000 | 362 | 1335 | 3949 | 102.36 | 492 | 3 |

## R1 unpipelined, 512 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/1 | 36860 | 391 | 2212 | 4948 | 40.40 | 31 | 3 |
| pgjdbc-tuned | lane3/1 | 25220 | 346 | 1827 | 50070 | 56.08 | 507 | 3 |
| r2dbc-postgresql | lane3/1 | 24000 | 477 | 2802 | 6980 | 101.07 | 540 | 3 |
| sqlx | lane3/1 | 18000 | 651 | 2277 | 6455 | 88.93 | 21 | 3 |
| karez-shared | lane3/1 | 15500 | 408 | 1319 | 5931 | 122.61 | 80 | 3 |

## R2 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/1 | 10039 | 1012 | 7766 | 12386 | 161.66 | 14 | 3 |
| karez-shared | lane3/1 | 9879 | 1221 | 6586 | 12911 | 236.93 | 19 | 3 |
| pgjdbc-tuned | lane3/1 | 8000 | 864 | 6259 | 11731 | 216.00 | 439 | 3 |
| sqlx | lane3/1 | 7760 | 1597 | 7176 | 11731 | 313.00 | 10 | 3 |
| r2dbc-postgresql | lane3/1 | 1122 | 1516 | 6324 | 10158 | 1157.72 | 469 | 3 |

## R4 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/1 | 29100 | 725 | 7176 | 11469 | 59.44 | 15 | 3 |
| karez-shared | lane3/1 | 28130 | 963 | 6259 | 10682 | 105.38 | 19 | 3 |
| pgjdbc-tuned | lane3/1 | 21000 | 494 | 2769 | 6455 | 84.72 | 434 | 3 |
| sqlx | lane3/1 | 17000 | 971 | 5931 | 9896 | 126.19 | 9 | 3 |
| r2dbc-postgresql | lane3/1 | 5250 | 1270 | 7438 | 13042 | 403.07 | 482 | 3 |

## S1 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/1 | 72000 | 322 | 1712 | 4620 | 24.83 | 10 | 3 |
| zero-postgres | lane3/1 | 69840 | 297 | 1237 | 4555 | 22.13 | 7 | 3 |
| karez-shared | lane3/1 | 64000 | 420 | 1696 | 3752 | 47.87 | 13 | 3 |
| pgjdbc-tuned | lane3/1 | 64000 | 342 | 1958 | 4080 | 36.40 | 422 | 3 |
| r2dbc-postgresql | lane3/1 | 34000 | 301 | 1663 | 4555 | 53.87 | 666 | 3 |
| sqlx | lane3/1 | 24250 | 528 | 1827 | 4047 | 81.10 | 8 | 3 |

## S2 unpipelined, 1 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/1 | 5248 | 228 | 1188 | 4555 | 26.70 | 5 | 3 |
| zero-postgres | lane3/1 | 5000 | 237 | 1073 | 3817 | 36.70 | 5 | 3 |
| pgjdbc-tuned | lane3/1 | 4940 | 226 | 1028 | 3523 | 32.04 | 280 | 3 |
| karez-shared | lane3/1 | 4608 | 249 | 668 | 2056 | 51.85 | 5 | 3 |
| r2dbc-postgresql | lane3/1 | 3125 | 297 | 528 | 2056 | 90.64 | 447 | 3 |
| sqlx | lane3/1 | 1879 | 412 | 1122 | 3424 | 97.10 | 6 | 3 |

## S2 unpipelined, 8 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| zero-postgres | lane3/1 | 33872 | 264 | 1303 | 4178 | 24.88 | 6 | 3 |
| pgjdbc-tuned | lane3/1 | 32980 | 264 | 1794 | 3850 | 31.36 | 401 | 3 |
| karez-blocking | lane3/1 | 31000 | 268 | 1450 | 4882 | 28.00 | 6 | 3 |
| karez-shared | lane3/1 | 29100 | 293 | 1483 | 3949 | 53.41 | 6 | 3 |
| r2dbc-postgresql | lane3/1 | 12000 | 309 | 971 | 2834 | 95.35 | 666 | 3 |
| sqlx | lane3/1 | 11761 | 461 | 1401 | 2212 | 88.62 | 6 | 3 |

## S2 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/1 | 54888 | 330 | 1253 | 2671 | 28.57 | 10 | 3 |
| zero-postgres | lane3/1 | 52690 | 317 | 1417 | 3588 | 24.57 | 7 | 3 |
| karez-shared | lane3/1 | 52000 | 489 | 1761 | 3686 | 58.16 | 13 | 3 |
| pgjdbc-tuned | lane3/1 | 50440 | 342 | 1892 | 3981 | 39.55 | 424 | 3 |
| sqlx | lane3/1 | 22582 | 561 | 1860 | 4882 | 82.64 | 8 | 3 |
| r2dbc-postgresql | lane3/1 | 18000 | 326 | 872 | 3097 | 86.60 | 512 | 3 |

## S2 unpipelined, 512 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/1 | 37784 | 346 | 1384 | 3719 | 37.69 | 29 | 3 |
| zero-postgres | lane3/1 | 36507 | 338 | 1778 | 7766 | 29.41 | 15 | 3 |
| karez-shared | lane3/1 | 31000 | 510 | 1843 | 3686 | 92.33 | 86 | 3 |
| pgjdbc-tuned | lane3/1 | 21641 | 305 | 1450 | 17695 | 51.85 | 499 | 3 |
| sqlx | lane3/1 | 18430 | 618 | 1778 | 4948 | 88.02 | 21 | 3 |
| r2dbc-postgresql | lane3/1 | 15500 | 334 | 872 | 3162 | 86.44 | 515 | 3 |

## S7 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/1 | 45634 | 371 | 1647 | 2834 | 34.35 | 12 | 3 |
| karez-shared | lane3/1 | 44265 | 489 | 1761 | 4014 | 64.40 | 16 | 3 |
| pgjdbc-tuned | lane3/1 | 30000 | 322 | 1319 | 3490 | 49.74 | 462 | 3 |
| sqlx | lane3/1 | 19400 | 618 | 1843 | 3883 | 95.44 | 11 | 3 |
| r2dbc-postgresql | lane3/1 | 9500 | 346 | 758 | 3817 | 112.92 | 526 | 3 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane3/1 | 1000000 | 0.535 | 1,868,111 | 0.78 | 7 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane3/1 | 1000000 | 0.735 | 1,361,285 | 0.34 | 4 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/1 | 1000000 | 0.755 | 1,325,205 | 0.42 | 4 | 3 | ok |
| sqlx (sqlx 0.8) | lane3/1 | 1000000 | 0.800 | 1,249,375 | 0.89 | 6 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane3/1 | 1000000 | 0.996 | 1,004,319 | 0.90 | 1722 | 3 | ok |
| r2dbc-postgresql (r2dbc-postgresql 1.0) | lane3/1 | 1000000 | 2.594 | 385,460 | 3.01 | 1975 | 3 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-mrow (karez-pg 0.0.0 (row view)) | lane3/1 | 100000 | 0.369 | 271,003 | 0.60 | 10 | 3 | ok |
| sqlx (sqlx 0.8) | lane3/1 | 100000 | 0.397 | 252,207 | 1.10 | 7 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane3/1 | 100000 | 0.569 | 175,840 | 9.80 | 321 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/1 | 100000 | 0.680 | 147,124 | 1.10 | 5 | 3 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/1 | 3000000 | 3.446 | 870,650 | 0.82 | 5 | 3 | ok |
| sqlx (sqlx 0.8) | lane3/1 | 3000000 | 3.550 | 845,142 | 0.83 | 6 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane3/1 | 3000000 | 4.296 | 698,292 | 1.37 | 1960 | 3 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane3/1 | 13069067 | 9.637 | 1,356,092 | 1.29 | 7 | 3 | ok |
| sqlx (sqlx 0.8) | lane3/1 | 13069067 | 13.685 | 955,013 | 1.11 | 6 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/1 | 13069067 | 14.386 | 908,445 | 0.71 | 5 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane3/1 | 13069067 | 14.517 | 900,247 | 0.56 | 4 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane3/1 | 13069067 | 19.762 | 661,316 | 0.98 | 3811 | 3 | ok |
