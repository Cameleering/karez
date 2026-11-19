# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R1 pipelined, 8 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| tokio-postgres | lane3/2 | 64000 | 358 | 2540 | 5472 | 38.99 | 1148 | 3 |
| karez-shared | lane3/2 | 60000 | 815 | 2900 | 5079 | 45.20 | 12 | 3 |

## R1 unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/2 | 52690 | 367 | 1892 | 3523 | 31.30 | 11 | 3 |
| libpq | lane3/2 | 52380 | 362 | 2802 | 5014 | 28.79 | 18 | 3 |
| tokio-postgres | lane3/2 | 51110 | 317 | 1663 | 3195 | 31.22 | 8 | 3 |
| pgjdbc | lane3/2 | 46000 | 330 | 2056 | 5472 | 42.71 | 427 | 3 |
| karez-shared | lane3/2 | 38000 | 408 | 1237 | 2638 | 70.00 | 14 | 3 |
| asyncpg | lane3/2 | 11640 | 514 | 1745 | 2671 | 90.85 | 30 | 3 |

## R1 unpipelined, 512 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane3/2 | 37636 | 774 | 2933 | 13304 | 88.29 | 86 | 3 |
| karez-blocking | lane3/2 | 34000 | 367 | 1696 | 3883 | 40.38 | 30 | 3 |
| libpq | lane3/2 | 34000 | 367 | 1942 | 4293 | 33.59 | 38 | 3 |
| tokio-postgres | lane3/2 | 32856 | 362 | 2572 | 5865 | 38.05 | 23 | 3 |
| pgjdbc | lane3/2 | 27160 | 338 | 2605 | 15532 | 54.23 | 499 | 3 |
| asyncpg | lane3/2 | 10952 | 922 | 2507 | 4112 | 96.20 | 40 | 3 |

## R2 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/2 | 10185 | 1020 | 6783 | 12648 | 156.59 | 14 | 3 |
| karez-shared | lane3/2 | 10185 | 1270 | 7700 | 12911 | 229.10 | 19 | 3 |
| libpq | lane3/2 | 10185 | 963 | 7111 | 10945 | 127.57 | 18 | 3 |
| tokio-postgres | lane3/2 | 10185 | 889 | 7766 | 12386 | 187.34 | 9 | 3 |
| pgjdbc | lane3/2 | 9000 | 856 | 4620 | 10813 | 209.03 | 440 | 3 |
| asyncpg | lane3/2 | 775 | 2408 | 6259 | 8094 | 1123.08 | 36 | 3 |

## R4 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane3/2 | 28227 | 913 | 7373 | 11731 | 103.63 | 20 | 3 |
| tokio-postgres | lane3/2 | 28227 | 668 | 5997 | 11338 | 63.75 | 9 | 3 |
| karez-blocking | lane3/2 | 28130 | 627 | 5079 | 9372 | 58.41 | 15 | 3 |
| libpq | lane3/2 | 27286 | 586 | 6062 | 11469 | 50.69 | 19 | 3 |
| pgjdbc | lane3/2 | 26468 | 741 | 6914 | 12648 | 96.03 | 436 | 3 |
| asyncpg | lane3/2 | 2624 | 1368 | 5669 | 8225 | 311.92 | 33 | 3 |

## S1 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| libpq | lane3/2 | 75272 | 342 | 1892 | 5341 | 26.65 | 21 | 3 |
| tokio-postgres | lane3/2 | 75272 | 305 | 1548 | 3523 | 28.26 | 8 | 3 |
| karez-blocking | lane3/2 | 69970 | 317 | 1483 | 3785 | 24.84 | 10 | 3 |
| karez-shared | lane3/2 | 65960 | 432 | 1876 | 4178 | 47.12 | 13 | 3 |
| pgjdbc | lane3/2 | 62080 | 317 | 1663 | 5014 | 36.28 | 418 | 3 |
| asyncpg | lane3/2 | 17000 | 791 | 1499 | 2179 | 62.62 | 30 | 3 |

## S2 unpipelined, 1 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| libpq | lane3/2 | 5312 | 222 | 1036 | 2998 | 24.86 | 11 | 3 |
| karez-blocking | lane3/2 | 5175 | 222 | 668 | 1843 | 26.30 | 5 | 3 |
| tokio-postgres | lane3/2 | 5175 | 234 | 1761 | 2998 | 36.43 | 5 | 3 |
| pgjdbc | lane3/2 | 5020 | 230 | 1794 | 5341 | 32.73 | 284 | 3 |
| karez-shared | lane3/2 | 4750 | 247 | 1696 | 4178 | 51.36 | 5 | 3 |
| asyncpg | lane3/2 | 3864 | 285 | 602 | 3490 | 91.03 | 29 | 3 |

## S2 unpipelined, 8 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| tokio-postgres | lane3/2 | 34000 | 264 | 1417 | 2900 | 28.32 | 6 | 3 |
| karez-blocking | lane3/2 | 33872 | 261 | 1384 | 4948 | 26.10 | 6 | 3 |
| libpq | lane3/2 | 32980 | 255 | 1155 | 3097 | 25.95 | 14 | 3 |
| pgjdbc | lane3/2 | 32856 | 264 | 1548 | 3424 | 31.05 | 406 | 3 |
| karez-shared | lane3/2 | 30070 | 297 | 1729 | 3523 | 52.36 | 6 | 3 |
| asyncpg | lane3/2 | 14065 | 449 | 1204 | 2179 | 75.96 | 29 | 3 |

## S2 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/2 | 56260 | 354 | 1532 | 2900 | 29.17 | 10 | 3 |
| tokio-postgres | lane3/2 | 54572 | 313 | 1761 | 5865 | 30.48 | 8 | 3 |
| karez-shared | lane3/2 | 51110 | 453 | 1827 | 4358 | 57.22 | 13 | 3 |
| pgjdbc | lane3/2 | 48927 | 322 | 1319 | 2540 | 39.31 | 418 | 3 |
| libpq | lane3/2 | 46000 | 297 | 1036 | 2507 | 28.03 | 18 | 3 |
| asyncpg | lane3/2 | 15054 | 545 | 1876 | 2507 | 70.18 | 30 | 3 |

## S2 unpipelined, 512 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/2 | 39518 | 350 | 1384 | 2933 | 36.76 | 29 | 3 |
| libpq | lane3/2 | 36860 | 350 | 1597 | 3621 | 33.48 | 39 | 3 |
| tokio-postgres | lane3/2 | 36507 | 342 | 1958 | 3817 | 39.26 | 22 | 3 |
| karez-shared | lane3/2 | 36000 | 510 | 1794 | 4358 | 80.29 | 86 | 3 |
| pgjdbc | lane3/2 | 15500 | 276 | 1139 | 25297 | 53.91 | 487 | 3 |
| asyncpg | lane3/2 | 14065 | 905 | 1810 | 2769 | 74.90 | 38 | 3 |

## S7 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/2 | 44265 | 350 | 1319 | 2507 | 34.07 | 12 | 3 |
| karez-shared | lane3/2 | 44265 | 473 | 1909 | 4555 | 63.32 | 16 | 3 |
| tokio-postgres | lane3/2 | 44265 | 326 | 1647 | 3817 | 36.08 | 10 | 3 |
| libpq | lane3/2 | 43808 | 367 | 1466 | 3293 | 39.33 | 23 | 3 |
| pgjdbc | lane3/2 | 29000 | 305 | 1036 | 3195 | 48.07 | 460 | 3 |
| asyncpg | lane3/2 | 5704 | 643 | 1237 | 2605 | 181.67 | 35 | 3 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane3/2 | 1000000 | 0.528 | 1,893,222 | 0.76 | 7 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/2 | 1000000 | 0.669 | 1,493,875 | 0.38 | 5 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane3/2 | 1000000 | 0.705 | 1,418,239 | 0.72 | 5 | 3 | ok |
| libpq (libpq 180007) | lane3/2 | 1000000 | 0.750 | 1,334,223 | 0.51 | 320 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane3/2 | 1000000 | 0.918 | 1,089,325 | 0.35 | 4 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane3/2 | 1000000 | 1.487 | 672,314 | 2.68 | 1770 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane3/2 | 1000000 | 2.214 | 451,610 | 2.20 | 520 | 3 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-mrow (karez-pg 0.0.0 (row view)) | lane3/2 | 100000 | 0.359 | 278,707 | 0.60 | 10 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane3/2 | 100000 | 0.579 | 172,592 | 3.40 | 29 | 3 | ok |
| libpq (libpq 180007) | lane3/2 | 100000 | 0.619 | 161,473 | 0.80 | 10 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/2 | 100000 | 0.673 | 148,677 | 1.00 | 5 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane3/2 | 100000 | 0.880 | 113,662 | 6.00 | 313 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane3/2 | 100000 | 1.661 | 60,208 | 7.40 | 8 | 3 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/2 | 3000000 | 3.430 | 874,585 | 0.78 | 5 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane3/2 | 3000000 | 3.796 | 790,306 | 0.89 | 6 | 3 | ok |
| libpq (libpq 180007) | lane3/2 | 3000000 | 4.401 | 681,741 | 1.38 | 10 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane3/2 | 3000000 | 5.738 | 522,803 | 1.82 | 30 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane3/2 | 3000000 | 5.945 | 504,643 | 1.84 | 1959 | 3 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane3/2 | 13069067 | 9.747 | 1,340,843 | 1.21 | 7 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/2 | 13069067 | 12.597 | 1,037,491 | 0.64 | 5 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane3/2 | 13069067 | 13.923 | 938,640 | 0.52 | 5 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane3/2 | 13069067 | 14.008 | 932,952 | 1.00 | 5 | 3 | ok |
| libpq (libpq 180007) | lane3/2 | 13069067 | 22.398 | 583,500 | 1.32 | 6541 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane3/2 | 13069067 | 129.365 | 101,025 | 9.90 | 42 | 3 | ok |

_failed: pgjdbc s8: SLF4J: Failed to load class "org.slf4j.impl.StaticLoggerBinder".
SLF4J: Defaulting to no-operation (NOP) logger implementation
SLF4J: See http://www.slf4j.org/codes.html#StaticLoggerBinder for further_
