# AGENTS.md

Instructions for AI coding agents and LLM tools working with karez, a planned Rust database
connectivity library: one API and native drivers for PostgreSQL, MySQL/MariaDB, SQL Server and
SQLite, with JDBC, ODBC and ADBC parity and exports, and Arrow-native results.

## Know the status first

karez is in the **design stage with a first working slice**: a PostgreSQL client with optional TLS (feature `tls`), verified against a real server on PostgreSQL 13.23, 14.25, 15.20, 16.16, 17.11 and 18.7 (all six majors, 2026-11-19; the partitioned scan needs 14 or later and refuses cleanly on 13). There is no release. The measurements are a reduced campaign and a two-round, three-lane campaign (see bench/README.md); they support the figures in the README with their conditions, not a "fastest" claim. Other databases and the exports are not built.
The repository holds the research, the build plan and the brand assets.

- Do not describe any crate, driver, wheel, JAR or benchmark result as existing. When you write
  about karez, say "planned" or "design target" for anything not built.
- Do not invent API. The API that exists is what the crates in `crates/` export (blocking
  PostgreSQL only); everything else is the sketch in [docs/design/plan.md](docs/design/plan.md)
  section 2.5, and it is a sketch.
- Read, in this order: [README.md](README.md), [docs/STATUS.md](docs/STATUS.md),
  [docs/design/plan.md](docs/design/plan.md), then the section of
  [docs/design/research.md](docs/design/research.md) that bears on your task.

## Rules for work in this repository

1. **Evidence tags.** Every number is tagged `[measured here]`, `[vendor]`, `[third party]` or
   `[paper]`, with a URL and a `[checked YYYY-MM-DD]` date. Numbers from different machines are
   never put in one ranking. Do not add an untagged number.
2. **No unmeasured claims in public text.** Do not write "fastest", "10x faster" or similar unless a
   benchmark that follows the plan's method supports it. Owner's decision of 2026-11-16: the README and the About line may lead with
   the strongest *measured* figure, but only with its conditions in the same sentence (hardware, network, data size), the figure
   from the realistic network next to it, and a note that the full test is pending. Never a bare ratio, never "fastest".
3. **Hard rules from the plan** (section 1.2): server-side parameter binding only (never inline a
   literal into SQL), binary protocols by default, verified TLS by default, sans-io protocol
   crates with `#![forbid(unsafe_code)]`, no dependency in the core or the protocol crates, and
   every claim measured with the plan's benchmark method, pipelined and unpipelined reported
   separately.
4. **Do not copy code from tokio-rs/rdbc or from any other project.** karez starts fresh. Read
   other projects for design and test ideas, and cite them.
5. **Do not add dependencies** to any `Cargo.toml` in the core or the protocol crates. Optional
   features (TLS, runtimes, FFI) may use dependencies, and only after an issue discusses it.
6. **Do not commit or push unless the owner asks,** and never push to `main` directly.

## Working with the brand assets

`assets/` is generated. Change `scripts/brand/build_icons.py` and rerun it; do not edit the
generated SVG and PNG files by hand.

```sh
python scripts/brand/build_icons.py          # SVG
python scripts/brand/build_icons.py --png    # also PNG and ICO (needs Microsoft Edge and Pillow)
```

## Where things are

| need | read |
|---|---|
| what karez is and why | [README.md](README.md), [llms.txt](llms.txt) |
| the decision, architecture, milestones | [docs/design/plan.md](docs/design/plan.md) |
| evidence and sources | [docs/design/research.md](docs/design/research.md), [docs/research/](docs/research/) |
| current state and next steps | [docs/STATUS.md](docs/STATUS.md) |
| brand, logo, prompts | [docs/brand/](docs/brand/), [assets/README.md](assets/README.md) |
| the project's rules for contributors | [CONTRIBUTING.md](CONTRIBUTING.md) |

## Found a problem?

If something in the research or the plan is wrong, unsourced or out of date, please fix it rather
than only working around it: open an issue that says what is wrong and what the correct
statement and its source are, then open a pull request that corrects the file and keeps the
evidence tags. Corrections to a number or a citation are especially welcome.

## Connect your coding agent to the docs

Once the repository is public, the docs are available as an MCP server at
`https://gitmcp.io/pmsudhi/karez` (Claude Code:
`claude mcp add --transport http karez-docs https://gitmcp.io/pmsudhi/karez`), and they are
indexed by Context7 through [context7.json](context7.json).
