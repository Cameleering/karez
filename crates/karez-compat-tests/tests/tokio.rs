//! karez-rt futures on tokio, against a real PostgreSQL. The address comes from `KAREZ_TEST_PG`;
//! without it each test prints `SKIPPED (not verified against a real server)` and returns, unless
//! `KAREZ_REQUIRE_SERVER=1`, which turns a skip into a failure.

use karez_pg::{Config, Value};
use karez_rt::{Error, Options, SharedConnection};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

fn server() -> Result<String, String> {
    let addr =
        std::env::var("KAREZ_TEST_PG").map_err(|_| "KAREZ_TEST_PG is not set".to_string())?;
    let sock = addr
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve {addr}: {e}"))?
        .next()
        .ok_or_else(|| format!("{addr} resolves to nothing"))?;
    TcpStream::connect_timeout(&sock, Duration::from_secs(2))
        .map_err(|e| format!("{addr} is not reachable: {e}"))?;
    Ok(addr)
}

macro_rules! need_server {
    () => {
        match server() {
            Ok(a) => a,
            Err(why) => {
                eprintln!("SKIPPED (not verified against a real server): {why}");
                if std::env::var("KAREZ_REQUIRE_SERVER").as_deref() == Ok("1") {
                    panic!("KAREZ_REQUIRE_SERVER=1 but there is no server: {why}");
                }
                return;
            }
        }
    };
}

fn connect(addr: &str) -> SharedConnection {
    let cfg = Config::from_url(&format!(
        "postgresql://karez_scram:karez_test_only_scram@{addr}/karez?sslmode=disable"
    ))
    .unwrap();
    SharedConnection::connect(&cfg, Options::default()).unwrap()
}

async fn one(c: &SharedConnection, i: i64) -> Result<i64, Error> {
    let d = c
        .submit_collect("SELECT $1::int8", &[Value::i64(i)])?
        .await?;
    Ok(d.chunks[0].row(0).unwrap().get::<i64>(0).unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_tasks_share_one_connection_on_a_multi_thread_runtime() {
    let addr = need_server!();
    let c = connect(&addr);
    let mut tasks = Vec::new();
    for t in 0..32i64 {
        let c = c.clone();
        tasks.push(tokio::spawn(async move {
            let mut sum = 0;
            for i in 0..50i64 {
                sum += one(&c, t * 100 + i).await.unwrap();
            }
            sum
        }));
    }
    for (t, h) in tasks.into_iter().enumerate() {
        let t = t as i64;
        assert_eq!(h.await.unwrap(), (0..50).map(|i| t * 100 + i).sum::<i64>());
    }
    assert_eq!(c.in_flight(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_current_thread_runtime_is_never_blocked_by_the_connection() {
    let addr = need_server!();
    let c = connect(&addr);
    // Prepare once (the first use of a statement blocks for one round trip, see karez-rt.md).
    one(&c, 0).await.unwrap();
    // A slow statement and a ticking timer on the same single thread: the timer must keep firing.
    let slow = c.submit_collect("SELECT pg_sleep(0.5)", &[]).unwrap();
    let mut ticks = 0;
    let waiter = tokio::spawn(async move { slow.await.map(|_| ()) });
    let tick = async {
        for _ in 0..4 {
            tokio::time::sleep(Duration::from_millis(80)).await;
            ticks += 1;
        }
    };
    tick.await;
    assert_eq!(ticks, 4, "the executor thread was blocked");
    waiter.await.unwrap().unwrap();
    // Futures race: many queued at once, awaited together.
    let (a, b, d) = tokio::join!(one(&c, 1), one(&c, 2), one(&c, 3));
    assert_eq!((a.unwrap(), b.unwrap(), d.unwrap()), (1, 2, 3));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_and_a_dropped_future_work_on_tokio() {
    let addr = need_server!();
    let c = connect(&addr);
    let mut s = c
        .stream("SELECT id FROM audit_log WHERE id <= 20000", &[])
        .unwrap();
    let mut rows = 0;
    while let Some(ch) = s.next_chunk_async().await.unwrap() {
        rows += ch.rows();
    }
    assert_eq!(rows, 20_000);
    // A future dropped before it completes (a tokio timeout) discards its answer and the
    // connection carries on in step.
    let r = tokio::time::timeout(
        Duration::from_millis(100),
        c.submit_collect("SELECT pg_sleep(1)", &[]).unwrap(),
    )
    .await;
    assert!(r.is_err(), "the timeout should have fired");
    assert_eq!(one(&c, 42).await.unwrap(), 42);
}
