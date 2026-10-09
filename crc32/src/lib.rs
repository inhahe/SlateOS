//! CRC-32 (ISO 3309 / IEEE 802.3) — the "classic" reflected-IEEE polynomial.
//!
//! This is the checksum of gzip, ZIP, PNG, Ethernet, RAR, 7-Zip and F2FS. It
//! is **not** CRC32C (Castagnoli, `0x82F63B78`), which ext4 uses and which the
//! kernel keeps separately in `crypto.rs`. The two are easy to confuse and
//! impossible to confuse safely: a checksum computed with the wrong polynomial
//! verifies against nothing, and the failure looks like data corruption rather
//! than like a bug.
//!
//! # Why this is its own crate
//!
//! It was a `const` table and three functions inside the kernel's `crypto`
//! module, which meant every non-kernel caller had to write its own. The
//! comment on the kernel's copy already recorded that this had happened four
//! times over (`rar.rs`, `sevenz.rs`, `properties.rs` and `compress.rs` each
//! grew a private bit-at-a-time loop, none table-driven and none covered by a
//! check-value test) and consolidated them onto one implementation — but a
//! module of a *binary* crate cannot be depended on, so the consolidation
//! stopped at the kernel's edge. `gui/imagecodec` needs this polynomial for
//! PNG chunk CRCs and the `deflate` crate needs it for the gzip trailer;
//! neither can name `crate::crypto`. Hence a leaf crate, on the model of
//! `sha2`, `sha1` and `md5`.
//!
//! # References
//!
//! - ISO 3309 / ITU-T V.42 / IEEE 802.3, reflected polynomial `0xEDB88320`
//! - RFC 1952 §8 (gzip's use of it)

#![no_std]

/// CRC-32 lookup table for the reflected IEEE polynomial `0xEDB88320`
/// (bit-reversed `0x04C11DB7`).
///
/// Built at compile time, so there is no initialisation order to get wrong and
/// no runtime cost; the `while` loops rather than iterators are because `const`
/// evaluation does not admit `for`.
const CRC32_TABLE: [u32; 256] = {
    const POLY: u32 = 0xEDB8_8320;
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut bit = 0;
        while bit < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ POLY;
            } else {
                crc >>= 1;
            }
            bit += 1;
        }
        // A `const` block cannot use `get_mut`, and `i` is bounded by the
        // `while` above, so the index is provably in range.
        #[allow(clippy::indexing_slicing)]
        {
            table[i as usize] = crc;
        }
        i += 1;
    }
    table
};

/// `TABLES[k][b]`: the CRC of byte `b` followed by `k` zero bytes, so that
/// eight input bytes are folded in by eight independent lookups rather than
/// eight that each wait for the last ("slicing by 8"). `TABLES[0]` is
/// [`CRC32_TABLE`]. 8 KiB, built at compile time.
const TABLES: [[u32; 256]; 8] = {
    let mut t = [[0u32; 256]; 8];
    t[0] = CRC32_TABLE;
    let mut k = 1;
    while k < 8 {
        let mut i = 0;
        while i < 256 {
            // `k` and `i` are bounded by the `while`s, so every index is in
            // range; a `const` block cannot use `get`.
            #[allow(clippy::indexing_slicing)]
            {
                let prev = t[k - 1][i];
                t[k][i] = (prev >> 8) ^ CRC32_TABLE[(prev & 0xFF) as usize];
            }
            i += 1;
        }
        k += 1;
    }
    t
};

/// Compute CRC-32 (ISO 3309 / IEEE 802.3) over a byte slice.
///
/// Initial value `!0`, final value inverted — the conventional framing, so this
/// matches what `gzip -l`, `unzip -v` and Python's `zlib.crc32` report.
///
/// # Examples
///
/// ```
/// assert_eq!(crc32::crc32(b"123456789"), 0xCBF4_3926);
/// ```
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    crc32_seed(!0u32, data)
}

/// Compute CRC-32 with a custom initial seed, inverting the result.
///
/// Chains with [`crc32_raw`]: feed a raw accumulator in, get a finished CRC
/// out, so a checksum over a discontiguous byte range can be computed in pieces
/// without materialising the concatenation.
///
/// # Examples
///
/// ```
/// let raw = crc32::crc32_raw(!0u32, b"1234");
/// assert_eq!(crc32::crc32_seed(raw, b"56789"), crc32::crc32(b"123456789"));
/// ```
#[must_use]
pub fn crc32_seed(seed: u32, data: &[u8]) -> u32 {
    crc32_raw(seed, data) ^ !0u32
}

/// Compute CRC-32 without the final inversion.
///
/// Returns the bare accumulator. Two callers need this rather than [`crc32`]:
/// anyone chaining a CRC across separate slices, and F2FS — whose metadata
/// checksums are Linux's `crc32_le()` seeded with the F2FS magic and with
/// *neither* inversion applied, so the conventional framing would produce a
/// value that is wrong by exactly `!0` on both ends.
///
/// Eight bytes a step through [`TABLES`], then the rest a byte at a time:
/// about five times faster than a byte at a time over a long input (lane F's
/// measurement over a PNG's pixel data, 7.2 cycles a byte before;
/// `benches/rate.rs` has this machine's figure), with the same result for
/// every input and every split -- the tests hold it to the byte-at-a-time
/// loop.
#[must_use]
pub fn crc32_raw(seed: u32, data: &[u8]) -> u32 {
    let mut crc = seed;
    let (words, rest) = data.as_chunks::<8>();
    for word in words {
        let [b0, b1, b2, b3, b4, b5, b6, b7] = *word;
        let lo = u32::from_le_bytes([b0, b1, b2, b3]) ^ crc;
        let hi = u32::from_le_bytes([b4, b5, b6, b7]);
        // Every index is masked to 8 bits (or is a top byte, `>> 24`), so it
        // is always in range; `get` would force a `Result` on a function that
        // cannot fail.
        #[allow(clippy::indexing_slicing)]
        {
            crc = TABLES[7][(lo & 0xFF) as usize]
                ^ TABLES[6][((lo >> 8) & 0xFF) as usize]
                ^ TABLES[5][((lo >> 16) & 0xFF) as usize]
                ^ TABLES[4][(lo >> 24) as usize]
                ^ TABLES[3][(hi & 0xFF) as usize]
                ^ TABLES[2][((hi >> 8) & 0xFF) as usize]
                ^ TABLES[1][((hi >> 16) & 0xFF) as usize]
                ^ TABLES[0][(hi >> 24) as usize];
        }
    }
    bytewise(crc, rest)
}

/// The byte-at-a-time loop: [`crc32_raw`]'s tail, and the tests' oracle for
/// its eight-byte steps.
fn bytewise(seed: u32, data: &[u8]) -> u32 {
    let mut crc = seed;
    for &byte in data {
        let idx = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        // The index is masked to 8 bits, so it is always in range; `get` here
        // would force a `Result` on a function that cannot fail.
        #[allow(clippy::indexing_slicing)]
        {
            crc = CRC32_TABLE[idx] ^ (crc >> 8);
        }
    }
    crc
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::{bytewise, crc32, crc32_raw, crc32_seed};

    /// A deterministic, non-repeating test buffer (xorshift), so a table entry
    /// wrong for one byte value cannot hide behind a buffer that never holds
    /// it.
    fn noise(len: usize) -> [u8; 4096] {
        assert!(len <= 4096);
        let mut out = [0u8; 4096];
        let mut x = 0x9E37_79B9_u32;
        for b in out.iter_mut().take(len) {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = x.to_le_bytes()[0];
        }
        out
    }

    /// Eight bytes a step and one a step agree for every length that takes
    /// the tail alone (0-7), a whole word (8), and a word and a tail (9-17),
    /// from two seeds.
    #[test]
    fn eight_at_a_time_matches_one_at_a_time_for_short_lengths() {
        let buf = noise(17);
        for len in 0..=17 {
            for seed in [!0u32, 0x1234_5678] {
                assert_eq!(
                    crc32_raw(seed, &buf[..len]),
                    bytewise(seed, &buf[..len]),
                    "length {len}, seed {seed:#x}"
                );
            }
        }
    }

    /// And over every byte value, at every alignment of the eight-byte steps:
    /// 4096 bytes of noise, each of its first sixteen offsets.
    #[test]
    fn eight_at_a_time_matches_one_at_a_time_over_noise() {
        let buf = noise(4096);
        for start in 0..16 {
            assert_eq!(
                crc32_raw(!0, &buf[start..]),
                bytewise(!0, &buf[start..]),
                "offset {start}"
            );
        }
    }

    /// Chaining across every split point of a buffer longer than a few words
    /// equals one call over the whole: the steps carry no state but `crc`.
    #[test]
    fn chaining_matches_one_shot_at_every_split() {
        let buf = noise(100);
        let whole = crc32(&buf[..100]);
        for split in 0..=100 {
            let (a, b) = buf[..100].split_at(split);
            assert_eq!(crc32_seed(crc32_raw(!0, a), b), whole, "split {split}");
        }
    }

    /// The standard check value. Every reflected-IEEE implementation agrees on
    /// it, which is what makes it the right vector to guard the table against a
    /// transcription error in the polynomial.
    #[test]
    fn check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    /// The empty input must be 0, not `!0`: it proves both inversions are
    /// applied and that they are applied in the right order.
    #[test]
    fn empty_is_zero() {
        assert_eq!(crc32(b""), 0);
    }

    /// Chaining across a split must equal the one-shot value, which is the
    /// property F2FS's checkpoint checksum depends on — it covers the block in
    /// two pieces, skipping the four bytes that hold the checksum itself.
    #[test]
    fn chaining_matches_one_shot() {
        assert_eq!(crc32_seed(crc32_raw(!0u32, b"1234"), b"56789"), 0xCBF4_3926);
        for split in 0..=9 {
            let (a, b) = b"123456789".split_at(split);
            assert_eq!(crc32_seed(crc32_raw(!0u32, a), b), 0xCBF4_3926);
        }
    }

    /// A vector from a different source than the check value, so a table that
    /// happened to be self-consistently wrong would still be caught. This is
    /// the gzip CRC of the string in RFC 1952's own worked example lineage;
    /// it also matches Python's `zlib.crc32(b"The quick brown fox jumps over
    /// the lazy dog")`.
    #[test]
    fn second_independent_vector() {
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    /// A single byte, checked against the table entry it must reduce to. This
    /// catches an off-by-one in the index computation that longer inputs can
    /// mask by averaging over many table lookups.
    #[test]
    fn single_byte() {
        assert_eq!(crc32(b"a"), 0xE8B7_BE43);
        assert_eq!(crc32(&[0u8]), 0xD202_EF8D);
    }

    /// The raw accumulator is *not* the finished CRC. Asserting they differ
    /// pins the distinction that F2FS depends on, so a "simplification" that
    /// made `crc32_raw` invert would fail here rather than silently in a
    /// filesystem.
    #[test]
    fn raw_is_not_inverted() {
        assert_ne!(crc32_raw(!0u32, b"123456789"), crc32(b"123456789"));
        assert_eq!(crc32_raw(!0u32, b"123456789") ^ !0u32, crc32(b"123456789"));
    }
}
