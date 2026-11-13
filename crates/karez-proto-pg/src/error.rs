use karez_chunk::ChunkError;
use std::fmt;

/// A protocol-level failure: malformed, oversized or unsupported input. After one of these the
/// byte stream cannot be trusted and the connection must be closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtoError {
    /// A message's length field is below 4 or negative.
    InvalidLength(i32),
    /// A message is larger than the configured maximum.
    MessageTooLarge {
        /// Length announced by the server.
        len: usize,
        /// The configured limit.
        max: usize,
    },
    /// A message body ended before a field was complete.
    Truncated(&'static str),
    /// A message body has bytes left over after its last field.
    TrailingBytes(&'static str),
    /// A string field is not valid UTF-8.
    InvalidUtf8,
    /// A string field has no terminating NUL.
    Unterminated,
    /// A string to be sent contains a NUL byte.
    InteriorNul,
    /// A backend message type this crate does not know.
    UnknownMessage(u8),
    /// A field holds a value outside its legal range.
    InvalidValue(&'static str),
    /// The server asked for an authentication method this crate does not implement.
    UnsupportedAuth(i32),
    /// A length or count does not fit its wire type, or arithmetic overflowed.
    Overflow,
    /// A binary value has the wrong size or contents for its type.
    BadValue {
        /// The type's OID.
        oid: u32,
        /// What is wrong.
        what: &'static str,
    },
    /// A DataRow does not match the RowDescription it is decoded against.
    RowShape(&'static str),
    /// SCRAM exchange failure.
    Scram(&'static str),
    /// A chunk could not accept a value.
    Chunk(ChunkError),
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtoError::InvalidLength(n) => write!(f, "invalid message length {n}"),
            ProtoError::MessageTooLarge { len, max } => {
                write!(f, "message of {len} bytes exceeds the limit of {max}")
            }
            ProtoError::Truncated(w) => write!(f, "message truncated in {w}"),
            ProtoError::TrailingBytes(w) => write!(f, "trailing bytes in {w}"),
            ProtoError::InvalidUtf8 => write!(f, "invalid UTF-8 in message"),
            ProtoError::Unterminated => write!(f, "string without terminator"),
            ProtoError::InteriorNul => write!(f, "string contains NUL"),
            ProtoError::UnknownMessage(t) => write!(f, "unknown message type 0x{t:02x}"),
            ProtoError::InvalidValue(w) => write!(f, "invalid value for {w}"),
            ProtoError::UnsupportedAuth(c) => write!(f, "unsupported authentication method {c}"),
            ProtoError::Overflow => write!(f, "length overflow"),
            ProtoError::BadValue { oid, what } => {
                write!(f, "bad binary value for type {oid}: {what}")
            }
            ProtoError::RowShape(w) => write!(f, "row does not match description: {w}"),
            ProtoError::Scram(w) => write!(f, "SCRAM failure: {w}"),
            ProtoError::Chunk(e) => write!(f, "chunk error: {e}"),
        }
    }
}

impl std::error::Error for ProtoError {}

impl From<ChunkError> for ProtoError {
    fn from(e: ChunkError) -> Self {
        ProtoError::Chunk(e)
    }
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, ProtoError>;
