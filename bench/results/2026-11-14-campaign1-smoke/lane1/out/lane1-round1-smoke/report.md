# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R4 unpipelined, 8 connections (p99 bound 20.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| pgjdbc | lane1/1 | 750 | 440 | 11993 | 387973 | 100.33 | 304 | 2 |
| karez-blocking | lane1/1 | 500 | 569 | 15630 | 398459 | 99.48 | 6 | 2 |
| karez-shared | lane1/1 | 500 | 594 | 12845 | 362807 | 145.35 | 7 | 2 |
| libpq | lane1/1 | 500 | 529 | 12190 | 392167 | 68.87 | 11 | 2 |
| tokio-postgres | lane1/1 | 500 | 499 | 13959 | 423625 | 76.52 | 6 | 2 |
| asyncpg | lane1/1 | 500 | 659 | 10486 | 356516 | 318.92 | 30 | 2 |

## S2 unpipelined, 8 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane1/1 | 32000 | 261 | 1161 | 2212 | 24.60 | 6 | 2 |
| libpq | lane1/1 | 32000 | 254 | 939 | 1673 | 23.91 | 12 | 2 |
| tokio-postgres | lane1/1 | 32000 | 258 | 688 | 1622 | 30.78 | 6 | 2 |
| pgjdbc | lane1/1 | 32000 | 257 | 1823 | 2785 | 29.18 | 405 | 2 |
| karez-shared | lane1/1 | 30400 | 283 | 1608 | 2347 | 46.58 | 6 | 2 |
| asyncpg | lane1/1 | 14000 | 336 | 631 | 795 | 71.44 | 29 | 2 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/1 | 1000000 | 0.944 | 1,059,771 | 0.36 | 4 | 2 | ok |
| libpq (libpq 180007) | lane1/1 | 1000000 | 0.970 | 1,030,503 | 0.47 | 320 | 2 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/1 | 1000000 | 1.234 | 810,274 | 0.75 | 5 | 2 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/1 | 1000000 | 1.693 | 590,824 | 2.31 | 1359 | 2 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/1 | 1000000 | 2.333 | 428,679 | 2.06 | 520 | 2 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| asyncpg (asyncpg 0.32.0) | lane1/1 | 20000 | 0.157 | 127,510 | 3.00 | 29 | 2 | ok |
| libpq (libpq 180007) | lane1/1 | 20000 | 0.172 | 116,212 | 1.25 | 10 | 2 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/1 | 20000 | 0.173 | 115,307 | 1.00 | 5 | 2 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/1 | 20000 | 0.327 | 61,209 | 36.75 | 207 | 2 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/1 | 20000 | 0.333 | 60,105 | 6.75 | 7 | 2 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| tokio-postgres (tokio-postgres 0.7) | lane1/1 | 100000 | 0.165 | 607,718 | 0.90 | 5 | 2 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/1 | 100000 | 0.166 | 602,954 | 0.75 | 5 | 2 | ok |
| libpq (libpq 180007) | lane1/1 | 100000 | 0.169 | 592,066 | 1.30 | 10 | 2 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/1 | 100000 | 0.191 | 524,109 | 1.70 | 32 | 2 | ok |
| pgjdbc (pgjdbc 42.7.5 (defaults)) | lane1/1 | 100000 | 0.455 | 219,660 | 11.60 | 334 | 2 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| libpq (libpq 180007) | lane1/1 | 13069067 | 26.364 | 495,713 | 1.34 | 6541 | 2 | ok |
| asyncpg (asyncpg 0.32.0) | lane1/1 | 13069067 | 122.354 | 106,814 | 9.36 | 42 | 2 | ok |

_failed: karez-blocking s8: rustbench: unknown scenario s8 (s1 to s6)
_

_failed: tokio-postgres s8: rustbench: unknown scenario s8 (s1 to s6)
_

_failed: pgjdbc s8: SLF4J: Failed to load class "org.slf4j.impl.StaticLoggerBinder".
SLF4J: Defaulting to no-operation (NOP) logger implementation
SLF4J: See http://www.slf4j.org/codes.html#StaticLoggerBinder for further_
