//! Incremental message framing: bytes in, `(tag, body)` frames out.
//!
//! The result does not depend on how the bytes were split between [`FrameReader::push`] calls.

use crate::error::{ProtoError, Result};

/// Default largest accepted message (tag byte excluded): 64 MiB.
pub const DEFAULT_MAX_MESSAGE: usize = 64 << 20;

/// One backend message: its type byte and its body (the length field is stripped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    /// The message type byte.
    pub tag: u8,
    /// The message body.
    pub body: &'a [u8],
}

/// Accumulates received bytes and yields complete frames.
#[derive(Debug)]
pub struct FrameReader {
    buf: Vec<u8>,
    pos: usize,
    max_message: usize,
}

impl Default for FrameReader {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_MESSAGE)
    }
}

impl FrameReader {
    /// A reader that rejects messages whose length field exceeds `max_message` bytes.
    pub fn new(max_message: usize) -> Self {
        FrameReader {
            buf: Vec::new(),
            pos: 0,
            max_message,
        }
    }

    /// Add received bytes. Frames returned earlier are no longer borrowed at this point.
    pub fn push(&mut self, bytes: &[u8]) {
        if self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        } else if self.pos > 0 && self.pos >= self.buf.len() / 2 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        self.buf.extend_from_slice(bytes);
    }

    /// Number of buffered bytes not yet returned as frames.
    pub fn buffered(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// The next complete frame, `Ok(None)` if more bytes are needed, or an error if the length
    /// field is invalid or over the limit (the connection must then be dropped).
    pub fn next_frame(&mut self) -> Result<Option<Frame<'_>>> {
        let avail = &self.buf[self.pos..];
        if avail.len() < 5 {
            return Ok(None);
        }
        let tag = avail[0];
        let len = i32::from_be_bytes([avail[1], avail[2], avail[3], avail[4]]);
        if len < 4 {
            return Err(ProtoError::InvalidLength(len));
        }
        let body_len = (len as usize) - 4;
        if body_len > self.max_message {
            return Err(ProtoError::MessageTooLarge {
                len: body_len,
                max: self.max_message,
            });
        }
        let total = 5usize.checked_add(body_len).ok_or(ProtoError::Overflow)?;
        if avail.len() < total {
            return Ok(None);
        }
        let start = self.pos + 5;
        self.pos += total;
        Ok(Some(Frame {
            tag,
            body: &self.buf[start..start + body_len],
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![tag];
        v.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn frames_split_anywhere_are_identical() {
        let mut stream = Vec::new();
        stream.extend(msg(b'Z', b"I"));
        stream.extend(msg(b'1', b""));
        stream.extend(msg(b'D', &[0, 1, 0, 0, 0, 2, b'h', b'i']));
        stream.extend(msg(b'C', b"SELECT 1\0"));
        let collect = |chunks: &[&[u8]]| {
            let mut r = FrameReader::default();
            let mut out = Vec::new();
            for c in chunks {
                r.push(c);
                while let Some(f) = r.next_frame().unwrap() {
                    out.push((f.tag, f.body.to_vec()));
                }
            }
            assert_eq!(r.buffered(), 0);
            out
        };
        let whole = collect(&[&stream]);
        assert_eq!(whole.len(), 4);
        for split in 0..=stream.len() {
            assert_eq!(
                collect(&[&stream[..split], &stream[split..]]),
                whole,
                "split {split}"
            );
        }
        for a in 0..stream.len() {
            for b in a..stream.len() {
                assert_eq!(
                    collect(&[&stream[..a], &stream[a..b], &stream[b..]]),
                    whole,
                    "splits {a} {b}"
                );
            }
        }
        let bytes: Vec<&[u8]> = stream.chunks(1).collect();
        assert_eq!(collect(&bytes), whole);
    }

    #[test]
    fn rejects_short_negative_and_oversized_lengths() {
        let mut r = FrameReader::new(100);
        r.push(&[b'X', 0, 0, 0, 3]);
        assert_eq!(r.next_frame(), Err(ProtoError::InvalidLength(3)));
        let mut r = FrameReader::new(100);
        r.push(&[b'X', 0xff, 0xff, 0xff, 0xff]);
        assert_eq!(r.next_frame(), Err(ProtoError::InvalidLength(-1)));
        let mut r = FrameReader::new(100);
        r.push(&[b'D', 0, 0, 0, 105]); // body 101 > 100, rejected before the body arrives
        assert!(matches!(
            r.next_frame(),
            Err(ProtoError::MessageTooLarge { len: 101, max: 100 })
        ));
        let mut r = FrameReader::new(100);
        r.push(&[b'D', 0x7f, 0xff, 0xff, 0xff]);
        assert!(matches!(
            r.next_frame(),
            Err(ProtoError::MessageTooLarge { .. })
        ));
    }

    #[test]
    fn exact_limit_is_accepted() {
        let mut r = FrameReader::new(3);
        r.push(&msg(b'D', &[1, 2, 3]));
        assert_eq!(r.next_frame().unwrap().unwrap().body, &[1, 2, 3]);
    }
}
