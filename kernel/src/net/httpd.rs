//! HTTP/1.1 server for serving files from the VFS.
//!
//! Provides a minimal HTTP/1.1 server (RFC 7230-7235) that serves static
//! files from the kernel's virtual filesystem.  This enables remote file
//! access, system information retrieval (via /proc, /sys), and basic
//! web-based management.
//!
//! ## Protocol
//!
//! Supports HTTP/1.1 GET and HEAD methods.  The server parses the
//! Request-Line and Host header, maps the URI path to a VFS path under
//! the configured document root, and returns the file contents with
//! appropriate headers (Content-Type, Content-Length, Date, Server).
//!
//! ## Architecture
//!
//! ```text
//! Browser/curl ─── TCP:8080 ──→ HTTP server
//!                                  ├── accept connections
//!                                  ├── parse HTTP request
//!                                  ├── resolve VFS path
//!                                  ├── read file contents
//!                                  └── send HTTP response
//! ```
//!
//! ## Directory Listing
//!
//! When the requested path is a directory, the server generates an HTML
//! directory listing with file names, sizes, and types.
//!
//! ## MIME Types
//!
//! Content-Type is determined from file extensions using a built-in
//! extension→MIME table.  Unknown types default to `application/octet-stream`.
//!
//! ## Security
//!
//! - Read-only: only GET/HEAD, no PUT/POST/DELETE.
//! - Path traversal protection: normalizes `..` and rejects escapes.
//! - Maximum request size: 8 KiB headers.
//! - Maximum response body: 4 MiB per request.
//!
//! ## Limitations
//!
//! - No chunked transfer encoding (Content-Length only).
//! - No persistent connections (Connection: close after each response).
//! - No authentication (trusted-network only).
//! - Single-threaded: serves one request at a time per connection.
//!
//! ## HTTPS
//!
//! Optional TLS 1.3 support via `start_tls()`.  Binds a separate TCP
//! listener (default port 443) and performs a TLS 1.3 handshake with
//! Ed25519 self-signed certificates on each connection.  Uses the same
//! request parsing and response logic as plain HTTP.  The TLS host key
//! is generated from the kernel CSPRNG on first start.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Default HTTP listening port.
const DEFAULT_PORT: u16 = 8080;

/// Default HTTPS listening port.
const DEFAULT_TLS_PORT: u16 = 443;

/// Maximum HTTP request header size (bytes).
const MAX_REQUEST_SIZE: usize = 8192;

/// Maximum response body size (bytes).
const MAX_BODY_SIZE: usize = 4 * 1024 * 1024; // 4 MiB

/// Poll timeout for reading request (in poll iterations, ~1ms each).
const READ_TIMEOUT_POLLS: u32 = 5000; // 5 seconds

/// Maximum bytes to read in a single `tls_server_recv` attempt.
const TLS_RECV_MAX: usize = 16384;

/// Maximum number of TLS recv attempts to accumulate a full HTTP request.
const TLS_RECV_ATTEMPTS: u32 = 50;

/// Server name for Server header.
const SERVER_NAME: &str = "MintOS-httpd/0.1";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Whether the server is enabled.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Current listening port.
static PORT: AtomicU16 = AtomicU16::new(DEFAULT_PORT);

/// Document root path.
pub static DOC_ROOT: spin::Mutex<&str> = spin::Mutex::new("/");

/// Active listener handle (0 = none).
static LISTENER: spin::Mutex<usize> = spin::Mutex::new(0);

/// WebSocket message handler callback.
///
/// Defaults to the echo handler.  Set via `set_ws_handler()`.
static WS_HANDLER: spin::Mutex<super::websocket::WsMessageHandler> =
    spin::Mutex::new(super::websocket::echo_handler);

// ---------------------------------------------------------------------------
// HTTPS / TLS state
// ---------------------------------------------------------------------------

/// Whether the HTTPS (TLS) server is enabled.
static TLS_ENABLED: AtomicBool = AtomicBool::new(false);

/// HTTPS listening port.
static TLS_PORT: AtomicU16 = AtomicU16::new(DEFAULT_TLS_PORT);

/// Active HTTPS listener handle (0 = none).
static TLS_LISTENER: spin::Mutex<usize> = spin::Mutex::new(0);

/// TLS host key: Ed25519 seed (32 bytes).
static TLS_HOST_KEY_SEED: spin::Mutex<[u8; 32]> = spin::Mutex::new([0u8; 32]);

/// TLS host key: Ed25519 public key (32 bytes).
static TLS_HOST_KEY_PUBLIC: spin::Mutex<[u8; 32]> = spin::Mutex::new([0u8; 32]);

// ---------------------------------------------------------------------------
// MIME type detection
// ---------------------------------------------------------------------------

/// Map file extension to MIME type.
fn mime_for_extension(ext: &str) -> &'static str {
    match ext {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "xml" => "application/xml",
        "txt" | "log" | "conf" | "cfg" | "ini" | "toml" | "yaml" | "yml" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" | "gzip" => "application/gzip",
        "tar" => "application/x-tar",
        "wasm" => "application/wasm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "rs" | "c" | "h" | "py" | "sh" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Get MIME type for a file path.
fn mime_for_path(path: &str) -> &'static str {
    if let Some(dot_pos) = path.rfind('.') {
        let ext = &path[dot_pos.saturating_add(1)..];
        mime_for_extension(ext)
    } else {
        "application/octet-stream"
    }
}

// ---------------------------------------------------------------------------
// HTTP request parsing
// ---------------------------------------------------------------------------

/// Parsed HTTP request.
struct HttpRequest {
    /// HTTP method (GET, HEAD, etc.).
    method: String,
    /// Request URI path (decoded, no query string).
    path: String,
    /// HTTP version string.
    #[allow(dead_code)]
    version: String,
}

/// Parse an HTTP request from raw bytes.
///
/// Returns `None` if the request is malformed or incomplete.
fn parse_request(data: &[u8]) -> Option<HttpRequest> {
    // Find the end of the request line.
    let text = core::str::from_utf8(data).ok()?;
    let request_line = text.lines().next()?;

    let mut parts = request_line.split_whitespace();
    let method = String::from(parts.next()?);
    let raw_uri = parts.next()?;
    let version = String::from(parts.next().unwrap_or("HTTP/1.0"));

    // Extract path (before '?' query string).
    let path_str = if let Some(q) = raw_uri.find('?') {
        &raw_uri[..q]
    } else {
        raw_uri
    };

    // Percent-decode the path.
    let decoded = percent_decode(path_str);

    // Normalize the path to prevent traversal.
    let normalized = normalize_path(&decoded);

    Some(HttpRequest {
        method,
        path: normalized,
        version,
    })
}

/// Percent-decode a URI path component.
#[allow(clippy::arithmetic_side_effects)]
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (
                hex_digit(bytes[i + 1]),
                hex_digit(bytes[i + 2]),
            ) {
                result.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(result).unwrap_or_default()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Normalize a path: resolve `.` and `..`, collapse slashes, ensure leading `/`.
///
/// Prevents path traversal by never allowing the path to escape the root.
fn normalize_path(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();

    for part in path.split('/') {
        match part {
            "" | "." => { /* skip */ }
            ".." => { segments.pop(); }
            _ => segments.push(part),
        }
    }

    if segments.is_empty() {
        return String::from("/");
    }

    let mut result = String::new();
    for seg in &segments {
        result.push('/');
        result.push_str(seg);
    }
    result
}

// ---------------------------------------------------------------------------
// HTTP response building
// ---------------------------------------------------------------------------

/// Build an HTTP response with status, headers, and optional body.
#[allow(clippy::arithmetic_side_effects)]
fn build_response(status: u16, reason: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let resp = format!(
        "HTTP/1.1 {} {}\r\n\
         Server: {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        status, reason,
        SERVER_NAME,
        content_type,
        body.len(),
    );

    let mut bytes = resp.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// Build a HEAD response (headers only, no body).
fn build_head_response(status: u16, reason: &str, content_type: &str, content_length: usize) -> Vec<u8> {
    let resp = format!(
        "HTTP/1.1 {} {}\r\n\
         Server: {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        status, reason,
        SERVER_NAME,
        content_type,
        content_length,
    );
    resp.into_bytes()
}

/// Build a simple error response.
fn error_response(status: u16, reason: &str) -> Vec<u8> {
    let body = format!(
        "<html><head><title>{} {}</title></head>\
         <body><h1>{} {}</h1></body></html>",
        status, reason, status, reason,
    );
    build_response(status, reason, "text/html; charset=utf-8", body.as_bytes())
}

// ---------------------------------------------------------------------------
// Directory listing
// ---------------------------------------------------------------------------

/// Generate an HTML directory listing for the given VFS path.
#[allow(clippy::arithmetic_side_effects)]
fn directory_listing(vfs_path: &str, uri_path: &str) -> KernelResult<Vec<u8>> {
    use crate::fs::vfs::{Vfs, EntryType};

    let entries = Vfs::readdir(vfs_path)?;

    let mut html = format!(
        "<!DOCTYPE html>\n<html><head>\
         <meta charset=\"utf-8\">\
         <title>Index of {}</title>\
         <style>\
         body {{ font-family: monospace; margin: 2em; }}\
         table {{ border-collapse: collapse; }}\
         th, td {{ text-align: left; padding: 4px 16px; }}\
         a {{ text-decoration: none; color: #0066cc; }}\
         a:hover {{ text-decoration: underline; }}\
         .dir {{ font-weight: bold; }}\
         </style>\
         </head><body>\
         <h1>Index of {}</h1>\
         <table><tr><th>Name</th><th>Size</th><th>Type</th></tr>\n",
        uri_path, uri_path,
    );

    // Parent directory link (if not root).
    if uri_path != "/" {
        let parent = if let Some(pos) = uri_path[..uri_path.len().saturating_sub(1)].rfind('/') {
            &uri_path[..pos.saturating_add(1)]
        } else {
            "/"
        };
        html.push_str(&format!(
            "<tr><td><a href=\"{}\">..</a></td><td>-</td><td>Directory</td></tr>\n",
            parent,
        ));
    }

    for entry in &entries {
        let name = &entry.name;
        let _entry_vfs_path = if vfs_path.ends_with('/') {
            format!("{}{}", vfs_path, name)
        } else {
            format!("{}/{}", vfs_path, name)
        };

        let entry_uri = if uri_path.ends_with('/') {
            format!("{}{}", uri_path, name)
        } else {
            format!("{}/{}", uri_path, name)
        };

        let is_dir = entry.entry_type == EntryType::Directory;

        let size_str = if is_dir {
            String::from("-")
        } else {
            format_size(entry.size)
        };

        let type_str = if is_dir { "Directory" } else {
            mime_for_path(name)
        };

        let display_name = if is_dir {
            format!("{}/", name)
        } else {
            name.clone()
        };

        let href = if is_dir {
            format!("{}/", entry_uri)
        } else {
            entry_uri.clone()
        };

        let class = if is_dir { " class=\"dir\"" } else { "" };

        html.push_str(&format!(
            "<tr><td><a href=\"{}\"{}>{}</a></td><td>{}</td><td>{}</td></tr>\n",
            href, class, display_name, size_str, type_str,
        ));
    }

    html.push_str("</table>\n");
    html.push_str(&format!("<hr><p>{}</p>\n", SERVER_NAME));
    html.push_str("</body></html>\n");

    Ok(html.into_bytes())
}

/// Format a file size in human-readable form.
#[allow(clippy::arithmetic_side_effects)]
fn format_size(size: u64) -> String {
    if size < 1024 {
        format!("{} B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1} KiB", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GiB", size as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// ---------------------------------------------------------------------------
// Request handler
// ---------------------------------------------------------------------------

/// Handle a single HTTP request on an accepted TCP connection.
#[allow(clippy::arithmetic_side_effects)]
fn handle_connection(conn_handle: usize) {
    use crate::net::tcp;

    // Read the request (up to MAX_REQUEST_SIZE).
    let request_data = match tcp::read_blocking(conn_handle, READ_TIMEOUT_POLLS, MAX_REQUEST_SIZE) {
        Ok(data) => data,
        Err(_) => {
            // Timeout or error reading — send 408.
            let resp = error_response(408, "Request Timeout");
            let _ = tcp::send(conn_handle, &resp);
            let _ = tcp::close(conn_handle);
            return;
        }
    };

    if request_data.is_empty() {
        let _ = tcp::close(conn_handle);
        return;
    }

    // Check for WebSocket upgrade request before normal HTTP handling.
    // (WebSocket over TLS is not yet supported — only plain HTTP.)
    if super::websocket::is_upgrade_request(&request_data) {
        let handler = WS_HANDLER.lock();
        if let Err(e) = super::websocket::handle_upgrade(conn_handle, &request_data, *handler) {
            serial_println!("[httpd] WebSocket upgrade failed: {:?}", e);
        }
        return;
    }

    // Shared request processing.
    let response = process_http_request(&request_data);
    let _ = tcp::send(conn_handle, &response);
    let _ = tcp::close(conn_handle);
}

/// Serve a file from the VFS.
fn serve_file(_conn_handle: usize, vfs_path: &str, method: &str) -> Vec<u8> {
    use crate::fs::vfs::Vfs;

    match Vfs::read_file(vfs_path) {
        Ok(data) => {
            if data.len() > MAX_BODY_SIZE {
                return error_response(413, "Payload Too Large");
            }

            let content_type = mime_for_path(vfs_path);

            if method == "HEAD" {
                build_head_response(200, "OK", content_type, data.len())
            } else {
                build_response(200, "OK", content_type, &data)
            }
        }
        Err(KernelError::NotFound) => error_response(404, "Not Found"),
        Err(KernelError::PermissionDenied) => error_response(403, "Forbidden"),
        Err(_) => error_response(500, "Internal Server Error"),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Start the HTTP server on the specified port.
///
/// The server runs synchronously — call `tick()` periodically from the
/// network poll loop to accept connections and serve requests.
pub fn start(port: u16) -> KernelResult<()> {
    if ENABLED.load(Ordering::Relaxed) {
        return Err(KernelError::AlreadyExists);
    }

    let listener = super::tcp::bind(crate::netns::ROOT_NS, port)?;
    *LISTENER.lock() = listener;
    PORT.store(port, Ordering::Relaxed);
    ENABLED.store(true, Ordering::Relaxed);

    serial_println!("[httpd] Server started on port {}", port);
    Ok(())
}

/// Stop the HTTP server.
pub fn stop() {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    ENABLED.store(false, Ordering::Relaxed);

    let listener = {
        let mut guard = LISTENER.lock();
        let h = *guard;
        *guard = 0;
        h
    };

    if listener != 0 {
        let _ = super::tcp::close_listener(listener);
    }

    serial_println!("[httpd] Server stopped");
}

/// Check if the server is running.
pub fn is_running() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Get the current listening port.
pub fn port() -> u16 {
    PORT.load(Ordering::Relaxed)
}

/// Set the document root path.
pub fn set_doc_root(path: &'static str) {
    *DOC_ROOT.lock() = path;
}

/// Set the WebSocket message handler callback.
///
/// When a client sends a WebSocket upgrade request to the HTTP server,
/// the server will call this handler for each incoming message.
/// Default is `echo_handler` (echoes text/binary back).
pub fn set_ws_handler(handler: super::websocket::WsMessageHandler) {
    *WS_HANDLER.lock() = handler;
}

/// Accept and handle pending HTTP connections (non-blocking).
///
/// Call this periodically (e.g., from the network tick loop or a
/// dedicated kshell command).  Each call accepts at most one connection
/// and handles it synchronously.
pub fn tick() {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let listener = *LISTENER.lock();
    if listener == 0 {
        return;
    }

    // Try to accept a connection (non-blocking via try_accept).
    if let Ok(conn) = super::tcp::accept(listener) {
        handle_connection(conn);
    }
}

/// Accept and handle pending HTTPS (TLS) connections (non-blocking).
///
/// Call this periodically alongside `tick()`.  Accepts a TCP connection,
/// performs a TLS 1.3 handshake, then processes the HTTP request over the
/// encrypted channel.
pub fn tick_tls() {
    if !TLS_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let listener = *TLS_LISTENER.lock();
    if listener == 0 {
        return;
    }

    // Try to accept a TCP connection.
    if let Ok(conn) = super::tcp::accept(listener) {
        handle_tls_connection(conn);
    }
}

// ---------------------------------------------------------------------------
// HTTPS connection handler
// ---------------------------------------------------------------------------

/// Handle a single HTTPS request: TLS handshake + encrypted HTTP.
#[allow(clippy::arithmetic_side_effects)]
fn handle_tls_connection(tcp_handle: usize) {
    use super::tls;

    let seed = *TLS_HOST_KEY_SEED.lock();
    let public = *TLS_HOST_KEY_PUBLIC.lock();

    // Perform TLS 1.3 handshake.
    let mut session = match tls::tls_accept(tcp_handle, &seed, &public) {
        Ok(s) => s,
        Err(e) => {
            serial_println!("[httpd] TLS handshake failed: {:?}", e);
            let _ = super::tcp::close(tcp_handle);
            return;
        }
    };

    // Read the HTTP request over TLS.
    let request_data = match tls_read_request(&mut session) {
        Ok(data) => data,
        Err(_) => {
            let resp = error_response(408, "Request Timeout");
            let _ = tls::tls_server_send(&mut session, &resp);
            let _ = tls::tls_server_close(&mut session);
            return;
        }
    };

    if request_data.is_empty() {
        let _ = tls::tls_server_close(&mut session);
        return;
    }

    // Process the HTTP request (same logic as plain HTTP).
    let response = process_http_request(&request_data);
    let _ = tls::tls_server_send(&mut session, &response);
    let _ = tls::tls_server_close(&mut session);
}

/// Read a complete HTTP request from a TLS session.
///
/// Accumulates data from `tls_server_recv()` until we see the end of
/// the HTTP headers (`\r\n\r\n`), or until we hit a size/attempt limit.
fn tls_read_request(
    session: &mut super::tls::TlsServerSession,
) -> KernelResult<Vec<u8>> {
    let mut buf = Vec::new();

    for _ in 0..TLS_RECV_ATTEMPTS {
        match super::tls::tls_server_recv(session, TLS_RECV_MAX) {
            Ok(data) => {
                buf.extend_from_slice(&data);
                // Check if we have the full HTTP header.
                if contains_header_end(&buf) {
                    return Ok(buf);
                }
                if buf.len() >= MAX_REQUEST_SIZE {
                    return Ok(buf);
                }
            }
            Err(KernelError::WouldBlock) => {
                // No data yet — keep trying.
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    if buf.is_empty() {
        Err(KernelError::TimedOut)
    } else {
        Ok(buf)
    }
}

/// Check if a buffer contains the HTTP header terminator `\r\n\r\n`.
fn contains_header_end(buf: &[u8]) -> bool {
    buf.windows(4).any(|w| w == b"\r\n\r\n")
}

/// Process an HTTP request and return the response bytes.
///
/// This is the shared logic for both plain HTTP and HTTPS.
/// Handles WebSocket detection (skipped for HTTPS), dashboard API,
/// and VFS file serving.
#[allow(clippy::arithmetic_side_effects)]
fn process_http_request(request_data: &[u8]) -> Vec<u8> {
    use crate::fs::vfs::{Vfs, EntryType};

    // Parse the HTTP request.
    let req = match parse_request(request_data) {
        Some(r) => r,
        None => {
            return error_response(400, "Bad Request");
        }
    };

    serial_println!("[httpd/tls] {} {}", req.method, req.path);

    // Only allow GET and HEAD.
    if req.method != "GET" && req.method != "HEAD" {
        return error_response(405, "Method Not Allowed");
    }

    // Dashboard API and HTML — intercept before VFS serving.
    if req.path.starts_with("/api/") || req.path == "/dashboard" || req.path == "/dashboard/" {
        if let Some((content_type, body)) = super::dashboard::handle_api_request(&req.path) {
            let response = if req.method == "HEAD" {
                build_head_response(200, "OK", &content_type, body.len())
            } else {
                build_response(200, "OK", &content_type, &body)
            };
            return response;
        }
    }

    // Map URI path to VFS path.
    let doc_root = *DOC_ROOT.lock();
    let vfs_path = if req.path == "/" {
        String::from(doc_root)
    } else if doc_root == "/" {
        req.path.clone()
    } else {
        format!("{}{}", doc_root.trim_end_matches('/'), req.path)
    };

    // Check if path exists and is a directory or file.
    let meta = Vfs::stat(&vfs_path);

    match meta {
        Ok(m) if m.entry_type == EntryType::Directory => {
            // Try index.html first.
            let index_path = if vfs_path.ends_with('/') {
                format!("{}index.html", vfs_path)
            } else {
                format!("{}/index.html", vfs_path)
            };

            if Vfs::stat(&index_path).is_ok() {
                serve_file(0, &index_path, &req.method)
            } else {
                match directory_listing(&vfs_path, &req.path) {
                    Ok(body) => {
                        if req.method == "HEAD" {
                            build_head_response(200, "OK", "text/html; charset=utf-8", body.len())
                        } else {
                            build_response(200, "OK", "text/html; charset=utf-8", &body)
                        }
                    }
                    Err(_) => error_response(500, "Internal Server Error"),
                }
            }
        }
        Ok(_) => serve_file(0, &vfs_path, &req.method),
        Err(KernelError::NotFound) => error_response(404, "Not Found"),
        Err(KernelError::PermissionDenied) => error_response(403, "Forbidden"),
        Err(_) => error_response(500, "Internal Server Error"),
    }
}

// ---------------------------------------------------------------------------
// HTTPS public API
// ---------------------------------------------------------------------------

/// Start the HTTPS (TLS) server on the specified port.
///
/// Generates a fresh Ed25519 host key, binds a TCP listener, and begins
/// accepting TLS connections.  Call `tick_tls()` periodically to serve.
pub fn start_tls(port: u16) -> KernelResult<()> {
    if TLS_ENABLED.load(Ordering::Relaxed) {
        return Err(KernelError::AlreadyExists);
    }

    // Generate host key from CSPRNG.
    let mut seed = [0u8; 32];
    crate::rng::fill(&mut seed);
    let public = crate::crypto::ed25519_public_key(&seed);

    // Bind TCP listener.
    let listener = super::tcp::bind(crate::netns::ROOT_NS, port)?;

    *TLS_HOST_KEY_SEED.lock() = seed;
    *TLS_HOST_KEY_PUBLIC.lock() = public;
    *TLS_LISTENER.lock() = listener;
    TLS_PORT.store(port, Ordering::Relaxed);
    TLS_ENABLED.store(true, Ordering::Relaxed);

    // Log fingerprint.
    let fingerprint = crate::crypto::sha256(&public);
    serial_println!(
        "[httpd] HTTPS server started on port {} (cert fingerprint: SHA256:{:02x}{:02x}{:02x}...{:02x})",
        port,
        fingerprint[0], fingerprint[1], fingerprint[2],
        fingerprint[31],
    );
    Ok(())
}

/// Stop the HTTPS (TLS) server.
pub fn stop_tls() {
    if !TLS_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    TLS_ENABLED.store(false, Ordering::Relaxed);

    let listener = {
        let mut guard = TLS_LISTENER.lock();
        let h = *guard;
        *guard = 0;
        h
    };

    if listener != 0 {
        let _ = super::tcp::close_listener(listener);
    }

    // Zero the host key material.
    *TLS_HOST_KEY_SEED.lock() = [0u8; 32];
    *TLS_HOST_KEY_PUBLIC.lock() = [0u8; 32];

    serial_println!("[httpd] HTTPS server stopped");
}

/// Check if the HTTPS server is running.
pub fn is_tls_running() -> bool {
    TLS_ENABLED.load(Ordering::Relaxed)
}

/// Get the current HTTPS listening port.
pub fn tls_port() -> u16 {
    TLS_PORT.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Benchmark helpers (public for bench module)
// ---------------------------------------------------------------------------

/// Parse an HTTP request from raw bytes.  Exposed for benchmarking.
#[inline(never)]
pub fn bench_parse_request(data: &[u8]) -> Option<bool> {
    parse_request(data).map(|r| !r.method.is_empty())
}

/// Determine MIME type for a file path.  Exposed for benchmarking.
#[inline(never)]
pub fn bench_mime_type(path: &str) -> &'static str {
    mime_for_path(path)
}

/// Percent-decode a URI string.  Exposed for benchmarking.
#[inline(never)]
pub fn bench_percent_decode(s: &str) -> String {
    percent_decode(s)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// HTTP server self-test (unit tests for parsing and response building).
pub fn self_test() -> KernelResult<()> {
    serial_println!("[httpd] Running self-test...");

    // Test 1: Path normalization.
    assert_eq!(normalize_path("/"), "/");
    assert_eq!(normalize_path("/foo/bar"), "/foo/bar");
    assert_eq!(normalize_path("/foo/../bar"), "/bar");
    assert_eq!(normalize_path("/foo/./bar"), "/foo/bar");
    assert_eq!(normalize_path("/../../../etc/passwd"), "/etc/passwd");
    assert_eq!(normalize_path("/a//b///c"), "/a/b/c");
    assert_eq!(normalize_path(""), "/");
    serial_println!("[httpd]   Path normalization: OK");

    // Test 2: Percent decoding.
    assert_eq!(percent_decode("/foo%20bar"), "/foo bar");
    assert_eq!(percent_decode("/hello%2Fworld"), "/hello/world");
    assert_eq!(percent_decode("/plain"), "/plain");
    assert_eq!(percent_decode("%41%42%43"), "ABC");
    serial_println!("[httpd]   Percent decode: OK");

    // Test 3: MIME type detection.
    assert_eq!(mime_for_path("/index.html"), "text/html; charset=utf-8");
    assert_eq!(mime_for_path("/style.css"), "text/css; charset=utf-8");
    assert_eq!(mime_for_path("/app.js"), "application/javascript; charset=utf-8");
    assert_eq!(mime_for_path("/data.json"), "application/json; charset=utf-8");
    assert_eq!(mime_for_path("/image.png"), "image/png");
    assert_eq!(mime_for_path("/unknown"), "application/octet-stream");
    serial_println!("[httpd]   MIME type detection: OK");

    // Test 4: Request parsing.
    let req = parse_request(b"GET /index.html HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert!(req.is_some());
    let r = req.unwrap();
    assert_eq!(r.method, "GET");
    assert_eq!(r.path, "/index.html");
    serial_println!("[httpd]   Request parsing: OK");

    // Test 5: Request with query string.
    let req2 = parse_request(b"GET /search?q=hello&lang=en HTTP/1.1\r\n\r\n");
    assert!(req2.is_some());
    assert_eq!(req2.unwrap().path, "/search");
    serial_println!("[httpd]   Query string stripping: OK");

    // Test 6: Response building.
    let resp = build_response(200, "OK", "text/plain", b"Hello");
    let resp_str = core::str::from_utf8(&resp).unwrap_or("");
    assert!(resp_str.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(resp_str.contains("Content-Length: 5\r\n"));
    assert!(resp_str.contains("Content-Type: text/plain\r\n"));
    assert!(resp_str.ends_with("Hello"));
    serial_println!("[httpd]   Response building: OK");

    // Test 7: Error response.
    let err = error_response(404, "Not Found");
    let err_str = core::str::from_utf8(&err).unwrap_or("");
    assert!(err_str.starts_with("HTTP/1.1 404 Not Found\r\n"));
    assert!(err_str.contains("404 Not Found"));
    serial_println!("[httpd]   Error response: OK");

    // Test 8: Path traversal prevention.
    assert_eq!(normalize_path("/../../etc/shadow"), "/etc/shadow");
    assert_eq!(normalize_path("/foo/../../bar"), "/bar");
    // Path can never escape root.
    assert_eq!(normalize_path("/../../../../"), "/");
    serial_println!("[httpd]   Path traversal prevention: OK");

    // Test 9: contains_header_end detection.
    assert!(contains_header_end(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n"));
    assert!(!contains_header_end(b"GET / HTTP/1.1\r\nHost: x\r\n"));
    assert!(contains_header_end(b"\r\n\r\n"));
    assert!(!contains_header_end(b"\r\n\r"));
    serial_println!("[httpd]   Header end detection: OK");

    // Test 10: process_http_request for valid GET.
    let resp = process_http_request(b"GET /proc/version HTTP/1.1\r\nHost: x\r\n\r\n");
    let resp_str = core::str::from_utf8(&resp).unwrap_or("");
    // /proc/version should exist and return 200.
    assert!(resp_str.starts_with("HTTP/1.1 200 OK\r\n") ||
            resp_str.starts_with("HTTP/1.1 404 Not Found\r\n"),
            "Expected 200 or 404 for /proc/version");
    serial_println!("[httpd]   process_http_request: OK");

    // Test 11: process_http_request rejects POST.
    let resp2 = process_http_request(b"POST /index.html HTTP/1.1\r\nHost: x\r\n\r\n");
    let resp2_str = core::str::from_utf8(&resp2).unwrap_or("");
    assert!(resp2_str.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));
    serial_println!("[httpd]   POST rejection: OK");

    // Test 12: process_http_request with malformed request.
    let resp3 = process_http_request(b"GARBAGE");
    let resp3_str = core::str::from_utf8(&resp3).unwrap_or("");
    assert!(resp3_str.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    serial_println!("[httpd]   Malformed request handling: OK");

    serial_println!("[httpd] Self-test PASSED (12 tests)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("/"), "/");
        assert_eq!(normalize_path("/foo/../bar"), "/bar");
        assert_eq!(normalize_path("/../../../etc/passwd"), "/etc/passwd");
    }

    #[test]
    fn test_percent_decode() {
        assert_eq!(percent_decode("/foo%20bar"), "/foo bar");
        assert_eq!(percent_decode("%41%42%43"), "ABC");
    }
}
