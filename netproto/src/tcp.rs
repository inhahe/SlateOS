//! TCP (RFC 793) segment *header* parsing and construction over IPv4.
//!
//! This models the wire format only — parsing/serializing a segment header and
//! verifying/computing the pseudo-header checksum. The connection state machine
//! (SYN/ACK handshake, retransmission, congestion control) is deliberately out
//! of scope here; it belongs in the daemon/kernel that owns per-connection
//! state. Options (data offset > 5) are tolerated and exposed as a borrowed
//! slice rather than decoded.

use crate::checksum;
use crate::ipv4::PROTO_TCP;
use crate::Ipv4Addr;

/// Minimum TCP header length (no options), in bytes.
pub const MIN_HEADER_LEN: usize = 20;

/// TCP flag: FIN — no more data from sender.
pub const FLAG_FIN: u8 = 0x01;
/// TCP flag: SYN — synchronize sequence numbers.
pub const FLAG_SYN: u8 = 0x02;
/// TCP flag: RST — reset the connection.
pub const FLAG_RST: u8 = 0x04;
/// TCP flag: PSH — push buffered data to the application.
pub const FLAG_PSH: u8 = 0x08;
/// TCP flag: ACK — acknowledgement field is significant.
pub const FLAG_ACK: u8 = 0x10;
/// TCP flag: URG — urgent pointer field is significant.
pub const FLAG_URG: u8 = 0x20;

/// A borrowed, parsed TCP segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment<'a> {
    /// Source port.
    pub src_port: u16,
    /// Destination port.
    pub dst_port: u16,
    /// Sequence number.
    pub seq: u32,
    /// Acknowledgement number (meaningful only when [`FLAG_ACK`] is set).
    pub ack: u32,
    /// Control flags (FIN/SYN/RST/PSH/ACK/URG bits).
    pub flags: u8,
    /// Advertised receive window.
    pub window: u16,
    /// Urgent pointer (meaningful only when [`FLAG_URG`] is set).
    pub urgent: u16,
    /// Option bytes between the fixed header and the payload (may be empty).
    pub options: &'a [u8],
    /// Segment payload (after the header + options).
    pub payload: &'a [u8],
}

impl<'a> Segment<'a> {
    /// True if a given flag (e.g. [`FLAG_SYN`]) is set.
    #[must_use]
    pub fn has_flag(&self, flag: u8) -> bool {
        (self.flags & flag) != 0
    }
}

/// Accumulate the IPv4 TCP pseudo-header (src IP, dst IP, protocol, TCP length)
/// into a running checksum sum.
#[must_use]
fn pseudo_header_sum(src: &Ipv4Addr, dst: &Ipv4Addr, tcp_len: u16) -> u32 {
    let mut ph = [0u8; 12];
    ph[0..4].copy_from_slice(src);
    ph[4..8].copy_from_slice(dst);
    ph[8] = 0; // zero
    ph[9] = PROTO_TCP;
    ph[10..12].copy_from_slice(&tcp_len.to_be_bytes());
    checksum::accumulate(0, &ph)
}

impl<'a> Segment<'a> {
    /// Parse a TCP segment carried in an IPv4 packet. `src`/`dst` are the IPv4
    /// addresses (needed for the pseudo-header checksum). `buf` must be exactly
    /// the TCP bytes (header + options + payload), i.e. the IPv4 payload.
    /// Returns `None` on a short buffer, a bad data-offset, or a checksum that
    /// fails to verify.
    #[must_use]
    pub fn parse(buf: &'a [u8], src: &Ipv4Addr, dst: &Ipv4Addr) -> Option<Self> {
        if buf.len() < MIN_HEADER_LEN {
            return None;
        }
        let data_offset = (buf[12] >> 4) as usize;
        let header_len = data_offset.checked_mul(4)?;
        if header_len < MIN_HEADER_LEN || buf.len() < header_len {
            return None;
        }
        // Verify the checksum over the pseudo-header + the whole TCP segment.
        let sum = pseudo_header_sum(src, dst, buf.len() as u16);
        if checksum::internet_continue(sum, buf) != 0 {
            return None;
        }
        let src_port = u16::from_be_bytes([buf[0], buf[1]]);
        let dst_port = u16::from_be_bytes([buf[2], buf[3]]);
        let seq = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let ack = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let flags = buf[13];
        let window = u16::from_be_bytes([buf[14], buf[15]]);
        let urgent = u16::from_be_bytes([buf[18], buf[19]]);
        let options = &buf[MIN_HEADER_LEN..header_len];
        let payload = &buf[header_len..];
        Some(Segment {
            src_port,
            dst_port,
            seq,
            ack,
            flags,
            window,
            urgent,
            options,
            payload,
        })
    }
}

/// Fields for building a fixed-20-byte-header TCP segment (no options).
#[derive(Debug, Clone, Copy)]
pub struct Builder {
    /// Source port.
    pub src_port: u16,
    /// Destination port.
    pub dst_port: u16,
    /// Sequence number.
    pub seq: u32,
    /// Acknowledgement number.
    pub ack: u32,
    /// Control flags.
    pub flags: u8,
    /// Advertised receive window.
    pub window: u16,
}

impl Builder {
    /// Serialize the segment (20-byte header + `payload`) into `out`, computing
    /// the pseudo-header checksum from `src`/`dst`. Returns the number of bytes
    /// written, or `None` if `out` is too small.
    #[must_use]
    pub fn write(
        &self,
        out: &mut [u8],
        src: &Ipv4Addr,
        dst: &Ipv4Addr,
        payload: &[u8],
    ) -> Option<usize> {
        let total = MIN_HEADER_LEN.checked_add(payload.len())?;
        if total > u16::MAX as usize || out.len() < total {
            return None;
        }
        out[0..2].copy_from_slice(&self.src_port.to_be_bytes());
        out[2..4].copy_from_slice(&self.dst_port.to_be_bytes());
        out[4..8].copy_from_slice(&self.seq.to_be_bytes());
        out[8..12].copy_from_slice(&self.ack.to_be_bytes());
        out[12] = (5u8) << 4; // data offset 5 (20 bytes), reserved bits zero
        out[13] = self.flags;
        out[14..16].copy_from_slice(&self.window.to_be_bytes());
        out[16..18].copy_from_slice(&0u16.to_be_bytes()); // checksum placeholder
        out[18..20].copy_from_slice(&0u16.to_be_bytes()); // urgent pointer
        out[MIN_HEADER_LEN..total].copy_from_slice(payload);

        let sum = pseudo_header_sum(src, dst, total as u16);
        let csum = checksum::internet_continue(sum, &out[..total]);
        out[16..18].copy_from_slice(&csum.to_be_bytes());
        Some(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Ipv4Addr = [10, 0, 2, 15];
    const B: Ipv4Addr = [10, 0, 2, 2];

    #[test]
    fn write_then_parse_roundtrips() {
        let b = Builder {
            src_port: 40000,
            dst_port: 80,
            seq: 0x1234_5678,
            ack: 0x9ABC_DEF0,
            flags: FLAG_SYN | FLAG_ACK,
            window: 65535,
        };
        let mut buf = [0u8; MIN_HEADER_LEN + 4];
        let n = b.write(&mut buf, &A, &B, &[1, 2, 3, 4]).unwrap();
        assert_eq!(n, MIN_HEADER_LEN + 4);
        let s = Segment::parse(&buf, &A, &B).unwrap();
        assert_eq!(s.src_port, 40000);
        assert_eq!(s.dst_port, 80);
        assert_eq!(s.seq, 0x1234_5678);
        assert_eq!(s.ack, 0x9ABC_DEF0);
        assert!(s.has_flag(FLAG_SYN));
        assert!(s.has_flag(FLAG_ACK));
        assert!(!s.has_flag(FLAG_FIN));
        assert_eq!(s.window, 65535);
        assert!(s.options.is_empty());
        assert_eq!(s.payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn checksum_binds_to_addresses() {
        let b = Builder {
            src_port: 1,
            dst_port: 2,
            seq: 1,
            ack: 0,
            flags: FLAG_SYN,
            window: 1024,
        };
        let mut buf = [0u8; MIN_HEADER_LEN];
        b.write(&mut buf, &A, &B, &[]).unwrap();
        // Wrong source IP → checksum verification fails.
        assert!(Segment::parse(&buf, &[1, 1, 1, 1], &B).is_none());
        assert!(Segment::parse(&buf, &A, &B).is_some());
    }

    #[test]
    fn rejects_short_and_bad_offset() {
        assert!(Segment::parse(&[0u8; 10], &A, &B).is_none());
        let b = Builder {
            src_port: 1,
            dst_port: 2,
            seq: 0,
            ack: 0,
            flags: 0,
            window: 0,
        };
        let mut buf = [0u8; MIN_HEADER_LEN];
        b.write(&mut buf, &A, &B, &[]).unwrap();
        // Data offset 4 (< 20 bytes) is invalid.
        buf[12] = 4 << 4;
        assert!(Segment::parse(&buf, &A, &B).is_none());
    }

    #[test]
    fn corrupt_payload_fails_checksum() {
        let b = Builder {
            src_port: 1,
            dst_port: 2,
            seq: 0,
            ack: 0,
            flags: FLAG_PSH | FLAG_ACK,
            window: 512,
        };
        let mut buf = [0u8; MIN_HEADER_LEN + 3];
        b.write(&mut buf, &A, &B, &[9, 9, 9]).unwrap();
        buf[MIN_HEADER_LEN] ^= 0xFF; // mutate payload
        assert!(Segment::parse(&buf, &A, &B).is_none());
    }

    #[test]
    fn output_too_small_is_none() {
        let b = Builder {
            src_port: 1,
            dst_port: 2,
            seq: 0,
            ack: 0,
            flags: 0,
            window: 0,
        };
        let mut out = [0u8; 10];
        assert!(b.write(&mut out, &A, &B, &[]).is_none());
    }
}
