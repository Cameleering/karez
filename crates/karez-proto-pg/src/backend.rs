//! Parsers for the messages a server sends.

use crate::error::{ProtoError, Result};
use crate::frame::Frame;
use crate::wire::Reader;
use std::fmt;

/// Transaction status carried by ReadyForQuery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxStatus {
    /// Idle, not in a transaction block (`I`).
    Idle,
    /// In a transaction block (`T`).
    InTransaction,
    /// In a failed transaction block; only ROLLBACK is accepted (`E`).
    Failed,
}

/// A five-character SQLSTATE code.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SqlState(String);

impl SqlState {
    /// Wrap a code.
    pub fn new(code: impl Into<String>) -> Self {
        SqlState(code.into())
    }

    /// The code, for example `23505`.
    pub fn code(&self) -> &str {
        &self.0
    }

    /// The two-character class, for example `23`.
    pub fn class(&self) -> &str {
        self.0.get(..2).unwrap_or("")
    }
}

impl fmt::Debug for SqlState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SqlState({})", self.0)
    }
}

impl fmt::Display for SqlState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The fields of an ErrorResponse or NoticeResponse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErrorFields {
    /// `S`: severity, possibly localised.
    pub severity: String,
    /// `V`: severity, never localised (PostgreSQL 9.6+).
    pub severity_nonlocalized: Option<String>,
    /// `C`: SQLSTATE code.
    pub code: String,
    /// `M`: the primary message.
    pub message: String,
    /// `D`: detail.
    pub detail: Option<String>,
    /// `H`: hint.
    pub hint: Option<String>,
    /// `P`: character position in the query.
    pub position: Option<String>,
    /// `W`: context.
    pub where_: Option<String>,
    /// `s`: schema name.
    pub schema: Option<String>,
    /// `t`: table name.
    pub table: Option<String>,
    /// `c`: column name.
    pub column: Option<String>,
    /// `d`: data type name.
    pub datatype: Option<String>,
    /// `n`: constraint name.
    pub constraint: Option<String>,
    /// Any other field, as `(type byte, value)`.
    pub other: Vec<(u8, String)>,
}

impl ErrorFields {
    /// The SQLSTATE as a typed value.
    pub fn sqlstate(&self) -> SqlState {
        SqlState::new(self.code.clone())
    }

    fn parse(body: &[u8], ctx: &'static str) -> Result<Self> {
        let mut r = Reader::new(body, ctx);
        let mut f = ErrorFields::default();
        loop {
            let t = r.u8()?;
            if t == 0 {
                break;
            }
            let v = r.cstr()?.to_owned();
            match t {
                b'S' => f.severity = v,
                b'V' => f.severity_nonlocalized = Some(v),
                b'C' => f.code = v,
                b'M' => f.message = v,
                b'D' => f.detail = Some(v),
                b'H' => f.hint = Some(v),
                b'P' => f.position = Some(v),
                b'W' => f.where_ = Some(v),
                b's' => f.schema = Some(v),
                b't' => f.table = Some(v),
                b'c' => f.column = Some(v),
                b'd' => f.datatype = Some(v),
                b'n' => f.constraint = Some(v),
                other => f.other.push((other, v)),
            }
        }
        r.finish()?;
        Ok(f)
    }
}

/// One column of a RowDescription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDescription {
    /// Column name.
    pub name: String,
    /// OID of the source table, or 0.
    pub table_oid: u32,
    /// Attribute number in the source table, or 0.
    pub column_attr: i16,
    /// OID of the column's data type.
    pub type_oid: u32,
    /// Type size (negative for variable width).
    pub type_size: i16,
    /// Type modifier.
    pub type_modifier: i32,
    /// Format code in use: 0 text, 1 binary.
    pub format: i16,
}

/// A DataRow whose framing has been validated: the column count is known and the body is
/// otherwise decoded lazily or by [`crate::types::ResultPlan::decode_row`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataRow<'a> {
    /// The message body (starts with the column count).
    pub body: &'a [u8],
    /// Number of columns in the row.
    pub columns: u16,
}

impl<'a> DataRow<'a> {
    /// Validate that a DataRow body is well formed (all lengths in range, nothing left over).
    pub fn parse(body: &'a [u8]) -> Result<Self> {
        let mut r = Reader::new(body, "DataRow");
        let columns = r.u16()?;
        for _ in 0..columns {
            let len = r.i32()?;
            match len {
                -1 => {}
                n if n >= 0 => {
                    r.take(n as usize)?;
                }
                _ => return Err(ProtoError::InvalidValue("DataRow field length")),
            }
        }
        r.finish()?;
        Ok(DataRow { body, columns })
    }

    /// The field values: `None` for NULL.
    pub fn fields(&self) -> impl Iterator<Item = Result<Option<&'a [u8]>>> + 'a {
        let mut r = Reader::new(self.body, "DataRow");
        let n = r.u16();
        let mut left = n.as_ref().map(|n| *n).unwrap_or(0);
        let mut pending_err = n.err();
        std::iter::from_fn(move || {
            if let Some(e) = pending_err.take() {
                return Some(Err(e));
            }
            if left == 0 {
                return None;
            }
            left -= 1;
            Some(r.i32().and_then(|len| match len {
                -1 => Ok(None),
                n if n >= 0 => r.take(n as usize).map(Some),
                _ => Err(ProtoError::InvalidValue("DataRow field length")),
            }))
        })
    }
}

/// A message from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendMessage<'a> {
    /// Authentication succeeded.
    AuthenticationOk,
    /// The server wants a cleartext password.
    AuthenticationCleartextPassword,
    /// The server wants an MD5 password (not supported by karez; reported so it can be refused).
    AuthenticationMd5Password {
        /// The 4-byte salt.
        salt: [u8; 4],
    },
    /// The server offers SASL mechanisms.
    AuthenticationSasl {
        /// Mechanism names in server preference order.
        mechanisms: Vec<String>,
    },
    /// SASL server-first data.
    AuthenticationSaslContinue(&'a [u8]),
    /// SASL server-final data.
    AuthenticationSaslFinal(&'a [u8]),
    /// A run-time parameter's value.
    ParameterStatus {
        /// Parameter name.
        name: String,
        /// Parameter value.
        value: String,
    },
    /// Cancellation key data.
    BackendKeyData {
        /// Backend process id.
        process_id: i32,
        /// Secret key bytes (4 bytes in protocol 3.0).
        secret: Vec<u8>,
    },
    /// The server is ready for a new query cycle.
    ReadyForQuery(TxStatus),
    /// An error; the current batch is aborted up to the next Sync.
    ErrorResponse(ErrorFields),
    /// A notice or warning.
    NoticeResponse(ErrorFields),
    /// Parse finished.
    ParseComplete,
    /// Bind finished.
    BindComplete,
    /// Close finished.
    CloseComplete,
    /// The statement or portal returns no rows.
    NoData,
    /// Parameter types of a statement.
    ParameterDescription(Vec<u32>),
    /// Columns of a result.
    RowDescription(Vec<FieldDescription>),
    /// One row.
    DataRow(DataRow<'a>),
    /// A statement finished, with its command tag (`INSERT 0 1`, `SELECT 5`, ...).
    CommandComplete(String),
    /// An empty query string was executed.
    EmptyQueryResponse,
    /// The row limit of Execute was reached.
    PortalSuspended,
    /// LISTEN/NOTIFY payload.
    NotificationResponse {
        /// Notifying backend's process id.
        process_id: i32,
        /// Channel name.
        channel: String,
        /// Payload.
        payload: String,
    },
    /// The server does not support the requested protocol minor version or options.
    NegotiateProtocolVersion {
        /// Newest minor version the server supports.
        newest_minor: i32,
        /// Options the server did not recognise.
        unrecognized: Vec<String>,
    },
    /// A COPY message (`G`, `H`, `W`, `d`, `c`); COPY is not implemented in this version.
    Copy(u8),
}

fn auth_message<'a>(body: &'a [u8]) -> Result<BackendMessage<'a>> {
    let mut r = Reader::new(body, "Authentication");
    let code = r.i32()?;
    let msg = match code {
        0 => BackendMessage::AuthenticationOk,
        3 => BackendMessage::AuthenticationCleartextPassword,
        5 => {
            let s = r.take(4)?;
            BackendMessage::AuthenticationMd5Password {
                salt: [s[0], s[1], s[2], s[3]],
            }
        }
        10 => {
            let mut mechanisms = Vec::new();
            loop {
                let m = r.cstr_bytes()?;
                if m.is_empty() {
                    break;
                }
                mechanisms.push(
                    std::str::from_utf8(m)
                        .map_err(|_| ProtoError::InvalidUtf8)?
                        .to_owned(),
                );
            }
            BackendMessage::AuthenticationSasl { mechanisms }
        }
        11 => BackendMessage::AuthenticationSaslContinue(r.rest()),
        12 => BackendMessage::AuthenticationSaslFinal(r.rest()),
        other => return Err(ProtoError::UnsupportedAuth(other)),
    };
    r.finish()?;
    Ok(msg)
}

impl<'a> BackendMessage<'a> {
    /// Parse one frame. Unknown message types and malformed bodies are errors.
    pub fn parse(frame: Frame<'a>) -> Result<Self> {
        let body = frame.body;
        Ok(match frame.tag {
            b'R' => return auth_message(body),
            b'S' => {
                let mut r = Reader::new(body, "ParameterStatus");
                let name = r.cstr()?.to_owned();
                let value = r.cstr()?.to_owned();
                r.finish()?;
                BackendMessage::ParameterStatus { name, value }
            }
            b'K' => {
                let mut r = Reader::new(body, "BackendKeyData");
                let process_id = r.i32()?;
                let secret = r.rest().to_vec();
                if secret.len() < 4 || secret.len() > 256 {
                    return Err(ProtoError::InvalidValue("BackendKeyData secret length"));
                }
                BackendMessage::BackendKeyData { process_id, secret }
            }
            b'Z' => {
                let mut r = Reader::new(body, "ReadyForQuery");
                let s = match r.u8()? {
                    b'I' => TxStatus::Idle,
                    b'T' => TxStatus::InTransaction,
                    b'E' => TxStatus::Failed,
                    _ => return Err(ProtoError::InvalidValue("ReadyForQuery status")),
                };
                r.finish()?;
                BackendMessage::ReadyForQuery(s)
            }
            b'E' => BackendMessage::ErrorResponse(ErrorFields::parse(body, "ErrorResponse")?),
            b'N' => BackendMessage::NoticeResponse(ErrorFields::parse(body, "NoticeResponse")?),
            b'1' | b'2' | b'3' | b'n' | b'I' | b's' => {
                if !body.is_empty() {
                    return Err(ProtoError::TrailingBytes("empty message"));
                }
                match frame.tag {
                    b'1' => BackendMessage::ParseComplete,
                    b'2' => BackendMessage::BindComplete,
                    b'3' => BackendMessage::CloseComplete,
                    b'n' => BackendMessage::NoData,
                    b'I' => BackendMessage::EmptyQueryResponse,
                    _ => BackendMessage::PortalSuspended,
                }
            }
            b't' => {
                let mut r = Reader::new(body, "ParameterDescription");
                let n = r.u16()?;
                let mut oids = Vec::with_capacity(usize::from(n).min(body.len() / 4));
                for _ in 0..n {
                    oids.push(r.u32()?);
                }
                r.finish()?;
                BackendMessage::ParameterDescription(oids)
            }
            b'T' => {
                let mut r = Reader::new(body, "RowDescription");
                let n = r.u16()?;
                let mut fields = Vec::with_capacity(usize::from(n).min(body.len() / 18));
                for _ in 0..n {
                    fields.push(FieldDescription {
                        name: r.cstr()?.to_owned(),
                        table_oid: r.u32()?,
                        column_attr: r.i16()?,
                        type_oid: r.u32()?,
                        type_size: r.i16()?,
                        type_modifier: r.i32()?,
                        format: r.i16()?,
                    });
                }
                r.finish()?;
                BackendMessage::RowDescription(fields)
            }
            b'D' => BackendMessage::DataRow(DataRow::parse(body)?),
            b'C' => {
                let mut r = Reader::new(body, "CommandComplete");
                let tag = r.cstr()?.to_owned();
                r.finish()?;
                BackendMessage::CommandComplete(tag)
            }
            b'A' => {
                let mut r = Reader::new(body, "NotificationResponse");
                let process_id = r.i32()?;
                let channel = r.cstr()?.to_owned();
                let payload = r.cstr()?.to_owned();
                r.finish()?;
                BackendMessage::NotificationResponse {
                    process_id,
                    channel,
                    payload,
                }
            }
            b'v' => {
                let mut r = Reader::new(body, "NegotiateProtocolVersion");
                let newest_minor = r.i32()?;
                let n = r.i32()?;
                if n < 0 {
                    return Err(ProtoError::InvalidValue("option count"));
                }
                let mut unrecognized = Vec::new();
                for _ in 0..n {
                    unrecognized.push(r.cstr()?.to_owned());
                }
                r.finish()?;
                BackendMessage::NegotiateProtocolVersion {
                    newest_minor,
                    unrecognized,
                }
            }
            t @ (b'G' | b'H' | b'W' | b'd' | b'c') => BackendMessage::Copy(t),
            other => return Err(ProtoError::UnknownMessage(other)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(tag: u8, body: &[u8]) -> Result<BackendMessage<'_>> {
        BackendMessage::parse(Frame { tag, body })
    }

    #[test]
    fn error_response_fields() {
        let body =
            b"SERROR\0VERROR\0C23505\0Mduplicate key\0Dkey (id)=(1) exists\0nusers_pkey\0Zxx\0\0";
        match parse(b'E', body).unwrap() {
            BackendMessage::ErrorResponse(e) => {
                assert_eq!(e.severity, "ERROR");
                assert_eq!(e.severity_nonlocalized.as_deref(), Some("ERROR"));
                assert_eq!(e.sqlstate().code(), "23505");
                assert_eq!(e.sqlstate().class(), "23");
                assert_eq!(e.message, "duplicate key");
                assert_eq!(e.detail.as_deref(), Some("key (id)=(1) exists"));
                assert_eq!(e.constraint.as_deref(), Some("users_pkey"));
                assert_eq!(e.other, vec![(b'Z', "xx".to_string())]);
            }
            other => panic!("{other:?}"),
        }
        assert!(parse(b'E', b"SERROR\0").is_err()); // no terminator
        assert!(parse(b'E', b"SERROR\0\0extra").is_err());
    }

    #[test]
    fn authentication_variants() {
        assert_eq!(
            parse(b'R', &[0, 0, 0, 0]).unwrap(),
            BackendMessage::AuthenticationOk
        );
        assert_eq!(
            parse(b'R', &[0, 0, 0, 3]).unwrap(),
            BackendMessage::AuthenticationCleartextPassword
        );
        assert_eq!(
            parse(b'R', &[0, 0, 0, 5, 1, 2, 3, 4]).unwrap(),
            BackendMessage::AuthenticationMd5Password { salt: [1, 2, 3, 4] }
        );
        assert_eq!(
            parse(b'R', b"\0\0\0\x0aSCRAM-SHA-256\0SCRAM-SHA-256-PLUS\0\0").unwrap(),
            BackendMessage::AuthenticationSasl {
                mechanisms: vec!["SCRAM-SHA-256".into(), "SCRAM-SHA-256-PLUS".into()]
            }
        );
        assert_eq!(
            parse(b'R', b"\0\0\0\x0br=abc").unwrap(),
            BackendMessage::AuthenticationSaslContinue(b"r=abc")
        );
        assert_eq!(
            parse(b'R', &[0, 0, 0, 7]),
            Err(ProtoError::UnsupportedAuth(7))
        );
        assert!(parse(b'R', &[0, 0, 0, 0, 9]).is_err());
        assert!(parse(b'R', &[0, 0]).is_err());
    }

    #[test]
    fn row_description_and_data_row() {
        let mut body = vec![0, 1];
        body.extend_from_slice(b"id\0");
        body.extend_from_slice(&[
            0, 0, 0, 5, 0, 1, 0, 0, 0, 23, 0, 4, 0xff, 0xff, 0xff, 0xff, 0, 1,
        ]);
        match parse(b'T', &body).unwrap() {
            BackendMessage::RowDescription(f) => {
                assert_eq!(f[0].name, "id");
                assert_eq!(f[0].type_oid, 23);
                assert_eq!(f[0].type_modifier, -1);
                assert_eq!(f[0].format, 1);
            }
            other => panic!("{other:?}"),
        }
        let row = [0, 2, 0, 0, 0, 2, b'h', b'i', 0xff, 0xff, 0xff, 0xff];
        match parse(b'D', &row).unwrap() {
            BackendMessage::DataRow(d) => {
                assert_eq!(d.columns, 2);
                let v: Vec<_> = d.fields().map(|f| f.unwrap()).collect();
                assert_eq!(v, vec![Some(&b"hi"[..]), None]);
            }
            other => panic!("{other:?}"),
        }
        // Length past the end, negative length other than -1, trailing bytes, short count.
        assert!(parse(b'D', &[0, 1, 0, 0, 0, 9, 1]).is_err());
        assert!(parse(b'D', &[0, 1, 0xff, 0xff, 0xff, 0xfe]).is_err());
        assert!(parse(b'D', &[0, 1, 0, 0, 0, 0, 7]).is_err());
        assert!(parse(b'D', &[0, 2, 0, 0, 0, 0]).is_err());
        assert!(parse(b'D', &[0]).is_err());
        assert!(parse(b'D', &[0, 1, 0x7f, 0xff, 0xff, 0xff]).is_err());
    }

    #[test]
    fn small_messages() {
        assert_eq!(
            parse(b'Z', b"I").unwrap(),
            BackendMessage::ReadyForQuery(TxStatus::Idle)
        );
        assert_eq!(
            parse(b'Z', b"T").unwrap(),
            BackendMessage::ReadyForQuery(TxStatus::InTransaction)
        );
        assert_eq!(
            parse(b'Z', b"E").unwrap(),
            BackendMessage::ReadyForQuery(TxStatus::Failed)
        );
        assert!(parse(b'Z', b"X").is_err());
        assert_eq!(parse(b'1', b"").unwrap(), BackendMessage::ParseComplete);
        assert!(parse(b'1', b"x").is_err());
        assert_eq!(
            parse(b'C', b"INSERT 0 1\0").unwrap(),
            BackendMessage::CommandComplete("INSERT 0 1".into())
        );
        assert_eq!(
            parse(b'S', b"server_version\x0016.1\0").unwrap(),
            BackendMessage::ParameterStatus {
                name: "server_version".into(),
                value: "16.1".into()
            }
        );
        assert_eq!(
            parse(b'K', &[0, 0, 0, 9, 1, 2, 3, 4]).unwrap(),
            BackendMessage::BackendKeyData {
                process_id: 9,
                secret: vec![1, 2, 3, 4]
            }
        );
        assert!(parse(b'K', &[0, 0, 0, 9, 1]).is_err());
        assert_eq!(parse(b'd', b"zz").unwrap(), BackendMessage::Copy(b'd'));
        assert_eq!(parse(b'?', b""), Err(ProtoError::UnknownMessage(b'?')));
        assert_eq!(
            parse(b't', &[0, 2, 0, 0, 0, 23, 0, 0, 0, 25]).unwrap(),
            BackendMessage::ParameterDescription(vec![23, 25])
        );
        assert!(parse(b't', &[0xff, 0xff]).is_err()); // count with no data must not allocate wildly
    }
}
