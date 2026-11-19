# karez status

_Updated 2026-11-16. Read this first, then [plan.md](design/plan.md)._

## State

karez is in the **design stage with a first working slice** on the branch `claude/m0-spike-1bacb7`
(local, not pushed, not merged): a PostgreSQL client with optional TLS. It has no release and
**first measurements** (a reduced campaign on 2026-11-14, one-lane experiments and a two-round, three-lane campaign on 2026-11-16 plus a third round and a tail run on 2026-11-18, [perf-program.md](design/perf-program.md) sections 17, 19 and 20; they support the README figures with their conditions, not a "fastest" claim). The owner decided on 2026-11-16 to go ahead with the current numbers and to **park the faster-network test** that would settle the plan's 2x bulk-read gate ([design/perf-program.md](design/perf-program.md) sections 9 to 15). It was verified against a real server on **PostgreSQL 14.25, 16.15 and 17.12;
versions 13, 15 and 18 were not run**. The repository is private at `github.com/pmsudhi/karez`. What was built,
tested and how to rerun it is in [design/m0-spike-results.md](design/m0-spike-results.md).

| area | state |
|---|---|
| Research | done: seven sourced notes in [research/](research/) and a synthesis in [design/research.md](design/research.md), with a list of unverified items in section 13 |
| Plan | done: [design/plan.md](design/plan.md), including the Milestone 0 gate, the architecture and the risks |
| Name and identity | decided 2026-11-09: **karez**, in the Cameleer family; Apache-2.0; a fresh repository with no rdbc history |
| Brand | K mark, icon set generator and assets are built; the logo and social preview are Gemini drafts that need small fixes ([brand/gemini-prompts.md](brand/gemini-prompts.md)) |
| Repo metadata | About line and 20 topics set; README, `llms.txt`, `AGENTS.md`, `CLAUDE.md`, `context7.json`, `CITATION.cff` and community files written |
| Code | M0 spike, PostgreSQL only: `karez-chunk`, `karez-proto-pg`, `karez-pool`, `karez-pg`, `karez-rt`, `karez` (zero dependencies, no `unsafe`; MSRV 1.85 tested); the whole workspace test run passes on 2026-11-16 against PostgreSQL 16 (47 real-server tests in `karez-pg`, 50 with `tls`; the 2026-11-13 counts of 76 and 57 are out of date); blocking client with pool, `COPY`, `LISTEN`/`NOTIFY`, cancel and batches, plus a shared pipelined connection with futures (`karez-rt`); optional TLS (`tls` feature); `scan_partitioned` and `scan_partitioned_pool` (a table read on several connections from one snapshot, ctid ranges, PostgreSQL 14+; the pool variant reuses connections) and `Connection::insert_batch` (multi-row insert), added 2026-11-16 with 7 unit tests and 5 real-server tests (pass on PostgreSQL 14, 16 and 17; the whole suite and a workspace check also pass on Rust 1.85.0, rechecked 2026-11-16); benchmark harness in `bench/` (built and tested in WSL; a reduced campaign ran on DigitalOcean on 2026-11-14 and campaign 2 (three lanes, two rounds, 14 clients) on 2026-11-16, see [bench/README.md](../bench/README.md) "Campaign log"; the figures are quotable with their conditions, and no "fastest" claim follows) |

## Decisions already made

- The project is a standalone repository named karez, not a fork of tokio-rs/rdbc.
- Licence: Apache-2.0.
- Architecture option D in the plan: new sans-io protocol crates, a pipelined connection,
  Arrow-layout chunks with a row view, a standards-shaped API, and ADBC, ODBC and JDBC exports.
- On single-row queries the target is parity with the fastest Rust client; the large-margin
  claims are for the bulk and columnar path only.
- `cameleer-tls` is an opt-in TLS backend only, until it is audited and has a hardware AES path.
- Oracle is out of 1.0 pending a legal review.

## Open decisions (plan section 11)

1. Go or no-go on Milestone 0. The owner chose on 2026-11-16 to continue with the current numbers (gate state in plan section 1.3: OLTP and latency met, bulk-read 2x met only without a network limit, size open); the faster-network test is parked (owner, 2026-11-16: "park this to be done"; the recommended route is a DigitalOcean tier request or a faster machine pair), not cancelled.
2. ~~Whether the ODBC driver is in 1.0 or deferred.~~ **Decided 2026-11-13 by the owner: ODBC is in 1.0.** It needs a separate FFI crate with `unsafe` (the core stays `forbid(unsafe_code)`), which the pre-1.0 audit must cover, and it adds months to the schedule; plan.md still needs updating.
3. ~~Whether to fund an audit of `cameleer-tls`.~~ Decided: not now (see below).
4. ~~The budget for the external audit before 1.0.~~ Decided 2026-11-16: no paid audit (see Decisions).
5. ~~Reserving the names.~~ Decided: now. The owner does it.

## Decisions of 2026-11-13 (owner)

- **TLS:** an opt-in cargo feature backed by the `rustls` crate; the core stays dependency-free. **Built 2026-11-13** ([design/tls-decision.md](design/tls-decision.md) stands in for the issue AGENTS.md asks for; no issue was opened). Verified on 14.25, 16.15 and 17.12. Over `karez-rt` too. `execute_many` over 64 KiB works on a blocking TLS connection since 2026-11-19 (sent in windows, each read before the next).
- **Async (`karez-rt`):** a small design with no async runtime (std-only I/O threads and channels over the sans-io crates), informed by what the simoom project learnt. The simoom session answered: thread per connection with sans-io crates suits a database client, and a pump thread only helps from two requests in flight (see [research/simoom-io-learnings.md](research/simoom-io-learnings.md), second-hand and unchecked). The design is in [design/karez-rt.md](design/karez-rt.md); steps 0 to 4 are built (section 13 of it lists the gaps), step 5 (benchmarks) is not.
- **Replay:** only commands never written to a dead connection move to a new one; a written command is never resent automatically.
- **Slow streams:** a stream leases its connection; other commands wait in a bounded queue. No automatic second connection.
- **Audit:** ~~plan and fund an external audit before 1.0~~ **Changed 2026-11-16 by the owner: no paid audit.** Assurance comes from tests, fuzzing (30 CPU-days without a new crash, plan M8), `cargo audit`, `cargo deny`, review of every `unsafe` block in the ODBC FFI crate and public review. This is a lower assurance than a paid audit for an ODBC driver that BI tools would load; the risk is recorded in plan section 9.
- **Names:** reserve `karez` and `karez-*` on crates.io, PyPI and npm now, as minimal placeholders. **crates.io done 2026-11-16** (`karez`, `karez-chunk`, `karez-proto-pg`, `karez-pg`, `karez-rt`, version 0.0.1, no code; the owner told me to publish them with the existing cargo login). **npm and PyPI not done**: there is no login on this machine (`npm whoami` fails, no PyPI token); the packages are built and checked in `packaging/placeholders/`, and need the owner to log in. Planned names beyond these five (`karez-odbc`, `karez-adbc`, `karez-jdbc`, ...) are not reserved: crates.io forbids squatting, so reserve each when it has code.
- **PostgreSQL versions:** the real-server suite passes on 14.25, 16.15 and 17.12 (13, 15, 18 not run).

## Milestone 1 (started 2026-11-18)

The owner moved the project from Milestone 0 to Milestone 1 on 2026-11-18 and chose to start with the core API and the fuzz and differential harnesses. The plan, refined against the code, is [design/m1-plan.md](design/m1-plan.md). Step 1 is done: SQLSTATE class and a transient flag on errors, and a std-only mutation fuzzer for every server-to-client parser (400,000 mutated streams per target in release mode, no panic; not coverage-guided). Step 1b is done: the owner decided on 2026-11-18 to extract the pool now, so `karez-pool` is a driver-neutral crate (a `Manager` trait and a first-come-first-served `Pool`) and `karez-pg`'s `Pool` is a thin wrapper over it with the same API (m1-plan.md section 3). The two large-batch tests that failed on Windows were not a library bug: the WSL localhost relay stalls on large two-way transfers (a plain echo server does too), and all 47 real-server tests pass on Windows through `tests/docker/windows-test.ps1` (tests/docker/README.md, "Windows"). Step 2 is done: multi-host URLs, `target_session_attrs` and `load_balance_hosts` (m1-plan.md step 2; tested against a real primary and standby on PostgreSQL 16, 2026-11-19). Next: step 3, native types.

## Next steps

- [ ] Fix the logo tagline and the social preview caption, and make the unlabelled social image
      (prompts in [brand/gemini-prompts.md](brand/gemini-prompts.md)).
- [ ] Upload the social preview in the repository settings (GitHub has no API for it).
- [x] Workspace with `karez-chunk`, `karez-proto-pg`, `karez-pg` and `karez`, and a PostgreSQL
      integration fixture in `tests/docker/` (done on the spike branch).
- [x] Blocking pool, `COPY` in and out, `LISTEN`/`NOTIFY`, cancel, write batches, and decoders for
      `numeric`, `json`, `jsonb`, `interval`, `time`, `inet`, enums, arrays and more (rendered to
      text); MSRV 1.85 tested.
- [x] Run the same suite on PostgreSQL 14.25 and 17.12 (all 39 real-server tests pass on both). 13, 15 and 18 are not run.
- [x] TLS (`sslmode=verify-full` default, `tls` feature, rustls). The shared connection works over TLS.
- [x] `karez-rt`: shared pipelined connection with futures (built; see [design/karez-rt.md](design/karez-rt.md) section 13 for the gaps).
- [x] The `bench/` load generator: open loop, SLO-throughput search, adapters for 16 PostgreSQL clients (Rust, Python, Go, C, Java), bulk scenarios S4 to S6, DigitalOcean scripts. Tested in WSL on PostgreSQL 17 and 18, plain and TLS. See [bench/README.md](../bench/README.md).
- [x] Campaign 2 on DigitalOcean (2026-11-16): three lanes, two rounds of `lean2`, clients rotated across lanes (n=2 per competitor, n=6 for karez); results in `bench/results/2026-11-16-campaign2/`. Round 3 (2026-11-18, cut at its stop time, bulk only on lane 2): `bench/results/2026-11-18-round3/`, [perf-program.md](design/perf-program.md) section 19. Its tail run (2026-11-18, four `c-4` droplets, lanes 1 and 3: the cut-off bulk and clients, about $1) and the combined report with 95% intervals (11 lane-rounds, karez n=11 point queries and n=9 bulk, competitors n=3): `bench/results/2026-11-18-round3-tail/`, [perf-program.md](design/perf-program.md) section 20; diesel and wtx (S1/S2) added, pgbench cross-check on droplets in 20 cells. Still open: TLS bulk reads, diesel R2/R3/R4 and S5/S6/S8, and the faster network (parked).
- [ ] Acquire the two primary texts the research could not read (JDBC 4.3 chapter 6, ISO
      9075-3:2023) before freezing the parity checklist.
- [ ] Resolve the unverified items in research.md section 13 before quoting them outside the repo.
- [x] README headline: strongest measured figure with its conditions and the pending-test note (owner decision 2026-11-16).
- [ ] Make the repository public with the M1 PostgreSQL driver and the published benchmark
      results, so the first public claim can be rerun.
