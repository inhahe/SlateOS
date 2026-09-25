//! SHA-512 and SHA-384 (FIPS 180-4 §6.4 and §6.5).
//!
//! # Provenance
//!
//! The compression function, the eighty round constants and the two initial
//! hash values are **ported, not written**: they come from RustCrypto's `sha2`
//! crate, version 0.11.0 (git `ffe093984c004769747e998f77da8ff7c0e7a765`),
//! files `sha2/src/sha512/soft/compact.rs` and `sha2/src/consts.rs`, under
//! `MIT OR Apache-2.0`. The `.crate` it was read from has SHA-256
//! `446ba717509524cb3f22f17ecc096f10f4822d76ab5c0b9822c5f9c284e825f4`.
//!
//! That is `design-decisions.md` §539: a cryptographic primitive comes from an
//! implementation other people have spent years attacking, and only the
//! plumbing around it is this tree's own. Here the plumbing is the buffering and
//! the padding, which are `blockbuf`'s — the one copy of that logic MD5, SHA-1
//! and SM3 already share — and the output, a fixed array like SHA-256's.
//!
//! The port changes the compression function's *spelling* in two places and
//! its arithmetic in none. The message schedule reads its sixteen-word window
//! through `last_chunk`, as [`crate::Sha256`]'s does, rather than as
//! `w[i - 15]`, so every index is checked at compile time; and the round loop
//! zips the constants with the schedule instead of counting. Every operation,
//! rotation amount and constant is upstream's, and the FIPS 180-4 vectors below
//! are what say so.
//!
//! # Why SHA-384 is here and not a separate algorithm
//!
//! Because it is not one. SHA-384 is SHA-512 started from different initial
//! values and cut to 48 bytes (FIPS 180-4 §5.3.4, §6.5), exactly as SHA-224 is
//! SHA-256 cut to 28. Sharing the engine is what the standard says, not an
//! economy.

use blockbuf::BlockBuffer;
use core::fmt;

/// The block size SHA-512 compresses, in bytes.
const BLOCK_LEN: usize = 128;

/// Bytes of SHA-512 output.
pub const SHA512_DIGEST_LEN: usize = 64;

/// Bytes of SHA-384 output: the first 48 of the 64 the engine produces.
pub const SHA384_DIGEST_LEN: usize = 48;

/// SHA-384's initial hash value: the first 64 bits of the fractional parts of
/// the square roots of the ninth through sixteenth primes (FIPS 180-4 §5.3.4).
const INIT_384: [u64; 8] = [
    0xcbbb_9d5d_c105_9ed8,
    0x629a_292a_367c_d507,
    0x9159_015a_3070_dd17,
    0x152f_ecd8_f70e_5939,
    0x6733_2667_ffc0_0b31,
    0x8eb4_4a87_6858_1511,
    0xdb0c_2e0d_64f9_8fa7,
    0x47b5_481d_befa_4fa4,
];

/// SHA-512's initial hash value: the first 64 bits of the fractional parts of
/// the square roots of the first eight primes (FIPS 180-4 §5.3.5).
const INIT_512: [u64; 8] = [
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
];

/// Round constants: the first 64 bits of the fractional parts of the cube
/// roots of the first eighty primes (FIPS 180-4 §4.2.3).
const K: [u64; 80] = [
    0x428a_2f98_d728_ae22,
    0x7137_4491_23ef_65cd,
    0xb5c0_fbcf_ec4d_3b2f,
    0xe9b5_dba5_8189_dbbc,
    0x3956_c25b_f348_b538,
    0x59f1_11f1_b605_d019,
    0x923f_82a4_af19_4f9b,
    0xab1c_5ed5_da6d_8118,
    0xd807_aa98_a303_0242,
    0x1283_5b01_4570_6fbe,
    0x2431_85be_4ee4_b28c,
    0x550c_7dc3_d5ff_b4e2,
    0x72be_5d74_f27b_896f,
    0x80de_b1fe_3b16_96b1,
    0x9bdc_06a7_25c7_1235,
    0xc19b_f174_cf69_2694,
    0xe49b_69c1_9ef1_4ad2,
    0xefbe_4786_384f_25e3,
    0x0fc1_9dc6_8b8c_d5b5,
    0x240c_a1cc_77ac_9c65,
    0x2de9_2c6f_592b_0275,
    0x4a74_84aa_6ea6_e483,
    0x5cb0_a9dc_bd41_fbd4,
    0x76f9_88da_8311_53b5,
    0x983e_5152_ee66_dfab,
    0xa831_c66d_2db4_3210,
    0xb003_27c8_98fb_213f,
    0xbf59_7fc7_beef_0ee4,
    0xc6e0_0bf3_3da8_8fc2,
    0xd5a7_9147_930a_a725,
    0x06ca_6351_e003_826f,
    0x1429_2967_0a0e_6e70,
    0x27b7_0a85_46d2_2ffc,
    0x2e1b_2138_5c26_c926,
    0x4d2c_6dfc_5ac4_2aed,
    0x5338_0d13_9d95_b3df,
    0x650a_7354_8baf_63de,
    0x766a_0abb_3c77_b2a8,
    0x81c2_c92e_47ed_aee6,
    0x9272_2c85_1482_353b,
    0xa2bf_e8a1_4cf1_0364,
    0xa81a_664b_bc42_3001,
    0xc24b_8b70_d0f8_9791,
    0xc76c_51a3_0654_be30,
    0xd192_e819_d6ef_5218,
    0xd699_0624_5565_a910,
    0xf40e_3585_5771_202a,
    0x106a_a070_32bb_d1b8,
    0x19a4_c116_b8d2_d0c8,
    0x1e37_6c08_5141_ab53,
    0x2748_774c_df8e_eb99,
    0x34b0_bcb5_e19b_48a8,
    0x391c_0cb3_c5c9_5a63,
    0x4ed8_aa4a_e341_8acb,
    0x5b9c_ca4f_7763_e373,
    0x682e_6ff3_d6b2_b8a3,
    0x748f_82ee_5def_b2fc,
    0x78a5_636f_4317_2f60,
    0x84c8_7814_a1f0_ab72,
    0x8cc7_0208_1a64_39ec,
    0x90be_fffa_2363_1e28,
    0xa450_6ceb_de82_bde9,
    0xbef9_a3f7_b2c6_7915,
    0xc671_78f2_e372_532b,
    0xca27_3ece_ea26_619c,
    0xd186_b8c7_21c0_c207,
    0xeada_7dd6_cde0_eb1e,
    0xf57d_4f7f_ee6e_d178,
    0x06f0_67aa_7217_6fba,
    0x0a63_7dc5_a2c8_98a6,
    0x113f_9804_bef9_0dae,
    0x1b71_0b35_131c_471b,
    0x28db_77f5_2304_7d84,
    0x32ca_ab7b_40c7_2493,
    0x3c9e_be0a_15c9_bebc,
    0x431d_67c4_9c10_0d4c,
    0x4cc5_d4be_cb3e_42b6,
    0x597f_299c_fc65_7e2a,
    0x5fcb_6fab_3ad6_faec,
    0x6c44_198c_4a47_5817,
];

/// Hash `data` with SHA-512 in one call.
#[must_use]
pub fn sha512(data: &[u8]) -> [u8; SHA512_DIGEST_LEN] {
    let mut hasher = Sha512::new();
    hasher.update(data);
    hasher.finalize()
}

/// Hash `data` with SHA-384 in one call.
#[must_use]
pub fn sha384(data: &[u8]) -> [u8; SHA384_DIGEST_LEN] {
    let mut hasher = Sha384::new();
    hasher.update(data);
    hasher.finalize()
}

/// The state both hashes run: eight 64-bit words and one partial block.
#[derive(Clone)]
struct Engine {
    /// Chaining value: the hash of everything compressed so far.
    state: [u64; 8],
    /// The partial block, the byte count and the padding rule.
    buffer: BlockBuffer<BLOCK_LEN>,
}

impl Engine {
    fn new(init: [u64; 8]) -> Self {
        Self {
            state: init,
            buffer: BlockBuffer::new(),
        }
    }

    fn update(&mut self, data: &[u8]) {
        let state = &mut self.state;
        self.buffer.update(data, |block| compress(state, block));
    }

    /// Pad, compress what is left, and write the state out big-endian. The
    /// caller keeps as many leading bytes as its digest is wide.
    fn finalize(mut self) -> [u8; SHA512_DIGEST_LEN] {
        let state = &mut self.state;
        self.buffer.finalize_wide(|block| compress(state, block));

        let mut out = [0u8; SHA512_DIGEST_LEN];
        for (slot, word) in out.as_chunks_mut::<8>().0.iter_mut().zip(self.state) {
            *slot = word.to_be_bytes();
        }
        out
    }

    fn bytes_hashed(&self) -> u64 {
        self.buffer.bytes_absorbed()
    }
}

/// An incremental SHA-512 hasher.
///
/// Holds one partial block — 128 bytes — however much has been hashed, so a
/// file of any size can be digested from a fixed-size read buffer.
#[derive(Clone)]
pub struct Sha512(Engine);

/// An incremental SHA-384 hasher: [`Sha512`] from other initial values,
/// truncated.
#[derive(Clone)]
pub struct Sha384(Engine);

impl Default for Sha512 {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for Sha384 {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Sha512 {
    /// Deliberately opaque, as [`crate::Sha256`]'s is: the buffer can hold a
    /// password or a key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sha512")
            .field("bytes_hashed", &self.0.bytes_hashed())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Sha384 {
    /// Deliberately opaque; see [`Sha512`]'s.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sha384")
            .field("bytes_hashed", &self.0.bytes_hashed())
            .finish_non_exhaustive()
    }
}

impl Sha512 {
    /// Start a new hash.
    #[must_use]
    pub fn new() -> Self {
        Self(Engine::new(INIT_512))
    }

    /// Feed `data` in. Splitting the same input differently across calls gives
    /// the same digest.
    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    /// Finish, returning the 64-byte digest. Consumes the hasher, because a
    /// padded state cannot be extended.
    #[must_use]
    pub fn finalize(self) -> [u8; SHA512_DIGEST_LEN] {
        self.0.finalize()
    }
}

impl Sha384 {
    /// Start a new hash.
    #[must_use]
    pub fn new() -> Self {
        Self(Engine::new(INIT_384))
    }

    /// Feed `data` in. Splitting the same input differently across calls gives
    /// the same digest.
    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    /// Finish, returning the 48-byte digest: the leading six words of the
    /// final state (FIPS 180-4 §6.5).
    #[must_use]
    pub fn finalize(self) -> [u8; SHA384_DIGEST_LEN] {
        let full = self.0.finalize();
        let mut out = [0u8; SHA384_DIGEST_LEN];
        if let Some(head) = full.first_chunk::<SHA384_DIGEST_LEN>() {
            out = *head;
        }
        out
    }
}

/// One application of the SHA-512 compression function to a 128-byte block.
///
/// Upstream's `compress_u64` (RustCrypto `sha2` 0.11.0,
/// `src/sha512/soft/compact.rs`), with its `to_u64s` folded in.
fn compress(state: &mut [u64; 8], block: &[u8; BLOCK_LEN]) {
    // The first sixteen schedule words are the block, big-endian.
    let mut w = [0u64; 80];
    for (word, bytes) in w.iter_mut().zip(block.as_chunks::<8>().0) {
        *word = u64::from_be_bytes(*bytes);
    }

    // The other sixty-four are upstream's recurrence. `prev` is the sixteen
    // words before `i`: prev[0] is w[i-16], prev[1] w[i-15], prev[9] w[i-7] and
    // prev[14] w[i-2].
    for i in 16..80 {
        let Some(prev) = w.get(..i).and_then(<[u64]>::last_chunk::<16>) else {
            // Unreachable: `16 <= i < 80 == w.len()`.
            continue;
        };
        let w15 = prev[1];
        let s0 = (w15.rotate_right(1)) ^ (w15.rotate_right(8)) ^ (w15 >> 7);
        let w2 = prev[14];
        let s1 = (w2.rotate_right(19)) ^ (w2.rotate_right(61)) ^ (w2 >> 6);
        let next = prev[0]
            .wrapping_add(s0)
            .wrapping_add(prev[9])
            .wrapping_add(s1);
        if let Some(slot) = w.get_mut(i) {
            *slot = next;
        }
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;

    for (k, word) in K.iter().zip(w) {
        let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = s1
            .wrapping_add(ch)
            .wrapping_add(*k)
            .wrapping_add(word)
            .wrapping_add(h);
        let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    let round = [a, b, c, d, e, f, g, h];
    for (acc, delta) in state.iter_mut().zip(round) {
        *acc = acc.wrapping_add(delta);
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

    const TWO_BLOCK_112: &[u8] = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmn\
hijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";

    // -- FIPS 180-4 example vectors (NIST CSRC "Examples with Intermediate
    // Values"), and the one-million-`a` vector from the older FIPS 180-2 --

    #[test]
    fn sha512_empty() {
        assert_eq!(
            hex(&sha512(b"")),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
             47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
    }

    #[test]
    fn sha512_abc() {
        assert_eq!(
            hex(&sha512(b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn sha512_two_block_message() {
        // 112 bytes: the 0x80 fits but the 16-byte length does not, so this is
        // the vector that catches a broken second block.
        assert_eq!(
            hex(&sha512(TWO_BLOCK_112)),
            "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018\
             501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
        );
    }

    #[test]
    fn sha512_one_million_a() {
        let mut hasher = Sha512::new();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            hasher.update(&chunk);
        }
        assert_eq!(
            hex(&hasher.finalize()),
            "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973eb\
             de0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
        );
    }

    #[test]
    fn sha384_empty() {
        assert_eq!(
            hex(&sha384(b"")),
            "38b060a751ac96384cd9327eb1b1e36a21fdb71114be0743\
             4c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b"
        );
    }

    #[test]
    fn sha384_abc() {
        assert_eq!(
            hex(&sha384(b"abc")),
            "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded163\
             1a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
        );
    }

    #[test]
    fn sha384_two_block_message() {
        assert_eq!(
            hex(&sha384(TWO_BLOCK_112)),
            "09330c33f71147e83d192fc782cd1b4753111b173b3b05d2\
             2fa08086e3b0f712fcc7c71a557e2db966c3e9fa91746039"
        );
    }

    #[test]
    fn sha384_one_million_a() {
        let mut hasher = Sha384::new();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            hasher.update(&chunk);
        }
        assert_eq!(
            hex(&hasher.finalize()),
            "9d0e1809716474cb086e834e310a4a1ced149e9c00f24852\
             7972cec5704c2a5b07b8b3dc38ecc4ebae97ddd87f3d8985"
        );
    }

    // -- The padding boundaries of a 128-byte block with a 16-byte length --
    // Cross-checked against Python's `hashlib`, an independent implementation.

    #[test]
    fn sha512_at_the_padding_boundaries() {
        for (len, want) in [
            (
                111,
                "9a2a120825c2319867758ec277924f6faa254968bf752046dacdd948d8ad299b\
                 10359fd04bfd7d3810b5fa1b16a294236138baff981cbb85248478053ac4d3dd",
            ),
            (
                112,
                "a3722b515ef40c910f2419f6e0da8ca51d410114ce6272faae64045f9e9f630e\
                 7fa8dd5a3243c9860b899d148c3da4bc0f9e07454542604d030bb55531fe0d5b",
            ),
            (
                128,
                "e2e22f8422b54b06e35c3ea30a383d1de7a8fbc27992923074103117020d8dd7\
                 024c3ecf7d6d1a15a6de5a75ff32fb486b9e8ced4c02ffe05822bf2cb734d0e0",
            ),
        ] {
            assert_eq!(hex(&sha512(&[b'x'; 128][..len])), want, "{len} bytes");
        }
    }

    #[test]
    fn sha384_at_the_padding_boundaries() {
        for (len, want) in [
            (
                111,
                "dfec6588f894c2b089119e1884a23941e5aa69ed38702839\
                 cf7352ac6d155d315693bb5d26b7468d8b69ecf4631ef419",
            ),
            (
                112,
                "d8489693bc428374931aedf508740398d9d4a92887116fec\
                 b2fcdd91c68b9db329fc4a474e05e78c3eb34e649a6b5c77",
            ),
            (
                128,
                "e660584956c8b1df44c92acb7c8eccfe0dca5255627c9fb4\
                 4637c15363b772e5709edcf35b07bf43531951ab2fd51130",
            ),
        ] {
            assert_eq!(hex(&sha384(&[b'x'; 128][..len])), want, "{len} bytes");
        }
    }

    // -- Streaming --

    #[test]
    fn every_split_of_a_three_block_message_agrees_with_one_shot() {
        let data: Vec<u8> = (0..300_u32).map(|i| (i % 251) as u8).collect();
        let one_shot = sha512(&data);
        for split in 0..=data.len() {
            let mut hasher = Sha512::new();
            hasher.update(&data[..split]);
            hasher.update(&data[split..]);
            assert_eq!(hasher.finalize(), one_shot, "split at {split}");
        }
    }

    #[test]
    fn sha384_is_not_a_prefix_of_sha512() {
        // Different initial values, so the truncation is of a different hash:
        // an implementation that reused SHA-512's IV would fail here and pass
        // every length and padding test.
        assert_ne!(sha384(b"abc")[..], sha512(b"abc")[..SHA384_DIGEST_LEN]);
    }

    #[test]
    fn debug_does_not_print_the_buffer() {
        let mut hasher = Sha512::new();
        hasher.update(b"hunter2");
        let rendered = std::format!("{hasher:?}");
        assert!(!rendered.contains("hunter2") && !rendered.contains("104"));
        assert!(
            rendered.contains('7'),
            "expected the byte count: {rendered}"
        );
    }
}
