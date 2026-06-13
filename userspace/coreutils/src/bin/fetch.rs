//! fetch -- HTTP fetch utility (wget/curl-like).
//!
//! Downloads files over HTTP/1.1. Supports GET, POST, PUT, DELETE, HEAD,
//! redirects, chunked transfer-encoding, basic auth, custom headers, and
//! progress indication.

use std::env;
use std::fmt::Write as FmtWrite;
#[allow(unused_imports)]
use std::io::{self, Read, Write};
use std::process;

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

const EXIT_OK: i32 = 0;
const EXIT_HTTP_ERROR: i32 = 1;
const EXIT_CONN_ERROR: i32 = 2;
const EXIT_BAD_ARGS: i32 = 3;

// ---------------------------------------------------------------------------
// Configuration from CLI arguments
// ---------------------------------------------------------------------------

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Config {
    urls: Vec<String>,
    output_file: Option<String>,
    output_from_url: bool,
    quiet: bool,
    verbose: bool,
    headers: Vec<(String, String)>,
    method: Option<String>,
    data: Option<String>,
    follow_redirects: bool,
    max_redirects: u32,
    head_only: bool,
    user_pass: Option<String>,
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
    host: String,
    port: u16,
    path: String,
    query: Option<String>,
}

impl ParsedUrl {
    fn parse(raw: &str) -> Result<Self, String> {
        let (scheme, rest) = if let Some(idx) = raw.find("://") {
            (raw[..idx].to_ascii_lowercase(), &raw[idx + 3..])
        } else {
            // Default to http if no scheme given.
            ("http".to_string(), raw)
        };

        let default_port: u16 = match scheme.as_str() {
            "http" => 80,
            "https" => 443,
            other => return Err(format!("unsupported scheme: {other}")),
        };

        // Split host+port from path.
        let (authority, path_and_query) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, "/"),
        };

        // Separate host from port.
        let (host, port) = if let Some(colon_idx) = authority.rfind(':') {
            let port_str = &authority[colon_idx + 1..];
            match port_str.parse::<u16>() {
                Ok(p) => (authority[..colon_idx].to_string(), p),
                Err(_) => (authority.to_string(), default_port),
            }
        } else {
            (authority.to_string(), default_port)
        };

        if host.is_empty() {
            return Err("empty host in URL".to_string());
        }

        // Separate path from query string.
        let (path, query) = match path_and_query.find('?') {
            Some(idx) => (
                path_and_query[..idx].to_string(),
                Some(path_and_query[idx + 1..].to_string()),
            ),
            None => (path_and_query.to_string(), None),
        };

        Ok(Self {
            scheme,
            host,
            port,
            path,
            query,
        })
    }

    /// Full request-target (path + optional query).
    fn request_target(&self) -> String {
        match &self.query {
            Some(q) => format!("{}?{}", self.path, q),
            None => self.path.clone(),
        }
    }

    /// Derive a filename from the URL path (last path segment, or "index.html").
    fn filename(&self) -> String {
        let segment = self.path.rsplit('/').next().unwrap_or("");
        if segment.is_empty() {
            "index.html".to_string()
        } else {
            segment.to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Base64 encoder (for basic auth)
// ---------------------------------------------------------------------------

const BASE64_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(input: &[u8]) -> String {
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i] as u32;
        let b1 = if i + 1 < input.len() { input[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < input.len() { input[i + 2] as u32 } else { 0 };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        output.push(BASE64_CHARS[((triple >> 18) & 0x3F) as usize] as char);
        output.push(BASE64_CHARS[((triple >> 12) & 0x3F) as usize] as char);

        if i + 1 < input.len() {
            output.push(BASE64_CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            output.push('=');
        }

        if i + 2 < input.len() {
            output.push(BASE64_CHARS[(triple & 0x3F) as usize] as char);
        } else {
            output.push('=');
        }

        i += 3;
    }
    output
}

// ---------------------------------------------------------------------------
// HTTP response
// ---------------------------------------------------------------------------

struct HttpResponse {
    status_code: u16,
    status_text: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn header_value(&self, name: &str) -> Option<&str> {
        let lower = name.to_ascii_lowercase();
        for (k, v) in &self.headers {
            if k.to_ascii_lowercase() == lower {
                return Some(v.as_str());
            }
        }
        None
    }

    fn content_length(&self) -> Option<usize> {
        self.header_value("content-length")
            .and_then(|v| v.trim().parse::<usize>().ok())
    }

    fn is_chunked(&self) -> bool {
        self.header_value("transfer-encoding")
            .map(|v| v.to_ascii_lowercase().contains("chunked"))
            .unwrap_or(false)
    }

    fn location(&self) -> Option<&str> {
        self.header_value("location")
    }
}

// ---------------------------------------------------------------------------
// Networking abstraction
// ---------------------------------------------------------------------------
// In the real SlateOS environment, these would use socket syscalls (connect, send,
// recv). For now we use std::net::TcpStream which can be swapped out later.

mod net {
    use std::io::{self, Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    pub struct Connection {
        stream: TcpStream,
    }

    impl Connection {
        pub fn connect(host: &str, port: u16, timeout_secs: u32) -> io::Result<Self> {
            let addr = format!("{host}:{port}");
            let timeout = Duration::from_secs(u64::from(timeout_secs));
            let stream = TcpStream::connect_timeout(
                &addr.parse().map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidInput, format!("bad address: {e}"))
                })?,
                timeout,
            )?;
            stream.set_read_timeout(Some(timeout))?;
            stream.set_write_timeout(Some(timeout))?;
            Ok(Self { stream })
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

fn build_request(
    method: &str,
    url: &ParsedUrl,
    headers: &[(String, String)],
    body: Option<&str>,
    user_pass: Option<&str>,
) -> String {
    let mut req = String::new();
    let target = url.request_target();
    let _ = writeln!(req, "{method} {target} HTTP/1.1\r");
    let _ = writeln!(req, "Host: {}\r", url.host);
    let _ = writeln!(req, "Connection: close\r");
    let _ = writeln!(req, "User-Agent: fetch/1.0 (SlateOS)\r");

    // Basic auth.
    if let Some(credentials) = user_pass {
        let encoded = base64_encode(credentials.as_bytes());
        let _ = writeln!(req, "Authorization: Basic {encoded}\r");
    }

    // Custom headers.
    for (name, value) in headers {
        let _ = writeln!(req, "{name}: {value}\r");
    }

    // Body handling.
    if let Some(data) = body {
        // Only set Content-Type if user hasn't provided one.
        let has_content_type = headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
        if !has_content_type {
            let _ = writeln!(req, "Content-Type: application/x-www-form-urlencoded\r");
        }
        let _ = writeln!(req, "Content-Length: {}\r", data.len());
        let _ = writeln!(req, "\r");
        req.push_str(data);
    } else {
        let _ = writeln!(req, "\r");
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
            .map_err(|e| format!("read error: {e}"))?;
        if n == 0 {
            return Err("connection closed before headers received".to_string());
        }
        raw.extend_from_slice(&buf[..n]);

        // Look for the end of headers (\r\n\r\n).
        if let Some(pos) = find_subsequence(&raw, b"\r\n\r\n") {
            header_end = pos + 4;
            break;
        }
        if raw.len() > 64 * 1024 {
            return Err("headers too large (>64KB)".to_string());
        }
    }

    // Parse status line and headers.
    let header_bytes = &raw[..header_end];
    let header_str = String::from_utf8_lossy(header_bytes);
    let mut lines = header_str.lines();

    let status_line = lines.next().ok_or("empty response")?;
    let (status_code, status_text) = parse_status_line(status_line)?;

    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        let trimmed = line.trim_end_matches('\r');
        if trimmed.is_empty() {
            break;
        }
        if let Some(colon_pos) = trimmed.find(':') {
            let name = trimmed[..colon_pos].trim().to_string();
            let value = trimmed[colon_pos + 1..].trim().to_string();
            headers.push((name, value));
        }
    }

    let mut response = HttpResponse {
        status_code,
        status_text,
        headers,
        body: Vec::new(),
    };

    // Read body.
    let body_start_data = &raw[header_end..];

    if response.is_chunked() {
        response.body = read_chunked_body(conn, body_start_data)?;
    } else if let Some(content_len) = response.content_length() {
        response.body = read_fixed_body(conn, body_start_data, content_len)?;
    } else {
        // Read until connection close.
        response.body = read_until_close(conn, body_start_data)?;
    }

    Ok(response)
}

fn parse_status_line(line: &str) -> Result<(u16, String), String> {
    // Format: HTTP/1.x STATUS_CODE STATUS_TEXT
    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Err(format!("malformed status line: {line}"));
    }
    let code = parts[1]
        .parse::<u16>()
        .map_err(|_| format!("invalid status code: {}", parts[1]))?;
    let text = if parts.len() >= 3 {
        parts[2].trim_end_matches('\r').to_string()
    } else {
        String::new()
    };
    Ok((code, text))
}

fn read_fixed_body(
    conn: &mut net::Connection,
    initial: &[u8],
    total: usize,
) -> Result<Vec<u8>, String> {
    let mut body = Vec::with_capacity(total);
    body.extend_from_slice(initial);

    let mut buf = [0u8; 8192];
    while body.len() < total {
        let n = conn
            .read_some(&mut buf)
            .map_err(|e| format!("read error: {e}"))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    body.truncate(total);
    Ok(body)
}

fn read_chunked_body(
    conn: &mut net::Connection,
    initial: &[u8],
) -> Result<Vec<u8>, String> {
    // Accumulate all remaining data, then parse chunks.
    let mut raw = Vec::from(initial);
    let mut buf = [0u8; 8192];

    // We need to parse chunk by chunk. Keep reading until we find the
    // terminating 0-length chunk.
    let mut body = Vec::new();

    loop {
        // Find a chunk size line in raw.
        let chunk_result = try_parse_chunk(&raw);
        match chunk_result {
            ChunkParse::NeedMore => {
                let n = conn
                    .read_some(&mut buf)
                    .map_err(|e| format!("read error: {e}"))?;
                if n == 0 {
                    // Connection closed; return what we have.
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
            }
            ChunkParse::Chunk { size, consumed } => {
                if size == 0 {
                    break; // Final chunk.
                }
                // We need `size` bytes of data + 2 bytes for trailing \r\n.
                let needed = consumed + size + 2;
                while raw.len() < needed {
                    let n = conn
                        .read_some(&mut buf)
                        .map_err(|e| format!("read error: {e}"))?;
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                }
                let data_start = consumed;
                let data_end = consumed + size;
                if raw.len() >= data_end {
                    body.extend_from_slice(&raw[data_start..data_end]);
                }
                // Advance past chunk data + \r\n.
                let advance = if raw.len() >= needed { needed } else { raw.len() };
                raw = raw[advance..].to_vec();
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

    let line = &data[..line_end];
    let hex_str = String::from_utf8_lossy(line);
    // Strip chunk extensions (anything after ';').
    let hex_part = hex_str.split(';').next().unwrap_or("").trim();

    match usize::from_str_radix(hex_part, 16) {
        Ok(size) => ChunkParse::Chunk {
            size,
            consumed: line_end + 2,
        },
        Err(_) => ChunkParse::Invalid(format!("bad chunk size: {hex_str}")),
    }
}

fn read_until_close(
    conn: &mut net::Connection,
    initial: &[u8],
) -> Result<Vec<u8>, String> {
    let mut body = Vec::from(initial);
    let mut buf = [0u8; 8192];
    loop {
        let n = conn
            .read_some(&mut buf)
            .map_err(|e| format!("read error: {e}"))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    Ok(body)
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// ---------------------------------------------------------------------------
// Human-readable file size
// ---------------------------------------------------------------------------

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

fn fetch_url(config: &Config, url_str: &str) -> i32 {
    let method = determine_method(config);

    let mut current_url = match ParsedUrl::parse(url_str) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("fetch: invalid URL '{url_str}': {e}");
            return EXIT_BAD_ARGS;
        }
    };

    if current_url.scheme == "https" {
        eprintln!("fetch: HTTPS not yet supported (no TLS implementation)");
        return EXIT_CONN_ERROR;
    }

    let mut redirects_remaining = if config.follow_redirects {
        config.max_redirects
    } else {
        0
    };

    loop {
        let request_method = if config.head_only { "HEAD" } else { &method };

        if config.verbose {
            eprintln!("> {request_method} {} HTTP/1.1", current_url.request_target());
            eprintln!("> Host: {}", current_url.host);
            for (name, value) in &config.headers {
                eprintln!("> {name}: {value}");
            }
            eprintln!(">");
        }

        let request = build_request(
            request_method,
            &current_url,
            &config.headers,
            config.data.as_deref(),
            config.user_pass.as_deref(),
        );

        // Connect.
        let mut conn =
            match net::Connection::connect(&current_url.host, current_url.port, config.timeout_secs)
            {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("fetch: connection to {}:{} failed: {e}", current_url.host, current_url.port);
                    return EXIT_CONN_ERROR;
                }
            };

        // Send request.
        if let Err(e) = conn.send_all(request.as_bytes()) {
            eprintln!("fetch: send failed: {e}");
            return EXIT_CONN_ERROR;
        }

        // Read response.
        let response = match read_response(&mut conn) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("fetch: {e}");
                return EXIT_CONN_ERROR;
            }
        };

        if config.verbose {
            eprintln!("< HTTP/1.1 {} {}", response.status_code, response.status_text);
            for (name, value) in &response.headers {
                eprintln!("< {name}: {value}");
            }
            eprintln!("<");
        }

        // Check for redirect.
        let is_redirect = matches!(
            response.status_code,
            301 | 302 | 303 | 307 | 308
        );

        if is_redirect && redirects_remaining > 0 {
            if let Some(location) = response.location() {
                let next_url = resolve_redirect(&current_url, location);
                match ParsedUrl::parse(&next_url) {
                    Ok(parsed) => {
                        if !config.quiet {
                            eprintln!("fetch: following redirect to {next_url}");
                        }
                        if parsed.scheme == "https" {
                            eprintln!("fetch: HTTPS not yet supported (no TLS implementation)");
                            return EXIT_CONN_ERROR;
                        }
                        current_url = parsed;
                        redirects_remaining -= 1;
                        continue;
                    }
                    Err(e) => {
                        eprintln!("fetch: invalid redirect URL '{location}': {e}");
                        return EXIT_HTTP_ERROR;
                    }
                }
            }
        } else if is_redirect && redirects_remaining == 0 && config.follow_redirects {
            eprintln!(
                "fetch: too many redirects (max {})",
                config.max_redirects
            );
            return EXIT_HTTP_ERROR;
        }

        // Print status to stderr.
        if !config.quiet {
            eprintln!(
                "HTTP {} {} [{}]",
                response.status_code,
                response.status_text,
                format_size(response.body.len())
            );
        }

        // HEAD request: show headers and exit.
        if config.head_only {
            for (name, value) in &response.headers {
                println!("{name}: {value}");
            }
            return if response.status_code < 400 {
                EXIT_OK
            } else {
                EXIT_HTTP_ERROR
            };
        }

        // Determine output destination.
        let write_result = write_output(config, &current_url, &response.body);
        if let Err(e) = write_result {
            eprintln!("fetch: write error: {e}");
            return EXIT_CONN_ERROR;
        }

        return if response.status_code >= 400 {
            EXIT_HTTP_ERROR
        } else {
            EXIT_OK
        };
    }
}

fn determine_method(config: &Config) -> String {
    if let Some(ref m) = config.method {
        m.to_ascii_uppercase()
    } else if config.data.is_some() {
        "POST".to_string()
    } else {
        "GET".to_string()
    }
}

fn resolve_redirect(base: &ParsedUrl, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        // Absolute URL.
        location.to_string()
    } else if location.starts_with('/') {
        // Absolute path, same origin.
        format!("{}://{}:{}{}", base.scheme, base.host, base.port, location)
    } else {
        // Relative path (append to current directory).
        let dir = match base.path.rfind('/') {
            Some(idx) => &base.path[..=idx],
            None => "/",
        };
        format!("{}://{}:{}{}{}", base.scheme, base.host, base.port, dir, location)
    }
}

fn write_output(config: &Config, url: &ParsedUrl, body: &[u8]) -> io::Result<()> {
    if let Some(ref path) = config.output_file {
        std::fs::write(path, body)?;
        if !config.quiet {
            eprintln!("Saved to: {path} [{}]", format_size(body.len()));
        }
    } else if config.output_from_url {
        let filename = url.filename();
        std::fs::write(&filename, body)?;
        if !config.quiet {
            eprintln!("Saved to: {filename} [{}]", format_size(body.len()));
        }
    } else {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        out.write_all(body)?;
        out.flush()?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

/// Outcome of CLI parsing: either a usable Config, a fatal error, or the
/// caller-requested help screen (so the test-only `parse_args` doesn't
/// need to call `process::exit`).
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum ParseOutcome {
    Config(Config),
    Help,
}

fn parse_args(args: &[String]) -> Result<ParseOutcome, String> {
    let mut config = Config::new();
    let mut i = 0;

    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "-h" | "--help" => {
                return Ok(ParseOutcome::Help);
            }
            "-o" | "--output" => {
                i = i.saturating_add(1);
                let val = args.get(i).ok_or("-o requires a filename argument")?;
                config.output_file = Some(val.clone());
            }
            "-O" => {
                config.output_from_url = true;
            }
            "-q" | "--quiet" => {
                config.quiet = true;
            }
            "-v" | "--verbose" => {
                config.verbose = true;
            }
            "-H" | "--header" => {
                i = i.saturating_add(1);
                let val = args
                    .get(i)
                    .ok_or("-H requires a header argument (\"Name: Value\")")?;
                let (name, value) = parse_header_arg(val)?;
                config.headers.push((name, value));
            }
            "-X" | "--method" => {
                i = i.saturating_add(1);
                let val = args.get(i).ok_or("-X requires a method argument")?;
                config.method = Some(val.to_ascii_uppercase());
            }
            "-d" | "--data" => {
                i = i.saturating_add(1);
                let val = args.get(i).ok_or("-d requires a data argument")?;
                config.data = Some(val.clone());
            }
            "-L" | "--follow" => {
                config.follow_redirects = true;
            }
            "--no-follow" => {
                config.follow_redirects = false;
            }
            "-I" | "--head" => {
                config.head_only = true;
            }
            "-u" | "--user" => {
                i = i.saturating_add(1);
                let val = args.get(i).ok_or("-u requires user:password argument")?;
                config.user_pass = Some(val.clone());
            }
            "--timeout" => {
                i = i.saturating_add(1);
                let val = args.get(i).ok_or("--timeout requires a number")?;
                config.timeout_secs = val
                    .parse::<u32>()
                    .map_err(|_| format!("invalid timeout value: {val}"))?;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option: {other}"));
            }
            _ => {
                config.urls.push(arg.clone());
            }
        }
        i = i.saturating_add(1);
    }

    if config.urls.is_empty() {
        return Err("no URL specified".to_string());
    }

    Ok(ParseOutcome::Config(config))
}

fn parse_header_arg(raw: &str) -> Result<(String, String), String> {
    match raw.find(':') {
        Some(idx) => {
            let name = raw[..idx].trim().to_string();
            let value = raw[idx + 1..].trim().to_string();
            if name.is_empty() {
                return Err(format!("empty header name in: {raw}"));
            }
            Ok((name, value))
        }
        None => Err(format!("invalid header format (expected \"Name: Value\"): {raw}")),
    }
}

// ---------------------------------------------------------------------------
// Help text
// ---------------------------------------------------------------------------

fn print_help() {
    let help = "\
fetch - HTTP fetch utility for SlateOS

Usage: fetch [OPTIONS] URL [URL...]

Downloads files over HTTP. Similar to wget/curl.

Options:
  -o, --output FILE    Save response body to FILE
  -O                   Save to file named from URL (last path segment)
  -q, --quiet          Suppress progress/status output on stderr
  -v, --verbose        Show request and response headers
  -H, --header HEADER  Add custom header (format: \"Name: Value\")
                       Can be specified multiple times
  -X, --method METHOD  HTTP method (default: GET, or POST if -d is used)
  -d, --data DATA      Request body (implies POST if -X not specified)
                       Sets Content-Type: application/x-www-form-urlencoded
  -L, --follow         Follow redirects (default: yes, up to 10 hops)
  --no-follow          Don't follow redirects
  -I, --head           Send HEAD request, display response headers only
  -u, --user USER:PASS HTTP Basic authentication credentials
  --timeout SECS       Connection/read timeout in seconds (default: 30)
  -h, --help           Show this help message

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

Note: HTTPS is not yet supported (requires TLS implementation).
";
    print!("{help}");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let config = match parse_args(&args) {
        Ok(ParseOutcome::Config(c)) => c,
        Ok(ParseOutcome::Help) => {
            print_help();
            process::exit(EXIT_OK);
        }
        Err(e) => {
            eprintln!("fetch: {e}");
            eprintln!("Try 'fetch --help' for usage information.");
            process::exit(EXIT_BAD_ARGS);
        }
    };

    let mut worst_exit = EXIT_OK;

    for url in &config.urls {
        let code = fetch_url(&config, url);
        if code > worst_exit {
            worst_exit = code;
        }
    }

    process::exit(worst_exit);
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

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    fn cfg(args: &[&str]) -> Config {
        match parse_args(&s(args)).unwrap() {
            ParseOutcome::Config(c) => c,
            ParseOutcome::Help => panic!("expected Config, got Help"),
        }
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
        assert_eq!(c.urls, vec!["http://example.com/".to_string()]);
        assert!(c.follow_redirects);
        assert_eq!(c.timeout_secs, 30);
    }

    #[test]
    fn parse_args_help_flag() {
        let out = parse_args(&s(&["-h"])).unwrap();
        assert_eq!(out, ParseOutcome::Help);
        let out = parse_args(&s(&["--help"])).unwrap();
        assert_eq!(out, ParseOutcome::Help);
    }

    #[test]
    fn parse_args_output_file() {
        let c = cfg(&["-o", "out.html", "http://x/"]);
        assert_eq!(c.output_file, Some("out.html".to_string()));
        let c = cfg(&["--output", "out.html", "http://x/"]);
        assert_eq!(c.output_file, Some("out.html".to_string()));
    }

    #[test]
    fn parse_args_missing_output_value_errors() {
        let err = parse_args(&s(&["-o"])).unwrap_err();
        assert!(err.contains("-o"));
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
    }

    #[test]
    fn parse_args_method_uppercased() {
        let c = cfg(&["-X", "post", "http://x/"]);
        assert_eq!(c.method, Some("POST".to_string()));
    }

    #[test]
    fn parse_args_data_implies_post_via_determine_method() {
        // -d alone doesn't change config.method, but determine_method() picks POST.
        let c = cfg(&["-d", "key=value", "http://x/"]);
        assert_eq!(c.data, Some("key=value".to_string()));
        assert_eq!(c.method, None);
        assert_eq!(determine_method(&c), "POST");
    }

    #[test]
    fn parse_args_header_collects_multiple() {
        let c = cfg(&[
            "-H", "Accept: application/json",
            "-H", "X-Custom: 42",
            "http://x/",
        ]);
        assert_eq!(
            c.headers,
            vec![
                ("Accept".to_string(), "application/json".to_string()),
                ("X-Custom".to_string(), "42".to_string()),
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
        assert_eq!(c.user_pass, Some("admin:secret".to_string()));
    }

    #[test]
    fn parse_args_timeout_numeric() {
        let c = cfg(&["--timeout", "5", "http://x/"]);
        assert_eq!(c.timeout_secs, 5);
    }

    #[test]
    fn parse_args_bad_timeout_errors() {
        let err = parse_args(&s(&["--timeout", "abc", "http://x/"])).unwrap_err();
        assert!(err.contains("invalid timeout"));
    }

    #[test]
    fn parse_args_unknown_flag_errors() {
        let err = parse_args(&s(&["--bogus", "http://x/"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn parse_args_multiple_urls() {
        let c = cfg(&["http://a/", "http://b/"]);
        assert_eq!(c.urls.len(), 2);
    }

    // ---------- parse_header_arg ----------

    #[test]
    fn parse_header_splits_on_first_colon() {
        let (n, v) = parse_header_arg("Name: value:extra").unwrap();
        assert_eq!(n, "Name");
        assert_eq!(v, "value:extra");
    }

    #[test]
    fn parse_header_trims_whitespace() {
        let (n, v) = parse_header_arg("  X-Foo  :   bar  ").unwrap();
        assert_eq!(n, "X-Foo");
        assert_eq!(v, "bar");
    }

    #[test]
    fn parse_header_no_colon_errors() {
        assert!(parse_header_arg("garbage").is_err());
    }

    #[test]
    fn parse_header_empty_name_errors() {
        assert!(parse_header_arg(": value").is_err());
    }

    // ---------- ParsedUrl ----------

    #[test]
    fn parsed_url_http_with_path() {
        let u = ParsedUrl::parse("http://example.com/foo/bar").unwrap();
        assert_eq!(u.scheme, "http");
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, 80);
        assert_eq!(u.path, "/foo/bar");
        assert_eq!(u.query, None);
    }

    #[test]
    fn parsed_url_https_default_port() {
        let u = ParsedUrl::parse("https://example.com/").unwrap();
        assert_eq!(u.scheme, "https");
        assert_eq!(u.port, 443);
    }

    #[test]
    fn parsed_url_explicit_port() {
        let u = ParsedUrl::parse("http://example.com:8080/x").unwrap();
        assert_eq!(u.port, 8080);
    }

    #[test]
    fn parsed_url_no_scheme_defaults_to_http() {
        let u = ParsedUrl::parse("example.com/foo").unwrap();
        assert_eq!(u.scheme, "http");
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, 80);
        assert_eq!(u.path, "/foo");
    }

    #[test]
    fn parsed_url_no_path_defaults_to_slash() {
        let u = ParsedUrl::parse("http://example.com").unwrap();
        assert_eq!(u.path, "/");
    }

    #[test]
    fn parsed_url_query_string() {
        let u = ParsedUrl::parse("http://example.com/search?q=hello&n=10").unwrap();
        assert_eq!(u.path, "/search");
        assert_eq!(u.query, Some("q=hello&n=10".to_string()));
    }

    #[test]
    fn parsed_url_unsupported_scheme_errors() {
        assert!(ParsedUrl::parse("ftp://example.com/").is_err());
    }

    #[test]
    fn parsed_url_empty_host_errors() {
        assert!(ParsedUrl::parse("http:///foo").is_err());
    }

    #[test]
    fn parsed_url_request_target_includes_query() {
        let u = ParsedUrl::parse("http://h/p?x=1").unwrap();
        assert_eq!(u.request_target(), "/p?x=1");
    }

    #[test]
    fn parsed_url_request_target_no_query() {
        let u = ParsedUrl::parse("http://h/p").unwrap();
        assert_eq!(u.request_target(), "/p");
    }

    #[test]
    fn parsed_url_filename_from_last_segment() {
        let u = ParsedUrl::parse("http://h/dir/file.tar").unwrap();
        assert_eq!(u.filename(), "file.tar");
    }

    #[test]
    fn parsed_url_filename_trailing_slash_is_index_html() {
        let u = ParsedUrl::parse("http://h/dir/").unwrap();
        assert_eq!(u.filename(), "index.html");
    }

    // ---------- base64_encode ----------

    #[test]
    fn base64_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn base64_single_byte() {
        // "f" -> "Zg=="
        assert_eq!(base64_encode(b"f"), "Zg==");
    }

    #[test]
    fn base64_two_bytes() {
        // "fo" -> "Zm8="
        assert_eq!(base64_encode(b"fo"), "Zm8=");
    }

    #[test]
    fn base64_three_bytes() {
        // "foo" -> "Zm9v"
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn base64_credential_string() {
        // "admin:secret" round-tripped against a known reference.
        assert_eq!(base64_encode(b"admin:secret"), "YWRtaW46c2VjcmV0");
    }

    // ---------- determine_method ----------

    #[test]
    fn determine_method_default_is_get() {
        let c = Config::new();
        assert_eq!(determine_method(&c), "GET");
    }

    #[test]
    fn determine_method_data_implies_post() {
        let mut c = Config::new();
        c.data = Some("x=1".to_string());
        assert_eq!(determine_method(&c), "POST");
    }

    #[test]
    fn determine_method_explicit_method_wins_over_data() {
        let mut c = Config::new();
        c.data = Some("x=1".to_string());
        c.method = Some("put".to_string());
        assert_eq!(determine_method(&c), "PUT");
    }

    // ---------- build_request ----------

    #[test]
    fn build_request_get_minimal() {
        let u = ParsedUrl::parse("http://example.com/path").unwrap();
        let req = build_request("GET", &u, &[], None, None);
        assert!(req.starts_with("GET /path HTTP/1.1\r\n"));
        assert!(req.contains("Host: example.com\r\n"));
        assert!(req.contains("Connection: close\r\n"));
        // Empty body still terminates with a blank line.
        assert!(req.ends_with("\r\n\r\n"));
    }

    #[test]
    fn build_request_post_adds_content_length() {
        let u = ParsedUrl::parse("http://example.com/").unwrap();
        let req = build_request("POST", &u, &[], Some("hello"), None);
        assert!(req.contains("Content-Length: 5\r\n"));
        assert!(req.contains("Content-Type: application/x-www-form-urlencoded\r\n"));
        assert!(req.ends_with("hello"));
    }

    #[test]
    fn build_request_custom_content_type_suppresses_default() {
        let u = ParsedUrl::parse("http://example.com/").unwrap();
        let headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        let req = build_request("POST", &u, &headers, Some("{}"), None);
        // Only the user-supplied content-type should appear.
        assert!(req.contains("Content-Type: application/json\r\n"));
        assert!(!req.contains("Content-Type: application/x-www-form-urlencoded"));
    }

    #[test]
    fn build_request_basic_auth_header_present() {
        let u = ParsedUrl::parse("http://example.com/").unwrap();
        let req = build_request("GET", &u, &[], None, Some("admin:secret"));
        assert!(req.contains("Authorization: Basic YWRtaW46c2VjcmV0\r\n"));
    }

    #[test]
    fn build_request_includes_query() {
        let u = ParsedUrl::parse("http://example.com/search?q=hi").unwrap();
        let req = build_request("GET", &u, &[], None, None);
        assert!(req.starts_with("GET /search?q=hi HTTP/1.1\r\n"));
    }

    // ---------- parse_status_line ----------

    #[test]
    fn status_line_basic() {
        let (code, text) = parse_status_line("HTTP/1.1 200 OK").unwrap();
        assert_eq!(code, 200);
        assert_eq!(text, "OK");
    }

    #[test]
    fn status_line_multiword_text() {
        let (code, text) = parse_status_line("HTTP/1.1 404 Not Found").unwrap();
        assert_eq!(code, 404);
        assert_eq!(text, "Not Found");
    }

    #[test]
    fn status_line_no_reason_phrase() {
        let (code, text) = parse_status_line("HTTP/1.0 204").unwrap();
        assert_eq!(code, 204);
        assert_eq!(text, "");
    }

    #[test]
    fn status_line_malformed_errors() {
        assert!(parse_status_line("garbage").is_err());
        assert!(parse_status_line("HTTP/1.1 not-a-number OK").is_err());
    }

    // ---------- HttpResponse helpers ----------

    fn mk_resp(headers: Vec<(&str, &str)>) -> HttpResponse {
        HttpResponse {
            status_code: 200,
            status_text: "OK".to_string(),
            headers: headers
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: Vec::new(),
        }
    }

    #[test]
    fn response_header_value_case_insensitive() {
        let r = mk_resp(vec![("Content-Type", "text/html")]);
        assert_eq!(r.header_value("content-type"), Some("text/html"));
        assert_eq!(r.header_value("CONTENT-TYPE"), Some("text/html"));
    }

    #[test]
    fn response_header_value_missing_is_none() {
        let r = mk_resp(vec![]);
        assert_eq!(r.header_value("Location"), None);
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
        let r = mk_resp(vec![("Transfer-Encoding", "chunked")]);
        assert!(r.is_chunked());
        let r = mk_resp(vec![("Transfer-Encoding", "gzip, chunked")]);
        assert!(r.is_chunked());
    }

    #[test]
    fn response_is_chunked_false_when_absent() {
        let r = mk_resp(vec![]);
        assert!(!r.is_chunked());
    }

    #[test]
    fn response_location_field() {
        let r = mk_resp(vec![("Location", "http://other/")]);
        assert_eq!(r.location(), Some("http://other/"));
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
        // No CRLF yet.
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
        let base = ParsedUrl::parse("http://a.com/x").unwrap();
        assert_eq!(
            resolve_redirect(&base, "http://b.com/y"),
            "http://b.com/y"
        );
    }

    #[test]
    fn redirect_absolute_path_keeps_origin() {
        let base = ParsedUrl::parse("http://a.com/x").unwrap();
        assert_eq!(
            resolve_redirect(&base, "/new"),
            "http://a.com:80/new"
        );
    }

    #[test]
    fn redirect_relative_appends_to_current_dir() {
        let base = ParsedUrl::parse("http://a.com/dir/page").unwrap();
        assert_eq!(
            resolve_redirect(&base, "other"),
            "http://a.com:80/dir/other"
        );
    }
}
