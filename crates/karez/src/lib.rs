//! karez: database connectivity with Arrow-layout results.
//!
//! This is the first working slice (Milestone 0 spike): a **blocking PostgreSQL** client with no
//! TLS. It was verified against a real server on **PostgreSQL 16.15 only**; versions 14 and 17
//! were not run. Everything else in the plan (other databases, async, pool, exports) is not built.
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

#![forbid(unsafe_code)]

pub use karez_chunk as chunk;
pub use karez_pg::*;
pub use karez_proto_pg as proto_pg;
