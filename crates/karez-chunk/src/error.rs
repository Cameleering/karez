use crate::ColumnType;
use std::fmt;

/// Errors from building or reading a chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkError {
    /// The column has a different type from the one asked for.
    TypeMismatch {
        /// Type the caller asked for.
        expected: ColumnType,
        /// Type the column has.
        found: ColumnType,
    },
    /// A row or column index is outside the chunk.
    OutOfBounds {
        /// The index asked for.
        index: usize,
        /// The length it was checked against.
        len: usize,
    },
    /// A value was read as non-optional but is NULL.
    Null,
    /// A length or offset calculation overflowed, or a buffer exceeded `i32::MAX` bytes.
    Overflow,
    /// A UTF-8 column holds bytes that are not valid UTF-8.
    InvalidUtf8,
    /// A row was finished while some column had no value for it.
    RowIncomplete,
    /// A column name was not found.
    UnknownColumn,
    /// A value does not fit the column it was pushed to (a time of day past 24 hours, a decimal
    /// with more digits than the column's precision).
    OutOfRange(&'static str),
    /// Internal invariant of a column or chunk is broken.
    Corrupt(&'static str),
}

impl fmt::Display for ChunkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChunkError::TypeMismatch { expected, found } => {
                write!(
                    f,
                    "type mismatch: asked for {expected:?}, column is {found:?}"
                )
            }
            ChunkError::OutOfBounds { index, len } => {
                write!(f, "index {index} out of bounds for length {len}")
            }
            ChunkError::Null => write!(f, "value is NULL"),
            ChunkError::Overflow => write!(f, "length arithmetic overflow"),
            ChunkError::InvalidUtf8 => write!(f, "invalid UTF-8"),
            ChunkError::RowIncomplete => write!(f, "row finished with a column missing a value"),
            ChunkError::UnknownColumn => write!(f, "unknown column"),
            ChunkError::OutOfRange(what) => write!(f, "value out of range: {what}"),
            ChunkError::Corrupt(what) => write!(f, "corrupt chunk: {what}"),
        }
    }
}

impl std::error::Error for ChunkError {}
