//! `rustbench`: an open-loop load generator with Rust PostgreSQL client adapters.
//!
//! One invocation is one cell: one client, one scenario, one rate. It prints one JSON line.
//! `bench/run.py` runs the cells, repeats them, searches the rate and builds the tables.
//! Method: see bench/README.md and docs/design/plan.md section 6.

mod bulk;
mod bulk_async;
mod bulk_karez;
mod clients;
mod hist;

use hist::Hist;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    S1,
    S2,
    S3,
    S4,
    S5,
    S6,
    /// Bulk scan of the whole NYC taxi table (17 columns, NULLs, timestamps).
    S8,
    /// NYC taxi data: point select by primary key (17 columns, NULLs, timestamps).
    R1,
    /// 100 consecutive rows by primary key range.
    R2,
    /// Aggregate over one pickup zone.
    R3,
    /// Mixed workload: 80% R1, 15% R2, 5% R3.
    R4,
}

impl Scenario {
    fn parse(s: &str) -> Scenario {
        match s {
            "s1" => Scenario::S1,
            "s2" => Scenario::S2,
            "s3" => Scenario::S3,
            "s4" => Scenario::S4,
            "s5" => Scenario::S5,
            "s6" => Scenario::S6,
            "s8" => Scenario::S8,
            "r1" => Scenario::R1,
            "r2" => Scenario::R2,
            "r3" => Scenario::R3,
            "r4" => Scenario::R4,
            other => die(&format!("unknown scenario {other} (s1 to s6)")),
        }
    }
    pub fn sql(self) -> &'static str {
        match self {
            Scenario::S1 => "SELECT 1",
            Scenario::S2 => {
                "SELECT id, k1, k2, flag, score, name, note, ref, tag, big FROM bench_item WHERE id = $1"
            }
            Scenario::S3 => "SELECT id, message FROM bench_fortune",
            Scenario::R1 => TAXI_COLS_SQL_R1,
            Scenario::R2 => TAXI_COLS_SQL_R2,
            Scenario::R3 => "SELECT pu, count(*), max(fare), max(total) FROM taxi_trips WHERE pu = $1 GROUP BY pu",
            Scenario::R4 => TAXI_COLS_SQL_R1,
            Scenario::S8 => TAXI_SCAN_SQL,
            _ => bulk::S4_SQL,
        }
    }
    pub fn rows(self) -> usize {
        match self {
            Scenario::S3 => 12,
            Scenario::R2 => 100,
            _ => 1,
        }
    }
    /// Does the number of rows returned fit? (R3 returns no row for a zone without trips.)
    pub fn rows_ok(self, n: usize) -> bool {
        match self {
            Scenario::R3 => n <= 1,
            other => n == other.rows(),
        }
    }
    pub fn has_param(self) -> bool {
        matches!(self, Scenario::S2 | Scenario::R1 | Scenario::R2 | Scenario::R3 | Scenario::R4)
    }
    /// R3 binds an int4 (the zone), everything else an int8.
    pub fn param_i32(self) -> bool {
        self == Scenario::R3
    }
    /// The statements a scenario uses (R4 mixes three).
    pub fn kinds(self) -> Vec<Scenario> {
        match self {
            Scenario::R4 => vec![Scenario::R1, Scenario::R2, Scenario::R3],
            o => vec![o],
        }
    }
    /// One request: the concrete query and its parameter.
    pub fn request(self, rng: &mut clients::Rng, cfg: &Cfg) -> (Scenario, i64) {
        let q = if self == Scenario::R4 {
            match rng.next(100) {
                1..=80 => Scenario::R1,
                81..=95 => Scenario::R2,
                _ => Scenario::R3,
            }
        } else {
            self
        };
        let id = match q {
            Scenario::S2 => rng.next(cfg.rows),
            Scenario::R1 => rng.next(cfg.taxi_rows),
            Scenario::R2 => rng.next((cfg.taxi_rows - 99).max(1)),
            Scenario::R3 => rng.next(260),
            _ => 0,
        };
        (q, id)
    }
}

const TAXI_COLS: &str = "trip_id, vendor, pickup, dropoff, passengers, distance, ratecode, flag, pu, do_, payment, fare, extra, tip, tolls, total, congestion";
const TAXI_COLS_SQL_R1: &str = "SELECT trip_id, vendor, pickup, dropoff, passengers, distance, ratecode, flag, pu, do_, payment, fare, extra, tip, tolls, total, congestion FROM taxi_trips WHERE trip_id = $1";
const TAXI_COLS_SQL_R2: &str = "SELECT trip_id, vendor, pickup, dropoff, passengers, distance, ratecode, flag, pu, do_, payment, fare, extra, tip, tolls, total, congestion FROM taxi_trips WHERE trip_id BETWEEN $1 AND $1 + 99 ORDER BY trip_id";
pub const TAXI_SCAN_SQL: &str = "SELECT trip_id, vendor, pickup, dropoff, passengers, distance, ratecode, flag, pu, do_, payment, fare, extra, tip, tolls, total, congestion FROM taxi_trips";
#[allow(dead_code)]
const _: &str = TAXI_COLS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// One request at a time per connection.
    Unpipelined,
    /// Several requests in flight on one connection.
    Pipelined,
}

#[derive(Debug, Clone)]
pub struct Cfg {
    pub client: String,
    pub url: String,
    pub scenario: Scenario,
    pub mode: Mode,
    /// Connections (workers). With `pool > 0`, the number of workers contending for the pool.
    pub conns: usize,
    pub pool: usize,
    /// Arrivals per second; 0 = closed loop (each worker back to back).
    pub rate: f64,
    pub duration: Duration,
    pub warmup: Duration,
    pub rt_threads: usize,
    /// "shared": one arrival queue all workers draw from. "perconn": one queue per worker.
    pub per_conn_queue: bool,
    pub root_cert: Option<String>,
    pub rows: i64,
    /// Rows of `taxi_trips` (R scenarios), from the loader.
    pub taxi_rows: i64,
    /// Bulk scenarios: timed passes, and the row count of S5 and S6.
    pub repeats: usize,
    pub bulk_rows: i64,
}

pub fn die(msg: &str) -> ! {
    eprintln!("rustbench: {msg}");
    std::process::exit(2)
}

fn parse_args() -> Cfg {
    let mut m = std::collections::HashMap::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let k = a
            .strip_prefix("--")
            .unwrap_or_else(|| die(&format!("unexpected argument {a}")))
            .to_string();
        let v = it
            .next()
            .unwrap_or_else(|| die(&format!("--{k} needs a value")));
        m.insert(k, v);
    }
    let get = |k: &str| m.get(k).cloned();
    let num = |k: &str, d: f64| -> f64 {
        get(k)
            .map(|v| v.parse().unwrap_or_else(|_| die(&format!("--{k}: not a number"))))
            .unwrap_or(d)
    };
    Cfg {
        client: get("client").unwrap_or_else(|| die("--client is required")),
        url: get("url").unwrap_or_else(|| die("--url is required")),
        scenario: Scenario::parse(&get("scenario").unwrap_or_else(|| "s1".into())),
        mode: match get("mode").as_deref().unwrap_or("unpipelined") {
            "unpipelined" => Mode::Unpipelined,
            "pipelined" => Mode::Pipelined,
            o => die(&format!("--mode {o}: unpipelined or pipelined")),
        },
        conns: num("conns", 1.0) as usize,
        pool: num("pool", 0.0) as usize,
        rate: num("rate", 1000.0),
        duration: Duration::from_secs_f64(num("duration", 10.0)),
        warmup: Duration::from_secs_f64(num("warmup", 2.0)),
        rt_threads: num("rt-threads", 4.0) as usize,
        per_conn_queue: get("dispatch").as_deref() == Some("perconn"),
        root_cert: get("root-cert"),
        rows: num("rows", 100_000.0) as i64,
        taxi_rows: num("taxi-rows", 1_000_000.0) as i64,
        repeats: num("repeats", 5.0) as usize,
        bulk_rows: num("bulk-rows", 100_000.0) as i64,
    }
}

/// State every worker shares.
#[derive(Debug)]
pub struct Shared {
    pub cfg: Cfg,
    pub t0: std::sync::OnceLock<Instant>,
    pub warm_ns: u64,
    pub end_ns: u64,
    /// Latency from the intended start.
    pub hist: Hist,
    /// Latency from the actual send.
    pub service: Hist,
    pub ok: AtomicU64,
    pub errors: AtomicU64,
    pub dropped: AtomicU64,
    pub sent: AtomicU64,
    pub checksum: AtomicU64,
    pub first_error: Mutex<Option<String>>,
}

impl Shared {
    /// Start the clock. Clients call this once their connections are open.
    pub fn start(&self) {
        let _ = self.t0.set(Instant::now());
        // The orchestrator samples process CPU relative to this line (bench/README.md).
        eprintln!("T0");
    }

    pub fn now_ns(&self) -> u64 {
        self.t0.get().expect("clock not started").elapsed().as_nanos() as u64
    }

    /// Past this point unfinished arrivals are dropped (counted), so an overloaded cell ends.
    pub fn deadline_ns(&self) -> u64 {
        self.end_ns + 5_000_000_000
    }

    pub fn record(&self, intended: u64, start: u64, end: u64, result: Result<u64, String>) {
        if intended < self.warm_ns || intended >= self.end_ns {
            return;
        }
        match result {
            Ok(sum) => {
                self.hist.record(end.saturating_sub(intended));
                self.service.record(end.saturating_sub(start));
                self.ok.fetch_add(1, Relaxed);
                self.checksum.fetch_add(sum, Relaxed);
            }
            Err(e) => {
                self.errors.fetch_add(1, Relaxed);
                let mut f = self.first_error.lock().unwrap();
                if f.is_none() {
                    *f = Some(e);
                }
            }
        }
    }

    pub fn drop_one(&self, intended: u64) {
        if intended >= self.warm_ns && intended < self.end_ns {
            self.dropped.fetch_add(1, Relaxed);
        }
    }
}

/// Emit arrivals at a fixed rate. `emit(queue, intended_ns)`. Runs on the caller's thread until
/// the window ends. Sleeps when ahead; when behind it emits every overdue arrival at once, so
/// lateness shows up as latency (coordinated omission is not hidden).
pub fn generate(sh: &Shared, nq: usize, mut emit: impl FnMut(usize, u64)) {
    let period = 1e9 / sh.cfg.rate;
    let mut k: u64 = 0;
    loop {
        let intended = (k as f64 * period) as u64;
        if intended >= sh.end_ns {
            return;
        }
        let now = sh.now_ns();
        if intended > now {
            std::thread::sleep(Duration::from_nanos(intended - now));
        }
        if intended >= sh.warm_ns {
            sh.sent.fetch_add(1, Relaxed);
        }
        emit((k as usize) % nq, intended);
        k += 1;
    }
}

/// Process CPU and peak RSS, from /proc (Linux); zeros elsewhere.
fn proc_stats() -> (f64, f64, u64) {
    let (mut user, mut sys, mut rss) = (0.0, 0.0, 0);
    if let Ok(s) = std::fs::read_to_string("/proc/self/stat") {
        // fields after the closing paren of comm
        if let Some(rest) = s.rsplit_once(')').map(|x| x.1) {
            let f: Vec<&str> = rest.split_whitespace().collect();
            // utime is field 14, stime 15 (1-based), i.e. index 11 and 12 after state at index 0
            let tick = 100.0;
            user = f.get(11).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) / tick;
            sys = f.get(12).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) / tick;
        }
    }
    if let Ok(s) = std::fs::read_to_string("/proc/self/status") {
        for l in s.lines() {
            if let Some(v) = l.strip_prefix("VmHWM:") {
                rss = v.trim().trim_end_matches("kB").trim().parse().unwrap_or(0);
            }
        }
    }
    (user, sys, rss)
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push(' '),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn main() {
    let cfg = parse_args();
    if let Some(line) = bulk::run(&cfg) {
        println!("{line}");
        return;
    }
    if cfg.mode == Mode::Pipelined && cfg.pool > 0 {
        die("--pool applies to unpipelined mode");
    }
    let sh = Arc::new(Shared {
        t0: std::sync::OnceLock::new(),
        warm_ns: cfg.warmup.as_nanos() as u64,
        end_ns: (cfg.warmup + cfg.duration).as_nanos() as u64,
        cfg: cfg.clone(),
        hist: Hist::new(),
        service: Hist::new(),
        ok: AtomicU64::new(0),
        errors: AtomicU64::new(0),
        dropped: AtomicU64::new(0),
        sent: AtomicU64::new(0),
        checksum: AtomicU64::new(0),
        first_error: Mutex::new(None),
    });
    // Connections are opened before the clock starts: the run measures requests, not logins.
    let started = Instant::now();
    let version = clients::run(&cfg.client, sh.clone());
    let wall = started.elapsed().as_secs_f64();
    let (user, sys, rss_kb) = proc_stats();
    let err = sh.first_error.lock().unwrap().clone();
    println!(
        "{{\"harness\":\"rustbench\",\"client\":{},\"version\":{},\"scenario\":\"{:?}\",\"mode\":\"{:?}\",\
\"conns\":{},\"pool\":{},\"rate\":{},\"duration_s\":{},\"warmup_s\":{},\"dispatch\":\"{}\",\"rt_threads\":{},\
\"sent\":{},\"ok\":{},\"errors\":{},\"dropped\":{},\"checksum\":{},\"first_error\":{},\
\"wall_s\":{:.3},\"cpu_user_s\":{:.2},\"cpu_sys_s\":{:.2},\"rss_kb\":{},\
\"hist\":{},\"service_hist\":{}}}",
        json_str(&cfg.client),
        json_str(&version),
        cfg.scenario,
        cfg.mode,
        cfg.conns,
        cfg.pool,
        cfg.rate,
        cfg.duration.as_secs_f64(),
        cfg.warmup.as_secs_f64(),
        if cfg.per_conn_queue { "perconn" } else { "shared" },
        cfg.rt_threads,
        sh.sent.load(Relaxed),
        sh.ok.load(Relaxed),
        sh.errors.load(Relaxed),
        sh.dropped.load(Relaxed),
        sh.checksum.load(Relaxed),
        err.as_deref().map(json_str).unwrap_or_else(|| "null".into()),
        wall,
        user,
        sys,
        rss_kb,
        sh.hist.to_json(),
        sh.service.to_json(),
    );
}
