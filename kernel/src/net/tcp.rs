//! TCP (Transmission Control Protocol) implementation — dual-stack IPv4/IPv6.
//!
//! Supports both client and server operation over either IPv4 or IPv6:
//!
//! - **Client**: `connect()` performs a 3-way handshake to a remote server.
//! - **Server**: `bind()` + `listen()` + `accept()` implements passive open.
//!
//! ## Dual-stack architecture
//!
//! Connections store the remote address as `IpAddr` (IPv4 or IPv6).
//! The shared TCP state machine (`process_tcp_common`) handles both
//! address families identically.  Address-family-specific logic is
//! confined to:
//!
//! - **Checksum**: IPv4 pseudo-header (12 bytes) vs IPv6 pseudo-header
//!   (40 bytes) — dispatched by `tcp_checksum_ip()`.
//! - **Send path**: `ip_send_tcp()` dispatches to `ipv4::send_ecn()` or
//!   `ipv6::send_raw()` based on the destination address type.
//! - **Receive path**: `process_tcp()` (IPv4 entry) and
//!   `process_tcp_v6()` (IPv6 entry) verify the appropriate checksum
//!   and delegate to `process_tcp_common()`.
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
//!   segments when unacknowledged data is in flight.  Small writes
//!   are buffered internally (`nagle_buf`) and flushed when an ACK
//!   acknowledges all outstanding data or when enough data accumulates
//!   to fill an MSS.  Disable with `set_nodelay(handle, true)` (TCP_NODELAY).
//! - **Congestion control** (simplified AIMD, RFC 5681): slow start and
//!   congestion avoidance via `cwnd`/`ssthresh`.  Multiplicative decrease
//!   on loss.
//! - **Zero window probing** (persist timer, RFC 1122 §4.2.2.17): when the
//!   peer advertises a zero receive window, the persist timer sends periodic
//!   probes with exponential backoff (500ms → 60s).  This prevents permanent
//!   deadlock when the receiver's window update ACK is lost.
//! - **ECN** (Explicit Congestion Notification, RFC 3168): negotiated in the
//!   3-way handshake via ECE+CWR flags (both client and server).  When a
//!   router CE-marks an IP packet, the receiver echoes ECE in ACKs; the
//!   sender reduces cwnd (like a loss event) and sets CWR.  IP datagrams
//!   on ECN-negotiated connections are marked ECT(0) so routers can signal
//!   congestion without dropping packets.
//!
//! - **Timestamps** (RFC 7323 §3-4): negotiated in the 3-way handshake via
//!   the Timestamp option (kind=8, len=10).  Every segment carries TSval
//!   (our millisecond clock) and TSecr (echoed peer clock).  Provides per-ACK
//!   RTT measurement (instead of timing one segment per flight) and PAWS
//!   (Protection Against Wrapped Sequence numbers) which drops old duplicate
//!   segments with stale timestamps.  When both timestamps and SACK are
//!   active, SACK is limited to 3 blocks (12 + 28 ≤ 40 byte option space).
//!
//! - **Window scaling** (RFC 7323): negotiated during the 3-way handshake
//!   so that peers with large receive windows (Linux default scale 7) are
//!   interpreted correctly.  Our advertised scale is 0 (64 KiB rx buffer
//!   fits in 16 bits).
//! - **Selective acknowledgment** (SACK, RFC 2018): negotiated via
//!   SACK-Permitted option in SYN.  Receiver tracks up to 4 out-of-order
//!   blocks and reports them in ACKs.  Out-of-order data is buffered in
//!   `ooo_buf` and delivered to `rx_buffer` when the gap fills — no need
//!   to wait for retransmission of already-received data.
//!
//! ## ICMP error handling
//!
//! ICMP Destination Unreachable and Time Exceeded errors are dispatched
//! to `icmp_error()` by the ICMP handler.  Per RFC 5461, only `SynSent`
//! and `SynReceived` connections are aborted (hard errors); established
//! connections treat ICMP errors as soft errors (logged but not aborted).
//!
//! ## Path MTU Discovery (RFC 1191)
//!
//! When an ICMP "Fragmentation Needed" (type 3, code 4) carries a
//! next-hop MTU, the connection's `peer_mss` is reduced to
//! `MTU - 40` (IP + TCP headers).  This ensures future segments fit
//! within the path MTU without IP-level fragmentation.  Combined
//! with the per-connection `peer_mss` / `effective_mss()`, all
//! subsequent data segments use the reduced MSS automatically.
//!
//! ## Retransmit buffer
//!
//! Sent data is copied into a per-connection retransmit buffer (up to 64 KiB).
//! This enables:
//! - **Fast retransmit** (RFC 5681 §3.2): after 3 duplicate ACKs, the
//!   first unacknowledged segment is resent immediately from the buffer.
//! - **RTO retransmit** (RFC 6298): when the retransmission timeout expires
//!   without receiving an ACK, the oldest unacknowledged data is resent
//!   with exponential backoff (doubled RTO per timeout, capped at 60s).
//! - The buffer is trimmed from the front when ACKs advance `snd_una`,
//!   keeping memory usage proportional to in-flight data.
//!
//! ## Duplicate ACK / Fast Recovery (RFC 5681 §3.2)
//!
//! Counts consecutive duplicate ACKs.  After 3 duplicate ACKs, enters
//! fast recovery: `ssthresh = flight_size / 2`, `cwnd = ssthresh + 3*MSS`,
//! and retransmits the first unacknowledged segment from the retransmit
//! buffer.  Additional dup ACKs inflate `cwnd` by one MSS.
//!
//! ## Limitations
//!
//! - Maximum 32 concurrent connections.
//! - Maximum 8 listeners, each with a backlog of 16 pending connections.

use alloc::vec::Vec;
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

use super::interface::{self, IpAddr, Ipv4Addr};
use super::ipv4::{self, Ipv4Packet, PROTO_TCP};
use super::ipv6::{self, Ipv6Addr, Ipv6Packet};
use crate::netns::NetNsId;

// ---------------------------------------------------------------------------
// TCP constants
// ---------------------------------------------------------------------------

/// TCP header size (without options).
const TCP_HEADER_SIZE: usize = 20;

/// Maximum out-of-order segments tracked per connection for SACK.
/// Each entry is a (left_edge, right_edge) pair describing a contiguous
/// block of received data above `rcv_nxt`.  4 blocks is the practical
/// maximum we can fit in TCP options (each SACK block = 8 bytes, plus
/// 2 bytes for the option header, limited by 40 bytes total options).
const MAX_SACK_BLOCKS: usize = 4;

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

/// Maximum retransmit buffer size per connection.
///
/// Stores copies of sent-but-unacknowledged data so that fast retransmit
/// (3 dup ACKs) and timeout retransmit can resend lost segments.
const MAX_TX_BUFFER: usize = 65536;

// ---------------------------------------------------------------------------
// RTT estimation (Jacobson/Karels, RFC 6298)
// ---------------------------------------------------------------------------

/// Minimum RTO in nanoseconds (200ms, per RFC 6298 §2.4 recommendation).
const RTO_MIN_NS: u64 = 200_000_000;

/// Maximum RTO in nanoseconds (60 seconds).
const RTO_MAX_NS: u64 = 60_000_000_000;

/// Initial RTO before any measurements (1 second, per RFC 6298 §2.1).
const RTO_INITIAL_NS: u64 = 1_000_000_000;

/// Maximum number of consecutive RTO-based retransmissions before
/// aborting the connection.  Matches Linux `tcp_retries2` default (15).
/// With exponential backoff starting at 1s and capping at 60s, this
/// covers roughly 1+2+4+8+16+32+60×9 ≈ 603 seconds (~10 minutes).
const MAX_RETRANSMITS: u16 = 15;

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
#[allow(dead_code)] // Protocol constant.
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

// ---------------------------------------------------------------------------
// Zero window probe (persist timer, RFC 793 §3.7, RFC 1122 §4.2.2.17)
// ---------------------------------------------------------------------------

/// Minimum persist timer interval (500ms).
///
/// The persist timer fires to probe a peer that has advertised a zero
/// window.  Per RFC 1122 §4.2.2.17, the interval should use exponential
/// backoff starting near the RTO, bounded between a minimum and maximum.
const PERSIST_MIN_NS: u64 = 500_000_000;

/// Maximum persist timer interval (60 seconds).
///
/// RFC 1122 §4.2.2.17 says the upper bound should be "at least 60
/// seconds."  We cap at 60s to avoid unnecessarily long stalls.
const PERSIST_MAX_NS: u64 = 60_000_000_000;

// TCP flags.
const TCP_FIN: u8 = 0x01;
const TCP_SYN: u8 = 0x02;
const TCP_RST: u8 = 0x04;
const TCP_PSH: u8 = 0x08;
const TCP_ACK: u8 = 0x10;
/// Congestion Window Reduced (RFC 3168 §6.1).
///
/// Set by the sender to indicate it has reduced its congestion window
/// in response to an ECE-flagged ACK from the receiver.
const TCP_CWR: u8 = 0x80;
/// ECN-Echo (RFC 3168 §6.1).
///
/// In the SYN: sender supports ECN.
/// In ACKs: the receiver saw a CE-marked IP packet and is echoing
/// the congestion signal to the sender.
const TCP_ECE: u8 = 0x40;

// ---------------------------------------------------------------------------
// TCP state machine
// ---------------------------------------------------------------------------

/// TCP connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    #[allow(dead_code)] // TCP state machine variant.
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
    /// Network namespace this connection belongs to.
    /// Connections in different namespaces are fully independent — the
    /// same 4-tuple (local_port, remote_ip, remote_port) can exist in
    /// multiple namespaces without conflict.
    ns_id: NetNsId,
    /// Connection state.
    state: TcpState,
    /// Local port.
    local_port: u16,
    /// Remote IP (IPv4 or IPv6).
    remote_ip: IpAddr,
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
    /// Whether the local write side has been shut down (SHUT_WR).
    /// After this is set, no more data can be sent; a FIN has been queued.
    local_write_closed: bool,
    /// Whether the local read side has been shut down (SHUT_RD).
    /// After this is set, incoming data is ACKed but discarded.
    local_read_closed: bool,
    /// Retransmit counter (incremented each poll cycle).
    retransmit_timer: u32,
    /// Number of consecutive RTO-based retransmissions without receiving
    /// an ACK.  Incremented in `tick_retransmit()`, reset to 0 when an
    /// ACK advances `snd_una`.  Connection is aborted when this reaches
    /// `MAX_RETRANSMITS` (15), matching Linux `tcp_retries2`.
    retransmit_count: u16,

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
    /// Buffer for small writes delayed by Nagle's algorithm.
    ///
    /// When Nagle is enabled and unacknowledged data is in flight, writes
    /// smaller than MSS are buffered here instead of returning WouldBlock.
    /// The buffer is flushed when an ACK acknowledges all outstanding data
    /// (snd_una == snd_nxt) or when enough data accumulates to fill an MSS.
    nagle_buf: Vec<u8>,

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

    // -- SACK (RFC 2018) --

    /// Whether SACK was negotiated during the handshake (both sides must
    /// send SACK-Permitted in their SYN).
    sack_ok: bool,
    /// Out-of-order received blocks as (left_edge, right_edge) pairs.
    /// `left_edge` is the first sequence number of the block, `right_edge`
    /// is the sequence number just past the last byte.
    /// Blocks are ordered by left_edge.  Used to generate SACK option
    /// blocks in outgoing ACKs.
    sack_blocks: [(u32, u32); MAX_SACK_BLOCKS],
    /// Number of active SACK blocks (entries in `sack_blocks`).
    sack_block_count: u8,

    // -- Out-of-order receive buffer --

    /// Buffer holding out-of-order received data indexed by sequence offset.
    ///
    /// `ooo_buf[i]` corresponds to sequence number `ooo_base + i`.  Only
    /// ranges covered by SACK blocks contain valid data; the gaps are
    /// undefined.  When in-order data fills a gap (rcv_nxt advances into
    /// a SACK block), the contiguous data is moved to `rx_buffer`.
    ooo_buf: Vec<u8>,
    /// Sequence number of `ooo_buf[0]`.  Kept equal to `rcv_nxt` after
    /// delivery so the buffer only holds data above the cumulative ACK.
    ooo_base: u32,

    // -- Duplicate ACK / fast retransmit (RFC 5681 §3.2) --

    /// Number of consecutive duplicate ACKs received.
    ///
    /// A "duplicate ACK" is an ACK that does not advance `snd_una`.
    /// After 3 duplicate ACKs, RFC 5681 triggers fast retransmit (resend
    /// the lost segment from the retransmit buffer) and fast recovery
    /// (halve cwnd, inflate on additional dup ACKs).
    dup_ack_count: u8,

    // -- Retransmit buffer --

    /// Sent data waiting to be acknowledged.
    ///
    /// When `send()` transmits data, a copy is appended here.  When an
    /// ACK advances `snd_una`, the front is trimmed.  On fast retransmit
    /// or RTO timeout, data is resent from this buffer.
    tx_buffer: Vec<u8>,
    /// Sequence number corresponding to `tx_buffer[0]`.
    ///
    /// Invariant: `tx_buf_seq == snd_una` after ACK trimming.
    tx_buf_seq: u32,
    /// Timestamp (ns) when unacknowledged data was first sent.
    /// Used for RTO timeout-based retransmission.
    tx_last_send_ns: u64,

    // -- Peer MSS (RFC 793 §3.1, RFC 879) --

    /// Maximum segment size the peer will accept.  Parsed from the MSS
    /// option in the SYN/SYN-ACK.  Outgoing data segments must not
    /// exceed this value.  0 means no MSS option was present — in that
    /// case the default 536 bytes applies per RFC 1122 §4.2.2.6, but
    /// we use our MSS (1460) since we're on the same Ethernet segment
    /// as the peer in all current configurations.
    peer_mss: u16,

    // -- ECN (Explicit Congestion Notification, RFC 3168) --

    /// Whether ECN was negotiated during the handshake (both sides
    /// set ECE+CWR in their SYN).
    ecn_ok: bool,
    /// Whether we've received a CE-marked packet and need to echo
    /// ECE in our next ACK.  Cleared when we send an ACK with ECE set.
    ecn_ce_pending: bool,
    /// Whether we've already reduced cwnd for the current CE event.
    /// Prevents multiple reductions for a burst of CE-marked packets.
    /// Reset when we receive a CWR from the peer.
    ecn_cwr_sent: bool,

    // -- Timestamps (RFC 7323 §3-4) --

    /// Whether TCP timestamps were negotiated during the handshake.
    /// Both sides must include the TSopt in their SYN for timestamps
    /// to be active.
    ts_ok: bool,
    /// Most recent valid TSval received from the peer (TS.Recent).
    /// Echoed back in TSecr of outgoing segments so the peer can
    /// measure RTT.
    ts_recent: u32,
    /// Monotonic time (ns) when `ts_recent` was last updated.
    /// Used for PAWS aging — segments are not rejected by PAWS if
    /// `ts_recent` is older than 24 days (the TCP MSL equivalent).
    ts_recent_age_ns: u64,

    // -- Persist timer (zero window probe, RFC 1122 §4.2.2.17) --

    /// Whether the persist timer is active (peer advertised zero window
    /// and we have data pending).
    persist_active: bool,
    /// Current persist timer interval (nanoseconds).  Doubles on each
    /// probe (exponential backoff), clamped to [PERSIST_MIN_NS, PERSIST_MAX_NS].
    persist_interval_ns: u64,
    /// Timestamp (ns, monotonic) when we last sent a window probe.
    /// Zero means no probe sent yet — use `persist_interval_ns` from the
    /// moment `persist_active` was set.
    persist_last_ns: u64,

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

    /// Last error that caused this connection to be deactivated.
    ///
    /// Set when the connection transitions to `active = false` due to an
    /// error (RST, timeout, etc.).  Queried by `getsockopt(SO_ERROR)` to
    /// distinguish between ECONNREFUSED (RST on SYN_SENT), ECONNRESET
    /// (RST on established/closing), and ETIMEDOUT (retransmit exhaustion).
    ///
    /// Value 0 means no error (normal close).
    last_error: u8,
}

impl TcpConnection {
    const fn empty() -> Self {
        Self {
            active: false,
            ns_id: crate::netns::ROOT_NS,
            state: TcpState::Closed,
            local_port: 0,
            remote_ip: IpAddr::UNSPECIFIED_V4,
            remote_port: 0,
            snd_una: 0,
            snd_nxt: 0,
            snd_iss: 0,
            snd_wnd: DEFAULT_WINDOW as u32,
            rcv_nxt: 0,
            rcv_irs: 0,
            rx_buffer: Vec::new(),
            remote_closed: false,
            local_write_closed: false,
            local_read_closed: false,
            retransmit_timer: 0,
            retransmit_count: 0,
            srtt_ns_x8: 0,
            rttvar_ns_x4: 0,
            rto_ns: RTO_INITIAL_NS,
            rtt_initialized: false,
            rtt_seq: 0,
            rtt_sent_ns: 0,
            nagle_enabled: NAGLE_DEFAULT,
            nagle_buf: Vec::new(),
            cwnd: INITIAL_CWND_SEGS.saturating_mul(MSS as u32),
            ssthresh: u32::MAX, // Start in slow-start.
            wscale_ok: false,
            snd_wnd_scale: 0,
            rcv_wnd_scale: 0,
            sack_ok: false,
            sack_blocks: [(0, 0); MAX_SACK_BLOCKS],
            sack_block_count: 0,
            ooo_buf: Vec::new(),
            ooo_base: 0,
            dup_ack_count: 0,
            tx_buffer: Vec::new(),
            tx_buf_seq: 0,
            tx_last_send_ns: 0,
            peer_mss: 0,
            ecn_ok: false,
            ecn_ce_pending: false,
            ecn_cwr_sent: false,
            ts_ok: false,
            ts_recent: 0,
            ts_recent_age_ns: 0,
            persist_active: false,
            persist_interval_ns: PERSIST_MIN_NS,
            persist_last_ns: 0,
            keepalive_enabled: false,
            keepalive_idle_ns: KEEPALIVE_IDLE_DEFAULT_NS,
            keepalive_interval_ns: KEEPALIVE_INTERVAL_DEFAULT_NS,
            keepalive_probes_max: KEEPALIVE_PROBES_DEFAULT,
            keepalive_probes_sent: 0,
            last_activity_ns: 0,
            last_error: 0,
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
    /// Network namespace this listener belongs to.
    /// The same port can be bound in different namespaces.
    ns_id: NetNsId,
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
            ns_id: crate::netns::ROOT_NS,
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

const EPHEMERAL_TCP_START: u16 = 49200;
const EPHEMERAL_TCP_END: u16 = 65000;

/// Allocate an ephemeral port that doesn't conflict with an existing
/// connection to the same remote endpoint within the same namespace
/// (5-tuple uniqueness: ns_id + local_port + remote_ip + remote_port).
///
/// Must be called while holding the CONNECTIONS lock — the caller passes
/// a slice of connections to check against.
#[allow(clippy::arithmetic_side_effects)]
fn alloc_port_for(
    conns: &[TcpConnection; MAX_CONNECTIONS],
    _ns_id: NetNsId,
    remote_ip: IpAddr,
    remote_port: u16,
) -> KernelResult<u16> {
    let mut port_guard = NEXT_PORT.lock();
    let start = *port_guard;
    let mut candidate = start;

    loop {
        // Check globally (all namespaces) because ip_send_tcp() routes all
        // outgoing TCP through ROOT_NS — all connections share the host's
        // single IP on the wire.  A per-namespace check would allow two
        // containers to allocate the same ephemeral port for the same remote,
        // creating a 4-tuple collision on the physical network.
        let conflicts = conns.iter().any(|c| {
            c.active
                && c.local_port == candidate
                && c.remote_ip == remote_ip
                && c.remote_port == remote_port
        });

        if !conflicts {
            // Advance the counter past this candidate for next call.
            *port_guard = if candidate >= EPHEMERAL_TCP_END {
                EPHEMERAL_TCP_START
            } else {
                candidate + 1
            };
            return Ok(candidate);
        }

        // Advance to next candidate.
        candidate = if candidate >= EPHEMERAL_TCP_END {
            EPHEMERAL_TCP_START
        } else {
            candidate + 1
        };

        // If we've wrapped all the way around, no port is available.
        if candidate == start {
            return Err(KernelError::OutOfMemory);
        }
    }
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
    src_ip: IpAddr,
    dst_ip: IpAddr,
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

    // Compute TCP checksum (includes IPv4 or IPv6 pseudo-header).
    let checksum = tcp_checksum_ip(&seg, src_ip, dst_ip);
    seg[checksum_offset] = (checksum >> 8) as u8;
    seg[checksum_offset + 1] = checksum as u8;

    seg
}

/// Build a TCP segment with arbitrary option bytes (for SACK blocks in ACKs).
///
/// `options` is the raw option bytes to place between the fixed header
/// and the payload.  Must be padded to a 4-byte boundary.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn build_segment_with_options(
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    options: &[u8],
    payload: &[u8],
    src_ip: IpAddr,
    dst_ip: IpAddr,
) -> Vec<u8> {
    // Pad options to 4-byte boundary.
    let opt_padded = (options.len() + 3) & !3;
    let header_len = TCP_HEADER_SIZE + opt_padded;
    let header_words = (header_len / 4) as u8;
    let total_len = header_len + payload.len();
    let mut seg = Vec::with_capacity(total_len);

    seg.extend_from_slice(&src_port.to_be_bytes());
    seg.extend_from_slice(&dst_port.to_be_bytes());
    seg.extend_from_slice(&seq.to_be_bytes());
    seg.extend_from_slice(&ack.to_be_bytes());
    seg.push(header_words << 4);
    seg.push(flags);
    seg.extend_from_slice(&window.to_be_bytes());
    let checksum_offset = seg.len();
    seg.extend_from_slice(&0u16.to_be_bytes());
    seg.extend_from_slice(&0u16.to_be_bytes());

    // Options.
    seg.extend_from_slice(options);
    // Pad with NOP/END to 4-byte boundary.
    seg.resize(seg.len() + opt_padded.saturating_sub(options.len()), TCP_OPT_END);

    // Payload.
    seg.extend_from_slice(payload);

    let checksum = tcp_checksum_ip(&seg, src_ip, dst_ip);
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

/// Compute TCP checksum including the IPv6 pseudo-header (RFC 8200 §8.1).
///
/// The IPv6 pseudo-header is 40 bytes: source address (16), destination
/// address (16), upper-layer packet length (4, u32), zero (3) + next
/// header (1).
#[allow(clippy::arithmetic_side_effects)]
fn tcp_checksum_v6(segment: &[u8], src_ip: &Ipv6Addr, dst_ip: &Ipv6Addr) -> u16 {
    let mut sum: u32 = 0;

    // Pseudo-header: source address (16 bytes).
    for i in 0..8 {
        let word = u16::from_be_bytes([src_ip.0[i * 2], src_ip.0[i * 2 + 1]]);
        sum = sum.wrapping_add(u32::from(word));
    }
    // Pseudo-header: destination address (16 bytes).
    for i in 0..8 {
        let word = u16::from_be_bytes([dst_ip.0[i * 2], dst_ip.0[i * 2 + 1]]);
        sum = sum.wrapping_add(u32::from(word));
    }
    // Pseudo-header: upper-layer packet length (32 bits).
    let seg_len = segment.len() as u32;
    sum = sum.wrapping_add(seg_len >> 16);
    sum = sum.wrapping_add(seg_len & 0xFFFF);
    // Pseudo-header: zero (3 bytes) + next header = 6 (TCP).
    sum = sum.wrapping_add(6);

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

/// Compute TCP checksum with the appropriate pseudo-header for the
/// address family.  Dispatches to [`tcp_checksum`] (IPv4) or
/// [`tcp_checksum_v6`] (IPv6).
fn tcp_checksum_ip(segment: &[u8], src: IpAddr, dst: IpAddr) -> u16 {
    match (src, dst) {
        (IpAddr::V4(s), IpAddr::V4(d)) => tcp_checksum(segment, s, d),
        (IpAddr::V6(ref s), IpAddr::V6(ref d)) => tcp_checksum_v6(segment, s, d),
        // Mismatched address families should never happen; return 0
        // which will cause checksum validation to fail.
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// TCP options parsing (RFC 7323, RFC 2018, RFC 793)
// ---------------------------------------------------------------------------

/// TCP option kinds.
const TCP_OPT_END: u8 = 0;        // End of option list.
const TCP_OPT_NOP: u8 = 1;        // No-operation (padding).
const TCP_OPT_MSS: u8 = 2;        // Maximum segment size.
const TCP_OPT_WSCALE: u8 = 3;     // Window scale (RFC 7323).
const TCP_OPT_SACK_PERM: u8 = 4;  // SACK permitted (RFC 2018).
const TCP_OPT_SACK: u8 = 5;       // SACK blocks (RFC 2018).
const TCP_OPT_TIMESTAMP: u8 = 8;  // Timestamps (RFC 7323).

/// Our advertised window scale shift count.
///
/// 0 = no scaling needed — our 64 KiB rx buffer fits in 16 bits.
/// We still *send* the option so the peer knows we understand
/// scaling and will apply its shift to their advertised window.
const OUR_WSCALE: u8 = 0;

/// Get the current TCP timestamp value (millisecond granularity).
///
/// Uses the monotonic clock divided to milliseconds, truncated to
/// 32 bits.  This wraps every ~49.7 days — fine for PAWS since the
/// window for detecting old duplicates is much shorter.
#[allow(clippy::arithmetic_side_effects)]
fn tcp_now_ms() -> u32 {
    (crate::hrtimer::now_ns() / 1_000_000) as u32
}

/// Maximum number of SACK blocks when timestamps are also in use.
///
/// The Timestamp option consumes 12 bytes (NOP+NOP+kind+len+TSval+TSecr)
/// of the 40-byte TCP option space.  SACK header is 4 bytes (NOP+NOP+kind+len)
/// plus 8 bytes per block.  With 28 bytes left: 4 + 3×8 = 28 → max 3 blocks.
const MAX_SACK_BLOCKS_WITH_TS: usize = 3;

/// Parsed TCP options from an incoming segment.
struct TcpOptions {
    /// MSS value from the peer (0 if not present).
    mss: u16,
    /// Window scale shift count (None if option not present).
    wscale: Option<u8>,
    /// Whether SACK-Permitted option was present (RFC 2018, SYN only).
    sack_permitted: bool,
    /// Timestamp option (TSval, TSecr).  None if option not present.
    timestamp: Option<(u32, u32)>,
}

/// Parse TCP options from the option bytes (header bytes 20..data_offset).
///
/// Returns the parsed `TcpOptions`.  Unknown options are skipped.
/// Malformed options (truncated length, etc.) terminate parsing early.
fn parse_tcp_options(option_bytes: &[u8]) -> TcpOptions {
    let mut opts = TcpOptions { mss: 0, wscale: None, sack_permitted: false, timestamp: None };
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
                    TCP_OPT_SACK_PERM if len == 2 => {
                        opts.sack_permitted = true;
                    }
                    TCP_OPT_TIMESTAMP if len == 10 => {
                        // RFC 7323 §3.2: TSval (4 bytes) + TSecr (4 bytes).
                        let tsval = u32::from_be_bytes([
                            option_bytes[i.wrapping_add(2)],
                            option_bytes[i.wrapping_add(3)],
                            option_bytes[i.wrapping_add(4)],
                            option_bytes[i.wrapping_add(5)],
                        ]);
                        let tsecr = u32::from_be_bytes([
                            option_bytes[i.wrapping_add(6)],
                            option_bytes[i.wrapping_add(7)],
                            option_bytes[i.wrapping_add(8)],
                            option_bytes[i.wrapping_add(9)],
                        ]);
                        opts.timestamp = Some((tsval, tsecr));
                    }
                    _ => {} // Unknown option — skip.
                }
                i = i.wrapping_add(len);
            }
        }
    }
    opts
}

/// Build a TCP segment with SYN options (MSS + WScale + SACK-Permitted + Timestamps).
///
/// SYN and SYN-ACK segments carry options to negotiate parameters.
/// Layout:
///   MSS(kind=2, len=4, value)                           =  4 bytes
///   NOP(kind=1)                                         =  1 byte
///   WScale(kind=3, len=3, shift)                        =  3 bytes
///   NOP(kind=1)                                         =  1 byte
///   NOP(kind=1)                                         =  1 byte
///   SACK-Perm(kind=4, len=2)                            =  2 bytes
///   NOP(kind=1)                                         =  1 byte
///   NOP(kind=1)                                         =  1 byte
///   Timestamp(kind=8, len=10, TSval=4, TSecr=4)         = 10 bytes
///   Total options: 24 bytes → header is 44 bytes (11 words).
#[allow(clippy::arithmetic_side_effects)]
fn build_segment_with_syn_options(
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
    src_ip: IpAddr,
    dst_ip: IpAddr,
    wscale: u8,
    tsval: u32,
    tsecr: u32,
) -> Vec<u8> {
    // Options: MSS(4) + NOP(1) + WScale(3) + NOP(1) + NOP(1) + SACK-Perm(2)
    //        + NOP(1) + NOP(1) + Timestamp(10) = 24 bytes.
    let header_words: u8 = 11; // (20 + 24) / 4 = 11 words.
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

    // Options: NOP + NOP (alignment for SACK-Permitted).
    seg.push(TCP_OPT_NOP);
    seg.push(TCP_OPT_NOP);

    // Options: SACK-Permitted (RFC 2018 §2).
    seg.push(TCP_OPT_SACK_PERM);
    seg.push(2); // Option length.

    // Options: NOP + NOP (alignment for Timestamp).
    seg.push(TCP_OPT_NOP);
    seg.push(TCP_OPT_NOP);

    // Options: Timestamp (RFC 7323 §3.2).
    seg.push(TCP_OPT_TIMESTAMP);
    seg.push(10); // Option length.
    seg.extend_from_slice(&tsval.to_be_bytes());
    seg.extend_from_slice(&tsecr.to_be_bytes());

    debug_assert_eq!(seg.len(), header_len, "SYN options size mismatch");

    // Payload (usually empty for SYN/SYN-ACK).
    seg.extend_from_slice(payload);

    // Compute TCP checksum (IPv4 or IPv6 pseudo-header).
    let checksum = tcp_checksum_ip(&seg, src_ip, dst_ip);
    seg[checksum_offset] = (checksum >> 8) as u8;
    seg[checksum_offset + 1] = checksum as u8;

    seg
}

// ---------------------------------------------------------------------------
// TCP segment sending (dual-stack dispatch)
// ---------------------------------------------------------------------------

/// Determine the local IP address to use for a connection based on the
/// remote address family.
///
/// IPv4 remotes use the interface's configured IPv4 address; IPv6 remotes
/// use the link-local address derived from the NIC's MAC address.
fn local_ip_for(remote: IpAddr) -> IpAddr {
    match remote {
        IpAddr::V4(_) => IpAddr::V4(interface::ip()),
        IpAddr::V6(_) => {
            let mac = interface::mac();
            IpAddr::V6(Ipv6Addr::from_mac_link_local(&mac))
        }
    }
}

/// Send a built TCP segment via the appropriate IP layer.
///
/// Dispatches to IPv4 or IPv6 based on the destination address family.
/// `ip_ecn` is the ECN codepoint for the IP header (0 = not-ECT,
/// 2 = ECT(0)).  IPv6 carries ECN in the traffic class field.
fn ip_send_tcp(dst: IpAddr, segment: &[u8], ip_ecn: u8) -> KernelResult<()> {
    match dst {
        IpAddr::V4(v4) => ipv4::send_ecn(v4, PROTO_TCP, segment, ip_ecn),
        IpAddr::V6(v6) => {
            // IPv6 sends via send_raw with link-local source.
            // ECN is carried in the traffic class byte; for now we use
            // the low 2 bits (same encoding as IPv4 ECN field).
            let src = {
                let mac = interface::mac();
                Ipv6Addr::from_mac_link_local(&mac)
            };
            ipv6::send_raw(src, v6, PROTO_TCP, ipv6::DEFAULT_HOP_LIMIT, segment)
        }
    }
}

/// Send a TCP segment via IP, advertising the given receive window.
fn send_segment_with_window(
    local_port: u16,
    remote_ip: IpAddr,
    remote_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
    ip_ecn: u8,
) -> KernelResult<()> {
    let local_ip = local_ip_for(remote_ip);
    let seg = build_segment(
        local_port, remote_port,
        seq, ack, flags,
        window,
        payload,
        local_ip, remote_ip,
    );

    ip_send_tcp(remote_ip, &seg, ip_ecn)
}

/// Send a TCP segment via IP, advertising the default receive window.
///
/// Convenience wrapper for control segments (SYN, RST, handshake ACKs)
/// where we don't have a connection context to compute a dynamic window.
fn send_segment(
    local_port: u16,
    remote_ip: IpAddr,
    remote_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
) -> KernelResult<()> {
    send_segment_with_window(
        local_port, remote_ip, remote_port,
        seq, ack, flags, DEFAULT_WINDOW, payload, 0,
    )
}

/// Send a SYN (or SYN-ACK) segment with TCP options (MSS + WScale + Timestamps).
///
/// Used during the 3-way handshake to negotiate window scaling and timestamps.
/// `tsval` is our current timestamp; `tsecr` echoes the peer's TSval
/// (0 for the initial SYN since we haven't received one yet).
fn send_syn_segment(
    local_port: u16,
    remote_ip: IpAddr,
    remote_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    wscale: u8,
    tsval: u32,
    tsecr: u32,
) -> KernelResult<()> {
    let local_ip = local_ip_for(remote_ip);
    let seg = build_segment_with_syn_options(
        local_port, remote_port,
        seq, ack, flags,
        window,
        &[],
        local_ip, remote_ip,
        wscale,
        tsval,
        tsecr,
    );

    ip_send_tcp(remote_ip, &seg, 0)
}

/// Send a data segment with optional Timestamp option.
///
/// When timestamps are negotiated (`ts_ok=true`), the segment includes a
/// Timestamp option (TSval=our clock, TSecr=echoed peer clock) so the peer
/// can update its ts_recent and measure RTT from our TSval.  When timestamps
/// are not active, this delegates to `send_segment_with_window()`.
fn send_data_with_ts(
    local_port: u16,
    remote_ip: IpAddr,
    remote_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
    ip_ecn: u8,
    ts_ok: bool,
    ts_recent: u32,
) -> KernelResult<()> {
    if ts_ok {
        // Build timestamp option: NOP + NOP + kind(8) + len(10) + TSval + TSecr.
        let mut ts_opt = [0u8; 12];
        ts_opt[0] = TCP_OPT_NOP;
        ts_opt[1] = TCP_OPT_NOP;
        ts_opt[2] = TCP_OPT_TIMESTAMP;
        ts_opt[3] = 10;
        let tsval = tcp_now_ms();
        ts_opt[4..8].copy_from_slice(&tsval.to_be_bytes());
        ts_opt[8..12].copy_from_slice(&ts_recent.to_be_bytes());

        let local_ip = local_ip_for(remote_ip);
        let seg = build_segment_with_options(
            local_port, remote_port,
            seq, ack, flags, window,
            &ts_opt, payload,
            local_ip, remote_ip,
        );
        ip_send_tcp(remote_ip, &seg, ip_ecn)
    } else {
        send_segment_with_window(
            local_port, remote_ip, remote_port,
            seq, ack, flags, window, payload, ip_ecn,
        )
    }
}

/// Send an ACK segment with TCP options (Timestamps and/or SACK blocks).
///
/// If timestamps were negotiated, every outgoing segment carries a
/// Timestamp option.  If SACK was also negotiated and there are
/// out-of-order blocks, SACK blocks are appended (limited to 3 when
/// timestamps are also present, to fit in the 40-byte option space).
///
/// ECN handling (RFC 3168 §6.1.3): if `ecn_ce_pending` is set, the
/// ACK includes the ECE flag to echo the congestion signal back to the
/// sender.  The IP header is marked ECT(0) when ECN is negotiated.
fn send_ack_with_sack(conn: &TcpConnection) -> KernelResult<()> {
    // ECN: add ECE flag to ACKs when we've seen CE-marked packets.
    let mut flags = TCP_ACK;
    if conn.ecn_ok && conn.ecn_ce_pending {
        flags |= TCP_ECE;
    }

    // IP ECN: mark ECT(0) for ECN-negotiated connections.
    let ip_ecn = if conn.ecn_ok { ipv4::ECN_ECT0 } else { 0 };

    // When timestamps are active, use the combined builder that puts
    // TSopt first, then SACK blocks (limited to 3 blocks).
    if conn.ts_ok {
        let (opts, opts_len) = build_ts_and_sack_options(conn);
        if opts_len > 0 {
            let local_ip = local_ip_for(conn.remote_ip);
            let seg = build_segment_with_options(
                conn.local_port, conn.remote_port,
                conn.snd_nxt, conn.rcv_nxt,
                flags,
                advertised_window(conn),
                &opts[..opts_len],
                &[],
                local_ip, conn.remote_ip,
            );
            return ip_send_tcp(conn.remote_ip, &seg, ip_ecn);
        }
    }

    // No timestamps — use the old SACK-only builder.
    let (sack_opt, sack_len) = build_sack_option(conn);

    if sack_len > 0 {
        let local_ip = local_ip_for(conn.remote_ip);
        let seg = build_segment_with_options(
            conn.local_port, conn.remote_port,
            conn.snd_nxt, conn.rcv_nxt,
            flags,
            advertised_window(conn),
            &sack_opt[..sack_len],
            &[],
            local_ip, conn.remote_ip,
        );
        ip_send_tcp(conn.remote_ip, &seg, ip_ecn)
    } else {
        send_segment_with_window(
            conn.local_port, conn.remote_ip, conn.remote_port,
            conn.snd_nxt, conn.rcv_nxt,
            flags, advertised_window(conn), &[], ip_ecn,
        )
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Open a TCP connection to the given address and port.
///
/// `ns_id` identifies the network namespace; connections in different
/// namespaces are fully independent.  Pass `netns::ROOT_NS` for the
/// host namespace.
///
/// Performs the 3-way handshake (SYN → SYN-ACK → ACK).
/// Returns a connection handle on success.
#[allow(clippy::arithmetic_side_effects)]
pub fn connect(ns_id: NetNsId, remote_ip: IpAddr, remote_port: u16) -> KernelResult<usize> {
    let isn = generate_isn();

    // Find a free slot. If all slots are occupied, try to recycle the
    // oldest TIME_WAIT connection (safest to evict since the connection
    // is fully closed and only waiting to absorb stale duplicate segments).
    let (handle, local_port) = {
        let mut conns = CONNECTIONS.lock();

        // Allocate a port that won't conflict with existing connections
        // to the same remote endpoint within the same namespace.
        let local_port = alloc_port_for(&conns, ns_id, remote_ip, remote_port)?;

        let slot = match conns.iter().position(|c| !c.active) {
            Some(idx) => idx,
            None => {
                // No free slots — try to reclaim a TIME_WAIT connection.
                let mut best: Option<usize> = None;
                let mut oldest_activity: u64 = u64::MAX;
                for (i, c) in conns.iter().enumerate() {
                    if c.active && c.state == TcpState::TimeWait {
                        if c.last_activity_ns < oldest_activity {
                            oldest_activity = c.last_activity_ns;
                            best = Some(i);
                        }
                    }
                }
                let idx = best.ok_or(KernelError::OutOfMemory)?;
                // Reclaim the TIME_WAIT slot.
                crate::serial_println!(
                    "[tcp] Recycling TIME_WAIT slot {} (port {}) for new connection",
                    idx, conns[idx].local_port
                );
                conns[idx].active = false;
                conns[idx].state = TcpState::Closed;
                conns[idx].rx_buffer.clear();
                conns[idx].tx_buffer.clear();
                conns[idx].nagle_buf.clear();
                conns[idx].ooo_buf.clear();
                idx
            }
        };

        let conn = &mut conns[slot];
        conn.active = true;
        conn.ns_id = ns_id;
        conn.state = TcpState::SynSent;
        conn.last_error = TCP_ERR_NONE; // Clear any stale error from recycled slot.
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
        // tx_buffer init is handled below (after sack_block_count).
        conn.remote_closed = false;
        conn.local_write_closed = false;
        conn.local_read_closed = false;
        conn.retransmit_timer = 0;
        conn.retransmit_count = 0; // Clear stale count from recycled slot.
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
        conn.sack_ok = false; // Set on SYN-ACK if peer supports it.
        conn.sack_blocks = [(0, 0); MAX_SACK_BLOCKS];
        conn.sack_block_count = 0;
        conn.ooo_buf.clear();
        conn.ooo_base = 0;
        conn.dup_ack_count = 0;
        conn.tx_buffer.clear();
        conn.nagle_buf.clear();
        conn.tx_buf_seq = isn.wrapping_add(1); // After SYN sequence.
        conn.tx_last_send_ns = 0;
        conn.ecn_ok = false; // Confirmed on SYN-ACK.
        conn.ecn_ce_pending = false;
        conn.ecn_cwr_sent = false;
        conn.ts_ok = false; // Set on SYN-ACK if peer supports timestamps.
        conn.ts_recent = 0;
        conn.ts_recent_age_ns = 0;
        conn.peer_mss = 0; // Updated from SYN-ACK MSS option.
        conn.persist_active = false;
        conn.persist_interval_ns = PERSIST_MIN_NS;
        conn.persist_last_ns = 0;
        conn.keepalive_enabled = false;
        conn.keepalive_idle_ns = KEEPALIVE_IDLE_DEFAULT_NS;
        conn.keepalive_interval_ns = KEEPALIVE_INTERVAL_DEFAULT_NS;
        conn.keepalive_probes_max = KEEPALIVE_PROBES_DEFAULT;
        conn.keepalive_probes_sent = 0;
        conn.last_activity_ns = crate::hrtimer::now_ns();
        (slot, local_port)
    };

    // Send SYN with MSS + WScale + Timestamp options (RFC 7323).
    // The window field in the SYN itself is NOT scaled (RFC 7323 §2.2).
    // Timestamp: TSval = our clock, TSecr = 0 (no peer value yet).
    // ECN negotiation (RFC 3168 §6.1.1): set ECE+CWR in the SYN to
    // signal ECN support.  The server responds with ECE (no CWR) if
    // it also supports ECN.
    send_syn_segment(
        local_port, remote_ip, remote_port,
        isn, 0, TCP_SYN | TCP_ECE | TCP_CWR, DEFAULT_WINDOW, OUR_WSCALE,
        tcp_now_ms(), 0,
    )?;
    crate::serial_println!(
        "[tcp] SYN sent to {}:{} (seq={}, wscale={})",
        remote_ip, remote_port, isn, OUR_WSCALE
    );

    // Wait for SYN-ACK with SYN retransmission (1s, 2s, 4s, 8s = ~15s total).
    // Each "attempt" is a SYN send followed by a polling wait.  If the SYN
    // or SYN-ACK is lost, we retransmit rather than silently timing out.
    const SYN_ATTEMPT_POLLS: [u32; 4] = [1000, 2000, 4000, 8000]; // ~1s, 2s, 4s, 8s

    for (attempt, &polls) in SYN_ATTEMPT_POLLS.iter().enumerate() {
        // On retry (not the first attempt), retransmit the SYN.
        if attempt > 0 {
            crate::serial_println!(
                "[tcp] SYN retransmit #{} to {}:{}", attempt, remote_ip, remote_port
            );
            let _ = send_syn_segment(
                local_port, remote_ip, remote_port,
                isn, 0, TCP_SYN | TCP_ECE | TCP_CWR, DEFAULT_WINDOW, OUR_WSCALE,
                tcp_now_ms(), 0,
            );
        }

        for _ in 0..polls {
            super::poll();

            let state = CONNECTIONS.lock()[handle].state;
            if state == TcpState::Established {
                crate::serial_println!(
                    "[tcp] Connection established to {}:{}", remote_ip, remote_port
                );
                return Ok(handle);
            }
            if state == TcpState::Closed {
                return Err(KernelError::NotSupported); // Connection refused.
            }

            for _ in 0..10_000 {
                core::hint::spin_loop();
            }
        }
    }

    // All attempts exhausted — clean up.
    let mut conns = CONNECTIONS.lock();
    conns[handle].last_error = TCP_ERR_TIMEDOUT;
    conns[handle].active = false;
    conns[handle].state = TcpState::Closed;
    Err(KernelError::TimedOut)
}

/// Start a TCP connection without waiting for completion (non-blocking).
///
/// `ns_id` identifies the network namespace; pass `netns::ROOT_NS` for
/// the host namespace.
///
/// Allocates a connection slot, sends the initial SYN, and returns the
/// handle immediately in `SynSent` state.  The caller should use
/// `poll_status()` to detect when the connection completes:
///
/// - `POLL_WRITABLE` (without ERROR) → connection established.
/// - `POLL_WRITABLE | POLL_ERROR | ...` → connection failed (RST/timeout).
///
/// POSIX requires POLLOUT on connect completion regardless of success or
/// failure, so applications can use `poll(fd, POLLOUT, -1)` then check
/// `getsockopt(SO_ERROR)` to distinguish success from failure.
///
/// Returns `(handle, KernelError::WouldBlock)` — the WouldBlock is the
/// expected "in progress" signal, not an error.  The caller should map
/// this to EINPROGRESS at the POSIX layer.
#[allow(clippy::arithmetic_side_effects)]
pub fn connect_start(ns_id: NetNsId, remote_ip: IpAddr, remote_port: u16) -> KernelResult<usize> {
    let isn = generate_isn();

    // Find a free slot (same recycling logic as connect()).
    let (handle, local_port) = {
        let mut conns = CONNECTIONS.lock();

        // Allocate a port that won't conflict with existing connections
        // to the same remote endpoint within the same namespace.
        let local_port = alloc_port_for(&conns, ns_id, remote_ip, remote_port)?;

        let slot = match conns.iter().position(|c| !c.active) {
            Some(idx) => idx,
            None => {
                let mut best: Option<usize> = None;
                let mut oldest_activity: u64 = u64::MAX;
                for (i, c) in conns.iter().enumerate() {
                    if c.active && c.state == TcpState::TimeWait {
                        if c.last_activity_ns < oldest_activity {
                            oldest_activity = c.last_activity_ns;
                            best = Some(i);
                        }
                    }
                }
                let idx = best.ok_or(KernelError::OutOfMemory)?;
                conns[idx].active = false;
                conns[idx].state = TcpState::Closed;
                conns[idx].rx_buffer.clear();
                conns[idx].tx_buffer.clear();
                conns[idx].nagle_buf.clear();
                conns[idx].ooo_buf.clear();
                idx
            }
        };

        let conn = &mut conns[slot];
        conn.active = true;
        conn.ns_id = ns_id;
        conn.state = TcpState::SynSent;
        conn.last_error = TCP_ERR_NONE; // Clear any stale error from recycled slot.
        conn.local_port = local_port;
        conn.remote_ip = remote_ip;
        conn.remote_port = remote_port;
        conn.snd_iss = isn;
        conn.snd_una = isn;
        conn.snd_nxt = isn.wrapping_add(1);
        conn.snd_wnd = DEFAULT_WINDOW as u32;
        conn.rcv_nxt = 0;
        conn.rcv_irs = 0;
        conn.rx_buffer.clear();
        conn.remote_closed = false;
        conn.local_write_closed = false;
        conn.local_read_closed = false;
        conn.retransmit_timer = 0;
        conn.retransmit_count = 0; // Clear stale count from recycled slot.
        conn.srtt_ns_x8 = 0;
        conn.rttvar_ns_x4 = 0;
        conn.rto_ns = RTO_INITIAL_NS;
        conn.rtt_initialized = false;
        conn.rtt_seq = isn;
        conn.rtt_sent_ns = crate::hrtimer::now_ns();
        conn.nagle_enabled = NAGLE_DEFAULT;
        conn.cwnd = INITIAL_CWND_SEGS.saturating_mul(MSS as u32);
        conn.ssthresh = u32::MAX;
        conn.wscale_ok = false;
        conn.snd_wnd_scale = 0;
        conn.rcv_wnd_scale = OUR_WSCALE;
        conn.sack_ok = false;
        conn.sack_blocks = [(0, 0); MAX_SACK_BLOCKS];
        conn.sack_block_count = 0;
        conn.ooo_buf.clear();
        conn.ooo_base = 0;
        conn.dup_ack_count = 0;
        conn.tx_buffer.clear();
        conn.nagle_buf.clear();
        conn.tx_buf_seq = isn.wrapping_add(1);
        conn.tx_last_send_ns = 0;
        conn.ecn_ok = false;
        conn.ecn_ce_pending = false;
        conn.ecn_cwr_sent = false;
        conn.ts_ok = false;
        conn.ts_recent = 0;
        conn.ts_recent_age_ns = 0;
        conn.peer_mss = 0;
        conn.persist_active = false;
        conn.persist_interval_ns = PERSIST_MIN_NS;
        conn.persist_last_ns = 0;
        conn.keepalive_enabled = false;
        conn.keepalive_idle_ns = KEEPALIVE_IDLE_DEFAULT_NS;
        conn.keepalive_interval_ns = KEEPALIVE_INTERVAL_DEFAULT_NS;
        conn.keepalive_probes_max = KEEPALIVE_PROBES_DEFAULT;
        conn.keepalive_probes_sent = 0;
        conn.last_activity_ns = crate::hrtimer::now_ns();
        (slot, local_port)
    };

    // Send the initial SYN.
    send_syn_segment(
        local_port, remote_ip, remote_port,
        isn, 0, TCP_SYN | TCP_ECE | TCP_CWR, DEFAULT_WINDOW, OUR_WSCALE,
        tcp_now_ms(), 0,
    )?;

    crate::serial_println!(
        "[tcp] SYN sent (non-blocking) to {}:{} (handle={})",
        remote_ip, remote_port, handle
    );

    Ok(handle)
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
        let err = sample_ns.abs_diff(srtt);

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

/// Attempt an RTT sample using TCP timestamps (RFC 7323 §4.1).
///
/// When timestamps are negotiated, the TSecr field of every ACK echoes
/// back the TSval we sent with the original data.  Since our TSval is
/// `tcp_now_ms()`, RTT = now_ms − TSecr (in milliseconds).
///
/// This provides a sample on every ACK, much better than timing a single
/// segment per flight (which is the fallback when timestamps aren't available).
#[allow(clippy::arithmetic_side_effects)]
fn try_rtt_sample_ts(conn: &mut TcpConnection, tsecr: u32) {
    if tsecr == 0 {
        return; // No echo yet (e.g. pure SYN-ACK response).
    }
    let now_ms = tcp_now_ms();
    // Wrapping subtraction handles clock wrap naturally.
    let rtt_ms = now_ms.wrapping_sub(tsecr);
    // Sanity: reject RTT > 60s (likely a stale or corrupt TSecr).
    if rtt_ms > 60_000 {
        return;
    }
    // Convert to nanoseconds for the existing RTT estimator.
    let sample_ns = u64::from(rtt_ms).saturating_mul(1_000_000);
    update_rtt(conn, sample_ns);
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
/// Compute the usable send window — how many NEW bytes we can put in flight.
///
/// This is `min(snd_wnd, cwnd) - bytes_in_flight`.  Without subtracting
/// in-flight bytes, back-to-back send() calls (before ACKs arrive) would
/// each use the full window, grossly overshooting congestion control.
fn effective_window(conn: &TcpConnection) -> usize {
    let peer = conn.snd_wnd as usize;
    let cong = conn.cwnd as usize;
    let total_window = peer.min(cong);
    // Bytes currently in-flight (sent but not yet acknowledged).
    let in_flight = conn.snd_nxt.wrapping_sub(conn.snd_una) as usize;
    total_window.saturating_sub(in_flight)
}

/// Effective MSS for outgoing data on this connection.
///
/// Returns the smaller of our MSS (Ethernet-derived, 1460) and the
/// peer's advertised MSS from the SYN/SYN-ACK.  Per RFC 1122 §4.2.2.6,
/// if the peer didn't send an MSS option, the default is 536 bytes.
/// However, since we're on the same Ethernet segment in our current
/// configurations, we fall back to our own MSS rather than the
/// conservative 536.
fn effective_mss(conn: &TcpConnection) -> usize {
    if conn.peer_mss > 0 {
        MSS.min(conn.peer_mss as usize)
    } else {
        MSS
    }
}

/// Called when an ACK arrives — updates congestion window (slow-start or
/// congestion avoidance, RFC 5681 §3.1).
#[allow(clippy::arithmetic_side_effects)]
fn on_ack_congestion(conn: &mut TcpConnection, bytes_acked: u32) {
    if bytes_acked == 0 {
        return;
    }
    let mss = effective_mss(conn) as u32;
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
    let mss = effective_mss(conn) as u32;
    // ssthresh = max(FlightSize / 2, 2*MSS)
    let flight = conn.snd_nxt.wrapping_sub(conn.snd_una);
    conn.ssthresh = (flight / 2).max(mss.saturating_mul(2));
    // cwnd = 1 MSS (enter slow start after loss).
    conn.cwnd = mss;
}

// ---------------------------------------------------------------------------
// Retransmit buffer helpers
// ---------------------------------------------------------------------------

/// Trim acknowledged data from the front of the retransmit buffer.
///
/// Called when `snd_una` advances.  `bytes_acked` is the number of bytes
/// newly acknowledged.  Keeps `tx_buf_seq` in sync with `snd_una`.
#[allow(clippy::arithmetic_side_effects)]
fn tx_buffer_trim(conn: &mut TcpConnection, bytes_acked: u32) {
    let trim = (bytes_acked as usize).min(conn.tx_buffer.len());
    if trim > 0 {
        // Remove the front `trim` bytes.
        // For moderate sizes, this Vec::drain is acceptable — in the
        // common case we're draining nearly all of the buffer (ACK covers
        // entire send).
        conn.tx_buffer.drain(..trim);
        conn.tx_buf_seq = conn.tx_buf_seq.wrapping_add(trim as u32);
    }
    // If all data is acknowledged, reset the send timestamp.
    if conn.tx_buffer.is_empty() {
        conn.tx_last_send_ns = 0;
    }
}

/// Retransmit the first unacknowledged segment from the retransmit buffer.
///
/// Sends up to `effective_mss(conn)` bytes from `tx_buffer[0..]`
/// (sequence `snd_una`).
#[allow(clippy::arithmetic_side_effects)]
/// Retransmit info tuple.
type RetxInfo = (u16, IpAddr, u16, u32, u32, u16, [u8; 1460], usize, bool, u32);

fn retransmit_from_buffer(conn: &TcpConnection) -> Option<RetxInfo> {
    let retx_len = conn.tx_buffer.len().min(effective_mss(conn));
    if retx_len == 0 {
        return None;
    }
    let mut data = [0u8; 1460]; // MSS-sized stack buffer.
    data[..retx_len].copy_from_slice(&conn.tx_buffer[..retx_len]);
    Some((
        conn.local_port,
        conn.remote_ip,
        conn.remote_port,
        conn.snd_una,       // Retransmit from the first unacked byte.
        conn.rcv_nxt,
        advertised_window(conn),
        data,
        retx_len,
        conn.ts_ok,
        conn.ts_recent,
    ))
}

// ---------------------------------------------------------------------------
// SACK receive-side helpers (RFC 2018)
// ---------------------------------------------------------------------------

/// Record an out-of-order received segment as a SACK block.
///
/// `left` is the first sequence number of the received data, `right` is
/// one past the last byte.  If the new range overlaps or is adjacent to
/// an existing block, they are merged.  The block list is sorted by
/// `left_edge` and capped at `MAX_SACK_BLOCKS`.
#[allow(clippy::arithmetic_side_effects)]
fn sack_insert(conn: &mut TcpConnection, left: u32, right: u32) {
    // Merge with existing blocks that overlap or are adjacent.
    let mut new_left = left;
    let mut new_right = right;
    let count = conn.sack_block_count as usize;

    // Find and remove blocks that overlap or are contiguous.
    let mut kept = 0usize;
    for i in 0..count {
        let (bl, br) = conn.sack_blocks[i];
        // Two ranges [new_left, new_right) and [bl, br) overlap or touch if
        // neither is entirely before the other:
        //   !(new_right < bl || br < new_left)  in modular arithmetic.
        // For simplicity (our windows are < 2^31), we compare directly.
        let separate = seq_lt(new_right, bl) || seq_lt(br, new_left);
        if separate {
            // Keep this block.
            conn.sack_blocks[kept] = (bl, br);
            kept = kept.wrapping_add(1);
        } else {
            // Merge: extend new range to cover this block.
            if seq_lt(bl, new_left) {
                new_left = bl;
            }
            if seq_lt(new_right, br) {
                new_right = br;
            }
        }
    }

    // Insert the merged block.
    if kept < MAX_SACK_BLOCKS {
        conn.sack_blocks[kept] = (new_left, new_right);
        kept = kept.wrapping_add(1);
    }

    conn.sack_block_count = kept as u8;

    // Sort by left edge (insertion sort on small array).
    let n = conn.sack_block_count as usize;
    for i in 1..n {
        let key = conn.sack_blocks[i];
        let mut j = i;
        while j > 0 && seq_lt(key.0, conn.sack_blocks[j.wrapping_sub(1)].0) {
            conn.sack_blocks[j] = conn.sack_blocks[j.wrapping_sub(1)];
            j = j.wrapping_sub(1);
        }
        conn.sack_blocks[j] = key;
    }
}

/// Clean up SACK blocks that are now below `rcv_nxt`.
///
/// After `rcv_nxt` advances (because in-order or OOO-delivered data was
/// accepted), removes SACK blocks whose left edge is at or below
/// `rcv_nxt` (they're covered by the cumulative ACK).
#[allow(clippy::arithmetic_side_effects)]
fn sack_advance(conn: &mut TcpConnection) {
    // Remove blocks that are now below rcv_nxt (already acknowledged
    // by the cumulative ACK).
    let mut kept = 0usize;
    let count = conn.sack_block_count as usize;
    for i in 0..count {
        let (bl, br) = conn.sack_blocks[i];
        // If the block's right edge is past rcv_nxt, it's still relevant.
        if seq_lt(conn.rcv_nxt, br) {
            // Trim left edge if it's below rcv_nxt.
            let trimmed_left = if seq_lt(bl, conn.rcv_nxt) {
                conn.rcv_nxt
            } else {
                bl
            };
            conn.sack_blocks[kept] = (trimmed_left, br);
            kept = kept.wrapping_add(1);
        }
    }
    conn.sack_block_count = kept as u8;
}

// ---------------------------------------------------------------------------
// Out-of-order receive buffer
// ---------------------------------------------------------------------------

/// Maximum size of the OOO receive buffer per connection.
///
/// Limits memory consumption from buffered out-of-order data.  Must
/// be at least as large as the receive window (MAX_RX_BUFFER) to handle
/// a full window of reordered data.
const MAX_OOO_BUF: usize = MAX_RX_BUFFER;

/// Store an out-of-order segment's payload in the OOO buffer.
///
/// `seq` is the segment's starting sequence number, `data` is the
/// payload.  Data is written into `ooo_buf` at offset `seq - ooo_base`.
/// The buffer is grown as needed (bounded by `MAX_OOO_BUF`).
#[allow(clippy::arithmetic_side_effects)]
fn ooo_store(conn: &mut TcpConnection, seq: u32, data: &[u8]) {
    if data.is_empty() {
        return;
    }

    // Initialize ooo_base to rcv_nxt if the buffer is empty.
    if conn.ooo_buf.is_empty() {
        conn.ooo_base = conn.rcv_nxt;
    }

    // Reject segments behind ooo_base (retransmissions/stale data).
    // seq_lt uses signed 32-bit comparison, correctly handling wrap at 2^32.
    if seq_lt(seq, conn.ooo_base) {
        // Segment starts before our buffer base.  Compute how many bytes
        // of data, if any, fall *after* ooo_base.
        let behind = conn.ooo_base.wrapping_sub(seq) as usize;
        if behind >= data.len() {
            // Entire segment is behind ooo_base — discard.
            return;
        }
        // Partial overlap: store only the portion at/after ooo_base.
        // Recurse with the trimmed data starting at ooo_base.
        ooo_store(conn, conn.ooo_base, &data[behind..]);
        return;
    }

    // Compute offset into the buffer.
    let offset = seq.wrapping_sub(conn.ooo_base) as usize;
    let end = offset.saturating_add(data.len());

    // Refuse if the data would place beyond our buffer limit.
    if end > MAX_OOO_BUF {
        return;
    }

    // Grow the buffer if needed (zero-fill gaps — the SACK blocks
    // track which ranges contain valid data).
    if end > conn.ooo_buf.len() {
        conn.ooo_buf.resize(end, 0);
    }

    // Copy payload into the buffer at the right offset.
    conn.ooo_buf[offset..end].copy_from_slice(data);
}

/// Deliver contiguous OOO-buffered data to the receive buffer.
///
/// After in-order data has been accepted (advancing `rcv_nxt`), scan
/// the SACK blocks to find contiguous data starting at `rcv_nxt`.
/// Copy that data from `ooo_buf` to `rx_buffer`, advance `rcv_nxt`,
/// and trim the OOO buffer.
///
/// Called before `sack_advance` so the SACK blocks are still present
/// for lookup.
#[allow(clippy::arithmetic_side_effects)]
fn ooo_deliver(conn: &mut TcpConnection) {
    if conn.ooo_buf.is_empty() {
        return;
    }

    // Repeatedly scan SACK blocks for one whose left edge == rcv_nxt.
    // When found, deliver that block's data and advance rcv_nxt.
    loop {
        let mut found = false;
        let count = conn.sack_block_count as usize;

        for i in 0..count {
            let (bl, br) = conn.sack_blocks[i];
            // Check if this block starts at (or before) rcv_nxt and
            // extends past it — i.e., it's contiguous with what we've
            // already received.
            if !seq_lt(conn.rcv_nxt, bl) && seq_lt(conn.rcv_nxt, br) {
                // This block overlaps rcv_nxt.  Deliver from rcv_nxt to br.
                let start_off = conn.rcv_nxt.wrapping_sub(conn.ooo_base) as usize;
                let end_off = br.wrapping_sub(conn.ooo_base) as usize;

                // Bounds check against actual buffer size.
                if end_off > conn.ooo_buf.len() || start_off >= conn.ooo_buf.len() {
                    break;
                }

                // Limit buffered amount by rx_buffer capacity.
                let raw_len = end_off.saturating_sub(start_off);
                if raw_len == 0 {
                    break;
                }
                // When read side is shut down, discard data but advance
                // rcv_nxt by the full amount (protocol must keep moving
                // so the connection can close gracefully).
                // When buffer is full, only advance by what we stored —
                // otherwise we'd ACK bytes the application never sees.
                let deliver_len = if conn.local_read_closed {
                    // Intentional discard — advance past all of it.
                    raw_len
                } else {
                    raw_len.min(MAX_RX_BUFFER.saturating_sub(conn.rx_buffer.len()))
                };

                if deliver_len > 0 && !conn.local_read_closed {
                    let actual_end = start_off.saturating_add(deliver_len);
                    conn.rx_buffer.extend_from_slice(&conn.ooo_buf[start_off..actual_end]);
                }
                conn.rcv_nxt = conn.rcv_nxt.wrapping_add(deliver_len as u32);
                found = true;
                break; // Re-scan from the beginning (blocks may now be contiguous).
            }
        }

        if !found {
            break;
        }
    }

    // Trim consumed data from the front of ooo_buf.
    let consumed = conn.rcv_nxt.wrapping_sub(conn.ooo_base) as usize;
    if consumed > 0 && consumed <= conn.ooo_buf.len() {
        conn.ooo_buf.drain(..consumed);
        conn.ooo_base = conn.rcv_nxt;
    } else if consumed > conn.ooo_buf.len() {
        // rcv_nxt advanced past all buffered data.
        conn.ooo_buf.clear();
        conn.ooo_base = conn.rcv_nxt;
    }
}

/// Sequence number comparison: `a < b` in modular 32-bit arithmetic.
///
/// Returns true if `a` is before `b` in the sequence space.
/// Valid when the difference is less than 2^31.
fn seq_lt(a: u32, b: u32) -> bool {
    // (a - b) as i32 < 0 means a is before b.
    (a.wrapping_sub(b) as i32) < 0
}

/// Build SACK option bytes for an outgoing ACK.
///
/// Returns the option bytes to append to the TCP header (including NOP
/// padding and the SACK option header).  Returns empty if no SACK blocks
/// or SACK not negotiated.
///
/// SACK option format (RFC 2018 §3):
///   NOP, NOP  (alignment padding)
///   kind=5, length=2+8*N
///   [left_edge1, right_edge1]  (each 4 bytes, big-endian)
///   [left_edge2, right_edge2]
///   ...
#[allow(clippy::arithmetic_side_effects)]
/// Build combined options (Timestamp + optional SACK) for outgoing segments.
///
/// When timestamps are negotiated, every outgoing segment must carry the
/// Timestamp option (RFC 7323 §3.2).  If SACK blocks are also needed,
/// they are appended after the timestamp, limited to 3 blocks (the
/// 40-byte option space is shared: 12 for TSopt + up to 28 for SACK).
///
/// Returns `(buffer, length)`.  `length` is the total option bytes to
/// include.  If neither timestamps nor SACK is active, returns length 0.
fn build_ts_and_sack_options(conn: &TcpConnection) -> ([u8; 40], usize) {
    let mut buf = [0u8; 40];
    let mut pos = 0;

    if conn.ts_ok {
        // Timestamp option: NOP(1) + NOP(1) + kind(1) + len(1) + TSval(4) + TSecr(4) = 12.
        buf[pos] = TCP_OPT_NOP;
        buf[pos + 1] = TCP_OPT_NOP;
        buf[pos + 2] = TCP_OPT_TIMESTAMP;
        buf[pos + 3] = 10;
        let tsval = tcp_now_ms();
        buf[pos + 4..pos + 8].copy_from_slice(&tsval.to_be_bytes());
        buf[pos + 8..pos + 12].copy_from_slice(&conn.ts_recent.to_be_bytes());
        pos += 12;
    }

    // SACK blocks (if negotiated and present).
    if conn.sack_ok && conn.sack_block_count > 0 {
        // When timestamps are active, limit to 3 SACK blocks (28 bytes
        // remain from the 40-byte option space after the 12-byte TSopt).
        let max_blocks = if conn.ts_ok { MAX_SACK_BLOCKS_WITH_TS } else { MAX_SACK_BLOCKS };
        let n = (conn.sack_block_count as usize).min(max_blocks);
        let opt_len = 2 + n * 8; // kind + length + blocks.

        buf[pos] = TCP_OPT_NOP;
        buf[pos + 1] = TCP_OPT_NOP;
        buf[pos + 2] = TCP_OPT_SACK;
        buf[pos + 3] = opt_len as u8;
        pos += 4;

        for i in 0..n {
            let (left, right) = conn.sack_blocks[i];
            buf[pos..pos + 4].copy_from_slice(&left.to_be_bytes());
            buf[pos + 4..pos + 8].copy_from_slice(&right.to_be_bytes());
            pos += 8;
        }
    }

    (buf, pos)
}

fn build_sack_option(conn: &TcpConnection) -> ([u8; 36], usize) {
    // Max: 2 (NOP+NOP) + 2 (kind+len) + 4*8 (4 blocks × 8 bytes) = 36.
    let mut buf = [0u8; 36];

    if !conn.sack_ok || conn.sack_block_count == 0 {
        return (buf, 0);
    }

    let n = (conn.sack_block_count as usize).min(MAX_SACK_BLOCKS);
    let opt_len = 2 + n * 8; // kind + length + blocks.
    let total = 2 + opt_len;  // NOP + NOP + option.

    buf[0] = TCP_OPT_NOP;
    buf[1] = TCP_OPT_NOP;
    buf[2] = TCP_OPT_SACK;
    buf[3] = opt_len as u8;

    let mut pos = 4;
    for i in 0..n {
        let (left, right) = conn.sack_blocks[i];
        buf[pos..pos + 4].copy_from_slice(&left.to_be_bytes());
        buf[pos + 4..pos + 8].copy_from_slice(&right.to_be_bytes());
        pos += 8;
    }

    (buf, total)
}

/// Send data on an established TCP connection.
///
/// Respects both the peer's advertised receive window (`snd_wnd`) and
/// the congestion window (`cwnd`).  Nagle's algorithm (when enabled)
/// delays small segments when unacknowledged data is in flight.
///
/// Returns `Ok(())` even if the effective window truncated the send.
#[allow(clippy::arithmetic_side_effects)]
pub fn send(handle: usize, data: &[u8]) -> KernelResult<usize> {
    let (local_port, remote_ip, remote_port, seq, ack, eff_wnd, our_wnd,
         nagle, has_unacked, ecn_ok, ecn_cwr, eff_mss,
         ts_ok, ts_recent) = {
        let conns = CONNECTIONS.lock();
        let conn = conns.get(handle).ok_or(KernelError::InvalidArgument)?;
        // Allow sending in Established (normal) or CloseWait (remote
        // sent FIN but we haven't — we can still transmit data).
        if !conn.active
            || (conn.state != TcpState::Established && conn.state != TcpState::CloseWait)
        {
            // Connection was reset or is in a closing state — report
            // as "channel closed" which maps to ECONNRESET/EPIPE at
            // the POSIX layer.
            return Err(KernelError::ChannelClosed);
        }
        // Reject sends after shutdown(SHUT_WR).
        if conn.local_write_closed {
            return Err(KernelError::ChannelClosed);
        }
        let unacked = conn.snd_nxt != conn.snd_una;
        (conn.local_port, conn.remote_ip, conn.remote_port,
         conn.snd_nxt, conn.rcv_nxt, effective_window(conn),
         advertised_window(conn), conn.nagle_enabled, unacked,
         conn.ecn_ok, conn.ecn_cwr_sent, effective_mss(conn),
         conn.ts_ok, conn.ts_recent)
    };

    // Nagle's algorithm (RFC 896): if we have unacknowledged data in
    // flight, only send if the new data fills a full MSS.  This coalesces
    // small writes into larger segments.
    if nagle && has_unacked && data.len() < eff_mss {
        // Buffer the small write internally.  It will be flushed when
        // an ACK acknowledges all outstanding data (snd_una == snd_nxt)
        // or when enough data accumulates to fill an MSS.
        let mut conns = CONNECTIONS.lock();
        let conn = conns.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
        let conn_mss = effective_mss(conn);
        let space = conn_mss.saturating_sub(conn.nagle_buf.len());
        let copy_len = data.len().min(space);
        if copy_len > 0 {
            conn.nagle_buf.extend_from_slice(&data[..copy_len]);
        }
        // If the nagle_buf now fills an MSS, flush it immediately.
        if conn.nagle_buf.len() >= conn_mss {
            let lp = conn.local_port;
            let ri = conn.remote_ip;
            let rp = conn.remote_port;
            let s = conn.snd_nxt;
            let a = conn.rcv_nxt;
            let w = advertised_window(conn);
            let flush_len = conn.nagle_buf.len().min(conn_mss);
            let mut flush_data = [0u8; 1460];
            flush_data[..flush_len].copy_from_slice(&conn.nagle_buf[..flush_len]);
            conn.snd_nxt = s.wrapping_add(flush_len as u32);
            // Buffer for retransmit.
            let tx_space = MAX_TX_BUFFER.saturating_sub(conn.tx_buffer.len());
            let tx_copy = flush_len.min(tx_space);
            if tx_copy > 0 {
                conn.tx_buffer.extend_from_slice(&flush_data[..tx_copy]);
            }
            conn.tx_last_send_ns = crate::hrtimer::now_ns();
            conn.last_activity_ns = conn.tx_last_send_ns;
            // ECN: add CWR flag on data segments when we've reduced
            // cwnd for a congestion event (tells peer to stop echoing ECE).
            let mut tcp_flags = TCP_ACK | TCP_PSH;
            let ip_ecn_val = if conn.ecn_ok {
                if conn.ecn_cwr_sent {
                    tcp_flags |= TCP_CWR;
                }
                ipv4::ECN_ECT0
            } else {
                0
            };
            // Drain flushed data from nagle_buf.
            let n_ts_ok = conn.ts_ok;
            let n_ts_recent = conn.ts_recent;
            conn.nagle_buf.drain(..flush_len);
            drop(conns);
            let _ = send_data_with_ts(
                lp, ri, rp, s, a, tcp_flags, w,
                &flush_data[..flush_len], ip_ecn_val,
                n_ts_ok, n_ts_recent,
            );
        }
        // Return the number of bytes actually buffered, not data.len().
        // When the Nagle buffer is nearly full, copy_len < data.len() and
        // the remaining bytes haven't been accepted — the caller must
        // retry with the remainder (standard POSIX send semantics).
        return Ok(copy_len);
    }

    // Effective window = min(cwnd, snd_wnd).  Limit data to what we can
    // have in flight.
    let sendable = data.len().min(eff_wnd);

    if sendable == 0 && !data.is_empty() {
        // Peer advertised a zero window (or cwnd is exhausted).  Activate
        // the persist timer so tick_persist() will probe periodically until
        // the window opens.  The caller gets WouldBlock and retries later.
        let mut conns = CONNECTIONS.lock();
        if let Some(conn) = conns.get_mut(handle) {
            if conn.snd_wnd == 0 && !conn.persist_active {
                conn.persist_active = true;
                conn.persist_interval_ns = conn.rto_ns.max(PERSIST_MIN_NS);
                conn.persist_last_ns = crate::hrtimer::now_ns();
            }
        }
        return Err(KernelError::WouldBlock);
    }

    let send_data = &data[..sendable];

    let mut offset = 0;
    let mut first_seq = seq;

    // ECN: set CWR on data segments when we've reduced cwnd in response
    // to peer's ECE (RFC 3168 §6.1.2).  Mark IP header ECT(0) for
    // ECN-negotiated connections so routers can CE-mark instead of drop.
    let mut data_flags = TCP_ACK | TCP_PSH;
    let ip_ecn_data = if ecn_ok {
        if ecn_cwr {
            data_flags |= TCP_CWR;
        }
        ipv4::ECN_ECT0
    } else {
        0
    };

    while offset < send_data.len() {
        let chunk_end = (offset + eff_mss).min(send_data.len());
        let chunk = &send_data[offset..chunk_end];

        let send_seq = seq.wrapping_add(offset as u32);
        send_data_with_ts(
            local_port, remote_ip, remote_port,
            send_seq, ack,
            data_flags,
            our_wnd,
            chunk,
            ip_ecn_data,
            ts_ok, ts_recent,
        )?;

        if offset == 0 {
            first_seq = send_seq;
        }

        offset = chunk_end;
    }

    // Update snd_nxt, buffer sent data for retransmission, and reset keepalive.
    {
        let mut conns = CONNECTIONS.lock();
        let conn = &mut conns[handle];
        conn.snd_nxt = seq.wrapping_add(sendable as u32);
        let now = crate::hrtimer::now_ns();
        conn.last_activity_ns = now;
        conn.keepalive_probes_sent = 0;
        start_rtt_timing(conn, first_seq);

        // Buffer a copy for retransmission.  Trim to MAX_TX_BUFFER to
        // bound memory usage.
        let space = MAX_TX_BUFFER.saturating_sub(conn.tx_buffer.len());
        let copy_len = sendable.min(space);
        if copy_len > 0 {
            conn.tx_buffer.extend_from_slice(&send_data[..copy_len]);
        }
        // Record send time for RTO-based retransmit.
        if conn.tx_last_send_ns == 0 {
            conn.tx_last_send_ns = now;
        }
    }

    Ok(sendable)
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

// ---------------------------------------------------------------------------
// Connection information / diagnostics
// ---------------------------------------------------------------------------

/// Snapshot of a TCP connection's current state, for diagnostics.
///
/// Returned by [`connection_info`] and [`all_connections`].  All fields
/// are copies — no locks are held after the function returns.
#[derive(Debug, Clone)]
pub struct TcpConnectionInfo {
    /// Connection table index.
    #[allow(dead_code)] // Public API.
    pub handle: usize,
    /// Network namespace this connection belongs to.
    pub ns_id: NetNsId,
    /// Current TCP state.
    pub state: TcpState,
    /// Local port.
    pub local_port: u16,
    /// Remote IP address (IPv4 or IPv6).
    pub remote_ip: IpAddr,
    /// Remote port.
    pub remote_port: u16,
    /// Smoothed RTT in nanoseconds (0 if not yet measured).
    pub srtt_ns: u64,
    /// Current retransmission timeout in nanoseconds.
    pub rto_ns: u64,
    /// Congestion window in bytes.
    pub cwnd: u32,
    /// Slow-start threshold in bytes.
    pub ssthresh: u32,
    /// Peer's advertised receive window (scaled).
    pub snd_wnd: u32,
    /// Effective MSS for outgoing data.
    pub eff_mss: u16,
    /// Peer's raw MSS from SYN/SYN-ACK (0 if not advertised).
    pub peer_mss: u16,
    /// Bytes in the receive buffer (data ready to read).
    pub rx_buffered: usize,
    /// Bytes in the retransmit buffer (unacknowledged in-flight data).
    pub tx_buffered: usize,
    /// Whether ECN was negotiated.
    pub ecn_ok: bool,
    /// Whether SACK was negotiated.
    pub sack_ok: bool,
    /// Whether window scaling was negotiated.
    pub wscale_ok: bool,
    /// Whether TCP timestamps (RFC 7323) were negotiated.
    pub ts_ok: bool,
    /// Whether keepalive probes are enabled.
    pub keepalive: bool,
    /// Whether Nagle's algorithm is enabled.
    pub nagle: bool,
}

/// Query detailed information about a single TCP connection.
///
/// Returns `None` if the handle is invalid or the slot is inactive.
#[allow(dead_code)] // Diagnostic API.
pub fn connection_info(handle: usize) -> Option<TcpConnectionInfo> {
    let conns = CONNECTIONS.lock();
    let conn = conns.get(handle)?;
    if !conn.active {
        return None;
    }
    Some(TcpConnectionInfo {
        handle,
        ns_id: conn.ns_id,
        state: conn.state,
        local_port: conn.local_port,
        remote_ip: conn.remote_ip,
        remote_port: conn.remote_port,
        srtt_ns: conn.srtt_ns_x8 >> SRTT_ALPHA_SHIFT,
        rto_ns: conn.rto_ns,
        cwnd: conn.cwnd,
        ssthresh: conn.ssthresh,
        snd_wnd: conn.snd_wnd,
        eff_mss: effective_mss(conn) as u16,
        peer_mss: conn.peer_mss,
        rx_buffered: conn.rx_buffer.len(),
        tx_buffered: conn.tx_buffer.len(),
        ecn_ok: conn.ecn_ok,
        sack_ok: conn.sack_ok,
        wscale_ok: conn.wscale_ok,
        ts_ok: conn.ts_ok,
        keepalive: conn.keepalive_enabled,
        nagle: conn.nagle_enabled,
    })
}

/// Return information about all active TCP connections.
///
/// Useful for kshell diagnostic commands and network monitoring.
#[allow(dead_code)] // Diagnostic API.
pub fn all_connections() -> Vec<TcpConnectionInfo> {
    let conns = CONNECTIONS.lock();
    let mut result = Vec::new();
    for (idx, conn) in conns.iter().enumerate() {
        if conn.active {
            result.push(TcpConnectionInfo {
                handle: idx,
                ns_id: conn.ns_id,
                state: conn.state,
                local_port: conn.local_port,
                remote_ip: conn.remote_ip,
                remote_port: conn.remote_port,
                srtt_ns: conn.srtt_ns_x8 >> SRTT_ALPHA_SHIFT,
                rto_ns: conn.rto_ns,
                cwnd: conn.cwnd,
                ssthresh: conn.ssthresh,
                snd_wnd: conn.snd_wnd,
                eff_mss: effective_mss(conn) as u16,
                peer_mss: conn.peer_mss,
                rx_buffered: conn.rx_buffer.len(),
                tx_buffered: conn.tx_buffer.len(),
                ecn_ok: conn.ecn_ok,
                sack_ok: conn.sack_ok,
                wscale_ok: conn.wscale_ok,
                ts_ok: conn.ts_ok,
                keepalive: conn.keepalive_enabled,
                nagle: conn.nagle_enabled,
            });
        }
    }
    result
}

/// Information about a TCP listener.
#[derive(Debug, Clone, Copy)]
pub struct TcpListenerInfo {
    /// Listener table index.
    #[allow(dead_code)] // Public API.
    pub handle: usize,
    /// Local port being listened on.
    pub port: u16,
    /// Number of pending (accepted but not yet retrieved) connections.
    pub backlog_used: usize,
    /// Maximum backlog capacity.
    pub backlog_max: usize,
}

/// Return information about all active TCP listeners.
///
/// Uses a fixed-size array to avoid heap allocation (suitable for
/// syscall handlers and interrupt context).
pub fn all_listeners() -> ([TcpListenerInfo; MAX_LISTENERS], usize) {
    let listeners = LISTENERS.lock();
    let mut result = [TcpListenerInfo {
        handle: 0, port: 0, backlog_used: 0, backlog_max: MAX_BACKLOG,
    }; MAX_LISTENERS];
    let mut count = 0;
    for (idx, listener) in listeners.iter().enumerate() {
        if listener.active {
            let used = listener.backlog.iter().filter(|p| p.active).count();
            if let Some(slot) = result.get_mut(count) {
                *slot = TcpListenerInfo {
                    handle: idx,
                    port: listener.port,
                    backlog_used: used,
                    backlog_max: MAX_BACKLOG,
                };
                count = count.wrapping_add(1);
            }
        }
    }
    (result, count)
}

/// Summary statistics about the TCP subsystem.
#[derive(Debug, Clone, Copy)]
pub struct TcpStats {
    /// Number of active connections (any state except Closed).
    pub active_connections: usize,
    /// Number of connections in ESTABLISHED state.
    pub established: usize,
    /// Number of connections in SYN_SENT state.
    pub syn_sent: usize,
    /// Number of connections in TIME_WAIT state.
    pub time_wait: usize,
    /// Number of connections in CLOSE_WAIT state.
    pub close_wait: usize,
    /// Number of active listeners.
    pub listeners: usize,
    /// Total receive buffer bytes across all connections.
    pub total_rx_bytes: usize,
    /// Total transmit buffer bytes across all connections.
    pub total_tx_bytes: usize,
}

/// Return summary statistics about the TCP subsystem.
pub fn stats() -> TcpStats {
    let conns = CONNECTIONS.lock();
    let mut s = TcpStats {
        active_connections: 0,
        established: 0,
        syn_sent: 0,
        time_wait: 0,
        close_wait: 0,
        listeners: 0,
        total_rx_bytes: 0,
        total_tx_bytes: 0,
    };
    for conn in conns.iter() {
        if conn.active {
            s.active_connections = s.active_connections.wrapping_add(1);
            match conn.state {
                TcpState::Established => {
                    s.established = s.established.wrapping_add(1);
                }
                TcpState::SynSent => {
                    s.syn_sent = s.syn_sent.wrapping_add(1);
                }
                TcpState::TimeWait => {
                    s.time_wait = s.time_wait.wrapping_add(1);
                }
                TcpState::CloseWait => {
                    s.close_wait = s.close_wait.wrapping_add(1);
                }
                _ => {}
            }
            s.total_rx_bytes = s.total_rx_bytes.wrapping_add(conn.rx_buffer.len());
            s.total_tx_bytes = s.total_tx_bytes.wrapping_add(conn.tx_buffer.len());
        }
    }
    drop(conns);
    let listeners = LISTENERS.lock();
    for listener in listeners.iter() {
        if listener.active {
            s.listeners = s.listeners.wrapping_add(1);
        }
    }
    s
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

/// Read up to `max_bytes` from a TCP connection's receive buffer.
///
/// Consumes only as many bytes as requested; the remainder stays
/// in the buffer for subsequent reads.  This matches POSIX recv()
/// semantics where the caller specifies a buffer length.
///
/// Returns the data consumed (may be empty if no data available).
pub fn read_up_to(handle: usize, max_bytes: usize) -> KernelResult<Vec<u8>> {
    let (data, need_wnd_update, lp, ri, rp, snd_nxt, rcv_nxt, wnd) = {
        let mut conns = CONNECTIONS.lock();
        let conn = conns.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
        if !conn.active {
            // Connection was reset — report as closed channel.
            return Err(KernelError::ChannelClosed);
        }
        if conn.local_read_closed {
            return Ok(Vec::new());
        }

        if conn.rx_buffer.is_empty() {
            return Ok(Vec::new());
        }

        // Record buffer occupancy before drain (for window update decision).
        let old_free = MAX_RX_BUFFER.saturating_sub(conn.rx_buffer.len());

        let take = max_bytes.min(conn.rx_buffer.len());
        // Split: take the front `take` bytes, keep the rest.
        let remainder = conn.rx_buffer.split_off(take);
        let result = core::mem::replace(&mut conn.rx_buffer, remainder);

        // Window update (RFC 1122 §4.2.3.3): send an ACK with the new
        // window when we've freed enough buffer space.  Specifically,
        // if the old free space was less than one MSS and now it's >= MSS,
        // or if it was 0 and now it's > 0.  This prevents deadlock when
        // the peer stopped sending due to our zero window.
        let new_free = MAX_RX_BUFFER.saturating_sub(conn.rx_buffer.len());
        let mss = effective_mss(conn);
        let should_update = (old_free < mss && new_free >= mss)
            || (old_free == 0 && new_free > 0);

        let wnd = if should_update { advertised_window(conn) } else { 0 };
        (result, should_update,
         conn.local_port, conn.remote_ip, conn.remote_port,
         conn.snd_nxt, conn.rcv_nxt, wnd)
    };

    // Send window update ACK outside the lock.
    if need_wnd_update {
        let _ = send_segment_with_window(
            lp, ri, rp, snd_nxt, rcv_nxt, TCP_ACK, wnd, &[], 0,
        );
    }

    Ok(data)
}

/// Peek at data in a TCP connection's receive buffer without consuming.
///
/// Returns a copy of up to `max_bytes` from the buffer.  The data
/// remains in the buffer for a subsequent `read_up_to()`.  Supports
/// the POSIX `MSG_PEEK` flag.
pub fn peek(handle: usize, max_bytes: usize) -> KernelResult<Vec<u8>> {
    let conns = CONNECTIONS.lock();
    let conn = conns.get(handle).ok_or(KernelError::InvalidArgument)?;
    if !conn.active {
        return Err(KernelError::ChannelClosed);
    }
    if conn.local_read_closed {
        return Ok(Vec::new());
    }

    let take = max_bytes.min(conn.rx_buffer.len());
    let mut data = Vec::new();
    if let Some(slice) = conn.rx_buffer.get(..take) {
        data.extend_from_slice(slice);
    }
    Ok(data)
}

/// Read data from a TCP connection, blocking until data arrives or
/// the connection closes.
///
/// Returns the received data (up to `max_bytes`), or empty Vec if
/// the connection closed.
pub fn read_blocking(handle: usize, timeout_polls: u32, max_bytes: usize) -> KernelResult<Vec<u8>> {
    for _ in 0..timeout_polls {
        super::poll();

        {
            let conns = CONNECTIONS.lock();
            let conn = conns.get(handle).ok_or(KernelError::InvalidArgument)?;
            if !conn.active {
                return Err(KernelError::InvalidArgument);
            }
            // Return immediately if: data available, remote closed (EOF),
            // or local read side shut down.
            if !conn.rx_buffer.is_empty() || conn.remote_closed
                || conn.local_read_closed
            {
                break;
            }
        }

        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
    }

    read_up_to(handle, max_bytes)
}

/// Close a TCP connection.
///
/// If `rst` is true, send RST instead of FIN (abortive close, e.g.,
/// when SO_LINGER timeout is 0 or unread data exists in rx_buffer).
#[allow(clippy::arithmetic_side_effects)]
pub fn close(handle: usize) -> KernelResult<()> {
    let (local_port, remote_ip, remote_port, seq, ack, state, has_unread) = {
        let conns = CONNECTIONS.lock();
        let conn = conns.get(handle).ok_or(KernelError::InvalidArgument)?;
        if !conn.active {
            return Ok(());
        }
        let unread = !conn.rx_buffer.is_empty() && !conn.local_read_closed;
        (conn.local_port, conn.remote_ip, conn.remote_port,
         conn.snd_nxt, conn.rcv_nxt, conn.state, unread)
    };

    // RFC 1122 §4.2.2.13: if the receive buffer contains unread data
    // when close is called, send RST instead of FIN to signal to the
    // peer that the data was discarded (not consumed).
    if has_unread && (state == TcpState::Established || state == TcpState::CloseWait) {
        let _ = send_segment(
            local_port, remote_ip, remote_port,
            seq, ack, TCP_RST | TCP_ACK, &[],
        );
        crate::serial_println!(
            "[tcp] RST close (unread data) on port {} → {}:{}",
            local_port, remote_ip, remote_port
        );
        let mut conns = CONNECTIONS.lock();
        conns[handle].active = false;
        conns[handle].state = TcpState::Closed;
        conns[handle].rx_buffer.clear();
        conns[handle].tx_buffer.clear();
        conns[handle].nagle_buf.clear();
        conns[handle].ooo_buf.clear();
        return Ok(());
    }

    match state {
        TcpState::Established => {
            // Send FIN.
            send_segment(local_port, remote_ip, remote_port, seq, ack, TCP_FIN | TCP_ACK, &[])?;
            let mut conns = CONNECTIONS.lock();
            conns[handle].state = TcpState::FinWait1;
            conns[handle].snd_nxt = seq.wrapping_add(1);
            conns[handle].last_activity_ns = crate::hrtimer::now_ns();

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

            // If the connection completed its teardown (Closed or TimeWait),
            // or timed out in FinWait1, force-deactivate the slot.
            // TimeWait cleanup and FIN retransmission for in-progress
            // teardowns are handled by tick_retransmit / tick_time_wait_cleanup.
            let mut conns = CONNECTIONS.lock();
            let final_state = conns[handle].state;
            if final_state == TcpState::Closed {
                conns[handle].active = false;
                conns[handle].rx_buffer.clear();
                conns[handle].tx_buffer.clear();
                conns[handle].nagle_buf.clear();
                conns[handle].ooo_buf.clear();
            }
            // If still FinWait1/FinWait2/TimeWait, leave slot active for
            // the timer-based handlers (FIN retransmit, TIME_WAIT cleanup).
        }
        TcpState::CloseWait => {
            // Remote already sent FIN; send our FIN.
            send_segment(local_port, remote_ip, remote_port, seq, ack, TCP_FIN | TCP_ACK, &[])?;
            let mut conns = CONNECTIONS.lock();
            conns[handle].state = TcpState::LastAck;
            conns[handle].snd_nxt = seq.wrapping_add(1);
            conns[handle].last_activity_ns = crate::hrtimer::now_ns();
            // Leave slot active — tick_retransmit() handles FIN retransmission
            // if the ACK is lost, and deactivation happens when the ACK arrives.
        }
        TcpState::SynSent => {
            // Connection never established — just deactivate.
            let mut conns = CONNECTIONS.lock();
            conns[handle].active = false;
            conns[handle].state = TcpState::Closed;
            conns[handle].rx_buffer.clear();
            conns[handle].tx_buffer.clear();
            conns[handle].nagle_buf.clear();
            conns[handle].ooo_buf.clear();
        }
        _ => {
            // Already closing or other state — force deactivate.
            let mut conns = CONNECTIONS.lock();
            conns[handle].active = false;
            conns[handle].state = TcpState::Closed;
            conns[handle].rx_buffer.clear();
            conns[handle].tx_buffer.clear();
            conns[handle].nagle_buf.clear();
            conns[handle].ooo_buf.clear();
        }
    }

    Ok(())
}

/// Shut down part of a TCP connection (half-close).
///
/// `how`: 0 = SHUT_RD, 1 = SHUT_WR, 2 = SHUT_RDWR.
///
/// - **SHUT_RD**: marks the read side closed.  Incoming data is ACKed
///   but discarded; reads return empty (EOF).
/// - **SHUT_WR**: sends a FIN to the peer, transitioning to FIN_WAIT_1
///   (or LAST_ACK if already in CLOSE_WAIT).  No more sends allowed.
/// - **SHUT_RDWR**: both of the above.
///
/// Unlike `close()`, the connection slot remains active for the
/// remaining half that is still open.
#[allow(clippy::arithmetic_side_effects)]
pub fn shutdown(handle: usize, how: u32) -> KernelResult<()> {
    let shut_rd = how == 0 || how == 2;
    let shut_wr = how == 1 || how == 2;

    if shut_rd {
        let mut conns = CONNECTIONS.lock();
        let conn = conns.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
        if !conn.active {
            return Err(KernelError::InvalidArgument);
        }
        conn.local_read_closed = true;
        conn.rx_buffer.clear();
    }

    if shut_wr {
        let (local_port, remote_ip, remote_port, seq, ack, state) = {
            let conns = CONNECTIONS.lock();
            let conn = conns.get(handle).ok_or(KernelError::InvalidArgument)?;
            if !conn.active {
                return Err(KernelError::InvalidArgument);
            }
            if conn.local_write_closed {
                // Already shut down for writing.
                return Ok(());
            }
            (conn.local_port, conn.remote_ip, conn.remote_port,
             conn.snd_nxt, conn.rcv_nxt, conn.state)
        };

        match state {
            TcpState::Established => {
                // Send FIN, transition to FIN_WAIT_1 but keep connection
                // active for reading.
                send_segment(local_port, remote_ip, remote_port, seq, ack, TCP_FIN | TCP_ACK, &[])?;
                let mut conns = CONNECTIONS.lock();
                conns[handle].state = TcpState::FinWait1;
                conns[handle].snd_nxt = seq.wrapping_add(1);
                conns[handle].local_write_closed = true;
            }
            TcpState::CloseWait => {
                // Remote already sent FIN; our FIN completes the close.
                send_segment(local_port, remote_ip, remote_port, seq, ack, TCP_FIN | TCP_ACK, &[])?;
                let mut conns = CONNECTIONS.lock();
                conns[handle].state = TcpState::LastAck;
                conns[handle].snd_nxt = seq.wrapping_add(1);
                conns[handle].local_write_closed = true;
            }
            TcpState::SynSent | TcpState::SynReceived => {
                // Not yet established — just mark write closed.
                let mut conns = CONNECTIONS.lock();
                conns[handle].local_write_closed = true;
            }
            _ => {
                // Already in a closing state (FinWait1, FinWait2, etc.).
                let mut conns = CONNECTIONS.lock();
                conns[handle].local_write_closed = true;
            }
        }
    }

    Ok(())
}

/// Abort a TCP connection by sending RST to the peer.
///
/// Unlike `close()` which performs an orderly shutdown (FIN handshake),
/// `abort()` immediately sends RST and reclaims the connection slot.
/// Use this when:
/// - A process exits without closing its sockets
/// - An unrecoverable error makes orderly shutdown pointless
/// - The application wants to signal an error to the peer
///
/// The peer will see a "connection reset" error on its next read/write.
pub fn abort(handle: usize) -> KernelResult<()> {
    let mut conns = CONNECTIONS.lock();
    let conn = conns.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
    if !conn.active {
        return Ok(());
    }

    // Only send RST if the connection was at least partially established.
    let should_rst = matches!(
        conn.state,
        TcpState::Established
            | TcpState::FinWait1
            | TcpState::FinWait2
            | TcpState::CloseWait
            | TcpState::LastAck
            | TcpState::SynReceived
    );

    let local_port = conn.local_port;
    let remote_ip = conn.remote_ip;
    let remote_port = conn.remote_port;
    let snd_nxt = conn.snd_nxt;
    let rcv_nxt = conn.rcv_nxt;

    // Immediately reclaim the slot.
    conn.last_error = TCP_ERR_RESET; // Aborted by local side.
    conn.active = false;
    conn.state = TcpState::Closed;
    conn.rx_buffer.clear();
    conn.tx_buffer.clear();
    conn.nagle_buf.clear();
    conn.ooo_buf.clear();

    drop(conns);

    // Send RST to the peer (best-effort — send failures don't matter
    // since we're aborting anyway).
    if should_rst {
        let _ = send_segment(
            local_port, remote_ip, remote_port,
            snd_nxt, rcv_nxt, TCP_RST | TCP_ACK, &[],
        );
        crate::serial_println!(
            "[tcp] Connection aborted (RST sent): port {} → {}:{}",
            local_port, remote_ip, remote_port
        );
    }

    Ok(())
}

/// Get the peer (remote) address and port for a connection.
///
/// Returns `(ip, port)` if the handle is valid and active.
pub fn peer_addr(handle: usize) -> Option<(IpAddr, u16)> {
    let conns = CONNECTIONS.lock();
    let conn = conns.get(handle)?;
    if !conn.active {
        return None;
    }
    Some((conn.remote_ip, conn.remote_port))
}

/// Get the local port for a connection.
pub fn local_port(handle: usize) -> Option<u16> {
    let conns = CONNECTIONS.lock();
    let conn = conns.get(handle)?;
    if !conn.active {
        return None;
    }
    Some(conn.local_port)
}

/// Query the local port of a TCP listener.
///
/// Returns the port the listener is bound to, or `None` if the
/// handle is invalid or the listener is inactive.
pub fn listener_local_port(handle: usize) -> Option<u16> {
    let listeners = LISTENERS.lock();
    let listener = listeners.get(handle)?;
    if !listener.active {
        return None;
    }
    Some(listener.port)
}

/// Check if a connection's remote end has closed.
/// Check if reading from this connection should return EOF (0 bytes).
///
/// Returns true when:
/// - The remote end sent FIN (`remote_closed`)
/// - The local read side was shut down (`local_read_closed`)
/// - The connection is in CloseWait (remote FIN received)
/// - The handle doesn't exist or is inactive
///
/// This is used by `sys_tcp_recv` to distinguish "no data yet, wait"
/// (WouldBlock) from "connection is done, return EOF" (0).
pub fn is_remote_closed(handle: usize) -> bool {
    let conns = CONNECTIONS.lock();
    conns.get(handle)
        .map(|c| c.remote_closed || c.local_read_closed || c.state == TcpState::CloseWait)
        .unwrap_or(true)
}

// ---------------------------------------------------------------------------
// Connection error codes (for getsockopt SO_ERROR)
// ---------------------------------------------------------------------------

/// No error — connection closed normally.
pub const TCP_ERR_NONE: u8 = 0;
/// Connection refused — RST received while in SYN_SENT.
pub const TCP_ERR_REFUSED: u8 = 1;
/// Connection reset — RST received on established/closing connection.
pub const TCP_ERR_RESET: u8 = 2;
/// Connection timed out — retransmission limit exceeded.
pub const TCP_ERR_TIMEDOUT: u8 = 3;

/// Query the last error for a TCP connection.
///
/// Returns the error code that caused the connection to become inactive.
/// Used by `getsockopt(SO_ERROR)` to report the correct POSIX error:
/// - `TCP_ERR_NONE` (0) → no pending error
/// - `TCP_ERR_REFUSED` (1) → `ECONNREFUSED`
/// - `TCP_ERR_RESET` (2) → `ECONNRESET`
/// - `TCP_ERR_TIMEDOUT` (3) → `ETIMEDOUT`
pub fn last_error(handle: usize) -> u8 {
    let conns = CONNECTIONS.lock();
    conns.get(handle).map_or(TCP_ERR_RESET, |c| c.last_error)
}

/// Read and clear the pending error code for a connection.
///
/// Used by `getsockopt(SO_ERROR)` which requires clear-on-read semantics
/// per POSIX: after reading, the pending error is reset to zero.
pub fn take_last_error(handle: usize) -> u8 {
    let mut conns = CONNECTIONS.lock();
    conns.get_mut(handle).map_or(TCP_ERR_RESET, |c| {
        let err = c.last_error;
        c.last_error = TCP_ERR_NONE;
        err
    })
}

// ---------------------------------------------------------------------------
// Poll readiness (for POSIX poll/select)
// ---------------------------------------------------------------------------

/// Poll readiness bits returned by `poll_status()`.
///
/// Matches POSIX semantics: POLLIN means data can be read without blocking,
/// POLLOUT means data can be written without blocking, POLLHUP means the
/// remote end has closed, POLLERR means the connection encountered an error.
pub const POLL_READABLE: u16 = 0x0001;
pub const POLL_WRITABLE: u16 = 0x0004;
pub const POLL_ERROR: u16    = 0x0008;
pub const POLL_HANGUP: u16   = 0x0010;

/// Query the poll readiness status of a TCP connection.
///
/// Returns a bitmask of `POLL_*` flags indicating what operations can
/// proceed without blocking.  Returns 0 if the handle is invalid.
///
/// - `POLL_READABLE` — rx_buffer has data, or remote has closed (EOF).
/// - `POLL_WRITABLE` — connection is established and send window > 0.
/// - `POLL_HANGUP`   — remote end closed (FIN received).
/// - `POLL_ERROR`    — connection was reset or is in an error state.
///
/// When a connection has been reset or refused (active=false), all of
/// READABLE|WRITABLE|ERROR|HANGUP are returned.  POSIX requires POLLOUT
/// on connect completion (success or failure) so applications using the
/// standard `poll(POLLOUT)` pattern for non-blocking connect detect failure.
/// POLLIN is also set because read() won't block (returns error/EOF).
pub fn poll_status(handle: usize) -> u16 {
    let conns = CONNECTIONS.lock();
    let conn = match conns.get(handle) {
        Some(c) if c.active => c,
        Some(_) => {
            // Connection slot exists but was deactivated (RST, timeout, refused,
            // or fully closed).  Report ready for all operations — they will
            // return errors immediately without blocking.
            return POLL_READABLE | POLL_WRITABLE | POLL_ERROR | POLL_HANGUP;
        }
        None => return POLL_HANGUP | POLL_ERROR,
    };

    let mut flags: u16 = 0;

    // Readable: data in rx_buffer or remote closed (EOF condition),
    // unless the local read side was shut down (then no more readability).
    if !conn.local_read_closed {
        if !conn.rx_buffer.is_empty() || conn.remote_closed {
            flags |= POLL_READABLE;
        }
    }

    // Writable: connection is established, send window allows data,
    // and the local write side hasn't been shut down.
    if !conn.local_write_closed {
        match conn.state {
            TcpState::Established | TcpState::CloseWait => {
                // Writable if congestion/flow-control window allows ≥1 byte.
                let ew = effective_window(conn);
                if ew > 0 {
                    flags |= POLL_WRITABLE;
                }
            }
            _ => {}
        }
    }

    // Hangup: remote closed or local write side shut down (peer sees EOF).
    if conn.remote_closed {
        flags |= POLL_HANGUP;
    }

    flags
}

/// Check if a TCP listener has pending connections ready to accept.
///
/// Returns `true` if at least one completed connection is in the backlog.
/// Returns `false` for invalid or inactive listeners.
pub fn listener_has_pending(listener_handle: usize) -> bool {
    let listeners = LISTENERS.lock();
    let listener = match listeners.get(listener_handle) {
        Some(l) if l.active => l,
        _ => return false,
    };

    listener.backlog.iter().any(|p| p.active)
}

// ---------------------------------------------------------------------------
// Server API (bind / listen / accept)
// ---------------------------------------------------------------------------

/// Bind a TCP listener to a local port within a network namespace.
///
/// `ns_id` identifies the network namespace; pass `netns::ROOT_NS` for
/// the host namespace.  The same port can be bound in different
/// namespaces without conflict.
///
/// Returns a listener handle that can be used with `accept()`.
/// The listener starts accepting connections immediately.
///
/// # Errors
///
/// - `InvalidArgument` — port is 0.
/// - `AlreadyExists` — another listener in the same namespace is already
///   bound to this port.
/// - `OutOfMemory` — no free listener slots.
pub fn bind(ns_id: NetNsId, port: u16) -> KernelResult<usize> {
    if port == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let mut listeners = LISTENERS.lock();

    // Check for duplicate binding within the same namespace.
    for listener in listeners.iter() {
        if listener.active && listener.ns_id == ns_id && listener.port == port {
            return Err(KernelError::AlreadyExists);
        }
    }

    // Find a free slot.
    let slot = listeners.iter().position(|l| !l.active)
        .ok_or(KernelError::OutOfMemory)?;

    listeners[slot].active = true;
    listeners[slot].ns_id = ns_id;
    listeners[slot].port = port;
    // Clear backlog.
    for pending in &mut listeners[slot].backlog {
        pending.active = false;
    }

    crate::serial_println!("[tcp] Listener bound to port {} (ns {})", port, ns_id);
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

    // Collect connection metadata for RST, then deactivate.
    struct RstInfo {
        local_port: u16,
        remote_ip: IpAddr,
        remote_port: u16,
        seq: u32,
        ack: u32,
    }
    let mut rst_targets: [Option<RstInfo>; MAX_BACKLOG] = [const { None }; MAX_BACKLOG];
    {
        let mut conns = CONNECTIONS.lock();
        for (i, handle) in pending_handles.into_iter().enumerate() {
            let Some(h) = handle else { continue };
            if let Some(conn) = conns.get_mut(h) {
                if conn.active {
                    rst_targets[i] = Some(RstInfo {
                        local_port: conn.local_port,
                        remote_ip: conn.remote_ip,
                        remote_port: conn.remote_port,
                        seq: conn.snd_nxt,
                        ack: conn.rcv_nxt,
                    });
                    conn.last_error = TCP_ERR_TIMEDOUT;
                    conn.active = false;
                    conn.state = TcpState::Closed;
                    conn.rx_buffer.clear();
                    conn.tx_buffer.clear();
                    conn.nagle_buf.clear();
                    conn.ooo_buf.clear();
                }
            }
        }
    }
    // Send RSTs to peers of unaccepted connections.
    for info in rst_targets.iter().flatten() {
        let _ = send_segment(
            info.local_port, info.remote_ip, info.remote_port,
            info.seq, info.ack, TCP_RST | TCP_ACK, &[],
        );
    }

    crate::serial_println!("[tcp] Listener on port {} closed", port);
    Ok(())
}

// ---------------------------------------------------------------------------
// Server-side passive open helpers
// ---------------------------------------------------------------------------

/// Handle an incoming SYN for a listening port (passive open step 1).
///
/// If a listener is bound on `dst_port` in the specified namespace,
/// allocate a new connection in `SynReceived` state, send SYN-ACK.
/// When the ACK arrives, the connection transitions to `Established`
/// and is queued to the listener's backlog for `accept()`.
///
/// `option_bytes` contains the TCP options from the SYN segment (bytes
/// 20..data_offset of the TCP header).  Used to parse window scale.
///
/// `syn_flags` is the full TCP flags byte from the SYN segment, used to
/// detect ECN negotiation (RFC 3168 §6.1.1: ECE+CWR in SYN).
#[allow(clippy::arithmetic_side_effects)]
fn handle_incoming_syn(
    ns_id: NetNsId,
    remote_ip: IpAddr,
    remote_port: u16,
    local_port: u16,
    remote_seq: u32,
    remote_window: u16,
    option_bytes: &[u8],
    syn_flags: u8,
) -> KernelResult<()> {
    // Find the listener for this port.
    // Same ns_id logic as connection lookup: physical NIC (ROOT_NS) can
    // reach listeners in any namespace; veth-delivered SYNs are restricted.
    // Returns the listener's actual ns_id so accepted connections inherit
    // the correct namespace (not ROOT_NS from the physical NIC path).
    let listener_ns = {
        let listeners = LISTENERS.lock();
        listeners.iter().find(|l| {
            l.active
                && (ns_id == crate::netns::ROOT_NS || l.ns_id == ns_id)
                && l.port == local_port
        }).map(|l| l.ns_id)
    };

    let Some(effective_ns) = listener_ns else {
        // No listener — send RST.
        let rst_ack = remote_seq.wrapping_add(1);
        let _ = send_segment(
            local_port, remote_ip, remote_port,
            0, rst_ack, TCP_RST | TCP_ACK, &[],
        );
        return Ok(());
    };

    // Parse TCP options from the SYN.
    let syn_opts = parse_tcp_options(option_bytes);

    // Allocate a connection slot for this incoming connection.
    // If the table is full, recycle the oldest TIME_WAIT connection.
    let isn = generate_isn();
    let handle = {
        let mut conns = CONNECTIONS.lock();
        let slot = match conns.iter().position(|c| !c.active) {
            Some(idx) => idx,
            None => {
                // No free slots — try to reclaim a TIME_WAIT connection.
                let mut best: Option<usize> = None;
                let mut oldest_activity: u64 = u64::MAX;
                for (i, c) in conns.iter().enumerate() {
                    if c.active && c.state == TcpState::TimeWait {
                        if c.last_activity_ns < oldest_activity {
                            oldest_activity = c.last_activity_ns;
                            best = Some(i);
                        }
                    }
                }
                let idx = best.ok_or(KernelError::OutOfMemory)?;
                crate::serial_println!(
                    "[tcp] Recycling TIME_WAIT slot {} (port {}) for incoming SYN",
                    idx, conns[idx].local_port
                );
                conns[idx].active = false;
                conns[idx].state = TcpState::Closed;
                conns[idx].rx_buffer.clear();
                conns[idx].tx_buffer.clear();
                conns[idx].nagle_buf.clear();
                conns[idx].ooo_buf.clear();
                idx
            }
        };

        let conn = &mut conns[slot];
        conn.active = true;
        conn.ns_id = effective_ns;
        conn.state = TcpState::SynReceived;
        conn.last_error = TCP_ERR_NONE; // Server-side connections start clean.
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
        conn.tx_buffer.clear();
        conn.nagle_buf.clear();
        conn.tx_buf_seq = isn.wrapping_add(1); // After SYN-ACK sequence.
        conn.tx_last_send_ns = 0;
        conn.ooo_buf.clear();
        conn.ooo_base = remote_seq.wrapping_add(1);
        conn.remote_closed = false;
        conn.local_write_closed = false;
        conn.local_read_closed = false;
        conn.dup_ack_count = 0;
        conn.retransmit_timer = 0;
        conn.retransmit_count = 0; // Clear stale count from recycled slot.
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

        // SACK (RFC 2018): both sides must send SACK-Permitted.
        // We always send it in our SYN-ACK, so it's active if the
        // client sent it too.
        conn.sack_ok = syn_opts.sack_permitted;
        conn.sack_blocks = [(0, 0); MAX_SACK_BLOCKS];
        conn.sack_block_count = 0;

        // MSS: store the client's advertised MSS so we limit our
        // outgoing segments accordingly (RFC 793 §3.1).
        conn.peer_mss = syn_opts.mss;

        // ECN (RFC 3168 §6.1.1): if the client's SYN has both ECE and CWR
        // set, it supports ECN.  We confirm by setting ECE (without CWR) in
        // our SYN-ACK.
        conn.ecn_ok = (syn_flags & TCP_ECE != 0) && (syn_flags & TCP_CWR != 0);
        conn.ecn_ce_pending = false;
        conn.ecn_cwr_sent = false;
        conn.persist_active = false;
        conn.persist_interval_ns = PERSIST_MIN_NS;
        conn.persist_last_ns = 0;

        // Timestamps (RFC 7323 §3.2): if the client's SYN included a
        // Timestamp option, we negotiate timestamps and store its TSval
        // as ts_recent so we can echo it in the SYN-ACK.
        if let Some((peer_tsval, _)) = syn_opts.timestamp {
            conn.ts_ok = true;
            conn.ts_recent = peer_tsval;
            conn.ts_recent_age_ns = crate::hrtimer::now_ns();
        } else {
            conn.ts_ok = false;
            conn.ts_recent = 0;
            conn.ts_recent_age_ns = 0;
        }

        conn.keepalive_enabled = false;
        conn.keepalive_idle_ns = KEEPALIVE_IDLE_DEFAULT_NS;
        conn.keepalive_interval_ns = KEEPALIVE_INTERVAL_DEFAULT_NS;
        conn.keepalive_probes_max = KEEPALIVE_PROBES_DEFAULT;
        conn.keepalive_probes_sent = 0;
        conn.last_activity_ns = crate::hrtimer::now_ns();
        slot
    };

    // Send SYN-ACK with WScale + Timestamp options (if the client sent them).
    // ECN: add ECE flag if ECN was negotiated (RFC 3168 §6.1.1).
    let rcv_nxt = remote_seq.wrapping_add(1);
    let (ecn_negotiated, ts_negotiated, ts_recent_echo) = {
        let conns = CONNECTIONS.lock();
        conns.get(handle).map_or((false, false, 0), |c| (c.ecn_ok, c.ts_ok, c.ts_recent))
    };
    let synack_flags = if ecn_negotiated {
        TCP_SYN | TCP_ACK | TCP_ECE
    } else {
        TCP_SYN | TCP_ACK
    };

    if syn_opts.wscale.is_some() {
        // SYN-ACK with full options (WScale + SACK-Perm + Timestamp).
        // Timestamp TSecr echoes the client's TSval from the SYN.
        send_syn_segment(
            local_port, remote_ip, remote_port,
            isn, rcv_nxt, synack_flags,
            DEFAULT_WINDOW, OUR_WSCALE,
            tcp_now_ms(),
            if ts_negotiated { ts_recent_echo } else { 0 },
        )?;
    } else {
        // Client doesn't support window scaling — plain SYN-ACK.
        send_segment(
            local_port, remote_ip, remote_port,
            isn, rcv_nxt, synack_flags, &[],
        )?;
    }

    crate::serial_println!(
        "[tcp] SYN-ACK sent to {}:{} (handle={}, isn={}, wscale={}, ts={})",
        remote_ip, remote_port, handle, isn,
        if syn_opts.wscale.is_some() { OUR_WSCALE } else { 0 },
        ts_negotiated
    );

    Ok(())
}

/// Place a fully-established connection into its listener's backlog.
fn enqueue_to_listener(ns_id: NetNsId, local_port: u16, conn_handle: usize) {
    let mut listeners = LISTENERS.lock();
    for listener in listeners.iter_mut() {
        if listener.active && listener.ns_id == ns_id && listener.port == local_port {
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
// TCP segment processing (dual-stack: IPv4 and IPv6 entry points)
// ---------------------------------------------------------------------------

/// Process an incoming TCP segment received via IPv4.
///
/// Verifies the IPv4 pseudo-header checksum, wraps the source address
/// in `IpAddr::V4`, and delegates to the shared TCP state machine.
#[allow(clippy::arithmetic_side_effects)]
pub fn process_tcp(ip_packet: &Ipv4Packet<'_>, ns_id: NetNsId) -> KernelResult<()> {
    let data = ip_packet.payload;
    if data.len() < TCP_HEADER_SIZE {
        return Ok(());
    }

    // Verify TCP checksum (IPv4 pseudo-header + segment).
    if !ipv4::verify_transport_checksum(
        ip_packet.src, ip_packet.dst, PROTO_TCP, data,
    ) {
        crate::serial_println!(
            "[tcp] Dropped segment from {} — bad checksum",
            ip_packet.src
        );
        return Ok(());
    }

    let remote_addr = IpAddr::V4(ip_packet.src);
    let ip_ecn_ce = ip_packet.ecn == ipv4::ECN_CE;

    // `ns_id` is the namespace the frame arrived in: ROOT_NS for the
    // physical NIC, or a container namespace for veth-delivered frames.
    process_tcp_common(ns_id, remote_addr, data, ip_ecn_ce)
}

/// Process an incoming TCP segment received via IPv6.
///
/// Verifies the IPv6 pseudo-header checksum, wraps the source address
/// in `IpAddr::V6`, and delegates to the shared TCP state machine.
///
/// ECN is extracted from the IPv6 traffic class field (low 2 bits,
/// same encoding as IPv4).
#[allow(clippy::arithmetic_side_effects)]
pub fn process_tcp_v6(ip_packet: &Ipv6Packet<'_>, ns_id: NetNsId) -> KernelResult<()> {
    let data = ip_packet.payload;
    if data.len() < TCP_HEADER_SIZE {
        return Ok(());
    }

    // Verify TCP checksum (IPv6 pseudo-header + segment).
    if !ipv6::verify_transport_checksum(
        &ip_packet.src, &ip_packet.dst, PROTO_TCP, data,
    ) {
        crate::serial_println!(
            "[tcp] Dropped IPv6 segment from {} — bad checksum",
            ip_packet.src
        );
        return Ok(());
    }

    let remote_addr = IpAddr::V6(ip_packet.src);
    // IPv6 traffic class carries ECN in bits 0-1 (same encoding as IPv4).
    let ip_ecn_ce = (ip_packet.traffic_class & 0x03) == ipv4::ECN_CE;

    // `ns_id` is the arrival namespace (ROOT_NS for the physical NIC).
    process_tcp_common(ns_id, remote_addr, data, ip_ecn_ce)
}

/// Shared TCP segment processing for both IPv4 and IPv6.
///
/// The caller has already verified the transport checksum and extracted
/// the remote address and ECN congestion-experienced flag.
///
/// `ns_id` — network namespace this packet arrived in (ROOT_NS for
///   physical NIC, container NS for veth-delivered packets).
/// `remote_addr` — sender's IP (IpAddr::V4 or V6).
/// `data` — raw TCP segment bytes (header + options + payload).
/// `ip_ecn_ce` — true if the IP header indicated Congestion Experienced.
#[allow(clippy::arithmetic_side_effects)]
fn process_tcp_common(ns_id: NetNsId, remote_addr: IpAddr, data: &[u8], ip_ecn_ce: bool) -> KernelResult<()> {
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
    //
    // When packets arrive from the physical NIC (ns_id == ROOT_NS), we
    // search ALL namespaces because ip_send_tcp() sends all outgoing TCP
    // through the host's single IP regardless of the connection's namespace.
    // Replies arrive on the host IP and must match container connections too.
    //
    // When packets arrive via veth (ns_id != ROOT_NS), we restrict the
    // lookup to that specific namespace (veth traffic is namespace-isolated).
    let mut conns = CONNECTIONS.lock();
    let conn_idx = conns.iter().position(|c| {
        c.active
            && (ns_id == crate::netns::ROOT_NS || c.ns_id == ns_id)
            && c.local_port == dst_port
            && c.remote_ip == remote_addr
            && c.remote_port == src_port
    });

    let Some(idx) = conn_idx else {
        // No matching connection — check if this is a SYN for a listener
        // (passive open / server-side handshake).
        if flags & TCP_SYN != 0 && flags & TCP_ACK == 0 && flags & TCP_RST == 0 {
            drop(conns);
            return handle_incoming_syn(
                ns_id, remote_addr, src_port, dst_port, seq, window,
                option_bytes, flags,
            );
        }
        // No matching connection and not a SYN for a listener.
        // Send RST if this isn't itself a RST.
        if flags & TCP_RST == 0 {
            drop(conns);
            let rst_seq = if flags & TCP_ACK != 0 { ack } else { 0 };
            let rst_ack = seq.wrapping_add(payload.len() as u32);
            let _ = send_segment(
                dst_port, remote_addr, src_port,
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

    // Window opened — deactivate persist timer so the sender can resume
    // normal transmission.  Reset the backoff interval for next time.
    if conn.snd_wnd > 0 && conn.persist_active {
        conn.persist_active = false;
        conn.persist_interval_ns = PERSIST_MIN_NS;
        conn.persist_last_ns = 0;
    }

    // Timestamps / PAWS (RFC 7323 §4):
    // Parse the Timestamp option from the incoming segment.  If timestamps
    // were negotiated, perform PAWS check: reject segments with TSval older
    // than ts_recent (protects against wrapped sequence numbers and old
    // duplicate segments).  Then update ts_recent with the segment's TSval.
    let seg_opts = parse_tcp_options(option_bytes);
    if conn.ts_ok {
        if let Some((tsval, _tsecr)) = seg_opts.timestamp {
            // PAWS check (RFC 7323 §5.2): drop segments with TSval
            // strictly less than ts_recent, unless ts_recent hasn't been
            // updated in >24 days (the timestamp clock could have wrapped
            // on the peer side).
            //
            // We use wrapping subtraction: if tsval < ts_recent in modular
            // arithmetic (high bit set), it's an old duplicate.
            let ts_delta = tsval.wrapping_sub(conn.ts_recent);
            let now_ns = crate::hrtimer::now_ns();
            // 24-day aging window in nanoseconds.
            const PAWS_IDLE_NS: u64 = 24 * 86400 * 1_000_000_000;
            let ts_recent_too_old =
                now_ns.saturating_sub(conn.ts_recent_age_ns) > PAWS_IDLE_NS;

            if ts_delta >= 0x8000_0000 && !ts_recent_too_old && conn.ts_recent != 0 {
                // Old duplicate — drop silently but send an ACK (RFC 7323 §5.2
                // step R4).
                let lp = conn.local_port;
                let ri = conn.remote_ip;
                let rp = conn.remote_port;
                let sn = conn.snd_nxt;
                let rn = conn.rcv_nxt;
                let w = advertised_window(conn);
                let ie = if conn.ecn_ok { ipv4::ECN_ECT0 } else { 0 };
                let paws_ts_recent = conn.ts_recent;
                drop(conns);
                let _ = send_data_with_ts(lp, ri, rp, sn, rn, TCP_ACK, w, &[], ie, true, paws_ts_recent);
                return Ok(());
            }

            // Update ts_recent with the segment's TSval.
            // RFC 7323 §4.3: only accept TSval if the segment is at or
            // past the expected sequence (no older data).
            let seq_delta = seq.wrapping_sub(conn.rcv_nxt);
            if seq_delta < 0x8000_0000 || seq == conn.rcv_nxt {
                conn.ts_recent = tsval;
                conn.ts_recent_age_ns = now_ns;
            }
        }
    }

    // ECN: if the IP header has CE (Congestion Experienced) set and
    // ECN was negotiated, remember that we need to echo ECE in our
    // next ACK so the sender knows to reduce its congestion window.
    if conn.ecn_ok && ip_ecn_ce {
        conn.ecn_ce_pending = true;
    }

    // ECN: if the peer sends CWR, it has acknowledged our ECE — we
    // can stop echoing ECE.
    if conn.ecn_ok && (flags & TCP_CWR != 0) {
        conn.ecn_ce_pending = false;
    }

    // Any incoming segment counts as activity for keepalive purposes.
    conn.last_activity_ns = crate::hrtimer::now_ns();
    conn.keepalive_probes_sent = 0;

    // Handle RST.
    if flags & TCP_RST != 0 {
        crate::serial_println!("[tcp] RST received — connection reset");
        // RST on SYN_SENT = connection refused; on established+ = reset.
        conn.last_error = if conn.state == TcpState::SynSent {
            TCP_ERR_REFUSED
        } else {
            TCP_ERR_RESET
        };
        conn.active = false;
        conn.state = TcpState::Closed;
        conn.rx_buffer.clear();
        conn.tx_buffer.clear();
        conn.nagle_buf.clear();
        conn.ooo_buf.clear();
        return Ok(());
    }

    // Receive window validation (RFC 793 §3.3).
    //
    // For states with a defined receive window (Established and later),
    // check that the segment's sequence number falls within the acceptable
    // window [rcv_nxt, rcv_nxt + rcv_wnd).  This rejects stale/replayed
    // segments and prevents unbounded OOO buffer growth from far-future
    // sequence numbers.
    //
    // Skip for SynSent (receive window not yet established — we're waiting
    // for SYN-ACK) and SynReceived (handshake still in progress).
    if conn.state != TcpState::SynSent && conn.state != TcpState::SynReceived {
        let seg_len = payload.len() as u32
            + if flags & TCP_SYN != 0 { 1 } else { 0 }
            + if flags & TCP_FIN != 0 { 1 } else { 0 };
        let rcv_wnd = (advertised_window(conn) as u32) << (conn.rcv_wnd_scale as u32);
        let acceptable = if rcv_wnd == 0 {
            // Zero window: only accept zero-length segment at exactly rcv_nxt.
            seg_len == 0 && seq == conn.rcv_nxt
        } else if seg_len == 0 {
            // Empty segment: rcv_nxt <= seq < rcv_nxt + rcv_wnd.
            let delta = seq.wrapping_sub(conn.rcv_nxt);
            delta < rcv_wnd
        } else {
            // Data segment: first or last byte must fall within the window.
            let start_delta = seq.wrapping_sub(conn.rcv_nxt);
            let end_seq = seq.wrapping_add(seg_len.saturating_sub(1));
            let end_delta = end_seq.wrapping_sub(conn.rcv_nxt);
            start_delta < rcv_wnd || end_delta < rcv_wnd
        };

        if !acceptable {
            // Out-of-window segment — drop and send ACK (RFC 793 §3.3).
            let lp = conn.local_port;
            let ri = conn.remote_ip;
            let rp = conn.remote_port;
            let sn = conn.snd_nxt;
            let rn = conn.rcv_nxt;
            let w = advertised_window(conn);
            let ie = if conn.ecn_ok { ipv4::ECN_ECT0 } else { 0 };
            let ts_ok_local = conn.ts_ok;
            let ts_recent_local = conn.ts_recent;
            drop(conns);
            let _ = send_data_with_ts(
                lp, ri, rp, sn, rn, TCP_ACK, w, &[], ie,
                ts_ok_local, ts_recent_local,
            );
            return Ok(());
        }
    }

    match conn.state {
        TcpState::SynSent => {
            // Expecting SYN-ACK.
            if flags & TCP_SYN != 0 && flags & TCP_ACK != 0 {
                conn.rcv_irs = seq;
                conn.rcv_nxt = seq.wrapping_add(1);
                conn.snd_una = ack;

                // Parse SYN-ACK options for window scaling and SACK.
                let synack_opts = parse_tcp_options(option_bytes);
                if let Some(peer_shift) = synack_opts.wscale {
                    conn.wscale_ok = true;
                    conn.snd_wnd_scale = peer_shift;
                    crate::serial_println!(
                        "[tcp] Window scaling negotiated: snd_shift={}, rcv_shift={}",
                        peer_shift, conn.rcv_wnd_scale
                    );
                } else {
                    conn.wscale_ok = false;
                    conn.snd_wnd_scale = 0;
                    conn.rcv_wnd_scale = 0;
                }

                // SACK: both sides must have sent SACK-Permitted (RFC 2018).
                conn.sack_ok = synack_opts.sack_permitted;

                // MSS: store the peer's advertised MSS to limit our
                // outgoing segment size (RFC 793 §3.1).
                conn.peer_mss = synack_opts.mss;

                // ECN (RFC 3168 §6.1.1): the server confirms ECN support
                // by setting ECE (without CWR) in the SYN-ACK.
                if flags & TCP_ECE != 0 && flags & TCP_CWR == 0 {
                    conn.ecn_ok = true;
                    crate::serial_println!("[tcp] ECN negotiated");
                }

                // Timestamps (RFC 7323 §3.2): if the SYN-ACK included a
                // Timestamp option, timestamps are negotiated.  Store the
                // peer's TSval as ts_recent for echoing in future segments.
                if let Some((peer_tsval, _tsecr)) = synack_opts.timestamp {
                    conn.ts_ok = true;
                    conn.ts_recent = peer_tsval;
                    conn.ts_recent_age_ns = crate::hrtimer::now_ns();
                    crate::serial_println!("[tcp] Timestamps negotiated");
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
                let conn_ns = conn.ns_id;

                // Place this connection in the listener's backlog.
                drop(conns);
                enqueue_to_listener(conn_ns, local_port, idx);

                crate::serial_println!(
                    "[tcp] 3-way handshake complete for {}:{} → port {}",
                    remote_addr, src_port, local_port
                );
            } else if flags & TCP_RST != 0 {
                conn.last_error = TCP_ERR_REFUSED;
                conn.active = false;
                conn.state = TcpState::Closed;
            } else if flags & TCP_SYN != 0 {
                // Retransmitted SYN from client — our SYN-ACK was likely lost.
                // Retransmit the SYN-ACK.  Gather connection info and drop
                // the lock before sending.
                let lp = conn.local_port;
                let ri = conn.remote_ip;
                let rp = conn.remote_port;
                let our_isn = conn.snd_iss;
                let rcv = conn.rcv_nxt;
                let ecn = conn.ecn_ok;
                let ts = conn.ts_ok;
                let ts_recent_echo = conn.ts_recent;
                let wscale = conn.wscale_ok;
                conn.last_activity_ns = crate::hrtimer::now_ns();
                drop(conns);

                let synack_flags = if ecn {
                    TCP_SYN | TCP_ACK | TCP_ECE
                } else {
                    TCP_SYN | TCP_ACK
                };

                if wscale || ts {
                    let _ = send_syn_segment(
                        lp, ri, rp, our_isn, rcv, synack_flags,
                        DEFAULT_WINDOW, OUR_WSCALE,
                        tcp_now_ms(),
                        if ts { ts_recent_echo } else { 0 },
                    );
                } else {
                    let _ = send_segment(lp, ri, rp, our_isn, rcv, synack_flags, &[]);
                }
                crate::serial_println!(
                    "[tcp] SYN-ACK retransmit to {}:{} (port {})", ri, rp, lp
                );
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

                if bytes_acked > 0 {
                    // New data acknowledged — reset retransmit and dup-ACK
                    // counters.  Forward progress means the path is working.
                    conn.dup_ack_count = 0;
                    conn.retransmit_count = 0;

                    // Trim retransmit buffer: discard acknowledged data.
                    tx_buffer_trim(conn, bytes_acked);

                    // RTT sample: prefer timestamps (RFC 7323 §4.1)
                    // which give a sample on every ACK, over the legacy
                    // single-segment timing (Karn's algorithm).
                    if conn.ts_ok {
                        if let Some((_tsval, tsecr)) = seg_opts.timestamp {
                            try_rtt_sample_ts(conn, tsecr);
                        }
                    } else {
                        try_rtt_sample(conn, ack);
                    }

                    // Congestion window update.
                    on_ack_congestion(conn, bytes_acked);

                    // ECN: if the peer sent ECE, it received a CE-marked
                    // packet.  Reduce cwnd (like a loss event) and set
                    // CWR to tell the peer we've responded.  Only reduce
                    // once per congestion event (ecn_cwr_sent guards).
                    if conn.ecn_ok && (flags & TCP_ECE != 0) && !conn.ecn_cwr_sent {
                        on_loss_congestion(conn);
                        conn.ecn_cwr_sent = true;
                        crate::serial_println!(
                            "[tcp] ECN: peer reported congestion (ECE) — cwnd reduced to {}",
                            conn.cwnd
                        );
                    }
                    // Clear CWR-sent once the peer stops sending ECE
                    // (the congestion event is resolved).
                    if conn.ecn_ok && (flags & TCP_ECE == 0) && conn.ecn_cwr_sent {
                        conn.ecn_cwr_sent = false;
                    }

                    // Nagle flush: if all outstanding data is now acked
                    // and the nagle buffer has pending data, send it.
                    if conn.snd_una == conn.snd_nxt && !conn.nagle_buf.is_empty() {
                        let lp = conn.local_port;
                        let ri = conn.remote_ip;
                        let rp = conn.remote_port;
                        let s = conn.snd_nxt;
                        let a = conn.rcv_nxt;
                        let w = advertised_window(conn);
                        let emss = effective_mss(conn);
                        let flush_len = conn.nagle_buf.len().min(emss);
                        let mut flush_data = [0u8; 1460];
                        flush_data[..flush_len]
                            .copy_from_slice(&conn.nagle_buf[..flush_len]);
                        conn.snd_nxt = s.wrapping_add(flush_len as u32);
                        // Buffer for retransmit.
                        let tx_space =
                            MAX_TX_BUFFER.saturating_sub(conn.tx_buffer.len());
                        let tx_copy = flush_len.min(tx_space);
                        if tx_copy > 0 {
                            conn.tx_buffer
                                .extend_from_slice(&flush_data[..tx_copy]);
                        }
                        let now = crate::hrtimer::now_ns();
                        conn.tx_last_send_ns = now;
                        conn.last_activity_ns = now;
                        start_rtt_timing(conn, s);
                        // ECN: CWR on data segments, ECT(0) in IP header.
                        let mut nagle_flags = TCP_ACK | TCP_PSH;
                        let nagle_ecn = if conn.ecn_ok {
                            if conn.ecn_cwr_sent { nagle_flags |= TCP_CWR; }
                            ipv4::ECN_ECT0
                        } else { 0 };
                        let n_ts_ok = conn.ts_ok;
                        let n_ts_recent = conn.ts_recent;
                        conn.nagle_buf.drain(..flush_len);
                        drop(conns);
                        let _ = send_data_with_ts(
                            lp, ri, rp, s, a, nagle_flags, w,
                            &flush_data[..flush_len], nagle_ecn,
                            n_ts_ok, n_ts_recent,
                        );
                        return Ok(());
                    }
                } else if payload.is_empty() {
                    // Duplicate ACK (no new data, no payload).
                    // RFC 5681 §3.2: after 3 duplicate ACKs, enter fast
                    // recovery and retransmit the first unacked segment.
                    conn.dup_ack_count = conn.dup_ack_count.saturating_add(1);
                    if conn.dup_ack_count == 3 {
                        // Fast retransmit trigger — halve congestion window
                        // and retransmit the lost segment.
                        let emss = effective_mss(conn) as u32;
                        let flight = conn.snd_nxt.wrapping_sub(conn.snd_una);
                        conn.ssthresh = (flight / 2).max(emss);
                        conn.cwnd = conn.ssthresh.saturating_add(3u32.saturating_mul(emss));

                        // Retransmit from the tx buffer.
                        let retx_len = conn.tx_buffer.len().min(emss as usize);
                        if retx_len > 0 {
                            let lp = conn.local_port;
                            let ri = conn.remote_ip;
                            let rp = conn.remote_port;
                            let s = conn.snd_una;
                            let a = conn.rcv_nxt;
                            let w = advertised_window(conn);
                            let retx_ecn = if conn.ecn_ok { ipv4::ECN_ECT0 } else { 0 };
                            let r_ts_ok = conn.ts_ok;
                            let r_ts_recent = conn.ts_recent;
                            // Copy data out before dropping the lock.
                            let mut retx_data = [0u8; 1460]; // MSS-sized stack buffer.
                            retx_data[..retx_len].copy_from_slice(&conn.tx_buffer[..retx_len]);
                            drop(conns);
                            let _ = send_data_with_ts(
                                lp, ri, rp, s, a, TCP_ACK | TCP_PSH, w,
                                &retx_data[..retx_len], retx_ecn,
                                r_ts_ok, r_ts_recent,
                            );
                            crate::serial_println!(
                                "[tcp] Fast retransmit: {} bytes from seq {} (port {})",
                                retx_len, s, lp
                            );
                        } else {
                            crate::serial_println!(
                                "[tcp] 3 dup ACKs for port {} — fast recovery (no buffered data)",
                                conn.local_port
                            );
                        }
                        return Ok(());
                    } else if conn.dup_ack_count > 3 {
                        // Additional dup ACKs during fast recovery — inflate cwnd
                        // by one MSS per RFC 5681 §3.2 to allow more in-flight data.
                        conn.cwnd = conn.cwnd.saturating_add(effective_mss(conn) as u32);
                    }
                }
            }

            // Process data.
            if !payload.is_empty() {
                if seq == conn.rcv_nxt {
                    // In-order data — deliver to rx_buffer (unless read
                    // side is shut down, in which case ACK but discard).
                    let can_accept = if conn.local_read_closed {
                        // Read side shut down — discard data but still advance
                        // rcv_nxt so the connection can close gracefully.
                        // The application doesn't want this data.
                        payload.len()
                    } else {
                        MAX_RX_BUFFER.saturating_sub(conn.rx_buffer.len())
                    };
                    let accept = payload.len().min(can_accept);
                    if accept > 0 && !conn.local_read_closed {
                        conn.rx_buffer.extend_from_slice(&payload[..accept]);
                    }
                    // Advance rcv_nxt only by the amount we actually consumed.
                    // When local_read_closed: we accept (and discard) the full
                    // payload to keep the protocol advancing.
                    // When buffer full: advance only by what we stored.
                    // Advancing past data we couldn't store would lie to the
                    // peer (ACKing bytes the application will never see),
                    // silently corrupting the TCP byte stream.  The shrunk
                    // advertised window tells the peer to stop sending.
                    let advance = accept as u32;
                    conn.rcv_nxt = conn.rcv_nxt.wrapping_add(advance);

                    // Deliver contiguous OOO-buffered data now that the
                    // gap has been filled.
                    ooo_deliver(conn);
                    sack_advance(conn);

                    // Send ACK (with SACK blocks if any remain).
                    let _ = send_ack_with_sack(conn);
                    drop(conns);
                    return Ok(());
                } else if conn.sack_ok && seq_lt(conn.rcv_nxt, seq) {
                    // Out-of-order data — record SACK block and buffer
                    // the payload for delivery when the gap fills.
                    let right = seq.wrapping_add(payload.len() as u32);
                    sack_insert(conn, seq, right);
                    ooo_store(conn, seq, payload);

                    // Send duplicate ACK with SACK blocks.
                    let _ = send_ack_with_sack(conn);
                    drop(conns);
                    return Ok(());
                }
                // else: old/retransmitted data (seq < rcv_nxt) — ignore payload,
                // fall through to send a plain ACK.
            }

            // Process FIN — only if all preceding data has been received.
            // FIN's implicit sequence byte follows the payload, so the
            // FIN is in-order when seq + payload_len == rcv_nxt.
            if flags & TCP_FIN != 0
                && seq.wrapping_add(payload.len() as u32) == conn.rcv_nxt
            {
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
            // RFC 793 §3.5: In FIN_WAIT_1 we have sent our FIN but
            // the peer may still be sending data.  We must continue
            // accepting data, processing ACKs (including for our
            // in-flight data and FIN), and handling the peer's FIN.

            // --- ACK processing ---
            if flags & TCP_ACK != 0 {
                let old_una = conn.snd_una;
                if ack.wrapping_sub(conn.snd_una)
                    <= conn.snd_nxt.wrapping_sub(conn.snd_una)
                {
                    conn.snd_una = ack;
                }
                let bytes_acked = conn.snd_una.wrapping_sub(old_una);
                if bytes_acked > 0 {
                    conn.dup_ack_count = 0;
                    conn.retransmit_count = 0;
                    tx_buffer_trim(conn, bytes_acked);
                    if conn.ts_ok {
                        if let Some((_tsval, tsecr)) = seg_opts.timestamp {
                            try_rtt_sample_ts(conn, tsecr);
                        }
                    } else {
                        try_rtt_sample(conn, ack);
                    }
                    on_ack_congestion(conn, bytes_acked);
                }
            }

            // --- Incoming data ---
            // The peer hasn't sent FIN yet (or it's in this same
            // segment after the payload).  Buffer data just like
            // Established so the application can read it.
            if !payload.is_empty() {
                if seq == conn.rcv_nxt {
                    let can_accept = if conn.local_read_closed {
                        payload.len()
                    } else {
                        MAX_RX_BUFFER.saturating_sub(conn.rx_buffer.len())
                    };
                    let accept = payload.len().min(can_accept);
                    if accept > 0 && !conn.local_read_closed {
                        conn.rx_buffer.extend_from_slice(&payload[..accept]);
                    }
                    conn.rcv_nxt = conn.rcv_nxt.wrapping_add(accept as u32);
                    ooo_deliver(conn);
                    sack_advance(conn);
                } else if conn.sack_ok && seq_lt(conn.rcv_nxt, seq) {
                    let right = seq.wrapping_add(payload.len() as u32);
                    sack_insert(conn, seq, right);
                    ooo_store(conn, seq, payload);
                }
            }

            // --- FIN processing ---
            // FIN's implicit seq occupies one byte *after* any payload.
            // By this point rcv_nxt has been advanced past the payload
            // (if any), so FIN seq == rcv_nxt is the correct check.
            if flags & TCP_FIN != 0 && seq.wrapping_add(payload.len() as u32) == conn.rcv_nxt {
                conn.rcv_nxt = conn.rcv_nxt.wrapping_add(1);
                conn.remote_closed = true;

                // Determine next state:
                // - If our FIN has been ACKed (snd_una == snd_nxt after
                //   FIN), go to TIME_WAIT (simultaneous close path).
                // - Otherwise, go to Closing (FIN received, our FIN
                //   not yet ACKed — rare "simultaneous close" case per
                //   RFC 793 Fig. 13).
                if conn.snd_una == conn.snd_nxt {
                    conn.state = TcpState::TimeWait;
                } else {
                    // Our FIN hasn't been ACKed yet.  Go to TimeWait
                    // as a simplification (RFC allows Closing state
                    // but TIME_WAIT is safe — we'll still ACK the
                    // peer's FIN retransmits during the 2MSL wait).
                    conn.state = TcpState::TimeWait;
                }
            } else if flags & TCP_FIN == 0 {
                // No FIN in this segment.  Check if our FIN has been
                // ACKed (transition to FIN_WAIT_2).
                if conn.snd_una == conn.snd_nxt {
                    conn.state = TcpState::FinWait2;
                    conn.last_activity_ns = crate::hrtimer::now_ns();
                }
            }

            // Send ACK for data and/or FIN.
            let _ = send_ack_with_sack(conn);
            conn.last_activity_ns = crate::hrtimer::now_ns();
        }

        TcpState::FinWait2 => {
            // RFC 793 §3.5: Our FIN has been ACKed.  We're still
            // receiving data from the peer until they send their FIN.

            // --- Incoming data ---
            if !payload.is_empty() {
                conn.last_activity_ns = crate::hrtimer::now_ns();
                if seq == conn.rcv_nxt {
                    let can_accept = if conn.local_read_closed {
                        payload.len()
                    } else {
                        MAX_RX_BUFFER.saturating_sub(conn.rx_buffer.len())
                    };
                    let accept = payload.len().min(can_accept);
                    if accept > 0 && !conn.local_read_closed {
                        conn.rx_buffer.extend_from_slice(&payload[..accept]);
                    }
                    conn.rcv_nxt = conn.rcv_nxt.wrapping_add(accept as u32);
                    ooo_deliver(conn);
                    sack_advance(conn);
                } else if conn.sack_ok && seq_lt(conn.rcv_nxt, seq) {
                    let right = seq.wrapping_add(payload.len() as u32);
                    sack_insert(conn, seq, right);
                    ooo_store(conn, seq, payload);
                }
            }

            // --- FIN processing ---
            if flags & TCP_FIN != 0 && seq.wrapping_add(payload.len() as u32) == conn.rcv_nxt {
                conn.rcv_nxt = conn.rcv_nxt.wrapping_add(1);
                conn.state = TcpState::TimeWait;
                conn.remote_closed = true;
            }

            // Send ACK for data and/or FIN.
            let _ = send_ack_with_sack(conn);
        }

        TcpState::LastAck => {
            if flags & TCP_ACK != 0 {
                conn.last_error = TCP_ERR_NONE; // Normal close.
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

// ---------------------------------------------------------------------------
// Zero window probe — persist timer (RFC 1122 §4.2.2.17)
// ---------------------------------------------------------------------------

/// Periodic persist-timer tick — probes peers that have advertised a zero
/// receive window.
///
/// When a receiver's buffer is full it advertises window=0.  Without the
/// persist timer the sender would stall forever: the receiver's eventual
/// window update is a bare ACK which is not retransmitted, so a single
/// lost ACK causes permanent deadlock.
///
/// The probe sends a 1-byte segment at `snd_nxt` (i.e., the next
/// unsent sequence number).  The peer will either:
/// - Still have a zero window → responds with ACK + window=0.  We back
///   off and try again later.
/// - Have freed buffer space → responds with ACK + window>0.  The
///   `snd_wnd` update above clears `persist_active` and normal sending
///   resumes.
///
/// Called from the same periodic tick as `tick_keepalive`.
#[allow(clippy::arithmetic_side_effects)]
pub fn tick_persist() {
    let now = crate::hrtimer::now_ns();
    let mut conns = CONNECTIONS.lock();

    for idx in 0..MAX_CONNECTIONS {
        let conn = &mut conns[idx];
        if !conn.active || !conn.persist_active {
            continue;
        }
        // Only probe in states where we'd be sending data.
        if conn.state != TcpState::Established && conn.state != TcpState::CloseWait {
            conn.persist_active = false;
            continue;
        }
        // Window opened since we last checked (e.g., ACK processed
        // between ticks).
        if conn.snd_wnd > 0 {
            conn.persist_active = false;
            conn.persist_interval_ns = PERSIST_MIN_NS;
            conn.persist_last_ns = 0;
            continue;
        }

        let elapsed = now.saturating_sub(conn.persist_last_ns);
        if elapsed < conn.persist_interval_ns {
            continue;
        }

        // Time to send a zero-window probe.  The probe is a 1-byte
        // segment at `snd_nxt`.  We don't advance `snd_nxt` because
        // the peer may discard the byte (zero window) and we'd need
        // to resend it.  We use `snd_una` (the last acknowledged seq)
        // so the peer's ACK tells us the current window without
        // requiring it to buffer new data.
        let lp = conn.local_port;
        let ri = conn.remote_ip;
        let rp = conn.remote_port;
        let probe_seq = conn.snd_una.wrapping_sub(1);
        let ack = conn.rcv_nxt;
        let wnd = advertised_window(conn);

        // Exponential backoff for the persist interval.
        conn.persist_interval_ns = conn
            .persist_interval_ns
            .saturating_mul(2)
            .min(PERSIST_MAX_NS);
        conn.persist_last_ns = now;

        let probe_ecn = if conn.ecn_ok { ipv4::ECN_ECT0 } else { 0 };
        let p_ts_ok = conn.ts_ok;
        let p_ts_recent = conn.ts_recent;

        crate::serial_println!(
            "[tcp] Zero-window probe for {}:{} → {}:{} (next in {}ms)",
            lp, rp, ri, rp,
            conn.persist_interval_ns / 1_000_000
        );

        // Drop the lock before sending (send_segment acquires
        // interface locks).
        drop(conns);
        let _ = send_data_with_ts(
            lp, ri, rp, probe_seq, ack, TCP_ACK, wnd, &[], probe_ecn,
            p_ts_ok, p_ts_recent,
        );
        // Dropped lock — restart scan next tick.
        return;
    }
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

            conn.last_error = TCP_ERR_TIMEDOUT; // Keepalive failure.
            conn.active = false;
            conn.state = TcpState::Closed;
            conn.rx_buffer.clear();
            conn.tx_buffer.clear();
            conn.nagle_buf.clear();
            conn.ooo_buf.clear();

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
        let ka_ecn = if conn.ecn_ok { ipv4::ECN_ECT0 } else { 0 };
        let k_ts_ok = conn.ts_ok;
        let k_ts_recent = conn.ts_recent;

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
        let _ = send_data_with_ts(
            local_port, remote_ip, remote_port,
            probe_seq, rcv_nxt, TCP_ACK, our_wnd, &[], ka_ecn,
            k_ts_ok, k_ts_recent,
        );
        return; // Dropped lock; restart scan next tick.
    }
}

// ---------------------------------------------------------------------------
// TIME_WAIT cleanup
// ---------------------------------------------------------------------------

/// TIME_WAIT duration: 2 × MSL = 60 seconds (RFC 793 recommends MSL=2min,
/// but Linux uses 60s total for TIME_WAIT, which is the practical standard).
const TIME_WAIT_DURATION_NS: u64 = 60_000_000_000;

/// FIN_WAIT_2 timeout: 60 seconds (matches Linux `tcp_fin_timeout` default).
///
/// A connection enters FIN_WAIT_2 after we send our FIN and receive the
/// peer's ACK, but the peer hasn't sent its own FIN yet.  If the peer
/// crashes or forgets to close, the connection stalls here indefinitely.
/// This timeout reclaims the slot after 60 seconds of inactivity.
const FIN_WAIT2_TIMEOUT_NS: u64 = 60_000_000_000;

/// CLOSE_WAIT timeout: 5 minutes (300 seconds).
///
/// A connection enters CLOSE_WAIT when the peer sends FIN but the local
/// application hasn't called close() yet.  In a microkernel, the driver
/// or application process may crash without closing connections.  Without
/// a timeout, these slots would leak forever.  5 minutes is generous
/// enough for any well-behaved app, but prevents permanent slot exhaustion.
/// Linux has no equivalent (relies on the app closing), but our kernel must
/// be self-healing since userspace drivers can crash.
const CLOSE_WAIT_TIMEOUT_NS: u64 = 300_000_000_000;

/// SYN_RECEIVED timeout: 30 seconds.
///
/// A server-side connection enters SYN_RECEIVED after receiving a SYN
/// and sending SYN-ACK, but the final ACK from the client never arrived.
/// This happens when the client crashes mid-handshake, or during SYN
/// flood attacks.  30 seconds matches typical TCP implementations.
const SYN_RECEIVED_TIMEOUT_NS: u64 = 30_000_000_000;

/// Clean up connections in terminal TCP states after their timers expire.
///
/// Called from the same periodic tick as `tick_keepalive`.  Scans all
/// connections and reclaims slots for:
///
/// - **TIME_WAIT** (60s): the standard 2×MSL wait before slot reuse.
/// - **FIN_WAIT_2** (60s): we sent FIN and got the ACK, but the peer
///   never sent its FIN.  Likely a crashed peer or leaked socket.
///   Matches Linux `tcp_fin_timeout` default.
/// - **LAST_ACK** (30s): we sent FIN, waiting for the final ACK.  The
///   peer's FIN-ACK may have been lost.
/// - **SYN_RECEIVED** (30s): received SYN and sent SYN-ACK, but the
///   client's ACK never arrived (crashed client or SYN flood).
pub fn tick_time_wait_cleanup() {
    let now = crate::hrtimer::now_ns();
    let mut conns = CONNECTIONS.lock();

    for conn in conns.iter_mut() {
        if !conn.active {
            continue;
        }

        match conn.state {
            TcpState::TimeWait => {
                let elapsed = now.saturating_sub(conn.last_activity_ns);
                if elapsed >= TIME_WAIT_DURATION_NS {
                    crate::serial_println!(
                        "[tcp] TIME_WAIT expired for port {} → {}:{} — reclaiming slot",
                        conn.local_port, conn.remote_ip, conn.remote_port
                    );
                    conn.last_error = TCP_ERR_NONE; // Normal close.
                    conn.active = false;
                    conn.state = TcpState::Closed;
                    conn.rx_buffer.clear();
                    conn.tx_buffer.clear();
                    conn.nagle_buf.clear();
                    conn.ooo_buf.clear();
                }
            }
            TcpState::FinWait1 => {
                // FIN_WAIT_1: we sent FIN but never got the ACK.
                // The FIN retransmit loop in tick_retransmit() handles
                // resending the FIN, but after 60 seconds the connection
                // is clearly dead — reclaim.
                let elapsed = now.saturating_sub(conn.last_activity_ns);
                if elapsed >= FIN_WAIT2_TIMEOUT_NS {
                    crate::serial_println!(
                        "[tcp] FIN_WAIT_1 timeout for port {} → {}:{} — reclaiming",
                        conn.local_port, conn.remote_ip, conn.remote_port
                    );
                    conn.last_error = TCP_ERR_TIMEDOUT;
                    conn.active = false;
                    conn.state = TcpState::Closed;
                    conn.rx_buffer.clear();
                    conn.tx_buffer.clear();
                    conn.nagle_buf.clear();
                    conn.ooo_buf.clear();
                }
            }
            TcpState::FinWait2 => {
                // FIN_WAIT_2: we sent our FIN and got the ACK, but
                // the peer hasn't sent its FIN yet.  If the peer
                // crashed or the application leaked a socket, this
                // connection would linger forever.  Clean up after
                // FIN_WAIT2_TIMEOUT_NS (60s, matching Linux tcp_fin_timeout).
                let elapsed = now.saturating_sub(conn.last_activity_ns);
                if elapsed >= FIN_WAIT2_TIMEOUT_NS {
                    crate::serial_println!(
                        "[tcp] FIN_WAIT_2 timeout for port {} → {}:{} — reclaiming",
                        conn.local_port, conn.remote_ip, conn.remote_port
                    );
                    conn.last_error = TCP_ERR_TIMEDOUT;
                    conn.active = false;
                    conn.state = TcpState::Closed;
                    conn.rx_buffer.clear();
                    conn.tx_buffer.clear();
                    conn.nagle_buf.clear();
                    conn.ooo_buf.clear();
                }
            }
            TcpState::LastAck => {
                // LastAck: we sent FIN, waiting for final ACK.  If idle
                // for more than 30 seconds, the peer likely crashed —
                // reclaim the slot.
                let elapsed = now.saturating_sub(conn.last_activity_ns);
                if elapsed >= 30_000_000_000 {
                    crate::serial_println!(
                        "[tcp] LAST_ACK timeout for port {} → {}:{} — reclaiming",
                        conn.local_port, conn.remote_ip, conn.remote_port
                    );
                    conn.last_error = TCP_ERR_TIMEDOUT;
                    conn.active = false;
                    conn.state = TcpState::Closed;
                    conn.rx_buffer.clear();
                    conn.tx_buffer.clear();
                    conn.nagle_buf.clear();
                    conn.ooo_buf.clear();
                }
            }
            TcpState::SynReceived => {
                // Half-open connection: received SYN and sent SYN-ACK,
                // but the client's final ACK never arrived.  This prevents
                // slot exhaustion from SYN floods or crashed clients.
                let elapsed = now.saturating_sub(conn.last_activity_ns);
                if elapsed >= SYN_RECEIVED_TIMEOUT_NS {
                    crate::serial_println!(
                        "[tcp] SYN_RECEIVED timeout for port {} ← {}:{} — reclaiming",
                        conn.local_port, conn.remote_ip, conn.remote_port
                    );
                    conn.last_error = TCP_ERR_TIMEDOUT;
                    conn.active = false;
                    conn.state = TcpState::Closed;
                    conn.rx_buffer.clear();
                    conn.tx_buffer.clear();
                    conn.nagle_buf.clear();
                    conn.ooo_buf.clear();
                }
            }
            TcpState::SynSent => {
                // Non-blocking connect (connect_start): the initial SYN
                // was sent but no SYN-ACK arrived.  After 30 seconds,
                // give up and reclaim the slot.  This prevents slot
                // exhaustion from abandoned non-blocking connect attempts.
                let elapsed = now.saturating_sub(conn.last_activity_ns);
                if elapsed >= SYN_RECEIVED_TIMEOUT_NS {
                    crate::serial_println!(
                        "[tcp] SYN_SENT timeout for port {} → {}:{} — reclaiming",
                        conn.local_port, conn.remote_ip, conn.remote_port
                    );
                    conn.last_error = TCP_ERR_TIMEDOUT;
                    conn.active = false;
                    conn.state = TcpState::Closed;
                    conn.rx_buffer.clear();
                    conn.tx_buffer.clear();
                    conn.nagle_buf.clear();
                    conn.ooo_buf.clear();
                }
            }
            TcpState::CloseWait => {
                // Peer sent FIN, but the local app hasn't closed yet.
                // If the app crashed or leaked the connection, this slot
                // would be stuck forever.  In a microkernel where driver
                // processes can crash without cleaning up, we need a
                // safety timeout.  5 minutes is generous for any real app.
                let elapsed = now.saturating_sub(conn.last_activity_ns);
                if elapsed >= CLOSE_WAIT_TIMEOUT_NS && conn.tx_buffer.is_empty() {
                    crate::serial_println!(
                        "[tcp] CLOSE_WAIT timeout for port {} → {}:{} — reclaiming (app likely crashed)",
                        conn.local_port, conn.remote_ip, conn.remote_port
                    );
                    conn.last_error = TCP_ERR_TIMEDOUT;
                    conn.active = false;
                    conn.state = TcpState::Closed;
                    conn.rx_buffer.clear();
                    conn.tx_buffer.clear();
                    conn.nagle_buf.clear();
                    conn.ooo_buf.clear();
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// RTO-based retransmission (RFC 6298)
// ---------------------------------------------------------------------------

/// Periodic retransmit tick — called from the network poll loop.
///
/// Scans all established connections with pending unacknowledged data in
/// the retransmit buffer.  If the RTO timer has expired, retransmits the
/// first unacknowledged segment and applies exponential backoff (doubles
/// RTO per RFC 6298 §5.5, capped at `RTO_MAX_NS`).
///
/// This handles the case where a segment is lost and no duplicate ACKs
/// arrive (e.g., the last segment in a burst is lost — there's no
/// subsequent data to trigger dup ACKs).
pub fn tick_retransmit() {
    let now = crate::hrtimer::now_ns();
    let mut conns = CONNECTIONS.lock();

    // --- SYN retransmission for non-blocking connects ---
    // Scan SYN_SENT connections and retransmit the SYN with exponential
    // backoff.  This is separate from data retransmit because SYN_SENT
    // connections have no tx_buffer — the SYN is a control segment.
    for idx in 0..MAX_CONNECTIONS {
        let conn = &mut conns[idx];
        if !conn.active || conn.state != TcpState::SynSent {
            continue;
        }
        // Use last_activity_ns as the SYN send timestamp and rto_ns
        // for the retransmit interval.
        let elapsed = now.saturating_sub(conn.last_activity_ns);
        if elapsed < conn.rto_ns {
            continue;
        }

        // Check retransmit limit — abort if exceeded.
        conn.retransmit_count = conn.retransmit_count.saturating_add(1);
        if conn.retransmit_count > MAX_RETRANSMITS {
            crate::serial_println!(
                "[tcp] SYN retransmit exhausted ({} retries) for port {} → {}:{}",
                conn.retransmit_count, conn.local_port, conn.remote_ip, conn.remote_port
            );
            conn.last_error = TCP_ERR_TIMEDOUT;
            conn.active = false;
            conn.tx_buffer.clear();
            conn.nagle_buf.clear();
            conn.ooo_buf.clear();
            continue;
        }

        // Retransmit the SYN.
        let lp = conn.local_port;
        let ri = conn.remote_ip;
        let rp = conn.remote_port;
        let isn = conn.snd_iss;
        let wscale = conn.rcv_wnd_scale;

        // Exponential backoff (RFC 6298 §5.5).
        conn.rto_ns = conn.rto_ns.saturating_mul(2).min(RTO_MAX_NS);
        conn.last_activity_ns = now;

        crate::serial_println!(
            "[tcp] SYN retransmit #{} to {}:{} (port {}, rto={}ms)",
            conn.retransmit_count, ri, rp, lp, conn.rto_ns / 1_000_000
        );

        // Drop lock before sending.
        drop(conns);
        let _ = send_syn_segment(
            lp, ri, rp,
            isn, 0, TCP_SYN | TCP_ECE | TCP_CWR, DEFAULT_WINDOW, wscale,
            tcp_now_ms(), 0,
        );
        // Lock dropped — can't continue scan; next tick handles rest.
        return;
    }

    // --- Data + FIN retransmission for FinWait1/LastAck ---
    //
    // If the connection has unacked data in tx_buffer, retransmit the
    // data first.  If no data remains (all ACKed), retransmit the FIN.
    // Per RFC 793, both data and FIN can be outstanding simultaneously
    // in FinWait1 (data sent before close() + the FIN).
    for idx in 0..MAX_CONNECTIONS {
        let conn = &mut conns[idx];
        if !conn.active {
            continue;
        }
        if conn.state != TcpState::FinWait1 && conn.state != TcpState::LastAck {
            continue;
        }
        let elapsed = now.saturating_sub(
            if !conn.tx_buffer.is_empty() && conn.tx_last_send_ns > 0 {
                conn.tx_last_send_ns
            } else {
                conn.last_activity_ns
            }
        );
        if elapsed < conn.rto_ns {
            continue;
        }

        // Check retransmit limit for FIN/data retransmission.
        conn.retransmit_count = conn.retransmit_count.saturating_add(1);
        if conn.retransmit_count > MAX_RETRANSMITS {
            crate::serial_println!(
                "[tcp] {:?} retransmit exhausted ({} retries) for port {} — aborting",
                conn.state, conn.retransmit_count, conn.local_port
            );
            conn.last_error = TCP_ERR_TIMEDOUT;
            conn.active = false;
            conn.tx_buffer.clear();
            conn.nagle_buf.clear();
            conn.ooo_buf.clear();
            continue;
        }

        // If there's unacked data, retransmit it first (data before FIN
        // in the byte stream).
        if !conn.tx_buffer.is_empty() {
            if let Some((lp, ri, rp, seq, ack, wnd, data, len, r_ts_ok, r_ts_recent)) =
                retransmit_from_buffer(conn)
            {
                conn.rto_ns = conn.rto_ns.saturating_mul(2).min(RTO_MAX_NS);
                conn.tx_last_send_ns = now;
                on_loss_congestion(conn);
                let rto_ecn = if conn.ecn_ok { ipv4::ECN_ECT0 } else { 0 };

                crate::serial_println!(
                    "[tcp] RTO data retransmit ({:?}): {} bytes from seq {} (port {}, rto={}ms)",
                    conn.state, len, seq, lp, conn.rto_ns / 1_000_000
                );

                drop(conns);
                let _ = send_data_with_ts(
                    lp, ri, rp, seq, ack, TCP_ACK | TCP_PSH, wnd,
                    &data[..len], rto_ecn,
                    r_ts_ok, r_ts_recent,
                );
                return;
            }
        }

        // No data in tx_buffer — retransmit the FIN.
        let lp = conn.local_port;
        let ri = conn.remote_ip;
        let rp = conn.remote_port;
        // FIN occupies one sequence number; snd_nxt is already past it,
        // so we retransmit at snd_nxt-1 (the FIN's seq).
        let fin_seq = conn.snd_nxt.wrapping_sub(1);
        let ack_nr = conn.rcv_nxt;

        conn.rto_ns = conn.rto_ns.saturating_mul(2).min(RTO_MAX_NS);
        conn.last_activity_ns = now;

        crate::serial_println!(
            "[tcp] FIN retransmit ({:?}) to {}:{} (port {}, rto={}ms)",
            conn.state, ri, rp, lp, conn.rto_ns / 1_000_000
        );

        drop(conns);
        let _ = send_segment(
            lp, ri, rp, fin_seq, ack_nr, TCP_FIN | TCP_ACK, &[],
        );
        return;
    }

    // --- Data retransmission for Established/CloseWait ---
    for idx in 0..MAX_CONNECTIONS {
        let conn = &mut conns[idx];
        if !conn.active {
            continue;
        }
        if conn.state != TcpState::Established && conn.state != TcpState::CloseWait {
            continue;
        }
        if conn.tx_buffer.is_empty() || conn.tx_last_send_ns == 0 {
            continue;
        }

        let elapsed = now.saturating_sub(conn.tx_last_send_ns);
        if elapsed < conn.rto_ns {
            continue;
        }

        // RTO expired — check retransmit limit before resending.
        conn.retransmit_count = conn.retransmit_count.saturating_add(1);
        if conn.retransmit_count > MAX_RETRANSMITS {
            crate::serial_println!(
                "[tcp] Retransmit exhausted ({} retries) for port {} → {}:{} — aborting",
                conn.retransmit_count, conn.local_port, conn.remote_ip, conn.remote_port
            );
            conn.last_error = TCP_ERR_TIMEDOUT;
            conn.active = false;
            conn.tx_buffer.clear();
            conn.nagle_buf.clear();
            conn.ooo_buf.clear();
            continue;
        }

        if let Some((lp, ri, rp, seq, ack, wnd, data, len, r_ts_ok, r_ts_recent)) = retransmit_from_buffer(conn) {
            // Exponential backoff (RFC 6298 §5.5).
            conn.rto_ns = conn.rto_ns.saturating_mul(2).min(RTO_MAX_NS);
            // Reset send timestamp for the next RTO measurement.
            conn.tx_last_send_ns = now;
            // Enter slow start after timeout (RFC 5681 §3.1).
            on_loss_congestion(conn);
            let rto_ecn = if conn.ecn_ok { ipv4::ECN_ECT0 } else { 0 };

            crate::serial_println!(
                "[tcp] RTO retransmit #{}: {} bytes from seq {} (port {}, rto={}ms)",
                conn.retransmit_count, len, seq, lp, conn.rto_ns / 1_000_000
            );

            // Drop lock before sending.
            drop(conns);
            let _ = send_data_with_ts(
                lp, ri, rp, seq, ack, TCP_ACK | TCP_PSH, wnd,
                &data[..len], rto_ecn,
                r_ts_ok, r_ts_recent,
            );
            // We dropped the lock, so we can't continue the scan.
            // The next tick will handle remaining connections.
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// ICMP error handling
// ---------------------------------------------------------------------------

/// Notify the TCP stack about an ICMP error for a connection.
///
/// Called by the ICMP handler when it receives a Destination Unreachable
/// or similar error.  The original IP header + first 8 bytes of the TCP
/// segment are in `orig_hdr`, which gives us the ports to identify the
/// connection.
///
/// `icmp_type` and `icmp_code` identify the ICMP error (e.g., type 3
/// code 1 = host unreachable).
///
/// Per RFC 5461, a "soft error" (e.g., host unreachable) on an established
/// connection should NOT abort it — only SYN_SENT connections are aborted
/// immediately by ICMP errors.  This prevents transient routing issues
/// from tearing down long-lived connections.
/// Handle an ICMP error notification for a TCP connection.
///
/// Called by the ICMP handler when an ICMP error references an original
/// TCP segment (identified by the first 8 bytes of the TCP header
/// embedded in the ICMP payload).
///
/// # Path MTU Discovery (RFC 1191)
///
/// When `next_hop_mtu` is `Some(mtu)`, the ICMP was "Fragmentation
/// Needed" (type 3, code 4) carrying the next-hop MTU.  We reduce the
/// connection's `peer_mss` to `mtu - 40` (IP + TCP headers) so future
/// segments fit without fragmentation.  This is the sender-side PMTUD
/// adjustment.
pub fn icmp_error(
    orig_src_ip: Ipv4Addr,  // IPv4-specific (called from ICMP handler)
    orig_dst_ip: Ipv4Addr,  // IPv4-specific (called from ICMP handler)
    orig_tcp_hdr: &[u8],
    icmp_type: u8,
    icmp_code: u8,
    next_hop_mtu: Option<u16>,
) {
    // Need at least 4 bytes for src_port + dst_port.
    if orig_tcp_hdr.len() < 4 {
        return;
    }

    let src_port = u16::from_be_bytes([orig_tcp_hdr[0], orig_tcp_hdr[1]]);
    let dst_port = u16::from_be_bytes([orig_tcp_hdr[2], orig_tcp_hdr[3]]);

    let mut conns = CONNECTIONS.lock();

    for conn in conns.iter_mut() {
        if !conn.active {
            continue;
        }

        // Match: the *original* packet was from us (local_port = src_port)
        // to the remote (remote_ip = dst_ip, remote_port = dst_port).
        // Wrap in IpAddr::V4 since icmp_error is IPv4-specific.
        if conn.local_port != src_port
            || conn.remote_port != dst_port
            || conn.remote_ip != IpAddr::V4(orig_dst_ip)
        {
            continue;
        }

        match conn.state {
            TcpState::SynSent => {
                // Hard error: abort the connection attempt immediately.
                // The 3-way handshake cannot complete if the destination
                // is unreachable.
                crate::serial_println!(
                    "[tcp] ICMP error (type={} code={}) for SYN_SENT {}:{} → {}:{} — aborting",
                    icmp_type, icmp_code,
                    orig_src_ip, src_port,
                    orig_dst_ip, dst_port
                );
                conn.last_error = TCP_ERR_REFUSED; // ICMP unreachable on connect.
                conn.active = false;
                conn.state = TcpState::Closed;
                conn.rx_buffer.clear();
                conn.tx_buffer.clear();
                conn.nagle_buf.clear();
                conn.ooo_buf.clear();
            }
            TcpState::SynReceived => {
                // Also abort half-open connections from the server side.
                crate::serial_println!(
                    "[tcp] ICMP error (type={} code={}) for SYN_RECEIVED {}:{} → {}:{} — aborting",
                    icmp_type, icmp_code,
                    orig_src_ip, src_port,
                    orig_dst_ip, dst_port
                );
                conn.last_error = TCP_ERR_REFUSED; // ICMP unreachable on half-open.
                conn.active = false;
                conn.state = TcpState::Closed;
                conn.rx_buffer.clear();
                conn.tx_buffer.clear();
                conn.nagle_buf.clear();
                conn.ooo_buf.clear();
            }
            _ => {
                // PMTUD (RFC 1191): if ICMP "Fragmentation Needed" carries
                // a next-hop MTU, reduce this connection's MSS to fit.
                // This avoids IP fragmentation on the path.
                if let Some(mtu) = next_hop_mtu {
                    // MSS = MTU - 20 (IP header) - 20 (TCP header).
                    let new_mss = mtu.saturating_sub(40);
                    if new_mss > 0 && (conn.peer_mss == 0 || new_mss < conn.peer_mss) {
                        crate::serial_println!(
                            "[tcp] PMTUD: reducing MSS to {} (MTU={}) for {:?} {}:{} → {}:{}",
                            new_mss, mtu, conn.state,
                            orig_src_ip, src_port,
                            orig_dst_ip, dst_port
                        );
                        conn.peer_mss = new_mss;
                    }
                }

                // RFC 5461: soft error on established connections.
                // Log but do not abort — the route may recover.
                crate::serial_println!(
                    "[tcp] ICMP soft error (type={} code={}) for {:?} {}:{} → {}:{} — ignored",
                    icmp_type, icmp_code,
                    conn.state,
                    orig_src_ip, src_port,
                    orig_dst_ip, dst_port
                );
            }
        }

        return; // Only one connection per (src_port, dst_ip, dst_port).
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
    test_sack_blocks()?;
    test_seq_lt()?;
    test_ipv6_checksum()?;
    test_dual_stack_ip_addr()?;
    test_namespace_isolation()?;

    crate::serial_println!("[tcp] TCP self-test PASSED (9 tests)");
    Ok(())
}

/// Test 1: bind and close a listener.
fn test_bind_close() -> KernelResult<()> {
    let handle = bind(crate::netns::ROOT_NS, 9999)?;

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
    let handle = bind(crate::netns::ROOT_NS, 8888)?;

    // Try to bind the same port again — should fail.
    match bind(crate::netns::ROOT_NS, 8888) {
        Err(KernelError::AlreadyExists) => {}
        other => {
            // Best-effort cleanup — we're about to return an error anyway.
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
    let handle2 = bind(crate::netns::ROOT_NS, 8888)?;
    close_listener(handle2)?;

    crate::serial_println!("[tcp]   Duplicate bind rejected: OK");
    Ok(())
}

/// Test 3: try_accept on empty backlog returns WouldBlock.
fn test_try_accept_empty() -> KernelResult<()> {
    let handle = bind(crate::netns::ROOT_NS, 7777)?;

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

    // SACK-Permitted: kind=4, len=2.
    let sack_perm = [4, 2];
    let opts = parse_tcp_options(&sack_perm);
    if !opts.sack_permitted {
        crate::serial_println!("[tcp]   FAIL: SACK-Permitted not parsed");
        return Err(KernelError::InternalError);
    }

    // Full Linux-style SYN options: MSS + NOP + WScale + NOP + NOP + SACK-Perm.
    let full_syn = [2, 4, 0x05, 0xB4, 1, 3, 3, 7, 1, 1, 4, 2];
    let opts = parse_tcp_options(&full_syn);
    if opts.mss != 1460 || opts.wscale != Some(7) || !opts.sack_permitted {
        crate::serial_println!(
            "[tcp]   FAIL: full SYN parse: mss={} wscale={:?} sack={}",
            opts.mss, opts.wscale, opts.sack_permitted
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[tcp]   TCP option parsing: OK");
    Ok(())
}

/// Test 5: SACK block insertion and merging.
fn test_sack_blocks() -> KernelResult<()> {
    let mut conn = TcpConnection::empty();
    conn.sack_ok = true;

    // Insert a single block: [100, 200).
    sack_insert(&mut conn, 100, 200);
    if conn.sack_block_count != 1 || conn.sack_blocks[0] != (100, 200) {
        crate::serial_println!(
            "[tcp]   FAIL: single block: count={} block={:?}",
            conn.sack_block_count, conn.sack_blocks[0]
        );
        return Err(KernelError::InternalError);
    }

    // Insert adjacent block [200, 300) — should merge into [100, 300).
    sack_insert(&mut conn, 200, 300);
    if conn.sack_block_count != 1 || conn.sack_blocks[0] != (100, 300) {
        crate::serial_println!(
            "[tcp]   FAIL: merge adjacent: count={} block={:?}",
            conn.sack_block_count, conn.sack_blocks[0]
        );
        return Err(KernelError::InternalError);
    }

    // Insert non-contiguous block [500, 600).
    sack_insert(&mut conn, 500, 600);
    if conn.sack_block_count != 2 {
        crate::serial_println!(
            "[tcp]   FAIL: non-contiguous: count={}",
            conn.sack_block_count
        );
        return Err(KernelError::InternalError);
    }

    // Insert overlapping block [250, 550) — should merge into [100, 600).
    sack_insert(&mut conn, 250, 550);
    if conn.sack_block_count != 1 || conn.sack_blocks[0] != (100, 600) {
        crate::serial_println!(
            "[tcp]   FAIL: merge overlapping: count={} block={:?}",
            conn.sack_block_count, conn.sack_blocks[0]
        );
        return Err(KernelError::InternalError);
    }

    // Test sack_advance: set rcv_nxt to 300, block [100, 600) should
    // be trimmed to [300, 600).
    conn.rcv_nxt = 300;
    sack_advance(&mut conn);
    if conn.sack_block_count != 1 || conn.sack_blocks[0] != (300, 600) {
        crate::serial_println!(
            "[tcp]   FAIL: advance trim: count={} block={:?}",
            conn.sack_block_count, conn.sack_blocks[0]
        );
        return Err(KernelError::InternalError);
    }

    // Set rcv_nxt past the block — should remove it entirely.
    conn.rcv_nxt = 700;
    sack_advance(&mut conn);
    if conn.sack_block_count != 0 {
        crate::serial_println!(
            "[tcp]   FAIL: advance remove: count={}",
            conn.sack_block_count
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[tcp]   SACK block management: OK");
    Ok(())
}

/// Test 6: seq_lt() modular 32-bit sequence number comparison.
fn test_seq_lt() -> KernelResult<()> {
    // Basic ordering.
    if !seq_lt(0, 1) {
        crate::serial_println!("[tcp]   FAIL: seq_lt(0, 1) expected true");
        return Err(KernelError::InternalError);
    }
    if seq_lt(1, 0) {
        crate::serial_println!("[tcp]   FAIL: seq_lt(1, 0) expected false");
        return Err(KernelError::InternalError);
    }
    // Equal values: a < b is false when a == b.
    if seq_lt(100, 100) {
        crate::serial_println!("[tcp]   FAIL: seq_lt(100, 100) expected false");
        return Err(KernelError::InternalError);
    }

    // Wrap-around: MAX is before 0 in modular arithmetic (distance 1).
    if !seq_lt(u32::MAX, 0) {
        crate::serial_println!("[tcp]   FAIL: seq_lt(MAX, 0) expected true");
        return Err(KernelError::InternalError);
    }
    if seq_lt(0, u32::MAX) {
        crate::serial_println!("[tcp]   FAIL: seq_lt(0, MAX) expected false");
        return Err(KernelError::InternalError);
    }

    // Wrap-around near the midpoint: 2^31-1 apart is the maximum valid
    // "less than" distance.
    let a = 1_000_000_000u32;
    let b = a.wrapping_add(0x7FFF_FFFF); // a + 2^31 - 1
    if !seq_lt(a, b) {
        crate::serial_println!("[tcp]   FAIL: seq_lt(a, a+2^31-1) expected true");
        return Err(KernelError::InternalError);
    }

    // Exactly 2^31 apart: (a - b) as i32 == i32::MIN, which is < 0,
    // so seq_lt returns true (by convention this edge case goes to "less").
    let c = a.wrapping_add(0x8000_0000);
    if !seq_lt(a, c) {
        crate::serial_println!("[tcp]   FAIL: seq_lt(a, a+2^31) expected true");
        return Err(KernelError::InternalError);
    }

    // Just past halfway: 2^31 + 1 apart means b is "before" a.
    let d = a.wrapping_add(0x8000_0001);
    if seq_lt(a, d) {
        crate::serial_println!("[tcp]   FAIL: seq_lt(a, a+2^31+1) expected false");
        return Err(KernelError::InternalError);
    }

    // OOO guard scenario: seq behind ooo_base near wrap boundary.
    let ooo_base: u32 = 10;
    let stale_seq: u32 = u32::MAX.wrapping_sub(5); // 0xFFFF_FFFA
    // stale_seq is "before" ooo_base in sequence space.
    if !seq_lt(stale_seq, ooo_base) {
        crate::serial_println!("[tcp]   FAIL: stale segment not detected as behind ooo_base");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[tcp]   seq_lt modular comparison: OK");
    Ok(())
}

/// Test 7: IPv6 TCP checksum computation.
///
/// Validates the TCP checksum with IPv6 pseudo-header by computing a
/// checksum over a known segment and verifying it with the IPv6
/// transport checksum verifier.
fn test_ipv6_checksum() -> KernelResult<()> {
    let src = Ipv6Addr([
        0xfe, 0x80, 0, 0, 0, 0, 0, 0,
        0x02, 0x00, 0x00, 0xff, 0xfe, 0x00, 0x00, 0x01,
    ]);
    let dst = Ipv6Addr([
        0xfe, 0x80, 0, 0, 0, 0, 0, 0,
        0x02, 0x00, 0x00, 0xff, 0xfe, 0x00, 0x00, 0x02,
    ]);

    // Build a minimal SYN segment using our builder with IPv6 addresses.
    let seg = build_segment(
        12345, 80,
        1000, 0,
        TCP_SYN, 65535,
        &[],
        IpAddr::V6(src), IpAddr::V6(dst),
    );

    // The checksum should already be embedded in the segment.
    // Verify it using the IPv6 transport checksum verifier.
    if !ipv6::verify_transport_checksum(&src, &dst, PROTO_TCP, &seg) {
        crate::serial_println!("[tcp]   FAIL: IPv6 TCP checksum verification failed");
        return Err(KernelError::InternalError);
    }

    // Corrupt one byte and verify that the checksum now fails.
    let mut corrupted = seg.clone();
    corrupted[0] ^= 0xFF;
    if ipv6::verify_transport_checksum(&src, &dst, PROTO_TCP, &corrupted) {
        crate::serial_println!("[tcp]   FAIL: corrupted segment passed IPv6 checksum");
        return Err(KernelError::InternalError);
    }

    // Also verify that tcp_checksum_ip dispatches correctly for both families.
    let v4_src = Ipv4Addr::new(10, 0, 0, 1);
    let v4_dst = Ipv4Addr::new(10, 0, 0, 2);
    let v4_seg = build_segment(
        12345, 80, 1000, 0, TCP_SYN, 65535, &[],
        IpAddr::V4(v4_src), IpAddr::V4(v4_dst),
    );
    if !ipv4::verify_transport_checksum(v4_src, v4_dst, PROTO_TCP, &v4_seg) {
        crate::serial_println!("[tcp]   FAIL: IPv4 TCP checksum via dual-stack builder failed");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[tcp]   IPv6 TCP checksum: OK");
    Ok(())
}

/// Test 8: dual-stack IpAddr in connection matching.
///
/// Validates that IPv4 and IPv6 connections with the same port numbers
/// are distinguished by address family (IpAddr::V4 != IpAddr::V6).
fn test_dual_stack_ip_addr() -> KernelResult<()> {
    let v4 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
    let v6 = IpAddr::V6(Ipv6Addr([
        0xfe, 0x80, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 1,
    ]));

    // Different address families must not be equal.
    if v4 == v6 {
        crate::serial_println!("[tcp]   FAIL: IPv4 == IPv6 with same port would collide");
        return Err(KernelError::InternalError);
    }

    // Same family, different addresses.
    let v4b = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
    if v4 == v4b {
        crate::serial_println!("[tcp]   FAIL: different IPv4 addresses compared equal");
        return Err(KernelError::InternalError);
    }

    // Same address roundtrips through IpAddr.
    let orig = Ipv4Addr::new(192, 168, 1, 1);
    let wrapped: IpAddr = orig.into();
    if wrapped.as_v4() != Some(orig) {
        crate::serial_println!("[tcp]   FAIL: IpAddr::V4 roundtrip failed");
        return Err(KernelError::InternalError);
    }
    if wrapped.as_v6().is_some() {
        crate::serial_println!("[tcp]   FAIL: V4 address reported as V6");
        return Err(KernelError::InternalError);
    }

    // Display format.
    let v4_display = alloc::format!("{}", IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)));
    if v4_display != "1.2.3.4" {
        crate::serial_println!("[tcp]   FAIL: IpAddr::V4 display = '{}'", v4_display);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[tcp]   Dual-stack IpAddr: OK");
    Ok(())
}

/// Test 9: namespace isolation — same port can be bound in different namespaces.
fn test_namespace_isolation() -> KernelResult<()> {
    let ns0 = crate::netns::ROOT_NS;
    let ns1: NetNsId = 42; // Fake non-root namespace (doesn't need to exist in netns table).

    // Bind port 6666 in namespace 0.
    let h0 = bind(ns0, 6666)?;

    // Same port in a different namespace should succeed.
    let h1 = bind(ns1, 6666)?;

    // Duplicate in the same namespace should still be rejected.
    match bind(ns0, 6666) {
        Err(KernelError::AlreadyExists) => {}
        other => {
            close_listener(h0).ok();
            close_listener(h1).ok();
            crate::serial_println!(
                "[tcp]   FAIL: duplicate bind in same NS returned {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    close_listener(h0)?;
    close_listener(h1)?;

    crate::serial_println!("[tcp]   Namespace isolation: OK");
    Ok(())
}
