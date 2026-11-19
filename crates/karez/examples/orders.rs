//! Runnable example against the test database in `tests/docker/`.
//!
//! ```sh
//! docker compose -f tests/docker/compose.yaml up -d --wait
//! KAREZ_URL='postgresql://karez_scram:karez_test_only_scram@127.0.0.1:54316/karez?sslmode=disable' \
//!   cargo run -p karez --example orders
//! docker compose -f tests/docker/compose.yaml down -v
//! ```
//!
//! It reads one customer's orders as rows, then scans a large table as chunks, then places an
//! order in a transaction and rolls it back.

use karez::{Connection, Error, Isolation, Value};

fn main() -> Result<(), Error> {
    let Ok(url) = std::env::var("KAREZ_URL") else {
        eprintln!("set KAREZ_URL, for example (the test-only database from tests/docker):");
        eprintln!("  postgresql://karez_scram:karez_test_only_scram@127.0.0.1:54316/karez?sslmode=disable");
        std::process::exit(2);
    };
    let mut conn = Connection::connect_url(&url)?;
    println!(
        "connected, server_version = {}",
        conn.server_param("server_version").unwrap_or("?")
    );

    // Row view.
    let mut cur = conn.query(
        "SELECT o.id, o.order_no, o.status, o.total_cents, o.note \
           FROM orders o WHERE o.customer_id = $1 ORDER BY o.id LIMIT 5",
        &[Value::i64(7)],
    )?;
    while let Some(row) = cur.next()? {
        println!(
            "order {} {} {} total={} note={:?}",
            row.get::<i64>(0)?,
            row.get::<&str>(1)?,
            row.get::<&str>(2)?,
            row.get::<i64>(3)?,
            row.get::<Option<&str>>(4)?,
        );
    }
    drop(cur);

    // Columnar view: a 400,000-row table arrives as chunks.
    let mut cur = conn.query("SELECT id, actor, details FROM audit_log", &[])?;
    let (mut chunks, mut rows) = (0, 0);
    while let Some(chunk) = cur.next_chunk()? {
        chunks += 1;
        rows += chunk.rows();
    }
    println!("audit_log: {rows} rows in {chunks} chunks");
    drop(cur);

    // A transaction that is rolled back leaves nothing behind.
    let before = count_orders(&mut conn)?;
    let mut tx = conn.begin(Isolation::Default)?;
    tx.execute(
        "INSERT INTO orders (customer_id, ship_address_id, note) VALUES ($1, $2, $3)",
        &[
            Value::i64(1),
            Value::i64(20000),
            Value::text("example, rolled back"),
        ],
    )?;
    tx.rollback()?;
    assert_eq!(count_orders(&mut conn)?, before);
    println!("rolled back; orders still {before}");
    Ok(())
}

fn count_orders(conn: &mut Connection) -> Result<i64, Error> {
    let mut cur = conn.query("SELECT count(*) FROM orders", &[])?;
    let n = cur.next()?.expect("one row").get::<i64>(0)?;
    cur.close()?;
    Ok(n)
}
