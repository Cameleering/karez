# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R1 pipelined, 8 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| tokio-postgres | lane1/1 | 65960 | 334 | 2089 | 3785 | 40.24 | 1183 | 3 |
| karez-shared | lane1/1 | 60218 | 766 | 2900 | 4555 | 47.70 | 13 | 3 |

## R1 unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| tokio-postgres | lane1/1 | 60140 | 309 | 2867 | 5734 | 34.31 | 8 | 3 |
| karez-blocking | lane1/1 | 58336 | 334 | 1171 | 2441 | 33.67 | 11 | 3 |
| libpq | lane1/1 | 58200 | 338 | 2933 | 5734 | 31.71 | 19 | 3 |
| karez-shared | lane1/1 | 54572 | 733 | 2769 | 3883 | 68.83 | 14 | 3 |
| pgjdbc | lane1/1 | 50000 | 322 | 1171 | 2867 | 45.96 | 426 | 3 |
| asyncpg | lane1/1 | 12000 | 1319 | 2933 | 3883 | 88.04 | 31 | 3 |

## R1 unpipelined, 512 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 45163 | 338 | 1155 | 3129 | 41.23 | 31 | 3 |
| tokio-postgres | lane1/1 | 45163 | 317 | 2966 | 5603 | 39.27 | 23 | 3 |
| libpq | lane1/1 | 42680 | 322 | 987 | 2245 | 36.49 | 40 | 3 |
| karez-shared | lane1/1 | 36860 | 700 | 2867 | 55312 | 91.72 | 85 | 3 |
| pgjdbc | lane1/1 | 24788 | 285 | 2146 | 40108 | 55.84 | 500 | 3 |
| asyncpg | lane1/1 | 9127 | 807 | 1581 | 1974 | 116.08 | 40 | 3 |

## R2 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 10820 | 987 | 5931 | 9634 | 172.45 | 14 | 3 |
| karez-shared | lane1/1 | 10820 | 1335 | 6717 | 10813 | 253.50 | 20 | 3 |
| libpq | lane1/1 | 10820 | 856 | 4424 | 9241 | 139.65 | 18 | 3 |
| tokio-postgres | lane1/1 | 10820 | 831 | 5079 | 8159 | 208.64 | 9 | 3 |
| pgjdbc | lane1/1 | 8214 | 774 | 2277 | 6717 | 220.68 | 439 | 3 |
| asyncpg | lane1/1 | 735 | 2966 | 5997 | 7373 | 1179.22 | 36 | 3 |

## R4 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| tokio-postgres | lane1/1 | 32000 | 700 | 7635 | 13566 | 69.20 | 9 | 3 |
| karez-blocking | lane1/1 | 31991 | 684 | 6062 | 11469 | 65.09 | 15 | 3 |
| karez-shared | lane1/1 | 31040 | 1106 | 6062 | 9765 | 110.52 | 19 | 3 |
| libpq | lane1/1 | 31040 | 635 | 6652 | 11469 | 57.16 | 20 | 3 |
| pgjdbc | lane1/1 | 25000 | 518 | 2245 | 4686 | 91.60 | 442 | 3 |
| asyncpg | lane1/1 | 2375 | 1548 | 7963 | 27656 | 345.96 | 33 | 3 |

## S1 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 97000 | 362 | 1516 | 2703 | 26.94 | 10 | 3 |
| libpq | lane1/1 | 89240 | 391 | 1892 | 3097 | 30.77 | 22 | 3 |
| tokio-postgres | lane1/1 | 89240 | 305 | 1286 | 3260 | 29.81 | 8 | 3 |
| karez-shared | lane1/1 | 76000 | 553 | 1679 | 2802 | 48.43 | 13 | 3 |
| pgjdbc | lane1/1 | 68000 | 289 | 1204 | 2769 | 37.55 | 427 | 3 |
| asyncpg | lane1/1 | 14584 | 537 | 1942 | 3162 | 72.72 | 30 | 3 |

## S2 unpipelined, 1 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| libpq | lane1/1 | 5645 | 216 | 1630 | 5472 | 23.75 | 11 | 3 |
| karez-blocking | lane1/1 | 5500 | 214 | 651 | 7242 | 26.39 | 5 | 3 |
| tokio-postgres | lane1/1 | 5500 | 226 | 1483 | 7242 | 36.84 | 6 | 3 |
| pgjdbc | lane1/1 | 5020 | 226 | 1696 | 4751 | 33.34 | 283 | 3 |
| karez-shared | lane1/1 | 4750 | 245 | 1106 | 5210 | 49.66 | 5 | 3 |
| asyncpg | lane1/1 | 3375 | 375 | 913 | 1401 | 115.01 | 29 | 3 |

## S2 unpipelined, 8 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| tokio-postgres | lane1/1 | 38800 | 259 | 1991 | 3523 | 24.97 | 6 | 3 |
| karez-blocking | lane1/1 | 37636 | 251 | 668 | 1516 | 26.62 | 6 | 3 |
| libpq | lane1/1 | 36860 | 245 | 979 | 2605 | 25.92 | 14 | 3 |
| pgjdbc | lane1/1 | 36000 | 249 | 1581 | 3457 | 32.28 | 404 | 3 |
| karez-shared | lane1/1 | 32000 | 285 | 938 | 3719 | 55.07 | 6 | 3 |
| asyncpg | lane1/1 | 13095 | 610 | 1499 | 1843 | 81.38 | 29 | 3 |

## S2 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 65960 | 330 | 1335 | 3424 | 31.33 | 10 | 3 |
| tokio-postgres | lane1/1 | 64000 | 301 | 1516 | 3555 | 33.33 | 8 | 3 |
| libpq | lane1/1 | 62080 | 313 | 1368 | 2441 | 30.66 | 19 | 3 |
| karez-shared | lane1/1 | 60218 | 586 | 1581 | 2638 | 59.58 | 13 | 3 |
| pgjdbc | lane1/1 | 54000 | 317 | 1384 | 4014 | 42.38 | 423 | 3 |
| asyncpg | lane1/1 | 12321 | 545 | 1761 | 2408 | 86.14 | 30 | 3 |

## S2 unpipelined, 512 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 50000 | 334 | 1171 | 3293 | 37.86 | 29 | 3 |
| libpq | lane1/1 | 50000 | 322 | 1483 | 5669 | 35.01 | 40 | 3 |
| tokio-postgres | lane1/1 | 49807 | 301 | 1483 | 3883 | 38.15 | 22 | 3 |
| karez-shared | lane1/1 | 38000 | 553 | 1761 | 2900 | 84.53 | 88 | 3 |
| asyncpg | lane1/1 | 11865 | 733 | 1352 | 1761 | 89.54 | 38 | 3 |
| pgjdbc | lane1/1 | did not meet the bound | | | | | | 0 |

## S7 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 50809 | 362 | 1597 | 3064 | 36.43 | 12 | 3 |
| tokio-postgres | lane1/1 | 50000 | 313 | 1876 | 3457 | 38.89 | 10 | 3 |
| libpq | lane1/1 | 49284 | 354 | 1712 | 3752 | 43.81 | 24 | 3 |
| karez-shared | lane1/1 | 48500 | 510 | 1450 | 3162 | 66.96 | 16 | 3 |
| pgjdbc | lane1/1 | 28000 | 293 | 518 | 2834 | 52.53 | 463 | 3 |
| asyncpg | lane1/1 | 4750 | 618 | 1696 | 2310 | 215.00 | 35 | 3 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane1/1 | 1000000 | 0.493 | 2,029,221 | 0.83 | 7 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/1 | 1000000 | 0.608 | 1,644,466 | 0.40 | 4 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/1 | 1000000 | 0.609 | 1,642,576 | 0.40 | 4 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/1 | 1000000 | 0.645 | 1,549,907 | 0.90 | 5 | 3 | ok |
| libpq (libpq 180007) | lane1/1 | 1000000 | 0.760 | 1,315,097 | 0.60 | 321 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/1 | 1000000 | 1.523 | 656,685 | 2.85 | 1769 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/1 | 1000000 | 2.317 | 431,537 | 2.32 | 520 | 3 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-mrow (karez-pg 0.0.0 (row view)) | lane1/1 | 100000 | 0.418 | 239,177 | 0.60 | 10 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/1 | 100000 | 0.649 | 154,154 | 3.90 | 29 | 3 | ok |
| libpq (libpq 180007) | lane1/1 | 100000 | 0.654 | 152,788 | 1.00 | 10 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/1 | 100000 | 0.723 | 138,370 | 1.10 | 5 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/1 | 100000 | 0.976 | 102,470 | 6.50 | 318 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/1 | 100000 | 1.407 | 71,053 | 7.70 | 8 | 3 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/1 | 3000000 | 4.075 | 736,214 | 0.89 | 5 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/1 | 3000000 | 4.252 | 705,600 | 1.03 | 6 | 3 | ok |
| libpq (libpq 180007) | lane1/1 | 3000000 | 4.795 | 625,704 | 1.52 | 10 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/1 | 3000000 | 4.844 | 619,374 | 1.40 | 1956 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/1 | 3000000 | 6.152 | 487,615 | 1.95 | 31 | 3 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-part (karez-pg 0.0.0 (scan_partitioned, 8 partitions)) | lane1/1 | 13069067 | 9.661 | 1,352,807 | 1.31 | 7 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/1 | 13069067 | 12.189 | 1,072,211 | 0.54 | 4 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/1 | 13069067 | 14.540 | 898,866 | 0.73 | 5 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/1 | 13069067 | 14.646 | 892,300 | 1.17 | 5 | 3 | ok |
| libpq (libpq 180007) | lane1/1 | 13069067 | 20.980 | 622,930 | 1.43 | 6541 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/1 | 13069067 | 133.517 | 97,883 | 10.22 | 42 | 3 | ok |

_failed: pgjdbc s8: SLF4J: Failed to load class "org.slf4j.impl.StaticLoggerBinder".
SLF4J: Defaulting to no-operation (NOP) logger implementation
SLF4J: See http://www.slf4j.org/codes.html#StaticLoggerBinder for further_
