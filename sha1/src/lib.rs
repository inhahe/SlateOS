//! SHA-1, written once, checked against the FIPS 180-4 vectors.
//!
//! # SHA-1 is broken, and this crate is still the right thing to have
//!
//! SHA-1 has been collision-broken in practice since 2017 (SHAttered), and
//! chosen-prefix collisions have been demonstrated since 2020. **Nothing in
//! this tree should use it to decide whether to trust something.** If you are
//! reaching for it to verify a signature, authenticate a message, or address
//! content, you want [`sha2`](../sha2/index.html) instead.
//!
//! It exists here for the two jobs that are not about trust:
//!
//! - **Reproducing a checksum someone else published.** `apps/diskimager`
//!   needs to tell a user whether the image they downloaded matches the
//!   `sha1sum` on the download page. Whether SHA-1 is a good choice is the
//!   publisher's decision, already made; our job is to compute the same
//!   function they did. An implementation that refused to would simply be
//!   unable to answer the question.
//! - **Protocols that specify it.** The WebSocket opening handshake (RFC 6455
//!   §4.2.2) is defined in terms of SHA-1 over a fixed GUID. There is no
//!   security claim resting on it — it exists to prove the server understood
//!   the request — and no freedom to substitute anything else.
//!
//! # Usage
//!
//! ```
//! # use sha1::{sha1, hex};
//! assert_eq!(
//!     hex(&sha1(b"abc")).as_str(),
//!     "a9993e364706816aba3e25717850c26c9cd0d89d"
//! );
//! ```
//!
//! For data that does not arrive all at once:
//!
//! ```
//! # use sha1::Sha1;
//! let mut hasher = Sha1::new();
//! hasher.update(b"a");
//! hasher.update(b"bc");
//! assert_eq!(hasher.finalize(), sha1::sha1(b"abc"));
//! ```

#![no_std]

use blockbuf::{BlockBuffer, LengthOrder};
use core::fmt;

/// The block size SHA-1 compresses, in bytes.
const BLOCK_LEN: usize = 64;

/// Bytes of digest output.
pub const DIGEST_LEN: usize = 20;

/// Initial hash value (FIPS 180-4 §5.3.1).
const INIT: [u32; 5] = [
    0x6745_2301,
    0xefcd_ab89,
    0x98ba_dcfe,
    0x1032_5476,
    0xc3d2_e1f0,
];

/// Round constants, one per twenty rounds (FIPS 180-4 §4.2.1).
const K: [u32; 4] = [0x5a82_7999, 0x6ed9_eba1, 0x8f1b_bcdc, 0xca62_c1d6];

/// SHA-1 of `data`.
#[must_use]
pub fn sha1(data: &[u8]) -> [u8; DIGEST_LEN] {
    let mut hasher = Sha1::new();
    hasher.update(data);
    hasher.finalize()
}

/// An in-progress SHA-1, for data that arrives in pieces.
#[derive(Clone)]
pub struct Sha1 {
    state: [u32; 5],
    buffer: BlockBuffer<BLOCK_LEN>,
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Sha1 {
    /// Deliberately opaque; see [`blockbuf::BlockBuffer`]'s `Debug`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sha1")
            .field("bytes_hashed", &self.buffer.bytes_absorbed())
            .finish_non_exhaustive()
    }
}

impl Sha1 {
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

    /// Finish, returning the 20-byte digest.
    ///
    /// Consumes the hasher: the padding is part of the hashed message, so a
    /// finalised state cannot be extended. Taking `self` by value makes that a
    /// compile error rather than a wrong answer.
    #[must_use]
    pub fn finalize(mut self) -> [u8; DIGEST_LEN] {
        let state = &mut self.state;
        self.buffer
            .finalize(LengthOrder::BigEndian, |block| compress(state, block));

        let mut out = [0u8; DIGEST_LEN];
        for (slot, word) in out.chunks_exact_mut(4).zip(self.state) {
            slot.copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

/// One application of the SHA-1 compression function to a 64-byte block.
fn compress(state: &mut [u32; 5], block: &[u8; BLOCK_LEN]) {
    // Message schedule: the sixteen block words, then sixty-four more from a
    // recurrence over them.
    let mut w = [0u32; 80];
    for (word, bytes) in w.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_be_bytes(<[u8; 4]>::try_from(bytes).unwrap_or([0; 4]));
    }
    // Taking the previous sixteen words as a `last_chunk` of the filled prefix,
    // rather than as `w[i - 3]` and friends, is what lets the offsets be
    // compile-time-checked constants into a `&[u32; 16]` instead of runtime
    // bounds checks on computed indices. Same trick as `sha2`.
    for i in 16..80 {
        let Some(prev) = w.get(..i).and_then(<[u32]>::last_chunk::<16>) else {
            // Unreachable: `16 <= i < 80 == w.len()`.
            continue;
        };
        // prev[0] is w[i-16], prev[2] is w[i-14], prev[8] is w[i-8],
        // prev[13] is w[i-3].
        let next = (prev[13] ^ prev[8] ^ prev[2] ^ prev[0]).rotate_left(1);
        if let Some(slot) = w.get_mut(i) {
            *slot = next;
        }
    }

    let [mut a, mut b, mut c, mut d, mut e] = *state;

    for (round, word) in w.into_iter().enumerate() {
        // The round function and constant change every twenty rounds. `round`
        // is `< 80` by construction, so the final arm is the 60..80 quarter.
        let (f, k) = match round / 20 {
            0 => ((b & c) | (!b & d), K[0]),
            1 => (b ^ c ^ d, K[1]),
            2 => ((b & c) | (b & d) | (c & d), K[2]),
            _ => (b ^ c ^ d, K[3]),
        };
        let temp = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(word);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = temp;
    }

    for (slot, delta) in state.iter_mut().zip([a, b, c, d, e]) {
        *slot = slot.wrapping_add(delta);
    }
}

/// A 40-character hex digest, without allocating.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Hex([u8; DIGEST_LEN * 2]);

impl Hex {
    /// The digest as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // The bytes are written only by `hex` below, which emits ASCII hex
        // digits exclusively, so this is valid UTF-8 by construction.
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

/// SHA-1 of `data`, rendered as lowercase hex.
#[must_use]
pub fn sha1_hex(data: &[u8]) -> Hex {
    hex(&sha1(data))
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
    use super::{Sha1, sha1_hex};
    use std::vec::Vec;

    #[test]
    fn fips_empty_string() {
        assert_eq!(
            sha1_hex(b"").as_str(),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }

    #[test]
    fn fips_abc() {
        assert_eq!(
            sha1_hex(b"abc").as_str(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn fips_two_block_message() {
        assert_eq!(
            sha1_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq").as_str(),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn fips_one_million_a() {
        // The vector that exercises the block loop rather than the padding.
        let mut hasher = Sha1::new();
        for _ in 0..1000 {
            hasher.update(&[b'a'; 1000]);
        }
        assert_eq!(
            super::hex(&hasher.finalize()).as_str(),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    #[test]
    fn the_padding_boundary_cases() {
        // 55 bytes is the longest message that pads into one block; 56 is the
        // shortest that needs two; 64 is a whole block with nothing left over
        // and still needs a second for the length.
        assert_eq!(
            sha1_hex(&[b'y'; 55]).as_str(),
            "f3c8b47e97bc2a23d9870c16d129390bf78225bb"
        );
        assert_eq!(
            sha1_hex(&[b'z'; 56]).as_str(),
            "aaf29dd7fdb380d32791213ae5acf0f6cea0c5e3"
        );
        assert_eq!(
            sha1_hex(&[b'x'; 64]).as_str(),
            "bb2fa3ee7afb9f54c6dfb5d021f14b1ffe40c163"
        );
    }

    #[test]
    fn every_length_up_to_three_blocks_streams_the_same_as_one_shot() {
        for len in 0..200_usize {
            let msg: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let once = super::sha1(&msg);
            for split in 0..=len {
                let mut hasher = Sha1::new();
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
        let a = super::sha1(b"the quick brown fox");
        let b = super::sha1(b"the quick brown fo\x79");
        assert_ne!(a, b);
    }

    #[test]
    fn hex_is_lowercase_and_zero_padded() {
        assert_eq!(
            sha1_hex(b"message digest").as_str(),
            "c12252ceda8be8994d5fa0290a47231c1d16aae3"
        );
        // This input's digest begins with the byte 0x08. A formatter that used
        // `{:x}` instead of `{:02x}` would render it as "8" and shift every
        // later character left by one — a digest that looks fine and is wrong,
        // so it needs a vector chosen specifically to catch it.
        let rendered = sha1_hex(b"pad10");
        let text = rendered.as_str();
        assert_eq!(text, "08046917d057970fc1594afa3e7479ca1fadad23");
        assert_eq!(text.len(), 40);
        assert!(text.chars().all(|c| !c.is_ascii_uppercase()));
    }

    #[test]
    fn debug_does_not_print_the_buffer() {
        let mut hasher = Sha1::new();
        hasher.update(b"correct horse battery staple");
        let shown = std::format!("{hasher:?}");
        assert!(!shown.contains("horse"), "Debug leaked the input: {shown}");
    }
}
