//! `OurOS` Telnet Client
//!
//! An RFC 854 telnet client for `OurOS`. Connects to a remote host on the
//! default telnet port (23) or a specified port, negotiates telnet options,
//! and provides an interactive terminal session.
//!
//! # Usage
//!
//! ```text
//! telnet hostname [port]          Connect to hostname on port (default 23)
//! telnet -l user hostname [port]  Connect and send auto-login username
//! telnet -e char hostname [port]  Set escape character (default Ctrl+])
//! ```
//!
//! # Escape mode
//!
//! Press the escape character (default Ctrl+]) to enter command mode:
//! ```text
//! telnet> quit        Close connection and exit
//! telnet> close       Close connection (stay in command mode)
//! telnet> status      Show connection status
//! telnet> send <seq>  Send special sequences (e.g. send nop)
//! telnet> ?           Show help
//! ```
//!
//! # Telnet option negotiation
//!
//! This client supports a fixed set of options (RFC 854 / RFC 855 family):
//! - ECHO (option 1):  we request the server WILL ECHO so we suppress local echo.
//! - SGA  (option 3):  we request WILL SGA and send DO SGA.
//! - TTYPE (option 24): we advertise WILL TTYPE and respond with "xterm-256color".
//! - NAWS  (option 31): we send our window size when requested.
//!
//! All other WILL/DO proposals from the server are rejected with WONT/DONT.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
//
// telnet runs a byte-pump between stdin and a TCP socket with inline IAC
// option negotiation — every option-byte read and inline command parse
// is offset+length arithmetic on a length-validated read buffer. The
// defensive `arithmetic_side_effects`, `indexing_slicing`, and
// `slicing` lints fire on every such site (30+ warnings) with no real
// DoS risk; buffer indices come from the kernel read() return value.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
)]

use std::env;
use std::process;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

// ============================================================================
// Syscall numbers (from kernel/src/syscall/net syscall table)
// ============================================================================

// OurOS native console syscalls (kernel syscall/number.rs).  There is no
// Linux-style fd read/write or ioctl: 0 and 1 are SYS_YIELD/SYS_EXIT here, so
// the previous SYS_READ=0/SYS_WRITE=1 actually yielded and *terminated* the
// process.  Terminal I/O goes through the bootstrap console syscalls instead.
const SYS_CONSOLE_WRITE: u64 = 100;
const SYS_CONSOLE_READ_CHAR: u64 = 101;
const SYS_CONSOLE_TRY_READ_CHAR: u64 = 103;
const SYS_TCP_CONNECT: u64 = 800;
const SYS_TCP_SEND: u64 = 801;
const SYS_TCP_RECV: u64 = 802;
const SYS_TCP_CLOSE: u64 = 803;
const SYS_DNS_RESOLVE: u64 = 820;

// ============================================================================
// Syscall wrappers
// ============================================================================

/// Issue a 3-argument syscall.
///
/// # Safety
///
/// Caller must ensure `nr` is a valid syscall number and all arguments are
/// valid for that syscall. The `syscall` instruction clobbers `rcx` and `r11`.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Arguments are validated by the caller. rcx/r11 are clobbered by
    // the syscall instruction per the x86_64 syscall ABI and declared here.
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

/// Issue a 1-argument syscall.
///
/// # Safety
///
/// Caller must ensure `nr` is a valid syscall number and `a1` is valid.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall1(nr: u64, a1: u64) -> i64 {
    let ret: i64;
    // SAFETY: Same ABI as syscall3. Single-argument variant.
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

/// Stub implementations for non-x86_64 (used when running `cargo test` on the
/// host).  These are never called at runtime on a non-x86_64 host because the
/// test suite avoids exercising paths that issue real syscalls.
#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 {
    -1
}

#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall1(_nr: u64, _a1: u64) -> i64 {
    -1
}

// ============================================================================
// Networking: TCP + DNS wrappers
// ============================================================================

/// Connect to `ip:port` (blocking). Returns a connection handle on success.
fn tcp_connect(ip: u32, port: u16, timeout_ms: u64) -> Result<u64, i64> {
    // arg3 encodes timeout in milliseconds; 0 = use kernel default.
    // SAFETY: SYS_TCP_CONNECT takes (ip, port, timeout_ms), all scalars.
    let ret = unsafe { syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), timeout_ms) };
    if ret < 0 { Err(ret) } else { Ok(ret as u64) }
}

/// Send `data` on a TCP connection. Returns bytes sent.
fn tcp_send(handle: u64, data: &[u8]) -> Result<usize, i64> {
    // SAFETY: handle is a valid connection handle. data slice is valid.
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

/// Send all bytes in `data`, looping until fully transmitted.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), i64> {
    let mut offset = 0usize;
    while offset < data.len() {
        let n = tcp_send(handle, &data[offset..])?;
        if n == 0 {
            return Err(-5); // EIO
        }
        offset = offset.saturating_add(n);
    }
    Ok(())
}

/// Receive data from a TCP connection (blocking). Returns 0 on EOF (peer closed).
/// Kept for completeness; the session loop uses `tcp_recv_nonblock`.
#[allow(dead_code)]
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, i64> {
    // SAFETY: handle is valid. buf is a valid mutable slice.
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

/// Non-blocking receive. Returns `Err(-11)` (EAGAIN/EWOULDBLOCK) if no data.
fn tcp_recv_nonblock(handle: u64, buf: &mut [u8]) -> Result<usize, i64> {
    // arg3 = MSG_DONTWAIT flag (0x40), matching the nc client convention.
    const MSG_DONTWAIT: u64 = 0x40;
    // SAFETY: handle is valid. buf is a valid mutable slice. arg3 is a flag.
    let ret = unsafe {
        // Use syscall4 inline since we only need it here.
        let r: i64;
        #[cfg(target_arch = "x86_64")]
        {
            core::arch::asm!(
                "syscall",
                inlateout("rax") SYS_TCP_RECV as i64 => r,
                in("rdi") handle,
                in("rsi") buf.as_mut_ptr() as u64,
                in("rdx") buf.len() as u64,
                in("r10") MSG_DONTWAIT,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            r = -1i64;
            let _ = (handle, buf, MSG_DONTWAIT);
        }
        r
    };
    if ret < 0 { Err(ret) } else { Ok(ret as usize) }
}

/// Close a TCP connection.
fn tcp_close(handle: u64) {
    // SAFETY: handle is (or was) a valid TCP connection handle.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

/// Resolve a hostname to a u32 IPv4 address (network byte order).
fn dns_resolve(hostname: &str) -> Result<u32, i64> {
    // The kernel writes the four address octets [a, b, c, d] (MSB first) into
    // this buffer.  Reading them back as a native-endian u32 on little-endian
    // x86_64 would reverse the address, so reassemble explicitly with
    // from_be_bytes (matching sys_tcp_connect's Ipv4Addr::from_u32 == to_be_bytes).
    let mut octets = [0u8; 4];
    // SAFETY: hostname slice is valid; octets is a 4-byte stack buffer.
    let ret = unsafe {
        syscall3(
            SYS_DNS_RESOLVE,
            hostname.as_ptr() as u64,
            hostname.len() as u64,
            octets.as_mut_ptr() as u64,
        )
    };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(u32::from_be_bytes(octets))
    }
}

// ============================================================================
// Terminal window size
// ============================================================================

/// Rows of the `OurOS` bootstrap framebuffer console.
const CONSOLE_ROWS: u16 = 25;
/// Columns of the `OurOS` bootstrap framebuffer console.
const CONSOLE_COLS: u16 = 80;

/// Return the terminal dimensions as (rows, cols).
///
/// `OurOS` has no `ioctl`/`TIOCGWINSZ` syscall (and no resizable terminal yet):
/// the bootstrap console is a fixed-size framebuffer grid.  Report its actual
/// dimensions so NAWS negotiation advertises a sensible size.
fn get_terminal_size() -> (u16, u16) {
    (CONSOLE_ROWS, CONSOLE_COLS)
}

// ============================================================================
// Low-level I/O: raw stdin read / stdout write
// ============================================================================

/// Read available keyboard bytes into `buf`.
///
/// Blocks until at least one byte is available (matching a typical blocking
/// stdin read), then drains any further immediately-available bytes without
/// blocking.  Returns the number of bytes read.
///
/// The `OurOS` bootstrap console keyboard never reports EOF, so for a non-empty
/// `buf` this only returns 0 on a syscall error.  Because the first read
/// blocks, a thread parked here cannot observe a shutdown flag until the next
/// keypress — a known limitation of the fixed bootstrap console.
fn stdin_read(buf: &mut [u8]) -> usize {
    if buf.is_empty() {
        return 0;
    }
    let mut ch: u8 = 0;
    // Block for the first byte.
    // SAFETY: SYS_CONSOLE_READ_CHAR writes one byte to the provided pointer.
    let ret = unsafe { syscall1(SYS_CONSOLE_READ_CHAR, &raw mut ch as u64) };
    if ret < 0 {
        return 0;
    }
    let mut n = 0usize;
    if let Some(slot) = buf.get_mut(n) {
        *slot = ch;
        n = n.saturating_add(1);
    }
    // Drain any further buffered bytes without blocking.
    while n < buf.len() {
        // SAFETY: SYS_CONSOLE_TRY_READ_CHAR writes one byte or returns WouldBlock.
        let r = unsafe { syscall1(SYS_CONSOLE_TRY_READ_CHAR, &raw mut ch as u64) };
        if r < 0 {
            break; // WouldBlock: no more buffered input.
        }
        if let Some(slot) = buf.get_mut(n) {
            *slot = ch;
            n = n.saturating_add(1);
        } else {
            break;
        }
    }
    n
}

/// Write all bytes of `data` to the console.
fn console_write_all(data: &[u8]) {
    let mut offset = 0usize;
    while offset < data.len() {
        let Some(chunk) = data.get(offset..) else {
            break;
        };
        // SAFETY: SYS_CONSOLE_WRITE takes (ptr, len) and writes to the console.
        let ret = unsafe {
            syscall3(
                SYS_CONSOLE_WRITE,
                chunk.as_ptr() as u64,
                chunk.len() as u64,
                0,
            )
        };
        if ret <= 0 {
            break;
        }
        offset = offset.saturating_add(ret as usize);
    }
}

/// Write all bytes of `data` to the console (stdout).
fn stdout_write(data: &[u8]) {
    console_write_all(data);
}

/// Write all bytes of `data` to the console (stderr).
///
/// `OurOS`'s bootstrap console has no separate stderr stream, so this writes to
/// the same console as `stdout_write`.
fn stderr_write(data: &[u8]) {
    console_write_all(data);
}

// ============================================================================
// IPv4 utilities
// ============================================================================

/// Parse a dotted-decimal IPv4 string into a network-byte-order u32.
// a/b/c/d are the conventional names for the four IPv4 octets.
#[allow(clippy::many_single_char_names)]
fn parse_ipv4(s: &str) -> Option<u32> {
    let mut parts = s.splitn(5, '.');
    let a: u8 = parts.next()?.parse().ok()?;
    let b: u8 = parts.next()?.parse().ok()?;
    let c: u8 = parts.next()?.parse().ok()?;
    let d_str = parts.next()?;
    if parts.next().is_some() {
        return None; // too many parts
    }
    let d: u8 = d_str.parse().ok()?;
    Some(u32::from_be_bytes([a, b, c, d]))
}

/// Format a network-byte-order u32 as dotted-decimal.
fn format_ipv4(ip: u32) -> String {
    let o = ip.to_be_bytes();
    format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])
}

/// Resolve a hostname to an IPv4 address, handling dotted-decimal directly.
fn resolve_host(hostname: &str) -> Result<u32, String> {
    if let Some(ip) = parse_ipv4(hostname) {
        return Ok(ip);
    }
    if hostname == "localhost" {
        return Ok(0x7F00_0001);
    }
    dns_resolve(hostname).map_err(|e| format!("cannot resolve '{hostname}': error {e}"))
}

// ============================================================================
// Telnet protocol constants (RFC 854)
// ============================================================================

/// Interpret As Command — the telnet escape byte.
const IAC: u8 = 255;

// Telnet command bytes (follow IAC).
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250; // Subnegotiation begin
const SE: u8 = 240; // Subnegotiation end
const NOP: u8 = 241; // No-operation
const DM: u8 = 242; // Data mark
const BRK: u8 = 243; // Break
const IP: u8 = 244; // Interrupt process
const AO: u8 = 245; // Abort output
const AYT: u8 = 246; // Are you there
const EC: u8 = 247; // Erase character
const EL: u8 = 248; // Erase line
const GA: u8 = 249; // Go ahead

// Telnet options we care about.
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_TTYPE: u8 = 24;
const OPT_NAWS: u8 = 31;

// Subnegotiation sub-commands.
const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;

// ============================================================================
// Option negotiation state
// ============================================================================

/// Whether we have agreed to an option on our side (local) or the server's side.
/// The `WantYes`/`WantNo` variants are part of the RFC 855 queue discipline and
/// are included for completeness even though the current implementation does not
/// yet use the full queued-negotiation flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum OptState {
    /// Not negotiated / default off.
    No,
    /// Option is active.
    Yes,
    /// We sent a request and are waiting for the server's reply.
    WantYes,
    /// We sent a cancel and are waiting for confirmation.
    WantNo,
}

/// Tracks the negotiation state for both sides of each option.
struct OptionTable {
    /// local: do WE perform this option?
    local: [OptState; 256],
    /// remote: does the SERVER perform this option?
    remote: [OptState; 256],
}

impl OptionTable {
    fn new() -> Self {
        Self {
            local: [OptState::No; 256],
            remote: [OptState::No; 256],
        }
    }
}

// ============================================================================
// Telnet option negotiation helper: build IAC responses
// ============================================================================

/// Append an `IAC WILL <opt>` sequence to `out`.
fn iac_will(out: &mut Vec<u8>, opt: u8) {
    out.extend_from_slice(&[IAC, WILL, opt]);
}

/// Append an `IAC WONT <opt>` sequence to `out`.
fn iac_wont(out: &mut Vec<u8>, opt: u8) {
    out.extend_from_slice(&[IAC, WONT, opt]);
}

/// Append an `IAC DO <opt>` sequence to `out`.
fn iac_do(out: &mut Vec<u8>, opt: u8) {
    out.extend_from_slice(&[IAC, DO, opt]);
}

/// Append an `IAC DONT <opt>` sequence to `out`.
fn iac_dont(out: &mut Vec<u8>, opt: u8) {
    out.extend_from_slice(&[IAC, DONT, opt]);
}

// ============================================================================
// Telnet parser state machine
// ============================================================================

/// States of the IAC parser.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ParseState {
    /// Normal data — pass through to terminal.
    Data,
    /// Saw IAC, waiting for command byte.
    Iac,
    /// Saw IAC + WILL/WONT/DO/DONT — waiting for option byte.
    Opt(u8),
    /// Inside subnegotiation (IAC SB ... IAC SE).
    Subneg,
    /// Inside subnegotiation, saw IAC (could be SE or escaped IAC).
    SubnegIac,
}

/// Accumulated subnegotiation buffer.
const SUBNEG_MAX: usize = 512;

// ============================================================================
// Telnet session state
// ============================================================================

/// Session connection state.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ConnState {
    /// Not yet connected.
    Disconnected,
    /// TCP connection established, telnet active.
    Connected,
    /// Connection closed by us or the peer.
    Closed,
}

/// All mutable state for one telnet session.
struct Session {
    /// TCP connection handle (valid only when `conn == Connected`).
    handle: u64,
    conn: ConnState,
    /// Option negotiation state.
    opts: OptionTable,
    /// IAC parser state.
    parse: ParseState,
    /// Accumulator for current subnegotiation payload.
    subneg_buf: Vec<u8>,
    /// Remote host name (for display).
    remote_host: String,
    /// Remote port.
    remote_port: u16,
    /// Escape character (default 0x1D = Ctrl+]).
    escape_char: u8,
    /// Optional username to send at login prompt (from -l flag).
    login_user: Option<String>,
}

impl Session {
    fn new(host: String, port: u16, escape_char: u8, login_user: Option<String>) -> Self {
        Self {
            handle: 0,
            conn: ConnState::Disconnected,
            opts: OptionTable::new(),
            parse: ParseState::Data,
            subneg_buf: Vec::new(),
            remote_host: host,
            remote_port: port,
            escape_char,
            login_user,
        }
    }
}

// ============================================================================
// Process incoming telnet bytes
// ============================================================================

/// Process a chunk of bytes from the network, separating telnet control
/// sequences from printable data. Returns data bytes to display and any
/// IAC responses to send back.
///
/// `plain_out`: data bytes to write to the terminal.
/// `net_out`:   IAC responses to send to the server.
fn process_incoming(
    session: &mut Session,
    input: &[u8],
    plain_out: &mut Vec<u8>,
    net_out: &mut Vec<u8>,
) {
    for &byte in input {
        match session.parse {
            ParseState::Data => {
                if byte == IAC {
                    session.parse = ParseState::Iac;
                } else {
                    plain_out.push(byte);
                }
            }

            ParseState::Iac => {
                match byte {
                    IAC => {
                        // Escaped IAC — literal 0xFF in data stream.
                        plain_out.push(IAC);
                        session.parse = ParseState::Data;
                    }
                    WILL | WONT | DO | DONT => {
                        session.parse = ParseState::Opt(byte);
                    }
                    SB => {
                        session.subneg_buf.clear();
                        session.parse = ParseState::Subneg;
                    }
                    SE => {
                        // Stray SE without matching SB — ignore.
                        session.parse = ParseState::Data;
                    }
                    NOP | DM | BRK | IP | AO | AYT | EC | EL | GA => {
                        // Single-byte commands. GA can be ignored (we use SGA).
                        session.parse = ParseState::Data;
                    }
                    _ => {
                        // Unknown command byte — ignore.
                        session.parse = ParseState::Data;
                    }
                }
            }

            ParseState::Opt(cmd) => {
                handle_option_negotiation(session, cmd, byte, net_out);
                session.parse = ParseState::Data;
            }

            ParseState::Subneg => {
                if byte == IAC {
                    session.parse = ParseState::SubnegIac;
                } else if session.subneg_buf.len() < SUBNEG_MAX {
                    session.subneg_buf.push(byte);
                }
                // If buffer is full we drop the byte silently (a malformed
                // or hostile server — we'll wait for the IAC SE to finish).
            }

            ParseState::SubnegIac => {
                if byte == SE {
                    // Subnegotiation complete.
                    let buf = session.subneg_buf.clone();
                    handle_subnegotiation(session, &buf, net_out);
                    session.subneg_buf.clear();
                    session.parse = ParseState::Data;
                } else if byte == IAC {
                    // Escaped IAC inside subneg.
                    if session.subneg_buf.len() < SUBNEG_MAX {
                        session.subneg_buf.push(IAC);
                    }
                    session.parse = ParseState::Subneg;
                } else {
                    // Any other byte: treat as malformed, restart.
                    session.subneg_buf.clear();
                    session.parse = ParseState::Data;
                }
            }
        }
    }
}

/// Handle an `IAC <cmd> <opt>` negotiation command from the server.
///
/// RFC 855 negotiation rules:
/// - Server says WILL <opt>: we reply DO if we want it, DONT otherwise.
/// - Server says WONT <opt>: acknowledge with DONT.
/// - Server says DO <opt>: we reply WILL if we support it, WONT otherwise.
/// - Server says DONT <opt>: acknowledge with WONT.
fn handle_option_negotiation(session: &mut Session, cmd: u8, opt: u8, net_out: &mut Vec<u8>) {
    let uopt = usize::from(opt);
    match cmd {
        WILL => {
            // Server offers to enable opt on its side.
            match opt {
                OPT_ECHO => {
                    // We want the server to echo: accept.
                    if session.opts.remote[uopt] != OptState::Yes {
                        session.opts.remote[uopt] = OptState::Yes;
                        iac_do(net_out, opt);
                    }
                }
                OPT_SGA => {
                    // Server suppresses go-ahead: accept.
                    if session.opts.remote[uopt] != OptState::Yes {
                        session.opts.remote[uopt] = OptState::Yes;
                        iac_do(net_out, opt);
                    }
                }
                _ => {
                    // Reject anything else.
                    iac_dont(net_out, opt);
                }
            }
        }
        WONT
            // Server refuses or disables opt on its side. Acknowledge.
            if session.opts.remote[uopt] != OptState::No => {
                session.opts.remote[uopt] = OptState::No;
                iac_dont(net_out, opt);
            }
        DO => {
            // Server requests we enable opt on our side.
            match opt {
                OPT_SGA => {
                    if session.opts.local[uopt] != OptState::Yes {
                        session.opts.local[uopt] = OptState::Yes;
                        iac_will(net_out, opt);
                    }
                }
                OPT_TTYPE => {
                    // We support terminal type: agree.
                    if session.opts.local[uopt] != OptState::Yes {
                        session.opts.local[uopt] = OptState::Yes;
                        iac_will(net_out, opt);
                    }
                }
                OPT_NAWS => {
                    // Window size: agree and send current size.
                    if session.opts.local[uopt] != OptState::Yes {
                        session.opts.local[uopt] = OptState::Yes;
                        iac_will(net_out, opt);
                        append_naws(net_out);
                    }
                }
                _ => {
                    // We don't support this option on our side.
                    iac_wont(net_out, opt);
                }
            }
        }
        DONT
            // Server wants us to stop the option. Acknowledge.
            if session.opts.local[uopt] != OptState::No => {
                session.opts.local[uopt] = OptState::No;
                iac_wont(net_out, opt);
            }
        _ => {
            // Should not reach here — Opt() only stores WILL/WONT/DO/DONT.
        }
    }
}

/// Handle a completed subnegotiation payload `IAC SB <payload> IAC SE`.
fn handle_subnegotiation(_session: &mut Session, payload: &[u8], net_out: &mut Vec<u8>) {
    if payload.is_empty() {
        return;
    }
    match payload[0] {
        OPT_TTYPE
            // Expect: TTYPE SEND — server is asking for our terminal type.
            if payload.get(1) == Some(&TTYPE_SEND) => {
                // Reply: IAC SB TTYPE IS "xterm-256color" IAC SE
                let term = b"xterm-256color";
                net_out.extend_from_slice(&[IAC, SB, OPT_TTYPE, TTYPE_IS]);
                net_out.extend_from_slice(term);
                net_out.extend_from_slice(&[IAC, SE]);
            }
        OPT_NAWS => {
            // Server requesting NAWS again — resend.
            append_naws(net_out);
        }
        _ => {
            // Unknown subneg — ignore.
        }
    }
}

/// Build and append an `IAC SB NAWS <cols_hi> <cols_lo> <rows_hi> <rows_lo> IAC SE`
/// sequence reflecting the current terminal size.
fn append_naws(out: &mut Vec<u8>) {
    let (rows, cols) = get_terminal_size();
    out.extend_from_slice(&[IAC, SB, OPT_NAWS]);
    // Cols high byte, cols low byte, rows high byte, rows low byte.
    // IAC bytes in the data must be doubled (RFC 855 §3).
    push_naws_byte(out, (cols >> 8) as u8);
    push_naws_byte(out, (cols & 0xFF) as u8);
    push_naws_byte(out, (rows >> 8) as u8);
    push_naws_byte(out, (rows & 0xFF) as u8);
    out.extend_from_slice(&[IAC, SE]);
}

/// Push one byte for a NAWS subneg payload, escaping 0xFF as 0xFF 0xFF.
fn push_naws_byte(out: &mut Vec<u8>, b: u8) {
    out.push(b);
    if b == IAC {
        out.push(IAC); // RFC 855: IAC in subneg payload must be doubled
    }
}

// ============================================================================
// Initial option requests sent upon connection
// ============================================================================

/// Send the initial option negotiations after connecting.
/// We proactively request:
/// - DO ECHO    — ask server to echo our input
/// - DO SGA     — ask server to suppress go-ahead
/// - WILL SGA   — we suppress go-ahead too
/// - WILL TTYPE — we will provide terminal type if asked
/// - WILL NAWS  — we will provide window size
fn send_initial_options(handle: u64) -> Result<(), i64> {
    let mut buf = Vec::with_capacity(18);
    iac_do(&mut buf, OPT_ECHO);
    iac_do(&mut buf, OPT_SGA);
    iac_will(&mut buf, OPT_SGA);
    iac_will(&mut buf, OPT_TTYPE);
    iac_will(&mut buf, OPT_NAWS);
    tcp_send_all(handle, &buf)
}

// ============================================================================
// Escape command mode
// ============================================================================

/// Commands recognised in escape mode.
#[derive(Debug, PartialEq, Eq)]
enum EscapeCmd {
    Quit,
    Close,
    Status,
    SendNop,
    SendAyt,
    SendBrk,
    Help,
    Unknown(String),
}

/// Parse a line typed in escape mode.
fn parse_escape_cmd(line: &str) -> EscapeCmd {
    let trimmed = line.trim();
    let lower = trimmed.to_lowercase();
    match lower.as_str() {
        "quit" | "q" | "exit" => EscapeCmd::Quit,
        "close" => EscapeCmd::Close,
        "status" => EscapeCmd::Status,
        "send nop" | "send nop\n" => EscapeCmd::SendNop,
        "send ayt" => EscapeCmd::SendAyt,
        "send brk" | "send break" => EscapeCmd::SendBrk,
        "?" | "help" => EscapeCmd::Help,
        _ => EscapeCmd::Unknown(trimmed.to_string()),
    }
}

/// Print the escape-mode help text.
fn print_escape_help() {
    stdout_write(b"Commands:\r\n");
    stdout_write(b"  quit        -- close connection and exit\r\n");
    stdout_write(b"  close       -- close connection\r\n");
    stdout_write(b"  status      -- show connection status\r\n");
    stdout_write(b"  send nop    -- send telnet NOP\r\n");
    stdout_write(b"  send ayt    -- send Are-You-There\r\n");
    stdout_write(b"  send brk    -- send Break\r\n");
    stdout_write(b"  ?           -- show this help\r\n");
}

/// Print connection status in escape mode.
fn print_status(session: &Session) {
    match session.conn {
        ConnState::Connected => {
            let msg = format!(
                "Connected to {} port {}.\r\n",
                session.remote_host, session.remote_port
            );
            stdout_write(msg.as_bytes());
            let echo = if session.opts.remote[usize::from(OPT_ECHO)] == OptState::Yes {
                "server"
            } else {
                "local"
            };
            let msg2 = format!(
                "Echo: {}. Escape character: ^{}.\r\n",
                echo,
                (session.escape_char + b'@') as char
            );
            stdout_write(msg2.as_bytes());
        }
        ConnState::Disconnected | ConnState::Closed => {
            stdout_write(b"No connection.\r\n");
        }
    }
}

/// Run the interactive escape-command loop. Returns `true` if we should quit
/// the whole program, `false` to return to the session.
fn run_escape_mode(session: &mut Session) -> bool {
    stdout_write(b"\r\ntelnet> ");

    let mut line_buf = [0u8; 128];
    let mut line = String::new();

    // Read characters until newline or EOF, echoing locally.
    loop {
        let n = stdin_read(&mut line_buf);
        if n == 0 {
            return true; // EOF — quit
        }
        for &b in &line_buf[..n] {
            match b {
                b'\r' | b'\n' => {
                    stdout_write(b"\r\n");
                    break;
                }
                0x08 | 0x7F => {
                    // Backspace / DEL.
                    if !line.is_empty() {
                        line.pop();
                        stdout_write(b"\x08 \x08");
                    }
                }
                _ => {
                    if b.is_ascii_graphic() || b == b' ' {
                        line.push(b as char);
                        stdout_write(&[b]);
                    }
                }
            }
        }
        // Break out of char loop if we found a newline.
        if line_buf[..n].contains(&b'\n') || line_buf[..n].contains(&b'\r') {
            break;
        }
    }

    match parse_escape_cmd(&line) {
        EscapeCmd::Quit => {
            if session.conn == ConnState::Connected {
                tcp_close(session.handle);
                session.conn = ConnState::Closed;
            }
            return true;
        }
        EscapeCmd::Close => {
            if session.conn == ConnState::Connected {
                tcp_close(session.handle);
                session.conn = ConnState::Closed;
                stdout_write(b"Connection closed.\r\n");
            }
        }
        EscapeCmd::Status => {
            print_status(session);
        }
        EscapeCmd::SendNop => {
            if session.conn == ConnState::Connected {
                let _ = tcp_send_all(session.handle, &[IAC, NOP]);
            }
        }
        EscapeCmd::SendAyt => {
            if session.conn == ConnState::Connected {
                let _ = tcp_send_all(session.handle, &[IAC, AYT]);
            }
        }
        EscapeCmd::SendBrk => {
            if session.conn == ConnState::Connected {
                let _ = tcp_send_all(session.handle, &[IAC, BRK]);
            }
        }
        EscapeCmd::Help => {
            print_escape_help();
        }
        EscapeCmd::Unknown(ref s) => {
            if !s.is_empty() {
                let msg = format!("?Invalid command '{s}'\r\n");
                stdout_write(msg.as_bytes());
            }
        }
    }

    if session.conn != ConnState::Connected {
        return false; // closed during command — drop back to shell
    }

    stdout_write(b"telnet> (press return to return to session) ");
    // Swallow the confirming newline.
    let mut dummy = [0u8; 8];
    let _ = stdin_read(&mut dummy);
    stdout_write(b"\r\n");
    false
}

// ============================================================================
// Encode outgoing user data: escape IAC bytes
// ============================================================================

/// Copy `input` into `out`, doubling any 0xFF (IAC) bytes per RFC 854.
fn encode_data(input: &[u8], out: &mut Vec<u8>) {
    for &b in input {
        out.push(b);
        if b == IAC {
            out.push(IAC);
        }
    }
}

// ============================================================================
// Main session loop
// ============================================================================

/// Global flag set when the session should terminate.
static RUNNING: AtomicBool = AtomicBool::new(true);

/// Connect and run the interactive telnet session.
// This is the linear session driver (resolve, connect, spawn the stdin
// thread, run the select-style poll loop, then tear down).  The steps share
// a lot of tightly-coupled local state, so splitting it would scatter that
// state across helpers and hurt readability more than the length does.
#[allow(clippy::too_many_lines)]
fn run_session(session: &mut Session, connect_timeout_ms: u64) -> Result<(), String> {
    // Resolve host.
    let ip = resolve_host(&session.remote_host)?;

    let msg = format!(
        "Trying {} ({})...\r\n",
        session.remote_host,
        format_ipv4(ip)
    );
    stdout_write(msg.as_bytes());

    // Connect.
    let handle = tcp_connect(ip, session.remote_port, connect_timeout_ms).map_err(|e| {
        format!(
            "telnet: connection to {} port {} failed (error {})",
            format_ipv4(ip),
            session.remote_port,
            e
        )
    })?;

    session.handle = handle;
    session.conn = ConnState::Connected;

    let msg2 = format!(
        "Connected to {} port {}.\r\nEscape character is '^{}'.\r\n",
        session.remote_host,
        session.remote_port,
        (session.escape_char + b'@') as char
    );
    stdout_write(msg2.as_bytes());

    // Send initial option negotiation.
    if let Err(e) = send_initial_options(handle) {
        let err_msg = format!("telnet: warning: could not send options (error {e})\r\n");
        stderr_write(err_msg.as_bytes());
    }

    // If -l was given, queue the username followed by a newline.
    if let Some(ref user) = session.login_user.clone() {
        let mut login_bytes = user.as_bytes().to_vec();
        login_bytes.push(b'\r');
        login_bytes.push(b'\n');
        let _ = tcp_send_all(handle, &login_bytes);
    }

    // Shared flag used by the stdin thread to signal the main loop.
    let done = Arc::new(AtomicBool::new(false));

    // --- Stdin thread ---
    // Reads from stdin and sends to the socket, watching for the escape char.
    // Sends via a channel (we use a simple pipe-free mechanism: the stdin
    // thread writes into a shared buffer protected by a Mutex, and signals
    // the main thread via an AtomicBool).  For simplicity (and to avoid
    // std::sync::mpsc overhead in a no-heap-growth context) we use a small
    // channel over a shared Vec protected by a Mutex.
    let done_tx = Arc::clone(&done);
    let escape_char = session.escape_char;

    // We use a pair of Arcs: one for raw user input to send and one for
    // "enter escape mode" requests.
    let escape_request = Arc::new(AtomicBool::new(false));
    let escape_req_tx = Arc::clone(&escape_request);

    // A shared ring-buffer for stdin→socket data.  Using Mutex<Vec<u8>>.
    let stdin_buf = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let stdin_buf_tx = Arc::clone(&stdin_buf);

    let stdin_thread = thread::spawn(move || {
        let mut raw = [0u8; 256];
        loop {
            if done_tx.load(Ordering::Relaxed) || !RUNNING.load(Ordering::Relaxed) {
                break;
            }
            let n = stdin_read(&mut raw);
            if n == 0 {
                // EOF on stdin.
                done_tx.store(true, Ordering::Relaxed);
                break;
            }

            // Scan for escape character.
            let mut send_data = Vec::with_capacity(n);
            let mut saw_escape = false;
            for &b in &raw[..n] {
                if b == escape_char {
                    saw_escape = true;
                    break;
                }
                send_data.push(b);
            }

            if !send_data.is_empty()
                && let Ok(mut guard) = stdin_buf_tx.lock()
            {
                guard.extend_from_slice(&send_data);
            }

            if saw_escape {
                escape_req_tx.store(true, Ordering::Relaxed);
                // Wait for main thread to acknowledge (spin with small yield).
                let start = Instant::now();
                while escape_req_tx.load(Ordering::Relaxed) {
                    if start.elapsed() > Duration::from_secs(10) {
                        break;
                    }
                    thread::yield_now();
                }
            }
        }
    });

    // --- Main loop ---
    let mut net_buf = [0u8; 4096];
    let mut plain = Vec::with_capacity(4096);
    let mut net_out = Vec::with_capacity(128);

    while session.conn == ConnState::Connected && RUNNING.load(Ordering::Relaxed) {
        // Drain stdin data and send to server.
        {
            let mut encoded = Vec::new();
            if let Ok(mut guard) = stdin_buf.lock()
                && !guard.is_empty()
            {
                encode_data(&guard, &mut encoded);
                guard.clear();
            }
            if !encoded.is_empty() && tcp_send_all(session.handle, &encoded).is_err() {
                break;
            }
        }

        // Handle escape mode request from stdin thread.
        if escape_request.load(Ordering::Relaxed) {
            if run_escape_mode(session) {
                // User chose quit.
                break;
            }
            escape_request.store(false, Ordering::Relaxed); // acknowledge
        }

        if session.conn != ConnState::Connected {
            break;
        }

        // Read data from server (non-blocking).
        match tcp_recv_nonblock(session.handle, &mut net_buf) {
            Ok(0) => {
                // EOF: server closed connection.
                stdout_write(b"\r\nConnection closed by foreign host.\r\n");
                session.conn = ConnState::Closed;
                break;
            }
            Ok(n) => {
                plain.clear();
                net_out.clear();
                process_incoming(session, &net_buf[..n], &mut plain, &mut net_out);

                if !plain.is_empty() {
                    stdout_write(&plain);
                }
                if !net_out.is_empty() && tcp_send_all(session.handle, &net_out).is_err() {
                    break;
                }
            }
            Err(-11) => {
                // No data — brief yield.
                thread::sleep(Duration::from_millis(5));
            }
            Err(e) => {
                let msg = format!("\r\ntelnet: recv error {e}\r\n");
                stderr_write(msg.as_bytes());
                break;
            }
        }

        // Check if the stdin thread signalled done.
        if done.load(Ordering::Relaxed) {
            break;
        }
    }

    if session.conn == ConnState::Connected {
        tcp_close(session.handle);
        session.conn = ConnState::Closed;
    }

    // Signal stdin thread to stop.
    done.store(true, Ordering::Relaxed);
    // We cannot join the stdin thread if it is blocked in stdin_read (the
    // syscall will not return until the user presses a key). Drop it and let
    // the process exit.
    drop(stdin_thread);

    Ok(())
}

// ============================================================================
// CLI parsing
// ============================================================================

struct CliOptions {
    host: String,
    port: u16,
    escape_char: u8,
    login_user: Option<String>,
    connect_timeout_ms: u64,
}

fn print_usage() {
    stderr_write(b"Usage: telnet [-e escape_char] [-l user] hostname [port]\r\n");
    stderr_write(b"\r\n");
    stderr_write(b"  hostname   Remote host to connect to\r\n");
    stderr_write(b"  port       TCP port (default: 23)\r\n");
    stderr_write(b"  -e char    Escape character (default: ^])\r\n");
    stderr_write(b"  -l user    Send username at login prompt\r\n");
    stderr_write(b"  -w secs    Connection timeout in seconds (default: 10)\r\n");
    stderr_write(b"  -h         Show this help\r\n");
}

fn parse_args() -> Result<CliOptions, String> {
    let argv: Vec<String> = env::args().collect();
    if argv.len() < 2 {
        return Err("too few arguments".to_string());
    }

    let mut escape_char: u8 = 0x1D; // Ctrl+]
    let mut login_user: Option<String> = None;
    let mut connect_timeout_ms: u64 = 10_000;
    let mut positionals: Vec<String> = Vec::new();

    let mut i = 1usize;
    while i < argv.len() {
        match argv[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-e" => {
                i = i.saturating_add(1);
                let val = argv
                    .get(i)
                    .ok_or_else(|| "-e requires an argument".to_string())?;
                escape_char = parse_escape_char(val)?;
            }
            "-l" => {
                i = i.saturating_add(1);
                let val = argv
                    .get(i)
                    .ok_or_else(|| "-l requires a username".to_string())?;
                login_user = Some(val.clone());
            }
            "-w" => {
                i = i.saturating_add(1);
                let val = argv
                    .get(i)
                    .ok_or_else(|| "-w requires a timeout in seconds".to_string())?;
                let secs: u64 = val
                    .parse()
                    .map_err(|_| format!("invalid timeout '{val}'"))?;
                connect_timeout_ms = secs.saturating_mul(1000);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option '{other}'"));
            }
            _ => {
                positionals.push(argv[i].clone());
            }
        }
        i = i.saturating_add(1);
    }

    if positionals.is_empty() {
        return Err("hostname is required".to_string());
    }
    let host = positionals[0].clone();
    let port = if positionals.len() >= 2 {
        positionals[1]
            .parse::<u16>()
            .map_err(|_| format!("invalid port '{}'", positionals[1]))?
    } else {
        23
    };

    if port == 0 {
        return Err("port must be 1–65535".to_string());
    }

    Ok(CliOptions {
        host,
        port,
        escape_char,
        login_user,
        connect_timeout_ms,
    })
}

/// Parse an escape character from a `-e` argument.
/// Accepts: `^X` (Ctrl+X), a single ASCII character, or a decimal/hex byte.
fn parse_escape_char(s: &str) -> Result<u8, String> {
    if s.starts_with('^') && s.len() == 2 {
        let ch = s.as_bytes()[1];
        if ch.is_ascii_alphabetic()
            || ch == b'['
            || ch == b'\\'
            || ch == b']'
            || ch == b'^'
            || ch == b'_'
        {
            let ctrl = ch.to_ascii_uppercase() & 0x1F;
            return Ok(ctrl);
        }
        return Err(format!("invalid control character '^{}'", ch as char));
    }
    if s.len() == 1 {
        return Ok(s.as_bytes()[0]);
    }
    // Try decimal.
    if let Ok(v) = s.parse::<u8>() {
        return Ok(v);
    }
    // Try 0xNN hex.
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u8::from_str_radix(hex, 16).map_err(|_| format!("invalid escape character '{s}'"));
    }
    Err(format!("invalid escape character '{s}'"))
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> Result<(), String> {
    let opts = parse_args()?;

    let mut session = Session::new(opts.host, opts.port, opts.escape_char, opts.login_user);

    run_session(&mut session, opts.connect_timeout_ms)
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(e) => {
            let msg = format!("telnet: {e}\r\n");
            stderr_write(msg.as_bytes());
            process::exit(1);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // IPv4 parsing
    // -------------------------------------------------------------------------

    #[test]
    fn parse_ipv4_loopback() {
        assert_eq!(parse_ipv4("127.0.0.1"), Some(0x7F00_0001));
    }

    #[test]
    fn parse_ipv4_broadcast() {
        assert_eq!(parse_ipv4("255.255.255.255"), Some(0xFFFF_FFFF));
    }

    #[test]
    fn parse_ipv4_zeros() {
        assert_eq!(parse_ipv4("0.0.0.0"), Some(0));
    }

    #[test]
    fn parse_ipv4_too_few_parts() {
        assert_eq!(parse_ipv4("192.168.1"), None);
    }

    #[test]
    fn parse_ipv4_too_many_parts() {
        assert_eq!(parse_ipv4("1.2.3.4.5"), None);
    }

    #[test]
    fn parse_ipv4_octet_overflow() {
        // 256 doesn't fit in u8.
        assert_eq!(parse_ipv4("256.0.0.1"), None);
    }

    #[test]
    fn parse_ipv4_non_numeric() {
        assert_eq!(parse_ipv4("abc.def.ghi.jkl"), None);
    }

    #[test]
    fn format_ipv4_roundtrip() {
        for s in ["10.0.0.1", "172.16.0.1", "8.8.8.8", "192.0.2.1"] {
            assert_eq!(format_ipv4(parse_ipv4(s).unwrap()), s);
        }
    }

    // -------------------------------------------------------------------------
    // Escape character parsing
    // -------------------------------------------------------------------------

    #[test]
    fn escape_char_ctrl_bracket() {
        assert_eq!(parse_escape_char("^]").unwrap(), 0x1D);
    }

    #[test]
    fn escape_char_ctrl_c() {
        assert_eq!(parse_escape_char("^C").unwrap(), 0x03);
    }

    #[test]
    fn escape_char_single_printable() {
        assert_eq!(parse_escape_char("~").unwrap(), b'~');
    }

    #[test]
    fn escape_char_hex() {
        assert_eq!(parse_escape_char("0x1d").unwrap(), 0x1D);
    }

    #[test]
    fn escape_char_decimal() {
        assert_eq!(parse_escape_char("29").unwrap(), 29);
    }

    #[test]
    fn escape_char_invalid() {
        assert!(parse_escape_char("toolong").is_err());
    }

    // -------------------------------------------------------------------------
    // Escape mode command parsing
    // -------------------------------------------------------------------------

    #[test]
    fn escape_cmd_quit() {
        assert_eq!(parse_escape_cmd("quit"), EscapeCmd::Quit);
        assert_eq!(parse_escape_cmd("q"), EscapeCmd::Quit);
        assert_eq!(parse_escape_cmd("exit"), EscapeCmd::Quit);
        assert_eq!(parse_escape_cmd("  QUIT  "), EscapeCmd::Quit);
    }

    #[test]
    fn escape_cmd_close() {
        assert_eq!(parse_escape_cmd("close"), EscapeCmd::Close);
    }

    #[test]
    fn escape_cmd_status() {
        assert_eq!(parse_escape_cmd("status"), EscapeCmd::Status);
    }

    #[test]
    fn escape_cmd_unknown() {
        match parse_escape_cmd("frobulate") {
            EscapeCmd::Unknown(s) => assert_eq!(s, "frobulate"),
            _ => panic!("expected Unknown"),
        }
    }

    // -------------------------------------------------------------------------
    // IAC builder helpers
    // -------------------------------------------------------------------------

    #[test]
    fn iac_will_sequence() {
        let mut out = Vec::new();
        iac_will(&mut out, OPT_ECHO);
        assert_eq!(out, [IAC, WILL, OPT_ECHO]);
    }

    #[test]
    fn iac_do_sequence() {
        let mut out = Vec::new();
        iac_do(&mut out, OPT_SGA);
        assert_eq!(out, [IAC, DO, OPT_SGA]);
    }

    #[test]
    fn iac_dont_sequence() {
        let mut out = Vec::new();
        iac_dont(&mut out, 99);
        assert_eq!(out, [IAC, DONT, 99]);
    }

    // -------------------------------------------------------------------------
    // Telnet parser: plain data pass-through
    // -------------------------------------------------------------------------

    #[test]
    fn process_plain_data() {
        let mut session = Session::new("host".into(), 23, 0x1D, None);
        let mut plain = Vec::new();
        let mut net_out = Vec::new();
        process_incoming(&mut session, b"Hello, world!\r\n", &mut plain, &mut net_out);
        assert_eq!(plain, b"Hello, world!\r\n");
        assert!(net_out.is_empty());
    }

    #[test]
    fn process_escaped_iac() {
        let mut session = Session::new("host".into(), 23, 0x1D, None);
        let mut plain = Vec::new();
        let mut net_out = Vec::new();
        // IAC IAC should produce a single 0xFF in plain data.
        process_incoming(&mut session, &[IAC, IAC], &mut plain, &mut net_out);
        assert_eq!(plain, &[0xFF]);
    }

    // -------------------------------------------------------------------------
    // Telnet parser: option negotiation
    // -------------------------------------------------------------------------

    #[test]
    fn process_server_will_echo_generates_do() {
        let mut session = Session::new("host".into(), 23, 0x1D, None);
        let mut plain = Vec::new();
        let mut net_out = Vec::new();
        // Server sends WILL ECHO.
        process_incoming(
            &mut session,
            &[IAC, WILL, OPT_ECHO],
            &mut plain,
            &mut net_out,
        );
        assert!(plain.is_empty());
        assert_eq!(net_out, [IAC, DO, OPT_ECHO]);
        assert_eq!(session.opts.remote[usize::from(OPT_ECHO)], OptState::Yes);
    }

    #[test]
    fn process_server_will_unknown_generates_dont() {
        let mut session = Session::new("host".into(), 23, 0x1D, None);
        let mut plain = Vec::new();
        let mut net_out = Vec::new();
        // Server sends WILL for option 99 (unknown).
        process_incoming(&mut session, &[IAC, WILL, 99], &mut plain, &mut net_out);
        assert_eq!(net_out, [IAC, DONT, 99]);
    }

    #[test]
    fn process_server_do_ttype_generates_will() {
        let mut session = Session::new("host".into(), 23, 0x1D, None);
        let mut plain = Vec::new();
        let mut net_out = Vec::new();
        process_incoming(
            &mut session,
            &[IAC, DO, OPT_TTYPE],
            &mut plain,
            &mut net_out,
        );
        assert_eq!(&net_out[..3], &[IAC, WILL, OPT_TTYPE]);
    }

    #[test]
    fn process_server_do_unknown_generates_wont() {
        let mut session = Session::new("host".into(), 23, 0x1D, None);
        let mut plain = Vec::new();
        let mut net_out = Vec::new();
        process_incoming(&mut session, &[IAC, DO, 77], &mut plain, &mut net_out);
        assert_eq!(net_out, [IAC, WONT, 77]);
    }

    // -------------------------------------------------------------------------
    // Telnet parser: subnegotiation
    // -------------------------------------------------------------------------

    #[test]
    fn process_ttype_send_subneg() {
        let mut session = Session::new("host".into(), 23, 0x1D, None);
        // Activate TTYPE first (as if the DO/WILL exchange already happened).
        session.opts.local[usize::from(OPT_TTYPE)] = OptState::Yes;
        let mut plain = Vec::new();
        let mut net_out = Vec::new();
        // Server sends IAC SB TTYPE SEND IAC SE.
        let sb = [IAC, SB, OPT_TTYPE, TTYPE_SEND, IAC, SE];
        process_incoming(&mut session, &sb, &mut plain, &mut net_out);
        assert!(plain.is_empty());
        // Response: IAC SB TTYPE IS "xterm-256color" IAC SE.
        assert_eq!(&net_out[..4], &[IAC, SB, OPT_TTYPE, TTYPE_IS]);
        assert!(net_out.ends_with(&[IAC, SE]));
        let term = b"xterm-256color";
        assert!(net_out[4..net_out.len() - 2] == *term);
    }

    // -------------------------------------------------------------------------
    // Data encoding (IAC escaping)
    // -------------------------------------------------------------------------

    #[test]
    fn encode_data_no_iac() {
        let mut out = Vec::new();
        encode_data(b"hello", &mut out);
        assert_eq!(out, b"hello");
    }

    #[test]
    fn encode_data_with_iac() {
        let mut out = Vec::new();
        encode_data(&[0x41, 0xFF, 0x42], &mut out);
        // 0xFF should be doubled.
        assert_eq!(out, &[0x41, 0xFF, 0xFF, 0x42]);
    }

    // -------------------------------------------------------------------------
    // NAWS byte escaping
    // -------------------------------------------------------------------------

    #[test]
    fn naws_byte_normal() {
        let mut out = Vec::new();
        push_naws_byte(&mut out, 80);
        assert_eq!(out, &[80]);
    }

    #[test]
    fn naws_byte_iac_doubled() {
        let mut out = Vec::new();
        push_naws_byte(&mut out, 0xFF);
        assert_eq!(out, &[0xFF, 0xFF]);
    }
}
