//! The latency floor (perf-program.md 3.2): one connection, closed loop, pre-encoded protocol messages, no decoding.
//! Compared in the same process and against the same server with karez-blocking doing the same request.
//! Trust authentication only (a test server on localhost); this is a measuring tool, not a client.
//!
//!   floor URL [requests]        URL like postgresql://bench@127.0.0.1:55441/bench?sslmode=disable

use karez_pg::{Config, Connection, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Instant;

fn msg(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend_from_slice(&((body.len() as u32 + 4).to_be_bytes()));
    v.extend_from_slice(body);
    v
}

fn read_until_z(s: &mut TcpStream, buf: &mut Vec<u8>) {
    buf.clear();
    let mut tmp = [0u8; 16384];
    loop {
        let n = s.read(&mut tmp).expect("read");
        assert!(n > 0, "closed");
        buf.extend_from_slice(&tmp[..n]);
        // scan messages; done when a complete ReadyForQuery ('Z') is the last one
        let mut p = 0;
        let mut z = false;
        while p + 5 <= buf.len() {
            let len = u32::from_be_bytes(buf[p + 1..p + 5].try_into().unwrap()) as usize;
            if p + 1 + len > buf.len() {
                break;
            }
            z = buf[p] == b'Z';
            if buf[p] == b'E' {
                panic!("server error: {}", String::from_utf8_lossy(&buf[p + 5..p + 1 + len]));
            }
            p += 1 + len;
        }
        if z && p == buf.len() {
            return;
        }
    }
}

fn pct(v: &mut [u64], q: f64) -> f64 {
    v.sort_unstable();
    v[((v.len() as f64 * q) as usize).min(v.len() - 1)] as f64 / 1e3
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let url = a.get(1).expect("url");
    let n: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let hostport = url.split('@').nth(1).unwrap().split('/').next().unwrap();
    let user = url.split("://").nth(1).unwrap().split('@').next().unwrap().split(':').next().unwrap().to_string();
    let cases: [(&str, &str, bool); 2] = [
        ("S1 SELECT 1", "SELECT 1", false),
        ("S2 point select, 10 columns", "SELECT id, k1, k2, flag, score, name, note, ref, tag, big FROM bench_item WHERE id = $1", true),
    ];
    for (label, sql, has_param) in cases {
        // ---- the floor: raw socket
        let mut s = TcpStream::connect(hostport).expect("connect");
        s.set_nodelay(true).unwrap();
        let mut start = Vec::new();
        start.extend_from_slice(&196608u32.to_be_bytes());
        start.extend_from_slice(format!("user\0{user}\0database\0bench\0\0").as_bytes());
        let mut startup = (start.len() as u32 + 4).to_be_bytes().to_vec();
        startup.extend_from_slice(&start);
        s.write_all(&startup).unwrap();
        let mut buf = Vec::new();
        read_until_z(&mut s, &mut buf);
        let mut parse = b"s\0".to_vec();
        parse.extend_from_slice(sql.as_bytes());
        parse.push(0);
        if has_param {
            parse.extend_from_slice(&1u16.to_be_bytes());
            parse.extend_from_slice(&20u32.to_be_bytes());
        } else {
            parse.extend_from_slice(&0u16.to_be_bytes());
        }
        let mut first = msg(b'P', &parse);
        first.extend(msg(b'S', &[]));
        s.write_all(&first).unwrap();
        read_until_z(&mut s, &mut buf);
        let mut reqs: Vec<Vec<u8>> = Vec::new();
        for i in 0..1000i64 {
            let mut bind = b"\0s\0".to_vec();
            if has_param {
                bind.extend_from_slice(&1u16.to_be_bytes()); // one parameter format code ...
                bind.extend_from_slice(&1u16.to_be_bytes()); // ... binary
                bind.extend_from_slice(&1u16.to_be_bytes()); // one parameter
                bind.extend_from_slice(&8u32.to_be_bytes());
                bind.extend_from_slice(&(i * 97 % 100_000 + 1).to_be_bytes());
            } else {
                bind.extend_from_slice(&0u16.to_be_bytes());
                bind.extend_from_slice(&0u16.to_be_bytes());
            }
            bind.extend_from_slice(&1u16.to_be_bytes());
            bind.extend_from_slice(&1u16.to_be_bytes()); // results in binary
            let mut r = msg(b'B', &bind);
            r.extend(msg(b'E', b"\0\0\0\0\0"));
            r.extend(msg(b'S', &[]));
            reqs.push(r);
        }
        for i in 0..5000 {
            s.write_all(&reqs[i % 1000]).unwrap();
            read_until_z(&mut s, &mut buf);
        }
        let mut lat = Vec::with_capacity(n);
        for i in 0..n {
            let t = Instant::now();
            s.write_all(&reqs[i % 1000]).unwrap();
            read_until_z(&mut s, &mut buf);
            lat.push(t.elapsed().as_nanos() as u64);
        }
        let (f50, f99) = (pct(&mut lat.clone(), 0.5), pct(&mut lat, 0.99));
        // ---- karez-blocking, same request
        let c = Config::from_url(&format!("{url}&auth=trust")).expect("url");
        let mut conn = Connection::connect(&c).expect("karez connect");
        let mut run = |i: usize| {
            if has_param {
                let mut cur = conn.query(sql, &[Value::i64((i as i64) * 97 % 100_000 + 1)]).unwrap();
                while let Some(r) = cur.next().unwrap() {
                    std::hint::black_box(r.get::<i64>(0).unwrap());
                }
            } else {
                let mut cur = conn.query(sql, &[]).unwrap();
                while let Some(r) = cur.next().unwrap() {
                    std::hint::black_box(r.get::<i32>(0).unwrap());
                }
            }
        };
        for i in 0..5000 {
            run(i);
        }
        let mut lat2 = Vec::with_capacity(n);
        for i in 0..n {
            let t = Instant::now();
            run(i);
            lat2.push(t.elapsed().as_nanos() as u64);
        }
        let (k50, k99) = (pct(&mut lat2.clone(), 0.5), pct(&mut lat2, 0.99));
        println!(
            "{label}: floor p50 {f50:.1} us p99 {f99:.1} us | karez-blocking p50 {k50:.1} us p99 {k99:.1} us | karez/floor p50 {:.3}",
            k50 / f50
        );
    }
}
