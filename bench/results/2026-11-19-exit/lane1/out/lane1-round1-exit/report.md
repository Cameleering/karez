# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| pgx (pgx v5) | lane1/x1 | 1000000 | 1.401 | 713,770 | 1.42 | 17 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x1 | 1000000 | 1.478 | 676,773 | 1.03 | 7 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x1 | 1000000 | 1.557 | 642,343 | 0.82 | 6 | 3 | ok |
| sqlx (sqlx 0.8) | lane1/x1 | 1000000 | 1.604 | 623,558 | 1.46 | 7 | 3 | ok |
| libpq (libpq 180007) | lane1/x1 | 1000000 | 1.610 | 621,234 | 1.04 | 320 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/x1 | 1000000 | 1.729 | 578,269 | 0.85 | 6 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/x1 | 1000000 | 1.981 | 504,796 | 1.54 | 1691 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/x1 | 1000000 | 3.567 | 280,355 | 3.52 | 521 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/x1 | 1000000 | 4.292 | 232,997 | 3.88 | 826 | 3 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| sqlx (sqlx 0.8) | lane1/x1 | 100000 | 1.133 | 88,261 | 2.60 | 9 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/x1 | 100000 | 1.341 | 74,549 | 19.00 | 360 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/x1 | 100000 | 1.482 | 67,467 | 6.20 | 29 | 3 | ok |
| libpq (libpq 180007) | lane1/x1 | 100000 | 1.620 | 61,740 | 1.90 | 10 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x1 | 100000 | 1.938 | 51,602 | 2.50 | 7 | 3 | ok |
| pgx (pgx v5) | lane1/x1 | 100000 | 2.696 | 37,096 | 13.70 | 16 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/x1 | 100000 | 3.278 | 30,505 | 32.10 | 89 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x1 | 100000 | 4.000 | 25,002 | 20.90 | 9 | 3 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| pgbulkinsert (pgbulkinsert 8.1.4 on pgjdbc 42.7.5) | lane1/x1 | 10000000 | 27.944 | 357,862 | 2.23 | 1935 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x1 | 10000000 | 31.359 | 318,889 | 1.48 | 6 | 3 | ok |
| sqlx (sqlx 0.8) | lane1/x1 | 10000000 | 31.360 | 318,873 | 1.61 | 8 | 3 | ok |
| libpq (libpq 180007) | lane1/x1 | 10000000 | 32.839 | 304,512 | 2.40 | 10 | 3 | ok |
| pgx (pgx v5) | lane1/x1 | 10000000 | 32.885 | 304,094 | 3.20 | 17 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x1 | 10000000 | 32.962 | 303,380 | 2.35 | 7 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/x1 | 10000000 | 33.741 | 296,378 | 2.74 | 1962 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/x1 | 10000000 | 33.792 | 295,926 | 2.80 | 1953 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/x1 | 10000000 | 38.839 | 257,472 | 3.40 | 31 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/x1 | 10000000 | 58.221 | 171,758 | 5.24 | 41 | 3 | ok |

### S4 over TLS (sslmode=verify-full)

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| pgx (pgx v5) | lane1/x1 | 1000000 | 1.767 | 565,949 | 2.05 | 18 | 3 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/x1 | 1000000 | 1.830 | 546,538 | 0.88 | 7 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x1 | 1000000 | 1.867 | 535,504 | 1.16 | 8 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/x1 | 1000000 | 2.039 | 490,436 | 1.65 | 1837 | 3 | ok |
| libpq (libpq 180007) | lane1/x1 | 1000000 | 2.046 | 488,830 | 1.24 | 322 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x1 | 1000000 | 2.049 | 488,043 | 0.95 | 7 | 3 | ok |
| sqlx (sqlx 0.8) | lane1/x1 | 1000000 | 2.487 | 402,139 | 1.69 | 8 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/x1 | 1000000 | 4.706 | 212,481 | 4.70 | 523 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/x1 | 1000000 | 5.180 | 193,043 | 4.20 | 827 | 3 | ok |

### S5 over TLS (sslmode=verify-full)

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| sqlx (sqlx 0.8) | lane1/x1 | 100000 | 1.157 | 86,445 | 3.30 | 10 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/x1 | 100000 | 1.296 | 77,178 | 17.60 | 342 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/x1 | 100000 | 1.484 | 67,399 | 6.90 | 32 | 3 | ok |
| libpq (libpq 180007) | lane1/x1 | 100000 | 1.774 | 56,357 | 2.10 | 11 | 3 | ok |
| pgx (pgx v5) | lane1/x1 | 100000 | 2.265 | 44,145 | 7.40 | 17 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x1 | 100000 | 2.344 | 42,669 | 2.80 | 8 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/x1 | 100000 | 3.424 | 29,210 | 33.20 | 91 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x1 | 100000 | 3.622 | 27,608 | 22.60 | 11 | 3 | ok |

### S6 over TLS (sslmode=verify-full)

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| pgbulkinsert (pgbulkinsert 8.1.4 on pgjdbc 42.7.5) | lane1/x1 | 10000000 | 31.003 | 322,547 | 2.33 | 1947 | 3 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x1 | 10000000 | 31.735 | 315,111 | 2.23 | 9 | 3 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x1 | 10000000 | 32.530 | 307,412 | 1.46 | 7 | 3 | ok |
| sqlx (sqlx 0.8) | lane1/x1 | 10000000 | 32.785 | 305,013 | 1.56 | 8 | 3 | ok |
| pgx (pgx v5) | lane1/x1 | 10000000 | 35.249 | 283,694 | 3.58 | 18 | 3 | ok |
| libpq (libpq 180007) | lane1/x1 | 10000000 | 36.861 | 271,293 | 2.26 | 11 | 3 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/x1 | 10000000 | 37.240 | 268,531 | 1.96 | 1962 | 3 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane1/x1 | 10000000 | 39.145 | 255,464 | 2.62 | 1975 | 3 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/x1 | 10000000 | 39.596 | 252,548 | 3.38 | 33 | 3 | ok |
| psycopg3 (psycopg 3.3.9 (binary)) | lane1/x1 | 10000000 | 58.168 | 171,915 | 5.36 | 42 | 3 | ok |
