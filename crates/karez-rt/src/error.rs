//! Errors of the shared connection.

use std::fmt;

/// Everything that can go wrong on a [`crate::SharedConnection`].
#[derive(Debug)]
pub enum Error {
    /// An error from the PostgreSQL layer: a server error, a usage error, an unsupported type.
    Pg(karez_pg::Error),
    /// The submit queue is full, or a streaming result holds the connection, and the wait for
    /// room ran out. The command was not queued: retrying is safe.
    QueueFull,
    /// The caller's deadline passed before the command finished.
    Timeout,
    /// The connection went away. `retry_safe` is true if this command was never written to the
    /// socket, so it cannot have run; false if it was written and its outcome is unknown.
    Closed {
        /// Whether the command can be retried on another connection without risk of running twice.
        retry_safe: bool,
        /// What ended the connection.
        reason: String,
    },
    /// An earlier statement in the same pipeline failed, so this one did not run.
    Skipped,
    /// A collected result grew past the configured limit; the rest was discarded.
    ResultTooLarge {
        /// The limit in bytes.
        limit: usize,
    },
}

impl Error {
    /// True if the command is known not to have run, so repeating it elsewhere cannot run it twice.
    pub fn retry_safe(&self) -> bool {
        match self {
            Error::Closed { retry_safe, .. } => *retry_safe,
            Error::QueueFull | Error::Skipped => true,
            _ => false,
        }
    }

    /// The SQLSTATE if the server reported an error.
    pub fn sqlstate(&self) -> Option<&str> {
        match self {
            Error::Pg(e) => e.sqlstate(),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Pg(e) => write!(f, "{e}"),
            Error::QueueFull => write!(f, "the submit queue is full or the connection is leased"),
            Error::Timeout => write!(f, "the command did not finish before the deadline"),
            Error::Closed { retry_safe, reason } => write!(
                f,
                "connection lost ({reason}); the command {}",
                if *retry_safe {
                    "was never sent"
                } else {
                    "was sent and its outcome is unknown"
                }
            ),
            Error::Skipped => write!(f, "skipped: an earlier statement in the pipeline failed"),
            Error::ResultTooLarge { limit } => {
                write!(f, "the result is larger than the {limit} byte limit")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Pg(e) => Some(e),
            _ => None,
        }
    }
}

impl From<karez_pg::Error> for Error {
    fn from(e: karez_pg::Error) -> Self {
        Error::Pg(e)
    }
}

/// Result alias.
pub type Result<T> = std::result::Result<T, Error>;
