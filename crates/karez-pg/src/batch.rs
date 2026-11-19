//! Multi-row `INSERT`.

use crate::conn::{quote_ident, Connection};
use crate::error::{Error, Result};
use crate::scan::quoted_table;
use karez_proto_pg::{TxStatus, Value};

/// PostgreSQL allows 65,535 bind parameters in one statement.
const MAX_PARAMS: usize = 65_535;
/// Rows per statement unless the parameter limit forces fewer. Measured best between 1,000 and 3,000 rows for ten
/// columns (`docs/design/perf-program.md`); larger statements gain nothing.
const MAX_ROWS: usize = 1_000;

/// Rows per statement for `ncols` columns.
pub(crate) fn rows_per_statement(ncols: usize) -> usize {
    (MAX_PARAMS / ncols.max(1)).clamp(1, MAX_ROWS)
}

/// `INSERT INTO t (a, b) VALUES ($1, $2), ($3, $4), ...` for `nrows` rows.
pub(crate) fn insert_sql(table: &str, columns: &[&str], nrows: usize) -> Result<String> {
    if columns.is_empty() {
        return Err(Error::Usage("insert_batch needs at least one column"));
    }
    let mut cols = Vec::with_capacity(columns.len());
    for c in columns {
        cols.push(quote_ident(c)?);
    }
    let mut sql = format!(
        "INSERT INTO {} ({}) VALUES ",
        quoted_table(table)?,
        cols.join(", ")
    );
    let mut n = 1;
    for r in 0..nrows {
        if r > 0 {
            sql.push_str(", ");
        }
        sql.push('(');
        for c in 0..columns.len() {
            if c > 0 {
                sql.push_str(", ");
            }
            sql.push('$');
            sql.push_str(&n.to_string());
            n += 1;
        }
        sql.push(')');
    }
    Ok(sql)
}

impl Connection {
    /// Insert many rows with multi-row `INSERT ... VALUES` statements (up to 1,000 rows and 65,535 parameters per
    /// statement). Returns the number of rows inserted.
    ///
    /// All values are bound server-side. `table` is `name` or `schema.name`; names are quoted. Every row must
    /// have exactly `columns.len()` values.
    ///
    /// The whole batch is atomic: outside a transaction it runs in one of its own, which is rolled back on an
    /// error; inside a transaction it joins it (an error then leaves the transaction to the caller, as any failed
    /// statement does).
    ///
    /// For plain loads `COPY` ([`Connection::copy_in`]) is faster still; this is for `INSERT` semantics:
    /// defaults, triggers, rules and the usual error codes.
    ///
    /// Measured on 100,000 ten-column rows (`docs/design/perf-program.md`): 1.7x to 1.9x the speed of
    /// [`Connection::execute_many`] with one statement per row.
    pub fn insert_batch(
        &mut self,
        table: &str,
        columns: &[&str],
        rows: &[&[Value<'_>]],
    ) -> Result<u64> {
        let ncols = columns.len();
        if ncols == 0 {
            return Err(Error::Usage("insert_batch needs at least one column"));
        }
        if ncols > MAX_PARAMS {
            return Err(Error::Usage("too many columns for one statement"));
        }
        if rows.iter().any(|r| r.len() != ncols) {
            return Err(Error::Usage(
                "every row of insert_batch must have one value per column",
            ));
        }
        if rows.is_empty() {
            return Ok(0);
        }
        let own_tx = self.tx_status() == TxStatus::Idle;
        if own_tx {
            self.execute("BEGIN", &[])?;
        }
        let r = self.insert_batch_inner(table, columns, rows);
        if own_tx {
            match &r {
                Ok(_) => {
                    self.execute("COMMIT", &[])?;
                }
                Err(_) => {
                    let _ = self.execute("ROLLBACK", &[]);
                }
            }
        }
        r
    }

    fn insert_batch_inner(
        &mut self,
        table: &str,
        columns: &[&str],
        rows: &[&[Value<'_>]],
    ) -> Result<u64> {
        let per = rows_per_statement(columns.len());
        let mut total = 0;
        let mut cached: Option<(usize, String)> = None;
        let mut params: Vec<Value<'_>> = Vec::with_capacity(per * columns.len());
        for batch in rows.chunks(per) {
            if cached.as_ref().map(|(n, _)| *n) != Some(batch.len()) {
                cached = Some((batch.len(), insert_sql(table, columns, batch.len())?));
            }
            params.clear();
            for r in batch {
                params.extend_from_slice(r);
            }
            let sql = &cached.as_ref().expect("set above").1;
            total += self.execute(sql, &params)?;
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statement_text_has_one_placeholder_per_value() {
        assert_eq!(
            insert_sql("public.t", &["a", "b"], 2).unwrap(),
            "INSERT INTO \"public\".\"t\" (\"a\", \"b\") VALUES ($1, $2), ($3, $4)"
        );
        assert_eq!(
            insert_sql("t", &["we\"ird"], 1).unwrap(),
            "INSERT INTO \"t\" (\"we\"\"ird\") VALUES ($1)"
        );
        assert!(insert_sql("t", &[], 1).is_err());
        assert!(insert_sql("", &["a"], 1).is_err());
    }

    #[test]
    fn statements_stay_under_the_parameter_limit() {
        for ncols in [1usize, 2, 10, 64, 65, 1000, 65_535] {
            let per = rows_per_statement(ncols);
            assert!((1..=MAX_ROWS).contains(&per));
            assert!(per * ncols <= MAX_PARAMS || per == 1, "ncols {ncols}");
        }
        assert_eq!(rows_per_statement(10), 1000);
        assert_eq!(rows_per_statement(100), 655);
    }
}
