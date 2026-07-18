//! NTP client (RFC 5905 — SNTPv4).
//!
//! Implements a Simple NTP (SNTP) client that queries NTP servers
//! over UDP to synchronize the system clock.  Uses the existing
//! kernel UDP stack and DNS resolver.
//!
//! ## Protocol
//!
//! NTP uses UDP port 123.  An SNTP client sends a 48-byte request
//! with LI=0, VN=4 (NTPv4), Mode=3 (Client).  The server responds
//! with timestamps that allow computing the clock offset and
//! round-trip delay:
//!
//! ```text
//!   T1 = client transmit timestamp (origin)
//!   T2 = server receive timestamp
//!   T3 = server transmit timestamp
//!   T4 = client receive timestamp (destination)
//!
//!   offset = ((T2 - T1) + (T3 - T4)) / 2
//!   delay  = (T4 - T1) - (T3 - T2)
//! ```
//!
//! ## Features
//!
//! - NTP packet construction and parsing (48-byte NTPv4)
//! - Multiple server support (pool with round-robin)
//! - Clock offset calculation with delay estimation
//! - Kiss-of-Death (KoD) packet detection
//! - Leap indicator and stratum validation
//! - Periodic sync with configurable interval (default 64s)
//! - Statistics: queries sent, responses, timeouts, best offset
//!
//! ## Time representation
//!
//! NTP timestamps are 64-bit values: 32-bit seconds since 1900-01-01
//! plus 32-bit fractional seconds.  We convert between NTP timestamps
//! and our kernel nanosecond monotonic clock for offset calculation,
//! and between NTP and Unix epoch for wall-clock adjustment.
//!
//! ## Limitations
//!
//! - SNTP (no full NTP state machine, no intersection algorithm).
//! - No authentication (NTS, autokey, or symmetric key).
//! - Single-query offset (no multi-sample filtering).
//!
//! ## IPv6 support
//!
//! Queries can be sent over IPv6 when a SLAAC global address is available.
//! `sync_now()` tries IPv4 first; on failure it falls back to IPv6 (AAAA
//! DNS resolution + UDP-over-IPv6).  The `ntp sync6` kshell command forces
//! an IPv6-only sync attempt.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use super::interface::Ipv4Addr;
use super::ipv6::Ipv6Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// NTP server UDP port.
const NTP_PORT: u16 = 123;

/// NTP packet size (48 bytes minimum).
const NTP_PACKET_SIZE: usize = 48;

/// NTP version 4.
#[allow(dead_code)] // Referenced in documentation.
const NTP_VERSION: u8 = 4;

/// Client mode (3).
#[allow(dead_code)] // Referenced in documentation.
const MODE_CLIENT: u8 = 3;

/// Server mode (4).
const MODE_SERVER: u8 = 4;

/// Maximum number of configured NTP servers.
const MAX_SERVERS: usize = 8;

/// Default sync interval in nanoseconds (64 seconds).
const DEFAULT_SYNC_INTERVAL_NS: u64 = 64_000_000_000;

/// Minimum sync interval (16 seconds).
#[allow(dead_code)] // Used when interval API is called.
const MIN_SYNC_INTERVAL_NS: u64 = 16_000_000_000;

/// Maximum sync interval (1024 seconds).
#[allow(dead_code)] // Used when interval API is called.
const MAX_SYNC_INTERVAL_NS: u64 = 1_024_000_000_000;

/// Timeout for NTP response in poll cycles.
const NTP_TIMEOUT_POLLS: u32 = 30_000;

/// Seconds between Unix epoch (1970-01-01) and NTP epoch (1900-01-01).
/// 70 years, accounting for 17 leap years in 1904–1968.
const NTP_UNIX_OFFSET: u64 = 2_208_988_800;

/// Maximum acceptable stratum (above this, response is unreliable).
const MAX_STRATUM: u8 = 15;

/// Maximum acceptable delay in nanoseconds (500ms — anything higher
/// suggests a congested or distant path, making the offset unreliable).
const MAX_ACCEPTABLE_DELAY_NS: i64 = 500_000_000;

// ---------------------------------------------------------------------------
// NTP packet structures
// ---------------------------------------------------------------------------

/// Parsed NTP timestamp (seconds since 1900-01-01 + fraction).
#[derive(Debug, Clone, Copy, Default)]
struct NtpTimestamp {
    /// Seconds since 1900-01-01 00:00:00 UTC.
    seconds: u32,
    /// Fractional seconds (2^-32 seconds per unit).
    fraction: u32,
}

impl NtpTimestamp {
    /// Convert to nanoseconds since NTP epoch.
    fn to_nanos(self) -> u64 {
        let secs_ns = (self.seconds as u64).saturating_mul(1_000_000_000);
        // fraction / 2^32 * 10^9 ≈ fraction * 10^9 >> 32
        let frac_ns = ((self.fraction as u64).saturating_mul(1_000_000_000)) >> 32;
        secs_ns.saturating_add(frac_ns)
    }

    /// Create from NTP epoch nanoseconds.
    fn from_nanos(ns: u64) -> Self {
        let seconds = (ns / 1_000_000_000) as u32;
        let remainder_ns = ns % 1_000_000_000;
        // remainder_ns / 10^9 * 2^32
        let fraction = ((remainder_ns << 32) / 1_000_000_000) as u32;
        Self { seconds, fraction }
    }

    /// Convert NTP timestamp to Unix timestamp (seconds since 1970-01-01).
    fn to_unix_secs(self) -> i64 {
        (self.seconds as i64).saturating_sub(NTP_UNIX_OFFSET as i64)
    }

    /// Encode into 8 bytes (network byte order).
    fn encode(&self, buf: &mut [u8]) {
        if let Some(b) = buf.get_mut(0) { *b = (self.seconds >> 24) as u8; }
        if let Some(b) = buf.get_mut(1) { *b = (self.seconds >> 16) as u8; }
        if let Some(b) = buf.get_mut(2) { *b = (self.seconds >> 8) as u8; }
        if let Some(b) = buf.get_mut(3) { *b = self.seconds as u8; }
        if let Some(b) = buf.get_mut(4) { *b = (self.fraction >> 24) as u8; }
        if let Some(b) = buf.get_mut(5) { *b = (self.fraction >> 16) as u8; }
        if let Some(b) = buf.get_mut(6) { *b = (self.fraction >> 8) as u8; }
        if let Some(b) = buf.get_mut(7) { *b = self.fraction as u8; }
    }

    /// Decode from 8 bytes (network byte order).
    fn decode(buf: &[u8]) -> Self {
        let seconds = (*buf.first().unwrap_or(&0) as u32) << 24
            | (*buf.get(1).unwrap_or(&0) as u32) << 16
            | (*buf.get(2).unwrap_or(&0) as u32) << 8
            | *buf.get(3).unwrap_or(&0) as u32;
        let fraction = (*buf.get(4).unwrap_or(&0) as u32) << 24
            | (*buf.get(5).unwrap_or(&0) as u32) << 16
            | (*buf.get(6).unwrap_or(&0) as u32) << 8
            | *buf.get(7).unwrap_or(&0) as u32;
        Self { seconds, fraction }
    }

    fn is_zero(self) -> bool {
        self.seconds == 0 && self.fraction == 0
    }
}

/// Parsed NTP response packet.
#[derive(Debug)]
#[allow(dead_code)] // All fields used in validation and offset calculation.
struct NtpResponse {
    /// Leap indicator (0=none, 1=+1sec, 2=-1sec, 3=unsynchronized).
    leap_indicator: u8,
    /// NTP version (should be 3 or 4).
    version: u8,
    /// Mode (should be 4=server for responses).
    mode: u8,
    /// Stratum (1=primary, 2-15=secondary, 0=KoD, 16=unsynchronized).
    stratum: u8,
    /// Poll interval (log2 seconds).
    poll_interval: i8,
    /// Precision (log2 seconds).
    precision: i8,
    /// Root delay (seconds, fixed-point).
    root_delay: u32,
    /// Root dispersion (seconds, fixed-point).
    root_dispersion: u32,
    /// Reference ID (4 bytes — stratum 1: ASCII source, else: IP of ref server).
    reference_id: [u8; 4],
    /// Reference timestamp (last clock update).
    reference_ts: NtpTimestamp,
    /// Origin timestamp (T1 — our transmit time, echoed back).
    origin_ts: NtpTimestamp,
    /// Receive timestamp (T2 — server's receive time).
    receive_ts: NtpTimestamp,
    /// Transmit timestamp (T3 — server's transmit time).
    transmit_ts: NtpTimestamp,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Configured NTP server entry.
struct NtpServer {
    /// Hostname or IP address.
    address: String,
    /// Resolved IPv4 address (cached).
    resolved_ip: Option<Ipv4Addr>,
    /// Resolved IPv6 address (cached).
    resolved_ipv6: Option<Ipv6Addr>,
    /// Whether this server is active/reachable.
    reachable: bool,
    /// Last query timestamp (kernel monotonic ns).
    last_query_ns: u64,
    /// Last measured offset (nanoseconds, signed).
    last_offset_ns: i64,
    /// Last measured delay (nanoseconds).
    last_delay_ns: i64,
    /// Stratum of last response.
    last_stratum: u8,
    /// Number of successful responses.
    success_count: u32,
    /// Number of failed queries.
    fail_count: u32,
}

impl NtpServer {
    fn new(address: &str) -> Self {
        Self {
            address: String::from(address),
            resolved_ip: None,
            resolved_ipv6: None,
            reachable: false,
            last_query_ns: 0,
            last_offset_ns: 0,
            last_delay_ns: 0,
            last_stratum: 0,
            success_count: 0,
            fail_count: 0,
        }
    }
}

/// Global NTP state.
struct NtpState {
    servers: Vec<NtpServer>,
    /// Index of next server to query (round-robin).
    next_server: usize,
    /// Whether NTP sync is enabled.
    enabled: bool,
    /// Current sync interval (nanoseconds).
    sync_interval_ns: u64,
}

impl NtpState {
    const fn new() -> Self {
        Self {
            servers: Vec::new(),
            next_server: 0,
            enabled: false,
            sync_interval_ns: DEFAULT_SYNC_INTERVAL_NS,
        }
    }
}

static STATE: Mutex<NtpState> = Mutex::new(NtpState::new());

/// Whether NTP has been initialized.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Best measured clock offset (nanoseconds, signed).
/// Positive = our clock is behind the server.
static BEST_OFFSET_NS: AtomicI64 = AtomicI64::new(0);

/// Timestamp of last successful sync (kernel monotonic ns).
static LAST_SYNC_NS: AtomicU64 = AtomicU64::new(0);

/// Timestamp of last sync attempt (kernel monotonic ns).
static LAST_ATTEMPT_NS: AtomicU64 = AtomicU64::new(0);

// Statistics.
static QUERIES_SENT: AtomicU64 = AtomicU64::new(0);
static RESPONSES_OK: AtomicU64 = AtomicU64::new(0);
static RESPONSES_BAD: AtomicU64 = AtomicU64::new(0);
static TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static KOD_RECEIVED: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Packet construction
// ---------------------------------------------------------------------------

/// Build a 48-byte NTP client request packet.
///
/// The packet has LI=0 (no warning), VN=4, Mode=3 (client).
/// The transmit timestamp is set to the current wall-clock time
/// so the server can echo it back as the origin timestamp.
fn build_request(transmit_ts: NtpTimestamp) -> [u8; NTP_PACKET_SIZE] {
    let mut pkt = [0u8; NTP_PACKET_SIZE];

    // Byte 0: LI (2 bits) | VN (3 bits) | Mode (3 bits)
    // LI=0 (00), VN=4 (100), Mode=3 (011) → 0b00_100_011 = 0x23
    if let Some(b) = pkt.get_mut(0) {
        *b = 0x23;
    }

    // Byte 1: stratum (0 for client request).
    // Byte 2: poll interval (6 = 64 seconds as log2).
    if let Some(b) = pkt.get_mut(2) {
        *b = 6;
    }
    // Byte 3: precision (-20 ≈ microsecond).
    if let Some(b) = pkt.get_mut(3) {
        *b = 0xEC; // -20 as signed byte
    }

    // Bytes 12-15: Reference ID — leave as zeros for client.
    // Bytes 40-47: Transmit timestamp (our T1).
    if let Some(slice) = pkt.get_mut(40..48) {
        transmit_ts.encode(slice);
    }

    pkt
}

/// Parse a 48-byte NTP response packet.
fn parse_response(data: &[u8]) -> KernelResult<NtpResponse> {
    if data.len() < NTP_PACKET_SIZE {
        return Err(KernelError::InvalidArgument);
    }

    let byte0 = *data.first().unwrap_or(&0);
    let leap_indicator = (byte0 >> 6) & 0x03;
    let version = (byte0 >> 3) & 0x07;
    let mode = byte0 & 0x07;

    let stratum = *data.get(1).unwrap_or(&0);
    let poll_interval = *data.get(2).unwrap_or(&0) as i8;
    let precision = *data.get(3).unwrap_or(&0) as i8;

    let root_delay = read_u32(data, 4);
    let root_dispersion = read_u32(data, 8);

    let mut reference_id = [0u8; 4];
    for i in 0..4 {
        reference_id[i] = *data.get(12 + i).unwrap_or(&0);
    }

    let reference_ts = NtpTimestamp::decode(data.get(16..24).unwrap_or(&[0; 8]));
    let origin_ts = NtpTimestamp::decode(data.get(24..32).unwrap_or(&[0; 8]));
    let receive_ts = NtpTimestamp::decode(data.get(32..40).unwrap_or(&[0; 8]));
    let transmit_ts = NtpTimestamp::decode(data.get(40..48).unwrap_or(&[0; 8]));

    Ok(NtpResponse {
        leap_indicator,
        version,
        mode,
        stratum,
        poll_interval,
        precision,
        root_delay,
        root_dispersion,
        reference_id,
        reference_ts,
        origin_ts,
        receive_ts,
        transmit_ts,
    })
}

/// Read a big-endian u32 from a byte slice at the given offset.
fn read_u32(data: &[u8], offset: usize) -> u32 {
    (*data.get(offset).unwrap_or(&0) as u32) << 24
        | (*data.get(offset.wrapping_add(1)).unwrap_or(&0) as u32) << 16
        | (*data.get(offset.wrapping_add(2)).unwrap_or(&0) as u32) << 8
        | *data.get(offset.wrapping_add(3)).unwrap_or(&0) as u32
}

// ---------------------------------------------------------------------------
// NTP query
// ---------------------------------------------------------------------------

/// Perform a single NTP query to the specified server.
///
/// Returns (offset_ns, delay_ns, stratum) on success.
fn query_server(server_ip: Ipv4Addr) -> KernelResult<(i64, i64, u8)> {
    // Get current wall-clock time for T1.
    let t1_kernel_ns = crate::hrtimer::now_ns();
    let t1_ntp = wall_clock_to_ntp(t1_kernel_ns);

    // Build request packet.
    let request = build_request(t1_ntp);

    // Bind a UDP socket for the query.
    let local_port = ephemeral_port();
    let udp_handle = super::udp::bind(crate::netns::ROOT_NS, local_port)?;

    // Send the NTP request.
    let send_result = super::udp::send(local_port, server_ip, NTP_PORT, &request);
    if let Err(e) = send_result {
        super::udp::close(udp_handle);
        return Err(e);
    }

    QUERIES_SENT.fetch_add(1, Ordering::Relaxed);

    // Wait for response.
    let mut response_data = None;
    for _ in 0..NTP_TIMEOUT_POLLS {
        super::poll();

        if let Some(dgram) = super::udp::recv(udp_handle) {
            if dgram.src_ip == server_ip && dgram.src_port == NTP_PORT {
                response_data = Some(dgram.data);
                break;
            }
        }

        for _ in 0..5_000 {
            core::hint::spin_loop();
        }
    }

    // Record T4 immediately after receiving the response.
    let t4_kernel_ns = crate::hrtimer::now_ns();

    super::udp::close(udp_handle);

    let data = match response_data {
        Some(d) => d,
        None => {
            TIMEOUTS.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::TimedOut);
        }
    };

    // Parse the response.
    let resp = parse_response(&data)?;

    // Validate the response.
    validate_response(&resp, &t1_ntp)?;

    // T2 and T3 from the server.
    let t2_ns = resp.receive_ts.to_nanos();
    let t3_ns = resp.transmit_ts.to_nanos();

    // Convert T1 and T4 to NTP epoch nanoseconds for consistent math.
    let t1_ns = t1_ntp.to_nanos();
    let t4_ntp = wall_clock_to_ntp(t4_kernel_ns);
    let t4_ns = t4_ntp.to_nanos();

    // Calculate offset and delay.
    // offset = ((T2 - T1) + (T3 - T4)) / 2
    // delay  = (T4 - T1) - (T3 - T2)
    let t2_minus_t1 = (t2_ns as i64).saturating_sub(t1_ns as i64);
    let t3_minus_t4 = (t3_ns as i64).saturating_sub(t4_ns as i64);
    let offset_ns = (t2_minus_t1.saturating_add(t3_minus_t4)) / 2;

    let t4_minus_t1 = (t4_ns as i64).saturating_sub(t1_ns as i64);
    let t3_minus_t2 = (t3_ns as i64).saturating_sub(t2_ns as i64);
    let delay_ns = t4_minus_t1.saturating_sub(t3_minus_t2);

    RESPONSES_OK.fetch_add(1, Ordering::Relaxed);

    Ok((offset_ns, delay_ns, resp.stratum))
}

/// Perform a single NTP query over IPv6.
///
/// Same protocol as [`query_server`] but uses the UDP-over-IPv6 transport.
/// Returns (offset_ns, delay_ns, stratum) on success.
fn query_server_v6(server_ip: Ipv6Addr) -> KernelResult<(i64, i64, u8)> {
    let t1_kernel_ns = crate::hrtimer::now_ns();
    let t1_ntp = wall_clock_to_ntp(t1_kernel_ns);

    let request = build_request(t1_ntp);

    // Bind a UDP socket for the query (same dual-stack socket handles v6).
    let local_port = ephemeral_port();
    let udp_handle = super::udp::bind(crate::netns::ROOT_NS, local_port)?;

    // Send the NTP request over IPv6.
    let send_result = super::udp::send_v6(local_port, server_ip, NTP_PORT, &request);
    if let Err(e) = send_result {
        super::udp::close(udp_handle);
        return Err(e);
    }

    QUERIES_SENT.fetch_add(1, Ordering::Relaxed);

    // Wait for IPv6 response.
    let mut response_data = None;
    for _ in 0..NTP_TIMEOUT_POLLS {
        super::poll();

        if let Some(dgram) = super::udp::recv_v6(udp_handle) {
            if dgram.src_ip == server_ip && dgram.src_port == NTP_PORT {
                response_data = Some(dgram.data);
                break;
            }
        }

        for _ in 0..5_000 {
            core::hint::spin_loop();
        }
    }

    let t4_kernel_ns = crate::hrtimer::now_ns();

    super::udp::close(udp_handle);

    let data = match response_data {
        Some(d) => d,
        None => {
            TIMEOUTS.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::TimedOut);
        }
    };

    let resp = parse_response(&data)?;
    validate_response(&resp, &t1_ntp)?;

    let t2_ns = resp.receive_ts.to_nanos();
    let t3_ns = resp.transmit_ts.to_nanos();
    let t1_ns = t1_ntp.to_nanos();
    let t4_ntp = wall_clock_to_ntp(t4_kernel_ns);
    let t4_ns = t4_ntp.to_nanos();

    let t2_minus_t1 = (t2_ns as i64).saturating_sub(t1_ns as i64);
    let t3_minus_t4 = (t3_ns as i64).saturating_sub(t4_ns as i64);
    let offset_ns = (t2_minus_t1.saturating_add(t3_minus_t4)) / 2;

    let t4_minus_t1 = (t4_ns as i64).saturating_sub(t1_ns as i64);
    let t3_minus_t2 = (t3_ns as i64).saturating_sub(t2_ns as i64);
    let delay_ns = t4_minus_t1.saturating_sub(t3_minus_t2);

    RESPONSES_OK.fetch_add(1, Ordering::Relaxed);

    Ok((offset_ns, delay_ns, resp.stratum))
}

/// Validate an NTP response for sanity.
fn validate_response(resp: &NtpResponse, our_origin: &NtpTimestamp) -> KernelResult<()> {
    // Must be server mode (4).
    if resp.mode != MODE_SERVER {
        RESPONSES_BAD.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::InvalidArgument);
    }

    // Leap indicator 3 = unsynchronized.
    if resp.leap_indicator == 3 {
        RESPONSES_BAD.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::NotSupported);
    }

    // Stratum 0 = Kiss-of-Death.
    if resp.stratum == 0 {
        KOD_RECEIVED.fetch_add(1, Ordering::Relaxed);
        let kod_code = core::str::from_utf8(&resp.reference_id).unwrap_or("????");
        crate::serial_println!("[ntp] Kiss-of-Death received: {}", kod_code);
        return Err(KernelError::PermissionDenied);
    }

    // Stratum too high.
    if resp.stratum > MAX_STRATUM {
        RESPONSES_BAD.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::InvalidArgument);
    }

    // Origin timestamp must match what we sent (prevents replay).
    if resp.origin_ts.seconds != our_origin.seconds
        || resp.origin_ts.fraction != our_origin.fraction
    {
        RESPONSES_BAD.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::InvalidArgument);
    }

    // Transmit and receive timestamps must not be zero — a valid server
    // always populates both T2 (receive) and T3 (transmit).
    if resp.transmit_ts.is_zero() || resp.receive_ts.is_zero() {
        RESPONSES_BAD.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::InvalidArgument);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Time conversion helpers
// ---------------------------------------------------------------------------

/// Convert a wall-clock instant to an NTP timestamp.
///
/// Uses the timekeeping module's realtime clock (Unix epoch nanoseconds)
/// to produce an NTP epoch timestamp.  The `kernel_ns` parameter is
/// the monotonic timestamp at the moment we want to express; we adjust
/// the realtime clock by the delta between now and `kernel_ns` so that
/// T1/T4 timestamps reflect the actual send/receive instants.
fn wall_clock_to_ntp(kernel_ns: u64) -> NtpTimestamp {
    // clock_realtime() returns nanoseconds since Unix epoch.
    let now_mono = crate::hrtimer::now_ns();
    let realtime_ns = crate::timekeeping::clock_realtime();
    // Adjust: if kernel_ns was captured before now, subtract the
    // elapsed time so we reconstruct the realtime at that instant.
    let adjusted = if kernel_ns <= now_mono {
        realtime_ns.saturating_sub(now_mono.saturating_sub(kernel_ns))
    } else {
        // kernel_ns is in the future (shouldn't happen, but be safe).
        realtime_ns.saturating_add(kernel_ns.saturating_sub(now_mono))
    };
    let unix_secs = adjusted / 1_000_000_000;
    let ntp_secs = unix_secs.saturating_add(NTP_UNIX_OFFSET);
    let remainder_ns = adjusted % 1_000_000_000;

    NtpTimestamp {
        seconds: ntp_secs as u32,
        fraction: ((remainder_ns << 32) / 1_000_000_000) as u32,
    }
}

/// Generate an ephemeral port for NTP queries.
///
/// Uses a simple counter to avoid port collisions.
fn ephemeral_port() -> u16 {
    static PORT_COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = PORT_COUNTER.fetch_add(1, Ordering::Relaxed);
    // Range 49152–65535 (14 bits of space).
    
    49152u16.saturating_add((n % 16384) as u16)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the NTP client with default servers.
///
/// Adds common NTP pool servers and enables periodic sync.
pub fn init() {
    let mut state = STATE.lock();
    if INITIALIZED.load(Ordering::Relaxed) {
        return;
    }

    // Add default NTP pool servers.
    state.servers.push(NtpServer::new("pool.ntp.org"));
    state.servers.push(NtpServer::new("0.pool.ntp.org"));
    state.servers.push(NtpServer::new("1.pool.ntp.org"));
    state.servers.push(NtpServer::new("2.pool.ntp.org"));

    state.enabled = true;
    INITIALIZED.store(true, Ordering::Relaxed);

    crate::serial_println!("[ntp] Initialized with {} servers", state.servers.len());
}

/// Add a custom NTP server.
pub fn add_server(address: &str) -> bool {
    let mut state = STATE.lock();
    if state.servers.len() >= MAX_SERVERS {
        return false;
    }
    // Don't add duplicates.
    if state.servers.iter().any(|s| s.address == address) {
        return false;
    }
    state.servers.push(NtpServer::new(address));
    true
}

/// Remove an NTP server by address.
pub fn remove_server(address: &str) -> bool {
    let mut state = STATE.lock();
    let initial_len = state.servers.len();
    state.servers.retain(|s| s.address != address);
    state.servers.len() < initial_len
}

/// Enable or disable NTP synchronization.
pub fn set_enabled(enabled: bool) {
    STATE.lock().enabled = enabled;
}

/// Check if NTP sync is enabled.
pub fn is_enabled() -> bool {
    STATE.lock().enabled
}

/// Set the sync interval (clamped to 16s–1024s).
#[allow(dead_code)] // Public API for future settings UI.
pub fn set_sync_interval_secs(secs: u64) {
    let ns = secs.saturating_mul(1_000_000_000);
    let clamped = ns.clamp(MIN_SYNC_INTERVAL_NS, MAX_SYNC_INTERVAL_NS);
    STATE.lock().sync_interval_ns = clamped;
}

/// Trigger an immediate NTP sync attempt.
///
/// Queries the next server in the pool and returns the measured offset.
pub fn sync_now() -> KernelResult<i64> {
    let (address, server_idx) = {
        let state = STATE.lock();
        if state.servers.is_empty() {
            return Err(KernelError::NotFound);
        }
        let idx = state.next_server % state.servers.len();
        let addr = state.servers.get(idx)
            .map(|s| s.address.clone())
            .ok_or(KernelError::InternalError)?;
        (addr, idx)
    };

    crate::serial_println!("[ntp] Querying {}...", address);

    let now = crate::hrtimer::now_ns();
    LAST_ATTEMPT_NS.store(now, Ordering::Relaxed);

    // Try IPv4 first, fall back to IPv6 if IPv4 resolution or query fails.
    let (offset_ns, delay_ns, stratum) = match resolve_ntp_server(&address, server_idx) {
        Ok(ip) => {
            match query_server(ip) {
                Ok(result) => result,
                Err(_v4_err) => {
                    // IPv4 query failed — try IPv6.
                    let ipv6 = resolve_ntp_server_v6(&address, server_idx)?;
                    crate::serial_println!("[ntp] IPv4 failed, trying IPv6 ({})", ipv6);
                    query_server_v6(ipv6)?
                }
            }
        }
        Err(_v4_resolve_err) => {
            // IPv4 resolution failed — try IPv6.
            let ipv6 = resolve_ntp_server_v6(&address, server_idx)?;
            crate::serial_println!("[ntp] No IPv4 address, using IPv6 ({})", ipv6);
            query_server_v6(ipv6)?
        }
    };

    // Validate delay.
    if delay_ns > MAX_ACCEPTABLE_DELAY_NS {
        crate::serial_println!(
            "[ntp] Delay too high: {}ms (max {}ms)",
            delay_ns / 1_000_000,
            MAX_ACCEPTABLE_DELAY_NS / 1_000_000,
        );
        // Still record the result but don't apply it.
        update_server_stats(server_idx, offset_ns, delay_ns, stratum, false);
        return Err(KernelError::TimedOut);
    }

    // Record successful sync.
    BEST_OFFSET_NS.store(offset_ns, Ordering::Relaxed);
    LAST_SYNC_NS.store(now, Ordering::Relaxed);

    update_server_stats(server_idx, offset_ns, delay_ns, stratum, true);

    // Advance to next server.
    {
        let mut state = STATE.lock();
        state.next_server = server_idx.wrapping_add(1);
    }

    crate::serial_println!(
        "[ntp] Sync OK: offset={:+}ms, delay={}ms, stratum={}",
        offset_ns / 1_000_000,
        delay_ns / 1_000_000,
        stratum,
    );

    Ok(offset_ns)
}

/// Resolve an NTP server address, caching the result.
fn resolve_ntp_server(address: &str, server_idx: usize) -> KernelResult<Ipv4Addr> {
    // Check cached IP first.
    {
        let state = STATE.lock();
        if let Some(server) = state.servers.get(server_idx) {
            if let Some(ip) = server.resolved_ip {
                return Ok(ip);
            }
        }
    }

    // Try parsing as dotted-decimal IP.
    if let Some(ip) = parse_ipv4(address) {
        let mut state = STATE.lock();
        if let Some(server) = state.servers.get_mut(server_idx) {
            server.resolved_ip = Some(ip);
        }
        return Ok(ip);
    }

    // DNS resolution.
    let ip = super::dns::resolve(address)?;
    let mut state = STATE.lock();
    if let Some(server) = state.servers.get_mut(server_idx) {
        server.resolved_ip = Some(ip);
    }
    Ok(ip)
}

/// Resolve an NTP server to an IPv6 address, caching the result.
fn resolve_ntp_server_v6(address: &str, server_idx: usize) -> KernelResult<Ipv6Addr> {
    // Check cached IPv6 first.
    {
        let state = STATE.lock();
        if let Some(server) = state.servers.get(server_idx) {
            if let Some(ip) = server.resolved_ipv6 {
                return Ok(ip);
            }
        }
    }

    // Try parsing as an IPv6 literal.
    if let Some(ip) = Ipv6Addr::parse(address) {
        let mut state = STATE.lock();
        if let Some(server) = state.servers.get_mut(server_idx) {
            server.resolved_ipv6 = Some(ip);
        }
        return Ok(ip);
    }

    // DNS AAAA resolution.
    let ip = super::dns::resolve6(address)?;
    let mut state = STATE.lock();
    if let Some(server) = state.servers.get_mut(server_idx) {
        server.resolved_ipv6 = Some(ip);
    }
    Ok(ip)
}

/// Force an NTP sync using only IPv6 transport.
///
/// Unlike `sync_now()` which tries IPv4 first, this function uses
/// AAAA DNS resolution and UDP-over-IPv6 exclusively.  Useful for
/// testing IPv6 NTP connectivity.
pub fn sync_now_v6() -> KernelResult<i64> {
    let (address, server_idx) = {
        let state = STATE.lock();
        if state.servers.is_empty() {
            return Err(KernelError::NotFound);
        }
        let idx = state.next_server % state.servers.len();
        let addr = state.servers.get(idx)
            .map(|s| s.address.clone())
            .ok_or(KernelError::InternalError)?;
        (addr, idx)
    };

    crate::serial_println!("[ntp] Querying {} (IPv6)...", address);

    let ipv6 = resolve_ntp_server_v6(&address, server_idx)?;

    let now = crate::hrtimer::now_ns();
    LAST_ATTEMPT_NS.store(now, Ordering::Relaxed);

    let (offset_ns, delay_ns, stratum) = query_server_v6(ipv6)?;

    if delay_ns > MAX_ACCEPTABLE_DELAY_NS {
        crate::serial_println!(
            "[ntp] IPv6 delay too high: {}ms (max {}ms)",
            delay_ns / 1_000_000,
            MAX_ACCEPTABLE_DELAY_NS / 1_000_000,
        );
        update_server_stats(server_idx, offset_ns, delay_ns, stratum, false);
        return Err(KernelError::TimedOut);
    }

    BEST_OFFSET_NS.store(offset_ns, Ordering::Relaxed);
    LAST_SYNC_NS.store(now, Ordering::Relaxed);

    update_server_stats(server_idx, offset_ns, delay_ns, stratum, true);

    {
        let mut state = STATE.lock();
        state.next_server = server_idx.wrapping_add(1);
    }

    crate::serial_println!(
        "[ntp] IPv6 sync OK: offset={:+}ms, delay={}ms, stratum={}",
        offset_ns / 1_000_000,
        delay_ns / 1_000_000,
        stratum,
    );

    Ok(offset_ns)
}

/// Update per-server statistics after a query.
fn update_server_stats(
    server_idx: usize,
    offset_ns: i64,
    delay_ns: i64,
    stratum: u8,
    success: bool,
) {
    let mut state = STATE.lock();
    if let Some(server) = state.servers.get_mut(server_idx) {
        server.last_query_ns = crate::hrtimer::now_ns();
        server.last_offset_ns = offset_ns;
        server.last_delay_ns = delay_ns;
        server.last_stratum = stratum;
        if success {
            server.reachable = true;
            server.success_count = server.success_count.saturating_add(1);
        } else {
            server.fail_count = server.fail_count.saturating_add(1);
        }
    }
}

/// Periodic tick — called from the network poll loop.
///
/// Checks if it's time for a sync and triggers one if so.
pub fn tick() {
    if !INITIALIZED.load(Ordering::Relaxed) {
        return;
    }

    let (enabled, interval_ns, has_servers) = {
        let state = STATE.lock();
        (state.enabled, state.sync_interval_ns, !state.servers.is_empty())
    };

    if !enabled || !has_servers {
        return;
    }

    let now = crate::hrtimer::now_ns();
    let last = LAST_ATTEMPT_NS.load(Ordering::Relaxed);

    if now.saturating_sub(last) >= interval_ns {
        let _ = sync_now();
    }
}

/// Get the current best clock offset in nanoseconds.
///
/// Positive means our clock is behind the NTP server.
#[allow(dead_code)] // Public API.
pub fn clock_offset_ns() -> i64 {
    BEST_OFFSET_NS.load(Ordering::Relaxed)
}

/// Get the corrected Unix timestamp (realtime clock + NTP offset).
pub fn corrected_unix_secs() -> i64 {
    let realtime_ns = crate::timekeeping::clock_realtime();
    let unix_secs = (realtime_ns / 1_000_000_000) as i64;
    let offset_ns = BEST_OFFSET_NS.load(Ordering::Relaxed);
    // Convert offset from ns to seconds (truncate towards zero).
    let offset_secs = offset_ns / 1_000_000_000;
    unix_secs.saturating_add(offset_secs)
}

/// Check if the clock has been synchronized at least once.
#[allow(dead_code)] // Public API.
pub fn is_synchronized() -> bool {
    LAST_SYNC_NS.load(Ordering::Relaxed) > 0
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// NTP client statistics.
#[derive(Debug)]
#[allow(dead_code)] // All fields used by kshell and procfs.
pub struct NtpStats {
    pub initialized: bool,
    pub enabled: bool,
    pub synchronized: bool,
    pub server_count: usize,
    pub queries_sent: u64,
    pub responses_ok: u64,
    pub responses_bad: u64,
    pub timeouts: u64,
    pub kod_received: u32,
    pub best_offset_ms: i64,
    pub sync_interval_secs: u64,
    pub last_sync_ago_secs: u64,
}

/// Get NTP client statistics.
pub fn stats() -> NtpStats {
    let state = STATE.lock();
    let now = crate::hrtimer::now_ns();
    let last_sync = LAST_SYNC_NS.load(Ordering::Relaxed);
    let ago = if last_sync > 0 {
        now.saturating_sub(last_sync) / 1_000_000_000
    } else {
        0
    };

    NtpStats {
        initialized: INITIALIZED.load(Ordering::Relaxed),
        enabled: state.enabled,
        synchronized: last_sync > 0,
        server_count: state.servers.len(),
        queries_sent: QUERIES_SENT.load(Ordering::Relaxed),
        responses_ok: RESPONSES_OK.load(Ordering::Relaxed),
        responses_bad: RESPONSES_BAD.load(Ordering::Relaxed),
        timeouts: TIMEOUTS.load(Ordering::Relaxed),
        kod_received: KOD_RECEIVED.load(Ordering::Relaxed),
        best_offset_ms: BEST_OFFSET_NS.load(Ordering::Relaxed) / 1_000_000,
        sync_interval_secs: state.sync_interval_ns / 1_000_000_000,
        last_sync_ago_secs: ago,
    }
}

/// Get info about all configured servers.
pub fn server_info() -> Vec<ServerInfo> {
    let state = STATE.lock();
    let mut result = Vec::with_capacity(state.servers.len());
    for (i, s) in state.servers.iter().enumerate() {
        result.push(ServerInfo {
            index: i,
            address: s.address.clone(),
            resolved_ip: s.resolved_ip,
            resolved_ipv6: s.resolved_ipv6,
            reachable: s.reachable,
            last_offset_ms: s.last_offset_ns / 1_000_000,
            last_delay_ms: s.last_delay_ns / 1_000_000,
            last_stratum: s.last_stratum,
            success_count: s.success_count,
            fail_count: s.fail_count,
        });
    }
    result
}

/// Info about a single NTP server.
#[derive(Debug)]
pub struct ServerInfo {
    pub index: usize,
    pub address: String,
    pub resolved_ip: Option<Ipv4Addr>,
    pub resolved_ipv6: Option<Ipv6Addr>,
    pub reachable: bool,
    pub last_offset_ms: i64,
    pub last_delay_ms: i64,
    pub last_stratum: u8,
    pub success_count: u32,
    pub fail_count: u32,
}

// ---------------------------------------------------------------------------
// Procfs
// ---------------------------------------------------------------------------

/// Generate procfs content for `/proc/ntp`.
pub fn procfs_content() -> String {
    let s = stats();
    let servers = server_info();

    let mut out = String::with_capacity(512);
    out.push_str("NTP Client Status\n");
    out.push_str("==================\n\n");

    out.push_str(&format!("Enabled:       {}\n", s.enabled));
    out.push_str(&format!("Synchronized:  {}\n", s.synchronized));
    out.push_str(&format!("Clock offset:  {:+}ms\n", s.best_offset_ms));
    out.push_str(&format!("Sync interval: {}s\n", s.sync_interval_secs));
    if s.last_sync_ago_secs > 0 {
        out.push_str(&format!("Last sync:     {}s ago\n", s.last_sync_ago_secs));
    } else {
        out.push_str("Last sync:     never\n");
    }
    out.push_str(&format!("Queries sent:  {}\n", s.queries_sent));
    out.push_str(&format!("  OK:          {}\n", s.responses_ok));
    out.push_str(&format!("  Bad:         {}\n", s.responses_bad));
    out.push_str(&format!("  Timeouts:    {}\n", s.timeouts));
    if s.kod_received > 0 {
        out.push_str(&format!("  KoD:         {}\n", s.kod_received));
    }

    out.push_str(&format!("\nServers: {}\n", servers.len()));
    for srv in &servers {
        let v4_str = match srv.resolved_ip {
            Some(ip) => format!("{}", ip),
            None => String::from("-"),
        };
        let v6_str = match srv.resolved_ipv6 {
            Some(ip) => format!("{}", ip),
            None => String::from("-"),
        };
        out.push_str(&format!(
            "  [{}] {} (v4={}, v6={}) — {}reachable, offset={:+}ms, delay={}ms, stratum={}, ok={}, fail={}\n",
            srv.index, srv.address, v4_str, v6_str,
            if srv.reachable { "" } else { "un" },
            srv.last_offset_ms, srv.last_delay_ms,
            srv.last_stratum, srv.success_count, srv.fail_count,
        ));
    }

    out
}

// ---------------------------------------------------------------------------
// IPv4 parsing helper
// ---------------------------------------------------------------------------

/// Parse a dotted-decimal IPv4 string.
fn parse_ipv4(s: &str) -> Option<Ipv4Addr> {
    let mut octets = [0u8; 4];
    let mut idx = 0usize;
    let mut current: u16 = 0;
    let mut digit_count = 0u8;

    for &b in s.as_bytes() {
        if b == b'.' {
            if digit_count == 0 || idx >= 3 {
                return None;
            }
            if current > 255 {
                return None;
            }
            octets[idx] = current as u8;
            idx = idx.checked_add(1)?;
            current = 0;
            digit_count = 0;
        } else if b >= b'0' && b <= b'9' {
            current = current.checked_mul(10)?.checked_add((b - b'0') as u16)?;
            digit_count = digit_count.checked_add(1)?;
            if digit_count > 3 {
                return None;
            }
        } else {
            return None;
        }
    }

    if digit_count == 0 || idx != 3 || current > 255 {
        return None;
    }
    octets[3] = current as u8;

    Some(Ipv4Addr(octets))
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run NTP module self-tests.
// Self-tests deliberately runtime-assert RFC 5905 constants
// (mode codes, stratum values, port number) as living documentation.
#[allow(clippy::assertions_on_constants)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[ntp] Running NTP self-tests...");
    let mut passed = 0u32;

    // --- Test 1: NTP timestamp encode/decode round-trip ---
    {
        let ts = NtpTimestamp { seconds: 0xDEAD_BEEF, fraction: 0xCAFE_BABE };
        let mut buf = [0u8; 8];
        ts.encode(&mut buf);
        let decoded = NtpTimestamp::decode(&buf);
        assert!(decoded.seconds == ts.seconds, "seconds mismatch");
        assert!(decoded.fraction == ts.fraction, "fraction mismatch");
        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 1 (timestamp encode/decode) PASSED");
    }

    // --- Test 2: NTP timestamp to_nanos ---
    {
        let ts = NtpTimestamp { seconds: 1, fraction: 0 };
        assert!(ts.to_nanos() == 1_000_000_000, "1 second = 1e9 ns");

        let ts = NtpTimestamp { seconds: 0, fraction: 0x8000_0000 };
        // 0.5 seconds = 500_000_000 ns
        let ns = ts.to_nanos();
        let diff = ns.abs_diff(500_000_000);
        assert!(diff < 2, "0.5 seconds should be ~500M ns");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 2 (timestamp to_nanos) PASSED");
    }

    // --- Test 3: NTP timestamp from_nanos round-trip ---
    {
        let original_ns: u64 = 1_500_000_000; // 1.5 seconds
        let ts = NtpTimestamp::from_nanos(original_ns);
        assert!(ts.seconds == 1, "1.5s → seconds=1");
        let recovered = ts.to_nanos();
        let diff = recovered.abs_diff(original_ns);
        assert!(diff < 10, "round-trip should be close"); // Allow tiny rounding error

        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 3 (from_nanos round-trip) PASSED");
    }

    // --- Test 4: to_unix_secs ---
    {
        // NTP epoch + 70 years = Unix epoch 0.
        let ts = NtpTimestamp { seconds: NTP_UNIX_OFFSET as u32, fraction: 0 };
        assert!(ts.to_unix_secs() == 0, "NTP_UNIX_OFFSET → Unix 0");

        // A known date: 2024-01-01 00:00:00 UTC = Unix 1704067200
        // NTP = 1704067200 + 2208988800 = 3913056000
        let ts2 = NtpTimestamp { seconds: 3_913_056_000, fraction: 0 };
        assert!(ts2.to_unix_secs() == 1_704_067_200, "2024-01-01 UTC");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 4 (to_unix_secs) PASSED");
    }

    // --- Test 5: Packet construction ---
    {
        let ts = NtpTimestamp { seconds: 0x12345678, fraction: 0xABCDEF01 };
        let pkt = build_request(ts);

        // Byte 0: LI=0, VN=4, Mode=3 → 0x23
        assert!(pkt[0] == 0x23, "first byte should be 0x23");
        // Byte 2: poll = 6
        assert!(pkt[2] == 6, "poll should be 6");
        // Byte 3: precision = -20 = 0xEC
        assert!(pkt[3] == 0xEC, "precision should be 0xEC");

        // Transmit timestamp at bytes 40-47.
        let tx_ts = NtpTimestamp::decode(&pkt[40..48]);
        assert!(tx_ts.seconds == 0x12345678, "transmit seconds");
        assert!(tx_ts.fraction == 0xABCDEF01, "transmit fraction");

        // All other timestamps should be zero.
        let ref_ts = NtpTimestamp::decode(&pkt[16..24]);
        assert!(ref_ts.is_zero(), "reference should be zero");
        let orig_ts = NtpTimestamp::decode(&pkt[24..32]);
        assert!(orig_ts.is_zero(), "origin should be zero");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 5 (packet construction) PASSED");
    }

    // --- Test 6: Response parsing ---
    {
        let mut pkt = [0u8; 48];
        // LI=0, VN=4, Mode=4 (server) → 0x24
        pkt[0] = 0x24;
        // Stratum 2.
        pkt[1] = 2;
        // Poll 6.
        pkt[2] = 6;
        // Precision -20.
        pkt[3] = 0xEC;

        // Reference ID "GPS\0"
        pkt[12] = b'G'; pkt[13] = b'P'; pkt[14] = b'S'; pkt[15] = 0;

        // Transmit timestamp.
        let tx = NtpTimestamp { seconds: 100, fraction: 200 };
        tx.encode(&mut pkt[40..48]);

        let resp = parse_response(&pkt)?;
        assert!(resp.leap_indicator == 0, "LI should be 0");
        assert!(resp.version == 4, "version should be 4");
        assert!(resp.mode == 4, "mode should be 4 (server)");
        assert!(resp.stratum == 2, "stratum should be 2");
        assert!(resp.reference_id == [b'G', b'P', b'S', 0], "reference_id");
        assert!(resp.transmit_ts.seconds == 100, "transmit seconds");
        assert!(resp.transmit_ts.fraction == 200, "transmit fraction");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 6 (response parsing) PASSED");
    }

    // --- Test 7: KoD detection ---
    {
        let mut pkt = [0u8; 48];
        // Stratum 0 = KoD.
        pkt[0] = 0x24; // LI=0, VN=4, Mode=4
        pkt[1] = 0; // Stratum 0

        // KoD code "DENY"
        pkt[12] = b'D'; pkt[13] = b'E'; pkt[14] = b'N'; pkt[15] = b'Y';

        let origin = NtpTimestamp { seconds: 1, fraction: 2 };
        origin.encode(&mut pkt[24..32]); // Origin = our transmit.

        // Transmit must be non-zero.
        let tx = NtpTimestamp { seconds: 3, fraction: 4 };
        tx.encode(&mut pkt[40..48]);

        let resp = parse_response(&pkt)?;
        let result = validate_response(&resp, &origin);
        assert!(result.is_err(), "KoD should be rejected");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 7 (KoD detection) PASSED");
    }

    // --- Test 8: Unsynchronized LI=3 rejection ---
    {
        let mut pkt = [0u8; 48];
        // LI=3, VN=4, Mode=4 → 0b11_100_100 = 0xE4
        pkt[0] = 0xE4;
        pkt[1] = 2; // Stratum 2.
        let origin = NtpTimestamp { seconds: 10, fraction: 20 };
        origin.encode(&mut pkt[24..32]);
        let tx = NtpTimestamp { seconds: 11, fraction: 21 };
        tx.encode(&mut pkt[40..48]);

        let resp = parse_response(&pkt)?;
        let result = validate_response(&resp, &origin);
        assert!(result.is_err(), "LI=3 should be rejected");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 8 (LI=3 rejection) PASSED");
    }

    // --- Test 9: Origin mismatch rejection ---
    {
        let mut pkt = [0u8; 48];
        pkt[0] = 0x24; // LI=0, VN=4, Mode=4
        pkt[1] = 1; // Stratum 1.

        // Origin doesn't match what we sent.
        let wrong_origin = NtpTimestamp { seconds: 999, fraction: 888 };
        wrong_origin.encode(&mut pkt[24..32]);
        let tx = NtpTimestamp { seconds: 1000, fraction: 0 };
        tx.encode(&mut pkt[40..48]);

        let resp = parse_response(&pkt)?;
        let our_origin = NtpTimestamp { seconds: 1, fraction: 2 };
        let result = validate_response(&resp, &our_origin);
        assert!(result.is_err(), "origin mismatch should be rejected");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 9 (origin mismatch) PASSED");
    }

    // --- Test 10: IPv4 parsing ---
    {
        let ip = parse_ipv4("192.168.1.1");
        assert!(ip.is_some(), "valid IP");
        assert!(ip.unwrap().0 == [192, 168, 1, 1], "IP octets");

        assert!(parse_ipv4("256.0.0.0").is_none(), "out of range");
        assert!(parse_ipv4("abc").is_none(), "non-numeric");
        assert!(parse_ipv4("").is_none(), "empty");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 10 (IPv4 parsing) PASSED");
    }

    // --- Test 11: NTP_UNIX_OFFSET correctness ---
    {
        // 70 years from 1900 to 1970.
        // 17 leap years: 1904,1908,...,1968 (divisible by 4, not century except 400)
        // Total days = 70*365 + 17 = 25567
        // Total seconds = 25567 * 86400 = 2208988800
        assert!(NTP_UNIX_OFFSET == 2_208_988_800, "NTP Unix offset");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 11 (NTP_UNIX_OFFSET) PASSED");
    }

    // --- Test 12: Ephemeral port generation ---
    {
        let p1 = ephemeral_port();
        let p2 = ephemeral_port();
        assert!(p1 >= 49152, "port in ephemeral range");
        assert!(p2 >= 49152, "port in ephemeral range");
        assert!(p1 != p2, "ports should differ");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 12 (ephemeral ports) PASSED");
    }

    // --- Test 13: Server add/remove ---
    {
        // Save current state.
        let initial_count = STATE.lock().servers.len();

        let added = add_server("test.ntp.example.com");
        assert!(added, "should add server");

        let dup = add_server("test.ntp.example.com");
        assert!(!dup, "should reject duplicate");

        let count = STATE.lock().servers.len();
        assert!(count == initial_count + 1, "server count +1");

        let removed = remove_server("test.ntp.example.com");
        assert!(removed, "should remove server");

        let count2 = STATE.lock().servers.len();
        assert!(count2 == initial_count, "back to original count");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ntp]   test 13 (server add/remove) PASSED");
    }

    crate::serial_println!("[ntp] All {} self-tests PASSED", passed);
    Ok(())
}
