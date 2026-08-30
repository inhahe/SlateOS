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

/// Accumulate the IPv4 upper-layer pseudo-header (src, dst, protocol,
/// upper-layer length) into a running checksum sum, per RFC 793 §3.1 and
/// RFC 768. Use with [`crate::checksum::internet_continue`] to checksum
/// TCP/UDP over IPv4.
///
/// This is the v4 counterpart of [`crate::ipv6::pseudo_header_sum`], which has
/// been public and shared by `tcp`, `udp` and `icmpv6` since it was written.
/// The v4 side predates it and was instead written out twice — privately in
/// `tcp.rs` and again in `udp.rs`, identical apart from the protocol byte each
/// hardcoded. Lane A found the same duplication in the kernel's own net stack,
/// at seven copies rather than two, and asked for this function so the kernel
/// can delete its copies and depend on this crate instead
/// (`requests/a-c-netproto-checksum-already-owns-what-the-kernel-just-reunified.md`).
///
/// # Argument order
///
/// `(upper_len, protocol)` and not `(protocol, upper_len)`, for two reasons.
/// It matches [`crate::ipv6::pseudo_header_sum`], so the two do not differ in
/// a way a reader has to remember; and because `u16` does not coerce to `u8`,
/// a call site that swaps them does not compile. With both as integers in the
/// other order, `pseudo_header_sum(src, dst, 6, 20)` and
/// `pseudo_header_sum(src, dst, 20, 6)` would both build — one meaning "TCP,
/// 20 bytes" and the other "protocol 20, 6 bytes" — and the only symptom would
/// be a checksum that never verifies.
///
/// # What this does not bind
///
/// The sum is unchanged by exchanging `src` and `dst`. Both addresses occupy
/// whole aligned 16-bit words, and the Internet checksum is a commutative sum
/// over those words, so a swap merely reorders four addends. The direction of
/// a flow is distinguished by the port numbers, which live in the checksummed
/// header itself. This is a property of RFC 1071, not a shortcut taken here —
/// [`crate::ipv6::pseudo_header_sum`] has it too — but it is easy to assume
/// away, so it is pinned by
/// `ipv4::tests::the_checksum_cannot_see_a_source_destination_swap`.
#[must_use]
pub fn pseudo_header_sum(src: &Ipv4Addr, dst: &Ipv4Addr, upper_len: u16, protocol: u8) -> u32 {
    let mut ph = [0u8; 12];
    ph[0..4].copy_from_slice(src);
    ph[4..8].copy_from_slice(dst);
    // ph[8] is the mandatory zero byte.
    ph[9] = protocol;
    ph[10..12].copy_from_slice(&upper_len.to_be_bytes());
    checksum::accumulate(0, &ph)
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

    /// The pseudo-header is the twelve bytes RFC 793 §3.1 draws, in that order.
    ///
    /// Asserted against a hand-built buffer rather than against another call
    /// to the same function, because the point of the shared version is that
    /// the *layout* is stated once — and a layout that agrees with itself is
    /// no evidence at all. A checksum computed over a wrong-but-consistent
    /// pseudo-header verifies perfectly between two Slate machines and is
    /// rejected by everything else on the network, which is the worst
    /// available failure: it looks like an interop problem in the peer.
    #[test]
    fn the_pseudo_header_is_the_twelve_bytes_rfc_793_draws() {
        let expected: [u8; 12] = [
            10, 0, 2, 15, // source address
            10, 0, 2, 2, // destination address
            0,    // mandatory zero
            PROTO_TCP, // protocol
            0x01, 0x2c, // upper-layer length, 300, big-endian
        ];
        assert_eq!(
            pseudo_header_sum(&A, &B, 300, PROTO_TCP),
            checksum::accumulate(0, &expected)
        );
    }

    /// A third address, distinct from both [`A`] and [`B`], for the tests that
    /// need "a different host" rather than "the other end".
    const C: Ipv4Addr = [192, 168, 1, 1];

    /// Changing any one input changes the sum.
    ///
    /// The pseudo-header exists to bind a segment to the addresses and
    /// protocol it was sent under; a sum that ignored one of its inputs would
    /// still verify against itself and would accept a segment redirected from
    /// another connection.
    #[test]
    fn every_input_reaches_the_sum() {
        let base = checksum::fold(pseudo_header_sum(&A, &B, 300, PROTO_TCP));
        assert_ne!(base, checksum::fold(pseudo_header_sum(&C, &B, 300, PROTO_TCP)));
        assert_ne!(base, checksum::fold(pseudo_header_sum(&A, &C, 300, PROTO_TCP)));
        assert_ne!(base, checksum::fold(pseudo_header_sum(&A, &B, 301, PROTO_TCP)));
        assert_ne!(base, checksum::fold(pseudo_header_sum(&A, &B, 300, PROTO_UDP)));
    }

    /// Swapping source and destination leaves the sum unchanged, and that is a
    /// property of the Internet checksum rather than a defect here.
    ///
    /// The sum is over 16-bit words, and both addresses occupy whole aligned
    /// pairs of them, so exchanging the two exchanges four addends in a
    /// commutative sum. Nothing a pseudo-header can do would fix that; the
    /// direction of a flow is distinguished by the port numbers, which are
    /// inside the checksummed header and *are* asymmetric.
    ///
    /// Asserted rather than merely written down because the natural way to
    /// test "the addresses reach the sum" is to swap them, that test passes on
    /// every *other* field, and it would look like a bug in this function
    /// rather than a fact about the algorithm. It cost one debugging cycle
    /// here; pinning it means it costs nobody else one.
    #[test]
    fn the_checksum_cannot_see_a_source_destination_swap() {
        assert_eq!(
            pseudo_header_sum(&A, &B, 300, PROTO_TCP),
            pseudo_header_sum(&B, &A, 300, PROTO_TCP)
        );
    }

    /// TCP and UDP agree with the shared function they now both call.
    ///
    /// This is the regression that would fire if either module grew its own
    /// copy back. `tcp::pseudo_header_sum` and `udp::pseudo_header_sum` are
    /// private, so they are reached here through the parse/build round trip
    /// each performs: a segment built with one layout and parsed with another
    /// fails its checksum.
    #[test]
    fn tcp_and_udp_checksums_still_verify_over_ipv4() {
        let payload = [1u8, 2, 3, 4, 5, 6, 7];
        let seg = crate::tcp::Builder {
            src_port: 1234,
            dst_port: 80,
            seq: 42,
            ack: 0,
            flags: crate::tcp::FLAG_SYN,
            window: 8192,
        };
        let mut buf = [0u8; 64];
        let n = seg
            .write(&mut buf, &A, &B, &payload)
            .expect("fits in 64 bytes");
        assert!(crate::tcp::Segment::parse(&buf[..n], &A, &B).is_some());
        // A segment presented as coming from a *different host* must not
        // verify: that is the pseudo-header doing its job. Note this is a
        // third address and not the pair swapped -- see
        // `the_checksum_cannot_see_a_source_destination_swap`.
        assert!(crate::tcp::Segment::parse(&buf[..n], &C, &B).is_none());

        let mut ubuf = [0u8; 64];
        let un = crate::udp::write(&mut ubuf, &A, &B, 5353, 53, &payload)
            .expect("fits in 64 bytes");
        assert!(crate::udp::Datagram::parse(&ubuf[..un], &A, &B).is_some());
        assert!(crate::udp::Datagram::parse(&ubuf[..un], &C, &B).is_none());
    }
}
