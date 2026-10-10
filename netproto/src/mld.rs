//! MLDv1 (RFC 2710) messages, and IPv6 multicast addressing: IGMP's
//! counterpart for IPv6 ([`crate::igmp`]).
//!
//! A host listening to an IPv6 multicast group says so when it starts (a
//! Multicast Listener Report, sent to the group), again when a router asks (a
//! Multicast Listener Query), and when it stops (Multicast Listener Done, sent
//! to all routers, `ff02::2`). Each message is a 24-byte ICMPv6 message, sent
//! from a link-local address with hop limit 1 behind a Hop-by-Hop header that
//! carries the Router Alert option ([`HOP_BY_HOP_ROUTER_ALERT`]); the group's
//! frames arrive at the Ethernet address [`multicast_mac`] names.
//!
//! MLDv2 (RFC 3810) adds source filtering, which no caller asks for yet; an
//! MLDv2 query is longer than 24 bytes, and [`parse`] reads it as the MLDv1
//! query it begins with (RFC 3810 §8.2.1).

use crate::icmpv6;
use crate::ipv6::Ipv6Addr;

/// ICMPv6 type: Multicast Listener Query.
pub const TYPE_QUERY: u8 = 130;
/// ICMPv6 type: Multicast Listener Report (MLDv1).
pub const TYPE_REPORT: u8 = 131;
/// ICMPv6 type: Multicast Listener Done.
pub const TYPE_DONE: u8 = 132;
/// ICMPv6 type: MLDv2 Multicast Listener Report (accepted, never sent).
pub const TYPE_V2_REPORT: u8 = 143;
/// Length of an MLDv1 message.
pub const MESSAGE_LEN: usize = 24;

/// The Hop-by-Hop extension header every MLD message travels behind (RFC
/// 2710 §3): next header ICMPv6 (58), length 0 (eight bytes), the Router
/// Alert option (type 5, length 2, value 0: "MLD"), and a two-byte PadN.
/// Put `0` (Hop-by-Hop) in the IPv6 header's next-header field and this
/// before the message. It is not part of the ICMPv6 checksum.
pub const HOP_BY_HOP_ROUTER_ALERT: [u8; 8] = [58, 0, 5, 2, 0, 0, 1, 0];

/// Whether `addr` is an IPv6 multicast address, `ff00::/8`.
#[must_use]
pub fn is_multicast(addr: &Ipv6Addr) -> bool {
    addr[0] == 0xFF
}

/// Whether a listener reports `group` at all. Never `ff02::1`, which every
/// node listens to without saying so (RFC 2710 §5), and never a group of
/// scope 0 (reserved) or 1 (interface-local), which no link carries.
#[must_use]
pub fn reportable(group: &Ipv6Addr) -> bool {
    is_multicast(group)
        && *group != icmpv6::ALL_NODES_LINK_LOCAL
        && !matches!(group[1] & 0x0F, 0 | 1)
}

/// The Ethernet address IPv6 multicast `group` is sent to: 33:33 and the
/// group's low 32 bits (RFC 2464 §7).
#[must_use]
pub fn multicast_mac(group: &Ipv6Addr) -> crate::MacAddr {
    [0x33, 0x33, group[12], group[13], group[14], group[15]]
}

/// A parsed MLD message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Message {
    /// The ICMPv6 type ([`TYPE_QUERY`], [`TYPE_REPORT`], [`TYPE_DONE`]).
    pub kind: u8,
    /// For a query, how long a listener may wait before reporting, in
    /// milliseconds.
    pub max_response_delay_ms: u16,
    /// The group the message is about; `::` in a general query.
    pub group: Ipv6Addr,
}

impl Message {
    /// Whether this is a general query (about every group).
    #[must_use]
    pub fn is_general_query(&self) -> bool {
        self.kind == TYPE_QUERY && self.group == icmpv6::UNSPECIFIED
    }
}

/// Parse an MLD message from an ICMPv6 payload that arrived from `src` to
/// `dst`. `None` when it is shorter than 24 bytes or its checksum (over the
/// IPv6 pseudo-header and the whole payload) does not verify. An MLDv2
/// query, which is longer, is read as the MLDv1 query its first 24 bytes
/// are.
#[must_use]
pub fn parse(payload: &[u8], src: &Ipv6Addr, dst: &Ipv6Addr) -> Option<Message> {
    if payload.len() < MESSAGE_LEN || !icmpv6::verify_checksum(src, dst, payload) {
        return None;
    }
    let mut group = [0u8; 16];
    group.copy_from_slice(&payload[8..24]);
    Some(Message {
        kind: payload[0],
        max_response_delay_ms: u16::from_be_bytes([payload[4], payload[5]]),
        group,
    })
}

/// Write the 24-byte MLDv1 message `kind` about `group`, from `src` to
/// `dst`, into `out`, with its checksum; `None` when `out` is too short.
fn write(
    out: &mut [u8],
    kind: u8,
    src: &Ipv6Addr,
    dst: &Ipv6Addr,
    group: &Ipv6Addr,
) -> Option<usize> {
    let msg = out.get_mut(..MESSAGE_LEN)?;
    msg.fill(0);
    msg[0] = kind;
    msg[8..24].copy_from_slice(group);
    let csum = icmpv6::checksum(src, dst, msg);
    msg[2..4].copy_from_slice(&csum.to_be_bytes());
    Some(MESSAGE_LEN)
}

/// Write a Multicast Listener Report for `group` from `src` (a link-local
/// address) into `out`, returning its length. It is sent to `group` itself.
#[must_use]
pub fn write_report(out: &mut [u8], src: &Ipv6Addr, group: &Ipv6Addr) -> Option<usize> {
    write(out, TYPE_REPORT, src, group, group)
}

/// Write a Multicast Listener Done for `group` from `src` into `out`,
/// returning its length. It is sent to all routers, `ff02::2`.
#[must_use]
pub fn write_done(out: &mut [u8], src: &Ipv6Addr, group: &Ipv6Addr) -> Option<usize> {
    write(out, TYPE_DONE, src, &icmpv6::ALL_ROUTERS_LINK_LOCAL, group)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: Ipv6Addr = [
        0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0x50, 0x54, 0, 0xFF, 0xFE, 0x12, 0x34, 0x56,
    ];
    /// `ff02::fb`, mDNS.
    const MDNS: Ipv6Addr = [0xFF, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFB];

    #[test]
    fn a_report_round_trips_against_its_own_addresses() {
        let mut buf = [0u8; 32];
        let n = write_report(&mut buf, &SRC, &MDNS).unwrap();
        assert_eq!(n, MESSAGE_LEN);
        // Sent to the group: the checksum verifies against src -> group.
        let m = parse(&buf[..n], &SRC, &MDNS).unwrap();
        assert_eq!((m.kind, m.group), (TYPE_REPORT, MDNS));
        assert!(!m.is_general_query());
        // And not against another destination.
        assert!(parse(&buf[..n], &SRC, &icmpv6::ALL_ROUTERS_LINK_LOCAL).is_none());
    }

    #[test]
    fn a_done_goes_to_all_routers() {
        let mut buf = [0u8; MESSAGE_LEN];
        write_done(&mut buf, &SRC, &MDNS).unwrap();
        let m = parse(&buf, &SRC, &icmpv6::ALL_ROUTERS_LINK_LOCAL).unwrap();
        assert_eq!((m.kind, m.group), (TYPE_DONE, MDNS));
    }

    #[test]
    fn a_general_query_and_a_v2_query() {
        let router: Ipv6Addr = [0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        let mut q = [0u8; MESSAGE_LEN];
        q[0] = TYPE_QUERY;
        q[4..6].copy_from_slice(&10_000u16.to_be_bytes());
        let c = icmpv6::checksum(&router, &icmpv6::ALL_NODES_LINK_LOCAL, &q);
        q[2..4].copy_from_slice(&c.to_be_bytes());
        let m = parse(&q, &router, &icmpv6::ALL_NODES_LINK_LOCAL).unwrap();
        assert!(m.is_general_query());
        assert_eq!(m.max_response_delay_ms, 10_000);

        // MLDv2: four more bytes (flags, QQIC, zero sources), checksummed whole.
        let mut v2 = [0u8; MESSAGE_LEN + 4];
        v2[0] = TYPE_QUERY;
        v2[8..24].copy_from_slice(&MDNS);
        v2[24] = 2;
        v2[25] = 125;
        let c = icmpv6::checksum(&router, &MDNS, &v2);
        v2[2..4].copy_from_slice(&c.to_be_bytes());
        let m = parse(&v2, &router, &MDNS).unwrap();
        assert_eq!((m.kind, m.group), (TYPE_QUERY, MDNS));
    }

    #[test]
    fn a_short_or_corrupt_message_is_refused() {
        let mut buf = [0u8; MESSAGE_LEN];
        write_report(&mut buf, &SRC, &MDNS).unwrap();
        assert!(parse(&buf[..23], &SRC, &MDNS).is_none());
        let mut bad = buf;
        bad[20] ^= 1;
        assert!(parse(&bad, &SRC, &MDNS).is_none());
        assert!(write_done(&mut [0u8; 23], &SRC, &MDNS).is_none());
    }

    #[test]
    fn which_groups_are_reported() {
        assert!(reportable(&MDNS));
        assert!(!reportable(&icmpv6::ALL_NODES_LINK_LOCAL));
        // Interface-local (ff01::fb) and reserved-scope (ff00::fb) groups.
        let mut local = MDNS;
        local[1] = 0x01;
        assert!(!reportable(&local));
        local[1] = 0x00;
        assert!(!reportable(&local));
        // A unicast address is no group at all.
        assert!(!reportable(&SRC));
        assert!(is_multicast(&MDNS) && !is_multicast(&SRC));
    }

    #[test]
    fn the_ethernet_address_and_the_hop_by_hop_header() {
        assert_eq!(multicast_mac(&MDNS), [0x33, 0x33, 0, 0, 0, 0xFB]);
        // Eight bytes, next header ICMPv6, Router Alert with value 0.
        assert_eq!(HOP_BY_HOP_ROUTER_ALERT.len(), 8);
        assert_eq!(HOP_BY_HOP_ROUTER_ALERT[0], 58);
        assert_eq!(&HOP_BY_HOP_ROUTER_ALERT[2..6], &[5, 2, 0, 0]);
    }
}
