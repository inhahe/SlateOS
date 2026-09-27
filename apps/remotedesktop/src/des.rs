//! DES (FIPS 46-3), encryption only, for one purpose: VNC's password
//! challenge (RFB 3.8 section 7.2.2, "VNC Authentication").
//!
//! The server sends sixteen random bytes; the client returns them encrypted
//! with DES in ECB mode under a key made from the password -- its first eight
//! bytes, zero-padded, **each byte's bits reversed**. That reversal is not in
//! any standard; it is how the original VNC code built the key, and every
//! server since has had to agree with it.
//!
//! DES is broken as a cipher and this is not a use of it as one: VNC's
//! challenge proves the password to the server without sending it, over a
//! connection that is otherwise plain text anyway (see the module doc of
//! `rfb`). Nothing else should call this.
//!
//! Checked against FIPS 46-3's example (key `133457799BBCDFF1`) and against
//! VNC responses computed with an independent DES (Python's `cryptography`).

/// Initial permutation.
const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6,
    64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61,
    53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];

/// Final permutation (the inverse of `IP`).
const FP: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62, 30,
    37, 5, 45, 13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19, 59, 27,
    34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
];

/// Expansion of the 32-bit half block to 48 bits.
const E: [u8; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18,
    19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1,
];

/// The permutation after the S-boxes.
const P: [u8; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19,
    13, 30, 6, 22, 11, 4, 25,
];

/// Permuted choice 1: the 56 key bits, parity bits dropped.
const PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60,
    52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29,
    21, 13, 5, 28, 20, 12, 4,
];

/// Permuted choice 2: the 48 bits of each round key.
const PC2: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41, 52,
    31, 37, 47, 55, 30, 40, 51, 45, 33, 48, 44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];

/// How far each round rotates the key halves.
const SHIFTS: [u32; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

/// The eight S-boxes, each four rows of sixteen.
const S: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12,
        11, 9, 5, 3, 8, 4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2, 4, 9,
        1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1,
        10, 6, 9, 11, 5, 0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, 13, 8, 10, 1, 3, 15,
        4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5,
        14, 12, 11, 15, 1, 13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, 1, 10, 13, 0, 6,
        9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2,
        12, 1, 10, 14, 9, 10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6, 10, 1,
        13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15,
        10, 3, 9, 8, 6, 4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, 11, 8, 12, 7, 1, 14,
        2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13,
        14, 0, 11, 3, 8, 9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, 4, 3, 2, 12, 9, 5,
        15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5,
        12, 2, 15, 8, 6, 1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8, 1, 4,
        10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6,
        11, 0, 14, 9, 2, 7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, 2, 1, 14, 7, 4, 10,
        8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

/// Permute the `from`-bit value `input` (bit 1 its most significant) by
/// `table`, whose entries name input bits, 1-based.
fn permute(input: u64, from: u32, table: &[u8]) -> u64 {
    table.iter().fold(0_u64, |out, &bit| {
        let shift = from.saturating_sub(u32::from(bit));
        (out << 1) | ((input >> shift) & 1)
    })
}

/// The sixteen 48-bit round keys of `key`.
fn round_keys(key: u64) -> [u64; 16] {
    let cd = permute(key, 64, &PC1);
    let mut c = (cd >> 28) & 0x0FFF_FFFF;
    let mut d = cd & 0x0FFF_FFFF;
    let mut keys = [0_u64; 16];
    for (slot, &shift) in keys.iter_mut().zip(SHIFTS.iter()) {
        c = ((c << shift) | (c >> 28_u32.saturating_sub(shift))) & 0x0FFF_FFFF;
        d = ((d << shift) | (d >> 28_u32.saturating_sub(shift))) & 0x0FFF_FFFF;
        *slot = permute((c << 28) | d, 56, &PC2);
    }
    keys
}

/// The round function: expand, mix in the round key, substitute, permute.
fn feistel(half: u32, key: u64) -> u32 {
    let x = permute(u64::from(half), 32, &E) ^ key;
    let mut out = 0_u32;
    for (i, sbox) in S.iter().enumerate() {
        let shift = 42_usize.saturating_sub(i.saturating_mul(6));
        let six = ((x >> shift) & 0x3F) as usize;
        let row = ((six & 0x20) >> 4) | (six & 1);
        let col = (six >> 1) & 0xF;
        let value = sbox
            .get(row.saturating_mul(16).saturating_add(col))
            .copied()
            .unwrap_or(0);
        out = (out << 4) | u32::from(value);
    }
    u32::try_from(permute(u64::from(out), 32, &P)).unwrap_or(0)
}

/// Encrypt one 64-bit block under `key`.
#[must_use]
pub fn encrypt_block(key: [u8; 8], block: [u8; 8]) -> [u8; 8] {
    let keys = round_keys(u64::from_be_bytes(key));
    let ip = permute(u64::from_be_bytes(block), 64, &IP);
    let mut left = u32::try_from(ip >> 32).unwrap_or(0);
    let mut right = u32::try_from(ip & 0xFFFF_FFFF).unwrap_or(0);
    for key in keys {
        let next = left ^ feistel(right, key);
        left = right;
        right = next;
    }
    // The halves swap before the final permutation.
    let joined = (u64::from(right) << 32) | u64::from(left);
    permute(joined, 64, &FP).to_be_bytes()
}

/// The response to a VNC authentication `challenge` for `password`.
///
/// Only the password's first eight bytes count, as in every VNC server.
#[must_use]
pub fn vnc_response(password: &[u8], challenge: &[u8; 16]) -> [u8; 16] {
    let mut key = [0_u8; 8];
    for (k, &p) in key.iter_mut().zip(password.iter()) {
        *k = p.reverse_bits();
    }
    let mut out = [0_u8; 16];
    for (half, chunk) in out.chunks_exact_mut(8).zip(challenge.chunks_exact(8)) {
        let mut block = [0_u8; 8];
        block.copy_from_slice(chunk);
        half.copy_from_slice(&encrypt_block(key, block));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::format_collect)]

    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// FIPS 46-3's worked example.
    #[test]
    fn the_standards_example_encrypts_as_published() {
        let key = 0x1334_5779_9BBC_DFF1_u64.to_be_bytes();
        let block = 0x0123_4567_89AB_CDEF_u64.to_be_bytes();
        assert_eq!(hex(&encrypt_block(key, block)), "85e813540f0ab405");
    }

    /// VNC's responses, as an independent DES computes them with the key
    /// bytes' bits reversed.
    #[test]
    fn vnc_responses_match_an_independent_des() {
        let challenge: [u8; 16] = core::array::from_fn(|i| u8::try_from(i).unwrap());
        for (password, expected) in [
            ("password", "b866924125c8eebb9debc1db61c538e2"),
            ("secret", "ee22539f33a5983ec12f9c2edbc995dd"),
            ("a", "449407fb6f71bd517aca76866568a4e2"),
            ("12345678abc", "83dd2b4dbd04367f28578fdd5b142740"),
        ] {
            assert_eq!(
                hex(&vnc_response(password.as_bytes(), &challenge)),
                expected,
                "{password}"
            );
        }
    }
}
