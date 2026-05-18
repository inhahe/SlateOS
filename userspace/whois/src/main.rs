//! OurOS WHOIS Lookup Utility
//!
//! Queries WHOIS servers for registration and contact information about domain
//! names, IPv4/IPv6 addresses, and autonomous system numbers (ASNs). Built for
//! OurOS using the kernel's TCP and DNS syscall interface.
//!
//! # Usage
//!
//! ```text
//! whois <query>                   Look up a domain, IP, or ASN
//! whois -h <server> <query>       Use a specific WHOIS server
//! whois -p <port> <query>         Use a non-default port (default: 43)
//! whois --no-referral <query>     Do not follow referral responses
//! whois -v <query>                Verbose: show server being queried
//! whois <query1> <query2> ...     Multiple queries in one invocation
//! ```

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::manual_range_contains)] // explicit comparisons are clearer
#![allow(clippy::missing_errors_doc)] // internal helpers
#![allow(clippy::missing_panics_doc)] // no panics in prod code
// The indexing_slicing, unwrap_used, expect_used, arithmetic_side_effects, and
// panic lints are enabled as warnings so they alert without blocking `cargo test`
// (test code legitimately uses indexing and unwrap).
#![warn(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::env;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Syscall numbers
// ============================================================================

const SYS_TCP_CONNECT: u64 = 800;
const SYS_TCP_SEND: u64 = 802;
const SYS_TCP_RECV: u64 = 803;
const SYS_TCP_CLOSE: u64 = 804;
const SYS_DNS_RESOLVE: u64 = 820;

// ============================================================================
// Low-level syscall stubs
// ============================================================================

/// Perform a 3-argument syscall via the `syscall` instruction.
///
/// # Safety
/// The caller must guarantee that `nr` is a valid syscall number and that
/// `a1`/`a2`/`a3` are valid arguments for that syscall (valid pointers, lengths,
/// handles, etc.). The `syscall` instruction clobbers `rcx` and `r11` per the
/// System V AMD64 ABI; both are marked as clobbered here.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller upholds the contract above. rcx and r11 are clobbered per
    // the x86-64 syscall ABI and are declared as such.
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

/// Perform a 4-argument syscall (used by `SYS_TCP_CONNECT` to pass a timeout).
///
/// # Safety
/// Same contract as `syscall3`; the fourth argument goes in `r10` per the
/// System V AMD64 ABI (since `rcx` is clobbered by the `syscall` instruction).
#[cfg(target_arch = "x86_64")]
unsafe fn syscall4(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller upholds the same contract as syscall3. The fourth argument
    // is passed in r10 because rcx is destroyed by `syscall`.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Single-argument syscall (used for `SYS_TCP_CLOSE`).
///
/// # Safety
/// Caller must supply a valid handle obtained from a prior `SYS_TCP_CONNECT`.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall1(nr: u64, a1: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller upholds the handle-validity contract. rcx/r11 clobbered.
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
// Kernel network wrappers
// ============================================================================

/// Resolve `hostname` to an IPv4 address via the kernel DNS resolver.
///
/// Returns the address as a `u32` in **network byte order** on success.
fn dns_resolve(hostname: &str) -> Result<u32, WhoisError> {
    let mut result_ip: u32 = 0;
    // SAFETY: We pass a valid UTF-8 byte slice pointer and its length, plus a
    // mutable pointer to a u32 for the kernel to write the resolved address into.
    // The kernel reads exactly `hostname.len()` bytes and writes exactly 4 bytes.
    let ret = unsafe {
        syscall3(
            SYS_DNS_RESOLVE,
            hostname.as_ptr() as u64,
            hostname.len() as u64,
            core::ptr::addr_of_mut!(result_ip) as u64,
        )
    };
    if ret < 0 {
        return Err(WhoisError::DnsFailure(hostname.to_string()));
    }
    Ok(result_ip)
}

/// Open a TCP connection to `ip` (network byte order) on `port`.
///
/// `timeout_ms` is passed to the kernel; 0 means "use the kernel default."
/// Returns an opaque connection handle on success.
fn tcp_connect(ip: u32, port: u16, timeout_ms: u32) -> Result<u64, WhoisError> {
    // SAFETY: ip is a valid network-order IPv4 address, port is valid, and
    // timeout_ms is an advisory hint that the kernel may clamp. No pointers.
    let ret = unsafe {
        syscall4(
            SYS_TCP_CONNECT,
            u64::from(ip),
            u64::from(port),
            u64::from(timeout_ms),
            0,
        )
    };
    if ret < 0 {
        return Err(WhoisError::ConnectionFailed(format!(
            "kernel error {ret}"
        )));
    }
    Ok(ret as u64)
}

/// Send `data` on the connection identified by `handle`.
///
/// Returns the number of bytes actually sent (may be less than `data.len()`).
fn tcp_send(handle: u64, data: &[u8]) -> Result<usize, WhoisError> {
    // SAFETY: handle is a valid TCP connection handle. We pass a pointer to
    // the byte slice and its length; the kernel reads at most `data.len()` bytes.
    let ret = unsafe {
        syscall3(
            SYS_TCP_SEND,
            handle,
            data.as_ptr() as u64,
            data.len() as u64,
        )
    };
    if ret < 0 {
        return Err(WhoisError::SendFailed);
    }
    Ok(ret as usize)
}

/// Send all of `data`, looping until the entire buffer has been transmitted.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), WhoisError> {
    let mut offset = 0usize;
    while offset < data.len() {
        let n = tcp_send(handle, data.get(offset..).unwrap_or(&[]))?;
        if n == 0 {
            return Err(WhoisError::SendFailed);
        }
        offset = offset.checked_add(n).ok_or(WhoisError::SendFailed)?;
    }
    Ok(())
}

/// Receive up to `buf.len()` bytes from `handle`.
///
/// Returns 0 when the remote peer has closed the connection.
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, WhoisError> {
    // SAFETY: handle is a valid connection handle. We pass a mutable pointer to
    // the buffer and its capacity; the kernel writes at most `buf.len()` bytes.
    let ret = unsafe {
        syscall3(
            SYS_TCP_RECV,
            handle,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(WhoisError::RecvFailed);
    }
    Ok(ret as usize)
}

/// Close the connection handle and release kernel resources.
///
/// Errors from close are intentionally ignored: the handle is invalid after
/// this call regardless, and there is no useful recovery action.
fn tcp_close(handle: u64) {
    // SAFETY: handle was obtained from tcp_connect and has not been closed yet.
    // The return value is discarded because close is unconditionally best-effort.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
enum WhoisError {
    DnsFailure(String),
    ConnectionFailed(String),
    SendFailed,
    RecvFailed,
    InvalidArgument(String),
}

impl std::fmt::Display for WhoisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DnsFailure(host) => write!(f, "could not resolve host: {host}"),
            Self::ConnectionFailed(msg) => write!(f, "connection failed: {msg}"),
            Self::SendFailed => write!(f, "send failed"),
            Self::RecvFailed => write!(f, "recv failed"),
            Self::InvalidArgument(msg) => write!(f, "invalid argument: {msg}"),
        }
    }
}

// ============================================================================
// Query type detection
// ============================================================================

/// The kind of WHOIS query being made.
#[derive(Debug, Clone, PartialEq, Eq)]
enum QueryKind {
    /// A domain name (e.g. `example.com`).
    Domain,
    /// An IPv4 address (e.g. `1.2.3.4`).
    Ipv4,
    /// An IPv6 address (e.g. `2001:db8::1`).
    Ipv6,
    /// An autonomous system number (e.g. `AS15169` or `15169`).
    Asn,
}

/// Detect whether `query` is a domain, IPv4, IPv6, or ASN.
fn detect_query_kind(query: &str) -> QueryKind {
    // ASN: starts with "AS" (case-insensitive) followed by digits,
    // or is a plain all-digit string between 1 and 10 digits (ASN range).
    let upper = query.to_ascii_uppercase();
    if upper.starts_with("AS") {
        let suffix = &upper[2..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
            return QueryKind::Asn;
        }
    }
    // Pure digits that could plausibly be a bare ASN (1–10 decimal digits,
    // value ≤ 4294967295).  We cap at 10 to avoid misclassifying long numbers.
    if !query.is_empty()
        && query.len() <= 10
        && query.chars().all(|c| c.is_ascii_digit())
    {
        return QueryKind::Asn;
    }

    // IPv4: parseable as Ipv4Addr.
    if query.parse::<std::net::Ipv4Addr>().is_ok() {
        return QueryKind::Ipv4;
    }

    // IPv6: contains ':' and parseable as Ipv6Addr (strip optional brackets).
    let unbracketed = query
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(query);
    if unbracketed.contains(':') && unbracketed.parse::<std::net::Ipv6Addr>().is_ok() {
        return QueryKind::Ipv6;
    }

    QueryKind::Domain
}

// ============================================================================
// Built-in WHOIS server database
// ============================================================================

/// Default port for the WHOIS protocol (RFC 3912).
const DEFAULT_WHOIS_PORT: u16 = 43;

/// Return the canonical WHOIS server for `query`.
///
/// The selection priority is:
/// 1. IP addresses and ASNs always start at `whois.arin.net`.
/// 2. Known TLD-specific servers from the built-in table.
/// 3. Fall back to `whois.iana.org` for everything else.
fn whois_server_for(query: &str, kind: &QueryKind) -> &'static str {
    match kind {
        QueryKind::Ipv4 | QueryKind::Ipv6 | QueryKind::Asn => "whois.arin.net",
        QueryKind::Domain => server_for_domain(query),
    }
}

/// Look up the WHOIS server for a domain based on its TLD.
fn server_for_domain(domain: &str) -> &'static str {
    // Extract the rightmost label (the TLD).
    let tld = domain
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

    // Two-level check: first try <sld>.<tld> for commonly delegated SLDs.
    // We do this by extracting the last two labels.
    let two_labels: Option<String> = {
        let mut parts = domain.rsplitn(3, '.');
        let last = parts.next().map(str::to_ascii_lowercase);
        let second = parts.next().map(str::to_ascii_lowercase);
        match (second, last) {
            (Some(s), Some(l)) => Some(format!("{s}.{l}")),
            _ => None,
        }
    };

    if let Some(ref two) = two_labels {
        if let Some(srv) = lookup_two_label_tld(two) {
            return srv;
        }
    }

    lookup_tld(&tld)
}

/// Look up a WHOIS server for a two-label TLD (e.g. `co.uk`).
fn lookup_two_label_tld(two: &str) -> Option<&'static str> {
    // Only a handful of two-label TLDs have their own WHOIS server.
    let srv = match two {
        "co.uk" | "org.uk" | "me.uk" | "net.uk" => "whois.nic.uk",
        "com.au" | "net.au" | "org.au" | "edu.au" => "whois.auda.org.au",
        _ => return None,
    };
    Some(srv)
}

/// Look up a WHOIS server for a single-label TLD.
fn lookup_tld(tld: &str) -> &'static str {
    match tld {
        "com" | "net" => "whois.verisign-grs.com",
        "org" => "whois.pir.org",
        "io" => "whois.nic.io",
        "dev" => "whois.nic.google",
        "app" => "whois.nic.google",
        "page" => "whois.nic.google",
        "info" => "whois.afilias.net",
        "us" => "whois.nic.us",
        "uk" => "whois.nic.uk",
        "de" => "whois.denic.de",
        "fr" => "whois.nic.fr",
        "au" => "whois.auda.org.au",
        "ca" => "whois.cira.ca",
        "nl" => "whois.domain-registry.nl",
        "br" => "whois.registro.br",
        "jp" => "whois.jprs.jp",
        "cn" => "whois.cnnic.cn",
        "ru" => "whois.tcinet.ru",
        "it" => "whois.nic.it",
        "es" => "whois.nic.es",
        "pl" => "whois.dns.pl",
        "ch" => "whois.nic.ch",
        "se" => "whois.iis.se",
        "no" => "whois.norid.no",
        "dk" => "whois.dk-hostmaster.dk",
        "fi" => "whois.fi",
        "at" => "whois.nic.at",
        "be" => "whois.dns.be",
        "nz" => "whois.srs.net.nz",
        "mx" => "whois.mx",
        "in" => "whois.inregistry.net",
        "co" => "whois.nic.co",
        "tv" => "whois.nic.tv",
        "cc" => "whois.nic.cc",
        "biz" => "whois.biz",
        "mobi" => "whois.afilias.net",
        "name" => "whois.nic.name",
        "pro" => "whois.afilias.net",
        "museum" => "whois.museum",
        "travel" => "whois.nic.travel",
        "edu" => "whois.educause.edu",
        "gov" => "whois.dotgov.gov",
        "mil" => "whois.nic.mil",
        "int" => "whois.iana.org",
        "arpa" => "whois.iana.org",
        _ => "whois.iana.org",
    }
}

// ============================================================================
// Referral parsing
// ============================================================================

/// Parse a referral server from a raw WHOIS response.
///
/// Looks for `ReferralServer:` (ARIN-style) or `refer:` (IANA-style) lines
/// and extracts the hostname, stripping any `whois://` URI prefix.
///
/// Returns `None` if no referral directive is found or the directive is empty.
fn parse_referral(response: &str) -> Option<String> {
    for line in response.lines() {
        let trimmed = line.trim();

        // ARIN / RIPE style: "ReferralServer: whois://whois.ripe.net"
        // or "ReferralServer: whois.ripe.net"
        if let Some(val) = trimmed
            .strip_prefix("ReferralServer:")
            .or_else(|| trimmed.strip_prefix("referralserver:"))
        {
            let server = extract_host_from_referral(val.trim());
            if !server.is_empty() {
                return Some(server.to_string());
            }
        }

        // IANA style: "refer: whois.verisign-grs.com"
        if let Some(val) = trimmed
            .strip_prefix("refer:")
            .or_else(|| trimmed.strip_prefix("Refer:"))
        {
            let server = extract_host_from_referral(val.trim());
            if !server.is_empty() {
                return Some(server.to_string());
            }
        }
    }
    None
}

/// Strip a `whois://` (or `rwhois://`) URI prefix and optional `:<port>` suffix,
/// returning just the hostname portion.
fn extract_host_from_referral(s: &str) -> &str {
    // Strip known URI schemes.
    let s = s
        .strip_prefix("whois://")
        .or_else(|| s.strip_prefix("rwhois://"))
        .unwrap_or(s);

    // Strip trailing port number.
    if let Some(colon) = s.rfind(':') {
        let port_part = &s[colon.checked_add(1).unwrap_or(colon)..];
        if !port_part.is_empty() && port_part.chars().all(|c| c.is_ascii_digit()) {
            return &s[..colon];
        }
    }
    s
}

// ============================================================================
// Core WHOIS query execution
// ============================================================================

/// Perform a single WHOIS query against `server`:`port` for `query`.
///
/// The WHOIS protocol (RFC 3912) is minimal:
///   1. Open a TCP connection.
///   2. Send `<query>\r\n`.
///   3. Read all data until the server closes the connection.
///
/// Returns the raw response text.
fn whois_query(server: &str, port: u16, query: &str) -> Result<String, WhoisError> {
    // Resolve the WHOIS server hostname.
    let ip = dns_resolve(server)?;

    // Connect with a 10-second timeout.
    let handle = tcp_connect(ip, port, 10_000)?;

    // Build and send the request: "<query>\r\n"
    let mut request = String::with_capacity(query.len().saturating_add(2));
    request.push_str(query);
    request.push_str("\r\n");

    let send_result = tcp_send_all(handle, request.as_bytes());
    if let Err(e) = send_result {
        tcp_close(handle);
        return Err(e);
    }

    // Read the full response.
    let mut response = Vec::with_capacity(4096);
    let mut buf = [0u8; 4096];

    loop {
        match tcp_recv(handle, &mut buf) {
            Ok(0) => break, // Server closed the connection.
            Ok(n) => {
                let chunk = buf.get(..n).unwrap_or(&buf);
                response.extend_from_slice(chunk);
                // Guard against pathologically large responses (8 MiB).
                if response.len() > 8 * 1024 * 1024 {
                    break;
                }
            }
            Err(e) => {
                tcp_close(handle);
                return Err(e);
            }
        }
    }

    tcp_close(handle);

    // Convert to string, replacing invalid UTF-8 sequences with the replacement
    // character.  WHOIS responses are nominally ASCII/Latin-1 but may contain
    // stray bytes.
    Ok(String::from_utf8_lossy(&response).into_owned())
}

// ============================================================================
// High-level lookup with referral following
// ============================================================================

/// Options controlling a single WHOIS lookup session.
struct LookupOptions<'a> {
    /// Override the WHOIS server instead of using the built-in database.
    server_override: Option<&'a str>,
    /// TCP port to connect to (default: 43).
    port: u16,
    /// If `true`, follow `ReferralServer:` / `refer:` directives.
    follow_referral: bool,
    /// If `true`, print the server name before querying it.
    verbose: bool,
}

/// Perform a full WHOIS lookup for `query`, following referrals as configured.
///
/// Returns a `Vec` of `(server_name, response_text)` pairs — one entry per
/// query attempt (initial + any followed referrals).
fn lookup(query: &str, opts: &LookupOptions<'_>) -> Result<Vec<(String, String)>, WhoisError> {
    let kind = detect_query_kind(query);

    // Choose the initial server.
    let initial_server: String = if let Some(srv) = opts.server_override {
        srv.to_string()
    } else {
        whois_server_for(query, &kind).to_string()
    };

    let mut results: Vec<(String, String)> = Vec::new();
    let mut current_server = initial_server;
    // Referral depth limit to prevent infinite loops.
    let max_referrals: usize = 8;
    let mut depth = 0usize;

    loop {
        if opts.verbose {
            eprintln!("[whois] querying {current_server}:{}", opts.port);
        }

        let response = whois_query(&current_server, opts.port, query)?;
        let server_name = current_server.clone();
        results.push((server_name, response.clone()));

        // Follow referral if enabled and we haven't reached the depth limit.
        if opts.follow_referral && depth < max_referrals {
            if let Some(referral) = parse_referral(&response) {
                // Do not re-query the same server (avoids trivial loops).
                if referral != current_server {
                    current_server = referral;
                    depth = depth.saturating_add(1);
                    continue;
                }
            }
        }

        break;
    }

    Ok(results)
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Parsed command-line arguments.
struct Args {
    /// Queries to look up (domains, IPs, ASNs).
    queries: Vec<String>,
    /// Optional WHOIS server override (`-h <server>`).
    server: Option<String>,
    /// TCP port override (`-p <port>`).
    port: u16,
    /// If `true`, do not follow referral responses.
    no_referral: bool,
    /// If `true`, print the server name before each query.
    verbose: bool,
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  whois <query>                  Look up a domain, IP, or ASN");
    eprintln!("  whois -h <server> <query>      Use a specific WHOIS server");
    eprintln!("  whois -p <port> <query>        Use a non-default port (default: 43)");
    eprintln!("  whois --no-referral <query>    Do not follow referral responses");
    eprintln!("  whois -v <query>               Verbose: show server being queried");
    eprintln!("  whois <q1> <q2> ...            Multiple queries");
    eprintln!();
    eprintln!("Queries may be: domain names, IPv4/IPv6 addresses, or ASNs (AS12345).");
}

fn parse_args() -> Result<Args, WhoisError> {
    let argv: Vec<String> = env::args().collect();

    if argv.len() < 2 {
        return Err(WhoisError::InvalidArgument("no query specified".to_string()));
    }

    let mut queries: Vec<String> = Vec::new();
    let mut server: Option<String> = None;
    let mut port = DEFAULT_WHOIS_PORT;
    let mut no_referral = false;
    let mut verbose = false;

    let mut i = 1usize;
    while i < argv.len() {
        let arg = argv.get(i).map(String::as_str).unwrap_or("");

        match arg {
            "-h" | "--host" => {
                i = i.saturating_add(1);
                let val = argv.get(i).ok_or_else(|| {
                    WhoisError::InvalidArgument(format!("{arg} requires a value"))
                })?;
                server = Some(val.clone());
            }
            "-p" | "--port" => {
                i = i.saturating_add(1);
                let val = argv.get(i).ok_or_else(|| {
                    WhoisError::InvalidArgument(format!("{arg} requires a value"))
                })?;
                port = val.parse::<u16>().map_err(|_| {
                    WhoisError::InvalidArgument(format!("invalid port: '{val}'"))
                })?;
            }
            "--no-referral" => {
                no_referral = true;
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            "--help" | "-?" => {
                print_usage();
                process::exit(0);
            }
            _ if arg.starts_with('-') => {
                return Err(WhoisError::InvalidArgument(format!(
                    "unknown option: '{arg}'"
                )));
            }
            _ => {
                queries.push(arg.to_string());
            }
        }

        i = i.saturating_add(1);
    }

    if queries.is_empty() {
        return Err(WhoisError::InvalidArgument(
            "no query specified".to_string(),
        ));
    }

    Ok(Args {
        queries,
        server,
        port,
        no_referral,
        verbose,
    })
}

// ============================================================================
// Main entry point
// ============================================================================

fn run() -> Result<(), WhoisError> {
    let args = parse_args()?;
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let opts = LookupOptions {
        server_override: args.server.as_deref(),
        port: args.port,
        follow_referral: !args.no_referral,
        verbose: args.verbose,
    };

    let query_count = args.queries.len();
    for (idx, query) in args.queries.iter().enumerate() {
        if query_count > 1 {
            writeln!(out, "=== {query} ===").ok();
        }

        match lookup(query, &opts) {
            Ok(results) => {
                for (server, response) in &results {
                    if results.len() > 1 {
                        // Multiple rounds (referral was followed): show which
                        // server produced each response.
                        writeln!(out, "# Results from {server}:").ok();
                        writeln!(out).ok();
                    }
                    out.write_all(response.as_bytes()).ok();
                    // Ensure the response ends with a newline.
                    if !response.ends_with('\n') {
                        writeln!(out).ok();
                    }
                }
            }
            Err(e) => {
                eprintln!("whois: {query}: {e}");
            }
        }

        // Blank line between multiple queries.
        if idx.saturating_add(1) < query_count {
            writeln!(out).ok();
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("whois: {e}");
        print_usage();
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // detect_query_kind
    // ------------------------------------------------------------------

    #[test]
    fn detect_domain_simple() {
        assert_eq!(detect_query_kind("example.com"), QueryKind::Domain);
    }

    #[test]
    fn detect_domain_subdomain() {
        assert_eq!(detect_query_kind("www.example.co.uk"), QueryKind::Domain);
    }

    #[test]
    fn detect_ipv4() {
        assert_eq!(detect_query_kind("1.2.3.4"), QueryKind::Ipv4);
    }

    #[test]
    fn detect_ipv4_loopback() {
        assert_eq!(detect_query_kind("127.0.0.1"), QueryKind::Ipv4);
    }

    #[test]
    fn detect_ipv6_full() {
        assert_eq!(
            detect_query_kind("2001:0db8:85a3:0000:0000:8a2e:0370:7334"),
            QueryKind::Ipv6
        );
    }

    #[test]
    fn detect_ipv6_compressed() {
        assert_eq!(detect_query_kind("2001:db8::1"), QueryKind::Ipv6);
    }

    #[test]
    fn detect_ipv6_loopback() {
        assert_eq!(detect_query_kind("::1"), QueryKind::Ipv6);
    }

    #[test]
    fn detect_asn_with_prefix() {
        assert_eq!(detect_query_kind("AS15169"), QueryKind::Asn);
    }

    #[test]
    fn detect_asn_lowercase_prefix() {
        assert_eq!(detect_query_kind("as64512"), QueryKind::Asn);
    }

    #[test]
    fn detect_asn_bare_digits() {
        assert_eq!(detect_query_kind("64512"), QueryKind::Asn);
    }

    #[test]
    fn detect_asn_not_too_long() {
        // 11 digits is too long to be treated as an ASN.
        assert_ne!(detect_query_kind("12345678901"), QueryKind::Asn);
    }

    // ------------------------------------------------------------------
    // whois_server_for
    // ------------------------------------------------------------------

    #[test]
    fn server_for_com_domain() {
        assert_eq!(
            whois_server_for("example.com", &QueryKind::Domain),
            "whois.verisign-grs.com"
        );
    }

    #[test]
    fn server_for_net_domain() {
        assert_eq!(
            whois_server_for("example.net", &QueryKind::Domain),
            "whois.verisign-grs.com"
        );
    }

    #[test]
    fn server_for_org_domain() {
        assert_eq!(
            whois_server_for("example.org", &QueryKind::Domain),
            "whois.pir.org"
        );
    }

    #[test]
    fn server_for_io_domain() {
        assert_eq!(
            whois_server_for("example.io", &QueryKind::Domain),
            "whois.nic.io"
        );
    }

    #[test]
    fn server_for_dev_domain() {
        assert_eq!(
            whois_server_for("example.dev", &QueryKind::Domain),
            "whois.nic.google"
        );
    }

    #[test]
    fn server_for_uk_domain() {
        assert_eq!(
            whois_server_for("example.uk", &QueryKind::Domain),
            "whois.nic.uk"
        );
    }

    #[test]
    fn server_for_co_uk_domain() {
        // Two-label TLD.
        assert_eq!(
            whois_server_for("example.co.uk", &QueryKind::Domain),
            "whois.nic.uk"
        );
    }

    #[test]
    fn server_for_com_au_domain() {
        assert_eq!(
            whois_server_for("example.com.au", &QueryKind::Domain),
            "whois.auda.org.au"
        );
    }

    #[test]
    fn server_for_ipv4() {
        assert_eq!(
            whois_server_for("8.8.8.8", &QueryKind::Ipv4),
            "whois.arin.net"
        );
    }

    #[test]
    fn server_for_ipv6() {
        assert_eq!(
            whois_server_for("2001:db8::1", &QueryKind::Ipv6),
            "whois.arin.net"
        );
    }

    #[test]
    fn server_for_asn() {
        assert_eq!(
            whois_server_for("AS15169", &QueryKind::Asn),
            "whois.arin.net"
        );
    }

    #[test]
    fn server_for_unknown_tld_falls_back_to_iana() {
        assert_eq!(
            whois_server_for("example.xyzzy", &QueryKind::Domain),
            "whois.iana.org"
        );
    }

    // ------------------------------------------------------------------
    // extract_host_from_referral
    // ------------------------------------------------------------------

    #[test]
    fn extract_plain_hostname() {
        assert_eq!(extract_host_from_referral("whois.ripe.net"), "whois.ripe.net");
    }

    #[test]
    fn extract_whois_uri() {
        assert_eq!(
            extract_host_from_referral("whois://whois.ripe.net"),
            "whois.ripe.net"
        );
    }

    #[test]
    fn extract_whois_uri_with_port() {
        assert_eq!(
            extract_host_from_referral("whois://whois.ripe.net:43"),
            "whois.ripe.net"
        );
    }

    #[test]
    fn extract_rwhois_uri() {
        assert_eq!(
            extract_host_from_referral("rwhois://rwhois.example.com:4321"),
            "rwhois.example.com"
        );
    }

    // ------------------------------------------------------------------
    // parse_referral
    // ------------------------------------------------------------------

    #[test]
    fn referral_from_arin_style() {
        let response = "NetRange: 8.8.0.0 - 8.8.255.255\r\n\
                        ReferralServer: whois://whois.google.com\r\n\
                        CIDR: 8.8.0.0/16\r\n";
        assert_eq!(
            parse_referral(response),
            Some("whois.google.com".to_string())
        );
    }

    #[test]
    fn referral_from_iana_style() {
        let response = "domain:       COM\r\n\
                        refer:        whois.verisign-grs.com\r\n\
                        nserver:      A.GTLD-SERVERS.NET\r\n";
        assert_eq!(
            parse_referral(response),
            Some("whois.verisign-grs.com".to_string())
        );
    }

    #[test]
    fn referral_none_when_absent() {
        let response = "domain: EXAMPLE.COM\r\nStatus: ACTIVE\r\n";
        assert_eq!(parse_referral(response), None);
    }

    #[test]
    fn referral_empty_value_ignored() {
        let response = "ReferralServer:   \r\n";
        assert_eq!(parse_referral(response), None);
    }

    // ------------------------------------------------------------------
    // lookup_tld coverage
    // ------------------------------------------------------------------

    #[test]
    fn known_tlds_have_servers() {
        let known = [
            "com", "net", "org", "io", "dev", "info", "us", "uk", "de", "fr", "au",
        ];
        for tld in &known {
            let srv = lookup_tld(tld);
            assert!(
                !srv.is_empty(),
                "expected non-empty server for TLD .{tld}"
            );
            assert_ne!(srv, "whois.iana.org", ".{tld} should have its own server");
        }
    }

    #[test]
    fn unknown_tld_returns_iana() {
        assert_eq!(lookup_tld("zzzzz"), "whois.iana.org");
    }

    // ------------------------------------------------------------------
    // server_for_domain edge cases
    // ------------------------------------------------------------------

    #[test]
    fn domain_with_only_tld_does_not_panic() {
        // Single-label input: treat as unknown TLD.
        let srv = server_for_domain("com");
        // Must return some server without panicking.
        assert!(!srv.is_empty());
    }

    #[test]
    fn domain_empty_string_does_not_panic() {
        let srv = server_for_domain("");
        assert!(!srv.is_empty());
    }
}
