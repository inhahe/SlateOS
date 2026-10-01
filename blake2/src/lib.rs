//! BLAKE2b (RFC 7693), ported from the BLAKE2 reference implementation.
//!
//! # Provenance
//!
//! This is `blake2b-ref.c` from the BLAKE2 reference source package, by Samuel
//! Neves — the copy GNU coreutils 9.4 vendors as `src/blake2/blake2b-ref.c` and
//! builds `b2sum` from, read out of `coreutils-9.4.tar.xz` (SHA-256
//! `ea613a4cf44612326e917201bbbcdfbd301de21ffc3b59b6e5c07e040b275e52`). Its
//! licence is the reader's choice of CC0 1.0, the OpenSSL licence or Apache 2.0.
//!
//! Porting rather than writing is `design-decisions.md` §539: a cryptographic
//! primitive comes from an implementation other people have spent years
//! attacking, and only the plumbing around it is ours. The reference is also
//! the one `b2sum` itself runs, so for the program this crate was brought in
//! for, the arithmetic is not merely equivalent to upstream's — it is
//! upstream's.
//!
//! What was carried over, function by function:
//!
//! | reference | here |
//! |---|---|
//! | `blake2b_IV`, `blake2b_sigma` | [`IV`], [`SIGMA`] |
//! | `blake2b_init`, `blake2b_init_param` | [`Blake2b::new`] |
//! | `blake2b_init_key` | [`Blake2b::new_keyed`] |
//! | `G`, `ROUND`, `blake2b_compress` | [`g`], [`compress`] |
//! | `blake2b_increment_counter` | [`Blake2b::increment_counter`] |
//! | `blake2b_update` | [`Blake2b::update`] |
//! | `blake2b_final` | [`Blake2b::finalize`] |
//!
//! Left out: the tree-hashing parameters (`fanout`, `depth`, `leaf_length`,
//! `node_offset`, `node_depth`, `inner_length`, the salt and the
//! personalisation string) and `last_node`. [`Blake2b::new`] sets them to the
//! values `blake2b_init` sets — sequential mode, all zero — which makes the
//! parameter block's last seven words zero, and so the initial state is the IV
//! with only its first word changed.
//!
//! # The one structural difference from Merkle-Damgard hashes
//!
//! BLAKE2b compresses its final block with a flag set and no other block with
//! it, and it cannot know a block is final until the input ends. So a block
//! that fills the buffer is **held back**, not compressed: `update` compresses
//! a full buffer only once it knows more input follows (`inlen > fill`,
//! strictly, in the reference). An implementation that compressed eagerly,
//! as `blockbuf` does for SHA-2, would hash every message whose length is a
//! multiple of 128 bytes wrongly — which the vectors below at 128 and 256 bytes
//! are there to catch.

#![no_std]

use core::fmt;

/// Bytes BLAKE2b compresses at a time (`BLAKE2B_BLOCKBYTES`).
pub const BLOCK_BYTES: usize = 128;

/// The longest digest, in bytes (`BLAKE2B_OUTBYTES`): 512 bits.
pub const OUT_BYTES: usize = 64;

/// The longest key, in bytes (`BLAKE2B_KEYBYTES`).
pub const KEY_BYTES: usize = 64;

/// `blake2b_IV`: SHA-512's initial hash value, reused (RFC 7693 §2.6).
const IV: [u64; 8] = [
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
];

/// `blake2b_sigma`: the message-word permutation for each of the twelve
/// rounds. Rounds 10 and 11 reuse the first two rows (RFC 7693 §2.7).
const SIGMA: [[u8; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

/// A finished digest: up to [`OUT_BYTES`] bytes, of which the first
/// [`Digest::as_bytes`] are the hash.
///
/// A fixed array and a length rather than a `Vec`, so the crate needs no
/// allocator; the width is chosen at run time (`b2sum -l`), so it cannot be a
/// const generic either.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Digest {
    bytes: [u8; OUT_BYTES],
    len: usize,
}

impl Digest {
    /// The digest, exactly as long as was asked for.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or(&self.bytes)
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.as_bytes() {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// Hash `data` unkeyed, with an `out_len`-byte digest, in one call.
///
/// `None` if `out_len` is 0 or more than [`OUT_BYTES`] — `blake2b`'s `-1`.
#[must_use]
pub fn blake2b(out_len: usize, data: &[u8]) -> Option<Digest> {
    let mut state = Blake2b::new(out_len)?;
    state.update(data);
    Some(state.finalize())
}

/// An incremental BLAKE2b hasher: the reference's `blake2b_state`.
#[derive(Clone)]
pub struct Blake2b {
    /// Chaining value.
    h: [u64; 8],
    /// Bytes compressed so far, as a 128-bit counter in two words.
    t: [u64; 2],
    /// The final-block flag. The reference's second word is the tree-mode
    /// `last_node` flag, which sequential hashing never sets.
    last_block: bool,
    /// The held-back block: see the module docs for why it may be full.
    buf: [u8; BLOCK_BYTES],
    /// Live bytes in `buf`, `0..=BLOCK_BYTES`.
    buflen: usize,
    /// Digest length in bytes, `1..=OUT_BYTES`.
    outlen: usize,
}

impl fmt::Debug for Blake2b {
    /// Opaque: the buffer can hold a key, and after `new_keyed` it does.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Blake2b")
            .field("out_len", &self.outlen)
            .finish_non_exhaustive()
    }
}

impl Blake2b {
    /// `blake2b_init`: an unkeyed hash with an `out_len`-byte digest.
    ///
    /// `None` for an `out_len` of 0 or above [`OUT_BYTES`], where the reference
    /// returns -1.
    #[must_use]
    pub fn new(out_len: usize) -> Option<Self> {
        Self::with_key_length(out_len, 0)
    }

    /// `blake2b_init_key`: a keyed hash (a MAC). The key is padded to a full
    /// block and hashed as the first block of the message.
    ///
    /// `None` for a bad `out_len`, as [`new`](Self::new), or for an empty key or
    /// one longer than [`KEY_BYTES`].
    #[must_use]
    pub fn new_keyed(out_len: usize, key: &[u8]) -> Option<Self> {
        if key.is_empty() || key.len() > KEY_BYTES {
            return None;
        }
        let mut state = Self::with_key_length(out_len, key.len())?;
        // The reference pads the key to a block on its stack, `update`s it,
        // and wipes the stack copy with `secure_zero_memory`. On a fresh state
        // that `update` only *stores* the block — a full buffer is held back
        // until more input follows — so writing the key straight into the
        // buffer is the same state, and leaves no second copy to wipe.
        state.buf.get_mut(..key.len())?.copy_from_slice(key);
        state.buflen = BLOCK_BYTES;
        Some(state)
    }

    /// `blake2b_init_param` over the parameter block `blake2b_init` builds:
    /// digest length, key length, fanout 1, depth 1, everything else zero.
    fn with_key_length(out_len: usize, key_len: usize) -> Option<Self> {
        if out_len == 0 || out_len > OUT_BYTES {
            return None;
        }
        let out = u64::try_from(out_len).ok()?;
        let key = u64::try_from(key_len).ok()?;
        // The parameter block's first eight bytes, little-endian: digest
        // length, key length, fanout, depth.
        let param0 = out | (key << 8) | (1 << 16) | (1 << 24);
        let mut h = IV;
        if let Some(first) = h.first_mut() {
            *first ^= param0;
        }
        Some(Self {
            h,
            t: [0, 0],
            last_block: false,
            buf: [0; BLOCK_BYTES],
            buflen: 0,
            outlen: out_len,
        })
    }

    /// `blake2b_increment_counter`: add `inc` to the 128-bit byte counter.
    fn increment_counter(&mut self, inc: u64) {
        let [lo, hi] = &mut self.t;
        *lo = lo.wrapping_add(inc);
        *hi = hi.wrapping_add(u64::from(*lo < inc));
    }

    /// `blake2b_update`. A full buffer is compressed only when more input is
    /// known to follow, so the final block is always still here for
    /// [`finalize`](Self::finalize).
    pub fn update(&mut self, data: &[u8]) {
        let mut input = data;
        if input.is_empty() {
            return;
        }
        let left = self.buflen;
        let fill = BLOCK_BYTES.saturating_sub(left);
        if input.len() > fill {
            // Fill the buffer and compress it: more input follows, so it is
            // not the last block.
            self.buflen = 0;
            if let (Some(dst), Some(src)) = (self.buf.get_mut(left..), input.get(..fill)) {
                dst.copy_from_slice(src);
            }
            self.increment_counter(BLOCK_BYTES as u64);
            let block = self.buf;
            compress(&mut self.h, &block, self.t, false);
            input = input.get(fill..).unwrap_or(&[]);

            // Whole blocks straight from the input, while *more than* one
            // block remains — the last one, even if full, is held back.
            while input.len() > BLOCK_BYTES {
                let Some((block, rest)) = input.split_first_chunk::<BLOCK_BYTES>() else {
                    break;
                };
                self.increment_counter(BLOCK_BYTES as u64);
                compress(&mut self.h, block, self.t, false);
                input = rest;
            }
        }
        let end = self.buflen.saturating_add(input.len());
        if let Some(dst) = self.buf.get_mut(self.buflen..end) {
            dst.copy_from_slice(input);
        }
        self.buflen = end;
    }

    /// `blake2b_final`: count the held-back bytes, zero-pad them to a block,
    /// compress it with the final-block flag, and return the first `out_len`
    /// bytes of the state, little-endian.
    ///
    /// Consumes the hasher, which is how the reference's `is_lastblock` refusal
    /// to finalise twice is expressed here: as a compile error.
    #[must_use]
    pub fn finalize(mut self) -> Digest {
        // `buflen <= BLOCK_BYTES`, so the cast cannot truncate.
        self.increment_counter(self.buflen as u64);
        self.last_block = true;
        if let Some(pad) = self.buf.get_mut(self.buflen..) {
            pad.fill(0);
        }
        let block = self.buf;
        compress(&mut self.h, &block, self.t, self.last_block);

        let mut bytes = [0u8; OUT_BYTES];
        for (slot, word) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(self.h) {
            *slot = word.to_le_bytes();
        }
        // Only `out_len` bytes are the digest. The reference copies exactly
        // that many out and wipes its staging buffer; here the staging buffer
        // *is* the result, so the rest of it is cleared instead of returned.
        if let Some(tail) = bytes.get_mut(self.outlen..) {
            tail.fill(0);
        }
        Digest {
            bytes,
            len: self.outlen,
        }
    }
}

/// The reference's `G` macro: one quarter-round, mixing two message words into
/// four state words. The rotation amounts are RFC 7693 §2.1's R1..R4.
#[inline(always)]
fn g(v: &mut [u64; 16], [a, b, c, d]: [usize; 4], x: u64, y: u64) {
    // The indices come from the fixed table in `compress`, all below 16, so
    // every `get` succeeds; the fallbacks exist only to avoid indexing.
    let (Some(&va), Some(&vb), Some(&vc), Some(&vd)) = (v.get(a), v.get(b), v.get(c), v.get(d))
    else {
        return;
    };
    let va = va.wrapping_add(vb).wrapping_add(x);
    let vd = (vd ^ va).rotate_right(32);
    let vc = vc.wrapping_add(vd);
    let vb = (vb ^ vc).rotate_right(24);
    let va = va.wrapping_add(vb).wrapping_add(y);
    let vd = (vd ^ va).rotate_right(16);
    let vc = vc.wrapping_add(vd);
    let vb = (vb ^ vc).rotate_right(63);
    for (i, value) in [(a, va), (b, vb), (c, vc), (d, vd)] {
        if let Some(slot) = v.get_mut(i) {
            *slot = value;
        }
    }
}

/// The reference's `ROUND`: the eight `G` applications, columns then
/// diagonals, as `(state indices)` in the reference's order.
const ROUND: [[usize; 4]; 8] = [
    [0, 4, 8, 12],
    [1, 5, 9, 13],
    [2, 6, 10, 14],
    [3, 7, 11, 15],
    [0, 5, 10, 15],
    [1, 6, 11, 12],
    [2, 7, 8, 13],
    [3, 4, 9, 14],
];

/// `blake2b_compress`: twelve rounds over one block. `t` is the byte counter
/// *after* this block, and `last` the final-block flag.
fn compress(h: &mut [u64; 8], block: &[u8; BLOCK_BYTES], t: [u64; 2], last: bool) {
    let mut m = [0u64; 16];
    for (word, bytes) in m.iter_mut().zip(block.as_chunks::<8>().0) {
        *word = u64::from_le_bytes(*bytes);
    }

    let mut v = [0u64; 16];
    let (low, high) = v.split_at_mut(8);
    low.copy_from_slice(h);
    high.copy_from_slice(&IV);
    let [t0, t1] = t;
    let f0 = if last { u64::MAX } else { 0 };
    // v[12..16] ^= t[0], t[1], f[0], f[1]; `f[1]` (last_node) is always 0.
    for (slot, x) in high.iter_mut().skip(4).zip([t0, t1, f0, 0]) {
        *slot ^= x;
    }

    for sigma in &SIGMA {
        for (i, &quad) in ROUND.iter().enumerate() {
            // `m[sigma[r][2*i]]` and `m[sigma[r][2*i + 1]]`: `i < 8`, so both
            // positions are in the sixteen-entry row, and every entry of a row
            // is below sixteen, so both words exist.
            let pair = sigma
                .get(i.saturating_mul(2)..)
                .and_then(<[u8]>::first_chunk::<2>);
            let (x, y) = match pair {
                Some(&[p, q]) => (
                    m.get(usize::from(p)).copied().unwrap_or(0),
                    m.get(usize::from(q)).copied().unwrap_or(0),
                ),
                None => (0, 0),
            };
            g(&mut v, quad, x, y);
        }
    }

    let (low, high) = v.split_at(8);
    for ((acc, a), b) in h.iter_mut().zip(low).zip(high) {
        *acc ^= a ^ b;
    }
}

// Tests assert on fixed fixtures; a panic is the failure being reported.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::cast_possible_truncation
)]
#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    fn hex(d: &Digest) -> String {
        format!("{d:?}")
    }

    fn unkeyed(out_len: usize, data: &[u8]) -> String {
        hex(&blake2b(out_len, data).unwrap())
    }

    // -- RFC 7693 Appendix A --

    #[test]
    fn rfc_7693_appendix_a() {
        assert_eq!(
            unkeyed(64, b"abc"),
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d1\
             7d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
        );
    }

    // -- The reference's own self-test: keyed, key = 00..3f, message = 00.. --
    // The entries of `blake2b_keyed_kat` at these lengths, cross-checked
    // against Python's `hashlib.blake2b`, an independent implementation.

    fn kat(len: usize) -> String {
        let key: Vec<u8> = (0..64u8).collect();
        let msg: Vec<u8> = (0..len).map(|i| i as u8).collect();
        let mut state = Blake2b::new_keyed(OUT_BYTES, &key).unwrap();
        state.update(&msg);
        hex(&state.finalize())
    }

    #[test]
    fn the_reference_keyed_vectors() {
        for (len, want) in [
            (
                0,
                "10ebb67700b1868efb4417987acf4690ae9d972fb7a590c2f02871799aaa4786\
                 b5e996e8f0f4eb981fc214b005f42d2ff4233499391653df7aefcbc13fc51568",
            ),
            (
                1,
                "961f6dd1e4dd30f63901690c512e78e4b45e4742ed197c3c5e45c549fd25f2e4\
                 187b0bc9fe30492b16b0d0bc4ef9b0f34c7003fac09a5ef1532e69430234cebd",
            ),
            (
                63,
                "bd965bf31e87d70327536f2a341cebc4768eca275fa05ef98f7f1b71a0351298\
                 de006fba73fe6733ed01d75801b4a928e54231b38e38c562b2e33ea1284992fa",
            ),
            (
                127,
                "76d2d819c92bce55fa8e092ab1bf9b9eab237a25267986cacf2b8ee14d214d73\
                 0dc9a5aa2d7b596e86a1fd8fa0804c77402d2fcd45083688b218b1cdfa0dcbcb",
            ),
            (
                128,
                "72065ee4dd91c2d8509fa1fc28a37c7fc9fa7d5b3f8ad3d0d7a25626b57b1b44\
                 788d4caf806290425f9890a3a2a35a905ab4b37acfd0da6e4517b2525c9651e4",
            ),
            (
                129,
                "64475dfe7600d7171bea0b394e27c9b00d8e74dd1e416a79473682ad3dfdbb70\
                 6631558055cfc8a40e07bd015a4540dcdea15883cbbf31412df1de1cd4152b91",
            ),
            (
                255,
                "142709d62e28fcccd0af97fad0f8465b971e82201dc51070faa0372aa43e9248\
                 4be1c1e73ba10906d5d1853db6a4106e0a7bf9800d373d6dee2d46d62ef2a461",
            ),
        ] {
            assert_eq!(kat(len), want, "keyed KAT at {len} bytes");
        }
    }

    // -- Unkeyed, at the widths `b2sum -l` picks and the held-back boundary --

    #[test]
    fn the_empty_message_at_several_widths() {
        for (out, want) in [
            (
                64,
                "786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419\
                 d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce",
            ),
            (
                32,
                "0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8",
            ),
            (20, "3345524abf6bbe1809449224b5972c41790b6cf2"),
            (1, "2e"),
        ] {
            assert_eq!(unkeyed(out, b""), want, "{out}-byte digest of nothing");
        }
    }

    #[test]
    fn a_narrower_digest_is_a_different_hash_not_a_prefix() {
        // The output length is in the parameter block, so BLAKE2b-256 is not
        // the first half of BLAKE2b-512 — the property `b2sum -l` depends on.
        let wide = unkeyed(64, b"abc");
        let narrow = unkeyed(32, b"abc");
        assert_eq!(
            narrow,
            "bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319"
        );
        assert!(!wide.starts_with(&narrow));
    }

    #[test]
    fn whole_blocks_are_held_back_for_the_final_flag() {
        for (len, want) in [
            (
                127,
                "acc1cd9ebcd76c8f0e9afcfe2e2479a1ab53ad0d02c8ebd767fd1e26c5cf9676\
                 7c81077e5edd55f3fd8709dea6849b9792c8c19813f5ae6de9ac4d3a5efad515",
            ),
            (
                128,
                "082b91ea2e15d1556d2ceefdd5af5d64d31b4e01aff1959724578876293825b2\
                 36ee8079173a0a38160d7d6685d6bca0bfb62c177b3599b8727d9173e2115b91",
            ),
            (
                129,
                "362a53bbe2ec08097b2f358a41d0e153aeed4c132af928400872413650e7bf22\
                 f9ae428ff73770170bbd95f935e5dd1953c17de8c7264c72d1f99303bf22dfaa",
            ),
            (
                256,
                "26066ae992ec734e85f05f962b49e72bcb2be54fcb53bce7e7b4d7f4dc88f568\
                 62235fd16b988877db71cc5e9bb50e489e884450fdb6f74968e6da7d1e493428",
            ),
            (
                257,
                "277871d7679a32a3bed9c5683b726e2d1d40df26a6d80b9ee28391f07f7bed61\
                 1e82eb6b301c7380f9adb0c1c6689b3bf07ffe3871334b1526cf30e1cc67e03c",
            ),
        ] {
            assert_eq!(unkeyed(64, &[b'x'; 257][..len]), want, "{len} bytes");
        }
    }

    #[test]
    fn one_million_a() {
        let mut state = Blake2b::new(64).unwrap();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            state.update(&chunk);
        }
        assert_eq!(
            hex(&state.finalize()),
            "98fb3efb7206fd19ebf69b6f312cf7b64e3b94dbe1a17107913975a793f177e1\
             d077609d7fba363cbba00d05f7aa4e4fa8715d6428104c0a75643b0ff3fd3eaf"
        );
    }

    // -- Streaming: the reference's second self-test loop, every step size --

    #[test]
    fn every_split_agrees_with_one_shot() {
        let msg: Vec<u8> = (0..300_u32).map(|i| (i % 251) as u8).collect();
        for len in [0, 1, 127, 128, 129, 255, 256, 257, 300] {
            let whole = unkeyed(64, &msg[..len]);
            for split in 0..=len {
                let mut state = Blake2b::new(64).unwrap();
                state.update(&msg[..split]);
                state.update(&msg[split..len]);
                assert_eq!(hex(&state.finalize()), whole, "len {len} split {split}");
            }
        }
    }

    #[test]
    fn step_sizes_below_a_block_agree_with_one_shot() {
        let msg: Vec<u8> = (0..256_u32).map(|i| i as u8).collect();
        let whole = kat(256);
        let key: Vec<u8> = (0..64u8).collect();
        for step in 1..BLOCK_BYTES {
            let mut state = Blake2b::new_keyed(OUT_BYTES, &key).unwrap();
            for piece in msg.chunks(step) {
                state.update(piece);
            }
            assert_eq!(hex(&state.finalize()), whole, "step {step}");
        }
    }

    // -- Refusals, as the reference's -1 --

    #[test]
    fn bad_lengths_are_refused() {
        assert!(Blake2b::new(0).is_none());
        assert!(Blake2b::new(65).is_none());
        assert!(Blake2b::new(64).is_some());
        assert!(Blake2b::new_keyed(64, b"").is_none());
        assert!(Blake2b::new_keyed(64, &[0u8; 65]).is_none());
        assert!(Blake2b::new_keyed(64, &[0u8; 64]).is_some());
    }

    #[test]
    fn debug_shows_neither_the_key_nor_the_buffer() {
        let state = Blake2b::new_keyed(32, b"secret-key").unwrap();
        let rendered = format!("{state:?}");
        assert!(!rendered.contains("secret"), "{rendered}");
        assert!(rendered.contains("32"), "{rendered}");
    }
}
