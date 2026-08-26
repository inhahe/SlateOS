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
#[must_use]
pub fn crc32_raw(seed: u32, data: &[u8]) -> u32 {
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
    use super::{crc32, crc32_raw, crc32_seed};

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
