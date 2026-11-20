use crate::config::{Config, LoadBalance, TargetSessionAttrs, UnsupportedTypes};
use crate::error::{Error, Result, ServerError};
use crate::transport::Transport;
use crate::tx::Isolation;
use crate::tx::Transaction;
use karez_chunk::{Chunk, ChunkBuilder, Field, RowRef};
use karez_proto_pg::oauth::{self, OAuthChallenge};
use karez_proto_pg::scram::{
    cert_hash_algorithm, certificate_signature_oid, generate_nonce, ChannelBinding, ScramClient,
};
use karez_proto_pg::{
    frontend, BackendMessage, ErrorFields, Frame, FrameReader, ProtoError, ResultPlan, TxStatus,
    Value,
};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

mod copy;
mod pipeline;
mod portal;
pub use copy::CopyWriter;
pub use pipeline::{Batch, BatchOutcome, BatchRows, Pipeline, StepResult};
pub use portal::{Fetched, Portal, PortalCursor};

const READ_BUF: usize = 64 * 1024;
const MAX_NOTICES: usize = 64;
const MAX_NOTIFICATIONS: usize = 4096;
/// A batch larger than this is written by a helper thread while results are read.
const OVERLAP_WRITE_BYTES: usize = 64 * 1024;

/// A `NOTIFY` received on a channel this session listens to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// Process id of the notifying backend.
    pub process_id: i32,
    /// Channel name.
    pub channel: String,
    /// Payload (empty if none was given).
    pub payload: String,
}

/// The password as a SCRAM exchange must use it. PostgreSQL stores `SASLprep(password)` (RFC 4013:
/// non-ASCII spaces become spaces, "mapped to nothing" characters such as the soft hyphen are
/// dropped, then Unicode NFKC) and expects the client to apply the same, so `U+2168` and `IX` are
/// one password. An ASCII password is unchanged by it. With the `saslprep` feature a password that
/// can be prepared is prepared; one that cannot (it holds a prohibited character) is used as typed,
/// which is what PostgreSQL itself does. Without the feature a non-ASCII password is used as typed:
/// correct when it is already in that form, which is usual for text typed on a keyboard.
fn scram_password(pw: &str) -> std::borrow::Cow<'_, str> {
    if pw.is_ascii() {
        return std::borrow::Cow::Borrowed(pw);
    }
    #[cfg(feature = "saslprep")]
    if let Ok(p) = stringprep::saslprep(pw) {
        return std::borrow::Cow::Owned(p.into_owned());
    }
    std::borrow::Cow::Borrowed(pw)
}

fn unexpected(what: &BackendMessage<'_>) -> Error {
    let _ = what;
    Error::Proto(ProtoError::InvalidValue("unexpected message"))
}

/// Messages that may arrive at any time and are not part of a request's reply.
#[derive(Default)]
struct Side {
    params: HashMap<String, String>,
    notices: VecDeque<ErrorFields>,
    notifications: VecDeque<Notification>,
    dropped_notifications: u64,
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
            BackendMessage::NotificationResponse {
                process_id,
                channel,
                payload,
            } => {
                if self.notifications.len() == MAX_NOTIFICATIONS {
                    self.notifications.pop_front();
                    self.dropped_notifications += 1;
                }
                self.notifications.push_back(Notification {
                    process_id,
                    channel,
                    payload,
                });
            }
            _ => {}
        }
    }
}

/// What one statement of a group produced: its row count from the command tag, its decoded rows
/// if it was a query, and a value error if a value could not be held.
#[derive(Debug, Default)]
pub(crate) struct Step {
    pub(crate) rows: u64,
    pub(crate) chunks: Vec<Chunk>,
    pub(crate) value_err: Option<String>,
}

/// How far `read_replies` reads.
#[derive(Clone, Copy)]
enum Until {
    /// To ReadyForQuery (the group's Sync has been sent).
    Ready,
    /// Until this many statements are answered, with no Sync sent yet.
    Steps(usize),
}

/// The replies of a group of statements, filled in by `read_replies`.
struct Replies<'a> {
    stmts: &'a [Statement],
    decode: &'a [bool],
    steps: Vec<Step>,
    cur: Step,
    started: bool,
    err: Option<ErrorFields>,
}

impl<'a> Replies<'a> {
    fn new(stmts: &'a [Statement], decode: &'a [bool]) -> Self {
        Replies {
            stmts,
            decode,
            steps: Vec::with_capacity(stmts.len()),
            cur: Step::default(),
            started: false,
            err: None,
        }
    }

    /// The statement in progress is complete: keep its rows and start the next.
    fn finish_step(&mut self, rows: u64, builder: &mut ChunkBuilder) {
        if self.started && builder.rows() > 0 {
            self.cur.chunks.push(builder.chunk().clone());
        }
        builder.reset();
        self.started = false;
        self.cur.rows = rows;
        self.steps.push(std::mem::take(&mut self.cur));
    }
}

struct Io {
    stream: Transport,
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
    portal_seq: u64,
    tx_status: TxStatus,
    broken: bool,
    closed: bool,
    process_id: i32,
    unsupported: UnsupportedTypes,
    secret: Vec<u8>,
    max_message: usize,
    host: String,
    port: u16,
    connect_timeout: Option<Duration>,
    /// Type OIDs looked up in `pg_type`: true for enums (binary value is the label text).
    enum_types: HashMap<u32, bool>,
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

/// The decision of `target_session_attrs`, given what was learned about the session: `standby`
/// (None if not needed) and `read_only` (None if not needed).
fn session_matches(
    want: TargetSessionAttrs,
    standby: Option<bool>,
    read_only: Option<bool>,
) -> bool {
    match want {
        TargetSessionAttrs::Any => true,
        TargetSessionAttrs::ReadWrite => read_only == Some(false),
        TargetSessionAttrs::ReadOnly => read_only == Some(true),
        TargetSessionAttrs::Primary => standby == Some(false),
        TargetSessionAttrs::Standby | TargetSessionAttrs::PreferStandby => standby == Some(true),
    }
}

/// Put the hosts in a random order. Randomness is `std`'s per-process hash keys: not for
/// cryptography, only to spread load.
fn shuffle(v: &mut [(String, u16)]) {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    for i in (1..v.len()).rev() {
        let r = RandomState::new().build_hasher().finish();
        v.swap(i, (r % (i as u64 + 1)) as usize);
    }
}

/// The Kerberos context type: the real one with the `gss` feature, a placeholder without.
#[cfg(feature = "gss")]
type GssContext = karez_gss::Context;
#[cfg(not(feature = "gss"))]
type GssContext = ();

#[cfg(feature = "gss")]
fn gss_complete(c: &GssContext) -> bool {
    c.is_complete()
}
#[cfg(not(feature = "gss"))]
fn gss_complete(_: &GssContext) -> bool {
    true
}

/// Start a Kerberos exchange with the service `krbsrvname@host` and return the first token.
#[cfg(feature = "gss")]
fn gss_start(cfg: &Config) -> Result<(GssContext, Vec<u8>)> {
    let target = format!("{}@{}", cfg.krbsrvname, cfg.host);
    let login = cfg.gss_login().map(|(u, p)| {
        (
            u,
            std::str::from_utf8(p).map_err(|_| ProtoError::InvalidUtf8),
        )
    });
    let creds = match login {
        Some((user, Ok(password))) => karez_gss::Credentials::Password { user, password },
        Some((_, Err(e))) => return Err(e.into()),
        None => karez_gss::Credentials::Default,
    };
    let mut ctx = karez_gss::Context::new(&target, creds)
        .map_err(|e| Error::Auth(format!("Kerberos: {e}")))?;
    let token = ctx
        .step(None)
        .map_err(|e| Error::Auth(format!("Kerberos: {e}")))?;
    Ok((ctx, token))
}

#[cfg(not(feature = "gss"))]
fn gss_start(_cfg: &Config) -> Result<(GssContext, Vec<u8>)> {
    Err(Error::Auth(
        "the server asked for Kerberos (GSSAPI), which needs karez-pg's `gss` feature".into(),
    ))
}

#[cfg(feature = "gss")]
fn gss_step(ctx: &mut GssContext, token: &[u8]) -> Result<Vec<u8>> {
    ctx.step(Some(token))
        .map_err(|e| Error::Auth(format!("Kerberos: {e}")))
}

#[cfg(not(feature = "gss"))]
fn gss_step(_ctx: &mut GssContext, _token: &[u8]) -> Result<Vec<u8>> {
    Err(Error::Auth("Kerberos needs the `gss` feature".into()))
}

fn tcp_connect(cfg: &Config) -> Result<TcpStream> {
    tcp_connect_to(&cfg.host, cfg.port, cfg.connect_timeout)
}

fn tcp_connect_to(host: &str, port: u16, timeout: Option<Duration>) -> Result<TcpStream> {
    let addrs = (host, port).to_socket_addrs()?;
    let mut last = None;
    for a in addrs {
        let r = match timeout {
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
        .unwrap_or_else(|| Error::Config(format!("host {host:?} resolved to no address"))))
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

/// What is needed to cancel the statement a connection is running, from any thread.
///
/// Get one with [`Connection::cancel_token`]. Cancelling opens a second connection, sends a
/// CancelRequest and waits for the server to close it, which it does after delivering the
/// signal. The cancelled statement fails with SQLSTATE 57014; the connection stays usable. A
/// cancel sent when the connection is idle does nothing, but one that arrives while the
/// connection already runs a later statement cancels that one, so cancel only what you mean to.
#[derive(Clone)]
pub struct CancelToken {
    host: String,
    port: u16,
    pid: i32,
    secret: Vec<u8>,
    timeout: Option<Duration>,
}

impl std::fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancelToken")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("pid", &self.pid)
            .finish_non_exhaustive()
    }
}

impl CancelToken {
    /// Ask the server to cancel the connection's current statement.
    pub fn cancel(&self) -> Result<()> {
        let mut s = tcp_connect_to(&self.host, self.port, self.timeout)?;
        s.set_nodelay(true)?;
        let mut buf = Vec::with_capacity(16 + self.secret.len());
        frontend::cancel_request(&mut buf, self.pid, &self.secret)?;
        s.write_all(&buf)?;
        s.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut sink = [0u8; 16];
        loop {
            match s.read(&mut sink) {
                Ok(0) => return Ok(()),
                Ok(_) => continue,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }
}

/// An established connection taken apart: see [`Connection::into_raw_parts`].
#[derive(Debug)]
pub struct RawParts {
    /// The connected, authenticated socket (ciphertext, if `tls` is set).
    pub stream: TcpStream,
    /// The TLS session over `stream`, if the connection uses TLS.
    #[cfg(feature = "tls")]
    pub tls: Option<rustls::ClientConnection>,
    /// Backend process id.
    pub process_id: i32,
    /// Transaction state at the last ReadyForQuery.
    pub tx_status: TxStatus,
    /// Run-time parameters the server reported.
    pub server_params: HashMap<String, String>,
    /// Largest accepted message.
    pub max_message: usize,
    /// Statement cache capacity from the configuration.
    pub statement_cache_size: usize,
    /// Handling of types without a decoder.
    pub unsupported: UnsupportedTypes,
    /// A token that cancels the running statement.
    pub cancel: CancelToken,
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
    ///
    /// With one host and `target_session_attrs=any` this is a single attempt and its error is
    /// returned as it is. With several hosts, or another `target_session_attrs`, hosts are tried
    /// in order (or in random order with `load_balance_hosts=random`): a host that cannot be
    /// reached, refuses the login, or does not satisfy the attributes is skipped, and if none
    /// is usable the result is [`Error::NoSuitableHost`] listing every attempt. With
    /// `prefer-standby` every host is first tried for a standby, then for any host.
    pub fn connect(cfg: &Config) -> Result<Connection> {
        Transport::check_supported(cfg)?;
        let single = cfg.fallback_hosts.is_empty();
        if single && cfg.target_session_attrs == TargetSessionAttrs::Any {
            return Connection::connect_host(cfg);
        }
        let mut hosts = cfg.hosts();
        if cfg.load_balance_hosts == LoadBalance::Random {
            shuffle(&mut hosts);
        }
        let passes: &[TargetSessionAttrs] = match cfg.target_session_attrs {
            TargetSessionAttrs::PreferStandby => {
                &[TargetSessionAttrs::Standby, TargetSessionAttrs::Any]
            }
            other => &[other][..],
        };
        let mut attempts = Vec::new();
        let mut transient = false;
        for want in passes {
            for (host, port) in &hosts {
                let mut one = cfg.clone();
                one.host = host.clone();
                one.port = *port;
                one.fallback_hosts.clear();
                one.target_session_attrs = TargetSessionAttrs::Any;
                match Connection::connect_host(&one) {
                    Err(e) => {
                        if single {
                            return Err(e);
                        }
                        transient |= e.is_transient();
                        attempts.push(format!("{host}:{port}: {e}"));
                    }
                    Ok(mut conn) => match conn.satisfies(*want) {
                        Ok(true) => return Ok(conn),
                        Ok(false) => {
                            // a standby may be promoted, a primary may fail over: worth a retry
                            transient = true;
                            attempts.push(format!(
                                "{host}:{port}: connected, but not {}",
                                want.as_str()
                            ));
                        }
                        Err(e) => {
                            transient |= e.is_transient();
                            attempts
                                .push(format!("{host}:{port}: could not check the session ({e})"));
                        }
                    },
                }
            }
        }
        Err(Error::NoSuitableHost {
            attempts: attempts.join("; "),
            transient,
        })
    }

    /// Whether this session meets `want`. Uses the parameters the server reported at startup
    /// (`in_hot_standby` and `default_transaction_read_only`, sent by PostgreSQL 14 and later) and
    /// asks with a query only when they are missing.
    fn satisfies(&mut self, want: TargetSessionAttrs) -> Result<bool> {
        let standby_param = self.server_param("in_hot_standby").map(|v| v == "on");
        let default_ro_param = self
            .server_param("default_transaction_read_only")
            .map(|v| v == "on");
        let need_standby = matches!(
            want,
            TargetSessionAttrs::Primary | TargetSessionAttrs::Standby
        );
        let need_ro = matches!(
            want,
            TargetSessionAttrs::ReadWrite | TargetSessionAttrs::ReadOnly
        );
        let mut standby = standby_param;
        let mut ro = None;
        if need_standby && standby.is_none() {
            standby = Some(self.ask_bool("SELECT pg_is_in_recovery()")?);
        }
        if need_ro {
            ro = match (standby_param, default_ro_param) {
                (Some(s), Some(d)) => Some(s || d),
                _ => Some(self.ask_text("SHOW transaction_read_only")? == "on"),
            };
        }
        Ok(session_matches(want, standby, ro))
    }

    fn ask_bool(&mut self, sql: &str) -> Result<bool> {
        let mut cur = self.query(sql, &[])?;
        let v = match cur.next()? {
            Some(row) => row.get::<bool>(0)?,
            None => return Err(Error::Usage("the session check returned no row")),
        };
        drop(cur);
        Ok(v)
    }

    fn ask_text(&mut self, sql: &str) -> Result<String> {
        let mut cur = self.query(sql, &[])?;
        let v = match cur.next()? {
            Some(row) => row.get::<&str>(0)?.to_owned(),
            None => return Err(Error::Usage("the session check returned no row")),
        };
        drop(cur);
        Ok(v)
    }

    fn connect_host(cfg: &Config) -> Result<Connection> {
        match Connection::connect_once(cfg, None) {
            // The server was asked what token it needs (the token source returned none the first
            // time): get one for that challenge and connect again, once.
            Err(Error::OAuthChallenge {
                challenge,
                discovery: true,
            }) => {
                let provider = cfg.oauth.as_ref().ok_or_else(|| {
                    Error::Auth("the server asked for an OAuth token and none is configured".into())
                })?;
                let token = provider
                    .token(Some(&challenge))
                    .map_err(|e| Error::Auth(format!("the OAuth token source failed: {e}")))?;
                if token.is_empty() {
                    return Err(Error::Auth(
                        "the OAuth token source returned no token for the server's challenge"
                            .into(),
                    ));
                }
                Connection::connect_once(cfg, Some(token))
            }
            other => other,
        }
    }

    /// One connection attempt. `token` replaces the token source's first answer (the retry after a
    /// discovery).
    fn connect_once(cfg: &Config, token: Option<String>) -> Result<Connection> {
        let tcp = tcp_connect(cfg)?;
        tcp.set_nodelay(true)?;
        tcp.set_read_timeout(cfg.read_timeout)?;
        // The TLS handshake must not hang forever on a server that never answers.
        if cfg.read_timeout.is_none() {
            tcp.set_read_timeout(cfg.connect_timeout)?;
        }
        let stream = Transport::establish(tcp, cfg)?;
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
            portal_seq: 0,
            tx_status: TxStatus::Idle,
            broken: false,
            closed: false,
            process_id: 0,
            unsupported: cfg.unsupported_types,
            secret: Vec::new(),
            max_message: cfg.max_message,
            host: cfg.host.clone(),
            port: cfg.port,
            connect_timeout: cfg.connect_timeout,
            enum_types: HashMap::new(),
        };
        conn.startup(cfg, token)?;
        Ok(conn)
    }

    fn startup(&mut self, cfg: &Config, token: Option<String>) -> Result<()> {
        enum Step {
            AuthOk,
            Cleartext,
            Gss,
            GssContinue(Vec<u8>),
            Md5,
            Sasl(Vec<String>),
            SaslContinue(Vec<u8>),
            SaslFinal(Vec<u8>),
            Failed(Box<ErrorFields>),
            Key(i32, Vec<u8>),
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

        let mut gss: Option<GssContext> = None;
        let mut scram: Option<ScramClient> = None;
        // OAuth: whether a token was sent, and the server's JSON error once it arrives
        let mut oauth: Option<(bool, Option<OAuthChallenge>)> = None;
        let mut exchanged = false;
        let mut verified = false;
        loop {
            let step = match self.io.next_msg(&mut self.side)? {
                BackendMessage::AuthenticationOk => Step::AuthOk,
                BackendMessage::AuthenticationCleartextPassword => Step::Cleartext,
                BackendMessage::AuthenticationGss => Step::Gss,
                BackendMessage::AuthenticationGssContinue(d) => Step::GssContinue(d.to_vec()),
                BackendMessage::AuthenticationMd5Password { .. } => Step::Md5,
                BackendMessage::AuthenticationSasl { mechanisms } => Step::Sasl(mechanisms),
                BackendMessage::AuthenticationSaslContinue(d) => Step::SaslContinue(d.to_vec()),
                BackendMessage::AuthenticationSaslFinal(d) => Step::SaslFinal(d.to_vec()),
                BackendMessage::ErrorResponse(f) => Step::Failed(Box::new(f)),
                BackendMessage::BackendKeyData { process_id, secret } => {
                    Step::Key(process_id, secret)
                }
                BackendMessage::ReadyForQuery(s) => Step::Ready(s),
                BackendMessage::NegotiateProtocolVersion { .. } => Step::Skip,
                other => return Err(unexpected(&other)),
            };
            match step {
                Step::AuthOk => {
                    // A server that checked our client certificate (`sslcert`) over TLS says
                    // AuthenticationOk with no exchange: that is the login the user asked for, not
                    // a downgrade. Without a certificate it is `trust`, which must be allowed.
                    let by_certificate = cfg.ssl_cert.is_some() && self.io.stream.is_tls();
                    if !exchanged && !cfg.auth.trust && !by_certificate {
                        return Err(Error::Auth(
                            "the server did not ask for authentication; allow that explicitly with auth=trust"
                                .into(),
                        ));
                    }
                    if gss.as_ref().is_some_and(|g| !gss_complete(g)) {
                        return Err(Error::Auth(
                            "the server finished Kerberos login before the security context was established (no mutual authentication)"
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
                Step::Gss => {
                    if !cfg.auth.gss {
                        return Err(Error::Auth(
                            "the server asked for Kerberos (GSSAPI); allow that explicitly with auth=gss"
                                .into(),
                        ));
                    }
                    let (ctx, token) = gss_start(cfg)?;
                    self.out.clear();
                    frontend::gss_response(&mut self.out, &token)?;
                    self.io.send(&self.out)?;
                    gss = Some(ctx);
                    exchanged = true;
                }
                Step::GssContinue(d) => {
                    let ctx = gss
                        .as_mut()
                        .ok_or_else(|| Error::Auth("unexpected GSSAPI continue".into()))?;
                    let out = gss_step(ctx, &d)?;
                    if !out.is_empty() {
                        self.out.clear();
                        frontend::gss_response(&mut self.out, &out)?;
                        self.io.send(&self.out)?;
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
                    let offers = |m: &str| mechs.iter().any(|x| x == m);
                    // OAuth is used only when the application set a token source and allowed it,
                    // and the server offers it; otherwise the choice is SCRAM
                    if cfg.auth.oauth && offers(oauth::MECHANISM) {
                        let provider = cfg.oauth.as_ref().ok_or_else(|| {
                            Error::Auth(
                                "the server offers OAUTHBEARER and auth= allows it, but no token source is set"
                                    .into(),
                            )
                        })?;
                        let tok = match token.as_deref() {
                            Some(t) => t.to_string(),
                            None => provider.token(None).map_err(|e| {
                                Error::Auth(format!("the OAuth token source failed: {e}"))
                            })?,
                        };
                        let first = oauth::initial_response(&tok)?;
                        self.out.clear();
                        frontend::sasl_initial_response(&mut self.out, oauth::MECHANISM, &first)?;
                        self.io.send(&self.out)?;
                        oauth = Some((!tok.is_empty(), None));
                        exchanged = true;
                        continue;
                    }
                    if !cfg.auth.scram {
                        return Err(Error::Auth(
                            "SCRAM authentication is not allowed by auth=".into(),
                        ));
                    }
                    let binding = self.scram_binding(cfg, &mechs)?;
                    let mech = if matches!(binding, ChannelBinding::TlsServerEndPoint(_)) {
                        "SCRAM-SHA-256-PLUS"
                    } else {
                        "SCRAM-SHA-256"
                    };
                    if !offers(mech) {
                        return Err(Error::Auth(format!(
                            "server offers {mechs:?}; this client needs {mech}"
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
                    let prepared = scram_password(pw);
                    let c = ScramClient::with_binding("", &prepared, generate_nonce(), binding);
                    self.out.clear();
                    frontend::sasl_initial_response(
                        &mut self.out,
                        c.mechanism(),
                        c.client_first().as_bytes(),
                    )?;
                    self.io.send(&self.out)?;
                    scram = Some(c);
                    exchanged = true;
                }
                Step::SaslContinue(d) if oauth.is_some() => {
                    // the server rejected the token and sent its JSON error; the RFC wants a single
                    // 0x01 in answer, after which the server ends the exchange with an error
                    let (sent_token, ch) = oauth.as_mut().expect("checked");
                    if ch.is_some() {
                        return Err(Error::Auth(
                            "the server sent a second OAuth error message".into(),
                        ));
                    }
                    *ch = Some(oauth::parse_challenge(&d));
                    let _ = sent_token;
                    self.out.clear();
                    frontend::sasl_response(&mut self.out, oauth::ERROR_ACK)?;
                    self.io.send(&self.out)?;
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
                Step::SaslFinal(_) if oauth.is_some() => {
                    // nothing to verify: the token itself was the proof
                }
                Step::SaslFinal(d) => {
                    let c = scram
                        .as_ref()
                        .ok_or_else(|| Error::Auth("unexpected SASL final".into()))?;
                    let sf = std::str::from_utf8(&d).map_err(|_| ProtoError::InvalidUtf8)?;
                    c.verify_server_final(sf)?;
                    verified = true;
                }
                Step::Failed(mut f) => {
                    // A non-ASCII password sent as typed: PostgreSQL stores passwords after
                    // SASLprep, so a password that SASLprep would change fails here. Say so.
                    if scram.is_some()
                        && f.code == "28P01"
                        && f.hint.is_none()
                        && !cfg!(feature = "saslprep")
                        && cfg.password_bytes().is_some_and(|p| !p.is_ascii())
                    {
                        f.hint = Some(
                            "the password has non-ASCII characters, which PostgreSQL normalises (SASLprep) before it checks a SCRAM login;                              enable the `saslprep` feature of karez-pg, or use an ASCII password"
                                .into(),
                        );
                    }
                    if let Some((sent_token, Some(challenge))) = oauth.take() {
                        // the error that follows the JSON challenge: report what the server
                        // said it needs, not the bare "authentication failed"
                        return Err(Error::OAuthChallenge {
                            challenge,
                            discovery: !sent_token,
                        });
                    }
                    return Err(Error::Server(ServerError::new(*f)));
                }
                Step::Key(pid, secret) => {
                    self.process_id = pid;
                    self.secret = secret;
                }
                Step::Ready(s) => {
                    self.tx_status = s;
                    return Ok(());
                }
                Step::Skip => {}
            }
        }
    }

    /// The channel-binding choice for a SCRAM exchange on this connection, from the policy, whether
    /// the connection is TLS, what the server offers, and the server certificate.
    fn scram_binding(&self, cfg: &Config, mechs: &[String]) -> Result<ChannelBinding> {
        use crate::config::ChannelBindingMode as M;
        let offers_plus = mechs.iter().any(|m| m == "SCRAM-SHA-256-PLUS");
        let tls = self.io.stream.is_tls();
        match cfg.channel_binding {
            M::Disable => Ok(ChannelBinding::None),
            mode => {
                if !tls {
                    return if mode == M::Require {
                        Err(Error::Auth(
                            "channel_binding=require needs a TLS connection".into(),
                        ))
                    } else {
                        Ok(ChannelBinding::None)
                    };
                }
                if !offers_plus {
                    return if mode == M::Require {
                        Err(Error::Auth(
                            "channel_binding=require, but the server did not offer SCRAM-SHA-256-PLUS"
                                .into(),
                        ))
                    } else {
                        // we could bind and the server did not offer it: `y` lets a server that
                        // does support it detect that the offer was stripped on the way
                        Ok(ChannelBinding::Supported)
                    };
                }
                let data = self.server_certificate_hash();
                match (data, mode) {
                    (Some(d), _) => Ok(ChannelBinding::TlsServerEndPoint(d)),
                    (None, M::Require) => Err(Error::Auth(
                        "channel_binding=require, but the server certificate's signature algorithm has no hash to bind to"
                            .into(),
                    )),
                    (None, _) => Ok(ChannelBinding::None),
                }
            }
        }
    }

    /// The `tls-server-end-point` data (RFC 5929) for the server's certificate: its hash under the
    /// hash of its own signature algorithm, with SHA-256 for MD5 and SHA-1.
    fn server_certificate_hash(&self) -> Option<Vec<u8>> {
        let der = self.io.stream.server_certificate()?;
        let alg = cert_hash_algorithm(certificate_signature_oid(&der)?)?;
        Some(alg.digest(&der))
    }

    /// Give up the connection object and return its socket and session facts, for a layer that
    /// drives the socket itself (`karez-rt`). Fails if the connection is broken or has bytes
    /// buffered that were not consumed (an idle server sends none).
    pub fn into_raw_parts(mut self) -> Result<RawParts> {
        self.check_usable()?;
        if self.io.reader.buffered() != 0 {
            return Err(Error::Usage("the connection has unread bytes buffered"));
        }
        #[cfg(feature = "tls")]
        let (stream, tls) = match self.io.stream.take_tls() {
            Some((sock, conn)) => (sock, Some(conn)),
            None => (self.io.stream.try_clone_plain()?, None),
        };
        #[cfg(not(feature = "tls"))]
        let stream = self.io.stream.try_clone_plain()?;
        let parts = RawParts {
            stream,
            #[cfg(feature = "tls")]
            tls,
            process_id: self.process_id,
            tx_status: self.tx_status,
            server_params: self.side.params.clone(),
            max_message: self.max_message,
            statement_cache_size: self.cache_cap,
            unsupported: self.unsupported,
            cancel: self.cancel_token(),
        };
        self.closed = true; // Drop must not send Terminate
        Ok(parts)
    }

    /// A token that cancels this connection's running statement from another thread.
    pub fn cancel_token(&self) -> CancelToken {
        CancelToken {
            host: self.host.clone(),
            port: self.port,
            pid: self.process_id,
            secret: self.secret.clone(),
            timeout: self.connect_timeout,
        }
    }

    /// Notifications received so far (from `LISTEN`ed channels), oldest first; empties the queue.
    pub fn take_notifications(&mut self) -> Vec<Notification> {
        self.side.notifications.drain(..).collect()
    }

    /// Number of notifications dropped because more than 4,096 were queued unread.
    pub fn dropped_notifications(&self) -> u64 {
        self.side.dropped_notifications
    }

    /// Wait for the next notification. `None` waits indefinitely; `Some(d)` returns `Ok(None)`
    /// after `d` with nothing received. Use it on an idle connection after `LISTEN`.
    pub fn wait_for_notification(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<Option<Notification>> {
        self.check_usable()?;
        if let Some(n) = self.side.notifications.pop_front() {
            return Ok(Some(n));
        }
        if timeout == Some(Duration::ZERO) {
            return Ok(None);
        }
        let old = self.io.stream.read_timeout()?;
        self.io.stream.set_read_timeout(timeout)?;
        let r = self.wait_inner();
        let _ = self.io.stream.set_read_timeout(old);
        self.track(r)
    }

    fn wait_inner(&mut self) -> Result<Option<Notification>> {
        loop {
            while self.io.reader.frame_ready()? {
                let (tag, span) = self
                    .io
                    .reader
                    .next_span()?
                    .ok_or(Error::Usage("internal: frame vanished"))?;
                let msg = BackendMessage::parse(Frame {
                    tag,
                    body: self.io.reader.span(span),
                })?;
                match msg {
                    BackendMessage::ErrorResponse(f) => {
                        self.broken = true;
                        return Err(Error::Server(ServerError::new(f)));
                    }
                    BackendMessage::NotificationResponse { .. } => {
                        self.side.absorb(msg);
                        return Ok(self.side.notifications.pop_front());
                    }
                    BackendMessage::ParameterStatus { .. } | BackendMessage::NoticeResponse(_) => {
                        self.side.absorb(msg)
                    }
                    other => return Err(unexpected(&other)),
                }
            }
            match self.io.stream.read(&mut self.io.buf) {
                Ok(0) => return Err(Error::Closed),
                Ok(n) => self.io.reader.push(&self.io.buf[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None)
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Run a `COPY ... FROM STDIN` statement, sending everything `data` yields. Returns the
    /// number of rows copied. `sql` is sent as written (COPY takes no bound parameters): build it
    /// only from trusted text. The data itself is never part of the SQL.
    pub fn copy_in(&mut self, sql: &str, data: &mut dyn Read) -> Result<u64> {
        self.check_usable()?;
        let r = self.copy_in_inner(sql, data);
        self.track(r)
    }

    fn copy_in_inner(&mut self, sql: &str, data: &mut dyn Read) -> Result<u64> {
        self.copy_in_begin(sql)?;
        let mut buf = vec![0u8; READ_BUF];
        let mut io_err = None;
        loop {
            match data.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.copy_in_send(&buf[..n])?,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    io_err = Some(e);
                    break;
                }
            }
        }
        let fail = io_err.as_ref().map(|e| format!("client read error: {e}"));
        let (rows, err) = self.copy_in_end(fail.as_deref())?;
        if let Some(e) = io_err {
            return Err(Error::Caller(e));
        }
        match err {
            Some(f) => Err(Error::Server(ServerError::new(f))),
            None => Ok(rows),
        }
    }

    /// Send the COPY statement and wait for the server to be ready for data. Returns whether the
    /// stream is binary and the column count. Pair with [`Connection::copy_in_end`].
    fn copy_in_begin(&mut self, sql: &str) -> Result<(bool, u16)> {
        self.out.clear();
        frontend::query(&mut self.out, sql)?;
        self.io.send(&self.out)?;
        let mut err = None;
        loop {
            match self.io.next_msg(&mut self.side)? {
                BackendMessage::CopyInResponse { binary, columns } => return Ok((binary, columns)),
                BackendMessage::ErrorResponse(f) => err = Some(f),
                BackendMessage::CopyOutResponse { .. } => {
                    // Wrong direction: stop the stream the server is about to send.
                    self.drain_to_ready()?;
                    return Err(Error::Usage(
                        "the statement is COPY TO, not COPY FROM STDIN",
                    ));
                }
                BackendMessage::ReadyForQuery(s) => {
                    self.tx_status = s;
                    return Err(match err.take() {
                        Some(f) => Error::Server(ServerError::new(f)),
                        None => Error::Usage("the statement is not COPY ... FROM STDIN"),
                    });
                }
                BackendMessage::CommandComplete(_)
                | BackendMessage::EmptyQueryResponse
                | BackendMessage::RowDescription(_)
                | BackendMessage::DataRow(_) => {}
                other => return Err(unexpected(&other)),
            }
        }
    }

    /// Send one `CopyData` message.
    fn copy_in_send(&mut self, data: &[u8]) -> Result<()> {
        self.out.clear();
        frontend::copy_data(&mut self.out, data)?;
        self.io.send(&self.out)
    }

    /// End the stream (`CopyFail` with the message when `fail` is set, `CopyDone` otherwise) and
    /// read to `ReadyForQuery`. Returns the row count and the server's error, if it sent one.
    fn copy_in_end(&mut self, fail: Option<&str>) -> Result<(u64, Option<ErrorFields>)> {
        self.out.clear();
        match fail {
            Some(m) => frontend::copy_fail(&mut self.out, m)?,
            None => frontend::copy_done(&mut self.out),
        }
        self.io.send(&self.out)?;
        let mut rows = 0;
        let mut err = None;
        loop {
            match self.io.next_msg(&mut self.side)? {
                BackendMessage::CommandComplete(tag) => rows = rows_from_tag(&tag),
                BackendMessage::ErrorResponse(f) => err = Some(f),
                BackendMessage::ReadyForQuery(s) => {
                    self.tx_status = s;
                    return Ok((rows, err));
                }
                other => return Err(unexpected(&other)),
            }
        }
    }

    /// Run a `COPY ... TO STDOUT` statement, writing the stream to `out`. Returns the number of
    /// rows copied. See [`Connection::copy_in`] for the rule about `sql`.
    pub fn copy_out(&mut self, sql: &str, out: &mut dyn Write) -> Result<u64> {
        self.check_usable()?;
        let r = self.copy_out_inner(sql, out);
        self.track(r)
    }

    fn copy_out_inner(&mut self, sql: &str, out: &mut dyn Write) -> Result<u64> {
        self.out.clear();
        frontend::query(&mut self.out, sql)?;
        self.io.send(&self.out)?;
        let mut err = None;
        loop {
            match self.io.next_msg(&mut self.side)? {
                BackendMessage::CopyOutResponse { .. } => break,
                BackendMessage::ErrorResponse(f) => err = Some(f),
                BackendMessage::CopyInResponse { .. } => {
                    self.out.clear();
                    frontend::copy_fail(&mut self.out, "not a COPY TO statement")?;
                    self.io.send(&self.out)?;
                    self.drain_to_ready()?;
                    return Err(Error::Usage(
                        "the statement is COPY FROM STDIN, not COPY TO",
                    ));
                }
                BackendMessage::ReadyForQuery(s) => {
                    self.tx_status = s;
                    return Err(match err.take() {
                        Some(f) => Error::Server(ServerError::new(f)),
                        None => Error::Usage("the statement is not COPY ... TO STDOUT"),
                    });
                }
                BackendMessage::CommandComplete(_)
                | BackendMessage::EmptyQueryResponse
                | BackendMessage::RowDescription(_)
                | BackendMessage::DataRow(_) => {}
                other => return Err(unexpected(&other)),
            }
        }
        let mut rows = 0;
        let mut io_err = None;
        loop {
            match self.io.next_msg(&mut self.side)? {
                BackendMessage::CopyData(d) => {
                    if io_err.is_none() {
                        if let Err(e) = out.write_all(d) {
                            io_err = Some(e);
                        }
                    }
                }
                BackendMessage::CopyDone => {}
                BackendMessage::CommandComplete(tag) => rows = rows_from_tag(&tag),
                BackendMessage::ErrorResponse(f) => err = Some(f),
                BackendMessage::ReadyForQuery(s) => {
                    self.tx_status = s;
                    break;
                }
                other => return Err(unexpected(&other)),
            }
        }
        if let Some(e) = io_err {
            return Err(Error::Caller(e));
        }
        match err {
            Some(f) => Err(Error::Server(ServerError::new(f))),
            None => Ok(rows),
        }
    }

    /// Run many statements with one network round trip: every Bind and Execute goes out in a
    /// single write followed by one Sync, then the results are read. Returns the rows affected by
    /// each statement, in order.
    ///
    /// One Sync means one implicit transaction: if any statement fails, none of them takes
    /// effect (inside an explicit transaction the transaction is aborted instead) and the
    /// server's error is returned. All statements must fit the statement cache together.
    /// Rows a statement returns are discarded; use [`Connection::query`] to read them.
    pub fn execute_many(&mut self, items: &[(&str, &[Value<'_>])]) -> Result<Vec<u64>> {
        self.check_usable()?;
        let r = self.execute_many_inner(items);
        self.track(r)
    }

    fn execute_many_inner(&mut self, items: &[(&str, &[Value<'_>])]) -> Result<Vec<u64>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let mut distinct: Vec<(&str, Vec<u32>)> = Vec::new();
        for (sql, params) in items {
            let oids: Vec<u32> = params.iter().map(Value::oid).collect();
            if !distinct.iter().any(|(s, o)| s == sql && *o == oids) {
                distinct.push((sql, oids));
            }
        }
        if distinct.len() > self.cache_cap {
            return Err(Error::Usage(
                "the batch uses more distinct statements than the statement cache holds",
            ));
        }
        let mut stmts = Vec::with_capacity(items.len());
        for (sql, params) in items {
            let oids: Vec<u32> = params.iter().map(Value::oid).collect();
            let stmt = match self.cached(sql, &oids) {
                Some(s) => s,
                None => self.prepare_new(sql, &oids, true)?,
            };
            Self::check_params(&stmt, params)?;
            stmts.push(stmt);
        }
        let decode = vec![false; stmts.len()];
        let params: Vec<&[Value<'_>]> = items.iter().map(|(_, p)| *p).collect();
        let (steps, err) = self.run_group(&stmts, &params, &decode)?;
        if let Some(f) = err {
            return Err(Error::Server(ServerError::new(f)));
        }
        if steps.len() != items.len() {
            return Err(Error::Proto(ProtoError::InvalidValue(
                "batch result count differs from statement count",
            )));
        }
        Ok(steps.iter().map(|s| s.rows).collect())
    }

    /// Run prepared statements with one Sync: encode every Bind and Execute, send them (in windows
    /// or with a writer thread when the batch is large), and read the replies. `decode[i]` says
    /// whether statement `i`'s rows are decoded into chunks (a query) or discarded (an execute).
    /// Returns the steps that completed, and the server's error if one stopped the group; the
    /// statements after the failing one were skipped by the server.
    fn run_group(
        &mut self,
        stmts: &[Statement],
        params: &[&[Value<'_>]],
        decode: &[bool],
    ) -> Result<(Vec<Step>, Option<ErrorFields>)> {
        self.out.clear();
        // where each statement's Bind and Execute end in `out`, for sending a batch in windows
        let mut bounds = Vec::with_capacity(stmts.len());
        for (params, stmt) in params.iter().zip(stmts) {
            self.scratch.clear();
            let mut ranges = Vec::with_capacity(params.len());
            for p in params.iter() {
                ranges.push(p.encode_into(&mut self.scratch)?);
            }
            let slices: Vec<Option<&[u8]>> = ranges
                .iter()
                .map(|r| r.clone().map(|r| &self.scratch[r]))
                .collect();
            let formats: Vec<i16> = params.iter().map(Value::format).collect();
            frontend::bind(
                &mut self.out,
                "",
                &stmt.inner.name,
                &formats,
                &slices,
                stmt.inner.plan.result_formats(),
            )?;
            frontend::execute(&mut self.out, "", 0)?;
            bounds.push(self.out.len());
        }
        frontend::sync(&mut self.out);
        let mut rep = Replies::new(stmts, decode);
        if self.out.len() <= OVERLAP_WRITE_BYTES {
            self.io.send(&self.out)?;
            self.read_replies(&mut rep, Until::Ready)?;
        } else if self.io.stream.is_tls() {
            // A TLS stream cannot be split between a reader and a writer thread, so the batch goes
            // out in windows of about 64 KiB, each followed by a Flush and a read of its results.
            self.send_in_windows(&bounds, &mut rep)?;
        } else {
            self.write_while_reading(&mut rep)?;
        }
        Ok((rep.steps, rep.err))
    }

    /// Send the encoded batch (`self.out`, statement ends in `bounds`, then a Sync) in windows of
    /// at least [`OVERLAP_WRITE_BYTES`], reading the results of each window before sending the
    /// next. Never more than about one window is unread by the server, so neither side can block
    /// the other, and no second thread is needed. The batch still ends in one Sync, so it is one
    /// implicit transaction, as in the unwindowed path.
    fn send_in_windows(&mut self, bounds: &[usize], rep: &mut Replies<'_>) -> Result<()> {
        let out = std::mem::take(&mut self.out);
        let r = self.send_in_windows_inner(&out, bounds, rep);
        self.out = out;
        self.out.clear();
        r
    }

    fn send_in_windows_inner(
        &mut self,
        out: &[u8],
        bounds: &[usize],
        rep: &mut Replies<'_>,
    ) -> Result<()> {
        let mut start = 0usize;
        for (i, &end) in bounds.iter().enumerate() {
            let last = i + 1 == bounds.len();
            if last || end - start < OVERLAP_WRITE_BYTES {
                continue;
            }
            let mut msg = Vec::new();
            frontend::flush(&mut msg);
            self.io.send(&out[start..end])?;
            self.io.send(&msg)?;
            // read until the statements sent so far are answered; after an error the server skips
            // to the next Sync, so `read_replies` sends it and reads to ReadyForQuery
            if self.read_replies(rep, Until::Steps(i + 1))? {
                return Ok(());
            }
            start = end;
        }
        // the rest and the Sync
        self.io.send(&out[start..])?;
        self.read_replies(rep, Until::Ready)?;
        Ok(())
    }

    /// Send the encoded batch from a helper thread while this thread reads the results. A server
    /// stops reading its input when it cannot write its output, so a client that writes
    /// everything before reading anything can block forever (both sides stuck in `write`).
    fn write_while_reading(&mut self, rep: &mut Replies<'_>) -> Result<()> {
        let out = std::mem::take(&mut self.out);
        let mut writer = self.io.stream.try_clone_plain()?;
        let (read, written) = std::thread::scope(|sc| {
            let w = sc.spawn(move || writer.write_all(&out).map(|()| out));
            let read = self.read_replies(rep, Until::Ready);
            if matches!(&read, Err(e) if e.is_fatal()) || self.broken {
                // Unblock the writer: the connection is unusable after this error anyway. A
                // server error is not fatal: the server reads on to the Sync, so the writer ends.
                let _ = self.io.stream.shutdown(std::net::Shutdown::Both);
            }
            let written = w
                .join()
                .unwrap_or_else(|_| Err(std::io::Error::other("batch writer thread panicked")));
            (read, written)
        });
        match (read, written) {
            (Ok(_), Ok(out)) => {
                self.out = out;
                self.out.clear();
                Ok(())
            }
            (Err(e), _) => Err(e),
            (Ok(_), Err(e)) => Err(Error::Io(e)),
        }
    }

    /// Read replies of a Bind and Execute group into `rep`. Returns true if the server's
    /// ReadyForQuery was reached (the group is over), false if `until` was met first.
    fn read_replies(&mut self, rep: &mut Replies<'_>, until: Until) -> Result<bool> {
        loop {
            let m = match self.io.next_msg(&mut self.side) {
                Ok(m) => m,
                Err(e) => return Err(self.server_error_or(rep.err.take(), e)),
            };
            match m {
                BackendMessage::BindComplete | BackendMessage::PortalSuspended => {}
                BackendMessage::DataRow(d) => {
                    let i = rep.steps.len();
                    if rep.err.is_none()
                        && rep.cur.value_err.is_none()
                        && rep.decode.get(i).copied().unwrap_or(false)
                    {
                        let plan = &rep.stmts[i].inner.plan;
                        if !rep.started {
                            self.builder.set_schema(plan.fields().to_vec());
                            rep.started = true;
                        }
                        match plan.decode_row(d.body, &mut self.builder) {
                            Ok(()) => {
                                if self.builder.is_full() {
                                    rep.cur.chunks.push(self.builder.chunk().clone());
                                    self.builder.reset();
                                }
                            }
                            // a value this version cannot hold: the stream is intact, so the step
                            // fails with a value error and the group carries on
                            Err(ProtoError::BadValue { oid, what }) => {
                                rep.cur.value_err =
                                    Some(format!("value of type oid {oid}: {what}"));
                            }
                            Err(ProtoError::Chunk(c)) => rep.cur.value_err = Some(c.to_string()),
                            Err(e) => {
                                self.broken = true; // unread messages remain
                                return Err(e.into());
                            }
                        }
                    }
                }
                BackendMessage::CommandComplete(_) | BackendMessage::EmptyQueryResponse
                    if rep.steps.len() >= rep.stmts.len() =>
                {
                    self.broken = true;
                    return Err(Error::Proto(ProtoError::InvalidValue(
                        "more command completions than statements in the group",
                    )));
                }
                BackendMessage::CommandComplete(tag) => {
                    rep.finish_step(rows_from_tag(&tag), &mut self.builder);
                    if matches!(until, Until::Steps(n) if rep.steps.len() >= n) {
                        return Ok(false);
                    }
                }
                BackendMessage::EmptyQueryResponse => {
                    rep.finish_step(0, &mut self.builder);
                    if matches!(until, Until::Steps(n) if rep.steps.len() >= n) {
                        return Ok(false);
                    }
                }
                BackendMessage::ErrorResponse(f) => {
                    rep.err = Some(f);
                    if matches!(until, Until::Steps(_)) {
                        // no Sync has been sent yet and the server is skipping input until one
                        // arrives: send it and read to ReadyForQuery
                        let mut sync = Vec::new();
                        frontend::sync(&mut sync);
                        self.io.send(&sync)?;
                        loop {
                            if let BackendMessage::ReadyForQuery(st) =
                                self.io.next_msg(&mut self.side)?
                            {
                                self.tx_status = st;
                                return Ok(true);
                            }
                        }
                    }
                }
                BackendMessage::ReadyForQuery(s) => {
                    self.tx_status = s;
                    self.builder.reset();
                    return Ok(true);
                }
                other => return Err(unexpected(&other)),
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

    /// Probe an idle connection without a round trip: is the server still there?
    ///
    /// Reads whatever the server has already sent, without waiting. End of stream, a reset, an
    /// `ErrorResponse` (a server shutting down or terminating the session sends `FATAL` first)
    /// or any message a command did not ask for means the connection is dead, and it is marked
    /// broken. `ParameterStatus`, notices and notifications are kept for the next command. A
    /// connection with a command in progress (a cursor or a copy open) is not idle and this
    /// returns `false` for it. A server that vanished without closing the socket (a cable
    /// pulled, a firewall that drops packets) cannot be seen this way; only a command, or the
    /// operating system's keep-alive, finds it.
    pub fn is_alive(&mut self) -> bool {
        if self.broken {
            return false;
        }
        let alive = self.probe_idle();
        if !alive {
            self.broken = true;
        }
        alive
    }

    fn probe_idle(&mut self) -> bool {
        if self.io.stream.set_nonblocking(true).is_err() {
            return false;
        }
        let mut alive = true;
        // an idle server sends nothing, or a few messages; a bound keeps a babbling one from
        // holding this forever
        for _ in 0..64 {
            match self.io.stream.read(&mut self.io.buf) {
                Ok(0) => {
                    alive = false;
                    break;
                }
                Ok(n) => self.io.reader.push(&self.io.buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    alive = false;
                    break;
                }
            }
        }
        if self.io.stream.set_nonblocking(false).is_err() {
            return false;
        }
        while alive {
            match self.io.reader.frame_ready() {
                Ok(true) => {}
                Ok(false) => break,
                Err(_) => return false,
            }
            let Ok(Some((tag, span))) = self.io.reader.next_span() else {
                return false;
            };
            let Ok(msg) = BackendMessage::parse(Frame {
                tag,
                body: self.io.reader.span(span),
            }) else {
                return false;
            };
            match msg {
                BackendMessage::ParameterStatus { .. }
                | BackendMessage::NoticeResponse(_)
                | BackendMessage::NotificationResponse { .. } => self.side.absorb(msg),
                // an ErrorResponse here is the server saying goodbye; anything else is a stream
                // nobody should reuse
                _ => alive = false,
            }
        }
        alive
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

    /// Look up, once per connection, which result types unknown to the codec are enums.
    fn resolve_enums(&mut self, desc: &[karez_proto_pg::backend::FieldDescription]) -> Result<()> {
        for d in desc {
            let o = d.type_oid;
            if ResultPlan::knows_type(o) || self.enum_types.contains_key(&o) {
                continue;
            }
            let text = o.to_string();
            let mut cur = self.query(
                "SELECT typtype::text FROM pg_type WHERE oid = $1::oid",
                &[Value::Unknown(&text)],
            )?;
            let is_enum = match cur.next()? {
                Some(r) => r.get::<&str>(0)? == "e",
                None => false,
            };
            cur.close()?;
            self.enum_types.insert(o, is_enum);
        }
        Ok(())
    }

    fn prepare_new(&mut self, sql: &str, oids: &[u32], cache: bool) -> Result<Statement> {
        let name = format!("karez_s{}", self.next_stmt);
        self.next_stmt += 1;
        self.out.clear();
        frontend::parse(&mut self.out, &name, sql, oids)?;
        frontend::describe_statement(&mut self.out, &name)?;
        frontend::sync(&mut self.out);
        self.io.send(&self.out)?;
        let mut desc = None;
        let mut described_params = None;
        let mut no_data = false;
        let mut err = None;
        loop {
            let m = match self.io.next_msg(&mut self.side) {
                Ok(m) => m,
                Err(e) => return Err(self.server_error_or(err.take(), e)),
            };
            match m {
                BackendMessage::ParseComplete => {}
                BackendMessage::ParameterDescription(p) => described_params = Some(p),
                BackendMessage::RowDescription(d) => desc = Some(d),
                BackendMessage::NoData => no_data = true,
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
        let plan = match desc {
            Some(d) => {
                self.resolve_enums(&d)?;
                let enums = &self.enum_types;
                ResultPlan::from_description_with(&d, &|o| enums.get(&o).copied().unwrap_or(false))
            }
            None if no_data => ResultPlan::empty(),
            None => return Err(Error::Proto(ProtoError::InvalidValue("no row description"))),
        };
        let stmt = Statement {
            inner: Arc::new(StmtInner {
                name,
                // The server's own resolution of every parameter type, falling back to what
                // was declared if it did not describe them.
                param_oids: described_params.unwrap_or_else(|| oids.to_vec()),
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
///
/// A `Cursor` reads the result from the socket as it goes, so it **owns the connection until it
/// ends**: this is how the protocol delivers one result, and no other statement can run meanwhile.
/// To read a large result while the connection serves other statements, or to read several results
/// in turn, use a [`PortalCursor`] (`Connection::portal_cursor`), which fetches a few rows at a time
/// and takes the connection per call, inside a transaction.
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
