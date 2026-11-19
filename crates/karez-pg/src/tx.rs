use crate::conn::{quote_ident, Connection, Cursor, Fetched, Portal, PortalCursor, Statement};
use crate::error::Result;
use karez_chunk::RowRef;
use karez_proto_pg::{TxStatus, Value};

/// Transaction isolation level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Isolation {
    /// The server's default (`READ COMMITTED` unless configured otherwise).
    #[default]
    Default,
    /// `READ COMMITTED`
    ReadCommitted,
    /// `REPEATABLE READ`
    RepeatableRead,
    /// `SERIALIZABLE`
    Serializable,
}

impl Isolation {
    pub(crate) fn sql(self) -> &'static str {
        match self {
            Isolation::Default | Isolation::ReadCommitted => "READ COMMITTED",
            Isolation::RepeatableRead => "REPEATABLE READ",
            Isolation::Serializable => "SERIALIZABLE",
        }
    }
}

/// A named savepoint inside a transaction.
#[derive(Debug)]
pub struct Savepoint {
    quoted: String,
}

/// An open transaction. Dropping it without calling [`Transaction::commit`] rolls it back.
#[derive(Debug)]
pub struct Transaction<'c> {
    conn: &'c mut Connection,
    finished: bool,
}

impl<'c> Transaction<'c> {
    pub(crate) fn new(conn: &'c mut Connection) -> Self {
        Transaction {
            conn,
            finished: false,
        }
    }

    /// Run SQL with bound parameters and stream the rows.
    pub fn query(&mut self, sql: &str, params: &[Value<'_>]) -> Result<Cursor<'_>> {
        self.conn.query(sql, params)
    }

    /// Run SQL with bound parameters; returns the affected row count.
    pub fn execute(&mut self, sql: &str, params: &[Value<'_>]) -> Result<u64> {
        self.conn.execute(sql, params)
    }

    /// Run a prepared statement and stream its result.
    pub fn query_prepared(&mut self, stmt: &Statement, params: &[Value<'_>]) -> Result<Cursor<'_>> {
        self.conn.query_prepared(stmt, params)
    }

    /// Run a prepared statement; returns the affected row count.
    pub fn execute_prepared(&mut self, stmt: &Statement, params: &[Value<'_>]) -> Result<u64> {
        self.conn.execute_prepared(stmt, params)
    }

    /// Open a server-side cursor on `sql`; see [`Connection::portal`]. It lives until the end of
    /// this transaction, and other statements can run between its fetches.
    pub fn portal(&mut self, sql: &str, params: &[Value<'_>]) -> Result<Portal> {
        self.conn.portal(sql, params)
    }

    /// Read up to `max_rows` more rows from a portal (`0` for all the rest); see
    /// [`Connection::fetch`].
    pub fn fetch(&mut self, portal: &mut Portal, max_rows: usize) -> Result<Fetched> {
        self.conn.fetch(portal, max_rows)
    }

    /// Close a portal before the transaction ends; see [`Connection::close_portal`].
    pub fn close_portal(&mut self, portal: Portal) -> Result<()> {
        self.conn.close_portal(portal)
    }

    /// Open a [`PortalCursor`]: a row-by-row reader that holds neither this transaction nor the
    /// connection between calls. Read it with [`Transaction::cursor_next`].
    pub fn portal_cursor(
        &mut self,
        sql: &str,
        params: &[Value<'_>],
        fetch_size: usize,
    ) -> Result<PortalCursor> {
        self.conn.portal_cursor(sql, params, fetch_size)
    }

    /// The next row of a [`PortalCursor`] opened in this transaction, or `None` at the end.
    pub fn cursor_next<'a>(&mut self, cursor: &'a mut PortalCursor) -> Result<Option<RowRef<'a>>> {
        cursor.next(self.conn)
    }

    /// Transaction state as the server last reported it (`Failed` after an error until rollback).
    pub fn status(&self) -> TxStatus {
        self.conn.tx_status()
    }

    /// Create a savepoint. The name is quoted, so any non-empty name without NUL is safe.
    pub fn savepoint(&mut self, name: &str) -> Result<Savepoint> {
        let quoted = quote_ident(name)?;
        self.conn.simple(&format!("SAVEPOINT {quoted}"))?;
        Ok(Savepoint { quoted })
    }

    /// Roll back to a savepoint; the transaction stays open and usable again.
    pub fn rollback_to(&mut self, sp: &Savepoint) -> Result<()> {
        self.conn
            .simple(&format!("ROLLBACK TO SAVEPOINT {}", sp.quoted))
    }

    /// Release a savepoint, keeping its changes.
    pub fn release(&mut self, sp: Savepoint) -> Result<()> {
        self.conn
            .simple(&format!("RELEASE SAVEPOINT {}", sp.quoted))
    }

    /// Commit. If the server turns the commit into a rollback (the transaction had failed), this
    /// still returns `Ok`; check [`Transaction::status`] first if that matters.
    pub fn commit(mut self) -> Result<()> {
        self.finished = true;
        self.conn.simple("COMMIT")
    }

    /// Roll back.
    pub fn rollback(mut self) -> Result<()> {
        self.finished = true;
        self.conn.simple("ROLLBACK")
    }
}

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.finished && !self.conn.is_broken() {
            let _ = self.conn.simple("ROLLBACK");
        }
    }
}
