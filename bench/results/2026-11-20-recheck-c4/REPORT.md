# Recheck on `c-4` in `sfo2`, 2026-11-20: S8 after the pool fix, and S4 over TLS

One lane: two `c-4` droplets (4 vCPU, 8 GB; PostgreSQL 18.7) in `sfo2`, created about 02:44 UTC and deleted at 03:15 UTC (about 35 minutes, about $0.15). The owner's tenth spend exception (CLAUDE.md): `c-4` was no longer available in `nyc1`, so `sfo2` was chosen in chat. **Another region and droplet pair than every earlier run, so none of these numbers is ranked with them.** Script: `bench/cloud/lane-recheck.sh` (round label `x2`). Raw cells: `lane1/out/lane1-round1-recheck/cells.jsonl`. All figures `[measured here, 2026-11-20]`.

## S8 after the pool fix (512 workers on a pool of 32, open loop, p99 bound 2 ms, median of 3 confirm runs)

| client | plain: SLO rate/s | over TLS: SLO rate/s |
|---|---:|---:|
| libpq | 38,000 | 31,871 |
| pgx | 37,636 | 26,468 |
| **karez-blocking** | **31,040** | **30,100** |

- **The collapse is gone.** On 2026-11-19 (pool before the fix, `nyc1`) karez held 10,000 requests a second and collapsed above it (p99 7 to 8 s). Now it holds 31,040 plain and 30,100 over TLS inside the 2 ms bound, with no drop or error up to 64,000 requests a second offered (the p99 grows beyond the bound above about 34,000, as for the other two).
- karez is 0.82x libpq and 0.82x pgx plain, 0.94x libpq and 1.14x pgx over TLS. The search steps are coarse (differences under about 10% are noise) and n=3 confirm runs; one lane.
- Not done: the wait-time distribution against Erlang C.

## S4 (1,000,000 rows x 10 columns), chunk path against row view against tokio-postgres, plain and over TLS

Three rounds of five passes per cell, in alternating order (the middle round reversed). 15 passes per cell:

| client | plain, median s | TLS, median s | TLS cost | passes (min to max, s) plain / TLS |
|---|---:|---:|---:|---|
| karez, chunk path | 1.49 | 1.78 | +19% | 1.02 to 1.71 / 0.90 to 1.85 |
| karez, row view | 1.45 | 1.83 | +26% | 0.68 to 1.73 / 0.92 to 1.85 |
| tokio-postgres | 1.58 | 1.06 | -33% | 0.97 to 1.73 / 0.77 to 1.62 |

- **The chunk path over TLS is not worse than the row view over TLS** (1.78 s against 1.83 s), and its TLS cost (+19%) matches the local result (10% to 20%) and the row view's. **The 44% of 2026-11-19 did not reproduce in a second environment either.** It is not a property of the chunk path. It stays unexplained (one cell on one droplet pair).
- **This pair is slow and unstable for every client.** Passes of one cell range from 0.7 to 1.85 s, and many passes land on the same few values (1.83 to 1.85, about 1.6, about 1.7), as if the transfer were limited in steps. On 2026-11-19 in `nyc1` the same scan took 0.6 to 0.7 s. The client CPU per pass was also two to three times higher (0.8 to 1.0 s against 0.3 to 0.5 s), so the machines, not only the network, were slower. **S4 here is not a clean measurement.**
- **An open observation, not a finding:** over TLS karez (both paths) was slower than tokio-postgres on this pair (1.8 s against 1.06 s), where in `nyc1` the row view was level with it (0.99x). With passes this spread it could be the pair; it could also be how karez reads TLS records. A cleaner test (a quieter pair, or a longer scan, alternating more) would be needed before saying either.
