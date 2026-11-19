//! Binary COPY against a real PostgreSQL. The address is `KAREZ_TEST_PG`; see tests/docker/README.md.
//!
//! The judge is the server: a table loaded by binary COPY must hold exactly what a table loaded by
//! bound-parameter `INSERT` of the same values holds (compared as text, row by row).

use karez_chunk::rng::SplitMix64;
use karez_chunk::{Date, Decimal128, Interval, Time, Timestamp};
use karez_pg::{Array, Connection, Error, Isolation, Value};
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

fn count(c: &mut Connection, table: &str) -> Result<i64, Error> {
    let mut cur = c.query(&format!("SELECT count(*)::int8 FROM {table}"), &[])?;
    let n = cur.next()?.expect("a row").get::<i64>(0)?;
    cur.close()?;
    Ok(n)
}

/// One text per table: every row as text, in id order, hashed by the server.
fn digest(c: &mut Connection, table: &str) -> Result<String, Error> {
    let mut cur = c.query(
        &format!("SELECT md5(coalesce(string_agg(t::text, '|' ORDER BY id), ''))::text FROM {table} t"),
        &[],
    )?;
    let v = cur.next()?.expect("a row").get::<String>(0)?;
    cur.close()?;
    Ok(v)
}

const COLUMNS: &str = "id int8, b bool, s2 int2, s4 int4, s8 int8, f4 float4, f8 float8, t text, \
    y bytea, u uuid, d date, ts timestamp, tz timestamptz, tm time, iv interval, n numeric, \
    ai int4[], at text[]";

fn names() -> &'static str {
    "id, b, s2, s4, s8, f4, f8, t, y, u, d, ts, tz, tm, iv, n, ai, at"
}

/// The values of row `i`: edge values for the first rows, random ones after, NULL now and then.
struct RowData {
    text: String,
    bytes: Vec<u8>,
    ai: Vec<Option<i32>>,
    at_owned: Vec<Option<String>>,
}

impl RowData {
    fn make(i: i64, rng: &mut SplitMix64) -> RowData {
        let r = |rng: &mut SplitMix64| rng.next_u64();
        let len = (r(rng) % 40) as usize;
        let text: String = (0..len)
            .map(|_| char::from_u32(0x20 + (r(rng) % 0x5f) as u32 + if r(rng) % 7 == 0 { 0x400 } else { 0 }).unwrap())
            .collect();
        let bytes: Vec<u8> = (0..(r(rng) % 30) as usize).map(|_| r(rng) as u8).collect();
        let ai: Vec<Option<i32>> = (0..(r(rng) % 5) as usize)
            .map(|_| if r(rng) % 4 == 0 { None } else { Some(r(rng) as i32) })
            .collect();
        let at_owned: Vec<Option<String>> = (0..(r(rng) % 4) as usize)
            .map(|k| if r(rng) % 4 == 0 { None } else { Some(format!("e{i}-{k}")) })
            .collect();
        RowData { text, bytes, ai, at_owned }
    }
}

fn values<'a>(i: i64, rng: &mut SplitMix64, d: &'a RowData, at: &'a [Option<&'a str>]) -> Vec<Value<'a>> {
    let r = |rng: &mut SplitMix64| rng.next_u64();
    let null = |rng: &mut SplitMix64| r(rng) % 6 == 0;
    let mut v: Vec<Value> = Vec::new();
    v.push(Value::I64(i));
    let edge = i < 8;
    v.push(if null(rng) { Value::Null } else { Value::Bool(r(rng) % 2 == 0) });
    v.push(if edge { Value::I16([i16::MIN, i16::MAX, 0, -1, 1, 255, -256, 12345][i as usize]) } else if null(rng) { Value::Null } else { Value::I16(r(rng) as i16) });
    v.push(if edge { Value::I32([i32::MIN, i32::MAX, 0, -1, 1, 65535, -65536, 7][i as usize]) } else if null(rng) { Value::Null } else { Value::I32(r(rng) as i32) });
    v.push(if edge { Value::I64([i64::MIN, i64::MAX, 0, -1, 1, 1 << 40, -(1 << 40), 7][i as usize]) } else if null(rng) { Value::Null } else { Value::I64(r(rng) as i64) });
    v.push(if edge { Value::F32([0.0, -0.0, f32::MAX, f32::MIN_POSITIVE, f32::INFINITY, f32::NEG_INFINITY, 1.5, -2.25][i as usize]) } else if null(rng) { Value::Null } else { Value::F32(f32::from_bits(r(rng) as u32 & 0x7f7f_ffff)) });
    v.push(if edge { Value::F64([0.0, -0.0, f64::MAX, f64::MIN_POSITIVE, f64::INFINITY, f64::NEG_INFINITY, 0.1, -1e300][i as usize]) } else if null(rng) { Value::Null } else { Value::F64(f64::from_bits(r(rng) & 0x7fef_ffff_ffff_ffff)) });
    v.push(if null(rng) { Value::Null } else { Value::Text(&d.text) });
    v.push(if null(rng) { Value::Null } else { Value::Bytes(&d.bytes) });
    v.push(if null(rng) { Value::Null } else { let mut u = [0u8; 16]; u[..8].copy_from_slice(&r(rng).to_be_bytes()); u[8..].copy_from_slice(&r(rng).to_be_bytes()); Value::Uuid(u) });
    v.push(if null(rng) { Value::Null } else { Value::Date(Date((r(rng) % 4_000_000) as i32 - 1_000_000)) });
    v.push(if null(rng) { Value::Null } else { Value::Timestamp(Timestamp((r(rng) % 8_000_000_000_000_000) as i64 - 4_000_000_000_000_000)) });
    v.push(if null(rng) { Value::Null } else { Value::TimestampTz(Timestamp((r(rng) % 8_000_000_000_000_000) as i64 - 4_000_000_000_000_000)) });
    v.push(if null(rng) { Value::Null } else { Value::Time(Time((r(rng) % 86_400_000_001) as i64)) });
    v.push(if null(rng) { Value::Null } else { Value::Interval(Interval { months: r(rng) as i32 % 100_000, days: r(rng) as i32 % 100_000, micros: (r(rng) % 1_000_000_000_000) as i64 - 500_000_000_000 }) });
    v.push(if null(rng) { Value::Null } else { Value::Numeric(Decimal128 { unscaled: (r(rng) as i64 as i128) * 1_000_003, scale: (r(rng) % 8) as u8 }) });
    v.push(if null(rng) { Value::Null } else { Value::Array(Array::I32(&d.ai)) });
    v.push(if null(rng) { Value::Null } else { Value::Array(Array::Text(at)) });
    v
}

#[test]
fn binary_copy_loads_what_insert_loads_for_every_type() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let (a, b) = (
        format!("karez_bc_a_{}", std::process::id()),
        format!("karez_bc_b_{}", std::process::id()),
    );
    for t in [&a, &b] {
        c.execute(&format!("CREATE TEMP TABLE {t} ({COLUMNS})"), &[])?;
    }
    let rows = 3000i64;
    let mut rng = SplitMix64::new(77);
    let mut w = c.copy_in_binary(&format!("COPY {a} ({}) FROM STDIN (FORMAT binary)", names()))?;
    let mut all = Vec::new();
    for i in 0..rows {
        let d = RowData::make(i, &mut rng);
        all.push((i, d));
    }
    // the same values twice: the generator is replayed from the same seed for each table
    let mut rng2 = SplitMix64::new(1234);
    let mut seeds = Vec::new();
    for _ in 0..rows {
        seeds.push(rng2.next_u64());
    }
    for (i, d) in &all {
        let at: Vec<Option<&str>> = d.at_owned.iter().map(|o| o.as_deref()).collect();
        let mut g = SplitMix64::new(seeds[*i as usize]);
        w.row(&values(*i, &mut g, d, &at))?;
    }
    assert_eq!(w.rows(), rows as u64);
    assert_eq!(w.finish()?, rows as u64);
    let ins = format!(
        "INSERT INTO {b} ({}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)",
        names()
    );
    let stmt = c.prepare(&ins, &[])?;
    for (i, d) in &all {
        let at: Vec<Option<&str>> = d.at_owned.iter().map(|o| o.as_deref()).collect();
        let mut g = SplitMix64::new(seeds[*i as usize]);
        c.execute_prepared(&stmt, &values(*i, &mut g, d, &at))?;
    }
    assert_eq!(count(&mut c, &a)?, rows);
    assert_eq!(count(&mut c, &b)?, rows);
    assert_eq!(digest(&mut c, &a)?, digest(&mut c, &b)?, "the two tables differ");
    Ok(())
}

#[test]
fn errors_leave_the_connection_usable_and_load_nothing() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let t = format!("karez_bc_e_{}", std::process::id());
    c.execute(&format!("CREATE TEMP TABLE {t} (id int4 PRIMARY KEY, name text)"), &[])?;
    let sql = format!("COPY {t} (id, name) FROM STDIN (FORMAT binary)");

    // a wrong number of values is refused by the client; the writer takes the next row
    let mut w = c.copy_in_binary(&sql)?;
    assert!(matches!(w.row(&[Value::I32(1)]), Err(Error::Proto(_))));
    assert!(w.row(&[Value::I32(1), Value::Unknown("x")]).is_err());
    w.row(&[Value::I32(1), Value::Text("one")])?;
    assert_eq!(w.finish()?, 1);

    // a wrong width (int8 into int4) is refused by the server: nothing of that COPY stays
    let mut w = c.copy_in_binary(&sql)?;
    w.row(&[Value::I32(2), Value::Text("two")])?;
    w.row(&[Value::I64(3), Value::Text("three")])?;
    assert!(matches!(w.finish(), Err(Error::Server(_))));
    assert_eq!(count(&mut c, &t)?, 1);

    // a unique violation at the end of the stream
    let mut w = c.copy_in_binary(&sql)?;
    w.row(&[Value::I32(1), Value::Text("again")])?;
    assert!(matches!(w.finish(), Err(Error::Server(e)) if e.sqlstate() == "23505"));
    assert_eq!(count(&mut c, &t)?, 1);

    // dropped without finish, and abort: nothing is loaded and the connection works
    let mut w = c.copy_in_binary(&sql)?;
    w.row(&[Value::I32(10), Value::Text("ten")])?;
    drop(w);
    let mut w = c.copy_in_binary(&sql)?;
    w.row(&[Value::I32(11), Value::Text("eleven")])?;
    w.abort()?;
    assert_eq!(count(&mut c, &t)?, 1);
    assert!(!c.is_broken());

    // a statement that is not binary is refused, and the connection is fine afterwards
    assert!(matches!(
        c.copy_in_binary(&format!("COPY {t} (id, name) FROM STDIN (FORMAT csv)")).err(),
        Some(Error::Usage(_))
    ));
    assert!(c.copy_in_binary("SELECT 1").is_err());
    assert_eq!(count(&mut c, &t)?, 1);
    Ok(())
}

#[test]
fn a_large_load_crosses_the_send_threshold_and_rolls_back_with_its_transaction() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let t = format!("karez_bc_l_{}", std::process::id());
    c.execute(&format!("CREATE TEMP TABLE {t} (id int8, a float8, s text)"), &[])?;
    let sql = format!("COPY {t} FROM STDIN (FORMAT binary)");
    let n = 300_000u64;
    let mut w = c.copy_in_binary(&sql)?;
    let mut s = String::new();
    for i in 0..n {
        s.clear();
        s.push_str("row-");
        s.push_str(&i.to_string());
        w.row(&[Value::I64(i as i64), Value::F64(i as f64 / 3.0), Value::Text(&s)])?;
    }
    assert_eq!(w.finish()?, n);
    assert_eq!(count(&mut c, &t)?, n as i64);
    let mut cur = c.query(&format!("SELECT sum(a), max(length(s)) FROM {t}"), &[])?;
    let r = cur.next()?.expect("a row");
    let (sum, maxlen) = (r.get::<f64>(0)?, r.get::<i32>(1)?);
    drop(cur);
    assert!((sum - (0..n).map(|i| i as f64 / 3.0).sum::<f64>()).abs() < 1.0);
    assert_eq!(maxlen, format!("row-{}", n - 1).len() as i32);

    // inside a transaction: rolled back with it
    c.execute(&format!("TRUNCATE {t}"), &[])?;
    let mut tx = c.begin(Isolation::ReadCommitted)?;
    {
        let mut w = tx.copy_in_binary(&sql)?;
        for i in 0..1000i64 {
            w.row(&[Value::I64(i), Value::F64(0.0), Value::Text("x")])?;
        }
        assert_eq!(w.finish()?, 1000);
    }
    tx.rollback()?;
    assert_eq!(count(&mut c, &t)?, 0);
    Ok(())
}
