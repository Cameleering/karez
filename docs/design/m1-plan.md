# M1 work plan: core API and the PostgreSQL driver

_Written 2026-11-18, the day the owner moved the project from Milestone 0 to Milestone 1. This refines
[plan.md](plan.md) section 8 (row M1) against what the code already does. Where it differs from the
plan it says so._

## 1. Where M0 left the code

Read from `crates/` on 2026-11-18 (about 14,500 lines in five crates plus a compatibility-test crate; `karez-pool` was added the same day, step 1b).

| M1 item in plan section 8 | state |
|---|---|
| types | partial (the state when M1 began; step 3 changed it on 2026-11-19, see below): native codecs for bool, ints, floats, text, bytea, uuid, date, timestamp(tz); `numeric`, `json(b)`, `interval`, `time(tz)`, `inet`, `cidr`, `macaddr`, bit, enums and arrays were decoded to **text** (`render.rs`); no native `Decimal128`; parameters accepted only the `Value` variants then listed in `types.rs` |
| transactions, savepoints | built (`tx.rs`: `Isolation`, `Savepoint`, `Transaction`) |
| COPY | built, in and out |
| LISTEN | built |
| cancel | built (`CancelToken`) |
| portals | was **not built** when M1 began (the cursor streams from the socket, so a second statement cannot run while one is open); built 2026-11-19 as step 5 |
| metadata chunks | was **not built** when M1 began; built 2026-11-19 as step 5 |
| TLS with rustls | built (feature `tls`) |
| GSS/SSPI | GSSAPI (Kerberos) on Unix **built 2026-11-19 (step 7)** in `karez-gss`; Windows SSPI **open** |
| OAUTHBEARER | **built 2026-11-19 (step 7)** |
| SCRAM-SHA-256-PLUS | **built 2026-11-19 (step 7)** (`tls-server-end-point`, the only binding PostgreSQL has) |
| async adapter | built as `karez-rt` (std-only I/O threads and channels); a tokio adapter is not |
| `karez-pool` | **built 2026-11-18 (step 1b)**: a std-only crate with a `Manager` trait and a first-come-first-served `Pool`; `karez-pg::Pool` wraps it. The plan's other `karez-pool` items (statement cache, multi-host routing, health checks) are not in it: the statement cache is per connection in `karez-pg`, routing is step 2 |
| URL translation | built for PostgreSQL (`karez:pg://`, `postgresql://`, `postgres://`); several hosts, `target_session_attrs` and `load_balance_hosts` since 2026-11-18 (step 2) |
| errors with SQLSTATE | built; the class and a transient flag were added on 2026-11-18 (step 1) |
| fuzz harness | a std-only mutation fuzzer was added on 2026-11-18 (step 1); coverage-guided targets in `fuzz/` since 2026-11-19 (step 8) |
| differential harness | **built 2026-11-19 (step 4)** |
| parity checklist | not started (needs the two primary texts, STATUS next steps) |

## 2. Steps, in order

Each step ends in tests that pass and a number or a list that is checked in. Steps 1 to 5 need no spend and no
network beyond the local PostgreSQL containers in `tests/docker/`.

1b. **`karez-pool` extracted. Done 2026-11-18** (owner: "extract now"). `crates/karez-pool`: `Manager` (`connect`, `recycle`), `Pool<M>`, `Pooled<M>`, `PoolError`; recycle runs outside the lock and a panic in it still frees the slot; 8 unit tests (lazy open, reuse, maximum, timeout, failed connect, discard, panic in recycle, FIFO order, 12 threads against a limit of 3). `karez-pg`'s `Pool` and `PooledConnection` keep their API; the 47 real-server tests pass on Linux with Rust 1.85.0 against PostgreSQL 16.
1. **Error classes and the std-only fuzz floor. Done 2026-11-18.**
   `ServerError::class`, `ServerError::is_transient`, `Error::is_transient`, `Error::sqlstate_class` (unit tests for 15 transient and 16 permanent codes and every non-server variant).
   `crates/karez-proto-pg/tests/fuzz_backend.rs`: a corpus of valid server streams (every message type, 30 known type OIDs and one unknown) mutated with the in-repo PRNG and fed through framing,
   message parsing, row decoding into chunks, SCRAM and base64, in random splits. 400,000 mutated streams per target in release mode ran without a panic, a hang or an over-limit buffer.
   Limits: it is not coverage-guided; it was not checked for what share of the decoders it reaches; it covers the server-to-client direction only.
2. **Multi-host URLs and `target_session_attrs`. Done 2026-11-18.** `host1[:port],host2[:port]` in the URL (or `Config::add_host`), `target_session_attrs` (`any`, `read-write`, `read-only`, `primary`, `standby`,
   `prefer-standby`) and `load_balance_hosts` (`disable`, `random`). Hosts are tried in order; one that is unreachable, refuses the login or does not satisfy the attributes is skipped; if none is usable the error is
   `Error::NoSuitableHost` with one line per host and a `transient` flag (a single host with `any` still returns its own error unchanged). The check uses `in_hot_standby` and `default_transaction_read_only`, which
   PostgreSQL 14 and later report at startup, so it costs no query; older servers are asked once (`pg_is_in_recovery()`, `SHOW transaction_read_only`). `prefer-standby` tries every host for a standby, then every host
   for any. The timeout is per host (`connect_timeout`). 13 tests against scripted servers (dead and refusing hosts, read-only, standby, legacy servers, ordering, random order, the error text; a mutation of the
   read-write rule makes 5 of them fail), 1 against the real PostgreSQL 16 container, and 4 against **a real primary and a real streaming standby** (`tests/docker/standby.yaml`, PostgreSQL 16, built with
   `pg_basebackup -R`; `crates/karez-pg/tests/standby.rs`, run 2026-11-19 in WSL on Rust 1.85.0): every attribute picks the right server in either host order, the standby refuses a write, a missing kind of server is a transient
   `NoSuitableHost`, `prefer-standby` falls back, and 200 connections with `load_balance_hosts=random` split between the two within 60 to 140 (the scripted-server test uses the same bounds). Also fixed on 2026-11-19: the TLS
   suite was rerun after step 2 (karez-pg 51 tests with `tls`, karez-rt 14 and 6; all pass), the dead-host tests no longer wait two seconds each on Windows (a host that closes at once replaces a refused port, except in the one test about refusal),
   and `execute_many` over 64 KiB now works on a blocking TLS connection: it goes out in windows of about 64 KiB, each followed by a Flush and a read of its results, with one Sync at the end (test: 60 requests of 300 KB answered with 1 MB each, and a failing
   statement in the middle). Still not tested: a failover while a connection is open (only the choice at connect time is).
3. **Native types. Built 2026-11-19; the exit (step 4 for every type) is open.** One decision per type, with the reason:
   - **`numeric(p,s)` with `p <= 38` and `0 <= s <= p` -> `Decimal128 { precision, scale }`** (16 bytes, little-endian `i128`, Arrow's `Decimal128` layout). The scale comes from the type modifier in
     RowDescription, so every value in the column has it. **An unconstrained `numeric`, a precision above 38, and the PostgreSQL 15 forms with a negative scale or a scale above the precision stay `Utf8`**, rendered exactly
     (a column whose scale varies by value has no fixed Arrow type, and the text keeps every digit). **`NaN`, `Infinity` and `-Infinity` in a `Decimal128` column are a value error** (`Error::Value`, with the advice to select the column
     `::text`); the connection stays usable and the rest of the result is read and discarded (as for any value this version cannot hold). A value with more fractional digits than the column scale cannot occur for a constrained column and is an error if a server sends one.
   - **`interval` -> `Interval { months: i32, days: i32, micros: i64 }`** (16 bytes: PostgreSQL's own three fields). Not Arrow's `MonthDayNano`: PostgreSQL's microseconds times 1,000 overflow an `i64` beyond about 292 years, so the ADBC export (step 5)
     converts with a checked multiply and a clear error. `Display` is the ISO 8601 form (what `intervalstyle = 'iso_8601'` prints); one implementation serves the column type and the text rendering inside arrays.
   - **`time` -> `Time(i64)`** microseconds since midnight, 0 to 86,400,000,000 (PostgreSQL allows `24:00:00`); `Display` is PostgreSQL's text.
   - **`timetz` stays `Utf8`.** Arrow has no type for a time with an offset, PostgreSQL's own documentation discourages it, and its text form round-trips.
   - **`inet`, `cidr`, `macaddr`, `macaddr8`, `bit`, `varbit`, `json`, `jsonb`, `xml`, `"char"`, enums and arrays stay `Utf8`**, read in binary and rendered to the server's `::text` form. No standard Arrow type exists for the network and
     bit types, and a fixed-size binary would force every consumer to re-implement PostgreSQL's formatting. **No marker in the chunk:** `ResultPlan::type_oids()` already tells which PostgreSQL type a `Utf8` column came from, and the ADBC export (step 5) reads it from there;
     a field-level tag would change every `Field::new` call for no use before then. JSON has no `Value` variant: send it as `Value::Unknown` text.
   - **Parameters (binary format):** `Value::Numeric(Decimal128)`, `Value::Interval`, `Value::Time`, and `Value::Array(Array::{Bool, I16, I32, I64, F32, F64, Text})` (one dimension, NULL elements, the empty array). Anything else, including **enum
     labels and arrays of other types, goes as `Value::Unknown` text** (format 0, type inferred by the server); that is tested for an enum and an enum array and needs no new variant.
   Tests: `numeric.rs` (type modifiers, known values, refusals, encode against hand-built server bytes, 20,000 random round trips), chunk columns, decoder and encoder unit tests, the mutation fuzzer with a typed numeric column (400,000 streams, no panic),
   and 3 real-server tests on PostgreSQL 14, 16 and 17: columns against the server's own `::text` (11 numeric, interval, time and `(0)` precision cases, a 5,000-row random numeric, the extremes), parameters sent and printed back, and NaN. Existing
   rendered-type tests now read interval and time natively. `karez-rt`'s `OwnedValue` has the same native variants (uuid, date, timestamp, timestamptz, time, interval, numeric, arrays of the seven basic types, and an unknown-typed text literal; `every_owned_parameter_type_reaches_the_server_intact`), added 2026-11-19. The type x value x format matrix is step 4.
4. **Differential harness.** For every supported type: the values NULL, zero, one, minimum, maximum, a random sample and the known edge cases (numeric `NaN`, infinity dates, time zones across DST),
   written with `COPY` or `INSERT`, read back in binary, rendered, and compared with the server's own `::text` output on the same row. A property test over the type x value x format matrix,
   run against PostgreSQL 13 to 18. This is the cross-check plan section 9 asks for ("cross-checked against libpq's text output").
   **Built 2026-11-19** as `crates/karez-pg/tests/differential.rs`, for the 15 native types (bool, int2, int4, int8, float4, float8, text, bytea, uuid, date, timestamp, timestamptz, numeric in five precisions and scales, interval, time).
   The server is the judge, so no formatter of ours stands between the value and the check: (1) edge values written as SQL literals (NULL, zero, one, minimum, maximum, `NaN`, the infinities, `-0`, subnormals, the calendar's ends,
   the repeated DST hour, `24:00:00`, 100 KB text and bytea) are read in binary and sent back as binary parameters, and the server must print the same text and find the value `IS NOT DISTINCT FROM` the original; (2) the same value as a text-format
   parameter (`Value::Unknown` with the server's text) must give the same answer; (3) 300 random values per type (`KAREZ_DIFF_ROWS`, `KAREZ_DIFF_SEED`) go in as binary parameters and must come back identical (floats by bit pattern, any NaN equal to any NaN),
   and for numeric, interval and time our `Display` must equal the server's text; (4) 200 random one-dimensional arrays of bool, int2, int4, int8, float4, float8 and text with NULLs, found equal by the server and, for the exactly rendered types, equal to
   its `::text`; (5) a statement prepared against the table's own types accepts the native `Value` for each type without a cast. Run 2026-11-19 on PostgreSQL 14, 16 and 17 (5 tests, 0.4 s) and at 5,000 rows per type with three seeds on 16.
   Three mutations of the decoders and encoders (days and months swapped in `interval`, the sign dropped when encoding `numeric`, an off-by-one microsecond in `time`) each make 2 or 3 of the 5 tests fail. The types that are read as rendered text have a random generator too
   (`random_values_of_the_rendered_types_match_the_servers_text`, added 2026-11-19): 12 scalar columns (`inet` against `abbrev()`, `cidr`, `macaddr`, `macaddr8`, `bit(13)`, `varbit`, `json`, `jsonb`, `xml`, `timetz`, an enum, an unconstrained `numeric`) and 20 array element types (numeric, interval, time, timetz, uuid, text and varchar with
   quoting traps, bytea, inet, cidr, macaddr, bit, varbit, jsonb, json, xml, int2, int4, int8, bool), with NULL elements, two dimensions and lower bounds other than 1. The text is parsed by the server, read back in binary, rendered by us and compared with the server's text of the stored value; 300 rows per case (`KAREZ_DIFF_ROWS`),
   also run at 3,000 rows with three seeds on PostgreSQL 16. Two mutations of the renderer (IPv4 octets swapped; array elements containing a tab left unquoted) are caught. The fixed cases stay in `pg_integration.rs` (`rendered_types_match_the_servers_own_text`, about 30 cases including a 100,000-element array).
   **Server versions:** the whole suite, this harness included, passes on PostgreSQL 13.23, 14.25, 15.20, 16.16, 17.11 and 18.7, plain and with TLS (2026-11-19). The version-dependent parts are guarded: numeric infinities exist from 14, the startup parameters `in_hot_standby` and `default_transaction_read_only` from 14 (on 13 the same
   `target_session_attrs` checks run through the legacy queries against the real server), and the partitioned scan needs 14 (on 13 the tests check the refusal).
   **Found by the harness:** PostgreSQL's largest timestamps (about year 294247 and later) do not fit `Timestamp`, which counts microseconds from 1970 in an `i64` (as Arrow's `timestamp[us]` does). They are a value error that leaves the connection usable (the error
   names the type; `::text` still works); the test `timestamps_beyond_the_unix_epoch_range_are_a_value_error` pins that. Dates cover PostgreSQL's whole range.
5. **Portals and metadata chunks.** `Execute` with a row limit and `PortalSuspended`, so a cursor can be open while the connection serves another statement in the same transaction;
   `conn.metadata().objects(...)` returning ADBC's fixed schemas from `pg_catalog` (tables, columns, constraints), as chunks. Exit: tests for interleaved cursors, and the metadata schemas
   compared field by field with the ADBC specification.
   **Built 2026-11-19. Decisions:**
   - **Portals** (`conn/portal.rs`): `Connection::portal(sql, params)` binds a named portal and returns a `Portal`; `Connection::fetch(&mut portal, max_rows)` sends `Execute` with the limit and `Sync`, reads exactly those rows, and reads on to
     `ReadyForQuery`, so the connection is idle between fetches (`0` reads all the rest). It returns `Fetched { chunks, done }`: whole chunks of at most 1,024 rows, copied out of the connection's builder. `Transaction` has the same three methods.
     A portal needs an open transaction block, because PostgreSQL destroys portals when the transaction ends (outside one `portal` returns `Error::Usage`; after COMMIT or ROLLBACK a fetch is a server error, SQLSTATE 34000, and the connection stays
     usable); `Drop` sends nothing, `close_portal` frees one early. An error in a later row fails that fetch with the server's error and leaves the transaction failed, as in plain SQL; a value this version cannot hold (a timestamp past year 294246) is an
     `Error::Value`, the portal is then done and the connection is fine. `command_tag()` is the server's tag of the last fetch, whose row count covers that fetch only; `rows_fetched()` is the total. **The existing `Cursor` is unchanged**
     (it still streams a whole result, and the protocol delivers one result at a time, so it owns the connection while open; its documentation says so and points to the portal cursor). Added 2026-11-19: **`PortalCursor`**
     (`Connection::portal_cursor(sql, params, fetch_size)`, also on `Transaction`): the row-by-row API of a `Cursor` over a portal, fetching `fetch_size` rows at a time and **taking the connection as an argument of each call**, so
     the connection is free between calls (`next(&mut conn)` returns a row that borrows the cursor, not the connection; `next_chunk` returns whole chunks and refuses to follow a half-read chunk). **`karez-rt` has portals too**
     (added 2026-11-19): `SharedConnection::portal`, `fetch`, `submit_fetch` (a handle to wait on or `.await`; several fetches can be queued at once and are answered in order, the last ones finding the portal finished) and
     `close_portal`, backed by new engine commands (`open_portal`: Bind and Sync; `fetch_portal`: Execute with a limit and Sync, whose `Done` says `suspended`). A shared connection has one session for all its users, so the
     transaction a portal needs is the connection's: `BEGIN` it, and other threads' commands run inside it too (documented on `portal`).
   - **Metadata** (`metadata.rs`, `conn.metadata()`): `catalogs`, `db_schemas`, `tables`, `columns`, `constraints`, `table_types`, `table_schema` and `objects(depth, filter)`, as flat chunks. **`GetObjects` is nested (`list<struct<...>>`) and a `Chunk` has no list or
     struct column, so each level is one flat result with one row per leaf, keyed by `catalog_name`, `db_schema_name` and `table_name`; every ADBC field keeps its name, order and type.** The export (M5) groups the rows back. Two ADBC lists are folded into rows:
     `constraint_column_names` becomes one row per column (`constraint_column_name`, `constraint_column_position`) and `constraint_column_usage` repeats on those rows. Patterns are ADBC's (`%`, `_`, no escape character, so `LIKE ... ESCAPE ''`); `None` is no filter;
     the catalog is the current database; `pg_toast*` and `pg_temp_*` schemas are left out and `pg_catalog` and `information_schema` are in. Table types: `table`, `partitioned table`, `view`, `materialized view`, `foreign table`.
     `ordinal_position` counts from 1 without gaps left by dropped columns. The `xdbc_*` fields are filled where PostgreSQL has the answer (JDBC type code, base type name, length, precision, scale, radix, nullability, default, identity or `nextval` as
     autoincrement, `attgenerated`) and are NULL otherwise; a domain reports its base type. `xdbc_datetime_sub` is ODBC's code for date (1), time (2) and timestamp (3) types; `xdbc_char_octet_length` is the character length times the encoding's widest character (as
     `information_schema` computes it, 1 GiB for unbounded text), compared with `information_schema.columns.character_octet_length` in the test. The three scope fields are NULL **by definition**: they describe REF types, which PostgreSQL does not have.
     **Added 2026-11-19: `GetInfo`, `GetStatistics` and `GetStatisticNames`.** Their results contain dense unions (`info_value`, `statistic_value`), which a chunk cannot hold, so each union member is a column and exactly one is set on a row
     (none for a null value), in the specification's order and with its names; this needed two new chunk types, `UInt32` (`info_name`) and `UInt64` (the `uint64` member). `info(codes)` answers vendor name, version, SQL (true), Substrait (false, minimum and maximum
     null), driver name and version, and the ADBC API version (1001000, `ADBC_VERSION_1_1_0`) without sending a query; codes 2 and 102 (Arrow library versions) are omitted because there is no Arrow library, as ADBC allows for codes a driver does not report. The union's
     `string_list` and `int32_to_int32_list_map` members have no column because no reported code uses them. `statistics(filter, approximate)`: with `approximate` the planner's numbers as `float64` flagged approximate (`pg_class.reltuples` for the row count, and
     from `pg_stats` the average width, the distinct count with `n_distinct` below zero read as a fraction of the rows, and the null count from `null_frac`; a table never analysed has none); without it the statistics are computed exactly by reading each table
     (one scan per table: the row count, and per column the null count, distinct count, average and largest `pg_column_size`, and for integer, floating-point and text columns the minimum and maximum, text as UTF-8 bytes), as `int64` except the average width, which is `float64`. Tables the role
     cannot read are skipped. `statistic_names` is empty: there are no driver-specific statistics. The test compares exact values with the data, approximate values with `pg_stats`, and the schemas with `adbc.h` (the fixture now holds fourteen tables).
   - **Tests:** `tests/portals.rs` (10: two portals and other statements interleaved, 50 portals at once, 200,000 rows in fetches of 5,000, limit 0 and chunk splitting, native types and NULLs, no transaction, COMMIT and ROLLBACK, a failing row, a
     value error, savepoints, unsupported types; breaking the row limit fails 5) and `tests/metadata.rs` (5): **the schemas compared field by field with `adbc.h`** (`tests/adbc/get_objects_schema.txt` is the specification's tables, copied from
     apache/arrow-adbc commit 7f35429e, read 2026-11-19, parsed by the test and compared with the constants and the flat result schemas, including `not null`), a fixture table with a dropped column, a domain, an enum, an array, `numeric(12,3)`, `char(5)`, `time(0)`,
     a comment, identity and generated columns, a composite primary key, a composite foreign key, a unique and a check constraint, a view, a materialized view and a partitioned table, and the seeded database against `information_schema.columns` (122 columns) and
     `key_column_usage` with its foreign-key targets. Three mutations of the SQL (ordinal position from `attnum`, a foreign key naming the first referenced column only, an inverted nullable flag) each fail the fixture test; ignoring `PortalSuspended` in the `karez-rt` engine fails 3 of its portal tests. All of it passes on PostgreSQL 13.23 to 18.7
     (plain; portals and metadata also with TLS on 18).
6. **Pipeline and batch builders** in the blocking API as in plan 2.5 (`conn.pipeline()...run()`, `tx.batch(&stmt).rows(...)`), on top of the pipelining that `karez-rt` already does.
   Exit: S2 pipelined and unpipelined reported separately, as the plan's method requires.
   **Built 2026-11-19. Decisions:**
   - **`conn.pipeline()`** (`conn/pipeline.rs`; also `tx.pipeline()`): `query(sql, params)`, `execute(sql, params)`, `query_prepared(&stmt, params)`, `execute_prepared(&stmt, params)`, then `run()`. All statements go out with **one `Sync`** (Parse only for statements not
     yet prepared, one round trip each, before the group is sent) and the answers are read together. `run()` returns `Result<Vec<Result<StepResult, Error>>>`: the outer error means nothing ran (a statement that cannot be prepared, wrong parameter types or number,
     a type without a decoder in a query, a dead connection); otherwise one result per statement, each with its row count and, for a query, its rows as chunks. **A server error in one statement is that statement's result and every later one is the new
     `Error::Skipped`**, because the server skips the rest of a group; the group is one implicit transaction, so outside a transaction block nothing in it takes effect, and inside one the transaction is left failed. A value that does not fit a chunk fails only its own
     statement (`Error::Value`). The group may use at most as many distinct statements as the statement cache holds (the same rule as `execute_many`).
   - **`conn.batch(&stmt).rows(iter).group_size(n).run()`** (also `tx.batch`): one prepared statement run once per row of parameters, the iterator read lazily, **`group_size` statements per `Sync`** (default 1,000). The whole batch is atomic: outside a transaction it is
     wrapped in `BEGIN` and `COMMIT` (rolled back on any error), inside one it joins it. The outcome is `BatchOutcome { statements, rows_affected }`, the sum of each execution's own row count (so `ON CONFLICT DO NOTHING` counts what was inserted). For plain loads `COPY` is faster;
     a batch is for `INSERT`, `UPDATE` and `DELETE` with their semantics. The plan's "COPY, bulk-execute or INSERT BULK when available" is not done for PostgreSQL: its bulk path is `COPY`, which already exists (`copy_in`) and has different semantics.
   - **One engine for three callers.** `execute_many`, `pipeline().run()` and `batch().run()` share `run_group`, which encodes the Bind and Execute messages and reads the answers; the reading code was rewritten to attribute rows to statements (`Replies`), keeping
     the large-batch paths: a writer thread beside the reader on a plain connection, windows of about 64 KiB each followed by a Flush on a TLS connection.
   - **Tests** (`tests/pipeline.rs`, 8 with TLS): order and results of mixed queries and executes, a failure that skips the rest (atomic outside a transaction, a failed transaction inside), nothing runs on a prepare or parameter error, a value error that fails one statement,
     120 statements with 20 KB parameters and 2,000 small ones against the same queries run one at a time, a failure in the middle of a large group, a 25,000-row batch with row counts and group sizes, an `UPDATE` batch, `ON CONFLICT`, atomicity across groups, joining a transaction, and the TLS windows.
     Making queries lose their rows, reporting skipped statements as completed, or counting only the first statement of a group fails 5, 3 and 3 tests. Reading one statement fewer per TLS window is not caught: it only changes how eagerly replies are read.
   - **S2, pipelined and unpipelined, reported separately:** `crates/karez-pg/examples/pipeline_s2.rs` and perf-program.md section 21: one connection, a point select, unpipelined 170 µs per query and pipelined from 49 µs (4 per `Sync`) to 7 µs (256 per `Sync`) per query
     on a machine whose round trip is slow (WSL2 with Docker's proxy). **That is a local, one-connection figure, not the plan's S2 gate**, which needs the open-loop harness, competitors and the spend approved in step 9.
7. **Authentication. Built 2026-11-19 except Windows SSPI.** In this order: OAUTHBEARER (a token callback; PostgreSQL 18; no `unsafe`), SCRAM-SHA-256-PLUS (channel binding from the rustls exporter), then GSS/SSPI in its own crate
   with `unsafe` and a written safety argument per block (plan section 7.2; there is no paid audit). Exit: a Kerberos container and an OAuth validator fixture; if they cannot be built here, the step
   stops at OAUTHBEARER and SCRAM-PLUS and records GSS/SSPI as open.
   **Done, with decisions:**
   - **SCRAM-SHA-256-PLUS** (`karez-proto-pg/src/scram.rs`, `sha512.rs`; `karez-pg` `Config::channel_binding`, URL `channel_binding=disable|prefer|require`, default `prefer` as in libpq). **PostgreSQL implements one binding, `tls-server-end-point`
     (RFC 5929): the hash of the server's certificate.** The plan's "channel binding from the rustls exporter" does not apply: `tls-exporter` is not offered by PostgreSQL, so there is nothing for the exporter to bind. The hash function follows the certificate's own signature algorithm
     (SHA-256, SHA-384 or SHA-512; SHA-1, MD5 and SHA-224 are replaced by SHA-256, as RFC 5929 says), read from the DER certificate without a parser dependency (`certificate_signature_oid`); RSASSA-PSS and EdDSA have no plain hash, so no binding (and `require` fails with that reason). SHA-384/512
     are new (std only, constants derived at first use, checked against the NIST vectors including one million `a`). GS2 header: `p=tls-server-end-point,,` when bound; `y,,` when the connection is TLS and the server did not offer `-PLUS` (so a server that does support it can detect a stripped offer); `n,,` otherwise or when disabled.
     `require` without TLS, or against a server that offers no `-PLUS`, is refused before any secret is sent. **Real-server tests** (PostgreSQL 13 to 18 over TLS; the PostgreSQL 18 fixture now has a **SHA-384-signed certificate**, so that version binds with SHA-384): `require`, `prefer`, `disable` and a wrong password.
     Corrupting the hash, or sending `y` instead of binding, makes the real server refuse the login in both cases.
   - **OAUTHBEARER** (`karez-proto-pg/src/oauth.rs`; `Config::oauth_token(token)` and `oauth_token_with(|challenge| ...)`, `TokenProvider`, URL `auth=oauth`). The client sends `n,,\x01auth=Bearer <token>\x01\x01` after checking the token is in RFC 6750's `b64token` syntax (a control character could change the message). A rejected
     token gets the RFC's single `0x01` answer and then `Error::OAuthChallenge { challenge, discovery }` carrying the server's `status`, `scope` and `openid-configuration`. **Discovery:** if the token source returns an empty string the client sends an empty `auth=`, reads the challenge, calls the source again
     with it, and connects once more with the token it gets; the client does **not** fetch tokens itself (no HTTP client, no device flow: that is the callback's job, and libpq's built-in flow needs libcurl). It never logs the token. **Real-server test fixture:** `tests/docker/oauth` builds PostgreSQL 18 with a small validator module (C, 12 lines of logic, built against the server's headers in the image) that accepts
     `karez-token-for-<role>`; four tests (valid token, wrong token with the server's challenge, discovery, a role that takes a password); plus six against a scripted server (message shape, the `0x01` answer, discovery, failing or empty token source, policy, URL, no token in `Debug`).
   - **GSSAPI (Kerberos) on Unix** (`crates/karez-gss`, `karez-pg` feature `gss`; `Config::gss_credentials(principal, password)` or the ticket cache, `krbsrvname`, URL `auth=gss`). A new crate with the project's only FFI to a security library: 12 `unsafe` blocks, each with a `SAFETY:` comment, and a written argument in
     [gss-safety.md](gss-safety.md). The login needs the server's token back before it accepts (mutual authentication: `AuthenticationOk` before a complete context is an error). Gaps: no GSS encryption (`gssencmode`), no delegation, no channel binding for GSS. **Real-server test fixture:** `tests/docker/gss` builds a KDC (realm `KAREZ.TEST`) and a PostgreSQL 16 with a
     keytab; five tests (a login with a password and mutual authentication, a wrong password, policy, a service name the KDC does not know, a host name that is not the service principal). Needs `KRB5_CONFIG=tests/docker/gss/krb5-client.conf`, which sets `qualify_shortname = ""` (otherwise MIT Kerberos appends the machine's DNS search domain to `localhost`).
   - **Windows SSPI is open.** It is a different interface (`InitializeSecurityContext` in `secur32`), cannot be tested here without an Active Directory or a Kerberos realm reachable from Windows, and the plan says to record it as open if the fixture cannot be built. `karez-gss` compiles to a stub on Windows that
     returns an error saying so. On macOS the crate links the `GSS` framework but was not built there.
   - **Client certificates and direct TLS** (added 2026-11-19, after the step): `sslcert` and `sslkey` (PEM; PKCS#8, PKCS#1 and SEC1 keys; a key with a passphrase is refused with a clear error) are offered to the server in all three verifier modes. When a client certificate is
     set and the connection is TLS, the server may accept it with no password exchange (`hostssl ... cert`), and the client then accepts `AuthenticationOk` without an exchange (without a certificate that is still an error: "server did not ask for authentication"). `sslnegotiation=direct` skips the `SSLRequest` and
     starts the TLS handshake at once with ALPN `postgresql` (PostgreSQL 17 and later). Tested against PostgreSQL 17 (`cert` compose profile, port 54321): a login by certificate with the server seeing the certificate's subject, a missing key, an unreadable file, garbage in the key file, and a capture of the first bytes
     sent (the 8-byte `SSLRequest` against a TLS handshake record).
   - **Not done:** SCRAM `SCRAM-SHA-256` with SASLprep for non-ASCII passwords (the password is used as given; SASLprep needs the Unicode NFKC tables, which std does not have), `tls-exporter` binding (not offered by PostgreSQL), GSS encryption (`gssencmode`), Windows SSPI, and passphrase-protected client keys.
8. **Coverage-guided fuzzing outside the std-only crates. Built 2026-11-19.** A `fuzz/` directory (its own workspace, excluded from the main one, so the core keeps no dependency) with seven cargo-fuzz (libFuzzer) targets: `frame`, `message`, `row`, `scram`,
   `oauth`, `cert` and `config_url` (what each one checks is in [fuzz/README.md](../../fuzz/README.md)). Seed corpus: 101 small valid inputs in `fuzz/corpus/`, written by the ignored test `dump_seed_corpus` from step 1's generators (the `cert` seeds are the DER of the fixture CA certificates).
   `fuzz/run.sh` runs every target for a set time and keeps new inputs outside the repository. A workflow, `.github/workflows/fuzz.yml`, runs it by hand and once a week (120 s per target by default); it has not run yet, because nothing is pushed to a runner that has it.
   **First run [measured here] (2026-11-19, WSL2 on the owner's Windows machine, nightly rustc 1.102.0, cargo-fuzz 0.13.2, one core per target, 240 s each, from the seeds):** 89.7 million executions in total (frame 8.7 M, message 12.6 M, row 2.3 M, scram 0.16 M, oauth 7.0 M, cert 56.3 M, config_url 2.7 M),
   **no crash, hang or failed assertion in the library code.** One failure came from the first version of the `cert` target's own assertion (it required a non-empty OID, which a damaged certificate can legitimately give); the target was corrected to check that an empty OID chooses no hash, and the corrected target ran clean.
   Coverage at the end: frame 741 edges, message 751, row 1,253, scram 435, oauth 293, cert 101, config_url 644 (libFuzzer's `cov`; not a line-coverage figure).
   **Limits:** that is 28 CPU-minutes in total, against the plan's M8 target of 30 CPU-days without a new crash, so it shows the harness works and finds nothing in the first minutes, not that the parsers are free of crashes. No dictionary and no structure-aware mutator (the `row` target builds its column list from raw bytes). The `scram` target runs slowly (about 670 runs a second) because the client does a PBKDF2 of the server's chosen iteration count; the count is capped at 1,000,000 (`MAX_ITERATIONS`), so there is no hang. It does not fuzz the client-to-server direction, TLS, GSS tokens (the system library parses those) or the metadata and portal code, which only see parsed rows.
9. **Exit measurements (needs the owner's spend approval, per CLAUDE.md). Partly done 2026-11-19; the gate is open.** S6 COPY against PgBulkInsert (the plan wants at least 1.5x; plan section 10 row 4), S1 to S8 with TLS on,
   and the S8 wait-time distribution against the Erlang C prediction. The TLS bulk-read adapter gap from perf-program.md section 20 closes here.
   **Built:** a `pgbulkinsert` adapter (binary COPY through PgBulkInsert 8.1.4 on pgjdbc, S6 only; its table contents match pgjdbc's COPY by md5), `run.py bulk --bulk-tls` (S4 to S6 over TLS; the tokio-postgres bulk adapter now connects with TLS, which it did not before;
   the trust root is passed to bulk cells), the `exit` profile (S1, S2, S3 and S8, each plain and over TLS) and `bench/cloud/lane-exit.sh`.
   **Run** (the owner's eighth spend exception, CLAUDE.md): one lane, two droplets, 77 minutes, about $0.18. `c-4` was not available in `blr1`, so the owner chose `s-4vcpu-8gb` (shared CPU); the numbers are not comparable with the `c-4` campaigns.
   Full report: [bench/results/2026-11-19-exit/REPORT.md](../../bench/results/2026-11-19-exit/REPORT.md). Results `[measured here]`:
   - **S6 (10,000,000 rows): karez 318,889 rows/s, PgBulkInsert 357,862 (karez / PgBulkInsert = 0.89x; karez uses 0.66x the CPU and about 1/300 of the memory).** The 1.5x gate is **not met on this class and not decided**: the database had one busy backend (20% to 32% of four vCPUs) and
     eight of ten clients were within 296,000 to 358,000 rows/s, so the server's insert rate is the likely limit, while on `c-4` in campaign 2 karez wrote 736,214 rows/s. The gate needs a rerun on `c-4` with PgBulkInsert beside karez. PgBulkInsert writes binary COPY and karez's adapter text COPY; a binary COPY writer in karez is the
     first thing to try and is not built. Over TLS: 0.95x (the passes overlap).
   - **TLS bulk reads (S4):** every client loses 3% to 36% of its rate over TLS; karez's row view 24%, its chunk path 5%. karez is not ahead on S4 in this run (pgx and tokio-postgres are 11% and 5% above the row view, plain). S5 batch insert: karez is fifth of eight.
   - **Not done:** S1, S2, S3 and S8 over TLS (open loop). On the shared droplets no client met the 2 ms p99 bound at any rate down to 16 requests a second (p99 3.4 to 36 ms), so the open-loop part gave no result and was stopped. It needs `c-4`. The S8 wait-time distribution against Erlang C was not attempted
     (it needs the open loop). The harness side of both is built and was checked locally.

## 3. Two decisions that change the plan (the first one decided)

- **`karez-pool` and the driver-neutral API (plan 2.5: `Driver`, `Url`, `drivers().get(&url)`).** Decided by the owner on 2026-11-18: extract the pool now (the recommendation had been to wait for M2). Done as step 1b. What
  that covers: the pool and its `Manager` trait, which is the one driver-side interface the pool needs. What it does not cover: the query-level traits of plan 2.5 (`Driver`, `Connection`, `Statement`, `Cursor`, the
  registry). With one driver those would be guesses about placeholders, type systems and auth that a second driver would contradict, so they wait for M2; the `Manager` trait is small enough to change then.
- **GSS/SSPI.** It is the only M1 item that needs `unsafe` and it cannot be tested without a Kerberos or Windows domain fixture. It is last in step 7 so the rest of M1 does not wait for it.

## 4. Not in M1 (unchanged from the plan)

Other databases (M2 to M4), the exports (M5), the JDBC and ODBC facades (M6, M7), the parity checklist freeze (needs JDBC 4.3 chapter 6 and ISO 9075-3:2023, STATUS next steps), and any claim
about speed: the M1 speed targets are read off step 9 only.
