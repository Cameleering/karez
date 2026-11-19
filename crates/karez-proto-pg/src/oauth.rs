//! The client side of SASL `OAUTHBEARER` (RFC 7628) as PostgreSQL 18 uses it.
//!
//! The client sends one message, `n,,` then `auth=Bearer <token>` between 0x01 separators. A server
//! that accepts the token answers `AuthenticationOk`. One that does not answers with a SASL
//! continue message holding a JSON error (`status`, and the `scope` and `openid-configuration` a
//! client needs to get a token); the client must reply with a single 0x01 byte, after which the
//! server ends the exchange with an error. A client that has no token yet sends an empty `auth=`
//! value to ask for exactly that JSON ("discovery").
//!
//! This module only builds and reads these messages. Getting a token (a device flow, a cached
//! token, a service account) is the caller's business: it is a callback in `karez-pg`.

use crate::error::{ProtoError, Result};

/// The mechanism name in the server's offer and in the initial response.
pub const MECHANISM: &str = "OAUTHBEARER";

/// What the server's JSON error said. Fields it left out are `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OAuthChallenge {
    /// The error code, for example `invalid_token`, or `invalid_request` for a discovery request.
    pub status: Option<String>,
    /// The scope the token needs.
    pub scope: Option<String>,
    /// The OpenID provider metadata URL the token must come from.
    pub openid_configuration: Option<String>,
}

/// True if `token` is a bearer token in RFC 6750's `b64token` syntax: one or more of
/// `A-Z a-z 0-9 - . _ ~ + /` and then any number of `=`. Anything else, in particular a control
/// character such as the 0x01 separator, could change the structure of the message.
pub fn valid_token(token: &str) -> bool {
    let body = token.trim_end_matches('=');
    !body.is_empty()
        && body.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'+' | b'/')
        })
}

/// The initial client response for `token`; an empty token asks the server for the discovery
/// error instead.
pub fn initial_response(token: &str) -> Result<Vec<u8>> {
    if !token.is_empty() && !valid_token(token) {
        return Err(ProtoError::InvalidValue(
            "the OAuth bearer token is not in the b64token syntax",
        ));
    }
    let mut m = Vec::with_capacity(token.len() + 20);
    m.extend_from_slice(b"n,,\x01auth=");
    if !token.is_empty() {
        m.extend_from_slice(b"Bearer ");
        m.extend_from_slice(token.as_bytes());
    }
    m.extend_from_slice(b"\x01\x01");
    Ok(m)
}

/// The reply to a JSON error: a single 0x01, which the RFC requires before the server may fail
/// the exchange.
pub const ERROR_ACK: &[u8] = b"\x01";

/// Read the JSON error. Unknown fields are ignored, and a document that is not JSON yields an
/// empty challenge rather than an error: the server is about to fail the exchange anyway.
pub fn parse_challenge(json: &[u8]) -> OAuthChallenge {
    let Ok(text) = std::str::from_utf8(json) else {
        return OAuthChallenge::default();
    };
    OAuthChallenge {
        status: string_field(text, "status"),
        scope: string_field(text, "scope"),
        openid_configuration: string_field(text, "openid-configuration"),
    }
}

/// The string value of a top-level key of a flat JSON object, with escapes decoded.
fn string_field(text: &str, key: &str) -> Option<String> {
    let mut chars = text.char_indices().peekable();
    let mut depth = 0i32;
    while let Some((i, c)) = chars.next() {
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            '"' => {
                let (k, end) = read_string(text, i + 1)?;
                // skip to after the closing quote
                while chars.peek().is_some_and(|(j, _)| *j < end) {
                    chars.next();
                }
                let rest = text[end..].trim_start();
                if depth == 1 && k == key {
                    if let Some(v) = rest.strip_prefix(':') {
                        let v = v.trim_start();
                        if v.starts_with('"') {
                            let start = text.len() - v.len() + 1;
                            return read_string(text, start).map(|(s, _)| s);
                        }
                        return None;
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Read a JSON string body starting just after its opening quote. Returns the decoded text and the
/// index just after the closing quote.
fn read_string(text: &str, start: usize) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut it = text[start..].char_indices();
    while let Some((i, c)) = it.next() {
        match c {
            '"' => return Some((out, start + i + 1)),
            '\\' => match it.next()?.1 {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let mut code = 0u32;
                    for _ in 0..4 {
                        code = code * 16 + it.next()?.1.to_digit(16)?;
                    }
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_initial_response_has_the_rfc_7628_shape() {
        assert_eq!(
            initial_response("abc.DEF-123_~+/==").unwrap(),
            b"n,,\x01auth=Bearer abc.DEF-123_~+/==\x01\x01"
        );
        // a discovery request carries no token
        assert_eq!(initial_response("").unwrap(), b"n,,\x01auth=\x01\x01");
    }

    #[test]
    fn a_token_that_could_change_the_message_is_refused() {
        for bad in [
            "a b",
            "a\x01b",
            "a\x01\x01n,,",
            "tok\nen",
            "é",
            "=",
            "a,b",
            "a\"b",
            "a;b",
        ] {
            assert!(initial_response(bad).is_err(), "{bad:?}");
            assert!(!valid_token(bad), "{bad:?}");
        }
        assert!(valid_token("eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln"));
    }

    #[test]
    fn the_json_error_is_read_with_its_escapes_and_nesting() {
        let j = br#"{"status":"invalid_token","scope":"openid email","openid-configuration":"https://idp.example/.well-known/openid-configuration"}"#;
        let c = parse_challenge(j);
        assert_eq!(c.status.as_deref(), Some("invalid_token"));
        assert_eq!(c.scope.as_deref(), Some("openid email"));
        assert_eq!(
            c.openid_configuration.as_deref(),
            Some("https://idp.example/.well-known/openid-configuration")
        );
        // whitespace, escapes, an unknown field, a nested object that repeats a key, other types
        let j = br#" { "extra" : {"status":"nested"}, "n": 5, "scope" : "a\"b\u00e9\/c", "status":"x" , "list":[1,"scope"] } "#;
        let c = parse_challenge(j);
        assert_eq!(c.status.as_deref(), Some("x"), "only top-level keys count");
        assert_eq!(c.scope.as_deref(), Some("a\"b\u{e9}/c"));
        assert_eq!(c.openid_configuration, None);
        // a value that is not a string, garbage, truncation and invalid UTF-8 never panic
        assert_eq!(parse_challenge(br#"{"status":5}"#).status, None);
        for g in [
            &b""[..],
            b"not json",
            b"{",
            b"{\"status\":\"unterminated",
            b"\xff\xfe",
            b"{\"scope\":\"\\u12",
        ] {
            let _ = parse_challenge(g);
        }
    }
}
