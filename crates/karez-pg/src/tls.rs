//! TLS through rustls (ring provider). Compiled only with the `tls` feature.

use crate::config::{Config, SslMode};
use crate::error::{Error, Result};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, ClientConnection, DigitallySignedStruct, Error as TlsError,
    RootCertStore, SignatureScheme, StreamOwned,
};
use std::net::TcpStream;
use std::sync::Arc;

fn tls_err(e: impl std::fmt::Display) -> Error {
    Error::Tls(e.to_string())
}

fn roots(cfg: &Config) -> Result<RootCertStore> {
    let mut store = RootCertStore::empty();
    match cfg.ssl_root_cert.as_deref() {
        None | Some("system") => {
            store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        }
        Some(path) => {
            let bytes = std::fs::read(path)
                .map_err(|e| Error::Tls(format!("cannot read sslrootcert {path:?}: {e}")))?;
            let mut n = 0;
            for cert in CertificateDer::pem_slice_iter(&bytes) {
                let cert = cert.map_err(|e| Error::Tls(format!("sslrootcert {path:?}: {e}")))?;
                store.add(cert).map_err(tls_err)?;
                n += 1;
            }
            if n == 0 {
                return Err(Error::Tls(format!(
                    "sslrootcert {path:?} holds no certificate"
                )));
            }
        }
    }
    Ok(store)
}

/// `sslmode=verify-ca`: the chain must be trusted; the host name is not checked.
#[derive(Debug)]
struct ChainOnly(Arc<WebPkiServerVerifier>);

impl ServerCertVerifier for ChainOnly {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, TlsError> {
        match self
            .0
            .verify_server_cert(end_entity, intermediates, server_name, ocsp, now)
        {
            Err(TlsError::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

/// `sslmode=require`: encrypt, trust whatever certificate arrives. Stops passive listening only.
#[derive(Debug)]
struct AcceptAny(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// The client certificate chain and key from `sslcert` and `sslkey`, if set. Both or neither.
fn client_auth(
    cfg: &Config,
) -> Result<
    Option<(
        Vec<CertificateDer<'static>>,
        rustls::pki_types::PrivateKeyDer<'static>,
    )>,
> {
    use rustls::pki_types::PrivateKeyDer;
    let (cert_path, key_path) = match (&cfg.ssl_cert, &cfg.ssl_key) {
        (None, None) => return Ok(None),
        (Some(c), Some(k)) => (c, k),
        _ => {
            return Err(Error::Tls(
                "sslcert and sslkey must be given together".into(),
            ))
        }
    };
    let bytes = std::fs::read(cert_path)
        .map_err(|e| Error::Tls(format!("cannot read sslcert {cert_path:?}: {e}")))?;
    let chain = CertificateDer::pem_slice_iter(&bytes)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Tls(format!("sslcert {cert_path:?}: {e}")))?;
    if chain.is_empty() {
        return Err(Error::Tls(format!(
            "sslcert {cert_path:?} holds no certificate"
        )));
    }
    let key_bytes = std::fs::read(key_path)
        .map_err(|e| Error::Tls(format!("cannot read sslkey {key_path:?}: {e}")))?;
    // PKCS#8, PKCS#1 ("RSA PRIVATE KEY") and SEC1 ("EC PRIVATE KEY") are all read; an encrypted key
    // ("ENCRYPTED PRIVATE KEY") is not, and says so
    let key = PrivateKeyDer::from_pem_slice(&key_bytes).map_err(|e| {
        Error::Tls(format!(
            "sslkey {key_path:?}: {e} (a key protected by a passphrase is not supported)"
        ))
    })?;
    Ok(Some((chain, key)))
}

/// Run the TLS handshake over `stream` (after the server answered `S` to the SSLRequest, or at once
/// for `sslnegotiation=direct`).
pub(crate) fn wrap(
    mut stream: TcpStream,
    cfg: &Config,
) -> Result<StreamOwned<ClientConnection, TcpStream>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(tls_err)?;
    let auth = client_auth(cfg)?;
    // the verifier decides how the server is checked; the client certificate, if any, is separate
    let config = match cfg.ssl_mode {
        SslMode::VerifyFull => {
            let b = builder.with_root_certificates(roots(cfg)?);
            match auth {
                Some((chain, key)) => b.with_client_auth_cert(chain, key).map_err(tls_err)?,
                None => b.with_no_client_auth(),
            }
        }
        SslMode::VerifyCa => {
            let inner = WebPkiServerVerifier::builder_with_provider(
                Arc::new(roots(cfg)?),
                provider.clone(),
            )
            .build()
            .map_err(tls_err)?;
            let b = builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(ChainOnly(inner)));
            match auth {
                Some((chain, key)) => b.with_client_auth_cert(chain, key).map_err(tls_err)?,
                None => b.with_no_client_auth(),
            }
        }
        SslMode::Require => {
            let b = builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(AcceptAny(provider)));
            match auth {
                Some((chain, key)) => b.with_client_auth_cert(chain, key).map_err(tls_err)?,
                None => b.with_no_client_auth(),
            }
        }
        SslMode::Disable => return Err(Error::Usage("TLS requested with sslmode=disable")),
    };
    let mut config = config;
    if cfg.ssl_negotiation == crate::config::SslNegotiation::Direct {
        // a server that accepts a direct handshake requires this protocol name (PostgreSQL 17)
        config.alpn_protocols = vec![b"postgresql".to_vec()];
    }
    let name = ServerName::try_from(cfg.host.clone())
        .map_err(|e| Error::Tls(format!("{:?} is not a valid server name: {e}", cfg.host)))?;
    let mut conn = ClientConnection::new(Arc::new(config), name).map_err(tls_err)?;
    // Finish the handshake now so a bad certificate is reported here, not on the first query.
    while conn.is_handshaking() {
        conn.complete_io(&mut stream).map_err(|e| {
            // rustls reports certificate problems as the source of an io::Error.
            match e.get_ref().and_then(|s| s.downcast_ref::<TlsError>()) {
                Some(t) => Error::Tls(t.to_string()),
                None => Error::Tls(e.to_string()),
            }
        })?;
    }
    Ok(StreamOwned::new(conn, stream))
}
