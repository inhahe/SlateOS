//! IPv4 packet parsing and construction.
//!
//! Handles basic IPv4 packets (RFC 791).  No fragmentation support —
//! all packets must fit in a single Ethernet frame.
//!
//! ## Namespace integration
//!
//! `send_ns()` sends packets within a specific network namespace,
//! using the namespace's IP address as source and its routing table
//! for next-hop determination.  `send()` is a convenience wrapper
//! that sends via the root namespace (the physical NIC).
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
        })
    }
}

// ---------------------------------------------------------------------------
// IPv4 packet construction
// ---------------------------------------------------------------------------

/// Build an IPv4 packet.
///
/// Returns the raw packet bytes (header + payload).
/// Computes the IP header checksum.
#[allow(clippy::arithmetic_side_effects)]
pub fn build_packet(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    protocol: u8,
    payload: &[u8],
) -> Vec<u8> {
    let total_len = IPV4_HEADER_SIZE + payload.len();
    let mut pkt = Vec::with_capacity(total_len);

    // Version (4) + IHL (5 = 20 bytes, no options).
    pkt.push(0x45);
    // DSCP + ECN.
    pkt.push(0);
    // Total length.
    pkt.extend_from_slice(&(total_len as u16).to_be_bytes());
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

// ---------------------------------------------------------------------------
// IPv4 processing
// ---------------------------------------------------------------------------

/// Process an incoming IPv4 packet.
pub fn process_ipv4(data: &[u8]) -> KernelResult<()> {
    let packet = Ipv4Packet::parse(data)?;

    // Check if the packet is addressed to us or is broadcast.
    let our_ip = interface::ip();
    let is_for_us = packet.dst == our_ip
        || packet.dst.is_broadcast()
        || our_ip.is_unspecified(); // Accept all during DHCP.

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
/// routing table for next-hop gateway determination.  The physical
/// NIC's MAC address and ARP cache are shared across all namespaces
/// (since virtual ethernet pairs are not yet implemented).
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
    // Firewall outbound check — global for now (per-namespace firewall
    // is future work; netns.rs documents this as "future").
    if !super::firewall::check_outbound(protocol, dst, payload) {
        return Err(KernelError::PermissionDenied);
    }

    // MAC always comes from the physical NIC (shared across namespaces).
    let our_mac = interface::mac();

    // Source IP comes from the namespace's interface configuration.
    let our_ip = interface::ns_ip(ns_id);

    // Build the IP packet with the namespace's source address.
    let ip_packet = build_packet(our_ip, dst, protocol, payload);

    // Determine the next-hop MAC address.
    let dst_mac = if dst.is_broadcast() {
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
