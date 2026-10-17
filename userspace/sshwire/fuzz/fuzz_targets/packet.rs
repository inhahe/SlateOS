//! The bytes an SSH peer sends, through the packet layer: before the key
//! exchange, in the clear, and after it, under aes128-ctr and hmac-sha2-256
//! with keys fixed here -- so the length, decryption and padding checks that
//! run before the MAC is compared see what a hostile peer can send them.
#![no_main]

use libfuzzer_sys::fuzz_target;
use sshwire::{PacketCodec, Role};

fn drain(codec: &mut PacketCodec, mut buf: &[u8]) {
    // Every packet in the input, as the stream reader hands them over. A
    // decoded packet must use bytes, and no more than it was given: a reader
    // would loop forever on the one, and slice past its buffer on the other.
    for _ in 0..64 {
        match codec.decode(buf) {
            Ok(Some((_payload, used))) => {
                assert!(used > 0 && used <= buf.len(), "decode used {used} of {}", buf.len());
                buf = &buf[used..];
            }
            _ => return,
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else { return };
    let mut codec = PacketCodec::new();
    if selector & 1 == 1 {
        let role = if selector & 2 == 0 { Role::Client } else { Role::Server };
        if codec.activate(role, &[7u8; 32], &[1u8; 32], &[2u8; 32]).is_err() {
            return;
        }
    }
    let _ = (codec.is_encrypted(), codec.inbound_block_size(), codec.padding_len(rest.len()));
    drain(&mut codec, rest);
});
