//! IPv4 (RFC 791) header parsing and construction.
//!
//! Only the fixed 20-byte header is modeled here; IHL > 5 (options) is parsed
//! far enough to locate the payload but the option bytes are exposed as a
//! borrowed slice rather than decoded. The header checksum is computed with
//! the shared [`crate::checksum`] implementation.

use crate::checksum;
use crate::Ipv4Addr;

/// Minimum IPv4 header length (no options), in bytes.
pub const MIN_HEADER_LEN: usize = 20;

/// IP protocol number: ICMP.
pub const PROTO_ICMP: u8 = 1;
/// IP protocol number: TCP.
pub const PROTO_TCP: u8 = 6;
/// IP protocol number: UDP.
pub const PROTO_UDP: u8 = 17;

/// A borrowed, parsed IPv4 datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet<'a> {
    /// Differentiated services / ECN byte (TOS).
    pub dscp_ecn: u8,
    /// Total length field (header + payload), as carried on the wire.
    pub total_len: u16,
    /// Identification field.
    pub id: u16,
    /// Flags (top 3 bits) and fragment offset (low 13 bits), host order.
    pub flags_frag: u16,
    /// Time to live.
    pub ttl: u8,
    /// Upper-layer protocol number (e.g. [`PROTO_ICMP`]).
    pub protocol: u8,
    /// Source address.
    pub src: Ipv4Addr,
    /// Destination address.
    pub dst: Ipv4Addr,
    /// Upper-layer payload (after the header + any options), clamped to
    /// `total_len` when that is shorter than the buffer.
    pub payload: &'a [u8],
}

impl<'a> Packet<'a> {
    /// True if the "don't fragment" flag is set.
    #[must_use]
    pub fn dont_fragment(&self) -> bool {
        (self.flags_frag & 0x4000) != 0
    }

    /// True if this is a fragment (MF set or a non-zero fragment offset).
    #[must_use]
    pub fn is_fragment(&self) -> bool {
        (self.flags_frag & 0x2000) != 0 || (self.flags_frag & 0x1FFF) != 0
    }

    /// Parse an IPv4 datagram. Returns `None` on a short buffer, wrong version,
    /// bad header length, or a header checksum that does not verify.
    #[must_use]
    pub fn parse(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < MIN_HEADER_LEN {
            return None;
        }
        let version = buf[0] >> 4;
        if version != 4 {
            return None;
        }
        let ihl = (buf[0] & 0x0F) as usize;
        let header_len = ihl.checked_mul(4)?;
        if header_len < MIN_HEADER_LEN || buf.len() < header_len {
            return None;
        }
        // Verify the header checksum over exactly the header bytes.
        if checksum::internet(&buf[..header_len]) != 0 {
            return None;
        }
        let dscp_ecn = buf[1];
        let total_len = u16::from_be_bytes([buf[2], buf[3]]);
        let id = u16::from_be_bytes([buf[4], buf[5]]);
        let flags_frag = u16::from_be_bytes([buf[6], buf[7]]);
        let ttl = buf[8];
        let protocol = buf[9];
        let src = [buf[12], buf[13], buf[14], buf[15]];
        let dst = [buf[16], buf[17], buf[18], buf[19]];
        // Clamp the payload to total_len when it is sane; otherwise use the
        // rest of the buffer. Never index past the validated bound.
        let total = total_len as usize;
        let end = if total >= header_len && total <= buf.len() { total } else { buf.len() };
        let payload = &buf[header_len..end];
        Some(Packet { dscp_ecn, total_len, id, flags_frag, ttl, protocol, src, dst, payload })
    }
}

/// Fields needed to build a fixed 20-byte IPv4 header.
#[derive(Debug, Clone, Copy)]
pub struct Builder {
    /// Differentiated services / ECN byte.
    pub dscp_ecn: u8,
    /// Identification field.
    pub id: u16,
    /// Flags (top 3 bits) and fragment offset (low 13 bits), host order.
    pub flags_frag: u16,
    /// Time to live.
    pub ttl: u8,
    /// Upper-layer protocol number.
    pub protocol: u8,
    /// Source address.
    pub src: Ipv4Addr,
    /// Destination address.
    pub dst: Ipv4Addr,
}

impl Builder {
    /// Build a header carrying `payload_len` bytes of upper-layer data,
    /// computing `total_len` and the header checksum. The returned header is
    /// always 20 bytes (no options emitted).
    #[must_use]
    pub fn build_header(&self, payload_len: u16) -> [u8; MIN_HEADER_LEN] {
        let mut h = [0u8; MIN_HEADER_LEN];
        h[0] = (4 << 4) | 5; // version 4, IHL 5 (20 bytes)
        h[1] = self.dscp_ecn;
        let total = (MIN_HEADER_LEN as u16).saturating_add(payload_len);
        h[2..4].copy_from_slice(&total.to_be_bytes());
        h[4..6].copy_from_slice(&self.id.to_be_bytes());
        h[6..8].copy_from_slice(&self.flags_frag.to_be_bytes());
        h[8] = self.ttl;
        h[9] = self.protocol;
        // h[10..12] checksum left zero for the computation below.
        h[12..16].copy_from_slice(&self.src);
        h[16..20].copy_from_slice(&self.dst);
        let csum = checksum::internet(&h);
        h[10..12].copy_from_slice(&csum.to_be_bytes());
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Ipv4Addr = [10, 0, 2, 15];
    const B: Ipv4Addr = [10, 0, 2, 2];

    #[test]
    fn build_then_parse_roundtrips() {
        let b = Builder {
            dscp_ecn: 0,
            id: 0x1234,
            flags_frag: 0x4000, // DF
            ttl: 64,
            protocol: PROTO_ICMP,
            src: A,
            dst: B,
        };
        let hdr = b.build_header(8);
        let mut buf = [0u8; MIN_HEADER_LEN + 8];
        buf[..MIN_HEADER_LEN].copy_from_slice(&hdr);
        buf[MIN_HEADER_LEN..].copy_from_slice(&[9, 8, 7, 6, 5, 4, 3, 2]);
        let p = Packet::parse(&buf).unwrap();
        assert_eq!(p.protocol, PROTO_ICMP);
        assert_eq!(p.src, A);
        assert_eq!(p.dst, B);
        assert_eq!(p.ttl, 64);
        assert_eq!(p.total_len, (MIN_HEADER_LEN + 8) as u16);
        assert!(p.dont_fragment());
        assert!(!p.is_fragment());
        assert_eq!(p.payload, &[9, 8, 7, 6, 5, 4, 3, 2]);
    }

    #[test]
    fn built_header_has_valid_checksum() {
        let b = Builder {
            dscp_ecn: 0,
            id: 1,
            flags_frag: 0,
            ttl: 64,
            protocol: PROTO_UDP,
            src: A,
            dst: B,
        };
        let hdr = b.build_header(0);
        // A valid header sums to zero when re-checksummed.
        assert_eq!(checksum::internet(&hdr), 0);
    }

    #[test]
    fn rejects_bad_version_and_short() {
        assert!(Packet::parse(&[0u8; 10]).is_none());
        let b = Builder {
            dscp_ecn: 0,
            id: 0,
            flags_frag: 0,
            ttl: 64,
            protocol: PROTO_ICMP,
            src: A,
            dst: B,
        };
        let mut hdr = b.build_header(0);
        hdr[0] = (6 << 4) | 5; // version 6
        assert!(Packet::parse(&hdr).is_none());
    }

    #[test]
    fn rejects_corrupt_checksum() {
        let b = Builder {
            dscp_ecn: 0,
            id: 0,
            flags_frag: 0,
            ttl: 64,
            protocol: PROTO_ICMP,
            src: A,
            dst: B,
        };
        let mut hdr = b.build_header(0);
        hdr[8] ^= 0xFF; // mutate TTL without fixing checksum
        assert!(Packet::parse(&hdr).is_none());
    }

    #[test]
    fn fragment_flags_detected() {
        let b = Builder {
            dscp_ecn: 0,
            id: 7,
            flags_frag: 0x2000 | 10, // MF set, offset 10
            ttl: 64,
            protocol: PROTO_UDP,
            src: A,
            dst: B,
        };
        let hdr = b.build_header(0);
        let p = Packet::parse(&hdr).unwrap();
        assert!(p.is_fragment());
        assert!(!p.dont_fragment());
    }
}
