//! Bulk scenarios for karez: the row view and the chunk path (S4), batched inserts (S5), COPY (S6).

use crate::bulk::*;
use crate::{die, Cfg, Scenario};
use karez_pg::{Config, Connection, Value};
use std::io::Read;

fn le<const N: usize>(b: &[u8]) -> impl Iterator<Item = [u8; N]> + '_ {
    b.chunks_exact(N).map(|c| c.try_into().unwrap())
}

fn scan_rows(conn: &mut Connection) -> Result<(u64, u64), String> {
    let mut cur = conn.query(S4_SQL, &[]).map_err(|e| e.to_string())?;
    let mut acc = Acc::default();
    while let Some(r) = cur.next().map_err(|e| e.to_string())? {
        let g = |e: karez_pg::Error| e.to_string();
        let _ = g;
        acc.add(
            r.get::<i64>(0).map_err(|e| e.to_string())?,
            r.get::<i32>(1).map_err(|e| e.to_string())?,
            r.get::<i32>(2).map_err(|e| e.to_string())?,
            r.get::<i64>(3).map_err(|e| e.to_string())?,
            r.get::<f64>(4).map_err(|e| e.to_string())?,
            r.get::<f64>(5).map_err(|e| e.to_string())?,
            r.get::<&str>(6).map_err(|e| e.to_string())?.len(),
            r.get::<&str>(7).map_err(|e| e.to_string())?.len(),
            r.get::<bool>(8).map_err(|e| e.to_string())?,
            r.get::<i16>(9).map_err(|e| e.to_string())?,
        );
    }
    Ok((acc.rows, acc.sum))
}

/// The chunk path: whole columns at a time, straight over the Arrow-layout buffers.
fn scan_chunks(conn: &mut Connection) -> Result<(u64, u64), String> {
    scan_chunks_sql(conn, S4_SQL, &[])
}

/// The per-chunk fold of S4 (all ten columns), shared by the single-connection and the API paths.
pub fn s4_fold(ch: &karez_pg::Chunk) -> Result<u64, String> {
    let n = ch.rows();
    let col = |i: usize| ch.column(i).map_err(|e| e.to_string());
    let mut s = 0u64;
    for i in [0usize, 3] {
        for v in le::<8>(col(i)?.data()).take(n) {
            s = s.wrapping_add(i64::from_le_bytes(v) as u64);
        }
    }
    for i in [1usize, 2] {
        for v in le::<4>(col(i)?.data()).take(n) {
            s = s.wrapping_add(i32::from_le_bytes(v) as u64);
        }
    }
    for i in [4usize, 5] {
        for v in le::<8>(col(i)?.data()).take(n) {
            s = s.wrapping_add(u64::from_le_bytes(v));
        }
    }
    for v in le::<2>(col(9)?.data()).take(n) {
        s = s.wrapping_add(i16::from_le_bytes(v) as u64);
    }
    for i in [6usize, 7] {
        let o = col(i)?.offsets();
        s = s.wrapping_add((o[n] - o[0]) as u64);
    }
    let bits = col(8)?.data();
    let full = n / 8;
    let mut t = bits[..full].iter().map(|b| b.count_ones() as u64).sum::<u64>();
    if n % 8 != 0 {
        t += (bits[full] & ((1u8 << (n % 8)) - 1)).count_ones() as u64;
    }
    s = s.wrapping_add(t);
    Ok(s)
}

fn scan_chunks_sql(conn: &mut Connection, sql: &str, params: &[Value<'_>]) -> Result<(u64, u64), String> {
    let mut cur = conn.query(sql, params).map_err(|e| e.to_string())?;
    let (mut rows, mut sum) = (0u64, 0u64);
    while let Some(ch) = cur.next_chunk().map_err(|e| e.to_string())? {
        let n = ch.rows();
        let s = s4_fold(ch)?;
        sum = sum.wrapping_add(s);
        rows += n as u64;
    }
    Ok((rows, sum))
}

fn scan_taxi_rows(conn: &mut Connection) -> Result<(u64, u64), String> {
    let mut cur = conn.query(crate::TAXI_SCAN_SQL, &[]).map_err(|e| e.to_string())?;
    let (mut rows, mut sum) = (0u64, 0u64);
    while let Some(r) = cur.next().map_err(|e| e.to_string())? {
        sum = sum.wrapping_add(crate::clients::karez_taxi(&r)?);
        rows += 1;
    }
    Ok((rows, sum))
}

/// The chunk path on the real table: whole columns, NULLs through the validity bitmaps.
fn scan_taxi_chunks(conn: &mut Connection) -> Result<(u64, u64), String> {
    scan_taxi_chunks_sql(conn, crate::TAXI_SCAN_SQL, &[])
}

/// The per-chunk fold of S8 (17 columns, NULLs through the validity bitmaps).
pub fn taxi_fold(ch: &karez_pg::Chunk) -> Result<u64, String> {
    let n = ch.rows();
    let col = |i: usize| ch.column(i).map_err(|e| e.to_string());
    let valid = |c: &karez_pg::Column, r: usize| c.null_count() == 0 || (c.validity()[r / 8] >> (r % 8)) & 1 == 1;
    let mut s = 0u64;
    for v in le::<8>(col(0)?.data()).take(n) {
        s = s.wrapping_add(i64::from_le_bytes(v) as u64);
    }
    for i in [1usize, 10] {
        for v in le::<2>(col(i)?.data()).take(n) {
            s = s.wrapping_add(i16::from_le_bytes(v) as u64);
        }
    }
    for i in [2usize, 3] {
        for v in le::<8>(col(i)?.data()).take(n) {
            s = s.wrapping_add(i64::from_le_bytes(v) as u64);
        }
    }
    for i in [4usize, 6] {
        let c = col(i)?;
        for (r, v) in le::<2>(c.data()).take(n).enumerate() {
            if valid(c, r) {
                s = s.wrapping_add(i16::from_le_bytes(v) as u64);
            }
        }
    }
    for i in [8usize, 9] {
        for v in le::<4>(col(i)?.data()).take(n) {
            s = s.wrapping_add(i32::from_le_bytes(v) as u64);
        }
    }
    for i in [5usize, 11, 12, 13, 14, 15] {
        for v in le::<8>(col(i)?.data()).take(n) {
            s = s.wrapping_add(u64::from_le_bytes(v));
        }
    }
    let c = col(16)?;
    for (r, v) in le::<8>(c.data()).take(n).enumerate() {
        if valid(c, r) {
            s = s.wrapping_add(u64::from_le_bytes(v));
        }
    }
    let o = col(7)?.offsets();
    s = s.wrapping_add((o[n] - o[0]) as u64);
    Ok(s)
}

fn scan_taxi_chunks_sql(conn: &mut Connection, sql: &str, params: &[Value<'_>]) -> Result<(u64, u64), String> {
    let mut cur = conn.query(sql, params).map_err(|e| e.to_string())?;
    let (mut rows, mut sum) = (0u64, 0u64);
    while let Some(ch) = cur.next_chunk().map_err(|e| e.to_string())? {
        let n = ch.rows();
        let s = taxi_fold(ch)?;
        sum = sum.wrapping_add(s);
        rows += n as u64;
    }
    Ok((rows, sum))
}

/// Set by the `karez-blocking-mrow` client: S5 with one multi-row `INSERT ... VALUES` per batch (perf-program.md 3.3).
pub static MULTIROW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn insert_multirow(conn: &mut Connection, rows: i64, _batch: i64) -> Result<(u64, u64), String> {
    // the library's `Connection::insert_batch` (1,000 rows per statement, atomic)
    let e = |e: karez_pg::Error| e.to_string();
    conn.execute("TRUNCATE bench_ins", &[]).map_err(e)?;
    let cols = ["id", "a", "b", "c", "d", "e", "s1", "s2", "flag", "f"];
    let mut i = 1;
    let mut inserted = 0u64;
    while i <= rows {
        let end = (i + 9_999).min(rows);
        let gens: Vec<GenRow> = (i..=end).map(gen_row).collect();
        let vals: Vec<[Value<'_>; 10]> = gens
            .iter()
            .map(|r| {
                [
                    Value::i64(r.id),
                    Value::i32(r.a),
                    Value::i32(r.b),
                    Value::i64(r.c),
                    Value::f64(r.d),
                    Value::f64(r.e),
                    Value::text(&r.s1),
                    Value::text(&r.s2),
                    Value::bool(r.flag),
                    Value::i16(r.f),
                ]
            })
            .collect();
        let refs: Vec<&[Value<'_>]> = vals.iter().map(|v| &v[..]).collect();
        inserted += conn.insert_batch("bench_ins", &cols, &refs).map_err(e)?;
        i = end + 1;
    }
    let n = count(conn)?;
    if n != inserted {
        return Err(format!("inserted {inserted}, counted {n}"));
    }
    Ok((n, 0))
}

fn insert_batched(conn: &mut Connection, rows: i64) -> Result<(u64, u64), String> {
    if MULTIROW.load(std::sync::atomic::Ordering::Relaxed) {
        let batch = std::env::var("KAREZ_MROW_BATCH").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);
        return insert_multirow(conn, rows, batch);
    }
    let e = |e: karez_pg::Error| e.to_string();
    conn.execute("TRUNCATE bench_ins", &[]).map_err(e)?;
    conn.execute("BEGIN", &[]).map_err(e)?;
    const BATCH: i64 = 1000;
    let mut i = 1;
    while i <= rows {
        let end = (i + BATCH - 1).min(rows);
        let gens: Vec<GenRow> = (i..=end).map(gen_row).collect();
        let params: Vec<[Value<'_>; 10]> = gens
            .iter()
            .map(|r| {
                [
                    Value::i64(r.id),
                    Value::i32(r.a),
                    Value::i32(r.b),
                    Value::i64(r.c),
                    Value::f64(r.d),
                    Value::f64(r.e),
                    Value::text(&r.s1),
                    Value::text(&r.s2),
                    Value::bool(r.flag),
                    Value::i16(r.f),
                ]
            })
            .collect();
        let items: Vec<(&str, &[Value<'_>])> = params.iter().map(|p| (INS_SQL, &p[..])).collect();
        conn.execute_many(&items).map_err(e)?;
        i = end + 1;
    }
    conn.execute("COMMIT", &[]).map_err(e)?;
    let n = count(conn)?;
    Ok((n, 0))
}

fn count(conn: &mut Connection) -> Result<u64, String> {
    let mut cur = conn.query("SELECT count(*) FROM bench_ins", &[]).map_err(|e| e.to_string())?;
    let r = cur.next().map_err(|e| e.to_string())?.ok_or("no count row")?;
    Ok(r.get::<i64>(0).map_err(|e| e.to_string())? as u64)
}

struct GenReader {
    next: i64,
    end: i64,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for GenReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.buf.len() {
            self.buf.clear();
            self.pos = 0;
            let mut s = String::new();
            while self.next <= self.end && s.len() < 64 * 1024 {
                gen_text(self.next, &mut s);
                self.next += 1;
            }
            self.buf.extend_from_slice(s.as_bytes());
            if self.buf.is_empty() {
                return Ok(0);
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Set by the `karez-blocking-binary` client: S6 with binary COPY (`Connection::copy_in_binary`), the same rows.
pub static BINARY_COPY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn copy_in_binary(conn: &mut Connection, rows: i64) -> Result<(u64, u64), String> {
    use karez_pg::Value;
    let e = |e: karez_pg::Error| e.to_string();
    conn.execute("TRUNCATE bench_ins", &[]).map_err(e)?;
    let mut w = conn.copy_in_binary(COPY_BINARY_SQL).map_err(e)?;
    for i in 1..=rows {
        let r = gen_row(i);
        w.row(&[
            Value::I64(r.id),
            Value::I32(r.a),
            Value::I32(r.b),
            Value::I64(r.c),
            Value::F64(r.d),
            Value::F64(r.e),
            Value::Text(&r.s1),
            Value::Text(&r.s2),
            Value::Bool(r.flag),
            Value::I16(r.f),
        ])
        .map_err(e)?;
    }
    let n = w.finish().map_err(e)?;
    Ok((n, 0))
}

fn copy_in(conn: &mut Connection, rows: i64) -> Result<(u64, u64), String> {
    if BINARY_COPY.load(std::sync::atomic::Ordering::Relaxed) {
        return copy_in_binary(conn, rows);
    }
    let e = |e: karez_pg::Error| e.to_string();
    conn.execute("TRUNCATE bench_ins", &[]).map_err(e)?;
    let mut r = GenReader { next: 1, end: rows, buf: Vec::new(), pos: 0 };
    let n = conn.copy_in(COPY_SQL, &mut r).map_err(e)?;
    Ok((n, 0))
}

pub fn run(cfg: &Cfg, chunk: bool) -> (String, Vec<Pass>) {
    let c = Config::from_url(&cfg.url).unwrap_or_else(|e| die(&format!("bad url: {e}")));
    let mut conn = Connection::connect(&c).unwrap_or_else(|e| die(&format!("connect: {e}")));
    let mut passes = Vec::new();
    // one untimed pass warms the server's cache and the statement cache
    let warm = |conn: &mut Connection| match cfg.scenario {
        Scenario::S4 => if chunk { scan_chunks(conn) } else { scan_rows(conn) },
        Scenario::S8 => if chunk { scan_taxi_chunks(conn) } else { scan_taxi_rows(conn) },
        Scenario::S5 => insert_batched(conn, 1000),
        _ => copy_in(conn, 1000),
    };
    warm(&mut conn).unwrap_or_else(|e| die(&format!("warm-up: {e}")));
    for _ in 0..cfg.repeats {
        passes.push(timed(|| match cfg.scenario {
            Scenario::S4 => if chunk { scan_chunks(&mut conn) } else { scan_rows(&mut conn) },
            Scenario::S8 => if chunk { scan_taxi_chunks(&mut conn) } else { scan_taxi_rows(&mut conn) },
            Scenario::S5 => insert_batched(&mut conn, cfg.bulk_rows),
            _ => copy_in(&mut conn, cfg.bulk_rows),
        }));
    }
    (format!("karez-pg {}{}", env!("CARGO_PKG_VERSION"), if chunk { " (chunk path)" } else if BINARY_COPY.load(std::sync::atomic::Ordering::Relaxed) { " (binary COPY)" } else { " (row view)" }), passes)
}

/// Prototype of a partitioned scan (perf-program.md 3.1): `KAREZ_PARTS` connections (default 4), each scans one key range
/// through the chunk path on its own thread; the checksums add up. S4 (`bench_wide.id`) and S8 (`taxi_trips.trip_id`).
/// `KAREZ_SNAPSHOT=1`: all partitions read one snapshot (`pg_export_snapshot` / `SET TRANSACTION SNAPSHOT`), inside the timing.
pub fn run_part(cfg: &Cfg) -> (String, Vec<Pass>) {
    let parts: i64 = std::env::var("KAREZ_PARTS").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    let snapshot = std::env::var("KAREZ_SNAPSHOT").map(|v| v == "1").unwrap_or(false);
    let taxi = match cfg.scenario {
        Scenario::S4 => false,
        Scenario::S8 => true,
        _ => die("karez-blocking-part: only S4 and S8 are implemented"),
    };
    let (key, table) = if taxi { ("trip_id", "taxi_trips") } else { ("id", "bench_wide") };
    let c = Config::from_url(&cfg.url).unwrap_or_else(|e| die(&format!("bad url: {e}")));
    if std::env::var("KAREZ_API").map(|v| v == "1").unwrap_or(false) {
        return run_part_api(cfg, &c, parts, snapshot, taxi);
    }
    let mut conns: Vec<Connection> =
        (0..parts).map(|_| Connection::connect(&c).unwrap_or_else(|e| die(&format!("connect: {e}")))).collect();
    let (min, max) = {
        let mut cur =
            conns[0].query(&format!("SELECT min({key}), max({key}) FROM {table}"), &[]).unwrap_or_else(|e| die(&e.to_string()));
        let r = cur.next().unwrap_or_else(|e| die(&e.to_string())).unwrap_or_else(|| die("no row"));
        (r.get::<i64>(0).unwrap_or_else(|e| die(&e.to_string())), r.get::<i64>(1).unwrap_or_else(|e| die(&e.to_string())))
    };
    let span = max - min + 1;
    let sql = if taxi {
        format!("{} WHERE {key} > $1 AND {key} <= $2", crate::TAXI_SCAN_SQL)
    } else {
        format!("SELECT id, a, b, c, d, e, s1, s2, flag, f FROM bench_wide WHERE {key} > $1 AND {key} <= $2")
    };
    let pass = |conns: &mut Vec<Connection>| -> Result<(u64, u64), String> {
        let e = |e: karez_pg::Error| e.to_string();
        if snapshot {
            conns[0].execute("BEGIN ISOLATION LEVEL REPEATABLE READ", &[]).map_err(e)?;
            let snap = {
                let mut cur = conns[0].query("SELECT pg_export_snapshot()", &[]).map_err(e)?;
                let r = cur.next().map_err(e)?.ok_or("no snapshot row")?;
                r.get::<&str>(0).map_err(|x| x.to_string())?.to_string()
            };
            for conn in conns.iter_mut().skip(1) {
                conn.execute("BEGIN ISOLATION LEVEL REPEATABLE READ", &[]).map_err(e)?;
                conn.execute(&format!("SET TRANSACTION SNAPSHOT '{snap}'"), &[]).map_err(e)?;
            }
        }
        let res: Vec<Result<(u64, u64), String>> = std::thread::scope(|sc| {
            let hs: Vec<_> = conns
                .iter_mut()
                .enumerate()
                .map(|(i, conn)| {
                    let (lo, hi) = (min - 1 + span * i as i64 / parts, min - 1 + span * (i as i64 + 1) / parts);
                    let sql = &sql;
                    sc.spawn(move || {
                        let p = [Value::i64(lo), Value::i64(hi)];
                        if taxi { scan_taxi_chunks_sql(conn, sql, &p) } else { scan_chunks_sql(conn, sql, &p) }
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("thread panicked".into()))).collect()
        });
        if snapshot {
            for conn in conns.iter_mut() {
                conn.execute("COMMIT", &[]).map_err(e)?;
            }
        }
        let (mut rows, mut sum) = (0u64, 0u64);
        for r in res {
            let (a, b) = r?;
            rows += a;
            sum = sum.wrapping_add(b);
        }
        Ok((rows, sum))
    };
    pass(&mut conns).unwrap_or_else(|e| die(&format!("warm-up: {e}")));
    let mut passes = Vec::new();
    for _ in 0..cfg.repeats {
        passes.push(timed(|| pass(&mut conns)));
    }
    (format!("karez-pg {} (chunk path, {parts} partitions{})", env!("CARGO_PKG_VERSION"), if snapshot { ", shared snapshot" } else { "" }), passes)
}

/// The same scan through the library's `scan_partitioned` (ctid ranges, shared snapshot by default).
fn run_part_api(cfg: &Cfg, c: &Config, parts: i64, snapshot: bool, taxi: bool) -> (String, Vec<Pass>) {
    let s4_cols = ["id", "a", "b", "c", "d", "e", "s1", "s2", "flag", "f"];
    let taxi_cols = [
        "trip_id", "vendor", "pickup", "dropoff", "passengers", "distance", "ratecode", "flag", "pu", "do_", "payment", "fare",
        "extra", "tip", "tolls", "total", "congestion",
    ];
    let (table, cols): (&str, &[&str]) = if taxi { ("taxi_trips", &taxi_cols) } else { ("bench_wide", &s4_cols) };
    let pass = || -> Result<(u64, u64), String> {
        let scan = karez_pg::Scan::new(table, cols).partitions(parts as usize).snapshot(snapshot);
        let states = karez_pg::scan_partitioned(
            c,
            &scan,
            |_| (0u64, 0u64),
            |st, ch| {
                let s = if taxi { taxi_fold(ch) } else { s4_fold(ch) }.map_err(karez_pg::Error::Value)?;
                st.0 += ch.rows() as u64;
                st.1 = st.1.wrapping_add(s);
                Ok(())
            },
        )
        .map_err(|e| e.to_string())?;
        Ok(states.iter().fold((0, 0), |a, s| (a.0 + s.0, a.1.wrapping_add(s.1))))
    };
    pass().unwrap_or_else(|e| die(&format!("warm-up: {e}")));
    let mut passes = Vec::new();
    for _ in 0..cfg.repeats {
        passes.push(timed(pass));
    }
    (format!("karez-pg {} (scan_partitioned, {parts} partitions{})", env!("CARGO_PKG_VERSION"), if snapshot { ", snapshot" } else { "" }), passes)
}
