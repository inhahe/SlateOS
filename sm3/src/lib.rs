//! SM3 (GB/T 32905-2016, ISO/IEC 10118-3:2018), ported from RustCrypto's `sm3`.
//!
//! # Provenance
//!
//! The compression function, its round constants and the initial value are
//! **ported, not written**: they come from RustCrypto's `sm3` crate, version
//! 0.5.0 (git `b5051e5a5e7dc86a6c27c1ec7a390744ebcfb97a`), files
//! `sm3/src/compress.rs` and `sm3/src/consts.rs`, under `MIT OR Apache-2.0`.
//! The `.crate` it was read from has SHA-256
//! `da6a89ba31723d185fd7413b98c576a575f356d9b84729d8ecb6ead60000a5b6`.
//! Porting rather than writing is `design-decisions.md` §539.
//!
//! Upstream unrolls the sixty-four rounds into sixty-four macro calls that
//! rename the eight registers instead of moving them, and keeps the message
//! schedule in a sixteen-word ring. This port keeps upstream's round functions
//! (`ff1`, `ff2`, `gg1`, `gg2`, `p0`, `p1`, spelled as upstream spells them,
//! `gg2`'s `(y ^ z) & x ^ z` included) and its pre-rotated constant table, and
//! writes the round as the loop GB/T 32905 §5.3.3 writes it: the renaming
//! becomes the standard's register shift, and the ring becomes the standard's
//! sixty-eight-word expansion. Same words, same order; the standard's two
//! example vectors and a million-`a` run below are what say so.
//!
//! The buffering and the length padding — SHA-256's exactly: a `0x80`, zeros,
//! and the bit length as a big-endian 64-bit field — are `blockbuf`'s.
//!
//! # What it is for
//!
//! `cksum -a sm3`, which GNU coreutils has offered since 9.0 and which is the
//! only way to reach SM3 from a shell there too. It is a Chinese national
//! standard, used where that standard is required.

#![no_std]

use blockbuf::{BlockBuffer, LengthOrder};
use core::fmt;

/// The block size SM3 compresses, in bytes.
const BLOCK_LEN: usize = 64;

/// Bytes of SM3 output.
pub const DIGEST_LEN: usize = 32;

/// The initial value IV (GB/T 32905 §4.1).
const H0: [u32; 8] = [
    0x7380_166f,
    0x4914_b2b9,
    0x1724_42d7,
    0xda8a_0600,
    0xa96f_30bc,
    0x1631_38aa,
    0xe38d_ee4d,
    0xb0fb_0e4e,
];

/// `T_j <<< (j mod 32)`, precomputed: `T_j` is 0x79cc4519 for the first
/// sixteen rounds and 0x7a879d8a after (GB/T 32905 §4.2). Upstream's `T32`.
const T32: [u32; 64] = [
    0x79cc_4519,
    0xf398_8a32,
    0xe731_1465,
    0xce62_28cb,
    0x9cc4_5197,
    0x3988_a32f,
    0x7311_465e,
    0xe622_8cbc,
    0xcc45_1979,
    0x988a_32f3,
    0x3114_65e7,
    0x6228_cbce,
    0xc451_979c,
    0x88a3_2f39,
    0x1146_5e73,
    0x228c_bce6,
    0x9d8a_7a87,
    0x3b14_f50f,
    0x7629_ea1e,
    0xec53_d43c,
    0xd8a7_a879,
    0xb14f_50f3,
    0x629e_a1e7,
    0xc53d_43ce,
    0x8a7a_879d,
    0x14f5_0f3b,
    0x29ea_1e76,
    0x53d4_3cec,
    0xa7a8_79d8,
    0x4f50_f3b1,
    0x9ea1_e762,
    0x3d43_cec5,
    0x7a87_9d8a,
    0xf50f_3b14,
    0xea1e_7629,
    0xd43c_ec53,
    0xa879_d8a7,
    0x50f3_b14f,
    0xa1e7_629e,
    0x43ce_c53d,
    0x879d_8a7a,
    0x0f3b_14f5,
    0x1e76_29ea,
    0x3cec_53d4,
    0x79d8_a7a8,
    0xf3b1_4f50,
    0xe762_9ea1,
    0xcec5_3d43,
    0x9d8a_7a87,
    0x3b14_f50f,
    0x7629_ea1e,
    0xec53_d43c,
    0xd8a7_a879,
    0xb14f_50f3,
    0x629e_a1e7,
    0xc53d_43ce,
    0x8a7a_879d,
    0x14f5_0f3b,
    0x29ea_1e76,
    0x53d4_3cec,
    0xa7a8_79d8,
    0x4f50_f3b1,
    0x9ea1_e762,
    0x3d43_cec5,
];

/// Hash `data` in one call.
#[must_use]
pub fn sm3(data: &[u8]) -> [u8; DIGEST_LEN] {
    let mut hasher = Sm3::new();
    hasher.update(data);
    hasher.finalize()
}

/// An incremental SM3 hasher.
#[derive(Clone)]
pub struct Sm3 {
    /// Chaining value: the hash of everything compressed so far.
    state: [u32; 8],
    /// The partial block, the byte count and the padding rule.
    buffer: BlockBuffer<BLOCK_LEN>,
}

impl Default for Sm3 {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Sm3 {
    /// Deliberately opaque, as every hasher in this tree is: the buffer holds
    /// whatever is being hashed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sm3")
            .field("bytes_hashed", &self.buffer.bytes_absorbed())
            .finish_non_exhaustive()
    }
}

impl Sm3 {
    /// Start a new hash.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: H0,
            buffer: BlockBuffer::new(),
        }
    }

    /// Feed `data` in. Splitting the same input differently across calls gives
    /// the same digest.
    pub fn update(&mut self, data: &[u8]) {
        let state = &mut self.state;
        self.buffer.update(data, |block| compress(state, block));
    }

    /// Finish, returning the 32-byte digest. Consumes the hasher, because a
    /// padded state cannot be extended.
    #[must_use]
    pub fn finalize(mut self) -> [u8; DIGEST_LEN] {
        let state = &mut self.state;
        self.buffer
            .finalize(LengthOrder::BigEndian, |block| compress(state, block));

        let mut out = [0u8; DIGEST_LEN];
        for (slot, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *slot = word.to_be_bytes();
        }
        out
    }
}

// -- Upstream's round functions, verbatim --

#[inline(always)]
fn ff1(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

#[inline(always)]
fn ff2(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (x & z) | (y & z)
}

#[inline(always)]
fn gg1(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

#[inline(always)]
fn gg2(x: u32, y: u32, z: u32) -> u32 {
    // Upstream's note: equivalent to `(x & y) | (!x & z)`, but faster.
    (y ^ z) & x ^ z
}

#[inline(always)]
fn p0(x: u32) -> u32 {
    x ^ x.rotate_left(9) ^ x.rotate_left(17)
}

#[inline(always)]
fn p1(x: u32) -> u32 {
    x ^ x.rotate_left(15) ^ x.rotate_left(23)
}

/// One application of the SM3 compression function to a 64-byte block.
fn compress(state: &mut [u32; 8], block: &[u8; BLOCK_LEN]) {
    // Message expansion (GB/T 32905 §5.3.2): the block, big-endian, then
    // fifty-two more words by upstream's `w2` recurrence. `prev` is the sixteen
    // words before `j`: prev[0] is W[j-16], prev[3] W[j-13], prev[7] W[j-9],
    // prev[10] W[j-6] and prev[13] W[j-3].
    let mut w = [0u32; 68];
    for (word, bytes) in w.iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_be_bytes(*bytes);
    }
    for j in 16..68 {
        let Some(prev) = w.get(..j).and_then(<[u32]>::last_chunk::<16>) else {
            // Unreachable: `16 <= j < 68 == w.len()`.
            continue;
        };
        let tw = prev[0] ^ prev[7] ^ prev[13].rotate_left(15);
        let next = p1(tw) ^ prev[3].rotate_left(7) ^ prev[10];
        if let Some(slot) = w.get_mut(j) {
            *slot = next;
        }
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;

    // Round j uses W[j] and W[j + 4]; `ahead` walks four words in front.
    for (j, ((t, &wj), &wj4)) in T32.iter().zip(&w).zip(w.iter().skip(4)).enumerate() {
        let a12 = a.rotate_left(12);
        let ss1 = a12.wrapping_add(e).wrapping_add(*t).rotate_left(7);
        let ss2 = ss1 ^ a12;
        let (ff, gg) = if j < 16 {
            (ff1(a, b, c), gg1(e, f, g))
        } else {
            (ff2(a, b, c), gg2(e, f, g))
        };
        let tt1 = ff.wrapping_add(d).wrapping_add(ss2).wrapping_add(wj ^ wj4);
        let tt2 = gg.wrapping_add(h).wrapping_add(ss1).wrapping_add(wj);

        d = c;
        c = b.rotate_left(9);
        b = a;
        a = tt1;
        h = g;
        g = f.rotate_left(19);
        f = e;
        e = p0(tt2);
    }

    // SM3 chains with XOR, where SHA-2 adds.
    let round = [a, b, c, d, e, f, g, h];
    for (acc, delta) in state.iter_mut().zip(round) {
        *acc ^= delta;
    }
}

// Tests assert on fixed fixtures; a panic is the failure being reported.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::string::String;
    use std::vec::Vec;

    fn hex(bytes: &[u8]) -> String {
        use core::fmt::Write as _;
        let mut out = String::new();
        for b in bytes {
            write!(out, "{b:02x}").unwrap();
        }
        out
    }

    // -- GB/T 32905-2016 Appendix A --

    #[test]
    fn standard_example_1_abc() {
        assert_eq!(
            hex(&sm3(b"abc")),
            "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0"
        );
    }

    #[test]
    fn standard_example_2_sixty_four_bytes() {
        // Exactly one block of message, so the padding takes a second one.
        assert_eq!(
            hex(&sm3(&b"abcd".repeat(16))),
            "debe9ff92275b8a138604889c18e5a4d6fdb70e5387e5765293dcba39c0c5732"
        );
    }

    // -- Cross-checked against Python's `hashlib.new("sm3")` (OpenSSL), an
    // independent implementation --

    #[test]
    fn the_empty_message() {
        assert_eq!(
            hex(&sm3(b"")),
            "1ab21d8355cfa17f8e61194831e81a8f22bec8c728fefb747ed035eb5082aa2b"
        );
    }

    #[test]
    fn the_padding_boundaries() {
        for (len, want) in [
            (
                55,
                "ff8f8d58b95a1f90e39d96f739fa873eee33c0a80e59c7bbbf184eb7d9b1f112",
            ),
            (
                56,
                "c4c1c6206d36c325e66ae5432948b26f04acff8dfc0ea2606a79d59b83a16d61",
            ),
            (
                64,
                "ad6af8cbe6a5a10beac0ecd1df9fa779228553fd7ead00313765349b32bd81ce",
            ),
            (
                119,
                "ebeb591c289fe14873d297f6d2fc8c70e6ed03a2ff6e2240600eede2e8197bbf",
            ),
            (
                120,
                "f86deedb441db075511ff2ae5ace229719f7d0e754af15391be725e4e553cc4a",
            ),
            (
                128,
                "96f6390c8b42e871f8001e3aa1856ff360c3668e4fcd5f6c2d74adc457847349",
            ),
        ] {
            assert_eq!(hex(&sm3(&[b'x'; 128][..len])), want, "{len} bytes");
        }
    }

    #[test]
    fn one_million_a() {
        let mut hasher = Sm3::new();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            hasher.update(&chunk);
        }
        assert_eq!(
            hex(&hasher.finalize()),
            "c8aaf89429554029e231941a2acc0ad61ff2a5acd8fadd25847a3a732b3b02c3"
        );
    }

    #[test]
    fn every_split_agrees_with_one_shot() {
        let data: Vec<u8> = (0..200_u32).map(|i| (i % 251) as u8).collect();
        let whole = sm3(&data);
        for split in 0..=data.len() {
            let mut hasher = Sm3::new();
            hasher.update(&data[..split]);
            hasher.update(&data[split..]);
            assert_eq!(hasher.finalize(), whole, "split at {split}");
        }
    }

    #[test]
    fn the_pre_rotated_constants_are_the_standards() {
        // T32[j] is T_j rotated left by j mod 32 — upstream's precomputation,
        // re-derived here from the standard's two base constants.
        for (j, t) in T32.iter().enumerate() {
            let base: u32 = if j < 16 { 0x79cc_4519 } else { 0x7a87_9d8a };
            assert_eq!(*t, base.rotate_left((j % 32) as u32), "T32[{j}]");
        }
    }

    #[test]
    fn debug_does_not_print_the_buffer() {
        let mut hasher = Sm3::new();
        hasher.update(b"hunter2");
        let rendered = std::format!("{hasher:?}");
        assert!(!rendered.contains("hunter2") && !rendered.contains("104"));
    }
}
