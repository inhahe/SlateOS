//! Slate OS HTTP File Download Utility
//!
//! Downloads files from HTTP URLs using the kernel's TCP syscall interface.
//! Supports chunked transfer encoding, redirect following, resume, and
//! progress display.
//!
//! # Usage
//!
//! ```text
//! wget <url>                        Download URL to local file
//! wget -O <file> <url>              Save to specific filename
//! wget -O - <url>                   Output to stdout
//! wget -q <url>                     Quiet mode (no progress)
//! wget -v <url>                     Verbose (show headers)
//! wget -c <url>                     Resume partial download
//! wget --header "Name: Value" <url> Add custom header
//! wget --tries 5 <url>              Retry up to 5 times
//! wget --timeout 30 <url>           Set timeout in seconds
//! wget --max-redirect 5 <url>       Limit redirects
//! wget --user-agent "Bot/1.0" <url> Custom User-Agent
//! ```

#![deny(clippy::all)]
#![allow(clippy::manual_range_contains)] // clearer as explicit comparisons in some spots

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::process;
use std::time::Instant;

// ============================================================================
// Syscall interface
// ============================================================================

const SYS_TCP_CONNECT: u64 = 800;
const SYS_TCP_SEND: u64 = 801;
const SYS_TCP_RECV: u64 = 802;
const SYS_TCP_CLOSE: u64 = 803;
const SYS_DNS_RESOLVE: u64 = 820;

/// Perform a 3-argument syscall via the `syscall` instruction.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall number.
    // The `syscall` instruction is the defined kernel entry point on x86-64.
    // rcx and r11 are marked as clobbered per the hardware specification.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Perform a 1-argument syscall (close only needs the handle).
#[cfg(target_arch = "x86_64")]
unsafe fn syscall1(nr: u64, a1: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures the handle argument is valid. The `syscall`
    // instruction is the defined kernel entry point. rcx and r11 are clobbered.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

// ============================================================================
// Syscall wrappers
// ============================================================================

/// Resolve a hostname to an IPv4 address via the kernel DNS resolver.
/// Returns the IP as a `u32` in network byte order on success.
fn dns_resolve(hostname: &str) -> Result<u32, WgetError> {
    let mut result_ip: u32 = 0;
    // SAFETY: We pass a valid pointer to the hostname bytes and their length,
    // plus a valid mutable pointer for the kernel to write the resolved IP into.
    // The kernel reads exactly `hostname.len()` bytes and writes exactly 4 bytes.
    let ret = unsafe {
        syscall3(
            SYS_DNS_RESOLVE,
            hostname.as_ptr() as u64,
            hostname.len() as u64,
            &mut result_ip as *mut u32 as u64,
        )
    };
    if ret < 0 {
        return Err(WgetError::DnsFailure(hostname.to_string()));
    }
    Ok(result_ip)
}

/// Open a TCP connection to the given IP (network byte order) and port.
/// Returns a handle on success.
fn tcp_connect(ip: u32, port: u16) -> Result<u64, WgetError> {
    // SAFETY: We pass a valid IP and port. The kernel returns a handle (>= 0)
    // or a negative error code. No pointers are involved.
    let ret = unsafe { syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), 0) };
    if ret < 0 {
        return Err(WgetError::ConnectionRefused);
    }
    Ok(ret as u64)
}

/// Send data on a TCP connection. Returns the number of bytes actually sent.
fn tcp_send(handle: u64, data: &[u8]) -> Result<usize, WgetError> {
    // SAFETY: We pass a valid handle and a pointer to a byte buffer with its
    // correct length. The kernel reads up to `data.len()` bytes from the buffer.
    let ret = unsafe {
        syscall3(
            SYS_TCP_SEND,
            handle,
            data.as_ptr() as u64,
            data.len() as u64,
        )
    };
    if ret < 0 {
        return Err(WgetError::SendFailed);
    }
    Ok(ret as usize)
}

/// Send all bytes, looping until the entire buffer is transmitted.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), WgetError> {
    let mut offset = 0;
    while offset < data.len() {
        let sent = tcp_send(handle, &data[offset..])?;
        if sent == 0 {
            return Err(WgetError::SendFailed);
        }
        offset = offset.checked_add(sent).ok_or(WgetError::SendFailed)?;
    }
    Ok(())
}

/// Receive data from a TCP connection. Returns 0 when the peer has closed.
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, WgetError> {
    // SAFETY: We pass a valid handle and a mutable buffer pointer with its
    // correct length. The kernel writes at most `buf.len()` bytes into the buffer.
    let ret = unsafe {
        syscall3(
            SYS_TCP_RECV,
            handle,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(WgetError::RecvFailed);
    }
    Ok(ret as usize)
}

/// Close a TCP connection handle.
fn tcp_close(handle: u64) {
    // SAFETY: We pass a valid handle. The kernel deallocates internal state.
    // Ignoring the return value is safe: the handle becomes invalid regardless.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
enum WgetError {
    DnsFailure(String),
    ConnectionRefused,
    SendFailed,
    RecvFailed,
    #[allow(dead_code)] // Will be used when kernel supports TCP timeout syscall.
    Timeout,
    InvalidUrl(String),
    HttpError(u16, String),
    TooManyRedirects,
    ChunkedDecodeError(String),
    IoError(io::Error),
    InvalidResponse(String),
}

impl std::fmt::Display for WgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DnsFailure(host) => write!(f, "failed to resolve host '{host}'"),
            Self::ConnectionRefused => write!(f, "connection refused"),
            Self::SendFailed => write!(f, "failed to send data"),
            Self::RecvFailed => write!(f, "failed to receive data"),
            Self::Timeout => write!(f, "connection timed out"),
            Self::InvalidUrl(msg) => write!(f, "invalid URL: {msg}"),
            Self::HttpError(code, reason) => write!(f, "HTTP error {code}: {reason}"),
            Self::TooManyRedirects => write!(f, "too many redirects"),
            Self::ChunkedDecodeError(msg) => write!(f, "chunked decode error: {msg}"),
            Self::IoError(e) => write!(f, "I/O error: {e}"),
            Self::InvalidResponse(msg) => write!(f, "invalid HTTP response: {msg}"),
        }
    }
}

impl From<io::Error> for WgetError {
    fn from(e: io::Error) -> Self {
        Self::IoError(e)
    }
}

// ============================================================================
// URL parsing
// ============================================================================

/// Parsed components of an HTTP URL.
struct ParsedUrl {
    host: String,
    port: u16,
    path: String,
}

/// Parse `http://host[:port]/path` into its components.
/// Only HTTP is supported (no HTTPS).
fn parse_url(url: &str) -> Result<ParsedUrl, WgetError> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| WgetError::InvalidUrl("only http:// URLs are supported".to_string()))?;

    // Split host+port from path at the first '/'.
    let (host_port, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };

    if host_port.is_empty() {
        return Err(WgetError::InvalidUrl("empty hostname".to_string()));
    }

    // Split host from port.
    let (host, port) = if let Some(colon_idx) = host_port.rfind(':') {
        let port_str = &host_port[colon_idx + 1..];
        let port: u16 = port_str.parse().map_err(|_| {
            WgetError::InvalidUrl(format!("invalid port number '{port_str}'"))
        })?;
        (&host_port[..colon_idx], port)
    } else {
        (host_port, 80)
    };

    Ok(ParsedUrl {
        host: host.to_string(),
        port,
        path: path.to_string(),
    })
}

/// Extract the filename from a URL path for use as the default output filename.
fn filename_from_url(url: &ParsedUrl) -> String {
    let path = &url.path;
    // Take everything after the last '/'.
    let basename = path.rsplit('/').next().unwrap_or("");
    // Strip query string.
    let name = basename.split('?').next().unwrap_or("");
    if name.is_empty() {
        "index.html".to_string()
    } else {
        name.to_string()
    }
}

// ============================================================================
// IP address formatting
// ============================================================================

/// Format a u32 IP (network byte order) as a dotted-quad string.
fn ip_to_string(ip: u32) -> String {
    let bytes = ip.to_be_bytes();
    format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
}

// ============================================================================
// Human-readable size formatting
// ============================================================================

/// Format a byte count into a human-readable string (e.g. "1.2M").
fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;

    if bytes >= GIB {
        format!("{:.1}G", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1}M", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1}K", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes}")
    }
}

/// Format a transfer speed in bytes/sec into a human-readable string.
fn format_speed(bytes_per_sec: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    if bytes_per_sec >= GIB {
        format!("{:.2}GB/s", bytes_per_sec / GIB)
    } else if bytes_per_sec >= MIB {
        format!("{:.2}MB/s", bytes_per_sec / MIB)
    } else if bytes_per_sec >= KIB {
        format!("{:.2}KB/s", bytes_per_sec / KIB)
    } else {
        format!("{:.0}B/s", bytes_per_sec)
    }
}

// ============================================================================
// Progress display
// ============================================================================

/// Width of the progress bar (number of characters between brackets).
const PROGRESS_BAR_WIDTH: usize = 20;

/// Render and print a progress line to stderr.
fn print_progress(
    filename: &str,
    downloaded: u64,
    total: Option<u64>,
    start_time: Instant,
) {
    let elapsed = start_time.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        downloaded as f64 / elapsed
    } else {
        0.0
    };

    let stderr = io::stderr();
    let mut err = stderr.lock();

    match total {
        Some(total_bytes) if total_bytes > 0 => {
            let pct = if total_bytes > 0 {
                (downloaded as f64 / total_bytes as f64 * 100.0).min(100.0)
            } else {
                0.0
            };
            let filled = (pct / 100.0 * PROGRESS_BAR_WIDTH as f64) as usize;
            let empty = PROGRESS_BAR_WIDTH.saturating_sub(filled);

            let remaining_bytes = total_bytes.saturating_sub(downloaded);
            let eta = if speed > 0.0 {
                remaining_bytes as f64 / speed
            } else {
                0.0
            };
            let eta_str = if eta < 60.0 {
                format!("{:.0}s", eta)
            } else if eta < 3600.0 {
                format!("{:.0}m {:.0}s", (eta / 60.0).floor(), eta % 60.0)
            } else {
                format!("{:.0}h {:.0}m", (eta / 3600.0).floor(), (eta % 3600.0) / 60.0)
            };

            let bar_filled = "=".repeat(filled.saturating_sub(1));
            let bar_tip = ">";
            let bar_empty = " ".repeat(empty);
            let _ = write!(
                err,
                "\r{:<20} {:>3.0}%[{}{}{}] {} {}  eta {}   ",
                truncate_name(filename, 20),
                pct,
                bar_filled,
                bar_tip,
                bar_empty,
                format_size(downloaded),
                format_speed(speed),
                eta_str,
            );
        }
        _ => {
            // Unknown total: just show downloaded amount and speed.
            let _ = write!(
                err,
                "\r{:<20}  [ <=>                  ] {} {}   ",
                truncate_name(filename, 20),
                format_size(downloaded),
                format_speed(speed),
            );
        }
    }
    let _ = err.flush();
}

/// Truncate a filename to fit in `max_len` characters, replacing the middle
/// with ".." if necessary.
fn truncate_name(name: &str, max_len: usize) -> String {
    if name.len() <= max_len {
        return name.to_string();
    }
    if max_len < 5 {
        return name[..max_len].to_string();
    }
    let prefix_len = (max_len - 2) / 2;
    let suffix_len = max_len - 2 - prefix_len;
    format!(
        "{}..{}",
        &name[..prefix_len],
        &name[name.len() - suffix_len..]
    )
}

// ============================================================================
// HTTP response parsing
// ============================================================================

/// Parsed HTTP status line.
struct HttpStatus {
    code: u16,
    reason: String,
}

/// Parsed HTTP response headers.
struct HttpResponse {
    status: HttpStatus,
    headers: Vec<(String, String)>,
    /// Byte offset where the body begins in the raw response buffer.
    body_offset: usize,
}

/// Parse the HTTP response header section from a raw byte buffer.
/// The buffer must contain at least the complete header section
/// (terminated by \r\n\r\n).
fn parse_http_response(data: &[u8]) -> Result<HttpResponse, WgetError> {
    // Find the end of headers.
    let header_end = find_header_end(data)
        .ok_or_else(|| WgetError::InvalidResponse("incomplete headers".to_string()))?;

    let header_bytes = &data[..header_end];
    let header_text = String::from_utf8_lossy(header_bytes);

    let mut lines = header_text.split("\r\n");

    // Parse status line: "HTTP/1.x NNN reason"
    let status_line = lines
        .next()
        .ok_or_else(|| WgetError::InvalidResponse("empty response".to_string()))?;

    let status = parse_status_line(status_line)?;

    // Parse headers.
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((
                name.trim().to_ascii_lowercase(),
                value.trim().to_string(),
            ));
        }
    }

    // Body starts after \r\n\r\n.
    let body_offset = header_end + 4;

    Ok(HttpResponse {
        status,
        headers,
        body_offset,
    })
}

/// Parse an HTTP status line like "HTTP/1.1 200 OK".
fn parse_status_line(line: &str) -> Result<HttpStatus, WgetError> {
    let mut parts = line.splitn(3, ' ');
    let _version = parts
        .next()
        .ok_or_else(|| WgetError::InvalidResponse("missing HTTP version".to_string()))?;
    let code_str = parts
        .next()
        .ok_or_else(|| WgetError::InvalidResponse("missing status code".to_string()))?;
    let reason = parts.next().unwrap_or("").to_string();

    let code: u16 = code_str.parse().map_err(|_| {
        WgetError::InvalidResponse(format!("invalid status code '{code_str}'"))
    })?;

    Ok(HttpStatus { code, reason })
}

/// Find the position of \r\n\r\n in the buffer (marks end of headers).
fn find_header_end(data: &[u8]) -> Option<usize> {
    if data.len() < 4 {
        return None;
    }
    data.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Look up a header value by (lowercase) name.
fn get_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

// ============================================================================
// Chunked transfer decoding
// ============================================================================

/// State machine for decoding chunked transfer encoding.
///
/// Chunked encoding format:
///   <hex-size>\r\n
///   <data of that size>\r\n
///   ... repeat ...
///   0\r\n
///   \r\n
struct ChunkedDecoder {
    /// Bytes remaining in the current chunk.
    remaining: usize,
    /// Whether we have read the chunk size for the current chunk.
    in_chunk: bool,
    /// Whether the final zero-length chunk has been seen.
    finished: bool,
    /// Internal buffer for accumulating chunk-size lines.
    line_buf: Vec<u8>,
}

impl ChunkedDecoder {
    fn new() -> Self {
        Self {
            remaining: 0,
            in_chunk: false,
            finished: false,
            line_buf: Vec::new(),
        }
    }

    /// Feed raw bytes from the network and return decoded body bytes.
    ///
    /// This may return fewer bytes than `input.len()` because some of the
    /// input is chunk framing (size lines, trailing \r\n). It may also
    /// return zero bytes if the input is entirely framing.
    fn decode(&mut self, input: &[u8]) -> Result<Vec<u8>, WgetError> {
        let mut output = Vec::new();
        let mut pos = 0;

        while pos < input.len() && !self.finished {
            if self.in_chunk {
                // We are in the middle of a data chunk. Copy up to `remaining`
                // bytes from input to output.
                let available = input.len() - pos;
                let to_copy = available.min(self.remaining);
                output.extend_from_slice(&input[pos..pos + to_copy]);
                pos += to_copy;
                self.remaining -= to_copy;

                if self.remaining == 0 {
                    // Chunk data exhausted; expect trailing \r\n.
                    self.in_chunk = false;
                    // Skip the trailing \r\n if present in remaining input.
                    if pos + 2 <= input.len()
                        && input.get(pos) == Some(&b'\r')
                        && input.get(pos + 1) == Some(&b'\n')
                    {
                        pos += 2;
                    } else if pos < input.len() && input.get(pos) == Some(&b'\r') {
                        // Only \r received; the \n will come in the next recv.
                        pos += 1;
                    }
                    // If neither, the \r\n spans a recv boundary; we handle
                    // it on the next call.
                }
            } else {
                // We need to read a chunk-size line.
                // Accumulate bytes into line_buf until we see \r\n.
                while pos < input.len() {
                    let byte = input[pos];
                    pos += 1;
                    self.line_buf.push(byte);

                    if self.line_buf.len() >= 2
                        && self.line_buf[self.line_buf.len() - 2] == b'\r'
                        && self.line_buf[self.line_buf.len() - 1] == b'\n'
                    {
                        // Got a complete line.
                        // Remove trailing \r\n.
                        let len = self.line_buf.len() - 2;
                        let line = &self.line_buf[..len];

                        // The chunk size line may contain extensions after a
                        // semicolon; ignore them.
                        let size_str = String::from_utf8_lossy(line);
                        let hex_part = size_str.split(';').next().unwrap_or("");
                        let hex_trimmed = hex_part.trim();

                        if hex_trimmed.is_empty() {
                            // Blank line between chunks or leading CRLF;
                            // just skip it.
                            self.line_buf.clear();
                            continue;
                        }

                        let chunk_size =
                            usize::from_str_radix(hex_trimmed, 16).map_err(|_| {
                                WgetError::ChunkedDecodeError(format!(
                                    "invalid chunk size '{hex_trimmed}'"
                                ))
                            })?;

                        self.line_buf.clear();

                        if chunk_size == 0 {
                            self.finished = true;
                        } else {
                            self.remaining = chunk_size;
                            self.in_chunk = true;
                        }
                        break;
                    }

                    // Guard against absurdly long chunk-size lines.
                    if self.line_buf.len() > 256 {
                        return Err(WgetError::ChunkedDecodeError(
                            "chunk size line too long".to_string(),
                        ));
                    }
                }
            }
        }

        Ok(output)
    }

    fn is_finished(&self) -> bool {
        self.finished
    }
}

// ============================================================================
// CLI argument parsing
// ============================================================================

/// Verbosity level.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verbosity {
    Quiet,
    Normal,
    Verbose,
}

/// Parsed command-line options.
struct Options {
    url: String,
    output_file: Option<String>,
    output_stdout: bool,
    verbosity: Verbosity,
    resume: bool,
    custom_headers: Vec<(String, String)>,
    max_redirects: u32,
    #[allow(dead_code)] // Will be used when kernel supports TCP timeout syscall.
    timeout_secs: u64,
    tries: u32,
    user_agent: String,
}

fn print_usage() {
    let stderr = io::stderr();
    let mut err = stderr.lock();
    let _ = writeln!(err, "Usage: wget [OPTIONS] <URL>");
    let _ = writeln!(err);
    let _ = writeln!(err, "Download files from HTTP servers.");
    let _ = writeln!(err);
    let _ = writeln!(err, "Options:");
    let _ = writeln!(err, "  -O <file>              Save to specific file ('-' for stdout)");
    let _ = writeln!(err, "  --output-document <f>  Same as -O");
    let _ = writeln!(err, "  -q, --quiet            No output except errors");
    let _ = writeln!(err, "  -v, --verbose          Show request/response headers");
    let _ = writeln!(err, "  --no-verbose           Default progress-only mode");
    let _ = writeln!(err, "  -c, --continue         Resume a partial download");
    let _ = writeln!(err, "  --header <name:value>  Add a custom HTTP header");
    let _ = writeln!(err, "  --max-redirect <n>     Maximum redirects (default: 10)");
    let _ = writeln!(err, "  --timeout <secs>       Connection/read timeout");
    let _ = writeln!(err, "  --tries <n>            Number of retries (default: 3)");
    let _ = writeln!(err, "  --user-agent <string>  Custom User-Agent header");
    let _ = writeln!(err, "  -h, --help             Show this help message");
}

fn parse_args() -> Result<Options, WgetError> {
    let argv: Vec<String> = env::args().collect();

    if argv.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut url: Option<String> = None;
    let mut output_file: Option<String> = None;
    let mut output_stdout = false;
    let mut verbosity = Verbosity::Normal;
    let mut resume = false;
    let mut custom_headers: Vec<(String, String)> = Vec::new();
    let mut max_redirects: u32 = 10;
    let mut timeout_secs: u64 = 30;
    let mut tries: u32 = 3;
    let mut user_agent = String::from("SlateOS-wget/0.1");

    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-O" | "--output-document" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    WgetError::InvalidUrl("-O requires a filename argument".to_string())
                })?;
                if val == "-" {
                    output_stdout = true;
                } else {
                    output_file = Some(val.clone());
                }
            }
            "-q" | "--quiet" => {
                verbosity = Verbosity::Quiet;
            }
            "-v" | "--verbose" => {
                verbosity = Verbosity::Verbose;
            }
            "--no-verbose" => {
                verbosity = Verbosity::Normal;
            }
            "-c" | "--continue" => {
                resume = true;
            }
            "--header" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    WgetError::InvalidUrl("--header requires a value".to_string())
                })?;
                if let Some((name, value)) = val.split_once(':') {
                    custom_headers.push((name.trim().to_string(), value.trim().to_string()));
                } else {
                    return Err(WgetError::InvalidUrl(format!(
                        "invalid header format '{val}' (expected Name: Value)"
                    )));
                }
            }
            "--max-redirect" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    WgetError::InvalidUrl("--max-redirect requires a number".to_string())
                })?;
                max_redirects = val.parse().map_err(|_| {
                    WgetError::InvalidUrl(format!("invalid redirect count '{val}'"))
                })?;
            }
            "--timeout" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    WgetError::InvalidUrl("--timeout requires a number".to_string())
                })?;
                timeout_secs = val.parse().map_err(|_| {
                    WgetError::InvalidUrl(format!("invalid timeout '{val}'"))
                })?;
            }
            "--tries" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    WgetError::InvalidUrl("--tries requires a number".to_string())
                })?;
                tries = val.parse().map_err(|_| {
                    WgetError::InvalidUrl(format!("invalid retry count '{val}'"))
                })?;
            }
            "--user-agent" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    WgetError::InvalidUrl("--user-agent requires a string".to_string())
                })?;
                user_agent = val.clone();
            }
            s if s.starts_with('-') => {
                return Err(WgetError::InvalidUrl(format!("unknown option '{s}'")));
            }
            _ => {
                // Positional argument: the URL.
                if url.is_some() {
                    return Err(WgetError::InvalidUrl(
                        "multiple URLs not supported".to_string(),
                    ));
                }
                url = Some(arg.clone());
            }
        }
        i += 1;
    }

    let url = url.ok_or_else(|| WgetError::InvalidUrl("no URL specified".to_string()))?;

    Ok(Options {
        url,
        output_file,
        output_stdout,
        verbosity,
        resume,
        custom_headers,
        max_redirects,
        timeout_secs,
        tries,
        user_agent,
    })
}

// ============================================================================
// HTTP request building
// ============================================================================

/// Build an HTTP/1.1 GET request string.
fn build_request(
    url: &ParsedUrl,
    opts: &Options,
    resume_offset: u64,
) -> String {
    let mut req = format!("GET {} HTTP/1.1\r\n", url.path);
    req.push_str(&format!("Host: {}\r\n", url.host));
    req.push_str(&format!("User-Agent: {}\r\n", opts.user_agent));
    req.push_str("Accept: */*\r\n");
    req.push_str("Connection: close\r\n");

    if resume_offset > 0 {
        req.push_str(&format!("Range: bytes={resume_offset}-\r\n"));
    }

    for (name, value) in &opts.custom_headers {
        req.push_str(&format!("{name}: {value}\r\n"));
    }

    req.push_str("\r\n");
    req
}

// ============================================================================
// Core download logic
// ============================================================================

/// Read the full HTTP response headers by receiving data until \r\n\r\n is
/// found. Returns the raw buffer containing headers and possibly the start
/// of the body.
fn recv_headers(handle: u64) -> Result<Vec<u8>, WgetError> {
    let mut buf = Vec::with_capacity(4096);
    let mut recv_buf = [0u8; 8192];

    loop {
        let n = tcp_recv(handle, &mut recv_buf)?;
        if n == 0 {
            // Connection closed before headers were complete.
            if buf.is_empty() {
                return Err(WgetError::InvalidResponse("empty response".to_string()));
            }
            break;
        }
        buf.extend_from_slice(&recv_buf[..n]);

        // Check if we have the complete header section.
        if find_header_end(&buf).is_some() {
            break;
        }

        // Guard against absurdly large headers.
        if buf.len() > 64 * 1024 {
            return Err(WgetError::InvalidResponse(
                "response headers exceed 64 KiB".to_string(),
            ));
        }
    }

    Ok(buf)
}

/// Resolve a possibly-relative redirect URL against the current URL.
fn resolve_redirect(current_url: &str, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        return location.to_string();
    }

    // Relative URL: combine with current host.
    if let Ok(parsed) = parse_url(current_url) {
        if location.starts_with('/') {
            // Absolute path on same host.
            if parsed.port == 80 {
                format!("http://{}{}", parsed.host, location)
            } else {
                format!("http://{}:{}{}", parsed.host, parsed.port, location)
            }
        } else {
            // Relative path: append to current directory.
            let dir = match parsed.path.rfind('/') {
                Some(idx) => &parsed.path[..=idx],
                None => "/",
            };
            if parsed.port == 80 {
                format!("http://{}{}{}", parsed.host, dir, location)
            } else {
                format!("http://{}:{}{}{}", parsed.host, parsed.port, dir, location)
            }
        }
    } else {
        // Cannot parse current URL; just use location as-is.
        location.to_string()
    }
}

/// Perform a single HTTP GET request and download the response body.
///
/// Returns `Ok(Some(redirect_url))` if the server responded with a redirect,
/// or `Ok(None)` on successful download.
fn do_request(
    url_str: &str,
    opts: &Options,
    output_filename: &str,
    resume_offset: u64,
) -> Result<Option<String>, WgetError> {
    let url = parse_url(url_str)?;

    // DNS resolution.
    if opts.verbosity != Verbosity::Quiet {
        eprint!("Resolving {}... ", url.host);
    }
    let ip = dns_resolve(&url.host)?;
    if opts.verbosity != Verbosity::Quiet {
        eprintln!("{}", ip_to_string(ip));
    }

    // TCP connect.
    if opts.verbosity != Verbosity::Quiet {
        eprint!(
            "Connecting to {} ({}):{} ... ",
            url.host,
            ip_to_string(ip),
            url.port
        );
    }
    let handle = tcp_connect(ip, url.port)?;
    if opts.verbosity != Verbosity::Quiet {
        eprintln!("connected.");
    }

    // Build and send the HTTP request.
    let request = build_request(&url, opts, resume_offset);

    if opts.verbosity == Verbosity::Verbose {
        eprintln!("---request begin---");
        for line in request.lines() {
            eprintln!("  {line}");
        }
        eprintln!("---request end---");
    }

    if let Err(e) = tcp_send_all(handle, request.as_bytes()) {
        tcp_close(handle);
        return Err(e);
    }

    // Receive headers.
    let raw_response = match recv_headers(handle) {
        Ok(buf) => buf,
        Err(e) => {
            tcp_close(handle);
            return Err(e);
        }
    };

    let response = match parse_http_response(&raw_response) {
        Ok(r) => r,
        Err(e) => {
            tcp_close(handle);
            return Err(e);
        }
    };

    if opts.verbosity == Verbosity::Verbose {
        eprintln!("---response begin---");
        eprintln!(
            "  HTTP {} {}",
            response.status.code, response.status.reason
        );
        for (name, value) in &response.headers {
            eprintln!("  {name}: {value}");
        }
        eprintln!("---response end---");
    }

    // Handle redirects (3xx).
    if response.status.code >= 300 && response.status.code < 400 {
        tcp_close(handle);
        if let Some(location) = get_header(&response.headers, "location") {
            let redirect_url = resolve_redirect(url_str, location);
            if opts.verbosity != Verbosity::Quiet {
                eprintln!(
                    "HTTP request sent, awaiting response... {} {}",
                    response.status.code, response.status.reason
                );
                eprintln!("Location: {redirect_url} [following]");
            }
            return Ok(Some(redirect_url));
        }
        return Err(WgetError::HttpError(
            response.status.code,
            "redirect without Location header".to_string(),
        ));
    }

    // Check for non-success status.
    if response.status.code < 200 || response.status.code >= 300 {
        tcp_close(handle);
        // 416 Range Not Satisfiable: the file is already complete.
        if response.status.code == 416 && resume_offset > 0 {
            if opts.verbosity != Verbosity::Quiet {
                eprintln!("File already fully downloaded; nothing to do.");
            }
            return Ok(None);
        }
        return Err(WgetError::HttpError(
            response.status.code,
            response.status.reason,
        ));
    }

    if opts.verbosity != Verbosity::Quiet {
        eprintln!(
            "HTTP request sent, awaiting response... {} {}",
            response.status.code, response.status.reason
        );
    }

    // Determine content length and transfer encoding.
    let content_length: Option<u64> = get_header(&response.headers, "content-length")
        .and_then(|v| v.parse().ok());
    let content_type = get_header(&response.headers, "content-type")
        .unwrap_or("application/octet-stream");
    let is_chunked = get_header(&response.headers, "transfer-encoding")
        .map(|v| v.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false);

    let total_for_display = if resume_offset > 0 {
        content_length.map(|cl| cl.checked_add(resume_offset).unwrap_or(cl))
    } else {
        content_length
    };

    if opts.verbosity != Verbosity::Quiet {
        match total_for_display {
            Some(total) => {
                eprintln!(
                    "Length: {} ({}) [{}]",
                    total,
                    format_size(total),
                    content_type
                );
            }
            None => {
                eprintln!("Length: unspecified [{}]", content_type);
            }
        }
        if opts.output_stdout {
            eprintln!("Saving to: 'stdout'");
        } else {
            eprintln!("Saving to: '{output_filename}'");
        }
        eprintln!();
    }

    // Open output file or stdout.
    let result = download_body(
        handle,
        &raw_response[response.body_offset..],
        is_chunked,
        content_length,
        opts,
        output_filename,
        resume_offset,
        total_for_display,
    );

    tcp_close(handle);
    result?;
    Ok(None)
}

/// Download the HTTP response body, writing to a file or stdout.
///
/// `initial_body` contains any body bytes that were already received as part
/// of the header read.
#[allow(clippy::too_many_arguments)]
fn download_body(
    handle: u64,
    initial_body: &[u8],
    is_chunked: bool,
    content_length: Option<u64>,
    opts: &Options,
    output_filename: &str,
    resume_offset: u64,
    total_for_display: Option<u64>,
) -> Result<(), WgetError> {
    let mut writer: Box<dyn Write> = if opts.output_stdout {
        Box::new(io::stdout().lock())
    } else if resume_offset > 0 {
        Box::new(
            OpenOptions::new()
                .append(true)
                .create(true)
                .open(output_filename)?,
        )
    } else {
        Box::new(File::create(output_filename)?)
    };

    let start_time = Instant::now();
    let mut downloaded: u64 = resume_offset;
    let mut chunked_decoder = if is_chunked {
        Some(ChunkedDecoder::new())
    } else {
        None
    };

    // Process any body bytes already in the header buffer.
    if !initial_body.is_empty() {
        let body_data = if let Some(ref mut decoder) = chunked_decoder {
            decoder.decode(initial_body)?
        } else {
            initial_body.to_vec()
        };
        if !body_data.is_empty() {
            writer.write_all(&body_data)?;
            downloaded = downloaded
                .checked_add(body_data.len() as u64)
                .unwrap_or(downloaded);
        }
    }

    // Show initial progress.
    if opts.verbosity != Verbosity::Quiet && !opts.output_stdout {
        print_progress(output_filename, downloaded, total_for_display, start_time);
    }

    // Receive loop.
    let mut recv_buf = [0u8; 8192];

    loop {
        // Check if we are done (chunked decoder signaled end, or we have
        // received all content-length bytes).
        if let Some(ref decoder) = chunked_decoder {
            if decoder.is_finished() {
                break;
            }
        } else if let Some(cl) = content_length {
            let body_downloaded = downloaded.saturating_sub(resume_offset);
            if body_downloaded >= cl {
                break;
            }
        }

        let n = tcp_recv(handle, &mut recv_buf)?;
        if n == 0 {
            // Connection closed by peer.
            break;
        }

        let body_data = if let Some(ref mut decoder) = chunked_decoder {
            decoder.decode(&recv_buf[..n])?
        } else {
            recv_buf[..n].to_vec()
        };

        if !body_data.is_empty() {
            writer.write_all(&body_data)?;
            downloaded = downloaded
                .checked_add(body_data.len() as u64)
                .unwrap_or(downloaded);
        }

        if opts.verbosity != Verbosity::Quiet && !opts.output_stdout {
            print_progress(output_filename, downloaded, total_for_display, start_time);
        }
    }

    writer.flush()?;

    // Final progress line.
    if opts.verbosity != Verbosity::Quiet && !opts.output_stdout {
        let elapsed = start_time.elapsed().as_secs_f64();
        let body_bytes = downloaded.saturating_sub(resume_offset);
        eprintln!();
        eprintln!();
        eprintln!(
            "'{}' saved [{} in {:.1}s]",
            output_filename, format_size(body_bytes), elapsed
        );
    }

    Ok(())
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> Result<(), WgetError> {
    let opts = parse_args()?;

    let url = parse_url(&opts.url)?;
    let output_filename = if opts.output_stdout {
        String::from("-")
    } else if let Some(ref f) = opts.output_file {
        f.clone()
    } else {
        filename_from_url(&url)
    };

    // Determine resume offset from existing file size.
    let resume_offset = if opts.resume && !opts.output_stdout {
        match fs::metadata(&output_filename) {
            Ok(meta) => meta.len(),
            Err(_) => 0,
        }
    } else {
        0
    };

    // Retry loop.
    let mut current_url = opts.url.clone();
    let mut redirects_remaining = opts.max_redirects;

    for attempt in 1..=opts.tries {
        match do_request(&current_url, &opts, &output_filename, resume_offset) {
            Ok(None) => {
                // Download complete.
                return Ok(());
            }
            Ok(Some(redirect_url)) => {
                // Follow redirect.
                if redirects_remaining == 0 {
                    return Err(WgetError::TooManyRedirects);
                }
                redirects_remaining = redirects_remaining.saturating_sub(1);
                current_url = redirect_url;
                // Redirects don't count as a retry attempt.
                continue;
            }
            Err(e) => {
                if attempt < opts.tries {
                    if opts.verbosity != Verbosity::Quiet {
                        eprintln!(
                            "wget: attempt {attempt}/{}: {e}",
                            opts.tries
                        );
                        eprintln!("Retrying...");
                    }
                    // On retry, use the redirect chain so far, not the original URL,
                    // since we already resolved the redirects.
                    continue;
                }
                return Err(e);
            }
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("wget: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- URL parsing ---

    #[test]
    fn parse_simple_url() {
        let url = parse_url("http://example.com/index.html").unwrap();
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 80);
        assert_eq!(url.path, "/index.html");
    }

    #[test]
    fn parse_url_with_port() {
        let url = parse_url("http://example.com:8080/foo/bar").unwrap();
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 8080);
        assert_eq!(url.path, "/foo/bar");
    }

    #[test]
    fn parse_url_no_path() {
        let url = parse_url("http://example.com").unwrap();
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 80);
        assert_eq!(url.path, "/");
    }

    #[test]
    fn parse_url_root_path() {
        let url = parse_url("http://example.com/").unwrap();
        assert_eq!(url.host, "example.com");
        assert_eq!(url.path, "/");
    }

    #[test]
    fn parse_url_https_rejected() {
        assert!(parse_url("https://example.com/").is_err());
    }

    #[test]
    fn parse_url_no_scheme() {
        assert!(parse_url("example.com/index.html").is_err());
    }

    #[test]
    fn parse_url_empty_host() {
        assert!(parse_url("http:///path").is_err());
    }

    #[test]
    fn parse_url_invalid_port() {
        assert!(parse_url("http://example.com:abc/path").is_err());
    }

    #[test]
    fn parse_url_with_query() {
        let url = parse_url("http://example.com/search?q=test&lang=en").unwrap();
        assert_eq!(url.path, "/search?q=test&lang=en");
    }

    // --- Filename extraction ---

    #[test]
    fn filename_simple() {
        let url = parse_url("http://example.com/files/archive.tar.gz").unwrap();
        assert_eq!(filename_from_url(&url), "archive.tar.gz");
    }

    #[test]
    fn filename_index() {
        let url = parse_url("http://example.com/").unwrap();
        assert_eq!(filename_from_url(&url), "index.html");
    }

    #[test]
    fn filename_with_query() {
        let url = parse_url("http://example.com/file.zip?v=2").unwrap();
        assert_eq!(filename_from_url(&url), "file.zip");
    }

    #[test]
    fn filename_no_path() {
        let url = parse_url("http://example.com").unwrap();
        assert_eq!(filename_from_url(&url), "index.html");
    }

    // --- IP formatting ---

    #[test]
    fn ip_format_loopback() {
        // ip_to_string interprets its argument big-endian (to_be_bytes): the
        // most-significant byte is the first octet, matching the kernel's
        // network-byte-order u32 (from_be_bytes). So 0x7F000001 == 127.0.0.1.
        // (The earlier spurious `.to_be()` here reversed the octets — the same
        // test bug that was fixed in curl.)
        let ip = 0x7F000001_u32;
        assert_eq!(ip_to_string(ip), "127.0.0.1");
    }

    #[test]
    fn ip_format_zeros() {
        assert_eq!(ip_to_string(0), "0.0.0.0");
    }

    // --- Size formatting ---

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(512), "512");
    }

    #[test]
    fn format_size_kib() {
        assert_eq!(format_size(1536), "1.5K");
    }

    #[test]
    fn format_size_mib() {
        assert_eq!(format_size(1_572_864), "1.5M");
    }

    #[test]
    fn format_size_gib() {
        assert_eq!(format_size(1_610_612_736), "1.5G");
    }

    // --- Speed formatting ---

    #[test]
    fn format_speed_bytes() {
        assert_eq!(format_speed(500.0), "500B/s");
    }

    #[test]
    fn format_speed_kib() {
        assert_eq!(format_speed(10240.0), "10.00KB/s");
    }

    #[test]
    fn format_speed_mib() {
        assert_eq!(format_speed(5.0 * 1024.0 * 1024.0), "5.00MB/s");
    }

    // --- Name truncation ---

    #[test]
    fn truncate_short_name() {
        assert_eq!(truncate_name("file.txt", 20), "file.txt");
    }

    #[test]
    fn truncate_long_name() {
        let name = "very-long-filename-that-needs-truncating.tar.gz";
        let result = truncate_name(name, 20);
        assert!(result.len() <= 20);
        assert!(result.contains(".."));
    }

    #[test]
    fn truncate_exact_length() {
        let name = "exactly-twenty-chars";
        assert_eq!(truncate_name(name, 20), name);
    }

    // --- HTTP status line parsing ---

    #[test]
    fn parse_status_200() {
        let status = parse_status_line("HTTP/1.1 200 OK").unwrap();
        assert_eq!(status.code, 200);
        assert_eq!(status.reason, "OK");
    }

    #[test]
    fn parse_status_301() {
        let status = parse_status_line("HTTP/1.1 301 Moved Permanently").unwrap();
        assert_eq!(status.code, 301);
        assert_eq!(status.reason, "Moved Permanently");
    }

    #[test]
    fn parse_status_404() {
        let status = parse_status_line("HTTP/1.1 404 Not Found").unwrap();
        assert_eq!(status.code, 404);
        assert_eq!(status.reason, "Not Found");
    }

    #[test]
    fn parse_status_no_reason() {
        let status = parse_status_line("HTTP/1.0 204").unwrap();
        assert_eq!(status.code, 204);
        assert_eq!(status.reason, "");
    }

    #[test]
    fn parse_status_invalid() {
        assert!(parse_status_line("GARBAGE").is_err());
    }

    // --- Header end detection ---

    #[test]
    fn find_header_end_present() {
        let data = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let pos = find_header_end(data);
        // find_header_end returns the START index of the "\r\n\r\n" separator.
        // "HTTP/1.1 200 OK\r\n" is 17 bytes (0-16), "Content-Length: 5" is 17
        // bytes (17-33), so the blank-line "\r\n\r\n" begins at index 34.
        assert_eq!(pos, Some(34));
    }

    #[test]
    fn find_header_end_not_present() {
        let data = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n";
        assert!(find_header_end(data).is_none());
    }

    #[test]
    fn find_header_end_empty() {
        assert!(find_header_end(b"").is_none());
    }

    #[test]
    fn find_header_end_short() {
        assert!(find_header_end(b"\r\n").is_none());
    }

    // --- Full HTTP response parsing ---

    #[test]
    fn parse_response_simple() {
        let data = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Type: text/plain\r\n\r\nhello";
        let resp = parse_http_response(data).unwrap();
        assert_eq!(resp.status.code, 200);
        assert_eq!(resp.headers.len(), 2);
        assert_eq!(
            get_header(&resp.headers, "content-length"),
            Some("5")
        );
        assert_eq!(
            get_header(&resp.headers, "content-type"),
            Some("text/plain")
        );
        // body_offset = (start of "\r\n\r\n") + 4. The headers end with the
        // blank line beginning at index 60, so the body starts at 64. The
        // assertion below (&data[body_offset..] == b"hello") only holds at 64.
        assert_eq!(resp.body_offset, 64);
        assert_eq!(&data[resp.body_offset..], b"hello");
    }

    #[test]
    fn parse_response_chunked() {
        let data =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let resp = parse_http_response(data).unwrap();
        assert_eq!(resp.status.code, 200);
        let te = get_header(&resp.headers, "transfer-encoding").unwrap_or("");
        assert!(te.contains("chunked"));
    }

    #[test]
    fn parse_response_redirect() {
        let data = b"HTTP/1.1 301 Moved\r\nLocation: http://new.example.com/\r\n\r\n";
        let resp = parse_http_response(data).unwrap();
        assert_eq!(resp.status.code, 301);
        assert_eq!(
            get_header(&resp.headers, "location"),
            Some("http://new.example.com/")
        );
    }

    #[test]
    fn parse_response_no_headers() {
        let data = b"HTTP/1.1 200 OK\r\n\r\nbody";
        let resp = parse_http_response(data).unwrap();
        assert_eq!(resp.status.code, 200);
        assert!(resp.headers.is_empty());
    }

    // --- Header lookup ---

    #[test]
    fn get_header_found() {
        let headers = vec![
            ("content-type".to_string(), "text/html".to_string()),
            ("content-length".to_string(), "42".to_string()),
        ];
        assert_eq!(get_header(&headers, "content-type"), Some("text/html"));
        assert_eq!(get_header(&headers, "content-length"), Some("42"));
    }

    #[test]
    fn get_header_not_found() {
        let headers = vec![("content-type".to_string(), "text/html".to_string())];
        assert_eq!(get_header(&headers, "x-custom"), None);
    }

    // --- Chunked transfer decoding ---

    #[test]
    fn chunked_single_chunk() {
        let mut decoder = ChunkedDecoder::new();
        let input = b"5\r\nhello\r\n0\r\n\r\n";
        let output = decoder.decode(input).unwrap();
        assert_eq!(output, b"hello");
        assert!(decoder.is_finished());
    }

    #[test]
    fn chunked_multiple_chunks() {
        let mut decoder = ChunkedDecoder::new();
        let input = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let output = decoder.decode(input).unwrap();
        assert_eq!(output, b"hello world");
        assert!(decoder.is_finished());
    }

    #[test]
    fn chunked_split_across_reads() {
        let mut decoder = ChunkedDecoder::new();

        // First read: chunk size and partial data.
        let part1 = b"5\r\nhel";
        let out1 = decoder.decode(part1).unwrap();
        assert_eq!(out1, b"hel");
        assert!(!decoder.is_finished());

        // Second read: rest of data + terminator.
        let part2 = b"lo\r\n0\r\n\r\n";
        let out2 = decoder.decode(part2).unwrap();
        assert_eq!(out2, b"lo");
        assert!(decoder.is_finished());
    }

    #[test]
    fn chunked_hex_uppercase() {
        let mut decoder = ChunkedDecoder::new();
        let input = b"A\r\n0123456789\r\n0\r\n\r\n";
        let output = decoder.decode(input).unwrap();
        assert_eq!(output.len(), 10);
        assert!(decoder.is_finished());
    }

    #[test]
    fn chunked_with_extension() {
        let mut decoder = ChunkedDecoder::new();
        // Chunk extensions (after semicolon) should be ignored.
        let input = b"5;ext=val\r\nhello\r\n0\r\n\r\n";
        let output = decoder.decode(input).unwrap();
        assert_eq!(output, b"hello");
        assert!(decoder.is_finished());
    }

    #[test]
    fn chunked_empty() {
        let mut decoder = ChunkedDecoder::new();
        let input = b"0\r\n\r\n";
        let output = decoder.decode(input).unwrap();
        assert!(output.is_empty());
        assert!(decoder.is_finished());
    }

    #[test]
    fn chunked_invalid_hex() {
        let mut decoder = ChunkedDecoder::new();
        let input = b"xyz\r\n";
        assert!(decoder.decode(input).is_err());
    }

    // --- Redirect URL resolution ---

    #[test]
    fn resolve_absolute_redirect() {
        let result = resolve_redirect(
            "http://old.example.com/page",
            "http://new.example.com/other",
        );
        assert_eq!(result, "http://new.example.com/other");
    }

    #[test]
    fn resolve_relative_redirect_absolute_path() {
        let result = resolve_redirect("http://example.com/dir/page", "/new-path");
        assert_eq!(result, "http://example.com/new-path");
    }

    #[test]
    fn resolve_relative_redirect_relative_path() {
        let result = resolve_redirect("http://example.com/dir/page", "other.html");
        assert_eq!(result, "http://example.com/dir/other.html");
    }

    #[test]
    fn resolve_redirect_with_port() {
        let result = resolve_redirect("http://example.com:8080/dir/page", "/new");
        assert_eq!(result, "http://example.com:8080/new");
    }

    // --- Request building ---

    #[test]
    fn build_basic_request() {
        let url = parse_url("http://example.com/file.txt").unwrap();
        let opts = Options {
            url: "http://example.com/file.txt".to_string(),
            output_file: None,
            output_stdout: false,
            verbosity: Verbosity::Normal,
            resume: false,
            custom_headers: Vec::new(),
            max_redirects: 10,
            timeout_secs: 30,
            tries: 3,
            user_agent: "TestAgent/1.0".to_string(),
        };
        let req = build_request(&url, &opts, 0);
        assert!(req.starts_with("GET /file.txt HTTP/1.1\r\n"));
        assert!(req.contains("Host: example.com\r\n"));
        assert!(req.contains("User-Agent: TestAgent/1.0\r\n"));
        assert!(req.contains("Connection: close\r\n"));
        assert!(req.ends_with("\r\n\r\n"));
        // No Range header when offset is 0.
        assert!(!req.contains("Range:"));
    }

    #[test]
    fn build_resume_request() {
        let url = parse_url("http://example.com/file.bin").unwrap();
        let opts = Options {
            url: "http://example.com/file.bin".to_string(),
            output_file: None,
            output_stdout: false,
            verbosity: Verbosity::Normal,
            resume: true,
            custom_headers: Vec::new(),
            max_redirects: 10,
            timeout_secs: 30,
            tries: 3,
            user_agent: "SlateOS-wget/0.1".to_string(),
        };
        let req = build_request(&url, &opts, 1024);
        assert!(req.contains("Range: bytes=1024-\r\n"));
    }

    #[test]
    fn build_request_custom_headers() {
        let url = parse_url("http://example.com/api").unwrap();
        let opts = Options {
            url: "http://example.com/api".to_string(),
            output_file: None,
            output_stdout: false,
            verbosity: Verbosity::Normal,
            resume: false,
            custom_headers: vec![
                ("Authorization".to_string(), "Bearer token123".to_string()),
                ("Accept".to_string(), "application/json".to_string()),
            ],
            max_redirects: 10,
            timeout_secs: 30,
            tries: 3,
            user_agent: "SlateOS-wget/0.1".to_string(),
        };
        let req = build_request(&url, &opts, 0);
        assert!(req.contains("Authorization: Bearer token123\r\n"));
        assert!(req.contains("Accept: application/json\r\n"));
    }

    // --- Error display ---

    #[test]
    fn error_display_dns() {
        let e = WgetError::DnsFailure("bad.host".to_string());
        let msg = format!("{e}");
        assert!(msg.contains("bad.host"));
    }

    #[test]
    fn error_display_http() {
        let e = WgetError::HttpError(404, "Not Found".to_string());
        let msg = format!("{e}");
        assert!(msg.contains("404"));
        assert!(msg.contains("Not Found"));
    }

    #[test]
    fn error_display_redirect() {
        let e = WgetError::TooManyRedirects;
        let msg = format!("{e}");
        assert!(msg.contains("redirect"));
    }

    #[test]
    fn error_display_chunked() {
        let e = WgetError::ChunkedDecodeError("bad chunk".to_string());
        let msg = format!("{e}");
        assert!(msg.contains("bad chunk"));
    }

    #[test]
    fn error_display_io() {
        let e = WgetError::IoError(io::Error::new(io::ErrorKind::NotFound, "missing"));
        let msg = format!("{e}");
        assert!(msg.contains("missing"));
    }
}
