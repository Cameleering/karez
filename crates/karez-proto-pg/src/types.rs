//! Binary value codecs: decoding DataRow fields straight into a chunk, and encoding parameters.
//!
//! Supported result types are in [`oid`]. A column of any other type is requested in text
//! format and stored as `Utf8` (PostgreSQL's own text rendering), because those types have no
//! decoder here yet. That is the only text path.

use crate::backend::FieldDescription;
use crate::error::{ProtoError, Result};
use crate::wire::Reader;
use karez_chunk::{ChunkBuilder, ColumnType, Date, Field, Timestamp};
use std::ops::Range;

/// PostgreSQL type OIDs used by this crate.
pub mod oid {
    /// `bool`
    pub const BOOL: u32 = 16;
    /// `bytea`
    pub const BYTEA: u32 = 17;
    /// `name`
    pub const NAME: u32 = 19;
    /// `int8`
    pub const INT8: u32 = 20;
    /// `int2`
    pub const INT2: u32 = 21;
    /// `int4`
    pub const INT4: u32 = 23;
    /// `text`
    pub const TEXT: u32 = 25;
    /// `float4`
    pub const FLOAT4: u32 = 700;
    /// `float8`
    pub const FLOAT8: u32 = 701;
    /// `bpchar` (`char(n)`)
    pub const BPCHAR: u32 = 1042;
    /// `varchar`
    pub const VARCHAR: u32 = 1043;
    /// `date`
    pub const DATE: u32 = 1082;
    /// `timestamp`
    pub const TIMESTAMP: u32 = 1114;
    /// `timestamptz`
    pub const TIMESTAMPTZ: u32 = 1184;
    /// `uuid`
    pub const UUID: u32 = 2950;
}

/// Days from 1970-01-01 to 2000-01-01, PostgreSQL's epoch.
pub const PG_EPOCH_DAYS: i32 = 10_957;
/// Microseconds from 1970-01-01 to 2000-01-01.
pub const PG_EPOCH_MICROS: i64 = 946_684_800_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
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
    Timestamp,
    TimestampTz,
    /// Unsupported type, fetched in text format.
    TextFallback,
}

impl Kind {
    fn from_oid(oid: u32) -> Kind {
        match oid {
            oid::BOOL => Kind::Bool,
            oid::INT2 => Kind::I16,
            oid::INT4 => Kind::I32,
            oid::INT8 => Kind::I64,
            oid::FLOAT4 => Kind::F32,
            oid::FLOAT8 => Kind::F64,
            oid::TEXT | oid::VARCHAR | oid::BPCHAR | oid::NAME => Kind::Text,
            oid::BYTEA => Kind::Bytea,
            oid::UUID => Kind::Uuid,
            oid::DATE => Kind::Date,
            oid::TIMESTAMP => Kind::Timestamp,
            oid::TIMESTAMPTZ => Kind::TimestampTz,
            _ => Kind::TextFallback,
        }
    }

    fn column_type(self) -> ColumnType {
        match self {
            Kind::Bool => ColumnType::Bool,
            Kind::I16 => ColumnType::Int16,
            Kind::I32 => ColumnType::Int32,
            Kind::I64 => ColumnType::Int64,
            Kind::F32 => ColumnType::Float32,
            Kind::F64 => ColumnType::Float64,
            Kind::Text | Kind::TextFallback => ColumnType::Utf8,
            Kind::Bytea => ColumnType::Binary,
            Kind::Uuid => ColumnType::Uuid,
            Kind::Date => ColumnType::Date32,
            Kind::Timestamp => ColumnType::TimestampMicros { tz: false },
            Kind::TimestampTz => ColumnType::TimestampMicros { tz: true },
        }
    }
}

fn bad(oid: u32, what: &'static str) -> ProtoError {
    ProtoError::BadValue { oid, what }
}

fn fixed<const N: usize>(v: &[u8], oid: u32) -> Result<[u8; N]> {
    <[u8; N]>::try_from(v).map_err(|_| bad(oid, "wrong length"))
}

/// How to request and decode the columns of one result: built once per prepared statement from
/// its RowDescription.
#[derive(Debug, Clone)]
pub struct ResultPlan {
    kinds: Vec<Kind>,
    formats: Vec<i16>,
    fields: Vec<Field>,
}

impl ResultPlan {
    /// Build a plan from a RowDescription.
    pub fn from_description(desc: &[FieldDescription]) -> Self {
        let kinds: Vec<Kind> = desc.iter().map(|d| Kind::from_oid(d.type_oid)).collect();
        let formats = kinds
            .iter()
            .map(|k| if *k == Kind::TextFallback { 0 } else { 1 })
            .collect();
        let fields = desc
            .iter()
            .zip(&kinds)
            .map(|(d, k)| Field::new(d.name.clone(), k.column_type()))
            .collect();
        ResultPlan {
            kinds,
            formats,
            fields,
        }
    }

    /// A plan for a statement that returns no columns.
    pub fn empty() -> Self {
        ResultPlan {
            kinds: Vec::new(),
            formats: Vec::new(),
            fields: Vec::new(),
        }
    }

    /// The chunk schema this plan produces.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Result format codes to put in Bind (1 binary, 0 text for unsupported types).
    pub fn result_formats(&self) -> &[i16] {
        &self.formats
    }

    /// Number of columns.
    pub fn column_count(&self) -> usize {
        self.kinds.len()
    }

    /// Decode one DataRow body into the builder and finish the row. On any error the partial
    /// row is discarded and the builder is left consistent.
    pub fn decode_row(&self, body: &[u8], out: &mut ChunkBuilder) -> Result<()> {
        let r = self.decode_row_inner(body, out);
        if r.is_err() {
            out.abort_row();
        }
        r
    }

    fn decode_row_inner(&self, body: &[u8], out: &mut ChunkBuilder) -> Result<()> {
        let mut r = Reader::new(body, "DataRow");
        let n = usize::from(r.u16()?);
        if n != self.kinds.len() {
            return Err(ProtoError::RowShape("column count"));
        }
        for (i, kind) in self.kinds.iter().enumerate() {
            let len = r.i32()?;
            let col = out.column_mut(i)?;
            if len == -1 {
                col.push_null()?;
                continue;
            }
            let len = usize::try_from(len).map_err(|_| ProtoError::InvalidValue("field length"))?;
            let v = r.take(len)?;
            match kind {
                Kind::Bool => match v {
                    [0] => col.push_bool(false)?,
                    [1] => col.push_bool(true)?,
                    _ => return Err(bad(oid::BOOL, "not 0 or 1")),
                },
                Kind::I16 => col.push_i16(i16::from_be_bytes(fixed(v, oid::INT2)?))?,
                Kind::I32 => col.push_i32(i32::from_be_bytes(fixed(v, oid::INT4)?))?,
                Kind::I64 => col.push_i64(i64::from_be_bytes(fixed(v, oid::INT8)?))?,
                Kind::F32 => col.push_f32(f32::from_be_bytes(fixed(v, oid::FLOAT4)?))?,
                Kind::F64 => col.push_f64(f64::from_be_bytes(fixed(v, oid::FLOAT8)?))?,
                Kind::Text | Kind::TextFallback => col.push_utf8(v)?,
                Kind::Bytea => col.push_bytes(v)?,
                Kind::Uuid => col.push_uuid(fixed(v, oid::UUID)?)?,
                Kind::Date => {
                    let days = i32::from_be_bytes(fixed(v, oid::DATE)?);
                    col.push_date(date_from_pg(days)?)?
                }
                Kind::Timestamp | Kind::TimestampTz => {
                    let us = i64::from_be_bytes(fixed(v, oid::TIMESTAMP)?);
                    col.push_timestamp(timestamp_from_pg(us)?)?
                }
            }
        }
        r.finish()?;
        out.finish_row()?;
        Ok(())
    }
}

/// PostgreSQL days-since-2000 to days-since-1970. `infinity` and `-infinity` map to
/// `i32::MAX` and `i32::MIN`.
pub fn date_from_pg(days: i32) -> Result<Date> {
    match days {
        i32::MAX | i32::MIN => Ok(Date(days)),
        d => d
            .checked_add(PG_EPOCH_DAYS)
            .filter(|x| *x != i32::MAX && *x != i32::MIN)
            .map(Date)
            .ok_or_else(|| bad(oid::DATE, "date out of range")),
    }
}

/// Inverse of [`date_from_pg`].
pub fn date_to_pg(d: Date) -> Result<i32> {
    match d.0 {
        i32::MAX | i32::MIN => Ok(d.0),
        v => v
            .checked_sub(PG_EPOCH_DAYS)
            .filter(|x| *x != i32::MAX && *x != i32::MIN)
            .ok_or_else(|| bad(oid::DATE, "date out of range")),
    }
}

/// PostgreSQL microseconds-since-2000 to microseconds-since-1970, with infinities preserved as
/// `i64::MAX` / `i64::MIN`.
pub fn timestamp_from_pg(us: i64) -> Result<Timestamp> {
    match us {
        i64::MAX | i64::MIN => Ok(Timestamp(us)),
        t => t
            .checked_add(PG_EPOCH_MICROS)
            .filter(|x| *x != i64::MAX && *x != i64::MIN)
            .map(Timestamp)
            .ok_or_else(|| bad(oid::TIMESTAMP, "timestamp out of range")),
    }
}

/// Inverse of [`timestamp_from_pg`].
pub fn timestamp_to_pg(t: Timestamp) -> Result<i64> {
    match t.0 {
        i64::MAX | i64::MIN => Ok(t.0),
        v => v
            .checked_sub(PG_EPOCH_MICROS)
            .filter(|x| *x != i64::MAX && *x != i64::MIN)
            .ok_or_else(|| bad(oid::TIMESTAMP, "timestamp out of range")),
    }
}

/// A parameter value, bound server-side. Nothing here is ever spliced into SQL text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value<'a> {
    /// SQL NULL (type left to the server).
    Null,
    /// `bool`
    Bool(bool),
    /// `int2`
    I16(i16),
    /// `int4`
    I32(i32),
    /// `int8`
    I64(i64),
    /// `float4`
    F32(f32),
    /// `float8`
    F64(f64),
    /// `text`
    Text(&'a str),
    /// `bytea`
    Bytes(&'a [u8]),
    /// `uuid`
    Uuid([u8; 16]),
    /// `date`
    Date(Date),
    /// `timestamp`
    Timestamp(Timestamp),
    /// `timestamptz`
    TimestampTz(Timestamp),
    /// A text-format literal whose type the server infers from context (for types without a
    /// codec here, such as `numeric`). It is still sent as a bound value.
    Unknown(&'a str),
}

impl<'a> Value<'a> {
    /// `Value::Bool`
    pub fn bool(v: bool) -> Self {
        Value::Bool(v)
    }
    /// `Value::I16`
    pub fn i16(v: i16) -> Self {
        Value::I16(v)
    }
    /// `Value::I32`
    pub fn i32(v: i32) -> Self {
        Value::I32(v)
    }
    /// `Value::I64`
    pub fn i64(v: i64) -> Self {
        Value::I64(v)
    }
    /// `Value::F32`
    pub fn f32(v: f32) -> Self {
        Value::F32(v)
    }
    /// `Value::F64`
    pub fn f64(v: f64) -> Self {
        Value::F64(v)
    }
    /// `Value::Text`
    pub fn text(v: &'a str) -> Self {
        Value::Text(v)
    }
    /// `Value::Bytes`
    pub fn bytes(v: &'a [u8]) -> Self {
        Value::Bytes(v)
    }

    /// The OID sent in Parse for this parameter (0 lets the server decide).
    pub fn oid(&self) -> u32 {
        match self {
            Value::Null | Value::Unknown(_) => 0,
            Value::Bool(_) => oid::BOOL,
            Value::I16(_) => oid::INT2,
            Value::I32(_) => oid::INT4,
            Value::I64(_) => oid::INT8,
            Value::F32(_) => oid::FLOAT4,
            Value::F64(_) => oid::FLOAT8,
            Value::Text(_) => oid::TEXT,
            Value::Bytes(_) => oid::BYTEA,
            Value::Uuid(_) => oid::UUID,
            Value::Date(_) => oid::DATE,
            Value::Timestamp(_) => oid::TIMESTAMP,
            Value::TimestampTz(_) => oid::TIMESTAMPTZ,
        }
    }

    /// The format code for Bind: 1 binary, 0 text (only `Unknown`).
    pub fn format(&self) -> i16 {
        match self {
            Value::Unknown(_) => 0,
            _ => 1,
        }
    }

    /// Append the encoded value to `scratch` and return its byte range, or `None` for NULL.
    pub fn encode_into(&self, scratch: &mut Vec<u8>) -> Result<Option<Range<usize>>> {
        let start = scratch.len();
        match self {
            Value::Null => return Ok(None),
            Value::Bool(v) => scratch.push(u8::from(*v)),
            Value::I16(v) => scratch.extend_from_slice(&v.to_be_bytes()),
            Value::I32(v) => scratch.extend_from_slice(&v.to_be_bytes()),
            Value::I64(v) => scratch.extend_from_slice(&v.to_be_bytes()),
            Value::F32(v) => scratch.extend_from_slice(&v.to_be_bytes()),
            Value::F64(v) => scratch.extend_from_slice(&v.to_be_bytes()),
            Value::Text(s) | Value::Unknown(s) => scratch.extend_from_slice(s.as_bytes()),
            Value::Bytes(b) => scratch.extend_from_slice(b),
            Value::Uuid(u) => scratch.extend_from_slice(u),
            Value::Date(d) => scratch.extend_from_slice(&date_to_pg(*d)?.to_be_bytes()),
            Value::Timestamp(t) | Value::TimestampTz(t) => {
                scratch.extend_from_slice(&timestamp_to_pg(*t)?.to_be_bytes())
            }
        }
        Ok(Some(start..scratch.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karez_chunk::rng::SplitMix64;

    fn desc(oids: &[u32]) -> Vec<FieldDescription> {
        oids.iter()
            .enumerate()
            .map(|(i, &o)| FieldDescription {
                name: format!("c{i}"),
                table_oid: 0,
                column_attr: 0,
                type_oid: o,
                type_size: 0,
                type_modifier: -1,
                format: 1,
            })
            .collect()
    }

    fn row(fields: &[Option<Vec<u8>>]) -> Vec<u8> {
        let mut b = (fields.len() as u16).to_be_bytes().to_vec();
        for f in fields {
            match f {
                None => b.extend_from_slice(&(-1i32).to_be_bytes()),
                Some(v) => {
                    b.extend_from_slice(&(v.len() as i32).to_be_bytes());
                    b.extend_from_slice(v);
                }
            }
        }
        b
    }

    #[test]
    fn plan_schema_and_formats() {
        let p = ResultPlan::from_description(&desc(&[
            oid::INT4,
            oid::TEXT,
            1700, // numeric: no codec, text fallback
        ]));
        assert_eq!(p.result_formats(), &[1, 1, 0]);
        assert_eq!(p.fields()[0].ty, ColumnType::Int32);
        assert_eq!(p.fields()[2].ty, ColumnType::Utf8);
        assert_eq!(p.fields()[1].name, "c1");
    }

    #[test]
    fn decodes_every_type_and_null() {
        let oids = [
            oid::BOOL,
            oid::INT2,
            oid::INT4,
            oid::INT8,
            oid::FLOAT4,
            oid::FLOAT8,
            oid::TEXT,
            oid::VARCHAR,
            oid::BYTEA,
            oid::UUID,
            oid::DATE,
            oid::TIMESTAMP,
            oid::TIMESTAMPTZ,
            1700,
        ];
        let plan = ResultPlan::from_description(&desc(&oids));
        let mut b = ChunkBuilder::new(plan.fields().to_vec());
        let uuid = [0xABu8; 16];
        let r = row(&[
            Some(vec![1]),
            Some(i16::MIN.to_be_bytes().to_vec()),
            Some(i32::MAX.to_be_bytes().to_vec()),
            Some(i64::MIN.to_be_bytes().to_vec()),
            Some(1.5f32.to_be_bytes().to_vec()),
            Some((-2.25f64).to_be_bytes().to_vec()),
            Some("héllo".as_bytes().to_vec()),
            Some(Vec::new()),
            Some(vec![0, 255, 0]),
            Some(uuid.to_vec()),
            Some(0i32.to_be_bytes().to_vec()), // 2000-01-01
            Some(0i64.to_be_bytes().to_vec()),
            Some((-PG_EPOCH_MICROS).to_be_bytes().to_vec()), // 1970-01-01
            Some(b"12.50".to_vec()),
        ]);
        plan.decode_row(&r, &mut b).unwrap();
        let nulls = row(&vec![None; oids.len()]);
        plan.decode_row(&nulls, &mut b).unwrap();
        let c = b.chunk();
        c.validate().unwrap();
        let r0 = c.row(0).unwrap();
        assert!(r0.get::<bool>(0).unwrap());
        assert_eq!(r0.get::<i16>(1).unwrap(), i16::MIN);
        assert_eq!(r0.get::<i32>(2).unwrap(), i32::MAX);
        assert_eq!(r0.get::<i64>(3).unwrap(), i64::MIN);
        assert_eq!(r0.get::<f32>(4).unwrap(), 1.5);
        assert_eq!(r0.get::<f64>(5).unwrap(), -2.25);
        assert_eq!(r0.get::<&str>(6).unwrap(), "héllo");
        assert_eq!(r0.get::<&str>(7).unwrap(), "");
        assert_eq!(r0.get::<&[u8]>(8).unwrap(), &[0, 255, 0]);
        assert_eq!(r0.get::<[u8; 16]>(9).unwrap(), uuid);
        assert_eq!(r0.get::<Date>(10).unwrap(), Date(PG_EPOCH_DAYS));
        assert_eq!(r0.get::<Timestamp>(11).unwrap(), Timestamp(PG_EPOCH_MICROS));
        assert_eq!(r0.get::<Timestamp>(12).unwrap(), Timestamp(0));
        assert_eq!(r0.get::<&str>(13).unwrap(), "12.50");
        let r1 = c.row(1).unwrap();
        for i in 0..oids.len() {
            assert!(r1.is_null(i).unwrap());
        }
    }

    #[test]
    fn infinities_and_extremes() {
        assert_eq!(date_from_pg(i32::MAX).unwrap(), Date(i32::MAX));
        assert_eq!(date_from_pg(i32::MIN).unwrap(), Date(i32::MIN));
        assert_eq!(timestamp_from_pg(i64::MAX).unwrap(), Timestamp(i64::MAX));
        assert_eq!(timestamp_from_pg(i64::MIN).unwrap(), Timestamp(i64::MIN));
        // Finite values that would land on an infinity sentinel or overflow are errors.
        assert!(date_from_pg(i32::MAX - 1).is_err());
        assert!(date_from_pg(i32::MAX - PG_EPOCH_DAYS).is_err());
        assert!(date_from_pg(i32::MAX - PG_EPOCH_DAYS - 1).is_ok());
        assert!(timestamp_from_pg(i64::MAX - PG_EPOCH_MICROS).is_err());
        assert!(timestamp_from_pg(i64::MAX - 1).is_err());
        assert!(timestamp_from_pg(i64::MIN + 1).is_ok());
    }

    #[test]
    fn date_and_timestamp_round_trip() {
        let mut rng = SplitMix64::new(31);
        for _ in 0..2000 {
            let d = rng.next_u32() as i32;
            if let Ok(u) = date_from_pg(d) {
                assert_eq!(date_to_pg(u).unwrap(), d);
            }
            let t = rng.next_u64() as i64;
            if let Ok(u) = timestamp_from_pg(t) {
                assert_eq!(timestamp_to_pg(u).unwrap(), t);
            }
        }
        for t in [i64::MAX, i64::MIN, 0, -1, 1] {
            let u = timestamp_from_pg(t).unwrap();
            assert_eq!(timestamp_to_pg(u).unwrap(), t);
        }
    }

    #[test]
    fn malformed_rows_are_errors_and_leave_the_builder_clean() {
        let plan = ResultPlan::from_description(&desc(&[oid::INT4, oid::BOOL, oid::TEXT]));
        let mut b = ChunkBuilder::new(plan.fields().to_vec());
        let ok = row(&[
            Some(7i32.to_be_bytes().to_vec()),
            Some(vec![0]),
            Some(b"x".to_vec()),
        ]);
        plan.decode_row(&ok, &mut b).unwrap();
        let cases: Vec<Vec<u8>> = vec![
            row(&[Some(vec![0, 0, 7]), Some(vec![0]), Some(vec![])]), // int4 of 3 bytes
            row(&[Some(vec![0; 4]), Some(vec![2]), Some(vec![])]),    // bool 2
            row(&[Some(vec![0; 4]), Some(vec![0]), Some(vec![0xff])]), // invalid UTF-8
            row(&[Some(vec![0; 4]), Some(vec![0])]),                  // wrong column count
            {
                let mut r = ok.clone();
                r.push(0); // trailing byte
                r
            },
            ok[..ok.len() - 1].to_vec(), // truncated
        ];
        for c in cases {
            assert!(plan.decode_row(&c, &mut b).is_err());
            b.chunk().validate().unwrap();
            assert_eq!(b.rows(), 1);
        }
        plan.decode_row(&ok, &mut b).unwrap();
        b.chunk().validate().unwrap();
        assert_eq!(b.rows(), 2);
    }

    #[test]
    fn random_rows_round_trip_through_encode_and_decode() {
        let plan = ResultPlan::from_description(&desc(&[
            oid::INT2,
            oid::INT4,
            oid::INT8,
            oid::FLOAT4,
            oid::FLOAT8,
            oid::TEXT,
            oid::BYTEA,
            oid::UUID,
            oid::BOOL,
        ]));
        let mut rng = SplitMix64::new(2024);
        let mut b = ChunkBuilder::new(plan.fields().to_vec());
        let mut want = Vec::new();
        for _ in 0..300 {
            let s = rng.string(10);
            let blen = rng.range(0, 9);
            let bytes = rng.bytes(blen);
            let mut u = [0u8; 16];
            u.copy_from_slice(&rng.bytes(16));
            let n = (
                rng.next_u32() as i16,
                rng.next_u32() as i32,
                rng.next_u64() as i64,
                f32::from_bits(rng.next_u32()),
                f64::from_bits(rng.next_u64()),
                rng.chance(1, 2),
            );
            let mut scratch = Vec::new();
            let values = [
                Value::I16(n.0),
                Value::I32(n.1),
                Value::I64(n.2),
                Value::F32(n.3),
                Value::F64(n.4),
                Value::Text(&s),
                Value::Bytes(&bytes),
                Value::Uuid(u),
                Value::Bool(n.5),
            ];
            let mut fields = Vec::new();
            for (i, v) in values.iter().enumerate() {
                if i == 3 && rng.chance(1, 5) {
                    fields.push(None);
                    continue;
                }
                let r = v.encode_into(&mut scratch).unwrap().unwrap();
                fields.push(Some(scratch[r].to_vec()));
            }
            plan.decode_row(&row(&fields), &mut b).unwrap();
            want.push((n, s, bytes, u, fields[3].is_none()));
        }
        b.chunk().validate().unwrap();
        for (i, (n, s, bytes, u, null3)) in want.iter().enumerate() {
            let r = b.chunk().row(i).unwrap();
            assert_eq!(r.get::<i16>(0).unwrap(), n.0);
            assert_eq!(r.get::<i32>(1).unwrap(), n.1);
            assert_eq!(r.get::<i64>(2).unwrap(), n.2);
            assert_eq!(r.is_null(3).unwrap(), *null3);
            if !null3 {
                assert_eq!(r.get::<f32>(3).unwrap().to_bits(), n.3.to_bits());
            }
            assert_eq!(r.get::<f64>(4).unwrap().to_bits(), n.4.to_bits());
            assert_eq!(r.get::<&str>(5).unwrap(), s);
            assert_eq!(r.get::<&[u8]>(6).unwrap(), &bytes[..]);
            assert_eq!(r.get::<[u8; 16]>(7).unwrap(), *u);
            assert_eq!(r.get::<bool>(8).unwrap(), n.5);
        }
    }

    #[test]
    fn value_oids_and_formats() {
        assert_eq!(Value::i32(1).oid(), oid::INT4);
        assert_eq!(Value::text("x").oid(), oid::TEXT);
        assert_eq!(Value::Null.oid(), 0);
        assert_eq!(Value::Unknown("1.5").format(), 0);
        assert_eq!(Value::i64(1).format(), 1);
        let mut s = Vec::new();
        assert_eq!(Value::Null.encode_into(&mut s).unwrap(), None);
        assert_eq!(Value::i16(-2).encode_into(&mut s).unwrap(), Some(0..2));
        assert_eq!(s, [0xff, 0xfe]);
        assert!(Value::Date(Date(i32::MIN + 5)).encode_into(&mut s).is_err());
    }
}
