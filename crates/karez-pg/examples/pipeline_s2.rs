//! A point select by primary key, one connection, run unpipelined and pipelined, reported
//! separately (plan section 6: "pipelined and unpipelined reported separately").
//!
//!   KAREZ_URL='postgresql://karez_scram:karez_test_only_scram@127.0.0.1:54316/karez?sslmode=disable' \
//!     cargo run --release -p karez-pg --example pipeline_s2
//!
//! It is closed-loop and measures one connection on one machine. It is **not** the open-loop
//! S2 of `bench/` (which has latency bounds, 64 connections and other clients); it shows what
//! pipelining does to the cost of a query on this connection, and every figure it prints carries the
//! conditions of the run. Environment: `KAREZ_URL` (required), `KAREZ_QUERIES` (default 20000 per
//! repetition), `KAREZ_REPS` (default 7).

use karez_pg::{Connection, Value};
use std::time::Instant;

const SQL: &str = "SELECT * FROM customers WHERE id = $1";

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// The ids to look up: a deterministic walk over the table's 20,000 rows.
fn id(i: usize) -> i64 {
    (i as i64 * 7919) % 20_000 + 1
}

/// A checksum of one row, so every mode must return the same rows.
fn fold(row: &karez_pg::RowRef<'_>) -> u64 {
    let id = row.get::<i64>(0).unwrap() as u64;
    let email = row.get::<&str>(1).unwrap().len() as u64;
    id.wrapping_mul(31).wrapping_add(email)
}

fn unpipelined(c: &mut Connection, n: usize) -> u64 {
    let mut sum = 0u64;
    for i in 0..n {
        let mut cur = c.query(SQL, &[Value::I64(id(i))]).unwrap();
        while let Some(r) = cur.next().unwrap() {
            sum = sum.wrapping_add(fold(&r));
        }
    }
    sum
}

fn pipelined(c: &mut Connection, n: usize, depth: usize) -> u64 {
    let mut sum = 0u64;
    let mut i = 0;
    while i < n {
        let k = depth.min(n - i);
        let ids: Vec<i64> = (i..i + k).map(id).collect();
        let mut p = c.pipeline();
        for v in &ids {
            p = p.query(SQL, &[Value::I64(*v)]);
        }
        for r in p.run().unwrap() {
            for ch in r.unwrap().chunks {
                for row in 0..ch.rows() {
                    sum = sum.wrapping_add(fold(&ch.row(row).unwrap()));
                }
            }
        }
        i += k;
    }
    sum
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn main() {
    let url = std::env::var("KAREZ_URL").expect("set KAREZ_URL");
    let n = env_usize("KAREZ_QUERIES", 20_000);
    let reps = env_usize("KAREZ_REPS", 7);
    let mut c = Connection::connect_url(&url).expect("connect");
    println!(
        "server {}, {n} point selects of `{SQL}` per repetition, {reps} repetitions, one connection",
        c.server_param("server_version").unwrap_or("?")
    );
    // the statement is prepared and the cache warm before any clock starts
    let want = unpipelined(&mut c, n);
    let mut rows = Vec::new();
    let mut run = |name: String, f: &mut dyn FnMut(&mut Connection) -> u64| {
        let mut per_query_us = Vec::new();
        for _ in 0..reps {
            let t = Instant::now();
            let sum = f(&mut c);
            let dt = t.elapsed().as_secs_f64();
            assert_eq!(
                sum, want,
                "{name}: a different result than the unpipelined run"
            );
            per_query_us.push(dt * 1e6 / n as f64);
        }
        let m = median(per_query_us.clone());
        let (lo, hi) = (
            per_query_us.iter().cloned().fold(f64::MAX, f64::min),
            per_query_us.iter().cloned().fold(0.0, f64::max),
        );
        rows.push((name, m, lo, hi));
    };
    run("unpipelined (one query per round trip)".into(), &mut |c| {
        unpipelined(c, n)
    });
    for depth in [4usize, 16, 64, 256] {
        run(format!("pipelined, {depth} queries per Sync"), &mut |c| {
            pipelined(c, n, depth)
        });
    }
    let base = rows[0].1;
    println!(
        "{:<44} {:>12} {:>14} {:>10}",
        "mode", "median us/q", "min..max us/q", "queries/s"
    );
    for (name, m, lo, hi) in &rows {
        println!(
            "{name:<44} {m:>12.1} {:>14} {:>10.0}   ({:.2}x the unpipelined rate)",
            format!("{lo:.1}..{hi:.1}"),
            1e6 / m,
            base / m
        );
    }
}
