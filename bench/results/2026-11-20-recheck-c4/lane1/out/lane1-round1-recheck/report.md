# Benchmark report

Generated from the raw cells in `cells.jsonl`. Every number is `[measured here]` on the machine in `machine.json`. Ratios compare clients **only within one table row group of the same lane and round**.


## S8 unpipelined, 512 connections, pool 32 (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| libpq | lane1/x2 | 38000 | 379 | 1630 | 5079 | 45.97 | 20 | 3 |
| pgx | lane1/x2 | 37636 | 350 | 1925 | 3424 | 50.81 | 30 | 3 |
| karez-blocking | lane1/x2 | 31040 | 358 | 1450 | 3391 | 47.77 | 18 | 3 |

## S8T unpipelined, 512 connections, pool 32 (p99 bound 2.0 ms)

| client | lane/round | SLO rate/s | p50 us | p99 us | p99.9 us | CPU-s per M req | RSS MB | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| libpq | lane1/x2 | 31871 | 420 | 1925 | 7307 | 61.24 | 26 | 3 |
| karez-blocking | lane1/x2 | 30100 | 395 | 1663 | 2867 | 52.91 | 19 | 3 |
| pgx | lane1/x2 | 26468 | 362 | 1761 | 3752 | 57.78 | 35 | 3 |

## Bulk scenarios (closed loop; seconds are per timed pass, medians)

S4 = scan of 1,000,000 rows x 10 columns; S5 = batch insert; S6 = COPY. Rows per second is the headline here; CPU-seconds per million rows and peak RSS show the cost. `checksum` column: every client must fold the same S4 checksum, or the row says MISMATCH.


### S4

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/x2 | 1000000 | 1.128 | 886,368 | 0.76 | 6 | 5 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x2 | 1000000 | 1.312 | 762,137 | 0.76 | 6 | 5 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x2 | 1000000 | 1.449 | 690,322 | 1.12 | 7 | 5 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/x2 | 1000000 | 1.490 | 671,141 | 0.86 | 6 | 5 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x2 | 1000000 | 1.508 | 662,954 | 0.67 | 6 | 5 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x2 | 1000000 | 1.583 | 631,552 | 0.93 | 6 | 5 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x2 | 1000000 | 1.589 | 629,406 | 1.00 | 7 | 5 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x2 | 1000000 | 1.646 | 607,644 | 1.14 | 7 | 5 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/x2 | 1000000 | 1.707 | 585,926 | 0.87 | 6 | 5 | ok |

### S4 over TLS (sslmode=verify-full)

| client | lane/round | rows | median s | rows/s | CPU-s per M rows | peak RSS MB | runs | checksum |
|---|---|---:|---:|---:|---:|---:|---:|---|
| tokio-postgres (tokio-postgres 0.7) | lane1/x2 | 1000000 | 0.971 | 1,029,972 | 1.01 | 8 | 5 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x2 | 1000000 | 1.010 | 989,805 | 1.04 | 8 | 5 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/x2 | 1000000 | 1.577 | 633,995 | 0.82 | 8 | 5 | ok |
| tokio-postgres (tokio-postgres 0.7) | lane1/x2 | 1000000 | 1.591 | 628,575 | 1.10 | 9 | 5 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x2 | 1000000 | 1.699 | 588,478 | 0.81 | 7 | 5 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/x2 | 1000000 | 1.780 | 561,766 | 0.85 | 7 | 5 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x2 | 1000000 | 1.828 | 547,166 | 1.08 | 7 | 5 | ok |
| karez-blocking-chunk (karez-pg 0.0.0 (chunk path)) | lane1/x2 | 1000000 | 1.829 | 546,657 | 1.06 | 7 | 5 | ok |
| karez-blocking (karez-pg 0.0.0 (row view)) | lane1/x2 | 1000000 | 1.836 | 544,722 | 1.02 | 7 | 5 | ok |
