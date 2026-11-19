# karez: research

_Internal design note, 2026-11-09. Written to support the build decision in [plan.md](plan.md). Nothing here has been built._

## How to read this note

This note synthesises the seven research notes in `docs/research/` (`rust-landscape.md`,
`standards.md`, `wire-protocols.md`, `papers-and-math.md`, `jdbc-baseline-and-benchmarks.md`,
`security-size-testing.md`, `consumers-and-bridges.md`). It answers one question: what does the
evidence say about building a ground-up Rust database connectivity library (karez: a common API
plus native drivers, succeeding the dormant tokio-rs/rdbc) that is significantly faster than native JDBC
drivers and every Rust alternative, smaller in binary size, and at feature parity with the JDBC,
ODBC and ADBC standards. It does not contain the plan; plan.md cites the section numbers below.

- **[measured here]** means a number measured on this project's own machines with this
  project's own harness. **There are none yet.** Every number in this note is somebody else's.
- **[vendor]** means a number or claim published by the project or company that owns the thing
  measured (a crate's README, a database's docs, a driver vendor's blog, a benchmark run by the
  crate's own maintainers). The notes also use **[project]** for an open-source project's own
  repository; it is folded into [vendor] here. Treat these as claims, not measurements.
- **[third party]** means an independent write-up, aggregation, advisory database or blog.
- **[paper]** means a peer-reviewed primary source that the research opened in full.
- **[checked YYYY-MM-DD]** means the research opened that URL on that date. Almost everything
  was checked 2026-11-09; a few pages were opened 2026-11-07 in a first, rate-limited session.
  Standards PDFs that the fetcher could not read were downloaded and text-extracted locally; the
  notes tag these **[downloaded+pdftotext 2026-11-09]**. Statements that a note marked as
  recollection rather than an opened source are repeated here with that caveat, and listed again
  in section 13.
- In section 6, **(restated)** marks a textbook formula quoted from memory of its standard form
  without the primary text having been opened; **(derived)** marks algebra the notes did from
  cited inputs. Check derived results before relying on them.
- **Numbers from different machines, rounds or load models are never put in one ranking.** Each
  table below names its machine or says the machine is unknown. Where the same quantity was
  measured on two machines, both appear in separate rows or tables with their own tags.
- Where two notes disagree, both figures are kept and the disagreement is stated.

## 1. Starting point: tokio-rs/rdbc today

### What it is

tokio-rs/rdbc is a 2019 proof of concept. The crate `rdbc` 0.1.6 was published 2019-12-23 and
never again; it has 12,400 lifetime and 131 recent (rolling ~90-day) downloads; licence
Apache-2.0 **[vendor, checked 2026-11-09]** (https://crates.io/api/v1/crates/rdbc). The
repository was created 2019-12-15, last pushed 2021-05-23, and has 583 stars, 26 forks and 23
open issues plus PRs **[vendor, checked 2026-11-09]**
(https://api.github.com/repos/tokio-rs/rdbc). Andy Grove announced it on r/rust on 2019-12-18:
"This weekend I was trying to write a generic database tool but could not find an equivalent to
ODBC/JDBC" **[third party, checked 2026-11-09 via search]**
(https://redlib.hbubli.cc/r/rust/comments/ecdr8t/announcing_rust_database_connectivity_rdbc/).

Its README describes five traits and states its own limits **[vendor, checked 2026-11-09]**
(https://raw.githubusercontent.com/tokio-rs/rdbc/master/README.md):

- `Driver` (`Sync + Send`, `connect(&self, url) -> Box<dyn Connection>`);
- `Connection` (`create` / `prepare` returning `Box<dyn Statement + '_>`);
- `Statement` (`execute_query(&[Value])` / `execute_update`);
- `ResultSet` (`next()`, typed getters `get_i8` .. `get_f64` / `get_string` / `get_bytes`
  returning `Option`, 0-based);
- `ResultSetMetaData`.

"The provided RDBC drivers are just wrappers around existing database driver crates"; "There is
currently no `async` support"; "This is just an experimental PoC and is not currently suitable
for anything"; the design is "intentionally modeled directly after ODBC and JDBC". The crate no
longer compiles: a `sqlparser` error was reported in July 2020 in issue #64 and PR #68 "update
sqlparser" (2021-05-23, the last activity in the repository) was never merged **[vendor, checked
2026-11-09]** (https://github.com/tokio-rs/rdbc/issues/64,
https://api.github.com/repos/tokio-rs/rdbc/issues?state=all&per_page=100).

### The 2021 stall

Three compounding causes are visible in the tracker **[vendor, checked 2026-11-09]**
(https://api.github.com/repos/tokio-rs/rdbc/issues/66/comments,
https://api.github.com/repos/tokio-rs/rdbc/issues/67/comments,
https://github.com/tokio-rs/rdbc/issues/53, https://github.com/tokio-rs/rdbc/issues/64):

1. **A sync, JDBC-copied API that could not express async.** On 2021-01-31 andygrove answered
   "Is this project dead?" with "I wouldn't say 'dead' but maybe comatose", saying he was busy
   with Ballista and nobody had stepped up to lead. On 2021-02-09 aloucks added "The lack of
   `async` trait functions also presents some challenges". `async fn` in traits stabilised only
   in Rust 1.75 (December 2023), nearly three years after the stall.
2. **Design problems the author himself listed** in issue #53 (2020-01-14): traits are
   synchronous; "one accessor per type (e.g. `get_i32()`) instead of generics"; ownership
   problems with more than one prepared statement per connection; "no batch or columnar
   support".
3. **An undecided trait-object-vs-generics argument** (#64, July 2020 to January 2021): mversic
   wanted associated-type generics with a boxed wrapper; aloucks a `RawConnectionVTable` like
   `futures::task::RawWaker`; nmusatti (5 upvotes) argued dynamic dispatch is negligible next to
   I/O and that "sqlx already offers the generic approach, so RDBC should focus on the
   runtime-pluggable model". No decision was taken.

Issue #67 "Clarify maintenance status" (opened 2021-03-31) is still open. Its comments are the
project's epitaph: nociza (2023-04-05) announcing rdbc2 as a replacement; jcbritobr (2023-04-12)
"Seems this project died suddenly. 3 Years without updates"; rafialhamd (2025-08-14) noting that
rdbc2 itself had been inactive for two years.

### What its open issues asked for

Every item below is still open (2019-12 to 2021-05) **[vendor, checked 2026-11-09]**
(https://api.github.com/repos/tokio-rs/rdbc/issues?state=all&per_page=100&page=1&direction=asc).
They read as a requirements list for the rewrite.

| Issue | Ask | Where this note treats it |
|---|---|---|
| #5 | RDBC-ODBC bridge | sections 4, 10 |
| #9 | Dynamically load a driver at runtime | sections 4 (ADBC manager), 10 |
| #13 | SQL Server driver | sections 2, 5 |
| #17 | SQL-injection protection | section 8 |
| #18 | "How can we support async drivers?" | sections 2, 4 (R2DBC) |
| #26 | Coverage | section 11 |
| #37 | Named parameters | section 4 |
| #38 | "Enable Performant Batch Inserts" | sections 5, 7 |
| #39 | Retrieval of auto-generated keys | section 4 |
| #40 | Transactions | section 4 |
| #41 | Vectors / complex types | section 4 |
| #42 | 64-bit integers | section 4 |
| #46 | "Impossible to create multiple prepared statements at once" | section 2 (lessons) |
| #52 | "Design RowSet access for batch and columnar support" | sections 5, 6, 7 |
| #53 | "Design async version of traits" | section 4 |
| #55 | Temporal types in result sets | section 4 |
| #56 | Multiple result sets per statement | section 4 |
| #63 | MySQL prepared-statement Date conversion failure | section 5 |
| #64 | "Replace trait objects with generics" | section 2 (lessons) |
| PR #61 | "Use Stream<Row> in API" | section 4 (R2DBC) |
| PR #65 | "Generics for data access" (draft) | section 2 |
| PR #68 | "update sqlparser" (2021-05-23, last activity) | — |

### What its successors did

All of them are dormant **[vendor, checked 2026-11-09]**.

| Project | Shape | Last release / push | Downloads (total / recent) | Licence | Note |
|---|---|---:|---:|---|---|
| rdbc2 (CoLearn-Dev) | one `Database::new(url)` with `execute_query[_with_params]` and JSON/raw serialisation variants; no Connection/Statement/Row types documented | 0.2.2 on 2023-04-28; last push 2023-05-06; 3 stars | 10,538 / 52 | crates.io MIT; GitHub licence field null | README: "As of May 2023, rdbc is no longer maintained"; MySQL and SQLite checked, PostgreSQL unchecked (https://crates.io/api/v1/crates/rdbc2, https://raw.githubusercontent.com/CoLearn-Dev/rdbc2/main/README.md) |
| rdbc-rs | "Futures base Rust DataBase Connectivity (RDBC) library" | 0.1.0 on 2023-02-27 | 2,158 / 40 | "non-standard", no repository field | https://crates.io/api/v1/crates/rdbc-rs |
| jdbc (sika-rs/jdbc-rs) | JNI bridge to a JVM: `Builder::new().jdbc_url(..).build()`, `prepare_statement`, `set_int`, `execute_query`, `next`, `get_string/get_int` | 0.1.0, August 2023 | — | MIT | purpose "to provide the implementation of sqlx to temporarily support databases not supported by Sqlx"; type checklist short/int/long/float/double/boolean/String/Date only (https://docs.rs/crate/jdbc/latest/source/README.md) |
| rust-dbc/rdbc | unrelated earlier "Rust Database Connectivity Layer", not a fork | created and last pushed 2019-02-19; 5 stars | — | MIT | https://api.github.com/repos/rust-dbc/rdbc |

Measured by adoption the entire RDBC family has under 25,000 lifetime downloads. The crates it
would have to interoperate with have 184.8 M (sqlx, 4,249 dependent crates) and 148.9 M
(rusqlite, 6,001 dependents) **[vendor, checked 2026-11-09]** (section 2). The note's reading:
a new common API must interoperate with those, not displace them.

## 2. The Rust field

### Comparison table: architecture

Sources are each crate's README or docs.rs page, all **[vendor, checked 2026-11-09]**; URLs
follow the table. "Protocol" says whether the wire protocol is implemented in Rust or wrapped
from a C/Go library. "Pipelining" means several requests in flight on one connection.

| Crate | I/O model | Sync / async | Protocol | Pipelining | Binary protocol | Columnar output | TLS | Pooling |
|---|---|---|---|---|---|---|---|---|
| sqlx 0.9.0 | I/O runs in the calling task (no connection task) | async: tokio; smol and async-global-executor added in 0.9.0; async-std deprecated | pure Rust for PostgreSQL, MySQL, MariaDB; SQLite bundled C (`sqlite-unbundled` links system lib); MSSQL removed "pending a full rewrite" | no: #408 open since 2020-06-13, "low priority"; PR #3891 closed unmerged 2026-09-14 | yes: prepared statements cached per connection, "binary mode of communication"; rows "decoded on demand" | no | `tls-native-tls`, `tls-rustls` (ring), `tls-rustls-aws-lc-rs`, `tls-rustls-ring-native-roots`, `tls-none` | built-in `sqlx::Pool` (`max_connections`, `min_connections`, `acquire_timeout`, `idle_timeout`, `max_lifetime`, `test_before_acquire` default true, hooks); no fairness statement in docs |
| sqlx-oldapi 0.6.59 | as sqlx 0.6 | async | adds MSSQL and ODBC drivers to the 0.6 API | no | yes | no | as sqlx 0.6 | built-in |
| tokio-postgres 0.7.18 | `connect` returns a `Client` and a `Connection`; "the connection object performs the actual communication with the database, so spawn it off" | async over any `AsyncRead + AsyncWrite`; `runtime` feature adds tokio conveniences | pure Rust (`postgres-protocol`, `postgres-types`) | yes: "happens automatically when futures are polled concurrently" | yes | no | external: `postgres-openssl`, `postgres-native-tls`; rustls via third-party `tokio-postgres-rustls` | none in-tree (deadpool-postgres adds `StatementCache`, `RecyclingMethod::{Fast, Verified}`) |
| postgres 0.19.14 | "a wrapper around a `tokio_postgres::Client` along side a tokio `Runtime`" | sync | as above | n/a | yes | no | as above | r2d2-postgres |
| xitca-postgres 0.4.0 (crates) / 0.5.0 (git, pinned by TFB) | dedicated task; "A built-in connection pool with pipelining enabled"; "Row data is held in reference-counted byte buffers, so values can be sliced and parsed without copying" | async and blocking | "builds on rust-postgres" | yes | yes | no | rustls; optional io_uring via `tokio-uring-xitca` (nightly); QUIC transport proxy | built-in, pipelined |
| may_postgres (git only) | stackful coroutines on the `may` runtime | coroutine | "Porting from rust-postgres" | yes | yes | no | — | — |
| wtx 0.52.1 | multi-protocol crate with a PostgreSQL client | async | pure Rust | not documented in the notes | not documented | no | — | — |
| mysql_async 0.37.1 | tokio | async | pure Rust (`mysql_common`) | not mentioned in README | text protocol in `query*` ("values come back as strings"), binary in `exec*`; LRU statement cache (`stmt_cache_size`) | no | `native-tls` default, or rustls | built-in `Pool` (`pool_min`, `pool_max`, `reset_connection`; "`Pool::new` is lazy") |
| mysql 28.0.4 | blocking | sync | pure Rust (`mysql_common`) | no | same split; LRU statement cache bounded by server `max_prepared_stmt_count`; named parameters `:name` via `params!` | no | `native-tls`, `rustls-tls` (aws-lc-rs), `rustls-tls-ring`, bare `rustls` | r2d2-mysql |
| rusqlite 0.41.1 | FFI over `libsqlite3-sys` | sync | C: `bundled` compiles SQLite 3.54.0; default dynamic link requires SQLite >= 3.45.3 | n/a | n/a (in-process) | no | n/a | r2d2-sqlite, deadpool-sqlite |
| duckdb 1.20000.1 | FFI; "API modeled on rusqlite"; `Connection` is `Send` but not `Sync` | sync | C: bundled DuckDB v2.0.0 | n/a | n/a | yes: `appender-arrow`, `vtab-arrow` (RecordBatch <-> chunks), `polars` | n/a | r2d2 feature |
| tiberius 0.13.0 | `Client` over any futures `AsyncRead + AsyncWrite` (tokio, async-std, smol) | async | pure Rust TDS; SQL Server 2022/2019/2017 tested in CI, 2016–2008 "should work", 2005 with `tds73` off | not mentioned | TDS is binary; NUMERIC in i128 (precision <= 38); bulk via `BulkLoadRequest::send(TokenRow)`, flushed per full packet, `finalize()` required | no | `native-tls` default, `rustls`, `vendored-openssl`; encryption `Required` (default) / `Off` (login only) / `NotSupported` | "connection pooling" is a stated non-goal (bb8-tiberius, deadpool-tiberius exist) |
| oracle 0.6.3 | FFI over ODPI-C; "requires Oracle client 11.2 or later" at runtime and a C compiler at build | sync | C | n/a | via client | no | via client | r2d2-oracle |
| odbc-api 30.0.0 / arrow-odbc 27.0.0 | FFI over the platform ODBC driver manager; "An ODBC 3.8 environment" | sync, plus `CursorPolling` and `ConcurrentBlockCursor` on a dedicated thread | C (the ODBC driver) | n/a | block cursors into bound buffers; `ColumnarBulkInserter` for array-parameter inserts | yes: arrow-odbc reads "as a sequence of Arrow record batches", `with_max_bytes_per_batch` | via driver | — |
| diesel 2.3.14 | C client libraries: `pq-sys` (libpq), `mysqlclient-sys`, `libsqlite3-sys`; `pq-src`/`mysqlclient-src` bundle sources | sync | C | n/a | via C lib | no | via C lib | r2d2 |
| diesel-async 0.9.2 | tokio-postgres / mysql_async underneath; "designed to work in combination with diesel, not to replace diesel" | async | pure Rust | yes on PostgreSQL ("supported by diesel-async and tokio-postgres, but not by SQLx or SeaORM") | yes | no | via underlying crate | deadpool, bb8, mobc |
| sea-orm 2.0.4 | built on sqlx 0.9; backend erasure is an enum (`DatabaseConnection` with per-backend sqlx pool variants plus Mock/Proxy — variant list not confirmed, see section 13) | async; `sea-orm-sync` since 2025-12-12 | sqlx | no | yes | `with-arrow` feature; Arrow & Parquet support 2026-02-22 | "SeaORM's default features do not enable TLS" | sqlx pool |
| adbc_core 0.25.0 / adbc_driver_manager / adbc_ffi | traits `Driver`, `Database`, `Connection`, `Statement`; a driver manager that loads drivers over the C ABI; `adbc_ffi::export_driver!` for `cdylib` export | sync traits | drivers: `adbc_snowflake` "is a Rust wrapper around the Go ADBC Snowflake driver" (default `bundled` "requires a Go compiler at build time") | n/a | Arrow record batches | yes (native) | per driver | — |
| connectorx 0.4.6 | "follows 'zero-copy' principle", data "copied exactly once"; partitions a query on a numeric column, one thread per partition | sync | a federation of existing drivers behind `src_*` features: `postgres` 0.19 + `r2d2_postgres`, `mysql_common` 0.32 + `r2d2_mysql`, `rusqlite` 0.37, `tiberius` 0.7 + `bb8-tiberius`, `oracle` 0.6 + `r2d2-oracle`, `gcp-bigquery-client`, `prusto`, `clickhouse` 0.15, `datafusion` 50.3 | per underlying driver | per underlying driver | yes: Pandas, PyArrow, Modin, Dask, Polars destinations | per driver | r2d2 / bb8 |
| datafusion-table-providers 0.15.1 | leaf crates per source; federation via `datafusion-federation` | async | wraps PostgreSQL, MySQL, SQLite (rusqlite bundled), ClickHouse (official), DuckDB, Flight SQL, MongoDB, ADBC, ODBC (arrow-odbc 25.3), Oracle (rust-oracle), Cassandra (scylla) | per source | per source | yes (DataFusion) | per source | bb8 0.9, r2d2 0.8 |
| clickhouse 0.15.3 (official) | HTTP transport | async | "`RowBinaryWithNamesAndTypes` or `RowBinary` formats over HTTP"; "There are plans to implement `Native` format over TCP" | — | RowBinary | no | `native-tls` or `rustls-tls` | — |
| clickhouse-rs (suharev7) | native TCP | async | pure Rust native protocol | — | columnar blocks | — | — | — |
| snowflake-api 0.14.0 | REST | async | "undocumented public API" per its own README | — | — | Arrow results | — | — |
| rdbc 0.1.6 | `Box<dyn>` wrappers over other crates | sync | wrappers | no | no | no | — | — |

Pools (all **[vendor, checked 2026-11-09]**): r2d2 is sync with a `ManageConnection` trait and
adapters for postgres, mysql, sqlite, oracle, odbc. bb8 is tokio-based, "originally based on"
r2d2, with explicit `QueueStrategy::{Fifo, Lifo}` (Fifo "spreads load evenly", Lifo "keeps the
total pool size small"). deadpool offers managed and unmanaged pools, "never performs any
actions in the background", checks health on the next retrieval rather than on return, depends
on tokio only for `tokio::sync::Semaphore`, and has hooks `post_create/pre_recycle/post_recycle`
and runtime resize; deadpool-postgres's default `RecyclingMethod::Fast` (only `is_closed`) "can
occasionally return a connection that is no longer usable" on unreliable networks. mobc has
`max_open` default 10, `get_timeout` default 30 s and wait metrics. None of the four documents
fairness beyond bb8's two strategies; an old sqlx bench file compared "(fair)" vs "(unfair)"
acquire, implying a removed or internal knob **[third party, via search]**.

Table sources: https://raw.githubusercontent.com/launchbadge/sqlx/main/README.md,
https://raw.githubusercontent.com/launchbadge/sqlx/main/CHANGELOG.md,
https://docs.rs/crate/sqlx/latest/features, https://github.com/launchbadge/sqlx/issues/408,
https://github.com/launchbadge/sqlx/issues/2436, https://github.com/launchbadge/sqlx/pull/3891,
https://docs.rs/sqlx-core/latest/sqlx_core/pool/struct.PoolOptions.html,
https://docs.rs/tokio-postgres/latest/tokio_postgres/,
https://docs.rs/deadpool-postgres/latest/deadpool_postgres/, https://docs.rs/postgres/latest/postgres/,
https://raw.githubusercontent.com/HFQR/xitca-web/main/postgres/README.md,
https://raw.githubusercontent.com/Xudong-Huang/may_postgres/master/README.md,
https://raw.githubusercontent.com/blackbeam/mysql_async/master/README.md,
https://raw.githubusercontent.com/blackbeam/rust-mysql-simple/master/README.md,
https://docs.rs/mysql_common/latest/mysql_common/,
https://raw.githubusercontent.com/rusqlite/rusqlite/master/README.md,
https://raw.githubusercontent.com/duckdb/duckdb-rs/main/README.md,
https://raw.githubusercontent.com/prisma/tiberius/main/README.md,
https://docs.rs/tiberius/latest/tiberius/struct.BulkLoadRequest.html,
https://raw.githubusercontent.com/kubo/rust-oracle/master/README.md,
https://docs.rs/odbc-api/latest/odbc_api/, https://raw.githubusercontent.com/pacman82/arrow-odbc/main/README.md,
https://docs.rs/crate/diesel/latest/features, https://diesel.rs/compare_diesel.html,
https://raw.githubusercontent.com/weiznich/diesel_async/main/README.md,
https://docs.rs/crate/sea-orm/latest/features, https://www.sea-ql.org/blog/,
https://raw.githubusercontent.com/apache/arrow-adbc/main/rust/README.md,
https://docs.rs/adbc_snowflake/latest/adbc_snowflake/,
https://raw.githubusercontent.com/sfu-db/connector-x/main/README.md,
https://raw.githubusercontent.com/sfu-db/connector-x/main/connectorx/Cargo.toml,
https://raw.githubusercontent.com/datafusion-contrib/datafusion-table-providers/main/README.md,
https://raw.githubusercontent.com/ClickHouse/clickhouse-rs/main/README.md,
https://raw.githubusercontent.com/mycelial/snowflake-rs/main/README.md,
https://raw.githubusercontent.com/sfackler/r2d2/master/README.md,
https://raw.githubusercontent.com/djc/bb8/main/README.md, https://docs.rs/deadpool/latest/deadpool/,
https://raw.githubusercontent.com/importcjj/mobc/master/README.md.

### Comparison table: maintenance status, adoption, licence

crates.io figures are the registry's own (`https://crates.io/api/v1/crates/<name>`); "recent"
is the rolling ~90-day download count; GitHub figures are from
`https://api.github.com/repos/<owner>/<repo>` and "open" includes PRs. All **[vendor, checked
2026-11-09]**.

| Crate | Latest release (date) | Downloads total / recent | Reverse deps | Stars / open / last push | Licence | Status |
|---|---|---:|---:|---|---|---|
| sqlx | 0.9.0 (2026-05-21; 0.8.6 was 2025-05-19, 0.8.0 2024-07-22) | 184,768,424 / 55,609,771 | 4,249 | 17,641 / 786 / 2026-11-08 (now `transact-rs/sqlx`) | MIT OR Apache-2.0 | active; 0.9.0 announcement says the project "has not been owned or maintained by LaunchBadge, LLC. for a few years" |
| sqlx-oldapi | 0.6.59 (2026-10-01) | 76,651 / 6,606 | — | — | MIT OR Apache-2.0 | lovasoa fork of the 0.6 API |
| diesel | 2.3.14 (2026-10-07) | 44,490,277 / 11,663,457 | 675 | 14,226 / 169 / 2026-11-07 | MIT OR Apache-2.0 | active |
| diesel-async | 0.9.2 (2026-06-19) | 17,097,475 / 6,162,654 | — | 829 / 10 / 2026-11-08 (now `diesel-rs/diesel_async`) | MIT OR Apache-2.0 | active |
| sea-orm | 2.0.4 (2026-09-27; 2.0.0 on 2026-07-20 after "43 release candidates") | 29,123,906 / 6,057,706 | 567 | 9,953 / 227 / 2026-11-08 | MIT OR Apache-2.0 | active; "SeaORM X" is a proprietary MSSQL extension (2026-02-25) |
| sea-query | 1.0.2 (2026-08-12) | 40,564,701 / 7,106,787 | — | 1,779 / 113 | MIT OR Apache-2.0 (GitHub shows NOASSERTION) | active |
| tokio-postgres | 0.7.18 (2026-06-12) | 85,308,301 / 25,656,071 | 893 | 4,032 / 202 / 2026-10-03 (now `rust-postgres/rust-postgres`) | MIT OR Apache-2.0 | active |
| postgres (sync) | 0.19.14 (2026-06-12) | 21,603,270 / 4,974,117 | — | same repo | MIT OR Apache-2.0 | active |
| postgres-protocol | 0.6.12 (2026-06-12) | 91,411,959 / 27,332,850 | 82 direct | same repo | MIT OR Apache-2.0 | active; 0.6.x line since 2019 |
| postgres-types | 0.2.14 (2026-06-12) | 90,451,335 / 27,237,326 | — | same repo | MIT OR Apache-2.0 | active |
| xitca-postgres | 0.4.0 (2026-02-20); TFB pins 0.5.0 from git | 5,787 / 1,495 | — | — | Apache-2.0 | active |
| wtx | 0.52.1 (2026-08-14) | 72,509 / 3,674 | — | — | MPL-2.0 | active |
| mysql (sync) | 28.0.4 (2026-10-16) | 4,723,209 / 899,591 | — | 693 / 15 / 2026-10-16 | MIT/Apache-2.0 | active |
| mysql_async | 0.37.1 (2026-09-01) | 8,695,144 / 1,879,909 | 157 | 419 / 36 / 2026-11-08 | MIT OR Apache-2.0 | active |
| mysql_common | 0.38.2 (2026-07-24) | 20,910,114 / 4,684,177 | 60 direct | — | MIT OR Apache-2.0 | active |
| rusqlite | 0.41.1 (2026-10-28) | 148,853,857 / 57,639,210 | 6,001 | 4,469 / 172 / 2026-11-08 | MIT | active |
| duckdb | 1.20000.1 (2026-10-28) | 5,819,475 / 2,542,506 | — | 986 / 26 / 2026-11-09 | MIT | active |
| tiberius | 0.13.0 (2026-09-25) | 5,869,720 / 996,701 | 123 | 448 / 50 / 2026-09-25 (now `tiberius-rs/tiberius`) | MIT/Apache-2.0 | risk: org page names no maintainers, security contact still `security@prisma.io`, CI badge still `prisma/tiberius`, Releases page empty; historical RUSTSEC-2020-0010 on the original steffengy repo |
| oracle (rust-oracle) | 0.6.3 (2025-01-02) | 3,570,475 / 1,118,961 | — | 229 / 29 / 2025-03-23 | UPL-1.0/Apache-2.0 (GitHub field null) | risk: no release in 22 months |
| odbc-api | 30.0.0 (2026-11-07) | 1,828,536 / 312,514 | — | 188 / 10 / 2026-11-08 | MIT | active |
| arrow-odbc | 27.0.0 (2026-11-07) | 592,568 / 145,929 | — | 83 / 2 | MIT | active |
| r2d2 | 0.8.10 (2022-06-21) | 42,338,751 / 9,307,134 | 447 | 1,655 / — / 2026-10-26 | MIT/Apache-2.0 | no release in 4+ years, repo still pushed |
| bb8 | 0.9.1 (2025-11-24) | 43,764,664 / 10,672,256 | 172 | 955 / — / 2026-10-26 | MIT | active |
| deadpool | 0.13.1 (2026-08-26) | 166,815,447 / 50,494,079 | 206 | 1,336 / 66 | MIT OR Apache-2.0 | active |
| mobc | 0.9.0 (2025-06-25) | 2,486,707 / 216,472 | — | 297 / — / 2025-06-26 | MIT/Apache-2.0 | quiet |
| adbc_core / adbc_driver_manager / adbc_ffi | 0.25.0 (2026-10-26) | 441,875 / 228,890; 258,108 / 121,688; 403,876 / 212,920 | — | apache/arrow-adbc 637 / 358 / 2026-11-09 | Apache-2.0 | active; docs.rs failed to build adbc_snowflake 0.18–0.23 |
| connectorx | 0.4.6 (2026-09-18) | 125,056 / 13,681 (the Python wheel is the main distribution) | — | 2,660 / 176 / 2026-11-08 | MIT | active; still pins tiberius 0.7 |
| datafusion-table-providers | 0.15.1 (2026-10-29) | 222,437 / 123,602 | — | 214 / — / 2026-11-03 | Apache-2.0 | active; "not an official Apache Software Foundation project" |
| clickhouse (official) | 0.15.3 (2026-10-13) | 16,861,937 / 5,864,189 | — | 570 / 94 / 2026-11-07 | MIT OR Apache-2.0 | active, HTTP only |
| clickhouse-rs (suharev7) | 1.1.0-alpha.1 (2023-09-10); last stable 0.1.21 | 210,316 / 18,828 | — | 340 / 67 / 2024-07-16 | — | dormant |
| snowflake-api | 0.14.0 (2025-10-23) | 7,573,785 / 441,017 | — | repo `mycelial/snowflake-rs` (the `snowflake-api` path 404s) | Apache-2.0 | built on an "undocumented public API" |
| dbcrossbar (CLI, not a library) | 0.5.7 stable, 0.5.8-pre.3 (2024-11-01) | 87,889 / 203 | — | 239 / — / 2026-09-02 | Apache-2.0 OR MIT | quiet |
| rdbc (tokio-rs) | 0.1.6 (2019-12-23) | 12,400 / 131 | — | 583 / 23 / 2021-05-23 | Apache-2.0 | dead; does not compile |

Four repository moves in 2025–2026 matter for citations: sqlx to `transact-rs/sqlx`,
rust-postgres to `rust-postgres/rust-postgres`, tiberius to `tiberius-rs/tiberius`, and
diesel_async to `diesel-rs/diesel_async`; the old GitHub API paths redirect.

### The sqlx `Any` driver, the only runtime-erased common API in wide use

sqlx's `Any` driver is the closest living relative of what rdbc tried to be, so its limits are
load-bearing **[vendor, checked 2026-11-09]** (https://docs.rs/sqlx/latest/sqlx/any/index.html,
https://docs.rs/sqlx/latest/sqlx/any/enum.AnyTypeInfoKind.html): "The underlying database drivers
are chosen at runtime from the list set via `install_drivers`"; "Any use of `AnyConnection` or
`AnyPool` without this will panic"; the driver is selected "from the connection url"; `AnyKind`
is deprecated. `AnyTypeInfoKind` has exactly nine variants: `Null, Bool, SmallInt, Integer,
BigInt, Real, Double, Text, Blob`. The SQLite adapter returns a driver error for unmapped types;
sea-query's Any binding panics for IpNetwork/MacAddress/array/vector arguments **[third party,
via search]** (https://docs.rs/sqlx-sqlite/0.8.6/src/sqlx_sqlite/any.rs.html,
https://docs.rs/sea-query-sqlx/latest/src/sea_query_sqlx/sqlx_any.rs.html). 0.9.0 still
changed Any semantics ("breaking(any+mysql): correctly convert text and blob types to
`AnyTypeInfo`", #4255). In 2021 abonander acknowledged "the boxed trait objects in use were a
known inefficiency" pending async-fn-in-traits (https://github.com/launchbadge/sqlx/issues/1481).
0.9.0 also made all `query*()` functions take `impl SqlSafeStr` (only `&'static str` qualifies
by default; dynamic SQL needs `AssertSqlSafe`) (https://github.com/transact-rs/sqlx/discussions/4271).

### Measured numbers

#### TechEmpower Round 23, Fortunes test, physical hardware

Round 23 was run on new, Microsoft-sponsored hardware: HPE ProLiant DL360 Gen10 Plus, Intel Xeon
Gold 6330 @ 2.00 GHz (56 cores), 64 GB RAM, Mellanox ConnectX-6 40 Gbps, Ubuntu 24.04; TechEmpower
says network-bound results rose about 3x against Round 22, so **Round 23 numbers are not
comparable with earlier rounds** **[third party, checked 2026-11-09]**
(https://www.techempower.com/blog/2025/03/17/framework-benchmarks-round-23/,
https://github.com/TechEmpower/FrameworkBenchmarks/releases). The database image is
`postgres:18-trixie` as of 2026-11-09
(https://raw.githubusercontent.com/TechEmpower/FrameworkBenchmarks/master/toolset/databases/postgres/postgres.dockerfile).
The publication date is given three ways in the notes: 2025-02-24 (a third-party blog's reading),
2025-03-03 (the TFB issue announcing publication, https://github.com/TechEmpower/FrameworkBenchmarks/issues/9589)
and 2025-03-17 (the TechEmpower blog post). All three are kept. Actix "failed to build during the
Round 23 run" (toolchain pinned to Rust 1.58) and is absent. The FrameworkBenchmarks repository
"was archived by its owner on March 24, 2026, and is now read-only" and TechEmpower has announced
it is sunsetting the project (https://news.ycombinator.com/item?id=47497763,
https://github.com/TechEmpower/FrameworkBenchmarks/issues/10932) **[third party]**. Round 23 is
therefore the last round.

Rust rows, read from the rendered results page with a headless browser; the client behind each
entry is from the `master` `Cargo.toml` **[third party, checked 2026-11-09]**
(https://www.techempower.com/benchmarks/#section=data-r23&hw=ph&test=fortune):

| Rank | Entry | req/s | % of best | Database client |
|---:|---|---:|---:|---|
| 1 | may-minihttp | 1,327,378 | 100.0 | `may_postgres` (git rev 917ed78), coroutine-based |
| 3 | ntex [sailfish] | 1,210,348 | 91.2 | `tokio-postgres` from fork `fafhrd91/postgres` branch `ntex-3` |
| 4 | ntex [async-std,db] | 1,197,351 | 90.2 | same fork |
| 5 | xitca-web | 1,146,712 | 86.4 | `xitca-postgres` 0.5.0 |
| 6 | xitca-web [orm] | 1,115,124 | 84.0 | diesel + `xitca-postgres-diesel` |
| 7 | axum [postgresql] | 1,114,265 | 83.9 | `tokio-postgres` 0.7.13 (binary `axum-pg`) |
| 10 | hyper-db | 1,066,644 | 80.4 | `tokio-postgres` 0.7.13 + `deadpool-postgres` 0.14.1 |
| 11 | viz [postgresql] | 1,060,105 | 79.9 | Cargo.toml 404'd; name implies tokio-postgres |
| 16 | salvo [postgres] | 969,747 | 73.1 | `tokio-postgres` 0.7 |
| 107 | xitca-web [sync] | 374,093 | 28.2 | blocking API |
| 116 | axum [mongodb] | 335,954 | | mongodb |
| 132 | salvo [mongodb] | 310,057 | | mongodb |
| 192 | rocket | 220,494 | | |
| 350 | axum [postgresql - deadpool] | 67,449 | 5.1 | `deadpool-postgres` 0.14.1 |
| 406 | viz [postgresql - diesel] | 39,766 | 3.0 | diesel |
| 409 | axum [postgresql - sqlx] | 38,712 | 2.9 | `sqlx` 0.8.6 (`postgres, macros, runtime-tokio, tls-rustls`) |
| 418 | salvo [diesel] | 34,005 | | diesel |
| 422 | salvo [postgres-sqlx] | 32,289 | | sqlx 0.7.1 |
| 428 | ohkami | 31,256 | | |
| 442 | viz [postgresql - sqlx] | 28,075 | | sqlx |
| 448 | salvo [postgres-deadpool] | 26,782 | | deadpool-postgres |

Cargo manifests: https://raw.githubusercontent.com/TechEmpower/FrameworkBenchmarks/master/frameworks/Rust/{may-minihttp,ntex,xitca-web,axum,hyper,salvo,actix}/Cargo.toml.

The reading that matters: inside the same web frameworks (axum, salvo, viz), swapping the
database client moves Fortunes throughput 20–40x. Every entry above 1.0 M req/s uses
tokio-postgres or a pipelined derivative; the sqlx entries land at 28–39 k and the
deadpool-postgres entries at 27–67 k. An axum discussion (2025-03-13) asked "why are the SQLx and
Deadpool implementations that bad in comparison?" and received zero replies
(https://github.com/tokio-rs/axum/discussions/3266). In sqlx #2436 (2024) abonander called the
comparison unfair "because of per-core connections and pool overhead"
(https://github.com/launchbadge/sqlx/issues/2436) **[vendor]**. Both readings are compatible:
the fast entries use one pipelined connection per core and the slow ones a shared pool with one
request in flight per connection.

Two TFB rules bound what this measures: cross-request database pipelining has been banned since
Round 21 ("the execution behavior and semantics must match", issue #7019 via #7289), and the
Lithium maintainer reported Fortunes falling from ~670,000 to ~380,000 req/s after removing it,
while drogon, actix-http and just-js saw no change **[third party, checked 2026-11-09]**
(https://github.com/TechEmpower/FrameworkBenchmarks/issues/7289,
https://github.com/matt-42/lithium/discussions/116). The single-query ceiling is dominated by
HTTP and the database server; TFB measures driver overhead only indirectly.

#### TechEmpower continuous run on Citrine (2023-09-19, run de12400a)

The official Round 22/23 per-framework tables are rendered client-side and
tfb-status.techempower.com refused connections on 2026-11-09, so the second dataset is a
third-party aggregation (ajdust/tfbvis) of a continuous run on the older Citrine hardware: three
identical servers (app, database, load generator), Intel Xeon Gold 5120 @ 2.20 GHz, 28 HT cores,
32 GB RAM, switched 10 GbE **[third party, checked 2026-11-09]**
(https://github.com/ajdust/tfbvis, data under
`docs/Citrine_started2023-09-19_de12400a-863e-4f9e-9b93-dea9eb260422/`;
https://github.com/TechEmpower/FrameworkBenchmarks/wiki/Project-Information-Environment). Best
concurrency per entry; `mem` is dstat "used" memory on the app host in MB, not process RSS;
latency in ms; PostgreSQL unless noted. Rows are comparable with each other because they are from
one machine; they are **not** comparable with the Round 23 table above.

Single query (`db`, 527 entries):

| Rank | Entry | Client | req/s | p50 | p99 | mem MB | CPU % |
|---:|---|---|---:|---:|---:|---:|---:|
| 1 | ntex-db (Rust) | tokio-postgres fork | 622,917 | 0.79 | 1.60 | 2,742 | 99.6 |
| 2 | xitca-web (Rust) | xitca-postgres | 614,465 | 0.78 | 1.77 | 3,785 | 99.8 |
| 3 | may-minihttp (Rust) | may_postgres | 613,426 | 0.79 | 15.57 | 2,347 | 89.8 |
| 4 | h2o (C) | custom libpq-style | 612,899 | 0.79 | 1.55 | 2,089 | 99.0 |
| 6 | vertx-postgres (Java) | vertx-pg-client | 572,712 | 0.87 | 1.34 | 4,465 | 98.5 |
| 7 | vertx-web-postgres (Java) | vertx-pg-client | 561,594 | 0.86 | 1.78 | 3,861 | 98.4 |
| 9 | axum-pg (Rust) | tokio-postgres, connection per core | 542,732 | 0.90 | 2.66 | 1,353 | 99.9 |
| 10 | salvo-pg (Rust) | tokio-postgres | 541,612 | 0.92 | 2.01 | 3,886 | 99.4 |
| 14 | jooby-pgclient (Java) | vertx-pg-client | 512,328 | 0.88 | 4.16 | 2,577 | 95.3 |
| 24 | actix-http (Rust) | tokio-postgres | 427,147 | 1.13 | 1.76 | 5,415 | 61.5 |
| 26 | aspcore-ado-pg (.NET) | Npgsql | 388,807 | 1.24 | 5.70 | 5,919 | 96.5 |
| 58 | quarkus-hibernate-reactive (Java) | vertx-pg-client | 319,048 | 0.71 | 2.36 | 3,956 | 90.6 |
| 68 | quarkus (Java, JDBC/Hibernate) | pgjdbc + Agroal | 306,249 | 1.45 | 12.11 | 3,861 | 95.8 |
| 129 | micronaut-jdbc (Java) | pgjdbc + Hikari | 222,484 | 2.05 | 11.01 | 3,233 | 98.4 |
| 142 | jooby (Java, JDBC) | pgjdbc + Hikari | 200,625 | 2.82 | 9.51 | 2,163 | 64.7 |
| 151 | actix-web-diesel (Rust) | diesel (sync, thread pool) | 193,157 | 3.13 | 3.54 | 5,428 | 85.8 |
| 192 | spring (Java) | pgjdbc + Hikari | 145,311 | 2.45 | 26.88 | 4,277 | 96.2 |
| 233 | spring-jpa (Java) | pgjdbc + Hibernate | 103,683 | 3.32 | 30.25 | 4,231 | 95.2 |
| 253 | actix-web-pg-deadpool (Rust) | deadpool-postgres | 93,902 (conn 64) | 0.57 | 0.86 | 5,363 | 45.4 |
| 265 | axum-pg-pool (Rust) | deadpool-postgres | 88,255 (conn 64) | 0.62 | 0.81 | 1,165 | 40.9 |
| 308 | salvo-sqlx (Rust) | sqlx | 73,746 (conn 128) | 1.50 | 2.04 | 3,028 | 53.4 |
| 339 | micronaut-r2dbc (Java) | r2dbc-postgresql | 61,734 (conn 64) | 0.86 | 1.77 | 2,862 | 41.3 |
| 396 | axum-sqlx (Rust) | sqlx | 39,467 | 12.67 | 15.56 | 1,150 | 41.4 |

Fortunes (484 entries), req/s: may-minihttp 581,366; xitca-web 575,493; ntex-db 554,452; h2o
543,873; vertx-postgres 454,920; axum-pg 435,504; jooby-pgclient 424,886; salvo-pg 411,687;
actix-http 400,083; vertx-web-postgres 387,483; aspcore-ado-pg 362,033; quarkus 216,934;
quarkus-hibernate-reactive 188,489; jooby (JDBC) 180,677; micronaut-jdbc 178,575;
actix-web-diesel 170,813; salvo-sqlx 98,149; actix-web-pg-deadpool 94,151; axum-pg-pool 82,974;
axum-sqlx 44,112; spring 23,761 (conn 32); spring-jpa 22,260.

Multiple queries (20 queries per request, 512 connections, 509 entries), req/s: axum-pg 32,369;
salvo-pg 32,335; ntex-db 32,271; xitca-web 32,254; may-minihttp 32,152; vertx-postgres 32,016;
jooby-pgclient 31,839; h2o 31,261; vertx-web-postgres 30,237; axum-pg-pool 26,022; aspcore-ado-pg
25,259; actix-http 22,386; actix-web-diesel 18,184; jooby 17,483; quarkus 17,380; spring 15,811;
micronaut-jdbc 15,183; quarkus-hibernate-reactive 15,053; spring-jpa 8,140;
actix-web-pg-deadpool 3,497.

Updates (20 per request, 442 entries), req/s: xitca-web 22,942; may-minihttp 22,772; axum-pg
22,764; salvo-pg 22,704; ntex-db 22,659; h2o 19,465; vertx-postgres 16,917; vertx-web-postgres
16,366; jooby-pgclient 15,818; actix-http 15,512; jooby 13,026; axum-pg-pool 10,741;
micronaut-jdbc 10,374; spring 7,099; quarkus 6,611; quarkus-hibernate-reactive 6,582 (p99
1,080 ms); actix-web-diesel 4,437; actix-web-pg-deadpool 3,213; spring-jpa 3,148.

Within Rust on this machine, the choice of client and its pooling model moves single-query
throughput by 15x (axum-pg 542,732 vs axum-pg-pool 88,255 vs axum-sqlx 39,467). The entries at
equal throughput also differ 60–100% in CPU, which is why section 3 argues for CPU-seconds per
million rows as a metric. Note the deadpool and sqlx entries peak at 64–128 connections with
CPU at 41–53%: they are latency-bound on round trips, not CPU-bound.

#### diesel's continuous benchmark

`diesel-rs/metrics` "contains the numbers collected by diesel continuous scheduled benchmark
actions to track changes over time"; groups are trivial queries, medium complex queries, loading
associations sequentially and inserts; databases PostgreSQL, SQLite, MySQL; raw Criterion
aggregates live in `aggregated_data/{postgres,sqlite,mysql}.csv` on the `results` branch; 2,255
commits; **no hardware description** (GitHub Actions runners). The CSV schema is `group,
Benchmark (crate), value (row count), timestamp, backend, commit, mean_time, max_time, min_time`
with the first rows dated 2021-05-05 and the latest 2026-11-06 **[vendor, checked
2026-11-09]** (https://github.com/diesel-rs/metrics,
https://raw.githubusercontent.com/diesel-rs/metrics/results/aggregated_data/postgres.csv). The
diesel maintainers run it, so crate ordering is vendor-run; only within-suite ratios are
meaningful.

PostgreSQL, mean of the seven nightly runs 2026-10-31 to 2026-11-06, trivial query
(`SELECT ... FROM users` returning N rows), µs per query:

| Crate / API | N=1 | N=100 | N=1,000 | N=10,000 |
|---|---:|---:|---:|---:|
| zero_postgres (by_id) | 40.5 | 69.6 | 258.9 | 1,985 |
| postgres (sync, by_id) | 44.8 | 101.5 | 519.0 | 4,499 |
| wtx | 54.0 | 78.4 | 283.3 | 2,283 |
| zero_postgres_async | 55.4 | 86.6 | 307.2 | 2,351 |
| sqlx `query_as!` macro | 55.3 | 106.8 | 476.4 | 3,879 |
| sqlx `FromRow` | 55.6 | 112.4 | 535.3 | 4,423 |
| diesel (sync) | 57.3 | 86.1 | 347.6 | 2,951 |
| tokio_postgres (by_id) | 76.1 | 110.3 | 352.7 | 2,665 |
| diesel_async | 78.9 | 118.6 | 393.9 | 2,972 |
| sea-orm | 111.9 | 181.1 | 711.1 | 5,655 |

Medium complex query (join + filter), µs: N=1 zero_postgres 53.7, postgres 57.9, wtx 66.7, sqlx
71.3–71.6, diesel 74.3, tokio_postgres 90.5, diesel_async 96.8, sea-orm 149.8; N=10,000
zero_postgres 2,122, wtx 2,268, tokio_postgres 2,452, diesel_async 2,614, diesel 2,914, postgres
3,172, sqlx 3,340–4,028, sea-orm 5,793. Insert (N rows in one statement), µs: N=1 zero_postgres
159, wtx 176, sqlx 179, diesel 183, diesel_async 205, postgres 249, sea-orm 255, tokio_postgres
319; N=100 zero_postgres 468, wtx 493, sqlx 506, diesel 525, diesel_async 546, postgres 647,
tokio_postgres 715, sea-orm 732. Load associations sequentially, µs: diesel_async 4,819; diesel
4,949; zero_postgres 5,139; wtx 5,426; tokio_postgres 5,896; postgres 7,369; sqlx 7,956; sea-orm
9,455.

The rust-landscape note read only the head and tail of the same >10 MB CSV and recorded the
newest `zero_postgres_by_name` `bench_trivial_query` rows at 10,000 rows with mean_time between
1,165,887 and 2,769,610 (unit unlabelled; magnitudes consistent with nanoseconds, so 1.2–2.8 ms).
The jdbc-baseline note's seven-run average for the `by_id` variant is 1,985 µs. The two readings
are of different variants and different windows and are both kept.

MySQL backend, same window (`aggregated_data/mysql.csv`): trivial N=1 zero_mysql 61.3 µs, mysql
62.6, rustorm 67.7, diesel_async 72.5, sqlx 81.7–82.3, diesel 84.6, sea-orm 148.4; trivial
N=10,000 zero_mysql 4,526, mysql 4,667, diesel 5,928, sqlx 7,266–7,336, diesel_async 7,493,
sea-orm 7,923; 100-row insert zero_mysql 1,298, mysql 1,369, sqlx 1,394, diesel 1,398, sea-orm
1,767.

SQLite backend (`aggregated_data/sqlite.csv`), in-process, so this isolates binding overhead:
trivial N=1 rusqlite 0.37 µs, diesel 0.56 µs, diesel (queryable_by_name) 2.5 µs; medium complex
N=1 rusqlite 2.1 µs, diesel 2.5 µs, diesel_async 25.0 µs, sqlx 25.3 µs, sea-orm 53.2 µs; medium
complex N=10,000 rusqlite 1,768 µs, diesel 2,058 µs, diesel_async 4,299 µs, sea-orm 13,467 µs,
sqlx 20,284 µs; insert N=1 rusqlite 1.5 µs, diesel 1.9 µs, sqlx 22.9 µs, sea-orm 41.6 µs,
diesel_async 59.0 µs; insert N=100 rusqlite 29.1, diesel 36.4, sqlx 69.8, sea-orm 141.7,
diesel_async 2,030 µs.

diesel.rs's own summary of these results: "All async SQLite crates perform considerably worse
than their sync counterparts"; "For small output sizes there is almost no difference between most
of them"; for large outputs Diesel/diesel-async "perform relative well"; crates.io "reported a
20% performance increase" after adopting pipelining; and it recommends "perform your **own**
benchmarks" **[vendor, checked 2026-11-09]** (https://diesel.rs/compare_diesel.html).

Readings: on a single localhost connection the spread between fastest and slowest Rust
PostgreSQL client is ~2.5x at N=1 (40 vs 112 µs) and ~2.9x at N=10,000 (2.0 vs 5.7 ms). The
fastest async client (zero_postgres_async, 55 µs) is ~37% slower than the fastest sync one at
N=1; that is the async-runtime tax on a localhost round trip. sqlx falls 1.5–2x behind
tokio-postgres as result sets reach 1,000–10,000 rows. On SQLite the 20 µs floor under every
async wrapper is the background-thread-plus-channel design (sqlx #1481).

#### sqlx-specific measurements

- **Large result sets, sqlx #2007** (opened 2022-07-29, still open): 12,096,001 rows x 6 columns
  (TimescaleDB), `fetch_all`: sqlx 0.6 `query_as` 29,830 ms vs tokio-postgres 9,386 ms (13,028
  ms with mapping); bench repo 43.26 s vs 12.30 s; retest on sqlx 0.7 (2023-03-30) 26.24 s vs
  12.21 s (sea-orm on sqlx 0.6: 52.62 s). abonander found sqlx made ~100x the `read()` calls of
  tokio-postgres because a buffered-stream routine was called with very small byte counts after
  cancellation-safety refactors, and that `FromRow` does a hashmap lookup per column per row
  **[vendor, checked 2026-11-09]** (https://github.com/launchbadge/sqlx/issues/2007).
- **sqlx #2436** (2023-03-30): a reporter measured sqlx 0.6.3 "150x" slower than tokio-postgres
  on PostgreSQL 15.2; root cause was sqlx declaring a parameter as TEXT against a `char(24)`
  column, defeating the index (SeqScan vs IndexScan); with a single connection "a roughly 70x
  gap remained"; closed "not planned" 2023-04-07 **[third party]**
  (https://github.com/launchbadge/sqlx/issues/2436).
- **2021 dispute, sqlx #1481** (2021-10-08): "more than 60 times slower" than Diesel in some
  suite benchmarks; abonander said sqlx was "within 2-3x" elsewhere and that the SQLite spikes
  came from running SQLite on a background thread with an async channel; the suite was switched
  from async-std to tokio (diesel PR #2954, 2021-11) **[vendor and third party]**
  (https://github.com/launchbadge/sqlx/issues/1481).
- **Forum benchmark** (2024-05-25; sqlx 0.7.4, diesel 2.1.6, diesel-async 0.4.1, deadpool 200
  connections, 20,000 queries, no hardware or PG version): Sqlx-NS 2,518 ms, Sqlx-NT 4,474,
  Diesel-NS 3,894, Diesel-NT 5,688, Diesel-OS 2,045, Diesel-OT 4,291 (N raw SQL, O ORM DSL, S no
  transaction, T transaction); the diesel maintainer declined to review **[third party]**
  (https://users.rust-lang.org/t/diesel-vs-sqlx-my-benchmark/111982).
- **Remote anecdote** (2025): sqlx ~450 ms vs tokio-postgres ~300 ms on a trivial query to a
  remote Supabase database, attributed to extra round trips **[third party, anecdotal]**
  (https://axebps-redlib.hf.space/r/rust/comments/1jiuvb4/extremely_slow_sqlx_query_performance).

#### ConnectorX (bulk extraction)

The only peer-reviewed measurement in the field, and it covers bulk extraction, not OLTP. README:
loading "a 10x TPC-H lineitem table (8.6GB) from Postgres into a DataFrame, with 4 cores
parallelism", ConnectorX "uses up to 3x less memory and 21x less time" (vs the slowest
alternative) and "3x less memory and 13x less time compared with Pandas" **[vendor, checked
2026-11-09]** (https://raw.githubusercontent.com/sfu-db/connector-x/main/README.md). The paper
(Wang et al., PVLDB 15(11):2994–3003, 2022) measured pandas 12.46 min and 95.6 GB peak vs
ConnectorX 0.97 min and 31.4 GB on the 7.2 GB-CSV lineitem (60 M rows) between two AWS
r5.4xlarge instances; "the actual time spent on query execution is less than 1 min (13x time
overhead)" **[paper, checked 2026-11-09]** (https://www.vldb.org/pvldb/vol15/p2994-wang.pdf).
The project's Benchmark.md gives ratios vs pandas per database (client r5.4xlarge,
`partition_num=4`, TPC-H SF10 lineitem): PostgreSQL (db.m6g.4xlarge RDS) 13x time / 3x memory;
MySQL 8x / 3x; SQLite 5x / 2x; Oracle (db.r5.4xlarge) 3x / 3x; MSSQL (r5.4xlarge Docker) 14x /
3x **[vendor]** (https://github.com/sfu-db/connector-x/blob/main/Benchmark.md). Section 6.2 has
the paper's mechanism.

#### Compile-time measurements

- weiznich (2021-09-27), minimal one-table example: from scratch Diesel 21.5 s vs SeaORM 94 s;
  no-op rebuild 0.34 s vs 0.87 s; add one query 0.48 s vs 1.79 s; SeaQL removed its compile-time
  claim in response **[third party]** (https://github.com/SeaQL/sea-orm/issues/203).
- sqlx #4070 (2025-10-24): making `serde`/offline optional cut a minimal
  `runtime-smol,postgres,migrate` build from 21 s to 16 s; fixed by PR #4077 (2026-02-06)
  **[third party and vendor]** (https://github.com/launchbadge/sqlx/issues/4070).
- wtx self-reports for its "Postgres Client" build row: 13 required deps, 26 all deps, 5.12 s
  clean check, 6.19 s clean debug build, 6.69 s clean opt build, **652 K opt size**; it publishes
  no cross-driver query benchmarks **[vendor]** (https://raw.githubusercontent.com/c410-f3r/wtx/main/README.md).
- Kobzol (2026-06-21): a project with ~350 `#[sqlx::test]` tests and 30 migrations took 7.5–10 s
  to rebuild after a no-op change because the macro inlined all migrations into every test
  (32 MiB of `cargo expand` output); one migration cut it to ~5 s and 6 MiB **[third party]**
  (https://kobzol.github.io/rust/2026/06/21/optimizing-sqlx-test-rebuild-time.html).

### Lessons from the designs

1. **The TechEmpower gap is an architecture gap, not a language gap.** Both pipelining designs
   that exist (rust-postgres's split `Client`/`Connection` task and xitca's pooled pipelining)
   rely on a dedicated I/O task plus per-request response channels. sqlx's maintainers described
   exactly this design in September 2026 as the required path and said it "would require a
   breaking change to `Executor`" because moving only I/O to a background task "would not fix the
   desync issues while stateful `.await`s remain in the calling task" (PR #3891). The same
   refactor blocks sqlx's cancellation safety, CopyBoth (#2924), query cancellation and
   `PgListener` drop. A new core that bakes in the worker-per-connection model starts ahead of
   sqlx on this axis.
2. **A lowest-common-denominator scalar type set fails twice.** sqlx-Any's nine kinds and rdbc's
   typed getters are the same mistake; both trackers show users leaving for driver-specific
   crates. The common type model needs an escape hatch to driver-native types (section 4.6
   derives the union set).
3. **Pure-Rust wire implementations exist for every network protocol except Oracle.** PostgreSQL
   (`postgres-protocol`, 82 direct dependents), MySQL (`mysql_common`, 60 direct dependents) and
   TDS (tiberius, monolithic) are pure Rust under MIT OR Apache-2.0. SQLite and DuckDB are
   in-process C by nature; Oracle is ODPI-C; Snowflake is REST or Go behind FFI; ODBC wraps the
   platform driver manager.
4. **Columnar output exists only in the analytics layer** (arrow-odbc, ADBC, connectorx,
   duckdb-rs, datafusion-table-providers); every OLTP driver is row-at-a-time.
5. **Nothing normalises SQLSTATE or errors.** Each driver has its own error enum (sqlx
   `DatabaseError` trait, tokio-postgres `DbError` with `code()`, mysql `ServerError`, tiberius
   `Error::Server`). A common API has to do this itself (the per-driver error surfaces were not
   read from source; section 13).
6. **Decimal handling is fragile everywhere** because every driver delegates to `rust_decimal`
   (96-bit mantissa, max scale 28) or `bigdecimal`: sqlx #2688 "mismatched types" decoding
   DECIMAL; #2887 notes out-of-range values "may result in truncation and thus loss of
   precision"; rust_decimal PR #647 capped rescale so unbounded PostgreSQL NUMERIC does not reach
   scale 29; rust_decimal 1.36.0 fixed `Numeric::NaN` decoding as 0 **[via search]**. A
   wire-faithful decimal (digits + scale) with lossless conversion on request is the only design
   that avoids this.
7. **One facade crate with many optional drivers bloats every consumer's lockfile.** sqlx's pinned
   issue #3211 (2024-04-21) aggregates about a dozen reports; weak dependency features
   (`crate?/feature`) pull crates into `Cargo.lock`; blamed on rust-lang/cargo#10801, open since
   2022; example #3908: enabling `time` on sqlx 0.8.6 "added 31 packages" including
   `libsqlite3-sys`, `sqlx-mysql`, `sqlx-sqlite`, `rsa` with neither database enabled **[vendor,
   checked 2026-11-09]**. Per-driver crates behind a thin trait crate avoid it by construction.
8. **Pool fairness is undocumented** in sqlx and deadpool; only bb8 exposes Fifo/Lifo. deadpool's
   Fast recycling trades correctness for speed on unreliable networks.
9. **Governance is a dependency risk**: tiberius (org transfer, no named maintainers, empty
   Releases; a third-party scorer rated 0.11.8 "62/100" with maintenance 5/25, depscope.dev),
   rust-oracle (22 months without a release), the native ClickHouse client (dormant since 2024).
10. **Zero-copy rows cost ergonomics**: xitca-postgres's self-reported weakness is that "public
    type parameters expose lifetimes, which makes it hard to return these types from
    functions".
11. **The async tax on localhost is measurable** (~37% at N=1 on diesel's suite) and the sync
    SQLite path is 20–60x faster than any async wrapper; a common API needs a blocking path that
    does not go through a channel.
12. **Multiple prepared statements per connection** (rdbc #46, #53) is an ownership problem the
    old API could not express (`Box<dyn Statement + '_>` borrows the connection). The notes did
    not read how tokio-postgres or xitca-postgres own statements; section 4 gives the ADBC and
    R2DBC statement models as the standards' answer.

## 3. The baseline to beat: JDBC and the fastest non-Java clients

### Driver versions and sizes

Read from Maven Central `maven-metadata.xml` and HTTP HEAD on the JAR URL **[third party,
checked 2026-11-09]** (https://repo1.maven.org/maven2/org/postgresql/postgresql/maven-metadata.xml
and sibling paths).

| Artifact | Latest release (lastUpdated) | Main JAR size |
|---|---|---:|
| org.postgresql:postgresql (pgJDBC) | 42.7.14 (2026-10-07) | 1,221,004 B (1.19 MiB) |
| com.mysql:mysql-connector-j | 26.10.0 (2026-10-20) | 2,618,793 B (2.56 MiB) |
| org.mariadb.jdbc:mariadb-java-client | 4.0.0 (2026-10-21), Java 17+ | 817,345 B (0.80 MiB) |
| com.microsoft.sqlserver:mssql-jdbc | 14.1.0.jre11-preview (2026-11-02); latest GA 13.6.2 | 1,583,785 B (preview); 13.4.0.jre11 = 1,547,706 B |
| com.oracle.database.jdbc:ojdbc11 / ojdbc17 | 23.26.4.0.0 (2026-10-29) | 8,828,929 B / 8,830,049 B (8.6 MiB) |
| org.xerial:sqlite-jdbc | 3.54.1.0 (2026-10-29) | 12,132,578 B (11.8 MiB; bundles native SQLite for all platforms) |
| org.duckdb:duckdb_jdbc | 2.0.0.1 (2026-10-31) | 80,297,452 B (78.4 MiB; bundles the engine) |
| com.zaxxer:HikariCP | 7.1.0 (2026-06-14), Java 11+ | 172,996 B |
| io.agroal:agroal-pool | 3.2.1 (2026-06-26) | 150,830 B |
| org.apache.commons:commons-dbcp2 | 2.14.0 (2025-12-16) | 222,783 B |
| com.mchange:c3p0 | 0.15.0 (2026-11-08) | 565,005 B |
| io.vertx:vertx-pg-client | 5.2.3 (2026-11-03) | 184,597 B |
| org.postgresql:r2dbc-postgresql | 1.1.3.RELEASE (2026-09-21) | 658,084 B |

A size comparison must be stated per artifact plus transitive dependencies plus the JVM: pgJDBC
itself is 1.2 MiB and HikariCP 0.17 MiB; the JVM runtime image dominates. DuckDB JDBC and
sqlite-jdbc are large only because they ship the engine.

### Their defaults

Every major driver still ships defaults that cost performance; a fair benchmark has to tune
them, and "native JDBC as shipped" is a weaker opponent than "JDBC tuned". All **[vendor, checked
2026-11-09]**.

- **pgJDBC** (https://jdbc.postgresql.org/documentation/use/): `prepareThreshold=5` (a
  PreparedStatement becomes a named server-side statement only on its fifth execution; `-1`
  prepares immediately); `binaryTransfer=true` but it applies only once the statement is
  server-prepared, so the first four executions of every statement decode text;
  `defaultRowFetchSize=0` ("fetches all rows in one trip", buffered in heap);
  `preferQueryMode=extended`; `reWriteBatchedInserts=false` (docs claim "2-3x performance
  improvement" on; merged rows capped at `min(65535 / parametersPerRow, 32768)`);
  `preparedStatementCacheQueries=256`, `preparedStatementCacheSizeMiB=5`;
  `maxSendBufferSize=8192`; `tcpNoDelay=true`; `tcpKeepAlive=false`; `sslNegotiation=postgres`;
  `channelBinding=prefer`; `gssEncMode=allow`; `adaptiveFetch=false`. 42.6.0 (2023-03-17)
  replaced `synchronized` with `ReentrantLock` "(loom)" (PR #2635)
  (https://jdbc.postgresql.org/changelogs/2023-03-17-42.6.0-release/).
- **MySQL Connector/J 26.10**
  (https://dev.mysql.com/doc/connector-j/en/connector-j-connp-props-performance-extensions.html,
  https://dev.mysql.com/doc/connector-j/en/connector-j-connp-props-prepared-statements.html):
  `useServerPrepStmts=false` (client-side emulated prepared statements over the text protocol);
  `cachePrepStmts=false`, `prepStmtCacheSize=25`, `prepStmtCacheSqlLimit=256`;
  `rewriteBatchedStatements=false`; `useCursorFetch=false`; `defaultFetchSize=0`;
  `useReadAheadInput=true`; `cacheResultSetMetadata=false`; `maintainTimeStats=true` (two
  `System.currentTimeMillis()` calls per query); `useLocalSessionState=false`;
  `elideSetAutoCommits=false`.
- **MariaDB Connector/J 4.0.0** (https://mariadb.com/docs/connectors/mariadb-connector-j/about-mariadb-connector-j,
  https://mariadb.com/resources/blog/whats-new-in-mariadb-connector-j-4-0.md): Java 17+;
  `useServerPrepStmts=false` (text; the docs say binary can cost "up to about 50% on cache
  misses" and that metadata skipping on MariaDB 10.6+ gains "10-30%"); `cachePrepStmts=true`,
  `prepStmtCacheSize=250`; `useBulkStmts=false` (COM_STMT_BULK_EXECUTE), `useBulkStmtsForInserts=true`
  in 3.x; `rewriteBatchedStatements=false` (ignored in 3.0.0–3.5.5, reintroduced 3.5.6);
  `defaultFetchSize=0` (`Integer.MIN_VALUE` streams in 4.0); `allowLocalInfile=true`.
  Connector/J 3.0 (2022-01-25) added "Optimistic Prepare" (PREPARE and EXECUTE sent together);
  the 2.x `useBatchMultiSend` was removed in 3.0 because it was incompatible with Aurora, and
  Aurora failover was dropped because the driver "relies on pipelining"
  (https://mariadb.com/resources/blog/whats-new-in-mariadb-connector-j-3-0/). 4.0 removed
  `synchronized` monitors around blocking network calls so virtual threads park instead of pinning.
- **mssql-jdbc** (https://learn.microsoft.com/en-us/sql/connect/jdbc/prepared-statement-metadata-caching-for-the-jdbc-driver):
  since 6.2 the first execution of a PreparedStatement calls `sp_executesql`; only the second
  calls `sp_prepare`/`sp_prepexec` (`enablePrepareOnFirstPreparedStatementCall=false`);
  `sp_unprepare` calls are batched up to `serverPreparedStatementDiscardThreshold=10`; handle
  caching off by default (`statementPoolingCacheSize` < 1); `useBulkCopyForBatchInsert` (9.2+)
  routes `executeBatch()` through Bulk Copy.
- **Oracle JDBC 23** (https://docs.oracle.com/en/database/oracle/oracle-database/23/jjdbc/performance-extensions.html):
  default row prefetch 10; the Thin driver fetches the first prefetch batch in the first round
  trip (11g+); prefetch drops to 1 when a LONG/LOB streaming column is present; Oracle-style
  update batching is a no-op since 12.2.
- **HikariCP** README: 7.1.0, Java 11+; pool-sizing wiki: start from
  `connections = (core_count * 2) + effective_spindle_count`; the Oracle Real-World Performance
  demo cut response time from ~100 ms to ~2 ms ("over 50x") by reducing the pool from 2048 to 96
  connections (https://github.com/brettwooldridge/HikariCP/wiki/About-Pool-Sizing; the video
  itself was not watched).
- **Reactive Java**: vertx-pg-client 5.2.3 implements the PostgreSQL protocol directly with
  pipelining; the TFB `vertx-web` entry uses Vert.x 5.0.5, Java 25, Netty io_uring transport and
  no JDBC driver (https://raw.githubusercontent.com/TechEmpower/FrameworkBenchmarks/master/frameworks/Java/vertx-web/pom.xml).
  r2dbc-postgresql maintainer mp911de (2023-08-14): "You should rather benchmark your own
  application"; "If you're doing reactive just because of non-blocking, then you're doing it
  wrong" (https://github.com/pgjdbc/r2dbc-postgresql/discussions/607).

### MariaDB, HikariCP and R2DBC measurements

**MariaDB Corporation's benchmark series (Diego Dupin)** **[vendor, checked 2026-11-09]**. These
are vendor-run against a competitor's driver; they are the most detailed JDBC-vs-JDBC numbers
that exist.

- 2016-10-06 (updated 2019-08-26), JMH, one DigitalOcean droplet (1 CPU, 512 MB, Java
  1.8.0_101), MariaDB 10.1.17 / MySQL 5.7.15, MariaDB Connector/J 1.5.3 vs MySQL Connector/J
  5.1.39 vs Drizzle 1.4. 1-row SELECT, text protocol: 62.7 vs 88.7 vs 78.7 µs/op; binary cache
  hit 58.3 vs 73.8; binary cache miss 118.9 vs 150.7; SELECT 1000 rows 244.2 vs 298.8 vs 406.9
  µs/op; 100-row batch insert local: MariaDB rewrite 0.404 ms/op, binary 5.29, text 6.08; MySQL
  rewrite 0.592, binary 9.015, text 7.932 ms/op; the same batch to a "distant" host (0.35–0.45 ms
  ping): MariaDB binary 7.64 ms/op vs MySQL binary 43.6 ms/op because MySQL Connector/J waited
  for each INSERT's acknowledgement while MariaDB's `useBatchMultiSend` pipelined
  (https://mariadb.com/resources/blog/mariadb-java-connector-driver-performance/).
- rusher/mariadb-java-driver-benchmark (JMH, 20 forks, 10 warm-up + 10 measurement iterations of
  1 s; MariaDB 10.3.10 local; MySQL Connector/J 8.0.21 vs MariaDB Connector/J 2.7.0 vs Drizzle
  1.4; undated): Do_1 45.9 vs 39.1 vs 41.8 µs/op; Select_1 74.7 vs 57.1 vs 61.5; 46-column row
  201.2 vs 167.2 vs 224.1; 10 columns x 100,000 rows 1,396 vs 1,031 vs 1,711 ms/op;
  Create_and_close_Connection 4.81 vs 2.59 vs 1.77 ms/op
  (https://github.com/rusher/mariadb-java-driver-benchmark).
- 2022-02-07, MariaDB Connector/J 3.0.3 vs MySQL Connector/J 8.0.27, MariaDB Server 10.6, Java
  17, BenchBase TPC-C (scale 128, 128 terminals, 600 s, SERIALIZABLE, 64 GB server,
  `innodb_buffer_pool_size=50G`) and JMH `select * from seq_1_to_X` (threads = CPU count,
  `sslMode=DISABLED`). TPC-C steady state read from chart images: ~3,050–3,150 ops/s (MariaDB
  binary), 2,700–2,800 (MariaDB text), 2,300 (MySQL text), 1,900 (MySQL binary). JMH throughput
  (ops/s) for 1/10/100/1,000/10,000 rows: text MySQL 23,284 / 21,922 / 13,979 / 4,535 / 481;
  text MariaDB 29,358 / 27,983 / 20,220 / 6,560 / 1,111; binary MySQL 13,761 / 13,281 / 10,025 /
  3,722 / 553; binary MariaDB 32,434 / 30,598 / 21,342 / 5,973 / 997. MySQL Connector/J was
  slower with server-side prepared statements than without; MariaDB was faster with them
  (https://mariadb.com/resources/blog/how-to-benchmark-mariadb-mysql-using-java-connector/).
- 2026-10-23, Connector/J 4.0.0 vs 3.5.10 on the connector's own suite (hardware not stated):
  0–15% faster overall; +14.5% on 100-row batch insert with server-side prepared statements;
  +6–9% on 1,000-row result sets; +6% on 100-column wide rows; no change on `SELECT 1`; Unix
  sockets via the JDK path instead of JNA: ~2.5 µs less per round trip and 1.5x faster connects;
  a 15 MiB LONGBLOB streamed in 8–10 ms vs 12–15 ms, and the default fetch throws
  OutOfMemoryError with a 12 MiB heap.
- CONJ-1015 (2022-09-23, fixed 3.1.0): Java pipelining was "about 10% slower" than Connector/C
  against a distant server because it wrote to the socket several times; writing the packets as
  one byte array fixed it (https://jira.mariadb.org/browse/CONJ-1015).
- 2013-05-06 (historic): single thread, Windows Server 2008 R2, `SELECT 1` unprepared: MySQL
  Connector/J 5.1.24 17,004 QPS, MariaDB JDBC 1.1.2 19,305, Drizzle 13,410; the native C client
  24,063 over TCP and 33,122 over a named pipe; Connector/J spent ~7.5% of runtime in
  `findStartOfStatement()` scanning SQL text (https://mariadb.org/on-performance-of-jdbc-drivers/).
  The C-client-vs-Java gap was ~8.5% then.

**PostgreSQL JDBC batching** **[third party, checked 2026-11-09]**:

- Vlad Mihalcea (2018-04-11), Hibernate batch size 100, 5,000 rows: median batch-insert timer
  83.99 ms with `reWriteBatchedInserts=false` vs 48.78 ms with it on (p99 161.7 vs 83.0 ms)
  (https://vladmihalcea.com/postgresql-multi-row-insert-rewritebatchedinserts-property/).
- sqg.dev (2026-03-26), Java 25 (Temurin), PostgreSQL 18 in Testcontainers, 24-CPU Linux box
  (CPU model not stated), single connection, median of 5 runs, 1 M rows: individual INSERT
  39,225 rows/s; `addBatch/executeBatch` 161,990; batch + `reWriteBatchedInserts=true` 292,091;
  hand-written multi-VALUES 252,504; COPY CSV 360,420; **COPY BINARY via PgBulkInsert 712,418
  rows/s** (https://sqg.dev/blog/java-postgres-insert-benchmark/).
- Mihalcea (2020-12-16), MySQL 8.0.22: client-side prepared statements beat server-side ones in
  all tests; recommends `useServerPrepStmts=false&cachePrepStmts=true` (numbers only in charts)
  (https://vladmihalcea.com/mysql-jdbc-statement-caching/). Vendor and third-party results
  therefore disagree on `useServerPrepStmts` for MySQL; a benchmark must run both.

**HikariCP JMH** **[vendor, checked 2026-11-09]** (chart `HikariCP-bench-2.6.0.png`, Intel Core
i7-3770 @ 3.40 GHz, Java 8u111, HikariCP 2.6.0, commons-dbcp2 2.1.1, Tomcat 8.0.24, Vibur 16.1,
c3p0 0.9.5.2; 32 threads/32 connections unconstrained, 32/16 constrained): connection cycle
(getConnection + close) ops/ms, constrained / unconstrained: HikariCP 47,609 / 50,273; Vibur
5,651 / 5,960; Tomcat 1,025 / 2,222; dbcp2 1,303 / 1,440; c3p0 234 / 248. Statement cycle
(prepareStatement + execute + close) ops/ms: HikariCP 146,667; Tomcat 85,800; Vibur 17,944; c3p0
16,859; dbcp2 122 (dbcp2 failed the statement benchmark "due to excessive garbage collection
times", via a mirrored README). Arithmetic: 50,273 ops/ms is about 20 ns per
getConnection/close pair on that machine; the next-best pool is ~8.4x slower
(https://raw.githubusercontent.com/wiki/brettwooldridge/HikariCP/HikariCP-bench-2.6.0.png,
https://github.com/brettwooldridge/HikariCP-benchmark).

**R2DBC vs JDBC**:

- r2dbc-postgresql PR #158 JMH `StagedResultSizeBenchmarks` (2019-09, ops/s, result sizes
  1/10/100/200): `extendedJdbc` 23,413 / 20,850 / 7,914 / 3,877; `simpleJdbc` 13,306 / 10,699 /
  8,129 / 5,909; `extendedR2dbc` 10,296 / 8,835 / 4,417 / 3,288; `simpleR2dbc` 9,381 / 9,446 /
  5,141 / 3,403. pgJDBC in extended mode was ~2.3x faster than R2DBC on a 1-row query **[vendor,
  checked 2026-11-09]** (https://github.com/pgjdbc/r2dbc-postgresql/pull/158).
- mp911de (2019-10-15): PGJDBC ~15,000 q/s vs R2DBC ~11,000 q/s against localhost; ~1,360 vs
  ~1,355 q/s when PostgreSQL ran in Docker (latency hides the gap); Mono/Flux allocation adds GC
  pressure costing ~25% on small results; a bare-bones Netty client reached only ~60% of PGJDBC's
  SELECT throughput; "You don't buy into reactive programming because you want your queries to
  run faster" **[vendor]** (https://github.com/spring-projects/spring-data-r2dbc/issues/203).
- artemik (2023; app on i7-7700K, DB on i7-8550U/Windows, 500,000 single-row SELECTs, pool 200):
  standalone JDBC 18 s, R2DBC 18.3 s, Vert.x 18.1 s; under WebFlux R2DBC took 25.5 s with custom
  LoopResources and 41.5–51 s without; WebFlux + Vert.x 19 s **[third party]**
  (https://github.com/artemik/r2dbc-jdbc-vertx-issue-190).
- AMIS (2020, wrk): R2DBC+WebFlux won only above ~200 concurrent requests **[third party]**
  (https://technology.amis.nl/software-development/performance-and-tuning/spring-blocking-vs-non-blocking-r2dbc-vs-jdbc-and-webflux-vs-web-mvc/).

### Where JVM driver cost comes from

Profiling evidence from three vendors agrees that result-set materialisation and per-statement
string handling dominate once the network is not the bottleneck.

- pgJDBC stores result sets as `byte[][]` and cannot avoid full buffering without
  `setFetchSize()` or "a major resultset overhaul" (pgsql-jdbc thread,
  https://postgrespro.com/list/thread-id/1925276). A later commit wrapped `byte[][]` in a
  `Tuple` class; a 2020 heap analysis found Tuple objects "30% or so of the heap" and maintainer
  Vladimir Sitnikov replied "We might want to revise the addition of Tuple" **[vendor mailing
  list]** (https://postgrespro.com/list/id/CAB=Je-HjCjHxkCY3WW_WLy0XGwXqs_r5Db4daWLrdbYpT21wvA@mail.gmail.com).
  Historic profiling (2002): a `getXxx` call allocates a `byte[]`, then a `String`, then the boxed
  `Integer`/`BigDecimal` because "the lack of byte[]-to-object methods in the Java API forces a
  String intermediary"; a hand-written UTF-8 decoder sped iteration 1.5–2x
  (https://postgrespro.com/list/thread-id/1920747); a 2006 profile found hotspots in
  `org.postgresql.core.Encoding.decode` and "excessive new String creation" per row
  (https://postgresql.org/message-id/20060823195511.93081.qmail%40web32004.mail.mud.yahoo.com).
- Microsoft (2026-10-08, "Faster by design"): for large sparse result sets "network I/O was
  often not the main bottleneck"; cost came from allocations, temporary objects and repeated
  work materialising values from the TDS stream. Fixes: check the NBCROW null bitmap before
  initialising decoders (PR #2600), reuse decoder state instead of per-value decoder objects
  (PRs #2974, #2991), decode numeric/MONEY/temporal values primitive-first (PRs #2975, #2991),
  reusable buffers in `getString()` (PR #2991). No numbers published **[vendor, checked
  2026-11-09]** (https://devblogs.microsoft.com/java/faster-by-design-performance-engineering-in-the-microsoft-jdbc-driver-for-sql-server/).
- MariaDB (2013): Connector/J spent ~7.5% of a `SELECT 1` loop in `findStartOfStatement()`;
  Drizzle built a hex-dump string for `log.finest` on every send even with logging off (25–30%
  gain after guarding).
- R2DBC maintainers: heavy `Mono`/`Flux` creation adds GC pressure worth ~25% throughput on small
  result sets.
- **JDBC row layout vs Arrow, same engine both sides**: on an M3 MacBook Air (16 GB), DuckDB
  `SELECT *` on TPC-H SF1 `lineitem` (6,001,215 rows) took 16,315.58 ms via JDBC into a
  `CachedRowSet` using 5,196 MB of JVM heap, vs 1,261.18 ms and 139 MB heap + 1,000 MB Arrow
  allocator via ADBC (13x faster, ~5x less memory); DuckDB's JDBC Arrow extension was "still"
  slower than ADBC **[third party, but Columnar is the ADBC maintainers' company; treat as
  vendor-adjacent; checked 2026-11-09]** (https://columnar.tech/blog/adbc-java/). Arithmetic:
  5,196 MB / 6.0 M rows is ~866 bytes of heap per 16-column row in the JDBC path. This is the
  only modern per-row heap number found; it should be reproduced on PostgreSQL rather than
  cited as general.
- One practitioner's bulk read rate: ~730–755 k rows/s streaming 24 M rows from PostgreSQL with
  a high fetch size and autocommit off; ~504 k rows/s at fetch size 1 M (undated, hardware
  unknown) **[third party; weak]** (https://domui.atlassian.net/wiki/x/B4Bn).
- JVM boxing sizes: `java.lang.Integer` is 16 bytes on a 64-bit HotSpot JVM (8-byte mark word,
  4-byte compressed class pointer, 4-byte value) plus a 4-byte compressed reference per element,
  roughly 20 bytes per boxed int vs 4 bytes primitive; JDK 25 compact object headers reduce the
  header **[third party, search summaries; pages not opened]** (https://dzone.com/articles/whats-wrong-with-java-boxed-numbers,
  https://www.baeldung.com/java-size-of-object, https://javapro.io/2026/02/10/mastering-memory-efficiency-with-compact-object-headers-in-jdk-25/).
- Virtual threads: through JDK 21–23 a virtual thread blocking inside `synchronized` pinned its
  carrier; JEP 491 (JDK 24) fixes the monitor implementation
  (https://inside.java/2024/02/21/quality-heads-up/). pgJDBC moved to `ReentrantLock` in 42.6.0
  (2023); MariaDB Connector/J 4.0 (2026) removed blocking-under-monitor paths; Oracle's fix
  "first appears in version 21.1" of ojdbc and an Oracle engineer reported ojdbc11 21.9
  "considerably faster" than ojdbc8 19.19 under virtual threads
  (https://mail.openjdk.org/pipermail/loom-dev/2023-June/005823.html) **[vendor]**.

**Startup and RSS** **[third party, checked 2026-11-09]**: Gillius (2025-10; Apple M2 Pro 16 GB,
macOS 15.7, Oracle GraalVM 25+37.1, PostgreSQL 17.6, Flyway; Spring Boot = Spring Data
JPA/Hibernate + HikariCP + Tomcat; Micronaut = Micronaut Data JDBC + HikariCP + Netty; Quarkus =
Panache/Hibernate + Agroal + Vert.x). Framework-reported startup, mean of last 3 of 4 runs:
Spring Boot JVM 1.909 s / native 0.104 s; Quarkus JVM 1.154 s / native 0.049 s; Micronaut JVM
0.656 s / native 0.050 s. Max RSS (units unlabelled, almost certainly MB): Spring Boot 388.9 /
149.4; Quarkus 271.2 / 70.5; Micronaut 253.2 / 83.8
(https://gillius.org/blog/2025/10/java-25-framework-startup.html). Quarkus's 2020 QCon slides
(vendor): REST app RSS 13 MB native vs 74 MB OpenJDK; "JPA and DB operation" startup 0.055 s
native vs 2.5 s OpenJDK. Quarkus's measurement guide: report time-to-first-request, measure RSS
with `ps -o pid,rss` or `pmap -x` (never `docker stats`), pin `-XX:ActiveProcessorCount=4
-Xms512m -Xmx512m`, use `taskset`; its reference harness uses Hyperfoil's wrk2 bindings "to avoid
coordinated omission" and pins app to cores 0–3, DB to 4–6, load generator to 7–9 **[vendor]**
(https://quarkus.io/guides/performance-measure,
https://github.com/quarkusio/spring-quarkus-perf-comparison). HelloWorld JVM startup fell from
~120 ms (JDK 9) to under 40 ms (JDK 14) (https://mail.openjdk.org/pipermail/discuss/2020-June/005514.html).

The cost model a Rust competitor has to beat is therefore: per column a `byte[]` slice, a
`String` (for text protocol or types without binary decoders) and a boxed object; per row a
Tuple/array; per result set full buffering in heap unless the application sets fetch size; and
a process that starts in 0.7–1.9 s with 250–390 MB RSS in JVM mode or 50–104 ms with 70–150 MB
as a GraalVM native image. The native-image numbers are already in the range a Rust binary
targets, so a size and startup claim must compare against both.

### The asyncpg / pgx / psycopg3 ceilings

These are the fastest protocol-native clients on PostgreSQL. Each table is one machine.

**asyncpg benchmark suite, 2023-06-22 rerun** (AMD Ryzen Threadripper 3970X, Linux 6.3.7,
concurrency 10, 10 s per query, each client in one process; PostgreSQL and driver versions not
recorded) **[vendor: run by the asyncpg authors; checked 2026-11-09]**
(https://gist.github.com/0ed296e93523831ea0918d42dd1258c2, https://github.com/MagicStack/asyncpg):

| Query | asyncpg | pgx (Go) | psycopg3 async | aiopg (psycopg2) | node-pg |
|---|---:|---:|---:|---:|---:|
| `pg_type` (~613 wide rows), rows/s | 3.07 M (5,039 q/s, 1.98 ms) | 1.18 M (1,939 q/s, 5.15 ms) | 897 K (1,473 q/s, 6.79 ms) | 273 K (449 q/s, 22.27 ms) | 464 K (762 q/s, 13.10 ms) |
| `generate_series` (1,000 ints), rows/s | 7.71 M (7,711 q/s, 1.30 ms) | 6.72 M (6,720 q/s, 1.48 ms) | 2.55 M (2,550 q/s, 3.92 ms) | 1.16 M (1,163 q/s) | 3.18 M (3,184 q/s) |
| `large_object` (100 x 1 KB bytea), rows/s | 1.99 M (19,923 q/s, 0.50 ms) | 1.41 M | 282 K | 332 K | — |
| `arrays` (100 rows x 100 ints), rows/s | 705 K | 152 K | 231 K | 212 K | 54 K |
| `copyfrom`, rows/s | 3.12 M | 2.49 M | 1.08 M | 557 K | 204 K |
| `batch` inserts, rows/s | 900 K | 890 K | 37 K | 179 K | — |
| Geometric mean rows/s | 2.13 M | 1.25 M | 425 K | 361 K | 258 K |

The README's "on average 5x faster than psycopg3" is this geometric mean. The 2016 launch post
(bare-metal Xeon E5-1620 v2 @ 3.70 GHz, PostgreSQL 9.5.2, single-threaded, GOMAXPROCS=1) reached
1 M rows/s on the wide-row query and was "at least 3x faster than psycopg2 (or aiopg)"; it was
slower than Go lib/pq on the integer-array query "because of the cost of creating and freeing
Python tuples". Its explanation of the speed: binary protocol ("psycopg2 exchanges data in text
format, which requires non-trivial parsing"), prepared statements with cached per-statement
decode pipelines, Cython with careful buffer management, direct protocol implementation (no
libpq) **[vendor, checked 2026-11-09]** (https://www.geldata.com/blog/m-rows-s-from-postgres-to-python).

**Timescale/TigerData, 2024-02-28** (AWS t2.xlarge 4 vCPU 16 GB, PostgreSQL 16.2, psycopg2 2.9.9
vs psycopg 3.1.18, MagicStack harness) **[third party, checked 2026-11-09]**
(https://www.tigerdata.com/blog/psycopg2-vs-psycopg3-performance-benchmark):

| Query | psycopg2 | psycopg3 sync | psycopg3 async |
|---|---:|---:|---:|
| `generate_series`, rows/s (mean latency) | 626,674 (15.9 ms) | 1,250,967 (8.0 ms) | 1,286,341 (7.8 ms) |
| `pg_type`, rows/s | 150,418 | 491,519 | 457,796 |
| `large_object`, rows/s | 209,909 | 101,391 | 133,010 |
| `arrays`, rows/s | 109,184 | 101,523 | 113,906 |
| `copyfrom`, rows/s | 444,407 | 456,626 | 470,290 |
| `batch` inserts, rows/s | 13,257 | 4,933 | 11,041 |
| `SELECT 1+1`, queries/s | 9,250 | 4,437 | 7,190 |

psycopg3's binary mode was not separated out. psycopg3 "sends the query and the parameters to the
server separately" (extended protocol); its pipeline mode is "experimental", requires libpq 14+,
excludes COPY and server-side cursors; `executemany()` uses pipeline mode internally since 3.1
**[vendor]** (https://www.psycopg.org/psycopg3/docs/basic/from_pg2.html,
https://www.psycopg.org/psycopg3/docs/advanced/pipeline.html).

**Go**: jackc/go_db_bench (archived 2024-11-29): "all the Go drivers perform similarly when
performing queries that return small result sets over TCP"; larger differences on large result
sets "especially if one driver uses the binary format and another uses the text format"; Unix
sockets "several times faster" than TCP; TLS costs ~15%; batching helps when latency is
significant **[vendor]** (https://github.com/jackc/go_db_bench). pgx v3 (2017): 16,483 q/s vs
lib/pq 10,106 q/s on large result sets (https://groups.google.com/g/golang-nuts/c/Lue-Kao5mGQ).
Phil Eaton (2023-10-05, 10 M rows via COPY, 10 runs, hardware not stated): 16 cols x 32 B lib/pq
via database/sql 95,665 rows/s vs pgx native 214,869; 3 cols x 8 B 1,225,986 vs 1,923,723 **[third
party]** (https://notes.eatonphil.com/2023-10-05-go-database-sql-overhead-on-insert-heavy-workloads.html).
pgx text-vs-binary bytes on the wire (100,000 rows of 2 x uuid, 2 x timestamp, int4, int8, bool;
strace): simple protocol 17.7 MB, extended/binary 9.6 MB (-46%); lib/pq 17.8 MB in both modes
("pq always receives results as text"); savings dominated by uuid 36 -> 16 bytes and timestamp
26 -> 8 **[third party, checked 2026-11-09]** (https://boosterkrd.github.io/2025/12/20/golang-postgresql-binary-protocol.html).

**.NET**: Npgsql "regularly figures in the top contenders" of TechEmpower; automatic
preparation, `NpgsqlBatch` (one round trip for several statements), 8 KB default read/write
buffers, pooled-connection reset buffered into the next statement **[vendor]**
(https://www.npgsql.org/doc/performance.html). On Citrine `aspcore-ado-pg` did 388,807
single-query req/s vs 622,917 for the best Rust entry.

**Text vs binary, libpq**: no controlled rows/s benchmark exists and anecdotes conflict: Jack
Christensen measured 1,000 rows x 5 text columns ~16% slower in binary (server-side extra copy,
https://www.postgresql.org/message-id/CAMovtNoHFod2jMAKQjjxv209PCTJx5Kc66anwWvX0mEiaXwgmA@mail.gmail.com);
a 2020 report fetching 10 M integers saw text 18.5 s / 633 MB vs binary 21.5 s / 480 MB
(https://www.postgrespro.com/list/thread-id/2497004); bytea is ~2.9x faster to select in binary
(2007, https://www.postgresql.org/message-id/466299A7.20403@eba.si) **[mailing lists]**.

**Bulk and columnar ceilings** (each its own machine): DuckDB postgres_scanner (2022-09-30, DuckDB
0.5.1, PostgreSQL 14.5, M1 Max, TPC-H SF1) reads via `COPY (SELECT ...) TO STDOUT (FORMAT
binary)`, parallelises over ctid page ranges (1,000 pages per task) with `pg_export_snapshot()`,
and beat stock PostgreSQL on about half of the 22 queries (Q1 0.74 s vs 1.12 s; Q17 0.75 s vs
>60 s) **[vendor]** (https://duckdb.org/2022/09/30/postgres-scanner.html). DuckDB ADBC
(2023-08-04, M1 Max 32 GB, TPC-H SF1 lineitem output + insert): ODBC 28.149 s vs ADBC 0.724 s
(38x), attributed to "extra allocations and copies" converting columnar results to row-wise
ODBC buffers **[vendor]** (https://duckdb.org/2023/08/04/adbc.html). Teradata (2026-08-24, driver
nodes 8 vCPU/32 GB): parallel multi-stream Flight SQL reads 15–30x JDBC on standard hardware and
47–84x on larger AWS nodes, sustaining 270–350 MB/s; **single-stream direct queries were 10–15%
slower than JDBC** (Arrow 30.4 / 39.9 / 40.3 MB/s vs JDBC 33.8 / 46.2 / 47.5 MB/s for 100 MB /
500 MB / ~780 MB) "because it adds a format conversion and a gRPC/TLS hop"; uploads at parity
(~1.4% overhead at 1 GB) **[vendor]** (https://www.teradata.com/insights/data-architecture/how-fast-is-arrow-flight-sql-with-teradata).
Voltron Data via a Carahsoft mirror: ADBC over Flight SQL "over 33%" faster than optimised JDBC
and ">21x" faster than default JDBC (TPC-H SF1 lineitem, DuckDB-backed Flight SQL server, Arrow
JDBC driver 11.0.0); the original post was unreachable (DNS failure) **[vendor]**
(https://www.carahsoft.com/resources/25251-comparing-performance-of-adbc-and-jdbc-in-python-for-arrow-flight-sql).
The Dremio claims (Flight "more than 15x" over ODBC; a "20x faster alternative to ODBC and JDBC"
talk) are vendor marketing and a Dremio community user reported Flight and JDBC "similar
performance" in their own test (https://community.dremio.com/t/jdbc-vs-arrow-flight-for-java/7158).
Section 6.3 has the Flight numbers.

Readings: the realistic single-core decode ceiling for simple integer rows from PostgreSQL over
localhost is roughly 7–8 M rows/s (asyncpg and pgx, 2023, high-clock desktop); ~600-row wide rows
1–3 M rows/s; 1 KB bytea 1.4–2 M rows/s. A Rust driver should publish numbers on the MagicStack
query set so it can be placed on this scale directly. For bulk reads the ceiling is set by binary
COPY and columnar transports, and any per-row API loses to them by 5–40x, so the "faster than
JDBC" claim must say whether the API surface is row-at-a-time or bulk-to-Arrow.

### Java without JDBC vs Java with JDBC vs Rust, same hardware

From the Citrine table in section 2 (one machine, so the rows are comparable) **[third party,
checked 2026-11-09]**:

| Test | Best Rust | Best Java without JDBC (vertx-pg-client) | Best Java with JDBC | JDBC gap |
|---|---:|---:|---:|---|
| Single query, req/s | ntex-db 622,917 | vertx-postgres 572,712 (−8%) | quarkus (pgjdbc + Agroal) 306,249 | 2.0x behind Rust; p99 12.11 ms vs 1.60 |
| Fortunes, req/s | may-minihttp 581,366 | vertx-postgres 454,920 | quarkus 216,934 | 2.7x; spring (pgjdbc + Hikari) 23,761 is 24x |
| 20 queries, req/s | axum-pg 32,369 | vertx-postgres 32,016 (−1%) | jooby (JDBC) 17,483 | 1.9x |
| 20 updates, req/s | xitca-web 22,942 | vertx-postgres 16,917 | jooby 13,026 | 1.8x; spring-jpa 3,148 |

The best Java entries that bypass JDBC are within 8% of the top Rust entries on single query and
within 1% on 20-query; the JDBC-based Java entries are 2–6x behind on the same tests with p99s
5–20x higher. So "faster than native JDBC" is a low bar on these workloads; "faster than Vert.x
pg client and tokio-postgres with per-core pipelined connections" is the real bar. No direct,
published tokio-postgres-vs-pgJDBC or sqlx-vs-JDBC driver benchmark exists (searches 2026-11-09);
the closest are whole-stack comparisons like TFB and a 2026 "PostgreSQL REST API benchmark"
that includes Actix+tokio-postgres and Spring Boot on Java 24 but measures the HTTP stack
**[third party, not verified in detail]** (https://npgsqlrest.github.io/blog/postgresql-rest-api-benchmark-2026).

Adjacent evidence on "Rust is faster" claims: ScyllaDB says its Rust driver was "the most
performant driver, beating the C++ driver" and that its Rust Alternator driver gives ~58% more
throughput than the AWS SDK on a 3-node cluster (2026-08-27) **[vendor]**
(https://www.scylladb.com/2026/08/27/new-rust-driver-for-scylladbs-dynamodb-api/); critics of an
earlier Go-vs-Rust ScyllaDB comparison attributed the Go driver's win to "request coalescing",
not language **[third party]** (https://redlib.hbubli.cc/r/rust/comments/y23dhn/a_new_scylladb_go_driver_faster_than_gocql_and).
The rust-postgres maintainer's questions on an early benchmark (whether the table was truncated
between runs, whether environments matched, and that "JDBC drivers do it almost universally when
it comes to statement caching") are the questions any new claim will face
(https://github.com/sfackler/rust-postgres/issues/83). Section 6.8 and the method notes in
section 11 turn this into a protocol.

## 4. Standards: what parity means

### Version baseline

| Standard | Version cited | Date | Status |
|---|---|---|---|
| JDBC (JSR 221) | 4.3 (MR3, Java SE 9) + MR4 "JDBC 4.4" (approved 2024-12-25) + MR5 "JDBC 4.5" (approved 2025-12-09; accepted change log 2026-03-02) | 2017–2026 | MR5 is newest; the JDK 21 javadoc still says `JDBC 4.3 API` |
| ODBC | 3.8 (Windows 7/8 Driver Manager 3.80/3.81); ODBC 4.0 is an unfinished GitHub draft | 2010–2013; draft 2016+ | 3.8 is the shipping standard |
| ISO/IEC 9075-3 SQL/CLI | 9075-3:2023 (6th ed., 2023-06, 371 pp.) | 2023 | paid; not opened |
| ISO/IEC 9075-2 SQL/Foundation | 9075-2:2023 (6th ed., 2023-06, ~1715 pp.) | 2023 | paid; not opened |
| ADBC | API spec 1.1.0; libraries ADBC 24 (2026-07-28, lib 1.12.0); ADBC 25 (2026-10-26, lib 1.13.0, third-party report only) | 2026 | active |
| R2DBC | 1.0.0.RELEASE (2022-04-25) | 2022 | "stable for the foreseeable future"; no 1.1 exists |
| PEP 249 DB-API | 2.0 | 1999/2001 | informational PEP |
| Go database/sql | Go 1.27 docs | 2026 | stdlib |
| ADO.NET System.Data.Common | .NET 10 docs | 2025–26 | stdlib |
| Arrow Flight SQL | Arrow docs v26.0.1 | 2026 | protocol |

Sources: https://jcp.org/en/jsr/detail?id=221,
https://www.jcp.org/aboutJava/communityprocess/maintenance/jsr221/221ChangeLog.html (JSR 221
timeline: Final Release 2006-12-11, MR1 2011-10-13, MR2 2014-03-04, MR3 2017-09-21, MR4
2025-03-11, MR5 2026-04-10 on the detail page; the change log gives acceptance dates MR1
2011-04-15, MR2 2013-12-05, MR3 2017-03-30, MR4 2024-12-25, MR5 2025-12-09) **[vendor, checked
2026-11-09]**.

### JDBC 4.3 / 4.4 / 4.5: surface and method counts

JDBC is by far the largest surface **[vendor, checked 2026-11-09]** (Java SE 21 javadoc,
https://docs.oracle.com/en/java/javase/21/docs/api/java.sql/java/sql/package-summary.html and
the per-interface pages linked below):

| Interface / package | Count | Load-bearing semantics |
|---|---:|---|
| `java.sql` | 55 public types (27 interfaces, 24 classes, 4 enums) | core API |
| `javax.sql` | 19 types | `DataSource` preferred over `DriverManager`; `ConnectionPoolDataSource`/`PooledConnection`/events for pooling; `XADataSource`/`XAConnection`; RowSets |
| `Driver` | 7 methods | `connect` returns `null` for a URL it does not recognise; `jdbcCompliant()` must return `false` unless the driver passes the compliance tests, where compliance = "full support for the JDBC API and full support for SQL-92 Entry Level" (https://docs.oracle.com/en/java/javase/21/docs/api/java.sql/java/sql/Driver.html) |
| `DriverManager` | — | loads drivers from `jdbc.drivers` and the service-provider mechanism; URL `jdbc:subprotocol:subname`; precedence between URL and `Properties` values is implementation-defined |
| `Connection` | 58 declared methods (+2 `Wrapper`), six `default` | auto-commit on for new connections; `commit/rollback` throw in auto-commit; isolation constants `NONE=0, READ_UNCOMMITTED=1, READ_COMMITTED=2, REPEATABLE_READ=4, SERIALIZABLE=8`; savepoints; `abort(Executor)`; `setNetworkTimeout` (closes the connection) distinct from `Statement.setQueryTimeout` (cancels the statement); `isValid(timeout)`; `beginRequest/endRequest` pool hints; `createArrayOf/createStruct/createBlob/...` throw `SQLFeatureNotSupportedException` when unsupported (https://docs.oracle.com/en/java/javase/21/docs/api/java.sql/java/sql/Connection.html) |
| `Statement` | 54 declared (+2) | `executeQuery` must return exactly one `ResultSet`; `execute` + `getMoreResults()/getUpdateCount()` until `false && -1`; batch `executeBatch()` returns `int[]` of counts / `SUCCESS_NO_INFO=-2` / `EXECUTE_FAILED=-3` and throws `BatchUpdateException` with partial counts; `executeLargeBatch` default throws; `setMaxRows` silently drops; `setFetchSize` is a hint; `setEscapeProcessing` default on; `cancel()` from another thread; JDBC 4.3 `enquoteLiteral` (`G'Day` -> `'G''Day'`), `enquoteIdentifier`, `isSimpleIdentifier` (ASCII letter start, letters/digits/`_`, 1–128 chars), `enquoteNCharLiteral` (https://docs.oracle.com/en/java/javase/21/docs/api/java.sql/java/sql/Statement.html) |
| `PreparedStatement` | 58 declared (36 names): 8 non-setters + 50 setter overloads | 1-based parameter indexes; streams with no-length/int/long variants; `setObject(..., SQLType)` (https://docs.oracle.com/en/java/javase/21/docs/api/java.sql/java/sql/PreparedStatement.html) |
| `CallableStatement` | ~121: 12 `registerOutParameter` overloads, ~30 `get*` by index, ~31 by name, 47 named `set*`, `wasNull` | `{?= call proc[(args)]}` / `{call ...}`; OUT parameters registered before execution; process ResultSets and update counts before reading OUT values (https://docs.oracle.com/en/java/javase/21/docs/api/java.sql/java/sql/CallableStatement.html) |
| `ResultSet` | ~195 (~70 getters, ~85 updaters) | constants `TYPE_FORWARD_ONLY=1003, TYPE_SCROLL_INSENSITIVE=1004, TYPE_SCROLL_SENSITIVE=1005, CONCUR_READ_ONLY=1007, CONCUR_UPDATABLE=1008, HOLD_CURSORS_OVER_COMMIT=1, CLOSE_CURSORS_AT_COMMIT=2, FETCH_FORWARD=1000, FETCH_REVERSE=1001, FETCH_UNKNOWN=1002`; getters by 1-based index and case-insensitive label; `getObject(int, Class<T>)`; row ops `insertRow/updateRow/deleteRow/refreshRow/...`; streams must be fully read before the next getter; `wasNull` (https://docs.oracle.com/en/java/javase/21/docs/api/java.sql/java/sql/ResultSet.html) |
| `ResultSetMetaData` | 21 methods | name, label, type code, type name, precision, scale, nullability (`columnNoNulls=0, columnNullable=1, columnNullableUnknown=2`), signed, auto-increment, case-sensitive, searchable, currency, read-only/writable, table/schema/catalog, column class name |
| `ParameterMetaData` | 9 methods | count, nullable, signed, precision, scale, type, type name, class name, mode (`In=1/InOut=2/Out=4`) — **from JDK source knowledge, not the fetched page** |
| `DatabaseMetaData` | ~177 methods + 61 constants | 25 catalog lookups (`getTables, getColumns, getPrimaryKeys, getImportedKeys, getExportedKeys, getCrossReference, getProcedures, getProcedureColumns, getFunctions, getFunctionColumns, getIndexInfo, getTypeInfo, getUDTs, getAttributes, getBestRowIdentifier, getVersionColumns, getPseudoColumns, getColumnPrivileges, getTablePrivileges, getSchemas x2, getCatalogs, getTableTypes, getSuperTables, getSuperTypes, getClientInfoProperties`); ~150 capability probes (`supports*`, 21 `getMax*`, identifier rules, keyword lists, transaction and result-set capability, `getSQLStateType` with `sqlStateXOpen`/`sqlStateSQL`/`sqlStateSQL99`) (https://docs.oracle.com/en/java/javase/21/docs/api/java.sql/java/sql/DatabaseMetaData.html) |
| `SQLException` | 8 constructors | `getSQLState()`, `getErrorCode()`, chaining, `Iterable<Throwable>`; subclasses map to SQLSTATE classes: `SQLDataException`->22, `SQLFeatureNotSupportedException`->0A, `SQLIntegrityConstraintViolationException`->23, `SQLInvalidAuthorizationSpecException`->28, `SQL(Non)TransientConnectionException`->08, `SQLSyntaxErrorException`->42, `SQLTransactionRollbackException`->40, `SQLTimeoutException` for query/login timeouts; `DataTruncation` (writes -> exception, reads -> warning); transient vs non-transient = whether retry may succeed |

What each version added (package doc): 4.3 (Java 9: sharding keys, `beginRequest/endRequest`,
`DriverManager.drivers()`, `Statement` enquoting); 4.2 (Java 8: `JDBCType`/`SQLType`, REF CURSOR,
large update counts, `java.time` conversions, `Types.REF_CURSOR=2012, TIME_WITH_TIMEZONE=2013,
TIMESTAMP_WITH_TIMEZONE=2014`); 4.1 (Java 7: try-with-resources, `getObject(…, Class<T>)`,
pseudo-columns, `setSchema/abort/setNetworkTimeout`, `closeOnCompletion`); 4.0 (Java 6: auto
driver discovery, national character set, SQLXML, `SQLException` chaining and subclasses, LOB
create/free, ROWID, `Wrapper`, statement-pool events).

Maintenance releases **[downloaded+pdftotext 2026-11-09]**
(https://www.jcp.org/aboutJava/communityprocess/maintenance/jsr221/JDBC4.2MR-January2014.pdf,
.../JDBC4.3MR-January2017.pdf, .../JDBC4.4MR-December_2024.pdf,
https://jcp.org/aboutJava/communityprocess/maintenance/jsr221/JDBC4.5MR-November_2025.pdf,
.../JDBC_Change_Log_March_02_2026.pdf):

- **4.3 (MR3)**, 19 items: `ShardingKey`, `ShardingKeyBuilder.subkey(Object, SQLType)` (must
  support `JDBCType.VARCHAR`), `ConnectionBuilder`, `XAConnectionBuilder`,
  `PooledConnectionBuilder`, `Connection.beginRequest/endRequest/setShardingKey/setShardingKeyIfValid`,
  `DriverManager.drivers()`, `DatabaseMetaData.supportsSharding()`, `getTables` REMARKS may be
  null, `Statement.enquote*`/`isSimpleIdentifier`, pool managers must call `endRequest` or
  `close` on the logical handle when the application closes it.
- **4.4 (MR4, December 2024)**: one change: remove SecurityManager/`SecurityException` wording
  (JEP 486, JDK 24); `Connection.abort`, `setNetworkTimeout`, `DriverManager.deregisterDriver/
  setLogStream/setLogWriter` no longer throw `SecurityException`.
- **4.5 (MR5, draft November 2025, accepted change log 2026-03-02)**, seven changes: (1) `Array,
  Blob, Clob, NClob, SQLXML` become AutoCloseable via `default void close() { free(); }`; (2)
  `Connection` gains `enquoteIdentifier/enquoteLiteral/enquoteNCharLiteral/isSimpleIdentifier`;
  (3) new `Types`/`JDBCType` constants **`DECFLOAT = 2015` and `JSON = 2016`**; (4)
  `Timestamp.hashCode` doc fix; (5) `SQLPermission` deprecated for removal; (6) **selective
  disabling of escape processing and `?` parameter markers** inside a region delimited by `{\\`
  … `\\}` (needed because SQL:2016 `MATCH_RECOGNIZE` patterns use `?`), with `\\\\` to embed a
  backslash; (7) Appendix B updated: `JSON -> String`, `DECFLOAT -> BigDecimal`,
  `TIMESTAMP_WITH_TIMEZONE -> OffsetDateTime`, `TIME_WITH_TIMEZONE -> OffsetTime`, `BigInteger ->
  BIGINT`, `java.util.Calendar/Date -> TIMESTAMP`, widened getter/setter matrices.

`java.sql.Types` constants (value / since): `BIT -7, TINYINT -6, SMALLINT 5, INTEGER 4, BIGINT -5,
FLOAT 6, REAL 7, DOUBLE 8, NUMERIC 2, DECIMAL 3, CHAR 1, VARCHAR 12, LONGVARCHAR -1, DATE 91, TIME
92, TIMESTAMP 93, BINARY -2, VARBINARY -3, LONGVARBINARY -4, NULL 0, OTHER 1111` (1.1);
`JAVA_OBJECT 2000, DISTINCT 2001, STRUCT 2002, ARRAY 2003, BLOB 2004, CLOB 2005, REF 2006` (1.2);
`DATALINK 70, BOOLEAN 16` (1.4); `ROWID -8, NCHAR -15, NVARCHAR -9, LONGNVARCHAR -16, NCLOB 2011,
SQLXML 2009` (1.6); `REF_CURSOR 2012, TIME_WITH_TIMEZONE 2013, TIMESTAMP_WITH_TIMEZONE 2014`
(1.8); MR5 adds `DECFLOAT 2015, JSON 2016`. The numeric values were recalled from the
constant-values page rather than read; names and since-versions are from the javadoc
(https://docs.oracle.com/en/java/javase/21/docs/api/java.sql/java/sql/Types.html) — see section 13.

Escape syntax (Chapter 13, as implemented by Microsoft's driver) **[vendor, checked
2026-11-09]** (https://learn.microsoft.com/en-us/sql/connect/jdbc/using-sql-escape-sequences): six
forms: LIKE escape `{escape '\\'}`; scalar functions `{fn ...}` with the string, numeric,
datetime and system function sets (the ODBC Appendix E set); `{d 'yyyy-mm-dd'}`, `{t 'hh:mm:ss'}`,
`{ts '...'}`; `{[?=]call proc[(...)]}`; `{oj ... OUTER JOIN ...}`; `{limit <rows> [offset
<offset>]}` from JDBC 4.1. ODBC advertises scalar-function support through `SQLGetInfo` bitmasks
(`SQL_STRING_FUNCTIONS`, `SQL_NUMERIC_FUNCTIONS`, `SQL_TIMEDATE_FUNCTIONS`, `SQL_SYSTEM_FUNCTIONS`,
`SQL_TIMEDATE_ADD/DIFF_INTERVALS`) (https://learn.microsoft.com/en-us/sql/odbc/reference/appendixes/appendix-e-scalar-functions).

Appendix B type mapping (MR5 text): B-1 JDBC -> Java (CHAR family and JSON -> `String`;
NUMERIC/DECIMAL/DECFLOAT -> `BigDecimal`; BIT/BOOLEAN -> `boolean`; TINYINT -> `byte`; SMALLINT
-> `short`; INTEGER -> `int`; BIGINT -> `long`; REAL -> `float`; FLOAT/DOUBLE -> `double`; BINARY
family -> `byte[]`; DATE/TIME/TIMESTAMP -> `java.sql.*`; LOBs, ARRAY, STRUCT, REF, DATALINK ->
`java.net.URL`, ROWID, NCLOB, SQLXML; TZ types -> `OffsetDateTime`/`OffsetTime`); B-3 `getObject`
boxed with the quirk TINYINT and SMALLINT -> `Integer`; B-4 `setObject` defaults (`BigInteger` ->
BIGINT, `LocalDate` -> DATE, `LocalDateTime` -> TIMESTAMP, `OffsetDateTime` ->
TIMESTAMP_WITH_TIMEZONE, ...); B-6 getter matrix (`getString` accepts everything incl. JSON and
DECFLOAT; `getObject` everything). JDBC 3.0-era text: REAL 7 decimal digits, FLOAT/DOUBLE 15,
DECIMAL/NUMERIC "precision and scale of at least 15" required
(https://docs.oracle.com/javase/1.5.0/docs/guide/jdbc/getstart/mapping.html).

**Compliance and TCK.** `Driver.jdbcCompliant()` is the only public normative statement of what
"JDBC Compliant" means that was reachable. The Scribd mirror of `jdbc4.3-fr-spec.pdf` (228 pp.)
shows Chapter 6 = "Compliance" (6.1 Definitions, 6.2 Guidelines and Requirements, 6.3 JDBC 4.x API
Compliance, 6.4 Java EE JDBC Compliance) but its body text was not retrievable. The JDBC TCK is
not separate: the JSR 221 MR3 page refers RI/TCK to the Java SE 9 (JSR 379) release, and the Java
SE TCK (JCK) is available free only under the OpenJDK Community TCK License Agreement (OCTLA) to
developers of OpenJDK-derived compatible implementations, via a request form, screening committee
and signed OCTLA and OCA **[checked 2026-11-09 via search snippets]**
(https://openjdk.org/groups/conformance/JckAccess). Java TCK licences forbid "comparative
compatibility" claims (e.g. "90% compatible")
(https://www.jcp.org/aboutJava/communityprocess/licenses/jsr379/SATCK-JSR_379-SE-9.pdf). **A Rust
library therefore cannot literally be JDBC-compliant**; the achievable claim is feature parity
with the JDBC 4.3 + MR5 API surface. An unverified recollection of §6.2/6.3 (a compliant driver
must fully implement `Driver`, `Connection`, `Statement`, `PreparedStatement`,
`CallableStatement` if the DBMS has stored procedures, `ResultSet`, `ResultSetMetaData`,
`DatabaseMetaData`, `ParameterMetaData`, support `TYPE_FORWARD_ONLY`/`CONCUR_READ_ONLY`, the
escape syntax, SQL-92 Entry Level plus DROP TABLE, and may use `SQLFeatureNotSupportedException`
for optional features) is a checklist hypothesis to be confirmed against the licensed PDF.

### ODBC 3.8: conformance levels, descriptors, diagnostics

ODBC 3.x is a C handle-based API (ENV/DBC/STMT/DESC) with about 76 functions (~60 current plus
deprecated 2.x forms), four descriptors, three interface-conformance levels, a 5-character
SQLSTATE model and ~175 `SQLGetInfo` info types. All **[vendor, checked 2026-11-07/09]** from
Microsoft Learn (reference updated 2026-02-05).

**Function conformance** (https://learn.microsoft.com/en-us/sql/odbc/reference/develop-app/function-conformance):

- Core: `SQLAllocHandle, SQLBindCol, SQLBindParameter, SQLCancel, SQLCloseCursor, SQLColAttribute,
  SQLColumns, SQLConnect, SQLCopyDesc, SQLDataSources, SQLDescribeCol, SQLDisconnect,
  SQLDriverConnect, SQLDrivers, SQLEndTran, SQLExecDirect, SQLExecute, SQLFetch, SQLFetchScroll,
  SQLFreeHandle, SQLFreeStmt, SQLGetConnectAttr, SQLGetCursorName, SQLGetData, SQLGetDescField,
  SQLGetDescRec, SQLGetDiagField, SQLGetDiagRec, SQLGetEnvAttr, SQLGetFunctions, SQLGetInfo,
  SQLGetStmtAttr, SQLGetTypeInfo, SQLNativeSql, SQLNumParams, SQLNumResultCols, SQLParamData,
  SQLPrepare, SQLPutData, SQLRowCount, SQLSetConnectAttr, SQLSetCursorName, SQLSetDescField,
  SQLSetDescRec, SQLSetEnvAttr, SQLSetStmtAttr, SQLSpecialColumns, SQLStatistics, SQLTables`.
- Level 1: `SQLBrowseConnect, SQLBulkOperations, SQLMoreResults, SQLPrimaryKeys,
  SQLProcedureColumns, SQLProcedures, SQLSetPos`.
- Level 2: `SQLColumnPrivileges, SQLDescribeParam, SQLForeignKeys, SQLTablePrivileges`.

Each function is tagged ISO 92, Open Group, ODBC or Deprecated; ISO 92 ⊂ Open Group ⊂ ODBC 3.x
(https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/odbc-api-reference).

**Interface conformance** is feature-based, reported via
`SQLGetInfo(SQL_ODBC_INTERFACE_CONFORMANCE)` (`SQL_OIC_CORE/LEVEL1/LEVEL2`); "Conformance with a
given level implies complete conformance with all lower levels"; ODBC 3.x Core absorbs everything
from 2.x API Level 1 (https://learn.microsoft.com/en-us/sql/odbc/reference/develop-app/interface-conformance-levels).

- **Core** (mandatory; "correspond to the features defined in the ISO CLI specification and to
  the nonoptional features defined in the Open Group CLI specification"): all handle types;
  `SQLBindCol`; input-only parameters incl. arrays; bind offsets; data-at-execution; cursor
  names; result metadata excluding bookmark column 0; catalog `SQLColumns, SQLGetTypeInfo,
  SQLStatistics, SQLTables`; connect functions; prepare/execute/exec-direct; forward-only fetch;
  `SQLGetData` in parts; attribute get/set-to-default; descriptor manipulation; diagnostics;
  `SQLGetFunctions`, `SQLGetInfo`, `SQLNativeSql`; `SQLEndTran` to commit — **a Core driver
  need not support true transactions** (no `SQL_ROLLBACK`, no `SQL_AUTOCOMMIT_OFF`); `SQLCancel`;
  thread safety (serialisation is conformant); `SQLSpecialColumns(SQL_BEST_ROWID)`
  (https://learn.microsoft.com/en-us/sql/odbc/reference/develop-app/core-interface-conformance).
- **Level 1** (features 101–109): two-part names; true async (all-or-nothing per connection);
  scrollable cursors via `SQLFetchScroll` (bookmarks excluded); `SQLPrimaryKeys`; stored
  procedures via the call escape and `SQLProcedures`/`SQLProcedureColumns`; `SQLBrowseConnect`;
  `SQLSetPos` POSITION/REFRESH; `SQLMoreResults`; true atomic transactions with `SQL_ROLLBACK`.
- **Level 2** (features 201–211): three-part names; `SQLDescribeParam`; output and input/output
  parameters and return values; bookmarks; `SQLColumnPrivileges`, `SQLForeignKeys`,
  `SQLTablePrivileges`; `SQLBulkOperations(SQL_ADD)` and `SQLSetPos(DELETE|UPDATE)`;
  statement-level async; `SQLSpecialColumns(SQL_ROWVER)`; concurrency other than read-only;
  `SQL_ATTR_LOGIN_TIMEOUT` and `SQL_ATTR_QUERY_TIMEOUT`; changing default isolation and
  executing SERIALIZABLE.

Attribute conformance assigns each env/connection/statement attribute a level (e.g. `AUTOCOMMIT`
and `CONNECTION_DEAD` Level 1; `TXN_ISOLATION` L1 one non-default value / L2 must include
SERIALIZABLE; `CURSOR_TYPE` Core for forward-only; `QUERY_TIMEOUT`, `USE_BOOKMARKS`,
`CURSOR_SENSITIVITY` Level 2) (https://learn.microsoft.com/en-us/sql/odbc/reference/develop-app/attribute-conformance).

**Descriptors**: four kinds — Application Parameter Descriptor (APD), Implementation Parameter
Descriptor (IPD), Application Row Descriptor (ARD), Implementation Row Descriptor (IRD).
Different types in corresponding APD/IPD or ARD/IRD records cause the driver to convert
(Appendix D). Explicit descriptors can be shared across statements; `SQLBindCol` sets ARD fields,
`SQLColAttribute` reads IRD fields (https://learn.microsoft.com/en-us/sql/odbc/reference/develop-app/descriptors).
Parameter arrays: column-wise (default) or row-wise binding (`SQL_ATTR_PARAM_BIND_TYPE =
sizeof(struct)`), `SQL_ATTR_PARAMSET_SIZE`, per-set status array (`SQL_PARAM_SUCCESS /
SUCCESS_WITH_INFO / ERROR / UNUSED / DIAG_UNAVAILABLE`), `SQL_ATTR_PARAMS_PROCESSED_PTR`
(https://learn.microsoft.com/en-us/sql/odbc/reference/develop-app/binding-arrays-of-parameters).
The descriptor model (APD/IPD/ARD/IRD plus row-wise/column-wise binding and status arrays) is the
closest classical analogue to Arrow-style columnar buffers: ARD/APD describe the application
buffer layout, IRD/IPD the server-described schema — the same split ADBC makes with Arrow schemas.

**Type tables**: SQL type identifiers `SQL_CHAR, SQL_VARCHAR, SQL_LONGVARCHAR, SQL_WCHAR,
SQL_WVARCHAR, SQL_WLONGVARCHAR, SQL_DECIMAL(p,s), SQL_NUMERIC(p,s), SQL_SMALLINT, SQL_INTEGER,
SQL_REAL, SQL_FLOAT(p), SQL_DOUBLE, SQL_BIT, SQL_TINYINT, SQL_BIGINT, SQL_BINARY, SQL_VARBINARY,
SQL_LONGVARBINARY, SQL_TYPE_DATE, SQL_TYPE_TIME(p), SQL_TYPE_TIMESTAMP(p), SQL_TYPE_UTCDATETIME,
SQL_TYPE_UTCTIME`, 13 `SQL_INTERVAL_*`, `SQL_GUID`. All drivers must support all C types
(`SQL_C_CHAR, SQL_C_WCHAR, SQL_C_SSHORT/USHORT, SQL_C_SLONG/ULONG, SQL_C_FLOAT, SQL_C_DOUBLE,
SQL_C_BIT, SQL_C_STINYINT/UTINYINT, SQL_C_SBIGINT/UBIGINT, SQL_C_BINARY, SQL_C_TYPE_DATE/TIME/
TIMESTAMP` with `SQL_TIMESTAMP_STRUCT.fraction` in billionths of a second, `SQL_C_NUMERIC`
(`SQL_NUMERIC_STRUCT`: precision, scale, sign, little-endian scaled integer `val[SQL_MAX_NUMERIC_LEN]`),
`SQL_C_GUID`, `SQL_C_INTERVAL_*`, `SQL_C_DEFAULT`); 3.8 drivers may define driver-specific C types
from `SQL_DRIVER_C_TYPE_BASE` (https://learn.microsoft.com/en-us/sql/odbc/reference/appendixes/sql-data-types,
https://learn.microsoft.com/en-us/sql/odbc/reference/appendixes/c-data-types).

**`SQLGetInfo`** returns ~170–180 info types in groups: Driver Information (~39), DBMS Product
(3), Data Source (~23), Supported SQL (~52 incl. `SQL_SQL_CONFORMANCE` ∈ {`SQL_SC_SQL92_ENTRY,
FIPS127_2_TRANSITIONAL, SQL92_INTERMEDIATE, SQL92_FULL`}, `SQL_SQL92_*` bitmasks), SQL Limits (20
`SQL_MAX_*`), Scalar Function (7), Conversion (`SQL_CONVERT_*`, 22 + GUID), plus `SQL_DM_VER`,
`SQL_XOPEN_CLI_YEAR`, `SQL_STANDARD_CLI_CONFORMANCE` (`SQL_SCC_XOPEN_CLI_VERSION1`,
`SQL_SCC_ISO92_CLI`), `SQL_TXN_CAPABLE` ("a SQL-92 conformant driver will never return
SQL_TC_NONE"), `SQL_TXN_ISOLATION_OPTION` (Entry level must include SERIALIZABLE), cursor
attribute bitmasks (https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlgetinfo-function).

**Diagnostics and SQLSTATE**: header fields `SQL_DIAG_CURSOR_ROW_COUNT, DYNAMIC_FUNCTION(_CODE),
NUMBER, RETURNCODE, ROW_COUNT`; record fields `CLASS_ORIGIN ("ISO 9075" or "ODBC 3.0"),
COLUMN_NUMBER, CONNECTION_NAME, MESSAGE_TEXT, NATIVE, ROW_NUMBER, SERVER_NAME, SQLSTATE,
SUBCLASS_ORIGIN`; records ordered by row number then rank (errors > no-data > warnings)
(https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlgetdiagfield-function). Appendix A
lists ~120 ODBC 3.x SQLSTATEs (`01xxx` warnings; `07`, `08`, `21S`, `22`, `23000`, `24000`, `25`,
`28000`, `34000`, `3C000`, `3D000`, `3F000`, `40`, `42`, `44000`, `HY000–HYT01`, `IM001–IM015`)
(https://learn.microsoft.com/en-us/sql/odbc/reference/appendixes/appendix-a-odbc-error-codes).
ODBC 3.x returns `HYxxx` instead of 2.x `S1xxx` and `42Sxx` instead of `S00xx`; the DM maps back
for `SQL_OV_ODBC2` applications (https://learn.microsoft.com/en-us/sql/odbc/reference/develop-app/sqlstate-mappings).

**ODBC 3.8 additions**: async connection operations (polling), streamed output parameters,
C-type extensibility (DM 3.80, Windows 7); driver-aware connection pooling, notification-based
async via `SQLCompleteAsync`, tracing (DM 3.81, Windows 8). Upgrading a 3.5 driver: accept
`SQL_OV_ODBC3_80`; return "03.80" for `SQL_DRIVER_ODBC_VER`; implement `SQL_ATTR_RESET_CONNECTION`
("All version 3.8 drivers should implement this"); implement `SQL_ASYNC_DBC_FUNCTIONS` even if
NOT_CAPABLE; export `SQLCancelHandle` if supported
(https://learn.microsoft.com/en-us/sql/odbc/reference/what-s-new-in-odbc-3-8,
https://learn.microsoft.com/en-us/sql/odbc/reference/develop-driver/upgrading-a-3-5-driver-to-a-3-8-driver,
https://learn.microsoft.com/en-us/sql/odbc/reference/develop-app/driver-aware-connection-pooling).

**ODBC 4.0** is a Microsoft GitHub draft (90 commits, 57 open issues, no release/date) adding
`SQL_UDT, SQL_ROW, SQL_ARRAY, SQL_MULTISET`, `SQL_VARIANT_TYPE`, `SQL_TIME_WITH_TIMEZONE`,
`SQLNextColumn`, `SQLGetNestedHandle`, dynamic columns, OAuth via `Auth_*` keywords, private
drivers, escape clauses `{limit}, {return}, {format}, {native}, {refresh}`; no shipped Driver
Manager implements it (https://github.com/microsoft/ODBC-Specification,
https://raw.githubusercontent.com/microsoft/ODBC-Specification/master/ODBC%204.0.md).

**ISO/IEC 9075-3:2023** (SQL/CLI, 371 pages) is paid and was not opened; the ODBC "Core" list is
the most precise public proxy for its mandatory routine set (functions tagged "ISO 92" are in the
ISO CLI; "ODBC"-tagged ones such as `SQLDriverConnect` are not).

**What makes a driver loadable**: on Windows, `HKLM\SOFTWARE\ODBC\Odbcinst.ini` with per-driver
`Driver` (DLL path), `Setup`, `APILevel` (0/1/2 = `SQL_ODBC_INTERFACE_CONFORMANCE`),
`ConnectFunctions` ("YYN"), `DriverODBCVer` ("03.80"), `FileUsage`, `SQLLevel`
(https://learn.microsoft.com/en-us/sql/odbc/reference/install/driver-specification-subkeys);
on unixODBC, an `odbcinst.ini` section with `Driver=` path to the `.so` loaded with `dlopen()`
at connect and `odbc.ini` DSNs (https://www.unixodbc.org/odbcinst.html). The DM connect sequence
is `SQLAllocHandle(ENV)` -> `SQLAllocHandle(DBC)` -> `SQLSetConnectAttr` -> `SQLDriverConnect`
(SQLSTATEs IM003–IM006 mark each failure). Connection-string grammar: `keyword=value;`, keywords
case-insensitive, **first occurrence wins**, braces for values containing `;`
(https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqldriverconnect-function). The exact
set of symbols the DMs resolve via `dlsym` (ANSI vs `W` entry points) was not found in an opened
document; section 10 has the Unicode-driver rule.

### ADBC 1.0 / 1.1: function table and manifests

ADBC is a small, columnar, ABI-stable C API; `adbc.h` is canonical **[vendor, checked
2026-11-09]** (https://raw.githubusercontent.com/apache/arrow-adbc/main/c/include/arrow-adbc/adbc.h,
https://arrow.apache.org/adbc/current/format/specification.html).

- Objects: `AdbcDatabase` (shared state and caches), `AdbcConnection` (one logical connection),
  `AdbcStatement` (query state, reusable; reuse invalidates prior results), each
  `{private_data, private_driver}`; `AdbcError {message, vendor_code, sqlstate[5], release}`
  plus in 1.1.0 `private_data, private_driver` present iff `vendor_code ==
  ADBC_ERROR_VENDOR_CODE_PRIVATE_DATA` (INT32_MIN); `AdbcErrorDetail {key, value, value_length}`;
  `AdbcPartitions`.
- 15 status codes: `OK 0, UNKNOWN 1, NOT_IMPLEMENTED 2, NOT_FOUND 3, ALREADY_EXISTS 4,
  INVALID_ARGUMENT 5, INVALID_STATE 6, INVALID_DATA 7, INTEGRITY 8, INTERNAL 9, IO 10, CANCELLED 11,
  TIMEOUT 12, UNAUTHENTICATED 13, UNAUTHORIZED 14`.
- `struct AdbcDriver` 1.0.0: 26 entries (`DatabaseInit, DatabaseNew, DatabaseSetOption,
  DatabaseRelease; ConnectionCommit, ConnectionGetInfo, ConnectionGetObjects,
  ConnectionGetTableSchema, ConnectionGetTableTypes, ConnectionInit, ConnectionNew,
  ConnectionSetOption, ConnectionReadPartition, ConnectionRelease, ConnectionRollback;
  StatementBind, StatementBindStream, StatementExecuteQuery, StatementExecutePartitions,
  StatementGetParameterSchema, StatementNew, StatementPrepare, StatementRelease,
  StatementSetOption, StatementSetSqlQuery, StatementSetSubstraitPlan`). 1.1.0 adds 31:
  `ErrorGetDetailCount, ErrorGetDetail, ErrorFromArrayStream`; `{Database, Connection, Statement}
  x {GetOption, GetOptionBytes, GetOptionDouble, GetOptionInt, SetOptionBytes, SetOptionDouble,
  SetOptionInt}`; `ConnectionCancel, ConnectionGetStatistics, ConnectionGetStatisticNames,
  StatementCancel, StatementExecuteSchema`. Total 57 entry points.
- Standard options: `uri`, `username`, `password`; `adbc.connection.autocommit`, `.readonly`,
  `.catalog`, `.db_schema`, `.transaction.isolation_level` ∈ {`default, read_uncommitted,
  read_committed, repeatable_read, snapshot, serializable, linearizable`};
  `adbc.statement.exec.incremental/progress/max_progress`; ingestion `adbc.ingest.target_table`,
  `.target_catalog`, `.target_db_schema`, `.temporary`, `.mode` ∈ {`create, append, replace
  (1.1), create_append (1.1)`}. Info codes `VENDOR_NAME 0 ... DRIVER_ADBC_VERSION 103`;
  statistics keys `AVERAGE_BYTE_WIDTH 0 ... ROW_COUNT 6`.
- Metadata is returned as Arrow data with fixed schemas, not as scalar probes: `GetInfo` =
  `info_name uint32, info_value dense_union{...}`; `GetObjects` = nested catalog -> db_schemas ->
  tables -> columns (`xdbc_*` fields) and constraints; `GetStatistics` per schema.
- `AdbcDriverInitFunc(int version, void* driver, AdbcError* error)` returns `NOT_IMPLEMENTED` for
  unsupported versions so the client can retry; name `AdbcDriverInit` or derived from the library
  name (`libadbc_driver_sqlite.so.2.0.0 -> AdbcDriverSqliteInit`). Compatibility rule: a 1.0.0
  driver under a 1.1.0 manager "must not access the new fields, which will carry undefined
  values"; a 1.1.0 driver under a 1.0.0 manager fills only the old portion. Option getters "must
  always be thread-safe (other operations are not)".
- Versioning: compatible additions -> 1.2.0, incompatible -> 2.0.0; no struct member changes;
  manifest format has its own `manifest_version` (currently 1)
  (https://arrow.apache.org/adbc/current/format/versioning.html).
- Driver manager: "a library that pretends to be a single driver"; a driver provides each ADBC
  function, a thin wrapper exporting each standard `Adbc…` name, and an entrypoint returning the
  table; the hidden implementation avoids the dynamic linker resolving names to the manager's
  copy (`RTLD_DEEPBIND` rejected as non-portable)
  (https://arrow.apache.org/adbc/current/format/how_manager.html).
- Manifests (TOML): `manifest_version`, `name, version, publisher, license, url`, `[ADBC] version`,
  `[ADBC.features] supported/unsupported`, `[Driver] entrypoint`, `[Driver.shared]` per platform
  (`linux_amd64`, `_musl`, `_mingw` suffixes; "the only key needed"). Search order:
  `ADBC_DRIVER_PATH`, `$VIRTUAL_ENV/etc/adbc/drivers`, `$CONDA_PREFIX/etc/adbc/drivers`, user dir
  (`~/Library/Application Support/ADBC/Drivers`, `$XDG_CONFIG_HOME/adbc/drivers`), system dir
  (`/Library/Application Support/ADBC/Drivers`, `/etc/adbc/drivers`); Windows adds
  `HKCU/HKLM\SOFTWARE\ADBC\Drivers\<name>` and `%LOCALAPPDATA%\ADBC\Drivers`; relative paths need
  `ADBC_LOAD_FLAG_ALLOW_RELATIVE_PATHS`; no `driver` option -> URI scheme is the driver name
  (https://arrow.apache.org/adbc/current/format/driver_manifests.html).
- Driver catalogue (November 2026): 22 drivers listed (BigQuery, Cassandra, chDB, ClickHouse,
  Databricks, DataFusion, Druid, DuckDB, Exasol, Firebolt, Flight SQL, SQL Server, MySQL/MariaDB,
  PostgreSQL, Presto, Quack, Redshift, SingleStore, Snowflake, Spark, SQLite, Trino) split among
  ASF, the "ADBC Driver Foundry" and vendors; no feature matrix on the page
  (https://arrow.apache.org/adbc/current/driver/index.html). Releases: ADBC 22 (2026-01-09), 23
  (2026-04-07, Node.js driver manager), 24 (2026-07-28, lib 1.12.0; C++20 required from the next
  release; DataFusion/BigQuery/Databricks/Snowflake drivers moved to the Foundry), 25 (2026-10-26,
  lib 1.13.0, Go core/driver split) reported only by third-party blogs
  (https://arrow.apache.org/blog/2026/07/28/adbc-24-release/,
  https://datalakehousehub.com/blog/adbc-25-upgrade-guide/).
- Rust: `adbc_core` 0.25.0 (Apache-2.0; `arrow-array`/`arrow-schema` >= 58, < 61) provides
  traits `Driver, Database, Connection, Statement`, modules `constants, error, options, schemas`,
  `PartitionedResult`, `LOAD_FLAG_*`; `adbc_ffi::export_driver!` with `crate-type = ["lib",
  "cdylib"]` exports a Rust driver as a C driver (https://docs.rs/adbc_core/latest/adbc_core/,
  https://docs.rs/adbc_ffi).

Transport is the structural difference from JDBC/ODBC: ADBC moves whole Arrow record batches
through `ArrowArrayStream`, so "parity with ADBC" means exposing results and parameters as
record-batch streams, bulk ingest, partitions, Substrait plans and the fixed metadata schemas —
not getters. Its error model (status + message + vendor code + SQLSTATE + key/value details) is
a superset of JDBC's `SQLException` and ODBC's diagnostic record; one Rust error type with these
five fields maps losslessly to all three.

### R2DBC 1.0 and its TCK

R2DBC 1.0.0.RELEASE (final 2022-04-25, after 268 tickets; "stable for the foreseeable future")
is a deliberately minimal non-blocking SPI on Reactive Streams **[vendor, checked 2026-11-09]**
(https://r2dbc.io/spec/1.0.0.RELEASE/spec/html/, https://r2dbc.io/2022/04/25/r2dbc-1.0-goes-ga):
`ConnectionFactory -> Connection -> Statement/Batch -> Result -> Row/RowMetadata`; every
operation deferred until subscription and cancellable; **0-based indexes**; bind by index or
name; `Result.Segment` streams (rows, update counts, messages, OUT parameters — subtype names
unverified); extensible `IsolationLevel`/`TransactionDefinition`; `R2dbcException {getMessage,
getSqlState, getErrorCode, getSql}` with transient (`R2dbcRollbackException,
R2dbcTimeoutException, R2dbcTransientResourceException`) and non-transient
(`R2dbcBadGrammarException, R2dbcDataIntegrityViolationException, R2dbcPermissionDeniedException,
R2dbcNonTransientResourceException`) families; `ConnectionFactoryOptions` typed `Option<T>`
(`DRIVER, PROTOCOL, HOST, PORT, USER, PASSWORD, DATABASE, CONNECT_TIMEOUT, SSL, LOCK_WAIT_TIMEOUT,
STATEMENT_TIMEOUT`); URL `r2dbc[s]:driver[:protocol]://[user:password@]host[:port][/path][?options]`;
`Connection.validate(ValidationDepth.LOCAL|REMOTE)`; `Statement.add()` to queue parameter sets,
`returnGeneratedValues(columns...)`, `fetchSize(int)` hint; `Blob/Clob` are publishers consumed
once; `Lifecycle.postAllocate/preRelease` pool hooks. Chapter 4 compliance requires transactions,
bind markers, zero-based indexes and ServiceLoader discovery. It deliberately omits scrollable
and updatable cursors, `DatabaseMetaData`-style probing, escape syntax and pooling (left to
r2dbc-pool), so R2DBC parity is far cheaper than JDBC parity.

The TCK (`io.r2dbc:r2dbc-spi-test` 1.0.0.RELEASE, Apache-2.0, on Maven Central) is a JUnit 5
`TestKit<T>` that drivers extend by supplying `getConnectionFactory()`, `getJdbcOperations()`
(Spring JDBC for fixtures), `getPlaceholder(int)`, `getIdentifier(int)` and optional hooks; about
35 scenarios: `validate; autoCommitByDefault, changeAutoCommitCommitsTransaction,
sameAutoCommitLeavesTransactionUnchanged; transactionCommit, transactionRollback; savePoint,
savePointStartsTransaction; batch, compoundStatement, createStatementFails; bindFails,
bindValueAsParameter, bindNullAsParameter, bindNull, bindNullFails, prepareStatement,
prepareStatementWithTrailingAddShouldFail, prepareStatementWithIncompleteBatchFails,
prepareStatementWithIncompleteBindingFails; blobInsert, blobSelect, clobInsert, clobSelect;
columnMetadata, rowMetadata, row, duplicateColumnNames; returnGeneratedValues,
returnGeneratedValuesFails; segmentInsertEmitsUpdateCount, segment*WithFilter*,
segmentSelectWithEmitsRow`; no isolation or timeout tests seen
(https://github.com/r2dbc/r2dbc-spi/blob/main/r2dbc-spi-test/src/main/java/io/r2dbc/spi/test/TestKit.java,
https://central.sonatype.com/artifact/io.r2dbc/r2dbc-spi-test). It is the only open-source,
runnable TCK among the surveyed standards. R2DBC is the closest template for an async Rust API:
`Publisher` ≈ `Stream`, cancellation-releases-resources ≈ `Drop`, `Result.Segment` ≈ an enum
of `Row | UpdateCount | Message | OutParameters`, `fetchSize` a hint with demand as the real flow
control.

### The convergence map across all APIs

PEP 249, Go `database/sql[/driver]`, ADO.NET `System.Data.Common` and Arrow Flight SQL were read
alongside the four above **[vendor, checked 2026-11-09]** (https://peps.python.org/pep-0249/,
https://pkg.go.dev/database/sql/driver, https://pkg.go.dev/database/sql,
https://learn.microsoft.com/en-us/dotnet/api/system.data.common,
https://arrow.apache.org/docs/format/FlightSql.html).

| Feature | JDBC | ODBC | ADBC | R2DBC | DB-API 2.0 | Go database/sql | ADO.NET | Flight SQL |
|---|---|---|---|---|---|---|---|---|
| Connection from string/options | `jdbc:sub:subname` + Properties | `keyword=value;` | `uri/username/password` options | `r2dbc[s]:` URL + typed options | module-defined | driver-defined DSN | `key=value;` builder | gRPC endpoint |
| Auto-commit default on; begin/commit/rollback | yes | yes (Core may lack rollback) | yes | yes | commit/rollback (no begin) | `BeginTx` | `BeginTransaction` | `ActionBeginTransaction`, auto-commit without `transaction_id` |
| Isolation levels | 4 + NONE | bitmask | 7 incl. snapshot, linearizable | extensible | — | 8 incl. WriteCommitted, Linearizable | enum | `SqlInfo` |
| Placeholders | `?` 1-based | `?` 1-based | Arrow-bound | index or name, 0-based | 5 styles (`qmark, numeric, named, format, pyformat`) | driver-defined (`?`, `:id`, `@end`) | provider | record-batch bind |
| Index base | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 |
| Prepared statements | yes | yes | yes | yes (implicit) | implicit | yes | `Prepare` | opaque handle via `DoAction` |
| Update count | int and long | `SQLRowCount` | `rows_affected` int64 | `Publisher<Long>` | `rowcount` | `RowsAffected` int64 | `RecordsAffected` | `DoPutUpdateResult{record_count}` |
| Forward-only iteration with typed access | yes | yes | Arrow batches | yes | yes | `Scan` | yes | Arrow batches |
| Scrollable / updatable cursors | yes | yes (+ bookmarks) | no | no | optional `scroll` | no | no | no |
| Multiple result sets | `getMoreResults` | `SQLMoreResults` | — | one `Result` per statement; `Segment` | `nextset` | `NextResultSet` | `NextResult` | — |
| Generated keys | flags / columns | — | — | `returnGeneratedValues` | `lastrowid` | `LastInsertId` | provider | — |
| Stored-procedure OUT params | `CallableStatement` | Level 2 | — | `Parameters.out` | `callproc` | `sql.Out` | `ParameterDirection` | — |
| Savepoints | yes | via SQL | — | yes | — | — | — | `ActionBeginSavepoint` |
| Batching | `addBatch` | parameter arrays | bulk ingest | `Batch`, `Statement.add()` | `executemany` | — | `DbBatch` | `CommandStatementIngest` |
| LOB streaming | streams, `Blob/Clob` | `SQLGetData/SQLPutData` | — | `Blob/Clob` publishers | — | `RawBytes` | `GetStream/GetTextReader` | — |
| Capability metadata | ~177 `DatabaseMetaData` methods | ~175 `SQLGetInfo` types | `GetInfo` Arrow table | `ConnectionMetadata` (product name/version) | — | — | `GetSchema` collections | `SqlInfo` IDs 0–500 server, 500–1000 syntax |
| Catalog metadata | 25 queries | 11 functions | `GetObjects` depth levels, `GetTableSchema`, `GetTableTypes`, `GetStatistics` | — | — | — | `GetSchema` | `CommandGet{Catalogs, DbSchemas, Tables, TableTypes, SqlInfo, XdbcTypeInfo, PrimaryKeys, ExportedKeys, ImportedKeys, CrossReference}` |
| Escape syntax | `{fn} {call} {d} {t} {ts} {oj} {escape} {limit}` + MR5 `{\\ \\}` | same set | — | — | — | — | — | — |
| Timeouts | login / network / query | `LOGIN_TIMEOUT`, `CONNECTION_TIMEOUT`, `QUERY_TIMEOUT` | option | `connectTimeout`, `statementTimeout`, `lockWaitTimeout` | — | `context` | `CommandTimeout` | `SqlInfo` |
| Cancellation | `Statement.cancel()` | `SQLCancel[Handle]` | `ConnectionCancel`, `StatementCancel` (1.1) | subscription cancel | — | `context` | `Cancel` | `CancelFlightInfo` |
| Error = message + vendor code + SQLSTATE | yes | yes (+ diagnostic fields) | yes (+ details) | yes | provider | provider | provider | gRPC status |
| Connection validation | `isValid` | `SQL_ATTR_CONNECTION_DEAD` | — | `validate(depth)` | — | `Pinger` / `Validator` | — | — |
| Async | none | 3.8 polling / notification | — | native (Reactive Streams) | — | contexts | `*Async` | gRPC streams |
| Transport | rows | rows / bound buffers | Arrow columnar | rows | rows | rows | rows | Arrow columnar |
| Pooling | `javax.sql` interfaces | DM / driver-aware (3.8) | — | r2dbc-pool | — | built into `DB` | provider | — |
| XA / 2PC | `XADataSource` | — | — | — | `tpc_*` | — | `EnlistTransaction` | — |

The common core present in every API: open from a string; begin/commit/rollback with auto-commit
default; isolation selection; direct and prepared execution with positional parameters; update
count; forward-only row iteration with per-column typed access and NULL detection; column
name/type/precision/scale/nullability metadata; multiple result sets; timeouts; cancellation;
error with message + vendor code (+ SQLSTATE in JDBC, ODBC, R2DBC, ADBC). Index base and
placeholder style are the two most common sources of bridge bugs: a Rust core should pick one
(0-based; `?` or `$n`) and document the translation for each bridge.

### SQL type system and SQLSTATE

ISO/IEC 9075-2:2023 (SQL/Foundation, ~1715 pp.) is paywalled; its predefined-type rules are
inherited largely unchanged from SQL-92 §4.2–4.6 **[third party, checked 2026-11-09]**
(https://www.contrib.andrew.cmu.edu/~shadow/sql/sql1992.txt): exact numerics with precision and
non-negative scale (assignment to an exact target rounds or truncates to target scale,
implementation-defined), approximate numerics with binary precision, DATE/TIME/TIMESTAMP each
optionally WITH TIME ZONE with fractional-seconds precision, year-month and day-time INTERVAL
classes with leading-field precision, CAST for explicit conversion. Later editions added BOOLEAN,
BIGINT, BLOB/CLOB, ARRAY/MULTISET, ROW, XML and, in SQL:2023, the JSON type (T801/T802/T803) plus
SQL/JSON item methods (T865–T878), `ANY_VALUE`, `GREATEST/LEAST`, underscores in numeric literals,
and a new Part 16 SQL/PGQ **[third party]** (https://peter.eisentraut.org/blog/2023/04/04/sql-2023-is-finished-here-is-whats-new,
https://modern-sql.com/standard/2023). Whether SQL:2023 kept DECFLOAT was not confirmed from a
primary source.

ODBC's view: `SQL_DECIMAL(p,s)` precision >= p, `SQL_NUMERIC(p,s)` exactly p; `SQL_FLOAT(p)` is
REAL (24) or DOUBLE (53); TINYINT/BIGINT/BINARY family "have no corresponding SQL-92 type";
`SQL_TIMESTAMP_STRUCT.fraction` is billionths of a second. JDBC's view adds JDBC-only codes and
has **no INTERVAL type code** (intervals surface as vendor `OTHER` or strings).

SQLSTATE is a 2-char class + 3-char subclass shared verbatim by ODBC, JDBC's exception
subclasses, ADBC's `sqlstate[5]`, R2DBC's `getSqlState()` and PostgreSQL's error codes: `00`
success, `01` warning, `02` no data; `07` dynamic SQL, `08` connection (001 client cannot connect,
003 does not exist, 006 failure, 007 transaction resolution unknown), `0A` feature not supported,
`22` data exception (001 right truncation, 003 numeric out of range, 007 invalid datetime format,
012 division by zero, ...), `23` integrity, `24` invalid cursor state, `25` invalid transaction
state, `28` invalid authorization, `3B` savepoint, `40` transaction rollback (001 serialization
failure), `42` syntax or access rule, `HY` CLI-specific (`HY010` function sequence error, `HYC00`
optional feature not implemented); implementation-defined values use I–Z or 5–9 **[third party]**
(https://en.wikipedia.org/wiki/SQLSTATE). PostgreSQL 18 Appendix A has 43 classes with
`P`-subclasses (`42P01 undefined_table`, `40P01 deadlock_detected`, `08P01 protocol_violation`)
and PostgreSQL-only classes `F0`, `P0`, `XX`, `53/54/55/57/58` **[vendor]**
(https://www.postgresql.org/docs/current/errcodes-appendix.html). The design consequence: store
SQLSTATE as `[u8; 5]`, not an enum, with helper classification into warning/no-data/error and the
JDBC families, since every API accepts vendor-defined classes.

The minimal "type parity" set is the union of ODBC SQL type identifiers (incl. 13 intervals, GUID,
W-types), `java.sql.Types` (incl. BOOLEAN, ROWID, N-types, SQLXML, REF_CURSOR, TZ types,
DECFLOAT=2015, JSON=2016) and the Arrow logical types needed for ADBC/Flight SQL; the ISO list is a
strict subset of that union.

### Connection-URL conventions

Two families exist **[vendor, checked 2026-11-09]**:

| Family | Examples | Rules that differ |
|---|---|---|
| URI-shaped | JDBC `jdbc:subprotocol:subname`; PgJDBC `jdbc:postgresql://host1:port1,host2:port2/database?k=v` (~60 parameters; defaults localhost, 5432, database = user) (https://jdbc.postgresql.org/documentation/use/); Connector/J `jdbc:mysql:[loadbalance:\|replication:]//hosts/db?props`, `mysqlx:`, `jdbc:mysql+srv:`, per-host `address=(host=h)(port=p)` and bracketed sublists; keys **case-sensitive**, leftmost credentials win (https://dev.mysql.com/doc/connector-j/en/connector-j-reference-jdbc-url-format.html); MySQL X DevAPI `mysqlx://` "similar to but not identical to" RFC 3986, parameters case-insensitive, each at most once (https://dev.mysql.com/doc/refman/8.4/en/connecting-using-uri-or-key-value-pairs.html); libpq `postgresql://[userspec@][hostspec][/dbname][?paramspec]` (also `postgres://`), multi-host with matching port lists, Unix socket via percent-encoded path, `ssl=true` -> `sslmode=require` for JDBC compatibility, ~55 keywords, **last non-empty value wins** in `PQconnectdbParams` (https://www.postgresql.org/docs/current/libpq-connect.html); R2DBC `r2dbc[s]:driver[:protocol]://...`; ADBC `uri` option with the scheme selecting the driver | percent-encoding (RFC 3986), IPv6 in brackets, multi-host lists, `?`/`&` options |
| `keyword=value;` | ODBC `DSN=…;DRIVER={…};UID=;PWD=;` — keywords case-insensitive, **first occurrence wins**, braces around values containing `;`, `DSN`/`FILEDSN` mutually exclusive; libpq `host=localhost port=5432 dbname=mydb` with single quotes and backslash escapes; ADO.NET `DbConnectionStringBuilder` | duplicate-key rule inverted vs libpq |

A scheme of the form `karez:<driver>[+<protocol>]://[user[:password]@]host1[:port1][,host2[:port2]]/[database][?key=value&…]`
can round-trip to all URI-shaped families and be lowered to ODBC/libpq keyword strings by a
per-driver keyword table (`user -> UID/user`, `password -> PWD/password`, `database ->
DATABASE/dbname`, `host -> SERVER/host`, `sslmode -> Encrypt/sslmode`). It must define
percent-encoding, IPv6 brackets, Unix-socket hosts, a duplicate-key policy (the notes recommend
"last wins" like libpq and rejecting conflicting case variants like Connector/J) and a `driver=`
override like ADBC/R2DBC. The Oracle thin (`jdbc:oracle:thin:@//host:port/service`) and SQL Server
(`jdbc:sqlserver://host:port;databaseName=…;`) grammars, which mix both styles, were not opened.

### Parity checklist skeleton

Parity should be declared per standard and per level, not as one boolean: (a) JDBC 4.3/4.5
surface parity; (b) ODBC Core / Level 1 / Level 2 feature parity using Microsoft's numbered
features 101–109 and 201–211; (c) ADBC 1.1.0 function-table parity (57 entry points, option keys,
fixed metadata schemas); (d) R2DBC 1.0 TCK parity (~35 scenarios); plus a common type / SQLSTATE /
URL layer. The only normative compliance statements that exist publicly are `jdbcCompliant()`
(JDBC), "ODBC Drivers must implement the functions in the Core interface conformance level"
(ODBC), the driver-manager contract (ADBC) and Chapter 4 (R2DBC). None defines a numeric
"percentage of parity"; the list below is the notes' synthesis.

1. **Connection layer**: open from URL/options; `isValid` / `validate(LOCAL|REMOTE)` / `Ping`;
   `abort`; network vs login vs statement timeout; client info (`ApplicationName`, `ClientUser`,
   `ClientHostname`); catalog/schema switching; read-only mode; pool hooks
   (`beginRequest/endRequest`, `SQL_ATTR_RESET_CONNECTION`, R2DBC `Lifecycle`, Go
   `SessionResetter`).
2. **Transactions**: auto-commit default on; begin/commit/rollback; isolation = union {NONE,
   READ_UNCOMMITTED, READ_COMMITTED, WRITE_COMMITTED (Go), REPEATABLE_READ, SNAPSHOT (ADBC/Go),
   SERIALIZABLE, LINEARIZABLE (ADBC/Go)}; savepoints (named/unnamed, release);
   `TransactionDefinition`-style attributes; capability flags (`supportsTransactions`,
   `SQL_TXN_CAPABLE` DML/DDL semantics, `dataDefinitionCausesTransactionCommit`); XA optional.
3. **Statements**: direct execute with multiple results; prepared (positional `?`/`$n`, named
   `:name` optional); callable (IN/OUT/INOUT, return value, REF CURSOR); batch with per-command
   status incl. `SUCCESS_NO_INFO`/`EXECUTE_FAILED` and large counts; parameter arrays
   (`PARAMSET_SIZE` semantics); generated keys (flag / column indexes / column names); max
   rows/field size; fetch size/direction hints; escape processing on/off plus the JDBC 4.5
   `{\\ … \\}` suppression; `nativeSQL`; cancel from another thread; poolable/closeOnCompletion
   hints; parameter metadata.
4. **Results**: forward-only streaming default; optional scroll-insensitive/sensitive,
   updatable, holdability over commit, bookmarks (ODBC L2); row-at-a-time typed access with the
   B-6 conversion matrix; columnar access as Arrow batches (ADBC parity); LOB streaming in
   chunks; `wasNull`; warnings chain; full column metadata; multiple result sets with update
   counts interleaved.
5. **Metadata**: the 25 JDBC catalog queries / 11 ODBC catalog functions / ADBC `GetObjects`
   depth levels / Flight SQL `CommandGet*` as one `Catalog` API returning tables (ideally Arrow)
   with the JDBC/ODBC column layouts; a capability map covering `SQLGetInfo`'s ~175 info types
   and `DatabaseMetaData`'s ~150 `supports*/getMax*` methods, with SQL-conformance and
   interface-conformance level reporting.
6. **Types**: enum = union of ODBC SQL type ids + `java.sql.Types` + Arrow logical types;
   conversion tables B-1…B-6 and ODBC Appendix D as the normative matrix;
   `SQL_NUMERIC_STRUCT`-style exact decimals; nanosecond timestamps; time zones via offset types.
7. **Errors**: `{message, sqlstate: [u8;5] | None, vendor_code: i32, status:
   AdbcStatusCode-like, details: Vec<(key, bytes)>, cause, next}` classified into JDBC's
   transient/non-transient families and ODBC's warning/no-data/error ranking; diagnostic extras
   (row/column number, server/connection name, dynamic function).
8. **Loading/bridging**: C ABI exports for ODBC (Core function set, "03.80", registry/odbcinst
   keys) and ADBC (`AdbcDriverInit`, versioned table, TOML manifest); R2DBC/JDBC adapters via
   JNI/FFM are facades, but their URL/option names should be accepted.
9. **Async**: R2DBC-style demand-driven streams with cancellation-releases-resources semantics;
   ODBC 3.8 async maps to futures with `SQL_STILL_EXECUTING` ≈ `Poll::Pending`.

The two missing primary texts (JDBC 4.3 Chapter 6 and ISO 9075-3:2023) should be acquired before
this checklist is frozen.

## 5. Wire protocols: what each costs

### PostgreSQL frontend/backend protocol 3.0 and 3.2

The most completely documented open wire protocol of the group **[vendor, checked 2026-11-09]**
(https://www.postgresql.org/docs/current/protocol-overview.html,
https://www.postgresql.org/docs/current/protocol-message-formats.html,
https://www.postgresql.org/docs/current/protocol-flow.html).

- **Framing**: every message is a 1-byte type plus an Int32 length that "includes itself, but not
  the message-type byte"; the startup message alone has no type byte; integers are big-endian;
  format codes 0 (text) and 1 (binary), "all other codes are reserved"; "binary representations
  for complex data types might change across server versions".
- **Versions**: 3.2 is current (PG 18+) and changes only the cancel key (4 bytes -> variable, up
  to 256 bytes; "the server currently sends at most 32 bytes") plus NegotiateProtocolVersion
  minor-version downgrade (introduced 9.3.21); 3.1 was "reserved, never used ... skipped because
  old pgbouncer versions incorrectly claimed to support it"; "libpq still uses 3.0 by default for
  compatibility". libpq `max_protocol_version` defaults to 3.0 "unless the connection string
  specifies a feature that relies on a higher protocol version"
  (https://www.postgresql.org/docs/current/libpq-connect.html). PG 18 release notes: 256-bit
  cancel keys, `PQfullProtocolVersion()`, OAuth (`oauth` in pg_hba.conf), MD5 password
  deprecation with `md5_password_warnings`, `sslkeylogfile`, COPY `REJECT_LIMIT`, SCRAM
  pass-through (https://www.postgresql.org/docs/18/release-18.html).
- **Startup and TLS**: SSLRequest (Int32 8, Int32 80877103) is answered by one byte `S`/`N`
  (read exactly one byte, CVE-2021-23222); GSSENCRequest is 80877104. **Direct SSL** (PG 17): the
  client starts TLS immediately and "must send ALPN `postgresql`"; libpq `sslnegotiation=direct`
  is "only allowed with sslmode=require or higher" and saves one round trip
  (https://www.postgresql.org/docs/17/release-17.html). `sslmode` default `prefer`;
  `channel_binding` default `prefer`; `sslcompression` default 0 and "PostgreSQL 14 disabled
  compression completely in the backend"; `require_auth` accepts `password, md5, gss, sspi,
  scram-sha-256, oauth, none`.
- **Authentication** messages start `R` + Int32 length + Int32 code: Ok=0, MD5Password=5
  (4-byte salt), SASL=10, SASLContinue=11, SASLFinal=12. MD5 is "deprecated, scheduled for
  removal". SCRAM: the server advertises `SCRAM-SHA-256-PLUS` and `SCRAM-SHA-256` when built with
  SSL; channel-binding type `tls-server-end-point`; OAUTHBEARER (PG 18) has no `-PLUS` variant
  and uses a two-connection discovery flow; "libpq does not cache tokens"
  (https://www.postgresql.org/docs/current/sasl-authentication.html).
- **Extended query**: Parse (`P`: statement name, query, Int16 n param OIDs), Bind (`B`: portal,
  statement, Int16 parameter format codes, Int16 N params each Int32 length (-1 NULL) + bytes,
  Int16 result format codes), Describe (`D`), Execute (`E`: portal, Int32 max rows), Sync (`S`)
  and Flush (`H`) are 5 bytes. RowDescription (`T`): per field name, table OID, attnum, type OID,
  typlen, typmod, format code. **DataRow (`D`): Int16 column count; per column Int32 length (-1
  NULL) + bytes.** After an error the backend "discards messages until Sync"; exactly one
  ReadyForQuery per Sync. Named statements live for the session; named portals until end of
  transaction. Protocol limits: Int16 column, parameter and format counts (65,535) — pgJDBC's
  `min(65535 / parametersPerRow, 32768)` batch cap is a direct consequence; the Int32 length
  bounds a message at 2 GiB (the server's actual 1 GB cap was not confirmed from a fetched page).
- **Pipelining** needs no server changes: send several requests without waiting; "Track
  completion by counting ReadyForQuery messages, not CommandComplete". libpq pipeline mode
  (14+): `PQenterPipelineMode`, `PQpipelineSync` (Sync + flush), `PQsendPipelineSync` (no flush),
  `PQsendFlushRequest`; `PQexec`, `PQsendQuery` (simple protocol), multi-statement strings and
  COPY are disallowed; blocking mode can deadlock when "the client sends enough queries to fill
  both its output buffer and the server's receive buffer before it reads input", so non-blocking
  + poll is recommended; "100 statements at 300 ms RTT take about 30 s without pipelining, and
  possibly about 0.3 s with it"; the cost is memory, "no meaningful overhead per query"
  (https://www.postgresql.org/docs/current/libpq-pipeline-mode.html).
- **Binary encodings** (from `postgres-protocol` and pgx, **[third party, checked 2026-11-09]**,
  https://raw.githubusercontent.com/sfackler/rust-postgres/master/postgres-protocol/src/types/mod.rs,
  https://raw.githubusercontent.com/jackc/pgx/master/pgtype/numeric.go): bool 1 byte;
  int2/int4/int8 big-endian; float4/float8 IEEE big-endian; text/varchar/bpchar/name raw UTF-8
  with no prefix; bytea raw; timestamp/timestamptz i64 microseconds since 2000-01-01; date i32
  days since 2000-01-01; time i64 microseconds; uuid 16 raw bytes; inet/cidr 8 or 20 bytes;
  array header 12 + 8·ndim then per element i32 length + data; range flags byte + bounded sides;
  interval i64 microseconds + i32 days + i32 months = 16 bytes; **numeric**: u16 ndigits, i16
  weight, u16 sign (0x0000 positive, 0x4000 negative, 0xC000 NaN, 0xD000 +Inf, 0xF000 -Inf), i16
  dscale, then ndigits x i16 base-10000 digits — pgx's decoder "allocates a big.Int for the
  accumulator and a new big.Int for each four-digit group". jsonb's binary form (1-byte version
  prefix + JSON text) and json/money/infinity sentinels were not in the fetched file.
- **COPY**: CopyInResponse `G` / CopyOutResponse `H` carry the overall format and per-column
  format codes; CopyData `d` ("frontend chunks may split rows arbitrarily; backend sends one row
  per message"); "Flush and Sync are ignored during copy-in". Binary COPY file: 11-byte
  signature `PGCOPY\n\377\r\n\0`, Int32 flags, Int32 extension length, per tuple Int16 field count
  then per field Int32 length + data "in the same representation as the binary send function";
  trailer Int16 -1; "no alignment padding"; "Binary is faster than text and CSV, but is less
  portable" (https://www.postgresql.org/docs/current/sql-copy.html). The page says "15 bytes of
  fixed fields" but the listed fields sum to 19.
- **Cancel** opens a new connection and sends CancelRequest (Int32 length = 12 + key_len, Int32
  80877102, PID, key); the server "closes the connection, and sends no reply".

### MySQL / MariaDB client/server protocol

Packet-oriented: 3-byte little-endian length + 1-byte sequence id, 16 MiB max per packet, so
large payloads split **[vendor, checked 2026-11-09]**
(https://dev.mysql.com/doc/dev/mysql-server/latest/page_protocol_connection_phase.html,
https://mariadb.com/kb/en/connection/, https://dev.mysql.com/doc/dev/mysql-server/latest/page_protocol_com_stmt_prepare.html,
https://dev.mysql.com/doc/dev/mysql-server/latest/page_protocol_com_stmt_execute.html,
https://dev.mysql.com/doc/dev/mysql-server/latest/page_protocol_binary_resultset.html).

- **Handshake**: the server's default plugin challenge is sent in the handshake "to save a round
  trip"; mismatches produce AuthSwitchRequest; extra server packets are AuthMoreData "prefixed
  with 0x01, so they can't be confused with ERR (0xFF) or OK (0x00)"; `mysql_native_password`
  default 4.1–5.7, `caching_sha2_password` default from 8.0, "`mysql_native_password` was
  removed from the 9.0 server" (section 8). MariaDB 10.x prefixes its version string with
  `5.5.5-`; 11.0+ does not. Native password token = `SHA1(passwd) ^ SHA1(seed + SHA1(SHA1(passwd)))`.
- **Capability bits**: CLIENT_MYSQL 0x1, FOUND_ROWS 0x2, COMPRESS 0x20, LOCAL_FILES 0x80,
  PROTOCOL_41 0x200, SSL 0x800, TRANSACTIONS 0x2000, MULTI_STATEMENTS 0x10000, MULTI_RESULTS
  0x20000, PLUGIN_AUTH 0x80000, CONNECT_ATTRS 0x100000, SESSION_TRACK 0x800000, DEPRECATE_EOF
  0x1000000, OPTIONAL_RESULTSET_METADATA 0x2000000 (not used by MariaDB), ZSTD 0x4000000.
  MariaDB-only bits above 31: PROGRESS 1<<32, STMT_BULK_OPERATIONS 1<<34, EXTENDED_METADATA
  1<<35, **CACHE_METADATA 1<<36 ("permits skipping metadata")**, BULK_UNIT_RESULTS 1<<37.
- **Compression** is a separate layer (zlib or zstd; "If both CLIENT_COMPRESS and
  CLIENT_ZSTD_COMPRESSION_ALGORITHM are set, zlib is used") that re-frames packets inside a
  7-byte compressed-packet header, adding a copy (header details on an unfetched sub-page)
  (https://dev.mysql.com/doc/dev/mysql-server/latest/page_protocol_basic_compression.html).
- **Text vs binary**: COM_QUERY returns every value as a length-encoded string. COM_STMT_PREPARE
  (`0x16`) costs a round trip and returns statement_id, num_columns, num_params plus column
  definitions; `LOAD DATA` cannot be prepared. COM_STMT_EXECUTE (`0x17`): statement_id, flags,
  iteration_count (always 1), NULL bitmap `(parameter_count + 7) / 8`, new_params_bind_flag, per
  param type int2, then values; the worked example is an 18-byte payload for one VARCHAR
  `"foo"`. **Binary row**: header 0x00, NULL bitmap `(column_count + 7 + 2) / 8` bytes with a
  2-bit offset, then only non-NULL values: TINY 1, SHORT/YEAR 2, LONG/INT24 4, LONGLONG 8, FLOAT
  4, DOUBLE 8; DECIMAL/NEWDECIMAL and all string/BLOB/JSON/BIT/GEOMETRY types are lenenc
  strings; DATE/DATETIME/TIMESTAMP 1 length byte + 0/4/7/11 bytes; TIME 1 + 0/8/12. **DECIMAL is
  text in both modes** — a decode trap for numeric-heavy workloads. WL#3359: prepared statements
  cost "an extra round trip for the prepare phase" and "using MySQL's binary protocol for result
  sets is almost always good for performance" (https://dev.mysql.com/worklog/task/?id=3359).
- **No protocol-level pipelining.** Boost.MySQL: "The protocol has no explicit pipelining. The
  server sees a pipeline as a sequence of unrelated requests"; every stage runs even if an
  earlier one fails; stages cannot depend on each other "because the protocol can't express
  that"; once a resultset is obtained "you must read it entirely before engaging in any
  subsequent operation"; the sequence id "is reset to 0 when a new command begins" **[third
  party, checked 2026-11-09]** (https://www.boost.org/doc/libs/latest/libs/mysql/doc/html/mysql/pipeline.html).
  MariaDB Connector/J 3.1.0: "pipelining writes a single byte array of multiple packets to the
  socket" (CONJ-1015).
- **Bulk paths**: `LOAD DATA LOCAL INFILE` (server sends a `0xFB` packet with the filename; the
  vendor page's example payload is `fb` + `/etc/passwd`, which illustrates the security risk —
  section 8). MariaDB **COM_STMT_BULK_EXECUTE** (`0xFA`): statement id, bulk flags
  (SEND_UNIT_RESULTS 64, SEND_TYPES_TO_SERVER 128), optional per-param type bytes, then per row
  one indicator byte per parameter (0 value follows, 1 NULL, 2 DEFAULT, 3 IGNORE); a statement
  that returns a result set produces an error (https://mariadb.com/kb/en/com_stmt_bulk_execute/).
  This is the only true server-side batch path in the MySQL family.
- **Published numbers**: MariaDB's Node.js connector benchmark (Feb 2023, updated Jan 2024; two
  4-vCPU/16 GB DigitalOcean hosts, MariaDB 10.6, Node 18), ops/s mysql | mysql2 | mariadb: simple
  insert 7,064 | 6,465 | 8,532; select 1 row of 100 ints 2,739 | 2,405 | 5,651; same with
  prepared statement — | 2,473 | 10,533; select 1000 rows 1,141 | 1,475 | 2,185; **batch insert
  of 100 rows 219 | 174 | 6,490**; attributed to MariaDB 10.6 skipping unchanged column metadata
  (MDEV-19237) and bulk execution **[vendor]** (https://mariadb.com/resources/blog/which-node-js-connector-is-best-to-use/).
  Boost.MySQL 1.87 vs libmysqlclient 8.4.4 vs libmariadb 10.11.11 (prepared statements only):
  "the three libraries perform similarly, which ... is expected from a correctly implemented
  binary protocol"; numbers only in an image **[third party]**
  (https://www.boost.org/doc/libs/master/libs/mysql/doc/html/mysql/benchmarks.html).
  go-sql-driver/mysql (2013): most `Rows.Next` allocations were `runtime.convT2E` boxing a
  `[]byte` into `interface{}`, roughly 32 bytes per scanned string column even with
  `sql.RawBytes` (https://github.com/go-sql-driver/mysql/pull/52). No published throughput for
  mysql_async or current go-sql-driver was found.
- **X Protocol** (protobuf over the X Plugin) was not read beyond its index page.

### Microsoft SQL Server: TDS 7.4 and 8.0

A packetised token stream **[vendor and third party, checked 2026-11-07/09]** (MS-TDS revision
42.0, 2026-06-17, https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-tds/b46a581a-39de-4745-b076-ec4dbb7d13ec;
layouts from go-mssqldb and tiberius source, https://raw.githubusercontent.com/microsoft/go-mssqldb/main/tds.go,
.../token.go, .../types.go, https://docs.rs/crate/tiberius/latest/source/src/tds/codec/header.rs).

- **Packets**: 8-byte header (type, status, 16-bit big-endian length, SPID, ...); default packet
  size 4,096, negotiable 512–32,767; go-mssqldb caps encrypted connections at 16,383. Packet
  types SQLBatch 0x01, RPCRequest 0x03, Reply 0x04, Attention 0x06, BulkLoadBCP 0x07,
  FedAuthToken 0x08, TransMgrReq 0x0E, Login7 0x10, SSPIMessage 0x11, Prelogin 0x12. TDS
  versions 7.4 = 0x74000004 (SQL Server 2012+), 8.0 = 0x08000000 (2022+).
- **PRELOGIN** (option table: VERSION, ENCRYPTION, INSTOPT, THREADID, MARS, TRACEID, FEDAUTHREQUIRED,
  NONCEOPT). "In TDS 7.x ... SSL negotiation between client and server happens immediately after
  the PRELOGIN packet"; "The SSL payloads MUST be transported as data in TDS packets with the
  message type set to 0x12"; "In TDS 8.0, because the TLS session is already established, the
  encryption value that is sent by the client is ignored"
  (https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-tds/60f56408-0188-4cd5-8b90-25c6f2423868).
  **TDS 8.0** (`Encrypt=strict`): TLS first with ALPN `tds/8.0`, then encrypted PRELOGIN and
  login; "only TDS 7.x supports non-encrypted (optional) communication"; TLS 1.3 reachable only
  in 8.0; JDBC 11.2.0+ (https://learn.microsoft.com/sql/relational-databases/security/networking/tds-8).
- **LOGIN7**: 94-byte fixed header with offset/length pointers into UCS-2LE strings; password
  "swap the nibbles, then XOR with 0xA5"; FEATUREEXT ids SESSIONRECOVERY 0x01, FEDAUTH 0x02,
  COLUMNENCRYPTION 0x04, GLOBALTRANSACTIONS 0x05, AZURESQLSUPPORT 0x08, DATACLASSIFICATION 0x09,
  UTF8SUPPORT 0x0A.
- **Tokens**: COLMETADATA 0x81 (uint16 count, per column usertype, flags, type info, B_VARCHAR
  name), **ROW 0xD1** (no bitmap), **NBCROW 0xD2** (null bitmap `(columns+7)/8` then only
  non-null values), DONE 0xFD / DONEPROC 0xFE / DONEINPROC 0xFF, ERROR 0xAA, INFO 0xAB, ENVCHANGE
  0xE3 (incl. routing 20), RETURNVALUE 0xAC, FEDAUTHINFO 0xEE.
- **Types**: INTN/BITN/FLTN/DECIMALN/MONEYN/DATETIMEN nullable families; DECIMALN per value a
  length byte, sign byte and 4/8/12/16-byte little-endian magnitude (**max 16 bytes, so a
  128-bit integer suffices, no big-integer arithmetic**); DATETIME int32 days since 1900 + uint32
  1/300-second ticks; DATETIME2 3/4/5-byte time + 3-byte days since 0001-01-01; MONEY swapped
  dwords; GUID with the first three groups byte-swapped; collation 5 bytes; **NVARCHAR is UCS-2LE**
  with uint16 length (0xFFFF NULL), PLP (uint64 total + uint32 chunks) above 8,000 bytes;
  VARCHAR/CHAR/TEXT converted "using the column's collation via cp.CharsetToUTF8".
- **Bulk load**: "The client sends the INSERT BULK SQL statement and then sends a COLMETADATA
  token that describes the raw data. Multiple rows of binary data are then sent ... The stream
  is the same as if the data were being selected from the server rather than being sent to the
  server" (https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-tds/88176081-df75-4b24-bcfb-4c16ff03cbfa).
- tiberius 0.13.0 does not mention TDS 8.0, MARS or Entra tokens on its crate page; separate
  `mssql-tls` and `tds-protocol` crates mention ALPN for TDS 8.0; tedious (Node) documents
  7.1–7.4 only. No published throughput numbers for tiberius, FreeTDS, mssql-jdbc or go-mssqldb
  were found.

The decode costs TDS adds that no other protocol here has: UCS-2 -> UTF-8 transcoding for every
N-string (bandwidth and CPU), code-page conversion for non-UTF-8 VARCHAR collations, PLP chunk
reassembly for MAX types, and an 8-byte header to strip every 4,096 bytes whose boundary may
fall mid-token (so the parser must run over a logical byte stream; a 1 MB result at the default
is ~256 packets; raising to 32,767 cuts header overhead 8x).

### SQLite: in-process C API

No wire protocol; the cost model is FFI calls and copies **[vendor, checked 2026-11-09]**
(https://www.sqlite.org/c3ref/funclist.html, https://www.sqlite.org/c3ref/column_blob.html,
https://www.sqlite.org/threadsafe.html).

- Accessor rules: call `sqlite3_column_type` before any accessor (its result is "only meaningful
  if no automatic type conversions have occurred"); `column_text/blob` before `column_bytes`;
  pointers are valid "until a type conversion occurs ... or until sqlite3_step() or
  sqlite3_reset() or sqlite3_finalize() is called"; UTF-8 <-> UTF-16 conversions reallocate;
  `column_blob` returns NULL for a zero-length blob; only `_blob/_text/_bytes` can fail with OOM
  and then look like NULL. So a minimum of two or three FFI calls per column per row: for 10
  columns ≈ 21–31 calls per row (derived). `sqlite3_bind_*` with `SQLITE_STATIC` is zero-copy;
  `SQLITE_TRANSIENT` copies; `sqlite3_prepare_v3` adds `SQLITE_PREPARE_PERSISTENT`.
- Threading: Serialized (mutex per API call) "is the default"; `SQLITE_OPEN_NOMUTEX` selects
  multi-thread per connection; "Runtime overrides start-time, and start-time overrides
  compile-time". The per-call mutex is pure overhead when a Rust `Connection` is already `!Sync`.
- rusqlite's `bundled` compiles SQLite 3.54.0 with the `cc` crate and is "not a default feature";
  dynamic linking requires SQLite >= 3.45.3; bundling is "a good option when linking to SQLite is
  complicated, such as on Windows". Bundling is the de facto Rust standard (rusqlite, sqlx, Turso).
- **Turso Database** (pure-Rust rewrite, ex-Limbo, MIT): "runs in production today at multiple
  organizations" (Turso Cloud, Kin, Spice.ai), not yet 1.0, targets SQLite 3.50.4 for dialect,
  file format and C API, "compatibility is not yet 100%"; `BEGIN CONCURRENT` MVCC; io_uring on
  Linux; PostgreSQL compatibility, encryption at rest, FTS and multi-process WAL coordination are
  experimental **[vendor, checked 2026-11-09]** (https://raw.githubusercontent.com/tursodatabase/turso/main/README.md).
  Third-party summaries call it beta and not a drop-in replacement. Not a safe production target
  for a general driver in 2026; a trait-level abstraction that can host either `libsqlite3-sys`
  or Turso behind the same API is the practical option.
- No published measurement of FFI overhead per `sqlite3_column_*` call or of serialized-mode
  mutex cost was found. diesel's metrics put rusqlite at 0.37 µs per trivial query (section 2).

### Oracle, DuckDB, ClickHouse and the HTTP+Arrow warehouses

**Oracle**: the wire protocol (Net8/TNS, TTC) remains undocumented, but Oracle itself now ships
"thin" reimplementations: python-oracledb Thin mode (default) "connects directly to Oracle
Database 12.1 or later" over Oracle Net; Thick mode loads Oracle Client 19+ (Thin lacks native
network encryption/checksumming, needs PEM wallets, one mode per process); tuning: `arraysize`
default 100, `prefetchrows` default 2, "10,000 rows with the defaults cost 101 round-trips, 11
with `arraysize=1000`", statement cache default 20 per connection **[vendor, checked
2026-11-09]** (https://python-oracledb.readthedocs.io/en/latest/user_guide/introduction.html,
.../appendix_b.html, .../tuning.html). Oracle's Go driver `github.com/oracle/go-oracledb`
("Native Go implementation of Go's sql/driver package", DB 19c+, UPL-1.0, v0.0.1 2026-08-18;
packages `ttc`, `network/transport`, `network/session`; NUMBER(p,s) returned as string)
(https://pkg.go.dev/github.com/oracle/go-oracledb). Third-party MIT-licensed go-ora has
implemented TNS independently for years; the only legal remark found is a 2015 golang-nuts
comment, "I don't know what the legal ramifications would be" **[third party]**
(https://groups.google.com/g/golang-nuts/c/RNC0JwZDQtw). **The legal position of a third-party
Oracle wire implementation is unknown.** Thin fetch cost is governed by `arraysize`/`prefetchrows`
round trips, not per-row encoding.

**DuckDB**: C API "modeled somewhat following the SQLite C API"; stable C API 2.0.0; "C API v2"
in `duckdb_v2.h` "is not yet stable"; the row accessors `duckdb_value_*`, `duckdb_column_data`,
`duckdb_nullmask_data`, `duckdb_result_get_chunk` and streaming variants are deprecated since
1.0.0 in favour of `duckdb_fetch_chunk` data chunks (2,048 rows by default) with
`duckdb_vector_get_data` / `duckdb_vector_get_validity` ("may be NULL when all values are
valid"); the appender (`duckdb_append_data_chunk`) is the bulk path; `duckdb_bind_value` (0.9.0)
supersedes typed binders; pending-result execution; new in 1.5.6:
`duckdb_unsafe_vector_assign_string_element_len` (skips UTF-8 validation) **[vendor, checked
2026-11-09]** (https://duckdb.org/docs/2.0/clients/c/overview.html,
https://duckdb.org/docs/2.0/clients/c/api.html). The Arrow functions and `duckdb_string_t` inline
threshold are in the unread tail of the reference.

**ClickHouse native TCP**: packet type VarUInt; little-endian; String = VarUInt length + bytes;
chunked framing (rev 54470+) = u32 LE size + payload; ClientHello/Query/Data/Cancel/Ping
client-side, Hello/Data/Exception/Progress/Pong/EndOfStream/ProfileInfo/... server-side. **Block**:
BlockInfo, VarUInt num_columns, VarUInt num_rows, per column name, type, custom-serialization
byte, then data "column by column, not row by row": ints/floats fixed LE; String per-value
lengths below rev 54492 and an offsets layout (UInt64 cumulative end offsets then bytes) at
54492+; Nullable(T) = null map then values; Array = offsets then flattened; LowCardinality dict +
keys; Date UInt16, DateTime UInt32, DateTime64 Int64; Decimal 4/8/16/32 bytes; UUID 16 bytes with
each half byte-reversed. **Compression frame**: 16-byte CityHash128 over the 9-byte header +
body, method byte 0x02 None / 0x82 LZ4 / 0x90 ZSTD, UInt32 compressed and uncompressed sizes;
frames emitted at ~1 MB (`DBMS_DEFAULT_BUFFER_SIZE`) or block flush. `max_block_size` default
65,409; `max_compress_block_size` 1,048,576 **[vendor, checked 2026-11-09]**
(https://clickhouse.com/docs/native-protocol/basics, https://clickhouse.com/docs/native-protocol/columns,
https://clickhouse.com/docs/reference/settings/session-settings/max.md). Because data arrives
columnar, per-row cost is near zero and decode cost is per column per block (one LZ4 decompress +
checksum per ~1 MB frame, then fixed-width slices). The official Rust crate uses RowBinary over
HTTP and `Client::with_validation(false)` gives "x1.1 to x3 performance improvement" (section 2).

**HTTP + Arrow warehouses** **[vendor, checked 2026-11-09]**: Snowflake (Feb 2020) fetches results
in Arrow, "up to 5x faster" for JDBC and Python and "up to 10x" into pandas; batch size "set by
Snowflake's back end" (Sigma split oversized batches themselves)
(https://www.snowflake.com/blog/fetching-query-results-from-snowflake-just-got-a-lot-faster-with-apache-arrow/,
https://sigmacomputing.com/blog/optimizing-data-retrieval). Databricks Cloud Fetch (Aug 2021):
"Each query task writes data to cloud storage in 20 MB chunks using the Arrow streaming format",
LZ4-compressed, pre-signed URLs downloaded in parallel; results under 1 MB go inline; "more than
2x throughput" for results > 1 MB, "a 10x speed-up in extract performance due to parallelism",
12x on a 20-column / 4 M-row / 3.42 GB set (https://databricks.com/blog/2021/08/11/how-we-achieved-high-bandwidth-connectivity-with-bi-tools.html).
BigQuery Storage Read API: CreateReadSession with max streams, selected columns, predicate;
ReadRows is a streaming RPC resumable by row offset; Avro or Arrow; `arrow_serialization_options`
buffer compression (https://docs.cloud.google.com/bigquery/docs/reference/storage). For all three
the client's job is HTTP(S) fan-out, LZ4/zstd decompression and Arrow IPC parsing; a Rust driver
should expose `RecordBatch`es directly.

### Arrow Flight SQL

Flight SQL layers protobuf commands (`CommandStatementQuery`, `CommandGetTables`, ...) over
Flight RPC: `GetFlightInfo` returns one or more endpoints with opaque tickets; `DoGet(ticket)`
streams record batches; parallel endpoints enable parallel fetch; prepared statements via
`DoAction` (opaque handle), parameters via `DoPut`; transactions and savepoints via actions;
`ActionCancelQueryRequest` deprecated since 13.0.0 for `CancelFlightInfo`; sessions via cookies;
`CommandStatementIngest` is DoPut bulk load with create/append/replace modes **[vendor, checked
2026-11-09]** (https://arrow.apache.org/docs/format/FlightSql.html,
https://arrow.apache.org/blog/2022/02/16/introducing-arrow-flight-sql/). Row-based APIs "force
columnar databases to transpose data twice". For a Rust client it is the one protocol where the
server hands over Arrow IPC bytes directly; per-row cost collapses to IPC validation and
gRPC/HTTP2 framing. It fits analytical fetch, not OLTP point queries: GetFlightInfo + DoGet is
two RPCs minimum per query versus one round trip in PG/MySQL/TDS, and Teradata measured single
streams 10–15% slower than JDBC (section 3). Flight's parallel endpoints are the server-side
partitioning ConnectorX asked for (section 6.2) without the k extra scans.

### Protocol comparison table

| Protocol | Framing | Binary result format | Pipelining | Bulk path | Auth |
|---|---|---|---|---|---|
| PostgreSQL 3.0/3.2 | 1-byte type + Int32 length, big-endian; DataRow has Int16 ncols + Int32 length per column | yes, per column via Bind result-format codes; fixed-width ints/floats/timestamps, raw UTF-8 text, base-10000 numeric | yes, server-side, count ReadyForQuery per Sync; libpq 14+ pipeline mode | COPY binary (`PGCOPY` signature, per-field Int32 lengths) and text/CSV | SCRAM-SHA-256 (+PLUS with `tls-server-end-point`), MD5 (deprecated), password, GSS/SSPI, OAUTHBEARER (18) |
| MySQL / MariaDB | 3-byte LE length + 1-byte seq id, 16 MiB packets; one packet per row | binary protocol only via COM_STMT_PREPARE/EXECUTE; NULL bitmap with 2-bit offset; DECIMAL stays text | no protocol pipelining (server treats a pipeline as unrelated requests); clients batch writes | `LOAD DATA LOCAL INFILE` (server-driven, security-sensitive); MariaDB COM_STMT_BULK_EXECUTE | caching_sha2_password (default 8.0+; RSA fallback), mysql_native_password (removed from 9.0 server), sha256_password (deprecated), MariaDB ed25519 and PARSEC |
| TDS 7.4 / 8.0 | 8-byte packet header, default 4,096-byte packets (512–32,767); token stream inside | always binary; ROW/NBCROW tokens; UCS-2LE N-strings; PLP chunks for MAX types | not documented in the notes; MARS exists (7.2+) | INSERT BULK + COLMETADATA + row stream ("same as if the data were being selected") | SQL login, NTLM/Kerberos via SSPI/GSSAPI, FedAuth (Entra) tokens |
| SQLite | none (in-process C API) | n/a; typed accessors per column | n/a | prepared INSERT in a transaction; `sqlite3_bind_*` with `SQLITE_STATIC` | n/a |
| Oracle TNS/TTC | undocumented; SDU negotiated | undocumented | — | array binds; `arraysize`/`prefetchrows` | password, wallets (mTLS), tokens; Kerberos docs not found |
| DuckDB | none (C API); 2,048-row data chunks | n/a; vectors with validity masks | n/a | appender, `duckdb_append_data_chunk` | n/a |
| ClickHouse native | VarUInt packet type; chunked u32 frames; columnar Blocks of up to `max_block_size` 65,409 rows | yes, column by column, little-endian; Nullable null map; offsets layout for strings | — | Data packets of Blocks (end-of-data = empty block) | password in ClientHello |
| Snowflake / Databricks / BigQuery | HTTPS/gRPC; result chunks on cloud storage or streams | Arrow IPC batches (LZ4) | n/a | parallel chunk download / read streams | OAuth / tokens |
| Arrow Flight SQL | gRPC/HTTP2 bidirectional streams; protobuf commands | Arrow IPC record batches | parallel endpoints | `DoPut`, `CommandStatementIngest` | Flight handlers (TLS, bearer tokens) |

### Estimated bytes per 10-column row

Arithmetic from the documented layouts, **not measurements**, for one row of (int4, int8,
float8, numeric(12,2), bool, timestamp, date, uuid, varchar(20 ASCII), text(100 ASCII)),
including per-row framing (derived in the wire-protocols note):

| Protocol | Bytes per row | Per-column overhead | Notes |
|---|---:|---|---|
| ClickHouse native (columnar) | ≈177 | 0 | no per-row framing; Decimal64, DateTime64 |
| TDS ROW with VARCHAR | ≈190 | 1–2 length bytes | 1 token byte; INTN(4) 5, INTN(8) 9, FLTN 9, DECIMALN 10, BITN 2, DATETIME2N 9, DATEN 4, GUID 17, VARCHAR 22, VARCHAR(100) 102 |
| MySQL binary | ≈198 | 1-byte lenenc for strings only | 4 packet header + 1 + 2 bitmap; DECIMAL as 1+13 text; datetime 1+11 |
| PostgreSQL binary | ≈230 | 4 bytes (Int32 length) | 7 bytes framing + 40 bytes of lengths + 183 payload; numeric 8+3·2 |
| MySQL text | ≈256 | 1 byte lenenc | 4 + 10 + 242 |
| PostgreSQL text | ≈289 | 4 bytes | 47 + 242 |
| TDS ROW with NVARCHAR(100) | ≈290 | 1–2 bytes | UCS-2 doubles the 100-char string to 200 bytes |

Binary saves ~20% on this mix for PostgreSQL and ~23% for MySQL; the measured 46% saving (pgx,
uuid/timestamp-heavy rows) is the upper end. Strings dominate either way; PostgreSQL's 4-byte
per-column length is the fixed overhead a driver cannot avoid, and Raasveldt and Mühleisen
measured it "almost doubling" a mostly-int4 dataset (section 6.1).

### Where client time goes

- **Syscalls per query** (steady state, prepared statement, result fits the read buffer,
  derived): PG 1 write + 1 read (Parse+Bind+Describe+Execute+Sync fit one `write`); MySQL 1 + 1
  plus a prepare round trip on cache miss; TDS 1 write + ceil(result / read buffer) reads
  regardless of 4 KiB packets if the buffer is large; ClickHouse 1 write + one read per ~1 MB
  compressed frame; SQLite 0. Pipelining amortises the write and the ReadyForQuery wait.
- **Allocations per row** (derived): zero for fixed-width binary columns decoded in place from a
  shared buffer (PG binary, MySQL binary, TDS numerics, ClickHouse); one allocation per string
  column only when ownership must outlive the buffer; unavoidable transcoding for TDS N-strings
  and non-UTF-8 VARCHAR collations; arithmetic allocations for PG `numeric` if decoded to
  arbitrary precision (avoidable with a 128-bit fast path for <= 38 digits); arrays and
  composites need a lazy view to avoid per-element allocation.
- **Published profiles** all put the time in per-value parsing and per-row object allocation,
  not in syscalls, once the driver reads into a large buffer: asyncpg (text parsing, Python
  tuple create/free), pgJDBC 2006 (`Encoding.decode`, `new String`), go-sql-driver (`[]byte` ->
  `interface{}` boxing, ~32 bytes per string column), Boost.MySQL (correct binary clients
  converge). No primary-source flamegraph of a modern Rust/Go/C driver hot path was found; the
  evidence is 2006–2016 profiles plus 2024–2026 benchmarks without profiles.
- **Nagle and delayed ACK**: "a sender that uses Nagle and a receiver that uses Delayed ACK may
  wait for an ACK that is delayed, adding ~40 ms to each request"; the fix is TCP_NODELAY on the
  sender; the stall arises mainly when one application message is split across several writes
  **[third party, checked 2026-11-09]** (https://oneuptime.com/blog/post/2026-03-20-debug-tcp-nagle-algorithm-delays/view,
  https://lkml.indiana.edu/hypermail/linux/kernel/0507.2/0969.html). pgJDBC sets
  `tcpNoDelay=true` and buffers up to `maxSendBufferSize=8192` bytes before sending. Section
  6.6 has the kernel constants.
- **TLS record overhead**: TLS 1.3 plaintext per record <= 2^14 (+1 for the content-type byte),
  AEAD expansion <= 256 octets, "typically only 16 octets" in earlier versions (RFC 8449,
  https://www.rfc-editor.org/rfc/rfc8449.html); the 22-byte fixed overhead per record (5-byte
  header + 1-byte inner type + 16-byte tag) is derived from RFC 8449 plus the known header and
  tag sizes, not quoted from RFC 8446 (section 6.9).
- **Default fetch sizes found**: pgJDBC `defaultRowFetchSize=0` (all rows), `prepareThreshold=5`;
  python-oracledb `arraysize=100`, `prefetchrows=2`; Oracle JDBC prefetch 10; ClickHouse
  `max_block_size=65409`; Databricks 20 MB Arrow chunks with a 1 MB inline threshold; Snowflake
  backend-set; libpq pipelining flushes on `PQpipelineSync`/`PQflush` only. MySQL Connector/J,
  mssql-jdbc, asyncpg cursor prefetch and psycopg3 `prepare_threshold` defaults were not
  confirmed from fetched pages (section 13).

The design consequence: the protocol families split into (1) row-framed with per-column lengths
(PG, MySQL, TDS), where the win is binary formats + zero-copy row views + pipelining/batching, and
(2) columnar/Arrow (ClickHouse, Flight SQL, warehouse chunks), where the win is handing out
column slices or Arrow batches and never materialising rows.

## 6. Papers and mathematics

Primary texts the research opened in full: Raasveldt & Mühleisen VLDB 2017; Wang et al.
ConnectorX VLDB 2022; Boncz/Zukowski/Nes CIDR 2005; Mühlbauer et al. VLDB 2013;
Schroeder/Wierman/Harchol-Balter NSDI 2006; Langdale & Lemire simdjson; Lemire fast_float;
Keiser & Lemire UTF-8; Kalibera & Jones ISMM 2013; Mitzenmacher/Richa/Sitaraman survey
(partially; OCR poor). Formulas are plain text; variables are defined where introduced;
**(restated)** and **(derived)** are kept as in the source note.

### 6.1 The transfer cost model and compression break-even (Raasveldt & Mühleisen)

"Don't Hold My Data Hostage" (PVLDB 10(10), 2017, DOI 10.14778/3115404.3115408) **[paper,
checked 2026-11-09]** (http://www.vldb.org/pvldb/vol10/p1022-muehleisen.pdf) measured result-set
(de)serialisation and transfer of TPC-H SF10 `lineitem` (60 M rows, 7.2 GB CSV) on one machine,
ODBC fetch, warm:

| System | Time (s) | Bytes on the wire |
|---|---:|---:|
| netcat CSV | 10.25 | 7.19 GB |
| MySQL | 101.22 | 7.44 GB |
| DB2 | 169.48 | 7.33 GB |
| "DBMS X" | 189.50 | 6.35 GB |
| PostgreSQL | 201.89 | 10.39 GB |
| MonetDB | 209.02 | 8.97 GB |
| MySQL + GZIP | 391.27 | 2.85 GB |
| Hive (Thrift) | 627.75 | 8.69 GB |
| MongoDB | 686.45 | 43.6 GB |

"Transferring a CSV file over a socket is an order of magnitude faster than exporting the same
amount of data from any of the measured systems"; connection and query-execution time were
"insignificant" compared with serialisation plus transfer; on loopback "most time is spent
(de)serializing the result set." Why: PostgreSQL's per-row DataRow with a 4-byte length per field
means that for an (INT32, VARCHAR(10)) row "the amount of per-row metadata is greater than the
actual data", and on the mostly-int4 ACS dataset the fixed lengths "almost double the size"
(12.5 GB for a 5.38 GB CSV); MySQL's text integers are compact for small values (ACS 5.78 GB vs
their binary 8.24–9.49 GB); MonetDB converts binary to ASCII and back; Hive is "unnecessarily
verbose"; MongoDB repeats field names (~6x CSV). Contrary to their prediction all protocols
degrade with latency because TCP ACKs are per filled buffer, so "protocols that send more data
trigger more acknowledgements"; DB2 and DBMS X degrade much more because they send explicit
per-batch confirmations. With reduced bandwidth "compression becomes more effective".

Design-space results: chunk size on lineitem 2 KB 55.9 s, 10 KB 15.2 s, 100 KB 10.9 s, **1 MB
10.0 s** (5.80 GB), 10 MB 10.9 s, 100 MB 13.3 s — "for all three datasets, the performance is
optimal when the chunk size is around 1MB"; column-major compresses better than row-major on
identical binary data (LZ4 2.10x vs 1.85x; Snappy 2.20x vs 1.92x; GZIP 3.24x vs 2.76x; XZ 4.44x vs
3.74x); compression vs link speed (seconds, Local / 1000 Mbit / 100 Mbit / 10 Mbit, size): None
1.5 / 10.4 / 84.8 / 848 (1012 MB); Snappy 3.3 / 3.8 / 37.3 / 373 (447 MB); LZ4 4.5 / 4.9 / 38.4 /
383 (456 MB); GZIP 59.8 / 60.4 / 59.6 / 226 (272 MB); XZ 695 / 689 / 666 / 649 (203 MB) — no
compression on localhost, lightweight from 1 Gbit/s down, heavy only at 10 Mbit/s, XZ never;
column-specific integer codecs (binpacking, PFOR) beat Snappy on lineitem but lose badly on ACS
(265 int columns -> few rows per chunk) so were not made default; Protobuf is 3.2x slower
locally than their custom serialisation (33.1 vs 10.3 s) because of generic varint/endianness
work; null-terminated and varint-prefixed strings are equal in size and time, fixed width wins
only for VARCHAR(1); a per-column NULL bitmask omitted for NOT NULL columns transfers far less
when >50% NULL (ontime 2.24 vs 3.49 GB) but compresses worse; the client negotiates a maximum
chunk size at authentication so it "only needs to allocate a single buffer to hold the result set
messages" and can "directly access the data stored without needing to unnecessarily convert
and/or copy the data." Their protocol's results (Local / LAN 1 Gbit 0.3 ms / WAN 100 Mbit 25 ms
1% loss): MonetDB++ 10.6 / 50.3 / 510.8 s (5.80 GB); MonetDB++ with Snappy 15.5 / 19.9 / 200.6 s
(2.27 GB); PostgreSQL++ 39.3 / 46.1 / 518.8 s; PostgreSQL++ with Snappy 42.4 / 43.8 / 229.5 s
(2.53 GB); versus PostgreSQL 205.8 / 301.1 / 2108.8 s and DB2 166.9 / 598.4 / timeout.

**Cost model (derived from the paper's stage decomposition; the paper gives no closed form).**
Let S = bytes on the wire, S_raw = raw data size, B = link bandwidth (bytes/s), RTT = round-trip
time, n_rt = number of request/acknowledge round trips the protocol forces (0 for a pure
server-push stream, 1 per batch for DB2-style confirmations), C_ser and C_deser = per-core
(de)serialisation throughput (bytes/s).

- Sequential (store-and-forward): `T = T_ser + T_net + T_deser`, with `T_net = S/B + n_rt * RTT`,
  `T_ser = S_raw / C_ser`, `T_deser = S_raw / C_deser`.
- Streamed/overlapped (what the measurements fit): `T ≈ max(T_ser, T_net, T_deser) + one chunk's
  worth of the other two stages`. Evidence: Snappy at 1 Gbit took 3.8 s, close to max(CPU 3.3 s,
  wire 447 MB / 125 MB/s = 3.6 s) and far from the sum (6.9 s); MySQL + GZIP is flat across link
  speeds because its GZIP stage dominates the max.
- Compression break-even (derived). Let r = compression ratio, C_c and C_d = compress and
  decompress throughput (bytes/s).
  - Sequential: compress iff `1/C_c + 1/C_d < (1 - 1/r) / B`, i.e. `B < (1 - 1/r) / (1/C_c + 1/C_d)`.
  - Overlapped: compress iff `max(1/C_c, 1/(r*B), 1/C_d) < 1/B`, i.e. `B < min(C_c, C_d)` (any
    r > 1 helps as long as neither compressor side is slower than the uncompressed link). On
    loopback B is effectively memory bandwidth, so compression never pays (None 1.5 s vs Snappy
    3.3 s).
  - From the paper's own numbers: Snappy on lineitem cost about 1.8 s per 1012 MB (≈560 MB/s
    compress + decompress), so Snappy pays below roughly 4.5 Gbit/s (overlapped) or 2.4 Gbit/s
    (sequential) on that 2017 VM. Section 6.9 recomputes with 2024-era codecs.
- Chunk size: the measured optimum (~1 MB, flat from 100 KB to 10 MB) is where per-chunk fixed
  costs are amortised while the chunk still fits L2/L3 for transposition. It matches X100's
  cache-fit argument and Instant Loading's `0.25–1.0 * L3 / hardware_threads` rule (6.4). For a
  Rust driver: a default receive/decode chunk of 256 KB–1 MB, negotiated once per connection,
  allocated once and reused.

Design consequences the note draws: expose a chunk/columnar fetch path; never send or accept
per-row acknowledgements; keep values fixed-width binary; treat compression as a per-connection
policy keyed on measured link speed; and expect decoding CPU, not the wire, to be the bottleneck
on LAN for text protocols.

### 6.2 ConnectorX (Wang et al., VLDB 2022)

**[paper, checked 2026-11-09]** (https://www.vldb.org/pvldb/vol15/p2994-wang.pdf; two AWS
r5.4xlarge, 16 vCPU, 128 GB, 10 Gbit/s; TPC-H SF10 lineitem). Headline: pandas 12.46 min and
95.6 GB peak vs ConnectorX 0.97 min and 31.4 GB; "the actual time spent on query execution is
less than 1 min (13x time overhead) and the final Pandas.DataFrame is only 24.4 GB (4x memory
overhead)." ">85% of time is spent in the client"; deserialisation and conversion to dataframe
"each accounting for approximately 40% of the running time". Memory: PostgreSQL raw bytes 12.4 GB
-> Python objects 52.6 GB -> dataframe 24.4 GB, peak 95.6 GB, because a Python int is 8 bytes of
payload plus a 20-byte object header and "Pandas.read_sql keeps three copies of the entire data
in memory". Chunking reduces peak memory to about the final size but "has little help in
improving the running time".

Architecture: preparation (query row count and column types; allocate NumPy arrays; partition
the query on a user-given or `SELECT MIN(col), MAX(col)`-derived range) then one worker per
partition fetches small batches and writes decoded values directly into final slots. String
creation is batched under one GIL acquisition (disabling this costs 6% unpartitioned but makes
the run 4.6x slower with 4 partitions). Disabling streaming costs ~20% time and 2.3x memory
(70.4 GB, still 20 GB below pandas). A Rust macro DSL cut type-mapping code "by 97% (from 37k to
1k lines of code)". "No Partition cannot saturate the network bandwidth at all"; four partitions
saturate 10 Gbit. Server cost of client-side partitioning (PostgreSQL): no partition 1 scan,
1.1 M disk-block misses, 156.1 s; 2/4/8/16 partitions = 3/5/9/17 scans, 3.2 M / 3.8 M / 3.8 M /
17.1 M misses, 86.4 / 49.1 / 30.4 / 26.7 s; a server-side result-partitioning prototype (one scan)
86.7 / 45.7 / 23.9 / 19.6 s. Speedups vs pandas (4 partitions): Local 14.26x, 10 Gbit 12.80x,
200 Mbit 3.05x; vs Turbodbc 6.64x / 6.21x / 1.90x; without partitioning ConnectorX is 3.16x /
3.03x / 1.08x slower than with — "the gap ... becomes larger when the bandwidth is higher".
Without partitioning it beats pandas 4.2x (PostgreSQL), 5.2x (MySQL), 3.0x (DBMS-A), 2.3x
(SQLite); "how efficient the DBMS's ODBC driver is implemented ... highly determines Turbodbc's
performance." SPJ queries: up to 8.4x faster with partitioning but as slow as 0.3x (Q19) when the
result is small because the range query and extra plans dominate; they recommend Arrow as the
intermediate destination for complex queries to avoid "the potentially costly COUNT query".
Linear from SF1 to SF100.

Inferences: count-then-preallocate trades one round trip and a COUNT scan for zero
reallocation; it is wrong for small or complex results (their own Q19 regression), so it should
be optional with chunked Arrow-style growth as the fallback. Parallel partitioned reads are a
bandwidth technique whose benefit scales with link speed and costs the server k+1 scans and
3.5–15.5x the disk misses; expose it as an explicit API and prefer server-side parallel endpoints
where protocols offer them. The 20-byte-per-int, three-copies finding is the general argument
against per-value boxed objects in any host language (the JVM analogue is in 6.7). Amdahl-style
(derived, not from the paper): with k partitions, time ≈ max(S/B, S_raw/(k*C_decode)) +
per-partition overhead.

### 6.3 Arrow and columnar interchange

- **Arrow C Data Interface**: "Allow zero-copy sharing of Arrow data between independent runtimes
  and components running in the same process"; "Expose an ABI-stable interface"; "the C ABI is
  frozen"; `ArrowArray` = {length, null_count, offset, n_buffers, buffers, n_children, children,
  dictionary, release, private_data}; "Consumers MUST call a base structure's release callback";
  moving is a bitwise copy; not for cross-process sharing **[vendor, checked 2026-11-09]**
  (https://arrow.apache.org/docs/format/CDataInterface.html). This gives a Rust driver a free,
  dependency-light zero-copy export path: produce chunks as Arrow-layout buffers (validity +
  data + offsets) and hand out `ArrowArray` structs — the same shape Raasveldt's protocol
  converged on, so one internal chunk layout can serve a row API and a columnar export.
- **Arrow Flight** (2019): gRPC/HTTP2 streaming; batches produced "without going through any
  intermediate memory copying or serialization steps" and rebuilt "without any memory copying or
  deserialization"; `arrow-flight-benchmark` read 12,800,000,000 bytes in 3,900,466,413 ns =
  **3,129.63 MB/s on localhost without TLS**; Dremio's "20-50x better performance over ODBC"
  relayed **[vendor]** (https://arrow.apache.org/blog/2019/10/13/introducing-arrow-flight/).
- **Independent Flight benchmark** (Ahmad, Al Ars, Hofstee, arXiv 2204.03032, preprint, not
  peer-reviewed): "up to 6000 MB/s and 4800 MB/s throughput for DoGet() and DoPut()"; up to 95%
  of Mellanox ConnectX-3/Connect-IB bandwidth; "up to half of the available system cores"; on
  Dremio "about 20x better than turbodbc and about 30x better than ODBC"; "more than 80% of the
  total time spent in accessing data is elapsed in serialization/de-serialization step". The
  jdbc-baseline note's reading of the body: ~1 GB/s per stream to ~10 GB/s with 16 streams on
  loopback and 1.5–2 GB/s DoGet over 56 Gbit/s InfiniBand, and the abstract's 6,000/4,800 MB/s
  figures "are not matched by a described setup"; JDBC was not tested **[third party, checked
  2026-11-09]** (https://arxiv.org/abs/2204.03032, https://ar5iv.labs.arxiv.org/html/2204.03032).
- **ADBC rationale** (2023): JDBC is row-oriented; ODBC can carry columns but its type system does
  not map to Arrow; Turbodbc and arrow-jdbc still do row-to-column conversion; PostgreSQL has no
  Arrow output; Arrow-native sources pass data through unchanged, others convert once; no
  performance numbers given **[vendor]** (https://arrow.apache.org/blog/2023/01/05/introducing-arrow-adbc/).
- Teradata's near-parity single-stream measurement (section 3) is the counter-weight: Flight's
  "no deserialization" is literally true only when the client is Arrow-native; for text/binary
  row protocols the driver still decodes, so the win is one decode into a columnar chunk instead
  of decode-to-rows-then-transpose. No peer-reviewed, independent Flight-vs-JDBC measurement was
  found. The DataFusion SIGMOD 2024 paper (DOI 10.1145/3626246.3653368) was not opened.

### 6.4 X100 and SIMD per-byte costs

**MonetDB/X100** (Boncz, Zukowski, Nes, CIDR 2005) **[paper, checked 2026-11-09]**
(https://www.cidrdb.org/cidr2005/papers/P19.pdf): a MySQL 4.1 gprof trace on TPC-H Q1 showed the
five operations that do the "work" taking <10% of time, 28% hash-table work and 62% record
navigation/copying; `Item_func_plus::val` costs 38 instructions at IPC 0.8 = "49 cycles!" per
addition vs ~3 for the RISC sequence (~20 cycles of dependent-instruction stalls, ~20 of function
call). Commercial DBMSs get IPC ~0.7. Full column materialisation (MonetDB/MIL) is
memory-bandwidth bound at 500 MB/s with 75 cycles per multiplication, "even worse than MySQL".
X100 primitives run at 1.9–6.6 cycles per tuple and sustain >7.5 GB/s on cache-resident vectors;
compound primitives are often 2x faster because 2-ary vectorised ops are load/store bound.
**Vector size**: "the optimal vector size seems to be 1000, but all values between 128 and 8K
actually work well. Performance starts to deteriorate when intermediate results do not fit in
the cache anymore"; default 1024. Overall 50–100x faster than MySQL on Q1 and within 2x of
hand-coded C. The 49-vs-2.2 cycles is the quantitative case for a chunk-oriented driver API: a
per-value virtual `get_i32(row, col)` reintroduces the function-call-per-value and dependent
stalls MySQL paid; `column_i32(col) -> &[i32]` lets the compiler and SIMD do the work. Exposing
both, with the row API a thin view over chunks, keeps compatibility without paying on the bulk
path.

**SIMD parsing** **[paper, checked 2026-11-09]**:

| Operation | Throughput | Machine | Source |
|---|---|---|---|
| JSON parse + validate (simdjson) | 2.2–3.2 GB/s (twitter, gsoc); 0.95–1.1 GB/s number-heavy; RapidJSON 0.41–0.51 GB/s | Intel i7-6700 Skylake 3.4 GHz, GCC 9.1 | Langdale & Lemire, VLDB J 2019, DOI 10.1007/s00778-019-00578-5 (https://arxiv.org/abs/1902.08318); README 2026: 5.11 GB/s to native structs on Xeon Gold 6548N, UTF-8 validation "about 13 GB/s" [vendor] (https://github.com/simdjson/simdjson) |
| Text -> double (fast_float, Eisel–Lemire) | 45 M floats/s (canada), 61 M (32-bit ints as floats) vs glibc strtod 9.0, abseil 18 | Skylake 3.7 GHz; Zen 2 51 M; Ampere 22 M; POWER9 42 M | Lemire, SPE 51(8) 2021, DOI 10.1002/spe.2984 (https://arxiv.org/abs/2101.11408); README 1042 MB/s vs strtod 191 [vendor] |
| UTF-8 validation (lookup) | 24–28 GiB/s (twitter), 17–18 GiB/s (hongkong.html); "slightly under 0.4 instructions per byte"; >memcpy on pure ASCII (59–66 GiB/s); 30x branchy, 6x finite-state on random non-ASCII | AMD Rome 3.4 GHz; Skylake 3.7 GHz | Keiser & Lemire, SPE 51(5) 2021, DOI 10.1002/spe.2920 (https://arxiv.org/abs/2010.03090) |
| CSV parse + deserialise (Instant Loading) | >1.6 GB/s with 8 threads (4 cores) SSE 4.2 `pcmpistri` vs ~1.0 GB/s non-SSE; baseline single-threaded ~100 MB/s in HyPer/MonetDB/Vectorwise; perf: ~50% parsing, 20% deserialisation; "almost one misprediction per field and record delimiter"; end to end TPC-H SF10 CSV in 6.9 s on a 4-core i7-3770 | i7-3770; Xeon X7560 | Mühlbauer et al., PVLDB 6(14) 2013 (http://www.vldb.org/pvldb/vol6/p1702-muehlbauer.pdf) |
| Base64 decode (AVX2) | ~0.2 cycles/byte vs ~2 for Chrome's table decoder; memcpy <0.05 | — | Muła & Lemire, ACM TWEB (arXiv 1704.00605); author blog (https://lemire.me/blog/2018/01/17/ridiculously-fast-base64-encoding-and-decoding/) |
| Double -> text (Ryu) | C double 27.5 vs Grisu3 99.0; Java 89.8 vs JDK 1085.6; units not stated on the page | — | Adams, PLDI 2018 (https://github.com/ulfjack/ryu) [vendor]; Dragonbox "defeats or is on par with" Ryu, charts only (https://github.com/jk-jeon/dragonbox) |

Instant Loading also gives the chunk rule `0.25 to 1.0 * L3_size / hardware_threads` (256 KB–
1 MB on an 8 MB L3, 8 threads), notes SSE integer deserialisation is slower than scalar below 4
characters (so use a hybrid), finds chunk-based merge 12% faster than copy, +5% from
cardinality-estimate allocation hints, and recommends huge pages for buffers.

**Per-value cost ladder on one modern core (derived; components measured on different
machines)**: binary fixed-width copy ≈ 0.02–0.05 cycles/byte; UTF-8 validation 0.3–0.4
instructions/byte (~0.1–0.15 ns/byte); base64 0.2 cycles/byte; JSON structural parse ~1–1.5
ns/byte; text -> double ~20–22 ns per value; double -> text tens of ns. A text-protocol row with
16 numeric columns therefore costs roughly 300–400 ns of parsing before any allocation (~2.5–3 M
rows/s/core); the same row in a binary protocol is a few dozen bytes of memcpy plus endianness
swaps. Chunk-size defaults are supported by three independent sources: ~1 MB (Raasveldt), 128–8K
values (X100), 0.25–1.0 x L3/threads (Instant Loading); for t decode threads on a 32 MB L3 part,
1–4 MB per in-flight chunk per thread is the envelope. Use SIMD for delimiter/structural search
and validation, branch-reduced scalar code for short values. No direct measurement of a
PostgreSQL text-vs-binary wire decoder exists in the literature.

### 6.5 Queueing and pool sizing

- **Little's law (restated)**: `L = λ * W` — mean number in system equals arrival rate times mean
  time in system (Little, Operations Research 9(3), 1961, DOI 10.1287/opre.9.3.383; primary not
  opened). For a pool: `in_use = throughput_per_s * mean_hold_seconds`.
- **M/M/c and Erlang C** **[third party, checked 2026-11-09]** (https://en.wikipedia.org/wiki/M/M/c_queue):
  arrival rate λ, per-server rate μ, utilisation `ρ = λ/(c μ) < 1`, offered load `a = λ/μ`.
  Probability an arrival must wait:
  `C(c, a) = [ (cρ)^c / c! * 1/(1-ρ) ] / [ Σ_{k=0}^{c-1} (cρ)^k / k! + (cρ)^c / c! * 1/(1-ρ) ]`;
  mean response `W = C / (c μ - λ) + 1/μ`; mean queueing wait `Wq = C / (c μ - λ)`; mean number
  in system `L = ρ/(1-ρ) * C + c ρ`.
- **HikariCP's supply-side cap** **[vendor, checked 2026-11-09]**
  (https://github.com/brettwooldridge/HikariCP/wiki/About-Pool-Sizing): `connections =
  ((core_count * 2) + effective_spindle_count)` (cores exclude hyperthreads; spindles 0 if the
  working set is cached; a 4-core 1-disk server => 9, "round to 10"; SSD applicability "hasn't
  been analyzed"); the Oracle demo's 2048 -> 96 connections cut response time ~100 ms -> ~2 ms;
  "More threads only perform better when blocking creates opportunities for executing";
  deadlock-avoidance minimum `pool_size = Tn * (Cm - 1) + 1` (Tn = max threads, Cm = max
  connections one thread holds at once); mixed long/short workloads: two pools.
- **PostgreSQL connection cost** **[third party, PG committer; checked 2026-11-09]**: "it starts
  ('forks') a new process for each connection" (https://www.postgresql.org/docs/current/tutorial-arch.html);
  corrected per-connection memory `Pss_Anon + VmPTE` ≈ 7,665 kB (~7.6 MiB) without huge pages,
  ≈ 1,305 kB (~1.3 MiB) with (page tables 6,480 kB -> 132 kB)
  (https://blog.anarazel.de/2020/10/07/measuring-the-memory-overhead-of-a-postgres-connection/);
  idle connections slow active ones via `GetSnapshotData()`: 1 active connection 33,457 TPS with
  0 idle, 30,558 with 1,000, 23,284 with 5,000, **14,496 with 10,000 idle**; 48 active 1,032,435
  -> 521,558 TPS at 10,000 idle; mostly-idle clients worse (1 active drops to 6,049 TPS, 18% of
  baseline, with 10,000 of them); with simulated 1 ms network + 1 ms app delay pgbench peaked
  around 3,000 connections vs ~48 without; poolers help only in transaction/statement modes,
  which exclude prepared statements and temp tables (PG12,
  https://techcommunity.microsoft.com/t5/azure-database-for-postgresql/analyzing-the-limits-of-connection-scalability-in-postgres/ba-p/1757266).
- **Power of two choices** **[paper, partially read]**
  (https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf): one random choice gives
  max load `log n / log log n (1+o(1))`; the least loaded of d >= 2 random bins gives
  `log log n / log d + Θ(1)` — "even a small amount of choice can lead to drastically different
  results"; supermarket model: with one choice expected time in system is the M/M/1 value
  `1/(1-λ)`; with d >= 2 it grows only like `log(1/(1-λ))` as λ -> 1 (the exact constant was not
  legible; standard form (restated): `s_i = λ^((d^i - 1)/(d - 1))` for the fraction of queues
  with >= i customers).
- **LRU hit ratio, Che approximation (restated)** (Fricker, Robert, Roberts, arXiv 1202.3974; Che,
  Tung, Wang, IEEE JSAC 2002, not opened): `h_i = 1 - exp(-q_i * T_C)` where q_i is object i's
  request rate and T_C solves `Σ_i (1 - exp(-q_i * T_C)) = C` (C = cache size); overall
  `H = Σ_i p_i * h_i`; "remarkably accurate" for general popularity distributions.

Inferences (derived): pool size `N_needed = λ * W_hold / target_utilization`, then Erlang C gives
P(wait) and Wq; the HikariCP rule is the supply-side cap and Little's law the demand side; the
smaller is the pool and the application must queue above it. **Partitioned vs shared pools
(restated standard result)**: c independent M/M/1 queues each fed λ/c have mean wait
`ρ/(μ(1-ρ))` per queue, while one M/M/c fed λ has `C(c,a)/(cμ-λ)`, strictly smaller for c > 1 at
equal ρ (resource pooling); partition only when checkout contention is measured, and add work
stealing or two-choice checkout to recover most of the pooled behaviour. Thundering herd on
return (waking all waiters) is O(waiters) wakeups for one winner; direct FIFO/LIFO handoff is the
standard fix (no paper measured this for DB pools). Two-choice routing across replicas gets
within a constant of least-loaded with O(1) information. Prepared-statement caches: per-connection
LRU of size C against D distinct statements with Zipf popularity; pick C so that H >= target via
Che; since memory on both sides scales as `pool_size × C`, a pool of 50 with 1,000 cached
statements each is 50,000 server-side prepared statements, so size C from the hit-rate model,
not "unlimited". Each new PostgreSQL connection is a fork plus ~1.3–7.6 MiB plus TLS/auth round
trips, and even idle connections tax `GetSnapshotData`; this favours protocol-level pipelining
over connection count for throughput.

### 6.6 Batching and pipelining bounds, and the Nagle stall

- **Round-trip bound (restated)**: with N requests outstanding, `X ≤ N / RTT`; strictly
  `X = N / (RTT + S_srv + S_cli)`; sequential request/response is N = 1.
- **Pipelining gain (derived)**: k statements per round trip take `T_pipe ≈ RTT + k * s` versus
  `T_seq = k * (RTT + s)` (s = per-statement server time), so `speedup = k (RTT + s) / (RTT + k s)`
  -> k when `RTT >> k s`, -> 1 when `s >> RTT`. Check: k = 100, RTT = 0.3 s, s ≈ 0 gives 30 s vs
  0.3 s, matching PostgreSQL's own example. libpq's warning: "The client must not assume that
  work is committed when it sends a COMMIT — only when the corresponding result is received."
- **Nagle + delayed ACK**: Linux `include/net/tcp.h`: `TCP_DELACK_MAX = HZ/5` (200 ms at
  HZ=1000), `TCP_DELACK_MIN = HZ/25` (40 ms), `TCP_ATO_MIN = HZ/25` **[primary source, checked
  2026-11-09]** (https://raw.githubusercontent.com/torvalds/linux/master/include/net/tcp.h). John
  Nagle: Nagle's algorithm holds small writes until outstanding data is ACKed; delayed ACK holds
  the ACK hoping to piggyback; "The combination of the two is awful"; recommends `TCP_QUICKACK`
  "unless you have a very unusual application"; `TCP_NODELAY` works but can hurt throughput for
  many small writes because each becomes a packet, so the application must manage its own
  flushing **[primary source, author comment]** (https://news.ycombinator.com/item?id=10608356).
  Rule (derived): a driver that writes a request as two `send()` calls with the server replying
  only after the last one pays `TCP_DELACK_MIN` = 40 ms per request on Linux; set `TCP_NODELAY`
  on every database socket and coalesce each logical request into one write (vectored I/O). No
  peer-reviewed measurement of the 40 ms stall in database drivers specifically exists.
- **Bandwidth-delay product**: RFC 7323 §1.1 — problems arise "when the bandwidth * delay product
  is large"; the 16-bit window caps the unscaled window at 64 KiB; scaling allows 1 GiB
  (https://www.rfc-editor.org/rfc/rfc7323). `BDP_bytes = B_bytes_per_s * RTT`: 1 Gbit/s x 0.3 ms
  ≈ 37.5 KB; 10 Gbit/s x 1 ms ≈ 1.25 MB; 100 Mbit/s x 25 ms ≈ 312 KB; unscaled TCP caps a
  single stream at `64 KiB / RTT` (2.6 MB/s at 25 ms).
- **Optimal fetch/batch size for request-per-batch protocols (derived; this note's)**: with F
  rows of b bytes per fetch, `t = RTT + F b / B + c0` per batch (c0 = fixed per-batch CPU),
  rows/s `= F / t`, asymptote `B / b`; link efficiency `η = 1 / (1 + BDP / (F b))`; to reach η:
  `F b ≥ (η / (1 - η)) * BDP`; for η = 0.9, batch bytes ≥ 9 x BDP (LAN ≥ 340 KB; WAN 100 Mbit /
  25 ms ≥ 2.8 MB). With server-push streaming F only affects memory and decode granularity and
  the ~1 MB chunk of 6.1 applies; if the protocol forces fetch requests, pipeline the next fetch
  before decoding the current one (N = 2 keeps the link busy: `X ≈ 2 / RTT`).
- **When to open parallel connections for a bulk read (derived from ConnectorX)**: one stream is
  limited by `min(B, C_decode_per_thread, server_single_stream_rate)`; with per-thread decode
  C_d ≈ 100–300 MB/s for text protocols and 10 Gbit/s = 1,250 MB/s, `k ≈ ceil(B / C_d)` ≈ 4–12
  connections saturate the link; beyond that each connection costs the server an extra scan and
  3.5x–15.5x the disk misses; below ~1 Gbit/s one connection is already network-bound
  (ConnectorX's 1.08x at 200 Mbit).

### 6.7 Memory and allocation

- Per-value heap objects dominate in dataframe and JVM clients: Python int 20-byte header on 8
  bytes payload and 4x peak memory (ConnectorX, 6.2); JVM `Integer` 16 bytes plus a 4-byte
  reference for a 4-byte value (section 3, secondary sources).
- Raasveldt: negotiate a maximum chunk size so the client "only needs to allocate a single
  buffer"; 1 MB chunks mean "the client does not need a large amount of memory to get good
  performance." Instant Loading: heap (re)allocation of partition buffers "is costly as, in
  general, it needs to be synchronized"; allocation hints +5%; huge pages; chunk-pointer append
  12% faster than memcpy merge.
- Allocator choice (mimalloc README benchmarks, AMD 5950X, 2021-01-30, tcmalloc 2.8.1, jemalloc
  5.2.1): leanN ~13% faster than tcmalloc; sh6bench "more than 2.5x faster than jemalloc";
  sh8bench makes tcmalloc ~10x slower once objects migrate between threads; cross-thread free
  is "a single CAS"; a 2026-09 note says other allocators have since improved **[vendor, checked
  2026-11-09]** (https://github.com/microsoft/mimalloc). The design wins are structural, not
  allocator choice.
- Cost model for a row API that allocates per cell (derived): `T_row ≈ n_cols * (t_alloc +
  t_decode) + t_row_alloc`; with t_alloc ≈ 20–50 ns (**a placeholder, not a cited number**) and
  16 columns, allocation alone is ≈ 0.3–0.8 µs/row, the same order as the whole text-decoding
  budget in 6.4 and more than the entire binary decode. An arena per chunk (one `Vec<u8>` per
  chunk, values as offsets) or borrowed slices into the receive buffer (`&[u8]`/`&str` with the
  chunk's lifetime; `Bytes`-style refcounted slices) removes this term entirely.
- Receive-buffer discipline: choose a chunk size once (0.25–1 MB), keep a small ring of reusable
  buffers per connection, decode in place, copy only when the user asks for an owned value.
  Column-major chunks make each column's decode loop stream through contiguous memory (X100's
  premise), while per-row object graphs scatter.
- No published measurement of JDBC per-row object allocation cost exists beyond object-size
  facts and the Columnar heap number (section 3).

### 6.8 Coordinated omission, open vs closed loops, warm-up and statistics

- **Coordinated omission (Gil Tene)**: a system at 100 req/s, 1 ms each, frozen for 100 s after
  every 100 s of normal operation has a true 200-second average of 25 s, p75 of 50 s, p99.99 of
  100 s; a naive closed-loop generator records ~10,000 x 1 ms plus one 100 s sample and reports
  average 10.9 ms, p75 and p99.99 of 1 ms; a change making every post-stall request 5 ms makes
  the reported p99.99 5 ms, so the "improvement" looks 5x worse. User exposure: 5 page loads x
  ~40 resources = 200 resources per session; `0.95^200 ≈ 0.003%` of users avoid the p95; 18% see
  worse than p99.9; >99% see the p99.995. "The number one indicator you should never get rid of is
  the maximum value." **[third party summary; Tene's slides returned 403]**
  (https://bravenewgeek.com/everything-you-know-about-latency-is-wrong/,
  https://groups.google.com/g/mechanical-sympathy/c/icNZJejUHfE). wrk2's README: at 2,000 req/s
  corrected p99 10.52 ms vs uncorrected 5.43 ms in a quiet run; with one 1.4 s server pause,
  corrected p99 1.27 s vs uncorrected 6.04 ms ("about 200x") **[vendor]**
  (https://github.com/giltene/wrk2). Tene: when benchmarking "you don't have to correct for
  Coordinated Omission, because you can cleanly avoid Coordinated Omission to begin with"
  (open loop); HdrHistogram's `recordValueWithExpectedInterval()` back-fills only when a large
  latency is observed; HdrHistogram footprint "around 185KB" for 1 µs–1 hour at 3 significant
  digits, recording "3-6 nanoseconds" (c. 2012 CPUs) (https://github.com/HdrHistogram/HdrHistogram).
  pgbench `-R rate` schedules on a Poisson process and reports latency from the scheduled start
  plus "schedule lag"; `-L limit` counts late transactions
  (https://www.postgresql.org/docs/current/pgbench.html).
- **Open vs closed** (Schroeder, Wierman, Harchol-Balter, NSDI 2006) **[paper, checked
  2026-11-09]** (https://www.usenix.org/legacy/event/nsdi06/tech/full_papers/schroeder/schroeder.pdf):
  closed = fixed multiprogramming level N where "a new request is only triggered by the
  completion of a previous request"; open = arrivals at rate λ independent of completions;
  partly-open = sessions arrive openly and issue several requests closed-loop. "For a fixed load,
  the mean response time for an open system model can exceed that for a closed system model by
  an order of magnitude or more"; "there is still a significant difference ... even for an MPL
  of 1000"; eight principles: (i) mean response times are significantly lower in closed systems;
  (ii) as MPL grows closed systems become open, but convergence is slow; (iii) "While variability
  has a large effect in open systems, the effect is much smaller in closed systems"; (iv) open
  systems benefit significantly from size-based scheduling, closed far less; (v) scheduling
  significantly improves closed systems only at moderate load and high MPL; (vi) scheduling can
  limit the effect of variability in both; (vii) a partly-open system behaves like open when the
  expected requests per session is small (<= 5) and like closed when large (>= 10); (viii) think
  time has little effect in partly-open systems. Most web benchmarks surveyed assume a closed
  model and often do not say so.
- **JIT warm-up** (Barrett et al., OOPSLA 2017, DOI 10.1145/3133876): "at most 43.5% of <VM,
  benchmark> pairs consistently reach a steady state of peak performance" across three machines;
  small deterministic microbenchmarks often fail to reach a steady state **[paper abstract]**
  (https://arxiv.org/abs/1602.00602).
- **Kalibera & Jones**, "Rigorous Benchmarking in Reasonable Time" (ISMM 2013, DOI
  10.1145/2464157.2464160) **[paper, checked 2026-11-09]** (https://kar.kent.ac.uk/33611/45/p63-kaliber.pdf):
  "repetition is most needed where most uncertainty arises (whether between builds, between
  executions or between iterations)"; measure only once iteration times "are (statistically)
  independent and identically distributed"; the DaCapo and Georges-et-al steady-state heuristics
  "do not perform well"; a cookbook yields optimal repetition counts per level (SPEC CPU needs
  4–6 executions for 1% precision; some DaCapo benchmarks tens); "Report performance changes with
  effect size confidence intervals" ("we are 95% confident that system A is faster than system B
  by 5.5% ± 2.5%"). The closed-form repetition formula (their §9.2, eq. 2) was not legibly
  extracted.
- **criterion.rs**: warm-up by doubling iterations until `warm_up_time`; linear sampling; whole
  samples timed; Tukey fences at 1.5 and 3 IQR, outliers kept but flagged; bootstrap confidence
  intervals; bootstrapped t-test with a configurable noise threshold **[vendor]**
  (https://bheisler.github.io/criterion.rs/book/analysis.html).
- **Benchmarking crimes** (Heiser; van der Kouwe et al., IEEE EuroS&P 2019, 22 flaws, ~5 per
  paper on average in a survey of ~50 top-tier papers): selective benchmarking; micro-benchmarks
  as overall performance; "throughput degraded by x% means overhead is x%"; no significance;
  arithmetic mean of normalised scores; wrong baseline; evaluating only against yourself;
  missing platform specification; relative numbers only **[third party]**
  (https://gernot-heiser.org/benchmarking-crimes.html, https://www.vusec.net/projects/benchmarking-crimes/).

For a driver benchmark suite the notes conclude: drive the server-side comparison open-loop at
fixed arrival rates (or partly-open with >= 10 requests per session when modelling pooled
application threads) and record with HdrHistogram's expected-interval correction; report max,
p99, p99.9, not means; label any closed-loop "N threads hammering" test as a different
experiment that flatters the tail; compare Rust with JVM drivers only after the JVM run is shown
to be in steady state (changepoint or Kalibera's independence test) and report warm-up
separately because 56% of VM/benchmark pairs never settle; repeat at the process level (new
process, new connection); report effect sizes with bootstrap CIs and absolute numbers alongside
ratios. ConnectorX's methodology (five runs averaged, no CI) and Raasveldt's (single warm run)
fall short of this.

### 6.9 Compression and TLS throughput

Codec throughput, single thread, Silesia **[vendor, checked 2026-11-09]** (lz4 README, lzbench
2.4.1, i7-9700K, GCC 13.3, https://github.com/lz4/lz4; zstd README, lzbench, i7-9700K @ 4.9 GHz,
gcc 14.2, https://github.com/facebook/zstd — the two READMEs differ slightly for the same codecs):

| Codec | Ratio | Compress MB/s | Decompress MB/s |
|---|---:|---:|---:|
| memcpy | — | 13,700 | — |
| LZ4 1.11.0 (lz4 README) / 1.10.0 (zstd README) | 2.112 / 2.101 | 770 / 675 | 4,950 / 3,850 |
| LZ4 HC -9 | 2.717 | 40 | 4,800 |
| zstd 1.5.7 -1 | 2.896 | 510 | 1,570 / 1,550 |
| zstd --fast=1 | 2.439 | 545 | 1,850 |
| zstd --fast=4 | 2.146 | 665 | 2,050 |
| Snappy 1.3.1 / 1.2.1 | 2.090 / 2.089 | 520 | 1,350 / 1,500 |
| zlib 1.3.2 -1 / 1.3.1 -1 | 2.743 | 110 / 105 | 410 / 390 |
| zlib -6 | 3.106 | 36 | 430 |
| zlib-ng -1 | 2.252 | 250 | 555 |
| brotli -1 | 2.883 | 290 | 425 |

Break-even bandwidths (derived from these tables with the 6.1 inequalities; database columns
compress differently — Raasveldt saw 2.2x Snappy on lineitem, 10x on ontime): LZ4 (r = 2.1,
C_c = 770, C_d = 4,950 MB/s) sequential `B < (1 - 1/2.1) / (1/770 + 1/4950) ≈ 349 MB/s ≈ 2.8
Gbit/s`, overlapped `B < min(C_c, C_d) = 770 MB/s ≈ 6.2 Gbit/s`; zstd -1 sequential ≈ 251 MB/s ≈
2.0 Gbit/s, overlapped ≈ 4.1 Gbit/s; zstd --fast=4 ≈ 2.1 / 5.3 Gbit/s; zlib -1 ≈ 0.42 / 0.84
Gbit/s. Against links: at 1 Gbit/s all of LZ4/zstd/zlib pay; at 10 Gbit/s none pays on one
core (LZ4/zstd only with >= 2–3 compressing cores and overlapped pipelines); at 25 Gbit/s only
with >= 4–6 cores per direction, i.e. not worth it for a client driver. Decompression is cheap
for LZ4 (≈ 4–5 GB/s ≈ 32–40 Gbit/s per core): a client can always afford to accept
LZ4-compressed streams; the question is whether the server can afford to compress. This matches
Raasveldt's heuristic and MySQL's "compression dominates even at 100 Mbit" with zlib.

TLS: AES-GCM went from "~23 cycles per byte in 2009" to "0.65 cycles per byte" on Skylake with
AES-NI + CLMUL (Gueron, https://crypto.stanford.edu/seclab/sem-15-16/gueron.html); Cloudflare
(Haswell AVX2): ChaCha20-Poly1305 "getting below 2 cycles/byte", AVX-512 projected below 1;
"AES-128-GCM and AES-256-GCM both beat ChaCha20-Poly1305 for records larger than 320 bytes";
many connections never reach 16 KB records (often <1400 bytes, then ~4 KB, then 16 KB)
(https://blog.cloudflare.com/it-takes-two-to-chacha-poly/) **[vendor, checked 2026-11-09]**. TLS
1.3 record (RFC 8446 §5): 1-byte opaque_type (23) + 2-byte legacy version (0x0303) + 2-byte
length + encrypted record; inner plaintext = content + 1-byte content type + padding; plaintext
<= 2^14; ciphertext <= 2^14 + 256; 16-byte tags for AES-GCM and ChaCha20-Poly1305 (restated;
RFC 5116/8439) (https://www.rfc-editor.org/rfc/rfc8446). Derived: `overhead_fraction = 22 /
(payload + 22)` — a 64-byte message is 34% framing, 1 KB is 2.1%, a 16 KB record 0.13%; plus a
per-record AEAD setup cost (Cloudflare's 320-byte crossover). Consequence: never flush one TLS
record per protocol message; buffer a pipeline/batch and write once (`writev` -> one record of
up to 16 KB). Cipher budget (derived): 0.65 cycles/byte at 3.5 GHz ≈ 5.4 GB/s ≈ 43 Gbit/s per
core for AES-GCM; ChaCha20-Poly1305 ≈ 1.75 GB/s ≈ 14 Gbit/s per core on Haswell; encryption is
not the bottleneck for a 10–25 GbE client with AES-NI and dwarfs text decoding cost either way;
prefer ChaCha20-Poly1305 on CPUs without AES-NI/CLMUL. Compression ratios for database result-set
chunks exist only in Raasveldt's 2017 tables and should be re-measured with 1 MB column-major
chunks on current codecs; no like-for-like AES-GCM vs ChaCha20 GB/s table on one modern CPU was
reachable.

## 7. Techniques: what the fastest clients do

Each item names the clients that do it and the source in the sections above.

1. **Binary result formats with server-side prepared statements, cached per connection.**
   asyncpg ("implementation of PostgreSQL server binary protocol"; cached per-statement decode
   pipelines), pgx (binary "much quicker", automatic preparation and caching), pgJDBC
   (`binaryTransfer=true` once server-prepared), sqlx (binary mode, prepared statements cached per
   connection), mysql_async/mysql (`exec*` binary with LRU statement cache), Npgsql (automatic
   preparation). Measured: pgx 9.6 MB binary vs 17.7 MB text for 100 k uuid/timestamp rows
   (section 3); MariaDB binary cache hit 58.3 µs vs text 62.7 vs binary cache miss 118.9 µs
   (section 3) — the miss cost is why MariaDB and MySQL Connector/J default to text and why a
   cache is mandatory, not optional.
2. **Extended protocol with server-side parameter binding as the only default**, never client-side
   literal inlining (psycopg3, pgJDBC `preferQueryMode=extended`; the CVE class in section 8 lives
   entirely in the non-default simple mode).
3. **A dedicated connection task with per-request response channels, so requests from many
   handlers share one pipelined socket.** tokio-postgres (`Client`/`Connection` split), xitca
   (pooled pipelining), may_postgres, vertx-pg-client, Npgsql `NpgsqlBatch`, MariaDB Connector/J
   "Optimistic Prepare" and single-byte-array pipelined writes (CONJ-1015). This is the 20–40x
   TechEmpower gap and the refactor sqlx cannot make without breaking `Executor` (section 2).
   PostgreSQL's own arithmetic: 100 statements at 300 ms RTT, 30 s vs 0.3 s (section 6.6).
4. **One `write` per logical request, `TCP_NODELAY` on every socket.** pgJDBC (`tcpNoDelay=true`,
   `maxSendBufferSize=8192`), Npgsql (8 KB buffers), MariaDB (CONJ-1015). Avoids the 40 ms
   `TCP_DELACK_MIN` stall (section 6.6) and, under TLS, one 22-byte record per message (6.9).
5. **Read into a large buffer and walk messages in memory; never one `read` per packet.** The
   TDS 4,096-byte packet and MySQL 16 MiB packet both require a logical byte stream (go-mssqldb,
   tiberius); sqlx #2007 showed ~100x the `read()` calls of tokio-postgres from a buffered-stream
   routine called with tiny byte counts (section 2).
6. **Zero-copy, reference-counted row buffers with in-place decode and lazy per-column
   conversion.** tokio-postgres (`Row::get<'a>` borrows `&str`/`&[u8]` from the row buffer; `Bytes`),
   xitca-postgres ("sliced and parsed without copying"), sqlx ("decoded on demand"), asyncpg
   ("careful buffer management"). The allocation term in section 6.7 disappears.
7. **Check the null bitmap first, reuse decoder state, decode primitives first.** mssql-jdbc's
   2026 PRs #2600, #2974, #2975, #2991 (section 3); NBCROW (TDS), MySQL binary NULL bitmap,
   PostgreSQL's -1 length all allow skipping decoder setup for NULLs.
8. **A fixed 128-bit fast path for decimals** (TDS DECIMALN is at most 16 bytes; tiberius stores
   NUMERIC in i128 with precision <= 38; arrow-odbc maps Decimal(p <= 38) to Decimal128), instead
   of pgx's big.Int per four digits or rust_decimal's 96-bit mantissa (sections 2, 5).
9. **Batch rewriting and server-side bulk statements for inserts.** pgJDBC `reWriteBatchedInserts`
   (83.99 -> 48.78 ms median; 161,990 -> 292,091 rows/s), MariaDB COM_STMT_BULK_EXECUTE (6,490 vs
   219 ops/s for 100-row batches on the Node connector), mssql-jdbc `useBulkCopyForBatchInsert`,
   pgx batches (890 k rows/s), asyncpg (900 k rows/s) (section 3).
10. **Binary COPY / bulk load as the bulk path.** PgBulkInsert 712,418 rows/s vs 39,225 for
    individual INSERTs on one connection (section 3); DuckDB postgres_scanner reads via
    `COPY ... TO STDOUT (FORMAT binary)` with ctid-range parallelism; TDS INSERT BULK streams
    rows "the same as if the data were being selected from the server" (section 5).
11. **Columnar chunks of about 1 MB (or 1,024–2,048 rows) as the unit of decode and transfer.**
    Raasveldt (1 MB optimum), X100 (128–8K values, default 1024), DuckDB data chunks (2,048 rows),
    ClickHouse Blocks (`max_block_size` 65,409) and ~1 MB compression frames, Databricks 20 MB
    Arrow chunks, Instant Loading `0.25–1.0 x L3 / threads` (sections 5, 6.1, 6.4).
12. **Hand out Arrow buffers through the C Data Interface instead of materialising rows.**
    ADBC/Flight/DuckDB; the Columnar measurement (13x, 5x less heap vs JDBC `CachedRowSet` on the
    same engine) and DuckDB's 38x ADBC-vs-ODBC are the payoff (sections 3, 6.3).
13. **Count-then-preallocate for large known results; chunked growth otherwise.** ConnectorX
    (section 6.2), with its own Q19 regression as the warning.
14. **Parallel partitioned reads only as an explicit API, 4–12 connections for 10 Gbit/s links,
    and server-side endpoints where the protocol has them.** ConnectorX, Flight endpoints,
    Databricks Cloud Fetch, BigQuery streams, DuckDB ctid ranges (sections 5, 6.2, 6.6).
15. **Skip unchanged column metadata.** MariaDB `MARIADB_CLIENT_CACHE_METADATA` (MDEV-19237;
    "10-30%" on 10.6+), MySQL `CLIENT_OPTIONAL_RESULTSET_METADATA`, Connector/J
    `cacheResultSetMetadata` (section 5).
16. **TLS-first connects to save a round trip and reach TLS 1.3.** PostgreSQL 17
    `sslnegotiation=direct` with ALPN `postgresql` (only with `sslmode >= require`); TDS 8.0
    `Encrypt=strict` with ALPN `tds/8.0` (section 5; defaults in section 8).
17. **Prefetch sizing to collapse fetch round trips.** python-oracledb `arraysize` 100 -> 1000
    cuts 10,000 rows from 101 to 11 round trips; `prefetchrows = expected rows + 1` removes the
    end-of-fetch trip; Oracle JDBC `defaultRowPrefetch` (default 10) raised; Spark `fetchsize`
    for "drivers which default to low fetch size (e.g. Oracle with 10 rows)" (sections 5, 10).
    Pipeline the next fetch before decoding the current one when the protocol forces fetch
    requests (6.6).
18. **Compression as a per-connection policy keyed on link speed**: off on localhost, LZ4/zstd-fast
    below ~2–6 Gbit/s per core, never zlib above ~0.8 Gbit/s (6.1, 6.9); always accept
    LZ4-compressed streams (ClickHouse, Databricks) because decompression is 4–5 GB/s per core.
19. **Small, warm pools sized by Little's law and the HikariCP cap, with FIFO/LIFO handoff and
    two-choice checkout if partitioned**; HikariCP's ~20 ns connection cycle is the floor for pool
    overhead; PostgreSQL's per-connection cost and `GetSnapshotData` tax argue for pipelining over
    connection count (6.5).
20. **Prepared-statement caches sized from the hit-rate model, not unlimited**, because server-side
    memory scales as `pool_size x cache_size` (6.5); pgJDBC's 256 statements / 5 MiB per connection
    is the shipped example.
21. **SIMD for delimiter and structural search and for UTF-8 validation; branch-reduced scalar code
    for short tokens.** picohttpparser-style `pcmpistri` (Instant Loading), simdjson, fast_float,
    the Keiser–Lemire UTF-8 lookup (6.4); the X100 argument against per-value virtual calls.
22. **For SQLite: open with `SQLITE_OPEN_NOMUTEX` and enforce exclusivity in Rust; skip
    `sqlite3_column_type` when the declared affinity is trusted; borrow text/blob until the next
    `step`; prepare with `SQLITE_PREPARE_PERSISTENT`; bind with `SQLITE_STATIC`** (section 5).
    rusqlite's 0.37 µs trivial query is the floor to match.
23. **Measure open-loop at fixed arrival rates with HdrHistogram and intended-start timestamps**
    (wrk2, pgbench `-R`, Hyperfoil), report max/p99/p99.9 and CPU-seconds per million rows, after
    JVM steady state is demonstrated (6.8; Quarkus's pinned-core harness in section 3).

## 8. Security

### Authentication floors per database

**[vendor, checked 2026-11-09]** unless tagged.

| Database | Floor for parity | Legacy / deprecated | Source |
|---|---|---|---|
| PostgreSQL | SCRAM-SHA-256 and **SCRAM-SHA-256-PLUS** (channel binding `tls-server-end-point`, RFC 5929; the server lists `-PLUS` first when built with SSL; libpq and pgJDBC default `channel_binding=prefer`, "only supported over SSL connections with PostgreSQL 11 or later"); `require_auth` allow-list (`password, md5, gss, sspi, scram-sha-256, oauth, none`, negatable with `!`); GSS/SSPI; OAUTHBEARER (PG 18, no `-PLUS`, two-connection discovery) | MD5 ("deprecated and will be removed in a future release"; the server auto-upgrades to SCRAM when the stored hash is SCRAM); cleartext `password` | https://www.postgresql.org/docs/current/sasl-authentication.html, https://www.postgresql.org/docs/current/auth-password.html, https://www.postgresql.org/docs/current/libpq-connect.html, https://www.rfc-editor.org/rfc/rfc5929.html; RFC 5802 / 7677 cited but not re-fetched |
| MySQL | `caching_sha2_password` (default via `authentication_policy` in 8.4): fast path = challenge-response that "does not require a secure connection" after a cache hit; first login after creation / password change / `RENAME USER` / `FLUSH PRIVILEGES` needs TLS, Unix socket, shared memory, or the RSA key-pair exchange (`--get-server-public-key` fetches the key from the server, "less secure"; `--server-public-key-path` uses a trusted local copy) else `ERROR 2061 ... Authentication requires secure connection` | `mysql_native_password`: deprecated 8.0.34, disabled by default 8.4 (`--mysql-native-password=ON` re-enables), **removed from the 9.0.0 server (2024-07-01)**; the client-side plugin remains and became dynamically loadable; `sha256_password` deprecated, "subject to removal" | https://dev.mysql.com/doc/refman/8.4/en/caching-sha2-pluggable-authentication.html, https://docs.oracle.com/cd/E17952_01/mysql-8.4-en/native-pluggable-authentication.html, https://dev.mysql.com/doc/relnotes/mysql/9.0/en/news-9-0-0.html (the Oracle blog post "it's time to abandon the weak authentication method" returned 403) |
| MariaDB | `client_ed25519` ("hashes and signs the password using ECDSA"; shipped but not installed by default; supported by Connector/C, ODBC, J, Node.js, MySqlConnector); **PARSEC** (11.6+: PBKDF2-derived ed25519 key; 32-byte server scramble, ext-salt request, client scramble + signature; `parsec_iterations` in 13.1; "intended to be the default in a future release", MDEV-12320) | native password | https://mariadb.com/docs/server/reference/plugins/authentication-plugins/authentication-plugin-ed25519, https://mariadb.com/kb/en/authentication-plugin-parsec/ (the KB `ed25519` page did not render; its introduction version is unsourced) |
| SQL Server | SQL login; NTLM/Kerberos via SSPI (Windows; tiberius `winauth`) and GSSAPI / `sspi-rs` (Unix); Entra ID token path (FedAuth; mssql-jdbc `accessToken` property and modes `ActiveDirectoryIntegrated` (6.0), `ActiveDirectoryManagedIdentity` (8.3.1), `ActiveDirectoryInteractive`/`ServicePrincipal` (9.2), `ActiveDirectoryDefault` (12.2), `ServicePrincipalCertificate` (12.4)) | `ActiveDirectoryPassword` deprecated | https://learn.microsoft.com/en-us/sql/connect/jdbc/connecting-using-azure-active-directory-authentication, tiberius `test.yml` |
| Oracle | wallets (mTLS, `TNS_ADMIN=<wallet dir>`) or TLS-only without a wallet (JDK 8u162+); TLS 1.3 with 1.2 fallback | — | https://docs.oracle.com/en/cloud/paas/autonomous-database/serverless/adbsb/connect-jdbc-thin-wallet.html; Thin-driver Kerberos docs not found |
| Cloud IAM | AWS RDS: SigV4 token, **15-minute lifetime**, used as the password over mandatory TLS, "generally about 1 KB but can be larger"; GCP Cloud SQL: OAuth 2.0 access token (**1 hour**) as the password with `sslmode=require`, username = full IAM e-mail, token "too long to paste" | — | https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/UsingWithRDS.IAMDBAuth.html, https://docs.cloud.google.com/sql/docs/postgres/iam-logins, https://docs.cloud.google.com/sql/docs/mysql/iam-overview |

What the floors imply: a driver that ships SCRAM-SHA-256 without `-PLUS` is already behind libpq
and pgJDBC; the `-PLUS` variant costs one certificate hash (the leaf certificate's DER and
signature-algorithm OID must be exposed by the TLS stack — confirm for rustls and any in-house
stack) and is the only thing that stops a certificate-relaying MITM when `sslmode < verify-full`.
MD5 belongs behind a legacy feature or runtime warning with a `require_auth`-style allow-list to
prevent downgrade by a hostile server. For MySQL, `caching_sha2_password` fast and full paths
over TLS are mandatory; RSA exchange over plaintext is needed for parity with libmysqlclient but
should default off unless the key file is local. All cloud IAM schemes reduce to "password =
short-lived token; TLS mandatory; token up to several KB; refresh before 15 min / 1 h", so a
`TokenProvider` callable per connection attempt covers RDS, Cloud SQL, Entra `accessToken` and
PG 18 OAUTHBEARER without a protocol change. Tokens >= 1 KB must survive the password path.

### TLS defaults and the cameleer-tls verdict

Every ecosystem's default is still "prefer/optional" while the vendors say it is not
recommended **[vendor, checked 2026-11-09]**:

- libpq `sslmode` default `prefer` ("not recommended in secure deployments"); `require` gives
  eavesdropping protection but no MITM protection; `verify-ca` depends on CA policy;
  `verify-full` both (SAN dNSName/iPAddress first, CN only without SAN; `*` does not match `.`).
  Quirk: with a root CA file present, `require` behaves like `verify-ca`; apps "should not rely
  on this". `sslrootcert=system` loads the implementation's trusted roots and "the default
  `sslmode` is changed to `verify-full`, and any weaker setting will result in an error";
  `ssl_min_protocol_version` default `TLSv1.2` (https://www.postgresql.org/docs/current/libpq-ssl.html,
  https://www.postgresql.org/docs/current/libpq-connect.html).
- SQL Server: connection-string `Encrypt` default is **false**; `mandatory|true` = TDS 7.x + TLS
  (TLS up to 1.2 only); `strict` = TDS 8.0, TLS first, TLS 1.3, `TrustServerCertificate=true` not
  allowed (use `HostNameInCertificate`); minimum driver versions for `strict`: ADO.NET 5.1, ODBC
  18.1.2.1, OLE DB 19.2.0, JDBC 11.2.0, PHP 5.10 (https://learn.microsoft.com/en-us/sql/relational-databases/security/networking/tds-8).
  mssql-jdbc's CVE-2025-59250 fix line was reported downstream to have "encryption now enabled by
  default" **[third party]** (https://documentation.coremedia.com/cmcc-12/current/release-notes-webhelp/content/CMS-28866-1-2406.2.2.html).
- MySQL 8.4 supports TLS 1.2 and 1.3 only (1.0/1.1 removed in 8.0); default 1.3 suites
  `TLS_AES_128_GCM_SHA256, TLS_AES_256_GCM_SHA384, TLS_CHACHA20_POLY1305_SHA256, TLS_AES_128_CCM_SHA256`
  (https://dev.mysql.com/doc/refman/8.4/en/encrypted-connection-protocols-ciphers.html). The
  client `--ssl-mode` default (`PREFERRED`) was not on the fetched page.

Rust TLS stacks: `rustls-platform-verifier` uses Windows CryptoAPI (revocation), macOS
Security.framework (revocation), Android Trust Manager, Linux/BSD system bundle via webpki (no
revocation, loaded once); its maintainers recommend it "as the default for client-side apps on
common OSes", with `webpki-roots` for frequently redeployed containers; MSRV 1.85
(https://github.com/rustls/rustls-platform-verifier). `native-tls` (SChannel / Secure Transport /
OpenSSL), rustls with `aws-lc-rs` or `ring`, and `vendored-openssl` appear in the sqlx and
tiberius CI matrices; tiberius notes Secure Transport "can't complete the SQL Server TLS
handshake" on macOS.

Measured: ISRG/Prossimo Q1 2026 (published 2026-04-09, run March 2026; Rustls 0.23.37 + aws-lc-rs
1.16.0 vs OpenSSL 3.6.1 vs BoringSSL): Rustls first in 14 of 16 tests, second in 2; BoringSSL
2/10/4; OpenSSL 0/4/12; per-suite MB/s only in charts **[vendor: ISRG funds rustls; checked
2026-11-09]** (https://www.memorysafety.org/blog/26q1-rustls-performance/). InfoQ's summary:
OpenSSL "about 1.38x slower on handshakes and about 1.18x slower on throughput" on x86_64 **[third
party]** (https://www.infoq.com/news/2026/09/rustls-one-decade/). January 2024 (Rustls 0.22 vs
OpenSSL 3.2): bulk send roughly even on AES, receive 7–17% slower due to an extra copy, ChaCha20
~45% behind (AVX-512 in OpenSSL) (https://www.memorysafety.org/blog/rustls-performance/). No
independent (non-ISRG) 2025/2026 benchmark was found; the harness is public.

**cameleer-tls** (crates.io: "Cameleer std-only TLS 1.3 / TLS 1.2 client and server with
from-scratch cryptography"; 0.2.0 on 2026-11-05 through 0.4.0 on 2026-11-06; 297 downloads;
Apache-2.0; repo `github.com/cameleering/cameleer`; only dependency `cameleer-core ^0.4.0`; source
752.9 kB; doc coverage 54.54%) **[vendor, checked 2026-11-09]**
(https://crates.io/api/v1/crates/cameleer-tls, https://docs.rs/cameleer-tls/latest/cameleer_tls/).
Its 0.4.0 docs list TLS 1.3 suites `TLS_AES_128_GCM_SHA256`, `TLS_AES_256_GCM_SHA384`,
`TLS_CHACHA20_POLY1305_SHA256`, HelloRetryRequest, KeyUpdate, no 0-RTT; TLS 1.2 ECDHE-only with
AEAD suites and extended master secret, renegotiation refused; groups x25519, secp256r1,
secp384r1; signatures Ed25519, ECDSA P-256/P-384, RSA-PSS and PKCS#1 v1.5; X.509 checks with SAN
matching (one left-most wildcard label, CN only without SAN); CRL revocation (a certificate with
no matching CRL is accepted); name constraints and policies tolerated, not enforced; OCSP status
contradictory (listed as not implemented and as an `ocsp` module); PSK resumption; client
certificates; `/dev/urandom` / `BCryptGenRandom` feeding a ChaCha20 DRBG. Constant-time claims
are "best effort": ChaCha20/Poly1305/X25519/GHASH/P-256/P-384 intended constant-time, but **AES
uses table-based S-boxes (cache-timing exposed; the docs recommend ChaCha20-Poly1305 on shared
hardware), RSA has no CRT or blinding, secrets are not zeroized on drop**, and the implementation
"has not been audited". No measured handshake or throughput numbers exist for it.

Verdict from the security note: cameleer-tls meets the *feature* bar for database TLS (1.3 +
1.2, x25519/P-256, ECDSA/RSA-PSS/Ed25519, SAN hostname check, client certs, CRLs) and is
dependency-free, but it is not yet a defensible *default* for a security product (no audit,
unblinded RSA without CRT, no zeroization, CRL-only revocation with no OCSP, and no AES-NI or
PCLMULQDQ path, so bulk AES-GCM is slow). A reasonable posture: rustls + aws-lc-rs (or ring for
size) as default, cameleer-tls as an opt-in `std-only` feature.

**AES discrepancy resolved [checked 2026-11-09 against the source].** The crate's module doc in
`src/lib.rs` (and therefore the docs.rs page) still says "AES uses a table-based S-box and is
therefore exposed to cache-timing attacks". The implementation does not: `src/crypto/aes.rs`
computes SubBytes "without table lookups", bitslicing the 16 state bytes into eight planes and
evaluating the S-box as GF(2^8) inversion, with a test that the constant-time S-box matches the
table (`constant_time_sbox_matches_table`). The `SBOX` tables exist only for that test and the
key-schedule cross-check. The sibling HTTP-server note was right; the `lib.rs` caveat is stale and
should be corrected in the Cameleer repository. AES-GCM in cameleer-tls is constant time but
software-only. Separately, `tls-server-end-point` channel binding needs the leaf certificate's
DER and signature OID from whichever stack is used.

Safe 2026 default drawn from the vendor direction (`sslrootcert=system`, `Encrypt=strict`):
`verify-full` semantics whenever TLS is requested, `prefer` only as an explicit opt-in,
`require`/`verify-ca` for self-signed deployments, `disable` explicit; for SQL Server default to
`mandatory` with certificate validation and support `strict`; implement PG 17 direct SSL only
when `sslmode >= require`, as libpq enforces.

### CVE classes and the design rule each implies

The record of existing drivers splits into four recurring classes **[vendor advisories and third
party, checked 2026-11-09]**.

| Class | Incidents | Design rule |
|---|---|---|
| **1. Client-side parameter inlining** | pgJDBC **CVE-2024-1597** (GHSA-24rp-q3w6-vc56, 2024-02-21, CVSS 10.0, Sonar): only with non-default `preferQueryMode=simple`; "When a numeric placeholder directly follows a minus sign, the negated value becomes a `--` comment marker" that swallows the opening quote of a later string placeholder; fix wraps inlined parameters as `-('-1'::int4)`; fixed 42.2.28 … 42.7.2. pgx **CVE-2024-27289** (same class, fixed 4.18.2, CVSS 8.1). **CVE-2006-2314** (PostgreSQL + psycopg): SJIS/BIG5/GBK/GB18030/UHC multibyte characters ending in `\` defeat `\'` escaping. libpq **CVE-2025-1094** (fixed 2025-02-13 in 17.3/16.7/15.11/14.16/13.19, CVSS 8.1): `PQescapeLiteral/Identifier/String[Conn]` "miss neutralizing quoting syntax in text that fails encoding validation". | Never inline parameters client-side by default. If a simple/text mode exists, wrap every inlined literal in a typed cast with explicit parentheses and reject or escape newlines, `--` and `/*`; regression-test `-?` followed by `?` on one line. Escaping must be encoding-aware and fail closed: validate bytes before quoting, refuse invalid multibyte sequences, prefer `''` over `\'`, force UTF-8 sessions where possible. |
| **2. Length-prefix overflow / message smuggling** | sqlx **RUSTSEC-2024-0363** / GHSA-xmrp-424f-vfpx (<= 0.8.0, patched 0.8.1, 2024-08-16): "encoding a value larger than 4GiB can cause the length prefix in the protocol to overflow"; root cause a truncating cast `(self.len() - offset - 4) as i32` in `sqlx-postgres/src/arguments.rs` that "has existed essentially since the beginning"; PostgreSQL exploit demonstrated; the advisory also warns `Encode::size_hint()` is unreliable for `Json<T>`/`Text<T>`. pgx **CVE-2024-27304** (GHSA-mrww-27vc-gghv, 2024-03-04): one > 4 GB message sent as multiple attacker-controlled messages; fixed 4.18.2 / 5.5.4. Source talk: Gerste, "SQL Injection Isn't Dead: Smuggling Queries at the Protocol Level", DEF CON 32. | Checked length arithmetic everywhere: all length prefixes (PG `i32`, MySQL 3-byte with 16 MiB continuation, TDS packet/stream) through `try_from`/`checked_add` and a configurable `max_message_size`; never `as i32`/`as u32`; `clippy::cast_possible_truncation` as `deny` in codec crates; fuzz the **encoder** as well as the decoder (both bugs were on the encode side); `encode(v).len() == size_hint(v)` as a property. |
| **3. Connection-string-driven code loading, deserialisation, file reads** | pgJDBC **CVE-2022-21724** (2022-02-01): `authenticationPluginClassName`, `sslhostnameverifier`, `socketFactory`, `sslfactory`, `sslpasswordcallback` instantiated without interface checks; Spring `ClassPathXmlApplicationContext` with a remote XML as the demo. MySQL Connector/J **CVE-2017-3523** (deserialisation from database contents even with `autoDeserialize` off; malicious server -> client RCE), **CVE-2023-21971** (`propertiesTransform`, CVSS 5.3), **CVE-2023-22102** (`autoDeserialize=true`, CVSS 8.3, "takeover of MySQL Connectors"). Rogue-server `LOAD DATA LOCAL INFILE` file read (Percona 2019; OpenRefine GHSA-qqh2-wvmv-h72m with `allowLoadLocalInfile=true` injected via the database-name field; 2026 MariaDB Connector/J **CVE-2026-119210** (fixed 3.3.7/3.4.5/3.5.11) and **CVE-2026-61700**: even `allowLocalInfile=false` did not block a server that echoes a filename). mssql-jdbc **CVE-2025-59250** (2025-10-14, CVSS 8.1, CWE-20 spoofing; root cause undisclosed). | No code loading or type instantiation from connection strings: plugins (auth, TLS verifier, socket factory) are trait objects supplied by the application via a builder, never resolved from a URL. Unknown connection-string keys are errors. Treat database-name/user fields as untrusted when they come from end users. Server-initiated client actions are opt-in per statement: `LOAD DATA LOCAL` uploads only if the application requested it for that statement and the filename matches; never deserialise server-sent bytes into typed objects beyond declared column types; cap BLOB/row sizes. |
| **4. Driver-generated SQL with unescaped identifiers** | pgJDBC **CVE-2022-31197** (2022-08-03, CVSS 5.9): `ResultSet.refreshRow()` built SQL without escaping column names, so a column named with `;` injected SQL; fixed 42.2.26, 42.3.7, 42.4.1. | Every metadata/refresh/`RETURNING`-synthesis path uses the driver's identifier quoter (`"..."` with doubled quotes, `` ` `` for MySQL, `[]`/`""` for TDS), tested with identifiers containing `;`, quotes, newlines and `--`. |

Sources: https://github.com/pgjdbc/pgjdbc/security/advisories/GHSA-24rp-q3w6-vc56,
https://nvd.nist.gov/vuln/detail/CVE-2024-27289, https://tenable.com/plugins/nessus/21613,
https://www.postgresql.org/support/security/CVE-2025-1094/,
https://rustsec.org/advisories/RUSTSEC-2024-0363.html,
https://github.com/jackc/pgx/security/advisories/GHSA-mrww-27vc-gghv,
https://simonwillison.net/2024/Aug/12/smuggling-queries-at-the-protocol-level,
https://github.com/pgjdbc/pgjdbc/security/advisories/GHSA-v7wg-cpwc-24m4,
https://sector7.computest.nl/post/2017-04-mysql-connectorj, https://www.oracle.com/security-alerts/cpuapr2023.html,
https://www.oracle.com/security-alerts/cpuoct2023.html,
https://www.percona.com/blog/2019/02/06/percona-responds-to-mysql-local-infile-security-issues/,
https://github.com/OpenRefine/OpenRefine/security/advisories/GHSA-qqh2-wvmv-h72m,
https://app.opencve.io/cve/CVE-2026-119210, https://jira.mariadb.org/browse/CONJ-1318,
https://github.com/advisories/GHSA-m494-w24q-6f7w,
https://github.com/pgjdbc/pgjdbc/security/advisories/GHSA-r38f-c4h4-hqq2. A seventh rule from the
same note: credential hygiene — passwords and tokens in a `Secret<String>`-style wrapper with
redacted `Debug`, zeroized on drop (the gap cameleer-tls admits for itself), never logged.

The pattern matches the sibling HTTP-server note's finding: in memory-safe languages the bugs
are leniency and arithmetic, not memory corruption. A new Rust driver inherits memory safety
only where it avoids `unsafe`; its real exposure is classes 1–4, and that is what testing has to
target. Third-party reports that CVE-2025-1094 was the initial-access vector in the BeyondTrust
compromise were not confirmed from primary sources.

### Fuzzing and CI practice

- `cargo-fuzz` "needs a nightly compiler", "only works on x86-64 and Aarch64, and only on
  Unix-like operating systems (not Windows)", needs a C++11 compiler; the Rust Fuzz Book covers
  `afl.rs` and `cargo-fuzz`, structure-aware fuzzing, coverage and CI **[vendor, checked
  2026-11-09]** (https://github.com/rust-fuzz/cargo-fuzz, https://rust-fuzz.github.io/book/introduction.html).
- Existing Rust protocol fuzzing is mostly proptest-based: `pg-wired` `tests/protocol_fuzz.rs`
  feeds random bytes to `parse_message` and checks for panics, unknown tags, truncated bodies and
  bad lengths; `qail-pg` has a wire proptest rejecting short headers and over-declared lengths;
  `spg-server` hand-rolled a harness because "cargo fuzz requires nightly + libFuzzer"
  (https://docs.rs/crate/pg-wired/0.4.0/source/tests/protocol_fuzz.rs,
  https://docs.rs/crate/qail-pg/latest/source/tests/wire_proptest.rs,
  https://docs.rs/crate/spg-server/7.8.0/source/tests/e2e_fuzz.rs).
- **No mainstream Rust DB driver CI runs fuzzing or Miri except diesel**: diesel runs
  AddressSanitizer on the sqlite-, postgres- and mysql-bundled builds, nightly Miri on the default
  target and on big-endian `s390x` plus a native-library SQLite Miri run, `cargo xtask
  semver-checks`, a `riscv32imac-unknown-none-elf` no-std SQLite build and wasm32 tests
  (https://raw.githubusercontent.com/diesel-rs/diesel/master/.github/workflows/ci.yml). sqlx CI has
  "No docs build, Miri, fuzzing, or proptest jobs"; rust-postgres CI has none either (section 11).
  No public libFuzzer harness in sqlx, tokio-postgres, mysql_async or tiberius was found; no
  documented `unsafe` count for them was located.
- Supply chain: `cargo-vet` "helps projects confirm that third-party Rust dependencies were audited
  by a trusted entity", with shared audits, diff audits and exemptions (https://mozilla.github.io/cargo-vet/);
  RustSec (`cargo audit`) carries RUSTSEC-2024-0363, cross-listed on OSV; `cargo-deny` covers
  licences, bans, duplicates and advisories. A near-zero-dependency driver makes `cargo vet`
  tractable.
- The note's inferences: `#![forbid(unsafe_code)]` on all protocol/codec crates is realistic (no
  parsing needs unsafe); confine `unsafe` to FFI crates (SQLite, SSPI/GSSAPI, SChannel) and any
  in-house crypto with `// SAFETY:` comments and Miri coverage (diesel shows Miri on big-endian is
  practical); run libFuzzer on Linux CI and keep a `proptest` fallback so Windows/macOS exercise
  the same corpora; store crash corpora as regression tests; disable multi-statement strings by
  default in the extended protocol (PostgreSQL's Parse rejects them anyway) and, if a text/batch
  mode is provided, tokenise with dollar-quoting, `E''`, nested `/* */` and MySQL `DELIMITER`
  awareness and fuzz the tokeniser.

## 9. Size and dependencies

### What is known

- **The hello-world floor** (min-sized-rust, macOS x86_64, nightly) **[third party, checked
  2026-11-09]** (https://github.com/johnthagen/min-sized-rust): `build-std` + `optimize_for_size`
  -> **51 KB stripped**; + `panic=immediate-abort` -> **30 KB**; `no_main` with careful libstd ->
  **8 KB**; `no_std + no_main + libc` "around 8KB". Unmeasured but listed: `strip=true` (Rust >=
  1.59), `opt-level="z"` (sometimes `"s"` is smaller), `lto=true`, `codegen-units=1` (release
  default 16), `panic="abort"`, `-Zlocation-detail=none`, `-Zfmt-debug=none` (nightly), UPX
  "50–70%". Debug builds "30% or more larger". `prefer-dynamic` rejected (no stable ABI).
- **Trimming anecdotes** **[third party, checked 2026-11-09]**: a CLI with clap/reqwest/tokio/regex
  went 11 MB -> ~10 MB with `lto=true, strip=true, opt-level=3, codegen-units=1, panic="abort"` ->
  **4.5 MB** after feature trimming; per change: reqwest native-tls -> rustls "~4MB (the biggest
  win)", regex Unicode off ~1 MB, easy-archive ~1 MB, clap 100–200 KB, tokio limited to
  `rt-multi-thread` + `macros` ~100 KB; "about 80% of the binary came from about 20% of
  dependencies" (https://dev.to/ahaoboy/i-shrunk-my-rust-binary-from-11mb-to-45mb-with-bloaty-metafile-1n7i).
  Counter-example: AerynOS `boulder` shrank 15.2 -> 13.1 MiB by switching reqwest *from* rustls
  *to* native-tls because it links the system OpenSSL dynamically
  (https://github-users.yhajoa.workers.dev/AerynOS/os-tools/pull/283); on Android native-tls
  statically links OpenSSL and is much larger (https://users.rust-lang.org/t/android-targets-having-significantly-larger-text-size/56832).
- **The only external anchor for a driver stack**: Alpine Linux's `sqlx` (sqlx-cli,
  postgres + mysql + sqlite + rustls, musl static) v3.22 x86_64: **1.8 MiB download, 3.3 MiB
  installed** (stripping status not stated) **[third party, checked 2026-11-09]**
  (https://pkgs.alpinelinux.org/package/v3.22/community/x86_64/sqlx). It also proves the
  sqlx + rustls stack builds for `x86_64-unknown-linux-musl`.
- wtx self-reports **652 K** optimised size for its "Postgres Client" build row with 13 required /
  26 total dependencies and a 6.69 s clean optimised build **[vendor]** (section 2).
- Cameleer (std-only, ~230 components) claims a **5.8 MB** binary with only the `core` feature,
  "~10 MB RAM", startup 3 ms (README) vs 10 ms (About) **[vendor]** (https://github.com/cameleering/cameleer).
- **JDBC side for comparison**: pgJDBC 1.19 MiB, HikariCP 0.17 MiB, MariaDB Connector/J 0.80 MiB,
  Connector/J 2.56 MiB, mssql-jdbc 1.5 MiB, ojdbc 8.6 MiB, sqlite-jdbc 11.8 MiB, duckdb_jdbc
  78.4 MiB (section 3), plus a JVM: 250–390 MB RSS in JVM mode or 70–150 MB as a GraalVM native
  image for a framework app; Quarkus 13 MB native vs 74 MB OpenJDK for a REST app (section 3).

### Dependency counts of the Rust crates

Direct required (non-dev) dependencies from the crates.io index **[vendor, checked 2026-11-09]**
(`https://crates.io/api/v1/crates/<name>/<version>/dependencies`):

| Crate | Required normal deps | Optional | Feature flags |
|---|---:|---:|---:|
| sqlx-core 0.9.0 | 20 (base64, bytes, cfg-if, crossbeam-queue, either, event-listener, futures-core, futures-intrusive, futures-io, futures-util, hashbrown, hashlink, indexmap, log, memchr, percent-encoding, smallvec, thiserror, tracing, url) | 27 (48 entries) | sqlx: 43 (defaults `any, json, macros, migrate`; three runtimes; five TLS variants; eleven type flags; six sqlite sub-features) |
| tokio-postgres 0.7.18 | 18 (async-trait, byteorder, bytes, fallible-iterator, futures-channel, futures-util, log, parking_lot, percent-encoding, phf, pin-project-lite, postgres-protocol, postgres-types, rand, socket2, tokio, tokio-util, whoami) | 0 | — |
| tiberius 0.13.0 | 20 (async-trait, asynchronous-codec, byteorder, bytes, connection-string, encoding_rs, enumflags2, futures-timer, futures-util, libc, num-traits, once_cell, pin-project-lite, pretty-hex, secrecy, thiserror, tiberius-macros, tracing, uuid, zeroize) | 20 (56 entries) | 31 (6 default: native-tls, tds80, winauth, async-native-tls, rustls-pki-types, tds73) |
| mysql_async 0.37.1 | 20 (arc-swap, bytes, crossbeam-queue, crossbeam-utils, flate2, futures-core, futures-sink, futures-util, keyed_priority_queue, lru, mysql_common, percent-encoding, rand, serde, socket2, thiserror, tokio, tokio-util, twox-hash, url) | 8 | — |
| rusqlite 0.41.1 | 5 (bitflags, fallible-iterator, fallible-streaming-iterator, libsqlite3-sys, smallvec) | 15 (20 entries) | — |
| diesel 2.3.14 | 2 (diesel_derives, downcast-rs); `pq-sys`, `mysqlclient-sys`, `libsqlite3-sys` optional | — | 27 |
| sea-orm 2.0.4 | built on sqlx | — | 58 (13 default) |

Pure-Rust drivers converge on ~18–20 direct runtime dependencies (tokio, bytes, futures-*,
TLS-agnostic) regardless of vendor; a new driver family can budget to that. Notable: tiberius's
`tds80` default feature appears in its feature list although TDS 8.0 is not mentioned on its
crate page (section 5).

Build weight of the FFI wrappers: duckdb-rs `bundled` compiles DuckDB from source,
`buildtime_bindgen` "requires a recent Clang", cross-compiling with `bundled` is "best-effort",
`httpfs` needs network at CMake configure time; adbc_snowflake's default build "requires a Go
compiler at build time"; rusqlite `bundled` compiles SQLite 3.54.0 with `cc` **[vendor]**.

Rust ABI crates, if dynamic driver loading were wanted: `abi_stable` 0.11.3 (2026-10-17;
load-time type-layout checking, `#[sabi_trait]`, MSRV 1.61; no code-size or runtime-check cost
stated) and `stabby` (compact sum types; warns a Rust 1.78 change may hurt trait-object
performance) **[vendor]** (https://docs.rs/abi_stable/latest/abi_stable/, https://docs.rs/stabby/latest/stabby/).
The consumers-and-bridges note argues neither is needed for a `repr(C)` ADBC-shaped surface and
that static linking with Cargo features per driver is the lower-risk default for a library whose
goals are minimal size and `forbid(unsafe_code)`.

Tokio's MSRV is 1.85 with a ">= 6 months" policy and LTS lines 1.51.x (to March 2027) and 1.53.x
(to September 2027) **[vendor]** (https://raw.githubusercontent.com/tokio-rs/tokio/master/README.md).

### What is not known

- **No public cargo-bloat table or stripped hello-world size for tokio-postgres, sqlx,
  mysql_async, tiberius or rusqlite (bundled) exists.** lib.rs, which publishes transitive counts
  and sizes, returned 403 for every crate, and the research brief forbade running cargo on
  downloaded code. The only reverse-dependency counts it gave via search are popularity measures
  (rusqlite used by 992 crates, sqlx-core by 1,248), not footprint.
- No `cargo tree` transitive counts for sqlx-all-drivers vs tokio-postgres vs rusqlite.
- No measured tokio vs smol vs blocking-std binary-size delta beyond the ~100 KB tokio
  feature-trim anecdote; no measured cost of monomorphisation vs trait objects for drivers; no
  abi_stable vs stabby head-to-head.
- No per-connection memory numbers for tokio-postgres, sqlx or deadpool.
- No published compile times for rusqlite/duckdb-rs bundled builds.

What the evidence supports: a size budget stated per configuration — (a) single driver, blocking
std I/O, rustls-ring, `opt-level="z"`, fat LTO, cgu=1, panic=abort, strip, glibc; (b) the same
with all drivers; (c) musl static — with sqlx-cli's 3.3 MiB installed as the only external anchor,
so a well-trimmed driver library below that is plausible but must be measured in-repo with
`cargo bloat --release --crates`. Expected big-ticket items from the trimming reports: the TLS
stack (rustls + aws-lc-rs vs ring; vendored OpenSSL worst), `regex` (avoid), `chrono`/`time`
(feature-gate), `serde` (feature-gate JSON types; hand-written wire codecs), tokio (`rt` + `net`
+ `io-util` only, or a blocking `std::net` backend). `panic=abort` changes pool semantics (a panic
in one connection task kills the process) and should be a profile recommendation, not a library
default.

## 10. Reaching other ecosystems

### What each consumer accepts

**[vendor, checked 2026-11-09]** unless tagged.

| Consumer | Accepts | Evidence |
|---|---|---|
| Spark, Trino/Presto, Kafka Connect, Airbyte Java connectors, Metabase, enterprise Java | **JDBC only** | Spark JDBC source: single partition unless `partitionColumn/lowerBound/upperBound/numPartitions` are all set; `fetchsize` helps "drivers which default to low fetch size (e.g. Oracle with 10 rows)" (https://spark.apache.org/docs/3.5.1/sql-data-sources-jdbc.html); Trino connectors implement the Trino SPI, Metabase drivers "are written in Clojure" over a SQL-JDBC base; Kafka Connect: "Find the JDBC 4.0 driver JAR file ... Place these JAR files into share/java/kafka-connect-jdbc" (https://docs.confluent.io/kafka-connectors/jdbc/10.9/jdbc-drivers.html); Airbyte: "most database sources and destinations are written in Java" |
| dbt classic adapters, Superset, Airflow hooks, SQLAlchemy ORMs | **Python DB-API 2.0 + SQLAlchemy dialect** | dbt-postgres installs `psycopg2-binary` (https://pypi.org/project/dbt-postgres); Superset "requires a Python DB-API database driver and a SQLAlchemy dialect" and pins SQLAlchemy 1.4 (https://superset.apache.org/user-docs/6.0.0/configuration/databases) |
| pandas 2.2+, Polars, DuckDB, DataFusion, dbt Fusion, dlt, GDAL 3.11+, R (adbi, adbcdrivermanager), Ruby (red-adbc, activerecord-adbc-adapter), Go (`database/sql` adapter), C#, Java (JNI driver manager since ADBC 21), JavaScript (driver manager since ADBC 23) | **ADBC** | https://arrow.apache.org/adbc/current/integrations.html; pandas `read_sql` accepts an ADBC connection since 2.2.0; Polars `read_database_uri` defaults to ConnectorX and takes `engine="adbc"` (https://docs.pola.rs/api/python/stable/reference/api/polars.read_database_uri.html); "DuckDB itself is an ADBC driver" |
| Power BI (`Odbc.DataSource`, 64-bit only), Tableau ("Other Databases (ODBC)", TDC-tunable, "may work only for creating extracts"), Excel, Qlik, legacy ETL | **ODBC** | https://learn.microsoft.com/en-us/power-query/odbc, https://help.tableau.com/current/pro/desktop/en-us/odbc_tableau.htm |
| Node ORMs (Prisma, Drizzle, Knex) | bespoke `pg` / `mysql2` / `better-sqlite3` APIs | Turso ships a napi-rs `better-sqlite3` drop-in; ADBC has a JavaScript driver manager |
| Grafana | Go backend plugins over RPC | https://grafana.com/developers/plugin-tools/key-concepts/backend-plugins/ |

The documented JDBC bottleneck in the engines is the specific pain a Rust core could relieve:
Abadi/Song for Starburst (2026-01-27, TPC-H SF100, CloudLab) read lineitem 52.71 GB from HDFS in
5.11 s with 2,537 parallel connections vs part 8.72 GB from Oracle over one JDBC connection in
273 s — a "320x" extraction-rate gap; Starburst's parallel JDBC gave >7x on Q14; gains flatten
after ~20 partitions **[third party, commissioned by Starburst]**
(https://www.starburst.io/blog/benchmarking-the-jdbc-bottleneck-in-trino/); a Mercedes-Benz
practitioner: unpartitioned Spark read >1 h vs 32 partitions <10 min **[third party]**. Sail (a
Spark-compatible engine) exposes a `jdbc` format "based on the Python connectorx library. No
actual JDBC driver or JVM is involved" (https://docs.lakesail.com/sail/main/guide/sources/jdbc/).
But Spark/Trino parallelism is decided by the engine, not the driver; a driver can only make each
connection's extraction columnar and cheaper, and only if it is reachable as a JDBC driver.

### ADBC export

One `cdylib` exporting `AdbcDriverInit` plus a TOML manifest is loadable by every ADBC driver
manager (C/C++, Python `adbc_driver_manager`, R, Go, Java via JNI, C#, Ruby, JavaScript, Rust).
Distribution channels: Columnar's `dbc` installer and the **ADBC Driver Foundry** (announced
2025-10-29 by Columnar with Databricks, dbt Labs, Improving, Microsoft, Snowflake and the Arrow
community; GitHub org `adbc-drivers`; "precompiled, signed driver binaries"; `dbc install
<name>`) **[vendor, checked 2026-11-09]** (https://columnar.tech/blog/announcing-columnar/,
https://columnar.tech/blog/introducing-dbc/). **ClickHouse shipped exactly this shape**: ADBC
driver 0.1.0 (2026-07-10), "written in Rust", ClickHouse HTTP with TLS, Foundry-distributed,
loaded via `clickhouse://`, targeting Ruby, R, C, Polars, pandas, dlt and dbt Fusion (which
bundles it); no benchmarks given (https://clickhouse.com/blog/introducing-the-clickhouse-adbc-driver).
Validation suites exist at three levels: the Apache C++ `c/validation` and Go
`validation.go` (DriverQuirks) conformance suites "checking the C contract", the Java
`adbc-driver-validation` module, and the Foundry's pytest `adbc-drivers/validation` suite (SQL
behaviour, type round-trips, `GetObjects` metadata depth, bulk ingest modes); chDB runs both
(https://github.com/adbc-drivers/validation, https://clickhouse.com/docs/chdb/install/adbc).
Measured gains vs row drivers: Java ADBC vs JDBC 13x / 5x less heap (section 3); Snowflake ADBC vs
JDBC "~20–30% faster" at 1 K–100 K rows, "roughly tied" at 1 M mixed rows (user report,
https://mail-archive.com/github@arrow.apache.org/msg403775.html); write path ADBC vs SQLAlchemy
2.3x at 10 M rows into PostgreSQL (https://github.com/AliMassoud/ADBC-Driver-with-PostgreSQL) and
dlt's DuckDB -> MySQL 5 M rows 344 s vs 92 s (3.7x) (https://dlthub.com/blog/arrow-adbc-vs-sqlalchemy)
**[third party / vendor]**. One R user reported Snowflake ADBC taking ~57 min vs ODBC ~50 s on
their largest dataset, cause unknown (https://mail-archive.com/issues@arrow.apache.org/msg81060.html).

The C ABI design lesson (consumers note): opaque `private_data` handles; a versioned init
function filling a `repr(C)` table so new functions append and never reorder; an error struct
with embedded `release` and a sentinel for extension (ADBC 1.1 extended `AdbcError` without
breaking ABI via `vendor_code == INT32_MIN`); all result data via `ArrowArrayStream`; typed option
get/set. If this *is* `adbc.h`, the ADBC manager loads it directly and every other bridge is a
facade over the same table. libpq's history is the cautionary tale: shlib version "5.1 as
compared to 5.0 in PG 8.2", "no document that describes libpq ABI versioning and compatibility
rules", and a 2007 change that "breaks ABI compatibility without announcing it in the SONAME"
(https://postgrespro.com/list/thread-id/1699444) — publish a written compatibility policy.

### ODBC export

An ODBC driver is a shared library exporting the ODBC 3.x `SQL*` entry points; "A Unicode driver
must export SQLConnectW to be recognized as a Unicode driver by the Driver Manager" and "must
accept Unicode functions (with a suffix of W)"; the DM strips the `A` suffix from ANSI calls; an
optional setup DLL exports `ConfigDSN`/`ConfigDSNW` (`ODBC_ADD_DSN / CONFIG_DSN / REMOVE_DSN`);
registration is an `odbcinst.ini` section (unixODBC/iODBC) or the registry **[vendor, checked
2026-11-09]** (https://learn.microsoft.com/en-us/sql/odbc/reference/develop-app/unicode-drivers,
https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/configdsn-function). **No shipping
Rust-implemented ODBC driver was found**: odbc-sys, odbc-api and rs-odbc are all client-side
(they link the driver manager); no crate exporting `SQLAllocHandle`/`SQLDriverConnect`/
`SQLGetDiagRecW` with `#[no_mangle]` turned up in three searches. C++ precedents: ClickHouse ODBC
(Apache-2.0; "The driver and the application must use the same driver manager") and Dremio's
Flight SQL ODBC driver (LGPL-2, kept out of the Arrow project "due to dependency licensing
issues"). Work estimate from the note: the largest of the exports — 70+ Core entry points,
descriptors, every C-type conversion, `SQLGetInfo` with ~170 info types, catalog functions,
diagnostics, both ANSI and W variants, a setup DLL, and BI-tool quirks (Power BI 64-bit; Tableau
TDC files with `SQLGetInfo` overrides). Best built on top of the ADBC/Arrow core (`SQLFetch` pulls
from an `ArrowArrayStream` into bound buffers), the reverse of what adbcBridge does. Microsoft's
ODBC Test (`odbct32w.exe`, open source, with a "Quick Test" conformance DLL) is the available
test tool; unixODBC ships only `isql`/`iusql`.

### A JDBC facade via FFM

Precedents for native-backed JDBC drivers are well established: sqlite-jdbc bundles native builds
for Windows, macOS, Linux (glibc and musl), FreeBSD and Android in one JAR, extracted at load, and
supports GraalVM native-image "out of the box starting from version 3.40.1.0"; DuckDB Java 2.0.0
"implements the main parts of the standard JDBC API, version 4.1", adds Arrow interchange
(`DuckDBResultSet.arrowExportStream(allocator, 256)`, `registerArrowStream`) and GraalVM support;
Oracle's OCI driver is a Type 2 driver over a C library; Snowflake JDBC 3.11.0+ fetches results in
Arrow ("up to 5x" over its previous path); the Arrow Flight SQL JDBC driver (Arrow 10.0.0, Dremio
grant) "adapts the Arrow result set to the JDBC interface" and Dremio has made its legacy JDBC
driver end-of-life in its favour **[vendor, checked 2026-11-09]**
(https://github.com/xerial/sqlite-jdbc, https://duckdb.org/docs/2.0/clients/java/overview.html,
https://duckdb.org/docs/2.0/clients/java/result_handling.html,
https://arrow.apache.org/blog/2022/11/01/arrow-flight-sql-jdbc/).

The boundary cost: JEP 454 (FFM API, final in Java 22) targets "overhead comparable to, if not
better than, JNI"; restricted methods warn by default and "will be blocked in a future release
unless native access is enabled"; Arenas (confined/shared/automatic) release native memory
deterministically (https://openjdk.org/jeps/454). Measured: Lemire (2026-01-17, MacBook M4) Java
-> C downcall 22.7 ns, with `Linker.Option.critical` 19.5 ns, pure Java 2.5 ns
(https://lemire.me/blog/2026/01/17/a-new-way-to-call-c-from-java-how-fast-is-it/); SoftwareMill
JMH: JNI 34.7 ns/op, Panama `invokeExact` 29.5, Panama with type conversion 34.3 (~15% gain,
erased by argument conversion) (https://softwaremill.com/welcome-to-panama/); zakgof's benchmark
via Java Code Geeks: FFM "outperforms raw JNI by ~12%"; not caching the downcall handle adds
"hundreds of nanoseconds" **[third party, checked 2026-11-09]**. The Arrow Java C Data Interface
(`arrow-c-data`, `org.apache.arrow.c.Data` with `exportArrayStream/importArrayStream`) is still
JNI-based and needs CMake 3.11+ and a C++17 compiler; no FFM port was announced
(https://arrow.apache.org/docs/java/cdata.html). ADBC 21 (2025-11-07) ships "JNI bindings to the
C++ driver manager" for Java with prebuilt binaries for amd64/arm64 Linux, arm64 macOS, amd64
Windows, "does not yet bind every ADBC function" (https://arrow.apache.org/blog/2025/11/07/adbc-21-release/).

What the evidence supports: the ~20–35 ns per-call boundary is irrelevant if the design moves
batches, not cells; a JDBC facade's cost will be dominated by `ResultSet.getXxx()` accessors
reading off-heap `MemorySegment`s. The Columnar 13x / 5x-less-heap measurement is the closest
evidence that a native columnar result path beats a conventional `ResultSet` on the same engine.
"Faster than pgJDBC" is plausible for bulk reads (pgJDBC materialises `byte[][]` and boxes on
access) but **unproven**: no published head-to-head of any native/Arrow JDBC driver against
pgJDBC on PostgreSQL exists, and on small OLTP round trips parity or loss is likely because the
FFM transition and Arrow batch framing add fixed cost. Java 22+ for FFM excludes Java 8/11/17 LTS
users unless a JNI fallback is kept; tie an Arena to `ResultSet.close()`.

### Python

PyO3 + maturin with abi3 wheels is the standard route (Turso, ConnectorX, Polars). The Arrow
PyCapsule Interface (`__arrow_c_schema__`, `__arrow_c_array__`, `__arrow_c_stream__`; capsule
names `"arrow_schema"`, `"arrow_array"`, `"arrow_array_stream"`) gives zero-copy hand-off to
pandas, Polars, pyarrow and DuckDB without a pyarrow dependency; `pyo3-arrow` 0.19.0 provides
the conversions **[vendor, checked 2026-11-09]**
(https://arrow.apache.org/docs/format/CDataInterface/PyCapsuleInterface.html, https://docs.rs/pyo3-arrow).
Free-threading: "Since PyO3 0.28, PyO3 defaults to assuming Python modules created with it are
thread-safe"; "The free-threaded build uses a completely new ABI and there is not yet an
equivalent to the limited API", so abi3 is ignored for 3.13t/3.14t until CPython 3.15's abi3t;
PyO3 0.23.0–0.23.2 abi3 wheels built for multiple versions were "highly unstable" (fixed 0.23.3)
(https://pyo3.rs/latest/free-threading, https://docs.python.org/ru/3.15/howto/abi3t-migration.html).
Per-call overhead: a maintainer measured a no-op Python function at ~18 ns vs a no-op PyO3
function at ~25 ns (older API, https://github.com/PyO3/pyo3/issues/3787); no current benchmark.
ConnectorX's own numbers (section 6.2) are the Python-side evidence for batch APIs.

Two surfaces are needed: (a) Arrow-native (`fetch_arrow()` / `__arrow_c_stream__`) for pandas
2.2+, Polars, DuckDB, pyarrow; (b) a PEP 249 DB-API class with a SQLAlchemy dialect, because dbt
adapters, Superset, Airflow and most ORMs speak only DB-API — this is what ConnectorX lacks and why
it is used only as a bulk reader. Wheel matrix: abi3 covers GIL builds; budget one extra wheel per
platform for each free-threaded version until 3.15. A core that releases the GIL during I/O
directly benefits free-threaded users over psycopg2/pyodbc-style extensions.

### Node

napi-rs v3 (2025-07-07) is the mature path (compiles to `wasm32-wasip1-threads` too; Turso dropped
its separate WASM target because "napi v3 can compile to WASM") **[vendor, checked 2026-11-09]**
(https://napi.rs/blog/announce-v3). Per-call cost: ~18 ns in an old napi-rs repo benchmark (noop
54.6 M ops/s vs 713 M plain JS); a paywalled 2025 post reports "roughly 100 ns of overhead per
call" (Node 22, 2 vCPU) **[third party, not independently verified]**. **Prisma's retreat is the
cautionary tale**: it moved its query engine from Rust to TypeScript (2025-01-30) because "data
must be serialized from JavaScript to Rust and then back to JavaScript, reducing efficiency",
every OS and OpenSSL version "needs its own binary", and edge runtimes "aren't always compatible
with large Rust binaries"; bundle ~14 MB -> ~1.6 MB; the TypeScript compiler beat the Rust engine
on `findMany` 25,000 records 55.0 ms vs 185.0 ms (3.4x) because "the cost of serializing data
between Rust and TypeScript is very high" **[vendor]** (https://www.prisma.io/blog/from-rust-to-typescript-a-new-chapter-for-prisma-orm,
https://www.prisma.io/blog/rust-to-typescript-update-boosting-prisma-orm-performance). The lesson
is about per-row object serialisation, not the call: never return `Vec<Row>` of JS objects from
Rust; return Arrow buffers (one copy or zero-copy via external `ArrayBuffer`s) and decode lazily
in JS. The Node consumer base for a generic SQL driver is small; highest leverage is a
`pg`/`better-sqlite3`-compatible facade or an ADBC JS driver-manager-loadable library. No
published Arrow-over-N-API database throughput exists.

### Go, .NET, R, Ruby

- **Go** actively resists native cores: a trivial cgo call costs ~40 ns on Go 1.21 (~60 ns and
  ">80%" scheduler work in Go 1.15, https://go.dev/issue/42469); projects build with
  `CGO_ENABLED=0` (Cloud Foundry Stratos switched from `mattn/go-sqlite3` to `ncruces/go-sqlite3`,
  SQLite compiled to wasm on wazero, where "memory usage will be higher ... because each
  connection runs in a Wasm sandbox"; `modernc.org/sqlite` is a machine-translated port) **[third
  party, checked 2026-11-09]**. The only realistic Go story is the ADBC Go driver manager plus its
  `database/sql` adapter (ADBC 25 split Go core and drivers into modules); a pure-Go port is out of
  scope.
- **.NET**: P/Invoke costs "between 10 and 30 x86 instructions per call" with "no marshaling cost
  between blittable types" (older Microsoft guidance; `SetLastError=true` 4x slowdown in a 2011
  test; `Marshal.PtrToStructure` 575.86 ns vs ~64 ns until a stub-caching fix) **[vendor / third
  party, dated]**; DuckDB.NET-style hand-written P/Invoke or the ADBC C# driver manager (C#
  0.21.0 in ADBC 21). The latter is zero incremental work once an ADBC driver exists. No modern
  .NET 8/9 P/Invoke benchmark was found.
- **R and Ruby** come for free through ADBC (DBI via adbi/adbcdrivermanager; ActiveRecord via
  `activerecord-adbc-adapter`, `red-adbc`); extendr/rb-sys bespoke bindings are not worth building.

### Reach per unit of work (the consumers note's ranking)

1. **ADBC cdylib + manifest**: smallest work, largest Arrow-native reach (pandas, Polars, DuckDB,
   DataFusion, dbt Fusion, dlt, R, Ruby, Go, C#, Java via JNI), plus Foundry distribution and
   validation suites.
2. **Python DB-API 2.0 + SQLAlchemy dialect** over the same core: moderate work; unlocks dbt
   classic, Superset, Airflow, SQLAlchemy ORMs.
3. **JDBC driver (FFM, Java 22+, JNI fallback)**: large work (JDBC 4.x surface, type mapping,
   metadata); the only way into Spark, Trino/Presto, Kafka Connect, Airbyte, Metabase and
   enterprise Java, and the only place the "faster than pgJDBC" claim can be made.
4. **ODBC driver**: largest work, zero reusable Rust scaffolding; BI desktops and legacy ETL;
   after JDBC, over the ADBC core.
5. **Flight SQL server**: a separate deployable (a proxy in front of the databases) that gains the
   Flight SQL JDBC/ODBC drivers and Dremio-style BI connectivity, not a client-library export;
   Apache ships a PostgreSQL Flight SQL extension and community servers exist
   (https://arrow.apache.org/flight-sql-postgresql/devel/_sources/overview.md.txt).

### Licensing of the pieces

- Reuse under MIT OR Apache-2.0: `postgres-protocol` and `postgres-types` ("building block for
  higher-level libraries such as rust-postgres"), `mysql_common` (shared by `mysql`, `mysql_async`
  and connectorx directly), tiberius (monolithic; forkable), rusqlite/libsqlite3-sys (MIT; bundled
  SQLite public domain; SQLCipher BSD-style), duckdb-rs (MIT), odbc-api/arrow-odbc (MIT),
  rust-oracle (UPL-1.0 or Apache-2.0), adbc_core/adbc_ffi (Apache-2.0), bb8 (MIT), deadpool (MIT OR
  Apache-2.0), r2d2 and mobc (MIT/Apache-2.0), connectorx (MIT), datafusion-table-providers
  (Apache-2.0). sqlx's protocol code is reusable by licence but coupled to `Executor`/`Connection`
  and the in-task I/O model its maintainers plan to replace **[vendor, checked 2026-11-09]**
  (section 2 URLs).
- Avoid as dependencies: wtx (MPL-2.0 on a permissive stack), snowflake-api (undocumented
  upstream API), the rdbc family (dormant; rdbc-rs "non-standard" licence with no repository;
  rdbc2 GitHub licence null).
- C libraries carry copyleft: `libmysqlclient` is GPL with Oracle's Universal FOSS Exception 1.0
  (packagers note derivative works must carry an approved licence,
  https://docs.oracle.com/cd/E17952_01/mysqld-8.4-license-gpl-en/licensing-information.html);
  FreeTDS is LGPL (static or dynamic linking binds the program; recipients must be able to
  relink, https://lists.ibiblio.org/sympa/arc/freetds/2015-08/msg00001.html); unixODBC is LGPL-2.1
  and iODBC BSD/LGPL (texts not fetched); Oracle Instant Client "can be bundled and redistributed
  for free" under OTN terms that must be passed to end users and do "not flow through"
  (https://www.oracle.com/database/technologies/faq-instant-client.html; 2010 wording). Pure-Rust
  protocol implementations avoid libmysqlclient and FreeTDS entirely; Oracle is the one backend
  that forces an optional, separately downloaded client (or an undocumented-protocol thin
  implementation of unknown legal standing).
- Dremio's Flight SQL ODBC driver being LGPL-2 and outside the Arrow project "due to dependency
  licensing issues" is the precedent that ODBC drivers drag in copyleft.
- Trademarks: Oracle's guidelines say "do not use Oracle trademarks or potentially confusing
  variations as all or part of your company, product or service names" ("Oracle, Java, MySQL, and
  NetSuite are registered trademarks"); JDBC was not found on the retrieved pages (the full list
  returned 403); Microsoft's list was not fetched, so whether "ODBC" is claimed is unverified. Use
  "JDBC driver" / "ODBC driver" descriptively, never combined with those marks in a product name;
  register the ADBC manifest `name`/`publisher` under the project's own name.

## 11. Testing and conformance

### TCKs and validation suites

| Suite | What it covers | Availability | Source |
|---|---|---|---|
| JDBC TCK (part of the Java SE TCK / JCK) | the JDBC compliance tests referenced by `jdbcCompliant()` | Oracle-licensed; free under OCTLA only to OpenJDK-derived compatible implementations, via request form, screening committee, signed OCTLA and OCA; stand-alone TCK licences forbid "comparative compatibility" claims; **no public download found**; Java-only | https://openjdk.org/groups/conformance/JckAccess, https://www.jcp.org/aboutJava/communityprocess/licenses/jsr379/SATCK-JSR_379-SE-9.pdf **[vendor, checked 2026-11-09]** |
| R2DBC TCK `io.r2dbc:r2dbc-spi-test` 1.0.0.RELEASE | ~35 `TestKit` scenarios: connection validation, auto-commit semantics, commit/rollback, savepoints, batches, bind by index/name, null binding, prepared statements, Blob/Clob, column/row metadata, duplicate column names, generated values, result segments | Apache-2.0, Maven Central, runnable against arbitrary drivers; drivers supply connection factory, placeholder and identifier conventions, fixtures via Spring JDBC; TCK accepted query-syntax customisation (#138) | https://github.com/r2dbc/r2dbc-spi, https://central.sonatype.com/artifact/io.r2dbc/r2dbc-spi-test **[vendor, checked 2026-11-09]** |
| ADBC C++ `c/validation` and Go `validation.go` | "A driver-agnostic test suite intended to aid in driver development": database, connection, statement lifecycle; the C contract (zero-initialised structs, `release` semantics, 1.0.0/1.1.0 negotiation); Go uses a `DriverQuirks` interface | Apache-2.0; C++11, Googletest, vendored Nanoarrow | https://github.com/apache/arrow-adbc/tree/main/c/validation, https://pkg.go.dev/github.com/apache/arrow-adbc/go/adbc/validation **[vendor, checked 2026-11-09]** |
| ADBC Driver Foundry `adbc-drivers/validation` and Java `adbc-driver-validation` | "reusable, pluggable test suite ... written with Python and pytest": SQL behaviour, type round-trips, metadata, bulk ingest modes; Java per-DB suites (Derby, SQL Server, PostgreSQL) and a JNI suite against SQLite | open; chDB runs both the Apache conformance suite and the Foundry suite | https://github.com/adbc-drivers/validation, https://clickhouse.com/docs/chdb/install/adbc **[vendor, checked 2026-11-09]** |
| sqllogictest (via `sqllogictest-rs`) | SQL-semantics corpus: `statement ok`, `query <types> [rowsort]` with `----` results, error matching by regex / SQLSTATE `(42P01)` / message, `retry`, substitution, `let`; runner features colour diff, `--override` to regenerate expectations, JUnit reports, parallel isolated DBs; bin "currently supports only PostgreSQL and MySQL"; used by RisingWave, DataFusion, Databend, CnosDB | open (the sqlite.org corpus page returned a bot check twice; corpus size and licence unsourced) | https://github.com/risinglightdb/sqllogictest-rs **[vendor, checked 2026-11-09]** |
| Microsoft ODBC Test (`odbct32w.exe`) | "an ODBC-enabled application that you can use to test ODBC drivers and the ODBC Driver Manager"; includes a "Quick Test" DLL covering "basic areas of ODBC driver conformance" | open source on GitHub (MDAC 2.8 SDK era); Windows | https://learn.microsoft.com/en-us/sql/odbc/odbc-test **[vendor, checked 2026-11-09]** |
| unixODBC / iODBC | only `isql`/`iusql` and `iodbctest` connection tools; **no conformance suite found** | — | https://wikis.openlinksw.com/dataspace/owiki/wiki/UdaWikiWeb/TestingUnixODBCDSNs **[third party]** |
| testcontainers-modules 0.15.0 (2026-09-14) | feature-gated containers: `postgres`, `mysql`, `mariadb`, `mssql_server`, `oracle`, `cockroach_db`, `clickhouse`, `databend`, `cratedb`, `rqlite`, `surrealdb`, `arrow_flightsql`; `blocking` for sync tests | MIT; depends on `testcontainers ^0.27` | https://docs.rs/testcontainers-modules/latest/testcontainers_modules/ **[vendor, checked 2026-11-09]** |

The R2DBC TCK's test list is the best open specification of what a "complete" driver must do;
the JDBC TCK is not a practical dependency. No MySQL/MariaDB connector conformance suite beyond
the connectors' own unit tests was identified. Benchmark harnesses that exist cover different
layers and none isolates driver overhead (section 3): TechEmpower (HTTP + driver + DB; Fortunes
and single query at 16–512 connections, multi-query/updates at 512 with 1–20 queries, 10,000-row
World table, no bulk reads, cross-request pipelining banned), BenchBase (CMU, JDBC-based; TPC-C,
TPC-H, YCSB, Twitter, Wikipedia and 11 more mixes; "variable rate, variable mixture load";
histogram export), pgbench (`-R` Poisson scheduling, `-L` late-transaction limit, `-M prepared`,
custom scripts with `\startpipeline`), sysbench (single table, ascending PK, "minimal contention"),
HammerDB (TPROC-C/H, stored procedures by default), YCSB (percentiles cannot be averaged;
`hdrhistogram.fileoutput=true`; a `jdbc` binding). Driver-level microbenchmarks that exist:
MariaDB's JMH suites, HikariCP-benchmark, r2dbc-postgresql `StagedResultSizeBenchmarks`,
MagicStack pgbench, jackc/go_db_bench (with a "theoretical max" raw-wire mode), diesel_bench. No
harness drives both JDBC and Rust clients from one load generator with identical semantics; the
jdbc-baseline note's recommended protocol (servers, baselines, two JDBC configurations, warm-up,
workloads a–h at concurrency 1–512 and fixed arrival rates, metrics incl. CPU-seconds per million
rows and RSS, two API modes, publication of raw HDR logs) is reproduced in section 3's closing
paragraphs and section 6.8.

### CI matrices of existing crates

**[vendor, checked 2026-11-09]** from each repository's workflow file.

| Crate | Runners | Toolchain / MSRV | Servers | Feature / TLS matrix | Sanitisers, Miri, fuzzing |
|---|---|---|---|---|---|
| sqlx | `ubuntu-24.04` only | stable; beta clippy; nightly `-Z minimal-versions` | Postgres 13 and 17, MySQL 8, MariaDB `verylatest, 11_8, 11_4, 10_11, 10_6`; SQLite bundled and `sqlite-unbundled`; separate `*_client_ssl` compose services (Postgres 13/17, MySQL 8, each MariaDB) for client-certificate auth | TLS `native-tls`, `rustls-aws-lc-rs`, `rustls-ring`, `none`; runtimes `tokio`, `smol`, `async-global-executor` (+`async-std` in check/SSL jobs); offline-mode macro tests | none ("No docs build, Miri, fuzzing, or proptest jobs") (https://raw.githubusercontent.com/launchbadge/sqlx/main/.github/workflows/sqlx.yml) |
| rust-postgres | `ubuntu-latest` only | test job pins Rust 1.85.0; `RUSTFLAGS=-Dwarnings` | `docker.io/postgres:18` via compose with an init script; no SSL/SCRAM settings in the compose file | `--no-default-features`, `--all-features`, `wasm32-unknown-unknown` check with `--features js` | none (https://raw.githubusercontent.com/sfackler/rust-postgres/master/.github/workflows/ci.yml) |
| tiberius | Linux; `macos-15-intel` (SQL Server 2022 in colima); `windows-latest` (SQL Server 2019 via Chocolatey, `IntegratedSecurity=true`) | MSRV job Rust 1.88; semver-checks advisory | SQL Server 2017/2019/2022/Azure SQL Edge | feature sets `all`, no-default, `chrono`, `time`, `rustls`, `vendored-openssl`, rustls+`tds73`+decimals; `rustls-webpki-roots` unit-only; Windows `all`, `rustls,winauth`, `vendored-openssl,winauth`; native-tls omitted on macOS (Secure Transport cannot complete the handshake) | none (https://raw.githubusercontent.com/prisma/tiberius/main/.github/workflows/test.yml) |
| diesel | `ubuntu-latest`, `ubuntu-24.04-arm`, `macos-26`, `windows-2025` | MSRV 1.88.0 via cargo-minimal-versions; cargo-nextest; Codecov | Postgres (apt / Homebrew `postgresql@14` / Windows `postgresql-x64-17`), MySQL (apt / `mysql@8.4` / installed), MariaDB (Docker `mariadb:13.0` / `mariadb@12.3` / Chocolatey 12.3.2), SQLite | bundled and system C libraries; `riscv32imac-unknown-none-elf` no-std SQLite; wasm32 | **ASan** on sqlite-, postgres- (Valgrind-enabled Linux) and mysql-bundled builds; **nightly Miri** on default target and big-endian `s390x` plus native SQLite; `diesel_compile_tests` (trybuild); `cargo xtask semver-checks` (https://raw.githubusercontent.com/diesel-rs/diesel/master/.github/workflows/ci.yml) |

Only diesel runs the full OS x arch matrix; sqlx and rust-postgres are Linux-only; no comparable
Rust driver runs musl or ARM64 integration tests except diesel (ARM64). MSRVs in late 2026
cluster at 1.85–1.88 (tokio 1.85, rust-postgres 1.85.0, tiberius 1.88, diesel 1.88,
rustls-platform-verifier 1.85); 1.85 (edition 2024) is the ecosystem floor. Tiberius's
`docker-compose.yml` with exact `mcr.microsoft.com/mssql/server` tags was not fetched.

A credible matrix for karez, from the security note: Linux x86-64 + ARM64 (`ubuntu-24.04-arm`
exists), musl static, Windows (SChannel/SSPI with local SQL Server and PostgreSQL services),
macOS; server versions at both ends of each vendor's support window (PG 13 and 18, MySQL
8.0/8.4/9.x, MariaDB 10.6 LTS and 11.8/13, SQL Server 2017/2019/2022/2025, SQLite bundled and
system); TLS backends x auth modes (SCRAM, SCRAM-PLUS, MD5 legacy, caching_sha2 fast/full/RSA,
client certificate, SSPI); a tokio-style ">= 6 months, bump only in minor releases" MSRV rule.

### Differential and property testing

- Public evidence shows property-based round-trip tests on wire codecs (proptest, section 8) as
  the common Rust practice, ASan/Miri where C libraries are bundled (diesel), and SSL
  client-auth containers (sqlx) for auth-path coverage. **Nothing found publicly implements
  replay-based differential testing against libpq, libmysqlclient or FreeTDS**; that would be
  novel for a Rust driver.
- Round-trip laws to encode as properties (security note): `decode(encode(v)) == v` for every
  SQL type x NULL x extreme values (i16/i32/i64 bounds, NaN/±Inf, max-precision NUMERIC, dates at
  4713 BC / 294276 AD, `infinity`, 1 µs timestamps, half-hour time-zone offsets);
  `encode(v).len() == size_hint(v)` (the sqlx advisory flags `size_hint` lies); "any byte string
  either decodes or returns `Err` without panic".
- Differential testing design: capture libpq/libmysqlclient/FreeTDS traffic with a TCP proxy for
  a fixed script, replay server bytes to the Rust decoder and compare decoded rows to the
  reference client's output; replay Rust client bytes to a real server; record/replay also gives
  deterministic offline CI for auth flows (SCRAM nonces must be injectable). RFC 5929's
  hash-selection rule (SHA-256 for MD5/SHA-1-signed certificates, else the certificate's own
  hash) is a concrete channel-binding conformance case.
- Big-endian (s390x via Miri or QEMU) and 32-bit targets catch endianness and `usize` truncation
  bugs in length handling — the 4 GiB class.
- HTTP Garden-style multi-implementation differential fuzzing (the sibling HTTP note's
  recommendation) has no database-protocol equivalent in the notes.
- No published type-mapping-matrix test artefact from sqlx/tokio-postgres/tiberius was located;
  their tests exist in-repo but were not enumerated.

## 12. What this means for karez

Each conclusion names the evidence it rests on. These are what the evidence supports and does not
support; the plan decides what to do with them.

1. **The performance gap to close is architectural, and it is already closed by existing Rust
   clients.** On identical TFB hardware the same frameworks differ 15–40x by database client
   alone (axum-pg 542,732 vs axum-sqlx 39,467 single-query req/s on Citrine; 1,114,265 vs 38,712
   Fortunes req/s on Round 23), and every fast entry uses a dedicated connection task with
   pipelining (section 2). The best Java client without JDBC (vertx-pg-client) is within 8% of
   the best Rust entry and within 1% on 20-query; JDBC-based Java is 2–6x behind with 5–20x worse
   p99 (section 3). "Faster than native JDBC" on OLTP workloads is therefore a low bar that
   tokio-postgres already clears; the real bar is "faster than tokio-postgres with per-core
   pipelined connections, zero-postgres and wtx on diesel's suite, asyncpg and pgx on the
   MagicStack set, and vertx-pg-client". Nothing in the notes shows a design that beats those by
   a large margin on OLTP; the evidence supports parity-plus-ergonomics, not a step change.
2. **A large, defensible speed margin exists only on the bulk path, and only with a columnar
   API.** Binary COPY, Arrow and columnar transfer beat row-at-a-time APIs by 5–40x in every
   measurement found (PgBulkInsert 712 k vs 39 k rows/s; DuckDB ADBC 38x over ODBC; Columnar's
   13x and 5x less heap over JDBC on the same engine; ConnectorX 13x over pandas; Teradata's
   15–84x with parallel streams vs 10–15% *slower* single-stream) (sections 3, 6.2, 6.3). A
   "significantly faster than JDBC" claim must therefore name the API surface; the evidence
   supports it for bulk-to-Arrow and not for single-row round trips.
3. **Pipelining via a worker per connection with per-request channels is the one design decision
   the whole field agrees on**, and sqlx cannot adopt it without a breaking `Executor` change
   (PR #3891). PostgreSQL supports it server-side with no changes; MySQL has no protocol
   pipelining but clients can batch writes and MariaDB has COM_STMT_BULK_EXECUTE; TDS pipelining
   is undocumented in the notes (sections 2, 5, 6.6).
4. **The common type model must not be a lowest common denominator.** sqlx-Any's nine kinds and
   rdbc's typed getters failed the same way (section 2). The union of ODBC SQL type ids, JDBC
   `Types` incl. DECFLOAT/JSON, and Arrow logical types is the minimal parity set; decimals need a
   wire-faithful digits-plus-scale representation with a 128-bit fast path; SQLSTATE is
   `[u8; 5]`, not an enum (section 4).
5. **Zero-copy, in-place decode from a reference-counted receive buffer, with a chunk/columnar
   view as the primary representation and the row API as a view over it**, is what X100's
   49-vs-2.2 cycles, Raasveldt's 1 MB chunks, the per-value cost ladder and the allocation cost
   model all point at (sections 6.1, 6.4, 6.7). Three independent sources put the chunk at
   ~1 MB / 128–8K values / 0.25–1.0 x L3 per thread. The Arrow C Data Interface then gives a free
   export path and the same layout serves ADBC parity.
6. **The async tax is real and a blocking path must exist that does not cross a channel.**
   diesel's suite shows the fastest async PostgreSQL client ~37% slower than the fastest sync one
   at N=1 and every async SQLite wrapper 20–60x slower than rusqlite (section 2). R2DBC's
   maintainers say the same about reactive Java (section 3).
7. **Parity has to be declared per standard and per level.** A Rust library cannot be JDBC
   Compliant (no Java, licensed TCK, no comparative claims allowed); it can claim JDBC 4.3 + MR5
   surface parity (DECFLOAT 2015, JSON 2016, `{\\ … \\}` suppression), ODBC Core / Level 1 / Level
   2 feature parity (features 101–109, 201–211), ADBC 1.1.0 function-table parity (57 entry
   points) and R2DBC TCK parity (~35 open scenarios) (section 4). The ODBC Core list is the only
   public proxy for ISO 9075-3. Two primary texts (JDBC 4.3 Chapter 6, ISO 9075-3:2023) are
   missing and should be acquired before the checklist is frozen.
8. **Pure-Rust wire implementations are available and permissively licensed for PostgreSQL
   (`postgres-protocol`/`postgres-types`), MySQL (`mysql_common`) and TDS (tiberius, forkable);
   SQLite, DuckDB and ODBC must stay FFI wrappers; Oracle is either ODPI-C (proprietary client
   download) or an undocumented protocol of unknown legal standing** (sections 2, 5, 10). Avoid
   libmysqlclient (GPL + FOSS exception) and FreeTDS (LGPL) entirely. What must be written from
   scratch: the connection-worker/pipelining core, the common type model, SQLSTATE normalisation, a
   TDS protocol crate if tiberius's governance is unacceptable, and a ClickHouse native client if
   wanted.
9. **The binary-size claim is unmeasured on both sides.** The only anchors are sqlx-cli at 3.3 MiB
   installed (musl, three drivers + rustls), wtx's self-reported 652 K Postgres client, the 51 KB
   / 30 KB / 8 KB hello-world floor, and the trimming anecdotes (TLS stack ~4 MB, regex ~1 MB)
   (section 9). On the JDBC side pgJDBC is 1.2 MiB plus a JVM at 250–390 MB RSS, or a GraalVM
   native image at 70–150 MB. "Smaller than JDBC" is trivially true against a JVM and must be
   measured in-repo against native images and against sqlx/tokio-postgres with `cargo bloat`;
   no public table exists.
10. **Per-driver crates behind a thin trait crate, with static linking and Cargo features, are the
    only structure that avoids sqlx's lockfile bloat (cargo#10801) and keeps `cargo vet`
    tractable**; ~18–20 direct dependencies is the field's floor for a pure-Rust driver (section 9).
11. **Security defaults are set by the vendors' direction, not their defaults**: `verify-full`
    when TLS is requested (libpq `sslrootcert=system`, TDS 8.0 `strict`), SCRAM-SHA-256-PLUS,
    `caching_sha2_password` with RSA off unless the key is local, token providers for IAM, and
    the seven design rules derived from the four CVE classes (section 8). cameleer-tls is
    feature-complete and its AES is constant time (bitsliced; its own doc comment saying
    otherwise is stale), but it is unaudited, has unblinded RSA and no hardware AES path; it can
    be an opt-in std-only backend, not the default.
12. **Testing has to target leniency and arithmetic, not memory safety**: checked length
    arithmetic, encoder fuzzing (both 4 GiB bugs were on the encode side), proptest round-trip
    laws, Miri on big-endian, and a differential replay harness against libpq/libmysqlclient/
    FreeTDS that nobody has built (sections 8, 11). diesel is the only comparable crate running
    ASan and Miri in CI.
13. **ADBC export is the highest-leverage bridge; JDBC is the only route into the JVM data
    engines and the only place the "faster than pgJDBC" claim can be tested; ODBC is the largest
    and least-scaffolded.** One `adbc.h`-shaped C ABI can serve all of them; Prisma's retreat
    shows that per-row serialisation across any FFI boundary destroys the benefit, so every bridge
    must move Arrow batches (section 10). The Java-side claim is unproven: no head-to-head of any
    native JDBC driver against pgJDBC on PostgreSQL exists.
14. **The measurement method is a prerequisite, not an afterthought.** Every prior Rust-vs-JDBC
    comparison was criticised on statement caching, environment matching or pipelining rules;
    coordinated omission inverts tail results; 56% of JVM benchmark pairs never reach steady
    state; numbers from different machines cannot be ranked (sections 3, 6.8). The protocol in the
    jdbc-baseline note (open-loop fixed rates, HdrHistogram with intended-start, both JDBC
    configurations, JVM and native image, two API modes, CPU-seconds per million rows, raw HDR
    logs published) is what a defensible claim needs, and the MagicStack and diesel suites are the
    scales to place the result on.
15. **The old rdbc API should not be revived in shape.** Its five `Box<dyn>` sync traits, typed
    getters and connection-borrowing statements are exactly the problems its author listed in #53,
    and every successor that copied them is dead (section 1). R2DBC's segment stream and ADBC's
    three-object model are the standards' answers to the async and columnar questions rdbc never
    settled.

What the evidence does not support: a claim of a large OLTP speed margin over the best existing
clients in any language; a claim of smaller binary size until measured; JDBC compliance in the
formal sense; a pure-Rust Oracle driver without a legal review; a pure-Rust SQLite replacement
(Turso) as a production target in 2026; and any ranking that mixes Round 23, Citrine, diesel's
runners and the MagicStack Threadripper.

## 13. Gaps and unverified items

Everything the notes flagged as unreachable, paywalled, recalled or unverified, so the owner knows
what to check before quoting.

### rust-landscape
- Owner/maintainer lists per crate were not retrievable (lib.rs returned 403 for every crate;
  GitHub API unauthenticated); only org names and security contacts are recorded.
- `mycelial/snowflake-api` GitHub API returned 404; stars for `snowflake-rs` not captured.
- No `database-rs` crate with meaningful adoption surfaced; if the brief meant a specific project
  it needs a URL.
- sea-orm 2.0 `DatabaseConnection` enum variants, sqlx `AnyConnectionBackend` method list and
  per-driver error/SQLSTATE surfaces were not fetched (docs.rs paths 404'd or not attempted).
- sqlx `PgRow` buffer internals and whether it allocates per column were not confirmed from
  source; no issue documenting sqlx "allocation per row" was found.
- Decimal/JSON/array representation tables per driver were not compiled beyond feature-flag names.
- TFB Round 23 single-query, multiple-query and update tables for Rust were not captured (browser
  daemon failed on the second pass); only Fortunes; `hw=ph` hardware details not extracted from
  that page (the jdbc-baseline note's Round 23 hardware comes from the TechEmpower blog).
- Round 23 publication date is given as 2025-02-24, 2025-03-03 and 2025-03-17 by different sources.
- Diesel metrics: the full >10 MB CSV was not downloaded by the landscape note; the jdbc-baseline
  note did download and average seven runs; the two notes read different variants (`by_name` vs
  `by_id`). Runner hardware is undocumented.
- ConnectorX paper experiment tables (Modin, Dask, Turbodbc, Spark) could not be parsed from the
  PDF text dump.
- Two Reddit threads (weiznich's December 2024 comparison; the March 2025 "fast asynchronous
  client for MySQL-like") were 403-blocked.
- Transitive dependency counts and binary-size contributions were not obtainable (lib.rs 403; the
  brief forbade running cargo on downloaded code).
- Whether `postgres-protocol`'s API is stable enough to pin was not established beyond its 0.6.x
  line since 2019.
- Licence texts for unixODBC (LGPL-2.1), iODBC and Oracle Instant Client redistribution were not
  fetched.
- mysql_async and rusqlite weakness threads were not surveyed beyond their READMEs.
- Several items are `[via search]` only: sea-query Any panics, rust_decimal PR #647 and 1.36.0
  changelog, the depscope.dev tiberius score, the sqlx "(fair)/(unfair)" bench file, sqlx #1598
  and PR #1946 compile-cost reports, the Kobzol blog.

### standards
- JDBC 4.3 Chapter 6 (Compliance) and Chapter 13 (escape syntax) full text could not be opened
  (Oracle click-through; Scribd paywall; the 4.5 MR PDF reprints only Appendix B). The "must"
  list in section 4 is a recollection.
- `java.sql.Types` numeric values were recalled from the constant-values page, not read;
  `ParameterMetaData` method and constant lists are from JDK source knowledge; the
  `javax.sql.rowset` family list is the fetcher's general knowledge (page did not render).
- `@since` tags for individual `ResultSet`/`CallableStatement` methods were truncated.
- No public source confirms which JDK release ships the JDBC 4.5 changes.
- ISO/IEC 9075-3:2023 and 9075-2:2023 not opened (paid; iso.org and iTeh blocked the fetcher); no
  clause numbers cited; whether SQL:2023 kept DECFLOAT unconfirmed.
- Exact unixODBC/iODBC `dlsym` contract (ANSI vs `W` entry points, `SQLGetFunctions` fallback) not
  found in an opened document; the ODBC installer-DLL function list not opened.
- ODBC 4.0 draft status inferred from the empty Releases page and absent Driver Manager.
- Per-attribute "ODBC version introduced" and the full `SQL_DESC_*` applicability matrix were
  fetched but not tabulated.
- ADBC per-driver feature matrix (bulk ingest, partitions, transactions, cancellation, statistics)
  was removed/moved (`driver/status.html` empty); official ADBC 25 announcement not found;
  `adbc_core` trait method lists and `export_driver!` not visible on the crate front page (the
  consumers note did find `adbc_ffi::export_driver!`); the `AdbcDriverInitFunc` typedef line was
  cut off (semantics from its doc comment).
- R2DBC `Result.Segment` subtype names unverified.
- Flight SQL `SqlInfo` numeric values and `XdbcDataType` enum not in the fetched excerpts.
- `DbBatch`/`DbDataSource` introduction versions (.NET 6/7, from memory) not verified.
- Oracle thin and SQL Server JDBC URL grammars not opened; libpq environment-variable list
  (§32.15) and MySQL Windows named-pipe URI form not on the fetched pages.

### wire-protocols
- PostgreSQL 3.0 -> 3.2 complete diff list not found in the official docs (taken from the 54.1
  version table and 54.7); the "max message length 1 GB" server limit not stated on any fetched
  page (verify in `PqRecvMessage`/`MaxAllocSize`); jsonb binary format, json/money/timestamp
  infinity sentinels not in the fetched tokio-postgres file (verify against `jsonb_send`).
- No current (2024–2026) published rows/s for pgx or tokio-postgres; a vendor benchmark by a
  competing Rust driver (QAIL, 3,903 q/s wide rows) should not be relied on; the 2016 asyncpg
  post's per-driver charts are images; the magic.io domain is dead.
- MySQL: `caching_sha2_password` sub-codes (0x03 fast-auth OK, 0x04 full auth, 0x02 RSA request)
  and the RSA/XOR-nonce rule are on an unfetched sub-page; compressed-packet header details (7
  bytes, 50-byte MIN_COMPRESS_LENGTH, zstd 1–22) unfetched; ed25519 signature construction and
  PARSEC plugin details not on the fetched page (PARSEC is covered by the security note); X
  Protocol framing, auth and row encodings not verified; COM_STMT_FETCH / cursor semantics and
  `CLIENT_OPTIONAL_RESULTSET_METADATA` behaviour beyond the prepare response not fetched; no
  throughput numbers for mysql_async or current go-sql-driver; Boost.MySQL results only as an
  image.
- TDS: MS-TDS packet-header (2.2.3.1), NBCROW/ROW/COLMETADATA spec pages not fetched (layouts are
  from go-mssqldb and tiberius source); RPC request layout and well-known procedure ids
  (sp_executesql 10, sp_prepare 11, sp_execute 12, sp_prepexec 13, sp_unprepare 15, sp_cursoropen
  2) not verified; MARS (SMP framing), transaction-manager payloads, FEDAUTH/Entra exchange and
  session recovery not fetched; whether tiberius supports TDS 8.0 strict, MARS or FEDAUTH not
  confirmed from its crate page (its `tds80` default feature suggests TDS 8.0); FreeTDS protocol
  document failed to fetch; no throughput for tiberius, FreeTDS, mssql-jdbc or go-mssqldb.
- SQLite: no measurement of FFI overhead per `sqlite3_column_*` or serialized-mode mutex cost;
  Turso's COMPAT.md not fetched; the amalgamation's shipped default threading mode not confirmed
  separately.
- Oracle: no legal analysis or Oracle statement about third-party protocol implementations; Net/TTC
  message formats undocumented; python-oracledb and go-oracledb source not fetched.
- DuckDB: Arrow functions and `duckdb_string_t` inline threshold in the unread final third of the
  C API reference; whether `libduckdb` is distributed static or shared not stated.
- ClickHouse default TCP port and `min_compress_block_size` not confirmed.
- Snowflake chunk mechanics, Databricks' Thrift inline-Arrow path, BigQuery quotas not on fetched
  pages.
- Flight SQL: no Voltron Data publication with numbers; Dremio/Teradata/TU Delft figures are vendor
  or vendor-adjacent on varied hardware; the TU Delft abstract's 6,000/4,800 MB/s is not matched
  by a described setup; current status of the Flight SQL JDBC/ODBC drivers not re-verified.
- No primary-source flamegraph of a modern Rust/Go/C driver hot path.
- RFC 8446's record-layer section was not reached in the fetched text; the 22-byte per-record
  figure is derived from RFC 8449 plus known header and tag sizes.
- Default fetch sizes for MySQL Connector/J (streams all rows unless `useCursorFetch`), Oracle
  JDBC (`fetchSize=10` — the jdbc-baseline note did confirm prefetch 10 from Oracle's docs),
  mssql-jdbc (`responseBuffering=adaptive`), asyncpg cursor prefetch (50) and psycopg3
  `prepare_threshold` (5) not confirmed from fetched pages.

### papers-and-math
- Raasveldt & Mühleisen give no closed-form cost model; the formulas are derived; their 2017 VM
  and Snappy are dated; MonetDBLite paper unpublished; DuckDB's CIDR 2020 paper not opened.
- ConnectorX: no formula for speedup vs partitions; the technical report with Arrow/Polars
  destination numbers not opened.
- DataFusion SIGMOD 2024 paper not opened; Arrow C Stream Interface page not opened; no
  peer-reviewed independent Flight-vs-JDBC measurement.
- No direct PostgreSQL text-vs-binary wire-decoder measurement; the per-value ladder is assembled
  from component benchmarks on different machines; Ryu README units not stated; Dragonbox charts
  only; Schubfach paper not opened.
- Little's law primary not opened; the Oracle pool-sizing video not watched (numbers via
  HikariCP's summary); no measured study of pool fairness or thundering herd for DB pools; Che's
  2002 paper and the Zipf closed form not opened; the Mitzenmacher supermarket constant not
  legible (TPDS 2001 paper not opened).
- No peer-reviewed measurement of the 40 ms Nagle stall in database drivers; the fetch-size
  derivation is the note's own.
- No allocator paper gives ns-per-malloc on current hardware; the 20–50 ns figure is a
  placeholder; mimalloc TR not extracted; no published JDBC per-row allocation cost.
- Gil Tene's slides returned 403; the Kalibera & Jones repetition formula not legibly extracted;
  the van der Kouwe paper itself not opened.
- zstd levels 2–3 not tabulated; no like-for-like AES-GCM vs ChaCha20-Poly1305 table on one modern
  CPU; the ChaCha20 AVX-512 figure is a search summary of an unreachable page; compression ratios
  for database result-set chunks exist only in the 2017 tables.

### jdbc-baseline-and-benchmarks
- Official TFB Round 22 and Round 23 per-framework tables could not be retrieved (client-side
  rendering; tfb-status ECONNREFUSED on 2026-11-09); re-check
  https://www.techempower.com/benchmarks/#section=data-r23&test=fortunes and
  https://tfb-status.techempower.com/results/91a66052-9d86-446c-b31a-eadbd669ed08; the Round 23
  ORM-filtered figures (ASP.NET 609,966; Actix 320,144 from a separate 2025-03-15 run; Spring
  243,639) are a third-party blog's filtered view, not the raw leaderboard.
- The Java target of mysql-connector-j 26.10.0 and pgJDBC 42.7.14 could not be read from their
  POMs; no single page lists MariaDB Connector/J 4.0's `useServerPrepStmts` default with release
  numbers.
- Diego Dupin's R2DBC-vs-JDBC comparison and Connector/J pipelining-on/off tables not found;
  HikariCP's "leak detection costs ~5 µs" appears only in mirrored READMEs.
- No current heap-per-row or allocation profile of pgJDBC or Connector/J; no RSS for a bare
  "hello world + pgJDBC + HikariCP" JVM without a web framework; no GraalVM native-image numbers
  for pgJDBC specifically; the Gillius RSS units are unlabelled.
- No controlled libpq rows/s or MB/s for binary vs text bulk SELECT; absolute ODBC/JDBC MB/s in the
  Dremio and Voltron comparisons unpublished; the Voltron Data original post unreachable.
- No cargo-bloat or binary-size table for sqlx/tokio-postgres/diesel/rusqlite; no per-connection
  memory for tokio-postgres, sqlx or deadpool; no deadpool vs bb8 vs r2d2 vs sqlx-pool benchmark.
- The asyncpg 2023 gist records neither PostgreSQL nor driver versions; the Timescale rerun did
  not separate psycopg3's binary mode; Phil Eaton's and the forum benchmark's hardware unstated.
- No harness drives JDBC and Rust clients from one load generator; no peer-reviewed guidance
  specific to database-driver benchmarking beyond ConnectorX's finding.
- The 2026 "PostgreSQL REST API benchmark" was not verified in detail.

### security-size-testing
- MariaDB `ed25519` KB page did not render (introduction version believed 10.1.22/10.2.5 is
  unsourced); Oracle Thin-driver Kerberos docs not found; RFC 5802/7677 text not re-fetched; no
  formal "JDBC parity" auth checklist exists.
- No independent rustls-vs-OpenSSL benchmark (all headline numbers are ISRG's); no measured
  numbers for cameleer-tls at all; its default trust-root behaviour unconfirmed; MySQL client
  `--ssl-mode` default and `VERIFY_IDENTITY` semantics not on the fetched page; MariaDB TLS 1.3
  specifics not researched.
- Resolved: the cameleer-tls AES description differed between the 0.4.0 docs (table-based
  S-boxes) and the sibling HTTP-server note (bitsliced, constant time). The source settles it:
  bitsliced and constant time; the doc comment in `src/lib.rs` is stale (section 8).
- No MySQL Connector/J CVE in 2024–2025 with public root cause; CVE-2025-59250 root cause
  undisclosed; whether Postgres.js or DuckDB clients were affected by the 4 GiB class unconfirmed;
  no DNS-rebinding CVE on a database driver found; the BeyondTrust link to CVE-2025-1094 not
  confirmed from primary sources.
- No public libFuzzer harness in a mainstream Rust DB driver; no Miri run-time cost data; no
  `unsafe` counts (cargo-geiger) for sqlx/tokio-postgres/tiberius.
- No public cargo-bloat table, `cargo tree` counts, or compile times for the driver crates (lib.rs
  403); no monomorphisation-vs-trait-object or abi_stable-vs-stabby measurements; no tokio vs
  smol vs std binary-size delta.
- sqlite.org sqllogictest page returned a bot check (corpus size, engine list, licence unsourced);
  `adbc_validation.h` contents not read; no MySQL/MariaDB connector conformance suite identified;
  TPC-H/TPC-C usage in driver benchmarks not researched.
- No public record/replay or differential harness against libpq/libmysqlclient/FreeTDS; no
  type-mapping-matrix test artefact located.
- Tiberius's compose file with exact SQL Server image tags not fetched; no comparable crate runs
  musl or ARM64 integration tests except diesel (ARM64).

### consumers-and-bridges
- Three primary pages refused fetch: voltrondata.com (DNS failure), the duckdb.org Java
  installation page (404; bundled-platform list unverified), oracle.com/legal/trademarks (403;
  content from search snippets).
- No published head-to-head of any native/Arrow JDBC driver against pgJDBC on PostgreSQL; no
  quantified JNI pinning / GC cost for bulk transfer; whether Arrow Java will port `arrow-c-data`
  from JNI to FFM unknown.
- No current PyO3-vs-CPython per-call benchmark (2021 blog and an older issue only); no direct
  read-path benchmark of pandas `read_sql` via ADBC vs SQLAlchemy.
- No vendor-neutral napi-rs per-call benchmark (the only figure is paywalled); no Arrow-over-N-API
  database throughput.
- extendr (R) and rb-sys (Ruby) not researched beyond the ADBC adapters; no modern .NET 8/9
  P/Invoke benchmark.
- SQLite's explicit API-stability statements not retrieved (c3ref/intro lacks them).
- No Rust ODBC driver precedent (the "Rust ODBC driver for Exasol" could not be located; the
  Exasol Metabase driver found is Clojure); Microsoft's full ODBC conformance tables not fetched by
  this note (the standards note did fetch them); Qlik, Informatica, SSIS, Talend not individually
  verified.
- Exact `libadbc_driver_manager` shared-library version for ADBC 25 not confirmed from an Apache
  source; which ADBC release introduced manifests not stated.
- No benchmark of ConnectorX vs partitioned Spark JDBC; Sail's claim is architectural.
- Microsoft's trademark list not fetched (whether "ODBC" is claimed unverified); the Oracle OTN
  Instant Client "no charge to end users" clause is from a 2010 copy; no legal opinion on Rust
  `cdylib` + LGPL relinking obligations.

### Cross-cutting
- **Nothing is [measured here].** Every number above was produced on somebody else's machine with
  somebody else's harness. The first thing the plan should produce is a set of numbers tagged
  [measured here]: the diesel suite and the MagicStack query set on one documented machine, with
  the JDBC baselines in both shipped and tuned configurations, and `cargo bloat` for the
  candidate stack and its competitors.
