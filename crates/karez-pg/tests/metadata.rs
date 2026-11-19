//! Catalog metadata (`Connection::metadata`): the schemas compared field by field with the ADBC
//! specification, and the contents checked against a purpose-built fixture and against
//! `information_schema`.
//!
//! The first test needs no server. The rest need `KAREZ_TEST_PG` (see tests/docker/README.md) and
//! print `SKIPPED` without it, unless `KAREZ_REQUIRE_SERVER=1`.

use karez_pg::metadata::{
    self, columns_fields, constraints_fields, db_schemas_fields, info_fields,
    statistic_names_fields, statistics_fields, table_types_fields, tables_fields, Depth,
    ObjectFilter, Rows,
};
use karez_pg::{Chunk, ColumnType, Connection, Error, Field};
use std::collections::BTreeSet;
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

// ---- the schemas against the specification --------------------------------------------------

/// One field of a schema table in the specification: name, type text, `not null`.
#[derive(Debug, PartialEq)]
struct SpecField {
    name: String,
    ty: String,
    not_null: bool,
}

/// The tables in `adbc/get_objects_schema.txt`, in order: GetObjects' top level, DB_SCHEMA_SCHEMA,
/// TABLE_SCHEMA, COLUMN_SCHEMA, CONSTRAINT_SCHEMA, USAGE_SCHEMA, then GetTableTypes.
fn spec_tables() -> Vec<Vec<SpecField>> {
    let text = include_str!("adbc/get_objects_schema.txt");
    let mut tables: Vec<Vec<SpecField>> = Vec::new();
    for line in text.lines().filter(|l| l.starts_with('|')) {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let (name, ty) = (cells[1], cells[2]);
        if name == "Field Name" {
            tables.push(Vec::new());
        } else if !name.starts_with('-') {
            let not_null = ty.ends_with("not null");
            tables.last_mut().expect("a header first").push(SpecField {
                name: name.to_string(),
                ty: ty.trim_end_matches("not null").trim().to_string(),
                not_null,
            });
        }
    }
    tables
}

fn arrow_type(t: &str) -> Option<ColumnType> {
    match t {
        "utf8" => Some(ColumnType::Utf8),
        "int16" => Some(ColumnType::Int16),
        "int32" => Some(ColumnType::Int32),
        "int64" => Some(ColumnType::Int64),
        "bool" => Some(ColumnType::Bool),
        "uint32" => Some(ColumnType::UInt32),
        "uint64" => Some(ColumnType::UInt64),
        "float64" => Some(ColumnType::Float64),
        "binary" => Some(ColumnType::Binary),
        _ => None, // a list, a map, a union or a struct
    }
}

/// The scalar fields of a spec table as (name, type, not null), and the names of its list fields.
fn split(spec: &[SpecField]) -> (Vec<(String, ColumnType, bool)>, Vec<String>) {
    let (mut scalars, mut lists) = (Vec::new(), Vec::new());
    for f in spec {
        match arrow_type(&f.ty) {
            Some(t) => scalars.push((f.name.clone(), t, f.not_null)),
            None => {
                // a list, a map, or a union or struct named by its schema
                lists.push(f.name.clone());
            }
        }
    }
    (scalars, lists)
}

fn ours(consts: &[metadata::AdbcField]) -> Vec<(String, ColumnType, bool)> {
    consts
        .iter()
        .map(|(n, t, nn)| (n.to_string(), *t, *nn))
        .collect()
}

#[test]
fn the_schemas_equal_adbc_1_1_0_field_by_field() {
    let names = |f: &[Field]| f.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
    let t = spec_tables();
    assert_eq!(t.len(), 14, "the specification excerpt has fourteen tables");

    // GetObjects: catalog_name, then the list of schemas
    let (s, l) = split(&t[0]);
    assert_eq!(
        s,
        vec![("catalog_name".to_string(), ColumnType::Utf8, false)]
    );
    assert_eq!(l, ["catalog_db_schemas"]);
    // DB_SCHEMA_SCHEMA: db_schema_name, then the list of tables
    let (s, l) = split(&t[1]);
    assert_eq!(
        s,
        vec![("db_schema_name".to_string(), ColumnType::Utf8, false)]
    );
    assert_eq!(l, ["db_schema_tables"]);
    // TABLE_SCHEMA: two scalars, then the column and constraint lists
    let (s, l) = split(&t[2]);
    assert_eq!(s, ours(metadata::TABLE_FIELDS));
    assert_eq!(l, ["table_columns", "table_constraints"]);
    // COLUMN_SCHEMA: all 19 fields, no lists
    let (s, l) = split(&t[3]);
    assert!(l.is_empty());
    assert_eq!(s.len(), 19);
    assert_eq!(s, ours(metadata::COLUMN_FIELDS));
    // CONSTRAINT_SCHEMA: two scalars, then the column names and the usage lists
    let (s, l) = split(&t[4]);
    assert_eq!(s, ours(metadata::CONSTRAINT_FIELDS));
    assert_eq!(l, ["constraint_column_names", "constraint_column_usage"]);
    assert!(t[4][2].not_null, "constraint_column_names is `not null`");
    // USAGE_SCHEMA
    let (s, l) = split(&t[5]);
    assert!(l.is_empty());
    assert_eq!(s, ours(metadata::USAGE_FIELDS));
    // GetTableTypes
    let (s, _) = split(&t[6]);
    assert_eq!(s, vec![("table_type".to_string(), ColumnType::Utf8, true)]);
    assert_eq!(
        table_types_fields(),
        vec![Field::new("table_type", ColumnType::Utf8)]
    );

    // GetInfo: info_name, then the union of values
    let (s, l) = split(&t[7]);
    assert_eq!(s, vec![("info_name".to_string(), ColumnType::UInt32, true)]);
    assert_eq!(s, ours(&[metadata::INFO_NAME_FIELD]));
    assert_eq!(l, ["info_value"]);
    // INFO_SCHEMA: four scalar members, then a list and a map that no reported code uses
    let (s, l) = split(&t[8]);
    assert_eq!(s, ours(metadata::INFO_VALUE_FIELDS));
    assert_eq!(l, ["string_list", "int32_to_int32_list_map"]);
    // GetStatistics: catalog, then the schemas; the schema level; STATISTICS_SCHEMA; VALUE_SCHEMA
    let (s, l) = split(&t[9]);
    assert_eq!(
        s,
        vec![("catalog_name".to_string(), ColumnType::Utf8, false)]
    );
    assert_eq!(l, ["catalog_db_schemas"]);
    let (s, l) = split(&t[10]);
    assert_eq!(
        s,
        vec![("db_schema_name".to_string(), ColumnType::Utf8, false)]
    );
    assert_eq!(l, ["db_schema_statistics"]);
    let (s, l) = split(&t[11]);
    let mut want = ours(metadata::STATISTICS_FIELDS);
    want.push(ours(&[metadata::STATISTIC_IS_APPROXIMATE_FIELD]).remove(0));
    assert_eq!(s, want, "the scalars around statistic_value");
    assert_eq!(l, ["statistic_value"]);
    assert!(t[11][3].not_null, "statistic_value is `not null`");
    let (s, l) = split(&t[12]);
    assert!(l.is_empty());
    assert_eq!(s, ours(metadata::STATISTIC_VALUE_FIELDS));
    // GetStatisticNames
    let (s, l) = split(&t[13]);
    assert!(l.is_empty());
    assert_eq!(s, ours(metadata::STATISTIC_NAMES_FIELDS));
    assert_eq!(statistic_names_fields().len(), 2);
    // the flat results: keys first, then the specification's fields in its order
    assert_eq!(
        names(&info_fields()),
        [
            "info_name",
            "string_value",
            "bool_value",
            "int64_value",
            "int32_bitmask"
        ]
    );
    assert_eq!(
        names(&statistics_fields()),
        [
            "catalog_name",
            "db_schema_name",
            "table_name",
            "column_name",
            "statistic_key",
            "int64",
            "uint64",
            "float64",
            "binary",
            "statistic_is_approximate"
        ]
    );
    let names = |f: &[Field]| f.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
    assert_eq!(
        names(&db_schemas_fields()),
        ["catalog_name", "db_schema_name"]
    );
    let mut want = vec!["catalog_name".to_string(), "db_schema_name".to_string()];
    want.extend(t[2].iter().take(2).map(|f| f.name.clone()));
    assert_eq!(names(&tables_fields()), want);
    let mut want = vec![
        "catalog_name".to_string(),
        "db_schema_name".to_string(),
        "table_name".to_string(),
    ];
    want.extend(t[3].iter().map(|f| f.name.clone()));
    assert_eq!(names(&columns_fields()), want);
    let types = |f: &[Field]| f.iter().skip(3).map(|x| x.ty).collect::<Vec<_>>();
    assert_eq!(
        types(&columns_fields()),
        t[3].iter()
            .map(|f| arrow_type(&f.ty).unwrap())
            .collect::<Vec<_>>()
    );
    let mut want = vec![
        "catalog_name".to_string(),
        "db_schema_name".to_string(),
        "table_name".to_string(),
        "constraint_name".to_string(),
        "constraint_type".to_string(),
        "constraint_column_name".to_string(),
        "constraint_column_position".to_string(),
    ];
    want.extend(t[5].iter().map(|f| f.name.clone()));
    assert_eq!(names(&constraints_fields()), want);
    assert!(constraints_fields()
        .iter()
        .all(|f| f.ty != ColumnType::Float64));
}

// ---- reading results ------------------------------------------------------------------------

fn cell(ch: &Chunk, row: usize, col: usize) -> Option<String> {
    let r = ch.row(row).unwrap();
    if r.is_null(col).unwrap() {
        return None;
    }
    Some(match ch.fields()[col].ty {
        ColumnType::Utf8 => r.get::<String>(col).unwrap(),
        ColumnType::Int16 => r.get::<i16>(col).unwrap().to_string(),
        ColumnType::Int32 => r.get::<i32>(col).unwrap().to_string(),
        ColumnType::Int64 => r.get::<i64>(col).unwrap().to_string(),
        ColumnType::UInt32 => r.get::<u32>(col).unwrap().to_string(),
        ColumnType::UInt64 => r.get::<u64>(col).unwrap().to_string(),
        ColumnType::Float64 => r.get::<f64>(col).unwrap().to_string(),
        ColumnType::Binary => r
            .get::<Vec<u8>>(col)
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        ColumnType::Bool => r.get::<bool>(col).unwrap().to_string(),
        other => panic!("{other:?}"),
    })
}

/// All rows as strings, in order.
fn table(r: &Rows) -> Vec<Vec<Option<String>>> {
    let mut out = Vec::new();
    for ch in &r.chunks {
        for row in 0..ch.rows() {
            out.push((0..r.fields.len()).map(|c| cell(ch, row, c)).collect());
        }
    }
    out
}

fn col_index(r: &Rows, name: &str) -> usize {
    r.fields
        .iter()
        .position(|f| f.name == name)
        .unwrap_or_else(|| panic!("no column {name}"))
}

/// The value of column `name` for the row whose `key` column equals `val`.
fn pick(r: &Rows, key: &str, val: &str, name: &str) -> Option<String> {
    let (k, n) = (col_index(r, key), col_index(r, name));
    let rows = table(r);
    let row = rows
        .iter()
        .find(|row| row[k].as_deref() == Some(val))
        .unwrap_or_else(|| panic!("no row with {key} = {val}"));
    row[n].clone()
}

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

// ---- a fixture with awkward columns -----------------------------------------------------------

/// Drops the named statements' objects on a new connection when it goes out of scope, so a failed
/// assertion does not leave tables behind for the tests that run next to this one.
struct Cleanup {
    addr: String,
    sql: &'static [&'static str],
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Ok(mut c) = connect(&self.addr) {
            for q in self.sql {
                let _ = c.execute(q, &[]);
            }
        }
    }
}

const DROP_FIXTURE: &[&str] = &[
    "DROP MATERIALIZED VIEW IF EXISTS kmeta_mv",
    "DROP VIEW IF EXISTS kmeta_v",
    "DROP TABLE IF EXISTS kmeta_child, kmeta_parent, kmeta_part CASCADE",
    "DROP DOMAIN IF EXISTS kmeta_dom CASCADE",
];

#[test]
fn a_fixture_with_awkward_columns_is_described_exactly() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    for sql in DROP_FIXTURE {
        c.execute(sql, &[])?;
    }
    let _cleanup = Cleanup {
        addr: addr.clone(),
        sql: DROP_FIXTURE,
    };
    for sql in [
        "CREATE DOMAIN kmeta_dom AS varchar(12)",
        "CREATE TABLE kmeta_parent (a int, b int, c numeric(12,3) NOT NULL, d varchar(20), e char(5), \
         f text DEFAULT 'x', gone int, g int GENERATED ALWAYS AS (a + 1) STORED, \
         h bigint GENERATED BY DEFAULT AS IDENTITY, i int4[], j timestamp(3), k kmeta_dom, l time(0), \
         m mood, n bit(7), o numeric, p interval, \
         PRIMARY KEY (b, a), UNIQUE (c, d), CHECK (a > 0))",
        "ALTER TABLE kmeta_parent DROP COLUMN gone",
        "COMMENT ON COLUMN kmeta_parent.d IS 'the d column'",
        "CREATE TABLE kmeta_child (x int, y int, z int, FOREIGN KEY (y, x) REFERENCES kmeta_parent (a, b), \
         CONSTRAINT kmeta_child_pk PRIMARY KEY (z))",
        "CREATE VIEW kmeta_v AS SELECT a, b FROM kmeta_parent",
        "CREATE MATERIALIZED VIEW kmeta_mv AS SELECT a FROM kmeta_parent",
        "CREATE TABLE kmeta_part (id int, v int) PARTITION BY RANGE (id)",
    ] {
        c.execute(sql, &[])?;
    }
    let r = (|| -> R {
        let mut m = c.metadata();
        // tables: kinds, order, and the table type filter
        let f = ObjectFilter {
            table_name: Some("kmeta_%"),
            ..Default::default()
        };
        let t = m.tables(&f)?;
        let got: Vec<(String, String)> = table(&t)
            .into_iter()
            .map(|r| (r[2].clone().unwrap(), r[3].clone().unwrap()))
            .collect();
        let want: Vec<(String, String)> = [
            ("kmeta_child", "table"),
            ("kmeta_mv", "materialized view"),
            ("kmeta_parent", "table"),
            ("kmeta_part", "partitioned table"),
            ("kmeta_v", "view"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        assert_eq!(got, want);
        assert_eq!(t.fields, tables_fields());
        let only_views = m.tables(&ObjectFilter {
            table_name: Some("kmeta_%"),
            table_types: Some(&["view", "foreign table"]),
            ..Default::default()
        })?;
        assert_eq!(table(&only_views).len(), 1);
        assert_eq!(
            pick(&only_views, "table_name", "kmeta_v", "table_type"),
            s("view")
        );

        // columns of kmeta_parent
        let cols = m.columns(&ObjectFilter {
            table_name: Some("kmeta_parent"),
            ..Default::default()
        })?;
        assert_eq!(cols.fields, columns_fields());
        let names: Vec<String> = table(&cols).iter().map(|r| r[3].clone().unwrap()).collect();
        assert_eq!(
            names,
            ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p"],
            "the dropped column is gone"
        );
        let get = |col: &str, what: &str| pick(&cols, "column_name", col, what);
        // ordinal positions have no gap where the column was dropped
        assert_eq!(get("f", "ordinal_position"), s("6"));
        assert_eq!(get("g", "ordinal_position"), s("7"));
        assert_eq!(get("p", "ordinal_position"), s("16"));
        // keys and nullability
        assert_eq!(get("a", "xdbc_is_nullable"), s("NO"));
        assert_eq!(get("a", "xdbc_nullable"), s("0"));
        assert_eq!(get("c", "xdbc_nullable"), s("0"));
        assert_eq!(get("d", "xdbc_nullable"), s("1"));
        assert_eq!(get("d", "xdbc_is_nullable"), s("YES"));
        // integer, numeric(12,3), varchar(20), char(5), text
        assert_eq!(get("a", "xdbc_type_name"), s("int4"));
        assert_eq!(get("a", "xdbc_data_type"), s("4"));
        assert_eq!(get("a", "xdbc_sql_data_type"), s("4"));
        assert_eq!(get("a", "xdbc_column_size"), s("4"));
        assert_eq!(get("a", "xdbc_decimal_digits"), s("0"));
        assert_eq!(get("a", "xdbc_num_prec_radix"), s("10"));
        assert_eq!(get("c", "xdbc_data_type"), s("2"));
        assert_eq!(get("c", "xdbc_column_size"), s("12"));
        assert_eq!(get("c", "xdbc_decimal_digits"), s("3"));
        assert_eq!(get("d", "xdbc_data_type"), s("12"));
        assert_eq!(get("d", "xdbc_column_size"), s("20"));
        assert_eq!(get("d", "remarks"), s("the d column"));
        assert_eq!(get("e", "xdbc_data_type"), s("1"));
        assert_eq!(get("e", "xdbc_column_size"), s("5"));
        assert_eq!(get("f", "xdbc_data_type"), s("12"));
        assert_eq!(get("f", "xdbc_column_size"), None, "text has no length");
        assert_eq!(get("f", "xdbc_column_def"), s("'x'::text"));
        assert_eq!(get("a", "xdbc_column_def"), None);
        // generated and identity columns
        assert_eq!(get("g", "xdbc_is_generatedcolumn"), s("true"));
        assert_eq!(get("a", "xdbc_is_generatedcolumn"), s("false"));
        assert_eq!(get("h", "xdbc_is_autoincrement"), s("true"));
        assert_eq!(get("a", "xdbc_is_autoincrement"), s("false"));
        // array, timestamp(3), domain, time(0), enum, bit(7), unconstrained numeric, interval
        assert_eq!(get("i", "xdbc_type_name"), s("_int4"));
        assert_eq!(get("i", "xdbc_data_type"), s("2003"));
        assert_eq!(get("j", "xdbc_data_type"), s("93"));
        assert_eq!(get("j", "xdbc_decimal_digits"), s("3"));
        assert_eq!(
            get("k", "xdbc_type_name"),
            s("varchar"),
            "a domain reports its base type"
        );
        assert_eq!(get("k", "xdbc_data_type"), s("12"));
        assert_eq!(get("l", "xdbc_data_type"), s("92"));
        assert_eq!(get("l", "xdbc_decimal_digits"), s("0"));
        assert_eq!(get("m", "xdbc_type_name"), s("mood"));
        assert_eq!(get("m", "xdbc_data_type"), s("12"));
        assert_eq!(get("n", "xdbc_column_size"), s("7"));
        assert_eq!(get("o", "xdbc_data_type"), s("2"));
        assert_eq!(get("o", "xdbc_column_size"), None);
        assert_eq!(get("o", "xdbc_decimal_digits"), None);
        assert_eq!(get("p", "xdbc_data_type"), s("1111"));
        assert_eq!(get("p", "xdbc_decimal_digits"), s("6"));
        // the unused fields are NULL
        // the datetime subcode (ODBC's SQL_CODE_DATE, _TIME, _TIMESTAMP) and the octet length
        // (the character length times the encoding's widest character; 1 GiB for unbounded text)
        assert_eq!(get("j", "xdbc_datetime_sub"), s("3"));
        assert_eq!(get("l", "xdbc_datetime_sub"), s("2"));
        assert_eq!(get("a", "xdbc_datetime_sub"), None);
        assert_eq!(
            get("d", "xdbc_char_octet_length"),
            s("80"),
            "varchar(20) in UTF8"
        );
        assert_eq!(
            get("e", "xdbc_char_octet_length"),
            s("20"),
            "char(5) in UTF8"
        );
        assert_eq!(get("f", "xdbc_char_octet_length"), s("1073741824"));
        assert_eq!(get("a", "xdbc_char_octet_length"), None);
        // PostgreSQL has no REF types, so the scope fields are NULL by definition
        for f in [
            "xdbc_scope_catalog",
            "xdbc_scope_schema",
            "xdbc_scope_table",
        ] {
            assert_eq!(get("a", f), None, "{f}");
        }
        // column filters: one name, a pattern, and a pattern that matches nothing
        let one = m.columns(&ObjectFilter {
            table_name: Some("kmeta_parent"),
            column_name: Some("d"),
            ..Default::default()
        })?;
        assert_eq!(table(&one).len(), 1);
        assert_eq!(
            pick(&one, "column_name", "d", "ordinal_position"),
            s("4"),
            "a filter does not renumber"
        );
        let all = m.columns(&ObjectFilter {
            table_name: Some("kmeta_parent"),
            column_name: Some("_"),
            ..Default::default()
        })?;
        assert_eq!(table(&all).len(), 16);
        let none = m.columns(&ObjectFilter {
            table_name: Some("kmeta_parent"),
            column_name: Some("zz%"),
            ..Default::default()
        })?;
        assert_eq!(table(&none).len(), 0);
        assert_eq!(
            none.fields,
            columns_fields(),
            "the schema is kept for an empty result"
        );

        // constraints
        let k = m.constraints(&ObjectFilter {
            table_name: Some("kmeta_%"),
            ..Default::default()
        })?;
        assert_eq!(k.fields, constraints_fields());
        // (table, name, type, column, position, fk_table, fk_column)
        let got: Vec<[Option<String>; 7]> = table(&k)
            .into_iter()
            .map(|r| {
                [
                    r[2].clone(),
                    r[3].clone(),
                    r[4].clone(),
                    r[5].clone(),
                    r[6].clone(),
                    r[9].clone(),
                    r[10].clone(),
                ]
            })
            .collect();
        let row =
            |a: &str, b: &str, c: &str, d: &str, e: &str, f: Option<&str>, g: Option<&str>| {
                [
                    s(a),
                    s(b),
                    s(c),
                    s(d),
                    s(e),
                    f.map(String::from),
                    g.map(String::from),
                ]
            };
        let want = vec![
            row(
                "kmeta_child",
                "kmeta_child_pk",
                "PRIMARY KEY",
                "z",
                "1",
                None,
                None,
            ),
            row(
                "kmeta_child",
                "kmeta_child_y_x_fkey",
                "FOREIGN KEY",
                "y",
                "1",
                Some("kmeta_parent"),
                Some("a"),
            ),
            row(
                "kmeta_child",
                "kmeta_child_y_x_fkey",
                "FOREIGN KEY",
                "x",
                "2",
                Some("kmeta_parent"),
                Some("b"),
            ),
            row(
                "kmeta_parent",
                "kmeta_parent_a_check",
                "CHECK",
                "a",
                "1",
                None,
                None,
            ),
            row(
                "kmeta_parent",
                "kmeta_parent_c_d_key",
                "UNIQUE",
                "c",
                "1",
                None,
                None,
            ),
            row(
                "kmeta_parent",
                "kmeta_parent_c_d_key",
                "UNIQUE",
                "d",
                "2",
                None,
                None,
            ),
            row(
                "kmeta_parent",
                "kmeta_parent_pkey",
                "PRIMARY KEY",
                "b",
                "1",
                None,
                None,
            ),
            row(
                "kmeta_parent",
                "kmeta_parent_pkey",
                "PRIMARY KEY",
                "a",
                "2",
                None,
                None,
            ),
        ];
        assert_eq!(got, want);
        // the foreign key names the referenced catalog and schema, and only foreign keys do
        let fk_cat = col_index(&k, "fk_catalog");
        let fk_schema = col_index(&k, "fk_db_schema");
        for r in table(&k) {
            let is_fk = r[4].as_deref() == Some("FOREIGN KEY");
            assert_eq!(r[fk_cat].is_some(), is_fk);
            if is_fk {
                assert_eq!(r[fk_cat].as_deref(), Some("karez"));
                assert_eq!(r[fk_schema].as_deref(), Some("public"));
            }
        }

        // table_schema: what `SELECT *` returns, and the usual errors
        let fields = m.table_schema(Some("public"), "kmeta_v")?;
        assert_eq!(
            fields
                .iter()
                .map(|f| (f.name.as_str(), f.ty))
                .collect::<Vec<_>>(),
            [("a", ColumnType::Int32), ("b", ColumnType::Int32)]
        );
        let fields = m.table_schema(None, "kmeta_parent")?;
        assert_eq!(fields.len(), 16);
        assert_eq!(
            fields[2].ty,
            ColumnType::Decimal128 {
                precision: 12,
                scale: 3
            }
        );
        match m.table_schema(Some("public"), "no_such_table_here") {
            Err(e @ Error::Server(_)) => assert_eq!(e.sqlstate(), Some("42P01")),
            other => panic!("{other:?}"),
        }
        // a hostile name is quoted, not interpreted
        match m.table_schema(None, "x\"; DROP TABLE kmeta_parent; --") {
            Err(e @ Error::Server(_)) => assert_eq!(e.sqlstate(), Some("42P01")),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            table(&m.columns(&ObjectFilter {
                table_name: Some("kmeta_parent"),
                ..Default::default()
            })?)
            .len(),
            16,
            "the table is still there"
        );
        Ok(())
    })();
    for sql in DROP_FIXTURE {
        c.execute(sql, &[])?;
    }
    r
}

// ---- the seeded database, against information_schema ----------------------------------------

fn not_fixture(name: &str) -> bool {
    !name.starts_with("kmeta") && !name.starts_with("kmstat")
}

#[test]
fn columns_agree_with_information_schema_for_the_seeded_database() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let f = ObjectFilter {
        db_schema: Some("public"),
        ..Default::default()
    };
    let tables = c.metadata().tables(&f)?;
    let plain: BTreeSet<String> = table(&tables)
        .into_iter()
        .filter(|r| {
            matches!(
                r[3].as_deref(),
                Some("table" | "view" | "partitioned table")
            )
        })
        .map(|r| r[2].clone().unwrap())
        // `seed_done` belongs to another role: information_schema hides tables the user cannot
        // access, while the catalog queries (like ADBC drivers) list everything
        .filter(|t| not_fixture(t) && t != "seed_done")
        .collect();
    assert!(plain.len() >= 20, "{}", plain.len());
    let cols = c.metadata().columns(&f)?;
    let ours: Vec<Vec<Option<String>>> = table(&cols)
        .into_iter()
        .filter(|r| plain.contains(r[2].as_deref().unwrap()))
        .collect();
    let mut cur = c.query(
        "SELECT table_name::text, column_name::text, ordinal_position::int4, is_nullable::text, \
         column_default::text, udt_name::text, character_maximum_length::int4, numeric_precision::int4, \
         numeric_scale::int4, datetime_precision::int4, is_identity::text, is_generated::text, \
         character_octet_length::int4 \
         FROM information_schema.columns WHERE table_schema = 'public' AND table_name NOT LIKE 'kmeta%' AND table_name NOT LIKE 'kmstat%' \
         ORDER BY 1, 3",
        &[],
    )?;
    let mut theirs = Vec::new();
    while let Some(r) = cur.next()? {
        let text = |i: usize| r.get::<Option<String>>(i);
        let int = |i: usize| r.get::<Option<i32>>(i).map(|v| v.map(|v| v.to_string()));
        theirs.push(vec![
            text(0)?,
            text(1)?,
            int(2)?,
            text(3)?,
            text(4)?,
            text(5)?,
            int(6)?,
            int(7)?,
            int(8)?,
            int(9)?,
            text(10)?,
            text(11)?,
            int(12)?,
        ]);
    }
    drop(cur);
    // information_schema.columns also lists materialized views' absence the same way: compare the sets
    let theirs: Vec<_> = theirs
        .into_iter()
        .filter(|r| plain.contains(r[0].as_deref().unwrap()))
        .collect();
    assert_eq!(ours.len(), theirs.len(), "same number of columns");
    assert!(ours.len() > 100);
    let i = |name: &str| col_index(&cols, name);
    for (o, t) in ours.iter().zip(&theirs) {
        let what = format!("{}.{}", o[2].as_deref().unwrap(), o[3].as_deref().unwrap());
        assert_eq!(o[2], t[0], "{what}: table");
        assert_eq!(o[3], t[1], "{what}: column");
        assert_eq!(o[i("ordinal_position")], t[2], "{what}: ordinal position");
        assert_eq!(o[i("xdbc_is_nullable")], t[3], "{what}: is_nullable");
        assert_eq!(o[i("xdbc_column_def")], t[4], "{what}: default");
        assert_eq!(o[i("xdbc_type_name")], t[5], "{what}: type name");
        let ty = t[5].as_deref().unwrap();
        if matches!(ty, "varchar" | "bpchar") {
            assert_eq!(o[i("xdbc_column_size")], t[6], "{what}: length");
        }
        if ty == "numeric" {
            assert_eq!(o[i("xdbc_column_size")], t[7], "{what}: precision");
            assert_eq!(o[i("xdbc_decimal_digits")], t[8], "{what}: scale");
        }
        if matches!(
            ty,
            "time" | "timetz" | "timestamp" | "timestamptz" | "interval"
        ) {
            assert_eq!(
                o[i("xdbc_decimal_digits")],
                t[9],
                "{what}: datetime precision"
            );
        }
        if matches!(ty, "varchar" | "bpchar" | "text") {
            assert_eq!(
                o[i("xdbc_char_octet_length")],
                t[12],
                "{what}: octet length"
            );
        }
        if t[10].as_deref() == Some("YES") {
            assert_eq!(o[i("xdbc_is_autoincrement")], s("true"), "{what}: identity");
        }
        assert_eq!(
            o[i("xdbc_is_generatedcolumn")],
            s(if t[11].as_deref() == Some("ALWAYS") {
                "true"
            } else {
                "false"
            }),
            "{what}: generated"
        );
    }
    Ok(())
}

#[test]
fn constraints_agree_with_information_schema_for_the_seeded_database() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let k = c.metadata().constraints(&ObjectFilter {
        db_schema: Some("public"),
        ..Default::default()
    })?;
    // (table, constraint, column, position, fk table, fk column), without the check constraints,
    // which information_schema does not list by column
    let ours: BTreeSet<Vec<Option<String>>> = table(&k)
        .into_iter()
        .filter(|r| r[4].as_deref() != Some("CHECK") && not_fixture(r[2].as_deref().unwrap()))
        .map(|r| {
            vec![
                r[2].clone(),
                r[3].clone(),
                r[5].clone(),
                r[6].clone(),
                r[9].clone(),
                r[10].clone(),
            ]
        })
        .collect();
    assert!(ours.len() > 40, "{}", ours.len());
    let mut cur = c.query(
        "SELECT kcu.table_name::text, kcu.constraint_name::text, kcu.column_name::text, \
                kcu.ordinal_position::int4, tgt.table_name::text, tgt.column_name::text \
           FROM information_schema.key_column_usage kcu \
           JOIN information_schema.table_constraints tc \
             ON tc.constraint_schema = kcu.constraint_schema AND tc.constraint_name = kcu.constraint_name \
            AND tc.table_name = kcu.table_name \
           LEFT JOIN information_schema.referential_constraints rc \
             ON rc.constraint_schema = kcu.constraint_schema AND rc.constraint_name = kcu.constraint_name \
           LEFT JOIN information_schema.key_column_usage tgt \
             ON tgt.constraint_schema = rc.unique_constraint_schema \
            AND tgt.constraint_name = rc.unique_constraint_name \
            AND tgt.ordinal_position = kcu.position_in_unique_constraint \
          WHERE kcu.table_schema = 'public' AND kcu.table_name NOT LIKE 'kmeta%' AND kcu.table_name NOT LIKE 'kmstat%' \
            AND tc.constraint_type IN ('PRIMARY KEY', 'UNIQUE', 'FOREIGN KEY')",
        &[],
    )?;
    let mut theirs = BTreeSet::new();
    while let Some(r) = cur.next()? {
        theirs.insert(vec![
            r.get::<Option<String>>(0)?,
            r.get::<Option<String>>(1)?,
            r.get::<Option<String>>(2)?,
            r.get::<Option<i32>>(3)?.map(|v| v.to_string()),
            r.get::<Option<String>>(4)?,
            r.get::<Option<String>>(5)?,
        ]);
    }
    drop(cur);
    assert_eq!(ours.len(), theirs.len());
    assert_eq!(ours, theirs);
    // the seed's foreign keys are there by name
    assert!(ours.contains(&vec![
        s("order_items"),
        s("order_items_order_id_fkey"),
        s("order_id"),
        s("1"),
        s("orders"),
        s("id"),
    ]));
    Ok(())
}

#[test]
fn objects_by_depth_and_the_catalog_helpers() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let all = ObjectFilter::default();
    let o = c.metadata().objects(Depth::Catalogs, &all)?;
    assert_eq!(table(&o.catalogs), vec![vec![s("karez")]]);
    assert!(
        o.schemas.is_none() && o.tables.is_none() && o.columns.is_none() && o.constraints.is_none()
    );
    let o = c.metadata().objects(Depth::Schemas, &all)?;
    let schemas: Vec<String> = table(o.schemas.as_ref().unwrap())
        .into_iter()
        .map(|r| r[1].clone().unwrap())
        .collect();
    assert!(schemas.contains(&"public".to_string()) && schemas.contains(&"pg_catalog".to_string()));
    assert!(schemas
        .iter()
        .all(|n| !n.starts_with("pg_toast") && !n.starts_with("pg_temp_")));
    let mut sorted = schemas.clone();
    sorted.sort();
    assert_eq!(schemas, sorted, "in name order");
    assert!(o.tables.is_none());
    let f = ObjectFilter {
        db_schema: Some("public"),
        table_name: Some("order%"),
        ..Default::default()
    };
    let o = c.metadata().objects(Depth::Tables, &f)?;
    let names: Vec<String> = table(o.tables.as_ref().unwrap())
        .into_iter()
        .map(|r| r[2].clone().unwrap())
        .collect();
    assert!(
        names.contains(&"orders".to_string()) && names.contains(&"order_items".to_string()),
        "{names:?}"
    );
    assert!(names.iter().all(|n| n.starts_with("order")));
    assert!(o.columns.is_none());
    let o = c.metadata().objects(Depth::All, &f)?;
    assert!(o.columns.as_ref().unwrap().rows() > 10 && o.constraints.as_ref().unwrap().rows() > 5);
    // filters: another catalog, the empty string, and a schema that is not there
    let none = [
        ObjectFilter {
            catalog: Some("other_db"),
            ..Default::default()
        },
        ObjectFilter {
            catalog: Some(""),
            ..Default::default()
        },
        ObjectFilter {
            db_schema: Some(""),
            ..Default::default()
        },
        ObjectFilter {
            db_schema: Some("no_such_schema"),
            ..Default::default()
        },
    ];
    for f in &none {
        let o = c.metadata().objects(Depth::All, f)?;
        // a schema filter leaves the catalog level alone; a catalog filter empties every level
        let lower = o.schemas.as_ref().unwrap().rows() + o.tables.as_ref().unwrap().rows();
        assert_eq!(lower, 0, "{f:?}");
        assert_eq!(o.catalogs.rows(), usize::from(f.catalog.is_none()), "{f:?}");
    }
    // a catalog pattern matches
    let o = c.metadata().objects(
        Depth::Catalogs,
        &ObjectFilter {
            catalog: Some("kar%"),
            ..Default::default()
        },
    )?;
    assert_eq!(o.catalogs.rows(), 1);
    // there is no escape character: a backslash is an ordinary character
    let t = c.metadata().tables(&ObjectFilter {
        table_name: Some("order\\_items"),
        ..Default::default()
    })?;
    assert_eq!(t.rows(), 0);
    // table types
    let tt = c.metadata().table_types()?;
    assert_eq!(tt.fields, table_types_fields());
    let got: Vec<String> = table(&tt)
        .into_iter()
        .map(|r| r[0].clone().unwrap())
        .collect();
    assert_eq!(got, metadata::TABLE_TYPES);
    // every table type the database actually has is one the driver names
    let all_tables = c.metadata().tables(&all)?;
    for r in table(&all_tables) {
        assert!(
            metadata::TABLE_TYPES.contains(&r[3].as_deref().unwrap()),
            "{r:?}"
        );
    }
    Ok(())
}

#[test]
fn info_reports_the_server_and_the_driver() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    let version = {
        let mut cur = c.query("SHOW server_version", &[])?;
        cur.next()?.unwrap().get::<String>(0)?
    };
    let info = c.metadata().info(None)?;
    assert_eq!(info.fields, info_fields());
    // (info_name, string, bool, int64, bitmask) per row: exactly one value, or none for a null
    let rows = table(&info);
    let by = |code: u32| -> Vec<Option<String>> {
        rows.iter()
            .find(|r| r[0].as_deref() == Some(&code.to_string()))
            .unwrap_or_else(|| panic!("no row for code {code}"))[1..]
            .to_vec()
    };
    assert_eq!(by(0), [s("PostgreSQL"), None, None, None]);
    assert_eq!(by(1), [Some(version.clone()), None, None, None]);
    assert_eq!(by(3), [None, s("true"), None, None]);
    assert_eq!(by(4), [None, s("false"), None, None]);
    assert_eq!(
        by(5),
        [None, None, None, None],
        "Substrait is not supported: null"
    );
    assert_eq!(by(6), [None, None, None, None]);
    assert_eq!(by(100), [s("karez-pg"), None, None, None]);
    assert!(by(101)[0].as_deref().is_some_and(|v| v.contains('.')));
    assert_eq!(
        by(103),
        [None, None, s("1001000"), None],
        "ADBC_VERSION_1_1_0"
    );
    for r in &rows {
        assert!(r[1..].iter().filter(|v| v.is_some()).count() <= 1, "{r:?}");
    }
    let codes: Vec<String> = rows.iter().map(|r| r[0].clone().unwrap()).collect();
    assert!(!codes.contains(&"2".to_string()) && !codes.contains(&"102".to_string()));
    // a selection, in the driver's order, with a code nobody knows left out
    let some = c.metadata().info(Some(&[103, 0, 9999]))?;
    let got: Vec<String> = table(&some).iter().map(|r| r[0].clone().unwrap()).collect();
    assert_eq!(got, ["0", "103"]);
    assert_eq!(c.metadata().info(Some(&[]))?.rows(), 0);
    // the full u32 range is a column value
    assert_eq!(metadata::info_fields()[0].ty, ColumnType::UInt32);
    Ok(())
}

#[test]
fn statistics_exact_and_approximate_agree_with_the_data_and_the_planner() -> R {
    let addr = need_server!();
    let mut c = connect(&addr)?;
    c.execute("DROP TABLE IF EXISTS kmstat", &[])?;
    let _cleanup = Cleanup {
        addr: addr.clone(),
        sql: &["DROP TABLE IF EXISTS kmstat"],
    };
    c.execute(
        "CREATE TABLE kmstat (id int4 PRIMARY KEY, name text, f float8, b bytea, n int8)",
        &[],
    )?;
    c.execute(
        "INSERT INTO kmstat SELECT g, CASE WHEN g % 10 = 0 THEN NULL ELSE 'n' || (g % 7) END, \
         g * 0.5, decode(repeat('ab', g % 5), 'hex'), CASE WHEN g <= 100 THEN g END \
         FROM generate_series(1, 1000) g",
        &[],
    )?;
    c.execute("ANALYZE kmstat", &[])?;
    let r = (|| -> R {
        let mut m = c.metadata();
        let f = ObjectFilter {
            db_schema: Some("public"),
            table_name: Some("kmstat"),
            ..Default::default()
        };
        assert_eq!(m.statistic_names()?.fields, statistic_names_fields());
        assert_eq!(m.statistic_names()?.rows(), 0);

        // exact: int64 values, flagged not approximate
        let ex = m.statistics(&f, false)?;
        assert_eq!(ex.fields, statistics_fields());
        let rows = table(&ex);
        let get = |col: Option<&str>, key: i16| -> Vec<Option<String>> {
            rows.iter()
                .find(|r| r[3].as_deref() == col && r[4].as_deref() == Some(&key.to_string()))
                .unwrap_or_else(|| panic!("no statistic {key} for {col:?}"))[5..]
                .to_vec()
        };
        let (i64c, f64c, binc) = (0usize, 2usize, 3usize);
        assert_eq!(get(None, 6)[i64c], s("1000"), "row count");
        assert!(rows.iter().all(|r| r[9].as_deref() == Some("false")));
        assert_eq!(get(Some("id"), 5)[i64c], s("0"), "null count");
        assert_eq!(get(Some("name"), 5)[i64c], s("100"));
        assert_eq!(get(Some("n"), 5)[i64c], s("900"));
        assert_eq!(get(Some("id"), 1)[i64c], s("1000"), "distinct count");
        assert_eq!(get(Some("name"), 1)[i64c], s("7"));
        assert_eq!(get(Some("id"), 4)[i64c], s("1"), "min");
        assert_eq!(get(Some("id"), 3)[i64c], s("1000"), "max");
        assert_eq!(get(Some("f"), 3)[f64c], s("500"));
        assert_eq!(get(Some("f"), 4)[f64c], s("0.5"));
        assert_eq!(
            get(Some("name"), 4)[binc].as_deref().map(|_| ()),
            Some(()),
            "text minimum is bytes"
        );
        // the text extremes, read as bytes: compare with the server's own min and max
        let mut cur = m_query_min_max(&mut c)?;
        let (mn, mx) = (cur.0.take().unwrap(), cur.1.take().unwrap());
        let mut m = c.metadata();
        let ex_chunks = &ex.chunks;
        let bytes_of = |key: i16| -> Vec<u8> {
            for ch in ex_chunks {
                for r in 0..ch.rows() {
                    let row = ch.row(r).unwrap();
                    if row.get::<String>(3).ok().as_deref() == Some("name")
                        && row.get::<i16>(4).unwrap() == key
                    {
                        return row.get::<Vec<u8>>(8).unwrap();
                    }
                }
            }
            panic!("no binary statistic {key}")
        };
        assert_eq!(bytes_of(4), mn.as_bytes());
        assert_eq!(bytes_of(3), mx.as_bytes());
        // widths: the server's pg_column_size
        assert_eq!(get(Some("id"), 0)[f64c], s("4"), "average width of an int4");
        assert_eq!(get(Some("id"), 2)[i64c], s("4"), "largest width");
        // exactly one union member is set on every row
        for r in &rows {
            assert_eq!(r[5..9].iter().filter(|v| v.is_some()).count(), 1, "{r:?}");
        }

        // approximate: float64 values from the planner, flagged approximate
        let ap = m.statistics(&f, true)?;
        let rows = table(&ap);
        assert!(rows.iter().all(|r| r[9].as_deref() == Some("true")));
        let getf = |col: Option<&str>, key: i16| -> f64 {
            rows.iter()
                .find(|r| r[3].as_deref() == col && r[4].as_deref() == Some(&key.to_string()))
                .unwrap_or_else(|| panic!("no statistic {key} for {col:?}"))[7]
                .as_deref()
                .unwrap()
                .parse()
                .unwrap()
        };
        assert_eq!(getf(None, 6), 1000.0);
        assert_eq!(getf(Some("id"), 1), 1000.0, "n_distinct -1 means every row");
        assert_eq!(getf(Some("name"), 1), 7.0);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-3;
        assert!(
            near(getf(Some("name"), 5), 100.0),
            "null_frac 0.1 of 1000 rows"
        );
        assert!(near(getf(Some("n"), 5), 900.0));
        assert_eq!(getf(Some("id"), 0), 4.0);
        assert!(rows
            .iter()
            .all(|r| r[5].is_none() && r[6].is_none() && r[8].is_none()));
        // a table nobody analysed has no approximate statistics
        // (a pattern that matches nothing gives an empty result with the schema)
        let none = m.statistics(
            &ObjectFilter {
                table_name: Some("no_such_%"),
                ..Default::default()
            },
            true,
        )?;
        assert_eq!(none.rows(), 0);
        assert_eq!(none.fields, statistics_fields());
        // an unreadable table (owned by another role) is skipped, not an error
        let all = m.statistics(
            &ObjectFilter {
                db_schema: Some("public"),
                table_name: Some("seed_%"),
                ..Default::default()
            },
            false,
        )?;
        assert_eq!(all.rows(), 0);
        Ok(())
    })();
    c.execute("DROP TABLE IF EXISTS kmstat", &[])?;
    r
}

/// The server's own minimum and maximum of `kmstat.name`.
fn m_query_min_max(c: &mut Connection) -> Result<(Option<String>, Option<String>), Error> {
    let mut cur = c.query("SELECT min(name), max(name) FROM kmstat", &[])?;
    let r = cur.next()?.unwrap();
    Ok((r.get(0)?, r.get(1)?))
}
