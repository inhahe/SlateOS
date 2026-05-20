//! Multi-personality relay utility for OurOS.
//!
//! This binary provides three personalities detected from `argv[0]`:
//!   - `socat`  -- multipurpose bidirectional data relay between two address endpoints
//!   - `filan`  -- file descriptor analysis (list open fds with types)
//!   - `procan` -- process/address analysis (PID, PPID, UID, GID, groups, env)
//!
//! # Address types (socat)
//!
//! `STDIO`, `STDIN`, `STDOUT`, `STDERR`, `PIPE`, `FILE`, `TCP`, `TCP-LISTEN`,
//! `UDP`, `UDP-LISTEN`, `UNIX-CONNECT`, `UNIX-LISTEN`, `EXEC`, `SYSTEM`,
//! `OPEN`, `CREATE`, `READLINE`, `PTY`.

#![deny(clippy::all)]

use std::env;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{
    Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, UdpSocket,
};
use std::process::{self, Command, Stdio};
use std::str::FromStr;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const VERSION: &str = "1.0.0";
const DEFAULT_BUFFER_SIZE: usize = 8192;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// All possible errors in the socat utility.
#[derive(Debug)]
enum SocatError {
    Io(io::Error),
    Parse(String),
    Address(String),
    Option(String),
    Usage(String),
}

impl fmt::Display for SocatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Parse(s) => write!(f, "parse error: {s}"),
            Self::Address(s) => write!(f, "address error: {s}"),
            Self::Option(s) => write!(f, "option error: {s}"),
            Self::Usage(s) => write!(f, "{s}"),
        }
    }
}

impl From<io::Error> for SocatError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

// ---------------------------------------------------------------------------
// Debug level
// ---------------------------------------------------------------------------

/// Verbosity level controlled by `-d` flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DebugLevel(u8);

impl DebugLevel {
    const NONE: Self = Self(0);

    fn enabled(self, level: u8) -> bool {
        self.0 >= level
    }
}

// ---------------------------------------------------------------------------
// IP preference
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IpPreference {
    Any,
    V4,
    V6,
}

// ---------------------------------------------------------------------------
// Global options
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct GlobalOpts {
    debug: DebugLevel,
    verbose: bool,
    hex_dump: bool,
    buffer_size: usize,
    send_buffer_size: Option<usize>,
    total_timeout: Option<Duration>,
    transfer_timeout: Option<Duration>,
    unidirectional: bool,
    reverse_unidirectional: bool,
    ip_pref: IpPreference,
}

impl Default for GlobalOpts {
    fn default() -> Self {
        Self {
            debug: DebugLevel::NONE,
            verbose: false,
            hex_dump: false,
            buffer_size: DEFAULT_BUFFER_SIZE,
            send_buffer_size: None,
            total_timeout: None,
            transfer_timeout: None,
            unidirectional: false,
            reverse_unidirectional: false,
            ip_pref: IpPreference::Any,
        }
    }
}

// ---------------------------------------------------------------------------
// Address options (per-endpoint, comma-separated after the address)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct AddressOptions {
    fork: bool,
    reuseaddr: bool,
    bind_addr: Option<String>,
    connect_timeout: Option<Duration>,
    retry: Option<u32>,
    interval: Option<Duration>,
    crlf: bool,
    ignoreeof: bool,
    nonblock: bool,
    append: bool,
    create: bool,
    excl: bool,
    /// Unrecognised options stored for diagnostics.
    unknown: Vec<String>,
}

fn parse_address_options(raw: &[&str]) -> Result<AddressOptions, SocatError> {
    let mut opts = AddressOptions::default();
    for &item in raw {
        if item.is_empty() {
            continue;
        }
        if item == "fork" {
            opts.fork = true;
        } else if item == "reuseaddr" {
            opts.reuseaddr = true;
        } else if item == "crlf" {
            opts.crlf = true;
        } else if item == "ignoreeof" {
            opts.ignoreeof = true;
        } else if item == "nonblock" {
            opts.nonblock = true;
        } else if item == "append" {
            opts.append = true;
        } else if item == "create" {
            opts.create = true;
        } else if item == "excl" {
            opts.excl = true;
        } else if let Some(val) = item.strip_prefix("bind=") {
            opts.bind_addr = Some(val.to_string());
        } else if let Some(val) = item.strip_prefix("connect-timeout=") {
            let secs: f64 = val.parse().map_err(|_| {
                SocatError::Option(format!("invalid connect-timeout: {val}"))
            })?;
            opts.connect_timeout = Some(Duration::from_secs_f64(secs));
        } else if let Some(val) = item.strip_prefix("retry=") {
            let n: u32 = val.parse().map_err(|_| {
                SocatError::Option(format!("invalid retry: {val}"))
            })?;
            opts.retry = Some(n);
        } else if let Some(val) = item.strip_prefix("interval=") {
            let secs: f64 = val.parse().map_err(|_| {
                SocatError::Option(format!("invalid interval: {val}"))
            })?;
            opts.interval = Some(Duration::from_secs_f64(secs));
        } else {
            opts.unknown.push(item.to_string());
        }
    }
    Ok(opts)
}

// ---------------------------------------------------------------------------
// Address specification (parsed from the user string)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum AddressSpec {
    /// `STDIO` or `-` -- stdin+stdout combined.
    Stdio,
    /// `STDIN`
    Stdin,
    /// `STDOUT`
    Stdout,
    /// `STDERR`
    Stderr,
    /// `PIPE[:<filename>]`
    Pipe { path: Option<String>, opts: AddressOptions },
    /// `FILE:<filename>[,options]`
    File { path: String, opts: AddressOptions },
    /// `TCP:<host>:<port>[,options]`
    TcpConnect { host: String, port: u16, opts: AddressOptions },
    /// `TCP-LISTEN:<port>[,options]`
    TcpListen { port: u16, opts: AddressOptions },
    /// `UDP:<host>:<port>[,options]`
    UdpClient { host: String, port: u16, opts: AddressOptions },
    /// `UDP-LISTEN:<port>[,options]`
    UdpListen { port: u16, opts: AddressOptions },
    /// `UNIX-CONNECT:<path>`
    UnixConnect { path: String, opts: AddressOptions },
    /// `UNIX-LISTEN:<path>[,options]`
    UnixListen { path: String, opts: AddressOptions },
    /// `EXEC:<command>[,options]`
    Exec { command: String, opts: AddressOptions },
    /// `SYSTEM:<command>`
    System { command: String, opts: AddressOptions },
    /// `OPEN:<filename>[,options]`
    Open { path: String, opts: AddressOptions },
    /// `CREATE:<filename>[,options]`
    Create { path: String, opts: AddressOptions },
    /// `READLINE`
    Readline,
    /// `PTY`
    Pty { opts: AddressOptions },
}

/// Split a raw address string on commas that are NOT inside balanced
/// parentheses or quotes.  This lets addresses like
/// `EXEC:"cmd --flag=a,b",fork` work correctly.
fn split_address_options(s: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = Vec::new();
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut depth_paren: i32 = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'(' if !in_single && !in_double => depth_paren += 1,
            b')' if !in_single && !in_double => {
                depth_paren = depth_paren.saturating_sub(1);
            }
            b',' if !in_single && !in_double && depth_paren == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start <= s.len() {
        parts.push(&s[start..]);
    }
    parts
}

/// Strip balanced quotes from a string value.
fn strip_quotes(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Parse a host:port pair where the host may be an IPv6 address in brackets.
fn parse_host_port(s: &str) -> Result<(String, u16), SocatError> {
    // IPv6: [::1]:8080
    if let Some(rest) = s.strip_prefix('[') {
        if let Some(bracket_end) = rest.find(']') {
            let host = &rest[..bracket_end];
            let after = &rest[bracket_end + 1..];
            let port_str = after.strip_prefix(':').ok_or_else(|| {
                SocatError::Parse(format!("expected ':' after ']' in {s}"))
            })?;
            let port: u16 = port_str.parse().map_err(|_| {
                SocatError::Parse(format!("invalid port: {port_str}"))
            })?;
            return Ok((host.to_string(), port));
        }
        return Err(SocatError::Parse(format!("unmatched '[' in {s}")));
    }
    // host:port -- find last colon (to support IPv6 without brackets, though
    // not standard).
    if let Some(colon) = s.rfind(':') {
        let host = &s[..colon];
        let port_str = &s[colon + 1..];
        let port: u16 = port_str.parse().map_err(|_| {
            SocatError::Parse(format!("invalid port: {port_str}"))
        })?;
        Ok((host.to_string(), port))
    } else {
        Err(SocatError::Parse(format!(
            "expected host:port but got: {s}"
        )))
    }
}

fn parse_port(s: &str) -> Result<u16, SocatError> {
    s.parse::<u16>()
        .map_err(|_| SocatError::Parse(format!("invalid port: {s}")))
}

fn parse_address(raw: &str) -> Result<AddressSpec, SocatError> {
    let upper = raw.to_ascii_uppercase();

    // Simplest aliases first.
    if upper == "STDIO" || raw == "-" {
        return Ok(AddressSpec::Stdio);
    }
    if upper == "STDIN" {
        return Ok(AddressSpec::Stdin);
    }
    if upper == "STDOUT" {
        return Ok(AddressSpec::Stdout);
    }
    if upper == "STDERR" {
        return Ok(AddressSpec::Stderr);
    }
    if upper == "READLINE" {
        return Ok(AddressSpec::Readline);
    }

    // Split on commas (respecting quotes / parens).
    let parts = split_address_options(raw);
    let primary = parts.first().copied().unwrap_or("");
    let opt_slices: Vec<&str> = parts.iter().skip(1).copied().collect();

    let primary_upper = primary.to_ascii_uppercase();

    // PTY
    if primary_upper == "PTY" {
        return Ok(AddressSpec::Pty {
            opts: parse_address_options(&opt_slices)?,
        });
    }

    // PIPE[:<path>]
    if primary_upper == "PIPE" || primary_upper.starts_with("PIPE:") {
        let path = primary_upper
            .strip_prefix("PIPE:")
            .and_then(|_| {
                // Use the original-case text, not upper.
                let offset = "PIPE:".len();
                primary.get(offset..)
            })
            .or_else(|| {
                if primary.len() > "PIPE:".len() {
                    primary.get("PIPE:".len()..)
                } else {
                    None
                }
            })
            .map(|s| strip_quotes(s).to_string());
        return Ok(AddressSpec::Pipe {
            path,
            opts: parse_address_options(&opt_slices)?,
        });
    }

    // FILE:<path>[,opts]
    if let Some(rest) = strip_prefix_ci(primary, "FILE:") {
        let path = strip_quotes(rest).to_string();
        return Ok(AddressSpec::File {
            path,
            opts: parse_address_options(&opt_slices)?,
        });
    }

    // OPEN:<path>[,opts]
    if let Some(rest) = strip_prefix_ci(primary, "OPEN:") {
        let path = strip_quotes(rest).to_string();
        return Ok(AddressSpec::Open {
            path,
            opts: parse_address_options(&opt_slices)?,
        });
    }

    // CREATE:<path>[,opts]
    if let Some(rest) = strip_prefix_ci(primary, "CREATE:") {
        let path = strip_quotes(rest).to_string();
        return Ok(AddressSpec::Create {
            path,
            opts: parse_address_options(&opt_slices)?,
        });
    }

    // TCP-LISTEN:<port>[,opts]
    if let Some(rest) = strip_prefix_ci(primary, "TCP-LISTEN:") {
        let port = parse_port(rest)?;
        return Ok(AddressSpec::TcpListen {
            port,
            opts: parse_address_options(&opt_slices)?,
        });
    }

    // TCP:<host>:<port>[,opts]
    if let Some(rest) = strip_prefix_ci(primary, "TCP:") {
        let (host, port) = parse_host_port(rest)?;
        return Ok(AddressSpec::TcpConnect {
            host,
            port,
            opts: parse_address_options(&opt_slices)?,
        });
    }

    // UDP-LISTEN:<port>[,opts]
    if let Some(rest) = strip_prefix_ci(primary, "UDP-LISTEN:") {
        let port = parse_port(rest)?;
        return Ok(AddressSpec::UdpListen {
            port,
            opts: parse_address_options(&opt_slices)?,
        });
    }

    // UDP:<host>:<port>[,opts]
    if let Some(rest) = strip_prefix_ci(primary, "UDP:") {
        let (host, port) = parse_host_port(rest)?;
        return Ok(AddressSpec::UdpClient {
            host,
            port,
            opts: parse_address_options(&opt_slices)?,
        });
    }

    // UNIX-CONNECT:<path>
    if let Some(rest) = strip_prefix_ci(primary, "UNIX-CONNECT:") {
        let path = strip_quotes(rest).to_string();
        return Ok(AddressSpec::UnixConnect {
            path,
            opts: parse_address_options(&opt_slices)?,
        });
    }

    // UNIX-LISTEN:<path>[,opts]
    if let Some(rest) = strip_prefix_ci(primary, "UNIX-LISTEN:") {
        let path = strip_quotes(rest).to_string();
        return Ok(AddressSpec::UnixListen {
            path,
            opts: parse_address_options(&opt_slices)?,
        });
    }

    // EXEC:<command>[,opts]
    if let Some(rest) = strip_prefix_ci(primary, "EXEC:") {
        let command = strip_quotes(rest).to_string();
        return Ok(AddressSpec::Exec {
            command,
            opts: parse_address_options(&opt_slices)?,
        });
    }

    // SYSTEM:<command>[,opts]
    if let Some(rest) = strip_prefix_ci(primary, "SYSTEM:") {
        let command = strip_quotes(rest).to_string();
        return Ok(AddressSpec::System {
            command,
            opts: parse_address_options(&opt_slices)?,
        });
    }

    Err(SocatError::Address(format!("unknown address: {raw}")))
}

/// Case-insensitive prefix strip: if `s` starts with `prefix` (ignoring case)
/// return the remainder using the *original* casing of `s`.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len()
        && s[..prefix.len()].eq_ignore_ascii_case(prefix)
    {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Hex dump helper
// ---------------------------------------------------------------------------

fn hex_dump(data: &[u8], out: &mut dyn Write) -> io::Result<()> {
    let mut offset = 0usize;
    while offset < data.len() {
        write!(out, "{offset:08x}  ")?;
        let end = std::cmp::min(offset + 16, data.len());
        for i in offset..offset + 16 {
            if i < end {
                write!(out, "{:02x} ", data[i])?;
            } else {
                write!(out, "   ")?;
            }
            if i == offset + 7 {
                write!(out, " ")?;
            }
        }
        write!(out, " |")?;
        for &b in &data[offset..end] {
            let c = if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            };
            write!(out, "{c}")?;
        }
        writeln!(out, "|")?;
        offset += 16;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Opened address -- a read/write handle pair
// ---------------------------------------------------------------------------

/// A pair of byte streams (read side, write side) produced by opening an address.
struct OpenedAddress {
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
}

impl OpenedAddress {
    fn stdio() -> Self {
        Self {
            reader: Box::new(io::stdin()),
            writer: Box::new(io::stdout()),
        }
    }

    fn stdin_only() -> Self {
        Self {
            reader: Box::new(io::stdin()),
            writer: Box::new(io::sink()),
        }
    }

    fn stdout_only() -> Self {
        Self {
            reader: Box::new(io::empty()),
            writer: Box::new(io::stdout()),
        }
    }

    fn stderr_only() -> Self {
        Self {
            reader: Box::new(io::empty()),
            writer: Box::new(io::stderr()),
        }
    }
}

fn open_file_address(
    path: &str,
    opts: &AddressOptions,
) -> Result<OpenedAddress, SocatError> {
    use std::fs::OpenOptions;
    let mut oo = OpenOptions::new();
    oo.read(true).write(true);
    if opts.append {
        oo.append(true);
    }
    if opts.create || opts.excl {
        oo.create(true);
    }
    if opts.excl {
        oo.create_new(true);
    }
    let f = oo.open(path)?;
    let f2 = f.try_clone()?;
    Ok(OpenedAddress {
        reader: Box::new(f),
        writer: Box::new(f2),
    })
}

fn open_create_address(
    path: &str,
    _opts: &AddressOptions,
) -> Result<OpenedAddress, SocatError> {
    use std::fs::OpenOptions;
    let f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    Ok(OpenedAddress {
        reader: Box::new(io::empty()),
        writer: Box::new(f),
    })
}

fn resolve_addr(host: &str, port: u16, pref: IpPreference) -> Result<SocketAddr, SocatError> {
    // Try parsing as a direct IP first.
    if let Ok(v4) = Ipv4Addr::from_str(host) {
        return Ok(SocketAddr::from((v4, port)));
    }
    if let Ok(v6) = Ipv6Addr::from_str(host) {
        return Ok(SocketAddr::from((v6, port)));
    }

    // DNS resolution.
    use std::net::ToSocketAddrs;
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| SocatError::Address(format!("resolve {host}: {e}")))?
        .collect();

    if addrs.is_empty() {
        return Err(SocatError::Address(format!(
            "no addresses found for {host}"
        )));
    }

    match pref {
        IpPreference::V4 => addrs
            .iter()
            .find(|a| a.is_ipv4())
            .copied()
            .ok_or_else(|| {
                SocatError::Address(format!("no IPv4 address for {host}"))
            }),
        IpPreference::V6 => addrs
            .iter()
            .find(|a| a.is_ipv6())
            .copied()
            .ok_or_else(|| {
                SocatError::Address(format!("no IPv6 address for {host}"))
            }),
        IpPreference::Any => Ok(addrs[0]),
    }
}

fn open_tcp_connect(
    host: &str,
    port: u16,
    opts: &AddressOptions,
    gopts: &GlobalOpts,
) -> Result<OpenedAddress, SocatError> {
    let addr = resolve_addr(host, port, gopts.ip_pref)?;

    let retries = opts.retry.unwrap_or(0);
    let interval = opts.interval.unwrap_or(Duration::from_secs(1));

    let mut last_err = None;
    for attempt in 0..=retries {
        if attempt > 0 {
            std::thread::sleep(interval);
        }
        let result = if let Some(timeout) = opts.connect_timeout {
            TcpStream::connect_timeout(&addr, timeout)
        } else {
            TcpStream::connect(addr)
        };
        match result {
            Ok(stream) => {
                if opts.nonblock {
                    let _ = stream.set_nonblocking(true);
                }
                let s2 = stream.try_clone()?;
                return Ok(OpenedAddress {
                    reader: Box::new(stream),
                    writer: Box::new(s2),
                });
            }
            Err(e) => {
                if gopts.debug.enabled(2) {
                    eprintln!("socat: TCP connect attempt {attempt} failed: {e}");
                }
                last_err = Some(e);
            }
        }
    }
    Err(SocatError::Io(
        last_err.unwrap_or_else(|| io::Error::new(io::ErrorKind::Other, "connect failed")),
    ))
}

fn open_tcp_listen(
    port: u16,
    opts: &AddressOptions,
    gopts: &GlobalOpts,
) -> Result<OpenedAddress, SocatError> {
    let bind_addr = opts
        .bind_addr
        .as_deref()
        .unwrap_or(match gopts.ip_pref {
            IpPreference::V6 => "::0",
            _ => "0.0.0.0",
        });

    let listener = TcpListener::bind(format!("{bind_addr}:{port}"))?;

    if gopts.debug.enabled(1) {
        eprintln!("socat: listening on TCP port {port}");
    }

    let (stream, peer) = listener.accept()?;
    if gopts.debug.enabled(1) {
        eprintln!("socat: accepted connection from {peer}");
    }

    if opts.nonblock {
        let _ = stream.set_nonblocking(true);
    }

    let s2 = stream.try_clone()?;
    Ok(OpenedAddress {
        reader: Box::new(stream),
        writer: Box::new(s2),
    })
}

fn open_udp_client(
    host: &str,
    port: u16,
    opts: &AddressOptions,
    gopts: &GlobalOpts,
) -> Result<OpenedAddress, SocatError> {
    let dest = resolve_addr(host, port, gopts.ip_pref)?;
    let local = opts.bind_addr.as_deref().unwrap_or(match gopts.ip_pref {
        IpPreference::V6 => "[::]:0",
        _ => "0.0.0.0:0",
    });
    let socket = UdpSocket::bind(local)?;
    socket.connect(dest)?;
    if opts.nonblock {
        let _ = socket.set_nonblocking(true);
    }
    let s2 = socket.try_clone()?;
    Ok(OpenedAddress {
        reader: Box::new(UdpReader(s2)),
        writer: Box::new(UdpWriter(socket)),
    })
}

fn open_udp_listen(
    port: u16,
    opts: &AddressOptions,
    gopts: &GlobalOpts,
) -> Result<OpenedAddress, SocatError> {
    let bind_addr = opts
        .bind_addr
        .as_deref()
        .unwrap_or(match gopts.ip_pref {
            IpPreference::V6 => "::0",
            _ => "0.0.0.0",
        });
    let socket = UdpSocket::bind(format!("{bind_addr}:{port}"))?;

    if gopts.debug.enabled(1) {
        eprintln!("socat: listening on UDP port {port}");
    }

    // Receive the first datagram, then "connect" to the sender so subsequent
    // reads/writes are scoped to that peer.
    let mut probe = [0u8; 1];
    let (_n, peer) = socket.peek_from(&mut probe).map_err(SocatError::Io)?;
    socket.connect(peer)?;
    if gopts.debug.enabled(1) {
        eprintln!("socat: UDP peer: {peer}");
    }
    if opts.nonblock {
        let _ = socket.set_nonblocking(true);
    }
    let s2 = socket.try_clone()?;
    Ok(OpenedAddress {
        reader: Box::new(UdpReader(s2)),
        writer: Box::new(UdpWriter(socket)),
    })
}

/// Thin wrapper that implements `Read` on a connected `UdpSocket`.
struct UdpReader(UdpSocket);

impl Read for UdpReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.recv(buf)
    }
}

/// Thin wrapper that implements `Write` on a connected `UdpSocket`.
struct UdpWriter(UdpSocket);

impl Write for UdpWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.send(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(unix)]
fn open_unix_connect(
    path: &str,
    opts: &AddressOptions,
) -> Result<OpenedAddress, SocatError> {
    let stream = UnixStream::connect(path)?;
    if opts.nonblock {
        let _ = stream.set_nonblocking(true);
    }
    let s2 = stream.try_clone()?;
    Ok(OpenedAddress {
        reader: Box::new(stream),
        writer: Box::new(s2),
    })
}

#[cfg(not(unix))]
fn open_unix_connect(
    _path: &str,
    _opts: &AddressOptions,
) -> Result<OpenedAddress, SocatError> {
    Err(SocatError::Address(
        "UNIX-CONNECT not supported on this platform".to_string(),
    ))
}

#[cfg(unix)]
fn open_unix_listen(
    path: &str,
    opts: &AddressOptions,
    gopts: &GlobalOpts,
) -> Result<OpenedAddress, SocatError> {
    // Remove stale socket file if it exists.
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;

    if gopts.debug.enabled(1) {
        eprintln!("socat: listening on UNIX socket {path}");
    }

    let (stream, _peer) = listener.accept()?;
    if opts.nonblock {
        let _ = stream.set_nonblocking(true);
    }
    let s2 = stream.try_clone()?;
    Ok(OpenedAddress {
        reader: Box::new(stream),
        writer: Box::new(s2),
    })
}

#[cfg(not(unix))]
fn open_unix_listen(
    _path: &str,
    _opts: &AddressOptions,
    _gopts: &GlobalOpts,
) -> Result<OpenedAddress, SocatError> {
    Err(SocatError::Address(
        "UNIX-LISTEN not supported on this platform".to_string(),
    ))
}

fn open_exec(
    command: &str,
    _opts: &AddressOptions,
) -> Result<OpenedAddress, SocatError> {
    // Split command on whitespace (simplistic, no shell quoting).
    let mut parts = command.split_whitespace();
    let prog = parts.next().ok_or_else(|| {
        SocatError::Address("empty EXEC command".to_string())
    })?;
    let args: Vec<&str> = parts.collect();

    let mut child = Command::new(prog)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| SocatError::Io(io::Error::new(io::ErrorKind::Other, "no child stdin")))?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| SocatError::Io(io::Error::new(io::ErrorKind::Other, "no child stdout")))?;

    Ok(OpenedAddress {
        reader: Box::new(child_stdout),
        writer: Box::new(child_stdin),
    })
}

fn open_system(
    command: &str,
    _opts: &AddressOptions,
) -> Result<OpenedAddress, SocatError> {
    let shell = if cfg!(windows) { "cmd" } else { "sh" };
    let flag = if cfg!(windows) { "/C" } else { "-c" };

    let mut child = Command::new(shell)
        .arg(flag)
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| SocatError::Io(io::Error::new(io::ErrorKind::Other, "no child stdin")))?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| SocatError::Io(io::Error::new(io::ErrorKind::Other, "no child stdout")))?;

    Ok(OpenedAddress {
        reader: Box::new(child_stdout),
        writer: Box::new(child_stdin),
    })
}

fn open_pipe_address(
    path: &Option<String>,
    _opts: &AddressOptions,
) -> Result<OpenedAddress, SocatError> {
    // If a path is given, use it as a file; otherwise fall back to stdio.
    match path {
        Some(p) => {
            use std::fs::OpenOptions;
            let f = OpenOptions::new().read(true).write(true).open(p)?;
            let f2 = f.try_clone()?;
            Ok(OpenedAddress {
                reader: Box::new(f),
                writer: Box::new(f2),
            })
        }
        None => Ok(OpenedAddress::stdio()),
    }
}

fn open_readline() -> Result<OpenedAddress, SocatError> {
    // Readline personality: read lines from stdin, echo a prompt.
    // In a real implementation this would use termios / line-editing.
    // Here we provide a basic line-buffered reader.
    Ok(OpenedAddress {
        reader: Box::new(io::stdin()),
        writer: Box::new(io::stdout()),
    })
}

fn open_pty(_opts: &AddressOptions) -> Result<OpenedAddress, SocatError> {
    // PTY creation requires OS-specific APIs.  On OurOS this will use the
    // native PTY subsystem; for now, fall back to a pipe pair placeholder.
    Err(SocatError::Address(
        "PTY address not yet implemented on this platform".to_string(),
    ))
}

fn open_address(
    spec: &AddressSpec,
    gopts: &GlobalOpts,
) -> Result<OpenedAddress, SocatError> {
    match spec {
        AddressSpec::Stdio => Ok(OpenedAddress::stdio()),
        AddressSpec::Stdin => Ok(OpenedAddress::stdin_only()),
        AddressSpec::Stdout => Ok(OpenedAddress::stdout_only()),
        AddressSpec::Stderr => Ok(OpenedAddress::stderr_only()),
        AddressSpec::Pipe { path, opts } => open_pipe_address(path, opts),
        AddressSpec::File { path, opts } => open_file_address(path, opts),
        AddressSpec::Open { path, opts } => open_file_address(path, opts),
        AddressSpec::Create { path, opts } => open_create_address(path, opts),
        AddressSpec::TcpConnect { host, port, opts } => {
            open_tcp_connect(host, *port, opts, gopts)
        }
        AddressSpec::TcpListen { port, opts } => {
            open_tcp_listen(*port, opts, gopts)
        }
        AddressSpec::UdpClient { host, port, opts } => {
            open_udp_client(host, *port, opts, gopts)
        }
        AddressSpec::UdpListen { port, opts } => {
            open_udp_listen(*port, opts, gopts)
        }
        AddressSpec::UnixConnect { path, opts } => {
            open_unix_connect(path, opts)
        }
        AddressSpec::UnixListen { path, opts } => {
            open_unix_listen(path, opts, gopts)
        }
        AddressSpec::Exec { command, opts } => open_exec(command, opts),
        AddressSpec::System { command, opts } => open_system(command, opts),
        AddressSpec::Readline => open_readline(),
        AddressSpec::Pty { opts } => open_pty(opts),
    }
}

// ---------------------------------------------------------------------------
// Data relay engine
// ---------------------------------------------------------------------------

/// Transfer data from `reader` to `writer` until EOF or error.  Applies CRLF
/// conversion if requested.  Returns the total number of bytes written.
fn transfer(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    buf_size: usize,
    crlf: bool,
    verbose: bool,
    hex: bool,
    ignoreeof: bool,
) -> Result<u64, SocatError> {
    let mut buf = vec![0u8; buf_size];
    let mut total: u64 = 0;
    let mut stderr = io::stderr();

    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => {
                if ignoreeof {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                break;
            }
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(SocatError::Io(e)),
        };

        let data = &buf[..n];

        if verbose {
            let dir = "> ";
            let _ = write!(stderr, "{dir}");
            let _ = stderr.write_all(data);
            if !data.ends_with(b"\n") {
                let _ = writeln!(stderr);
            }
        }

        if hex {
            let _ = hex_dump(data, &mut stderr);
        }

        if crlf {
            // Convert bare LF to CRLF.
            for &b in data {
                if b == b'\n' {
                    writer.write_all(b"\r\n")?;
                    total += 2;
                } else {
                    writer.write_all(&[b])?;
                    total += 1;
                }
            }
        } else {
            writer.write_all(data)?;
            total += n as u64;
        }

        writer.flush()?;
    }

    Ok(total)
}

/// The bidirectional relay: spawn two threads, one for each direction, and wait
/// for either side to finish (EOF / error).
fn relay(
    mut left: OpenedAddress,
    mut right: OpenedAddress,
    gopts: &GlobalOpts,
    left_opts: &AddressOptions,
    right_opts: &AddressOptions,
) -> Result<(), SocatError> {
    let buf = gopts.buffer_size;
    let verbose = gopts.verbose;
    let hex = gopts.hex_dump;
    let left_crlf = left_opts.crlf;
    let right_crlf = right_opts.crlf;
    let left_ignoreeof = left_opts.ignoreeof;
    let right_ignoreeof = right_opts.ignoreeof;

    if gopts.unidirectional {
        // left -> right only.
        transfer(
            &mut *left.reader,
            &mut *right.writer,
            buf,
            right_crlf,
            verbose,
            hex,
            left_ignoreeof,
        )?;
        return Ok(());
    }

    if gopts.reverse_unidirectional {
        // right -> left only.
        transfer(
            &mut *right.reader,
            &mut *left.writer,
            buf,
            left_crlf,
            verbose,
            hex,
            right_ignoreeof,
        )?;
        return Ok(());
    }

    // Bidirectional: two threads.
    let buf2 = buf;
    let handle_l2r = std::thread::spawn(move || -> Result<u64, SocatError> {
        transfer(
            &mut *left.reader,
            &mut *right.writer,
            buf,
            right_crlf,
            verbose,
            hex,
            left_ignoreeof,
        )
    });

    let handle_r2l = std::thread::spawn(move || -> Result<u64, SocatError> {
        transfer(
            &mut *right.reader,
            &mut *left.writer,
            buf2,
            left_crlf,
            verbose,
            hex,
            right_ignoreeof,
        )
    });

    // Wait for both.  First one to end terminates the relay.
    let r1 = handle_l2r.join().unwrap_or(Err(SocatError::Io(
        io::Error::new(io::ErrorKind::Other, "thread panic"),
    )));
    let r2 = handle_r2l.join().unwrap_or(Err(SocatError::Io(
        io::Error::new(io::ErrorKind::Other, "thread panic"),
    )));

    // Propagate the first real error, ignoring benign I/O from the other side
    // shutting down.
    if let Err(e) = r1 {
        match &e {
            SocatError::Io(ioe)
                if ioe.kind() == io::ErrorKind::BrokenPipe
                    || ioe.kind() == io::ErrorKind::ConnectionReset => {}
            _ => return Err(e),
        }
    }
    if let Err(e) = r2 {
        match &e {
            SocatError::Io(ioe)
                if ioe.kind() == io::ErrorKind::BrokenPipe
                    || ioe.kind() == io::ErrorKind::ConnectionReset => {}
            _ => return Err(e),
        }
    }

    Ok(())
}

fn get_address_opts(spec: &AddressSpec) -> AddressOptions {
    match spec {
        AddressSpec::Stdio
        | AddressSpec::Stdin
        | AddressSpec::Stdout
        | AddressSpec::Stderr
        | AddressSpec::Readline => AddressOptions::default(),
        AddressSpec::Pipe { opts, .. }
        | AddressSpec::File { opts, .. }
        | AddressSpec::TcpConnect { opts, .. }
        | AddressSpec::TcpListen { opts, .. }
        | AddressSpec::UdpClient { opts, .. }
        | AddressSpec::UdpListen { opts, .. }
        | AddressSpec::UnixConnect { opts, .. }
        | AddressSpec::UnixListen { opts, .. }
        | AddressSpec::Exec { opts, .. }
        | AddressSpec::System { opts, .. }
        | AddressSpec::Open { opts, .. }
        | AddressSpec::Create { opts, .. }
        | AddressSpec::Pty { opts, .. } => opts.clone(),
    }
}

// ---------------------------------------------------------------------------
// CLI argument parser
// ---------------------------------------------------------------------------

fn parse_global_opts(args: &[String]) -> Result<(GlobalOpts, Vec<String>), SocatError> {
    let mut gopts = GlobalOpts::default();
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let a = &args[i];
        if a == "--version" || a == "-V" {
            println!("socat version {VERSION}");
            process::exit(0);
        } else if a == "--help" || a == "-h" {
            print_help();
            process::exit(0);
        } else if a == "-u" {
            gopts.unidirectional = true;
        } else if a == "-U" {
            gopts.reverse_unidirectional = true;
        } else if a == "-v" {
            gopts.verbose = true;
        } else if a == "-x" {
            gopts.hex_dump = true;
        } else if a == "-4" {
            gopts.ip_pref = IpPreference::V4;
        } else if a == "-6" {
            gopts.ip_pref = IpPreference::V6;
        } else if a == "-b" {
            i += 1;
            let val = args.get(i).ok_or_else(|| {
                SocatError::Usage("-b requires an argument".to_string())
            })?;
            gopts.buffer_size = val.parse().map_err(|_| {
                SocatError::Usage(format!("invalid buffer size: {val}"))
            })?;
        } else if a == "-s" {
            i += 1;
            let val = args.get(i).ok_or_else(|| {
                SocatError::Usage("-s requires an argument".to_string())
            })?;
            gopts.send_buffer_size = Some(val.parse().map_err(|_| {
                SocatError::Usage(format!("invalid send buffer size: {val}"))
            })?);
        } else if a == "-t" {
            i += 1;
            let val = args.get(i).ok_or_else(|| {
                SocatError::Usage("-t requires an argument".to_string())
            })?;
            let secs: f64 = val.parse().map_err(|_| {
                SocatError::Usage(format!("invalid timeout: {val}"))
            })?;
            gopts.total_timeout = Some(Duration::from_secs_f64(secs));
        } else if a == "-T" {
            i += 1;
            let val = args.get(i).ok_or_else(|| {
                SocatError::Usage("-T requires an argument".to_string())
            })?;
            let secs: f64 = val.parse().map_err(|_| {
                SocatError::Usage(format!("invalid transfer timeout: {val}"))
            })?;
            gopts.transfer_timeout = Some(Duration::from_secs_f64(secs));
        } else if a.starts_with("-d") && a.len() > 1 && a.bytes().skip(1).all(|b| b == b'd') {
            // -d, -dd, -ddd, -dddd
            gopts.debug = DebugLevel((a.len() - 1) as u8);
        } else if a.starts_with('-') && a != "-" {
            return Err(SocatError::Usage(format!("unknown option: {a}")));
        } else {
            rest.push(a.clone());
        }
        i += 1;
    }

    Ok((gopts, rest))
}

fn print_help() {
    println!(
        "\
socat - multipurpose relay (version {VERSION})

Usage: socat [options] <address1> <address2>

Address types:
  STDIO / -            stdin/stdout combined
  STDIN / STDOUT / STDERR  individual standard fds
  PIPE[:<filename>]    named pipe (or stdio if no path)
  FILE:<filename>      regular file
  OPEN:<filename>      open file with options
  CREATE:<filename>    create file
  TCP:<host>:<port>    TCP client
  TCP-LISTEN:<port>    TCP server
  UDP:<host>:<port>    UDP client
  UDP-LISTEN:<port>    UDP server
  UNIX-CONNECT:<path>  Unix domain socket client
  UNIX-LISTEN:<path>   Unix domain socket server
  EXEC:<command>       execute program
  SYSTEM:<command>     execute via shell
  READLINE             interactive line input
  PTY                  pseudo-terminal

Address options (comma-separated):
  fork, reuseaddr, bind=<addr>, connect-timeout=<s>,
  retry=<n>, interval=<s>, crlf, ignoreeof, nonblock,
  append, create, excl

Global options:
  -d[ddd]     debug level (1-4)
  -v          verbose data dump
  -x          hex dump of data
  -b <size>   buffer size (default: {DEFAULT_BUFFER_SIZE})
  -s <size>   send buffer size
  -t <sec>    total timeout
  -T <sec>    transfer timeout
  -u          unidirectional (left to right)
  -U          reverse unidirectional (right to left)
  -4          prefer IPv4
  -6          prefer IPv6
  -V/--version   version info
  -h/--help      this help

Other personalities (via argv[0]):
  filan      list open file descriptors
  procan     show process information"
    );
}

// ---------------------------------------------------------------------------
// socat main
// ---------------------------------------------------------------------------

fn run_socat(args: Vec<String>) -> Result<(), SocatError> {
    let (gopts, positional) = parse_global_opts(&args)?;

    if positional.len() != 2 {
        return Err(SocatError::Usage(
            "socat requires exactly two address arguments".to_string(),
        ));
    }

    let left_spec = parse_address(&positional[0])?;
    let right_spec = parse_address(&positional[1])?;

    if gopts.debug.enabled(1) {
        eprintln!("socat: left  = {left_spec:?}");
        eprintln!("socat: right = {right_spec:?}");
    }

    let left_opts = get_address_opts(&left_spec);
    let right_opts = get_address_opts(&right_spec);

    let left = open_address(&left_spec, &gopts)?;
    let right = open_address(&right_spec, &gopts)?;

    relay(left, right, &gopts, &left_opts, &right_opts)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// filan personality -- file descriptor analysis
// ---------------------------------------------------------------------------

/// Describe a single file descriptor.
#[cfg(unix)]
fn describe_fd(fd: i32) -> Option<String> {
    use std::fs;
    let link = format!("/proc/self/fd/{fd}");
    let target = fs::read_link(&link).ok()?;
    let fdinfo_path = format!("/proc/self/fdinfo/{fd}");
    let fdinfo = fs::read_to_string(&fdinfo_path).unwrap_or_default();

    let mut flags_str = String::new();
    for line in fdinfo.lines() {
        if let Some(rest) = line.strip_prefix("flags:\t") {
            flags_str = rest.to_string();
            break;
        }
    }

    let target_display = target.to_string_lossy();
    Some(format!(
        "fd {fd}: {target_display}  flags: {flags_str}"
    ))
}

#[cfg(not(unix))]
fn describe_fd(fd: i32) -> Option<String> {
    // On non-Unix, we can only describe the standard three.
    match fd {
        0 => Some("fd 0: stdin".to_string()),
        1 => Some("fd 1: stdout".to_string()),
        2 => Some("fd 2: stderr".to_string()),
        _ => None,
    }
}

fn run_filan(args: Vec<String>) -> Result<(), SocatError> {
    let max_fd = if let Some(val) = args.first() {
        val.parse::<i32>().unwrap_or(256)
    } else {
        256
    };

    println!("filan: analyzing file descriptors 0..{max_fd}");
    for fd in 0..max_fd {
        if let Some(desc) = describe_fd(fd) {
            println!("  {desc}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// procan personality -- process analysis
// ---------------------------------------------------------------------------

fn run_procan(_args: Vec<String>) -> Result<(), SocatError> {
    println!("procan: process information");

    println!("  pid  = {}", process::id());

    // Read process status from /proc/self/status when available (Linux, OurOS).
    // Falls back to reporting unavailable on platforms without /proc.
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(val) = line.strip_prefix("PPid:\t") {
                println!("  ppid = {}", val.trim());
            } else if let Some(val) = line.strip_prefix("Uid:\t") {
                // Format: real effective saved fs
                let parts: Vec<&str> = val.split_whitespace().collect();
                if let Some(uid) = parts.first() {
                    println!("  uid  = {uid}");
                }
                if let Some(euid) = parts.get(1) {
                    println!("  euid = {euid}");
                }
            } else if let Some(val) = line.strip_prefix("Gid:\t") {
                let parts: Vec<&str> = val.split_whitespace().collect();
                if let Some(gid) = parts.first() {
                    println!("  gid  = {gid}");
                }
                if let Some(egid) = parts.get(1) {
                    println!("  egid = {egid}");
                }
            } else if let Some(val) = line.strip_prefix("Groups:\t") {
                println!("  groups = {}", val.trim());
            }
        }
    } else {
        println!("  ppid = (not available on this platform)");
        println!("  uid  = (not available on this platform)");
        println!("  gid  = (not available on this platform)");
    }

    // Current working directory.
    if let Ok(cwd) = env::current_dir() {
        println!("  cwd  = {}", cwd.display());
    }

    // Environment variables.
    println!("  environment:");
    let mut envs: Vec<(String, String)> = env::vars().collect();
    envs.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in &envs {
        println!("    {k}={v}");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Personality dispatch
// ---------------------------------------------------------------------------

fn detect_personality(args: &[String]) -> (&str, Vec<String>) {
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("socat");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };
    let rest: Vec<String> = args.iter().skip(1).cloned().collect();

    let personality = match prog_name.as_str() {
        "filan" => "filan",
        "procan" => "procan",
        _ => "socat",
    };

    (personality, rest)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let (personality, rest) = detect_personality(&args);

    let result = match personality {
        "filan" => run_filan(rest),
        "procan" => run_procan(rest),
        _ => run_socat(rest),
    };

    if let Err(e) = result {
        eprintln!("{personality}: {e}");
        process::exit(1);
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Personality detection
    // -----------------------------------------------------------------------

    #[test]
    fn personality_socat_default() {
        let args = vec!["socat".to_string()];
        let (p, _) = detect_personality(&args);
        assert_eq!(p, "socat");
    }

    #[test]
    fn personality_filan() {
        let args = vec!["filan".to_string(), "256".to_string()];
        let (p, rest) = detect_personality(&args);
        assert_eq!(p, "filan");
        assert_eq!(rest, vec!["256"]);
    }

    #[test]
    fn personality_procan() {
        let args = vec!["procan".to_string()];
        let (p, _) = detect_personality(&args);
        assert_eq!(p, "procan");
    }

    #[test]
    fn personality_with_path_unix() {
        let args = vec!["/usr/bin/filan".to_string()];
        let (p, _) = detect_personality(&args);
        assert_eq!(p, "filan");
    }

    #[test]
    fn personality_with_path_windows() {
        let args = vec!["C:\\bin\\procan.exe".to_string()];
        let (p, _) = detect_personality(&args);
        assert_eq!(p, "procan");
    }

    #[test]
    fn personality_unknown_defaults_to_socat() {
        let args = vec!["something_else".to_string()];
        let (p, _) = detect_personality(&args);
        assert_eq!(p, "socat");
    }

    #[test]
    fn personality_with_exe_suffix() {
        let args = vec!["socat.exe".to_string()];
        let (p, _) = detect_personality(&args);
        assert_eq!(p, "socat");
    }

    #[test]
    fn personality_empty_args() {
        let args: Vec<String> = vec![];
        let (p, rest) = detect_personality(&args);
        assert_eq!(p, "socat");
        assert!(rest.is_empty());
    }

    // -----------------------------------------------------------------------
    // Address parsing -- simple types
    // -----------------------------------------------------------------------

    #[test]
    fn parse_stdio() {
        let spec = parse_address("STDIO").unwrap();
        assert!(matches!(spec, AddressSpec::Stdio));
    }

    #[test]
    fn parse_stdio_dash() {
        let spec = parse_address("-").unwrap();
        assert!(matches!(spec, AddressSpec::Stdio));
    }

    #[test]
    fn parse_stdin() {
        let spec = parse_address("STDIN").unwrap();
        assert!(matches!(spec, AddressSpec::Stdin));
    }

    #[test]
    fn parse_stdout() {
        let spec = parse_address("STDOUT").unwrap();
        assert!(matches!(spec, AddressSpec::Stdout));
    }

    #[test]
    fn parse_stderr() {
        let spec = parse_address("STDERR").unwrap();
        assert!(matches!(spec, AddressSpec::Stderr));
    }

    #[test]
    fn parse_readline() {
        let spec = parse_address("READLINE").unwrap();
        assert!(matches!(spec, AddressSpec::Readline));
    }

    #[test]
    fn parse_stdio_case_insensitive() {
        let spec = parse_address("stdio").unwrap();
        assert!(matches!(spec, AddressSpec::Stdio));
    }

    #[test]
    fn parse_stdin_mixed_case() {
        let spec = parse_address("StdIn").unwrap();
        assert!(matches!(spec, AddressSpec::Stdin));
    }

    // -----------------------------------------------------------------------
    // Address parsing -- file types
    // -----------------------------------------------------------------------

    #[test]
    fn parse_file() {
        let spec = parse_address("FILE:/tmp/test.txt").unwrap();
        match spec {
            AddressSpec::File { path, opts } => {
                assert_eq!(path, "/tmp/test.txt");
                assert!(!opts.append);
            }
            _ => panic!("expected File"),
        }
    }

    #[test]
    fn parse_file_with_options() {
        let spec = parse_address("FILE:/tmp/out.log,append,create").unwrap();
        match spec {
            AddressSpec::File { path, opts } => {
                assert_eq!(path, "/tmp/out.log");
                assert!(opts.append);
                assert!(opts.create);
            }
            _ => panic!("expected File"),
        }
    }

    #[test]
    fn parse_file_case_insensitive() {
        let spec = parse_address("file:/data/x").unwrap();
        match spec {
            AddressSpec::File { path, .. } => assert_eq!(path, "/data/x"),
            _ => panic!("expected File"),
        }
    }

    #[test]
    fn parse_open() {
        let spec = parse_address("OPEN:/etc/config,nonblock").unwrap();
        match spec {
            AddressSpec::Open { path, opts } => {
                assert_eq!(path, "/etc/config");
                assert!(opts.nonblock);
            }
            _ => panic!("expected Open"),
        }
    }

    #[test]
    fn parse_create() {
        let spec = parse_address("CREATE:/tmp/new.txt,excl").unwrap();
        match spec {
            AddressSpec::Create { path, opts } => {
                assert_eq!(path, "/tmp/new.txt");
                assert!(opts.excl);
            }
            _ => panic!("expected Create"),
        }
    }

    // -----------------------------------------------------------------------
    // Address parsing -- pipe
    // -----------------------------------------------------------------------

    #[test]
    fn parse_pipe_no_path() {
        let spec = parse_address("PIPE").unwrap();
        match spec {
            AddressSpec::Pipe { path, .. } => assert!(path.is_none()),
            _ => panic!("expected Pipe"),
        }
    }

    #[test]
    fn parse_pipe_with_path() {
        let spec = parse_address("PIPE:/tmp/fifo").unwrap();
        match spec {
            AddressSpec::Pipe { path, .. } => {
                assert_eq!(path.as_deref(), Some("/tmp/fifo"));
            }
            _ => panic!("expected Pipe"),
        }
    }

    // -----------------------------------------------------------------------
    // Address parsing -- TCP
    // -----------------------------------------------------------------------

    #[test]
    fn parse_tcp_connect() {
        let spec = parse_address("TCP:localhost:8080").unwrap();
        match spec {
            AddressSpec::TcpConnect { host, port, .. } => {
                assert_eq!(host, "localhost");
                assert_eq!(port, 8080);
            }
            _ => panic!("expected TcpConnect"),
        }
    }

    #[test]
    fn parse_tcp_connect_ipv6() {
        let spec = parse_address("TCP:[::1]:443").unwrap();
        match spec {
            AddressSpec::TcpConnect { host, port, .. } => {
                assert_eq!(host, "::1");
                assert_eq!(port, 443);
            }
            _ => panic!("expected TcpConnect"),
        }
    }

    #[test]
    fn parse_tcp_connect_with_options() {
        let spec =
            parse_address("TCP:example.com:80,connect-timeout=5,retry=3")
                .unwrap();
        match spec {
            AddressSpec::TcpConnect { host, port, opts } => {
                assert_eq!(host, "example.com");
                assert_eq!(port, 80);
                assert_eq!(
                    opts.connect_timeout,
                    Some(Duration::from_secs(5))
                );
                assert_eq!(opts.retry, Some(3));
            }
            _ => panic!("expected TcpConnect"),
        }
    }

    #[test]
    fn parse_tcp_listen() {
        let spec = parse_address("TCP-LISTEN:9090").unwrap();
        match spec {
            AddressSpec::TcpListen { port, .. } => assert_eq!(port, 9090),
            _ => panic!("expected TcpListen"),
        }
    }

    #[test]
    fn parse_tcp_listen_with_fork() {
        let spec =
            parse_address("TCP-LISTEN:8080,fork,reuseaddr").unwrap();
        match spec {
            AddressSpec::TcpListen { port, opts } => {
                assert_eq!(port, 8080);
                assert!(opts.fork);
                assert!(opts.reuseaddr);
            }
            _ => panic!("expected TcpListen"),
        }
    }

    #[test]
    fn parse_tcp_listen_with_bind() {
        let spec =
            parse_address("TCP-LISTEN:3000,bind=127.0.0.1").unwrap();
        match spec {
            AddressSpec::TcpListen { port, opts } => {
                assert_eq!(port, 3000);
                assert_eq!(opts.bind_addr.as_deref(), Some("127.0.0.1"));
            }
            _ => panic!("expected TcpListen"),
        }
    }

    // -----------------------------------------------------------------------
    // Address parsing -- UDP
    // -----------------------------------------------------------------------

    #[test]
    fn parse_udp_client() {
        let spec = parse_address("UDP:192.168.1.1:5000").unwrap();
        match spec {
            AddressSpec::UdpClient { host, port, .. } => {
                assert_eq!(host, "192.168.1.1");
                assert_eq!(port, 5000);
            }
            _ => panic!("expected UdpClient"),
        }
    }

    #[test]
    fn parse_udp_listen() {
        let spec = parse_address("UDP-LISTEN:6000").unwrap();
        match spec {
            AddressSpec::UdpListen { port, .. } => assert_eq!(port, 6000),
            _ => panic!("expected UdpListen"),
        }
    }

    // -----------------------------------------------------------------------
    // Address parsing -- Unix domain
    // -----------------------------------------------------------------------

    #[test]
    fn parse_unix_connect() {
        let spec = parse_address("UNIX-CONNECT:/var/run/app.sock").unwrap();
        match spec {
            AddressSpec::UnixConnect { path, .. } => {
                assert_eq!(path, "/var/run/app.sock");
            }
            _ => panic!("expected UnixConnect"),
        }
    }

    #[test]
    fn parse_unix_listen() {
        let spec = parse_address("UNIX-LISTEN:/tmp/my.sock,fork").unwrap();
        match spec {
            AddressSpec::UnixListen { path, opts } => {
                assert_eq!(path, "/tmp/my.sock");
                assert!(opts.fork);
            }
            _ => panic!("expected UnixListen"),
        }
    }

    // -----------------------------------------------------------------------
    // Address parsing -- EXEC and SYSTEM
    // -----------------------------------------------------------------------

    #[test]
    fn parse_exec() {
        let spec = parse_address("EXEC:cat").unwrap();
        match spec {
            AddressSpec::Exec { command, .. } => {
                assert_eq!(command, "cat");
            }
            _ => panic!("expected Exec"),
        }
    }

    #[test]
    fn parse_exec_with_opts() {
        let spec = parse_address("EXEC:cat,fork").unwrap();
        match spec {
            AddressSpec::Exec { command, opts } => {
                assert_eq!(command, "cat");
                assert!(opts.fork);
            }
            _ => panic!("expected Exec"),
        }
    }

    #[test]
    fn parse_system() {
        let spec = parse_address("SYSTEM:ls -la").unwrap();
        match spec {
            AddressSpec::System { command, .. } => {
                assert_eq!(command, "ls -la");
            }
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn parse_exec_quoted() {
        let spec = parse_address("EXEC:\"echo hello\"").unwrap();
        match spec {
            AddressSpec::Exec { command, .. } => {
                assert_eq!(command, "echo hello");
            }
            _ => panic!("expected Exec"),
        }
    }

    // -----------------------------------------------------------------------
    // Address parsing -- PTY
    // -----------------------------------------------------------------------

    #[test]
    fn parse_pty() {
        let spec = parse_address("PTY").unwrap();
        assert!(matches!(spec, AddressSpec::Pty { .. }));
    }

    #[test]
    fn parse_pty_with_options() {
        let spec = parse_address("PTY,nonblock").unwrap();
        match spec {
            AddressSpec::Pty { opts } => {
                assert!(opts.nonblock);
            }
            _ => panic!("expected Pty"),
        }
    }

    // -----------------------------------------------------------------------
    // Address parsing -- errors
    // -----------------------------------------------------------------------

    #[test]
    fn parse_unknown_address() {
        let result = parse_address("FOOBAR:something");
        assert!(result.is_err());
    }

    #[test]
    fn parse_tcp_invalid_port() {
        let result = parse_address("TCP:host:notaport");
        assert!(result.is_err());
    }

    #[test]
    fn parse_tcp_listen_invalid_port() {
        let result = parse_address("TCP-LISTEN:xyz");
        assert!(result.is_err());
    }

    #[test]
    fn parse_udp_invalid_port() {
        let result = parse_address("UDP:host:99999");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Address option parsing
    // -----------------------------------------------------------------------

    #[test]
    fn opts_empty() {
        let opts = parse_address_options(&[]).unwrap();
        assert!(!opts.fork);
        assert!(!opts.reuseaddr);
        assert!(!opts.crlf);
    }

    #[test]
    fn opts_fork() {
        let opts = parse_address_options(&["fork"]).unwrap();
        assert!(opts.fork);
    }

    #[test]
    fn opts_reuseaddr() {
        let opts = parse_address_options(&["reuseaddr"]).unwrap();
        assert!(opts.reuseaddr);
    }

    #[test]
    fn opts_crlf() {
        let opts = parse_address_options(&["crlf"]).unwrap();
        assert!(opts.crlf);
    }

    #[test]
    fn opts_ignoreeof() {
        let opts = parse_address_options(&["ignoreeof"]).unwrap();
        assert!(opts.ignoreeof);
    }

    #[test]
    fn opts_nonblock() {
        let opts = parse_address_options(&["nonblock"]).unwrap();
        assert!(opts.nonblock);
    }

    #[test]
    fn opts_append() {
        let opts = parse_address_options(&["append"]).unwrap();
        assert!(opts.append);
    }

    #[test]
    fn opts_create() {
        let opts = parse_address_options(&["create"]).unwrap();
        assert!(opts.create);
    }

    #[test]
    fn opts_excl() {
        let opts = parse_address_options(&["excl"]).unwrap();
        assert!(opts.excl);
    }

    #[test]
    fn opts_bind() {
        let opts = parse_address_options(&["bind=127.0.0.1"]).unwrap();
        assert_eq!(opts.bind_addr.as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn opts_connect_timeout() {
        let opts =
            parse_address_options(&["connect-timeout=3.5"]).unwrap();
        assert_eq!(
            opts.connect_timeout,
            Some(Duration::from_secs_f64(3.5))
        );
    }

    #[test]
    fn opts_retry() {
        let opts = parse_address_options(&["retry=5"]).unwrap();
        assert_eq!(opts.retry, Some(5));
    }

    #[test]
    fn opts_interval() {
        let opts = parse_address_options(&["interval=2"]).unwrap();
        assert_eq!(
            opts.interval,
            Some(Duration::from_secs(2))
        );
    }

    #[test]
    fn opts_unknown_stored() {
        let opts = parse_address_options(&["weird_opt"]).unwrap();
        assert_eq!(opts.unknown, vec!["weird_opt"]);
    }

    #[test]
    fn opts_multiple() {
        let opts = parse_address_options(&[
            "fork",
            "reuseaddr",
            "crlf",
            "bind=0.0.0.0",
        ])
        .unwrap();
        assert!(opts.fork);
        assert!(opts.reuseaddr);
        assert!(opts.crlf);
        assert_eq!(opts.bind_addr.as_deref(), Some("0.0.0.0"));
    }

    #[test]
    fn opts_connect_timeout_invalid() {
        let result = parse_address_options(&["connect-timeout=abc"]);
        assert!(result.is_err());
    }

    #[test]
    fn opts_retry_invalid() {
        let result = parse_address_options(&["retry=abc"]);
        assert!(result.is_err());
    }

    #[test]
    fn opts_interval_invalid() {
        let result = parse_address_options(&["interval=nope"]);
        assert!(result.is_err());
    }

    #[test]
    fn opts_empty_items_skipped() {
        let opts = parse_address_options(&["", "fork", ""]).unwrap();
        assert!(opts.fork);
        assert!(opts.unknown.is_empty());
    }

    // -----------------------------------------------------------------------
    // split_address_options
    // -----------------------------------------------------------------------

    #[test]
    fn split_simple() {
        let parts = split_address_options("TCP:host:80,fork,reuseaddr");
        assert_eq!(parts, vec!["TCP:host:80", "fork", "reuseaddr"]);
    }

    #[test]
    fn split_no_options() {
        let parts = split_address_options("STDIO");
        assert_eq!(parts, vec!["STDIO"]);
    }

    #[test]
    fn split_quoted_comma() {
        let parts = split_address_options("EXEC:\"a,b\",fork");
        assert_eq!(parts, vec!["EXEC:\"a,b\"", "fork"]);
    }

    #[test]
    fn split_single_quoted() {
        let parts = split_address_options("EXEC:'a,b',fork");
        assert_eq!(parts, vec!["EXEC:'a,b'", "fork"]);
    }

    #[test]
    fn split_paren_comma() {
        let parts = split_address_options("EXEC:(a,b),fork");
        assert_eq!(parts, vec!["EXEC:(a,b)", "fork"]);
    }

    #[test]
    fn split_empty_trailing() {
        let parts = split_address_options("FILE:/x,");
        assert_eq!(parts, vec!["FILE:/x", ""]);
    }

    #[test]
    fn split_empty_string() {
        let parts = split_address_options("");
        assert_eq!(parts, vec![""]);
    }

    // -----------------------------------------------------------------------
    // strip_quotes
    // -----------------------------------------------------------------------

    #[test]
    fn strip_double_quotes() {
        assert_eq!(strip_quotes("\"hello\""), "hello");
    }

    #[test]
    fn strip_single_quotes() {
        assert_eq!(strip_quotes("'world'"), "world");
    }

    #[test]
    fn strip_no_quotes() {
        assert_eq!(strip_quotes("plain"), "plain");
    }

    #[test]
    fn strip_mismatched_quotes() {
        assert_eq!(strip_quotes("\"mixed'"), "\"mixed'");
    }

    #[test]
    fn strip_empty() {
        assert_eq!(strip_quotes(""), "");
    }

    #[test]
    fn strip_single_char() {
        assert_eq!(strip_quotes("x"), "x");
    }

    // -----------------------------------------------------------------------
    // strip_prefix_ci
    // -----------------------------------------------------------------------

    #[test]
    fn prefix_ci_match() {
        assert_eq!(strip_prefix_ci("TCP:host", "TCP:"), Some("host"));
    }

    #[test]
    fn prefix_ci_case_insensitive() {
        assert_eq!(strip_prefix_ci("tcp:host", "TCP:"), Some("host"));
    }

    #[test]
    fn prefix_ci_mixed_case() {
        assert_eq!(strip_prefix_ci("Tcp:host", "TCP:"), Some("host"));
    }

    #[test]
    fn prefix_ci_no_match() {
        assert_eq!(strip_prefix_ci("UDP:host", "TCP:"), None);
    }

    #[test]
    fn prefix_ci_too_short() {
        assert_eq!(strip_prefix_ci("TC", "TCP:"), None);
    }

    // -----------------------------------------------------------------------
    // parse_host_port
    // -----------------------------------------------------------------------

    #[test]
    fn host_port_simple() {
        let (h, p) = parse_host_port("localhost:8080").unwrap();
        assert_eq!(h, "localhost");
        assert_eq!(p, 8080);
    }

    #[test]
    fn host_port_ipv4() {
        let (h, p) = parse_host_port("192.168.1.1:443").unwrap();
        assert_eq!(h, "192.168.1.1");
        assert_eq!(p, 443);
    }

    #[test]
    fn host_port_ipv6_brackets() {
        let (h, p) = parse_host_port("[::1]:9090").unwrap();
        assert_eq!(h, "::1");
        assert_eq!(p, 9090);
    }

    #[test]
    fn host_port_ipv6_full() {
        let (h, p) =
            parse_host_port("[2001:db8::1]:80").unwrap();
        assert_eq!(h, "2001:db8::1");
        assert_eq!(p, 80);
    }

    #[test]
    fn host_port_no_colon() {
        let result = parse_host_port("hostonly");
        assert!(result.is_err());
    }

    #[test]
    fn host_port_bad_port() {
        let result = parse_host_port("host:abc");
        assert!(result.is_err());
    }

    #[test]
    fn host_port_port_overflow() {
        let result = parse_host_port("host:70000");
        assert!(result.is_err());
    }

    #[test]
    fn host_port_unmatched_bracket() {
        let result = parse_host_port("[::1:80");
        assert!(result.is_err());
    }

    #[test]
    fn host_port_bracket_no_colon() {
        let result = parse_host_port("[::1]80");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // parse_port
    // -----------------------------------------------------------------------

    #[test]
    fn port_valid() {
        assert_eq!(parse_port("80").unwrap(), 80);
    }

    #[test]
    fn port_max() {
        assert_eq!(parse_port("65535").unwrap(), 65535);
    }

    #[test]
    fn port_zero() {
        assert_eq!(parse_port("0").unwrap(), 0);
    }

    #[test]
    fn port_invalid() {
        assert!(parse_port("abc").is_err());
    }

    #[test]
    fn port_overflow() {
        assert!(parse_port("65536").is_err());
    }

    // -----------------------------------------------------------------------
    // Global option parsing
    // -----------------------------------------------------------------------

    #[test]
    fn gopts_defaults() {
        let (g, rest) = parse_global_opts(&[]).unwrap();
        assert_eq!(g.debug, DebugLevel::NONE);
        assert!(!g.verbose);
        assert!(!g.hex_dump);
        assert_eq!(g.buffer_size, DEFAULT_BUFFER_SIZE);
        assert!(rest.is_empty());
    }

    #[test]
    fn gopts_debug_levels() {
        let (g, _) = parse_global_opts(&["-d".into()]).unwrap();
        assert_eq!(g.debug, DebugLevel(1));
        let (g, _) = parse_global_opts(&["-dd".into()]).unwrap();
        assert_eq!(g.debug, DebugLevel(2));
        let (g, _) = parse_global_opts(&["-ddd".into()]).unwrap();
        assert_eq!(g.debug, DebugLevel(3));
        let (g, _) = parse_global_opts(&["-dddd".into()]).unwrap();
        assert_eq!(g.debug, DebugLevel(4));
    }

    #[test]
    fn gopts_verbose() {
        let (g, _) = parse_global_opts(&["-v".into()]).unwrap();
        assert!(g.verbose);
    }

    #[test]
    fn gopts_hex() {
        let (g, _) = parse_global_opts(&["-x".into()]).unwrap();
        assert!(g.hex_dump);
    }

    #[test]
    fn gopts_buffer_size() {
        let (g, _) =
            parse_global_opts(&["-b".into(), "4096".into()]).unwrap();
        assert_eq!(g.buffer_size, 4096);
    }

    #[test]
    fn gopts_send_buffer() {
        let (g, _) =
            parse_global_opts(&["-s".into(), "2048".into()]).unwrap();
        assert_eq!(g.send_buffer_size, Some(2048));
    }

    #[test]
    fn gopts_total_timeout() {
        let (g, _) =
            parse_global_opts(&["-t".into(), "10.5".into()]).unwrap();
        assert_eq!(
            g.total_timeout,
            Some(Duration::from_secs_f64(10.5))
        );
    }

    #[test]
    fn gopts_transfer_timeout() {
        let (g, _) =
            parse_global_opts(&["-T".into(), "30".into()]).unwrap();
        assert_eq!(
            g.transfer_timeout,
            Some(Duration::from_secs(30))
        );
    }

    #[test]
    fn gopts_unidirectional() {
        let (g, _) = parse_global_opts(&["-u".into()]).unwrap();
        assert!(g.unidirectional);
    }

    #[test]
    fn gopts_reverse_unidirectional() {
        let (g, _) = parse_global_opts(&["-U".into()]).unwrap();
        assert!(g.reverse_unidirectional);
    }

    #[test]
    fn gopts_ipv4() {
        let (g, _) = parse_global_opts(&["-4".into()]).unwrap();
        assert_eq!(g.ip_pref, IpPreference::V4);
    }

    #[test]
    fn gopts_ipv6() {
        let (g, _) = parse_global_opts(&["-6".into()]).unwrap();
        assert_eq!(g.ip_pref, IpPreference::V6);
    }

    #[test]
    fn gopts_positional_passthrough() {
        let (_, rest) = parse_global_opts(&[
            "-v".into(),
            "STDIO".into(),
            "TCP:host:80".into(),
        ])
        .unwrap();
        assert_eq!(rest, vec!["STDIO", "TCP:host:80"]);
    }

    #[test]
    fn gopts_dash_is_positional() {
        let (_, rest) = parse_global_opts(&["-".into()]).unwrap();
        assert_eq!(rest, vec!["-"]);
    }

    #[test]
    fn gopts_unknown_flag_error() {
        let result = parse_global_opts(&["--bogus".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn gopts_buffer_missing_arg() {
        let result = parse_global_opts(&["-b".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn gopts_buffer_invalid_arg() {
        let result = parse_global_opts(&["-b".into(), "abc".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn gopts_send_buffer_missing_arg() {
        let result = parse_global_opts(&["-s".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn gopts_total_timeout_missing() {
        let result = parse_global_opts(&["-t".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn gopts_transfer_timeout_missing() {
        let result = parse_global_opts(&["-T".into()]);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Hex dump
    // -----------------------------------------------------------------------

    #[test]
    fn hex_dump_basic() {
        let data = b"Hello, World!";
        let mut out = Vec::new();
        hex_dump(data, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("00000000"));
        assert!(s.contains("48 65 6c 6c"));
        assert!(s.contains("|Hello, World!|"));
    }

    #[test]
    fn hex_dump_empty() {
        let mut out = Vec::new();
        hex_dump(b"", &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn hex_dump_16_bytes() {
        let data: Vec<u8> = (0..16).collect();
        let mut out = Vec::new();
        hex_dump(&data, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn hex_dump_17_bytes() {
        let data: Vec<u8> = (0..17).collect();
        let mut out = Vec::new();
        hex_dump(&data, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn hex_dump_non_printable() {
        let data = [0u8, 1, 2, 3, 127, 255];
        let mut out = Vec::new();
        hex_dump(&data, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        // Non-printable chars should be rendered as '.'
        assert!(s.contains("......|"));
    }

    // -----------------------------------------------------------------------
    // DebugLevel
    // -----------------------------------------------------------------------

    #[test]
    fn debug_level_none() {
        assert!(!DebugLevel::NONE.enabled(1));
    }

    #[test]
    fn debug_level_1() {
        let d = DebugLevel(1);
        assert!(d.enabled(1));
        assert!(!d.enabled(2));
    }

    #[test]
    fn debug_level_ordering() {
        assert!(DebugLevel(2) > DebugLevel(1));
        assert!(DebugLevel(0) < DebugLevel(1));
    }

    // -----------------------------------------------------------------------
    // Error display
    // -----------------------------------------------------------------------

    #[test]
    fn error_display_io() {
        let e = SocatError::Io(io::Error::new(io::ErrorKind::NotFound, "gone"));
        let s = format!("{e}");
        assert!(s.contains("I/O error"));
        assert!(s.contains("gone"));
    }

    #[test]
    fn error_display_parse() {
        let e = SocatError::Parse("bad input".to_string());
        assert_eq!(format!("{e}"), "parse error: bad input");
    }

    #[test]
    fn error_display_address() {
        let e = SocatError::Address("no such".to_string());
        assert_eq!(format!("{e}"), "address error: no such");
    }

    #[test]
    fn error_display_option() {
        let e = SocatError::Option("invalid".to_string());
        assert_eq!(format!("{e}"), "option error: invalid");
    }

    #[test]
    fn error_display_usage() {
        let e = SocatError::Usage("need args".to_string());
        assert_eq!(format!("{e}"), "need args");
    }

    #[test]
    fn error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::Other, "test");
        let e: SocatError = io_err.into();
        assert!(matches!(e, SocatError::Io(_)));
    }

    // -----------------------------------------------------------------------
    // GlobalOpts defaults
    // -----------------------------------------------------------------------

    #[test]
    fn global_opts_default() {
        let g = GlobalOpts::default();
        assert_eq!(g.debug, DebugLevel::NONE);
        assert!(!g.verbose);
        assert!(!g.hex_dump);
        assert_eq!(g.buffer_size, DEFAULT_BUFFER_SIZE);
        assert!(g.send_buffer_size.is_none());
        assert!(g.total_timeout.is_none());
        assert!(g.transfer_timeout.is_none());
        assert!(!g.unidirectional);
        assert!(!g.reverse_unidirectional);
        assert_eq!(g.ip_pref, IpPreference::Any);
    }

    // -----------------------------------------------------------------------
    // AddressOptions default
    // -----------------------------------------------------------------------

    #[test]
    fn address_opts_default() {
        let o = AddressOptions::default();
        assert!(!o.fork);
        assert!(!o.reuseaddr);
        assert!(o.bind_addr.is_none());
        assert!(o.connect_timeout.is_none());
        assert!(o.retry.is_none());
        assert!(o.interval.is_none());
        assert!(!o.crlf);
        assert!(!o.ignoreeof);
        assert!(!o.nonblock);
        assert!(!o.append);
        assert!(!o.create);
        assert!(!o.excl);
        assert!(o.unknown.is_empty());
    }

    // -----------------------------------------------------------------------
    // get_address_opts
    // -----------------------------------------------------------------------

    #[test]
    fn get_opts_stdio() {
        let spec = AddressSpec::Stdio;
        let opts = get_address_opts(&spec);
        assert!(!opts.fork);
    }

    #[test]
    fn get_opts_tcp_listen() {
        let spec = AddressSpec::TcpListen {
            port: 80,
            opts: AddressOptions {
                fork: true,
                ..Default::default()
            },
        };
        let opts = get_address_opts(&spec);
        assert!(opts.fork);
    }

    // -----------------------------------------------------------------------
    // Transfer (in-memory)
    // -----------------------------------------------------------------------

    #[test]
    fn transfer_basic() {
        let input = b"hello world";
        let mut reader: &[u8] = input;
        let mut writer = Vec::new();
        let n = transfer(
            &mut reader, &mut writer, 1024, false, false, false, false,
        )
        .unwrap();
        assert_eq!(n, 11);
        assert_eq!(&writer, b"hello world");
    }

    #[test]
    fn transfer_crlf() {
        let input = b"line1\nline2\n";
        let mut reader: &[u8] = input;
        let mut writer = Vec::new();
        let _n = transfer(
            &mut reader, &mut writer, 1024, true, false, false, false,
        )
        .unwrap();
        assert_eq!(&writer, b"line1\r\nline2\r\n");
    }

    #[test]
    fn transfer_empty() {
        let input = b"";
        let mut reader: &[u8] = input;
        let mut writer = Vec::new();
        let n = transfer(
            &mut reader, &mut writer, 1024, false, false, false, false,
        )
        .unwrap();
        assert_eq!(n, 0);
        assert!(writer.is_empty());
    }

    #[test]
    fn transfer_small_buffer() {
        let input = b"abcdefghijklmnop";
        let mut reader: &[u8] = input;
        let mut writer = Vec::new();
        let n = transfer(
            &mut reader, &mut writer, 4, false, false, false, false,
        )
        .unwrap();
        assert_eq!(n, 16);
        assert_eq!(&writer, input);
    }

    #[test]
    fn transfer_one_byte_buffer() {
        let input = b"AB";
        let mut reader: &[u8] = input;
        let mut writer = Vec::new();
        let n = transfer(
            &mut reader, &mut writer, 1, false, false, false, false,
        )
        .unwrap();
        assert_eq!(n, 2);
        assert_eq!(&writer, b"AB");
    }

    // -----------------------------------------------------------------------
    // OpenedAddress constructors
    // -----------------------------------------------------------------------

    #[test]
    fn opened_stdio_read_write() {
        // Just ensure the constructors don't panic.
        let _ = OpenedAddress::stdio();
        let _ = OpenedAddress::stdin_only();
        let _ = OpenedAddress::stdout_only();
        let _ = OpenedAddress::stderr_only();
    }

    // -----------------------------------------------------------------------
    // run_socat usage errors
    // -----------------------------------------------------------------------

    #[test]
    fn socat_no_args_error() {
        let result = run_socat(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn socat_one_arg_error() {
        let result = run_socat(vec!["STDIO".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn socat_three_args_error() {
        let result = run_socat(vec![
            "STDIO".into(),
            "STDIO".into(),
            "STDIO".into(),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn socat_bad_address_error() {
        let result = run_socat(vec![
            "STDIO".into(),
            "UNKNOWN_ADDR:foo".into(),
        ]);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // resolve_addr (only tests that don't require DNS)
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_ipv4_literal() {
        let addr = resolve_addr("127.0.0.1", 80, IpPreference::Any).unwrap();
        assert!(addr.is_ipv4());
        assert_eq!(addr.port(), 80);
    }

    #[test]
    fn resolve_ipv6_literal() {
        let addr = resolve_addr("::1", 443, IpPreference::Any).unwrap();
        assert!(addr.is_ipv6());
        assert_eq!(addr.port(), 443);
    }

    #[test]
    fn resolve_ipv4_pref_on_v4() {
        let addr = resolve_addr("127.0.0.1", 80, IpPreference::V4).unwrap();
        assert!(addr.is_ipv4());
    }

    #[test]
    fn resolve_ipv6_pref_on_v6() {
        let addr = resolve_addr("::1", 80, IpPreference::V6).unwrap();
        assert!(addr.is_ipv6());
    }

    // -----------------------------------------------------------------------
    // IpPreference
    // -----------------------------------------------------------------------

    #[test]
    fn ip_pref_eq() {
        assert_eq!(IpPreference::Any, IpPreference::Any);
        assert_ne!(IpPreference::V4, IpPreference::V6);
    }

    // -----------------------------------------------------------------------
    // Complex address parsing integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_complex_tcp_listen() {
        let spec = parse_address(
            "TCP-LISTEN:8080,fork,reuseaddr,bind=0.0.0.0",
        )
        .unwrap();
        match spec {
            AddressSpec::TcpListen { port, opts } => {
                assert_eq!(port, 8080);
                assert!(opts.fork);
                assert!(opts.reuseaddr);
                assert_eq!(opts.bind_addr.as_deref(), Some("0.0.0.0"));
            }
            _ => panic!("expected TcpListen"),
        }
    }

    #[test]
    fn parse_complex_tcp_connect() {
        let spec = parse_address(
            "TCP:192.168.0.1:22,connect-timeout=10,retry=5,interval=2",
        )
        .unwrap();
        match spec {
            AddressSpec::TcpConnect { host, port, opts } => {
                assert_eq!(host, "192.168.0.1");
                assert_eq!(port, 22);
                assert_eq!(
                    opts.connect_timeout,
                    Some(Duration::from_secs(10))
                );
                assert_eq!(opts.retry, Some(5));
                assert_eq!(opts.interval, Some(Duration::from_secs(2)));
            }
            _ => panic!("expected TcpConnect"),
        }
    }

    #[test]
    fn parse_file_all_opts() {
        let spec = parse_address(
            "FILE:/var/log/app.log,append,create,excl,nonblock,crlf",
        )
        .unwrap();
        match spec {
            AddressSpec::File { path, opts } => {
                assert_eq!(path, "/var/log/app.log");
                assert!(opts.append);
                assert!(opts.create);
                assert!(opts.excl);
                assert!(opts.nonblock);
                assert!(opts.crlf);
            }
            _ => panic!("expected File"),
        }
    }

    // -----------------------------------------------------------------------
    // filan (basic)
    // -----------------------------------------------------------------------

    #[test]
    fn filan_describe_stdin() {
        // fd 0 should always exist in the test process.
        let desc = describe_fd(0);
        assert!(desc.is_some());
    }

    #[test]
    fn filan_describe_stdout() {
        let desc = describe_fd(1);
        assert!(desc.is_some());
    }

    #[test]
    fn filan_describe_stderr() {
        let desc = describe_fd(2);
        assert!(desc.is_some());
    }

    #[test]
    fn filan_describe_high_fd() {
        // Very high fd is almost certainly not open.
        let desc = describe_fd(9999);
        assert!(desc.is_none());
    }

    // -----------------------------------------------------------------------
    // procan smoke test
    // -----------------------------------------------------------------------

    #[test]
    fn procan_runs_ok() {
        let result = run_procan(vec![]);
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // File I/O integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn open_create_and_read() {
        let dir = std::env::temp_dir();
        let path = dir.join("socat_test_create.txt");
        let path_str = path.to_str().unwrap().to_string();

        // Clean up from any prior run.
        let _ = std::fs::remove_file(&path);

        // CREATE writes.
        let spec_str = format!("CREATE:{path_str}");
        let spec = parse_address(&spec_str).unwrap();
        let gopts = GlobalOpts::default();
        let mut opened = open_address(&spec, &gopts).unwrap();
        opened.writer.write_all(b"test data").unwrap();
        drop(opened);

        // FILE reads back.
        let spec_str = format!("FILE:{path_str}");
        let spec = parse_address(&spec_str).unwrap();
        let mut opened = open_address(&spec, &gopts).unwrap();
        let mut buf = Vec::new();
        opened.reader.read_to_end(&mut buf).unwrap();
        assert_eq!(&buf, b"test data");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn open_file_append() {
        let dir = std::env::temp_dir();
        let path = dir.join("socat_test_append.txt");
        let path_str = path.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&path);

        // Initial write.
        std::fs::write(&path, b"AAA").unwrap();

        // Append via FILE address.
        let spec_str = format!("FILE:{path_str},append,create");
        let spec = parse_address(&spec_str).unwrap();
        let gopts = GlobalOpts::default();
        let mut opened = open_address(&spec, &gopts).unwrap();
        opened.writer.write_all(b"BBB").unwrap();
        drop(opened);

        let content = std::fs::read(&path).unwrap();
        assert_eq!(&content, b"AAABBB");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn open_file_excl_fails_on_existing() {
        let dir = std::env::temp_dir();
        let path = dir.join("socat_test_excl.txt");
        let path_str = path.to_str().unwrap().to_string();

        // Ensure the file exists.
        std::fs::write(&path, b"exists").unwrap();

        let spec_str = format!("FILE:{path_str},excl,create");
        let spec = parse_address(&spec_str).unwrap();
        let gopts = GlobalOpts::default();
        let result = open_address(&spec, &gopts);
        assert!(result.is_err());

        let _ = std::fs::remove_file(&path);
    }

    // -----------------------------------------------------------------------
    // EXEC address integration
    // -----------------------------------------------------------------------

    #[test]
    fn exec_echo() {
        // Use a command that exits immediately with known output.
        let cmd = if cfg!(windows) {
            "EXEC:cmd /C echo hello"
        } else {
            "EXEC:echo hello"
        };

        // We need to parse a bit differently: the command contains spaces but
        // no commas, so it parses correctly.
        let spec = if cfg!(windows) {
            // On Windows, EXEC:cmd splits into prog="cmd", args=["/C","echo","hello"]
            parse_address("EXEC:cmd /C echo hello").unwrap()
        } else {
            parse_address("EXEC:echo hello").unwrap()
        };
        let gopts = GlobalOpts::default();
        let mut opened = open_address(&spec, &gopts).unwrap();
        let mut buf = Vec::new();
        opened.reader.read_to_end(&mut buf).unwrap();
        let output = String::from_utf8_lossy(&buf);
        assert!(output.contains("hello"));
    }

    // -----------------------------------------------------------------------
    // End-to-end: relay via in-process addresses (file to file)
    // -----------------------------------------------------------------------

    #[test]
    fn relay_file_to_file() {
        let dir = std::env::temp_dir();
        let src = dir.join("socat_relay_src.txt");
        let dst = dir.join("socat_relay_dst.txt");
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);

        std::fs::write(&src, b"relay payload").unwrap();

        let src_str = src.to_str().unwrap();
        let dst_str = dst.to_str().unwrap();

        let result = run_socat(vec![
            "-u".into(),
            format!("FILE:{src_str}"),
            format!("CREATE:{dst_str}"),
        ]);
        assert!(result.is_ok());

        let content = std::fs::read(&dst).unwrap();
        assert_eq!(&content, b"relay payload");

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }

    #[test]
    fn relay_file_to_file_crlf() {
        let dir = std::env::temp_dir();
        let src = dir.join("socat_relay_crlf_src.txt");
        let dst = dir.join("socat_relay_crlf_dst.txt");
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);

        std::fs::write(&src, b"line1\nline2\n").unwrap();

        let src_str = src.to_str().unwrap();
        let dst_str = dst.to_str().unwrap();

        let result = run_socat(vec![
            "-u".into(),
            format!("FILE:{src_str}"),
            format!("CREATE:{dst_str},crlf"),
        ]);
        assert!(result.is_ok());

        let content = std::fs::read(&dst).unwrap();
        assert_eq!(&content, b"line1\r\nline2\r\n");

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }
}
