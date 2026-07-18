//! Telnet server for remote kernel shell access.
//!
//! Provides a minimal Telnet (RFC 854) server that exposes the kernel
//! debug shell (`kshell`) over a TCP connection.  This allows remote
//! debugging and management of the kernel from another machine on the
//! network — essential for headless systems or when the local console
//! is unavailable.
//!
//! ## Protocol
//!
//! Telnet uses TCP port 23 (configurable).  The protocol is simple:
//! text is transmitted as-is, with control sequences prefixed by
//! IAC (0xFF).  On connect the server negotiates WILL ECHO and
//! WILL/DO SUPPRESS-GO-AHEAD for line-at-a-time operation.
//!
//! ## Architecture
//!
//! ```text
//! Remote terminal ─── TCP:23 ──→ telnet server
//!                                   ├── accept new connections
//!                                   ├── read command lines
//!                                   ├── execute via kshell capture
//!                                   └── send output back
//! ```
//!
//! ## Security
//!
//! This is a **kernel debug facility** with no authentication.  It
//! should only be enabled on trusted networks.  The firewall can
//! restrict access by source IP.
//!
//! ## Limitations
//!
//! - Maximum 4 concurrent sessions.
//! - No authentication (trusted-network only).
//! - Line-at-a-time mode only (no character-at-a-time / raw mode).
//! - No terminal type negotiation (NAWS, TTYPE, etc.).
//! - Commands execute synchronously (one at a time per session).
//! - Maximum 512-byte command lines.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU16, Ordering};
use crate::sync::Mutex;

use crate::error::KernelResult;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default listening port.
const DEFAULT_PORT: u16 = 23;

/// Maximum concurrent telnet sessions.
const MAX_SESSIONS: usize = 4;

/// Maximum input line length (bytes).
const MAX_LINE_LEN: usize = 512;

/// Poll interval for accepting connections and reading data (ns).
/// 500ms — fast enough for interactive use without wasting cycles.
const TICK_INTERVAL_NS: u64 = 500_000_000;

// Telnet protocol bytes (RFC 854).
const IAC: u8 = 255;  // Interpret As Command
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;   // Sub-negotiation Begin
const SE: u8 = 240;   // Sub-negotiation End

// Telnet options.
const OPT_ECHO: u8 = 1;
const OPT_SUPPRESS_GA: u8 = 3;
const OPT_LINEMODE: u8 = 34;

/// Carriage return + line feed.
const CRLF: &[u8] = b"\r\n";

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// A single telnet session.
struct Session {
    /// TCP connection handle.
    tcp_handle: usize,
    /// Input line buffer being accumulated.
    line_buf: Vec<u8>,
    /// Whether we're in the middle of an IAC sequence.
    iac_state: IacState,
    /// Remote IP (for logging).
    remote_ip: super::interface::IpAddr,
    /// Remote port.
    remote_port: u16,
    /// Whether this session slot is active.
    active: bool,
    /// Total bytes received.
    bytes_rx: u64,
    /// Total bytes sent.
    bytes_tx: u64,
    /// Total commands executed.
    commands_run: u64,
    /// Connection timestamp (ns).
    connected_at_ns: u64,
}

/// IAC (telnet command) parsing state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IacState {
    /// Normal data mode.
    Normal,
    /// Received IAC byte, waiting for command.
    GotIac,
    /// Received IAC + WILL/WONT/DO/DONT, waiting for option byte.
    GotNegotiation(u8),
    /// Inside sub-negotiation (IAC SB ... IAC SE).
    SubNeg,
    /// Got IAC inside sub-negotiation (could be SE or escaped IAC).
    SubNegIac,
}

impl Session {
    const fn empty() -> Self {
        Self {
            tcp_handle: 0,
            line_buf: Vec::new(),
            iac_state: IacState::Normal,
            remote_ip: super::interface::IpAddr::V4(super::interface::Ipv4Addr([0, 0, 0, 0])),
            remote_port: 0,
            active: false,
            bytes_rx: 0,
            bytes_tx: 0,
            commands_run: 0,
            connected_at_ns: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct TelnetState {
    /// TCP listener handle.
    listener_handle: Option<usize>,
    /// Active sessions.
    sessions: [Session; MAX_SESSIONS],
}

impl TelnetState {
    const fn new() -> Self {
        Self {
            listener_handle: None,
            sessions: [
                Session::empty(),
                Session::empty(),
                Session::empty(),
                Session::empty(),
            ],
        }
    }
}

static STATE: Mutex<TelnetState> = Mutex::new(TelnetState::new());
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(false);
static LISTEN_PORT: AtomicU16 = AtomicU16::new(DEFAULT_PORT);
static LAST_TICK: AtomicU64 = AtomicU64::new(0);

// Statistics.
static TOTAL_CONNECTIONS: AtomicU64 = AtomicU64::new(0);
static TOTAL_COMMANDS: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES_TX: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES_RX: AtomicU64 = AtomicU64::new(0);
static REJECTED_CONNECTIONS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the telnet server.
///
/// Binds a TCP listener on the configured port (default 23) and begins
/// accepting connections.
pub fn init() -> KernelResult<()> {
    if INITIALIZED.load(Ordering::Relaxed) {
        return Ok(());
    }

    let port = LISTEN_PORT.load(Ordering::Relaxed);
    let listener = super::tcp::bind(crate::netns::ROOT_NS, port)?;

    let mut state = STATE.lock();
    state.listener_handle = Some(listener);

    INITIALIZED.store(true, Ordering::Relaxed);
    ENABLED.store(true, Ordering::Relaxed);

    crate::serial_println!("[telnet] Server listening on port {}", port);
    Ok(())
}

/// Shut down the telnet server.
///
/// Closes all active sessions and the listener.
pub fn shutdown() {
    ENABLED.store(false, Ordering::Relaxed);

    let mut state = STATE.lock();

    // Close all sessions.
    for session in &mut state.sessions {
        if session.active {
            let _ = super::tcp::close(session.tcp_handle);
            session.active = false;
        }
    }

    // Close listener.
    if let Some(listener) = state.listener_handle.take() {
        let _ = super::tcp::close_listener(listener);
    }

    INITIALIZED.store(false, Ordering::Relaxed);
    crate::serial_println!("[telnet] Server shut down");
}

/// Set the listening port (must be called before `init()`).
pub fn set_port(port: u16) {
    LISTEN_PORT.store(port, Ordering::Relaxed);
}

/// Check if the telnet server is running.
#[allow(dead_code)] // Public API.
pub fn is_running() -> bool {
    INITIALIZED.load(Ordering::Relaxed) && ENABLED.load(Ordering::Relaxed)
}

/// Enable or disable the telnet server.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    if !enabled {
        // Disconnect all sessions when disabling.
        let mut state = STATE.lock();
        for session in &mut state.sessions {
            if session.active {
                let _ = send_line(session.tcp_handle, "\r\n[telnet] Server shutting down. Goodbye.\r\n");
                let _ = super::tcp::close(session.tcp_handle);
                session.active = false;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Connection management
// ---------------------------------------------------------------------------

/// Send the initial telnet negotiation and welcome banner.
fn send_welcome(tcp_handle: usize) -> KernelResult<usize> {
    // Negotiate: server WILL ECHO, server WILL SUPPRESS-GO-AHEAD,
    // client DO SUPPRESS-GO-AHEAD, client DONT LINEMODE.
    let negotiate = [
        IAC, WILL, OPT_ECHO,
        IAC, WILL, OPT_SUPPRESS_GA,
        IAC, DO, OPT_SUPPRESS_GA,
        IAC, DONT, OPT_LINEMODE,
    ];
    let mut total = super::tcp::send(tcp_handle, &negotiate)?;

    // Welcome banner.
    let banner = "\r\n\
        ===========================================\r\n\
        Welcome to Neo Kernel Shell (telnet)\r\n\
        ===========================================\r\n\
        \r\n\
        Type 'help' for available commands.\r\n\
        Type 'exit' or 'logout' to disconnect.\r\n\
        \r\n";

    total = total.saturating_add(super::tcp::send(tcp_handle, banner.as_bytes())?);

    // Send initial prompt.
    total = total.saturating_add(super::tcp::send(tcp_handle, b"neo$ ")?);

    Ok(total)
}

/// Accept pending connections from the listener.
fn accept_connections(state: &mut TelnetState) {
    let listener = match state.listener_handle {
        Some(h) => h,
        None => return,
    };

    // Accept as many connections as we have free session slots.
    loop {
        if !super::tcp::listener_has_pending(listener) {
            break;
        }

        // Find a free session slot.
        let slot = state.sessions.iter().position(|s| !s.active);
        let idx = match slot {
            Some(i) => i,
            None => {
                // All slots full — accept and immediately reject.
                if let Ok(handle) = super::tcp::accept(listener) {
                    let _ = super::tcp::send(handle,
                        b"\r\nServer busy (max sessions reached). Try again later.\r\n");
                    let _ = super::tcp::close(handle);
                    REJECTED_CONNECTIONS.fetch_add(1, Ordering::Relaxed);
                }
                break;
            }
        };

        match super::tcp::accept(listener) {
            Ok(handle) => {
                let now = crate::hrtimer::now_ns();
                let info = super::tcp::connection_info(handle);
                let (remote_ip, remote_port) = match info {
                    Some(ci) => (ci.remote_ip, ci.remote_port),
                    None => (super::interface::IpAddr::V4(super::interface::Ipv4Addr([0, 0, 0, 0])), 0),
                };

                crate::serial_println!(
                    "[telnet] New connection from {}:{} (session #{})",
                    remote_ip, remote_port, idx
                );

                let session = &mut state.sessions[idx];
                session.tcp_handle = handle;
                session.line_buf = Vec::with_capacity(MAX_LINE_LEN);
                session.iac_state = IacState::Normal;
                session.remote_ip = remote_ip;
                session.remote_port = remote_port;
                session.active = true;
                session.bytes_rx = 0;
                session.bytes_tx = 0;
                session.commands_run = 0;
                session.connected_at_ns = now;

                // Send welcome.
                match send_welcome(handle) {
                    Ok(n) => {
                        session.bytes_tx = session.bytes_tx.saturating_add(n as u64);
                        TOTAL_BYTES_TX.fetch_add(n as u64, Ordering::Relaxed);
                    }
                    Err(_) => {
                        // Failed to send welcome — close immediately.
                        let _ = super::tcp::close(handle);
                        session.active = false;
                        continue;
                    }
                }

                TOTAL_CONNECTIONS.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => break,
        }
    }
}

/// Process data from all active sessions.
fn process_sessions(state: &mut TelnetState) {
    for idx in 0..MAX_SESSIONS {
        let session = &mut state.sessions[idx];
        if !session.active {
            continue;
        }

        // Read available data from this session's TCP connection.
        let data = match super::tcp::read_up_to(session.tcp_handle, 1024) {
            Ok(d) if !d.is_empty() => d,
            Ok(_) => continue, // No data available.
            Err(_) => {
                // Connection error or closed — clean up.
                crate::serial_println!(
                    "[telnet] Session #{} disconnected ({}:{})",
                    idx, session.remote_ip, session.remote_port
                );
                let _ = super::tcp::close(session.tcp_handle);
                session.active = false;
                continue;
            }
        };

        session.bytes_rx = session.bytes_rx.saturating_add(data.len() as u64);
        TOTAL_BYTES_RX.fetch_add(data.len() as u64, Ordering::Relaxed);

        // Process each byte through the telnet state machine.
        let mut lines_to_execute: Vec<String> = Vec::new();

        for &byte in &data {
            match session.iac_state {
                IacState::Normal => {
                    if byte == IAC {
                        session.iac_state = IacState::GotIac;
                    } else if byte == b'\r' {
                        // CR — will be followed by LF or NUL; line is complete.
                        if let Ok(line) = core::str::from_utf8(&session.line_buf) {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                lines_to_execute.push(String::from(trimmed));
                            } else {
                                // Empty line — just re-prompt.
                                lines_to_execute.push(String::new());
                            }
                        }
                        session.line_buf.clear();
                    } else if byte == b'\n' {
                        // LF — after CR this is a no-op (line_buf was cleared).
                        // For bare-LF clients (no preceding CR), treat as line
                        // terminator so commands aren't silently dropped.
                        if !session.line_buf.is_empty() {
                            if let Ok(line) = core::str::from_utf8(&session.line_buf) {
                                let trimmed = line.trim();
                                if !trimmed.is_empty() {
                                    lines_to_execute.push(String::from(trimmed));
                                } else {
                                    lines_to_execute.push(String::new());
                                }
                            }
                            session.line_buf.clear();
                        }
                    } else if byte == 0 {
                        // NUL after CR — ignore.
                    } else if byte == 127 || byte == 8 {
                        // Backspace / DEL — remove last character.
                        session.line_buf.pop();
                    } else if byte == 3 {
                        // Ctrl-C — clear line.
                        session.line_buf.clear();
                        lines_to_execute.push(String::new()); // Re-prompt.
                    } else if byte >= 32 {
                        // Printable character.
                        if session.line_buf.len() < MAX_LINE_LEN {
                            session.line_buf.push(byte);
                        }
                    }
                    // Control characters < 32 (except the ones above) are ignored.
                }
                IacState::GotIac => {
                    match byte {
                        WILL | WONT | DO | DONT => {
                            session.iac_state = IacState::GotNegotiation(byte);
                        }
                        SB => {
                            session.iac_state = IacState::SubNeg;
                        }
                        IAC => {
                            // Escaped 0xFF — literal data byte.
                            if session.line_buf.len() < MAX_LINE_LEN {
                                session.line_buf.push(IAC);
                            }
                            session.iac_state = IacState::Normal;
                        }
                        _ => {
                            // Other IAC commands (IP, AO, AYT, etc.) — ignore.
                            session.iac_state = IacState::Normal;
                        }
                    }
                }
                IacState::GotNegotiation(cmd) => {
                    // Received a complete negotiation: IAC cmd option.
                    // We just ignore most options.  Reply WONT/DONT to
                    // anything we didn't initiate.
                    handle_negotiation(session.tcp_handle, cmd, byte);
                    session.iac_state = IacState::Normal;
                }
                IacState::SubNeg => {
                    if byte == IAC {
                        session.iac_state = IacState::SubNegIac;
                    }
                    // Ignore sub-negotiation content.
                }
                IacState::SubNegIac => {
                    if byte == SE {
                        // End of sub-negotiation.
                        session.iac_state = IacState::Normal;
                    } else {
                        // Escaped IAC inside sub-negotiation, or
                        // continuation — stay in SubNeg.
                        session.iac_state = IacState::SubNeg;
                    }
                }
            }
        }

        // Execute accumulated command lines.
        let tcp_handle = session.tcp_handle;
        for line in &lines_to_execute {
            if line.is_empty() {
                // Empty line — just send prompt.
                let n = send_bytes(tcp_handle, b"neo$ ");
                session.bytes_tx = session.bytes_tx.saturating_add(n as u64);
                TOTAL_BYTES_TX.fetch_add(n as u64, Ordering::Relaxed);
                continue;
            }

            // Check for disconnect commands.
            let trimmed = line.trim();
            if trimmed == "exit" || trimmed == "logout" || trimmed == "quit" || trimmed == "disconnect" {
                let _ = send_line(tcp_handle, "\r\nGoodbye.\r\n");
                let _ = super::tcp::close(tcp_handle);
                session.active = false;
                crate::serial_println!(
                    "[telnet] Session #{} logged out ({}:{})",
                    idx, session.remote_ip, session.remote_port
                );
                break;
            }

            // Don't allow reboot via telnet (safety).
            if trimmed == "reboot" || trimmed == "shutdown" || trimmed == "poweroff" {
                let n = send_line(tcp_handle,
                    "Reboot/shutdown not permitted via telnet.\r\n");
                session.bytes_tx = session.bytes_tx.saturating_add(n as u64);
                TOTAL_BYTES_TX.fetch_add(n as u64, Ordering::Relaxed);
                let n2 = send_bytes(tcp_handle, b"neo$ ");
                session.bytes_tx = session.bytes_tx.saturating_add(n2 as u64);
                TOTAL_BYTES_TX.fetch_add(n2 as u64, Ordering::Relaxed);
                continue;
            }

            // Execute via kshell capture.
            let output = crate::kshell::capture_command(trimmed);

            // Convert LF to CR+LF for telnet.
            let telnet_output = lf_to_crlf(&output);

            let mut n = send_bytes(tcp_handle, telnet_output.as_bytes());
            // Ensure output ends with newline.
            if !telnet_output.ends_with("\r\n") && !telnet_output.is_empty() {
                n = n.saturating_add(send_bytes(tcp_handle, CRLF));
            }
            // Send prompt.
            n = n.saturating_add(send_bytes(tcp_handle, b"neo$ "));

            session.bytes_tx = session.bytes_tx.saturating_add(n as u64);
            TOTAL_BYTES_TX.fetch_add(n as u64, Ordering::Relaxed);
            session.commands_run = session.commands_run.saturating_add(1);
            TOTAL_COMMANDS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Handle a telnet negotiation command.
fn handle_negotiation(tcp_handle: usize, cmd: u8, option: u8) {
    // We only support ECHO and SUPPRESS-GO-AHEAD.
    // For anything else, reply with refusal.
    match cmd {
        DO => {
            match option {
                OPT_ECHO | OPT_SUPPRESS_GA => {
                    // We already sent WILL for these — no need to reply.
                }
                _ => {
                    // Refuse: IAC WONT option.
                    let reply = [IAC, WONT, option];
                    let _ = super::tcp::send(tcp_handle, &reply);
                }
            }
        }
        WILL => {
            match option {
                OPT_SUPPRESS_GA => {
                    // Client will suppress GA — good.
                }
                _ => {
                    // Refuse: IAC DONT option.
                    let reply = [IAC, DONT, option];
                    let _ = super::tcp::send(tcp_handle, &reply);
                }
            }
        }
        DONT | WONT => {
            // Client refuses something — that's fine, just ignore.
        }
        _ => {}
    }
}

/// Send bytes over a TCP handle, returning bytes sent.
fn send_bytes(tcp_handle: usize, data: &[u8]) -> usize {
    super::tcp::send(tcp_handle, data).unwrap_or_default()
}

/// Send a text line over TCP, returning bytes sent.
fn send_line(tcp_handle: usize, text: &str) -> usize {
    send_bytes(tcp_handle, text.as_bytes())
}

/// Convert LF line endings to CR+LF for telnet protocol.
fn lf_to_crlf(input: &str) -> String {
    let mut out = String::with_capacity(input.len().saturating_add(input.len() / 10));
    let mut prev_cr = false;

    for ch in input.chars() {
        if ch == '\n' && !prev_cr {
            out.push('\r');
            out.push('\n');
        } else {
            out.push(ch);
        }
        prev_cr = ch == '\r';
    }

    out
}

// ---------------------------------------------------------------------------
// Periodic tick
// ---------------------------------------------------------------------------

/// Periodic tick — accept connections and process session I/O.
///
/// Called from `net::poll()`.  Rate-limited to avoid busy-looping.
pub fn tick() {
    if !INITIALIZED.load(Ordering::Relaxed) || !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let now = crate::hrtimer::now_ns();
    let last = LAST_TICK.load(Ordering::Relaxed);
    if now.saturating_sub(last) < TICK_INTERVAL_NS {
        return;
    }
    LAST_TICK.store(now, Ordering::Relaxed);

    let mut state = STATE.lock();
    accept_connections(&mut state);
    process_sessions(&mut state);
}

// ---------------------------------------------------------------------------
// Statistics and diagnostics
// ---------------------------------------------------------------------------

/// Telnet server statistics.
#[derive(Debug)]
pub struct TelnetStats {
    pub initialized: bool,
    pub enabled: bool,
    pub port: u16,
    pub active_sessions: usize,
    pub total_connections: u64,
    pub total_commands: u64,
    pub total_bytes_tx: u64,
    pub total_bytes_rx: u64,
    pub rejected_connections: u64,
}

/// Session information for diagnostics.
#[derive(Debug)]
pub struct SessionInfo {
    pub index: usize,
    pub remote_ip: super::interface::IpAddr,
    pub remote_port: u16,
    pub commands_run: u64,
    pub bytes_tx: u64,
    pub bytes_rx: u64,
    pub connected_secs: u64,
}

/// Get telnet server statistics.
pub fn stats() -> TelnetStats {
    let state = STATE.lock();
    TelnetStats {
        initialized: INITIALIZED.load(Ordering::Relaxed),
        enabled: ENABLED.load(Ordering::Relaxed),
        port: LISTEN_PORT.load(Ordering::Relaxed),
        active_sessions: state.sessions.iter().filter(|s| s.active).count(),
        total_connections: TOTAL_CONNECTIONS.load(Ordering::Relaxed),
        total_commands: TOTAL_COMMANDS.load(Ordering::Relaxed),
        total_bytes_tx: TOTAL_BYTES_TX.load(Ordering::Relaxed),
        total_bytes_rx: TOTAL_BYTES_RX.load(Ordering::Relaxed),
        rejected_connections: REJECTED_CONNECTIONS.load(Ordering::Relaxed),
    }
}

/// Get information about active sessions.
pub fn active_sessions() -> Vec<SessionInfo> {
    let state = STATE.lock();
    let now = crate::hrtimer::now_ns();
    let mut result = Vec::new();

    for (i, session) in state.sessions.iter().enumerate() {
        if session.active {
            result.push(SessionInfo {
                index: i,
                remote_ip: session.remote_ip,
                remote_port: session.remote_port,
                commands_run: session.commands_run,
                bytes_tx: session.bytes_tx,
                bytes_rx: session.bytes_rx,
                connected_secs: now.saturating_sub(session.connected_at_ns) / 1_000_000_000,
            });
        }
    }

    result
}

/// Disconnect a specific session by index.
pub fn disconnect_session(index: usize) -> bool {
    let mut state = STATE.lock();
    if let Some(session) = state.sessions.get_mut(index) {
        if session.active {
            let _ = send_line(session.tcp_handle, "\r\n[telnet] Disconnected by admin.\r\n");
            let _ = super::tcp::close(session.tcp_handle);
            session.active = false;
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Procfs
// ---------------------------------------------------------------------------

/// Generate procfs content for `/proc/telnet`.
pub fn procfs_content() -> String {
    let s = stats();
    let sessions = active_sessions();

    let mut out = String::with_capacity(512);
    out.push_str("Telnet Server\n");
    out.push_str("=============\n\n");

    out.push_str(&format!("Status:       {}\n",
        if s.initialized {
            if s.enabled { "running" } else { "disabled" }
        } else { "not initialized" }
    ));
    out.push_str(&format!("Port:         {}\n", s.port));
    out.push_str(&format!("Sessions:     {}/{}\n", s.active_sessions, MAX_SESSIONS));
    out.push_str(&format!("Connections:  {} total ({} rejected)\n",
        s.total_connections, s.rejected_connections));
    out.push_str(&format!("Commands:     {} total\n", s.total_commands));
    out.push_str(&format!("Traffic:      {} TX, {} RX bytes\n",
        s.total_bytes_tx, s.total_bytes_rx));

    if !sessions.is_empty() {
        out.push_str("\nActive Sessions:\n");
        for si in &sessions {
            out.push_str(&format!(
                "  #{}: {}:{} ({}s, {} cmds, {} TX / {} RX)\n",
                si.index, si.remote_ip, si.remote_port,
                si.connected_secs, si.commands_run,
                si.bytes_tx, si.bytes_rx,
            ));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run telnet server self-tests.
// Self-tests deliberately runtime-assert telnet protocol constants
// (IAC byte, command codes, port number) as living documentation.
#[allow(clippy::assertions_on_constants)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[telnet] Running telnet server self-tests...");
    let mut passed = 0u32;

    // --- Test 1: LF to CRLF conversion ---
    {
        let input = "hello\nworld\n";
        let output = lf_to_crlf(input);
        assert!(output == "hello\r\nworld\r\n", "LF→CRLF basic");

        // Already-CRLF input should not be doubled.
        let input2 = "hello\r\nworld\r\n";
        let output2 = lf_to_crlf(input2);
        assert!(output2 == "hello\r\nworld\r\n", "LF→CRLF no double");

        passed = passed.saturating_add(1);
        crate::serial_println!("[telnet]   test 1 (LF→CRLF conversion) PASSED");
    }

    // --- Test 2: Empty input ---
    {
        let output = lf_to_crlf("");
        assert!(output.is_empty(), "empty input");

        passed = passed.saturating_add(1);
        crate::serial_println!("[telnet]   test 2 (empty LF→CRLF) PASSED");
    }

    // --- Test 3: No LF in input ---
    {
        let output = lf_to_crlf("hello world");
        assert!(output == "hello world", "no LF");

        passed = passed.saturating_add(1);
        crate::serial_println!("[telnet]   test 3 (no-LF passthrough) PASSED");
    }

    // --- Test 4: IAC state machine transitions ---
    {
        // Verify initial state is Normal.
        let s = Session::empty();
        assert!(s.iac_state == IacState::Normal, "initial state Normal");

        passed = passed.saturating_add(1);
        crate::serial_println!("[telnet]   test 4 (IAC initial state) PASSED");
    }

    // --- Test 5: Session slot management ---
    {
        let state = TelnetState::new();
        // All sessions should be inactive initially.
        let active = state.sessions.iter().filter(|s| s.active).count();
        assert!(active == 0, "no active sessions initially");

        // Should have MAX_SESSIONS slots.
        assert!(state.sessions.len() == MAX_SESSIONS, "session count");

        passed = passed.saturating_add(1);
        crate::serial_println!("[telnet]   test 5 (session slot management) PASSED");
    }

    // --- Test 6: Stats with no server ---
    {
        let s = stats();
        // Stats should work even when server hasn't been initialized.
        assert!(s.port == DEFAULT_PORT || s.port > 0, "port valid");
        // active_sessions should be >= 0 (it's usize, so always true, but
        // verify the function doesn't panic).
        let _ = s.active_sessions;

        passed = passed.saturating_add(1);
        crate::serial_println!("[telnet]   test 6 (stats without init) PASSED");
    }

    // --- Test 7: Mixed CRLF and LF ---
    {
        let input = "line1\r\nline2\nline3\r\nline4\n";
        let output = lf_to_crlf(input);
        assert!(output == "line1\r\nline2\r\nline3\r\nline4\r\n",
            "mixed CRLF/LF normalization");

        passed = passed.saturating_add(1);
        crate::serial_println!("[telnet]   test 7 (mixed CRLF/LF) PASSED");
    }

    // --- Test 8: Consecutive LFs ---
    {
        let input = "\n\n\n";
        let output = lf_to_crlf(input);
        assert!(output == "\r\n\r\n\r\n", "consecutive LFs");

        passed = passed.saturating_add(1);
        crate::serial_println!("[telnet]   test 8 (consecutive LFs) PASSED");
    }

    // --- Test 9: Protocol constants ---
    {
        assert!(IAC == 0xFF, "IAC = 0xFF");
        assert!(WILL == 0xFB, "WILL = 0xFB");
        assert!(WONT == 0xFC, "WONT = 0xFC");
        assert!(DO == 0xFD, "DO = 0xFD");
        assert!(DONT == 0xFE, "DONT = 0xFE");
        assert!(SB == 0xFA, "SB = 0xFA");
        assert!(SE == 0xF0, "SE = 0xF0");

        passed = passed.saturating_add(1);
        crate::serial_println!("[telnet]   test 9 (protocol constants) PASSED");
    }

    // --- Test 10: Option constants ---
    {
        assert!(OPT_ECHO == 1, "ECHO = 1");
        assert!(OPT_SUPPRESS_GA == 3, "SGA = 3");
        assert!(OPT_LINEMODE == 34, "LINEMODE = 34");

        passed = passed.saturating_add(1);
        crate::serial_println!("[telnet]   test 10 (option constants) PASSED");
    }

    crate::serial_println!("[telnet] All {} self-tests PASSED", passed);
    Ok(())
}
