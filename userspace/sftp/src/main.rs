//! OurOS SFTP Client
//!
//! An interactive and batch-mode SFTP client that implements SFTP protocol v3
//! (SSH File Transfer Protocol, draft-ietf-secsh-filexfer-02).
//!
//! # Modes
//!
//! **Local mode** — navigate and transfer files between local directories.
//! Useful for testing the full command set without a live SSH connection.
//!
//! **Remote mode** — connect to an SFTP server via TCP on port 22 (or custom
//! port). The client speaks the SFTP subsystem wire protocol directly over a
//! raw TCP connection (a real deployment would layer this over SSH; here the
//! transport is the OurOS TCP syscall layer).
//!
//! # Usage
//!
//! ```text
//! sftp [user@]host[:path]          Connect to remote host
//! sftp -P 2222 user@host           Connect on custom port
//! sftp -v user@host                Verbose protocol debugging
//! sftp -b batchfile user@host      Batch mode: run commands from file
//! sftp                             Local-only interactive session
//! ```
//!
//! # Interactive Commands
//!
//! ls [path], cd path, pwd, lcd path, lpwd, get remote [local],
//! put local [remote], mget pattern, mput pattern, mkdir dir, rmdir dir,
//! rm file, rename old new, chmod mode file, stat file, lstat file,
//! !command, help/?, bye/quit/exit

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]

use std::env;
use std::fmt;
use std::io::{self, BufRead, Read, Write};

// ============================================================================
// Syscall numbers
// ============================================================================

const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_STAT: u64 = 4;
const SYS_MKDIR: u64 = 83;
const SYS_UNLINK: u64 = 87;
const SYS_GETDENTS: u64 = 78;
const SYS_GETCWD: u64 = 79;
const SYS_CHDIR: u64 = 80;
const SYS_RENAME: u64 = 82;
const SYS_CHMOD: u64 = 90;
const SYS_TCP_CONNECT: u64 = 800;
const SYS_TCP_SEND: u64 = 802;
const SYS_TCP_RECV: u64 = 803;
const SYS_TCP_CLOSE: u64 = 804;

// Open flags
const O_RDONLY: u64 = 0;
const O_WRONLY: u64 = 1;
const O_CREAT: u64 = 0o100;
const O_TRUNC: u64 = 0o1000;

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

/// Issue a 2-argument raw syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for that syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall2(nr: u64, a1: u64, a2: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
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

// Non-x86_64 stub so the crate compiles for host-side `cargo test`.
#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall1(_nr: u64, _a1: u64) -> i64 {
    -1
}
#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall2(_nr: u64, _a1: u64, _a2: u64) -> i64 {
    -1
}
#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 {
    -1
}

// ============================================================================
// Raw syscall wrappers
// ============================================================================

/// Kernel `stat` result layout (matches x86_64 Linux stat64).
#[repr(C)]
#[derive(Default, Clone)]
struct KernelStat {
    st_dev: u64,
    st_ino: u64,
    st_nlink: u64,
    st_mode: u32,
    st_uid: u32,
    st_gid: u32,
    _pad0: u32,
    st_rdev: u64,
    st_size: i64,
    st_blksize: i64,
    st_blocks: i64,
    st_atime: i64,
    st_atime_ns: u64,
    st_mtime: i64,
    st_mtime_ns: u64,
    st_ctime: i64,
    st_ctime_ns: u64,
    _reserved: [i64; 3],
}

/// Kernel `getdents64` directory entry.
#[repr(C)]
struct KernelDirent64 {
    d_ino: u64,
    d_off: i64,
    d_reclen: u16,
    d_type: u8,
    // d_name follows as variable-length null-terminated string
}

const DT_DIR: u8 = 4;
const DT_LNK: u8 = 10;

/// Stat a path (follows symlinks).
fn os_stat(path: &str) -> Result<KernelStat, OsError> {
    let cpath = to_cstring(path);
    let mut st = KernelStat::default();
    // SAFETY: cpath is a valid null-terminated C string; st is a valid writable
    // KernelStat-sized buffer.
    let ret = unsafe {
        syscall2(
            SYS_STAT,
            cpath.as_ptr() as u64,
            &mut st as *mut KernelStat as u64,
        )
    };
    if ret < 0 {
        return Err(OsError::Syscall("stat", ret));
    }
    Ok(st)
}

/// Open a file; returns an fd on success.
fn os_open(path: &str, flags: u64, mode: u32) -> Result<i64, OsError> {
    let cpath = to_cstring(path);
    // SAFETY: cpath is valid; flags and mode are plain integers.
    let ret = unsafe { syscall3(SYS_OPEN, cpath.as_ptr() as u64, flags, u64::from(mode)) };
    if ret < 0 {
        return Err(OsError::Syscall("open", ret));
    }
    Ok(ret)
}

/// Close a file descriptor. Ignores errors (fd becomes invalid either way).
fn os_close(fd: i64) {
    // SAFETY: We pass a valid (or already-invalid) fd; worst case the syscall
    // returns an error which we intentionally ignore.
    let _ = unsafe { syscall1(SYS_CLOSE, fd as u64) };
}

/// Read up to `buf.len()` bytes from `fd`. Returns bytes read (0 = EOF).
fn os_read(fd: i64, buf: &mut [u8]) -> Result<usize, OsError> {
    // SAFETY: buf is a valid writable buffer; fd was opened successfully.
    let ret = unsafe {
        syscall3(
            SYS_READ,
            fd as u64,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(OsError::Syscall("read", ret));
    }
    Ok(ret as usize)
}

/// Write `buf` to `fd`. Returns bytes written.
fn os_write(fd: i64, buf: &[u8]) -> Result<usize, OsError> {
    // SAFETY: buf is a valid read-only buffer; fd was opened successfully.
    let ret = unsafe {
        syscall3(
            SYS_WRITE,
            fd as u64,
            buf.as_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(OsError::Syscall("write", ret));
    }
    Ok(ret as usize)
}

/// Write all of `buf` to `fd`, looping until done.
fn os_write_all(fd: i64, buf: &[u8]) -> Result<(), OsError> {
    let mut offset = 0usize;
    while offset < buf.len() {
        let n = os_write(fd, &buf[offset..])?;
        if n == 0 {
            return Err(OsError::Syscall("write", -1));
        }
        offset = offset.checked_add(n).ok_or(OsError::Syscall("write", -1))?;
    }
    Ok(())
}

/// Read file contents into a Vec, looping until EOF.
fn os_read_file(path: &str) -> Result<Vec<u8>, OsError> {
    let fd = os_open(path, O_RDONLY, 0)?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = match os_read(fd, &mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                os_close(fd);
                return Err(e);
            }
        };
        buf.extend_from_slice(&chunk[..n]);
    }
    os_close(fd);
    Ok(buf)
}

/// Write a byte slice to a file, creating or truncating it.
fn os_write_file(path: &str, data: &[u8]) -> Result<(), OsError> {
    let fd = os_open(path, O_WRONLY | O_CREAT | O_TRUNC, 0o644)?;
    let result = os_write_all(fd, data);
    os_close(fd);
    result
}

/// Create a directory (mode 0755).
fn os_mkdir(path: &str) -> Result<(), OsError> {
    let cpath = to_cstring(path);
    // SAFETY: cpath is a valid null-terminated path string.
    let ret = unsafe { syscall2(SYS_MKDIR, cpath.as_ptr() as u64, 0o755) };
    if ret < 0 {
        return Err(OsError::Syscall("mkdir", ret));
    }
    Ok(())
}

/// Remove a file.
fn os_unlink(path: &str) -> Result<(), OsError> {
    let cpath = to_cstring(path);
    // SAFETY: cpath is a valid null-terminated path string.
    let ret = unsafe { syscall1(SYS_UNLINK, cpath.as_ptr() as u64) };
    if ret < 0 {
        return Err(OsError::Syscall("unlink", ret));
    }
    Ok(())
}

/// Rename / move a file or directory.
fn os_rename(old: &str, new: &str) -> Result<(), OsError> {
    let cold = to_cstring(old);
    let cnew = to_cstring(new);
    // SAFETY: Both paths are valid null-terminated C strings.
    let ret = unsafe { syscall2(SYS_RENAME, cold.as_ptr() as u64, cnew.as_ptr() as u64) };
    if ret < 0 {
        return Err(OsError::Syscall("rename", ret));
    }
    Ok(())
}

/// Change file permissions.
fn os_chmod(path: &str, mode: u32) -> Result<(), OsError> {
    let cpath = to_cstring(path);
    // SAFETY: cpath is a valid null-terminated path; mode is a plain integer.
    let ret = unsafe { syscall2(SYS_CHMOD, cpath.as_ptr() as u64, u64::from(mode)) };
    if ret < 0 {
        return Err(OsError::Syscall("chmod", ret));
    }
    Ok(())
}

/// Change the working directory.
fn os_chdir(path: &str) -> Result<(), OsError> {
    let cpath = to_cstring(path);
    // SAFETY: cpath is a valid null-terminated path string.
    let ret = unsafe { syscall1(SYS_CHDIR, cpath.as_ptr() as u64) };
    if ret < 0 {
        return Err(OsError::Syscall("chdir", ret));
    }
    Ok(())
}

/// Get the current working directory as a UTF-8 string.
fn os_getcwd() -> Result<String, OsError> {
    let mut buf = vec![0u8; 4096];
    // SAFETY: buf is a valid writable buffer of the given length.
    let ret = unsafe {
        syscall2(
            SYS_GETCWD,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(OsError::Syscall("getcwd", ret));
    }
    // Trim the null terminator and convert to String.
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..end].to_vec()).map_err(|_| OsError::BadUtf8)
}

/// Read directory entries. Returns a list of (name, is_dir, size) tuples.
fn os_readdir(path: &str) -> Result<Vec<DirEntry>, OsError> {
    let fd = os_open(path, O_RDONLY, 0)?;
    let mut entries = Vec::new();
    let mut buf = vec![0u8; 4096];

    loop {
        // SAFETY: buf is a valid writable buffer; fd is an open directory fd.
        let ret = unsafe {
            syscall3(
                SYS_GETDENTS,
                fd as u64,
                buf.as_mut_ptr() as u64,
                buf.len() as u64,
            )
        };
        if ret < 0 {
            os_close(fd);
            return Err(OsError::Syscall("getdents", ret));
        }
        if ret == 0 {
            break;
        }
        let nbytes = ret as usize;
        let mut offset = 0usize;
        while offset < nbytes {
            if offset.checked_add(core::mem::size_of::<KernelDirent64>()).map_or(true, |end| end > nbytes) {
                break;
            }
            // SAFETY: We checked that there are enough bytes for the header.
            let dirent = unsafe { &*(buf.as_ptr().add(offset) as *const KernelDirent64) };
            let reclen = dirent.d_reclen as usize;
            if reclen == 0 || offset.checked_add(reclen).map_or(true, |end| end > nbytes) {
                break;
            }
            // Name starts immediately after the fixed header.
            let name_offset = offset + core::mem::size_of::<KernelDirent64>();
            let name_end = buf[name_offset..offset + reclen]
                .iter()
                .position(|&b| b == 0)
                .map(|p| name_offset + p)
                .unwrap_or(offset + reclen);
            if let Ok(name) = std::str::from_utf8(&buf[name_offset..name_end]) {
                if name != "." && name != ".." {
                    let is_dir = dirent.d_type == DT_DIR;
                    let is_link = dirent.d_type == DT_LNK;
                    let full = format!("{path}/{name}");
                    let size = os_stat(&full).map(|s| s.st_size as u64).unwrap_or(0);
                    entries.push(DirEntry {
                        name: name.to_string(),
                        is_dir,
                        is_link,
                        size,
                    });
                }
            }
            offset = offset.checked_add(reclen).unwrap_or(nbytes);
        }
    }
    os_close(fd);
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

// ============================================================================
// TCP helpers
// ============================================================================

/// Open a TCP connection. Returns a handle on success.
fn tcp_connect(ip: u32, port: u16) -> Result<u64, SftpError> {
    // SAFETY: ip and port are plain integers; no pointers involved.
    let ret = unsafe { syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), 0) };
    if ret < 0 {
        return Err(SftpError::ConnectionFailed(format!(
            "tcp_connect returned {ret}"
        )));
    }
    Ok(ret as u64)
}

/// Send data on a TCP handle. Returns bytes sent.
fn tcp_send(handle: u64, data: &[u8]) -> Result<usize, SftpError> {
    // SAFETY: data is a valid read-only buffer with its correct length.
    let ret = unsafe {
        syscall3(
            SYS_TCP_SEND,
            handle,
            data.as_ptr() as u64,
            data.len() as u64,
        )
    };
    if ret < 0 {
        return Err(SftpError::NetworkError("send failed".into()));
    }
    Ok(ret as usize)
}

/// Send all bytes on a TCP handle.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), SftpError> {
    let mut offset = 0usize;
    while offset < data.len() {
        let n = tcp_send(handle, &data[offset..])?;
        if n == 0 {
            return Err(SftpError::NetworkError("connection closed".into()));
        }
        offset = offset.checked_add(n).ok_or_else(|| SftpError::NetworkError("overflow".into()))?;
    }
    Ok(())
}

/// Receive data from a TCP handle. Returns bytes received (0 = peer closed).
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, SftpError> {
    // SAFETY: buf is a valid writable buffer with its correct length.
    let ret = unsafe {
        syscall3(
            SYS_TCP_RECV,
            handle,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(SftpError::NetworkError("recv failed".into()));
    }
    Ok(ret as usize)
}

/// Receive exactly `n` bytes from a TCP handle.
fn tcp_recv_exact(handle: u64, buf: &mut [u8]) -> Result<(), SftpError> {
    let mut offset = 0usize;
    while offset < buf.len() {
        let n = tcp_recv(handle, &mut buf[offset..])?;
        if n == 0 {
            return Err(SftpError::NetworkError("connection closed mid-read".into()));
        }
        offset = offset.checked_add(n).ok_or_else(|| SftpError::NetworkError("overflow".into()))?;
    }
    Ok(())
}

/// Close a TCP handle.
fn tcp_close(handle: u64) {
    // SAFETY: We pass the handle as-is. Errors are intentionally ignored —
    // the handle becomes invalid regardless of whether the syscall succeeds.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

// ============================================================================
// Helper utilities
// ============================================================================

/// Append a null byte to a string slice and return a Vec<u8> suitable as a
/// C string pointer. The lifetime of the returned Vec must exceed any syscall
/// that uses its pointer.
fn to_cstring(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

/// Parse a simple octal number string (e.g. "644", "755") into a u32.
fn parse_octal(s: &str) -> Option<u32> {
    u32::from_str_radix(s, 8).ok()
}

/// Match a glob pattern against a file name. Supports `*` (any sequence) and
/// `?` (any single character). No path separators in pattern.
fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    glob_match_inner(&p, &n)
}

fn glob_match_inner(p: &[char], n: &[char]) -> bool {
    match (p.first(), n.first()) {
        (None, None) => true,
        (None, _) => false,
        (Some(&'*'), _) => {
            // '*' matches zero or more characters
            if glob_match_inner(&p[1..], n) {
                return true;
            }
            if n.is_empty() {
                return false;
            }
            glob_match_inner(p, &n[1..])
        }
        (Some(&'?'), Some(_)) => glob_match_inner(&p[1..], &n[1..]),
        (Some(&'?'), None) => false,
        (Some(pc), Some(nc)) => {
            if pc == nc {
                glob_match_inner(&p[1..], &n[1..])
            } else {
                false
            }
        }
        (Some(_), None) => false,
    }
}

/// Format a file size with human-readable suffix.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}M", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}K", bytes as f64 / KB as f64)
    } else {
        format!("{bytes}B")
    }
}

/// Format Unix permissions mode as `drwxrwxrwx`-style string.
fn format_mode(mode: u32) -> String {
    let ftype = match (mode >> 12) & 0xF {
        0x4 => 'd',
        0xA => 'l',
        _ => '-',
    };
    let bits = [
        (mode & 0o400 != 0, 'r'),
        (mode & 0o200 != 0, 'w'),
        (mode & 0o100 != 0, 'x'),
        (mode & 0o040 != 0, 'r'),
        (mode & 0o020 != 0, 'w'),
        (mode & 0o010 != 0, 'x'),
        (mode & 0o004 != 0, 'r'),
        (mode & 0o002 != 0, 'w'),
        (mode & 0o001 != 0, 'x'),
    ];
    let mut s = String::with_capacity(10);
    s.push(ftype);
    for (set, c) in &bits {
        s.push(if *set { *c } else { '-' });
    }
    s
}

/// Resolve a path relative to a base directory. Handles absolute paths and `..`.
fn resolve_path(base: &str, path: &str) -> String {
    if path.starts_with('/') {
        normalise_path(path)
    } else {
        normalise_path(&format!("{base}/{path}"))
    }
}

/// Collapse `.` and `..` components in a POSIX path.
fn normalise_path(path: &str) -> String {
    let mut components: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            p => components.push(p),
        }
    }
    if components.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", components.join("/"))
    }
}

/// Print a progress bar for file transfers.
fn print_progress(filename: &str, transferred: u64, total: u64) {
    if total == 0 {
        return;
    }
    let pct = (transferred * 100) / total;
    let filled = (pct as usize * 40) / 100;
    let bar: String = std::iter::repeat('#').take(filled)
        .chain(std::iter::repeat(' ').take(40usize.saturating_sub(filled)))
        .collect();
    print!(
        "\r{filename:<20} [{bar}] {:>3}% {}/{}   ",
        pct,
        format_size(transferred),
        format_size(total)
    );
    let _ = io::stdout().flush();
}

// ============================================================================
// Error types
// ============================================================================

/// Low-level OS / syscall errors.
#[derive(Debug)]
enum OsError {
    Syscall(&'static str, i64),
    BadUtf8,
}

impl fmt::Display for OsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syscall(name, code) => write!(f, "{name} failed (code {code})"),
            Self::BadUtf8 => write!(f, "path contains invalid UTF-8"),
        }
    }
}

/// SFTP client errors.
#[derive(Debug)]
enum SftpError {
    /// A local OS / syscall error.
    Os(OsError),
    /// A network / TCP error.
    NetworkError(String),
    /// Connection to remote host failed.
    ConnectionFailed(String),
    /// SFTP protocol error.
    Protocol(String),
    /// Remote server returned an error status.
    Remote { code: u32, message: String },
    /// Bad argument supplied by the user.
    BadArg(String),
    /// I/O error from the std library (used in non-no_std paths).
    Io(io::Error),
}

impl fmt::Display for SftpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Os(e) => write!(f, "os error: {e}"),
            Self::NetworkError(msg) => write!(f, "network error: {msg}"),
            Self::ConnectionFailed(msg) => write!(f, "connection failed: {msg}"),
            Self::Protocol(msg) => write!(f, "protocol error: {msg}"),
            Self::Remote { code, message } => write!(f, "remote error {code}: {message}"),
            Self::BadArg(msg) => write!(f, "bad argument: {msg}"),
            Self::Io(e) => write!(f, "i/o error: {e}"),
        }
    }
}

impl From<OsError> for SftpError {
    fn from(e: OsError) -> Self {
        Self::Os(e)
    }
}

impl From<io::Error> for SftpError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

// ============================================================================
// Directory entry type
// ============================================================================

/// A single entry returned by directory listing.
#[derive(Debug, Clone)]
struct DirEntry {
    name: String,
    is_dir: bool,
    is_link: bool,
    size: u64,
}

// ============================================================================
// SFTP protocol v3 constants and packet building
// ============================================================================

/// SFTP packet type codes (draft-ietf-secsh-filexfer-02).
mod fxp {
    pub const SSH_FXP_INIT: u8 = 1;
    pub const SSH_FXP_VERSION: u8 = 2;
    pub const SSH_FXP_OPEN: u8 = 3;
    pub const SSH_FXP_CLOSE: u8 = 4;
    pub const SSH_FXP_READ: u8 = 5;
    pub const SSH_FXP_WRITE: u8 = 6;
    pub const SSH_FXP_LSTAT: u8 = 7;
    pub const SSH_FXP_STAT: u8 = 17;
    pub const SSH_FXP_SETSTAT: u8 = 9;
    pub const SSH_FXP_OPENDIR: u8 = 11;
    pub const SSH_FXP_READDIR: u8 = 12;
    pub const SSH_FXP_REMOVE: u8 = 13;
    pub const SSH_FXP_MKDIR: u8 = 14;
    pub const SSH_FXP_RMDIR: u8 = 15;
    pub const SSH_FXP_RENAME: u8 = 18;
    pub const SSH_FXP_STATUS: u8 = 101;
    pub const SSH_FXP_HANDLE: u8 = 102;
    pub const SSH_FXP_DATA: u8 = 103;
    pub const SSH_FXP_NAME: u8 = 104;
    pub const SSH_FXP_ATTRS: u8 = 105;
}

/// SFTP status codes.
mod status {
    pub const SSH_FX_OK: u32 = 0;
    pub const SSH_FX_EOF: u32 = 1;
    // Reserved for future error-reporting use; kept for protocol completeness.
    #[allow(dead_code)]
    pub const SSH_FX_NO_SUCH_FILE: u32 = 2;
    #[allow(dead_code)]
    pub const SSH_FX_PERMISSION_DENIED: u32 = 3;
    #[allow(dead_code)]
    pub const SSH_FX_FAILURE: u32 = 4;
    #[allow(dead_code)]
    pub const SSH_FX_BAD_MESSAGE: u32 = 5;
    #[allow(dead_code)]
    pub const SSH_FX_OP_UNSUPPORTED: u32 = 8;
}

/// SFTP open flags.
mod pflags {
    pub const SSH_FXF_READ: u32 = 0x00000001;
    pub const SSH_FXF_WRITE: u32 = 0x00000002;
    pub const SSH_FXF_CREAT: u32 = 0x00000008;
    pub const SSH_FXF_TRUNC: u32 = 0x00000010;
}

/// SFTP attribute flags.
mod attr_flags {
    pub const SSH_FILEXFER_ATTR_SIZE: u32 = 0x00000001;
    pub const SSH_FILEXFER_ATTR_PERMISSIONS: u32 = 0x00000004;
}

/// File attributes as used in SFTP packets.
#[derive(Debug, Default, Clone)]
struct FileAttrs {
    size: Option<u64>,
    uid: Option<u32>,
    gid: Option<u32>,
    permissions: Option<u32>,
    atime: Option<u32>,
    mtime: Option<u32>,
}

/// A decoded SFTP response packet.
#[derive(Debug)]
enum SftpPacket {
    Version {
        version: u32,
    },
    Status {
        /// Request ID echoed from the server; not currently verified per-call.
        #[allow(dead_code)]
        request_id: u32,
        code: u32,
        message: String,
    },
    Handle {
        /// Request ID echoed back by the server; checked at call sites.
        request_id: u32,
        handle: Vec<u8>,
    },
    Data {
        /// Request ID echoed back by the server; unused after dispatch.
        #[allow(dead_code)]
        request_id: u32,
        data: Vec<u8>,
    },
    Name {
        /// Request ID echoed back by the server; unused after dispatch.
        #[allow(dead_code)]
        request_id: u32,
        entries: Vec<NameEntry>,
    },
    Attrs {
        /// Request ID echoed back by the server; unused after dispatch.
        #[allow(dead_code)]
        request_id: u32,
        attrs: FileAttrs,
    },
}

/// A single name entry from SSH_FXP_NAME.
#[derive(Debug, Clone)]
struct NameEntry {
    filename: String,
    /// Long-format listing string (e.g. `ls -l` style); reserved for display.
    #[allow(dead_code)]
    longname: String,
    attrs: FileAttrs,
}

// ============================================================================
// SFTP packet serialisation helpers
// ============================================================================

/// Append a big-endian u32 to a buffer.
fn push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// Append a big-endian u64 to a buffer.
fn push_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// Append an SFTP length-prefixed string (u32 len + bytes) to a buffer.
fn push_str(buf: &mut Vec<u8>, s: &str) {
    push_u32(buf, s.len() as u32);
    buf.extend_from_slice(s.as_bytes());
}

/// Append an SFTP length-prefixed byte blob to a buffer.
fn push_bytes(buf: &mut Vec<u8>, b: &[u8]) {
    push_u32(buf, b.len() as u32);
    buf.extend_from_slice(b);
}

/// Serialise `FileAttrs` into the SFTP wire format.
fn push_attrs(buf: &mut Vec<u8>, attrs: &FileAttrs) {
    let mut flags = 0u32;
    if attrs.size.is_some() {
        flags |= attr_flags::SSH_FILEXFER_ATTR_SIZE;
    }
    if attrs.permissions.is_some() {
        flags |= attr_flags::SSH_FILEXFER_ATTR_PERMISSIONS;
    }
    push_u32(buf, flags);
    if let Some(sz) = attrs.size {
        push_u64(buf, sz);
    }
    if let Some(perm) = attrs.permissions {
        push_u32(buf, perm);
    }
}

/// Wrap a payload in an SFTP framing header (u32 length prefix).
fn frame_packet(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());
    push_u32(&mut out, payload.len() as u32);
    out.extend_from_slice(payload);
    out
}

// ============================================================================
// SFTP packet deserialisation helpers
// ============================================================================

/// A cursor over a byte slice for reading SFTP packet fields.
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    fn read_u8(&mut self) -> Result<u8, SftpError> {
        if self.remaining() < 1 {
            return Err(SftpError::Protocol("truncated packet (u8)".into()));
        }
        let v = self.buf[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn read_u32(&mut self) -> Result<u32, SftpError> {
        if self.remaining() < 4 {
            return Err(SftpError::Protocol("truncated packet (u32)".into()));
        }
        let v = u32::from_be_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    fn read_u64(&mut self) -> Result<u64, SftpError> {
        if self.remaining() < 8 {
            return Err(SftpError::Protocol("truncated packet (u64)".into()));
        }
        let mut b = [0u8; 8];
        b.copy_from_slice(&self.buf[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(u64::from_be_bytes(b))
    }

    fn read_string(&mut self) -> Result<String, SftpError> {
        let len = self.read_u32()? as usize;
        if self.remaining() < len {
            return Err(SftpError::Protocol("truncated string".into()));
        }
        let s = String::from_utf8(self.buf[self.pos..self.pos + len].to_vec())
            .map_err(|_| SftpError::Protocol("non-UTF-8 string in packet".into()))?;
        self.pos += len;
        Ok(s)
    }

    fn read_bytes(&mut self) -> Result<Vec<u8>, SftpError> {
        let len = self.read_u32()? as usize;
        if self.remaining() < len {
            return Err(SftpError::Protocol("truncated bytes".into()));
        }
        let b = self.buf[self.pos..self.pos + len].to_vec();
        self.pos += len;
        Ok(b)
    }

    fn read_attrs(&mut self) -> Result<FileAttrs, SftpError> {
        let flags = self.read_u32()?;
        let mut attrs = FileAttrs::default();
        if flags & attr_flags::SSH_FILEXFER_ATTR_SIZE != 0 {
            attrs.size = Some(self.read_u64()?);
        }
        if flags & 0x00000002 != 0 {
            // uidgid
            attrs.uid = Some(self.read_u32()?);
            attrs.gid = Some(self.read_u32()?);
        }
        if flags & attr_flags::SSH_FILEXFER_ATTR_PERMISSIONS != 0 {
            attrs.permissions = Some(self.read_u32()?);
        }
        if flags & 0x00000008 != 0 {
            // acmodtime
            attrs.atime = Some(self.read_u32()?);
            attrs.mtime = Some(self.read_u32()?);
        }
        // Skip any extended attributes
        if flags & 0x80000000 != 0 {
            let count = self.read_u32()?;
            for _ in 0..count {
                let _name = self.read_string()?;
                let _data = self.read_string()?;
            }
        }
        Ok(attrs)
    }
}

/// Parse a raw SFTP payload (excluding the 4-byte length prefix) into a typed packet.
fn parse_sftp_packet(payload: &[u8]) -> Result<SftpPacket, SftpError> {
    if payload.is_empty() {
        return Err(SftpError::Protocol("empty packet".into()));
    }
    let mut cur = Cursor::new(payload);
    let ptype = cur.read_u8()?;

    match ptype {
        t if t == fxp::SSH_FXP_VERSION => {
            let version = cur.read_u32()?;
            Ok(SftpPacket::Version { version })
        }
        t if t == fxp::SSH_FXP_STATUS => {
            let request_id = cur.read_u32()?;
            let code = cur.read_u32()?;
            let message = if cur.remaining() >= 4 {
                cur.read_string().unwrap_or_default()
            } else {
                String::new()
            };
            Ok(SftpPacket::Status { request_id, code, message })
        }
        t if t == fxp::SSH_FXP_HANDLE => {
            let request_id = cur.read_u32()?;
            let handle = cur.read_bytes()?;
            Ok(SftpPacket::Handle { request_id, handle })
        }
        t if t == fxp::SSH_FXP_DATA => {
            let request_id = cur.read_u32()?;
            let data = cur.read_bytes()?;
            Ok(SftpPacket::Data { request_id, data })
        }
        t if t == fxp::SSH_FXP_NAME => {
            let request_id = cur.read_u32()?;
            let count = cur.read_u32()?;
            let mut entries = Vec::new();
            for _ in 0..count {
                let filename = cur.read_string()?;
                let longname = cur.read_string()?;
                let attrs = cur.read_attrs()?;
                entries.push(NameEntry { filename, longname, attrs });
            }
            Ok(SftpPacket::Name { request_id, entries })
        }
        t if t == fxp::SSH_FXP_ATTRS => {
            let request_id = cur.read_u32()?;
            let attrs = cur.read_attrs()?;
            Ok(SftpPacket::Attrs { request_id, attrs })
        }
        other => Err(SftpError::Protocol(format!("unknown packet type {other}"))),
    }
}

// ============================================================================
// Remote connection state
// ============================================================================

/// An active TCP connection to an SFTP server with a rolling request-ID counter.
struct RemoteConn {
    handle: u64,
    next_id: u32,
    verbose: bool,
}

impl RemoteConn {
    fn new(handle: u64, verbose: bool) -> Self {
        Self { handle, next_id: 1, verbose }
    }

    /// Allocate the next request ID.
    fn next_request_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    /// Send an SFTP packet payload (with the 4-byte length framing).
    fn send_packet(&self, payload: &[u8]) -> Result<(), SftpError> {
        if self.verbose {
            eprintln!("[sftp] → {} bytes", payload.len());
        }
        let framed = frame_packet(payload);
        tcp_send_all(self.handle, &framed)
    }

    /// Receive one SFTP packet from the wire.
    fn recv_packet(&self) -> Result<SftpPacket, SftpError> {
        let mut len_buf = [0u8; 4];
        tcp_recv_exact(self.handle, &mut len_buf)?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > 256 * 1024 {
            return Err(SftpError::Protocol(format!("packet too large: {len}")));
        }
        let mut payload = vec![0u8; len];
        tcp_recv_exact(self.handle, &mut payload)?;
        if self.verbose {
            eprintln!("[sftp] ← {} bytes, type={}", len, payload.first().copied().unwrap_or(0));
        }
        parse_sftp_packet(&payload)
    }

    /// Send SSH_FXP_INIT and receive SSH_FXP_VERSION.
    fn init(&mut self) -> Result<u32, SftpError> {
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_INIT);
        push_u32(&mut payload, 3); // request version 3
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Version { version } => Ok(version),
            other => Err(SftpError::Protocol(format!("expected VERSION, got {other:?}"))),
        }
    }

    /// Open a remote file handle.
    fn open(&mut self, path: &str, flags: u32, attrs: &FileAttrs) -> Result<Vec<u8>, SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_OPEN);
        push_u32(&mut payload, id);
        push_str(&mut payload, path);
        push_u32(&mut payload, flags);
        push_attrs(&mut payload, attrs);
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Handle { request_id, handle } if request_id == id => Ok(handle),
            SftpPacket::Status { code, message, .. } => {
                Err(SftpError::Remote { code, message })
            }
            _ => Err(SftpError::Protocol("unexpected response to OPEN".into())),
        }
    }

    /// Close an open handle.
    fn close(&mut self, handle: &[u8]) -> Result<(), SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_CLOSE);
        push_u32(&mut payload, id);
        push_bytes(&mut payload, handle);
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Status { code, message: _, .. } if code == status::SSH_FX_OK => Ok(()),
            SftpPacket::Status { code, message, .. } => Err(SftpError::Remote { code, message }),
            _ => Err(SftpError::Protocol("unexpected response to CLOSE".into())),
        }
    }

    /// Read up to `len` bytes from `offset` in a remote file.
    fn read(&mut self, handle: &[u8], offset: u64, len: u32) -> Result<Option<Vec<u8>>, SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_READ);
        push_u32(&mut payload, id);
        push_bytes(&mut payload, handle);
        push_u64(&mut payload, offset);
        push_u32(&mut payload, len);
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Data { data, .. } => Ok(Some(data)),
            SftpPacket::Status { code, .. } if code == status::SSH_FX_EOF => Ok(None),
            SftpPacket::Status { code, message, .. } => Err(SftpError::Remote { code, message }),
            _ => Err(SftpError::Protocol("unexpected response to READ".into())),
        }
    }

    /// Write data to an open remote handle at `offset`.
    fn write(&mut self, handle: &[u8], offset: u64, data: &[u8]) -> Result<(), SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_WRITE);
        push_u32(&mut payload, id);
        push_bytes(&mut payload, handle);
        push_u64(&mut payload, offset);
        push_bytes(&mut payload, data);
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Status { code, message: _, .. } if code == status::SSH_FX_OK => Ok(()),
            SftpPacket::Status { code, message, .. } => Err(SftpError::Remote { code, message }),
            _ => Err(SftpError::Protocol("unexpected response to WRITE".into())),
        }
    }

    /// Open a remote directory handle.
    fn opendir(&mut self, path: &str) -> Result<Vec<u8>, SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_OPENDIR);
        push_u32(&mut payload, id);
        push_str(&mut payload, path);
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Handle { request_id, handle } if request_id == id => Ok(handle),
            SftpPacket::Status { code, message, .. } => Err(SftpError::Remote { code, message }),
            _ => Err(SftpError::Protocol("unexpected response to OPENDIR".into())),
        }
    }

    /// Read the next batch of entries from a directory handle.
    fn readdir(&mut self, handle: &[u8]) -> Result<Option<Vec<NameEntry>>, SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_READDIR);
        push_u32(&mut payload, id);
        push_bytes(&mut payload, handle);
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Name { entries, .. } => Ok(Some(entries)),
            SftpPacket::Status { code, .. } if code == status::SSH_FX_EOF => Ok(None),
            SftpPacket::Status { code, message, .. } => Err(SftpError::Remote { code, message }),
            _ => Err(SftpError::Protocol("unexpected response to READDIR".into())),
        }
    }

    /// Stat a remote path (follows symlinks).
    fn stat(&mut self, path: &str) -> Result<FileAttrs, SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_STAT);
        push_u32(&mut payload, id);
        push_str(&mut payload, path);
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Attrs { attrs, .. } => Ok(attrs),
            SftpPacket::Status { code, message, .. } => Err(SftpError::Remote { code, message }),
            _ => Err(SftpError::Protocol("unexpected response to STAT".into())),
        }
    }

    /// Lstat a remote path (does NOT follow symlinks).
    fn lstat(&mut self, path: &str) -> Result<FileAttrs, SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_LSTAT);
        push_u32(&mut payload, id);
        push_str(&mut payload, path);
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Attrs { attrs, .. } => Ok(attrs),
            SftpPacket::Status { code, message, .. } => Err(SftpError::Remote { code, message }),
            _ => Err(SftpError::Protocol("unexpected response to LSTAT".into())),
        }
    }

    /// Create a remote directory.
    fn mkdir(&mut self, path: &str) -> Result<(), SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_MKDIR);
        push_u32(&mut payload, id);
        push_str(&mut payload, path);
        push_attrs(&mut payload, &FileAttrs { permissions: Some(0o755), ..Default::default() });
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Status { code, message: _, .. } if code == status::SSH_FX_OK => Ok(()),
            SftpPacket::Status { code, message, .. } => Err(SftpError::Remote { code, message }),
            _ => Err(SftpError::Protocol("unexpected response to MKDIR".into())),
        }
    }

    /// Remove a remote directory.
    fn rmdir(&mut self, path: &str) -> Result<(), SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_RMDIR);
        push_u32(&mut payload, id);
        push_str(&mut payload, path);
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Status { code, message: _, .. } if code == status::SSH_FX_OK => Ok(()),
            SftpPacket::Status { code, message, .. } => Err(SftpError::Remote { code, message }),
            _ => Err(SftpError::Protocol("unexpected response to RMDIR".into())),
        }
    }

    /// Delete a remote file.
    fn remove(&mut self, path: &str) -> Result<(), SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_REMOVE);
        push_u32(&mut payload, id);
        push_str(&mut payload, path);
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Status { code, message: _, .. } if code == status::SSH_FX_OK => Ok(()),
            SftpPacket::Status { code, message, .. } => Err(SftpError::Remote { code, message }),
            _ => Err(SftpError::Protocol("unexpected response to REMOVE".into())),
        }
    }

    /// Rename a remote file or directory.
    fn rename_remote(&mut self, old: &str, new: &str) -> Result<(), SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_RENAME);
        push_u32(&mut payload, id);
        push_str(&mut payload, old);
        push_str(&mut payload, new);
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Status { code, message: _, .. } if code == status::SSH_FX_OK => Ok(()),
            SftpPacket::Status { code, message, .. } => Err(SftpError::Remote { code, message }),
            _ => Err(SftpError::Protocol("unexpected response to RENAME".into())),
        }
    }

    /// Set attributes (e.g. permissions) on a remote file.
    fn setstat(&mut self, path: &str, attrs: &FileAttrs) -> Result<(), SftpError> {
        let id = self.next_request_id();
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_SETSTAT);
        push_u32(&mut payload, id);
        push_str(&mut payload, path);
        push_attrs(&mut payload, attrs);
        self.send_packet(&payload)?;
        match self.recv_packet()? {
            SftpPacket::Status { code, message: _, .. } if code == status::SSH_FX_OK => Ok(()),
            SftpPacket::Status { code, message, .. } => Err(SftpError::Remote { code, message }),
            _ => Err(SftpError::Protocol("unexpected response to SETSTAT".into())),
        }
    }
}

// ============================================================================
// Session state
// ============================================================================

/// Where the remote side lives (local filesystem or TCP remote).
enum Remote {
    /// Local-only mode: remote and local both refer to the local filesystem.
    Local { cwd: String },
    /// Connected TCP mode with an active SFTP session.
    Connected { conn: RemoteConn, remote_cwd: String },
}

/// Top-level session: holds local and remote state.
struct Session {
    remote: Remote,
    local_cwd: String,
    /// Verbose flag; stored for future use (e.g. per-session verbose toggling).
    #[allow(dead_code)]
    verbose: bool,
}

impl Session {
    /// Create a local-only session.
    fn local(cwd: String, verbose: bool) -> Self {
        Self {
            remote: Remote::Local { cwd: cwd.clone() },
            local_cwd: cwd,
            verbose,
        }
    }

    /// Create a connected session over an existing `RemoteConn`.
    fn connected(conn: RemoteConn, remote_cwd: String, local_cwd: String, verbose: bool) -> Self {
        Self {
            remote: Remote::Connected { conn, remote_cwd },
            local_cwd,
            verbose,
        }
    }

    /// True if we have a live TCP connection.
    fn is_connected(&self) -> bool {
        matches!(self.remote, Remote::Connected { .. })
    }

    /// Get a reference to the remote current working directory.
    fn remote_cwd(&self) -> &str {
        match &self.remote {
            Remote::Local { cwd } => cwd,
            Remote::Connected { remote_cwd, .. } => remote_cwd,
        }
    }

    /// Close the TCP connection if open.
    fn disconnect(&mut self) {
        if let Remote::Connected { conn, .. } = &self.remote {
            tcp_close(conn.handle);
        }
    }
}

// ============================================================================
// Command execution
// ============================================================================

/// Execute a single parsed `SftpCommand` against the session.
fn execute_command(session: &mut Session, cmd: &SftpCommand) -> Result<bool, SftpError> {
    match cmd {
        SftpCommand::Ls(path) => cmd_ls(session, path.as_deref()),
        SftpCommand::Cd(path) => cmd_cd(session, path),
        SftpCommand::Pwd => cmd_pwd(session),
        SftpCommand::Lcd(path) => cmd_lcd(session, path),
        SftpCommand::Lpwd => cmd_lpwd(session),
        SftpCommand::Get { remote, local } => cmd_get(session, remote, local.as_deref()),
        SftpCommand::Put { local, remote } => cmd_put(session, local, remote.as_deref()),
        SftpCommand::Mget(pattern) => cmd_mget(session, pattern),
        SftpCommand::Mput(pattern) => cmd_mput(session, pattern),
        SftpCommand::Mkdir(path) => cmd_mkdir(session, path),
        SftpCommand::Rmdir(path) => cmd_rmdir(session, path),
        SftpCommand::Rm(path) => cmd_rm(session, path),
        SftpCommand::Rename { old, new } => cmd_rename(session, old, new),
        SftpCommand::Chmod { mode, path } => cmd_chmod(session, *mode, path),
        SftpCommand::Stat(path) => cmd_stat(session, path),
        SftpCommand::Lstat(path) => cmd_lstat(session, path),
        SftpCommand::Shell(cmd_str) => cmd_shell(cmd_str),
        SftpCommand::Help => cmd_help(),
        SftpCommand::Quit => {
            println!("Bye.");
            return Ok(false);
        }
    }?;
    Ok(true)
}

// ---- ls ----

fn cmd_ls(session: &mut Session, path: Option<&str>) -> Result<(), SftpError> {
    let dir = match path {
        Some(p) => resolve_path(session.remote_cwd(), p),
        None => session.remote_cwd().to_string(),
    };
    let entries = remote_readdir(session, &dir)?;
    if entries.is_empty() {
        println!("(empty)");
    } else {
        for e in &entries {
            let suffix = if e.is_dir { "/" } else if e.is_link { "@" } else { "" };
            println!("  {:<40} {:>8}", format!("{}{suffix}", e.name), format_size(e.size));
        }
    }
    Ok(())
}

// ---- cd ----

fn cmd_cd(session: &mut Session, path: &str) -> Result<(), SftpError> {
    let new_dir = resolve_path(session.remote_cwd(), path);
    // Verify the remote directory exists before changing to it.
    remote_stat(session, &new_dir)?;
    match &mut session.remote {
        Remote::Local { cwd } => *cwd = new_dir.clone(),
        Remote::Connected { remote_cwd, .. } => *remote_cwd = new_dir.clone(),
    }
    println!("Remote working directory: {new_dir}");
    Ok(())
}

// ---- pwd ----

fn cmd_pwd(session: &mut Session) -> Result<(), SftpError> {
    println!("Remote working directory: {}", session.remote_cwd());
    Ok(())
}

// ---- lcd ----

fn cmd_lcd(session: &mut Session, path: &str) -> Result<(), SftpError> {
    let new_dir = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{path}", session.local_cwd)
    };
    let new_dir = normalise_path(&new_dir);
    // On target this changes the kernel CWD; on host we just update the field.
    #[cfg(target_arch = "x86_64")]
    os_chdir(&new_dir)?;
    session.local_cwd = new_dir.clone();
    println!("Local working directory: {new_dir}");
    Ok(())
}

// ---- lpwd ----

fn cmd_lpwd(session: &mut Session) -> Result<(), SftpError> {
    println!("Local working directory: {}", session.local_cwd);
    Ok(())
}

// ---- get ----

fn cmd_get(session: &mut Session, remote_path: &str, local_path: Option<&str>) -> Result<(), SftpError> {
    let remote_abs = resolve_path(session.remote_cwd(), remote_path);
    let local_name = local_path.unwrap_or_else(|| {
        remote_abs.rsplit('/').next().unwrap_or(remote_path)
    });
    let local_abs = if local_name.starts_with('/') {
        local_name.to_string()
    } else {
        format!("{}/{local_name}", session.local_cwd)
    };

    match &mut session.remote {
        Remote::Local { .. } => {
            // Local-to-local copy
            let data = os_read_file(&remote_abs).map_err(SftpError::from)?;
            println!("Fetching {remote_abs} → {local_abs} ({} bytes)", data.len());
            os_write_file(&local_abs, &data).map_err(SftpError::from)?;
        }
        Remote::Connected { conn, .. } => {
            let attrs = conn.stat(&remote_abs)?;
            let total = attrs.size.unwrap_or(0);
            let open_flags = pflags::SSH_FXF_READ;
            let handle = conn.open(&remote_abs, open_flags, &FileAttrs::default())?;
            let local_fd = os_open(&local_abs, O_WRONLY | O_CREAT | O_TRUNC, 0o644)
                .map_err(SftpError::from)?;
            let chunk_size: u32 = 32768;
            let mut offset: u64 = 0;
            loop {
                match conn.read(&handle, offset, chunk_size)? {
                    None => break,
                    Some(data) => {
                        if data.is_empty() {
                            break;
                        }
                        if let Err(e) = os_write_all(local_fd, &data) {
                            os_close(local_fd);
                            let _ = conn.close(&handle);
                            return Err(SftpError::from(e));
                        }
                        offset = offset.checked_add(data.len() as u64).unwrap_or(offset);
                        print_progress(local_name, offset, total);
                    }
                }
            }
            os_close(local_fd);
            conn.close(&handle)?;
            println!(); // newline after progress
        }
    }
    println!("Fetched {remote_abs} → {local_abs}");
    Ok(())
}

// ---- put ----

fn cmd_put(session: &mut Session, local_path: &str, remote_path: Option<&str>) -> Result<(), SftpError> {
    let local_abs = if local_path.starts_with('/') {
        local_path.to_string()
    } else {
        format!("{}/{local_path}", session.local_cwd)
    };
    let local_name = local_abs.rsplit('/').next().unwrap_or(local_path);
    let remote_name = remote_path.unwrap_or(local_name);
    let remote_abs = resolve_path(session.remote_cwd(), remote_name);

    match &mut session.remote {
        Remote::Local { .. } => {
            let data = os_read_file(&local_abs).map_err(SftpError::from)?;
            println!("Uploading {local_abs} → {remote_abs} ({} bytes)", data.len());
            os_write_file(&remote_abs, &data).map_err(SftpError::from)?;
        }
        Remote::Connected { conn, .. } => {
            let data = os_read_file(&local_abs).map_err(SftpError::from)?;
            let total = data.len() as u64;
            let open_flags = pflags::SSH_FXF_WRITE | pflags::SSH_FXF_CREAT | pflags::SSH_FXF_TRUNC;
            let handle = conn.open(&remote_abs, open_flags, &FileAttrs::default())?;
            let chunk_size = 32768usize;
            let mut offset: u64 = 0;
            for chunk in data.chunks(chunk_size) {
                if let Err(e) = conn.write(&handle, offset, chunk) {
                    let _ = conn.close(&handle);
                    return Err(e);
                }
                offset = offset.checked_add(chunk.len() as u64).unwrap_or(offset);
                print_progress(local_name, offset, total);
            }
            conn.close(&handle)?;
            println!();
        }
    }
    println!("Uploaded {local_abs} → {remote_abs}");
    Ok(())
}

// ---- mget ----

fn cmd_mget(session: &mut Session, pattern: &str) -> Result<(), SftpError> {
    let entries = remote_readdir(session, session.remote_cwd().to_string().as_str())?;
    let mut matched = 0usize;
    for entry in &entries {
        if !entry.is_dir && glob_match(pattern, &entry.name) {
            let remote_path = format!("{}/{}", session.remote_cwd(), entry.name);
            // We need to pass a clone of the local_cwd and remote_cwd to avoid
            // the borrow checker conflict when calling cmd_get.
            let local_cwd = session.local_cwd.clone();
            let _ = cmd_get(session, &remote_path, Some(&format!("{}/{}", local_cwd, entry.name)));
            matched = matched.saturating_add(1);
        }
    }
    if matched == 0 {
        println!("No files matched pattern '{pattern}'");
    }
    Ok(())
}

// ---- mput ----

fn cmd_mput(session: &mut Session, pattern: &str) -> Result<(), SftpError> {
    let local_cwd = session.local_cwd.clone();
    let entries = match os_readdir(&local_cwd) {
        Ok(e) => e,
        Err(e) => return Err(SftpError::from(e)),
    };
    let mut matched = 0usize;
    for entry in &entries {
        if !entry.is_dir && glob_match(pattern, &entry.name) {
            let local_path = format!("{local_cwd}/{}", entry.name);
            let _ = cmd_put(session, &local_path, None);
            matched = matched.saturating_add(1);
        }
    }
    if matched == 0 {
        println!("No files matched pattern '{pattern}'");
    }
    Ok(())
}

// ---- mkdir ----

fn cmd_mkdir(session: &mut Session, path: &str) -> Result<(), SftpError> {
    let abs = resolve_path(session.remote_cwd(), path);
    match &mut session.remote {
        Remote::Local { .. } => os_mkdir(&abs).map_err(SftpError::from)?,
        Remote::Connected { conn, .. } => conn.mkdir(&abs)?,
    }
    println!("Created directory: {abs}");
    Ok(())
}

// ---- rmdir ----

fn cmd_rmdir(session: &mut Session, path: &str) -> Result<(), SftpError> {
    let abs = resolve_path(session.remote_cwd(), path);
    match &mut session.remote {
        Remote::Local { .. } => {
            // Kernel rmdir — for local mode use our syscall path.
            let cpath = to_cstring(&abs);
            // SAFETY: cpath is a valid null-terminated path string.
            let ret = unsafe { syscall1(SYS_UNLINK, cpath.as_ptr() as u64) };
            if ret < 0 {
                return Err(OsError::Syscall("rmdir", ret).into());
            }
        }
        Remote::Connected { conn, .. } => conn.rmdir(&abs)?,
    }
    println!("Removed directory: {abs}");
    Ok(())
}

// ---- rm ----

fn cmd_rm(session: &mut Session, path: &str) -> Result<(), SftpError> {
    let abs = resolve_path(session.remote_cwd(), path);
    match &mut session.remote {
        Remote::Local { .. } => os_unlink(&abs).map_err(SftpError::from)?,
        Remote::Connected { conn, .. } => conn.remove(&abs)?,
    }
    println!("Removed: {abs}");
    Ok(())
}

// ---- rename ----

fn cmd_rename(session: &mut Session, old: &str, new: &str) -> Result<(), SftpError> {
    let old_abs = resolve_path(session.remote_cwd(), old);
    let new_abs = resolve_path(session.remote_cwd(), new);
    match &mut session.remote {
        Remote::Local { .. } => os_rename(&old_abs, &new_abs).map_err(SftpError::from)?,
        Remote::Connected { conn, .. } => conn.rename_remote(&old_abs, &new_abs)?,
    }
    println!("Renamed {old_abs} → {new_abs}");
    Ok(())
}

// ---- chmod ----

fn cmd_chmod(session: &mut Session, mode: u32, path: &str) -> Result<(), SftpError> {
    let abs = resolve_path(session.remote_cwd(), path);
    match &mut session.remote {
        Remote::Local { .. } => os_chmod(&abs, mode).map_err(SftpError::from)?,
        Remote::Connected { conn, .. } => {
            conn.setstat(&abs, &FileAttrs { permissions: Some(mode), ..Default::default() })?;
        }
    }
    println!("chmod {mode:04o} {abs}");
    Ok(())
}

// ---- stat ----

fn cmd_stat(session: &mut Session, path: &str) -> Result<(), SftpError> {
    let abs = resolve_path(session.remote_cwd(), path);
    let attrs = remote_stat(session, &abs)?;
    print_attrs(&abs, &attrs);
    Ok(())
}

// ---- lstat ----

fn cmd_lstat(session: &mut Session, path: &str) -> Result<(), SftpError> {
    let abs = resolve_path(session.remote_cwd(), path);
    let attrs = match &mut session.remote {
        Remote::Local { .. } => {
            let st = os_stat(&abs)?;
            FileAttrs {
                size: Some(st.st_size as u64),
                permissions: Some(st.st_mode),
                uid: Some(st.st_uid),
                gid: Some(st.st_gid),
                atime: Some(st.st_atime as u32),
                mtime: Some(st.st_mtime as u32),
            }
        }
        Remote::Connected { conn, .. } => conn.lstat(&abs)?,
    };
    print_attrs(&abs, &attrs);
    Ok(())
}

fn print_attrs(path: &str, attrs: &FileAttrs) {
    println!("  Path:        {path}");
    if let Some(sz) = attrs.size {
        println!("  Size:        {} ({sz} bytes)", format_size(sz));
    }
    if let Some(perm) = attrs.permissions {
        println!("  Permissions: {} ({perm:04o})", format_mode(perm));
    }
    if let Some(uid) = attrs.uid {
        println!("  UID/GID:     {uid}/{}", attrs.gid.unwrap_or(0));
    }
    if let Some(mt) = attrs.mtime {
        println!("  Modified:    {mt}");
    }
}

// ---- shell ----

fn cmd_shell(cmd_str: &str) -> Result<(), SftpError> {
    // On OurOS there's no exec/fork path; print a notice. In a full
    // implementation this would spawn a process via the process manager IPC.
    println!("Shell command (local): {cmd_str}");
    println!("  (Local shell execution is not available in this environment)");
    Ok(())
}

// ---- help ----

fn cmd_help() -> Result<(), SftpError> {
    println!(
        "\
Available commands:
  ls [path]              List remote directory
  dir [path]             Alias for ls
  cd path                Change remote directory
  pwd                    Print remote working directory
  lcd path               Change local directory
  lpwd                   Print local working directory
  get remote [local]     Download file from remote
  put local [remote]     Upload file to remote
  mget pattern           Download files matching glob pattern
  mput pattern           Upload files matching glob pattern
  mkdir dir              Create remote directory
  rmdir dir              Remove remote directory
  rm file                Delete remote file
  rename old new         Rename remote file or directory
  chmod mode file        Change remote file permissions (octal mode)
  stat file              Show remote file attributes (follows symlinks)
  lstat file             Show remote file attributes (no symlink follow)
  !command               Execute local shell command
  help / ?               Show this help
  bye / quit / exit      Disconnect and exit
"
    );
    Ok(())
}

// ============================================================================
// Remote access helpers (abstraction over Local / Connected)
// ============================================================================

/// List a remote directory, returning `DirEntry` values regardless of mode.
fn remote_readdir(session: &mut Session, path: &str) -> Result<Vec<DirEntry>, SftpError> {
    match &mut session.remote {
        Remote::Local { .. } => os_readdir(path).map_err(SftpError::from),
        Remote::Connected { conn, .. } => {
            let dir_handle = conn.opendir(path)?;
            let mut entries = Vec::new();
            loop {
                match conn.readdir(&dir_handle)? {
                    None => break,
                    Some(batch) => {
                        for ne in batch {
                            if ne.filename == "." || ne.filename == ".." {
                                continue;
                            }
                            let is_dir = ne.attrs.permissions.map_or(false, |p| (p & 0o170000) == 0o040000);
                            let is_link = ne.attrs.permissions.map_or(false, |p| (p & 0o170000) == 0o120000);
                            entries.push(DirEntry {
                                name: ne.filename,
                                is_dir,
                                is_link,
                                size: ne.attrs.size.unwrap_or(0),
                            });
                        }
                    }
                }
            }
            let _ = conn.close(&dir_handle);
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(entries)
        }
    }
}

/// Stat a remote path and return `FileAttrs`.
fn remote_stat(session: &mut Session, path: &str) -> Result<FileAttrs, SftpError> {
    match &mut session.remote {
        Remote::Local { .. } => {
            let st = os_stat(path)?;
            Ok(FileAttrs {
                size: Some(st.st_size as u64),
                permissions: Some(st.st_mode),
                uid: Some(st.st_uid),
                gid: Some(st.st_gid),
                atime: Some(st.st_atime as u32),
                mtime: Some(st.st_mtime as u32),
            })
        }
        Remote::Connected { conn, .. } => conn.stat(path),
    }
}

// ============================================================================
// Command parsing
// ============================================================================

/// All interactive commands the SFTP client recognises.
#[derive(Debug, PartialEq)]
enum SftpCommand {
    Ls(Option<String>),
    Cd(String),
    Pwd,
    Lcd(String),
    Lpwd,
    Get { remote: String, local: Option<String> },
    Put { local: String, remote: Option<String> },
    Mget(String),
    Mput(String),
    Mkdir(String),
    Rmdir(String),
    Rm(String),
    Rename { old: String, new: String },
    Chmod { mode: u32, path: String },
    Stat(String),
    Lstat(String),
    Shell(String),
    Help,
    Quit,
}

/// Parse a command line into a `SftpCommand`, or return an error string.
fn parse_command(line: &str) -> Result<SftpCommand, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Err(String::new()); // empty / comment → skip
    }
    let mut parts = line.splitn(4, char::is_whitespace).filter(|s| !s.is_empty());
    let verb = parts.next().unwrap_or("").to_lowercase();
    let arg1 = parts.next().map(str::to_string);
    let arg2 = parts.next().map(str::to_string);
    let arg3 = parts.next().map(str::to_string);

    match verb.as_str() {
        "ls" | "dir" => Ok(SftpCommand::Ls(arg1)),
        "cd" => arg1.map(SftpCommand::Cd).ok_or_else(|| "cd: missing argument".into()),
        "pwd" => Ok(SftpCommand::Pwd),
        "lcd" => arg1.map(SftpCommand::Lcd).ok_or_else(|| "lcd: missing argument".into()),
        "lpwd" => Ok(SftpCommand::Lpwd),
        "get" => {
            arg1.map(|r| SftpCommand::Get { remote: r, local: arg2 })
                .ok_or_else(|| "get: missing remote path".into())
        }
        "put" => {
            arg1.map(|l| SftpCommand::Put { local: l, remote: arg2 })
                .ok_or_else(|| "put: missing local path".into())
        }
        "mget" => arg1.map(SftpCommand::Mget).ok_or_else(|| "mget: missing pattern".into()),
        "mput" => arg1.map(SftpCommand::Mput).ok_or_else(|| "mput: missing pattern".into()),
        "mkdir" => arg1.map(SftpCommand::Mkdir).ok_or_else(|| "mkdir: missing argument".into()),
        "rmdir" => arg1.map(SftpCommand::Rmdir).ok_or_else(|| "rmdir: missing argument".into()),
        "rm" | "delete" => arg1.map(SftpCommand::Rm).ok_or_else(|| "rm: missing argument".into()),
        "rename" | "mv" => {
            match (arg1, arg2) {
                (Some(old), Some(new)) => Ok(SftpCommand::Rename { old, new }),
                _ => Err("rename: requires two arguments".into()),
            }
        }
        "chmod" => {
            match (arg1, arg2) {
                (Some(mode_str), Some(path)) => {
                    let mode = parse_octal(&mode_str)
                        .ok_or_else(|| format!("chmod: invalid mode '{mode_str}'"))?;
                    Ok(SftpCommand::Chmod { mode, path })
                }
                _ => Err("chmod: requires mode and path arguments".into()),
            }
        }
        "stat" => arg1.map(SftpCommand::Stat).ok_or_else(|| "stat: missing argument".into()),
        "lstat" => arg1.map(SftpCommand::Lstat).ok_or_else(|| "lstat: missing argument".into()),
        _ if line.starts_with('!') => Ok(SftpCommand::Shell(line[1..].trim().to_string())),
        "help" | "?" => Ok(SftpCommand::Help),
        "bye" | "quit" | "exit" => Ok(SftpCommand::Quit),
        _ => {
            let _ = arg3; // suppress unused-var lint
            Err(format!("Unknown command '{verb}'. Type 'help' for a list of commands."))
        }
    }
}

// ============================================================================
// CLI argument parsing
// ============================================================================

/// Parsed command-line configuration.
struct Config {
    host: Option<String>,
    port: u16,
    user: Option<String>,
    remote_start_path: Option<String>,
    batch_file: Option<String>,
    verbose: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: None,
            port: 22,
            user: None,
            remote_start_path: None,
            batch_file: None,
            verbose: false,
        }
    }
}

/// Parse CLI arguments into a `Config`.
fn parse_args(args: &[String]) -> Result<Config, SftpError> {
    let mut cfg = Config::default();
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "-P" | "--port" => {
                i = i.checked_add(1).ok_or_else(|| SftpError::BadArg("overflow".into()))?;
                let port_str = args.get(i).ok_or_else(|| SftpError::BadArg("-P requires a port number".into()))?;
                cfg.port = port_str.parse::<u16>()
                    .map_err(|_| SftpError::BadArg(format!("invalid port '{port_str}'")))?;
            }
            "-b" | "--batch" => {
                i = i.checked_add(1).ok_or_else(|| SftpError::BadArg("overflow".into()))?;
                cfg.batch_file = Some(args.get(i)
                    .ok_or_else(|| SftpError::BadArg("-b requires a filename".into()))?
                    .clone());
            }
            "-v" | "--verbose" => {
                cfg.verbose = true;
            }
            arg if !arg.starts_with('-') => {
                // [user@]host[:path]
                let (user_host, path) = if let Some(colon) = arg.rfind(':') {
                    (&arg[..colon], Some(arg[colon + 1..].to_string()))
                } else {
                    (arg, None)
                };
                cfg.remote_start_path = path;
                if let Some(at) = user_host.find('@') {
                    cfg.user = Some(user_host[..at].to_string());
                    cfg.host = Some(user_host[at + 1..].to_string());
                } else {
                    cfg.host = Some(user_host.to_string());
                }
            }
            other => {
                return Err(SftpError::BadArg(format!("unrecognised option '{other}'")));
            }
        }
        i = i.checked_add(1).ok_or_else(|| SftpError::BadArg("overflow".into()))?;
    }
    Ok(cfg)
}

// ============================================================================
// Connection setup
// ============================================================================

/// Parse an IPv4 dotted-decimal address string into a network-byte-order u32.
fn parse_ipv4(s: &str) -> Option<u32> {
    let mut parts = s.split('.');
    let a = parts.next()?.parse::<u8>().ok()?;
    let b = parts.next()?.parse::<u8>().ok()?;
    let c = parts.next()?.parse::<u8>().ok()?;
    let d = parts.next()?.parse::<u8>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    // Network byte order: a.b.c.d → a<<24 | b<<16 | c<<8 | d
    let ip = (u32::from(a) << 24) | (u32::from(b) << 16) | (u32::from(c) << 8) | u32::from(d);
    Some(ip.to_be())
}

/// Attempt to connect to an SFTP server and exchange SSH_FXP_INIT/VERSION.
fn connect_sftp(host: &str, port: u16, verbose: bool) -> Result<RemoteConn, SftpError> {
    // Try to parse host as a raw IPv4 address first; then assume it is already
    // an address that the kernel resolves (in a full implementation we would
    // call a DNS syscall, but that is handled by the ssh utility layer).
    let ip = parse_ipv4(host).ok_or_else(|| {
        SftpError::ConnectionFailed(format!(
            "cannot resolve '{host}': DNS syscall not available in sftp; use an IP address or connect via ssh -sftp"
        ))
    })?;
    if verbose {
        eprintln!("[sftp] connecting to {host}:{port}");
    }
    let handle = tcp_connect(ip, port)?;
    let mut conn = RemoteConn::new(handle, verbose);
    let version = conn.init().map_err(|e| {
        tcp_close(handle);
        e
    })?;
    if verbose {
        eprintln!("[sftp] server SFTP version {version}");
    }
    if version < 3 {
        tcp_close(handle);
        return Err(SftpError::Protocol(format!(
            "server supports only SFTP version {version}; we require v3+"
        )));
    }
    Ok(conn)
}

// ============================================================================
// Interactive loop
// ============================================================================

/// Print the interactive prompt.
fn print_prompt(session: &Session) {
    if session.is_connected() {
        print!("sftp> ");
    } else {
        print!("sftp[local]> ");
    }
    let _ = io::stdout().flush();
}

/// Run the interactive read-eval loop, reading from `reader`.
fn run_interactive<R: BufRead>(session: &mut Session, reader: &mut R) -> Result<(), SftpError> {
    let mut line = String::new();
    loop {
        print_prompt(session);
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => return Err(SftpError::from(e)),
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match parse_command(trimmed) {
            Err(e) if e.is_empty() => continue,
            Err(e) => eprintln!("sftp: {e}"),
            Ok(cmd) => {
                match execute_command(session, &cmd) {
                    Ok(false) => break,
                    Ok(true) => {}
                    Err(e) => eprintln!("sftp: {e}"),
                }
            }
        }
    }
    Ok(())
}

/// Run batch mode: read commands from a file.
fn run_batch(session: &mut Session, batch_file: &str) -> Result<(), SftpError> {
    let data = os_read_file(batch_file)?;
    let text = String::from_utf8(data)
        .map_err(|_| SftpError::BadArg(format!("batch file '{batch_file}' is not valid UTF-8")))?;
    let mut reader = io::Cursor::new(text.as_bytes().to_vec());
    // In batch mode we still use run_interactive but without a real terminal.
    run_interactive(session, &mut io::BufReader::new(&mut reader as &mut dyn Read))
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> Result<(), SftpError> {
    let args: Vec<String> = env::args().skip(1).collect();
    let cfg = parse_args(&args)?;

    // Determine local CWD.
    let local_cwd = os_getcwd().unwrap_or_else(|_| "/".to_string());

    let mut session = if let Some(ref host) = cfg.host {
        let conn = connect_sftp(host, cfg.port, cfg.verbose)?;
        let remote_cwd = cfg.remote_start_path
            .clone()
            .unwrap_or_else(|| "/".to_string());
        println!("Connected to {host}.");
        Session::connected(conn, remote_cwd, local_cwd, cfg.verbose)
    } else {
        println!("sftp: no host specified; entering local mode.");
        Session::local(local_cwd, cfg.verbose)
    };

    let result = if let Some(ref batch_file) = cfg.batch_file {
        run_batch(&mut session, batch_file)
    } else {
        let stdin = io::stdin();
        let mut reader = io::BufReader::new(stdin.lock());
        run_interactive(&mut session, &mut reader)
    };

    session.disconnect();
    result
}

fn main() {
    if let Err(e) = run() {
        eprintln!("sftp: error: {e}");
        std::process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- glob_match ---

    #[test]
    fn glob_star_matches_any() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(!glob_match("*.rs", "main.c"));
    }

    #[test]
    fn glob_star_matches_empty() {
        // `*` should match an empty string when at the end of pattern.
        assert!(glob_match("file*", "file"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn glob_question_matches_single() {
        assert!(glob_match("?.rs", "a.rs"));
        assert!(!glob_match("?.rs", "ab.rs"));
        assert!(!glob_match("?.rs", ".rs"));
    }

    #[test]
    fn glob_no_wildcard_exact() {
        assert!(glob_match("Cargo.toml", "Cargo.toml"));
        assert!(!glob_match("Cargo.toml", "cargo.toml"));
    }

    #[test]
    fn glob_mixed_wildcards() {
        assert!(glob_match("foo*bar?.rs", "fooXXXbarY.rs"));
        assert!(!glob_match("foo*bar?.rs", "fooXXXbar.rs")); // missing char for ?
    }

    // --- parse_octal ---

    #[test]
    fn octal_valid() {
        assert_eq!(parse_octal("755"), Some(0o755));
        assert_eq!(parse_octal("644"), Some(0o644));
        assert_eq!(parse_octal("0"), Some(0));
    }

    #[test]
    fn octal_invalid() {
        assert_eq!(parse_octal("999"), None); // 9 is not a valid octal digit
        assert_eq!(parse_octal("abc"), None);
        assert_eq!(parse_octal(""), None);
    }

    // --- parse_ipv4 ---

    #[test]
    fn ipv4_valid() {
        // 127.0.0.1 in network byte order
        let nbo = parse_ipv4("127.0.0.1").unwrap();
        // The raw u32 in native byte order after .to_be() should have bytes 127,0,0,1.
        let bytes = nbo.to_be_bytes();
        assert_eq!(bytes, [127, 0, 0, 1]);
    }

    #[test]
    fn ipv4_broadcast() {
        let nbo = parse_ipv4("255.255.255.255").unwrap();
        assert_eq!(nbo.to_be_bytes(), [255, 255, 255, 255]);
    }

    #[test]
    fn ipv4_invalid() {
        assert!(parse_ipv4("256.0.0.1").is_none());
        assert!(parse_ipv4("1.2.3").is_none());
        assert!(parse_ipv4("1.2.3.4.5").is_none());
        assert!(parse_ipv4("not_an_ip").is_none());
    }

    // --- normalise_path ---

    #[test]
    fn normalise_simple() {
        assert_eq!(normalise_path("/foo/bar"), "/foo/bar");
    }

    #[test]
    fn normalise_dotdot() {
        assert_eq!(normalise_path("/foo/bar/../baz"), "/foo/baz");
        assert_eq!(normalise_path("/foo/../../bar"), "/bar");
    }

    #[test]
    fn normalise_dot() {
        assert_eq!(normalise_path("/foo/./bar"), "/foo/bar");
    }

    #[test]
    fn normalise_trailing_slash() {
        assert_eq!(normalise_path("/foo/bar/"), "/foo/bar");
    }

    #[test]
    fn normalise_root() {
        assert_eq!(normalise_path("/"), "/");
        assert_eq!(normalise_path(""), "/");
    }

    // --- resolve_path ---

    #[test]
    fn resolve_absolute() {
        assert_eq!(resolve_path("/home/user", "/etc/passwd"), "/etc/passwd");
    }

    #[test]
    fn resolve_relative() {
        assert_eq!(resolve_path("/home/user", "docs"), "/home/user/docs");
    }

    #[test]
    fn resolve_dotdot() {
        assert_eq!(resolve_path("/home/user/src", "../bin"), "/home/user/bin");
    }

    // --- format_size ---

    #[test]
    fn size_bytes() {
        assert_eq!(format_size(512), "512B");
    }

    #[test]
    fn size_kib() {
        assert_eq!(format_size(2048), "2.0K");
    }

    #[test]
    fn size_mib() {
        assert_eq!(format_size(3 * 1024 * 1024), "3.0M");
    }

    #[test]
    fn size_gib() {
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.0G");
    }

    // --- format_mode ---

    #[test]
    fn mode_regular_file() {
        let m = format_mode(0o100644);
        assert_eq!(&m[..1], "-");
        assert!(m.contains('r'));
    }

    #[test]
    fn mode_directory() {
        let m = format_mode(0o040755);
        assert_eq!(&m[..1], "d");
    }

    #[test]
    fn mode_symlink() {
        let m = format_mode(0o120777);
        assert_eq!(&m[..1], "l");
    }

    // --- parse_command ---

    #[test]
    fn parse_ls_no_args() {
        assert_eq!(parse_command("ls"), Ok(SftpCommand::Ls(None)));
    }

    #[test]
    fn parse_ls_with_path() {
        assert_eq!(parse_command("ls /tmp"), Ok(SftpCommand::Ls(Some("/tmp".into()))));
    }

    #[test]
    fn parse_cd() {
        assert_eq!(parse_command("cd /home"), Ok(SftpCommand::Cd("/home".into())));
    }

    #[test]
    fn parse_cd_missing_arg() {
        assert!(parse_command("cd").is_err());
    }

    #[test]
    fn parse_get_with_both_paths() {
        assert_eq!(
            parse_command("get /remote/file /local/dest"),
            Ok(SftpCommand::Get {
                remote: "/remote/file".into(),
                local: Some("/local/dest".into()),
            })
        );
    }

    #[test]
    fn parse_put_remote_default() {
        assert_eq!(
            parse_command("put localfile.txt"),
            Ok(SftpCommand::Put {
                local: "localfile.txt".into(),
                remote: None,
            })
        );
    }

    #[test]
    fn parse_chmod_valid() {
        assert_eq!(
            parse_command("chmod 755 script.sh"),
            Ok(SftpCommand::Chmod { mode: 0o755, path: "script.sh".into() })
        );
    }

    #[test]
    fn parse_chmod_bad_mode() {
        assert!(parse_command("chmod 999 file").is_err());
    }

    #[test]
    fn parse_rename() {
        assert_eq!(
            parse_command("rename old.txt new.txt"),
            Ok(SftpCommand::Rename { old: "old.txt".into(), new: "new.txt".into() })
        );
    }

    #[test]
    fn parse_quit_variants() {
        assert_eq!(parse_command("bye"), Ok(SftpCommand::Quit));
        assert_eq!(parse_command("quit"), Ok(SftpCommand::Quit));
        assert_eq!(parse_command("exit"), Ok(SftpCommand::Quit));
    }

    #[test]
    fn parse_shell_command() {
        assert_eq!(
            parse_command("!ls -la"),
            Ok(SftpCommand::Shell("ls -la".into()))
        );
    }

    #[test]
    fn parse_unknown_command() {
        assert!(parse_command("frobnicate").is_err());
    }

    // --- parse_args ---

    #[test]
    fn args_default() {
        let cfg = parse_args(&[]).unwrap();
        assert!(cfg.host.is_none());
        assert_eq!(cfg.port, 22);
        assert!(!cfg.verbose);
    }

    #[test]
    fn args_verbose_flag() {
        let args: Vec<String> = ["-v"].iter().map(|s| s.to_string()).collect();
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.verbose);
    }

    #[test]
    fn args_port() {
        let args: Vec<String> = ["-P", "2222"].iter().map(|s| s.to_string()).collect();
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.port, 2222);
    }

    #[test]
    fn args_user_at_host() {
        let args: Vec<String> = ["alice@example.com"].iter().map(|s| s.to_string()).collect();
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.user.as_deref(), Some("alice"));
        assert_eq!(cfg.host.as_deref(), Some("example.com"));
    }

    #[test]
    fn args_host_with_path() {
        let args: Vec<String> = ["alice@192.168.1.1:/home/alice"].iter().map(|s| s.to_string()).collect();
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.remote_start_path.as_deref(), Some("/home/alice"));
        assert_eq!(cfg.host.as_deref(), Some("192.168.1.1"));
    }

    // --- SFTP packet serialisation round-trip ---

    #[test]
    fn frame_and_parse_status_ok() {
        // Build a minimal SSH_FXP_STATUS OK packet.
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_STATUS);
        push_u32(&mut payload, 42); // request_id
        push_u32(&mut payload, status::SSH_FX_OK);
        push_str(&mut payload, "OK");
        push_str(&mut payload, "en"); // language tag (ignored)

        let pkt = parse_sftp_packet(&payload).unwrap();
        match pkt {
            SftpPacket::Status { request_id, code, message } => {
                assert_eq!(request_id, 42);
                assert_eq!(code, status::SSH_FX_OK);
                assert_eq!(message, "OK");
            }
            _ => panic!("expected Status packet"),
        }
    }

    #[test]
    fn parse_handle_packet() {
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_HANDLE);
        push_u32(&mut payload, 7); // request_id
        push_bytes(&mut payload, b"\x00\x01\x02\x03");
        let pkt = parse_sftp_packet(&payload).unwrap();
        match pkt {
            SftpPacket::Handle { request_id, handle } => {
                assert_eq!(request_id, 7);
                assert_eq!(handle, b"\x00\x01\x02\x03");
            }
            _ => panic!("expected Handle packet"),
        }
    }

    #[test]
    fn parse_name_packet() {
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_NAME);
        push_u32(&mut payload, 1); // request_id
        push_u32(&mut payload, 1); // count = 1
        push_str(&mut payload, "file.txt");
        push_str(&mut payload, "-rw-r--r-- 1 user group 100 Jan 1 file.txt");
        push_attrs(&mut payload, &FileAttrs { size: Some(100), ..Default::default() });
        let pkt = parse_sftp_packet(&payload).unwrap();
        match pkt {
            SftpPacket::Name { entries, .. } => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].filename, "file.txt");
                assert_eq!(entries[0].attrs.size, Some(100));
            }
            _ => panic!("expected Name packet"),
        }
    }

    #[test]
    fn parse_version_packet() {
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_VERSION);
        push_u32(&mut payload, 3);
        let pkt = parse_sftp_packet(&payload).unwrap();
        match pkt {
            SftpPacket::Version { version } => assert_eq!(version, 3),
            _ => panic!("expected Version packet"),
        }
    }

    #[test]
    fn parse_data_packet() {
        let mut payload = Vec::new();
        payload.push(fxp::SSH_FXP_DATA);
        push_u32(&mut payload, 9); // request_id
        push_bytes(&mut payload, b"hello world");
        let pkt = parse_sftp_packet(&payload).unwrap();
        match pkt {
            SftpPacket::Data { request_id, data } => {
                assert_eq!(request_id, 9);
                assert_eq!(data, b"hello world");
            }
            _ => panic!("expected Data packet"),
        }
    }

    #[test]
    fn parse_empty_packet_is_error() {
        assert!(parse_sftp_packet(&[]).is_err());
    }

    #[test]
    fn frame_packet_length_prefix() {
        let payload = b"hello";
        let framed = frame_packet(payload);
        assert_eq!(framed.len(), 9); // 4-byte length + 5 payload bytes
        let len = u32::from_be_bytes([framed[0], framed[1], framed[2], framed[3]]);
        assert_eq!(len, 5);
        assert_eq!(&framed[4..], b"hello");
    }

    // --- Cursor edge cases ---

    #[test]
    fn cursor_truncated_u32() {
        let buf = [0u8, 1u8, 2u8]; // only 3 bytes
        let mut cur = Cursor::new(&buf);
        assert!(cur.read_u32().is_err());
    }

    #[test]
    fn cursor_truncated_string() {
        let mut buf = Vec::new();
        push_u32(&mut buf, 100); // claim 100 bytes but provide none
        let mut cur = Cursor::new(&buf);
        assert!(cur.read_string().is_err());
    }

    // --- to_cstring ---

    #[test]
    fn cstring_null_terminated() {
        let cs = to_cstring("hello");
        assert_eq!(cs, b"hello\0");
    }

    #[test]
    fn cstring_empty() {
        let cs = to_cstring("");
        assert_eq!(cs, b"\0");
    }
}
