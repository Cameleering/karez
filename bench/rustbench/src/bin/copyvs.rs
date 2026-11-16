//! COPY ... TO STDOUT (FORMAT binary) against the normal row messages for the S4 table (perf-program.md 3.1/3.5).
//! Measures wall time, bytes received, process CPU and the busy time of all CPUs (the server shares this machine in
//! WSL, so the sum is the cost of server plus client). Nothing is decoded: this is about what the server and the wire cost.
//!
//!   copyvs URL [passes]

use karez_pg::{Config, Connection};
use std::io::Write;
use std::time::Instant;

struct Count(u64);
impl Write for Count {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0 += b.len() as u64;
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn busy() -> (f64, f64) {
    let s = std::fs::read_to_string("/proc/stat").unwrap();
    let v: Vec<f64> = s.lines().next().unwrap().split_whitespace().skip(1).map(|x| x.parse().unwrap()).collect();
    let idle = v[3] + v[4];
    (v.iter().sum::<f64>() - idle, v.iter().sum::<f64>())
}

fn proc_cpu() -> f64 {
    let s = std::fs::read_to_string("/proc/self/stat").unwrap();
    let f: Vec<&str> = s.rsplit(')').next().unwrap().split_whitespace().collect();
    (f[11].parse::<f64>().unwrap() + f[12].parse::<f64>().unwrap()) / 100.0
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = Config::from_url(a.get(1).expect("url")).expect("url");
    let passes: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(5);
    let mut conn = Connection::connect(&c).expect("connect");
    let q = "SELECT id, a, b, c, d, e, s1, s2, flag, f FROM bench_wide";
    for label in ["DataRow (query, rows counted)", "COPY binary (bytes counted)"] {
        for p in 0..passes + 1 {
            let (b0, t0) = busy();
            let c0 = proc_cpu();
            let t = Instant::now();
            let (rows, bytes) = if label.starts_with("DataRow") {
                let mut cur = conn.query(q, &[]).unwrap();
                let mut n = 0u64;
                while cur.next().unwrap().is_some() {
                    n += 1;
                }
                (n, 0)
            } else {
                let mut w = Count(0);
                conn.copy_out(&format!("COPY ({q}) TO STDOUT (FORMAT binary)"), &mut w).unwrap();
                (0, w.0)
            };
            let secs = t.elapsed().as_secs_f64();
            let (b1, t1) = busy();
            let tick = 100.0;
            if p > 0 {
                println!(
                    "{label}: {secs:.3} s  rows {rows} bytes {bytes}  client CPU {:.2} s  all-CPU busy {:.2} s",
                    proc_cpu() - c0,
                    (b1 - b0) / tick
                );
            }
            let _ = t1 - t0;
        }
    }
}
