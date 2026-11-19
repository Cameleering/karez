//! Binary `COPY ... FROM STDIN`: rows go in as typed values, with no text formatting on the client
//! and no text parsing on the server.
//!
//! ```ignore
//! let mut w = conn.copy_in_binary("COPY t (id, name) FROM STDIN (FORMAT binary)")?;
//! for (id, name) in rows {
//!     w.row(&[Value::I64(id), Value::Text(name)])?;
//! }
//! let n = w.finish()?;
//! ```
//!
//! A value's type has to be its column's type (`int4` takes [`Value::I32`], not [`Value::I64`]);
//! COPY does not convert, and the server refuses a wrong width. The number of values per row is
//! checked here against the number of columns the server announced.

use super::*;
use karez_proto_pg::BinaryCopyEncoder;

/// Send the stream when this many bytes are waiting.
const FLUSH_AT: usize = 64 * 1024;

/// An open binary `COPY ... FROM STDIN`. Finish it with [`CopyWriter::finish`]; dropping it
/// without that aborts the COPY (nothing is loaded) and leaves the connection usable.
#[derive(Debug)]
pub struct CopyWriter<'c> {
    conn: &'c mut Connection,
    enc: BinaryCopyEncoder,
    open: bool,
}

impl Connection {
    /// Start `COPY ... FROM STDIN (FORMAT binary)`. `sql` is sent as written, so build it only
    /// from trusted text. A statement that is not binary is refused (and its stream is cancelled).
    pub fn copy_in_binary(&mut self, sql: &str) -> Result<CopyWriter<'_>> {
        self.check_usable()?;
        let r = self.copy_in_begin(sql);
        let (binary, columns) = self.track(r)?;
        if !binary {
            // the server is waiting for data in the wrong format: end the stream and read its answer
            let r = self.copy_in_end(Some("karez: the statement is not FORMAT binary"));
            self.track(r)?;
            return Err(Error::Usage(
                "copy_in_binary needs COPY ... FROM STDIN (FORMAT binary)",
            ));
        }
        Ok(CopyWriter {
            conn: self,
            enc: BinaryCopyEncoder::new(usize::from(columns)),
            open: true,
        })
    }
}

impl CopyWriter<'_> {
    /// Add one row. Sends the stream in 64 KiB pieces as it fills. An error from the encoder
    /// (a wrong number of values, a value out of range) leaves the writer usable for the next row;
    /// an I/O error breaks the connection.
    pub fn row(&mut self, values: &[Value]) -> Result<()> {
        self.enc.row(values)?;
        if self.enc.len() >= FLUSH_AT {
            self.flush()?;
        }
        Ok(())
    }

    /// Rows added so far.
    pub fn rows(&self) -> u64 {
        self.enc.rows()
    }

    fn flush(&mut self) -> Result<()> {
        let r = self.conn.copy_in_send(self.enc.pending());
        self.enc.clear();
        if r.is_err() {
            self.open = false;
        }
        self.conn.track(r)
    }

    /// End the stream and return the number of rows the server loaded. A server error (a wrong
    /// type, a constraint) is returned as `Error::Server`, and nothing is loaded.
    pub fn finish(mut self) -> Result<u64> {
        self.enc.finish();
        self.flush()?;
        self.open = false;
        let r = self.conn.copy_in_end(None);
        let (rows, err) = self.conn.track(r)?;
        match err {
            Some(f) => Err(Error::Server(ServerError::new(f))),
            None => Ok(rows),
        }
    }

    /// Abort the COPY: the server loads nothing and the connection is usable again.
    pub fn abort(mut self) -> Result<()> {
        self.abort_inner()
    }

    fn abort_inner(&mut self) -> Result<()> {
        if !self.open {
            return Ok(());
        }
        self.open = false;
        let r = self.conn.copy_in_end(Some("aborted by the caller"));
        self.conn.track(r).map(|_| ())
    }
}

impl Drop for CopyWriter<'_> {
    fn drop(&mut self) {
        let _ = self.abort_inner();
    }
}
