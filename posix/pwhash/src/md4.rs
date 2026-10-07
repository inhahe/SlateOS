//! MD4 (RFC 1320): the hashing behind the NT method (`$3$`, FreeBSD's
//! name for Windows' NT hash), whose settings `posix/src/nthash.rs` reads.
//!
//! A port of libxcrypt 4.4.36's `alg-md4.c`, as design-decisions §539 asks
//! (primitives are ported, not written).  Its notice, as it asks for credit:
//!
//! ```text
//! Author:
//! Alexander Peslyak, better known as Solar Designer <solar at openwall.com>
//!
//! This software was written by Alexander Peslyak in 2001.  No copyright is
//! claimed, and the software is hereby placed in the public domain.
//! In case this attempt to disclaim copyright and place the software in the
//! public domain is deemed null and void, then the software is
//! Copyright (c) 2001 Alexander Peslyak and it is hereby released to the
//! general public under the following terms:
//!
//! Redistribution and use in source and binary forms, with or without
//! modification, are permitted.
//!
//! There's ABSOLUTELY NO WARRANTY, express or implied.
//! ```

#![allow(clippy::indexing_slicing)] // the block's 16 words, by the fixed orders below
#![allow(clippy::arithmetic_side_effects)] // MD4's additions are modulo 2^32, spelled wrapping; the rest count a block

/// The first round's function: where `x`, `y`, else `z`.
fn f(x: u32, y: u32, z: u32) -> u32 {
    z ^ (x & (y ^ z))
}

/// The second round's: the majority of the three.
fn g(x: u32, y: u32, z: u32) -> u32 {
    (x & (y | z)) | (y & z)
}

/// The third round's: parity.
fn h(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

/// The rounds' constants, word orders and rotations (RFC 1320, 3.4).
const ROUNDS: [(u32, [usize; 16], [u32; 4]); 3] = [
    (
        0,
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        [3, 7, 11, 19],
    ),
    (
        0x5A82_7999,
        [0, 4, 8, 12, 1, 5, 9, 13, 2, 6, 10, 14, 3, 7, 11, 15],
        [3, 5, 9, 13],
    ),
    (
        0x6ED9_EBA1,
        [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15],
        [3, 9, 11, 15],
    ),
];

/// `body`: one block into the state.
fn compress(state: &mut [u32; 4], block: &[u8; 64]) {
    let mut x = [0u32; 16];
    for (word, bytes) in x.iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_le_bytes(*bytes);
    }
    let mut v = *state;
    for (round, (k, order, shifts)) in ROUNDS.iter().enumerate() {
        for (step, &i) in order.iter().enumerate() {
            // a, b, c, d turn by one each step: a is v[(4 - step) % 4].
            let a = (4 - step % 4) % 4;
            let (b, c, d) = ((a + 1) % 4, (a + 2) % 4, (a + 3) % 4);
            let fun = match round {
                0 => f(v[b], v[c], v[d]),
                1 => g(v[b], v[c], v[d]),
                _ => h(v[b], v[c], v[d]),
            };
            v[a] = v[a]
                .wrapping_add(fun)
                .wrapping_add(x[i])
                .wrapping_add(*k)
                .rotate_left(shifts[step % 4]);
        }
    }
    for (s, add) in state.iter_mut().zip(v) {
        *s = s.wrapping_add(add);
    }
}

/// MD4 of the concatenation of `parts`.
#[must_use]
pub fn digest(parts: &[&[u8]]) -> [u8; 16] {
    let mut state = [0x6745_2301u32, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    let mut block = [0u8; 64];
    let mut fill = 0;
    let mut total: u64 = 0;
    for part in parts {
        total = total.wrapping_add(part.len() as u64);
        let mut data = *part;
        while !data.is_empty() {
            let take = (64 - fill).min(data.len());
            block[fill..fill + take].copy_from_slice(&data[..take]);
            fill += take;
            data = &data[take..];
            if fill == 64 {
                compress(&mut state, &block);
                fill = 0;
            }
        }
    }
    block[fill] = 0x80;
    block[fill + 1..].fill(0);
    if fill >= 56 {
        compress(&mut state, &block);
        block.fill(0);
    }
    block[56..].copy_from_slice(&total.wrapping_mul(8).to_le_bytes());
    compress(&mut state, &block);
    let mut out = [0u8; 16];
    for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *chunk = word.to_le_bytes();
    }
    for b in &mut block {
        // SAFETY: a byte of this function's own block, written volatile so
        // that the password's traces are not left on the stack.
        unsafe { core::ptr::write_volatile(b, 0) };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> std::string::String {
        use core::fmt::Write;
        let mut s = std::string::String::new();
        for b in bytes {
            write!(s, "{b:02x}").unwrap();
        }
        s
    }

    /// libxcrypt's `test/alg-md4.c`: RFC 1320's appendix A.5, whole and a
    /// byte at a time.
    #[test]
    fn rfc_1320_vectors() {
        for (message, want) in [
            ("", "31d6cfe0d16ae931b73c59d7e0c089c0"),
            ("a", "bde52cb31de33e46245e05fbdbd6fb24"),
            ("abc", "a448017aaf21d8525fc10ae87aa6729d"),
            ("message digest", "d9130a8164549fe818874806e1c7014b"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "d79e1c308aa5bbcdeea8ed63df412da9",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "043f8582f241db351ce627e153e7f0e4",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "e33b4ddc9c38f2199c3e7b164fcc0536",
            ),
        ] {
            assert_eq!(hex(&digest(&[message.as_bytes()])), want, "{message}");
            let bytes: std::vec::Vec<&[u8]> = message.as_bytes().chunks(1).collect();
            assert_eq!(hex(&digest(&bytes)), want, "{message}, a byte at a time");
        }
    }
}
