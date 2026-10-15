//! An Ethernet frame, read all the way down: ARP, IPv4 and IPv6, and under
//! them ICMP, IGMP, ICMPv6's neighbour discovery and MLD, TCP and UDP, and
//! DNS and DHCP in UDP. Each checksum is made valid before the layer is read,
//! so that the fuzzer reaches what lies behind it rather than stopping at a
//! sum it would almost never get right by chance.
#![no_main]

use libfuzzer_sys::fuzz_target;
use netproto::{arp, checksum, dhcp, dns, ethernet, icmp, icmpv6, igmp, ipv4, ipv6, mld, tcp, udp};

/// Write into `seg[at..at + 2]` the checksum that makes `seg` verify against
/// the pseudo-header sum `sum`.
fn fix(seg: &mut [u8], at: usize, sum: u32) {
    if seg.len() >= at + 2 {
        seg[at] = 0;
        seg[at + 1] = 0;
        let c = checksum::internet_continue(sum, seg);
        seg[at..at + 2].copy_from_slice(&c.to_be_bytes());
    }
}

fn application(payload: &[u8]) {
    if let Some(m) = dns::Message::parse(payload) {
        let _ = (m.is_response(), m.truncated(), m.rcode());
        for a in m.answers() {
            let _ = (a.atype, a.class, a.ttl, a.rdata.len());
        }
        let (mut v4, mut v6, mut name) = ([0u8; 4], [0u8; 16], [0u8; 256]);
        let _ = (
            m.first_ipv4(&mut v4),
            m.first_ipv6(&mut v6),
            m.first_ptr(&mut name),
        );
    }
    if let Some(m) = dhcp::Message::parse(payload) {
        let _ = (
            m.msg_type(),
            m.subnet_mask(),
            m.router(),
            m.dns(),
            m.server_id(),
        );
    }
}

fn udp_v4(p: &ipv4::Packet<'_>) {
    let mut seg = p.payload.to_vec();
    if seg.len() >= 8 {
        let len = usize::from(u16::from_be_bytes([seg[4], seg[5]]));
        if (8..=seg.len()).contains(&len) {
            let sum = ipv4::pseudo_header_sum(&p.src, &p.dst, len as u16, ipv4::PROTO_UDP);
            fix(&mut seg[..len], 6, sum);
        }
    }
    if let Some(d) = udp::Datagram::parse(&seg, &p.src, &p.dst) {
        application(d.payload);
    }
}

fn ipv4_packet(buf: &[u8]) {
    // The header's own checksum, over the header alone.
    let mut buf = buf.to_vec();
    if let Some(&b0) = buf.first() {
        let header_len = usize::from(b0 & 0x0f) * 4;
        if header_len >= 20 && buf.len() >= header_len {
            fix(&mut buf[..header_len], 10, 0);
        }
    }
    let Some(p) = ipv4::Packet::parse(&buf) else {
        return;
    };
    let _ = (p.dont_fragment(), p.is_fragment());
    match p.protocol {
        ipv4::PROTO_ICMP => {
            // ICMP's checksum covers the message alone, as IGMP's does.
            let mut msg = p.payload.to_vec();
            fix(&mut msg, 2, 0);
            if let Some(e) = icmp::Echo::parse(&msg) {
                let mut out = [0u8; 2048];
                let _ = icmp::reply_to(&mut out, &e);
            }
        }
        igmp::PROTO_IGMP => {
            let mut msg = p.payload.to_vec();
            fix(&mut msg, 2, 0);
            if let Some(m) = igmp::parse(&msg) {
                let _ = m.is_general_query();
            }
        }
        ipv4::PROTO_TCP => {
            let mut seg = p.payload.to_vec();
            let sum = ipv4::pseudo_header_sum(&p.src, &p.dst, seg.len() as u16, ipv4::PROTO_TCP);
            fix(&mut seg, 16, sum);
            if let Some(s) = tcp::Segment::parse(&seg, &p.src, &p.dst) {
                let _ = (s.has_flag(tcp::FLAG_SYN), s.options.len(), s.payload.len());
            }
        }
        ipv4::PROTO_UDP => udp_v4(&p),
        _ => {}
    }
}

fn ipv6_packet(buf: &[u8]) {
    let Some(p) = ipv6::Packet::parse(buf) else {
        return;
    };
    let mut seg = p.payload.to_vec();
    match p.next_header {
        6 => {
            let sum = ipv6::pseudo_header_sum(&p.src, &p.dst, seg.len() as u32, 6);
            fix(&mut seg, 16, sum);
            let _ = tcp::Segment::parse_v6(&seg, &p.src, &p.dst);
        }
        17 => {
            if seg.len() >= 8 {
                let len = usize::from(u16::from_be_bytes([seg[4], seg[5]]));
                if (8..=seg.len()).contains(&len) {
                    let sum = ipv6::pseudo_header_sum(&p.src, &p.dst, len as u32, 17);
                    fix(&mut seg[..len], 6, sum);
                }
            }
            if let Some(d) = udp::Datagram::parse_v6(&seg, &p.src, &p.dst) {
                application(d.payload);
            }
        }
        58 => {
            if seg.len() >= 4 {
                seg[2] = 0;
                seg[3] = 0;
                let c = icmpv6::checksum(&p.src, &p.dst, &seg);
                seg[2..4].copy_from_slice(&c.to_be_bytes());
            }
            let _ = icmpv6::parse_neighbor_solicitation(&seg, &p.src, &p.dst);
            let _ = icmpv6::parse_neighbor_advertisement(&seg, &p.src, &p.dst);
            let _ = icmpv6::is_echo_request(&seg, &p.src, &p.dst);
            let _ = mld::parse(&seg, &p.src, &p.dst);
        }
        _ => {}
    }
}

fuzz_target!(|data: &[u8]| {
    let Some(frame) = ethernet::Frame::parse(data) else {
        return;
    };
    let _ = (frame.is_broadcast(), frame.is_multicast());
    match frame.ethertype {
        ethernet::ETHERTYPE_ARP => {
            if let Some(a) = arp::Packet::parse(frame.payload) {
                let _ = arp::reply_to(&a, &[2, 0, 0, 0, 0, 1]);
            }
        }
        0x0800 => ipv4_packet(frame.payload),
        0x86dd => ipv6_packet(frame.payload),
        _ => {}
    }
});
