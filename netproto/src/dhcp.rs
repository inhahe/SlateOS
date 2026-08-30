//! DHCPv4 (RFC 2131 / RFC 2132) client message construction and parsing.
//!
//! Enough for a client to run the DISCOVER → OFFER → REQUEST → ACK exchange:
//! build DISCOVER/REQUEST messages and read the fields a client needs from an
//! OFFER/ACK (the offered address `yiaddr`, plus the common options — message
//! type, subnet mask, router, DNS, lease time, server id). Allocation-free:
//! messages are written into / borrowed from caller buffers. The message rides
//! inside a UDP datagram (client port 68, server port 67).

use crate::{Ipv4Addr, MacAddr};

/// BOOTP opcode: request (client → server).
pub const OP_REQUEST: u8 = 1;
/// BOOTP opcode: reply (server → client).
pub const OP_REPLY: u8 = 2;

const HTYPE_ETHERNET: u8 = 1;
const HLEN_ETHERNET: u8 = 6;

/// DHCP magic cookie preceding the options field (RFC 2132 §2).
pub const MAGIC_COOKIE: u32 = 0x6382_5363;

/// Offset of the options field (after the 236-byte fixed area + 4-byte cookie).
const OPTIONS_OFFSET: usize = 240;

// --- Option codes (RFC 2132) ------------------------------------------------
/// Option 1: subnet mask.
pub const OPT_SUBNET_MASK: u8 = 1;
/// Option 3: router (default gateway) list.
pub const OPT_ROUTER: u8 = 3;
/// Option 6: DNS server list.
pub const OPT_DNS: u8 = 6;
/// Option 50: requested IP address.
pub const OPT_REQUESTED_IP: u8 = 50;
/// Option 51: IP address lease time (seconds).
pub const OPT_LEASE_TIME: u8 = 51;
/// Option 53: DHCP message type.
pub const OPT_MSG_TYPE: u8 = 53;
/// Option 54: server identifier.
pub const OPT_SERVER_ID: u8 = 54;
/// Option 55: parameter request list.
pub const OPT_PARAM_REQUEST: u8 = 55;
const OPT_PAD: u8 = 0;
const OPT_END: u8 = 255;

// --- DHCP message types (option 53 values) ----------------------------------
/// DHCPDISCOVER.
pub const DISCOVER: u8 = 1;
/// DHCPOFFER.
pub const OFFER: u8 = 2;
/// DHCPREQUEST.
pub const REQUEST: u8 = 3;
/// DHCPDECLINE.
pub const DECLINE: u8 = 4;
/// DHCPACK.
pub const ACK: u8 = 5;
/// DHCPNAK.
pub const NAK: u8 = 6;
/// DHCPRELEASE.
pub const RELEASE: u8 = 7;

/// A borrowed, parsed DHCP message.
#[derive(Debug, Clone, Copy)]
pub struct Message<'a> {
    /// BOOTP opcode ([`OP_REQUEST`] / [`OP_REPLY`]).
    pub op: u8,
    /// Transaction id (echoes the client's `xid`).
    pub xid: u32,
    /// "Your" (client) IP address — the address the server is offering.
    pub yiaddr: Ipv4Addr,
    /// Next-server IP (`siaddr`).
    pub siaddr: Ipv4Addr,
    buf: &'a [u8],
}

impl<'a> Message<'a> {
    /// Parse a DHCP message. Returns `None` if the buffer is shorter than the
    /// fixed area + cookie, or the magic cookie is wrong.
    #[must_use]
    pub fn parse(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < OPTIONS_OFFSET {
            return None;
        }
        let cookie = u32::from_be_bytes([buf[236], buf[237], buf[238], buf[239]]);
        if cookie != MAGIC_COOKIE {
            return None;
        }
        let op = buf[0];
        let xid = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let yiaddr = [buf[16], buf[17], buf[18], buf[19]];
        let siaddr = [buf[20], buf[21], buf[22], buf[23]];
        Some(Message { op, xid, yiaddr, siaddr, buf })
    }

    /// Look up option `code`, returning its value bytes. Walks the TLV option
    /// area; `PAD` (0) bytes are skipped and `END` (255) terminates. All reads
    /// are bounds-checked (returns `None` past a malformed option).
    #[must_use]
    pub fn option(&self, code: u8) -> Option<&'a [u8]> {
        let mut i = OPTIONS_OFFSET;
        while i < self.buf.len() {
            let c = self.buf[i];
            if c == OPT_END {
                return None;
            }
            if c == OPT_PAD {
                i += 1;
                continue;
            }
            // TLV: code, length, value.
            let len = *self.buf.get(i + 1)? as usize;
            let val_start = i + 2;
            let val_end = val_start.checked_add(len)?;
            if val_end > self.buf.len() {
                return None;
            }
            if c == code {
                return Some(&self.buf[val_start..val_end]);
            }
            i = val_end;
        }
        None
    }

    /// DHCP message type (option 53), if present.
    #[must_use]
    pub fn msg_type(&self) -> Option<u8> {
        self.option(OPT_MSG_TYPE).and_then(|v| v.first().copied())
    }

    /// Read a 4-byte-address option (subnet mask, first router, first DNS, etc).
    #[must_use]
    fn addr_option(&self, code: u8) -> Option<Ipv4Addr> {
        let v = self.option(code)?;
        if v.len() < 4 {
            return None;
        }
        Some([v[0], v[1], v[2], v[3]])
    }

    /// Subnet mask (option 1).
    #[must_use]
    pub fn subnet_mask(&self) -> Option<Ipv4Addr> {
        self.addr_option(OPT_SUBNET_MASK)
    }

    /// First router / default gateway (option 3).
    #[must_use]
    pub fn router(&self) -> Option<Ipv4Addr> {
        self.addr_option(OPT_ROUTER)
    }

    /// First DNS server (option 6).
    #[must_use]
    pub fn dns(&self) -> Option<Ipv4Addr> {
        self.addr_option(OPT_DNS)
    }

    /// Server identifier (option 54).
    #[must_use]
    pub fn server_id(&self) -> Option<Ipv4Addr> {
        self.addr_option(OPT_SERVER_ID)
    }

    /// Lease time in seconds (option 51).
    #[must_use]
    pub fn lease_secs(&self) -> Option<u32> {
        let v = self.option(OPT_LEASE_TIME)?;
        if v.len() < 4 {
            return None;
        }
        Some(u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
    }
}

/// Write the fixed BOOTP area (through the magic cookie) for a client request,
/// zeroing the buffer first. Returns the offset where options begin, or `None`
/// if `out` can't hold the fixed area.
fn write_fixed(out: &mut [u8], xid: u32, chaddr: &MacAddr, broadcast: bool) -> Option<usize> {
    if out.len() < OPTIONS_OFFSET {
        return None;
    }
    for b in out[..OPTIONS_OFFSET].iter_mut() {
        *b = 0;
    }
    out[0] = OP_REQUEST;
    out[1] = HTYPE_ETHERNET;
    out[2] = HLEN_ETHERNET;
    // hops = 0
    out[4..8].copy_from_slice(&xid.to_be_bytes());
    // secs = 0
    if broadcast {
        out[10..12].copy_from_slice(&0x8000u16.to_be_bytes());
    }
    // ci/yi/si/gi addr all zero; chaddr in the first 6 of the 16-byte field.
    out[28..34].copy_from_slice(chaddr);
    out[236..240].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    Some(OPTIONS_OFFSET)
}

/// Append a TLV option; returns the new position or `None` on overflow.
fn push_option(out: &mut [u8], mut pos: usize, code: u8, val: &[u8]) -> Option<usize> {
    let end = pos.checked_add(2 + val.len())?;
    if end > out.len() || val.len() > u8::MAX as usize {
        return None;
    }
    out[pos] = code;
    out[pos + 1] = val.len() as u8;
    out[pos + 2..end].copy_from_slice(val);
    pos = end;
    Some(pos)
}

/// Append the `END` option; returns the total message length.
fn finish(out: &mut [u8], pos: usize) -> Option<usize> {
    if pos >= out.len() {
        return None;
    }
    out[pos] = OPT_END;
    pos.checked_add(1)
}

/// The parameter-request list a typical client asks for.
const PARAM_REQUEST_LIST: [u8; 4] =
    [OPT_SUBNET_MASK, OPT_ROUTER, OPT_DNS, OPT_LEASE_TIME];

/// Build a DHCPDISCOVER for MAC `chaddr` with transaction id `xid`, writing
/// into `out`. Returns the message length. The broadcast flag is set so the
/// server can reply before the client has an address.
#[must_use]
pub fn build_discover(out: &mut [u8], xid: u32, chaddr: &MacAddr) -> Option<usize> {
    let mut pos = write_fixed(out, xid, chaddr, true)?;
    pos = push_option(out, pos, OPT_MSG_TYPE, &[DISCOVER])?;
    pos = push_option(out, pos, OPT_PARAM_REQUEST, &PARAM_REQUEST_LIST)?;
    finish(out, pos)
}

/// Build a DHCPREQUEST accepting `requested_ip` from server `server_id`,
/// writing into `out`. Returns the message length.
#[must_use]
pub fn build_request(
    out: &mut [u8],
    xid: u32,
    chaddr: &MacAddr,
    requested_ip: &Ipv4Addr,
    server_id: &Ipv4Addr,
) -> Option<usize> {
    let mut pos = write_fixed(out, xid, chaddr, true)?;
    pos = push_option(out, pos, OPT_MSG_TYPE, &[REQUEST])?;
    pos = push_option(out, pos, OPT_REQUESTED_IP, requested_ip)?;
    pos = push_option(out, pos, OPT_SERVER_ID, server_id)?;
    pos = push_option(out, pos, OPT_PARAM_REQUEST, &PARAM_REQUEST_LIST)?;
    finish(out, pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: MacAddr = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

    #[test]
    fn discover_is_parseable() {
        let mut buf = [0u8; 300];
        let n = build_discover(&mut buf, 0xDEAD_BEEF, &MAC).unwrap();
        let m = Message::parse(&buf[..n]).unwrap();
        assert_eq!(m.op, OP_REQUEST);
        assert_eq!(m.xid, 0xDEAD_BEEF);
        assert_eq!(m.msg_type(), Some(DISCOVER));
        // chaddr landed in the right place.
        assert_eq!(&buf[28..34], &MAC);
        // Broadcast flag set.
        assert_eq!(&buf[10..12], &[0x80, 0x00]);
    }

    #[test]
    fn request_carries_requested_ip_and_server() {
        let mut buf = [0u8; 300];
        let n = build_request(&mut buf, 1, &MAC, &[10, 0, 2, 15], &[10, 0, 2, 2]).unwrap();
        let m = Message::parse(&buf[..n]).unwrap();
        assert_eq!(m.msg_type(), Some(REQUEST));
        assert_eq!(m.option(OPT_REQUESTED_IP), Some(&[10, 0, 2, 15][..]));
        assert_eq!(m.server_id(), Some([10, 0, 2, 2]));
    }

    /// Build a synthetic OFFER by hand: reply op, yiaddr, and a set of options.
    fn synth_offer() -> ([u8; 300], usize) {
        let mut buf = [0u8; 300];
        // Fixed area.
        buf[0] = OP_REPLY;
        buf[1] = HTYPE_ETHERNET;
        buf[2] = HLEN_ETHERNET;
        buf[4..8].copy_from_slice(&0x1234u32.to_be_bytes());
        buf[16..20].copy_from_slice(&[10, 0, 2, 15]); // yiaddr
        buf[236..240].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
        let mut pos = OPTIONS_OFFSET;
        pos = push_option(&mut buf, pos, OPT_MSG_TYPE, &[OFFER]).unwrap();
        pos = push_option(&mut buf, pos, OPT_SUBNET_MASK, &[255, 255, 255, 0]).unwrap();
        pos = push_option(&mut buf, pos, OPT_ROUTER, &[10, 0, 2, 2]).unwrap();
        pos = push_option(&mut buf, pos, OPT_DNS, &[8, 8, 8, 8]).unwrap();
        pos = push_option(&mut buf, pos, OPT_LEASE_TIME, &86400u32.to_be_bytes()).unwrap();
        pos = push_option(&mut buf, pos, OPT_SERVER_ID, &[10, 0, 2, 2]).unwrap();
        let n = finish(&mut buf, pos).unwrap();
        (buf, n)
    }

    #[test]
    fn parses_offer_fields() {
        let (buf, n) = synth_offer();
        let m = Message::parse(&buf[..n]).unwrap();
        assert_eq!(m.op, OP_REPLY);
        assert_eq!(m.msg_type(), Some(OFFER));
        assert_eq!(m.yiaddr, [10, 0, 2, 15]);
        assert_eq!(m.subnet_mask(), Some([255, 255, 255, 0]));
        assert_eq!(m.router(), Some([10, 0, 2, 2]));
        assert_eq!(m.dns(), Some([8, 8, 8, 8]));
        assert_eq!(m.lease_secs(), Some(86400));
        assert_eq!(m.server_id(), Some([10, 0, 2, 2]));
    }

    #[test]
    fn rejects_bad_cookie_and_short() {
        let (mut buf, n) = synth_offer();
        buf[236] ^= 0xFF; // corrupt cookie
        assert!(Message::parse(&buf[..n]).is_none());
        assert!(Message::parse(&[0u8; 100]).is_none());
    }

    #[test]
    fn missing_option_is_none() {
        let (buf, n) = synth_offer();
        let m = Message::parse(&buf[..n]).unwrap();
        // No requested-IP option in an OFFER.
        assert!(m.option(OPT_REQUESTED_IP).is_none());
    }

    #[test]
    fn tiny_buffer_rejected() {
        let mut out = [0u8; 100];
        assert!(build_discover(&mut out, 1, &MAC).is_none());
    }
}
