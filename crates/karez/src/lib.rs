//! karez: database connectivity with Arrow-layout results.
//!
//! This is the first working slice (Milestone 0 spike): a **PostgreSQL** client with optional
//! TLS (feature `tls` of `karez-pg`). It was verified against a real server on **PostgreSQL 14.25, 16.15 and 17.12**;
//! versions 13, 15 and 18 were not run. Everything else in the plan (other databases, exports) is not built.
//!
//! ```no_run
//! use karez::{Connection, Value};
//!
//! # fn main() -> Result<(), karez::Error> {
//! let mut conn = Connection::connect_url(
//!     "postgresql://app:secret@127.0.0.1:5432/sales?sslmode=disable",
//! )?;
//!
//! // Row view: strings borrow from the chunk, nothing is allocated per row.
//! let mut cur = conn.query("SELECT id, name FROM customers WHERE id < $1", &[Value::i64(10)])?;
//! while let Some(row) = cur.next()? {
//!     let id: i64 = row.get(0)?;
//!     let name: &str = row.get(1)?;
//!     println!("{id} {name}");
//! }
//! drop(cur);
//!
//! // Columnar view: whole chunks (up to 1,024 rows or 1 MiB).
//! let mut cur = conn.query("SELECT id FROM customers", &[])?;
//! while let Some(chunk) = cur.next_chunk()? {
//!     println!("{} rows", chunk.rows());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Reading a large table on several connections
//!
//! [`scan_partitioned`] splits an ordinary table into ranges of heap blocks, reads each range on its own
//! connection and thread, and by default makes all of them see one snapshot. [`scan_partitioned_pool`] does the
//! same with connections from a [`Pool`]. It needs PostgreSQL 14 or later. On a link that is not the limit it is
//! several times faster than one connection; on a slow link the link is the limit (measured numbers:
//! `docs/design/perf-program.md`).
//!
//! ```no_run
//! use karez::{scan_partitioned, Config, Scan};
//!
//! # fn main() -> Result<(), karez::Error> {
//! let cfg = Config::from_url("postgresql://app:secret@127.0.0.1:5432/sales?sslmode=disable")?;
//! let cols = ["id", "amount"];
//! // one state per partition; `on_chunk` runs on that partition's thread for every chunk it reads
//! let sums = scan_partitioned(
//!     &cfg,
//!     &Scan::new("public.orders", &cols).partitions(4),
//!     |_partition| 0i64,
//!     |sum, chunk| {
//!         for v in chunk.column(1)?.data().chunks_exact(8).take(chunk.rows()) {
//!             *sum += i64::from_le_bytes(v.try_into().unwrap());
//!         }
//!         Ok(())
//!     },
//! )?;
//! println!("total {}", sums.iter().sum::<i64>());
//! # Ok(())
//! # }
//! ```
//!
//! ## Inserting many rows
//!
//! [`Connection::insert_batch`] sends multi-row `INSERT ... VALUES` statements (up to 1,000 rows each) in one
//! atomic batch. For plain loads [`Connection::copy_in`] is faster still.
//!
//! ```no_run
//! use karez::{Connection, Value};
//!
//! # fn main() -> Result<(), karez::Error> {
//! let mut conn = Connection::connect_url("postgresql://app:secret@127.0.0.1:5432/sales?sslmode=disable")?;
//! let rows = [
//!     [Value::i64(1), Value::text("first")],
//!     [Value::i64(2), Value::text("second")],
//! ];
//! let refs: Vec<&[Value<'_>]> = rows.iter().map(|r| &r[..]).collect();
//! let inserted = conn.insert_batch("public.notes", &["id", "body"], &refs)?;
//! assert_eq!(inserted, 2);
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub use karez_chunk as chunk;
pub use karez_pg::*;
pub use karez_proto_pg as proto_pg;
