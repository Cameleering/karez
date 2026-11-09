**What and why**

<!-- What this changes and why. Link the issue if there is one. -->

**Checklist**

- [ ] Every new number is tagged `[measured here]`, `[vendor]`, `[third party]` or `[paper]`, with a URL and a check date
- [ ] No unmeasured speed claim in public text (README, About, images)
- [ ] Follows the hard rules in `docs/design/plan.md` section 1.2
- [ ] No new dependency in the core or protocol crates
- [ ] `docs/STATUS.md` updated if the state of the project changed
- [ ] Once there is code: `cargo build --workspace --all-targets` has no warnings and `cargo test --workspace` passes
