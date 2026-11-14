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
    let mut cur = conn.query(S4_SQL, &[]).map_err(|e| e.to_string())?;
    let (mut rows, mut sum) = (0u64, 0u64);
    while let Some(ch) = cur.next_chunk().map_err(|e| e.to_string())? {
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
        sum = sum.wrapping_add(s);
        rows += n as u64;
    }
    Ok((rows, sum))
}

fn insert_batched(conn: &mut Connection, rows: i64) -> Result<(u64, u64), String> {
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

fn copy_in(conn: &mut Connection, rows: i64) -> Result<(u64, u64), String> {
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
        Scenario::S5 => insert_batched(conn, 1000),
        _ => copy_in(conn, 1000),
    };
    warm(&mut conn).unwrap_or_else(|e| die(&format!("warm-up: {e}")));
    for _ in 0..cfg.repeats {
        passes.push(timed(|| match cfg.scenario {
            Scenario::S4 => if chunk { scan_chunks(&mut conn) } else { scan_rows(&mut conn) },
            Scenario::S5 => insert_batched(&mut conn, cfg.bulk_rows),
            _ => copy_in(&mut conn, cfg.bulk_rows),
        }));
    }
    (format!("karez-pg {}{}", env!("CARGO_PKG_VERSION"), if chunk { " (chunk path)" } else { " (row view)" }), passes)
}
