//! What an SSH peer, or a key file, hands `sshwire` before anything is
//! authenticated: the identification line, the wire encoding's reads, base64
//! and an OpenSSH private key file, and the big-number arithmetic the key
//! exchange does on the peer's values.
#![no_main]

use libfuzzer_sys::fuzz_target;
use sshwire::{
    BigUint, base64_decode, decode_identification, decode_openssh_private_key,
    is_identification_line, read_bool, read_mpint, read_ssh_string, read_u32,
    strip_line_terminator,
};

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    match selector % 4 {
        0 => {
            let line = strip_line_terminator(rest);
            let _ = (is_identification_line(line), decode_identification(line));
        }
        1 => {
            // A run of fields, read as a message's parser reads them: the
            // next byte of the input says which read comes next.
            let (ops, buf) = rest.split_at(rest.len().min(16));
            let mut at = 0;
            for op in ops {
                let r = match op % 4 {
                    0 => read_u32(buf, at).map(|(_, n)| n),
                    1 => read_bool(buf, at).map(|(_, n)| n),
                    2 => read_ssh_string(buf, at).map(|(_, n)| n),
                    _ => read_mpint(buf, at).map(|(_, n)| n),
                };
                match r {
                    Ok(next) => at = next,
                    Err(_) => break,
                }
            }
        }
        2 => {
            let _ = base64_decode(rest);
            if let Ok(text) = std::str::from_utf8(rest) {
                let _ = decode_openssh_private_key(text);
            }
        }
        _ => {
            // The peer's DH value against the group: bounded, as the key
            // exchange bounds it, so a long input is not a long exponent.
            let half = rest.len() / 2;
            let (a, b) = rest.split_at(half);
            let (a, b) = (&a[..a.len().min(256)], &b[..b.len().min(256)]);
            let (x, m) = (BigUint::from_bytes_be(a), BigUint::from_bytes_be(b));
            let _ = x.mul(&m).to_bytes_be();
            if !m.is_zero() {
                let _ = x.div_rem(&m);
                let _ = x.mod_reduce(&m);
                let e = BigUint::from_bytes_be(&a[..a.len().min(8)]);
                let _ = x.mod_pow(&e, &m);
            }
            let _ = (x.cmp_unsigned(&m), x.bit_length());
            if x.cmp_unsigned(&m) != std::cmp::Ordering::Less {
                let _ = x.sub(&m);
            }
        }
    }
});
