//! Multi-host connection and `target_session_attrs`, against scripted in-process servers (no
//! PostgreSQL needed). Each mock server answers the startup like a PostgreSQL server of the kind
//! its `Kind` says and counts the queries it receives, so the tests can tell whether the client
//! decided from the reported parameters or had to ask.

use karez_pg::{Config, Connection, Error, LoadBalance, SslMode, TargetSessionAttrs};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

fn msg(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
    v.extend_from_slice(body);
    v
}

fn param(name: &str, value: &str) -> Vec<u8> {
    msg(b'S', format!("{name}\0{value}\0").as_bytes())
}

/// What the mock server is.
#[derive(Clone, Copy)]
enum Kind {
    /// PostgreSQL 14+: reports `in_hot_standby` and `default_transaction_read_only`.
    Modern { standby: bool, default_ro: bool },
    /// An older server: reports neither, and answers `SHOW transaction_read_only` and
    /// `SELECT pg_is_in_recovery()` with these values.
    Legacy { standby: bool, ro: bool },
    /// Rejects the login with SQLSTATE 28P01.
    BadPassword,
}

struct Mock {
    port: u16,
    queries: Arc<AtomicUsize>,
    pid: i32,
}

fn row_description(oid: u32) -> Vec<u8> {
    let mut b = 1u16.to_be_bytes().to_vec();
    b.extend_from_slice(b"c\0");
    b.extend_from_slice(&0u32.to_be_bytes());
    b.extend_from_slice(&0i16.to_be_bytes());
    b.extend_from_slice(&oid.to_be_bytes());
    b.extend_from_slice(&(-1i16).to_be_bytes());
    b.extend_from_slice(&(-1i32).to_be_bytes());
    b.extend_from_slice(&1i16.to_be_bytes());
    b
}

fn serve(mut s: TcpStream, kind: Kind, pid: i32, queries: Arc<AtomicUsize>) {
    let mut len = [0u8; 4];
    if s.read_exact(&mut len).is_err() {
        return;
    }
    let n = i32::from_be_bytes(len) as usize - 4;
    let mut startup = vec![0; n];
    if s.read_exact(&mut startup).is_err() {
        return;
    }
    let mut out = Vec::new();
    if let Kind::BadPassword = kind {
        out.extend(msg(
            b'E',
            b"SFATAL\0C28P01\0Mpassword authentication failed\0\0",
        ));
        let _ = s.write_all(&out);
        return;
    }
    out.extend(msg(b'R', &[0, 0, 0, 0]));
    out.extend(param("server_version", "16.0"));
    if let Kind::Modern {
        standby,
        default_ro,
    } = kind
    {
        out.extend(param("in_hot_standby", if standby { "on" } else { "off" }));
        out.extend(param(
            "default_transaction_read_only",
            if default_ro { "on" } else { "off" },
        ));
    }
    let mut key = pid.to_be_bytes().to_vec();
    key.extend_from_slice(&[1, 2, 3, 4]);
    out.extend(msg(b'K', &key));
    out.extend(msg(b'Z', b"I"));
    if s.write_all(&out).is_err() {
        return;
    }
    let mut sql = String::new();
    loop {
        let mut head = [0u8; 5];
        if s.read_exact(&mut head).is_err() {
            return;
        }
        let n = i32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize - 4;
        let mut body = vec![0; n];
        if s.read_exact(&mut body).is_err() {
            return;
        }
        let mut out = Vec::new();
        match head[0] {
            b'X' => return,
            b'S' => out.extend(msg(b'Z', b"I")),
            b'P' => {
                queries.fetch_add(1, Ordering::SeqCst);
                // Parse: statement name, query text, parameter count
                let text = &body[1..];
                let end = text.iter().position(|b| *b == 0).unwrap_or(0);
                let text = &text[end + 1..];
                let end = text.iter().position(|b| *b == 0).unwrap_or(0);
                sql = String::from_utf8_lossy(&text[..end]).into_owned();
                out.extend(msg(b'1', b""));
            }
            b'D' => {
                let oid = if sql.contains("pg_is_in_recovery") {
                    16
                } else {
                    25
                };
                out.extend(msg(b't', &[0, 0]));
                out.extend(msg(b'T', &row_description(oid)));
            }
            b'B' => out.extend(msg(b'2', b"")),
            b'E' => {
                let (standby, ro) = match kind {
                    Kind::Legacy { standby, ro } => (standby, ro),
                    _ => (false, false),
                };
                let value: Vec<u8> = if sql.contains("pg_is_in_recovery") {
                    vec![u8::from(standby)]
                } else {
                    (if ro { "on" } else { "off" }).as_bytes().to_vec()
                };
                let mut row = 1u16.to_be_bytes().to_vec();
                row.extend_from_slice(&(value.len() as i32).to_be_bytes());
                row.extend_from_slice(&value);
                out.extend(msg(b'D', &row));
                out.extend(msg(b'C', b"SELECT 1\0"));
            }
            other => panic!("mock got unexpected message {}", other as char),
        }
        if s.write_all(&out).is_err() {
            return;
        }
    }
}

fn start(kind: Kind, pid: i32) -> Mock {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let queries = Arc::new(AtomicUsize::new(0));
    let q = queries.clone();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let q = q.clone();
            thread::spawn(move || serve(s, kind, pid, q));
        }
    });
    Mock { port, queries, pid }
}

/// A port nothing listens on.
fn dead_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

fn cfg(ports: &[u16], attrs: TargetSessionAttrs) -> Config {
    let mut c = Config::new("127.0.0.1", ports[0], "u");
    for p in &ports[1..] {
        c.add_host("127.0.0.1", *p);
    }
    c.ssl_mode = SslMode::Disable;
    c.auth.trust = true;
    c.target_session_attrs = attrs;
    c
}

const MODERN_RW: Kind = Kind::Modern {
    standby: false,
    default_ro: false,
};
const MODERN_RO: Kind = Kind::Modern {
    standby: false,
    default_ro: true,
};
const MODERN_STANDBY: Kind = Kind::Modern {
    standby: true,
    default_ro: false,
};

#[test]
fn an_unreachable_first_host_is_skipped() {
    let ok = start(MODERN_RW, 11);
    let c = Connection::connect(&cfg(&[dead_port(), ok.port], TargetSessionAttrs::Any)).unwrap();
    assert_eq!(c.process_id(), ok.pid);
}

#[test]
fn a_refused_login_moves_on_to_the_next_host() {
    let bad = start(Kind::BadPassword, 21);
    let ok = start(MODERN_RW, 22);
    let c = Connection::connect(&cfg(&[bad.port, ok.port], TargetSessionAttrs::Any)).unwrap();
    assert_eq!(c.process_id(), ok.pid);
}

#[test]
fn hosts_are_tried_in_the_order_written() {
    let a = start(MODERN_RW, 31);
    let b = start(MODERN_RW, 32);
    for _ in 0..10 {
        let c = Connection::connect(&cfg(&[a.port, b.port], TargetSessionAttrs::Any)).unwrap();
        assert_eq!(c.process_id(), a.pid);
    }
}

#[test]
fn read_write_skips_a_read_only_host_without_asking_it_anything() {
    let ro = start(MODERN_RO, 41);
    let standby = start(MODERN_STANDBY, 42);
    let rw = start(MODERN_RW, 43);
    let c = Connection::connect(&cfg(
        &[ro.port, standby.port, rw.port],
        TargetSessionAttrs::ReadWrite,
    ))
    .unwrap();
    assert_eq!(c.process_id(), rw.pid);
    for m in [&ro, &standby, &rw] {
        assert_eq!(
            m.queries.load(Ordering::SeqCst),
            0,
            "the reported parameters were enough"
        );
    }
}

#[test]
fn read_only_accepts_a_standby_and_a_read_only_default() {
    let rw = start(MODERN_RW, 51);
    let standby = start(MODERN_STANDBY, 52);
    let c =
        Connection::connect(&cfg(&[rw.port, standby.port], TargetSessionAttrs::ReadOnly)).unwrap();
    assert_eq!(c.process_id(), standby.pid);
    let ro = start(MODERN_RO, 53);
    let c = Connection::connect(&cfg(&[rw.port, ro.port], TargetSessionAttrs::ReadOnly)).unwrap();
    assert_eq!(c.process_id(), ro.pid);
}

#[test]
fn primary_and_standby_look_at_hot_standby_only() {
    // a read-only default on a primary does not make it a standby
    let ro_primary = start(MODERN_RO, 61);
    let standby = start(MODERN_STANDBY, 62);
    let c = Connection::connect(&cfg(
        &[standby.port, ro_primary.port],
        TargetSessionAttrs::Primary,
    ))
    .unwrap();
    assert_eq!(c.process_id(), ro_primary.pid);
    let c = Connection::connect(&cfg(
        &[ro_primary.port, standby.port],
        TargetSessionAttrs::Standby,
    ))
    .unwrap();
    assert_eq!(c.process_id(), standby.pid);
}

#[test]
fn prefer_standby_takes_a_standby_if_any_host_is_one_else_the_first_host() {
    let primary = start(MODERN_RW, 71);
    let standby = start(MODERN_STANDBY, 72);
    let c = Connection::connect(&cfg(
        &[primary.port, standby.port],
        TargetSessionAttrs::PreferStandby,
    ))
    .unwrap();
    assert_eq!(c.process_id(), standby.pid);
    let other = start(MODERN_RW, 73);
    let c = Connection::connect(&cfg(
        &[primary.port, other.port],
        TargetSessionAttrs::PreferStandby,
    ))
    .unwrap();
    assert_eq!(
        c.process_id(),
        primary.pid,
        "no standby anywhere: any host, in order"
    );
}

#[test]
fn an_older_server_is_asked_with_one_query() {
    let ro = start(
        Kind::Legacy {
            standby: false,
            ro: true,
        },
        81,
    );
    let rw = start(
        Kind::Legacy {
            standby: false,
            ro: false,
        },
        82,
    );
    let c = Connection::connect(&cfg(&[ro.port, rw.port], TargetSessionAttrs::ReadWrite)).unwrap();
    assert_eq!(c.process_id(), rw.pid);
    assert_eq!(ro.queries.load(Ordering::SeqCst), 1);
    assert_eq!(rw.queries.load(Ordering::SeqCst), 1);

    let standby = start(
        Kind::Legacy {
            standby: true,
            ro: true,
        },
        83,
    );
    let c =
        Connection::connect(&cfg(&[rw.port, standby.port], TargetSessionAttrs::Standby)).unwrap();
    assert_eq!(c.process_id(), standby.pid);
    let c =
        Connection::connect(&cfg(&[standby.port, rw.port], TargetSessionAttrs::Primary)).unwrap();
    assert_eq!(c.process_id(), rw.pid);
}

#[test]
fn nothing_suitable_lists_every_attempt() {
    let ro = start(MODERN_RO, 91);
    let bad = start(Kind::BadPassword, 92);
    let dead = dead_port();
    let err = Connection::connect(&cfg(
        &[dead, bad.port, ro.port],
        TargetSessionAttrs::ReadWrite,
    ))
    .unwrap_err();
    match &err {
        Error::NoSuitableHost {
            attempts,
            transient,
        } => {
            assert_eq!(attempts.matches("127.0.0.1:").count(), 3, "{attempts}");
            assert!(
                attempts.contains(&format!("127.0.0.1:{dead}")),
                "{attempts}"
            );
            assert!(attempts.contains("28P01"), "{attempts}");
            assert!(attempts.contains("not read-write"), "{attempts}");
            assert!(
                *transient,
                "an unreachable host and a read-only host may pass"
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(err.is_transient());
    assert!(!err.is_fatal());
}

#[test]
fn only_permanent_failures_are_not_transient() {
    let a = start(Kind::BadPassword, 101);
    let b = start(Kind::BadPassword, 102);
    let err = Connection::connect(&cfg(&[a.port, b.port], TargetSessionAttrs::Any)).unwrap_err();
    assert!(
        matches!(
            err,
            Error::NoSuitableHost {
                transient: false,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn one_host_keeps_its_own_error() {
    // single host, any: the exact error, as before multi-host support
    let c = cfg(&[dead_port()], TargetSessionAttrs::Any);
    assert!(matches!(Connection::connect(&c), Err(Error::Io(_))));
    // single host, attributes it cannot meet: NoSuitableHost, but a connection failure is still the I/O error
    let ro = start(MODERN_RO, 111);
    let err = Connection::connect(&cfg(&[ro.port], TargetSessionAttrs::ReadWrite)).unwrap_err();
    assert!(matches!(err, Error::NoSuitableHost { .. }), "{err:?}");
    let c = cfg(&[dead_port()], TargetSessionAttrs::ReadWrite);
    assert!(matches!(Connection::connect(&c), Err(Error::Io(_))));
}

#[test]
fn random_order_uses_every_host() {
    let a = start(MODERN_RW, 121);
    let b = start(MODERN_RW, 122);
    let mut c = cfg(&[a.port, b.port], TargetSessionAttrs::Any);
    c.load_balance_hosts = LoadBalance::Random;
    let (mut na, mut nb) = (0, 0);
    for _ in 0..80 {
        let conn = Connection::connect(&c).unwrap();
        if conn.process_id() == a.pid {
            na += 1;
        } else {
            nb += 1;
        }
    }
    // each host is first with probability 1/2: all 80 landing on one is a 2^-79 event
    assert!(na > 0 && nb > 0, "a: {na}, b: {nb}");
}

#[test]
fn urls_with_several_hosts_connect() {
    let dead = dead_port();
    let ok = start(MODERN_RW, 131);
    let url = format!(
        "postgresql://u@127.0.0.1:{dead},127.0.0.1:{}/d?sslmode=disable&auth=trust&target_session_attrs=read-write",
        ok.port
    );
    let c = Connection::connect_url(&url).unwrap();
    assert_eq!(c.process_id(), ok.pid);
}
