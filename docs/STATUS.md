# karez status

_Updated 2026-11-13. Read this first, then [plan.md](design/plan.md)._

## State

karez is in the **design stage with a first working slice** on the branch `claude/m0-spike-1bacb7`
(local, not pushed, not merged): a blocking PostgreSQL client with no TLS. It has no release and
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
| Code | M0 spike, PostgreSQL only: `karez-chunk`, `karez-proto-pg`, `karez-pg`, `karez` (zero dependencies, no `unsafe`; MSRV 1.85 tested); 69 tests that run without a server and 39 that need one; blocking only, with a pool, `COPY`, `LISTEN`/`NOTIFY`, cancel and batches; no TLS, no async, no benchmarks |

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
3. Whether to fund an audit of `cameleer-tls`.
4. The budget for the external audit before 1.0.
5. Reserving the `karez` and `karez-*` names on crates.io, PyPI and npm, now or at M0. All were
   free on 2026-11-09.

## Decisions of 2026-11-13 (owner)

- **TLS:** an opt-in cargo feature backed by the `rustls` crate; the core stays dependency-free. AGENTS.md requires an issue that discusses the dependency first, so open that issue before any code. Until then the client refuses anything but `sslmode=disable`.
- **Async (`karez-rt`):** a small design with no async runtime (std-only I/O threads and channels over the sans-io crates), informed by what the simoom project learnt. A question to the simoom session is out; the answer is not yet in this repository.
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
- [ ] Add TLS (`sslmode=verify-full` by default per plan section 1.2); until then the client refuses
      anything but `sslmode=disable`.
- [ ] Add `karez-rt` (async connection task, pipelining) and the `bench/` load generator.
- [ ] Build the benchmark harness first, and baseline the competitors listed in plan section 6.3
      on two machines before making any speed claim.
- [ ] Acquire the two primary texts the research could not read (JDBC 4.3 chapter 6, ISO
      9075-3:2023) before freezing the parity checklist.
- [ ] Resolve the unverified items in research.md section 13 before quoting them outside the repo.
- [ ] Make the repository public with the M1 PostgreSQL driver and the published benchmark
      results, so the first public claim can be rerun.
