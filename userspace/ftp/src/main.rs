//! `SlateOS` FTP Client
//!
//! A full-featured interactive FTP client for `SlateOS`. Implements the FTP
//! protocol (RFC 959) over the kernel's TCP syscall interface with support
//! for both active and passive data connections, ASCII and binary transfer
//! modes, glob-based multi-file transfers, and anonymous login.
//!
//! # Usage
//!
//! ```text
//! ftp [options] [host [port]]
//!
//! Options:
//!   -n            Do not auto-login after connecting
//!   -v            Verbose mode (show server replies)
//!   -d            Debug mode (show all protocol traffic)
//!   -p            Use passive mode for data connections (default)
//!   -a            Use active mode for data connections
//! ```
//!
//! # Interactive Commands
//!
//! ```text
//! open host [port]     Connect to FTP server
//! close                Close current connection
//! quit / bye / exit    Close connection and exit
//! user [name]          Login with username (prompts for password)
//! cd dir               Change remote directory
//! pwd                  Print remote working directory
//! ls [path]            List remote directory
//! dir [path]           List remote directory (verbose)
//! get remote [local]   Download a file
//! recv remote [local]  Download a file (alias for get)
//! put local [remote]   Upload a file
//! send local [remote]  Upload a file (alias for put)
//! mget pattern         Download multiple files matching glob
//! mput pattern         Upload multiple files matching glob
//! mkdir dir            Create remote directory
//! rmdir dir            Remove remote directory
//! delete file          Delete remote file
//! rm file              Delete remote file (alias)
//! rename from to       Rename remote file
//! binary               Set binary transfer mode
//! ascii                Set ASCII transfer mode
//! type                 Show current transfer type
//! passive / pasv       Toggle passive mode
//! active               Switch to active mode
//! status               Show connection status
//! system               Show remote system type
//! size file            Show remote file size
//! hash                 Toggle hash-mark printing
//! bell                 Toggle bell on transfer completion
//! verbose              Toggle verbose mode
//! debug                Toggle debug mode
//! lcd [dir]            Change local directory
//! lpwd                 Print local working directory
//! prompt               Toggle interactive prompting for mget/mput
//! glob                 Toggle glob expansion for mget/mput
//! help / ?             Show help
//! ```

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
//
// ftp marshalls the RFC 959 protocol over TCP — every PASV-port decode,
// directory-listing parse, and progress-bar render is offset+length
// arithmetic on validated-length buffers / bounded counters. The
// defensive `arithmetic_side_effects`, `indexing_slicing`, and
// `slicing` lints fire on every such site (30+ warnings) with no real
// DoS risk; the wire data is length-validated by the read layer.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
)]

use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

// ============================================================================
// Syscall numbers (from kernel/src/syscall/ tables)
// ============================================================================
//
// Networking goes through the native SlateOS TCP/DNS syscalls.  Local file I/O
// (open/read/write/stat/getcwd/chdir/readdir) is handled entirely through
// `std::fs`/`std::env`, which the SlateOS libc routes to the correct native
// syscalls — the previous raw Linux syscall numbers (0/1/2/3/4/78/79/80) did
// not exist on this OS (0 and 1 are SYS_YIELD/SYS_EXIT here).

const SYS_TCP_CONNECT: u64 = 800;
const SYS_TCP_SEND: u64 = 801;
const SYS_TCP_RECV: u64 = 802;
const SYS_TCP_CLOSE: u64 = 803;
const SYS_TCP_BIND: u64 = 804;
const SYS_TCP_ACCEPT: u64 = 805;
const SYS_TCP_CLOSE_LISTENER: u64 = 806;
const SYS_DNS_RESOLVE: u64 = 820;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 1-argument raw syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and `a1` is valid
/// for that syscall.
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

/// Issue a 3-argument raw syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for that syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid.
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

// Non-x86_64 stubs so the crate compiles for host-side `cargo test`.
// These are never called at runtime on a non-x86_64 host because the test
// suite avoids exercising paths that issue real syscalls.
#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall1(_nr: u64, _a1: u64) -> i64 {
    -1
}
#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 {
    -1
}

// ============================================================================
// Raw syscall wrappers
// ============================================================================

/// Resolve a hostname to an IPv4 address (network byte order).
fn dns_resolve(hostname: &str) -> Result<u32, FtpError> {
    // The kernel writes the four address octets [a, b, c, d] (MSB first) here.
    // Reading them back as a native-endian u32 on little-endian x86_64 would
    // reverse the address, so reassemble explicitly with from_be_bytes (matching
    // sys_tcp_connect's Ipv4Addr::from_u32 == to_be_bytes and parse_ipv4 below).
    let mut octets = [0u8; 4];
    // SAFETY: We pass valid pointer/length for the hostname string and a valid
    // mutable 4-byte buffer for the kernel to write the resolved IP address.
    let ret = unsafe {
        syscall3(
            SYS_DNS_RESOLVE,
            hostname.as_ptr() as u64,
            hostname.len() as u64,
            octets.as_mut_ptr() as u64,
        )
    };
    if ret < 0 {
        return Err(FtpError::DnsFailure(hostname.to_string()));
    }
    Ok(u32::from_be_bytes(octets))
}

/// Open a TCP connection to the given IPv4 address and port.
/// Returns a connection handle on success.
fn tcp_connect(ip: u32, port: u16) -> Result<u64, FtpError> {
    // SAFETY: We pass scalar IP (network order) and port. The kernel returns a
    // handle (>= 0) or negative error code.
    let ret = unsafe { syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), 0) };
    if ret < 0 {
        return Err(FtpError::ConnectionFailed(format!(
            "tcp_connect failed: {ret}"
        )));
    }
    Ok(ret as u64)
}

/// Send data on a TCP connection. Returns number of bytes sent.
fn tcp_send(handle: u64, data: &[u8]) -> Result<usize, FtpError> {
    // SAFETY: Valid handle and pointer/length to byte buffer.
    let ret = unsafe {
        syscall3(
            SYS_TCP_SEND,
            handle,
            data.as_ptr() as u64,
            data.len() as u64,
        )
    };
    if ret < 0 {
        return Err(FtpError::SendFailed);
    }
    Ok(ret as usize)
}

/// Send all bytes, looping until the entire buffer has been transmitted.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), FtpError> {
    let mut offset = 0;
    while offset < data.len() {
        let n = tcp_send(handle, &data[offset..])?;
        if n == 0 {
            return Err(FtpError::SendFailed);
        }
        offset = offset.checked_add(n).ok_or(FtpError::SendFailed)?;
    }
    Ok(())
}

/// Receive data from a TCP connection. Returns number of bytes read (0 = EOF).
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, FtpError> {
    // SAFETY: Valid handle and writable buffer with correct length.
    let ret = unsafe {
        syscall3(
            SYS_TCP_RECV,
            handle,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(FtpError::RecvFailed);
    }
    Ok(ret as usize)
}

/// Close a TCP connection.
fn tcp_close(handle: u64) {
    // SAFETY: Valid handle. Ignoring the return value is safe here because
    // close failures are not actionable (the handle becomes invalid regardless).
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

/// Bind a listening socket on the given port. Returns a listener handle.
fn tcp_bind(port: u16) -> Result<u64, FtpError> {
    // SAFETY: Scalar port argument. Kernel returns listener handle or negative error.
    let ret = unsafe { syscall1(SYS_TCP_BIND, u64::from(port)) };
    if ret < 0 {
        return Err(FtpError::ConnectionFailed(format!(
            "tcp_bind failed: {ret}"
        )));
    }
    Ok(ret as u64)
}

/// Accept an incoming connection on a listener. Returns connection handle.
fn tcp_accept(listener: u64) -> Result<u64, FtpError> {
    // SAFETY: Valid listener handle. Kernel returns connection handle or negative.
    let ret = unsafe { syscall1(SYS_TCP_ACCEPT, listener) };
    if ret < 0 {
        return Err(FtpError::ConnectionFailed(format!(
            "tcp_accept failed: {ret}"
        )));
    }
    Ok(ret as u64)
}

/// Close a listener handle.
fn tcp_close_listener(listener: u64) {
    // SAFETY: Valid listener handle. Close failures are not actionable.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE_LISTENER, listener) };
}

/// Get the current working directory as a String.
fn get_cwd() -> Result<String, FtpError> {
    let dir = env::current_dir().map_err(|e| FtpError::IoError(format!("getcwd failed: {e}")))?;
    Ok(dir.to_string_lossy().into_owned())
}

/// Change the local working directory.
fn change_dir(path: &str) -> Result<(), FtpError> {
    env::set_current_dir(path)
        .map_err(|e| FtpError::IoError(format!("chdir to '{path}' failed: {e}")))
}

/// Return the size of a file in bytes (also verifies it exists/is readable).
fn file_size(path: &str) -> Result<u64, FtpError> {
    let meta =
        fs::metadata(path).map_err(|e| FtpError::IoError(format!("stat '{path}' failed: {e}")))?;
    Ok(meta.len())
}

/// List entries in a directory. Returns a vector of filenames (excluding `.`/`..`).
fn list_directory(path: &str) -> Result<Vec<String>, FtpError> {
    let mut names = Vec::new();
    let read_dir = fs::read_dir(path)
        .map_err(|e| FtpError::IoError(format!("read_dir '{path}' failed: {e}")))?;
    for entry in read_dir {
        let dir_entry =
            entry.map_err(|e| FtpError::IoError(format!("read_dir '{path}' failed: {e}")))?;
        // std::fs::read_dir never yields "." or ".." entries.
        names.push(dir_entry.file_name().to_string_lossy().into_owned());
    }
    Ok(names)
}

// ============================================================================
// Error types
// ============================================================================

/// Errors that can occur during FTP operations.
#[derive(Debug)]
enum FtpError {
    /// DNS lookup failed.
    DnsFailure(String),
    /// TCP connection or listener failure.
    ConnectionFailed(String),
    /// TCP send failed.
    SendFailed,
    /// TCP receive failed.
    RecvFailed,
    /// An I/O error (file operations, cwd, etc.).
    IoError(String),
    /// FTP protocol error (unexpected reply code).
    ProtocolError(String),
    /// Not connected to a server.
    NotConnected,
    /// Login required for this operation.
    NotLoggedIn,
}

impl fmt::Display for FtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DnsFailure(host) => write!(f, "DNS resolution failed for '{host}'"),
            Self::ConnectionFailed(msg) => write!(f, "Connection failed: {msg}"),
            Self::SendFailed => write!(f, "Failed to send data"),
            Self::RecvFailed => write!(f, "Failed to receive data"),
            Self::IoError(msg) => write!(f, "I/O error: {msg}"),
            Self::ProtocolError(msg) => write!(f, "FTP protocol error: {msg}"),
            Self::NotConnected => write!(f, "Not connected"),
            Self::NotLoggedIn => write!(f, "Not logged in"),
        }
    }
}

// ============================================================================
// FTP reply parsing
// ============================================================================

/// A parsed FTP reply from the server: a 3-digit code and the full text.
#[derive(Debug, Clone)]
struct FtpReply {
    code: u16,
    text: String,
}

impl FtpReply {
    /// Whether this reply indicates success (2xx).
    fn is_positive(&self) -> bool {
        self.code >= 200 && self.code < 300
    }

    /// Whether this reply is a positive preliminary (1xx) — operation started.
    fn is_preliminary(&self) -> bool {
        self.code >= 100 && self.code < 200
    }

    /// Whether this reply is an intermediate (3xx) — needs more info (e.g. password).
    fn is_intermediate(&self) -> bool {
        self.code >= 300 && self.code < 400
    }

    /// Whether this reply indicates a transient error (4xx).
    fn is_transient_error(&self) -> bool {
        self.code >= 400 && self.code < 500
    }

    /// Whether this reply indicates a permanent error (5xx).
    fn is_permanent_error(&self) -> bool {
        self.code >= 500 && self.code < 600
    }

    /// Whether this reply is any kind of error (4xx or 5xx).
    fn is_error(&self) -> bool {
        self.is_transient_error() || self.is_permanent_error()
    }
}

/// Parse a single FTP reply code from a line.
/// Returns `Some(code)` if the line starts with a 3-digit number.
fn parse_reply_code(line: &str) -> Option<u16> {
    if line.len() < 3 {
        return None;
    }
    let code_str = &line[..3];
    let code: u16 = code_str.parse().ok()?;
    if !(100..=599).contains(&code) {
        return None;
    }
    Some(code)
}

/// Determine whether a reply line is the final line of a (possibly multi-line)
/// FTP reply.
///
/// Multi-line replies use "NNN-" on all lines except the final one, which uses
/// "NNN " (space after the code). A single-line reply is "NNN text".
fn is_final_reply_line(line: &str, expected_code: u16) -> bool {
    if line.len() < 4 {
        // A line with just "NNN" (no separator) is treated as final.
        return parse_reply_code(line) == Some(expected_code);
    }
    let code_match = parse_reply_code(line) == Some(expected_code);
    let separator = line.as_bytes().get(3).copied();
    // Final line has a space (or nothing) after the code, not a hyphen.
    code_match && separator != Some(b'-')
}

// ============================================================================
// PASV response parsing
// ============================================================================

/// Parse a PASV (227) response to extract the data connection address and port.
///
/// The response contains `(h1,h2,h3,h4,p1,p2)` where the IP is h1.h2.h3.h4
/// and the port is p1*256 + p2.
fn parse_pasv_response(text: &str) -> Option<(u32, u16)> {
    // Find the opening parenthesis (or the first digit sequence after the code).
    let start = text.find('(')?;
    let end = text[start..].find(')')? + start;
    let inner = &text[start + 1..end];

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

    // IP in network byte order (big-endian).
    let ip = u32::from_be_bytes([h1, h2, h3, h4]);
    let port = p1 * 256 + p2;

    Some((ip, port))
}

// ============================================================================
// Glob pattern matching
// ============================================================================

/// Match a filename against a glob pattern.
///
/// Supports `*` (any sequence of characters) and `?` (any single character).
/// No character classes or brace expansion.
fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), text.as_bytes())
}

/// Recursive byte-level glob match.
fn glob_match_bytes(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pat = usize::MAX;
    let mut star_txt = usize::MAX;

    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_pat = pi;
            star_txt = ti;
            pi += 1;
        } else if star_pat != usize::MAX {
            pi = star_pat + 1;
            star_txt += 1;
            ti = star_txt;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}

// ============================================================================
// Command-line argument parsing
// ============================================================================

/// Parsed command-line arguments for the FTP client.
struct CliArgs {
    host: Option<String>,
    port: u16,
    auto_login: bool,
    verbose: bool,
    debug: bool,
    passive: bool,
}

impl Default for CliArgs {
    fn default() -> Self {
        Self {
            host: None,
            port: 21,
            auto_login: true,
            verbose: false,
            debug: false,
            passive: true,
        }
    }
}

/// Parse command-line arguments.
fn parse_args(args: &[String]) -> CliArgs {
    let mut result = CliArgs::default();
    let mut positional = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-n" => result.auto_login = false,
            "-v" => result.verbose = true,
            "-d" => {
                result.debug = true;
                result.verbose = true;
            }
            "-p" => result.passive = true,
            "-a" => result.passive = false,
            _ => {
                if let Some(flags) = arg.strip_prefix('-') {
                    // Try combined flags like "-nv"
                    let mut recognized = true;
                    for ch in flags.chars() {
                        match ch {
                            'n' => result.auto_login = false,
                            'v' => result.verbose = true,
                            'd' => {
                                result.debug = true;
                                result.verbose = true;
                            }
                            'p' => result.passive = true,
                            'a' => result.passive = false,
                            _ => {
                                recognized = false;
                                break;
                            }
                        }
                    }
                    if !recognized {
                        eprintln!("ftp: unknown option: {arg}");
                    }
                } else {
                    positional.push(arg.clone());
                }
            }
        }
        i += 1;
    }

    if let Some(host) = positional.first() {
        result.host = Some(host.clone());
    }
    if let Some(port_str) = positional.get(1) {
        if let Ok(p) = port_str.parse::<u16>() {
            result.port = p;
        } else {
            eprintln!("ftp: invalid port: {port_str}");
        }
    }

    result
}

// ============================================================================
// Interactive command parsing
// ============================================================================

/// A parsed interactive command from the user.
#[derive(Debug, PartialEq)]
enum Command {
    Open {
        host: String,
        port: u16,
    },
    Close,
    Quit,
    User {
        name: Option<String>,
    },
    Cd {
        path: String,
    },
    Pwd,
    Ls {
        path: Option<String>,
    },
    Dir {
        path: Option<String>,
    },
    Get {
        remote: String,
        local: Option<String>,
    },
    Put {
        local: String,
        remote: Option<String>,
    },
    Mget {
        pattern: String,
    },
    Mput {
        pattern: String,
    },
    Mkdir {
        path: String,
    },
    Rmdir {
        path: String,
    },
    Delete {
        path: String,
    },
    Rename {
        from: String,
        to: String,
    },
    Binary,
    Ascii,
    Type,
    Passive,
    Active,
    Status,
    System,
    Size {
        path: String,
    },
    Hash,
    Bell,
    Verbose,
    Debug,
    Lcd {
        path: Option<String>,
    },
    Lpwd,
    Prompt,
    Glob,
    Help,
    Empty,
    Unknown(String),
}

/// Parse a user input line into a `Command`.
fn parse_command(line: &str) -> Command {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Command::Empty;
    }

    let parts: Vec<&str> = trimmed.splitn(3, char::is_whitespace).collect();
    let cmd = parts[0].to_lowercase();
    let arg1 = parts.get(1).map(|s| s.trim());
    let arg2 = parts.get(2).map(|s| s.trim());

    match cmd.as_str() {
        "open" => {
            let host = match arg1 {
                Some(h) if !h.is_empty() => h.to_string(),
                _ => return Command::Unknown("open requires a host".to_string()),
            };
            let port = arg2.and_then(|p| p.parse::<u16>().ok()).unwrap_or(21);
            Command::Open { host, port }
        }
        "close" | "disconnect" => Command::Close,
        "quit" | "bye" | "exit" => Command::Quit,
        "user" => Command::User {
            name: arg1.map(std::string::ToString::to_string),
        },
        "cd" | "cwd" => {
            let path = match arg1 {
                Some(p) if !p.is_empty() => p.to_string(),
                _ => return Command::Unknown("cd requires a path".to_string()),
            };
            Command::Cd { path }
        }
        "pwd" => Command::Pwd,
        "ls" | "nlist" => Command::Ls {
            path: arg1.map(std::string::ToString::to_string),
        },
        "dir" | "list" => Command::Dir {
            path: arg1.map(std::string::ToString::to_string),
        },
        "get" | "recv" => {
            let remote = match arg1 {
                Some(r) if !r.is_empty() => r.to_string(),
                _ => return Command::Unknown("get requires a remote filename".to_string()),
            };
            Command::Get {
                remote,
                local: arg2.map(std::string::ToString::to_string),
            }
        }
        "put" | "send" => {
            let local = match arg1 {
                Some(l) if !l.is_empty() => l.to_string(),
                _ => return Command::Unknown("put requires a local filename".to_string()),
            };
            Command::Put {
                local,
                remote: arg2.map(std::string::ToString::to_string),
            }
        }
        "mget" => {
            let pattern = match arg1 {
                Some(p) if !p.is_empty() => p.to_string(),
                _ => return Command::Unknown("mget requires a pattern".to_string()),
            };
            Command::Mget { pattern }
        }
        "mput" => {
            let pattern = match arg1 {
                Some(p) if !p.is_empty() => p.to_string(),
                _ => return Command::Unknown("mput requires a pattern".to_string()),
            };
            Command::Mput { pattern }
        }
        "mkdir" => {
            let path = match arg1 {
                Some(p) if !p.is_empty() => p.to_string(),
                _ => return Command::Unknown("mkdir requires a path".to_string()),
            };
            Command::Mkdir { path }
        }
        "rmdir" => {
            let path = match arg1 {
                Some(p) if !p.is_empty() => p.to_string(),
                _ => return Command::Unknown("rmdir requires a path".to_string()),
            };
            Command::Rmdir { path }
        }
        "delete" | "rm" | "del" => {
            let path = match arg1 {
                Some(p) if !p.is_empty() => p.to_string(),
                _ => return Command::Unknown("delete requires a filename".to_string()),
            };
            Command::Delete { path }
        }
        "rename" | "ren" => {
            let from = match arg1 {
                Some(f) if !f.is_empty() => f.to_string(),
                _ => return Command::Unknown("rename requires two arguments".to_string()),
            };
            let to = match arg2 {
                Some(t) if !t.is_empty() => t.to_string(),
                _ => return Command::Unknown("rename requires two arguments".to_string()),
            };
            Command::Rename { from, to }
        }
        "binary" | "bin" | "image" | "i" => Command::Binary,
        "ascii" | "asc" | "a" => Command::Ascii,
        "type" => Command::Type,
        "passive" | "pasv" => Command::Passive,
        "active" | "port" => Command::Active,
        "status" | "stat" => Command::Status,
        "system" | "syst" => Command::System,
        "size" => {
            let path = match arg1 {
                Some(p) if !p.is_empty() => p.to_string(),
                _ => return Command::Unknown("size requires a filename".to_string()),
            };
            Command::Size { path }
        }
        "hash" => Command::Hash,
        "bell" => Command::Bell,
        "verbose" => Command::Verbose,
        "debug" => Command::Debug,
        "lcd" => Command::Lcd {
            path: arg1.map(std::string::ToString::to_string),
        },
        "lpwd" => Command::Lpwd,
        "prompt" => Command::Prompt,
        "glob" => Command::Glob,
        "help" | "?" => Command::Help,
        _ => Command::Unknown(cmd),
    }
}

// ============================================================================
// Transfer type
// ============================================================================

/// FTP transfer type: ASCII (text) or binary (image).
#[derive(Debug, Clone, Copy, PartialEq)]
enum TransferType {
    Ascii,
    Binary,
}

impl fmt::Display for TransferType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ascii => write!(f, "ascii"),
            Self::Binary => write!(f, "binary"),
        }
    }
}

// ============================================================================
// FTP session
// ============================================================================

/// State of the FTP client session.
struct FtpSession {
    /// TCP handle for the control connection, or None if disconnected.
    control_handle: Option<u64>,
    /// The connected server hostname (for display).
    hostname: String,
    /// IP address of the server (network byte order).
    server_ip: u32,
    /// Control port.
    control_port: u16,
    /// Whether the user is logged in.
    logged_in: bool,
    /// Current transfer type.
    transfer_type: TransferType,
    /// Whether to use passive mode for data connections.
    passive_mode: bool,
    /// Port counter for active mode data connections.
    active_port: u16,
    /// Whether to show server replies.
    verbose: bool,
    /// Whether to show all protocol traffic.
    debug: bool,
    /// Whether to print hash marks during transfers.
    hash_print: bool,
    /// Whether to ring the bell after transfers.
    bell: bool,
    /// Whether to prompt interactively for mget/mput.
    interactive_prompt: bool,
    /// Whether to expand glob patterns for mget/mput.
    glob_enabled: bool,
    /// Receive buffer for the control connection.
    recv_buffer: Vec<u8>,
}

impl FtpSession {
    fn new() -> Self {
        Self {
            control_handle: None,
            hostname: String::new(),
            server_ip: 0,
            control_port: 21,
            logged_in: false,
            transfer_type: TransferType::Binary,
            passive_mode: true,
            active_port: 20_000,
            verbose: false,
            debug: false,
            hash_print: false,
            bell: false,
            interactive_prompt: true,
            glob_enabled: true,
            recv_buffer: Vec::with_capacity(8192),
        }
    }

    /// Whether the client is connected to a server.
    fn is_connected(&self) -> bool {
        self.control_handle.is_some()
    }

    // ========================================================================
    // Control connection I/O
    // ========================================================================

    /// Send a raw FTP command to the server.
    fn send_command(&mut self, cmd: &str) -> Result<(), FtpError> {
        let handle = self.control_handle.ok_or(FtpError::NotConnected)?;
        if self.debug {
            eprintln!("---> {cmd}");
        }
        let line = format!("{cmd}\r\n");
        tcp_send_all(handle, line.as_bytes())
    }

    /// Read a complete FTP reply (possibly multi-line) from the control
    /// connection.
    fn read_reply(&mut self) -> Result<FtpReply, FtpError> {
        let handle = self.control_handle.ok_or(FtpError::NotConnected)?;

        let mut full_text = String::new();
        let mut final_code: Option<u16> = None;

        loop {
            // Try to extract a complete line from the buffer.
            if let Some(line) = self.extract_line() {
                if self.debug {
                    eprintln!("<--- {line}");
                }

                // On the first line, extract the expected reply code.
                if final_code.is_none() {
                    if let Some(code) = parse_reply_code(&line) {
                        final_code = Some(code);
                    } else {
                        // Non-standard line before any code — accumulate.
                        if !full_text.is_empty() {
                            full_text.push('\n');
                        }
                        full_text.push_str(&line);
                        continue;
                    }
                }

                if !full_text.is_empty() {
                    full_text.push('\n');
                }
                full_text.push_str(&line);

                if let Some(code) = final_code
                    && is_final_reply_line(&line, code)
                {
                    if self.verbose && !self.debug {
                        // In verbose (but non-debug) mode, print the reply.
                        println!("{full_text}");
                    }
                    return Ok(FtpReply {
                        code,
                        text: full_text,
                    });
                }

                continue;
            }

            // Need more data from the network.
            let mut tmp = [0u8; 4096];
            let n = tcp_recv(handle, &mut tmp)?;
            if n == 0 {
                // Connection closed mid-reply.
                self.control_handle = None;
                return Err(FtpError::ProtocolError(
                    "Connection closed by server".to_string(),
                ));
            }
            self.recv_buffer.extend_from_slice(&tmp[..n]);
        }
    }

    /// Try to extract a single line (terminated by `\r\n` or `\n`) from the
    /// receive buffer. Returns `None` if no complete line is available.
    fn extract_line(&mut self) -> Option<String> {
        let newline_pos = self.recv_buffer.iter().position(|&b| b == b'\n')?;
        let mut line_bytes: Vec<u8> = self.recv_buffer.drain(..=newline_pos).collect();
        // Strip trailing \r\n or \n.
        if line_bytes.last() == Some(&b'\n') {
            line_bytes.pop();
        }
        if line_bytes.last() == Some(&b'\r') {
            line_bytes.pop();
        }
        Some(String::from_utf8_lossy(&line_bytes).into_owned())
    }

    // ========================================================================
    // Connection management
    // ========================================================================

    /// Connect to an FTP server.
    fn connect(&mut self, host: &str, port: u16) -> Result<(), FtpError> {
        if self.is_connected() {
            self.disconnect();
        }

        let ip = dns_resolve(host)?;
        let handle = tcp_connect(ip, port)?;

        self.control_handle = Some(handle);
        self.hostname = host.to_string();
        self.server_ip = ip;
        self.control_port = port;
        self.logged_in = false;
        self.recv_buffer.clear();

        println!("Connected to {host}.");

        // Read the welcome message (220 reply).
        let reply = self.read_reply()?;
        if !reply.is_positive() && !reply.is_preliminary() {
            self.disconnect();
            return Err(FtpError::ProtocolError(format!(
                "Server rejected connection: {}",
                reply.code
            )));
        }

        Ok(())
    }

    /// Disconnect from the current server.
    fn disconnect(&mut self) {
        if let Some(handle) = self.control_handle.take() {
            // Try to send QUIT, but ignore errors since we're disconnecting.
            let _ = tcp_send_all(handle, b"QUIT\r\n");
            tcp_close(handle);
        }
        self.logged_in = false;
        self.hostname.clear();
        self.server_ip = 0;
        self.recv_buffer.clear();
    }

    /// Login to the server with username and password.
    fn login(&mut self, user: &str, pass: &str) -> Result<(), FtpError> {
        self.send_command(&format!("USER {user}"))?;
        let reply = self.read_reply()?;

        match reply.code {
            230 => {
                // Logged in, no password needed.
                self.logged_in = true;
                return Ok(());
            }
            331 => {
                // Password required.
            }
            _ => {
                return Err(FtpError::ProtocolError(format!(
                    "USER failed: {} {}",
                    reply.code, reply.text
                )));
            }
        }

        self.send_command(&format!("PASS {pass}"))?;
        let reply = self.read_reply()?;

        match reply.code {
            230 => {
                self.logged_in = true;
                Ok(())
            }
            332 => {
                // Account required — send ACCT with empty string.
                self.send_command("ACCT noaccount")?;
                let reply = self.read_reply()?;
                if reply.code == 230 {
                    self.logged_in = true;
                    Ok(())
                } else {
                    Err(FtpError::ProtocolError(format!(
                        "ACCT failed: {} {}",
                        reply.code, reply.text
                    )))
                }
            }
            _ => Err(FtpError::ProtocolError(format!(
                "Login failed: {} {}",
                reply.code, reply.text
            ))),
        }
    }

    /// Ensure the client is connected and logged in.
    fn require_login(&self) -> Result<(), FtpError> {
        if !self.is_connected() {
            return Err(FtpError::NotConnected);
        }
        if !self.logged_in {
            return Err(FtpError::NotLoggedIn);
        }
        Ok(())
    }

    // ========================================================================
    // Data connection setup
    // ========================================================================

    /// Open a data connection using either passive or active mode.
    /// Returns the TCP handle for the data connection.
    fn open_data_connection(&mut self) -> Result<u64, FtpError> {
        if self.passive_mode {
            self.open_passive_data()
        } else {
            self.open_active_data()
        }
    }

    /// Open a data connection using PASV mode.
    fn open_passive_data(&mut self) -> Result<u64, FtpError> {
        self.send_command("PASV")?;
        let reply = self.read_reply()?;

        if reply.code != 227 {
            return Err(FtpError::ProtocolError(format!(
                "PASV failed: {} {}",
                reply.code, reply.text
            )));
        }

        let (ip, port) = parse_pasv_response(&reply.text)
            .ok_or_else(|| FtpError::ProtocolError("Failed to parse PASV response".to_string()))?;

        tcp_connect(ip, port)
    }

    /// Open a data connection using PORT (active) mode.
    fn open_active_data(&mut self) -> Result<u64, FtpError> {
        // Pick a port for the data connection.
        self.active_port = self.active_port.wrapping_add(1);
        if self.active_port < 1024 {
            self.active_port = 20_000;
        }
        let port = self.active_port;

        let listener = tcp_bind(port)?;

        // Build PORT command: PORT h1,h2,h3,h4,p1,p2
        // We use 127.0.0.1 as a placeholder — in a real network this would be
        // the client's externally-reachable IP.
        let p1 = port / 256;
        let p2 = port % 256;
        let cmd = format!("PORT 127,0,0,1,{p1},{p2}");
        self.send_command(&cmd)?;
        let reply = self.read_reply()?;

        if reply.code != 200 {
            tcp_close_listener(listener);
            return Err(FtpError::ProtocolError(format!(
                "PORT failed: {} {}",
                reply.code, reply.text
            )));
        }

        // Wait for the server to connect back to us.
        let data_handle = tcp_accept(listener)?;
        tcp_close_listener(listener);

        Ok(data_handle)
    }

    // ========================================================================
    // Transfer type management
    // ========================================================================

    /// Set the transfer type on the server.
    fn set_type(&mut self, tt: TransferType) -> Result<(), FtpError> {
        let type_cmd = match tt {
            TransferType::Ascii => "TYPE A",
            TransferType::Binary => "TYPE I",
        };
        self.send_command(type_cmd)?;
        let reply = self.read_reply()?;
        if reply.is_positive() {
            self.transfer_type = tt;
            println!("Transfer type set to {tt}.");
            Ok(())
        } else {
            Err(FtpError::ProtocolError(format!(
                "TYPE failed: {} {}",
                reply.code, reply.text
            )))
        }
    }

    // ========================================================================
    // FTP operations
    // ========================================================================

    /// Print remote working directory (PWD).
    fn cmd_pwd(&mut self) -> Result<(), FtpError> {
        self.require_login()?;
        self.send_command("PWD")?;
        let reply = self.read_reply()?;
        if !self.verbose {
            println!("{}", reply.text);
        }
        Ok(())
    }

    /// Change remote directory (CWD).
    fn cmd_cd(&mut self, path: &str) -> Result<(), FtpError> {
        self.require_login()?;
        self.send_command(&format!("CWD {path}"))?;
        let reply = self.read_reply()?;
        if reply.is_positive() {
            if !self.verbose {
                println!("Directory changed to {path}.");
            }
        } else {
            println!("cd failed: {} {}", reply.code, reply.text);
        }
        Ok(())
    }

    /// List remote directory (LIST or NLST).
    fn cmd_list(&mut self, path: Option<&str>, verbose_list: bool) -> Result<(), FtpError> {
        self.require_login()?;

        let data_handle = self.open_data_connection()?;

        let cmd = if verbose_list {
            match path {
                Some(p) => format!("LIST {p}"),
                None => "LIST".to_string(),
            }
        } else {
            match path {
                Some(p) => format!("NLST {p}"),
                None => "NLST".to_string(),
            }
        };

        self.send_command(&cmd)?;
        let reply = self.read_reply()?;

        if !reply.is_preliminary() && !reply.is_positive() {
            tcp_close(data_handle);
            return Err(FtpError::ProtocolError(format!(
                "LIST failed: {} {}",
                reply.code, reply.text
            )));
        }

        // Read listing data.
        let mut buf = [0u8; 4096];
        loop {
            let n = tcp_recv(data_handle, &mut buf)?;
            if n == 0 {
                break;
            }
            // Print listing directly to stdout.
            let text = String::from_utf8_lossy(&buf[..n]);
            print!("{text}");
        }
        let _ = io::stdout().flush();

        tcp_close(data_handle);

        // Read the transfer-complete reply.
        let reply = self.read_reply()?;
        if !reply.is_positive() && !self.verbose {
            println!("{}", reply.text);
        }

        Ok(())
    }

    /// Download a file (RETR).
    fn cmd_get(&mut self, remote: &str, local: Option<&str>) -> Result<(), FtpError> {
        self.require_login()?;

        let local_name = local.unwrap_or(remote);
        // Extract just the filename if remote contains a path.
        let local_name = local_name.rsplit('/').next().unwrap_or(local_name);

        let data_handle = self.open_data_connection()?;

        self.send_command(&format!("RETR {remote}"))?;
        let reply = self.read_reply()?;

        if !reply.is_preliminary() && !reply.is_positive() {
            tcp_close(data_handle);
            return Err(FtpError::ProtocolError(format!(
                "RETR failed: {} {}",
                reply.code, reply.text
            )));
        }

        // Open local file for writing (create/truncate).
        let mut file = match fs::File::create(local_name) {
            Ok(f) => f,
            Err(e) => {
                tcp_close(data_handle);
                return Err(FtpError::IoError(format!(
                    "open '{local_name}' failed: {e}"
                )));
            }
        };

        let mut total: u64 = 0;
        let mut buf = [0u8; 8192];
        let mut hash_count: u64 = 0;

        loop {
            let n = tcp_recv(data_handle, &mut buf)?;
            if n == 0 {
                break;
            }
            // Write to local file.
            if let Err(e) = file.write_all(&buf[..n]) {
                tcp_close(data_handle);
                return Err(FtpError::IoError(format!(
                    "write to local file failed: {e}"
                )));
            }
            total += n as u64;

            if self.hash_print {
                let new_count = total / 1024;
                while hash_count < new_count {
                    print!("#");
                    hash_count += 1;
                }
                let _ = io::stdout().flush();
            }
        }

        drop(file);
        tcp_close(data_handle);

        if self.hash_print {
            println!();
        }

        println!("{total} bytes received.");

        if self.bell {
            print!("\x07"); // BEL character
            let _ = io::stdout().flush();
        }

        // Read transfer-complete reply.
        let reply = self.read_reply()?;
        if reply.is_error() && !self.verbose {
            println!("{}", reply.text);
        }

        Ok(())
    }

    /// Upload a file (STOR).
    fn cmd_put(&mut self, local: &str, remote: Option<&str>) -> Result<(), FtpError> {
        self.require_login()?;

        let remote_name = remote.unwrap_or(local);
        // Extract just the filename if local contains a path.
        let remote_name = remote_name.rsplit('/').next().unwrap_or(remote_name);

        // Check that the local file exists and get its size.
        let local_size = file_size(local)?;

        let data_handle = self.open_data_connection()?;

        self.send_command(&format!("STOR {remote_name}"))?;
        let reply = self.read_reply()?;

        if !reply.is_preliminary() && !reply.is_positive() {
            tcp_close(data_handle);
            return Err(FtpError::ProtocolError(format!(
                "STOR failed: {} {}",
                reply.code, reply.text
            )));
        }

        // Open local file for reading.
        let mut file = match fs::File::open(local) {
            Ok(f) => f,
            Err(e) => {
                tcp_close(data_handle);
                return Err(FtpError::IoError(format!("open '{local}' failed: {e}")));
            }
        };

        let mut total: u64 = 0;
        let mut buf = [0u8; 8192];
        let mut hash_count: u64 = 0;

        loop {
            let n = match file.read(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    tcp_close(data_handle);
                    return Err(FtpError::IoError(format!(
                        "read from local file failed: {e}"
                    )));
                }
            };
            if n == 0 {
                break;
            }
            tcp_send_all(data_handle, &buf[..n])?;
            total += n as u64;

            if self.hash_print {
                let new_count = total / 1024;
                while hash_count < new_count {
                    print!("#");
                    hash_count += 1;
                }
                let _ = io::stdout().flush();
            }
        }

        drop(file);
        tcp_close(data_handle);

        if self.hash_print {
            println!();
        }

        println!("{total} bytes sent (file size: {local_size}).");

        if self.bell {
            print!("\x07");
            let _ = io::stdout().flush();
        }

        // Read transfer-complete reply.
        let reply = self.read_reply()?;
        if reply.is_error() && !self.verbose {
            println!("{}", reply.text);
        }

        Ok(())
    }

    /// Download multiple files matching a glob pattern.
    fn cmd_mget(&mut self, pattern: &str) -> Result<(), FtpError> {
        self.require_login()?;

        // Get remote file listing.
        let files = self.get_remote_file_list(None)?;

        let matching: Vec<&String> = if self.glob_enabled {
            files.iter().filter(|f| glob_match(pattern, f)).collect()
        } else {
            files.iter().filter(|f| f.as_str() == pattern).collect()
        };

        if matching.is_empty() {
            println!("No files match '{pattern}'.");
            return Ok(());
        }

        for filename in &matching {
            if self.interactive_prompt {
                print!("mget {filename}? ");
                let _ = io::stdout().flush();
                let mut answer = String::new();
                if io::stdin().read_line(&mut answer).is_ok() {
                    let answer = answer.trim().to_lowercase();
                    if answer != "y" && answer != "yes" {
                        continue;
                    }
                }
            }
            if let Err(e) = self.cmd_get(filename, None) {
                eprintln!("Error getting {filename}: {e}");
            }
        }

        Ok(())
    }

    /// Upload multiple files matching a glob pattern.
    fn cmd_mput(&mut self, pattern: &str) -> Result<(), FtpError> {
        self.require_login()?;

        // Get local file listing.
        let cwd = get_cwd().unwrap_or_else(|_| ".".to_string());
        let files = list_directory(&cwd)?;

        let matching: Vec<&String> = if self.glob_enabled {
            files.iter().filter(|f| glob_match(pattern, f)).collect()
        } else {
            files.iter().filter(|f| f.as_str() == pattern).collect()
        };

        if matching.is_empty() {
            println!("No local files match '{pattern}'.");
            return Ok(());
        }

        for filename in &matching {
            if self.interactive_prompt {
                print!("mput {filename}? ");
                let _ = io::stdout().flush();
                let mut answer = String::new();
                if io::stdin().read_line(&mut answer).is_ok() {
                    let answer = answer.trim().to_lowercase();
                    if answer != "y" && answer != "yes" {
                        continue;
                    }
                }
            }
            if let Err(e) = self.cmd_put(filename, None) {
                eprintln!("Error putting {filename}: {e}");
            }
        }

        Ok(())
    }

    /// Get a list of filenames in the current remote directory using NLST.
    fn get_remote_file_list(&mut self, path: Option<&str>) -> Result<Vec<String>, FtpError> {
        let data_handle = self.open_data_connection()?;

        let cmd = match path {
            Some(p) => format!("NLST {p}"),
            None => "NLST".to_string(),
        };
        self.send_command(&cmd)?;
        let reply = self.read_reply()?;

        if !reply.is_preliminary() && !reply.is_positive() {
            tcp_close(data_handle);
            // Empty listing is not an error for mget/mput.
            if reply.code == 450 || reply.code == 550 {
                return Ok(Vec::new());
            }
            return Err(FtpError::ProtocolError(format!(
                "NLST failed: {} {}",
                reply.code, reply.text
            )));
        }

        let mut data = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = tcp_recv(data_handle, &mut buf)?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
        }
        tcp_close(data_handle);

        // Read transfer-complete reply.
        let _reply = self.read_reply()?;

        // Parse filenames (one per line).
        let text = String::from_utf8_lossy(&data);
        let files: Vec<String> = text
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        Ok(files)
    }

    /// Create remote directory (MKD).
    fn cmd_mkdir(&mut self, path: &str) -> Result<(), FtpError> {
        self.require_login()?;
        self.send_command(&format!("MKD {path}"))?;
        let reply = self.read_reply()?;
        if reply.is_positive() {
            println!("Directory created: {path}");
        } else if !self.verbose {
            println!("mkdir failed: {} {}", reply.code, reply.text);
        }
        Ok(())
    }

    /// Remove remote directory (RMD).
    fn cmd_rmdir(&mut self, path: &str) -> Result<(), FtpError> {
        self.require_login()?;
        self.send_command(&format!("RMD {path}"))?;
        let reply = self.read_reply()?;
        if reply.is_positive() {
            println!("Directory removed: {path}");
        } else if !self.verbose {
            println!("rmdir failed: {} {}", reply.code, reply.text);
        }
        Ok(())
    }

    /// Delete remote file (DELE).
    fn cmd_delete(&mut self, path: &str) -> Result<(), FtpError> {
        self.require_login()?;
        self.send_command(&format!("DELE {path}"))?;
        let reply = self.read_reply()?;
        if reply.is_positive() {
            println!("Deleted: {path}");
        } else if !self.verbose {
            println!("delete failed: {} {}", reply.code, reply.text);
        }
        Ok(())
    }

    /// Rename remote file (RNFR + RNTO).
    fn cmd_rename(&mut self, from: &str, to: &str) -> Result<(), FtpError> {
        self.require_login()?;
        self.send_command(&format!("RNFR {from}"))?;
        let reply = self.read_reply()?;
        if !reply.is_intermediate() {
            if !self.verbose {
                println!("rename failed: {} {}", reply.code, reply.text);
            }
            return Ok(());
        }

        self.send_command(&format!("RNTO {to}"))?;
        let reply = self.read_reply()?;
        if reply.is_positive() {
            println!("Renamed {from} -> {to}");
        } else if !self.verbose {
            println!("rename failed: {} {}", reply.code, reply.text);
        }
        Ok(())
    }

    /// Show remote system type (SYST).
    fn cmd_system(&mut self) -> Result<(), FtpError> {
        self.require_login()?;
        self.send_command("SYST")?;
        let reply = self.read_reply()?;
        if !self.verbose {
            println!("{}", reply.text);
        }
        Ok(())
    }

    /// Show remote file size (SIZE).
    fn cmd_size(&mut self, path: &str) -> Result<(), FtpError> {
        self.require_login()?;
        self.send_command(&format!("SIZE {path}"))?;
        let reply = self.read_reply()?;
        if reply.is_positive() {
            // Reply is "213 <size>".
            let size_str = reply.text.trim_start_matches("213 ").trim();
            println!("{path}: {size_str} bytes");
        } else if !self.verbose {
            println!("size failed: {} {}", reply.code, reply.text);
        }
        Ok(())
    }

    /// Print connection status.
    fn cmd_status(&self) {
        if self.is_connected() {
            println!("Connected to {}.", self.hostname);
            if self.logged_in {
                println!("Logged in.");
            } else {
                println!("Not logged in.");
            }
        } else {
            println!("Not connected.");
        }
        println!(
            "Mode: {}; Type: {}; Verbose: {}; Debug: {}",
            if self.passive_mode {
                "passive"
            } else {
                "active"
            },
            self.transfer_type,
            if self.verbose { "on" } else { "off" },
            if self.debug { "on" } else { "off" },
        );
        println!(
            "Hash: {}; Bell: {}; Prompt: {}; Glob: {}",
            if self.hash_print { "on" } else { "off" },
            if self.bell { "on" } else { "off" },
            if self.interactive_prompt { "on" } else { "off" },
            if self.glob_enabled { "on" } else { "off" },
        );
    }

    /// Print help text.
    fn print_help() {
        println!("Commands:");
        println!("  open host [port]     Connect to FTP server");
        println!("  close                Close connection");
        println!("  quit / bye / exit    Exit FTP client");
        println!("  user [name]          Login with username");
        println!("  cd dir               Change remote directory");
        println!("  pwd                  Print remote directory");
        println!("  ls [path]            List directory (names only)");
        println!("  dir [path]           List directory (detailed)");
        println!("  get remote [local]   Download file");
        println!("  put local [remote]   Upload file");
        println!("  mget pattern         Download files matching glob");
        println!("  mput pattern         Upload files matching glob");
        println!("  mkdir dir            Create remote directory");
        println!("  rmdir dir            Remove remote directory");
        println!("  delete / rm file     Delete remote file");
        println!("  rename from to       Rename remote file");
        println!("  binary               Binary transfer mode");
        println!("  ascii                ASCII transfer mode");
        println!("  type                 Show transfer type");
        println!("  passive / pasv       Toggle passive mode");
        println!("  active               Switch to active mode");
        println!("  status               Show status");
        println!("  system               Show server system type");
        println!("  size file            Show file size");
        println!("  hash                 Toggle hash printing");
        println!("  bell                 Toggle bell on completion");
        println!("  verbose              Toggle verbose mode");
        println!("  debug                Toggle debug mode");
        println!("  lcd [dir]            Change local directory");
        println!("  lpwd                 Print local directory");
        println!("  prompt               Toggle mget/mput prompting");
        println!("  glob                 Toggle glob expansion");
        println!("  help / ?             Show this help");
    }
}

// ============================================================================
// REPL
// ============================================================================

/// Read a line from stdin, returning `None` on EOF.
fn read_line(prompt: &str) -> Option<String> {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(
            line.trim_end_matches('\n')
                .trim_end_matches('\r')
                .to_string(),
        ),
    }
}

/// Prompt for a password (no echo on a real terminal, but we cannot suppress
/// echo via raw syscalls easily, so we just print a prompt and read).
fn read_password(prompt: &str) -> Option<String> {
    // In a real terminal we would disable echo here. For now, just read a line.
    read_line(prompt)
}

/// Run the interactive FTP REPL.
fn run_repl(session: &mut FtpSession) {
    // `read_line` returns None on EOF, which ends the REPL.
    while let Some(line) = read_line("ftp> ") {
        let cmd = parse_command(&line);
        let result = execute_command(session, cmd);

        match result {
            Ok(should_quit) => {
                if should_quit {
                    break;
                }
            }
            Err(e) => {
                eprintln!("Error: {e}");
            }
        }
    }
}

/// Execute a single parsed command. Returns `Ok(true)` if the client should
/// exit, `Ok(false)` to continue, or `Err` on error.
fn execute_command(session: &mut FtpSession, cmd: Command) -> Result<bool, FtpError> {
    match cmd {
        Command::Open { host, port } => {
            session.connect(&host, port)?;
            // Auto-login with anonymous if no explicit user command follows.
            if session.is_connected() {
                // Prompt for username.
                let user = read_line("Name: ").unwrap_or_default();
                let user = if user.is_empty() {
                    "anonymous".to_string()
                } else {
                    user
                };
                let pass = if user == "anonymous" {
                    "user@slateos".to_string()
                } else {
                    read_password("Password: ").unwrap_or_default()
                };
                session.login(&user, &pass)?;
            }
        }
        Command::Close => {
            if session.is_connected() {
                session.disconnect();
                println!("Connection closed.");
            } else {
                println!("Not connected.");
            }
        }
        Command::Quit => {
            if session.is_connected() {
                session.disconnect();
            }
            println!("Goodbye.");
            return Ok(true);
        }
        Command::User { name } => {
            if !session.is_connected() {
                return Err(FtpError::NotConnected);
            }
            let user = match name {
                Some(n) => n,
                None => read_line("Name: ").unwrap_or_default(),
            };
            let user = if user.is_empty() {
                "anonymous".to_string()
            } else {
                user
            };
            let pass = if user == "anonymous" {
                "user@slateos".to_string()
            } else {
                read_password("Password: ").unwrap_or_default()
            };
            session.login(&user, &pass)?;
        }
        Command::Cd { path } => {
            session.cmd_cd(&path)?;
        }
        Command::Pwd => {
            session.cmd_pwd()?;
        }
        Command::Ls { path } => {
            session.cmd_list(path.as_deref(), false)?;
        }
        Command::Dir { path } => {
            session.cmd_list(path.as_deref(), true)?;
        }
        Command::Get { remote, local } => {
            session.cmd_get(&remote, local.as_deref())?;
        }
        Command::Put { local, remote } => {
            session.cmd_put(&local, remote.as_deref())?;
        }
        Command::Mget { pattern } => {
            session.cmd_mget(&pattern)?;
        }
        Command::Mput { pattern } => {
            session.cmd_mput(&pattern)?;
        }
        Command::Mkdir { path } => {
            session.cmd_mkdir(&path)?;
        }
        Command::Rmdir { path } => {
            session.cmd_rmdir(&path)?;
        }
        Command::Delete { path } => {
            session.cmd_delete(&path)?;
        }
        Command::Rename { from, to } => {
            session.cmd_rename(&from, &to)?;
        }
        Command::Binary => {
            session.require_login()?;
            session.set_type(TransferType::Binary)?;
        }
        Command::Ascii => {
            session.require_login()?;
            session.set_type(TransferType::Ascii)?;
        }
        Command::Type => {
            println!("Using {} mode for file transfers.", session.transfer_type);
        }
        Command::Passive => {
            session.passive_mode = !session.passive_mode;
            println!(
                "Passive mode {}.",
                if session.passive_mode { "on" } else { "off" }
            );
        }
        Command::Active => {
            session.passive_mode = false;
            println!("Active mode on (passive mode off).");
        }
        Command::Status => {
            session.cmd_status();
        }
        Command::System => {
            session.cmd_system()?;
        }
        Command::Size { path } => {
            session.cmd_size(&path)?;
        }
        Command::Hash => {
            session.hash_print = !session.hash_print;
            println!(
                "Hash mark printing {}.",
                if session.hash_print { "on" } else { "off" }
            );
        }
        Command::Bell => {
            session.bell = !session.bell;
            println!("Bell {}.", if session.bell { "on" } else { "off" });
        }
        Command::Verbose => {
            session.verbose = !session.verbose;
            println!(
                "Verbose mode {}.",
                if session.verbose { "on" } else { "off" }
            );
        }
        Command::Debug => {
            session.debug = !session.debug;
            if session.debug {
                session.verbose = true;
            }
            println!(
                "Debug mode {} (verbose {}).",
                if session.debug { "on" } else { "off" },
                if session.verbose { "on" } else { "off" },
            );
        }
        Command::Lcd { path } => {
            if let Some(p) = path {
                change_dir(&p)?;
                let cwd = get_cwd().unwrap_or_else(|_| p.clone());
                println!("Local directory now: {cwd}");
            } else {
                // No path = go to home / root.
                change_dir("/")?;
                println!("Local directory now: /");
            }
        }
        Command::Lpwd => {
            let cwd = get_cwd()?;
            println!("Local directory: {cwd}");
        }
        Command::Prompt => {
            session.interactive_prompt = !session.interactive_prompt;
            println!(
                "Interactive prompting {}.",
                if session.interactive_prompt {
                    "on"
                } else {
                    "off"
                }
            );
        }
        Command::Glob => {
            session.glob_enabled = !session.glob_enabled;
            println!(
                "Glob expansion {}.",
                if session.glob_enabled { "on" } else { "off" }
            );
        }
        Command::Help => {
            FtpSession::print_help();
        }
        Command::Empty => {}
        Command::Unknown(s) => {
            println!("Unknown command: {s}. Type 'help' for commands.");
        }
    }

    Ok(false)
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let cli = parse_args(&args[1..]);

    let mut session = FtpSession::new();
    session.verbose = cli.verbose;
    session.debug = cli.debug;
    session.passive_mode = cli.passive;

    // If a host was given on the command line, connect immediately.
    if let Some(host) = &cli.host {
        match session.connect(host, cli.port) {
            Ok(()) => {
                if cli.auto_login {
                    // Prompt for username.
                    let user = read_line("Name: ").unwrap_or_default();
                    let user = if user.is_empty() {
                        "anonymous".to_string()
                    } else {
                        user
                    };
                    let pass = if user == "anonymous" {
                        "user@slateos".to_string()
                    } else {
                        read_password("Password: ").unwrap_or_default()
                    };
                    if let Err(e) = session.login(&user, &pass) {
                        eprintln!("Login failed: {e}");
                    }
                }
            }
            Err(e) => {
                eprintln!("ftp: {e}");
            }
        }
    }

    run_repl(&mut session);

    if session.is_connected() {
        session.disconnect();
    }

    process::exit(0);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Reply parsing ----

    #[test]
    fn test_parse_reply_code_valid() {
        assert_eq!(parse_reply_code("220 Welcome"), Some(220));
        assert_eq!(parse_reply_code("331 Password required"), Some(331));
        assert_eq!(parse_reply_code("550 File not found"), Some(550));
        assert_eq!(parse_reply_code("150 Opening data connection"), Some(150));
        assert_eq!(parse_reply_code("226 Transfer complete"), Some(226));
    }

    #[test]
    fn test_parse_reply_code_edge_cases() {
        assert_eq!(parse_reply_code("100 Minimum"), Some(100));
        assert_eq!(parse_reply_code("599 Maximum"), Some(599));
        assert_eq!(parse_reply_code("200"), Some(200));
    }

    #[test]
    fn test_parse_reply_code_invalid() {
        assert_eq!(parse_reply_code(""), None);
        assert_eq!(parse_reply_code("ab"), None);
        assert_eq!(parse_reply_code("abc def"), None);
        assert_eq!(parse_reply_code("99 Too low"), None);
        assert_eq!(parse_reply_code("600 Too high"), None);
        assert_eq!(parse_reply_code("12"), None);
    }

    #[test]
    fn test_is_final_reply_line_single() {
        assert!(is_final_reply_line("220 Welcome to FTP", 220));
        assert!(is_final_reply_line("226 Transfer complete", 226));
    }

    #[test]
    fn test_is_final_reply_line_multiline() {
        // Continuation line (hyphen after code).
        assert!(!is_final_reply_line("220-Welcome", 220));
        assert!(!is_final_reply_line("220-This is line 2", 220));
        // Final line (space after code).
        assert!(is_final_reply_line("220 End of greeting", 220));
    }

    #[test]
    fn test_is_final_reply_line_wrong_code() {
        assert!(!is_final_reply_line("221 Goodbye", 220));
        assert!(!is_final_reply_line("331 Password needed", 220));
    }

    #[test]
    fn test_is_final_reply_line_code_only() {
        // A line with just the 3-digit code and no separator.
        assert!(is_final_reply_line("220", 220));
    }

    #[test]
    fn test_reply_is_positive() {
        let r = FtpReply {
            code: 200,
            text: "OK".to_string(),
        };
        assert!(r.is_positive());
        assert!(!r.is_preliminary());
        assert!(!r.is_intermediate());
        assert!(!r.is_error());
    }

    #[test]
    fn test_reply_is_preliminary() {
        let r = FtpReply {
            code: 150,
            text: "Opening data connection".to_string(),
        };
        assert!(r.is_preliminary());
        assert!(!r.is_positive());
    }

    #[test]
    fn test_reply_is_intermediate() {
        let r = FtpReply {
            code: 331,
            text: "Password required".to_string(),
        };
        assert!(r.is_intermediate());
        assert!(!r.is_positive());
        assert!(!r.is_error());
    }

    #[test]
    fn test_reply_is_transient_error() {
        let r = FtpReply {
            code: 421,
            text: "Service not available".to_string(),
        };
        assert!(r.is_transient_error());
        assert!(r.is_error());
        assert!(!r.is_permanent_error());
    }

    #[test]
    fn test_reply_is_permanent_error() {
        let r = FtpReply {
            code: 550,
            text: "File not found".to_string(),
        };
        assert!(r.is_permanent_error());
        assert!(r.is_error());
        assert!(!r.is_transient_error());
    }

    // ---- PASV response parsing ----

    #[test]
    fn test_parse_pasv_standard() {
        let text = "227 Entering Passive Mode (192,168,1,1,4,1)";
        // IP: 192.168.1.1 in network byte order. Port: 4*256 + 1 = 1025.
        let expected_ip = u32::from_be_bytes([192, 168, 1, 1]);
        assert_eq!(parse_pasv_response(text), Some((expected_ip, 1025)));
    }

    #[test]
    fn test_parse_pasv_high_port() {
        let text = "227 Entering Passive Mode (10,0,0,1,200,100)";
        let expected_ip = u32::from_be_bytes([10, 0, 0, 1]);
        assert_eq!(
            parse_pasv_response(text),
            Some((expected_ip, 200 * 256 + 100))
        );
    }

    #[test]
    fn test_parse_pasv_with_spaces() {
        let text = "227 Entering Passive Mode ( 10, 0, 0, 1, 200, 100 )";
        let expected_ip = u32::from_be_bytes([10, 0, 0, 1]);
        assert_eq!(
            parse_pasv_response(text),
            Some((expected_ip, 200 * 256 + 100))
        );
    }

    #[test]
    fn test_parse_pasv_localhost() {
        let text = "227 Entering Passive Mode (127,0,0,1,0,21)";
        let expected_ip = u32::from_be_bytes([127, 0, 0, 1]);
        assert_eq!(parse_pasv_response(text), Some((expected_ip, 21)));
    }

    #[test]
    fn test_parse_pasv_invalid_no_parens() {
        assert!(parse_pasv_response("227 Entering Passive Mode 1,2,3,4,5,6").is_none());
    }

    #[test]
    fn test_parse_pasv_invalid_too_few() {
        assert!(parse_pasv_response("227 Entering Passive Mode (1,2,3,4,5)").is_none());
    }

    #[test]
    fn test_parse_pasv_invalid_non_numeric() {
        assert!(parse_pasv_response("227 Entering Passive Mode (a,b,c,d,e,f)").is_none());
    }

    // ---- Mode switching ----

    #[test]
    fn test_session_default_passive() {
        let s = FtpSession::new();
        assert!(s.passive_mode);
    }

    #[test]
    fn test_session_default_transfer_type() {
        let s = FtpSession::new();
        assert_eq!(s.transfer_type, TransferType::Binary);
    }

    #[test]
    fn test_session_not_connected() {
        let s = FtpSession::new();
        assert!(!s.is_connected());
        assert!(!s.logged_in);
    }

    // ---- Command parsing ----

    #[test]
    fn test_parse_command_open() {
        assert_eq!(
            parse_command("open ftp.example.com"),
            Command::Open {
                host: "ftp.example.com".to_string(),
                port: 21,
            }
        );
    }

    #[test]
    fn test_parse_command_open_with_port() {
        assert_eq!(
            parse_command("open ftp.example.com 2121"),
            Command::Open {
                host: "ftp.example.com".to_string(),
                port: 2121,
            }
        );
    }

    #[test]
    fn test_parse_command_quit_variants() {
        assert_eq!(parse_command("quit"), Command::Quit);
        assert_eq!(parse_command("bye"), Command::Quit);
        assert_eq!(parse_command("exit"), Command::Quit);
    }

    #[test]
    fn test_parse_command_get() {
        assert_eq!(
            parse_command("get remote.txt"),
            Command::Get {
                remote: "remote.txt".to_string(),
                local: None,
            }
        );
    }

    #[test]
    fn test_parse_command_get_with_local() {
        assert_eq!(
            parse_command("get remote.txt local.txt"),
            Command::Get {
                remote: "remote.txt".to_string(),
                local: Some("local.txt".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_command_put() {
        assert_eq!(
            parse_command("put local.txt"),
            Command::Put {
                local: "local.txt".to_string(),
                remote: None,
            }
        );
    }

    #[test]
    fn test_parse_command_rename() {
        assert_eq!(
            parse_command("rename old.txt new.txt"),
            Command::Rename {
                from: "old.txt".to_string(),
                to: "new.txt".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_command_aliases() {
        // recv is alias for get
        assert_eq!(
            parse_command("recv file.txt"),
            Command::Get {
                remote: "file.txt".to_string(),
                local: None,
            }
        );
        // send is alias for put
        assert_eq!(
            parse_command("send file.txt"),
            Command::Put {
                local: "file.txt".to_string(),
                remote: None,
            }
        );
        // rm is alias for delete
        assert!(matches!(
            parse_command("rm file.txt"),
            Command::Delete { .. }
        ));
    }

    #[test]
    fn test_parse_command_toggles() {
        assert_eq!(parse_command("binary"), Command::Binary);
        assert_eq!(parse_command("ascii"), Command::Ascii);
        assert_eq!(parse_command("type"), Command::Type);
        assert_eq!(parse_command("passive"), Command::Passive);
        assert_eq!(parse_command("active"), Command::Active);
        assert_eq!(parse_command("hash"), Command::Hash);
        assert_eq!(parse_command("bell"), Command::Bell);
        assert_eq!(parse_command("verbose"), Command::Verbose);
        assert_eq!(parse_command("debug"), Command::Debug);
        assert_eq!(parse_command("prompt"), Command::Prompt);
        assert_eq!(parse_command("glob"), Command::Glob);
    }

    #[test]
    fn test_parse_command_case_insensitive() {
        assert_eq!(
            parse_command("OPEN ftp.example.com"),
            Command::Open {
                host: "ftp.example.com".to_string(),
                port: 21,
            }
        );
        assert_eq!(parse_command("QUIT"), Command::Quit);
        assert_eq!(parse_command("Binary"), Command::Binary);
    }

    #[test]
    fn test_parse_command_empty() {
        assert_eq!(parse_command(""), Command::Empty);
        assert_eq!(parse_command("   "), Command::Empty);
    }

    #[test]
    fn test_parse_command_unknown() {
        assert!(matches!(parse_command("foobar"), Command::Unknown(_)));
    }

    #[test]
    fn test_parse_command_missing_args() {
        // These should return Unknown with error message.
        assert!(matches!(parse_command("cd"), Command::Unknown(_)));
        assert!(matches!(parse_command("get"), Command::Unknown(_)));
        assert!(matches!(parse_command("put"), Command::Unknown(_)));
        assert!(matches!(parse_command("mget"), Command::Unknown(_)));
        assert!(matches!(parse_command("mput"), Command::Unknown(_)));
        assert!(matches!(parse_command("mkdir"), Command::Unknown(_)));
        assert!(matches!(parse_command("rmdir"), Command::Unknown(_)));
        assert!(matches!(parse_command("delete"), Command::Unknown(_)));
        assert!(matches!(parse_command("rename"), Command::Unknown(_)));
        assert!(matches!(
            parse_command("rename onlyOne"),
            Command::Unknown(_)
        ));
        assert!(matches!(parse_command("size"), Command::Unknown(_)));
    }

    // ---- Glob matching ----

    #[test]
    fn test_glob_exact_match() {
        assert!(glob_match("hello.txt", "hello.txt"));
        assert!(!glob_match("hello.txt", "hello.log"));
    }

    #[test]
    fn test_glob_star() {
        assert!(glob_match("*.txt", "hello.txt"));
        assert!(glob_match("*.txt", ".txt"));
        assert!(!glob_match("*.txt", "hello.log"));
        assert!(glob_match("hello.*", "hello.txt"));
        assert!(glob_match("hello.*", "hello."));
    }

    #[test]
    fn test_glob_question_mark() {
        assert!(glob_match("h?llo", "hello"));
        assert!(glob_match("h?llo", "hallo"));
        assert!(!glob_match("h?llo", "hllo"));
        assert!(!glob_match("h?llo", "heello"));
    }

    #[test]
    fn test_glob_complex() {
        assert!(glob_match("*.t?t", "file.txt"));
        assert!(glob_match("*.t?t", "file.tAt"));
        assert!(!glob_match("*.t?t", "file.text"));
        assert!(glob_match("*.*", "file.txt"));
        assert!(!glob_match("*.*", "noext"));
    }

    #[test]
    fn test_glob_star_only() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn test_glob_empty_pattern() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "notempty"));
    }

    #[test]
    fn test_glob_multiple_stars() {
        assert!(glob_match("*a*b*", "xaxbx"));
        assert!(glob_match("*a*b*", "ab"));
        assert!(glob_match("*a*b*", "aabb"));
        assert!(!glob_match("*a*b*", "ba"));
    }

    // ---- CLI args parsing ----

    #[test]
    fn test_parse_args_defaults() {
        let args: Vec<String> = vec![];
        let cli = parse_args(&args);
        assert!(cli.host.is_none());
        assert_eq!(cli.port, 21);
        assert!(cli.auto_login);
        assert!(!cli.verbose);
        assert!(!cli.debug);
        assert!(cli.passive);
    }

    #[test]
    fn test_parse_args_host() {
        let args: Vec<String> = vec!["ftp.example.com".to_string()];
        let cli = parse_args(&args);
        assert_eq!(cli.host.as_deref(), Some("ftp.example.com"));
        assert_eq!(cli.port, 21);
    }

    #[test]
    fn test_parse_args_host_and_port() {
        let args = vec!["ftp.example.com".to_string(), "2121".to_string()];
        let cli = parse_args(&args);
        assert_eq!(cli.host.as_deref(), Some("ftp.example.com"));
        assert_eq!(cli.port, 2121);
    }

    #[test]
    fn test_parse_args_flags() {
        let args = vec!["-n".to_string(), "-v".to_string(), "-a".to_string()];
        let cli = parse_args(&args);
        assert!(!cli.auto_login);
        assert!(cli.verbose);
        assert!(!cli.passive);
    }

    #[test]
    fn test_parse_args_debug_implies_verbose() {
        let args = vec!["-d".to_string()];
        let cli = parse_args(&args);
        assert!(cli.debug);
        assert!(cli.verbose);
    }

    #[test]
    fn test_parse_args_combined_flags() {
        let args = vec!["-nv".to_string(), "myhost".to_string()];
        let cli = parse_args(&args);
        assert!(!cli.auto_login);
        assert!(cli.verbose);
        assert_eq!(cli.host.as_deref(), Some("myhost"));
    }

    // ---- Transfer type display ----

    #[test]
    fn test_transfer_type_display() {
        assert_eq!(format!("{}", TransferType::Ascii), "ascii");
        assert_eq!(format!("{}", TransferType::Binary), "binary");
    }

    // ---- Error display ----

    #[test]
    fn test_error_display() {
        let e = FtpError::DnsFailure("example.com".to_string());
        let msg = format!("{e}");
        assert!(msg.contains("example.com"));

        let e = FtpError::NotConnected;
        assert_eq!(format!("{e}"), "Not connected");

        let e = FtpError::NotLoggedIn;
        assert_eq!(format!("{e}"), "Not logged in");
    }
}
