//! # `Slate OS` HTTP Client Library
//!
//! A native HTTP/1.1 client library for `Slate OS`. Provides URL parsing, request building,
//! response parsing, cookie handling, and HTTP protocol serialization/deserialization.
//!
//! This library is used by the package manager and other applications for network fetching.
//! It implements the HTTP/1.1 protocol with support for chunked transfer encoding,
//! redirects, cookies, and common authentication schemes.
//!
//! # Example
//!
//! ```rust
//! use httpclient::RequestBuilder;
//!
//! let request = RequestBuilder::get("http://example.com/api/data")
//!     .expect("valid URL")
//!     .header("Accept", "application/json")
//!     .timeout(5000)
//!     .build();
//! ```

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
#[allow(unused_imports)]
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during HTTP operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The URL could not be parsed.
    InvalidUrl(String),
    /// The connection to the remote server failed.
    ConnectionFailed(String),
    /// The request timed out.
    Timeout,
    /// Too many redirects were followed.
    TooManyRedirects,
    /// The response from the server was malformed.
    InvalidResponse(String),
    /// An I/O error occurred.
    Io(String),
    /// A header name or value was invalid.
    InvalidHeader(String),
    /// A text encoding error occurred (e.g., response body is not valid UTF-8).
    EncodingError,
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(msg) => write!(f, "invalid URL: {msg}"),
            Self::ConnectionFailed(msg) => write!(f, "connection failed: {msg}"),
            Self::Timeout => write!(f, "request timed out"),
            Self::TooManyRedirects => write!(f, "too many redirects"),
            Self::InvalidResponse(msg) => write!(f, "invalid response: {msg}"),
            Self::Io(msg) => write!(f, "I/O error: {msg}"),
            Self::InvalidHeader(msg) => write!(f, "invalid header: {msg}"),
            Self::EncodingError => write!(f, "encoding error"),
        }
    }
}

// ---------------------------------------------------------------------------
// URL parsing
// ---------------------------------------------------------------------------

/// A parsed URL with scheme, host, port, path, query, and fragment components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    /// The scheme (e.g., "http" or "https").
    pub scheme: String,
    /// The hostname.
    pub host: String,
    /// The port number (defaults to 80 for http, 443 for https).
    pub port: u16,
    /// The path component including the leading `/`.
    pub path: String,
    /// The query string without the leading `?`.
    pub query: Option<String>,
    /// The fragment without the leading `#`.
    pub fragment: Option<String>,
}

impl Url {
    /// Parse a URL string into its components.
    ///
    /// Supports `http://` and `https://` schemes. If no port is specified,
    /// defaults to 80 for HTTP and 443 for HTTPS.
    ///
    /// # Errors
    ///
    /// Returns `HttpError::InvalidUrl` if the URL is malformed.
    pub fn parse(url: &str) -> Result<Self, HttpError> {
        let (scheme, rest) = if let Some(stripped) = url.strip_prefix("https://") {
            ("https".to_string(), stripped)
        } else if let Some(stripped) = url.strip_prefix("http://") {
            ("http".to_string(), stripped)
        } else {
            return Err(HttpError::InvalidUrl(
                "URL must start with http:// or https://".to_string(),
            ));
        };

        if rest.is_empty() {
            return Err(HttpError::InvalidUrl("empty host".to_string()));
        }

        // Split off path+query+fragment from authority
        let (authority, path_query_fragment) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, "/"),
        };

        if authority.is_empty() {
            return Err(HttpError::InvalidUrl("empty host".to_string()));
        }

        // Parse host and port from authority (ignoring userinfo for now)
        let default_port: u16 = if scheme == "https" { 443 } else { 80 };
        let (host, port) = Self::parse_authority(authority, default_port)?;

        if host.is_empty() {
            return Err(HttpError::InvalidUrl("empty host".to_string()));
        }

        // Split path, query, fragment
        let (path, query, fragment) = Self::parse_path_query_fragment(path_query_fragment);

        Ok(Self {
            scheme,
            host,
            port,
            path,
            query,
            fragment,
        })
    }

    /// Parse the authority section into host and port.
    fn parse_authority(authority: &str, default_port: u16) -> Result<(String, u16), HttpError> {
        // Check for IPv6 address in brackets
        if authority.starts_with('[') {
            // IPv6 literal
            let bracket_end = authority.find(']').ok_or_else(|| {
                HttpError::InvalidUrl("unclosed bracket in IPv6 address".to_string())
            })?;
            let host = authority[1..bracket_end].to_string();
            let after_bracket = &authority[bracket_end + 1..];
            let port = if let Some(port_str) = after_bracket.strip_prefix(':') {
                port_str.parse::<u16>().map_err(|_| {
                    HttpError::InvalidUrl(format!("invalid port: {port_str}"))
                })?
            } else {
                default_port
            };
            Ok((host, port))
        } else if let Some(colon_idx) = authority.rfind(':') {
            let host_part = &authority[..colon_idx];
            let port_str = &authority[colon_idx + 1..];
            // Only treat as port if the part after colon is a valid number
            match port_str.parse::<u16>() {
                Ok(port) => Ok((host_part.to_string(), port)),
                Err(_) => {
                    // If it doesn't parse as a port, the whole thing is the host
                    Ok((authority.to_string(), default_port))
                }
            }
        } else {
            Ok((authority.to_string(), default_port))
        }
    }

    /// Split a path string into path, query, and fragment components.
    fn parse_path_query_fragment(input: &str) -> (String, Option<String>, Option<String>) {
        let (path_and_query, fragment) = match input.find('#') {
            Some(idx) => (&input[..idx], Some(input[idx + 1..].to_string())),
            None => (input, None),
        };

        let (path, query) = match path_and_query.find('?') {
            Some(idx) => (
                path_and_query[..idx].to_string(),
                Some(path_and_query[idx + 1..].to_string()),
            ),
            None => (path_and_query.to_string(), None),
        };

        let final_path = if path.is_empty() {
            "/".to_string()
        } else {
            path
        };

        (final_path, query, fragment)
    }

    /// Return the path and query string combined, suitable for the HTTP request line.
    pub fn request_path(&self) -> String {
        match &self.query {
            Some(q) => format!("{}?{}", self.path, q),
            None => self.path.clone(),
        }
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}", self.scheme, self.host)?;

        let is_default_port = (self.scheme == "http" && self.port == 80)
            || (self.scheme == "https" && self.port == 443);

        if !is_default_port {
            write!(f, ":{}", self.port)?;
        }

        f.write_str(&self.path)?;

        if let Some(ref q) = self.query {
            write!(f, "?{q}")?;
        }

        if let Some(ref fragment) = self.fragment {
            write!(f, "#{fragment}")?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Headers
// ---------------------------------------------------------------------------

/// A collection of HTTP headers supporting case-insensitive lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headers {
    entries: Vec<(String, String)>,
}

impl Headers {
    /// Create an empty header collection.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Set a header value, replacing any existing header with the same name (case-insensitive).
    pub fn set(&mut self, name: &str, value: &str) {
        let lower = name.to_ascii_lowercase();
        // Remove existing entries with same name
        self.entries.retain(|(n, _)| n.to_ascii_lowercase() != lower);
        self.entries.push((name.to_string(), value.to_string()));
    }

    /// Get the first value for a header name (case-insensitive lookup).
    pub fn get(&self, name: &str) -> Option<&str> {
        let lower = name.to_ascii_lowercase();
        self.entries
            .iter()
            .find(|(n, _)| n.to_ascii_lowercase() == lower)
            .map(|(_, v)| v.as_str())
    }

    /// Get all values for a header name (case-insensitive lookup).
    pub fn get_all(&self, name: &str) -> Vec<&str> {
        let lower = name.to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|(n, _)| n.to_ascii_lowercase() == lower)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// Remove all headers with the given name (case-insensitive).
    pub fn remove(&mut self, name: &str) {
        let lower = name.to_ascii_lowercase();
        self.entries.retain(|(n, _)| n.to_ascii_lowercase() != lower);
    }

    /// Iterate over all header name-value pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }

    /// Check if a header with the given name exists (case-insensitive).
    pub fn contains(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        self.entries
            .iter()
            .any(|(n, _)| n.to_ascii_lowercase() == lower)
    }

    /// Return the number of headers stored.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return true if there are no headers.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Append a header without removing existing headers with the same name.
    /// Used when parsing responses that may have duplicate headers (e.g., Set-Cookie).
    pub fn append(&mut self, name: &str, value: &str) {
        self.entries.push((name.to_string(), value.to_string()));
    }
}

impl Default for Headers {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// HTTP Method
// ---------------------------------------------------------------------------

/// HTTP request methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// GET method — retrieve a resource.
    Get,
    /// POST method — submit data.
    Post,
    /// PUT method — replace a resource.
    Put,
    /// DELETE method — remove a resource.
    Delete,
    /// HEAD method — like GET but without response body.
    Head,
    /// PATCH method — partial modification of a resource.
    Patch,
    /// OPTIONS method — describe communication options.
    Options,
}

impl Method {
    /// Return the method as its HTTP wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Patch => "PATCH",
            Self::Options => "OPTIONS",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

/// A fully-constructed HTTP request ready to be serialized and sent.
#[derive(Debug, Clone)]
pub struct Request {
    /// The HTTP method.
    pub method: Method,
    /// The target URL.
    pub url: Url,
    /// Request headers.
    pub headers: Headers,
    /// Optional request body.
    pub body: Option<Vec<u8>>,
    /// Request timeout in milliseconds (0 means no timeout).
    pub timeout_ms: u32,
    /// Whether to automatically follow redirects.
    pub follow_redirects: bool,
    /// Maximum number of redirects to follow.
    pub max_redirects: u32,
}

impl Request {
    /// Serialize this request into the HTTP/1.1 wire format.
    ///
    /// Returns the complete byte sequence including request line, headers,
    /// blank line separator, and body (if present).
    pub fn serialize(&self) -> Vec<u8> {
        let mut output = Vec::with_capacity(256);

        // Request line: METHOD /path HTTP/1.1\r\n
        let request_line = format!(
            "{} {} HTTP/1.1\r\n",
            self.method.as_str(),
            self.url.request_path()
        );
        output.extend_from_slice(request_line.as_bytes());

        // Host header (required in HTTP/1.1)
        let host_header = if (self.url.scheme == "http" && self.url.port == 80)
            || (self.url.scheme == "https" && self.url.port == 443)
        {
            format!("Host: {}\r\n", self.url.host)
        } else {
            format!("Host: {}:{}\r\n", self.url.host, self.url.port)
        };
        output.extend_from_slice(host_header.as_bytes());

        // User-specified headers
        for (name, value) in self.headers.iter() {
            let header_line = format!("{name}: {value}\r\n");
            output.extend_from_slice(header_line.as_bytes());
        }

        // Content-Length if body is present and not already set
        if let Some(ref body_data) = self.body
            && !self.headers.contains("Content-Length")
        {
            let cl = format!("Content-Length: {}\r\n", body_data.len());
            output.extend_from_slice(cl.as_bytes());
        }

        // End of headers
        output.extend_from_slice(b"\r\n");

        // Body
        if let Some(ref body_data) = self.body {
            output.extend_from_slice(body_data);
        }

        output
    }
}

// ---------------------------------------------------------------------------
// Request Builder
// ---------------------------------------------------------------------------

/// A builder for constructing HTTP requests with a fluent API.
///
/// # Example
///
/// ```no_run
/// use httpclient::RequestBuilder;
/// let req = RequestBuilder::post("http://api.example.com/data")
///     .expect("valid URL")
///     .header("X-Custom", "value")
///     .content_type("application/json")
///     .json("{\"key\": \"value\"}")
///     .timeout(10000)
///     .build();
/// ```
#[derive(Debug, Clone)]
#[must_use = "RequestBuilder builds a Request only when `.build()` (or a send method) is called"]
pub struct RequestBuilder {
    method: Method,
    url: Url,
    headers: Headers,
    body: Option<Vec<u8>>,
    timeout_ms: u32,
    follow_redirects: bool,
    max_redirects: u32,
}

impl RequestBuilder {
    /// Create a GET request builder for the given URL.
    ///
    /// # Errors
    ///
    /// Returns `HttpError::InvalidUrl` if the URL cannot be parsed.
    pub fn get(url: &str) -> Result<Self, HttpError> {
        Self::new(Method::Get, url)
    }

    /// Create a POST request builder for the given URL.
    ///
    /// # Errors
    ///
    /// Returns `HttpError::InvalidUrl` if the URL cannot be parsed.
    pub fn post(url: &str) -> Result<Self, HttpError> {
        Self::new(Method::Post, url)
    }

    /// Create a PUT request builder for the given URL.
    ///
    /// # Errors
    ///
    /// Returns `HttpError::InvalidUrl` if the URL cannot be parsed.
    pub fn put(url: &str) -> Result<Self, HttpError> {
        Self::new(Method::Put, url)
    }

    /// Create a DELETE request builder for the given URL.
    ///
    /// # Errors
    ///
    /// Returns `HttpError::InvalidUrl` if the URL cannot be parsed.
    pub fn delete(url: &str) -> Result<Self, HttpError> {
        Self::new(Method::Delete, url)
    }

    /// Create a HEAD request builder for the given URL.
    ///
    /// # Errors
    ///
    /// Returns `HttpError::InvalidUrl` if the URL cannot be parsed.
    pub fn head(url: &str) -> Result<Self, HttpError> {
        Self::new(Method::Head, url)
    }

    /// Create a new request builder with the given method and URL.
    fn new(method: Method, url: &str) -> Result<Self, HttpError> {
        let parsed_url = Url::parse(url)?;
        Ok(Self {
            method,
            url: parsed_url,
            headers: Headers::new(),
            body: None,
            timeout_ms: 30_000, // 30 second default timeout
            follow_redirects: true,
            max_redirects: 10,
        })
    }

    /// Add a custom header to the request.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.set(name, value);
        self
    }

    /// Set the Content-Type header.
    pub fn content_type(self, ct: &str) -> Self {
        self.header("Content-Type", ct)
    }

    /// Set the User-Agent header.
    pub fn user_agent(self, ua: &str) -> Self {
        self.header("User-Agent", ua)
    }

    /// Set a Bearer token in the Authorization header.
    pub fn bearer_token(self, token: &str) -> Self {
        let value = format!("Bearer {token}");
        self.header("Authorization", &value)
    }

    /// Set HTTP Basic authentication credentials.
    ///
    /// Encodes `username:password` in base64 for the Authorization header.
    pub fn basic_auth(self, username: &str, password: &str) -> Self {
        let credentials = format!("{username}:{password}");
        let encoded = base64_encode(credentials.as_bytes());
        let value = format!("Basic {encoded}");
        self.header("Authorization", &value)
    }

    /// Set the request body as raw bytes.
    pub fn body(mut self, data: Vec<u8>) -> Self {
        self.body = Some(data);
        self
    }

    /// Set the request body as a JSON string and set Content-Type to application/json.
    pub fn json(mut self, json_str: &str) -> Self {
        self.body = Some(json_str.as_bytes().to_vec());
        self.headers.set("Content-Type", "application/json");
        self
    }

    /// Set the request timeout in milliseconds.
    pub fn timeout(mut self, ms: u32) -> Self {
        self.timeout_ms = ms;
        self
    }

    /// Set whether to follow HTTP redirects automatically.
    pub fn follow_redirects(mut self, follow: bool) -> Self {
        self.follow_redirects = follow;
        self
    }

    /// Build the final `Request` from this builder.
    pub fn build(self) -> Request {
        Request {
            method: self.method,
            url: self.url,
            headers: self.headers,
            body: self.body,
            timeout_ms: self.timeout_ms,
            follow_redirects: self.follow_redirects,
            max_redirects: self.max_redirects,
        }
    }
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

/// An HTTP response including status, headers, and body.
#[derive(Debug, Clone)]
pub struct Response {
    /// The HTTP status code (e.g., 200, 404, 500).
    pub status: u16,
    /// The status text (e.g., "OK", "Not Found").
    pub status_text: String,
    /// Response headers.
    pub headers: Headers,
    /// The response body bytes.
    pub body: Vec<u8>,
    /// The final URL (may differ from request URL if redirects were followed).
    pub url: Url,
}

impl Response {
    /// Return the HTTP status code.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// Return true if the status code indicates success (200-299).
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Return true if the status code indicates a redirect (300-399).
    pub fn is_redirect(&self) -> bool {
        (300..400).contains(&self.status)
    }

    /// Return true if the status code indicates a client error (400-499).
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status)
    }

    /// Return true if the status code indicates a server error (500-599).
    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status)
    }

    /// Get a response header value by name (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }

    /// Get the Content-Type header value.
    pub fn content_type(&self) -> Option<&str> {
        self.headers.get("Content-Type")
    }

    /// Get the Content-Length header value as a `usize`.
    pub fn content_length(&self) -> Option<usize> {
        self.headers
            .get("Content-Length")
            .and_then(|v| v.parse::<usize>().ok())
    }

    /// Interpret the response body as UTF-8 text.
    ///
    /// # Errors
    ///
    /// Returns `HttpError::EncodingError` if the body is not valid UTF-8.
    pub fn text(&self) -> Result<&str, HttpError> {
        core::str::from_utf8(&self.body).map_err(|_| HttpError::EncodingError)
    }

    /// Return the raw response body bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.body
    }
}

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

/// Parse an HTTP/1.1 response from raw bytes.
///
/// Handles both Content-Length and chunked Transfer-Encoding.
///
/// # Errors
///
/// Returns `HttpError::InvalidResponse` if the response is malformed.
pub fn parse_response(data: &[u8], request_url: &Url) -> Result<Response, HttpError> {
    let header_end = find_header_end(data).ok_or_else(|| {
        HttpError::InvalidResponse("no header/body separator found".to_string())
    })?;

    let header_bytes = &data[..header_end];
    let header_str = core::str::from_utf8(header_bytes)
        .map_err(|_| HttpError::InvalidResponse("headers are not valid UTF-8".to_string()))?;

    let mut lines = header_str.split("\r\n");

    // Parse status line
    let status_line = lines.next().ok_or_else(|| {
        HttpError::InvalidResponse("empty response".to_string())
    })?;

    let (status, status_text) = parse_status_line(status_line)?;

    // Parse headers
    let mut headers = Headers::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = parse_header_line(line) {
            headers.append(name, value);
        }
    }

    // Parse body: the body starts after the \r\n\r\n separator
    let body_start = header_end + 4; // skip \r\n\r\n
    let body_data = if body_start < data.len() {
        &data[body_start..]
    } else {
        &[] as &[u8]
    };

    // Determine body decoding strategy
    let body = if headers
        .get("Transfer-Encoding")
        .is_some_and(|te| te.contains("chunked"))
    {
        decode_chunked(body_data)?
    } else if let Some(cl) = headers
        .get("Content-Length")
        .and_then(|v| v.parse::<usize>().ok())
    {
        let available = body_data.len().min(cl);
        body_data[..available].to_vec()
    } else {
        body_data.to_vec()
    };

    Ok(Response {
        status,
        status_text,
        headers,
        body,
        url: request_url.clone(),
    })
}

/// Find the index of the \r\n\r\n header terminator in raw bytes.
fn find_header_end(data: &[u8]) -> Option<usize> {
    let needle = b"\r\n\r\n";
    data.windows(4).position(|w| w == needle)
}

/// Parse an HTTP status line like "HTTP/1.1 200 OK".
fn parse_status_line(line: &str) -> Result<(u16, String), HttpError> {
    // Expected format: HTTP/x.y STATUS_CODE REASON_PHRASE
    let mut parts = line.splitn(3, ' ');

    let _version = parts.next().ok_or_else(|| {
        HttpError::InvalidResponse("missing HTTP version in status line".to_string())
    })?;

    let status_str = parts.next().ok_or_else(|| {
        HttpError::InvalidResponse("missing status code in status line".to_string())
    })?;

    let status = status_str.parse::<u16>().map_err(|_| {
        HttpError::InvalidResponse(format!("invalid status code: {status_str}"))
    })?;

    let reason = parts.next().unwrap_or("");

    Ok((status, reason.to_string()))
}

/// Parse a single header line like "Content-Type: text/html".
fn parse_header_line(line: &str) -> Option<(&str, &str)> {
    let colon_idx = line.find(':')?;
    let name = line[..colon_idx].trim();
    let value = line[colon_idx + 1..].trim();
    if name.is_empty() {
        return None;
    }
    Some((name, value))
}

/// Decode a chunked transfer-encoded body.
///
/// Chunked format:
/// ```text
/// <hex-size>\r\n
/// <data>\r\n
/// ...
/// 0\r\n
/// \r\n
/// ```
fn decode_chunked(data: &[u8]) -> Result<Vec<u8>, HttpError> {
    let mut result = Vec::new();
    let mut pos = 0;

    loop {
        // Find the end of the chunk size line
        let line_end = find_crlf(data, pos).ok_or_else(|| {
            HttpError::InvalidResponse("malformed chunked encoding: missing chunk size".to_string())
        })?;

        let size_str = core::str::from_utf8(&data[pos..line_end])
            .map_err(|_| HttpError::InvalidResponse("chunk size is not UTF-8".to_string()))?
            .trim();

        // The chunk size may have extensions after a semicolon; ignore them
        let size_hex = size_str.split(';').next().unwrap_or(size_str).trim();

        let chunk_size = usize::from_str_radix(size_hex, 16).map_err(|_| {
            HttpError::InvalidResponse(format!("invalid chunk size: {size_hex}"))
        })?;

        if chunk_size == 0 {
            // Terminal chunk
            break;
        }

        let chunk_start = line_end + 2; // skip \r\n after size line
        let chunk_end = chunk_start + chunk_size;

        if chunk_end > data.len() {
            // Incomplete chunk — take what we have
            result.extend_from_slice(&data[chunk_start..]);
            break;
        }

        result.extend_from_slice(&data[chunk_start..chunk_end]);

        // Skip the trailing \r\n after chunk data
        pos = chunk_end + 2;
        if pos > data.len() {
            break;
        }
    }

    Ok(result)
}

/// Find the position of the next \r\n starting from `start`.
fn find_crlf(data: &[u8], start: usize) -> Option<usize> {
    if start >= data.len() {
        return None;
    }
    let slice = &data[start..];
    slice
        .windows(2)
        .position(|w| w == b"\r\n")
        .map(|p| p + start)
}

// ---------------------------------------------------------------------------
// Cookie handling
// ---------------------------------------------------------------------------

/// An HTTP cookie with associated metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    /// The cookie name.
    pub name: String,
    /// The cookie value.
    pub value: String,
    /// The domain the cookie is valid for.
    pub domain: Option<String>,
    /// The path the cookie is valid for.
    pub path: Option<String>,
    /// Expiration time as a Unix timestamp (seconds since epoch).
    pub expires: Option<u64>,
    /// If true, only send over HTTPS.
    pub secure: bool,
    /// If true, not accessible to client-side scripts.
    pub http_only: bool,
}

/// A collection of cookies that can be matched against request URLs.
#[derive(Debug, Clone)]
pub struct CookieJar {
    cookies: Vec<Cookie>,
}

impl CookieJar {
    /// Create an empty cookie jar.
    pub fn new() -> Self {
        Self {
            cookies: Vec::new(),
        }
    }

    /// Add a cookie to the jar. If a cookie with the same name and domain exists,
    /// it is replaced.
    pub fn add(&mut self, cookie: Cookie) {
        // Remove any existing cookie with same name+domain
        let name = cookie.name.clone();
        let domain = cookie.domain.clone();
        self.cookies.retain(|c| {
            !(c.name == name && c.domain == domain)
        });
        self.cookies.push(cookie);
    }

    /// Get a cookie by name, domain, and path.
    pub fn get(&self, name: &str, domain: &str, path: &str) -> Option<&Cookie> {
        self.cookies.iter().find(|c| {
            c.name == name
                && c.domain.as_deref() == Some(domain)
                && c.path.as_deref().is_none_or(|p| path.starts_with(p))
        })
    }

    /// Get all cookies that should be sent for a given URL.
    ///
    /// Matches based on domain (suffix match) and path (prefix match).
    /// Respects the `secure` flag (only matches HTTPS URLs).
    pub fn matching(&self, url: &Url) -> Vec<&Cookie> {
        self.cookies
            .iter()
            .filter(|c| {
                // Check secure flag
                if c.secure && url.scheme != "https" {
                    return false;
                }

                // Check domain
                if let Some(ref cookie_domain) = c.domain {
                    let host = &url.host;
                    if !domain_matches(host, cookie_domain) {
                        return false;
                    }
                }

                // Check path
                if let Some(ref cookie_path) = c.path
                    && !url.path.starts_with(cookie_path.as_str())
                {
                    return false;
                }

                true
            })
            .collect()
    }

    /// Remove a cookie by name and domain.
    pub fn remove(&mut self, name: &str, domain: &str) {
        self.cookies.retain(|c| {
            !(c.name == name && c.domain.as_deref() == Some(domain))
        });
    }

    /// Remove all cookies from the jar.
    pub fn clear(&mut self) {
        self.cookies.clear();
    }

    /// Parse a `Set-Cookie` header value and return the resulting `Cookie`.
    ///
    /// Uses the request URL to set default domain and path if not specified.
    pub fn parse_set_cookie(header: &str, request_url: &Url) -> Option<Cookie> {
        let mut parts = header.split(';');

        // First part is name=value
        let name_value = parts.next()?.trim();
        let eq_idx = name_value.find('=')?;
        let name = name_value[..eq_idx].trim().to_string();
        let value = name_value[eq_idx + 1..].trim().to_string();

        if name.is_empty() {
            return None;
        }

        let mut cookie = Cookie {
            name,
            value,
            domain: Some(request_url.host.clone()),
            path: Some("/".to_string()),
            expires: None,
            secure: false,
            http_only: false,
        };

        // Parse attributes
        for part in parts {
            let attr = part.trim();
            if attr.is_empty() {
                continue;
            }

            let lower_attr = attr.to_ascii_lowercase();

            if lower_attr == "secure" {
                cookie.secure = true;
            } else if lower_attr == "httponly" {
                cookie.http_only = true;
            } else if let Some(attr_eq) = attr.find('=') {
                let attr_name = attr[..attr_eq].trim().to_ascii_lowercase();
                let attr_value = attr[attr_eq + 1..].trim();

                match attr_name.as_str() {
                    "domain" => {
                        let domain = attr_value.strip_prefix('.').unwrap_or(attr_value);
                        cookie.domain = Some(domain.to_string());
                    }
                    "path" => {
                        cookie.path = Some(attr_value.to_string());
                    }
                    "max-age" => {
                        if let Ok(seconds) = attr_value.parse::<u64>() {
                            // Store as relative seconds for now (no system clock available)
                            cookie.expires = Some(seconds);
                        }
                    }
                    _ => {} // Ignore unknown attributes
                }
            }
        }

        Some(cookie)
    }

    /// Return the number of cookies in the jar.
    pub fn len(&self) -> usize {
        self.cookies.len()
    }

    /// Return true if the jar is empty.
    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }
}

impl Default for CookieJar {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a host matches a cookie domain (suffix matching).
///
/// "example.com" matches "example.com" and ".example.com".
/// "sub.example.com" matches "example.com" and ".example.com".
fn domain_matches(host: &str, cookie_domain: &str) -> bool {
    let normalized = cookie_domain.strip_prefix('.').unwrap_or(cookie_domain);
    if host == normalized {
        return true;
    }
    // Suffix match: host ends with ".domain"
    if host.ends_with(&format!(".{normalized}")) {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// URL encoding / decoding
// ---------------------------------------------------------------------------

/// Percent-encode a string for use in a URL.
///
/// Encodes all characters except unreserved characters (A-Z, a-z, 0-9, `-`, `_`, `.`, `~`).
pub fn percent_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.bytes() {
        if is_unreserved(byte) {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(hex_char_upper(byte >> 4));
            encoded.push(hex_char_upper(byte & 0x0F));
        }
    }
    encoded
}

/// Decode a percent-encoded string.
///
/// Converts `%XX` sequences back to the original byte values.
///
/// # Errors
///
/// Returns `HttpError::InvalidUrl` if a percent-encoded sequence is malformed.
pub fn percent_decode(input: &str) -> Result<String, HttpError> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(HttpError::InvalidUrl(
                    "incomplete percent-encoding".to_string(),
                ));
            }
            let hi = hex_digit_value(bytes[i + 1]).ok_or_else(|| {
                HttpError::InvalidUrl("invalid hex digit in percent-encoding".to_string())
            })?;
            let lo = hex_digit_value(bytes[i + 2]).ok_or_else(|| {
                HttpError::InvalidUrl("invalid hex digit in percent-encoding".to_string())
            })?;
            decoded.push((hi << 4) | lo);
            i += 3;
        } else if bytes[i] == b'+' {
            // In query strings, + means space
            decoded.push(b' ');
            i += 1;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }

    String::from_utf8(decoded).map_err(|_| HttpError::EncodingError)
}

/// Check if a byte is an unreserved URI character.
fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~'
}

/// Convert a nibble (0-15) to an uppercase hex character.
fn hex_char_upper(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + nibble - 10) as char,
        _ => '0', // Should never happen with valid nibbles
    }
}

/// Parse a hex digit character to its numeric value.
fn hex_digit_value(ch: u8) -> Option<u8> {
    match ch {
        b'0'..=b'9' => Some(ch - b'0'),
        b'a'..=b'f' => Some(ch - b'a' + 10),
        b'A'..=b'F' => Some(ch - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Base64 encoding
// ---------------------------------------------------------------------------

/// The standard base64 alphabet.
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode binary data to a base64 string.
///
/// Uses the standard base64 alphabet with `=` padding.
pub fn base64_encode(data: &[u8]) -> String {
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;

    while i + 2 < data.len() {
        let triple = (u32::from(data[i]) << 16)
            | (u32::from(data[i + 1]) << 8)
            | u32::from(data[i + 2]);

        result.push(BASE64_ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        result.push(BASE64_ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
        result.push(BASE64_ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
        result.push(BASE64_ALPHABET[(triple & 0x3F) as usize] as char);

        i += 3;
    }

    // Handle remaining bytes
    let remaining = data.len() - i;
    match remaining {
        1 => {
            let val = u32::from(data[i]) << 16;
            result.push(BASE64_ALPHABET[((val >> 18) & 0x3F) as usize] as char);
            result.push(BASE64_ALPHABET[((val >> 12) & 0x3F) as usize] as char);
            result.push('=');
            result.push('=');
        }
        2 => {
            let val = (u32::from(data[i]) << 16) | (u32::from(data[i + 1]) << 8);
            result.push(BASE64_ALPHABET[((val >> 18) & 0x3F) as usize] as char);
            result.push(BASE64_ALPHABET[((val >> 12) & 0x3F) as usize] as char);
            result.push(BASE64_ALPHABET[((val >> 6) & 0x3F) as usize] as char);
            result.push('=');
        }
        _ => {}
    }

    result
}

// ---------------------------------------------------------------------------
// Content-Type parsing
// ---------------------------------------------------------------------------

/// Parse a Content-Type header into the MIME type and optional charset parameter.
///
/// For example, `"text/html; charset=utf-8"` returns `("text/html", Some("utf-8"))`.
pub fn parse_content_type(header: &str) -> (String, Option<String>) {
    let mut parts = header.splitn(2, ';');
    let mime_type = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let charset = parts.next().and_then(|params| {
        params
            .split(';')
            .find_map(|param| {
                let trimmed = param.trim();
                let lower = trimmed.to_ascii_lowercase();
                if lower.starts_with("charset=") {
                    Some(
                        trimmed[8..]
                            .trim()
                            .trim_matches('"')
                            .to_ascii_lowercase(),
                    )
                } else {
                    None
                }
            })
    });

    (mime_type, charset)
}

// ---------------------------------------------------------------------------
// Client struct (convenience wrapper)
// ---------------------------------------------------------------------------

/// A reusable HTTP client with shared configuration and cookie storage.
///
/// The `Client` holds default headers, timeout settings, and a cookie jar that
/// persists across requests.
#[derive(Debug, Clone)]
pub struct Client {
    /// Default headers sent with every request.
    pub default_headers: Headers,
    /// Default timeout in milliseconds.
    pub timeout_ms: u32,
    /// Whether to follow redirects by default.
    pub follow_redirects: bool,
    /// Maximum number of redirects to follow.
    pub max_redirects: u32,
    /// Persistent cookie storage.
    pub cookie_jar: CookieJar,
    /// User-Agent string.
    pub user_agent: String,
}

impl Client {
    /// Create a new client with default settings.
    pub fn new() -> Self {
        Self {
            default_headers: Headers::new(),
            timeout_ms: 30_000,
            follow_redirects: true,
            max_redirects: 10,
            cookie_jar: CookieJar::new(),
            user_agent: "SlateOS-HttpClient/0.1".to_string(),
        }
    }

    /// Create a GET request builder using this client's defaults.
    ///
    /// # Errors
    ///
    /// Returns `HttpError::InvalidUrl` if the URL cannot be parsed.
    pub fn get(&self, url: &str) -> Result<RequestBuilder, HttpError> {
        let mut builder = RequestBuilder::get(url)?;
        builder = self.apply_defaults(builder);
        Ok(builder)
    }

    /// Create a POST request builder using this client's defaults.
    ///
    /// # Errors
    ///
    /// Returns `HttpError::InvalidUrl` if the URL cannot be parsed.
    pub fn post(&self, url: &str) -> Result<RequestBuilder, HttpError> {
        let mut builder = RequestBuilder::post(url)?;
        builder = self.apply_defaults(builder);
        Ok(builder)
    }

    /// Create a PUT request builder using this client's defaults.
    ///
    /// # Errors
    ///
    /// Returns `HttpError::InvalidUrl` if the URL cannot be parsed.
    pub fn put(&self, url: &str) -> Result<RequestBuilder, HttpError> {
        let mut builder = RequestBuilder::put(url)?;
        builder = self.apply_defaults(builder);
        Ok(builder)
    }

    /// Create a DELETE request builder using this client's defaults.
    ///
    /// # Errors
    ///
    /// Returns `HttpError::InvalidUrl` if the URL cannot be parsed.
    pub fn delete(&self, url: &str) -> Result<RequestBuilder, HttpError> {
        let mut builder = RequestBuilder::delete(url)?;
        builder = self.apply_defaults(builder);
        Ok(builder)
    }

    /// Apply client defaults to a request builder.
    fn apply_defaults(&self, mut builder: RequestBuilder) -> RequestBuilder {
        builder.timeout_ms = self.timeout_ms;
        builder.follow_redirects = self.follow_redirects;
        builder.max_redirects = self.max_redirects;
        builder = builder.user_agent(&self.user_agent);
        // Apply default headers
        for (name, value) in self.default_headers.iter() {
            if !builder.headers.contains(name) {
                builder.headers.set(name, value);
            }
        }
        builder
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- URL parsing tests ---

    #[test]
    fn test_url_parse_simple_http() {
        let url = Url::parse("http://example.com/path").unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 80);
        assert_eq!(url.path, "/path");
        assert_eq!(url.query, None);
        assert_eq!(url.fragment, None);
    }

    #[test]
    fn test_url_parse_https_with_port() {
        let url = Url::parse("https://secure.example.com:8443/api/v1").unwrap();
        assert_eq!(url.scheme, "https");
        assert_eq!(url.host, "secure.example.com");
        assert_eq!(url.port, 8443);
        assert_eq!(url.path, "/api/v1");
    }

    #[test]
    fn test_url_parse_with_query_and_fragment() {
        let url = Url::parse("http://example.com/search?q=hello&lang=en#results").unwrap();
        assert_eq!(url.path, "/search");
        assert_eq!(url.query, Some("q=hello&lang=en".to_string()));
        assert_eq!(url.fragment, Some("results".to_string()));
    }

    #[test]
    fn test_url_parse_no_path() {
        let url = Url::parse("http://example.com").unwrap();
        assert_eq!(url.path, "/");
    }

    #[test]
    fn test_url_parse_invalid_no_scheme() {
        let result = Url::parse("example.com/path");
        assert!(result.is_err());
    }

    #[test]
    fn test_url_parse_empty_host() {
        let result = Url::parse("http:///path");
        assert!(result.is_err());
    }

    #[test]
    fn test_url_to_string_default_port() {
        let url = Url::parse("http://example.com/path").unwrap();
        assert_eq!(url.to_string(), "http://example.com/path");
    }

    #[test]
    fn test_url_to_string_custom_port() {
        let url = Url::parse("http://example.com:9090/path").unwrap();
        assert_eq!(url.to_string(), "http://example.com:9090/path");
    }

    #[test]
    fn test_url_request_path_with_query() {
        let url = Url::parse("http://example.com/api?key=val").unwrap();
        assert_eq!(url.request_path(), "/api?key=val");
    }

    // --- Header tests ---

    #[test]
    fn test_headers_case_insensitive_get() {
        let mut headers = Headers::new();
        headers.set("Content-Type", "text/html");
        assert_eq!(headers.get("content-type"), Some("text/html"));
        assert_eq!(headers.get("CONTENT-TYPE"), Some("text/html"));
    }

    #[test]
    fn test_headers_set_replaces() {
        let mut headers = Headers::new();
        headers.set("Accept", "text/plain");
        headers.set("Accept", "application/json");
        assert_eq!(headers.get("Accept"), Some("application/json"));
        assert_eq!(headers.len(), 1);
    }

    #[test]
    fn test_headers_remove() {
        let mut headers = Headers::new();
        headers.set("X-Custom", "value");
        headers.remove("x-custom");
        assert!(!headers.contains("X-Custom"));
    }

    #[test]
    fn test_headers_get_all() {
        let mut headers = Headers::new();
        headers.append("Set-Cookie", "a=1");
        headers.append("Set-Cookie", "b=2");
        let values = headers.get_all("set-cookie");
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], "a=1");
        assert_eq!(values[1], "b=2");
    }

    // --- Request serialization tests ---

    #[test]
    fn test_request_serialize_get() {
        let req = RequestBuilder::get("http://example.com/path")
            .unwrap()
            .header("Accept", "text/html")
            .build();
        let bytes = req.serialize();
        let text = core::str::from_utf8(&bytes).unwrap();
        assert!(text.starts_with("GET /path HTTP/1.1\r\n"));
        assert!(text.contains("Host: example.com\r\n"));
        assert!(text.contains("Accept: text/html\r\n"));
        assert!(text.ends_with("\r\n\r\n"));
    }

    #[test]
    fn test_request_serialize_post_with_body() {
        let req = RequestBuilder::post("http://api.example.com/data")
            .unwrap()
            .body(b"hello=world".to_vec())
            .build();
        let bytes = req.serialize();
        let text = core::str::from_utf8(&bytes).unwrap();
        assert!(text.starts_with("POST /data HTTP/1.1\r\n"));
        assert!(text.contains("Content-Length: 11\r\n"));
        assert!(text.ends_with("hello=world"));
    }

    #[test]
    fn test_request_serialize_custom_port_in_host() {
        let req = RequestBuilder::get("http://example.com:9090/path")
            .unwrap()
            .build();
        let bytes = req.serialize();
        let text = core::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("Host: example.com:9090\r\n"));
    }

    // --- Response parsing tests ---

    #[test]
    fn test_response_parse_simple() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 5\r\n\r\nhello";
        let url = Url::parse("http://example.com/").unwrap();
        let resp = parse_response(raw, &url).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.status_text, "OK");
        assert_eq!(resp.header("Content-Type"), Some("text/html"));
        assert_eq!(resp.body, b"hello");
    }

    #[test]
    fn test_response_parse_chunked() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let url = Url::parse("http://example.com/").unwrap();
        let resp = parse_response(raw, &url).unwrap();
        assert_eq!(resp.body, b"hello world");
    }

    #[test]
    fn test_response_status_categories() {
        let url = Url::parse("http://example.com/").unwrap();
        let resp = Response {
            status: 301,
            status_text: "Moved Permanently".to_string(),
            headers: Headers::new(),
            body: Vec::new(),
            url: url.clone(),
        };
        assert!(resp.is_redirect());
        assert!(!resp.is_success());

        let resp404 = Response {
            status: 404,
            status_text: "Not Found".to_string(),
            headers: Headers::new(),
            body: Vec::new(),
            url,
        };
        assert!(resp404.is_client_error());
    }

    #[test]
    fn test_response_text() {
        let url = Url::parse("http://example.com/").unwrap();
        let resp = Response {
            status: 200,
            status_text: "OK".to_string(),
            headers: Headers::new(),
            body: b"Hello, world!".to_vec(),
            url,
        };
        assert_eq!(resp.text().unwrap(), "Hello, world!");
    }

    // --- Cookie tests ---

    #[test]
    fn test_cookie_jar_add_and_get() {
        let mut jar = CookieJar::new();
        jar.add(Cookie {
            name: "session".to_string(),
            value: "abc123".to_string(),
            domain: Some("example.com".to_string()),
            path: Some("/".to_string()),
            expires: None,
            secure: false,
            http_only: false,
        });
        let cookie = jar.get("session", "example.com", "/page").unwrap();
        assert_eq!(cookie.value, "abc123");
    }

    #[test]
    fn test_cookie_jar_matching() {
        let mut jar = CookieJar::new();
        jar.add(Cookie {
            name: "token".to_string(),
            value: "xyz".to_string(),
            domain: Some("api.example.com".to_string()),
            path: Some("/v1".to_string()),
            expires: None,
            secure: false,
            http_only: false,
        });

        let url_match = Url::parse("http://api.example.com/v1/users").unwrap();
        let url_no_match = Url::parse("http://other.com/v1/users").unwrap();

        assert_eq!(jar.matching(&url_match).len(), 1);
        assert_eq!(jar.matching(&url_no_match).len(), 0);
    }

    #[test]
    fn test_cookie_parse_set_cookie() {
        let url = Url::parse("http://example.com/path").unwrap();
        let cookie = CookieJar::parse_set_cookie(
            "session=abc; Path=/; HttpOnly; Secure",
            &url,
        )
        .unwrap();
        assert_eq!(cookie.name, "session");
        assert_eq!(cookie.value, "abc");
        assert_eq!(cookie.path, Some("/".to_string()));
        assert!(cookie.http_only);
        assert!(cookie.secure);
    }

    #[test]
    fn test_cookie_secure_not_sent_over_http() {
        let mut jar = CookieJar::new();
        jar.add(Cookie {
            name: "secure_tok".to_string(),
            value: "secret".to_string(),
            domain: Some("example.com".to_string()),
            path: Some("/".to_string()),
            expires: None,
            secure: true,
            http_only: false,
        });

        let plain_url = Url::parse("http://example.com/page").unwrap();
        let tls_url = Url::parse("https://example.com/page").unwrap();

        assert_eq!(jar.matching(&plain_url).len(), 0);
        assert_eq!(jar.matching(&tls_url).len(), 1);
    }

    // --- Percent encoding tests ---

    #[test]
    fn test_percent_encode_basic() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
        assert_eq!(percent_encode("a+b=c&d"), "a%2Bb%3Dc%26d");
    }

    #[test]
    fn test_percent_encode_unreserved_unchanged() {
        assert_eq!(percent_encode("abcXYZ-_.~"), "abcXYZ-_.~");
    }

    #[test]
    fn test_percent_decode_basic() {
        assert_eq!(percent_decode("hello%20world").unwrap(), "hello world");
        assert_eq!(percent_decode("a%2Bb%3Dc").unwrap(), "a+b=c");
    }

    #[test]
    fn test_percent_decode_plus_as_space() {
        assert_eq!(percent_decode("hello+world").unwrap(), "hello world");
    }

    #[test]
    fn test_percent_decode_invalid() {
        assert!(percent_decode("hello%GG").is_err());
        assert!(percent_decode("incomplete%2").is_err());
    }

    // --- Base64 tests ---

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"Hello, World!"), "SGVsbG8sIFdvcmxkIQ==");
    }

    #[test]
    fn test_base64_encode_binary() {
        assert_eq!(base64_encode(&[0, 1, 2, 3]), "AAECAw==");
        assert_eq!(base64_encode(&[255, 254, 253]), "//79");
    }

    // --- Builder pattern tests ---

    #[test]
    fn test_builder_json() {
        let req = RequestBuilder::post("http://api.example.com/data")
            .unwrap()
            .json("{\"key\":\"value\"}")
            .build();
        assert_eq!(req.headers.get("Content-Type"), Some("application/json"));
        assert_eq!(req.body.as_deref(), Some(b"{\"key\":\"value\"}" as &[u8]));
    }

    #[test]
    fn test_builder_basic_auth() {
        let req = RequestBuilder::get("http://example.com/")
            .unwrap()
            .basic_auth("user", "pass")
            .build();
        // "user:pass" in base64 = "dXNlcjpwYXNz"
        assert_eq!(
            req.headers.get("Authorization"),
            Some("Basic dXNlcjpwYXNz")
        );
    }

    #[test]
    fn test_builder_bearer_token() {
        let req = RequestBuilder::get("http://example.com/")
            .unwrap()
            .bearer_token("my_token_123")
            .build();
        assert_eq!(
            req.headers.get("Authorization"),
            Some("Bearer my_token_123")
        );
    }

    #[test]
    fn test_builder_timeout_and_redirects() {
        let req = RequestBuilder::get("http://example.com/")
            .unwrap()
            .timeout(5000)
            .follow_redirects(false)
            .build();
        assert_eq!(req.timeout_ms, 5000);
        assert!(!req.follow_redirects);
    }

    // --- Content-Type parsing ---

    #[test]
    fn test_parse_content_type_simple() {
        let (mime, charset) = parse_content_type("text/html");
        assert_eq!(mime, "text/html");
        assert_eq!(charset, None);
    }

    #[test]
    fn test_parse_content_type_with_charset() {
        let (mime, charset) = parse_content_type("text/html; charset=utf-8");
        assert_eq!(mime, "text/html");
        assert_eq!(charset, Some("utf-8".to_string()));
    }

    #[test]
    fn test_parse_content_type_quoted_charset() {
        let (mime, charset) = parse_content_type("text/html; charset=\"UTF-8\"");
        assert_eq!(mime, "text/html");
        assert_eq!(charset, Some("utf-8".to_string()));
    }

    // --- Client defaults test ---

    #[test]
    fn test_client_applies_defaults() {
        let mut client = Client::new();
        client.timeout_ms = 5000;
        client.user_agent = "TestAgent/1.0".to_string();
        client.default_headers.set("X-Custom", "default-val");

        let builder = client.get("http://example.com/").unwrap();
        let req = builder.build();

        assert_eq!(req.timeout_ms, 5000);
        assert_eq!(req.headers.get("User-Agent"), Some("TestAgent/1.0"));
        assert_eq!(req.headers.get("X-Custom"), Some("default-val"));
    }

    // --- Chunked decoding edge case ---

    #[test]
    fn test_chunked_decode_with_extensions() {
        // Some servers send chunk extensions like "5;ext=val\r\n"
        let chunked_data = b"5;ext=val\r\nhello\r\n0\r\n\r\n";
        let result = decode_chunked(chunked_data).unwrap();
        assert_eq!(result, b"hello");
    }

    #[test]
    fn test_chunked_decode_empty() {
        let chunked_data = b"0\r\n\r\n";
        let result = decode_chunked(chunked_data).unwrap();
        assert!(result.is_empty());
    }
}
