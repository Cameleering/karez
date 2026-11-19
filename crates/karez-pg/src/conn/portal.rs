//! Server-side cursors: named portals that are read a few rows at a time.
//!
//! A [`Cursor`] streams a whole result from the socket, so nothing else can run on the connection
//! until it is read or dropped. A [`Portal`] instead lives on the server. Each [`Connection::fetch`]
//! sends `Execute` with a row limit, reads exactly the rows it asked for plus `PortalSuspended`, and
//! reads on to `ReadyForQuery`, so the connection is idle between fetches: other statements, and
//! other portals, can run in the same transaction while a portal is half read.
//!
//! A portal exists only inside a transaction block (PostgreSQL destroys portals when the
//! transaction ends), so [`Connection::portal`] refuses to open one outside it.

use super::*;

/// A server-side cursor over one statement's result. See [`Connection::portal`].
///
/// Dropping a portal does not talk to the server: the server drops it when the transaction ends.
/// Use [`Connection::close_portal`] to free it earlier.
#[derive(Debug)]
pub struct Portal {
    name: String,
    stmt: Statement,
    done: bool,
    tag: Option<String>,
    rows: u64,
}

impl Portal {
    /// The result columns.
    pub fn fields(&self) -> &[Field] {
        self.stmt.fields()
    }

    /// True once the server has sent every row, or once an error made the portal unusable.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// The command tag the server sent with the last fetch, once every row has been read. Its row
    /// count is that of the last fetch only (`SELECT 0` when the previous fetch took the final
    /// rows); use [`Portal::rows_fetched`] for the total.
    pub fn command_tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    /// Rows fetched so far.
    pub fn rows_fetched(&self) -> u64 {
        self.rows
    }
}

/// The rows of one [`Connection::fetch`]: whole chunks, at most 1,024 rows and 1 MiB each.
#[derive(Debug, Default, Clone)]
pub struct Fetched {
    /// The rows, in order. Empty at the end of the result.
    pub chunks: Vec<Chunk>,
    /// True when the portal has no more rows.
    pub done: bool,
}

impl Fetched {
    /// Number of rows in all chunks.
    pub fn rows(&self) -> usize {
        self.chunks.iter().map(Chunk::rows).sum()
    }
}

impl Connection {
    /// Open a portal on `sql` with bound parameters. No rows are read yet.
    ///
    /// The connection must be inside a transaction block (`begin`): outside one, PostgreSQL would
    /// destroy the portal at the next `Sync`, so this returns [`Error::Usage`] instead. The
    /// statement is prepared on first use and cached, like [`Connection::query`].
    pub fn portal(&mut self, sql: &str, params: &[Value<'_>]) -> Result<Portal> {
        let r = self.portal_inner(sql, params);
        self.track(r)
    }

    fn portal_inner(&mut self, sql: &str, params: &[Value<'_>]) -> Result<Portal> {
        self.check_usable()?;
        if self.tx_status != TxStatus::InTransaction {
            return Err(Error::Usage(
                "a portal needs an open transaction (call begin first): the server destroys portals when a transaction ends",
            ));
        }
        let oids: Vec<u32> = params.iter().map(Value::oid).collect();
        let stmt = match self.cached(sql, &oids) {
            Some(s) => s,
            None => self.prepare_new(sql, &oids, true)?,
        };
        self.check_supported(&stmt)?;
        self.portal_seq += 1;
        let name = format!("karez_p{}", self.portal_seq);
        self.scratch.clear();
        let mut ranges = Vec::with_capacity(params.len());
        for p in params {
            ranges.push(p.encode_into(&mut self.scratch)?);
        }
        let slices: Vec<Option<&[u8]>> = ranges
            .iter()
            .map(|r| r.clone().map(|r| &self.scratch[r]))
            .collect();
        let formats: Vec<i16> = params.iter().map(Value::format).collect();
        self.out.clear();
        frontend::bind(
            &mut self.out,
            &name,
            &stmt.inner.name,
            &formats,
            &slices,
            stmt.inner.plan.result_formats(),
        )?;
        frontend::sync(&mut self.out);
        self.io.send(&self.out)?;
        match self.io.next_msg(&mut self.side)? {
            BackendMessage::BindComplete => {}
            BackendMessage::ErrorResponse(f) => {
                let e = ServerError::new(f);
                self.drain_to_ready()?;
                return Err(Error::Server(e));
            }
            other => return Err(unexpected(&other)),
        }
        self.drain_to_ready()?;
        Ok(Portal {
            name,
            stmt,
            done: false,
            tag: None,
            rows: 0,
        })
    }

    /// Read up to `max_rows` more rows from a portal (`0` reads all the rest). The connection is
    /// idle again when this returns, whatever the result.
    ///
    /// A server error (a failing expression in a later row, or a portal the server no longer has
    /// because the transaction ended) is returned as `Error::Server`; the portal is then done, and
    /// after an error inside the transaction the transaction is failed until `rollback`.
    pub fn fetch(&mut self, portal: &mut Portal, max_rows: usize) -> Result<Fetched> {
        self.check_usable()?;
        let r = self.fetch_inner(portal, max_rows);
        self.track(r)
    }

    fn fetch_inner(&mut self, portal: &mut Portal, max_rows: usize) -> Result<Fetched> {
        if portal.done {
            return Ok(Fetched {
                chunks: Vec::new(),
                done: true,
            });
        }
        let limit = i32::try_from(max_rows)
            .map_err(|_| Error::Usage("max_rows does not fit the protocol's 32-bit limit"))?;
        self.out.clear();
        frontend::execute(&mut self.out, &portal.name, limit)?;
        frontend::sync(&mut self.out);
        self.io.send(&self.out)?;
        self.builder.set_schema(portal.stmt.fields().to_vec());
        let mut chunks = Vec::new();
        let mut err: Option<ErrorFields> = None;
        let mut bad_value: Option<String> = None;
        let mut finished = false;
        let mut tag = None;
        loop {
            let m = match self.io.next_msg(&mut self.side) {
                Ok(m) => m,
                Err(e) => {
                    portal.done = true;
                    return Err(self.server_error_or(err.take(), e));
                }
            };
            match m {
                BackendMessage::DataRow(d) => {
                    if err.is_none() && bad_value.is_none() {
                        match portal.stmt.inner.plan.decode_row(d.body, &mut self.builder) {
                            Ok(()) => {
                                portal.rows += 1;
                                if self.builder.is_full() {
                                    chunks.push(self.builder.chunk().clone());
                                    self.builder.reset();
                                }
                            }
                            // a value this version cannot represent: the stream is intact, so
                            // read on to ReadyForQuery and report it afterwards
                            Err(ProtoError::BadValue { oid, what }) => {
                                bad_value = Some(format!("value of type oid {oid}: {what}"));
                            }
                            Err(ProtoError::Chunk(c)) => bad_value = Some(c.to_string()),
                            Err(e) => {
                                portal.done = true;
                                self.broken = true; // unread messages remain
                                return Err(e.into());
                            }
                        }
                    }
                }
                BackendMessage::PortalSuspended => {}
                BackendMessage::CommandComplete(t) => {
                    finished = true;
                    tag = Some(t);
                }
                BackendMessage::EmptyQueryResponse => finished = true,
                BackendMessage::ErrorResponse(f) => err = Some(f),
                BackendMessage::ReadyForQuery(s) => {
                    self.tx_status = s;
                    break;
                }
                other => {
                    portal.done = true;
                    self.broken = true;
                    return Err(unexpected(&other));
                }
            }
        }
        if let Some(f) = err {
            portal.done = true;
            self.builder.reset();
            return Err(Error::Server(ServerError::new(f)));
        }
        if let Some(m) = bad_value {
            // rows were skipped, so the portal's position no longer matches what the caller saw
            portal.done = true;
            self.builder.reset();
            return Err(Error::Value(m));
        }
        if self.builder.rows() > 0 {
            chunks.push(self.builder.chunk().clone());
            self.builder.reset();
        }
        if finished {
            portal.done = true;
            portal.tag = tag;
        }
        Ok(Fetched {
            chunks,
            done: portal.done,
        })
    }

    /// Close a portal on the server now instead of at the end of the transaction. Closing a portal
    /// that has already finished or that the server no longer has is not an error.
    pub fn close_portal(&mut self, portal: Portal) -> Result<()> {
        self.check_usable()?;
        if portal.done || self.tx_status != TxStatus::InTransaction {
            return Ok(());
        }
        let r = self.close_portal_inner(&portal.name);
        self.track(r)
    }

    fn close_portal_inner(&mut self, name: &str) -> Result<()> {
        self.out.clear();
        frontend::close_portal(&mut self.out, name)?;
        frontend::sync(&mut self.out);
        self.io.send(&self.out)?;
        let mut err = None;
        loop {
            match self.io.next_msg(&mut self.side)? {
                BackendMessage::CloseComplete => {}
                BackendMessage::ErrorResponse(f) => err = Some(f),
                BackendMessage::ReadyForQuery(s) => {
                    self.tx_status = s;
                    break;
                }
                other => return Err(unexpected(&other)),
            }
        }
        match err {
            Some(f) => Err(Error::Server(ServerError::new(f))),
            None => Ok(()),
        }
    }
}
