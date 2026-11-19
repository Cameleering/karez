# karez status

_Updated 2026-11-09. Read this first, then [plan.md](design/plan.md)._

## State

karez is in the **design stage**. There is no library code, no release and no measurement. The
repository is private at `github.com/cameleering/karez`.

| area | state |
|---|---|
| Research | done: seven sourced notes in [research/](research/) and a synthesis in [design/research.md](design/research.md), with a list of unverified items in section 13 |
| Plan | done: [design/plan.md](design/plan.md), including the Milestone 0 gate, the architecture and the risks |
| Name and identity | decided 2026-11-09: **karez**, in the Cameleer family; Apache-2.0; a fresh repository with no rdbc history |
| Brand | K mark, icon set generator and assets are built; the logo and social preview are Gemini drafts that need small fixes ([brand/gemini-prompts.md](brand/gemini-prompts.md)) |
| Repo metadata | About line and 20 topics set; README, `llms.txt`, `AGENTS.md`, `CLAUDE.md`, `context7.json`, `CITATION.cff` and community files written |
| Code | none |

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

1. Go or no-go on Milestone 0 (nothing has been started).
2. Whether the ODBC driver is in 1.0 or deferred.
3. Whether to fund an audit of `cameleer-tls`.
4. The budget for the external audit before 1.0.
5. Reserving the `karez` and `karez-*` names on crates.io, PyPI and npm, now or at M0. All were
   free on 2026-11-09.

## Next steps

- [ ] Fix the logo tagline and the social preview caption, and make the unlabelled social image
      (prompts in [brand/gemini-prompts.md](brand/gemini-prompts.md)).
- [ ] Upload the social preview in the repository settings (GitHub has no API for it).
- [ ] Decide on Milestone 0, then create the workspace: `karez-chunk`, `karez-proto-pg`,
      `karez-rt` and the `bench/` load generator.
- [ ] Build the benchmark harness first, and baseline the competitors listed in plan section 6.3
      on two machines before writing driver code.
- [ ] Acquire the two primary texts the research could not read (JDBC 4.3 chapter 6, ISO
      9075-3:2023) before freezing the parity checklist.
- [ ] Resolve the unverified items in research.md section 13 before quoting them outside the repo.
- [ ] Make the repository public with the M1 PostgreSQL driver and the published benchmark
      results, so the first public claim can be rerun.
