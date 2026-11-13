# M0 spike: what was built, what was tested

_2026-11-13. Branch `claude/m0-spike-1bacb7`, local, not pushed, not merged._

**Scope in one line:** a blocking PostgreSQL client with no TLS, **verified against a real server
on PostgreSQL 16.15 only; versions 14 and 17 were not run.** There are **no benchmarks and no
performance claims** in this note. Speed is measured later with the method in
[plan.md](plan.md) section 6, on separate machines.

## What was built

Four crates, zero dependencies (not even dev-dependencies), `#![forbid(unsafe_code)]` in each,
edition 2021, MSRV 1.85 declared (built here with rustc 1.99.1; the MSRV was not tested).

| crate | contents |
|---|---|
| `karez-chunk` | Arrow-layout columns (validity bitmap, fixed-width little-endian data or `i32` offsets plus bytes, bit-packed bool); `ChunkBuilder` reused between chunks (`reset` keeps capacity, `abort_row` rolls back a partial row); checked typed readers (`get::<T>` and `get::<Option<T>>`); `RowRef` row view; in-repo SplitMix64 PRNG for tests |
| `karez-proto-pg` | sans-io: `FrameReader` (incremental framing, default message limit 64 MiB), frontend encoders (startup, password, SASL, Parse, Bind, Describe, Close, Execute, Sync, Flush, Terminate, Query), backend parsers (authentication, ParameterStatus, BackendKeyData, ReadyForQuery, Error/NoticeResponse with SQLSTATE fields, row description, DataRow, CommandComplete, NotificationResponse, NegotiateProtocolVersion), `ResultPlan` decoding binary rows into a chunk, `Value` parameter encoding, SHA-256, HMAC-SHA-256, PBKDF2, base64 and a SCRAM-SHA-256 client (no channel binding), all in-repo |
| `karez-pg` | `Config` and URL parsing, blocking `Connection` (`TCP_NODELAY`, one write per request), authentication allow-list (default SCRAM only), prepare with an LRU statement cache keyed by SQL and parameter types, `query` / `execute` with server-side binding only, streaming `Cursor` (`next` row view, `next_chunk`), `Transaction` guard with isolation levels and savepoints, `ServerError` with SQLSTATE |
| `karez` | re-exports; one doc example; `examples/orders.rs` |

Decoded types: `bool`, `int2`, `int4`, `int8`, `float4`, `float8`, `text`, `varchar`, `bpchar`,
`name`, `bytea`, `uuid`, `date`, `timestamp`, `timestamptz`, and NULL.

## Tests

Counts from the final run (below). "Needs a server" means the test skips loudly without one.

| crate | tests | needs a server |
|---|---|---|
| `karez-chunk` | 16 unit (randomised round trips with the in-repo PRNG, rollback of partial rows, corrupt-buffer detection) | no |
| `karez-proto-pg` | 33 unit, 4 integration in `tests/split_decoding.rs` | no |
| `karez-pg` | 4 unit (URL parsing), 7 in `tests/mock_server.rs` (scripted in-process server), 33 in `tests/pg_integration.rs` | the 33 yes |
| `karez` | 1 doc test (compiled, not run: `no_run`) | no |
| total | 98 | 33 |

What the test families check:

- **Published vectors:** SHA-256 (NIST: empty, `abc`, the 448-bit message, one million `a`), HMAC-SHA-256 (RFC 4231 cases 1, 2, 3, 6), PBKDF2-HMAC-SHA-256 (iterations 1, 2, 4096), Base64 (RFC 4648), the full SCRAM-SHA-256 exchange of RFC 7677 section 3. The vectors were written from memory and every one passed, so none had to be corrected.
- **Split invariance:** the same server byte stream, cut at every single position, at every pair of positions (in the frame tests), byte by byte, and in 300 random chunkings, gives identical messages and identical decoded rows. A garbage-input test feeds 2,000 random byte strings to the parsers and requires no panic.
- **Limits:** a message over the size limit is rejected as soon as its header arrives; lengths and counts are checked; a NUL in an outgoing string is refused.
- **Real server, PostgreSQL 16.15:** every supported type including NULL and extremes (integer limits; `float4`/`float8` NaN, infinities, negative zero, smallest subnormals, largest finite; empty, 1 MiB, multi-byte, emoji, quote/backslash/newline text; dates from 4713 BC to 5874897; timestamps including the minimum, `infinity`, `-infinity` and a daylight-saving change; `bytea` from 0 bytes to 5 MiB; uuids; booleans; all-NULL and never-NULL columns), parameter round trips, a 400,000-row read through `next_chunk` (391 chunks) and through the row view with identical sums, a six-table report of more than 40,000 rows, the view and the sequence, a ten-parameter search with NULL parameters, order-placement transactions (commit, failure with rollback, drop rolls back, savepoints), 5,000 and 3,000 reuses of a cached statement, `ALTER TABLE` between executions, 8 threads each with their own connection and transactions.
- **Errors with SQLSTATE, from a real server:** 23505, 23503, 23502, 23514, 42601, 42P01, 22012, 28P01 (wrong password; the password is not in the error text), 3D000, 40001 (two serializable transactions), 40P01 (a real deadlock; exactly one side fails), 57014 (statement timeout), 57P01 (backend terminated with `pg_terminate_backend` in the middle of a 400,000-row result: the cursor ended with `FATAL: terminating connection due to administrator command (SQLSTATE 57P01)`, no hang). Also 42501 for the read-only cleartext user and 25P02 inside a failed transaction.
- **Authentication:** SCRAM-SHA-256, cleartext and trust, each for its own user in `pg_hba.conf`; cleartext and trust are refused unless the URL allows them (`auth=password`, `auth=trust`).
- **Stale plans:** after `ALTER TABLE ... ADD COLUMN` and `ALTER COLUMN ... TYPE`, the cached statement is re-prepared once (outside a transaction) and returns the new columns and types. An explicit `Statement` handle is not silently replaced: it fails with the server's SQLSTATE 0A000 "cached plan must not change result type", and a fresh `prepare` works.

## How to reproduce

Without a server (the 33 integration tests print `SKIPPED (not verified against a real server)`
and pass without checking anything; this is **not** a verification):

```sh
cargo build --workspace --all-targets
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

With the throwaway database (needs Docker; the compose file uses `postgres:16`, which Docker
pulls if it is not already on the machine. It was already present for this work):

```sh
docker compose -f tests/docker/compose.yaml up -d --wait          # about 25 s
KAREZ_TEST_PG=127.0.0.1:54316 KAREZ_REQUIRE_SERVER=1 cargo test --workspace
KAREZ_URL='postgresql://karez_scram:karez_test_only_scram@127.0.0.1:54316/karez?sslmode=disable' \
  cargo run -p karez --example orders
docker compose -f tests/docker/compose.yaml down -v
```

`KAREZ_REQUIRE_SERVER=1` turns a missing or unreachable server into a failure. The fixture, its
roles, schema and seed are described in [tests/docker/README.md](../../tests/docker/README.md).
The passwords in it are public test values.

## The server that was used

| item | value |
|---|---|
| image | `postgres:16`, already on the machine, not pulled for this work |
| image id | `sha256:f9a7b9ebf6f56201f949db745e47bc423c0f0f8294bafe991cc10bf74b029657` |
| `RepoDigests` | `postgres@sha256:f9a7b9ebf6f56201f949db745e47bc423c0f0f8294bafe991cc10bf74b029657` |
| image created / size | 2026-11-05 / 636,323,268 bytes |
| server | PostgreSQL 16.15 (Debian 16.15-1.pgdg13+2) on x86_64-pc-linux-gnu |
| container | `karez-test-pg16`, memory limit 1 GiB, port bound to `127.0.0.1:54316`; container and volume removed after each run |
| client | Windows 11, rustc 1.99.1, cargo 1.99.1, Docker Desktop 29.9.2 on WSL2 |

## Known limits

- **No TLS.** Connecting requires `sslmode=disable`; every other `sslmode` is rejected, so a connection is never silently downgraded. This departs from the plan's "verified TLS by default" until TLS exists.
- **One server version.** PostgreSQL 14 and 17 were not run (the images were not on the machine and pulling them was not confirmed by the owner). The plan's 13 to 18 range is a target.
- **Timestamp range.** Values are stored as microseconds since 1970. PostgreSQL's last roughly 30 years of timestamp range (after about year 294246) cannot be represented and give `Error::Value`; the connection stays usable. The largest PostgreSQL date does fit. `infinity` and `-infinity` are stored as `i32`/`i64` `MAX`/`MIN`, and a finite value that would collide with them is an error.
- **Unsupported types.** `numeric`, `json`, `jsonb`, arrays, `interval`, enums, `inet` and every other type without a decoder fail the query with `Error::UnsupportedType` naming the type, or are read as server text with `unsupported_types=text`. This is the only text path, and it is opt-in.
- **SCRAM.** No SASLprep: a non-ASCII password may fail against a server that normalised it. No channel binding (`SCRAM-SHA-256-PLUS` is not offered). The client nonce comes from std's OS-seeded `RandomState`, not a dedicated CSPRNG. Password wiping on drop is best effort, because safe Rust cannot guarantee it.
- **Statement cache retry** only happens outside a transaction. Inside one, the failed Bind aborts the transaction and the error is returned.
- **Cursor.** Dropping a cursor early reads and discards the rest of the result (it does not send a cancel). A decode failure in the middle of a result is reported after the stream is drained; an unexpected protocol error marks the connection broken.
- **Not built:** async, pipelining, pool, COPY, LISTEN/NOTIFY, cancel, portals with partial fetch, metadata chunks, MySQL, SQL Server, SQLite, the C ABI and every export. No fuzz targets (the split and garbage tests are ordinary tests), no Miri, no `cargo-deny`, no size report, no differential test against libpq.
- **Not measured:** speed, memory and binary size. The plan's Milestone 0 gate (plan section 1.3) is therefore still open.

## Final run (2026-11-13)

Quoted from the run that produced the counts above; the commands are the ones listed under
"How to reproduce".

```text
cargo fmt --all --check            -> clean
cargo build --workspace --all-targets -> 0 warnings
cargo clippy --workspace --all-targets -- -D warnings -> no issues
KAREZ_TEST_PG=127.0.0.1:54316 KAREZ_REQUIRE_SERVER=1 cargo test --workspace
  karez_chunk                16 passed
  karez_proto_pg             33 passed
  split_decoding              4 passed
  karez_pg                    4 passed
  mock_server                 7 passed
  pg_integration             33 passed
  karez doc test              1 passed
  (0 failed, 0 ignored)
example orders: connected, server_version = 16.15 (Debian 16.15-1.pgdg13+2);
  audit_log: 400000 rows in 391 chunks; rolled back; orders still 150000
```
