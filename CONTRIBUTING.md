# Contributing to karez

Thanks for helping. karez is a planned Rust database connectivity library, and right now it is
in the design stage: the useful contributions are corrections to the research, reviews of the
plan and the benchmark method, and reviews of the PostgreSQL client in `crates/` (Milestone 1 is in progress).

## Ground rules

1. **Evidence first.** Every number is tagged `[measured here]`, `[vendor]`, `[third party]` or
   `[paper]`, with a URL and a `[checked YYYY-MM-DD]` date. Numbers from different machines are
   never put in one ranking. A pull request that adds an untagged number will be asked to tag it.
2. **No unmeasured claims.** Public text (README, About, images) does not say "fastest" or give a
   speed-up until a benchmark that follows [docs/design/plan.md](docs/design/plan.md) section 6
   supports it. Targets are labelled as targets.
3. **Follow the plan's hard rules** (section 1.2): server-side parameter binding only, binary
   protocols by default, verified TLS by default, sans-io protocol crates with
   `#![forbid(unsafe_code)]`, and no dependency in the core or the protocol crates. If you think
   one is unavoidable, open an issue first.
4. **Do not copy code** from tokio-rs/rdbc or any other project. Read them for design and test
   ideas, and cite them.
5. **Every change has a test** once there is code. Tests are hermetic; network components are
   tested against embedded servers or containers, never the public internet.
6. **Zero warnings, green tests** once there is code: `cargo build --workspace --all-targets`
   must not warn and `cargo test --workspace` must pass.

## Good first contributions now

- Fix a wrong or outdated number or citation in [docs/design/research.md](docs/design/research.md)
  or [docs/research/](docs/research/), and keep the evidence tags.
- Resolve an item from research.md section 13 (gaps and unverified items), for example reading
  a primary text the research could not open.
- Review the benchmark method in plan section 6 for coordinated omission, warm-up, defaults
  versus tuned configurations, and fairness to each competitor.
- Review the security rules in plan section 7 against a CVE we missed.

## Pull requests

- One topic per pull request; say what changes and why.
- Update [docs/STATUS.md](docs/STATUS.md) when the state of the project changes.
- `assets/` is generated: change `scripts/brand/build_icons.py` and rerun it.
- Never push to `main` directly; branch first.
- By contributing you agree that your contribution is licensed under the
  [Apache License 2.0](LICENSE).

## Reporting bugs and security issues

Open an issue with what you saw, what you expected, and how to reproduce it. For security
vulnerabilities, follow [SECURITY.md](SECURITY.md) instead of opening a public issue.

Everyone taking part is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## For coding agents and LLM tools

Read [AGENTS.md](AGENTS.md) first. If something in the research or plan is wrong, unsourced or
out of date, please fix it rather than only working around it: open an issue that says what is
wrong and gives the correct statement and its source, then open a pull request that corrects the
file and keeps the evidence tags.
