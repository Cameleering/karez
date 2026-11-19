# CLAUDE.md — project memory for karez

Claude Code loads this file automatically. Read it first, then [docs/STATUS.md](docs/STATUS.md)
for the current state and next steps. The agent-facing rules are in [AGENTS.md](AGENTS.md).

@docs/STATUS.md
@AGENTS.md

## What this project is

karez is a planned Rust database connectivity library: one API and native drivers for
PostgreSQL, MySQL/MariaDB, SQL Server and SQLite, with JDBC, ODBC and ADBC parity and exports,
and Arrow-native results. It belongs to the Cameleer family (a desert and camel naming theme),
next to Cameleer (integration engine) and simoom (HTTP engine). It is in the design stage with a first working slice:
research and plan are done, and a PostgreSQL client (optional TLS, verified on PostgreSQL 13.23, 14.25, 15.20, 16.16, 17.11 and 18.7, 2026-11-19) exists on the branch `claude/m0-spike-1bacb7`.

The goal given by the owner: a complete, ground-up rewrite that is significantly faster than
native JDBC and every Rust alternative where the evidence supports it, smaller in binary size, and
at feature parity with the standards, with every claim measured honestly.

## Working agreements

1. **Do not commit or push unless the owner asks.** The default branch is `main`: branch first.
2. **Evidence first.** Tag every number, cite its source and check date, and never rank numbers
   from different machines together. Do not claim "fastest" or any speed-up until a benchmark
   following the plan's method supports it. Exception (owner, 2026-11-16): a headline may use the strongest measured figure if its
   conditions and the realistic-network figure are in the same place and the full test is stated as pending (see AGENTS.md).
3. **Follow the plan.** The hard rules are in [docs/design/plan.md](docs/design/plan.md) section
   1.2. If the evidence contradicts the plan, say so and propose a change to the plan; do not
   quietly drift from it.
4. **Std-only core.** No dependencies in the core or the protocol crates; optional features may
   have them after an issue discusses it.
5. **Never create cloud accounts, accept terms, publish crates or spend money** on the owner's
   behalf. Reserving crate names is the owner's decision.
   One exception, given in writing by the owner on 2026-11-14: the benchmark campaign may create and delete
   DigitalOcean droplets (tag `karez-bench`, six `c-4` in `blr1`, at most 12 hours, deleted by
   `bench/cloud/safety-net.sh` at a hard deadline) with the owner's token file, which only those scripts read
   and which is never printed.
   A second, smaller exception, given in chat by the owner on 2026-11-16 ("Approve one lane"): one lane only, two `c-4`
   droplets in `blr1` (tag `karez-bench`, `LANES=1 bash bench/cloud/ctl.sh up`), at most 5 hours (about $1.25), to confirm the
   first performance experiments (docs/design/perf-program.md); deleted by the same safety net. Campaign 2 is not approved.
   A third, given in chat on 2026-11-16 ("Fix, then larger droplet"): the same one-lane layout once more with a larger class (`DO_SIZE=c-8`,
   two droplets, at most 3 hours, about $1.50), to see whether a faster network changes the bulk-read result. `c-8` and larger are restricted for the account (DigitalOcean asks for a
   support ticket to raise the tier), so the run used a second `c-4` pair for 25 minutes instead.
   A fourth, given in chat on 2026-11-16 ("Run it now on c-4", answering whether to run campaign 2 now): the benchmark campaign again, as in the
   first exception: six `c-4` in `blr1` (tag `karez-bench`), a smoke round and then two rounds of the profile `lean2`, at most 12 hours
   (safety net 10 hours, the campaign script stops waiting at 9.5 hours), about $9, deleted by `bench/cloud/campaign.sh` at the end and by the safety net at
   the deadline.
   A fifth, given in chat on 2026-11-16 ("you do it yourself. all previous packages in other projects done same by pushing initial version"): publish
   minimal name-reserving placeholder packages (no code) for `karez` and the four existing `karez-*` crates, using the owner's existing registry
   logins; credentials are never typed or read by the agent. Done on crates.io on 2026-11-16 (version 0.0.1). npm and PyPI wait for the owner to log in.
   A sixth, given in chat on 2026-11-18 (answering "Round 3, six c-4 (Recommended)"): one more `lean2` round (round 3, rotation continues) plus the
   pgbench cross-check and a smoke round, six `c-4` in `blr1` (tag `karez-bench`), at most 6 hours (safety net 6 hours, the campaign script stops waiting
   at 5.5 hours), about $4.50, deleted by `bench/cloud/campaign.sh` at the end and by the safety net at the deadline.
   A seventh, given in chat on 2026-11-18 (answering "Finish round 3 bulk on droplets"): the part of round 3 the stop time cut off, on fresh droplets (`bench/cloud/lane-tail.sh`),
   at most six `c-4` in `blr1` (tag `karez-bench`; four are planned, lanes 1 and 3), at most 6 hours (safety net 6 hours), about $5 at the most, deleted by `bench/cloud/campaign.sh` at the end and by the safety net at the deadline.
   An eighth, given in chat on 2026-11-19 (answering "One lane, TLS + S6 (Recommended)" to the question about M1 step 9): one lane only, two `c-4` in `blr1`
   (tag `karez-bench`, `LANES=1`), at most 4 hours (safety net 4 hours), about $1, for S6 against PgBulkInsert and the TLS runs of the exit measurements
   (docs/design/m1-plan.md step 9); the adapters are built and tested locally first, and no droplet is created before they work; deleted by the same safety net.
6. `assets/` is generated by `scripts/brand/build_icons.py`; edit the script, not the output.

## Environment notes

- The owner works on Windows 11 with Git Bash and PowerShell. Use forward slashes in Bash.
- Rendering the icon PNGs uses headless Microsoft Edge and Pillow.
- Other Cameleer-family repositories live next to this one in `E:\Work\DataHQ\Code\` (`Rust-camel`,
  `simoom`). Read them for house style; do not change them from this repository.

## Writing style for docs in this repository

Short sentences, one idea each. State facts and conclusions, with the source next to the number.
No marketing adjectives. Say "planned" or "design target" for anything not built.
