//! The shared connection against a real PostgreSQL (see tests/docker/README.md).
//!
//! The address comes from `KAREZ_TEST_PG`. Without it every test prints `SKIPPED (not verified
//! against a real server)` and returns, unless `KAREZ_REQUIRE_SERVER=1`, which turns a skip into
//! a failure.

use karez_pg::{Config, Connection, Error as PgError, Value};
use karez_rt::{Error, Options, OwnedValue, SharedConnection};
use std::future::Future;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::{Duration, Instant};

type R = Result<(), Error>;

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
                return Ok(());
            }
        }
    };
}

fn cfg(addr: &str) -> Config {
    Config::from_url(&format!(
        "postgresql://karez_scram:karez_test_only_scram@{addr}/karez?sslmode=disable"
    ))
    .unwrap()
}

fn shared(addr: &str) -> Result<SharedConnection, Error> {
    SharedConnection::connect(&cfg(addr), Options::default())
}

fn shared_with(addr: &str, opts: Options) -> Result<SharedConnection, Error> {
    SharedConnection::connect(&cfg(addr), opts)
}

fn one_i64(c: &SharedConnection, sql: &str, params: &[Value]) -> Result<i64, Error> {
    let d = c.collect(sql, params)?;
    Ok(d.chunks[0].row(0).unwrap().get::<i64>(0).unwrap())
}

fn tag(name: &str) -> String {
    format!("krt_{}_{}", std::process::id(), name)
}

struct ThreadWaker(thread::Thread);
impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// A minimal executor: polls one future on this thread, parking until it is woken.
fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = Box::pin(f);
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        match f.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => thread::park_timeout(Duration::from_secs(60)),
        }
    }
}

#[test]
fn basics_exec_collect_and_statement_reuse() -> R {
    let addr = need_server!();
    let c = shared(&addr)?;
    let t = tag("basics");
    assert_eq!(
        c.exec(
            &format!("CREATE TEMP TABLE {t} (id int PRIMARY KEY, v text)"),
            &[]
        )?,
        0
    );
    for i in 0..50 {
        let n = c.exec(
            &format!("INSERT INTO {t} VALUES ($1, $2)"),
            &[Value::i32(i), Value::text(&format!("row {i}"))],
        )?;
        assert_eq!(n, 1);
    }
    let d = c.collect(&format!("SELECT id, v FROM {t} ORDER BY id"), &[])?;
    assert_eq!(d.chunks.iter().map(|c| c.rows()).sum::<usize>(), 50);
    let r = d.chunks[0].row(7).unwrap();
    assert_eq!(
        (r.get::<i32>(0).unwrap(), r.get::<&str>(1).unwrap()),
        (7, "row 7")
    );
    assert_eq!(one_i64(&c, "SELECT $1::int8 + 1", &[Value::i64(41)])?, 42);
    // Types that arrive rendered, and an enum found by catalogue lookup.
    let d = c.collect("SELECT n, m FROM unsupported_zoo", &[])?;
    let r = d.chunks[0].row(0).unwrap();
    assert_eq!(r.get::<&str>(0).unwrap(), "12345678901234567890.123456789");
    assert_eq!(r.get::<&str>(1).unwrap(), "happy");
    // A type without a decoder fails by name, and the connection carries on.
    match c.collect("SELECT mny FROM unsupported_zoo", &[]) {
        Err(Error::Pg(PgError::UnsupportedType { type_name, .. })) => {
            assert_eq!(type_name, "money")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(one_i64(&c, "SELECT 1::int8", &[])?, 1);
    c.close();
    Ok(())
}

#[test]
fn many_threads_share_one_session_and_one_backend() -> R {
    let addr = need_server!();
    let c = shared(&addr)?;
    let pid = one_i64(&c, "SELECT pg_backend_pid()::int8", &[])?;
    let mut hs = Vec::new();
    for t in 0..16i64 {
        let c = c.clone();
        hs.push(thread::spawn(move || -> Result<(i64, i64), Error> {
            let mut sum = 0;
            let mut pid_seen = 0;
            for i in 0..200i64 {
                sum += one_i64(&c, "SELECT $1::int8 * 2", &[Value::i64(t * 1000 + i)])?;
                pid_seen = one_i64(&c, "SELECT pg_backend_pid()::int8", &[])?;
            }
            Ok((sum, pid_seen))
        }));
    }
    for (t, h) in hs.into_iter().enumerate() {
        let (sum, p) = h.join().unwrap()?;
        let t = t as i64;
        assert_eq!(sum, (0..200).map(|i| (t * 1000 + i) * 2).sum::<i64>());
        assert_eq!(p, pid);
    }
    assert_eq!(c.in_flight(), 0);
    Ok(())
}

#[test]
fn queued_commands_are_pipelined_and_answered_in_order() -> R {
    let addr = need_server!();
    let c = shared(&addr)?;
    one_i64(&c, "SELECT $1::int8", &[Value::i64(0)])?; // prepare once
    let pendings: Vec<_> = (0..500i64)
        .map(|i| c.submit_collect("SELECT $1::int8", &[Value::i64(i)]))
        .collect::<Result<_, _>>()?;
    let got: Vec<i64> = pendings
        .into_iter()
        .map(|p| {
            p.wait()
                .map(|d| d.chunks[0].row(0).unwrap().get::<i64>(0).unwrap())
        })
        .collect::<Result<_, _>>()?;
    assert_eq!(got, (0..500).collect::<Vec<_>>());
    Ok(())
}

#[test]
fn a_stream_leases_the_connection_and_a_dropped_stream_frees_it() -> R {
    let addr = need_server!();
    let c = shared(&addr)?;
    let mut s = c.stream("SELECT id FROM audit_log", &[])?;
    // Another thread's command waits for the lease and then runs.
    let c2 = c.clone();
    let other = thread::spawn(move || one_i64(&c2, "SELECT 5::int8", &[]));
    thread::sleep(Duration::from_millis(100));
    let mut rows = 0usize;
    let mut chunks = 0usize;
    while let Some(ch) = s.next_chunk()? {
        rows += ch.rows();
        chunks += 1;
        if chunks == 3 {
            thread::sleep(Duration::from_millis(200)); // a slow consumer: the reader pauses
        }
    }
    assert_eq!(rows, 400_000);
    assert!(chunks >= 391);
    assert_eq!(other.join().unwrap()?, 5);
    // Dropping a stream early discards the rest and frees the connection.
    let mut s = c.stream("SELECT id FROM audit_log", &[])?;
    assert!(s.next_chunk()?.is_some());
    drop(s);
    assert_eq!(one_i64(&c, "SELECT 6::int8", &[])?, 6);
    Ok(())
}

#[test]
fn errors_do_not_disturb_other_commands_and_a_pipeline_is_atomic() -> R {
    let addr = need_server!();
    let c = shared(&addr)?;
    let t = tag("pipe");
    c.exec(&format!("CREATE TEMP TABLE {t} (id int PRIMARY KEY)"), &[])?;
    let ins = format!("INSERT INTO {t} VALUES ($1)");
    // Independent commands: one fails, its neighbours are untouched.
    let a = c.submit_exec(&ins, &[Value::i32(1)])?;
    let b = c.submit_exec(&ins, &[Value::i32(1)])?; // duplicate key
    let d = c.submit_exec(&ins, &[Value::i32(2)])?;
    assert_eq!(a.wait()?.rows, 1);
    assert_eq!(b.wait().unwrap_err().sqlstate(), Some("23505"));
    assert_eq!(d.wait()?.rows, 1);
    // A pipeline shares one Sync: the failure skips the rest and nothing takes effect.
    let res = c
        .pipeline()
        .exec(&ins, vec![OwnedValue::I32(10)])
        .exec(&ins, vec![OwnedValue::I32(1)]) // duplicate
        .exec(&ins, vec![OwnedValue::I32(11)])
        .run()?;
    assert_eq!(res.len(), 3);
    assert!(res[0].is_ok());
    assert_eq!(res[1].as_ref().unwrap_err().sqlstate(), Some("23505"));
    assert!(matches!(res[2], Err(Error::Skipped)));
    assert_eq!(
        one_i64(&c, &format!("SELECT count(*)::int8 FROM {t}"), &[])?,
        2
    );
    // A good pipeline commits all of it.
    let res = c
        .pipeline()
        .exec(&ins, vec![OwnedValue::I32(20)])
        .exec(&ins, vec![OwnedValue::I32(21)])
        .collect(&format!("SELECT count(*)::int8 FROM {t}"), vec![])
        .run()?;
    assert!(res.iter().all(|r| r.is_ok()));
    assert_eq!(
        res[2].as_ref().unwrap().chunks[0]
            .row(0)
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        4
    );
    // Errors the server reports: syntax, missing table.
    assert_eq!(
        c.exec("SELEKT 1", &[]).unwrap_err().sqlstate(),
        Some("42601")
    );
    assert_eq!(
        c.collect("SELECT * FROM no_such_table_krt", &[])
            .unwrap_err()
            .sqlstate(),
        Some("42P01")
    );
    assert_eq!(one_i64(&c, "SELECT 1::int8", &[])?, 1);
    Ok(())
}

#[test]
fn futures_work_on_a_hand_written_executor() -> R {
    let addr = need_server!();
    let c = shared(&addr)?;
    let sum = block_on(async {
        let pendings: Vec<_> = (0..100i64)
            .map(|i| c.submit_collect("SELECT $1::int8", &[Value::i64(i)]))
            .collect::<Result<_, _>>()?;
        let mut sum = 0i64;
        for p in pendings {
            let d = p.await?;
            sum += d.chunks[0].row(0).unwrap().get::<i64>(0).unwrap();
        }
        Ok::<_, Error>(sum)
    })?;
    assert_eq!(sum, (0..100).sum::<i64>());
    let rows = block_on(async {
        let mut s = c.stream("SELECT id FROM audit_log WHERE id <= 5000", &[])?;
        let mut n = 0;
        while let Some(ch) = s.next_chunk_async().await? {
            n += ch.rows();
        }
        Ok::<_, Error>(n)
    })?;
    assert_eq!(rows, 5000);
    Ok(())
}

#[test]
fn a_deadline_cancels_the_running_statement_and_the_connection_survives() -> R {
    let addr = need_server!();
    let c = shared(&addr)?;
    let t0 = Instant::now();
    let r = c
        .submit_collect("SELECT pg_sleep(30)", &[])?
        .wait_timeout(Duration::from_millis(300));
    assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
    // The server cancelled it; the next command still gets its own answer.
    assert_eq!(one_i64(&c, "SELECT 9::int8", &[])?, 9);
    assert!(t0.elapsed() < Duration::from_secs(20), "{:?}", t0.elapsed());
    Ok(())
}

#[test]
fn a_dead_connection_says_which_commands_may_have_run() -> R {
    let addr = need_server!();
    let c = shared(&addr)?;
    let pid = one_i64(&c, "SELECT pg_backend_pid()::int8", &[])?;
    let running = c.submit_collect("SELECT pg_sleep(30)", &[])?;
    thread::sleep(Duration::from_millis(200));
    let mut killer = Connection::connect(&cfg(&addr))?;
    {
        let mut cur = killer.query(
            "SELECT pg_terminate_backend($1::int)",
            &[Value::i32(pid as i32)],
        )?;
        while cur.next()?.is_some() {}
    }
    let e = running.wait().unwrap_err();
    assert!(!e.retry_safe(), "{e}");
    // From now on the connection is dead, and new commands were never sent.
    thread::sleep(Duration::from_millis(100));
    let e = c.exec("SELECT 1", &[]).unwrap_err();
    assert!(
        matches!(
            e,
            Error::Closed {
                retry_safe: true,
                ..
            }
        ),
        "{e}"
    );
    assert!(c.is_closed());
    Ok(())
}

#[test]
fn limits_close_and_queue_full_are_errors_not_hangs() -> R {
    let addr = need_server!();
    let opts = Options {
        max_queue: 2,
        wait_timeout: Duration::from_millis(300),
        ..Options::default()
    };
    let c = shared_with(&addr, opts)?;
    one_i64(&c, "SELECT 1::int8", &[])?;
    let a = c.submit_collect("SELECT pg_sleep(1)", &[])?;
    let b = c.submit_collect("SELECT pg_sleep(1)", &[])?;
    let e = c.submit_collect("SELECT 1", &[]).unwrap_err();
    assert!(matches!(e, Error::QueueFull) && e.retry_safe(), "{e}");
    a.wait()?;
    b.wait()?;
    assert_eq!(one_i64(&c, "SELECT 2::int8", &[])?, 2);
    // Close is graceful: what is in flight finishes, then new commands are refused untouched.
    let slow = c.submit_collect("SELECT pg_sleep(0.3)", &[])?;
    c.close();
    assert!(slow.wait().is_ok());
    let e = c.exec("SELECT 1", &[]).unwrap_err();
    assert!(
        matches!(
            e,
            Error::Closed {
                retry_safe: true,
                ..
            }
        ),
        "{e}"
    );
    Ok(())
}

#[test]
fn notifications_arrive_while_commands_are_in_flight() -> R {
    let addr = need_server!();
    let c = shared(&addr)?;
    let chan = tag("chan");
    c.exec(&format!("LISTEN {chan}"), &[])?;
    let mut other = Connection::connect(&cfg(&addr))?;
    other.execute("SELECT pg_notify($1, 'hi')", &[Value::text(&chan)])?;
    thread::sleep(Duration::from_millis(100));
    assert_eq!(one_i64(&c, "SELECT 1::int8", &[])?, 1);
    let n = c.take_notifications();
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(
        (n[0].channel.as_str(), n[0].payload.as_str()),
        (chan.as_str(), "hi")
    );
    Ok(())
}

#[test]
fn a_dropped_pending_does_not_desynchronise_the_stream() -> R {
    let addr = need_server!();
    let c = shared(&addr)?;
    for i in 0..200i64 {
        let p = c.submit_collect("SELECT $1::int8", &[Value::i64(i)])?;
        if i % 2 == 0 {
            drop(p); // abandoned: its answer is read and discarded
        } else {
            assert_eq!(
                p.wait()?.chunks[0].row(0).unwrap().get::<i64>(0).unwrap(),
                i
            );
        }
    }
    assert_eq!(one_i64(&c, "SELECT 77::int8", &[])?, 77);
    Ok(())
}

#[cfg(feature = "tls")]
mod tls {
    use super::*;
    use karez_pg::SslMode;

    fn tls_cfg(addr: &str) -> Config {
        let port = addr.rsplit(':').next().unwrap();
        let mut c = cfg(addr);
        c.ssl_mode = SslMode::VerifyFull;
        c.host = "localhost".into(); // the fixture certificate covers this name
        c.ssl_root_cert = Some(std::env::var("KAREZ_TEST_CA").unwrap_or_else(|_| {
            format!(
                "{}/../../tests/docker/certs/ca-{port}.crt",
                env!("CARGO_MANIFEST_DIR")
            )
        }));
        c
    }

    fn encrypted(c: &SharedConnection) -> Result<bool, Error> {
        let d = c.collect(
            "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
            &[],
        )?;
        Ok(d.chunks[0].row(0).unwrap().get::<bool>(0).unwrap())
    }

    #[test]
    fn a_shared_connection_over_tls_pipelines_streams_and_closes() -> R {
        let addr = need_server!();
        let c = SharedConnection::connect(&tls_cfg(&addr), Options::default())?;
        assert!(encrypted(&c)?);
        // 8 threads on the one encrypted session.
        let mut hs = Vec::new();
        for t in 0..8i64 {
            let c = c.clone();
            hs.push(thread::spawn(move || -> Result<i64, Error> {
                let mut sum = 0;
                for i in 0..100i64 {
                    sum += one_i64(&c, "SELECT $1::int8 * 2", &[Value::i64(t * 1000 + i)])?;
                }
                Ok(sum)
            }));
        }
        for (t, h) in hs.into_iter().enumerate() {
            let t = t as i64;
            assert_eq!(
                h.join().unwrap()?,
                (0..100).map(|i| (t * 1000 + i) * 2).sum::<i64>()
            );
        }
        // 500 queued commands answered in order.
        let pendings: Vec<_> = (0..500i64)
            .map(|i| c.submit_collect("SELECT $1::int8", &[Value::i64(i)]))
            .collect::<Result<_, _>>()?;
        let got: Vec<i64> = pendings
            .into_iter()
            .map(|p| {
                p.wait()
                    .map(|d| d.chunks[0].row(0).unwrap().get::<i64>(0).unwrap())
            })
            .collect::<Result<_, _>>()?;
        assert_eq!(got, (0..500).collect::<Vec<_>>());
        // A large parameter (300 KB x 2 bytes each way), and a stream of 400,000 rows.
        let big = "é日".repeat(150_000);
        let d = c.collect("SELECT $1::text", &[Value::text(&big)])?;
        assert_eq!(d.chunks[0].row(0).unwrap().get::<&str>(0).unwrap(), big);
        let mut s = c.stream("SELECT id FROM audit_log", &[])?;
        let mut rows = 0;
        while let Some(ch) = s.next_chunk()? {
            rows += ch.rows();
        }
        assert_eq!(rows, 400_000);
        // An atomic pipeline.
        let t = tag("tlspipe");
        c.exec(&format!("CREATE TEMP TABLE {t} (id int PRIMARY KEY)"), &[])?;
        let ins = format!("INSERT INTO {t} VALUES ($1)");
        let res = c
            .pipeline()
            .exec(&ins, vec![OwnedValue::I32(1)])
            .exec(&ins, vec![OwnedValue::I32(1)])
            .run()?;
        assert_eq!(res[1].as_ref().unwrap_err().sqlstate(), Some("23505"));
        c.close();
        let e = c.exec("SELECT 1", &[]).unwrap_err();
        assert!(
            matches!(
                e,
                Error::Closed {
                    retry_safe: true,
                    ..
                }
            ),
            "{e}"
        );
        Ok(())
    }

    #[test]
    fn a_killed_backend_over_tls_fails_commands_as_not_retry_safe() -> R {
        let addr = need_server!();
        let c = SharedConnection::connect(&tls_cfg(&addr), Options::default())?;
        let pid = one_i64(&c, "SELECT pg_backend_pid()::int8", &[])?;
        let running = c.submit_collect("SELECT pg_sleep(30)", &[])?;
        thread::sleep(Duration::from_millis(200));
        let mut killer = Connection::connect(&super::cfg(&addr))?;
        {
            let mut cur = killer.query(
                "SELECT pg_terminate_backend($1::int)",
                &[Value::i32(pid as i32)],
            )?;
            while cur.next()?.is_some() {}
        }
        let e = running.wait().unwrap_err();
        assert!(!e.retry_safe(), "{e}");
        Ok(())
    }
}
