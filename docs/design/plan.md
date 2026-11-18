# Plan: karez, a ground-up Rust database connectivity library

_Internal design note, 2026-11-09, for the owner to decide on. Evidence and sources are in
[research.md](research.md); the seven research notes behind it are in
[`../research/`](../research/). Nothing here has been built._

The library is named **karez**: an underground channel that carries water from a deep aquifer to
where it is used, by gravity alone, with no pumps. It joins the Cameleer family next to simoom,
the HTTP engine. On 2026-11-09 `karez` and every planned `karez-*` sub-crate name were free on
crates.io, `karez` was free on PyPI and npm, and `pmsudhi/karez` was free on GitHub. The
`karez.dev`, `karez.io` and `karez.com` domains already resolve, so they are probably
registered. No trademark search has been done.

## 0. Summary

1. **tokio-rs/rdbc cannot be fixed in place.** It is a 1,100-line 2019 proof of concept over
   three third-party drivers, synchronous, three value types, one prepared statement per
   connection, dormant since May 2021 and called "comatose" by its own author. Every successor
   fork is also dormant. What stays valid is the idea: one driver-agnostic API for tools that
   must run arbitrary SQL against any database (research.md §1).
2. **The performance gap is architectural, not incremental.** On identical hardware, the
   TechEmpower Fortunes test puts pipelined Rust clients (tokio-postgres and its derivatives) at
   over 1.0M req/s and sqlx at 28k–39k: a 20–40x spread from the client library alone. Java that
   bypasses JDBC (vertx-pg-client) is within 8% of the best Rust; Java on JDBC is 2–6x behind
   (research.md §2, §3). The two levers are **pipelining on a connection worker** and
   **decoding binary protocols straight into columnar chunks**, which the 2017 VLDB protocol
   study showed removes the per-row and per-value costs that make every mainstream protocol
   10–60x slower than raw transfer (research.md §6).
   **What the evidence does and does not support (research.md §12):** on single-row
   round trips, the best existing clients (tokio-postgres, zero-postgres, wtx, asyncpg, pgx,
   vertx-pg-client) already beat JDBC by 2–6x, and nothing shows a design that beats *them* by a
   large margin. So the plan targets **parity with the fastest client on OLTP** and puts its
   large-margin claims (3x and more) on the **bulk and columnar path**, where every measurement
   found shows 5–40x over row-at-a-time APIs. Every claim names the API surface it applies to.
3. **Parity is definable.** JDBC 4.3/4.5 (including the 2025 maintenance release's DECFLOAT and
   JSON types), ODBC 3.8 Core/Level 1/Level 2, ADBC 1.1.0's function table and R2DBC 1.0's TCK
   converge on one core (connect, transactions, prepared execution, typed forward-only rows,
   metadata, SQLSTATE errors, cancel, batching) and diverge on index base, cursors, escapes and
   transport. The plan declares parity per standard and per level (research.md §4).
4. **Recommendation: build a new core as its own project, karez** (option D in §1): sans-io protocol
   crates for PostgreSQL, MySQL/MariaDB and TDS; a pipelined connection worker; an
   Arrow-layout columnar chunk as the one result representation with a zero-allocation row
   view over it; a standards-shaped public API in both blocking and async forms; and three
   exports, ADBC, ODBC and a JDBC facade, that make one Rust core a driver for Python, R, Go,
   Java, .NET, BI tools and the JVM query engines. Gate the build behind **Milestone 0**, a
   three-week measurement spike on PostgreSQL that must match the fastest Rust client on point
   queries, beat tuned pgJDBC by at least 2x there, and beat everything by at least 2x on a bulk
   scan into Arrow, on a stated method, before anything else is built (§1.3).
5. **The "faster than native JDBC" claim does not exist in the literature yet.** No published
   benchmark compares a native-backed driver with pgJDBC on PostgreSQL. It has to be produced,
   with the method in §6, and the JDBC facade (M6) is the only place it can be made inside Java.

## 1. Decision

### 1.1 The options

| option | what it means | verdict |
|---|---|---|
| A. Revive tokio-rs/rdbc as it stands | async-ify the traits, re-wrap sqlx or tokio-postgres, add types | caps at the wrapped driver; a wrapper over sqlx inherits its 9-type `Any` model and its inability to pipeline (an open issue since 2020; the maintainers say fixing it is a breaking change). **No.** |
| B. Contribute to sqlx instead | improve `Any`, add pipelining upstream | the pipelining PR (#3891) was closed unmerged in Sept 2026 as needing a worker-per-connection redesign; `Any` is 9 type kinds; 43 feature flags and a lockfile-bloat bug. Not our project to redesign. **No.** |
| C. Thin common API over the best existing drivers | tokio-postgres, mysql_async, tiberius, rusqlite behind one trait | fast for PostgreSQL only (only tokio-postgres pipelines); MySQL and TDS drivers are row-at-a-time, no columnar path, four type systems and four error models to reconcile, tiberius has no named maintainer, no SQLSTATE normalisation. Parity would be the weakest driver's. **Keep as the comparison baseline and as protocol references, not as the core.** Fallback if M0 fails. |
| **D. New core as its own project (karez)** | sans-io protocol crates, pipelined worker, columnar chunk, standards-shaped API, ADBC/ODBC/JDBC exports | **Recommended.** It is the only option that attacks both measured bottlenecks and gives a type model wide enough for JDBC 4.5 and ADBC parity. The protocol building blocks (postgres-protocol, mysql_common, tiberius) are MIT/Apache-2.0 and can be read, tested against, or vendored. |

**"Build versus adapt", answered:** adapt **designs** and **test corpora**, build **code**. The
designs come from tokio-postgres (split client and connection task), asyncpg and pgx (binary
extended protocol, zero-copy row buffers, statement cache), Raasveldt and Mühleisen (1 MB
column-major chunks, whole-chunk compression chosen by link speed), ConnectorX (preallocate,
partition, write into the final buffers), MariaDB Connector/C (bulk execute, metadata skipping),
adbc.h (versioned C function table) and the CVE record (the seven rules in §7).

### 1.2 Hard rules

1. **Server-side parameter binding only.** No code path ever inlines a literal into SQL text.
   This is the rule pgJDBC's CVSS 10.0 CVE-2024-1597 and pgx's CVE-2024-27289 broke.
2. **Binary protocols by default** (PostgreSQL extended protocol with binary result format,
   MySQL binary protocol, TDS native encodings). Text only where a type has no binary form.
3. **Verified TLS by default** (`verify-full` semantics with the OS trust store). Downgrade is
   an explicit option with a name that says so. Authentication is an allow-list, in the style
   of libpq's `require_auth`, so a server cannot downgrade a client to MD5 or cleartext.
4. **Sans-io protocol crates with `#![forbid(unsafe_code)]`**, no I/O, no runtime, no
   allocation beyond `alloc`. Everything that touches a socket, a C library or SIMD lives in two
   named crates with an `unsafe` budget (§7.2).
5. **No dependency in the core or the protocol crates.** TLS, async runtimes, Arrow
   integration and C libraries are optional features. The columnar chunk implements the Arrow
   memory layout and the Arrow C Data Interface itself; it does not depend on the `arrow` crate.
6. **Every claim is measured with the method in §6**, pipelined and unpipelined numbers are
   reported separately, and nothing is published that would not survive the maintainers of
   tokio-postgres or pgJDBC rerunning it.

### 1.3 What Milestone 0 must show before the build goes ahead

A PostgreSQL-only spike (sans-io extended-protocol decoder into chunks, the caller-driven
blocking connection and an async connection task, the row view, SCRAM auth, no TLS) is measured
against tokio-postgres (per-core pipelined connections, as in TechEmpower), zero-postgres, wtx,
sqlx, diesel, pgJDBC with HikariCP (default **and** tuned: `prepareThreshold=1`,
`binaryTransfer=true`, `defaultRowFetchSize` set), vertx-pg-client, asyncpg, pgx and libpq on
the same machines with the §6 method. The gate:

| scenario | must show |
|---|---|
| S1/S2 single connection, blocking (diesel's suite shape) | **within 5% of the fastest Rust client** (zero-postgres led diesel's 2026-11-06 run at 40.5 µs per trivial query) |
| S2, point select of one 10-column row, unpipelined, 64 connections | **within 5% of the fastest of tokio-postgres, zero-postgres and wtx**, and **at least 2x tuned pgJDBC** SLO throughput, at p99 no worse |
| S2, pipelined (reported separately) | within 5% of the best pipelined client (tokio-postgres per-core, vertx-pg-client) |
| S4, 1M-row scan into Arrow chunks | **at least 2x the best Rust row API** (tokio-postgres or zero-postgres collecting into Arrow), **at least 3x tuned pgJDBC** rows/s at **at most one third of its RSS**, and **within 15% of asyncpg's geometric-mean ceiling** on the MagicStack queries |
| S4, same scan through the row-view API | within 15% of the chunk path (proves the view costs nothing) |
| size | stripped blocking hello-world with the PostgreSQL driver and no TLS **at most 600 KB**; direct dependencies: zero |

The OLTP rows ask for parity, not a margin, because the evidence supports nothing more
(research.md §12, conclusion 1); the case for a new crate family rests on the S4 row plus
the standards parity and exports that no existing client offers. **Stop rule:** if S4 is under
2x the best Rust row API, or OLTP is more than 10% behind the fastest Rust client, the
architecture is not worth a new crate family. Do option C instead: a thin standards-shaped API
over tokio-postgres, contributing the chunk decoder to it. Everything else in this plan is
conditional on this gate.

**Where the gate stands (2026-11-18; campaign 2, round 3 and its tail run: three lanes, three rounds, karez n=11 on point queries and n=9 on bulk, competitors n=3, so still indicative; details and sources in
[perf-program.md](perf-program.md)):**

| row of the gate | measured | state |
|---|---|---|
| S1/S2 one connection, within 5% of the fastest Rust client | karez-blocking at 0.88 to 1.07 of a raw pre-encoded floor (three droplet pairs); level with tokio-postgres, pgx, deadpool in the open-loop runs | **met** |
| S2 at 64 connections, within 5% of the fastest; at least 2x tuned pgJDBC | within one search step of the best (the database host is 85% busy, server-bound); about 1.1x to 1.2x tuned pgJDBC (rounds 1 to 3: 0.89 and 0.81 of karez), **not 2x** | parity met; the 2x over pgJDBC **not met** (server-bound) |
| S4 chunk path at least 2x the best Rust row API | 1.0x on one connection; with `scan_partitioned` **1.33x (S4, 1M rows; 0.96 to 1.61, one loss) and 1.65x (S8, 13M rows; 1.42 to 1.90) the best other client in the same lane and round (n=9)** on DigitalOcean `c-4` (1.9 Gbit/s link, the limit; campaign 2, round 3 and its tail, three lanes, three rounds), and 1.9x to 2.0x tuned pgJDBC (n=3; the plan wants 3x) and 2.4x (S4) to 3.4x (S8) with no network limit (WSL, loopback) | **not met on the available network; met without a network limit.** A faster two-machine network is the deciding test and is parked by the owner |
| S4 row view within 15% of the chunk path | CPU equal (0.35 s against 0.36 s); wall difference was noise | met (CPU) |
| client CPU | about half of tokio-postgres on S8 (6 to 7.7 CPU-s against 12 to 14), 14% below on S1 | met, not a gate row |
| size, dependencies | zero third-party dependencies (the tree is `karez`, `karez-pg`, `karez-chunk`, `karez-proto-pg`); stripped blocking hello-world **441 KiB** (LTO, `opt-level="s"`, `panic="abort"`) or **483 KiB** (`opt-level=3`), against 282 and 296 KiB for an empty `println!` binary; tokio-postgres hello-world 824 and 940 KiB (68 crates in its tree), sqlx 1,060 KiB (147 crates, `opt-level="s"` only) | **met** (at most 600 KB); diesel 6,157 KiB (static libpq and OpenSSL), wtx 995 KiB and rusqlite 1,317 KiB (SQLite, not PostgreSQL) measured on 2026-11-18 with the same profile (perf-program.md section 18) |

The stop rule is therefore **neither triggered nor cleared**: the owner decided on 2026-11-16 to go ahead with the current numbers
and to park the faster-network test. Until that test is run, every speed claim in public text must stay at "level on OLTP, about
half the client CPU, partitioned scans up to the network limit".

**Decisions taken after this plan was written (owner):** ODBC is in 1.0 (2026-11-13; needs a separate FFI crate with `unsafe`, to be
covered by the audit, and adds months to M4/M7); TLS is an opt-in `rustls` feature; `karez-rt` is a std-only async layer; replay of
commands only when never written; slow streams lease their connection. See [../STATUS.md](../STATUS.md).

## 2. Architecture

### 2.1 Layers

```
exports (cdylib / jar / wheel)   karez-adbc        AdbcDriverInit + TOML manifest; results as ArrowArrayStream
                                 karez-odbc-driver SQL* entry points, W functions, setup DLL; descriptors
                                 karez-jdbc        Java 22+ FFM facade (JDBC 4.3 interfaces) over the C ABI; JNI fallback
                                 karez-py          PyO3 abi3: DB-API 2.0 + __arrow_c_stream__
                                 karez-node        napi-rs: Arrow buffers out, never row objects
C ABI                            karez-capi        repr(C) handles, versioned function table, Arrow C Data/Stream
public API, safe code            karez             Driver, Connection, Statement, Cursor, Chunk, Row view, Value,
                                                  SqlType, SqlState errors, Metadata, Url, Tx/Savepoint, Pool traits
pool                             karez-pool        fair FIFO pool, statement cache, multi-host routing, health
runtime                          karez-rt          caller-driven blocking connection (no thread hop); async connection
                                                  task: command queue, write coalescing, demux; buffer pool, timers;
                                                  async adapter: tokio (feature)
drivers                          karez-pg          PostgreSQL 3.0/3.2, COPY, LISTEN, cancel, pipelining
                                 karez-mysql       MySQL 8/9 and MariaDB 10.6+ (bulk execute, metadata skip)
                                 karez-tds         SQL Server 2017+ TDS 7.4 and 8.0 strict, bulk copy
                                 karez-sqlite      FFI over bundled amalgamation (unsafe budget)
                                 karez-odbc        bridge to unixODBC / Windows DM (unsafe budget)
protocol, sans-io, forbid unsafe karez-proto-pg, karez-proto-mysql, karez-proto-tds
codecs                           karez-codec       SIMD number/UTF-8/base64 scan with scalar fallback, Decimal128,
                                                  temporal types, UCS-2 <-> UTF-8, collations
columnar                         karez-chunk       Arrow-layout chunk: validity bitmap, offsets, data; builder;
                                                  Arrow C Data Interface export; no arrow crate
```

The protocol crates take bytes and return events and bytes, as rustls and quinn-proto do. They
are fuzzable, run under Miri, and work over any I/O. `karez-sys`-style unsafe is confined to
`karez-rt` (sockets, SIMD dispatch) and the two FFI drivers.

### 2.2 The result model: one chunk, two views

The single biggest design choice. Every driver decodes rows **directly from the receive buffer
into a columnar chunk** in Arrow memory layout: per column a validity bitmap, fixed-width data
or offsets plus bytes. No intermediate row object, no `Vec<Value>` per row, no `String` per
cell.

- **Chunk size:** 1,024 rows or 1 MiB, whichever first (the X100 vector range of 128–8K values
  and the Raasveldt optimum of about 1 MB with a flat region from 100 KB to 10 MB, research.md
  §6). One builder per connection is reused; a one-row result costs a reset, not an allocation.
- **Row view:** `Cursor::next()` advances an index into the current chunk; `row.get::<i32>(c)`
  is a bounds check, a validity-bit test and a load. Strings come back as `&str` borrowed from
  the chunk; owned copies are explicit. This is what makes the JDBC-shaped API free: the
  ResultSet-style accessors are views, and the S4 gate (§1.3) verifies it.
- **Columnar view:** `Cursor::next_chunk()` hands the chunk out whole. The ADBC export
  streams chunks as `ArrowArrayStream` with no copy; Python, Polars, DuckDB and pandas read
  them through the Arrow C Data Interface.
- **Types:** one `SqlType` enum covering JDBC Appendix B and ADBC's Arrow types: Bool,
  Int8/16/32/64, UInt8/16/32/64 (MySQL), Float32/64, Decimal128 and Decimal256 (own
  implementation; `rust_decimal` stops at a 96-bit mantissa and scale 28, research.md §2),
  DecFloat (JDBC 4.5), Date, Time(p), Timestamp(p, tz), Interval (month-day-nanosecond), Utf8,
  Binary, Uuid, Json/Jsonb, List, Struct, Vector (pgvector), Null. Each maps losslessly to an
  Arrow logical type so export never converts.
- **Streaming by default.** Results stream chunk by chunk; `fetch_all` is a convenience that
  concatenates. pgJDBC's `defaultRowFetchSize=0` (the whole result in heap) is the behaviour we
  refuse to have.

### 2.3 The connection worker and pipelining

- **Async:** each connection is owned by **one task** holding the socket, the receive buffer,
  the chunk builder and a FIFO of in-flight commands. Callers submit commands and receive
  completions; they never touch the socket. This is the tokio-postgres split that TechEmpower
  shows to be worth 20–40x over I/O in the calling task, and it also gives cancellation safety:
  a dropped future cannot desynchronise the stream (sqlx's documented problem).
- **Blocking:** the caller's thread drives the sans-io connection directly, with **no worker
  thread and no channel** on the path. diesel's suite shows the fastest async PostgreSQL client
  about 37% slower than the fastest sync one on single queries, and async SQLite wrappers 20–60x
  slower than rusqlite (research.md §2, conclusion 6), so the blocking path must not pay for a
  hand-off. Pipelining in the blocking API is explicit (the `Pipeline` builder); a pooled
  connection shared across threads uses the async-style task.
- **Write coalescing:** everything queued in one worker turn is serialised into one buffer and
  sent with one `write`: one TCP segment and one TLS record, so Nagle plus delayed ACK (the
  40 ms Linux stall) never triggers and the 22-byte TLS record cost is paid once per flush.
  `TCP_NODELAY` is set regardless.
- **PostgreSQL:** implicit pipelining of independent statements (Parse/Bind/Describe/Execute
  with one Sync per logical batch), as the server has supported since protocol 3.0; an
  explicit `Pipeline` builder for the blocking API; errors abort to the next Sync exactly as
  the protocol defines.
- **MySQL:** the server has no pipelining (the sequence id resets per command), so the worker
  executes serially but still coalesces writes, caches prepared statements and uses MariaDB's
  `COM_STMT_BULK_EXECUTE` and metadata skipping when the server advertises them. MariaDB's own
  benchmarks show 2–30x on batches from exactly these two features (vendor-run).
- **TDS:** no pipelining without MARS (deferred); RPC with `sp_prepexec` and server statement
  handles cached per connection, multi-statement batches, and `INSERT BULK` for bulk loads.
- **Read path:** one `read` into a large buffer per response where the buffer suffices (the
  steady state for all three protocols); packet and message headers are stripped in memory,
  never with a syscall per packet.

### 2.4 Pool

`karez-pool` is driver-agnostic and sized by the mathematics in research.md §6:

- Default maximum `2 × cores + effective_spindles` (the HikariCP/Oracle argument), with a
  documented warning when a user configures more than a few hundred connections to PostgreSQL
  (10,000 idle backends cut an active connection's throughput by more than half).
- A fair FIFO wait queue with a timeout; Little's law exposed as metrics (in-use = arrival rate
  × hold time) so operators can size from observation.
- A per-connection prepared-statement LRU whose size is reasoned from the Che approximation
  (hit rate as a function of statement popularity and cache size).
- Multi-host URLs with power-of-two-choices routing and read/write role tagging.
- Health checks by protocol-level ping, not `SELECT 1`.

### 2.5 Public API shape

The API follows the common core of the standards (research.md §4) in idiomatic Rust, 0-based
like R2DBC, ADBC and Go, with explicit names for the JDBC concepts people search for. A sketch,
not final:

```rust
use karez::{Driver, Connection, Statement, Cursor, Chunk, Value, SqlType, Url, Tx, Isolation};

let url = Url::parse("karez:pg://app@db.internal:5432/sales?sslmode=verify-full&target_session_attrs=read-write")?;
let driver = karez::drivers().get(&url)?;                 // registry; static per enabled feature
let mut conn = driver.connect(&url, &Credentials::from_env()?)?;

// Direct and prepared execution; `?` placeholders are rewritten per dialect by the driver.
let n = conn.execute("UPDATE orders SET state = ? WHERE id = ?", &[Value::text("paid"), Value::i64(42)])?;
let stmt = conn.prepare("SELECT id, total, placed_at FROM orders WHERE customer = ?")?;   // cached per connection

// Row view: JDBC-shaped, zero allocation.
let mut cur = stmt.query(&[Value::i64(7)])?;
while let Some(row) = cur.next()? {
    let id: i64 = row.get(0)?;
    let total: Decimal128 = row.get(1)?;
    let placed: Timestamp = row.get(2)?;
    let name: &str = row.get_str_by_name("customer_name").unwrap_or("");
}

// Columnar view: the same cursor, whole chunks, zero copy to Arrow.
let mut cur = stmt.query(&[Value::i64(7)])?;
while let Some(chunk) = cur.next_chunk()? {
    let totals: &[i128] = chunk.column(1).decimal128()?.values();
    let stream = chunk.to_arrow_c_stream();                    // Arrow C Data Interface, no arrow crate
}

// Pipelining (blocking API; implicit in the async API when futures are polled together).
let results = conn.pipeline()
    .query(&stmt, &[Value::i64(1)])
    .query(&stmt, &[Value::i64(2)])
    .execute("REFRESH MATERIALIZED VIEW daily", &[])
    .run()?;

// Transactions, savepoints, isolation, batch, bulk.
let mut tx = conn.begin(Isolation::RepeatableRead)?;
tx.batch(&stmt_insert).rows(rows_iter).run()?;                 // binary batch; COPY/bulk-execute/INSERT BULK when available
let sp = tx.savepoint("s1")?; tx.rollback_to(sp)?;
tx.commit()?;

// Metadata with ADBC's fixed schemas; JDBC/ODBC catalog calls are derived from these.
let tables: Chunk = conn.metadata().objects(Depth::Tables, Some("public"), None)?;

// Errors carry SQLSTATE, vendor code, hints and a transient/non-transient class.
match conn.execute("...", &[]) { Err(e) if e.sqlstate().class() == "40" && e.is_transient() => retry(), .. }
```

The async API is the same surface with `async fn` on the same types, generated from the same
sans-io drivers; `karez-rt`'s tokio adapter is a feature.

### 2.6 Connection URL

`karez:<driver>[+<variant>]://user[:pw]@host1[:port],host2/db?k=v`, with each driver lowering
keywords to its native names (libpq's 55 keywords, Connector/J's properties, ODBC key=value).
Native URLs (`postgresql://`, `mysql://`, `jdbc:...`, an ODBC connection string) are accepted and
translated so existing configuration moves over unchanged. Passwords may be a reference to a
provider (environment, file, a callback for IAM or Entra tokens), never logged, and zeroised.

### 2.7 Drivers in scope

| driver | protocol | auth | bulk path | notes |
|---|---|---|---|---|
| PostgreSQL 13–18 | 3.0, 3.2 negotiation, direct TLS (17), extended query, binary | SCRAM-SHA-256(-PLUS), GSS/SSPI, cert, OAUTHBEARER (18); MD5 only when allowed | COPY BINARY in and out | LISTEN/NOTIFY, cancel keys, portals for partial fetch |
| MySQL 8.0–9.x, MariaDB 10.6–11.x | binary protocol, zlib/zstd compression | caching_sha2 (RSA exchange only over TLS or with a key file), ed25519, PARSEC | `LOAD DATA LOCAL` (off unless the statement enables it), MariaDB bulk execute | metadata skipping, multi-result sets |
| SQL Server 2017–2025, Azure SQL | TDS 7.4, TDS 8.0 strict (ALPN `tds/8.0`) | SQL, NTLM/Kerberos via SSPI and GSSAPI, Entra ID tokens (FedAuth) | `INSERT BULK` token stream | routing/redirect, 32 KB packets, UCS-2 fast path, PLP |
| SQLite 3.45+ | in-process C API | n/a | prepared insert in transaction | bundled amalgamation; `NOMUTEX`; FFI calls per row minimised to one type probe plus one accessor |
| ODBC bridge | unixODBC, iODBC, Windows DM | passthrough | array binding | reaches every database with an ODBC driver; row-wise binding into chunks |

After 1.0, by demand: ClickHouse native (columnar, LZ4, a natural fit for the chunk), DuckDB
(data chunks API), Flight SQL client, Oracle thin (a legal decision: Oracle ships thin clients
in Python, Node and Go, but the protocol is undocumented), and the Arrow-based cloud
warehouses, which already deliver Arrow and only need an HTTP client.

### 2.8 Design mathematics applied

Formulas restated from research.md §6; variables defined there.

- **Chunk size:** throughput is flat for chunks between about 100 KB and 10 MB and falls
  outside it (Raasveldt and Mühleisen), while vectors of 128–8K values fit the cache (X100). So
  1,024 rows or 1 MiB per chunk, builder reused per connection.
- **Compression:** compress a chunk only if link bandwidth B satisfies
  B < (1 − 1/r) / (1/C_c + 1/C_d) for ratio r, compression speed C_c and decompression speed
  C_d. With LZ4 (about 700 MB/s in, 4 GB/s out, r ≈ 2) that is below roughly 2–3 Gbit/s per
  core; with zstd -1 about 2 Gbit/s; never on loopback or 25 GbE. Default: off on loopback and
  Unix sockets, LZ4 or zstd on when the measured link is at or under 1 Gbit/s or the user asks.
- **Pipelining:** throughput is bounded by N / RTT for N requests in flight; k statements per
  round trip collapse k × RTT to about one RTT. PostgreSQL's own example: 100 statements at
  300 ms RTT, 30 s unpipelined, 0.3 s pipelined.
- **Fetch size:** to run at a fraction η of link speed a fetch must carry at least
  (η / (1 − η)) × B × RTT bytes (nine bandwidth-delay products for 90%), or stream without
  per-batch acknowledgement. The chunk path streams; portals are used only when the caller asks
  for partial results.
- **Pool size:** Little's law L = λ W sizes in-use connections; Erlang C gives the wait
  probability for c connections; the default maximum follows the HikariCP argument.
- **Decode cost:** text-to-double runs at about 45–50 M values/s per core even with SIMD
  parsers, while a binary fixed-width value is a load; the estimated per-row cost of a 16-numeric
  text row is 300–400 ns against memcpy class for binary. Hence hard rule 2.

## 3. Standards parity

Parity is declared **per standard and per level**, with a checklist in the repository that CI
turns into a published matrix. Research.md §4 holds the inventories.

| standard | level claimed at 1.0 | how it is met |
|---|---|---|
| JDBC 4.3 (JSR 221) and the 4.4/4.5 maintenance releases | full `java.sql` surface via the `karez-jdbc` facade: Connection (58 methods), Statement (54), PreparedStatement (58), CallableStatement, ResultSet (about 195: forward-only and scroll-insensitive over a cached chunk list; updatable cursors are emulated with a generated statement and quoted identifiers), ResultSetMetaData, ParameterMetaData, DatabaseMetaData (about 177 methods, derived from the ADBC-schema metadata chunks), Savepoint, Blob/Clob/Array/Struct (AutoCloseable per 4.5), SQLException hierarchy with SQLSTATE, escape syntax `{fn} {call} {d} {t} {ts} {oj} {escape}` and 4.5's `{\ ... \}`, `Types.DECFLOAT=2015`, `Types.JSON=2016`, `DataSource` and `ConnectionPoolDataSource` over `karez-pool`. XA is out of scope for 1.0. "JDBC Compliant" cannot be claimed without the Oracle-licensed TCK; the plan claims "JDBC 4.3 API-complete" and ships its own suite. |
| ODBC 3.8 | **Core and Level 1**, with the Level 2 features that the chunk model gives for free (array binding, `SQLFetchScroll` over cached chunks, `SQLBulkOperations` add); descriptors APD/IPD/ARD/IRD; the full SQLSTATE Appendix A mapping; `SQLGetInfo` info types; Unicode `W` entry points; a setup library with `ConfigDSN`; `DriverODBCVer=03.80`. Asynchronous execution and `SQLCancelHandle` after 1.0. Tested with Microsoft's ODBC Test and against Power BI, Tableau and Excel. |
| ADBC 1.1.0 | the full 31-function driver table, typed options, `Cancel`, `GetStatistics`, `ExecuteSchema`, `GetObjects`/`GetInfo`/`GetTableSchema` with the fixed Arrow schemas, bulk ingest modes create/append/replace/create_append, incremental execution, TOML manifests and the OS search paths; passes the C validation suite and the Go/Python suites. Distributed through the ADBC Driver Foundry channel when ready. |
| R2DBC 1.0 | not a Java reactive implementation, but its **TCK scenarios (about 35)** are ported to Rust as the conformance suite for the async API, because it is the only open TCK among these standards. |
| SQL types | SQL:2023 predefined types including JSON; SQLSTATE class codes from ISO with PostgreSQL's `P` and SQL Server's `S` vendor subclasses preserved alongside the normalised class. |

The checklist's nine areas (connection, transactions, statements, results, metadata, types,
errors, loading and bridging, async) come from research.md §4.8 and live in
`docs/parity/`.

## 4. Repository layout

A fresh repository, `pmsudhi/karez`, with no code or history from tokio-rs/rdbc.

```
karez/                        workspace
  Cargo.toml                 resolver 3, MSRV 1.85, shared lints, [profile.release] lto, codegen-units 1, panic abort for cdylibs
  crates/
    karez-chunk/ karez-codec/
    karez-proto-pg/ karez-proto-mysql/ karez-proto-tds/
    karez-rt/ karez-pool/ karez/
    karez-pg/ karez-mysql/ karez-tds/ karez-sqlite/ karez-odbc/
    karez-capi/ karez-adbc/ karez-odbc-driver/
  bindings/ java/ (Gradle, FFM facade, JNI fallback)  python/ (maturin)  node/ (napi-rs)
  bench/                     the §6 harness: Rust load generator with HdrHistogram, JMH projects, scripts
  conformance/               parity checklist, R2DBC-TCK port, sqllogictest corpus, ADBC validation runner
  fuzz/                      cargo-fuzz targets (dev-only, never in a shipped dependency tree)
  docs/ design/ research/ parity/ BENCHMARKS.md
  karez-cli/                 a SQL console and benchmark driver on the public API
```

### 4.1 CI

- Linux x86-64 and ARM64, Windows 2025 (SChannel-free: rustls plus the platform verifier;
  SSPI for TDS), macOS; MUSL static build for size tracking.
- Server matrix in containers: PostgreSQL 13, 16, 18; MySQL 8.0, 8.4, 9.x; MariaDB 10.6,
  11.x; SQL Server 2019, 2022, 2025 (Linux images, plus a Windows runner for SSPI); SQLite
  bundled.
- Every PR: fmt, clippy with `-D warnings`, tests, Miri on the sans-io and chunk crates,
  `cargo-deny` (licences, advisories), `cargo-vet`, the size report (stripped bytes per
  configuration, failing on regression over 2%), the dependency-count report, and the
  `unsafe` count.
- Nightly: the fuzz corpus for one hour per target, ASan on `karez-rt` and the FFI drivers, the
  §6 micro-benchmarks on a pinned runner with criterion, the ADBC validation suites.
- Releases: semver with `cargo-semver-checks`, signed artefacts for the cdylibs and wheels,
  an SBOM.

## 5. Size and dependency accounting

Targets, measured on every CI run and published in `docs/BENCHMARKS.md`:

| configuration | target (stripped, release, x86-64 Linux) |
|---|---|
| `karez` + `karez-pg`, blocking, no TLS, hello-world | **at most 600 KB** |
| the same with rustls (aws-lc-rs) and the platform verifier | at most 1.6 MB |
| all five drivers, blocking, rustls | at most 3.0 MB (Alpine's sqlx-cli with three drivers and rustls installs at 3.3 MiB; we carry five) |
| `karez-adbc` cdylib, all drivers | at most 3.0 MB |
| `karez-odbc-driver` cdylib | at most 3.0 MB |
| direct dependencies, core and protocol crates | **zero** |
| direct dependencies, a driver with TLS | rustls, its crypto provider, the platform verifier: 3 |

Means: `opt-level = "s"` for the cdylibs and `3` for the library defaults with a measured
comparison; fat LTO; `codegen-units = 1`; `panic = "abort"` in the C exports; no `regex`,
`serde`, `chrono` or `tokio` in non-optional paths; hand-written codecs; trait objects at the
driver boundary to stop monomorphisation of the whole stack per driver. The `cameleer-tls` crate
(std-only TLS 1.3/1.2 from the same owner, 0.4.0) is an **opt-in** feature only. Its AES is
constant time (bitsliced; the crate's doc comment saying it uses table S-boxes is stale, checked
against the source on 2026-11-09), but it is unaudited, its RSA is unblinded and it has no
hardware AES path, so it cannot be the default for a database client yet (research.md §8). If it
is audited and gains an AES-NI/PCLMULQDQ path, it becomes the zero-dependency TLS option and the
600 KB configuration grows by its size alone.

## 6. Benchmark method

Nothing in §1.3 or §9 is accepted from a closed-loop run or a single machine.

### 6.1 Load generation

- **Open loop**: a Rust load generator in `bench/` issues requests at fixed arrival rates and
  records latency from the *intended* start time in an HdrHistogram (coordinated omission
  corrected). Cross-checked with `pgbench -R` and, for HTTP-shaped runs, wrk2.
- Connection sweeps 1, 8, 64, 256, 512. Rates swept to find **SLO throughput**: the highest
  rate at which p99 stays under the scenario's bound.
- Both the row-view API and the chunk API are measured; both are reported.
- **Pipelined and unpipelined runs are separate tables.** TechEmpower banned cross-request
  pipelining in Round 21 for good reason; a reader must be able to see both.

### 6.2 Machines and hygiene

- Database on its own host; clients on another; a third for the load generator where the
  client is a server process. Same server version, configuration and dataset for every client.
  CPU governor fixed, turbo and SMT recorded, `perf stat` captured.
- JVM clients: an LTS JDK with 60 s warm-up and 300 s runs for load tests; JMH with at least 5
  forks and 10 warm-up iterations for micro-benchmarks; HikariCP sized per its own guide; a
  GraalVM native build reported too. JDBC drivers run with **defaults and tuned** settings, both
  published.
- Repetition where the variance lives (Kalibera and Jones): at least 5 runs per cell; medians
  with confidence intervals; no best-of-N.
- Metrics per cell: SLO throughput, p50/p99/p99.9, CPU-seconds per million rows, RSS, bytes on
  the wire, allocations per row (Rust: a counting allocator; JVM: allocation profiler), binary
  or JAR size.

### 6.3 Competitors

PostgreSQL: libpq (the C ceiling), asyncpg, psycopg3 binary, pgx, tokio-postgres (pooled
and per-core), zero-postgres, wtx, deadpool-postgres, sqlx, diesel, pgJDBC with HikariCP, vertx-pg-client, r2dbc-postgresql,
ConnectorX for the bulk scenario. MySQL/MariaDB: libmariadb, mysql_async, sqlx, MariaDB
Connector/J (with and without pipelining), MySQL Connector/J, go-sql-driver. SQL Server:
tiberius, mssql-jdbc, go-mssqldb, FreeTDS. SQLite: rusqlite, sqlx, sqlite-jdbc. ADBC: the
PostgreSQL ADBC driver, DuckDB JDBC versus the chunk path.

### 6.4 Scenarios

| # | scenario | what it isolates |
|---|---|---|
| S1 | `SELECT 1` | protocol overhead per round trip |
| S2 | point select, one 10-column mixed row by primary key | the OLTP case; the headline S2 number is unpipelined |
| S3 | 12-row select (Fortunes-like) with a string column | small result decoding |
| S4 | 1M-row, 10-column scan into Arrow and through the row view | bulk decode, bytes per row, RSS |
| S5 | 100k-row batch insert, prepared | batch path (MariaDB bulk, pgJDBC `reWriteBatchedInserts` tuned) |
| S6 | COPY / bulk copy / `INSERT BULK` of 10M rows | the bulk-load ceiling (PgBulkInsert's 712k rows/s is the Java mark) |
| S7 | S2 with TLS on | TLS record and handshake cost; handshakes per second separately |
| S8 | S2 at 512 connections with a pool of 32 | pool fairness and wait distribution |
| S9 | ConnectorX-style partitioned parallel scan, 4 and 8 connections | the analytics path against ConnectorX itself |
| S10 | the same S2/S4 through the ADBC export from Python and through the JDBC facade from Java | export overhead; the pgJDBC claim inside Java |

## 7. Security plan

### 7.1 The seven rules from the CVE record

| rule | the CVE it answers |
|---|---|
| 1. Never inline a literal client-side; placeholders are always server-bound | pgJDBC CVE-2024-1597 (CVSS 10.0), pgx CVE-2024-27289 |
| 2. Escaping, where unavoidable (identifiers), is encoding-aware and fails closed on invalid multibyte input | libpq/psycopg CVE-2006-2314, libpq CVE-2025-1094 |
| 3. Checked arithmetic on every length and count; a message that overflows is a protocol error | sqlx RUSTSEC-2024-0363, pgx CVE-2024-27304 |
| 4. No connection-string parameter loads code, opens a file or picks a class | pgJDBC CVE-2022-21724, Connector/J CVE-2017-3523, 2023-21971, 2023-22102 |
| 5. Server-initiated actions (`LOAD DATA LOCAL`, file reads) are opt-in per statement, never per connection | the MySQL rogue-server family, 2026 MariaDB Connector/J CVEs |
| 6. Identifiers in driver-built SQL are quoted | pgJDBC CVE-2022-31197 |
| 7. Credentials: tokens may exceed 1 KB, `Debug` redacts, secrets are zeroised, nothing is logged | cloud IAM and Entra token paths |

### 7.2 Unsafe budget and memory safety

- `#![forbid(unsafe_code)]` in `karez`, `karez-pool`, the three protocol crates, `karez-codec`'s
  scalar paths and `karez-chunk` (bitmap and offset arithmetic stay safe and checked).
- `unsafe` allowed in `karez-rt` (socket options, SIMD dispatch via `std::arch` with a scalar
  fallback and a differential test on every fuzz input), `karez-sqlite`, `karez-odbc`, and the
  export crates' C ABI. Target: **at most 200 `unsafe` blocks at 1.0**, each with a `SAFETY:`
  comment, counted in CI.
- Miri on everything forbidding unsafe; ASan and TSan nightly on the rest.

### 7.3 Fuzzing and differential testing

Targets: every protocol decoder with inputs split at arbitrary byte boundaries (the parse must
not depend on how bytes arrive); every encoder round-tripped through its decoder; the chunk
builder; the URL parser; the SQLSTATE mapper; the whole driver through a simulated socket with
invariants (no panic, bounded memory, limits enforced). **Differential:** record libpq,
libmariadb and FreeTDS sessions with a proxy and replay them through our decoders, comparing
every decoded value. No public Rust driver does this today (research.md §11); it is the
cheapest route to protocol fidelity and a credible claim.

### 7.4 Defaults

TLS `verify-full` with the OS trust store (`rustls-platform-verifier`); `sslmode=prefer` and
`Encrypt=false` exist only as explicitly named options and log a warning once. Authentication
allow-list defaults: PostgreSQL `scram-sha-256,scram-sha-256-plus,gss,cert`; MySQL
`caching_sha2_password` with RSA exchange only over TLS or a pinned key file; TDS 8.0 strict
when the server supports it, else 7.4 with encryption required. Limits: message size, column
count, row count per chunk, statement length, all configurable with sane defaults. Session
tickets on, 0-RTT off.

## 8. Milestones and measurable targets

"Engineer-weeks" assumes one experienced engineer working with agents. After M1, M2, M3, M4
and M5 touch different crates and can run in parallel worktrees.

| # | milestone | effort | exit targets |
|---|---|---|---|
| **M0** | **Measurement spike (go/no-go):** `karez-chunk`, `karez-proto-pg` for the extended protocol in binary, a caller-driven blocking connection and an async connection task, SCRAM, the row view, the `bench/` load generator; baselines for every §6.3 PostgreSQL competitor on the same hosts | 3 weeks | the §1.3 gate. Also: the size and dependency floor measured for tokio-postgres, sqlx, diesel and rusqlite hello-worlds, since no public data exists |
| M1 | Core API and PostgreSQL driver complete: types, transactions, savepoints, COPY, LISTEN, cancel, portals, metadata chunks, TLS with rustls, GSS/SSPI, OAUTHBEARER, async adapter, `karez-pool`, URL translation, errors with SQLSTATE; fuzz and differential harnesses | 8 weeks | S1–S8 on PostgreSQL meet or beat the M0 margins with TLS on; S6 COPY **at least 1.5x PgBulkInsert**; S8 wait-time distribution within Erlang C prediction; zero open fuzz findings; parity checklist green for the PostgreSQL rows |
| M2 | MySQL and MariaDB driver | 6 weeks | S2 **at least 2x MySQL Connector/J** and at least MariaDB Connector/J with pipelining; S5 within 10% of MariaDB Connector/C bulk; auth matrix (caching_sha2, ed25519, PARSEC, TLS 1.3) green on 8.0, 8.4, 9.x, 10.6, 11.x |
| M3 | SQL Server driver: TDS 7.4 and 8.0 strict, SSPI and Kerberos, Entra tokens, routing, `INSERT BULK` | 7 weeks | S2 **at least 2x mssql-jdbc** and at least 1.3x tiberius; S4 UCS-2 decode at or above 1 GB/s per core; Windows SSPI and Linux Kerberos both green |
| M4 | SQLite driver and the ODBC bridge | 4 weeks | SQLite S2 within 20% of rusqlite (the FFI floor) and **at least 10x sqlx-sqlite**; ODBC bridge passes the parity rows it can against PostgreSQL's ODBC driver |
| M5 | Exports: `karez-capi`, `karez-adbc` with manifests, Python wheels (DB-API 2.0 plus Arrow stream, free-threaded builds), Node | 5 weeks | ADBC C, Go and Python validation suites pass; S10 from Python **at least 10x pandas `read_sql`** and within 15% of ConnectorX on S9; wheel at most 4 MB |
| M6 | JDBC facade on Java 22+ FFM (JNI fallback for 17), full `java.sql` surface, `DataSource`, escape processing | 6 weeks | **the pgJDBC claim:** S2 at least 1.5x and S4 at least 3x tuned pgJDBC from inside Java, published with the §6 method; the JDBC-shaped suite (R2DBC TCK port plus our own) green; Spark and Trino JDBC source smoke tests |
| M7 | ODBC driver export: `SQL*` entry points, descriptors, setup library, Core and Level 1 | 6 weeks | Microsoft ODBC Test Core and Level 1 clean; Power BI, Tableau and Excel connect and query; the first Rust ODBC driver |
| M8 | Hardening and 1.0: 30 CPU-days of fuzzing without a new crash, external audit of `karez-rt`, the three decoders and the C ABI, docs, the public release | 4 weeks | no open high or critical findings; parity matrix published; size table within §5; `docs/BENCHMARKS.md` reproducible from `bench/` |

**Total to 1.0:** about 49 engineer-weeks; the first useful artefact (M1, a PostgreSQL driver
whose design target is parity with the fastest Rust client on single-row queries and the §1.3
margin on bulk and columnar scans, to be confirmed by the M1 measurements) at about week 11;
M2–M5 in parallel compress the calendar to roughly 30 weeks.

### 8.1 Project identity and release strategy

karez is a standalone project, not a fork. On 2026-11-09 the owner decided to start fresh: the
repository `pmsudhi/karez` (private) has no code or history from tokio-rs/rdbc, which is cited in
the research as prior work. Because no rdbc code is copied, its Apache-2.0 licence places no
obligations on karez.

1. Reserve `karez` and the planned `karez-*` crate names on crates.io, and `karez` on PyPI and
   npm, with placeholder releases once M0 passes. All were free on 2026-11-09.
2. Develop in the private repository through M1. Make it public with the M1 PostgreSQL driver,
   the benchmark harness and the published §6 results, so the first public claim can be rerun.
3. Tell the tokio-rs/rdbc maintainers and its original author about karez. Its 20 open issues ask
   for exactly this feature list: async, transactions, named parameters, batches, generated
   keys, multiple result sets, temporal and 64-bit types, a columnar row set, an ODBC bridge,
   dynamic loading and SQL Server. Offer it as a successor that its README could point to. The
   plan does not depend on their answer.
4. Distribute the ADBC driver through the ADBC Driver Foundry channel once the validation suites
   pass (M5).

## 9. Risks

| risk | likelihood | impact | mitigation |
|---|---|---|---|
| The margin over tokio-postgres is small because it already pipelines and uses binary | expected on OLTP | medium | the plan already asks only for OLTP parity; the M0 stop rule rests on S4, where the design differs most; if S2 parity but S4 3x, the project still stands on the analytics and export value, and the plan says so honestly |
| The pgJDBC comparison is contested (defaults vs tuned, JIT warm-up, GraalVM) | high | high | publish both configurations, the JMH projects and the raw histograms; invite pgJDBC maintainers to rerun; never lead with pipelined numbers |
| TDS is the largest protocol and tiberius has no maintainer to learn from | medium | medium | fork tiberius's test corpus; differential replay against FreeTDS and mssql-jdbc captures; MARS deferred |
| Decimal128/256 and temporal types are where drivers quietly lose data | high | medium | property tests over the full SQL type × NULL × extreme value × time zone matrix, cross-checked against libpq's text output |
| The JDBC facade cannot claim "JDBC Compliant" without the Oracle TCK | certain | low | claim "JDBC 4.3 API-complete" and ship the suite; pursue OCTLA access only if a user needs the mark |
| An ODBC driver in Rust has no precedent; driver managers are quirky | medium | medium | start from ClickHouse ODBC's C++ surface as a map; test with Microsoft ODBC Test and unixODBC early in M7 |
| Oracle thin protocol is undocumented and the legal position unclear | medium | low | out of 1.0; decide with counsel; the OCI path via the ODBC bridge works meanwhile |
| Chunk model adds latency to one-row OLTP results | low | medium | builder reuse, no allocation on reset, measured in M0 S1/S2 unpipelined |
| No paid external audit (owner, 2026-11-16) | certain | high for the ODBC FFI crate (`unsafe`, loaded by BI tools) | mitigation: fuzzing to the M8 target, `cargo audit` and `cargo deny` in CI, two-person review of every `unsafe` block, a written safety argument per block, public review before 1.0; revisit if the ODBC driver gets real users |
| Size budget blown by TLS | medium | low | rustls is measured in M0; cameleer-tls as the std-only fallback once audited and given a hardware AES path |
| Weekly API limits and process restarts interrupt agent-driven work (as happened during this research) | certain | low | notes are written to the repository as they are produced; milestones are small and committed often |

## 10. The five numbers to beat

Expected, with the evidence for each. All on the §6 machines; nothing is measured yet.

| # | metric | today (published) | target | evidence |
|---|---|---|---|---|
| 1 | S2 point select, 64 connections, unpipelined, PostgreSQL | tokio-postgres, zero-postgres and wtx are the Rust ceiling; JDBC-based Java is 2–6x behind the best Java (TFB) | **within 5% of the fastest Rust client, at least 2x tuned pgJDBC** | parity, not a margin: no published design beats the fastest clients by much on OLTP (research.md §12); the JDBC margin comes from per-row JVM objects (about 866 B/row in the only modern measurement), which the chunk builder never creates |
| 2 | S4 1M-row scan to Arrow | ConnectorX 13x pandas; ADBC-over-DuckDB 13x DuckDB JDBC at a fifth of the heap; asyncpg 2.1M rows/s geomean | **at least 3x tuned pgJDBC rows/s, at most one third of its RSS; within 15% of asyncpg** | Raasveldt: column-major chunks removed the 10–60x protocol penalty; no per-row object; decode is a copy for fixed-width types |
| 3 | stripped PostgreSQL hello-world, no TLS | no public data for tokio-postgres or sqlx (research.md §9); Alpine sqlx-cli 3.3 MiB with 3 drivers and rustls | **at most 600 KB**, zero direct dependencies | hand-written codecs, no serde/regex/chrono/tokio in the path; measured against the competitors' own builds in M0 |
| 4 | S6 bulk load, PostgreSQL COPY BINARY | PgBulkInsert 712k rows/s (Java 25, PG 18); pgx 1.9M rows/s | **at least 1.5x PgBulkInsert; within 20% of pgx** | encode from chunks with no per-row objects; one write per 1 MiB |
| 5 | S10 inside Java through the FFM facade | DuckDB ADBC vs DuckDB JDBC 13x (third party); no native-vs-pgJDBC number exists | **at least 1.5x tuned pgJDBC on S2, at least 3x on S4** | FFM downcalls cost 20–35 ns and are per chunk, not per cell; the Arrow Java C Data Interface hands off-heap batches to Java |

## 11. What the owner needs to decide

Already decided on 2026-11-09, and no longer open: the name karez and the Apache-2.0 licence
(as for simoom; permissive and patent-safe for the C, Java, Python and ODBC distributions).
See [STATUS.md](../STATUS.md).

**Decided on 2026-11-13 by the owner** (details in [STATUS.md](../STATUS.md)): TLS is an optional `rustls`
feature ([tls-decision.md](tls-decision.md)); async is a runtime-free design ([karez-rt.md](karez-rt.md));
**the ODBC driver is in 1.0** (a separate FFI crate with `unsafe`, covered by the audit; this adds months to the
schedule above and the milestone table has not been updated for it); the external audit before 1.0 covers the
core, the protocol crates and the ODBC FFI crate, not `cameleer-tls`; the `karez` and `karez-*` names are to be
reserved now (the owner does it); after a lost connection only commands never written are moved to a new one.

1. **Go or no-go on Milestone 0.** It measures and writes about 3,000 lines of spike code; it
   changes nothing published.
2. **Name reservation:** reserve the crates.io, PyPI and npm names now or at M0 (§8.1); a
   trademark search before the public release is optional.
3. **Scope of 1.0 exports:** ADBC and Python are cheap and high-leverage; the JDBC facade is
   where the headline claim lives; the ODBC driver is the most work for the BI audience.
   Confirm all three or defer ODBC.
4. **cameleer-tls:** opt-in only until audited; whether to fund the audit and the hardware
   AES path in the Cameleer repository, which would make the zero-dependency TLS configuration
   possible.
5. **Oracle:** out of 1.0 pending a legal view on the thin protocol.
6. **Budget for the external audit before 1.0** (§8, M8).
