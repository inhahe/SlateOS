//! UDP (User Datagram Protocol) implementation.
//!
//! Provides connectionless, unreliable datagram transport (RFC 768).
//! Used by DHCP, DNS, and other protocols that don't need TCP's
//! reliability guarantees.
//!
//! ## Multicast (RFC 1112)
//!
//! UDP sockets can join multicast groups (224.0.0.0/4) via
//! `join_group()` / `leave_group()`.  Incoming multicast datagrams are
//! delivered to all sockets bound to the destination port that have
//! joined the group (fan-out).  A global membership table is maintained
//! so the IPv4 receive path can quickly accept multicast-addressed
//! packets without scanning per-socket state.
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

/// Maximum number of multicast groups a single socket can join.
const MAX_GROUPS_PER_SOCKET: usize = 8;

/// Maximum number of total multicast group memberships across all sockets.
/// Used by the IP layer to quickly check if we should accept a multicast
/// destination address.
const MAX_GLOBAL_GROUPS: usize = 32;

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
    /// Multicast groups this socket has joined.
    /// Each entry is a multicast IPv4 address (224.0.0.0/4).
    mcast_groups: [Ipv4Addr; MAX_GROUPS_PER_SOCKET],
    /// Number of active multicast group memberships.
    mcast_count: u8,
}

impl UdpSocket {
    const fn empty() -> Self {
        Self {
            port: 0,
            active: false,
            rx_queue: Vec::new(),
            mcast_groups: [Ipv4Addr::UNSPECIFIED; MAX_GROUPS_PER_SOCKET],
            mcast_count: 0,
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

/// Global table of multicast group addresses we have joined.
///
/// The IPv4 receive path checks this to decide whether to accept
/// multicast-destined packets.  Entries are reference-counted:
/// multiple sockets can join the same group, and the entry is only
/// removed when the last socket leaves.
struct McastEntry {
    /// Multicast group address.
    addr: Ipv4Addr,
    /// Number of sockets that have joined this group.
    refcount: u16,
}

impl McastEntry {
    const fn empty() -> Self {
        Self { addr: Ipv4Addr::UNSPECIFIED, refcount: 0 }
    }
}

/// Global multicast group membership table.
static MCAST_GROUPS: Mutex<[McastEntry; MAX_GLOBAL_GROUPS]> = Mutex::new({
    const EMPTY: McastEntry = McastEntry::empty();
    [EMPTY; MAX_GLOBAL_GROUPS]
});

/// Add a reference to a multicast group in the global table.
fn mcast_global_join(group: Ipv4Addr) {
    let mut groups = MCAST_GROUPS.lock();

    // If already present, increment refcount.
    for entry in groups.iter_mut() {
        if entry.refcount > 0 && entry.addr == group {
            entry.refcount = entry.refcount.saturating_add(1);
            return;
        }
    }

    // Not present — find a free slot.
    for entry in groups.iter_mut() {
        if entry.refcount == 0 {
            entry.addr = group;
            entry.refcount = 1;
            return;
        }
    }

    // Table full — silently ignore (best effort).
    crate::serial_println!(
        "[udp] Warning: multicast group table full, cannot join {}",
        group
    );
}

/// Remove a reference from a multicast group in the global table.
fn mcast_global_leave(group: Ipv4Addr) {
    let mut groups = MCAST_GROUPS.lock();
    for entry in groups.iter_mut() {
        if entry.refcount > 0 && entry.addr == group {
            entry.refcount = entry.refcount.saturating_sub(1);
            return;
        }
    }
}

/// Check if any socket has joined the given multicast group.
///
/// Called by the IPv4 receive path to decide whether to accept
/// a packet addressed to a multicast destination.
pub fn is_multicast_member(group: Ipv4Addr) -> bool {
    let groups = MCAST_GROUPS.lock();
    groups.iter().any(|e| e.refcount > 0 && e.addr == group)
}

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
///
/// Also leaves all multicast groups the socket had joined.
pub fn close(handle: usize) {
    let mut sockets = SOCKETS.lock();
    if let Some(sock) = sockets.get_mut(handle) {
        // Leave all multicast groups before closing.
        for i in 0..sock.mcast_count as usize {
            let group = sock.mcast_groups[i];
            if !group.is_unspecified() {
                mcast_global_leave(group);
            }
        }
        sock.active = false;
        sock.port = 0;
        sock.rx_queue.clear();
        sock.mcast_groups = [Ipv4Addr::UNSPECIFIED; MAX_GROUPS_PER_SOCKET];
        sock.mcast_count = 0;
    }
}

/// Join a multicast group on a UDP socket.
///
/// After joining, the socket will receive datagrams sent to the
/// multicast group address on the socket's bound port.
///
/// # Errors
///
/// - `InvalidArgument` — handle invalid, socket not active, or address
///   is not a multicast address (224.0.0.0/4).
/// - `OutOfMemory` — socket has joined the maximum number of groups.
/// - `AlreadyExists` — socket is already a member of this group.
pub fn join_group(handle: usize, group: Ipv4Addr) -> KernelResult<()> {
    if !group.is_multicast() {
        return Err(KernelError::InvalidArgument);
    }

    let mut sockets = SOCKETS.lock();
    let sock = sockets.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
    if !sock.active {
        return Err(KernelError::InvalidArgument);
    }

    // Check if already a member.
    let count = sock.mcast_count as usize;
    for i in 0..count {
        if sock.mcast_groups[i] == group {
            return Err(KernelError::AlreadyExists);
        }
    }

    // Check capacity.
    if count >= MAX_GROUPS_PER_SOCKET {
        return Err(KernelError::OutOfMemory);
    }

    sock.mcast_groups[count] = group;
    sock.mcast_count = sock.mcast_count.saturating_add(1);

    // Add to global table so the IP layer accepts the multicast address.
    drop(sockets);
    mcast_global_join(group);

    crate::serial_println!(
        "[udp] Socket {} joined multicast group {}",
        handle, group
    );
    Ok(())
}

/// Leave a multicast group on a UDP socket.
///
/// # Errors
///
/// - `InvalidArgument` — handle invalid or socket not active.
/// - `NotFound` — socket is not a member of this group.
pub fn leave_group(handle: usize, group: Ipv4Addr) -> KernelResult<()> {
    let mut sockets = SOCKETS.lock();
    let sock = sockets.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
    if !sock.active {
        return Err(KernelError::InvalidArgument);
    }

    let count = sock.mcast_count as usize;
    let mut found = false;
    for i in 0..count {
        if sock.mcast_groups[i] == group {
            // Swap-remove: move last entry here and shrink count.
            let last = count.wrapping_sub(1);
            sock.mcast_groups[i] = sock.mcast_groups[last];
            sock.mcast_groups[last] = Ipv4Addr::UNSPECIFIED;
            sock.mcast_count = sock.mcast_count.saturating_sub(1);
            found = true;
            break;
        }
    }

    if !found {
        return Err(KernelError::NotFound);
    }

    // Remove from global table.
    drop(sockets);
    mcast_global_leave(group);

    crate::serial_println!(
        "[udp] Socket {} left multicast group {}",
        handle, group
    );
    Ok(())
}

/// Check whether a UDP socket has datagrams ready to receive.
///
/// Returns the number of queued datagrams (0 if none).  Useful for
/// POSIX poll/select readiness checks without consuming data.
/// Returns 0 for invalid or inactive handles.
pub fn rx_ready(handle: usize) -> usize {
    let sockets = SOCKETS.lock();
    match sockets.get(handle) {
        Some(sock) if sock.active => sock.rx_queue.len(),
        _ => 0,
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
///
/// Computes a proper UDP checksum over the pseudo-header + segment
/// (RFC 768).  While checksum=0 is technically valid for UDP over
/// IPv4 (meaning "no checksum"), sending real checksums enables
/// receivers to detect corruption from faulty NICs, bit-flips, or
/// intermediate routers.
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
    // Checksum placeholder (zeroed for checksum computation).
    udp_packet.extend_from_slice(&0u16.to_be_bytes());
    // Payload.
    udp_packet.extend_from_slice(data);

    // Compute and fill in the UDP checksum.
    let cksum = ipv4::compute_transport_checksum(
        src_ip, dst_ip, PROTO_UDP, &udp_packet,
    );
    udp_packet[6] = (cksum >> 8) as u8;
    udp_packet[7] = cksum as u8;

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

    // Verify UDP checksum (pseudo-header + segment).
    // verify_transport_checksum handles the "checksum = 0 means no
    // checksum" case for UDP over IPv4 (RFC 768).
    if !ipv4::verify_transport_checksum(
        ip_packet.src, ip_packet.dst, PROTO_UDP, data,
    ) {
        crate::serial_println!(
            "[udp] Dropped datagram from {} — bad checksum",
            ip_packet.src
        );
        return Ok(());
    }

    let src_port = u16::from_be_bytes([data[0], data[1]]);
    let dst_port = u16::from_be_bytes([data[2], data[3]]);
    let _length = u16::from_be_bytes([data[4], data[5]]);

    let payload = &data[UDP_HEADER_SIZE..];

    // First check DHCP (port 68 = DHCP client).
    if dst_port == 68 {
        return super::dhcp::process_dhcp_response(payload);
    }

    let is_mcast = ip_packet.dst.is_multicast();

    // Deliver to bound socket(s).
    // For unicast: deliver to the first matching socket.
    // For multicast: deliver to ALL sockets bound to this port that have
    //   joined the multicast group (fan-out).
    let mut sockets = SOCKETS.lock();
    let mut delivered = false;
    for sock in sockets.iter_mut() {
        if !sock.active || sock.port != dst_port {
            continue;
        }

        // For multicast, check group membership.
        if is_mcast {
            let count = sock.mcast_count as usize;
            let mut member = false;
            for i in 0..count {
                if sock.mcast_groups[i] == ip_packet.dst {
                    member = true;
                    break;
                }
            }
            if !member {
                continue;
            }
        }

        if sock.rx_queue.len() < MAX_QUEUED {
            sock.rx_queue.push(Datagram {
                src_ip: ip_packet.src,
                src_port,
                data: Vec::from(payload),
            });
        }
        // else: queue full, drop silently (UDP is unreliable).

        delivered = true;

        // For unicast, only deliver to the first matching socket.
        if !is_mcast {
            break;
        }
    }

    if delivered {
        return Ok(());
    }

    // No socket bound — send ICMP Port Unreachable (RFC 1122 §3.2.2.1).
    // Only for unicast; multicast/broadcast should be dropped silently.
    if !is_mcast && !ip_packet.dst.is_broadcast() {
        // First 8 bytes of the UDP header for the ICMP error payload.
        if data.len() >= 8 {
            let _ = super::icmp::send_port_unreachable(
                ip_packet.src,
                ip_packet.raw_header,
                &data[..8],
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// UDP socket diagnostics
// ---------------------------------------------------------------------------

/// Diagnostic snapshot of one UDP socket.
#[derive(Debug, Clone, Copy)]
pub struct UdpSocketInfo {
    /// Slot index (handle).
    pub handle: usize,
    /// Bound local port.
    pub local_port: u16,
    /// Number of datagrams queued for receive.
    pub rx_queue_len: usize,
    /// Number of multicast groups joined.
    pub mcast_groups: u8,
}

/// Return a list of all active UDP sockets.
///
/// Useful for `netstat -u` style display in the shell.
pub fn all_sockets() -> ([UdpSocketInfo; MAX_SOCKETS], usize) {
    let sockets = SOCKETS.lock();
    let mut out = [UdpSocketInfo {
        handle: 0,
        local_port: 0,
        rx_queue_len: 0,
        mcast_groups: 0,
    }; MAX_SOCKETS];
    let mut count: usize = 0;

    for (i, sock) in sockets.iter().enumerate() {
        if sock.active {
            if let Some(slot) = out.get_mut(count) {
                *slot = UdpSocketInfo {
                    handle: i,
                    local_port: sock.port,
                    rx_queue_len: sock.rx_queue.len(),
                    mcast_groups: sock.mcast_count,
                };
                count = count.wrapping_add(1);
            }
        }
    }

    (out, count)
}
