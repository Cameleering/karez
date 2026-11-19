//! SCRAM-SHA-256 client (RFC 5802, RFC 7677), with or without channel binding, as PostgreSQL uses
//! it. Channel binding is `tls-server-end-point` (RFC 5929) only: the hash of the server's
//! certificate, which is the one binding PostgreSQL implements (mechanism `SCRAM-SHA-256-PLUS`).
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
    // The operating system's generator where a device file exists (Unix); otherwise the
    // OS-seeded `RandomState` below, which std seeds from the OS on every platform.
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        if f.read_exact(&mut raw).is_ok() {
            return base64::encode(&raw);
        }
    }
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

/// What the client tells the server about channel binding in the GS2 header (RFC 5802 section 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelBinding {
    /// `n,,`: the client does not support channel binding, or it is switched off, or the
    /// connection is not TLS.
    None,
    /// `y,,`: the client supports channel binding but the server did not offer it. A server that
    /// does support it then refuses the exchange, which detects a downgrade by a man in the middle.
    Supported,
    /// `p=tls-server-end-point,,`: bound to the server certificate; the bytes are the certificate
    /// hash (see [`cert_hash_algorithm`]) and are sent, hidden in `c=`, in the client-final message.
    TlsServerEndPoint(Vec<u8>),
}

impl ChannelBinding {
    fn gs2_header(&self) -> &'static str {
        match self {
            ChannelBinding::None => "n,,",
            ChannelBinding::Supported => "y,,",
            ChannelBinding::TlsServerEndPoint(_) => "p=tls-server-end-point,,",
        }
    }

    /// The `c=` value: the base64 of the GS2 header followed by the binding data.
    fn c_value(&self) -> String {
        let mut v = self.gs2_header().as_bytes().to_vec();
        if let ChannelBinding::TlsServerEndPoint(d) = self {
            v.extend_from_slice(d);
        }
        base64::encode(&v)
    }
}

/// The hash `tls-server-end-point` uses for a certificate, from the OID of its signature
/// algorithm: the hash named by it, except that MD5 and SHA-1 are replaced by SHA-256 (RFC 5929
/// section 4.1). `None` for an algorithm without a plain hash (EdDSA, RSASSA-PSS), where
/// PostgreSQL itself cannot bind either.
pub fn cert_hash_algorithm(signature_oid: &[u8]) -> Option<CertHash> {
    // DER content bytes of the OIDs
    const RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01]; // 1.2.840.113549.1.1.x
    const ECDSA_SHA2: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03]; // 1.2.840.10045.4.3.x
    const ECDSA_SHA1: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x01]; // 1.2.840.10045.4.1
    if let Some(&last) = signature_oid.strip_prefix(RSA).and_then(|r| r.first()) {
        return match (signature_oid.len() == RSA.len() + 1, last) {
            (true, 4 | 5) => Some(CertHash::Sha256), // md5WithRSA, sha1WithRSA
            (true, 11) => Some(CertHash::Sha256),
            (true, 12) => Some(CertHash::Sha384),
            (true, 13) => Some(CertHash::Sha512),
            (true, 14) => Some(CertHash::Sha256), // sha224WithRSA: replaced like the weak ones
            _ => None,
        };
    }
    if let Some(&last) = signature_oid
        .strip_prefix(ECDSA_SHA2)
        .and_then(|r| r.first())
    {
        return match (signature_oid.len() == ECDSA_SHA2.len() + 1, last) {
            (true, 1) => Some(CertHash::Sha256), // ecdsa-with-SHA224
            (true, 2) => Some(CertHash::Sha256),
            (true, 3) => Some(CertHash::Sha384),
            (true, 4) => Some(CertHash::Sha512),
            _ => None,
        };
    }
    if signature_oid == ECDSA_SHA1 {
        return Some(CertHash::Sha256);
    }
    None
}

/// The hash function of a certificate's channel-binding data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertHash {
    /// SHA-256 (also used for MD5, SHA-1 and SHA-224 signatures).
    Sha256,
    /// SHA-384.
    Sha384,
    /// SHA-512.
    Sha512,
}

impl CertHash {
    /// Hash a DER certificate.
    pub fn digest(self, cert_der: &[u8]) -> Vec<u8> {
        match self {
            CertHash::Sha256 => sha256(cert_der).to_vec(),
            CertHash::Sha384 => crate::sha512::sha384(cert_der).to_vec(),
            CertHash::Sha512 => crate::sha512::sha512(cert_der).to_vec(),
        }
    }
}

/// The signature algorithm OID (the DER content bytes) of an X.509 certificate, read without
/// parsing the rest: `Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signature }`.
pub fn certificate_signature_oid(der: &[u8]) -> Option<&[u8]> {
    // one TLV: (tag, content, rest)
    fn tlv(b: &[u8]) -> Option<(u8, &[u8], &[u8])> {
        let (&tag, b) = b.split_first()?;
        let (&l0, b) = b.split_first()?;
        let (len, b) = if l0 < 0x80 {
            (usize::from(l0), b)
        } else {
            let n = usize::from(l0 & 0x7f);
            if n == 0 || n > 4 || b.len() < n {
                return None;
            }
            let len = b[..n]
                .iter()
                .fold(0usize, |a, x| (a << 8) | usize::from(*x));
            (len, &b[n..])
        };
        if b.len() < len {
            return None;
        }
        Some((tag, &b[..len], &b[len..]))
    }
    let (tag, cert, _) = tlv(der)?;
    if tag != 0x30 {
        return None;
    }
    let (tag, _tbs, rest) = tlv(cert)?;
    if tag != 0x30 {
        return None;
    }
    let (tag, alg, _) = tlv(rest)?;
    if tag != 0x30 {
        return None;
    }
    let (tag, oid, _) = tlv(alg)?;
    (tag == 0x06).then_some(oid)
}

fn escape_user(user: &str) -> String {
    user.replace('=', "=3D").replace(',', "=2C")
}

/// The client side of one SCRAM-SHA-256 exchange.
#[derive(Debug)]
pub struct ScramClient {
    password: Secret,
    binding: ChannelBinding,
    client_first_bare: String,
    nonce: String,
    expected_server_signature: Option<[u8; 32]>,
}

impl ScramClient {
    /// Start an exchange. PostgreSQL ignores the SCRAM user name and uses the startup user, so
    /// pass an empty string there. `nonce` must be unpredictable and free of commas
    /// ([`generate_nonce`] qualifies).
    pub fn new(username: &str, password: &str, nonce: String) -> Self {
        Self::with_binding(username, password, nonce, ChannelBinding::None)
    }

    /// Like [`ScramClient::new`], with the channel-binding choice for the GS2 header.
    pub fn with_binding(
        username: &str,
        password: &str,
        nonce: String,
        binding: ChannelBinding,
    ) -> Self {
        let client_first_bare = format!("n={},r={}", escape_user(username), nonce);
        ScramClient {
            password: Secret::new(password.as_bytes()),
            binding,
            client_first_bare,
            nonce,
            expected_server_signature: None,
        }
    }

    /// The SASL mechanism this exchange is for.
    pub fn mechanism(&self) -> &'static str {
        match self.binding {
            ChannelBinding::TlsServerEndPoint(_) => "SCRAM-SHA-256-PLUS",
            _ => "SCRAM-SHA-256",
        }
    }

    /// The client-first message, including the GS2 header (`n,,`, `y,,` or
    /// `p=tls-server-end-point,,`).
    pub fn client_first(&self) -> String {
        format!("{}{}", self.binding.gs2_header(), self.client_first_bare)
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
        let client_final_no_proof = format!("c={},r={r}", self.binding.c_value());
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
    fn channel_binding_goes_into_the_gs2_header_and_the_c_attribute() {
        let data = vec![7u8; 32];
        let mut c = ScramClient::with_binding(
            "",
            "pencil",
            NONCE.to_string(),
            ChannelBinding::TlsServerEndPoint(data.clone()),
        );
        assert_eq!(c.mechanism(), "SCRAM-SHA-256-PLUS");
        assert_eq!(
            c.client_first(),
            "p=tls-server-end-point,,n=,r=rOprNGfwEbeRWgbNEkqO"
        );
        let fin = c.client_final(SERVER_FIRST).unwrap();
        let mut gs2 = b"p=tls-server-end-point,,".to_vec();
        gs2.extend_from_slice(&data);
        assert!(
            fin.starts_with(&format!("c={},r=", base64::encode(&gs2))),
            "{fin}"
        );
        // the proof depends on the binding: the same exchange without it gives another proof
        let mut plain = ScramClient::new("", "pencil", NONCE.to_string());
        let plain_fin = plain.client_final(SERVER_FIRST).unwrap();
        assert_ne!(fin.rsplit("p=").next(), plain_fin.rsplit("p=").next());
        // `y` and `n`
        let y = ScramClient::with_binding("", "x", "n".into(), ChannelBinding::Supported);
        assert_eq!(y.client_first(), "y,,n=,r=n");
        assert_eq!(y.mechanism(), "SCRAM-SHA-256");
        assert_eq!(ChannelBinding::Supported.c_value(), "eSws");
        assert_eq!(ChannelBinding::None.c_value(), "biws");
    }

    /// A DER certificate prefix with the given signature algorithm OID: enough for the reader.
    fn cert_with(oid: &[u8]) -> Vec<u8> {
        let alg = [&[0x06, oid.len() as u8][..], oid, &[0x05, 0x00]].concat();
        let alg = [&[0x30, alg.len() as u8][..], &alg].concat();
        let tbs = [0x30, 0x03, 0x02, 0x01, 0x00];
        let body = [&tbs[..], &alg, &[0x03, 0x02, 0x00, 0x00]].concat();
        [&[0x30, body.len() as u8][..], &body].concat()
    }

    #[test]
    fn the_hash_follows_the_certificates_signature_algorithm() {
        let rsa = |n: u8| [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, n];
        let ec = |n: u8| [0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, n];
        let h = |oid: &[u8]| {
            let der = cert_with(oid);
            cert_hash_algorithm(certificate_signature_oid(&der).unwrap())
        };
        assert_eq!(h(&rsa(11)), Some(CertHash::Sha256));
        assert_eq!(h(&rsa(12)), Some(CertHash::Sha384));
        assert_eq!(h(&rsa(13)), Some(CertHash::Sha512));
        assert_eq!(
            h(&rsa(5)),
            Some(CertHash::Sha256),
            "SHA-1 is replaced by SHA-256"
        );
        assert_eq!(h(&rsa(4)), Some(CertHash::Sha256), "MD5 too");
        assert_eq!(h(&rsa(10)), None, "RSASSA-PSS has no plain hash");
        assert_eq!(h(&ec(2)), Some(CertHash::Sha256));
        assert_eq!(h(&ec(3)), Some(CertHash::Sha384));
        assert_eq!(h(&ec(4)), Some(CertHash::Sha512));
        assert_eq!(h(&[0x2b, 0x65, 0x70]), None, "Ed25519");
        // malformed input is None, never a panic
        assert!(certificate_signature_oid(&[]).is_none());
        assert!(certificate_signature_oid(&[0x30, 0x84, 0xff, 0xff, 0xff, 0xff]).is_none());
        let der = cert_with(&rsa(11));
        for cut in 0..der.len() {
            let _ = certificate_signature_oid(&der[..cut]);
        }
        assert_eq!(CertHash::Sha256.digest(b"abc").len(), 32);
        assert_eq!(CertHash::Sha384.digest(b"abc").len(), 48);
        assert_eq!(CertHash::Sha512.digest(b"abc").len(), 64);
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
