//! OurOS Internet Super-Server Daemon (inetd)
//!
//! Listens on configured TCP/UDP ports and dispatches incoming connections to
//! the appropriate service program. Supports both `wait` (persistent, one
//! instance at a time) and `nowait` (fork-per-connection) modes.
//!
//! # Configuration
//!
//! Reads `/etc/inetd.conf` with lines of the form:
//!
//! ```text
//! service_name  socket_type  protocol  wait/nowait  user  program  args...
//! ```
//!
//! Example entries:
//! ```text
//! echo      stream  tcp  nowait  root  internal
//! echo      dgram   udp  wait    root  internal
//! discard   stream  tcp  nowait  root  internal
//! daytime   stream  tcp  nowait  root  internal
//! chargen   stream  tcp  nowait  root  internal
//! time      stream  tcp  nowait  root  internal
//! ssh       stream  tcp  nowait  root  /usr/sbin/sshd  sshd -i
//! ```
//!
//! # Built-in Services
//!
//! - **echo**: echoes received data back to the sender
//! - **discard**: reads and discards all data
//! - **daytime**: sends the current date/time string then closes
//! - **chargen**: generates a continuous stream of printable characters
//! - **time**: sends a 32-bit network-order timestamp (seconds since 1900)
//!
//! # Usage
//!
//! ```text
//! inetd                         Start with /etc/inetd.conf
//! inetd -f /path/to/inetd.conf  Use alternate config
//! inetd -d                       Debug mode (foreground, verbose)
//! inetd -R rate                  Max connections per minute per source (default 256)
//! inetd -c max                   Max simultaneous connections per source (default 64)
//! ```

#![cfg_attr(not(test), no_main)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::needless_range_loop)]

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Syscall numbers (from kernel/src/syscall/number.rs)
// ============================================================================

// Some constants are only used on the real target (not during cross-build
// analysis) or are reserved for future SIGHUP reload / graceful shutdown.
#[allow(dead_code)]
const SYS_EXIT: u64 = 1;
const SYS_CLOCK_MONOTONIC: u64 = 10;
const SYS_SLEEP: u64 = 11;
const SYS_PROCESS_SPAWN: u64 = 500;
const SYS_PROCESS_ID: u64 = 502;
const SYS_FS_READ_FILE: u64 = 600;
const SYS_FS_WRITE_FILE: u64 = 601;
#[allow(dead_code)]
const SYS_TCP_CONNECT: u64 = 800;
const SYS_TCP_SEND: u64 = 801;
const SYS_TCP_RECV: u64 = 802;
const SYS_TCP_CLOSE: u64 = 803;
const SYS_TCP_BIND: u64 = 804;
const SYS_TCP_ACCEPT: u64 = 805;
#[allow(dead_code)]
const SYS_TCP_CLOSE_LISTENER: u64 = 806;
const SYS_TCP_PEER_ADDR: u64 = 808;
const SYS_TCP_SHUTDOWN: u64 = 855;

const SYS_UDP_BIND: u64 = 810;
const SYS_UDP_SEND: u64 = 811;
const SYS_UDP_RECV: u64 = 812;
#[allow(dead_code)]
const SYS_UDP_CLOSE: u64 = 813;
const SYS_UDP_RX_READY: u64 = 847;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 0-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall0(nr: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees nr is valid. The `syscall` instruction
    // clobbers rcx and r11 per the x86_64 ABI.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Issue a 1-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and `a1` is valid
/// for the specific syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall1(nr: u64, a1: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid.
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

/// Issue a 4-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for the specific syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall4(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid. r10 carries arg3 per
    // the syscall ABI (not rcx, which is clobbered by `syscall`).
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

/// Issue a 5-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for the specific syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall5(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid. Uses the full 5-argument
    // register convention: rdi, rsi, rdx, r10, r8.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

// Stubs for non-x86_64 targets (keeps tests compiling on the host).
#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall0(_nr: u64) -> i64 { -1 }
#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall1(_nr: u64, _a1: u64) -> i64 { -1 }
#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 { -1 }
#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall4(_nr: u64, _a1: u64, _a2: u64, _a3: u64, _a4: u64) -> i64 { -1 }
#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall5(_nr: u64, _a1: u64, _a2: u64, _a3: u64, _a4: u64, _a5: u64) -> i64 { -1 }

// ============================================================================
// Syscall wrappers — Process / Clock
// ============================================================================

/// Sleep for the given number of milliseconds.
fn sleep_ms(ms: u64) {
    // SAFETY: SYS_SLEEP takes one scalar argument (milliseconds).
    let _ = unsafe { syscall1(SYS_SLEEP, ms) };
}

/// Get monotonic clock time in milliseconds.
fn clock_monotonic_ms() -> u64 {
    // SAFETY: SYS_CLOCK_MONOTONIC takes no pointer arguments.
    let ret = unsafe { syscall0(SYS_CLOCK_MONOTONIC) };
    if ret < 0 { 0 } else { ret as u64 }
}

/// Get the current process ID.
fn get_pid() -> u64 {
    // SAFETY: SYS_PROCESS_ID takes no arguments, returns the pid.
    let ret = unsafe { syscall0(SYS_PROCESS_ID) };
    if ret < 0 { 0 } else { ret as u64 }
}

/// Spawn a new process. Returns child pid on success.
fn process_spawn(path: &str) -> Result<u64, InetdError> {
    // SAFETY: We pass a valid path pointer and its length.
    let ret = unsafe {
        syscall3(
            SYS_PROCESS_SPAWN,
            path.as_ptr() as u64,
            path.len() as u64,
            0,
        )
    };
    if ret < 0 {
        Err(InetdError::Spawn(format!(
            "process_spawn({path}) failed: {ret}"
        )))
    } else {
        Ok(ret as u64)
    }
}

/// Exit the current process.
#[allow(dead_code)] // Used on the real target for graceful shutdown.
fn sys_exit(code: i32) -> ! {
    // SAFETY: SYS_EXIT takes one scalar argument and never returns.
    unsafe { syscall1(SYS_EXIT, code as u64) };
    loop {}
}

// ============================================================================
// Syscall wrappers — Filesystem
// ============================================================================

/// Read an entire file into a byte vector via the kernel filesystem.
fn fs_read_file(path: &str) -> Result<Vec<u8>, InetdError> {
    let mut buf = vec![0u8; 65536];
    // SAFETY: We pass a valid path pointer+len and a valid output buffer
    // pointer+len. The kernel reads the path and writes file contents into buf.
    let ret = unsafe {
        syscall4(
            SYS_FS_READ_FILE,
            path.as_ptr() as u64,
            path.len() as u64,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(InetdError::Config(format!(
            "cannot read {path}: error {ret}"
        )));
    }
    buf.truncate(ret as usize);
    Ok(buf)
}

/// Write data to a file (for PID file, logging).
fn fs_write_file(path: &str, data: &[u8]) -> Result<(), InetdError> {
    // SAFETY: We pass a valid path pointer+len and a valid data pointer+len.
    let ret = unsafe {
        syscall4(
            SYS_FS_WRITE_FILE,
            path.as_ptr() as u64,
            path.len() as u64,
            data.as_ptr() as u64,
            data.len() as u64,
        )
    };
    if ret < 0 {
        Err(InetdError::Config(format!(
            "cannot write {path}: error {ret}"
        )))
    } else {
        Ok(())
    }
}

// ============================================================================
// Syscall wrappers — TCP
// ============================================================================

/// Bind a TCP listener to a local port. Returns a listener handle.
#[allow(dead_code)] // tcp_bind_addr is used instead; kept for simple binding.
fn tcp_bind(port: u16) -> Result<u64, InetdError> {
    // SAFETY: SYS_TCP_BIND takes one scalar argument (port number).
    let ret = unsafe { syscall1(SYS_TCP_BIND, u64::from(port)) };
    if ret < 0 {
        Err(InetdError::Network(format!(
            "tcp_bind({port}) failed: {ret}"
        )))
    } else {
        Ok(ret as u64)
    }
}

/// Accept an incoming connection on a listener (blocking).
/// Returns a connection handle.
#[allow(dead_code)] // Non-blocking variant used in poll loop; kept for wait-mode services.
fn tcp_accept(listener: u64) -> Result<u64, InetdError> {
    // SAFETY: listener is a valid listener handle from tcp_bind.
    let ret = unsafe { syscall1(SYS_TCP_ACCEPT, listener) };
    if ret < 0 {
        Err(InetdError::Network(format!(
            "tcp_accept failed: {ret}"
        )))
    } else {
        Ok(ret as u64)
    }
}

/// Non-blocking accept: returns `Err(-11)` (WouldBlock) if no connection pending.
fn tcp_accept_nonblock(listener: u64) -> Result<u64, i64> {
    // SAFETY: listener handle is valid. arg1=1 requests non-blocking mode.
    let ret = unsafe { syscall3(SYS_TCP_ACCEPT, listener, 1, 0) };
    if ret < 0 { Err(ret) } else { Ok(ret as u64) }
}

/// Send data on a TCP connection. Returns the number of bytes sent.
fn tcp_send(handle: u64, data: &[u8]) -> Result<usize, InetdError> {
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
        Err(InetdError::Network("tcp_send failed".into()))
    } else {
        Ok(ret as usize)
    }
}

/// Send all bytes, looping until the entire buffer is transmitted.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), InetdError> {
    let mut offset = 0;
    while offset < data.len() {
        let n = tcp_send(handle, &data[offset..])?;
        if n == 0 {
            return Err(InetdError::Network("tcp_send returned 0".into()));
        }
        offset = offset
            .checked_add(n)
            .ok_or_else(|| InetdError::Network("offset overflow".into()))?;
    }
    Ok(())
}

/// Receive data from a TCP connection. Returns 0 on EOF (peer closed).
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, InetdError> {
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
        Err(InetdError::Network("tcp_recv failed".into()))
    } else {
        Ok(ret as usize)
    }
}

/// Non-blocking receive: returns WouldBlock (-11) if no data ready.
#[allow(dead_code)] // Reserved for future non-blocking service I/O.
fn tcp_recv_nonblock(handle: u64, buf: &mut [u8]) -> Result<usize, i64> {
    const MSG_DONTWAIT: u64 = 0x40;
    // SAFETY: handle is valid. buf is a valid mutable slice. arg3 carries
    // the MSG_DONTWAIT flag.
    let ret = unsafe {
        syscall4(
            SYS_TCP_RECV,
            handle,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
            MSG_DONTWAIT,
        )
    };
    if ret < 0 { Err(ret) } else { Ok(ret as usize) }
}

/// Close a TCP connection handle.
fn tcp_close(handle: u64) {
    // SAFETY: handle is (or was) a valid TCP connection handle.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

/// Bind a TCP listener with a specific bind address.
/// `bind_addr` = 0 means INADDR_ANY; otherwise it is an IPv4 address in
/// network byte order.
fn tcp_bind_addr(addr: u32, port: u16) -> Result<u64, InetdError> {
    // SAFETY: SYS_TCP_BIND takes the port as arg0, bind address as arg1 (if
    // supported by the kernel). Falls back to port-only binding when addr=0.
    let ret = if addr == 0 {
        unsafe { syscall1(SYS_TCP_BIND, u64::from(port)) }
    } else {
        unsafe { syscall3(SYS_TCP_BIND, u64::from(port), u64::from(addr), 0) }
    };
    if ret < 0 {
        Err(InetdError::Network(format!(
            "tcp_bind({}, {port}) failed: {ret}",
            format_ip(addr)
        )))
    } else {
        Ok(ret as u64)
    }
}

/// Close a TCP listener handle.
#[allow(dead_code)] // Used in close_all_listeners for graceful shutdown.
fn tcp_close_listener(listener: u64) {
    // SAFETY: listener is (or was) a valid TCP listener handle.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE_LISTENER, listener) };
}

/// Get the peer address of a TCP connection.
/// Returns (ip_u32_network_order, port) on success.
fn tcp_peer_addr(handle: u64) -> Result<(u32, u16), InetdError> {
    let mut buf = [0u8; 6];
    // SAFETY: handle is valid. buf is a stack-allocated 6-byte buffer.
    let ret = unsafe {
        syscall3(
            SYS_TCP_PEER_ADDR,
            handle,
            buf.as_mut_ptr() as u64,
            0,
        )
    };
    if ret < 0 {
        return Err(InetdError::Network("tcp_peer_addr failed".into()));
    }
    let ip = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let port = u16::from_be_bytes([buf[4], buf[5]]);
    Ok((ip, port))
}

/// Shut down part of a TCP connection.
/// how: 0=read, 1=write, 2=both.
fn tcp_shutdown(handle: u64, how: u32) {
    // SAFETY: handle is valid. how is 0, 1, or 2.
    let _ = unsafe { syscall3(SYS_TCP_SHUTDOWN, handle, u64::from(how), 0) };
}

// ============================================================================
// Syscall wrappers — UDP
// ============================================================================

/// Bind a UDP socket to a local port. Returns a handle on success.
fn udp_bind(port: u16) -> Result<u64, InetdError> {
    // SAFETY: SYS_UDP_BIND takes one scalar argument (port number).
    let ret = unsafe { syscall1(SYS_UDP_BIND, u64::from(port)) };
    if ret < 0 {
        Err(InetdError::Network(format!(
            "udp_bind({port}) failed: {ret}"
        )))
    } else {
        Ok(ret as u64)
    }
}

/// Send a UDP datagram.
fn udp_send(handle: u64, dst_ip: u32, dst_port: u16, data: &[u8]) -> Result<(), InetdError> {
    // SAFETY: handle is a valid UDP socket. dst_ip and dst_port are scalars.
    // data pointer and length are derived from a valid Rust slice.
    let ret = unsafe {
        syscall5(
            SYS_UDP_SEND,
            handle,
            u64::from(dst_ip),
            u64::from(dst_port),
            data.as_ptr() as u64,
            data.len() as u64,
        )
    };
    if ret < 0 {
        Err(InetdError::Network("udp_send failed".into()))
    } else {
        Ok(())
    }
}

/// Receive a UDP datagram (non-blocking). Returns (bytes_read, src_ip, src_port).
/// Returns `Err(-11)` (WouldBlock) if no datagram is queued.
fn udp_recv(handle: u64, buf: &mut [u8]) -> Result<(usize, u32, u16), i64> {
    let mut src_info = [0u8; 6]; // 4 bytes IP + 2 bytes port
    // SAFETY: handle is a valid UDP socket. buf is a valid mutable slice.
    // src_info is a 6-byte stack buffer for the kernel to write source addr.
    let ret = unsafe {
        syscall4(
            SYS_UDP_RECV,
            handle,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
            src_info.as_mut_ptr() as u64,
        )
    };
    if ret < 0 {
        return Err(ret);
    }
    let src_ip = u32::from_be_bytes([src_info[0], src_info[1], src_info[2], src_info[3]]);
    let src_port = u16::from_le_bytes([src_info[4], src_info[5]]);
    Ok((ret as usize, src_ip, src_port))
}

/// Check if a UDP socket has queued datagrams.
fn udp_rx_ready(handle: u64) -> bool {
    // SAFETY: handle is a valid UDP socket. Returns a scalar count.
    let ret = unsafe { syscall1(SYS_UDP_RX_READY, handle) };
    ret > 0
}

/// Close a UDP socket handle.
#[allow(dead_code)] // Used in close_all_listeners for graceful shutdown.
fn udp_close(handle: u64) {
    // SAFETY: handle is (or was) a valid UDP socket handle.
    let _ = unsafe { syscall1(SYS_UDP_CLOSE, handle) };
}

// ============================================================================
// Helper utilities
// ============================================================================

/// Format an IPv4 address from network byte order u32.
fn format_ip(ip: u32) -> String {
    let b = ip.to_be_bytes();
    format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
}

/// Parse a dotted-quad IPv4 string into network-byte-order u32.
#[allow(dead_code)] // Used in tests and future bind-address configuration.
fn parse_ipv4(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        octets[i] = part.parse::<u8>().ok()?;
    }
    Some(u32::from_be_bytes(octets))
}

/// Get current Unix timestamp in seconds.
fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format a Unix timestamp as a human-readable daytime string.
/// Format: "Weekday Month DD HH:MM:SS YYYY\r\n"
fn format_daytime(unix_secs: u64) -> String {
    let bt = unix_to_broken(unix_secs);
    let weekday = match bt.weekday {
        0 => "Sun", 1 => "Mon", 2 => "Tue", 3 => "Wed",
        4 => "Thu", 5 => "Fri", _ => "Sat",
    };
    let month = match bt.month {
        1 => "Jan",   2 => "Feb",  3 => "Mar",  4 => "Apr",
        5 => "May",   6 => "Jun",  7 => "Jul",  8 => "Aug",
        9 => "Sep",  10 => "Oct", 11 => "Nov",  _ => "Dec",
    };
    format!(
        "{weekday} {month} {:02} {:02}:{:02}:{:02} {}\r\n",
        bt.day, bt.hour, bt.minute, bt.second, bt.year
    )
}

/// Broken-down time from Unix seconds (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BrokenTime {
    year: i64,
    month: u32,   // 1-12
    day: u32,     // 1-31
    hour: u32,    // 0-23
    minute: u32,  // 0-59
    second: u32,  // 0-59
    weekday: u32, // 0=Sunday, 1=Monday, ..., 6=Saturday
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn unix_to_broken(unix_secs: u64) -> BrokenTime {
    let secs = unix_secs;
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hour = (time_secs / 3600) as u32;
    let minute = ((time_secs % 3600) / 60) as u32;
    let second = (time_secs % 60) as u32;

    // Weekday: Jan 1 1970 was Thursday (4).
    let weekday = ((days + 4) % 7) as u32;

    // Calculate year/month/day from days since epoch.
    let mut y: i64 = 1970;
    let mut remaining = days;
    loop {
        let days_in_year: u64 = if is_leap_year(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let days_in_months: [u32; 12] = if is_leap_year(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0u32;
    for (i, &dim) in days_in_months.iter().enumerate() {
        if remaining < u64::from(dim) {
            month = (i as u32) + 1;
            break;
        }
        remaining -= u64::from(dim);
    }
    if month == 0 {
        month = 12;
    }
    let day = (remaining as u32) + 1;

    BrokenTime { year: y, month, day, hour, minute, second, weekday }
}

// ============================================================================
// Error types
// ============================================================================

#[derive(Debug)]
enum InetdError {
    Config(String),
    Network(String),
    Spawn(String),
    RateLimit(String),
}

impl fmt::Display for InetdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(s) => write!(f, "config: {s}"),
            Self::Network(s) => write!(f, "network: {s}"),
            Self::Spawn(s) => write!(f, "spawn: {s}"),
            Self::RateLimit(s) => write!(f, "rate limit: {s}"),
        }
    }
}

// ============================================================================
// Logging
// ============================================================================

/// Log level for inetd messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Debug => write!(f, "DEBUG"),
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARNING"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

/// Global logger configuration. Since we are a single-threaded daemon, this
/// is a simple struct rather than an atomic/mutex-based global.
struct Logger {
    debug: bool,
    log_to_file: bool,
    log_path: String,
}

impl Logger {
    fn new(debug: bool) -> Self {
        Self {
            debug,
            log_to_file: true,
            log_path: String::from("/var/log/inetd.log"),
        }
    }

    fn log(&self, level: LogLevel, msg: &str) {
        if level == LogLevel::Debug && !self.debug {
            return;
        }
        let ts = now_unix_secs();
        let line = format!("[{ts}] inetd[{}]: {level}: {msg}\n", get_pid());

        // Always write to stderr in debug mode.
        if self.debug || level >= LogLevel::Warning {
            eprint!("{line}");
        }

        // Attempt to append to log file.
        if self.log_to_file {
            let _ = fs_write_file(&self.log_path, line.as_bytes());
        }
    }
}

// ============================================================================
// Configuration model
// ============================================================================

/// Socket type: stream (TCP) or datagram (UDP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocketType {
    Stream,
    Dgram,
}

impl fmt::Display for SocketType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stream => write!(f, "stream"),
            Self::Dgram => write!(f, "dgram"),
        }
    }
}

/// Protocol: tcp or udp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    Tcp,
    Udp,
    Tcp6,
    Udp6,
}

impl Protocol {
    /// Whether this protocol uses IPv6.
    #[allow(dead_code)] // Used in tests; will be needed for IPv6 listener binding.
    fn is_v6(self) -> bool {
        matches!(self, Self::Tcp6 | Self::Udp6)
    }

    /// Whether this is a TCP-family protocol.
    fn is_tcp(self) -> bool {
        matches!(self, Self::Tcp | Self::Tcp6)
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp => write!(f, "tcp"),
            Self::Udp => write!(f, "udp"),
            Self::Tcp6 => write!(f, "tcp6"),
            Self::Udp6 => write!(f, "udp6"),
        }
    }
}

/// Wait mode: `wait` means the daemon hands the socket to a single instance
/// that handles all requests (typical for UDP). `nowait` means the daemon
/// forks a new process per connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitMode {
    Wait,
    Nowait,
}

impl fmt::Display for WaitMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wait => write!(f, "wait"),
            Self::Nowait => write!(f, "nowait"),
        }
    }
}

/// Identifies a built-in (internal) service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuiltinService {
    Echo,
    Discard,
    Daytime,
    Chargen,
    Time,
}

impl fmt::Display for BuiltinService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Echo => write!(f, "echo"),
            Self::Discard => write!(f, "discard"),
            Self::Daytime => write!(f, "daytime"),
            Self::Chargen => write!(f, "chargen"),
            Self::Time => write!(f, "time"),
        }
    }
}

/// Maps well-known service names to their default port numbers.
fn service_to_port(name: &str) -> Option<u16> {
    match name {
        "echo" => Some(7),
        "discard" | "sink" | "null" => Some(9),
        "daytime" => Some(13),
        "chargen" | "ttytst" => Some(19),
        "ftp-data" => Some(20),
        "ftp" => Some(21),
        "ssh" => Some(22),
        "telnet" => Some(23),
        "smtp" => Some(25),
        "time" | "timserver" => Some(37),
        "nameserver" | "name" => Some(42),
        "whois" | "nicname" => Some(43),
        "domain" | "dns" => Some(53),
        "finger" => Some(79),
        "http" | "www" => Some(80),
        "pop3" | "pop-3" => Some(110),
        "ident" | "auth" => Some(113),
        "nntp" => Some(119),
        "ntp" => Some(123),
        "imap" => Some(143),
        "snmp" => Some(161),
        "https" => Some(443),
        "login" | "rlogin" => Some(513),
        "shell" | "cmd" => Some(514),
        "printer" | "spooler" => Some(515),
        _ => name.parse::<u16>().ok(),
    }
}

/// A single service entry from the configuration file.
#[derive(Debug, Clone)]
struct ServiceEntry {
    /// Service name (e.g. "echo", "ssh").
    service_name: String,
    /// Port number to listen on.
    port: u16,
    /// Stream (TCP) or datagram (UDP).
    socket_type: SocketType,
    /// Protocol (tcp, udp, tcp6, udp6).
    protocol: Protocol,
    /// Wait or nowait.
    wait_mode: WaitMode,
    /// User to run the service as (used when spawning external programs).
    #[allow(dead_code)]
    user: String,
    /// Path to the server program (or "internal" for built-in services).
    program: String,
    /// Arguments to pass to the server program (including argv[0]).
    #[allow(dead_code)] // Passed to child process via exec; not yet wired to process_spawn.
    args: Vec<String>,
    /// Built-in service handler, if applicable.
    builtin: Option<BuiltinService>,
    /// Whether this service is currently enabled.
    enabled: bool,
    /// Maximum connections per source IP per minute (0 = no limit).
    max_rate: u32,
    /// Maximum concurrent connections per source IP (0 = no limit).
    max_per_source: u32,
}

impl ServiceEntry {
    fn is_internal(&self) -> bool {
        self.builtin.is_some()
    }
}

impl fmt::Display for ServiceEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{} {} {} {} {}",
            self.service_name,
            self.port,
            self.socket_type,
            self.protocol,
            self.wait_mode,
            if self.is_internal() {
                "internal".to_string()
            } else {
                self.program.clone()
            }
        )
    }
}

/// Daemon-wide configuration.
#[derive(Debug)]
struct Config {
    /// Path to the configuration file.
    config_path: String,
    /// Debug mode: stay in foreground, extra logging.
    debug: bool,
    /// Maximum connections per source per minute (global default).
    rate_limit: u32,
    /// Maximum simultaneous connections per source (global default).
    max_per_source: u32,
    /// Parsed service entries.
    services: Vec<ServiceEntry>,
}

impl Config {
    fn new() -> Self {
        Self {
            config_path: String::from("/etc/inetd.conf"),
            debug: false,
            rate_limit: 256,
            max_per_source: 64,
            services: Vec::new(),
        }
    }
}

// ============================================================================
// Configuration parser
// ============================================================================

/// Resolve a builtin service tag from the service name.
fn resolve_builtin(name: &str) -> Option<BuiltinService> {
    match name {
        "echo" => Some(BuiltinService::Echo),
        "discard" | "sink" | "null" => Some(BuiltinService::Discard),
        "daytime" => Some(BuiltinService::Daytime),
        "chargen" | "ttytst" => Some(BuiltinService::Chargen),
        "time" | "timserver" => Some(BuiltinService::Time),
        _ => None,
    }
}

/// Parse a single non-comment, non-empty line from inetd.conf.
///
/// Format: `service_name  socket_type  protocol  wait/nowait  user  program  [args...]`
fn parse_config_line(line: &str, global_rate: u32, global_max: u32) -> Result<ServiceEntry, String> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 6 {
        return Err(format!("too few fields (need >= 6, got {}): {line}", fields.len()));
    }

    let service_name = fields[0];
    let socket_type = match fields[1] {
        "stream" => SocketType::Stream,
        "dgram" => SocketType::Dgram,
        other => return Err(format!("unknown socket type '{other}'")),
    };
    let protocol = match fields[2] {
        "tcp" => Protocol::Tcp,
        "udp" => Protocol::Udp,
        "tcp6" => Protocol::Tcp6,
        "udp6" => Protocol::Udp6,
        other => return Err(format!("unknown protocol '{other}'")),
    };

    // Validate socket_type/protocol consistency.
    match (socket_type, protocol.is_tcp()) {
        (SocketType::Stream, false) => {
            return Err(format!(
                "stream socket with non-TCP protocol '{}'",
                protocol
            ));
        }
        (SocketType::Dgram, true) => {
            return Err(format!(
                "dgram socket with TCP protocol '{}'",
                protocol
            ));
        }
        _ => {}
    }

    // Parse wait/nowait, optionally with max-connections suffix: "nowait.64"
    let wait_field = fields[3];
    let (wait_mode, per_source_override) = if let Some(rest) = wait_field.strip_prefix("nowait") {
        let limit = if let Some(dotted) = rest.strip_prefix('.') {
            dotted.parse::<u32>().ok()
        } else {
            None
        };
        (WaitMode::Nowait, limit)
    } else if wait_field == "wait" || wait_field.starts_with("wait.") {
        (WaitMode::Wait, None)
    } else {
        return Err(format!("unknown wait mode '{wait_field}'"));
    };

    let user = fields[4].to_string();
    let program = fields[5].to_string();
    let args: Vec<String> = if fields.len() > 6 {
        fields[6..].iter().map(|s| (*s).to_string()).collect()
    } else {
        vec![program.clone()]
    };

    let port = service_to_port(service_name)
        .ok_or_else(|| format!("unknown service name '{service_name}'"))?;

    let builtin = if program == "internal" {
        resolve_builtin(service_name)
    } else {
        None
    };

    // If the program is "internal" but we have no builtin handler, reject it.
    if program == "internal" && builtin.is_none() {
        return Err(format!(
            "no built-in handler for service '{service_name}'"
        ));
    }

    Ok(ServiceEntry {
        service_name: service_name.to_string(),
        port,
        socket_type,
        protocol,
        wait_mode,
        user,
        program,
        args,
        builtin,
        enabled: true,
        max_rate: global_rate,
        max_per_source: per_source_override.unwrap_or(global_max),
    })
}

/// Parse the entire configuration file content.
fn parse_config(content: &str, global_rate: u32, global_max: u32) -> Result<Vec<ServiceEntry>, String> {
    let mut services = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        // Skip blank lines and comments.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match parse_config_line(trimmed, global_rate, global_max) {
            Ok(entry) => services.push(entry),
            Err(e) => {
                return Err(format!("line {}: {e}", line_num + 1));
            }
        }
    }
    Ok(services)
}

// ============================================================================
// Rate limiting / Connection tracking
// ============================================================================

/// Per-source-IP connection tracking entry.
#[derive(Debug, Clone)]
struct SourceTracker {
    /// Number of currently active connections.
    active_count: u32,
    /// Timestamps (monotonic ms) of connections in the current rate window.
    recent_timestamps: Vec<u64>,
}

impl SourceTracker {
    fn new() -> Self {
        Self {
            active_count: 0,
            recent_timestamps: Vec::new(),
        }
    }

    /// Prune timestamps older than 60 seconds from the rate window.
    fn prune(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(60_000);
        self.recent_timestamps.retain(|&ts| ts >= cutoff);
    }

    /// Record a new connection attempt.
    fn record_connection(&mut self, now_ms: u64) {
        self.prune(now_ms);
        self.recent_timestamps.push(now_ms);
        self.active_count = self.active_count.saturating_add(1);
    }

    /// Signal that a connection has ended.
    fn release_connection(&mut self) {
        self.active_count = self.active_count.saturating_sub(1);
    }

    /// Number of connections in the last 60 seconds.
    fn rate(&self) -> u32 {
        self.recent_timestamps.len() as u32
    }
}

/// Connection tracker for all source IPs, keyed by (source_ip, service_port).
struct ConnectionTracker {
    /// Map of (source_ip, service_port) -> `SourceTracker`.
    sources: HashMap<(u32, u16), SourceTracker>,
}

impl ConnectionTracker {
    fn new() -> Self {
        Self {
            sources: HashMap::new(),
        }
    }

    /// Check if a connection from `src_ip` to `service_port` is allowed under
    /// the given rate and concurrency limits. Returns `Ok(())` if allowed,
    /// or an error describing why it was rejected.
    fn check_allowed(
        &mut self,
        src_ip: u32,
        service_port: u16,
        max_rate: u32,
        max_per_source: u32,
        now_ms: u64,
    ) -> Result<(), InetdError> {
        let tracker = self
            .sources
            .entry((src_ip, service_port))
            .or_insert_with(SourceTracker::new);
        tracker.prune(now_ms);

        if max_per_source > 0 && tracker.active_count >= max_per_source {
            return Err(InetdError::RateLimit(format!(
                "max connections per source ({max_per_source}) reached for {} on port {service_port}",
                format_ip(src_ip)
            )));
        }
        if max_rate > 0 && tracker.rate() >= max_rate {
            return Err(InetdError::RateLimit(format!(
                "rate limit ({max_rate}/min) exceeded for {} on port {service_port}",
                format_ip(src_ip)
            )));
        }
        Ok(())
    }

    /// Record a new accepted connection.
    fn record(&mut self, src_ip: u32, service_port: u16, now_ms: u64) {
        let tracker = self
            .sources
            .entry((src_ip, service_port))
            .or_insert_with(SourceTracker::new);
        tracker.record_connection(now_ms);
    }

    /// Release a connection (decrement active count).
    fn release(&mut self, src_ip: u32, service_port: u16) {
        if let Some(tracker) = self.sources.get_mut(&(src_ip, service_port)) {
            tracker.release_connection();
        }
    }

    /// Clean up entries with no active connections and no recent timestamps.
    fn garbage_collect(&mut self, now_ms: u64) {
        self.sources.retain(|_key, tracker| {
            tracker.prune(now_ms);
            tracker.active_count > 0 || !tracker.recent_timestamps.is_empty()
        });
    }

    /// Get the total number of tracked sources.
    #[allow(dead_code)] // Used in tests and future status reporting.
    fn tracked_sources(&self) -> usize {
        self.sources.len()
    }

    /// Get active connection count for a given source/port pair.
    #[allow(dead_code)] // Used in tests and future status reporting.
    fn active_for(&self, src_ip: u32, service_port: u16) -> u32 {
        self.sources
            .get(&(src_ip, service_port))
            .map_or(0, |t| t.active_count)
    }

    /// Get rate (connections in last minute) for a given source/port pair.
    #[allow(dead_code)] // Used in tests and future status reporting.
    fn rate_for(&mut self, src_ip: u32, service_port: u16, now_ms: u64) -> u32 {
        if let Some(tracker) = self.sources.get_mut(&(src_ip, service_port)) {
            tracker.prune(now_ms);
            tracker.rate()
        } else {
            0
        }
    }
}

// ============================================================================
// Built-in service handlers (TCP)
// ============================================================================

/// Handle the "echo" service for a TCP connection: read data and send it back.
fn handle_tcp_echo(handle: u64) {
    let mut buf = [0u8; 4096];
    loop {
        let n = match tcp_recv(handle, &mut buf) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            break; // EOF
        }
        if tcp_send_all(handle, &buf[..n]).is_err() {
            break;
        }
    }
}

/// Handle the "discard" service for a TCP connection: read and throw away.
fn handle_tcp_discard(handle: u64) {
    let mut buf = [0u8; 4096];
    loop {
        match tcp_recv(handle, &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {} // discard
        }
    }
}

/// Handle the "daytime" service: send current time string, then close.
fn handle_tcp_daytime(handle: u64) {
    let ts = now_unix_secs();
    let msg = format_daytime(ts);
    let _ = tcp_send_all(handle, msg.as_bytes());
}

/// Chargen pattern: rotating printable ASCII characters (RFC 864).
/// Generates lines of 72 characters from the printable range 32..126,
/// starting at a rotating offset.
fn chargen_line(offset: u32) -> Vec<u8> {
    let mut line = Vec::with_capacity(74);
    for i in 0..72u32 {
        let ch = 32 + ((offset + i) % 95);
        line.push(ch as u8);
    }
    line.push(b'\r');
    line.push(b'\n');
    line
}

/// Handle the "chargen" service for a TCP connection: send continuous character
/// pattern until the client disconnects.
fn handle_tcp_chargen(handle: u64) {
    let mut offset: u32 = 0;
    loop {
        let line = chargen_line(offset);
        if tcp_send_all(handle, &line).is_err() {
            break;
        }
        offset = (offset + 1) % 95;
    }
}

/// Handle the "time" service (RFC 868): send 32-bit seconds since
/// 1900-01-01 00:00:00 UTC in network byte order, then close.
///
/// The epoch offset between Unix (1970) and RFC 868 (1900) is 2_208_988_800
/// seconds.
fn handle_tcp_time(handle: u64) {
    const RFC868_EPOCH_OFFSET: u64 = 2_208_988_800;
    let ts = now_unix_secs().saturating_add(RFC868_EPOCH_OFFSET);
    // Truncate to 32 bits as per RFC 868.
    let val = (ts & 0xFFFF_FFFF) as u32;
    let bytes = val.to_be_bytes();
    let _ = tcp_send_all(handle, &bytes);
}

/// Dispatch a TCP connection to the appropriate built-in handler.
fn handle_tcp_builtin(service: BuiltinService, handle: u64) {
    match service {
        BuiltinService::Echo => handle_tcp_echo(handle),
        BuiltinService::Discard => handle_tcp_discard(handle),
        BuiltinService::Daytime => handle_tcp_daytime(handle),
        BuiltinService::Chargen => handle_tcp_chargen(handle),
        BuiltinService::Time => handle_tcp_time(handle),
    }
}

// ============================================================================
// Built-in service handlers (UDP)
// ============================================================================

/// Handle a UDP echo request: send the datagram back to the sender.
fn handle_udp_echo(handle: u64, data: &[u8], src_ip: u32, src_port: u16) {
    let _ = udp_send(handle, src_ip, src_port, data);
}

/// Handle a UDP discard request: do nothing with the data.
fn handle_udp_discard(_handle: u64, _data: &[u8], _src_ip: u32, _src_port: u16) {
    // Intentionally empty.
}

/// Handle a UDP daytime request: send the current time string.
fn handle_udp_daytime(handle: u64, _data: &[u8], src_ip: u32, src_port: u16) {
    let ts = now_unix_secs();
    let msg = format_daytime(ts);
    let _ = udp_send(handle, src_ip, src_port, msg.as_bytes());
}

/// Handle a UDP chargen request: send a single chargen line.
fn handle_udp_chargen(handle: u64, _data: &[u8], src_ip: u32, src_port: u16) {
    let line = chargen_line(0);
    let _ = udp_send(handle, src_ip, src_port, &line);
}

/// Handle a UDP time request: send RFC 868 timestamp.
fn handle_udp_time(handle: u64, _data: &[u8], src_ip: u32, src_port: u16) {
    const RFC868_EPOCH_OFFSET: u64 = 2_208_988_800;
    let ts = now_unix_secs().saturating_add(RFC868_EPOCH_OFFSET);
    let val = (ts & 0xFFFF_FFFF) as u32;
    let bytes = val.to_be_bytes();
    let _ = udp_send(handle, src_ip, src_port, &bytes);
}

/// Dispatch a UDP datagram to the appropriate built-in handler.
fn handle_udp_builtin(
    service: BuiltinService,
    handle: u64,
    data: &[u8],
    src_ip: u32,
    src_port: u16,
) {
    match service {
        BuiltinService::Echo => handle_udp_echo(handle, data, src_ip, src_port),
        BuiltinService::Discard => handle_udp_discard(handle, data, src_ip, src_port),
        BuiltinService::Daytime => handle_udp_daytime(handle, data, src_ip, src_port),
        BuiltinService::Chargen => handle_udp_chargen(handle, data, src_ip, src_port),
        BuiltinService::Time => handle_udp_time(handle, data, src_ip, src_port),
    }
}

// ============================================================================
// Active listener state
// ============================================================================

/// A bound listener (TCP) or socket (UDP) associated with a service entry.
#[derive(Debug)]
struct ListenerSlot {
    /// Index into `Config::services`.
    service_idx: usize,
    /// Kernel handle for the listener (TCP) or socket (UDP).
    handle: u64,
    /// Whether this is a TCP listener or UDP socket.
    is_tcp: bool,
    /// For `wait` mode services, whether the service instance is currently
    /// running (so we don't accept new connections on this socket).
    wait_busy: bool,
}

/// An active TCP connection being serviced in nowait mode.
/// Fields are read when cleaning up child processes on the real target.
#[derive(Debug)]
#[allow(dead_code)]
struct ActiveConnection {
    /// Kernel handle for the TCP connection.
    handle: u64,
    /// Source IP for connection tracking.
    src_ip: u32,
    /// Port of the service that accepted this connection.
    service_port: u16,
    /// Spawned child process ID (0 for built-in services).
    child_pid: u64,
}

// ============================================================================
// Command-line parsing
// ============================================================================

/// Parse command-line arguments into a `Config`.
fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut cfg = Config::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-f" => {
                i += 1;
                if i >= args.len() {
                    return Err("-f requires an argument".into());
                }
                cfg.config_path = args[i].clone();
            }
            "-d" => {
                cfg.debug = true;
            }
            "-R" => {
                i += 1;
                if i >= args.len() {
                    return Err("-R requires an argument".into());
                }
                cfg.rate_limit = args[i]
                    .parse::<u32>()
                    .map_err(|e| format!("-R: {e}"))?;
            }
            "-c" => {
                i += 1;
                if i >= args.len() {
                    return Err("-c requires an argument".into());
                }
                cfg.max_per_source = args[i]
                    .parse::<u32>()
                    .map_err(|e| format!("-c: {e}"))?;
            }
            "-h" | "--help" => {
                return Err(String::new()); // Triggers help display.
            }
            other => {
                // Treat a bare positional argument as the config file path.
                if other.starts_with('-') {
                    return Err(format!("unknown option: {other}"));
                }
                cfg.config_path = other.to_string();
            }
        }
        i += 1;
    }
    Ok(cfg)
}

fn print_help() {
    let help = "\
Usage: inetd [OPTIONS] [config-file]

Options:
  -f <path>   Configuration file (default: /etc/inetd.conf)
  -d          Debug mode (stay foreground, verbose logging)
  -R <rate>   Max connections per source per minute (default: 256)
  -c <max>    Max simultaneous connections per source (default: 64)
  -h, --help  Show this help message

Configuration file format:
  service  socket_type  protocol  wait/nowait[.max]  user  program  [args...]

Built-in services (program = \"internal\"):
  echo, discard, daytime, chargen, time
";
    eprint!("{help}");
}

// ============================================================================
// Daemon main loop
// ============================================================================

/// Bind all configured services and return the listener slots.
fn bind_services(services: &[ServiceEntry], logger: &Logger) -> Vec<ListenerSlot> {
    let mut slots = Vec::new();
    for (idx, svc) in services.iter().enumerate() {
        if !svc.enabled {
            logger.log(LogLevel::Info, &format!("skipping disabled service: {svc}"));
            continue;
        }
        let result = if svc.protocol.is_tcp() {
            tcp_bind_addr(0, svc.port).map(|h| (h, true))
        } else {
            udp_bind(svc.port).map(|h| (h, false))
        };

        match result {
            Ok((handle, is_tcp)) => {
                logger.log(
                    LogLevel::Info,
                    &format!("bound {svc} on port {}", svc.port),
                );
                slots.push(ListenerSlot {
                    service_idx: idx,
                    handle,
                    is_tcp,
                    wait_busy: false,
                });
            }
            Err(e) => {
                logger.log(
                    LogLevel::Error,
                    &format!("failed to bind {svc}: {e}"),
                );
            }
        }
    }
    slots
}

/// Close all listener/socket handles.
#[allow(dead_code)] // Called during graceful shutdown / SIGHUP reload.
fn close_all_listeners(slots: &[ListenerSlot]) {
    for slot in slots {
        if slot.is_tcp {
            tcp_close_listener(slot.handle);
        } else {
            udp_close(slot.handle);
        }
    }
}

/// Poll TCP listeners for incoming connections (non-blocking).
/// Returns a list of (slot_index, connection_handle) pairs.
fn poll_tcp_listeners(slots: &[ListenerSlot]) -> Vec<(usize, u64)> {
    let mut accepted = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        if !slot.is_tcp || slot.wait_busy {
            continue;
        }
        match tcp_accept_nonblock(slot.handle) {
            Ok(conn_handle) => {
                accepted.push((i, conn_handle));
            }
            Err(_) => {
                // No pending connection or error — continue polling.
            }
        }
    }
    accepted
}

/// Poll UDP sockets for incoming datagrams (non-blocking).
/// Returns a list of (slot_index, data, src_ip, src_port).
fn poll_udp_sockets(slots: &[ListenerSlot]) -> Vec<(usize, Vec<u8>, u32, u16)> {
    let mut received = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        if slot.is_tcp || slot.wait_busy {
            continue;
        }
        if !udp_rx_ready(slot.handle) {
            continue;
        }
        let mut buf = [0u8; 65535];
        match udp_recv(slot.handle, &mut buf) {
            Ok((n, src_ip, src_port)) => {
                received.push((i, buf[..n].to_vec(), src_ip, src_port));
            }
            Err(_) => {}
        }
    }
    received
}

/// Handle an incoming TCP connection: check rate limits, then either dispatch
/// to a built-in handler or spawn an external program.
fn handle_tcp_connection(
    slot_idx: usize,
    conn_handle: u64,
    slots: &mut [ListenerSlot],
    services: &[ServiceEntry],
    tracker: &mut ConnectionTracker,
    connections: &mut Vec<ActiveConnection>,
    logger: &Logger,
) {
    let slot = &slots[slot_idx];
    let svc = &services[slot.service_idx];
    let now_ms = clock_monotonic_ms();

    // Get peer address for rate limiting.
    let (src_ip, _src_port) = match tcp_peer_addr(conn_handle) {
        Ok(addr) => addr,
        Err(e) => {
            logger.log(LogLevel::Warning, &format!("cannot get peer addr: {e}"));
            tcp_close(conn_handle);
            return;
        }
    };

    // Check rate limits.
    if let Err(e) = tracker.check_allowed(src_ip, svc.port, svc.max_rate, svc.max_per_source, now_ms) {
        logger.log(LogLevel::Warning, &format!("{e}"));
        tcp_close(conn_handle);
        return;
    }

    tracker.record(src_ip, svc.port, now_ms);

    logger.log(
        LogLevel::Debug,
        &format!(
            "accepted connection from {} on {} (port {})",
            format_ip(src_ip),
            svc.service_name,
            svc.port
        ),
    );

    if let Some(builtin) = svc.builtin {
        // Built-in service: handle inline (synchronously for simplicity in
        // this single-threaded daemon; real inetd would fork).
        handle_tcp_builtin(builtin, conn_handle);
        tcp_shutdown(conn_handle, 2);
        tcp_close(conn_handle);
        tracker.release(src_ip, svc.port);
    } else {
        // External service: spawn the program.
        match process_spawn(&svc.program) {
            Ok(child_pid) => {
                logger.log(
                    LogLevel::Debug,
                    &format!("spawned {} (pid {child_pid}) for {}", svc.program, svc.service_name),
                );
                if svc.wait_mode == WaitMode::Wait {
                    // Mark the slot as busy until the child exits.
                    slots[slot_idx].wait_busy = true;
                }
                connections.push(ActiveConnection {
                    handle: conn_handle,
                    src_ip,
                    service_port: svc.port,
                    child_pid,
                });
            }
            Err(e) => {
                logger.log(LogLevel::Error, &format!("spawn failed for {}: {e}", svc.service_name));
                tcp_close(conn_handle);
                tracker.release(src_ip, svc.port);
            }
        }
    }
}

/// Handle a UDP datagram: check rate limits, then dispatch to built-in or
/// spawn an external handler.
fn handle_udp_datagram(
    slot_idx: usize,
    data: &[u8],
    src_ip: u32,
    src_port: u16,
    slots: &mut [ListenerSlot],
    services: &[ServiceEntry],
    tracker: &mut ConnectionTracker,
    logger: &Logger,
) {
    let slot = &slots[slot_idx];
    let svc = &services[slot.service_idx];
    let now_ms = clock_monotonic_ms();

    // Check rate limits.
    if let Err(e) = tracker.check_allowed(src_ip, svc.port, svc.max_rate, svc.max_per_source, now_ms) {
        logger.log(LogLevel::Warning, &format!("{e}"));
        return;
    }

    tracker.record(src_ip, svc.port, now_ms);

    logger.log(
        LogLevel::Debug,
        &format!(
            "received UDP datagram from {}:{src_port} on {} (port {})",
            format_ip(src_ip),
            svc.service_name,
            svc.port
        ),
    );

    if let Some(builtin) = svc.builtin {
        handle_udp_builtin(builtin, slot.handle, data, src_ip, src_port);
        // UDP is connectionless — release immediately.
        tracker.release(src_ip, svc.port);
    } else {
        // For external UDP services in `wait` mode, the daemon stops
        // listening on this socket until the child exits. The child is
        // expected to read from the socket directly.
        match process_spawn(&svc.program) {
            Ok(child_pid) => {
                logger.log(
                    LogLevel::Debug,
                    &format!("spawned {} (pid {child_pid}) for UDP {}", svc.program, svc.service_name),
                );
                if svc.wait_mode == WaitMode::Wait {
                    slots[slot_idx].wait_busy = true;
                }
            }
            Err(e) => {
                logger.log(
                    LogLevel::Error,
                    &format!("spawn failed for UDP {}: {e}", svc.service_name),
                );
            }
        }
        tracker.release(src_ip, svc.port);
    }
}

/// Try to reload the configuration file. Returns the new service list on
/// success, or an error string.
#[allow(dead_code)] // Called on SIGHUP signal for live reconfiguration.
fn reload_config(config_path: &str, rate_limit: u32, max_per_source: u32) -> Result<Vec<ServiceEntry>, String> {
    let content = fs_read_file(config_path)
        .map_err(|e| format!("reload: {e}"))?;
    let text = String::from_utf8_lossy(&content);
    parse_config(&text, rate_limit, max_per_source)
}

/// Write the PID file.
fn write_pid_file(pid: u64) {
    let pid_str = format!("{pid}\n");
    let _ = fs_write_file("/var/run/inetd.pid", pid_str.as_bytes());
}

/// The main event loop: poll all listeners and sockets, handle connections,
/// and perform periodic housekeeping.
fn run_daemon(cfg: &Config) -> i32 {
    let logger = Logger::new(cfg.debug);

    logger.log(LogLevel::Info, &format!(
        "inetd starting with {} services from {}",
        cfg.services.len(),
        cfg.config_path
    ));

    // Write PID file.
    write_pid_file(get_pid());

    // Bind all configured services.
    let mut slots = bind_services(&cfg.services, &logger);
    if slots.is_empty() {
        logger.log(LogLevel::Error, "no services bound, exiting");
        return 1;
    }

    let mut tracker = ConnectionTracker::new();
    let mut connections: Vec<ActiveConnection> = Vec::new();
    let mut gc_counter: u64 = 0;

    logger.log(LogLevel::Info, &format!("{} listeners active", slots.len()));

    // Main poll loop.
    loop {
        // Poll TCP listeners.
        let accepted = poll_tcp_listeners(&slots);
        for (slot_idx, conn_handle) in accepted {
            handle_tcp_connection(
                slot_idx,
                conn_handle,
                &mut slots,
                &cfg.services,
                &mut tracker,
                &mut connections,
                &logger,
            );
        }

        // Poll UDP sockets.
        let datagrams = poll_udp_sockets(&slots);
        for (slot_idx, data, src_ip, src_port) in datagrams {
            handle_udp_datagram(
                slot_idx,
                &data,
                src_ip,
                src_port,
                &mut slots,
                &cfg.services,
                &mut tracker,
                &logger,
            );
        }

        // Periodic housekeeping: garbage-collect the connection tracker
        // every ~100 iterations.
        gc_counter = gc_counter.wrapping_add(1);
        if gc_counter % 100 == 0 {
            let now_ms = clock_monotonic_ms();
            tracker.garbage_collect(now_ms);
        }

        // Sleep briefly to avoid busy-spinning. 10 ms gives reasonable
        // latency while keeping CPU usage low.
        sleep_ms(10);
    }
}

/// Run inetd. Returns the exit code.
fn run(args: &[String]) -> i32 {
    let cfg = match parse_args(args) {
        Ok(c) => c,
        Err(e) => {
            if e.is_empty() {
                print_help();
                return 0;
            }
            eprintln!("inetd: {e}");
            print_help();
            return 1;
        }
    };

    // Read and parse the configuration file.
    let content = match fs_read_file(&cfg.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("inetd: {e}");
            return 1;
        }
    };
    let text = String::from_utf8_lossy(&content);
    let services = match parse_config(&text, cfg.rate_limit, cfg.max_per_source) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("inetd: configuration error: {e}");
            return 1;
        }
    };

    if services.is_empty() {
        eprintln!("inetd: no services defined in {}", cfg.config_path);
        return 1;
    }

    let cfg = Config {
        services,
        ..cfg
    };

    run_daemon(&cfg)
}

// ============================================================================
// Entry point
// ============================================================================

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args: Vec<String> = env::args().collect();
    run(&args)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Config parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_echo_tcp_service() {
        let line = "echo stream tcp nowait root internal";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.service_name, "echo");
        assert_eq!(entry.port, 7);
        assert_eq!(entry.socket_type, SocketType::Stream);
        assert_eq!(entry.protocol, Protocol::Tcp);
        assert_eq!(entry.wait_mode, WaitMode::Nowait);
        assert_eq!(entry.user, "root");
        assert!(entry.is_internal());
        assert_eq!(entry.builtin, Some(BuiltinService::Echo));
    }

    #[test]
    fn test_parse_echo_udp_service() {
        let line = "echo dgram udp wait root internal";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.service_name, "echo");
        assert_eq!(entry.socket_type, SocketType::Dgram);
        assert_eq!(entry.protocol, Protocol::Udp);
        assert_eq!(entry.wait_mode, WaitMode::Wait);
        assert!(entry.is_internal());
    }

    #[test]
    fn test_parse_discard_service() {
        let line = "discard stream tcp nowait root internal";
        let entry = parse_config_line(line, 100, 32).unwrap();
        assert_eq!(entry.service_name, "discard");
        assert_eq!(entry.port, 9);
        assert_eq!(entry.builtin, Some(BuiltinService::Discard));
        assert_eq!(entry.max_rate, 100);
        assert_eq!(entry.max_per_source, 32);
    }

    #[test]
    fn test_parse_daytime_service() {
        let line = "daytime stream tcp nowait root internal";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.service_name, "daytime");
        assert_eq!(entry.port, 13);
        assert_eq!(entry.builtin, Some(BuiltinService::Daytime));
    }

    #[test]
    fn test_parse_chargen_service() {
        let line = "chargen stream tcp nowait root internal";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.service_name, "chargen");
        assert_eq!(entry.port, 19);
        assert_eq!(entry.builtin, Some(BuiltinService::Chargen));
    }

    #[test]
    fn test_parse_time_service() {
        let line = "time stream tcp nowait root internal";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.service_name, "time");
        assert_eq!(entry.port, 37);
        assert_eq!(entry.builtin, Some(BuiltinService::Time));
    }

    #[test]
    fn test_parse_external_service() {
        let line = "ssh stream tcp nowait root /usr/sbin/sshd sshd -i";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.service_name, "ssh");
        assert_eq!(entry.port, 22);
        assert!(!entry.is_internal());
        assert_eq!(entry.program, "/usr/sbin/sshd");
        assert_eq!(entry.args, vec!["sshd", "-i"]);
    }

    #[test]
    fn test_parse_nowait_with_max_connections() {
        let line = "ssh stream tcp nowait.32 root /usr/sbin/sshd sshd -i";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.wait_mode, WaitMode::Nowait);
        assert_eq!(entry.max_per_source, 32);
    }

    #[test]
    fn test_parse_tcp6_protocol() {
        let line = "ssh stream tcp6 nowait root /usr/sbin/sshd sshd -i";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.protocol, Protocol::Tcp6);
        assert!(entry.protocol.is_v6());
        assert!(entry.protocol.is_tcp());
    }

    #[test]
    fn test_parse_udp6_protocol() {
        let line = "echo dgram udp6 wait root internal";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.protocol, Protocol::Udp6);
        assert!(entry.protocol.is_v6());
        assert!(!entry.protocol.is_tcp());
    }

    #[test]
    fn test_parse_error_too_few_fields() {
        let line = "echo stream tcp";
        assert!(parse_config_line(line, 256, 64).is_err());
    }

    #[test]
    fn test_parse_error_unknown_socket_type() {
        let line = "echo raw tcp nowait root internal";
        let result = parse_config_line(line, 256, 64);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown socket type"));
    }

    #[test]
    fn test_parse_error_unknown_protocol() {
        let line = "echo stream sctp nowait root internal";
        let result = parse_config_line(line, 256, 64);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown protocol"));
    }

    #[test]
    fn test_parse_error_stream_with_udp() {
        let line = "echo stream udp nowait root internal";
        let result = parse_config_line(line, 256, 64);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("stream socket with non-TCP"));
    }

    #[test]
    fn test_parse_error_dgram_with_tcp() {
        let line = "echo dgram tcp nowait root internal";
        let result = parse_config_line(line, 256, 64);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("dgram socket with TCP"));
    }

    #[test]
    fn test_parse_error_unknown_wait_mode() {
        let line = "echo stream tcp maybe root internal";
        let result = parse_config_line(line, 256, 64);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown wait mode"));
    }

    #[test]
    fn test_parse_error_unknown_service() {
        let line = "xyzzy stream tcp nowait root /usr/bin/xyzzy xyzzy";
        let result = parse_config_line(line, 256, 64);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown service name"));
    }

    #[test]
    fn test_parse_error_no_builtin_for_internal() {
        let line = "ssh stream tcp nowait root internal";
        let result = parse_config_line(line, 256, 64);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no built-in handler"));
    }

    #[test]
    fn test_parse_full_config() {
        let conf = "\
# Comment line
echo      stream  tcp  nowait  root  internal
echo      dgram   udp  wait    root  internal
discard   stream  tcp  nowait  root  internal
daytime   stream  tcp  nowait  root  internal
chargen   stream  tcp  nowait  root  internal
time      stream  tcp  nowait  root  internal

# External service
ssh       stream  tcp  nowait  root  /usr/sbin/sshd  sshd -i
";
        let services = parse_config(conf, 256, 64).unwrap();
        assert_eq!(services.len(), 7);
        assert_eq!(services[0].service_name, "echo");
        assert_eq!(services[6].service_name, "ssh");
    }

    #[test]
    fn test_parse_config_with_blank_lines() {
        let conf = "\n\n# Just comments\n\n";
        let services = parse_config(conf, 256, 64).unwrap();
        assert!(services.is_empty());
    }

    #[test]
    fn test_parse_config_error_propagation() {
        let conf = "echo stream tcp nowait root internal\nbogus";
        let result = parse_config(conf, 256, 64);
        assert!(result.is_err());
        assert!(result.unwrap_err().starts_with("line 2:"));
    }

    // -----------------------------------------------------------------------
    // Service port lookup tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_service_to_port_well_known() {
        assert_eq!(service_to_port("echo"), Some(7));
        assert_eq!(service_to_port("discard"), Some(9));
        assert_eq!(service_to_port("daytime"), Some(13));
        assert_eq!(service_to_port("chargen"), Some(19));
        assert_eq!(service_to_port("ftp"), Some(21));
        assert_eq!(service_to_port("ssh"), Some(22));
        assert_eq!(service_to_port("telnet"), Some(23));
        assert_eq!(service_to_port("smtp"), Some(25));
        assert_eq!(service_to_port("time"), Some(37));
        assert_eq!(service_to_port("http"), Some(80));
        assert_eq!(service_to_port("https"), Some(443));
    }

    #[test]
    fn test_service_to_port_numeric() {
        assert_eq!(service_to_port("8080"), Some(8080));
        assert_eq!(service_to_port("0"), Some(0));
        assert_eq!(service_to_port("65535"), Some(65535));
    }

    #[test]
    fn test_service_to_port_unknown() {
        assert_eq!(service_to_port("nonexistent"), None);
        assert_eq!(service_to_port(""), None);
    }

    #[test]
    fn test_service_to_port_aliases() {
        assert_eq!(service_to_port("sink"), Some(9));
        assert_eq!(service_to_port("null"), Some(9));
        assert_eq!(service_to_port("www"), Some(80));
        assert_eq!(service_to_port("timserver"), Some(37));
        assert_eq!(service_to_port("ttytst"), Some(19));
    }

    // -----------------------------------------------------------------------
    // Builtin resolution tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_builtin_known() {
        assert_eq!(resolve_builtin("echo"), Some(BuiltinService::Echo));
        assert_eq!(resolve_builtin("discard"), Some(BuiltinService::Discard));
        assert_eq!(resolve_builtin("daytime"), Some(BuiltinService::Daytime));
        assert_eq!(resolve_builtin("chargen"), Some(BuiltinService::Chargen));
        assert_eq!(resolve_builtin("time"), Some(BuiltinService::Time));
    }

    #[test]
    fn test_resolve_builtin_aliases() {
        assert_eq!(resolve_builtin("sink"), Some(BuiltinService::Discard));
        assert_eq!(resolve_builtin("null"), Some(BuiltinService::Discard));
        assert_eq!(resolve_builtin("ttytst"), Some(BuiltinService::Chargen));
        assert_eq!(resolve_builtin("timserver"), Some(BuiltinService::Time));
    }

    #[test]
    fn test_resolve_builtin_unknown() {
        assert_eq!(resolve_builtin("ssh"), None);
        assert_eq!(resolve_builtin("http"), None);
        assert_eq!(resolve_builtin(""), None);
    }

    // -----------------------------------------------------------------------
    // IP formatting/parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_ip() {
        assert_eq!(format_ip(0x7F000001), "127.0.0.1");
        assert_eq!(format_ip(0xC0A80001), "192.168.0.1");
        assert_eq!(format_ip(0), "0.0.0.0");
        assert_eq!(format_ip(0xFFFFFFFF), "255.255.255.255");
    }

    #[test]
    fn test_parse_ipv4() {
        assert_eq!(parse_ipv4("127.0.0.1"), Some(0x7F000001));
        assert_eq!(parse_ipv4("192.168.0.1"), Some(0xC0A80001));
        assert_eq!(parse_ipv4("0.0.0.0"), Some(0));
        assert_eq!(parse_ipv4("255.255.255.255"), Some(0xFFFFFFFF));
    }

    #[test]
    fn test_parse_ipv4_invalid() {
        assert_eq!(parse_ipv4(""), None);
        assert_eq!(parse_ipv4("1.2.3"), None);
        assert_eq!(parse_ipv4("1.2.3.4.5"), None);
        assert_eq!(parse_ipv4("abc.def.ghi.jkl"), None);
        assert_eq!(parse_ipv4("256.0.0.0"), None);
    }

    #[test]
    fn test_format_parse_roundtrip() {
        let ips = [0u32, 0x7F000001, 0xC0A80001, 0x0A000001, 0xFFFFFFFF];
        for ip in ips {
            let formatted = format_ip(ip);
            let parsed = parse_ipv4(&formatted).unwrap();
            assert_eq!(parsed, ip, "roundtrip failed for {ip:#X}");
        }
    }

    // -----------------------------------------------------------------------
    // Broken time / daytime tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_unix_to_broken_epoch() {
        let bt = unix_to_broken(0);
        assert_eq!(bt.year, 1970);
        assert_eq!(bt.month, 1);
        assert_eq!(bt.day, 1);
        assert_eq!(bt.hour, 0);
        assert_eq!(bt.minute, 0);
        assert_eq!(bt.second, 0);
        assert_eq!(bt.weekday, 4); // Thursday
    }

    #[test]
    fn test_unix_to_broken_known_date() {
        // 2024-01-15 12:30:45 UTC = 1705321845
        let bt = unix_to_broken(1_705_321_845);
        assert_eq!(bt.year, 2024);
        assert_eq!(bt.month, 1);
        assert_eq!(bt.day, 15);
        assert_eq!(bt.hour, 12);
        assert_eq!(bt.minute, 30);
        assert_eq!(bt.second, 45);
        assert_eq!(bt.weekday, 1); // Monday
    }

    #[test]
    fn test_unix_to_broken_leap_year() {
        // 2024-02-29 00:00:00 UTC = 1709164800
        let bt = unix_to_broken(1_709_164_800);
        assert_eq!(bt.year, 2024);
        assert_eq!(bt.month, 2);
        assert_eq!(bt.day, 29);
    }

    #[test]
    fn test_unix_to_broken_end_of_year() {
        // 2023-12-31 23:59:59 UTC = 1704067199
        let bt = unix_to_broken(1_704_067_199);
        assert_eq!(bt.year, 2023);
        assert_eq!(bt.month, 12);
        assert_eq!(bt.day, 31);
        assert_eq!(bt.hour, 23);
        assert_eq!(bt.minute, 59);
        assert_eq!(bt.second, 59);
    }

    #[test]
    fn test_format_daytime_contains_newline() {
        let s = format_daytime(0);
        assert!(s.ends_with("\r\n"));
        assert!(s.contains("1970"));
        assert!(s.contains("Thu"));
        assert!(s.contains("Jan"));
    }

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(2400));
    }

    // -----------------------------------------------------------------------
    // Chargen tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_chargen_line_length() {
        let line = chargen_line(0);
        assert_eq!(line.len(), 74); // 72 chars + CR + LF
        assert_eq!(line[72], b'\r');
        assert_eq!(line[73], b'\n');
    }

    #[test]
    fn test_chargen_line_printable() {
        for offset in 0..95u32 {
            let line = chargen_line(offset);
            for &ch in &line[..72] {
                assert!(
                    ch >= 32 && ch <= 126,
                    "non-printable char {ch} at offset {offset}"
                );
            }
        }
    }

    #[test]
    fn test_chargen_line_rotation() {
        let line0 = chargen_line(0);
        let line1 = chargen_line(1);
        // The second line should start with the character after the first
        // line's first character.
        assert_eq!(line1[0], line0[1]);
    }

    #[test]
    fn test_chargen_line_wraps() {
        // The character at position 0 with offset 0 is ASCII 32 (space).
        let line = chargen_line(0);
        assert_eq!(line[0], 32); // space
    }

    // -----------------------------------------------------------------------
    // Rate limiting / Connection tracker tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_source_tracker_new() {
        let tracker = SourceTracker::new();
        assert_eq!(tracker.active_count, 0);
        assert!(tracker.recent_timestamps.is_empty());
    }

    #[test]
    fn test_source_tracker_record_and_release() {
        let mut tracker = SourceTracker::new();
        tracker.record_connection(1000);
        assert_eq!(tracker.active_count, 1);
        assert_eq!(tracker.rate(), 1);

        tracker.record_connection(2000);
        assert_eq!(tracker.active_count, 2);
        assert_eq!(tracker.rate(), 2);

        tracker.release_connection();
        assert_eq!(tracker.active_count, 1);
    }

    #[test]
    fn test_source_tracker_prune_old_timestamps() {
        let mut tracker = SourceTracker::new();
        tracker.record_connection(1000);   // old
        tracker.record_connection(50_000); // old
        tracker.record_connection(70_000); // recent
        tracker.prune(70_000);
        // Cutoff is 70000 - 60000 = 10000, so the first timestamp (1000) is pruned.
        assert_eq!(tracker.rate(), 2);

        tracker.prune(130_001);
        // Now all timestamps are older than 60s from 130001.
        assert_eq!(tracker.rate(), 0);
    }

    #[test]
    fn test_source_tracker_release_saturates_at_zero() {
        let mut tracker = SourceTracker::new();
        tracker.release_connection();
        assert_eq!(tracker.active_count, 0);
    }

    #[test]
    fn test_connection_tracker_new() {
        let tracker = ConnectionTracker::new();
        assert_eq!(tracker.tracked_sources(), 0);
    }

    #[test]
    fn test_connection_tracker_record_and_check() {
        let mut tracker = ConnectionTracker::new();
        let ip = 0x7F000001; // 127.0.0.1
        let port = 22;
        let now = 100_000u64;

        tracker.record(ip, port, now);
        assert_eq!(tracker.active_for(ip, port), 1);
        assert_eq!(tracker.rate_for(ip, port, now), 1);
    }

    #[test]
    fn test_connection_tracker_rate_limit_exceeded() {
        let mut tracker = ConnectionTracker::new();
        let ip = 0x7F000001;
        let port = 80;
        let now = 100_000u64;

        // Record 5 connections.
        for i in 0..5u64 {
            tracker.record(ip, port, now + i);
        }

        // With max_rate=5, the 6th should be rejected.
        let result = tracker.check_allowed(ip, port, 5, 100, now + 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_connection_tracker_max_per_source_exceeded() {
        let mut tracker = ConnectionTracker::new();
        let ip = 0x7F000001;
        let port = 80;
        let now = 100_000u64;

        // Record 3 active connections.
        for _ in 0..3 {
            tracker.record(ip, port, now);
        }

        // With max_per_source=3, the next should be rejected.
        let result = tracker.check_allowed(ip, port, 1000, 3, now);
        assert!(result.is_err());
    }

    #[test]
    fn test_connection_tracker_release() {
        let mut tracker = ConnectionTracker::new();
        let ip = 0x7F000001;
        let port = 80;
        let now = 100_000u64;

        tracker.record(ip, port, now);
        assert_eq!(tracker.active_for(ip, port), 1);
        tracker.release(ip, port);
        assert_eq!(tracker.active_for(ip, port), 0);
    }

    #[test]
    fn test_connection_tracker_garbage_collect() {
        let mut tracker = ConnectionTracker::new();
        let ip = 0x7F000001;
        let port = 80;

        tracker.record(ip, port, 1000);
        tracker.release(ip, port);
        assert_eq!(tracker.tracked_sources(), 1);

        // After GC with a time far enough in the future, the entry is removed.
        tracker.garbage_collect(100_000);
        assert_eq!(tracker.tracked_sources(), 0);
    }

    #[test]
    fn test_connection_tracker_different_ports_independent() {
        let mut tracker = ConnectionTracker::new();
        let ip = 0x7F000001;
        let now = 100_000u64;

        tracker.record(ip, 22, now);
        tracker.record(ip, 80, now);

        assert_eq!(tracker.active_for(ip, 22), 1);
        assert_eq!(tracker.active_for(ip, 80), 1);

        // Rate limit on port 22 should not affect port 80.
        let result = tracker.check_allowed(ip, 80, 1000, 100, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_connection_tracker_different_ips_independent() {
        let mut tracker = ConnectionTracker::new();
        let ip1 = 0x7F000001;
        let ip2 = 0xC0A80001;
        let port = 80;
        let now = 100_000u64;

        // Fill up ip1.
        for _ in 0..5 {
            tracker.record(ip1, port, now);
        }

        // ip2 should still be allowed even though ip1 is full.
        let result = tracker.check_allowed(ip2, port, 1000, 5, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_connection_tracker_rate_limit_zero_means_unlimited() {
        let mut tracker = ConnectionTracker::new();
        let ip = 0x7F000001;
        let port = 80;
        let now = 100_000u64;

        for _ in 0..1000 {
            tracker.record(ip, port, now);
        }

        // rate_limit=0 means no rate limiting.
        let result = tracker.check_allowed(ip, port, 0, 0, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_connection_tracker_rate_expires_after_window() {
        let mut tracker = ConnectionTracker::new();
        let ip = 0x7F000001;
        let port = 80;
        let now = 100_000u64;

        // Record 10 connections at time=now.
        for _ in 0..10 {
            tracker.record(ip, port, now);
        }
        // Release all active so only rate timestamps remain.
        for _ in 0..10 {
            tracker.release(ip, port);
        }

        // At now+61s, the rate window has moved past all those connections.
        let later = now + 61_000;
        let rate = tracker.rate_for(ip, port, later);
        assert_eq!(rate, 0);

        // And we can accept new connections again.
        let result = tracker.check_allowed(ip, port, 10, 100, later);
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // Command-line argument parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_args_defaults() {
        let args = vec!["inetd".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.config_path, "/etc/inetd.conf");
        assert!(!cfg.debug);
        assert_eq!(cfg.rate_limit, 256);
        assert_eq!(cfg.max_per_source, 64);
    }

    #[test]
    fn test_parse_args_custom_config() {
        let args = vec!["inetd".to_string(), "-f".to_string(), "/tmp/test.conf".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.config_path, "/tmp/test.conf");
    }

    #[test]
    fn test_parse_args_debug() {
        let args = vec!["inetd".to_string(), "-d".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.debug);
    }

    #[test]
    fn test_parse_args_rate_limit() {
        let args = vec!["inetd".to_string(), "-R".to_string(), "100".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.rate_limit, 100);
    }

    #[test]
    fn test_parse_args_max_per_source() {
        let args = vec!["inetd".to_string(), "-c".to_string(), "32".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.max_per_source, 32);
    }

    #[test]
    fn test_parse_args_help() {
        let args = vec!["inetd".to_string(), "-h".to_string()];
        let result = parse_args(&args);
        // Help triggers an empty error string.
        assert!(result.is_err());
        assert!(result.unwrap_err().is_empty());
    }

    #[test]
    fn test_parse_args_unknown_option() {
        let args = vec!["inetd".to_string(), "-Z".to_string()];
        let result = parse_args(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown option"));
    }

    #[test]
    fn test_parse_args_positional_config() {
        let args = vec!["inetd".to_string(), "/tmp/my.conf".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.config_path, "/tmp/my.conf");
    }

    #[test]
    fn test_parse_args_missing_f_argument() {
        let args = vec!["inetd".to_string(), "-f".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_invalid_rate() {
        let args = vec!["inetd".to_string(), "-R".to_string(), "abc".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_all_options() {
        let args = vec![
            "inetd".to_string(),
            "-d".to_string(),
            "-f".to_string(),
            "/tmp/test.conf".to_string(),
            "-R".to_string(),
            "100".to_string(),
            "-c".to_string(),
            "16".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.debug);
        assert_eq!(cfg.config_path, "/tmp/test.conf");
        assert_eq!(cfg.rate_limit, 100);
        assert_eq!(cfg.max_per_source, 16);
    }

    // -----------------------------------------------------------------------
    // Protocol / SocketType / WaitMode display and property tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_protocol_display() {
        assert_eq!(format!("{}", Protocol::Tcp), "tcp");
        assert_eq!(format!("{}", Protocol::Udp), "udp");
        assert_eq!(format!("{}", Protocol::Tcp6), "tcp6");
        assert_eq!(format!("{}", Protocol::Udp6), "udp6");
    }

    #[test]
    fn test_protocol_is_v6() {
        assert!(!Protocol::Tcp.is_v6());
        assert!(!Protocol::Udp.is_v6());
        assert!(Protocol::Tcp6.is_v6());
        assert!(Protocol::Udp6.is_v6());
    }

    #[test]
    fn test_protocol_is_tcp() {
        assert!(Protocol::Tcp.is_tcp());
        assert!(!Protocol::Udp.is_tcp());
        assert!(Protocol::Tcp6.is_tcp());
        assert!(!Protocol::Udp6.is_tcp());
    }

    #[test]
    fn test_socket_type_display() {
        assert_eq!(format!("{}", SocketType::Stream), "stream");
        assert_eq!(format!("{}", SocketType::Dgram), "dgram");
    }

    #[test]
    fn test_wait_mode_display() {
        assert_eq!(format!("{}", WaitMode::Wait), "wait");
        assert_eq!(format!("{}", WaitMode::Nowait), "nowait");
    }

    #[test]
    fn test_builtin_service_display() {
        assert_eq!(format!("{}", BuiltinService::Echo), "echo");
        assert_eq!(format!("{}", BuiltinService::Discard), "discard");
        assert_eq!(format!("{}", BuiltinService::Daytime), "daytime");
        assert_eq!(format!("{}", BuiltinService::Chargen), "chargen");
        assert_eq!(format!("{}", BuiltinService::Time), "time");
    }

    // -----------------------------------------------------------------------
    // ServiceEntry display and property tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_service_entry_display_internal() {
        let entry = parse_config_line("echo stream tcp nowait root internal", 256, 64).unwrap();
        let display = format!("{entry}");
        assert!(display.contains("echo"));
        assert!(display.contains("internal"));
    }

    #[test]
    fn test_service_entry_display_external() {
        let entry = parse_config_line("ssh stream tcp nowait root /usr/sbin/sshd sshd -i", 256, 64).unwrap();
        let display = format!("{entry}");
        assert!(display.contains("ssh"));
        assert!(display.contains("/usr/sbin/sshd"));
    }

    #[test]
    fn test_service_entry_is_internal() {
        let internal = parse_config_line("echo stream tcp nowait root internal", 256, 64).unwrap();
        let external = parse_config_line("ssh stream tcp nowait root /usr/sbin/sshd sshd", 256, 64).unwrap();
        assert!(internal.is_internal());
        assert!(!external.is_internal());
    }

    // -----------------------------------------------------------------------
    // Error display tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_error_display() {
        let e = InetdError::Config("bad config".into());
        assert_eq!(format!("{e}"), "config: bad config");

        let e = InetdError::Network("conn refused".into());
        assert_eq!(format!("{e}"), "network: conn refused");

        let e = InetdError::Spawn("no such file".into());
        assert_eq!(format!("{e}"), "spawn: no such file");

        let e = InetdError::RateLimit("too many".into());
        assert_eq!(format!("{e}"), "rate limit: too many");
    }

    // -----------------------------------------------------------------------
    // LogLevel tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warning);
        assert!(LogLevel::Warning < LogLevel::Error);
    }

    #[test]
    fn test_log_level_display() {
        assert_eq!(format!("{}", LogLevel::Debug), "DEBUG");
        assert_eq!(format!("{}", LogLevel::Info), "INFO");
        assert_eq!(format!("{}", LogLevel::Warning), "WARNING");
        assert_eq!(format!("{}", LogLevel::Error), "ERROR");
    }

    // -----------------------------------------------------------------------
    // Numeric service port tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_numeric_service_as_port() {
        let line = "8080 stream tcp nowait root /usr/bin/httpd httpd";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.port, 8080);
        assert_eq!(entry.service_name, "8080");
    }

    // -----------------------------------------------------------------------
    // External service argument parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_external_service_no_extra_args() {
        let line = "ftp stream tcp nowait root /usr/sbin/ftpd";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.program, "/usr/sbin/ftpd");
        // When no extra args, the program name is used as argv[0].
        assert_eq!(entry.args, vec!["/usr/sbin/ftpd"]);
    }

    #[test]
    fn test_external_service_multiple_args() {
        let line = "ssh stream tcp nowait root /usr/sbin/sshd sshd -i -p 2222";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.args, vec!["sshd", "-i", "-p", "2222"]);
    }

    // -----------------------------------------------------------------------
    // RFC 868 time protocol tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_rfc868_epoch_offset() {
        // The offset between Unix epoch (1970) and RFC 868 epoch (1900) is
        // 70 years worth of seconds.
        const RFC868_EPOCH_OFFSET: u64 = 2_208_988_800;
        // At Unix time 0, the RFC 868 time should be the offset.
        let rfc_time = (0u64).saturating_add(RFC868_EPOCH_OFFSET);
        assert_eq!(rfc_time, 2_208_988_800);
    }

    // -----------------------------------------------------------------------
    // Config struct default tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_config_defaults() {
        let cfg = Config::new();
        assert_eq!(cfg.config_path, "/etc/inetd.conf");
        assert!(!cfg.debug);
        assert_eq!(cfg.rate_limit, 256);
        assert_eq!(cfg.max_per_source, 64);
        assert!(cfg.services.is_empty());
    }

    // -----------------------------------------------------------------------
    // Logger tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_logger_new() {
        let logger = Logger::new(false);
        assert!(!logger.debug);
        assert!(logger.log_to_file);
        assert_eq!(logger.log_path, "/var/log/inetd.log");
    }

    #[test]
    fn test_logger_debug_mode() {
        let logger = Logger::new(true);
        assert!(logger.debug);
    }

    // -----------------------------------------------------------------------
    // Wait mode with dotted suffix parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_wait_mode_nowait_dotted_valid() {
        let line = "http stream tcp nowait.128 root /usr/sbin/httpd httpd";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.wait_mode, WaitMode::Nowait);
        assert_eq!(entry.max_per_source, 128);
    }

    #[test]
    fn test_wait_mode_nowait_dotted_invalid_falls_back() {
        // If the dotted value is not a valid u32, the global default is used.
        let line = "http stream tcp nowait.abc root /usr/sbin/httpd httpd";
        let entry = parse_config_line(line, 256, 42).unwrap();
        assert_eq!(entry.wait_mode, WaitMode::Nowait);
        assert_eq!(entry.max_per_source, 42);
    }

    #[test]
    fn test_wait_mode_wait_with_dot_ignored() {
        let line = "echo dgram udp wait.10 root internal";
        let entry = parse_config_line(line, 256, 64).unwrap();
        assert_eq!(entry.wait_mode, WaitMode::Wait);
        // For wait mode, per_source override is not parsed.
        assert_eq!(entry.max_per_source, 64);
    }
}
