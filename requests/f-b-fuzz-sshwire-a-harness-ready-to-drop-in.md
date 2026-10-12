# F → B — fuzz `sshwire`: a harness, ready to drop in

**From:** Lane F. **To:** Lane B (`userspace/sshwire`).
**Filed:** 2026-10-12, on the operator's instruction. **Status:** open.

**In short:** the operator chose to fuzz every parser of untrusted input
(design-decisions §1393). `sshwire` reads what an
SSH peer sends before anything is authenticated, and what a private key
file holds. `scripts/fuzz.py` (lane F's) now runs any crate's harness in WSL
with cargo-fuzz. The harness below was run through it for two minutes a
target: `wire` ran 1.1 million inputs and `packet` 52 million, with no
finding. Put the files in `userspace/sshwire/fuzz/`, commit them, and run a
round whenever the parsing changes:

    python scripts/fuzz.py run userspace/sshwire/fuzz 1200

The two targets:
- `wire` covers the identification line, a run of the wire encoding's
  reads, base64 and an OpenSSH private key file, and the big-number
  arithmetic the key exchange does on the peer's values (bounded, as the
  exchange bounds them).
- `packet` covers the packet layer in the clear and under aes128-ctr with
  hmac-sha2-256 and keys fixed in the target, so the length, decryption
  and padding checks that run before the MAC is compared see what a
  hostile peer can send.

Neither runs under the address sanitizer: `sshwire` has no `unsafe` code.
A finding lands in `~/fuzz/sshwire-fuzz/tree/fuzz/artifacts/` in WSL. Copy
it into the crate's tests as a regression test once it is fixed.

## The files

`harness.toml`:

```toml
# sshwire's cargo-fuzz harness, run by `scripts/fuzz.py run userspace/sshwire/fuzz`.
# Installed as ~/fuzz/sshwire-fuzz/tree/fuzz/Cargo.toml in WSL; the `path`s
# below are relative to that place. Kept under this name so that nothing in the
# real workspace mistakes this directory for a package. Seeds: `seeds.py`.
[package]
name = "sshwire-fuzz"
version = "0.0.0"
publish = false
edition = "2021"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"
sshwire = { path = "../userspace/sshwire" }

[workspace]
members = ["."]

[profile.release]
debug = 1

[[bin]]
name = "wire"
path = "fuzz_targets/wire.rs"
test = false
doc = false
bench = false

[[bin]]
name = "packet"
path = "fuzz_targets/packet.rs"
test = false
doc = false
bench = false
```

`seeds.py`:

```python
"""Seeds for sshwire's targets: an identification line, a KEXINIT's fields,
base64 and an OpenSSH private key's armour, two big numbers, and clear and
"encrypted" packets. Called by `scripts/fuzz.py` as `seed(src, corpus)`."""
import os
import struct


def ssh_string(b):
    return struct.pack(">I", len(b)) + b


KEXINIT = bytes([20]) + bytes(16) + b"".join(ssh_string(s) for s in (
    b"diffie-hellman-group14-sha256", b"ssh-ed25519", b"aes128-ctr", b"aes128-ctr",
    b"hmac-sha2-256", b"hmac-sha2-256", b"none", b"none", b"", b"")) + bytes([0]) + bytes(4)


def packet(payload, block=8):
    pad = block - (5 + len(payload)) % block
    if pad < 4:
        pad += block
    return struct.pack(">IB", 1 + len(payload) + pad, pad) + payload + bytes(pad)


def seed(src, corpus):
    def put(target, label, data):
        open(os.path.join(corpus(target), label), "wb").write(data)

    put("wire", "ident", b"\x00SSH-2.0-OpenSSH_9.6p1 Ubuntu-3\r\n")
    put("wire", "fields", b"\x01" + bytes([2, 2, 0, 1, 3, 2, 2, 2] * 2) + KEXINIT[17:])
    put("wire", "base64", b"\x02" + b"c3NoLWVkMjU1MTkgQUFBQUMzTnphQzFsWkRJMU5URTVBQUFBSUE=")
    put("wire", "privkey", b"\x02-----BEGIN OPENSSH PRIVATE KEY-----\n"
        b"b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n"
        b"-----END OPENSSH PRIVATE KEY-----\n")
    put("wire", "bignum", b"\x03" + bytes(range(1, 65)) + bytes(range(64, 0, -1)))
    put("packet", "clear-kexinit", b"\x00" + packet(KEXINIT))
    put("packet", "clear-two", b"\x00" + packet(bytes([21])) + packet(bytes([5]) + ssh_string(b"ssh-userauth")))
    put("packet", "encrypted", b"\x01" + bytes(range(96)))
    put("packet", "encrypted-server", b"\x03" + bytes(range(96)))
```

`fuzz_targets/packet.rs`:

```rust
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
```

`fuzz_targets/wire.rs`:

```rust
//! What an SSH peer, or a key file, hands `sshwire` before anything is
//! authenticated: the identification line, the wire encoding's reads, base64
//! and an OpenSSH private key file, and the big-number arithmetic the key
//! exchange does on the peer's values.
#![no_main]

use libfuzzer_sys::fuzz_target;
use sshwire::{
    base64_decode, decode_identification, decode_openssh_private_key, is_identification_line,
    read_bool, read_mpint, read_ssh_string, read_u32, strip_line_terminator, BigUint,
};

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else { return };
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
```

## Replies
