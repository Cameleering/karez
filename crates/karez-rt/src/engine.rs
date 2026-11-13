//! The sans-io session: bytes in, events and bytes out. It never touches a socket.
//!
//! Commands are queued with [`Engine::run`], [`Engine::run_group`] or [`Engine::prepare`]; their
//! wire bytes accumulate in an output buffer taken with [`Engine::take_output`]. Server bytes go
//! in through [`Engine::feed`], and [`Engine::poll_event`] returns what they completed. PostgreSQL
//! answers in the order commands were sent, so a FIFO of in-flight commands is all the state
//! needed to pair answers with commands.
//!
//! Every command is its own Bind/Execute/Sync group, except the members of a group submitted
//! with [`Engine::run_group`], which share one Sync: an error skips the rest of the group.

use crate::error::Error;
use karez_chunk::{Chunk, ChunkBuilder};
use karez_pg::{Error as PgError, Notification, ServerError};
use karez_proto_pg::backend::FieldDescription;
use karez_proto_pg::{
    frontend, BackendMessage, Frame, FrameReader, ProtoError, ResultPlan, TxStatus, Value,
};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Identifies a queued command.
pub type CommandId = u64;

/// What to keep of a command's result rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Only the number of rows affected; rows are discarded undecoded.
    Exec,
    /// All chunks, buffered up to a limit and returned with the completion.
    Collect,
    /// Chunks are delivered as they fill, through [`Event::Chunk`].
    Stream,
}

/// A statement the server has prepared, with the plan for decoding its rows.
#[derive(Debug)]
pub struct Prepared {
    /// The server-side statement name.
    pub name: String,
    /// The parameter types the server resolved.
    pub param_oids: Vec<u32>,
    /// How to decode the result.
    pub plan: ResultPlan,
}

/// The server's description of a freshly parsed statement.
#[derive(Debug, Clone)]
pub struct RawPrepared {
    /// The statement name used in Parse.
    pub name: String,
    /// Parameter type OIDs the server resolved.
    pub param_oids: Vec<u32>,
    /// Result columns (empty for a statement that returns none).
    pub fields: Vec<FieldDescription>,
}

/// The successful completion of a command.
#[derive(Debug, Default)]
pub struct Done {
    /// Rows affected, from the command tag.
    pub rows: u64,
    /// Buffered chunks of a [`Class::Collect`] command.
    pub chunks: Vec<Chunk>,
    /// Set for a prepare command.
    pub prepared: Option<RawPrepared>,
}

/// Something the engine learnt from server bytes.
#[derive(Debug)]
pub enum Event {
    /// A full chunk of a [`Class::Stream`] command.
    Chunk {
        /// The command.
        id: CommandId,
        /// The rows.
        chunk: Chunk,
    },
    /// A command finished.
    Done {
        /// The command.
        id: CommandId,
        /// Its outcome.
        result: Result<Done, Error>,
    },
}

enum Kind {
    Prepare {
        name: String,
        param_oids: Vec<u32>,
        fields: Option<Vec<FieldDescription>>,
    },
    Close,
    Run {
        stmt: Arc<Prepared>,
        class: Class,
        builder: Option<ChunkBuilder>,
        rows: u64,
        chunks: Vec<Chunk>,
        collected: usize,
    },
}

struct Entry {
    id: CommandId,
    kind: Kind,
    /// Its bytes have been handed out by `take_output`.
    sent: bool,
    /// Ends a Sync group: its answer ends with ReadyForQuery.
    sync: bool,
    /// The server's CommandComplete (or EmptyQueryResponse) was seen.
    replied: bool,
    err: Option<Error>,
}

struct CacheEntry {
    stmt: Arc<Prepared>,
    used: u64,
}

struct Core {
    out: Vec<u8>,
    scratch: Vec<u8>,
    queue: VecDeque<Entry>,
    events: VecDeque<Event>,
    next_id: CommandId,
    next_stmt: u64,
    tx_status: TxStatus,
    cache: HashMap<(String, Vec<u32>), CacheEntry>,
    cache_cap: usize,
    tick: u64,
    notifications: VecDeque<Notification>,
    params: HashMap<String, String>,
    max_collect_bytes: usize,
}

/// The sans-io session. See the module documentation.
pub struct Engine {
    reader: FrameReader,
    core: Core,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("in_flight", &self.core.queue.len())
            .field("tx_status", &self.core.tx_status)
            .field("cached", &self.core.cache.len())
            .finish()
    }
}

fn rows_from_tag(tag: &str) -> u64 {
    tag.rsplit(' ')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn proto(msg: &'static str) -> Error {
    Error::Pg(PgError::Proto(ProtoError::InvalidValue(msg)))
}

fn is_stale(e: &ServerError) -> bool {
    e.sqlstate() == "26000"
        || (e.sqlstate() == "0A000"
            && e.message()
                .contains("cached plan must not change result type"))
}

impl Engine {
    /// A session for an already authenticated connection.
    pub fn new(
        max_message: usize,
        cache_cap: usize,
        max_collect_bytes: usize,
        tx_status: TxStatus,
        params: HashMap<String, String>,
    ) -> Self {
        Engine {
            reader: FrameReader::new(max_message),
            core: Core {
                out: Vec::with_capacity(512),
                scratch: Vec::new(),
                queue: VecDeque::new(),
                events: VecDeque::new(),
                next_id: 1,
                next_stmt: 0,
                tx_status,
                cache: HashMap::new(),
                cache_cap: cache_cap.max(1),
                tick: 0,
                notifications: VecDeque::new(),
                params,
                max_collect_bytes,
            },
        }
    }

    /// Commands queued and not yet completed.
    pub fn in_flight(&self) -> usize {
        self.core.queue.len()
    }

    /// True if `id` is the next command the server will answer.
    pub fn is_head(&self, id: CommandId) -> bool {
        self.core.queue.front().is_some_and(|e| e.id == id)
    }

    /// Transaction state as of the last ReadyForQuery.
    pub fn tx_status(&self) -> TxStatus {
        self.core.tx_status
    }

    /// A run-time parameter the server reported.
    pub fn server_param(&self, name: &str) -> Option<&str> {
        self.core.params.get(name).map(String::as_str)
    }

    /// Notifications received, oldest first; empties the queue.
    pub fn take_notifications(&mut self) -> Vec<Notification> {
        self.core.notifications.drain(..).collect()
    }

    /// True if bytes are waiting to be written.
    pub fn has_output(&self) -> bool {
        !self.core.out.is_empty()
    }

    /// The bytes to write to the server. Every queued command is marked as sent.
    pub fn take_output(&mut self) -> Vec<u8> {
        for e in &mut self.core.queue {
            e.sent = true;
        }
        std::mem::take(&mut self.core.out)
    }

    /// The next completed result, if any.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.core.events.pop_front()
    }

    /// Look up a prepared statement by SQL and parameter types.
    pub fn lookup(&mut self, sql: &str, oids: &[u32]) -> Option<Arc<Prepared>> {
        self.core.tick += 1;
        let tick = self.core.tick;
        let e = self.core.cache.get_mut(&(sql.to_owned(), oids.to_vec()))?;
        e.used = tick;
        Some(e.stmt.clone())
    }

    /// Queue Parse and Describe for a statement. Its completion event carries
    /// [`Done::prepared`]; install the plan with [`Engine::install`].
    pub fn prepare(&mut self, sql: &str, oids: &[u32]) -> Result<CommandId, Error> {
        let c = &mut self.core;
        let name = format!("krt_s{}", c.next_stmt);
        c.next_stmt += 1;
        let mark = c.out.len();
        let r = frontend::parse(&mut c.out, &name, sql, oids)
            .and_then(|()| frontend::describe_statement(&mut c.out, &name));
        if let Err(e) = r {
            c.out.truncate(mark);
            return Err(Error::Pg(PgError::Proto(e)));
        }
        frontend::sync(&mut c.out);
        let id = c.push(
            Kind::Prepare {
                name,
                param_oids: oids.to_vec(),
                fields: None,
            },
            true,
        );
        Ok(id)
    }

    /// Record a described statement and its decoding plan in the cache. If another thread cached
    /// the same statement meanwhile, that one is returned and this duplicate is closed.
    pub fn install(
        &mut self,
        sql: &str,
        oids: &[u32],
        raw: RawPrepared,
        plan: ResultPlan,
    ) -> Arc<Prepared> {
        let key = (sql.to_owned(), oids.to_vec());
        if let Some(e) = self.core.cache.get(&key) {
            let existing = e.stmt.clone();
            self.close_statement(&raw.name);
            return existing;
        }
        while self.core.cache.len() >= self.core.cache_cap {
            let oldest = self
                .core
                .cache
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(k, e)| (k.clone(), e.stmt.name.clone()));
            match oldest {
                Some((k, name)) => {
                    self.core.cache.remove(&k);
                    self.close_statement(&name);
                }
                None => break,
            }
        }
        let stmt = Arc::new(Prepared {
            name: raw.name,
            param_oids: raw.param_oids,
            plan,
        });
        self.core.tick += 1;
        self.core.cache.insert(
            key,
            CacheEntry {
                stmt: stmt.clone(),
                used: self.core.tick,
            },
        );
        stmt
    }

    /// Queue a Close for a server-side statement; no event is produced.
    pub fn close_statement(&mut self, name: &str) {
        let c = &mut self.core;
        let mark = c.out.len();
        if frontend::close_statement(&mut c.out, name).is_err() {
            c.out.truncate(mark);
            return;
        }
        frontend::sync(&mut c.out);
        c.push(Kind::Close, true);
    }

    /// Queue Bind, Execute and Sync for a prepared statement.
    pub fn run(
        &mut self,
        stmt: &Arc<Prepared>,
        params: &[Value<'_>],
        class: Class,
    ) -> Result<CommandId, Error> {
        let ids = self.run_group(&[(stmt, params, class)])?;
        Ok(ids[0])
    }

    /// Queue several statements that share one Sync, so an error skips the rest of the group.
    /// Nothing is queued if any of them cannot be encoded.
    pub fn run_group(
        &mut self,
        items: &[(&Arc<Prepared>, &[Value<'_>], Class)],
    ) -> Result<Vec<CommandId>, Error> {
        let mark = self.core.out.len();
        for (stmt, params, _) in items {
            if let Err(e) = self.core.encode_run(stmt, params) {
                self.core.out.truncate(mark);
                return Err(e);
            }
        }
        frontend::sync(&mut self.core.out);
        let n = items.len();
        let mut ids = Vec::with_capacity(n);
        for (i, (stmt, _, class)) in items.iter().enumerate() {
            let builder = (*class != Class::Exec)
                .then(|| ChunkBuilder::new(stmt.plan.fields().to_vec()));
            ids.push(self.core.push(
                Kind::Run {
                    stmt: (*stmt).clone(),
                    class: *class,
                    builder,
                    rows: 0,
                    chunks: Vec::new(),
                    collected: 0,
                },
                i + 1 == n,
            ));
        }
        Ok(ids)
    }

    /// Fail everything in flight because the connection is gone. Commands whose bytes were never
    /// handed out are reported as safe to retry.
    pub fn fail_all(&mut self, reason: &str) {
        while let Some(e) = self.core.queue.pop_front() {
            if matches!(e.kind, Kind::Close) {
                continue;
            }
            let err = Error::Closed {
                retry_safe: !e.sent,
                reason: reason.to_owned(),
            };
            self.core.events.push_back(Event::Done {
                id: e.id,
                result: Err(err),
            });
        }
        self.core.out.clear();
    }

    /// Feed bytes received from the server. An error means the stream cannot continue (bad
    /// framing, an unexpected message, a FATAL error): the caller should [`Engine::fail_all`].
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.reader.push(bytes);
        while let Some((tag, span)) = self
            .reader
            .next_span()
            .map_err(|e| Error::Pg(PgError::Proto(e)))?
        {
            let msg = BackendMessage::parse(Frame {
                tag,
                body: self.reader.span(span),
            })
            .map_err(|e| Error::Pg(PgError::Proto(e)))?;
            self.core.handle(msg)?;
        }
        Ok(())
    }
}

impl Core {
    fn push(&mut self, kind: Kind, sync: bool) -> CommandId {
        let id = self.next_id;
        self.next_id += 1;
        self.queue.push_back(Entry {
            id,
            kind,
            sent: false,
            sync,
            replied: false,
            err: None,
        });
        id
    }

    fn encode_run(&mut self, stmt: &Prepared, params: &[Value<'_>]) -> Result<(), Error> {
        if params.len() != stmt.param_oids.len() {
            return Err(Error::Pg(PgError::Usage(
                "wrong number of parameters for the statement",
            )));
        }
        for (v, &want) in params.iter().zip(&stmt.param_oids) {
            if !matches!(v, Value::Null | Value::Unknown(_)) && v.oid() != want {
                return Err(Error::Pg(PgError::Usage(
                    "parameter type differs from the type the statement was prepared with",
                )));
            }
        }
        self.scratch.clear();
        let mut ranges = Vec::with_capacity(params.len());
        for p in params {
            ranges.push(
                p.encode_into(&mut self.scratch)
                    .map_err(|e| Error::Pg(PgError::Proto(e)))?,
            );
        }
        let slices: Vec<Option<&[u8]>> = ranges
            .iter()
            .map(|r| r.clone().map(|r| &self.scratch[r]))
            .collect();
        let formats: Vec<i16> = params.iter().map(Value::format).collect();
        frontend::bind(
            &mut self.out,
            "",
            &stmt.name,
            &formats,
            &slices,
            stmt.plan.result_formats(),
        )
        .and_then(|()| frontend::execute(&mut self.out, "", 0))
        .map_err(|e| Error::Pg(PgError::Proto(e)))
    }

    fn handle(&mut self, msg: BackendMessage<'_>) -> Result<(), Error> {
        match msg {
            BackendMessage::ParameterStatus { name, value } => {
                self.params.insert(name, value);
                return Ok(());
            }
            BackendMessage::NoticeResponse(_) => return Ok(()),
            BackendMessage::NotificationResponse {
                process_id,
                channel,
                payload,
            } => {
                if self.notifications.len() == 4096 {
                    self.notifications.pop_front();
                }
                self.notifications.push_back(Notification {
                    process_id,
                    channel,
                    payload,
                });
                return Ok(());
            }
            _ => {}
        }
        let Some(head) = self.queue.front_mut() else {
            return Err(match msg {
                BackendMessage::ErrorResponse(f) => Error::Pg(PgError::Server(
                    ServerError::from_fields(f),
                )),
                _ => proto("the server sent a message nobody asked for"),
            });
        };
        match (&mut head.kind, msg) {
            (Kind::Prepare { .. }, BackendMessage::ParseComplete) => {}
            (Kind::Prepare { param_oids, .. }, BackendMessage::ParameterDescription(o)) => {
                *param_oids = o;
            }
            (Kind::Prepare { fields, .. }, BackendMessage::RowDescription(d)) => {
                *fields = Some(d);
            }
            (Kind::Prepare { fields, .. }, BackendMessage::NoData) => *fields = Some(Vec::new()),
            (Kind::Prepare { .. }, BackendMessage::ErrorResponse(f)) => {
                head.err = Some(Error::Pg(PgError::Server(ServerError::from_fields(f))));
            }
            (Kind::Prepare { .. }, BackendMessage::ReadyForQuery(s)) => {
                self.tx_status = s;
                self.finish_group();
            }
            (Kind::Close, BackendMessage::CloseComplete | BackendMessage::ErrorResponse(_)) => {}
            (Kind::Close, BackendMessage::ReadyForQuery(s)) => {
                self.tx_status = s;
                self.queue.pop_front();
            }
            (Kind::Run { .. }, BackendMessage::BindComplete | BackendMessage::PortalSuspended) => {}
            (
                Kind::Run {
                    stmt,
                    class,
                    builder,
                    chunks,
                    collected,
                    ..
                },
                BackendMessage::DataRow(d),
            ) => {
                if head.err.is_none() && *class != Class::Exec {
                    if let Some(b) = builder.as_mut() {
                        if let Err(e) = stmt.plan.decode_row(d.body, b) {
                            head.err = Some(Error::Pg(PgError::Value(e.to_string())));
                        } else if b.is_full() {
                            let id = head.id;
                            let chunk = b.chunk().clone();
                            b.reset();
                            if *class == Class::Stream {
                                self.events.push_back(Event::Chunk { id, chunk });
                            } else {
                                *collected += chunk.byte_size();
                                if *collected > self.max_collect_bytes {
                                    chunks.clear();
                                    head.err = Some(Error::ResultTooLarge {
                                        limit: self.max_collect_bytes,
                                    });
                                } else {
                                    chunks.push(chunk);
                                }
                            }
                        }
                    }
                }
            }
            (Kind::Run { rows, .. }, BackendMessage::CommandComplete(tag)) => {
                *rows = rows_from_tag(&tag);
                head.replied = true;
                if !head.sync && head.err.is_none() {
                    self.finish_one();
                }
            }
            (Kind::Run { .. }, BackendMessage::EmptyQueryResponse) => {
                head.replied = true;
                if !head.sync && head.err.is_none() {
                    self.finish_one();
                }
            }
            (Kind::Run { stmt, .. }, BackendMessage::ErrorResponse(f)) => {
                let e = ServerError::from_fields(f);
                if is_stale(&e) {
                    let name = stmt.name.clone();
                    self.cache.retain(|_, c| c.stmt.name != name);
                }
                let head = self.queue.front_mut().expect("head checked above");
                head.err = Some(Error::Pg(PgError::Server(e)));
            }
            (Kind::Run { .. }, BackendMessage::ReadyForQuery(s)) => {
                self.tx_status = s;
                if !head.replied && head.err.is_none() {
                    return Err(proto("ReadyForQuery before the command completed"));
                }
                self.finish_group();
            }
            (_, other) => {
                let _ = other;
                return Err(proto("unexpected message for the command in flight"));
            }
        }
        Ok(())
    }

    /// Complete the head command, which finished without error and is not the end of a group.
    fn finish_one(&mut self) {
        if let Some(e) = self.queue.pop_front() {
            self.complete(e);
        }
    }

    /// ReadyForQuery arrived: complete the head and, after an error, the rest of its group,
    /// which the server skipped.
    fn finish_group(&mut self) {
        while let Some(e) = self.queue.pop_front() {
            let end = e.sync;
            let first = e.replied || e.err.is_some() || matches!(e.kind, Kind::Prepare { .. });
            if first {
                self.complete(e);
            } else {
                self.events.push_back(Event::Done {
                    id: e.id,
                    result: Err(Error::Skipped),
                });
            }
            if end {
                break;
            }
        }
    }

    fn complete(&mut self, e: Entry) {
        let id = e.id;
        let result = match (e.err, e.kind) {
            (Some(err), _) => Err(err),
            (None, Kind::Prepare {
                name,
                param_oids,
                fields,
            }) => Ok(Done {
                rows: 0,
                chunks: Vec::new(),
                prepared: Some(RawPrepared {
                    name,
                    param_oids,
                    fields: fields.unwrap_or_default(),
                }),
            }),
            (None, Kind::Close) => return,
            (
                None,
                Kind::Run {
                    class,
                    builder,
                    rows,
                    mut chunks,
                    ..
                },
            ) => {
                if let Some(b) = builder.filter(|b| b.rows() > 0) {
                    let chunk = b.chunk().clone();
                    if class == Class::Stream {
                        self.events.push_back(Event::Chunk { id, chunk });
                    } else {
                        chunks.push(chunk);
                    }
                }
                Ok(Done {
                    rows,
                    chunks,
                    prepared: None,
                })
            }
        };
        self.events.push_back(Event::Done { id, result });
    }
}
