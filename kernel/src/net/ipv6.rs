//! IPv6 packet parsing and construction (RFC 8200).
//!
//! Provides basic IPv6 support:
//! - **Parsing**: 40-byte fixed header, next-header chain (extension headers skipped)
//! - **Building**: construct IPv6 packets with a given next-header and payload
//! - **Sending**: resolve next-hop, wrap in Ethernet frame, send via NIC
//! - **Receiving**: dispatch to ICMPv6, TCP, or UDP based on next-header
//!
//! ## IPv6 header format (40 bytes, fixed)
//!
//! ```text
//!  0                   1                   2                   3
//!  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |Version| Traffic Class |           Flow Label                  |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |         Payload Length        |  Next Header  |   Hop Limit   |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                       Source Address (128 bits)                |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                    Destination Address (128 bits)              |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```
//!
//! Unlike IPv4, IPv6 has no header checksum (relies on link-layer and
//! transport-layer checksums) and no fragmentation fields in the base
//! header (fragmentation uses extension headers).
//!
//! ## Design note
//!
//! This implementation supports basic connectivity (ping6, neighbor
//! discovery, UDP, SLAAC) with full extension header processing.
//! Fragment extension headers are parsed and routed to the reassembly
//! module (`frag.rs`) per RFC 8200 §4.5.  Atomic fragments (RFC 6946)
//! are detected and processed without reassembly overhead.

use alloc::vec::Vec;
use core::fmt;

use crate::error::{KernelError, KernelResult};
use crate::virtio::net::MacAddress;

use super::ethernet;
use super::interface;

// ---------------------------------------------------------------------------
// IPv6 address type
// ---------------------------------------------------------------------------

/// An IPv6 address (128 bits / 16 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv6Addr(pub [u8; 16]);

impl Ipv6Addr {
    /// The unspecified address (::).
    pub const UNSPECIFIED: Self = Self([0; 16]);

    /// The loopback address (::1).
    pub const LOOPBACK: Self = Self([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);

    /// All-nodes link-local multicast (ff02::1).
    pub const ALL_NODES_LINK_LOCAL: Self = Self([
        0xFF, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
    ]);

    /// All-routers link-local multicast (ff02::2).
    #[allow(dead_code)] // Reserved for router solicitation.
    pub const ALL_ROUTERS_LINK_LOCAL: Self = Self([
        0xFF, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02,
    ]);

    /// Create an IPv6 address from 16 bytes.
    #[allow(dead_code)] // Public API.
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Check if this is the unspecified address (::).
    pub fn is_unspecified(self) -> bool {
        self == Self::UNSPECIFIED
    }

    /// Check if this is the loopback address (::1).
    pub fn is_loopback(self) -> bool {
        self == Self::LOOPBACK
    }

    /// Check if this is a multicast address (ff00::/8).
    pub fn is_multicast(self) -> bool {
        self.0[0] == 0xFF
    }

    /// Check if this is a link-local unicast address (fe80::/10).
    pub fn is_link_local(self) -> bool {
        self.0[0] == 0xFE && (self.0[1] & 0xC0) == 0x80
    }

    /// Generate a solicited-node multicast address for this unicast address.
    ///
    /// Per RFC 4291 section 2.7.1, the solicited-node multicast address
    /// is formed by appending the low 24 bits of the unicast address to
    /// the prefix ff02::1:ff00:0/104.
    ///
    /// Used by NDP Neighbor Solicitation to efficiently resolve addresses
    /// without broadcasting to all nodes.
    pub fn solicited_node_multicast(self) -> Self {
        Self([
            0xFF, 0x02, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0x01, 0xFF, self.0[13], self.0[14], self.0[15],
        ])
    }

    /// Generate a link-local address from a MAC address using modified
    /// EUI-64 (RFC 4291 Appendix A).
    ///
    /// The MAC address is split, ff:fe is inserted in the middle, and
    /// the universal/local bit (bit 1 of the first octet) is flipped.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn from_mac_link_local(mac: &MacAddress) -> Self {
        let mut addr = [0u8; 16];
        addr[0] = 0xFE;
        addr[1] = 0x80;
        // bytes 2..7 are zero (link-local prefix is fe80::/64)
        // Modified EUI-64 from MAC:
        addr[8] = mac.0[0] ^ 0x02; // Flip U/L bit.
        addr[9] = mac.0[1];
        addr[10] = mac.0[2];
        addr[11] = 0xFF;
        addr[12] = 0xFE;
        addr[13] = mac.0[3];
        addr[14] = mac.0[4];
        addr[15] = mac.0[5];
        Self(addr)
    }

    /// Parse an IPv6 address from a string in standard colon-hex notation.
    ///
    /// Supports the full RFC 5952 formats:
    /// - Full form: `2001:0db8:0000:0000:0000:0000:0000:0001`
    /// - Compressed: `2001:db8::1` (consecutive zero groups collapsed to `::`)
    /// - Loopback: `::1`
    /// - Unspecified: `::`
    ///
    /// Returns `None` if the string is not a valid IPv6 address.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }

        // Split on "::" to handle compressed notation.
        let has_double_colon = s.contains("::");
        let (left, right) = if let Some(pos) = s.find("::") {
            (&s[..pos], &s[pos + 2..])
        } else {
            (s, "")
        };

        // Parse groups from left side.
        let left_groups: Vec<u16> = if left.is_empty() {
            Vec::new()
        } else {
            let mut v = Vec::new();
            for part in left.split(':') {
                let val = u16::from_str_radix(part, 16).ok()?;
                v.push(val);
            }
            v
        };

        // Parse groups from right side (after "::").
        let right_groups: Vec<u16> = if !has_double_colon || right.is_empty() {
            Vec::new()
        } else {
            let mut v = Vec::new();
            for part in right.split(':') {
                let val = u16::from_str_radix(part, 16).ok()?;
                v.push(val);
            }
            v
        };

        let total = left_groups.len() + right_groups.len();
        if !has_double_colon && total != 8 {
            return None; // Must have exactly 8 groups without "::".
        }
        if total > 8 {
            return None; // Too many groups.
        }

        // Build 8 groups: left + zeros + right.
        let zeros_needed = 8 - total;
        let mut groups = [0u16; 8];
        let mut idx = 0;
        for &g in &left_groups {
            if idx >= 8 { return None; }
            groups[idx] = g;
            idx += 1;
        }
        idx += zeros_needed;
        for &g in &right_groups {
            if idx >= 8 { return None; }
            groups[idx] = g;
            idx += 1;
        }

        // Convert to bytes.
        let mut bytes = [0u8; 16];
        for i in 0..8 {
            let be = groups[i].to_be_bytes();
            bytes[i * 2] = be[0];
            bytes[i * 2 + 1] = be[1];
        }

        Some(Self(bytes))
    }

    /// Check if this is a global unicast address (not link-local, multicast,
    /// loopback, or unspecified).
    #[allow(dead_code)] // Public API.
    pub fn is_global_unicast(self) -> bool {
        !self.is_unspecified()
            && !self.is_loopback()
            && !self.is_multicast()
            && !self.is_link_local()
    }

    /// Return a string representation of this address with a prefix length,
    /// masking off the host portion (e.g. `"2001:db8:1::/64"`).
    ///
    /// Only the first `prefix_len` bits are kept; the rest are zeroed.
    #[allow(dead_code)] // Public API.
    pub fn prefix_string(self, prefix_len: u8) -> alloc::string::String {
        use alloc::format;
        let mut masked = self.0;
        // Zero out host bits beyond prefix_len.
        let full_bytes = (prefix_len / 8) as usize;
        let remaining_bits = prefix_len % 8;
        if full_bytes < 16 {
            if remaining_bits > 0 {
                // Partial byte: keep the top `remaining_bits` bits.
                #[allow(clippy::arithmetic_side_effects)]
                let mask = 0xFF_u8 << (8 - remaining_bits);
                if let Some(b) = masked.get_mut(full_bytes) {
                    *b &= mask;
                }
                // Zero all bytes after.
                for b in masked.iter_mut().skip(full_bytes.saturating_add(1)) {
                    *b = 0;
                }
            } else {
                // Zero all bytes from full_bytes onwards.
                for b in masked.iter_mut().skip(full_bytes) {
                    *b = 0;
                }
            }
        }
        format!("{}/{}", Self(masked), prefix_len)
    }
}

impl fmt::Display for Ipv6Addr {
    /// Format as RFC 5952 compressed notation.
    ///
    /// Finds the longest run of consecutive zero 16-bit groups and
    /// replaces it with "::".  Ties are broken by choosing the first run.
    #[allow(clippy::arithmetic_side_effects)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Extract 8 groups of 16-bit values.
        let mut groups = [0u16; 8];
        for i in 0..8 {
            groups[i] = u16::from_be_bytes([self.0[i * 2], self.0[i * 2 + 1]]);
        }

        // Find the longest run of consecutive zero groups.
        let mut best_start = usize::MAX;
        let mut best_len: usize = 0;
        let mut cur_start = usize::MAX;
        let mut cur_len: usize = 0;

        for (i, &g) in groups.iter().enumerate() {
            if g == 0 {
                if cur_len == 0 {
                    cur_start = i;
                }
                cur_len += 1;
            } else {
                if cur_len > best_len {
                    best_start = cur_start;
                    best_len = cur_len;
                }
                cur_len = 0;
            }
        }
        if cur_len > best_len {
            best_start = cur_start;
            best_len = cur_len;
        }

        // Don't use :: for a single zero group (RFC 5952 recommendation).
        if best_len <= 1 {
            best_start = usize::MAX;
            best_len = 0;
        }

        let mut i = 0;
        let mut need_colon = false;
        while i < 8 {
            if i == best_start {
                write!(f, "::")?;
                i += best_len;
                need_colon = false;
                continue;
            }
            if need_colon {
                write!(f, ":")?;
            }
            write!(f, "{:x}", groups[i])?;
            need_colon = true;
            i += 1;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Protocol / next-header constants
// ---------------------------------------------------------------------------

/// Next Header: Hop-by-Hop Options.
const NH_HOP_BY_HOP: u8 = 0;
/// Next Header: TCP.
#[allow(dead_code)] // Will be used when TCP over IPv6 is implemented.
pub const NH_TCP: u8 = 6;
/// Next Header: UDP.
pub const NH_UDP: u8 = 17;
/// Next Header: Routing.
const NH_ROUTING: u8 = 43;
/// Next Header: Fragment.
const NH_FRAGMENT: u8 = 44;
/// Next Header: ICMPv6.
pub const NH_ICMPV6: u8 = 58;
/// Next Header: No Next Header.
#[allow(dead_code)] // Defined for completeness per RFC 8200.
const NH_NONE: u8 = 59;
/// Next Header: Destination Options.
const NH_DESTINATION: u8 = 60;

/// IPv6 fixed header size (always 40 bytes).
pub const IPV6_HEADER_SIZE: usize = 40;

/// Default hop limit for outgoing packets.
#[allow(dead_code)] // Used by send() which is a public API.
pub const DEFAULT_HOP_LIMIT: u8 = 64;

/// EtherType for IPv6.
pub const ETHERTYPE_IPV6: u16 = 0x86DD;

// ---------------------------------------------------------------------------
// IPv6 packet parsing
// ---------------------------------------------------------------------------

/// A parsed IPv6 packet header.
#[allow(dead_code)] // Spec-defined fields not all used yet.
pub struct Ipv6Packet<'a> {
    /// IP version (should be 6).
    pub version: u8,
    /// Traffic class (DSCP + ECN).
    pub traffic_class: u8,
    /// Flow label (20 bits).
    pub flow_label: u32,
    /// Payload length (bytes after the 40-byte header, including extension headers).
    pub payload_length: u16,
    /// Next header in the chain (transport protocol or extension header type).
    pub next_header: u8,
    /// Hop limit (decremented by each router, analogous to IPv4 TTL).
    pub hop_limit: u8,
    /// Source IPv6 address.
    pub src: Ipv6Addr,
    /// Destination IPv6 address.
    pub dst: Ipv6Addr,
    /// Upper-layer protocol number after skipping extension headers.
    ///
    /// This is the "final" next-header value (TCP, UDP, ICMPv6, etc.).
    /// If the packet has no extension headers, this equals `next_header`.
    /// For fragmented packets, this is the Fragment header's Next Header
    /// (i.e., the protocol of the fragmentable part).
    pub upper_protocol: u8,
    /// Upper-layer payload (after all extension headers).
    /// For fragmented packets, this is the fragment data (after the
    /// Fragment header), NOT the reassembled datagram.
    pub payload: &'a [u8],
    /// Raw header bytes (for error generation).
    pub raw_header: &'a [u8],
    /// Fragment header info, if the packet contains a Fragment extension
    /// header.  `None` for unfragmented packets.
    ///
    /// Fields: (fragment_offset_units, more_fragments, identification)
    /// - `fragment_offset_units`: 13-bit offset in 8-byte units.
    /// - `more_fragments`: M flag (true = more fragments follow).
    /// - `identification`: 32-bit datagram identifier.
    pub fragment_info: Option<(u16, bool, u32)>,
}

impl<'a> Ipv6Packet<'a> {
    /// Parse an IPv6 packet from raw bytes.
    ///
    /// Skips known extension headers (Hop-by-Hop, Routing, Fragment,
    /// Destination Options) to find the upper-layer payload.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn parse(data: &'a [u8]) -> KernelResult<Self> {
        if data.len() < IPV6_HEADER_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        let version = data[0] >> 4;
        if version != 6 {
            return Err(KernelError::InvalidArgument);
        }

        // Traffic class: high 4 bits of byte 0 (after version) + low 4 bits of byte 1.
        let traffic_class = ((data[0] & 0x0F) << 4) | (data[1] >> 4);

        // Flow label: low 4 bits of byte 1 + bytes 2-3.
        let flow_label = (u32::from(data[1] & 0x0F) << 16)
            | (u32::from(data[2]) << 8)
            | u32::from(data[3]);

        let payload_length = u16::from_be_bytes([data[4], data[5]]);
        let next_header = data[6];
        let hop_limit = data[7];

        let mut src = [0u8; 16];
        let mut dst = [0u8; 16];
        src.copy_from_slice(&data[8..24]);
        dst.copy_from_slice(&data[24..40]);

        // Determine the end of the payload based on payload_length.
        let payload_end = IPV6_HEADER_SIZE
            .checked_add(payload_length as usize)
            .unwrap_or(data.len())
            .min(data.len());

        let full_payload = if IPV6_HEADER_SIZE < payload_end {
            &data[IPV6_HEADER_SIZE..payload_end]
        } else {
            &[]
        };

        // Skip extension headers to find the upper-layer payload.
        let ext = skip_extension_headers(next_header, full_payload);

        Ok(Self {
            version,
            traffic_class,
            flow_label,
            payload_length,
            next_header,
            hop_limit,
            src: Ipv6Addr(src),
            dst: Ipv6Addr(dst),
            upper_protocol: ext.upper_protocol,
            payload: ext.payload,
            raw_header: &data[..IPV6_HEADER_SIZE],
            fragment_info: ext.fragment_info,
        })
    }
}

/// Result of extension header traversal.
struct ExtHeaderResult<'a> {
    /// Final upper-layer protocol number.
    upper_protocol: u8,
    /// Payload after all extension headers.
    payload: &'a [u8],
    /// Fragment header info, if present.
    fragment_info: Option<(u16, bool, u32)>,
}

/// Skip known IPv6 extension headers to find the upper-layer payload.
///
/// Extension headers (Hop-by-Hop, Routing, Fragment, Destination Options)
/// each have a "Next Header" field (first byte) and a "Header Extension
/// Length" field (second byte, in 8-octet units excluding the first 8).
///
/// Fragment headers are fixed at 8 bytes (no length field).  When a
/// Fragment header is encountered, its fields are extracted and stored
/// so the caller can route the packet to reassembly.
#[allow(clippy::arithmetic_side_effects)]
fn skip_extension_headers<'a>(mut nh: u8, mut data: &'a [u8]) -> ExtHeaderResult<'a> {
    let mut frag_info: Option<(u16, bool, u32)> = None;

    loop {
        match nh {
            NH_HOP_BY_HOP | NH_ROUTING | NH_DESTINATION => {
                // These have: next_header (1 byte) + hdr_ext_len (1 byte) + data.
                // Total length = (hdr_ext_len + 1) * 8 bytes.
                if data.len() < 2 {
                    return ExtHeaderResult {
                        upper_protocol: nh,
                        payload: data,
                        fragment_info: frag_info,
                    };
                }
                let next = data[0];
                let hdr_ext_len = data[1] as usize;
                let total = (hdr_ext_len + 1) * 8;
                if data.len() < total {
                    return ExtHeaderResult {
                        upper_protocol: nh,
                        payload: data,
                        fragment_info: frag_info,
                    };
                }
                nh = next;
                data = &data[total..];
            }
            NH_FRAGMENT => {
                // Fragment header is always 8 bytes.
                // Parse fragment fields using the shared parser.
                match super::frag::parse_fragment_header(data) {
                    Some((next_hdr, offset, more, id)) => {
                        frag_info = Some((offset, more, id));
                        nh = next_hdr;
                        data = &data[8..];
                    }
                    None => {
                        // Truncated fragment header — return what we have.
                        return ExtHeaderResult {
                            upper_protocol: nh,
                            payload: data,
                            fragment_info: frag_info,
                        };
                    }
                }
            }
            _ => {
                // Not a known extension header — this is the upper-layer protocol.
                return ExtHeaderResult {
                    upper_protocol: nh,
                    payload: data,
                    fragment_info: frag_info,
                };
            }
        }
    }
}

// ---------------------------------------------------------------------------
// IPv6 packet construction
// ---------------------------------------------------------------------------

/// Build an IPv6 packet with the given parameters.
///
/// Returns the raw packet bytes (40-byte header + payload).
#[allow(clippy::arithmetic_side_effects)]
pub fn build_packet(
    src: Ipv6Addr,
    dst: Ipv6Addr,
    next_header: u8,
    hop_limit: u8,
    payload: &[u8],
) -> Vec<u8> {
    let payload_len = u16::try_from(payload.len()).unwrap_or(u16::MAX);
    let total = IPV6_HEADER_SIZE + payload.len();
    let mut pkt = Vec::with_capacity(total);

    // Byte 0: version (6) in high nibble + high 4 bits of traffic class (0).
    pkt.push(0x60);
    // Byte 1: low 4 bits of traffic class (0) + high 4 bits of flow label (0).
    pkt.push(0x00);
    // Bytes 2-3: low 16 bits of flow label (0).
    pkt.extend_from_slice(&[0x00, 0x00]);
    // Bytes 4-5: payload length.
    pkt.extend_from_slice(&payload_len.to_be_bytes());
    // Byte 6: next header.
    pkt.push(next_header);
    // Byte 7: hop limit.
    pkt.push(hop_limit);
    // Bytes 8-23: source address.
    pkt.extend_from_slice(&src.0);
    // Bytes 24-39: destination address.
    pkt.extend_from_slice(&dst.0);
    // Payload.
    pkt.extend_from_slice(payload);

    pkt
}

// ---------------------------------------------------------------------------
// IPv6 transport checksum (pseudo-header, RFC 8200 section 8.1)
// ---------------------------------------------------------------------------

/// Compute the upper-layer checksum using the IPv6 pseudo-header.
///
/// The pseudo-header for IPv6 checksums consists of:
/// - Source Address (16 bytes)
/// - Destination Address (16 bytes)
/// - Upper-Layer Packet Length (4 bytes, big-endian)
/// - 3 zero bytes + Next Header (1 byte)
///
/// This is used by ICMPv6, TCP, and UDP over IPv6.
/// The checksum field within the segment MUST be zeroed before calling this.
#[allow(clippy::arithmetic_side_effects)]
pub fn compute_transport_checksum(
    src: &Ipv6Addr,
    dst: &Ipv6Addr,
    next_header: u8,
    segment: &[u8],
) -> u16 {
    let mut sum: u32 = 0;

    // Pseudo-header: source address (16 bytes = 8 words).
    for i in 0..8 {
        let word = u16::from_be_bytes([src.0[i * 2], src.0[i * 2 + 1]]);
        sum = sum.wrapping_add(u32::from(word));
    }
    // Pseudo-header: destination address.
    for i in 0..8 {
        let word = u16::from_be_bytes([dst.0[i * 2], dst.0[i * 2 + 1]]);
        sum = sum.wrapping_add(u32::from(word));
    }
    // Pseudo-header: upper-layer packet length (32 bits).
    let seg_len = segment.len() as u32;
    sum = sum.wrapping_add(seg_len >> 16);
    sum = sum.wrapping_add(seg_len & 0xFFFF);
    // Pseudo-header: zero + next header.
    sum = sum.wrapping_add(u32::from(next_header));

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

    // For UDP over IPv6, a checksum of 0 is transmitted as 0xFFFF (RFC 8200).
    if next_header == NH_UDP && cksum == 0 {
        0xFFFF
    } else {
        cksum
    }
}

/// Verify an upper-layer checksum using the IPv6 pseudo-header.
///
/// Returns `true` if the checksum is valid (folds to 0xFFFF).
#[allow(clippy::arithmetic_side_effects)]
pub fn verify_transport_checksum(
    src: &Ipv6Addr,
    dst: &Ipv6Addr,
    next_header: u8,
    segment: &[u8],
) -> bool {
    let mut sum: u32 = 0;

    // Pseudo-header: source address.
    for i in 0..8 {
        let word = u16::from_be_bytes([src.0[i * 2], src.0[i * 2 + 1]]);
        sum = sum.wrapping_add(u32::from(word));
    }
    // Pseudo-header: destination address.
    for i in 0..8 {
        let word = u16::from_be_bytes([dst.0[i * 2], dst.0[i * 2 + 1]]);
        sum = sum.wrapping_add(u32::from(word));
    }
    // Pseudo-header: upper-layer packet length (32 bits).
    let seg_len = segment.len() as u32;
    sum = sum.wrapping_add(seg_len >> 16);
    sum = sum.wrapping_add(seg_len & 0xFFFF);
    // Pseudo-header: next header.
    sum = sum.wrapping_add(u32::from(next_header));

    // Sum the segment.
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

    sum == 0xFFFF
}

// ---------------------------------------------------------------------------
// IPv6 multicast MAC mapping (RFC 2464 section 7)
// ---------------------------------------------------------------------------

/// Compute the Ethernet multicast MAC for an IPv6 multicast address.
///
/// Per RFC 2464 section 7: the Ethernet MAC is formed by prepending
/// 33:33 to the low 32 bits of the IPv6 multicast address.
pub fn multicast_mac(ip: &Ipv6Addr) -> MacAddress {
    MacAddress([0x33, 0x33, ip.0[12], ip.0[13], ip.0[14], ip.0[15]])
}

// ---------------------------------------------------------------------------
// IPv6 processing
// ---------------------------------------------------------------------------

/// Process an incoming IPv6 packet.
///
/// If the packet contains a Fragment extension header, it is routed to
/// the reassembly module.  When all fragments have arrived, the
/// reassembled datagram is dispatched to the appropriate transport handler.
/// Unfragmented packets are dispatched directly.
pub fn process_ipv6(data: &[u8]) -> KernelResult<()> {
    let packet = Ipv6Packet::parse(data)?;

    // Check if the packet is addressed to us (link-local, SLAAC global,
    // multicast, or loopback).
    let our_mac = interface::mac();
    let our_link_local = Ipv6Addr::from_mac_link_local(&our_mac);

    let is_for_us = packet.dst == our_link_local
        || packet.dst.is_multicast()
        || packet.dst == Ipv6Addr::LOOPBACK
        || (super::icmpv6::slaac_global_addr() == Some(packet.dst));

    if !is_for_us {
        return Ok(());
    }

    // If the packet is a fragment, route to reassembly.
    if let Some((frag_offset, more_fragments, identification)) = packet.fragment_info {
        // An "atomic fragment" has offset=0 and M=0 — it is not actually
        // fragmented (RFC 6946).  Process it directly.
        if frag_offset == 0 && !more_fragments {
            // Fall through to normal dispatch below.
        } else {
            return process_fragment(
                packet.src,
                packet.dst,
                packet.upper_protocol,
                frag_offset,
                more_fragments,
                identification,
                packet.payload,
            );
        }
    }

    // IPv6 firewall: check inbound packet before dispatching.
    // Pass the upper-layer protocol and payload to the firewall.
    if !super::firewall::check_inbound_v6(packet.upper_protocol, packet.src, packet.payload) {
        return Ok(()); // Silently drop — firewall denied.
    }

    dispatch_upper_layer(packet.upper_protocol, packet.src, packet.dst, packet.payload)
}

/// Dispatch a complete (reassembled or unfragmented) datagram to the
/// appropriate transport-layer handler.
fn dispatch_upper_layer(
    protocol: u8,
    src: Ipv6Addr,
    dst: Ipv6Addr,
    payload: &[u8],
) -> KernelResult<()> {
    // Build a minimal Ipv6Packet for handlers that need it.
    // The raw_header is empty since we don't have it for reassembled
    // datagrams, but current handlers don't use it for IPv6.
    let fake_packet = Ipv6Packet {
        version: 6,
        traffic_class: 0,
        flow_label: 0,
        payload_length: u16::try_from(payload.len()).unwrap_or(u16::MAX),
        next_header: protocol,
        hop_limit: 0,
        src,
        dst,
        upper_protocol: protocol,
        payload,
        raw_header: &[],
        fragment_info: None,
    };

    match protocol {
        NH_ICMPV6 => super::icmpv6::process_icmpv6(&fake_packet),
        NH_UDP => super::udp::process_udp_v6(&fake_packet),
        NH_TCP => super::tcp::process_tcp_v6(&fake_packet),
        _ => {
            // Unknown upper-layer protocol — silently drop.
            Ok(())
        }
    }
}

/// Handle a received IPv6 fragment.
///
/// Passes the fragment to the reassembly module.  If reassembly completes,
/// the reassembled datagram is dispatched to the transport layer.
fn process_fragment(
    src: Ipv6Addr,
    dst: Ipv6Addr,
    upper_protocol: u8,
    fragment_offset: u16,
    more_fragments: bool,
    identification: u32,
    fragment_data: &[u8],
) -> KernelResult<()> {
    if let Some(reassembled) = super::frag::add_fragment_v6(
        src,
        dst,
        identification,
        upper_protocol,
        fragment_offset,
        more_fragments,
        fragment_data,
    ) {
        // Run the firewall on the reassembled datagram, not individual fragments.
        if !super::firewall::check_inbound_v6(
            reassembled.upper_protocol,
            reassembled.src,
            &reassembled.payload,
        ) {
            return Ok(());
        }

        dispatch_upper_layer(
            reassembled.upper_protocol,
            reassembled.src,
            reassembled.dst,
            &reassembled.payload,
        )
    } else {
        Ok(())
    }
}

/// Send an IPv6 packet.
///
/// Resolves the destination MAC address (multicast mapping or neighbor
/// cache for unicast) and sends via the active NIC.
#[allow(dead_code)] // Public API — called by ping6 and future IPv6 sockets.
pub fn send(dst: Ipv6Addr, next_header: u8, payload: &[u8]) -> KernelResult<()> {
    let our_mac = interface::mac();
    let src = Ipv6Addr::from_mac_link_local(&our_mac);

    let ip_packet = build_packet(src, dst, next_header, DEFAULT_HOP_LIMIT, payload);

    // Determine the destination MAC.
    let dst_mac = if dst.is_multicast() {
        multicast_mac(&dst)
    } else {
        // For unicast link-local, try NDP neighbor cache.
        // Fall back to solicited-node multicast if not cached.
        // For now, use the neighbor cache with a fallback to multicast.
        match super::icmpv6::neighbor_lookup(&dst) {
            Some(mac) => mac,
            None => {
                // Send a neighbor solicitation and return an error.
                // The caller should retry after NDP resolution completes.
                let _ = super::icmpv6::send_neighbor_solicitation(dst);
                return Err(KernelError::TimedOut);
            }
        }
    };

    let frame = ethernet::build_frame(&dst_mac, &our_mac, ETHERTYPE_IPV6, &ip_packet);
    super::send_frame(&frame)
}

/// Send an IPv6 packet with a pre-built payload and explicit source address.
///
/// Used by ICMPv6 which needs to control the source address (e.g.,
/// using the unspecified address during DAD).
pub fn send_raw(
    src: Ipv6Addr,
    dst: Ipv6Addr,
    next_header: u8,
    hop_limit: u8,
    payload: &[u8],
) -> KernelResult<()> {
    // IPv6 firewall: check outbound packet before sending.
    if !super::firewall::check_outbound_v6(next_header, dst, payload) {
        return Err(KernelError::PermissionDenied);
    }

    let our_mac = interface::mac();
    let ip_packet = build_packet(src, dst, next_header, hop_limit, payload);

    let dst_mac = if dst.is_multicast() {
        multicast_mac(&dst)
    } else {
        match super::icmpv6::neighbor_lookup(&dst) {
            Some(mac) => mac,
            None => return Err(KernelError::TimedOut),
        }
    };

    let frame = ethernet::build_frame(&dst_mac, &our_mac, ETHERTYPE_IPV6, &ip_packet);
    super::send_frame(&frame)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// IPv6 unit tests — exercises address types, Display formatting,
/// packet build/parse round-trip, extension header skipping, multicast
/// MAC mapping, and transport checksum computation.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[ipv6] Running IPv6 self-test...");

    test_ipv6_addr_constants()?;
    test_ipv6_addr_classification()?;
    test_ipv6_addr_display()?;
    test_ipv6_addr_link_local_from_mac()?;
    test_ipv6_addr_solicited_node()?;
    test_build_parse_roundtrip()?;
    test_parse_too_short()?;
    test_parse_wrong_version()?;
    test_extension_header_skip()?;
    test_fragment_header_parse()?;
    test_atomic_fragment()?;
    test_multicast_mac_mapping()?;
    test_transport_checksum_roundtrip()?;
    test_ipv6_addr_parse()?;

    crate::serial_println!("[ipv6] IPv6 self-test PASSED (14 tests)");
    Ok(())
}

/// Test IPv6 address constants.
fn test_ipv6_addr_constants() -> KernelResult<()> {
    if !Ipv6Addr::UNSPECIFIED.is_unspecified() {
        crate::serial_println!("[ipv6]   FAIL: UNSPECIFIED.is_unspecified()");
        return Err(KernelError::InternalError);
    }
    if !Ipv6Addr::LOOPBACK.is_loopback() {
        crate::serial_println!("[ipv6]   FAIL: LOOPBACK.is_loopback()");
        return Err(KernelError::InternalError);
    }
    if Ipv6Addr::UNSPECIFIED.is_loopback() {
        crate::serial_println!("[ipv6]   FAIL: UNSPECIFIED should not be loopback");
        return Err(KernelError::InternalError);
    }
    if Ipv6Addr::LOOPBACK.is_unspecified() {
        crate::serial_println!("[ipv6]   FAIL: LOOPBACK should not be unspecified");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv6]   address constants: OK");
    Ok(())
}

/// Test IPv6 address classification methods.
fn test_ipv6_addr_classification() -> KernelResult<()> {
    // Multicast: ff02::1.
    if !Ipv6Addr::ALL_NODES_LINK_LOCAL.is_multicast() {
        crate::serial_println!("[ipv6]   FAIL: ff02::1 should be multicast");
        return Err(KernelError::InternalError);
    }

    // Non-multicast.
    if Ipv6Addr::LOOPBACK.is_multicast() {
        crate::serial_println!("[ipv6]   FAIL: ::1 should not be multicast");
        return Err(KernelError::InternalError);
    }

    // Link-local: fe80::1.
    let ll = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    if !ll.is_link_local() {
        crate::serial_println!("[ipv6]   FAIL: fe80::1 should be link-local");
        return Err(KernelError::InternalError);
    }

    // Global unicast is not link-local.
    let global = Ipv6Addr([0x20, 0x01, 0x0D, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    if global.is_link_local() {
        crate::serial_println!("[ipv6]   FAIL: 2001:db8::1 should not be link-local");
        return Err(KernelError::InternalError);
    }
    if global.is_multicast() {
        crate::serial_println!("[ipv6]   FAIL: 2001:db8::1 should not be multicast");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv6]   address classification: OK");
    Ok(())
}

/// Test IPv6 Display formatting (RFC 5952 compressed notation).
fn test_ipv6_addr_display() -> KernelResult<()> {
    use alloc::format;

    // :: (all zeros).
    let s = format!("{}", Ipv6Addr::UNSPECIFIED);
    if s != "::" {
        crate::serial_println!("[ipv6]   FAIL: UNSPECIFIED = '{}', expected '::'", s);
        return Err(KernelError::InternalError);
    }

    // ::1 (loopback).
    let s = format!("{}", Ipv6Addr::LOOPBACK);
    if s != "::1" {
        crate::serial_println!("[ipv6]   FAIL: LOOPBACK = '{}', expected '::1'", s);
        return Err(KernelError::InternalError);
    }

    // ff02::1 (all-nodes multicast).
    let s = format!("{}", Ipv6Addr::ALL_NODES_LINK_LOCAL);
    if s != "ff02::1" {
        crate::serial_println!("[ipv6]   FAIL: ALL_NODES = '{}', expected 'ff02::1'", s);
        return Err(KernelError::InternalError);
    }

    // fe80::1 (link-local).
    let ll = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let s = format!("{}", ll);
    if s != "fe80::1" {
        crate::serial_println!("[ipv6]   FAIL: fe80::1 = '{}'", s);
        return Err(KernelError::InternalError);
    }

    // 2001:db8::1 (global unicast with zero run).
    let global = Ipv6Addr([0x20, 0x01, 0x0D, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let s = format!("{}", global);
    if s != "2001:db8::1" {
        crate::serial_println!("[ipv6]   FAIL: 2001:db8::1 = '{}'", s);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv6]   Display formatting: OK");
    Ok(())
}

/// Test link-local address generation from MAC.
fn test_ipv6_addr_link_local_from_mac() -> KernelResult<()> {
    // MAC 52:54:00:12:34:56 → fe80::5054:ff:fe12:3456
    // (with U/L bit flipped: 52 ^ 02 = 50)
    let mac = MacAddress([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    let ll = Ipv6Addr::from_mac_link_local(&mac);

    let expected = Ipv6Addr([
        0xFE, 0x80, 0, 0, 0, 0, 0, 0,
        0x50, 0x54, 0x00, 0xFF, 0xFE, 0x12, 0x34, 0x56,
    ]);

    if ll != expected {
        crate::serial_println!("[ipv6]   FAIL: link-local from MAC mismatch");
        crate::serial_println!("[ipv6]     got:      {}", ll);
        crate::serial_println!("[ipv6]     expected: {}", expected);
        return Err(KernelError::InternalError);
    }

    // Verify it's classified as link-local.
    if !ll.is_link_local() {
        crate::serial_println!("[ipv6]   FAIL: generated address not link-local");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv6]   link-local from MAC: OK");
    Ok(())
}

/// Test solicited-node multicast address generation.
fn test_ipv6_addr_solicited_node() -> KernelResult<()> {
    // fe80::5054:ff:fe12:3456 → ff02::1:ff12:3456
    let addr = Ipv6Addr([
        0xFE, 0x80, 0, 0, 0, 0, 0, 0,
        0x50, 0x54, 0x00, 0xFF, 0xFE, 0x12, 0x34, 0x56,
    ]);
    let snm = addr.solicited_node_multicast();

    let expected = Ipv6Addr([
        0xFF, 0x02, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0x01, 0xFF, 0x12, 0x34, 0x56,
    ]);

    if snm != expected {
        crate::serial_println!("[ipv6]   FAIL: solicited-node mcast mismatch");
        crate::serial_println!("[ipv6]     got:      {}", snm);
        crate::serial_println!("[ipv6]     expected: {}", expected);
        return Err(KernelError::InternalError);
    }

    // Must be multicast.
    if !snm.is_multicast() {
        crate::serial_println!("[ipv6]   FAIL: solicited-node not multicast");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv6]   solicited-node multicast: OK");
    Ok(())
}

/// Test build_packet + parse round-trip.
fn test_build_parse_roundtrip() -> KernelResult<()> {
    let src = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let dst = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
    let payload = b"Hello, IPv6!";

    let pkt = build_packet(src, dst, NH_ICMPV6, 64, payload);
    let parsed = Ipv6Packet::parse(&pkt)?;

    if parsed.version != 6 {
        crate::serial_println!("[ipv6]   FAIL: version = {}", parsed.version);
        return Err(KernelError::InternalError);
    }
    if parsed.next_header != NH_ICMPV6 {
        crate::serial_println!("[ipv6]   FAIL: next_header = {}", parsed.next_header);
        return Err(KernelError::InternalError);
    }
    if parsed.hop_limit != 64 {
        crate::serial_println!("[ipv6]   FAIL: hop_limit = {}", parsed.hop_limit);
        return Err(KernelError::InternalError);
    }
    if parsed.src != src {
        crate::serial_println!("[ipv6]   FAIL: src mismatch");
        return Err(KernelError::InternalError);
    }
    if parsed.dst != dst {
        crate::serial_println!("[ipv6]   FAIL: dst mismatch");
        return Err(KernelError::InternalError);
    }
    if parsed.payload != payload {
        crate::serial_println!("[ipv6]   FAIL: payload mismatch (len={})", parsed.payload.len());
        return Err(KernelError::InternalError);
    }
    if parsed.payload_length as usize != payload.len() {
        crate::serial_println!("[ipv6]   FAIL: payload_length = {}", parsed.payload_length);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv6]   build/parse round-trip: OK");
    Ok(())
}

/// Test that parse rejects too-short input.
fn test_parse_too_short() -> KernelResult<()> {
    let short = [0u8; 39]; // One byte short of minimum.
    if Ipv6Packet::parse(&short).is_ok() {
        crate::serial_println!("[ipv6]   FAIL: accepted 39-byte packet");
        return Err(KernelError::InternalError);
    }

    if Ipv6Packet::parse(&[]).is_ok() {
        crate::serial_println!("[ipv6]   FAIL: accepted empty packet");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv6]   parse too-short: OK (rejected)");
    Ok(())
}

/// Test that parse rejects non-IPv6 version.
fn test_parse_wrong_version() -> KernelResult<()> {
    let src = Ipv6Addr::LOOPBACK;
    let dst = Ipv6Addr::LOOPBACK;
    let mut pkt = build_packet(src, dst, NH_ICMPV6, 64, b"test");

    // Change version from 6 to 4.
    pkt[0] = (4 << 4) | (pkt[0] & 0x0F);

    if Ipv6Packet::parse(&pkt).is_ok() {
        crate::serial_println!("[ipv6]   FAIL: accepted version 4 packet");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv6]   parse wrong version: OK (rejected)");
    Ok(())
}

/// Test extension header skipping.
#[allow(clippy::arithmetic_side_effects)]
fn test_extension_header_skip() -> KernelResult<()> {
    // Build a packet with a Hop-by-Hop extension header before the payload.
    // Hop-by-Hop: next_header=ICMPv6(58), hdr_ext_len=0 (8 bytes total), 6 pad bytes.
    let src = Ipv6Addr::LOOPBACK;
    let dst = Ipv6Addr::LOOPBACK;

    let upper_payload = b"upper-layer data";

    // Extension header: [next_header, hdr_ext_len, 6 padding bytes]
    let mut ext_and_payload = Vec::new();
    ext_and_payload.push(NH_ICMPV6); // Next header: ICMPv6
    ext_and_payload.push(0);          // hdr_ext_len=0 → 8 bytes total
    ext_and_payload.extend_from_slice(&[0; 6]); // Padding
    ext_and_payload.extend_from_slice(upper_payload);

    // The IPv6 header's next_header field points to Hop-by-Hop (0).
    let pkt = build_packet(src, dst, NH_HOP_BY_HOP, 64, &ext_and_payload);
    let parsed = Ipv6Packet::parse(&pkt)?;

    // The parser should have skipped the extension header.
    if parsed.upper_protocol != NH_ICMPV6 {
        crate::serial_println!(
            "[ipv6]   FAIL: upper_protocol = {}, expected {}",
            parsed.upper_protocol, NH_ICMPV6
        );
        return Err(KernelError::InternalError);
    }
    if parsed.payload != upper_payload {
        crate::serial_println!(
            "[ipv6]   FAIL: payload len = {}, expected {}",
            parsed.payload.len(), upper_payload.len()
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv6]   extension header skip: OK");
    Ok(())
}

/// Test that Fragment extension headers are parsed and exposed.
#[allow(clippy::arithmetic_side_effects)]
fn test_fragment_header_parse() -> KernelResult<()> {
    let src = Ipv6Addr::LOOPBACK;
    let dst = Ipv6Addr::LOOPBACK;

    let upper_payload = b"fragment data";

    // Build a Fragment extension header:
    // byte 0: Next Header = UDP (17)
    // byte 1: Reserved = 0
    // bytes 2-3: Fragment Offset = 0 (first fragment), Res=0, M=1
    //   offset 0 << 3 = 0, | M=1 = 0x0001
    // bytes 4-7: Identification = 0x12345678
    let mut ext_and_payload = Vec::new();
    ext_and_payload.push(NH_UDP);    // Next Header: UDP
    ext_and_payload.push(0);          // Reserved
    ext_and_payload.extend_from_slice(&0x0001u16.to_be_bytes()); // offset=0, M=1
    ext_and_payload.extend_from_slice(&0x12345678u32.to_be_bytes()); // ID
    ext_and_payload.extend_from_slice(upper_payload);

    // IPv6 header's next_header = NH_FRAGMENT (44).
    let pkt = build_packet(src, dst, NH_FRAGMENT, 64, &ext_and_payload);
    let parsed = Ipv6Packet::parse(&pkt)?;

    // upper_protocol should be UDP (from fragment header's Next Header).
    if parsed.upper_protocol != NH_UDP {
        crate::serial_println!(
            "[ipv6]   FAIL: fragment upper_protocol = {}, expected {}",
            parsed.upper_protocol, NH_UDP
        );
        return Err(KernelError::InternalError);
    }

    // Payload should be the data after the Fragment header.
    if parsed.payload != upper_payload {
        crate::serial_println!(
            "[ipv6]   FAIL: fragment payload len = {}, expected {}",
            parsed.payload.len(), upper_payload.len()
        );
        return Err(KernelError::InternalError);
    }

    // fragment_info should be Some with correct values.
    match parsed.fragment_info {
        Some((offset, more, id)) => {
            if offset != 0 {
                crate::serial_println!("[ipv6]   FAIL: frag offset = {}, expected 0", offset);
                return Err(KernelError::InternalError);
            }
            if !more {
                crate::serial_println!("[ipv6]   FAIL: frag M flag should be true");
                return Err(KernelError::InternalError);
            }
            if id != 0x12345678 {
                crate::serial_println!("[ipv6]   FAIL: frag id = 0x{:08X}, expected 0x12345678", id);
                return Err(KernelError::InternalError);
            }
        }
        None => {
            crate::serial_println!("[ipv6]   FAIL: fragment_info is None");
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[ipv6]   fragment header parse: OK");
    Ok(())
}

/// Test that an atomic fragment (offset=0, M=0) is detected.
#[allow(clippy::arithmetic_side_effects)]
fn test_atomic_fragment() -> KernelResult<()> {
    let src = Ipv6Addr::LOOPBACK;
    let dst = Ipv6Addr::LOOPBACK;

    let upper_payload = b"atomic frag";

    // Fragment header: offset=0, M=0 (atomic fragment, RFC 6946).
    let mut ext_and_payload = Vec::new();
    ext_and_payload.push(NH_ICMPV6); // Next Header
    ext_and_payload.push(0);          // Reserved
    ext_and_payload.extend_from_slice(&0x0000u16.to_be_bytes()); // offset=0, M=0
    ext_and_payload.extend_from_slice(&0x0000ABCDu32.to_be_bytes()); // ID
    ext_and_payload.extend_from_slice(upper_payload);

    let pkt = build_packet(src, dst, NH_FRAGMENT, 64, &ext_and_payload);
    let parsed = Ipv6Packet::parse(&pkt)?;

    // Should still detect the fragment header.
    match parsed.fragment_info {
        Some((offset, more, _id)) => {
            if offset != 0 || more {
                crate::serial_println!(
                    "[ipv6]   FAIL: atomic frag: offset={}, M={}",
                    offset, more
                );
                return Err(KernelError::InternalError);
            }
        }
        None => {
            crate::serial_println!("[ipv6]   FAIL: atomic fragment_info is None");
            return Err(KernelError::InternalError);
        }
    }

    // Upper protocol should be ICMPv6.
    if parsed.upper_protocol != NH_ICMPV6 {
        crate::serial_println!(
            "[ipv6]   FAIL: atomic frag upper = {}, expected {}",
            parsed.upper_protocol, NH_ICMPV6
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv6]   atomic fragment (RFC 6946): OK");
    Ok(())
}

/// Test multicast MAC mapping per RFC 2464.
fn test_multicast_mac_mapping() -> KernelResult<()> {
    // ff02::1 → 33:33:00:00:00:01
    let mac = multicast_mac(&Ipv6Addr::ALL_NODES_LINK_LOCAL);
    if mac.0 != [0x33, 0x33, 0x00, 0x00, 0x00, 0x01] {
        crate::serial_println!("[ipv6]   FAIL: ff02::1 MAC = {:?}", mac.0);
        return Err(KernelError::InternalError);
    }

    // ff02::1:ff12:3456 → 33:33:ff:12:34:56
    let snm = Ipv6Addr([
        0xFF, 0x02, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0x01, 0xFF, 0x12, 0x34, 0x56,
    ]);
    let mac2 = multicast_mac(&snm);
    if mac2.0 != [0x33, 0x33, 0xFF, 0x12, 0x34, 0x56] {
        crate::serial_println!("[ipv6]   FAIL: solicited-node MAC = {:?}", mac2.0);
        return Err(KernelError::InternalError);
    }

    // ff02::2 → 33:33:00:00:00:02
    let mac3 = multicast_mac(&Ipv6Addr::ALL_ROUTERS_LINK_LOCAL);
    if mac3.0 != [0x33, 0x33, 0x00, 0x00, 0x00, 0x02] {
        crate::serial_println!("[ipv6]   FAIL: ff02::2 MAC = {:?}", mac3.0);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv6]   multicast MAC mapping: OK");
    Ok(())
}

/// Test transport checksum compute + verify round-trip.
fn test_transport_checksum_roundtrip() -> KernelResult<()> {
    let src = Ipv6Addr::LOOPBACK;
    let dst = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);

    // Build a fake ICMPv6 echo request: type(1) + code(1) + cksum(2) + id(2) + seq(2) + data.
    let data = b"ping6 test";
    let total_len = 8 + data.len();
    let mut segment = Vec::with_capacity(total_len);
    segment.push(128); // Type: Echo Request
    segment.push(0);   // Code: 0
    segment.extend_from_slice(&[0, 0]); // Checksum placeholder
    segment.extend_from_slice(&0x1234u16.to_be_bytes()); // ID
    segment.extend_from_slice(&0x0001u16.to_be_bytes()); // Seq
    segment.extend_from_slice(data);

    // Compute checksum.
    let cksum = compute_transport_checksum(&src, &dst, NH_ICMPV6, &segment);

    // Write it in.
    segment[2] = (cksum >> 8) as u8;
    segment[3] = cksum as u8;

    // Verify.
    if !verify_transport_checksum(&src, &dst, NH_ICMPV6, &segment) {
        crate::serial_println!("[ipv6]   FAIL: transport checksum verify failed");
        return Err(KernelError::InternalError);
    }

    // Corrupt and verify rejection.
    let orig = segment[4];
    segment[4] ^= 0xFF;
    if verify_transport_checksum(&src, &dst, NH_ICMPV6, &segment) {
        crate::serial_println!("[ipv6]   FAIL: corrupted segment passed verification");
        return Err(KernelError::InternalError);
    }
    segment[4] = orig;

    crate::serial_println!("[ipv6]   transport checksum round-trip: OK");
    Ok(())
}

/// Test IPv6 address parsing from string.
fn test_ipv6_addr_parse() -> KernelResult<()> {
    use alloc::format;

    // Full form: 2001:0db8:0000:0000:0000:0000:0000:0001
    let full = Ipv6Addr::parse("2001:0db8:0000:0000:0000:0000:0000:0001");
    let expected = Ipv6Addr([0x20, 0x01, 0x0D, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    if full != Some(expected) {
        crate::serial_println!("[ipv6]   FAIL: full-form parse mismatch");
        return Err(KernelError::InternalError);
    }

    // Compressed: 2001:db8::1
    let compressed = Ipv6Addr::parse("2001:db8::1");
    if compressed != Some(expected) {
        crate::serial_println!("[ipv6]   FAIL: compressed parse mismatch");
        return Err(KernelError::InternalError);
    }

    // Loopback: ::1
    let lo = Ipv6Addr::parse("::1");
    if lo != Some(Ipv6Addr::LOOPBACK) {
        crate::serial_println!("[ipv6]   FAIL: loopback parse");
        return Err(KernelError::InternalError);
    }

    // Unspecified: ::
    let unspec = Ipv6Addr::parse("::");
    if unspec != Some(Ipv6Addr::UNSPECIFIED) {
        crate::serial_println!("[ipv6]   FAIL: unspecified parse");
        return Err(KernelError::InternalError);
    }

    // ff02::1 (all-nodes multicast)
    let allnodes = Ipv6Addr::parse("ff02::1");
    if allnodes != Some(Ipv6Addr::ALL_NODES_LINK_LOCAL) {
        crate::serial_println!("[ipv6]   FAIL: ff02::1 parse");
        return Err(KernelError::InternalError);
    }

    // Link-local: fe80::1
    let ll = Ipv6Addr::parse("fe80::1");
    let expected_ll = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    if ll != Some(expected_ll) {
        crate::serial_println!("[ipv6]   FAIL: fe80::1 parse");
        return Err(KernelError::InternalError);
    }
    // Verify link-local classification.
    if !ll.unwrap_or(Ipv6Addr::UNSPECIFIED).is_link_local() {
        crate::serial_println!("[ipv6]   FAIL: parsed fe80::1 not link-local");
        return Err(KernelError::InternalError);
    }

    // Round-trip: parse → Display → parse.
    let addr = Ipv6Addr::parse("2001:db8:85a3::8a2e:370:7334");
    if let Some(a) = addr {
        let s = format!("{}", a);
        let reparsed = Ipv6Addr::parse(&s);
        if reparsed != Some(a) {
            crate::serial_println!(
                "[ipv6]   FAIL: round-trip mismatch: '{}' → {:?}",
                s, reparsed
            );
            return Err(KernelError::InternalError);
        }
    } else {
        crate::serial_println!("[ipv6]   FAIL: failed to parse 2001:db8:85a3::8a2e:370:7334");
        return Err(KernelError::InternalError);
    }

    // Invalid inputs.
    if Ipv6Addr::parse("").is_some() {
        crate::serial_println!("[ipv6]   FAIL: empty string accepted");
        return Err(KernelError::InternalError);
    }
    if Ipv6Addr::parse("not:an:ipv6").is_some() {
        // This has fewer than 8 groups and no ::, so should fail.
        crate::serial_println!("[ipv6]   FAIL: 'not:an:ipv6' accepted");
        return Err(KernelError::InternalError);
    }
    if Ipv6Addr::parse("gggg::1").is_some() {
        crate::serial_println!("[ipv6]   FAIL: invalid hex 'gggg' accepted");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ipv6]   address parse: OK");
    Ok(())
}
