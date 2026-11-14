# Benchmark report

Built by `bench/report.py` from the raw cells (`cells.jsonl`) of every lane and round. Every number is `[measured here]`, on the machines listed at the end, on the dates in the raw files. **Ratios compare clients only between rows of the same lane and round; the figure shown is their median with the count and range.** Absolute numbers belong to their droplets. S1 to S3 and S8 are open loop with the p99 bound given in each heading; the SLO rate is the highest rate searched at which the median p99 of the final runs stayed inside it. Losses are in the same tables as wins.

Lanes x rounds in this report: 3.

## Open-loop scenarios

### R1 pipelined, 8 connections (p99 bound 3.0 ms)

| client | lanes x rounds | SLO rate/s (median) | vs karez-blocking | vs karez-shared | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB |
|---|---:|---:|---|---|---:|---:|---:|---:|---:|
| karez-shared | 3 | 56,000 | - | 1.00 (n=3, 1.00-1.00) | 684 | 2,736 | 4,817 | 46.9 | 11 |
| tokio-postgres | 1 | 53,200 | - | 0.89 (n=1, 0.89-0.89) | 305 | 1,548 | 4,227 | 37.6 | 955 |
| vertx-pg-client | 1 | 36,000 | - | 0.73 (n=1, 0.73-0.73) | 230 | 1,761 | 9,765 | 31.6 | 524 |

### R1 unpipelined, 64 connections (p99 bound 3.0 ms)

| client | lanes x rounds | SLO rate/s (median) | vs karez-blocking | vs karez-shared | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB |
|---|---:|---:|---|---|---:|---:|---:|---:|---:|
| deadpool-postgres | 1 | 49,400 | 1.00 (n=1, 1.00-1.00) | 1.05 (n=1, 1.05-1.05) | 281 | 1,745 | 3,949 | 46.4 | 8 |
| karez-blocking | 3 | 46,930 | 1.00 (n=3, 1.00-1.00) | 1.00 (n=3, 0.64-1.05) | 309 | 1,028 | 2,277 | 30.0 | 11 |
| karez-shared | 3 | 46,930 | 1.00 (n=3, 0.95-1.56) | 1.00 (n=3, 1.00-1.00) | 449 | 1,581 | 2,769 | 63.0 | 14 |
| pgx | 1 | 46,930 | 0.95 (n=1, 0.95-0.95) | 1.00 (n=1, 1.00-1.00) | 276 | 2,507 | 5,603 | 49.1 | 27 |
| libpq | 1 | 45,600 | 0.97 (n=1, 0.97-0.97) | 0.97 (n=1, 0.97-0.97) | 330 | 1,090 | 2,310 | 26.0 | 18 |
| tokio-postgres | 1 | 45,600 | 0.97 (n=1, 0.97-0.97) | 0.97 (n=1, 0.97-0.97) | 305 | 668 | 1,548 | 28.8 | 8 |
| pgjdbc | 1 | 43,320 | 0.92 (n=1, 0.92-0.92) | 0.92 (n=1, 0.92-0.92) | 326 | 1,401 | 2,933 | 37.7 | 424 |
| pgjdbc-tuned | 1 | 39,096 | 1.30 (n=1, 1.30-1.30) | 0.83 (n=1, 0.83-0.83) | 334 | 2,056 | 4,751 | 38.9 | 424 |
| sqlx-tuned | 1 | 32,000 | 1.07 (n=1, 1.07-1.07) | 0.68 (n=1, 0.68-0.68) | 424 | 1,499 | 2,474 | 54.4 | 8 |
| vertx-pg-client | 1 | 30,000 | 0.61 (n=1, 0.61-0.61) | 0.64 (n=1, 0.64-0.64) | 214 | 537 | 4,751 | 34.3 | 430 |
| sqlx | 1 | 25,270 | 0.84 (n=1, 0.84-0.84) | 0.54 (n=1, 0.54-0.54) | 627 | 2,179 | 3,752 | 76.3 | 8 |
| r2dbc-postgresql | 1 | 20,000 | 0.67 (n=1, 0.67-0.67) | 0.43 (n=1, 0.43-0.43) | 338 | 1,352 | 3,719 | 94.8 | 666 |
| asyncpg | 1 | 13,000 | 0.28 (n=1, 0.28-0.28) | 0.28 (n=1, 0.28-0.28) | 987 | 1,532 | 2,310 | 80.3 | 30 |
| psycopg3 | 1 | 3,750 | 0.08 (n=1, 0.08-0.08) | 0.08 (n=1, 0.08-0.08) | 528 | 1,368 | 8,847 | 245.1 | 41 |

### R1 unpipelined, 512 connections (p99 bound 3.0 ms)

| client | lanes x rounds | SLO rate/s (median) | vs karez-blocking | vs karez-shared | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB |
|---|---:|---:|---|---|---:|---:|---:|---:|---:|
| karez-blocking | 3 | 32,000 | 1.00 (n=3, 1.00-1.00) | 1.00 (n=3, 0.94-1.23) | 358 | 2,245 | 5,931 | 35.9 | 30 |
| karez-shared | 3 | 32,000 | 1.00 (n=3, 0.81-1.07) | 1.00 (n=3, 1.00-1.00) | 465 | 2,310 | 24,510 | 79.8 | 86 |
| tokio-postgres | 1 | 30,400 | 0.95 (n=1, 0.95-0.95) | 0.95 (n=1, 0.95-0.95) | 342 | 1,303 | 3,064 | 30.6 | 23 |
| libpq | 1 | 30,000 | 0.94 (n=1, 0.94-0.94) | 0.94 (n=1, 0.94-0.94) | 358 | 1,745 | 4,489 | 28.5 | 38 |
| deadpool-postgres | 1 | 28,500 | 0.89 (n=1, 0.89-0.89) | 1.09 (n=1, 1.09-1.09) | 326 | 1,319 | 3,162 | 49.4 | 24 |
| pgx | 1 | 28,500 | 0.89 (n=1, 0.89-0.89) | 1.09 (n=1, 1.09-1.09) | 326 | 2,998 | 6,062 | 59.6 | 50 |
| vertx-pg-client | 1 | 28,000 | 0.88 (n=1, 0.88-0.88) | 1.07 (n=1, 1.07-1.07) | 210 | 553 | 5,603 | 36.3 | 464 |
| pgjdbc-tuned | 1 | 27,436 | 0.86 (n=1, 0.86-0.86) | 0.80 (n=1, 0.80-0.80) | 322 | 1,974 | 18,481 | 41.3 | 503 |
| pgjdbc | 1 | 26,064 | 0.81 (n=1, 0.81-0.81) | 0.81 (n=1, 0.81-0.81) | 301 | 725 | 5,014 | 45.1 | 503 |
| r2dbc-postgresql | 1 | 24,000 | 0.75 (n=1, 0.75-0.75) | 0.70 (n=1, 0.70-0.70) | 375 | 1,450 | 3,555 | 94.0 | 541 |
| sqlx-tuned | 1 | 20,000 | 0.62 (n=1, 0.62-0.62) | 0.58 (n=1, 0.58-0.58) | 375 | 1,106 | 2,376 | 47.9 | 22 |
| sqlx | 1 | 17,100 | 0.53 (n=1, 0.53-0.53) | 0.50 (n=1, 0.50-0.50) | 578 | 1,778 | 4,047 | 70.7 | 22 |
| asyncpg | 1 | 11,400 | 0.36 (n=1, 0.36-0.36) | 0.36 (n=1, 0.36-0.36) | 514 | 2,179 | 2,802 | 92.3 | 40 |
| psycopg3 | 1 | 3,750 | 0.12 (n=1, 0.12-0.12) | 0.14 (n=1, 0.14-0.14) | 651 | 2,245 | 2,802 | 271.8 | 50 |

### R2 unpipelined, 64 connections (p99 bound 8.0 ms)

| client | lanes x rounds | SLO rate/s (median) | vs karez-blocking | vs karez-shared | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB |
|---|---:|---:|---|---|---:|---:|---:|---:|---:|
| deadpool-postgres | 1 | 9,500 | 1.00 (n=1, 1.00-1.00) | 1.06 (n=1, 1.06-1.06) | 782 | 3,850 | 7,504 | 216.7 | 9 |
| karez-blocking | 3 | 8,550 | 1.00 (n=3, 1.00-1.00) | 1.00 (n=3, 0.83-1.06) | 881 | 4,817 | 10,027 | 136.2 | 14 |
| karez-shared | 3 | 8,550 | 1.00 (n=3, 0.95-1.20) | 1.00 (n=3, 1.00-1.00) | 938 | 4,227 | 9,503 | 206.7 | 19 |
| libpq | 1 | 8,550 | 1.00 (n=1, 1.00-1.00) | 1.00 (n=1, 1.00-1.00) | 848 | 6,521 | 13,173 | 106.1 | 18 |
| pgx | 1 | 8,550 | 0.90 (n=1, 0.90-0.90) | 0.95 (n=1, 0.95-0.95) | 1,106 | 7,635 | 17,170 | 308.3 | 34 |
| tokio-postgres | 1 | 8,550 | 1.00 (n=1, 1.00-1.00) | 1.00 (n=1, 1.00-1.00) | 799 | 3,654 | 9,634 | 174.5 | 9 |
| pgjdbc-tuned | 1 | 7,600 | 1.07 (n=1, 1.07-1.07) | 0.89 (n=1, 0.89-0.89) | 864 | 7,242 | 13,828 | 161.3 | 426 |
| pgjdbc | 1 | 7,331 | 0.86 (n=1, 0.86-0.86) | 0.86 (n=1, 0.86-0.86) | 815 | 3,260 | 7,307 | 162.8 | 429 |
| sqlx | 1 | 6,500 | 0.91 (n=1, 0.91-0.91) | 0.76 (n=1, 0.76-0.76) | 1,401 | 3,424 | 6,193 | 287.8 | 9 |
| sqlx-tuned | 1 | 6,500 | 0.91 (n=1, 0.91-0.91) | 0.76 (n=1, 0.76-0.76) | 946 | 2,834 | 5,407 | 263.6 | 10 |
| vertx-pg-client | 1 | 5,500 | 0.58 (n=1, 0.58-0.58) | 0.61 (n=1, 0.61-0.61) | 635 | 1,368 | 2,474 | 186.0 | 449 |
| r2dbc-postgresql | 1 | 1,500 | 0.21 (n=1, 0.21-0.21) | 0.18 (n=1, 0.18-0.18) | 1,581 | 6,980 | 11,469 | 1,111.7 | 470 |
| asyncpg | 1 | 875 | 0.10 (n=1, 0.10-0.10) | 0.10 (n=1, 0.10-0.10) | 3,424 | 7,373 | 8,716 | 1,043.8 | 36 |
| psycopg3 | 1 | 500 | 0.05 (n=1, 0.05-0.05) | 0.06 (n=1, 0.06-0.06) | 5,472 | 7,307 | 7,897 | 1,967.5 | 46 |

### R3 unpipelined, 8 connections (p99 bound 60.0 ms)

| client | lanes x rounds | SLO rate/s (median) | vs karez-blocking | vs karez-shared | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB |
|---|---:|---:|---|---|---:|---:|---:|---:|---:|
| karez-blocking | 3 | - | - | - | 6,914 | 1,182,794 | 1,518,338 | 48.6 | 5 |
| karez-shared | 3 | - | - | - | 7,111 | 1,149,239 | 1,535,115 | 87.8 | 6 |
| asyncpg | 1 | - | - | - | 6,980 | 1,266,680 | 1,551,892 | 159.7 | 29 |
| deadpool-postgres | 1 | - | - | - | 6,586 | 1,132,462 | 1,619,001 | 83.6 | 6 |
| libpq | 1 | - | - | - | 6,849 | 1,166,017 | 1,501,561 | 36.3 | 10 |
| pgjdbc | 1 | - | - | - | 7,176 | 1,082,130 | 1,518,338 | 169.1 | 108 |
| pgjdbc-tuned | 1 | - | - | - | 8,159 | 1,283,457 | 1,619,001 | 235.8 | 107 |
| pgx | 1 | - | - | - | 6,783 | 1,132,462 | 1,468,006 | 122.3 | 11 |
| psycopg3 | 1 | - | - | - | 6,652 | 1,149,239 | 1,484,784 | 329.6 | 40 |
| r2dbc-postgresql | 1 | - | - | - | - | - | - | - | - |
| sqlx | 1 | - | - | - | 8,716 | 1,182,794 | 1,400,898 | 216.4 | 6 |
| sqlx-tuned | 1 | - | - | - | 8,159 | 1,233,125 | 1,535,115 | 195.5 | 6 |
| tokio-postgres | 1 | - | - | - | 7,045 | 1,115,685 | 1,535,115 | 72.7 | 6 |
| vertx-pg-client | 1 | - | - | - | - | - | - | - | - |

Did not meet the p99 bound at any rate: karez-blocking, karez-shared, asyncpg, deadpool-postgres, libpq, pgjdbc, pgjdbc-tuned, pgx, psycopg3, r2dbc-postgresql, sqlx, sqlx-tuned, tokio-postgres, vertx-pg-client.

### R4 unpipelined, 64 connections (p99 bound 20.0 ms)

| client | lanes x rounds | SLO rate/s (median) | vs karez-blocking | vs karez-shared | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB |
|---|---:|---:|---|---|---:|---:|---:|---:|---:|
| pgjdbc | 1 | 469 | - | - | 489 | 14,352 | 354,419 | 101.1 | 244 |
| pgjdbc-tuned | 1 | 469 | - | - | 602 | 15,139 | 400,556 | 124.7 | 258 |
| karez-shared | 3 | 445 | - | 1.00 (n=1, 1.00-1.00) | 537 | 17,170 | 367,002 | 280.6 | 15 |
| asyncpg | 1 | 445 | - | - | 627 | 18,481 | 362,807 | 303.2 | 32 |
| libpq | 1 | 445 | - | - | 518 | 18,481 | 354,419 | 58.8 | 15 |
| karez-blocking | 3 | - | - | 0.00 (n=1, 0.00-0.00) | - | - | - | - | - |
| deadpool-postgres | 1 | - | - | 0.00 (n=1, 0.00-0.00) | - | - | - | - | - |
| pgx | 1 | - | - | 0.00 (n=1, 0.00-0.00) | - | - | - | - | - |
| psycopg3 | 1 | - | - | 0.00 (n=1, 0.00-0.00) | - | - | - | - | - |
| r2dbc-postgresql | 1 | - | - | - | - | - | - | - | - |
| sqlx | 1 | - | - | - | - | - | - | - | - |
| sqlx-tuned | 1 | - | - | - | - | - | - | - | - |
| tokio-postgres | 1 | - | - | - | - | - | - | - | - |
| vertx-pg-client | 1 | - | - | 0.00 (n=1, 0.00-0.00) | - | - | - | - | - |

Did not meet the p99 bound at any rate: karez-blocking, deadpool-postgres, pgx, psycopg3, r2dbc-postgresql, sqlx, sqlx-tuned, tokio-postgres, vertx-pg-client.

### S1 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lanes x rounds | SLO rate/s (median) | vs karez-blocking | vs karez-shared | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB |
|---|---:|---:|---|---|---:|---:|---:|---:|---:|
| karez-blocking | 3 | 79,420 | 1.00 (n=3, 1.00-1.00) | 1.04 (n=3, 1.04-1.53) | 293 | 1,028 | 2,023 | 23.3 | 10 |
| tokio-postgres | 1 | 79,420 | 1.00 (n=1, 1.00-1.00) | 1.04 (n=1, 1.04-1.04) | 297 | 1,106 | 2,736 | 27.4 | 8 |
| karez-shared | 3 | 76,000 | 0.96 (n=3, 0.65-0.96) | 1.00 (n=3, 1.00-1.00) | 399 | 1,253 | 2,802 | 43.7 | 13 |
| libpq | 1 | 76,000 | 0.96 (n=1, 0.96-0.96) | 1.00 (n=1, 1.00-1.00) | 297 | 1,270 | 2,769 | 24.8 | 21 |
| pgx | 1 | 76,000 | 0.96 (n=1, 0.96-0.96) | 1.46 (n=1, 1.46-1.46) | 206 | 1,696 | 3,359 | 33.9 | 25 |
| zero-postgres | 1 | 76,000 | 0.96 (n=1, 0.96-0.96) | 1.46 (n=1, 1.46-1.46) | 465 | 1,909 | 2,900 | 32.4 | 7 |
| deadpool-postgres | 1 | 75,449 | 0.95 (n=1, 0.95-0.95) | 1.45 (n=1, 1.45-1.45) | 264 | 1,597 | 2,933 | 39.1 | 8 |
| pgjdbc-tuned | 1 | 72,200 | 0.91 (n=1, 0.91-0.91) | 0.95 (n=1, 0.95-0.95) | 313 | 1,401 | 3,391 | 33.3 | 421 |
| pgjdbc | 1 | 65,160 | 0.82 (n=1, 0.82-0.82) | 0.86 (n=1, 0.86-0.86) | 301 | 1,057 | 2,376 | 34.1 | 419 |
| sqlx-tuned | 1 | 49,400 | 0.62 (n=1, 0.62-0.62) | 0.65 (n=1, 0.65-0.65) | 334 | 1,876 | 5,210 | 50.8 | 8 |
| sqlx | 1 | 32,490 | 0.41 (n=1, 0.41-0.41) | 0.43 (n=1, 0.43-0.43) | 518 | 1,597 | 2,769 | 72.3 | 8 |
| r2dbc-postgresql | 1 | 32,000 | 0.40 (n=1, 0.40-0.40) | 0.42 (n=1, 0.42-0.42) | 281 | 1,020 | 3,359 | 50.5 | 670 |
| vertx-pg-client | 1 | 32,000 | 0.40 (n=1, 0.40-0.40) | 0.62 (n=1, 0.62-0.62) | 189 | 848 | 2,441 | 28.1 | 423 |
| psycopg3 | 1 | 5,000 | 0.06 (n=1, 0.06-0.06) | 0.10 (n=1, 0.10-0.10) | 399 | 676 | 1,761 | 195.4 | 41 |
| asyncpg | 1 | - | 0.00 (n=1, 0.00-0.00) | 0.00 (n=1, 0.00-0.00) | - | - | - | - | - |

Did not meet the p99 bound at any rate: asyncpg.

### S2 unpipelined, 1 connections (p99 bound 2.0 ms)

| client | lanes x rounds | SLO rate/s (median) | vs karez-blocking | vs karez-shared | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB |
|---|---:|---:|---|---|---:|---:|---:|---:|---:|
| deadpool-postgres | 1 | 8,000 | 1.07 (n=1, 1.07-1.07) | 1.14 (n=1, 1.14-1.14) | 181 | 1,303 | 4,358 | 37.7 | 6 |
| vertx-pg-client | 1 | 7,600 | 1.01 (n=1, 1.01-1.01) | 1.09 (n=1, 1.09-1.09) | 185 | 1,991 | 4,227 | 50.4 | 401 |
| zero-postgres | 1 | 7,500 | 1.00 (n=1, 1.00-1.00) | 1.07 (n=1, 1.07-1.07) | 183 | 528 | 2,638 | 41.9 | 5 |
| pgx | 1 | 6,650 | 0.89 (n=1, 0.89-0.89) | 0.95 (n=1, 0.95-0.95) | 717 | 1,303 | 2,769 | 76.6 | 22 |
| pgjdbc-tuned | 1 | 5,225 | 1.39 (n=1, 1.39-1.39) | 1.39 (n=1, 1.39-1.39) | 226 | 872 | 2,998 | 25.9 | 298 |
| karez-blocking | 3 | 5,000 | 1.00 (n=3, 1.00-1.00) | 1.05 (n=3, 1.00-1.07) | 222 | 330 | 1,647 | 21.8 | 5 |
| pgjdbc | 1 | 5,000 | 1.00 (n=1, 1.00-1.00) | 1.05 (n=1, 1.05-1.05) | 230 | 1,483 | 5,276 | 27.4 | 283 |
| tokio-postgres | 1 | 5,000 | 1.00 (n=1, 1.00-1.00) | 1.05 (n=1, 1.05-1.05) | 239 | 1,237 | 5,276 | 27.8 | 5 |
| libpq | 1 | 4,964 | 0.99 (n=1, 0.99-0.99) | 1.04 (n=1, 1.04-1.04) | 230 | 412 | 3,260 | 22.7 | 11 |
| karez-shared | 3 | 4,750 | 0.95 (n=3, 0.93-1.00) | 1.00 (n=3, 1.00-1.00) | 247 | 635 | 2,966 | 43.0 | 5 |
| asyncpg | 1 | 4,000 | 0.80 (n=1, 0.80-0.80) | 0.84 (n=1, 0.84-0.84) | 301 | 1,036 | 2,998 | 83.4 | 29 |
| psycopg3 | 1 | 3,750 | 0.50 (n=1, 0.50-0.50) | 0.54 (n=1, 0.54-0.54) | 395 | 791 | 3,654 | 209.9 | 40 |
| r2dbc-postgresql | 1 | 3,500 | 0.93 (n=1, 0.93-0.93) | 0.93 (n=1, 0.93-0.93) | 276 | 840 | 3,097 | 69.4 | 437 |
| sqlx-tuned | 1 | 2,750 | 0.73 (n=1, 0.73-0.73) | 0.73 (n=1, 0.73-0.73) | 226 | 635 | 4,178 | 48.4 | 6 |
| sqlx | 1 | 2,000 | 0.53 (n=1, 0.53-0.53) | 0.53 (n=1, 0.53-0.53) | 367 | 1,139 | 5,669 | 56.5 | 6 |

### S2 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lanes x rounds | SLO rate/s (median) | vs karez-blocking | vs karez-shared | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB |
|---|---:|---:|---|---|---:|---:|---:|---:|---:|
| karez-blocking | 3 | 53,200 | 1.00 (n=3, 1.00-1.00) | 1.05 (n=3, 1.00-1.10) | 317 | 1,548 | 2,736 | 27.9 | 10 |
| zero-postgres | 1 | 53,200 | 0.98 (n=1, 0.98-0.98) | 1.08 (n=1, 1.08-1.08) | 268 | 1,319 | 2,867 | 35.9 | 7 |
| deadpool-postgres | 1 | 52,000 | 0.96 (n=1, 0.96-0.96) | 1.05 (n=1, 1.05-1.05) | 276 | 1,122 | 2,933 | 44.9 | 8 |
| karez-shared | 3 | 50,540 | 0.95 (n=3, 0.91-1.00) | 1.00 (n=3, 1.00-1.00) | 436 | 1,581 | 2,933 | 54.4 | 13 |
| libpq | 1 | 49,400 | 0.98 (n=1, 0.98-0.98) | 0.98 (n=1, 0.98-0.98) | 313 | 930 | 2,441 | 24.8 | 18 |
| tokio-postgres | 1 | 48,013 | 0.95 (n=1, 0.95-0.95) | 0.95 (n=1, 0.95-0.95) | 297 | 856 | 2,802 | 27.3 | 8 |
| pgx | 1 | 48,000 | 0.89 (n=1, 0.89-0.89) | 0.97 (n=1, 0.97-0.97) | 239 | 1,286 | 2,703 | 45.1 | 26 |
| pgjdbc | 1 | 44,584 | 0.88 (n=1, 0.88-0.88) | 0.88 (n=1, 0.88-0.88) | 309 | 1,974 | 3,949 | 35.0 | 420 |
| pgjdbc-tuned | 1 | 43,332 | 0.81 (n=1, 0.81-0.81) | 0.86 (n=1, 0.86-0.86) | 297 | 1,139 | 2,703 | 35.1 | 417 |
| sqlx-tuned | 1 | 34,295 | 0.64 (n=1, 0.64-0.64) | 0.68 (n=1, 0.68-0.68) | 350 | 1,434 | 3,457 | 52.5 | 8 |
| vertx-pg-client | 1 | 30,000 | 0.55 (n=1, 0.55-0.55) | 0.61 (n=1, 0.61-0.61) | 210 | 395 | 2,507 | 31.8 | 432 |
| sqlx | 1 | 24,700 | 0.46 (n=1, 0.46-0.46) | 0.49 (n=1, 0.49-0.49) | 537 | 1,499 | 2,703 | 75.8 | 8 |
| r2dbc-postgresql | 1 | 20,000 | 0.38 (n=1, 0.38-0.38) | 0.40 (n=1, 0.40-0.40) | 313 | 545 | 2,933 | 78.3 | 494 |
| asyncpg | 1 | 15,200 | 0.30 (n=1, 0.30-0.30) | 0.30 (n=1, 0.30-0.30) | 545 | 1,368 | 2,179 | 69.4 | 30 |
| psycopg3 | 1 | 4,287 | 0.08 (n=1, 0.08-0.08) | 0.09 (n=1, 0.09-0.09) | 528 | 1,942 | 2,245 | 234.5 | 41 |

### S2 unpipelined, 512 connections (p99 bound 2.0 ms)

| client | lanes x rounds | SLO rate/s (median) | vs karez-blocking | vs karez-shared | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB |
|---|---:|---:|---|---|---:|---:|---:|---:|---:|
| karez-blocking | 3 | 34,200 | 1.00 (n=3, 1.00-1.00) | 1.00 (n=3, 0.95-1.25) | 326 | 954 | 2,277 | 31.9 | 29 |
| karez-shared | 3 | 34,200 | 1.00 (n=3, 0.80-1.06) | 1.00 (n=3, 1.00-1.00) | 428 | 1,729 | 4,112 | 65.7 | 86 |
| tokio-postgres | 1 | 34,200 | 1.00 (n=1, 1.00-1.00) | 0.95 (n=1, 0.95-0.95) | 334 | 1,663 | 4,489 | 29.1 | 22 |
| libpq | 1 | 32,000 | 0.94 (n=1, 0.94-0.94) | 0.89 (n=1, 0.89-0.89) | 338 | 1,073 | 3,097 | 27.9 | 39 |
| pgx | 1 | 32,000 | 0.94 (n=1, 0.94-0.94) | 1.17 (n=1, 1.17-1.17) | 297 | 1,827 | 4,178 | 53.7 | 48 |
| vertx-pg-client | 1 | 32,000 | 0.94 (n=1, 0.94-0.94) | 1.17 (n=1, 1.17-1.17) | 208 | 1,335 | 7,766 | 31.8 | 450 |
| pgjdbc-tuned | 1 | 30,400 | 0.89 (n=1, 0.89-0.89) | 0.89 (n=1, 0.89-0.89) | 309 | 1,012 | 5,079 | 38.0 | 497 |
| zero-postgres | 1 | 30,400 | 0.89 (n=1, 0.89-0.89) | 1.11 (n=1, 1.11-1.11) | 305 | 963 | 2,736 | 36.4 | 15 |
| deadpool-postgres | 1 | 30,000 | 0.88 (n=1, 0.88-0.88) | 1.09 (n=1, 1.09-1.09) | 305 | 864 | 2,474 | 49.8 | 23 |
| r2dbc-postgresql | 1 | 28,500 | 0.83 (n=1, 0.83-0.83) | 0.83 (n=1, 0.83-0.83) | 350 | 1,155 | 3,686 | 82.7 | 523 |
| pgjdbc | 1 | 27,436 | 0.80 (n=1, 0.80-0.80) | 0.76 (n=1, 0.76-0.76) | 317 | 1,663 | 24,773 | 39.9 | 499 |
| sqlx-tuned | 1 | 22,800 | 0.67 (n=1, 0.67-0.67) | 0.67 (n=1, 0.67-0.67) | 362 | 1,417 | 3,949 | 50.5 | 21 |
| sqlx | 1 | 18,000 | 0.53 (n=1, 0.53-0.53) | 0.53 (n=1, 0.53-0.53) | 561 | 1,565 | 3,752 | 75.3 | 21 |
| asyncpg | 1 | 14,000 | 0.41 (n=1, 0.41-0.41) | 0.39 (n=1, 0.39-0.39) | 651 | 1,581 | 1,991 | 76.0 | 38 |
| psycopg3 | 1 | 4,061 | 0.12 (n=1, 0.12-0.12) | 0.15 (n=1, 0.15-0.15) | 473 | 1,909 | 3,883 | 212.5 | 50 |

### S7 unpipelined, 64 connections (p99 bound 2.0 ms)

| client | lanes x rounds | SLO rate/s (median) | vs karez-blocking | vs karez-shared | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB |
|---|---:|---:|---|---|---:|---:|---:|---:|---:|
| karez-blocking | 3 | 39,710 | 1.00 (n=3, 1.00-1.00) | 1.00 (n=3, 1.00-1.00) | 334 | 1,155 | 2,507 | 31.0 | 12 |
| karez-shared | 3 | 39,710 | 1.00 (n=3, 1.00-1.00) | 1.00 (n=3, 1.00-1.00) | 420 | 1,499 | 2,769 | 59.0 | 16 |
| pgx | 1 | 39,710 | 0.95 (n=1, 0.95-0.95) | 0.95 (n=1, 0.95-0.95) | 268 | 1,827 | 3,457 | 50.8 | 35 |
| tokio-postgres | 1 | 39,710 | 1.00 (n=1, 1.00-1.00) | 1.00 (n=1, 1.00-1.00) | 322 | 1,073 | 2,507 | 33.9 | 10 |
| libpq | 1 | 38,000 | 0.96 (n=1, 0.96-0.96) | 0.96 (n=1, 0.96-0.96) | 338 | 881 | 1,909 | 34.3 | 22 |
| pgjdbc-tuned | 1 | 34,200 | 0.86 (n=1, 0.86-0.86) | 0.86 (n=1, 0.86-0.86) | 338 | 1,761 | 3,686 | 43.0 | 460 |
| pgjdbc | 1 | 32,000 | 0.81 (n=1, 0.81-0.81) | 0.81 (n=1, 0.81-0.81) | 322 | 987 | 2,867 | 43.2 | 460 |
| sqlx-tuned | 1 | 25,270 | 0.64 (n=1, 0.64-0.64) | 0.64 (n=1, 0.64-0.64) | 375 | 1,614 | 3,752 | 55.3 | 11 |
| sqlx | 1 | 19,000 | 0.48 (n=1, 0.48-0.48) | 0.48 (n=1, 0.48-0.48) | 578 | 1,581 | 2,998 | 85.0 | 11 |
| r2dbc-postgresql | 1 | 15,000 | 0.38 (n=1, 0.38-0.38) | 0.38 (n=1, 0.38-0.38) | 338 | 553 | 1,843 | 102.7 | 533 |
| vertx-pg-client | 1 | 15,000 | 0.36 (n=1, 0.36-0.36) | 0.36 (n=1, 0.36-0.36) | 230 | 387 | 3,097 | 60.3 | 469 |
| asyncpg | 1 | 6,500 | 0.16 (n=1, 0.16-0.16) | 0.16 (n=1, 0.16-0.16) | 905 | 1,909 | 2,376 | 164.1 | 35 |
| psycopg3 | 1 | - | 0.00 (n=1, 0.00-0.00) | 0.00 (n=1, 0.00-0.00) | - | - | - | - | - |

Did not meet the p99 bound at any rate: psycopg3.

## Bulk scenarios (closed loop)

Seconds are the median of the timed passes of one cell. The ratio compares rows/s of a client with `karez-blocking` (row view) of the same lane and round; the chunk path row is a different access path and is labelled. CPU-s per million rows is the process CPU over the pass; RSS is the peak of the whole process.


### S4

| client | lanes x rounds | rows | median s | rows/s | vs karez-blocking | CPU-s per M rows | peak RSS MB | checksum |
|---|---:|---:|---:|---:|---|---:|---:|---|
| karez-blocking | 3 | 1000000 | 0.823 | 1,215,658 | 1.00 (n=3, 1.00-1.00) | 0.41 | 4 | ok |
| asyncpg | 1 | 1000000 | 2.082 | 480,400 | 0.40 (n=1, 0.40-0.40) | 2.08 | 520 | ok |
| libpq | 1 | 1000000 | 1.115 | 896,781 | 0.74 (n=1, 0.74-0.74) | 0.55 | 320 | ok |
| pgjdbc | 1 | 1000000 | 1.423 | 702,938 | 0.58 (n=1, 0.58-0.58) | 2.44 | 1,768 | ok |
| pgx | 1 | 1000000 | 0.782 | 1,278,921 | 0.92 (n=1, 0.92-0.92) | 0.87 | 18 | ok |
| psycopg3 | 1 | 1000000 | 2.817 | 355,038 | 0.26 (n=1, 0.26-0.26) | 2.70 | 826 | ok |
| sqlx | 1 | 1000000 | 0.784 | 1,275,348 | 1.55 (n=1, 1.55-1.55) | 0.81 | 6 | ok |
| tokio-postgres | 1 | 1000000 | 1.052 | 951,022 | 0.78 (n=1, 0.78-0.78) | 0.68 | 5 | ok |
| vertx-pg-client | 1 | 1000000 | 1.170 | 854,628 | 0.62 (n=1, 0.62-0.62) | 2.84 | 1,710 | ok |

### S5

| client | lanes x rounds | rows | median s | rows/s | vs karez-blocking | CPU-s per M rows | peak RSS MB | checksum |
|---|---:|---:|---:|---:|---|---:|---:|---|
| karez-blocking | 3 | 100000 | 0.814 | 122,850 | 1.00 (n=3, 1.00-1.00) | 1.30 | 5 | ok |
| asyncpg | 1 | 100000 | 0.724 | 138,064 | 1.17 (n=1, 1.17-1.17) | 3.20 | 29 | ok |
| libpq | 1 | 100000 | 0.780 | 128,222 | 1.09 (n=1, 1.09-1.09) | 0.90 | 10 | ok |
| pgjdbc | 1 | 100000 | 1.046 | 95,557 | 0.81 (n=1, 0.81-0.81) | 6.30 | 317 | ok |
| pgx | 1 | 100000 | 1.044 | 95,790 | 0.76 (n=1, 0.76-0.76) | 5.30 | 17 | ok |
| psycopg3 | 1 | 100000 | 3.312 | 30,195 | 0.24 (n=1, 0.24-0.24) | 33.00 | 88 | ok |
| sqlx | 1 | 100000 | 0.487 | 205,339 | 1.67 (n=1, 1.67-1.67) | 1.30 | 7 | ok |
| tokio-postgres | 1 | 100000 | 1.849 | 54,083 | 0.46 (n=1, 0.46-0.46) | 7.00 | 7 | ok |
| vertx-pg-client | 1 | 100000 | 0.968 | 103,316 | 0.82 (n=1, 0.82-0.82) | 5.50 | 318 | ok |

### S6

| client | lanes x rounds | rows | median s | rows/s | vs karez-blocking | CPU-s per M rows | peak RSS MB | checksum |
|---|---:|---:|---:|---:|---|---:|---:|---|
| karez-blocking | 3 | 3000000 | 4.703 | 637,918 | 1.00 (n=3, 1.00-1.00) | 0.79 | 5 | ok |
| asyncpg | 1 | 3000000 | 5.536 | 541,917 | 0.87 (n=1, 0.87-0.87) | 1.72 | 31 | ok |
| libpq | 1 | 3000000 | 5.081 | 590,412 | 0.95 (n=1, 0.95-0.95) | 1.35 | 10 | ok |
| pgjdbc | 1 | 3000000 | 5.353 | 560,423 | 0.90 (n=1, 0.90-0.90) | 1.61 | 1,936 | ok |
| pgx | 1 | 3000000 | 6.925 | 433,218 | 0.66 (n=1, 0.66-0.66) | 2.47 | 18 | ok |
| psycopg3 | 1 | 3000000 | 13.055 | 229,802 | 0.35 (n=1, 0.35-0.35) | 4.27 | 40 | ok |
| sqlx | 1 | 3000000 | 4.897 | 612,645 | 0.96 (n=1, 0.96-0.96) | 0.79 | 6 | ok |
| tokio-postgres | 1 | 3000000 | 5.090 | 589,391 | 0.94 (n=1, 0.94-0.94) | 0.86 | 5 | ok |

### S8

| client | lanes x rounds | rows | median s | rows/s | vs karez-blocking | CPU-s per M rows | peak RSS MB | checksum |
|---|---:|---:|---:|---:|---|---:|---:|---|
| karez-blocking | 3 | 13069067 | 17.815 | 733,587 | 1.00 (n=3, 1.00-1.00) | 0.55 | 4 | ok |
| asyncpg | 1 | 13069067 | 123.071 | 106,192 | 0.14 (n=1, 0.14-0.14) | 9.42 | 42 | ok |
| libpq | 1 | 13069067 | 27.504 | 475,175 | 0.65 (n=1, 0.65-0.65) | 1.24 | 6,541 | ok |
| pgx | 1 | 13069067 | 18.120 | 721,255 | 0.77 (n=1, 0.77-0.77) | 1.55 | 18 | ok |
| psycopg3 | 1 | 13069067 | 132.692 | 98,492 | 0.10 (n=1, 0.10-0.10) | 9.85 | 81 | ok |
| tokio-postgres | 1 | 13069067 | 18.727 | 697,877 | 0.95 (n=1, 0.95-0.95) | 0.93 | 5 | ok |

_failed: pgjdbc s8: SLF4J: Failed to load class "org.slf4j.impl.StaticLoggerBinder".
SLF4J: Defaulting to no-operation (NOP) logger implementation
SLF4J: See http://www.slf4j.org/c_

## Machines


`lane1\machine-db.txt`

```
host: karez-bench-l1-db  private ip: 10.122.0.3
kernel: 6.8.0-147-generic  cpus: 4  mem_mb: 7941
fsync=on
jit=off
max_connections=1000
password_encryption=scram-sha-256
shared_buffers=262144
ssl=on
synchronous_commit=on
wal_level=replica
PostgreSQL 18.7 (Ubuntu 18.7-1.pgdg24.04+2) on x86_64-pc-linux-gnu, compiled by gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0, 64-bit
taxi_trips rows=13069067 size=2120 MB
```

`lane1\out\lane1-round1-lean\machine-db.txt`

```

```

`lane2\machine-db.txt`

```
host: karez-bench-l2-db  private ip: 10.122.0.6
kernel: 6.8.0-147-generic  cpus: 4  mem_mb: 7941
fsync=on
jit=off
max_connections=1000
password_encryption=scram-sha-256
shared_buffers=262144
ssl=on
synchronous_commit=on
wal_level=replica
PostgreSQL 18.7 (Ubuntu 18.7-1.pgdg24.04+2) on x86_64-pc-linux-gnu, compiled by gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0, 64-bit
taxi_trips rows=13069067 size=2120 MB
```

`lane2\out\lane2-round1-lean\machine-db.txt`

```

```

`lane3\machine-db.txt`

```
host: karez-bench-l3-db  private ip: 10.122.0.4
kernel: 6.8.0-147-generic  cpus: 4  mem_mb: 7941
fsync=on
jit=off
max_connections=1000
password_encryption=scram-sha-256
shared_buffers=262144
ssl=on
synchronous_commit=on
wal_level=replica
PostgreSQL 18.7 (Ubuntu 18.7-1.pgdg24.04+2) on x86_64-pc-linux-gnu, compiled by gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0, 64-bit
taxi_trips rows=13069067 size=2120 MB
```

`lane3\out\lane3-round1-lean\machine-db.txt`

```

```

`lane1\out\lane1-round1-lean\machine.json`: `{"host": "karez-bench-l1-cl", "kernel": "6.8.0-147-generic", "python": "3.12.3", "cpus": 4, "cpu_model": "Intel(R) Xeon(R) Platinum 8280 CPU @ 2.70GHz", "mem_mb": 7941, "lane": "lane1", "round": "1", "profile": "lean", "url_host": "10.122.0.3:5432", "started": "2026-11-14T10:11:10Z"}`

`lane2\out\lane2-round1-lean\machine.json`: `{"host": "karez-bench-l2-cl", "kernel": "6.8.0-147-generic", "python": "3.12.3", "cpus": 4, "cpu_model": "Intel(R) Xeon(R) Platinum 8168 CPU @ 2.70GHz", "mem_mb": 7941, "lane": "lane2", "round": "1", "profile": "lean", "url_host": "10.122.0.6:5432", "started": "2026-11-14T10:11:10Z"}`

`lane3\out\lane3-round1-lean\machine.json`: `{"host": "karez-bench-l3-cl", "kernel": "6.8.0-147-generic", "python": "3.12.3", "cpus": 4, "cpu_model": "Intel(R) Xeon(R) Platinum 8280 CPU @ 2.70GHz", "mem_mb": 7941, "lane": "lane3", "round": "1", "profile": "lean", "url_host": "10.122.0.4:5432", "started": "2026-11-14T10:11:11Z"}`
