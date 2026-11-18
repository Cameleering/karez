# M1 work plan: core API and the PostgreSQL driver

_Written 2026-11-18, the day the owner moved the project from Milestone 0 to Milestone 1. This refines
[plan.md](plan.md) section 8 (row M1) against what the code already does. Where it differs from the
plan it says so._

## 1. Where M0 left the code

Read from `crates/` on 2026-11-18 (about 14,500 lines in five crates plus a compatibility-test crate).

| M1 item in plan section 8 | state |
|---|---|
| types | partial: native codecs for bool, ints, floats, text, bytea, uuid, date, timestamp(tz); `numeric`, `json(b)`, `interval`, `time(tz)`, `inet`, `cidr`, `macaddr`, bit, enums and arrays are decoded to **text** (`render.rs`); no native `Decimal128`; parameters accept only the `Value` variants listed in `types.rs` |
| transactions, savepoints | built (`tx.rs`: `Isolation`, `Savepoint`, `Transaction`) |
| COPY | built, in and out |
| LISTEN | built |
| cancel | built (`CancelToken`) |
| portals | **not built**: the cursor streams from the socket (memory stays bounded) but there is no `Execute` with a row limit, so a second statement cannot run on the connection while a cursor is open |
| metadata chunks | **not built** |
| TLS with rustls | built (feature `tls`) |
| GSS/SSPI | **not built** (needs FFI, so `unsafe`) |
| OAUTHBEARER | **not built** |
| SCRAM-SHA-256-PLUS | **not built** (SCRAM without channel binding works) |
| async adapter | built as `karez-rt` (std-only I/O threads and channels); a tokio adapter is not |
| `karez-pool` | a `Pool` exists inside `karez-pg`; a driver-neutral crate does not (there is one driver, so there is nothing to be neutral over) |
| URL translation | built for PostgreSQL (`karez:pg://`, `postgresql://`, `postgres://`); **one host only**, no `target_session_attrs` |
| errors with SQLSTATE | built; the class and a transient flag were added on 2026-11-18 (step 1) |
| fuzz harness | a std-only mutation fuzzer was added on 2026-11-18 (step 1); coverage-guided fuzzing is not set up |
| differential harness | **not built** |
| parity checklist | not started (needs the two primary texts, STATUS next steps) |

## 2. Steps, in order

Each step ends in tests that pass and a number or a list that is checked in. Steps 1 to 5 need no spend and no
network beyond the local PostgreSQL containers in `tests/docker/`.

1. **Error classes and the std-only fuzz floor. Done 2026-11-18.**
   `ServerError::class`, `ServerError::is_transient`, `Error::is_transient`, `Error::sqlstate_class` (unit tests for 15 transient and 16 permanent codes and every non-server variant).
   `crates/karez-proto-pg/tests/fuzz_backend.rs`: a corpus of valid server streams (every message type, 30 known type OIDs and one unknown) mutated with the in-repo PRNG and fed through framing,
   message parsing, row decoding into chunks, SCRAM and base64, in random splits. 400,000 mutated streams per target in release mode ran without a panic, a hang or an over-limit buffer.
   Limits: it is not coverage-guided; it was not checked for what share of the decoders it reaches; it covers the server-to-client direction only.
2. **Multi-host URLs and `target_session_attrs`** (`any`, `read-write`, `read-only`, `primary`, `standby`, `prefer-standby`), with the connect loop trying hosts in order or at random
   (`load_balance_hosts`), a per-host timeout and the error of the last host kept. Exit: tests against two containers (one made a standby), including a dead first host.
3. **Native types.** `numeric` to a `Decimal128` column (precision and scale kept) with a text fallback beyond 38 digits; `interval`, `time`, `timetz`, `inet`/`cidr`/`macaddr`, `bit`, `json(b)` as
   dedicated chunk types or as `Utf8` with a marker, one decision per type recorded here before code; array and enum parameters. Exit: step 4 passes for each.
4. **Differential harness.** For every supported type: the values NULL, zero, one, minimum, maximum, a random sample and the known edge cases (numeric `NaN`, infinity dates, time zones across DST),
   written with `COPY` or `INSERT`, read back in binary, rendered, and compared with the server's own `::text` output on the same row. A property test over the type x value x format matrix,
   run against PostgreSQL 14, 16 and 17 (13, 15 and 18 once the containers exist). This is the cross-check plan section 9 asks for ("cross-checked against libpq's text output").
5. **Portals and metadata chunks.** `Execute` with a row limit and `PortalSuspended`, so a cursor can be open while the connection serves another statement in the same transaction;
   `conn.metadata().objects(...)` returning ADBC's fixed schemas from `pg_catalog` (tables, columns, constraints), as chunks. Exit: tests for interleaved cursors, and the metadata schemas
   compared field by field with the ADBC specification.
6. **Pipeline and batch builders** in the blocking API as in plan 2.5 (`conn.pipeline()...run()`, `tx.batch(&stmt).rows(...)`), on top of the pipelining that `karez-rt` already does.
   Exit: S2 pipelined and unpipelined reported separately, as the plan's method requires.
7. **Authentication.** In this order: OAUTHBEARER (a token callback; PostgreSQL 18; no `unsafe`), SCRAM-SHA-256-PLUS (channel binding from the rustls exporter), then GSS/SSPI in its own crate
   with `unsafe` and a written safety argument per block (plan section 7.2; there is no paid audit). Exit: a Kerberos container and an OAuth validator fixture; if they cannot be built here, the step
   stops at OAUTHBEARER and SCRAM-PLUS and records GSS/SSPI as open.
8. **Coverage-guided fuzzing outside the std-only crates.** A `fuzz/` directory with cargo-fuzz targets for frame, message, row and SCRAM parsing, excluded from the workspace so the core keeps no
   dependency; seed corpus from step 1's corpus; a CI job. The plan's target is 30 CPU-days without a new crash (M8); M1 only needs the harness and a first run.
9. **Exit measurements (needs the owner's spend approval, per CLAUDE.md).** S6 COPY against PgBulkInsert (the plan wants at least 1.5x; plan section 10 row 4), S1 to S8 with TLS on,
   and the S8 wait-time distribution against the Erlang C prediction. The TLS bulk-read adapter gap from perf-program.md section 20 closes here.

## 3. Two decisions that change the plan

- **`karez-pool` and the driver-neutral API (plan 2.5: `Driver`, `Url`, `drivers().get(&url)`).** With one driver, a neutral trait would be guesswork, and it would be wrong in ways that only a second
  driver shows (placeholders, type systems, auth). Recommendation: keep `Pool` in `karez-pg` through M1, and extract `karez-pool` and the `Driver` traits at the start of M2, when MySQL gives a second
  implementation to shape them. The M1 row then reads "pool inside the driver crate". This needs the owner's agreement because it moves a plan item.
- **GSS/SSPI.** It is the only M1 item that needs `unsafe` and it cannot be tested without a Kerberos or Windows domain fixture. It is last in step 7 so the rest of M1 does not wait for it.

## 4. Not in M1 (unchanged from the plan)

Other databases (M2 to M4), the exports (M5), the JDBC and ODBC facades (M6, M7), the parity checklist freeze (needs JDBC 4.3 chapter 6 and ISO 9075-3:2023, STATUS next steps), and any claim
about speed: the M1 speed targets are read off step 9 only.
