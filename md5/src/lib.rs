//! MD5, written once, checked against the RFC 1321 vectors.
//!
//! # MD5 is thoroughly broken, and this crate is still the right thing to have
//!
//! MD5 collisions have been computable on ordinary hardware since 2004, and
//! chosen-prefix collisions since 2007 — the latter were used to forge a
//! real, trusted CA certificate in 2008. **Nothing in this tree may use MD5 to
//! decide whether to trust something.** Not for signatures, not for password
//! storage, not for content addressing, not for detecting tampering. For any
//! of those, use [`sha2`](../sha2/index.html).
//!
//! It is here for one job, which is not about trust: **reproducing a checksum
//! somebody else published.** `apps/diskimager` has to tell a user whether the
//! `.iso` they downloaded matches the `md5sum` printed on the download page.
//! Whether MD5 was a good choice was the publisher's decision and it has
//! already been made; our only job is to compute the same function they did.
//! An imager that refused to implement MD5 would not be more secure, it would
//! simply be unable to answer the question the user asked.
//!
//! Note the distinction that makes this safe: comparing against a
//! publisher-supplied MD5 detects *accidental* corruption — a truncated
//! download, a bad sector, a flipped bit — which is what checksums on download
//! pages are actually for. It does not detect a *deliberately* substituted
//! image, because an attacker who can replace the file can also replace the
//! checksum, and could in any case construct a collision. Anything relying on
//! the second property is a bug, wherever it is.
//!
//! # Usage
//!
//! ```
//! # use md5::{md5, hex};
//! assert_eq!(hex(&md5(b"abc")).as_str(), "900150983cd24fb0d6963f7d28e17f72");
//! ```
//!
//! For data that does not arrive all at once:
//!
//! ```
//! # use md5::Md5;
//! let mut hasher = Md5::new();
//! hasher.update(b"a");
//! hasher.update(b"bc");
//! assert_eq!(hasher.finalize(), md5::md5(b"abc"));
//! ```

#![no_std]

use blockbuf::{BlockBuffer, LengthOrder};
use core::fmt;

/// The block size MD5 compresses, in bytes.
const BLOCK_LEN: usize = 64;

/// Bytes of digest output.
pub const DIGEST_LEN: usize = 16;

/// Initial hash value (RFC 1321 §3.3).
const INIT: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];

/// Per-round additive constants: `floor(2^32 * abs(sin(i + 1)))` for `i` in
/// `0..64`, with the angle in radians (RFC 1321 §3.4).
const T: [u32; 64] = [
    0xd76a_a478, 0xe8c7_b756, 0x2420_70db, 0xc1bd_ceee,
    0xf57c_0faf, 0x4787_c62a, 0xa830_4613, 0xfd46_9501,
    0x6980_98d8, 0x8b44_f7af, 0xffff_5bb1, 0x895c_d7be,
    0x6b90_1122, 0xfd98_7193, 0xa679_438e, 0x49b4_0821,
    0xf61e_2562, 0xc040_b340, 0x265e_5a51, 0xe9b6_c7aa,
    0xd62f_105d, 0x0244_1453, 0xd8a1_e681, 0xe7d3_fbc8,
    0x21e1_cde6, 0xc337_07d6, 0xf4d5_0d87, 0x455a_14ed,
    0xa9e3_e905, 0xfcef_a3f8, 0x676f_02d9, 0x8d2a_4c8a,
    0xfffa_3942, 0x8771_f681, 0x6d9d_6122, 0xfde5_380c,
    0xa4be_ea44, 0x4bde_cfa9, 0xf6bb_4b60, 0xbebf_bc70,
    0x289b_7ec6, 0xeaa1_27fa, 0xd4ef_3085, 0x0488_1d05,
    0xd9d4_d039, 0xe6db_99e5, 0x1fa2_7cf8, 0xc4ac_5665,
    0xf429_2244, 0x432a_ff97, 0xab94_23a7, 0xfc93_a039,
    0x655b_59c3, 0x8f0c_cc92, 0xffef_f47d, 0x8584_5dd1,
    0x6fa8_7e4f, 0xfe2c_e6e0, 0xa301_4314, 0x4e08_11a1,
    0xf753_7e82, 0xbd3a_f235, 0x2ad7_d2bb, 0xeb86_d391,
];

/// Left-rotation amounts, four per round, repeated four times each within a
/// round (RFC 1321 §3.4).
const SHIFTS: [u32; 16] = [
    7, 12, 17, 22, // round 1
    5, 9, 14, 20, // round 2
    4, 11, 16, 23, // round 3
    6, 10, 15, 21, // round 4
];

/// MD5 of `data`.
#[must_use]
pub fn md5(data: &[u8]) -> [u8; DIGEST_LEN] {
    let mut hasher = Md5::new();
    hasher.update(data);
    hasher.finalize()
}

/// An in-progress MD5, for data that arrives in pieces.
#[derive(Clone)]
pub struct Md5 {
    state: [u32; 4],
    buffer: BlockBuffer<BLOCK_LEN>,
}

impl Default for Md5 {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Md5 {
    /// Deliberately opaque; see [`blockbuf::BlockBuffer`]'s `Debug`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Md5")
            .field("bytes_hashed", &self.buffer.bytes_absorbed())
            .finish_non_exhaustive()
    }
}

impl Md5 {
    /// Start a new hash.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: INIT,
            buffer: BlockBuffer::new(),
        }
    }

    /// Feed `data` in. Splitting the same input differently across calls gives
    /// the same digest.
    pub fn update(&mut self, data: &[u8]) {
        let state = &mut self.state;
        self.buffer.update(data, |block| compress(state, block));
    }

    /// Finish, returning the 16-byte digest.
    ///
    /// Consumes the hasher: the padding is part of the hashed message, so a
    /// finalised state cannot be extended.
    #[must_use]
    pub fn finalize(mut self) -> [u8; DIGEST_LEN] {
        let state = &mut self.state;
        // Little-endian, unlike the SHA family. This one byte-order choice is
        // the only structural difference between the two paddings, which is
        // why `blockbuf` takes it as a parameter.
        self.buffer
            .finalize(LengthOrder::LittleEndian, |block| compress(state, block));

        let mut out = [0u8; DIGEST_LEN];
        for (slot, word) in out.chunks_exact_mut(4).zip(self.state) {
            slot.copy_from_slice(&word.to_le_bytes());
        }
        out
    }
}

/// One application of the MD5 compression function to a 64-byte block.
fn compress(state: &mut [u32; 4], block: &[u8; BLOCK_LEN]) {
    // The sixteen message words, little-endian.
    let mut m = [0u32; 16];
    for (word, bytes) in m.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_le_bytes(<[u8; 4]>::try_from(bytes).unwrap_or([0; 4]));
    }

    let [mut a, mut b, mut c, mut d] = *state;

    for (round, t) in T.into_iter().enumerate() {
        // MD5's four rounds differ in their mixing function and, unlike SHA-1,
        // in *which* message word each step consumes. `round` is `< 64`, so
        // `round / 16` is `< 4` and the final arm is round 4.
        let quarter = round / 16;
        let step = round % 16;
        // The index arithmetic is `wrapping_*` because it is genuinely modular:
        // every result is reduced mod 16, and 2^usize::BITS is a multiple of
        // 16, so wrapping and exact arithmetic agree here. (They agree
        // trivially in any case — `step < 16` makes `7 * step` at most 105.)
        let (f, word_idx) = match quarter {
            0 => ((b & c) | (!b & d), step),
            1 => ((d & b) | (!d & c), step.wrapping_mul(5).wrapping_add(1) % 16),
            2 => (b ^ c ^ d, step.wrapping_mul(3).wrapping_add(5) % 16),
            _ => (c ^ (b | !d), step.wrapping_mul(7) % 16),
        };
        // Both indices are reduced mod 16 into arrays of length 16, so neither
        // `get` can fail; the fallbacks are unreachable.
        let word = m.get(word_idx).copied().unwrap_or(0);
        let shift = SHIFTS
            .get(quarter.saturating_mul(4).saturating_add(step % 4))
            .copied()
            .unwrap_or(0);

        let temp = d;
        d = c;
        c = b;
        b = b.wrapping_add(
            a.wrapping_add(f)
                .wrapping_add(t)
                .wrapping_add(word)
                .rotate_left(shift),
        );
        a = temp;
    }

    for (slot, delta) in state.iter_mut().zip([a, b, c, d]) {
        *slot = slot.wrapping_add(delta);
    }
}

/// A 32-character hex digest, without allocating.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Hex([u8; DIGEST_LEN * 2]);

impl Hex {
    /// The digest as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Written only by `hex` below, which emits ASCII hex digits
        // exclusively, so this is valid UTF-8 by construction.
        core::str::from_utf8(&self.0).unwrap_or("")
    }
}

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Render a digest as lowercase hex.
#[must_use]
pub fn hex(digest: &[u8; DIGEST_LEN]) -> Hex {
    let mut out = [b'0'; DIGEST_LEN * 2];
    for (pair, byte) in out.chunks_exact_mut(2).zip(digest) {
        // Both nibbles are `< 16`, so `from_digit` cannot fail; the fallback
        // is unreachable rather than load-bearing.
        let hi = char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0');
        let lo = char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0');
        pair.copy_from_slice(&[hi as u8, lo as u8]);
    }
    Hex(out)
}

/// MD5 of `data`, rendered as lowercase hex.
#[must_use]
pub fn md5_hex(data: &[u8]) -> Hex {
    hex(&md5(data))
}

#[cfg(test)]
mod tests {
    // The five defensive lints the workspace turns on are for production code:
    // a test that indexes a fixed-size fixture, or unwraps a value it just
    // constructed, is *asserting*, and a panic there is the failure being
    // reported rather than a bug being introduced.
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_used
    )]

    extern crate std;
    use super::{Md5, md5_hex};
    use std::vec::Vec;

    #[test]
    fn rfc_1321_test_suite() {
        // The seven vectors in RFC 1321 appendix A.5, in order, verbatim.
        // Together they cover the empty message, single-block messages of
        // several lengths, and one that crosses a block boundary.
        for (input, expected) in [
            (&b""[..], "d41d8cd98f00b204e9800998ecf8427e"),
            (&b"a"[..], "0cc175b9c0f1b6a831c399e269772661"),
            (&b"abc"[..], "900150983cd24fb0d6963f7d28e17f72"),
            (&b"message digest"[..], "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                &b"abcdefghijklmnopqrstuvwxyz"[..],
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                &b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"[..],
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                &b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"[..],
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            assert_eq!(
                md5_hex(input).as_str(),
                expected,
                "wrong digest for {:?}",
                core::str::from_utf8(input).unwrap_or("<non-utf8>")
            );
        }
    }

    #[test]
    fn the_padding_boundary_cases() {
        // 55 bytes is the longest message that pads into one block; 56 is the
        // shortest that needs two; 64 is a whole block with nothing left over
        // and still needs a second for the length.
        assert_eq!(
            md5_hex(&[b'y'; 55]).as_str(),
            "b8f486d6805a37c97c6b929c2e880937"
        );
        assert_eq!(
            md5_hex(&[b'z'; 56]).as_str(),
            "fa2cae340242b29bd7ded21c7bc11da1"
        );
        assert_eq!(
            md5_hex(&[b'x'; 64]).as_str(),
            "c1bb4f81d892b2d57947682aeb252456"
        );
    }

    #[test]
    fn one_million_a() {
        // Exercises the block loop rather than the padding, and confirms the
        // length field is still right past the point a 32-bit byte counter
        // would have been fine.
        let mut hasher = Md5::new();
        for _ in 0..1000 {
            hasher.update(&[b'a'; 1000]);
        }
        assert_eq!(
            super::hex(&hasher.finalize()).as_str(),
            "7707d6ae4e027c70eea2a935c2296f21"
        );
    }

    #[test]
    fn every_length_up_to_three_blocks_streams_the_same_as_one_shot() {
        for len in 0..200_usize {
            let msg: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let once = super::md5(&msg);
            for split in 0..=len {
                let mut hasher = Md5::new();
                hasher.update(&msg[..split]);
                hasher.update(&msg[split..]);
                assert_eq!(
                    hasher.finalize(),
                    once,
                    "len {len} split at {split} disagreed"
                );
            }
        }
    }

    #[test]
    fn a_single_flipped_bit_changes_the_whole_digest() {
        assert_ne!(super::md5(b"the quick brown fox"), super::md5(b"the quick brown foy"));
    }

    #[test]
    fn hex_is_lowercase_and_zero_padded() {
        // This input's digest begins with the byte 0x09. A formatter that used
        // `{:x}` instead of `{:02x}` would render it as "9" and shift every
        // later character left by one — a digest that looks fine and is wrong,
        // so it needs a vector chosen specifically to catch it.
        let rendered = md5_hex(b"pad10");
        let text = rendered.as_str();
        assert_eq!(text, "09bd587b0e885ecd2be3c81d68c4c1b7");
        assert_eq!(text.len(), 32);
        assert!(text.chars().all(|c| !c.is_ascii_uppercase()));
    }

    #[test]
    fn debug_does_not_print_the_buffer() {
        let mut hasher = Md5::new();
        hasher.update(b"correct horse battery staple");
        let shown = std::format!("{hasher:?}");
        assert!(!shown.contains("horse"), "Debug leaked the input: {shown}");
    }
}
