//! A small blocking connection pool for karez drivers. Std only, no dependencies.
//!
//! The pool is generic over a [`Manager`], which knows how to open a connection and how to check
//! one that comes back. A driver crate (`karez-pg` today; MySQL and the others later) implements
//! `Manager` and wraps [`Pool`] in its own types.
//!
//! Behaviour:
//!
//! * Connections are opened lazily, up to a maximum, and handed out one at a time.
//! * Waiters are served first come, first served. Only the waiter at the front of the queue is woken when a
//!   connection comes back, so a release costs one wake-up however many threads wait.
//! * A connection goes back when its guard is dropped, after [`Manager::recycle`] has looked at it.
//!   `recycle` runs outside the pool's lock, so it may do network I/O (a `ROLLBACK`, say). If it
//!   returns `false` the connection is discarded and its slot is free again.
//! * A connection that has sat idle for a while (by default one second, see
//!   [`Pool::set_validate_after`]) is checked with [`Manager::validate`] before it is handed out; one
//!   that fails is dropped and replaced by a new connection in the same place in the queue, so a
//!   server that went away (a failover, a restart) does not show up as an error on first use. A
//!   connection that was idle for less than that is handed out without a check, so a pool that
//!   is busy pays nothing. A connection that dies within that window still fails on first use; the
//!   driver's `recycle` then discards it and the next `get` opens a new one.

#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// What a driver supplies to the pool.
pub trait Manager: Send + Sync + 'static {
    /// The connection type held by the pool.
    type Connection: Send + 'static;
    /// The error of opening a connection.
    type Error;

    /// Open a new connection. Called without the pool's lock held.
    fn connect(&self) -> Result<Self::Connection, Self::Error>;

    /// Look at a connection that is being returned and make it ready for the next user (for
    /// example roll back an open transaction). Return `true` to keep it, `false` to discard it.
    /// Called without the pool's lock held.
    fn recycle(&self, conn: &mut Self::Connection) -> bool;

    /// Check a connection that has been idle before it is handed out. Return `false` if it is
    /// dead (the pool drops it and opens another). Must not wait on the network for long: a
    /// non-blocking probe of the socket is the intended use. Called without the pool's lock held.
    /// The default accepts every connection.
    fn validate(&self, _conn: &mut Self::Connection) -> bool {
        true
    }
}

/// Why [`Pool::get`] or [`Pool::new`] failed.
#[derive(Debug, PartialEq, Eq)]
pub enum PoolError<E> {
    /// A pool must have room for at least one connection.
    ZeroSize,
    /// No connection became free before the deadline.
    Timeout,
    /// Opening a new connection failed; the pool's count is unchanged.
    Connect(E),
}

impl<E: fmt::Display> fmt::Display for PoolError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PoolError::ZeroSize => f.write_str("a pool needs room for at least one connection"),
            PoolError::Timeout => f.write_str("timed out waiting for a pooled connection"),
            PoolError::Connect(e) => write!(f, "{e}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for PoolError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PoolError::Connect(e) => Some(e),
            _ => None,
        }
    }
}

/// One thread waiting for a connection. Each waiter has its own condition variable, so that a
/// release can wake exactly the one that is next in line (a shared one woke every waiter on every
/// release: with 512 threads on 32 connections that was most of the pool's time, see
/// bench/results/2026-11-19-exit-c4/REPORT.md).
struct Waiter {
    ticket: u64,
    wake: Arc<Condvar>,
}

struct State<C> {
    /// Idle connections and when each went idle; the last in is the first out.
    idle: Vec<(C, Instant)>,
    /// Connections that exist: idle, checked out, or being opened.
    total: usize,
    /// The threads waiting for a connection, in arrival order.
    waiters: VecDeque<Waiter>,
    next_ticket: u64,
}

impl<C> State<C> {
    /// Wake the waiter at the front of the queue, if any. Call it after every change that can let
    /// the front waiter proceed: a connection came back, a slot was freed, or the front left.
    fn wake_front(&self) {
        if let Some(w) = self.waiters.front() {
            w.wake.notify_one();
        }
    }
}

struct Shared<M: Manager> {
    manager: M,
    max: usize,
    state: Mutex<State<M::Connection>>,
    /// An idle connection older than this is validated before it is handed out (milliseconds).
    validate_after_ms: AtomicU64,
    /// Loop turns of waiting threads, to test that a release wakes one thread and not all.
    #[cfg(test)]
    turns: std::sync::atomic::AtomicUsize,
}

impl<M: Manager> Shared<M> {
    fn lock(&self) -> MutexGuard<'_, State<M::Connection>> {
        // The state is a counter and a list; a panic elsewhere cannot leave it inconsistent.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A blocking pool of connections to one server. Cheap to clone; clones share the pool.
pub struct Pool<M: Manager> {
    shared: Arc<Shared<M>>,
}

impl<M: Manager> Clone for Pool<M> {
    fn clone(&self) -> Self {
        Pool {
            shared: self.shared.clone(),
        }
    }
}

impl<M: Manager> fmt::Debug for Pool<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.shared.lock();
        f.debug_struct("Pool")
            .field("max", &self.shared.max)
            .field("total", &s.total)
            .field("idle", &s.idle.len())
            .finish()
    }
}

impl<M: Manager> Pool<M> {
    /// Create a pool of at most `max_size` connections. Nothing is opened yet.
    pub fn new(manager: M, max_size: usize) -> Result<Self, PoolError<M::Error>> {
        if max_size == 0 {
            return Err(PoolError::ZeroSize);
        }
        Ok(Pool {
            shared: Arc::new(Shared {
                manager,
                max: max_size,
                state: Mutex::new(State {
                    idle: Vec::new(),
                    total: 0,
                    waiters: VecDeque::new(),
                    next_ticket: 0,
                }),
                validate_after_ms: AtomicU64::new(1000),
                #[cfg(test)]
                turns: std::sync::atomic::AtomicUsize::new(0),
            }),
        })
    }

    /// Validate a connection that has been idle at least this long before handing it out (default
    /// one second; `Duration::ZERO` checks every idle connection every time).
    pub fn set_validate_after(&self, after: Duration) {
        let ms = u64::try_from(after.as_millis()).unwrap_or(u64::MAX);
        self.shared.validate_after_ms.store(ms, Ordering::Relaxed);
    }

    /// The manager this pool was built with.
    pub fn manager(&self) -> &M {
        &self.shared.manager
    }

    /// Take a connection, opening one if there is room and waiting if the pool is full.
    pub fn get(&self) -> Result<Pooled<M>, PoolError<M::Error>> {
        self.get_inner(None)
    }

    /// Like [`Pool::get`], giving up with [`PoolError::Timeout`] after `timeout`.
    pub fn get_timeout(&self, timeout: Duration) -> Result<Pooled<M>, PoolError<M::Error>> {
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

    fn get_inner(&self, deadline: Option<Instant>) -> Result<Pooled<M>, PoolError<M::Error>> {
        let mut st = self.shared.lock();
        // Nobody is waiting: take a connection, or a free slot, at once.
        if st.waiters.is_empty() {
            if let Some((c, since)) = st.idle.pop() {
                drop(st);
                return self.checked(c, since);
            }
            if st.total < self.shared.max {
                st.total += 1;
                drop(st);
                return self.open();
            }
        }
        // First come, first served: a waiter proceeds only when it is at the front of the queue,
        // and it is woken only when something has changed for it (see `State::wake_front`).
        let ticket = st.next_ticket;
        st.next_ticket += 1;
        let wake = Arc::new(Condvar::new());
        st.waiters.push_back(Waiter {
            ticket,
            wake: wake.clone(),
        });
        loop {
            #[cfg(test)]
            self.shared
                .turns
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if st.waiters.front().map(|w| w.ticket) == Some(ticket) {
                if let Some((c, since)) = st.idle.pop() {
                    st.waiters.pop_front();
                    st.wake_front(); // the next in line may be able to go too
                    drop(st);
                    return self.checked(c, since);
                }
                if st.total < self.shared.max {
                    st.total += 1;
                    st.waiters.pop_front();
                    st.wake_front();
                    drop(st);
                    return self.open();
                }
            }
            st = match deadline {
                None => wake.wait(st).unwrap_or_else(|e| e.into_inner()),
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        let was_front = st.waiters.front().map(|w| w.ticket) == Some(ticket);
                        st.waiters.retain(|w| w.ticket != ticket);
                        if was_front {
                            st.wake_front();
                        }
                        return Err(PoolError::Timeout);
                    }
                    wake.wait_timeout(st, d - now)
                        .unwrap_or_else(|e| e.into_inner())
                        .0
                }
            };
        }
    }

    /// Hand out a connection taken from the idle list, after checking it if it has been idle long
    /// enough. A dead one is replaced in the same slot (the count does not change).
    fn checked(
        &self,
        mut c: M::Connection,
        since: Instant,
    ) -> Result<Pooled<M>, PoolError<M::Error>> {
        let after = Duration::from_millis(self.shared.validate_after_ms.load(Ordering::Relaxed));
        if since.elapsed() >= after {
            let alive = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.shared.manager.validate(&mut c)
            }))
            .unwrap_or(false);
            if !alive {
                drop(c);
                return self.open();
            }
        }
        Ok(self.guard(c))
    }

    /// Open the connection for a slot already counted in `total`. A failure gives the slot back.
    fn open(&self) -> Result<Pooled<M>, PoolError<M::Error>> {
        match self.shared.manager.connect() {
            Ok(c) => Ok(self.guard(c)),
            Err(e) => {
                let mut st = self.shared.lock();
                st.total -= 1;
                st.wake_front();
                Err(PoolError::Connect(e))
            }
        }
    }

    fn guard(&self, c: M::Connection) -> Pooled<M> {
        Pooled {
            conn: Some(c),
            shared: self.shared.clone(),
        }
    }
}

/// A connection on loan from a [`Pool`]; it goes back when dropped.
pub struct Pooled<M: Manager> {
    conn: Option<M::Connection>,
    shared: Arc<Shared<M>>,
}

impl<M: Manager> fmt::Debug for Pooled<M>
where
    M::Connection: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Pooled").field(&self.conn).finish()
    }
}

impl<M: Manager> Pooled<M> {
    /// Remove this connection from the pool instead of returning it.
    pub fn discard(mut self) {
        self.conn = None;
        // Drop does the bookkeeping.
    }
}

impl<M: Manager> Deref for Pooled<M> {
    type Target = M::Connection;
    fn deref(&self) -> &M::Connection {
        self.conn.as_ref().expect("connection present until drop")
    }
}

impl<M: Manager> DerefMut for Pooled<M> {
    fn deref_mut(&mut self) -> &mut M::Connection {
        self.conn.as_mut().expect("connection present until drop")
    }
}

impl<M: Manager> Drop for Pooled<M> {
    fn drop(&mut self) {
        // recycle may do I/O, and may panic in a driver: run it outside the lock, and make sure the
        // slot is released either way.
        let keep = match self.conn.take() {
            Some(mut c) => {
                let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.shared.manager.recycle(&mut c)
                }))
                .unwrap_or(false);
                ok.then_some(c)
            }
            None => None,
        };
        let mut st = self.shared.lock();
        match keep {
            Some(c) => st.idle.push((c, Instant::now())),
            None => st.total -= 1,
        }
        st.wake_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    struct Mock {
        opened: AtomicUsize,
        fail_next: AtomicUsize,
        keep: bool,
    }

    impl Mock {
        fn new(keep: bool) -> Self {
            Mock {
                opened: AtomicUsize::new(0),
                fail_next: AtomicUsize::new(0),
                keep,
            }
        }
    }

    impl Manager for Mock {
        type Connection = usize;
        type Error = &'static str;
        fn connect(&self) -> Result<usize, &'static str> {
            if self.fail_next.load(Ordering::SeqCst) > 0 {
                self.fail_next.fetch_sub(1, Ordering::SeqCst);
                return Err("refused");
            }
            Ok(self.opened.fetch_add(1, Ordering::SeqCst))
        }
        fn recycle(&self, _: &mut usize) -> bool {
            self.keep
        }
    }

    /// Connections are numbers; the ones in `dead` fail validation.
    struct Checked {
        opened: AtomicUsize,
        validated: AtomicUsize,
        dead: Mutex<Vec<usize>>,
    }

    impl Manager for Checked {
        type Connection = usize;
        type Error = &'static str;
        fn connect(&self) -> Result<usize, &'static str> {
            Ok(self.opened.fetch_add(1, Ordering::SeqCst))
        }
        fn recycle(&self, _: &mut usize) -> bool {
            true
        }
        fn validate(&self, c: &mut usize) -> bool {
            self.validated.fetch_add(1, Ordering::SeqCst);
            !self.dead.lock().unwrap().contains(c)
        }
    }

    fn checked() -> Pool<Checked> {
        Pool::new(
            Checked {
                opened: AtomicUsize::new(0),
                validated: AtomicUsize::new(0),
                dead: Mutex::new(Vec::new()),
            },
            3,
        )
        .unwrap()
    }

    #[test]
    fn a_dead_idle_connection_is_replaced_before_it_is_handed_out() {
        let pool = checked();
        pool.set_validate_after(Duration::ZERO);
        let (a, b) = (pool.get().unwrap(), pool.get().unwrap());
        assert_eq!((*a, *b), (0, 1));
        drop((a, b));
        assert_eq!((pool.size(), pool.idle()), (2, 2));
        pool.manager().dead.lock().unwrap().extend([0, 1]); // the server went away
        let c = pool.get().unwrap();
        assert_eq!(*c, 2, "a new connection, not the dead one");
        let d = pool.get().unwrap();
        assert_eq!(*d, 3, "the second dead one is replaced too");
        assert_eq!(pool.size(), 2, "the slots were reused, none leaked");
        assert_eq!(pool.manager().validated.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_connection_that_was_idle_only_briefly_is_not_checked() {
        let pool = checked();
        assert_eq!(*pool.get().unwrap(), 0);
        pool.manager().dead.lock().unwrap().push(0);
        // idle for a moment, under the default one second: handed out without a check
        let a = pool.get().unwrap();
        assert_eq!(*a, 0);
        assert_eq!(pool.manager().validated.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_waiter_gets_a_replacement_for_a_dead_connection_in_its_place() {
        let pool = checked();
        pool.set_validate_after(Duration::ZERO);
        let held: Vec<_> = (0..3).map(|_| pool.get().unwrap()).collect();
        let p = pool.clone();
        let waiter = thread::spawn(move || *p.get().unwrap());
        thread::sleep(Duration::from_millis(50));
        pool.manager().dead.lock().unwrap().extend([0, 1, 2]);
        drop(held); // all three go idle and dead; the waiter is woken for the front one
        assert_eq!(
            waiter.join().unwrap(),
            3,
            "it got a new connection, not a dead one"
        );
        assert!(pool.size() <= 3);
    }

    #[test]
    fn zero_size_is_refused() {
        assert_eq!(
            Pool::new(Mock::new(true), 0).unwrap_err(),
            PoolError::ZeroSize
        );
    }

    #[test]
    fn opens_lazily_and_reuses() {
        let pool = Pool::new(Mock::new(true), 3).unwrap();
        assert_eq!((pool.size(), pool.idle()), (0, 0));
        let a = pool.get().unwrap();
        assert_eq!(*a, 0);
        assert_eq!((pool.size(), pool.idle()), (1, 0));
        drop(a);
        assert_eq!((pool.size(), pool.idle()), (1, 1));
        let b = pool.get().unwrap();
        assert_eq!(*b, 0, "the idle connection is reused, not a new one");
        assert_eq!(pool.manager().opened.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn never_exceeds_the_maximum_and_times_out() {
        let pool = Pool::new(Mock::new(true), 2).unwrap();
        let _a = pool.get().unwrap();
        let _b = pool.get().unwrap();
        assert_eq!(pool.size(), 2);
        let t = Instant::now();
        assert_eq!(
            pool.get_timeout(Duration::from_millis(60)).unwrap_err(),
            PoolError::Timeout
        );
        assert!(t.elapsed() >= Duration::from_millis(50));
        assert_eq!(pool.size(), 2);
    }

    #[test]
    fn a_failed_connect_gives_the_slot_back() {
        let pool = Pool::new(Mock::new(true), 1).unwrap();
        pool.manager().fail_next.store(1, Ordering::SeqCst);
        assert_eq!(pool.get().unwrap_err(), PoolError::Connect("refused"));
        assert_eq!(pool.size(), 0);
        assert!(pool.get().is_ok());
    }

    #[test]
    fn recycle_false_and_discard_free_the_slot() {
        let pool = Pool::new(Mock::new(false), 1).unwrap();
        drop(pool.get().unwrap());
        assert_eq!((pool.size(), pool.idle()), (0, 0));
        let pool = Pool::new(Mock::new(true), 1).unwrap();
        pool.get().unwrap().discard();
        assert_eq!((pool.size(), pool.idle()), (0, 0));
        // a new connection is opened (id 1 on the second manager is not the point; the slot is free)
        assert!(pool.get_timeout(Duration::from_millis(50)).is_ok());
    }

    #[test]
    fn a_panic_in_recycle_still_frees_the_slot() {
        struct Boom;
        impl Manager for Boom {
            type Connection = ();
            type Error = ();
            fn connect(&self) -> Result<(), ()> {
                Ok(())
            }
            fn recycle(&self, _: &mut ()) -> bool {
                panic!("driver bug")
            }
        }
        let pool = Pool::new(Boom, 1).unwrap();
        drop(pool.get().unwrap());
        assert_eq!(pool.size(), 0);
        assert!(pool.get_timeout(Duration::from_millis(50)).is_ok());
    }

    #[test]
    fn waiters_are_served_in_arrival_order() {
        let pool = Pool::new(Mock::new(true), 1).unwrap();
        let held = pool.get().unwrap();
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut handles = Vec::new();
        for i in 0..4usize {
            let (p, o) = (pool.clone(), order.clone());
            handles.push(thread::spawn(move || {
                let g = p.get().unwrap();
                o.lock().unwrap().push(i);
                thread::sleep(Duration::from_millis(5));
                drop(g);
            }));
            // let each thread reach the wait queue before the next starts
            thread::sleep(Duration::from_millis(40));
        }
        drop(held);
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(*order.lock().unwrap(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn a_release_wakes_one_waiter_not_all_of_them() {
        let pool = Pool::new(Mock::new(true), 1).unwrap();
        let held = pool.get().unwrap();
        let waiters = 48usize;
        let hs: Vec<_> = (0..waiters)
            .map(|_| {
                let p = pool.clone();
                thread::spawn(move || drop(p.get().unwrap()))
            })
            .collect();
        // let every thread reach the queue
        while pool.shared.lock().waiters.len() < waiters {
            thread::sleep(Duration::from_millis(2));
        }
        let before = pool.shared.turns.load(Ordering::SeqCst);
        drop(held);
        for h in hs {
            h.join().unwrap();
        }
        let turns = pool.shared.turns.load(Ordering::SeqCst) - before;
        // each waiter runs its loop once when it is woken for its turn; a shared condition variable
        // woke every waiter at every release, about waiters * waiters / 2 turns (1,100 here)
        assert!(
            turns <= waiters * 3,
            "{turns} loop turns for {waiters} waiters: releases wake more than one thread"
        );
        assert_eq!(pool.size(), pool.idle());
    }

    #[test]
    fn a_waiter_that_times_out_in_front_lets_the_next_one_through() {
        let pool = Pool::new(Mock::new(true), 1).unwrap();
        let held = pool.get().unwrap();
        let p1 = pool.clone();
        let front = thread::spawn(move || p1.get_timeout(Duration::from_millis(80)).err());
        thread::sleep(Duration::from_millis(30)); // the first is in the queue
        let p2 = pool.clone();
        let second = thread::spawn(move || p2.get_timeout(Duration::from_secs(5)).map(drop));
        thread::sleep(Duration::from_millis(30));
        assert_eq!(front.join().unwrap(), Some(PoolError::Timeout));
        drop(held); // the first has left the queue: the second must be woken by this
        assert!(second.join().unwrap().is_ok());
    }

    #[test]
    fn many_threads_never_hold_more_than_max() {
        let pool = Pool::new(Mock::new(true), 3).unwrap();
        let live = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let hs: Vec<_> = (0..12)
            .map(|_| {
                let (p, l, k) = (pool.clone(), live.clone(), peak.clone());
                thread::spawn(move || {
                    for _ in 0..50 {
                        let g = p.get().unwrap();
                        let now = l.fetch_add(1, Ordering::SeqCst) + 1;
                        k.fetch_max(now, Ordering::SeqCst);
                        thread::yield_now();
                        l.fetch_sub(1, Ordering::SeqCst);
                        drop(g);
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        assert!(peak.load(Ordering::SeqCst) <= 3);
        assert!(pool.manager().opened.load(Ordering::SeqCst) <= 3);
        assert_eq!(pool.size(), pool.idle());
    }
}
