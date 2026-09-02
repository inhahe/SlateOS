//! AES — the block cipher (FIPS-197) and the RFC 3394 key wrap built on it.
//!
//! # Why this crate exists
//!
//! Nothing in this tree implemented AES. `kernel/src/fs/diskencrypt.rs` names
//! `Aes256Xts` in an enum and stops there; the `userspace/openssl` and
//! `userspace/ssh` tools mention it in help text. So the first thing that
//! genuinely needed the cipher — the WiFi supplicant, which cannot join a
//! WPA2 network without unwrapping the group key that arrives in message 3 of
//! the 4-way handshake — would otherwise have had to grow a private copy.
//!
//! That is exactly the history [`crc32`](../crc32/index.html) records: the
//! same polynomial written out four separate times before anyone factored it
//! out. A block cipher is a worse thing to copy than a checksum, because a
//! transcription slip in an S-box does not produce a wrong answer that
//! anything notices — it produces a cipher that interoperates with nothing and
//! whose failure looks like a network problem.
//!
//! # Scope
//!
//! - AES-128, AES-192 and AES-256, encrypting and decrypting a single block.
//! - [`keywrap`]: RFC 3394 AES Key Wrap, which is how a WPA2/WPA3 GTK, and a
//!   wrapped disk-encryption master key, are protected.
//!
//! There is deliberately **no mode of operation here** — no CBC, no CTR, no
//! XTS, no GCM. A mode needs an IV/nonce policy and a padding policy, and
//! those belong with the protocol that chooses them, not with the primitive.
//! Key wrap is the exception because it is fully specified: it has a constant
//! IV, no padding and no nonce, so there is no policy left for a caller to get
//! wrong.
//!
//! # What this implementation is not
//!
//! **It is not constant-time.** It is the straightforward byte-oriented
//! implementation from FIPS-197, and its S-box lookups are data-dependent, so
//! a local attacker who can measure the cache can in principle recover the
//! key. Constant-time AES needs either bitslicing or the AES-NI instructions,
//! and AES-NI needs a CPUID check and `unsafe` intrinsics that this crate
//! forbids. That tradeoff is acceptable for the two current callers — a WiFi
//! key unwrap that runs a handful of times per association, and key wrapping
//! at rest — and is **not** acceptable for bulk data encryption on a machine
//! running untrusted local code. Anything that reaches that point should add
//! an AES-NI backend behind this same API rather than a second software one.
//!
//! ```
//! // FIPS-197 Appendix C.1.
//! let key = [
//!     0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
//!     0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
//! ];
//! let cipher = aes::Aes::new(&key).expect("128-bit key");
//! let mut block = [
//!     0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
//!     0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
//! ];
//! cipher.encrypt_block(&mut block);
//! assert_eq!(block[0], 0x69);
//! cipher.decrypt_block(&mut block);
//! assert_eq!(block[0], 0x00);
//! ```
//!
//! # References
//!
//! - FIPS PUB 197, *Advanced Encryption Standard*.
//! - RFC 3394, *Advanced Encryption Standard (AES) Key Wrap Algorithm*.

#![no_std]
#![forbid(unsafe_code)]

/// The AES block size, in octets. The same for all three key lengths — only
/// the key length and the round count vary.
pub const BLOCK_LEN: usize = 16;

/// The largest supported key length, in octets (AES-256).
pub const MAX_KEY_LEN: usize = 32;

/// The largest expanded key, in octets: 4 columns × 4 rows × (14 + 1) rounds.
const MAX_ROUND_KEY_LEN: usize = BLOCK_LEN * 15;

/// The AES substitution box (FIPS-197 figure 7).
const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

/// The inverse substitution box, *derived* from [`SBOX`] at compile time.
///
/// FIPS-197 prints this table too, and copying it out is the obvious thing to
/// do. It is also 256 more opportunities to mistype a hex digit, in a table
/// where a single wrong entry breaks only decryption — and only for the one
/// input byte that hits it, so a round-trip test on a handful of blocks can
/// easily miss it. Inverting the forward table instead makes that class of
/// error unrepresentable: `INV_SBOX[SBOX[i]] == i` holds by construction.
//
// The indexing lint is suppressed here rather than worked around because
// `<[T]>::get` is not usable in a `const` context, and because the hazard the
// lint guards against cannot arise in one: an out-of-range index inside a
// const block is a *compile* error, not a runtime panic. There is no input to
// this expression and no way for it to reach a user's machine mis-indexed.
#[allow(clippy::indexing_slicing)]
const INV_SBOX: [u8; 256] = {
    let mut inv = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        inv[SBOX[i] as usize] = i as u8;
        i += 1;
    }
    inv
};

/// Round constants for the key schedule: `x^(i-1)` in GF(2^8) (FIPS-197 §5.2).
///
/// Ten entries is enough for every key size: AES-128 uses all ten, AES-192
/// uses eight and AES-256 uses seven.
const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

/// Multiply by `x` in GF(2^8) modulo the AES polynomial `x^8 + x^4 + x^3 + x + 1`.
///
/// `wrapping_shl` rather than `<<` because the bit shifted out of the top is
/// meant to be discarded — it is accounted for by the conditional reduction —
/// and writing that intent explicitly keeps the overflow lint from flagging a
/// deliberate truncation.
const fn xtime(b: u8) -> u8 {
    let shifted = b.wrapping_shl(1);
    if b & 0x80 != 0 {
        shifted ^ 0x1b
    } else {
        shifted
    }
}

/// Multiply two elements of GF(2^8) — Russian-peasant multiplication, used
/// only by `inv_mix_columns`, whose coefficients are 9, 11, 13 and 14.
const fn gmul(a: u8, b: u8) -> u8 {
    let mut product = 0u8;
    let mut a = a;
    let mut b = b;
    while b != 0 {
        if b & 1 != 0 {
            product ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }
    product
}

/// The supported key length was not 16, 24 or 32 octets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadKeyLength;

/// An expanded AES key, ready to encrypt or decrypt blocks.
///
/// Construction runs the key schedule once; the per-block operations then do
/// no key work at all. Callers that encrypt many blocks under one key — the
/// key wrap does 6·n of them — should build this once and reuse it.
#[derive(Clone)]
pub struct Aes {
    /// The expanded key. Only the first `16 * (rounds + 1)` octets are used.
    round_key: [u8; MAX_ROUND_KEY_LEN],
    /// 10, 12 or 14.
    rounds: usize,
}

impl core::fmt::Debug for Aes {
    /// Deliberately prints no key material. A `#[derive(Debug)]` here would
    /// put the expanded key — from which the original key is recoverable —
    /// into any log line that formatted the cipher.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Aes")
            .field("rounds", &self.rounds)
            .finish_non_exhaustive()
    }
}

impl Aes {
    /// Expand `key` into a round-key schedule. `key` must be 16, 24 or 32
    /// octets long.
    ///
    /// # Errors
    ///
    /// [`BadKeyLength`] if `key` is not one of the three permitted lengths.
    /// AES has no "nearest supported size": a 20-octet key is a caller bug,
    /// and silently padding or truncating it would produce a cipher that
    /// works perfectly and interoperates with nothing.
    pub fn new(key: &[u8]) -> Result<Self, BadKeyLength> {
        let nk = match key.len() {
            16 => 4usize,
            24 => 6,
            32 => 8,
            _ => return Err(BadKeyLength),
        };
        let rounds = nk.saturating_add(6);
        let total_words = rounds.saturating_add(1).saturating_mul(4);

        let mut round_key = [0u8; MAX_ROUND_KEY_LEN];
        let Some(head) = round_key.get_mut(..key.len()) else {
            return Err(BadKeyLength);
        };
        head.copy_from_slice(key);

        for i in nk..total_words {
            // The previous word, transformed for the first word of each new
            // key (and, for AES-256 only, for the middle word as well).
            let prev = i.saturating_sub(1).saturating_mul(4);
            let mut t = [0u8; 4];
            let Some(src) = round_key.get(prev..prev.saturating_add(4)) else {
                return Err(BadKeyLength);
            };
            t.copy_from_slice(src);

            // `nk` is 4, 6 or 8 and so is never zero, but the remainder and
            // the quotient are written in their checked forms so that division
            // by zero is ruled out by the code rather than by this comment.
            let residue = i.checked_rem(nk);

            if residue == Some(0) {
                t.rotate_left(1);
                for byte in &mut t {
                    *byte = sub_byte(*byte);
                }
                // `i / nk` is at least 1 here and at most 10 for every
                // supported key size, so the index is always in RCON.
                let Some(rc) = i
                    .checked_div(nk)
                    .and_then(|round| RCON.get(round.saturating_sub(1)))
                else {
                    return Err(BadKeyLength);
                };
                let Some(first) = t.first_mut() else {
                    return Err(BadKeyLength);
                };
                *first ^= *rc;
            } else if nk > 6 && residue == Some(4) {
                // AES-256 only: an extra SubWord halfway through each key, to
                // keep the schedule from being too linear in the long key.
                for byte in &mut t {
                    *byte = sub_byte(*byte);
                }
            }

            let back = i.saturating_sub(nk).saturating_mul(4);
            let here = i.saturating_mul(4);
            for j in 0..4 {
                let Some(&old) = round_key.get(back.saturating_add(j)) else {
                    return Err(BadKeyLength);
                };
                let Some(slot) = round_key.get_mut(here.saturating_add(j)) else {
                    return Err(BadKeyLength);
                };
                // In bounds: `t` is a fixed `[u8; 4]` and `j < 4`.
                let Some(&word_byte) = t.get(j) else {
                    return Err(BadKeyLength);
                };
                *slot = old ^ word_byte;
            }
        }

        Ok(Self { round_key, rounds })
    }

    /// The number of rounds this key runs: 10, 12 or 14 for AES-128, -192 and
    /// -256 respectively.
    #[must_use]
    pub fn rounds(&self) -> usize {
        self.rounds
    }

    /// XOR round key `round` into `block`.
    fn add_round_key(&self, block: &mut [u8; BLOCK_LEN], round: usize) {
        let at = round.saturating_mul(BLOCK_LEN);
        let Some(rk) = self.round_key.get(at..at.saturating_add(BLOCK_LEN)) else {
            // Unreachable for `round <= self.rounds`, which is the only way
            // this is called; returning leaves the block untouched rather
            // than panicking, because a kernel must not die on a key schedule
            // bug it could instead report as a decryption failure.
            return;
        };
        for (b, k) in block.iter_mut().zip(rk.iter()) {
            *b ^= *k;
        }
    }

    /// Encrypt one block in place.
    pub fn encrypt_block(&self, block: &mut [u8; BLOCK_LEN]) {
        self.add_round_key(block, 0);
        for round in 1..self.rounds {
            sub_bytes(block);
            shift_rows(block);
            mix_columns(block);
            self.add_round_key(block, round);
        }
        // The last round omits MixColumns — without that asymmetry, encryption
        // and decryption would not be inverses of each other.
        sub_bytes(block);
        shift_rows(block);
        self.add_round_key(block, self.rounds);
    }

    /// Decrypt one block in place.
    pub fn decrypt_block(&self, block: &mut [u8; BLOCK_LEN]) {
        self.add_round_key(block, self.rounds);
        for round in (1..self.rounds).rev() {
            inv_shift_rows(block);
            inv_sub_bytes(block);
            self.add_round_key(block, round);
            inv_mix_columns(block);
        }
        inv_shift_rows(block);
        inv_sub_bytes(block);
        self.add_round_key(block, 0);
    }
}

/// Substitute one byte through the S-box.
///
/// A `u8` index into a 256-entry table cannot be out of range, but saying so
/// with `get` costs nothing on this path and keeps the lookup free of raw
/// indexing. The `unwrap_or` arm is unreachable.
fn sub_byte(b: u8) -> u8 {
    SBOX.get(b as usize).copied().unwrap_or(b)
}

/// Substitute one byte through the inverse S-box.
fn inv_sub_byte(b: u8) -> u8 {
    INV_SBOX.get(b as usize).copied().unwrap_or(b)
}

fn sub_bytes(block: &mut [u8; BLOCK_LEN]) {
    for b in block.iter_mut() {
        *b = sub_byte(*b);
    }
}

fn inv_sub_bytes(block: &mut [u8; BLOCK_LEN]) {
    for b in block.iter_mut() {
        *b = inv_sub_byte(*b);
    }
}

use rounds::{inv_mix_columns, inv_shift_rows, mix_columns, shift_rows};

/// The three state-mixing round operations.
///
/// These are the only place in the crate that indexes without `get`, and the
/// only place that adds and multiplies `usize` without a `checked_`/`saturating_`
/// guard. The suppressions below are deliberate and are safe by construction
/// rather than by inspection:
///
/// - every index is `4 * c + r` with `c` and `r` drawn from `0..4`, so the
///   largest value any expression here can take is `4 * 3 + 3 == 15`;
/// - the thing being indexed is a `[u8; BLOCK_LEN]` — a *fixed-size array*,
///   not a slice, so 16 is its length by type and cannot change with input;
/// - the loop bounds are literals, not derived from a caller's length.
///
/// Writing these four functions with `get`/`get_mut` would mean sixteen
/// fallible lookups per round and an `else` arm per lookup that is
/// unreachable — code that is strictly harder to check by eye than the bound
/// argument above, in the one part of the crate where a reader most needs to
/// be able to follow the FIPS-197 pseudocode line for line.
mod rounds {
    #![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

    use super::{BLOCK_LEN, gmul, xtime};

    /// Row `r` of the state moves left by `r` positions.
    ///
    /// The state is column-major: byte `4c + r` is row `r` of column `c`. That
    /// is the layout the input arrives in, so nothing has to be transposed.
    pub fn shift_rows(block: &mut [u8; BLOCK_LEN]) {
        let src = *block;
        for r in 1..4usize {
            for c in 0..4usize {
                block[4 * c + r] = src[4 * ((c + r) % 4) + r];
            }
        }
    }

    /// Row `r` of the state moves right by `r` positions — the inverse of
    /// [`shift_rows`].
    pub fn inv_shift_rows(block: &mut [u8; BLOCK_LEN]) {
        let src = *block;
        for r in 1..4usize {
            for c in 0..4usize {
                block[4 * ((c + r) % 4) + r] = src[4 * c + r];
            }
        }
    }

    /// Mix each column by the fixed polynomial `03x^3 + 01x^2 + 01x + 02`.
    pub fn mix_columns(block: &mut [u8; BLOCK_LEN]) {
        for c in 0..4usize {
            let base = 4 * c;
            let a = [
                block[base],
                block[base + 1],
                block[base + 2],
                block[base + 3],
            ];
            let sum = a[0] ^ a[1] ^ a[2] ^ a[3];
            // Each output is `a[i] ^ sum ^ xtime(a[i] ^ a[i+1])`, which is the
            // 2/3/1/1 matrix rewritten to need one `xtime` per byte instead of
            // four multiplications.
            block[base] = a[0] ^ sum ^ xtime(a[0] ^ a[1]);
            block[base + 1] = a[1] ^ sum ^ xtime(a[1] ^ a[2]);
            block[base + 2] = a[2] ^ sum ^ xtime(a[2] ^ a[3]);
            block[base + 3] = a[3] ^ sum ^ xtime(a[3] ^ a[0]);
        }
    }

    /// The inverse of [`mix_columns`]: the matrix with coefficients 14, 11,
    /// 13, 9.
    ///
    /// Written as explicit field multiplications rather than in terms of
    /// `xtime`, because the compact form of the inverse is genuinely harder to
    /// read than the matrix it implements, and this runs once per round on a
    /// path that is not hot.
    pub fn inv_mix_columns(block: &mut [u8; BLOCK_LEN]) {
        for c in 0..4usize {
            let base = 4 * c;
            let a = [
                block[base],
                block[base + 1],
                block[base + 2],
                block[base + 3],
            ];
            block[base] = gmul(a[0], 14) ^ gmul(a[1], 11) ^ gmul(a[2], 13) ^ gmul(a[3], 9);
            block[base + 1] = gmul(a[0], 9) ^ gmul(a[1], 14) ^ gmul(a[2], 11) ^ gmul(a[3], 13);
            block[base + 2] = gmul(a[0], 13) ^ gmul(a[1], 9) ^ gmul(a[2], 14) ^ gmul(a[3], 11);
            block[base + 3] = gmul(a[0], 11) ^ gmul(a[1], 13) ^ gmul(a[2], 9) ^ gmul(a[3], 14);
        }
    }
}

pub mod cmac;
pub mod keywrap;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// FIPS-197 Appendix C uses the same plaintext for all three key sizes.
    const PLAINTEXT: [u8; 16] = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
        0xff,
    ];

    /// `00 01 02 ... (len-1)`, which is the key in every FIPS-197 vector.
    fn counting_key<const N: usize>() -> [u8; N] {
        let mut k = [0u8; N];
        for (i, b) in k.iter_mut().enumerate() {
            *b = u8::try_from(i).expect("N <= 32");
        }
        k
    }

    #[test]
    fn the_inverse_sbox_really_inverts_the_sbox() {
        // The property that makes deriving `INV_SBOX` safer than copying it.
        for i in 0..256usize {
            let f = SBOX[i];
            assert_eq!(
                INV_SBOX[f as usize] as usize, i,
                "S-box is not a permutation at {i}"
            );
        }
    }

    #[test]
    fn aes128_matches_the_fips197_vector() {
        // FIPS-197 C.1: key 000102...0f, ciphertext 69c4e0d8 6a7b0430 d8cdb780 70b4c55a.
        let cipher = Aes::new(&counting_key::<16>()).unwrap();
        let mut block = PLAINTEXT;
        cipher.encrypt_block(&mut block);
        assert_eq!(
            block,
            [
                0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30, 0xd8, 0xcd, 0xb7, 0x80, 0x70, 0xb4,
                0xc5, 0x5a
            ]
        );
        cipher.decrypt_block(&mut block);
        assert_eq!(block, PLAINTEXT);
    }

    #[test]
    fn aes192_matches_the_fips197_vector() {
        // FIPS-197 C.2: ciphertext dda97ca4 864cdfe0 6eaf70a0 ec0d7191.
        let cipher = Aes::new(&counting_key::<24>()).unwrap();
        let mut block = PLAINTEXT;
        cipher.encrypt_block(&mut block);
        assert_eq!(
            block,
            [
                0xdd, 0xa9, 0x7c, 0xa4, 0x86, 0x4c, 0xdf, 0xe0, 0x6e, 0xaf, 0x70, 0xa0, 0xec, 0x0d,
                0x71, 0x91
            ]
        );
        cipher.decrypt_block(&mut block);
        assert_eq!(block, PLAINTEXT);
    }

    #[test]
    fn aes256_matches_the_fips197_vector() {
        // FIPS-197 C.3: ciphertext 8ea2b7ca 516745bf eafc4990 4b496089.
        let cipher = Aes::new(&counting_key::<32>()).unwrap();
        let mut block = PLAINTEXT;
        cipher.encrypt_block(&mut block);
        assert_eq!(
            block,
            [
                0x8e, 0xa2, 0xb7, 0xca, 0x51, 0x67, 0x45, 0xbf, 0xea, 0xfc, 0x49, 0x90, 0x4b, 0x49,
                0x60, 0x89
            ]
        );
        cipher.decrypt_block(&mut block);
        assert_eq!(block, PLAINTEXT);
    }

    #[test]
    fn the_round_count_follows_the_key_length() {
        assert_eq!(Aes::new(&[0u8; 16]).unwrap().rounds(), 10);
        assert_eq!(Aes::new(&[0u8; 24]).unwrap().rounds(), 12);
        assert_eq!(Aes::new(&[0u8; 32]).unwrap().rounds(), 14);
    }

    #[test]
    fn a_key_of_the_wrong_length_is_refused_not_padded() {
        // Padding a short key to 16 octets would give a cipher that works,
        // round-trips, and agrees with no other implementation on earth.
        for len in 0..40usize {
            let key = [0u8; 40];
            let got = Aes::new(&key[..len]);
            if matches!(len, 16 | 24 | 32) {
                assert!(got.is_ok(), "{len} octets is a valid AES key length");
            } else {
                assert_eq!(
                    got.err(),
                    Some(BadKeyLength),
                    "{len} octets must be refused"
                );
            }
        }
    }

    #[test]
    fn shift_rows_is_undone_by_inv_shift_rows() {
        let mut block = PLAINTEXT;
        shift_rows(&mut block);
        assert_ne!(block, PLAINTEXT, "ShiftRows must actually move something");
        inv_shift_rows(&mut block);
        assert_eq!(block, PLAINTEXT);
    }

    #[test]
    fn mix_columns_is_undone_by_inv_mix_columns() {
        // These two are the pair most likely to disagree, because the forward
        // one is written in the compact `xtime` form and the inverse as an
        // explicit matrix.
        let mut block = PLAINTEXT;
        mix_columns(&mut block);
        assert_ne!(block, PLAINTEXT);
        inv_mix_columns(&mut block);
        assert_eq!(block, PLAINTEXT);
    }

    #[test]
    fn mix_columns_matches_the_worked_example() {
        // FIPS-197 §5.1.3's example column: db 13 53 45 -> 8e 4d a1 bc.
        let mut block = [0u8; BLOCK_LEN];
        block[..4].copy_from_slice(&[0xdb, 0x13, 0x53, 0x45]);
        mix_columns(&mut block);
        assert_eq!(&block[..4], &[0x8e, 0x4d, 0xa1, 0xbc]);
    }

    #[test]
    fn every_block_round_trips_under_every_key_size() {
        for key_len in [16usize, 24, 32] {
            let mut key = [0u8; 32];
            for (i, b) in key.iter_mut().enumerate() {
                *b = u8::try_from(i * 7 % 251).expect("small");
            }
            let cipher = Aes::new(&key[..key_len]).unwrap();
            for seed in 0..64u8 {
                let mut block = [0u8; BLOCK_LEN];
                for (i, b) in block.iter_mut().enumerate() {
                    *b = seed ^ u8::try_from(i).expect("small");
                }
                let original = block;
                cipher.encrypt_block(&mut block);
                assert_ne!(block, original, "ciphertext equalled plaintext");
                cipher.decrypt_block(&mut block);
                assert_eq!(block, original, "key {key_len} seed {seed}");
            }
        }
    }

    #[test]
    fn the_debug_impl_does_not_print_key_material() {
        // The expanded key contains the original key verbatim in its first
        // words, so a derived `Debug` would leak it into any log line.
        use core::fmt::Write;

        struct Sink {
            buf: [u8; 256],
            len: usize,
        }
        impl Write for Sink {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                let end = self.len + s.len();
                self.buf[self.len..end].copy_from_slice(s.as_bytes());
                self.len = end;
                Ok(())
            }
        }

        let cipher = Aes::new(&counting_key::<16>()).unwrap();
        let mut sink = Sink {
            buf: [0u8; 256],
            len: 0,
        };
        write!(sink, "{cipher:?}").expect("formats");
        let rendered = core::str::from_utf8(&sink.buf[..sink.len]).expect("ascii");
        assert!(rendered.contains("rounds"), "{rendered}");
        assert!(!rendered.contains("round_key"), "{rendered}");
        // The first round key *is* the raw key, so its bytes must not appear.
        assert!(!rendered.contains("01, 2"), "{rendered}");
    }
}
