//! IGMPv2 (RFC 2236) messages, and IPv4 multicast addressing.
//!
//! What a host needs to belong to an IPv4 multicast group on a LAN: say so
//! when it joins (a Membership Report, sent to the group), say so again when
//! a router asks (a Membership Query), and say it is leaving (a Leave Group,
//! sent to all routers). Each message is eight bytes, sent in an IPv4
//! datagram with TTL 1 and the Router Alert option
//! ([`crate::ipv4::Builder::build_header_router_alert`]); the group's
//! frames arrive at the Ethernet address [`multicast_mac`] names.
//!
//! IGMPv3 (RFC 3376) adds source filtering, which no caller asks for yet. A
//! v3 query is longer than eight bytes; [`parse`] reads its first eight as
//! the v2 query they begin with, which is how RFC 3376 §7.2.1 has a v2 host
//! answer one.

use crate::Ipv4Addr;
use crate::checksum;

/// IP protocol number of IGMP.
pub const PROTO_IGMP: u8 = 2;
/// Message type: Membership Query (general or group-specific).
pub const TYPE_QUERY: u8 = 0x11;
/// Message type: IGMPv1 Membership Report (accepted, never sent).
pub const TYPE_V1_REPORT: u8 = 0x12;
/// Message type: IGMPv2 Membership Report.
pub const TYPE_V2_REPORT: u8 = 0x16;
/// Message type: IGMPv2 Leave Group.
pub const TYPE_LEAVE: u8 = 0x17;
/// Message type: IGMPv3 Membership Report (accepted, never sent).
pub const TYPE_V3_REPORT: u8 = 0x22;
/// Length of an IGMPv2 message.
pub const MESSAGE_LEN: usize = 8;

/// The all-hosts group, 224.0.0.1: every multicast host belongs to it, and
/// general queries are sent to it. Never reported or left (RFC 2236 §6).
pub const ALL_HOSTS: Ipv4Addr = [224, 0, 0, 1];
/// The all-routers group, 224.0.0.2, where Leave Group messages go.
pub const ALL_ROUTERS: Ipv4Addr = [224, 0, 0, 2];

/// Whether `addr` is an IPv4 multicast (class D) address, 224.0.0.0/4.
#[must_use]
pub fn is_multicast(addr: &Ipv4Addr) -> bool {
    addr[0] & 0xF0 == 0xE0
}

/// The Ethernet address IPv4 multicast `group` is sent to: 01:00:5e and the
/// group's low 23 bits (RFC 1112 §6.4). 32 groups share each address, so a
/// receiver still checks the IP destination.
#[must_use]
pub fn multicast_mac(group: &Ipv4Addr) -> crate::MacAddr {
    [0x01, 0x00, 0x5e, group[1] & 0x7F, group[2], group[3]]
}

/// A parsed IGMP message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Message {
    /// The message type ([`TYPE_QUERY`], [`TYPE_V2_REPORT`], ...).
    pub kind: u8,
    /// For a query, how long a member may wait before reporting, in tenths
    /// of a second (0 in an IGMPv1 query: treat as 10 s, RFC 2236 §4).
    pub max_resp_time: u8,
    /// The group the message is about; 0.0.0.0 in a general query.
    pub group: Ipv4Addr,
}

impl Message {
    /// Whether this is a general query (about every group).
    #[must_use]
    pub fn is_general_query(&self) -> bool {
        self.kind == TYPE_QUERY && self.group == [0, 0, 0, 0]
    }
}

/// Parse an IGMP message from an IPv4 payload. `None` when it is shorter
/// than eight bytes or its checksum (over the whole payload) does not
/// verify. An IGMPv3 query, which is longer, is read as the v2 query its
/// first eight bytes are.
#[must_use]
pub fn parse(payload: &[u8]) -> Option<Message> {
    if payload.len() < MESSAGE_LEN || checksum::internet(payload) != 0 {
        return None;
    }
    Some(Message {
        kind: payload[0],
        max_resp_time: payload[1],
        group: [payload[4], payload[5], payload[6], payload[7]],
    })
}

/// Write the eight-byte IGMPv2 message `kind` about `group` into `out`, with
/// its checksum; `None` when `out` is shorter than eight bytes.
fn write(out: &mut [u8], kind: u8, group: &Ipv4Addr) -> Option<usize> {
    let msg = out.get_mut(..MESSAGE_LEN)?;
    msg[0] = kind;
    msg[1] = 0; // Max Resp Time: meaningful in queries only.
    msg[2] = 0;
    msg[3] = 0;
    msg[4..8].copy_from_slice(group);
    let csum = checksum::internet(msg);
    msg[2..4].copy_from_slice(&csum.to_be_bytes());
    Some(MESSAGE_LEN)
}

/// Write an IGMPv2 Membership Report for `group` into `out`, returning its
/// length. Send it to `group` itself (RFC 2236 §3).
#[must_use]
pub fn write_report(out: &mut [u8], group: &Ipv4Addr) -> Option<usize> {
    write(out, TYPE_V2_REPORT, group)
}

/// Write an IGMPv2 Leave Group for `group` into `out`, returning its length.
/// Send it to [`ALL_ROUTERS`] (RFC 2236 §3).
#[must_use]
pub fn write_leave(out: &mut [u8], group: &Ipv4Addr) -> Option<usize> {
    write(out, TYPE_LEAVE, group)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_round_trips_and_its_checksum_verifies() {
        let mut buf = [0u8; 16];
        let n = write_report(&mut buf, &[224, 0, 0, 251]).unwrap();
        assert_eq!(n, MESSAGE_LEN);
        assert_eq!(checksum::internet(&buf[..n]), 0);
        let m = parse(&buf[..n]).unwrap();
        assert_eq!(m.kind, TYPE_V2_REPORT);
        assert_eq!(m.group, [224, 0, 0, 251]);
        assert!(!m.is_general_query());
    }

    #[test]
    fn a_leave_names_its_group() {
        let mut buf = [0u8; MESSAGE_LEN];
        write_leave(&mut buf, &[239, 1, 2, 3]).unwrap();
        let m = parse(&buf).unwrap();
        assert_eq!((m.kind, m.group), (TYPE_LEAVE, [239, 1, 2, 3]));
    }

    #[test]
    fn known_report_bytes() {
        // A report for 224.0.0.251 (mDNS): 16 00 checksum e0 00 00 fb, where
        // the checksum is the complement of 0x1600 + 0xe000 + 0x00fb.
        let mut buf = [0u8; MESSAGE_LEN];
        write_report(&mut buf, &[224, 0, 0, 251]).unwrap();
        let sum: u32 = 0x1600 + 0xE000 + 0x00FB;
        let folded = ((sum & 0xFFFF) + (sum >> 16)) as u16;
        assert_eq!(
            buf,
            [0x16, 0, (!folded >> 8) as u8, !folded as u8, 224, 0, 0, 251]
        );
    }

    #[test]
    fn a_short_or_corrupt_message_is_refused() {
        let mut buf = [0u8; MESSAGE_LEN];
        write_report(&mut buf, &[224, 0, 0, 251]).unwrap();
        assert!(parse(&buf[..7]).is_none());
        let mut bad = buf;
        bad[7] ^= 1;
        assert!(parse(&bad).is_none());
        assert!(write_report(&mut [0u8; 7], &[224, 0, 0, 1]).is_none());
    }

    #[test]
    fn a_general_query_and_a_v3_query() {
        // General query, max resp time 10 s.
        let mut q = [TYPE_QUERY, 100, 0, 0, 0, 0, 0, 0];
        let c = checksum::internet(&q);
        q[2..4].copy_from_slice(&c.to_be_bytes());
        let m = parse(&q).unwrap();
        assert!(m.is_general_query());
        assert_eq!(m.max_resp_time, 100);

        // A v3 query: twelve bytes, the checksum over all of them.
        let mut v3 = [TYPE_QUERY, 50, 0, 0, 224, 0, 0, 251, 0x02, 125, 0, 0];
        let c = checksum::internet(&v3);
        v3[2..4].copy_from_slice(&c.to_be_bytes());
        let m = parse(&v3).unwrap();
        assert_eq!((m.kind, m.group), (TYPE_QUERY, [224, 0, 0, 251]));
        assert!(!m.is_general_query());
    }

    #[test]
    fn multicast_addresses_and_their_ethernet_addresses() {
        assert!(is_multicast(&[224, 0, 0, 251]));
        assert!(is_multicast(&[239, 255, 255, 250]));
        assert!(!is_multicast(&[223, 255, 255, 255]));
        assert!(!is_multicast(&[240, 0, 0, 1]));
        assert_eq!(
            multicast_mac(&[224, 0, 0, 251]),
            [0x01, 0x00, 0x5e, 0x00, 0x00, 0xfb]
        );
        // The 24th bit from the bottom is dropped: 239.129.1.2 and 239.1.1.2
        // share an Ethernet address.
        assert_eq!(
            multicast_mac(&[239, 129, 1, 2]),
            multicast_mac(&[239, 1, 1, 2])
        );
    }
}
