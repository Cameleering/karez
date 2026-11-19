//! Catalog metadata (`Connection::metadata`): the schemas compared field by field with the ADBC
//! specification, and the contents checked against a purpose-built fixture and against
//! `information_schema`.
//!
//! The first test needs no server. The rest need `KAREZ_TEST_PG` (see tests/docker/README.md) and
//! print `SKIPPED` without it, unless `KAREZ_REQUIRE_SERVER=1`.

use karez_pg::metadata::{
    self, columns_fields, constraints_fields, db_schemas_fields, table_types_fields, tables_fields,
    Depth, ObjectFilter, Rows,
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
        "bool" => Some(ColumnType::Bool),
        _ => None, // a list or a struct
    }
}

/// The scalar fields of a spec table as (name, type, not null), and the names of its list fields.
fn split(spec: &[SpecField]) -> (Vec<(String, ColumnType, bool)>, Vec<String>) {
    let (mut scalars, mut lists) = (Vec::new(), Vec::new());
    for f in spec {
        match arrow_type(&f.ty) {
            Some(t) => scalars.push((f.name.clone(), t, f.not_null)),
            None => {
                assert!(f.ty.starts_with("list<"), "{f:?}");
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
    let t = spec_tables();
    assert_eq!(t.len(), 7, "the specification excerpt has seven tables");

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

    // the flat results: keys first, then the specification's fields in its order
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
        for f in [
            "xdbc_datetime_sub",
            "xdbc_char_octet_length",
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
    !name.starts_with("kmeta")
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
         numeric_scale::int4, datetime_precision::int4, is_identity::text, is_generated::text \
         FROM information_schema.columns WHERE table_schema = 'public' AND table_name NOT LIKE 'kmeta%' \
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
          WHERE kcu.table_schema = 'public' AND kcu.table_name NOT LIKE 'kmeta%' \
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
