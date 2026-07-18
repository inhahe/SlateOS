//! Network syslog client and receiver (RFC 5424 / RFC 3164).
//!
//! Provides two complementary capabilities:
//!
//! - **Syslog client**: forward kernel log messages to a remote syslog
//!   server over UDP (RFC 5424 format).  Enables centralized logging
//!   for headless systems and fleet management.
//!
//! - **Syslog receiver**: accept incoming syslog messages from other
//!   machines on the network.  Stores them in a ring buffer for display
//!   and optional forwarding to the kernel log.
//!
//! ## Protocol
//!
//! Uses UDP port 514 (configurable).  Messages follow RFC 5424 structured
//! format or the legacy BSD format (RFC 3164) for interoperability.
//!
//! ## Message format (RFC 5424)
//!
//! ```text
//! <PRI>VERSION TIMESTAMP HOSTNAME APP-NAME PROCID MSGID MSG
//! ```
//!
//! Priority = facility × 8 + severity.
//!
//! ## Architecture
//!
//! ```text
//! kernel syslog! macro ──→ syslog::forward_to_remote()
//!                            ↓
//!                     UDP 514 → remote server
//!
//! remote host ──→ UDP 514 → syslog::tick() → ring buffer + serial
//! ```
//!
//! ## Limitations
//!
//! - UDP only (no TCP/TLS syslog transport).
//! - Ring buffer holds 64 messages maximum.
//! - No structured data elements (STRUCTURED-DATA = "-").
//!
//! ## IPv6 support
//!
//! Log forwarding can use an IPv6 remote server via
//! `set_remote_server_v6()`.  The receiver also accepts incoming IPv6
//! syslog messages when the receiver is running (same UDP port).

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU16, Ordering};
use crate::sync::Mutex;

use crate::error::KernelResult;
use super::interface::Ipv4Addr;
use super::ipv6::Ipv6Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default syslog UDP port.
const DEFAULT_PORT: u16 = 514;

/// Ring buffer capacity for received messages.
const RING_BUFFER_SIZE: usize = 64;

/// Maximum syslog message length (bytes).
#[allow(dead_code)] // Protocol constant.
const MAX_MSG_LEN: usize = 1024;

/// Syslog version (RFC 5424).
const SYSLOG_VERSION: u8 = 1;

/// Tick interval for receiver processing (ns) — 1 second.
const TICK_INTERVAL_NS: u64 = 1_000_000_000;

// ---------------------------------------------------------------------------
// Facility codes (RFC 5424 §6.2.1)
// ---------------------------------------------------------------------------

/// Syslog facility codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Facility {
    Kern = 0,
    User = 1,
    Mail = 2,
    Daemon = 3,
    Auth = 4,
    Syslog = 5,
    Lpr = 6,
    News = 7,
    Uucp = 8,
    Cron = 9,
    Authpriv = 10,
    Ftp = 11,
    Local0 = 16,
    Local1 = 17,
    Local2 = 18,
    Local3 = 19,
    Local4 = 20,
    Local5 = 21,
    Local6 = 22,
    Local7 = 23,
}

impl Facility {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Kern),
            1 => Some(Self::User),
            2 => Some(Self::Mail),
            3 => Some(Self::Daemon),
            4 => Some(Self::Auth),
            5 => Some(Self::Syslog),
            6 => Some(Self::Lpr),
            7 => Some(Self::News),
            8 => Some(Self::Uucp),
            9 => Some(Self::Cron),
            10 => Some(Self::Authpriv),
            11 => Some(Self::Ftp),
            16 => Some(Self::Local0),
            17 => Some(Self::Local1),
            18 => Some(Self::Local2),
            19 => Some(Self::Local3),
            20 => Some(Self::Local4),
            21 => Some(Self::Local5),
            22 => Some(Self::Local6),
            23 => Some(Self::Local7),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Kern => "kern",
            Self::User => "user",
            Self::Mail => "mail",
            Self::Daemon => "daemon",
            Self::Auth => "auth",
            Self::Syslog => "syslog",
            Self::Lpr => "lpr",
            Self::News => "news",
            Self::Uucp => "uucp",
            Self::Cron => "cron",
            Self::Authpriv => "authpriv",
            Self::Ftp => "ftp",
            Self::Local0 => "local0",
            Self::Local1 => "local1",
            Self::Local2 => "local2",
            Self::Local3 => "local3",
            Self::Local4 => "local4",
            Self::Local5 => "local5",
            Self::Local6 => "local6",
            Self::Local7 => "local7",
        }
    }
}

// ---------------------------------------------------------------------------
// Severity codes (RFC 5424 §6.2.1)
// ---------------------------------------------------------------------------

/// Syslog severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Severity {
    Emergency = 0,
    Alert = 1,
    Critical = 2,
    Error = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}

impl Severity {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Emergency),
            1 => Some(Self::Alert),
            2 => Some(Self::Critical),
            3 => Some(Self::Error),
            4 => Some(Self::Warning),
            5 => Some(Self::Notice),
            6 => Some(Self::Info),
            7 => Some(Self::Debug),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Emergency => "EMERG",
            Self::Alert => "ALERT",
            Self::Critical => "CRIT",
            Self::Error => "ERR",
            Self::Warning => "WARNING",
            Self::Notice => "NOTICE",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
        }
    }
}

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// A parsed syslog message.
#[derive(Debug, Clone)]
pub struct SyslogMessage {
    /// Priority value (facility * 8 + severity).
    pub priority: u8,
    /// Facility.
    pub facility: Facility,
    /// Severity.
    pub severity: Severity,
    /// Hostname of the sender.
    pub hostname: String,
    /// Application name.
    pub app_name: String,
    /// Message text.
    pub message: String,
    /// Source IP (for received IPv4 messages).
    #[allow(dead_code)] // Kept for backward compat; source_addr preferred.
    pub source_ip: Ipv4Addr,
    /// Source address as string (supports both IPv4 and IPv6).
    pub source_addr: String,
    /// Timestamp (kernel ns) when received.
    #[allow(dead_code)] // Public API field.
    pub received_at_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct SyslogState {
    /// UDP socket handle for receiving.
    recv_handle: Option<usize>,
    /// Ring buffer of received messages.
    ring: Vec<SyslogMessage>,
    /// Write index into ring buffer.
    ring_write: usize,
    /// Number of messages in ring (up to RING_BUFFER_SIZE).
    ring_count: usize,
    /// Remote syslog server for forwarding (IPv4).
    remote_server: Option<Ipv4Addr>,
    /// Remote syslog server for forwarding (IPv6).
    remote_server_v6: Option<Ipv6Addr>,
    /// Remote server port.
    remote_port: u16,
    /// Our hostname for outgoing messages.
    hostname: String,
}

impl SyslogState {
    const fn new() -> Self {
        Self {
            recv_handle: None,
            ring: Vec::new(),
            ring_write: 0,
            ring_count: 0,
            remote_server: None,
            remote_server_v6: None,
            remote_port: DEFAULT_PORT,
            hostname: String::new(),
        }
    }
}

static STATE: Mutex<SyslogState> = Mutex::new(SyslogState::new());
static RECEIVER_ENABLED: AtomicBool = AtomicBool::new(false);
static FORWARDER_ENABLED: AtomicBool = AtomicBool::new(false);
static LISTEN_PORT: AtomicU16 = AtomicU16::new(DEFAULT_PORT);
static LAST_TICK: AtomicU64 = AtomicU64::new(0);

// Statistics.
static MESSAGES_RECEIVED: AtomicU64 = AtomicU64::new(0);
static MESSAGES_FORWARDED: AtomicU64 = AtomicU64::new(0);
static MESSAGES_DROPPED: AtomicU64 = AtomicU64::new(0);
static PARSE_ERRORS: AtomicU64 = AtomicU64::new(0);
static FORWARD_ERRORS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Message construction (RFC 5424)
// ---------------------------------------------------------------------------

/// Compute the PRI value from facility and severity.
fn compute_pri(facility: Facility, severity: Severity) -> u8 {
    ((facility as u8) << 3) | (severity as u8)
}

/// Extract facility and severity from a PRI value.
fn decode_pri(pri: u8) -> (Option<Facility>, Option<Severity>) {
    let facility = Facility::from_u8(pri >> 3);
    let severity = Severity::from_u8(pri & 0x07);
    (facility, severity)
}

/// Build a syslog message in RFC 5424 format.
///
/// Format: `<PRI>VERSION TIMESTAMP HOSTNAME APP-NAME PROCID MSGID MSG`
fn build_message(
    facility: Facility,
    severity: Severity,
    hostname: &str,
    app_name: &str,
    message: &str,
) -> String {
    let pri = compute_pri(facility, severity);

    // Timestamp: use kernel wall clock.
    let now_ns = crate::timekeeping::clock_realtime();
    let secs = now_ns / 1_000_000_000;
    // Simple ISO 8601 approximation from Unix timestamp.
    let timestamp = format_timestamp(secs);

    format!(
        "<{}>{}  {} {} {} - - {}",
        pri, SYSLOG_VERSION, timestamp, hostname, app_name, message
    )
}

/// Format a Unix timestamp as ISO 8601 (simplified).
///
/// Returns something like "2024-01-15T10:30:45Z".
/// This is an approximation — doesn't account for leap seconds.
fn format_timestamp(unix_secs: u64) -> String {
    // Days since Unix epoch.
    let days = unix_secs / 86400;
    let time_of_day = unix_secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Convert days to year/month/day using a simplified algorithm.
    let (year, month, day) = days_to_date(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to (year, month, day).
///
/// Uses the civil calendar algorithm from Howard Hinnant.
fn days_to_date(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days.saturating_add(719468);
    let era = z / 146097;
    let doe = z.saturating_sub(era.saturating_mul(146097)); // day of era
    let yoe = (doe.saturating_sub(doe / 1460).saturating_add(doe / 36524)
        .saturating_sub(doe / 146096)) / 365;
    let y = yoe.saturating_add(era.saturating_mul(400));
    let doy = doe.saturating_sub(
        365u64.saturating_mul(yoe)
            .saturating_add(yoe / 4)
            .saturating_sub(yoe / 100)
    );
    let mp = (5u64.saturating_mul(doy).saturating_add(2)) / 153;
    let d = doy.saturating_sub(
        (153u64.saturating_mul(mp).saturating_add(2)) / 5
    ).saturating_add(1);
    let m = if mp < 10 { mp.saturating_add(3) } else { mp.saturating_sub(9) };
    let y = if m <= 2 { y.saturating_add(1) } else { y };

    (y, m, d)
}

// ---------------------------------------------------------------------------
// Message parsing (RFC 3164 / RFC 5424)
// ---------------------------------------------------------------------------

/// Parse a syslog message from raw bytes.
///
/// Supports both RFC 5424 and legacy BSD (RFC 3164) format.
fn parse_message(data: &[u8], source_ip: Ipv4Addr) -> Option<SyslogMessage> {
    let text = core::str::from_utf8(data).ok()?;
    let now = crate::hrtimer::now_ns();

    // Must start with '<' PRI '>'.
    if !text.starts_with('<') {
        return None;
    }

    let end_pri = text.find('>')?;
    let pri_str = text.get(1..end_pri)?;
    let pri: u8 = pri_str.parse().ok()?;
    // RFC 5424: valid PRI is 0-191 (facility 0-23, severity 0-7).
    // Reject out-of-range PRI rather than silently misclassifying.
    let (facility_opt, severity_opt) = decode_pri(pri);
    let facility = facility_opt?;
    let severity = severity_opt?;

    // Rest of message after PRI.
    let rest = text.get(end_pri.saturating_add(1)..)?;

    // Try to extract hostname and message.
    // RFC 5424: <PRI>VERSION SP TIMESTAMP SP HOSTNAME SP APP-NAME SP ...
    // RFC 3164: <PRI>TIMESTAMP SP HOSTNAME SP MSG
    // Simple approach: split on spaces, extract what we can.
    let parts: Vec<&str> = rest.splitn(5, ' ').collect();

    // Build owned strings for each field.  The minimal-format branch
    // produces a `String` from `join()`, so we convert everything to
    // owned `String` to avoid a lifetime workaround (previous code used
    // `.leak()` which leaked memory on every received message).
    let (hostname, app_name, message) = if parts.len() >= 4 {
        // Likely RFC 5424 or structured format.
        let hostname = parts.get(2).copied().unwrap_or("-");
        let app_name = parts.get(3).copied().unwrap_or("-");
        let msg = parts.get(4).copied().unwrap_or("");
        (String::from(hostname), String::from(app_name), String::from(msg))
    } else if parts.len() >= 2 {
        // Minimal format.
        let hostname = parts.first().copied().unwrap_or("-");
        let msg = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
        (String::from(hostname), String::from("-"), msg)
    } else {
        (String::from("-"), String::from("-"), String::from(rest))
    };

    Some(SyslogMessage {
        priority: pri,
        facility,
        severity,
        hostname,
        app_name,
        message,
        source_addr: format!("{}", source_ip),
        source_ip,
        received_at_ns: now,
    })
}

/// Parse an incoming IPv6 syslog message into a [`SyslogMessage`].
///
/// Same as [`parse_message`] but the source is an IPv6 address.
/// The `source_ip` field is set to `UNSPECIFIED` (IPv4 0.0.0.0) since
/// the real source is IPv6; use `source_addr` for display.
fn parse_message_v6(data: &[u8], source_ip: Ipv6Addr) -> Option<SyslogMessage> {
    let text = core::str::from_utf8(data).ok()?;
    let now = crate::hrtimer::now_ns();

    if !text.starts_with('<') {
        return None;
    }

    let end_pri = text.find('>')?;
    let pri_str = text.get(1..end_pri)?;
    let pri: u8 = pri_str.parse().ok()?;
    let (facility_opt, severity_opt) = decode_pri(pri);
    let facility = facility_opt?;
    let severity = severity_opt?;

    let rest = text.get(end_pri.saturating_add(1)..)?;
    let parts: Vec<&str> = rest.splitn(5, ' ').collect();

    let (hostname, app_name, message) = if parts.len() >= 4 {
        let hostname = parts.get(2).copied().unwrap_or("-");
        let app_name = parts.get(3).copied().unwrap_or("-");
        let msg = parts.get(4).copied().unwrap_or("");
        (String::from(hostname), String::from(app_name), String::from(msg))
    } else if parts.len() >= 2 {
        let hostname = parts.first().copied().unwrap_or("-");
        let msg = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
        (String::from(hostname), String::from("-"), msg)
    } else {
        (String::from("-"), String::from("-"), String::from(rest))
    };

    Some(SyslogMessage {
        priority: pri,
        facility,
        severity,
        hostname,
        app_name,
        message,
        source_addr: format!("{}", source_ip),
        source_ip: Ipv4Addr::UNSPECIFIED,
        received_at_ns: now,
    })
}

// ---------------------------------------------------------------------------
// Client API (forwarding)
// ---------------------------------------------------------------------------

/// Configure the remote syslog server for log forwarding.
pub fn set_remote_server(ip: Ipv4Addr, port: u16) {
    let mut state = STATE.lock();
    state.remote_server = Some(ip);
    state.remote_port = port;
    FORWARDER_ENABLED.store(true, Ordering::Relaxed);
    crate::serial_println!("[syslog] Forwarding to {}:{}", ip, port);
}

/// Configure an IPv6 remote syslog server for log forwarding.
///
/// When both IPv4 and IPv6 servers are configured, the forwarder
/// sends to the IPv6 server.  To use IPv4, clear the IPv6 server
/// by calling `disable_forwarding()` and reconfiguring IPv4 only.
pub fn set_remote_server_v6(ip: Ipv6Addr, port: u16) {
    let mut state = STATE.lock();
    state.remote_server_v6 = Some(ip);
    state.remote_port = port;
    FORWARDER_ENABLED.store(true, Ordering::Relaxed);
    crate::serial_println!("[syslog] Forwarding to [{}]:{}", ip, port);
}

/// Disable log forwarding.
pub fn disable_forwarding() {
    FORWARDER_ENABLED.store(false, Ordering::Relaxed);
    let mut state = STATE.lock();
    state.remote_server = None;
    state.remote_server_v6 = None;
}

/// Set the local hostname for outgoing messages.
pub fn set_hostname(name: &str) {
    STATE.lock().hostname = String::from(name);
}

/// Forward a log message to the configured remote syslog server.
///
/// This is called by the kernel logging infrastructure to send
/// messages to a remote collector.
pub fn forward(facility: Facility, severity: Severity, app_name: &str, message: &str) {
    if !FORWARDER_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let state = STATE.lock();
    let server_v4 = state.remote_server;
    let server_v6 = state.remote_server_v6;
    let server_port = state.remote_port;
    let hostname = if state.hostname.is_empty() {
        String::from("neo")
    } else {
        state.hostname.clone()
    };
    drop(state);

    if server_v4.is_none() && server_v6.is_none() {
        return;
    }

    let msg = build_message(facility, severity, &hostname, app_name, message);
    let data = msg.as_bytes();

    // Prefer IPv6 when configured, fall back to IPv4.
    let result = if let Some(ipv6) = server_v6 {
        super::udp::send_v6(DEFAULT_PORT, ipv6, server_port, data)
    } else if let Some(ipv4) = server_v4 {
        super::udp::send(DEFAULT_PORT, ipv4, server_port, data)
    } else {
        return;
    };

    match result {
        Ok(()) => {
            MESSAGES_FORWARDED.fetch_add(1, Ordering::Relaxed);
        }
        Err(_) => {
            FORWARD_ERRORS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Convenience: forward a kernel log message.
pub fn forward_kern(severity: Severity, message: &str) {
    forward(Facility::Kern, severity, "kernel", message);
}

// ---------------------------------------------------------------------------
// Receiver API
// ---------------------------------------------------------------------------

/// Start the syslog receiver on the configured port.
pub fn start_receiver() -> KernelResult<()> {
    if RECEIVER_ENABLED.load(Ordering::Relaxed) {
        return Ok(());
    }

    let port = LISTEN_PORT.load(Ordering::Relaxed);
    let handle = super::udp::bind(crate::netns::ROOT_NS, port)?;

    let mut state = STATE.lock();
    state.recv_handle = Some(handle);

    // Pre-allocate ring buffer.
    if state.ring.is_empty() {
        state.ring.reserve(RING_BUFFER_SIZE);
    }

    if state.hostname.is_empty() {
        state.hostname = String::from("neo");
    }

    RECEIVER_ENABLED.store(true, Ordering::Relaxed);
    crate::serial_println!("[syslog] Receiver listening on UDP port {}", port);
    Ok(())
}

/// Stop the syslog receiver.
pub fn stop_receiver() {
    RECEIVER_ENABLED.store(false, Ordering::Relaxed);
    let mut state = STATE.lock();
    if let Some(handle) = state.recv_handle.take() {
        super::udp::close(handle);
    }
    crate::serial_println!("[syslog] Receiver stopped");
}

/// Set the listening port (before starting receiver).
pub fn set_port(port: u16) {
    LISTEN_PORT.store(port, Ordering::Relaxed);
}

/// Get recent received messages (newest first).
pub fn recent_messages(count: usize) -> Vec<SyslogMessage> {
    let state = STATE.lock();
    let n = count.min(state.ring_count);
    let mut result = Vec::with_capacity(n);

    for i in 0..n {
        let idx = if state.ring_write >= i.saturating_add(1) {
            state.ring_write.saturating_sub(i.saturating_add(1))
        } else {
            state.ring.len().saturating_sub(i.saturating_add(1).saturating_sub(state.ring_write))
        };
        if let Some(msg) = state.ring.get(idx) {
            result.push(msg.clone());
        }
    }

    result
}

/// Clear the received message ring buffer.
pub fn clear_messages() {
    let mut state = STATE.lock();
    state.ring.clear();
    state.ring_write = 0;
    state.ring_count = 0;
}

// ---------------------------------------------------------------------------
// Periodic tick
// ---------------------------------------------------------------------------

/// Process incoming syslog messages.
///
/// Called from `net::poll()`.
pub fn tick() {
    if !RECEIVER_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let now = crate::hrtimer::now_ns();
    let last = LAST_TICK.load(Ordering::Relaxed);
    if now.saturating_sub(last) < TICK_INTERVAL_NS {
        return;
    }
    LAST_TICK.store(now, Ordering::Relaxed);

    let handle = match STATE.lock().recv_handle {
        Some(h) => h,
        None => return,
    };

    // Process all pending IPv4 datagrams.
    while let Some(dgram) = super::udp::recv(handle) {
        let msg = parse_message(&dgram.data, dgram.src_ip);
        match msg {
            Some(m) => {
                MESSAGES_RECEIVED.fetch_add(1, Ordering::Relaxed);

                crate::serial_println!(
                    "[syslog] {}:{} <{}.{}> {} {} {}",
                    dgram.src_ip, dgram.src_port,
                    m.facility.label(), m.severity.label(),
                    m.hostname, m.app_name, m.message
                );

                // Store in ring buffer.
                let mut state = STATE.lock();
                if state.ring.len() < RING_BUFFER_SIZE {
                    state.ring.push(m);
                    state.ring_write = state.ring.len();
                } else {
                    let idx = state.ring_write % RING_BUFFER_SIZE;
                    if let Some(slot) = state.ring.get_mut(idx) {
                        *slot = m;
                    }
                    state.ring_write = state.ring_write.wrapping_add(1) % RING_BUFFER_SIZE;
                }
                state.ring_count = state.ring_count.saturating_add(1).min(RING_BUFFER_SIZE);
            }
            None => {
                PARSE_ERRORS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    // Process all pending IPv6 datagrams.
    while let Some(dgram) = super::udp::recv_v6(handle) {
        let msg = parse_message_v6(&dgram.data, dgram.src_ip);
        match msg {
            Some(m) => {
                MESSAGES_RECEIVED.fetch_add(1, Ordering::Relaxed);

                crate::serial_println!(
                    "[syslog] [{}]:{} <{}.{}> {} {} {}",
                    dgram.src_ip, dgram.src_port,
                    m.facility.label(), m.severity.label(),
                    m.hostname, m.app_name, m.message
                );

                // Store in ring buffer.
                let mut state = STATE.lock();
                if state.ring.len() < RING_BUFFER_SIZE {
                    state.ring.push(m);
                    state.ring_write = state.ring.len();
                } else {
                    let idx = state.ring_write % RING_BUFFER_SIZE;
                    if let Some(slot) = state.ring.get_mut(idx) {
                        *slot = m;
                    }
                    state.ring_write = state.ring_write.wrapping_add(1) % RING_BUFFER_SIZE;
                }
                state.ring_count = state.ring_count.saturating_add(1).min(RING_BUFFER_SIZE);
            }
            None => {
                PARSE_ERRORS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Syslog statistics.
#[derive(Debug)]
pub struct SyslogStats {
    pub receiver_enabled: bool,
    pub forwarder_enabled: bool,
    pub listen_port: u16,
    pub remote_server: Option<(Ipv4Addr, u16)>,
    pub remote_server_v6: Option<(Ipv6Addr, u16)>,
    pub messages_received: u64,
    pub messages_forwarded: u64,
    #[allow(dead_code)] // Stats field — exposed for procfs.
    pub messages_dropped: u64,
    pub parse_errors: u64,
    pub forward_errors: u64,
    pub ring_count: usize,
    pub hostname: String,
}

/// Get syslog statistics.
pub fn stats() -> SyslogStats {
    let state = STATE.lock();
    SyslogStats {
        receiver_enabled: RECEIVER_ENABLED.load(Ordering::Relaxed),
        forwarder_enabled: FORWARDER_ENABLED.load(Ordering::Relaxed),
        listen_port: LISTEN_PORT.load(Ordering::Relaxed),
        remote_server: state.remote_server.map(|ip| (ip, state.remote_port)),
        remote_server_v6: state.remote_server_v6.map(|ip| (ip, state.remote_port)),
        messages_received: MESSAGES_RECEIVED.load(Ordering::Relaxed),
        messages_forwarded: MESSAGES_FORWARDED.load(Ordering::Relaxed),
        messages_dropped: MESSAGES_DROPPED.load(Ordering::Relaxed),
        parse_errors: PARSE_ERRORS.load(Ordering::Relaxed),
        forward_errors: FORWARD_ERRORS.load(Ordering::Relaxed),
        ring_count: state.ring_count,
        hostname: state.hostname.clone(),
    }
}

// ---------------------------------------------------------------------------
// Procfs
// ---------------------------------------------------------------------------

/// Generate procfs content for `/proc/syslog`.
pub fn procfs_content() -> String {
    let s = stats();
    let recent = recent_messages(20);

    let mut out = String::with_capacity(1024);
    out.push_str("Network Syslog\n");
    out.push_str("==============\n\n");

    out.push_str(&format!("Receiver:      {}\n",
        if s.receiver_enabled { "running" } else { "stopped" }));
    out.push_str(&format!("Listen port:   {}\n", s.listen_port));
    out.push_str(&format!("Forwarder:     {}\n",
        if s.forwarder_enabled {
            if let Some((ip6, port)) = s.remote_server_v6 {
                format!("[{}]:{}", ip6, port)
            } else if let Some((ip, port)) = s.remote_server {
                format!("{}:{}", ip, port)
            } else {
                String::from("configured but no server")
            }
        } else { String::from("disabled") }));
    out.push_str(&format!("Hostname:      {}\n", s.hostname));
    out.push_str(&format!("Received:      {}\n", s.messages_received));
    out.push_str(&format!("Forwarded:     {}\n", s.messages_forwarded));
    out.push_str(&format!("Parse errors:  {}\n", s.parse_errors));
    out.push_str(&format!("Forward errs:  {}\n", s.forward_errors));
    out.push_str(&format!("Buffer:        {}/{}\n", s.ring_count, RING_BUFFER_SIZE));

    if !recent.is_empty() {
        out.push_str("\nRecent Messages:\n");
        for msg in &recent {
            out.push_str(&format!(
                "  <{}.{}> {} [{}] {}: {}\n",
                msg.facility.label(), msg.severity.label(),
                msg.source_addr, msg.hostname, msg.app_name, msg.message,
            ));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run syslog self-tests.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[syslog] Running syslog self-tests...");
    let mut passed = 0u32;

    // --- Test 1: PRI computation ---
    {
        let pri = compute_pri(Facility::Kern, Severity::Emergency);
        assert!(pri == 0, "kern.emerg = 0");

        let pri2 = compute_pri(Facility::User, Severity::Info);
        assert!(pri2 == 14, "user.info = 14");

        let pri3 = compute_pri(Facility::Local0, Severity::Debug);
        assert!(pri3 == 135, "local0.debug = 135");

        passed = passed.saturating_add(1);
        crate::serial_println!("[syslog]   test 1 (PRI computation) PASSED");
    }

    // --- Test 2: PRI decoding ---
    {
        let (fac, sev) = decode_pri(0);
        assert!(fac == Some(Facility::Kern), "facility kern");
        assert!(sev == Some(Severity::Emergency), "severity emerg");

        let (fac2, sev2) = decode_pri(14);
        assert!(fac2 == Some(Facility::User), "facility user");
        assert!(sev2 == Some(Severity::Info), "severity info");

        let (fac3, sev3) = decode_pri(135);
        assert!(fac3 == Some(Facility::Local0), "facility local0");
        assert!(sev3 == Some(Severity::Debug), "severity debug");

        passed = passed.saturating_add(1);
        crate::serial_println!("[syslog]   test 2 (PRI decoding) PASSED");
    }

    // --- Test 3: PRI round-trip ---
    {
        for fac_num in [0u8, 1, 3, 4, 5, 9, 16, 23] {
            for sev_num in 0..8u8 {
                if let (Some(fac), Some(sev)) = (Facility::from_u8(fac_num), Severity::from_u8(sev_num)) {
                    let pri = compute_pri(fac, sev);
                    let (decoded_fac, decoded_sev) = decode_pri(pri);
                    assert!(decoded_fac == Some(fac), "round-trip facility");
                    assert!(decoded_sev == Some(sev), "round-trip severity");
                }
            }
        }

        passed = passed.saturating_add(1);
        crate::serial_println!("[syslog]   test 3 (PRI round-trip) PASSED");
    }

    // --- Test 4: Message building ---
    {
        let msg = build_message(
            Facility::Kern, Severity::Info,
            "testhost", "kernel", "Boot complete",
        );
        assert!(msg.starts_with("<6>1"), "starts with PRI VERSION");
        assert!(msg.contains("testhost"), "contains hostname");
        assert!(msg.contains("kernel"), "contains app-name");
        assert!(msg.contains("Boot complete"), "contains message");

        passed = passed.saturating_add(1);
        crate::serial_println!("[syslog]   test 4 (message building) PASSED");
    }

    // --- Test 5: Message parsing ---
    {
        let raw = b"<14>1 2024-01-15T10:30:45Z myhost myapp - - Test message";
        let msg = parse_message(raw, Ipv4Addr([10, 0, 0, 1]));
        assert!(msg.is_some(), "parsed ok");
        let m = msg.unwrap();
        assert!(m.priority == 14, "priority 14");
        assert!(m.facility == Facility::User, "facility user");
        assert!(m.severity == Severity::Info, "severity info");
        assert!(m.message.contains("Test message"), "message text");

        passed = passed.saturating_add(1);
        crate::serial_println!("[syslog]   test 5 (message parsing) PASSED");
    }

    // --- Test 6: Severity ordering ---
    {
        assert!(Severity::Emergency < Severity::Alert, "emerg < alert");
        assert!(Severity::Alert < Severity::Critical, "alert < crit");
        assert!(Severity::Critical < Severity::Error, "crit < err");
        assert!(Severity::Error < Severity::Warning, "err < warning");
        assert!(Severity::Warning < Severity::Notice, "warning < notice");
        assert!(Severity::Notice < Severity::Info, "notice < info");
        assert!(Severity::Info < Severity::Debug, "info < debug");

        passed = passed.saturating_add(1);
        crate::serial_println!("[syslog]   test 6 (severity ordering) PASSED");
    }

    // --- Test 7: Facility labels ---
    {
        assert!(Facility::Kern.label() == "kern", "kern label");
        assert!(Facility::Daemon.label() == "daemon", "daemon label");
        assert!(Facility::Local7.label() == "local7", "local7 label");

        passed = passed.saturating_add(1);
        crate::serial_println!("[syslog]   test 7 (facility labels) PASSED");
    }

    // --- Test 8: Severity labels ---
    {
        assert!(Severity::Emergency.label() == "EMERG", "emerg label");
        assert!(Severity::Error.label() == "ERR", "err label");
        assert!(Severity::Debug.label() == "DEBUG", "debug label");

        passed = passed.saturating_add(1);
        crate::serial_println!("[syslog]   test 8 (severity labels) PASSED");
    }

    // --- Test 9: Date conversion ---
    {
        // Unix epoch = 1970-01-01.
        let (y, m, d) = days_to_date(0);
        assert!(y == 1970, "epoch year");
        assert!(m == 1, "epoch month");
        assert!(d == 1, "epoch day");

        // 2024-01-01 = day 19723 since epoch.
        let (y2, m2, d2) = days_to_date(19723);
        assert!(y2 == 2024, "2024 year");
        assert!(m2 == 1, "2024 month");
        assert!(d2 == 1, "2024 day");

        passed = passed.saturating_add(1);
        crate::serial_println!("[syslog]   test 9 (date conversion) PASSED");
    }

    // --- Test 10: Timestamp formatting ---
    {
        // 0 seconds = 1970-01-01T00:00:00Z.
        let ts = format_timestamp(0);
        assert!(ts == "1970-01-01T00:00:00Z", "epoch timestamp");

        // 86400 seconds = 1970-01-02T00:00:00Z.
        let ts2 = format_timestamp(86400);
        assert!(ts2 == "1970-01-02T00:00:00Z", "day 2 timestamp");

        passed = passed.saturating_add(1);
        crate::serial_println!("[syslog]   test 10 (timestamp formatting) PASSED");
    }

    // --- Test 11: Invalid message parsing ---
    {
        assert!(parse_message(b"not a syslog message", Ipv4Addr([0,0,0,0])).is_none(),
            "no PRI");
        assert!(parse_message(b"<>", Ipv4Addr([0,0,0,0])).is_none(),
            "empty PRI");
        assert!(parse_message(b"<abc>", Ipv4Addr([0,0,0,0])).is_none(),
            "non-numeric PRI");

        passed = passed.saturating_add(1);
        crate::serial_println!("[syslog]   test 11 (invalid message parsing) PASSED");
    }

    // --- Test 12: PRI edge cases ---
    {
        // Maximum valid PRI: facility=23 (local7), severity=7 (debug) = 191.
        let pri = compute_pri(Facility::Local7, Severity::Debug);
        assert!(pri == 191, "max PRI = 191");

        // Minimum valid PRI: facility=0 (kern), severity=0 (emerg) = 0.
        let pri_min = compute_pri(Facility::Kern, Severity::Emergency);
        assert!(pri_min == 0, "min PRI = 0");

        passed = passed.saturating_add(1);
        crate::serial_println!("[syslog]   test 12 (PRI edge cases) PASSED");
    }

    crate::serial_println!("[syslog] All {} self-tests PASSED", passed);
    Ok(())
}
