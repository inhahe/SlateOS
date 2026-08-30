//! Ethernet II frame parsing and header construction.

use crate::MacAddr;

/// Size of the Ethernet II header (dst MAC + src MAC + EtherType).
pub const HEADER_LEN: usize = 14;

/// EtherType: IPv4.
pub const ETHERTYPE_IPV4: u16 = 0x0800;
/// EtherType: ARP.
pub const ETHERTYPE_ARP: u16 = 0x0806;
/// EtherType: IPv6.
pub const ETHERTYPE_IPV6: u16 = 0x86DD;

/// A borrowed, parsed Ethernet II frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    /// Destination MAC address.
    pub dst: MacAddr,
    /// Source MAC address.
    pub src: MacAddr,
    /// EtherType (host order).
    pub ethertype: u16,
    /// Payload following the 14-byte header (no FCS).
    pub payload: &'a [u8],
}

impl<'a> Frame<'a> {
    /// Parse an Ethernet II frame. Returns `None` if the buffer is shorter
    /// than the 14-byte header.
    #[must_use]
    pub fn parse(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < HEADER_LEN {
            return None;
        }
        let mut dst = [0u8; 6];
        let mut src = [0u8; 6];
        dst.copy_from_slice(&buf[0..6]);
        src.copy_from_slice(&buf[6..12]);
        let ethertype = u16::from_be_bytes([buf[12], buf[13]]);
        Some(Frame { dst, src, ethertype, payload: &buf[HEADER_LEN..] })
    }

    /// True if `dst` is the broadcast address.
    #[must_use]
    pub fn is_broadcast(&self) -> bool {
        self.dst == crate::BROADCAST_MAC
    }

    /// True if `dst` is a group (multicast/broadcast) address — low bit of the
    /// first octet set (IEEE 802.3 §3.2.3).
    #[must_use]
    pub fn is_multicast(&self) -> bool {
        (self.dst[0] & 0x01) != 0
    }
}

/// Write a 14-byte Ethernet II header into `out[..14]`.
///
/// # Panics
/// Panics if `out` is shorter than [`HEADER_LEN`]. Callers own the buffer, so
/// this is a programming error, not attacker-reachable.
pub fn write_header(out: &mut [u8], dst: &MacAddr, src: &MacAddr, ethertype: u16) {
    out[0..6].copy_from_slice(dst);
    out[6..12].copy_from_slice(src);
    out[12..14].copy_from_slice(&ethertype.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: MacAddr = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    const B: MacAddr = [0x52, 0x54, 0x00, 0xAB, 0xCD, 0xEF];

    #[test]
    fn roundtrip() {
        let mut buf = [0u8; HEADER_LEN + 4];
        write_header(&mut buf, &A, &B, ETHERTYPE_IPV4);
        buf[HEADER_LEN..].copy_from_slice(&[1, 2, 3, 4]);
        let f = Frame::parse(&buf).unwrap();
        assert_eq!(f.dst, A);
        assert_eq!(f.src, B);
        assert_eq!(f.ethertype, ETHERTYPE_IPV4);
        assert_eq!(f.payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn short_buffer_rejected() {
        assert!(Frame::parse(&[0u8; 13]).is_none());
    }

    #[test]
    fn broadcast_and_multicast() {
        let mut buf = [0u8; HEADER_LEN];
        write_header(&mut buf, &crate::BROADCAST_MAC, &A, ETHERTYPE_ARP);
        let f = Frame::parse(&buf).unwrap();
        assert!(f.is_broadcast());
        assert!(f.is_multicast());

        write_header(&mut buf, &[0x01, 0, 0x5e, 0, 0, 1], &A, ETHERTYPE_IPV4);
        let f = Frame::parse(&buf).unwrap();
        assert!(!f.is_broadcast());
        assert!(f.is_multicast());

        write_header(&mut buf, &A, &B, ETHERTYPE_IPV4);
        let f = Frame::parse(&buf).unwrap();
        assert!(!f.is_broadcast());
        assert!(!f.is_multicast());
    }
}
