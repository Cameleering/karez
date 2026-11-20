//! A failover while connections are open, against a real primary and standby
//! (`tests/docker/standby.yaml`, PostgreSQL 16).
//!
//! **This test destroys the fixture**: it kills the primary and promotes the standby, so the other
//! standby tests cannot run afterwards without `docker compose -f tests/docker/standby.yaml down -v`
//! and `up -d --wait`. It therefore runs only with `KAREZ_TEST_FAILOVER=1` (and the two addresses,
//! `KAREZ_TEST_PRIMARY` and `KAREZ_TEST_STANDBY`) and needs the `docker` command. Without those it
//! prints `SKIPPED` and returns, unless `KAREZ_REQUIRE_FAILOVER=1`, which makes that a failure.
//!
//! What it shows, in order: a pooled connection whose server session was terminated (the server
//! sends `FATAL` first) is replaced without an error on first use; a connection to a primary that
//! is killed fails with a transient error and is marked broken; with no primary, connecting
//! `read-write` is a transient `NoSuitableHost`; after the standby is promoted the same pool and
//! the same URL reach the new primary.

use karez_pg::{Connection, Error, Pool, Value};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::Command;
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

fn skip(why: &str) {
    eprintln!("SKIPPED (failover not verified): {why}");
    if std::env::var("KAREZ_REQUIRE_FAILOVER").as_deref() == Ok("1") {
        panic!("KAREZ_REQUIRE_FAILOVER=1 but the failover test cannot run: {why}");
    }
}

fn url(hosts: &[&str], attrs: &str) -> String {
    format!(
        "postgresql://postgres:karez_test_only_superuser_pw@{}/karez?sslmode=disable&target_session_attrs={attrs}",
        hosts.join(",")
    )
}

fn one_i64(conn: &mut Connection, sql: &str) -> i64 {
    let mut cur = conn.query(sql, &[]).unwrap();
    cur.next().unwrap().unwrap().get::<i64>(0).unwrap()
}

fn in_recovery(conn: &mut Connection) -> bool {
    let mut cur = conn.query("SELECT pg_is_in_recovery()", &[]).unwrap();
    cur.next().unwrap().unwrap().get::<bool>(0).unwrap()
}

#[test]
fn a_failover_under_open_connections_and_a_pool() {
    if std::env::var("KAREZ_TEST_FAILOVER").as_deref() != Ok("1") {
        return skip("KAREZ_TEST_FAILOVER=1 is not set (the test destroys the fixture)");
    }
    let (p, s) = match (
        reachable("KAREZ_TEST_PRIMARY"),
        reachable("KAREZ_TEST_STANDBY"),
    ) {
        (Ok(p), Ok(s)) => (p, s),
        (a, b) => return skip(&a.err().or(b.err()).unwrap_or_default()),
    };
    if Command::new("docker").arg("--version").output().is_err() {
        return skip("the docker command is not available");
    }
    let hosts = [p.as_str(), s.as_str()];
    let rw = url(&hosts, "read-write");

    // A pool that probes every idle connection, and a connection with a transaction open.
    let pool = Pool::new(karez_pg::Config::from_url(&rw).unwrap(), 2).unwrap();
    pool.set_validate_after(Duration::ZERO);
    let mut control = Connection::connect_url(&url(&[&p], "read-write")).unwrap();
    let first_pid = {
        let mut c = pool.get().unwrap();
        one_i64(&mut c, "SELECT pg_backend_pid()::int8")
    };
    // 1. the server terminates the pooled session (it sends FATAL, then closes): the next `get`
    //    must not hand it out
    control
        .execute(
            "SELECT pg_terminate_backend($1::int4)",
            &[Value::i32(first_pid as i32)],
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let second_pid = {
        let mut c = pool
            .get()
            .expect("a dead idle connection is replaced, not reported");
        one_i64(&mut c, "SELECT pg_backend_pid()::int8")
    };
    assert_ne!(
        first_pid, second_pid,
        "a new session, not the terminated one"
    );

    // 2. the primary dies: SIGKILL, as a crash
    let mut open = Connection::connect_url(&rw).unwrap();
    assert!(!in_recovery(&mut open), "the first host is the primary");
    open.execute("BEGIN", &[]).unwrap();
    open.execute("CREATE TABLE karez_failover_probe (x int)", &[])
        .unwrap();
    let out = Command::new("docker")
        .args(["kill", "karez-test-primary"])
        .output()
        .expect("docker kill");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::thread::sleep(Duration::from_millis(500));

    // the open connection fails with a transient error and is marked broken
    let err = open
        .execute("INSERT INTO karez_failover_probe VALUES (1)", &[])
        .unwrap_err();
    assert!(
        err.is_transient(),
        "a lost server is transient, got {err:?}"
    );
    assert!(open.is_broken());
    assert!(!open.is_alive());

    // 3. nobody is the primary yet (the standby is still read-only): transient, not permanent
    match Connection::connect_url(&rw) {
        Err(Error::NoSuitableHost { transient, .. }) => assert!(transient),
        other => panic!("expected NoSuitableHost, got {other:?}"),
    }
    match pool.get() {
        Err(Error::NoSuitableHost { transient, .. }) => assert!(transient),
        Err(other) => panic!("expected NoSuitableHost from the pool, got {other:?}"),
        Ok(_) => panic!("the pool handed out a connection with no primary"),
    }
    assert_eq!(
        pool.idle(),
        0,
        "the dead idle connection was dropped, not kept"
    );

    // 4. the standby is promoted: the same URL and the same pool reach the new primary
    let mut on_standby = Connection::connect_url(&url(&[&s], "any")).unwrap();
    assert!(in_recovery(&mut on_standby));
    on_standby
        .execute("SELECT pg_promote(true, 60)", &[])
        .unwrap();
    assert!(!in_recovery(&mut on_standby), "promoted");
    let mut c = Connection::connect_url(&rw).unwrap();
    assert!(!in_recovery(&mut c));
    c.execute("CREATE TABLE karez_failover_after (x int)", &[])
        .unwrap(); // writable
    let mut pooled = pool.get().expect("the pool follows the failover");
    assert!(!in_recovery(&mut pooled));
    pooled
        .execute("INSERT INTO karez_failover_after VALUES (1)", &[])
        .unwrap();
    // the transaction that was open on the dead primary is gone with it
    assert!(c
        .execute("SELECT 1 FROM karez_failover_probe", &[])
        .is_err());
}
