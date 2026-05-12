//! TCP (Transmission Control Protocol) implementation.
//!
//! Supports both client and server operation:
//!
//! - **Client**: `connect()` performs a 3-way handshake to a remote server.
//! - **Server**: `bind()` + `listen()` + `accept()` implements passive open.
//!
//! ## State machine
//!
//! ```text
//! Server path:
//!   CLOSED → LISTEN → SYN_RECEIVED → ESTABLISHED
//!
//! Client path:
//!   CLOSED → SYN_SENT → ESTABLISHED → FIN_WAIT_1 → FIN_WAIT_2 → TIME_WAIT → CLOSED
//!
//! Close (either side):
//!   ESTABLISHED → CLOSE_WAIT → LAST_ACK → CLOSED
//! ```
//!
//! ## Features
//!
//! - **Receive window tracking**: advertises dynamic window based on buffer
//!   space; tracks peer's advertised window to avoid overrunning it.
//! - **Keepalive probes** (RFC 1122 §4.2.3.6): detects dead peers on idle
//!   connections via periodic probes (`seq = snd_una - 1`).
//! - **RTT estimation** (Jacobson/Karels, RFC 6298): measures round-trip
//!   time to compute dynamic retransmission timeout (SRTT + 4×RTTVAR).
//! - **Nagle algorithm** (RFC 896): coalesces small writes into larger
//!   segments when unacknowledged data is in flight.  Disable with
//!   `set_nodelay(handle, true)` (TCP_NODELAY).
//! - **Congestion control** (simplified AIMD, RFC 5681): slow start and
//!   congestion avoidance via `cwnd`/`ssthresh`.  Multiplicative decrease
//!   on loss.
//!
//! ## Limitations
//!
//! - **Window scaling** (RFC 7323): negotiated during the 3-way handshake
//!   so that peers with large receive windows (Linux default scale 7) are
//!   interpreted correctly.  Our advertised scale is 0 (64 KiB rx buffer
//!   fits in 16 bits).
//! - No selective acknowledgment (SACK).
//! - No fast retransmit / fast recovery (triple duplicate ACK).
//! - Maximum 32 concurrent connections.
//! - Maximum 8 listeners, each with a backlog of 16 pending connections.

use alloc::vec::Vec;
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

use super::interface::{self, Ipv4Addr};
use super::ipv4::{self, Ipv4Packet, PROTO_TCP};

// ---------------------------------------------------------------------------
// TCP constants
// ---------------------------------------------------------------------------

/// TCP header size (without options).
const TCP_HEADER_SIZE: usize = 20;

/// Maximum concurrent TCP connections.
///
/// Increased from 8 to support multiple simultaneous network operations
/// (e.g., HTTP downloads, API calls, WebSocket connections).
const MAX_CONNECTIONS: usize = 32;

/// Maximum concurrent TCP listeners.
///
/// Supports multiple server services running concurrently.
const MAX_LISTENERS: usize = 8;

/// Maximum pending connections per listener (SYN backlog).
const MAX_BACKLOG: usize = 16;

/// Default receive window size (16 KiB).
const DEFAULT_WINDOW: u16 = 16384;

/// Maximum data waiting for delivery per connection.
const MAX_RX_BUFFER: usize = 65536;

/// Standard MSS for Ethernet (1500 MTU − 20 IP − 20 TCP).
const MSS: usize = 1460;

// ---------------------------------------------------------------------------
// RTT estimation (Jacobson/Karels, RFC 6298)
// ---------------------------------------------------------------------------

/// Minimum RTO in nanoseconds (200ms, per RFC 6298 §2.4 recommendation).
const RTO_MIN_NS: u64 = 200_000_000;

/// Maximum RTO in nanoseconds (60 seconds).
const RTO_MAX_NS: u64 = 60_000_000_000;

/// Initial RTO before any measurements (1 second, per RFC 6298 §2.1).
const RTO_INITIAL_NS: u64 = 1_000_000_000;

/// Alpha factor for SRTT: 1/8 (shift right by 3).  RFC 6298 §2.3.
const SRTT_ALPHA_SHIFT: u32 = 3;

/// Beta factor for RTTVAR: 1/4 (shift right by 2).  RFC 6298 §2.3.
const RTTVAR_BETA_SHIFT: u32 = 2;

// ---------------------------------------------------------------------------
// Nagle algorithm
// ---------------------------------------------------------------------------

/// Whether Nagle is enabled by default on new connections.
///
/// When enabled, small segments (< MSS) are held until all outstanding
/// data is acknowledged, reducing the number of tiny packets on the wire.
const NAGLE_DEFAULT: bool = true;

// ---------------------------------------------------------------------------
// Congestion control (simplified AIMD — RFC 5681)
// ---------------------------------------------------------------------------

/// Initial congestion window in segments (RFC 5681 §3.1 recommends
/// min(4*MSS, max(2*MSS, 4380))).  We use 4 segments for simplicity.
const INITIAL_CWND_SEGS: u32 = 4;

/// Minimum congestion window (1 segment).
const MIN_CWND_SEGS: u32 = 1;

// ---------------------------------------------------------------------------
// TCP keepalive defaults (RFC 1122 §4.2.3.6)
// ---------------------------------------------------------------------------

/// Default idle time before first keepalive probe (75 seconds).
///
/// Linux defaults to 7200s (2 hours) but that's too slow for our use case —
/// we're a lightweight OS where connections are few and detecting dead peers
/// quickly matters more than saving a few packets.
const KEEPALIVE_IDLE_DEFAULT_NS: u64 = 75_000_000_000;

/// Interval between successive keepalive probes (10 seconds).
const KEEPALIVE_INTERVAL_DEFAULT_NS: u64 = 10_000_000_000;

/// Maximum keepalive probes before declaring the connection dead.
const KEEPALIVE_PROBES_DEFAULT: u8 = 9;

// TCP flags.
const TCP_FIN: u8 = 0x01;
const TCP_SYN: u8 = 0x02;
const TCP_RST: u8 = 0x04;
const TCP_PSH: u8 = 0x08;
const TCP_ACK: u8 = 0x10;

// ---------------------------------------------------------------------------
// TCP state machine
// ---------------------------------------------------------------------------

/// TCP connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    TimeWait,
    CloseWait,
    LastAck,
}

// ---------------------------------------------------------------------------
// TCP connection
// ---------------------------------------------------------------------------

/// A TCP connection (control block).
struct TcpConnection {
    /// Whether this slot is active.
    active: bool,
    /// Connection state.
    state: TcpState,
    /// Local port.
    local_port: u16,
    /// Remote IP.
    remote_ip: Ipv4Addr,
    /// Remote port.
    remote_port: u16,
    /// Send sequence variables.
    snd_una: u32,   // Oldest unacknowledged.
    snd_nxt: u32,   // Next sequence to send.
    snd_iss: u32,   // Initial send sequence number.
    /// Peer's advertised receive window (how much data we may have in
    /// flight), *after* applying window scaling.  Updated on every
    /// incoming segment.  Stored as u32 because scaled windows can
    /// exceed 64 KiB (up to 1 GiB with scale factor 14).
    snd_wnd: u32,
    /// Receive sequence variables.
    rcv_nxt: u32,   // Next expected receive sequence.
    rcv_irs: u32,   // Initial receive sequence number.
    /// Receive buffer (data delivered in-order).
    rx_buffer: Vec<u8>,
    /// Whether the remote end has closed (FIN received).
    remote_closed: bool,
    /// Retransmit counter (incremented each poll cycle).
    retransmit_timer: u32,

    // -- RTT estimation (Jacobson/Karels, RFC 6298) --

    /// Smoothed round-trip time (nanoseconds, fixed-point ×8).
    /// Stored scaled by 8 to avoid floating-point; divide by 8 for
    /// the true SRTT.
    srtt_ns_x8: u64,
    /// RTT variation (nanoseconds, fixed-point ×4).
    /// Stored scaled by 4; divide by 4 for true RTTVAR.
    rttvar_ns_x4: u64,
    /// Current retransmission timeout (nanoseconds), derived from SRTT
    /// and RTTVAR per RFC 6298 §2.  Clamped to [RTO_MIN_NS, RTO_MAX_NS].
    rto_ns: u64,
    /// Whether we have at least one RTT measurement (first-sample init
    /// uses different formulas, per RFC 6298 §2.2).
    rtt_initialized: bool,
    /// Sequence number of the segment being timed for RTT.  Only one
    /// segment is timed at a time (Karn's algorithm: skip retransmitted).
    rtt_seq: u32,
    /// Timestamp when `rtt_seq` was sent (0 if no timing in progress).
    rtt_sent_ns: u64,

    // -- Nagle algorithm --

    /// Whether Nagle's algorithm is enabled (TCP_NODELAY = !nagle).
    nagle_enabled: bool,

    // -- Congestion control (AIMD, RFC 5681) --

    /// Congestion window in bytes.
    cwnd: u32,
    /// Slow-start threshold in bytes.
    ssthresh: u32,

    // -- Window scaling (RFC 7323) --

    /// Whether window scaling was negotiated during the handshake.
    /// Both sides must include the WScale option in their SYN for
    /// scaling to be active.
    wscale_ok: bool,
    /// Shift count to apply to the peer's advertised window field.
    /// The received window header field is left-shifted by this amount
    /// to get the true send window.  Per RFC 7323 §2.2 this is NOT
    /// applied to the window field in SYN segments themselves.
    snd_wnd_scale: u8,
    /// Shift count we advertise to the peer (right-shift our advertised
    /// window by this amount to fit in the 16-bit header field).
    /// We use 0 because our rx buffer is 64 KiB (fits in 16 bits).
    rcv_wnd_scale: u8,

    // -- Keepalive state (RFC 1122 §4.2.3.6) --

    /// Whether keepalive probes are enabled on this connection.
    keepalive_enabled: bool,
    /// Idle time (ns) before the first keepalive probe.
    keepalive_idle_ns: u64,
    /// Interval (ns) between successive probes.
    keepalive_interval_ns: u64,
    /// Maximum probes before declaring the connection dead.
    keepalive_probes_max: u8,
    /// Number of unanswered probes sent so far.
    keepalive_probes_sent: u8,
    /// Timestamp (ns, monotonic) of last data activity (send or receive).
    last_activity_ns: u64,
}

impl TcpConnection {
    const fn empty() -> Self {
        Self {
            active: false,
            state: TcpState::Closed,
            local_port: 0,
            remote_ip: Ipv4Addr::UNSPECIFIED,
            remote_port: 0,
            snd_una: 0,
            snd_nxt: 0,
            snd_iss: 0,
            snd_wnd: DEFAULT_WINDOW as u32,
            rcv_nxt: 0,
            rcv_irs: 0,
            rx_buffer: Vec::new(),
            remote_closed: false,
            retransmit_timer: 0,
            srtt_ns_x8: 0,
            rttvar_ns_x4: 0,
            rto_ns: RTO_INITIAL_NS,
            rtt_initialized: false,
            rtt_seq: 0,
            rtt_sent_ns: 0,
            nagle_enabled: NAGLE_DEFAULT,
            cwnd: INITIAL_CWND_SEGS.saturating_mul(MSS as u32),
            ssthresh: u32::MAX, // Start in slow-start.
            wscale_ok: false,
            snd_wnd_scale: 0,
            rcv_wnd_scale: 0,
            keepalive_enabled: false,
            keepalive_idle_ns: KEEPALIVE_IDLE_DEFAULT_NS,
            keepalive_interval_ns: KEEPALIVE_INTERVAL_DEFAULT_NS,
            keepalive_probes_max: KEEPALIVE_PROBES_DEFAULT,
            keepalive_probes_sent: 0,
            last_activity_ns: 0,
        }
    }
}

/// Global TCP connection table.
static CONNECTIONS: Mutex<[TcpConnection; MAX_CONNECTIONS]> = Mutex::new({
    const EMPTY: TcpConnection = TcpConnection::empty();
    [EMPTY; MAX_CONNECTIONS]
});

// ---------------------------------------------------------------------------
// TCP listener (server-side passive open)
// ---------------------------------------------------------------------------

/// A pending connection in the listener's backlog (completed 3-way handshake).
struct PendingConnection {
    /// Connection handle index in CONNECTIONS table.
    conn_handle: usize,
    /// Whether this slot is used.
    active: bool,
}

impl PendingConnection {
    const fn empty() -> Self {
        Self { conn_handle: 0, active: false }
    }
}

/// A TCP listener — bound to a local port, accepting incoming connections.
struct TcpListener {
    /// Whether this listener slot is active.
    active: bool,
    /// Local port to listen on.
    port: u16,
    /// Backlog of fully-established connections waiting to be accepted.
    backlog: [PendingConnection; MAX_BACKLOG],
}

impl TcpListener {
    const fn empty() -> Self {
        const EMPTY_PENDING: PendingConnection = PendingConnection::empty();
        Self {
            active: false,
            port: 0,
            backlog: [EMPTY_PENDING; MAX_BACKLOG],
        }
    }
}

/// Global TCP listener table.
static LISTENERS: Mutex<[TcpListener; MAX_LISTENERS]> = Mutex::new({
    const EMPTY: TcpListener = TcpListener::empty();
    [EMPTY; MAX_LISTENERS]
});

/// Next ephemeral port.
static NEXT_PORT: Mutex<u16> = Mutex::new(49200);

/// Allocate an ephemeral port.
#[allow(clippy::arithmetic_side_effects)]
fn alloc_port() -> u16 {
    let mut port = NEXT_PORT.lock();
    let p = *port;
    *port = if p >= 65000 { 49200 } else { p + 1 };
    p
}

/// Generate a cryptographically-random initial sequence number.
///
/// Uses the kernel CSPRNG (ChaCha20) to prevent ISN prediction attacks.
/// A predictable ISN allows off-path attackers to inject forged TCP
/// segments (CVE-2001-0328 and variants).
fn generate_isn() -> u32 {
    crate::rng::next_u32()
}

// ---------------------------------------------------------------------------
// TCP segment building
// ---------------------------------------------------------------------------

/// Build a TCP segment (header + payload).
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn build_segment(
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
) -> Vec<u8> {
    let header_words: u8 = 5; // 20 bytes, no options.
    let total_len = TCP_HEADER_SIZE + payload.len();
    let mut seg = Vec::with_capacity(total_len);

    // Source port.
    seg.extend_from_slice(&src_port.to_be_bytes());
    // Destination port.
    seg.extend_from_slice(&dst_port.to_be_bytes());
    // Sequence number.
    seg.extend_from_slice(&seq.to_be_bytes());
    // Acknowledgment number.
    seg.extend_from_slice(&ack.to_be_bytes());
    // Data offset (4 bits) + reserved (4 bits).
    seg.push(header_words << 4);
    // Flags.
    seg.push(flags);
    // Window size.
    seg.extend_from_slice(&window.to_be_bytes());
    // Checksum placeholder.
    let checksum_offset = seg.len();
    seg.extend_from_slice(&0u16.to_be_bytes());
    // Urgent pointer.
    seg.extend_from_slice(&0u16.to_be_bytes());
    // Payload.
    seg.extend_from_slice(payload);

    // Compute TCP checksum (includes pseudo-header).
    let checksum = tcp_checksum(&seg, src_ip, dst_ip);
    seg[checksum_offset] = (checksum >> 8) as u8;
    seg[checksum_offset + 1] = checksum as u8;

    seg
}

/// Compute TCP checksum including the IPv4 pseudo-header.
#[allow(clippy::arithmetic_side_effects)]
fn tcp_checksum(segment: &[u8], src_ip: Ipv4Addr, dst_ip: Ipv4Addr) -> u16 {
    let mut sum: u32 = 0;

    // Pseudo-header: src IP, dst IP, zero, protocol (6), TCP length.
    let pseudo = [
        src_ip.0[0], src_ip.0[1], src_ip.0[2], src_ip.0[3],
        dst_ip.0[0], dst_ip.0[1], dst_ip.0[2], dst_ip.0[3],
        0, 6, // zero + protocol TCP.
        (segment.len() >> 8) as u8, segment.len() as u8,
    ];

    for chunk in pseudo.chunks(2) {
        let word = u16::from_be_bytes([chunk[0], chunk.get(1).copied().unwrap_or(0)]);
        sum = sum.wrapping_add(u32::from(word));
    }

    // TCP segment.
    let mut i = 0;
    while i + 1 < segment.len() {
        let word = u16::from_be_bytes([segment[i], segment[i + 1]]);
        sum = sum.wrapping_add(u32::from(word));
        i += 2;
    }
    if i < segment.len() {
        sum = sum.wrapping_add(u32::from(segment[i]) << 8);
    }

    // Fold.
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF).wrapping_add(sum >> 16);
    }

    !sum as u16
}

// ---------------------------------------------------------------------------
// TCP options parsing (RFC 7323, RFC 2018, RFC 793)
// ---------------------------------------------------------------------------

/// TCP option kinds.
const TCP_OPT_END: u8 = 0;     // End of option list.
const TCP_OPT_NOP: u8 = 1;     // No-operation (padding).
const TCP_OPT_MSS: u8 = 2;     // Maximum segment size.
const TCP_OPT_WSCALE: u8 = 3;  // Window scale (RFC 7323).

/// Our advertised window scale shift count.
///
/// 0 = no scaling needed — our 64 KiB rx buffer fits in 16 bits.
/// We still *send* the option so the peer knows we understand
/// scaling and will apply its shift to their advertised window.
const OUR_WSCALE: u8 = 0;

/// Parsed TCP options from an incoming segment.
struct TcpOptions {
    /// MSS value from the peer (0 if not present).
    mss: u16,
    /// Window scale shift count (None if option not present).
    wscale: Option<u8>,
}

/// Parse TCP options from the option bytes (header bytes 20..data_offset).
///
/// Returns the parsed `TcpOptions`.  Unknown options are skipped.
/// Malformed options (truncated length, etc.) terminate parsing early.
fn parse_tcp_options(option_bytes: &[u8]) -> TcpOptions {
    let mut opts = TcpOptions { mss: 0, wscale: None };
    let mut i = 0;
    while i < option_bytes.len() {
        let kind = option_bytes[i];
        match kind {
            TCP_OPT_END => break,
            TCP_OPT_NOP => {
                i = i.wrapping_add(1);
                continue;
            }
            _ => {
                // All other options have a length byte.
                if i.wrapping_add(1) >= option_bytes.len() {
                    break; // Truncated.
                }
                let len = option_bytes[i.wrapping_add(1)] as usize;
                if len < 2 || i.wrapping_add(len) > option_bytes.len() {
                    break; // Malformed.
                }
                match kind {
                    TCP_OPT_MSS if len == 4 => {
                        opts.mss = u16::from_be_bytes([
                            option_bytes[i.wrapping_add(2)],
                            option_bytes[i.wrapping_add(3)],
                        ]);
                    }
                    TCP_OPT_WSCALE if len == 3 => {
                        // RFC 7323 §2.3: shift count MUST be ≤ 14.
                        let shift = option_bytes[i.wrapping_add(2)];
                        opts.wscale = Some(if shift > 14 { 14 } else { shift });
                    }
                    _ => {} // Unknown option — skip.
                }
                i = i.wrapping_add(len);
            }
        }
    }
    opts
}

/// Build a TCP segment with SYN options (MSS + window scale).
///
/// SYN and SYN-ACK segments carry options to negotiate parameters.
/// Layout: MSS(kind=2, len=4, value) + NOP(kind=1) + WScale(kind=3, len=3, shift) + NOP padding.
/// Total: 4 + 1 + 3 = 8 bytes → header is 28 bytes (7 words).
#[allow(clippy::arithmetic_side_effects)]
fn build_segment_with_syn_options(
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    wscale: u8,
) -> Vec<u8> {
    // Options: MSS(4) + NOP(1) + WScale(3) = 8 bytes → 28-byte header.
    let options_len = 8;
    let header_words: u8 = 7; // (20 + 8) / 4 = 7 words.
    let header_len = (header_words as usize) * 4;
    let total_len = header_len + payload.len();
    let mut seg = Vec::with_capacity(total_len);

    // Standard TCP header fields (first 20 bytes).
    seg.extend_from_slice(&src_port.to_be_bytes());
    seg.extend_from_slice(&dst_port.to_be_bytes());
    seg.extend_from_slice(&seq.to_be_bytes());
    seg.extend_from_slice(&ack.to_be_bytes());
    seg.push(header_words << 4); // Data offset.
    seg.push(flags);
    seg.extend_from_slice(&window.to_be_bytes());
    let checksum_offset = seg.len();
    seg.extend_from_slice(&0u16.to_be_bytes()); // Checksum placeholder.
    seg.extend_from_slice(&0u16.to_be_bytes()); // Urgent pointer.

    // Options: MSS.
    seg.push(TCP_OPT_MSS);
    seg.push(4); // Option length.
    seg.extend_from_slice(&(MSS as u16).to_be_bytes());

    // Options: NOP (alignment).
    seg.push(TCP_OPT_NOP);

    // Options: Window Scale.
    seg.push(TCP_OPT_WSCALE);
    seg.push(3); // Option length.
    seg.push(wscale);

    debug_assert_eq!(seg.len(), header_len, "SYN options size mismatch");

    // Payload (usually empty for SYN/SYN-ACK).
    seg.extend_from_slice(payload);

    // Compute TCP checksum.
    let checksum = tcp_checksum(&seg, src_ip, dst_ip);
    seg[checksum_offset] = (checksum >> 8) as u8;
    seg[checksum_offset + 1] = checksum as u8;

    seg
}

// ---------------------------------------------------------------------------
// TCP segment sending
// ---------------------------------------------------------------------------

/// Send a TCP segment via IP, advertising the given receive window.
fn send_segment_with_window(
    local_port: u16,
    remote_ip: Ipv4Addr,
    remote_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
) -> KernelResult<()> {
    let local_ip = interface::ip();
    let seg = build_segment(
        local_port, remote_port,
        seq, ack, flags,
        window,
        payload,
        local_ip, remote_ip,
    );

    ipv4::send(remote_ip, PROTO_TCP, &seg)
}

/// Send a TCP segment via IP, advertising the default receive window.
///
/// Convenience wrapper for control segments (SYN, RST, handshake ACKs)
/// where we don't have a connection context to compute a dynamic window.
fn send_segment(
    local_port: u16,
    remote_ip: Ipv4Addr,
    remote_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
) -> KernelResult<()> {
    send_segment_with_window(
        local_port, remote_ip, remote_port,
        seq, ack, flags, DEFAULT_WINDOW, payload,
    )
}

/// Send a SYN (or SYN-ACK) segment with TCP options (MSS + WScale).
///
/// Used during the 3-way handshake to negotiate window scaling.
fn send_syn_segment(
    local_port: u16,
    remote_ip: Ipv4Addr,
    remote_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    wscale: u8,
) -> KernelResult<()> {
    let local_ip = interface::ip();
    let seg = build_segment_with_syn_options(
        local_port, remote_port,
        seq, ack, flags,
        window,
        &[],
        local_ip, remote_ip,
        wscale,
    );

    ipv4::send(remote_ip, PROTO_TCP, &seg)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Open a TCP connection to the given address and port.
///
/// Performs the 3-way handshake (SYN → SYN-ACK → ACK).
/// Returns a connection handle on success.
#[allow(clippy::arithmetic_side_effects)]
pub fn connect(remote_ip: Ipv4Addr, remote_port: u16) -> KernelResult<usize> {
    let local_port = alloc_port();
    let isn = generate_isn();

    // Find a free slot.
    let handle = {
        let mut conns = CONNECTIONS.lock();
        let slot = conns.iter().position(|c| !c.active)
            .ok_or(KernelError::OutOfMemory)?;

        let conn = &mut conns[slot];
        conn.active = true;
        conn.state = TcpState::SynSent;
        conn.local_port = local_port;
        conn.remote_ip = remote_ip;
        conn.remote_port = remote_port;
        conn.snd_iss = isn;
        conn.snd_una = isn;
        conn.snd_nxt = isn.wrapping_add(1); // SYN consumes 1 sequence.
        conn.snd_wnd = DEFAULT_WINDOW as u32; // Refined on SYN-ACK.
        conn.rcv_nxt = 0;
        conn.rcv_irs = 0;
        conn.rx_buffer.clear();
        conn.remote_closed = false;
        conn.retransmit_timer = 0;
        conn.srtt_ns_x8 = 0;
        conn.rttvar_ns_x4 = 0;
        conn.rto_ns = RTO_INITIAL_NS;
        conn.rtt_initialized = false;
        conn.rtt_seq = isn; // Time the SYN for initial RTT sample.
        conn.rtt_sent_ns = crate::hrtimer::now_ns();
        conn.nagle_enabled = NAGLE_DEFAULT;
        conn.cwnd = INITIAL_CWND_SEGS.saturating_mul(MSS as u32);
        conn.ssthresh = u32::MAX;
        conn.wscale_ok = false; // Set on SYN-ACK if peer supports it.
        conn.snd_wnd_scale = 0;
        conn.rcv_wnd_scale = OUR_WSCALE;
        conn.keepalive_enabled = false;
        conn.keepalive_probes_sent = 0;
        conn.last_activity_ns = crate::hrtimer::now_ns();
        slot
    };

    // Send SYN with MSS + WScale options (RFC 7323).
    // The window field in the SYN itself is NOT scaled (RFC 7323 §2.2).
    send_syn_segment(
        local_port, remote_ip, remote_port,
        isn, 0, TCP_SYN, DEFAULT_WINDOW, OUR_WSCALE,
    )?;
    crate::serial_println!(
        "[tcp] SYN sent to {}:{} (seq={}, wscale={})",
        remote_ip, remote_port, isn, OUR_WSCALE
    );

    // Wait for SYN-ACK (blocking poll, up to ~5 seconds).
    for _ in 0..5000 {
        super::poll();

        let state = CONNECTIONS.lock()[handle].state;
        if state == TcpState::Established {
            crate::serial_println!("[tcp] Connection established to {}:{}", remote_ip, remote_port);
            return Ok(handle);
        }
        if state == TcpState::Closed {
            return Err(KernelError::NotSupported); // Connection refused.
        }

        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
    }

    // Timed out — clean up.
    let mut conns = CONNECTIONS.lock();
    conns[handle].active = false;
    conns[handle].state = TcpState::Closed;
    Err(KernelError::TimedOut)
}

/// Compute the dynamic receive window to advertise to the peer.
///
/// Returns the value to put in the TCP header's 16-bit window field.
/// If window scaling is active, the true window is right-shifted by
/// `rcv_wnd_scale` before being placed in the header (the peer
/// left-shifts it back).
#[allow(clippy::arithmetic_side_effects)]
fn advertised_window(conn: &TcpConnection) -> u16 {
    let free = MAX_RX_BUFFER.saturating_sub(conn.rx_buffer.len());
    // True window — up to 64 KiB for our buffer size.
    let true_wnd = free.min(u16::MAX as usize);
    // Apply receive-side scaling: divide by 2^rcv_wnd_scale for the
    // header field.  With OUR_WSCALE=0 this is a no-op.
    let scaled = true_wnd >> (conn.rcv_wnd_scale as usize);
    scaled.min(u16::MAX as usize) as u16
}

/// Update RTT estimate from a new measurement (Jacobson/Karels, RFC 6298).
///
/// `sample_ns` is the measured round-trip time in nanoseconds.
#[allow(clippy::arithmetic_side_effects)]
fn update_rtt(conn: &mut TcpConnection, sample_ns: u64) {
    if !conn.rtt_initialized {
        // First measurement (RFC 6298 §2.2):
        //   SRTT = R
        //   RTTVAR = R/2
        //   RTO = SRTT + max(G, 4*RTTVAR)  (G = clock granularity ≈ 0 for us)
        conn.srtt_ns_x8 = sample_ns.saturating_mul(8);
        conn.rttvar_ns_x4 = sample_ns.saturating_mul(2); // R/2 * 4 = 2R
        conn.rtt_initialized = true;
    } else {
        // Subsequent measurements (RFC 6298 §2.3):
        //   RTTVAR = (1 - beta) * RTTVAR + beta * |SRTT - R|
        //   SRTT   = (1 - alpha) * SRTT   + alpha * R
        //
        // With alpha = 1/8, beta = 1/4:
        //   RTTVAR_x4 = RTTVAR_x4 - (RTTVAR_x4 >> 2) + |err|
        //   SRTT_x8   = SRTT_x8   - (SRTT_x8   >> 3) + R

        let srtt = conn.srtt_ns_x8 >> SRTT_ALPHA_SHIFT; // True SRTT.
        let err = if sample_ns >= srtt {
            sample_ns - srtt
        } else {
            srtt - sample_ns
        };

        // RTTVAR update: 3/4 old + 1/4 |err|.
        conn.rttvar_ns_x4 = conn.rttvar_ns_x4
            .saturating_sub(conn.rttvar_ns_x4 >> RTTVAR_BETA_SHIFT)
            .saturating_add(err);

        // SRTT update: 7/8 old + 1/8 sample.
        conn.srtt_ns_x8 = conn.srtt_ns_x8
            .saturating_sub(conn.srtt_ns_x8 >> SRTT_ALPHA_SHIFT)
            .saturating_add(sample_ns);
    }

    // RTO = SRTT + 4 * RTTVAR, clamped.
    let srtt = conn.srtt_ns_x8 >> SRTT_ALPHA_SHIFT;
    let rttvar = conn.rttvar_ns_x4 >> RTTVAR_BETA_SHIFT;
    let rto = srtt.saturating_add(rttvar.saturating_mul(4));
    conn.rto_ns = rto.clamp(RTO_MIN_NS, RTO_MAX_NS);
}

/// Take an RTT sample if this ACK acknowledges the timed segment.
///
/// Returns true if a sample was taken.
fn try_rtt_sample(conn: &mut TcpConnection, ack: u32) -> bool {
    if conn.rtt_sent_ns == 0 {
        return false; // No timing in progress.
    }

    // Check if this ACK covers the timed sequence number.
    // ack > rtt_seq in modular arithmetic means the timed segment was ack'd.
    let covers = ack.wrapping_sub(conn.rtt_seq.wrapping_add(1)) < 0x8000_0000;
    if !covers {
        return false;
    }

    let now = crate::hrtimer::now_ns();
    let sample = now.saturating_sub(conn.rtt_sent_ns);
    update_rtt(conn, sample);

    // Clear timing state — start a new measurement on the next send.
    conn.rtt_sent_ns = 0;
    true
}

/// Start timing a segment for RTT measurement.
///
/// Only starts if no timing is currently in progress (one sample at
/// a time, per Karn's algorithm — retransmitted segments are never timed).
fn start_rtt_timing(conn: &mut TcpConnection, seq: u32) {
    if conn.rtt_sent_ns == 0 {
        conn.rtt_seq = seq;
        conn.rtt_sent_ns = crate::hrtimer::now_ns();
    }
}

/// Effective send window: min(congestion window, peer's receive window).
///
/// This is the maximum amount of data we may have in flight at once.
/// `snd_wnd` is already scaled (i.e. the true window after applying
/// the peer's window scale factor).
fn effective_window(conn: &TcpConnection) -> usize {
    let peer = conn.snd_wnd as usize;
    let cong = conn.cwnd as usize;
    peer.min(cong)
}

/// Called when an ACK arrives — updates congestion window (slow-start or
/// congestion avoidance, RFC 5681 §3.1).
#[allow(clippy::arithmetic_side_effects)]
fn on_ack_congestion(conn: &mut TcpConnection, bytes_acked: u32) {
    if bytes_acked == 0 {
        return;
    }
    let mss = MSS as u32;
    if conn.cwnd < conn.ssthresh {
        // Slow start: increase cwnd by min(bytes_acked, MSS) per ACK.
        conn.cwnd = conn.cwnd.saturating_add(bytes_acked.min(mss));
    } else {
        // Congestion avoidance: increase cwnd by MSS * MSS / cwnd per ACK
        // (approximately 1 MSS per RTT).
        let inc = (mss as u64)
            .saturating_mul(bytes_acked as u64)
            / (conn.cwnd as u64).max(1);
        conn.cwnd = conn.cwnd.saturating_add(inc.min(mss as u64) as u32);
    }
}

/// Called on packet loss (duplicate ACKs or timeout) — reduces congestion
/// window (multiplicative decrease, RFC 5681 §3.1).
fn on_loss_congestion(conn: &mut TcpConnection) {
    let mss = MSS as u32;
    // ssthresh = max(FlightSize / 2, 2*MSS)
    let flight = conn.snd_nxt.wrapping_sub(conn.snd_una);
    conn.ssthresh = (flight / 2).max(mss.saturating_mul(2));
    // cwnd = 1 MSS (enter slow start after loss).
    conn.cwnd = mss;
}

/// Send data on an established TCP connection.
///
/// Respects both the peer's advertised receive window (`snd_wnd`) and
/// the congestion window (`cwnd`).  Nagle's algorithm (when enabled)
/// delays small segments when unacknowledged data is in flight.
///
/// Returns `Ok(())` even if the effective window truncated the send.
#[allow(clippy::arithmetic_side_effects)]
pub fn send(handle: usize, data: &[u8]) -> KernelResult<()> {
    let (local_port, remote_ip, remote_port, seq, ack, eff_wnd, our_wnd, nagle, has_unacked) = {
        let conns = CONNECTIONS.lock();
        let conn = conns.get(handle).ok_or(KernelError::InvalidArgument)?;
        if !conn.active || conn.state != TcpState::Established {
            return Err(KernelError::InvalidArgument);
        }
        let unacked = conn.snd_nxt != conn.snd_una;
        (conn.local_port, conn.remote_ip, conn.remote_port,
         conn.snd_nxt, conn.rcv_nxt, effective_window(conn),
         advertised_window(conn), conn.nagle_enabled, unacked)
    };

    // Nagle's algorithm (RFC 896): if we have unacknowledged data in
    // flight, only send if the new data fills a full MSS.  This coalesces
    // small writes into larger segments.
    if nagle && has_unacked && data.len() < MSS {
        // Buffer the small write — it will be sent when the ACK arrives.
        // For now we return WouldBlock; the caller should retry later.
        // A real implementation would buffer internally, but our simple
        // model works because callers already retry on WouldBlock.
        return Err(KernelError::WouldBlock);
    }

    // Effective window = min(cwnd, snd_wnd).  Limit data to what we can
    // have in flight.
    let sendable = data.len().min(eff_wnd);

    if sendable == 0 && !data.is_empty() {
        return Err(KernelError::WouldBlock);
    }

    let send_data = &data[..sendable];

    let mut offset = 0;
    let mut first_seq = seq;

    while offset < send_data.len() {
        let chunk_end = (offset + MSS).min(send_data.len());
        let chunk = &send_data[offset..chunk_end];

        let send_seq = seq.wrapping_add(offset as u32);
        send_segment_with_window(
            local_port, remote_ip, remote_port,
            send_seq, ack,
            TCP_ACK | TCP_PSH,
            our_wnd,
            chunk,
        )?;

        if offset == 0 {
            first_seq = send_seq;
        }

        offset = chunk_end;
    }

    // Update snd_nxt, start RTT timing, and reset keepalive.
    {
        let mut conns = CONNECTIONS.lock();
        let conn = &mut conns[handle];
        conn.snd_nxt = seq.wrapping_add(sendable as u32);
        conn.last_activity_ns = crate::hrtimer::now_ns();
        conn.keepalive_probes_sent = 0;
        start_rtt_timing(conn, first_seq);
    }

    Ok(())
}

/// Enable or disable Nagle's algorithm on a connection.
///
/// `TCP_NODELAY = true` disables Nagle (nagle = false).
/// `TCP_NODELAY = false` enables Nagle (nagle = true).
pub fn set_nodelay(handle: usize, nodelay: bool) -> KernelResult<()> {
    let mut conns = CONNECTIONS.lock();
    let conn = conns.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
    if !conn.active {
        return Err(KernelError::InvalidArgument);
    }
    conn.nagle_enabled = !nodelay;
    Ok(())
}

/// Query whether Nagle's algorithm is enabled.
#[allow(dead_code)] // API — will be called from setsockopt.
pub fn get_nodelay(handle: usize) -> KernelResult<bool> {
    let conns = CONNECTIONS.lock();
    let conn = conns.get(handle).ok_or(KernelError::InvalidArgument)?;
    if !conn.active {
        return Err(KernelError::InvalidArgument);
    }
    Ok(!conn.nagle_enabled)
}

/// Query the current smoothed RTT for a connection (nanoseconds).
///
/// Returns 0 if no RTT samples have been collected yet.
#[allow(dead_code)] // Diagnostic API.
pub fn get_rtt_ns(handle: usize) -> KernelResult<u64> {
    let conns = CONNECTIONS.lock();
    let conn = conns.get(handle).ok_or(KernelError::InvalidArgument)?;
    if !conn.active {
        return Err(KernelError::InvalidArgument);
    }
    Ok(conn.srtt_ns_x8 >> SRTT_ALPHA_SHIFT)
}

/// Read data from a TCP connection.
///
/// Returns available data (may be empty if nothing received yet).
/// Call `poll()` first to process incoming packets.
pub fn read(handle: usize) -> KernelResult<Vec<u8>> {
    let mut conns = CONNECTIONS.lock();
    let conn = conns.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
    if !conn.active {
        return Err(KernelError::InvalidArgument);
    }

    // Drain the receive buffer.
    let data = core::mem::take(&mut conn.rx_buffer);
    Ok(data)
}

/// Read data from a TCP connection, blocking until data arrives or
/// the connection closes.
///
/// Returns the received data, or empty Vec if the connection closed.
pub fn read_blocking(handle: usize, timeout_polls: u32) -> KernelResult<Vec<u8>> {
    for _ in 0..timeout_polls {
        super::poll();

        {
            let conns = CONNECTIONS.lock();
            let conn = conns.get(handle).ok_or(KernelError::InvalidArgument)?;
            if !conn.active {
                return Err(KernelError::InvalidArgument);
            }
            if !conn.rx_buffer.is_empty() || conn.remote_closed {
                // Data available or connection closed.
                break;
            }
        }

        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
    }

    read(handle)
}

/// Close a TCP connection.
#[allow(clippy::arithmetic_side_effects)]
pub fn close(handle: usize) -> KernelResult<()> {
    let (local_port, remote_ip, remote_port, seq, ack, state) = {
        let conns = CONNECTIONS.lock();
        let conn = conns.get(handle).ok_or(KernelError::InvalidArgument)?;
        if !conn.active {
            return Ok(());
        }
        (conn.local_port, conn.remote_ip, conn.remote_port,
         conn.snd_nxt, conn.rcv_nxt, conn.state)
    };

    match state {
        TcpState::Established => {
            // Send FIN.
            send_segment(local_port, remote_ip, remote_port, seq, ack, TCP_FIN | TCP_ACK, &[])?;
            let mut conns = CONNECTIONS.lock();
            conns[handle].state = TcpState::FinWait1;
            conns[handle].snd_nxt = seq.wrapping_add(1);

            // Brief wait for FIN-ACK (non-blocking).
            drop(conns);
            for _ in 0..500 {
                super::poll();
                let state = CONNECTIONS.lock()[handle].state;
                if state == TcpState::Closed || state == TcpState::TimeWait {
                    break;
                }
                for _ in 0..10_000 {
                    core::hint::spin_loop();
                }
            }
        }
        TcpState::CloseWait => {
            // Remote already sent FIN; send our FIN.
            send_segment(local_port, remote_ip, remote_port, seq, ack, TCP_FIN | TCP_ACK, &[])?;
            let mut conns = CONNECTIONS.lock();
            conns[handle].state = TcpState::LastAck;
            conns[handle].snd_nxt = seq.wrapping_add(1);
        }
        _ => {}
    }

    // Force-close after timeout.
    let mut conns = CONNECTIONS.lock();
    conns[handle].active = false;
    conns[handle].state = TcpState::Closed;
    conns[handle].rx_buffer.clear();

    Ok(())
}

/// Check if a connection's remote end has closed.
pub fn is_remote_closed(handle: usize) -> bool {
    let conns = CONNECTIONS.lock();
    conns.get(handle)
        .map(|c| c.remote_closed || c.state == TcpState::CloseWait)
        .unwrap_or(true)
}

// ---------------------------------------------------------------------------
// Server API (bind / listen / accept)
// ---------------------------------------------------------------------------

/// Bind a TCP listener to a local port.
///
/// Returns a listener handle that can be used with `accept()`.
/// The listener starts accepting connections immediately.
///
/// # Errors
///
/// - `InvalidArgument` — port is 0.
/// - `AlreadyExists` — another listener is already bound to this port.
/// - `OutOfMemory` — no free listener slots.
pub fn bind(port: u16) -> KernelResult<usize> {
    if port == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let mut listeners = LISTENERS.lock();

    // Check for duplicate binding.
    for listener in listeners.iter() {
        if listener.active && listener.port == port {
            return Err(KernelError::AlreadyExists);
        }
    }

    // Find a free slot.
    let slot = listeners.iter().position(|l| !l.active)
        .ok_or(KernelError::OutOfMemory)?;

    listeners[slot].active = true;
    listeners[slot].port = port;
    // Clear backlog.
    for pending in &mut listeners[slot].backlog {
        pending.active = false;
    }

    crate::serial_println!("[tcp] Listener bound to port {}", port);
    Ok(slot)
}

/// Accept an incoming TCP connection on a listener.
///
/// Blocks (by polling) until a connection completes the 3-way handshake
/// and is placed in the listener's backlog.
///
/// Returns a connection handle (same type as `connect()` returns).
///
/// # Errors
///
/// - `InvalidArgument` — invalid listener handle.
/// - `TimedOut` — no connection arrived within the timeout.
pub fn accept(listener_handle: usize) -> KernelResult<usize> {
    // Validate the listener exists.
    {
        let listeners = LISTENERS.lock();
        let listener = listeners.get(listener_handle)
            .ok_or(KernelError::InvalidArgument)?;
        if !listener.active {
            return Err(KernelError::InvalidArgument);
        }
    }

    // Poll for an accepted connection (up to ~10 seconds).
    for _ in 0..10_000 {
        super::poll();

        // Check for a completed connection in the backlog.
        let mut listeners = LISTENERS.lock();
        let listener = &mut listeners[listener_handle];
        if !listener.active {
            return Err(KernelError::InvalidArgument);
        }

        for pending in listener.backlog.iter_mut() {
            if pending.active {
                // Found one — take it.
                let conn_handle = pending.conn_handle;
                pending.active = false;
                crate::serial_println!(
                    "[tcp] Accepted connection on port {} → handle {}",
                    listener.port, conn_handle
                );
                return Ok(conn_handle);
            }
        }

        drop(listeners);

        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
    }

    Err(KernelError::TimedOut)
}

/// Accept a connection without blocking (non-blocking variant).
///
/// Returns `Ok(handle)` if a connection is ready, or
/// `Err(WouldBlock)` if no pending connections.
pub fn try_accept(listener_handle: usize) -> KernelResult<usize> {
    let mut listeners = LISTENERS.lock();
    let listener = listeners.get_mut(listener_handle)
        .ok_or(KernelError::InvalidArgument)?;
    if !listener.active {
        return Err(KernelError::InvalidArgument);
    }

    for pending in listener.backlog.iter_mut() {
        if pending.active {
            let conn_handle = pending.conn_handle;
            pending.active = false;
            return Ok(conn_handle);
        }
    }

    Err(KernelError::WouldBlock)
}

/// Close a TCP listener, freeing its port.
///
/// Any pending (not yet accepted) connections in the backlog are
/// reset (RST sent to the remote).
pub fn close_listener(listener_handle: usize) -> KernelResult<()> {
    let mut listeners = LISTENERS.lock();
    let listener = listeners.get_mut(listener_handle)
        .ok_or(KernelError::InvalidArgument)?;
    if !listener.active {
        return Ok(());
    }

    let port = listener.port;

    // Close any pending connections that haven't been accepted yet.
    let pending_handles: [Option<usize>; MAX_BACKLOG] = {
        let mut handles = [None; MAX_BACKLOG];
        for (i, pending) in listener.backlog.iter_mut().enumerate() {
            if pending.active {
                handles[i] = Some(pending.conn_handle);
                pending.active = false;
            }
        }
        handles
    };

    listener.active = false;
    listener.port = 0;
    drop(listeners);

    // Reset unaccepted connections.
    let mut conns = CONNECTIONS.lock();
    for handle in pending_handles.into_iter().flatten() {
        if let Some(conn) = conns.get_mut(handle) {
            if conn.active {
                conn.active = false;
                conn.state = TcpState::Closed;
                conn.rx_buffer.clear();
            }
        }
    }
    drop(conns);

    crate::serial_println!("[tcp] Listener on port {} closed", port);
    Ok(())
}

// ---------------------------------------------------------------------------
// Server-side passive open helpers
// ---------------------------------------------------------------------------

/// Handle an incoming SYN for a listening port (passive open step 1).
///
/// If a listener is bound on `dst_port`, allocate a new connection in
/// `SynReceived` state, send SYN-ACK.  When the ACK arrives, the
/// connection transitions to `Established` and is queued to the
/// listener's backlog for `accept()`.
///
/// `option_bytes` contains the TCP options from the SYN segment (bytes
/// 20..data_offset of the TCP header).  Used to parse window scale.
#[allow(clippy::arithmetic_side_effects)]
fn handle_incoming_syn(
    remote_ip: Ipv4Addr,
    remote_port: u16,
    local_port: u16,
    remote_seq: u32,
    remote_window: u16,
    option_bytes: &[u8],
) -> KernelResult<()> {
    // Check if we have a listener for this port.
    let listener_exists = {
        let listeners = LISTENERS.lock();
        listeners.iter().any(|l| l.active && l.port == local_port)
    };

    if !listener_exists {
        // No listener — send RST.
        let rst_ack = remote_seq.wrapping_add(1);
        let _ = send_segment(
            local_port, remote_ip, remote_port,
            0, rst_ack, TCP_RST | TCP_ACK, &[],
        );
        return Ok(());
    }

    // Parse TCP options from the SYN.
    let syn_opts = parse_tcp_options(option_bytes);

    // Allocate a connection slot for this incoming connection.
    let isn = generate_isn();
    let handle = {
        let mut conns = CONNECTIONS.lock();
        let slot = conns.iter().position(|c| !c.active)
            .ok_or(KernelError::OutOfMemory)?;

        let conn = &mut conns[slot];
        conn.active = true;
        conn.state = TcpState::SynReceived;
        conn.local_port = local_port;
        conn.remote_ip = remote_ip;
        conn.remote_port = remote_port;
        conn.snd_iss = isn;
        conn.snd_una = isn;
        conn.snd_nxt = isn.wrapping_add(1); // SYN-ACK consumes 1 seq.
        // RFC 7323 §2.2: The window field in a SYN is never scaled.
        // Store unscaled for now; scaling applies after Established.
        conn.snd_wnd = remote_window as u32;
        conn.rcv_irs = remote_seq;
        conn.rcv_nxt = remote_seq.wrapping_add(1); // Client's SYN consumes 1.
        conn.rx_buffer.clear();
        conn.remote_closed = false;
        conn.retransmit_timer = 0;
        conn.srtt_ns_x8 = 0;
        conn.rttvar_ns_x4 = 0;
        conn.rto_ns = RTO_INITIAL_NS;
        conn.rtt_initialized = false;
        conn.rtt_seq = isn; // Time the SYN-ACK for initial RTT sample.
        conn.rtt_sent_ns = crate::hrtimer::now_ns();
        conn.nagle_enabled = NAGLE_DEFAULT;
        conn.cwnd = INITIAL_CWND_SEGS.saturating_mul(MSS as u32);
        conn.ssthresh = u32::MAX;

        // Window scaling (RFC 7323): both sides must include WScale in
        // their SYN for scaling to be active.
        if let Some(peer_shift) = syn_opts.wscale {
            conn.wscale_ok = true;
            conn.snd_wnd_scale = peer_shift;
            conn.rcv_wnd_scale = OUR_WSCALE;
        } else {
            conn.wscale_ok = false;
            conn.snd_wnd_scale = 0;
            conn.rcv_wnd_scale = 0;
        }

        conn.keepalive_enabled = false;
        conn.keepalive_probes_sent = 0;
        conn.last_activity_ns = crate::hrtimer::now_ns();
        slot
    };

    // Send SYN-ACK with WScale option (only if the client sent one).
    let rcv_nxt = remote_seq.wrapping_add(1);
    if syn_opts.wscale.is_some() {
        send_syn_segment(
            local_port, remote_ip, remote_port,
            isn, rcv_nxt, TCP_SYN | TCP_ACK,
            DEFAULT_WINDOW, OUR_WSCALE,
        )?;
    } else {
        // Client doesn't support window scaling — plain SYN-ACK.
        send_segment(
            local_port, remote_ip, remote_port,
            isn, rcv_nxt, TCP_SYN | TCP_ACK, &[],
        )?;
    }

    crate::serial_println!(
        "[tcp] SYN-ACK sent to {}:{} (handle={}, isn={}, wscale={})",
        remote_ip, remote_port, handle, isn,
        if syn_opts.wscale.is_some() { OUR_WSCALE } else { 0 }
    );

    Ok(())
}

/// Place a fully-established connection into its listener's backlog.
fn enqueue_to_listener(local_port: u16, conn_handle: usize) {
    let mut listeners = LISTENERS.lock();
    for listener in listeners.iter_mut() {
        if listener.active && listener.port == local_port {
            // Find a free backlog slot.
            for pending in listener.backlog.iter_mut() {
                if !pending.active {
                    pending.active = true;
                    pending.conn_handle = conn_handle;
                    return;
                }
            }
            // Backlog full — drop (connection will time out).
            crate::serial_println!(
                "[tcp] Warning: backlog full on port {}, dropping connection",
                local_port
            );
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// TCP segment processing (called from IPv4 layer)
// ---------------------------------------------------------------------------

/// Process an incoming TCP segment.
#[allow(clippy::arithmetic_side_effects)]
pub fn process_tcp(ip_packet: &Ipv4Packet<'_>) -> KernelResult<()> {
    let data = ip_packet.payload;
    if data.len() < TCP_HEADER_SIZE {
        return Ok(());
    }

    // Verify TCP checksum (pseudo-header + segment).
    if !ipv4::verify_transport_checksum(
        ip_packet.src, ip_packet.dst, PROTO_TCP, data,
    ) {
        crate::serial_println!(
            "[tcp] Dropped segment from {} — bad checksum",
            ip_packet.src
        );
        return Ok(());
    }

    let src_port = u16::from_be_bytes([data[0], data[1]]);
    let dst_port = u16::from_be_bytes([data[2], data[3]]);
    let seq = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let ack = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let data_offset = ((data[12] >> 4) as usize) * 4;
    let flags = data[13];
    let window = u16::from_be_bytes([data[14], data[15]]);

    // TCP options sit between the fixed header and the payload.
    let option_bytes = if data_offset > TCP_HEADER_SIZE && data_offset <= data.len() {
        &data[TCP_HEADER_SIZE..data_offset]
    } else {
        &[]
    };

    let payload = if data_offset < data.len() {
        &data[data_offset..]
    } else {
        &[]
    };

    // Find matching connection.
    let mut conns = CONNECTIONS.lock();
    let conn_idx = conns.iter().position(|c| {
        c.active
            && c.local_port == dst_port
            && c.remote_ip == ip_packet.src
            && c.remote_port == src_port
    });

    let Some(idx) = conn_idx else {
        // No matching connection — check if this is a SYN for a listener
        // (passive open / server-side handshake).
        if flags & TCP_SYN != 0 && flags & TCP_ACK == 0 && flags & TCP_RST == 0 {
            drop(conns);
            return handle_incoming_syn(
                ip_packet.src, src_port, dst_port, seq, window,
                option_bytes,
            );
        }
        // No matching connection and not a SYN for a listener.
        // Send RST if this isn't itself a RST.
        if flags & TCP_RST == 0 {
            drop(conns);
            let rst_seq = if flags & TCP_ACK != 0 { ack } else { 0 };
            let rst_ack = seq.wrapping_add(payload.len() as u32);
            let _ = send_segment(
                dst_port, ip_packet.src, src_port,
                rst_seq, rst_ack, TCP_RST | TCP_ACK, &[],
            );
        }
        return Ok(());
    };

    let conn = &mut conns[idx];

    // Update peer's advertised receive window on every segment.
    // RFC 7323 §2.2: The window field in a SYN segment is never scaled.
    // Scaling applies only after the handshake, once both sides have
    // exchanged and agreed on window scale factors.
    if flags & TCP_SYN != 0 {
        // SYN or SYN-ACK — store unscaled.
        conn.snd_wnd = window as u32;
    } else if conn.wscale_ok {
        // Post-handshake with negotiated scaling: apply peer's shift.
        conn.snd_wnd = (window as u32) << (conn.snd_wnd_scale as u32);
    } else {
        // No scaling negotiated — use raw 16-bit value.
        conn.snd_wnd = window as u32;
    }

    // Any incoming segment counts as activity for keepalive purposes.
    conn.last_activity_ns = crate::hrtimer::now_ns();
    conn.keepalive_probes_sent = 0;

    // Handle RST.
    if flags & TCP_RST != 0 {
        crate::serial_println!("[tcp] RST received — connection reset");
        conn.active = false;
        conn.state = TcpState::Closed;
        return Ok(());
    }

    match conn.state {
        TcpState::SynSent => {
            // Expecting SYN-ACK.
            if flags & TCP_SYN != 0 && flags & TCP_ACK != 0 {
                conn.rcv_irs = seq;
                conn.rcv_nxt = seq.wrapping_add(1);
                conn.snd_una = ack;

                // Parse SYN-ACK options for window scaling (RFC 7323).
                // Both sides must have sent WScale for scaling to be active.
                let synack_opts = parse_tcp_options(option_bytes);
                if let Some(peer_shift) = synack_opts.wscale {
                    conn.wscale_ok = true;
                    conn.snd_wnd_scale = peer_shift;
                    // rcv_wnd_scale already set to OUR_WSCALE in connect().
                    crate::serial_println!(
                        "[tcp] Window scaling negotiated: snd_shift={}, rcv_shift={}",
                        peer_shift, conn.rcv_wnd_scale
                    );
                } else {
                    // Peer doesn't support window scaling — disable it.
                    conn.wscale_ok = false;
                    conn.snd_wnd_scale = 0;
                    conn.rcv_wnd_scale = 0;
                }

                conn.state = TcpState::Established;

                // Take initial RTT sample from SYN → SYN-ACK.
                try_rtt_sample(conn, ack);

                // Send ACK.
                let local_port = conn.local_port;
                let remote_ip = conn.remote_ip;
                let remote_port = conn.remote_port;
                let snd_nxt = conn.snd_nxt;
                let rcv_nxt = conn.rcv_nxt;

                drop(conns);
                let _ = send_segment(
                    local_port, remote_ip, remote_port,
                    snd_nxt, rcv_nxt, TCP_ACK, &[],
                );
            }
        }

        TcpState::SynReceived => {
            // Server-side: waiting for ACK to complete 3-way handshake.
            if flags & TCP_ACK != 0 {
                // Take initial RTT sample from SYN-ACK → ACK.
                try_rtt_sample(conn, ack);
                conn.snd_una = ack;
                conn.state = TcpState::Established;

                let local_port = conn.local_port;

                // Place this connection in the listener's backlog.
                drop(conns);
                enqueue_to_listener(local_port, idx);

                crate::serial_println!(
                    "[tcp] 3-way handshake complete for {}:{} → port {}",
                    ip_packet.src, src_port, local_port
                );
            } else if flags & TCP_RST != 0 {
                conn.active = false;
                conn.state = TcpState::Closed;
            }
        }

        TcpState::Established => {
            // Process ACK.
            if flags & TCP_ACK != 0 {
                let old_una = conn.snd_una;
                // Advance snd_una.
                if ack.wrapping_sub(conn.snd_una) <= conn.snd_nxt.wrapping_sub(conn.snd_una) {
                    conn.snd_una = ack;
                }
                let bytes_acked = conn.snd_una.wrapping_sub(old_una);

                // RTT sample from this ACK (Karn: only non-retransmitted).
                try_rtt_sample(conn, ack);

                // Congestion window update.
                on_ack_congestion(conn, bytes_acked);
            }

            // Process data.
            if !payload.is_empty() && seq == conn.rcv_nxt {
                let can_accept = MAX_RX_BUFFER.saturating_sub(conn.rx_buffer.len());
                let accept = payload.len().min(can_accept);
                if accept > 0 {
                    conn.rx_buffer.extend_from_slice(&payload[..accept]);
                    conn.rcv_nxt = conn.rcv_nxt.wrapping_add(accept as u32);
                }

                // Send ACK with dynamic window reflecting our available
                // buffer space.
                let local_port = conn.local_port;
                let remote_ip = conn.remote_ip;
                let remote_port = conn.remote_port;
                let snd_nxt = conn.snd_nxt;
                let rcv_nxt = conn.rcv_nxt;
                let our_wnd = advertised_window(conn);

                drop(conns);
                let _ = send_segment_with_window(
                    local_port, remote_ip, remote_port,
                    snd_nxt, rcv_nxt, TCP_ACK, our_wnd, &[],
                );
                return Ok(());
            }

            // Process FIN.
            if flags & TCP_FIN != 0 {
                conn.rcv_nxt = conn.rcv_nxt.wrapping_add(1);
                conn.remote_closed = true;
                conn.state = TcpState::CloseWait;

                // Send ACK for FIN.
                let local_port = conn.local_port;
                let remote_ip = conn.remote_ip;
                let remote_port = conn.remote_port;
                let snd_nxt = conn.snd_nxt;
                let rcv_nxt = conn.rcv_nxt;

                drop(conns);
                let _ = send_segment(
                    local_port, remote_ip, remote_port,
                    snd_nxt, rcv_nxt, TCP_ACK, &[],
                );
                return Ok(());
            }
        }

        TcpState::FinWait1 => {
            if flags & TCP_ACK != 0 {
                conn.snd_una = ack;
                if flags & TCP_FIN != 0 {
                    // Simultaneous close: FIN+ACK.
                    conn.rcv_nxt = seq.wrapping_add(1);
                    conn.state = TcpState::TimeWait;
                    conn.remote_closed = true;

                    let local_port = conn.local_port;
                    let remote_ip = conn.remote_ip;
                    let remote_port = conn.remote_port;
                    let snd_nxt = conn.snd_nxt;
                    let rcv_nxt = conn.rcv_nxt;

                    drop(conns);
                    let _ = send_segment(
                        local_port, remote_ip, remote_port,
                        snd_nxt, rcv_nxt, TCP_ACK, &[],
                    );
                } else {
                    conn.state = TcpState::FinWait2;
                }
            }
        }

        TcpState::FinWait2 => {
            if flags & TCP_FIN != 0 {
                conn.rcv_nxt = seq.wrapping_add(1);
                conn.state = TcpState::TimeWait;
                conn.remote_closed = true;

                let local_port = conn.local_port;
                let remote_ip = conn.remote_ip;
                let remote_port = conn.remote_port;
                let snd_nxt = conn.snd_nxt;
                let rcv_nxt = conn.rcv_nxt;

                drop(conns);
                let _ = send_segment(
                    local_port, remote_ip, remote_port,
                    snd_nxt, rcv_nxt, TCP_ACK, &[],
                );
            }
        }

        TcpState::LastAck => {
            if flags & TCP_ACK != 0 {
                conn.active = false;
                conn.state = TcpState::Closed;
            }
        }

        _ => {}
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Keepalive probing (RFC 1122 §4.2.3.6)
// ---------------------------------------------------------------------------

/// Enable or disable TCP keepalive on a connection.
///
/// When enabled, the stack sends keepalive probes (zero-payload ACK with
/// `seq = snd_una - 1`) after the connection has been idle for
/// `keepalive_idle` nanoseconds.  If no response arrives after
/// `keepalive_probes_max` consecutive probes spaced `keepalive_interval`
/// apart, the connection is reset.
///
/// Keepalive parameters use compiled-in defaults (75s idle, 10s interval,
/// 9 probes).  To customise, use `set_keepalive_params()`.
pub fn set_keepalive(handle: usize, enabled: bool) -> KernelResult<()> {
    let mut conns = CONNECTIONS.lock();
    let conn = conns.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
    if !conn.active {
        return Err(KernelError::InvalidArgument);
    }
    conn.keepalive_enabled = enabled;
    if enabled {
        // Reset probe state so we start fresh from the current time.
        conn.keepalive_probes_sent = 0;
        conn.last_activity_ns = crate::hrtimer::now_ns();
    }
    Ok(())
}

/// Set keepalive timing parameters on a connection.
///
/// - `idle_ns`: time of inactivity before the first probe.
/// - `interval_ns`: time between successive probes.
/// - `max_probes`: probes sent before declaring the peer dead.
pub fn set_keepalive_params(
    handle: usize,
    idle_ns: u64,
    interval_ns: u64,
    max_probes: u8,
) -> KernelResult<()> {
    let mut conns = CONNECTIONS.lock();
    let conn = conns.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
    if !conn.active {
        return Err(KernelError::InvalidArgument);
    }
    conn.keepalive_idle_ns = idle_ns;
    conn.keepalive_interval_ns = interval_ns;
    conn.keepalive_probes_max = max_probes;
    Ok(())
}

/// Periodic keepalive tick — call from the network timer (e.g. softirq
/// or poll loop).
///
/// Scans all established connections with keepalive enabled.  For each
/// idle connection, either sends a probe or declares the connection dead
/// if the probe limit has been reached.
#[allow(clippy::arithmetic_side_effects)]
pub fn tick_keepalive() {
    let now = crate::hrtimer::now_ns();
    let mut conns = CONNECTIONS.lock();

    for idx in 0..MAX_CONNECTIONS {
        let conn = &mut conns[idx];
        if !conn.active
            || conn.state != TcpState::Established
            || !conn.keepalive_enabled
        {
            continue;
        }

        let elapsed = now.saturating_sub(conn.last_activity_ns);

        // Determine the threshold for the *next* action:
        //  - If no probes sent yet: must exceed idle_ns.
        //  - Otherwise: must exceed idle_ns + probes_sent * interval_ns.
        let threshold = conn.keepalive_idle_ns.saturating_add(
            (conn.keepalive_probes_sent as u64)
                .saturating_mul(conn.keepalive_interval_ns),
        );

        if elapsed < threshold {
            continue;
        }

        // Check if we've exhausted all probes.
        if conn.keepalive_probes_sent >= conn.keepalive_probes_max {
            crate::serial_println!(
                "[tcp] Keepalive timeout — connection {}:{} → {}:{} dead after {} probes",
                conn.local_port, conn.remote_port,
                conn.remote_ip, conn.remote_port,
                conn.keepalive_probes_max
            );

            // Send RST and tear down.
            let local_port = conn.local_port;
            let remote_ip = conn.remote_ip;
            let remote_port = conn.remote_port;
            let snd_nxt = conn.snd_nxt;
            let rcv_nxt = conn.rcv_nxt;

            conn.active = false;
            conn.state = TcpState::Closed;
            conn.rx_buffer.clear();

            drop(conns);
            let _ = send_segment(
                local_port, remote_ip, remote_port,
                snd_nxt, rcv_nxt, TCP_RST | TCP_ACK, &[],
            );
            return; // Dropped lock; restart scan next tick.
        }

        // Send a keepalive probe: ACK with seq = snd_una - 1, no payload.
        // This intentionally uses an out-of-window sequence number so the
        // peer responds with an ACK containing its current window/ack,
        // confirming liveness.
        let local_port = conn.local_port;
        let remote_ip = conn.remote_ip;
        let remote_port = conn.remote_port;
        let probe_seq = conn.snd_una.wrapping_sub(1);
        let rcv_nxt = conn.rcv_nxt;
        let our_wnd = advertised_window(conn);

        conn.keepalive_probes_sent = conn.keepalive_probes_sent.saturating_add(1);

        crate::serial_println!(
            "[tcp] Keepalive probe #{} for {}:{} (idle {}ms)",
            conn.keepalive_probes_sent,
            remote_ip, remote_port,
            elapsed / 1_000_000
        );

        // Must drop the lock before sending (send_segment acquires
        // interface locks).
        drop(conns);
        let _ = send_segment_with_window(
            local_port, remote_ip, remote_port,
            probe_seq, rcv_nxt, TCP_ACK, our_wnd, &[],
        );
        return; // Dropped lock; restart scan next tick.
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// TCP self-test: validates listener bind/close lifecycle and basic
/// state management without requiring actual network traffic.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[tcp] Running TCP self-test...");

    test_bind_close()?;
    test_bind_duplicate_rejected()?;
    test_try_accept_empty()?;
    test_parse_tcp_options()?;

    crate::serial_println!("[tcp] TCP self-test PASSED");
    Ok(())
}

/// Test 1: bind and close a listener.
fn test_bind_close() -> KernelResult<()> {
    let handle = bind(9999)?;

    // Verify listener is active.
    {
        let listeners = LISTENERS.lock();
        if !listeners[handle].active || listeners[handle].port != 9999 {
            crate::serial_println!("[tcp]   FAIL: listener not active or wrong port");
            return Err(KernelError::InternalError);
        }
    }

    close_listener(handle)?;

    // Verify listener is freed.
    {
        let listeners = LISTENERS.lock();
        if listeners[handle].active {
            crate::serial_println!("[tcp]   FAIL: listener still active after close");
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[tcp]   Bind + close: OK");
    Ok(())
}

/// Test 2: binding the same port twice is rejected.
fn test_bind_duplicate_rejected() -> KernelResult<()> {
    let handle = bind(8888)?;

    // Try to bind the same port again — should fail.
    match bind(8888) {
        Err(KernelError::AlreadyExists) => {}
        other => {
            close_listener(handle).ok();
            crate::serial_println!(
                "[tcp]   FAIL: duplicate bind returned {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    // After close, rebind should succeed.
    close_listener(handle)?;
    let handle2 = bind(8888)?;
    close_listener(handle2)?;

    crate::serial_println!("[tcp]   Duplicate bind rejected: OK");
    Ok(())
}

/// Test 3: try_accept on empty backlog returns WouldBlock.
fn test_try_accept_empty() -> KernelResult<()> {
    let handle = bind(7777)?;

    match try_accept(handle) {
        Err(KernelError::WouldBlock) => {}
        other => {
            close_listener(handle).ok();
            crate::serial_println!(
                "[tcp]   FAIL: try_accept on empty returned {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    close_listener(handle)?;
    crate::serial_println!("[tcp]   try_accept empty → WouldBlock: OK");
    Ok(())
}

/// Test 4: TCP option parsing (MSS, WScale, NOP, END).
fn test_parse_tcp_options() -> KernelResult<()> {
    // Empty options.
    let opts = parse_tcp_options(&[]);
    if opts.mss != 0 || opts.wscale.is_some() {
        crate::serial_println!("[tcp]   FAIL: empty options not zeroed");
        return Err(KernelError::InternalError);
    }

    // MSS only: kind=2, len=4, MSS=1460 (0x05B4).
    let mss_opt = [2, 4, 0x05, 0xB4];
    let opts = parse_tcp_options(&mss_opt);
    if opts.mss != 1460 {
        crate::serial_println!("[tcp]   FAIL: MSS parse expected 1460, got {}", opts.mss);
        return Err(KernelError::InternalError);
    }
    if opts.wscale.is_some() {
        crate::serial_println!("[tcp]   FAIL: wscale should be None for MSS-only");
        return Err(KernelError::InternalError);
    }

    // WScale only: kind=3, len=3, shift=7.
    let wscale_opt = [3, 3, 7];
    let opts = parse_tcp_options(&wscale_opt);
    if opts.wscale != Some(7) {
        crate::serial_println!("[tcp]   FAIL: wscale expected Some(7), got {:?}", opts.wscale);
        return Err(KernelError::InternalError);
    }

    // Combined options like a real Linux SYN: MSS + NOP + WScale + END.
    // MSS(2,4,05,B4) + NOP(1) + WScale(3,3,7) + END(0)
    let combined = [2, 4, 0x05, 0xB4, 1, 3, 3, 7, 0];
    let opts = parse_tcp_options(&combined);
    if opts.mss != 1460 {
        crate::serial_println!("[tcp]   FAIL: combined MSS expected 1460, got {}", opts.mss);
        return Err(KernelError::InternalError);
    }
    if opts.wscale != Some(7) {
        crate::serial_println!("[tcp]   FAIL: combined wscale expected Some(7), got {:?}", opts.wscale);
        return Err(KernelError::InternalError);
    }

    // WScale clamped to 14 per RFC 7323 §2.3.
    let big_wscale = [3, 3, 15];
    let opts = parse_tcp_options(&big_wscale);
    if opts.wscale != Some(14) {
        crate::serial_println!("[tcp]   FAIL: wscale clamp expected Some(14), got {:?}", opts.wscale);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[tcp]   TCP option parsing: OK");
    Ok(())
}
