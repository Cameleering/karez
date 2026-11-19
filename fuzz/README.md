# Coverage-guided fuzzing

Seven libFuzzer targets for the parts of karez that read bytes from a server or a user. They sit in
their own workspace (`fuzz/Cargo.toml`) because they need nightly Rust and `cargo-fuzz`; the main
workspace excludes them.

| target | input | what must hold |
|---|---|---|
| `frame` | a piece-size byte, then a byte stream | `FrameReader` fed in pieces never panics, loops or buffers past its limit; every frame it returns also parses |
| `message` | a tag byte, then a message body | `BackendMessage::parse` and `DataRow` never panic; field count matches the header |
| `row` | column types and type modifiers, then a `DataRow` body | decoding never panics; the chunk stays valid (`validate`); every value reads back as its own type |
| `scram` | a split byte, then two messages | the SCRAM client accepts or refuses server messages without panic; base64 round-trips |
| `oauth` | bytes | the OAUTHBEARER JSON error parser never panics; a token that is not valid never reaches a message, and a valid one adds no separator |
| `cert` | bytes | the signature-algorithm reader for SCRAM channel binding never panics |
| `config_url` | text | the URL parser never panics; a password in the URL never shows in `Debug` |

## Run

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
bash fuzz/run.sh 300        # every target for 300 s; a crash goes to fuzz/artifacts/<target>/
cargo +nightly fuzz run row # one target until stopped (from the fuzz/ directory)
```

`fuzz/corpus/<target>` holds small valid seeds, so the fuzzer starts inside the parsers. They are
made from the same generators as the std-only fuzzer in
`crates/karez-proto-pg/tests/fuzz_backend.rs`:

```sh
cargo test -p karez-proto-pg --test fuzz_backend dump_seed_corpus -- --ignored
```

`run.sh` keeps what the fuzzer finds in a working corpus outside the repository
(`$FUZZ_WORK`, default `~/karez-fuzz-work`), so the committed seeds stay small. The `cert` seeds
are the DER of the fixture CA certificates in `tests/docker/certs/`, which exist after the compose
fixtures have been started once.

## Results

The runs and their limits are recorded in
[docs/design/m1-plan.md](../docs/design/m1-plan.md), step 8. A crash found by a target becomes a
regression test in the crate that owns the code, with the input as a byte string.
