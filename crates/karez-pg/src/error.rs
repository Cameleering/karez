use karez_chunk::ChunkError;
use karez_proto_pg::{ErrorFields, ProtoError};
use std::fmt;

/// An error returned by the server in an ErrorResponse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerError(Box<ErrorFields>);

impl ServerError {
    /// Wrap the fields of an ErrorResponse (for layers that parse the protocol themselves).
    pub fn from_fields(f: ErrorFields) -> Self {
        Self::new(f)
    }

    pub(crate) fn new(f: ErrorFields) -> Self {
        ServerError(Box::new(f))
    }

    /// The five-character SQLSTATE, for example `23505`.
    pub fn sqlstate(&self) -> &str {
        &self.0.code
    }

    /// The primary message from the server.
    pub fn message(&self) -> &str {
        &self.0.message
    }

    /// Every field the server sent.
    pub fn fields(&self) -> &ErrorFields {
        &self.0
    }
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} (SQLSTATE {})",
            self.0.severity, self.0.message, self.0.code
        )?;
        if let Some(d) = &self.0.detail {
            write!(f, "; detail: {d}")?;
        }
        Ok(())
    }
}

/// Everything that can go wrong in this crate.
#[derive(Debug)]
pub enum Error {
    /// A socket error, including a connection closed by the peer mid-message.
    Io(std::io::Error),
    /// The server sent bytes that are not valid protocol.
    Proto(ProtoError),
    /// A chunk could not be built or read.
    Chunk(ChunkError),
    /// The server reported an error. The connection stays usable.
    Server(ServerError),
    /// Bad configuration or connection URL.
    Config(String),
    /// TLS could not be negotiated or verified (the server refused it, the certificate was not
    /// trusted, the name did not match, or the `tls` feature is not compiled in).
    Tls(String),
    /// Authentication could not be completed (policy, method, or server proof).
    Auth(String),
    /// A result column has a type this version cannot decode.
    UnsupportedType {
        /// Column name.
        column: String,
        /// Type OID.
        oid: u32,
        /// Type name, as the server knows it.
        type_name: String,
    },
    /// A value in the result cannot be represented by this version (for example a date past
    /// the Unix-epoch range). The connection stays usable.
    Value(String),
    /// Reading the COPY source or writing the COPY sink supplied by the caller failed. The COPY
    /// was aborted cleanly and the connection stays usable.
    Caller(std::io::Error),
    /// The server closed the connection.
    Closed,
    /// An earlier fatal error left the connection unusable.
    Broken,
    /// The call was made in a state where it is not allowed.
    Usage(&'static str),
}

impl Error {
    /// The SQLSTATE if this is a server error.
    pub fn sqlstate(&self) -> Option<&str> {
        match self {
            Error::Server(e) => Some(e.sqlstate()),
            _ => None,
        }
    }

    /// True if the connection can no longer be used after this error.
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            Error::Io(_)
                | Error::Proto(_)
                | Error::Chunk(_)
                | Error::Closed
                | Error::Broken
                | Error::Tls(_)
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Proto(e) => write!(f, "protocol error: {e}"),
            Error::Chunk(e) => write!(f, "chunk error: {e}"),
            Error::Server(e) => write!(f, "{e}"),
            Error::Config(m) => write!(f, "configuration error: {m}"),
            Error::Tls(m) => write!(f, "TLS error: {m}"),
            Error::Auth(m) => write!(f, "authentication failed: {m}"),
            Error::UnsupportedType {
                column,
                oid,
                type_name,
            } => write!(
                f,
                "column \"{column}\" has type {type_name} (oid {oid}), which this version of karez cannot decode; \
                 cast it in SQL (for example ::text) or set unsupported_types=text"
            ),
            Error::Value(m) => write!(f, "value cannot be represented: {m}"),
            Error::Caller(e) => write!(f, "COPY source or sink failed: {e}"),
            Error::Closed => write!(f, "connection closed by the server"),
            Error::Broken => write!(f, "connection is broken by an earlier error"),
            Error::Usage(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) | Error::Caller(e) => Some(e),
            Error::Proto(e) => Some(e),
            Error::Chunk(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<ProtoError> for Error {
    fn from(e: ProtoError) -> Self {
        Error::Proto(e)
    }
}

impl From<ChunkError> for Error {
    fn from(e: ChunkError) -> Self {
        Error::Chunk(e)
    }
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
