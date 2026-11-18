//! diesel 2 (blocking, libpq underneath) adapter: S1, S2 and R1 (open loop, one connection per worker,
//! no pool) and the S4 scan (closed loop, row by row). Not covered: R2/R3/R4 (a range query with a
//! repeated parameter does not map onto diesel's `sql::<T>().bind()` without changing the statement),
//! S5/S6/S8 (no diesel adapter written). libpq is built in (`pq-sys` with `bundled`), the same one on
//! every machine; it speaks `sslmode`, `sslrootcert` and SCRAM itself.

use crate::bulk::{Acc, Pass, S4_SQL};
use crate::clients::{run_generator, sync_queues, Rng};
use crate::{die, Cfg, Mode, Scenario, Shared};
use diesel::dsl::sql;
use diesel::pg::data_types::PgTimestamp;
use diesel::pg::{PgConnection, PgRowByRowLoadingMode};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Double, Integer, Nullable, SmallInt, Text, Timestamp};
use std::sync::{Arc, Barrier};

/// Microseconds from 1970-01-01 to 2000-01-01 (PgTimestamp counts from 2000).
const PG_EPOCH_US: i64 = 946_684_800_000_000;

fn url(raw: &str) -> String {
    // `auth=trust` is a karez URL option that libpq rejects.
    let Some((base, q)) = raw.split_once('?') else { return raw.to_string() };
    let kept: Vec<&str> = q.split('&').filter(|kv| !kv.starts_with("auth=")).collect();
    if kept.is_empty() { base.to_string() } else { format!("{base}?{}", kept.join("&")) }
}

fn connect(raw: &str) -> PgConnection {
    PgConnection::establish(&url(raw)).unwrap_or_else(|e| die(&format!("connect: {e}")))
}

type S2Row = (i64, i32, i16, bool, f64, String, String, i64, String, i64);
type S2Types = (BigInt, Integer, SmallInt, Bool, Double, Text, Text, BigInt, Text, BigInt);
type TaxiRow = (
    i64, i16, PgTimestamp, PgTimestamp, Option<i16>, f64, Option<i16>, Option<String>, i32, i32, i16, f64, f64, f64, f64, f64, Option<f64>,
);
type TaxiTypes = (
    BigInt, SmallInt, Timestamp, Timestamp, Nullable<SmallInt>, Double, Nullable<SmallInt>, Nullable<Text>, Integer, Integer, SmallInt, Double,
    Double, Double, Double, Double, Nullable<Double>,
);

const S2_PREFIX: &str = "SELECT id, k1, k2, flag, score, name, note, ref, tag, big FROM bench_item WHERE id = ";
const R1_PREFIX: &str = "SELECT trip_id, vendor, pickup, dropoff, passengers, distance, ratecode, flag, pu, do_, payment, fare, extra, tip, tolls, total, congestion FROM taxi_trips WHERE trip_id = ";

fn taxi_sum(r: &TaxiRow) -> u64 {
    let mut s = r.0 as u64;
    s = s.wrapping_add(r.1 as u64);
    s = s.wrapping_add((r.2 .0 + PG_EPOCH_US) as u64);
    s = s.wrapping_add((r.3 .0 + PG_EPOCH_US) as u64);
    s = s.wrapping_add(r.4.unwrap_or(0) as u64);
    s = s.wrapping_add(r.5.to_bits());
    s = s.wrapping_add(r.6.unwrap_or(0) as u64);
    s = s.wrapping_add(r.7.as_ref().map_or(0, |v| v.len()) as u64);
    s = s.wrapping_add(r.8 as u64);
    s = s.wrapping_add(r.9 as u64);
    s = s.wrapping_add(r.10 as u64);
    for v in [r.11, r.12, r.13, r.14, r.15] {
        s = s.wrapping_add(v.to_bits());
    }
    s.wrapping_add(r.16.map_or(0, |v| v.to_bits()))
}

fn query(conn: &mut PgConnection, sc: Scenario, id: i64) -> Result<u64, String> {
    match sc {
        Scenario::S1 => {
            let v: i32 = sql::<Integer>(sc.sql()).get_result(conn).map_err(|e| e.to_string())?;
            Ok(v as u64)
        }
        Scenario::S2 => {
            let rows: Vec<S2Row> = sql::<S2Types>(S2_PREFIX).bind::<BigInt, _>(id).load(conn).map_err(|e| e.to_string())?;
            if rows.len() != 1 || rows[0].0 != id {
                return Err(format!("S2: {} rows for id {id}", rows.len()));
            }
            let r = &rows[0];
            let mut s = r.0 as u64;
            s = s.wrapping_add(r.1 as u64);
            s = s.wrapping_add(r.2 as u64);
            s = s.wrapping_add(r.3 as u64);
            s = s.wrapping_add(r.4.to_bits());
            s = s.wrapping_add(r.5.len() as u64);
            s = s.wrapping_add(r.6.len() as u64);
            s = s.wrapping_add(r.7 as u64);
            s = s.wrapping_add(r.8.len() as u64);
            Ok(s.wrapping_add(r.9 as u64))
        }
        Scenario::R1 => {
            let rows: Vec<TaxiRow> = sql::<TaxiTypes>(R1_PREFIX).bind::<BigInt, _>(id).load(conn).map_err(|e| e.to_string())?;
            if rows.len() != 1 || rows[0].0 != id {
                return Err(format!("R1: {} rows for id {id}", rows.len()));
            }
            Ok(taxi_sum(&rows[0]))
        }
        other => die(&format!("diesel: scenario {other:?} not supported (s1, s2, r1, s4)")),
    }
}

pub fn run(sh: Arc<Shared>) -> String {
    if sh.cfg.mode == Mode::Pipelined {
        die("diesel cannot pipeline");
    }
    if sh.cfg.pool > 0 {
        die("diesel adapter: no pool (one connection per worker)");
    }
    let workers = sh.cfg.conns;
    let (txs, srcs) = sync_queues(&sh, workers);
    let b1 = Arc::new(Barrier::new(workers + 1));
    let b2 = Arc::new(Barrier::new(workers + 1));
    let mut hs = Vec::new();
    for (w, src) in srcs.into_iter().enumerate() {
        let (sh, b1, b2) = (sh.clone(), b1.clone(), b2.clone());
        hs.push(std::thread::spawn(move || {
            let mut conn = connect(&sh.cfg.url);
            let mut rng = Rng::new(w as u64 + 1);
            // diesel prepares a statement on first use and caches it per connection
            for k in sh.cfg.scenario.kinds() {
                let _ = query(&mut conn, k, 1);
            }
            b1.wait();
            b2.wait();
            while let Some(intended) = src.next(&sh) {
                let start = sh.now_ns();
                if start > sh.deadline_ns() {
                    sh.drop_one(intended);
                    continue;
                }
                let (sc, id) = sh.cfg.scenario.request(&mut rng, &sh.cfg);
                let r = query(&mut conn, sc, id);
                let end = sh.now_ns();
                sh.record(intended, start, end, r);
            }
        }));
    }
    b1.wait();
    sh.start();
    b2.wait();
    run_generator(&sh, txs);
    for h in hs {
        h.join().unwrap();
    }
    "diesel 2 (libpq bundled)".to_string()
}

type S4Row = (i64, i32, i32, i64, f64, f64, String, String, bool, i16);
type S4Types = (BigInt, Integer, Integer, BigInt, Double, Double, Text, Text, Bool, SmallInt);

/// S4 closed loop: the scan of 1M rows, read row by row (libpq single-row mode), folded like the others.
pub fn bulk(cfg: &Cfg) -> (String, Vec<Pass>) {
    if cfg.scenario != Scenario::S4 {
        die("diesel bulk adapter: s4 only");
    }
    let mut conn = connect(&cfg.url);
    let mut passes = Vec::new();
    for pass in 0..=cfg.repeats {
        let p = crate::bulk::timed(|| {
            let mut acc = Acc::default();
            let it = sql::<S4Types>(S4_SQL).load_iter::<S4Row, PgRowByRowLoadingMode>(&mut conn).map_err(|e| e.to_string())?;
            for r in it {
                let r = r.map_err(|e| e.to_string())?;
                acc.add(r.0, r.1, r.2, r.3, r.4, r.5, r.6.len(), r.7.len(), r.8, r.9);
            }
            Ok((acc.rows, acc.sum))
        });
        if pass > 0 {
            passes.push(p); // pass 0 is the warm-up
        }
    }
    ("diesel 2 (libpq bundled, single-row mode)".to_string(), passes)
}
