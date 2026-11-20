//! Arrow-layout columnar chunk for karez.
//!
//! A [`Chunk`] holds a fixed schema and a number of rows. Each [`Column`] stores a validity
//! bitmap (bit set means the value is present) plus either fixed-width little-endian data or
//! `i32` offsets and a byte buffer, as in the Arrow columnar format. Booleans are bit-packed.
//!
//! A [`ChunkBuilder`] is reused between chunks: [`ChunkBuilder::reset`] keeps every buffer's
//! capacity. Readers are checked: every index and offset is validated, and there is no
//! `unsafe` code in this crate.

#![forbid(unsafe_code)]

mod builder;
mod column;
mod error;
pub mod rng;
mod types;

pub use builder::{Chunk, ChunkBuilder, RowRef};
pub use column::{Column, FromColumn};
pub use error::ChunkError;
pub use types::{ColumnType, Date, Decimal128, Field, Interval, Time, Timestamp};

/// Rows after which a chunk is considered full (plan section 2.2).
pub const CHUNK_MAX_ROWS: usize = 1024;
/// Bytes after which a chunk is considered full (plan section 2.2).
pub const CHUNK_MAX_BYTES: usize = 1 << 20;
