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
}

impl ColumnType {
    /// Byte width of one value for fixed-width types, `None` for booleans and variable-width types.
    pub fn fixed_width(self) -> Option<usize> {
        match self {
            ColumnType::Int16 => Some(2),
            ColumnType::Int32 | ColumnType::Float32 | ColumnType::Date32 => Some(4),
            ColumnType::Int64 | ColumnType::Float64 | ColumnType::TimestampMicros { .. } => Some(8),
            ColumnType::Uuid => Some(16),
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
