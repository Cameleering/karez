//! Rust client adapters. Rules every adapter follows (bench/README.md, "adapter rules"):
//!
//! - open every connection before the clock starts;
//! - use the client's prepared or cached statement path (the best-practice one, as a user would);
//! - read every column of every row in its native type and fold it into the checksum;
//! - check the row count (and for S2 the id) and count a mismatch as an error;
//! - latency is measured from the intended start time, see `main.rs::generate`.

use crate::{die, generate, Mode, Scenario, Shared};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc;
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

/// xorshift64*: cheap, deterministic ids for S2.
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self, n: i64) -> i64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as i64 % n + 1
    }
}

pub fn run(client: &str, sh: Arc<Shared>) -> String {
    match client {
        "karez-blocking" => karez_blocking(sh),
        "karez-shared" => karez_shared(sh),
        "tokio-postgres" => tokio_pg(sh),
        "deadpool-postgres" => deadpool(sh),
        "zero-postgres" => zero_pg(sh),
        "sqlx" => sqlx_client(sh, false),
        "sqlx-tuned" => sqlx_client(sh, true),
        #[cfg(feature = "diesel-client")]
        "diesel" => crate::diesel_client::run(sh),
        #[cfg(feature = "wtx-client")]
        "wtx" => crate::wtx_client::run(sh),
        other => die(&format!(
            "unknown client {other} (karez-blocking, karez-shared, tokio-postgres, deadpool-postgres, zero-postgres, sqlx, sqlx-tuned)"
        )),
    }
}

// ---- arrival sources -------------------------------------------------------------------------

type SyncRx = Arc<Mutex<mpsc::Receiver<u64>>>;

pub enum SyncSrc {
    Open(SyncRx),
    Closed,
}

impl SyncSrc {
    pub fn next(&self, sh: &Shared) -> Option<u64> {
        match self {
            SyncSrc::Open(rx) => rx.lock().unwrap().recv().ok(),
            SyncSrc::Closed => {
                let n = sh.now_ns();
                (n < sh.end_ns).then_some(n)
            }
        }
    }
}

/// Queues for `workers` threads. Returns the senders (for the generator) and a source per worker.
pub fn sync_queues(sh: &Shared, workers: usize) -> (Vec<mpsc::Sender<u64>>, Vec<SyncSrc>) {
    if sh.cfg.rate == 0.0 {
        return (Vec::new(), (0..workers).map(|_| SyncSrc::Closed).collect());
    }
    let nq = if sh.cfg.per_conn_queue { workers } else { 1 };
    let mut txs = Vec::new();
    let mut rxs = Vec::new();
    for _ in 0..nq {
        let (tx, rx) = mpsc::channel();
        txs.push(tx);
        rxs.push(Arc::new(Mutex::new(rx)));
    }
    let srcs = (0..workers)
        .map(|w| SyncSrc::Open(rxs[w % nq].clone()))
        .collect();
    (txs, srcs)
}

pub fn run_generator(sh: &Shared, txs: Vec<mpsc::Sender<u64>>) {
    if txs.is_empty() {
        // closed loop: just let the window pass
        std::thread::sleep(Duration::from_nanos(sh.end_ns.saturating_sub(sh.now_ns())));
        return;
    }
    let n = txs.len();
    generate(sh, n, |q, t| {
        let _ = txs[q].send(t);
    });
    // dropping the senders ends the workers
}

// ---- karez, blocking -------------------------------------------------------------------------

fn karez_cfg(sh: &Shared) -> karez_pg::Config {
    karez_pg::Config::from_url(&sh.cfg.url).unwrap_or_else(|e| die(&format!("bad url: {e}")))
}

pub fn karez_taxi(row: &karez_pg::RowRef<'_>) -> Result<u64, String> {
    let mut s = row.get::<i64>(0).map_err(|x| x.to_string())? as u64;
    s = s.wrapping_add(row.get::<i16>(1).map_err(|x| x.to_string())? as u64);
    s = s.wrapping_add(row.get::<karez_pg::Timestamp>(2).map_err(|x| x.to_string())?.0 as u64);
    s = s.wrapping_add(row.get::<karez_pg::Timestamp>(3).map_err(|x| x.to_string())?.0 as u64);
    s = s.wrapping_add(row.get::<Option<i16>>(4).map_err(|x| x.to_string())?.unwrap_or(0) as u64);
    s = s.wrapping_add(row.get::<f64>(5).map_err(|x| x.to_string())?.to_bits());
    s = s.wrapping_add(row.get::<Option<i16>>(6).map_err(|x| x.to_string())?.unwrap_or(0) as u64);
    s = s.wrapping_add(row.get::<Option<&str>>(7).map_err(|x| x.to_string())?.map_or(0, |v| v.len()) as u64);
    s = s.wrapping_add(row.get::<i32>(8).map_err(|x| x.to_string())? as u64);
    s = s.wrapping_add(row.get::<i32>(9).map_err(|x| x.to_string())? as u64);
    s = s.wrapping_add(row.get::<i16>(10).map_err(|x| x.to_string())? as u64);
    for i in 11..=15 {
        s = s.wrapping_add(row.get::<f64>(i).map_err(|x| x.to_string())?.to_bits());
    }
    s = s.wrapping_add(row.get::<Option<f64>>(16).map_err(|x| x.to_string())?.map_or(0, |v| v.to_bits()));
    Ok(s)
}

fn karez_fold(row: &karez_pg::RowRef<'_>, sc: Scenario, want_id: i64, idx: usize) -> Result<u64, String> {
    match sc {
        Scenario::R1 | Scenario::R2 => {
            let s = karez_taxi(row)?;
            let id = row.get::<i64>(0).map_err(|x| x.to_string())?;
            let want = if sc == Scenario::R1 { want_id } else { want_id + idx as i64 };
            if id != want {
                return Err(format!("{sc:?} returned id {id}, wanted {want}"));
            }
            Ok(s)
        }
        Scenario::R3 => {
            let pu = row.get::<i32>(0).map_err(|x| x.to_string())?;
            if pu as i64 != want_id {
                return Err(format!("R3 returned zone {pu}, wanted {want_id}"));
            }
            Ok((pu as u64)
                .wrapping_add(row.get::<i64>(1).map_err(|x| x.to_string())? as u64)
                .wrapping_add(row.get::<f64>(2).map_err(|x| x.to_string())?.to_bits())
                .wrapping_add(row.get::<f64>(3).map_err(|x| x.to_string())?.to_bits()))
        }
        Scenario::S1 => Ok(row.get::<i32>(0).map_err(|x| x.to_string())? as u64),
        Scenario::S2 => {
            let id = row.get::<i64>(0).map_err(|x| x.to_string())?;
            if id != want_id {
                return Err(format!("S2 returned id {id}, wanted {want_id}"));
            }
            let mut s = id as u64;
            s = s.wrapping_add(row.get::<i32>(1).map_err(|x| x.to_string())? as u64);
            s = s.wrapping_add(row.get::<i16>(2).map_err(|x| x.to_string())? as u64);
            s = s.wrapping_add(row.get::<bool>(3).map_err(|x| x.to_string())? as u64);
            s = s.wrapping_add(row.get::<f64>(4).map_err(|x| x.to_string())?.to_bits());
            s = s.wrapping_add(row.get::<&str>(5).map_err(|x| x.to_string())?.len() as u64);
            s = s.wrapping_add(row.get::<&str>(6).map_err(|x| x.to_string())?.len() as u64);
            s = s.wrapping_add(row.get::<i64>(7).map_err(|x| x.to_string())? as u64);
            s = s.wrapping_add(row.get::<&str>(8).map_err(|x| x.to_string())?.len() as u64);
            s = s.wrapping_add(row.get::<i64>(9).map_err(|x| x.to_string())? as u64);
            Ok(s)
        }
        _ => {
            let id = row.get::<i32>(0).map_err(|x| x.to_string())? as u64;
            Ok(id.wrapping_add(row.get::<&str>(1).map_err(|x| x.to_string())?.len() as u64))
        }
    }
}

fn karez_params(sc: Scenario, id: i64) -> Vec<karez_pg::Value<'static>> {
    if !sc.has_param() {
        Vec::new()
    } else if sc.param_i32() {
        vec![karez_pg::Value::i32(id as i32)]
    } else {
        vec![karez_pg::Value::i64(id)]
    }
}

fn karez_query(
    conn: &mut karez_pg::Connection,
    sh: &Shared,
    rng: &mut Rng,
) -> Result<u64, String> {
    let (sc, id) = sh.cfg.scenario.request(rng, &sh.cfg);
    let params = karez_params(sc, id);
    let params: &[karez_pg::Value<'_>] = &params;
    let mut cur = conn.query(sc.sql(), params).map_err(|e| e.to_string())?;
    let (mut n, mut sum) = (0, 0u64);
    while let Some(row) = cur.next().map_err(|e| e.to_string())? {
        sum = sum.wrapping_add(karez_fold(&row, sc, id, n)?);
        n += 1;
    }
    if !sc.rows_ok(n) {
        return Err(format!("{n} rows, wanted {}", sc.rows()));
    }
    Ok(sum)
}

fn karez_blocking(sh: Arc<Shared>) -> String {
    if sh.cfg.mode == Mode::Pipelined {
        die("karez-blocking cannot pipeline; use karez-shared");
    }
    let workers = sh.cfg.conns;
    let pool = (sh.cfg.pool > 0).then(|| {
        let p = karez_pg::Pool::new(karez_cfg(&sh), sh.cfg.pool).unwrap_or_else(|e| die(&e.to_string()));
        // open the whole pool before the clock starts
        let held: Vec<_> = (0..sh.cfg.pool)
            .map(|_| p.get().unwrap_or_else(|e| die(&format!("pool: {e}"))))
            .collect();
        drop(held);
        p
    });
    let (txs, srcs) = sync_queues(&sh, workers);
    let b1 = Arc::new(Barrier::new(workers + 1));
    let b2 = Arc::new(Barrier::new(workers + 1));
    let mut hs = Vec::new();
    for (w, src) in srcs.into_iter().enumerate() {
        let (sh, pool, b1, b2) = (sh.clone(), pool.clone(), b1.clone(), b2.clone());
        hs.push(std::thread::spawn(move || {
            let mut own = if pool.is_none() {
                Some(karez_pg::Connection::connect(&karez_cfg(&sh)).unwrap_or_else(|e| die(&format!("connect: {e}"))))
            } else {
                None
            };
            let mut rng = Rng::new(w as u64 + 1);
            // prepare the statement before the clock starts
            if let Some(c) = own.as_mut() {
                for k in sh.cfg.scenario.kinds() {
                    let p = karez_params(k, 1);
                    if let Ok(mut cur) = c.query(k.sql(), &p) {
                        while let Ok(Some(_)) = cur.next() {}
                    }
                }
            }
            b1.wait();
            b2.wait();
            while let Some(intended) = src.next(&sh) {
                let start = sh.now_ns();
                if start > sh.deadline_ns() {
                    sh.drop_one(intended);
                    continue;
                }
                let r = match (&pool, own.as_mut()) {
                    (Some(p), _) => match p.get() {
                        Ok(mut c) => karez_query(&mut c, &sh, &mut rng),
                        Err(e) => Err(e.to_string()),
                    },
                    (None, Some(c)) => karez_query(c, &sh, &mut rng),
                    _ => unreachable!(),
                };
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
    format!("karez-pg {}", env!("CARGO_PKG_VERSION"))
}

// ---- karez-rt, shared connection ----------------------------------------------------------------

fn karez_shared(sh: Arc<Shared>) -> String {
    use karez_rt::{Options, SharedConnection};
    let cfg = karez_cfg(&sh);
    let conns: Vec<SharedConnection> = (0..sh.cfg.conns)
        .map(|_| SharedConnection::connect(&cfg, Options::default()).unwrap_or_else(|e| die(&format!("connect: {e}"))))
        .collect();
    let sc = sh.cfg.scenario;
    // prepare before the clock starts
    for c in &conns {
        for k in sc.kinds() {
            let p = karez_params(k, 1);
            c.collect(k.sql(), &p).unwrap_or_else(|e| die(&format!("prepare: {e}")));
        }
    }
    let pipelined = sh.cfg.mode == Mode::Pipelined;
    let workers = conns.len();
    let (txs, srcs) = sync_queues(&sh, workers);
    let b1 = Arc::new(Barrier::new(workers + 1));
    let b2 = Arc::new(Barrier::new(workers + 1));
    let mut hs = Vec::new();
    for (w, (src, conn)) in srcs.into_iter().zip(conns.iter().cloned()).enumerate() {
        let (sh, b1, b2) = (sh.clone(), b1.clone(), b2.clone());
        // Pipelined: this thread submits, a collector thread waits for the answers in order.
        let (ctx, crx) = mpsc::channel::<(karez_rt::Pending, u64, u64, i64, Scenario)>();
        let collector = {
            let sh = sh.clone();
            std::thread::spawn(move || {
                while let Ok((pending, intended, start, id, qsc)) = crx.recv() {
                    let r = pending
                        .wait()
                        .map_err(|e| e.to_string())
                        .and_then(|d| shared_fold(&d, qsc, id));
                    sh.record(intended, start, sh.now_ns(), r);
                }
            })
        };
        hs.push(std::thread::spawn(move || {
            let mut rng = Rng::new(w as u64 + 1);
            b1.wait();
            b2.wait();
            while let Some(intended) = src.next(&sh) {
                let start = sh.now_ns();
                if start > sh.deadline_ns() {
                    sh.drop_one(intended);
                    continue;
                }
                let (qsc, id) = sc.request(&mut rng, &sh.cfg);
                let p = karez_params(qsc, id);
                match conn.submit_collect(qsc.sql(), &p) {
                    Ok(pending) => {
                        if pipelined {
                            let _ = ctx.send((pending, intended, start, id, qsc));
                        } else {
                            let r = pending
                                .wait()
                                .map_err(|e| e.to_string())
                                .and_then(|d| shared_fold(&d, qsc, id));
                            sh.record(intended, start, sh.now_ns(), r);
                        }
                    }
                    Err(e) => sh.record(intended, start, sh.now_ns(), Err(e.to_string())),
                }
            }
            drop(ctx);
            collector.join().unwrap();
        }));
    }
    b1.wait();
    sh.start();
    b2.wait();
    run_generator(&sh, txs);
    for h in hs {
        h.join().unwrap();
    }
    for c in &conns {
        c.close();
    }
    format!("karez-rt {}", env!("CARGO_PKG_VERSION"))
}

fn shared_fold(d: &karez_rt::engine::Done, sc: Scenario, id: i64) -> Result<u64, String> {
    let mut n = 0;
    let mut sum = 0u64;
    for ch in &d.chunks {
        for r in 0..ch.rows() {
            let row = ch.row(r).map_err(|e| e.to_string())?;
            sum = sum.wrapping_add(karez_fold(&row, sc, id, n)?);
            n += 1;
        }
    }
    if !sc.rows_ok(n) {
        return Err(format!("{n} rows, wanted {}", sc.rows()));
    }
    Ok(sum)
}

// ---- tokio helpers -----------------------------------------------------------------------------

use tokio::sync::mpsc as tmpsc;
type AsyncRx = Arc<tokio::sync::Mutex<tmpsc::UnboundedReceiver<u64>>>;

enum AsyncSrc {
    Open(AsyncRx),
    Closed,
}

impl AsyncSrc {
    async fn next(&self, sh: &Shared) -> Option<u64> {
        match self {
            AsyncSrc::Open(rx) => rx.lock().await.recv().await,
            AsyncSrc::Closed => {
                let n = sh.now_ns();
                if n >= sh.end_ns {
                    None
                } else {
                    // yield so a closed loop shares the runtime fairly
                    tokio::task::yield_now().await;
                    Some(n)
                }
            }
        }
    }
}

fn async_queues(sh: &Shared, workers: usize) -> (Vec<tmpsc::UnboundedSender<u64>>, Vec<AsyncSrc>) {
    if sh.cfg.rate == 0.0 {
        return (Vec::new(), (0..workers).map(|_| AsyncSrc::Closed).collect());
    }
    let nq = if sh.cfg.per_conn_queue { workers } else { 1 };
    let mut txs = Vec::new();
    let mut rxs = Vec::new();
    for _ in 0..nq {
        let (tx, rx) = tmpsc::unbounded_channel();
        txs.push(tx);
        rxs.push(Arc::new(tokio::sync::Mutex::new(rx)));
    }
    let srcs = (0..workers)
        .map(|w| AsyncSrc::Open(rxs[w % nq].clone()))
        .collect();
    (txs, srcs)
}

fn async_generator(sh: Arc<Shared>, txs: Vec<tmpsc::UnboundedSender<u64>>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        if txs.is_empty() {
            return;
        }
        let n = txs.len();
        generate(&sh, n, |q, t| {
            let _ = txs[q].send(t);
        });
    })
}

fn runtime(sh: &Shared) -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(sh.cfg.rt_threads.max(1))
        .enable_all()
        .build()
        .unwrap()
}

// ---- tokio-postgres ----------------------------------------------------------------------------

fn tp_us(d: time::PrimitiveDateTime) -> u64 {
    (d.assume_utc().unix_timestamp_nanos() / 1000) as i64 as u64
}

pub fn tp_taxi(r: &tokio_postgres::Row) -> u64 {
    let mut s = r.get::<_, i64>(0) as u64;
    s = s.wrapping_add(r.get::<_, i16>(1) as u64);
    s = s.wrapping_add(tp_us(r.get(2)));
    s = s.wrapping_add(tp_us(r.get(3)));
    s = s.wrapping_add(r.get::<_, Option<i16>>(4).unwrap_or(0) as u64);
    s = s.wrapping_add(r.get::<_, f64>(5).to_bits());
    s = s.wrapping_add(r.get::<_, Option<i16>>(6).unwrap_or(0) as u64);
    s = s.wrapping_add(r.get::<_, Option<&str>>(7).map_or(0, |v| v.len()) as u64);
    s = s.wrapping_add(r.get::<_, i32>(8) as u64);
    s = s.wrapping_add(r.get::<_, i32>(9) as u64);
    s = s.wrapping_add(r.get::<_, i16>(10) as u64);
    for i in 11..=15 {
        s = s.wrapping_add(r.get::<_, f64>(i).to_bits());
    }
    s.wrapping_add(r.get::<_, Option<f64>>(16).map_or(0, |v| v.to_bits()))
}

fn tp_fold(rows: &[tokio_postgres::Row], sc: Scenario, id: i64) -> Result<u64, String> {
    if !sc.rows_ok(rows.len()) {
        return Err(format!("{} rows, wanted {}", rows.len(), sc.rows()));
    }
    let mut sum = 0u64;
    for (i, r) in rows.iter().enumerate() {
        match sc {
            Scenario::R1 | Scenario::R2 => {
                let rid: i64 = r.get(0);
                let want = if sc == Scenario::R1 { id } else { id + i as i64 };
                if rid != want {
                    return Err(format!("{sc:?} returned id {rid}, wanted {want}"));
                }
                sum = sum.wrapping_add(tp_taxi(r));
            }
            Scenario::R3 => {
                let pu: i32 = r.get(0);
                if pu as i64 != id {
                    return Err(format!("R3 returned zone {pu}, wanted {id}"));
                }
                sum = sum
                    .wrapping_add(pu as u64)
                    .wrapping_add(r.get::<_, i64>(1) as u64)
                    .wrapping_add(r.get::<_, f64>(2).to_bits())
                    .wrapping_add(r.get::<_, f64>(3).to_bits());
            }
            Scenario::S1 => sum = sum.wrapping_add(r.get::<_, i32>(0) as u64),
            Scenario::S2 => {
                let rid: i64 = r.get(0);
                if rid != id {
                    return Err(format!("S2 returned id {rid}, wanted {id}"));
                }
                sum = sum
                    .wrapping_add(rid as u64)
                    .wrapping_add(r.get::<_, i32>(1) as u64)
                    .wrapping_add(r.get::<_, i16>(2) as u64)
                    .wrapping_add(r.get::<_, bool>(3) as u64)
                    .wrapping_add(r.get::<_, f64>(4).to_bits())
                    .wrapping_add(r.get::<_, &str>(5).len() as u64)
                    .wrapping_add(r.get::<_, &str>(6).len() as u64)
                    .wrapping_add(r.get::<_, i64>(7) as u64)
                    .wrapping_add(r.get::<_, &str>(8).len() as u64)
                    .wrapping_add(r.get::<_, i64>(9) as u64);
            }
            _ => {
                sum = sum
                    .wrapping_add(r.get::<_, i32>(0) as u64)
                    .wrapping_add(r.get::<_, &str>(1).len() as u64)
            }
        }
    }
    Ok(sum)
}

/// tokio-postgres does not know `sslrootcert` or `sslmode=verify-full`: the rustls connector below
/// verifies the chain and the host name, so the URL asks for `require` and drops the file name.
pub fn tp_url_pub(url: &str) -> String {
    tp_url(url)
}

fn tp_url(url: &str) -> String {
    let Some((base, q)) = url.split_once('?') else { return url.to_string() };
    let kept: Vec<String> = q
        .split('&')
        .filter(|kv| !kv.starts_with("sslrootcert="))
        .map(|kv| if kv == "sslmode=verify-full" || kv == "sslmode=verify-ca" { "sslmode=require".to_string() } else { kv.to_string() })
        .collect();
    format!("{base}?{}", kept.join("&"))
}

async fn tp_connect(sh: &Shared) -> tokio_postgres::Client {
    let url = &tp_url(&sh.cfg.url);
    #[cfg(feature = "tls")]
    if url.contains("sslmode=verify") || url.contains("sslmode=require") {
        let tls = tls_connector(sh);
        let (c, conn) = tokio_postgres::connect(url, tls)
            .await
            .unwrap_or_else(|e| die(&format!("connect: {e}")));
        tokio::spawn(async move {
            let _ = conn.await;
        });
        return c;
    }
    let (c, conn) = tokio_postgres::connect(url, tokio_postgres::NoTls)
        .await
        .unwrap_or_else(|e| die(&format!("connect: {e}")));
    tokio::spawn(async move {
        let _ = conn.await;
    });
    c
}

#[cfg(feature = "tls")]
fn tls_connector(sh: &Shared) -> tokio_postgres_rustls::MakeRustlsConnect {
    let mut roots = rustls::RootCertStore::empty();
    if let Some(p) = &sh.cfg.root_cert {
        let f = std::fs::File::open(p).unwrap_or_else(|e| die(&format!("root-cert: {e}")));
        for c in rustls_pemfile::certs(&mut std::io::BufReader::new(f)) {
            roots.add(c.unwrap()).unwrap();
        }
    }
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tokio_postgres_rustls::MakeRustlsConnect::new(cfg)
}

fn tokio_pg(sh: Arc<Shared>) -> String {
    if sh.cfg.pool > 0 {
        die("tokio-postgres has no pool; use deadpool-postgres for --pool");
    }
    let rt = runtime(&sh);
    let sc = sh.cfg.scenario;
    let pipelined = sh.cfg.mode == Mode::Pipelined;
    rt.block_on(async {
        let n = sh.cfg.conns;
        let mut clients = Vec::new();
        for _ in 0..n {
            let c = tp_connect(&sh).await;
            let mut sts = Vec::new();
            for k in sc.kinds() {
                sts.push((k, c.prepare(k.sql()).await.unwrap_or_else(|e| die(&format!("prepare: {e}")))));
            }
            clients.push((Arc::new(c), Arc::new(sts)));
        }
        let (txs, srcs) = async_queues(&sh, n);
        sh.start();
        let gen = async_generator(sh.clone(), txs);
        let mut hs = Vec::new();
        for (w, (src, (client, st))) in srcs.into_iter().zip(clients).enumerate() {
            let sh = sh.clone();
            hs.push(tokio::spawn(async move {
                let mut rng = Rng::new(w as u64 + 1);
                let mut tasks = Vec::new();
                while let Some(intended) = src.next(&sh).await {
                    let start = sh.now_ns();
                    if start > sh.deadline_ns() {
                        sh.drop_one(intended);
                        continue;
                    }
                    let (qsc, id) = sc.request(&mut rng, &sh.cfg);
                    let (client, st, sh2) = (client.clone(), st.clone(), sh.clone());
                    let fut = async move {
                        let stmt = &st.iter().find(|(k, _)| *k == qsc).unwrap().1;
                        let id32 = id as i32;
                        let r = if !qsc.has_param() {
                            client.query(stmt, &[]).await
                        } else if qsc.param_i32() {
                            client.query(stmt, &[&id32]).await
                        } else {
                            client.query(stmt, &[&id]).await
                        };
                        let r = r.map_err(|e| e.to_string()).and_then(|rows| tp_fold(&rows, qsc, id));
                        sh2.record(intended, start, sh2.now_ns(), r);
                    };
                    if pipelined {
                        tasks.push(tokio::spawn(fut));
                    } else {
                        fut.await;
                    }
                }
                for t in tasks {
                    let _ = t.await;
                }
            }));
        }
        for h in hs {
            let _ = h.await;
        }
        gen.join().unwrap();
    });
    format!("tokio-postgres {}", "0.7")
}

// ---- deadpool-postgres ---------------------------------------------------------------------------

fn deadpool(sh: Arc<Shared>) -> String {
    if sh.cfg.mode == Mode::Pipelined {
        die("deadpool-postgres: pipelined mode is the tokio-postgres client");
    }
    let rt = runtime(&sh);
    let sc = sh.cfg.scenario;
    rt.block_on(async {
        let pg: tokio_postgres::Config = tp_url(&sh.cfg.url).parse().unwrap_or_else(|e| die(&format!("url: {e}")));
        let size = if sh.cfg.pool > 0 { sh.cfg.pool } else { sh.cfg.conns };
        let mgr = deadpool_postgres::Manager::from_config(
            pg,
            tokio_postgres::NoTls,
            deadpool_postgres::ManagerConfig {
                recycling_method: deadpool_postgres::RecyclingMethod::Fast,
            },
        );
        let pool = deadpool_postgres::Pool::builder(mgr)
            .max_size(size)
            .build()
            .unwrap();
        // open the whole pool and prepare before the clock starts
        let mut held = Vec::new();
        for _ in 0..size {
            let c = pool.get().await.unwrap_or_else(|e| die(&format!("pool: {e}")));
            for k in sc.kinds() {
                c.prepare_cached(k.sql()).await.unwrap_or_else(|e| die(&format!("prepare: {e}")));
            }
            held.push(c);
        }
        drop(held);
        let workers = sh.cfg.conns;
        let (txs, srcs) = async_queues(&sh, workers);
        sh.start();
        let gen = async_generator(sh.clone(), txs);
        let mut hs = Vec::new();
        for (w, src) in srcs.into_iter().enumerate() {
            let (sh, pool) = (sh.clone(), pool.clone());
            hs.push(tokio::spawn(async move {
                let mut rng = Rng::new(w as u64 + 1);
                while let Some(intended) = src.next(&sh).await {
                    let start = sh.now_ns();
                    if start > sh.deadline_ns() {
                        sh.drop_one(intended);
                        continue;
                    }
                    let (qsc, id) = sc.request(&mut rng, &sh.cfg);
                    let r = async {
                        let c = pool.get().await.map_err(|e| e.to_string())?;
                        let st = c.prepare_cached(qsc.sql()).await.map_err(|e| e.to_string())?;
                        let id32 = id as i32;
                        let rows = if !qsc.has_param() {
                            c.query(&st, &[]).await
                        } else if qsc.param_i32() {
                            c.query(&st, &[&id32]).await
                        } else {
                            c.query(&st, &[&id]).await
                        }
                        .map_err(|e| e.to_string())?;
                        tp_fold(&rows, qsc, id)
                    }
                    .await;
                    sh.record(intended, start, sh.now_ns(), r);
                }
            }));
        }
        for h in hs {
            let _ = h.await;
        }
        gen.join().unwrap();
    });
    "deadpool-postgres 0.14".to_string()
}

// ---- sqlx ------------------------------------------------------------------------------------------

fn sqlx_client(sh: Arc<Shared>, tuned: bool) -> String {
    use sqlx::{Executor, Row};
    if sh.cfg.mode == Mode::Pipelined {
        die("sqlx: unpipelined only (a pool of connections)");
    }
    let rt = runtime(&sh);
    let sc = sh.cfg.scenario;
    rt.block_on(async {
        let size = if sh.cfg.pool > 0 { sh.cfg.pool } else { sh.cfg.conns } as u32;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .min_connections(size)
            .max_connections(size)
            // defaults ping the connection on every acquire (one extra round trip); tuned does not
            .test_before_acquire(!tuned)
            .acquire_timeout(Duration::from_secs(60))
            .connect(&sh.cfg.url)
            .await
            .unwrap_or_else(|e| die(&format!("connect: {e}")));
        // warm every connection and its statement cache before the clock starts
        let mut held = Vec::new();
        for _ in 0..size {
            let mut c = pool.acquire().await.unwrap();
            for k in sc.kinds() {
                let q = sqlx::query(k.sql());
                let q = if !k.has_param() { q } else if k.param_i32() { q.bind(1i32) } else { q.bind(1i64) };
                let _ = c.fetch_all(q).await;
            }
            held.push(c);
        }
        drop(held);
        let workers = sh.cfg.conns;
        let (txs, srcs) = async_queues(&sh, workers);
        sh.start();
        let gen = async_generator(sh.clone(), txs);
        let mut hs = Vec::new();
        for (w, src) in srcs.into_iter().enumerate() {
            let (sh, pool) = (sh.clone(), pool.clone());
            hs.push(tokio::spawn(async move {
                let mut rng = Rng::new(w as u64 + 1);
                while let Some(intended) = src.next(&sh).await {
                    let start = sh.now_ns();
                    if start > sh.deadline_ns() {
                        sh.drop_one(intended);
                        continue;
                    }
                    let (qsc, id) = sc.request(&mut rng, &sh.cfg);
                    let q = sqlx::query(qsc.sql());
                    let q = if !qsc.has_param() { q } else if qsc.param_i32() { q.bind(id as i32) } else { q.bind(id) };
                    let r = q.fetch_all(&pool).await.map_err(|e| e.to_string()).and_then(|rows| {
                        if !qsc.rows_ok(rows.len()) {
                            return Err(format!("{} rows, wanted {}", rows.len(), qsc.rows()));
                        }
                        let mut sum = 0u64;
                        for (ri, r) in rows.iter().enumerate() {
                            match qsc {
                                Scenario::R1 | Scenario::R2 => {
                                    let rid: i64 = r.get(0);
                                    let want = if qsc == Scenario::R1 { id } else { id + ri as i64 };
                                    if rid != want {
                                        return Err(format!("{qsc:?} returned id {rid}, wanted {want}"));
                                    }
                                    let us = |i: usize| -> u64 { (r.get::<time::PrimitiveDateTime, _>(i).assume_utc().unix_timestamp_nanos() / 1000) as i64 as u64 };
                                    let mut s = (rid as u64)
                                        .wrapping_add(r.get::<i16, _>(1) as u64)
                                        .wrapping_add(us(2))
                                        .wrapping_add(us(3))
                                        .wrapping_add(r.get::<Option<i16>, _>(4).unwrap_or(0) as u64)
                                        .wrapping_add(r.get::<f64, _>(5).to_bits())
                                        .wrapping_add(r.get::<Option<i16>, _>(6).unwrap_or(0) as u64)
                                        .wrapping_add(r.get::<Option<&str>, _>(7).map_or(0, |v| v.len()) as u64)
                                        .wrapping_add(r.get::<i32, _>(8) as u64)
                                        .wrapping_add(r.get::<i32, _>(9) as u64)
                                        .wrapping_add(r.get::<i16, _>(10) as u64);
                                    for i in 11..=15 {
                                        s = s.wrapping_add(r.get::<f64, _>(i).to_bits());
                                    }
                                    s = s.wrapping_add(r.get::<Option<f64>, _>(16).map_or(0, |v| v.to_bits()));
                                    sum = sum.wrapping_add(s);
                                }
                                Scenario::R3 => {
                                    let pu: i32 = r.get(0);
                                    if pu as i64 != id {
                                        return Err(format!("R3 returned zone {pu}, wanted {id}"));
                                    }
                                    sum = sum
                                        .wrapping_add(pu as u64)
                                        .wrapping_add(r.get::<i64, _>(1) as u64)
                                        .wrapping_add(r.get::<f64, _>(2).to_bits())
                                        .wrapping_add(r.get::<f64, _>(3).to_bits());
                                }
                                Scenario::S1 => sum = sum.wrapping_add(r.get::<i32, _>(0) as u64),
                                Scenario::S2 => {
                                    let rid: i64 = r.get(0);
                                    if rid != id {
                                        return Err(format!("S2 returned id {rid}, wanted {id}"));
                                    }
                                    sum = sum
                                        .wrapping_add(rid as u64)
                                        .wrapping_add(r.get::<i32, _>(1) as u64)
                                        .wrapping_add(r.get::<i16, _>(2) as u64)
                                        .wrapping_add(r.get::<bool, _>(3) as u64)
                                        .wrapping_add(r.get::<f64, _>(4).to_bits())
                                        .wrapping_add(r.get::<&str, _>(5).len() as u64)
                                        .wrapping_add(r.get::<&str, _>(6).len() as u64)
                                        .wrapping_add(r.get::<i64, _>(7) as u64)
                                        .wrapping_add(r.get::<&str, _>(8).len() as u64)
                                        .wrapping_add(r.get::<i64, _>(9) as u64);
                                }
                                _ => {
                                    sum = sum
                                        .wrapping_add(r.get::<i32, _>(0) as u64)
                                        .wrapping_add(r.get::<&str, _>(1).len() as u64)
                                }
                            }
                        }
                        Ok(sum)
                    });
                    sh.record(intended, start, sh.now_ns(), r);
                }
            }));
        }
        for h in hs {
            let _ = h.await;
        }
        gen.join().unwrap();
    });
    if tuned { "sqlx 0.8 (tuned: no ping on acquire)" } else { "sqlx 0.8 (defaults)" }.to_string()
}

#[allow(dead_code)]
fn _unused(sh: &Shared) -> u64 {
    sh.ok.load(Relaxed)
}

// ---- zero-postgres (tokio) -------------------------------------------------------------------------

fn zero_pg(sh: Arc<Shared>) -> String {
    use zero_postgres::tokio::Conn;
    if sh.cfg.mode == Mode::Pipelined || sh.cfg.pool > 0 {
        die("zero-postgres: unpipelined, no pool");
    }
    if matches!(sh.cfg.scenario, Scenario::R1 | Scenario::R2 | Scenario::R3 | Scenario::R4) {
        die("zero-postgres: no taxi scenarios (its tuple decoder stops at 12 columns)");
    }
    let rt = runtime(&sh);
    let sc = sh.cfg.scenario;
    rt.block_on(async {
        let n = sh.cfg.conns;
        let mut conns = Vec::new();
        for _ in 0..n {
            let mut c = Conn::new(sh.cfg.url.as_str()).await.unwrap_or_else(|e| die(&format!("connect: {e}")));
            let st = c.prepare(sc.sql()).await.unwrap_or_else(|e| die(&format!("prepare: {e}")));
            conns.push((c, st));
        }
        let (txs, srcs) = async_queues(&sh, n);
        sh.start();
        let gen = async_generator(sh.clone(), txs);
        let mut hs = Vec::new();
        for (w, (src, (mut c, st))) in srcs.into_iter().zip(conns).enumerate() {
            let sh = sh.clone();
            hs.push(tokio::spawn(async move {
                let mut rng = Rng::new(w as u64 + 1);
                while let Some(intended) = src.next(&sh).await {
                    let start = sh.now_ns();
                    if start > sh.deadline_ns() {
                        sh.drop_one(intended);
                        continue;
                    }
                    let id = if sc == Scenario::S2 { rng.next(sh.cfg.rows) } else { 0 };
                    let r: Result<u64, String> = async {
                        match sc {
                            Scenario::S1 => {
                                let rows: Vec<(i32,)> = c.exec_collect(&st, ()).await.map_err(|e| e.to_string())?;
                                if rows.len() != 1 {
                                    return Err(format!("{} rows, wanted 1", rows.len()));
                                }
                                Ok(rows[0].0 as u64)
                            }
                            Scenario::S2 => {
                                type R = (i64, i32, i16, bool, f64, String, String, i64, String, i64);
                                let rows: Vec<R> = c.exec_collect(&st, (id,)).await.map_err(|e| e.to_string())?;
                                if rows.len() != 1 {
                                    return Err(format!("{} rows, wanted 1", rows.len()));
                                }
                                let r = &rows[0];
                                if r.0 != id {
                                    return Err(format!("S2 returned id {}, wanted {id}", r.0));
                                }
                                Ok((r.0 as u64)
                                    .wrapping_add(r.1 as u64)
                                    .wrapping_add(r.2 as u64)
                                    .wrapping_add(r.3 as u64)
                                    .wrapping_add(r.4.to_bits())
                                    .wrapping_add(r.5.len() as u64)
                                    .wrapping_add(r.6.len() as u64)
                                    .wrapping_add(r.7 as u64)
                                    .wrapping_add(r.8.len() as u64)
                                    .wrapping_add(r.9 as u64))
                            }
                            _ => {
                                let rows: Vec<(i32, String)> = c.exec_collect(&st, ()).await.map_err(|e| e.to_string())?;
                                if rows.len() != 12 {
                                    return Err(format!("{} rows, wanted 12", rows.len()));
                                }
                                Ok(rows.iter().fold(0u64, |a, r| a.wrapping_add(r.0 as u64).wrapping_add(r.1.len() as u64)))
                            }
                        }
                    }
                    .await;
                    sh.record(intended, start, sh.now_ns(), r);
                }
            }));
        }
        for h in hs {
            let _ = h.await;
        }
        gen.join().unwrap();
    });
    "zero-postgres 0.9".to_string()
}
