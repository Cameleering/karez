//! Pipelines and batches: many statements sent without waiting for each answer.
//!
//! A statement normally costs one round trip: send, wait, read. A **pipeline** sends several
//! statements (queries and executes, each with its own parameters) in one write and reads all their
//! answers, so a hundred point queries cost about one round trip instead of a hundred. A **batch**
//! runs one prepared statement for many parameter sets, in groups, which is how a bulk insert or
//! update goes without a multi-row `VALUES` list.
//!
//! Both use the extended protocol's `Bind` and `Execute` with **one `Sync` per group**. A group is
//! therefore atomic: if a statement fails, the server skips the rest of the group and nothing in it
//! takes effect (inside an explicit transaction the transaction is aborted instead, as for any
//! failed statement).

use super::*;

/// The result of one statement of a pipeline: the row count from its command tag and, for a query,
/// its rows as chunks.
#[derive(Debug, Clone, Default)]
pub struct StepResult {
    /// Rows the command reports (rows returned by a `SELECT`, rows affected by a write).
    pub rows: u64,
    /// The rows of a query, in chunks of at most 1,024 rows; empty for an execute.
    pub chunks: Vec<Chunk>,
}

enum Source<'p> {
    Sql(&'p str),
    Prepared(Statement),
}

struct Item<'p> {
    source: Source<'p>,
    query: bool,
    params: Vec<Value<'p>>,
}

/// A group of statements sent together. Build it with [`Connection::pipeline`] (or
/// `Transaction::pipeline`), add statements, and [`Pipeline::run`] it.
///
/// ```no_run
/// # use karez_pg::{Connection, Value};
/// # fn demo(conn: &mut Connection) -> Result<(), karez_pg::Error> {
/// let results = conn
///     .pipeline()
///     .query("SELECT name FROM customers WHERE id = $1", &[Value::i64(1)])
///     .query("SELECT name FROM customers WHERE id = $1", &[Value::i64(2)])
///     .execute("UPDATE customers SET loyalty_points = loyalty_points + 1 WHERE id = $1", &[Value::i64(1)])
///     .run()?;
/// assert_eq!(results.len(), 3);
/// # Ok(()) }
/// ```
pub struct Pipeline<'c, 'p> {
    conn: &'c mut Connection,
    items: Vec<Item<'p>>,
}

impl std::fmt::Debug for Pipeline<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pipeline")
            .field("statements", &self.items.len())
            .finish()
    }
}

impl Connection {
    /// Start a [`Pipeline`] on this connection.
    pub fn pipeline<'c, 'p>(&'c mut self) -> Pipeline<'c, 'p> {
        Pipeline {
            conn: self,
            items: Vec::new(),
        }
    }

    /// Start a [`Batch`] that runs the prepared statement `stmt` once per row of parameters.
    pub fn batch<'c, 's>(&'c mut self, stmt: &'s Statement) -> Batch<'c, 's> {
        Batch {
            conn: self,
            stmt,
            group: DEFAULT_GROUP,
        }
    }
}

impl<'c, 'p> Pipeline<'c, 'p> {
    fn add(mut self, source: Source<'p>, query: bool, params: &[Value<'p>]) -> Self {
        self.items.push(Item {
            source,
            query,
            params: params.to_vec(),
        });
        self
    }

    /// Add a query: its rows come back as chunks in the result.
    pub fn query(self, sql: &'p str, params: &[Value<'p>]) -> Self {
        self.add(Source::Sql(sql), true, params)
    }

    /// Add a statement whose rows, if any, are discarded; its result carries the row count.
    pub fn execute(self, sql: &'p str, params: &[Value<'p>]) -> Self {
        self.add(Source::Sql(sql), false, params)
    }

    /// Add a prepared statement as a query.
    pub fn query_prepared(self, stmt: &Statement, params: &[Value<'p>]) -> Self {
        self.add(Source::Prepared(stmt.clone()), true, params)
    }

    /// Add a prepared statement whose rows are discarded.
    pub fn execute_prepared(self, stmt: &Statement, params: &[Value<'p>]) -> Self {
        self.add(Source::Prepared(stmt.clone()), false, params)
    }

    /// Number of statements added.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when nothing has been added.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Send every statement with one `Sync` and read all the answers.
    ///
    /// The outer error means nothing ran: a statement that could not be prepared (a syntax error, an
    /// unknown table), parameters of the wrong type or number, a type without a decoder, or a
    /// connection failure. Otherwise there is one result per statement, in order. If a statement
    /// fails at run time its result is the error and **every later one is [`Error::Skipped`]**: the
    /// server skips the rest of the group, and the whole group is rolled back (one implicit
    /// transaction), or, inside a transaction block, the transaction is left failed. A value that
    /// this version cannot hold in a chunk fails only that statement with [`Error::Value`].
    ///
    /// Statements that are not prepared yet are prepared first, one round trip each; use
    /// [`Connection::prepare`] and the `_prepared` methods to avoid it. The group must not use more
    /// distinct statements than the statement cache holds.
    pub fn run(self) -> Result<Vec<std::result::Result<StepResult, Error>>> {
        let conn = self.conn;
        conn.check_usable()?;
        let r = run_pipeline(conn, self.items);
        conn.track(r)
    }
}

fn run_pipeline(
    conn: &mut Connection,
    items: Vec<Item<'_>>,
) -> Result<Vec<std::result::Result<StepResult, Error>>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    // every distinct statement must stay in the cache while the group is prepared
    let mut distinct: Vec<(String, Vec<u32>)> = Vec::new();
    for it in &items {
        if let Source::Sql(sql) = &it.source {
            let oids: Vec<u32> = it.params.iter().map(Value::oid).collect();
            if !distinct.iter().any(|(s, o)| s == sql && *o == oids) {
                distinct.push(((*sql).to_string(), oids));
            }
        }
    }
    if distinct.len() > conn.cache_cap {
        return Err(Error::Usage(
            "the pipeline uses more distinct statements than the statement cache holds",
        ));
    }
    let mut stmts = Vec::with_capacity(items.len());
    for it in &items {
        let stmt = match &it.source {
            Source::Prepared(s) => s.clone(),
            Source::Sql(sql) => {
                let oids: Vec<u32> = it.params.iter().map(Value::oid).collect();
                match conn.cached(sql, &oids) {
                    Some(s) => s,
                    None => conn.prepare_new(sql, &oids, true)?,
                }
            }
        };
        Connection::check_params(&stmt, &it.params)?;
        if it.query {
            conn.check_supported(&stmt)?;
        }
        stmts.push(stmt);
    }
    let decode: Vec<bool> = items.iter().map(|i| i.query).collect();
    let params: Vec<&[Value<'_>]> = items.iter().map(|i| i.params.as_slice()).collect();
    let (steps, err) = conn.run_group(&stmts, &params, &decode)?;
    let mut out = Vec::with_capacity(items.len());
    let mut err = err.map(ServerError::new);
    for (i, step) in steps.into_iter().enumerate() {
        let _ = i;
        out.push(match step.value_err {
            Some(m) => Err(Error::Value(m)),
            None => Ok(StepResult {
                rows: step.rows,
                chunks: step.chunks,
            }),
        });
    }
    // the statement the server stopped at, then the ones it skipped
    while out.len() < items.len() {
        out.push(match err.take() {
            Some(e) => Err(Error::Server(e)),
            None => Err(Error::Skipped),
        });
    }
    Ok(out)
}

/// Rows per group unless [`Batch::group_size`] says otherwise.
const DEFAULT_GROUP: usize = 1_000;

/// What a [`Batch`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchOutcome {
    /// Parameter sets run (one statement execution each).
    pub statements: u64,
    /// The sum of the row counts the server reported (rows inserted, updated or deleted).
    pub rows_affected: u64,
}

/// One prepared statement run once per row of parameters. See [`Connection::batch`].
pub struct Batch<'c, 's> {
    conn: &'c mut Connection,
    stmt: &'s Statement,
    group: usize,
}

impl std::fmt::Debug for Batch<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Batch")
            .field("group_size", &self.group)
            .finish()
    }
}

impl<'c, 's> Batch<'c, 's> {
    /// Statements sent per `Sync` (default 1,000; at least 1). A larger group means fewer round
    /// trips; a smaller one bounds the memory a group needs.
    pub fn group_size(mut self, n: usize) -> Self {
        self.group = n.max(1);
        self
    }

    /// The rows of parameters: one item per execution, each a slice of [`Value`]s (a `Vec`, an
    /// array or a slice all work). The iterator is read lazily, one group at a time.
    pub fn rows<'a, I, R>(self, rows: I) -> BatchRows<'c, 's, 'a, I::IntoIter>
    where
        I: IntoIterator<Item = R>,
        R: AsRef<[Value<'a>]>,
    {
        BatchRows {
            batch: self,
            rows: rows.into_iter(),
            _values: std::marker::PhantomData,
        }
    }
}

/// A [`Batch`] with its rows. Call [`BatchRows::run`].
pub struct BatchRows<'c, 's, 'a, I> {
    batch: Batch<'c, 's>,
    rows: I,
    _values: std::marker::PhantomData<&'a ()>,
}

impl<I> std::fmt::Debug for BatchRows<'_, '_, '_, I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatchRows")
            .field("group_size", &self.batch.group)
            .finish()
    }
}

impl<'c, 's, 'a, I, R> BatchRows<'c, 's, 'a, I>
where
    I: Iterator<Item = R>,
    R: AsRef<[Value<'a>]>,
{
    /// Statements sent per `Sync`; see [`Batch::group_size`].
    pub fn group_size(mut self, n: usize) -> Self {
        self.batch.group = n.max(1);
        self
    }

    /// Run the statement for every row.
    ///
    /// The whole batch is atomic. Outside a transaction it runs in one of its own (`BEGIN`, the
    /// groups, `COMMIT`), rolled back on any error; inside a transaction block it joins the
    /// transaction, and an error leaves the transaction to the caller, as any failed statement
    /// does. Every row's parameters must match the statement's parameter types.
    ///
    /// For a plain load `COPY` ([`Connection::copy_in`]) is faster; a batch is for `INSERT`,
    /// `UPDATE` and `DELETE` semantics: defaults, triggers, rules, `ON CONFLICT` and the usual
    /// error codes, with each row's own row count.
    pub fn run(mut self) -> Result<BatchOutcome> {
        let conn = &mut *self.batch.conn;
        conn.check_usable()?;
        let own_tx = conn.tx_status() == TxStatus::Idle;
        if own_tx {
            conn.execute("BEGIN", &[])?;
        }
        let r = run_batch(conn, self.batch.stmt, self.batch.group, &mut self.rows);
        let r = conn.track(r);
        if own_tx {
            match &r {
                Ok(_) => {
                    conn.execute("COMMIT", &[])?;
                }
                Err(_) => {
                    let _ = conn.execute("ROLLBACK", &[]);
                }
            }
        }
        r
    }
}

fn run_batch<'a, I, R>(
    conn: &mut Connection,
    stmt: &Statement,
    group: usize,
    rows: &mut I,
) -> Result<BatchOutcome>
where
    I: Iterator<Item = R>,
    R: AsRef<[Value<'a>]>,
{
    let mut out = BatchOutcome {
        statements: 0,
        rows_affected: 0,
    };
    let stmts = vec![stmt.clone(); group];
    let decode = vec![false; group];
    let mut buf: Vec<R> = Vec::with_capacity(group);
    loop {
        buf.clear();
        buf.extend(rows.by_ref().take(group));
        if buf.is_empty() {
            return Ok(out);
        }
        let params: Vec<&[Value<'a>]> = buf.iter().map(AsRef::as_ref).collect();
        for p in &params {
            Connection::check_params(stmt, p)?;
        }
        let (steps, err) =
            conn.run_group(&stmts[..params.len()], &params, &decode[..params.len()])?;
        if let Some(f) = err {
            return Err(Error::Server(ServerError::new(f)));
        }
        out.statements += steps.len() as u64;
        out.rows_affected += steps.iter().map(|s| s.rows).sum::<u64>();
    }
}
