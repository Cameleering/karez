//! Connection tests against a scripted in-process server (no PostgreSQL needed). They check the
//! client's protocol handling; they are not a substitute for the real-server tests.

use karez_pg::{Config, Connection, Error, SslMode, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

fn msg(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
    v.extend_from_slice(body);
    v
}

fn row_description() -> Vec<u8> {
    let mut b = 2u16.to_be_bytes().to_vec();
    for (name, oid) in [("n", 23u32), ("s", 25u32)] {
        b.extend_from_slice(name.as_bytes());
        b.push(0);
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0i16.to_be_bytes());
        b.extend_from_slice(&oid.to_be_bytes());
        b.extend_from_slice(&(-1i16).to_be_bytes());
        b.extend_from_slice(&(-1i32).to_be_bytes());
        b.extend_from_slice(&1i16.to_be_bytes());
    }
    b
}

fn data_row(i: i32) -> Vec<u8> {
    let s = format!("row {i}");
    let mut b = 2u16.to_be_bytes().to_vec();
    b.extend_from_slice(&4i32.to_be_bytes());
    b.extend_from_slice(&i.to_be_bytes());
    b.extend_from_slice(&(s.len() as i32).to_be_bytes());
    b.extend_from_slice(s.as_bytes());
    b
}

/// Serve one connection. `rows` rows come back for every executed statement; a Parse of the SQL
/// text `bad` fails with 42601.
fn serve(mut s: TcpStream, rows: i32, auth_ok_first: bool) {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).unwrap();
    let n = i32::from_be_bytes(len) as usize - 4;
    let mut startup = vec![0; n];
    s.read_exact(&mut startup).unwrap();
    if auth_ok_first {
        s.write_all(&msg(b'R', &[0, 0, 0, 0])).unwrap();
    }
    s.write_all(&msg(b'S', b"server_version\x0016.0\0"))
        .unwrap();
    s.write_all(&msg(b'K', &[0, 0, 0, 42, 1, 2, 3, 4])).unwrap();
    s.write_all(&msg(b'Z', b"I")).unwrap();
    let mut skipping = false;
    loop {
        let mut head = [0u8; 5];
        if s.read_exact(&mut head).is_err() {
            return;
        }
        let n = i32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize - 4;
        let mut body = vec![0; n];
        s.read_exact(&mut body).unwrap();
        let mut out = Vec::new();
        match head[0] {
            b'X' => return,
            b'S' => {
                skipping = false;
                out.extend(msg(b'Z', b"I"));
            }
            _ if skipping => {}
            b'P' => {
                if body.windows(3).any(|w| w == b"bad") {
                    out.extend(msg(b'E', b"SERROR\0C42601\0Msyntax error\0\0"));
                    skipping = true;
                } else {
                    out.extend(msg(b'1', b""));
                }
            }
            b'D' => {
                out.extend(msg(b't', &[0, 0]));
                out.extend(msg(b'T', &row_description()));
            }
            b'B' => out.extend(msg(b'2', b"")),
            b'E' => {
                for i in 0..rows {
                    out.extend(msg(b'D', &data_row(i)));
                }
                out.extend(msg(b'C', format!("SELECT {rows}\0").as_bytes()));
            }
            other => panic!("mock got unexpected message {}", other as char),
        }
        s.write_all(&out).unwrap();
    }
}

fn start(rows: i32, auth_ok_first: bool) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    thread::spawn(move || {
        if let Ok((s, _)) = l.accept() {
            serve(s, rows, auth_ok_first);
        }
    });
    port
}

fn cfg(port: u16, trust: bool) -> Config {
    let mut c = Config::new("127.0.0.1", port, "u");
    c.ssl_mode = SslMode::Disable;
    c.auth.trust = trust;
    c
}

#[test]
fn trust_is_refused_unless_allowed() {
    let port = start(0, true);
    match Connection::connect(&cfg(port, false)) {
        Err(Error::Auth(m)) => assert!(m.contains("auth=trust"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn plaintext_must_be_explicit() {
    // The default is verified TLS; plaintext needs an explicit sslmode=disable.
    let c = Config::new("127.0.0.1", 1, "u");
    assert_eq!(c.ssl_mode, SslMode::VerifyFull);
    #[cfg(not(feature = "tls"))]
    assert!(matches!(Connection::connect(&c), Err(Error::Tls(_))));
}

#[test]
fn rows_span_several_chunks_and_both_views_agree() {
    let total = 2500;
    // Row view.
    let port = start(total, true);
    let mut conn = Connection::connect(&cfg(port, true)).unwrap();
    assert_eq!(conn.process_id(), 42);
    assert_eq!(conn.server_param("server_version"), Some("16.0"));
    let mut by_row = Vec::new();
    {
        let mut cur = conn.query("SELECT n, s FROM t", &[]).unwrap();
        while let Some(r) = cur.next().unwrap() {
            by_row.push((r.get::<i32>(0).unwrap(), r.get::<String>(1).unwrap()));
        }
        assert_eq!(cur.command_tag(), Some("SELECT 2500"));
    }
    assert_eq!(by_row.len(), total as usize);
    assert_eq!(by_row[2499], (2499, "row 2499".to_string()));

    // Chunk view, over a fresh connection to a fresh mock.
    let port = start(total, true);
    let mut conn = Connection::connect(&cfg(port, true)).unwrap();
    let mut by_chunk = Vec::new();
    let mut chunks = 0;
    {
        let mut cur = conn.query("SELECT n, s FROM t", &[]).unwrap();
        while let Some(c) = cur.next_chunk().unwrap() {
            chunks += 1;
            assert!(c.rows() <= 1024);
            for i in 0..c.rows() {
                let r = c.row(i).unwrap();
                by_chunk.push((r.get::<i32>(0).unwrap(), r.get::<String>(1).unwrap()));
            }
        }
    }
    assert_eq!(chunks, 3, "2500 rows at 1024 per chunk");
    assert_eq!(by_chunk, by_row);
}

#[test]
fn dropping_a_cursor_early_keeps_the_connection_usable() {
    let port = start(3000, true);
    let mut conn = Connection::connect(&cfg(port, true)).unwrap();
    {
        let mut cur = conn.query("SELECT n, s FROM t", &[]).unwrap();
        let first = cur.next().unwrap().unwrap().get::<i32>(0).unwrap();
        assert_eq!(first, 0);
    } // dropped with ~3000 rows unread
    assert!(!conn.is_broken());
    let mut cur = conn.query("SELECT n, s FROM t", &[]).unwrap(); // cached statement
    assert!(cur.next().unwrap().is_some());
    drop(cur);
    assert_eq!(conn.cached_statements(), 1);
}

#[test]
fn server_error_has_sqlstate_and_connection_survives() {
    let port = start(1, true);
    let mut conn = Connection::connect(&cfg(port, true)).unwrap();
    match conn.query("bad", &[]) {
        Err(e) => {
            assert_eq!(e.sqlstate(), Some("42601"));
            assert!(e.to_string().contains("syntax error"));
            assert!(!e.is_fatal());
        }
        Ok(_) => panic!("expected an error"),
    }
    assert!(!conn.is_broken());
    let mut cur = conn.query("SELECT n, s FROM t", &[]).unwrap();
    assert!(cur.next().unwrap().is_some());
}

#[test]
fn parameters_are_checked_against_a_prepared_statement() {
    let port = start(1, true);
    let mut conn = Connection::connect(&cfg(port, true)).unwrap();
    let stmt = conn.prepare("SELECT n, s FROM t", &[]).unwrap();
    assert!(conn.query_prepared(&stmt, &[Value::i32(1)]).is_err());
    assert_eq!(stmt.fields().len(), 2);
}

#[test]
fn a_closed_socket_is_an_error_not_a_hang() {
    // The mock accepts, answers the handshake, then the test drops the server side mid-stream.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut buf = [0u8; 4];
        s.read_exact(&mut buf).unwrap();
        let n = i32::from_be_bytes(buf) as usize - 4;
        let mut b = vec![0; n];
        s.read_exact(&mut b).unwrap();
        s.write_all(&msg(b'R', &[0, 0, 0, 0])).unwrap();
        s.write_all(&msg(b'Z', b"I")).unwrap();
        let mut head = [0u8; 5];
        s.read_exact(&mut head).unwrap(); // Parse
                                          // close without answering
    });
    let mut conn = Connection::connect(&cfg(port, true)).unwrap();
    let e = conn.query("SELECT 1", &[]).unwrap_err();
    assert!(e.is_fatal(), "{e}");
    assert!(conn.is_broken());
    assert!(matches!(conn.query("SELECT 1", &[]), Err(Error::Broken)));
}

/// A server that behaves like PostgreSQL under back-pressure: after the first Execute it writes a
/// large result and does not read another byte until it has finished writing it. A client that
/// writes its whole batch before it reads blocks forever once the socket buffers fill.
fn serve_stalling(mut s: TcpStream, flood_rows: usize) {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).unwrap();
    let mut startup = vec![0; i32::from_be_bytes(len) as usize - 4];
    s.read_exact(&mut startup).unwrap();
    s.write_all(&msg(b'R', &[0, 0, 0, 0])).unwrap();
    s.write_all(&msg(b'K', &[0, 0, 0, 42, 1, 2, 3, 4])).unwrap();
    s.write_all(&msg(b'Z', b"I")).unwrap();
    let mut first_execute = true;
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
            b'P' => out.extend(msg(b'1', b"")),
            b'D' => {
                out.extend(msg(b't', &[0, 1, 0, 0, 0, 25]));
                let mut t = 1u16.to_be_bytes().to_vec();
                t.extend_from_slice(b"s\0");
                t.extend_from_slice(&0u32.to_be_bytes());
                t.extend_from_slice(&0i16.to_be_bytes());
                t.extend_from_slice(&25u32.to_be_bytes());
                t.extend_from_slice(&(-1i16).to_be_bytes());
                t.extend_from_slice(&(-1i32).to_be_bytes());
                t.extend_from_slice(&1i16.to_be_bytes());
                out.extend(msg(b'T', &t));
            }
            b'B' => out.extend(msg(b'2', b"")),
            b'E' => {
                if first_execute {
                    first_execute = false;
                    // 1 MiB per row; written while the client is still sending its batch.
                    let mut row = 1u16.to_be_bytes().to_vec();
                    row.extend_from_slice(&(1_000_000i32).to_be_bytes());
                    row.extend(std::iter::repeat_n(b'y', 1_000_000));
                    let row = msg(b'D', &row);
                    for _ in 0..flood_rows {
                        if s.write_all(&row).is_err() {
                            return;
                        }
                    }
                }
                out.extend(msg(b'C', b"SELECT 1\0"));
            }
            b'S' => out.extend(msg(b'Z', b"I")),
            other => panic!("mock got unexpected message {}", other as char),
        }
        if s.write_all(&out).is_err() {
            return;
        }
    }
}

#[test]
fn execute_many_does_not_deadlock_when_the_server_stops_reading() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    thread::spawn(move || {
        if let Ok((s, _)) = l.accept() {
            serve_stalling(s, 40);
        }
    });
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let r = (|| -> Result<Vec<u64>, Error> {
            let mut c = Connection::connect(&cfg(port, true))?;
            let big = "x".repeat(2_000_000);
            let params = [Value::text(&big)];
            let items: Vec<(&str, &[Value])> =
                (0..40).map(|_| ("SELECT $1", &params[..])).collect();
            c.execute_many(&items)
        })();
        let _ = tx.send(r);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(60)) {
        Ok(r) => assert_eq!(r.unwrap(), vec![1u64; 40]),
        Err(_) => panic!("execute_many is stuck: it wrote its whole batch before reading"),
    }
}
