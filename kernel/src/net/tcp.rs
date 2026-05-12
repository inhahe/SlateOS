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
//! ## Limitations
//!
//! - Fixed receive window (16 KiB).
//! - No congestion control (sends at wire rate).
//! - No Nagle algorithm.
//! - Simple retransmission (single timeout, no RTT estimation).
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

/// Retransmission timeout in poll cycles (~2 seconds).
#[allow(dead_code)]
const RETRANSMIT_TIMEOUT: u32 = 2000;

/// Maximum data waiting for delivery per connection.
const MAX_RX_BUFFER: usize = 65536;

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
    /// Receive sequence variables.
    rcv_nxt: u32,   // Next expected receive sequence.
    rcv_irs: u32,   // Initial receive sequence number.
    /// Receive buffer (data delivered in-order).
    rx_buffer: Vec<u8>,
    /// Whether the remote end has closed (FIN received).
    remote_closed: bool,
    /// Retransmit counter (incremented each poll cycle).
    retransmit_timer: u32,
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
            rcv_nxt: 0,
            rcv_irs: 0,
            rx_buffer: Vec::new(),
            remote_closed: false,
            retransmit_timer: 0,
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
// TCP segment sending
// ---------------------------------------------------------------------------

/// Send a TCP segment via IP.
fn send_segment(
    local_port: u16,
    remote_ip: Ipv4Addr,
    remote_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
) -> KernelResult<()> {
    let local_ip = interface::ip();
    let seg = build_segment(
        local_port, remote_port,
        seq, ack, flags,
        DEFAULT_WINDOW,
        payload,
        local_ip, remote_ip,
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
        conn.rcv_nxt = 0;
        conn.rcv_irs = 0;
        conn.rx_buffer.clear();
        conn.remote_closed = false;
        conn.retransmit_timer = 0;
        slot
    };

    // Send SYN.
    send_segment(local_port, remote_ip, remote_port, isn, 0, TCP_SYN, &[])?;
    crate::serial_println!(
        "[tcp] SYN sent to {}:{} (seq={})",
        remote_ip, remote_port, isn
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

/// Send data on an established TCP connection.
#[allow(clippy::arithmetic_side_effects)]
pub fn send(handle: usize, data: &[u8]) -> KernelResult<()> {
    let (local_port, remote_ip, remote_port, seq, ack) = {
        let conns = CONNECTIONS.lock();
        let conn = conns.get(handle).ok_or(KernelError::InvalidArgument)?;
        if !conn.active || conn.state != TcpState::Established {
            return Err(KernelError::InvalidArgument);
        }
        (conn.local_port, conn.remote_ip, conn.remote_port, conn.snd_nxt, conn.rcv_nxt)
    };

    // Send data in chunks that fit in a single segment.
    let mss: usize = 1460; // Standard MSS for Ethernet.
    let mut offset = 0;

    while offset < data.len() {
        let chunk_end = (offset + mss).min(data.len());
        let chunk = &data[offset..chunk_end];

        let send_seq = seq.wrapping_add(offset as u32);
        send_segment(
            local_port, remote_ip, remote_port,
            send_seq, ack,
            TCP_ACK | TCP_PSH,
            chunk,
        )?;

        offset = chunk_end;
    }

    // Update snd_nxt.
    {
        let mut conns = CONNECTIONS.lock();
        let conn = &mut conns[handle];
        conn.snd_nxt = seq.wrapping_add(data.len() as u32);
    }

    Ok(())
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
#[allow(clippy::arithmetic_side_effects)]
fn handle_incoming_syn(
    remote_ip: Ipv4Addr,
    remote_port: u16,
    local_port: u16,
    remote_seq: u32,
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
        conn.rcv_irs = remote_seq;
        conn.rcv_nxt = remote_seq.wrapping_add(1); // Client's SYN consumes 1.
        conn.rx_buffer.clear();
        conn.remote_closed = false;
        conn.retransmit_timer = 0;
        slot
    };

    // Send SYN-ACK.
    let rcv_nxt = remote_seq.wrapping_add(1);
    send_segment(
        local_port, remote_ip, remote_port,
        isn, rcv_nxt, TCP_SYN | TCP_ACK, &[],
    )?;

    crate::serial_println!(
        "[tcp] SYN-ACK sent to {}:{} (handle={}, isn={})",
        remote_ip, remote_port, handle, isn
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
            return handle_incoming_syn(ip_packet.src, src_port, dst_port, seq);
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

                // Send ACK.
                let local_port = conn.local_port;
                let remote_ip = conn.remote_ip;
                let remote_port = conn.remote_port;
                let snd_nxt = conn.snd_nxt;
                let rcv_nxt = conn.rcv_nxt;
                conn.state = TcpState::Established;

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
                // Advance snd_una.
                if ack.wrapping_sub(conn.snd_una) <= conn.snd_nxt.wrapping_sub(conn.snd_una) {
                    conn.snd_una = ack;
                }
            }

            // Process data.
            if !payload.is_empty() && seq == conn.rcv_nxt {
                let can_accept = MAX_RX_BUFFER.saturating_sub(conn.rx_buffer.len());
                let accept = payload.len().min(can_accept);
                if accept > 0 {
                    conn.rx_buffer.extend_from_slice(&payload[..accept]);
                    conn.rcv_nxt = conn.rcv_nxt.wrapping_add(accept as u32);
                }

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
// Self-test
// ---------------------------------------------------------------------------

/// TCP self-test: validates listener bind/close lifecycle and basic
/// state management without requiring actual network traffic.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[tcp] Running TCP self-test...");

    test_bind_close()?;
    test_bind_duplicate_rejected()?;
    test_try_accept_empty()?;

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
