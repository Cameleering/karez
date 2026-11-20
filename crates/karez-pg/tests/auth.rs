//! Authentication beyond a plain password: OAUTHBEARER against a scripted server, and SCRAM channel
//! binding against a real PostgreSQL over TLS (`KAREZ_TEST_PG`, tests/docker/README.md).

use karez_pg::{AuthPolicy, Config, Connection, Error, OAuthChallenge, SslMode};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

fn msg(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
    v.extend_from_slice(body);
    v
}

fn read_msg(s: &mut TcpStream) -> Option<(u8, Vec<u8>)> {
    let mut head = [0u8; 5];
    s.read_exact(&mut head).ok()?;
    let n = i32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize - 4;
    let mut body = vec![0; n];
    s.read_exact(&mut body).ok()?;
    Some((head[0], body))
}

fn error_response(code: &str, text: &str) -> Vec<u8> {
    let mut b = Vec::new();
    for (k, v) in [(b'S', "FATAL"), (b'V', "FATAL"), (b'C', code), (b'M', text)] {
        b.push(k);
        b.extend_from_slice(v.as_bytes());
        b.push(0);
    }
    b.push(0);
    msg(b'E', &b)
}

/// What the scripted server saw.
#[derive(Default)]
struct Seen {
    /// The data of each SASLInitialResponse, with the mechanism name.
    initial: Mutex<Vec<(String, Vec<u8>)>>,
    /// The data of each SASLResponse that followed.
    responses: Mutex<Vec<Vec<u8>>>,
    connections: AtomicUsize,
}

/// A server that offers `mechs`; accepts the bearer token `good`, answers an empty one with the
/// discovery JSON and any other token with the same JSON as a rejection.
fn start_oauth(mechs: &[&str], seen: Arc<Seen>) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let mechs: Vec<String> = mechs.iter().map(|m| m.to_string()).collect();
    thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            seen.connections.fetch_add(1, Ordering::SeqCst);
            let (mechs, seen) = (mechs.clone(), seen.clone());
            thread::spawn(move || {
                // startup message: no tag
                let mut len = [0u8; 4];
                if s.read_exact(&mut len).is_err() {
                    return;
                }
                let mut rest = vec![0; i32::from_be_bytes(len) as usize - 4];
                if s.read_exact(&mut rest).is_err() {
                    return;
                }
                let mut offer = 10i32.to_be_bytes().to_vec();
                for m in &mechs {
                    offer.extend_from_slice(m.as_bytes());
                    offer.push(0);
                }
                offer.push(0);
                s.write_all(&msg(b'R', &offer)).unwrap();
                let Some((b'p', body)) = read_msg(&mut s) else {
                    return;
                };
                // mechanism, then a length and the data
                let end = body.iter().position(|b| *b == 0).unwrap();
                let mech = String::from_utf8_lossy(&body[..end]).into_owned();
                let data = body[end + 5..].to_vec();
                seen.initial.lock().unwrap().push((mech, data.clone()));
                if data.starts_with(b"n,,\x01auth=Bearer good\x01") {
                    let mut out = msg(b'R', &0i32.to_be_bytes());
                    out.extend(msg(b'K', &[0, 0, 0, 7, 0, 0, 0, 9]));
                    out.extend(msg(b'Z', b"I"));
                    s.write_all(&out).unwrap();
                    // stay until the client leaves
                    while read_msg(&mut s).is_some() {}
                    return;
                }
                let json = br#"{"status":"invalid_token","scope":"openid karez","openid-configuration":"https://idp.example/.well-known/openid-configuration"}"#;
                let mut cont = 11i32.to_be_bytes().to_vec();
                cont.extend_from_slice(json);
                s.write_all(&msg(b'R', &cont)).unwrap();
                if let Some((b'p', ack)) = read_msg(&mut s) {
                    seen.responses.lock().unwrap().push(ack);
                }
                let _ = s.write_all(&error_response(
                    "28000",
                    "OAuth bearer authentication failed",
                ));
            });
        }
    });
    port
}

fn cfg(port: u16) -> Config {
    let mut c = Config::new("127.0.0.1", port, "u");
    c.ssl_mode = SslMode::Disable;
    c
}

#[test]
fn a_good_token_logs_in_and_the_message_has_the_rfc_shape() {
    let seen = Arc::new(Seen::default());
    let port = start_oauth(&["OAUTHBEARER"], seen.clone());
    let mut c = cfg(port);
    c.oauth_token("good");
    assert!(c.auth.oauth);
    let conn = Connection::connect(&c).unwrap();
    assert_eq!(conn.process_id(), 7);
    let initial = seen.initial.lock().unwrap();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].0, "OAUTHBEARER");
    assert_eq!(initial[0].1, b"n,,\x01auth=Bearer good\x01\x01");
}

#[test]
fn a_refused_token_is_answered_with_the_ack_and_reported_with_what_the_server_needs() {
    let seen = Arc::new(Seen::default());
    let port = start_oauth(&["OAUTHBEARER"], seen.clone());
    let calls = Arc::new(Mutex::new(Vec::<Option<OAuthChallenge>>::new()));
    let mut c = cfg(port);
    let log = calls.clone();
    c.oauth_token_with(move |ch| {
        log.lock().unwrap().push(ch.cloned());
        Ok("expired-token".to_string())
    });
    match Connection::connect(&c) {
        Err(Error::OAuthChallenge {
            challenge,
            discovery,
        }) => {
            assert!(!discovery, "a token was sent");
            assert_eq!(challenge.status.as_deref(), Some("invalid_token"));
            assert_eq!(challenge.scope.as_deref(), Some("openid karez"));
            assert_eq!(
                challenge.openid_configuration.as_deref(),
                Some("https://idp.example/.well-known/openid-configuration")
            );
        }
        other => panic!("{:?}", other.map(|_| ())),
    }
    // the RFC's single 0x01, and no second attempt
    assert_eq!(*seen.responses.lock().unwrap(), [b"\x01".to_vec()]);
    assert_eq!(seen.connections.load(Ordering::SeqCst), 1);
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn without_a_token_the_client_asks_the_server_and_connects_again_with_one() {
    let seen = Arc::new(Seen::default());
    let port = start_oauth(&["OAUTHBEARER"], seen.clone());
    let calls = Arc::new(Mutex::new(Vec::<Option<OAuthChallenge>>::new()));
    let mut c = cfg(port);
    let log = calls.clone();
    c.oauth_token_with(move |ch| {
        log.lock().unwrap().push(ch.cloned());
        // first no token (discovery), then a token for the server's challenge
        Ok(if ch.is_some() { "good" } else { "" }.to_string())
    });
    let conn = Connection::connect(&c).unwrap();
    assert_eq!(conn.process_id(), 7);
    let initial = seen.initial.lock().unwrap();
    assert_eq!(
        initial[0].1, b"n,,\x01auth=\x01\x01",
        "the first attempt carries no token"
    );
    assert_eq!(initial[1].1, b"n,,\x01auth=Bearer good\x01\x01");
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].is_none());
    let ch = calls[1]
        .as_ref()
        .expect("the second call gets the challenge");
    assert_eq!(ch.scope.as_deref(), Some("openid karez"));
    assert_eq!(seen.connections.load(Ordering::SeqCst), 2);
}

#[test]
fn a_token_source_that_fails_or_gives_nothing_is_an_authentication_error() {
    let seen = Arc::new(Seen::default());
    let port = start_oauth(&["OAUTHBEARER"], seen.clone());
    let mut c = cfg(port);
    c.oauth_token_with(|_| Err("the identity provider is down".into()));
    match Connection::connect(&c) {
        Err(Error::Auth(m)) => assert!(m.contains("identity provider is down"), "{m}"),
        other => panic!("{:?}", other.map(|_| ())),
    }
    // nothing was sent to the server
    assert!(seen.initial.lock().unwrap().is_empty());
    // discovery, then the source still has no token
    let mut c = cfg(port);
    c.oauth_token_with(|_| Ok(String::new()));
    match Connection::connect(&c) {
        Err(Error::Auth(m)) => assert!(m.contains("no token"), "{m}"),
        other => panic!("{:?}", other.map(|_| ())),
    }
    // a token that could change the message structure is refused before it is sent
    let before = seen.initial.lock().unwrap().len();
    let mut c = cfg(port);
    c.oauth_token("a\x01\x01n,,");
    assert!(Connection::connect(&c).is_err());
    assert_eq!(seen.initial.lock().unwrap().len(), before);
}

#[test]
fn oauth_is_used_only_when_allowed_and_configured() {
    let seen = Arc::new(Seen::default());
    let port = start_oauth(&["OAUTHBEARER"], seen.clone());
    // not configured: the default policy allows SCRAM only, and the server offers no SCRAM
    match Connection::connect(&cfg(port)) {
        Err(Error::Auth(m)) => assert!(m.contains("OAUTHBEARER") || m.contains("SCRAM"), "{m}"),
        other => panic!("{:?}", other.map(|_| ())),
    }
    // configured, but the policy says SCRAM only
    let mut c = cfg(port);
    c.oauth_token("good");
    c.auth = AuthPolicy {
        oauth: false,
        ..AuthPolicy::default()
    };
    assert!(Connection::connect(&c).is_err());
    assert!(seen.initial.lock().unwrap().is_empty());
    // the URL form
    let c =
        Config::from_url("postgresql://u@h/d?auth=oauth,scram&channel_binding=require").unwrap();
    assert!(c.auth.oauth && c.auth.scram && !c.auth.cleartext);
    assert_eq!(c.channel_binding, karez_pg::ChannelBindingMode::Require);
    assert!(Config::from_url("postgresql://u@h/d?channel_binding=sometimes").is_err());
}

#[test]
fn the_token_never_shows_in_debug_output() {
    let mut c = Config::new("h", 5432, "u");
    c.oauth_token("super-secret-token-value");
    assert!(!format!("{c:?}").contains("super-secret"));
}

#[cfg(feature = "tls")]
mod channel_binding {
    use super::*;
    use karez_pg::{ChannelBindingMode, Value};
    use std::net::ToSocketAddrs;
    use std::time::Duration;

    fn server() -> Result<String, String> {
        let addr =
            std::env::var("KAREZ_TEST_PG").map_err(|_| "KAREZ_TEST_PG is not set".to_string())?;
        let sock = addr
            .to_socket_addrs()
            .map_err(|e| format!("cannot resolve {addr}: {e}"))?
            .next()
            .ok_or_else(|| format!("{addr} resolves to nothing"))?;
        TcpStream::connect_timeout(&sock, Duration::from_secs(2))
            .map_err(|e| format!("{addr} is not reachable: {e}"))?;
        Ok(addr)
    }

    macro_rules! need_server {
        () => {
            match server() {
                Ok(a) => a,
                Err(why) => {
                    eprintln!("SKIPPED (not verified against a real server): {why}");
                    if std::env::var("KAREZ_REQUIRE_SERVER").as_deref() == Ok("1") {
                        panic!("KAREZ_REQUIRE_SERVER=1 but there is no server: {why}");
                    }
                    return;
                }
            }
        };
    }

    fn tls_cfg(addr: &str, mode: ChannelBindingMode) -> Config {
        let port = addr.rsplit(':').next().unwrap().to_string();
        let ca = std::env::var("KAREZ_TEST_CA").unwrap_or_else(|_| {
            format!(
                "{}/../../tests/docker/certs/ca-{port}.crt",
                env!("CARGO_MANIFEST_DIR")
            )
        });
        let mut c = Config::from_url(&format!(
            "postgresql://karez_scram:karez_test_only_scram@{addr}/karez"
        ))
        .unwrap();
        c.host = "localhost".into();
        c.ssl_mode = SslMode::VerifyFull;
        c.ssl_root_cert = Some(ca);
        c.channel_binding = mode;
        c
    }

    fn works(c: &mut Connection) {
        let mut cur = c.query("SELECT $1::int8 + 1", &[Value::I64(41)]).unwrap();
        assert_eq!(cur.next().unwrap().unwrap().get::<i64>(0).unwrap(), 42);
    }

    #[test]
    fn scram_plus_binds_the_login_to_the_servers_certificate() {
        let addr = need_server!();
        // `require` succeeds only if the exchange was bound and the server accepted the binding
        let mut c = Connection::connect(&tls_cfg(&addr, ChannelBindingMode::Require)).unwrap();
        works(&mut c);
        // the default is `prefer`, which binds here
        let mut c = Connection::connect(&tls_cfg(&addr, ChannelBindingMode::Prefer)).unwrap();
        works(&mut c);
        // and it can be switched off
        let mut c = Connection::connect(&tls_cfg(&addr, ChannelBindingMode::Disable)).unwrap();
        works(&mut c);
        // a wrong password still fails the bound exchange, with the server's own error
        let mut bad = tls_cfg(&addr, ChannelBindingMode::Require);
        bad.password("not the password");
        match Connection::connect(&bad) {
            Err(Error::Server(e)) => assert_eq!(e.sqlstate(), "28P01"),
            Err(Error::Proto(_)) | Err(Error::Auth(_)) => {}
            other => panic!("{:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn requiring_channel_binding_without_tls_is_refused_before_any_secret_is_sent() {
        let addr = need_server!();
        let mut c = Config::from_url(&format!(
            "postgresql://karez_scram:karez_test_only_scram@{addr}/karez?sslmode=disable&channel_binding=require"
        ))
        .unwrap();
        c.channel_binding = ChannelBindingMode::Require;
        match Connection::connect(&c) {
            Err(Error::Auth(m)) => assert!(m.contains("TLS"), "{m}"),
            other => panic!("{:?}", other.map(|_| ())),
        }
        // without `require` the same plain connection works
        c.channel_binding = ChannelBindingMode::Prefer;
        let mut conn = Connection::connect(&c).unwrap();
        works(&mut conn);
    }
}

/// OAUTHBEARER against a real PostgreSQL 18 with a test validator (`tests/docker/oauth`, profile
/// `oauth`, `KAREZ_TEST_OAUTH=127.0.0.1:54319`). The validator accepts `karez-token-for-<role>`.
mod oauth_server {
    use super::*;
    use karez_pg::Value;
    use std::net::ToSocketAddrs;
    use std::time::Duration;

    fn server() -> Result<String, String> {
        let addr = std::env::var("KAREZ_TEST_OAUTH")
            .map_err(|_| "KAREZ_TEST_OAUTH is not set".to_string())?;
        let sock = addr
            .to_socket_addrs()
            .map_err(|e| format!("cannot resolve {addr}: {e}"))?
            .next()
            .ok_or_else(|| format!("{addr} resolves to nothing"))?;
        TcpStream::connect_timeout(&sock, Duration::from_secs(2))
            .map_err(|e| format!("{addr} is not reachable: {e}"))?;
        Ok(addr)
    }

    macro_rules! need_oauth {
        () => {
            match server() {
                Ok(a) => a,
                Err(why) => {
                    eprintln!("SKIPPED (not verified against a real OAuth server): {why}");
                    if std::env::var("KAREZ_REQUIRE_OAUTH").as_deref() == Ok("1") {
                        panic!("KAREZ_REQUIRE_OAUTH=1 but there is no OAuth server: {why}");
                    }
                    return;
                }
            }
        };
    }

    const GOOD: &str = "karez-token-for-karez_oauth";

    fn oauth_cfg(addr: &str) -> Config {
        let mut c = Config::from_url(&format!(
            "postgresql://karez_oauth@{addr}/postgres?sslmode=disable"
        ))
        .unwrap();
        c.ssl_mode = SslMode::Disable;
        c
    }

    fn current_user(c: &mut Connection) -> String {
        let mut cur = c.query("SELECT current_user::text", &[]).unwrap();
        cur.next().unwrap().unwrap().get::<String>(0).unwrap()
    }

    #[test]
    fn a_valid_token_logs_in_as_the_role() {
        let addr = need_oauth!();
        let mut cfg = oauth_cfg(&addr);
        cfg.oauth_token(GOOD);
        let mut c = Connection::connect(&cfg).unwrap();
        assert_eq!(current_user(&mut c), "karez_oauth");
        let mut cur = c.query("SELECT $1::int8 + 1", &[Value::I64(1)]).unwrap();
        assert_eq!(cur.next().unwrap().unwrap().get::<i64>(0).unwrap(), 2);
    }

    #[test]
    fn a_wrong_token_is_refused_with_the_servers_challenge() {
        let addr = need_oauth!();
        let mut cfg = oauth_cfg(&addr);
        cfg.oauth_token("karez-token-for-somebody-else");
        match Connection::connect(&cfg) {
            Err(Error::OAuthChallenge {
                challenge,
                discovery,
            }) => {
                assert!(!discovery);
                assert_eq!(challenge.scope.as_deref(), Some("openid karez"));
                assert!(
                    challenge
                        .openid_configuration
                        .as_deref()
                        .is_some_and(|u| u.starts_with("https://issuer.example.test")),
                    "{challenge:?}"
                );
            }
            other => panic!("{:?}", other.map(|_| ())),
        }
        // a token with a character that is not allowed never reaches the server
        let mut cfg = oauth_cfg(&addr);
        cfg.oauth_token("bad token");
        assert!(matches!(Connection::connect(&cfg), Err(Error::Proto(_))));
    }

    #[test]
    fn discovery_finds_the_scope_and_the_token_source_answers_it() {
        let addr = need_oauth!();
        let asked = Arc::new(Mutex::new(Vec::<Option<OAuthChallenge>>::new()));
        let log = asked.clone();
        let mut cfg = oauth_cfg(&addr);
        cfg.oauth_token_with(move |ch| {
            log.lock().unwrap().push(ch.cloned());
            Ok(match ch {
                None => String::new(), // no token yet: ask the server what it needs
                Some(c) => {
                    assert_eq!(c.scope.as_deref(), Some("openid karez"));
                    GOOD.to_string()
                }
            })
        });
        let mut c = Connection::connect(&cfg).unwrap();
        assert_eq!(current_user(&mut c), "karez_oauth");
        let asked = asked.lock().unwrap();
        assert_eq!(asked.len(), 2);
        assert!(asked[0].is_none() && asked[1].is_some());
    }

    #[test]
    fn a_role_with_a_password_rule_does_not_take_a_token() {
        let addr = need_oauth!();
        let mut cfg = oauth_cfg(&addr);
        cfg.user = "postgres".into();
        cfg.oauth_token(GOOD);
        // the server offers SCRAM for this role, not OAUTHBEARER; with no password given the
        // client says so instead of sending a token to a server that did not ask for one
        match Connection::connect(&cfg) {
            Err(Error::Auth(_)) | Err(Error::Server(_)) => {}
            other => panic!("{:?}", other.map(|_| ())),
        }
    }
}

/// Kerberos (GSSAPI) against a real KDC and a PostgreSQL that requires it (`tests/docker/gss`,
/// profile `gss`). Needs the `gss` feature and a Unix system; `KAREZ_TEST_GSS=127.0.0.1:54320` and
/// `KRB5_CONFIG=tests/docker/gss/krb5-client.conf`. The user is `karez_gss@KAREZ.TEST`.
#[cfg(all(feature = "gss", unix))]
mod gss_server {
    use super::*;
    use karez_pg::Value;
    use std::net::ToSocketAddrs;
    use std::time::Duration;

    fn server() -> Result<String, String> {
        let addr =
            std::env::var("KAREZ_TEST_GSS").map_err(|_| "KAREZ_TEST_GSS is not set".to_string())?;
        if std::env::var("KRB5_CONFIG").is_err() {
            return Err("KRB5_CONFIG does not point at tests/docker/gss/krb5-client.conf".into());
        }
        let sock = addr
            .to_socket_addrs()
            .map_err(|e| format!("cannot resolve {addr}: {e}"))?
            .next()
            .ok_or_else(|| format!("{addr} resolves to nothing"))?;
        TcpStream::connect_timeout(&sock, Duration::from_secs(2))
            .map_err(|e| format!("{addr} is not reachable: {e}"))?;
        Ok(addr)
    }

    macro_rules! need_gss {
        () => {
            match server() {
                Ok(a) => a,
                Err(why) => {
                    eprintln!("SKIPPED (not verified against a real Kerberos server): {why}");
                    if std::env::var("KAREZ_REQUIRE_GSS").as_deref() == Ok("1") {
                        panic!("KAREZ_REQUIRE_GSS=1 but there is no Kerberos server: {why}");
                    }
                    return;
                }
            }
        };
    }

    /// The server's principal is `postgres/localhost@KAREZ.TEST`, so the host name must be
    /// `localhost`, whatever address it resolves to.
    fn gss_cfg(addr: &str) -> Config {
        let port = addr.rsplit(':').next().unwrap();
        let mut c = Config::from_url(&format!(
            "postgresql://karez_gss@localhost:{port}/postgres?sslmode=disable"
        ))
        .unwrap();
        c.ssl_mode = SslMode::Disable;
        c
    }

    #[test]
    fn a_kerberos_login_with_a_password_succeeds_and_is_mutual() {
        let addr = need_gss!();
        let mut cfg = gss_cfg(&addr);
        cfg.gss_credentials("karez_gss@KAREZ.TEST", "karez_test_only_gss");
        assert!(cfg.auth.gss);
        let mut c = Connection::connect(&cfg).unwrap();
        let mut cur = c
            .query("SELECT current_user::text, $1::int8 + 1", &[Value::I64(1)])
            .unwrap();
        let r = cur.next().unwrap().unwrap();
        assert_eq!(r.get::<String>(0).unwrap(), "karez_gss");
        assert_eq!(r.get::<i64>(1).unwrap(), 2);
    }

    #[test]
    fn a_wrong_password_is_refused_by_the_kdc_and_says_so() {
        let addr = need_gss!();
        let mut cfg = gss_cfg(&addr);
        cfg.gss_credentials("karez_gss@KAREZ.TEST", "not the password");
        match Connection::connect(&cfg) {
            Err(Error::Auth(m)) => assert!(
                m.starts_with("authentication failed") || m.contains("Kerberos"),
                "{m}"
            ),
            other => panic!("{:?}", other.map(|_| ())),
        }
        // the secret is not in the error
        let e = Connection::connect(&cfg)
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert!(!e.contains("not the password"), "{e}");
    }

    #[test]
    fn kerberos_is_used_only_when_allowed() {
        let addr = need_gss!();
        // the default policy does not allow it: the client refuses before sending a token
        let mut cfg = gss_cfg(&addr);
        cfg.gss_credentials("karez_gss@KAREZ.TEST", "karez_test_only_gss");
        cfg.auth.gss = false;
        match Connection::connect(&cfg) {
            Err(Error::Auth(m)) => assert!(m.contains("auth=gss"), "{m}"),
            other => panic!("{:?}", other.map(|_| ())),
        }
        // the URL spelling turns it on
        let c = Config::from_url("postgresql://u@h/d?auth=gss&krbsrvname=svc").unwrap();
        assert!(c.auth.gss && !c.auth.scram);
        assert_eq!(c.krbsrvname, "svc");
    }

    #[test]
    fn a_service_name_the_kdc_does_not_know_fails_cleanly() {
        let addr = need_gss!();
        let mut cfg = gss_cfg(&addr);
        cfg.gss_credentials("karez_gss@KAREZ.TEST", "karez_test_only_gss");
        cfg.krbsrvname = "wrongsvc".into();
        match Connection::connect(&cfg) {
            Err(Error::Auth(m)) => assert!(m.contains("Kerberos"), "{m}"),
            other => panic!("{:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn a_host_name_that_is_not_the_service_principals_fails_cleanly() {
        let addr = need_gss!();
        let mut cfg = gss_cfg(&addr);
        cfg.gss_credentials("karez_gss@KAREZ.TEST", "karez_test_only_gss");
        // the server is reachable on 127.0.0.1, but its principal is postgres/localhost
        cfg.host = "127.0.0.1".into();
        match Connection::connect(&cfg) {
            Err(Error::Auth(_)) => {}
            other => panic!("{:?}", other.map(|_| ())),
        }
    }
}

/// Client certificates and direct TLS negotiation, against a PostgreSQL 17 that requires a client
/// certificate for the role `karez_cert` (`tests/docker/compose.yaml`, profile `cert`,
/// `KAREZ_TEST_CERT=127.0.0.1:54321`). PostgreSQL 17 also accepts a direct TLS handshake.
#[cfg(feature = "tls")]
mod client_certificate {
    use super::*;
    use karez_pg::SslNegotiation;
    use std::net::ToSocketAddrs;
    use std::time::Duration;

    fn server() -> Result<String, String> {
        let addr = std::env::var("KAREZ_TEST_CERT")
            .map_err(|_| "KAREZ_TEST_CERT is not set".to_string())?;
        let sock = addr
            .to_socket_addrs()
            .map_err(|e| format!("cannot resolve {addr}: {e}"))?
            .next()
            .ok_or_else(|| format!("{addr} resolves to nothing"))?;
        TcpStream::connect_timeout(&sock, Duration::from_secs(2))
            .map_err(|e| format!("{addr} is not reachable: {e}"))?;
        Ok(addr)
    }

    macro_rules! need_cert {
        () => {
            match server() {
                Ok(a) => a,
                Err(why) => {
                    eprintln!("SKIPPED (not verified against a real server): {why}");
                    if std::env::var("KAREZ_REQUIRE_CERT").as_deref() == Ok("1") {
                        panic!("KAREZ_REQUIRE_CERT=1 but there is no server: {why}");
                    }
                    return;
                }
            }
        };
    }

    fn certs_dir() -> String {
        format!("{}/../../tests/docker/certs", env!("CARGO_MANIFEST_DIR"))
    }

    fn base(addr: &str) -> Config {
        let port = addr.rsplit(':').next().unwrap();
        let mut c =
            Config::from_url(&format!("postgresql://karez_cert@localhost:{port}/karez")).unwrap();
        c.ssl_mode = SslMode::VerifyFull;
        c.ssl_root_cert = Some(format!("{}/ca-{port}.crt", certs_dir()));
        c
    }

    fn with_cert(addr: &str, key_suffix: &str) -> Config {
        let port = addr.rsplit(':').next().unwrap();
        let mut c = base(addr);
        c.ssl_cert = Some(format!("{}/client-{port}.crt", certs_dir()));
        c.ssl_key = Some(format!("{}/client-{port}{key_suffix}.key", certs_dir()));
        c
    }

    fn who(c: &mut Connection) -> (String, String) {
        let mut cur = c
            .query(
                "SELECT current_user::text, coalesce(client_dn, '') FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
                &[],
            )
            .unwrap();
        let r = cur.next().unwrap().unwrap();
        (r.get(0).unwrap(), r.get(1).unwrap())
    }

    #[test]
    fn a_client_certificate_logs_in_without_a_password_in_both_key_formats() {
        let addr = need_cert!();
        // SEC1 ("EC PRIVATE KEY") and PKCS#8 ("PRIVATE KEY")
        for suffix in ["", "-pkcs8"] {
            let mut c = Connection::connect(&with_cert(&addr, suffix)).unwrap();
            let (user, dn) = who(&mut c);
            assert_eq!(user, "karez_cert");
            assert!(
                dn.contains("karez_cert"),
                "the server saw the certificate: {dn}"
            );
        }
        // the URL spelling
        let port = addr.rsplit(':').next().unwrap();
        let url = format!(
            "postgresql://karez_cert@localhost:{port}/karez?sslmode=verify-full&sslrootcert={d}/ca-{port}.crt&sslcert={d}/client-{port}.crt&sslkey={d}/client-{port}.key",
            d = certs_dir()
        );
        let mut c = Connection::connect_url(&url).unwrap();
        assert_eq!(who(&mut c).0, "karez_cert");
    }

    #[test]
    fn without_the_certificate_the_server_refuses_and_so_does_a_bad_setup() {
        let addr = need_cert!();
        // the server's rule demands a certificate
        match Connection::connect(&base(&addr)) {
            Err(Error::Server(e)) => assert_eq!(e.sqlstate(), "28000"),
            Err(Error::Io(_)) | Err(Error::Tls(_)) => {} // the alert can arrive as a closed socket
            other => panic!("{:?}", other.map(|_| ())),
        }
        // a certificate without its key, a missing file, and a file that is not a key
        let mut c = with_cert(&addr, "");
        c.ssl_key = None;
        match Connection::connect(&c) {
            Err(Error::Tls(m)) => assert!(m.contains("together"), "{m}"),
            other => panic!("{:?}", other.map(|_| ())),
        }
        let mut c = with_cert(&addr, "");
        c.ssl_key = Some("/no/such/key.pem".into());
        assert!(matches!(Connection::connect(&c), Err(Error::Tls(_))));
        let mut c = with_cert(&addr, "");
        c.ssl_key = c.ssl_cert.clone(); // a certificate where a key should be
        match Connection::connect(&c) {
            Err(Error::Tls(m)) => assert!(m.contains("sslkey"), "{m}"),
            other => panic!("{:?}", other.map(|_| ())),
        }
        let mut c = with_cert(&addr, "");
        c.ssl_cert = c.ssl_key.clone(); // a key where a certificate should be
        assert!(matches!(Connection::connect(&c), Err(Error::Tls(_))));
        // a working connection still works afterwards
        assert!(Connection::connect(&with_cert(&addr, "")).is_ok());
    }

    /// What a client sends first, seen by a listener that then closes the connection.
    fn first_bytes(negotiation: SslNegotiation) -> Vec<u8> {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut b = [0u8; 8];
            let n = s.read(&mut b).unwrap();
            b[..n].to_vec()
        });
        let mut c = Config::new("127.0.0.1", port, "u");
        c.ssl_mode = SslMode::Require;
        c.ssl_negotiation = negotiation;
        let _ = Connection::connect(&c);
        h.join().unwrap()
    }

    #[test]
    fn the_first_bytes_are_an_ssl_request_or_a_tls_client_hello() {
        // the SSLRequest message: length 8, code 80877103
        assert_eq!(
            first_bytes(SslNegotiation::Postgres),
            [0, 0, 0, 8, 0x04, 0xd2, 0x16, 0x2f]
        );
        // a TLS record: handshake (0x16), version 3.x
        let direct = first_bytes(SslNegotiation::Direct);
        assert!(
            direct.len() >= 3 && direct[0] == 0x16 && direct[1] == 3,
            "{direct:?}"
        );
    }

    #[test]
    fn a_direct_tls_handshake_skips_the_ssl_request() {
        let addr = need_cert!();
        let mut c = with_cert(&addr, "");
        c.ssl_negotiation = SslNegotiation::Direct;
        let mut conn = Connection::connect(&c).unwrap();
        assert_eq!(who(&mut conn).0, "karez_cert");
        // the setting parses from the URL
        let u = Config::from_url("postgresql://u@h/d?sslnegotiation=direct").unwrap();
        assert_eq!(u.ssl_negotiation, SslNegotiation::Direct);
        assert!(Config::from_url("postgresql://u@h/d?sslnegotiation=maybe").is_err());
    }
}
