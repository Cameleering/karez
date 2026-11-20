/// The logical type of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnType {
    /// Boolean, bit-packed.
    Bool,
    /// 16-bit signed integer.
    Int16,
    /// 32-bit signed integer.
    Int32,
    /// 64-bit signed integer.
    Int64,
    /// 32-bit unsigned integer (ADBC's `info_name`).
    UInt32,
    /// 64-bit unsigned integer (ADBC's `uint64` statistic value).
    UInt64,
    /// 32-bit IEEE float.
    Float32,
    /// 64-bit IEEE float.
    Float64,
    /// UTF-8 text (offsets plus bytes).
    Utf8,
    /// Arbitrary bytes (offsets plus bytes).
    Binary,
    /// 16 bytes, fixed width.
    Uuid,
    /// Days since 1970-01-01, `i32`.
    Date32,
    /// Microseconds since 1970-01-01T00:00:00, `i64`; `tz` is true for `timestamptz` (UTC instant).
    TimestampMicros {
        /// True when the value is an instant in UTC.
        tz: bool,
    },
    /// Microseconds since midnight, `i64`, from 0 to 86,400,000,000 (PostgreSQL allows `24:00:00`).
    Time64Micros,
    /// A decimal number: a 128-bit little-endian integer `unscaled` with the value
    /// `unscaled / 10^scale`. `precision` is at most 38 and `scale` at most `precision`.
    Decimal128 {
        /// Total number of decimal digits the column can hold (1 to 38).
        precision: u8,
        /// Digits after the decimal point (0 to `precision`).
        scale: u8,
    },
    /// A calendar interval: 16 bytes, `months: i32`, `days: i32`, `micros: i64`, all little-endian
    /// (PostgreSQL's own three fields).
    Interval,
}

impl ColumnType {
    /// Byte width of one value for fixed-width types, `None` for booleans and variable-width types.
    pub fn fixed_width(self) -> Option<usize> {
        match self {
            ColumnType::Int16 => Some(2),
            ColumnType::Int32 | ColumnType::UInt32 | ColumnType::Float32 | ColumnType::Date32 => {
                Some(4)
            }
            ColumnType::Int64
            | ColumnType::UInt64
            | ColumnType::Float64
            | ColumnType::TimestampMicros { .. }
            | ColumnType::Time64Micros => Some(8),
            ColumnType::Uuid | ColumnType::Decimal128 { .. } | ColumnType::Interval => Some(16),
            ColumnType::Bool | ColumnType::Utf8 | ColumnType::Binary => None,
        }
    }

    /// True for `Utf8` and `Binary`.
    pub fn is_variable(self) -> bool {
        matches!(self, ColumnType::Utf8 | ColumnType::Binary)
    }
}

/// A column name and type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Column name.
    pub name: String,
    /// Column type.
    pub ty: ColumnType,
}

impl Field {
    /// A new field.
    pub fn new(name: impl Into<String>, ty: ColumnType) -> Self {
        Field {
            name: name.into(),
            ty,
        }
    }
}

/// Days since 1970-01-01. `i32::MAX` and `i32::MIN` stand for PostgreSQL's `infinity` and `-infinity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(pub i32);

/// Microseconds since 1970-01-01T00:00:00. `i64::MAX` and `i64::MIN` stand for `infinity` and `-infinity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub i64);

/// Microseconds since midnight, 0 to 86,400,000,000 inclusive (`24:00:00` is a valid PostgreSQL
/// `time`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Time(pub i64);

impl std::fmt::Display for Time {
    /// `HH:MM:SS` with up to six fractional digits and no trailing zeros, as PostgreSQL prints it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let us = self.0;
        write!(
            f,
            "{:02}:{:02}:{:02}",
            us / 3_600_000_000,
            us / 60_000_000 % 60,
            us / 1_000_000 % 60
        )?;
        write_fraction(f, us % 1_000_000)
    }
}

/// `.ffffff` with trailing zeros removed; nothing when zero.
fn write_fraction(f: &mut std::fmt::Formatter<'_>, frac: i64) -> std::fmt::Result {
    if frac == 0 {
        return Ok(());
    }
    let s = format!("{frac:06}");
    write!(f, ".{}", s.trim_end_matches('0'))
}

/// A calendar interval in PostgreSQL's three fields. They are kept apart because a month has no
/// fixed number of days and a day has no fixed number of microseconds (daylight saving time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Interval {
    /// Months (a year is 12).
    pub months: i32,
    /// Days.
    pub days: i32,
    /// Microseconds (hours, minutes, seconds and fractions).
    pub micros: i64,
}

impl std::fmt::Display for Interval {
    /// ISO 8601, for example `P1Y2M3DT4H5M6.789S`, with `PT0S` for zero. This is what PostgreSQL
    /// prints with `SET intervalstyle = 'iso_8601'`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if *self == Interval::default() {
            return f.write_str("PT0S");
        }
        f.write_str("P")?;
        let (years, mons) = (self.months / 12, self.months % 12);
        if years != 0 {
            write!(f, "{years}Y")?;
        }
        if mons != 0 {
            write!(f, "{mons}M")?;
        }
        if self.days != 0 {
            write!(f, "{}D", self.days)?;
        }
        if self.micros != 0 {
            f.write_str("T")?;
            let hours = self.micros / 3_600_000_000;
            let mins = self.micros / 60_000_000 % 60;
            let sec = self.micros % 60_000_000;
            if hours != 0 {
                write!(f, "{hours}H")?;
            }
            if mins != 0 {
                write!(f, "{mins}M")?;
            }
            if sec != 0 {
                if sec < 0 {
                    f.write_str("-")?;
                }
                let a = sec.unsigned_abs();
                write!(f, "{}", a / 1_000_000)?;
                write_fraction(f, (a % 1_000_000) as i64)?;
                f.write_str("S")?;
            }
        }
        Ok(())
    }
}

/// A decimal value read from a `Decimal128` column: `unscaled / 10^scale`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Decimal128 {
    /// The value times `10^scale`.
    pub unscaled: i128,
    /// Digits after the decimal point.
    pub scale: u8,
}

impl Decimal128 {
    /// The largest number of digits a `Decimal128` holds.
    pub const MAX_PRECISION: u8 = 38;
}

impl std::fmt::Display for Decimal128 {
    /// The plain decimal form with exactly `scale` fractional digits, for example `-12.50`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let digits = self.unscaled.unsigned_abs().to_string();
        let scale = usize::from(self.scale);
        if self.unscaled < 0 {
            f.write_str("-")?;
        }
        if scale == 0 {
            return f.write_str(&digits);
        }
        let padded = if digits.len() <= scale {
            format!("{}{digits}", "0".repeat(scale + 1 - digits.len()))
        } else {
            digits
        };
        let (int, frac) = padded.split_at(padded.len() - scale);
        write!(f, "{int}.{frac}")
    }
}
