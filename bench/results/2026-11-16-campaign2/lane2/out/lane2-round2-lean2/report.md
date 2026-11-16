# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R1 pipelined, 8 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane2/2 | 60218 | 709 | 2998 | 5210 | 47.81 | 13 | 3 |

## R1 unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/2 | 54760 | 261 | 995 | 2671 | 32.46 | 11 | 3 |
| karez-shared | lane2/2 | 52690 | 627 | 2572 | 5145 | 69.19 | 14 | 3 |
| pgjdbc-tuned | lane2/2 | 47045 | 281 | 2966 | 6914 | 46.16 | 434 | 3 |
| sqlx | lane2/2 | 29206 | 561 | 2736 | 6324 | 85.88 | 8 | 3 |
| r2dbc-postgresql | lane2/2 | 18000 | 268 | 1499 | 3785 | 98.68 | 672 | 3 |

## R1 unpipelined, 512 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/2 | 34920 | 285 | 1532 | 4227 | 39.55 | 30 | 3 |
| karez-shared | lane2/2 | 34920 | 578 | 2310 | 77070 | 93.17 | 85 | 3 |
| r2dbc-postgresql | lane2/2 | 24000 | 358 | 2507 | 4555 | 106.67 | 535 | 3 |
| pgjdbc-tuned | lane2/2 | 23000 | 259 | 2474 | 19530 | 53.03 | 499 | 3 |
| sqlx | lane2/2 | 18430 | 498 | 2572 | 7766 | 93.14 | 21 | 3 |

## R2 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/2 | 9700 | 815 | 3785 | 10682 | 163.47 | 14 | 3 |
| karez-shared | lane2/2 | 8853 | 979 | 7111 | 14877 | 248.23 | 19 | 3 |
| pgjdbc-tuned | lane2/2 | 8000 | 815 | 2900 | 7111 | 209.19 | 440 | 3 |
| sqlx | lane2/2 | 7518 | 1384 | 5210 | 8454 | 333.06 | 9 | 3 |
| r2dbc-postgresql | lane2/2 | 1438 | 1581 | 7569 | 11600 | 1224.65 | 471 | 3 |

## R4 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/2 | 29100 | 586 | 5014 | 10027 | 63.60 | 15 | 3 |
| karez-shared | lane2/2 | 27286 | 807 | 4293 | 8585 | 109.75 | 19 | 3 |
| pgjdbc-tuned | lane2/2 | 23280 | 709 | 3883 | 9634 | 115.60 | 434 | 3 |
| sqlx | lane2/2 | 19476 | 954 | 6455 | 9896 | 133.72 | 9 | 3 |
| r2dbc-postgresql | lane2/2 | 4500 | 741 | 6128 | 11600 | 403.71 | 481 | 3 |

## S1 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/2 | 93120 | 264 | 1090 | 3391 | 26.40 | 10 | 3 |
| zero-postgres | lane2/2 | 81447 | 232 | 1139 | 7373 | 24.93 | 7 | 3 |
| karez-shared | lane2/2 | 77906 | 461 | 1630 | 3260 | 47.07 | 13 | 3 |
| pgjdbc-tuned | lane2/2 | 69363 | 243 | 1434 | 5276 | 36.65 | 421 | 3 |
| sqlx | lane2/2 | 36067 | 399 | 1335 | 2769 | 78.55 | 8 | 3 |
| r2dbc-postgresql | lane2/2 | 36000 | 224 | 1012 | 3129 | 54.91 | 666 | 3 |

## S2 unpipelined, 1 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/2 | 9700 | 163 | 1434 | 3752 | 22.03 | 5 | 3 |
| zero-postgres | lane2/2 | 8245 | 167 | 477 | 2114 | 33.14 | 5 | 3 |
| pgjdbc-tuned | lane2/2 | 8214 | 165 | 930 | 3031 | 31.30 | 390 | 3 |
| karez-shared | lane2/2 | 7518 | 179 | 498 | 2023 | 47.34 | 5 | 3 |
| r2dbc-postgresql | lane2/2 | 4750 | 216 | 522 | 1860 | 78.30 | 446 | 3 |
| sqlx | lane2/2 | 3194 | 257 | 709 | 3457 | 82.99 | 6 | 3 |

## S2 unpipelined, 8 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/2 | 48927 | 198 | 1810 | 7504 | 25.18 | 6 | 3 |
| zero-postgres | lane2/2 | 46560 | 200 | 1565 | 3359 | 25.57 | 6 | 3 |
| pgjdbc-tuned | lane2/2 | 43281 | 200 | 774 | 2769 | 32.12 | 405 | 3 |
| karez-shared | lane2/2 | 39518 | 237 | 1614 | 8094 | 53.36 | 6 | 3 |
| sqlx | lane2/2 | 15500 | 313 | 954 | 4489 | 82.65 | 6 | 3 |
| r2dbc-postgresql | lane2/2 | 12000 | 241 | 453 | 2507 | 90.89 | 675 | 3 |

## S2 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| zero-postgres | lane2/2 | 58200 | 239 | 1012 | 3949 | 25.69 | 7 | 3 |
| karez-shared | lane2/2 | 54959 | 412 | 1892 | 4555 | 59.87 | 13 | 3 |
| karez-blocking | lane2/2 | 54000 | 241 | 815 | 4882 | 30.78 | 10 | 3 |
| pgjdbc-tuned | lane2/2 | 48089 | 255 | 1401 | 4293 | 41.88 | 426 | 3 |
| sqlx | lane2/2 | 27286 | 412 | 1417 | 3621 | 86.16 | 8 | 3 |
| r2dbc-postgresql | lane2/2 | 19000 | 247 | 1270 | 3654 | 84.78 | 665 | 3 |

## S2 unpipelined, 512 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane2/2 | 39518 | 272 | 987 | 3391 | 37.25 | 29 | 3 |
| zero-postgres | lane2/2 | 33872 | 264 | 954 | 3129 | 28.66 | 15 | 3 |
| karez-shared | lane2/2 | 32000 | 444 | 1532 | 15270 | 91.87 | 86 | 3 |
| r2dbc-postgresql | lane2/2 | 25000 | 272 | 1237 | 4178 | 85.19 | 533 | 3 |
| pgjdbc-tuned | lane2/2 | 21000 | 232 | 537 | 3228 | 51.12 | 494 | 3 |
| sqlx | lane2/2 | 19759 | 473 | 1876 | 5800 | 89.85 | 21 | 3 |

## S7 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane2/2 | 46035 | 416 | 1368 | 4882 | 66.02 | 16 | 3 |
| karez-blocking | lane2/2 | 44620 | 268 | 1663 | 6128 | 35.29 | 12 | 3 |
| pgjdbc-tuned | lane2/2 | 31000 | 255 | 963 | 3162 | 50.64 | 462 | 3 |
| sqlx | lane2/2 | 20991 | 444 | 1974 | 3883 | 99.85 | 11 | 3 |
| r2dbc-postgresql | lane2/2 | 11500 | 276 | 502 | 1860 | 106.76 | 533 | 3 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane2/2 | 1000000 | 0.505 | 1,979,022 | 0.73 | 7 | 3 | ok |
| sqlx (sqlx 0.8) | lane2/2 | 1000000 | 0.812 | 1,232,134 | 0.87 | 6 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/2 | 1000000 | 0.850 | 1,175,917 | 0.47 | 5 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane2/2 | 1000000 | 0.915 | 1,093,016 | 0.56 | 4 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane2/2 | 1000000 | 1.050 | 952,472 | 0.99 | 1720 | 3 | ok |
| r2dbc-postgresql (r2dbc-postgresql 1.0) | lane2/2 | 1000000 | 2.873 | 348,056 | 3.03 | 1991 | 3 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-mrow (karez-pg 0.0.0 (row view)) | lane2/2 | 100000 | 0.395 | 252,908 | 0.60 | 10 | 3 | ok |
| sqlx (sqlx 0.8) | lane2/2 | 100000 | 0.429 | 232,883 | 0.90 | 7 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane2/2 | 100000 | 0.604 | 165,673 | 8.90 | 323 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/2 | 100000 | 0.729 | 137,231 | 1.00 | 5 | 3 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/2 | 3000000 | 4.444 | 675,098 | 0.82 | 5 | 3 | ok |
| sqlx (sqlx 0.8) | lane2/2 | 3000000 | 4.662 | 643,542 | 0.91 | 6 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane2/2 | 3000000 | 5.840 | 513,663 | 1.89 | 1959 | 3 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane2/2 | 13069067 | 9.685 | 1,349,357 | 1.07 | 7 | 3 | ok |
| sqlx (sqlx 0.8) | lane2/2 | 13069067 | 14.155 | 923,270 | 1.14 | 6 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane2/2 | 13069067 | 15.341 | 851,893 | 0.65 | 5 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane2/2 | 13069067 | 17.893 | 730,393 | 0.81 | 5 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane2/2 | 13069067 | 18.515 | 705,860 | 0.86 | 3796 | 3 | ok |
