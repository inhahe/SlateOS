//! Where a frame goes: util-linux 2.39.3's `unix_socket`, `inet_socket` and
//! the send in `write_output` (`misc-utils/logger.c`) -- and, on a system with
//! no Unix-domain sockets at all, the journal record a syslog daemon would
//! have written.
//!
//! # The one adaptation
//!
//! Upstream sends to `/dev/log` and, when that cannot be reached, drops the
//! message unless socket errors are on. On SlateOS `/dev/log` can never be
//! reached: `socket(AF_UNIX, ...)` fails with `EAFNOSUPPORT`, because the
//! platform has no path-bound Unix-domain sockets yet (known-issues
//! TD-B-NOTHING-RECEIVES-SYSLOG-MESSAGES). Dropping every message there would
//! be upstream's behaviour and useless, so when EVERY attempt on `/dev/log`
//! fails with `EAFNOSUPPORT` -- not "no daemon listening", which upstream
//! handles its own way and this still does -- the message becomes a
//! `journalrec` record in the file `journalctl` reads, as the daemon would
//! have filed it. On a host with Unix sockets the branch never runs, and
//! when SlateOS gains them it stops running there too.

use std::ffi::{OsStr, OsString};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::net::{UnixDatagram, UnixStream};

/// `TYPE_UDP`: datagrams (`SOCK_DGRAM`), for both a Unix socket and a server.
pub const TYPE_UDP: u8 = 1 << 1;
/// `TYPE_TCP`: a stream (`SOCK_STREAM`).
pub const TYPE_TCP: u8 = 1 << 2;
/// `ALL_TYPES`: try datagrams first, then a stream.
pub const ALL_TYPES: u8 = TYPE_UDP | TYPE_TCP;

/// `_PATH_DEVLOG`.
pub const PATH_DEVLOG: &str = "/dev/log";

/// `sizeof(((struct sockaddr_un *)0)->sun_path)`.
const SUN_PATH_MAX: usize = 108;

/// `EAFNOSUPPORT`: no socket of this family exists on the platform. The same
/// number on Linux and in the SlateOS libc.
#[cfg(unix)]
const EAFNOSUPPORT: i32 = 97;

/// `SOCK_STREAM` and `SOCK_DGRAM`, for `getaddrinfo`'s hints.
const SOCK_STREAM: i32 = 1;
const SOCK_DGRAM: i32 = 2;

/// An open connection: upstream's `fd`, and what kind of socket it is.
#[derive(Debug)]
pub enum Conn {
    /// `fd == -1`. `write_output` tries again before each message.
    None,
    #[cfg(unix)]
    UnixDgram(UnixDatagram),
    #[cfg(unix)]
    UnixStream(UnixStream),
    Udp(UdpSocket),
    Tcp(TcpStream),
    /// No Unix-domain sockets on this platform: messages become journal
    /// records (module docs).
    Journal,
}

impl Conn {
    /// `is_connected()`.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        !matches!(self, Conn::None)
    }
}

/// An opening failure that ends the program, as upstream words it.
#[derive(Debug)]
pub enum OpenError {
    /// `errx(EXIT_FAILURE, "openlog %s: pathname too long", path)`.
    PathTooLong(OsString),
    /// `err(EXIT_FAILURE, "socket %s", path)`: with the last `errno`.
    Socket(OsString, io::Error),
    /// `errx(EXIT_FAILURE, "failed to resolve name %s port %s: %s", ...)`.
    Resolve(OsString, OsString, String),
    /// `errx(EXIT_FAILURE, "failed to connect to %s port %s", ...)`.
    Connect(OsString, OsString),
}

/// `unix_socket()`: connect to `path` -- as a datagram socket first, then as
/// a stream, as `socket_type` allows -- and narrow `socket_type` to the kind
/// that worked.
///
/// # Errors
///
/// The path is too long for `sun_path` (always), or nothing connected and
/// `errors` is set. With `errors` clear an unreachable socket is
/// [`Conn::None`], as upstream returns -1 and lets `write_output` retry.
pub fn unix_socket(path: &OsStr, socket_type: &mut u8, errors: bool) -> Result<Conn, OpenError> {
    if path.as_encoded_bytes().len() >= SUN_PATH_MAX {
        return Err(OpenError::PathTooLong(path.to_owned()));
    }
    let mut last: Option<io::Error> = None;
    let mut all_unsupported = true;
    for (kind, allowed) in [
        (TYPE_UDP, *socket_type & TYPE_UDP != 0),
        (TYPE_TCP, *socket_type & TYPE_TCP != 0),
    ] {
        if !allowed {
            continue;
        }
        match connect_unix(path, kind) {
            Ok(conn) => {
                *socket_type = kind;
                return Ok(conn);
            }
            Err(e) => {
                all_unsupported &= is_unsupported(&e);
                last = Some(e);
            }
        }
    }
    if last.is_some() && all_unsupported && path == OsStr::new(PATH_DEVLOG) {
        return Ok(Conn::Journal);
    }
    match last {
        Some(e) if errors => Err(OpenError::Socket(path.to_owned(), e)),
        _ => Ok(Conn::None),
    }
}

/// One `socket()` + `connect()` of `kind` to `path`.
#[cfg(unix)]
fn connect_unix(path: &OsStr, kind: u8) -> io::Result<Conn> {
    if kind == TYPE_UDP {
        let s = UnixDatagram::unbound()?;
        s.connect(path)?;
        Ok(Conn::UnixDgram(s))
    } else {
        Ok(Conn::UnixStream(UnixStream::connect(path)?))
    }
}

/// The host the unit tests run on has no Unix-domain sockets, which is the
/// case the journal branch exists for.
#[cfg(not(unix))]
fn connect_unix(_path: &OsStr, _kind: u8) -> io::Result<Conn> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Whether a failure means the platform has no socket of this family.
fn is_unsupported(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(EAFNOSUPPORT)
    }
    #[cfg(not(unix))]
    {
        e.kind() == io::ErrorKind::Unsupported
    }
}

/// `inet_socket()`: resolve `server` and connect -- UDP first, then TCP, as
/// `socket_type` allows -- to `port`, or to the `syslog` (UDP) or
/// `syslog-conn` (TCP) service when none was given.
///
/// Only the FIRST address `getaddrinfo` returns is tried, as upstream tries
/// it.
///
/// # Errors
///
/// A name or service that does not resolve (at once, as upstream's `errx`
/// does), or no connection of either kind.
pub fn inet_socket(
    server: &OsStr,
    port: Option<&OsStr>,
    socket_type: &mut u8,
) -> Result<Conn, OpenError> {
    // `p` is upstream's: the service last tried, which is what a failure to
    // connect names.
    let mut p: OsString = port.map_or_else(OsString::new, OsStr::to_owned);
    for (kind, allowed, default, socktype) in [
        (TYPE_UDP, *socket_type & TYPE_UDP != 0, "syslog", SOCK_DGRAM),
        (
            TYPE_TCP,
            *socket_type & TYPE_TCP != 0,
            "syslog-conn",
            SOCK_STREAM,
        ),
    ] {
        if !allowed {
            continue;
        }
        if port.is_none() {
            p = OsString::from(default);
        }
        let addr = crate::sys::resolve(server.as_encoded_bytes(), p.as_encoded_bytes(), socktype)
            .map_err(|text| OpenError::Resolve(server.to_owned(), p.clone(), text))?;
        if let Ok(conn) = connect_inet(addr, kind) {
            *socket_type = kind;
            return Ok(conn);
        }
    }
    Err(OpenError::Connect(server.to_owned(), p))
}

fn connect_inet(addr: SocketAddr, kind: u8) -> io::Result<Conn> {
    if kind == TYPE_UDP {
        let local: SocketAddr = if addr.is_ipv4() {
            SocketAddr::from(([0, 0, 0, 0], 0))
        } else {
            SocketAddr::from(([0u16; 8], 0))
        };
        let s = UdpSocket::bind(local)?;
        s.connect(addr)?;
        Ok(Conn::Udp(s))
    } else {
        Ok(Conn::Tcp(TcpStream::connect(addr)?))
    }
}

/// What the journal branch records for one message: the parts a syslog
/// daemon takes out of the frame.
#[derive(Clone, Copy, Debug)]
pub struct Parts<'a> {
    pub pri: i32,
    pub tag: &'a [u8],
    /// The frame's PID (`--id`/`-i`), or 0 when it carries none.
    pub pid: i32,
    pub msg: &'a [u8],
}

/// The send in `write_output`: the whole frame as one datagram or one write.
///
/// `claim` is a PID to attach to a local message as the sender's credentials
/// (`SCM_CREDENTIALS`), already decided by the caller as upstream decides it;
/// only a Unix-domain socket can carry one. A stream that takes less than
/// the whole frame in that one `sendmsg` gets the rest by plain writes,
/// where upstream would silently drop it.
///
/// # Errors
///
/// Whatever the socket or the journal file reports; the caller reconnects
/// once and retries, as upstream does.
pub fn send(conn: &mut Conn, wire: &[u8], parts: Parts<'_>, claim: Option<i32>) -> io::Result<()> {
    #[cfg(not(unix))]
    let _ = claim; // No Unix-domain sockets here, so nothing to attach it to.
    match conn {
        Conn::None => Err(io::Error::from(io::ErrorKind::NotConnected)),
        #[cfg(unix)]
        Conn::UnixDgram(s) => match claim {
            Some(pid) => crate::sys::send_as(s.as_raw_fd(), wire, pid).map(drop),
            None => s.send(wire).map(drop),
        },
        #[cfg(unix)]
        Conn::UnixStream(s) => match claim {
            Some(pid) => {
                let sent = crate::sys::send_as(s.as_raw_fd(), wire, pid)?;
                s.write_all(wire.get(sent..).unwrap_or_default())
            }
            None => s.write_all(wire),
        },
        Conn::Udp(s) => s.send(wire).map(drop),
        Conn::Tcp(s) => s.write_all(wire),
        Conn::Journal => append_record(&journal_record(parts, now_secs())?),
    }
}

/// The record a syslog daemon would file for `parts`, stamped `ts`.
///
/// # Errors
///
/// `InvalidData` when the tag or the message is not UTF-8: a record is JSON,
/// JSON is UTF-8, and a message is never silently altered to fit.
pub fn journal_record(parts: Parts<'_>, ts: u64) -> io::Result<journalrec::Record> {
    let text = |b: &[u8]| {
        std::str::from_utf8(b)
            .map(str::to_string)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
    };
    let level = usize::try_from(parts.pri & crate::priority::LOG_PRIMASK)
        .ok()
        .and_then(|i| journalrec::PRIORITY_NAMES.get(i))
        .copied()
        .unwrap_or("notice");
    let facility = crate::priority::facility_name(parts.pri & crate::priority::LOG_FACMASK);
    Ok(journalrec::Record {
        ts,
        level: level.to_string(),
        service: text(parts.tag)?,
        msg: text(parts.msg)?,
        // The frame's PID if it has one; otherwise logger's own, which is
        // what a daemon learns from the socket's credentials.
        pid: u32::try_from(parts.pid)
            .ok()
            .filter(|&p| p != 0)
            .or(Some(std::process::id())),
        extra: facility
            .map(|f| vec![("facility".to_string(), f.to_string())])
            .unwrap_or_default(),
    })
}

/// Append one record, as one write, to the file `journalctl` reads.
///
/// # Errors
///
/// The file cannot be opened or written.
pub fn append_record(record: &journalrec::Record) -> io::Result<()> {
    let mut line = record.to_json_line().into_bytes();
    line.push(b'\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(journalrec::MAIN_LOG_PATH)?
        .write_all(&line)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    #[test]
    fn a_path_too_long_for_sun_path_is_refused_before_anything_is_tried() {
        let long = OsString::from("x".repeat(SUN_PATH_MAX));
        let mut ty = ALL_TYPES;
        assert!(matches!(
            unix_socket(&long, &mut ty, false),
            Err(OpenError::PathTooLong(_))
        ));
        let fits = OsString::from("x".repeat(SUN_PATH_MAX - 1));
        assert!(!matches!(
            unix_socket(&fits, &mut ty, false),
            Err(OpenError::PathTooLong(_))
        ));
    }

    /// On a platform without Unix sockets -- the Windows test host, and
    /// SlateOS today -- `/dev/log` becomes the journal, and any other path is
    /// what upstream makes of an unreachable socket.
    #[cfg(not(unix))]
    #[test]
    fn without_unix_sockets_dev_log_is_the_journal_and_nothing_else_is() {
        let mut ty = ALL_TYPES;
        assert!(matches!(
            unix_socket(OsStr::new(PATH_DEVLOG), &mut ty, true),
            Ok(Conn::Journal)
        ));
        let mut ty = ALL_TYPES;
        assert!(matches!(
            unix_socket(OsStr::new("/tmp/elsewhere"), &mut ty, false),
            Ok(Conn::None)
        ));
        let mut ty = ALL_TYPES;
        assert!(matches!(
            unix_socket(OsStr::new("/tmp/elsewhere"), &mut ty, true),
            Err(OpenError::Socket(..))
        ));
    }

    #[test]
    fn a_journal_record_is_what_a_daemon_would_file() {
        let r = journal_record(
            Parts {
                pri: (3 << 3) | 3,
                tag: b"ntpd",
                pid: 42,
                msg: b"step",
            },
            1_700_000_000,
        )
        .unwrap();
        assert_eq!(
            r.to_json_line(),
            r#"{"ts":1700000000,"level":"err","service":"ntpd","msg":"step","pid":42,"facility":"daemon"}"#
        );
    }

    #[test]
    fn a_record_without_a_frame_pid_is_attributed_to_the_sender() {
        let r = journal_record(
            Parts {
                pri: 13,
                tag: b"t",
                pid: 0,
                msg: b"m",
            },
            1,
        )
        .unwrap();
        assert_eq!(r.pid, Some(std::process::id()));
        let r = journal_record(
            Parts {
                pri: 13,
                tag: b"t",
                pid: -1,
                msg: b"m",
            },
            1,
        )
        .unwrap();
        assert_eq!(r.pid, Some(std::process::id()));
    }

    #[test]
    fn a_message_that_is_not_utf8_is_refused_not_altered() {
        let e = journal_record(
            Parts {
                pri: 13,
                tag: b"t",
                pid: 0,
                msg: b"\xff",
            },
            1,
        )
        .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    }
}
