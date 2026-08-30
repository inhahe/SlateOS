//! IPv6 (RFC 8200) base-header parsing and construction.
//!
//! Only the fixed 40-byte base header is modeled. Extension headers (the
//! chain selected by `next_header`) are *not* decoded here — `payload` is the
//! bytes immediately after the base header, and `next_header` tells the caller
//! what the first of them is. IPv6 has no header checksum (RFC 8200 §3), so
//! there is nothing to verify at this layer; upper-layer checksums use the
//! IPv6 pseudo-header (see [`pseudo_header_sum`]).

use crate::checksum;

/// Length of the fixed IPv6 base header.
pub const HEADER_LEN: usize = 40;

/// A 16-byte IPv6 address.
pub type Ipv6Addr = [u8; 16];

/// A borrowed, parsed IPv6 datagram (base header only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet<'a> {
    /// Traffic class (DSCP + ECN), reconstructed from the version/class/label
    /// word.
    pub traffic_class: u8,
    /// 20-bit flow label.
    pub flow_label: u32,
    /// Payload length field (bytes after the base header, per the wire).
    pub payload_len: u16,
    /// Next-header value (upper-layer protocol or first extension header).
    pub next_header: u8,
    /// Hop limit (the IPv6 analogue of IPv4 TTL).
    pub hop_limit: u8,
    /// Source address.
    pub src: Ipv6Addr,
    /// Destination address.
    pub dst: Ipv6Addr,
    /// Bytes after the base header, clamped to `payload_len` when that fits.
    pub payload: &'a [u8],
}

impl<'a> Packet<'a> {
    /// Parse an IPv6 datagram. Returns `None` on a short buffer or wrong
    /// version. There is no header checksum to verify.
    #[must_use]
    pub fn parse(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < HEADER_LEN {
            return None;
        }
        let version = buf[0] >> 4;
        if version != 6 {
            return None;
        }
        let vcf = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let traffic_class = ((vcf >> 20) & 0xFF) as u8;
        let flow_label = vcf & 0x000F_FFFF;
        let payload_len = u16::from_be_bytes([buf[4], buf[5]]);
        let next_header = buf[6];
        let hop_limit = buf[7];
        let mut src = [0u8; 16];
        let mut dst = [0u8; 16];
        src.copy_from_slice(&buf[8..24]);
        dst.copy_from_slice(&buf[24..40]);
        let rest = &buf[HEADER_LEN..];
        let plen = payload_len as usize;
        let payload = if plen <= rest.len() { &rest[..plen] } else { rest };
        Some(Packet {
            traffic_class,
            flow_label,
            payload_len,
            next_header,
            hop_limit,
            src,
            dst,
            payload,
        })
    }
}

/// Fields for building an IPv6 base header.
#[derive(Debug, Clone, Copy)]
pub struct Builder {
    /// Traffic class (DSCP + ECN).
    pub traffic_class: u8,
    /// 20-bit flow label (only the low 20 bits are used).
    pub flow_label: u32,
    /// Next-header value (upper-layer protocol number).
    pub next_header: u8,
    /// Hop limit.
    pub hop_limit: u8,
    /// Source address.
    pub src: Ipv6Addr,
    /// Destination address.
    pub dst: Ipv6Addr,
}

impl Builder {
    /// Build a 40-byte base header carrying `payload_len` bytes of upper-layer
    /// data.
    #[must_use]
    pub fn build_header(&self, payload_len: u16) -> [u8; HEADER_LEN] {
        let mut h = [0u8; HEADER_LEN];
        let vcf: u32 = (6u32 << 28)
            | ((self.traffic_class as u32) << 20)
            | (self.flow_label & 0x000F_FFFF);
        h[0..4].copy_from_slice(&vcf.to_be_bytes());
        h[4..6].copy_from_slice(&payload_len.to_be_bytes());
        h[6] = self.next_header;
        h[7] = self.hop_limit;
        h[8..24].copy_from_slice(&self.src);
        h[24..40].copy_from_slice(&self.dst);
        h
    }
}

/// Accumulate the IPv6 upper-layer pseudo-header (src, dst, upper-layer packet
/// length, next-header) into a running checksum sum, per RFC 8200 §8.1. Use
/// with [`crate::checksum::internet_continue`] to checksum TCP/UDP/ICMPv6 over
/// IPv6.
#[must_use]
pub fn pseudo_header_sum(src: &Ipv6Addr, dst: &Ipv6Addr, upper_len: u32, next_header: u8) -> u32 {
    let mut sum = checksum::accumulate(0, src);
    sum = checksum::accumulate(sum, dst);
    let mut tail = [0u8; 8];
    tail[0..4].copy_from_slice(&upper_len.to_be_bytes());
    // tail[4..7] are zero; tail[7] is the next-header value.
    tail[7] = next_header;
    checksum::accumulate(sum, &tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Ipv6Addr = [
        0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
    ];
    const B: Ipv6Addr = [
        0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02,
    ];

    #[test]
    fn build_then_parse_roundtrips() {
        let b = Builder {
            traffic_class: 0x28,
            flow_label: 0x0F_ABCD,
            next_header: 58, // ICMPv6
            hop_limit: 64,
            src: A,
            dst: B,
        };
        let hdr = b.build_header(4);
        let mut buf = [0u8; HEADER_LEN + 4];
        buf[..HEADER_LEN].copy_from_slice(&hdr);
        buf[HEADER_LEN..].copy_from_slice(&[1, 2, 3, 4]);
        let p = Packet::parse(&buf).unwrap();
        assert_eq!(p.traffic_class, 0x28);
        assert_eq!(p.flow_label, 0x0F_ABCD);
        assert_eq!(p.next_header, 58);
        assert_eq!(p.hop_limit, 64);
        assert_eq!(p.src, A);
        assert_eq!(p.dst, B);
        assert_eq!(p.payload_len, 4);
        assert_eq!(p.payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn rejects_bad_version_and_short() {
        assert!(Packet::parse(&[0u8; 20]).is_none());
        let b = Builder {
            traffic_class: 0,
            flow_label: 0,
            next_header: 17,
            hop_limit: 64,
            src: A,
            dst: B,
        };
        let mut hdr = b.build_header(0);
        hdr[0] = (4 << 4) | (hdr[0] & 0x0F); // version 4
        assert!(Packet::parse(&hdr).is_none());
    }

    #[test]
    fn payload_clamped_to_length_field() {
        let b = Builder {
            traffic_class: 0,
            flow_label: 0,
            next_header: 17,
            hop_limit: 64,
            src: A,
            dst: B,
        };
        let hdr = b.build_header(2); // claims 2 bytes of payload
        let mut buf = [0u8; HEADER_LEN + 5]; // but 5 trailing bytes present
        buf[..HEADER_LEN].copy_from_slice(&hdr);
        buf[HEADER_LEN..].copy_from_slice(&[1, 2, 3, 4, 5]);
        let p = Packet::parse(&buf).unwrap();
        assert_eq!(p.payload, &[1, 2]); // clamped to payload_len
    }

    #[test]
    fn pseudo_header_reflects_fields() {
        let base = checksum::fold(pseudo_header_sum(&A, &B, 8, 17));
        // Different next-header changes the sum.
        assert_ne!(base, checksum::fold(pseudo_header_sum(&A, &B, 8, 6)));
        // Different upper-layer length changes the sum.
        assert_ne!(base, checksum::fold(pseudo_header_sum(&A, &B, 16, 17)));
        // A genuinely different address (not just a src/dst swap, which is
        // commutative under one's-complement addition) changes the sum.
        let mut c = A;
        c[0] ^= 0xFF;
        assert_ne!(base, checksum::fold(pseudo_header_sum(&c, &B, 8, 17)));
    }
}
