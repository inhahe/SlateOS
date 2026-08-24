//! `md5sum` — print or check MD5 (128-bit) checksums.
//!
//! Everything except the hash itself lives in [`coreutils::digest`], which is
//! upstream's `src/digest.c`: the option table, the three checksum-file
//! formats, `--check`, the name escaping and the exit statuses. Upstream
//! compiles that one file eight times with a different `HASH_ALGO_*`; here the
//! same effect is a single [`Algorithm`] constant, so this file is the RFC 1321
//! transform and nothing else.
//!
//! # Why the hash is streaming and not `fn(&[u8]) -> [u8; 16]`
//!
//! The version this replaced read the whole file into a `Vec` and then hashed
//! it. `md5sum` on a disk image is an ordinary thing to do and that shape
//! answers it with an allocation the size of the input — which on a large
//! enough file is not a slow answer but no answer at all. [`Md5`] therefore
//! keeps the 64-byte block buffer that the algorithm already implies, and
//! [`coreutils::digest`] feeds it in 64 KiB reads.
//!
//! # Why MD5 is here rather than in a crate of its own
//!
//! `sha256sum` delegates to `userspace/sha2` because two things need SHA-256 —
//! the package manager verifies store paths with it, and a checksum utility
//! carrying a second copy of a hash whose whole purpose is that two machines
//! agree is the least defensible duplication in the tree. MD5 has exactly one
//! consumer. It stays local until it has two, at which point it moves out for
//! the same reason SHA-256 did.

use coreutils::digest::{Algorithm, Stream};
use std::process::ExitCode;

/// The `#if HASH_ALGO_MD5` block of upstream's `digest.c`, as data.
static MD5: Algorithm = Algorithm {
    program: "md5sum",
    tag: "MD5",
    bits: 128,
    reference: "RFC 1321",
    new: || Box::new(Md5::new()),
};

fn main() -> ExitCode {
    coreutils::digest::main(&MD5)
}

// ------------------------------------------------------------------ the hash ---

/// Per-round left-rotate amounts.
const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// `floor(2^32 * |sin(i + 1)|)`.
const K: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

/// MD5 (RFC 1321), incremental.
struct Md5 {
    /// `A`, `B`, `C`, `D`.
    state: [u32; 4],
    /// Bytes accepted so far. Wraps at 2^64 bits, which is the format's own
    /// limit — the length field is 64 bits — so wrapping here is the specified
    /// behaviour past 2 EiB rather than a defect.
    len: u64,
    /// The partial block. Only the first `used` bytes are meaningful.
    block: [u8; 64],
    used: usize,
}

impl Md5 {
    const fn new() -> Self {
        Md5 {
            state: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476],
            len: 0,
            block: [0u8; 64],
            used: 0,
        }
    }

    /// One 64-byte round, on a full block.
    fn compress(state: &mut [u32; 4], block: &[u8; 64]) {
        let mut m = [0u32; 16];
        for (word, chunk) in m.iter_mut().zip(block.chunks_exact(4)) {
            // `chunks_exact(4)` yields exactly four bytes, so the conversion
            // cannot fail; `unwrap_or` keeps the function panic-free anyway.
            *word = u32::from_le_bytes(chunk.try_into().unwrap_or([0; 4]));
        }

        let [mut a, mut b, mut c, mut d] = *state;

        for i in 0..64usize {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };

            // `i < 64` and `g < 16` by construction, but the crate denies
            // `indexing_slicing` and this loop is the hot path of a program
            // that runs on untrusted input, so read them fallibly.
            let k = K.get(i).copied().unwrap_or(0);
            let s = S.get(i).copied().unwrap_or(0);
            let mg = m.get(g).copied().unwrap_or(0);

            let f = f.wrapping_add(a).wrapping_add(k).wrapping_add(mg);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(s));
        }

        for (slot, add) in state.iter_mut().zip([a, b, c, d]) {
            *slot = slot.wrapping_add(add);
        }
    }

    /// Absorb `data`, compressing every complete block it completes or contains.
    fn absorb(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);

        // Top up a partial block first: until it is full nothing can be
        // compressed, and once it is, the rest is block-aligned.
        if self.used > 0 {
            let want = 64usize.saturating_sub(self.used);
            let take = want.min(data.len());
            let end = self.used.saturating_add(take);
            if let (Some(dst), Some(src)) = (self.block.get_mut(self.used..end), data.get(..take)) {
                dst.copy_from_slice(src);
            }
            self.used = end;
            data = data.get(take..).unwrap_or(&[]);
            if self.used < 64 {
                // `data` is necessarily empty here — `take` was all of it —
                // so the block is still partial and there is nothing further
                // to do. Returning early is not an optimisation: falling
                // through would reach the tail below, which assigns `used`
                // from the *remainder* and so would throw the partial block
                // away. That is not a hypothetical; it made `squeeze`'s
                // "pad until `used == 56`" loop run forever, which is what
                // `byte_at_a_time_agrees_with_one_shot` caught.
                return;
            }
            let full = self.block;
            Self::compress(&mut self.state, &full);
            self.used = 0;
        }

        // `used == 0` from here on, so the tail may assign it outright.
        let mut it = data.chunks_exact(64);
        for chunk in it.by_ref() {
            let mut full = [0u8; 64];
            full.copy_from_slice(chunk);
            Self::compress(&mut self.state, &full);
        }

        let rest = it.remainder();
        if let Some(dst) = self.block.get_mut(..rest.len()) {
            dst.copy_from_slice(rest);
        }
        self.used = rest.len();
    }

    /// Pad and emit. RFC 1321 §3.1–3.2: a `0x80`, zeroes up to 56 mod 64, then
    /// the message length in bits as a little-endian `u64`.
    fn squeeze(mut self) -> [u8; 16] {
        // Read the length *before* padding, since padding goes through
        // `absorb` and so counts itself.
        let bits = self.len.wrapping_mul(8);
        self.absorb(&[0x80]);
        while self.used != 56 {
            self.absorb(&[0]);
        }
        self.absorb(&bits.to_le_bytes());

        let mut out = [0u8; 16];
        for (dst, word) in out.chunks_exact_mut(4).zip(self.state) {
            dst.copy_from_slice(&word.to_le_bytes());
        }
        out
    }
}

impl Stream for Md5 {
    fn update(&mut self, data: &[u8]) {
        self.absorb(data);
    }

    fn finish(self: Box<Self>) -> Vec<u8> {
        (*self).squeeze().to_vec()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn hex(digest: &[u8]) -> String {
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn md5_hex(data: &[u8]) -> String {
        let mut h = Md5::new();
        h.update(data);
        hex(&Box::new(h).finish())
    }

    // ------------- the constant the shared module cannot check for itself -------------

    #[test]
    fn digest_length_matches_the_declared_bits() {
        // `Algorithm::bits` drives `hex_len`, which decides which check lines
        // parse at all. Upstream gets this consistency from the preprocessor;
        // here they are two independent statements and so need asserting.
        assert_eq!((MD5.new)().finish().len() * 8, MD5.bits);
        assert_eq!(MD5.hex_len(), 32);
    }

    // ---------------- RFC 1321 test vectors ----------------

    #[test]
    fn md5_empty() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn md5_a() {
        assert_eq!(md5_hex(b"a"), "0cc175b9c0f1b6a831c399e269772661");
    }

    #[test]
    fn md5_abc() {
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn md5_message_digest() {
        assert_eq!(
            md5_hex(b"message digest"),
            "f96b697d7cb7938d525a2f31aaf161d0"
        );
    }

    #[test]
    fn md5_alphabet() {
        assert_eq!(
            md5_hex(b"abcdefghijklmnopqrstuvwxyz"),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
    }

    #[test]
    fn md5_alphanumeric() {
        assert_eq!(
            md5_hex(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"),
            "d174ab98d277d9f5a5611c2c9f419d9f"
        );
    }

    #[test]
    fn md5_eight_digit_groups() {
        // Crosses multiple 64-byte blocks.
        assert_eq!(
            md5_hex(
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            ),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    // ---------------- padding boundaries ----------------

    #[test]
    fn md5_exactly_55_bytes() {
        // The last length at which padding still fits in the same block.
        assert_eq!(md5_hex(&[b'a'; 55]), "ef1772b6dff9a122358552954ad0df65");
    }

    #[test]
    fn md5_exactly_56_bytes() {
        // One over: padding must spill into a second block.
        assert_eq!(md5_hex(&[b'a'; 56]), "3b0c8ac703f828b04c6c197006d17218");
    }

    #[test]
    fn md5_exactly_64_bytes() {
        // One full block; padding still needs a second.
        assert_eq!(md5_hex(&[b'a'; 64]), "014842d480b571495a4a0363793f7367");
    }

    #[test]
    fn md5_high_bit_bytes() {
        // Byte-oriented, not text: 0xff is data.
        assert_eq!(md5_hex(&[0xffu8; 16]), "8d79cbc9a4ecdde112fc91ba625b13c2");
    }

    #[test]
    fn md5_single_zero_byte() {
        assert_eq!(md5_hex(&[0u8]), "93b885adfe0da089cdf634904fd59f71");
    }

    // ---------------- streaming ----------------

    /// The property the whole rewrite rests on: how the message is *divided*
    /// across `update` calls must not change the answer. A one-shot hash
    /// cannot get this wrong; an incremental one gets it wrong at exactly the
    /// splits that straddle a 64-byte block, so every split is tried.
    #[test]
    fn every_split_of_a_multi_block_message_agrees() {
        let msg: Vec<u8> = (0u16..200).map(|i| (i % 251) as u8).collect();
        let want = md5_hex(&msg);
        for cut in 0..=msg.len() {
            let mut h = Md5::new();
            h.update(&msg[..cut]);
            h.update(&msg[cut..]);
            assert_eq!(hex(&Box::new(h).finish()), want, "split at {cut} disagreed");
        }
    }

    /// One byte at a time is the pathological case for the top-up path:
    /// `used` walks every value from 0 to 63 and back.
    #[test]
    fn byte_at_a_time_agrees_with_one_shot() {
        let msg = b"The quick brown fox jumps over the lazy dog, twice, and then some more.";
        let mut h = Md5::new();
        for b in msg {
            h.update(&[*b]);
        }
        assert_eq!(hex(&Box::new(h).finish()), md5_hex(msg));
    }

    /// An empty `update` must be a no-op rather than anything at all — the
    /// shared module can issue one for a zero-length read.
    #[test]
    fn empty_updates_are_ignored() {
        let mut h = Md5::new();
        h.update(b"");
        h.update(b"abc");
        h.update(b"");
        assert_eq!(
            hex(&Box::new(h).finish()),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    /// A message far larger than one block, fed in chunks that are neither
    /// block-aligned nor block-sized, to exercise the bulk path and the
    /// top-up path alternately.
    #[test]
    fn a_large_message_hashes_the_same_however_it_is_chunked() {
        let msg = vec![b'z'; 100_000];
        let want = md5_hex(&msg);
        let mut h = Md5::new();
        for part in msg.chunks(4093) {
            h.update(part);
        }
        assert_eq!(hex(&Box::new(h).finish()), want);
    }
}
