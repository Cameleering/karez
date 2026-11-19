//! Differential tests (M1 step 4): every natively supported type, checked against the server.
//!
//! For each type the same values travel four ways and the server is the judge:
//!
//! 1. **Read, binary.** Edge-case values are written with SQL literals (the server's own parser), read
//!    back in the binary format, and sent straight back as binary parameters. The server must print
//!    the same text for the copy as for the original (`$1::T::text = v::text`) and consider them
//!    equal (`IS NOT DISTINCT FROM`). This needs no formatter on our side, so it covers the types
//!    whose text form we do not produce (floats, dates, timestamps, time zones).
//! 2. **Parameters, text format.** The server's text of each value sent as `Value::Unknown`, which
//!    is the text-format path, must give the same result as the binary parameter.
//! 3. **Write, random.** 300 random values per type (`KAREZ_DIFF_ROWS` changes the number; `KAREZ_DIFF_SEED` the seed) go in as binary parameters, are read back in
//!    binary and must be identical (floats by bit pattern, NaN as NaN). Random values of `numeric`,
//!    `interval` and `time` are also compared with the server's text through `Display`.
//! 4. **Arrays.** Random one-dimensional arrays with NULLs go in as `Value::Array`, are read back as
//!    text and must match the server's `::text`, and the server must find the stored value equal to
//!    the parameter.
//!
//! The values are NULL, zero, one, the minimum and maximum, the type's special values (`NaN`, the
//! infinities, `-0`, subnormals, dates at both ends of the calendar, DST changes) and random ones.
//! The server address is `KAREZ_TEST_PG`; see tests/docker/README.md. Without a server each test
//! prints `SKIPPED` and returns, unless `KAREZ_REQUIRE_SERVER=1`.

use karez_chunk::rng::SplitMix64;
use karez_pg::{
    Array, Connection, Date, Decimal128, Error, Interval, RowRef, Time, Timestamp, Value,
};
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
    let mut c = Connection::connect_url(&format!(
        "postgresql://karez_scram:karez_test_only_scram@{addr}/karez?sslmode=disable"
    ))?;
    c.execute("SET intervalstyle = 'iso_8601'", &[])?;
    c.execute("SET TimeZone = 'UTC'", &[])?;
    Ok(c)
}

/// The types under test. `Num` carries precision and scale.
#[derive(Debug, Clone, Copy, PartialEq)]
enum K {
    Bool,
    I16,
    I32,
    I64,
    F32,
    F64,
    Text,
    Bytea,
    Uuid,
    Date,
    Ts,
    Tstz,
    Num(u8, u8),
    Iv,
    Time,
}

impl K {
    fn sql(self) -> String {
        match self {
            K::Bool => "bool".into(),
            K::I16 => "int2".into(),
            K::I32 => "int4".into(),
            K::I64 => "int8".into(),
            K::F32 => "float4".into(),
            K::F64 => "float8".into(),
            K::Text => "text".into(),
            K::Bytea => "bytea".into(),
            K::Uuid => "uuid".into(),
            K::Date => "date".into(),
            K::Ts => "timestamp".into(),
            K::Tstz => "timestamptz".into(),
            K::Num(p, s) => format!("numeric({p},{s})"),
            K::Iv => "interval".into(),
            K::Time => "time".into(),
        }
    }

    /// SQL expressions for the edge cases, written the way a user would. NULL is added for every type.
    fn literals(self) -> Vec<String> {
        let l: Vec<&str> = match self {
            K::Bool => vec!["true", "false"],
            K::I16 => vec!["0", "1", "-1", "32767", "-32768"],
            K::I32 => vec!["0", "1", "-1", "2147483647", "-2147483648"],
            K::I64 => vec![
                "0",
                "1",
                "-1",
                "9223372036854775807",
                "-9223372036854775808",
            ],
            K::F32 => vec![
                "0",
                "'-0'",
                "1",
                "-1",
                "'NaN'",
                "'Infinity'",
                "'-Infinity'",
                "1e-45",
                "3.4028235e38",
                "-3.4028235e38",
                "1.17549435e-38",
                "0.1",
                "16777217",
                "123456.789",
            ],
            K::F64 => vec![
                "0",
                "'-0'",
                "1",
                "-1",
                "'NaN'",
                "'Infinity'",
                "'-Infinity'",
                "5e-324",
                "1.7976931348623157e308",
                "-1.7976931348623157e308",
                "2.2250738585072014e-308",
                "0.1",
                "9007199254740993",
                "123456.789012345678",
            ],
            K::Text => vec![
                "''",
                "'a'",
                "'é日🎉'",
                "repeat('x', 10000)",
                "'quote''s \"q\" \\ back'",
                "'NULL'",
                "' lead and trail '",
                "repeat('日', 50000)",
            ],
            K::Bytea => vec![
                "'\\x'::bytea",
                "'\\x00'::bytea",
                "'\\x00ff00'::bytea",
                "decode(repeat('ab', 100000), 'hex')",
            ],
            K::Uuid => vec![
                "'00000000-0000-0000-0000-000000000000'",
                "'ffffffff-ffff-ffff-ffff-ffffffffffff'",
                "'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'",
            ],
            K::Date => vec![
                "'4714-11-24 BC'",
                "'0001-01-01'",
                "'0001-12-31 BC'",
                "'1970-01-01'",
                "'1999-12-31'",
                "'2000-01-01'",
                "'2000-02-29'",
                "'2100-12-31'",
                "'5874897-12-31'",
                "'infinity'",
                "'-infinity'",
            ],
            K::Ts => vec![
                "'4714-11-24 00:00:00 BC'",
                "'0001-01-01 00:00:00'",
                "'1970-01-01 00:00:00'",
                "'1999-12-31 23:59:59.999999'",
                "'2000-01-01 00:00:00'",
                "'2000-01-01 00:00:00.000001'",
                "'2024-02-29 12:34:56.789012'",
                "'infinity'",
                "'-infinity'",
            ],
            K::Tstz => vec![
                "('4714-11-24 00:00:00 BC'::timestamp AT TIME ZONE 'UTC')",
                "'1970-01-01 00:00:00+00'",
                "'1999-12-31 23:59:59.999999+00'",
                "'2000-01-01 00:00:00+00'",
                "'2024-03-10 01:59:59-05'",
                "'2024-03-10 03:00:00-04'", // across the spring DST change
                "'2024-11-03 01:30:00-04'",
                "'2024-11-03 01:30:00-05'", // the repeated hour
                "'2024-11-03 06:30:00+00'",
                "'infinity'",
                "'-infinity'",
                "'2024-06-01 12:00:00+14'",
                "'2024-06-01 12:00:00-12'",
            ],
            K::Num(p, s) => {
                let mut v = vec!["0".to_string()];
                if p > s {
                    // one does not fit numeric(p,p)
                    v.push("1".into());
                    v.push("-1".into());
                }
                let nines = "9".repeat(usize::from(p));
                let max = if s == 0 {
                    nines.clone()
                } else {
                    let (i, f) = nines.split_at(usize::from(p - s));
                    format!("{}.{f}", if i.is_empty() { "0" } else { i })
                };
                v.push(max.clone());
                v.push(format!("-{max}"));
                if s > 0 {
                    v.push(format!("0.{}1", "0".repeat(usize::from(s - 1))));
                    v.push(format!("-0.{}1", "0".repeat(usize::from(s - 1))));
                }
                return with_null(v);
            }
            K::Iv => vec![
                "'0'",
                "'1 microsecond'",
                "'-1 microsecond'",
                "'1 day'",
                "'-1 day'",
                "'1 month'",
                "'13 months'",
                "'100 hours'",
                "'1 year 2 mons 3 days 04:05:06.789'",
                "'-1 year -2 mons +3 days -04:05:06.5'",
                "'2147483647 months'",
                "'-2147483648 months'",
                "'2147483647 days'",
                "'2147483647 hours'",
                "'-2147483647 hours'",
                "'00:00:00.5'",
                "'-00:00:00.5'",
            ],
            K::Time => vec![
                "'00:00:00'",
                "'00:00:00.000001'",
                "'12:00:00'",
                "'23:59:59.999999'",
                "'24:00:00'",
            ],
        };
        with_null(l.into_iter().map(String::from).collect())
    }
}

fn with_null(mut v: Vec<String>) -> Vec<String> {
    v.push("NULL".into());
    v
}

/// A value read from a column, owned so it can be sent back as a parameter. Floats are kept as bits.
#[derive(Debug, Clone, PartialEq)]
enum O {
    Null,
    Bool(bool),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
    Text(String),
    Bytes(Vec<u8>),
    Uuid([u8; 16]),
    Date(Date),
    Ts(Timestamp),
    Tstz(Timestamp),
    Num(Decimal128),
    Iv(Interval),
    Time(Time),
}

impl O {
    fn value(&self) -> Value<'_> {
        match self {
            O::Null => Value::Null,
            O::Bool(v) => Value::Bool(*v),
            O::I16(v) => Value::I16(*v),
            O::I32(v) => Value::I32(*v),
            O::I64(v) => Value::I64(*v),
            O::F32(v) => Value::F32(f32::from_bits(*v)),
            O::F64(v) => Value::F64(f64::from_bits(*v)),
            O::Text(v) => Value::Text(v),
            O::Bytes(v) => Value::Bytes(v),
            O::Uuid(v) => Value::Uuid(*v),
            O::Date(v) => Value::Date(*v),
            O::Ts(v) => Value::Timestamp(*v),
            O::Tstz(v) => Value::TimestampTz(*v),
            O::Num(v) => Value::Numeric(*v),
            O::Iv(v) => Value::Interval(*v),
            O::Time(v) => Value::Time(*v),
        }
    }

    /// Equality that treats every NaN as equal to every other NaN (PostgreSQL does not keep the
    /// payload) and compares the other floats by bit pattern, so `-0` differs from `0`.
    fn same(&self, other: &O) -> bool {
        match (self, other) {
            (O::F32(a), O::F32(b)) => {
                (f32::from_bits(*a).is_nan() && f32::from_bits(*b).is_nan()) || a == b
            }
            (O::F64(a), O::F64(b)) => {
                (f64::from_bits(*a).is_nan() && f64::from_bits(*b).is_nan()) || a == b
            }
            _ => self == other,
        }
    }
}

fn read(r: &RowRef<'_>, i: usize, k: K) -> Result<O, Error> {
    if r.is_null(i)? {
        return Ok(O::Null);
    }
    Ok(match k {
        K::Bool => O::Bool(r.get(i)?),
        K::I16 => O::I16(r.get(i)?),
        K::I32 => O::I32(r.get(i)?),
        K::I64 => O::I64(r.get(i)?),
        K::F32 => O::F32(r.get::<f32>(i)?.to_bits()),
        K::F64 => O::F64(r.get::<f64>(i)?.to_bits()),
        K::Text => O::Text(r.get(i)?),
        K::Bytea => O::Bytes(r.get(i)?),
        K::Uuid => O::Uuid(r.get(i)?),
        K::Date => O::Date(r.get(i)?),
        K::Ts => O::Ts(r.get(i)?),
        K::Tstz => O::Tstz(r.get::<Timestamp>(i)?),
        K::Num(..) => O::Num(r.get(i)?),
        K::Iv => O::Iv(r.get(i)?),
        K::Time => O::Time(r.get(i)?),
    })
}

fn random_value(k: K, rng: &mut SplitMix64) -> O {
    if rng.chance(1, 12) {
        return O::Null;
    }
    match k {
        K::Bool => O::Bool(rng.chance(1, 2)),
        K::I16 => O::I16(rng.next_u32() as i16),
        K::I32 => O::I32(rng.next_u32() as i32),
        K::I64 => O::I64(rng.next_u64() as i64),
        K::F32 => O::F32(rng.next_u32()),
        K::F64 => O::F64(rng.next_u64()),
        K::Text => O::Text(rng.string(40)),
        K::Bytea => {
            let n = rng.range(0, 40);
            O::Bytes(rng.bytes(n))
        }
        K::Uuid => {
            let mut u = [0u8; 16];
            u.copy_from_slice(&rng.bytes(16));
            O::Uuid(u)
        }
        // unix days within years about -1200 to 5100: every one is a valid PostgreSQL date
        K::Date => O::Date(Date(rng.range(0, 2_300_000) as i32 - 1_300_000)),
        K::Ts => O::Ts(Timestamp(random_micros(rng))),
        K::Tstz => O::Tstz(Timestamp(random_micros(rng))),
        K::Num(p, s) => {
            let mag = (u128::from(rng.next_u64()) << 64 | u128::from(rng.next_u64()))
                % 10u128.pow(u32::from(rng.range(1, usize::from(p)) as u8));
            let u = mag as i128;
            O::Num(Decimal128 {
                unscaled: if rng.chance(1, 2) { -u } else { u },
                scale: s,
            })
        }
        K::Iv => O::Iv(Interval {
            months: rng.next_u32() as i32,
            days: rng.next_u32() as i32,
            micros: (rng.next_u64() as i64) % 1_000_000_000_000_000,
        }),
        K::Time => O::Time(Time(rng.below(86_400_000_001) as i64)),
    }
}

/// Unix microseconds within years about -1200 to 5100.
fn random_micros(rng: &mut SplitMix64) -> i64 {
    (rng.below(200_000_000_000_000_000) as i64) - 100_000_000_000_000_000
}

/// A cell as text, for the columns we format ourselves (decimal, interval, time).
fn cell_text(r: &RowRef<'_>, i: usize, k: K) -> Result<Option<String>, Error> {
    if r.is_null(i)? {
        return Ok(None);
    }
    Ok(Some(match k {
        K::Num(..) => r.get::<Decimal128>(i)?.to_string(),
        K::Iv => r.get::<Interval>(i)?.to_string(),
        K::Time => r.get::<Time>(i)?.to_string(),
        _ => unreachable!("no formatter for {k:?}"),
    }))
}

fn has_display(k: K) -> bool {
    matches!(k, K::Num(..) | K::Iv | K::Time)
}

const KINDS: &[K] = &[
    K::Bool,
    K::I16,
    K::I32,
    K::I64,
    K::F32,
    K::F64,
    K::Text,
    K::Bytea,
    K::Uuid,
    K::Date,
    K::Ts,
    K::Tstz,
    K::Num(10, 2),
    K::Num(38, 0),
    K::Num(38, 38),
    K::Num(18, 6),
    K::Num(1, 0),
    K::Iv,
    K::Time,
];

/// Edge values: read in binary, sent back in binary and in text, judged by the server.
#[test]
fn edge_values_survive_a_round_trip_in_both_formats() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    for &k in KINDS {
        let ty = k.sql();
        c.execute(
            &format!("CREATE TEMP TABLE d (id int PRIMARY KEY, v {ty})"),
            &[],
        )?;
        for (i, lit) in k.literals().iter().enumerate() {
            c.execute(&format!("INSERT INTO d VALUES ({i}, ({lit})::{ty})"), &[])?;
        }
        let n = k.literals().len();
        let mut rows = Vec::new();
        let mut cur = c.query("SELECT id, v, v::text FROM d ORDER BY id", &[])?;
        while let Some(r) = cur.next()? {
            let id = r.get::<i32>(0)?;
            rows.push((
                id,
                read(&r, 1, k)?,
                r.get::<Option<String>>(2)?,
                if has_display(k) {
                    cell_text(&r, 1, k)?
                } else {
                    None
                },
            ));
        }
        drop(cur);
        assert_eq!(rows.len(), n, "{ty}");
        for (id, owned, server_text, shown) in &rows {
            let what = format!("{ty} #{id} {server_text:?}");
            if has_display(k) {
                assert_eq!(
                    shown, server_text,
                    "Display differs from the server: {what}"
                );
            }
            // binary parameter -> the server prints the same text and finds the value equal
            let probe = format!("SELECT ($1::{ty})::text, ($1::{ty}) IS NOT DISTINCT FROM (SELECT v FROM d WHERE id = $2)");
            let mut cur = c.query(&probe, &[owned.value(), Value::I32(*id)])?;
            let r = cur.next()?.unwrap();
            assert_eq!(
                &r.get::<Option<String>>(0)?,
                server_text,
                "binary parameter: {what}"
            );
            assert!(r.get::<bool>(1)?, "not distinct: {what}");
            drop(cur);
            // text-format parameter (the server's own text) gives the same answer
            if let Some(t) = server_text {
                let t = t.clone();
                let mut cur = c.query(&probe, &[Value::Unknown(&t), Value::I32(*id)])?;
                let r = cur.next()?.unwrap();
                assert_eq!(
                    &r.get::<Option<String>>(0)?,
                    server_text,
                    "text parameter: {what}"
                );
                assert!(r.get::<bool>(1)?, "text parameter not distinct: {what}");
            }
        }
        c.execute("DROP TABLE d", &[])?;
    }
    Ok(())
}

/// Random values: written as binary parameters, read back in binary, identical.
#[test]
fn random_values_round_trip_through_the_server_unchanged() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let rows: usize = std::env::var("KAREZ_DIFF_ROWS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let seed: u64 = std::env::var("KAREZ_DIFF_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x5EED_0004);
    eprintln!("random values: {rows} per type, seed {seed}");
    let mut rng = SplitMix64::new(seed);
    for &k in KINDS {
        let ty = k.sql();
        c.execute(
            &format!("CREATE TEMP TABLE d (id int PRIMARY KEY, v {ty})"),
            &[],
        )?;
        let want: Vec<O> = (0..rows).map(|_| random_value(k, &mut rng)).collect();
        let ids: Vec<i32> = (0..rows as i32).collect();
        let params: Vec<[Value; 2]> = want
            .iter()
            .zip(&ids)
            .map(|(o, id)| [Value::I32(*id), o.value()])
            .collect();
        let sql = format!("INSERT INTO d VALUES ($1, $2::{ty})");
        let items: Vec<(&str, &[Value])> = params.iter().map(|p| (sql.as_str(), &p[..])).collect();
        c.execute_many(&items)?;
        let mut cur = c.query("SELECT id, v, v::text FROM d ORDER BY id", &[])?;
        let mut n = 0;
        while let Some(r) = cur.next()? {
            let id = r.get::<i32>(0)? as usize;
            let got = read(&r, 1, k)?;
            assert!(
                got.same(&want[id]),
                "{ty} #{id}: wrote {:?}, read {got:?}",
                want[id]
            );
            if has_display(k) {
                assert_eq!(
                    cell_text(&r, 1, k)?,
                    r.get::<Option<String>>(2)?,
                    "{ty} #{id}: Display differs from the server"
                );
            }
            n += 1;
        }
        assert_eq!(n, rows, "{ty}");
        drop(cur);
        c.execute("DROP TABLE d", &[])?;
    }
    Ok(())
}

/// The native parameter must be what the server expects for a prepared statement's own type too:
/// a statement prepared against a column type accepts the matching `Value` without a cast.
#[test]
fn parameters_match_the_types_the_server_infers() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute(
        "CREATE TEMP TABLE p (b bool, i2 int2, i4 int4, i8 int8, f4 float4, f8 float8, t text, y bytea, \
         u uuid, d date, ts timestamp, tz timestamptz, n numeric, iv interval, tm time)",
        &[],
    )?;
    let n = Decimal128 {
        unscaled: -12345,
        scale: 3,
    };
    let vals = [
        Value::Bool(true),
        Value::I16(-2),
        Value::I32(-4),
        Value::I64(-8),
        Value::F32(1.5),
        Value::F64(-2.5),
        Value::Text("t"),
        Value::Bytes(&[1, 2]),
        Value::Uuid([9; 16]),
        Value::Date(Date(0)),
        Value::Timestamp(Timestamp(1)),
        Value::TimestampTz(Timestamp(2)),
        Value::Numeric(n),
        Value::Interval(Interval {
            months: 1,
            days: 2,
            micros: 3,
        }),
        Value::Time(Time(4)),
    ];
    // prepared once against the table's own types: every oid must match what the server inferred
    let stmt = c.prepare(
        "INSERT INTO p VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
        &[],
    )?;
    c.execute_prepared(&stmt, &vals)?;
    let mut cur = c.query("SELECT n::text, iv::text, tm::text, f4, f8 FROM p", &[])?;
    let r = cur.next()?.unwrap();
    assert_eq!(r.get::<&str>(0)?, "-12.345");
    assert_eq!(r.get::<&str>(1)?, "P1M2DT0.000003S");
    assert_eq!(r.get::<&str>(2)?, "00:00:00.000004");
    assert_eq!(r.get::<f32>(3)?, 1.5);
    assert_eq!(r.get::<f64>(4)?, -2.5);
    Ok(())
}

/// Random one-dimensional arrays with NULLs: stored, read back as text, and found equal.
#[test]
fn random_arrays_round_trip_and_match_the_servers_text() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let mut rng = SplitMix64::new(0xA77A_0004);
    c.execute(
        "CREATE TEMP TABLE a (id int PRIMARY KEY, b bool[], i2 int2[], i4 int4[], i8 int8[], f4 float4[], \
         f8 float8[], t text[])",
        &[],
    )?;
    for id in 0..200i32 {
        let len = |rng: &mut SplitMix64| {
            if rng.chance(1, 10) {
                0
            } else {
                rng.range(1, 12)
            }
        };
        let opt = |rng: &mut SplitMix64| rng.chance(1, 6);
        let n = len(&mut rng);
        let b: Vec<Option<bool>> = (0..n)
            .map(|_| (!opt(&mut rng)).then(|| rng.chance(1, 2)))
            .collect();
        let n = len(&mut rng);
        let i2: Vec<Option<i16>> = (0..n)
            .map(|_| (!opt(&mut rng)).then(|| rng.next_u32() as i16))
            .collect();
        let n = len(&mut rng);
        let i4: Vec<Option<i32>> = (0..n)
            .map(|_| (!opt(&mut rng)).then(|| rng.next_u32() as i32))
            .collect();
        let n = len(&mut rng);
        let i8_: Vec<Option<i64>> = (0..n)
            .map(|_| (!opt(&mut rng)).then(|| rng.next_u64() as i64))
            .collect();
        let n = len(&mut rng);
        let f4: Vec<Option<f32>> = (0..n)
            .map(|_| (!opt(&mut rng)).then(|| f32::from_bits(rng.next_u32())))
            .collect();
        let n = len(&mut rng);
        let f8: Vec<Option<f64>> = (0..n)
            .map(|_| (!opt(&mut rng)).then(|| f64::from_bits(rng.next_u64())))
            .collect();
        let n = len(&mut rng);
        let strs: Vec<Option<String>> = (0..n)
            .map(|_| (!opt(&mut rng)).then(|| rng.string(8)))
            .collect();
        let t: Vec<Option<&str>> = strs.iter().map(|s| s.as_deref()).collect();
        let arrays = [
            Value::Array(Array::Bool(&b)),
            Value::Array(Array::I16(&i2)),
            Value::Array(Array::I32(&i4)),
            Value::Array(Array::I64(&i8_)),
            Value::Array(Array::F32(&f4)),
            Value::Array(Array::F64(&f8)),
            Value::Array(Array::Text(&t)),
        ];
        let mut params = vec![Value::I32(id)];
        params.extend_from_slice(&arrays);
        c.execute("INSERT INTO a VALUES ($1,$2,$3,$4,$5,$6,$7,$8)", &params)?;
        // the server finds every stored array equal to the parameter that made it
        for (col, v) in ["b", "i2", "i4", "i8", "f4", "f8", "t"].iter().zip(&arrays) {
            let sql = format!("SELECT {col} IS NOT DISTINCT FROM $1 FROM a WHERE id = $2");
            let mut cur = c.query(&sql, &[*v, Value::I32(id)])?;
            assert!(
                cur.next()?.unwrap().get::<bool>(0)?,
                "array #{id} column {col}"
            );
        }
    }
    // read back as text (arrays are rendered): equal to the server's own text. Float arrays are
    // excluded: Rust and PostgreSQL print an exponent differently, so they are checked by the
    // IS NOT DISTINCT FROM above and by value in the integer-valued rows below.
    let mut cur = c.query(
        "SELECT b, b::text, i2, i2::text, i4, i4::text, i8, i8::text, t, t::text FROM a ORDER BY id",
        &[],
    )?;
    let mut rows = 0;
    while let Some(r) = cur.next()? {
        for k in (0..10).step_by(2) {
            assert_eq!(
                r.get::<Option<&str>>(k)?,
                r.get::<Option<&str>>(k + 1)?,
                "row {rows} column {k}"
            );
        }
        rows += 1;
    }
    assert_eq!(rows, 200);
    drop(cur);
    Ok(())
}

/// PostgreSQL's largest timestamps (about year 294247 on) do not fit a `Timestamp`, which counts
/// microseconds from 1970 in an `i64` like Arrow's `timestamp[us]`. That is a value error, not a
/// broken connection, and the server's text form is still reachable with `::text`.
#[test]
fn timestamps_beyond_the_unix_epoch_range_are_a_value_error() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    for sql in [
        "SELECT '294276-12-31 23:59:59.999999'::timestamp",
        "SELECT '294276-12-31 23:59:59.999999+00'::timestamptz",
    ] {
        let e = {
            let mut cur = c.query(sql, &[])?;
            loop {
                match cur.next() {
                    Ok(Some(_)) => continue,
                    Ok(None) => panic!("{sql}: expected an error"),
                    Err(e) => break e,
                }
            }
        };
        assert!(matches!(e, Error::Value(_)), "{sql}: {e:?}");
        assert!(!c.is_broken(), "{sql}");
    }
    let mut cur = c.query(
        "SELECT '294276-12-31 23:59:59.999999'::timestamp::text",
        &[],
    )?;
    assert_eq!(
        cur.next()?.unwrap().get::<&str>(0)?,
        "294276-12-31 23:59:59.999999"
    );
    Ok(())
}

// ---- the types that are read as rendered text ------------------------------------------------
//
// Random text goes in as a text-format parameter (the server parses it), is read back in the binary
// format and rendered by us, and must equal the server's own text of the same stored value.

type Gen = fn(&mut SplitMix64) -> String;

fn pick<'a>(rng: &mut SplitMix64, items: &[&'a str]) -> &'a str {
    items[rng.range(0, items.len() - 1)]
}

fn hex_bytes(rng: &mut SplitMix64, n: usize, sep: &str) -> String {
    rng.bytes(n)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(sep)
}

/// 8 groups with runs of zeros, so the IPv6 compression rules are exercised.
fn v6_groups(rng: &mut SplitMix64) -> [u16; 8] {
    let mut g = [0u16; 8];
    for x in g.iter_mut() {
        *x = match rng.below(4) {
            0 | 1 => 0,
            2 => rng.below(16) as u16,
            _ => rng.next_u32() as u16,
        };
    }
    if rng.chance(1, 5) {
        // an IPv4-mapped or IPv4-compatible shape
        g[..5].fill(0);
        g[5] = if rng.chance(1, 2) { 0xffff } else { 0 };
    }
    g
}

fn v6_text(g: &[u16; 8]) -> String {
    g.iter()
        .map(|x| format!("{x:x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn gen_inet(rng: &mut SplitMix64) -> String {
    if rng.chance(1, 2) {
        let a = rng.bytes(4);
        let p = rng.range(0, 32);
        let base = format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3]);
        if p == 32 && rng.chance(1, 2) {
            base
        } else {
            format!("{base}/{p}")
        }
    } else {
        let g = v6_groups(rng);
        let p = rng.range(0, 128);
        if p == 128 && rng.chance(1, 2) {
            v6_text(&g)
        } else {
            format!("{}/{p}", v6_text(&g))
        }
    }
}

fn gen_cidr(rng: &mut SplitMix64) -> String {
    if rng.chance(1, 2) {
        let p = rng.range(0, 32);
        let b = rng.bytes(4);
        let mut v = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        v = if p == 0 {
            0
        } else {
            v & (u32::MAX << (32 - p))
        };
        let a = v.to_be_bytes();
        format!("{}.{}.{}.{}/{p}", a[0], a[1], a[2], a[3])
    } else {
        let p = rng.range(0, 128);
        let g = v6_groups(rng);
        let mut v = g.iter().fold(0u128, |a, x| a << 16 | u128::from(*x));
        v = if p == 0 {
            0
        } else {
            v & (u128::MAX << (128 - p))
        };
        let mut out = [0u16; 8];
        for (i, o) in out.iter_mut().enumerate() {
            *o = (v >> (112 - 16 * i)) as u16;
        }
        format!("{}/{p}", v6_text(&out))
    }
}

fn gen_bits(rng: &mut SplitMix64, n: usize) -> String {
    (0..n)
        .map(|_| if rng.chance(1, 2) { '1' } else { '0' })
        .collect()
}

fn gen_json_value(rng: &mut SplitMix64, depth: usize) -> String {
    let sp = if rng.chance(1, 2) { " " } else { "" };
    match rng.below(if depth == 0 { 6 } else { 8 }) {
        0 => "null".into(),
        1 => "true".into(),
        2 => "false".into(),
        3 => format!("{}", rng.next_u64() as i64 / 1000),
        4 => format!(
            "{}{}.{}e{}",
            if rng.chance(1, 2) { "-" } else { "" },
            rng.below(1000),
            rng.below(100000),
            rng.range(0, 40) as i32 - 20
        ),
        5 => {
            let s = rng.string(8).replace(['"', '\\'], "x");
            let esc = pick(rng, &["", "\\n", "\\\"", "\\\\", "\\u00e9", "\\t", "/"]);
            format!("\"{s}{esc}\"")
        }
        6 => {
            let n = rng.range(0, 4);
            let items: Vec<String> = (0..n).map(|_| gen_json_value(rng, depth - 1)).collect();
            format!("[{sp}{}{sp}]", items.join(&format!(",{sp}")))
        }
        _ => {
            let n = rng.range(0, 4);
            let items: Vec<String> = (0..n)
                .map(|i| {
                    let k = format!("k{i}{}", rng.string(3).replace(['"', '\\'], "y"));
                    format!("\"{k}\"{sp}:{sp}{}", gen_json_value(rng, depth - 1))
                })
                .collect();
            format!("{{{sp}{}{sp}}}", items.join(&format!(",{sp}")))
        }
    }
}

fn gen_json(rng: &mut SplitMix64) -> String {
    gen_json_value(rng, 3)
}

fn gen_xml(rng: &mut SplitMix64) -> String {
    fn el(rng: &mut SplitMix64, depth: usize) -> String {
        let name = pick(rng, &["a", "item", "x1", "row"]);
        let attr = if rng.chance(1, 3) {
            format!(" id=\"{}\"", rng.below(1000))
        } else {
            String::new()
        };
        let text = rng
            .string(6)
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let kids = if depth > 0 && rng.chance(1, 2) {
            (0..rng.range(1, 3))
                .map(|_| el(rng, depth - 1))
                .collect::<String>()
        } else {
            String::new()
        };
        format!("<{name}{attr}>{text}{kids}</{name}>")
    }
    el(rng, 2)
}

fn gen_timetz(rng: &mut SplitMix64) -> String {
    let off_m = pick(rng, &["00", "30", "45", "15"]);
    let frac = if rng.chance(1, 2) {
        format!(".{:06}", rng.below(1_000_000))
    } else {
        String::new()
    };
    format!(
        "{:02}:{:02}:{:02}{frac}{}{:02}:{off_m}",
        rng.below(24),
        rng.below(60),
        rng.below(60),
        if rng.chance(1, 2) { "+" } else { "-" },
        rng.below(16)
    )
}

fn gen_time(rng: &mut SplitMix64) -> String {
    if rng.chance(1, 20) {
        return "24:00:00".into();
    }
    format!(
        "{:02}:{:02}:{:02}.{:06}",
        rng.below(24),
        rng.below(60),
        rng.below(60),
        rng.below(1_000_000)
    )
}

/// `numeric` has infinities from PostgreSQL 14; the generator avoids them on older servers.
static INFINITY_OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn gen_numeric(rng: &mut SplitMix64) -> String {
    match rng.below(20) {
        0 => return "NaN".into(),
        1 if INFINITY_OK.load(std::sync::atomic::Ordering::Relaxed) => {
            return pick(rng, &["Infinity", "-Infinity"]).into()
        }
        _ => {}
    }
    let int = rng.range(0, 28);
    let frac = rng.range(0, 20);
    let digits = |rng: &mut SplitMix64, n: usize| -> String {
        (0..n)
            .map(|_| char::from(b'0' + rng.below(10) as u8))
            .collect()
    };
    let i = if int == 0 {
        "0".to_string()
    } else {
        digits(rng, int)
    };
    let f = if frac == 0 {
        String::new()
    } else {
        format!(".{}", digits(rng, frac))
    };
    format!("{}{i}{f}", if rng.chance(1, 2) { "-" } else { "" })
}

fn gen_interval(rng: &mut SplitMix64) -> String {
    let sign = |rng: &mut SplitMix64| if rng.chance(1, 2) { "-" } else { "+" };
    format!(
        "{}{} years {}{} mons {}{} days {}{}:{:02}:{:02}.{:06}",
        sign(rng),
        rng.below(2000),
        sign(rng),
        rng.below(24),
        sign(rng),
        rng.below(1000),
        sign(rng),
        rng.below(5000),
        rng.below(60),
        rng.below(60),
        rng.below(1_000_000)
    )
}

fn gen_text(rng: &mut SplitMix64) -> String {
    if rng.chance(1, 3) {
        pick(
            rng,
            &[
                "",
                "NULL",
                "null",
                "a,b",
                "{x}",
                "q\"uote",
                "back\\slash",
                " lead",
                "trail ",
                "tab\t",
                "{",
                "}",
                "\"",
                "\\",
            ],
        )
        .into()
    } else {
        rng.string(10)
    }
}

fn gen_uuid(rng: &mut SplitMix64) -> String {
    let h = hex_bytes(rng, 16, "");
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

/// A random array literal built from an element generator: NULLs, one or two dimensions, now and
/// then a lower bound other than 1. Every element is quoted, so any text survives.
fn gen_array(rng: &mut SplitMix64, elem: Gen) -> String {
    let quote = |s: String| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let item = |rng: &mut SplitMix64| {
        if rng.chance(1, 6) {
            "NULL".to_string()
        } else {
            quote(elem(rng))
        }
    };
    if rng.chance(1, 12) {
        return "{}".into();
    }
    if rng.chance(1, 4) {
        let (r, c) = (rng.range(1, 3), rng.range(1, 3));
        let rows: Vec<String> = (0..r)
            .map(|_| {
                format!(
                    "{{{}}}",
                    (0..c).map(|_| item(rng)).collect::<Vec<_>>().join(",")
                )
            })
            .collect();
        format!("{{{}}}", rows.join(","))
    } else {
        let n = rng.range(1, 6);
        let body = format!(
            "{{{}}}",
            (0..n).map(|_| item(rng)).collect::<Vec<_>>().join(",")
        );
        if rng.chance(1, 5) {
            let lo = rng.range(0, 5) as i32 - 2;
            format!("[{lo}:{}]={body}", lo + n as i32 - 1)
        } else {
            body
        }
    }
}

#[test]
fn random_values_of_the_rendered_types_match_the_servers_text() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let rows: usize = std::env::var("KAREZ_DIFF_ROWS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let seed: u64 = std::env::var("KAREZ_DIFF_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x5EED_0104);
    let mut rng = SplitMix64::new(seed);
    let major: u32 = c
        .server_param("server_version")
        .and_then(|v| v.split(['.', ' ']).next())
        .and_then(|v| v.parse().ok())
        .unwrap_or(14);
    INFINITY_OK.store(major >= 14, std::sync::atomic::Ordering::Relaxed);
    let gen_bit13: Gen = |r| gen_bits(r, 13);
    let gen_varbit: Gen = |r| {
        let n = r.range(0, 70);
        gen_bits(r, n)
    };
    let gen_mac: Gen = |r| hex_bytes(r, 6, ":");
    let gen_mac8: Gen = |r| hex_bytes(r, 8, ":");
    let gen_bytea: Gen = |r| {
        let n = r.range(0, 10);
        format!("\\x{}", hex_bytes(r, n, ""))
    };
    let gen_mood: Gen = |r| pick(r, &["sad", "ok", "happy"]).into();
    let gen_i2: Gen = |r| (r.next_u32() as i16).to_string();
    let gen_i4: Gen = |r| (r.next_u32() as i32).to_string();
    let gen_i8: Gen = |r| (r.next_u64() as i64).to_string();
    let gen_bool: Gen = |r| {
        if r.chance(1, 2) {
            "t".into()
        } else {
            "f".into()
        }
    };
    // (column type, generator, the server's text of the stored value)
    let scalars: Vec<(&str, Gen, &str)> = vec![
        ("inet", gen_inet, "abbrev(v)"), // inet::text always adds the netmask; abbrev is its output form
        ("cidr", gen_cidr, "v::text"),
        ("macaddr", gen_mac, "v::text"),
        ("macaddr8", gen_mac8, "v::text"),
        ("bit(13)", gen_bit13, "v::text"),
        ("varbit", gen_varbit, "v::text"),
        ("json", gen_json, "v::text"),
        ("jsonb", gen_json, "v::text"),
        ("xml", gen_xml, "v::text"),
        ("timetz", gen_timetz, "v::text"),
        ("mood", gen_mood, "v::text"),
        ("numeric", gen_numeric, "v::text"), // unconstrained: delivered as text
    ];
    let elems: Vec<(&str, Gen)> = vec![
        ("numeric", gen_numeric),
        ("interval", gen_interval),
        ("time", gen_time),
        ("timetz", gen_timetz),
        ("uuid", gen_uuid),
        ("text", gen_text),
        ("varchar", gen_text),
        ("bytea", gen_bytea),
        ("inet", gen_inet),
        ("cidr", gen_cidr),
        ("macaddr", gen_mac),
        ("bit(13)", gen_bit13),
        ("varbit", gen_varbit),
        ("jsonb", gen_json),
        ("json", gen_json),
        ("xml", gen_xml),
        ("int2", gen_i2),
        ("int4", gen_i4),
        ("int8", gen_i8),
        ("bool", gen_bool),
    ];
    // (name, column type, scalar generator or element generator, server text expression)
    let mut cases: Vec<(String, Gen, bool, &str)> = Vec::new();
    for (ty, g, cmp) in &scalars {
        cases.push((ty.to_string(), *g, false, *cmp));
    }
    for (ty, g) in &elems {
        cases.push((format!("{ty}[]"), *g, true, "v::text"));
    }
    for (ty, g, is_array, cmp) in &cases {
        c.execute(
            &format!("CREATE TEMP TABLE d (id int PRIMARY KEY, v {ty})"),
            &[],
        )?;
        let mut texts = Vec::new();
        for id in 0..rows {
            let t = if *is_array {
                gen_array(&mut rng, *g)
            } else {
                g(&mut rng)
            };
            let null = rng.chance(1, 15);
            let p = [
                Value::I32(id as i32),
                if null {
                    Value::Null
                } else {
                    Value::Unknown(&t)
                },
            ];
            c.execute(&format!("INSERT INTO d VALUES ($1, $2::{ty})"), &p)
                .inspect_err(|_| eprintln!("{ty}: the server refused the generated value {t:?}"))?;
            texts.push(t);
        }
        let mut cur = c.query(&format!("SELECT id, v, {cmp} FROM d ORDER BY id"), &[])?;
        let mut n = 0;
        while let Some(r) = cur.next()? {
            let id = r.get::<i32>(0)? as usize;
            assert_eq!(
                r.get::<Option<&str>>(1)?,
                r.get::<Option<&str>>(2)?,
                "{ty} #{id}: sent {:?}",
                texts[id]
            );
            n += 1;
        }
        assert_eq!(n, rows, "{ty}");
        drop(cur);
        c.execute("DROP TABLE d", &[])?;
    }
    Ok(())
}
