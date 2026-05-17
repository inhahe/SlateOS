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
    let mut output = String::with_capacity((input.len() + 2) / 3 * 4);
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
// In the real OurOS environment, these would use socket syscalls (connect, send,
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
    let _ = writeln!(req, "User-Agent: fetch/1.0 (OurOS)\r");

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
            .any(|(k, _)| k.to_ascii_lowercase() == "content-type");
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

fn parse_args() -> Result<Config, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut config = Config::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                process::exit(EXIT_OK);
            }
            "-o" | "--output" => {
                i += 1;
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
                i += 1;
                let val = args
                    .get(i)
                    .ok_or("-H requires a header argument (\"Name: Value\")")?;
                let (name, value) = parse_header_arg(val)?;
                config.headers.push((name, value));
            }
            "-X" | "--method" => {
                i += 1;
                let val = args.get(i).ok_or("-X requires a method argument")?;
                config.method = Some(val.to_ascii_uppercase());
            }
            "-d" | "--data" => {
                i += 1;
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
                i += 1;
                let val = args.get(i).ok_or("-u requires user:password argument")?;
                config.user_pass = Some(val.clone());
            }
            "--timeout" => {
                i += 1;
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
        i += 1;
    }

    if config.urls.is_empty() {
        return Err("no URL specified".to_string());
    }

    Ok(config)
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
fetch - HTTP fetch utility for OurOS

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
    let config = match parse_args() {
        Ok(c) => c,
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
