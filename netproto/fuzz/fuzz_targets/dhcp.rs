//! A DHCP message from the network, and every option it might carry.
#![no_main]

use libfuzzer_sys::fuzz_target;
use netproto::dhcp;

fuzz_target!(|data: &[u8]| {
    if let Some(m) = dhcp::Message::parse(data) {
        let _ = (
            m.msg_type(),
            m.subnet_mask(),
            m.router(),
            m.dns(),
            m.server_id(),
        );
        for code in 0..=255u8 {
            let _ = m.option(code);
        }
    }
});
