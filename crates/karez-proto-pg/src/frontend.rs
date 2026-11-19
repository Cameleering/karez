//! Encoders for the messages a client sends. Each appends to a caller-supplied buffer, so a
//! whole pipeline can be built and sent with one write.
//!
//! Every length is checked: a body that cannot be described by the wire's `Int32` is an error and
//! leaves the buffer as it was.

use crate::error::{ProtoError, Result};

/// The protocol version number for 3.0.
pub const PROTOCOL_3_0: i32 = 196_608;

struct Msg {
    start: usize,
    tagged: bool,
}

fn begin(buf: &mut Vec<u8>, tag: Option<u8>) -> Msg {
    let start = buf.len();
    if let Some(t) = tag {
        buf.push(t);
    }
    buf.extend_from_slice(&[0; 4]);
    Msg {
        start,
        tagged: tag.is_some(),
    }
}

impl Msg {
    fn finish(self, buf: &mut Vec<u8>) -> Result<()> {
        let len_pos = self.start + usize::from(self.tagged);
        let len = buf.len() - len_pos;
        match i32::try_from(len) {
            Ok(l) => {
                buf[len_pos..len_pos + 4].copy_from_slice(&l.to_be_bytes());
                Ok(())
            }
            Err(_) => {
                buf.truncate(self.start);
                Err(ProtoError::Overflow)
            }
        }
    }
}

fn cstr(buf: &mut Vec<u8>, s: &[u8]) -> Result<()> {
    if s.contains(&0) {
        return Err(ProtoError::InteriorNul);
    }
    buf.extend_from_slice(s);
    buf.push(0);
    Ok(())
}

fn count(buf: &mut Vec<u8>, n: usize) -> Result<()> {
    let n = u16::try_from(n).map_err(|_| ProtoError::Overflow)?;
    buf.extend_from_slice(&n.to_be_bytes());
    Ok(())
}

/// Run `f` between begin/finish; on failure the buffer is restored.
fn message(
    buf: &mut Vec<u8>,
    tag: Option<u8>,
    f: impl FnOnce(&mut Vec<u8>) -> Result<()>,
) -> Result<()> {
    let m = begin(buf, tag);
    let start = m.start;
    match f(buf).and_then(|()| m.finish(buf)) {
        Ok(()) => Ok(()),
        Err(e) => {
            buf.truncate(start);
            Err(e)
        }
    }
}

/// StartupMessage with the given parameters (`user` is required by the server).
pub fn startup(buf: &mut Vec<u8>, params: &[(&str, &str)]) -> Result<()> {
    message(buf, None, |b| {
        b.extend_from_slice(&PROTOCOL_3_0.to_be_bytes());
        for (k, v) in params {
            cstr(b, k.as_bytes())?;
            cstr(b, v.as_bytes())?;
        }
        b.push(0);
        Ok(())
    })
}

/// PasswordMessage carrying a cleartext password.
pub fn password(buf: &mut Vec<u8>, password: &[u8]) -> Result<()> {
    message(buf, Some(b'p'), |b| cstr(b, password))
}

/// SASLInitialResponse: the mechanism name and the client-first message.
pub fn sasl_initial_response(buf: &mut Vec<u8>, mechanism: &str, data: &[u8]) -> Result<()> {
    message(buf, Some(b'p'), |b| {
        cstr(b, mechanism.as_bytes())?;
        let n = i32::try_from(data.len()).map_err(|_| ProtoError::Overflow)?;
        b.extend_from_slice(&n.to_be_bytes());
        b.extend_from_slice(data);
        Ok(())
    })
}

/// SASLResponse: the client-final message.
pub fn sasl_response(buf: &mut Vec<u8>, data: &[u8]) -> Result<()> {
    message(buf, Some(b'p'), |b| {
        b.extend_from_slice(data);
        Ok(())
    })
}

/// Parse: create a prepared statement. `name` may be empty (the unnamed statement). An OID of 0
/// leaves that parameter's type to the server.
pub fn parse(buf: &mut Vec<u8>, name: &str, sql: &str, param_oids: &[u32]) -> Result<()> {
    message(buf, Some(b'P'), |b| {
        cstr(b, name.as_bytes())?;
        cstr(b, sql.as_bytes())?;
        count(b, param_oids.len())?;
        for oid in param_oids {
            b.extend_from_slice(&oid.to_be_bytes());
        }
        Ok(())
    })
}

/// Bind: bind parameter values (always sent as values, never spliced into SQL) to a statement,
/// creating a portal. `param_formats` has one entry per parameter (0 text, 1 binary);
/// `None` in `params` is SQL NULL. `result_formats` has one entry per result column, or is empty
/// for all-text.
pub fn bind(
    buf: &mut Vec<u8>,
    portal: &str,
    statement: &str,
    param_formats: &[i16],
    params: &[Option<&[u8]>],
    result_formats: &[i16],
) -> Result<()> {
    if param_formats.len() != params.len() {
        return Err(ProtoError::InvalidValue("one format per parameter"));
    }
    message(buf, Some(b'B'), |b| {
        cstr(b, portal.as_bytes())?;
        cstr(b, statement.as_bytes())?;
        count(b, param_formats.len())?;
        for f in param_formats {
            b.extend_from_slice(&f.to_be_bytes());
        }
        count(b, params.len())?;
        for p in params {
            match p {
                None => b.extend_from_slice(&(-1i32).to_be_bytes()),
                Some(v) => {
                    let n = i32::try_from(v.len()).map_err(|_| ProtoError::Overflow)?;
                    b.extend_from_slice(&n.to_be_bytes());
                    b.extend_from_slice(v);
                }
            }
        }
        count(b, result_formats.len())?;
        for f in result_formats {
            b.extend_from_slice(&f.to_be_bytes());
        }
        Ok(())
    })
}

fn describe_or_close(buf: &mut Vec<u8>, tag: u8, kind: u8, name: &str) -> Result<()> {
    message(buf, Some(tag), |b| {
        b.push(kind);
        cstr(b, name.as_bytes())
    })
}

/// Describe a prepared statement (answers with ParameterDescription and RowDescription/NoData).
pub fn describe_statement(buf: &mut Vec<u8>, name: &str) -> Result<()> {
    describe_or_close(buf, b'D', b'S', name)
}

/// Describe a portal.
pub fn describe_portal(buf: &mut Vec<u8>, name: &str) -> Result<()> {
    describe_or_close(buf, b'D', b'P', name)
}

/// Close a prepared statement.
pub fn close_statement(buf: &mut Vec<u8>, name: &str) -> Result<()> {
    describe_or_close(buf, b'C', b'S', name)
}

/// Close a portal.
pub fn close_portal(buf: &mut Vec<u8>, name: &str) -> Result<()> {
    describe_or_close(buf, b'C', b'P', name)
}

/// Execute a portal, returning at most `max_rows` rows (0 for no limit).
pub fn execute(buf: &mut Vec<u8>, portal: &str, max_rows: i32) -> Result<()> {
    if max_rows < 0 {
        return Err(ProtoError::InvalidValue("max_rows"));
    }
    message(buf, Some(b'E'), |b| {
        cstr(b, portal.as_bytes())?;
        b.extend_from_slice(&max_rows.to_be_bytes());
        Ok(())
    })
}

fn bare(buf: &mut Vec<u8>, tag: u8) {
    buf.push(tag);
    buf.extend_from_slice(&4i32.to_be_bytes());
}

/// Sync: end of an extended-query batch; the server answers with ReadyForQuery.
pub fn sync(buf: &mut Vec<u8>) {
    bare(buf, b'S');
}

/// Flush: ask the server to send pending output now.
pub fn flush(buf: &mut Vec<u8>) {
    bare(buf, b'H');
}

/// Terminate: close the session.
pub fn terminate(buf: &mut Vec<u8>) {
    bare(buf, b'X');
}

/// SSLRequest: ask the server to switch to TLS. The server answers with one byte, `S` or `N`.
pub fn ssl_request(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&8i32.to_be_bytes());
    buf.extend_from_slice(&80_877_103i32.to_be_bytes());
}

/// CopyData: a piece of the COPY stream sent to the server.
pub fn copy_data(buf: &mut Vec<u8>, data: &[u8]) -> Result<()> {
    message(buf, Some(b'd'), |b| {
        b.extend_from_slice(data);
        Ok(())
    })
}

/// CopyDone: the COPY stream is complete.
pub fn copy_done(buf: &mut Vec<u8>) {
    bare(buf, b'c');
}

/// CopyFail: abort the COPY with a message.
pub fn copy_fail(buf: &mut Vec<u8>, message_text: &str) -> Result<()> {
    message(buf, Some(b'f'), |b| cstr(b, message_text.as_bytes()))
}

/// CancelRequest, sent on a fresh connection in place of a startup message.
pub fn cancel_request(buf: &mut Vec<u8>, process_id: i32, secret: &[u8]) -> Result<()> {
    message(buf, None, |b| {
        b.extend_from_slice(&80_877_102i32.to_be_bytes());
        b.extend_from_slice(&process_id.to_be_bytes());
        b.extend_from_slice(secret);
        Ok(())
    })
}

/// Simple Query. Use only for fixed statements the library itself builds (`BEGIN`, `COMMIT`,
/// `SAVEPOINT "x"`); user data always goes through [`parse`] and [`bind`].
pub fn query(buf: &mut Vec<u8>, sql: &str) -> Result<()> {
    message(buf, Some(b'Q'), |b| cstr(b, sql.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_layout() {
        let mut b = Vec::new();
        startup(&mut b, &[("user", "bob"), ("database", "db")]).unwrap();
        let mut want = Vec::new();
        want.extend_from_slice(&0i32.to_be_bytes());
        want.extend_from_slice(&PROTOCOL_3_0.to_be_bytes());
        want.extend_from_slice(b"user\0bob\0database\0db\0\0");
        let n = want.len() as i32;
        want[..4].copy_from_slice(&n.to_be_bytes());
        assert_eq!(b, want);
    }

    #[test]
    fn sync_flush_terminate_are_five_bytes() {
        let mut b = Vec::new();
        sync(&mut b);
        flush(&mut b);
        terminate(&mut b);
        assert_eq!(b, [b'S', 0, 0, 0, 4, b'H', 0, 0, 0, 4, b'X', 0, 0, 0, 4]);
    }

    #[test]
    fn parse_bind_execute_layout() {
        let mut b = Vec::new();
        parse(&mut b, "s1", "SELECT $1", &[23]).unwrap();
        assert_eq!(
            b,
            [
                b'P',
                0,
                0,
                0,
                4 + 3 + 10 + 2 + 4,
                b's',
                b'1',
                0,
                b'S',
                b'E',
                b'L',
                b'E',
                b'C',
                b'T',
                b' ',
                b'$',
                b'1',
                0,
                0,
                1,
                0,
                0,
                0,
                23
            ]
        );
        let mut b = Vec::new();
        bind(
            &mut b,
            "",
            "s1",
            &[1, 1],
            &[Some(&[0, 0, 0, 7]), None],
            &[1],
        )
        .unwrap();
        let want: Vec<u8> = [
            &[b'B', 0, 0, 0, 0][..],
            b"\0s1\0",
            &[0, 2, 0, 1, 0, 1],
            &[0, 2],
            &[0, 0, 0, 4, 0, 0, 0, 7],
            &[0xff, 0xff, 0xff, 0xff],
            &[0, 1, 0, 1],
        ]
        .concat();
        let mut want = want;
        let n = (want.len() - 1) as i32;
        want[1..5].copy_from_slice(&n.to_be_bytes());
        assert_eq!(b, want);
        let mut b = Vec::new();
        execute(&mut b, "", 0).unwrap();
        assert_eq!(b, [b'E', 0, 0, 0, 9, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn nul_in_strings_is_rejected_and_buffer_restored() {
        let mut b = vec![1, 2, 3];
        assert_eq!(
            parse(&mut b, "", "SELECT 1\0; DROP", &[]),
            Err(ProtoError::InteriorNul)
        );
        assert_eq!(b, [1, 2, 3]);
        assert_eq!(query(&mut b, "a\0b"), Err(ProtoError::InteriorNul));
        assert_eq!(password(&mut b, b"p\0w"), Err(ProtoError::InteriorNul));
        assert_eq!(b, [1, 2, 3]);
    }

    #[test]
    fn counts_over_u16_are_rejected() {
        let oids = vec![0u32; 70_000];
        let mut b = Vec::new();
        assert_eq!(parse(&mut b, "", "x", &oids), Err(ProtoError::Overflow));
        assert!(b.is_empty());
    }

    #[test]
    fn bind_requires_one_format_per_parameter() {
        let mut b = Vec::new();
        assert!(bind(&mut b, "", "", &[1], &[None, None], &[]).is_err());
        assert!(execute(&mut b, "", -1).is_err());
    }

    #[test]
    fn sasl_messages() {
        let mut b = Vec::new();
        sasl_initial_response(&mut b, "SCRAM-SHA-256", b"n,,n=,r=x").unwrap();
        assert_eq!(b[0], b'p');
        assert_eq!(&b[5..19], b"SCRAM-SHA-256\0");
        assert_eq!(&b[19..23], &9i32.to_be_bytes());
        assert_eq!(&b[23..], b"n,,n=,r=x");
        let mut b = Vec::new();
        sasl_response(&mut b, b"abc").unwrap();
        assert_eq!(b, [b'p', 0, 0, 0, 7, b'a', b'b', b'c']);
    }
}
