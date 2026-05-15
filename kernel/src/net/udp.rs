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

use alloc::collections::VecDeque;
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
    /// Connected peer address (0.0.0.0 = not connected / unfiltered).
    /// When set, recv/peek only return datagrams from this peer.
    peer_ip: Ipv4Addr,
    /// Connected peer port (0 = not connected / unfiltered).
    peer_port: u16,
    /// Received datagrams waiting to be read.
    rx_queue: VecDeque<Datagram>,
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
            peer_ip: Ipv4Addr::UNSPECIFIED,
            peer_port: 0,
            rx_queue: VecDeque::new(),
            mcast_groups: [Ipv4Addr::UNSPECIFIED; MAX_GROUPS_PER_SOCKET],
            mcast_count: 0,
        }
    }

    /// Whether this socket is in connected mode (has a peer filter).
    fn is_connected(&self) -> bool {
        !self.peer_ip.is_unspecified() || self.peer_port != 0
    }

    /// Whether a datagram matches the connected peer filter.
    /// Returns true if not connected (accept all) or if source matches.
    fn matches_peer(&self, src_ip: Ipv4Addr, src_port: u16) -> bool {
        if !self.is_connected() {
            return true;
        }
        self.peer_ip == src_ip && self.peer_port == src_port
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

/// Start of IANA dynamic/private port range for ephemeral allocation.
const EPHEMERAL_PORT_START: u16 = 49152;

/// Allocate an ephemeral port in the IANA dynamic range (49152–65535).
///
/// Scans linearly from `EPHEMERAL_PORT_START`.  With `MAX_SOCKETS=32`,
/// we can never have more than 32 active sockets, so a free port is
/// always found within the first 33 candidates.
fn allocate_ephemeral_port(sockets: &[UdpSocket; MAX_SOCKETS]) -> KernelResult<u16> {
    for candidate in EPHEMERAL_PORT_START..=u16::MAX {
        let in_use = sockets.iter().any(|s| s.active && s.port == candidate);
        if !in_use {
            return Ok(candidate);
        }
    }
    Err(KernelError::OutOfMemory)
}

/// Bind a UDP socket to a local port.
///
/// Pass port 0 to auto-assign an ephemeral port from the IANA
/// dynamic range (49152–65535).  Returns a socket index (handle)
/// on success.
pub fn bind(port: u16) -> KernelResult<usize> {
    let mut sockets = SOCKETS.lock();

    let effective_port = if port == 0 {
        allocate_ephemeral_port(&sockets)?
    } else {
        // Check for duplicate binding.
        for sock in sockets.iter() {
            if sock.active && sock.port == port {
                return Err(KernelError::AlreadyExists);
            }
        }
        port
    };

    // Find a free slot.
    for (i, sock) in sockets.iter_mut().enumerate() {
        if !sock.active {
            sock.active = true;
            sock.port = effective_port;
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
    // Collect multicast groups under the SOCKETS lock, then drop
    // it before touching MCAST_GROUPS to maintain consistent lock
    // ordering (join_group/leave_group drop SOCKETS before taking
    // MCAST_GROUPS — close() must do the same).
    let mut groups_to_leave: [Ipv4Addr; MAX_GROUPS_PER_SOCKET] =
        [Ipv4Addr::UNSPECIFIED; MAX_GROUPS_PER_SOCKET];
    let mut group_count: usize = 0;

    {
        let mut sockets = SOCKETS.lock();
        if let Some(sock) = sockets.get_mut(handle) {
            // Snapshot multicast memberships before clearing.
            group_count = sock.mcast_count as usize;
            for i in 0..group_count {
                if let Some(g) = sock.mcast_groups.get(i) {
                    groups_to_leave[i] = *g;
                }
            }
            sock.active = false;
            sock.port = 0;
            sock.peer_ip = Ipv4Addr::UNSPECIFIED;
            sock.peer_port = 0;
            sock.rx_queue.clear();
            sock.mcast_groups = [Ipv4Addr::UNSPECIFIED; MAX_GROUPS_PER_SOCKET];
            sock.mcast_count = 0;
        }
    }

    // SOCKETS lock released — now safe to take MCAST_GROUPS.
    for i in 0..group_count {
        let group = groups_to_leave[i];
        if !group.is_unspecified() {
            mcast_global_leave(group);
        }
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

/// Return the byte length of the first deliverable datagram.
///
/// For FIONREAD: POSIX specifies this returns the size of the first
/// pending datagram that would be returned by recv().  In connected
/// mode, skips non-matching datagrams (but does not discard them —
/// this is a pure query).
/// Returns 0 if no matching datagram is queued.
pub fn rx_front_bytes(handle: usize) -> usize {
    let sockets = SOCKETS.lock();
    let Some(sock) = sockets.get(handle) else { return 0 };
    if !sock.active {
        return 0;
    }
    // Find first datagram that matches the peer filter.
    for dg in &sock.rx_queue {
        if sock.matches_peer(dg.src_ip, dg.src_port) {
            return dg.data.len();
        }
    }
    0
}

/// Receive a datagram from a bound socket.
///
/// If the socket is in connected mode, skips (discards) datagrams
/// from non-matching sources and returns the first matching one.
/// Returns `None` if no matching datagrams are queued.
pub fn recv(handle: usize) -> Option<Datagram> {
    let mut sockets = SOCKETS.lock();
    let sock = sockets.get_mut(handle)?;
    if !sock.active {
        return None;
    }

    // In connected mode, discard datagrams that don't match the peer.
    // This is FIFO: we remove from the front, skipping non-matching.
    loop {
        if sock.rx_queue.is_empty() {
            return None;
        }
        // Check if front datagram matches the peer filter.
        let front = &sock.rx_queue[0];
        if sock.matches_peer(front.src_ip, front.src_port) {
            return sock.rx_queue.pop_front();
        }
        // Not from connected peer — discard and try next.
        sock.rx_queue.pop_front();
    }
}

/// Peek at the next datagram without removing it from the queue.
///
/// If the socket is in connected mode, returns the first datagram
/// that matches the peer filter (discarding earlier non-matching ones,
/// since they would never be delivered anyway).
/// Returns `None` if no matching datagrams are queued.
pub fn peek(handle: usize) -> Option<Datagram> {
    let mut sockets = SOCKETS.lock();
    let sock = sockets.get_mut(handle)?;
    if !sock.active {
        return None;
    }

    // Drain non-matching datagrams from the front (they'd be discarded
    // on recv() anyway, so removing them during peek is correct).
    loop {
        if sock.rx_queue.is_empty() {
            return None;
        }
        let front = &sock.rx_queue[0];
        if sock.matches_peer(front.src_ip, front.src_port) {
            return sock.rx_queue.front().cloned();
        }
        // Not from connected peer — discard.
        sock.rx_queue.pop_front();
    }
}

/// Set the connected peer for a UDP socket (connected-mode filter).
///
/// Return the local port bound to a UDP socket.
///
/// Returns `Some(port)` for valid active sockets, `None` for invalid handles.
pub fn local_port(handle: usize) -> Option<u16> {
    let sockets = SOCKETS.lock();
    let sock = sockets.get(handle)?;
    if !sock.active {
        return None;
    }
    Some(sock.port)
}

/// After calling this, recv/peek will only return datagrams from
/// the specified peer.  Pass `Ipv4Addr::UNSPECIFIED` and port 0 to
/// disconnect (remove filter).
pub fn connect(handle: usize, peer_ip: Ipv4Addr, peer_port: u16) -> KernelResult<()> {
    let mut sockets = SOCKETS.lock();
    let sock = sockets.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
    if !sock.active {
        return Err(KernelError::InvalidArgument);
    }
    sock.peer_ip = peer_ip;
    sock.peer_port = peer_port;
    Ok(())
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

    // UDP Length field is 16 bits; maximum datagram is 65535 bytes
    // (8-byte header + 65527 payload).  Reject oversized payloads
    // instead of silently truncating the length field.
    if udp_len > u16::MAX as usize {
        return Err(KernelError::InvalidArgument);
    }

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

    // Send as an IPv4 packet.  Use the fragmentable path since UDP
    // datagrams may exceed the interface MTU (unlike TCP which uses MSS).
    ipv4::send_fragmentable(dst_ip, PROTO_UDP, &udp_packet)
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
    let udp_length = u16::from_be_bytes([data[4], data[5]]) as usize;

    // Use the UDP Length field to determine actual payload size.
    // This strips Ethernet minimum-frame padding that the IP layer
    // may have passed through in its payload slice (RFC 768 §1).
    let payload_end = if udp_length >= UDP_HEADER_SIZE && udp_length <= data.len() {
        udp_length
    } else if udp_length < UDP_HEADER_SIZE {
        // Malformed length — less than the minimum 8-byte header.
        crate::serial_println!(
            "[udp] Dropped datagram from {} — malformed length {}",
            ip_packet.src, udp_length
        );
        return Ok(());
    } else {
        // Length exceeds IP payload (truncated?) — use what we have.
        data.len()
    };

    let payload = &data[UDP_HEADER_SIZE..payload_end];

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
            sock.rx_queue.push_back(Datagram {
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
    #[allow(dead_code)] // Public API.
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
