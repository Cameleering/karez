//! A smoke run that shows pipelining working, not a benchmark: one machine, one run, a client on
//! Windows or Linux talking to a Docker database through Docker's port proxy. The numbers only
//! say that queueing many commands beats one round trip each on THIS setup. They are not
//! comparable with any other client, machine or published figure.
//!
//!   KAREZ_URL='postgresql://karez_scram:karez_test_only_scram@127.0.0.1:54316/karez?sslmode=disable' \
//!     cargo run --release -p karez-rt --example pipeline_smoke

use karez_pg::{Config, Connection, Value};
use karez_rt::{Options, SharedConnection};
use std::time::Instant;

const N: i64 = 20_000;

fn main() {
    let url = std::env::var("KAREZ_URL").expect("set KAREZ_URL");
    let cfg = Config::from_url(&url).unwrap();
    let sql = "SELECT $1::int8";

    let mut c = Connection::connect(&cfg).unwrap();
    let t = Instant::now();
    let mut sum = 0;
    for i in 0..N {
        let mut cur = c.query(sql, &[Value::i64(i)]).unwrap();
        sum += cur.next().unwrap().unwrap().get::<i64>(0).unwrap();
    }
    report("blocking Connection, one at a time", t, sum);
    drop(c);

    let s = SharedConnection::connect(&cfg, Options::default()).unwrap();
    s.collect(sql, &[Value::i64(0)]).unwrap();
    let t = Instant::now();
    let mut sum = 0;
    for i in 0..N {
        let d = s.collect(sql, &[Value::i64(i)]).unwrap();
        sum += d.chunks[0].row(0).unwrap().get::<i64>(0).unwrap();
    }
    report("SharedConnection, one at a time", t, sum);

    for depth in [16usize, 128, 512] {
        let t = Instant::now();
        let mut sum = 0;
        let mut i = 0;
        while i < N {
            let batch: Vec<_> = (i..(i + depth as i64).min(N))
                .map(|k| s.submit_collect(sql, &[Value::i64(k)]).unwrap())
                .collect();
            i += depth as i64;
            for p in batch {
                sum += p.wait().unwrap().chunks[0]
                    .row(0)
                    .unwrap()
                    .get::<i64>(0)
                    .unwrap();
            }
        }
        report(&format!("SharedConnection, {depth} in flight"), t, sum);
    }

    let t = Instant::now();
    let hs: Vec<_> = (0..8)
        .map(|k| {
            let s = s.clone();
            std::thread::spawn(move || {
                let mut sum = 0;
                for i in (k..N).step_by(8) {
                    let d = s.collect(sql, &[Value::i64(i)]).unwrap();
                    sum += d.chunks[0].row(0).unwrap().get::<i64>(0).unwrap();
                }
                sum
            })
        })
        .collect();
    let sum: i64 = hs.into_iter().map(|h| h.join().unwrap()).sum();
    report("SharedConnection, 8 threads one connection", t, sum);
}

fn report(what: &str, t: Instant, sum: i64) {
    let e = t.elapsed();
    assert_eq!(sum, (0..N).sum::<i64>());
    println!(
        "{what:<45} {:>8.0} ms  {:>9.0} queries/s",
        e.as_secs_f64() * 1e3,
        N as f64 / e.as_secs_f64()
    );
}
