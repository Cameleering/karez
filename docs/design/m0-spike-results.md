# M0 spike: what was built, what was tested

_2026-11-13, updated 2026-11-16. Branch `claude/m0-spike-1bacb7`, pushed with a draft pull request (Cameleering/karez#4), not merged. This note describes the build as of 2026-11-13; later additions (`scan_partitioned`, `insert_batch`, the benchmark harness and its measurements) are in [perf-program.md](perf-program.md) and [../STATUS.md](../STATUS.md)._

**Scope in one line:** a PostgreSQL client with optional TLS (feature `tls`, rustls), **verified against a real server
on PostgreSQL 14.25, 16.15 and 17.12; versions 13, 15 and 18 were not run.** This note has **no benchmarks and no
performance claims**; the first measurements (campaign 2: three lanes, two rounds, plus round 3 and its tail run on 2026-11-18; method of
[plan.md](plan.md) section 6, on separate machines) are in [perf-program.md](perf-program.md).

## What was built

Five crates, zero dependencies by default (not even dev-dependencies; the optional `tls` feature adds rustls and webpki-roots; a sixth, unpublished crate `karez-compat-tests` has tokio as a dev-dependency to test the futures), `#![forbid(unsafe_code)]` in each,
edition 2021, MSRV 1.85 declared and tested (rustc 1.85.1 builds everything and passes the full suite against the server; the main runs used rustc 1.99.1).

| crate | contents |
|---|---|
| `karez-chunk` | Arrow-layout columns (validity bitmap, fixed-width little-endian data or `i32` offsets plus bytes, bit-packed bool); `ChunkBuilder` reused between chunks (`reset` keeps capacity, `abort_row` rolls back a partial row); checked typed readers (`get::<T>` and `get::<Option<T>>`); `RowRef` row view; in-repo SplitMix64 PRNG for tests |
| `karez-proto-pg` | sans-io: `FrameReader` (incremental framing, default message limit 64 MiB), frontend encoders (startup, password, SASL, Parse, Bind, Describe, Close, Execute, Sync, Flush, Terminate, Query, CopyData, CopyDone, CopyFail, CancelRequest), backend parsers (authentication, ParameterStatus, BackendKeyData, ReadyForQuery, Error/NoticeResponse with SQLSTATE fields, row description, DataRow, CommandComplete, NotificationResponse, NegotiateProtocolVersion, the COPY messages), `ResultPlan` decoding binary rows into a chunk (with a renderer that turns `numeric`, `jsonb`, `interval`, `inet`, arrays and others into their exact text form), `Value` parameter encoding, SHA-256, HMAC-SHA-256, PBKDF2, base64 and a SCRAM-SHA-256 client (no channel binding), all in-repo |
| `karez-pg` | `Config` and URL parsing, blocking `Connection` (`TCP_NODELAY`, one write per request), authentication allow-list (default SCRAM only), prepare with an LRU statement cache keyed by SQL and parameter types, `query` / `execute` with server-side binding only, streaming `Cursor` (`next` row view, `next_chunk`), `Transaction` guard with isolation levels and savepoints, `ServerError` with SQLSTATE, `CancelToken` (cancel the running statement from another thread), `LISTEN`/`NOTIFY` (`wait_for_notification`, `take_notifications`), `copy_in` / `copy_out`, `execute_many` (a batch in one round trip), and a blocking `Pool` |
| `karez-rt` | second slice (see below): sans-io `Engine` (command FIFO, statement cache, chunk decoding, Sync groups, `retry_safe`), `SharedConnection` (one mutex, a reader thread only while commands are in flight, blocking and `Future` APIs, streams with back-pressure, one-Sync pipelines, bounded queue, deadlines with cancel, graceful close) |
| `karez` | re-exports; one doc example; `examples/orders.rs` |

Native column types: `bool`, `int2`, `int4`, `int8`, `float4`, `float8`, `text`, `varchar`, `bpchar`,
`name`, `bytea`, `uuid`, `date`, `timestamp`, `timestamptz`, `oid` (as `Int64`), and NULL.

Read in binary and **rendered to a `Utf8` column in the server's text form**: `numeric` (every digit, `NaN`, `Infinity`),
`json`, `jsonb`, `xml`, `"char"`, `interval` (ISO 8601 form, for example `P1Y2M3DT4H5M6.789S`), `time`,
`timetz`, `inet`, `cidr`, `macaddr`, `macaddr8`, `bit`, `varbit`, `void`, enums (found with one `pg_type` lookup per
type and connection), and arrays of those and of the native types, in any dimension, as `{a,b,NULL}` literals.
This is not an Arrow decimal, interval or list type: a reader that wants those parses the string. Floating-point array
elements use Rust's shortest round-trip form, which can differ from the server's text (`1e+20`).

## Tests

Counts from the final run (below). "Needs a server" means the test skips loudly without one.

| crate | tests | needs a server |
|---|---|---|
| `karez-chunk` | 16 unit (randomised round trips with the in-repo PRNG, rollback of partial rows, corrupt-buffer detection) | no |
| `karez-proto-pg` | 37 unit, 4 integration in `tests/split_decoding.rs` | no |
| `karez-pg` | 4 unit (URL parsing, ssl options), 8 in `tests/mock_server.rs` (scripted in-process server), 42 in `tests/pg_integration.rs` | the 42 yes |
| `karez-compat-tests` | 3 tests of the futures on tokio (a dev-dependency of this unpublished crate only) | the 3 yes |
| `karez-rt` | 6 engine tests (scripted server bytes), 12 in `tests/shared_pg.rs` (14 with `tls`) | the 12 yes |
| `karez` | 1 doc test (compiled, not run: `no_run`) | no |
| total | 133 | 57 |
| with `--features tls` | 138 | 62 |

What the test families check:

- **Published vectors:** SHA-256 (NIST: empty, `abc`, the 448-bit message, one million `a`), HMAC-SHA-256 (RFC 4231 cases 1, 2, 3, 6), PBKDF2-HMAC-SHA-256 (iterations 1, 2, 4096), Base64 (RFC 4648), the full SCRAM-SHA-256 exchange of RFC 7677 section 3. The vectors were written from memory and every one passed, so none had to be corrected.
- **Split invariance:** the same server byte stream, cut at every single position, at every pair of positions (in the frame tests), byte by byte, and in 300 random chunkings, gives identical messages and identical decoded rows. A garbage-input test feeds 2,000 random byte strings to the parsers and requires no panic.
- **Limits:** a message over the size limit is rejected as soon as its header arrives; lengths and counts are checked; a NUL in an outgoing string is refused.
- **Real server (all of these on 14.25, 16.15 and 17.12):** every supported type including NULL and extremes (integer limits; `float4`/`float8` NaN, infinities, negative zero, smallest subnormals, largest finite; empty, 1 MiB, multi-byte, emoji, quote/backslash/newline text; dates from 4713 BC to 5874897; timestamps including the minimum, `infinity`, `-infinity` and a daylight-saving change; `bytea` from 0 bytes to 5 MiB; uuids; booleans; all-NULL and never-NULL columns), parameter round trips, a 400,000-row read through `next_chunk` (391 chunks) and through the row view with identical sums, a six-table report of more than 40,000 rows, the view and the sequence, a ten-parameter search with NULL parameters, order-placement transactions (commit, failure with rollback, drop rolls back, savepoints), 5,000 and 3,000 reuses of a cached statement, `ALTER TABLE` between executions, 8 threads each with their own connection and transactions.
- **Errors with SQLSTATE, from a real server:** 23505, 23503, 23502, 23514, 42601, 42P01, 22012, 28P01 (wrong password; the password is not in the error text), 3D000, 40001 (two serializable transactions), 40P01 (a real deadlock; exactly one side fails), 57014 (statement timeout), 57P01 (backend terminated with `pg_terminate_backend` in the middle of a 400,000-row result: the cursor ended with `FATAL: terminating connection due to administrator command (SQLSTATE 57P01)`, no hang). Also 42501 for the read-only cleartext user and 25P02 inside a failed transaction.
- **Rendered types against the server's own text:** for `numeric` (17 hand-picked values including `NaN`, `Infinity`, 1e100, 30-digit and `0.000`, plus 2,000 generated values of varying scale and sign), `interval` (iso_8601 style), `time`, `timetz`, `inet` (including the IPv6 compression rules and IPv4-mapped forms; `abbrev()` is `inet`'s own output form, because `inet::text` always appends the netmask), `cidr`, `macaddr`, `macaddr8`, `bit`, `varbit`, `json`, `jsonb`, `xml`, enums and arrays (`int2` `int4` `int8` `text` with quoting cases, `numeric`, `uuid`, `bytea`, `bool`, `jsonb`, `inet`, `interval`, `time`, `varchar`, multi-dimensional, a lower bound other than 1, NULL elements, a 100,000-element array), the rendered text equals the server's `::text` in every row. `oid` and `"char"` are checked by value.
- **Cancel:** `SELECT pg_sleep(30)` cancelled from another thread after 300 ms fails with SQLSTATE 57014 well inside 20 seconds, and the connection works afterwards.
- **LISTEN / NOTIFY:** a notification sent from another connection is returned by `wait_for_notification` (including an empty payload and a multi-byte one); a wait with nothing sent times out; one that arrives during a query is kept for `take_notifications`.
- **COPY:** 20,000 rows through `COPY ... FROM STDIN (FORMAT csv)` and back out equal the input byte for byte (quoted fields with commas, quotes and newlines); binary COPY out and in; bad data (SQLSTATE 22P02), a duplicate key (23505), a reader that fails mid-stream, a writer that fails mid-stream, and statements of the wrong kind each leave the connection usable.
- **Batches:** 1,001 statements in one `execute_many` give the right row counts; a duplicate key in the middle undoes the whole batch (SQLSTATE 23505, row count unchanged); wrong parameter counts and too many distinct statements are usage errors.
- **Pool:** 8 threads share 2 connections; a connection returned inside a transaction is rolled back; a full pool times out on a bounded wait; a connection whose backend was terminated is discarded and replaced.
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
| image `postgres:14` | id and digest `sha256:980b7cb415aa0f812f65bdec8f6f599be408d8ed9c83ffcaa12650eac397b264`, created 2026-11-12, 648,136,844 bytes; server PostgreSQL 14.25 (Debian 14.25-1.pgdg13+1); pulled 2026-11-13 with the owner's confirmation |
| image `postgres:17` | id and digest `sha256:846a7ddaa67a4b354ae1d21087b683901aa6113a4ed626d3787cfbf6c85e3e07`, created 2026-11-12, 665,162,665 bytes; server PostgreSQL 17.12 (Debian 17.12-1.pgdg13+1); pulled 2026-11-13 with the owner's confirmation |
| container | `karez-test-pg16`, memory limit 1 GiB, port bound to `127.0.0.1:54316`; container and volume removed after each run |
| client | Windows 11, rustc 1.99.1, cargo 1.99.1, Docker Desktop 29.9.2 on WSL2 |

## Second slice: the shared, pipelined connection (karez-rt)

Built 2026-11-13 from [karez-rt.md](karez-rt.md). What the tests check, all on PostgreSQL 14.25, 16.15 and
17.12 unless noted:

- **Engine (no server):** the same server bytes cut one byte at a time, in 997-byte pieces and at random
  positions give identical events and chunk sizes; a stream delivers chunks before completion; an error in a
  one-Sync group fails that statement, reports the later members as skipped and leaves the next command
  alone; after a lost connection a command whose bytes were handed out is not retry-safe and one still queued is;
  wrong parameters queue nothing; a stale plan leaves the cache; 500 random garbage byte strings never panic.
- **Shared connection:** 16 threads x 400 queries over one connection all see the same backend process;
  500 queued commands come back in order; a stream holds the connection while another thread's command waits,
  a slow consumer pauses the reader, and a stream dropped early frees the connection; an error in one
  independent command leaves its neighbours alone; a pipeline is atomic (a duplicate key skips the rest and
  nothing is stored); futures work on a hand-written executor; a deadline cancels the running statement and the
  next command still gets its own answer; a backend killed mid-query fails the running command as not
  retry-safe and later ones as retry-safe; a full queue gives `QueueFull` and close is graceful; notifications
  are kept while commands run; 200 dropped or waited commands in a row never desynchronise the stream.
- **Pool:** five waiters on a one-connection pool are served in arrival order, and a waiter that times out
  leaves the queue without blocking the others.

**Smoke run, not a benchmark** (`crates/karez-rt/examples/pipeline_smoke.rs`, 2026-11-13, `[measured here]`,
one run, 20,000 `SELECT $1::int8`, release build, a Windows 11 client talking to PostgreSQL 16.15 in Docker
Desktop through its port proxy, which is a poor network and the only one used):

| how | first run, thread started per command | second run, reader lingers 50 ms |
|---|---|---|
| blocking `Connection`, one at a time | 2,540 | 2,505 |
| `SharedConnection`, one at a time | 2,196 | 2,529 |
| `SharedConnection`, 16 in flight | 20,634 | 22,915 |
| `SharedConnection`, 128 in flight | 33,671 | 34,149 |
| `SharedConnection`, 512 in flight | 34,554 | 32,548 |
| `SharedConnection`, 8 threads on one connection | 16,683 | 16,585 |

What it shows: queueing many commands on one connection is about 13 times faster than one round trip each,
on this setup. The first run also showed a loss: a lone caller through the shared connection was about 14%
slower than through the blocking `Connection`, because the reader thread was started again for every command.
The reader now stays for `Options::idle_linger` (default 50 ms) after its last command and then leaves, so an
idle connection still has no thread; in the second run the lone caller is level with the blocking connection
(2,529 against 2,505, one run each, so within noise). What it does not show: anything about other clients,
other machines, other networks or larger loads. Each cell is one run. No competitor was run, so no ranking
exists, and none may be quoted from this table. The plan's benchmark method (open-loop p99, Linux machines, baselines) is not started.

## Known limits

- **TLS is an optional feature** (`tls`, rustls with the ring provider; see [tls-decision.md](tls-decision.md)). With it off there are no dependencies and a connection needs an explicit `sslmode=disable`. With it on the default is `verify-full`. Verified on 14.25, 16.15 and 17.12 with a throwaway CA: `verify-full`, `verify-ca` and `require` connect and `pg_stat_ssl` shows the session encrypted (TLS 1.3); a CA the client does not trust is refused; a missing or empty root file is a clear error; a 600 KB parameter, a 100,000-row stream, a transaction, `COPY`, `LISTEN`/`NOTIFY` and small batches work over TLS. **Not supported over TLS:** `execute_many` batches over 64 KiB on the blocking `Connection` (a clear error). The shared `karez-rt` connection works over TLS (8 threads, 500 queued commands, a 600 KB parameter, a 400,000-row stream, an atomic pipeline, graceful close and a killed backend, on 14.25, 16.15 and 17.12). The fixture certificate covers `localhost` and `karez.test` but not the address 127.0.0.1, so a name mismatch is tested too: `verify-full` by `localhost` connects, by 127.0.0.1 it is refused with "not valid for", and `verify-ca` and `require` accept it. **Not tested:** client certificates, `SCRAM-SHA-256-PLUS` (not offered), CRL and OCSP. Cancel requests use a fresh plaintext connection.
- **Three server versions.** Verified on PostgreSQL 14.25, 16.15 and 17.12 (the owner confirmed pulling the 14 and 17 images). 13, 15 and 18 were not run, so the plan's 13 to 18 range is still partly a target.
- **Timestamp range.** Values are stored as microseconds since 1970 in an `i64`, the Arrow layout. PostgreSQL timestamps after about year 294246 cannot be represented and give `Error::Value`; the connection stays usable. This follows from the column format, not from a missing feature. The largest PostgreSQL date does fit. `infinity` and `-infinity` are stored as `i32`/`i64` `MAX`/`MIN`, and a finite value that would collide with them is an error.
- **Types without a decoder.** `money`, geometric types, ranges and multiranges, `tsvector`, `hstore`, composite types, arrays of enums, arrays of dates and timestamps, domains over unsupported types and every other type not listed above fail the query with `Error::UnsupportedType` naming the type, or are read as server text with `unsupported_types=text`. `numeric`, `interval` and arrays arrive as strings, not as Arrow decimal, interval or list columns.
- **SCRAM.** No SASLprep: it needs Unicode NFKC tables, which a dependency-free core does not carry. An ASCII password is unaffected; a non-ASCII password may fail against a server that normalised it. The client nonce comes from `/dev/urandom` where it exists (Unix) and otherwise from std's OS-seeded `RandomState`. Password wiping on drop is best effort, because safe Rust cannot guarantee it.
- **Statement cache retry** only happens outside a transaction. Inside one, the failed Bind aborts the transaction (PostgreSQL's rule) and the error is returned; retrying would need a savepoint around every cached execution.
- **Cursor.** Dropping a cursor early reads and discards the rest of the result. To stop a long result sooner, call `CancelToken::cancel` from another thread, then drop the cursor. A cancel that arrives after the statement finished can hit the next statement on that connection; cancel only what you mean to. A decode failure in the middle of a result is reported after the stream is drained; an unexpected protocol error marks the connection broken.
- **`execute_many` and large batches.** A batch above 64 KiB is now written by a helper thread while the caller reads the results, because a server that cannot write its output stops reading its input, and a client that writes everything first could then block forever. **I could not reproduce the hang on this machine**: the Windows client accepts hundreds of megabytes of unread writes (400 MB out, 200 MB back, finished in under a second), so neither the real-server test (18 MB out, 60 MB back) nor the in-process stalling-server test fails on the old code. The fix is by reasoning; the two tests only show that large overlapped batches complete correctly. Run the stalling-server test on Linux, where it is expected to hang on the old code, before relying on it.
- **COPY** takes the statement as written (COPY has no bound parameters), so build its SQL only from trusted text; the data is never part of the SQL.
- **Pool.** Connections are not tested before they are handed out; a dead idle connection fails on first use and is then replaced.
- **Not built:** the leader optimisation and a `Connection` rebuilt on the engine (see karez-rt.md section 13), a pool of shared connections, portals with partial fetch (the cursor streams from the socket, so memory stays bounded), metadata chunks, MySQL, SQL Server, SQLite, the C ABI and every export. No fuzz targets (the split and garbage tests are ordinary tests; `cargo-fuzz` is a dependency), no Miri (there is no `unsafe`), no `cargo-deny` (there are no dependencies), no size report, no differential test against libpq (the rendered types are compared with the server's own text instead).
- **Measured later (2026-11-14 to 2026-11-16):** speed, memory and binary size; see [perf-program.md](perf-program.md) and the gate table in plan section 1.3. The faster-network test that decides the bulk-read gate is still pending.

## Final run (2026-11-13)

Quoted from the run that produced the counts above; the commands are the ones listed under
"How to reproduce". The same suite also passed with `cargo +1.85 test --workspace` (rustc 1.85.1), also with `--features karez-pg/tls,karez-rt/tls` (138 tests, TLS on 14.25, 16.15 and 17.12), and the 39 real-server tests pass on PostgreSQL 14.25 (port 54314) and 17.12 (port 54317), started with `docker compose -f tests/docker/compose.yaml --profile pg14 up -d --wait pg14` (and `pg17`).

```text
cargo fmt --all --check            -> clean
cargo build --workspace --all-targets -> 0 warnings
cargo clippy --workspace --all-targets -- -D warnings -> no issues
KAREZ_TEST_PG=127.0.0.1:54316 KAREZ_REQUIRE_SERVER=1 cargo test --workspace
  karez_chunk                16 passed
  karez_proto_pg             37 passed
  split_decoding              4 passed
  karez_pg                    4 passed
  mock_server                 8 passed
  pg_integration             42 passed
  karez_rt engine             6 passed
  karez_rt shared_pg         12 passed
  karez doc test              1 passed
  (0 failed, 0 ignored)
example orders: audit_log: 400000 rows in 391 chunks; rolled back; orders still 150000
```
