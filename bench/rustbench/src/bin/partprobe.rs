//! Does a table scan scale from one connection to two? (perf-program.md, the k=2 question)
//! Reads the taxi table through karez chunks without any fold, as: the whole table on one connection, each half alone,
//! both halves at once on two connections, and the whole table split in four on four connections. Wall time only.
//!
//!   partprobe URL [rounds]

use karez_pg::{Config, Connection, Value};
use std::time::Instant;

const COLS: &str = "trip_id, vendor, pickup, dropoff, passengers, distance, ratecode, flag, pu, do_, payment, fare, extra, tip, tolls, total, congestion FROM taxi_trips";

fn scan(conn: &mut Connection, lo: Option<i64>, hi: Option<i64>) -> u64 {
    let mut rows = 0u64;
    let mut cur = match (lo, hi) {
        (None, None) => conn.query(&format!("SELECT {COLS}"), &[]).unwrap(),
        (Some(l), None) => {
            let a = format!("({l},0)");
            conn.query(&format!("SELECT {COLS} WHERE ctid >= $1::text::tid"), &[Value::Text(&a)]).unwrap()
        }
        (Some(l), Some(h)) => {
            let (a, b) = (format!("({l},0)"), format!("({h},0)"));
            conn.query(&format!("SELECT {COLS} WHERE ctid >= $1::text::tid AND ctid < $2::text::tid"), &[Value::Text(&a), Value::Text(&b)])
                .unwrap()
        }
        _ => unreachable!(),
    };
    while let Some(ch) = cur.next_chunk().unwrap() {
        rows += ch.rows() as u64;
    }
    rows
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cfg = Config::from_url(&a[1]).expect("url");
    let rounds: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(2);
    let mut probe = Connection::connect(&cfg).unwrap();
    let blocks = {
        let mut cur = probe.query("SELECT (pg_relation_size('taxi_trips'::regclass) / 8192)::int8", &[]).unwrap();
        cur.next().unwrap().unwrap().get::<i64>(0).unwrap()
    };
    println!("blocks {blocks}");
    let mut conns: Vec<Connection> = (0..4).map(|_| Connection::connect(&cfg).unwrap()).collect();
    // warm the caches
    scan(&mut conns[0], None, None);
    for r in 1..=rounds {
        let t = Instant::now();
        let n = scan(&mut conns[0], None, None);
        println!("round {r}: whole table, 1 connection:        {:6.2} s ({n} rows)", t.elapsed().as_secs_f64());
        let mid = blocks / 2;
        let t = Instant::now();
        let n = scan(&mut conns[0], Some(0), Some(mid));
        println!("round {r}: first half alone, 1 connection:   {:6.2} s ({n} rows)", t.elapsed().as_secs_f64());
        let t = Instant::now();
        let n = scan(&mut conns[0], Some(mid), None);
        println!("round {r}: second half alone, 1 connection:  {:6.2} s ({n} rows)", t.elapsed().as_secs_f64());
        let t = Instant::now();
        let (x, y) = {
            let (c0, rest) = conns.split_at_mut(1);
            std::thread::scope(|sc| {
                let h0 = sc.spawn(|| scan(&mut c0[0], Some(0), Some(mid)));
                let h1 = sc.spawn(|| scan(&mut rest[0], Some(mid), None));
                (h0.join().unwrap(), h1.join().unwrap())
            })
        };
        println!("round {r}: both halves, 2 connections:       {:6.2} s ({} rows)", t.elapsed().as_secs_f64(), x + y);
        let t = Instant::now();
        let n: u64 = std::thread::scope(|sc| {
            let hs: Vec<_> = conns
                .iter_mut()
                .enumerate()
                .map(|(i, c)| {
                    let (lo, hi) = (blocks * i as i64 / 4, blocks * (i as i64 + 1) / 4);
                    sc.spawn(move || if i == 3 { scan(c, Some(lo), None) } else { scan(c, Some(lo), Some(hi)) })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).sum()
        });
        println!("round {r}: quarters, 4 connections:          {:6.2} s ({n} rows)", t.elapsed().as_secs_f64());
    }
}
