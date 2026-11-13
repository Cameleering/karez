//! SCRAM-SHA-256 client (RFC 5802, RFC 7677) without channel binding, as PostgreSQL uses it.
//!
//! Limits: the password is used as given (no SASLprep normalisation), which is identical for
//! ASCII passwords. A non-ASCII password may fail against a server that normalised it.

use crate::base64;
use crate::error::{ProtoError, Result};
use crate::sha256::{hmac_sha256, pbkdf2_hmac_sha256, sha256};
use std::fmt;

/// Highest PBKDF2 iteration count accepted from a server (PostgreSQL's default is 4096).
pub const MAX_ITERATIONS: u32 = 1_000_000;
const MAX_FIELD: usize = 1024;

/// A secret byte string: `Debug` prints nothing, and the bytes are overwritten on drop
/// (best effort; safe Rust cannot guarantee the compiler keeps the writes).
#[derive(Clone)]
pub struct Secret(Vec<u8>);

impl Secret {
    /// Wrap bytes.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Secret(bytes.into())
    }

    /// The secret bytes.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&self.0);
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for (x, y) in a.iter().zip(b) {
        d |= x ^ y;
    }
    d == 0
}

/// A random client nonce (24 base64 characters) from std's OS-seeded `RandomState`.
pub fn generate_nonce() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut raw = [0u8; 18];
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    for (i, chunk) in raw.chunks_mut(8).enumerate() {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(t ^ (i as u64));
        let v = h.finish().to_le_bytes();
        chunk.copy_from_slice(&v[..chunk.len()]);
    }
    base64::encode(&raw)
}

fn escape_user(user: &str) -> String {
    user.replace('=', "=3D").replace(',', "=2C")
}

/// The client side of one SCRAM-SHA-256 exchange.
#[derive(Debug)]
pub struct ScramClient {
    password: Secret,
    client_first_bare: String,
    nonce: String,
    expected_server_signature: Option<[u8; 32]>,
}

impl ScramClient {
    /// Start an exchange. PostgreSQL ignores the SCRAM user name and uses the startup user, so
    /// pass an empty string there. `nonce` must be unpredictable and free of commas
    /// ([`generate_nonce`] qualifies).
    pub fn new(username: &str, password: &str, nonce: String) -> Self {
        let client_first_bare = format!("n={},r={}", escape_user(username), nonce);
        ScramClient {
            password: Secret::new(password.as_bytes()),
            client_first_bare,
            nonce,
            expected_server_signature: None,
        }
    }

    /// The client-first message, including the GS2 header `n,,` (no channel binding).
    pub fn client_first(&self) -> String {
        format!("n,,{}", self.client_first_bare)
    }

    /// Consume the server-first message and produce the client-final message.
    pub fn client_final(&mut self, server_first: &str) -> Result<String> {
        let mut r = None;
        let mut s = None;
        let mut i = None;
        for (idx, part) in server_first.split(',').enumerate() {
            let (k, v) = part
                .split_once('=')
                .ok_or(ProtoError::Scram("malformed server-first"))?;
            match (idx, k) {
                (0, "r") => r = Some(v),
                (1, "s") => s = Some(v),
                (2, "i") => i = Some(v),
                (_, "m") => return Err(ProtoError::Scram("mandatory extension not supported")),
                _ => {}
            }
        }
        let (r, s, i) = match (r, s, i) {
            (Some(r), Some(s), Some(i)) => (r, s, i),
            _ => return Err(ProtoError::Scram("server-first missing r, s or i")),
        };
        if r.len() > MAX_FIELD || s.len() > MAX_FIELD {
            return Err(ProtoError::Scram("server-first field too long"));
        }
        if !r.starts_with(&self.nonce) || r.len() == self.nonce.len() {
            return Err(ProtoError::Scram(
                "server nonce does not extend client nonce",
            ));
        }
        let salt = base64::decode(s).ok_or(ProtoError::Scram("salt is not base64"))?;
        let iterations: u32 = i
            .parse()
            .map_err(|_| ProtoError::Scram("iteration count is not a number"))?;
        if iterations == 0 || iterations > MAX_ITERATIONS {
            return Err(ProtoError::Scram("iteration count out of range"));
        }

        let mut salted = [0u8; 32];
        pbkdf2_hmac_sha256(self.password.expose(), &salt, iterations, &mut salted);
        let client_key = hmac_sha256(&salted, b"Client Key");
        let stored_key = sha256(&client_key);
        let client_final_no_proof = format!("c=biws,r={r}");
        let auth_message = format!(
            "{},{},{}",
            self.client_first_bare, server_first, client_final_no_proof
        );
        let client_sig = hmac_sha256(&stored_key, auth_message.as_bytes());
        let mut proof = client_key;
        for (p, c) in proof.iter_mut().zip(client_sig) {
            *p ^= c;
        }
        let server_key = hmac_sha256(&salted, b"Server Key");
        self.expected_server_signature = Some(hmac_sha256(&server_key, auth_message.as_bytes()));
        Ok(format!(
            "{client_final_no_proof},p={}",
            base64::encode(&proof)
        ))
    }

    /// Check the server-final message (`v=<signature>`), proving the server knew the password.
    pub fn verify_server_final(&self, server_final: &str) -> Result<()> {
        let expected = self
            .expected_server_signature
            .ok_or(ProtoError::Scram("server-final before client-final"))?;
        if let Some(e) = server_final.strip_prefix("e=") {
            let _ = e;
            return Err(ProtoError::Scram("server reported an error"));
        }
        let v = server_final
            .split(',')
            .next()
            .and_then(|p| p.strip_prefix("v="))
            .ok_or(ProtoError::Scram("server-final has no verifier"))?;
        let got = base64::decode(v).ok_or(ProtoError::Scram("verifier is not base64"))?;
        if ct_eq(&got, &expected) {
            Ok(())
        } else {
            Err(ProtoError::Scram("server signature mismatch"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 7677 section 3.
    const NONCE: &str = "rOprNGfwEbeRWgbNEkqO";
    const SERVER_FIRST: &str =
        "r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096";

    #[test]
    fn rfc7677_exchange() {
        let mut c = ScramClient::new("user", "pencil", NONCE.to_string());
        assert_eq!(c.client_first(), "n,,n=user,r=rOprNGfwEbeRWgbNEkqO");
        let fin = c.client_final(SERVER_FIRST).unwrap();
        assert_eq!(
            fin,
            "c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,\
             p=dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ="
        );
        c.verify_server_final("v=6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=")
            .unwrap();
        assert!(c
            .verify_server_final("v=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
            .is_err());
    }

    #[test]
    fn rejects_bad_server_first() {
        let new = || ScramClient::new("", "pw", "abc".to_string());
        assert!(new().client_final("r=abc,s=AAAA,i=4096").is_err()); // nonce not extended
        assert!(new().client_final("r=xyz123,s=AAAA,i=4096").is_err()); // different nonce
        assert!(new().client_final("r=abc123,s=AAAA,i=0").is_err());
        assert!(new().client_final("r=abc123,s=AAAA,i=99999999").is_err());
        assert!(new().client_final("r=abc123,s=!!!!,i=4096").is_err());
        assert!(new().client_final("r=abc123,i=4096").is_err());
        assert!(new().client_final("garbage").is_err());
        assert!(new().verify_server_final("v=AAAA").is_err()); // before client-final
    }

    #[test]
    fn debug_never_shows_the_password() {
        let c = ScramClient::new("u", "hunter2", "n".to_string());
        assert!(!format!("{c:?}").contains("hunter2"));
    }

    #[test]
    fn nonces_differ_and_are_comma_free() {
        let a = generate_nonce();
        let b = generate_nonce();
        assert_ne!(a, b);
        assert!(!a.contains(','));
        assert_eq!(a.len(), 24);
    }
}
