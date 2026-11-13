//! A blocking PostgreSQL connection for karez.
//!
//! * One thread drives the socket; there is no worker and no channel. `TCP_NODELAY` is set and
//!   every request (Bind, Execute, Sync) goes out in a single `write`.
//! * Statements are prepared per connection and cached by SQL text and parameter types.
//! * All user data is bound server-side ([`Value`]); SQL text is never assembled from values.
//! * Results are binary and land in an Arrow-layout [`Chunk`]. Read them as rows with
//!   [`Cursor::next`] or as whole chunks with [`Cursor::next_chunk`].
//! * Passwords are redacted from `Debug` output and never logged.
//!
//! TLS needs the `tls` feature (rustls). The default `sslmode` is `verify-full`; without the
//! feature a connection needs an explicit `sslmode=disable`.

#![forbid(unsafe_code)]

mod config;
mod conn;
mod error;
mod pool;
#[cfg(feature = "tls")]
mod tls;
mod transport;
mod tx;

pub use config::{AuthPolicy, Config, SslMode, UnsupportedTypes};
pub use conn::{CancelToken, Connection, Cursor, Notification, RawParts, Statement};
pub use error::{Error, Result, ServerError};
pub use karez_chunk::{Chunk, Column, ColumnType, Date, Field, RowRef, Timestamp};
pub use karez_proto_pg::types::oid;
pub use karez_proto_pg::{ErrorFields, TxStatus, Value};
pub use pool::{Pool, PooledConnection};
pub use tx::{Isolation, Savepoint, Transaction};
