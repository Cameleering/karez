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
use karez_chunk::{Chunk, ColumnType, Field};
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
       NULL::int2 AS xdbc_datetime_sub,
       NULL::int4 AS xdbc_char_octet_length,
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
