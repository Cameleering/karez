//! SCRAM logins with a password PostgreSQL normalises (SASLprep, RFC 4013), against a real server
//! (`KAREZ_TEST_PG`, tests/docker/README.md).
//!
//! The test makes a role `karez_prep` whose password is stored as `U+2168` (ROMAN NUMERAL NINE); the
//! server prepares it to `IX` before it derives the verifier. A client that sends `IX`, `U+2168` or
//! `I` + soft hyphen + `X` is the same password once prepared. Without the `saslprep` feature only
//! `IX` can log in (and the error for the others says why); with it all three can.

use karez_pg::{Config, Connection, Error, SslMode};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

fn server() -> Option<String> {
    let why = match std::env::var("KAREZ_TEST_PG") {
        Err(_) => "KAREZ_TEST_PG is not set".to_string(),
        Ok(addr) => match addr
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next())
            .map(|s| TcpStream::connect_timeout(&s, Duration::from_secs(2)))
        {
            Some(Ok(_)) => return Some(addr),
            _ => format!("{addr} is not reachable"),
        },
    };
    eprintln!("SKIPPED (not verified against a real server): {why}");
    if std::env::var("KAREZ_REQUIRE_SERVER").as_deref() == Ok("1") {
        panic!("KAREZ_REQUIRE_SERVER=1 but there is no server: {why}");
    }
    None
}

fn cfg(addr: &str, user: &str, password: &str) -> Config {
    let (host, port) = addr.rsplit_once(':').unwrap();
    let mut c = Config::new(host, port.parse().unwrap(), user);
    c.database = "karez".into();
    c.ssl_mode = SslMode::Disable;
    c.password(password);
    c
}

#[test]
fn a_password_that_sasl_prep_changes_logs_in_with_the_feature_and_explains_itself_without() {
    let Some(addr) = server() else { return };
    let mut admin =
        Connection::connect(&cfg(&addr, "postgres", "karez_test_only_superuser_pw")).unwrap();
    admin.execute("SELECT pg_reload_conf()", &[]).unwrap(); // the pg_hba rule for karez_prep
    admin
        .execute("DROP ROLE IF EXISTS karez_prep", &[])
        .unwrap();
    // U&'\2168': the server stores the verifier of SASLprep("Ⅸ") = "IX"
    admin
        .execute(r"CREATE ROLE karez_prep LOGIN PASSWORD U&'\2168'", &[])
        .unwrap();
    std::thread::sleep(Duration::from_millis(300)); // the reload
    let login = |pw: &str| Connection::connect(&cfg(&addr, "karez_prep", pw));

    // the ASCII form needs nothing
    login("IX").expect("IX is the prepared password");

    for (what, pw) in [("U+2168", "\u{2168}"), ("soft hyphen", "I\u{00AD}X")] {
        let r = login(pw);
        if cfg!(feature = "saslprep") {
            r.unwrap_or_else(|e| panic!("{what} should be prepared to IX: {e}"));
        } else {
            match r {
                Err(Error::Server(e)) => {
                    assert_eq!(e.sqlstate(), "28P01", "{what}");
                    let hint = e.fields().hint.clone().unwrap_or_default();
                    assert!(hint.contains("saslprep"), "{what}: {hint:?}");
                }
                other => panic!("{what}: expected a password failure, got {other:?}"),
            }
        }
    }
    // a wrong password stays wrong either way, with or without the feature
    assert!(login("IY").is_err());
    assert!(login("\u{2169}").is_err()); // X, not IX
    admin.execute("DROP ROLE karez_prep", &[]).unwrap();
}
