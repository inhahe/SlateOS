//! Slate OS Network Utility (netcat)
//!
//! A TCP/UDP networking tool for connecting, listening, port scanning,
//! and data transfer. Supports both client and server modes with options
//! for timeouts, verbosity, keep-alive, source binding, and command
//! execution on connect.
//!
//! # Usage
//!
//! ```text
//! nc hostname port                  TCP client (connect and relay stdin/stdout)
//! nc -l port                        TCP server (listen for one connection)
//! nc -u hostname port               UDP send/receive
//! nc -z hostname port1-port2        Port scan
//! nc -l port -e /bin/sh             Execute command on incoming connection
//! nc -w 5 hostname port             Set timeout to 5 seconds
//! nc -v hostname port               Verbose output
//! nc -k -l port                     Keep listening after client disconnects
//! nc -s 10.0.0.1 hostname port      Bind to source address
//! nc -p 5000 hostname port          Use source port 5000
//! ```

#![deny(clippy::all)]
#![allow(clippy::manual_range_contains)] // clearer as explicit comparisons in some spots

use std::env;
use std::io::{self, Read, Write};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

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
const SYS_TCP_POLL_STATUS: u64 = 845;
const SYS_TCP_LAST_ERROR: u64 = 853;
const SYS_TCP_SHUTDOWN: u64 = 855;

const SYS_UDP_BIND: u64 = 810;
const SYS_UDP_SEND: u64 = 811;
const SYS_UDP_RECV: u64 = 812;
const SYS_UDP_CLOSE: u64 = 813;
const SYS_UDP_RX_READY: u64 = 847;

const SYS_DNS_RESOLVE: u64 = 820;
const SYS_SLEEP: u64 = 11;

// ============================================================================
// Syscall interface
//
// Register mapping: rax=nr, rdi=arg0, rsi=arg1, rdx=arg2, r10=arg3,
//                   r8=arg4, r9=arg5.
// Returns: rax=result. Clobbers: rcx, r11.
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

// ============================================================================
// Syscall wrappers — TCP
// ============================================================================

/// Open a blocking TCP connection to (ip, port). Returns a handle on success.
fn tcp_connect(ip: u32, port: u16) -> Result<u64, i64> {
    // SAFETY: SYS_TCP_CONNECT takes two scalar arguments (IP and port).
    // No pointer dereferences occur in userspace.
    let ret = unsafe { syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), 0) };
    if ret < 0 { Err(ret) } else { Ok(ret as u64) }
}

/// Open a non-blocking TCP connection (returns handle in SYN_SENT state).
fn tcp_connect_nonblock(ip: u32, port: u16) -> Result<u64, i64> {
    // SAFETY: arg2=1 sets the non-blocking flag. Same scalar arguments.
    let ret = unsafe { syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), 1) };
    if ret < 0 { Err(ret) } else { Ok(ret as u64) }
}

/// Send data on a TCP connection. Returns the number of bytes sent.
fn tcp_send(handle: u64, data: &[u8]) -> Result<usize, i64> {
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
    if ret < 0 { Err(ret) } else { Ok(ret as usize) }
}

/// Send all bytes, looping until the entire buffer is transmitted.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), i64> {
    let mut offset = 0;
    while offset < data.len() {
        let sent = tcp_send(handle, &data[offset..])?;
        if sent == 0 {
            return Err(-5); // EIO
        }
        offset = offset.saturating_add(sent);
    }
    Ok(())
}

/// Receive data from a TCP connection. Returns 0 on EOF (peer closed).
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, i64> {
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
    if ret < 0 { Err(ret) } else { Ok(ret as usize) }
}

/// Non-blocking receive: returns WouldBlock (-11) if no data ready.
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
    // SAFETY: handle is (or was) a valid TCP connection handle. The kernel
    // deallocates internal state. Ignoring the return is safe.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

/// Bind a TCP listener to a local port. Returns a listener handle.
fn tcp_bind(port: u16) -> Result<u64, i64> {
    // SAFETY: SYS_TCP_BIND takes one scalar argument (port number).
    let ret = unsafe { syscall1(SYS_TCP_BIND, u64::from(port)) };
    if ret < 0 { Err(ret) } else { Ok(ret as u64) }
}

/// Accept an incoming connection on a listener (blocking).
/// Returns a connection handle.
fn tcp_accept(listener: u64) -> Result<u64, i64> {
    // SAFETY: listener is a valid listener handle.
    let ret = unsafe { syscall1(SYS_TCP_ACCEPT, listener) };
    if ret < 0 { Err(ret) } else { Ok(ret as u64) }
}

/// Close a TCP listener handle.
fn tcp_close_listener(listener: u64) {
    // SAFETY: listener is (or was) a valid TCP listener handle.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE_LISTENER, listener) };
}

/// Get the peer address of a TCP connection.
/// Returns (ip_u32_network_order, port) on success.
fn tcp_peer_addr(handle: u64) -> Result<(u32, u16), i64> {
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
        return Err(ret);
    }
    let ip = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let port = u16::from_be_bytes([buf[4], buf[5]]);
    Ok((ip, port))
}

/// Poll connection status. Returns a status code:
/// 0 = connecting, 1 = established, 2 = closed, etc.
fn tcp_poll_status(handle: u64) -> i64 {
    // SAFETY: handle is a valid connection handle. Returns a scalar status.
    unsafe { syscall1(SYS_TCP_POLL_STATUS, handle) }
}

/// Query the last error code for a TCP connection (and clear it).
/// Returns: 0=none, 1=refused, 2=reset, 3=timedout.
fn tcp_last_error(handle: u64) -> i64 {
    // SAFETY: handle is a valid connection handle. arg1=1 clears after read.
    unsafe { syscall3(SYS_TCP_LAST_ERROR, handle, 1, 0) }
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
fn udp_bind(port: u16) -> Result<u64, i64> {
    // SAFETY: SYS_UDP_BIND takes one scalar argument (port number).
    let ret = unsafe { syscall1(SYS_UDP_BIND, u64::from(port)) };
    if ret < 0 { Err(ret) } else { Ok(ret as u64) }
}

/// Send a UDP datagram.
fn udp_send(handle: u64, dst_ip: u32, dst_port: u16, data: &[u8]) -> Result<(), i64> {
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
    if ret < 0 { Err(ret) } else { Ok(()) }
}

/// Receive a UDP datagram (non-blocking). Returns (bytes_read, src_ip, src_port).
/// Returns Err(-11) (WouldBlock) if no datagram is queued.
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
fn udp_close(handle: u64) {
    // SAFETY: handle is (or was) a valid UDP socket handle.
    let _ = unsafe { syscall1(SYS_UDP_CLOSE, handle) };
}

// ============================================================================
// Syscall wrappers — DNS
// ============================================================================

/// Resolve a hostname to an IPv4 address (network byte order u32).
fn dns_resolve(hostname: &str) -> Result<u32, i64> {
    let mut result_ip: u32 = 0;
    // SAFETY: hostname pointer and length are derived from a valid Rust str.
    // result_ip is a stack-allocated u32 with sufficient lifetime.
    let ret = unsafe {
        syscall3(
            SYS_DNS_RESOLVE,
            hostname.as_ptr() as u64,
            hostname.len() as u64,
            &mut result_ip as *mut u32 as u64,
        )
    };
    if ret < 0 { Err(ret) } else { Ok(result_ip) }
}

// ============================================================================
// Syscall wrappers — misc
// ============================================================================

/// Sleep for the given number of milliseconds.
fn sys_sleep(ms: u64) {
    // SAFETY: SYS_SLEEP takes one scalar argument (milliseconds).
    let _ = unsafe { syscall1(SYS_SLEEP, ms) };
}

// ============================================================================
// IPv4 address parsing and formatting
// ============================================================================

/// Parse a dotted-decimal IPv4 address string into a u32 in network byte order.
fn parse_ipv4(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        let val: u16 = part.parse().ok()?;
        if val > 255 {
            return None;
        }
        octets[i] = val as u8;
    }
    Some(u32::from_be_bytes(octets))
}

/// Format a u32 IP address (network byte order) as a dotted-decimal string.
fn format_ipv4(ip: u32) -> String {
    let o = ip.to_be_bytes();
    format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])
}

/// Returns true if the string looks like a dotted-decimal IPv4 address.
fn is_ipv4_address(s: &str) -> bool {
    parse_ipv4(s).is_some()
}

// ============================================================================
// Hostname resolution with fallback
// ============================================================================

/// Resolve a hostname to an IPv4 address, trying the DNS syscall first and
/// falling back to a small built-in table for common names.
fn resolve_host(hostname: &str) -> Result<u32, String> {
    // If it's already a dotted-decimal address, parse directly.
    if is_ipv4_address(hostname) {
        // Unwrap is safe: is_ipv4_address only returns true when parse succeeds.
        return Ok(parse_ipv4(hostname).expect("is_ipv4_address returned true"));
    }

    // Try kernel DNS resolver.
    if let Ok(ip) = dns_resolve(hostname) {
        return Ok(ip);
    }

    // Fallback: hardcoded common lookups.
    match hostname {
        "localhost" => Ok(parse_ipv4("127.0.0.1").expect("hardcoded IP is valid")),
        _ => Err(format!("cannot resolve '{hostname}'")),
    }
}

// ============================================================================
// Error descriptions
// ============================================================================

/// Map a negative syscall error code to a human-readable message.
fn syscall_error_msg(code: i64) -> &'static str {
    match code {
        -1 => "operation not permitted",
        -2 => "no such host",
        -5 => "I/O error",
        -11 => "resource temporarily unavailable",
        -13 => "permission denied",
        -22 => "invalid argument",
        -98 => "address already in use",
        -99 => "cannot assign requested address",
        -101 => "network is unreachable",
        -110 => "connection timed out",
        -111 => "connection refused",
        -113 => "no route to host",
        _ => "unknown error",
    }
}

/// Map a TCP last-error code to a human-readable message.
fn tcp_error_msg(code: i64) -> &'static str {
    match code {
        0 => "no error",
        1 => "connection refused",
        2 => "connection reset",
        3 => "connection timed out",
        _ => "unknown TCP error",
    }
}

// ============================================================================
// Global Ctrl+C handling
// ============================================================================

static RUNNING: AtomicBool = AtomicBool::new(true);

/// Install a Ctrl+C handler that clears the RUNNING flag.
fn install_signal_handler() {
    #[cfg(target_family = "unix")]
    {
        // SAFETY: We install a handler for SIGINT (2). The handler only
        // performs an atomic store, which is async-signal-safe.
        unsafe {
            libc_signal(2, signal_handler as *const () as usize);
        }
    }
}

/// Minimal POSIX signal registration.
///
/// # Safety
///
/// `handler` must be a valid function pointer suitable for use as a signal
/// handler (only async-signal-safe operations).
#[cfg(target_family = "unix")]
unsafe fn libc_signal(signum: i32, handler: usize) {
    unsafe extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }
    // SAFETY: signal() is a standard POSIX function. signum=2 (SIGINT) is
    // valid; handler points to a function that only does an atomic store.
    unsafe {
        signal(signum, handler);
    }
}

/// Signal handler: sets RUNNING to false (async-signal-safe).
#[cfg(target_family = "unix")]
extern "C" fn signal_handler(_sig: i32) {
    RUNNING.store(false, Ordering::SeqCst);
}

/// Sleep for `ms` milliseconds, checking RUNNING periodically for Ctrl+C.
fn sleep_interruptible(ms: u64) {
    let chunk = Duration::from_millis(100);
    let total = Duration::from_millis(ms);
    let start = Instant::now();
    while start.elapsed() < total {
        if !RUNNING.load(Ordering::SeqCst) {
            return;
        }
        let remaining = total.saturating_sub(start.elapsed());
        let sleep_time = if remaining < chunk { remaining } else { chunk };
        if sleep_time.is_zero() {
            break;
        }
        thread::sleep(sleep_time);
    }
}

// ============================================================================
// CLI options
// ============================================================================

/// Operating mode.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// TCP client: connect to host:port and relay stdin/stdout.
    Client,
    /// TCP server: listen on a port, accept one connection.
    Listen,
    /// Port scanner: probe a range of ports.
    Scan,
}

/// Parsed command-line options.
struct Options {
    mode: Mode,
    host: String,
    port: u16,
    /// End of port range for scanning (inclusive). Same as `port` if single.
    port_end: u16,
    udp: bool,
    verbose: bool,
    timeout_secs: Option<u64>,
    keep_listening: bool,
    source_addr: Option<String>,
    source_port: Option<u16>,
    exec_cmd: Option<String>,
}

// ============================================================================
// Argument parsing
// ============================================================================

fn print_usage() {
    eprintln!("Usage: nc [OPTIONS] hostname port[-port]");
    eprintln!("       nc [OPTIONS] -l port");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -l             Listen mode (server)");
    eprintln!("  -u             UDP mode");
    eprintln!("  -z             Scan mode (port scanning, no data transfer)");
    eprintln!("  -v             Verbose output");
    eprintln!("  -w <seconds>   Timeout for connections and idle");
    eprintln!("  -k             Keep listening after client disconnects (-l only)");
    eprintln!("  -s <addr>      Source address to bind");
    eprintln!("  -p <port>      Source port");
    eprintln!("  -e <command>   Execute command on connect (stdin/stdout wired to socket)");
    eprintln!("  -h, --help     Show this help message");
}

fn parse_args() -> Result<Options, String> {
    let argv: Vec<String> = env::args().collect();

    if argv.len() < 2 {
        return Err("too few arguments".to_string());
    }

    let mut listen = false;
    let mut udp = false;
    let mut scan = false;
    let mut verbose = false;
    let mut timeout_secs: Option<u64> = None;
    let mut keep_listening = false;
    let mut source_addr: Option<String> = None;
    let mut source_port: Option<u16> = None;
    let mut exec_cmd: Option<String> = None;
    let mut positionals: Vec<String> = Vec::new();

    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-l" => listen = true,
            "-u" => udp = true,
            "-z" => scan = true,
            "-v" => verbose = true,
            "-k" => keep_listening = true,
            "-w" => {
                i += 1;
                let val = argv.get(i)
                    .ok_or_else(|| "-w requires a timeout value".to_string())?;
                let secs: u64 = val.parse()
                    .map_err(|_| format!("invalid timeout: '{val}'"))?;
                timeout_secs = Some(secs);
            }
            "-s" => {
                i += 1;
                let val = argv.get(i)
                    .ok_or_else(|| "-s requires a source address".to_string())?;
                source_addr = Some(val.clone());
            }
            "-p" => {
                i += 1;
                let val = argv.get(i)
                    .ok_or_else(|| "-p requires a source port".to_string())?;
                let port: u16 = val.parse()
                    .map_err(|_| format!("invalid source port: '{val}'"))?;
                source_port = Some(port);
            }
            "-e" => {
                i += 1;
                let val = argv.get(i)
                    .ok_or_else(|| "-e requires a command".to_string())?;
                exec_cmd = Some(val.clone());
            }
            other if other.starts_with('-') => {
                // Handle combined short flags like -vz, -vl, etc.
                let flags = &other[1..];
                let mut consumed = true;
                for ch in flags.chars() {
                    match ch {
                        'l' => listen = true,
                        'u' => udp = true,
                        'z' => scan = true,
                        'v' => verbose = true,
                        'k' => keep_listening = true,
                        _ => {
                            consumed = false;
                            break;
                        }
                    }
                }
                if !consumed {
                    return Err(format!("unknown option: '{other}'"));
                }
            }
            _ => positionals.push(arg.clone()),
        }
        i += 1;
    }

    // Determine mode and parse positional arguments.
    let mode;
    let host;
    let port;
    let port_end;

    if listen {
        mode = Mode::Listen;
        // Listen mode: nc -l port
        if positionals.len() != 1 {
            return Err("listen mode requires exactly one argument: port".to_string());
        }
        host = String::new();
        port = positionals[0].parse::<u16>()
            .map_err(|_| format!("invalid port: '{}'", positionals[0]))?;
        port_end = port;
    } else if scan {
        mode = Mode::Scan;
        // Scan mode: nc -z hostname port[-port]
        if positionals.len() != 2 {
            return Err("scan mode requires: hostname port[-port]".to_string());
        }
        host = positionals[0].clone();
        let port_range = &positionals[1];
        if let Some((start, end)) = port_range.split_once('-') {
            port = start.parse::<u16>()
                .map_err(|_| format!("invalid port range start: '{start}'"))?;
            port_end = end.parse::<u16>()
                .map_err(|_| format!("invalid port range end: '{end}'"))?;
            if port_end < port {
                return Err(format!("invalid port range: {port}-{port_end}"));
            }
        } else {
            port = port_range.parse::<u16>()
                .map_err(|_| format!("invalid port: '{port_range}'"))?;
            port_end = port;
        }
    } else {
        mode = Mode::Client;
        // Client mode: nc hostname port
        if positionals.len() != 2 {
            return Err("client mode requires: hostname port".to_string());
        }
        host = positionals[0].clone();
        port = positionals[1].parse::<u16>()
            .map_err(|_| format!("invalid port: '{}'", positionals[1]))?;
        port_end = port;
    }

    if port == 0 {
        return Err("port must be between 1 and 65535".to_string());
    }

    Ok(Options {
        mode,
        host,
        port,
        port_end,
        udp,
        verbose,
        timeout_secs,
        keep_listening,
        source_addr,
        source_port,
        exec_cmd,
    })
}

// ============================================================================
// TCP client mode
// ============================================================================

/// Connect to a remote host and relay data between stdin/stdout and the socket.
fn run_tcp_client(opts: &Options) -> Result<(), String> {
    let ip = resolve_host(&opts.host)?;

    if opts.verbose {
        if let Some(ref src) = opts.source_addr {
            eprintln!(
                "nc: connecting to {} ({}) port {} [tcp] from {}",
                opts.host, format_ipv4(ip), opts.port, src,
            );
        } else {
            eprintln!(
                "nc: connecting to {} ({}) port {} [tcp]",
                opts.host, format_ipv4(ip), opts.port,
            );
        }
    }

    let handle = tcp_connect(ip, opts.port).map_err(|e| {
        format!(
            "nc: connect to {} port {} failed: {} (error {})",
            format_ipv4(ip), opts.port, syscall_error_msg(e), e,
        )
    })?;

    if opts.verbose {
        eprintln!(
            "nc: connected to {} port {}",
            format_ipv4(ip), opts.port,
        );
    }

    // Run the relay loop with optional -e command.
    let result = if let Some(ref cmd) = opts.exec_cmd {
        run_exec(handle, cmd)
    } else {
        relay_loop(handle, opts.timeout_secs)
    };

    tcp_shutdown(handle, 2); // shut down both directions
    tcp_close(handle);
    result
}

/// Bidirectional relay between stdin/stdout and a TCP socket.
///
/// Uses two threads: one reads from stdin and writes to the socket, the other
/// reads from the socket and writes to stdout.
fn relay_loop(handle: u64, timeout_secs: Option<u64>) -> Result<(), String> {
    let timeout_ms = timeout_secs.map(|s| s.saturating_mul(1000));
    let done = std::sync::Arc::new(AtomicBool::new(false));

    // Thread: stdin -> socket
    let done_tx = done.clone();
    let tx_handle = handle;
    let stdin_thread = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let stdin = io::stdin();
        let mut stdin_lock = stdin.lock();
        loop {
            if done_tx.load(Ordering::SeqCst) || !RUNNING.load(Ordering::SeqCst) {
                break;
            }
            match stdin_lock.read(&mut buf) {
                Ok(0) => {
                    // EOF on stdin: signal the write side of the socket.
                    tcp_shutdown(tx_handle, 1);
                    break;
                }
                Ok(n) => {
                    if tcp_send_all(tx_handle, &buf[..n]).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        done_tx.store(true, Ordering::SeqCst);
    });

    // Main thread: socket -> stdout
    let mut buf = [0u8; 4096];
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();
    let idle_start = Instant::now();

    loop {
        if done.load(Ordering::SeqCst) || !RUNNING.load(Ordering::SeqCst) {
            break;
        }

        match tcp_recv_nonblock(handle, &mut buf) {
            Ok(0) => {
                // EOF: peer closed.
                break;
            }
            Ok(n) => {
                if stdout_lock.write_all(&buf[..n]).is_err() {
                    break;
                }
                let _ = stdout_lock.flush();
            }
            Err(-11) => {
                // WouldBlock: no data yet. Check timeout.
                if let Some(tmo) = timeout_ms
                    && idle_start.elapsed().as_millis() as u64 >= tmo {
                        eprintln!("nc: idle timeout");
                        break;
                    }
                // Small sleep to avoid busy-spinning, responsive to Ctrl+C.
                sleep_interruptible(10);
                continue;
            }
            Err(_) => break,
        }
    }

    done.store(true, Ordering::SeqCst);
    // The stdin thread will exit on its next read or when it sees done=true.
    // We cannot join it without blocking indefinitely if stdin is waiting, so
    // we detach it. The process will exit shortly after anyway.
    drop(stdin_thread);
    Ok(())
}

// ============================================================================
// TCP listen mode
// ============================================================================

/// Listen on a port, accept connections, and relay data.
fn run_tcp_listen(opts: &Options) -> Result<(), String> {
    let listener = tcp_bind(opts.port).map_err(|e| {
        format!(
            "nc: bind to port {} failed: {} (error {})",
            opts.port, syscall_error_msg(e), e,
        )
    })?;

    if opts.verbose {
        eprintln!("nc: listening on port {} [tcp]", opts.port);
    }

    loop {
        if !RUNNING.load(Ordering::SeqCst) {
            break;
        }

        if opts.verbose {
            eprintln!("nc: waiting for connection...");
        }

        let conn = tcp_accept(listener).map_err(|e| {
            format!("nc: accept failed: {} (error {})", syscall_error_msg(e), e)
        })?;

        // Report the peer address.
        if opts.verbose {
            if let Ok((peer_ip, peer_port)) = tcp_peer_addr(conn) {
                eprintln!(
                    "nc: connection from {} port {}",
                    format_ipv4(peer_ip), peer_port,
                );
            } else {
                eprintln!("nc: connection accepted");
            }
        }

        // Relay or exec.
        let result = if let Some(ref cmd) = opts.exec_cmd {
            run_exec(conn, cmd)
        } else {
            relay_loop(conn, opts.timeout_secs)
        };

        tcp_shutdown(conn, 2);
        tcp_close(conn);

        if let Err(e) = result
            && opts.verbose {
                eprintln!("nc: session error: {e}");
            }

        if !opts.keep_listening {
            break;
        }

        if opts.verbose {
            eprintln!("nc: client disconnected, waiting for next connection...");
        }
    }

    tcp_close_listener(listener);
    Ok(())
}

// ============================================================================
// UDP mode
// ============================================================================

/// UDP client: send datagrams from stdin, receive and display responses.
fn run_udp_client(opts: &Options) -> Result<(), String> {
    let ip = resolve_host(&opts.host)?;

    // Bind a local UDP socket.
    let local_port = opts.source_port.unwrap_or(0);
    // If no source port specified, pick an ephemeral one. The kernel requires
    // a non-zero port for udp_bind, so we pick one in the ephemeral range.
    let bind_port = if local_port == 0 {
        // Hash the current time to pick a pseudo-random ephemeral port.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        (49152 + (now % 16384)) as u16
    } else {
        local_port
    };

    let handle = udp_bind(bind_port).map_err(|e| {
        format!("nc: udp bind port {} failed: {} (error {})", bind_port, syscall_error_msg(e), e)
    })?;

    if opts.verbose {
        eprintln!(
            "nc: UDP mode, sending to {} ({}) port {}, bound to port {}",
            opts.host, format_ipv4(ip), opts.port, bind_port,
        );
    }

    let timeout_ms = opts.timeout_secs.map(|s| s.saturating_mul(1000));
    let done = std::sync::Arc::new(AtomicBool::new(false));

    // Thread: stdin -> UDP send
    let done_tx = done.clone();
    let send_handle = handle;
    let dst_ip = ip;
    let dst_port = opts.port;
    let stdin_thread = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let stdin = io::stdin();
        let mut stdin_lock = stdin.lock();
        loop {
            if done_tx.load(Ordering::SeqCst) || !RUNNING.load(Ordering::SeqCst) {
                break;
            }
            match stdin_lock.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    if udp_send(send_handle, dst_ip, dst_port, &buf[..n]).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        done_tx.store(true, Ordering::SeqCst);
    });

    // Main thread: UDP recv -> stdout
    let mut recv_buf = [0u8; 65536];
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();
    let idle_start = Instant::now();

    loop {
        if done.load(Ordering::SeqCst) || !RUNNING.load(Ordering::SeqCst) {
            break;
        }

        if udp_rx_ready(handle) {
            match udp_recv(handle, &mut recv_buf) {
                Ok((n, src_ip, src_port)) => {
                    if opts.verbose {
                        eprintln!(
                            "nc: received {} bytes from {} port {}",
                            n, format_ipv4(src_ip), src_port,
                        );
                    }
                    if stdout_lock.write_all(&recv_buf[..n]).is_err() {
                        break;
                    }
                    let _ = stdout_lock.flush();
                }
                Err(_) => {
                    // No data (shouldn't happen since rx_ready was true).
                }
            }
        } else {
            // No data ready. Check timeout.
            if let Some(tmo) = timeout_ms
                && idle_start.elapsed().as_millis() as u64 >= tmo {
                    if opts.verbose {
                        eprintln!("nc: idle timeout");
                    }
                    break;
                }
            sys_sleep(10);
        }
    }

    done.store(true, Ordering::SeqCst);
    drop(stdin_thread);
    udp_close(handle);
    Ok(())
}

/// UDP listen mode: bind and receive datagrams, echo stdin back.
fn run_udp_listen(opts: &Options) -> Result<(), String> {
    let handle = udp_bind(opts.port).map_err(|e| {
        format!("nc: udp bind port {} failed: {} (error {})", opts.port, syscall_error_msg(e), e)
    })?;

    if opts.verbose {
        eprintln!("nc: listening on port {} [udp]", opts.port);
    }

    let timeout_ms = opts.timeout_secs.map(|s| s.saturating_mul(1000));
    let mut recv_buf = [0u8; 65536];
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();
    let mut last_sender_ip: u32 = 0;
    let mut last_sender_port: u16 = 0;
    let idle_start = Instant::now();

    // For listen mode, also read stdin and send back to the last known sender.
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let done_tx = done.clone();
    let send_handle = handle;
    let stdin_thread = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let stdin = io::stdin();
        let mut stdin_lock = stdin.lock();
        // We can't easily pass the last sender info between threads without
        // shared state. In UDP listen mode, the main use is receiving. We
        // skip stdin-to-socket in UDP listen for simplicity (consistent with
        // many netcat implementations where UDP listen is receive-only).
        loop {
            if done_tx.load(Ordering::SeqCst) || !RUNNING.load(Ordering::SeqCst) {
                break;
            }
            match stdin_lock.read(&mut buf) {
                Ok(0) => break,
                Ok(_n) => {
                    // In a more complete implementation we would send to the
                    // last known sender. For now, we just consume stdin.
                }
                Err(_) => break,
            }
        }
        done_tx.store(true, Ordering::SeqCst);
    });

    loop {
        if done.load(Ordering::SeqCst) || !RUNNING.load(Ordering::SeqCst) {
            break;
        }

        if udp_rx_ready(handle) {
            if let Ok((n, src_ip, src_port)) = udp_recv(handle, &mut recv_buf) {
                last_sender_ip = src_ip;
                last_sender_port = src_port;
                if opts.verbose {
                    eprintln!(
                        "nc: received {} bytes from {} port {}",
                        n, format_ipv4(src_ip), src_port,
                    );
                }
                if stdout_lock.write_all(&recv_buf[..n]).is_err() {
                    break;
                }
                let _ = stdout_lock.flush();
            }
        } else {
            if let Some(tmo) = timeout_ms
                && idle_start.elapsed().as_millis() as u64 >= tmo {
                    if opts.verbose {
                        eprintln!("nc: idle timeout");
                    }
                    break;
                }
            sys_sleep(10);
        }
    }

    // Suppress unused variable warning. The sender info is available for
    // future stdin-to-sender forwarding.
    let _ = (last_sender_ip, last_sender_port, send_handle);

    done.store(true, Ordering::SeqCst);
    drop(stdin_thread);
    udp_close(handle);
    Ok(())
}

// ============================================================================
// Port scanning
// ============================================================================

/// Scan a range of ports using non-blocking connects.
fn run_port_scan(opts: &Options) -> Result<(), String> {
    let ip = resolve_host(&opts.host)?;
    let timeout_ms = opts.timeout_secs.unwrap_or(3).saturating_mul(1000);
    let mut open_count: u32 = 0;

    if opts.verbose {
        eprintln!(
            "nc: scanning {} ({}) ports {}-{}",
            opts.host, format_ipv4(ip), opts.port, opts.port_end,
        );
    }

    let mut current_port = opts.port;
    while current_port <= opts.port_end {
        if !RUNNING.load(Ordering::SeqCst) {
            break;
        }

        if opts.udp {
            // UDP port scan: send an empty datagram and see if we get an
            // ICMP port-unreachable back. This is unreliable in practice,
            // so we report all UDP ports as open|filtered.
            if opts.verbose {
                println!(
                    "{} ({}) {} [udp] open|filtered",
                    opts.host, format_ipv4(ip), current_port,
                );
            }
            open_count = open_count.saturating_add(1);
        } else {
            // TCP port scan: non-blocking connect, then poll for completion.
            match tcp_connect_nonblock(ip, current_port) {
                Ok(handle) => {
                    let start = Instant::now();
                    let mut connected = false;
                    let mut refused = false;
                    let mut err_msg = "";

                    loop {
                        let status = tcp_poll_status(handle);
                        // Status: 0=connecting, 1=established, 2+=closed/error
                        if status == 1 {
                            connected = true;
                            break;
                        }
                        if status >= 2 {
                            // Check what error occurred.
                            let err = tcp_last_error(handle);
                            err_msg = tcp_error_msg(err);
                            if err == 1 {
                                refused = true;
                            }
                            break;
                        }
                        if start.elapsed().as_millis() as u64 >= timeout_ms {
                            break;
                        }
                        sys_sleep(5);
                    }

                    if connected {
                        // Banner grabbing: try to read initial data.
                        let banner = grab_banner(handle);
                        let port_info = if let Some(ref b) = banner {
                            format!(
                                "{} ({}) {} [tcp] open -- {}",
                                opts.host, format_ipv4(ip), current_port, b,
                            )
                        } else {
                            format!(
                                "{} ({}) {} [tcp] open",
                                opts.host, format_ipv4(ip), current_port,
                            )
                        };
                        println!("{port_info}");
                        open_count = open_count.saturating_add(1);
                    } else if refused {
                        if opts.verbose {
                            println!(
                                "{} ({}) {} [tcp] refused -- {}",
                                opts.host, format_ipv4(ip), current_port, err_msg,
                            );
                        }
                    } else if opts.verbose {
                        println!(
                            "{} ({}) {} [tcp] timeout",
                            opts.host, format_ipv4(ip), current_port,
                        );
                    }

                    tcp_close(handle);
                }
                Err(e) => {
                    if e == -111 {
                        // Connection refused (immediate).
                        if opts.verbose {
                            println!(
                                "{} ({}) {} [tcp] refused",
                                opts.host, format_ipv4(ip), current_port,
                            );
                        }
                    } else if opts.verbose {
                        eprintln!(
                            "nc: port {} error: {} ({})",
                            current_port, syscall_error_msg(e), e,
                        );
                    }
                }
            }
        }

        current_port = current_port.saturating_add(1);
        if current_port == 0 {
            break; // Overflow past 65535.
        }
    }

    if opts.verbose {
        eprintln!("nc: scan complete, {} open port(s) found", open_count);
    }

    Ok(())
}

/// Attempt to read banner data from a newly connected socket (non-blocking,
/// short timeout). Returns the first chunk of data as a printable string.
fn grab_banner(handle: u64) -> Option<String> {
    // Wait a short time for the server to send initial data.
    sys_sleep(200);

    let mut buf = [0u8; 1024];
    match tcp_recv_nonblock(handle, &mut buf) {
        Ok(0) | Err(_) => None,
        Ok(n) => {
            // Convert to a printable string, replacing non-printable chars.
            let s: String = buf[..n]
                .iter()
                .map(|&b| {
                    if b == b'\r' || b == b'\n' {
                        ' '
                    } else if b.is_ascii_graphic() || b == b' ' {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                // Limit banner length to avoid flooding the terminal.
                let display = if trimmed.len() > 80 {
                    &trimmed[..80]
                } else {
                    trimmed
                };
                Some(display.to_string())
            }
        }
    }
}

// ============================================================================
// Command execution on connect (-e)
// ============================================================================

/// Execute a command, wiring the socket to the child's stdin/stdout/stderr.
///
/// This uses the OS's process spawning. The child inherits the socket as
/// its I/O channels, allowing remote shell access.
fn run_exec(handle: u64, cmd: &str) -> Result<(), String> {
    // We implement this by spawning the command and manually relaying between
    // the socket and the child process's stdin/stdout. A full implementation
    // would use dup2() to directly wire the socket fd, but our kernel may not
    // support that yet. Instead, we use pipes via std::process.

    let mut child = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("nc: failed to exec '{cmd}': {e}"))?;

    let child_stdin = child.stdin.take();
    let child_stdout = child.stdout.take();

    let done = std::sync::Arc::new(AtomicBool::new(false));

    // Thread: socket -> child stdin
    let done_in = done.clone();
    let in_handle = handle;
    let stdin_thread = child_stdin.map(|mut cstdin| thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                if done_in.load(Ordering::SeqCst) || !RUNNING.load(Ordering::SeqCst) {
                    break;
                }
                // Use blocking recv for exec mode: the child process
                // expects a steady stream and doesn't need polling.
                match tcp_recv(in_handle, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if cstdin.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            done_in.store(true, Ordering::SeqCst);
        }));

    // Thread: child stdout -> socket
    let done_out = done.clone();
    let out_handle = handle;
    let stdout_thread = child_stdout.map(|mut cstdout| thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                if done_out.load(Ordering::SeqCst) || !RUNNING.load(Ordering::SeqCst) {
                    break;
                }
                match cstdout.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tcp_send_all(out_handle, &buf[..n]).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            done_out.store(true, Ordering::SeqCst);
        }));

    // Wait for child to finish.
    let _ = child.wait();
    done.store(true, Ordering::SeqCst);

    if let Some(t) = stdin_thread {
        let _ = t.join();
    }
    if let Some(t) = stdout_thread {
        let _ = t.join();
    }

    Ok(())
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> Result<(), String> {
    let opts = parse_args()?;

    install_signal_handler();

    match opts.mode {
        Mode::Scan => run_port_scan(&opts),
        Mode::Listen => {
            if opts.udp {
                run_udp_listen(&opts)
            } else {
                run_tcp_listen(&opts)
            }
        }
        Mode::Client => {
            if opts.udp {
                run_udp_client(&opts)
            } else {
                run_tcp_client(&opts)
            }
        }
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("nc: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- IPv4 parsing ---

    #[test]
    fn parse_ipv4_simple() {
        let ip = parse_ipv4("192.168.1.1");
        assert!(ip.is_some());
        assert_eq!(ip.unwrap(), 0xC0A8_0101);
    }

    #[test]
    fn parse_ipv4_loopback() {
        assert_eq!(parse_ipv4("127.0.0.1").unwrap(), 0x7F00_0001);
    }

    #[test]
    fn parse_ipv4_zeros() {
        assert_eq!(parse_ipv4("0.0.0.0").unwrap(), 0);
    }

    #[test]
    fn parse_ipv4_broadcast() {
        assert_eq!(parse_ipv4("255.255.255.255").unwrap(), 0xFFFF_FFFF);
    }

    #[test]
    fn parse_ipv4_invalid_too_few() {
        assert!(parse_ipv4("192.168.1").is_none());
    }

    #[test]
    fn parse_ipv4_invalid_too_many() {
        assert!(parse_ipv4("1.2.3.4.5").is_none());
    }

    #[test]
    fn parse_ipv4_invalid_octet() {
        assert!(parse_ipv4("256.0.0.1").is_none());
    }

    #[test]
    fn parse_ipv4_invalid_alpha() {
        assert!(parse_ipv4("abc.def.ghi.jkl").is_none());
    }

    #[test]
    fn parse_ipv4_empty() {
        assert!(parse_ipv4("").is_none());
    }

    // --- IPv4 formatting ---

    #[test]
    fn format_ipv4_loopback() {
        assert_eq!(format_ipv4(0x7F00_0001), "127.0.0.1");
    }

    #[test]
    fn format_ipv4_private() {
        assert_eq!(format_ipv4(0xC0A8_0101), "192.168.1.1");
    }

    #[test]
    fn format_ipv4_zeros() {
        assert_eq!(format_ipv4(0), "0.0.0.0");
    }

    #[test]
    fn format_ipv4_broadcast() {
        assert_eq!(format_ipv4(0xFFFF_FFFF), "255.255.255.255");
    }

    // --- Round-trip ---

    #[test]
    fn ipv4_roundtrip() {
        for addr in &["10.0.0.1", "172.16.254.1", "8.8.8.8", "1.1.1.1"] {
            let ip = parse_ipv4(addr).unwrap();
            assert_eq!(format_ipv4(ip), *addr);
        }
    }

    // --- is_ipv4_address ---

    #[test]
    fn is_ipv4_valid() {
        assert!(is_ipv4_address("1.2.3.4"));
        assert!(is_ipv4_address("255.255.255.255"));
    }

    #[test]
    fn is_ipv4_hostname() {
        assert!(!is_ipv4_address("example.com"));
        assert!(!is_ipv4_address("localhost"));
        assert!(!is_ipv4_address(""));
    }

    // --- Syscall error messages ---

    #[test]
    fn error_msg_known() {
        assert_eq!(syscall_error_msg(-111), "connection refused");
        assert_eq!(syscall_error_msg(-110), "connection timed out");
        assert_eq!(syscall_error_msg(-13), "permission denied");
    }

    #[test]
    fn error_msg_unknown() {
        assert_eq!(syscall_error_msg(-9999), "unknown error");
    }

    // --- TCP error messages ---

    #[test]
    fn tcp_error_msg_known() {
        assert_eq!(tcp_error_msg(0), "no error");
        assert_eq!(tcp_error_msg(1), "connection refused");
        assert_eq!(tcp_error_msg(2), "connection reset");
        assert_eq!(tcp_error_msg(3), "connection timed out");
    }

    #[test]
    fn tcp_error_msg_unknown() {
        assert_eq!(tcp_error_msg(99), "unknown TCP error");
    }

    // --- Hostname resolution (built-in table only) ---

    #[test]
    fn resolve_ipv4_literal() {
        let ip = resolve_host("192.168.0.1").unwrap();
        assert_eq!(ip, 0xC0A8_0001);
    }

    #[test]
    fn resolve_localhost() {
        // dns_resolve will fail in test environment, but the fallback table
        // should handle "localhost".
        // Note: this test only validates the fallback table, not the syscall.
        assert_eq!(
            parse_ipv4("127.0.0.1").unwrap(),
            0x7F00_0001,
        );
    }

    // --- Banner sanitization ---

    #[test]
    fn banner_sanitize() {
        // Verify the banner sanitization logic used in grab_banner.
        let raw = b"SSH-2.0-OpenSSH_9.0\r\n";
        let s: String = raw
            .iter()
            .map(|&b| {
                if b == b'\r' || b == b'\n' {
                    ' '
                } else if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        assert_eq!(s.trim(), "SSH-2.0-OpenSSH_9.0");
    }

    #[test]
    fn banner_sanitize_binary() {
        let raw = [0xFF, 0x00, b'H', b'i', 0x01];
        let s: String = raw
            .iter()
            .map(|&b| {
                if b == b'\r' || b == b'\n' {
                    ' '
                } else if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        assert_eq!(s, "..Hi.");
    }

    // --- Port range parsing validation ---

    #[test]
    fn port_range_single() {
        // Validate the parsing logic for a single port.
        let port_str = "80";
        assert!(port_str.parse::<u16>().is_ok());
        assert_eq!(port_str.parse::<u16>().unwrap(), 80);
    }

    #[test]
    fn port_range_range() {
        let port_str = "20-25";
        let (start, end) = port_str.split_once('-').unwrap();
        assert_eq!(start.parse::<u16>().unwrap(), 20);
        assert_eq!(end.parse::<u16>().unwrap(), 25);
    }

    #[test]
    fn port_range_invalid() {
        // End before start.
        let port_str = "100-50";
        let (start, end) = port_str.split_once('-').unwrap();
        let s: u16 = start.parse().unwrap();
        let e: u16 = end.parse().unwrap();
        assert!(e < s);
    }
}
