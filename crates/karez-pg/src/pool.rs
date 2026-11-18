//! A small blocking connection pool for PostgreSQL, on top of the driver-neutral `karez-pool`.
//!
//! The pool opens connections lazily up to a maximum, hands them out one at a time and takes
//! them back when the guard is dropped. Waiters are served first come, first served. A
//! connection that is broken is discarded. One that is still inside a transaction is rolled
//! back; if that fails it is discarded too. Connections are not tested before they are handed
//! out, so a server restart shows up as an error on first use (the broken connection is then
//! discarded and the next `get` opens a new one).

use crate::config::Config;
use crate::conn::Connection;
use crate::error::{Error, Result};
use karez_pool::{Manager, PoolError};
use karez_proto_pg::TxStatus;
use std::ops::{Deref, DerefMut};
use std::time::Duration;

/// What `karez-pool` needs from this driver: open a connection, and clean one that comes back.
struct PgManager {
    cfg: Config,
}

impl Manager for PgManager {
    type Connection = Connection;
    type Error = Error;

    fn connect(&self) -> Result<Connection> {
        Connection::connect(&self.cfg)
    }

    fn recycle(&self, c: &mut Connection) -> bool {
        if !c.is_broken() && c.tx_status() != TxStatus::Idle {
            let _ = c.execute("ROLLBACK", &[]);
        }
        !c.is_broken() && c.tx_status() == TxStatus::Idle
    }
}

fn map(e: PoolError<Error>) -> Error {
    match e {
        PoolError::ZeroSize => Error::Usage("a pool needs room for at least one connection"),
        PoolError::Timeout => Error::Usage("timed out waiting for a pooled connection"),
        PoolError::Connect(e) => e,
    }
}

/// A blocking pool of connections to one server. Cheap to clone; clones share the pool.
#[derive(Clone, Debug)]
pub struct Pool {
    inner: karez_pool::Pool<PgManager>,
}

impl Pool {
    /// Create a pool of at most `max_size` connections. Nothing is opened yet.
    pub fn new(cfg: Config, max_size: usize) -> Result<Pool> {
        Ok(Pool {
            inner: karez_pool::Pool::new(PgManager { cfg }, max_size).map_err(map)?,
        })
    }

    /// Take a connection, opening one if there is room and waiting if the pool is full.
    pub fn get(&self) -> Result<PooledConnection> {
        self.inner
            .get()
            .map(|inner| PooledConnection { inner })
            .map_err(map)
    }

    /// Like [`Pool::get`], giving up with an error after `timeout`.
    pub fn get_timeout(&self, timeout: Duration) -> Result<PooledConnection> {
        self.inner
            .get_timeout(timeout)
            .map(|inner| PooledConnection { inner })
            .map_err(map)
    }

    /// The most connections this pool will open.
    pub fn max_size(&self) -> usize {
        self.inner.max_size()
    }

    /// Connections that currently exist (idle or in use).
    pub fn size(&self) -> usize {
        self.inner.size()
    }

    /// Connections idle in the pool.
    pub fn idle(&self) -> usize {
        self.inner.idle()
    }
}

/// A connection on loan from a [`Pool`]; it goes back when dropped.
#[derive(Debug)]
pub struct PooledConnection {
    inner: karez_pool::Pooled<PgManager>,
}

impl PooledConnection {
    /// Remove this connection from the pool instead of returning it.
    pub fn discard(self) {
        self.inner.discard();
    }
}

impl Deref for PooledConnection {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.inner
    }
}

impl DerefMut for PooledConnection {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.inner
    }
}
