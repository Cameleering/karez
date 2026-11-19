# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## R4 unpipelined, 8 connections (p99 bound 20.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| pgjdbc-tuned | lane3/1 | 790 | 647 | 19431 | 438305 | 179.30 | 319 | 2 |
| karez-blocking | lane3/1 | 500 | 762 | 15499 | 404750 | 168.30 | 6 | 2 |
| karez-shared | lane3/1 | 500 | 750 | 14418 | 375390 | 224.46 | 7 | 2 |
| sqlx | lane3/1 | 500 | 831 | 13697 | 373293 | 278.03 | 7 | 2 |
| sqlx-tuned | lane3/1 | 500 | 614 | 12583 | 396362 | 224.52 | 6 | 2 |
| r2dbc-postgresql | lane3/1 | 500 | 803 | 14942 | 404750 | 449.09 | 456 | 2 |

## S2 unpipelined, 8 connections (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| karez-blocking | lane3/1 | 32000 | 253 | 568 | 1393 | 24.88 | 6 | 2 |
| karez-shared | lane3/1 | 30400 | 276 | 566 | 1497 | 48.35 | 6 | 2 |
| pgjdbc-tuned | lane3/1 | 30400 | 250 | 821 | 2249 | 30.42 | 402 | 2 |
| sqlx-tuned | lane3/1 | 18050 | 256 | 939 | 2003 | 50.75 | 6 | 2 |
| r2dbc-postgresql | lane3/1 | 14000 | 289 | 622 | 2085 | 81.63 | 664 | 2 |
| sqlx | lane3/1 | 12000 | 430 | 834 | 2322 | 70.07 | 6 | 2 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/1 | 1000000 | 0.721 | 1,387,155 | 0.34 | 4 | 2 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane3/1 | 1000000 | 1.133 | 882,690 | 0.98 | 1254 | 2 | ok |
| sqlx (sqlx 0.8) | lane3/1 | 1000000 | 1.134 | 881,912 | 1.11 | 6 | 2 | ok |
| r2dbc-postgresql (r2dbc-postgresql 1.0) | lane3/1 | 1000000 | 3.416 | 292,779 | 4.71 | 2000 | 2 | ok |

### S5

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| sqlx (sqlx 0.8) | lane3/1 | 20000 | 0.112 | 178,891 | 1.00 | 7 | 2 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/1 | 20000 | 0.174 | 114,745 | 1.25 | 5 | 2 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane3/1 | 20000 | 0.225 | 88,909 | 29.50 | 198 | 2 | ok |

### S6

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking (karez-pg 0.0.0 (row view)) | lane3/1 | 100000 | 0.145 | 688,468 | 0.80 | 4 | 2 | ok |
| sqlx (sqlx 0.8) | lane3/1 | 100000 | 0.148 | 677,966 | 0.85 | 6 | 2 | ok |
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane3/1 | 100000 | 0.374 | 267,344 | 8.95 | 324 | 2 | ok |

### S8

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| pgjdbc-tuned (pgjdbc 42.7.5 (tuned)) | lane3/1 | 13069067 | 30.192 | 432,870 | 1.23 | 3797 | 2 | ok |

_failed: karez-blocking s8: rustbench: unknown scenario s8 (s1 to s6)
_

_failed: sqlx s8: rustbench: unknown scenario s8 (s1 to s6)
_
