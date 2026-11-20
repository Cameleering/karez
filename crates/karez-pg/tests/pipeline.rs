//! Pipelines and batches against a real PostgreSQL. The address is `KAREZ_TEST_PG`
//! (tests/docker/README.md); without it each test prints `SKIPPED`, unless `KAREZ_REQUIRE_SERVER=1`.

use karez_pg::{oid, Connection, Error, Isolation, StepResult, TxStatus, Value};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

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

fn connect(addr: &str) -> Result<Connection, Error> {
    Connection::connect_url(&format!(
        "postgresql://karez_scram:karez_test_only_scram@{addr}/karez?sslmode=disable"
    ))
}

fn one_i64(c: &mut Connection, sql: &str, p: &[Value]) -> Result<i64, Error> {
    let mut cur = c.query(sql, p)?;
    let v = cur.next()?.expect("a row").get::<i64>(0)?;
    cur.close()?;
    Ok(v)
}

fn first_i64s(r: &StepResult) -> Vec<i64> {
    let mut v = Vec::new();
    for ch in &r.chunks {
        for i in 0..ch.rows() {
            v.push(ch.row(i).unwrap().get::<i64>(0).unwrap());
        }
    }
    v
}

#[test]
fn a_pipeline_returns_one_result_per_statement_in_order() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("CREATE TEMP TABLE pt (id int8 PRIMARY KEY, v text)", &[])?;
    let stmt = c.prepare("SELECT $1::int8 * 2", &[oid::INT8])?;
    let results = c
        .pipeline()
        .execute(
            "INSERT INTO pt VALUES ($1, $2)",
            &[Value::I64(1), Value::Text("a")],
        )
        .execute(
            "INSERT INTO pt VALUES ($1, $2), ($3, $4)",
            &[
                Value::I64(2),
                Value::Text("b"),
                Value::I64(3),
                Value::Text("c"),
            ],
        )
        .query("SELECT id FROM pt ORDER BY id", &[])
        .query_prepared(&stmt, &[Value::I64(21)])
        .execute(
            "UPDATE pt SET v = v || '!' WHERE id <= $1",
            &[Value::I64(2)],
        )
        .query("SELECT count(*)::int8 FROM pt WHERE v LIKE '%!'", &[])
        .run()?;
    assert_eq!(results.len(), 6);
    let r: Vec<_> = results.into_iter().map(|r| r.unwrap()).collect();
    assert_eq!(r[0].rows, 1);
    assert_eq!(r[1].rows, 2);
    assert_eq!(first_i64s(&r[2]), [1, 2, 3]);
    assert_eq!(r[2].rows, 3);
    assert_eq!(first_i64s(&r[3]), [42]);
    assert_eq!(r[4].rows, 2);
    assert!(r[4].chunks.is_empty(), "an execute keeps no rows");
    assert_eq!(first_i64s(&r[5]), [2]);
    // the connection is idle and usable, and the writes took effect (one implicit transaction)
    assert_eq!(one_i64(&mut c, "SELECT count(*) FROM pt", &[])?, 3);
    // an empty pipeline is nothing
    assert!(c.pipeline().run()?.is_empty());
    Ok(())
}

#[test]
fn a_failing_statement_fails_its_group_and_the_rest_are_skipped() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("CREATE TEMP TABLE pf (id int8 PRIMARY KEY)", &[])?;
    let results = c
        .pipeline()
        .execute("INSERT INTO pf VALUES ($1)", &[Value::I64(1)])
        .query("SELECT 1 / ($1::int8 - $1::int8)", &[Value::I64(5)])
        .execute("INSERT INTO pf VALUES ($1)", &[Value::I64(2)])
        .query("SELECT 1::int8", &[])
        .run()?;
    assert!(results[0].is_ok());
    match &results[1] {
        Err(e @ Error::Server(_)) => assert_eq!(e.sqlstate(), Some("22012")),
        other => panic!("{other:?}"),
    }
    assert!(matches!(results[2], Err(Error::Skipped)));
    assert!(matches!(results[3], Err(Error::Skipped)));
    // outside a transaction the group is one implicit transaction: the first insert is undone
    assert_eq!(one_i64(&mut c, "SELECT count(*) FROM pf", &[])?, 0);
    assert!(!c.is_broken());
    // inside a transaction block the transaction is left failed, as for any failed statement
    c.execute("BEGIN", &[])?;
    let results = c
        .pipeline()
        .execute("INSERT INTO pf VALUES ($1)", &[Value::I64(1)])
        .execute("INSERT INTO pf VALUES ($1)", &[Value::I64(1)]) // duplicate key
        .run()?;
    assert!(results[0].is_ok());
    match &results[1] {
        Err(e) => assert_eq!(e.sqlstate(), Some("23505")),
        other => panic!("{other:?}"),
    }
    assert_eq!(c.tx_status(), TxStatus::Failed);
    c.execute("ROLLBACK", &[])?;
    assert_eq!(one_i64(&mut c, "SELECT count(*) FROM pf", &[])?, 0);
    Ok(())
}

#[test]
fn nothing_runs_when_a_statement_cannot_be_prepared_or_its_parameters_are_wrong() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("CREATE TEMP TABLE pn (id int8)", &[])?;
    // a syntax error: the preparation fails before anything is sent
    match c
        .pipeline()
        .execute("INSERT INTO pn VALUES ($1)", &[Value::I64(1)])
        .execute("SELEKT 1", &[])
        .run()
    {
        Err(e @ Error::Server(_)) => assert_eq!(e.sqlstate(), Some("42601")),
        other => panic!("{other:?}"),
    }
    assert_eq!(one_i64(&mut c, "SELECT count(*) FROM pn", &[])?, 0);
    // a prepared statement given the wrong parameter type or number
    let stmt = c.prepare("INSERT INTO pn VALUES ($1)", &[oid::INT8])?;
    assert!(matches!(
        c.pipeline().execute_prepared(&stmt, &[Value::I32(1)]).run(),
        Err(Error::Usage(_))
    ));
    assert!(matches!(
        c.pipeline().execute_prepared(&stmt, &[]).run(),
        Err(Error::Usage(_))
    ));
    // a type without a decoder in a query, but not in an execute
    match c
        .pipeline()
        .query("SELECT mny FROM unsupported_zoo", &[])
        .run()
    {
        Err(Error::UnsupportedType { type_name, .. }) => assert_eq!(type_name, "money"),
        other => panic!("{other:?}"),
    }
    let ok = c
        .pipeline()
        .execute("SELECT mny FROM unsupported_zoo", &[])
        .run()?;
    assert!(ok[0].is_ok());
    assert_eq!(one_i64(&mut c, "SELECT 1::int8", &[])?, 1);
    Ok(())
}

#[test]
fn a_value_this_version_cannot_hold_fails_only_its_own_statement() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let results = c
        .pipeline()
        .query("SELECT 1::int8", &[])
        .query("SELECT '294276-12-31 23:59:59'::timestamp", &[])
        .query("SELECT 3::int8", &[])
        .run()?;
    assert_eq!(first_i64s(results[0].as_ref().unwrap()), [1]);
    assert!(matches!(results[1], Err(Error::Value(_))));
    assert_eq!(first_i64s(results[2].as_ref().unwrap()), [3]);
    assert!(!c.is_broken());
    Ok(())
}

#[test]
fn large_pipelines_do_not_stall_and_match_sequential_queries() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    // 120 statements, each carrying a 20 KB parameter (2.4 MB out) and returning 100 rows
    let big = "x".repeat(20_000);
    let sql = "SELECT g::int8 + length($1)::int8 FROM generate_series(1, 100) g";
    let mut p = c.pipeline();
    for _ in 0..120 {
        p = p.query(sql, &[Value::Text(&big)]);
    }
    let results = p.run()?;
    assert_eq!(results.len(), 120);
    for r in &results {
        let v = first_i64s(r.as_ref().unwrap());
        assert_eq!(v.len(), 100);
        assert_eq!((v[0], v[99]), (20_001, 20_100));
    }
    // results of 2,000 small point queries equal the same queries run one at a time
    let mut p = c.pipeline();
    for i in 0..2000i64 {
        p = p.query("SELECT $1::int8 * 3 + 1", &[Value::I64(i)]);
    }
    let piped: Vec<i64> = p
        .run()?
        .iter()
        .map(|r| first_i64s(r.as_ref().unwrap())[0])
        .collect();
    let mut seq = Vec::new();
    for i in 0..2000i64 {
        seq.push(one_i64(
            &mut c,
            "SELECT $1::int8 * 3 + 1",
            &[Value::I64(i)],
        )?);
    }
    assert_eq!(piped, seq);
    // a failure deep in a large group skips the rest and leaves the connection usable
    let mut p = c.pipeline();
    for i in 0..60 {
        let sql = if i == 40 {
            "SELECT 1 / (length($1)::int4 - length($1)::int4)"
        } else {
            "SELECT length($1)::int8"
        };
        p = p.query(sql, &[Value::Text(&big)]);
    }
    let r = p.run()?;
    assert!(r[..40].iter().all(|x| x.is_ok()));
    assert!(matches!(r[40], Err(Error::Server(_))));
    assert!(r[41..].iter().all(|x| matches!(x, Err(Error::Skipped))));
    assert_eq!(one_i64(&mut c, "SELECT 9::int8", &[])?, 9);
    Ok(())
}

#[test]
fn a_batch_inserts_updates_and_counts_rows() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute(
        "CREATE TEMP TABLE bt (id int8 PRIMARY KEY, n int4, s text)",
        &[],
    )?;
    let ins = c.prepare(
        "INSERT INTO bt VALUES ($1, $2, $3)",
        &[oid::INT8, oid::INT4, oid::TEXT],
    )?;
    let texts: Vec<String> = (0..25_000).map(|i| format!("row {i}")).collect();
    let rows: Vec<[Value; 3]> = (0..25_000i64)
        .map(|i| {
            [
                Value::I64(i),
                Value::I32((i % 100) as i32),
                Value::Text(&texts[i as usize]),
            ]
        })
        .collect();
    let out = c.batch(&ins).rows(rows.iter()).run()?;
    assert_eq!((out.statements, out.rows_affected), (25_000, 25_000));
    assert_eq!(one_i64(&mut c, "SELECT count(*) FROM bt", &[])?, 25_000);
    assert_eq!(
        one_i64(&mut c, "SELECT sum(n) FROM bt", &[])?,
        25_000 / 100 * (99 * 100 / 2)
    );
    // group size is a knob, not a semantic
    c.execute("TRUNCATE bt", &[])?;
    let out = c
        .batch(&ins)
        .rows(rows.iter().take(1234))
        .group_size(7)
        .run()?;
    assert_eq!(out.statements, 1234);
    assert_eq!(one_i64(&mut c, "SELECT count(*) FROM bt", &[])?, 1234);
    // an UPDATE batch sums each row's own count
    let upd = c.prepare(
        "UPDATE bt SET n = n + 1000 WHERE id % $1 = 0 AND id < $2",
        &[oid::INT8, oid::INT8],
    )?;
    let params = [
        [Value::I64(2), Value::I64(10)],
        [Value::I64(3), Value::I64(10)],
        [Value::I64(1_000_000), Value::I64(10)],
    ];
    let out = c.batch(&upd).rows(params.iter()).run()?;
    // id % 2 = 0 below 10: 0,2,4,6,8 (5 rows); id % 3 = 0 below 10: 0,3,6,9 (4 rows); only id 0 for the last
    assert_eq!((out.statements, out.rows_affected), (3, 5 + 4 + 1));
    // ON CONFLICT DO NOTHING reports 0 for a row that existed
    let upsert = c.prepare(
        "INSERT INTO bt VALUES ($1, 0, 'u') ON CONFLICT DO NOTHING",
        &[oid::INT8],
    )?;
    let ids = [[Value::I64(5)], [Value::I64(50_000)], [Value::I64(6)]];
    let out = c.batch(&upsert).rows(ids.iter()).run()?;
    assert_eq!((out.statements, out.rows_affected), (3, 1));
    // an empty input is fine
    let none: Vec<[Value; 3]> = Vec::new();
    let out = c.batch(&ins).rows(none).run()?;
    assert_eq!((out.statements, out.rows_affected), (0, 0));
    Ok(())
}

#[test]
fn a_batch_is_atomic_and_joins_a_transaction_when_there_is_one() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("CREATE TEMP TABLE ba (id int8 PRIMARY KEY)", &[])?;
    let ins = c.prepare("INSERT INTO ba VALUES ($1)", &[oid::INT8])?;
    // a duplicate in a late group: nothing is kept, and the connection is idle again
    let mut rows: Vec<[Value; 1]> = (0..50i64).map(|i| [Value::I64(i)]).collect();
    rows.push([Value::I64(7)]);
    let err = c
        .batch(&ins)
        .rows(rows.iter())
        .group_size(10)
        .run()
        .unwrap_err();
    assert_eq!(err.sqlstate(), Some("23505"));
    assert_eq!(c.tx_status(), TxStatus::Idle);
    assert_eq!(one_i64(&mut c, "SELECT count(*) FROM ba", &[])?, 0);
    // inside a transaction it joins it: the caller decides
    let mut tx = c.begin(Isolation::ReadCommitted)?;
    let ok: Vec<[Value; 1]> = (0..20i64).map(|i| [Value::I64(i)]).collect();
    let out = tx.batch(&ins).rows(ok.iter()).run()?;
    assert_eq!(out.rows_affected, 20);
    tx.rollback()?;
    assert_eq!(one_i64(&mut c, "SELECT count(*) FROM ba", &[])?, 0);
    // a row with the wrong parameters is a usage error before anything runs
    let bad: Vec<[Value; 2]> = vec![[Value::I64(1), Value::I64(2)]];
    assert!(matches!(
        c.batch(&ins).rows(bad.iter()).run(),
        Err(Error::Usage(_))
    ));
    assert_eq!(c.tx_status(), TxStatus::Idle);
    assert_eq!(one_i64(&mut c, "SELECT count(*) FROM ba", &[])?, 0);
    // a pipeline inside a transaction
    let mut tx = c.begin(Isolation::ReadCommitted)?;
    let r = tx
        .pipeline()
        .execute("INSERT INTO ba VALUES (100)", &[])
        .query("SELECT count(*)::int8 FROM ba", &[])
        .run()?;
    assert_eq!(first_i64s(r[1].as_ref().unwrap()), [1]);
    tx.commit()?;
    assert_eq!(one_i64(&mut c, "SELECT count(*) FROM ba", &[])?, 1);
    Ok(())
}

#[cfg(feature = "tls")]
mod tls {
    use super::*;
    use karez_pg::{Config, SslMode};

    #[test]
    fn large_pipelines_and_batches_over_tls_go_out_in_windows() -> R {
        let addr = need_server!();
        let port = addr.rsplit(':').next().unwrap().to_string();
        let ca = std::env::var("KAREZ_TEST_CA").unwrap_or_else(|_| {
            format!(
                "{}/../../tests/docker/certs/ca-{port}.crt",
                env!("CARGO_MANIFEST_DIR")
            )
        });
        let mut cfg = Config::from_url(&format!(
            "postgresql://karez_scram:karez_test_only_scram@{addr}/karez"
        ))?;
        cfg.host = "localhost".into();
        cfg.ssl_mode = SslMode::VerifyFull;
        cfg.ssl_root_cert = Some(ca);
        let mut c = Connection::connect(&cfg)?;
        let big = "x".repeat(20_000);
        let mut p = c.pipeline();
        for _ in 0..120 {
            p = p.query(
                "SELECT g::int8 + length($1)::int8 FROM generate_series(1, 100) g",
                &[Value::Text(&big)],
            );
        }
        let r = p.run()?;
        assert_eq!(r.len(), 120);
        for x in &r {
            assert_eq!(first_i64s(x.as_ref().unwrap()).len(), 100);
        }
        // a failure in a later window: the earlier ones are answered, the rest skipped, and the
        // connection stays usable
        let mut p = c.pipeline();
        for i in 0..60 {
            let sql = if i == 50 {
                "SELECT 1 / (length($1)::int4 - length($1)::int4)"
            } else {
                "SELECT length($1)::int8"
            };
            p = p.query(sql, &[Value::Text(&big)]);
        }
        let r = p.run()?;
        assert!(r[..50].iter().all(|x| x.is_ok()));
        assert!(matches!(r[50], Err(Error::Server(_))));
        assert!(r[51..].iter().all(|x| matches!(x, Err(Error::Skipped))));
        assert_eq!(one_i64(&mut c, "SELECT 1::int8", &[])?, 1);
        // a batch with large parameters
        c.execute("CREATE TEMP TABLE tb (id int8, s text)", &[])?;
        let ins = c.prepare("INSERT INTO tb VALUES ($1, $2)", &[oid::INT8, oid::TEXT])?;
        let rows: Vec<[Value; 2]> = (0..300i64)
            .map(|i| [Value::I64(i), Value::Text(&big)])
            .collect();
        let out = c.batch(&ins).rows(rows.iter()).group_size(100).run()?;
        assert_eq!(out.rows_affected, 300);
        assert_eq!(one_i64(&mut c, "SELECT count(*) FROM tb", &[])?, 300);
        Ok(())
    }
}
