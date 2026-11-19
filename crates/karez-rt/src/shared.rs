//! A connection shared by many threads or futures.
//!
//! One `Mutex` guards the [`Engine`] and the table of completion slots; it is held only briefly,
//! never across a socket call. Whoever queues a command also writes the pending bytes (the
//! "writer role": a second submitter just appends and returns). Replies are read by a **pump
//! thread** that exists only while commands are in flight. There is no thread on an idle
//! connection. See `docs/design/karez-rt.md`.

use crate::engine::{Class, CommandId, Done, Engine, Event, Prepared, RawPrepared};
use crate::error::{Error, Result};
use karez_chunk::Chunk;
use karez_pg::{
    CancelToken, Config, Connection, Error as PgError, Notification, RawParts, UnsupportedTypes,
};
use karez_proto_pg::{ResultPlan, TxStatus, Value};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

/// Limits and timeouts of a shared connection. Every one has a default.
#[derive(Debug, Clone)]
pub struct Options {
    /// Commands that may be queued or in flight at once (default 1,024).
    pub max_queue: usize,
    /// Largest result a collecting command buffers (default 64 MiB).
    pub max_collect_bytes: usize,
    /// Chunks a streaming command buffers before the connection stops reading (default 4).
    pub stream_queue_chunks: usize,
    /// How long a submit waits for room, or for a streaming result to release the connection,
    /// before failing with [`Error::QueueFull`] (default 30 s).
    pub wait_timeout: Duration,
    /// How long `close` waits for commands in flight (default 5 s).
    pub close_grace: Duration,
    /// How long the reader thread stays after the last command finished, in case another one
    /// follows (default 50 ms). Starting a thread for every command would cost more than the
    /// round trip itself. After this the connection has no thread; 0 means exit at once.
    pub idle_linger: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            max_queue: 1024,
            max_collect_bytes: 64 << 20,
            stream_queue_chunks: 4,
            wait_timeout: Duration::from_secs(30),
            close_grace: Duration::from_secs(5),
            idle_linger: Duration::from_millis(50),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct SlotState {
    chunks: VecDeque<Chunk>,
    done: Option<Result<Done>>,
    taken: bool,
    waker: Option<Waker>,
    abandoned: bool,
    stream: bool,
}

struct Slot {
    m: Mutex<SlotState>,
    cv: Condvar,
}

impl Slot {
    fn new(stream: bool) -> Arc<Slot> {
        Arc::new(Slot {
            m: Mutex::new(SlotState {
                chunks: VecDeque::new(),
                done: None,
                taken: false,
                waker: None,
                abandoned: false,
                stream,
            }),
            cv: Condvar::new(),
        })
    }
}

struct State {
    engine: Engine,
    /// The TLS session over the socket, if any. Its cipher state is only touched under the lock.
    #[cfg(feature = "tls")]
    tls: Option<karez_pg::TlsConnection>,
    slots: HashMap<CommandId, Arc<Slot>>,
    pump: bool,
    writing: bool,
    closed: bool,
    dead: Option<String>,
    leased: Option<CommandId>,
}

struct Inner {
    st: Mutex<State>,
    /// Signalled when a command completes, a streaming result frees room, or the lease ends.
    cv: Condvar,
    writer: Mutex<TcpStream>,
    reader: TcpStream,
    cancel: CancelToken,
    opts: Options,
    unsupported: UnsupportedTypes,
    enums: Mutex<HashMap<u32, bool>>,
}

/// A PostgreSQL connection that many threads, or many futures on any executor, can use at once.
/// Commands are pipelined: whatever is queued is written in one go and answered in order.
///
/// Cloning is cheap and shares the connection.
#[derive(Clone)]
pub struct SharedConnection {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for SharedConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let st = self.inner.state();
        f.debug_struct("SharedConnection")
            .field("in_flight", &st.engine.in_flight())
            .field("closed", &st.closed)
            .field("dead", &st.dead)
            .finish()
    }
}

fn wake_all(ws: Vec<Waker>) {
    for w in ws {
        w.wake();
    }
}

impl State {
    /// The bytes to put on the wire now: the engine's output, encrypted if the connection uses TLS
    /// (plus whatever the TLS layer itself wants to send). Taking marks the commands as sent.
    fn take_wire(&mut self) -> Vec<u8> {
        let plain = self.engine.take_output();
        #[cfg(feature = "tls")]
        if let Some(t) = self.tls.as_mut() {
            if !plain.is_empty() {
                let _ = t.writer().write_all(&plain);
            }
            let mut wire = Vec::new();
            while t.wants_write() {
                if t.write_tls(&mut wire).is_err() {
                    break;
                }
            }
            return wire;
        }
        plain
    }

    /// Feed bytes read from the socket: decrypt first if the connection uses TLS.
    fn feed_wire(&mut self, bytes: &[u8]) -> Result<()> {
        #[cfg(feature = "tls")]
        if let Some(t) = self.tls.as_mut() {
            let tls_err = |e: String| Error::Pg(PgError::Tls(e));
            let mut rd = bytes;
            while !rd.is_empty() {
                let n = t.read_tls(&mut rd).map_err(|e| tls_err(e.to_string()))?;
                if n == 0 {
                    break;
                }
                t.process_new_packets()
                    .map_err(|e| tls_err(e.to_string()))?;
                let mut tmp = [0u8; 16 * 1024];
                loop {
                    match t.reader().read(&mut tmp) {
                        Ok(0) => {
                            return Err(Error::Closed {
                                retry_safe: false,
                                reason: "the server closed the TLS session".into(),
                            })
                        }
                        Ok(n) => self.engine.feed(&tmp[..n])?,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(tls_err(e.to_string())),
                    }
                }
            }
            return Ok(());
        }
        self.engine.feed(bytes)
    }
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.st)
    }

    /// Hand every finished event to its slot. Returns wakers to call once the lock is released.
    fn dispatch(st: &mut State) -> Vec<Waker> {
        let mut wakers = Vec::new();
        while let Some(ev) = st.engine.poll_event() {
            match ev {
                Event::Chunk { id, chunk } => {
                    if let Some(slot) = st.slots.get(&id) {
                        let mut s = lock(&slot.m);
                        if !s.abandoned {
                            s.chunks.push_back(chunk);
                        }
                        if let Some(w) = s.waker.take() {
                            wakers.push(w);
                        }
                        slot.cv.notify_all();
                    }
                }
                Event::Done { id, result } => {
                    if st.leased == Some(id) {
                        st.leased = None;
                    }
                    if let Some(slot) = st.slots.remove(&id) {
                        let mut s = lock(&slot.m);
                        s.done = Some(result);
                        if let Some(w) = s.waker.take() {
                            wakers.push(w);
                        }
                        slot.cv.notify_all();
                    }
                }
            }
        }
        wakers
    }

    /// A streaming result whose consumer has not caught up: stop reading the socket.
    fn blocked(&self, st: &State) -> bool {
        st.slots.values().any(|slot| {
            let s = lock(&slot.m);
            s.stream && !s.abandoned && s.chunks.len() >= self.opts.stream_queue_chunks
        })
    }

    fn die(&self, reason: &str) {
        let wakers = {
            let mut st = self.state();
            if st.dead.is_some() {
                return;
            }
            st.dead = Some(reason.to_owned());
            st.engine.fail_all(reason);
            let w = Self::dispatch(&mut st);
            st.leased = None;
            w
        };
        let _ = lock(&self.writer).shutdown(Shutdown::Both);
        self.cv.notify_all();
        wake_all(wakers);
    }

    /// The writer role: write whatever the engine has queued, outside the lock, so a slow socket
    /// never blocks the pump from draining replies.
    fn flush<'a>(self: &'a Arc<Self>, mut st: MutexGuard<'a, State>) {
        if st.writing {
            return;
        }
        st.writing = true;
        loop {
            let bytes = st.take_wire();
            if bytes.is_empty() {
                st.writing = false;
                return;
            }
            drop(st);
            let r = lock(&self.writer).write_all(&bytes);
            st = self.state();
            if let Err(e) = r {
                st.writing = false;
                drop(st);
                self.die(&format!("write failed: {e}"));
                return;
            }
        }
    }

    fn ensure_pump(self: &Arc<Self>, st: &mut State) {
        if st.pump {
            // The reader is lingering after its last command: wake it for this one.
            self.cv.notify_all();
            return;
        }
        if st.dead.is_some() {
            return;
        }
        let rd = match self.reader.try_clone() {
            Ok(r) => r,
            Err(e) => {
                let reason = format!("cannot start the reader: {e}");
                st.dead = Some(reason.clone());
                st.engine.fail_all(&reason);
                return;
            }
        };
        st.pump = true;
        let me = self.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("karez-rt-pump".into())
            .spawn(move || me.pump(rd))
        {
            st.pump = false;
            let reason = format!("cannot start the reader thread: {e}");
            st.dead = Some(reason.clone());
            st.engine.fail_all(&reason);
        }
    }

    fn pump(self: Arc<Self>, mut rd: TcpStream) {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            {
                let mut st = self.state();
                let mut idle_since: Option<Instant> = None;
                loop {
                    if st.dead.is_some() {
                        st.pump = false;
                        drop(st);
                        self.cv.notify_all();
                        return;
                    }
                    if st.engine.in_flight() == 0 {
                        // Linger briefly: the next command usually follows at once.
                        let since = *idle_since.get_or_insert_with(Instant::now);
                        let left = self.opts.idle_linger.saturating_sub(since.elapsed());
                        if left.is_zero() {
                            st.pump = false;
                            drop(st);
                            self.cv.notify_all();
                            return;
                        }
                        st = self
                            .cv
                            .wait_timeout(st, left)
                            .unwrap_or_else(|e| e.into_inner())
                            .0;
                        continue;
                    }
                    idle_since = None;
                    if self.blocked(&st) {
                        st = self.cv.wait(st).unwrap_or_else(|e| e.into_inner());
                        continue;
                    }
                    break;
                }
            }
            match rd.read(&mut buf) {
                Ok(0) => {
                    self.die("the server closed the connection");
                    self.exit_pump();
                    return;
                }
                Ok(n) => {
                    let (wakers, err) = {
                        let mut st = self.state();
                        let r = st.feed_wire(&buf[..n]);
                        let w = Self::dispatch(&mut st);
                        (w, r.err())
                    };
                    // TLS may have something to send (a key update, an alert).
                    let st = self.state();
                    self.flush(st);
                    self.cv.notify_all();
                    wake_all(wakers);
                    if let Some(e) = err {
                        self.die(&format!("protocol error: {e}"));
                        self.exit_pump();
                        return;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => {
                    self.die(&format!("read failed: {e}"));
                    self.exit_pump();
                    return;
                }
            }
        }
    }

    fn exit_pump(&self) {
        self.state().pump = false;
        self.cv.notify_all();
    }

    /// Wait (bounded) until a command may be queued: not closed, room in the queue, no lease.
    fn admit(&self, stream: bool) -> Result<MutexGuard<'_, State>> {
        let deadline = Instant::now() + self.opts.wait_timeout;
        let mut st = self.state();
        loop {
            if let Some(reason) = &st.dead {
                return Err(Error::Closed {
                    retry_safe: true,
                    reason: reason.clone(),
                });
            }
            if st.closed {
                return Err(Error::Closed {
                    retry_safe: true,
                    reason: "the connection was closed".into(),
                });
            }
            let leased = st.leased.is_some();
            let full = st.slots.len() >= self.opts.max_queue;
            if !leased && !full && !(stream && !st.slots.is_empty()) {
                return Ok(st);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::QueueFull);
            }
            st = self
                .cv
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

impl SharedConnection {
    /// Connect, authenticate, and take over the socket.
    pub fn connect(cfg: &Config, opts: Options) -> Result<SharedConnection> {
        Self::from_connection(Connection::connect(cfg)?, opts)
    }

    /// Take over an established blocking connection.
    pub fn from_connection(conn: Connection, opts: Options) -> Result<SharedConnection> {
        let RawParts {
            stream,
            #[cfg(feature = "tls")]
            mut tls,
            tx_status,
            server_params,
            max_message,
            statement_cache_size,
            unsupported,
            cancel,
            ..
        } = conn.into_raw_parts()?;
        // The pump blocks in `read`; a read timeout configured on the socket would break a slow
        // but healthy query, so it is cleared here. Use command deadlines instead.
        stream.set_read_timeout(None).map_err(PgError::Io)?;
        let writer = stream.try_clone().map_err(PgError::Io)?;
        #[cfg(feature = "tls")]
        if let Some(t) = tls.as_mut() {
            // The engine bounds what it queues; do not let the TLS layer refuse a write.
            t.set_buffer_limit(None);
        }
        let engine = Engine::new(
            max_message,
            statement_cache_size,
            opts.max_collect_bytes,
            tx_status,
            server_params,
        );
        Ok(SharedConnection {
            inner: Arc::new(Inner {
                st: Mutex::new(State {
                    engine,
                    #[cfg(feature = "tls")]
                    tls,
                    slots: HashMap::new(),
                    pump: false,
                    writing: false,
                    closed: false,
                    dead: None,
                    leased: None,
                }),
                cv: Condvar::new(),
                writer: Mutex::new(writer),
                reader: stream,
                cancel,
                opts,
                unsupported,
                enums: Mutex::new(HashMap::new()),
            }),
        })
    }

    /// A token that cancels the statement the server is running, from any thread.
    pub fn cancel_token(&self) -> CancelToken {
        self.inner.cancel.clone()
    }

    /// Transaction state as of the last answer.
    pub fn tx_status(&self) -> TxStatus {
        self.inner.state().engine.tx_status()
    }

    /// Commands queued or in flight.
    pub fn in_flight(&self) -> usize {
        self.inner.state().slots.len()
    }

    /// Notifications received so far; empties the queue. They arrive while commands are in
    /// flight (the reader only runs then).
    pub fn take_notifications(&self) -> Vec<Notification> {
        self.inner.state().engine.take_notifications()
    }

    /// True while the reader thread exists. It exists only while commands are in flight and for
    /// `Options::idle_linger` after the last one (diagnostics and tests).
    pub fn reader_thread_running(&self) -> bool {
        self.inner.state().pump
    }

    /// True once the connection is closed or has failed.
    pub fn is_closed(&self) -> bool {
        let st = self.inner.state();
        st.closed || st.dead.is_some()
    }

    /// Run a statement and return the number of rows it affected. Blocks the calling thread.
    pub fn exec(&self, sql: &str, params: &[Value<'_>]) -> Result<u64> {
        Ok(self.submit(sql, params, Class::Exec)?.wait()?.rows)
    }

    /// Run a statement and return all its rows as chunks (up to `max_collect_bytes`).
    pub fn collect(&self, sql: &str, params: &[Value<'_>]) -> Result<Done> {
        self.submit(sql, params, Class::Collect)?.wait()
    }

    /// Queue a statement without waiting. The handle can be waited on or `.await`ed.
    pub fn submit_exec(&self, sql: &str, params: &[Value<'_>]) -> Result<Pending> {
        self.submit(sql, params, Class::Exec)
    }

    /// Queue a collecting statement without waiting.
    pub fn submit_collect(&self, sql: &str, params: &[Value<'_>]) -> Result<Pending> {
        self.submit(sql, params, Class::Collect)
    }

    /// Run a statement and stream its chunks. The stream **leases the connection**: other
    /// commands wait until it ends or is dropped, because answers come back in order.
    pub fn stream(&self, sql: &str, params: &[Value<'_>]) -> Result<Stream> {
        let p = self.submit(sql, params, Class::Stream)?;
        Ok(Stream {
            pending: Some(p),
            finished: false,
        })
    }

    /// Start a batch of statements that share one Sync: it is atomic, and an error skips the rest.
    pub fn pipeline(&self) -> Pipeline<'_> {
        Pipeline {
            conn: self,
            items: Vec::new(),
        }
    }

    fn submit(&self, sql: &str, params: &[Value<'_>], class: Class) -> Result<Pending> {
        let stmt = self.prepared(sql, params)?;
        let inner = &self.inner;
        let mut st = inner.admit(class == Class::Stream)?;
        let id = st.engine.run(&stmt, params, class)?;
        let slot = Slot::new(class == Class::Stream);
        st.slots.insert(id, slot.clone());
        if class == Class::Stream {
            st.leased = Some(id);
        }
        inner.ensure_pump(&mut st);
        inner.flush(st);
        Ok(Pending {
            slot,
            inner: inner.clone(),
            id,
        })
    }

    /// The prepared statement for `sql` and the parameter types, preparing it on first use.
    /// The first use of a statement blocks the caller for one round trip.
    fn prepared(&self, sql: &str, params: &[Value<'_>]) -> Result<Arc<Prepared>> {
        let oids: Vec<u32> = params.iter().map(Value::oid).collect();
        {
            let mut st = self.inner.state();
            if let Some(p) = st.engine.lookup(sql, &oids) {
                return Ok(p);
            }
        }
        let pending = {
            let mut st = self.inner.admit(false)?;
            let id = st.engine.prepare(sql, &oids)?;
            let slot = Slot::new(false);
            st.slots.insert(id, slot.clone());
            self.inner.ensure_pump(&mut st);
            self.inner.flush(st);
            Pending {
                slot,
                inner: self.inner.clone(),
                id,
            }
        };
        let raw = pending
            .wait()?
            .prepared
            .ok_or(Error::Pg(PgError::Usage("prepare returned no description")))?;
        let plan = match self.plan_for(&raw) {
            Ok(p) => p,
            Err(e) => {
                let mut st = self.inner.state();
                st.engine.close_statement(&raw.name);
                self.inner.flush(st);
                return Err(e);
            }
        };
        let mut st = self.inner.state();
        let stmt = st.engine.install(sql, &oids, raw, plan);
        // Installing may have queued a Close for an evicted or duplicate statement.
        self.inner.ensure_pump(&mut st);
        self.inner.flush(st);
        Ok(stmt)
    }

    /// The decoding plan for a described statement: finds enums with one catalogue query per
    /// type and connection, and rejects types without a decoder unless text is allowed.
    fn plan_for(&self, raw: &RawPrepared) -> Result<ResultPlan> {
        for f in &raw.fields {
            let o = f.type_oid;
            if ResultPlan::knows_type(o) || lock(&self.inner.enums).contains_key(&o) {
                continue;
            }
            let is_enum = {
                let text = o.to_string();
                let done = self.collect(
                    "SELECT typtype::text FROM pg_type WHERE oid = $1::oid",
                    &[Value::Unknown(&text)],
                )?;
                done.chunks
                    .first()
                    .and_then(|c| c.row(0).ok())
                    .and_then(|r| r.get::<&str>(0).ok().map(|s| s == "e"))
                    .unwrap_or(false)
            };
            lock(&self.inner.enums).insert(o, is_enum);
        }
        let enums = lock(&self.inner.enums).clone();
        let plan = ResultPlan::from_description_with(&raw.fields, &|o| {
            enums.get(&o).copied().unwrap_or(false)
        });
        if self.inner.unsupported == UnsupportedTypes::Error {
            if let Some((i, oid)) = plan.first_text_fallback() {
                let text = oid.to_string();
                let name = self
                    .collect(
                        "SELECT typname::text FROM pg_type WHERE oid = $1::oid",
                        &[Value::Unknown(&text)],
                    )
                    .ok()
                    .and_then(|d| {
                        d.chunks
                            .first()
                            .and_then(|c| c.row(0).ok())
                            .and_then(|r| r.get::<&str>(0).ok().map(str::to_owned))
                    })
                    .unwrap_or_else(|| format!("oid {oid}"));
                return Err(Error::Pg(PgError::UnsupportedType {
                    column: plan.fields()[i].name.clone(),
                    oid,
                    type_name: name,
                }));
            }
        }
        Ok(plan)
    }

    /// Wait (up to the grace period) for commands in flight, send Terminate and close the socket.
    /// New commands fail with a retry-safe error from the moment this is called.
    pub fn close(&self) {
        let inner = &self.inner;
        let deadline = Instant::now() + inner.opts.close_grace;
        {
            let mut st = inner.state();
            st.closed = true;
            while !st.slots.is_empty() && st.dead.is_none() {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                st = inner
                    .cv
                    .wait_timeout(st, deadline - now)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        }
        {
            let mut st = inner.state();
            if st.dead.is_none() {
                st.engine.terminate();
                inner.flush(st);
            }
        }
        inner.die("the connection was closed");
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let st = self.st.get_mut().unwrap_or_else(|e| e.into_inner());
        let bye = if st.dead.is_none() {
            st.engine.terminate();
            st.take_wire()
        } else {
            Vec::new()
        };
        let w = self.writer.get_mut().unwrap_or_else(|e| e.into_inner());
        let _ = w.write_all(&bye);
        let _ = w.shutdown(Shutdown::Both);
    }
}

/// A command that was queued. Wait for it with [`Pending::wait`], or `.await` it.
pub struct Pending {
    slot: Arc<Slot>,
    inner: Arc<Inner>,
    id: CommandId,
}

impl std::fmt::Debug for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pending").field("id", &self.id).finish()
    }
}

impl Pending {
    /// Block until the command finishes.
    pub fn wait(self) -> Result<Done> {
        let mut s = lock(&self.slot.m);
        loop {
            if let Some(r) = s.done.take() {
                s.taken = true;
                return r;
            }
            s = self.slot.cv.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Block until the command finishes or `timeout` passes. On expiry the command's result is
    /// discarded when it arrives, and the statement is cancelled on the server if it is the one
    /// running (never otherwise, because a late cancel could hit the next command).
    pub fn wait_timeout(self, timeout: Duration) -> Result<Done> {
        let deadline = Instant::now() + timeout;
        {
            let mut s = lock(&self.slot.m);
            loop {
                if let Some(r) = s.done.take() {
                    s.taken = true;
                    return r;
                }
                let now = Instant::now();
                if now >= deadline {
                    s.abandoned = true;
                    break;
                }
                s = self
                    .slot
                    .cv
                    .wait_timeout(s, deadline - now)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        }
        let head = self.inner.state().engine.is_head(self.id);
        if head {
            let _ = self.inner.cancel.cancel();
        }
        Err(Error::Timeout)
    }
}

impl Future for Pending {
    type Output = Result<Done>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut s = lock(&self.slot.m);
        if let Some(r) = s.done.take() {
            s.taken = true;
            return Poll::Ready(r);
        }
        s.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        let mut s = lock(&self.slot.m);
        if !s.taken {
            s.abandoned = true;
            s.chunks.clear();
        }
    }
}

/// A streaming result. Dropping it discards the rest; the connection is free again once the
/// server has finished sending it.
pub struct Stream {
    pending: Option<Pending>,
    finished: bool,
}

impl std::fmt::Debug for Stream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stream")
            .field("finished", &self.finished)
            .finish()
    }
}

impl Stream {
    fn p(&self) -> &Pending {
        self.pending.as_ref().expect("stream is alive until drop")
    }

    fn after_pop(&self) {
        // Let the reader know there is room again (under the state lock: no lost wake-up).
        let _g = self.p().inner.state();
        self.p().inner.cv.notify_all();
    }

    /// The next chunk, or `None` at the end. Blocks the calling thread.
    pub fn next_chunk(&mut self) -> Result<Option<Chunk>> {
        if self.finished {
            return Ok(None);
        }
        let slot = self.p().slot.clone();
        let mut s = lock(&slot.m);
        loop {
            if let Some(c) = s.chunks.pop_front() {
                drop(s);
                self.after_pop();
                return Ok(Some(c));
            }
            if let Some(r) = s.done.take() {
                s.taken = true;
                self.finished = true;
                return r.map(|_| None);
            }
            s = slot.cv.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// The next chunk as a future, for any executor.
    pub fn next_chunk_async(&mut self) -> NextChunk<'_> {
        NextChunk { stream: self }
    }
}

/// The future returned by [`Stream::next_chunk_async`].
#[derive(Debug)]
pub struct NextChunk<'a> {
    stream: &'a mut Stream,
}

impl Future for NextChunk<'_> {
    type Output = Result<Option<Chunk>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let st = &mut *self.stream;
        if st.finished {
            return Poll::Ready(Ok(None));
        }
        let slot = st.p().slot.clone();
        let mut s = lock(&slot.m);
        if let Some(c) = s.chunks.pop_front() {
            drop(s);
            st.after_pop();
            return Poll::Ready(Ok(Some(c)));
        }
        if let Some(r) = s.done.take() {
            s.taken = true;
            st.finished = true;
            return Poll::Ready(r.map(|_| None));
        }
        s.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(p) = self.pending.take() {
            {
                let mut s = lock(&p.slot.m);
                if !s.taken {
                    s.abandoned = true;
                    s.chunks.clear();
                }
            }
            // The reader may be waiting for room in this stream's queue.
            let _g = p.inner.state();
            p.inner.cv.notify_all();
        }
    }
}

/// A batch of statements that share one Sync. See [`SharedConnection::pipeline`].
pub struct Pipeline<'c> {
    conn: &'c SharedConnection,
    items: Vec<(String, Vec<OwnedValue>, Class)>,
}

/// A parameter value the pipeline owns until it runs.
#[derive(Debug, Clone, PartialEq)]
pub enum OwnedValue {
    /// `NULL`
    Null,
    /// `bool`
    Bool(bool),
    /// `int2`
    I16(i16),
    /// `int4`
    I32(i32),
    /// `int8`
    I64(i64),
    /// `float4`
    F32(f32),
    /// `float8`
    F64(f64),
    /// `text`
    Text(String),
    /// `bytea`
    Bytes(Vec<u8>),
}

impl OwnedValue {
    fn as_value(&self) -> Value<'_> {
        match self {
            OwnedValue::Null => Value::Null,
            OwnedValue::Bool(v) => Value::Bool(*v),
            OwnedValue::I16(v) => Value::I16(*v),
            OwnedValue::I32(v) => Value::I32(*v),
            OwnedValue::I64(v) => Value::I64(*v),
            OwnedValue::F32(v) => Value::F32(*v),
            OwnedValue::F64(v) => Value::F64(*v),
            OwnedValue::Text(v) => Value::Text(v),
            OwnedValue::Bytes(v) => Value::Bytes(v),
        }
    }
}

impl std::fmt::Debug for Pipeline<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pipeline")
            .field("statements", &self.items.len())
            .finish()
    }
}

impl Pipeline<'_> {
    /// Add a statement whose row count is wanted.
    pub fn exec(mut self, sql: &str, params: Vec<OwnedValue>) -> Self {
        self.items.push((sql.to_owned(), params, Class::Exec));
        self
    }

    /// Add a statement whose rows are wanted.
    pub fn collect(mut self, sql: &str, params: Vec<OwnedValue>) -> Self {
        self.items.push((sql.to_owned(), params, Class::Collect));
        self
    }

    /// Send the batch and wait. The result has one entry per statement, in order. If one fails,
    /// the later ones are reported as [`Error::Skipped`], and none of them took effect (one Sync
    /// means one implicit transaction).
    pub fn run(self) -> Result<Vec<Result<Done>>> {
        let c = self.conn;
        if self.items.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmts = Vec::with_capacity(self.items.len());
        for (sql, params, _) in &self.items {
            let vals: Vec<Value<'_>> = params.iter().map(OwnedValue::as_value).collect();
            stmts.push(c.prepared(sql, &vals)?);
        }
        let vals: Vec<Vec<Value<'_>>> = self
            .items
            .iter()
            .map(|(_, p, _)| p.iter().map(OwnedValue::as_value).collect())
            .collect();
        let refs: Vec<(&Arc<Prepared>, &[Value<'_>], Class)> = stmts
            .iter()
            .zip(&vals)
            .zip(&self.items)
            .map(|((s, v), (_, _, class))| (s, v.as_slice(), *class))
            .collect();
        let pendings: Vec<Pending> = {
            let mut st = c.inner.admit(false)?;
            let ids = st.engine.run_group(&refs)?;
            let mut v = Vec::with_capacity(ids.len());
            for id in ids {
                let slot = Slot::new(false);
                st.slots.insert(id, slot.clone());
                v.push(Pending {
                    slot,
                    inner: c.inner.clone(),
                    id,
                });
            }
            c.inner.ensure_pump(&mut st);
            c.inner.flush(st);
            v
        };
        Ok(pendings.into_iter().map(Pending::wait).collect())
    }
}
