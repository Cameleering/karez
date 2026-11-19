use crate::error::ChunkError;
use crate::types::{ColumnType, Date, Decimal128, Interval, Time, Timestamp};

type Result<T> = std::result::Result<T, ChunkError>;

/// One column in Arrow memory layout.
///
/// * `validity`: one bit per row, set means present. Always `ceil(len / 8)` bytes.
/// * Fixed-width types: `data` holds `len * width` little-endian bytes.
/// * `Bool`: `data` is a bit-packed bitmap of `ceil(len / 8)` bytes.
/// * `Utf8` and `Binary`: `offsets` holds `len + 1` non-decreasing `i32` values starting at 0,
///   and `data` holds the concatenated bytes.
#[derive(Debug, Clone)]
pub struct Column {
    ty: ColumnType,
    len: usize,
    null_count: usize,
    validity: Vec<u8>,
    offsets: Vec<i32>,
    data: Vec<u8>,
}

/// `10^n` for `n` up to 38 (the largest power of ten that fits in an `i128`).
fn pow10(n: u8) -> u128 {
    10u128.pow(u32::from(n.min(38)))
}

fn bit_get(bits: &[u8], i: usize) -> bool {
    bits.get(i / 8).is_some_and(|b| b & (1 << (i % 8)) != 0)
}

impl Column {
    /// An empty column of the given type.
    pub fn new(ty: ColumnType) -> Self {
        let mut offsets = Vec::new();
        if ty.is_variable() {
            offsets.push(0);
        }
        Column {
            ty,
            len: 0,
            null_count: 0,
            validity: Vec::new(),
            offsets,
            data: Vec::new(),
        }
    }

    /// The column type.
    pub fn ty(&self) -> ColumnType {
        self.ty
    }

    /// Number of rows.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when there are no rows.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of NULL rows.
    pub fn null_count(&self) -> usize {
        self.null_count
    }

    /// The validity bitmap (bit set means present), `ceil(len / 8)` bytes.
    pub fn validity(&self) -> &[u8] {
        &self.validity
    }

    /// The offsets of a variable-width column (`len + 1` entries), empty for other types.
    pub fn offsets(&self) -> &[i32] {
        &self.offsets
    }

    /// The data buffer: fixed-width little-endian values, a bool bitmap, or concatenated bytes.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Approximate memory held by the buffers, in bytes.
    pub fn byte_size(&self) -> usize {
        self.data.len() + self.validity.len() + self.offsets.len() * 4
    }

    /// Remove all rows, keeping the buffers' capacity.
    pub fn clear(&mut self) {
        self.len = 0;
        self.null_count = 0;
        self.validity.clear();
        self.data.clear();
        self.offsets.clear();
        if self.ty.is_variable() {
            self.offsets.push(0);
        }
    }

    // ---- appending -------------------------------------------------------------------------

    fn mark(&mut self, valid: bool) {
        let idx = self.len;
        if idx % 8 == 0 {
            self.validity.push(0);
        }
        if valid {
            self.validity[idx / 8] |= 1 << (idx % 8);
        } else {
            self.null_count += 1;
        }
    }

    fn expect(&self, ty: ColumnType) -> Result<()> {
        if self.ty == ty {
            Ok(())
        } else {
            Err(ChunkError::TypeMismatch {
                expected: ty,
                found: self.ty,
            })
        }
    }

    /// Append a NULL.
    pub fn push_null(&mut self) -> Result<()> {
        match self.ty {
            ColumnType::Bool => {
                if self.len % 8 == 0 {
                    self.data.push(0);
                }
            }
            ColumnType::Utf8 | ColumnType::Binary => {
                let last = *self
                    .offsets
                    .last()
                    .ok_or(ChunkError::Corrupt("no offsets"))?;
                self.offsets.push(last);
            }
            other => {
                let w = other.fixed_width().ok_or(ChunkError::Corrupt("width"))?;
                self.data.resize(
                    self.data.len().checked_add(w).ok_or(ChunkError::Overflow)?,
                    0,
                );
            }
        }
        self.mark(false);
        self.len += 1;
        Ok(())
    }

    fn push_fixed(&mut self, ty: ColumnType, bytes: &[u8]) -> Result<()> {
        self.expect(ty)?;
        debug_assert_eq!(ty.fixed_width(), Some(bytes.len()));
        self.data.extend_from_slice(bytes);
        self.mark(true);
        self.len += 1;
        Ok(())
    }

    /// Append a boolean.
    pub fn push_bool(&mut self, v: bool) -> Result<()> {
        self.expect(ColumnType::Bool)?;
        if self.len % 8 == 0 {
            self.data.push(0);
        }
        if v {
            self.data[self.len / 8] |= 1 << (self.len % 8);
        }
        self.mark(true);
        self.len += 1;
        Ok(())
    }

    /// Append an `i16`.
    pub fn push_i16(&mut self, v: i16) -> Result<()> {
        self.push_fixed(ColumnType::Int16, &v.to_le_bytes())
    }

    /// Append an `i32`.
    pub fn push_i32(&mut self, v: i32) -> Result<()> {
        self.push_fixed(ColumnType::Int32, &v.to_le_bytes())
    }

    /// Append an `i64`.
    pub fn push_i64(&mut self, v: i64) -> Result<()> {
        self.push_fixed(ColumnType::Int64, &v.to_le_bytes())
    }

    /// Append an `f32`.
    pub fn push_f32(&mut self, v: f32) -> Result<()> {
        self.push_fixed(ColumnType::Float32, &v.to_le_bytes())
    }

    /// Append an `f64`.
    pub fn push_f64(&mut self, v: f64) -> Result<()> {
        self.push_fixed(ColumnType::Float64, &v.to_le_bytes())
    }

    /// Append a UUID (16 bytes in network order).
    pub fn push_uuid(&mut self, v: [u8; 16]) -> Result<()> {
        self.push_fixed(ColumnType::Uuid, &v)
    }

    /// Append a date.
    pub fn push_date(&mut self, v: Date) -> Result<()> {
        self.push_fixed(ColumnType::Date32, &v.0.to_le_bytes())
    }

    /// Append a timestamp. The column's `tz` flag decides which timestamp type it is.
    pub fn push_timestamp(&mut self, v: Timestamp) -> Result<()> {
        match self.ty {
            ColumnType::TimestampMicros { .. } => {
                let ty = self.ty;
                self.push_fixed(ty, &v.0.to_le_bytes())
            }
            found => Err(ChunkError::TypeMismatch {
                expected: ColumnType::TimestampMicros { tz: false },
                found,
            }),
        }
    }

    /// Append a time of day, in microseconds since midnight (0 to 86,400,000,000).
    pub fn push_time(&mut self, v: Time) -> Result<()> {
        self.expect(ColumnType::Time64Micros)?;
        if !(0..=86_400_000_000).contains(&v.0) {
            return Err(ChunkError::OutOfRange("time of day"));
        }
        self.push_fixed(ColumnType::Time64Micros, &v.0.to_le_bytes())
    }

    /// Append a calendar interval.
    pub fn push_interval(&mut self, v: Interval) -> Result<()> {
        let mut b = [0u8; 16];
        b[..4].copy_from_slice(&v.months.to_le_bytes());
        b[4..8].copy_from_slice(&v.days.to_le_bytes());
        b[8..].copy_from_slice(&v.micros.to_le_bytes());
        self.push_fixed(ColumnType::Interval, &b)
    }

    /// Append a decimal given as its unscaled integer. The column's `scale` is the scale of the
    /// value; a value with more digits than the column's `precision` is an error.
    pub fn push_decimal(&mut self, unscaled: i128) -> Result<()> {
        let ty = self.ty;
        match ty {
            ColumnType::Decimal128 { precision, .. } => {
                if unscaled.unsigned_abs() >= pow10(precision) {
                    return Err(ChunkError::OutOfRange(
                        "decimal wider than the column precision",
                    ));
                }
                self.push_fixed(ty, &unscaled.to_le_bytes())
            }
            found => Err(ChunkError::TypeMismatch {
                expected: ColumnType::Decimal128 {
                    precision: Decimal128::MAX_PRECISION,
                    scale: 0,
                },
                found,
            }),
        }
    }

    fn push_var(&mut self, ty: ColumnType, bytes: &[u8]) -> Result<()> {
        self.expect(ty)?;
        let new_len = self
            .data
            .len()
            .checked_add(bytes.len())
            .ok_or(ChunkError::Overflow)?;
        let off = i32::try_from(new_len).map_err(|_| ChunkError::Overflow)?;
        self.data.extend_from_slice(bytes);
        self.offsets.push(off);
        self.mark(true);
        self.len += 1;
        Ok(())
    }

    /// Append bytes to a `Binary` column.
    pub fn push_bytes(&mut self, v: &[u8]) -> Result<()> {
        self.push_var(ColumnType::Binary, v)
    }

    /// Append UTF-8 bytes to a `Utf8` column, validating them.
    pub fn push_utf8(&mut self, v: &[u8]) -> Result<()> {
        if std::str::from_utf8(v).is_err() {
            return Err(ChunkError::InvalidUtf8);
        }
        self.push_var(ColumnType::Utf8, v)
    }

    /// Append a string to a `Utf8` column.
    pub fn push_str(&mut self, v: &str) -> Result<()> {
        self.push_var(ColumnType::Utf8, v.as_bytes())
    }

    /// Drop rows from the end so that `new_len` remain. Used to roll back a partial row.
    pub fn truncate(&mut self, new_len: usize) {
        if new_len >= self.len {
            return;
        }
        for i in new_len..self.len {
            if !bit_get(&self.validity, i) {
                self.null_count -= 1;
            }
        }
        self.len = new_len;
        self.validity.truncate(new_len.div_ceil(8));
        // Clear stale bits in the last validity byte so later pushes start from zero.
        if new_len % 8 != 0 {
            let keep = (1u16 << (new_len % 8)) as u8 - 1;
            if let Some(last) = self.validity.last_mut() {
                *last &= keep;
            }
        }
        match self.ty {
            ColumnType::Bool => {
                self.data.truncate(new_len.div_ceil(8));
                if new_len % 8 != 0 {
                    let keep = (1u16 << (new_len % 8)) as u8 - 1;
                    if let Some(last) = self.data.last_mut() {
                        *last &= keep;
                    }
                }
            }
            ColumnType::Utf8 | ColumnType::Binary => {
                self.offsets.truncate(new_len + 1);
                let end = self.offsets.last().copied().unwrap_or(0);
                self.data.truncate(end.max(0) as usize);
            }
            other => {
                if let Some(w) = other.fixed_width() {
                    self.data.truncate(new_len * w);
                }
            }
        }
    }

    // ---- reading ---------------------------------------------------------------------------

    /// True when the row is NULL.
    pub fn is_null(&self, row: usize) -> Result<bool> {
        if row >= self.len {
            return Err(ChunkError::OutOfBounds {
                index: row,
                len: self.len,
            });
        }
        Ok(!bit_get(&self.validity, row))
    }

    /// Read a value of type `T` (use `Option<T>` to accept NULL).
    pub fn get<'a, T: FromColumn<'a>>(&'a self, row: usize) -> Result<T> {
        T::read(self, row)
    }

    fn valid_row(&self, row: usize, ty: ColumnType) -> Result<()> {
        // `ty` may differ from self.ty only in the `tz` flag of timestamps, which we accept.
        let ok = match (self.ty, ty) {
            (ColumnType::TimestampMicros { .. }, ColumnType::TimestampMicros { .. }) => true,
            (a, b) => a == b,
        };
        if !ok {
            return Err(ChunkError::TypeMismatch {
                expected: ty,
                found: self.ty,
            });
        }
        if self.is_null(row)? {
            return Err(ChunkError::Null);
        }
        Ok(())
    }

    fn fixed_slice<const N: usize>(&self, row: usize) -> Result<[u8; N]> {
        let start = row.checked_mul(N).ok_or(ChunkError::Overflow)?;
        let end = start.checked_add(N).ok_or(ChunkError::Overflow)?;
        let s = self
            .data
            .get(start..end)
            .ok_or(ChunkError::Corrupt("data too short"))?;
        let mut out = [0u8; N];
        out.copy_from_slice(s);
        Ok(out)
    }

    fn var_slice(&self, row: usize) -> Result<&[u8]> {
        let a = *self
            .offsets
            .get(row)
            .ok_or(ChunkError::Corrupt("offsets too short"))?;
        let b = *self
            .offsets
            .get(row.checked_add(1).ok_or(ChunkError::Overflow)?)
            .ok_or(ChunkError::Corrupt("offsets too short"))?;
        let (a, b) = (
            usize::try_from(a).map_err(|_| ChunkError::Corrupt("negative offset"))?,
            usize::try_from(b).map_err(|_| ChunkError::Corrupt("negative offset"))?,
        );
        if a > b {
            return Err(ChunkError::Corrupt("offsets decrease"));
        }
        self.data
            .get(a..b)
            .ok_or(ChunkError::Corrupt("offset past data"))
    }

    /// Check that the buffers are consistent with the column's length and type.
    pub fn validate(&self) -> Result<()> {
        if self.validity.len() != self.len.div_ceil(8) {
            return Err(ChunkError::Corrupt("validity length"));
        }
        let nulls = (0..self.len)
            .filter(|&i| !bit_get(&self.validity, i))
            .count();
        if nulls != self.null_count {
            return Err(ChunkError::Corrupt("null count"));
        }
        match self.ty {
            ColumnType::Bool => {
                if self.data.len() != self.len.div_ceil(8) {
                    return Err(ChunkError::Corrupt("bool length"));
                }
            }
            ColumnType::Utf8 | ColumnType::Binary => {
                if self.offsets.len() != self.len + 1 || self.offsets[0] != 0 {
                    return Err(ChunkError::Corrupt("offsets length"));
                }
                let mut prev = 0i32;
                for &o in &self.offsets {
                    if o < prev {
                        return Err(ChunkError::Corrupt("offsets decrease"));
                    }
                    prev = o;
                }
                if usize::try_from(prev).ok() != Some(self.data.len()) {
                    return Err(ChunkError::Corrupt("data length"));
                }
                if self.ty == ColumnType::Utf8 {
                    for r in 0..self.len {
                        if std::str::from_utf8(self.var_slice(r)?).is_err() {
                            return Err(ChunkError::InvalidUtf8);
                        }
                    }
                }
            }
            other => {
                let w = other.fixed_width().ok_or(ChunkError::Corrupt("width"))?;
                if Some(self.data.len()) != self.len.checked_mul(w) {
                    return Err(ChunkError::Corrupt("data length"));
                }
                match other {
                    ColumnType::Time64Micros => {
                        for r in 0..self.len {
                            let us = i64::from_le_bytes(self.fixed_slice::<8>(r)?);
                            if !(0..=86_400_000_000).contains(&us) {
                                return Err(ChunkError::Corrupt("time of day out of range"));
                            }
                        }
                    }
                    ColumnType::Decimal128 { precision, scale } => {
                        if precision == 0
                            || precision > Decimal128::MAX_PRECISION
                            || scale > precision
                        {
                            return Err(ChunkError::Corrupt("decimal precision or scale"));
                        }
                        for r in 0..self.len {
                            let v = i128::from_le_bytes(self.fixed_slice::<16>(r)?);
                            if v.unsigned_abs() >= pow10(precision) {
                                return Err(ChunkError::Corrupt("decimal wider than precision"));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

/// A type that can be read out of a [`Column`].
pub trait FromColumn<'a>: Sized {
    /// Read the value at `row`. Returns [`ChunkError::Null`] if the row is NULL, except for
    /// `Option<T>`, which returns `None`.
    fn read(col: &'a Column, row: usize) -> Result<Self>;
}

impl<'a, T: FromColumn<'a>> FromColumn<'a> for Option<T> {
    fn read(col: &'a Column, row: usize) -> Result<Self> {
        if col.is_null(row)? {
            Ok(None)
        } else {
            T::read(col, row).map(Some)
        }
    }
}

macro_rules! fixed_impl {
    ($t:ty, $ty:expr, $n:expr, $conv:expr) => {
        impl<'a> FromColumn<'a> for $t {
            fn read(col: &'a Column, row: usize) -> Result<Self> {
                col.valid_row(row, $ty)?;
                let raw = col.fixed_slice::<$n>(row)?;
                Ok($conv(raw))
            }
        }
    };
}

fixed_impl!(i16, ColumnType::Int16, 2, i16::from_le_bytes);
fixed_impl!(i32, ColumnType::Int32, 4, i32::from_le_bytes);
fixed_impl!(i64, ColumnType::Int64, 8, i64::from_le_bytes);
fixed_impl!(f32, ColumnType::Float32, 4, f32::from_le_bytes);
fixed_impl!(f64, ColumnType::Float64, 8, f64::from_le_bytes);
fixed_impl!(Date, ColumnType::Date32, 4, |r| Date(i32::from_le_bytes(r)));
fixed_impl!(
    Timestamp,
    ColumnType::TimestampMicros { tz: false },
    8,
    |r| Timestamp(i64::from_le_bytes(r))
);
fixed_impl!([u8; 16], ColumnType::Uuid, 16, |r| r);
fixed_impl!(Time, ColumnType::Time64Micros, 8, |r| Time(
    i64::from_le_bytes(r)
));
fixed_impl!(Interval, ColumnType::Interval, 16, |r: [u8; 16]| Interval {
    months: i32::from_le_bytes([r[0], r[1], r[2], r[3]]),
    days: i32::from_le_bytes([r[4], r[5], r[6], r[7]]),
    micros: i64::from_le_bytes([r[8], r[9], r[10], r[11], r[12], r[13], r[14], r[15]]),
});

impl<'a> FromColumn<'a> for Decimal128 {
    /// Reads any `Decimal128` column; the scale comes from the column's type.
    fn read(col: &'a Column, row: usize) -> Result<Self> {
        let ColumnType::Decimal128 { scale, .. } = col.ty else {
            return Err(ChunkError::TypeMismatch {
                expected: ColumnType::Decimal128 {
                    precision: Decimal128::MAX_PRECISION,
                    scale: 0,
                },
                found: col.ty,
            });
        };
        if col.is_null(row)? {
            return Err(ChunkError::Null);
        }
        Ok(Decimal128 {
            unscaled: i128::from_le_bytes(col.fixed_slice::<16>(row)?),
            scale,
        })
    }
}

impl<'a> FromColumn<'a> for bool {
    fn read(col: &'a Column, row: usize) -> Result<Self> {
        col.valid_row(row, ColumnType::Bool)?;
        Ok(bit_get(&col.data, row))
    }
}

impl<'a> FromColumn<'a> for &'a str {
    fn read(col: &'a Column, row: usize) -> Result<Self> {
        col.valid_row(row, ColumnType::Utf8)?;
        std::str::from_utf8(col.var_slice(row)?).map_err(|_| ChunkError::InvalidUtf8)
    }
}

impl<'a> FromColumn<'a> for &'a [u8] {
    fn read(col: &'a Column, row: usize) -> Result<Self> {
        col.valid_row(row, ColumnType::Binary)?;
        col.var_slice(row)
    }
}

impl<'a> FromColumn<'a> for String {
    fn read(col: &'a Column, row: usize) -> Result<Self> {
        <&str>::read(col, row).map(str::to_owned)
    }
}

impl<'a> FromColumn<'a> for Vec<u8> {
    fn read(col: &'a Column, row: usize) -> Result<Self> {
        <&[u8]>::read(col, row).map(<[u8]>::to_vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SplitMix64;

    #[test]
    fn empty_variable_column_validates() {
        Column::new(ColumnType::Utf8).validate().unwrap();
        Column::new(ColumnType::Int32).validate().unwrap();
    }

    #[test]
    fn nulls_and_values_round_trip_for_each_type() {
        let mut c = Column::new(ColumnType::Int32);
        c.push_i32(i32::MIN).unwrap();
        c.push_null().unwrap();
        c.push_i32(i32::MAX).unwrap();
        c.validate().unwrap();
        assert_eq!(c.null_count(), 1);
        assert_eq!(c.get::<i32>(0).unwrap(), i32::MIN);
        assert_eq!(c.get::<Option<i32>>(1).unwrap(), None);
        assert_eq!(c.get::<i32>(1), Err(ChunkError::Null));
        assert_eq!(c.get::<Option<i32>>(2).unwrap(), Some(i32::MAX));
    }

    #[test]
    fn wrong_type_and_bounds_are_errors() {
        let mut c = Column::new(ColumnType::Int32);
        c.push_i32(1).unwrap();
        assert!(matches!(
            c.get::<i64>(0),
            Err(ChunkError::TypeMismatch { .. })
        ));
        assert!(matches!(
            c.get::<i32>(1),
            Err(ChunkError::OutOfBounds { .. })
        ));
        assert!(matches!(
            c.push_i64(1),
            Err(ChunkError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn invalid_utf8_is_rejected_on_push() {
        let mut c = Column::new(ColumnType::Utf8);
        assert_eq!(c.push_utf8(&[0xff, 0xfe]), Err(ChunkError::InvalidUtf8));
        assert_eq!(c.len(), 0);
        c.validate().unwrap();
    }

    #[test]
    fn float_special_values_keep_their_bits() {
        let mut c = Column::new(ColumnType::Float64);
        for v in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -0.0,
            f64::MIN_POSITIVE,
        ] {
            c.push_f64(v).unwrap();
        }
        assert!(c.get::<f64>(0).unwrap().is_nan());
        assert_eq!(c.get::<f64>(1).unwrap(), f64::INFINITY);
        assert!(c.get::<f64>(3).unwrap().is_sign_negative());
    }

    #[test]
    fn truncate_rolls_back_each_type() {
        for ty in [
            ColumnType::Bool,
            ColumnType::Int64,
            ColumnType::Utf8,
            ColumnType::Binary,
            ColumnType::Uuid,
        ] {
            let mut c = Column::new(ty);
            for i in 0..11 {
                match (ty, i % 3) {
                    (_, 1) => c.push_null().unwrap(),
                    (ColumnType::Bool, _) => c.push_bool(true).unwrap(),
                    (ColumnType::Int64, _) => c.push_i64(i as i64).unwrap(),
                    (ColumnType::Utf8, _) => c.push_str("héllo").unwrap(),
                    (ColumnType::Binary, _) => c.push_bytes(&[1, 2, 3]).unwrap(),
                    _ => c.push_uuid([7; 16]).unwrap(),
                }
            }
            c.validate().unwrap();
            c.truncate(5);
            c.validate().unwrap();
            assert_eq!(c.len(), 5);
            // Pushing again after a rollback must not see stale bits.
            c.push_null().unwrap();
            c.validate().unwrap();
            assert!(c.is_null(5).unwrap());
        }
    }

    #[test]
    fn randomised_round_trip_all_types() {
        let mut rng = SplitMix64::new(0xC0FFEE);
        for _round in 0..200 {
            let n = rng.range(0, 70);
            let mut b = Column::new(ColumnType::Bool);
            let mut i16c = Column::new(ColumnType::Int16);
            let mut i64c = Column::new(ColumnType::Int64);
            let mut f32c = Column::new(ColumnType::Float32);
            let mut sc = Column::new(ColumnType::Utf8);
            let mut bc = Column::new(ColumnType::Binary);
            let mut uc = Column::new(ColumnType::Uuid);
            let mut dc = Column::new(ColumnType::Date32);
            let mut tc = Column::new(ColumnType::TimestampMicros { tz: true });

            let mut want_b = Vec::new();
            let mut want_i16 = Vec::new();
            let mut want_i64 = Vec::new();
            let mut want_f32 = Vec::new();
            let mut want_s = Vec::new();
            let mut want_bytes = Vec::new();
            let mut want_u = Vec::new();
            let mut want_d = Vec::new();
            let mut want_t = Vec::new();

            for _ in 0..n {
                macro_rules! maybe {
                    ($col:ident, $want:ident, $gen:expr, $push:ident) => {
                        if rng.chance(1, 4) {
                            $col.push_null().unwrap();
                            $want.push(None);
                        } else {
                            let v = $gen;
                            $col.$push(v.clone()).unwrap();
                            $want.push(Some(v));
                        }
                    };
                }
                maybe!(b, want_b, rng.chance(1, 2), push_bool);
                maybe!(i16c, want_i16, rng.next_u32() as i16, push_i16);
                maybe!(i64c, want_i64, rng.next_u64() as i64, push_i64);
                maybe!(f32c, want_f32, f32::from_bits(rng.next_u32()), push_f32);
                if rng.chance(1, 4) {
                    sc.push_null().unwrap();
                    want_s.push(None);
                } else {
                    let v = rng.string(12);
                    sc.push_str(&v).unwrap();
                    want_s.push(Some(v));
                }
                if rng.chance(1, 4) {
                    bc.push_null().unwrap();
                    want_bytes.push(None);
                } else {
                    let len = rng.range(0, 20);
                    let v = rng.bytes(len);
                    bc.push_bytes(&v).unwrap();
                    want_bytes.push(Some(v));
                }
                if rng.chance(1, 4) {
                    uc.push_null().unwrap();
                    want_u.push(None);
                } else {
                    let mut v = [0u8; 16];
                    v.copy_from_slice(&rng.bytes(16));
                    uc.push_uuid(v).unwrap();
                    want_u.push(Some(v));
                }
                if rng.chance(1, 4) {
                    dc.push_null().unwrap();
                    want_d.push(None);
                } else {
                    let v = Date(rng.next_u32() as i32);
                    dc.push_date(v).unwrap();
                    want_d.push(Some(v));
                }
                if rng.chance(1, 4) {
                    tc.push_null().unwrap();
                    want_t.push(None);
                } else {
                    let v = Timestamp(rng.next_u64() as i64);
                    tc.push_timestamp(v).unwrap();
                    want_t.push(Some(v));
                }
            }

            for c in [&b, &i16c, &i64c, &f32c, &sc, &bc, &uc, &dc, &tc] {
                c.validate().unwrap();
                assert_eq!(c.len(), n);
            }
            for r in 0..n {
                assert_eq!(b.get::<Option<bool>>(r).unwrap(), want_b[r]);
                assert_eq!(i16c.get::<Option<i16>>(r).unwrap(), want_i16[r]);
                assert_eq!(i64c.get::<Option<i64>>(r).unwrap(), want_i64[r]);
                assert_eq!(
                    f32c.get::<Option<f32>>(r).unwrap().map(f32::to_bits),
                    want_f32[r].map(f32::to_bits)
                );
                assert_eq!(sc.get::<Option<&str>>(r).unwrap(), want_s[r].as_deref());
                assert_eq!(
                    bc.get::<Option<&[u8]>>(r).unwrap(),
                    want_bytes[r].as_deref()
                );
                assert_eq!(uc.get::<Option<[u8; 16]>>(r).unwrap(), want_u[r]);
                assert_eq!(dc.get::<Option<Date>>(r).unwrap(), want_d[r]);
                assert_eq!(tc.get::<Option<Timestamp>>(r).unwrap(), want_t[r]);
            }
        }
    }

    #[test]
    fn decimal_time_and_interval_round_trip_and_reject_bad_values() {
        let dec = ColumnType::Decimal128 {
            precision: 5,
            scale: 2,
        };
        let mut c = Column::new(dec);
        for v in [0i128, 1, -1, 99_999, -99_999, 5] {
            c.push_decimal(v).unwrap();
        }
        c.push_null().unwrap();
        assert!(
            c.push_decimal(100_000).is_err(),
            "six digits in precision 5"
        );
        assert!(c.push_decimal(-100_000).is_err());
        c.validate().unwrap();
        assert_eq!(c.len(), 7, "a rejected value adds no row");
        let d = c.get::<Decimal128>(3).unwrap();
        assert_eq!((d.unscaled, d.scale), (99_999, 2));
        assert_eq!(d.to_string(), "999.99");
        assert_eq!(c.get::<Decimal128>(4).unwrap().to_string(), "-999.99");
        assert_eq!(c.get::<Decimal128>(5).unwrap().to_string(), "0.05");
        assert_eq!(c.get::<Decimal128>(0).unwrap().to_string(), "0.00");
        assert_eq!(c.get::<Option<Decimal128>>(6).unwrap(), None);
        assert!(c.get::<i64>(0).is_err());
        // precision 38 holds the full range
        let wide = ColumnType::Decimal128 {
            precision: 38,
            scale: 0,
        };
        let mut w = Column::new(wide);
        let max = 10i128.pow(38) - 1;
        w.push_decimal(max).unwrap();
        w.push_decimal(-max).unwrap();
        assert!(w.push_decimal(max + 1).is_err());
        w.validate().unwrap();
        assert_eq!(w.get::<Decimal128>(1).unwrap().unscaled, -max);

        let mut t = Column::new(ColumnType::Time64Micros);
        for v in [0, 1, 86_399_999_999, 86_400_000_000] {
            t.push_time(Time(v)).unwrap();
        }
        assert!(t.push_time(Time(-1)).is_err());
        assert!(t.push_time(Time(86_400_000_001)).is_err());
        t.validate().unwrap();
        assert_eq!(t.get::<Time>(2).unwrap().to_string(), "23:59:59.999999");
        assert_eq!(t.get::<Time>(3).unwrap().to_string(), "24:00:00");
        assert_eq!(t.get::<Time>(1).unwrap().to_string(), "00:00:00.000001");

        let mut i = Column::new(ColumnType::Interval);
        let samples = [
            Interval::default(),
            Interval {
                months: 14,
                days: 3,
                micros: 14_706_789_000,
            },
            Interval {
                months: i32::MIN,
                days: i32::MIN,
                micros: i64::MIN,
            },
            Interval {
                months: i32::MAX,
                days: i32::MAX,
                micros: i64::MAX,
            },
        ];
        for v in samples {
            i.push_interval(v).unwrap();
        }
        i.validate().unwrap();
        for (r, v) in samples.iter().enumerate() {
            assert_eq!(i.get::<Interval>(r).unwrap(), *v);
        }
        assert_eq!(samples[0].to_string(), "PT0S");
        assert_eq!(samples[1].to_string(), "P1Y2M3DT4H5M6.789S");
        // a column built by hand with a value outside its type is caught by validate
        let mut bad = Column::new(ColumnType::Time64Micros);
        bad.push_time(Time(1)).unwrap();
        bad.data[..8].copy_from_slice(&(-5i64).to_le_bytes());
        assert!(bad.validate().is_err());
        let mut bad = Column::new(dec);
        bad.push_decimal(1).unwrap();
        bad.data.copy_from_slice(&i128::MAX.to_le_bytes());
        assert!(bad.validate().is_err());
        // truncating a partial row keeps these columns consistent
        c.truncate(2);
        c.validate().unwrap();
        i.truncate(1);
        i.validate().unwrap();
    }

    #[test]
    fn decimal_display_pads_and_signs() {
        let d = |u: i128, s: u8| {
            Decimal128 {
                unscaled: u,
                scale: s,
            }
            .to_string()
        };
        assert_eq!(d(0, 0), "0");
        assert_eq!(d(0, 3), "0.000");
        assert_eq!(d(-5, 3), "-0.005");
        assert_eq!(d(123_456, 3), "123.456");
        assert_eq!(d(-123_456, 0), "-123456");
        assert_eq!(d(i128::MIN, 0), i128::MIN.to_string());
    }

    #[test]
    fn corrupted_offsets_are_detected_not_panicked() {
        let mut c = Column::new(ColumnType::Binary);
        c.push_bytes(&[1, 2, 3]).unwrap();
        c.offsets[1] = 99;
        assert!(c.validate().is_err());
        assert!(c.get::<&[u8]>(0).is_err());
        c.offsets[1] = -1;
        assert!(c.get::<&[u8]>(0).is_err());
    }
}
