//! UDP (RFC 768) datagram parsing and construction over IPv4.
//!
//! The UDP checksum covers a pseudo-header derived from the IPv4 addresses and
//! protocol number, so building/verifying a datagram needs the source and
//! destination IPs in addition to the UDP bytes. The pseudo-header sum is
//! accumulated with [`crate::checksum::accumulate`] and folded together with
//! the header + payload.

use crate::checksum;
use crate::ipv4::PROTO_UDP;
use crate::Ipv4Addr;

/// Length of the fixed UDP header (src port, dst port, length, checksum).
pub const HEADER_LEN: usize = 8;

/// A borrowed, parsed UDP datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Datagram<'a> {
    /// Source port.
    pub src_port: u16,
    /// Destination port.
    pub dst_port: u16,
    /// Application payload (after the 8-byte header).
    pub payload: &'a [u8],
}

/// Accumulate the IPv4 UDP pseudo-header (src IP, dst IP, protocol, UDP length)
/// into a running checksum sum.
#[must_use]
fn pseudo_header_sum(src: &Ipv4Addr, dst: &Ipv4Addr, udp_len: u16) -> u32 {
    let mut ph = [0u8; 12];
    ph[0..4].copy_from_slice(src);
    ph[4..8].copy_from_slice(dst);
    ph[8] = 0; // zero
    ph[9] = PROTO_UDP;
    ph[10..12].copy_from_slice(&udp_len.to_be_bytes());
    checksum::accumulate(0, &ph)
}

impl<'a> Datagram<'a> {
    /// Parse a UDP datagram carried in an IPv4 packet. `src`/`dst` are the IPv4
    /// addresses (needed to verify the pseudo-header checksum). Returns `None`
    /// on a short buffer, a length field that disagrees with the buffer, or a
    /// non-zero checksum that fails to verify.
    ///
    /// A transmitted checksum of `0` means "no checksum" (RFC 768) and is
    /// accepted without verification.
    #[must_use]
    pub fn parse(buf: &'a [u8], src: &Ipv4Addr, dst: &Ipv4Addr) -> Option<Self> {
        if buf.len() < HEADER_LEN {
            return None;
        }
        let src_port = u16::from_be_bytes([buf[0], buf[1]]);
        let dst_port = u16::from_be_bytes([buf[2], buf[3]]);
        let length = u16::from_be_bytes([buf[4], buf[5]]) as usize;
        // The length field covers header + payload and must fit the buffer.
        if length < HEADER_LEN || length > buf.len() {
            return None;
        }
        let csum = u16::from_be_bytes([buf[6], buf[7]]);
        if csum != 0 {
            let sum = pseudo_header_sum(src, dst, length as u16);
            if checksum::internet_continue(sum, &buf[..length]) != 0 {
                return None;
            }
        }
        Some(Datagram { src_port, dst_port, payload: &buf[HEADER_LEN..length] })
    }
}

/// Serialize a UDP datagram (header + `payload`) into `out`, computing the
/// pseudo-header checksum from `src`/`dst`. Returns the number of bytes
/// written, or `None` if `out` is too small or the datagram would exceed the
/// 16-bit length field.
#[must_use]
pub fn write(
    out: &mut [u8],
    src: &Ipv4Addr,
    dst: &Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    payload: &[u8],
) -> Option<usize> {
    let total = HEADER_LEN.checked_add(payload.len())?;
    if total > u16::MAX as usize || out.len() < total {
        return None;
    }
    let total_u16 = total as u16;
    out[0..2].copy_from_slice(&src_port.to_be_bytes());
    out[2..4].copy_from_slice(&dst_port.to_be_bytes());
    out[4..6].copy_from_slice(&total_u16.to_be_bytes());
    out[6..8].copy_from_slice(&0u16.to_be_bytes()); // checksum placeholder
    out[HEADER_LEN..total].copy_from_slice(payload);

    let sum = pseudo_header_sum(src, dst, total_u16);
    let mut csum = checksum::internet_continue(sum, &out[..total]);
    // RFC 768: a computed checksum of zero is transmitted as all-ones so that
    // it isn't confused with "no checksum".
    if csum == 0 {
        csum = 0xFFFF;
    }
    out[6..8].copy_from_slice(&csum.to_be_bytes());
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Ipv4Addr = [10, 0, 2, 15];
    const B: Ipv4Addr = [10, 0, 2, 2];

    #[test]
    fn write_then_parse_roundtrips() {
        let mut buf = [0u8; HEADER_LEN + 5];
        let n = write(&mut buf, &A, &B, 5353, 53, &[1, 2, 3, 4, 5]).unwrap();
        assert_eq!(n, HEADER_LEN + 5);
        let d = Datagram::parse(&buf, &A, &B).unwrap();
        assert_eq!(d.src_port, 5353);
        assert_eq!(d.dst_port, 53);
        assert_eq!(d.payload, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn checksum_verifies_against_pseudo_header() {
        let mut buf = [0u8; HEADER_LEN + 3];
        write(&mut buf, &A, &B, 1234, 5678, &[9, 8, 7]).unwrap();
        // Wrong destination IP → pseudo-header differs → verification fails.
        assert!(Datagram::parse(&buf, &A, &[9, 9, 9, 9]).is_none());
        // Correct IPs still parse.
        assert!(Datagram::parse(&buf, &A, &B).is_some());
    }

    #[test]
    fn zero_checksum_is_accepted() {
        let mut buf = [0u8; HEADER_LEN + 2];
        write(&mut buf, &A, &B, 1, 2, &[0xAA, 0xBB]).unwrap();
        buf[6] = 0; // force "no checksum"
        buf[7] = 0;
        // Even with a wrong IP, a zero checksum skips verification.
        let d = Datagram::parse(&buf, &A, &[1, 1, 1, 1]).unwrap();
        assert_eq!(d.payload, &[0xAA, 0xBB]);
    }

    #[test]
    fn empty_payload_ok() {
        let mut buf = [0u8; HEADER_LEN];
        let n = write(&mut buf, &A, &B, 100, 200, &[]).unwrap();
        assert_eq!(n, HEADER_LEN);
        let d = Datagram::parse(&buf, &A, &B).unwrap();
        assert_eq!(d.src_port, 100);
        assert_eq!(d.dst_port, 200);
        assert!(d.payload.is_empty());
    }

    #[test]
    fn rejects_short_and_bad_length() {
        assert!(Datagram::parse(&[0u8; 4], &A, &B).is_none());
        let mut buf = [0u8; HEADER_LEN + 2];
        write(&mut buf, &A, &B, 1, 2, &[7, 7]).unwrap();
        // Corrupt the length field to claim more than the buffer holds.
        buf[4] = 0xFF;
        buf[5] = 0xFF;
        assert!(Datagram::parse(&buf, &A, &B).is_none());
    }

    #[test]
    fn output_too_small_is_none() {
        let mut out = [0u8; 4];
        assert!(write(&mut out, &A, &B, 1, 2, &[]).is_none());
    }
}
