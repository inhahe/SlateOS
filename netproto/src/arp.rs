//! ARP (Address Resolution Protocol, RFC 826) for IPv4-over-Ethernet.

use crate::ethernet::{self, ETHERTYPE_ARP};
use crate::{Ipv4Addr, MacAddr};

/// Length of an Ethernet/IPv4 ARP packet body (after the Ethernet header).
pub const PACKET_LEN: usize = 28;

/// Length of a complete ARP frame: Ethernet header + ARP body.
pub const FRAME_LEN: usize = ethernet::HEADER_LEN + PACKET_LEN;

const HW_TYPE_ETHERNET: u16 = 1;
const PROTO_TYPE_IPV4: u16 = 0x0800;

/// ARP operation code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `who-has` — request the MAC for a protocol address.
    Request,
    /// `is-at` — reply carrying the MAC for a protocol address.
    Reply,
}

impl Op {
    #[must_use]
    fn from_wire(v: u16) -> Option<Self> {
        match v {
            1 => Some(Op::Request),
            2 => Some(Op::Reply),
            _ => None,
        }
    }

    #[must_use]
    fn to_wire(self) -> u16 {
        match self {
            Op::Request => 1,
            Op::Reply => 2,
        }
    }
}

/// A parsed Ethernet/IPv4 ARP packet body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet {
    /// Operation (request or reply).
    pub op: Op,
    /// Sender hardware (MAC) address.
    pub sender_mac: MacAddr,
    /// Sender protocol (IPv4) address.
    pub sender_ip: Ipv4Addr,
    /// Target hardware (MAC) address.
    pub target_mac: MacAddr,
    /// Target protocol (IPv4) address.
    pub target_ip: Ipv4Addr,
}

impl Packet {
    /// Parse an ARP *body* (the bytes after the Ethernet header). Returns
    /// `None` if too short, not Ethernet/IPv4, or an unknown opcode.
    #[must_use]
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < PACKET_LEN {
            return None;
        }
        let htype = u16::from_be_bytes([buf[0], buf[1]]);
        let ptype = u16::from_be_bytes([buf[2], buf[3]]);
        let hlen = buf[4];
        let plen = buf[5];
        if htype != HW_TYPE_ETHERNET || ptype != PROTO_TYPE_IPV4 || hlen != 6 || plen != 4 {
            return None;
        }
        let op = Op::from_wire(u16::from_be_bytes([buf[6], buf[7]]))?;
        let mut sender_mac = [0u8; 6];
        let mut target_mac = [0u8; 6];
        sender_mac.copy_from_slice(&buf[8..14]);
        let sender_ip = [buf[14], buf[15], buf[16], buf[17]];
        target_mac.copy_from_slice(&buf[18..24]);
        let target_ip = [buf[24], buf[25], buf[26], buf[27]];
        Some(Packet { op, sender_mac, sender_ip, target_mac, target_ip })
    }

    /// Serialize just the 28-byte ARP body.
    #[must_use]
    pub fn to_body(&self) -> [u8; PACKET_LEN] {
        let mut b = [0u8; PACKET_LEN];
        b[0..2].copy_from_slice(&HW_TYPE_ETHERNET.to_be_bytes());
        b[2..4].copy_from_slice(&PROTO_TYPE_IPV4.to_be_bytes());
        b[4] = 6;
        b[5] = 4;
        b[6..8].copy_from_slice(&self.op.to_wire().to_be_bytes());
        b[8..14].copy_from_slice(&self.sender_mac);
        b[14..18].copy_from_slice(&self.sender_ip);
        b[18..24].copy_from_slice(&self.target_mac);
        b[24..28].copy_from_slice(&self.target_ip);
        b
    }

    /// Serialize a complete ARP frame (Ethernet header + body) into a fixed
    /// buffer. `eth_dst` is the L2 destination (broadcast for a request).
    #[must_use]
    pub fn to_frame(&self, eth_dst: &MacAddr) -> [u8; FRAME_LEN] {
        let mut f = [0u8; FRAME_LEN];
        ethernet::write_header(&mut f, eth_dst, &self.sender_mac, ETHERTYPE_ARP);
        f[ethernet::HEADER_LEN..].copy_from_slice(&self.to_body());
        f
    }
}

/// Build an ARP request ("who-has `target_ip`, tell `sender_ip`"), ready to
/// transmit. The Ethernet destination is broadcast and the target MAC is
/// zeroed, per convention.
#[must_use]
pub fn request(sender_mac: &MacAddr, sender_ip: &Ipv4Addr, target_ip: &Ipv4Addr) -> [u8; FRAME_LEN] {
    Packet {
        op: Op::Request,
        sender_mac: *sender_mac,
        sender_ip: *sender_ip,
        target_mac: [0u8; 6],
        target_ip: *target_ip,
    }
    .to_frame(&crate::BROADCAST_MAC)
}

/// Build an ARP reply answering `request` with our `own_mac`. Returns `None`
/// if the input is not a request. The reply is unicast to the requester.
#[must_use]
pub fn reply_to(request: &Packet, own_mac: &MacAddr) -> Option<[u8; FRAME_LEN]> {
    if request.op != Op::Request {
        return None;
    }
    Some(
        Packet {
            op: Op::Reply,
            sender_mac: *own_mac,
            sender_ip: request.target_ip,
            target_mac: request.sender_mac,
            target_ip: request.sender_ip,
        }
        .to_frame(&request.sender_mac),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ethernet::Frame;

    const MY_MAC: MacAddr = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    const MY_IP: Ipv4Addr = [10, 0, 2, 15];
    const GW_MAC: MacAddr = [0x52, 0x55, 0x0a, 0x00, 0x02, 0x02];
    const GW_IP: Ipv4Addr = [10, 0, 2, 2];

    #[test]
    fn request_roundtrips_through_ethernet() {
        let frame = request(&MY_MAC, &MY_IP, &GW_IP);
        let eth = Frame::parse(&frame).unwrap();
        assert!(eth.is_broadcast());
        assert_eq!(eth.ethertype, ETHERTYPE_ARP);
        let p = Packet::parse(eth.payload).unwrap();
        assert_eq!(p.op, Op::Request);
        assert_eq!(p.sender_mac, MY_MAC);
        assert_eq!(p.sender_ip, MY_IP);
        assert_eq!(p.target_ip, GW_IP);
        assert_eq!(p.target_mac, [0u8; 6]);
    }

    #[test]
    fn reply_targets_requester() {
        // Gateway asks who-has MY_IP.
        let req = Packet {
            op: Op::Request,
            sender_mac: GW_MAC,
            sender_ip: GW_IP,
            target_mac: [0u8; 6],
            target_ip: MY_IP,
        };
        let frame = reply_to(&req, &MY_MAC).unwrap();
        let eth = Frame::parse(&frame).unwrap();
        assert_eq!(eth.dst, GW_MAC); // unicast back to requester
        assert_eq!(eth.src, MY_MAC);
        let p = Packet::parse(eth.payload).unwrap();
        assert_eq!(p.op, Op::Reply);
        assert_eq!(p.sender_mac, MY_MAC);
        assert_eq!(p.sender_ip, MY_IP);
        assert_eq!(p.target_mac, GW_MAC);
        assert_eq!(p.target_ip, GW_IP);
    }

    #[test]
    fn reply_to_a_reply_is_none() {
        let rep = Packet {
            op: Op::Reply,
            sender_mac: GW_MAC,
            sender_ip: GW_IP,
            target_mac: MY_MAC,
            target_ip: MY_IP,
        };
        assert!(reply_to(&rep, &MY_MAC).is_none());
    }

    #[test]
    fn rejects_bad_fields() {
        let mut body = Packet {
            op: Op::Request,
            sender_mac: MY_MAC,
            sender_ip: MY_IP,
            target_mac: [0u8; 6],
            target_ip: GW_IP,
        }
        .to_body();
        // Corrupt hardware length.
        body[4] = 8;
        assert!(Packet::parse(&body).is_none());
        // Too short.
        assert!(Packet::parse(&body[..10]).is_none());
    }
}
