//! A small blocking connection pool. Waiters are served first come, first served.
//!
//! The pool opens connections lazily up to a maximum, hands them out one at a time and takes
//! them back when the guard is dropped. A connection that is broken is discarded. One that is
//! still inside a transaction is rolled back; if that fails it is discarded too. Connections are
//! not tested before they are handed out, so a server restart shows up as an error on first use
//! (the broken connection is then discarded and the next `get` opens a new one).

use crate::config::Config;
use crate::conn::Connection;
use crate::error::{Error, Result};
use karez_proto_pg::TxStatus;
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

struct State {
    idle: Vec<Connection>,
    /// Connections that exist: idle, checked out, or being opened.
    total: usize,
    /// Tickets of the threads waiting for a connection, in arrival order.
    waiters: VecDeque<u64>,
    next_ticket: u64,
}

struct Shared {
    cfg: Config,
    max: usize,
    state: Mutex<State>,
    available: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // The state is a counter and a list; a panic elsewhere cannot leave it inconsistent.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A blocking pool of connections to one server. Cheap to clone; clones share the pool.
#[derive(Clone)]
pub struct Pool {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.shared.lock();
        f.debug_struct("Pool")
            .field("max", &self.shared.max)
            .field("total", &s.total)
            .field("idle", &s.idle.len())
            .finish()
    }
}

impl Pool {
    /// Create a pool of at most `max_size` connections. Nothing is opened yet.
    pub fn new(cfg: Config, max_size: usize) -> Result<Pool> {
        if max_size == 0 {
            return Err(Error::Usage(
                "a pool needs room for at least one connection",
            ));
        }
        Ok(Pool {
            shared: Arc::new(Shared {
                cfg,
                max: max_size,
                state: Mutex::new(State {
                    idle: Vec::new(),
                    total: 0,
                    waiters: VecDeque::new(),
                    next_ticket: 0,
                }),
                available: Condvar::new(),
            }),
        })
    }

    /// Take a connection, opening one if there is room and waiting if the pool is full.
    pub fn get(&self) -> Result<PooledConnection> {
        self.get_inner(None)
    }

    /// Like [`Pool::get`], giving up with an error after `timeout`.
    pub fn get_timeout(&self, timeout: Duration) -> Result<PooledConnection> {
        self.get_inner(Some(Instant::now() + timeout))
    }

    /// The most connections this pool will open.
    pub fn max_size(&self) -> usize {
        self.shared.max
    }

    /// Connections that currently exist (idle or in use).
    pub fn size(&self) -> usize {
        self.shared.lock().total
    }

    /// Connections idle in the pool.
    pub fn idle(&self) -> usize {
        self.shared.lock().idle.len()
    }

    fn get_inner(&self, deadline: Option<Instant>) -> Result<PooledConnection> {
        let mut st = self.shared.lock();
        // First come, first served: a waiter proceeds only when it is at the front of the queue.
        let ticket = st.next_ticket;
        st.next_ticket += 1;
        st.waiters.push_back(ticket);
        loop {
            if st.waiters.front() == Some(&ticket) {
                if let Some(c) = st.idle.pop() {
                    st.waiters.pop_front();
                    drop(st);
                    self.shared.available.notify_all();
                    return Ok(self.guard(c));
                }
                if st.total < self.shared.max {
                    st.total += 1;
                    st.waiters.pop_front();
                    drop(st);
                    self.shared.available.notify_all();
                    return match Connection::connect(&self.shared.cfg) {
                        Ok(c) => Ok(self.guard(c)),
                        Err(e) => {
                            self.shared.lock().total -= 1;
                            self.shared.available.notify_all();
                            Err(e)
                        }
                    };
                }
            }
            st = match deadline {
                None => self
                    .shared
                    .available
                    .wait(st)
                    .unwrap_or_else(|e| e.into_inner()),
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        st.waiters.retain(|t| *t != ticket);
                        drop(st);
                        self.shared.available.notify_all();
                        return Err(Error::Usage("timed out waiting for a pooled connection"));
                    }
                    self.shared
                        .available
                        .wait_timeout(st, d - now)
                        .unwrap_or_else(|e| e.into_inner())
                        .0
                }
            };
        }
    }

    fn guard(&self, c: Connection) -> PooledConnection {
        PooledConnection {
            conn: Some(c),
            shared: self.shared.clone(),
        }
    }
}

/// A connection on loan from a [`Pool`]; it goes back when dropped.
pub struct PooledConnection {
    conn: Option<Connection>,
    shared: Arc<Shared>,
}

impl std::fmt::Debug for PooledConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PooledConnection").field(&self.conn).finish()
    }
}

impl PooledConnection {
    /// Remove this connection from the pool instead of returning it.
    pub fn discard(mut self) {
        self.conn = None;
        // Drop does the bookkeeping.
    }
}

impl Deref for PooledConnection {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn.as_ref().expect("connection present until drop")
    }
}

impl DerefMut for PooledConnection {
    fn deref_mut(&mut self) -> &mut Connection {
        self.conn.as_mut().expect("connection present until drop")
    }
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        let keep = match self.conn.take() {
            Some(mut c) => {
                if !c.is_broken() && c.tx_status() != TxStatus::Idle {
                    let _ = c.execute("ROLLBACK", &[]);
                }
                if !c.is_broken() && c.tx_status() == TxStatus::Idle {
                    Some(c)
                } else {
                    None
                }
            }
            None => None,
        };
        let mut st = self.shared.lock();
        match keep {
            Some(c) => st.idle.push(c),
            None => st.total -= 1,
        }
        drop(st);
        self.shared.available.notify_all();
    }
}
