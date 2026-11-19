//! Binary value codecs: decoding DataRow fields straight into a chunk, and encoding parameters.
//!
//! Supported result types are in [`oid`]. A column of any other type is requested in text
//! format and stored as `Utf8` (PostgreSQL's own text rendering), because those types have no
//! decoder here yet. That is the only text path.
//!
//! Native chunk types: `bool`, `int2`, `int4`, `int8`, `float4`, `float8`, `bytea`, `uuid`,
//! `date`, `time` (`Time64Micros`), `timestamp`, `timestamptz`, `interval` (`Interval`) and
//! `numeric(p,s)` with `p` up to 38 (`Decimal128`). Stored as `Utf8` text, read in binary and
//! rendered exactly as `::text` would (`render.rs`): an unconstrained `numeric` or one wider than
//! 38 digits, `timetz`, `json`, `jsonb`, `xml`, `inet`, `cidr`, `macaddr`, `macaddr8`, `bit`,
//! `varbit`, `"char"`, arrays and enums. The decision for each, with the reason, is in
//! `docs/design/m1-plan.md` step 3.

use crate::backend::FieldDescription;
use crate::error::{ProtoError, Result};
use crate::numeric;
use crate::render;
use crate::wire::Reader;
use karez_chunk::{ChunkBuilder, ColumnType, Date, Decimal128, Field, Interval, Time, Timestamp};
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
    /// `"char"`
    pub const CHAR: u32 = 18;
    /// `oid`
    pub const OID: u32 = 26;
    /// `json`
    pub const JSON: u32 = 114;
    /// `xml`
    pub const XML: u32 = 142;
    /// `cidr`
    pub const CIDR: u32 = 650;
    /// `macaddr8`
    pub const MACADDR8: u32 = 774;
    /// `macaddr`
    pub const MACADDR: u32 = 829;
    /// `inet`
    pub const INET: u32 = 869;
    /// `time`
    pub const TIME: u32 = 1083;
    /// `interval`
    pub const INTERVAL: u32 = 1186;
    /// `timetz`
    pub const TIMETZ: u32 = 1266;
    /// `bit`
    pub const BIT: u32 = 1560;
    /// `varbit`
    pub const VARBIT: u32 = 1562;
    /// `numeric`
    pub const NUMERIC: u32 = 1700;
    /// `jsonb`
    pub const JSONB: u32 = 3802;
    /// `void` (a function that returns nothing, such as `pg_sleep`); rendered as an empty string
    pub const VOID: u32 = 2278;
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
    /// `oid`, stored as `Int64`.
    OidI64,
    /// `numeric(precision, scale)` that fits a `Decimal128`.
    Numeric {
        precision: u8,
        scale: u8,
    },
    /// `time`
    Time,
    /// `interval`
    Interval,
    /// Binary value rendered to its text form by `render` (numeric, jsonb, interval, ...).
    Rendered(u32),
    /// Array of the given element type, rendered as an array literal.
    Array(u32),
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
            oid::OID => Kind::OidI64,
            oid::TIME => Kind::Time,
            oid::INTERVAL => Kind::Interval,
            o if render::is_renderable(o) => Kind::Rendered(o),
            o => match render::array_elem(o) {
                Some(e) => Kind::Array(e),
                None => Kind::TextFallback,
            },
        }
    }

    /// Like [`Kind::from_oid`], using the type modifier: `numeric(p,s)` with `p <= 38` is a
    /// `Decimal128`, any other `numeric` stays rendered text.
    fn from_field(oid: u32, typmod: i32) -> Kind {
        if oid == oid::NUMERIC {
            return match numeric::precision_scale(typmod) {
                Some((precision, scale)) => Kind::Numeric { precision, scale },
                None => Kind::Rendered(oid),
            };
        }
        Kind::from_oid(oid)
    }

    fn column_type(self) -> ColumnType {
        match self {
            Kind::Bool => ColumnType::Bool,
            Kind::I16 => ColumnType::Int16,
            Kind::I32 => ColumnType::Int32,
            Kind::I64 | Kind::OidI64 => ColumnType::Int64,
            Kind::F32 => ColumnType::Float32,
            Kind::F64 => ColumnType::Float64,
            Kind::Text | Kind::TextFallback | Kind::Rendered(_) | Kind::Array(_) => {
                ColumnType::Utf8
            }
            Kind::Bytea => ColumnType::Binary,
            Kind::Uuid => ColumnType::Uuid,
            Kind::Date => ColumnType::Date32,
            Kind::Timestamp => ColumnType::TimestampMicros { tz: false },
            Kind::TimestampTz => ColumnType::TimestampMicros { tz: true },
            Kind::Numeric { precision, scale } => ColumnType::Decimal128 { precision, scale },
            Kind::Time => ColumnType::Time64Micros,
            Kind::Interval => ColumnType::Interval,
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
    oids: Vec<u32>,
    kinds: Vec<Kind>,
    formats: Vec<i16>,
    fields: Vec<Field>,
}

impl ResultPlan {
    /// Build a plan from a RowDescription.
    pub fn from_description(desc: &[FieldDescription]) -> Self {
        Self::from_description_with(desc, &|_| false)
    }

    /// True if this crate has a codec for the type OID (native or rendered). Other types need
    /// the caller's help: see [`ResultPlan::from_description_with`].
    pub fn knows_type(oid: u32) -> bool {
        Kind::from_oid(oid) != Kind::TextFallback
    }

    /// Like [`ResultPlan::from_description`], where `is_text_binary(oid)` tells that a type
    /// unknown here sends its text form as its binary value (enums do). Such columns are read
    /// in binary format as `Utf8`.
    pub fn from_description_with(
        desc: &[FieldDescription],
        is_text_binary: &dyn Fn(u32) -> bool,
    ) -> Self {
        let kinds: Vec<Kind> = desc
            .iter()
            .map(|d| match Kind::from_field(d.type_oid, d.type_modifier) {
                Kind::TextFallback if is_text_binary(d.type_oid) => Kind::Text,
                k => k,
            })
            .collect();
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
            oids: desc.iter().map(|d| d.type_oid).collect(),
            kinds,
            formats,
            fields,
        }
    }

    /// The server's type OID of each column.
    pub fn type_oids(&self) -> &[u32] {
        &self.oids
    }

    /// Index and OID of the first column whose type has no binary codec here (it would be
    /// fetched as text), if any.
    pub fn first_text_fallback(&self) -> Option<(usize, u32)> {
        self.kinds
            .iter()
            .position(|k| *k == Kind::TextFallback)
            .map(|i| (i, self.oids[i]))
    }

    /// A plan for a statement that returns no columns.
    pub fn empty() -> Self {
        ResultPlan {
            oids: Vec::new(),
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
                Kind::OidI64 => col.push_i64(i64::from(u32::from_be_bytes(fixed(v, oid::OID)?)))?,
                Kind::Rendered(o) => {
                    let mut s = String::new();
                    render::render_scalar(*o, v, &mut s)?;
                    col.push_utf8(s.as_bytes())?
                }
                Kind::Array(e) => {
                    let mut s = String::new();
                    render::render_array(*e, v, &mut s)?;
                    col.push_utf8(s.as_bytes())?
                }
                Kind::Numeric { scale, .. } => col.push_decimal(numeric::decode(v, *scale)?)?,
                Kind::Time => {
                    let us = i64::from_be_bytes(fixed(v, oid::TIME)?);
                    if !(0..=86_400_000_000).contains(&us) {
                        return Err(bad(oid::TIME, "time out of range"));
                    }
                    col.push_time(Time(us))?
                }
                Kind::Interval => {
                    let b: [u8; 16] = fixed(v, oid::INTERVAL)?;
                    col.push_interval(Interval {
                        micros: i64::from_be_bytes([
                            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                        ]),
                        days: i32::from_be_bytes([b[8], b[9], b[10], b[11]]),
                        months: i32::from_be_bytes([b[12], b[13], b[14], b[15]]),
                    })?
                }
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
    /// `time`
    Time(Time),
    /// `interval`
    Interval(Interval),
    /// `numeric`: `unscaled / 10^scale`, sent in the binary format with that display scale.
    Numeric(Decimal128),
    /// A one-dimensional array of a basic type, sent in the binary format.
    Array(Array<'a>),
    /// A text-format literal whose type the server infers from context (for types without a
    /// codec here, such as an enum label, `inet` or `jsonb`). It is still sent as a bound value.
    Unknown(&'a str),
}

/// A one-dimensional array parameter. `None` is a NULL element. An empty slice is an empty array.
/// Arrays of other element types go through [`Value::Unknown`] as an array literal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Array<'a> {
    /// `bool[]`
    Bool(&'a [Option<bool>]),
    /// `int2[]`
    I16(&'a [Option<i16>]),
    /// `int4[]`
    I32(&'a [Option<i32>]),
    /// `int8[]`
    I64(&'a [Option<i64>]),
    /// `float4[]`
    F32(&'a [Option<f32>]),
    /// `float8[]`
    F64(&'a [Option<f64>]),
    /// `text[]`
    Text(&'a [Option<&'a str>]),
    /// `text[]` from owned strings (for callers that keep their values in `String`s).
    Strings(&'a [Option<String>]),
}

impl Array<'_> {
    /// OID of the array type.
    fn array_oid(&self) -> u32 {
        match self {
            Array::Bool(_) => 1000,
            Array::I16(_) => 1005,
            Array::I32(_) => 1007,
            Array::I64(_) => 1016,
            Array::F32(_) => 1021,
            Array::F64(_) => 1022,
            Array::Text(_) | Array::Strings(_) => 1009,
        }
    }

    fn elem_oid(&self) -> u32 {
        match self {
            Array::Bool(_) => oid::BOOL,
            Array::I16(_) => oid::INT2,
            Array::I32(_) => oid::INT4,
            Array::I64(_) => oid::INT8,
            Array::F32(_) => oid::FLOAT4,
            Array::F64(_) => oid::FLOAT8,
            Array::Text(_) | Array::Strings(_) => oid::TEXT,
        }
    }

    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        fn body<T>(
            items: &[Option<T>],
            elem: u32,
            out: &mut Vec<u8>,
            mut put: impl FnMut(&T, &mut Vec<u8>),
        ) -> Result<()> {
            let ndim: i32 = if items.is_empty() { 0 } else { 1 };
            let has_null = i32::from(items.iter().any(Option::is_none));
            out.extend_from_slice(&ndim.to_be_bytes());
            out.extend_from_slice(&has_null.to_be_bytes());
            out.extend_from_slice(&elem.to_be_bytes());
            if items.is_empty() {
                return Ok(());
            }
            let n = i32::try_from(items.len()).map_err(|_| ProtoError::Overflow)?;
            out.extend_from_slice(&n.to_be_bytes());
            out.extend_from_slice(&1i32.to_be_bytes()); // lower bound
            for it in items {
                match it {
                    None => out.extend_from_slice(&(-1i32).to_be_bytes()),
                    Some(x) => {
                        let at = out.len();
                        out.extend_from_slice(&[0; 4]);
                        put(x, out);
                        let len =
                            i32::try_from(out.len() - at - 4).map_err(|_| ProtoError::Overflow)?;
                        out[at..at + 4].copy_from_slice(&len.to_be_bytes());
                    }
                }
            }
            Ok(())
        }
        let e = self.elem_oid();
        match self {
            Array::Bool(v) => body(v, e, out, |x, o| o.push(u8::from(*x))),
            Array::I16(v) => body(v, e, out, |x, o| o.extend_from_slice(&x.to_be_bytes())),
            Array::I32(v) => body(v, e, out, |x, o| o.extend_from_slice(&x.to_be_bytes())),
            Array::I64(v) => body(v, e, out, |x, o| o.extend_from_slice(&x.to_be_bytes())),
            Array::F32(v) => body(v, e, out, |x, o| o.extend_from_slice(&x.to_be_bytes())),
            Array::F64(v) => body(v, e, out, |x, o| o.extend_from_slice(&x.to_be_bytes())),
            Array::Text(v) => body(v, e, out, |x, o| o.extend_from_slice(x.as_bytes())),
            Array::Strings(v) => body(v, e, out, |x, o| o.extend_from_slice(x.as_bytes())),
        }
    }
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
            Value::Time(_) => oid::TIME,
            Value::Interval(_) => oid::INTERVAL,
            Value::Numeric(_) => oid::NUMERIC,
            Value::Array(a) => a.array_oid(),
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
            Value::Time(t) => {
                if !(0..=86_400_000_000).contains(&t.0) {
                    return Err(bad(oid::TIME, "time out of range"));
                }
                scratch.extend_from_slice(&t.0.to_be_bytes())
            }
            Value::Interval(i) => {
                scratch.extend_from_slice(&i.micros.to_be_bytes());
                scratch.extend_from_slice(&i.days.to_be_bytes());
                scratch.extend_from_slice(&i.months.to_be_bytes());
            }
            Value::Numeric(d) => numeric::encode(d.unscaled, d.scale, scratch),
            Value::Array(a) => {
                a.encode(scratch).inspect_err(|_| scratch.truncate(start))?;
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
            790, // money: no codec, text fallback
            oid::NUMERIC,
        ]));
        assert_eq!(p.result_formats(), &[1, 1, 0, 1]);
        assert_eq!(p.first_text_fallback(), Some((2, 790)));
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
            790, // money: text fallback
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
    fn numeric_time_and_interval_decode_natively_and_the_rest_stays_text() {
        let mut d = desc(&[
            oid::NUMERIC,
            oid::NUMERIC,
            oid::TIME,
            oid::INTERVAL,
            oid::TIMETZ,
        ]);
        d[0].type_modifier = ((12 << 16) | 2) + 4; // numeric(12,2)
        d[1].type_modifier = -1; // numeric
        let plan = ResultPlan::from_description(&d);
        assert_eq!(
            plan.fields().iter().map(|f| f.ty).collect::<Vec<_>>(),
            vec![
                ColumnType::Decimal128 {
                    precision: 12,
                    scale: 2
                },
                ColumnType::Utf8,
                ColumnType::Time64Micros,
                ColumnType::Interval,
                ColumnType::Utf8,
            ]
        );
        assert_eq!(plan.result_formats(), &[1, 1, 1, 1, 1]);
        let mut b = ChunkBuilder::new(plan.fields().to_vec());
        // numeric(12,2) -12.50: digits 12 and 5000, weight 0, negative, dscale 2
        let mut n = Vec::new();
        for x in [2i16, 0, 0x4000, 2, 12, 5000] {
            n.extend_from_slice(&x.to_be_bytes());
        }
        let mut iv = (14_706_789_000i64).to_be_bytes().to_vec(); // micros
        iv.extend_from_slice(&3i32.to_be_bytes()); // days
        iv.extend_from_slice(&14i32.to_be_bytes()); // months
        let mut tz = 0i64.to_be_bytes().to_vec();
        tz.extend_from_slice(&0i32.to_be_bytes());
        plan.decode_row(
            &row(&[
                Some(n.clone()),
                Some(n),
                Some(86_400_000_000i64.to_be_bytes().to_vec()),
                Some(iv),
                Some(tz),
            ]),
            &mut b,
        )
        .unwrap();
        let r = b.chunk().row(0).unwrap();
        assert_eq!(r.get::<Decimal128>(0).unwrap().to_string(), "-12.50");
        assert_eq!(r.get::<&str>(1).unwrap(), "-12.50");
        assert_eq!(r.get::<Time>(2).unwrap().to_string(), "24:00:00");
        assert_eq!(
            r.get::<Interval>(3).unwrap(),
            Interval {
                months: 14,
                days: 3,
                micros: 14_706_789_000
            }
        );
        assert_eq!(r.get::<&str>(4).unwrap(), "00:00:00+00");
        // out of range values are value errors that leave the builder clean
        let bad_time = row(&[
            None,
            None,
            Some(86_400_000_001i64.to_be_bytes().to_vec()),
            None,
            None,
        ]);
        assert!(plan.decode_row(&bad_time, &mut b).is_err());
        let mut nan = Vec::new();
        for x in [0i16, 0, -0x4000, 0] {
            nan.extend_from_slice(&x.to_be_bytes());
        }
        let bad_nan = row(&[Some(nan), None, None, None, None]);
        assert!(plan.decode_row(&bad_nan, &mut b).is_err());
        b.chunk().validate().unwrap();
        assert_eq!(b.rows(), 1);
    }

    #[test]
    fn native_parameters_encode_as_the_server_expects() {
        let mut s = Vec::new();
        let t = Value::Time(Time(1)).encode_into(&mut s).unwrap().unwrap();
        assert_eq!(&s[t], &1i64.to_be_bytes());
        s.clear();
        let i = Value::Interval(Interval {
            months: 1,
            days: 2,
            micros: 3,
        })
        .encode_into(&mut s)
        .unwrap()
        .unwrap();
        assert_eq!(&s[i][..8], &3i64.to_be_bytes(), "micros come first");
        assert_eq!(&s[8..12], &2i32.to_be_bytes());
        assert_eq!(&s[12..16], &1i32.to_be_bytes());
        assert!(Value::Time(Time(-1)).encode_into(&mut s).is_err());
        assert!(Value::Time(Time(86_400_000_001))
            .encode_into(&mut s)
            .is_err());
        assert_eq!(Value::Time(Time(0)).oid(), oid::TIME);
        assert_eq!(
            Value::Numeric(Decimal128 {
                unscaled: 1,
                scale: 0
            })
            .oid(),
            oid::NUMERIC
        );
        // array: header, one dimension, NULL as length -1
        s.clear();
        let items = [Some(7i32), None];
        let a = Value::Array(Array::I32(&items));
        assert_eq!(a.oid(), 1007);
        assert_eq!(a.format(), 1);
        let r = a.encode_into(&mut s).unwrap().unwrap();
        let mut want = Vec::new();
        for x in [1i32, 1, 23, 2, 1] {
            want.extend_from_slice(&x.to_be_bytes());
        }
        want.extend_from_slice(&4i32.to_be_bytes());
        want.extend_from_slice(&7i32.to_be_bytes());
        want.extend_from_slice(&(-1i32).to_be_bytes());
        assert_eq!(&s[r], &want[..]);
        // an empty array has no dimensions
        s.clear();
        let none: [Option<i64>; 0] = [];
        let r = Value::Array(Array::I64(&none))
            .encode_into(&mut s)
            .unwrap()
            .unwrap();
        let mut want = Vec::new();
        for x in [0i32, 0, 20] {
            want.extend_from_slice(&x.to_be_bytes());
        }
        assert_eq!(&s[r], &want[..]);
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
