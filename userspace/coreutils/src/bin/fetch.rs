//! fetch -- HTTP fetch utility (wget/curl-like).
//!
//! ```text
//! fetch [-OqvLI] [-o FILE] [-H "Name: Value"] [-X METHOD] [-d DATA]
//!       [-u USER:PASS] [--timeout SECS] [--no-follow] URL...
//! ```
//!
//! Downloads files over HTTP/1.1: GET, POST, PUT, DELETE, HEAD, redirects,
//! chunked transfer-encoding, basic auth, custom headers.
//!
//! # Bytes, not text
//!
//! Everything that crosses the process boundary is `&[u8]`/`OsString`. A URL is
//! ASCII by RFC 3986 only in principle; in practice a server serves
//! `/caf%C3%A9.txt` and a redirect can name a path holding any byte, and `-O`
//! turns that path into a **file name** — which on this OS may contain every
//! byte but `/` and NUL. The old file collected `env::args()` into
//! `Vec<String>`, so a command line holding such a byte aborted the process
//! before a socket was opened, and `String::from_utf8_lossy` on the response
//! headers replaced any byte it did not like with U+FFFD on the way to that
//! file name. Both are gone: argv arrives as `OsString`, the URL, the headers,
//! the POST body and the credentials are byte strings, and only the *host* is
//! required to be ASCII — because a name that is not ASCII cannot be resolved
//! without IDN, which we do not have, so refusing it is the honest answer.
//!
//! Bytes that must be shown to a person go through
//! [`escape_unprintable`](coreutils::quote::escape_unprintable), which renders
//! an unprintable byte as `\377` rather than dropping it.
//!
//! # Deliberately absent
//!
//! HTTPS (there is no TLS implementation yet), cookies, compression, resume
//! (`-C`), rate limiting, and `.netrc`. `-O` does **not** percent-decode the
//! name it takes from the URL, which is curl's behaviour: `-O` on
//! `/caf%C3%A9.txt` writes a file called `caf%C3%A9.txt`.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{escape_unprintable, os_from_bytes, quotef_os};
use coreutils::stdfd;
use std::env;
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

coreutils::guard_std_fds!();

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

const EXIT_OK: u8 = 0;
const EXIT_HTTP_ERROR: u8 = 1;
const EXIT_CONN_ERROR: u8 = 2;
const EXIT_BAD_ARGS: u8 = 3;

// ---------------------------------------------------------------------------
// Small byte-slice helpers
// ---------------------------------------------------------------------------

/// The first `needle` in `haystack`.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The first `b`.
fn find_byte(haystack: &[u8], b: u8) -> Option<usize> {
    haystack.iter().position(|&x| x == b)
}

/// The last `b`.
fn rfind_byte(haystack: &[u8], b: u8) -> Option<usize> {
    haystack.iter().rposition(|&x| x == b)
}

/// `haystack[..at]` and `haystack[at + skip..]`, without a panic if the
/// arithmetic is impossible.
fn split_at_skipping(haystack: &[u8], at: usize, skip: usize) -> (&[u8], &[u8]) {
    let head = haystack.get(..at).unwrap_or(haystack);
    let tail = at
        .checked_add(skip)
        .and_then(|from| haystack.get(from..))
        .unwrap_or(&[]);
    (head, tail)
}

// ---------------------------------------------------------------------------
// Configuration from CLI arguments
// ---------------------------------------------------------------------------

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Config {
    /// The URLs, as typed. Bytes: a redirect target or a `%`-escaped path is
    /// not required to be text, and `-O` turns one into a file name.
    urls: Vec<Vec<u8>>,
    output_file: Option<OsString>,
    output_from_url: bool,
    quiet: bool,
    verbose: bool,
    headers: Vec<(Vec<u8>, Vec<u8>)>,
    method: Option<Vec<u8>>,
    /// The `-d` body. Bytes because a POST body is not text: `-d @binary` is a
    /// normal thing to do, and curl sends whatever it was given.
    data: Option<Vec<u8>>,
    follow_redirects: bool,
    max_redirects: u32,
    head_only: bool,
    /// `user:password`, base64-encoded raw. A password is bytes.
    user_pass: Option<Vec<u8>>,
    timeout_secs: u32,
}

impl Config {
    fn new() -> Self {
        Self {
            urls: Vec::new(),
            output_file: None,
            output_from_url: false,
            quiet: false,
            verbose: false,
            headers: Vec::new(),
            method: None,
            data: None,
            follow_redirects: true,
            max_redirects: 10,
            head_only: false,
            user_pass: None,
            timeout_secs: 30,
        }
    }
}

// ---------------------------------------------------------------------------
// URL parsing
// ---------------------------------------------------------------------------

#[cfg_attr(test, derive(PartialEq, Eq))]
#[derive(Clone, Debug)]
struct ParsedUrl {
    scheme: String,
    /// ASCII, enforced by [`ParsedUrl::parse`] — a host name that is not ASCII
    /// needs IDN to become resolvable, and we have no IDN. Storing it as
    /// `String` is therefore not a decode but a fact already checked.
    host: String,
    port: u16,
    /// Bytes: the path is what a `-O` file name is taken from.
    path: Vec<u8>,
    query: Option<Vec<u8>>,
}

impl ParsedUrl {
    fn parse(raw: &[u8]) -> Result<Self, String> {
        let (scheme, rest) = match find_subsequence(raw, b"://") {
            Some(idx) => {
                let (s, r) = split_at_skipping(raw, idx, 3);
                (s.to_ascii_lowercase(), r)
            }
            // Default to http if no scheme given.
            None => (b"http".to_vec(), raw),
        };

        let default_port: u16 = match scheme.as_slice() {
            b"http" => 80,
            b"https" => 443,
            other => return Err(format!("unsupported scheme: {}", escape_unprintable(other))),
        };

        // Split host+port from path.
        let (authority, path_and_query) = match find_byte(rest, b'/') {
            Some(idx) => (
                rest.get(..idx).unwrap_or(rest),
                rest.get(idx..).unwrap_or(b"/"),
            ),
            None => (rest, &b"/"[..]),
        };

        // Separate host from port. An IPv6 literal is written `[::1]:80`, whose
        // last colon is still the port separator, so `rposition` is right for
        // it too — and the brackets are kept, because both `Host:` and
        // `to_socket_addrs` want them.
        let (host, port) = match rfind_byte(authority, b':') {
            Some(colon) => {
                let (before, after) = split_at_skipping(authority, colon, 1);
                match std::str::from_utf8(after)
                    .ok()
                    .and_then(|s| s.parse::<u16>().ok())
                {
                    Some(p) => (before, p),
                    // Not a number: the colon belongs to the host (an IPv6
                    // literal, or a malformed authority we pass on as-is).
                    None => (authority, default_port),
                }
            }
            None => (authority, default_port),
        };

        if host.is_empty() {
            return Err("empty host in URL".to_string());
        }
        // The one place text is required, and it is required rather than
        // assumed: `to_socket_addrs` takes a `&str`, and a non-ASCII name would
        // need IDN (punycode) that this program does not implement. Saying so
        // beats resolving something the user did not type.
        let Ok(host) = std::str::from_utf8(host).map(str::to_owned) else {
            return Err("non-ASCII host name (IDN is not supported)".to_string());
        };
        if !host.is_ascii() {
            return Err("non-ASCII host name (IDN is not supported)".to_string());
        }

        // Separate path from query string.
        let (path, query) = match find_byte(path_and_query, b'?') {
            Some(idx) => {
                let (p, q) = split_at_skipping(path_and_query, idx, 1);
                (p.to_vec(), Some(q.to_vec()))
            }
            None => (path_and_query.to_vec(), None),
        };

        Ok(Self {
            // `scheme` was matched against ASCII literals above, so this is a
            // check that has already passed, not a hopeful decode.
            scheme: String::from_utf8(scheme).unwrap_or_else(|_| "http".to_string()),
            host,
            port,
            path,
            query,
        })
    }

    /// Full request-target (path + optional query).
    fn request_target(&self) -> Vec<u8> {
        let mut out = self.path.clone();
        if let Some(q) = &self.query {
            out.push(b'?');
            out.extend_from_slice(q);
        }
        out
    }

    /// Derive a file name from the URL path (last path segment, or
    /// `index.html`).
    ///
    /// Not percent-decoded, which is curl's `-O` behaviour: `-O` on
    /// `/caf%C3%A9.txt` writes `caf%C3%A9.txt`. A segment that is `.` or `..`
    /// is refused rather than written, since it names a directory and the write
    /// would fail with a confusing error at best and escape the working
    /// directory at worst.
    fn filename(&self) -> Result<OsString, String> {
        let segment = match rfind_byte(&self.path, b'/') {
            Some(idx) => self.path.get(idx.saturating_add(1)..).unwrap_or(&[]),
            None => self.path.as_slice(),
        };
        if segment.is_empty() {
            return Ok(OsString::from("index.html"));
        }
        if segment == b"." || segment == b".." {
            return Err(format!(
                "refusing to write to {} derived from the URL path",
                escape_unprintable(segment)
            ));
        }
        Ok(os_from_bytes(segment))
    }
}

// ---------------------------------------------------------------------------
// Base64 encoder (for basic auth)
// ---------------------------------------------------------------------------

const BASE64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(input: &[u8]) -> String {
    let mut output = String::with_capacity(input.len().div_ceil(3).saturating_mul(4));
    let digit = |six: u32| -> char {
        // The mask cannot exceed 63 and the table has 64 entries, so the `get`
        // always succeeds; `'='` is a padding character that can never be
        // reached and is only here to avoid an `unwrap`.
        BASE64_CHARS
            .get(usize::try_from(six & 0x3F).unwrap_or(0))
            .map_or('=', |&b| char::from(b))
    };
    for group in input.chunks(3) {
        let b0 = u32::from(group.first().copied().unwrap_or(0));
        let b1 = u32::from(group.get(1).copied().unwrap_or(0));
        let b2 = u32::from(group.get(2).copied().unwrap_or(0));
        let triple = (b0 << 16) | (b1 << 8) | b2;

        output.push(digit(triple >> 18));
        output.push(digit(triple >> 12));
        output.push(if group.len() > 1 {
            digit(triple >> 6)
        } else {
            '='
        });
        output.push(if group.len() > 2 { digit(triple) } else { '=' });
    }
    output
}

// ---------------------------------------------------------------------------
// HTTP response
// ---------------------------------------------------------------------------

struct HttpResponse {
    status_code: u16,
    status_text: Vec<u8>,
    headers: Vec<(Vec<u8>, Vec<u8>)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn header_value(&self, name: &[u8]) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_slice())
    }

    fn content_length(&self) -> Option<usize> {
        let v = self.header_value(b"content-length")?;
        std::str::from_utf8(v.trim_ascii()).ok()?.parse().ok()
    }

    fn is_chunked(&self) -> bool {
        self.header_value(b"transfer-encoding")
            .is_some_and(|v| find_subsequence(&v.to_ascii_lowercase(), b"chunked").is_some())
    }

    fn location(&self) -> Option<&[u8]> {
        self.header_value(b"location")
    }
}

// ---------------------------------------------------------------------------
// Networking abstraction
// ---------------------------------------------------------------------------
// In the real Slate OS environment, these would use socket syscalls (connect,
// send, recv). For now we use std::net::TcpStream which can be swapped out
// later.

mod net {
    use std::io::{self, Read, Write};
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
    use std::time::Duration;

    /// Why a connection could not be opened.
    ///
    /// The two arms are kept apart because they want different diagnostics.
    /// A name that does not resolve is far and away the commonest failure a
    /// user hits, and the host's own text for it is not portable — Windows
    /// says `No such host is known.`, glibc says `failed to lookup address
    /// information: Name or service not known`, and neither is an errno our
    /// [`strerror`](coreutils::errmsg::strerror) can normalise. So we do not
    /// quote the platform at all for that case: [`ConnectError::Resolve`]
    /// carries no message and the caller prints one wording everywhere.
    /// A refused or unreachable *address*, by contrast, is a real errno
    /// (`ECONNREFUSED`, `ETIMEDOUT`, `ENETUNREACH`) worth reporting exactly.
    pub enum ConnectError {
        /// The host name yielded no usable address.
        Resolve,
        /// Addresses were found; none of them accepted a connection.
        Connect(io::Error),
    }

    pub struct Connection {
        stream: TcpStream,
    }

    impl Connection {
        /// Resolve `host` and connect to the first address that answers.
        ///
        /// The resolution step is the whole point of this function and was
        /// missing until 2026-08-30: it read `format!("{host}:{port}").parse()`
        /// into a `SocketAddr`, which accepts a **numeric address only**. Every
        /// URL naming a host — which is every URL anyone types — therefore
        /// failed with `bad address: invalid socket address syntax` and the
        /// utility could reach nothing but a literal IP.
        ///
        /// Every address is tried, not just the first, because a dual-stack
        /// name commonly resolves to an AAAA record this machine has no route
        /// to; the error reported is the last one, which is the one for the
        /// address family that got furthest.
        ///
        /// `timeout_secs` bounds each `connect` and each later read and write,
        /// but **not** the name lookup: `to_socket_addrs` is the platform
        /// resolver and offers no deadline. A resolver that hangs hangs us.
        pub fn connect(host: &str, port: u16, timeout_secs: u32) -> Result<Self, ConnectError> {
            let timeout = Duration::from_secs(u64::from(timeout_secs));
            let target = format!("{host}:{port}");
            let addrs: Vec<SocketAddr> = target
                .to_socket_addrs()
                .map_err(|_| ConnectError::Resolve)?
                .collect();
            if addrs.is_empty() {
                return Err(ConnectError::Resolve);
            }
            let mut last: Option<io::Error> = None;
            for addr in addrs {
                match TcpStream::connect_timeout(&addr, timeout) {
                    Ok(stream) => {
                        // A socket we cannot put a deadline on is a socket
                        // that can hang the program, so this is fatal rather
                        // than a reason to move to the next address.
                        stream
                            .set_read_timeout(Some(timeout))
                            .map_err(ConnectError::Connect)?;
                        stream
                            .set_write_timeout(Some(timeout))
                            .map_err(ConnectError::Connect)?;
                        return Ok(Self { stream });
                    }
                    Err(e) => last = Some(e),
                }
            }
            // Unreachable in practice: `addrs` was checked non-empty above, so
            // the loop ran at least once and either returned or set `last`.
            Err(last.map_or(ConnectError::Resolve, ConnectError::Connect))
        }

        pub fn send_all(&mut self, data: &[u8]) -> io::Result<()> {
            self.stream.write_all(data)
        }

        pub fn read_some(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.stream.read(buf)
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP request building and execution
// ---------------------------------------------------------------------------

/// Append `name: value` as one header line.
fn put_header(req: &mut Vec<u8>, name: &[u8], value: &[u8]) {
    req.extend_from_slice(name);
    req.extend_from_slice(b": ");
    req.extend_from_slice(value);
    req.extend_from_slice(b"\r\n");
}

fn build_request(
    method: &[u8],
    url: &ParsedUrl,
    headers: &[(Vec<u8>, Vec<u8>)],
    body: Option<&[u8]>,
    user_pass: Option<&[u8]>,
) -> Vec<u8> {
    let mut req = Vec::with_capacity(256);
    req.extend_from_slice(method);
    req.push(b' ');
    req.extend_from_slice(&url.request_target());
    req.extend_from_slice(b" HTTP/1.1\r\n");
    put_header(&mut req, b"Host", url.host.as_bytes());
    put_header(&mut req, b"Connection", b"close");
    put_header(&mut req, b"User-Agent", b"fetch/1.0 (Slate OS)");

    // Basic auth.
    if let Some(credentials) = user_pass {
        let encoded = base64_encode(credentials);
        put_header(
            &mut req,
            b"Authorization",
            format!("Basic {encoded}").as_bytes(),
        );
    }

    // Custom headers.
    for (name, value) in headers {
        put_header(&mut req, name, value);
    }

    // Body handling.
    if let Some(data) = body {
        // Only set Content-Type if the user hasn't provided one.
        let has_content_type = headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case(b"content-type"));
        if !has_content_type {
            put_header(
                &mut req,
                b"Content-Type",
                b"application/x-www-form-urlencoded",
            );
        }
        put_header(
            &mut req,
            b"Content-Length",
            data.len().to_string().as_bytes(),
        );
        req.extend_from_slice(b"\r\n");
        req.extend_from_slice(data);
    } else {
        req.extend_from_slice(b"\r\n");
    }

    req
}

/// Read the full HTTP response from a connection, parsing headers, handling
/// chunked encoding, and reading the body.
fn read_response(conn: &mut net::Connection) -> Result<HttpResponse, String> {
    // Read data incrementally until we have the full header section.
    let mut raw = Vec::with_capacity(4096);
    let mut buf = [0u8; 4096];
    let header_end;

    loop {
        let n = conn
            .read_some(&mut buf)
            .map_err(|e| format!("read error: {}", strerror(&e)))?;
        if n == 0 {
            return Err("connection closed before headers received".to_string());
        }
        raw.extend_from_slice(buf.get(..n).unwrap_or(&buf));

        // Look for the end of headers (\r\n\r\n).
        if let Some(pos) = find_subsequence(&raw, b"\r\n\r\n") {
            header_end = pos.saturating_add(4);
            break;
        }
        if raw.len() > 64 * 1024 {
            return Err("headers too large (>64KB)".to_string());
        }
    }

    // Parse status line and headers. Bytes throughout: a `Location` that ends
    // up as a `-O` file name must not have been through `from_utf8_lossy`.
    let header_bytes = raw.get(..header_end).unwrap_or(&raw);
    let mut lines = header_bytes.split(|&b| b == b'\n');

    let status_line = lines.next().ok_or("empty response")?;
    let (status_code, status_text) = parse_status_line(status_line)?;

    let mut headers: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for line in lines {
        let trimmed = strip_cr(line);
        if trimmed.is_empty() {
            break;
        }
        if let Some(colon) = find_byte(trimmed, b':') {
            let (name, value) = split_at_skipping(trimmed, colon, 1);
            headers.push((name.trim_ascii().to_vec(), value.trim_ascii().to_vec()));
        }
    }

    let mut response = HttpResponse {
        status_code,
        status_text,
        headers,
        body: Vec::new(),
    };

    // Read body.
    let body_start_data = raw.get(header_end..).unwrap_or(&[]).to_vec();

    if response.is_chunked() {
        response.body = read_chunked_body(conn, &body_start_data)?;
    } else if let Some(content_len) = response.content_length() {
        response.body = read_fixed_body(conn, &body_start_data, content_len)?;
    } else {
        // Read until connection close.
        response.body = read_until_close(conn, &body_start_data)?;
    }

    Ok(response)
}

/// Drop one trailing `\r`, which is what splitting the header block on `\n`
/// leaves behind.
fn strip_cr(line: &[u8]) -> &[u8] {
    match line.split_last() {
        Some((b'\r', rest)) => rest,
        _ => line,
    }
}

fn parse_status_line(line: &[u8]) -> Result<(u16, Vec<u8>), String> {
    // Format: HTTP/1.x STATUS_CODE STATUS_TEXT
    let line = strip_cr(line);
    let mut parts = line.splitn(3, |&b| b == b' ');
    let _version = parts.next();
    let Some(code_bytes) = parts.next() else {
        return Err(format!(
            "malformed status line: {}",
            escape_unprintable(line)
        ));
    };
    let code = std::str::from_utf8(code_bytes)
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| format!("invalid status code: {}", escape_unprintable(code_bytes)))?;
    let text = parts.next().map(strip_cr).unwrap_or(&[]).to_vec();
    Ok((code, text))
}

fn read_fixed_body(
    conn: &mut net::Connection,
    initial: &[u8],
    total: usize,
) -> Result<Vec<u8>, String> {
    let mut body = Vec::with_capacity(total.min(1 << 20));
    body.extend_from_slice(initial);

    let mut buf = [0u8; 8192];
    while body.len() < total {
        let n = conn
            .read_some(&mut buf)
            .map_err(|e| format!("read error: {}", strerror(&e)))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(buf.get(..n).unwrap_or(&buf));
    }
    body.truncate(total);
    Ok(body)
}

fn read_chunked_body(conn: &mut net::Connection, initial: &[u8]) -> Result<Vec<u8>, String> {
    let mut raw = Vec::from(initial);
    let mut buf = [0u8; 8192];

    // Parse chunk by chunk, reading more whenever the buffer runs short, until
    // the terminating zero-length chunk.
    let mut body = Vec::new();

    loop {
        match try_parse_chunk(&raw) {
            ChunkParse::NeedMore => {
                let n = conn
                    .read_some(&mut buf)
                    .map_err(|e| format!("read error: {}", strerror(&e)))?;
                if n == 0 {
                    // Connection closed; return what we have.
                    break;
                }
                raw.extend_from_slice(buf.get(..n).unwrap_or(&buf));
            }
            ChunkParse::Chunk { size, consumed } => {
                if size == 0 {
                    break; // Final chunk.
                }
                // `size` bytes of data plus the two that end the chunk.
                let needed = consumed.saturating_add(size).saturating_add(2);
                while raw.len() < needed {
                    let n = conn
                        .read_some(&mut buf)
                        .map_err(|e| format!("read error: {}", strerror(&e)))?;
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(buf.get(..n).unwrap_or(&buf));
                }
                let data_end = consumed.saturating_add(size);
                if let Some(chunk) = raw.get(consumed..data_end) {
                    body.extend_from_slice(chunk);
                }
                // Advance past chunk data + \r\n.
                let advance = needed.min(raw.len());
                raw = raw.get(advance..).unwrap_or(&[]).to_vec();
            }
            ChunkParse::Invalid(msg) => {
                return Err(format!("chunked encoding error: {msg}"));
            }
        }
    }

    Ok(body)
}

enum ChunkParse {
    NeedMore,
    Chunk { size: usize, consumed: usize },
    Invalid(String),
}

fn try_parse_chunk(data: &[u8]) -> ChunkParse {
    // Look for \r\n that ends the chunk-size line.
    let Some(line_end) = find_subsequence(data, b"\r\n") else {
        return ChunkParse::NeedMore;
    };

    let line = data.get(..line_end).unwrap_or(data);
    // Strip chunk extensions (anything after ';').
    let hex_part = match find_byte(line, b';') {
        Some(idx) => line.get(..idx).unwrap_or(line),
        None => line,
    }
    .trim_ascii();

    match std::str::from_utf8(hex_part)
        .ok()
        .and_then(|s| usize::from_str_radix(s, 16).ok())
    {
        Some(size) => ChunkParse::Chunk {
            size,
            consumed: line_end.saturating_add(2),
        },
        None => ChunkParse::Invalid(format!("bad chunk size: {}", escape_unprintable(line))),
    }
}

fn read_until_close(conn: &mut net::Connection, initial: &[u8]) -> Result<Vec<u8>, String> {
    let mut body = Vec::from(initial);
    let mut buf = [0u8; 8192];
    loop {
        let n = conn
            .read_some(&mut buf)
            .map_err(|e| format!("read error: {}", strerror(&e)))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(buf.get(..n).unwrap_or(&buf));
    }
    Ok(body)
}

// ---------------------------------------------------------------------------
// Human-readable file size
// ---------------------------------------------------------------------------

#[allow(
    clippy::cast_precision_loss,
    reason = "a size is being rendered to one decimal place for a person to \
              read; the rounding a f64 introduces above 2^53 bytes is far \
              below the precision the output shows."
)]
fn format_size(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = 1024 * KB;
    const GB: usize = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

// ---------------------------------------------------------------------------
// Single URL fetch (with redirect following)
// ---------------------------------------------------------------------------

fn fetch_url(config: &Config, url_bytes: &[u8], out: &mut stdfd::Stream) -> u8 {
    let method = determine_method(config);

    let mut current_url = match ParsedUrl::parse(url_bytes) {
        Ok(u) => u,
        // Bound as `why`, not `e`: `ParsedUrl::parse` returns our own sentence
        // about the URL's shape, not an `io::Error` whose text is the host's.
        // The name is what `scripts/host-errmsg.py` reads to tell the two
        // apart, and here it is simply the accurate one.
        Err(why) => {
            diag!(
                "fetch: invalid URL {}: {why}",
                escape_unprintable(url_bytes)
            );
            return EXIT_BAD_ARGS;
        }
    };

    if current_url.scheme == "https" {
        diag!("fetch: HTTPS not yet supported (no TLS implementation)");
        return EXIT_CONN_ERROR;
    }

    let mut redirects_remaining = if config.follow_redirects {
        config.max_redirects
    } else {
        0
    };

    loop {
        let request_method: &[u8] = if config.head_only { b"HEAD" } else { &method };

        if config.verbose {
            diag!(
                "> {} {} HTTP/1.1",
                escape_unprintable(request_method),
                escape_unprintable(&current_url.request_target())
            );
            diag!("> Host: {}", current_url.host);
            for (name, value) in &config.headers {
                diag!(
                    "> {}: {}",
                    escape_unprintable(name),
                    escape_unprintable(value)
                );
            }
            diag!(">");
        }

        let request = build_request(
            request_method,
            &current_url,
            &config.headers,
            config.data.as_deref(),
            config.user_pass.as_deref(),
        );

        // Connect.
        let mut conn = match net::Connection::connect(
            &current_url.host,
            current_url.port,
            config.timeout_secs,
        ) {
            Ok(c) => c,
            Err(net::ConnectError::Resolve) => {
                diag!("fetch: could not resolve host: {}", current_url.host);
                return EXIT_CONN_ERROR;
            }
            Err(net::ConnectError::Connect(e)) => {
                diag!(
                    "fetch: connection to {}:{} failed: {}",
                    current_url.host,
                    current_url.port,
                    strerror(&e)
                );
                return EXIT_CONN_ERROR;
            }
        };

        // Send request.
        if let Err(e) = conn.send_all(&request) {
            diag!("fetch: send failed: {}", strerror(&e));
            return EXIT_CONN_ERROR;
        }

        // Read response.
        let response = match read_response(&mut conn) {
            Ok(r) => r,
            Err(e) => {
                diag!("fetch: {e}");
                return EXIT_CONN_ERROR;
            }
        };

        if config.verbose {
            diag!(
                "< HTTP/1.1 {} {}",
                response.status_code,
                escape_unprintable(&response.status_text)
            );
            for (name, value) in &response.headers {
                diag!(
                    "< {}: {}",
                    escape_unprintable(name),
                    escape_unprintable(value)
                );
            }
            diag!("<");
        }

        // Check for redirect.
        let is_redirect = matches!(response.status_code, 301 | 302 | 303 | 307 | 308);

        if is_redirect && redirects_remaining > 0 {
            if let Some(location) = response.location() {
                let next_url = resolve_redirect(&current_url, location);
                match ParsedUrl::parse(&next_url) {
                    Ok(parsed) => {
                        if !config.quiet {
                            diag!(
                                "fetch: following redirect to {}",
                                escape_unprintable(&next_url)
                            );
                        }
                        if parsed.scheme == "https" {
                            diag!("fetch: HTTPS not yet supported (no TLS implementation)");
                            return EXIT_CONN_ERROR;
                        }
                        current_url = parsed;
                        redirects_remaining = redirects_remaining.saturating_sub(1);
                        continue;
                    }
                    // `why` for the same reason as at the first parse above:
                    // this is our sentence about the URL, not the host's
                    // wording for an errno.
                    Err(why) => {
                        diag!(
                            "fetch: invalid redirect URL {}: {why}",
                            escape_unprintable(location)
                        );
                        return EXIT_HTTP_ERROR;
                    }
                }
            }
        } else if is_redirect && redirects_remaining == 0 && config.follow_redirects {
            diag!("fetch: too many redirects (max {})", config.max_redirects);
            return EXIT_HTTP_ERROR;
        }

        // Print status to stderr.
        if !config.quiet {
            diag!(
                "HTTP {} {} [{}]",
                response.status_code,
                escape_unprintable(&response.status_text),
                format_size(response.body.len())
            );
        }

        // HEAD request: show headers and exit.
        if config.head_only {
            for (name, value) in &response.headers {
                let _ = out.write_all(name);
                let _ = out.write_all(b": ");
                let _ = out.write_all(value);
                let _ = out.write_all(b"\n");
            }
            return if response.status_code < 400 {
                EXIT_OK
            } else {
                EXIT_HTTP_ERROR
            };
        }

        // Determine output destination.
        if let Err(e) = write_output(config, &current_url, &response.body, out) {
            diag!("fetch: {e}");
            return EXIT_CONN_ERROR;
        }

        return if response.status_code >= 400 {
            EXIT_HTTP_ERROR
        } else {
            EXIT_OK
        };
    }
}

fn determine_method(config: &Config) -> Vec<u8> {
    if let Some(m) = &config.method {
        m.to_ascii_uppercase()
    } else if config.data.is_some() {
        b"POST".to_vec()
    } else {
        b"GET".to_vec()
    }
}

fn resolve_redirect(base: &ParsedUrl, location: &[u8]) -> Vec<u8> {
    if location.starts_with(b"http://") || location.starts_with(b"https://") {
        // Absolute URL.
        return location.to_vec();
    }
    let mut out = format!("{}://{}:{}", base.scheme, base.host, base.port).into_bytes();
    if location.starts_with(b"/") {
        // Absolute path, same origin.
        out.extend_from_slice(location);
        return out;
    }
    // Relative path: append to the current directory.
    let dir = match rfind_byte(&base.path, b'/') {
        Some(idx) => base.path.get(..=idx).unwrap_or(b"/"),
        None => b"/",
    };
    out.extend_from_slice(dir);
    out.extend_from_slice(location);
    out
}

fn write_output(
    config: &Config,
    url: &ParsedUrl,
    body: &[u8],
    out: &mut stdfd::Stream,
) -> Result<(), String> {
    let target = if let Some(path) = &config.output_file {
        Some(path.clone())
    } else if config.output_from_url {
        Some(url.filename()?)
    } else {
        None
    };

    let Some(path) = target else {
        let _ = out.write_all(body);
        return Ok(());
    };

    std::fs::write(&path, body).map_err(|e| format!("{}: {}", quotef_os(&path), strerror(&e)))?;
    if !config.quiet {
        diag!(
            "Saved to: {} [{}]",
            quotef_os(&path),
            format_size(body.len())
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

const FETCH: Program = Program::new("fetch", EXIT_BAD_ARGS as i32);

/// GNU's `getopt_long` short string for this utility. Every option that takes a
/// value must appear here even if it had no short spelling, or the value is
/// left behind as an operand — see [`Program::parse`].
const SHORTS: &str = "o:OqvH:X:d:LIu:hV";

const LONGS: &[(&str, Takes)] = &[
    ("output", Takes::Required),
    ("quiet", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("header", Takes::Required),
    ("method", Takes::Required),
    ("data", Takes::Required),
    ("follow", Takes::Nothing),
    ("no-follow", Takes::Nothing),
    ("head", Takes::Nothing),
    ("user", Takes::Required),
    ("timeout", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// Outcome of CLI parsing: a usable [`Config`], or one of the two requests that
/// print and exit.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum ParseOutcome {
    Config(Box<Config>),
    Help,
    Version,
}

fn parse_args(args: &[OsString]) -> Result<ParseOutcome, String> {
    let mut config = Config::new();

    for item in FETCH.parse(args, SHORTS, LONGS) {
        let item = item.map_err(|e| e.sentence)?;
        match item {
            Opt::Short(b'h', _) | Opt::Long("help", _) => return Ok(ParseOutcome::Help),
            Opt::Short(b'V', _) | Opt::Long("version", _) => return Ok(ParseOutcome::Version),
            Opt::Short(b'o', v) | Opt::Long("output", v) => {
                config.output_file = v;
            }
            Opt::Short(b'O', _) => config.output_from_url = true,
            Opt::Short(b'q', _) | Opt::Long("quiet", _) => config.quiet = true,
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => config.verbose = true,
            Opt::Short(b'H', v) | Opt::Long("header", v) => {
                let raw = value_bytes(v.as_ref());
                config.headers.push(parse_header_arg(&raw)?);
            }
            Opt::Short(b'X', v) | Opt::Long("method", v) => {
                config.method = Some(value_bytes(v.as_ref()).to_ascii_uppercase());
            }
            Opt::Short(b'd', v) | Opt::Long("data", v) => {
                config.data = Some(value_bytes(v.as_ref()));
            }
            Opt::Short(b'L', _) | Opt::Long("follow", _) => config.follow_redirects = true,
            Opt::Long("no-follow", _) => config.follow_redirects = false,
            Opt::Short(b'I', _) | Opt::Long("head", _) => config.head_only = true,
            Opt::Short(b'u', v) | Opt::Long("user", v) => {
                config.user_pass = Some(value_bytes(v.as_ref()));
            }
            Opt::Long("timeout", v) => {
                let raw = value_bytes(v.as_ref());
                config.timeout_secs = std::str::from_utf8(&raw)
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
                    .ok_or_else(|| {
                        format!("invalid timeout value: {}", escape_unprintable(&raw))
                    })?;
            }
            Opt::Operand(word) => config.urls.push(value_bytes(Some(word)).to_vec()),
            // Every letter in SHORTS and every name in LONGS is handled above;
            // an unlisted one is refused by the parser before it reaches here.
            Opt::Short(_, _) | Opt::Long(_, _) => {}
        }
    }

    if config.urls.is_empty() {
        return Err("no URL specified".to_string());
    }

    Ok(ParseOutcome::Config(Box::new(config)))
}

/// An option's value as bytes.
///
/// `None` cannot happen for an option [`SHORTS`] marks as taking a value — the
/// parser reports a missing one as an error rather than yielding `None` — so
/// the empty vector is a value that will fail its own validation, not a
/// silently-accepted default.
fn value_bytes<S: AsRef<std::ffi::OsStr>>(v: Option<S>) -> Vec<u8> {
    v.map(|s| coreutils::quote::os_bytes(s.as_ref()).into_owned())
        .unwrap_or_default()
}

fn parse_header_arg(raw: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let Some(colon) = find_byte(raw, b':') else {
        return Err(format!(
            "invalid header format (expected \"Name: Value\"): {}",
            escape_unprintable(raw)
        ));
    };
    let (name, value) = split_at_skipping(raw, colon, 1);
    let name = name.trim_ascii();
    if name.is_empty() {
        return Err(format!("empty header name in: {}", escape_unprintable(raw)));
    }
    Ok((name.to_vec(), value.trim_ascii().to_vec()))
}

// ---------------------------------------------------------------------------
// Help text
// ---------------------------------------------------------------------------

const HELP: &str = "\
fetch - HTTP fetch utility for Slate OS

Usage: fetch [OPTIONS] URL...

Options:
  -o, --output FILE    Write response body to FILE
  -O                   Write to a file named by the URL's last path segment
  -q, --quiet          Suppress progress and status messages
  -v, --verbose        Show request and response headers
  -H, --header \"N: V\"  Add a custom request header (repeatable)
  -X, --method METHOD  HTTP method (GET, POST, PUT, DELETE, HEAD)
  -d, --data DATA      Request body (implies POST unless -X says otherwise)
  -L, --follow         Follow redirects (default: yes, up to 10 hops)
      --no-follow      Don't follow redirects
  -I, --head           Send HEAD request, display response headers only
  -u, --user USER:PASS HTTP Basic authentication credentials
      --timeout SECS   Connect/read/write timeout in seconds (default: 30)
                       The name lookup itself is the resolver's and is not
                       bounded by this.
  -h, --help           Show this help message
  -V, --version        Show version information

Exit codes:
  0  Success
  1  HTTP error (4xx or 5xx status)
  2  Connection/network error
  3  Invalid arguments or usage error

Examples:
  fetch http://example.com/
  fetch -o page.html http://example.com/index.html
  fetch -O http://example.com/files/archive.tar
  fetch -v -H \"Accept: application/json\" http://api.example.com/data
  fetch -X POST -d \"key=value\" http://example.com/submit
  fetch -I http://example.com/
  fetch -u admin:secret http://example.com/private/

Note: HTTPS is not yet supported (requires TLS implementation).";

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let mut out = stdfd::Stream::stdout();
    let config = match parse_args(&args) {
        Ok(ParseOutcome::Config(c)) => c,
        Ok(ParseOutcome::Help) => {
            let _ = writeln!(out, "{HELP}");
            return stdfd::close_stdout("fetch", out, ExitCode::from(EXIT_OK));
        }
        Ok(ParseOutcome::Version) => {
            let _ = writeln!(out, "fetch (SlateOS coreutils) 0.1.0");
            return stdfd::close_stdout("fetch", out, ExitCode::from(EXIT_OK));
        }
        Err(e) => {
            diag!("fetch: {e}");
            diag!("Try 'fetch --help' for usage information.");
            return ExitCode::from(EXIT_BAD_ARGS);
        }
    };

    let mut worst_exit = EXIT_OK;

    for url in &config.urls {
        let code = fetch_url(&config, url, &mut out);
        if code > worst_exit {
            worst_exit = code;
        }
    }

    stdfd::close_stdout("fetch", out, ExitCode::from(worst_exit))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn cfg(args: &[&str]) -> Config {
        match parse_args(&s(args)).unwrap() {
            ParseOutcome::Config(c) => *c,
            other => panic!("expected Config, got {other:?}"),
        }
    }

    fn url(raw: &str) -> ParsedUrl {
        ParsedUrl::parse(raw.as_bytes()).unwrap()
    }

    // ---------- parse_args ----------

    #[test]
    fn parse_args_empty_errors() {
        let err = parse_args(&[]).unwrap_err();
        assert!(err.contains("no URL"));
    }

    #[test]
    fn parse_args_single_url() {
        let c = cfg(&["http://example.com/"]);
        assert_eq!(c.urls, vec![b"http://example.com/".to_vec()]);
        assert!(c.follow_redirects);
        assert_eq!(c.timeout_secs, 30);
    }

    #[test]
    fn parse_args_help_flag() {
        assert_eq!(parse_args(&s(&["-h"])).unwrap(), ParseOutcome::Help);
        assert_eq!(parse_args(&s(&["--help"])).unwrap(), ParseOutcome::Help);
    }

    #[test]
    fn parse_args_version_flag() {
        assert_eq!(parse_args(&s(&["-V"])).unwrap(), ParseOutcome::Version);
        assert_eq!(
            parse_args(&s(&["--version"])).unwrap(),
            ParseOutcome::Version
        );
    }

    #[test]
    fn parse_args_output_file() {
        let c = cfg(&["-o", "out.html", "http://x/"]);
        assert_eq!(c.output_file, Some(OsString::from("out.html")));
        let c = cfg(&["--output", "out.html", "http://x/"]);
        assert_eq!(c.output_file, Some(OsString::from("out.html")));
        // Attached forms, which the hand-written loop could not read at all.
        let c = cfg(&["-oout.html", "http://x/"]);
        assert_eq!(c.output_file, Some(OsString::from("out.html")));
        let c = cfg(&["--output=out.html", "http://x/"]);
        assert_eq!(c.output_file, Some(OsString::from("out.html")));
    }

    #[test]
    fn parse_args_missing_output_value_errors() {
        // getopt's own wording, which is POSIX's: `option requires an
        // argument -- 'o'`. The test pins the letter, not the sentence.
        let err = parse_args(&s(&["-o"])).unwrap_err();
        assert!(err.contains("requires an argument"), "{err}");
        assert!(err.contains('o'), "{err}");
    }

    #[test]
    fn parse_args_output_from_url() {
        let c = cfg(&["-O", "http://x/file.tar"]);
        assert!(c.output_from_url);
    }

    #[test]
    fn parse_args_quiet_and_verbose() {
        let c = cfg(&["-q", "-v", "http://x/"]);
        assert!(c.quiet);
        assert!(c.verbose);
        // Bundled, which is the same command line a person actually types.
        let c = cfg(&["-qv", "http://x/"]);
        assert!(c.quiet);
        assert!(c.verbose);
    }

    #[test]
    fn parse_args_long_option_may_be_abbreviated() {
        // `getopt_long` resolves any unambiguous prefix, and every other
        // utility in this crate does; this one used to accept only the
        // full spelling.
        let c = cfg(&["--verb", "http://x/"]);
        assert!(c.verbose);
    }

    #[test]
    fn parse_args_ambiguous_abbreviation_errors() {
        // `--he` is both `--header` and `--head`.
        let err = parse_args(&s(&["--he", "http://x/"])).unwrap_err();
        assert!(err.contains("ambiguous"), "{err}");
    }

    #[test]
    fn parse_args_double_dash_ends_options() {
        // The only way to name a URL that begins with `-`. The hand-written
        // loop had no `--`, so such a URL was unreachable.
        let c = cfg(&["--", "-weird-host/path"]);
        assert_eq!(c.urls, vec![b"-weird-host/path".to_vec()]);
    }

    #[test]
    fn parse_args_method_uppercased() {
        let c = cfg(&["-X", "post", "http://x/"]);
        assert_eq!(c.method, Some(b"POST".to_vec()));
    }

    #[test]
    fn parse_args_data_implies_post_via_determine_method() {
        // -d alone doesn't change config.method, but determine_method() picks POST.
        let c = cfg(&["-d", "key=value", "http://x/"]);
        assert_eq!(c.data, Some(b"key=value".to_vec()));
        assert_eq!(c.method, None);
        assert_eq!(determine_method(&c), b"POST".to_vec());
    }

    #[test]
    fn parse_args_header_collects_multiple() {
        let c = cfg(&[
            "-H",
            "Accept: application/json",
            "-H",
            "X-Custom: 42",
            "http://x/",
        ]);
        assert_eq!(
            c.headers,
            vec![
                (b"Accept".to_vec(), b"application/json".to_vec()),
                (b"X-Custom".to_vec(), b"42".to_vec()),
            ]
        );
    }

    #[test]
    fn parse_args_bad_header_errors() {
        let err = parse_args(&s(&["-H", "no-colon", "http://x/"])).unwrap_err();
        assert!(err.contains("invalid header format"));
    }

    #[test]
    fn parse_args_follow_and_no_follow() {
        let c = cfg(&["--no-follow", "http://x/"]);
        assert!(!c.follow_redirects);
        let c = cfg(&["-L", "http://x/"]);
        assert!(c.follow_redirects);
    }

    #[test]
    fn parse_args_head_only() {
        let c = cfg(&["-I", "http://x/"]);
        assert!(c.head_only);
    }

    #[test]
    fn parse_args_user_pass() {
        let c = cfg(&["-u", "admin:secret", "http://x/"]);
        assert_eq!(c.user_pass, Some(b"admin:secret".to_vec()));
    }

    #[test]
    fn parse_args_timeout_numeric() {
        let c = cfg(&["--timeout", "5", "http://x/"]);
        assert_eq!(c.timeout_secs, 5);
    }

    #[test]
    fn parse_args_bad_timeout_errors() {
        let err = parse_args(&s(&["--timeout", "soon", "http://x/"])).unwrap_err();
        assert!(err.contains("invalid timeout"));
    }

    #[test]
    fn parse_args_unknown_flag_errors() {
        assert!(parse_args(&s(&["-Z", "http://x/"])).is_err());
        assert!(parse_args(&s(&["--nonesuch", "http://x/"])).is_err());
    }

    #[test]
    fn parse_args_multiple_urls() {
        let c = cfg(&["http://a/", "http://b/"]);
        assert_eq!(c.urls, vec![b"http://a/".to_vec(), b"http://b/".to_vec()]);
    }

    // ---------- bytes end to end ----------

    #[cfg(unix)]
    #[test]
    fn a_url_holding_an_invalid_byte_is_carried_not_decoded() {
        use std::os::unix::ffi::OsStringExt;
        // The whole point of the conversion: this argv used to abort the
        // process inside `env::args()` before anything ran.
        let argv = vec![OsString::from_vec(b"http://h/a\xffb".to_vec())];
        let c = match parse_args(&argv).unwrap() {
            ParseOutcome::Config(c) => *c,
            other => panic!("expected Config, got {other:?}"),
        };
        assert_eq!(c.urls, vec![b"http://h/a\xffb".to_vec()]);
        let u = ParsedUrl::parse(&c.urls[0]).unwrap();
        assert_eq!(u.path, b"/a\xffb".to_vec());
        // And it survives all the way to the name `-O` would write.
        assert_eq!(
            u.filename().unwrap(),
            OsString::from_vec(b"a\xffb".to_vec())
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_post_body_may_be_binary() {
        use std::os::unix::ffi::OsStringExt;
        let argv = vec![
            OsString::from("-d"),
            OsString::from_vec(vec![0x01, 0xff, 0x80]),
            OsString::from("http://h/"),
        ];
        let c = match parse_args(&argv).unwrap() {
            ParseOutcome::Config(c) => *c,
            other => panic!("expected Config, got {other:?}"),
        };
        assert_eq!(c.data, Some(vec![0x01, 0xff, 0x80]));
        let req = build_request(b"POST", &url("http://h/"), &[], c.data.as_deref(), None);
        assert!(req.ends_with(&[0x01, 0xff, 0x80]));
        assert!(find_subsequence(&req, b"Content-Length: 3\r\n").is_some());
    }

    #[test]
    fn a_non_ascii_host_is_refused_rather_than_guessed() {
        let err = ParsedUrl::parse("http://caf\u{e9}.example/".as_bytes()).unwrap_err();
        assert!(err.contains("IDN"), "{err}");
    }

    // ---------- parse_header_arg ----------

    #[test]
    fn parse_header_splits_on_first_colon() {
        let (n, v) = parse_header_arg(b"Accept: text/html").unwrap();
        assert_eq!(n, b"Accept".to_vec());
        assert_eq!(v, b"text/html".to_vec());
        // A value may itself contain a colon.
        let (n, v) = parse_header_arg(b"Referer: http://x/y").unwrap();
        assert_eq!(n, b"Referer".to_vec());
        assert_eq!(v, b"http://x/y".to_vec());
    }

    #[test]
    fn parse_header_trims_whitespace() {
        let (n, v) = parse_header_arg(b"  Accept  :   text/html  ").unwrap();
        assert_eq!(n, b"Accept".to_vec());
        assert_eq!(v, b"text/html".to_vec());
    }

    #[test]
    fn parse_header_no_colon_errors() {
        assert!(parse_header_arg(b"nocolon").is_err());
    }

    #[test]
    fn parse_header_empty_name_errors() {
        assert!(parse_header_arg(b": value").is_err());
    }

    // ---------- ParsedUrl ----------

    #[test]
    fn parsed_url_http_with_path() {
        let u = url("http://example.com/some/path");
        assert_eq!(u.scheme, "http");
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, 80);
        assert_eq!(u.path, b"/some/path".to_vec());
        assert_eq!(u.query, None);
    }

    #[test]
    fn parsed_url_https_default_port() {
        let u = url("https://example.com/");
        assert_eq!(u.scheme, "https");
        assert_eq!(u.port, 443);
    }

    #[test]
    fn parsed_url_explicit_port() {
        let u = url("http://example.com:8080/x");
        assert_eq!(u.port, 8080);
        assert_eq!(u.host, "example.com");
    }

    #[test]
    fn parsed_url_ipv6_literal_keeps_its_brackets() {
        // `to_socket_addrs` and the `Host:` header both want them.
        let u = url("http://[::1]:8080/x");
        assert_eq!(u.host, "[::1]");
        assert_eq!(u.port, 8080);
        let u = url("http://[::1]/x");
        assert_eq!(u.host, "[::1]");
        assert_eq!(u.port, 80);
    }

    #[test]
    fn parsed_url_no_scheme_defaults_to_http() {
        let u = url("example.com/x");
        assert_eq!(u.scheme, "http");
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, 80);
    }

    #[test]
    fn parsed_url_no_path_defaults_to_slash() {
        let u = url("http://example.com");
        assert_eq!(u.path, b"/".to_vec());
    }

    #[test]
    fn parsed_url_query_string() {
        let u = url("http://example.com/s?q=1&r=2");
        assert_eq!(u.path, b"/s".to_vec());
        assert_eq!(u.query, Some(b"q=1&r=2".to_vec()));
    }

    #[test]
    fn parsed_url_unsupported_scheme_errors() {
        assert!(ParsedUrl::parse(b"ftp://example.com/").is_err());
    }

    #[test]
    fn parsed_url_empty_host_errors() {
        assert!(ParsedUrl::parse(b"http:///path").is_err());
    }

    #[test]
    fn parsed_url_request_target_includes_query() {
        let u = url("http://x/p?q=1");
        assert_eq!(u.request_target(), b"/p?q=1".to_vec());
    }

    #[test]
    fn parsed_url_request_target_no_query() {
        let u = url("http://x/p");
        assert_eq!(u.request_target(), b"/p".to_vec());
    }

    #[test]
    fn parsed_url_filename_from_last_segment() {
        let u = url("http://x/a/b/file.tar");
        assert_eq!(u.filename().unwrap(), OsString::from("file.tar"));
    }

    #[test]
    fn parsed_url_filename_trailing_slash_is_index_html() {
        let u = url("http://x/a/b/");
        assert_eq!(u.filename().unwrap(), OsString::from("index.html"));
    }

    #[test]
    fn parsed_url_filename_refuses_dot_segments() {
        // `-O` on a path ending in `..` must not try to write a directory,
        // still less one above the working directory.
        assert!(url("http://x/a/..").filename().is_err());
        assert!(url("http://x/a/.").filename().is_err());
    }

    // ---------- base64 ----------

    #[test]
    fn base64_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn base64_one_byte() {
        assert_eq!(base64_encode(b"f"), "Zg==");
    }

    #[test]
    fn base64_two_bytes() {
        assert_eq!(base64_encode(b"fo"), "Zm8=");
    }

    #[test]
    fn base64_three_bytes() {
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn base64_credential_string() {
        assert_eq!(base64_encode(b"admin:secret"), "YWRtaW46c2VjcmV0");
    }

    #[test]
    fn base64_encodes_bytes_that_are_not_text() {
        // A password is bytes, and the encoder must not care.
        assert_eq!(base64_encode(&[0xff, 0xfe, 0xfd]), "//79");
    }

    // ---------- determine_method ----------

    #[test]
    fn determine_method_default_is_get() {
        let c = Config::new();
        assert_eq!(determine_method(&c), b"GET".to_vec());
    }

    #[test]
    fn determine_method_data_implies_post() {
        let mut c = Config::new();
        c.data = Some(b"x=1".to_vec());
        assert_eq!(determine_method(&c), b"POST".to_vec());
    }

    #[test]
    fn determine_method_explicit_method_wins_over_data() {
        let mut c = Config::new();
        c.data = Some(b"x=1".to_vec());
        c.method = Some(b"put".to_vec());
        assert_eq!(determine_method(&c), b"PUT".to_vec());
    }

    // ---------- build_request ----------

    #[test]
    fn build_request_get_minimal() {
        let req = build_request(b"GET", &url("http://example.com/path"), &[], None, None);
        assert!(req.starts_with(b"GET /path HTTP/1.1\r\n"));
        assert!(find_subsequence(&req, b"Host: example.com\r\n").is_some());
        assert!(find_subsequence(&req, b"Connection: close\r\n").is_some());
        // Empty body still terminates with a blank line.
        assert!(req.ends_with(b"\r\n\r\n"));
    }

    #[test]
    fn build_request_post_adds_content_length() {
        let req = build_request(
            b"POST",
            &url("http://example.com/"),
            &[],
            Some(b"hello"),
            None,
        );
        assert!(find_subsequence(&req, b"Content-Length: 5\r\n").is_some());
        assert!(
            find_subsequence(&req, b"Content-Type: application/x-www-form-urlencoded\r\n")
                .is_some()
        );
        assert!(req.ends_with(b"hello"));
    }

    #[test]
    fn build_request_custom_content_type_suppresses_default() {
        let headers = vec![(b"Content-Type".to_vec(), b"application/json".to_vec())];
        let req = build_request(
            b"POST",
            &url("http://example.com/"),
            &headers,
            Some(b"{}"),
            None,
        );
        assert!(find_subsequence(&req, b"Content-Type: application/json\r\n").is_some());
        assert!(
            find_subsequence(&req, b"Content-Type: application/x-www-form-urlencoded").is_none()
        );
    }

    #[test]
    fn build_request_basic_auth_header_present() {
        let req = build_request(
            b"GET",
            &url("http://example.com/"),
            &[],
            None,
            Some(b"admin:secret"),
        );
        assert!(find_subsequence(&req, b"Authorization: Basic YWRtaW46c2VjcmV0\r\n").is_some());
    }

    #[test]
    fn build_request_includes_query() {
        let req = build_request(
            b"GET",
            &url("http://example.com/search?q=hi"),
            &[],
            None,
            None,
        );
        assert!(req.starts_with(b"GET /search?q=hi HTTP/1.1\r\n"));
    }

    // ---------- parse_status_line ----------

    #[test]
    fn status_line_basic() {
        let (code, text) = parse_status_line(b"HTTP/1.1 200 OK").unwrap();
        assert_eq!(code, 200);
        assert_eq!(text, b"OK".to_vec());
    }

    #[test]
    fn status_line_multiword_text() {
        let (code, text) = parse_status_line(b"HTTP/1.1 404 Not Found").unwrap();
        assert_eq!(code, 404);
        assert_eq!(text, b"Not Found".to_vec());
    }

    #[test]
    fn status_line_no_reason_phrase() {
        let (code, text) = parse_status_line(b"HTTP/1.0 204").unwrap();
        assert_eq!(code, 204);
        assert_eq!(text, Vec::<u8>::new());
    }

    #[test]
    fn status_line_trailing_cr_is_not_part_of_the_reason() {
        let (code, text) = parse_status_line(b"HTTP/1.1 200 OK\r").unwrap();
        assert_eq!(code, 200);
        assert_eq!(text, b"OK".to_vec());
    }

    #[test]
    fn status_line_malformed_errors() {
        assert!(parse_status_line(b"garbage").is_err());
        assert!(parse_status_line(b"HTTP/1.1 not-a-number OK").is_err());
    }

    // ---------- HttpResponse helpers ----------

    fn mk_resp(headers: Vec<(&str, &str)>) -> HttpResponse {
        HttpResponse {
            status_code: 200,
            status_text: b"OK".to_vec(),
            headers: headers
                .into_iter()
                .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
                .collect(),
            body: Vec::new(),
        }
    }

    #[test]
    fn response_header_value_case_insensitive() {
        let r = mk_resp(vec![("Content-Type", "text/html")]);
        assert_eq!(r.header_value(b"content-type"), Some(&b"text/html"[..]));
        assert_eq!(r.header_value(b"CONTENT-TYPE"), Some(&b"text/html"[..]));
    }

    #[test]
    fn response_header_value_missing_is_none() {
        let r = mk_resp(vec![]);
        assert_eq!(r.header_value(b"Location"), None);
    }

    #[test]
    fn response_content_length_parsed() {
        let r = mk_resp(vec![("Content-Length", "42")]);
        assert_eq!(r.content_length(), Some(42));
    }

    #[test]
    fn response_content_length_garbage_is_none() {
        let r = mk_resp(vec![("Content-Length", "abc")]);
        assert_eq!(r.content_length(), None);
    }

    #[test]
    fn response_is_chunked_detected() {
        assert!(mk_resp(vec![("Transfer-Encoding", "chunked")]).is_chunked());
        assert!(mk_resp(vec![("Transfer-Encoding", "gzip, chunked")]).is_chunked());
        assert!(mk_resp(vec![("Transfer-Encoding", "CHUNKED")]).is_chunked());
    }

    #[test]
    fn response_is_chunked_false_when_absent() {
        assert!(!mk_resp(vec![]).is_chunked());
    }

    #[test]
    fn response_location_field() {
        let r = mk_resp(vec![("Location", "http://other/")]);
        assert_eq!(r.location(), Some(&b"http://other/"[..]));
    }

    // ---------- find_subsequence ----------

    #[test]
    fn find_subsequence_found() {
        assert_eq!(find_subsequence(b"hello world", b"world"), Some(6));
    }

    #[test]
    fn find_subsequence_missing() {
        assert_eq!(find_subsequence(b"hello", b"xyz"), None);
    }

    #[test]
    fn find_subsequence_at_start() {
        assert_eq!(find_subsequence(b"abcdef", b"abc"), Some(0));
    }

    #[test]
    fn find_subsequence_longer_than_haystack_is_none() {
        // `windows(n)` panics when n exceeds the slice, so this is guarded.
        assert_eq!(find_subsequence(b"ab", b"abcdef"), None);
        assert_eq!(find_subsequence(b"", b"a"), None);
    }

    // ---------- try_parse_chunk ----------

    #[test]
    fn chunk_parse_simple_size() {
        match try_parse_chunk(b"a\r\n0123456789\r\n") {
            ChunkParse::Chunk { size, consumed } => {
                assert_eq!(size, 0xa);
                assert_eq!(consumed, 3); // "a\r\n" = 3 bytes
            }
            _ => panic!("expected Chunk"),
        }
    }

    #[test]
    fn chunk_parse_with_extension() {
        match try_parse_chunk(b"ff;name=value\r\nDATA") {
            ChunkParse::Chunk { size, .. } => assert_eq!(size, 0xff),
            _ => panic!("expected Chunk"),
        }
    }

    #[test]
    fn chunk_parse_needs_more() {
        assert!(matches!(try_parse_chunk(b"ab"), ChunkParse::NeedMore));
    }

    #[test]
    fn chunk_parse_invalid_hex() {
        match try_parse_chunk(b"zz\r\n") {
            ChunkParse::Invalid(_) => {}
            _ => panic!("expected Invalid"),
        }
    }

    // ---------- format_size ----------

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(500), "500 B");
    }

    #[test]
    fn format_size_kilobytes() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
    }

    #[test]
    fn format_size_megabytes() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn format_size_gigabytes() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
    }

    // ---------- resolve_redirect ----------

    #[test]
    fn redirect_absolute_url_used_verbatim() {
        let base = url("http://a.com/x");
        assert_eq!(
            resolve_redirect(&base, b"http://b.com/y"),
            b"http://b.com/y".to_vec()
        );
    }

    #[test]
    fn redirect_absolute_path_keeps_origin() {
        let base = url("http://a.com/x");
        assert_eq!(
            resolve_redirect(&base, b"/new"),
            b"http://a.com:80/new".to_vec()
        );
    }

    #[test]
    fn redirect_relative_appends_to_current_dir() {
        let base = url("http://a.com/dir/page");
        assert_eq!(
            resolve_redirect(&base, b"other"),
            b"http://a.com:80/dir/other".to_vec()
        );
    }

    #[test]
    fn redirect_carries_a_byte_that_is_not_text() {
        let base = url("http://a.com/dir/page");
        let next = resolve_redirect(&base, b"caf\xe9.html");
        assert!(next.ends_with(b"/dir/caf\xe9.html"));
        // And it survives re-parsing, which is what `-O` reads.
        let u = ParsedUrl::parse(&next).unwrap();
        assert_eq!(u.path, b"/dir/caf\xe9.html".to_vec());
    }
}
