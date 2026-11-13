# karez status

_Updated 2026-11-13. Read this first, then [plan.md](design/plan.md)._

## State

karez is in the **design stage with a first working slice** on the branch `claude/m0-spike-1bacb7`
(local, not pushed, not merged): a PostgreSQL client with optional TLS. It has no release and
**no measurement**. It was verified against a real server on **PostgreSQL 14.25, 16.15 and 17.12;
versions 13, 15 and 18 were not run**. The repository is private at `github.com/pmsudhi/karez`. What was built,
tested and how to rerun it is in [design/m0-spike-results.md](design/m0-spike-results.md).

| area | state |
|---|---|
| Research | done: seven sourced notes in [research/](research/) and a synthesis in [design/research.md](design/research.md), with a list of unverified items in section 13 |
| Plan | done: [design/plan.md](design/plan.md), including the Milestone 0 gate, the architecture and the risks |
| Name and identity | decided 2026-11-09: **karez**, in the Cameleer family; Apache-2.0; a fresh repository with no rdbc history |
| Brand | K mark, icon set generator and assets are built; the logo and social preview are Gemini drafts that need small fixes ([brand/gemini-prompts.md](brand/gemini-prompts.md)) |
| Repo metadata | About line and 20 topics set; README, `llms.txt`, `AGENTS.md`, `CLAUDE.md`, `context7.json`, `CITATION.cff` and community files written |
| Code | M0 spike, PostgreSQL only: `karez-chunk`, `karez-proto-pg`, `karez-pg`, `karez-rt`, `karez` (zero dependencies, no `unsafe`; MSRV 1.85 tested); 76 tests that run without a server and 53 that need one (133 and 57 with `tls`); blocking client with pool, `COPY`, `LISTEN`/`NOTIFY`, cancel and batches, plus a shared pipelined connection with futures (`karez-rt`); optional TLS (`tls` feature); no benchmarks |

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

1. Go or no-go on Milestone 0. The functional spike exists; the measurement part of the gate (plan section 1.3) has not been started, so there is nothing to decide on yet.
2. ~~Whether the ODBC driver is in 1.0 or deferred.~~ **Decided 2026-11-13 by the owner: ODBC is in 1.0.** It needs a separate FFI crate with `unsafe` (the core stays `forbid(unsafe_code)`), which the pre-1.0 audit must cover, and it adds months to the schedule; plan.md still needs updating.
3. ~~Whether to fund an audit of `cameleer-tls`.~~ Decided: not now (see below).
4. The budget for the external audit before 1.0: scope decided, the amount is the owner's.
5. ~~Reserving the names.~~ Decided: now. The owner does it.

## Decisions of 2026-11-13 (owner)

- **TLS:** an opt-in cargo feature backed by the `rustls` crate; the core stays dependency-free. **Built 2026-11-13** ([design/tls-decision.md](design/tls-decision.md) stands in for the issue AGENTS.md asks for; no issue was opened). Verified on 14.25, 16.15 and 17.12. Gaps: not over `karez-rt` yet, `execute_many` over 64 KiB refused, no name-mismatch test.
- **Async (`karez-rt`):** a small design with no async runtime (std-only I/O threads and channels over the sans-io crates), informed by what the simoom project learnt. The simoom session answered: thread per connection with sans-io crates suits a database client, and a pump thread only helps from two requests in flight (see [research/simoom-io-learnings.md](research/simoom-io-learnings.md), second-hand and unchecked). The design is in [design/karez-rt.md](design/karez-rt.md); steps 0 to 4 are built (section 13 of it lists the gaps), step 5 (benchmarks) is not.
- **Replay:** only commands never written to a dead connection move to a new one; a written command is never resent automatically.
- **Slow streams:** a stream leases its connection; other commands wait in a bounded queue. No automatic second connection.
- **Audit:** plan and fund an external audit before 1.0 of the core, the protocol crates and the ODBC FFI crate; not of `cameleer-tls` for now. (The owner funds it; nothing was spent.)
- **Names:** reserve `karez` and `karez-*` on crates.io, PyPI and npm now, as minimal placeholders. The owner does this; I do not create accounts or publish.
- **PostgreSQL versions:** the real-server suite passes on 14.25, 16.15 and 17.12 (13, 15, 18 not run).

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
- [x] TLS (`sslmode=verify-full` default, `tls` feature, rustls). Next: TLS inside the `karez-rt` engine and a name-mismatch test.
- [x] `karez-rt`: shared pipelined connection with futures (built; see [design/karez-rt.md](design/karez-rt.md) section 13 for the gaps).
- [ ] The `bench/` load generator.
- [ ] Build the benchmark harness first, and baseline the competitors listed in plan section 6.3
      on two machines before making any speed claim.
- [ ] Acquire the two primary texts the research could not read (JDBC 4.3 chapter 6, ISO
      9075-3:2023) before freezing the parity checklist.
- [ ] Resolve the unverified items in research.md section 13 before quoting them outside the repo.
- [ ] Make the repository public with the M1 PostgreSQL driver and the published benchmark
      results, so the first public claim can be rerun.
