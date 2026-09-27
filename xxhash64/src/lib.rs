//! XXH64, the 64-bit variant of Yann Collet's xxHash.
//!
//! A non-cryptographic hash, here because filesystems checksum their
//! superblocks with it: bcachefs (checksum type 7) and btrfs
//! (`BTRFS_CSUM_TYPE_XXHASH`), both of which libblkid's probers verify. Only
//! the one-shot form is provided; nothing in the tree hashes a stream.
//!
//! A transcription of the reference implementation's scalar path
//! (`XXH64_endian_align`, `XXH64_finalize`, `XXH64_avalanche` in `xxhash.h`,
//! the copy util-linux 2.39.3 bundles), checked against that implementation's
//! own outputs. All arithmetic is modulo 2^64 and every read little-endian,
//! as the specification requires.
//!
//! # References
//!
//! - <https://github.com/Cyan4973/xxHash/blob/dev/doc/xxhash_spec.md>, "XXH64"
//! - util-linux 2.39.3 `include/xxhash.h`

#![no_std]

const PRIME64_1: u64 = 0x9E37_79B1_85EB_CA87;
const PRIME64_2: u64 = 0xC2B2_AE3D_27D4_EB4F;
const PRIME64_3: u64 = 0x1656_67B1_9E37_79F9;
const PRIME64_4: u64 = 0x85EB_CA77_C2B2_AE63;
const PRIME64_5: u64 = 0x27D4_EB2F_1656_67C5;

/// `XXH64_round`.
fn round(acc: u64, input: u64) -> u64 {
    acc.wrapping_add(input.wrapping_mul(PRIME64_2))
        .rotate_left(31)
        .wrapping_mul(PRIME64_1)
}

/// `XXH64_mergeRound`.
fn merge_round(acc: u64, val: u64) -> u64 {
    (acc ^ round(0, val))
        .wrapping_mul(PRIME64_1)
        .wrapping_add(PRIME64_4)
}

/// `XXH64_avalanche`.
fn avalanche(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(PRIME64_2);
    h ^= h >> 29;
    h = h.wrapping_mul(PRIME64_3);
    h ^ (h >> 32)
}

/// A little-endian `u64` from the first eight bytes of `b` (which the callers
/// guarantee are there).
fn le64(b: &[u8]) -> u64 {
    let mut w = [0u8; 8];
    for (d, s) in w.iter_mut().zip(b) {
        *d = *s;
    }
    u64::from_le_bytes(w)
}

/// A little-endian `u32` from the first four bytes of `b`.
fn le32(b: &[u8]) -> u32 {
    let mut w = [0u8; 4];
    for (d, s) in w.iter_mut().zip(b) {
        *d = *s;
    }
    u32::from_le_bytes(w)
}

/// XXH64 of `data` with `seed`.
///
/// ```
/// assert_eq!(xxhash64::xxh64(b"", 0), 0xEF46_DB37_51D8_E999);
/// ```
#[must_use]
pub fn xxh64(data: &[u8], seed: u64) -> u64 {
    let mut stripes = data.chunks_exact(32);
    let mut h = if data.len() >= 32 {
        let mut v1 = seed.wrapping_add(PRIME64_1).wrapping_add(PRIME64_2);
        let mut v2 = seed.wrapping_add(PRIME64_2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(PRIME64_1);
        for stripe in stripes.by_ref() {
            let (a, rest) = stripe.split_at(8);
            let (b, rest) = rest.split_at(8);
            let (c, d) = rest.split_at(8);
            v1 = round(v1, le64(a));
            v2 = round(v2, le64(b));
            v3 = round(v3, le64(c));
            v4 = round(v4, le64(d));
        }
        let mut h = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
        h = merge_round(h, v1);
        h = merge_round(h, v2);
        h = merge_round(h, v3);
        merge_round(h, v4)
    } else {
        seed.wrapping_add(PRIME64_5)
    };
    // `usize` is at most 64 bits on every target this builds for.
    h = h.wrapping_add(data.len() as u64);

    // `XXH64_finalize`: what the stripes did not consume.
    let mut tail = stripes.remainder();
    while tail.len() >= 8 {
        let (word, rest) = tail.split_at(8);
        h ^= round(0, le64(word));
        h = h.rotate_left(27).wrapping_mul(PRIME64_1).wrapping_add(PRIME64_4);
        tail = rest;
    }
    if tail.len() >= 4 {
        let (word, rest) = tail.split_at(4);
        h ^= u64::from(le32(word)).wrapping_mul(PRIME64_1);
        h = h.rotate_left(23).wrapping_mul(PRIME64_2).wrapping_add(PRIME64_3);
        tail = rest;
    }
    for &byte in tail {
        h ^= u64::from(byte).wrapping_mul(PRIME64_5);
        h = h.rotate_left(11).wrapping_mul(PRIME64_1);
    }
    avalanche(h)
}

#[cfg(test)]
mod tests {
    use super::xxh64;

    /// Outputs of util-linux 2.39.3's bundled `xxhash.h`, compiled and run on
    /// Linux, for inputs that reach every path: empty, the byte loop, the
    /// four-byte step, the eight-byte loop, and the 32-byte stripes with a
    /// tail of each size.
    #[test]
    fn reference_outputs() {
        let cases: [(&[u8], u64, u64); 6] = [
            (b"", 0xef46_db37_51d8_e999, 0xd5af_ba13_36a3_be4b),
            (b"a", 0xd24e_c4f1_a98c_6e5b, 0xdec2_bc81_c3cd_46c6),
            (b"abc", 0x44bc_2cf5_ad77_0999, 0xbea9_ca81_9932_8908),
            (b"123456789", 0x8cb8_41db_40e6_ae83, 0x1a4c_c2c9_e807_9790),
            (
                b"The quick brown fox jumps over the lazy dog",
                0x0b24_2d36_1fda_71bc,
                0xdf50_91b6_dad2_c6db,
            ),
            (
                b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef-tail",
                0xac0f_0e2e_f70e_9398,
                0x7c3a_bfaf_fa12_303f,
            ),
        ];
        for (input, seed0, seed1) in cases {
            assert_eq!(xxh64(input, 0), seed0, "{input:?} seed 0");
            assert_eq!(xxh64(input, 1), seed1, "{input:?} seed 1");
        }
    }
}
