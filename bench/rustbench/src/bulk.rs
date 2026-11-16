//! Bulk scenarios of the Rust adapters (closed loop, not latency): S4 scan of 1M rows x 10 columns,
//! S5 batch insert, S6 COPY. One run = one timed pass; the adapter does `--repeats` passes and
//! prints seconds and CPU-seconds of each, plus the process's peak RSS. The orchestrator
//! (`run.py bulk`) takes medians and checks that every client computed the same checksum.
//!
//! The checksum of S4: the wrapping sum over all rows of id + a + b + c + bits(d) + bits(e) +
//! len(s1) + len(s2) + flag + f. Every adapter, in every language, folds exactly this.

use crate::{die, Cfg, Scenario};
use std::time::Instant;

pub const S4_SQL: &str = "SELECT id, a, b, c, d, e, s1, s2, flag, f FROM bench_wide";
pub const INS_SQL: &str = "INSERT INTO bench_ins (id, a, b, c, d, e, s1, s2, flag, f) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";
pub const COPY_SQL: &str = "COPY bench_ins (id, a, b, c, d, e, s1, s2, flag, f) FROM STDIN";

/// The row recipe of S5 and S6; every language generates exactly this (the generation cost is part of
/// the measurement, the same for everyone).
#[derive(Clone, Debug)]
pub struct GenRow {
    pub id: i64,
    pub a: i32,
    pub b: i32,
    pub c: i64,
    pub d: f64,
    pub e: f64,
    pub s1: String,
    pub s2: String,
    pub flag: bool,
    pub f: i16,
}

pub fn gen_row(i: i64) -> GenRow {
    GenRow {
        id: i,
        a: (i % 100_000) as i32,
        b: ((i * 17) % 65_536) as i32,
        c: i * 10_007,
        d: i as f64 / 3.0,
        e: (i % 1000) as f64 * 0.25,
        s1: format!("row-{i}"),
        s2: format!("{:032x}", (i as u128).wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835)),
        flag: i % 2 == 0,
        f: (i % 1000) as i16,
    }
}

pub fn gen_text(i: i64, out: &mut String) {
    let r = gen_row(i);
    out.push_str(&format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        r.id,
        r.a,
        r.b,
        r.c,
        r.d,
        r.e,
        r.s1,
        r.s2,
        if r.flag { "t" } else { "f" },
        r.f
    ));
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Acc {
    pub rows: u64,
    pub sum: u64,
}

impl Acc {
    #[allow(clippy::too_many_arguments)]
    pub fn add(&mut self, id: i64, a: i32, b: i32, c: i64, d: f64, e: f64, l1: usize, l2: usize, flag: bool, f: i16) {
        self.rows += 1;
        self.sum = self
            .sum
            .wrapping_add(id as u64)
            .wrapping_add(a as u64)
            .wrapping_add(b as u64)
            .wrapping_add(c as u64)
            .wrapping_add(d.to_bits())
            .wrapping_add(e.to_bits())
            .wrapping_add(l1 as u64)
            .wrapping_add(l2 as u64)
            .wrapping_add(flag as u64)
            .wrapping_add(f as u64);
    }
}

pub fn cpu_now() -> f64 {
    if let Ok(s) = std::fs::read_to_string("/proc/self/stat") {
        if let Some(rest) = s.rsplit_once(')').map(|x| x.1) {
            let f: Vec<&str> = rest.split_whitespace().collect();
            let u: f64 = f.get(11).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let s: f64 = f.get(12).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            return (u + s) / 100.0;
        }
    }
    0.0
}

pub struct Pass {
    pub secs: f64,
    pub cpu: f64,
    pub rows: u64,
    pub sum: u64,
}

pub fn timed(f: impl FnOnce() -> Result<(u64, u64), String>) -> Pass {
    let c0 = cpu_now();
    let t = Instant::now();
    let (rows, sum) = f().unwrap_or_else(|e| die(&format!("bulk pass failed: {e}")));
    Pass { secs: t.elapsed().as_secs_f64(), cpu: cpu_now() - c0, rows, sum }
}

pub fn rss_kb() -> u64 {
    if let Ok(s) = std::fs::read_to_string("/proc/self/status") {
        for l in s.lines() {
            if let Some(v) = l.strip_prefix("VmHWM:") {
                return v.trim().trim_end_matches("kB").trim().parse().unwrap_or(0);
            }
        }
    }
    0
}

pub fn report(cfg: &Cfg, version: &str, passes: &[Pass]) -> String {
    let runs: Vec<String> = passes
        .iter()
        .map(|p| format!("{{\"secs\":{:.4},\"cpu_s\":{:.3},\"rows\":{},\"checksum\":{}}}", p.secs, p.cpu, p.rows, p.sum))
        .collect();
    format!(
        "{{\"harness\":\"rustbench\",\"bulk\":true,\"client\":\"{}\",\"version\":\"{}\",\"scenario\":\"{:?}\",\"rows_target\":{},\"runs\":[{}],\"rss_kb\":{}}}",
        cfg.client,
        version,
        cfg.scenario,
        cfg.bulk_rows,
        runs.join(","),
        rss_kb()
    )
}

/// Entry point: returns the JSON line, or None if the scenario is not a bulk one.
pub fn run(cfg: &Cfg) -> Option<String> {
    match cfg.scenario {
        Scenario::S4 | Scenario::S5 | Scenario::S6 | Scenario::S8 => {}
        _ => return None,
    }
    let (version, passes) = match cfg.client.as_str() {
        "karez-blocking" => crate::bulk_karez::run(cfg, false),
        "karez-blocking-chunk" => crate::bulk_karez::run(cfg, true),
        "karez-blocking-mrow" => {
            crate::bulk_karez::MULTIROW.store(true, std::sync::atomic::Ordering::Relaxed);
            crate::bulk_karez::run(cfg, false)
        }
        "karez-blocking-part" => crate::bulk_karez::run_part(cfg),
        "tokio-postgres" => crate::bulk_async::tokio_pg(cfg),
        "sqlx" | "sqlx-tuned" => crate::bulk_async::sqlx_(cfg),
        other => die(&format!("{other}: no bulk adapter (karez-blocking, karez-blocking-chunk, tokio-postgres, sqlx)")),
    };
    Some(report(cfg, &version, &passes))
}
