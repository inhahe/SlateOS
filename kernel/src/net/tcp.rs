//! TCP (Transmission Control Protocol) implementation.
//!
//! A minimal TCP client supporting connection establishment (3-way
//! handshake), reliable data transfer with sequence numbers, and
//! orderly connection teardown.
//!
//! ## State machine
//!
//! ```text
//! CLOSED → SYN_SENT → ESTABLISHED → FIN_WAIT_1 → FIN_WAIT_2 → TIME_WAIT → CLOSED
//!                                  → CLOSE_WAIT → LAST_ACK → CLOSED
//! ```
//!
//! ## Limitations
//!
//! - Client-side only (no listening sockets).
//! - Fixed receive window (16 KiB).
//! - No congestion control (sends at wire rate).
//! - No Nagle algorithm.
//! - Simple retransmission (single timeout, no RTT estimation).
//! - Maximum 8 concurrent connections.

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
const MAX_CONNECTIONS: usize = 8;

/// Default receive window size (16 KiB).
const DEFAULT_WINDOW: u16 = 16384;

/// Retransmission timeout in poll cycles (~2 seconds).
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
    SynSent,
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
// TCP segment processing (called from IPv4 layer)
// ---------------------------------------------------------------------------

/// Process an incoming TCP segment.
#[allow(clippy::arithmetic_side_effects)]
pub fn process_tcp(ip_packet: &Ipv4Packet<'_>) -> KernelResult<()> {
    let data = ip_packet.payload;
    if data.len() < TCP_HEADER_SIZE {
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
        // No matching connection.  Send RST if this isn't a RST.
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
