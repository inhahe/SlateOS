//! UDP (User Datagram Protocol) implementation.
//!
//! Provides connectionless, unreliable datagram transport (RFC 768).
//! Used by DHCP, DNS, and other protocols that don't need TCP's
//! reliability guarantees.
//!
//! ## Header format (8 bytes)
//!
//! ```text
//!  0      7 8     15 16    23 24    31
//! +--------+--------+--------+--------+
//! |     Source       |   Destination   |
//! |      Port        |      Port       |
//! +--------+--------+--------+--------+
//! |     Length        |    Checksum     |
//! +--------+--------+--------+--------+
//! |          data octets ...           |
//! +-----------------------------------+
//! ```

use alloc::vec::Vec;
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

use super::interface::Ipv4Addr;
use super::ipv4::{self, Ipv4Packet, PROTO_UDP};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// UDP header size.
const UDP_HEADER_SIZE: usize = 8;

/// Maximum number of bound UDP sockets.
///
/// Supports multiple concurrent UDP services (DNS, DHCP, game
/// clients, streaming, etc.).
const MAX_SOCKETS: usize = 32;

/// Maximum queued datagrams per socket.
const MAX_QUEUED: usize = 64;

// ---------------------------------------------------------------------------
// UDP socket
// ---------------------------------------------------------------------------

/// A received UDP datagram.
#[derive(Debug, Clone)]
pub struct Datagram {
    /// Source IP address.
    pub src_ip: Ipv4Addr,
    /// Source port.
    pub src_port: u16,
    /// Payload data.
    pub data: Vec<u8>,
}

/// A bound UDP socket.
struct UdpSocket {
    /// Local port this socket is bound to.
    port: u16,
    /// Whether this slot is in use.
    active: bool,
    /// Received datagrams waiting to be read.
    rx_queue: Vec<Datagram>,
}

impl UdpSocket {
    const fn empty() -> Self {
        Self {
            port: 0,
            active: false,
            rx_queue: Vec::new(),
        }
    }
}

/// Global UDP socket table.
static SOCKETS: Mutex<[UdpSocket; MAX_SOCKETS]> = Mutex::new(
    // Can't use array init with Vec, so spell it out.
    {
        const EMPTY: UdpSocket = UdpSocket::empty();
        [EMPTY; MAX_SOCKETS]
    }
);

// ---------------------------------------------------------------------------
// Socket API
// ---------------------------------------------------------------------------

/// Bind a UDP socket to a local port.
///
/// Returns a socket index (handle) on success.
pub fn bind(port: u16) -> KernelResult<usize> {
    let mut sockets = SOCKETS.lock();

    // Check for duplicate binding.
    for sock in sockets.iter() {
        if sock.active && sock.port == port {
            return Err(KernelError::AlreadyExists);
        }
    }

    // Find a free slot.
    for (i, sock) in sockets.iter_mut().enumerate() {
        if !sock.active {
            sock.active = true;
            sock.port = port;
            sock.rx_queue.clear();
            return Ok(i);
        }
    }

    // All socket slots are in use.
    Err(KernelError::OutOfMemory)
}

/// Close a UDP socket.
pub fn close(handle: usize) {
    let mut sockets = SOCKETS.lock();
    if let Some(sock) = sockets.get_mut(handle) {
        sock.active = false;
        sock.port = 0;
        sock.rx_queue.clear();
    }
}

/// Receive a datagram from a bound socket.
///
/// Returns `None` if no datagrams are queued.
pub fn recv(handle: usize) -> Option<Datagram> {
    let mut sockets = SOCKETS.lock();
    let sock = sockets.get_mut(handle)?;
    if !sock.active || sock.rx_queue.is_empty() {
        return None;
    }
    // Remove from the front (FIFO order).
    Some(sock.rx_queue.remove(0))
}

/// Send a UDP datagram.
#[allow(clippy::arithmetic_side_effects)]
pub fn send(src_port: u16, dst_ip: Ipv4Addr, dst_port: u16, data: &[u8]) -> KernelResult<()> {
    let src_ip = super::interface::ip();

    // Build the UDP header + payload.
    let udp_len = UDP_HEADER_SIZE + data.len();
    let mut udp_packet = Vec::with_capacity(udp_len);

    // Source port.
    udp_packet.extend_from_slice(&src_port.to_be_bytes());
    // Destination port.
    udp_packet.extend_from_slice(&dst_port.to_be_bytes());
    // Length (header + data).
    udp_packet.extend_from_slice(&(udp_len as u16).to_be_bytes());
    // Checksum (0 = disabled — valid for UDP over IPv4).
    udp_packet.extend_from_slice(&0u16.to_be_bytes());
    // Payload.
    udp_packet.extend_from_slice(data);

    // Send as an IPv4 packet.
    ipv4::send(dst_ip, PROTO_UDP, &udp_packet)
}

// ---------------------------------------------------------------------------
// UDP processing
// ---------------------------------------------------------------------------

/// Process an incoming UDP datagram extracted from an IPv4 packet.
#[allow(clippy::arithmetic_side_effects)]
pub fn process_udp(ip_packet: &Ipv4Packet<'_>) -> KernelResult<()> {
    let data = ip_packet.payload;
    if data.len() < UDP_HEADER_SIZE {
        return Err(KernelError::InvalidArgument);
    }

    let src_port = u16::from_be_bytes([data[0], data[1]]);
    let dst_port = u16::from_be_bytes([data[2], data[3]]);
    let _length = u16::from_be_bytes([data[4], data[5]]);
    // Skip checksum validation for now (checksum field = 0 is valid).

    let payload = &data[UDP_HEADER_SIZE..];

    // First check DHCP (port 68 = DHCP client).
    if dst_port == 68 {
        return super::dhcp::process_dhcp_response(payload);
    }

    // Deliver to bound socket.
    let mut sockets = SOCKETS.lock();
    for sock in sockets.iter_mut() {
        if sock.active && sock.port == dst_port {
            if sock.rx_queue.len() < MAX_QUEUED {
                sock.rx_queue.push(Datagram {
                    src_ip: ip_packet.src,
                    src_port,
                    data: Vec::from(payload),
                });
            }
            // else: queue full, drop silently (UDP is unreliable).
            return Ok(());
        }
    }

    // No socket bound — drop silently.
    Ok(())
}
