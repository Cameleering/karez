//! Integration tests against a real PostgreSQL server (see tests/docker/README.md).
//!
//! The server address comes from `KAREZ_TEST_PG` (`host:port`, for example `127.0.0.1:54316`).
//! If it is unset or unreachable every test prints `SKIPPED (not verified against a real
//! server): <reason>` and returns, unless `KAREZ_REQUIRE_SERVER=1`, which turns a skip into a
//! failure. The documented run command sets it, so a verification run cannot pass by doing nothing.
//!
//! Credentials are the test-only ones from tests/docker/init/01_roles.sql.

#![allow(clippy::type_complexity)]

use karez_pg::{
    oid, Chunk, Config, Connection, Date, Error, Isolation, Timestamp, TxStatus, Value,
};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

const SCRAM_PW: &str = "karez_test_only_scram";
const CLEAR_PW: &str = "karez_test_only_clear";

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

fn url(addr: &str, user: &str, pw: Option<&str>, db: &str, extra: &str) -> String {
    let cred = match pw {
        Some(p) => format!("{user}:{p}"),
        None => user.to_string(),
    };
    format!("postgresql://{cred}@{addr}/{db}?sslmode=disable{extra}")
}

fn connect(addr: &str) -> Result<Connection, Error> {
    Connection::connect_url(&url(addr, "karez_scram", Some(SCRAM_PW), "karez", ""))
}

fn connect_with(addr: &str, extra: &str) -> Result<Connection, Error> {
    Connection::connect_url(&url(addr, "karez_scram", Some(SCRAM_PW), "karez", extra))
}

fn tag(name: &str) -> String {
    format!("karez_t_{}_{}", std::process::id(), name)
}

fn one_i64(conn: &mut Connection, sql: &str, params: &[Value]) -> Result<i64, Error> {
    let mut cur = conn.query(sql, params)?;
    let v = cur.next()?.expect("one row").get::<i64>(0)?;
    cur.close()?;
    Ok(v)
}

fn expect_sqlstate<T: std::fmt::Debug>(r: Result<T, Error>, want: &str) {
    match r {
        Err(e) => assert_eq!(e.sqlstate(), Some(want), "wrong error: {e}"),
        Ok(v) => panic!("expected SQLSTATE {want}, got Ok({v:?})"),
    }
}

// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn micros(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64, us: i64) -> i64 {
    (((days_from_civil(y, mo, d) * 24 + h) * 60 + mi) * 60 + s) * 1_000_000 + us
}

// ---- authentication ---------------------------------------------------------------------

#[test]
fn auth_scram_succeeds_and_reports_the_server() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    assert!(c
        .server_param("server_version")
        .is_some_and(|v| !v.is_empty()));
    assert_eq!(c.server_param("client_encoding"), Some("UTF8"));
    assert_eq!(one_i64(&mut c, "SELECT 40 + 2::int8", &[])?, 42);
    assert_eq!(c.tx_status(), TxStatus::Idle);
    Ok(())
}

#[test]
fn auth_cleartext_is_refused_unless_allowed() -> R {
    let addr = need_server!();
    let denied = Connection::connect_url(&url(&addr, "karez_clear", Some(CLEAR_PW), "karez", ""));
    match denied {
        Err(Error::Auth(m)) => assert!(m.contains("auth=password"), "{m}"),
        other => panic!("{other:?}"),
    }
    let mut c = Connection::connect_url(&url(
        &addr,
        "karez_clear",
        Some(CLEAR_PW),
        "karez",
        "&auth=password",
    ))?;
    assert_eq!(one_i64(&mut c, "SELECT 1::int8", &[])?, 1);
    // The cleartext user has read-only rights.
    expect_sqlstate(c.execute("DELETE FROM seed_done", &[]), "42501");
    Ok(())
}

#[test]
fn auth_trust_is_refused_unless_allowed() -> R {
    let addr = need_server!();
    let denied = Connection::connect_url(&url(&addr, "karez_trust", None, "karez", ""));
    match denied {
        Err(Error::Auth(m)) => assert!(m.contains("auth=trust"), "{m}"),
        other => panic!("{other:?}"),
    }
    let mut c = Connection::connect_url(&url(&addr, "karez_trust", None, "karez", "&auth=trust"))?;
    assert_eq!(one_i64(&mut c, "SELECT 1::int8", &[])?, 1);
    Ok(())
}

#[test]
fn wrong_password_is_28p01_and_does_not_leak_the_password() {
    let addr = match server() {
        Ok(a) => a,
        Err(why) => {
            eprintln!("SKIPPED (not verified against a real server): {why}");
            assert!(
                std::env::var("KAREZ_REQUIRE_SERVER").as_deref() != Ok("1"),
                "{why}"
            );
            return;
        }
    };
    let bad = "definitely-not-the-password";
    let e =
        Connection::connect_url(&url(&addr, "karez_scram", Some(bad), "karez", "")).unwrap_err();
    assert_eq!(e.sqlstate(), Some("28P01"), "{e}");
    assert!(!format!("{e} {e:?}").contains(bad));
}

#[test]
fn unknown_database_is_3d000() {
    let addr = match server() {
        Ok(a) => a,
        Err(why) => {
            eprintln!("SKIPPED (not verified against a real server): {why}");
            assert!(
                std::env::var("KAREZ_REQUIRE_SERVER").as_deref() != Ok("1"),
                "{why}"
            );
            return;
        }
    };
    let e = Connection::connect_url(&url(&addr, "karez_scram", Some(SCRAM_PW), "no_such_db", ""))
        .unwrap_err();
    assert_eq!(e.sqlstate(), Some("3D000"), "{e}");
}

#[test]
fn config_debug_hides_the_password() -> R {
    let cfg = Config::from_url(&url("h:1", "u", Some("s3cret-pw"), "d", ""))?;
    assert!(!format!("{cfg:?}").contains("s3cret-pw"));
    Ok(())
}

// ---- types -------------------------------------------------------------------------------

#[test]
fn integers_at_their_limits() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    for (id, lo) in [(1, true), (2, false)] {
        let mut cur = c.query(
            "SELECT i2, i4, i8 FROM ints_zoo WHERE id = $1",
            &[Value::i32(id)],
        )?;
        let r = cur.next()?.unwrap();
        if lo {
            assert_eq!(r.get::<i16>(0)?, i16::MIN);
            assert_eq!(r.get::<i32>(1)?, i32::MIN);
            assert_eq!(r.get::<i64>(2)?, i64::MIN);
        } else {
            assert_eq!(r.get::<i16>(0)?, i16::MAX);
            assert_eq!(r.get::<i32>(1)?, i32::MAX);
            assert_eq!(r.get::<i64>(2)?, i64::MAX);
        }
    }
    let mut cur = c.query("SELECT i2, i4, i8 FROM ints_zoo WHERE id = 4", &[])?;
    let r = cur.next()?.unwrap();
    assert_eq!(r.get::<Option<i16>>(0)?, None);
    assert_eq!(r.get::<Option<i32>>(1)?, None);
    assert_eq!(r.get::<Option<i64>>(2)?, None);
    Ok(())
}

#[test]
fn floats_nan_infinity_negative_zero_subnormal() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let mut get = |id: i32| -> Result<(Option<f32>, Option<f64>), Error> {
        let mut cur = c.query(
            "SELECT f4, f8 FROM floats_zoo WHERE id = $1",
            &[Value::i32(id)],
        )?;
        let r = cur.next()?.unwrap();
        Ok((r.get(0)?, r.get(1)?))
    };
    let (a, b) = get(1)?;
    assert!(a.unwrap().is_nan() && b.unwrap().is_nan());
    assert_eq!(get(2)?, (Some(f32::INFINITY), Some(f64::INFINITY)));
    assert_eq!(get(3)?, (Some(f32::NEG_INFINITY), Some(f64::NEG_INFINITY)));
    let (a, b) = get(4)?;
    assert!(a.unwrap() == 0.0 && a.unwrap().is_sign_negative());
    assert!(b.unwrap() == 0.0 && b.unwrap().is_sign_negative());
    let (a, b) = get(5)?;
    assert_eq!(a.unwrap().to_bits(), 1, "smallest float4 subnormal");
    assert_eq!(b.unwrap().to_bits(), 1, "smallest float8 subnormal");
    assert_eq!(get(6)?, (Some(f32::MAX), Some(f64::MAX)));
    assert_eq!(get(7)?, (Some(f32::MIN), Some(f64::MIN)));
    assert_eq!(get(8)?, (None, None));
    Ok(())
}

#[test]
fn text_edge_cases() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let mut get = |id: i32| -> Result<(Option<String>, Option<String>, Option<String>), Error> {
        let mut cur = c.query(
            "SELECT t, v, c FROM text_zoo WHERE id = $1",
            &[Value::i32(id)],
        )?;
        let r = cur.next()?.unwrap();
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    };
    assert_eq!(
        get(1)?,
        (
            Some(String::new()),
            Some(String::new()),
            Some(" ".repeat(8))
        )
    );
    let (t, v, ch) = get(2)?;
    assert_eq!(t.unwrap(), "x".repeat(1_048_576));
    assert_eq!(
        (v.unwrap().as_str(), ch.unwrap().as_str()),
        ("short", "abc     ")
    );
    let (t, v, ch) = get(3)?;
    assert_eq!(t.unwrap(), "héllo wörld, 日本語, Ελληνικά");
    assert_eq!(v.unwrap(), "ünï");
    assert_eq!(ch.unwrap(), "çé      ");
    let (t, v, _) = get(4)?;
    assert_eq!(t.unwrap(), "🎉🚀 emoji 👩‍👩‍👧‍👦");
    assert_eq!(v.unwrap(), "😀");
    let (t, v, _) = get(5)?;
    assert_eq!(t.unwrap(), "it's \"quoted\" \\ back\nnew\r\nline\ttab");
    assert_eq!(v.unwrap(), "a'b\"c\\d");
    assert_eq!(get(6)?, (None, None, None));
    Ok(())
}

#[test]
fn dates_and_timestamps_including_extremes() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let mut get = |id: i32| -> Result<(Option<Date>, Option<Timestamp>, Option<Timestamp>), Error> {
        let mut cur = c.query(
            "SELECT d, ts, tstz FROM time_zoo WHERE id = $1",
            &[Value::i32(id)],
        )?;
        let r = cur.next()?.unwrap();
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    };
    let day = |y, m, d| Date(days_from_civil(y, m, d) as i32);
    let (d, ts, tz) = get(1)?;
    assert_eq!(d, Some(day(1900, 1, 1)));
    assert_eq!(ts, Some(Timestamp(micros(1900, 1, 1, 0, 0, 0, 0))));
    assert_eq!(tz, ts);
    let (_, ts, tz) = get(2)?;
    assert_eq!(ts, Some(Timestamp(946_684_799_999_999)));
    assert_eq!(tz, ts);
    let (d, ts, _) = get(3)?;
    assert_eq!(d, Some(day(2000, 1, 1)));
    assert_eq!(ts, Some(Timestamp(946_684_800_000_000)));
    assert_eq!(
        get(4)?,
        (Some(Date(0)), Some(Timestamp(0)), Some(Timestamp(0)))
    );
    let (d, ts, _) = get(5)?;
    assert_eq!(d, Some(day(2100, 12, 31)));
    assert_eq!(
        ts,
        Some(Timestamp(micros(2100, 12, 31, 12, 34, 56, 789_012)))
    );
    // Minimum PostgreSQL values (4713 BC is year -4712).
    let (d, ts, tz) = get(6)?;
    assert_eq!(d, Some(day(-4712, 1, 1)));
    assert_eq!(ts, Some(Timestamp(micros(-4712, 1, 1, 0, 0, 0, 0))));
    assert_eq!(tz, ts);
    // Infinities.
    assert_eq!(
        get(7)?,
        (
            Some(Date(i32::MAX)),
            Some(Timestamp(i64::MAX)),
            Some(Timestamp(i64::MAX))
        )
    );
    assert_eq!(
        get(8)?,
        (
            Some(Date(i32::MIN)),
            Some(Timestamp(i64::MIN)),
            Some(Timestamp(i64::MIN))
        )
    );
    // Daylight-saving change in New York, 2024-03-10: the instants are what is stored.
    let (d, ts, tz) = get(9)?;
    assert_eq!(d, Some(day(2024, 3, 10)));
    assert_eq!(ts, Some(Timestamp(micros(2024, 3, 10, 1, 59, 59, 0))));
    assert_eq!(tz, Some(Timestamp(micros(2024, 3, 10, 6, 59, 59, 0))));
    let (_, ts, tz) = get(10)?;
    assert_eq!(ts, Some(Timestamp(micros(2024, 3, 10, 3, 0, 0, 0))));
    assert_eq!(tz, Some(Timestamp(micros(2024, 3, 10, 7, 0, 0, 0))));
    assert_eq!(tz.unwrap().0 - micros(2024, 3, 10, 6, 59, 59, 0), 1_000_000);
    assert_eq!(get(11)?, (None, None, None));
    // A large year that still fits.
    let (_, ts, tz) = get(97)?;
    assert_eq!(ts, Some(Timestamp(micros(294_000, 1, 1, 0, 0, 0, 0))));
    assert_eq!(tz, ts);
    // The last ~30 years of PostgreSQL's timestamp range do not fit Unix-epoch microseconds: a clear error, and
    // the connection survives it.
    match get(99) {
        Err(Error::Value(m)) => assert!(m.contains("out of range"), "{m}"),
        other => panic!("id 99: {other:?}"),
    }
    // PostgreSQL's largest date (5874897-12-31) still fits a 32-bit day count.
    assert_eq!(get(98)?.0, Some(day(5_874_897, 12, 31)));
    assert!(!c.is_broken());
    assert_eq!(one_i64(&mut c, "SELECT 7::int8", &[])?, 7);
    Ok(())
}

#[test]
fn bytea_from_zero_bytes_to_five_mib() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let mut get = |id: i32| -> Result<Option<Vec<u8>>, Error> {
        let mut cur = c.query("SELECT b FROM bytea_zoo WHERE id = $1", &[Value::i32(id)])?;
        let v = cur.next()?.unwrap().get::<Option<Vec<u8>>>(0)?;
        cur.close()?;
        Ok(v)
    };
    assert_eq!(get(1)?, Some(vec![]));
    assert_eq!(get(2)?, Some(vec![0]));
    let b3 = get(3)?.unwrap();
    assert_eq!(b3.len(), 1_048_575);
    assert!(b3.chunks(3).all(|c| c == [0x00, 0xff, 0x10]));
    let b4 = get(4)?.unwrap();
    assert_eq!(b4.len(), 5 * 1024 * 1024);
    assert!(b4.chunks(2).all(|c| c == [0x00, 0xff]));
    assert_eq!(get(5)?, None);
    Ok(())
}

#[test]
fn uuid_and_bool() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let mut cur = c.query("SELECT u, flag FROM misc_zoo ORDER BY id", &[])?;
    let mut rows = Vec::new();
    while let Some(r) = cur.next()? {
        rows.push((r.get::<Option<[u8; 16]>>(0)?, r.get::<Option<bool>>(1)?));
    }
    assert_eq!(rows[0], (Some([0; 16]), Some(true)));
    assert_eq!(rows[1], (Some([0xff; 16]), Some(false)));
    assert_eq!(
        rows[2],
        (
            Some([
                0x12, 0x3e, 0x45, 0x67, 0xe8, 0x9b, 0x12, 0xd3, 0xa4, 0x56, 0x42, 0x66, 0x14, 0x17,
                0x40, 0x00
            ]),
            None
        )
    );
    assert_eq!(rows[3], (None, None));
    Ok(())
}

#[test]
fn all_null_and_never_null_columns() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let mut cur = c.query(
        "SELECT id, always_null, never_null, half_null FROM null_zoo ORDER BY id",
        &[],
    )?;
    let (mut n, mut half) = (0, 0);
    while let Some(r) = cur.next()? {
        n += 1;
        assert!(r.is_null(1)?);
        assert_eq!(r.get::<i32>(2)?, r.get::<i32>(0)?);
        if !r.is_null(3)? {
            half += 1;
            assert_eq!(r.get::<String>(3)?, format!("v{}", r.get::<i32>(0)?));
        }
    }
    assert_eq!((n, half), (5000, 2500));
    drop(cur);
    // A column that is NULL in every row of a chunk still decodes as NULL (customers.legacy_code).
    let mut cur = c.query("SELECT legacy_code FROM customers", &[])?;
    let mut chunks = 0;
    while let Some(ch) = cur.next_chunk()? {
        chunks += 1;
        let col = ch.column(0)?;
        assert_eq!(col.null_count(), ch.rows());
    }
    assert!(chunks >= 19, "{chunks}");
    Ok(())
}

#[test]
fn unsupported_types_fail_with_the_type_name() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    for (col, name) in [
        ("mny", "money"),
        ("pt", "point"),
        ("rng", "int4range"),
        ("tsv", "tsvector"),
        ("ma", "_mood"),
    ] {
        let sql = format!("SELECT {col} FROM unsupported_zoo WHERE id = 1");
        match c.query(&sql, &[]) {
            Err(Error::UnsupportedType {
                column, type_name, ..
            }) => {
                assert_eq!(column, col);
                assert_eq!(type_name, name);
            }
            Err(e) => panic!("{col}: {e}"),
            Ok(_) => panic!("{col}: expected an error"),
        }
    }
    // The error is specific and not fatal.
    let e = c
        .query("SELECT mny FROM unsupported_zoo", &[])
        .err()
        .unwrap();
    assert!(e.to_string().contains("money") && !e.is_fatal());
    assert!(!c.is_broken());
    assert_eq!(one_i64(&mut c, "SELECT 1::int8", &[])?, 1);
    // A cast makes the column readable.
    let mut cur = c.query("SELECT mny::numeric::text FROM unsupported_zoo", &[])?;
    assert_eq!(cur.next()?.unwrap().get::<&str>(0)?, "12.34");
    drop(cur);
    Ok(())
}

#[test]
fn unsupported_types_can_be_read_as_text_when_asked() -> R {
    let addr = need_server!();
    let mut c = connect_with(&addr, "&unsupported_types=text")?;
    let mut cur = c.query("SELECT mny, pt, rng, tsv, ma FROM unsupported_zoo", &[])?;
    let r = cur.next()?.unwrap();
    assert!(r.get::<&str>(0)?.contains("12.34"));
    assert_eq!(r.get::<&str>(1)?, "(1,2)");
    assert_eq!(r.get::<&str>(2)?, "[1,10)");
    assert_eq!(r.get::<&str>(3)?, "'a':1 'b':2");
    assert_eq!(r.get::<&str>(4)?, "{sad,happy}");
    Ok(())
}

/// Run `sql` (two text columns: the rendered value and the server's `::text` of it) and require
/// them to be equal in every row. Returns the number of rows.
fn same_as_server(c: &mut Connection, sql: &str) -> Result<usize, Error> {
    let mut cur = c.query(sql, &[])?;
    let mut n = 0;
    while let Some(r) = cur.next()? {
        let (a, b) = (r.get::<Option<&str>>(0)?, r.get::<Option<&str>>(1)?);
        assert_eq!(a, b, "{sql}: row {n}");
        n += 1;
    }
    Ok(n)
}

#[test]
fn rendered_types_match_the_servers_own_text() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("SET intervalstyle = 'iso_8601'", &[])?;
    let cases = [
        "SELECT v, v::text FROM (VALUES (0::numeric),(1),(-1),(0.5),(-0.0001),(0.0000000001),(10000),\
         (99999999),(1e100),(-123456789012345678901234567890.1234567890123456789),('NaN'),\
         ('Infinity'),('-Infinity'),(12345678901234567890.123456789),(5.10),(0.000),(NULL)) t(v)",
        "SELECT v, v::text FROM (SELECT round((i * 7919)::numeric * 123456789.123456789 \
         / (10::numeric ^ (i % 25)), i % 15) * (CASE WHEN i % 2 = 0 THEN -1 ELSE 1 END) AS v \
         FROM generate_series(1, 2000) i) t",
        "SELECT v, v::text FROM (VALUES ('0 microseconds'::interval),('1 microsecond'),\
         ('-1 microsecond'),('100 hours'),('1 year'),('13 months'),('-1 day'),\
         ('2147483647 months'),('1 year 2 mons 3 days 04:05:06.789'),\
         ('-1 year -2 mons +3 days -04:05:06.5'),('00:00:00.5'),('-00:00:00.5')) t(v)",
        "SELECT v, v::text FROM (VALUES ('00:00:00'::time),('00:00:00.000001'),('12:34:56.789'),\
         ('23:59:59.999999'),('24:00:00')) t(v)",
        "SELECT v, v::text FROM (VALUES ('00:00:00+00'::timetz),('12:00:00+05:30'),('12:00:00-08'),\
         ('23:59:59.5-12:34:56'),('01:02:03+14:30:15')) t(v)",
        // inet::text always adds the netmask; abbrev() is inet's own output form.
        "SELECT v, abbrev(v) FROM (VALUES ('0.0.0.0/0'::inet),('255.255.255.255'),('::'),('::1'),\
         ('::ffff:1.2.3.4'),('::1.2.3.4'),('2001:db8::1/64'),('2001:db8:0:0:1:0:0:1'),\
         ('1:0:0:2:0:0:0:3'),('fe80::1'),('ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff'),\
         ('1:2:3:4:5:6:7:8'),('::ffff:0:0'),('192.168.1.0/24')) t(v)",
        "SELECT v, v::text FROM (VALUES ('10.0.0.0/8'::cidr),('::/0'),('2001:db8::/32'),\
         ('192.168.1.0/32')) t(v)",
        "SELECT v, v::text FROM (VALUES ('08:00:2b:01:02:03'::macaddr),('ff:ff:ff:ff:ff:ff'),\
         ('00:00:00:00:00:00')) t(v)",
        "SELECT v, v::text FROM (VALUES ('08:00:2b:01:02:03:04:05'::macaddr8)) t(v)",
        "SELECT v, v::text FROM (VALUES (B'10110'::bit(5)),(B'00000'),(B'11111')) t(v)",
        "SELECT v, v::text FROM (VALUES (B''::varbit),(B'1'),(B'1010101010101'),\
         (repeat('10', 20)::varbit)) t(v)",
        "SELECT v, v::text FROM (VALUES ('{\"a\": [1, 2, {\"b\": null}]}'::json),('[]'),\
         ('\"é日🎉\"')) t(v)",
        "SELECT v, v::text FROM (VALUES ('{\"a\": [1, 2, {\"b\": null}], \"b\": 1e500}'::jsonb),\
         ('[]'),('\"é日🎉\"'),('null'),('1.50')) t(v)",
        "SELECT v, v::text FROM (VALUES ('<a>é</a>'::xml)) t(v)",
        "SELECT m, m::text FROM unsupported_zoo",
        "SELECT v, v::text FROM (VALUES ('{1,2,NULL,4}'::int4[]),('{}'),('{{1,2},{3,4}}'),\
         ('[0:2]={5,6,7}'),('{{{1,2},{3,4}},{{5,6},{7,8}}}'),(NULL)) t(v)",
        "SELECT v, v::text FROM (VALUES (ARRAY['a b','','NULL','q\"uote','back\\slash','{x}','é',\
         'x,y',E'tab\\t','null','plain'])) t(v)",
        "SELECT v, v::text FROM (VALUES (ARRAY[1.5, NULL, -0.001, 1e30]::numeric[])) t(v)",
        "SELECT v, v::text FROM (VALUES (ARRAY['00000000-0000-0000-0000-000000000000'::uuid,\
         'ffffffff-ffff-ffff-ffff-ffffffffffff'])) t(v)",
        "SELECT v, v::text FROM (VALUES (ARRAY['\\x00ff'::bytea, '\\x'::bytea, '\\x5c'::bytea])) t(v)",
        "SELECT v, v::text FROM (VALUES ('{t,f,NULL}'::bool[])) t(v)",
        "SELECT v, v::text FROM (VALUES (ARRAY[-9223372036854775808, 9223372036854775807]::int8[]),\
         (ARRAY[-32768, 32767]::int2[])) t(v)",
        "SELECT v, v::text FROM (VALUES (ARRAY['{\"k\":1}'::jsonb, '[1,2]'])) t(v)",
        "SELECT v, v::text FROM (VALUES (ARRAY['::1'::inet,'10.0.0.1'])) t(v)",
        "SELECT v, v::text FROM (VALUES (ARRAY['1 day'::interval, '-1 hour'])) t(v)",
        "SELECT v, v::text FROM (VALUES (ARRAY['01:02:03'::time, '24:00:00'])) t(v)",
        "SELECT v, v::text FROM (VALUES ('{a,b}'::varchar[]),('{\"x y\",NULL}')) t(v)",
        "SELECT v, v::text FROM (SELECT array_agg(i) v FROM generate_series(1, 100000) i) t",
    ];
    for sql in cases {
        assert!(same_as_server(&mut c, sql)? > 0, "{sql}");
    }
    // Floating-point arrays use Rust's shortest form, so check them against known values.
    let mut cur = c.query(
        "SELECT '{1.5,NULL,-2.25,NaN,Infinity}'::float8[], ARRAY[0.1]::float4[]",
        &[],
    )?;
    let r = cur.next()?.unwrap();
    assert_eq!(r.get::<&str>(0)?, "{1.5,NULL,-2.25,NaN,Infinity}");
    assert_eq!(r.get::<&str>(1)?, "{0.1}");
    drop(cur);
    // oid is an integer column; "char" is one character.
    let mut cur = c.query("SELECT 4294967295::oid, 'z'::\"char\", 0::oid", &[])?;
    let r = cur.next()?.unwrap();
    assert_eq!(r.get::<i64>(0)?, 4_294_967_295);
    assert_eq!(r.get::<&str>(1)?, "z");
    assert_eq!(r.get::<i64>(2)?, 0);
    drop(cur);
    // The values the seed holds, by name.
    let mut cur = c.query("SELECT n, j, arr, iv, m, ip FROM unsupported_zoo", &[])?;
    let r = cur.next()?.unwrap();
    assert_eq!(r.get::<&str>(0)?, "12345678901234567890.123456789");
    assert_eq!(r.get::<&str>(1)?, "{\"a\": [1, 2, {\"b\": null}]}");
    assert_eq!(r.get::<&str>(2)?, "{1,2,NULL,4}");
    assert_eq!(r.get::<&str>(3)?, "P1Y2M3DT4H5M6.789S");
    assert_eq!(r.get::<&str>(4)?, "happy");
    assert_eq!(r.get::<&str>(5)?, "192.168.1.0/24");
    Ok(())
}

#[test]
fn parameters_round_trip_through_the_server() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let long = "é日🎉".repeat(100_000);
    let big = vec![0xA5u8; 3 * 1024 * 1024];
    let mut cur = c.query(
        "SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12",
        &[
            Value::i16(i16::MIN),
            Value::i32(i32::MAX),
            Value::i64(i64::MIN),
            Value::f32(f32::from_bits(1)),
            Value::f64(-0.0),
            Value::bool(true),
            Value::text(&long),
            Value::bytes(&big),
            Value::Uuid([7; 16]),
            Value::Date(Date(days_from_civil(2100, 12, 31) as i32)),
            Value::Timestamp(Timestamp(micros(1900, 1, 1, 0, 0, 0, 1))),
            Value::TimestampTz(Timestamp(i64::MIN)),
        ],
    )?;
    let r = cur.next()?.unwrap();
    assert_eq!(r.get::<i16>(0)?, i16::MIN);
    assert_eq!(r.get::<i32>(1)?, i32::MAX);
    assert_eq!(r.get::<i64>(2)?, i64::MIN);
    assert_eq!(r.get::<f32>(3)?.to_bits(), 1);
    assert!(r.get::<f64>(4)?.is_sign_negative());
    assert!(r.get::<bool>(5)?);
    assert_eq!(r.get::<&str>(6)?, long);
    assert_eq!(r.get::<&[u8]>(7)?, &big[..]);
    assert_eq!(r.get::<[u8; 16]>(8)?, [7; 16]);
    assert_eq!(
        r.get::<Date>(9)?,
        Date(days_from_civil(2100, 12, 31) as i32)
    );
    assert_eq!(
        r.get::<Timestamp>(10)?,
        Timestamp(micros(1900, 1, 1, 0, 0, 0, 1))
    );
    assert_eq!(r.get::<Timestamp>(11)?, Timestamp(i64::MIN));
    drop(cur);
    // NULL parameters, and a text-format value whose type the server infers.
    let mut cur = c.query(
        "SELECT $1::int4, $2::text, $3::numeric::text",
        &[Value::Null, Value::Null, Value::Unknown("-12.50")],
    )?;
    let r = cur.next()?.unwrap();
    assert_eq!(r.get::<Option<i32>>(0)?, None);
    assert_eq!(r.get::<Option<&str>>(1)?, None);
    assert_eq!(r.get::<&str>(2)?, "-12.50");
    Ok(())
}

#[test]
fn quotes_and_sql_in_parameters_stay_data() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let evil = "x'); DROP TABLE customers; --";
    assert_eq!(
        one_i64(
            &mut c,
            "SELECT count(*) FROM customers WHERE name = $1",
            &[Value::text(evil)]
        )?,
        0
    );
    assert_eq!(
        one_i64(&mut c, "SELECT count(*) FROM customers", &[])?,
        20000
    );
    Ok(())
}

// ---- reading large results -----------------------------------------------------------------

#[test]
fn bulk_read_of_a_large_table_chunks_and_rows_agree() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let (n, sum_id, with_details) = {
        let mut cur = c.query(
            "SELECT count(*), sum(id)::int8, count(details) FROM audit_log",
            &[],
        )?;
        let r = cur.next()?.unwrap();
        (r.get::<i64>(0)?, r.get::<i64>(1)?, r.get::<i64>(2)?)
    };
    assert_eq!(n, 400_000);

    let sql = "SELECT id, at, actor, action, details FROM audit_log ORDER BY id";
    let mut chunks = 0;
    let (mut rows, mut sum, mut nonnull, mut bytes) = (0i64, 0i64, 0i64, 0usize);
    {
        let mut cur = c.query(sql, &[])?;
        while let Some(ch) = cur.next_chunk()? {
            chunks += 1;
            assert!(ch.rows() <= 1024);
            let ids = ch.column(0)?;
            let det = ch.column(4)?;
            for i in 0..ch.rows() {
                sum += ids.get::<i64>(i)?;
                if let Some(d) = det.get::<Option<&str>>(i)? {
                    nonnull += 1;
                    bytes += d.len();
                }
            }
            rows += ch.rows() as i64;
        }
        assert_eq!(cur.command_tag(), Some("SELECT 400000"));
    }
    assert!(
        chunks >= 391,
        "400000 rows need at least 391 chunks of 1024, got {chunks}"
    );
    assert_eq!((rows, sum, nonnull), (n, sum_id, with_details));

    let (mut rrows, mut rsum, mut rnonnull, mut rbytes) = (0i64, 0i64, 0i64, 0usize);
    let mut cur = c.query(sql, &[])?;
    while let Some(r) = cur.next()? {
        rrows += 1;
        rsum += r.get::<i64>(0)?;
        if let Some(d) = r.get::<Option<&str>>(4)? {
            rnonnull += 1;
            rbytes += d.len();
        }
    }
    assert_eq!((rrows, rsum, rnonnull, rbytes), (rows, sum, nonnull, bytes));
    Ok(())
}

#[test]
fn six_table_report_returns_tens_of_thousands_of_rows() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let report = "SELECT o.id, o.order_no, c.name, a.city, p.sku, cat.name AS category, \
                         oi.qty, oi.unit_price_cents, oi.discount_pct, pay.method, pay.paid_at \
                    FROM orders o \
                    JOIN customers c ON c.id = o.customer_id \
                    JOIN addresses a ON a.id = o.ship_address_id \
                    JOIN order_items oi ON oi.order_id = o.id \
                    JOIN products p ON p.id = oi.product_id \
                    JOIN categories cat ON cat.id = p.category_id \
                    LEFT JOIN payments pay ON pay.order_id = o.id \
                   WHERE o.id <= $1 ORDER BY o.id, oi.line_no";
    let expected = one_i64(
        &mut c,
        "SELECT count(*) FROM order_items WHERE order_id <= $1",
        &[Value::i64(20_000)],
    )?;
    assert!(expected > 40_000, "{expected}");
    let mut cur = c.query(report, &[Value::i64(20_000)])?;
    let mut n = 0i64;
    let mut qty_sum = 0i64;
    let mut discount_nulls = 0i64;
    let mut chunks = 0;
    while let Some(ch) = cur.next_chunk()? {
        chunks += 1;
        for i in 0..ch.rows() {
            let r = ch.row(i)?;
            qty_sum += i64::from(r.get::<i16>(6)?);
            discount_nulls += i64::from(r.is_null(8)?);
            assert!(r.get::<&str>(1)?.starts_with("ORD-"));
            let _ = r.get::<Option<Timestamp>>(10)?;
        }
        n += ch.rows() as i64;
    }
    assert_eq!(n, expected);
    assert!(chunks >= 40);
    drop(cur);
    let qty_server = one_i64(
        &mut c,
        "SELECT sum(qty)::int8 FROM order_items WHERE order_id <= $1",
        &[Value::i64(20_000)],
    )?;
    assert_eq!(qty_sum, qty_server);
    assert!(discount_nulls > 0 && discount_nulls < n);
    Ok(())
}

#[test]
fn view_and_sequence() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let mut cur = c.query(
        "SELECT order_id, order_no, customer_name, item_count, items_cents, total_cents \
           FROM v_order_summary WHERE order_id = $1",
        &[Value::i64(77)],
    )?;
    let r = cur.next()?.unwrap();
    assert_eq!(r.get::<i64>(0)?, 77);
    assert!(r.get::<&str>(1)?.starts_with("ORD-"));
    assert_eq!(
        r.get::<i64>(4)?,
        r.get::<i64>(5)?,
        "items total equals order total"
    );
    drop(cur);
    let a = one_i64(&mut c, "SELECT nextval('order_no_seq')", &[])?;
    let b = one_i64(&mut c, "SELECT nextval('order_no_seq')", &[])?;
    assert_eq!(b, a + 1);
    Ok(())
}

#[test]
fn parameterised_search_with_many_and_null_parameters() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let sql = "SELECT count(*) FROM products WHERE \
        ($1::text IS NULL OR name LIKE $1) AND ($2::int8 IS NULL OR price_cents >= $2) \
        AND ($3::int8 IS NULL OR price_cents <= $3) AND ($4::int4 IS NULL OR category_id = $4) \
        AND ($5::float8 IS NULL OR rating >= $5) \
        AND ($6::bool IS NULL OR (discontinued_on IS NOT NULL) = $6) \
        AND ($7::date IS NULL OR created_at::date >= $7) \
        AND ($8::text IS NULL OR description LIKE $8) \
        AND ($9::int4 IS NULL OR id % 2 = $9) AND ($10::text IS NULL OR sku > $10)";
    let all_null = [Value::Null; 10];
    assert_eq!(one_i64(&mut c, sql, &all_null)?, 5000);
    let mut some = [Value::Null; 10];
    some[1] = Value::i64(50_000);
    some[3] = Value::i32(7);
    let want = one_i64(
        &mut c,
        "SELECT count(*) FROM products WHERE price_cents >= 50000 AND category_id = 7",
        &[],
    )?;
    assert!(want > 0);
    assert_eq!(one_i64(&mut c, sql, &some)?, want);
    let all = [
        Value::text("Product 1%"),
        Value::i64(1000),
        Value::i64(90_000),
        Value::i32(1),
        Value::f64(0.5),
        Value::bool(false),
        Value::Date(Date(days_from_civil(2021, 1, 2) as i32)),
        Value::text("%description%"),
        Value::i32(1),
        Value::text("SKU-000000"),
    ];
    let want = one_i64(
        &mut c,
        "SELECT count(*) FROM products WHERE name LIKE 'Product 1%' AND price_cents BETWEEN 1000 AND 90000 \
         AND category_id = 1 AND rating >= 0.5 AND discontinued_on IS NULL \
         AND created_at::date >= date '2021-01-02' AND description LIKE '%description%' \
         AND id % 2 = 1 AND sku > 'SKU-000000'",
        &[],
    )?;
    assert_eq!(one_i64(&mut c, sql, &all)?, want);
    assert!(c.cached_statements() >= 3);
    Ok(())
}

// ---- transactions ----------------------------------------------------------------------------

fn place_order(
    tx: &mut karez_pg::Transaction<'_>,
    note: &str,
    product: i64,
    qty: i16,
) -> Result<i64, Error> {
    let id = {
        let mut cur = tx.query(
            "INSERT INTO orders (customer_id, ship_address_id, note) VALUES (1, 20000, $1) RETURNING id",
            &[Value::text(note)],
        )?;
        let id = cur.next()?.unwrap().get::<i64>(0)?;
        cur.close()?;
        id
    };
    tx.execute(
        "INSERT INTO order_items (order_id, line_no, product_id, qty, unit_price_cents) \
         SELECT $1, 1, id, $3, price_cents FROM products WHERE id = $2",
        &[Value::i64(id), Value::i64(product), Value::i16(qty)],
    )?;
    tx.execute(
        "UPDATE inventory SET on_hand = on_hand - $2 WHERE product_id = $1",
        &[Value::i64(product), Value::i32(i32::from(qty))],
    )?;
    tx.execute(
        "INSERT INTO inventory_movements (product_id, delta, reason, order_id) VALUES ($1, $2, 'sale', $3)",
        &[Value::i64(product), Value::i32(-i32::from(qty)), Value::i64(id)],
    )?;
    tx.execute(
        "UPDATE orders SET total_cents = (SELECT sum(qty * unit_price_cents) FROM order_items WHERE order_id = $1) WHERE id = $1",
        &[Value::i64(id)],
    )?;
    Ok(id)
}

fn cleanup_orders(c: &mut Connection, note: &str) -> R {
    c.execute(
        "DELETE FROM inventory_movements WHERE order_id IN (SELECT id FROM orders WHERE note = $1)",
        &[Value::text(note)],
    )?;
    c.execute(
        "DELETE FROM order_items WHERE order_id IN (SELECT id FROM orders WHERE note = $1)",
        &[Value::text(note)],
    )?;
    c.execute("DELETE FROM orders WHERE note = $1", &[Value::text(note)])?;
    Ok(())
}

#[test]
fn order_placement_commits() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let note = tag("commit");
    cleanup_orders(&mut c, &note)?;
    let product = 4242;
    let before = one_i64(
        &mut c,
        "SELECT on_hand::int8 FROM inventory WHERE product_id = $1",
        &[Value::i64(product)],
    )?;
    let price = one_i64(
        &mut c,
        "SELECT price_cents FROM products WHERE id = $1",
        &[Value::i64(product)],
    )?;
    let mut tx = c.begin(Isolation::Default)?;
    assert_eq!(tx.status(), TxStatus::InTransaction);
    let id = place_order(&mut tx, &note, product, 3)?;
    tx.commit()?;
    assert_eq!(c.tx_status(), TxStatus::Idle);
    assert_eq!(
        one_i64(
            &mut c,
            "SELECT total_cents FROM orders WHERE id = $1",
            &[Value::i64(id)]
        )?,
        3 * price
    );
    assert_eq!(
        one_i64(
            &mut c,
            "SELECT on_hand::int8 FROM inventory WHERE product_id = $1",
            &[Value::i64(product)]
        )?,
        before - 3
    );
    cleanup_orders(&mut c, &note)?;
    c.execute(
        "UPDATE inventory SET on_hand = $2 WHERE product_id = $1",
        &[Value::i64(product), Value::i32(before as i32)],
    )?;
    Ok(())
}

#[test]
fn order_placement_with_failure_rolls_back_everything() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let note = tag("rollback");
    cleanup_orders(&mut c, &note)?;
    let product = 4243;
    let before = one_i64(
        &mut c,
        "SELECT on_hand::int8 FROM inventory WHERE product_id = $1",
        &[Value::i64(product)],
    )?;
    let mut tx = c.begin(Isolation::Default)?;
    // Ordering more than is in stock violates inventory's CHECK (on_hand >= 0) halfway through.
    let r = place_order(&mut tx, &note, product, 5000);
    expect_sqlstate(r, "23514");
    assert_eq!(tx.status(), TxStatus::Failed);
    // Everything after the failure is refused until rollback.
    expect_sqlstate(tx.execute("SELECT 1", &[]), "25P02");
    tx.rollback()?;
    assert_eq!(c.tx_status(), TxStatus::Idle);
    assert_eq!(
        one_i64(
            &mut c,
            "SELECT count(*) FROM orders WHERE note = $1",
            &[Value::text(&note)]
        )?,
        0
    );
    assert_eq!(
        one_i64(
            &mut c,
            "SELECT on_hand::int8 FROM inventory WHERE product_id = $1",
            &[Value::i64(product)]
        )?,
        before
    );
    // Dropping a transaction rolls it back too.
    {
        let mut tx = c.begin(Isolation::Default)?;
        place_order(&mut tx, &note, product, 1)?;
    }
    assert_eq!(c.tx_status(), TxStatus::Idle);
    assert_eq!(
        one_i64(
            &mut c,
            "SELECT count(*) FROM orders WHERE note = $1",
            &[Value::text(&note)]
        )?,
        0
    );
    Ok(())
}

#[test]
fn savepoints_keep_the_good_work() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let note = tag("savepoint");
    cleanup_orders(&mut c, &note)?;
    let mut tx = c.begin(Isolation::RepeatableRead)?;
    let first = place_order(&mut tx, &note, 100, 1)?;
    let sp = tx.savepoint("before \"second\"")?; // a name that needs quoting
    expect_sqlstate(
        tx.execute(
            "INSERT INTO orders (customer_id, ship_address_id, note) VALUES (999999, 1, $1)",
            &[Value::text(&note)],
        ),
        "23503",
    );
    tx.rollback_to(&sp)?;
    assert_eq!(tx.status(), TxStatus::InTransaction);
    let second = place_order(&mut tx, &note, 101, 2)?;
    tx.release(sp)?;
    tx.commit()?;
    assert_eq!(
        one_i64(
            &mut c,
            "SELECT count(*) FROM orders WHERE note = $1",
            &[Value::text(&note)]
        )?,
        2
    );
    assert_ne!(first, second);
    cleanup_orders(&mut c, &note)?;
    c.execute(
        "UPDATE inventory SET on_hand = 100 + (product_id % 900) WHERE product_id IN (100, 101)",
        &[],
    )?;
    Ok(())
}

// ---- statement cache --------------------------------------------------------------------------

#[test]
fn a_prepared_statement_is_reused_thousands_of_times() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let mut total = 0i64;
    for i in 0..5000i64 {
        let id = (i * 7) % 5000 + 1;
        let mut cur = c.query(
            "SELECT id, name, price_cents FROM products WHERE id = $1",
            &[Value::i64(id)],
        )?;
        let r = cur.next()?.unwrap();
        assert_eq!(r.get::<i64>(0)?, id);
        assert_eq!(r.get::<String>(1)?, format!("Product {id}"));
        assert_eq!(r.get::<i64>(2)?, (id * 37) % 100_000 + 99);
        total += id;
    }
    assert_eq!(total, (0..5000i64).map(|i| (i * 7) % 5000 + 1).sum::<i64>());
    assert_eq!(c.cached_statements(), 1);
    let st = c.prepare("SELECT name FROM products WHERE id = $1", &[oid::INT8])?;
    for i in 0..3000i64 {
        let mut cur = c.query_prepared(&st, &[Value::i64(i % 5000 + 1)])?;
        assert_eq!(
            cur.next()?.unwrap().get::<String>(0)?,
            format!("Product {}", i % 5000 + 1)
        );
    }
    assert_eq!(c.cached_statements(), 2);
    // A tiny cache evicts (and closes) older statements without breaking anything.
    let mut small = connect_with(&addr, "")?;
    for n in 0..300 {
        let sql = format!("SELECT {n}::int8 + $1::int8");
        assert_eq!(one_i64(&mut small, &sql, &[Value::i64(1)])?, n + 1);
    }
    assert!(small.cached_statements() <= 256);
    Ok(())
}

#[test]
fn ddl_change_reprepares_or_fails_clearly_never_wrong_data() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let t = tag("ddl");
    c.execute(&format!("DROP TABLE IF EXISTS {t}"), &[])?;
    c.execute(
        &format!("CREATE TABLE {t} (id int PRIMARY KEY, v text)"),
        &[],
    )?;
    c.execute(&format!("INSERT INTO {t} VALUES (1, 'a')"), &[])?;
    let sql = format!("SELECT * FROM {t} WHERE id = $1");

    let mut cur = c.query(&sql, &[Value::i32(1)])?;
    assert_eq!(cur.fields().len(), 2);
    let r = cur.next()?.unwrap();
    assert_eq!((r.get::<i32>(0)?, r.get::<&str>(1)?), (1, "a"));
    drop(cur);

    // A column is added: the cached statement is stale; the next run re-prepares it.
    c.execute(&format!("ALTER TABLE {t} ADD COLUMN w int DEFAULT 7"), &[])?;
    let mut cur = c.query(&sql, &[Value::i32(1)])?;
    assert_eq!(cur.fields().len(), 3);
    let r = cur.next()?.unwrap();
    assert_eq!(
        (r.get::<i32>(0)?, r.get::<&str>(1)?, r.get::<i32>(2)?),
        (1, "a", 7)
    );
    drop(cur);

    // A column changes type: the decoder must follow it.
    c.execute(&format!("ALTER TABLE {t} ALTER COLUMN id TYPE int8"), &[])?;
    let mut cur = c.query(&sql, &[Value::i32(1)])?;
    assert_eq!(cur.fields()[0].ty, karez_pg::ColumnType::Int64);
    assert_eq!(cur.next()?.unwrap().get::<i64>(0)?, 1);
    drop(cur);

    // An explicit handle is not silently swapped: it fails with the server's clear error, and a
    // fresh prepare works.
    let st = c.prepare(&sql, &[oid::INT4])?;
    assert_eq!(
        c.query_prepared(&st, &[Value::i32(1)])?
            .next()?
            .unwrap()
            .get::<i64>(0)?,
        1
    );
    c.execute(&format!("ALTER TABLE {t} ADD COLUMN z int"), &[])?;
    match c.query_prepared(&st, &[Value::i32(1)]) {
        Err(Error::Server(e)) => {
            assert_eq!(e.sqlstate(), "0A000");
            assert!(
                e.message()
                    .contains("cached plan must not change result type"),
                "{e}"
            );
        }
        other => panic!("stale prepared handle must fail clearly: {other:?}"),
    }
    let st = c.prepare(&sql, &[oid::INT4])?;
    assert_eq!(st.fields().len(), 4);
    assert_eq!(
        c.query_prepared(&st, &[Value::i32(1)])?
            .next()?
            .unwrap()
            .get::<Option<i32>>(3)?,
        None
    );
    c.execute(&format!("DROP TABLE {t}"), &[])?;
    Ok(())
}

// ---- concurrency -------------------------------------------------------------------------------

#[test]
fn many_connections_from_threads_with_their_own_transactions() -> R {
    let addr = need_server!();
    let actor = tag("threads");
    let mut admin = connect(&addr)?;
    admin.execute(
        "DELETE FROM audit_log WHERE actor = $1",
        &[Value::text(&actor)],
    )?;
    let handles: Vec<_> = (0..8)
        .map(|t| {
            let addr = addr.clone();
            let actor = actor.clone();
            thread::spawn(move || -> R {
                let mut c = connect(&addr)?;
                for round in 0..5 {
                    let mut tx = c.begin(Isolation::Default)?;
                    for k in 0..10 {
                        tx.execute(
                            "INSERT INTO audit_log (actor, action, details) VALUES ($1, 'thread', $2)",
                            &[Value::text(&actor), Value::text(&format!("t{t} r{round} k{k}"))],
                        )?;
                    }
                    if round % 2 == 0 {
                        tx.commit()?;
                    } else {
                        tx.rollback()?;
                    }
                }
                Ok(())
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread panicked")?;
    }
    // 3 committed rounds (0, 2, 4) of 10 rows on each of 8 threads.
    assert_eq!(
        one_i64(
            &mut admin,
            "SELECT count(*) FROM audit_log WHERE actor = $1",
            &[Value::text(&actor)]
        )?,
        240
    );
    admin.execute(
        "DELETE FROM audit_log WHERE actor = $1",
        &[Value::text(&actor)],
    )?;
    Ok(())
}

// ---- errors with exact SQLSTATEs ------------------------------------------------------------------

#[test]
fn constraint_and_statement_errors_have_the_right_sqlstate() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    expect_sqlstate(
        c.execute(
            "INSERT INTO customers (email, name) VALUES ('user1@example.test', 'dup')",
            &[],
        ),
        "23505",
    );
    expect_sqlstate(
        c.execute(
            "INSERT INTO orders (customer_id, ship_address_id) VALUES (999999, 1)",
            &[],
        ),
        "23503",
    );
    expect_sqlstate(
        c.execute(
            "INSERT INTO customers (email, name) VALUES ($1, $2)",
            &[Value::text("nn@example.test"), Value::Null],
        ),
        "23502",
    );
    expect_sqlstate(
        c.execute("INSERT INTO customers (email, name, loyalty_points) VALUES ('c@example.test', 'c', -1)", &[]),
        "23514",
    );
    expect_sqlstate(c.execute("SELEC 1", &[]), "42601");
    expect_sqlstate(c.execute("SELECT * FROM no_such_table", &[]), "42P01");
    expect_sqlstate(
        c.query("SELECT 1 / $1::int4", &[Value::i32(0)])
            .and_then(|mut cur| cur.next().map(|_| ())),
        "22012",
    );
    let e = c
        .execute(
            "INSERT INTO customers (email, name) VALUES ('user1@example.test', 'dup')",
            &[],
        )
        .unwrap_err();
    assert!(
        e.to_string().contains("23505") && e.to_string().contains("duplicate key"),
        "{e}"
    );
    // None of that hurt the connection.
    assert!(!c.is_broken());
    assert_eq!(c.tx_status(), TxStatus::Idle);
    assert_eq!(
        one_i64(&mut c, "SELECT count(*) FROM customers", &[])?,
        20000
    );
    Ok(())
}

#[test]
fn statement_timeout_is_57014_and_the_connection_survives() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("SET statement_timeout = 300", &[])?;
    let started = Instant::now();
    expect_sqlstate(c.execute("SELECT pg_sleep(10)", &[]), "57014");
    assert!(started.elapsed() < Duration::from_secs(5));
    c.execute("SET statement_timeout = 0", &[])?;
    assert_eq!(one_i64(&mut c, "SELECT 5::int8", &[])?, 5);
    Ok(())
}

#[test]
fn two_serializable_transactions_give_40001() -> R {
    let addr = need_server!();
    let t = tag("ser");
    let mut admin = connect(&addr)?;
    admin.execute(&format!("DROP TABLE IF EXISTS {t}"), &[])?;
    admin.execute(&format!("CREATE TABLE {t} (grp int, v int)"), &[])?;
    admin.execute(&format!("INSERT INTO {t} VALUES (1, 1), (2, 1)"), &[])?;
    let mut a = connect(&addr)?;
    let mut b = connect(&addr)?;
    let sum = format!("SELECT sum(v)::int8 FROM {t} WHERE grp = $1");
    let ins = format!("INSERT INTO {t} VALUES ($1, 10)");
    let mut ta = a.begin(Isolation::Serializable)?;
    let mut tb = b.begin(Isolation::Serializable)?;
    for (tx, read, write) in [(&mut ta, 1, 2), (&mut tb, 2, 1)] {
        let mut cur = tx.query(&sum, &[Value::i32(read)])?;
        while cur.next()?.is_some() {}
        drop(cur);
        tx.execute(&ins, &[Value::i32(write)])?;
    }
    ta.commit()?;
    let r = match tb.execute("SELECT 1", &[]) {
        Ok(_) => tb.commit(),
        Err(e) => {
            tb.rollback()?;
            Err(e)
        }
    };
    expect_sqlstate(r, "40001");
    assert_eq!(b.tx_status(), TxStatus::Idle);
    admin.execute(&format!("DROP TABLE {t}"), &[])?;
    Ok(())
}

#[test]
fn deadlock_is_40p01_for_exactly_one_side() -> R {
    let addr = need_server!();
    let t = tag("dl");
    let mut admin = connect(&addr)?;
    admin.execute(&format!("DROP TABLE IF EXISTS {t}"), &[])?;
    admin.execute(
        &format!("CREATE TABLE {t} (id int PRIMARY KEY, v int)"),
        &[],
    )?;
    admin.execute(&format!("INSERT INTO {t} VALUES (1, 0), (2, 0)"), &[])?;
    let barrier = Arc::new(Barrier::new(2));
    let run = |first: i32, second: i32| {
        let (addr, t, barrier) = (addr.clone(), t.clone(), barrier.clone());
        thread::spawn(move || -> Result<(), Error> {
            let mut c = connect(&addr)?;
            let mut tx = c.begin(Isolation::Default)?;
            let upd = format!("UPDATE {t} SET v = v + 1 WHERE id = $1");
            tx.execute(&upd, &[Value::i32(first)])?;
            barrier.wait();
            let r = tx.execute(&upd, &[Value::i32(second)]);
            match r {
                Ok(_) => tx.commit(),
                Err(e) => {
                    tx.rollback()?;
                    Err(e)
                }
            }
        })
    };
    let (h1, h2) = (run(1, 2), run(2, 1));
    let results = [h1.join().unwrap(), h2.join().unwrap()];
    let victims = results
        .iter()
        .filter(|r| matches!(r, Err(e) if e.sqlstate() == Some("40P01")))
        .count();
    assert_eq!(victims, 1, "{results:?}");
    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "{results:?}"
    );
    admin.execute(&format!("DROP TABLE {t}"), &[])?;
    Ok(())
}

#[test]
fn backend_killed_mid_result_is_an_error_not_a_hang() -> R {
    let addr = need_server!();
    let mut victim = connect_with(&addr, "&read_timeout=30")?;
    let mut killer = connect(&addr)?;
    let pid = victim.process_id();
    let started = Instant::now();
    let mut cur = victim.query(
        "SELECT id, at, actor, action, details FROM audit_log ORDER BY id",
        &[],
    )?;
    let first = cur.next_chunk()?.expect("first chunk").rows();
    assert!(first > 0);
    let killed = one_i64(
        &mut killer,
        "SELECT (CASE WHEN pg_terminate_backend($1) THEN 1 ELSE 0 END)::int8",
        &[Value::i32(pid)],
    )?;
    assert_eq!(killed, 1);
    let mut err = None;
    loop {
        match cur.next_chunk() {
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }
    let e = err.expect("the stream must end with an error, not a clean finish");
    eprintln!("killed mid-result: {e}");
    assert!(
        started.elapsed() < Duration::from_secs(25),
        "took too long: {:?}",
        started.elapsed()
    );
    // Either the server's FATAL 57P01 arrives, or the socket closes first. Both are errors.
    assert!(e.sqlstate() == Some("57P01") || e.is_fatal(), "{e}");
    drop(cur);
    assert!(victim.is_broken() || e.sqlstate() == Some("57P01"));
    assert!(victim.query("SELECT 1", &[]).is_err());
    Ok(())
}

#[test]
fn chunk_type_is_exported() {
    // Compile-time check that the facade types needed by users are re-exported.
    fn _takes(_: &Chunk) {}
}

// ---- cancel, notifications, COPY, batches, pool ------------------------------------------

#[test]
fn a_running_statement_can_be_cancelled_from_another_thread() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let token = c.cancel_token();
    let t0 = Instant::now();
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        token.cancel()
    });
    let r = c.query("SELECT pg_sleep(30)", &[]).and_then(|mut cur| {
        while cur.next()?.is_some() {}
        Ok(())
    });
    canceller.join().unwrap()?;
    expect_sqlstate(r, "57014");
    assert!(t0.elapsed() < Duration::from_secs(20), "{:?}", t0.elapsed());
    // The connection is fine afterwards.
    assert!(!c.is_broken());
    assert_eq!(one_i64(&mut c, "SELECT 7::int8", &[])?, 7);
    // Cancelling an idle connection does nothing.
    c.cancel_token().cancel()?;
    assert_eq!(one_i64(&mut c, "SELECT 8::int8", &[])?, 8);
    // Cancelling a second, idle connection leaves it usable too.
    let mut other = connect(&addr)?;
    other.cancel_token().cancel()?;
    assert_eq!(one_i64(&mut other, "SELECT 9::int8", &[])?, 9);
    Ok(())
}

#[test]
fn notifications_are_delivered_while_waiting_and_during_queries() -> R {
    let addr = need_server!();
    let mut listener = connect(&addr)?;
    let mut sender = connect(&addr)?;
    let chan = tag("chan");
    listener.execute(&format!("LISTEN {chan}"), &[])?;
    // Nothing yet: a short wait times out.
    let t0 = Instant::now();
    assert_eq!(
        listener.wait_for_notification(Some(Duration::from_millis(200)))?,
        None
    );
    assert!(t0.elapsed() >= Duration::from_millis(150));
    // Notify from another connection while the listener waits.
    let chan2 = chan.clone();
    let h = thread::spawn(move || -> Result<(), Error> {
        thread::sleep(Duration::from_millis(200));
        sender.execute(
            "SELECT pg_notify($1, $2)",
            &[Value::text(&chan2), Value::text("hello 🎉")],
        )?;
        sender.execute(
            "SELECT pg_notify($1, $2)",
            &[Value::text(&chan2), Value::text("")],
        )?;
        Ok(())
    });
    let n = listener
        .wait_for_notification(Some(Duration::from_secs(10)))?
        .expect("a notification");
    assert_eq!(
        (n.channel.as_str(), n.payload.as_str()),
        (chan.as_str(), "hello 🎉")
    );
    assert!(n.process_id != 0);
    h.join().unwrap()?;
    // The second one arrived too; it is queued, whether it came in during the wait or the next.
    let n2 = listener
        .wait_for_notification(Some(Duration::from_secs(10)))?
        .expect("the second notification");
    assert_eq!(n2.payload, "");
    // A notification that arrives while a query runs is kept for later.
    let mut s2 = connect(&addr)?;
    s2.execute("SELECT pg_notify($1, 'late')", &[Value::text(&chan)])?;
    thread::sleep(Duration::from_millis(100));
    assert_eq!(one_i64(&mut listener, "SELECT 1::int8", &[])?, 1);
    let got = listener.take_notifications();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].payload, "late");
    assert_eq!(listener.dropped_notifications(), 0);
    Ok(())
}

#[test]
fn copy_in_and_out_round_trip() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let t = tag("copy");
    c.execute(
        &format!("CREATE TEMP TABLE {t} (id int PRIMARY KEY, name text, amount numeric(12,2), note text)"),
        &[],
    )?;
    let mut csv = String::new();
    for i in 0..20_000 {
        csv.push_str(&format!(
            "{i},name {i},{}.{:02},\"quote \"\"{i}\"\", comma, \nnewline\"\n",
            i * 3,
            i % 100
        ));
    }
    let n = c.copy_in(
        &format!("COPY {t} FROM STDIN WITH (FORMAT csv)"),
        &mut csv.as_bytes(),
    )?;
    assert_eq!(n, 20_000);
    assert_eq!(
        one_i64(&mut c, &format!("SELECT count(*)::int8 FROM {t}"), &[])?,
        20_000
    );
    // Read back with COPY TO and compare to what was sent.
    let mut out = Vec::new();
    let n = c.copy_out(
        &format!("COPY (SELECT * FROM {t} ORDER BY id) TO STDOUT WITH (FORMAT csv)"),
        &mut out,
    )?;
    assert_eq!(n, 20_000);
    assert_eq!(String::from_utf8(out).unwrap(), csv);
    // Binary COPY: out, then back in to a second table.
    let t2 = tag("copy2");
    c.execute(&format!("CREATE TEMP TABLE {t2} (LIKE {t})"), &[])?;
    let mut bin = Vec::new();
    c.copy_out(
        &format!("COPY {t} TO STDOUT WITH (FORMAT binary)"),
        &mut bin,
    )?;
    assert_eq!(&bin[..11], b"PGCOPY\n\xff\r\n\0");
    let n = c.copy_in(
        &format!("COPY {t2} FROM STDIN WITH (FORMAT binary)"),
        &mut &bin[..],
    )?;
    assert_eq!(n, 20_000);
    assert_eq!(
        one_i64(
            &mut c,
            &format!("SELECT count(*)::int8 FROM {t} a JOIN {t2} b USING (id, name, amount, note)"),
            &[]
        )?,
        20_000
    );
    // Bad data: the server's error comes back, nothing is loaded, the connection is usable.
    let before = one_i64(&mut c, &format!("SELECT count(*)::int8 FROM {t2}"), &[])?;
    let r = c.copy_in(
        &format!("COPY {t2} FROM STDIN WITH (FORMAT csv)"),
        &mut "99999,ok,1.00,x\nnot-a-number,bad,1.00,y\n".as_bytes(),
    );
    expect_sqlstate(r, "22P02");
    assert_eq!(
        one_i64(&mut c, &format!("SELECT count(*)::int8 FROM {t2}"), &[])?,
        before
    );
    // A duplicate key.
    let r = c.copy_in(
        &format!("COPY {t} FROM STDIN WITH (FORMAT csv)"),
        &mut "1,dup,1.00,x\n".as_bytes(),
    );
    expect_sqlstate(r, "23505");
    // A reader that fails: the COPY is aborted, the error reports the I/O failure.
    struct Failing(usize);
    impl std::io::Read for Failing {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                return Err(std::io::Error::other("source went away"));
            }
            self.0 -= 1;
            let line = b"500000,x,1.00,y\n";
            b[..line.len()].copy_from_slice(line);
            Ok(line.len())
        }
    }
    let r = c.copy_in(
        &format!("COPY {t2} FROM STDIN WITH (FORMAT csv)"),
        &mut Failing(3),
    );
    assert!(matches!(r, Err(Error::Caller(_))), "{r:?}");
    assert!(!c.is_broken());
    assert_eq!(
        one_i64(
            &mut c,
            &format!("SELECT count(*)::int8 FROM {t2} WHERE id = 500000"),
            &[]
        )?,
        0
    );
    // Wrong statement kinds are usage errors and leave the connection usable.
    assert!(matches!(
        c.copy_in("SELECT 1", &mut "".as_bytes()),
        Err(Error::Usage(_))
    ));
    assert!(matches!(
        c.copy_in(&format!("COPY {t} TO STDOUT"), &mut "".as_bytes()),
        Err(Error::Usage(_))
    ));
    let mut sink = Vec::new();
    assert!(matches!(
        c.copy_out(&format!("COPY {t} FROM STDIN"), &mut sink),
        Err(Error::Usage(_))
    ));
    expect_sqlstate(
        c.copy_out("COPY no_such_table_xyz TO STDOUT", &mut sink),
        "42P01",
    );
    // A writer that fails: the stream is drained and the connection stays usable.
    struct BadSink;
    impl std::io::Write for BadSink {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let r = c.copy_out(&format!("COPY {t} TO STDOUT"), &mut BadSink);
    assert!(matches!(r, Err(Error::Caller(_))), "{r:?}");
    assert!(!c.is_broken());
    assert_eq!(one_i64(&mut c, "SELECT 1::int8", &[])?, 1);
    Ok(())
}

#[test]
fn execute_many_sends_a_batch_in_one_round_trip_and_is_atomic() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let t = tag("batch");
    c.execute(
        &format!("CREATE TEMP TABLE {t} (id int PRIMARY KEY, v text)"),
        &[],
    )?;
    let ins = format!("INSERT INTO {t} VALUES ($1, $2)");
    let upd = format!("UPDATE {t} SET v = v || '!' WHERE id < $1");
    let texts: Vec<String> = (0..1000).map(|i| format!("row {i}")).collect();
    let mut items: Vec<(&str, Vec<Value>)> = Vec::new();
    for (i, s) in texts.iter().enumerate() {
        items.push((&ins, vec![Value::i32(i as i32), Value::text(s)]));
    }
    items.push((&upd, vec![Value::i32(10)]));
    let refs: Vec<(&str, &[Value])> = items.iter().map(|(s, p)| (*s, p.as_slice())).collect();
    let counts = c.execute_many(&refs)?;
    assert_eq!(counts.len(), 1001);
    assert!(counts[..1000].iter().all(|n| *n == 1));
    assert_eq!(counts[1000], 10);
    assert_eq!(
        one_i64(&mut c, &format!("SELECT count(*)::int8 FROM {t}"), &[])?,
        1000
    );
    assert_eq!(
        one_i64(
            &mut c,
            &format!("SELECT count(*)::int8 FROM {t} WHERE v LIKE '%!'"),
            &[]
        )?,
        10
    );
    // One failing statement undoes the whole batch.
    let bad: Vec<(&str, Vec<Value>)> = vec![
        (&ins, vec![Value::i32(5000), Value::text("fine")]),
        (&ins, vec![Value::i32(5), Value::text("duplicate key")]),
        (&ins, vec![Value::i32(5001), Value::text("never reached")]),
    ];
    let refs: Vec<(&str, &[Value])> = bad.iter().map(|(s, p)| (*s, p.as_slice())).collect();
    expect_sqlstate(c.execute_many(&refs), "23505");
    assert_eq!(
        one_i64(&mut c, &format!("SELECT count(*)::int8 FROM {t}"), &[])?,
        1000
    );
    assert!(!c.is_broken());
    // Empty batch, wrong parameter count, and too many distinct statements.
    assert_eq!(c.execute_many(&[])?, Vec::<u64>::new());
    assert!(matches!(
        c.execute_many(&[(ins.as_str(), &[Value::i32(1)])]),
        Err(Error::Usage(_))
    ));
    let sqls: Vec<String> = (0..300).map(|i| format!("SELECT {i}")).collect();
    let many: Vec<(&str, &[Value])> = sqls.iter().map(|s| (s.as_str(), &[][..])).collect();
    assert!(matches!(c.execute_many(&many), Err(Error::Usage(_))));
    assert_eq!(one_i64(&mut c, "SELECT 1::int8", &[])?, 1);
    Ok(())
}

#[test]
fn the_pool_shares_rolls_back_and_replaces_connections() -> R {
    let addr = need_server!();
    let cfg = Config::from_url(&url(&addr, "karez_scram", Some(SCRAM_PW), "karez", ""))?;
    let pool = karez_pg::Pool::new(cfg, 2)?;
    assert_eq!((pool.size(), pool.idle()), (0, 0));
    // Many threads, two connections.
    let mut hs = Vec::new();
    for i in 0..8i64 {
        let p = pool.clone();
        hs.push(thread::spawn(move || -> Result<i64, Error> {
            let mut total = 0;
            for _ in 0..20 {
                let mut c = p.get()?;
                total += one_i64(&mut c, "SELECT $1::int8", &[Value::i64(i)])?;
                assert!(p.size() <= 2);
            }
            Ok(total)
        }));
    }
    let sum: i64 = hs.into_iter().map(|h| h.join().unwrap().unwrap()).sum();
    assert_eq!(sum, 20 * (0..8).sum::<i64>());
    assert!(pool.size() <= 2 && pool.idle() == pool.size());
    // A connection returned inside a transaction is rolled back before reuse.
    let pid;
    {
        let mut c = pool.get()?;
        pid = c.process_id();
        c.execute("BEGIN", &[])?;
        assert_eq!(c.tx_status(), TxStatus::InTransaction);
    }
    let c = pool.get()?;
    assert_eq!(c.tx_status(), TxStatus::Idle);
    assert!(c.process_id() > 0);
    let _ = pid;
    // Exhausted: a bounded wait times out; releasing one lets the next get through.
    let c2 = pool.get()?;
    let r = pool.get_timeout(Duration::from_millis(200));
    assert!(matches!(r, Err(Error::Usage(_))), "{r:?}");
    drop(c);
    let c3 = pool.get_timeout(Duration::from_secs(5))?;
    // A broken connection is discarded, not reused; a discarded one frees its slot.
    let victim_pid = c3.process_id();
    let mut killer = connect(&addr)?;
    assert_eq!(
        one_i64(
            &mut killer,
            "SELECT count(*)::int8 FROM pg_terminate_backend($1::int)",
            &[Value::i32(victim_pid)]
        )?,
        1
    );
    drop(c2);
    let mut c3 = c3;
    assert!(c3.query("SELECT 1", &[]).is_err());
    drop(c3);
    let size_after = pool.size();
    assert!(size_after <= 1, "{size_after}");
    let mut fresh = pool.get()?;
    assert_eq!(one_i64(&mut fresh, "SELECT 5::int8", &[])?, 5);
    fresh.discard();
    assert!(pool.size() <= 1);
    Ok(())
}

#[test]
fn execute_many_does_not_deadlock_when_requests_and_results_are_both_large() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    // Each request carries 300 KB and each result is 1 MB. 60 of them is 18 MB out and 60 MB
    // back, far more than the socket buffers on both sides, so a client that writes the whole
    // batch before reading anything blocks forever once the server stops reading.
    let big = "x".repeat(300_000);
    let sql = "SELECT repeat('y', 1000000) WHERE length($1) >= 0";
    let params = [Value::text(&big)];
    let items: Vec<(&str, &[Value])> = (0..60).map(|_| (sql, &params[..])).collect();
    let t0 = Instant::now();
    let counts = c.execute_many(&items)?;
    assert_eq!(counts, vec![1u64; 60]);
    assert!(t0.elapsed() < Duration::from_secs(60), "{:?}", t0.elapsed());
    assert!(!c.is_broken());
    assert_eq!(one_i64(&mut c, "SELECT 3::int8", &[])?, 3);
    Ok(())
}

#[test]
fn a_failing_statement_in_a_large_batch_leaves_the_connection_usable() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let big = "x".repeat(300_000);
    let ok = "SELECT repeat('y', 100000) WHERE length($1) >= 0";
    let bad = "SELECT 1 / (length($1) - length($1))";
    let params = [Value::text(&big)];
    let mut items: Vec<(&str, &[Value])> = (0..10).map(|_| (ok, &params[..])).collect();
    items.push((bad, &params[..]));
    items.extend((0..10).map(|_| (ok, &params[..])));
    expect_sqlstate(c.execute_many(&items), "22012");
    assert!(!c.is_broken());
    assert_eq!(one_i64(&mut c, "SELECT 4::int8", &[])?, 4);
    // The same batch without the failing statement works.
    let items: Vec<(&str, &[Value])> = (0..20).map(|_| (ok, &params[..])).collect();
    assert_eq!(c.execute_many(&items)?, vec![1u64; 20]);
    Ok(())
}

#[test]
fn the_pool_serves_waiters_in_arrival_order() -> R {
    let addr = need_server!();
    let cfg = Config::from_url(&url(&addr, "karez_scram", Some(SCRAM_PW), "karez", ""))?;
    let pool = karez_pg::Pool::new(cfg, 1)?;
    let held = pool.get()?;
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut hs = Vec::new();
    for i in 0..5 {
        let (p, o) = (pool.clone(), order.clone());
        hs.push(thread::spawn(move || -> Result<(), Error> {
            let _c = p.get()?;
            o.lock().unwrap().push(i);
            thread::sleep(Duration::from_millis(30));
            Ok(())
        }));
        thread::sleep(Duration::from_millis(150)); // waiter i is queued before waiter i + 1
    }
    drop(held);
    for h in hs {
        h.join().unwrap()?;
    }
    assert_eq!(*order.lock().unwrap(), vec![0, 1, 2, 3, 4]);
    // A waiter that times out leaves the queue without blocking the ones behind it.
    let held = pool.get()?;
    let r = pool.get_timeout(Duration::from_millis(100));
    assert!(matches!(r, Err(Error::Usage(_))));
    drop(held);
    assert!(pool.get_timeout(Duration::from_secs(5)).is_ok());
    Ok(())
}
