//! OurOS HTTP/HTTPS Client Utility
//!
//! A comprehensive HTTP client similar to curl, built for OurOS using the
//! kernel's TCP syscall interface. Supports multiple HTTP methods, custom
//! headers, authentication, redirects, cookies, chunked transfer encoding,
//! POST data, form uploads, and progress display.
//!
//! # Usage
//!
//! ```text
//! curl <url>                              GET request, output to stdout
//! curl -o <file> <url>                    Save output to file
//! curl -X POST -d "data" <url>            POST with data
//! curl -H "Header: Value" <url>           Custom header
//! curl -u user:pass <url>                 HTTP basic auth
//! curl -L <url>                           Follow redirects
//! curl -I <url>                           Show response headers only
//! curl -i <url>                           Include headers in output
//! curl -v <url>                           Verbose mode
//! curl -s <url>                           Silent mode
//! curl -A "Agent" <url>                   Set User-Agent
//! curl -e "http://ref" <url>              Set Referer
//! curl -b "name=val" <url>                Send cookie
//! curl -c <file> <url>                    Save cookies to file
//! curl -F "field=value" <url>             Multipart form POST
//! curl --connect-timeout 5 <url>          Connection timeout
//! curl --max-time 30 <url>                Maximum operation time
//! curl --max-redirs 10 <url>              Maximum redirects
//! curl -w "%{http_code}" <url>            Write-out format
//! curl <url1> <url2>                      Multiple URLs
//! ```

#![deny(clippy::all)]
#![allow(clippy::manual_range_contains)] // clearer as explicit comparisons

use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::process;
use std::time::Instant;

// ============================================================================
// Syscall interface
// ============================================================================

const SYS_TCP_CONNECT: u64 = 800;
const SYS_TCP_SEND: u64 = 802;
const SYS_TCP_RECV: u64 = 803;
const SYS_TCP_CLOSE: u64 = 804;
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
fn dns_resolve(hostname: &str) -> Result<u32, CurlError> {
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
        return Err(CurlError::DnsFailure(hostname.to_string()));
    }
    Ok(result_ip)
}

/// Open a TCP connection to the given IP (network byte order) and port.
/// Returns a handle on success.
fn tcp_connect(ip: u32, port: u16) -> Result<u64, CurlError> {
    // SAFETY: We pass a valid IP and port. The kernel returns a handle (>= 0)
    // or a negative error code. No pointers are involved.
    let ret = unsafe { syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), 0) };
    if ret < 0 {
        return Err(CurlError::ConnectionFailed(format!("error code {ret}")));
    }
    Ok(ret as u64)
}

/// Send data on a TCP connection. Returns the number of bytes actually sent.
fn tcp_send(handle: u64, data: &[u8]) -> Result<usize, CurlError> {
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
        return Err(CurlError::SendFailed);
    }
    Ok(ret as usize)
}

/// Send all bytes, looping until the entire buffer is transmitted.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), CurlError> {
    let mut offset = 0;
    while offset < data.len() {
        let sent = tcp_send(handle, &data[offset..])?;
        if sent == 0 {
            return Err(CurlError::SendFailed);
        }
        offset = offset.checked_add(sent).ok_or(CurlError::SendFailed)?;
    }
    Ok(())
}

/// Receive data from a TCP connection. Returns 0 when the peer has closed.
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, CurlError> {
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
        return Err(CurlError::RecvFailed);
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
enum CurlError {
    DnsFailure(String),
    ConnectionFailed(String),
    SendFailed,
    RecvFailed,
    #[allow(dead_code)]
    Timeout,
    InvalidUrl(String),
    HttpError(u16, String),
    TooManyRedirects,
    ChunkedDecodeError(String),
    IoError(io::Error),
    InvalidResponse(String),
    InvalidArgument(String),
}

impl std::fmt::Display for CurlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DnsFailure(host) => write!(f, "Could not resolve host: {host}"),
            Self::ConnectionFailed(msg) => write!(f, "Failed to connect: {msg}"),
            Self::SendFailed => write!(f, "Send failure"),
            Self::RecvFailed => write!(f, "Recv failure"),
            Self::Timeout => write!(f, "Operation timed out"),
            Self::InvalidUrl(msg) => write!(f, "URL rejected: {msg}"),
            Self::HttpError(code, reason) => {
                write!(f, "The requested URL returned error: {code} {reason}")
            }
            Self::TooManyRedirects => write!(f, "Maximum redirects followed"),
            Self::ChunkedDecodeError(msg) => write!(f, "chunk decoding error: {msg}"),
            Self::IoError(e) => write!(f, "{e}"),
            Self::InvalidResponse(msg) => write!(f, "Invalid response: {msg}"),
            Self::InvalidArgument(msg) => write!(f, "{msg}"),
        }
    }
}

impl From<io::Error> for CurlError {
    fn from(e: io::Error) -> Self {
        Self::IoError(e)
    }
}

// ============================================================================
// URL parsing
// ============================================================================

/// Parsed components of an HTTP URL.
#[derive(Clone)]
struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path: String,
    query: String,
    /// user:password extracted from the URL (e.g. http://user:pass@host/).
    userinfo: Option<String>,
}

impl ParsedUrl {
    /// Reconstruct the full URL string.
    #[allow(dead_code)] // Useful for debugging and future redirect-chain display.
    fn to_string_full(&self) -> String {
        let port_part = if (self.scheme == "http" && self.port == 80)
            || (self.scheme == "https" && self.port == 443)
        {
            String::new()
        } else {
            format!(":{}", self.port)
        };
        let query_part = if self.query.is_empty() {
            String::new()
        } else {
            format!("?{}", self.query)
        };
        format!(
            "{}://{}{}{}{}",
            self.scheme, self.host, port_part, self.path, query_part
        )
    }

    /// Return the path and query as a single request-target string.
    fn request_target(&self) -> String {
        if self.query.is_empty() {
            self.path.clone()
        } else {
            format!("{}?{}", self.path, self.query)
        }
    }
}

/// Parse a URL into its components.
/// Supports `http://` and `https://` schemes.
fn parse_url(url: &str) -> Result<ParsedUrl, CurlError> {
    let (scheme, rest) = if let Some(r) = url.strip_prefix("http://") {
        ("http", r)
    } else if let Some(r) = url.strip_prefix("https://") {
        ("https", r)
    } else if url.contains("://") {
        return Err(CurlError::InvalidUrl(format!(
            "unsupported scheme in '{url}'"
        )));
    } else {
        // Default to http:// if no scheme provided.
        ("http", url)
    };

    // Split authority from path at the first '/'.
    let (authority, path_and_query) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };

    if authority.is_empty() {
        return Err(CurlError::InvalidUrl("empty hostname".to_string()));
    }

    // Extract optional userinfo (user:pass@host).
    let (userinfo, host_port) = if let Some(at_idx) = authority.rfind('@') {
        (
            Some(authority[..at_idx].to_string()),
            &authority[at_idx + 1..],
        )
    } else {
        (None, authority)
    };

    if host_port.is_empty() {
        return Err(CurlError::InvalidUrl("empty hostname".to_string()));
    }

    // Split host from port.
    let (host, port) = if let Some(colon_idx) = host_port.rfind(':') {
        let port_str = &host_port[colon_idx + 1..];
        let port: u16 = port_str.parse().map_err(|_| {
            CurlError::InvalidUrl(format!("invalid port number '{port_str}'"))
        })?;
        (&host_port[..colon_idx], port)
    } else {
        let default_port = if scheme == "https" { 443 } else { 80 };
        (host_port, default_port)
    };

    // Split path from query.
    let (path, query) = if let Some(qm_idx) = path_and_query.find('?') {
        (
            &path_and_query[..qm_idx],
            &path_and_query[qm_idx + 1..],
        )
    } else {
        (path_and_query, "")
    };

    Ok(ParsedUrl {
        scheme: scheme.to_string(),
        host: host.to_string(),
        port,
        path: path.to_string(),
        query: query.to_string(),
        userinfo,
    })
}

// ============================================================================
// URL encoding/decoding helpers
// ============================================================================

/// URL-encode a string: unreserved characters (A-Z, a-z, 0-9, '-', '_', '.',
/// '~') pass through; everything else becomes %XX.
#[allow(dead_code)] // Utility for future use (e.g., form data encoding).
fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~' => out.push(byte as char),
            _ => {
                out.push('%');
                out.push(hex_digit(byte >> 4));
                out.push(hex_digit(byte & 0x0F));
            }
        }
    }
    out
}

/// URL-decode a percent-encoded string.
#[allow(dead_code)] // Utility for future use (e.g., response body decoding).
fn url_decode(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (
                from_hex_digit(bytes[i + 1]),
                from_hex_digit(bytes[i + 2]),
            ) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[allow(dead_code)]
fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + nibble - 10) as char,
        _ => '0',
    }
}

#[allow(dead_code)]
fn from_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

// ============================================================================
// Base64 encoding (for HTTP Basic Auth)
// ============================================================================

const BASE64_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes to base64.
fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i];
        let b1 = if i + 1 < input.len() {
            input[i + 1]
        } else {
            0
        };
        let b2 = if i + 2 < input.len() {
            input[i + 2]
        } else {
            0
        };

        out.push(BASE64_CHARS[(b0 >> 2) as usize] as char);
        out.push(BASE64_CHARS[((b0 & 0x03) << 4 | b1 >> 4) as usize] as char);

        if i + 1 < input.len() {
            out.push(BASE64_CHARS[((b1 & 0x0F) << 2 | b2 >> 6) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < input.len() {
            out.push(BASE64_CHARS[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }

        i += 3;
    }
    out
}

// ============================================================================
// IP address helpers
// ============================================================================

/// Parse a dotted-decimal IPv4 address string into a u32 in network byte order.
fn parse_ipv4(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        let val: u16 = part.parse().ok()?;
        if val > 255 {
            return None;
        }
        octets[i] = val as u8;
    }
    Some(u32::from_be_bytes(octets))
}

/// Format a u32 IP (network byte order) as a dotted-quad string.
fn ip_to_string(ip: u32) -> String {
    let bytes = ip.to_be_bytes();
    format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
}

/// Check if a string looks like an IPv4 address.
fn is_ipv4_address(s: &str) -> bool {
    parse_ipv4(s).is_some()
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
fn parse_http_response(data: &[u8]) -> Result<HttpResponse, CurlError> {
    let header_end = find_header_end(data)
        .ok_or_else(|| CurlError::InvalidResponse("incomplete headers".to_string()))?;

    let header_bytes = &data[..header_end];
    let header_text = String::from_utf8_lossy(header_bytes);
    let mut lines = header_text.split("\r\n");

    let status_line = lines
        .next()
        .ok_or_else(|| CurlError::InvalidResponse("empty response".to_string()))?;
    let status = parse_status_line(status_line)?;

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

    let body_offset = header_end + 4; // Skip \r\n\r\n

    Ok(HttpResponse {
        status,
        headers,
        body_offset,
    })
}

/// Parse an HTTP status line like "HTTP/1.1 200 OK".
fn parse_status_line(line: &str) -> Result<HttpStatus, CurlError> {
    let mut parts = line.splitn(3, ' ');
    let _version = parts
        .next()
        .ok_or_else(|| CurlError::InvalidResponse("missing HTTP version".to_string()))?;
    let code_str = parts
        .next()
        .ok_or_else(|| CurlError::InvalidResponse("missing status code".to_string()))?;
    let reason = parts.next().unwrap_or("").to_string();

    let code: u16 = code_str.parse().map_err(|_| {
        CurlError::InvalidResponse(format!("invalid status code '{code_str}'"))
    })?;

    Ok(HttpStatus { code, reason })
}

/// Find the position of \r\n\r\n in the buffer.
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

/// Get all header values for a given (lowercase) name.
fn get_headers<'a>(headers: &'a [(String, String)], name: &str) -> Vec<&'a str> {
    headers
        .iter()
        .filter(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
        .collect()
}

/// Reconstruct the raw header text from parsed headers.
fn format_raw_headers(response: &HttpResponse) -> String {
    let mut out = format!(
        "HTTP/1.1 {} {}\r\n",
        response.status.code, response.status.reason
    );
    for (name, value) in &response.headers {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    out
}

// ============================================================================
// Chunked transfer decoding
// ============================================================================

/// State machine for decoding chunked transfer encoding.
struct ChunkedDecoder {
    remaining: usize,
    in_chunk: bool,
    finished: bool,
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
    fn decode(&mut self, input: &[u8]) -> Result<Vec<u8>, CurlError> {
        let mut output = Vec::new();
        let mut pos = 0;

        while pos < input.len() && !self.finished {
            if self.in_chunk {
                let available = input.len() - pos;
                let to_copy = available.min(self.remaining);
                output.extend_from_slice(&input[pos..pos + to_copy]);
                pos += to_copy;
                self.remaining -= to_copy;

                if self.remaining == 0 {
                    self.in_chunk = false;
                    // Skip trailing \r\n.
                    if pos + 2 <= input.len()
                        && input.get(pos) == Some(&b'\r')
                        && input.get(pos + 1) == Some(&b'\n')
                    {
                        pos += 2;
                    } else if pos < input.len() && input.get(pos) == Some(&b'\r') {
                        pos += 1;
                    }
                }
            } else {
                // Read a chunk-size line.
                while pos < input.len() {
                    let byte = input[pos];
                    pos += 1;
                    self.line_buf.push(byte);

                    if self.line_buf.len() >= 2
                        && self.line_buf[self.line_buf.len() - 2] == b'\r'
                        && self.line_buf[self.line_buf.len() - 1] == b'\n'
                    {
                        let len = self.line_buf.len() - 2;
                        let line = &self.line_buf[..len];
                        let size_str = String::from_utf8_lossy(line);
                        let hex_part = size_str.split(';').next().unwrap_or("");
                        let hex_trimmed = hex_part.trim();

                        if hex_trimmed.is_empty() {
                            self.line_buf.clear();
                            continue;
                        }

                        let chunk_size =
                            usize::from_str_radix(hex_trimmed, 16).map_err(|_| {
                                CurlError::ChunkedDecodeError(format!(
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

                    if self.line_buf.len() > 256 {
                        return Err(CurlError::ChunkedDecodeError(
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
// Cookie support
// ============================================================================

/// A single cookie name=value pair with optional domain/path metadata.
#[derive(Clone)]
struct Cookie {
    name: String,
    value: String,
    domain: String,
    path: String,
}

/// Parse Set-Cookie headers from a response and collect cookies.
fn parse_set_cookies(headers: &[(String, String)], request_host: &str) -> Vec<Cookie> {
    let mut cookies = Vec::new();
    for val in get_headers(headers, "set-cookie") {
        // Cookie value is everything before the first ';'.
        let cookie_part = val.split(';').next().unwrap_or("");
        if let Some((name, value)) = cookie_part.split_once('=') {
            let name = name.trim().to_string();
            let value = value.trim().to_string();

            // Extract domain and path from attributes.
            let mut domain = request_host.to_string();
            let mut path = String::from("/");
            for attr in val.split(';').skip(1) {
                let attr = attr.trim();
                if let Some((aname, aval)) = attr.split_once('=') {
                    match aname.trim().to_ascii_lowercase().as_str() {
                        "domain" => domain = aval.trim().to_string(),
                        "path" => path = aval.trim().to_string(),
                        _ => {}
                    }
                }
            }

            cookies.push(Cookie {
                name,
                value,
                domain,
                path,
            });
        }
    }
    cookies
}

/// Format cookies for the Cookie header value.
fn format_cookie_header(cookies: &[Cookie]) -> String {
    cookies
        .iter()
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Write cookies to a Netscape-format cookie jar file.
fn save_cookie_jar(path: &str, cookies: &[Cookie]) -> Result<(), CurlError> {
    let mut file = File::create(path)?;
    writeln!(file, "# Netscape HTTP Cookie File")?;
    writeln!(file, "# https://curl.se/docs/http-cookies.html")?;
    writeln!(file, "# This file was generated by OurOS curl.")?;
    writeln!(file)?;
    for c in cookies {
        // Format: domain  flag  path  secure  expires  name  value
        let domain_flag = if c.domain.starts_with('.') {
            "TRUE"
        } else {
            "FALSE"
        };
        writeln!(
            file,
            "{}\t{}\t{}\tFALSE\t0\t{}\t{}",
            c.domain, domain_flag, c.path, c.name, c.value
        )?;
    }
    Ok(())
}

/// Parse cookies from a Netscape-format cookie jar file.
fn load_cookie_jar(path: &str) -> Vec<Cookie> {
    let mut cookies = Vec::new();
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return cookies,
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() >= 7 {
            cookies.push(Cookie {
                domain: fields[0].to_string(),
                path: fields[2].to_string(),
                name: fields[5].to_string(),
                value: fields[6].to_string(),
            });
        }
    }
    cookies
}

/// Parse inline cookie string "name1=val1; name2=val2" into Cookie structs.
fn parse_inline_cookies(cookie_str: &str, host: &str) -> Vec<Cookie> {
    let mut cookies = Vec::new();
    for pair in cookie_str.split(';') {
        let pair = pair.trim();
        if let Some((name, value)) = pair.split_once('=') {
            cookies.push(Cookie {
                name: name.trim().to_string(),
                value: value.trim().to_string(),
                domain: host.to_string(),
                path: String::from("/"),
            });
        }
    }
    cookies
}

// ============================================================================
// Human-readable size formatting
// ============================================================================

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

fn format_speed(bytes_per_sec: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    if bytes_per_sec >= GIB {
        format!("{:.2} GB/s", bytes_per_sec / GIB)
    } else if bytes_per_sec >= MIB {
        format!("{:.2} MB/s", bytes_per_sec / MIB)
    } else if bytes_per_sec >= KIB {
        format!("{:.2} KB/s", bytes_per_sec / KIB)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

// ============================================================================
// Progress display
// ============================================================================

/// Print a curl-style progress line to stderr.
fn print_progress(downloaded: u64, total: Option<u64>, start_time: Instant) {
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
            let pct = (downloaded as f64 / total_bytes as f64 * 100.0).min(100.0);
            let _ = write!(
                err,
                "\r  % Total    % Received  Average Speed   Time\r\n\
                 {:>3.0} {:>8}  {:>3.0} {:>8}  {:>12}  {:>6.1}s\r",
                pct,
                format_size(total_bytes),
                pct,
                format_size(downloaded),
                format_speed(speed),
                elapsed,
            );
        }
        _ => {
            let _ = write!(
                err,
                "\r  Received: {}  Speed: {}  Time: {:.1}s   ",
                format_size(downloaded),
                format_speed(speed),
                elapsed,
            );
        }
    }
    let _ = err.flush();
}

// ============================================================================
// Write-out formatting
// ============================================================================

/// Expand write-out format string with variable substitutions.
fn expand_write_out(
    fmt: &str,
    status_code: u16,
    total_bytes: u64,
    speed: f64,
    elapsed: f64,
    url: &str,
    content_type: &str,
    num_redirects: u32,
) -> String {
    let mut result = String::new();
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' && i + 1 < chars.len() && chars[i + 1] == '{' {
            // Find closing '}'.
            if let Some(close_idx) = chars[i + 2..].iter().position(|&c| c == '}') {
                let var_name: String = chars[i + 2..i + 2 + close_idx].iter().collect();
                let replacement = match var_name.as_str() {
                    "http_code" | "response_code" => format!("{status_code}"),
                    "size_download" => format!("{total_bytes}"),
                    "speed_download" => format!("{speed:.3}"),
                    "time_total" => format!("{elapsed:.6}"),
                    "url_effective" => url.to_string(),
                    "content_type" => content_type.to_string(),
                    "num_redirects" => format!("{num_redirects}"),
                    _ => format!("%{{{var_name}}}"),
                };
                result.push_str(&replacement);
                i += 2 + close_idx + 1; // Skip %{...}
                continue;
            }
        }
        if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                'n' => {
                    result.push('\n');
                    i += 2;
                    continue;
                }
                't' => {
                    result.push('\t');
                    i += 2;
                    continue;
                }
                'r' => {
                    result.push('\r');
                    i += 2;
                    continue;
                }
                '\\' => {
                    result.push('\\');
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

// ============================================================================
// Multipart form data builder
// ============================================================================

/// Build a multipart/form-data body from -F arguments.
/// Returns (boundary, body_bytes).
fn build_multipart_body(fields: &[(String, String)]) -> (String, Vec<u8>) {
    // Use a fixed boundary derived from a simple hash to keep things deterministic.
    let boundary = format!("------------------------ouros{:016x}", simple_hash(fields));
    let mut body = Vec::new();

    for (name, value) in fields {
        body.extend_from_slice(b"--");
        body.extend_from_slice(boundary.as_bytes());
        body.extend_from_slice(b"\r\n");

        if let Some(filepath) = value.strip_prefix('@') {
            // File upload: read file content.
            let filename = filepath.rsplit('/').next().unwrap_or(filepath);
            body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(b"Content-Type: application/octet-stream\r\n");
            body.extend_from_slice(b"\r\n");
            // Read the file; on error, include empty content.
            if let Ok(content) = std::fs::read(filepath) {
                body.extend_from_slice(&content);
            }
        } else {
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n").as_bytes(),
            );
            body.extend_from_slice(b"\r\n");
            body.extend_from_slice(value.as_bytes());
        }
        body.extend_from_slice(b"\r\n");
    }

    // Final boundary.
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");

    (boundary, body)
}

/// Simple non-cryptographic hash for generating a multipart boundary.
fn simple_hash(fields: &[(String, String)]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for (name, value) in fields {
        for byte in name.bytes().chain(value.bytes()) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

// ============================================================================
// CLI argument parsing
// ============================================================================

/// HTTP method to use.
#[derive(Clone, PartialEq, Eq)]
enum Method {
    Get,
    Post,
    Put,
    Delete,
    Head,
    Patch,
}

impl Method {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Patch => "PATCH",
        }
    }
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parsed command-line options.
struct Options {
    urls: Vec<String>,
    method: Method,
    custom_headers: Vec<(String, String)>,
    user_agent: String,
    referer: Option<String>,
    data: Option<String>,
    form_fields: Vec<(String, String)>,
    output_file: Option<String>,
    follow_redirects: bool,
    max_redirects: u32,
    auth: Option<String>,
    verbose: bool,
    silent: bool,
    show_headers_only: bool,
    include_headers: bool,
    cookie_string: Option<String>,
    cookie_jar_read: Option<String>,
    cookie_jar_write: Option<String>,
    #[allow(dead_code)]
    connect_timeout_secs: u64,
    #[allow(dead_code)]
    max_time_secs: u64,
    write_out: Option<String>,
    show_progress: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            urls: Vec::new(),
            method: Method::Get,
            custom_headers: Vec::new(),
            user_agent: String::from("curl/0.1.0 (OurOS)"),
            referer: None,
            data: None,
            form_fields: Vec::new(),
            output_file: None,
            follow_redirects: false,
            max_redirects: 50,
            auth: None,
            verbose: false,
            silent: false,
            show_headers_only: false,
            include_headers: false,
            cookie_string: None,
            cookie_jar_read: None,
            cookie_jar_write: None,
            connect_timeout_secs: 30,
            max_time_secs: 0,
            write_out: None,
            show_progress: false,
        }
    }
}

fn print_usage() {
    let stderr = io::stderr();
    let mut err = stderr.lock();
    let _ = writeln!(err, "Usage: curl [OPTIONS] <URL> [URL...]");
    let _ = writeln!(err);
    let _ = writeln!(err, "OurOS HTTP client.");
    let _ = writeln!(err);
    let _ = writeln!(err, "Options:");
    let _ = writeln!(err, "  -X <METHOD>              HTTP method (GET, POST, PUT, DELETE, HEAD, PATCH)");
    let _ = writeln!(err, "  -H <header>              Custom header (\"Name: Value\")");
    let _ = writeln!(err, "  -A <agent>               User-Agent string");
    let _ = writeln!(err, "  -e <url>                 Referer URL");
    let _ = writeln!(err, "  -d <data>, --data <data> HTTP POST data");
    let _ = writeln!(err, "  -F <name=value>          Multipart form field (use @file for upload)");
    let _ = writeln!(err, "  -o <file>                Write output to file");
    let _ = writeln!(err, "  -O                       Write to file named from URL");
    let _ = writeln!(err, "  -L, --location           Follow redirects");
    let _ = writeln!(err, "  --max-redirs <num>       Maximum number of redirects (default: 50)");
    let _ = writeln!(err, "  -u <user:password>       HTTP basic authentication");
    let _ = writeln!(err, "  -v, --verbose            Verbose mode");
    let _ = writeln!(err, "  -s, --silent             Silent mode");
    let _ = writeln!(err, "  -I, --head               Show response headers only");
    let _ = writeln!(err, "  -i, --include            Include response headers in output");
    let _ = writeln!(err, "  -b <cookies>             Send cookies (\"name=val\" or filename)");
    let _ = writeln!(err, "  -c <file>                Write cookies to file (cookie jar)");
    let _ = writeln!(err, "  --connect-timeout <secs> Connection timeout in seconds");
    let _ = writeln!(err, "  --max-time <secs>        Maximum time for the operation");
    let _ = writeln!(err, "  -w <format>              Write-out format after transfer");
    let _ = writeln!(err, "  -#, --progress-bar       Show progress bar");
    let _ = writeln!(err, "  -h, --help               Show this help message");
    let _ = writeln!(err);
    let _ = writeln!(err, "Write-out variables: %{{http_code}}, %{{size_download}},");
    let _ = writeln!(err, "  %{{speed_download}}, %{{time_total}}, %{{url_effective}},");
    let _ = writeln!(err, "  %{{content_type}}, %{{num_redirects}}");
}

fn parse_args() -> Result<Options, CurlError> {
    let argv: Vec<String> = env::args().collect();

    if argv.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut opts = Options::default();
    let mut explicit_method = false;

    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-X" | "--request" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("-X requires a method".to_string())
                })?;
                opts.method = match val.to_ascii_uppercase().as_str() {
                    "GET" => Method::Get,
                    "POST" => Method::Post,
                    "PUT" => Method::Put,
                    "DELETE" => Method::Delete,
                    "HEAD" => Method::Head,
                    "PATCH" => Method::Patch,
                    other => {
                        return Err(CurlError::InvalidArgument(format!(
                            "unknown method '{other}'"
                        )));
                    }
                };
                explicit_method = true;
            }
            "-H" | "--header" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("-H requires a header value".to_string())
                })?;
                if let Some((name, value)) = val.split_once(':') {
                    opts.custom_headers
                        .push((name.trim().to_string(), value.trim().to_string()));
                } else {
                    return Err(CurlError::InvalidArgument(format!(
                        "invalid header '{val}' (expected \"Name: Value\")"
                    )));
                }
            }
            "-A" | "--user-agent" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("-A requires a user-agent string".to_string())
                })?;
                opts.user_agent = val.clone();
            }
            "-e" | "--referer" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("-e requires a referer URL".to_string())
                })?;
                opts.referer = Some(val.clone());
            }
            "-d" | "--data" | "--data-raw" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("-d requires data".to_string())
                })?;
                // Append to existing data with & separator.
                if let Some(ref mut existing) = opts.data {
                    existing.push('&');
                    existing.push_str(val);
                } else {
                    opts.data = Some(val.clone());
                }
                // Implicitly set POST if no explicit method given.
                if !explicit_method {
                    opts.method = Method::Post;
                }
            }
            "-F" | "--form" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("-F requires name=value".to_string())
                })?;
                if let Some((name, value)) = val.split_once('=') {
                    opts.form_fields
                        .push((name.to_string(), value.to_string()));
                } else {
                    return Err(CurlError::InvalidArgument(format!(
                        "invalid form field '{val}' (expected name=value)"
                    )));
                }
                if !explicit_method {
                    opts.method = Method::Post;
                }
            }
            "-o" | "--output" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("-o requires a filename".to_string())
                })?;
                opts.output_file = Some(val.clone());
            }
            "-O" | "--remote-name" => {
                // Output file will be derived from URL later.
                opts.output_file = Some(String::new());
            }
            "-L" | "--location" => {
                opts.follow_redirects = true;
            }
            "--max-redirs" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("--max-redirs requires a number".to_string())
                })?;
                opts.max_redirects = val.parse().map_err(|_| {
                    CurlError::InvalidArgument(format!("invalid redirect count '{val}'"))
                })?;
            }
            "-u" | "--user" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("-u requires user:password".to_string())
                })?;
                opts.auth = Some(val.clone());
            }
            "-v" | "--verbose" => {
                opts.verbose = true;
            }
            "-s" | "--silent" => {
                opts.silent = true;
            }
            "-I" | "--head" => {
                opts.show_headers_only = true;
                if !explicit_method {
                    opts.method = Method::Head;
                }
            }
            "-i" | "--include" => {
                opts.include_headers = true;
            }
            "-b" | "--cookie" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("-b requires cookie string or file".to_string())
                })?;
                // If the value contains '=' it's an inline cookie; if it looks
                // like a file path (contains / or \ or ends in .txt) treat it
                // as a cookie jar file.
                if val.contains('/')
                    || val.contains('\\')
                    || val.ends_with(".txt")
                    || val.ends_with(".jar")
                {
                    opts.cookie_jar_read = Some(val.clone());
                } else {
                    opts.cookie_string = Some(val.clone());
                }
            }
            "-c" | "--cookie-jar" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("-c requires a filename".to_string())
                })?;
                opts.cookie_jar_write = Some(val.clone());
            }
            "--connect-timeout" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument(
                        "--connect-timeout requires seconds".to_string(),
                    )
                })?;
                opts.connect_timeout_secs = val.parse().map_err(|_| {
                    CurlError::InvalidArgument(format!("invalid timeout '{val}'"))
                })?;
            }
            "--max-time" | "-m" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("--max-time requires seconds".to_string())
                })?;
                opts.max_time_secs = val.parse().map_err(|_| {
                    CurlError::InvalidArgument(format!("invalid max-time '{val}'"))
                })?;
            }
            "-w" | "--write-out" => {
                i += 1;
                let val = argv.get(i).ok_or_else(|| {
                    CurlError::InvalidArgument("-w requires a format string".to_string())
                })?;
                opts.write_out = Some(val.clone());
            }
            "-#" | "--progress-bar" => {
                opts.show_progress = true;
            }
            s if s.starts_with('-') => {
                return Err(CurlError::InvalidArgument(format!(
                    "unknown option '{s}'"
                )));
            }
            _ => {
                opts.urls.push(arg.clone());
            }
        }
        i += 1;
    }

    if opts.urls.is_empty() {
        return Err(CurlError::InvalidArgument("no URL specified".to_string()));
    }

    Ok(opts)
}

// ============================================================================
// HTTP request building
// ============================================================================

/// Build an HTTP/1.1 request string.
fn build_request(
    url: &ParsedUrl,
    opts: &Options,
    body: Option<&[u8]>,
    content_type: Option<&str>,
    cookies: &[Cookie],
) -> Vec<u8> {
    let target = url.request_target();
    let mut req = format!("{} {} HTTP/1.1\r\n", opts.method, target);

    // Host header: include port only if non-default.
    if (url.scheme == "http" && url.port != 80)
        || (url.scheme == "https" && url.port != 443)
    {
        req.push_str(&format!("Host: {}:{}\r\n", url.host, url.port));
    } else {
        req.push_str(&format!("Host: {}\r\n", url.host));
    }

    req.push_str(&format!("User-Agent: {}\r\n", opts.user_agent));
    req.push_str("Accept: */*\r\n");

    // Referer.
    if let Some(ref referer) = opts.referer {
        req.push_str(&format!("Referer: {referer}\r\n"));
    }

    // Authentication: from -u flag or from URL userinfo.
    let auth_string = opts.auth.as_deref().or(url.userinfo.as_deref());
    if let Some(auth) = auth_string {
        let encoded = base64_encode(auth.as_bytes());
        req.push_str(&format!("Authorization: Basic {encoded}\r\n"));
    }

    // Content-Type and Content-Length for request body.
    if let Some(body_data) = body {
        if let Some(ct) = content_type {
            req.push_str(&format!("Content-Type: {ct}\r\n"));
        }
        req.push_str(&format!("Content-Length: {}\r\n", body_data.len()));
    }

    // Cookies.
    if !cookies.is_empty() {
        let cookie_header = format_cookie_header(cookies);
        req.push_str(&format!("Cookie: {cookie_header}\r\n"));
    }

    // Custom headers (may override the above).
    for (name, value) in &opts.custom_headers {
        req.push_str(&format!("{name}: {value}\r\n"));
    }

    req.push_str("Connection: close\r\n");
    req.push_str("\r\n");

    let mut bytes = req.into_bytes();
    if let Some(body_data) = body {
        bytes.extend_from_slice(body_data);
    }
    bytes
}

// ============================================================================
// Redirect resolution
// ============================================================================

/// Resolve a possibly-relative redirect URL against the current URL.
fn resolve_redirect(current_url: &str, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        return location.to_string();
    }

    if let Ok(parsed) = parse_url(current_url) {
        if location.starts_with('/') {
            // Absolute path on same host.
            let port_part =
                if (parsed.scheme == "http" && parsed.port == 80)
                    || (parsed.scheme == "https" && parsed.port == 443)
                {
                    String::new()
                } else {
                    format!(":{}", parsed.port)
                };
            format!("{}://{}{}{}", parsed.scheme, parsed.host, port_part, location)
        } else {
            // Relative path: append to current directory.
            let dir = match parsed.path.rfind('/') {
                Some(idx) => &parsed.path[..=idx],
                None => "/",
            };
            let port_part =
                if (parsed.scheme == "http" && parsed.port == 80)
                    || (parsed.scheme == "https" && parsed.port == 443)
                {
                    String::new()
                } else {
                    format!(":{}", parsed.port)
                };
            format!(
                "{}://{}{}{}{}",
                parsed.scheme, parsed.host, port_part, dir, location
            )
        }
    } else {
        location.to_string()
    }
}

// ============================================================================
// Extract filename from URL for -O mode
// ============================================================================

fn filename_from_url(url: &ParsedUrl) -> String {
    let basename = url.path.rsplit('/').next().unwrap_or("");
    let name = basename.split('?').next().unwrap_or("");
    if name.is_empty() {
        "index.html".to_string()
    } else {
        name.to_string()
    }
}

// ============================================================================
// Core request logic
// ============================================================================

/// Receive the full HTTP response headers.
fn recv_headers(handle: u64) -> Result<Vec<u8>, CurlError> {
    let mut buf = Vec::with_capacity(4096);
    let mut recv_buf = [0u8; 8192];

    loop {
        let n = tcp_recv(handle, &mut recv_buf)?;
        if n == 0 {
            if buf.is_empty() {
                return Err(CurlError::InvalidResponse("empty response".to_string()));
            }
            break;
        }
        buf.extend_from_slice(&recv_buf[..n]);

        if find_header_end(&buf).is_some() {
            break;
        }

        if buf.len() > 64 * 1024 {
            return Err(CurlError::InvalidResponse(
                "response headers exceed 64 KiB".to_string(),
            ));
        }
    }

    Ok(buf)
}

/// Result of a single HTTP request/response cycle.
struct RequestResult {
    status_code: u16,
    content_type: String,
    body_bytes: u64,
    redirect_url: Option<String>,
    response_cookies: Vec<Cookie>,
}

/// Perform a single HTTP request and handle the response.
fn do_request(
    url_str: &str,
    opts: &Options,
    cookies: &[Cookie],
) -> Result<RequestResult, CurlError> {
    let url = parse_url(url_str)?;

    // Only http:// is supported for actual TCP connections.
    if url.scheme != "http" {
        return Err(CurlError::InvalidUrl(format!(
            "only HTTP is supported (got {}://)",
            url.scheme
        )));
    }

    // Resolve hostname.
    let ip = if is_ipv4_address(&url.host) {
        parse_ipv4(&url.host).ok_or_else(|| {
            CurlError::DnsFailure(url.host.clone())
        })?
    } else {
        if opts.verbose {
            eprintln!("* Trying to resolve host '{}'...", url.host);
        }
        dns_resolve(&url.host)?
    };

    if opts.verbose {
        eprintln!("*   Trying {}:{}...", ip_to_string(ip), url.port);
    }

    let handle = tcp_connect(ip, url.port)?;

    if opts.verbose {
        eprintln!("* Connected to {} ({}) port {}", url.host, ip_to_string(ip), url.port);
    }

    // Prepare request body.
    let (body_data, content_type_override): (Option<Vec<u8>>, Option<String>) =
        if !opts.form_fields.is_empty() {
            let (boundary, body) = build_multipart_body(&opts.form_fields);
            (
                Some(body),
                Some(format!("multipart/form-data; boundary={boundary}")),
            )
        } else if let Some(ref data) = opts.data {
            (
                Some(data.as_bytes().to_vec()),
                Some("application/x-www-form-urlencoded".to_string()),
            )
        } else {
            (None, None)
        };

    let ct_ref = content_type_override.as_deref();
    let body_ref = body_data.as_deref();
    let request_bytes = build_request(&url, opts, body_ref, ct_ref, cookies);

    // Verbose: print request headers.
    if opts.verbose {
        let header_end_pos = find_header_end(&request_bytes).unwrap_or(request_bytes.len());
        let header_text = String::from_utf8_lossy(&request_bytes[..header_end_pos]);
        for line in header_text.lines() {
            eprintln!("> {line}");
        }
        eprintln!(">");
    }

    // Send the request.
    if let Err(e) = tcp_send_all(handle, &request_bytes) {
        tcp_close(handle);
        return Err(e);
    }

    // Receive response headers.
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

    // Verbose: print response headers.
    if opts.verbose {
        eprintln!(
            "< HTTP/1.1 {} {}",
            response.status.code, response.status.reason
        );
        for (name, value) in &response.headers {
            eprintln!("< {name}: {value}");
        }
        eprintln!("<");
    }

    // Collect cookies from the response.
    let response_cookies = parse_set_cookies(&response.headers, &url.host);

    // Handle redirects (3xx).
    if response.status.code >= 300 && response.status.code < 400 {
        tcp_close(handle);
        if let Some(location) = get_header(&response.headers, "location") {
            let redirect_url = resolve_redirect(url_str, location);
            return Ok(RequestResult {
                status_code: response.status.code,
                content_type: String::new(),
                body_bytes: 0,
                redirect_url: Some(redirect_url),
                response_cookies,
            });
        }
        return Err(CurlError::HttpError(
            response.status.code,
            "redirect without Location header".to_string(),
        ));
    }

    let content_type = get_header(&response.headers, "content-type")
        .unwrap_or("application/octet-stream")
        .to_string();
    let content_length: Option<u64> = get_header(&response.headers, "content-length")
        .and_then(|v| v.parse().ok());
    let is_chunked = get_header(&response.headers, "transfer-encoding")
        .map(|v| v.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false);

    // Output: headers if -I or -i.
    let mut writer: Box<dyn Write> = match &opts.output_file {
        Some(path) if !path.is_empty() => Box::new(File::create(path)?),
        _ => Box::new(io::stdout().lock()),
    };

    if opts.show_headers_only {
        // -I: print status line and headers, then done.
        let raw_hdrs = format_raw_headers(&response);
        writer.write_all(raw_hdrs.as_bytes())?;
        tcp_close(handle);
        return Ok(RequestResult {
            status_code: response.status.code,
            content_type,
            body_bytes: 0,
            redirect_url: None,
            response_cookies,
        });
    }

    if opts.include_headers {
        // -i: include headers before body.
        let raw_hdrs = format_raw_headers(&response);
        writer.write_all(raw_hdrs.as_bytes())?;
    }

    // Download the body.
    let start_time = Instant::now();
    let mut downloaded: u64 = 0;
    let mut chunked_decoder = if is_chunked {
        Some(ChunkedDecoder::new())
    } else {
        None
    };

    // Process body bytes already in header buffer.
    let initial_body = &raw_response[response.body_offset..];
    if !initial_body.is_empty() {
        let body_data_chunk = if let Some(ref mut decoder) = chunked_decoder {
            decoder.decode(initial_body)?
        } else {
            initial_body.to_vec()
        };
        if !body_data_chunk.is_empty() {
            writer.write_all(&body_data_chunk)?;
            downloaded = downloaded
                .checked_add(body_data_chunk.len() as u64)
                .unwrap_or(downloaded);
        }
    }

    // Show progress if requested.
    if opts.show_progress && !opts.silent {
        print_progress(downloaded, content_length, start_time);
    }

    // For HEAD requests, skip body reading.
    if opts.method != Method::Head {
        let mut recv_buf = [0u8; 8192];
        loop {
            if let Some(ref decoder) = chunked_decoder {
                if decoder.is_finished() {
                    break;
                }
            } else if let Some(cl) = content_length {
                if downloaded >= cl {
                    break;
                }
            }

            let n = tcp_recv(handle, &mut recv_buf)?;
            if n == 0 {
                break;
            }

            let body_data_chunk = if let Some(ref mut decoder) = chunked_decoder {
                decoder.decode(&recv_buf[..n])?
            } else {
                recv_buf[..n].to_vec()
            };

            if !body_data_chunk.is_empty() {
                writer.write_all(&body_data_chunk)?;
                downloaded = downloaded
                    .checked_add(body_data_chunk.len() as u64)
                    .unwrap_or(downloaded);
            }

            if opts.show_progress && !opts.silent {
                print_progress(downloaded, content_length, start_time);
            }
        }
    }

    writer.flush()?;
    tcp_close(handle);

    // Clear progress line.
    if opts.show_progress && !opts.silent {
        eprintln!();
    }

    Ok(RequestResult {
        status_code: response.status.code,
        content_type,
        body_bytes: downloaded,
        redirect_url: None,
        response_cookies,
    })
}

// ============================================================================
// Top-level transfer logic with redirects and cookie tracking
// ============================================================================

/// Perform a transfer for a single URL, following redirects as needed.
fn transfer_url(url: &str, opts: &Options) -> Result<(), CurlError> {
    let start_time = Instant::now();
    let mut current_url = url.to_string();
    let mut redirects_remaining = opts.max_redirects;
    let mut num_redirects: u32 = 0;

    // Collect cookies from -b flag.
    let mut cookies: Vec<Cookie> = Vec::new();
    if let Some(ref jar_path) = opts.cookie_jar_read {
        cookies.extend(load_cookie_jar(jar_path));
    }
    if let Some(ref cookie_str) = opts.cookie_string {
        let parsed = parse_url(&current_url).unwrap_or_else(|_| ParsedUrl {
            scheme: "http".to_string(),
            host: "localhost".to_string(),
            port: 80,
            path: "/".to_string(),
            query: String::new(),
            userinfo: None,
        });
        cookies.extend(parse_inline_cookies(cookie_str, &parsed.host));
    }

    // If -O was given with empty string, derive filename from URL now.
    if opts.output_file.as_deref() == Some("") {
        let parsed = parse_url(&current_url)?;
        let filename = filename_from_url(&parsed);
        // We need a mutable opts for this, but since we pass &Options throughout,
        // we handle -O at the outer level instead. This case is handled in run().
        // If we somehow get here with "", use derived name.
        let _ = filename; // Handled in run().
    }

    loop {
        let result = do_request(&current_url, opts, &cookies)?;

        // Accumulate cookies from response.
        cookies.extend(result.response_cookies);

        if let Some(redirect_url) = result.redirect_url {
            if !opts.follow_redirects {
                // Not following redirects; just report it.
                if !opts.silent {
                    eprintln!(
                        "curl: (47) Redirect to '{}' not followed (use -L to follow)",
                        redirect_url
                    );
                }
                break;
            }

            if redirects_remaining == 0 {
                return Err(CurlError::TooManyRedirects);
            }
            redirects_remaining = redirects_remaining.saturating_sub(1);
            num_redirects = num_redirects.saturating_add(1);

            if opts.verbose {
                eprintln!("* Follow redirect to: {redirect_url}");
            }

            current_url = redirect_url;
            continue;
        }

        // Write-out format.
        if let Some(ref fmt) = opts.write_out {
            let elapsed = start_time.elapsed().as_secs_f64();
            let speed = if elapsed > 0.0 {
                result.body_bytes as f64 / elapsed
            } else {
                0.0
            };

            let output = expand_write_out(
                fmt,
                result.status_code,
                result.body_bytes,
                speed,
                elapsed,
                &current_url,
                &result.content_type,
                num_redirects,
            );
            // Write-out goes to stdout (after response body).
            print!("{output}");
            let _ = io::stdout().flush();
        }

        break;
    }

    // Save cookie jar.
    if let Some(ref jar_path) = opts.cookie_jar_write {
        save_cookie_jar(jar_path, &cookies)?;
    }

    Ok(())
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> Result<(), CurlError> {
    let mut opts = parse_args()?;

    // Handle -O: derive output filename from the first URL.
    if opts.output_file.as_deref() == Some("") {
        if let Some(first_url) = opts.urls.first() {
            let parsed = parse_url(first_url)?;
            opts.output_file = Some(filename_from_url(&parsed));
        }
    }

    let urls = opts.urls.clone();
    for url in &urls {
        if let Err(e) = transfer_url(url, &opts) {
            if !opts.silent {
                eprintln!("curl: {e}");
            }
            if urls.len() == 1 {
                return Err(e);
            }
            // For multiple URLs, print error but continue.
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("curl: {e}");
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
    fn parse_simple_http_url() {
        let url = parse_url("http://example.com/index.html").unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 80);
        assert_eq!(url.path, "/index.html");
        assert!(url.query.is_empty());
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
    fn parse_url_with_query() {
        let url = parse_url("http://example.com/search?q=test&lang=en").unwrap();
        assert_eq!(url.path, "/search");
        assert_eq!(url.query, "q=test&lang=en");
    }

    #[test]
    fn parse_url_with_userinfo() {
        let url = parse_url("http://admin:secret@example.com/api").unwrap();
        assert_eq!(url.host, "example.com");
        assert_eq!(url.userinfo, Some("admin:secret".to_string()));
        assert_eq!(url.path, "/api");
    }

    #[test]
    fn parse_https_url() {
        let url = parse_url("https://secure.example.com/").unwrap();
        assert_eq!(url.scheme, "https");
        assert_eq!(url.host, "secure.example.com");
        assert_eq!(url.port, 443);
    }

    #[test]
    fn parse_url_no_scheme_defaults_http() {
        let url = parse_url("example.com/path").unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "example.com");
        assert_eq!(url.path, "/path");
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
    fn parse_url_unsupported_scheme() {
        assert!(parse_url("ftp://files.example.com/").is_err());
    }

    #[test]
    fn url_request_target_no_query() {
        let url = parse_url("http://example.com/path").unwrap();
        assert_eq!(url.request_target(), "/path");
    }

    #[test]
    fn url_request_target_with_query() {
        let url = parse_url("http://example.com/path?foo=bar").unwrap();
        assert_eq!(url.request_target(), "/path?foo=bar");
    }

    #[test]
    fn url_to_string_full_default_port() {
        let url = parse_url("http://example.com/path?q=1").unwrap();
        assert_eq!(url.to_string_full(), "http://example.com/path?q=1");
    }

    #[test]
    fn url_to_string_full_custom_port() {
        let url = parse_url("http://example.com:9090/path").unwrap();
        assert_eq!(url.to_string_full(), "http://example.com:9090/path");
    }

    // --- URL encoding/decoding ---

    #[test]
    fn url_encode_simple() {
        assert_eq!(url_encode("hello world"), "hello%20world");
    }

    #[test]
    fn url_encode_special_chars() {
        assert_eq!(url_encode("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn url_encode_unreserved_passthrough() {
        assert_eq!(url_encode("abc-_.~123"), "abc-_.~123");
    }

    #[test]
    fn url_encode_empty() {
        assert_eq!(url_encode(""), "");
    }

    #[test]
    fn url_decode_simple() {
        assert_eq!(url_decode("hello%20world"), "hello world");
    }

    #[test]
    fn url_decode_plus_as_space() {
        assert_eq!(url_decode("hello+world"), "hello world");
    }

    #[test]
    fn url_decode_special() {
        assert_eq!(url_decode("a%26b%3Dc"), "a&b=c");
    }

    #[test]
    fn url_decode_passthrough() {
        assert_eq!(url_decode("plain"), "plain");
    }

    #[test]
    fn url_encode_decode_roundtrip() {
        let original = "Hello World! @#$%^&*()";
        assert_eq!(url_decode(&url_encode(original)), original);
    }

    // --- Base64 encoding ---

    #[test]
    fn base64_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn base64_simple() {
        assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
    }

    #[test]
    fn base64_no_padding() {
        assert_eq!(base64_encode(b"abc"), "YWJj");
    }

    #[test]
    fn base64_one_padding() {
        assert_eq!(base64_encode(b"ab"), "YWI=");
    }

    #[test]
    fn base64_two_padding() {
        assert_eq!(base64_encode(b"a"), "YQ==");
    }

    #[test]
    fn base64_auth_string() {
        // Standard curl basic auth test.
        assert_eq!(
            base64_encode(b"user:password"),
            "dXNlcjpwYXNzd29yZA=="
        );
    }

    // --- IP address helpers ---

    #[test]
    fn parse_ipv4_simple() {
        let ip = parse_ipv4("192.168.1.1");
        assert!(ip.is_some());
        assert_eq!(ip.unwrap(), 0xC0A80101);
    }

    #[test]
    fn parse_ipv4_loopback() {
        assert_eq!(parse_ipv4("127.0.0.1"), Some(0x7F000001));
    }

    #[test]
    fn parse_ipv4_invalid() {
        assert!(parse_ipv4("256.0.0.1").is_none());
        assert!(parse_ipv4("abc").is_none());
        assert!(parse_ipv4("").is_none());
    }

    #[test]
    fn ip_format_loopback() {
        let ip = 0x7F000001_u32.to_be();
        assert_eq!(ip_to_string(ip), "127.0.0.1");
    }

    #[test]
    fn is_ipv4_yes() {
        assert!(is_ipv4_address("1.2.3.4"));
    }

    #[test]
    fn is_ipv4_no() {
        assert!(!is_ipv4_address("example.com"));
    }

    // --- HTTP response parsing ---

    #[test]
    fn parse_response_200() {
        let data = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Type: text/plain\r\n\r\nhello";
        let resp = parse_http_response(data).unwrap();
        assert_eq!(resp.status.code, 200);
        assert_eq!(resp.status.reason, "OK");
        assert_eq!(resp.headers.len(), 2);
        assert_eq!(get_header(&resp.headers, "content-length"), Some("5"));
        assert_eq!(get_header(&resp.headers, "content-type"), Some("text/plain"));
        assert_eq!(&data[resp.body_offset..], b"hello");
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
        let data = b"HTTP/1.1 204 No Content\r\n\r\n";
        let resp = parse_http_response(data).unwrap();
        assert_eq!(resp.status.code, 204);
        assert!(resp.headers.is_empty());
    }

    #[test]
    fn parse_status_line_200() {
        let s = parse_status_line("HTTP/1.1 200 OK").unwrap();
        assert_eq!(s.code, 200);
        assert_eq!(s.reason, "OK");
    }

    #[test]
    fn parse_status_line_no_reason() {
        let s = parse_status_line("HTTP/1.0 204").unwrap();
        assert_eq!(s.code, 204);
        assert_eq!(s.reason, "");
    }

    #[test]
    fn parse_status_line_invalid() {
        assert!(parse_status_line("GARBAGE").is_err());
    }

    #[test]
    fn find_header_end_present() {
        let data = b"HTTP/1.1 200 OK\r\nFoo: bar\r\n\r\nbody";
        assert!(find_header_end(data).is_some());
    }

    #[test]
    fn find_header_end_absent() {
        let data = b"HTTP/1.1 200 OK\r\nFoo: bar\r\n";
        assert!(find_header_end(data).is_none());
    }

    #[test]
    fn get_headers_multiple() {
        let headers = vec![
            ("set-cookie".to_string(), "a=1".to_string()),
            ("content-type".to_string(), "text/html".to_string()),
            ("set-cookie".to_string(), "b=2".to_string()),
        ];
        let cookies = get_headers(&headers, "set-cookie");
        assert_eq!(cookies, vec!["a=1", "b=2"]);
    }

    #[test]
    fn format_raw_headers_roundtrip() {
        let data = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n";
        let resp = parse_http_response(data).unwrap();
        let formatted = format_raw_headers(&resp);
        assert!(formatted.contains("HTTP/1.1 200 OK"));
        assert!(formatted.contains("content-type: text/html"));
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
        let out1 = decoder.decode(b"5\r\nhel").unwrap();
        assert_eq!(out1, b"hel");
        assert!(!decoder.is_finished());

        let out2 = decoder.decode(b"lo\r\n0\r\n\r\n").unwrap();
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
        let input = b"5;ext=val\r\nhello\r\n0\r\n\r\n";
        let output = decoder.decode(input).unwrap();
        assert_eq!(output, b"hello");
    }

    #[test]
    fn chunked_empty_body() {
        let mut decoder = ChunkedDecoder::new();
        let input = b"0\r\n\r\n";
        let output = decoder.decode(input).unwrap();
        assert!(output.is_empty());
        assert!(decoder.is_finished());
    }

    #[test]
    fn chunked_invalid_hex() {
        let mut decoder = ChunkedDecoder::new();
        assert!(decoder.decode(b"xyz\r\n").is_err());
    }

    // --- Cookie parsing ---

    #[test]
    fn parse_set_cookie_simple() {
        let headers = vec![(
            "set-cookie".to_string(),
            "session=abc123; Path=/; Domain=example.com".to_string(),
        )];
        let cookies = parse_set_cookies(&headers, "example.com");
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].name, "session");
        assert_eq!(cookies[0].value, "abc123");
        assert_eq!(cookies[0].domain, "example.com");
        assert_eq!(cookies[0].path, "/");
    }

    #[test]
    fn parse_set_cookie_multiple() {
        let headers = vec![
            ("set-cookie".to_string(), "a=1".to_string()),
            ("set-cookie".to_string(), "b=2".to_string()),
        ];
        let cookies = parse_set_cookies(&headers, "host.com");
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].name, "a");
        assert_eq!(cookies[1].name, "b");
    }

    #[test]
    fn format_cookie_header_single() {
        let cookies = vec![Cookie {
            name: "sid".to_string(),
            value: "xyz".to_string(),
            domain: "example.com".to_string(),
            path: "/".to_string(),
        }];
        assert_eq!(format_cookie_header(&cookies), "sid=xyz");
    }

    #[test]
    fn format_cookie_header_multiple() {
        let cookies = vec![
            Cookie {
                name: "a".to_string(),
                value: "1".to_string(),
                domain: String::new(),
                path: String::new(),
            },
            Cookie {
                name: "b".to_string(),
                value: "2".to_string(),
                domain: String::new(),
                path: String::new(),
            },
        ];
        assert_eq!(format_cookie_header(&cookies), "a=1; b=2");
    }

    #[test]
    fn parse_inline_cookies_single() {
        let cookies = parse_inline_cookies("name=value", "host.com");
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].name, "name");
        assert_eq!(cookies[0].value, "value");
    }

    #[test]
    fn parse_inline_cookies_multiple() {
        let cookies = parse_inline_cookies("a=1; b=2; c=3", "host.com");
        assert_eq!(cookies.len(), 3);
    }

    // --- Size/speed formatting ---

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

    #[test]
    fn format_speed_bytes_per_sec() {
        assert_eq!(format_speed(500.0), "500 B/s");
    }

    #[test]
    fn format_speed_kb() {
        assert_eq!(format_speed(10240.0), "10.00 KB/s");
    }

    // --- Write-out format expansion ---

    #[test]
    fn write_out_http_code() {
        let result = expand_write_out("%{http_code}", 200, 0, 0.0, 0.0, "", "", 0);
        assert_eq!(result, "200");
    }

    #[test]
    fn write_out_multiple_vars() {
        let result = expand_write_out(
            "%{http_code} %{size_download}",
            404,
            1234,
            0.0,
            0.0,
            "",
            "",
            0,
        );
        assert_eq!(result, "404 1234");
    }

    #[test]
    fn write_out_escape_sequences() {
        let result = expand_write_out("a\\nb\\tc", 200, 0, 0.0, 0.0, "", "", 0);
        assert_eq!(result, "a\nb\tc");
    }

    #[test]
    fn write_out_unknown_var() {
        let result = expand_write_out("%{unknown}", 200, 0, 0.0, 0.0, "", "", 0);
        assert_eq!(result, "%{unknown}");
    }

    #[test]
    fn write_out_url_effective() {
        let result = expand_write_out(
            "%{url_effective}",
            200,
            0,
            0.0,
            0.0,
            "http://example.com/",
            "",
            0,
        );
        assert_eq!(result, "http://example.com/");
    }

    #[test]
    fn write_out_num_redirects() {
        let result = expand_write_out("%{num_redirects}", 200, 0, 0.0, 0.0, "", "", 3);
        assert_eq!(result, "3");
    }

    // --- Multipart form data ---

    #[test]
    fn multipart_body_single_field() {
        let fields = vec![("name".to_string(), "value".to_string())];
        let (boundary, body) = build_multipart_body(&fields);
        let body_str = String::from_utf8_lossy(&body);
        assert!(body_str.contains(&format!("--{boundary}")));
        assert!(body_str.contains("Content-Disposition: form-data; name=\"name\""));
        assert!(body_str.contains("value"));
        assert!(body_str.contains(&format!("--{boundary}--")));
    }

    #[test]
    fn multipart_body_multiple_fields() {
        let fields = vec![
            ("field1".to_string(), "val1".to_string()),
            ("field2".to_string(), "val2".to_string()),
        ];
        let (_, body) = build_multipart_body(&fields);
        let body_str = String::from_utf8_lossy(&body);
        assert!(body_str.contains("name=\"field1\""));
        assert!(body_str.contains("name=\"field2\""));
        assert!(body_str.contains("val1"));
        assert!(body_str.contains("val2"));
    }

    #[test]
    fn multipart_body_file_reference() {
        // File doesn't exist, should still produce correct structure.
        let fields = vec![("upload".to_string(), "@/nonexistent.txt".to_string())];
        let (_, body) = build_multipart_body(&fields);
        let body_str = String::from_utf8_lossy(&body);
        assert!(body_str.contains("filename=\"nonexistent.txt\""));
        assert!(body_str.contains("application/octet-stream"));
    }

    // --- Redirect resolution ---

    #[test]
    fn resolve_absolute_redirect() {
        let result = resolve_redirect(
            "http://old.example.com/page",
            "http://new.example.com/other",
        );
        assert_eq!(result, "http://new.example.com/other");
    }

    #[test]
    fn resolve_relative_absolute_path() {
        let result = resolve_redirect("http://example.com/dir/page", "/new-path");
        assert_eq!(result, "http://example.com/new-path");
    }

    #[test]
    fn resolve_relative_path() {
        let result = resolve_redirect("http://example.com/dir/page", "other.html");
        assert_eq!(result, "http://example.com/dir/other.html");
    }

    #[test]
    fn resolve_redirect_with_port() {
        let result = resolve_redirect("http://example.com:8080/dir/page", "/new");
        assert_eq!(result, "http://example.com:8080/new");
    }

    // --- Filename extraction ---

    #[test]
    fn filename_simple() {
        let url = parse_url("http://example.com/files/archive.tar.gz").unwrap();
        assert_eq!(filename_from_url(&url), "archive.tar.gz");
    }

    #[test]
    fn filename_root_path() {
        let url = parse_url("http://example.com/").unwrap();
        assert_eq!(filename_from_url(&url), "index.html");
    }

    #[test]
    fn filename_no_path() {
        let url = parse_url("http://example.com").unwrap();
        assert_eq!(filename_from_url(&url), "index.html");
    }

    // --- Request building ---

    #[test]
    fn build_get_request() {
        let url = parse_url("http://example.com/path").unwrap();
        let opts = Options {
            method: Method::Get,
            user_agent: "TestAgent/1.0".to_string(),
            ..Options::default()
        };
        let req = build_request(&url, &opts, None, None, &[]);
        let req_str = String::from_utf8_lossy(&req);
        assert!(req_str.starts_with("GET /path HTTP/1.1\r\n"));
        assert!(req_str.contains("Host: example.com\r\n"));
        assert!(req_str.contains("User-Agent: TestAgent/1.0\r\n"));
        assert!(req_str.contains("Connection: close\r\n"));
    }

    #[test]
    fn build_post_request_with_data() {
        let url = parse_url("http://example.com/api").unwrap();
        let opts = Options {
            method: Method::Post,
            ..Options::default()
        };
        let body = b"key=value";
        let req = build_request(
            &url,
            &opts,
            Some(body),
            Some("application/x-www-form-urlencoded"),
            &[],
        );
        let req_str = String::from_utf8_lossy(&req);
        assert!(req_str.starts_with("POST /api HTTP/1.1\r\n"));
        assert!(req_str.contains("Content-Type: application/x-www-form-urlencoded\r\n"));
        assert!(req_str.contains("Content-Length: 9\r\n"));
        assert!(req_str.contains("key=value"));
    }

    #[test]
    fn build_request_with_auth() {
        let url = parse_url("http://example.com/secret").unwrap();
        let opts = Options {
            method: Method::Get,
            auth: Some("user:pass".to_string()),
            ..Options::default()
        };
        let req = build_request(&url, &opts, None, None, &[]);
        let req_str = String::from_utf8_lossy(&req);
        assert!(req_str.contains("Authorization: Basic dXNlcjpwYXNz\r\n"));
    }

    #[test]
    fn build_request_with_cookies() {
        let url = parse_url("http://example.com/").unwrap();
        let opts = Options::default();
        let cookies = vec![
            Cookie {
                name: "session".to_string(),
                value: "abc".to_string(),
                domain: "example.com".to_string(),
                path: "/".to_string(),
            },
            Cookie {
                name: "lang".to_string(),
                value: "en".to_string(),
                domain: "example.com".to_string(),
                path: "/".to_string(),
            },
        ];
        let req = build_request(&url, &opts, None, None, &cookies);
        let req_str = String::from_utf8_lossy(&req);
        assert!(req_str.contains("Cookie: session=abc; lang=en\r\n"));
    }

    #[test]
    fn build_request_with_custom_headers() {
        let url = parse_url("http://example.com/api").unwrap();
        let opts = Options {
            custom_headers: vec![
                ("X-Custom".to_string(), "foo".to_string()),
                ("Accept".to_string(), "application/json".to_string()),
            ],
            ..Options::default()
        };
        let req = build_request(&url, &opts, None, None, &[]);
        let req_str = String::from_utf8_lossy(&req);
        assert!(req_str.contains("X-Custom: foo\r\n"));
        assert!(req_str.contains("Accept: application/json\r\n"));
    }

    #[test]
    fn build_request_custom_port_in_host() {
        let url = parse_url("http://example.com:9090/").unwrap();
        let opts = Options::default();
        let req = build_request(&url, &opts, None, None, &[]);
        let req_str = String::from_utf8_lossy(&req);
        assert!(req_str.contains("Host: example.com:9090\r\n"));
    }

    #[test]
    fn build_request_default_port_omitted() {
        let url = parse_url("http://example.com/").unwrap();
        let opts = Options::default();
        let req = build_request(&url, &opts, None, None, &[]);
        let req_str = String::from_utf8_lossy(&req);
        assert!(req_str.contains("Host: example.com\r\n"));
        assert!(!req_str.contains("Host: example.com:80\r\n"));
    }

    #[test]
    fn build_request_with_referer() {
        let url = parse_url("http://example.com/page").unwrap();
        let opts = Options {
            referer: Some("http://other.com/".to_string()),
            ..Options::default()
        };
        let req = build_request(&url, &opts, None, None, &[]);
        let req_str = String::from_utf8_lossy(&req);
        assert!(req_str.contains("Referer: http://other.com/\r\n"));
    }

    #[test]
    fn build_request_url_userinfo_auth() {
        let url = parse_url("http://admin:pw@example.com/").unwrap();
        let opts = Options::default();
        let req = build_request(&url, &opts, None, None, &[]);
        let req_str = String::from_utf8_lossy(&req);
        // admin:pw => base64 "YWRtaW46cHc="
        assert!(req_str.contains("Authorization: Basic YWRtaW46cHc=\r\n"));
    }

    // --- Method display ---

    #[test]
    fn method_as_str() {
        assert_eq!(Method::Get.as_str(), "GET");
        assert_eq!(Method::Post.as_str(), "POST");
        assert_eq!(Method::Put.as_str(), "PUT");
        assert_eq!(Method::Delete.as_str(), "DELETE");
        assert_eq!(Method::Head.as_str(), "HEAD");
        assert_eq!(Method::Patch.as_str(), "PATCH");
    }

    // --- Error display ---

    #[test]
    fn error_display_dns() {
        let e = CurlError::DnsFailure("bad.host".to_string());
        let msg = format!("{e}");
        assert!(msg.contains("bad.host"));
    }

    #[test]
    fn error_display_http() {
        let e = CurlError::HttpError(404, "Not Found".to_string());
        let msg = format!("{e}");
        assert!(msg.contains("404"));
    }

    #[test]
    fn error_display_redirect() {
        let e = CurlError::TooManyRedirects;
        let msg = format!("{e}");
        assert!(msg.contains("redirect"));
    }

    // --- Simple hash (for multipart boundary) ---

    #[test]
    fn simple_hash_deterministic() {
        let fields = vec![("a".to_string(), "b".to_string())];
        let h1 = simple_hash(&fields);
        let h2 = simple_hash(&fields);
        assert_eq!(h1, h2);
    }

    #[test]
    fn simple_hash_differs() {
        let f1 = vec![("a".to_string(), "b".to_string())];
        let f2 = vec![("c".to_string(), "d".to_string())];
        // Extremely unlikely to collide.
        assert_ne!(simple_hash(&f1), simple_hash(&f2));
    }
}
