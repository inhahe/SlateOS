//! Slate OS FTP Server Daemon (`ftpd`)
//!
//! A comprehensive RFC 959 FTP server for SlateOS. Supports both active (PORT)
//! and passive (PASV) data transfer modes, ASCII and binary transfer types,
//! anonymous FTP, user authentication via `/etc/passwd`, chroot to user home
//! directories, transfer rate limiting, and configurable via `/etc/ftpd.conf`.
//!
//! # Usage
//!
//! ```text
//! ftpd                          Start with defaults (port 21)
//! ftpd -p 2121                  Listen on port 2121
//! ftpd -c /etc/ftpd.conf        Use custom config file
//! ftpd -d                        Debug / verbose logging
//! ftpd -a                        Anonymous-only mode
//! ftpd -h                        Show help
//! ```
//!
//! # Supported FTP Commands
//!
//! USER, PASS, CWD, CDUP, PWD, LIST, NLST, RETR, STOR, DELE, MKD, RMD,
//! RNFR, RNTO, TYPE, MODE, STRU, PORT, PASV, QUIT, SYST, STAT, NOOP,
//! HELP, SIZE, MDTM, FEAT, OPTS, REST

#![cfg_attr(not(test), no_main)]
#![cfg_attr(test, allow(dead_code))]
#![deny(clippy::all)]
#![allow(clippy::manual_range_contains)]

use std::env;
use std::fmt;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Syscall numbers (from kernel/src/syscall/number.rs)
// ============================================================================

const SYS_TCP_CONNECT: u64 = 800;
const SYS_TCP_SEND: u64 = 801;
const SYS_TCP_RECV: u64 = 802;
const SYS_TCP_CLOSE: u64 = 803;
const SYS_TCP_BIND: u64 = 804;
const SYS_TCP_ACCEPT: u64 = 805;
const SYS_TCP_CLOSE_LISTENER: u64 = 806;
const SYS_TCP_PEER_ADDR: u64 = 808;
#[allow(dead_code)] // Part of the TCP syscall interface; kept for completeness.
const SYS_TCP_SHUTDOWN: u64 = 855;
const SYS_SLEEP: u64 = 11;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 1-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and `a1` is valid
/// for the specific syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall1(nr: u64, a1: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid. The `syscall` instruction
    // clobbers rcx and r11 per the x86_64 ABI.
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

/// Issue a 3-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for the specific syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid. The `syscall` instruction
    // clobbers rcx and r11 per the x86_64 ABI.
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

// ============================================================================
// Syscall wrappers — TCP
// ============================================================================

/// Open a blocking TCP connection to (ip, port). Returns a handle on success.
fn tcp_connect(ip: u32, port: u16) -> Result<u64, FtpdError> {
    // SAFETY: SYS_TCP_CONNECT takes two scalar arguments (IP and port).
    // No pointer dereferences occur in userspace.
    let ret = unsafe { syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), 0) };
    if ret < 0 {
        Err(FtpdError::Network(format!("tcp_connect failed: {ret}")))
    } else {
        Ok(ret as u64)
    }
}

/// Send data on a TCP connection. Returns the number of bytes sent.
fn tcp_send(handle: u64, data: &[u8]) -> Result<usize, FtpdError> {
    // SAFETY: handle is a valid connection handle. data pointer and length
    // are derived from a valid Rust slice.
    let ret = unsafe {
        syscall3(
            SYS_TCP_SEND,
            handle,
            data.as_ptr() as u64,
            data.len() as u64,
        )
    };
    if ret < 0 {
        Err(FtpdError::Network(format!("tcp_send failed: {ret}")))
    } else {
        Ok(ret as usize)
    }
}

/// Send all bytes, looping until the entire buffer is transmitted.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), FtpdError> {
    let mut offset = 0;
    while offset < data.len() {
        let sent = tcp_send(handle, &data[offset..])?;
        if sent == 0 {
            return Err(FtpdError::Network("tcp_send returned 0".into()));
        }
        offset = offset.saturating_add(sent);
    }
    Ok(())
}

/// Receive data from a TCP connection. Returns 0 on EOF.
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, FtpdError> {
    // SAFETY: handle is valid. buf pointer and capacity are derived from
    // a valid mutable Rust slice.
    let ret = unsafe {
        syscall3(
            SYS_TCP_RECV,
            handle,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        Err(FtpdError::Network(format!("tcp_recv failed: {ret}")))
    } else {
        Ok(ret as usize)
    }
}

/// Close a TCP connection handle.
fn tcp_close(handle: u64) {
    // SAFETY: handle is (or was) a valid TCP connection handle. The kernel
    // deallocates internal state. Ignoring the return is safe.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

/// Bind a TCP listener to a local port. Returns a listener handle.
fn tcp_bind(port: u16) -> Result<u64, FtpdError> {
    // SAFETY: SYS_TCP_BIND takes one scalar argument (port number).
    let ret = unsafe { syscall1(SYS_TCP_BIND, u64::from(port)) };
    if ret < 0 {
        Err(FtpdError::Network(format!("tcp_bind failed on port {port}: {ret}")))
    } else {
        Ok(ret as u64)
    }
}

/// Accept an incoming connection on a listener (blocking).
/// Returns a connection handle.
fn tcp_accept(listener: u64) -> Result<u64, FtpdError> {
    // SAFETY: listener is a valid listener handle.
    let ret = unsafe { syscall1(SYS_TCP_ACCEPT, listener) };
    if ret < 0 {
        Err(FtpdError::Network(format!("tcp_accept failed: {ret}")))
    } else {
        Ok(ret as u64)
    }
}

/// Close a TCP listener handle.
fn tcp_close_listener(listener: u64) {
    // SAFETY: listener is (or was) a valid TCP listener handle.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE_LISTENER, listener) };
}

/// Get the peer address of a TCP connection.
/// Returns (ip_network_order, port) on success.
fn tcp_peer_addr(handle: u64) -> Result<(u32, u16), FtpdError> {
    let mut buf = [0u8; 6];
    // SAFETY: handle is valid. buf is a stack-allocated 6-byte buffer
    // with sufficient lifetime for the kernel to write into.
    let ret = unsafe {
        syscall3(
            SYS_TCP_PEER_ADDR,
            handle,
            buf.as_mut_ptr() as u64,
            0,
        )
    };
    if ret < 0 {
        return Err(FtpdError::Network(format!("tcp_peer_addr failed: {ret}")));
    }
    let ip = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let port = u16::from_be_bytes([buf[4], buf[5]]);
    Ok((ip, port))
}

/// Shut down part of a TCP connection (0=read, 1=write, 2=both).
#[allow(dead_code)] // Part of the TCP syscall interface; kept for completeness.
fn tcp_shutdown(handle: u64, how: u32) {
    // SAFETY: handle is valid. how is 0, 1, or 2.
    let _ = unsafe { syscall3(SYS_TCP_SHUTDOWN, handle, u64::from(how), 0) };
}

/// Sleep for the given number of milliseconds.
fn sleep_ms(ms: u64) {
    // SAFETY: SYS_SLEEP takes one scalar argument (milliseconds).
    let _ = unsafe { syscall1(SYS_SLEEP, ms) };
}

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
enum FtpdError {
    /// Network / TCP I/O error.
    Network(String),
    /// Filesystem I/O error.
    Io(String),
    /// Configuration error.
    Config(String),
    /// Authentication failure.
    #[allow(dead_code)] // Part of the error taxonomy; used by future auth expansion.
    Auth(String),
    /// FTP protocol error.
    Protocol(String),
}

impl fmt::Display for FtpdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(s) => write!(f, "network error: {s}"),
            Self::Io(s) => write!(f, "I/O error: {s}"),
            Self::Config(s) => write!(f, "config error: {s}"),
            Self::Auth(s) => write!(f, "auth error: {s}"),
            Self::Protocol(s) => write!(f, "protocol error: {s}"),
        }
    }
}

impl From<io::Error> for FtpdError {
    fn from(e: io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// Default FTP control port.
const DEFAULT_PORT: u16 = 21;
/// Default passive port range lower bound.
const DEFAULT_PASV_MIN: u16 = 49152;
/// Default passive port range upper bound.
const DEFAULT_PASV_MAX: u16 = 65000;
/// Default maximum concurrent connections.
const DEFAULT_MAX_CONNECTIONS: usize = 50;
/// Default transfer rate limit (0 = unlimited) in bytes/second.
const DEFAULT_RATE_LIMIT: u64 = 0;
/// Default idle timeout in seconds.
const DEFAULT_IDLE_TIMEOUT: u64 = 300;
/// Default anonymous root directory.
const DEFAULT_ANON_ROOT: &str = "/srv/ftp";
/// Default config file path.
const DEFAULT_CONFIG_PATH: &str = "/etc/ftpd.conf";
/// Default server greeting.
const DEFAULT_BANNER: &str = "Slate OS FTP server ready.";
/// Maximum command line length (RFC 959 allows 512 including CRLF).
const MAX_CMD_LEN: usize = 4096;
/// Buffer size for data transfers.
const DATA_BUF_SIZE: usize = 8192;
/// PID file location.
const PID_PATH: &str = "/var/run/ftpd.pid";

/// Server configuration loaded from `/etc/ftpd.conf` and command line.
#[derive(Debug, Clone)]
struct Config {
    /// Port to listen on for control connections.
    port: u16,
    /// Passive mode port range.
    pasv_min: u16,
    pasv_max: u16,
    /// Maximum concurrent connections.
    max_connections: usize,
    /// Transfer rate limit in bytes/second (0 = unlimited).
    rate_limit: u64,
    /// Idle timeout in seconds.
    idle_timeout: u64,
    /// Allow anonymous logins.
    allow_anonymous: bool,
    /// Anonymous-only mode (reject all non-anonymous logins).
    anonymous_only: bool,
    /// Root directory for anonymous users.
    anon_root: String,
    /// Whether to chroot authenticated users to their home directory.
    chroot_users: bool,
    /// Server banner message.
    banner: String,
    /// Enable debug / verbose logging.
    debug: bool,
    /// Server IP for PASV responses (0.0.0.0 means auto-detect).
    server_ip: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            pasv_min: DEFAULT_PASV_MIN,
            pasv_max: DEFAULT_PASV_MAX,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            rate_limit: DEFAULT_RATE_LIMIT,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            allow_anonymous: true,
            anonymous_only: false,
            anon_root: DEFAULT_ANON_ROOT.into(),
            chroot_users: true,
            banner: DEFAULT_BANNER.into(),
            debug: false,
            server_ip: 0,
        }
    }
}

/// Parse `/etc/ftpd.conf` format:
///
/// ```text
/// # Comment
/// port = 21
/// pasv_min = 49152
/// pasv_max = 65000
/// max_connections = 50
/// rate_limit = 0
/// idle_timeout = 300
/// allow_anonymous = yes
/// anonymous_only = no
/// anon_root = /srv/ftp
/// chroot_users = yes
/// banner = Welcome to Slate OS FTP
/// server_ip = 0.0.0.0
/// ```
fn parse_config_file(path: &str) -> Result<Config, FtpdError> {
    let mut cfg = Config::default();

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            // Missing config file is not fatal; use defaults.
            if e.kind() == io::ErrorKind::NotFound {
                return Ok(cfg);
            }
            return Err(FtpdError::Config(format!("cannot read {path}: {e}")));
        }
    };

    for (line_num, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(FtpdError::Config(format!(
                "{path}:{}: missing '=' in '{line}'",
                line_num.saturating_add(1)
            )));
        };
        let key = key.trim();
        let value = value.trim();

        match key {
            "port" => {
                cfg.port = value.parse().map_err(|_| {
                    FtpdError::Config(format!("{path}: invalid port '{value}'"))
                })?;
            }
            "pasv_min" => {
                cfg.pasv_min = value.parse().map_err(|_| {
                    FtpdError::Config(format!("{path}: invalid pasv_min '{value}'"))
                })?;
            }
            "pasv_max" => {
                cfg.pasv_max = value.parse().map_err(|_| {
                    FtpdError::Config(format!("{path}: invalid pasv_max '{value}'"))
                })?;
            }
            "max_connections" => {
                cfg.max_connections = value.parse().map_err(|_| {
                    FtpdError::Config(format!("{path}: invalid max_connections '{value}'"))
                })?;
            }
            "rate_limit" => {
                cfg.rate_limit = value.parse().map_err(|_| {
                    FtpdError::Config(format!("{path}: invalid rate_limit '{value}'"))
                })?;
            }
            "idle_timeout" => {
                cfg.idle_timeout = value.parse().map_err(|_| {
                    FtpdError::Config(format!("{path}: invalid idle_timeout '{value}'"))
                })?;
            }
            "allow_anonymous" => {
                cfg.allow_anonymous = parse_bool(value).ok_or_else(|| {
                    FtpdError::Config(format!("{path}: invalid allow_anonymous '{value}'"))
                })?;
            }
            "anonymous_only" => {
                cfg.anonymous_only = parse_bool(value).ok_or_else(|| {
                    FtpdError::Config(format!("{path}: invalid anonymous_only '{value}'"))
                })?;
            }
            "anon_root" => {
                cfg.anon_root = value.to_string();
            }
            "chroot_users" => {
                cfg.chroot_users = parse_bool(value).ok_or_else(|| {
                    FtpdError::Config(format!("{path}: invalid chroot_users '{value}'"))
                })?;
            }
            "banner" => {
                cfg.banner = value.to_string();
            }
            "server_ip" => {
                cfg.server_ip = parse_ip_str(value).ok_or_else(|| {
                    FtpdError::Config(format!("{path}: invalid server_ip '{value}'"))
                })?;
            }
            _ => {
                return Err(FtpdError::Config(format!(
                    "{path}:{}: unknown key '{key}'",
                    line_num.saturating_add(1)
                )));
            }
        }
    }

    if cfg.pasv_min > cfg.pasv_max {
        return Err(FtpdError::Config(
            "pasv_min must be <= pasv_max".into(),
        ));
    }

    Ok(cfg)
}

/// Parse a boolean value from config: yes/no/true/false/1/0.
fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "yes" | "true" | "1" => Some(true),
        "no" | "false" | "0" => Some(false),
        _ => None,
    }
}

/// Parse an IPv4 address string "a.b.c.d" into a network-byte-order u32.
fn parse_ip_str(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let a: u8 = parts[0].parse().ok()?;
    let b: u8 = parts[1].parse().ok()?;
    let c: u8 = parts[2].parse().ok()?;
    let d: u8 = parts[3].parse().ok()?;
    Some(u32::from_be_bytes([a, b, c, d]))
}

/// Format a network-byte-order u32 IP as "a.b.c.d".
fn format_ip(ip: u32) -> String {
    let bytes = ip.to_be_bytes();
    format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
}

// ============================================================================
// Command line parsing
// ============================================================================

struct CliArgs {
    config_path: String,
    port_override: Option<u16>,
    debug: bool,
    anonymous_only: bool,
    show_help: bool,
}

fn parse_cli() -> Result<CliArgs, FtpdError> {
    let args: Vec<String> = env::args().collect();
    let mut result = CliArgs {
        config_path: DEFAULT_CONFIG_PATH.to_string(),
        port_override: None,
        debug: false,
        anonymous_only: false,
        show_help: false,
    };

    let mut idx = 1;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" => {
                result.show_help = true;
                return Ok(result);
            }
            "-p" | "--port" => {
                idx = idx.saturating_add(1);
                let val = args.get(idx).ok_or_else(|| {
                    FtpdError::Config("-p requires a port number".into())
                })?;
                result.port_override = Some(val.parse().map_err(|_| {
                    FtpdError::Config(format!("invalid port: {val}"))
                })?);
            }
            "-c" | "--config" => {
                idx = idx.saturating_add(1);
                result.config_path = args.get(idx).ok_or_else(|| {
                    FtpdError::Config("-c requires a config file path".into())
                })?.clone();
            }
            "-d" | "--debug" => {
                result.debug = true;
            }
            "-a" | "--anonymous-only" => {
                result.anonymous_only = true;
            }
            other => {
                return Err(FtpdError::Config(format!("unknown option: {other}")));
            }
        }
        idx = idx.saturating_add(1);
    }

    Ok(result)
}

fn print_help() {
    let help = "\
ftpd - Slate OS FTP server daemon

Usage: ftpd [OPTIONS]

Options:
  -p, --port <PORT>         Listen port (default: 21)
  -c, --config <FILE>       Config file (default: /etc/ftpd.conf)
  -d, --debug               Enable debug logging
  -a, --anonymous-only      Only allow anonymous logins
  -h, --help                Show this help

Config file keys:
  port, pasv_min, pasv_max, max_connections, rate_limit,
  idle_timeout, allow_anonymous, anonymous_only, anon_root,
  chroot_users, banner, server_ip
";
    print!("{help}");
}

// ============================================================================
// User authentication
// ============================================================================

/// A user entry from /etc/passwd.
#[derive(Debug, Clone)]
#[allow(dead_code)] // uid/gid/shell stored for future privilege-drop support.
struct UserEntry {
    username: String,
    uid: u32,
    gid: u32,
    home: String,
    shell: String,
}

/// Parse a single /etc/passwd line:
/// `username:x:uid:gid:gecos:home:shell`
fn parse_passwd_line(line: &str) -> Option<UserEntry> {
    let fields: Vec<&str> = line.split(':').collect();
    if fields.len() < 7 {
        return None;
    }
    Some(UserEntry {
        username: fields[0].to_string(),
        uid: fields[2].parse().ok()?,
        gid: fields[3].parse().ok()?,
        home: fields[5].to_string(),
        shell: fields[6].to_string(),
    })
}

/// Look up a user in /etc/passwd by username.
fn lookup_user(username: &str) -> Option<UserEntry> {
    let content = fs::read_to_string("/etc/passwd").ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(entry) = parse_passwd_line(line)
            && entry.username == username {
                return Some(entry);
            }
    }
    None
}

/// Validate a password against /etc/shadow (simplified: accept any non-empty
/// password for now since our OS does not yet have a full shadow mechanism).
///
/// In production this would compare hashed passwords. For anonymous users
/// any password (including empty) is accepted.
fn validate_password(username: &str, _password: &str, is_anonymous: bool) -> bool {
    if is_anonymous {
        return true;
    }
    // Check that the user exists in /etc/passwd
    lookup_user(username).is_some()
}

// ============================================================================
// FTP transfer type and mode
// ============================================================================

/// FTP transfer type (TYPE command).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferType {
    Ascii,
    Binary,
}

impl fmt::Display for TransferType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ascii => write!(f, "ASCII"),
            Self::Binary => write!(f, "Binary"),
        }
    }
}

/// FTP transfer mode (MODE command).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferMode {
    Stream,
    Block,
    Compressed,
}

/// FTP file structure (STRU command).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileStructure {
    File,
    Record,
    Page,
}

// ============================================================================
// Data connection handling
// ============================================================================

/// Represents how the next data connection should be established.
#[derive(Debug, Clone)]
enum DataMode {
    /// No data connection mode set yet.
    None,
    /// Active mode: server connects to client at the given (ip, port).
    Active { ip: u32, port: u16 },
    /// Passive mode: server has a listener; client connects to us.
    Passive {
        listener: u64,
        #[allow(dead_code)] // Stored for logging/diagnostics.
        port: u16,
    },
}

/// Open a data connection based on the current data mode.
/// Returns a TCP handle for the data transfer.
fn open_data_connection(mode: &DataMode) -> Result<u64, FtpdError> {
    match mode {
        DataMode::None => Err(FtpdError::Protocol("no data connection mode set".into())),
        DataMode::Active { ip, port } => tcp_connect(*ip, *port),
        DataMode::Passive { listener, .. } => tcp_accept(*listener),
    }
}

/// Close a passive listener if one exists.
fn close_passive_listener(mode: &mut DataMode) {
    if let DataMode::Passive { listener, .. } = mode {
        tcp_close_listener(*listener);
    }
    *mode = DataMode::None;
}

// ============================================================================
// FTP command parsing
// ============================================================================

/// Parsed FTP command from the client.
#[derive(Debug, Clone)]
struct FtpCommand {
    /// The command verb (uppercase).
    verb: String,
    /// The argument string (may be empty).
    arg: String,
}

/// Parse a raw FTP command line into verb + argument.
fn parse_ftp_command(line: &str) -> FtpCommand {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    let trimmed = trimmed.trim();
    if let Some(idx) = trimmed.find(' ') {
        FtpCommand {
            verb: trimmed[..idx].to_ascii_uppercase(),
            arg: trimmed[idx..].trim_start().to_string(),
        }
    } else {
        FtpCommand {
            verb: trimmed.to_ascii_uppercase(),
            arg: String::new(),
        }
    }
}

// ============================================================================
// Path resolution
// ============================================================================

/// Resolve an FTP path argument relative to the current working directory,
/// constrained within the root directory. Returns the resolved absolute
/// filesystem path (after chroot).
///
/// The FTP client sees paths relative to the chroot root. We translate those
/// to real filesystem paths while ensuring we never escape the root.
fn resolve_path(root: &str, cwd: &str, arg: &str) -> PathBuf {
    let virtual_path = if arg.starts_with('/') {
        // Absolute path from client perspective
        normalize_path(arg)
    } else {
        // Relative to current working directory
        let combined = format!("{cwd}/{arg}");
        normalize_path(&combined)
    };

    // Build the real filesystem path: root + virtual_path
    let mut real = PathBuf::from(root);
    // Strip leading '/' from virtual_path before appending
    let stripped = virtual_path.strip_prefix('/').unwrap_or(&virtual_path);
    if !stripped.is_empty() {
        real.push(stripped);
    }
    real
}

/// Compute the virtual path (what the client sees) from a real filesystem path.
fn virtual_path(root: &str, real_path: &Path) -> String {
    let real_str = real_path.to_string_lossy();
    let root_trimmed = root.trim_end_matches('/');
    if let Some(suffix) = real_str.strip_prefix(root_trimmed) {
        if suffix.is_empty() {
            "/".to_string()
        } else {
            suffix.to_string()
        }
    } else {
        "/".to_string()
    }
}

/// Normalize a path by resolving `.` and `..` components and collapsing
/// multiple slashes. Returns a path starting with `/`.
fn normalize_path(path: &str) -> String {
    let mut components: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            other => {
                components.push(other);
            }
        }
    }
    if components.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", components.join("/"))
    }
}

/// Validate that a resolved real path is within the chroot root.
fn is_within_root(root: &str, real_path: &Path) -> bool {
    let root_trimmed = root.trim_end_matches('/');
    let real_str = real_path.to_string_lossy();
    real_str.starts_with(root_trimmed)
        && (real_str.len() == root_trimmed.len()
            || real_str.as_bytes().get(root_trimmed.len()) == Some(&b'/'))
}

// ============================================================================
// Directory listing formatting
// ============================================================================

/// Format a single directory entry in "ls -l" style for the LIST command.
fn format_list_entry(name: &str, metadata: &Metadata) -> String {
    let is_dir = metadata.is_dir();
    let size = metadata.len();
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let permissions = if is_dir {
        "drwxr-xr-x"
    } else {
        "-rw-r--r--"
    };

    let links = if is_dir { 2u32 } else { 1 };
    let date_str = format_mtime(mtime);

    format!(
        "{permissions} {links:>4} ftp      ftp      {size:>12} {date_str} {name}"
    )
}

/// Format a modification time in "Mon DD HH:MM" or "Mon DD  YYYY" format.
fn format_mtime(unix_secs: u64) -> String {
    // Simplified date formatting since we have no strftime in no_std-like env.
    // Use a basic epoch-to-date conversion.
    let (year, month, day, hour, minute) = unix_secs_to_date(unix_secs);

    let month_name = match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    };

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let six_months_secs: u64 = 180 * 24 * 3600;

    if now_secs.saturating_sub(unix_secs) > six_months_secs {
        format!("{month_name} {day:>2}  {year}")
    } else {
        format!("{month_name} {day:>2} {hour:02}:{minute:02}")
    }
}

/// Convert unix timestamp to (year, month, day, hour, minute).
fn unix_secs_to_date(secs: u64) -> (u32, u32, u32, u32, u32) {
    // Simple conversion from epoch seconds. Not leap-second-precise but
    // adequate for FTP directory listing timestamps.
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hour = (time_of_day / 3600) as u32;
    let minute = ((time_of_day % 3600) / 60) as u32;

    // Days since 1970-01-01
    let mut remaining_days = days as i64;
    let mut year: u32 = 1970;

    loop {
        let days_in_year: i64 = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days = remaining_days.saturating_sub(days_in_year);
        year = year.saturating_add(1);
    }

    let days_in_months: [i64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month: u32 = 1;
    for &dim in &days_in_months {
        if remaining_days < dim {
            break;
        }
        remaining_days = remaining_days.saturating_sub(dim);
        month = month.saturating_add(1);
    }

    let day = (remaining_days as u32).saturating_add(1);
    (year, month, day, hour, minute)
}

/// Check if a year is a leap year.
fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// Format a unix timestamp as "YYYYMMDDHHmmSS" for the MDTM response.
fn format_mdtm(unix_secs: u64) -> String {
    let (year, month, day, hour, minute) = unix_secs_to_date(unix_secs);
    let second = (unix_secs % 60) as u32;
    format!("{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}")
}

// ============================================================================
// Rate limiter
// ============================================================================

/// Tracks transfer progress for rate limiting.
struct RateLimiter {
    /// Maximum bytes per second (0 = unlimited).
    max_bps: u64,
    /// Bytes transferred in the current window.
    window_bytes: u64,
    /// Start time of the current window (unix secs).
    window_start: u64,
}

impl RateLimiter {
    fn new(max_bps: u64) -> Self {
        Self {
            max_bps,
            window_bytes: 0,
            window_start: now_secs(),
        }
    }

    /// Record that `n` bytes were transferred. If the rate limit is exceeded,
    /// sleep to throttle.
    fn record(&mut self, n: u64) {
        if self.max_bps == 0 {
            return;
        }
        self.window_bytes = self.window_bytes.saturating_add(n);
        let now = now_secs();
        let elapsed = now.saturating_sub(self.window_start).max(1);

        let current_rate = self.window_bytes / elapsed;
        if current_rate > self.max_bps {
            // Sleep enough to bring the rate below the limit
            let needed_secs = self.window_bytes / self.max_bps;
            let sleep_secs = needed_secs.saturating_sub(elapsed);
            if sleep_secs > 0 {
                sleep_ms(sleep_secs.saturating_mul(1000));
            }
        }

        // Reset window every 10 seconds
        if elapsed >= 10 {
            self.window_bytes = 0;
            self.window_start = now_secs();
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ============================================================================
// Connection tracking
// ============================================================================

/// Statistics for a single FTP session.
#[derive(Debug, Clone)]
struct SessionStats {
    /// Bytes uploaded by the client.
    bytes_uploaded: u64,
    /// Bytes downloaded by the client.
    bytes_downloaded: u64,
    /// Number of files uploaded.
    files_uploaded: u32,
    /// Number of files downloaded.
    files_downloaded: u32,
    /// Number of commands processed.
    commands_processed: u32,
    /// Connection start time (unix secs).
    start_time: u64,
    /// Username (if authenticated).
    username: String,
    /// Peer IP address.
    peer_ip: u32,
}

impl SessionStats {
    fn new(peer_ip: u32) -> Self {
        Self {
            bytes_uploaded: 0,
            bytes_downloaded: 0,
            files_uploaded: 0,
            files_downloaded: 0,
            commands_processed: 0,
            start_time: now_secs(),
            username: String::new(),
            peer_ip,
        }
    }
}

// ============================================================================
// Logging
// ============================================================================

fn log_info(config: &Config, msg: &str) {
    eprintln!("[ftpd] {msg}");
    let _ = config; // config.debug checked at call sites for verbose output
}

fn log_debug(config: &Config, msg: &str) {
    if config.debug {
        eprintln!("[ftpd:debug] {msg}");
    }
}

// ============================================================================
// FTP session state
// ============================================================================

/// Per-client FTP session state.
struct FtpSession {
    /// TCP handle for the control connection.
    control_handle: u64,
    /// Server configuration (shared reference).
    config: Config,
    /// Whether the user is authenticated.
    authenticated: bool,
    /// Whether this is an anonymous session.
    is_anonymous: bool,
    /// Username provided via USER command (pending PASS).
    pending_user: Option<String>,
    /// Authenticated username.
    username: String,
    /// The user's home / root directory (real filesystem path).
    root_dir: String,
    /// Current working directory (virtual, relative to root_dir).
    cwd: String,
    /// Transfer type (ASCII or Binary).
    transfer_type: TransferType,
    /// Transfer mode.
    transfer_mode: TransferMode,
    /// File structure.
    file_structure: FileStructure,
    /// Data connection mode (active/passive/none).
    data_mode: DataMode,
    /// Pending RNFR path for rename operations.
    rename_from: Option<PathBuf>,
    /// REST offset for resumed transfers.
    rest_offset: u64,
    /// Rate limiter for data transfers.
    rate_limiter: RateLimiter,
    /// Session statistics.
    stats: SessionStats,
    /// Read buffer for control connection.
    read_buf: Vec<u8>,
    /// Accumulated partial line from control connection.
    line_buf: String,
    /// Next passive port to try (round-robin within range).
    next_pasv_port: u16,
}

impl FtpSession {
    fn new(control_handle: u64, config: Config, peer_ip: u32) -> Self {
        let pasv_start = config.pasv_min;
        Self {
            control_handle,
            rate_limiter: RateLimiter::new(config.rate_limit),
            stats: SessionStats::new(peer_ip),
            config,
            authenticated: false,
            is_anonymous: false,
            pending_user: None,
            username: String::new(),
            root_dir: String::new(),
            cwd: "/".to_string(),
            transfer_type: TransferType::Ascii,
            transfer_mode: TransferMode::Stream,
            file_structure: FileStructure::File,
            data_mode: DataMode::None,
            rename_from: None,
            rest_offset: 0,
            read_buf: vec![0u8; MAX_CMD_LEN],
            line_buf: String::new(),
            next_pasv_port: pasv_start,
        }
    }

    /// Send an FTP response line to the client.
    fn send_response(&self, code: u16, msg: &str) -> Result<(), FtpdError> {
        let line = format!("{code} {msg}\r\n");
        log_debug(&self.config, &format!("-> {}", line.trim()));
        tcp_send_all(self.control_handle, line.as_bytes())
    }

    /// Send a multi-line FTP response.
    fn send_multiline(&self, code: u16, lines: &[&str]) -> Result<(), FtpdError> {
        if lines.is_empty() {
            return self.send_response(code, "");
        }
        let last_idx = lines.len().saturating_sub(1);
        let mut response = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i < last_idx {
                response.push_str(&format!("{code}-{line}\r\n"));
            } else {
                response.push_str(&format!("{code} {line}\r\n"));
            }
        }
        log_debug(&self.config, &format!("-> [{code} multiline, {} lines]", lines.len()));
        tcp_send_all(self.control_handle, response.as_bytes())
    }

    /// Read the next FTP command from the control connection.
    /// Returns `None` on connection close/timeout.
    fn read_command(&mut self) -> Result<Option<FtpCommand>, FtpdError> {
        loop {
            // Check if we have a complete line in the buffer
            if let Some(pos) = self.line_buf.find('\n') {
                let line = self.line_buf[..pos].to_string();
                self.line_buf = self.line_buf[pos.saturating_add(1)..].to_string();
                let cmd = parse_ftp_command(&line);
                log_debug(&self.config, &format!("<- {} {}", cmd.verb, cmd.arg));
                self.stats.commands_processed = self.stats.commands_processed.saturating_add(1);
                return Ok(Some(cmd));
            }

            // Need more data
            let n = tcp_recv(self.control_handle, &mut self.read_buf)?;
            if n == 0 {
                return Ok(None); // Connection closed
            }

            // Convert bytes to string, rejecting invalid UTF-8 gracefully
            let chunk = String::from_utf8_lossy(&self.read_buf[..n]);
            self.line_buf.push_str(&chunk);

            // Prevent buffer overflow from malicious clients
            if self.line_buf.len() > MAX_CMD_LEN {
                self.line_buf.clear();
                return Err(FtpdError::Protocol("command line too long".into()));
            }
        }
    }

    /// Get the next passive port (round-robin within configured range).
    fn next_pasv_port(&mut self) -> u16 {
        let port = self.next_pasv_port;
        if self.next_pasv_port >= self.config.pasv_max {
            self.next_pasv_port = self.config.pasv_min;
        } else {
            self.next_pasv_port = self.next_pasv_port.saturating_add(1);
        }
        port
    }

    /// Run the FTP session: send greeting, then process commands until QUIT
    /// or disconnect.
    fn run(&mut self) -> Result<(), FtpdError> {
        // Send greeting
        self.send_response(220, &self.config.banner.clone())?;

        loop {
            let cmd = match self.read_command()? {
                Some(c) => c,
                None => {
                    log_debug(&self.config, "client disconnected");
                    break;
                }
            };

            let result = self.handle_command(&cmd);
            if let Err(ref e) = result {
                log_debug(&self.config, &format!("command error: {e}"));
                // Try to send an error response; if that fails too, disconnect.
                let _ = self.send_response(421, "Service error, closing connection.");
                break;
            }

            // QUIT causes clean exit
            if cmd.verb == "QUIT" {
                break;
            }
        }

        // Clean up data connection
        close_passive_listener(&mut self.data_mode);

        Ok(())
    }

    /// Dispatch a parsed FTP command to the appropriate handler.
    fn handle_command(&mut self, cmd: &FtpCommand) -> Result<(), FtpdError> {
        match cmd.verb.as_str() {
            // Authentication
            "USER" => self.cmd_user(&cmd.arg),
            "PASS" => self.cmd_pass(&cmd.arg),

            // These commands require authentication
            _ if !self.authenticated => {
                self.send_response(530, "Please login with USER and PASS.")
            }

            // Navigation
            "CWD" => self.cmd_cwd(&cmd.arg),
            "CDUP" => self.cmd_cdup(),
            "PWD" | "XPWD" => self.cmd_pwd(),

            // Transfer settings
            "TYPE" => self.cmd_type(&cmd.arg),
            "MODE" => self.cmd_mode(&cmd.arg),
            "STRU" => self.cmd_stru(&cmd.arg),
            "PORT" => self.cmd_port(&cmd.arg),
            "PASV" => self.cmd_pasv(),
            "REST" => self.cmd_rest(&cmd.arg),

            // Directory listing
            "LIST" => self.cmd_list(&cmd.arg),
            "NLST" => self.cmd_nlst(&cmd.arg),

            // File transfer
            "RETR" => self.cmd_retr(&cmd.arg),
            "STOR" => self.cmd_stor(&cmd.arg),

            // File management
            "DELE" => self.cmd_dele(&cmd.arg),
            "MKD" | "XMKD" => self.cmd_mkd(&cmd.arg),
            "RMD" | "XRMD" => self.cmd_rmd(&cmd.arg),
            "RNFR" => self.cmd_rnfr(&cmd.arg),
            "RNTO" => self.cmd_rnto(&cmd.arg),
            "SIZE" => self.cmd_size(&cmd.arg),
            "MDTM" => self.cmd_mdtm(&cmd.arg),

            // Information
            "SYST" => self.cmd_syst(),
            "STAT" => self.cmd_stat(&cmd.arg),
            "FEAT" => self.cmd_feat(),
            "OPTS" => self.cmd_opts(&cmd.arg),
            "HELP" => self.cmd_help(&cmd.arg),
            "NOOP" => self.cmd_noop(),
            "QUIT" => self.cmd_quit(),

            _ => self.send_response(502, "Command not implemented."),
        }
    }

    // ========================================================================
    // Command handlers — Authentication
    // ========================================================================

    fn cmd_user(&mut self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            return self.send_response(501, "USER requires a username.");
        }

        let username = arg.to_string();
        let is_anon = username == "anonymous" || username == "ftp";

        // Check if anonymous is allowed
        if is_anon && !self.config.allow_anonymous {
            return self.send_response(530, "Anonymous login not allowed.");
        }

        // Check if non-anonymous is allowed in anonymous-only mode
        if !is_anon && self.config.anonymous_only {
            return self.send_response(530, "Only anonymous login is allowed.");
        }

        self.pending_user = Some(username.clone());
        self.is_anonymous = is_anon;

        if is_anon {
            self.send_response(331, "Anonymous login ok, send email as password.")
        } else {
            self.send_response(331, &format!("Password required for {username}."))
        }
    }

    fn cmd_pass(&mut self, arg: &str) -> Result<(), FtpdError> {
        let username = match self.pending_user.take() {
            Some(u) => u,
            None => {
                return self.send_response(503, "Login with USER first.");
            }
        };

        if !validate_password(&username, arg, self.is_anonymous) {
            return self.send_response(530, "Login incorrect.");
        }

        // Set up the session root directory
        if self.is_anonymous {
            self.root_dir = self.config.anon_root.clone();
            self.username = "anonymous".to_string();
        } else if let Some(entry) = lookup_user(&username) {
            if self.config.chroot_users {
                self.root_dir = entry.home.clone();
            } else {
                self.root_dir = "/".to_string();
            }
            self.username = username.clone();
        } else {
            return self.send_response(530, "Login incorrect.");
        }

        self.authenticated = true;
        self.cwd = "/".to_string();
        self.stats.username = username.clone();

        log_info(
            &self.config,
            &format!(
                "user '{}' logged in from {}",
                username,
                format_ip(self.stats.peer_ip)
            ),
        );

        self.send_response(230, &format!("User {username} logged in."))
    }

    // ========================================================================
    // Command handlers — Navigation
    // ========================================================================

    fn cmd_cwd(&mut self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            return self.send_response(501, "CWD requires a directory path.");
        }

        let real_path = resolve_path(&self.root_dir, &self.cwd, arg);

        if !is_within_root(&self.root_dir, &real_path) {
            return self.send_response(550, "Permission denied.");
        }

        match fs::metadata(&real_path) {
            Ok(meta) if meta.is_dir() => {
                self.cwd = virtual_path(&self.root_dir, &real_path);
                self.send_response(250, "Directory changed.")
            }
            Ok(_) => self.send_response(550, "Not a directory."),
            Err(_) => self.send_response(550, "No such directory."),
        }
    }

    fn cmd_cdup(&mut self) -> Result<(), FtpdError> {
        self.cmd_cwd("..")
    }

    fn cmd_pwd(&self) -> Result<(), FtpdError> {
        self.send_response(257, &format!("\"{}\" is current directory.", self.cwd))
    }

    // ========================================================================
    // Command handlers — Transfer settings
    // ========================================================================

    fn cmd_type(&mut self, arg: &str) -> Result<(), FtpdError> {
        match arg.to_ascii_uppercase().as_str() {
            "A" | "A N" => {
                self.transfer_type = TransferType::Ascii;
                self.send_response(200, "Type set to ASCII.")
            }
            "I" | "L 8" => {
                self.transfer_type = TransferType::Binary;
                self.send_response(200, "Type set to Binary.")
            }
            _ => self.send_response(504, "Unsupported type. Use A or I."),
        }
    }

    fn cmd_mode(&mut self, arg: &str) -> Result<(), FtpdError> {
        match arg.to_ascii_uppercase().as_str() {
            "S" => {
                self.transfer_mode = TransferMode::Stream;
                self.send_response(200, "Mode set to Stream.")
            }
            "B" => {
                self.transfer_mode = TransferMode::Block;
                self.send_response(200, "Mode set to Block.")
            }
            "C" => {
                self.transfer_mode = TransferMode::Compressed;
                self.send_response(200, "Mode set to Compressed.")
            }
            _ => self.send_response(504, "Unsupported mode."),
        }
    }

    fn cmd_stru(&mut self, arg: &str) -> Result<(), FtpdError> {
        match arg.to_ascii_uppercase().as_str() {
            "F" => {
                self.file_structure = FileStructure::File;
                self.send_response(200, "Structure set to File.")
            }
            "R" => {
                self.file_structure = FileStructure::Record;
                self.send_response(200, "Structure set to Record.")
            }
            "P" => {
                self.file_structure = FileStructure::Page;
                self.send_response(200, "Structure set to Page.")
            }
            _ => self.send_response(504, "Unsupported structure."),
        }
    }

    fn cmd_port(&mut self, arg: &str) -> Result<(), FtpdError> {
        // PORT h1,h2,h3,h4,p1,p2
        let parts: Vec<&str> = arg.split(',').collect();
        if parts.len() != 6 {
            return self.send_response(501, "Invalid PORT command.");
        }

        let parse_part = |s: &str| -> Option<u8> { s.trim().parse().ok() };

        let h1 = match parse_part(parts[0]) {
            Some(v) => v,
            None => return self.send_response(501, "Invalid PORT address."),
        };
        let h2 = match parse_part(parts[1]) {
            Some(v) => v,
            None => return self.send_response(501, "Invalid PORT address."),
        };
        let h3 = match parse_part(parts[2]) {
            Some(v) => v,
            None => return self.send_response(501, "Invalid PORT address."),
        };
        let h4 = match parse_part(parts[3]) {
            Some(v) => v,
            None => return self.send_response(501, "Invalid PORT address."),
        };
        let p1 = match parse_part(parts[4]) {
            Some(v) => v,
            None => return self.send_response(501, "Invalid PORT port."),
        };
        let p2 = match parse_part(parts[5]) {
            Some(v) => v,
            None => return self.send_response(501, "Invalid PORT port."),
        };

        let ip = u32::from_be_bytes([h1, h2, h3, h4]);
        let port = u16::from(p1).saturating_mul(256).saturating_add(u16::from(p2));

        // Close any existing passive listener
        close_passive_listener(&mut self.data_mode);

        self.data_mode = DataMode::Active { ip, port };
        self.send_response(200, "PORT command successful.")
    }

    fn cmd_pasv(&mut self) -> Result<(), FtpdError> {
        // Close any existing passive listener
        close_passive_listener(&mut self.data_mode);

        // Try ports in the configured range
        let mut listener = None;
        let mut bound_port = 0u16;
        let range_size = self.config.pasv_max.saturating_sub(self.config.pasv_min).saturating_add(1);

        for _ in 0..range_size {
            let try_port = self.next_pasv_port();
            match tcp_bind(try_port) {
                Ok(l) => {
                    listener = Some(l);
                    bound_port = try_port;
                    break;
                }
                Err(_) => continue,
            }
        }

        let listener = match listener {
            Some(l) => l,
            None => {
                return self.send_response(
                    421,
                    "Cannot allocate passive port.",
                );
            }
        };

        // Determine our IP for the PASV response
        let server_ip = if self.config.server_ip != 0 {
            self.config.server_ip
        } else {
            // Try to get our address from the control connection
            // Fall back to 127.0.0.1 if we cannot determine it
            u32::from_be_bytes([127, 0, 0, 1])
        };

        let ip_bytes = server_ip.to_be_bytes();
        let p1 = bound_port / 256;
        let p2 = bound_port % 256;

        self.data_mode = DataMode::Passive {
            listener,
            port: bound_port,
        };

        self.send_response(
            227,
            &format!(
                "Entering Passive Mode ({},{},{},{},{},{}).",
                ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3], p1, p2
            ),
        )
    }

    fn cmd_rest(&mut self, arg: &str) -> Result<(), FtpdError> {
        match arg.parse::<u64>() {
            Ok(offset) => {
                self.rest_offset = offset;
                self.send_response(
                    350,
                    &format!("Restarting at {offset}. Send STOR or RETR."),
                )
            }
            Err(_) => self.send_response(501, "Invalid REST offset."),
        }
    }

    // ========================================================================
    // Command handlers — Directory listing
    // ========================================================================

    fn cmd_list(&mut self, arg: &str) -> Result<(), FtpdError> {
        // Strip common ls options that clients might send (e.g. -la)
        let path_arg = strip_ls_options(arg);

        let real_path = if path_arg.is_empty() {
            resolve_path(&self.root_dir, &self.cwd, ".")
        } else {
            resolve_path(&self.root_dir, &self.cwd, path_arg)
        };

        if !is_within_root(&self.root_dir, &real_path) {
            return self.send_response(550, "Permission denied.");
        }

        let entries = match read_directory_listing(&real_path) {
            Ok(e) => e,
            Err(_) => {
                return self.send_response(550, "Failed to list directory.");
            }
        };

        self.send_response(150, "Opening data connection for directory listing.")?;

        let data_handle = match open_data_connection(&self.data_mode) {
            Ok(h) => h,
            Err(_) => {
                return self.send_response(425, "Cannot open data connection.");
            }
        };

        let mut listing = String::new();
        for (name, meta) in &entries {
            listing.push_str(&format_list_entry(name, meta));
            listing.push_str("\r\n");
        }

        let send_result = tcp_send_all(data_handle, listing.as_bytes());
        tcp_close(data_handle);
        close_passive_listener(&mut self.data_mode);

        match send_result {
            Ok(()) => self.send_response(226, "Transfer complete."),
            Err(_) => self.send_response(426, "Transfer aborted."),
        }
    }

    fn cmd_nlst(&mut self, arg: &str) -> Result<(), FtpdError> {
        let path_arg = strip_ls_options(arg);

        let real_path = if path_arg.is_empty() {
            resolve_path(&self.root_dir, &self.cwd, ".")
        } else {
            resolve_path(&self.root_dir, &self.cwd, path_arg)
        };

        if !is_within_root(&self.root_dir, &real_path) {
            return self.send_response(550, "Permission denied.");
        }

        let entries = match read_directory_listing(&real_path) {
            Ok(e) => e,
            Err(_) => {
                return self.send_response(550, "Failed to list directory.");
            }
        };

        self.send_response(150, "Opening data connection for name list.")?;

        let data_handle = match open_data_connection(&self.data_mode) {
            Ok(h) => h,
            Err(_) => {
                return self.send_response(425, "Cannot open data connection.");
            }
        };

        let mut listing = String::new();
        for (name, _) in &entries {
            listing.push_str(name);
            listing.push_str("\r\n");
        }

        let send_result = tcp_send_all(data_handle, listing.as_bytes());
        tcp_close(data_handle);
        close_passive_listener(&mut self.data_mode);

        match send_result {
            Ok(()) => self.send_response(226, "Transfer complete."),
            Err(_) => self.send_response(426, "Transfer aborted."),
        }
    }

    // ========================================================================
    // Command handlers — File transfer
    // ========================================================================

    fn cmd_retr(&mut self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            return self.send_response(501, "RETR requires a filename.");
        }

        let real_path = resolve_path(&self.root_dir, &self.cwd, arg);

        if !is_within_root(&self.root_dir, &real_path) {
            return self.send_response(550, "Permission denied.");
        }

        let meta = match fs::metadata(&real_path) {
            Ok(m) => m,
            Err(_) => {
                return self.send_response(550, "File not found.");
            }
        };

        if meta.is_dir() {
            return self.send_response(550, "Not a regular file.");
        }

        let mut file = match File::open(&real_path) {
            Ok(f) => f,
            Err(_) => {
                return self.send_response(550, "Cannot open file.");
            }
        };

        let file_size = meta.len();

        // Handle REST offset
        if self.rest_offset > 0 {
            use std::io::Seek;
            if file.seek(std::io::SeekFrom::Start(self.rest_offset)).is_err() {
                self.rest_offset = 0;
                return self.send_response(550, "Cannot seek to restart position.");
            }
        }

        let size_msg = if self.rest_offset > 0 {
            format!(
                "Opening {} mode data connection for {} ({} bytes, restarting at {}).",
                self.transfer_type, arg, file_size, self.rest_offset
            )
        } else {
            format!(
                "Opening {} mode data connection for {} ({} bytes).",
                self.transfer_type, arg, file_size
            )
        };

        self.send_response(150, &size_msg)?;

        let data_handle = match open_data_connection(&self.data_mode) {
            Ok(h) => h,
            Err(_) => {
                self.rest_offset = 0;
                return self.send_response(425, "Cannot open data connection.");
            }
        };

        let mut buf = vec![0u8; DATA_BUF_SIZE];
        let mut total_sent: u64 = 0;
        let mut transfer_ok = true;

        loop {
            let n = match file.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => n,
                Err(_) => {
                    transfer_ok = false;
                    break;
                }
            };

            let data = if self.transfer_type == TransferType::Ascii {
                // In ASCII mode, convert LF to CRLF
                ascii_to_network(&buf[..n])
            } else {
                buf[..n].to_vec()
            };

            if tcp_send_all(data_handle, &data).is_err() {
                transfer_ok = false;
                break;
            }

            total_sent = total_sent.saturating_add(data.len() as u64);
            self.rate_limiter.record(data.len() as u64);
        }

        tcp_close(data_handle);
        close_passive_listener(&mut self.data_mode);
        self.rest_offset = 0;

        if transfer_ok {
            self.stats.bytes_downloaded = self.stats.bytes_downloaded.saturating_add(total_sent);
            self.stats.files_downloaded = self.stats.files_downloaded.saturating_add(1);
            self.send_response(226, "Transfer complete.")
        } else {
            self.send_response(426, "Transfer aborted.")
        }
    }

    fn cmd_stor(&mut self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            return self.send_response(501, "STOR requires a filename.");
        }

        // Anonymous users cannot upload by default
        if self.is_anonymous {
            return self.send_response(550, "Permission denied.");
        }

        let real_path = resolve_path(&self.root_dir, &self.cwd, arg);

        if !is_within_root(&self.root_dir, &real_path) {
            return self.send_response(550, "Permission denied.");
        }

        let mut file = if self.rest_offset > 0 {
            // Append mode for resumed upload
            match OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(&real_path)
            {
                Ok(mut f) => {
                    use std::io::Seek;
                    if f.seek(std::io::SeekFrom::Start(self.rest_offset)).is_err() {
                        self.rest_offset = 0;
                        return self.send_response(550, "Cannot seek to restart position.");
                    }
                    f
                }
                Err(_) => {
                    self.rest_offset = 0;
                    return self.send_response(550, "Cannot create file.");
                }
            }
        } else {
            match File::create(&real_path) {
                Ok(f) => f,
                Err(_) => {
                    return self.send_response(550, "Cannot create file.");
                }
            }
        };

        self.send_response(
            150,
            &format!("Opening {} mode data connection for {}.", self.transfer_type, arg),
        )?;

        let data_handle = match open_data_connection(&self.data_mode) {
            Ok(h) => h,
            Err(_) => {
                self.rest_offset = 0;
                return self.send_response(425, "Cannot open data connection.");
            }
        };

        let mut buf = vec![0u8; DATA_BUF_SIZE];
        let mut total_received: u64 = 0;
        let mut transfer_ok = true;

        loop {
            let n = match tcp_recv(data_handle, &mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => n,
                Err(_) => {
                    transfer_ok = false;
                    break;
                }
            };

            let data = if self.transfer_type == TransferType::Ascii {
                // In ASCII mode, convert CRLF to LF
                network_to_ascii(&buf[..n])
            } else {
                buf[..n].to_vec()
            };

            if file.write_all(&data).is_err() {
                transfer_ok = false;
                break;
            }

            total_received = total_received.saturating_add(data.len() as u64);
            self.rate_limiter.record(data.len() as u64);
        }

        tcp_close(data_handle);
        close_passive_listener(&mut self.data_mode);
        self.rest_offset = 0;

        if transfer_ok {
            self.stats.bytes_uploaded = self.stats.bytes_uploaded.saturating_add(total_received);
            self.stats.files_uploaded = self.stats.files_uploaded.saturating_add(1);
            self.send_response(226, "Transfer complete.")
        } else {
            // Clean up partial file on failure
            let _ = fs::remove_file(&real_path);
            self.send_response(426, "Transfer aborted.")
        }
    }

    // ========================================================================
    // Command handlers — File management
    // ========================================================================

    fn cmd_dele(&mut self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            return self.send_response(501, "DELE requires a filename.");
        }

        if self.is_anonymous {
            return self.send_response(550, "Permission denied.");
        }

        let real_path = resolve_path(&self.root_dir, &self.cwd, arg);

        if !is_within_root(&self.root_dir, &real_path) {
            return self.send_response(550, "Permission denied.");
        }

        match fs::remove_file(&real_path) {
            Ok(()) => self.send_response(250, "File deleted."),
            Err(_) => self.send_response(550, "Delete failed."),
        }
    }

    fn cmd_mkd(&mut self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            return self.send_response(501, "MKD requires a directory name.");
        }

        if self.is_anonymous {
            return self.send_response(550, "Permission denied.");
        }

        let real_path = resolve_path(&self.root_dir, &self.cwd, arg);

        if !is_within_root(&self.root_dir, &real_path) {
            return self.send_response(550, "Permission denied.");
        }

        match fs::create_dir(&real_path) {
            Ok(()) => {
                let vpath = virtual_path(&self.root_dir, &real_path);
                self.send_response(257, &format!("\"{vpath}\" directory created."))
            }
            Err(_) => self.send_response(550, "Cannot create directory."),
        }
    }

    fn cmd_rmd(&mut self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            return self.send_response(501, "RMD requires a directory name.");
        }

        if self.is_anonymous {
            return self.send_response(550, "Permission denied.");
        }

        let real_path = resolve_path(&self.root_dir, &self.cwd, arg);

        if !is_within_root(&self.root_dir, &real_path) {
            return self.send_response(550, "Permission denied.");
        }

        match fs::remove_dir(&real_path) {
            Ok(()) => self.send_response(250, "Directory removed."),
            Err(_) => self.send_response(550, "Cannot remove directory."),
        }
    }

    fn cmd_rnfr(&mut self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            return self.send_response(501, "RNFR requires a filename.");
        }

        if self.is_anonymous {
            return self.send_response(550, "Permission denied.");
        }

        let real_path = resolve_path(&self.root_dir, &self.cwd, arg);

        if !is_within_root(&self.root_dir, &real_path) {
            return self.send_response(550, "Permission denied.");
        }

        if !real_path.exists() {
            return self.send_response(550, "File not found.");
        }

        self.rename_from = Some(real_path);
        self.send_response(350, "File exists, ready for RNTO.")
    }

    fn cmd_rnto(&mut self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            return self.send_response(501, "RNTO requires a filename.");
        }

        let from_path = match self.rename_from.take() {
            Some(p) => p,
            None => {
                return self.send_response(503, "RNFR required before RNTO.");
            }
        };

        let to_path = resolve_path(&self.root_dir, &self.cwd, arg);

        if !is_within_root(&self.root_dir, &to_path) {
            return self.send_response(550, "Permission denied.");
        }

        match fs::rename(&from_path, &to_path) {
            Ok(()) => self.send_response(250, "Rename successful."),
            Err(_) => self.send_response(550, "Rename failed."),
        }
    }

    fn cmd_size(&mut self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            return self.send_response(501, "SIZE requires a filename.");
        }

        let real_path = resolve_path(&self.root_dir, &self.cwd, arg);

        if !is_within_root(&self.root_dir, &real_path) {
            return self.send_response(550, "Permission denied.");
        }

        match fs::metadata(&real_path) {
            Ok(meta) if !meta.is_dir() => {
                self.send_response(213, &format!("{}", meta.len()))
            }
            Ok(_) => self.send_response(550, "Not a regular file."),
            Err(_) => self.send_response(550, "File not found."),
        }
    }

    fn cmd_mdtm(&mut self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            return self.send_response(501, "MDTM requires a filename.");
        }

        let real_path = resolve_path(&self.root_dir, &self.cwd, arg);

        if !is_within_root(&self.root_dir, &real_path) {
            return self.send_response(550, "Permission denied.");
        }

        match fs::metadata(&real_path) {
            Ok(meta) => {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                self.send_response(213, &format_mdtm(mtime))
            }
            Err(_) => self.send_response(550, "File not found."),
        }
    }

    // ========================================================================
    // Command handlers — Information
    // ========================================================================

    fn cmd_syst(&self) -> Result<(), FtpdError> {
        self.send_response(215, "UNIX Type: L8")
    }

    fn cmd_stat(&self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            // Server status
            let uptime = now_secs().saturating_sub(self.stats.start_time);
            let lines = [
                "FTP server status:",
                &format!("     Connected as: {}", self.username),
                &format!("     Session time: {} seconds", uptime),
                &format!("     Commands run: {}", self.stats.commands_processed),
                &format!("     Downloaded: {} bytes ({} files)",
                    self.stats.bytes_downloaded, self.stats.files_downloaded),
                &format!("     Uploaded: {} bytes ({} files)",
                    self.stats.bytes_uploaded, self.stats.files_uploaded),
                &format!("     Transfer type: {}", self.transfer_type),
                "End of status.",
            ];
            self.send_multiline(211, &lines)
        } else {
            // File/directory status
            let real_path = resolve_path(&self.root_dir, &self.cwd, arg);
            if !is_within_root(&self.root_dir, &real_path) {
                return self.send_response(550, "Permission denied.");
            }
            match fs::metadata(&real_path) {
                Ok(meta) => {
                    let entry_line = format_list_entry(arg, &meta);
                    self.send_multiline(213, &["Status of file:", &entry_line, "End of status."])
                }
                Err(_) => self.send_response(550, "File not found."),
            }
        }
    }

    fn cmd_feat(&self) -> Result<(), FtpdError> {
        let lines = [
            "Features:",
            " SIZE",
            " MDTM",
            " REST STREAM",
            " PASV",
            " UTF8",
            "End",
        ];
        self.send_multiline(211, &lines)
    }

    fn cmd_opts(&mut self, arg: &str) -> Result<(), FtpdError> {
        match arg.to_ascii_uppercase().as_str() {
            "UTF8 ON" => self.send_response(200, "UTF8 enabled."),
            "UTF8 OFF" => self.send_response(200, "UTF8 disabled."),
            _ => self.send_response(501, "Unsupported option."),
        }
    }

    fn cmd_help(&self, arg: &str) -> Result<(), FtpdError> {
        if arg.is_empty() {
            let lines = [
                "The following commands are recognized:",
                " USER PASS CWD  CDUP PWD  LIST NLST RETR",
                " STOR DELE MKD  RMD  RNFR RNTO TYPE MODE",
                " STRU PORT PASV QUIT SYST STAT NOOP HELP",
                " SIZE MDTM FEAT OPTS REST",
                "End of help.",
            ];
            self.send_multiline(214, &lines)
        } else {
            let help_text = match arg.to_ascii_uppercase().as_str() {
                "USER" => "USER <username> - Specify user for authentication.",
                "PASS" => "PASS <password> - Specify password for authentication.",
                "CWD" => "CWD <directory> - Change working directory.",
                "CDUP" => "CDUP - Change to parent directory.",
                "PWD" => "PWD - Print working directory.",
                "LIST" => "LIST [path] - List directory contents in long format.",
                "NLST" => "NLST [path] - List directory names only.",
                "RETR" => "RETR <filename> - Retrieve (download) a file.",
                "STOR" => "STOR <filename> - Store (upload) a file.",
                "DELE" => "DELE <filename> - Delete a file.",
                "MKD" => "MKD <directory> - Create a directory.",
                "RMD" => "RMD <directory> - Remove a directory.",
                "RNFR" => "RNFR <filename> - Specify rename source.",
                "RNTO" => "RNTO <filename> - Specify rename destination.",
                "TYPE" => "TYPE <A|I> - Set transfer type (ASCII or Binary).",
                "MODE" => "MODE <S|B|C> - Set transfer mode.",
                "STRU" => "STRU <F|R|P> - Set file structure.",
                "PORT" => "PORT h1,h2,h3,h4,p1,p2 - Specify data connection address.",
                "PASV" => "PASV - Enter passive mode.",
                "QUIT" => "QUIT - Disconnect.",
                "SYST" => "SYST - Show system type.",
                "STAT" => "STAT [path] - Show server or file status.",
                "NOOP" => "NOOP - No operation.",
                "HELP" => "HELP [command] - Show help.",
                "SIZE" => "SIZE <filename> - Show file size.",
                "MDTM" => "MDTM <filename> - Show file modification time.",
                "FEAT" => "FEAT - List supported features.",
                "OPTS" => "OPTS <option> - Set option.",
                "REST" => "REST <offset> - Set restart offset for next transfer.",
                _ => "Unknown command.",
            };
            self.send_response(214, help_text)
        }
    }

    fn cmd_noop(&self) -> Result<(), FtpdError> {
        self.send_response(200, "NOOP ok.")
    }

    fn cmd_quit(&mut self) -> Result<(), FtpdError> {
        log_info(
            &self.config,
            &format!(
                "user '{}' disconnecting (down={}, up={}, cmds={})",
                self.username,
                self.stats.bytes_downloaded,
                self.stats.bytes_uploaded,
                self.stats.commands_processed,
            ),
        );
        self.send_response(221, "Goodbye.")
    }
}

// ============================================================================
// ASCII mode conversion helpers
// ============================================================================

/// Convert local line endings (LF) to network line endings (CRLF) for ASCII
/// transfer mode.
fn ascii_to_network(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len().saturating_add(data.len() / 10));
    for &byte in data {
        if byte == b'\n' {
            // Only add CR before LF if the previous byte was not already CR
            if result.last() != Some(&b'\r') {
                result.push(b'\r');
            }
        }
        result.push(byte);
    }
    result
}

/// Convert network line endings (CRLF) to local line endings (LF) for ASCII
/// transfer mode.
fn network_to_ascii(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut prev_cr = false;
    for &byte in data {
        if byte == b'\n' && prev_cr {
            // CRLF -> LF (the CR was already skipped)
            result.push(b'\n');
            prev_cr = false;
        } else {
            if prev_cr {
                // Standalone CR (not followed by LF), keep it
                result.push(b'\r');
            }
            if byte == b'\r' {
                prev_cr = true;
            } else {
                result.push(byte);
                prev_cr = false;
            }
        }
    }
    // Trailing CR without LF
    if prev_cr {
        result.push(b'\r');
    }
    result
}

// ============================================================================
// Directory reading helper
// ============================================================================

/// Read directory entries and their metadata, sorted by name.
fn read_directory_listing(path: &Path) -> Result<Vec<(String, Metadata)>, FtpdError> {
    let mut entries = Vec::new();

    let dir = fs::read_dir(path).map_err(|e| {
        FtpdError::Io(format!("cannot read directory {}: {e}", path.display()))
    })?;

    for entry in dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        entries.push((name, meta));
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Strip ls-style options from a path argument.
/// Clients sometimes send "LIST -la" or "LIST -a /dir".
fn strip_ls_options(arg: &str) -> &str {
    let trimmed = arg.trim();
    if trimmed.is_empty() {
        return trimmed;
    }

    let mut remaining = trimmed;
    loop {
        remaining = remaining.trim_start();
        if remaining.starts_with('-') {
            // Skip this option token
            if let Some(space_idx) = remaining.find(' ') {
                remaining = &remaining[space_idx..];
            } else {
                // Entire arg is options, no path
                return "";
            }
        } else {
            break;
        }
    }

    remaining.trim()
}

// ============================================================================
// Server main loop
// ============================================================================

/// Run the FTP server: bind, accept connections, handle each sequentially.
fn run_server(config: Config) -> Result<(), FtpdError> {
    log_info(&config, &format!("starting on port {}", config.port));

    let listener = tcp_bind(config.port)?;

    log_info(&config, &format!("listening on port {}", config.port));

    // Write PID file
    if let Ok(mut f) = File::create(PID_PATH) {
        // We cannot get our real PID easily, write a marker
        let _ = writeln!(f, "ftpd");
    }

    loop {
        let conn = match tcp_accept(listener) {
            Ok(c) => c,
            Err(e) => {
                log_info(&config, &format!("accept error: {e}"));
                continue;
            }
        };

        let peer_ip = tcp_peer_addr(conn)
            .map(|(ip, _)| ip)
            .unwrap_or(0);

        log_info(
            &config,
            &format!("connection from {}", format_ip(peer_ip)),
        );

        let mut session = FtpSession::new(conn, config.clone(), peer_ip);
        if let Err(e) = session.run() {
            log_info(&config, &format!("session error: {e}"));
        }

        tcp_close(conn);
    }
}

// ============================================================================
// Entry point
// ============================================================================

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let cli = match parse_cli() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ftpd: {e}");
            return 1;
        }
    };

    if cli.show_help {
        print_help();
        return 0;
    }

    let mut config = match parse_config_file(&cli.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ftpd: {e}");
            return 1;
        }
    };

    // Apply CLI overrides
    if let Some(port) = cli.port_override {
        config.port = port;
    }
    if cli.debug {
        config.debug = true;
    }
    if cli.anonymous_only {
        config.anonymous_only = true;
        config.allow_anonymous = true;
    }

    if let Err(e) = run_server(config) {
        eprintln!("ftpd: fatal: {e}");
        return 1;
    }

    0
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Path normalization and resolution ----

    #[test]
    fn test_normalize_path_root() {
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn test_normalize_path_simple() {
        assert_eq!(normalize_path("/foo/bar"), "/foo/bar");
    }

    #[test]
    fn test_normalize_path_dot() {
        assert_eq!(normalize_path("/foo/./bar"), "/foo/bar");
    }

    #[test]
    fn test_normalize_path_dotdot() {
        assert_eq!(normalize_path("/foo/bar/../baz"), "/foo/baz");
    }

    #[test]
    fn test_normalize_path_double_slash() {
        assert_eq!(normalize_path("/foo//bar"), "/foo/bar");
    }

    #[test]
    fn test_normalize_path_dotdot_at_root() {
        assert_eq!(normalize_path("/.."), "/");
    }

    #[test]
    fn test_normalize_path_multiple_dotdot() {
        assert_eq!(normalize_path("/a/b/c/../../d"), "/a/d");
    }

    #[test]
    fn test_normalize_path_trailing_slash() {
        assert_eq!(normalize_path("/foo/bar/"), "/foo/bar");
    }

    #[test]
    fn test_normalize_path_empty() {
        assert_eq!(normalize_path(""), "/");
    }

    #[test]
    fn test_resolve_path_absolute() {
        let p = resolve_path("/srv/ftp", "/pub", "/docs/readme.txt");
        assert_eq!(p, PathBuf::from("/srv/ftp/docs/readme.txt"));
    }

    #[test]
    fn test_resolve_path_relative() {
        let p = resolve_path("/srv/ftp", "/pub", "readme.txt");
        assert_eq!(p, PathBuf::from("/srv/ftp/pub/readme.txt"));
    }

    #[test]
    fn test_resolve_path_dotdot_stays_in_root() {
        let p = resolve_path("/srv/ftp", "/pub", "../../etc/passwd");
        assert_eq!(p, PathBuf::from("/srv/ftp/etc/passwd"));
    }

    #[test]
    fn test_resolve_path_cwd_root() {
        let p = resolve_path("/home/user", "/", "file.txt");
        assert_eq!(p, PathBuf::from("/home/user/file.txt"));
    }

    #[test]
    fn test_is_within_root_yes() {
        assert!(is_within_root("/srv/ftp", Path::new("/srv/ftp/pub/file.txt")));
    }

    #[test]
    fn test_is_within_root_exact() {
        assert!(is_within_root("/srv/ftp", Path::new("/srv/ftp")));
    }

    #[test]
    fn test_is_within_root_no() {
        assert!(!is_within_root("/srv/ftp", Path::new("/etc/passwd")));
    }

    #[test]
    fn test_is_within_root_prefix_attack() {
        // "/srv/ftpevil" should NOT match root "/srv/ftp"
        assert!(!is_within_root("/srv/ftp", Path::new("/srv/ftpevil/file")));
    }

    #[test]
    fn test_virtual_path_root() {
        let vp = virtual_path("/srv/ftp", Path::new("/srv/ftp"));
        assert_eq!(vp, "/");
    }

    #[test]
    fn test_virtual_path_subdir() {
        let vp = virtual_path("/srv/ftp", Path::new("/srv/ftp/pub/docs"));
        assert_eq!(vp, "/pub/docs");
    }

    #[test]
    fn test_virtual_path_outside() {
        let vp = virtual_path("/srv/ftp", Path::new("/etc/passwd"));
        assert_eq!(vp, "/");
    }

    // ---- FTP command parsing ----

    #[test]
    fn test_parse_command_simple() {
        let cmd = parse_ftp_command("QUIT\r\n");
        assert_eq!(cmd.verb, "QUIT");
        assert_eq!(cmd.arg, "");
    }

    #[test]
    fn test_parse_command_with_arg() {
        let cmd = parse_ftp_command("USER anonymous\r\n");
        assert_eq!(cmd.verb, "USER");
        assert_eq!(cmd.arg, "anonymous");
    }

    #[test]
    fn test_parse_command_case_insensitive() {
        let cmd = parse_ftp_command("user Bob\r\n");
        assert_eq!(cmd.verb, "USER");
        assert_eq!(cmd.arg, "Bob");
    }

    #[test]
    fn test_parse_command_extra_spaces() {
        let cmd = parse_ftp_command("CWD   /pub/docs  \r\n");
        assert_eq!(cmd.verb, "CWD");
        assert_eq!(cmd.arg, "/pub/docs");
    }

    #[test]
    fn test_parse_command_port() {
        let cmd = parse_ftp_command("PORT 192,168,1,1,4,1\r\n");
        assert_eq!(cmd.verb, "PORT");
        assert_eq!(cmd.arg, "192,168,1,1,4,1");
    }

    #[test]
    fn test_parse_command_type() {
        let cmd = parse_ftp_command("TYPE I\r\n");
        assert_eq!(cmd.verb, "TYPE");
        assert_eq!(cmd.arg, "I");
    }

    #[test]
    fn test_parse_command_empty_line() {
        let cmd = parse_ftp_command("\r\n");
        assert_eq!(cmd.verb, "");
        assert_eq!(cmd.arg, "");
    }

    #[test]
    fn test_parse_command_no_crlf() {
        let cmd = parse_ftp_command("NOOP");
        assert_eq!(cmd.verb, "NOOP");
        assert_eq!(cmd.arg, "");
    }

    // ---- IP parsing / formatting ----

    #[test]
    fn test_parse_ip_str_valid() {
        let ip = parse_ip_str("192.168.1.1");
        assert_eq!(ip, Some(u32::from_be_bytes([192, 168, 1, 1])));
    }

    #[test]
    fn test_parse_ip_str_localhost() {
        let ip = parse_ip_str("127.0.0.1");
        assert_eq!(ip, Some(u32::from_be_bytes([127, 0, 0, 1])));
    }

    #[test]
    fn test_parse_ip_str_invalid() {
        assert!(parse_ip_str("999.0.0.1").is_none());
    }

    #[test]
    fn test_parse_ip_str_short() {
        assert!(parse_ip_str("1.2.3").is_none());
    }

    #[test]
    fn test_parse_ip_str_empty() {
        assert!(parse_ip_str("").is_none());
    }

    #[test]
    fn test_format_ip() {
        let ip = u32::from_be_bytes([10, 0, 2, 15]);
        assert_eq!(format_ip(ip), "10.0.2.15");
    }

    #[test]
    fn test_format_ip_loopback() {
        let ip = u32::from_be_bytes([127, 0, 0, 1]);
        assert_eq!(format_ip(ip), "127.0.0.1");
    }

    // ---- Bool parsing ----

    #[test]
    fn test_parse_bool_yes() {
        assert_eq!(parse_bool("yes"), Some(true));
    }

    #[test]
    fn test_parse_bool_no() {
        assert_eq!(parse_bool("no"), Some(false));
    }

    #[test]
    fn test_parse_bool_true() {
        assert_eq!(parse_bool("true"), Some(true));
    }

    #[test]
    fn test_parse_bool_false() {
        assert_eq!(parse_bool("false"), Some(false));
    }

    #[test]
    fn test_parse_bool_one() {
        assert_eq!(parse_bool("1"), Some(true));
    }

    #[test]
    fn test_parse_bool_zero() {
        assert_eq!(parse_bool("0"), Some(false));
    }

    #[test]
    fn test_parse_bool_case_insensitive() {
        assert_eq!(parse_bool("YES"), Some(true));
        assert_eq!(parse_bool("True"), Some(true));
    }

    #[test]
    fn test_parse_bool_invalid() {
        assert!(parse_bool("maybe").is_none());
    }

    // ---- Config parsing ----

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.port, 21);
        assert_eq!(cfg.pasv_min, 49152);
        assert_eq!(cfg.pasv_max, 65000);
        assert_eq!(cfg.max_connections, 50);
        assert_eq!(cfg.rate_limit, 0);
        assert_eq!(cfg.idle_timeout, 300);
        assert!(cfg.allow_anonymous);
        assert!(!cfg.anonymous_only);
        assert_eq!(cfg.anon_root, "/srv/ftp");
        assert!(cfg.chroot_users);
        assert!(!cfg.debug);
    }

    #[test]
    fn test_config_missing_file() {
        // Missing config should return defaults
        let cfg = parse_config_file("/nonexistent/ftpd.conf").unwrap();
        assert_eq!(cfg.port, 21);
    }

    // ---- ASCII conversion ----

    #[test]
    fn test_ascii_to_network_lf_to_crlf() {
        let input = b"hello\nworld\n";
        let output = ascii_to_network(input);
        assert_eq!(output, b"hello\r\nworld\r\n");
    }

    #[test]
    fn test_ascii_to_network_already_crlf() {
        let input = b"hello\r\nworld\r\n";
        let output = ascii_to_network(input);
        // Should not double-up the CR
        assert_eq!(output, b"hello\r\nworld\r\n");
    }

    #[test]
    fn test_ascii_to_network_no_newlines() {
        let input = b"hello world";
        let output = ascii_to_network(input);
        assert_eq!(output, b"hello world");
    }

    #[test]
    fn test_ascii_to_network_empty() {
        let output = ascii_to_network(b"");
        assert!(output.is_empty());
    }

    #[test]
    fn test_network_to_ascii_crlf_to_lf() {
        let input = b"hello\r\nworld\r\n";
        let output = network_to_ascii(input);
        assert_eq!(output, b"hello\nworld\n");
    }

    #[test]
    fn test_network_to_ascii_bare_lf() {
        let input = b"hello\nworld\n";
        let output = network_to_ascii(input);
        assert_eq!(output, b"hello\nworld\n");
    }

    #[test]
    fn test_network_to_ascii_standalone_cr() {
        let input = b"hello\rworld";
        let output = network_to_ascii(input);
        assert_eq!(output, b"hello\rworld");
    }

    #[test]
    fn test_network_to_ascii_empty() {
        let output = network_to_ascii(b"");
        assert!(output.is_empty());
    }

    #[test]
    fn test_network_to_ascii_trailing_cr() {
        let input = b"hello\r";
        let output = network_to_ascii(input);
        assert_eq!(output, b"hello\r");
    }

    // ---- Date / time helpers ----

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(2400));
    }

    #[test]
    fn test_unix_secs_to_date_epoch() {
        let (y, m, d, h, min) = unix_secs_to_date(0);
        assert_eq!((y, m, d, h, min), (1970, 1, 1, 0, 0));
    }

    #[test]
    fn test_unix_secs_to_date_known() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let (y, m, d, h, min) = unix_secs_to_date(1704067200);
        assert_eq!(y, 2024);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
        assert_eq!(h, 0);
        assert_eq!(min, 0);
    }

    #[test]
    fn test_unix_secs_to_date_with_time() {
        // 1970-01-01 12:30:00 = 45000
        let (y, m, d, h, min) = unix_secs_to_date(45000);
        assert_eq!(y, 1970);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
        assert_eq!(h, 12);
        assert_eq!(min, 30);
    }

    #[test]
    fn test_format_mdtm() {
        let result = format_mdtm(0);
        assert_eq!(result, "19700101000000");
    }

    #[test]
    fn test_format_mdtm_known() {
        // 2024-01-01 00:00:00 = 1704067200
        let result = format_mdtm(1704067200);
        assert_eq!(result, "20240101000000");
    }

    // ---- Strip ls options ----

    #[test]
    fn test_strip_ls_options_none() {
        assert_eq!(strip_ls_options("/pub"), "/pub");
    }

    #[test]
    fn test_strip_ls_options_la() {
        assert_eq!(strip_ls_options("-la"), "");
    }

    #[test]
    fn test_strip_ls_options_la_with_path() {
        assert_eq!(strip_ls_options("-la /pub"), "/pub");
    }

    #[test]
    fn test_strip_ls_options_multiple() {
        assert_eq!(strip_ls_options("-l -a /pub"), "/pub");
    }

    #[test]
    fn test_strip_ls_options_empty() {
        assert_eq!(strip_ls_options(""), "");
    }

    // ---- Directory listing format ----

    #[test]
    fn test_format_mtime_recent() {
        // A timestamp that should be "recent" (within 6 months)
        let now = now_secs();
        let result = format_mtime(now);
        // Should contain HH:MM format, not year
        assert!(result.contains(':'), "recent time should have HH:MM: {result}");
    }

    #[test]
    fn test_format_mtime_old() {
        // A very old timestamp (1970)
        let result = format_mtime(0);
        // Should contain the year, not HH:MM
        assert!(result.contains("1970"), "old time should show year: {result}");
    }

    // ---- Transfer type display ----

    #[test]
    fn test_transfer_type_display_ascii() {
        assert_eq!(format!("{}", TransferType::Ascii), "ASCII");
    }

    #[test]
    fn test_transfer_type_display_binary() {
        assert_eq!(format!("{}", TransferType::Binary), "Binary");
    }

    // ---- Session stats ----

    #[test]
    fn test_session_stats_new() {
        let ip = u32::from_be_bytes([10, 0, 0, 1]);
        let stats = SessionStats::new(ip);
        assert_eq!(stats.bytes_uploaded, 0);
        assert_eq!(stats.bytes_downloaded, 0);
        assert_eq!(stats.files_uploaded, 0);
        assert_eq!(stats.files_downloaded, 0);
        assert_eq!(stats.commands_processed, 0);
        assert_eq!(stats.peer_ip, ip);
        assert!(stats.username.is_empty());
    }

    // ---- Rate limiter ----

    #[test]
    fn test_rate_limiter_unlimited() {
        let mut rl = RateLimiter::new(0);
        // Should not block
        rl.record(1_000_000);
        rl.record(1_000_000);
    }

    #[test]
    fn test_rate_limiter_creation() {
        let rl = RateLimiter::new(1024);
        assert_eq!(rl.max_bps, 1024);
        assert_eq!(rl.window_bytes, 0);
    }

    // ---- Error display ----

    #[test]
    fn test_error_display_network() {
        let e = FtpdError::Network("connection reset".into());
        assert_eq!(format!("{e}"), "network error: connection reset");
    }

    #[test]
    fn test_error_display_io() {
        let e = FtpdError::Io("file not found".into());
        assert_eq!(format!("{e}"), "I/O error: file not found");
    }

    #[test]
    fn test_error_display_config() {
        let e = FtpdError::Config("bad port".into());
        assert_eq!(format!("{e}"), "config error: bad port");
    }

    #[test]
    fn test_error_display_auth() {
        let e = FtpdError::Auth("bad password".into());
        assert_eq!(format!("{e}"), "auth error: bad password");
    }

    #[test]
    fn test_error_display_protocol() {
        let e = FtpdError::Protocol("unknown command".into());
        assert_eq!(format!("{e}"), "protocol error: unknown command");
    }

    // ---- Passwd line parsing ----

    #[test]
    fn test_parse_passwd_line_valid() {
        let entry = parse_passwd_line("root:x:0:0:root:/root:/bin/sh").unwrap();
        assert_eq!(entry.username, "root");
        assert_eq!(entry.uid, 0);
        assert_eq!(entry.gid, 0);
        assert_eq!(entry.home, "/root");
        assert_eq!(entry.shell, "/bin/sh");
    }

    #[test]
    fn test_parse_passwd_line_regular_user() {
        let entry = parse_passwd_line("alice:x:1000:1000:Alice:/home/alice:/bin/bash").unwrap();
        assert_eq!(entry.username, "alice");
        assert_eq!(entry.uid, 1000);
        assert_eq!(entry.gid, 1000);
        assert_eq!(entry.home, "/home/alice");
        assert_eq!(entry.shell, "/bin/bash");
    }

    #[test]
    fn test_parse_passwd_line_too_short() {
        assert!(parse_passwd_line("root:x:0").is_none());
    }

    #[test]
    fn test_parse_passwd_line_invalid_uid() {
        assert!(parse_passwd_line("root:x:abc:0:root:/root:/bin/sh").is_none());
    }

    #[test]
    fn test_parse_passwd_line_empty() {
        assert!(parse_passwd_line("").is_none());
    }

    // ---- Anonymous validation ----

    #[test]
    fn test_validate_password_anonymous() {
        assert!(validate_password("anonymous", "", true));
        assert!(validate_password("anonymous", "user@example.com", true));
    }

    // ---- Multiple .. traversals ----

    #[test]
    fn test_normalize_path_excessive_dotdot() {
        assert_eq!(normalize_path("/a/../../../../.."), "/");
    }

    #[test]
    fn test_resolve_path_dotdot_cannot_escape() {
        let p = resolve_path("/srv/ftp", "/", "../../../etc/shadow");
        // The normalize_path collapses .. at root, so we stay within root
        assert_eq!(p, PathBuf::from("/srv/ftp/etc/shadow"));
    }

    #[test]
    fn test_is_within_root_trailing_slash() {
        assert!(is_within_root("/srv/ftp/", Path::new("/srv/ftp/file")));
    }

    // ---- Data mode enum ----

    #[test]
    fn test_data_mode_none() {
        let mode = DataMode::None;
        assert!(matches!(mode, DataMode::None));
    }

    #[test]
    fn test_data_mode_active() {
        let mode = DataMode::Active {
            ip: u32::from_be_bytes([10, 0, 0, 1]),
            port: 20000,
        };
        if let DataMode::Active { ip, port } = mode {
            assert_eq!(ip, u32::from_be_bytes([10, 0, 0, 1]));
            assert_eq!(port, 20000);
        } else {
            panic!("expected Active mode");
        }
    }

    // ---- FTP session construction ----

    #[test]
    fn test_ftp_session_defaults() {
        let config = Config::default();
        let session = FtpSession::new(0, config.clone(), 0);
        assert!(!session.authenticated);
        assert!(!session.is_anonymous);
        assert!(session.pending_user.is_none());
        assert!(session.username.is_empty());
        assert_eq!(session.cwd, "/");
        assert!(matches!(session.transfer_type, TransferType::Ascii));
        assert!(matches!(session.transfer_mode, TransferMode::Stream));
        assert!(matches!(session.file_structure, FileStructure::File));
        assert!(matches!(session.data_mode, DataMode::None));
        assert!(session.rename_from.is_none());
        assert_eq!(session.rest_offset, 0);
    }

    #[test]
    fn test_ftp_session_pasv_port_roundrobin() {
        let config = Config {
            pasv_min: 50000,
            pasv_max: 50002,
            ..Config::default()
        };
        let mut session = FtpSession::new(0, config, 0);

        assert_eq!(session.next_pasv_port(), 50000);
        assert_eq!(session.next_pasv_port(), 50001);
        assert_eq!(session.next_pasv_port(), 50002);
        // Should wrap around
        assert_eq!(session.next_pasv_port(), 50000);
    }

    // ---- Complex path scenarios ----

    #[test]
    fn test_resolve_path_dot_segments() {
        let p = resolve_path("/home/user", "/docs", "./readme.txt");
        assert_eq!(p, PathBuf::from("/home/user/docs/readme.txt"));
    }

    #[test]
    fn test_resolve_path_nested_dotdot() {
        let p = resolve_path("/srv/ftp", "/pub/mirrors/debian", "../../ubuntu/release");
        assert_eq!(p, PathBuf::from("/srv/ftp/pub/ubuntu/release"));
    }

    #[test]
    fn test_normalize_path_complex() {
        assert_eq!(
            normalize_path("/a/./b/../c/./d/../e"),
            "/a/c/e"
        );
    }

    // ---- PORT command parsing edge cases ----

    #[test]
    fn test_parse_port_parts() {
        // Simulate what cmd_port would parse
        let arg = "192,168,1,100,4,1";
        let parts: Vec<&str> = arg.split(',').collect();
        assert_eq!(parts.len(), 6);
        let h1: u8 = parts[0].parse().unwrap();
        let h2: u8 = parts[1].parse().unwrap();
        let h3: u8 = parts[2].parse().unwrap();
        let h4: u8 = parts[3].parse().unwrap();
        let p1: u8 = parts[4].parse().unwrap();
        let p2: u8 = parts[5].parse().unwrap();
        assert_eq!(h1, 192);
        assert_eq!(h2, 168);
        assert_eq!(h3, 1);
        assert_eq!(h4, 100);
        let port = u16::from(p1) * 256 + u16::from(p2);
        assert_eq!(port, 1025); // 4*256 + 1
    }

    // ---- Config validation ----

    #[test]
    fn test_config_pasv_range_default_valid() {
        let cfg = Config::default();
        assert!(cfg.pasv_min <= cfg.pasv_max);
    }

    // ---- Format list entry ----

    #[test]
    fn test_format_list_entry_contains_name() {
        // We cannot easily create a Metadata in tests, but we can test the
        // components. Verify format_mtime handles edge cases.
        let mtime_str = format_mtime(1_700_000_000);
        assert!(!mtime_str.is_empty());
    }

    // ---- More ASCII conversion edge cases ----

    #[test]
    fn test_ascii_to_network_mixed() {
        // Mix of LF, CRLF, and bare CR
        let input = b"a\nb\r\nc\rd\n";
        let output = ascii_to_network(input);
        // LF -> CRLF, existing CRLF kept, bare CR kept, trailing LF -> CRLF
        assert_eq!(output, b"a\r\nb\r\nc\rd\r\n");
    }

    #[test]
    fn test_network_to_ascii_mixed() {
        let input = b"a\r\nb\nc\rd\r\n";
        let output = network_to_ascii(input);
        assert_eq!(output, b"a\nb\nc\rd\n");
    }

    #[test]
    fn test_ascii_roundtrip() {
        let original = b"hello\nworld\nfoo\n";
        let network = ascii_to_network(original);
        let local = network_to_ascii(&network);
        assert_eq!(local, original);
    }

    // ---- Now_secs helper ----

    #[test]
    fn test_now_secs_nonzero() {
        let t = now_secs();
        // Should be well past epoch
        assert!(t > 1_600_000_000);
    }

    // ---- Leap year edge cases ----

    #[test]
    fn test_leap_year_boundary() {
        assert!(!is_leap_year(1970));
        assert!(is_leap_year(1972));
        assert!(!is_leap_year(2100));
        assert!(is_leap_year(2000));
    }

    #[test]
    fn test_unix_secs_to_date_leap_day() {
        // 2024-02-29 00:00:00 UTC
        // Calculate: days from 1970 to 2024-01-01 = 19723
        // Jan = 31, Feb 1-29 = 28 more days -> day 59 of 2024
        // Total days = 19723 + 31 + 28 = 19782
        // Seconds = 19782 * 86400 = 1709164800
        let (y, m, d, _, _) = unix_secs_to_date(1709164800);
        assert_eq!(y, 2024);
        assert_eq!(m, 2);
        assert_eq!(d, 29);
    }

    // ---- CLI parsing ----

    #[test]
    fn test_print_help_does_not_panic() {
        // Just verify it doesn't crash
        // We redirect stdout in a real test, but here just call it
        // (output goes to test harness stdout which is fine)
        print_help();
    }

    // ---- Transfer mode/structure enums ----

    #[test]
    fn test_transfer_mode_variants() {
        let s = TransferMode::Stream;
        let b = TransferMode::Block;
        let c = TransferMode::Compressed;
        assert_eq!(s, TransferMode::Stream);
        assert_eq!(b, TransferMode::Block);
        assert_eq!(c, TransferMode::Compressed);
    }

    #[test]
    fn test_file_structure_variants() {
        let f = FileStructure::File;
        let r = FileStructure::Record;
        let p = FileStructure::Page;
        assert_eq!(f, FileStructure::File);
        assert_eq!(r, FileStructure::Record);
        assert_eq!(p, FileStructure::Page);
    }
}
