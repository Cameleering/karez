//! Sans-io PostgreSQL wire protocol 3.0 for karez.
//!
//! This crate takes bytes and returns messages and bytes. It does no I/O, spawns nothing and
//! has no dependencies besides `karez-chunk`. It contains:
//!
//! * [`frame`]: incremental message framing with a maximum message size,
//! * [`frontend`]: encoders for the messages a client sends,
//! * [`backend`]: parsers for the messages a server sends,
//! * [`types`]: binary result decoding straight into a [`karez_chunk::ChunkBuilder`], and
//!   binary parameter encoding,
//! * [`scram`], [`sha256`], [`base64`]: SCRAM-SHA-256 and the primitives it needs, written here
//!   so the crate stays dependency free.
//!
//! Authentication methods understood: trust (`AuthenticationOk`), cleartext password and
//! SCRAM-SHA-256 without channel binding. MD5 and the rest are parsed only so a caller can refuse
//! them.

#![forbid(unsafe_code)]

pub mod backend;
pub mod base64;
pub mod error;
pub mod frame;
pub mod frontend;
mod render;
pub mod scram;
pub mod sha256;
pub mod types;
mod wire;

pub use backend::{BackendMessage, DataRow, ErrorFields, FieldDescription, SqlState, TxStatus};
pub use error::{ProtoError, Result};
pub use frame::{Frame, FrameReader, DEFAULT_MAX_MESSAGE};
pub use types::{ResultPlan, Value};
