//! ICMPv4 (RFC 792) — enough to answer echo requests (ping).
//!
//! The header is 4 fixed bytes (type, code, checksum) followed by a 4-byte
//! rest-of-header. For echo messages that rest-of-header is the identifier and
//! sequence number, and the remainder of the packet is opaque data that a
//! reply must echo back verbatim.

use crate::checksum;

/// ICMP type: echo reply.
pub const TYPE_ECHO_REPLY: u8 = 0;
/// ICMP type: echo request.
pub const TYPE_ECHO_REQUEST: u8 = 8;

/// Length of the fixed ICMP header (type, code, checksum, rest-of-header).
pub const HEADER_LEN: usize = 8;

/// A borrowed, parsed ICMP echo request/reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Echo<'a> {
    /// True for a request (type 8), false for a reply (type 0).
    pub is_request: bool,
    /// Echo identifier.
    pub id: u16,
    /// Echo sequence number.
    pub seq: u16,
    /// Opaque payload that a reply must echo verbatim.
    pub data: &'a [u8],
}

impl<'a> Echo<'a> {
    /// Parse an ICMP echo request or reply from a full ICMP message. Returns
    /// `None` if too short, not an echo type, or the checksum does not verify.
    #[must_use]
    pub fn parse(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < HEADER_LEN {
            return None;
        }
        let ty = buf[0];
        let is_request = match ty {
            TYPE_ECHO_REQUEST => true,
            TYPE_ECHO_REPLY => false,
            _ => return None,
        };
        // Code must be 0 for echo.
        if buf[1] != 0 {
            return None;
        }
        if checksum::internet(buf) != 0 {
            return None;
        }
        let id = u16::from_be_bytes([buf[4], buf[5]]);
        let seq = u16::from_be_bytes([buf[6], buf[7]]);
        Some(Echo { is_request, id, seq, data: &buf[HEADER_LEN..] })
    }
}

/// Serialize an echo message (request or reply) into `out`, returning the
/// number of bytes written, or `None` if `out` cannot hold the header plus
/// `data`. The checksum is computed over the whole message.
#[must_use]
pub fn write_echo(out: &mut [u8], is_request: bool, id: u16, seq: u16, data: &[u8]) -> Option<usize> {
    let total = HEADER_LEN.checked_add(data.len())?;
    if out.len() < total {
        return None;
    }
    out[0] = if is_request { TYPE_ECHO_REQUEST } else { TYPE_ECHO_REPLY };
    out[1] = 0; // code
    out[2] = 0; // checksum placeholder
    out[3] = 0;
    out[4..6].copy_from_slice(&id.to_be_bytes());
    out[6..8].copy_from_slice(&seq.to_be_bytes());
    out[HEADER_LEN..total].copy_from_slice(data);
    let csum = checksum::internet(&out[..total]);
    out[2..4].copy_from_slice(&csum.to_be_bytes());
    Some(total)
}

/// Build an echo *reply* to a parsed echo *request*, writing into `out`.
/// Returns the number of bytes written, or `None` if `request` is not a
/// request or `out` is too small.
#[must_use]
pub fn reply_to(out: &mut [u8], request: &Echo) -> Option<usize> {
    if !request.is_request {
        return None;
    }
    write_echo(out, false, request.id, request.seq, request.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_parse_request() {
        let mut buf = [0u8; HEADER_LEN + 4];
        let n = write_echo(&mut buf, true, 0xABCD, 7, &[1, 2, 3, 4]).unwrap();
        assert_eq!(n, HEADER_LEN + 4);
        let e = Echo::parse(&buf).unwrap();
        assert!(e.is_request);
        assert_eq!(e.id, 0xABCD);
        assert_eq!(e.seq, 7);
        assert_eq!(e.data, &[1, 2, 3, 4]);
    }

    #[test]
    fn reply_echoes_id_seq_and_data() {
        let mut req = [0u8; HEADER_LEN + 3];
        write_echo(&mut req, true, 0x1111, 42, &[9, 8, 7]).unwrap();
        let parsed = Echo::parse(&req).unwrap();
        let mut rep = [0u8; HEADER_LEN + 3];
        reply_to(&mut rep, &parsed).unwrap();
        let r = Echo::parse(&rep).unwrap();
        assert!(!r.is_request);
        assert_eq!(r.id, 0x1111);
        assert_eq!(r.seq, 42);
        assert_eq!(r.data, &[9, 8, 7]);
    }

    #[test]
    fn reply_to_a_reply_is_none() {
        let mut rep = [0u8; HEADER_LEN];
        write_echo(&mut rep, false, 1, 1, &[]).unwrap();
        let parsed = Echo::parse(&rep).unwrap();
        let mut out = [0u8; HEADER_LEN];
        assert!(reply_to(&mut out, &parsed).is_none());
    }

    #[test]
    fn rejects_bad_checksum_and_type() {
        let mut buf = [0u8; HEADER_LEN];
        write_echo(&mut buf, true, 5, 5, &[]).unwrap();
        buf[4] ^= 0xFF; // corrupt id without fixing checksum
        assert!(Echo::parse(&buf).is_none());

        let mut buf2 = [0u8; HEADER_LEN];
        write_echo(&mut buf2, true, 5, 5, &[]).unwrap();
        buf2[0] = 3; // destination unreachable, not an echo
        assert!(Echo::parse(&buf2).is_none());
    }

    #[test]
    fn output_too_small_is_none() {
        let mut out = [0u8; 4];
        assert!(write_echo(&mut out, true, 0, 0, &[]).is_none());
    }
}
