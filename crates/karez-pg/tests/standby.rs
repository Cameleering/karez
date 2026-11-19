//! `target_session_attrs` and `load_balance_hosts` against a real primary and a real streaming
//! standby (`tests/docker/standby.yaml`, PostgreSQL 16).
//!
//! `KAREZ_TEST_PRIMARY` and `KAREZ_TEST_STANDBY` hold `host:port` of the two servers. Without
//! them each test prints `SKIPPED` and returns, unless `KAREZ_REQUIRE_SERVER=1`, which makes a
//! missing server a failure. A skipped run verifies nothing.

use karez_pg::{Connection, Error, Value};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

fn reachable(var: &str) -> Result<String, String> {
    let addr = std::env::var(var).map_err(|_| format!("{var} is not set"))?;
    let sock = addr
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve {addr}: {e}"))?
        .next()
        .ok_or_else(|| format!("{addr} resolves to nothing"))?;
    TcpStream::connect_timeout(&sock, Duration::from_secs(2))
        .map_err(|e| format!("{addr} is not reachable: {e}"))?;
    Ok(addr)
}

fn servers() -> Option<(String, String)> {
    match (
        reachable("KAREZ_TEST_PRIMARY"),
        reachable("KAREZ_TEST_STANDBY"),
    ) {
        (Ok(p), Ok(s)) => Some((p, s)),
        (a, b) => {
            let why = a.err().or(b.err()).unwrap_or_default();
            eprintln!("SKIPPED (not verified against a real standby): {why}");
            if std::env::var("KAREZ_REQUIRE_SERVER").as_deref() == Ok("1") {
                panic!(
                    "KAREZ_REQUIRE_SERVER=1 but the primary and standby are not both there: {why}"
                );
            }
            None
        }
    }
}

fn url(hosts: &[&str], attrs: &str, extra: &str) -> String {
    format!(
        "postgresql://postgres:karez_test_only_superuser_pw@{}/karez?sslmode=disable&target_session_attrs={attrs}{extra}",
        hosts.join(",")
    )
}

/// Which server a connection reached, asked of the server itself.
fn role(conn: &mut Connection) -> &'static str {
    let mut cur = conn.query("SELECT pg_is_in_recovery()", &[]).unwrap();
    let standby = cur.next().unwrap().unwrap().get::<bool>(0).unwrap();
    drop(cur);
    if standby {
        "standby"
    } else {
        "primary"
    }
}

fn connect(hosts: &[&str], attrs: &str) -> Result<Connection, Error> {
    Connection::connect_url(&url(hosts, attrs, ""))
}

#[test]
fn each_attribute_picks_the_right_server_in_either_order() {
    let Some((p, s)) = servers() else { return };
    for order in [[p.as_str(), s.as_str()], [s.as_str(), p.as_str()]] {
        for (attrs, want) in [
            ("read-write", "primary"),
            ("primary", "primary"),
            ("read-only", "standby"),
            ("standby", "standby"),
            ("prefer-standby", "standby"),
        ] {
            let mut c = connect(&order, attrs).unwrap();
            assert_eq!(role(&mut c), want, "attrs {attrs}, hosts {order:?}");
        }
        // `any` takes the first host
        let mut c = connect(&order, "any").unwrap();
        let first = if order[0] == p { "primary" } else { "standby" };
        assert_eq!(role(&mut c), first);
    }
}

#[test]
fn the_standby_really_is_read_only_and_the_primary_is_not() {
    let Some((p, s)) = servers() else { return };
    let mut on_standby = connect(&[&s], "any").unwrap();
    let err = on_standby
        .execute("CREATE TABLE karez_standby_probe (x int)", &[])
        .unwrap_err();
    assert!(err.to_string().contains("read-only"), "{err}");
    let mut on_primary = connect(&[&p], "read-write").unwrap();
    on_primary
        .execute("CREATE TEMP TABLE karez_standby_probe (x int)", &[])
        .unwrap();
    // and a query still works on a connection chosen as read-only
    let mut c = connect(&[&p, &s], "read-only").unwrap();
    let mut cur = c.query("SELECT $1::int8", &[Value::i64(7)]).unwrap();
    assert_eq!(cur.next().unwrap().unwrap().get::<i64>(0).unwrap(), 7);
}

#[test]
fn a_missing_kind_of_server_is_a_transient_error_and_prefer_standby_falls_back() {
    let Some((p, s)) = servers() else { return };
    let (p, s) = (p.as_str(), s.as_str());
    for (hosts, attrs) in [
        (&[p][..], "standby"),
        (&[p][..], "read-only"),
        (&[s][..], "read-write"),
        (&[s][..], "primary"),
    ] {
        match connect(hosts, attrs) {
            Err(Error::NoSuitableHost {
                transient,
                attempts,
            }) => {
                assert!(
                    transient,
                    "a promotion or failover can change the answer: {attempts}"
                );
                assert!(attempts.contains(hosts[0]), "{attempts}");
            }
            Err(e) => panic!("hosts {hosts:?}, attrs {attrs}: wrong error {e}"),
            Ok(_) => {
                panic!("hosts {hosts:?}, attrs {attrs}: connected to the wrong kind of server")
            }
        }
    }
    // prefer-standby with only a primary falls back to it
    let mut c = connect(&[p], "prefer-standby").unwrap();
    assert_eq!(role(&mut c), "primary");
    // a dead first host is skipped
    let mut c = connect(&["127.0.0.1:1", p], "read-write").unwrap();
    assert_eq!(role(&mut c), "primary");
}

#[test]
fn random_order_reaches_both_servers_about_equally() {
    let Some((p, s)) = servers() else { return };
    let n = 200;
    let mut on_primary = 0;
    for _ in 0..n {
        let mut c =
            Connection::connect_url(&url(&[&p, &s], "any", "&load_balance_hosts=random")).unwrap();
        if role(&mut c) == "primary" {
            on_primary += 1;
        }
    }
    // Binomial(200, 1/2): the standard deviation is 7; 60..=140 is 4 standard deviations.
    assert!(
        (60..=140).contains(&on_primary),
        "{on_primary} of {n} connections reached the primary"
    );
    // and without it the first host is always taken
    for _ in 0..20 {
        let mut c = connect(&[&p, &s], "any").unwrap();
        assert_eq!(role(&mut c), "primary");
    }
}
