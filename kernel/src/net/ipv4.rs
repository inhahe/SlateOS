//! IPv4 packet parsing and construction.
//!
//! Handles basic IPv4 packets (RFC 791).  No fragmentation support —
//! all packets must fit in a single Ethernet frame.
//!
//! ## Namespace integration
//!
//! `send_ns()` sends packets within a specific network namespace,
//! using the namespace's IP address as source, its routing table
//! for next-hop determination, and its per-namespace firewall for
//! outbound filtering.  `send()` is a convenience wrapper that
//! sends via the root namespace (the physical NIC).
//!
//! Incoming packets (`process_ipv4`) always arrive on the physical
//! NIC and are dispatched to the root namespace.  Per-namespace
//! inbound routing will require virtual ethernet pairs (future work).
//!
//! ## Header format (20 bytes minimum)
//!
//! ```text
//!  0                   1                   2                   3
//!  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |Version|  IHL  |    DSCP/ECN   |         Total Length          |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |         Identification        |Flags|     Fragment Offset     |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |  Time to Live |    Protocol   |       Header Checksum         |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                       Source Address                          |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                    Destination Address                        |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

use super::ethernet::{self, ETHERTYPE_IPV4};
use super::interface::{self, Ipv4Addr};

// ---------------------------------------------------------------------------
// Protocol numbers
// ---------------------------------------------------------------------------

/// IP protocol: ICMP.
pub const PROTO_ICMP: u8 = 1;
/// IP protocol: UDP.
pub const PROTO_UDP: u8 = 17;
/// IP protocol: TCP.
pub const PROTO_TCP: u8 = 6;

/// Minimum IPv4 header size (no options).
const IPV4_HEADER_SIZE: usize = 20;

/// Default TTL for outgoing packets.
const DEFAULT_TTL: u8 = 64;

// ---------------------------------------------------------------------------
// IPv4 packet parsing
// ---------------------------------------------------------------------------

/// A parsed IPv4 packet header.
pub struct Ipv4Packet<'a> {
    /// IP version (should be 4).
    pub version: u8,
    /// Header length in 32-bit words.
    pub ihl: u8,
    /// Total length of the packet (header + payload).
    pub total_length: u16,
    /// Time to live.
    pub ttl: u8,
    /// Protocol number (6=TCP, 17=UDP, 1=ICMP).
    pub protocol: u8,
    /// Source IP address.
    pub src: Ipv4Addr,
    /// Destination IP address.
    pub dst: Ipv4Addr,
    /// Payload (after IP header).
    pub payload: &'a [u8],
    /// Raw IP header bytes (for ICMP error generation).
    ///
    /// RFC 792 requires ICMP error messages to include the original IP
    /// header + first 8 bytes of the triggering packet.  Storing a
    /// reference to the raw header avoids reconstructing it later.
    pub raw_header: &'a [u8],
}

impl<'a> Ipv4Packet<'a> {
    /// Parse an IPv4 packet from raw bytes.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn parse(data: &'a [u8]) -> KernelResult<Self> {
        if data.len() < IPV4_HEADER_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        let version = data[0] >> 4;
        let ihl = data[0] & 0x0F;

        if version != 4 {
            return Err(KernelError::InvalidArgument);
        }

        let header_len = (ihl as usize) * 4;
        if header_len < IPV4_HEADER_SIZE || data.len() < header_len {
            return Err(KernelError::InvalidArgument);
        }

        // Verify IP header checksum.  The checksum covers only the
        // IP header (not the payload).  A correct checksum folds to
        // zero when computed over the header including the checksum
        // field (one's complement property, RFC 1071).
        if ip_checksum(&data[..header_len]) != 0 {
            return Err(KernelError::InvalidArgument);
        }

        let total_length = u16::from_be_bytes([data[2], data[3]]);
        let ttl = data[8];
        let protocol = data[9];

        let mut src = [0u8; 4];
        let mut dst = [0u8; 4];
        src.copy_from_slice(&data[12..16]);
        dst.copy_from_slice(&data[16..20]);

        let payload_end = (total_length as usize).min(data.len());
        let payload = if header_len < payload_end {
            &data[header_len..payload_end]
        } else {
            &[]
        };

        Ok(Self {
            version,
            ihl,
            total_length,
            ttl,
            protocol,
            src: Ipv4Addr(src),
            dst: Ipv4Addr(dst),
            payload,
            raw_header: &data[..header_len],
        })
    }
}

// ---------------------------------------------------------------------------
// IPv4 packet construction
// ---------------------------------------------------------------------------

/// Build an IPv4 packet.
///
/// Returns the raw packet bytes (header + payload), or an error if
/// the payload is too large to fit in a single IPv4 packet (max
/// 65515 bytes, since the 16-bit total length field includes the
/// 20-byte header).
///
/// Computes the IP header checksum.
#[allow(clippy::arithmetic_side_effects)]
pub fn build_packet(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    protocol: u8,
    payload: &[u8],
) -> Vec<u8> {
    let total_len = IPV4_HEADER_SIZE + payload.len();

    // IPv4 total length is a 16-bit field.  Silently truncating would
    // produce a corrupt packet header, so clamp to the maximum.  In
    // practice this is unreachable because our MTU is 1500 and we
    // don't support IP fragmentation, but defense-in-depth matters.
    let total_len_u16 = u16::try_from(total_len).unwrap_or(u16::MAX);

    let mut pkt = Vec::with_capacity(total_len);

    // Version (4) + IHL (5 = 20 bytes, no options).
    pkt.push(0x45);
    // DSCP + ECN.
    pkt.push(0);
    // Total length.
    pkt.extend_from_slice(&total_len_u16.to_be_bytes());
    // Identification (0 for now — no fragmentation).
    pkt.extend_from_slice(&0u16.to_be_bytes());
    // Flags (Don't Fragment) + Fragment Offset.
    pkt.extend_from_slice(&0x4000u16.to_be_bytes()); // DF bit set.
    // TTL.
    pkt.push(DEFAULT_TTL);
    // Protocol.
    pkt.push(protocol);
    // Checksum placeholder (2 bytes, will be filled in).
    let checksum_offset = pkt.len();
    pkt.extend_from_slice(&[0, 0]);
    // Source address.
    pkt.extend_from_slice(&src.0);
    // Destination address.
    pkt.extend_from_slice(&dst.0);

    // Compute IP header checksum over the 20-byte header.
    let checksum = ip_checksum(&pkt[..IPV4_HEADER_SIZE]);
    pkt[checksum_offset] = (checksum >> 8) as u8;
    pkt[checksum_offset + 1] = checksum as u8;

    // Append payload.
    pkt.extend_from_slice(payload);

    pkt
}

/// Compute the Internet checksum (RFC 1071) over a byte slice.
///
/// Returns the checksum in network byte order.
#[allow(clippy::arithmetic_side_effects)]
pub fn ip_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    // Sum 16-bit words.
    let mut i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum = sum.wrapping_add(u32::from(word));
        i += 2;
    }

    // Handle odd byte.
    if i < data.len() {
        sum = sum.wrapping_add(u32::from(data[i]) << 8);
    }

    // Fold 32-bit sum into 16 bits.
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF).wrapping_add(sum >> 16);
    }

    !sum as u16
}

/// Verify a TCP or UDP checksum using the IPv4 pseudo-header (RFC 793/768).
///
/// The transport-layer checksum covers:
/// 1. A pseudo-header: source IP, destination IP, zero byte, protocol, segment length
/// 2. The full transport segment (header + payload)
///
/// Returns `true` if the checksum is valid (folds to zero) or if the
/// checksum field is zero (valid for UDP over IPv4, meaning "no checksum").
///
/// `segment` is the full transport-layer data (TCP/UDP header + payload).
/// `protocol` is the IP protocol number (6 = TCP, 17 = UDP).
#[allow(clippy::arithmetic_side_effects)]
pub fn verify_transport_checksum(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    protocol: u8,
    segment: &[u8],
) -> bool {
    // UDP with checksum field = 0 means "no checksum" (valid per RFC 768).
    if protocol == PROTO_UDP && segment.len() >= 8 {
        let cksum_field = u16::from_be_bytes([segment[6], segment[7]]);
        if cksum_field == 0 {
            return true;
        }
    }

    // Build pseudo-header + segment and compute checksum.
    let seg_len = segment.len() as u16;
    let mut sum: u32 = 0;

    // Pseudo-header: src IP (4 bytes as two 16-bit words).
    sum = sum.wrapping_add(u32::from(u16::from_be_bytes([src.0[0], src.0[1]])));
    sum = sum.wrapping_add(u32::from(u16::from_be_bytes([src.0[2], src.0[3]])));
    // Pseudo-header: dst IP.
    sum = sum.wrapping_add(u32::from(u16::from_be_bytes([dst.0[0], dst.0[1]])));
    sum = sum.wrapping_add(u32::from(u16::from_be_bytes([dst.0[2], dst.0[3]])));
    // Pseudo-header: zero + protocol.
    sum = sum.wrapping_add(u32::from(protocol));
    // Pseudo-header: segment length.
    sum = sum.wrapping_add(u32::from(seg_len));

    // Sum the segment itself (16-bit words).
    let mut i = 0;
    while i + 1 < segment.len() {
        let word = u16::from_be_bytes([segment[i], segment[i + 1]]);
        sum = sum.wrapping_add(u32::from(word));
        i += 2;
    }
    // Handle odd trailing byte.
    if i < segment.len() {
        sum = sum.wrapping_add(u32::from(segment[i]) << 8);
    }

    // Fold 32-bit sum into 16 bits.
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF).wrapping_add(sum >> 16);
    }

    // Valid checksum folds to 0xFFFF (since the checksum field is
    // included in the computation, the complement is zero).
    sum == 0xFFFF
}

/// Compute a TCP/UDP checksum using the IPv4 pseudo-header (RFC 793/768).
///
/// The checksum covers:
/// 1. Pseudo-header: source IP, destination IP, zero byte, protocol, segment length
/// 2. The full transport segment (header + payload)
///
/// The checksum field within the segment MUST be zeroed before calling this.
/// Returns the 16-bit one's complement checksum (ready to write into the header).
/// A return value of 0x0000 is replaced with 0xFFFF per RFC 768 (UDP).
#[allow(clippy::arithmetic_side_effects)]
pub fn compute_transport_checksum(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    protocol: u8,
    segment: &[u8],
) -> u16 {
    let seg_len = segment.len() as u16;
    let mut sum: u32 = 0;

    // Pseudo-header: src IP.
    sum = sum.wrapping_add(u32::from(u16::from_be_bytes([src.0[0], src.0[1]])));
    sum = sum.wrapping_add(u32::from(u16::from_be_bytes([src.0[2], src.0[3]])));
    // Pseudo-header: dst IP.
    sum = sum.wrapping_add(u32::from(u16::from_be_bytes([dst.0[0], dst.0[1]])));
    sum = sum.wrapping_add(u32::from(u16::from_be_bytes([dst.0[2], dst.0[3]])));
    // Pseudo-header: zero + protocol.
    sum = sum.wrapping_add(u32::from(protocol));
    // Pseudo-header: segment length.
    sum = sum.wrapping_add(u32::from(seg_len));

    // Sum the segment (16-bit words).
    let mut i = 0;
    while i + 1 < segment.len() {
        let word = u16::from_be_bytes([segment[i], segment[i + 1]]);
        sum = sum.wrapping_add(u32::from(word));
        i += 2;
    }
    // Handle odd trailing byte.
    if i < segment.len() {
        sum = sum.wrapping_add(u32::from(segment[i]) << 8);
    }

    // Fold 32-bit sum into 16 bits.
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF).wrapping_add(sum >> 16);
    }

    let cksum = !sum as u16;

    // For UDP, a computed checksum of 0x0000 is transmitted as 0xFFFF
    // (RFC 768: 0x0000 means "no checksum").
    if protocol == PROTO_UDP && cksum == 0 {
        0xFFFF
    } else {
        cksum
    }
}

/// Check if `addr` is the subnet-directed broadcast for the given IP/mask.
///
/// The subnet broadcast address has all host bits set to 1.
/// E.g., for IP 192.168.1.x with mask 255.255.255.0: broadcast = 192.168.1.255.
fn is_subnet_broadcast(addr: Ipv4Addr, our_ip: Ipv4Addr, mask: Ipv4Addr) -> bool {
    let m = mask.to_u32();
    let net = our_ip.to_u32() & m;
    let host_bits = !m;
    // Subnet broadcast has all host bits set.
    addr.to_u32() == (net | host_bits)
}

// ---------------------------------------------------------------------------
// IPv4 processing
// ---------------------------------------------------------------------------

/// Process an incoming IPv4 packet.
pub fn process_ipv4(data: &[u8]) -> KernelResult<()> {
    let packet = Ipv4Packet::parse(data)?;

    // Reject packets with invalid source IPs — these violate protocol
    // rules and could pollute ARP caches or connection state.
    // RFC 1122 §3.2.1.3: source must not be broadcast or multicast.
    if packet.src.is_broadcast() || packet.src.is_multicast() {
        return Ok(()); // Silently drop.
    }

    // Check if the packet is addressed to us, is broadcast, or is
    // multicast for a group we have joined.
    let iface = interface::info();
    let is_for_us = packet.dst == iface.ip
        || packet.dst.is_broadcast()
        || iface.ip.is_unspecified() // Accept all during DHCP.
        || (packet.dst.is_multicast()
            && super::udp::is_multicast_member(packet.dst))
        // Subnet-directed broadcast (e.g., 192.168.1.255 for /24).
        || (!iface.subnet_mask.is_unspecified()
            && is_subnet_broadcast(packet.dst, iface.ip, iface.subnet_mask));

    if !is_for_us {
        return Ok(());
    }

    // Firewall inbound check — drop packet if denied.
    if !super::firewall::check_inbound(packet.protocol, packet.src, packet.payload) {
        return Ok(()); // Silently dropped by firewall.
    }

    match packet.protocol {
        PROTO_TCP => super::tcp::process_tcp(&packet),
        PROTO_UDP => super::udp::process_udp(&packet),
        PROTO_ICMP => super::icmp::process_icmp(&packet),
        _ => {
            // Unknown protocol — drop.
            Ok(())
        }
    }
}

/// Send an IPv4 packet via the root network namespace.
///
/// Convenience wrapper around `send_ns()` that uses the root
/// namespace (the physical NIC's IP and routing configuration).
///
/// Resolves the next-hop MAC via ARP (or uses broadcast for broadcast
/// addresses), wraps in an Ethernet frame, and sends via the NIC.
pub fn send(dst: Ipv4Addr, protocol: u8, payload: &[u8]) -> KernelResult<()> {
    send_ns(crate::netns::ROOT_NS, dst, protocol, payload)
}

/// Send an IPv4 packet within a specific network namespace.
///
/// Uses the namespace's IP address as the source address and its
/// routing table for next-hop gateway determination.  Checks the
/// namespace's firewall before sending (root uses the global firewall;
/// child namespaces use per-namespace firewall state).
///
/// The physical NIC's MAC address and ARP cache are shared across all
/// namespaces (since virtual ethernet pairs are not yet implemented).
///
/// # Parameters
///
/// - `ns_id`: Network namespace ID.  Use `ROOT_NS` (0) for the
///   physical NIC's namespace.
/// - `dst`: Destination IPv4 address.
/// - `protocol`: IP protocol number (e.g., `PROTO_TCP`, `PROTO_UDP`).
/// - `payload`: Protocol payload (e.g., TCP/UDP segment).
///
/// # Errors
///
/// - [`KernelError::PermissionDenied`] if the firewall blocks the packet.
/// - [`KernelError::TimedOut`] if ARP resolution fails.
/// - [`KernelError::NoSuchDevice`] if no NIC is available.
pub fn send_ns(
    ns_id: crate::netns::NetNsId,
    dst: Ipv4Addr,
    protocol: u8,
    payload: &[u8],
) -> KernelResult<()> {
    // Namespace-aware firewall outbound check.
    // Root namespace (0) uses the global firewall; child namespaces use
    // their own per-namespace firewall state.
    if !super::firewall::check_outbound_ns(ns_id, protocol, dst, payload) {
        return Err(KernelError::PermissionDenied);
    }

    // MAC always comes from the physical NIC (shared across namespaces).
    let our_mac = interface::mac();

    // Source IP comes from the namespace's interface configuration.
    let our_ip = interface::ns_ip(ns_id);

    // Build the IP packet with the namespace's source address.
    let ip_packet = build_packet(our_ip, dst, protocol, payload);

    // Determine the next-hop MAC address.
    let iface_info = interface::info();
    let dst_mac = if dst.is_broadcast()
        || (!iface_info.subnet_mask.is_unspecified()
            && is_subnet_broadcast(dst, iface_info.ip, iface_info.subnet_mask))
    {
        ethernet::BROADCAST_MAC
    } else {
        let next_hop = resolve_next_hop(ns_id, our_ip, dst);
        super::arp::resolve(next_hop)?
    };

    // Wrap in Ethernet frame and send via the active NIC.
    let frame = ethernet::build_frame(&dst_mac, &our_mac, ETHERTYPE_IPV4, &ip_packet);

    super::send_frame(&frame)?;

    Ok(())
}

/// Determine the next-hop IP for a destination within a namespace.
///
/// For the root namespace, uses the global interface configuration
/// (same-subnet → direct, cross-subnet → gateway).  For child
/// namespaces, consults the namespace's routing table via
/// `netns::route_lookup()` and falls back to the namespace's default
/// gateway.
fn resolve_next_hop(
    ns_id: crate::netns::NetNsId,
    our_ip: Ipv4Addr,
    dst: Ipv4Addr,
) -> Ipv4Addr {
    if ns_id != crate::netns::ROOT_NS {
        // Non-root namespace: use the per-namespace routing table.
        if let Some(gw) = crate::netns::route_lookup(
            ns_id,
            crate::netns::Ipv4Addr(dst.0),
        ) {
            return if gw == crate::netns::Ipv4Addr::UNSPECIFIED {
                dst // Direct delivery (connected route).
            } else {
                Ipv4Addr(gw.0) // Route via gateway.
            };
        }

        // No matching route — try the namespace's configured gateway.
        let ns = interface::ns_info(ns_id);
        if !ns.gateway.is_unspecified() {
            return ns.gateway;
        }

        // No gateway either — attempt direct delivery.
        return dst;
    }

    // Root namespace: use the global interface configuration.
    let info = interface::info();
    if !our_ip.is_unspecified()
        && !info.gateway.is_unspecified()
        && !our_ip.same_subnet(dst, info.subnet_mask)
    {
        info.gateway
    } else {
        dst
    }
}
