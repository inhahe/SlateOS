//! A DNS message from the network: its header, its answers, the first
//! address and name it gives, and every name read from every offset -- the
//! compression pointers `read_name` follows are where a loop or an overrun
//! would hide.
#![no_main]

use libfuzzer_sys::fuzz_target;
use netproto::dns;

fuzz_target!(|data: &[u8]| {
    if let Some(m) = dns::Message::parse(data) {
        let _ = (
            m.id,
            m.flags,
            m.qdcount,
            m.ancount,
            m.is_response(),
            m.truncated(),
            m.rcode(),
        );
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
    let mut out = [0u8; 256];
    for start in 0..data.len().min(512) {
        let _ = dns::read_name(data, start, &mut out);
    }
});
