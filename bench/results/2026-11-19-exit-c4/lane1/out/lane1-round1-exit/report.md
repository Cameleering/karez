# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## S1 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| tokio-postgres | lane1/x1 | 54959 | 214 | 1630 | 5210 | 32.22 | 9 | 3 |
| pgx | lane1/x1 | 54959 | 179 | 1122 | 4178 | 32.41 | 24 | 3 |
| karez-blocking | lane1/x1 | 54000 | 218 | 1843 | 4293 | 28.69 | 12 | 3 |
| libpq | lane1/x1 | 52000 | 210 | 1073 | 3555 | 30.59 | 18 | 3 |
| pgjdbc-tuned | lane1/x1 | 50809 | 218 | 1139 | 3686 | 37.94 | 409 | 3 |
| sqlx | lane1/x1 | 23018 | 375 | 1892 | 4227 | 83.56 | 10 | 3 |
| asyncpg | lane1/x1 | 19000 | 461 | 1401 | 2073 | 56.87 | 30 | 3 |

## S1T unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| tokio-postgres | lane1/x1 | 44000 | 230 | 1745 | 10158 | 36.34 | 12 | 3 |
| karez-blocking | lane1/x1 | 42680 | 230 | 1368 | 3883 | 32.02 | 14 | 3 |
| pgx | lane1/x1 | 42680 | 196 | 1417 | 4080 | 35.92 | 28 | 3 |
| libpq | lane1/x1 | 42000 | 243 | 1663 | 4047 | 38.45 | 23 | 3 |
| pgjdbc-tuned | lane1/x1 | 37182 | 243 | 1466 | 3391 | 44.33 | 455 | 3 |
| sqlx | lane1/x1 | 17000 | 408 | 1925 | 4555 | 91.97 | 12 | 3 |
| asyncpg | lane1/x1 | 7518 | 510 | 1712 | 2834 | 144.26 | 35 | 3 |

## S2 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/x1 | 40740 | 243 | 1860 | 4751 | 32.02 | 12 | 3 |
| tokio-postgres | lane1/x1 | 37636 | 230 | 1090 | 3457 | 28.31 | 9 | 3 |
| libpq | lane1/x1 | 37636 | 241 | 1286 | 4424 | 30.00 | 17 | 3 |
| pgjdbc-tuned | lane1/x1 | 37636 | 249 | 1565 | 4489 | 37.74 | 420 | 3 |
| pgx | lane1/x1 | 34000 | 214 | 930 | 2474 | 38.51 | 26 | 3 |
| asyncpg | lane1/x1 | 17341 | 676 | 1057 | 2736 | 61.80 | 30 | 3 |
| sqlx | lane1/x1 | 13500 | 403 | 1270 | 4424 | 83.18 | 10 | 3 |

## S3 unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| tokio-postgres | lane1/x1 | 50440 | 234 | 2114 | 4424 | 37.07 | 9 | 3 |
| pgx | lane1/x1 | 48000 | 206 | 2408 | 6259 | 37.67 | 25 | 3 |
| pgjdbc-tuned | lane1/x1 | 48000 | 253 | 2376 | 5210 | 42.16 | 421 | 3 |
| libpq | lane1/x1 | 46000 | 228 | 1221 | 3359 | 30.55 | 18 | 3 |
| karez-blocking | lane1/x1 | 44000 | 232 | 1925 | 5014 | 32.05 | 12 | 3 |
| sqlx | lane1/x1 | 18000 | 420 | 2540 | 6324 | 92.92 | 10 | 3 |
| asyncpg | lane1/x1 | 16490 | 750 | 1942 | 2540 | 64.99 | 30 | 3 |

## S3T unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| pgx | lane1/x1 | 38800 | 232 | 1991 | 6193 | 41.83 | 28 | 3 |
| pgjdbc-tuned | lane1/x1 | 38800 | 297 | 2867 | 7242 | 47.10 | 460 | 3 |
| tokio-postgres | lane1/x1 | 38000 | 255 | 2023 | 9241 | 39.26 | 12 | 3 |
| karez-blocking | lane1/x1 | 37636 | 276 | 2540 | 5800 | 35.20 | 14 | 3 |
| libpq | lane1/x1 | 31000 | 261 | 1368 | 3326 | 39.97 | 22 | 3 |
| sqlx | lane1/x1 | 16490 | 469 | 2212 | 5931 | 101.50 | 13 | 3 |
| asyncpg | lane1/x1 | 7292 | 651 | 2703 | 3359 | 150.56 | 36 | 3 |

## S7 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| tokio-postgres | lane1/x1 | 34000 | 251 | 1319 | 5079 | 36.89 | 12 | 3 |
| karez-blocking | lane1/x1 | 31871 | 268 | 1696 | 5734 | 37.13 | 14 | 3 |
| libpq | lane1/x1 | 29197 | 276 | 1044 | 2638 | 39.77 | 22 | 3 |
| pgx | lane1/x1 | 29100 | 251 | 1303 | 3490 | 42.47 | 35 | 3 |
| pgjdbc-tuned | lane1/x1 | 29100 | 289 | 1860 | 3752 | 45.83 | 454 | 3 |
| sqlx | lane1/x1 | 13279 | 465 | 1548 | 3785 | 89.58 | 13 | 3 |
| asyncpg | lane1/x1 | 7250 | 684 | 1450 | 3162 | 152.11 | 36 | 3 |

## S8 unpipelined, 512 connections, pool 32 (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| libpq | lane1/x1 | 42000 | 214 | 1384 | 5145 | 31.59 | 20 | 3 |
| pgx | lane1/x1 | 41400 | 200 | 1778 | 4227 | 38.94 | 30 | 3 |
| pgjdbc-tuned | lane1/x1 | 27471 | 222 | 1335 | 8225 | 43.22 | 467 | 3 |
| sqlx | lane1/x1 | 17000 | 375 | 1237 | 3588 | 84.62 | 9 | 3 |
| karez-blocking | lane1/x1 | 10000 | 189 | 281 | 1630 | 48.09 | 17 | 3 |
| asyncpg | lane1/x1 | 5932 | 823 | 1827 | 2408 | 175.57 | 31 | 3 |

## S8T unpipelined, 512 connections, pool 32 (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| libpq | lane1/x1 | 34682 | 232 | 1204 | 4424 | 40.08 | 27 | 3 |
| pgx | lane1/x1 | 31000 | 206 | 1335 | 3326 | 44.00 | 35 | 3 |
| pgjdbc-tuned | lane1/x1 | 21000 | 241 | 930 | 7832 | 52.76 | 487 | 3 |
| sqlx | lane1/x1 | 16490 | 416 | 1630 | 4014 | 91.90 | 11 | 3 |
| karez-blocking | lane1/x1 | 9500 | 200 | 309 | 1106 | 50.29 | 19 | 3 |
| asyncpg | lane1/x1 | 3274 | 905 | 1417 | 2474 | 284.19 | 34 | 3 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/x1 | 1000000 | 0.596 | 1,677,571 | 0.37 | 6 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x1 | 1000000 | 0.618 | 1,618,909 | 0.66 | 7 | 3 | ok |
| pgx (pgx v5) | lane1/x1 | 1000000 | 0.681 | 1,469,023 | 0.66 | 17 | 3 | ok |
| libpq (libpq 180007) | lane1/x1 | 1000000 | 0.746 | 1,340,123 | 0.55 | 320 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/x1 | 1000000 | 0.760 | 1,316,309 | 0.61 | 1625 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x1 | 1000000 | 0.769 | 1,300,728 | 0.48 | 6 | 3 | ok |
| sqlx (sqlx 0.8) | lane1/x1 | 1000000 | 0.918 | 1,089,562 | 0.77 | 7 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/x1 | 1000000 | 1.774 | 563,730 | 1.77 | 520 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/x1 | 1000000 | 2.035 | 491,352 | 1.72 | 826 | 3 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| sqlx (sqlx 0.8) | lane1/x1 | 100000 | 0.408 | 245,158 | 0.90 | 9 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/x1 | 100000 | 0.527 | 189,897 | 7.30 | 357 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/x1 | 100000 | 0.596 | 167,842 | 3.10 | 29 | 3 | ok |
| libpq (libpq 180007) | lane1/x1 | 100000 | 0.626 | 159,795 | 1.10 | 10 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x1 | 100000 | 0.686 | 145,730 | 1.00 | 7 | 3 | ok |
| pgx (pgx v5) | lane1/x1 | 100000 | 0.846 | 118,183 | 3.40 | 17 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/x1 | 100000 | 1.780 | 56,189 | 17.80 | 88 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x1 | 100000 | 2.080 | 48,075 | 6.90 | 9 | 3 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-binary (karez-pg 0.0.0 (binary COPY)) | lane1/x1 | 10000000 | 11.116 | 899,596 | 0.22 | 6 | 3 | ok |
| sqlx (sqlx 0.8) | lane1/x1 | 10000000 | 14.221 | 703,180 | 0.76 | 8 | 3 | ok |
| pgx (pgx v5) | lane1/x1 | 10000000 | 14.473 | 690,955 | 1.52 | 17 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x1 | 10000000 | 14.729 | 678,956 | 0.81 | 8 | 3 | ok |
| pgbulkinsert (pgbulkinsert 8.1.4 on pgjdbc 42.7.5) | lane1/x1 | 10000000 | 15.120 | 661,398 | 1.40 | 1934 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x1 | 10000000 | 15.278 | 654,553 | 0.68 | 6 | 3 | ok |
| libpq (libpq 180007) | lane1/x1 | 10000000 | 16.186 | 617,803 | 0.89 | 10 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/x1 | 10000000 | 16.230 | 616,147 | 1.16 | 1961 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/x1 | 10000000 | 16.746 | 597,161 | 1.11 | 1959 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/x1 | 10000000 | 18.090 | 552,798 | 1.66 | 31 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/x1 | 10000000 | 27.282 | 366,549 | 2.56 | 40 | 3 | ok |

### S4 over TLS (sslmode=verify-full)

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| tokio-postgres (tokio-postgres 0.7) | lane1/x1 | 1000000 | 0.694 | 1,441,545 | 0.64 | 8 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x1 | 1000000 | 0.702 | 1,424,096 | 0.33 | 7 | 3 | ok |
| pgx (pgx v5) | lane1/x1 | 1000000 | 0.727 | 1,375,188 | 0.67 | 18 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/x1 | 1000000 | 0.875 | 1,143,380 | 0.69 | 1927 | 3 | ok |
| libpq (libpq 180007) | lane1/x1 | 1000000 | 0.888 | 1,125,619 | 0.60 | 322 | 3 | ok |
| sqlx (sqlx 0.8) | lane1/x1 | 1000000 | 0.924 | 1,082,368 | 0.93 | 8 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/x1 | 1000000 | 1.061 | 942,418 | 0.47 | 7 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/x1 | 1000000 | 2.048 | 488,377 | 1.97 | 828 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/x1 | 1000000 | 2.232 | 448,089 | 2.23 | 524 | 3 | ok |

### S5 over TLS (sslmode=verify-full)

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| sqlx (sqlx 0.8) | lane1/x1 | 100000 | 0.386 | 259,202 | 0.80 | 10 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/x1 | 100000 | 0.546 | 183,050 | 8.10 | 343 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/x1 | 100000 | 0.627 | 159,363 | 3.40 | 33 | 3 | ok |
| libpq (libpq 180007) | lane1/x1 | 100000 | 0.656 | 152,393 | 0.90 | 12 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x1 | 100000 | 0.779 | 128,337 | 0.90 | 8 | 3 | ok |
| pgx (pgx v5) | lane1/x1 | 100000 | 0.862 | 116,073 | 3.70 | 17 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x1 | 100000 | 2.104 | 47,526 | 7.30 | 11 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/x1 | 100000 | 2.166 | 46,162 | 21.60 | 90 | 3 | ok |

### S6 over TLS (sslmode=verify-full)

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-binary (karez-pg 0.0.0 (binary COPY)) | lane1/x1 | 10000000 | 9.900 | 1,010,121 | 0.24 | 7 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x1 | 10000000 | 12.723 | 785,953 | 0.69 | 8 | 3 | ok |
| sqlx (sqlx 0.8) | lane1/x1 | 10000000 | 13.658 | 732,156 | 0.79 | 9 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x1 | 10000000 | 13.674 | 731,331 | 0.86 | 9 | 3 | ok |
| pgbulkinsert (pgbulkinsert 8.1.4 on pgjdbc 42.7.5) | lane1/x1 | 10000000 | 14.122 | 708,105 | 1.27 | 1954 | 3 | ok |
| pgx (pgx v5) | lane1/x1 | 10000000 | 15.060 | 664,028 | 1.62 | 18 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/x1 | 10000000 | 15.457 | 646,964 | 1.14 | 1968 | 3 | ok |
| libpq (libpq 180007) | lane1/x1 | 10000000 | 15.674 | 637,983 | 0.89 | 12 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/x1 | 10000000 | 17.143 | 583,342 | 1.17 | 1960 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/x1 | 10000000 | 19.075 | 524,260 | 1.72 | 33 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/x1 | 10000000 | 28.773 | 347,553 | 2.72 | 42 | 3 | ok |
