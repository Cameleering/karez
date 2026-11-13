use crate::config::{Config, UnsupportedTypes};
use crate::error::{Error, Result, ServerError};
use crate::tx::Isolation;
use crate::tx::Transaction;
use karez_chunk::{Chunk, ChunkBuilder, Field, RowRef};
use karez_proto_pg::scram::{generate_nonce, ScramClient};
use karez_proto_pg::{
    frontend, BackendMessage, ErrorFields, Frame, FrameReader, ProtoError, ResultPlan, TxStatus,
    Value,
};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;

const READ_BUF: usize = 64 * 1024;
const MAX_NOTICES: usize = 64;

fn unexpected(what: &BackendMessage<'_>) -> Error {
    let _ = what;
    Error::Proto(ProtoError::InvalidValue("unexpected message"))
}

/// Messages that may arrive at any time and are not part of a request's reply.
#[derive(Default)]
struct Side {
    params: HashMap<String, String>,
    notices: VecDeque<ErrorFields>,
}

impl Side {
    fn absorb(&mut self, m: BackendMessage<'_>) {
        match m {
            BackendMessage::ParameterStatus { name, value } => {
                self.params.insert(name, value);
            }
            BackendMessage::NoticeResponse(n) => {
                if self.notices.len() == MAX_NOTICES {
                    self.notices.pop_front();
                }
                self.notices.push_back(n);
            }
            _ => {}
        }
    }
}

struct Io {
    stream: TcpStream,
    reader: FrameReader,
    buf: Vec<u8>,
}

impl Io {
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.stream.write_all(bytes)?;
        Ok(())
    }

    fn read_more(&mut self) -> Result<()> {
        loop {
            match self.stream.read(&mut self.buf) {
                Ok(0) => return Err(Error::Closed),
                Ok(n) => {
                    self.reader.push(&self.buf[..n]);
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// The next message that is part of a reply. ParameterStatus, NoticeResponse and
    /// NotificationResponse are absorbed into `side` and skipped.
    fn next_msg<'a>(&'a mut self, side: &mut Side) -> Result<BackendMessage<'a>> {
        loop {
            while !self.reader.frame_ready()? {
                self.read_more()?;
            }
            let (tag, span) = self
                .reader
                .next_span()?
                .ok_or(Error::Usage("internal: frame vanished"))?;
            if matches!(tag, b'S' | b'N' | b'A') {
                let msg = BackendMessage::parse(Frame {
                    tag,
                    body: self.reader.span(span),
                })?;
                side.absorb(msg);
                continue;
            }
            return Ok(BackendMessage::parse(Frame {
                tag,
                body: self.reader.span(span),
            })?);
        }
    }
}

#[derive(Debug)]
struct StmtInner {
    name: String,
    param_oids: Vec<u32>,
    plan: ResultPlan,
}

/// A statement prepared on one connection. Cheap to clone; valid only on the connection that
/// prepared it, and until the server forgets it (for example after a schema change that alters
/// its result columns, which makes execution fail with a clear error; prepare again).
#[derive(Debug, Clone)]
pub struct Statement {
    inner: Arc<StmtInner>,
}

impl Statement {
    /// The result columns.
    pub fn fields(&self) -> &[Field] {
        self.inner.plan.fields()
    }

    /// The parameter type OIDs this statement was prepared with (0 where the server infers).
    pub fn param_oids(&self) -> &[u32] {
        &self.inner.param_oids
    }
}

struct CacheEntry {
    oids: Vec<u32>,
    stmt: Statement,
    used: u64,
}

/// A blocking connection to a PostgreSQL server.
///
/// The calling thread drives the protocol directly: there is no worker thread and no channel.
/// All user values are sent as bound parameters; SQL text is never assembled from them.
pub struct Connection {
    io: Io,
    side: Side,
    out: Vec<u8>,
    scratch: Vec<u8>,
    builder: ChunkBuilder,
    cache: HashMap<String, Vec<CacheEntry>>,
    cache_len: usize,
    cache_cap: usize,
    tick: u64,
    next_stmt: u64,
    tx_status: TxStatus,
    broken: bool,
    closed: bool,
    process_id: i32,
    unsupported: UnsupportedTypes,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("process_id", &self.process_id)
            .field("tx_status", &self.tx_status)
            .field("broken", &self.broken)
            .field("cached_statements", &self.cache_len)
            .finish()
    }
}

fn tcp_connect(cfg: &Config) -> Result<TcpStream> {
    let addrs = (cfg.host.as_str(), cfg.port).to_socket_addrs()?;
    let mut last = None;
    for a in addrs {
        let r = match cfg.connect_timeout {
            Some(t) => TcpStream::connect_timeout(&a, t),
            None => TcpStream::connect(a),
        };
        match r {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(e),
        }
    }
    Err(last
        .map(Error::Io)
        .unwrap_or_else(|| Error::Config(format!("host {:?} resolved to no address", cfg.host))))
}

/// Parse the affected-row count from a command tag such as `INSERT 0 5` or `UPDATE 3`.
fn rows_from_tag(tag: &str) -> u64 {
    tag.rsplit(' ')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn is_stale(e: &ServerError) -> bool {
    e.sqlstate() == "26000"
        || (e.sqlstate() == "0A000"
            && e.message()
                .contains("cached plan must not change result type"))
}

fn builtin_type_name(oid: u32) -> Option<&'static str> {
    Some(match oid {
        18 => "char",
        26 => "oid",
        114 => "json",
        142 => "xml",
        650 => "cidr",
        790 => "money",
        829 => "macaddr",
        869 => "inet",
        1000 => "bool[]",
        1005 => "int2[]",
        1007 => "int4[]",
        1009 => "text[]",
        1016 => "int8[]",
        1021 => "float4[]",
        1022 => "float8[]",
        1083 => "time",
        1186 => "interval",
        1266 => "timetz",
        1560 => "bit",
        1562 => "varbit",
        1700 => "numeric",
        3802 => "jsonb",
        _ => return None,
    })
}

/// Quote an identifier for use in SQL the library builds itself (savepoint names).
pub(crate) fn quote_ident(name: &str) -> Result<String> {
    if name.is_empty() || name.contains('\0') {
        return Err(Error::Usage(
            "identifier must be non-empty and contain no NUL",
        ));
    }
    Ok(format!("\"{}\"", name.replace('"', "\"\"")))
}

impl Connection {
    /// Connect using a URL (see [`Config::from_url`]).
    pub fn connect_url(url: &str) -> Result<Connection> {
        Connection::connect(&Config::from_url(url)?)
    }

    /// Connect and authenticate.
    pub fn connect(cfg: &Config) -> Result<Connection> {
        if !cfg.plaintext {
            return Err(Error::Config(
                "TLS is not implemented in this version; set sslmode=disable to connect without it"
                    .into(),
            ));
        }
        let stream = tcp_connect(cfg)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(cfg.read_timeout)?;
        let mut conn = Connection {
            io: Io {
                stream,
                reader: FrameReader::new(cfg.max_message),
                buf: vec![0; READ_BUF],
            },
            side: Side::default(),
            out: Vec::with_capacity(512),
            scratch: Vec::with_capacity(256),
            builder: ChunkBuilder::new(Vec::new()),
            cache: HashMap::new(),
            cache_len: 0,
            cache_cap: cfg.statement_cache_size.max(1),
            tick: 0,
            next_stmt: 0,
            tx_status: TxStatus::Idle,
            broken: false,
            closed: false,
            process_id: 0,
            unsupported: cfg.unsupported_types,
        };
        conn.startup(cfg)?;
        Ok(conn)
    }

    fn startup(&mut self, cfg: &Config) -> Result<()> {
        enum Step {
            AuthOk,
            Cleartext,
            Md5,
            Sasl(Vec<String>),
            SaslContinue(Vec<u8>),
            SaslFinal(Vec<u8>),
            Failed(Box<ErrorFields>),
            Key(i32),
            Ready(TxStatus),
            Skip,
        }
        let mut params = vec![
            ("user", cfg.user.as_str()),
            ("database", cfg.database.as_str()),
            ("client_encoding", "UTF8"),
        ];
        if let Some(a) = &cfg.application_name {
            params.push(("application_name", a));
        }
        self.out.clear();
        frontend::startup(&mut self.out, &params)?;
        self.io.send(&self.out)?;

        let mut scram: Option<ScramClient> = None;
        let mut exchanged = false;
        let mut verified = false;
        loop {
            let step = match self.io.next_msg(&mut self.side)? {
                BackendMessage::AuthenticationOk => Step::AuthOk,
                BackendMessage::AuthenticationCleartextPassword => Step::Cleartext,
                BackendMessage::AuthenticationMd5Password { .. } => Step::Md5,
                BackendMessage::AuthenticationSasl { mechanisms } => Step::Sasl(mechanisms),
                BackendMessage::AuthenticationSaslContinue(d) => Step::SaslContinue(d.to_vec()),
                BackendMessage::AuthenticationSaslFinal(d) => Step::SaslFinal(d.to_vec()),
                BackendMessage::ErrorResponse(f) => Step::Failed(Box::new(f)),
                BackendMessage::BackendKeyData { process_id, .. } => Step::Key(process_id),
                BackendMessage::ReadyForQuery(s) => Step::Ready(s),
                BackendMessage::NegotiateProtocolVersion { .. } => Step::Skip,
                other => return Err(unexpected(&other)),
            };
            match step {
                Step::AuthOk => {
                    if !exchanged && !cfg.auth.trust {
                        return Err(Error::Auth(
                            "the server did not ask for authentication; allow that explicitly with auth=trust"
                                .into(),
                        ));
                    }
                    if scram.is_some() && !verified {
                        return Err(Error::Auth(
                            "the server finished SCRAM without proving it knows the password"
                                .into(),
                        ));
                    }
                }
                Step::Cleartext => {
                    if !cfg.auth.cleartext {
                        return Err(Error::Auth(
                            "the server asked for a cleartext password; allow that explicitly with auth=password"
                                .into(),
                        ));
                    }
                    let pw = cfg.password_bytes().ok_or_else(|| {
                        Error::Auth("the server needs a password and none was given".into())
                    })?;
                    self.out.clear();
                    frontend::password(&mut self.out, pw)?;
                    self.io.send(&self.out)?;
                    exchanged = true;
                }
                Step::Md5 => {
                    return Err(Error::Auth(
                        "the server asked for MD5, which is not supported".into(),
                    ))
                }
                Step::Sasl(mechs) => {
                    if !cfg.auth.scram {
                        return Err(Error::Auth(
                            "SCRAM authentication is not allowed by auth=".into(),
                        ));
                    }
                    if !mechs.iter().any(|m| m == "SCRAM-SHA-256") {
                        return Err(Error::Auth(format!(
                            "server offers {mechs:?}; only SCRAM-SHA-256 without channel binding is supported"
                        )));
                    }
                    let pw = cfg
                        .password_bytes()
                        .and_then(|p| std::str::from_utf8(p).ok())
                        .ok_or_else(|| {
                            Error::Auth(
                                "the server needs a (UTF-8) password and none was given".into(),
                            )
                        })?;
                    let c = ScramClient::new("", pw, generate_nonce());
                    self.out.clear();
                    frontend::sasl_initial_response(
                        &mut self.out,
                        "SCRAM-SHA-256",
                        c.client_first().as_bytes(),
                    )?;
                    self.io.send(&self.out)?;
                    scram = Some(c);
                    exchanged = true;
                }
                Step::SaslContinue(d) => {
                    let c = scram
                        .as_mut()
                        .ok_or_else(|| Error::Auth("unexpected SASL continue".into()))?;
                    let sf = std::str::from_utf8(&d).map_err(|_| ProtoError::InvalidUtf8)?;
                    let fin = c.client_final(sf)?;
                    self.out.clear();
                    frontend::sasl_response(&mut self.out, fin.as_bytes())?;
                    self.io.send(&self.out)?;
                }
                Step::SaslFinal(d) => {
                    let c = scram
                        .as_ref()
                        .ok_or_else(|| Error::Auth("unexpected SASL final".into()))?;
                    let sf = std::str::from_utf8(&d).map_err(|_| ProtoError::InvalidUtf8)?;
                    c.verify_server_final(sf)?;
                    verified = true;
                }
                Step::Failed(f) => return Err(Error::Server(ServerError::new(*f))),
                Step::Key(pid) => self.process_id = pid,
                Step::Ready(s) => {
                    self.tx_status = s;
                    return Ok(());
                }
                Step::Skip => {}
            }
        }
    }

    /// Backend process id of this session.
    pub fn process_id(&self) -> i32 {
        self.process_id
    }

    /// Transaction state as of the last ReadyForQuery.
    pub fn tx_status(&self) -> TxStatus {
        self.tx_status
    }

    /// A run-time parameter reported by the server (`server_version`, `TimeZone`, ...).
    pub fn server_param(&self, name: &str) -> Option<&str> {
        self.side.params.get(name).map(String::as_str)
    }

    /// Notices and warnings received so far (the most recent 64).
    pub fn notices(&self) -> impl Iterator<Item = &ErrorFields> {
        self.side.notices.iter()
    }

    /// Number of prepared statements currently cached.
    pub fn cached_statements(&self) -> usize {
        self.cache_len
    }

    /// True once a fatal error has made the connection unusable.
    pub fn is_broken(&self) -> bool {
        self.broken
    }

    fn track<T>(&mut self, r: Result<T>) -> Result<T> {
        if let Err(e) = &r {
            if e.is_fatal() {
                self.broken = true;
            }
        }
        r
    }

    fn check_usable(&self) -> Result<()> {
        if self.broken || self.closed {
            Err(Error::Broken)
        } else {
            Ok(())
        }
    }

    /// Close the session politely.
    pub fn close(mut self) -> Result<()> {
        self.closed = true;
        self.out.clear();
        frontend::terminate(&mut self.out);
        self.io.send(&self.out)
    }

    // ---- request plumbing --------------------------------------------------------------

    /// Read until ReadyForQuery, discarding everything else.
    fn drain_to_ready(&mut self) -> Result<()> {
        loop {
            if let BackendMessage::ReadyForQuery(s) = self.io.next_msg(&mut self.side)? {
                self.tx_status = s;
                return Ok(());
            }
        }
    }

    fn lru_evict(&mut self) -> Option<String> {
        let (mut best_key, mut best_idx, mut best_used) = (None, 0, u64::MAX);
        for (k, v) in &self.cache {
            for (i, e) in v.iter().enumerate() {
                if e.used < best_used {
                    best_used = e.used;
                    best_key = Some(k.clone());
                    best_idx = i;
                }
            }
        }
        let k = best_key?;
        let v = self.cache.get_mut(&k)?;
        let e = v.swap_remove(best_idx);
        if v.is_empty() {
            self.cache.remove(&k);
        }
        self.cache_len -= 1;
        Some(e.stmt.inner.name.clone())
    }

    fn close_statement(&mut self, name: &str) -> Result<()> {
        self.out.clear();
        frontend::close_statement(&mut self.out, name)?;
        frontend::sync(&mut self.out);
        self.io.send(&self.out)?;
        loop {
            // A server error (statement already gone) is ignored; the cache entry is dropped anyway.
            if let BackendMessage::ReadyForQuery(s) = self.io.next_msg(&mut self.side)? {
                self.tx_status = s;
                return Ok(());
            }
        }
    }

    fn prepare_new(&mut self, sql: &str, oids: &[u32], cache: bool) -> Result<Statement> {
        let name = format!("karez_s{}", self.next_stmt);
        self.next_stmt += 1;
        self.out.clear();
        frontend::parse(&mut self.out, &name, sql, oids)?;
        frontend::describe_statement(&mut self.out, &name)?;
        frontend::sync(&mut self.out);
        self.io.send(&self.out)?;
        let mut plan = None;
        let mut err = None;
        loop {
            let m = match self.io.next_msg(&mut self.side) {
                Ok(m) => m,
                Err(e) => return Err(self.server_error_or(err.take(), e)),
            };
            match m {
                BackendMessage::ParseComplete | BackendMessage::ParameterDescription(_) => {}
                BackendMessage::RowDescription(d) => plan = Some(ResultPlan::from_description(&d)),
                BackendMessage::NoData => plan = Some(ResultPlan::empty()),
                BackendMessage::ErrorResponse(f) => err = Some(f),
                BackendMessage::ReadyForQuery(s) => {
                    self.tx_status = s;
                    break;
                }
                other => return Err(unexpected(&other)),
            }
        }
        if let Some(f) = err {
            return Err(Error::Server(ServerError::new(f)));
        }
        let plan = plan.ok_or(Error::Proto(ProtoError::InvalidValue("no row description")))?;
        let stmt = Statement {
            inner: Arc::new(StmtInner {
                name,
                param_oids: oids.to_vec(),
                plan,
            }),
        };
        if cache {
            while self.cache_len >= self.cache_cap {
                match self.lru_evict() {
                    Some(old) => self.close_statement(&old)?,
                    None => break,
                }
            }
            self.tick += 1;
            self.cache
                .entry(sql.to_owned())
                .or_default()
                .push(CacheEntry {
                    oids: oids.to_vec(),
                    stmt: stmt.clone(),
                    used: self.tick,
                });
            self.cache_len += 1;
        }
        Ok(stmt)
    }

    /// If the connection died after the server already reported an error (a FATAL such as
    /// `pg_terminate_backend` sends one and closes), report the server's error, not the EOF.
    fn server_error_or(&mut self, recorded: Option<ErrorFields>, e: Error) -> Error {
        self.broken = true;
        match recorded {
            Some(f) => Error::Server(ServerError::new(f)),
            None => e,
        }
    }

    fn cached(&mut self, sql: &str, oids: &[u32]) -> Option<Statement> {
        self.tick += 1;
        let tick = self.tick;
        let e = self
            .cache
            .get_mut(sql)?
            .iter_mut()
            .find(|e| e.oids == oids)?;
        e.used = tick;
        Some(e.stmt.clone())
    }

    fn forget(&mut self, sql: &str, oids: &[u32]) {
        if let Some(v) = self.cache.get_mut(sql) {
            let before = v.len();
            v.retain(|e| e.oids != oids);
            self.cache_len -= before - v.len();
            if v.is_empty() {
                self.cache.remove(sql);
            }
        }
    }

    /// Drop a cached statement the server no longer accepts, so the next `prepare` starts afresh.
    fn forget_by_name(&mut self, name: &str) {
        for v in self.cache.values_mut() {
            let before = v.len();
            v.retain(|e| e.stmt.inner.name != name);
            self.cache_len -= before - v.len();
        }
        self.cache.retain(|_, v| !v.is_empty());
    }

    fn after_prepared_failure(&mut self, stmt: &Statement, r: Result<()>) -> Result<()> {
        if let Err(Error::Server(e)) = &r {
            if is_stale(e) {
                self.forget_by_name(&stmt.inner.name);
                if e.sqlstate() == "0A000" && self.tx_status == TxStatus::Idle {
                    let _ = self.close_statement(&stmt.inner.name);
                }
            }
        }
        r
    }

    fn check_params(stmt: &Statement, params: &[Value<'_>]) -> Result<()> {
        if params.len() != stmt.inner.param_oids.len() {
            return Err(Error::Usage("wrong number of parameters for the statement"));
        }
        for (v, &want) in params.iter().zip(&stmt.inner.param_oids) {
            if !matches!(v, Value::Null | Value::Unknown(_)) && v.oid() != want {
                return Err(Error::Usage(
                    "parameter type differs from the type the statement was prepared with",
                ));
            }
        }
        Ok(())
    }

    /// Send Bind, Execute and Sync for the statement and wait for BindComplete.
    fn bind_and_wait(&mut self, stmt: &Statement, params: &[Value<'_>]) -> Result<()> {
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
            "",
            &stmt.inner.name,
            &formats,
            &slices,
            stmt.inner.plan.result_formats(),
        )?;
        frontend::execute(&mut self.out, "", 0)?;
        frontend::sync(&mut self.out);
        self.io.send(&self.out)?;
        match self.io.next_msg(&mut self.side)? {
            BackendMessage::BindComplete => Ok(()),
            BackendMessage::ErrorResponse(f) => {
                let e = ServerError::new(f);
                self.drain_to_ready()?;
                Err(Error::Server(e))
            }
            other => Err(unexpected(&other)),
        }
    }

    fn type_name(&mut self, oid: u32) -> Result<String> {
        if let Some(n) = builtin_type_name(oid) {
            return Ok(n.to_string());
        }
        let text = oid.to_string();
        let mut cur = self.query(
            "SELECT typname::text FROM pg_type WHERE oid = $1::oid",
            &[Value::Unknown(&text)],
        )?;
        let name = match cur.next()? {
            Some(r) => r.get::<String>(0)?,
            None => format!("oid {oid}"),
        };
        cur.close()?;
        Ok(name)
    }

    fn check_supported(&mut self, stmt: &Statement) -> Result<()> {
        if self.unsupported == UnsupportedTypes::Text {
            return Ok(());
        }
        if let Some((i, oid)) = stmt.inner.plan.first_text_fallback() {
            let type_name = self.type_name(oid)?;
            return Err(Error::UnsupportedType {
                column: stmt.inner.plan.fields()[i].name.clone(),
                oid,
                type_name,
            });
        }
        Ok(())
    }

    fn open(&mut self, sql: &str, params: &[Value<'_>], for_query: bool) -> Result<Statement> {
        self.check_usable()?;
        let oids: Vec<u32> = params.iter().map(Value::oid).collect();
        let (stmt, was_cached) = match self.cached(sql, &oids) {
            Some(s) => (s, true),
            None => (self.prepare_new(sql, &oids, true)?, false),
        };
        if for_query {
            self.check_supported(&stmt)?;
        }
        match self.bind_and_wait(&stmt, params) {
            Ok(()) => Ok(stmt),
            Err(Error::Server(e))
                if was_cached && is_stale(&e) && self.tx_status == TxStatus::Idle =>
            {
                // The server no longer accepts our cached statement (its result columns changed,
                // or it was discarded). Prepare it again once and describe it afresh. This is only
                // possible outside a transaction, where the failed Bind did not abort anything.
                let old = stmt.inner.name.clone();
                self.forget(sql, &oids);
                if e.sqlstate() == "0A000" {
                    self.close_statement(&old)?;
                }
                let fresh = self.prepare_new(sql, &oids, true)?;
                if for_query {
                    self.check_supported(&fresh)?;
                }
                self.bind_and_wait(&fresh, params)?;
                Ok(fresh)
            }
            Err(e) => Err(e),
        }
    }

    // ---- public query API -------------------------------------------------------------

    /// Prepare a statement without running it. `param_oids` are the parameter types (use
    /// [`karez_proto_pg::types::oid`]; 0 lets the server infer). Prepared statements are cached
    /// per connection by SQL text and parameter types.
    pub fn prepare(&mut self, sql: &str, param_oids: &[u32]) -> Result<Statement> {
        self.check_usable()?;
        if let Some(s) = self.cached(sql, param_oids) {
            return Ok(s);
        }
        let r = self.prepare_new(sql, param_oids, true);
        self.track(r)
    }

    /// Run a prepared statement and stream its result.
    pub fn query_prepared(&mut self, stmt: &Statement, params: &[Value<'_>]) -> Result<Cursor<'_>> {
        self.check_usable()?;
        Connection::check_params(stmt, params)?;
        let r = self
            .check_supported(stmt)
            .and_then(|()| self.bind_and_wait(stmt, params));
        let r = self.after_prepared_failure(stmt, r);
        self.track(r)?;
        Ok(self.cursor(stmt.clone()))
    }

    /// Run a prepared statement that returns no rows; returns the affected row count.
    pub fn execute_prepared(&mut self, stmt: &Statement, params: &[Value<'_>]) -> Result<u64> {
        self.check_usable()?;
        Connection::check_params(stmt, params)?;
        let r = self.bind_and_wait(stmt, params);
        let r = self.after_prepared_failure(stmt, r);
        let r = r.and_then(|()| self.finish_execute());
        self.track(r)
    }

    /// Run SQL with bound parameters (`$1`, `$2`, ...) and stream the rows. The statement is
    /// prepared on first use and cached.
    pub fn query(&mut self, sql: &str, params: &[Value<'_>]) -> Result<Cursor<'_>> {
        let r = self.open(sql, params, true);
        let stmt = self.track(r)?;
        Ok(self.cursor(stmt))
    }

    /// Run SQL with bound parameters, discarding any rows; returns the affected row count.
    pub fn execute(&mut self, sql: &str, params: &[Value<'_>]) -> Result<u64> {
        let r = self
            .open(sql, params, false)
            .and_then(|_| self.finish_execute());
        self.track(r)
    }

    fn finish_execute(&mut self) -> Result<u64> {
        let mut rows = 0;
        let mut err = None;
        loop {
            let m = match self.io.next_msg(&mut self.side) {
                Ok(m) => m,
                Err(e) => return Err(self.server_error_or(err.take(), e)),
            };
            match m {
                BackendMessage::DataRow(_)
                | BackendMessage::EmptyQueryResponse
                | BackendMessage::PortalSuspended => {}
                BackendMessage::CommandComplete(tag) => rows = rows_from_tag(&tag),
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
            None => Ok(rows),
        }
    }

    fn cursor(&mut self, stmt: Statement) -> Cursor<'_> {
        self.builder.set_schema(stmt.fields().to_vec());
        Cursor {
            conn: self,
            stmt,
            done: false,
            pos: 0,
            have_chunk: false,
            tag: None,
        }
    }

    /// Run a statement the library itself built (no user data) over the simple protocol.
    pub(crate) fn simple(&mut self, sql: &str) -> Result<()> {
        self.check_usable()?;
        let r = self.simple_inner(sql);
        self.track(r)
    }

    fn simple_inner(&mut self, sql: &str) -> Result<()> {
        self.out.clear();
        frontend::query(&mut self.out, sql)?;
        self.io.send(&self.out)?;
        let mut err = None;
        loop {
            let m = match self.io.next_msg(&mut self.side) {
                Ok(m) => m,
                Err(e) => return Err(self.server_error_or(err.take(), e)),
            };
            match m {
                BackendMessage::CommandComplete(_)
                | BackendMessage::EmptyQueryResponse
                | BackendMessage::RowDescription(_)
                | BackendMessage::DataRow(_) => {}
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

    /// Start a transaction. It rolls back when dropped unless committed.
    pub fn begin(&mut self, isolation: Isolation) -> Result<Transaction<'_>> {
        self.simple(&format!("BEGIN ISOLATION LEVEL {}", isolation.sql()))?;
        Ok(Transaction::new(self))
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if !self.closed && !self.broken {
            self.out.clear();
            frontend::terminate(&mut self.out);
            let _ = self.io.send(&self.out);
        }
    }
}

/// A streaming result. Rows are read through [`Cursor::next`] (a zero-allocation view into the
/// current chunk) or whole chunks through [`Cursor::next_chunk`]. Both come from the same
/// stream; calling `next_chunk` returns the next unread chunk and discards rows of the current
/// chunk that `next` has not yet returned.
///
/// Dropping a cursor before the end reads and discards the rest of the result so the connection
/// stays usable.
pub struct Cursor<'c> {
    conn: &'c mut Connection,
    stmt: Statement,
    done: bool,
    pos: usize,
    have_chunk: bool,
    tag: Option<String>,
}

impl std::fmt::Debug for Cursor<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cursor")
            .field("done", &self.done)
            .field("tag", &self.tag)
            .finish()
    }
}

impl Cursor<'_> {
    /// The result columns.
    pub fn fields(&self) -> &[Field] {
        self.stmt.fields()
    }

    /// The command tag (`SELECT 5`) once the whole result has been read.
    pub fn command_tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    /// True when the whole result has been read.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Read messages until a chunk is full or the result ends. Returns true if the chunk holds rows.
    fn fill(&mut self) -> Result<bool> {
        self.pos = 0;
        self.have_chunk = true;
        self.conn.builder.reset();
        if self.done {
            return Ok(false);
        }
        let conn = &mut *self.conn;
        let mut err: Option<ErrorFields> = None;
        let mut bad_value: Option<String> = None;
        loop {
            let m = match conn.io.next_msg(&mut conn.side) {
                Ok(m) => m,
                Err(e) => {
                    self.done = true;
                    return Err(conn.server_error_or(err.take(), e));
                }
            };
            match m {
                BackendMessage::DataRow(d) => {
                    if err.is_none() && bad_value.is_none() {
                        match self.stmt.inner.plan.decode_row(d.body, &mut conn.builder) {
                            Ok(()) => {
                                if conn.builder.is_full() {
                                    return Ok(true);
                                }
                            }
                            // A value this version cannot represent (for example a date beyond the
                            // Unix-epoch range). The stream itself is intact: keep reading to
                            // ReadyForQuery, then report it. The connection stays usable.
                            Err(ProtoError::BadValue { oid, what }) => {
                                bad_value = Some(format!("value of type oid {oid}: {what}"));
                            }
                            Err(ProtoError::Chunk(c)) => bad_value = Some(c.to_string()),
                            Err(e) => {
                                self.done = true;
                                conn.broken = true; // unread messages remain; the stream is lost
                                return Err(e.into());
                            }
                        }
                    }
                }
                BackendMessage::CommandComplete(t) => self.tag = Some(t),
                BackendMessage::EmptyQueryResponse | BackendMessage::PortalSuspended => {}
                BackendMessage::ErrorResponse(f) => err = Some(f),
                BackendMessage::ReadyForQuery(s) => {
                    conn.tx_status = s;
                    self.done = true;
                    break;
                }
                other => {
                    self.done = true;
                    conn.broken = true;
                    return Err(unexpected(&other));
                }
            }
        }
        if let Some(f) = err {
            conn.builder.reset();
            return Err(Error::Server(ServerError::new(f)));
        }
        if let Some(m) = bad_value {
            conn.builder.reset();
            return Err(Error::Value(m));
        }
        Ok(conn.builder.rows() > 0)
    }

    /// The next row, or `None` at the end. The row borrows the cursor's current chunk.
    #[allow(clippy::should_implement_trait)] // a lending iterator: the row borrows the cursor
    pub fn next(&mut self) -> Result<Option<RowRef<'_>>> {
        let ready = self.have_chunk && self.pos < self.conn.builder.rows();
        if !ready {
            let r = self.fill();
            if !self.conn.track(r)? {
                return Ok(None);
            }
        }
        let i = self.pos;
        self.pos += 1;
        Ok(Some(self.conn.builder.chunk().row(i)?))
    }

    /// The next whole chunk (up to 1,024 rows or 1 MiB), or `None` at the end.
    pub fn next_chunk(&mut self) -> Result<Option<&Chunk>> {
        let r = self.fill();
        if !self.conn.track(r)? {
            return Ok(None);
        }
        self.pos = self.conn.builder.rows();
        Ok(Some(self.conn.builder.chunk()))
    }

    /// Read and discard the rest of the result, reporting an error the server sends on the way.
    pub fn close(mut self) -> Result<()> {
        let r = self.drain();
        self.conn.track(r)
    }

    fn drain(&mut self) -> Result<()> {
        let mut err: Option<ErrorFields> = None;
        while !self.done {
            let conn = &mut *self.conn;
            let m = match conn.io.next_msg(&mut conn.side) {
                Ok(m) => m,
                Err(e) => {
                    self.done = true;
                    return Err(conn.server_error_or(err.take(), e));
                }
            };
            match m {
                BackendMessage::CommandComplete(t) => self.tag = Some(t),
                BackendMessage::ErrorResponse(f) => err = Some(f),
                BackendMessage::ReadyForQuery(s) => {
                    conn.tx_status = s;
                    self.done = true;
                }
                _ => {}
            }
        }
        match err {
            Some(f) => Err(Error::Server(ServerError::new(f))),
            None => Ok(()),
        }
    }
}

impl Drop for Cursor<'_> {
    fn drop(&mut self) {
        if !self.done && !self.conn.broken {
            if let Err(e) = self.drain() {
                if e.is_fatal() {
                    self.conn.broken = true;
                }
            }
        }
    }
}
