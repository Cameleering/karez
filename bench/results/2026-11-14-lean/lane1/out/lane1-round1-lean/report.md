# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R1 pipelined, 8 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane1/1 | 60000 | 684 | 2998 | 6128 | 44.80 | 11 | 3 |
| tokio-postgres | lane1/1 | 53200 | 305 | 1548 | 4227 | 37.63 | 955 | 3 |

## R1 unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 46930 | 330 | 1028 | 2277 | 29.04 | 11 | 3 |
| karez-shared | lane1/1 | 46930 | 449 | 1581 | 2769 | 62.63 | 14 | 3 |
| libpq | lane1/1 | 45600 | 330 | 1090 | 2310 | 25.98 | 18 | 3 |
| tokio-postgres | lane1/1 | 45600 | 305 | 668 | 1548 | 28.85 | 8 | 3 |
| pgjdbc | lane1/1 | 43320 | 326 | 1401 | 2933 | 37.66 | 424 | 3 |
| asyncpg | lane1/1 | 13000 | 987 | 1532 | 2310 | 80.34 | 30 | 3 |

## R1 unpipelined, 512 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 32000 | 375 | 2408 | 6390 | 35.88 | 30 | 3 |
| karez-shared | lane1/1 | 32000 | 465 | 2310 | 29753 | 79.83 | 86 | 3 |
| tokio-postgres | lane1/1 | 30400 | 342 | 1303 | 3064 | 30.57 | 23 | 3 |
| libpq | lane1/1 | 30000 | 358 | 1745 | 4489 | 28.50 | 38 | 3 |
| pgjdbc | lane1/1 | 26064 | 301 | 725 | 5014 | 45.09 | 503 | 3 |
| asyncpg | lane1/1 | 11400 | 514 | 2179 | 2802 | 92.33 | 40 | 3 |

## R2 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 8550 | 881 | 7111 | 14746 | 136.18 | 14 | 3 |
| karez-shared | lane1/1 | 8550 | 922 | 3686 | 8847 | 206.70 | 19 | 3 |
| libpq | lane1/1 | 8550 | 848 | 6521 | 13173 | 106.12 | 18 | 3 |
| tokio-postgres | lane1/1 | 8550 | 799 | 3654 | 9634 | 174.48 | 9 | 3 |
| pgjdbc | lane1/1 | 7331 | 815 | 3260 | 7307 | 162.82 | 429 | 3 |
| asyncpg | lane1/1 | 875 | 3424 | 7373 | 8716 | 1043.78 | 36 | 3 |

## R3 unpipelined, 8 connections (p99 bound 60.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 0 | 7569 | 1182794 | 1518338 | 36.00 | 5 | 3 |
| karez-shared | lane1/1 | 0 | 7242 | 1149239 | 1434452 | 71.23 | 6 | 3 |
| libpq | lane1/1 | 0 | 6849 | 1166017 | 1501561 | 36.26 | 10 | 3 |
| tokio-postgres | lane1/1 | 0 | 7045 | 1115685 | 1535115 | 72.65 | 6 | 3 |
| pgjdbc | lane1/1 | 0 | 7176 | 1082130 | 1518338 | 169.12 | 108 | 3 |
| asyncpg | lane1/1 | 0 | 6980 | 1266680 | 1551892 | 159.73 | 29 | 3 |

## R4 unpipelined, 64 connections (p99 bound 20.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| pgjdbc | lane1/1 | 469 | 489 | 14352 | 354419 | 101.08 | 244 | 3 |
| libpq | lane1/1 | 445 | 518 | 18481 | 354419 | 58.84 | 15 | 3 |
| asyncpg | lane1/1 | 445 | 627 | 18481 | 362807 | 303.25 | 32 | 3 |
| karez-blocking | lane1/1 | did not meet the bound | | | | | | 0 |
| karez-shared | lane1/1 | did not meet the bound | | | | | | 0 |
| tokio-postgres | lane1/1 | did not meet the bound | | | | | | 0 |

## S1 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 79420 | 305 | 1044 | 2040 | 22.89 | 10 | 3 |
| tokio-postgres | lane1/1 | 79420 | 297 | 1106 | 2736 | 27.41 | 8 | 3 |
| karez-shared | lane1/1 | 76000 | 403 | 1450 | 2802 | 43.72 | 13 | 3 |
| libpq | lane1/1 | 76000 | 297 | 1270 | 2769 | 24.83 | 21 | 3 |
| pgjdbc | lane1/1 | 65160 | 301 | 1057 | 2376 | 34.14 | 419 | 3 |
| asyncpg | lane1/1 | did not meet the bound | | | | | | 0 |

## S2 unpipelined, 1 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 5000 | 222 | 346 | 2056 | 20.97 | 5 | 3 |
| tokio-postgres | lane1/1 | 5000 | 239 | 1237 | 5276 | 27.82 | 5 | 3 |
| pgjdbc | lane1/1 | 5000 | 230 | 1483 | 5276 | 27.42 | 283 | 3 |
| libpq | lane1/1 | 4964 | 230 | 412 | 3260 | 22.75 | 11 | 3 |
| karez-shared | lane1/1 | 4750 | 247 | 905 | 3949 | 35.87 | 5 | 3 |
| asyncpg | lane1/1 | 4000 | 301 | 1036 | 2998 | 83.41 | 29 | 3 |

## S2 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 50540 | 338 | 1122 | 2212 | 27.94 | 10 | 3 |
| karez-shared | lane1/1 | 50540 | 412 | 1450 | 3129 | 53.56 | 13 | 3 |
| libpq | lane1/1 | 49400 | 313 | 930 | 2441 | 24.83 | 18 | 3 |
| tokio-postgres | lane1/1 | 48013 | 297 | 856 | 2802 | 27.32 | 8 | 3 |
| pgjdbc | lane1/1 | 44584 | 309 | 1974 | 3949 | 35.01 | 420 | 3 |
| asyncpg | lane1/1 | 15200 | 545 | 1368 | 2179 | 69.44 | 30 | 3 |

## S2 unpipelined, 512 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-shared | lane1/1 | 36100 | 428 | 1565 | 3686 | 65.72 | 86 | 3 |
| karez-blocking | lane1/1 | 34200 | 338 | 1004 | 2277 | 31.89 | 29 | 3 |
| tokio-postgres | lane1/1 | 34200 | 334 | 1663 | 4489 | 29.12 | 22 | 3 |
| libpq | lane1/1 | 32000 | 338 | 1073 | 3097 | 27.91 | 39 | 3 |
| pgjdbc | lane1/1 | 27436 | 317 | 1663 | 24773 | 39.87 | 499 | 3 |
| asyncpg | lane1/1 | 14000 | 651 | 1581 | 1991 | 75.97 | 38 | 3 |

## S7 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 39710 | 350 | 1221 | 2703 | 30.18 | 12 | 3 |
| karez-shared | lane1/1 | 39710 | 420 | 1499 | 2638 | 57.78 | 16 | 3 |
| tokio-postgres | lane1/1 | 39710 | 322 | 1073 | 2507 | 33.92 | 10 | 3 |
| libpq | lane1/1 | 38000 | 338 | 881 | 1909 | 34.26 | 22 | 3 |
| pgjdbc | lane1/1 | 32000 | 322 | 987 | 2867 | 43.19 | 460 | 3 |
| asyncpg | lane1/1 | 6500 | 905 | 1909 | 2376 | 164.06 | 35 | 3 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/1 | 1000000 | 0.823 | 1,215,658 | 0.41 | 4 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/1 | 1000000 | 1.052 | 951,022 | 0.68 | 5 | 3 | ok |
| libpq (libpq 180007) | lane1/1 | 1000000 | 1.115 | 896,781 | 0.55 | 320 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/1 | 1000000 | 1.423 | 702,938 | 2.44 | 1768 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/1 | 1000000 | 2.082 | 480,400 | 2.08 | 520 | 3 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| asyncpg (asyncpg 0.32.0) | lane1/1 | 100000 | 0.724 | 138,064 | 3.20 | 29 | 3 | ok |
| libpq (libpq 180007) | lane1/1 | 100000 | 0.780 | 128,222 | 0.90 | 10 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/1 | 100000 | 0.847 | 118,092 | 1.10 | 5 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/1 | 100000 | 1.046 | 95,557 | 6.30 | 317 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/1 | 100000 | 1.849 | 54,083 | 7.00 | 7 | 3 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/1 | 3000000 | 4.806 | 624,272 | 0.75 | 5 | 3 | ok |
| libpq (libpq 180007) | lane1/1 | 3000000 | 5.081 | 590,412 | 1.35 | 10 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/1 | 3000000 | 5.090 | 589,391 | 0.86 | 5 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/1 | 3000000 | 5.353 | 560,423 | 1.61 | 1936 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/1 | 3000000 | 5.536 | 541,917 | 1.72 | 31 | 3 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/1 | 13069067 | 17.815 | 733,587 | 0.55 | 5 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/1 | 13069067 | 18.727 | 697,877 | 0.93 | 5 | 3 | ok |
| libpq (libpq 180007) | lane1/1 | 13069067 | 27.504 | 475,175 | 1.24 | 6541 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/1 | 13069067 | 123.071 | 106,192 | 9.42 | 42 | 3 | ok |

_failed: pgjdbc s8: SLF4J: Failed to load class "org.slf4j.impl.StaticLoggerBinder".
SLF4J: Defaulting to no-operation (NOP) logger implementation
SLF4J: See http://www.slf4j.org/codes.html#StaticLoggerBinder for further_
