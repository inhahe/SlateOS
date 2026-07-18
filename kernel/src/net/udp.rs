//! UDP (User Datagram Protocol) implementation.
//!
//! Provides connectionless, unreliable datagram transport (RFC 768).
//! Used by DHCP, DNS, and other protocols that don't need TCP's
//! reliability guarantees.
//!
//! ## Multicast (RFC 1112, RFC 3810)
//!
//! UDP sockets can join IPv4 multicast groups (224.0.0.0/4) via
//! `join_group()` / `leave_group()` and IPv6 multicast groups (ff00::/8)
//! via `join_group_v6()` / `leave_group_v6()`.  Incoming multicast
//! datagrams are delivered to all sockets bound to the destination port
//! that have joined the group (fan-out).  Separate global membership
//! tables are maintained for IPv4 and IPv6 so the IP receive paths can
//! quickly accept multicast-addressed packets without scanning per-socket
//! state.
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

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

use super::interface::Ipv4Addr;
use super::ipv4::{self, Ipv4Packet, PROTO_UDP};
use super::ipv6::{self, Ipv6Addr, Ipv6Packet};
use crate::netns::NetNsId;

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

/// Maximum number of multicast groups a single socket can join (per address family).
const MAX_GROUPS_PER_SOCKET: usize = 8;

/// Maximum number of total multicast group memberships across all sockets.
/// Used by the IP layer to quickly check if we should accept a multicast
/// destination address.
const MAX_GLOBAL_GROUPS: usize = 32;

/// Maximum number of total IPv6 multicast group memberships across all sockets.
const MAX_GLOBAL_GROUPS_V6: usize = 32;

// ---------------------------------------------------------------------------
// UDP socket
// ---------------------------------------------------------------------------

/// A received UDP datagram (IPv4 source).
#[derive(Debug, Clone)]
pub struct Datagram {
    /// Source IPv4 address.
    pub src_ip: Ipv4Addr,
    /// Source port.
    pub src_port: u16,
    /// Payload data.
    pub data: Vec<u8>,
}

/// A received UDP datagram from an IPv6 source.
///
/// Separate from [`Datagram`] because IPv6 and IPv4 are distinct address
/// families with different address sizes (16 vs 4 bytes).  Keeping them
/// separate avoids forcing all existing IPv4 callers to handle a type
/// they'll never encounter, mirroring BSD's `sockaddr_in`/`sockaddr_in6`.
#[derive(Debug, Clone)]
pub struct DatagramV6 {
    /// Source IPv6 address.
    pub src_ip: Ipv6Addr,
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
    /// Network namespace this socket belongs to.
    /// Sockets in different namespaces are fully independent — the same
    /// port can be bound in multiple namespaces without conflict.
    ns_id: NetNsId,
    /// Connected peer address (0.0.0.0 = not connected / unfiltered).
    /// When set, recv/peek only return datagrams from this peer.
    peer_ip: Ipv4Addr,
    /// Connected peer port (0 = not connected / unfiltered).
    peer_port: u16,
    /// Received IPv4 datagrams waiting to be read.
    rx_queue: VecDeque<Datagram>,
    /// Received IPv6 datagrams waiting to be read.
    rx_queue_v6: VecDeque<DatagramV6>,
    /// Multicast groups this socket has joined.
    /// Each entry is a multicast IPv4 address (224.0.0.0/4).
    mcast_groups: [Ipv4Addr; MAX_GROUPS_PER_SOCKET],
    /// Number of active IPv4 multicast group memberships.
    mcast_count: u8,
    /// IPv6 multicast groups this socket has joined.
    /// Each entry is a multicast IPv6 address (ff00::/8).
    mcast_groups_v6: [Ipv6Addr; MAX_GROUPS_PER_SOCKET],
    /// Number of active IPv6 multicast group memberships.
    mcast_count_v6: u8,
}

impl UdpSocket {
    const fn empty() -> Self {
        Self {
            port: 0,
            active: false,
            ns_id: crate::netns::ROOT_NS,
            peer_ip: Ipv4Addr::UNSPECIFIED,
            peer_port: 0,
            rx_queue: VecDeque::new(),
            rx_queue_v6: VecDeque::new(),
            mcast_groups: [Ipv4Addr::UNSPECIFIED; MAX_GROUPS_PER_SOCKET],
            mcast_count: 0,
            mcast_groups_v6: [Ipv6Addr::UNSPECIFIED; MAX_GROUPS_PER_SOCKET],
            mcast_count_v6: 0,
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
// IPv6 multicast group management
// ---------------------------------------------------------------------------

/// IPv6 multicast group entry with refcount (mirrors `McastEntry` for IPv4).
struct McastEntryV6 {
    /// Multicast group address.
    addr: Ipv6Addr,
    /// Number of sockets that have joined this group.
    refcount: u16,
}

impl McastEntryV6 {
    const fn empty() -> Self {
        Self { addr: Ipv6Addr::UNSPECIFIED, refcount: 0 }
    }
}

/// Global IPv6 multicast group membership table.
static MCAST_GROUPS_V6: Mutex<[McastEntryV6; MAX_GLOBAL_GROUPS_V6]> = Mutex::new({
    const EMPTY: McastEntryV6 = McastEntryV6::empty();
    [EMPTY; MAX_GLOBAL_GROUPS_V6]
});

/// Add a reference to an IPv6 multicast group in the global table.
fn mcast_global_join_v6(group: Ipv6Addr) {
    let mut groups = MCAST_GROUPS_V6.lock();

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
        "[udp] Warning: IPv6 multicast group table full, cannot join {}",
        group
    );
}

/// Remove a reference from an IPv6 multicast group in the global table.
fn mcast_global_leave_v6(group: Ipv6Addr) {
    let mut groups = MCAST_GROUPS_V6.lock();
    for entry in groups.iter_mut() {
        if entry.refcount > 0 && entry.addr == group {
            entry.refcount = entry.refcount.saturating_sub(1);
            return;
        }
    }
}

/// Check if any socket has joined the given IPv6 multicast group.
///
/// Called by the IPv6 receive path to decide whether to accept
/// a packet addressed to a multicast destination.
pub fn is_multicast_member_v6(group: Ipv6Addr) -> bool {
    let groups = MCAST_GROUPS_V6.lock();
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
/// dynamic range (49152–65535).  `ns_id` identifies the network
/// namespace; pass `netns::ROOT_NS` for the host namespace.  The
/// same port can be bound in different namespaces without conflict.
///
/// Returns a socket index (handle) on success.
pub fn bind(ns_id: NetNsId, port: u16) -> KernelResult<usize> {
    let mut sockets = SOCKETS.lock();

    let effective_port = if port == 0 {
        allocate_ephemeral_port(&sockets)?
    } else {
        // Check for duplicate binding within the same namespace.
        for sock in sockets.iter() {
            if sock.active && sock.ns_id == ns_id && sock.port == port {
                return Err(KernelError::AlreadyExists);
            }
        }
        port
    };

    // Find a free slot.
    for (i, sock) in sockets.iter_mut().enumerate() {
        if !sock.active {
            sock.active = true;
            sock.ns_id = ns_id;
            sock.port = effective_port;
            sock.rx_queue.clear();
            sock.rx_queue_v6.clear();
            return Ok(i);
        }
    }

    // All socket slots are in use.
    Err(KernelError::OutOfMemory)
}

/// Close a UDP socket.
///
/// Also leaves all multicast groups (IPv4 and IPv6) the socket had joined.
pub fn close(handle: usize) {
    // Collect multicast groups under the SOCKETS lock, then drop
    // it before touching MCAST_GROUPS / MCAST_GROUPS_V6 to maintain
    // consistent lock ordering (join_group/leave_group drop SOCKETS
    // before taking MCAST_GROUPS — close() must do the same).
    let mut groups_to_leave: [Ipv4Addr; MAX_GROUPS_PER_SOCKET] =
        [Ipv4Addr::UNSPECIFIED; MAX_GROUPS_PER_SOCKET];
    let mut group_count: usize = 0;
    let mut groups_to_leave_v6: [Ipv6Addr; MAX_GROUPS_PER_SOCKET] =
        [Ipv6Addr::UNSPECIFIED; MAX_GROUPS_PER_SOCKET];
    let mut group_count_v6: usize = 0;

    {
        let mut sockets = SOCKETS.lock();
        if let Some(sock) = sockets.get_mut(handle) {
            // Snapshot IPv4 multicast memberships before clearing.
            group_count = sock.mcast_count as usize;
            for i in 0..group_count {
                if let Some(g) = sock.mcast_groups.get(i) {
                    groups_to_leave[i] = *g;
                }
            }
            // Snapshot IPv6 multicast memberships before clearing.
            group_count_v6 = sock.mcast_count_v6 as usize;
            for i in 0..group_count_v6 {
                if let Some(g) = sock.mcast_groups_v6.get(i) {
                    groups_to_leave_v6[i] = *g;
                }
            }
            sock.active = false;
            sock.ns_id = crate::netns::ROOT_NS;
            sock.port = 0;
            sock.peer_ip = Ipv4Addr::UNSPECIFIED;
            sock.peer_port = 0;
            sock.rx_queue.clear();
            sock.rx_queue_v6.clear();
            sock.mcast_groups = [Ipv4Addr::UNSPECIFIED; MAX_GROUPS_PER_SOCKET];
            sock.mcast_count = 0;
            sock.mcast_groups_v6 = [Ipv6Addr::UNSPECIFIED; MAX_GROUPS_PER_SOCKET];
            sock.mcast_count_v6 = 0;
        }
    }

    // SOCKETS lock released — now safe to take MCAST_GROUPS / MCAST_GROUPS_V6.
    for i in 0..group_count {
        let group = groups_to_leave[i];
        if !group.is_unspecified() {
            mcast_global_leave(group);
        }
    }
    for i in 0..group_count_v6 {
        let group = groups_to_leave_v6[i];
        if !group.is_unspecified() {
            mcast_global_leave_v6(group);
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

    // Notify IGMP so the network receives our membership report.
    super::igmp::join(group);

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

    // Notify IGMP so the network receives our leave message.
    super::igmp::leave(group);

    crate::serial_println!(
        "[udp] Socket {} left multicast group {}",
        handle, group
    );
    Ok(())
}

/// Join an IPv6 multicast group on a UDP socket.
///
/// After joining, the socket will receive IPv6 datagrams sent to the
/// multicast group address on the socket's bound port.
///
/// # Errors
///
/// - `InvalidArgument` — handle invalid, socket not active, or address
///   is not a multicast address (ff00::/8).
/// - `OutOfMemory` — socket has joined the maximum number of groups.
/// - `AlreadyExists` — socket is already a member of this group.
pub fn join_group_v6(handle: usize, group: Ipv6Addr) -> KernelResult<()> {
    if !group.is_multicast() {
        return Err(KernelError::InvalidArgument);
    }

    let mut sockets = SOCKETS.lock();
    let sock = sockets.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
    if !sock.active {
        return Err(KernelError::InvalidArgument);
    }

    // Check if already a member.
    let count = sock.mcast_count_v6 as usize;
    for i in 0..count {
        if sock.mcast_groups_v6[i] == group {
            return Err(KernelError::AlreadyExists);
        }
    }

    // Check capacity.
    if count >= MAX_GROUPS_PER_SOCKET {
        return Err(KernelError::OutOfMemory);
    }

    sock.mcast_groups_v6[count] = group;
    sock.mcast_count_v6 = sock.mcast_count_v6.saturating_add(1);

    // Add to global table so the IPv6 layer accepts the multicast address.
    drop(sockets);
    mcast_global_join_v6(group);

    // Notify MLD so the network receives our listener report.
    super::mld::join(group);

    crate::serial_println!(
        "[udp] Socket {} joined IPv6 multicast group {}",
        handle, group
    );
    Ok(())
}

/// Leave an IPv6 multicast group on a UDP socket.
///
/// # Errors
///
/// - `InvalidArgument` — handle invalid or socket not active.
/// - `NotFound` — socket is not a member of this group.
pub fn leave_group_v6(handle: usize, group: Ipv6Addr) -> KernelResult<()> {
    let mut sockets = SOCKETS.lock();
    let sock = sockets.get_mut(handle).ok_or(KernelError::InvalidArgument)?;
    if !sock.active {
        return Err(KernelError::InvalidArgument);
    }

    let count = sock.mcast_count_v6 as usize;
    let mut found = false;
    for i in 0..count {
        if sock.mcast_groups_v6[i] == group {
            // Swap-remove: move last entry here and shrink count.
            let last = count.wrapping_sub(1);
            sock.mcast_groups_v6[i] = sock.mcast_groups_v6[last];
            sock.mcast_groups_v6[last] = Ipv6Addr::UNSPECIFIED;
            sock.mcast_count_v6 = sock.mcast_count_v6.saturating_sub(1);
            found = true;
            break;
        }
    }

    if !found {
        return Err(KernelError::NotFound);
    }

    // Remove from global table.
    drop(sockets);
    mcast_global_leave_v6(group);

    // Notify MLD so the network receives our done message.
    super::mld::leave(group);

    crate::serial_println!(
        "[udp] Socket {} left IPv6 multicast group {}",
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
pub fn process_udp(ip_packet: &Ipv4Packet<'_>, ns_id: NetNsId) -> KernelResult<()> {
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

    // DHCP server (port 67 = DHCP server).
    // Containers send DISCOVER/REQUEST to the broadcast address on
    // port 67.  The dhcpd module processes the request and returns a
    // response payload + destination IP.  We send the reply as a
    // broadcast UDP datagram from port 67 to port 68.
    if dst_port == 67 {
        if let Some((response, dest_ip)) = super::dhcpd::process_request(payload) {
            let dest = super::interface::Ipv4Addr(dest_ip);
            // DHCP responses go from server port 67 to client port 68.
            if let Err(e) = send(67, dest, 68, &response) {
                crate::serial_println!("[udp] Failed to send DHCP response: {:?}", e);
            }
        }
        return Ok(());
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

        // Namespace scoping: a datagram arriving in namespace `ns_id` is
        // delivered only to sockets bound in that namespace.  Frames from
        // the physical NIC (ROOT_NS) may also reach namespace-bound
        // sockets (mirrors the TCP listener/connection lookup rule), so a
        // socket matches when it is in the arrival namespace or the frame
        // arrived in the root namespace.
        if ns_id != crate::netns::ROOT_NS && sock.ns_id != ns_id {
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
// IPv6 UDP processing
// ---------------------------------------------------------------------------

/// Process an incoming UDP datagram extracted from an IPv6 packet.
///
/// Verifies the mandatory IPv6 UDP checksum (RFC 8200 §8.1), parses
/// ports and length, and delivers to any socket bound to the destination
/// port.  Unlike IPv4 UDP, a checksum of zero is invalid for IPv6.
#[allow(clippy::arithmetic_side_effects)]
pub fn process_udp_v6(ip_packet: &Ipv6Packet<'_>, ns_id: NetNsId) -> KernelResult<()> {
    let data = ip_packet.payload;
    if data.len() < UDP_HEADER_SIZE {
        return Err(KernelError::InvalidArgument);
    }

    // For UDP over IPv6, checksum is mandatory (RFC 8200 §8.1).
    // A transmitted checksum of zero means "no checksum" in IPv4 but is
    // illegal over IPv6 — drop the datagram.
    let raw_cksum = u16::from_be_bytes([data[6], data[7]]);
    if raw_cksum == 0 {
        crate::serial_println!(
            "[udp] Dropped IPv6 datagram from {} — zero checksum (invalid over IPv6)",
            ip_packet.src
        );
        return Ok(());
    }

    // Verify UDP checksum using the IPv6 pseudo-header.
    if !ipv6::verify_transport_checksum(
        &ip_packet.src, &ip_packet.dst, ipv6::NH_UDP, data,
    ) {
        crate::serial_println!(
            "[udp] Dropped IPv6 datagram from {} — bad checksum",
            ip_packet.src
        );
        return Ok(());
    }

    let src_port = u16::from_be_bytes([data[0], data[1]]);
    let dst_port = u16::from_be_bytes([data[2], data[3]]);
    let udp_length = u16::from_be_bytes([data[4], data[5]]) as usize;

    // Use the UDP Length field to determine actual payload size,
    // stripping any link-layer padding (same logic as IPv4 path).
    let payload_end = if udp_length >= UDP_HEADER_SIZE && udp_length <= data.len() {
        udp_length
    } else if udp_length < UDP_HEADER_SIZE {
        crate::serial_println!(
            "[udp] Dropped IPv6 datagram from {} — malformed length {}",
            ip_packet.src, udp_length
        );
        return Ok(());
    } else {
        // Length exceeds payload (truncated?) — use what we have.
        data.len()
    };

    let payload = &data[UDP_HEADER_SIZE..payload_end];

    // DHCPv6 (port 546 = client, port 547 = server) is future work.
    // No special dispatch for now.

    let is_mcast = ip_packet.dst.is_multicast();

    // Deliver to bound socket(s).
    // For unicast: deliver to the first matching socket.
    // For multicast: deliver to ALL sockets bound to this port that have
    //   joined the multicast group (fan-out), mirroring IPv4 behavior.
    let mut sockets = SOCKETS.lock();
    let mut delivered = false;
    for sock in sockets.iter_mut() {
        if !sock.active || sock.port != dst_port {
            continue;
        }

        // Namespace scoping: mirror the IPv4 path — a datagram arriving in
        // namespace `ns_id` is delivered only to sockets bound in that
        // namespace, except frames from the physical NIC (ROOT_NS) which
        // may also reach namespace-bound sockets.
        if ns_id != crate::netns::ROOT_NS && sock.ns_id != ns_id {
            continue;
        }

        // For multicast, check IPv6 group membership.
        if is_mcast {
            let count = sock.mcast_count_v6 as usize;
            let is_member = (0..count).any(|i| sock.mcast_groups_v6[i] == ip_packet.dst);
            if !is_member {
                continue;
            }
        }

        if sock.rx_queue_v6.len() < MAX_QUEUED {
            sock.rx_queue_v6.push_back(DatagramV6 {
                src_ip: ip_packet.src,
                src_port,
                data: Vec::from(payload),
            });
        }
        // else: queue full, drop silently (UDP is unreliable).

        delivered = true;

        // Unicast: stop after first match.  Multicast: continue fan-out.
        if !is_mcast {
            break;
        }
    }

    if !delivered {
        // No socket bound.  Ideally we'd send ICMPv6 Destination
        // Unreachable (port unreachable), but that's not implemented yet.
        // Silently drop — correct behavior for multicast/broadcast too.
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// IPv6 UDP socket API
// ---------------------------------------------------------------------------

/// Receive an IPv6 datagram from a bound socket.
///
/// Returns `None` if no IPv6 datagrams are queued.  IPv6 datagrams
/// are stored in a separate queue from IPv4 datagrams; callers that
/// want both must call both [`recv`] and `recv_v6`.
#[allow(dead_code)] // Public API for future IPv6 consumers.
pub fn recv_v6(handle: usize) -> Option<DatagramV6> {
    let mut sockets = SOCKETS.lock();
    let sock = sockets.get_mut(handle)?;
    if !sock.active {
        return None;
    }
    sock.rx_queue_v6.pop_front()
}

/// Peek at the next IPv6 datagram without removing it from the queue.
#[allow(dead_code)] // Public API for future IPv6 consumers.
pub fn peek_v6(handle: usize) -> Option<DatagramV6> {
    let sockets = SOCKETS.lock();
    let sock = sockets.get(handle)?;
    if !sock.active {
        return None;
    }
    sock.rx_queue_v6.front().cloned()
}

/// Check whether a UDP socket has IPv6 datagrams ready to receive.
#[allow(dead_code)] // Public API for future IPv6 consumers.
pub fn rx_ready_v6(handle: usize) -> usize {
    let sockets = SOCKETS.lock();
    match sockets.get(handle) {
        Some(sock) if sock.active => sock.rx_queue_v6.len(),
        _ => 0,
    }
}

/// Send a UDP datagram over IPv6.
///
/// Computes a mandatory UDP checksum over the IPv6 pseudo-header +
/// segment (RFC 8200 §8.1).  Uses the SLAAC global address as source
/// for non-link-local destinations, falling back to the link-local
/// address derived from the interface MAC.
#[allow(dead_code)] // Public API for future IPv6 senders.
#[allow(clippy::arithmetic_side_effects)]
pub fn send_v6(src_port: u16, dst_ip: Ipv6Addr, dst_port: u16, data: &[u8]) -> KernelResult<()> {
    let our_mac = super::interface::mac();

    // Use SLAAC global address for non-link-local destinations,
    // fall back to the link-local address.
    let src_ip = if dst_ip.is_link_local() {
        Ipv6Addr::from_mac_link_local(&our_mac)
    } else {
        super::icmpv6::slaac_global_addr()
            .unwrap_or_else(|| Ipv6Addr::from_mac_link_local(&our_mac))
    };

    let udp_len = UDP_HEADER_SIZE + data.len();

    // UDP Length field is 16 bits.
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
    // Checksum placeholder (zeroed for computation).
    udp_packet.extend_from_slice(&0u16.to_be_bytes());
    // Payload.
    udp_packet.extend_from_slice(data);

    // Compute checksum using the IPv6 pseudo-header.
    // For UDP over IPv6, a computed checksum of 0 is transmitted as
    // 0xFFFF (handled inside compute_transport_checksum).
    let cksum = ipv6::compute_transport_checksum(
        &src_ip, &dst_ip, ipv6::NH_UDP, &udp_packet,
    );
    udp_packet[6] = (cksum >> 8) as u8;
    udp_packet[7] = cksum as u8;

    // Send as an IPv6 packet.
    ipv6::send_raw(src_ip, dst_ip, ipv6::NH_UDP, 64, &udp_packet)
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
    /// Number of IPv4 datagrams queued for receive.
    pub rx_queue_len: usize,
    /// Number of IPv6 datagrams queued for receive.
    pub rx_queue_v6_len: usize,
    /// Number of IPv4 multicast groups joined.
    pub mcast_groups: u8,
    /// Number of IPv6 multicast groups joined.
    pub mcast_groups_v6: u8,
    /// Network namespace this socket belongs to.
    pub ns_id: crate::netns::NetNsId,
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
        rx_queue_v6_len: 0,
        mcast_groups: 0,
        mcast_groups_v6: 0,
        ns_id: crate::netns::ROOT_NS,
    }; MAX_SOCKETS];
    let mut count: usize = 0;

    for (i, sock) in sockets.iter().enumerate() {
        if sock.active {
            if let Some(slot) = out.get_mut(count) {
                *slot = UdpSocketInfo {
                    handle: i,
                    local_port: sock.port,
                    rx_queue_len: sock.rx_queue.len(),
                    rx_queue_v6_len: sock.rx_queue_v6.len(),
                    mcast_groups: sock.mcast_count,
                    mcast_groups_v6: sock.mcast_count_v6,
                    ns_id: sock.ns_id,
                };
                count = count.wrapping_add(1);
            }
        }
    }

    (out, count)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// UDP unit tests — exercises socket bind/close, ephemeral port allocation,
/// multicast group management, socket state queries, and IPv6 datagram
/// processing.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[udp] Running UDP self-test...");

    test_bind_close()?;
    test_bind_duplicate()?;
    test_ephemeral_port()?;
    test_multicast_join_leave()?;
    test_multicast_join_leave_v6()?;
    test_rx_ready_empty()?;
    test_connected_mode()?;
    test_v6_recv_empty()?;
    test_v6_process_and_deliver()?;
    test_namespace_isolation()?;

    crate::serial_println!("[udp] UDP self-test PASSED (10 tests)");
    Ok(())
}

/// Test basic bind + local_port + close lifecycle.
fn test_bind_close() -> KernelResult<()> {
    let handle = bind(crate::netns::ROOT_NS, 55555)?;

    // local_port should return the bound port.
    match local_port(handle) {
        Some(55555) => {}
        Some(p) => {
            crate::serial_println!("[udp]   FAIL: local_port = {}, expected 55555", p);
            close(handle);
            return Err(KernelError::InternalError);
        }
        None => {
            crate::serial_println!("[udp]   FAIL: local_port returned None");
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    close(handle);

    // After close, local_port should return None.
    if local_port(handle).is_some() {
        crate::serial_println!("[udp]   FAIL: local_port still Some after close");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[udp]   bind/close lifecycle: OK");
    Ok(())
}

/// Test that binding to the same port twice is rejected.
fn test_bind_duplicate() -> KernelResult<()> {
    let h1 = bind(crate::netns::ROOT_NS, 55556)?;

    match bind(crate::netns::ROOT_NS, 55556) {
        Err(KernelError::AlreadyExists) => {} // Expected.
        Ok(h2) => {
            crate::serial_println!("[udp]   FAIL: duplicate bind succeeded");
            close(h2);
            close(h1);
            return Err(KernelError::InternalError);
        }
        Err(e) => {
            crate::serial_println!("[udp]   FAIL: unexpected error {:?}", e);
            close(h1);
            return Err(KernelError::InternalError);
        }
    }

    close(h1);

    // After close, the port should be available again.
    let h2 = bind(crate::netns::ROOT_NS, 55556)?;
    close(h2);

    crate::serial_println!("[udp]   bind duplicate: OK (rejected)");
    Ok(())
}

/// Test ephemeral port allocation (port 0).
fn test_ephemeral_port() -> KernelResult<()> {
    let h1 = bind(crate::netns::ROOT_NS, 0)?;
    let p1 = local_port(h1);

    if p1.is_none() {
        crate::serial_println!("[udp]   FAIL: ephemeral port returned None");
        close(h1);
        return Err(KernelError::InternalError);
    }
    let p1 = p1.unwrap_or(0);

    if p1 < 49152 {
        crate::serial_println!("[udp]   FAIL: ephemeral port {} below IANA range", p1);
        close(h1);
        return Err(KernelError::InternalError);
    }

    // Second ephemeral should be different.
    let h2 = bind(crate::netns::ROOT_NS, 0)?;
    let p2 = local_port(h2).unwrap_or(0);

    if p1 == p2 {
        crate::serial_println!("[udp]   FAIL: two ephemeral ports are identical ({})", p1);
        close(h1);
        close(h2);
        return Err(KernelError::InternalError);
    }

    close(h1);
    close(h2);

    crate::serial_println!("[udp]   ephemeral port: OK (p1={}, p2={})", p1, p2);
    Ok(())
}

/// Test multicast group join, leave, and is_multicast_member.
fn test_multicast_join_leave() -> KernelResult<()> {
    let handle = bind(crate::netns::ROOT_NS, 55557)?;
    let mcast = Ipv4Addr([224, 0, 0, 251]); // mDNS multicast

    // Not a member before join.
    if is_multicast_member(mcast) {
        crate::serial_println!("[udp]   FAIL: member before join");
        close(handle);
        return Err(KernelError::InternalError);
    }

    join_group(handle, mcast)?;

    // Should be a member now.
    if !is_multicast_member(mcast) {
        crate::serial_println!("[udp]   FAIL: not member after join");
        close(handle);
        return Err(KernelError::InternalError);
    }

    // Double-join should fail.
    match join_group(handle, mcast) {
        Err(KernelError::AlreadyExists) => {} // Expected.
        other => {
            crate::serial_println!("[udp]   FAIL: double join didn't return AlreadyExists: {:?}", other);
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    // Non-multicast address should be rejected.
    match join_group(handle, Ipv4Addr([10, 0, 0, 1])) {
        Err(KernelError::InvalidArgument) => {} // Expected.
        other => {
            crate::serial_println!("[udp]   FAIL: unicast join didn't fail: {:?}", other);
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    leave_group(handle, mcast)?;

    // Should no longer be a member.
    if is_multicast_member(mcast) {
        crate::serial_println!("[udp]   FAIL: still member after leave");
        close(handle);
        return Err(KernelError::InternalError);
    }

    close(handle);

    crate::serial_println!("[udp]   multicast join/leave: OK");
    Ok(())
}

/// Test IPv6 multicast group join, leave, and is_multicast_member_v6.
fn test_multicast_join_leave_v6() -> KernelResult<()> {
    let handle = bind(crate::netns::ROOT_NS, 55570)?;

    // ff02::fb is the mDNS IPv6 multicast address.
    let mcast_v6 = Ipv6Addr([
        0xFF, 0x02, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0xFB,
    ]);

    // Not a member before join.
    if is_multicast_member_v6(mcast_v6) {
        crate::serial_println!("[udp]   FAIL: v6 member before join");
        close(handle);
        return Err(KernelError::InternalError);
    }

    join_group_v6(handle, mcast_v6)?;

    // Should be a member now.
    if !is_multicast_member_v6(mcast_v6) {
        crate::serial_println!("[udp]   FAIL: not v6 member after join");
        close(handle);
        return Err(KernelError::InternalError);
    }

    // Double-join should fail.
    match join_group_v6(handle, mcast_v6) {
        Err(KernelError::AlreadyExists) => {} // Expected.
        other => {
            crate::serial_println!("[udp]   FAIL: v6 double join: {:?}", other);
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    // Non-multicast IPv6 address should be rejected.
    let unicast_v6 = Ipv6Addr([
        0x20, 0x01, 0x0D, 0xB8, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 1,
    ]);
    match join_group_v6(handle, unicast_v6) {
        Err(KernelError::InvalidArgument) => {} // Expected.
        other => {
            crate::serial_println!("[udp]   FAIL: v6 unicast join: {:?}", other);
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    leave_group_v6(handle, mcast_v6)?;

    // Should no longer be a member.
    if is_multicast_member_v6(mcast_v6) {
        crate::serial_println!("[udp]   FAIL: still v6 member after leave");
        close(handle);
        return Err(KernelError::InternalError);
    }

    close(handle);

    crate::serial_println!("[udp]   multicast v6 join/leave: OK");
    Ok(())
}

/// Test rx_ready and rx_front_bytes on an empty socket.
fn test_rx_ready_empty() -> KernelResult<()> {
    let handle = bind(crate::netns::ROOT_NS, 55558)?;

    if rx_ready(handle) != 0 {
        crate::serial_println!("[udp]   FAIL: rx_ready non-zero on new socket");
        close(handle);
        return Err(KernelError::InternalError);
    }

    if rx_front_bytes(handle) != 0 {
        crate::serial_println!("[udp]   FAIL: rx_front_bytes non-zero on new socket");
        close(handle);
        return Err(KernelError::InternalError);
    }

    // recv on empty queue should return None.
    if recv(handle).is_some() {
        crate::serial_println!("[udp]   FAIL: recv returned data on empty socket");
        close(handle);
        return Err(KernelError::InternalError);
    }

    // Invalid handle should return 0.
    if rx_ready(99) != 0 {
        crate::serial_println!("[udp]   FAIL: rx_ready for invalid handle != 0");
        close(handle);
        return Err(KernelError::InternalError);
    }

    close(handle);
    crate::serial_println!("[udp]   rx_ready/rx_front_bytes empty: OK");
    Ok(())
}

/// Test connected mode (peer filter).
fn test_connected_mode() -> KernelResult<()> {
    let handle = bind(crate::netns::ROOT_NS, 55559)?;
    let peer_ip = Ipv4Addr([10, 0, 0, 1]);

    // Connect to a peer.
    connect(handle, peer_ip, 8080)?;

    // Disconnect.
    connect(handle, Ipv4Addr::UNSPECIFIED, 0)?;

    // Connect on invalid handle should fail.
    match connect(99, peer_ip, 8080) {
        Err(KernelError::InvalidArgument) => {} // Expected.
        other => {
            crate::serial_println!("[udp]   FAIL: connect on invalid handle: {:?}", other);
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    close(handle);
    crate::serial_println!("[udp]   connected mode: OK");
    Ok(())
}

/// Test that recv_v6 on an empty socket returns None.
fn test_v6_recv_empty() -> KernelResult<()> {
    let handle = bind(crate::netns::ROOT_NS, 55560)?;

    // IPv6 queue should be empty.
    if rx_ready_v6(handle) != 0 {
        crate::serial_println!("[udp]   FAIL: rx_ready_v6 non-zero on new socket");
        close(handle);
        return Err(KernelError::InternalError);
    }

    if recv_v6(handle).is_some() {
        crate::serial_println!("[udp]   FAIL: recv_v6 returned data on empty socket");
        close(handle);
        return Err(KernelError::InternalError);
    }

    if peek_v6(handle).is_some() {
        crate::serial_println!("[udp]   FAIL: peek_v6 returned data on empty socket");
        close(handle);
        return Err(KernelError::InternalError);
    }

    // Invalid handle.
    if rx_ready_v6(99) != 0 {
        crate::serial_println!("[udp]   FAIL: rx_ready_v6 for invalid handle != 0");
        close(handle);
        return Err(KernelError::InternalError);
    }

    close(handle);
    crate::serial_println!("[udp]   v6 recv empty: OK");
    Ok(())
}

/// Test process_udp_v6 delivery: build a valid IPv6 UDP packet and verify
/// that process_udp_v6 delivers it to a bound socket.
#[allow(clippy::arithmetic_side_effects)]
fn test_v6_process_and_deliver() -> KernelResult<()> {
    let dst_port: u16 = 55561;
    let handle = bind(crate::netns::ROOT_NS, dst_port)?;

    // Build a fake IPv6 + UDP packet.
    let src = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let dst = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
    let payload = b"hello ipv6 udp";
    let src_port: u16 = 12345;

    // Build UDP segment: src_port(2) + dst_port(2) + length(2) + cksum(2) + data.
    let udp_len = UDP_HEADER_SIZE + payload.len();
    let mut udp_seg = Vec::with_capacity(udp_len);
    udp_seg.extend_from_slice(&src_port.to_be_bytes());
    udp_seg.extend_from_slice(&dst_port.to_be_bytes());
    udp_seg.extend_from_slice(&(udp_len as u16).to_be_bytes());
    udp_seg.extend_from_slice(&0u16.to_be_bytes()); // Checksum placeholder.
    udp_seg.extend_from_slice(payload);

    // Compute IPv6 UDP checksum.
    let cksum = ipv6::compute_transport_checksum(&src, &dst, ipv6::NH_UDP, &udp_seg);
    udp_seg[6] = (cksum >> 8) as u8;
    udp_seg[7] = cksum as u8;

    // Wrap in an IPv6 packet.
    let ip_pkt = ipv6::build_packet(src, dst, ipv6::NH_UDP, 64, &udp_seg);
    let parsed = Ipv6Packet::parse(&ip_pkt)?;

    // Process — should deliver to our socket (root namespace).
    process_udp_v6(&parsed, crate::netns::ROOT_NS)?;

    // Verify delivery.
    if rx_ready_v6(handle) != 1 {
        crate::serial_println!(
            "[udp]   FAIL: rx_ready_v6 = {}, expected 1",
            rx_ready_v6(handle)
        );
        close(handle);
        return Err(KernelError::InternalError);
    }

    let dgram = recv_v6(handle);
    match dgram {
        Some(ref dg) => {
            if dg.src_ip != src {
                crate::serial_println!("[udp]   FAIL: v6 datagram src mismatch");
                close(handle);
                return Err(KernelError::InternalError);
            }
            if dg.src_port != src_port {
                crate::serial_println!(
                    "[udp]   FAIL: v6 datagram src_port = {}, expected {}",
                    dg.src_port, src_port
                );
                close(handle);
                return Err(KernelError::InternalError);
            }
            if dg.data.as_slice() != payload {
                crate::serial_println!(
                    "[udp]   FAIL: v6 datagram payload len = {}, expected {}",
                    dg.data.len(), payload.len()
                );
                close(handle);
                return Err(KernelError::InternalError);
            }
        }
        None => {
            crate::serial_println!("[udp]   FAIL: recv_v6 returned None after delivery");
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    // IPv4 queue should still be empty.
    if rx_ready(handle) != 0 {
        crate::serial_println!("[udp]   FAIL: IPv4 queue non-empty after v6 delivery");
        close(handle);
        return Err(KernelError::InternalError);
    }

    close(handle);
    crate::serial_println!("[udp]   v6 process + deliver: OK");
    Ok(())
}

/// Test 10: namespace isolation — same port can be bound in different
/// namespaces, and datagrams are delivered only to sockets in the arrival
/// namespace (root being permissive).
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
fn test_namespace_isolation() -> KernelResult<()> {
    let ns0 = crate::netns::ROOT_NS;
    let ns1: NetNsId = 77; // Fake non-root namespace.

    // Bind port 55580 in namespace 0.
    let h0 = bind(ns0, 55580)?;

    // Same port in a different namespace should succeed.
    let h1 = bind(ns1, 55580)?;

    // Duplicate in the same namespace should still be rejected.
    match bind(ns0, 55580) {
        Err(KernelError::AlreadyExists) => {}
        other => {
            close(h0);
            close(h1);
            crate::serial_println!(
                "[udp]   FAIL: duplicate bind in same NS returned {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    // Delivery-level scoping: a datagram arriving in ns1 must reach only
    // the socket bound in ns1, not the same-port socket in ns0.  A helper
    // builds a valid UDP-over-IPv4 datagram for port 55580.
    let build_dgram = |src: super::interface::Ipv4Addr,
                       dst: super::interface::Ipv4Addr,
                       src_port: u16| -> Vec<u8> {
        let payload = b"ns-scoped";
        let udp_len = UDP_HEADER_SIZE + payload.len();
        let mut seg = Vec::with_capacity(udp_len);
        seg.extend_from_slice(&src_port.to_be_bytes());
        seg.extend_from_slice(&55580u16.to_be_bytes());
        seg.extend_from_slice(&(udp_len as u16).to_be_bytes());
        seg.extend_from_slice(&0u16.to_be_bytes()); // Checksum placeholder.
        seg.extend_from_slice(payload);
        let cksum = ipv4::compute_transport_checksum(src, dst, PROTO_UDP, &seg);
        seg[6] = (cksum >> 8) as u8;
        seg[7] = cksum as u8;
        ipv4::build_packet(src, dst, PROTO_UDP, &seg)
    };

    let src_ip = super::interface::Ipv4Addr([10, 0, 0, 1]);
    let dst_ip = super::interface::Ipv4Addr([10, 0, 0, 2]);

    // Arriving in ns1 → only h1 receives.
    let pkt_ns1 = build_dgram(src_ip, dst_ip, 40001);
    let parsed_ns1 = Ipv4Packet::parse(&pkt_ns1)?;
    process_udp(&parsed_ns1, ns1)?;
    if rx_ready(h1) != 1 || rx_ready(h0) != 0 {
        crate::serial_println!(
            "[udp]   FAIL: ns1 delivery — h1={}, h0={} (want 1, 0)",
            rx_ready(h1), rx_ready(h0)
        );
        close(h0);
        close(h1);
        return Err(KernelError::InternalError);
    }
    let _ = recv(h1); // Drain.

    // Arriving in the root namespace → permissive: reaches h0 (first
    // matching socket by port).
    let pkt_root = build_dgram(src_ip, dst_ip, 40002);
    let parsed_root = Ipv4Packet::parse(&pkt_root)?;
    process_udp(&parsed_root, ns0)?;
    if rx_ready(h0) != 1 {
        crate::serial_println!(
            "[udp]   FAIL: root delivery — h0={} (want 1)",
            rx_ready(h0)
        );
        close(h0);
        close(h1);
        return Err(KernelError::InternalError);
    }
    let _ = recv(h0); // Drain.

    close(h0);
    close(h1);

    crate::serial_println!("[udp]   Namespace isolation: OK");
    Ok(())
}
