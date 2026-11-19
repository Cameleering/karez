//! Server-side cursors (portals) against a real PostgreSQL: interleaved reading, errors, and the
//! rules about transactions. The address is `KAREZ_TEST_PG`; see tests/docker/README.md.

use karez_pg::{Connection, Error, Isolation, Portal, TxStatus, Value};
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

/// The first column of every row of a fetch, as i64.
fn ids(f: &karez_pg::Fetched) -> Vec<i64> {
    let mut v = Vec::new();
    for ch in &f.chunks {
        for r in 0..ch.rows() {
            v.push(ch.row(r).unwrap().get::<i64>(0).unwrap());
        }
    }
    v
}

#[test]
fn two_portals_and_other_statements_interleave_in_one_transaction() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("BEGIN", &[])?;
    let mut a = c.portal("SELECT g::int8 FROM generate_series(1, 1000) g", &[])?;
    let mut b = c.portal(
        "SELECT g::int8 * 10 FROM generate_series(1, $1::int4) g",
        &[Value::I32(250)],
    )?;
    assert_eq!(c.tx_status(), TxStatus::InTransaction);
    let (mut got_a, mut got_b) = (Vec::new(), Vec::new());
    let mut round = 0;
    while !a.is_done() || !b.is_done() {
        let fa = c.fetch(&mut a, 100)?;
        got_a.extend(ids(&fa));
        // another statement on the same connection, between fetches of the two portals
        assert_eq!(
            one_i64(&mut c, "SELECT $1::int8 + 1", &[Value::I64(round)])?,
            round + 1
        );
        let fb = c.fetch(&mut b, 60)?;
        got_b.extend(ids(&fb));
        // writes work too, and a cursor streamed to its end does not disturb the portals
        c.execute("CREATE TEMP TABLE IF NOT EXISTS pi (n int8)", &[])?;
        c.execute("INSERT INTO pi VALUES ($1)", &[Value::I64(round)])?;
        round += 1;
        assert!(round < 100, "the portals never finished");
    }
    assert_eq!(got_a, (1..=1000).collect::<Vec<i64>>());
    assert_eq!(got_b, (1..=250).map(|g| g * 10).collect::<Vec<i64>>());
    // the tag counts the rows of the last fetch only
    assert!(a.command_tag().is_some_and(|t| t.starts_with("SELECT ")));
    assert_eq!(a.rows_fetched(), 1000);
    assert_eq!(b.rows_fetched(), 250);
    assert_eq!(one_i64(&mut c, "SELECT count(*) FROM pi", &[])?, round);
    // a finished portal returns nothing, again and again
    let again = c.fetch(&mut a, 10)?;
    assert!(again.done && again.rows() == 0);
    c.execute("COMMIT", &[])?;
    assert_eq!(c.tx_status(), TxStatus::Idle);
    Ok(())
}

#[test]
fn a_large_table_is_read_in_fetches_and_adds_up() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let (n, sum) = {
        let mut cur = c.query("SELECT count(*), sum(id)::int8 FROM audit_log", &[])?;
        let r = cur.next()?.unwrap();
        (r.get::<i64>(0)?, r.get::<i64>(1)?)
    };
    let mut tx = c.begin(Isolation::ReadCommitted)?;
    let mut p = tx.portal("SELECT id::int8, action FROM audit_log ORDER BY id", &[])?;
    let (mut rows, mut total, mut fetches) = (0i64, 0i64, 0);
    while !p.is_done() {
        let f = tx.fetch(&mut p, 5_000)?;
        // 5,000 rows arrive as chunks of at most 1,024
        assert!(f.chunks.iter().all(|ch| ch.rows() <= 1024));
        for v in ids(&f) {
            rows += 1;
            total += v;
        }
        fetches += 1;
    }
    assert_eq!((rows, total), (n, sum));
    assert_eq!(
        fetches,
        (n as usize).div_ceil(5_000) + usize::from(n % 5_000 == 0)
    );
    tx.commit()?;
    Ok(())
}

#[test]
fn limits_zero_means_all_and_big_limits_make_several_chunks() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("BEGIN", &[])?;
    let mut p = c.portal("SELECT g::int8 FROM generate_series(1, 2500) g", &[])?;
    let f = c.fetch(&mut p, 0)?;
    assert!(f.done);
    assert_eq!(f.rows(), 2500);
    assert_eq!(f.chunks.len(), 3, "1,024 + 1,024 + 452");
    let mut q = c.portal("SELECT g::int8 FROM generate_series(1, 5) g", &[])?;
    let f = c.fetch(&mut q, 5)?; // exactly the number of rows: the server has not said "done" yet
    assert_eq!(f.rows(), 5);
    let f = c.fetch(&mut q, 5)?;
    assert!(f.done && f.rows() == 0);
    // an empty result
    let mut e = c.portal("SELECT 1::int8 WHERE false", &[])?;
    let f = c.fetch(&mut e, 10)?;
    assert!(f.done && f.rows() == 0 && f.chunks.is_empty());
    assert_eq!(e.command_tag(), Some("SELECT 0"));
    c.execute("ROLLBACK", &[])?;
    Ok(())
}

#[test]
fn native_types_and_nulls_come_through_a_portal() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("BEGIN", &[])?;
    let mut p = c.portal(
        "SELECT (g * 1.25)::numeric(10,2), (g || ' days')::interval, make_time(g % 24, 0, 0), \
         CASE WHEN g % 3 = 0 THEN NULL ELSE 'x' || g END FROM generate_series(1, 10) g",
        &[],
    )?;
    use karez_pg::{ColumnType, Decimal128, Interval, Time};
    assert_eq!(p.fields()[1].ty, ColumnType::Interval);
    let mut n = 0;
    while !p.is_done() {
        for ch in c.fetch(&mut p, 4)?.chunks {
            for r in 0..ch.rows() {
                n += 1;
                let row = ch.row(r)?;
                let g = i64::from(n);
                assert_eq!(row.get::<Decimal128>(0)?.unscaled, (g * 125) as i128);
                assert_eq!(row.get::<Interval>(1)?.days, n);
                assert_eq!(row.get::<Time>(2)?.0, (g % 24) * 3_600_000_000);
                assert_eq!(row.is_null(3)?, n % 3 == 0);
            }
        }
    }
    assert_eq!(n, 10);
    c.execute("ROLLBACK", &[])?;
    Ok(())
}

#[test]
fn a_portal_needs_a_transaction_and_ends_with_it() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    // outside a transaction: a clear usage error, and the connection is fine
    match c.portal("SELECT 1::int8", &[]) {
        Err(Error::Usage(m)) => assert!(m.contains("transaction"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(one_i64(&mut c, "SELECT 1::int8", &[])?, 1);
    // after COMMIT the server has dropped the portal: the next fetch is a server error
    c.execute("BEGIN", &[])?;
    let mut p = c.portal("SELECT g::int8 FROM generate_series(1, 100) g", &[])?;
    assert_eq!(c.fetch(&mut p, 10)?.rows(), 10);
    c.execute("COMMIT", &[])?;
    match c.fetch(&mut p, 10) {
        Err(e @ Error::Server(_)) => assert_eq!(e.sqlstate(), Some("34000")),
        other => panic!("{other:?}"),
    }
    assert!(p.is_done() && !c.is_broken());
    assert_eq!(one_i64(&mut c, "SELECT 2::int8", &[])?, 2);
    // the same after ROLLBACK
    c.execute("BEGIN", &[])?;
    let mut p = c.portal("SELECT g::int8 FROM generate_series(1, 100) g", &[])?;
    c.execute("ROLLBACK", &[])?;
    assert!(matches!(c.fetch(&mut p, 10), Err(Error::Server(_))));
    assert_eq!(one_i64(&mut c, "SELECT 3::int8", &[])?, 3);
    // closing frees it early; closing a finished portal or one from a dead transaction is fine
    c.execute("BEGIN", &[])?;
    let p = c.portal("SELECT g::int8 FROM generate_series(1, 100) g", &[])?;
    c.close_portal(p)?;
    let mut done = c.portal("SELECT 1::int8", &[])?;
    assert!(c.fetch(&mut done, 0)?.done);
    c.close_portal(done)?;
    c.execute("COMMIT", &[])?;
    Ok(())
}

#[test]
fn an_error_in_a_later_row_fails_the_fetch_and_the_transaction_cleanly() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("BEGIN", &[])?;
    // row 6 divides by zero: the first fetches work, the one that reaches it fails
    let mut p = c.portal(
        "SELECT (100 / (6 - g))::int8 FROM generate_series(1, 10) g",
        &[],
    )?;
    assert_eq!(c.fetch(&mut p, 3)?.rows(), 3);
    match c.fetch(&mut p, 5) {
        Err(e @ Error::Server(_)) => assert_eq!(e.sqlstate(), Some("22012")),
        other => panic!("{other:?}"),
    }
    assert!(p.is_done());
    assert_eq!(c.tx_status(), TxStatus::Failed);
    assert!(!c.is_broken());
    // a failed transaction accepts only ROLLBACK, as always
    assert!(c
        .query("SELECT 1::int8", &[])
        .and_then(|mut x| x.next().map(|_| ()))
        .is_err());
    c.execute("ROLLBACK", &[])?;
    assert_eq!(one_i64(&mut c, "SELECT 4::int8", &[])?, 4);
    Ok(())
}

#[test]
fn a_value_this_version_cannot_hold_fails_the_fetch_not_the_connection() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("BEGIN", &[])?;
    let mut p = c.portal(
        "SELECT x FROM (VALUES ('2000-01-01'::timestamp), ('294276-12-31 23:59:59'::timestamp)) t(x)",
        &[],
    )?;
    assert!(matches!(c.fetch(&mut p, 10), Err(Error::Value(_))));
    assert!(p.is_done() && !c.is_broken());
    c.execute("ROLLBACK", &[])?;
    assert_eq!(one_i64(&mut c, "SELECT 5::int8", &[])?, 5);
    Ok(())
}

#[test]
fn unsupported_types_are_refused_when_the_portal_opens() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("BEGIN", &[])?;
    match c.portal("SELECT mny FROM unsupported_zoo", &[]) {
        Err(Error::UnsupportedType { type_name, .. }) => assert_eq!(type_name, "money"),
        other => panic!("{other:?}"),
    }
    // the transaction is still usable
    assert_eq!(one_i64(&mut c, "SELECT 6::int8", &[])?, 6);
    c.execute("ROLLBACK", &[])?;
    Ok(())
}

#[test]
fn savepoints_and_portals_work_together() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("BEGIN", &[])?;
    let mut p = c.portal("SELECT g::int8 FROM generate_series(1, 30) g", &[])?;
    assert_eq!(c.fetch(&mut p, 10)?.rows(), 10);
    c.execute("SAVEPOINT s1", &[])?;
    assert_eq!(c.fetch(&mut p, 10)?.rows(), 10);
    // a failed statement rolled back to the savepoint leaves the portal alone
    assert!(c.execute("SELECT 1 / 0", &[]).is_err());
    c.execute("ROLLBACK TO SAVEPOINT s1", &[])?;
    let f = c.fetch(&mut p, 100)?;
    assert_eq!(f.rows(), 10);
    assert!(f.done);
    c.execute("COMMIT", &[])?;
    Ok(())
}

#[test]
fn many_portals_on_one_connection() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("BEGIN", &[])?;
    let mut ps: Vec<Portal> = (0..50)
        .map(|i| {
            c.portal(
                "SELECT ($1::int8 * 1000 + g) FROM generate_series(1, 20) g",
                &[Value::I64(i)],
            )
        })
        .collect::<Result<_, _>>()?;
    for round in 0..4 {
        for (i, p) in ps.iter_mut().enumerate() {
            let f = c.fetch(p, 5)?;
            let want: Vec<i64> = (1..=5).map(|k| i as i64 * 1000 + round * 5 + k).collect();
            assert_eq!(ids(&f), want, "portal {i} round {round}");
        }
    }
    assert!(ps.iter().all(|p| !p.is_done() || p.rows_fetched() == 20));
    c.execute("COMMIT", &[])?;
    Ok(())
}
