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

    /// The two-character SQLSTATE class, for example `23` (integrity constraint violation) or
    /// `40` (transaction rollback). Empty if the server sent no code.
    pub fn class(&self) -> &str {
        self.0.code.get(..2).unwrap_or("")
    }

    /// True if the same statement may succeed when tried again, on this connection or a new
    /// one, without the caller changing anything: a serialization failure or deadlock (`40001`,
    /// `40P01`), a connection exception (class `08`), insufficient resources (class `53`), a lock
    /// that could not be taken (`55P03`), a server that is shutting down or not yet accepting
    /// connections (`57P01`, `57P02`, `57P03`), or a write sent to a read-only server after a
    /// failover (`25006`). Everything else, including constraint violations and syntax errors,
    /// is not transient. This is a hint for a retry loop, not a promise: the caller still decides
    /// whether the statement is safe to repeat (an `INSERT` whose commit outcome is unknown is not).
    pub fn is_transient(&self) -> bool {
        matches!(
            self.0.code.as_str(),
            "40001" | "40P01" | "55P03" | "57P01" | "57P02" | "57P03" | "25006"
        ) || matches!(self.class(), "08" | "53")
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

    /// The two-character SQLSTATE class if this is a server error.
    pub fn sqlstate_class(&self) -> Option<&str> {
        match self {
            Error::Server(e) => Some(e.class()),
            _ => None,
        }
    }

    /// True if repeating the operation, on a new connection if this one is gone, can succeed
    /// without any change by the caller. See [`ServerError::is_transient`] for the server codes.
    /// Socket errors that mean the peer or the path went away (reset, aborted, broken pipe,
    /// refused, timed out, end of file) and a connection closed by the server count too.
    /// Configuration, authentication, TLS verification, protocol, type and usage errors do not:
    /// they repeat identically.
    ///
    /// A transient error is not a licence to retry blindly. If the error came after a commit or a
    /// non-idempotent statement was sent, the outcome is unknown, and only the caller can tell
    /// whether to repeat it.
    pub fn is_transient(&self) -> bool {
        use std::io::ErrorKind as K;
        match self {
            Error::Server(e) => e.is_transient(),
            Error::Io(e) => matches!(
                e.kind(),
                K::ConnectionReset
                    | K::ConnectionAborted
                    | K::BrokenPipe
                    | K::ConnectionRefused
                    | K::TimedOut
                    | K::UnexpectedEof
                    | K::WouldBlock
            ),
            Error::Closed => true,
            _ => false,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn server(code: &str) -> Error {
        Error::Server(ServerError::new(ErrorFields {
            severity: "ERROR".into(),
            code: code.into(),
            message: "m".into(),
            ..ErrorFields::default()
        }))
    }

    #[test]
    fn class_is_the_first_two_characters() {
        assert_eq!(server("23505").sqlstate_class(), Some("23"));
        assert_eq!(server("40P01").sqlstate_class(), Some("40"));
        assert_eq!(server("").sqlstate_class(), Some(""));
        assert_eq!(Error::Closed.sqlstate_class(), None);
    }

    #[test]
    fn transient_server_codes() {
        for code in [
            "40001", "40P01", "08000", "08006", "08P01", "53000", "53100", "53200", "53300",
            "53400", "55P03", "57P01", "57P02", "57P03", "25006",
        ] {
            assert!(server(code).is_transient(), "{code} should be transient");
        }
    }

    #[test]
    fn permanent_server_codes() {
        for code in [
            "23505", "23503", "23502", "42601", "42P01", "42703", "22012", "22P02", "28000",
            "28P01", "3D000", "57014", "XX000", "0A000", "42501", "",
        ] {
            assert!(
                !server(code).is_transient(),
                "{code} should not be transient"
            );
        }
    }

    #[test]
    fn non_server_errors() {
        use std::io::{Error as IoError, ErrorKind as K};
        for k in [
            K::ConnectionReset,
            K::ConnectionAborted,
            K::BrokenPipe,
            K::ConnectionRefused,
            K::TimedOut,
            K::UnexpectedEof,
        ] {
            assert!(Error::Io(IoError::from(k)).is_transient(), "{k:?}");
        }
        assert!(!Error::Io(IoError::from(K::PermissionDenied)).is_transient());
        assert!(!Error::Io(IoError::from(K::InvalidData)).is_transient());
        assert!(Error::Closed.is_transient());
        assert!(!Error::Broken.is_transient());
        assert!(!Error::Auth("x".into()).is_transient());
        assert!(!Error::Config("x".into()).is_transient());
        assert!(!Error::Tls("x".into()).is_transient());
        assert!(!Error::Usage("x").is_transient());
        assert!(!Error::Value("x".into()).is_transient());
        assert!(!Error::Caller(IoError::from(K::BrokenPipe)).is_transient());
    }
}
