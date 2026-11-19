//! The binary `COPY` stream: an encoder for `COPY ... FROM STDIN (FORMAT binary)`.
//!
//! The stream is a fixed header, one tuple per row (a field count, then each field as a length and
//! its bytes, or a length of -1 for NULL) and a trailer. Each field uses the same binary encoding as
//! a bound parameter ([`Value::encode_into`]), so the type of a value has to be the type of its
//! column: the server does not convert (an `int4` column needs [`Value::I32`], not [`Value::I64`]).
//!
//! The encoder only builds bytes. Taking them out in pieces ([`BinaryCopyEncoder::take_into`]) lets
//! a caller send them as `CopyData` messages while the stream is still being made.

use crate::error::{ProtoError, Result};
use crate::types::Value;

/// `PGCOPY\n\377\r\n\0`, then the flags field (0) and the length of the header extension (0).
const HEADER: &[u8] = b"PGCOPY\n\xff\r\n\0\0\0\0\0\0\0\0\0";

/// Encoder for the binary COPY stream of a table with a fixed number of columns.
#[derive(Debug)]
pub struct BinaryCopyEncoder {
    buf: Vec<u8>,
    columns: usize,
    rows: u64,
    finished: bool,
}

impl BinaryCopyEncoder {
    /// A new stream for `columns` columns. The header is already in the buffer.
    pub fn new(columns: usize) -> Self {
        let mut buf = Vec::with_capacity(64 * 1024 + 1024);
        buf.extend_from_slice(HEADER);
        BinaryCopyEncoder {
            buf,
            columns,
            rows: 0,
            finished: false,
        }
    }

    /// Append one row. On an error nothing of the row stays in the buffer.
    ///
    /// The value count must equal the column count. [`Value::Unknown`] is refused: it is a
    /// text-format literal and has no binary form.
    pub fn row(&mut self, values: &[Value]) -> Result<()> {
        if self.finished {
            return Err(ProtoError::InvalidValue("COPY row after the trailer"));
        }
        if values.len() != self.columns {
            return Err(ProtoError::InvalidValue(
                "COPY row has a different number of values than the statement has columns",
            ));
        }
        let start = self.buf.len();
        let n = i16::try_from(values.len()).map_err(|_| ProtoError::Overflow)?;
        self.buf.extend_from_slice(&n.to_be_bytes());
        for v in values {
            if let Err(e) = self.field(v) {
                self.buf.truncate(start);
                return Err(e);
            }
        }
        self.rows += 1;
        Ok(())
    }

    fn field(&mut self, v: &Value) -> Result<()> {
        if matches!(v, Value::Unknown(_)) {
            return Err(ProtoError::InvalidValue(
                "an Unknown (text) value cannot go into a binary COPY",
            ));
        }
        let at = self.buf.len();
        self.buf.extend_from_slice(&[0; 4]);
        match v.encode_into(&mut self.buf)? {
            None => {
                self.buf.truncate(at);
                self.buf.extend_from_slice(&(-1i32).to_be_bytes());
            }
            Some(r) => {
                let len = i32::try_from(r.len()).map_err(|_| ProtoError::Overflow)?;
                self.buf[at..at + 4].copy_from_slice(&len.to_be_bytes());
            }
        }
        Ok(())
    }

    /// Bytes waiting to be sent.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// True when no bytes are waiting.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Rows added so far.
    pub fn rows(&self) -> u64 {
        self.rows
    }

    /// Move the waiting bytes to `out` (appending) and keep the buffer's capacity.
    pub fn take_into(&mut self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.buf);
        self.buf.clear();
    }

    /// The waiting bytes, to be sent and then dropped with [`BinaryCopyEncoder::clear`].
    pub fn pending(&self) -> &[u8] {
        &self.buf
    }

    /// Drop the waiting bytes after they were sent.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Add the trailer (a field count of -1). Rows cannot follow.
    pub fn finish(&mut self) {
        if !self.finished {
            self.buf.extend_from_slice(&(-1i16).to_be_bytes());
            self.finished = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stream_has_the_documented_layout() {
        let mut e = BinaryCopyEncoder::new(3);
        e.row(&[Value::I32(7), Value::Null, Value::Text("ab")]).unwrap();
        e.finish();
        let mut want: Vec<u8> = b"PGCOPY\n\xff\r\n\0".to_vec();
        want.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]); // flags, extension length
        want.extend_from_slice(&[0, 3]); // fields
        want.extend_from_slice(&[0, 0, 0, 4, 0, 0, 0, 7]); // int4 7
        want.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]); // NULL
        want.extend_from_slice(&[0, 0, 0, 2, b'a', b'b']); // text
        want.extend_from_slice(&[0xff, 0xff]); // trailer
        assert_eq!(e.pending(), &want[..]);
        assert_eq!(e.rows(), 1);
    }

    #[test]
    fn a_bad_row_leaves_nothing_behind() {
        let mut e = BinaryCopyEncoder::new(2);
        let before = e.len();
        assert!(e.row(&[Value::I32(1)]).is_err(), "wrong count");
        assert!(e.row(&[Value::I32(1), Value::Unknown("x")]).is_err(), "text literal");
        assert!(e.row(&[Value::I32(1), Value::Time(karez_chunk::Time(-1))]).is_err(), "out of range");
        assert_eq!(e.len(), before);
        assert_eq!(e.rows(), 0);
        e.row(&[Value::I32(1), Value::I32(2)]).unwrap();
        assert_eq!(e.rows(), 1);
    }

    #[test]
    fn nothing_follows_the_trailer_and_take_into_moves_the_bytes() {
        let mut e = BinaryCopyEncoder::new(1);
        e.row(&[Value::Bool(true)]).unwrap();
        e.finish();
        e.finish(); // twice is the same as once
        assert!(e.row(&[Value::Bool(false)]).is_err());
        let mut out = Vec::new();
        e.take_into(&mut out);
        assert!(e.is_empty());
        assert!(out.ends_with(&[0xff, 0xff]));
    }
}
