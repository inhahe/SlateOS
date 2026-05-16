//! HTTP/1.1 and HTTPS client implementation (dual-stack IPv4/IPv6).
//!
//! Provides a minimal but functional HTTP/HTTPS client built on top of the
//! kernel's TCP stack, DNS resolver, and TLS 1.3 implementation.  Supports
//! the most common operations needed by OS services (package manager, update
//! checks, API calls, UPnP SOAP).  Automatically falls back to IPv6 (AAAA)
//! when IPv4 (A) DNS resolution fails.
//!
//! ## Features
//!
//! - **Methods**: GET, HEAD, POST, PUT, DELETE, PATCH
//! - **URL parsing**: `http://` and `https://` URL decomposition
//! - **HTTPS**: TLS 1.3 with ChaCha20-Poly1305 and X25519 key exchange
//! - **DNS resolution**: hostname → IP via the kernel DNS resolver
//! - **Request building**: proper HTTP/1.1 request formatting with Host,
//!   Content-Length, Content-Type, User-Agent, Connection headers
//! - **Response parsing**: status line, headers, body extraction
//! - **Chunked transfer encoding**: reassembles chunked responses
//! - **Redirects**: follows 301, 302, 307, 308 up to 5 hops (cross-scheme OK)
//! - **Basic authentication**: Base64-encoded `Authorization` header
//! - **Connection reuse**: optional keep-alive (default: close after request)
//! - **Configurable timeouts**: per-request poll-cycle limits
//!
//! ## Limitations
//!
//! - No certificate chain validation (accepts any server certificate).
//! - No cookie jar (stateless requests).
//! - No multipart form upload.
//! - Response body buffered entirely in memory (no streaming).
//! - Single-threaded (one request at a time per call).
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::net::http;
//!
//! // Simple GET request.
//! let resp = http::get("http://example.com/api/status")?;
//! assert_eq!(resp.status_code, 200);
//!
//! // POST with body.
//! let resp = http::post(
//!     "http://example.com/api/data",
//!     b"key=value",
//!     Some("application/x-www-form-urlencoded"),
//! )?;
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use crate::error::{KernelError, KernelResult};
use super::interface::{IpAddr, Ipv4Addr};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of HTTP redirects to follow.
const MAX_REDIRECTS: u8 = 5;

/// Default timeout in poll cycles for TCP operations.
/// Each poll cycle includes a spin-loop pause, so ~50k cycles ≈ several seconds.
const DEFAULT_TIMEOUT_POLLS: u32 = 50_000;

/// Maximum response body size (4 MiB) to prevent OOM from huge responses.
const MAX_BODY_SIZE: usize = 4 * 1024 * 1024;

/// Maximum response header section size (64 KiB).
const MAX_HEADER_SIZE: usize = 64 * 1024;

/// Maximum single header line length (8 KiB).
const MAX_HEADER_LINE: usize = 8 * 1024;

/// Maximum number of response headers.
const MAX_HEADER_COUNT: usize = 128;

/// Default HTTP port.
const DEFAULT_HTTP_PORT: u16 = 80;

/// User-Agent string identifying our OS.
const USER_AGENT: &str = "NeoKernel/0.1 (HTTP/1.1)";

// ---------------------------------------------------------------------------
// HTTP method
// ---------------------------------------------------------------------------

/// HTTP request method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // All methods are part of the public API.
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Patch,
}

impl Method {
    /// Returns the method string for the request line.
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
        }
    }
}

// ---------------------------------------------------------------------------
// URL parsing
// ---------------------------------------------------------------------------

/// Default HTTPS port.
const DEFAULT_HTTPS_PORT: u16 = 443;

/// Parsed HTTP/HTTPS URL components.
#[derive(Debug, Clone)]
pub struct Url {
    /// Hostname (e.g., "example.com").
    pub host: String,
    /// Port number (default 80 for HTTP, 443 for HTTPS).
    pub port: u16,
    /// Request path (e.g., "/api/status").
    pub path: String,
    /// Optional query string (without leading '?').
    pub query: Option<String>,
    /// True if the URL uses HTTPS scheme.
    pub is_https: bool,
}

impl Url {
    /// Parse an HTTP or HTTPS URL string.
    ///
    /// Supports `http://host[:port][/path][?query]` and
    /// `https://host[:port][/path][?query]`.
    pub fn parse(url: &str) -> KernelResult<Self> {
        // Strip scheme and determine if HTTPS.
        let (rest, is_https) = if let Some(stripped) = url.strip_prefix("https://") {
            (stripped, true)
        } else if let Some(stripped) = url.strip_prefix("http://") {
            (stripped, false)
        } else {
            // Assume bare URL without scheme (defaults to HTTP).
            (url, false)
        };

        // Split path from authority.
        let (authority, path_and_query) = match rest.find('/') {
            Some(idx) => {
                let (a, p) = rest.split_at(idx);
                (a, p)
            }
            None => (rest, "/"),
        };

        // Split query from path.
        let (path, query) = match path_and_query.find('?') {
            Some(idx) => {
                let (p, q) = path_and_query.split_at(idx);
                // Skip the '?' character.
                let q_str = if q.len() > 1 { Some(String::from(&q[1..])) } else { None };
                (p, q_str)
            }
            None => (path_and_query, None),
        };

        // Split port from host.
        let default_port = if is_https { DEFAULT_HTTPS_PORT } else { DEFAULT_HTTP_PORT };
        let (host, port) = match authority.rfind(':') {
            Some(idx) => {
                let h = &authority[..idx];
                let p_str = &authority[idx.saturating_add(1)..];
                let p = parse_u16(p_str).ok_or(KernelError::InvalidArgument)?;
                (h, p)
            }
            None => (authority, default_port),
        };

        if host.is_empty() {
            return Err(KernelError::InvalidArgument);
        }

        let path_str = if path.is_empty() { String::from("/") } else { String::from(path) };

        Ok(Self {
            host: String::from(host),
            port,
            path: path_str,
            query,
            is_https,
        })
    }

    /// Build the request URI (path + query).
    pub fn request_uri(&self) -> String {
        match &self.query {
            Some(q) => format!("{}?{}", self.path, q),
            None => self.path.clone(),
        }
    }

    /// Build the Host header value (host:port if non-default).
    pub fn host_header(&self) -> String {
        if self.port == DEFAULT_HTTP_PORT {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP request builder
// ---------------------------------------------------------------------------

/// An HTTP request to send.
#[derive(Debug)]
pub struct Request {
    /// HTTP method.
    pub method: Method,
    /// Parsed URL.
    pub url: Url,
    /// Extra headers (name, value) pairs.
    pub headers: Vec<(String, String)>,
    /// Request body (for POST, PUT, PATCH).
    pub body: Option<Vec<u8>>,
    /// Content-Type header for the body.
    pub content_type: Option<String>,
    /// Basic auth credentials (username, password).
    pub basic_auth: Option<(String, String)>,
    /// Timeout in poll cycles.
    pub timeout_polls: u32,
    /// Whether to follow redirects.
    pub follow_redirects: bool,
}

#[allow(dead_code)] // Builder API — not all methods used yet.
impl Request {
    /// Create a new request with defaults.
    pub fn new(method: Method, url: &str) -> KernelResult<Self> {
        let parsed = Url::parse(url)?;
        Ok(Self {
            method,
            url: parsed,
            headers: Vec::new(),
            body: None,
            content_type: None,
            basic_auth: None,
            timeout_polls: DEFAULT_TIMEOUT_POLLS,
            follow_redirects: true,
        })
    }

    /// Set a custom header.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((String::from(name), String::from(value)));
        self
    }

    /// Set the request body.
    pub fn body_bytes(mut self, data: &[u8], content_type: &str) -> Self {
        self.body = Some(Vec::from(data));
        self.content_type = Some(String::from(content_type));
        self
    }

    /// Set basic authentication.
    pub fn basic_auth_creds(mut self, user: &str, pass: &str) -> Self {
        self.basic_auth = Some((String::from(user), String::from(pass)));
        self
    }

    /// Set timeout in poll cycles.
    pub fn timeout(mut self, polls: u32) -> Self {
        self.timeout_polls = polls;
        self
    }

    /// Disable redirect following.
    pub fn no_redirects(mut self) -> Self {
        self.follow_redirects = false;
        self
    }

    /// Build the raw HTTP request bytes.
    fn build(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(512);
        let uri = self.url.request_uri();

        // Request line.
        buf.extend_from_slice(self.method.as_str().as_bytes());
        buf.push(b' ');
        buf.extend_from_slice(uri.as_bytes());
        buf.extend_from_slice(b" HTTP/1.1\r\n");

        // Host header (required for HTTP/1.1).
        buf.extend_from_slice(b"Host: ");
        buf.extend_from_slice(self.url.host_header().as_bytes());
        buf.extend_from_slice(b"\r\n");

        // User-Agent.
        buf.extend_from_slice(b"User-Agent: ");
        buf.extend_from_slice(USER_AGENT.as_bytes());
        buf.extend_from_slice(b"\r\n");

        // Connection: close (we don't do persistent connections by default).
        buf.extend_from_slice(b"Connection: close\r\n");

        // Content-Type and Content-Length for bodies.
        if let Some(ref body) = self.body {
            if let Some(ref ct) = self.content_type {
                buf.extend_from_slice(b"Content-Type: ");
                buf.extend_from_slice(ct.as_bytes());
                buf.extend_from_slice(b"\r\n");
            }
            let len_str = format_usize(body.len());
            buf.extend_from_slice(b"Content-Length: ");
            buf.extend_from_slice(len_str.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }

        // Basic auth.
        if let Some((ref user, ref pass)) = self.basic_auth {
            let creds = format!("{}:{}", user, pass);
            let encoded = base64_encode(creds.as_bytes());
            buf.extend_from_slice(b"Authorization: Basic ");
            buf.extend_from_slice(encoded.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }

        // Custom headers.
        for (name, value) in &self.headers {
            buf.extend_from_slice(name.as_bytes());
            buf.extend_from_slice(b": ");
            buf.extend_from_slice(value.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }

        // Accept all content types if not specified.
        let has_accept = self.headers.iter().any(|(n, _)| {
            n.eq_ignore_ascii_case("Accept")
        });
        if !has_accept {
            buf.extend_from_slice(b"Accept: */*\r\n");
        }

        // End of headers.
        buf.extend_from_slice(b"\r\n");

        // Body.
        if let Some(ref body) = self.body {
            buf.extend_from_slice(body);
        }

        buf
    }

    /// Execute the request and return the response.
    pub fn send_request(self) -> KernelResult<Response> {
        execute_request(self, 0)
    }
}

// ---------------------------------------------------------------------------
// HTTP response
// ---------------------------------------------------------------------------

/// Parsed HTTP response.
#[derive(Debug)]
pub struct Response {
    /// HTTP status code (e.g., 200, 404, 500).
    pub status_code: u16,
    /// HTTP reason phrase (e.g., "OK", "Not Found").
    pub reason: String,
    /// Response headers (name, value) pairs.
    pub headers: Vec<(String, String)>,
    /// Response body.
    pub body: Vec<u8>,
}

#[allow(dead_code)] // Response API — not all methods used from kshell yet.
impl Response {
    /// Check if the status code indicates success (2xx).
    pub fn is_success(&self) -> bool {
        self.status_code >= 200 && self.status_code < 300
    }

    /// Check if the status code indicates a redirect (3xx).
    pub fn is_redirect(&self) -> bool {
        self.status_code >= 300 && self.status_code < 400
    }

    /// Get the value of a header (case-insensitive lookup).
    pub fn header(&self, name: &str) -> Option<&str> {
        for (n, v) in &self.headers {
            if n.eq_ignore_ascii_case(name) {
                return Some(v.as_str());
            }
        }
        None
    }

    /// Get Content-Length header value, if present and valid.
    pub fn content_length(&self) -> Option<usize> {
        self.header("Content-Length").and_then(|v| parse_usize(v))
    }

    /// Get the body as a UTF-8 string (lossy).
    pub fn body_text(&self) -> String {
        // Manually convert: replace invalid UTF-8 sequences with nothing.
        // We avoid from_utf8_lossy per project rules — instead, skip
        // non-UTF-8 bytes entirely.
        match core::str::from_utf8(&self.body) {
            Ok(s) => String::from(s),
            Err(_) => {
                // Best-effort: take only valid UTF-8 prefix.
                let valid_len = valid_utf8_prefix_len(&self.body);
                if let Some(slice) = self.body.get(..valid_len) {
                    if let Ok(s) = core::str::from_utf8(slice) {
                        return String::from(s);
                    }
                }
                String::new()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience functions
// ---------------------------------------------------------------------------

/// Perform an HTTP GET request.
#[allow(dead_code)] // Public API.
pub fn get(url: &str) -> KernelResult<Response> {
    Request::new(Method::Get, url)?.send_request()
}

/// Perform an HTTP HEAD request (no body in response).
#[allow(dead_code)] // Public API.
pub fn head(url: &str) -> KernelResult<Response> {
    Request::new(Method::Head, url)?.send_request()
}

/// Perform an HTTP POST request with a body.
#[allow(dead_code)] // Public API.
pub fn post(url: &str, body: &[u8], content_type: Option<&str>) -> KernelResult<Response> {
    let mut req = Request::new(Method::Post, url)?;
    req.body = Some(Vec::from(body));
    req.content_type = content_type.map(String::from);
    req.send_request()
}

/// Perform an HTTP PUT request with a body.
#[allow(dead_code)] // Public API.
pub fn put(url: &str, body: &[u8], content_type: Option<&str>) -> KernelResult<Response> {
    let mut req = Request::new(Method::Put, url)?;
    req.body = Some(Vec::from(body));
    req.content_type = content_type.map(String::from);
    req.send_request()
}

/// Perform an HTTP DELETE request.
#[allow(dead_code)] // Public API.
pub fn delete(url: &str) -> KernelResult<Response> {
    Request::new(Method::Delete, url)?.send_request()
}

// ---------------------------------------------------------------------------
// Request execution engine
// ---------------------------------------------------------------------------

/// Execute an HTTP(S) request, following redirects up to `MAX_REDIRECTS`.
fn execute_request(req: Request, redirect_count: u8) -> KernelResult<Response> {
    if redirect_count > MAX_REDIRECTS {
        crate::serial_println!("[http] Too many redirects ({})", redirect_count);
        return Err(KernelError::ResourceExhausted);
    }

    // Resolve hostname to IP.
    let ip = resolve_host(&req.url.host)?;

    let scheme = if req.url.is_https { "https" } else { "http" };
    crate::serial_println!(
        "[{}] {} {} ({}:{})",
        scheme, req.method.as_str(), req.url.request_uri(), ip, req.url.port
    );

    // Connect to the remote server.
    let handle = super::tcp::connect(ip.into(), req.url.port)?;

    // Build the raw HTTP request bytes.
    let raw_request = req.build();

    let resp = if req.url.is_https {
        // --- HTTPS: wrap the TCP connection with TLS ---
        let tls_session = match super::tls::tls_connect(handle, &req.url.host) {
            Ok(s) => s,
            Err(e) => {
                let _ = super::tcp::close(handle);
                return Err(e);
            }
        };
        execute_https_request(tls_session, &raw_request, req.timeout_polls, req.method == Method::Head)?
    } else {
        // --- HTTP: plain TCP ---
        let send_result = super::tcp::send(handle, &raw_request);
        if let Err(e) = send_result {
            let _ = super::tcp::close(handle);
            return Err(e);
        }

        let response = read_response(handle, req.timeout_polls, req.method == Method::Head);
        let _ = super::tcp::close(handle);
        response?
    };

    // Handle redirects.
    if req.follow_redirects && resp.is_redirect() {
        if let Some(location) = resp.header("Location") {
            let redirect_url = resolve_redirect(&req.url, location);
            crate::serial_println!(
                "[http] Redirect {} → {}",
                resp.status_code, redirect_url
            );

            // 307 and 308 preserve the method; 301 and 302 change to GET.
            let new_method = match resp.status_code {
                307 | 308 => req.method,
                _ => Method::Get,
            };

            let mut new_req = Request::new(new_method, &redirect_url)?;
            new_req.timeout_polls = req.timeout_polls;
            new_req.follow_redirects = true;
            // Preserve body for 307/308 redirects.
            if new_method != Method::Get && new_method != Method::Head {
                new_req.body = req.body;
                new_req.content_type = req.content_type;
            }
            new_req.basic_auth = req.basic_auth;
            // Carry over custom headers.
            new_req.headers = req.headers;

            return execute_request(new_req, redirect_count.saturating_add(1));
        }
    }

    Ok(resp)
}

/// Execute an HTTPS request using a TLS session.
///
/// Sends the raw HTTP request over TLS, then reads and parses the response.
fn execute_https_request(
    mut tls: super::tls::TlsSession,
    raw_request: &[u8],
    timeout_polls: u32,
    head_only: bool,
) -> KernelResult<Response> {
    // Send the HTTP request over TLS.
    super::tls::tls_send(&mut tls, raw_request)?;

    // Read the response via TLS.
    let response = read_tls_response(&mut tls, timeout_polls, head_only);

    // Close the TLS session.
    let _ = super::tls::tls_close(&mut tls);

    response
}

/// Read an HTTP response from a TLS session.
///
/// Mirrors `read_response()` but reads from TLS instead of raw TCP.
fn read_tls_response(
    tls: &mut super::tls::TlsSession,
    timeout_polls: u32,
    head_only: bool,
) -> KernelResult<Response> {
    // Accumulate raw bytes until we find the end of headers (\r\n\r\n).
    let mut raw = Vec::with_capacity(4096);
    let header_end = read_tls_until_header_end(tls, timeout_polls, &mut raw)?;

    // Parse status line and headers from the raw bytes.
    let header_section = raw.get(..header_end).ok_or(KernelError::InternalError)?;
    let (status_code, reason, headers) = parse_response_headers(header_section)?;

    // For HEAD requests, there's no body.
    if head_only {
        return Ok(Response {
            status_code,
            reason,
            headers,
            body: Vec::new(),
        });
    }

    // The rest of `raw` after the headers (data already read but part of body).
    let body_start = header_end.checked_add(4).unwrap_or(header_end);
    let already_read = if body_start < raw.len() {
        Vec::from(raw.get(body_start..).unwrap_or(&[]))
    } else {
        Vec::new()
    };

    // Determine how to read the body.
    let is_chunked = headers.iter().any(|(n, v)| {
        n.eq_ignore_ascii_case("Transfer-Encoding") && v.contains("chunked")
    });

    let body = if is_chunked {
        read_tls_chunked_body(tls, timeout_polls, already_read)?
    } else {
        let content_length = headers.iter().find_map(|(n, v)| {
            if n.eq_ignore_ascii_case("Content-Length") {
                parse_usize(v)
            } else {
                None
            }
        });
        read_tls_fixed_body(tls, timeout_polls, already_read, content_length)?
    };

    Ok(Response {
        status_code,
        reason,
        headers,
        body,
    })
}

/// Read from TLS until we find the header/body boundary (\r\n\r\n).
fn read_tls_until_header_end(
    tls: &mut super::tls::TlsSession,
    timeout_polls: u32,
    buf: &mut Vec<u8>,
) -> KernelResult<usize> {
    let mut polls = 0u32;
    loop {
        if let Some(pos) = find_header_end(buf) {
            return Ok(pos);
        }
        if buf.len() > MAX_HEADER_SIZE {
            return Err(KernelError::ResourceExhausted);
        }

        super::super::net::poll();
        let data = super::tls::tls_recv(tls, 4096)?;
        if !data.is_empty() {
            buf.extend_from_slice(&data);
            polls = 0;
        } else {
            polls = polls.saturating_add(1);
            if polls >= timeout_polls {
                return Err(KernelError::TimedOut);
            }
            core::hint::spin_loop();
        }
    }
}

/// Read a fixed-length body from TLS.
fn read_tls_fixed_body(
    tls: &mut super::tls::TlsSession,
    timeout_polls: u32,
    already_read: Vec<u8>,
    content_length: Option<usize>,
) -> KernelResult<Vec<u8>> {
    let mut body = already_read;

    match content_length {
        Some(expected) => {
            if expected > MAX_BODY_SIZE {
                return Err(KernelError::ResourceExhausted);
            }
            let mut polls = 0u32;
            while body.len() < expected {
                super::super::net::poll();
                let remaining = expected.saturating_sub(body.len());
                let data = super::tls::tls_recv(tls, remaining.min(8192))?;
                if !data.is_empty() {
                    body.extend_from_slice(&data);
                    polls = 0;
                } else {
                    polls = polls.saturating_add(1);
                    if polls >= timeout_polls {
                        break; // Return what we have.
                    }
                    core::hint::spin_loop();
                }
            }
        }
        None => {
            // No Content-Length — read until connection close or timeout.
            let mut polls = 0u32;
            loop {
                super::super::net::poll();
                let data = super::tls::tls_recv(tls, 8192)?;
                if !data.is_empty() {
                    body.extend_from_slice(&data);
                    polls = 0;
                    if body.len() > MAX_BODY_SIZE {
                        break;
                    }
                } else {
                    polls = polls.saturating_add(1);
                    if polls >= timeout_polls {
                        break;
                    }
                    core::hint::spin_loop();
                }
            }
        }
    }

    Ok(body)
}

/// Read chunked-encoded body from TLS.
fn read_tls_chunked_body(
    tls: &mut super::tls::TlsSession,
    timeout_polls: u32,
    already_read: Vec<u8>,
) -> KernelResult<Vec<u8>> {
    let mut buf = already_read;
    let mut body = Vec::new();
    let mut polls = 0u32;

    loop {
        // Try to parse a chunk from buf.
        if let Some(crlf_pos) = find_crlf(&buf) {
            let size_str = core::str::from_utf8(buf.get(..crlf_pos).unwrap_or(&[]))
                .unwrap_or("0");
            let chunk_size = parse_hex_usize(size_str);
            if chunk_size == 0 {
                break; // Final chunk.
            }

            let data_start = crlf_pos + 2;
            let data_end = data_start + chunk_size;
            let after_data = data_end + 2; // Skip trailing \r\n

            if buf.len() >= after_data {
                body.extend_from_slice(buf.get(data_start..data_end).unwrap_or(&[]));
                let remaining = Vec::from(buf.get(after_data..).unwrap_or(&[]));
                buf = remaining;
                polls = 0;

                if body.len() > MAX_BODY_SIZE {
                    break;
                }
                continue;
            }
        }

        // Need more data.
        super::super::net::poll();
        let data = super::tls::tls_recv(tls, 8192)?;
        if !data.is_empty() {
            buf.extend_from_slice(&data);
            polls = 0;
        } else {
            polls = polls.saturating_add(1);
            if polls >= timeout_polls {
                break;
            }
            core::hint::spin_loop();
        }
    }

    Ok(body)
}

/// Resolve a hostname to an IP address (IPv4 preferred, IPv6 fallback).
///
/// Resolution order:
/// 1. Parse as dotted-decimal IPv4 (e.g., "192.168.1.1").
/// 2. Parse as IPv6 literal (e.g., "fe80::1").
/// 3. DNS A record (IPv4).
/// 4. DNS AAAA record (IPv6) if A record resolution fails.
fn resolve_host(host: &str) -> KernelResult<IpAddr> {
    // Try parsing as a dotted-decimal IPv4 first.
    if let Some(ip) = parse_ipv4(host) {
        return Ok(IpAddr::V4(ip));
    }

    // Try parsing as an IPv6 literal.
    if let Some(v6) = super::ipv6::Ipv6Addr::parse(host) {
        return Ok(IpAddr::V6(v6));
    }

    // DNS A record (IPv4) — preferred for compatibility.
    if let Ok(v4) = super::dns::resolve(host) {
        return Ok(IpAddr::V4(v4));
    }

    // DNS AAAA record (IPv6) — fallback when no A record.
    match super::dns::resolve6(host) {
        Ok(v6) => Ok(IpAddr::V6(v6)),
        Err(e) => {
            crate::serial_println!(
                "[http] DNS resolution failed for '{}' (both A and AAAA)",
                host
            );
            Err(e)
        }
    }
}

/// Parse a dotted-decimal IPv4 string (e.g., "192.168.1.1").
fn parse_ipv4(s: &str) -> Option<Ipv4Addr> {
    let mut octets = [0u8; 4];
    let mut idx = 0usize;
    let mut current: u16 = 0;
    let mut digit_count = 0u8;

    for &b in s.as_bytes() {
        if b == b'.' {
            if digit_count == 0 || idx >= 3 {
                return None;
            }
            if current > 255 {
                return None;
            }
            octets[idx] = current as u8;
            idx = idx.checked_add(1)?;
            current = 0;
            digit_count = 0;
        } else if b >= b'0' && b <= b'9' {
            current = current.checked_mul(10)?.checked_add((b - b'0') as u16)?;
            digit_count = digit_count.checked_add(1)?;
            if digit_count > 3 {
                return None;
            }
        } else {
            return None;
        }
    }

    // Last octet.
    if digit_count == 0 || idx != 3 || current > 255 {
        return None;
    }
    octets[3] = current as u8;

    Some(Ipv4Addr(octets))
}

/// Resolve a redirect Location header to an absolute URL.
fn resolve_redirect(base: &Url, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        // Absolute URL — may change scheme (e.g., HTTP→HTTPS upgrade).
        String::from(location)
    } else {
        // Relative or path-absolute redirect — preserve the base scheme.
        let scheme = if base.is_https { "https" } else { "http" };
        let default_port = if base.is_https { DEFAULT_HTTPS_PORT } else { DEFAULT_HTTP_PORT };

        if location.starts_with('/') {
            // Absolute path relative to host.
            if base.port == default_port {
                format!("{}://{}{}", scheme, base.host, location)
            } else {
                format!("{}://{}:{}{}", scheme, base.host, base.port, location)
            }
        } else {
            // Relative path — resolve against current path.
            let base_path = match base.path.rfind('/') {
                Some(idx) => {
                    if let Some(slice) = base.path.get(..idx.saturating_add(1)) {
                        slice
                    } else {
                        "/"
                    }
                }
                None => "/",
            };
            if base.port == default_port {
                format!("{}://{}{}{}", scheme, base.host, base_path, location)
            } else {
                format!("{}://{}:{}{}{}", scheme, base.host, base.port, base_path, location)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Response reading and parsing
// ---------------------------------------------------------------------------

/// Read and parse an HTTP response from a TCP connection.
///
/// Reads data in chunks, parses the status line and headers, then
/// reads the body according to Content-Length or chunked encoding.
fn read_response(handle: usize, timeout_polls: u32, head_only: bool) -> KernelResult<Response> {
    // Accumulate raw bytes until we find the end of headers (\r\n\r\n).
    let mut raw = Vec::with_capacity(4096);
    let header_end = read_until_header_end(handle, timeout_polls, &mut raw)?;

    // Parse status line and headers from the raw bytes.
    let header_section = raw.get(..header_end).ok_or(KernelError::InternalError)?;
    let (status_code, reason, headers) = parse_response_headers(header_section)?;

    // For HEAD requests, there's no body.
    if head_only {
        return Ok(Response {
            status_code,
            reason,
            headers,
            body: Vec::new(),
        });
    }

    // The rest of `raw` after the headers (data already read but part of body).
    let body_start = header_end.checked_add(4).unwrap_or(header_end); // Skip \r\n\r\n
    let already_read = if body_start < raw.len() {
        Vec::from(raw.get(body_start..).unwrap_or(&[]))
    } else {
        Vec::new()
    };

    // Determine how to read the body.
    let is_chunked = headers.iter().any(|(n, v)| {
        n.eq_ignore_ascii_case("Transfer-Encoding") && v.contains("chunked")
    });

    let body = if is_chunked {
        read_chunked_body(handle, timeout_polls, already_read)?
    } else {
        let content_length = headers.iter().find_map(|(n, v)| {
            if n.eq_ignore_ascii_case("Content-Length") {
                parse_usize(v)
            } else {
                None
            }
        });
        read_fixed_body(handle, timeout_polls, already_read, content_length)?
    };

    Ok(Response {
        status_code,
        reason,
        headers,
        body,
    })
}

/// Read from TCP until we find the header/body boundary (`\r\n\r\n`).
///
/// Returns the byte offset of the first `\r\n\r\n` within `buf`.
fn read_until_header_end(
    handle: usize,
    timeout_polls: u32,
    buf: &mut Vec<u8>,
) -> KernelResult<usize> {
    let mut polls_remaining = timeout_polls;

    loop {
        // Check if we already have the header end.
        if let Some(pos) = find_header_end(buf) {
            return Ok(pos);
        }

        if buf.len() > MAX_HEADER_SIZE {
            return Err(KernelError::MessageTooLarge);
        }

        if polls_remaining == 0 {
            return Err(KernelError::TimedOut);
        }

        // Try to read more data (use reduced timeout to poll incrementally).
        let chunk_polls = polls_remaining.min(5000);
        polls_remaining = polls_remaining.saturating_sub(chunk_polls);

        let data = super::tcp::read_blocking(handle, chunk_polls, 8192)?;
        if data.is_empty() {
            // Connection closed before headers complete.
            // If we have some data, try to parse what we have.
            if !buf.is_empty() {
                if let Some(pos) = find_header_end(buf) {
                    return Ok(pos);
                }
            }
            return Err(KernelError::ChannelClosed);
        }
        buf.extend_from_slice(&data);
    }
}

/// Search for `\r\n\r\n` in a byte slice.
fn find_header_end(data: &[u8]) -> Option<usize> {
    if data.len() < 4 {
        return None;
    }
    let limit = data.len().saturating_sub(3);
    for i in 0..limit {
        if data.get(i) == Some(&b'\r')
            && data.get(i.wrapping_add(1)) == Some(&b'\n')
            && data.get(i.wrapping_add(2)) == Some(&b'\r')
            && data.get(i.wrapping_add(3)) == Some(&b'\n')
        {
            return Some(i);
        }
    }
    None
}

/// Parse the HTTP response header section (everything before `\r\n\r\n`).
fn parse_response_headers(
    data: &[u8],
) -> KernelResult<(u16, String, Vec<(String, String)>)> {
    let text = core::str::from_utf8(data).map_err(|_| KernelError::InvalidArgument)?;

    let mut lines = text.split("\r\n");

    // Status line: "HTTP/1.1 200 OK"
    let status_line = lines.next().ok_or(KernelError::InvalidArgument)?;
    let (status_code, reason) = parse_status_line(status_line)?;

    // Parse headers.
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADER_COUNT {
            break;
        }
        if line.len() > MAX_HEADER_LINE {
            continue; // Skip absurdly long header lines.
        }

        if let Some(colon_pos) = line.find(':') {
            let name = line.get(..colon_pos).unwrap_or("").trim();
            let value = line.get(colon_pos.saturating_add(1)..).unwrap_or("").trim();
            if !name.is_empty() {
                headers.push((String::from(name), String::from(value)));
            }
        }
    }

    Ok((status_code, reason, headers))
}

/// Parse the HTTP status line: "HTTP/1.x NNN Reason Phrase"
fn parse_status_line(line: &str) -> KernelResult<(u16, String)> {
    // Must start with "HTTP/"
    if !line.starts_with("HTTP/") {
        return Err(KernelError::InvalidArgument);
    }

    // Find the status code after the first space.
    let rest = match line.find(' ') {
        Some(idx) => line.get(idx.saturating_add(1)..).unwrap_or(""),
        None => return Err(KernelError::InvalidArgument),
    };

    // Status code is the next 3 characters.
    let code_str = rest.get(..3).ok_or(KernelError::InvalidArgument)?;
    let code = parse_u16(code_str).ok_or(KernelError::InvalidArgument)?;

    // Reason phrase is everything after the code and space.
    let reason = rest.get(4..).unwrap_or("").trim();

    Ok((code, String::from(reason)))
}

/// Read a fixed-length body (Content-Length known) or read until connection close.
fn read_fixed_body(
    handle: usize,
    timeout_polls: u32,
    already_read: Vec<u8>,
    content_length: Option<usize>,
) -> KernelResult<Vec<u8>> {
    let mut body = already_read;

    match content_length {
        Some(expected) => {
            // Enforce size limit.
            if expected > MAX_BODY_SIZE {
                return Err(KernelError::MessageTooLarge);
            }

            let mut polls_remaining = timeout_polls;

            while body.len() < expected {
                if polls_remaining == 0 {
                    return Err(KernelError::TimedOut);
                }

                let remaining = expected.saturating_sub(body.len());
                let chunk_size = remaining.min(16384);
                let chunk_polls = polls_remaining.min(5000);
                polls_remaining = polls_remaining.saturating_sub(chunk_polls);

                let data = super::tcp::read_blocking(handle, chunk_polls, chunk_size)?;
                if data.is_empty() {
                    // Connection closed early — return what we have.
                    break;
                }
                body.extend_from_slice(&data);
            }
        }
        None => {
            // No Content-Length: read until connection close.
            let mut polls_remaining = timeout_polls;

            loop {
                if body.len() > MAX_BODY_SIZE {
                    break;
                }

                if polls_remaining == 0 {
                    break;
                }

                let chunk_polls = polls_remaining.min(5000);
                polls_remaining = polls_remaining.saturating_sub(chunk_polls);

                let data = super::tcp::read_blocking(handle, chunk_polls, 16384)?;
                if data.is_empty() {
                    break;
                }
                body.extend_from_slice(&data);
            }
        }
    }

    Ok(body)
}

/// Read a chunked transfer-encoded body.
///
/// Chunked encoding format:
/// ```text
/// <chunk-size-hex>\r\n
/// <chunk-data>\r\n
/// ...
/// 0\r\n
/// \r\n
/// ```
fn read_chunked_body(
    handle: usize,
    timeout_polls: u32,
    already_read: Vec<u8>,
) -> KernelResult<Vec<u8>> {
    let mut body = Vec::with_capacity(4096);
    let mut buf = already_read;
    let mut polls_remaining = timeout_polls;

    loop {
        // Read more data if needed.
        if !has_complete_chunk_header(&buf) {
            if polls_remaining == 0 {
                return Err(KernelError::TimedOut);
            }
            let chunk_polls = polls_remaining.min(5000);
            polls_remaining = polls_remaining.saturating_sub(chunk_polls);
            let data = super::tcp::read_blocking(handle, chunk_polls, 16384)?;
            if data.is_empty() && buf.is_empty() {
                break;
            }
            buf.extend_from_slice(&data);
        }

        // Parse chunk size line.
        let line_end = match find_crlf(&buf) {
            Some(pos) => pos,
            None => {
                if buf.is_empty() {
                    break;
                }
                continue;
            }
        };

        let size_line = buf.get(..line_end).unwrap_or(&[]);
        let size_str = core::str::from_utf8(size_line).unwrap_or("0");
        // Chunk size may have extensions after ';' — ignore them.
        let pure_size = size_str.split(';').next().unwrap_or("0").trim();
        let chunk_size = parse_hex_usize(pure_size);

        // Remove the size line + \r\n from buffer.
        let consume = line_end.checked_add(2).unwrap_or(line_end);
        buf = Vec::from(buf.get(consume..).unwrap_or(&[]));

        if chunk_size == 0 {
            // Terminal chunk — we're done.
            break;
        }

        if body.len().saturating_add(chunk_size) > MAX_BODY_SIZE {
            return Err(KernelError::MessageTooLarge);
        }

        // Read the chunk data (may need more TCP reads).
        // chunk_size bytes of data + \r\n
        let total_needed = chunk_size.checked_add(2).unwrap_or(chunk_size);
        while buf.len() < total_needed {
            if polls_remaining == 0 {
                return Err(KernelError::TimedOut);
            }
            let chunk_polls = polls_remaining.min(5000);
            polls_remaining = polls_remaining.saturating_sub(chunk_polls);
            let data = super::tcp::read_blocking(handle, chunk_polls, 16384)?;
            if data.is_empty() {
                // Premature close — return what we have.
                body.extend_from_slice(&buf);
                return Ok(body);
            }
            buf.extend_from_slice(&data);
        }

        // Copy chunk data to body.
        if let Some(chunk_data) = buf.get(..chunk_size) {
            body.extend_from_slice(chunk_data);
        }

        // Skip chunk data + trailing \r\n.
        buf = Vec::from(buf.get(total_needed..).unwrap_or(&[]));
    }

    Ok(body)
}

/// Check if the buffer contains a complete chunk header (line ending with \r\n).
fn has_complete_chunk_header(data: &[u8]) -> bool {
    find_crlf(data).is_some()
}

/// Find the position of the first `\r\n` in data.
fn find_crlf(data: &[u8]) -> Option<usize> {
    if data.len() < 2 {
        return None;
    }
    let limit = data.len().saturating_sub(1);
    for i in 0..limit {
        if data.get(i) == Some(&b'\r') && data.get(i.wrapping_add(1)) == Some(&b'\n') {
            return Some(i);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Base64 encoding (for Basic Auth)
// ---------------------------------------------------------------------------

/// Minimal Base64 encoder (RFC 4648).
///
/// We only need encoding (for Authorization header), not decoding.
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = Vec::with_capacity(data.len().saturating_mul(4) / 3 + 4);
    let mut i = 0usize;

    while i.saturating_add(2) < data.len() {
        let b0 = *data.get(i).unwrap_or(&0) as u32;
        let b1 = *data.get(i.wrapping_add(1)).unwrap_or(&0) as u32;
        let b2 = *data.get(i.wrapping_add(2)).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(*ALPHABET.get(((triple >> 18) & 0x3F) as usize).unwrap_or(&b'A'));
        result.push(*ALPHABET.get(((triple >> 12) & 0x3F) as usize).unwrap_or(&b'A'));
        result.push(*ALPHABET.get(((triple >> 6) & 0x3F) as usize).unwrap_or(&b'A'));
        result.push(*ALPHABET.get((triple & 0x3F) as usize).unwrap_or(&b'A'));

        i = i.wrapping_add(3);
    }

    let remaining = data.len().saturating_sub(i);
    if remaining == 2 {
        let b0 = *data.get(i).unwrap_or(&0) as u32;
        let b1 = *data.get(i.wrapping_add(1)).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8);

        result.push(*ALPHABET.get(((triple >> 18) & 0x3F) as usize).unwrap_or(&b'A'));
        result.push(*ALPHABET.get(((triple >> 12) & 0x3F) as usize).unwrap_or(&b'A'));
        result.push(*ALPHABET.get(((triple >> 6) & 0x3F) as usize).unwrap_or(&b'A'));
        result.push(b'=');
    } else if remaining == 1 {
        let b0 = *data.get(i).unwrap_or(&0) as u32;
        let triple = b0 << 16;

        result.push(*ALPHABET.get(((triple >> 18) & 0x3F) as usize).unwrap_or(&b'A'));
        result.push(*ALPHABET.get(((triple >> 12) & 0x3F) as usize).unwrap_or(&b'A'));
        result.push(b'=');
        result.push(b'=');
    }

    String::from_utf8(result).unwrap_or_else(|_| String::new())
}

// ---------------------------------------------------------------------------
// Numeric parsing helpers
// ---------------------------------------------------------------------------

/// Parse a decimal string to u16.
fn parse_u16(s: &str) -> Option<u16> {
    let mut result: u16 = 0;
    if s.is_empty() {
        return None;
    }
    for &b in s.as_bytes() {
        if b < b'0' || b > b'9' {
            return None;
        }
        result = result.checked_mul(10)?.checked_add((b - b'0') as u16)?;
    }
    Some(result)
}

/// Parse a decimal string to usize.
fn parse_usize(s: &str) -> Option<usize> {
    let s = s.trim();
    let mut result: usize = 0;
    if s.is_empty() {
        return None;
    }
    for &b in s.as_bytes() {
        if b < b'0' || b > b'9' {
            return None;
        }
        result = result.checked_mul(10)?.checked_add((b - b'0') as usize)?;
    }
    Some(result)
}

/// Parse a hexadecimal string to usize (for chunked encoding).
fn parse_hex_usize(s: &str) -> usize {
    let mut result: usize = 0;
    for &b in s.as_bytes() {
        let digit = match b {
            b'0'..=b'9' => (b - b'0') as usize,
            b'a'..=b'f' => (b - b'a') as usize + 10,
            b'A'..=b'F' => (b - b'A') as usize + 10,
            _ => break,
        };
        result = result.saturating_mul(16).saturating_add(digit);
    }
    result
}

/// Format a usize as a decimal string (no alloc::format! needed for simple case).
fn format_usize(n: usize) -> String {
    if n == 0 {
        return String::from("0");
    }
    let mut digits = Vec::with_capacity(20);
    let mut val = n;
    while val > 0 {
        digits.push(b'0' + (val % 10) as u8);
        val /= 10;
    }
    digits.reverse();
    String::from_utf8(digits).unwrap_or_else(|_| String::from("0"))
}

/// Find the length of valid UTF-8 prefix in a byte slice.
fn valid_utf8_prefix_len(data: &[u8]) -> usize {
    // Walk forward, accepting complete UTF-8 sequences.
    let mut i = 0usize;
    while i < data.len() {
        let b = *data.get(i).unwrap_or(&0);
        let seq_len = if b < 0x80 {
            1
        } else if b < 0xC0 {
            return i; // Unexpected continuation byte.
        } else if b < 0xE0 {
            2
        } else if b < 0xF0 {
            3
        } else if b < 0xF8 {
            4
        } else {
            return i; // Invalid byte.
        };

        if i.saturating_add(seq_len) > data.len() {
            return i; // Truncated sequence.
        }

        // Validate continuation bytes.
        for j in 1..seq_len {
            let cont = *data.get(i.wrapping_add(j)).unwrap_or(&0);
            if cont < 0x80 || cont >= 0xC0 {
                return i;
            }
        }

        i = i.saturating_add(seq_len);
    }
    i
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

use core::sync::atomic::{AtomicU64, Ordering};

/// Total requests made.
static REQUESTS_TOTAL: AtomicU64 = AtomicU64::new(0);
/// Successful responses (2xx).
static REQUESTS_SUCCESS: AtomicU64 = AtomicU64::new(0);
/// Failed requests (connection error, timeout, etc.).
static REQUESTS_FAILED: AtomicU64 = AtomicU64::new(0);
/// Redirect responses followed.
static REDIRECTS_FOLLOWED: AtomicU64 = AtomicU64::new(0);
/// Total bytes received (response bodies).
static BYTES_RECEIVED: AtomicU64 = AtomicU64::new(0);
/// Total bytes sent (request data).
static BYTES_SENT: AtomicU64 = AtomicU64::new(0);

/// HTTP client statistics.
#[derive(Debug)]
pub struct HttpStats {
    pub total_requests: u64,
    pub successful: u64,
    pub failed: u64,
    pub redirects_followed: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
}

/// Get current HTTP client statistics.
pub fn stats() -> HttpStats {
    HttpStats {
        total_requests: REQUESTS_TOTAL.load(Ordering::Relaxed),
        successful: REQUESTS_SUCCESS.load(Ordering::Relaxed),
        failed: REQUESTS_FAILED.load(Ordering::Relaxed),
        redirects_followed: REDIRECTS_FOLLOWED.load(Ordering::Relaxed),
        bytes_received: BYTES_RECEIVED.load(Ordering::Relaxed),
        bytes_sent: BYTES_SENT.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Instrumented convenience wrappers
// ---------------------------------------------------------------------------

/// Perform an instrumented HTTP GET request (updates stats).
pub fn fetch(url: &str) -> KernelResult<Response> {
    REQUESTS_TOTAL.fetch_add(1, Ordering::Relaxed);

    let req = Request::new(Method::Get, url)?;
    let raw_len = req.build().len();
    BYTES_SENT.fetch_add(raw_len as u64, Ordering::Relaxed);

    match req.send_request() {
        Ok(resp) => {
            BYTES_RECEIVED.fetch_add(resp.body.len() as u64, Ordering::Relaxed);
            if resp.is_success() {
                REQUESTS_SUCCESS.fetch_add(1, Ordering::Relaxed);
            }
            Ok(resp)
        }
        Err(e) => {
            REQUESTS_FAILED.fetch_add(1, Ordering::Relaxed);
            Err(e)
        }
    }
}

/// Perform an instrumented HTTP POST request (updates stats).
pub fn fetch_post(
    url: &str,
    body: &[u8],
    content_type: Option<&str>,
) -> KernelResult<Response> {
    REQUESTS_TOTAL.fetch_add(1, Ordering::Relaxed);

    let mut req = Request::new(Method::Post, url)?;
    req.body = Some(Vec::from(body));
    req.content_type = content_type.map(String::from);

    let raw_len = req.build().len();
    BYTES_SENT.fetch_add(raw_len as u64, Ordering::Relaxed);

    match req.send_request() {
        Ok(resp) => {
            BYTES_RECEIVED.fetch_add(resp.body.len() as u64, Ordering::Relaxed);
            if resp.is_success() {
                REQUESTS_SUCCESS.fetch_add(1, Ordering::Relaxed);
            }
            Ok(resp)
        }
        Err(e) => {
            REQUESTS_FAILED.fetch_add(1, Ordering::Relaxed);
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// procfs content
// ---------------------------------------------------------------------------

/// Generate procfs content for `/proc/http`.
pub fn procfs_content() -> String {
    let s = stats();
    let mut out = String::with_capacity(256);

    out.push_str("HTTP Client Statistics\n");
    out.push_str("======================\n\n");

    out.push_str(&format!("Total requests:      {}\n", s.total_requests));
    out.push_str(&format!("  Successful (2xx):  {}\n", s.successful));
    out.push_str(&format!("  Failed:            {}\n", s.failed));
    out.push_str(&format!("  Redirects followed:{}\n", s.redirects_followed));
    out.push_str(&format!("Bytes sent:          {}\n", s.bytes_sent));
    out.push_str(&format!("Bytes received:      {}\n", s.bytes_received));

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run HTTP module self-tests.
///
/// Tests parsing, Base64 encoding, URL handling, and response parsing.
/// Network-dependent tests (actual HTTP requests) only run if the
/// network interface is up.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[http] Running HTTP self-tests...");
    let mut passed = 0u32;

    // --- Test 1: URL parsing (simple) ---
    {
        let url = Url::parse("http://example.com/api/status")?;
        assert_eq_test(&url.host, "example.com", "host");
        assert_eq_test_u16(url.port, 80, "port");
        assert_eq_test(&url.path, "/api/status", "path");
        assert!(url.query.is_none(), "query should be None");
        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 1 (url parse simple) PASSED");
    }

    // --- Test 2: URL parsing (with port and query) ---
    {
        let url = Url::parse("http://api.example.com:8080/v1/data?key=value&fmt=json")?;
        assert_eq_test(&url.host, "api.example.com", "host");
        assert_eq_test_u16(url.port, 8080, "port");
        assert_eq_test(&url.path, "/v1/data", "path");
        assert!(url.query.as_deref() == Some("key=value&fmt=json"), "query mismatch");
        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 2 (url parse port+query) PASSED");
    }

    // --- Test 3: URL parsing (bare host) ---
    {
        let url = Url::parse("http://example.com")?;
        assert_eq_test(&url.host, "example.com", "host");
        assert_eq_test(&url.path, "/", "path");
        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 3 (url parse bare host) PASSED");
    }

    // --- Test 4: HTTPS URL parsing ---
    {
        let url = Url::parse("https://secure.example.com/api")?;
        assert!(url.is_https, "HTTPS flag set");
        assert_eq_test(&url.host, "secure.example.com", "https host");
        assert!(url.port == 443, "https default port");
        assert_eq_test(&url.path, "/api", "https path");

        // HTTPS with explicit port.
        let url2 = Url::parse("https://example.com:8443/secure")?;
        assert!(url2.is_https, "HTTPS flag with explicit port");
        assert!(url2.port == 8443, "https explicit port");

        // HTTP URL should NOT set is_https.
        let url3 = Url::parse("http://example.com/plain")?;
        assert!(!url3.is_https, "HTTP flag not set");

        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 4 (https url parsing) PASSED");
    }

    // --- Test 5: Base64 encoding ---
    {
        // Test vectors from RFC 4648.
        assert_eq_test(&base64_encode(b""), "", "base64 empty");
        assert_eq_test(&base64_encode(b"f"), "Zg==", "base64 f");
        assert_eq_test(&base64_encode(b"fo"), "Zm8=", "base64 fo");
        assert_eq_test(&base64_encode(b"foo"), "Zm9v", "base64 foo");
        assert_eq_test(&base64_encode(b"foobar"), "Zm9vYmFy", "base64 foobar");
        assert_eq_test(
            &base64_encode(b"user:password"),
            "dXNlcjpwYXNzd29yZA==",
            "base64 auth",
        );
        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 5 (base64 encoding) PASSED");
    }

    // --- Test 6: IPv4 parsing ---
    {
        let ip = parse_ipv4("192.168.1.1");
        assert!(ip.is_some(), "should parse 192.168.1.1");
        let ip = ip.unwrap();
        assert!(ip.0 == [192, 168, 1, 1], "parsed IP mismatch");

        assert!(parse_ipv4("0.0.0.0").is_some(), "0.0.0.0");
        assert!(parse_ipv4("255.255.255.255").is_some(), "255.255.255.255");
        assert!(parse_ipv4("256.0.0.0").is_none(), "256 out of range");
        assert!(parse_ipv4("1.2.3").is_none(), "too few octets");
        assert!(parse_ipv4("1.2.3.4.5").is_none(), "too many octets");
        assert!(parse_ipv4("abc").is_none(), "non-numeric");
        assert!(parse_ipv4("").is_none(), "empty");
        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 6 (ipv4 parsing) PASSED");
    }

    // --- Test 7: Status line parsing ---
    {
        let (code, reason) = parse_status_line("HTTP/1.1 200 OK")?;
        assert_eq_test_u16(code, 200, "status code");
        assert_eq_test(&reason, "OK", "reason phrase");

        let (code, reason) = parse_status_line("HTTP/1.1 404 Not Found")?;
        assert_eq_test_u16(code, 404, "404 code");
        assert_eq_test(&reason, "Not Found", "404 reason");

        let (code, _) = parse_status_line("HTTP/1.0 301 Moved Permanently")?;
        assert_eq_test_u16(code, 301, "301 code");

        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 7 (status line parsing) PASSED");
    }

    // --- Test 8: Response header parsing ---
    {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 42\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n";
        let (code, reason, headers) = parse_response_headers(raw)?;
        assert_eq_test_u16(code, 200, "header parse code");
        assert_eq_test(&reason, "OK", "header parse reason");
        assert!(headers.len() == 3, "expected 3 headers");

        // Verify Content-Length header.
        let cl = headers.iter().find(|(n, _)| n == "Content-Length");
        assert!(cl.is_some(), "Content-Length missing");
        assert_eq_test(&cl.unwrap().1, "42", "Content-Length value");

        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 8 (response header parsing) PASSED");
    }

    // --- Test 9: Hexadecimal parsing (chunked encoding) ---
    {
        assert!(parse_hex_usize("0") == 0, "hex 0");
        assert!(parse_hex_usize("a") == 10, "hex a");
        assert!(parse_hex_usize("ff") == 255, "hex ff");
        assert!(parse_hex_usize("FF") == 255, "hex FF");
        assert!(parse_hex_usize("100") == 256, "hex 100");
        assert!(parse_hex_usize("1a3f") == 0x1a3f, "hex 1a3f");
        assert!(parse_hex_usize("") == 0, "hex empty");
        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 9 (hex parsing) PASSED");
    }

    // --- Test 10: Redirect URL resolution ---
    {
        let base = Url {
            host: String::from("example.com"),
            port: 80,
            path: String::from("/api/v1/resource"),
            query: None,
            is_https: false,
        };

        // Absolute redirect.
        let r = resolve_redirect(&base, "http://other.com/new");
        assert_eq_test(&r, "http://other.com/new", "absolute redirect");

        // Absolute path redirect.
        let r = resolve_redirect(&base, "/new/path");
        assert_eq_test(&r, "http://example.com/new/path", "absolute path redirect");

        // Relative path redirect.
        let r = resolve_redirect(&base, "sibling");
        assert_eq_test(&r, "http://example.com/api/v1/sibling", "relative redirect");

        // Non-default port.
        let base2 = Url {
            host: String::from("example.com"),
            port: 8080,
            path: String::from("/api"),
            query: None,
            is_https: false,
        };
        let r = resolve_redirect(&base2, "/other");
        assert_eq_test(&r, "http://example.com:8080/other", "port redirect");

        // HTTPS base URL should preserve scheme in relative redirects.
        let https_base = Url {
            host: String::from("secure.example.com"),
            port: 443,
            path: String::from("/login"),
            query: None,
            is_https: true,
        };
        let r = resolve_redirect(&https_base, "/dashboard");
        assert_eq_test(&r, "https://secure.example.com/dashboard", "https redirect");

        // Cross-scheme redirect (absolute URL).
        let r = resolve_redirect(&https_base, "http://plain.example.com/downgrade");
        assert_eq_test(&r, "http://plain.example.com/downgrade", "cross-scheme redirect");

        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 10 (redirect resolution) PASSED");
    }

    // --- Test 11: Request building ---
    {
        let req = Request::new(Method::Get, "http://example.com/test")?;
        let raw = req.build();
        let text = core::str::from_utf8(&raw).unwrap_or("");

        assert!(text.starts_with("GET /test HTTP/1.1\r\n"), "request line");
        assert!(text.contains("Host: example.com\r\n"), "host header");
        assert!(text.contains("User-Agent: "), "user-agent header");
        assert!(text.contains("Connection: close\r\n"), "connection header");
        assert!(text.ends_with("\r\n\r\n"), "ends with blank line");

        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 11 (request building) PASSED");
    }

    // --- Test 12: Request with body ---
    {
        let req = Request::new(Method::Post, "http://example.com/api")?
            .body_bytes(b"key=value", "application/x-www-form-urlencoded");
        let raw = req.build();
        let text = core::str::from_utf8(&raw).unwrap_or("");

        assert!(text.starts_with("POST /api HTTP/1.1\r\n"), "POST request line");
        assert!(text.contains("Content-Type: application/x-www-form-urlencoded"), "content type");
        assert!(text.contains("Content-Length: 9\r\n"), "content length");
        assert!(text.ends_with("key=value"), "body present");

        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 12 (request with body) PASSED");
    }

    // --- Test 13: Request with basic auth ---
    {
        let req = Request::new(Method::Get, "http://example.com/secure")?
            .basic_auth_creds("admin", "secret");
        let raw = req.build();
        let text = core::str::from_utf8(&raw).unwrap_or("");

        assert!(text.contains("Authorization: Basic YWRtaW46c2VjcmV0"), "basic auth");

        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 13 (basic auth) PASSED");
    }

    // --- Test 14: Header end detection ---
    {
        let data = b"HTTP/1.1 200 OK\r\nHost: test\r\n\r\nbody here";
        let pos = find_header_end(data);
        // The \r\n\r\n starts at offset after "Host: test\r\n" — find it.
        assert!(pos.is_some(), "should find header end");
        let p = pos.unwrap();
        // Verify the boundary: data[p..p+4] == "\r\n\r\n"
        assert!(data.get(p) == Some(&b'\r'), "boundary byte 0");
        assert!(data.get(p + 1) == Some(&b'\n'), "boundary byte 1");
        assert!(data.get(p + 2) == Some(&b'\r'), "boundary byte 2");
        assert!(data.get(p + 3) == Some(&b'\n'), "boundary byte 3");

        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 14 (header end detection) PASSED");
    }

    // --- Test 15: CRLF finding ---
    {
        assert!(find_crlf(b"hello\r\nworld") == Some(5), "crlf basic");
        assert!(find_crlf(b"\r\n") == Some(0), "crlf at start");
        assert!(find_crlf(b"no newline") == None, "no crlf");
        assert!(find_crlf(b"") == None, "empty");
        assert!(find_crlf(b"\r") == None, "lone cr");

        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 15 (crlf finding) PASSED");
    }

    // --- Test 16: UTF-8 prefix length ---
    {
        assert!(valid_utf8_prefix_len(b"hello") == 5, "ascii");
        assert!(valid_utf8_prefix_len(b"") == 0, "empty");
        assert!(valid_utf8_prefix_len(b"\xc3\xa9") == 2, "é valid");
        // Invalid: lone continuation byte at byte 5
        let data = b"hello\x80world";
        assert!(valid_utf8_prefix_len(data) == 5, "prefix before invalid");

        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 16 (utf8 prefix) PASSED");
    }

    // --- Test 17: URL host header generation ---
    {
        let url = Url::parse("http://example.com/test")?;
        assert_eq_test(&url.host_header(), "example.com", "host header default port");

        let url = Url::parse("http://example.com:9090/test")?;
        assert_eq_test(&url.host_header(), "example.com:9090", "host header custom port");

        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 17 (host header) PASSED");
    }

    // --- Test 18: format_usize ---
    {
        assert_eq_test(&format_usize(0), "0", "format 0");
        assert_eq_test(&format_usize(1), "1", "format 1");
        assert_eq_test(&format_usize(42), "42", "format 42");
        assert_eq_test(&format_usize(12345), "12345", "format 12345");

        passed = passed.saturating_add(1);
        crate::serial_println!("[http]   test 18 (format_usize) PASSED");
    }

    crate::serial_println!("[http] All {} self-tests PASSED", passed);
    Ok(())
}

/// Helper for string equality assertions in self-tests.
fn assert_eq_test(got: &str, expected: &str, label: &str) {
    assert!(
        got == expected,
        "[http] assertion failed: {} — got '{}', expected '{}'",
        label, got, expected,
    );
}

/// Helper for u16 equality assertions in self-tests.
fn assert_eq_test_u16(got: u16, expected: u16, label: &str) {
    assert!(
        got == expected,
        "[http] assertion failed: {} — got {}, expected {}",
        label, got, expected,
    );
}
