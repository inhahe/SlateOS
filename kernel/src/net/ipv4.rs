//! IPv4 packet parsing and construction.
//!
//! Handles basic IPv4 packets (RFC 791).  No fragmentation support —
//! all packets must fit in a single Ethernet frame.
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

/// Send an IPv4 packet.
///
/// Resolves the next-hop MAC via ARP (or uses broadcast for broadcast
/// addresses), wraps in an Ethernet frame, and sends via the NIC.
pub fn send(dst: Ipv4Addr, protocol: u8, payload: &[u8]) -> KernelResult<()> {
    let our_mac = interface::mac();
    let our_ip = interface::ip();

    // Build the IP packet.
    let ip_packet = build_packet(our_ip, dst, protocol, payload);

    // Determine the next-hop MAC address.
    let dst_mac = if dst.is_broadcast() {
        ethernet::BROADCAST_MAC
    } else {
        // Check if on the same subnet; if not, route via gateway.
        let info = interface::info();
        let next_hop = if !our_ip.is_unspecified()
            && !info.gateway.is_unspecified()
            && !our_ip.same_subnet(dst, info.subnet_mask)
        {
            info.gateway
        } else {
            dst
        };

        super::arp::resolve(next_hop)?
    };

    // Wrap in Ethernet frame and send.
    let frame = ethernet::build_frame(&dst_mac, &our_mac, ETHERTYPE_IPV4, &ip_packet);

    crate::virtio::net::with_device(|dev| dev.send(&frame))
        .unwrap_or(Err(KernelError::NoSuchDevice))?;

    Ok(())
}
