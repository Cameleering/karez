# Campaign 2: plan (not started)

_Written 2026-11-15. Nothing here has run. It needs a new written go-ahead from the owner: the exception in
[CLAUDE.md](../CLAUDE.md) covered the 2026-11-14 campaign only._

## What campaign 1 taught

Evidence: [README Campaign log](README.md#campaign-log), `bench/results/2026-11-14-lean/`.

| finding | consequence for campaign 2 |
|---|---|
| The databases were OOM-killed (page tables of ~1000 backends over 2 GB shared buffers). | Huge pages become part of `setup-db.sh`, and the run checks the database between cells. |
| The clients ran 5 hours against a dead server and recorded only errors. | A health check before each cell; three failed cells in a row abort that client and the lane records why. |
| A `full` round needs about 6 hours per lane (7 clients at about 50 minutes). Three rounds need 18 hours; the owner's cap is 12. | Two rounds, each shorter. See the time budget below. |
| R3 met no bound and R4 is dominated by its 5% aggregate. Neither separates clients. | Change them first (see "Fix before booking"). |
| One pass (sqlx S8) took over an hour and blocked lane 3. | Every cell and every bulk pass gets a time limit. |
| n=1 per competitor, noise of up to 60% at one connection between lanes. | Rotation across lanes in round 2, so every client has n=2 on different droplets. |
| The pull step copied smoke and aborted directories into the report. | `ctl.sh pull` takes only the directories of the campaign's rounds. |
| The old `campaign.sh` kept running after its task was stopped. | A lock file and a stop file; `campaign.sh` checks the stop file between steps. |

## Time budget (cap 12 hours from `ctl.sh up`)

| step | hours |
|---|---:|
| `ctl.sh up`, ship, build, load the 13M-row table, verify | 1.0 |
| round 1 (`lean2`, below) | 4.0 |
| round 2 (same profile, groups rotated) | 4.0 |
| pull, report, `down` | 0.5 |
| margin | 2.5 |
| **total** | **12.0** |

The margin is the point: campaign 1 lost 5 hours to one failure. With the health check and the time limits a
failure costs minutes. The safety net is armed for 11.5 hours, `campaign.sh` stops waiting at 10.5 hours and pulls
what exists. Cost at the DigitalOcean list price for `c-4` in `blr1` ($0.125 per droplet per hour, six droplets,
`[vendor, DO API, checked 2026-11-14]`): at most $9.

If the owner prefers a bigger sample over a shorter run: a third round does not fit in 12 hours. A second campaign on
another day does.

## Fix before booking (local, no cost)

1. **R3 and R4.** R3: search from 5 requests per second and use a bound that a 20 to 40 ms query can meet (60 ms
   was never met because p99 was 1.2 seconds; first find out why: probably cold pages after the restart, so add a
   warm-up pass over the table before the sweep). R4: keep the mix but report p99 per query kind, because the
   aggregate decides the overall p99.
2. **Time limits.** `run.py`: a cell that runs over 6 minutes and a bulk pass that runs over 15 minutes are killed and
   recorded as `timeout`. Reproduce the sqlx S8 slowness in WSL first; it may be a real adapter defect.
3. **Health check.** Before each config, `SELECT 1` through psql; on failure wait 60 s and retry; after three failures
   abort the client with the database's `dmesg`/log tail in the record.
4. **Rate steps.** Finer steps near the knee (2.5% instead of 5%) in the last bisect step, so a 5% difference is not
   one step. Keep `repeats` at 3.
5. **`setup-db.sh`.** Huge pages (`vm.nr_hugepages` sized to `shared_buffers`), and a note in the machine file.
6. **`ctl.sh pull`.** Only the round directories of this campaign. **`campaign.sh`.** Lock file and stop file.
7. **pgbench cross-check.** Run `crosscheck.py` once on a database host before round 1 and keep its output; it
   decides whether the harness's absolute latencies can be trusted.
8. **S2 at one connection.** Confirm in the adapters that deadpool, vertx and zero-postgres use one connection when
   `conns=1` (deadpool and tokio-postgres differed by 60% on the same driver).
9. **`--clients` per lane.** Rebalance so each lane has about the same number of cell-hours (zero-postgres is cheap;
   the JVM clients and sqlx S8 are expensive).

## Profile `lean2`

As `lean` (11 configurations, 3 repeats, 10 s runs, bisect 3 plus the finer last step), with the R3 and R4 changes,
`s2@8` back in (it separates clients at a low load), and S6 at 3M rows. Bulk S4, S5, S6 and S8 for every client that
has them. About 4 hours for the largest lane if the time limits hold.

## Layout and rotation

Same layout as campaign 1: three lanes, a database and a client droplet each, `c-4`, `blr1`, PostgreSQL 18.
karez-blocking and karez-shared run in every lane and round. The 13 other clients are split into three groups; in
round 1 lane `l` runs group `l`, in round 2 group `(l mod 3) + 1`. So each client has n=2, on two different lanes,
and the report's ratio column is a median of two within-lane ratios. Run the checksum `verify` at the start of each
round (it takes minutes).

## What the report may say afterwards

Only what the tables show, with the losses next to the wins, the lane count and `[measured here]`. If after n=2 a
client's two ratios disagree by more than 15%, the report says so and does not rank it. "Level with" is allowed when
the ranges overlap. No headline number goes on the README, the About line or an image before the owner has read the
full tables.

## Open questions for the owner

1. Go-ahead for campaign 2 as above (two rounds, at most 12 hours, at most about $9), recorded in CLAUDE.md the way the
   2026-11-14 exception was.
2. Whether the local fixes in "Fix before booking" may be done first (they cost no money). Recommended: yes, and
   review the diff before booking.
3. Whether to skip the slowest clients in round 2 to save time (pgjdbc defaults on S8, which fails by design).
