//! FTP — File Transfer Protocol client (RFC 959).
//!
//! A minimal FTP client implementation for transferring files
//! to and from FTP servers.
//!
//! ## Features
//!
//! - Connect to FTP servers with anonymous or authenticated login
//! - LIST: directory listing
//! - RETR: file download
//! - STOR: file upload (placeholder)
//! - Passive mode data connections (PASV)
//! - Reply code parsing
//!
//! ## Usage
//!
//! ```text
//! ftp connect <host> [user] [pass]   — connect and login
//! ftp ls <host> [path]               — list remote directory
//! ftp get <host> <file>              — download file content
//! ftp status                         — show statistics
//! ```
//!
//! ## Limitations
//!
//! - Passive mode only (no active mode).
//! - No TLS/FTPS support.
//! - Single control connection (no persistent sessions).

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use super::interface::Ipv4Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// FTP control port.
const FTP_PORT: u16 = 21;

/// Timeout for connection establishment (poll iterations).
const CONNECT_TIMEOUT_POLLS: u32 = 300;

/// Timeout for waiting for server reply (poll iterations).
const REPLY_TIMEOUT_POLLS: u32 = 200;

/// Timeout for data transfer (poll iterations).
const DATA_TIMEOUT_POLLS: u32 = 500;

/// Maximum reply buffer size.
const MAX_REPLY_SIZE: usize = 4096;

/// Maximum data buffer size.
const MAX_DATA_SIZE: usize = 65536;

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

static CONNECTIONS: AtomicU64 = AtomicU64::new(0);
static LOGINS: AtomicU64 = AtomicU64::new(0);
static LIST_COMMANDS: AtomicU64 = AtomicU64::new(0);
static RETR_COMMANDS: AtomicU64 = AtomicU64::new(0);
static BYTES_DOWNLOADED: AtomicU64 = AtomicU64::new(0);
static BYTES_UPLOADED: AtomicU64 = AtomicU64::new(0);
static ERRORS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// FTP reply parsing
// ---------------------------------------------------------------------------

/// An FTP server reply.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API.
pub struct FtpReply {
    /// 3-digit reply code.
    pub code: u16,
    /// Reply message text.
    pub message: String,
}

impl FtpReply {
    /// Check if the reply indicates success (2xx).
    #[allow(dead_code)] // Public API.
    pub fn is_success(&self) -> bool {
        self.code >= 200 && self.code < 300
    }

    /// Check if the reply indicates a positive preliminary (1xx).
    #[allow(dead_code)] // Public API.
    pub fn is_preliminary(&self) -> bool {
        self.code >= 100 && self.code < 200
    }

    /// Check if this reply indicates the server is ready for transfer (150).
    #[allow(dead_code)] // Public API.
    pub fn is_transfer_starting(&self) -> bool {
        self.code == 150
    }

    /// Check if this is a "entering passive mode" reply (227).
    #[allow(dead_code)] // Public API.
    pub fn is_pasv(&self) -> bool {
        self.code == 227
    }
}

/// Parse an FTP reply from raw bytes.
///
/// FTP replies are in the format: `NNN text\r\n`
/// where NNN is a 3-digit code.
#[allow(dead_code)] // Public API.
pub fn parse_reply(data: &[u8]) -> Option<FtpReply> {
    if data.len() < 3 {
        return None;
    }

    let text = core::str::from_utf8(data).ok()?;
    let trimmed = text.trim();

    if trimmed.len() < 3 {
        return None;
    }

    let code: u16 = trimmed.get(..3)?.parse().ok()?;
    let message = if trimmed.len() > 4 {
        trimmed[4..].to_string()
    } else {
        String::new()
    };

    Some(FtpReply {
        code,
        message,
    })
}

/// Parse PASV reply to extract IP and port.
///
/// Reply format: `227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)`
#[allow(dead_code)] // Public API.
pub fn parse_pasv(reply: &FtpReply) -> Option<(Ipv4Addr, u16)> {
    if reply.code != 227 {
        return None;
    }

    // Find the parenthesized part.
    let msg = &reply.message;
    let start = msg.find('(')?;
    let end = msg.find(')')?;
    let inner = msg.get(start + 1..end)?;

    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 6 {
        return None;
    }

    let h1: u8 = parts[0].trim().parse().ok()?;
    let h2: u8 = parts[1].trim().parse().ok()?;
    let h3: u8 = parts[2].trim().parse().ok()?;
    let h4: u8 = parts[3].trim().parse().ok()?;
    let p1: u16 = parts[4].trim().parse().ok()?;
    let p2: u16 = parts[5].trim().parse().ok()?;

    let ip = Ipv4Addr::new(h1, h2, h3, h4);
    let port = (p1 << 8) | p2;

    Some((ip, port))
}

// ---------------------------------------------------------------------------
// FTP session operations
// ---------------------------------------------------------------------------

/// FTP session state.
#[derive(Debug)]
struct FtpSession {
    /// TCP control connection handle.
    control_handle: usize,
}

/// Connect to an FTP server and read the welcome banner.
fn ftp_connect(host: super::interface::IpAddr) -> KernelResult<(FtpSession, FtpReply)> {
    CONNECTIONS.fetch_add(1, Ordering::Relaxed);

    let handle = super::tcp::connect(crate::netns::ROOT_NS, host, FTP_PORT)?;

    // Wait for connection and welcome banner.
    for _ in 0..CONNECT_TIMEOUT_POLLS {
        super::poll();
    }

    // Read welcome banner.
    let banner_data = match super::tcp::read_up_to(handle, MAX_REPLY_SIZE) {
        Ok(d) => d,
        Err(_) => {
            let _ = super::tcp::close(handle);
            ERRORS.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::TimedOut);
        }
    };

    let reply = match parse_reply(&banner_data) {
        Some(r) => r,
        None => {
            let _ = super::tcp::close(handle);
            ERRORS.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::InvalidArgument);
        }
    };

    Ok((FtpSession { control_handle: handle }, reply))
}

/// Send an FTP command and read the reply.
fn ftp_command(session: &FtpSession, cmd: &str) -> KernelResult<FtpReply> {
    // Send command with CRLF.
    let mut buf = Vec::with_capacity(cmd.len() + 2);
    buf.extend_from_slice(cmd.as_bytes());
    buf.extend_from_slice(b"\r\n");

    super::tcp::send(session.control_handle, &buf)?;

    // Wait for reply.
    for _ in 0..REPLY_TIMEOUT_POLLS {
        super::poll();
    }

    let reply_data = super::tcp::read_up_to(session.control_handle, MAX_REPLY_SIZE)?;
    parse_reply(&reply_data).ok_or(KernelError::InvalidArgument)
}

/// Close an FTP session.
fn ftp_close(session: FtpSession) {
    let _ = ftp_command(&session, "QUIT");
    let _ = super::tcp::close(session.control_handle);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// FTP connection result with session info.
#[derive(Debug)]
#[allow(dead_code)] // Public API.
pub struct FtpConnectResult {
    pub banner: String,
    pub login_ok: bool,
}

/// Connect to an FTP server, login, and return the session info.
#[allow(dead_code)] // Public API.
pub fn connect_and_login(
    host: super::interface::IpAddr,
    user: &str,
    pass: &str,
) -> KernelResult<FtpConnectResult> {
    let (session, banner) = ftp_connect(host)?;

    let username = if user.is_empty() { "anonymous" } else { user };
    let password = if pass.is_empty() { "guest@" } else { pass };

    // Send USER command.
    let user_cmd = format!("USER {}", username);
    let user_reply = ftp_command(&session, &user_cmd)?;

    if user_reply.code == 230 {
        // Logged in without password.
        LOGINS.fetch_add(1, Ordering::Relaxed);
        ftp_close(session);
        return Ok(FtpConnectResult {
            banner: banner.message,
            login_ok: true,
        });
    }

    if user_reply.code != 331 {
        // Not asking for password — error.
        ERRORS.fetch_add(1, Ordering::Relaxed);
        ftp_close(session);
        return Ok(FtpConnectResult {
            banner: banner.message,
            login_ok: false,
        });
    }

    // Send PASS command.
    let pass_cmd = format!("PASS {}", password);
    let pass_reply = ftp_command(&session, &pass_cmd)?;

    let login_ok = pass_reply.code == 230;
    if login_ok {
        LOGINS.fetch_add(1, Ordering::Relaxed);
    } else {
        ERRORS.fetch_add(1, Ordering::Relaxed);
    }

    ftp_close(session);

    Ok(FtpConnectResult {
        banner: banner.message,
        login_ok,
    })
}

/// List a directory on an FTP server.
///
/// Connects, logs in, enters PASV mode, sends LIST, and reads
/// the directory listing from the data connection.
#[allow(dead_code)] // Public API.
pub fn list_directory(
    host: super::interface::IpAddr,
    path: &str,
    user: &str,
    pass: &str,
) -> KernelResult<String> {
    LIST_COMMANDS.fetch_add(1, Ordering::Relaxed);

    let (session, _banner) = ftp_connect(host)?;

    let username = if user.is_empty() { "anonymous" } else { user };
    let password = if pass.is_empty() { "guest@" } else { pass };

    // Login.
    let user_cmd = format!("USER {}", username);
    let user_reply = ftp_command(&session, &user_cmd)?;

    if user_reply.code == 331 {
        let pass_cmd = format!("PASS {}", password);
        let pass_reply = ftp_command(&session, &pass_cmd)?;
        if pass_reply.code != 230 {
            ERRORS.fetch_add(1, Ordering::Relaxed);
            ftp_close(session);
            return Err(KernelError::PermissionDenied);
        }
    } else if user_reply.code != 230 {
        ERRORS.fetch_add(1, Ordering::Relaxed);
        ftp_close(session);
        return Err(KernelError::PermissionDenied);
    }

    // Set binary mode.
    let _ = ftp_command(&session, "TYPE I");

    // Enter passive mode.
    let pasv_reply = ftp_command(&session, "PASV")?;
    let (data_ip, data_port) = match parse_pasv(&pasv_reply) {
        Some((ip, port)) => (ip, port),
        None => {
            ERRORS.fetch_add(1, Ordering::Relaxed);
            ftp_close(session);
            return Err(KernelError::InvalidArgument);
        }
    };

    // Open data connection.
    let data_handle = super::tcp::connect(crate::netns::ROOT_NS, data_ip.into(), data_port)?;
    for _ in 0..CONNECT_TIMEOUT_POLLS {
        super::poll();
    }

    // Send LIST command.
    let list_cmd = if path.is_empty() {
        String::from("LIST")
    } else {
        format!("LIST {}", path)
    };
    let list_reply = match ftp_command(&session, &list_cmd) {
        Ok(r) => r,
        Err(e) => {
            let _ = super::tcp::close(data_handle);
            ERRORS.fetch_add(1, Ordering::Relaxed);
            ftp_close(session);
            return Err(e);
        }
    };
    if !list_reply.is_transfer_starting() && !list_reply.is_success() {
        let _ = super::tcp::close(data_handle);
        ERRORS.fetch_add(1, Ordering::Relaxed);
        ftp_close(session);
        return Err(KernelError::InvalidArgument);
    }

    // Read directory listing from data connection.
    for _ in 0..DATA_TIMEOUT_POLLS {
        super::poll();
    }

    let listing_data = super::tcp::read_up_to(data_handle, MAX_DATA_SIZE).unwrap_or_default();

    BYTES_DOWNLOADED.fetch_add(listing_data.len() as u64, Ordering::Relaxed);

    let _ = super::tcp::close(data_handle);

    // Read completion reply (226).
    let _ = ftp_command(&session, "NOOP"); // Trigger reply read.

    ftp_close(session);

    // Convert to string.
    let listing = core::str::from_utf8(&listing_data)
        .unwrap_or("(binary data)")
        .to_string();

    Ok(listing)
}

/// Download a file from an FTP server.
///
/// Returns the raw file contents as bytes.
#[allow(dead_code)] // Public API.
pub fn download_file(
    host: super::interface::IpAddr,
    remote_path: &str,
    user: &str,
    pass: &str,
) -> KernelResult<Vec<u8>> {
    RETR_COMMANDS.fetch_add(1, Ordering::Relaxed);

    let (session, _banner) = ftp_connect(host)?;

    let username = if user.is_empty() { "anonymous" } else { user };
    let password = if pass.is_empty() { "guest@" } else { pass };

    // Login.
    let user_cmd = format!("USER {}", username);
    let user_reply = ftp_command(&session, &user_cmd)?;

    if user_reply.code == 331 {
        let pass_cmd = format!("PASS {}", password);
        let pass_reply = ftp_command(&session, &pass_cmd)?;
        if pass_reply.code != 230 {
            ERRORS.fetch_add(1, Ordering::Relaxed);
            ftp_close(session);
            return Err(KernelError::PermissionDenied);
        }
    } else if user_reply.code != 230 {
        ERRORS.fetch_add(1, Ordering::Relaxed);
        ftp_close(session);
        return Err(KernelError::PermissionDenied);
    }

    // Binary mode.
    let _ = ftp_command(&session, "TYPE I");

    // Passive mode.
    let pasv_reply = ftp_command(&session, "PASV")?;
    let (data_ip, data_port) = match parse_pasv(&pasv_reply) {
        Some((ip, port)) => (ip, port),
        None => {
            ERRORS.fetch_add(1, Ordering::Relaxed);
            ftp_close(session);
            return Err(KernelError::InvalidArgument);
        }
    };

    // Open data connection.
    let data_handle = super::tcp::connect(crate::netns::ROOT_NS, data_ip.into(), data_port)?;
    for _ in 0..CONNECT_TIMEOUT_POLLS {
        super::poll();
    }

    // Send RETR command.
    let retr_cmd = format!("RETR {}", remote_path);
    let retr_reply = match ftp_command(&session, &retr_cmd) {
        Ok(r) => r,
        Err(e) => {
            let _ = super::tcp::close(data_handle);
            ERRORS.fetch_add(1, Ordering::Relaxed);
            ftp_close(session);
            return Err(e);
        }
    };
    if !retr_reply.is_transfer_starting() {
        let _ = super::tcp::close(data_handle);
        ERRORS.fetch_add(1, Ordering::Relaxed);
        ftp_close(session);
        if retr_reply.code == 550 {
            return Err(KernelError::NotFound);
        }
        return Err(KernelError::InvalidArgument);
    }

    // Read file data.
    let mut file_data = Vec::new();
    let mut idle_polls: u32 = 0;
    let max_idle = 100;

    loop {
        for _ in 0..10 {
            super::poll();
        }

        match super::tcp::read_up_to(data_handle, MAX_DATA_SIZE) {
            Ok(chunk) if !chunk.is_empty() => {
                file_data.extend_from_slice(&chunk);
                idle_polls = 0;
            }
            _ => {
                idle_polls = idle_polls.saturating_add(1);
                if idle_polls >= max_idle {
                    break;
                }
            }
        }

        // Safety cap.
        if file_data.len() > 1_048_576 {
            break; // 1 MB max for kernel prototype.
        }
    }

    BYTES_DOWNLOADED.fetch_add(file_data.len() as u64, Ordering::Relaxed);

    let _ = super::tcp::close(data_handle);
    ftp_close(session);

    Ok(file_data)
}

// ---------------------------------------------------------------------------
// Reply code descriptions
// ---------------------------------------------------------------------------

/// Get a description for an FTP reply code.
#[allow(dead_code)] // Public API.
pub fn reply_description(code: u16) -> &'static str {
    match code {
        110 => "Restart marker reply",
        120 => "Service ready in N minutes",
        125 => "Data connection already open; transfer starting",
        150 => "File status okay; opening data connection",
        200 => "Command okay",
        211 => "System status",
        212 => "Directory status",
        213 => "File status",
        214 => "Help message",
        215 => "System type",
        220 => "Service ready for new user",
        221 => "Service closing control connection",
        225 => "Data connection open; no transfer in progress",
        226 => "Closing data connection; transfer complete",
        227 => "Entering Passive Mode",
        230 => "User logged in, proceed",
        250 => "Requested file action okay, completed",
        257 => "Pathname created",
        331 => "User name okay, need password",
        332 => "Need account for login",
        350 => "Requested file action pending further information",
        421 => "Service not available, closing control connection",
        425 => "Can't open data connection",
        426 => "Connection closed; transfer aborted",
        450 => "Requested file action not taken; file unavailable",
        451 => "Requested action aborted; local error in processing",
        452 => "Requested action not taken; insufficient storage space",
        500 => "Syntax error, command unrecognized",
        501 => "Syntax error in parameters or arguments",
        502 => "Command not implemented",
        503 => "Bad sequence of commands",
        504 => "Command not implemented for that parameter",
        530 => "Not logged in",
        532 => "Need account for storing files",
        550 => "Requested action not taken; file unavailable",
        551 => "Requested action aborted; page type unknown",
        552 => "Requested file action aborted; exceeded storage allocation",
        553 => "Requested action not taken; file name not allowed",
        _ => "Unknown reply code",
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// FTP client statistics.
#[derive(Debug)]
#[allow(dead_code)] // Public API.
pub struct FtpStats {
    pub connections: u64,
    pub logins: u64,
    pub list_commands: u64,
    pub retr_commands: u64,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
    pub errors: u64,
}

/// Get FTP statistics.
#[allow(dead_code)] // Public API.
pub fn stats() -> FtpStats {
    FtpStats {
        connections: CONNECTIONS.load(Ordering::Relaxed),
        logins: LOGINS.load(Ordering::Relaxed),
        list_commands: LIST_COMMANDS.load(Ordering::Relaxed),
        retr_commands: RETR_COMMANDS.load(Ordering::Relaxed),
        bytes_downloaded: BYTES_DOWNLOADED.load(Ordering::Relaxed),
        bytes_uploaded: BYTES_UPLOADED.load(Ordering::Relaxed),
        errors: ERRORS.load(Ordering::Relaxed),
    }
}

/// Generate procfs content for `/proc/ftp`.
#[allow(dead_code)] // Public API.
pub fn procfs_content() -> String {
    let s = stats();
    let mut out = String::with_capacity(256);
    out.push_str("FTP Client\n");
    out.push_str("==========\n\n");
    out.push_str(&format!("Connections:     {}\n", s.connections));
    out.push_str(&format!("Logins:          {}\n", s.logins));
    out.push_str(&format!("LIST commands:   {}\n", s.list_commands));
    out.push_str(&format!("RETR commands:   {}\n", s.retr_commands));
    out.push_str(&format!("Downloaded:      {} bytes\n", s.bytes_downloaded));
    out.push_str(&format!("Uploaded:        {} bytes\n", s.bytes_uploaded));
    out.push_str(&format!("Errors:          {}\n", s.errors));
    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run FTP self-tests.
#[allow(dead_code)] // Public API.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[ftp] Running FTP self-tests...");
    let mut passed = 0u32;

    // --- Test 1: Reply parsing ---
    {
        let reply = parse_reply(b"220 Welcome to FTP\r\n");
        assert!(reply.is_some(), "parse reply");
        let reply = reply.unwrap();
        assert!(reply.code == 220, "code");
        assert!(reply.message.contains("Welcome"), "message");
        assert!(reply.is_success(), "is success");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ftp]   test 1 (reply parsing) PASSED");
    }

    // --- Test 2: Reply classification ---
    {
        let r150 = FtpReply { code: 150, message: String::from("Opening data connection") };
        assert!(r150.is_preliminary(), "preliminary");
        assert!(r150.is_transfer_starting(), "transfer starting");
        assert!(!r150.is_success(), "not success");

        let r227 = FtpReply { code: 227, message: String::from("Entering Passive Mode") };
        assert!(r227.is_pasv(), "is pasv");
        assert!(r227.is_success(), "is success");

        let r530 = FtpReply { code: 530, message: String::from("Not logged in") };
        assert!(!r530.is_success(), "not success");
        assert!(!r530.is_preliminary(), "not preliminary");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ftp]   test 2 (reply classification) PASSED");
    }

    // --- Test 3: PASV parsing ---
    {
        let reply = FtpReply {
            code: 227,
            message: String::from("Entering Passive Mode (192,168,1,100,39,6)"),
        };
        let result = parse_pasv(&reply);
        assert!(result.is_some(), "parse PASV");
        let (ip, port) = result.unwrap();
        assert!(ip.0[0] == 192, "ip[0]");
        assert!(ip.0[1] == 168, "ip[1]");
        assert!(ip.0[2] == 1, "ip[2]");
        assert!(ip.0[3] == 100, "ip[3]");
        // Port = 39*256 + 6 = 9990 + 6 = 9990.
        assert!(port == (39 * 256 + 6), "port");

        // Invalid PASV.
        let bad = FtpReply { code: 200, message: String::new() };
        assert!(parse_pasv(&bad).is_none(), "wrong code");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ftp]   test 3 (PASV parsing) PASSED");
    }

    // --- Test 4: Reply descriptions ---
    {
        assert!(reply_description(220) == "Service ready for new user", "220");
        assert!(reply_description(230) == "User logged in, proceed", "230");
        assert!(reply_description(550) == "Requested action not taken; file unavailable", "550");
        assert!(reply_description(150).contains("opening data connection"), "150");
        assert!(reply_description(9999) == "Unknown reply code", "unknown");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ftp]   test 4 (reply descriptions) PASSED");
    }

    // --- Test 5: Stats accessible ---
    {
        let s = stats();
        let _ = s.connections;
        let _ = s.logins;
        let _ = s.bytes_downloaded;
        let _ = s.errors;

        passed = passed.saturating_add(1);
        crate::serial_println!("[ftp]   test 5 (stats) PASSED");
    }

    // --- Test 6: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("FTP"), "header");
        assert!(content.contains("Connections:"), "connections");
        assert!(content.contains("Downloaded:"), "downloaded");
        assert!(content.contains("Errors:"), "errors");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ftp]   test 6 (procfs content) PASSED");
    }

    // --- Test 7: FtpConnectResult struct ---
    {
        let r = FtpConnectResult {
            banner: String::from("Welcome"),
            login_ok: true,
        };
        assert!(r.login_ok, "login ok");
        assert!(r.banner == "Welcome", "banner");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ftp]   test 7 (FtpConnectResult) PASSED");
    }

    // --- Test 8: Multi-line reply parsing ---
    {
        // First line of multi-line.
        let reply = parse_reply(b"230-Welcome\r\n");
        assert!(reply.is_some(), "multi-line");
        let r = reply.unwrap();
        assert!(r.code == 230, "code");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ftp]   test 8 (multi-line reply) PASSED");
    }

    // --- Test 9: FtpStats struct ---
    {
        let s = FtpStats {
            connections: 10,
            logins: 8,
            list_commands: 5,
            retr_commands: 3,
            bytes_downloaded: 1_000_000,
            bytes_uploaded: 0,
            errors: 2,
        };
        assert!(s.connections == 10, "connections");
        assert!(s.bytes_downloaded == 1_000_000, "downloaded");
        assert!(s.errors == 2, "errors");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ftp]   test 9 (FtpStats) PASSED");
    }

    // --- Test 10: Reply edge cases ---
    {
        // Minimal reply.
        let r = parse_reply(b"200");
        assert!(r.is_some(), "minimal reply");
        assert!(r.unwrap().code == 200, "minimal code");

        // Empty data.
        assert!(parse_reply(b"").is_none(), "empty");

        // Too short.
        assert!(parse_reply(b"20").is_none(), "too short");

        // Non-numeric.
        assert!(parse_reply(b"abc response").is_none(), "non-numeric");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ftp]   test 10 (reply edge cases) PASSED");
    }

    crate::serial_println!("[ftp] All {} self-tests PASSED", passed);
    Ok(())
}
