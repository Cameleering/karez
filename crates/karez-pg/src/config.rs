use crate::error::{Error, Result};
use karez_proto_pg::scram::Secret;
use std::fmt;
use std::time::Duration;

/// Which authentication outcomes the client accepts. The default accepts only SCRAM-SHA-256,
/// so a hostile or misconfigured server cannot downgrade the client to a cleartext password or to
/// no authentication (plan section 1.2, rule 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthPolicy {
    /// Accept SCRAM-SHA-256 (without channel binding).
    pub scram: bool,
    /// Accept a cleartext password request.
    pub cleartext: bool,
    /// Accept a server that authenticates us without any exchange (`trust`).
    pub trust: bool,
}

impl Default for AuthPolicy {
    fn default() -> Self {
        AuthPolicy {
            scram: true,
            cleartext: false,
            trust: false,
        }
    }
}

/// What to do with a result column whose type has no decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnsupportedTypes {
    /// Fail the query with [`Error::UnsupportedType`] naming the type (default).
    #[default]
    Error,
    /// Fetch the column in text format into a `Utf8` column.
    Text,
}

/// How a connection uses TLS (PostgreSQL's `sslmode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslMode {
    /// No TLS. The traffic, including the password exchange, is readable on the network.
    Disable,
    /// Encrypt, but trust any certificate. This stops passive listening only; anyone who can
    /// intercept the connection can impersonate the server.
    Require,
    /// Encrypt and verify that the certificate chains to a trusted CA, but not the host name.
    VerifyCa,
    /// Encrypt and verify the chain and that the certificate is valid for the host (default).
    VerifyFull,
}

/// What a session must be like for [`Config::target_session_attrs`] to accept the host
/// (libpq's `target_session_attrs`). A server is a standby if it reports `in_hot_standby = on`
/// (PostgreSQL 14 and later; older servers are asked with `SELECT pg_is_in_recovery()`). A session
/// is read-only if the server is a standby or `default_transaction_read_only` is `on` (older
/// servers are asked with `SHOW transaction_read_only`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetSessionAttrs {
    /// Any host will do (default).
    #[default]
    Any,
    /// The session accepts writes.
    ReadWrite,
    /// The session does not accept writes.
    ReadOnly,
    /// The host is not a standby.
    Primary,
    /// The host is a standby.
    Standby,
    /// Prefer a standby: try every host for one, then fall back to any host.
    PreferStandby,
}

impl TargetSessionAttrs {
    /// The URL spelling, for example `read-write`.
    pub fn as_str(self) -> &'static str {
        match self {
            TargetSessionAttrs::Any => "any",
            TargetSessionAttrs::ReadWrite => "read-write",
            TargetSessionAttrs::ReadOnly => "read-only",
            TargetSessionAttrs::Primary => "primary",
            TargetSessionAttrs::Standby => "standby",
            TargetSessionAttrs::PreferStandby => "prefer-standby",
        }
    }

    fn parse(v: &str) -> Result<Self> {
        Ok(match v {
            "any" => TargetSessionAttrs::Any,
            "read-write" => TargetSessionAttrs::ReadWrite,
            "read-only" => TargetSessionAttrs::ReadOnly,
            "primary" => TargetSessionAttrs::Primary,
            "standby" => TargetSessionAttrs::Standby,
            "prefer-standby" => TargetSessionAttrs::PreferStandby,
            other => {
                return Err(Error::Config(format!(
                    "unknown target_session_attrs {other:?}; use any, read-write, read-only, primary, standby or prefer-standby"
                )))
            }
        })
    }
}

/// In which order the hosts of a multi-host configuration are tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoadBalance {
    /// In the order written (default).
    #[default]
    Disable,
    /// In a random order, chosen again for every connection.
    Random,
}

/// Connection settings.
#[derive(Clone)]
pub struct Config {
    /// Host name or IP address of the first host (see [`Config::hosts`] for all of them).
    pub host: String,
    /// TCP port of the first host.
    pub port: u16,
    /// More hosts to try after `host:port`, in order, if it cannot be reached or does not
    /// satisfy [`Config::target_session_attrs`]. Empty for a single-host configuration.
    pub fallback_hosts: Vec<(String, u16)>,
    /// What a session must be like to be accepted. Checked on every host before the connection
    /// is returned, from the server's reported parameters where it sends them.
    pub target_session_attrs: TargetSessionAttrs,
    /// The order in which hosts are tried.
    pub load_balance_hosts: LoadBalance,
    /// User name.
    pub user: String,
    /// Database name.
    pub database: String,
    password: Option<Secret>,
    /// `application_name` sent at startup.
    pub application_name: Option<String>,
    /// Timeout for the TCP connect.
    pub connect_timeout: Option<Duration>,
    /// Timeout for each socket read; `None` waits as long as the server does.
    pub read_timeout: Option<Duration>,
    /// Accepted authentication methods.
    pub auth: AuthPolicy,
    /// Handling of columns without a decoder.
    pub unsupported_types: UnsupportedTypes,
    /// TLS policy. The default, [`SslMode::VerifyFull`], encrypts and verifies the certificate
    /// chain and the host name. A plaintext connection needs an explicit `sslmode=disable`.
    pub ssl_mode: SslMode,
    /// Where the trusted CA certificates (PEM) come from: a file path, or `system` for the
    /// built-in Mozilla roots. `None` means the built-in roots.
    pub ssl_root_cert: Option<String>,
    /// Prepared statements kept per connection.
    pub statement_cache_size: usize,
    /// Largest accepted server message in bytes.
    pub max_message: usize,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("fallback_hosts", &self.fallback_hosts)
            .field("target_session_attrs", &self.target_session_attrs)
            .field("load_balance_hosts", &self.load_balance_hosts)
            .field("user", &self.user)
            .field("database", &self.database)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("application_name", &self.application_name)
            .field("connect_timeout", &self.connect_timeout)
            .field("read_timeout", &self.read_timeout)
            .field("auth", &self.auth)
            .field("unsupported_types", &self.unsupported_types)
            .field("ssl_mode", &self.ssl_mode)
            .field("ssl_root_cert", &self.ssl_root_cert)
            .field("statement_cache_size", &self.statement_cache_size)
            .field("max_message", &self.max_message)
            .finish()
    }
}

impl Config {
    /// Settings for `user` on `host:port`, database named after the user, no password.
    pub fn new(host: impl Into<String>, port: u16, user: impl Into<String>) -> Self {
        let user = user.into();
        Config {
            host: host.into(),
            port,
            fallback_hosts: Vec::new(),
            target_session_attrs: TargetSessionAttrs::Any,
            load_balance_hosts: LoadBalance::Disable,
            database: user.clone(),
            user,
            password: None,
            application_name: None,
            connect_timeout: Some(Duration::from_secs(10)),
            read_timeout: None,
            auth: AuthPolicy::default(),
            unsupported_types: UnsupportedTypes::Error,
            ssl_mode: SslMode::VerifyFull,
            ssl_root_cert: None,
            statement_cache_size: 256,
            max_message: karez_proto_pg::DEFAULT_MAX_MESSAGE,
        }
    }

    /// Every host to try, the first one first: `host:port` followed by the fallback hosts.
    pub fn hosts(&self) -> Vec<(String, u16)> {
        let mut v = Vec::with_capacity(1 + self.fallback_hosts.len());
        v.push((self.host.clone(), self.port));
        v.extend(self.fallback_hosts.iter().cloned());
        v
    }

    /// Add a host to try after the ones already configured.
    pub fn add_host(&mut self, host: impl Into<String>, port: u16) -> &mut Self {
        self.fallback_hosts.push((host.into(), port));
        self
    }

    /// Set the password.
    pub fn password(&mut self, password: &str) -> &mut Self {
        self.password = Some(Secret::new(password.as_bytes()));
        self
    }

    pub(crate) fn password_bytes(&self) -> Option<&[u8]> {
        self.password.as_ref().map(Secret::expose)
    }

    /// Parse a connection URL: `karez:pg://`, `postgresql://` or `postgres://`, then
    /// `user[:password]@host[:port][,host[:port]...]/database?key=value&...`. A comma-separated
    /// host list is tried in order (or at random with `load_balance_hosts=random`).
    ///
    /// Recognised keys: `sslmode` (`disable`, `require`, `verify-ca`, `verify-full`; default
    /// `verify-full`), `sslrootcert` (a PEM file or `system`), `application_name`, `connect_timeout` and
    /// `read_timeout` (seconds), `auth` (comma list of `scram`, `password`, `trust`),
    /// `unsupported_types` (`error` or `text`), `target_session_attrs` (`any`, `read-write`,
    /// `read-only`, `primary`, `standby`, `prefer-standby`) and `load_balance_hosts` (`disable` or
    /// `random`). Any other key is an error: nothing in a URL can
    /// load code or open a file.
    pub fn from_url(url: &str) -> Result<Config> {
        let rest = ["karez:pg://", "postgresql://", "postgres://"]
            .iter()
            .find_map(|p| url.strip_prefix(p))
            .ok_or_else(|| {
                Error::Config(
                    "URL must start with karez:pg://, postgresql:// or postgres://".into(),
                )
            })?;
        let (main, query) = match rest.split_once('?') {
            Some((m, q)) => (m, Some(q)),
            None => (rest, None),
        };
        let (authority, path) = match main.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (main, None),
        };
        let (userinfo, hostport) = match authority.rsplit_once('@') {
            Some((u, h)) => (Some(u), h),
            None => (None, authority),
        };
        let (user, password) = match userinfo {
            Some(u) => match u.split_once(':') {
                Some((u, p)) => (u, Some(p)),
                None => (u, None),
            },
            None => ("", None),
        };
        let user = percent_decode(user)?;
        if user.is_empty() {
            return Err(Error::Config("URL has no user name".into()));
        }
        let mut hostlist = Vec::new();
        for h in hostport.split(',') {
            let (host, port) = split_host_port(h)?;
            hostlist.push((
                if host.is_empty() {
                    "localhost".to_string()
                } else {
                    host
                },
                port,
            ));
        }
        let mut hostlist = hostlist.into_iter();
        let (host, port) = hostlist.next().expect("split always yields one item");
        let mut cfg = Config::new(host, port, user);
        cfg.fallback_hosts = hostlist.collect();
        if let Some(p) = password {
            cfg.password = Some(Secret::new(percent_decode(p)?.into_bytes()));
        }
        if let Some(db) = path.filter(|p| !p.is_empty()) {
            cfg.database = percent_decode(db)?;
        }
        if let Some(q) = query {
            for pair in q.split('&').filter(|p| !p.is_empty()) {
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                let (k, v) = (percent_decode(k)?, percent_decode(v)?);
                match k.as_str() {
                    "sslmode" => {
                        cfg.ssl_mode = match v.as_str() {
                            "disable" => SslMode::Disable,
                            "require" => SslMode::Require,
                            "verify-ca" => SslMode::VerifyCa,
                            "verify-full" => SslMode::VerifyFull,
                            "" => return Err(Error::Config("sslmode needs a value".into())),
                            other => {
                                return Err(Error::Config(format!(
                                    "unknown sslmode {other:?}; use disable, require, verify-ca or verify-full"
                                )))
                            }
                        }
                    }
                    "sslrootcert" => {
                        if v.is_empty() {
                            return Err(Error::Config("sslrootcert needs a value".into()));
                        }
                        cfg.ssl_root_cert = Some(v);
                    }
                    "application_name" => cfg.application_name = Some(v),
                    "connect_timeout" => cfg.connect_timeout = Some(secs(&k, &v)?),
                    "read_timeout" => cfg.read_timeout = Some(secs(&k, &v)?),
                    "auth" => {
                        let mut p = AuthPolicy {
                            scram: false,
                            cleartext: false,
                            trust: false,
                        };
                        for m in v.split(',') {
                            match m {
                                "scram" | "scram-sha-256" => p.scram = true,
                                "password" | "cleartext" => p.cleartext = true,
                                "trust" | "none" => p.trust = true,
                                other => {
                                    return Err(Error::Config(format!(
                                    "unknown auth method {other:?}; use scram, password or trust"
                                )))
                                }
                            }
                        }
                        cfg.auth = p;
                    }
                    "target_session_attrs" => {
                        cfg.target_session_attrs = TargetSessionAttrs::parse(&v)?
                    }
                    "load_balance_hosts" => {
                        cfg.load_balance_hosts = match v.as_str() {
                            "disable" => LoadBalance::Disable,
                            "random" => LoadBalance::Random,
                            other => {
                                return Err(Error::Config(format!(
                                    "unknown load_balance_hosts {other:?}; use disable or random"
                                )))
                            }
                        }
                    }
                    "unsupported_types" => {
                        cfg.unsupported_types = match v.as_str() {
                            "error" => UnsupportedTypes::Error,
                            "text" => UnsupportedTypes::Text,
                            _ => {
                                return Err(Error::Config(
                                    "unsupported_types must be error or text".into(),
                                ))
                            }
                        }
                    }
                    other => {
                        return Err(Error::Config(format!("unknown URL parameter {other:?}")));
                    }
                }
            }
        }
        Ok(cfg)
    }
}

fn secs(key: &str, v: &str) -> Result<Duration> {
    match v.parse::<u64>() {
        Ok(n) if n > 0 => Ok(Duration::from_secs(n)),
        _ => Err(Error::Config(format!(
            "{key} must be a positive number of seconds"
        ))),
    }
}

fn split_host_port(s: &str) -> Result<(String, u16)> {
    let (host, port) = if let Some(rest) = s.strip_prefix('[') {
        let (h, after) = rest
            .split_once(']')
            .ok_or_else(|| Error::Config("unterminated [ in host".into()))?;
        let port = match after {
            "" => None,
            a => Some(
                a.strip_prefix(':')
                    .ok_or_else(|| Error::Config("junk after ] in host".into()))?,
            ),
        };
        (h, port)
    } else {
        match s.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (s, None),
        }
    };
    let port = match port {
        None | Some("") => 5432,
        Some(p) => p
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(|| Error::Config(format!("invalid port {p:?}")))?,
    };
    Ok((percent_decode(host)?, port))
}

fn percent_decode(s: &str) -> Result<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = b
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| Error::Config("bad percent-escape in URL".into()))?;
            out.push(hex);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out)
        .map_err(|_| Error::Config("URL is not valid UTF-8 after decoding".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_url() {
        let c = Config::from_url(
            "karez:pg://app:s%3Acret%40x@db.internal:6543/sales%20db?sslmode=disable&application_name=etl&connect_timeout=3&auth=scram,trust&unsupported_types=text",
        )
        .unwrap();
        assert_eq!(c.host, "db.internal");
        assert_eq!(c.port, 6543);
        assert_eq!(c.user, "app");
        assert_eq!(c.password_bytes(), Some(&b"s:cret@x"[..]));
        assert_eq!(c.database, "sales db");
        assert_eq!(c.ssl_mode, SslMode::Disable);
        assert_eq!(c.application_name.as_deref(), Some("etl"));
        assert_eq!(c.connect_timeout, Some(Duration::from_secs(3)));
        assert_eq!(
            c.auth,
            AuthPolicy {
                scram: true,
                cleartext: false,
                trust: true
            }
        );
        assert_eq!(c.unsupported_types, UnsupportedTypes::Text);
    }

    #[test]
    fn defaults_and_ipv6() {
        let c = Config::from_url("postgresql://bob@/").unwrap();
        assert_eq!((c.host.as_str(), c.port), ("localhost", 5432));
        assert_eq!(c.database, "bob");
        assert_eq!(
            c.ssl_mode,
            SslMode::VerifyFull,
            "plaintext must be explicit"
        );
        assert_eq!(c.auth, AuthPolicy::default());
        let c = Config::from_url("postgres://u@[::1]:5433/d").unwrap();
        assert_eq!((c.host.as_str(), c.port), ("::1", 5433));
    }

    #[test]
    fn rejects_bad_input() {
        for bad in [
            "mysql://u@h/d",
            "postgresql://h/d",         // no user
            "postgresql://u@h:0/d",     // port 0
            "postgresql://u@h:99999/d", // port range
            "postgresql://u@h1,h2:0/d", // port 0 on the second host
            "postgresql://u@h1,h2:x/d", // bad port on the second host
            "postgresql://u@h/d?target_session_attrs=sometimes",
            "postgresql://u@h/d?load_balance_hosts=often",
            "postgresql://u@h/d?sslmode=sometimes",
            "postgresql://u@h/d?sslmode=",
            "postgresql://u@h/d?sslrootcert=",
            "postgresql://u@h/d?auth=md5",
            "postgresql://u@h/d?connect_timeout=0",
            "postgresql://u%zz@h/d",
            "postgresql://u@[::1/d",
        ] {
            assert!(Config::from_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_several_hosts_and_session_attrs() {
        let c = Config::from_url(
            "postgresql://u:p@a.example:5433,[::1],b.example:6000/d?target_session_attrs=read-write&load_balance_hosts=random&sslmode=disable",
        )
        .unwrap();
        assert_eq!((c.host.as_str(), c.port), ("a.example", 5433));
        assert_eq!(
            c.fallback_hosts,
            vec![("::1".to_string(), 5432), ("b.example".to_string(), 6000)]
        );
        assert_eq!(c.hosts().len(), 3);
        assert_eq!(c.target_session_attrs, TargetSessionAttrs::ReadWrite);
        assert_eq!(c.load_balance_hosts, LoadBalance::Random);
        // a single host keeps the old shape
        let c = Config::from_url("postgresql://u@h/d").unwrap();
        assert!(c.fallback_hosts.is_empty());
        assert_eq!(c.target_session_attrs, TargetSessionAttrs::Any);
        assert_eq!(c.load_balance_hosts, LoadBalance::Disable);
        // every spelling round-trips
        for a in [
            TargetSessionAttrs::Any,
            TargetSessionAttrs::ReadWrite,
            TargetSessionAttrs::ReadOnly,
            TargetSessionAttrs::Primary,
            TargetSessionAttrs::Standby,
            TargetSessionAttrs::PreferStandby,
        ] {
            assert_eq!(TargetSessionAttrs::parse(a.as_str()).unwrap(), a);
        }
        // the password never appears, however many hosts
        let dbg = format!(
            "{:?}",
            Config::from_url("postgresql://u:hunter2@a,b/d").unwrap()
        );
        assert!(!dbg.contains("hunter2"));
    }

    #[test]
    fn debug_never_shows_the_password() {
        let c = Config::from_url("postgresql://u:hunter2@h/d").unwrap();
        let s = format!("{c:?}");
        assert!(!s.contains("hunter2"));
        assert!(s.contains("redacted"));
    }
}
