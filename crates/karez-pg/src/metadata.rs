//! Catalog metadata as chunks, shaped after ADBC 1.1.0's `GetObjects`, `GetTableTypes` and
//! `GetTableSchema` (the schemas in `adbc.h`).
//!
//! ADBC's `GetObjects` result is nested (`list<struct<...>>`), and a [`Chunk`] has no list or struct
//! column. So each level is a **flat** chunk with one row per leaf, carrying the keys of the levels
//! above it (`catalog_name`, `db_schema_name`, `table_name`), and every ADBC field keeps its name,
//! its order and its type. The ADBC export (milestone 5) groups the flat rows back into the nested
//! form. Two ADBC list fields have no scalar twin and are folded into rows: a constraint's
//! `constraint_column_names` becomes one row per column (`constraint_column_name` and
//! `constraint_column_position`), and its `constraint_column_usage` is repeated on those rows.
//!
//! Search patterns follow ADBC: `%` matches any run of characters and `_` exactly one, there is no
//! escape character, and `None` means no filter. The catalog is the current database. Schemas
//! `pg_toast*` and the per-session `pg_temp_*` are left out; `pg_catalog` and `information_schema`
//! are included.

use crate::conn::{quote_ident, Connection};
use crate::error::Result;
use karez_chunk::{Chunk, ChunkBuilder, ColumnType, Field};
use karez_proto_pg::{Array, Value};

/// A metadata result: its schema (kept even when there are no rows) and its chunks.
#[derive(Debug, Clone)]
pub struct Rows {
    /// The columns, with ADBC's names and types.
    pub fields: Vec<Field>,
    /// The rows, in chunks of at most 1,024.
    pub chunks: Vec<Chunk>,
}

impl Rows {
    /// Number of rows in all chunks.
    pub fn rows(&self) -> usize {
        self.chunks.iter().map(Chunk::rows).sum()
    }
}

/// How deep [`Metadata::objects`] goes, as ADBC's `depth` argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// Catalogs only (ADBC depth 1).
    Catalogs,
    /// Catalogs and schemas (depth 2).
    Schemas,
    /// Catalogs, schemas and tables (depth 3).
    Tables,
    /// Everything: tables with their columns and constraints (depth 0).
    All,
}

/// The arguments of ADBC's `GetObjects` after `depth`. `None` is "do not filter".
#[derive(Debug, Clone, Copy, Default)]
pub struct ObjectFilter<'a> {
    /// Catalog (database) name pattern.
    pub catalog: Option<&'a str>,
    /// Schema name pattern.
    pub db_schema: Option<&'a str>,
    /// Table name pattern.
    pub table_name: Option<&'a str>,
    /// Table types to keep (see [`TABLE_TYPES`]).
    pub table_types: Option<&'a [&'a str]>,
    /// Column name pattern.
    pub column_name: Option<&'a str>,
}

/// The result of [`Metadata::objects`]: one flat result per level, down to the requested depth.
/// Levels below the depth are `None`.
#[derive(Debug, Clone)]
pub struct Objects {
    /// `catalog_name`.
    pub catalogs: Rows,
    /// `catalog_name`, `db_schema_name`.
    pub schemas: Option<Rows>,
    /// `catalog_name`, `db_schema_name`, then ADBC's TABLE_SCHEMA scalars.
    pub tables: Option<Rows>,
    /// `catalog_name`, `db_schema_name`, `table_name`, then ADBC's COLUMN_SCHEMA.
    pub columns: Option<Rows>,
    /// `catalog_name`, `db_schema_name`, `table_name`, then the constraint fields.
    pub constraints: Option<Rows>,
}

/// An ADBC field: name, type and whether the specification marks it `not null`.
pub type AdbcField = (&'static str, ColumnType, bool);

/// TABLE_SCHEMA's scalar fields. (`table_columns` and `table_constraints` are the lists that the
/// column and constraint results replace.)
pub const TABLE_FIELDS: &[AdbcField] = &[
    ("table_name", ColumnType::Utf8, true),
    ("table_type", ColumnType::Utf8, true),
];

/// COLUMN_SCHEMA, complete.
pub const COLUMN_FIELDS: &[AdbcField] = &[
    ("column_name", ColumnType::Utf8, true),
    ("ordinal_position", ColumnType::Int32, false),
    ("remarks", ColumnType::Utf8, false),
    ("xdbc_data_type", ColumnType::Int16, false),
    ("xdbc_type_name", ColumnType::Utf8, false),
    ("xdbc_column_size", ColumnType::Int32, false),
    ("xdbc_decimal_digits", ColumnType::Int16, false),
    ("xdbc_num_prec_radix", ColumnType::Int16, false),
    ("xdbc_nullable", ColumnType::Int16, false),
    ("xdbc_column_def", ColumnType::Utf8, false),
    ("xdbc_sql_data_type", ColumnType::Int16, false),
    ("xdbc_datetime_sub", ColumnType::Int16, false),
    ("xdbc_char_octet_length", ColumnType::Int32, false),
    ("xdbc_is_nullable", ColumnType::Utf8, false),
    ("xdbc_scope_catalog", ColumnType::Utf8, false),
    ("xdbc_scope_schema", ColumnType::Utf8, false),
    ("xdbc_scope_table", ColumnType::Utf8, false),
    ("xdbc_is_autoincrement", ColumnType::Bool, false),
    ("xdbc_is_generatedcolumn", ColumnType::Bool, false),
];

/// CONSTRAINT_SCHEMA's scalar fields. (`constraint_column_names` and `constraint_column_usage` are
/// lists: see the module documentation for how the constraint result carries them.)
pub const CONSTRAINT_FIELDS: &[AdbcField] = &[
    ("constraint_name", ColumnType::Utf8, false),
    ("constraint_type", ColumnType::Utf8, true),
];

/// USAGE_SCHEMA, complete: the referenced side of a foreign key.
pub const USAGE_FIELDS: &[AdbcField] = &[
    ("fk_catalog", ColumnType::Utf8, false),
    ("fk_db_schema", ColumnType::Utf8, false),
    ("fk_table", ColumnType::Utf8, true),
    ("fk_column_name", ColumnType::Utf8, true),
];

/// The table types this driver reports (`GetTableTypes`), as `table_type` values.
pub const TABLE_TYPES: &[&str] = &[
    "table",
    "partitioned table",
    "view",
    "materialized view",
    "foreign table",
];

fn fields(parts: &[&[AdbcField]], extra: &[(&str, ColumnType)]) -> Vec<Field> {
    let mut v = Vec::new();
    for p in parts {
        v.extend(p.iter().map(|(n, t, _)| Field::new(*n, *t)));
    }
    v.extend(extra.iter().map(|(n, t)| Field::new(*n, *t)));
    v
}

const KEYS_CATALOG: &[(&str, ColumnType)] = &[("catalog_name", ColumnType::Utf8)];
const KEYS_SCHEMA: &[(&str, ColumnType)] = &[
    ("catalog_name", ColumnType::Utf8),
    ("db_schema_name", ColumnType::Utf8),
];

/// The schema of [`Metadata::catalogs`].
pub fn catalogs_fields() -> Vec<Field> {
    fields(&[], KEYS_CATALOG)
}

/// The schema of [`Metadata::db_schemas`].
pub fn db_schemas_fields() -> Vec<Field> {
    fields(&[], KEYS_SCHEMA)
}

/// The schema of [`Metadata::tables`]: the two keys, then TABLE_SCHEMA's scalars.
pub fn tables_fields() -> Vec<Field> {
    let mut v = fields(&[], KEYS_SCHEMA);
    v.extend(TABLE_FIELDS.iter().map(|(n, t, _)| Field::new(*n, *t)));
    v
}

/// The schema of [`Metadata::columns`]: the three keys, then COLUMN_SCHEMA.
pub fn columns_fields() -> Vec<Field> {
    let mut v = fields(&[], KEYS_SCHEMA);
    v.push(Field::new("table_name", ColumnType::Utf8));
    v.extend(COLUMN_FIELDS.iter().map(|(n, t, _)| Field::new(*n, *t)));
    v
}

/// The schema of [`Metadata::constraints`]: the three keys, the constraint's name and type, one
/// constrained column with its position, and the referenced side of a foreign key.
pub fn constraints_fields() -> Vec<Field> {
    let mut v = fields(&[], KEYS_SCHEMA);
    v.push(Field::new("table_name", ColumnType::Utf8));
    v.extend(CONSTRAINT_FIELDS.iter().map(|(n, t, _)| Field::new(*n, *t)));
    v.push(Field::new("constraint_column_name", ColumnType::Utf8));
    v.push(Field::new("constraint_column_position", ColumnType::Int32));
    v.extend(USAGE_FIELDS.iter().map(|(n, t, _)| Field::new(*n, *t)));
    v
}

/// The schema of [`Metadata::table_types`].
pub fn table_types_fields() -> Vec<Field> {
    vec![Field::new("table_type", ColumnType::Utf8)]
}

/// Catalog queries on a connection. See [`Connection::metadata`].
#[derive(Debug)]
pub struct Metadata<'c> {
    conn: &'c mut Connection,
}

impl Connection {
    /// Catalog metadata (databases, schemas, tables, columns, constraints) as chunks.
    pub fn metadata(&mut self) -> Metadata<'_> {
        Metadata { conn: self }
    }
}

/// A pattern parameter: `None` is SQL NULL, which the queries read as "no filter".
fn pat<'a>(p: Option<&'a str>) -> Value<'a> {
    p.map_or(Value::Null, Value::Text)
}

/// The expression for ADBC's type code (`java.sql.Types`) of a column, from its base type name `t`.
const JDBC_TYPE: &str = "CASE \
    WHEN bt.typcategory = 'A' OR t.typcategory = 'A' THEN 2003 \
    WHEN et.typname = 'bool' THEN 16 \
    WHEN et.typname = 'int2' THEN 5 \
    WHEN et.typname = 'int4' THEN 4 \
    WHEN et.typname = 'int8' THEN -5 \
    WHEN et.typname = 'float4' THEN 7 \
    WHEN et.typname = 'float8' THEN 8 \
    WHEN et.typname = 'numeric' THEN 2 \
    WHEN et.typname = 'bpchar' THEN 1 \
    WHEN et.typname IN ('varchar', 'text', 'name') OR et.typtype = 'e' THEN 12 \
    WHEN et.typname = 'bytea' THEN -2 \
    WHEN et.typname = 'date' THEN 91 \
    WHEN et.typname IN ('time', 'timetz') THEN 92 \
    WHEN et.typname IN ('timestamp', 'timestamptz') THEN 93 \
    ELSE 1111 END";

const COLUMNS_SQL: &str = r#"
SELECT current_database()::text AS catalog_name,
       n.nspname::text AS db_schema_name,
       c.relname::text AS table_name,
       a.attname::text AS column_name,
       (SELECT count(*) FROM pg_attribute a2
         WHERE a2.attrelid = a.attrelid AND a2.attnum > 0 AND NOT a2.attisdropped
           AND a2.attnum <= a.attnum)::int4 AS ordinal_position,
       d.description::text AS remarks,
       (JDBC_TYPE)::int2 AS xdbc_data_type,
       et.typname::text AS xdbc_type_name,
       (CASE WHEN et.typname = 'numeric' THEN (CASE WHEN a.atttypmod >= 4 THEN ((a.atttypmod - 4) >> 16) & 65535 END)
             WHEN et.typname IN ('varchar', 'bpchar') THEN (CASE WHEN a.atttypmod >= 4 THEN a.atttypmod - 4 END)
             WHEN et.typname IN ('bit', 'varbit') THEN (CASE WHEN a.atttypmod > 0 THEN a.atttypmod END)
             WHEN et.typlen > 0 THEN et.typlen::int4 END)::int4 AS xdbc_column_size,
       (CASE WHEN et.typname = 'numeric' THEN (CASE WHEN a.atttypmod >= 4 THEN (a.atttypmod - 4) & 2047 END)
             WHEN et.typname IN ('int2', 'int4', 'int8') THEN 0
             WHEN et.typname IN ('time', 'timetz', 'timestamp', 'timestamptz', 'interval')
               THEN (CASE WHEN a.atttypmod >= 0 THEN a.atttypmod ELSE 6 END) END)::int2 AS xdbc_decimal_digits,
       (CASE WHEN et.typname IN ('numeric', 'int2', 'int4', 'int8') THEN 10
             WHEN et.typname IN ('float4', 'float8') THEN 2 END)::int2 AS xdbc_num_prec_radix,
       (CASE WHEN a.attnotnull THEN 0 ELSE 1 END)::int2 AS xdbc_nullable,
       pg_get_expr(ad.adbin, ad.adrelid)::text AS xdbc_column_def,
       (JDBC_TYPE)::int2 AS xdbc_sql_data_type,
       (CASE et.typname WHEN 'date' THEN 1 WHEN 'time' THEN 2 WHEN 'timetz' THEN 2
                        WHEN 'timestamp' THEN 3 WHEN 'timestamptz' THEN 3 END)::int2 AS xdbc_datetime_sub,
       (CASE WHEN et.oid IN (25, 1042, 1043)
             THEN (CASE WHEN a.atttypmod = -1 THEN 1073741824
                        ELSE (a.atttypmod - 4) * pg_encoding_max_length(
                               (SELECT encoding FROM pg_database WHERE datname = current_database())) END)
        END)::int4 AS xdbc_char_octet_length,
       (CASE WHEN a.attnotnull THEN 'NO' ELSE 'YES' END)::text AS xdbc_is_nullable,
       NULL::text AS xdbc_scope_catalog,
       NULL::text AS xdbc_scope_schema,
       NULL::text AS xdbc_scope_table,
       (a.attidentity::text <> '' OR coalesce(pg_get_expr(ad.adbin, ad.adrelid) LIKE 'nextval(%', false))::bool AS xdbc_is_autoincrement,
       (a.attgenerated::text <> '')::bool AS xdbc_is_generatedcolumn
  FROM pg_attribute a
  JOIN pg_class c ON c.oid = a.attrelid
  JOIN pg_namespace n ON n.oid = c.relnamespace
  JOIN pg_type t ON t.oid = a.atttypid
  LEFT JOIN pg_type bt ON t.typtype = 'd' AND bt.oid = t.typbasetype
  JOIN pg_type et ON et.oid = coalesce(bt.oid, t.oid)
  LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum
  LEFT JOIN pg_description d ON d.objoid = a.attrelid AND d.objsubid = a.attnum AND d.classoid = 'pg_class'::regclass
 WHERE a.attnum > 0 AND NOT a.attisdropped
   AND c.relkind IN ('r', 'p', 'v', 'm', 'f')
   AND n.nspname NOT LIKE 'pg\_toast%' AND n.nspname NOT LIKE 'pg\_temp\_%'
   AND ($1::text IS NULL OR current_database() LIKE $1 ESCAPE '')
   AND ($2::text IS NULL OR n.nspname LIKE $2 ESCAPE '')
   AND ($3::text IS NULL OR c.relname LIKE $3 ESCAPE '')
   AND ($4::text[] IS NULL OR (CASE c.relkind WHEN 'r' THEN 'table' WHEN 'p' THEN 'partitioned table'
        WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized view' ELSE 'foreign table' END) = ANY($4))
   AND ($5::text IS NULL OR a.attname LIKE $5 ESCAPE '')
 ORDER BY 2, 3, 5
"#;

const CONSTRAINTS_SQL: &str = r#"
SELECT current_database()::text AS catalog_name,
       n.nspname::text AS db_schema_name,
       c.relname::text AS table_name,
       k.conname::text AS constraint_name,
       (CASE k.contype WHEN 'p' THEN 'PRIMARY KEY' WHEN 'u' THEN 'UNIQUE' WHEN 'f' THEN 'FOREIGN KEY' ELSE 'CHECK' END)::text AS constraint_type,
       a.attname::text AS constraint_column_name,
       u.pos::int4 AS constraint_column_position,
       (CASE WHEN k.contype = 'f' THEN current_database() END)::text AS fk_catalog,
       fn.nspname::text AS fk_db_schema,
       fc.relname::text AS fk_table,
       fa.attname::text AS fk_column_name
  FROM pg_constraint k
  JOIN pg_class c ON c.oid = k.conrelid
  JOIN pg_namespace n ON n.oid = c.relnamespace
  LEFT JOIN LATERAL unnest(k.conkey) WITH ORDINALITY AS u(attnum, pos) ON true
  LEFT JOIN pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = u.attnum
  LEFT JOIN pg_class fc ON k.contype = 'f' AND fc.oid = k.confrelid
  LEFT JOIN pg_namespace fn ON fn.oid = fc.relnamespace
  LEFT JOIN pg_attribute fa ON k.contype = 'f' AND fa.attrelid = k.confrelid AND fa.attnum = k.confkey[u.pos]
 WHERE k.contype IN ('p', 'u', 'f', 'c')
   AND n.nspname NOT LIKE 'pg\_toast%' AND n.nspname NOT LIKE 'pg\_temp\_%'
   AND ($1::text IS NULL OR current_database() LIKE $1 ESCAPE '')
   AND ($2::text IS NULL OR n.nspname LIKE $2 ESCAPE '')
   AND ($3::text IS NULL OR c.relname LIKE $3 ESCAPE '')
   AND ($4::text[] IS NULL OR (CASE c.relkind WHEN 'r' THEN 'table' WHEN 'p' THEN 'partitioned table'
        WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized view' ELSE 'foreign table' END) = ANY($4))
 ORDER BY 2, 3, 4, 7
"#;

impl Metadata<'_> {
    fn run(&mut self, sql: &str, params: &[Value<'_>], fields: Vec<Field>) -> Result<Rows> {
        let mut cur = self.conn.query(sql, params)?;
        let mut chunks = Vec::new();
        while let Some(ch) = cur.next_chunk()? {
            chunks.push(ch.clone());
        }
        Ok(Rows { fields, chunks })
    }

    /// The catalogs: the current database, if it matches the pattern. ADBC `GetObjects` depth 1.
    pub fn catalogs(&mut self, catalog: Option<&str>) -> Result<Rows> {
        self.run(
            "SELECT current_database()::text AS catalog_name \
             WHERE $1::text IS NULL OR current_database() LIKE $1 ESCAPE ''",
            &[pat(catalog)],
            catalogs_fields(),
        )
    }

    /// The schemas, in name order.
    pub fn db_schemas(&mut self, catalog: Option<&str>, db_schema: Option<&str>) -> Result<Rows> {
        self.run(
            r#"SELECT current_database()::text AS catalog_name, n.nspname::text AS db_schema_name
                 FROM pg_namespace n
                WHERE n.nspname NOT LIKE 'pg\_toast%' AND n.nspname NOT LIKE 'pg\_temp\_%'
                  AND ($1::text IS NULL OR current_database() LIKE $1 ESCAPE '')
                  AND ($2::text IS NULL OR n.nspname LIKE $2 ESCAPE '')
                ORDER BY 2"#,
            &[pat(catalog), pat(db_schema)],
            db_schemas_fields(),
        )
    }

    /// The tables, views, materialized views, foreign tables and partitioned tables.
    pub fn tables(&mut self, f: &ObjectFilter<'_>) -> Result<Rows> {
        let types = table_types_param(f.table_types);
        self.run(
            r#"SELECT current_database()::text AS catalog_name, n.nspname::text AS db_schema_name,
                      c.relname::text AS table_name,
                      (CASE c.relkind WHEN 'r' THEN 'table' WHEN 'p' THEN 'partitioned table'
                        WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized view' ELSE 'foreign table' END)::text AS table_type
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f')
                  AND n.nspname NOT LIKE 'pg\_toast%' AND n.nspname NOT LIKE 'pg\_temp\_%'
                  AND ($1::text IS NULL OR current_database() LIKE $1 ESCAPE '')
                  AND ($2::text IS NULL OR n.nspname LIKE $2 ESCAPE '')
                  AND ($3::text IS NULL OR c.relname LIKE $3 ESCAPE '')
                  AND ($4::text[] IS NULL OR (CASE c.relkind WHEN 'r' THEN 'table' WHEN 'p' THEN 'partitioned table'
                        WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized view' ELSE 'foreign table' END) = ANY($4))
                ORDER BY 2, 3"#,
            &[pat(f.catalog), pat(f.db_schema), pat(f.table_name), types.as_value()],
            tables_fields(),
        )
    }

    /// The columns of the matching tables, in table and ordinal order.
    pub fn columns(&mut self, f: &ObjectFilter<'_>) -> Result<Rows> {
        let types = table_types_param(f.table_types);
        let sql = COLUMNS_SQL.replace("JDBC_TYPE", JDBC_TYPE);
        self.run(
            &sql,
            &[
                pat(f.catalog),
                pat(f.db_schema),
                pat(f.table_name),
                types.as_value(),
                pat(f.column_name),
            ],
            columns_fields(),
        )
    }

    /// The primary key, unique, foreign key and check constraints of the matching tables.
    pub fn constraints(&mut self, f: &ObjectFilter<'_>) -> Result<Rows> {
        let types = table_types_param(f.table_types);
        self.run(
            CONSTRAINTS_SQL,
            &[
                pat(f.catalog),
                pat(f.db_schema),
                pat(f.table_name),
                types.as_value(),
            ],
            constraints_fields(),
        )
    }

    /// ADBC `GetTableTypes`: one `table_type` row per entry of [`TABLE_TYPES`].
    pub fn table_types(&mut self) -> Result<Rows> {
        let items: Vec<Option<&str>> = TABLE_TYPES.iter().map(|t| Some(*t)).collect();
        self.run(
            "SELECT t::text AS table_type FROM unnest($1::text[]) WITH ORDINALITY AS u(t, i) ORDER BY i",
            &[Value::Array(Array::Text(&items))],
            table_types_fields(),
        )
    }

    /// ADBC `GetTableSchema`: the columns a `SELECT *` from the table returns, with this crate's
    /// column types. Fails with the server's error (`42P01`) if the table does not exist.
    pub fn table_schema(&mut self, db_schema: Option<&str>, table: &str) -> Result<Vec<Field>> {
        let name = match db_schema {
            Some(s) => format!("{}.{}", quote_ident(s)?, quote_ident(table)?),
            None => quote_ident(table)?,
        };
        let stmt = self
            .conn
            .prepare(&format!("SELECT * FROM {name} WHERE false"), &[])?;
        Ok(stmt.fields().to_vec())
    }

    /// ADBC `GetObjects`: the levels down to `depth`, each as a flat result (see the module
    /// documentation). Filters apply to every level they name.
    pub fn objects(&mut self, depth: Depth, f: &ObjectFilter<'_>) -> Result<Objects> {
        let catalogs = self.catalogs(f.catalog)?;
        let schemas = if depth == Depth::Catalogs {
            None
        } else {
            Some(self.db_schemas(f.catalog, f.db_schema)?)
        };
        let tables = if matches!(depth, Depth::Tables | Depth::All) {
            Some(self.tables(f)?)
        } else {
            None
        };
        let (columns, constraints) = if depth == Depth::All {
            (Some(self.columns(f)?), Some(self.constraints(f)?))
        } else {
            (None, None)
        };
        Ok(Objects {
            catalogs,
            schemas,
            tables,
            columns,
            constraints,
        })
    }
}

/// The `table_type` filter as an owned array, so it can be borrowed as a [`Value`].
struct TypesParam<'a> {
    items: Option<Vec<Option<&'a str>>>,
}

impl TypesParam<'_> {
    fn as_value(&self) -> Value<'_> {
        match &self.items {
            Some(v) => Value::Array(Array::Text(v)),
            None => Value::Null,
        }
    }
}

fn table_types_param<'a>(t: Option<&'a [&'a str]>) -> TypesParam<'a> {
    TypesParam {
        items: t.map(|t| t.iter().map(|s| Some(*s)).collect()),
    }
}

// ---- GetInfo and statistics -------------------------------------------------------------------

/// INFO_SCHEMA's scalar members, in the specification's order. `info_value` is a dense union, so
/// the result carries one column per member and exactly one of them is set on a row (none, for a
/// value that is null). The union's `string_list` and `int32_to_int32_list_map` members are lists
/// and maps; no code this driver reports uses them, so they have no column.
pub const INFO_VALUE_FIELDS: &[AdbcField] = &[
    ("string_value", ColumnType::Utf8, false),
    ("bool_value", ColumnType::Bool, false),
    ("int64_value", ColumnType::Int64, false),
    ("int32_bitmask", ColumnType::Int32, false),
];

/// The key of a `GetInfo` row.
pub const INFO_NAME_FIELD: AdbcField = ("info_name", ColumnType::UInt32, true);

/// STATISTICS_SCHEMA's scalar fields before `statistic_value`.
pub const STATISTICS_FIELDS: &[AdbcField] = &[
    ("table_name", ColumnType::Utf8, true),
    ("column_name", ColumnType::Utf8, false),
    ("statistic_key", ColumnType::Int16, true),
];

/// VALUE_SCHEMA, the dense union of `statistic_value`: one column per member, exactly one set.
pub const STATISTIC_VALUE_FIELDS: &[AdbcField] = &[
    ("int64", ColumnType::Int64, false),
    ("uint64", ColumnType::UInt64, false),
    ("float64", ColumnType::Float64, false),
    ("binary", ColumnType::Binary, false),
];

/// STATISTICS_SCHEMA's last field.
pub const STATISTIC_IS_APPROXIMATE_FIELD: AdbcField =
    ("statistic_is_approximate", ColumnType::Bool, true);

/// `GetStatisticNames`: names of statistics specific to this driver.
pub const STATISTIC_NAMES_FIELDS: &[AdbcField] = &[
    ("statistic_name", ColumnType::Utf8, true),
    ("statistic_key", ColumnType::Int16, true),
];

/// ADBC's `ADBC_INFO_*` codes this driver answers.
pub mod info {
    /// The server's product name (utf8).
    pub const VENDOR_NAME: u32 = 0;
    /// The server version (utf8).
    pub const VENDOR_VERSION: u32 = 1;
    /// Whether SQL is supported (bool).
    pub const VENDOR_SQL: u32 = 3;
    /// Whether Substrait is supported (bool).
    pub const VENDOR_SUBSTRAIT: u32 = 4;
    /// The minimum Substrait version: null, Substrait is not supported (utf8).
    pub const VENDOR_SUBSTRAIT_MIN_VERSION: u32 = 5;
    /// The maximum Substrait version: null, Substrait is not supported (utf8).
    pub const VENDOR_SUBSTRAIT_MAX_VERSION: u32 = 6;
    /// The driver name (utf8).
    pub const DRIVER_NAME: u32 = 100;
    /// The driver version (utf8).
    pub const DRIVER_VERSION: u32 = 101;
    /// The ADBC API version the driver follows (int64): `ADBC_VERSION_1_1_0` is 1001000.
    pub const DRIVER_ADBC_VERSION: u32 = 103;
    /// Codes `GetInfo` has no answer for here: the vendor's and the driver's Arrow library versions
    /// (2 and 102). PostgreSQL has no Arrow library and the driver uses its own chunk layout, so
    /// these rows are omitted, as ADBC allows for codes a driver does not recognise.
    pub const NOT_REPORTED: &[u32] = &[2, 102];
}

/// ADBC's standard statistic keys (`ADBC_STATISTIC_*_KEY`).
pub mod statistic {
    /// Average size in bytes of a value in the column (float64).
    pub const AVERAGE_BYTE_WIDTH: i16 = 0;
    /// Number of distinct values (int64 if exact, float64 if approximate).
    pub const DISTINCT_COUNT: i16 = 1;
    /// Largest size in bytes of a value in the column (int64 exact, float64 approximate).
    pub const MAX_BYTE_WIDTH: i16 = 2;
    /// Largest value (column-dependent type).
    pub const MAX_VALUE: i16 = 3;
    /// Smallest value (column-dependent type).
    pub const MIN_VALUE: i16 = 4;
    /// Number of NULLs (int64 exact, float64 approximate).
    pub const NULL_COUNT: i16 = 5;
    /// Number of rows (int64 exact, float64 approximate).
    pub const ROW_COUNT: i16 = 6;
}

/// The schema of [`Metadata::info`]: `info_name`, then INFO_SCHEMA's scalar members.
pub fn info_fields() -> Vec<Field> {
    let mut v = vec![Field::new(INFO_NAME_FIELD.0, INFO_NAME_FIELD.1)];
    v.extend(INFO_VALUE_FIELDS.iter().map(|(n, t, _)| Field::new(*n, *t)));
    v
}

/// The schema of [`Metadata::statistics`]: the two keys, STATISTICS_SCHEMA's fields with
/// `statistic_value` replaced by VALUE_SCHEMA's members, and `statistic_is_approximate`.
pub fn statistics_fields() -> Vec<Field> {
    let mut v = fields(&[], KEYS_SCHEMA);
    v.extend(STATISTICS_FIELDS.iter().map(|(n, t, _)| Field::new(*n, *t)));
    v.extend(
        STATISTIC_VALUE_FIELDS
            .iter()
            .map(|(n, t, _)| Field::new(*n, *t)),
    );
    v.push(Field::new(
        STATISTIC_IS_APPROXIMATE_FIELD.0,
        STATISTIC_IS_APPROXIMATE_FIELD.1,
    ));
    v
}

/// The schema of [`Metadata::statistic_names`].
pub fn statistic_names_fields() -> Vec<Field> {
    STATISTIC_NAMES_FIELDS
        .iter()
        .map(|(n, t, _)| Field::new(*n, *t))
        .collect()
}

/// Builds a result chunk by chunk.
struct Sink {
    fields: Vec<Field>,
    builder: ChunkBuilder,
    chunks: Vec<Chunk>,
}

impl Sink {
    fn new(fields: Vec<Field>) -> Self {
        Sink {
            builder: ChunkBuilder::new(fields.clone()),
            fields,
            chunks: Vec::new(),
        }
    }

    fn end_row(&mut self) -> Result<()> {
        self.builder.finish_row()?;
        if self.builder.is_full() {
            self.chunks.push(self.builder.chunk().clone());
            self.builder.reset();
        }
        Ok(())
    }

    fn col(&mut self, i: usize) -> Result<&mut karez_chunk::Column> {
        Ok(self.builder.column_mut(i)?)
    }

    fn finish(mut self) -> Rows {
        if self.builder.rows() > 0 {
            self.chunks.push(self.builder.chunk().clone());
        }
        Rows {
            fields: self.fields,
            chunks: self.chunks,
        }
    }
}

/// A statistic's value, one member of VALUE_SCHEMA.
#[derive(Debug, Clone, PartialEq)]
enum Stat {
    I64(i64),
    F64(f64),
    Bytes(Vec<u8>),
}

struct StatRow {
    catalog: String,
    schema: String,
    table: String,
    column: Option<String>,
    key: i16,
    value: Stat,
    approximate: bool,
}

impl Metadata<'_> {
    /// ADBC `GetInfo`: facts about the server and this driver. `codes` selects some of them
    /// (`None` for all); codes this driver does not know are left out of the result. The server
    /// version is the one the server reported at startup, so no query is sent.
    pub fn info(&mut self, codes: Option<&[u32]>) -> Result<Rows> {
        enum V {
            Str(String),
            Bool(bool),
            I64(i64),
            Null,
        }
        let version = self
            .conn
            .server_param("server_version")
            .unwrap_or("")
            .to_string();
        let all: Vec<(u32, V)> = vec![
            (info::VENDOR_NAME, V::Str("PostgreSQL".into())),
            (info::VENDOR_VERSION, V::Str(version)),
            (info::VENDOR_SQL, V::Bool(true)),
            (info::VENDOR_SUBSTRAIT, V::Bool(false)),
            (info::VENDOR_SUBSTRAIT_MIN_VERSION, V::Null),
            (info::VENDOR_SUBSTRAIT_MAX_VERSION, V::Null),
            (info::DRIVER_NAME, V::Str("karez-pg".into())),
            (
                info::DRIVER_VERSION,
                V::Str(env!("CARGO_PKG_VERSION").into()),
            ),
            (info::DRIVER_ADBC_VERSION, V::I64(1_001_000)),
        ];
        let mut sink = Sink::new(info_fields());
        for (code, v) in all {
            if codes.is_some_and(|c| !c.contains(&code)) {
                continue;
            }
            sink.col(0)?.push_u32(code)?;
            for i in 1..=4 {
                let c = sink.col(i)?;
                match (i, &v) {
                    (1, V::Str(s)) => c.push_str(s)?,
                    (2, V::Bool(b)) => c.push_bool(*b)?,
                    (3, V::I64(n)) => c.push_i64(*n)?,
                    _ => c.push_null()?,
                }
            }
            sink.end_row()?;
        }
        Ok(sink.finish())
    }

    /// ADBC `GetStatisticNames`: the statistics specific to this driver. There are none; the
    /// standard ones are the keys in [`statistic`].
    pub fn statistic_names(&mut self) -> Result<Rows> {
        Ok(Sink::new(statistic_names_fields()).finish())
    }

    /// ADBC `GetStatistics` for the tables that match the patterns (and that this role may read).
    ///
    /// With `approximate` the numbers come from the planner's statistics (`pg_class.reltuples` and
    /// `pg_stats`), as `float64` and marked approximate: the row count of each table, and for each
    /// analysed column its average width, distinct count and null count. A table that was never
    /// analysed has none. Without it every statistic is computed exactly by reading the tables,
    /// which can be slow, as `int64` (the average width as `float64`): the row count, and per
    /// column the null count, distinct count, average and largest byte width, and for integer,
    /// floating-point and text columns the smallest and largest value (text as UTF-8 bytes).
    pub fn statistics(&mut self, f: &ObjectFilter<'_>, approximate: bool) -> Result<Rows> {
        let mut out: Vec<StatRow> = Vec::new();
        if approximate {
            self.approximate_statistics(f, &mut out)?;
        } else {
            self.exact_statistics(f, &mut out)?;
        }
        let mut sink = Sink::new(statistics_fields());
        for r in out {
            sink.col(0)?.push_str(&r.catalog)?;
            sink.col(1)?.push_str(&r.schema)?;
            sink.col(2)?.push_str(&r.table)?;
            match &r.column {
                Some(c) => sink.col(3)?.push_str(c)?,
                None => sink.col(3)?.push_null()?,
            }
            sink.col(4)?.push_i16(r.key)?;
            for i in 5..=8 {
                let c = sink.col(i)?;
                match (i, &r.value) {
                    (5, Stat::I64(v)) => c.push_i64(*v)?,
                    (7, Stat::F64(v)) => c.push_f64(*v)?,
                    (8, Stat::Bytes(v)) => c.push_bytes(v)?,
                    _ => c.push_null()?,
                }
            }
            sink.col(9)?.push_bool(r.approximate)?;
            sink.end_row()?;
        }
        Ok(sink.finish())
    }

    fn approximate_statistics(
        &mut self,
        f: &ObjectFilter<'_>,
        out: &mut Vec<StatRow>,
    ) -> Result<()> {
        let params = [pat(f.catalog), pat(f.db_schema), pat(f.table_name)];
        let mut cur = self.conn.query(
            r#"SELECT current_database()::text, n.nspname::text, c.relname::text, NULL::text,
                      6::int2, c.reltuples::float8
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                WHERE c.relkind IN ('r', 'p', 'm') AND c.reltuples >= 0
                  AND n.nspname NOT LIKE 'pg\_toast%' AND n.nspname NOT LIKE 'pg\_temp\_%'
                  AND has_table_privilege(c.oid, 'SELECT')
                  AND ($1::text IS NULL OR current_database() LIKE $1 ESCAPE '')
                  AND ($2::text IS NULL OR n.nspname LIKE $2 ESCAPE '')
                  AND ($3::text IS NULL OR c.relname LIKE $3 ESCAPE '')
               UNION ALL
               SELECT current_database()::text, s.schemaname::text, s.tablename::text, s.attname::text,
                      k.key::int2, k.val::float8
                 FROM pg_stats s
                 JOIN pg_namespace n ON n.nspname = s.schemaname
                 JOIN pg_class c ON c.relnamespace = n.oid AND c.relname = s.tablename
                CROSS JOIN LATERAL (VALUES
                      (0, s.avg_width::float8),
                      (1, CASE WHEN s.n_distinct >= 0 THEN s.n_distinct::float8
                               ELSE -s.n_distinct::float8 * c.reltuples::float8 END),
                      (5, s.null_frac::float8 * c.reltuples::float8)) k(key, val)
                WHERE c.relkind IN ('r', 'p', 'm') AND c.reltuples >= 0
                  AND s.schemaname NOT LIKE 'pg\_toast%' AND s.schemaname NOT LIKE 'pg\_temp\_%'
                  AND ($1::text IS NULL OR current_database() LIKE $1 ESCAPE '')
                  AND ($2::text IS NULL OR s.schemaname LIKE $2 ESCAPE '')
                  AND ($3::text IS NULL OR s.tablename LIKE $3 ESCAPE '')
                ORDER BY 2, 3, 4 NULLS FIRST, 5"#,
            &params,
        )?;
        while let Some(r) = cur.next()? {
            out.push(StatRow {
                catalog: r.get(0)?,
                schema: r.get(1)?,
                table: r.get(2)?,
                column: r.get(3)?,
                key: r.get(4)?,
                value: Stat::F64(r.get(5)?),
                approximate: true,
            });
        }
        Ok(())
    }

    fn exact_statistics(&mut self, f: &ObjectFilter<'_>, out: &mut Vec<StatRow>) -> Result<()> {
        let params = [pat(f.catalog), pat(f.db_schema), pat(f.table_name)];
        let mut tables: Vec<(String, String, String)> = Vec::new();
        {
            let mut cur = self.conn.query(
                r#"SELECT current_database()::text, n.nspname::text, c.relname::text
                     FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                    WHERE c.relkind IN ('r', 'p', 'm')
                      AND n.nspname NOT LIKE 'pg\_toast%' AND n.nspname NOT LIKE 'pg\_temp\_%'
                      AND has_table_privilege(c.oid, 'SELECT')
                      AND ($1::text IS NULL OR current_database() LIKE $1 ESCAPE '')
                      AND ($2::text IS NULL OR n.nspname LIKE $2 ESCAPE '')
                      AND ($3::text IS NULL OR c.relname LIKE $3 ESCAPE '')
                    ORDER BY 2, 3"#,
                &params,
            )?;
            while let Some(r) = cur.next()? {
                tables.push((r.get(0)?, r.get(1)?, r.get(2)?));
            }
        }
        for (catalog, schema, table) in tables {
            // the columns, with the base type name that decides which extremes make sense
            let mut cols: Vec<(String, String)> = Vec::new();
            {
                let mut cur = self.conn.query(
                    r#"SELECT a.attname::text, et.typname::text
                         FROM pg_attribute a
                         JOIN pg_class c ON c.oid = a.attrelid
                         JOIN pg_namespace n ON n.oid = c.relnamespace
                         JOIN pg_type t ON t.oid = a.atttypid
                         LEFT JOIN pg_type bt ON t.typtype = 'd' AND bt.oid = t.typbasetype
                         JOIN pg_type et ON et.oid = coalesce(bt.oid, t.oid)
                        WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped
                        ORDER BY a.attnum"#,
                    &[Value::Text(&schema), Value::Text(&table)],
                )?;
                while let Some(r) = cur.next()? {
                    cols.push((r.get(0)?, r.get(1)?));
                }
            }
            let rel = format!("{}.{}", quote_ident(&schema)?, quote_ident(&table)?);
            // one scan per table: the row count, then for each column its statistics
            let mut select = String::from("SELECT count(*)::int8");
            let mut plan: Vec<(usize, i16, Kind)> = Vec::new(); // (column, key, how to read it)
            for (i, (name, ty)) in cols.iter().enumerate() {
                let q = quote_ident(name)?;
                let mut add = |expr: String, key: i16, kind: Kind, plan: &mut Vec<_>| {
                    select.push_str(&format!(", ({expr})"));
                    plan.push((i, key, kind));
                };
                add(
                    format!("count(*) - count({q})"),
                    statistic::NULL_COUNT,
                    Kind::I64,
                    &mut plan,
                );
                add(
                    format!("count(DISTINCT {q})"),
                    statistic::DISTINCT_COUNT,
                    Kind::I64,
                    &mut plan,
                );
                add(
                    format!("avg(pg_column_size({q}))::float8"),
                    statistic::AVERAGE_BYTE_WIDTH,
                    Kind::F64,
                    &mut plan,
                );
                add(
                    format!("max(pg_column_size({q}))::int8"),
                    statistic::MAX_BYTE_WIDTH,
                    Kind::I64,
                    &mut plan,
                );
                match ty.as_str() {
                    "int2" | "int4" | "int8" => {
                        add(
                            format!("min({q})::int8"),
                            statistic::MIN_VALUE,
                            Kind::I64,
                            &mut plan,
                        );
                        add(
                            format!("max({q})::int8"),
                            statistic::MAX_VALUE,
                            Kind::I64,
                            &mut plan,
                        );
                    }
                    "float4" | "float8" => {
                        add(
                            format!("min({q})::float8"),
                            statistic::MIN_VALUE,
                            Kind::F64,
                            &mut plan,
                        );
                        add(
                            format!("max({q})::float8"),
                            statistic::MAX_VALUE,
                            Kind::F64,
                            &mut plan,
                        );
                    }
                    "text" | "varchar" | "bpchar" => {
                        add(
                            format!("convert_to(min({q})::text, 'UTF8')"),
                            statistic::MIN_VALUE,
                            Kind::Bytes,
                            &mut plan,
                        );
                        add(
                            format!("convert_to(max({q})::text, 'UTF8')"),
                            statistic::MAX_VALUE,
                            Kind::Bytes,
                            &mut plan,
                        );
                    }
                    _ => {}
                }
            }
            select.push_str(&format!(" FROM {rel}"));
            let mut cur = self.conn.query(&select, &[])?;
            let Some(r) = cur.next()? else { continue };
            out.push(StatRow {
                catalog: catalog.clone(),
                schema: schema.clone(),
                table: table.clone(),
                column: None,
                key: statistic::ROW_COUNT,
                value: Stat::I64(r.get(0)?),
                approximate: false,
            });
            for (n, (col, key, kind)) in plan.iter().enumerate() {
                let i = n + 1;
                if r.is_null(i)? {
                    continue; // an empty table has no average, minimum or maximum
                }
                let value = match kind {
                    Kind::I64 => Stat::I64(r.get(i)?),
                    Kind::F64 => Stat::F64(r.get(i)?),
                    Kind::Bytes => Stat::Bytes(r.get(i)?),
                };
                out.push(StatRow {
                    catalog: catalog.clone(),
                    schema: schema.clone(),
                    table: table.clone(),
                    column: Some(cols[*col].0.clone()),
                    key: *key,
                    value,
                    approximate: false,
                });
            }
        }
        out.sort_by(|a, b| {
            (&a.schema, &a.table, &a.column, a.key).cmp(&(&b.schema, &b.table, &b.column, b.key))
        });
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Kind {
    I64,
    F64,
    Bytes,
}
