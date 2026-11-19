//! Checked reading of big-endian wire fields from a message body.

use crate::error::{ProtoError, Result};

/// A cursor over a message body. Every read is bounds-checked.
#[derive(Debug, Clone)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    ctx: &'static str,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8], ctx: &'static str) -> Self {
        Reader { data, pos: 0, ctx }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(ProtoError::Overflow)?;
        let s = self
            .data
            .get(self.pos..end)
            .ok_or(ProtoError::Truncated(self.ctx))?;
        self.pos = end;
        Ok(s)
    }

    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn i16(&mut self) -> Result<i16> {
        let b = self.take(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }

    pub(crate) fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub(crate) fn i32(&mut self) -> Result<i32> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A NUL-terminated byte string (without the NUL).
    pub(crate) fn cstr_bytes(&mut self) -> Result<&'a [u8]> {
        let rest = &self.data[self.pos..];
        let n = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or(ProtoError::Unterminated)?;
        self.pos += n + 1;
        Ok(&rest[..n])
    }

    pub(crate) fn cstr(&mut self) -> Result<&'a str> {
        std::str::from_utf8(self.cstr_bytes()?).map_err(|_| ProtoError::InvalidUtf8)
    }

    pub(crate) fn rest(&mut self) -> &'a [u8] {
        let s = &self.data[self.pos..];
        self.pos = self.data.len();
        s
    }

    pub(crate) fn finish(&self) -> Result<()> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(ProtoError::TrailingBytes(self.ctx))
        }
    }
}
