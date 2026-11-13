//! The socket under a connection: plain TCP, or TCP wrapped in TLS (feature `tls`).

use crate::config::{Config, SslMode};
use crate::error::{Error, Result};
use karez_proto_pg::frontend;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

pub(crate) enum Transport {
    Plain(TcpStream),
    /// The socket was handed to another layer (`into_raw_parts`); nothing may use it again.
    #[cfg_attr(not(feature = "tls"), allow(dead_code))]
    Taken,
    #[cfg(feature = "tls")]
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.is_tls() {
            "Transport(tls)"
        } else {
            "Transport(plain)"
        })
    }
}

impl Transport {
    /// Refuse early and clearly when TLS is wanted but not compiled in.
    pub(crate) fn check_supported(cfg: &Config) -> Result<()> {
        if cfg.ssl_mode != SslMode::Disable && !cfg!(feature = "tls") {
            return Err(Error::Tls(
                "TLS support is not compiled in (enable the `tls` feature of karez-pg), and the \
                 default sslmode is verify-full; add sslmode=disable to connect without TLS"
                    .into(),
            ));
        }
        Ok(())
    }

    /// Build the transport the configuration asks for, over an established TCP stream.
    pub(crate) fn establish(mut stream: TcpStream, cfg: &Config) -> Result<Transport> {
        if cfg.ssl_mode == SslMode::Disable {
            return Ok(Transport::Plain(stream));
        }
        let mut req = Vec::with_capacity(8);
        frontend::ssl_request(&mut req);
        stream.write_all(&req)?;
        let mut answer = [0u8; 1];
        stream.read_exact(&mut answer)?;
        match answer[0] {
            b'S' => Self::start_tls(stream, cfg),
            b'N' => Err(Error::Tls(
                "the server does not support TLS; use sslmode=disable only if you accept an \
                 unencrypted connection"
                    .into(),
            )),
            other => Err(Error::Tls(format!(
                "unexpected reply {:?} to the TLS request",
                other as char
            ))),
        }
    }

    #[cfg(feature = "tls")]
    fn start_tls(stream: TcpStream, cfg: &Config) -> Result<Transport> {
        let tls = crate::tls::wrap(stream, cfg)?;
        Ok(Transport::Tls(Box::new(tls)))
    }

    #[cfg(not(feature = "tls"))]
    fn start_tls(_stream: TcpStream, _cfg: &Config) -> Result<Transport> {
        Err(Error::Tls(
            "TLS support is not compiled in: enable the `tls` feature of karez-pg".into(),
        ))
    }

    /// Take the TLS state and the socket out, leaving the transport unusable. `None` (and the
    /// transport untouched) if this is a plain connection.
    #[cfg(feature = "tls")]
    pub(crate) fn take_tls(&mut self) -> Option<(TcpStream, rustls::ClientConnection)> {
        match std::mem::replace(self, Transport::Taken) {
            Transport::Tls(b) => {
                let s = *b;
                Some((s.sock, s.conn))
            }
            other => {
                *self = other;
                None
            }
        }
    }

    fn tcp(&self) -> &TcpStream {
        match self {
            Transport::Plain(s) => s,
            Transport::Taken => unreachable!("the socket was taken"),
            #[cfg(feature = "tls")]
            Transport::Tls(s) => &s.sock,
        }
    }

    pub(crate) fn is_tls(&self) -> bool {
        !matches!(self, Transport::Plain(_) | Transport::Taken)
    }

    pub(crate) fn set_read_timeout(&self, d: Option<Duration>) -> std::io::Result<()> {
        self.tcp().set_read_timeout(d)
    }

    pub(crate) fn read_timeout(&self) -> std::io::Result<Option<Duration>> {
        self.tcp().read_timeout()
    }

    /// A second handle on the plain socket, for a thread that writes while this one reads. Not
    /// possible over TLS, where one object holds the cipher state.
    pub(crate) fn try_clone_plain(&self) -> Result<TcpStream> {
        match self {
            Transport::Plain(s) => Ok(s.try_clone()?),
            Transport::Taken => Err(Error::Usage("the socket was taken")),
            #[cfg(feature = "tls")]
            Transport::Tls(_) => Err(Error::Usage(
                "this needs a second handle on the socket, which a TLS connection cannot give",
            )),
        }
    }

    pub(crate) fn shutdown(&mut self, how: Shutdown) -> std::io::Result<()> {
        match self {
            Transport::Plain(s) => s.shutdown(how),
            Transport::Taken => Ok(()),
            #[cfg(feature = "tls")]
            Transport::Tls(s) => {
                s.conn.send_close_notify();
                let _ = s.flush();
                s.sock.shutdown(how)
            }
        }
    }
}

impl Read for Transport {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Transport::Plain(s) => s.read(buf),
            Transport::Taken => Err(std::io::Error::other("the socket was taken")),
            #[cfg(feature = "tls")]
            Transport::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Transport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Transport::Plain(s) => s.write(buf),
            Transport::Taken => Err(std::io::Error::other("the socket was taken")),
            #[cfg(feature = "tls")]
            Transport::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Transport::Plain(s) => s.flush(),
            Transport::Taken => Ok(()),
            #[cfg(feature = "tls")]
            Transport::Tls(s) => s.flush(),
        }
    }
}
