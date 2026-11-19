//! wtx 0.52 PostgreSQL client adapter: S1 and S2 (open loop, one blocking `TcpStream` connection per
//! worker, plaintext, driven by wtx's own `StdRuntime`) Not covered: the taxi
//! scenarios (timestamp decoding not wired), TLS, pool, pipelining, and every bulk scenario: wtx's
//! `execute_stmt_many` keeps the whole result in its read buffer and refused the 1M-row S4 result
//! with `NetError(ForbiddenClear)` at the default buffer limit; no streaming call was found.

use crate::clients::{run_generator, sync_queues, Rng};
use crate::{die, Mode, Scenario, Shared};
use std::net::TcpStream;
use std::sync::{Arc, Barrier};
use wtx::{
    database::{
        client::postgres::{ClientBuffer, Config, PostgresClient},
        DbClient as _, Record,
    },
    executor::StdRuntime,
    net::UriRef,
    rng::{ChaCha20, CryptoSeedableRng},
    tls::{PlaintextCtx, TlsConfig, TlsConnectorBuilder},
};

type Client = PostgresClient<wtx::Error, TcpStream, PlaintextCtx>;

/// wtx takes a URI without karez/libpq query options.
fn uri(raw: &str) -> String {
    raw.split_once('?').map_or(raw, |(b, _)| b).to_string()
}

async fn connect(raw: &str) -> Client {
    let u = uri(raw);
    let uri = UriRef::new(u.as_str());
    let mut rng = ChaCha20::from_getrandom().unwrap_or_else(|e| die(&format!("rng: {e:?}")));
    let cb = ClientBuffer::new(usize::MAX, &mut rng);
    PostgresClient::connect(
        cb,
        &Config::from_uri(&uri).unwrap_or_else(|e| die(&format!("uri: {e:?}"))),
        TlsConnectorBuilder::std(uri)
            .build(TlsConfig::plaintext(), rng)
            .await
            .unwrap_or_else(|e| die(&format!("tls: {e:?}"))),
    )
    .await
    .unwrap_or_else(|e| die(&format!("connect: {e:?}")))
}

const S2_SQL: &str = "SELECT id, k1, k2, flag, score, name, note, ref, tag, big FROM bench_item WHERE id = $1";

async fn query(c: &mut Client, sc: Scenario, id: i64) -> Result<u64, String> {
    let e = |x: wtx::Error| format!("{x:?}");
    match sc {
        Scenario::S1 => {
            let r = c.execute_stmt_single(sc.sql(), ()).await.map_err(e)?;
            Ok(r.decode::<_, i32>(0).map_err(e)? as u64)
        }
        Scenario::S2 => {
            let r = c.execute_stmt_single(S2_SQL, (id,)).await.map_err(e)?;
            let got = r.decode::<_, i64>(0).map_err(e)?;
            if got != id {
                return Err(format!("S2 returned id {got}, wanted {id}"));
            }
            let mut s = got as u64;
            s = s.wrapping_add(r.decode::<_, i32>(1).map_err(e)? as u64);
            s = s.wrapping_add(r.decode::<_, i16>(2).map_err(e)? as u64);
            s = s.wrapping_add(r.decode::<_, bool>(3).map_err(e)? as u64);
            s = s.wrapping_add(r.decode::<_, f64>(4).map_err(e)?.to_bits());
            s = s.wrapping_add(r.decode::<_, &str>(5).map_err(e)?.len() as u64);
            s = s.wrapping_add(r.decode::<_, &str>(6).map_err(e)?.len() as u64);
            s = s.wrapping_add(r.decode::<_, i64>(7).map_err(e)? as u64);
            s = s.wrapping_add(r.decode::<_, &str>(8).map_err(e)?.len() as u64);
            Ok(s.wrapping_add(r.decode::<_, i64>(9).map_err(e)? as u64))
        }
        other => die(&format!("wtx: scenario {other:?} not supported (s1, s2, s4)")),
    }
}

pub fn run(sh: Arc<Shared>) -> String {
    if sh.cfg.mode == Mode::Pipelined {
        die("wtx adapter: unpipelined only");
    }
    if sh.cfg.pool > 0 {
        die("wtx adapter: no pool (one connection per worker)");
    }
    let workers = sh.cfg.conns;
    let (txs, srcs) = sync_queues(&sh, workers);
    let b1 = Arc::new(Barrier::new(workers + 1));
    let b2 = Arc::new(Barrier::new(workers + 1));
    let mut hs = Vec::new();
    for (w, src) in srcs.into_iter().enumerate() {
        let (sh, b1, b2) = (sh.clone(), b1.clone(), b2.clone());
        hs.push(std::thread::spawn(move || {
            StdRuntime::new().block_on(async move {
                let mut c = connect(&sh.cfg.url).await;
                let mut rng = Rng::new(w as u64 + 1);
                for k in sh.cfg.scenario.kinds() {
                    let _ = query(&mut c, k, 1).await;
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
                    let r = query(&mut c, sc, id).await;
                    let end = sh.now_ns();
                    sh.record(intended, start, end, r);
                }
            })
        }));
    }
    b1.wait();
    sh.start();
    b2.wait();
    run_generator(&sh, txs);
    for h in hs {
        h.join().unwrap();
    }
    "wtx 0.52".to_string()
}
