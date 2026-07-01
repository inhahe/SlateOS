//! IPv4 packet parsing and construction.
//!
//! Handles IPv4 packets (RFC 791) with fragmentation.
//!
//! - **Incoming**: fragmented packets are reassembled via the `frag` module.
//! - **Outgoing (TCP)**: always sets DF bit (TCP uses MSS to avoid
//!   fragmentation, Path MTU Discovery relies on DF).
//! - **Outgoing (UDP/other)**: uses `send_fragmentable()` which splits
//!   oversized datagrams into properly sequenced fragments (RFC 791 §2.3).
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
use core::sync::atomic::{AtomicU16, Ordering};

use crate::error::{KernelError, KernelResult};

use super::ethernet::{self, ETHERTYPE_IPV4};
use super::interface::{self, Ipv4Addr};
use crate::virtio::net::MacAddress;

/// Global IP identification counter for fragmented packets.
/// Incremented for each new datagram that requires fragmentation.
static IP_ID_COUNTER: AtomicU16 = AtomicU16::new(1);

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

/// Maximum Transmission Unit (Ethernet default).
const MTU: usize = 1500;

/// Maximum payload per IP fragment (must be multiple of 8).
/// MTU (1500) - IP header (20) = 1480, which is divisible by 8.
const MAX_FRAGMENT_PAYLOAD: usize = MTU - IPV4_HEADER_SIZE;

// ---------------------------------------------------------------------------
// IPv4 packet parsing
// ---------------------------------------------------------------------------

/// A parsed IPv4 packet header.
#[allow(dead_code)] // Spec-defined fields.
pub struct Ipv4Packet<'a> {
    /// IP version (should be 4).
    pub version: u8,
    /// Header length in 32-bit words.
    pub ihl: u8,
    /// Total length of the packet (header + payload).
    pub total_length: u16,
    /// Identification field — used to group fragments of the same datagram.
    pub identification: u16,
    /// Raw flags + fragment offset word (network byte order, 16 bits).
    ///
    /// Layout: `[0][DF][MF][Fragment Offset (13 bits)]`.
    /// Fragment offset is in 8-byte units.
    flags_frag: u16,
    /// ECN field (2 low bits of the DSCP/ECN byte, RFC 3168).
    ///
    /// - `0b00` (NotECT): Not ECN-Capable Transport.
    /// - `0b01` (ECT(1)): ECN-capable, codepoint 1.
    /// - `0b10` (ECT(0)): ECN-capable, codepoint 0.
    /// - `0b11` (CE): Congestion Experienced.
    pub ecn: u8,
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

/// ECN codepoint: Congestion Experienced (CE).  Set by routers when
/// their queues are filling up, as an alternative to dropping packets.
pub const ECN_CE: u8 = 0b11;

/// ECN codepoint: ECN-Capable Transport, codepoint 0.
pub const ECN_ECT0: u8 = 0b10;

/// ECN codepoint: ECN-Capable Transport, codepoint 1.
#[allow(dead_code)] // Used for reference; we send ECT(0).
pub const ECN_ECT1: u8 = 0b01;

impl Ipv4Packet<'_> {
    /// More Fragments flag — `true` if this is not the last fragment.
    pub fn more_fragments(&self) -> bool {
        (self.flags_frag >> 13) & 1 != 0
    }

    /// Fragment offset in 8-byte units (13-bit field).
    pub fn fragment_offset(&self) -> u16 {
        self.flags_frag & 0x1FFF
    }

    /// Whether this packet is a fragment (MF set or offset non-zero).
    ///
    /// A non-fragmented packet has MF = 0 and offset = 0.
    pub fn is_fragment(&self) -> bool {
        self.more_fragments() || self.fragment_offset() != 0
    }
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
        // ECN is the low 2 bits of the DSCP/ECN byte (byte 1).
        let ecn = data[1] & 0x03;
        let identification = u16::from_be_bytes([data[4], data[5]]);
        let flags_frag = u16::from_be_bytes([data[6], data[7]]);
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
            identification,
            flags_frag,
            ecn,
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

/// Build an IPv4 packet (ECN field set to 0 — not ECN-capable).
///
/// Returns the raw packet bytes (header + payload), or an error if
/// the payload is too large to fit in a single IPv4 packet (max
/// 65515 bytes, since the 16-bit total length field includes the
/// 20-byte header).
///
/// Computes the IP header checksum.
#[allow(dead_code)] // Public API.
pub fn build_packet(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    protocol: u8,
    payload: &[u8],
) -> Vec<u8> {
    build_packet_ecn(src, dst, protocol, payload, 0)
}

/// Build an IPv4 packet with an explicit ECN codepoint.
///
/// `ecn` is the 2-bit ECN field (0 = Not-ECT, 1 = ECT(1), 2 = ECT(0),
/// 3 = CE).  For TCP with ECN negotiated, use `ECN_ECT0` (2).
#[allow(clippy::arithmetic_side_effects)]
pub fn build_packet_ecn(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    protocol: u8,
    payload: &[u8],
    ecn: u8,
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
    // DSCP (0) + ECN (low 2 bits).
    pkt.push(ecn & 0x03);
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

/// Process an incoming IPv4 packet received in a given network namespace.
///
/// `ns_id` is the namespace the packet arrived in (`ROOT_NS` for the
/// physical NIC, or a container namespace for veth-delivered frames).  It
/// selects which interface's address is used for the "addressed to us"
/// check and is threaded into the transport handlers so socket lookup is
/// scoped to the correct namespace.
pub fn process_ipv4(data: &[u8], ns_id: crate::netns::NetNsId) -> KernelResult<()> {
    let packet = Ipv4Packet::parse(data)?;

    // Reject packets with invalid source IPs — these violate protocol
    // rules and could pollute ARP caches or connection state.
    // RFC 1122 §3.2.1.3: source must not be broadcast or multicast.
    if packet.src.is_broadcast() || packet.src.is_multicast() {
        return Ok(()); // Silently drop.
    }

    // Check if the packet is addressed to us, is broadcast, or is
    // multicast for a group we have joined.  The interface identity is
    // namespace-specific (the physical NIC in the root namespace, or the
    // veth endpoint's address in a container namespace).
    let iface = interface::ns_info(ns_id);
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
    // Note: for fragmented packets, the firewall checks each fragment
    // individually.  The first fragment (offset 0) contains the transport
    // header and is filtered normally.  Subsequent fragments lack the
    // transport header, so port-based rules won't match — they pass
    // through.  The reassembled datagram is not re-checked (the first
    // fragment's check is the authoritative decision).
    if !super::firewall::check_inbound_ns(ns_id, packet.protocol, packet.src, packet.payload) {
        return Ok(()); // Silently dropped by firewall.
    }

    // Handle fragmented packets: route to the reassembly module.
    if packet.is_fragment() {
        if let Some(reassembled) = super::frag::add_fragment(
            packet.src,
            packet.dst,
            packet.identification,
            packet.protocol,
            packet.fragment_offset(),
            packet.more_fragments(),
            packet.payload,
        ) {
            // Reassembly complete — dispatch the full datagram.
            // Build a temporary Ipv4Packet-like struct to pass to
            // protocol handlers.
            return dispatch_reassembled(&reassembled, ns_id);
        }
        return Ok(()); // Waiting for more fragments.
    }

    match packet.protocol {
        PROTO_TCP => super::tcp::process_tcp(&packet, ns_id),
        PROTO_UDP => super::udp::process_udp(&packet, ns_id),
        PROTO_ICMP => super::icmp::process_icmp(&packet, ns_id),
        super::igmp::PROTO_IGMP => super::igmp::process(&packet, packet.payload),
        _ => {
            // Unknown protocol — drop.
            Ok(())
        }
    }
}

/// Dispatch a reassembled datagram to the appropriate transport handler.
///
/// After fragment reassembly completes, we have the complete transport
/// payload but no longer have the original IP header bytes.  We create
/// a synthetic `Ipv4Packet` with a minimal header for protocol handlers
/// that need IP-level metadata (e.g., `process_tcp` uses `src`/`dst`).
fn dispatch_reassembled(
    pkt: &super::frag::ReassembledPacket,
    ns_id: crate::netns::NetNsId,
) -> KernelResult<()> {
    // Build a minimal fake raw header for handlers that reference it
    // (e.g., ICMP error generation).  20 bytes, all zeros except
    // version/IHL, total length, protocol, and addresses.
    let total_len = (20u16).saturating_add(pkt.payload.len() as u16);
    let mut fake_hdr = [0u8; 20];
    fake_hdr[0] = 0x45; // version=4, IHL=5
    fake_hdr[2] = (total_len >> 8) as u8;
    fake_hdr[3] = total_len as u8;
    fake_hdr[9] = pkt.protocol;
    fake_hdr[12..16].copy_from_slice(&pkt.src.0);
    fake_hdr[16..20].copy_from_slice(&pkt.dst.0);

    let ip_pkt = Ipv4Packet {
        version: 4,
        ihl: 5,
        total_length: total_len,
        identification: 0,
        flags_frag: 0,
        ecn: 0, // ECN not tracked through reassembly.
        ttl: 0,
        protocol: pkt.protocol,
        src: pkt.src,
        dst: pkt.dst,
        payload: &pkt.payload,
        raw_header: &fake_hdr,
    };

    match pkt.protocol {
        PROTO_TCP => super::tcp::process_tcp(&ip_pkt, ns_id),
        PROTO_UDP => super::udp::process_udp(&ip_pkt, ns_id),
        PROTO_ICMP => super::icmp::process_icmp(&ip_pkt, ns_id),
        super::igmp::PROTO_IGMP => super::igmp::process(&ip_pkt, ip_pkt.payload),
        _ => Ok(()),
    }
}

/// Compute the Ethernet multicast MAC for an IPv4 multicast address (RFC 1112).
///
/// The low 23 bits of the IPv4 multicast address are mapped into the
/// Ethernet MAC `01:00:5E:<IP[1]&0x7F>:<IP[2]>:<IP[3]>`.  This covers
/// all standard IPv4 multicast (224.0.0.0/4) including mDNS, IGMP, etc.
fn multicast_mac(ip: Ipv4Addr) -> MacAddress {
    MacAddress([0x01, 0x00, 0x5E, ip.0[1] & 0x7F, ip.0[2], ip.0[3]])
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

/// Send an IPv4 packet with a custom TTL (for traceroute).
///
/// Like [`send`], but uses the given TTL instead of the default 64.
pub fn send_with_ttl(dst: Ipv4Addr, protocol: u8, payload: &[u8], ttl: u8) -> KernelResult<()> {
    // Namespace-aware firewall outbound check.
    let ns_id = crate::netns::ROOT_NS;
    if !super::firewall::check_outbound_ns(ns_id, protocol, dst, payload) {
        return Err(KernelError::PermissionDenied);
    }

    let our_mac = interface::mac();
    let our_ip = interface::ns_ip(ns_id);

    // Build packet with custom TTL.
    let ip_packet = build_packet_custom_ttl(our_ip, dst, protocol, payload, ttl);

    // Determine the next-hop MAC address.
    let iface_info = interface::info();
    let dst_mac = if dst.is_broadcast()
        || (!iface_info.subnet_mask.is_unspecified()
            && is_subnet_broadcast(dst, iface_info.ip, iface_info.subnet_mask))
    {
        ethernet::BROADCAST_MAC
    } else if dst.is_multicast() {
        // RFC 1112: IPv4 multicast → Ethernet multicast MAC mapping.
        multicast_mac(dst)
    } else {
        let next_hop = resolve_next_hop(ns_id, our_ip, dst);
        super::arp::resolve(next_hop)?
    };

    let frame = ethernet::build_frame(&dst_mac, &our_mac, ETHERTYPE_IPV4, &ip_packet);
    super::send_frame(&frame)?;

    Ok(())
}

/// Build an IPv4 packet with a custom TTL.
///
/// Used by traceroute to send probes with increasing TTL values.
fn build_packet_custom_ttl(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    protocol: u8,
    payload: &[u8],
    ttl: u8,
) -> Vec<u8> {
    let total_len = IPV4_HEADER_SIZE + payload.len();
    let total_len_u16 = u16::try_from(total_len).unwrap_or(u16::MAX);

    let mut pkt = Vec::with_capacity(total_len);

    // Version (4) + IHL (5 = 20 bytes, no options).
    pkt.push(0x45);
    // DSCP (0) + ECN (Not-ECT).
    pkt.push(0);
    // Total length.
    pkt.extend_from_slice(&total_len_u16.to_be_bytes());
    // Identification (0 — no fragmentation).
    pkt.extend_from_slice(&0u16.to_be_bytes());
    // Flags (Don't Fragment) + Fragment Offset.
    pkt.extend_from_slice(&0x4000u16.to_be_bytes()); // DF bit set.
    // TTL (custom).
    pkt.push(ttl);
    // Protocol.
    pkt.push(protocol);
    // Checksum placeholder (2 bytes, will be filled in).
    let checksum_offset = pkt.len();
    pkt.extend_from_slice(&[0, 0]);
    // Source address.
    pkt.extend_from_slice(&src.0);
    // Destination address.
    pkt.extend_from_slice(&dst.0);

    // Compute IP header checksum.
    let checksum = ip_checksum(&pkt[..IPV4_HEADER_SIZE]);
    if let Some(b) = pkt.get_mut(checksum_offset) { *b = (checksum >> 8) as u8; }
    if let Some(b) = pkt.get_mut(checksum_offset + 1) { *b = checksum as u8; }

    // Append payload.
    pkt.extend_from_slice(payload);

    pkt
}

/// Send an IPv4 packet with an explicit ECN codepoint in the IP header.
///
/// Identical to [`send`] except the 2-bit ECN field is set to `ecn`
/// (use [`ECN_ECT0`] for TCP with ECN negotiated).  Non-ECN callers
/// should use [`send`] which defaults to Not-ECT (0).
pub fn send_ecn(dst: Ipv4Addr, protocol: u8, payload: &[u8], ecn: u8) -> KernelResult<()> {
    send_ns_ecn(crate::netns::ROOT_NS, dst, protocol, payload, ecn)
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
    send_ns_ecn(ns_id, dst, protocol, payload, 0)
}

/// Send an IPv4 packet within a specific network namespace, with an
/// explicit ECN codepoint.
///
/// This is the core send path.  [`send_ns`] and [`send`] are convenience
/// wrappers that pass `ecn = 0` (Not-ECT).
///
/// # Parameters
///
/// - `ns_id`: Network namespace ID.  Use `ROOT_NS` (0) for the
///   physical NIC's namespace.
/// - `dst`: Destination IPv4 address.
/// - `protocol`: IP protocol number (e.g., `PROTO_TCP`, `PROTO_UDP`).
/// - `payload`: Protocol payload (e.g., TCP/UDP segment).
/// - `ecn`: 2-bit ECN codepoint for the IP header (0 = Not-ECT,
///   [`ECN_ECT1`] = 1, [`ECN_ECT0`] = 2, [`ECN_CE`] = 3).
///
/// # Errors
///
/// - [`KernelError::PermissionDenied`] if the firewall blocks the packet.
/// - [`KernelError::TimedOut`] if ARP resolution fails.
/// - [`KernelError::NoSuchDevice`] if no NIC is available.
fn send_ns_ecn(
    ns_id: crate::netns::NetNsId,
    dst: Ipv4Addr,
    protocol: u8,
    payload: &[u8],
    ecn: u8,
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

    // Build the IP packet with the namespace's source address and ECN.
    let ip_packet = build_packet_ecn(our_ip, dst, protocol, payload, ecn);

    // Determine the next-hop MAC address.
    let iface_info = interface::info();
    let dst_mac = if dst.is_broadcast()
        || (!iface_info.subnet_mask.is_unspecified()
            && is_subnet_broadcast(dst, iface_info.ip, iface_info.subnet_mask))
    {
        ethernet::BROADCAST_MAC
    } else if dst.is_multicast() {
        // RFC 1112: IPv4 multicast → Ethernet multicast MAC mapping.
        multicast_mac(dst)
    } else {
        let next_hop = resolve_next_hop(ns_id, our_ip, dst);
        super::arp::resolve(next_hop)?
    };

    // Wrap in Ethernet frame and send via the active NIC.
    let frame = ethernet::build_frame(&dst_mac, &our_mac, ETHERTYPE_IPV4, &ip_packet);

    super::send_frame(&frame)?;

    Ok(())
}

/// Send an IPv4 packet that may require fragmentation (no DF bit).
///
/// Used by UDP and other protocols that may produce payloads exceeding
/// the interface MTU.  If the packet fits in one frame, it's sent as-is
/// (without DF).  If it exceeds MTU, the payload is split into fragments
/// per RFC 791 §2.3.
///
/// TCP uses `send_ns_ecn` which always sets DF (TCP relies on MSS
/// to avoid fragmentation, and uses Path MTU Discovery).
pub fn send_fragmentable(
    dst: Ipv4Addr,
    protocol: u8,
    payload: &[u8],
) -> KernelResult<()> {
    send_fragmentable_ns(crate::netns::ROOT_NS, dst, protocol, payload)
}

/// Send a fragmentable IPv4 packet within a specific network namespace.
#[allow(clippy::arithmetic_side_effects)]
fn send_fragmentable_ns(
    ns_id: crate::netns::NetNsId,
    dst: Ipv4Addr,
    protocol: u8,
    payload: &[u8],
) -> KernelResult<()> {
    // Namespace-aware firewall outbound check.
    if !super::firewall::check_outbound_ns(ns_id, protocol, dst, payload) {
        return Err(KernelError::PermissionDenied);
    }

    let our_mac = interface::mac();
    let our_ip = interface::ns_ip(ns_id);

    // Resolve next-hop MAC address.
    let iface_info = interface::info();
    let dst_mac = if dst.is_broadcast()
        || (!iface_info.subnet_mask.is_unspecified()
            && is_subnet_broadcast(dst, iface_info.ip, iface_info.subnet_mask))
    {
        ethernet::BROADCAST_MAC
    } else if dst.is_multicast() {
        // RFC 1112: IPv4 multicast → Ethernet multicast MAC mapping.
        multicast_mac(dst)
    } else {
        let next_hop = resolve_next_hop(ns_id, our_ip, dst);
        super::arp::resolve(next_hop)?
    };

    let total_ip_len = IPV4_HEADER_SIZE + payload.len();

    if total_ip_len <= MTU {
        // Fits in one frame — send without DF (but no fragmentation needed).
        let ip_packet = build_packet_no_df(our_ip, dst, protocol, payload, 0);
        let frame = ethernet::build_frame(&dst_mac, &our_mac, ETHERTYPE_IPV4, &ip_packet);
        return super::send_frame(&frame);
    }

    // Need fragmentation.  Allocate a unique IP identification.
    let ip_id = IP_ID_COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut offset: usize = 0;
    while offset < payload.len() {
        let remaining = payload.len() - offset;
        let is_last = remaining <= MAX_FRAGMENT_PAYLOAD;
        let frag_len = if is_last { remaining } else { MAX_FRAGMENT_PAYLOAD };

        let frag_payload = &payload[offset..offset + frag_len];
        // Fragment offset is in 8-byte units.
        let frag_offset_units = (offset / 8) as u16;
        let more_fragments = !is_last;

        let frag_packet = build_fragment(
            our_ip, dst, protocol, frag_payload,
            ip_id, frag_offset_units, more_fragments,
        );
        let frame = ethernet::build_frame(&dst_mac, &our_mac, ETHERTYPE_IPV4, &frag_packet);
        super::send_frame(&frame)?;

        offset += frag_len;
    }

    Ok(())
}

/// Build an IP packet without the DF (Don't Fragment) bit set.
///
/// Used for UDP datagrams that fit in one frame but shouldn't prevent
/// intermediate routers from fragmenting if needed.
#[allow(clippy::arithmetic_side_effects)]
fn build_packet_no_df(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    protocol: u8,
    payload: &[u8],
    ecn: u8,
) -> Vec<u8> {
    let total_len = IPV4_HEADER_SIZE + payload.len();
    let total_len_u16 = u16::try_from(total_len).unwrap_or(u16::MAX);
    let ip_id = IP_ID_COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut pkt = Vec::with_capacity(total_len);
    pkt.push(0x45); // Version 4, IHL 5
    pkt.push(ecn & 0x03); // DSCP 0 + ECN
    pkt.extend_from_slice(&total_len_u16.to_be_bytes());
    pkt.extend_from_slice(&ip_id.to_be_bytes()); // Identification
    pkt.extend_from_slice(&0x0000u16.to_be_bytes()); // Flags=0 (no DF), offset=0
    pkt.push(DEFAULT_TTL);
    pkt.push(protocol);
    let cksum_off = pkt.len();
    pkt.extend_from_slice(&[0, 0]); // Checksum placeholder
    pkt.extend_from_slice(&src.0);
    pkt.extend_from_slice(&dst.0);

    let cksum = ip_checksum(&pkt[..IPV4_HEADER_SIZE]);
    pkt[cksum_off] = (cksum >> 8) as u8;
    pkt[cksum_off + 1] = cksum as u8;

    pkt.extend_from_slice(payload);
    pkt
}

/// Build a single IP fragment.
///
/// `frag_offset_units`: fragment offset in 8-byte units.
/// `more_fragments`: true if this is not the last fragment.
#[allow(clippy::arithmetic_side_effects)]
fn build_fragment(
    src: Ipv4Addr,
    dst: Ipv4Addr,
    protocol: u8,
    payload: &[u8],
    ip_id: u16,
    frag_offset_units: u16,
    more_fragments: bool,
) -> Vec<u8> {
    let total_len = IPV4_HEADER_SIZE + payload.len();
    let total_len_u16 = u16::try_from(total_len).unwrap_or(u16::MAX);

    // Flags + Fragment Offset (16-bit field):
    //   bit 15: reserved (0)
    //   bit 14: DF (0 for fragments)
    //   bit 13: MF (More Fragments)
    //   bits 12-0: fragment offset in 8-byte units
    let mf_bit: u16 = if more_fragments { 0x2000 } else { 0 };
    let flags_frag = mf_bit | (frag_offset_units & 0x1FFF);

    let mut pkt = Vec::with_capacity(total_len);
    pkt.push(0x45); // Version 4, IHL 5
    pkt.push(0x00); // DSCP 0 + ECN 0
    pkt.extend_from_slice(&total_len_u16.to_be_bytes());
    pkt.extend_from_slice(&ip_id.to_be_bytes());
    pkt.extend_from_slice(&flags_frag.to_be_bytes());
    pkt.push(DEFAULT_TTL);
    pkt.push(protocol);
    let cksum_off = pkt.len();
    pkt.extend_from_slice(&[0, 0]); // Checksum placeholder
    pkt.extend_from_slice(&src.0);
    pkt.extend_from_slice(&dst.0);

    let cksum = ip_checksum(&pkt[..IPV4_HEADER_SIZE]);
    pkt[cksum_off] = (cksum >> 8) as u8;
    pkt[cksum_off + 1] = cksum as u8;

    pkt.extend_from_slice(payload);
    pkt
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// IPv4 unit tests — exercises checksum, parsing, building, fragmentation
/// flags, transport checksums, multicast MAC mapping, and subnet broadcast.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[ipv4] Running IPv4 self-test...");

    test_ip_checksum()?;
    test_build_parse_roundtrip()?;
    test_parse_too_short()?;
    test_parse_wrong_version()?;
    test_parse_bad_checksum()?;
    test_fragment_flags()?;
    test_transport_checksum_roundtrip()?;
    test_multicast_mac_mapping()?;
    test_subnet_broadcast()?;

    crate::serial_println!("[ipv4] IPv4 self-test PASSED (9 tests)");
    Ok(())
}

/// Test ip_checksum with a known-good header.
fn test_ip_checksum() -> KernelResult<()> {
    // Build a valid IP header with build_packet, then verify that
    // ip_checksum over it returns 0 (property of a correct header).
    let src = Ipv4Addr([192, 168, 1, 1]);
    let dst = Ipv4Addr([10, 0, 0, 1]);
    let pkt = build_packet(src, dst, PROTO_UDP, b"test payload");

    // The first 20 bytes are the IP header.
    let hdr = &pkt[..IPV4_HEADER_SIZE];
    let check = ip_checksum(hdr);
    if check != 0 {
        crate::serial_println!("[ipv4]   FAIL: checksum of valid header = {:#06x}, expected 0", check);
        return Err(KernelError::InternalError);
    }

    // Verify that corrupting a byte breaks the checksum.
    let mut corrupted = [0u8; 20];
    corrupted.copy_from_slice(hdr);
    corrupted[5] ^= 0xFF; // Flip bits in DSCP/ECN byte.
    let check2 = ip_checksum(&corrupted);
    if check2 == 0 {
        crate::serial_println!("[ipv4]   FAIL: corrupted header still has valid checksum");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv4]   ip_checksum: OK");
    Ok(())
}

/// Test build_packet + parse round-trip.
fn test_build_parse_roundtrip() -> KernelResult<()> {
    let src = Ipv4Addr([172, 16, 0, 1]);
    let dst = Ipv4Addr([172, 16, 0, 2]);
    let payload = b"Hello, IPv4!";

    let pkt = build_packet(src, dst, PROTO_TCP, payload);
    let parsed = Ipv4Packet::parse(&pkt)?;

    if parsed.version != 4 {
        crate::serial_println!("[ipv4]   FAIL: version = {}", parsed.version);
        return Err(KernelError::InternalError);
    }
    if parsed.ihl != 5 {
        crate::serial_println!("[ipv4]   FAIL: ihl = {}", parsed.ihl);
        return Err(KernelError::InternalError);
    }
    if parsed.protocol != PROTO_TCP {
        crate::serial_println!("[ipv4]   FAIL: protocol = {}", parsed.protocol);
        return Err(KernelError::InternalError);
    }
    if parsed.src != src || parsed.dst != dst {
        crate::serial_println!("[ipv4]   FAIL: src/dst mismatch");
        return Err(KernelError::InternalError);
    }
    if parsed.payload != payload {
        crate::serial_println!("[ipv4]   FAIL: payload mismatch (len={})", parsed.payload.len());
        return Err(KernelError::InternalError);
    }
    if parsed.ttl != DEFAULT_TTL {
        crate::serial_println!("[ipv4]   FAIL: ttl = {}, expected {}", parsed.ttl, DEFAULT_TTL);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv4]   build/parse round-trip: OK");
    Ok(())
}

/// Test that parse rejects too-short input.
fn test_parse_too_short() -> KernelResult<()> {
    let short = [0u8; 10];
    if Ipv4Packet::parse(&short).is_ok() {
        crate::serial_println!("[ipv4]   FAIL: accepted 10-byte packet");
        return Err(KernelError::InternalError);
    }

    // Empty input.
    if Ipv4Packet::parse(&[]).is_ok() {
        crate::serial_println!("[ipv4]   FAIL: accepted empty packet");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv4]   parse too-short: OK (rejected)");
    Ok(())
}

/// Test that parse rejects non-IPv4 version.
fn test_parse_wrong_version() -> KernelResult<()> {
    let src = Ipv4Addr([10, 0, 0, 1]);
    let dst = Ipv4Addr([10, 0, 0, 2]);
    let mut pkt = build_packet(src, dst, PROTO_UDP, b"x");

    // Change version from 4 to 6 (byte 0 high nibble).
    pkt[0] = 0x65; // version=6, IHL=5
    // Recompute checksum for the mangled header (otherwise checksum
    // fails before version check).
    pkt[10] = 0;
    pkt[11] = 0;
    let cksum = ip_checksum(&pkt[..IPV4_HEADER_SIZE]);
    pkt[10] = (cksum >> 8) as u8;
    pkt[11] = cksum as u8;

    if Ipv4Packet::parse(&pkt).is_ok() {
        crate::serial_println!("[ipv4]   FAIL: accepted version 6 packet");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv4]   parse wrong version: OK (rejected)");
    Ok(())
}

/// Test that parse rejects a packet with bad checksum.
fn test_parse_bad_checksum() -> KernelResult<()> {
    let src = Ipv4Addr([10, 1, 1, 1]);
    let dst = Ipv4Addr([10, 2, 2, 2]);
    let mut pkt = build_packet(src, dst, PROTO_ICMP, b"ping");

    // Corrupt the TTL field (byte 8).
    pkt[8] = 0;

    if Ipv4Packet::parse(&pkt).is_ok() {
        crate::serial_println!("[ipv4]   FAIL: accepted packet with bad checksum");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv4]   parse bad checksum: OK (rejected)");
    Ok(())
}

/// Test MF flag and fragment offset parsing.
fn test_fragment_flags() -> KernelResult<()> {
    let src = Ipv4Addr([10, 0, 0, 1]);
    let dst = Ipv4Addr([10, 0, 0, 2]);
    let payload = b"fragment test";

    // Build a normal packet (DF set, no fragmentation).
    let pkt = build_packet(src, dst, PROTO_UDP, payload);
    let parsed = Ipv4Packet::parse(&pkt)?;

    if parsed.more_fragments() {
        crate::serial_println!("[ipv4]   FAIL: DF packet has MF set");
        return Err(KernelError::InternalError);
    }
    if parsed.fragment_offset() != 0 {
        crate::serial_println!("[ipv4]   FAIL: DF packet has non-zero offset");
        return Err(KernelError::InternalError);
    }
    if parsed.is_fragment() {
        crate::serial_println!("[ipv4]   FAIL: DF packet is_fragment() == true");
        return Err(KernelError::InternalError);
    }

    // Build a fragment with MF=1, offset=185 (1480 bytes / 8 = 185).
    let frag = build_fragment(
        src, dst, PROTO_UDP, payload,
        42, // ip_id
        185, // offset in 8-byte units
        true, // more_fragments
    );
    let parsed_frag = Ipv4Packet::parse(&frag)?;

    if !parsed_frag.more_fragments() {
        crate::serial_println!("[ipv4]   FAIL: fragment should have MF set");
        return Err(KernelError::InternalError);
    }
    if parsed_frag.fragment_offset() != 185 {
        crate::serial_println!("[ipv4]   FAIL: offset = {}, expected 185", parsed_frag.fragment_offset());
        return Err(KernelError::InternalError);
    }
    if !parsed_frag.is_fragment() {
        crate::serial_println!("[ipv4]   FAIL: is_fragment() should be true");
        return Err(KernelError::InternalError);
    }

    // Last fragment (MF=0, offset=370).
    let last_frag = build_fragment(src, dst, PROTO_UDP, payload, 42, 370, false);
    let parsed_last = Ipv4Packet::parse(&last_frag)?;

    if parsed_last.more_fragments() {
        crate::serial_println!("[ipv4]   FAIL: last fragment has MF set");
        return Err(KernelError::InternalError);
    }
    if parsed_last.fragment_offset() != 370 {
        crate::serial_println!("[ipv4]   FAIL: last frag offset = {}", parsed_last.fragment_offset());
        return Err(KernelError::InternalError);
    }
    // Last fragment still is_fragment() because offset != 0.
    if !parsed_last.is_fragment() {
        crate::serial_println!("[ipv4]   FAIL: last frag should still be a fragment");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv4]   fragment flags: OK");
    Ok(())
}

/// Test compute_transport_checksum + verify_transport_checksum round-trip.
fn test_transport_checksum_roundtrip() -> KernelResult<()> {
    let src = Ipv4Addr([192, 168, 0, 10]);
    let dst = Ipv4Addr([192, 168, 0, 20]);

    // Build a fake UDP segment: src_port(2) + dst_port(2) + len(2) + cksum(2) + data.
    let data = b"test data";
    let udp_len: u16 = 8 + data.len() as u16;
    let mut segment = Vec::with_capacity(udp_len as usize);
    segment.extend_from_slice(&1234u16.to_be_bytes()); // src port
    segment.extend_from_slice(&5678u16.to_be_bytes()); // dst port
    segment.extend_from_slice(&udp_len.to_be_bytes()); // length
    segment.extend_from_slice(&0u16.to_be_bytes());    // checksum = 0 (to compute)
    segment.extend_from_slice(data);

    // Compute checksum.
    let cksum = compute_transport_checksum(src, dst, PROTO_UDP, &segment);

    // Write checksum into the segment.
    segment[6] = (cksum >> 8) as u8;
    segment[7] = cksum as u8;

    // Verify.
    if !verify_transport_checksum(src, dst, PROTO_UDP, &segment) {
        crate::serial_println!("[ipv4]   FAIL: transport checksum verify failed after compute");
        return Err(KernelError::InternalError);
    }

    // Corrupt a byte and verify rejection.
    let orig = segment[8];
    segment[8] ^= 0xFF;
    if verify_transport_checksum(src, dst, PROTO_UDP, &segment) {
        crate::serial_println!("[ipv4]   FAIL: corrupted segment passed verification");
        return Err(KernelError::InternalError);
    }
    segment[8] = orig; // restore

    // Test UDP with checksum = 0 (means "no checksum", should pass).
    segment[6] = 0;
    segment[7] = 0;
    if !verify_transport_checksum(src, dst, PROTO_UDP, &segment) {
        crate::serial_println!("[ipv4]   FAIL: UDP cksum=0 should be accepted");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv4]   transport checksum round-trip: OK");
    Ok(())
}

/// Test multicast MAC mapping per RFC 1112.
fn test_multicast_mac_mapping() -> KernelResult<()> {
    // 224.0.0.1 → 01:00:5E:00:00:01
    let mac = multicast_mac(Ipv4Addr([224, 0, 0, 1]));
    if mac.0 != [0x01, 0x00, 0x5E, 0x00, 0x00, 0x01] {
        crate::serial_println!("[ipv4]   FAIL: 224.0.0.1 → wrong MAC {:?}", mac.0);
        return Err(KernelError::InternalError);
    }

    // 239.255.255.250 (SSDP) → 01:00:5E:7F:FF:FA
    let mac2 = multicast_mac(Ipv4Addr([239, 255, 255, 250]));
    if mac2.0 != [0x01, 0x00, 0x5E, 0x7F, 0xFF, 0xFA] {
        crate::serial_println!("[ipv4]   FAIL: 239.255.255.250 → wrong MAC {:?}", mac2.0);
        return Err(KernelError::InternalError);
    }

    // 224.128.0.5 → 01:00:5E:00:00:05 (high bit of byte[1] is masked)
    let mac3 = multicast_mac(Ipv4Addr([224, 128, 0, 5]));
    if mac3.0 != [0x01, 0x00, 0x5E, 0x00, 0x00, 0x05] {
        crate::serial_println!("[ipv4]   FAIL: 224.128.0.5 → wrong MAC {:?}", mac3.0);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv4]   multicast MAC mapping: OK");
    Ok(())
}

/// Test subnet broadcast detection.
fn test_subnet_broadcast() -> KernelResult<()> {
    let our_ip = Ipv4Addr([192, 168, 1, 100]);
    let mask = Ipv4Addr([255, 255, 255, 0]);

    // 192.168.1.255 is the subnet broadcast for /24.
    if !is_subnet_broadcast(Ipv4Addr([192, 168, 1, 255]), our_ip, mask) {
        crate::serial_println!("[ipv4]   FAIL: 192.168.1.255 should be subnet broadcast");
        return Err(KernelError::InternalError);
    }

    // 192.168.1.100 is not broadcast.
    if is_subnet_broadcast(Ipv4Addr([192, 168, 1, 100]), our_ip, mask) {
        crate::serial_println!("[ipv4]   FAIL: 192.168.1.100 is not broadcast");
        return Err(KernelError::InternalError);
    }

    // 192.168.2.255 is NOT broadcast for 192.168.1.0/24.
    if is_subnet_broadcast(Ipv4Addr([192, 168, 2, 255]), our_ip, mask) {
        crate::serial_println!("[ipv4]   FAIL: 192.168.2.255 wrong subnet");
        return Err(KernelError::InternalError);
    }

    // /16 subnet: 10.1.255.255 is broadcast for 10.1.0.0/16.
    let our_ip2 = Ipv4Addr([10, 1, 50, 1]);
    let mask2 = Ipv4Addr([255, 255, 0, 0]);
    if !is_subnet_broadcast(Ipv4Addr([10, 1, 255, 255]), our_ip2, mask2) {
        crate::serial_println!("[ipv4]   FAIL: 10.1.255.255 should be /16 broadcast");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv4]   subnet broadcast: OK");
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
