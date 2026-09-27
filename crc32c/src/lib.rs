//! CRC-32C (Castagnoli) — the reflected polynomial `0x82F63B78`.
//!
//! This is the checksum of ext4's and btrfs's metadata, of XFS v5, bcachefs,
//! EROFS, iSCSI and SCTP. It is **not** CRC-32 (ISO 3309 / IEEE 802.3,
//! `0xEDB88320`), which gzip, ZIP and PNG use and which lives in the `crc32`
//! crate. The two are impossible to confuse safely: a checksum computed with
//! the wrong polynomial verifies against nothing, and the failure reads as
//! data corruption rather than as a bug.
//!
//! # Why this is its own crate
//!
//! The kernel keeps a copy inside `crypto.rs`, a module of a binary crate that
//! nothing else can depend on, so every userspace caller had to write its own:
//! `ulmount`'s libblkid layer carried a bit-at-a-time loop for the one ext4
//! checksum it verified. The libblkid port verifies eight kinds of superblock
//! with this polynomial, which is past the point where private copies are
//! defensible. Hence a leaf crate on the model of `crc32`.
//!
//! # The three framings
//!
//! Callers disagree about the conventional pre- and post-inversion, so all
//! three are here and named for what they do:
//!
//! - [`crc32c`] — seed `!0`, result inverted: the catalogued CRC-32C,
//!   check value `0xE3069283`.
//! - [`crc32c_seed`] — a caller's seed, result inverted.
//! - [`crc32c_raw`] — a caller's seed, *no* inversion. This is util-linux's
//!   `crc32c(crc, buf, size)`, which libblkid's probers call directly and
//!   invert (or not) themselves; ext4 compares the raw accumulator.
//!
//! # References
//!
//! - RFC 3720 §12.1 and appendix B.4 (iSCSI's definition and test vectors)
//! - util-linux 2.39.3 `lib/crc32c.c` (`crc32c`, `ul_crc32c_exclude_offset`)

#![no_std]

/// CRC-32C lookup table for the reflected Castagnoli polynomial `0x82F63B78`
/// (bit-reversed `0x1EDC6F41`), built at compile time.
const TABLE: [u32; 256] = {
    const POLY: u32 = 0x82F6_3B78;
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
        // `i` is bounded by the `while` above, so the index is in range; a
        // `const` block cannot use `get_mut`.
        #[allow(clippy::indexing_slicing)]
        {
            table[i as usize] = crc;
        }
        i += 1;
    }
    table
};

/// One byte into the accumulator.
#[inline]
fn step(crc: u32, byte: u8) -> u32 {
    let idx = ((crc ^ u32::from(byte)) & 0xFF) as usize;
    // Masked to eight bits, so always in range.
    #[allow(clippy::indexing_slicing)]
    let t = TABLE[idx];
    t ^ (crc >> 8)
}

/// CRC-32C of `data`: seed `!0`, result inverted.
///
/// ```
/// assert_eq!(crc32c::crc32c(b"123456789"), 0xE306_9283);
/// ```
#[must_use]
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c_seed(!0, data)
}

/// CRC-32C of `data` from a caller's `seed`, the result inverted.
///
/// Chains with [`crc32c_raw`]: ext4's metadata checksums seed with the raw
/// CRC of the filesystem UUID and finish over the block.
#[must_use]
pub fn crc32c_seed(seed: u32, data: &[u8]) -> u32 {
    crc32c_raw(seed, data) ^ !0
}

/// The bare accumulator: `seed` folded over `data`, with no inversion either
/// side. util-linux's `crc32c(crc, buf, size)`.
#[must_use]
pub fn crc32c_raw(seed: u32, data: &[u8]) -> u32 {
    data.iter().fold(seed, |crc, &b| step(crc, b))
}

/// util-linux's `ul_crc32c_exclude_offset`: [`crc32c_raw`] over `data` with
/// the `exclude_len` bytes at `exclude_off` read as zeros — how a superblock
/// that stores its own checksum inside the checksummed range is verified.
///
/// Upstream asserts the range lies inside `data`; here a range reaching past
/// the end zeroes only what exists, so the function has no failure mode.
#[must_use]
pub fn crc32c_raw_exclude(seed: u32, data: &[u8], exclude_off: usize, exclude_len: usize) -> u32 {
    let end = exclude_off.saturating_add(exclude_len);
    data.iter().enumerate().fold(seed, |crc, (i, &b)| {
        let b = if i >= exclude_off && i < end { 0 } else { b };
        step(crc, b)
    })
}

#[cfg(test)]
mod tests {
    use super::{crc32c, crc32c_raw, crc32c_raw_exclude, crc32c_seed};

    /// The catalogue's check value.
    #[test]
    fn check_value() {
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
    }

    /// Both inversions, in the right order: the empty input is 0.
    #[test]
    fn empty_is_zero() {
        assert_eq!(crc32c(b""), 0);
        assert_eq!(crc32c_raw(!0, b""), !0);
    }

    /// Raw accumulators computed by util-linux 2.39.3's own `crc32c(~0U, …)`,
    /// compiled and run on Linux: a second source, so a table that were
    /// self-consistently wrong would still fail.
    #[test]
    fn util_linux_raw_values() {
        assert_eq!(crc32c_raw(!0, b"a"), 0x3e2f_bccf);
        assert_eq!(crc32c_raw(!0, b"abc"), 0xc9b4_c048);
        assert_eq!(crc32c_raw(!0, b"123456789"), 0x1cf9_6d7c);
        assert_eq!(
            crc32c_raw(!0, b"The quick brown fox jumps over the lazy dog"),
            0xdd9d_fbfb
        );
    }

    /// RFC 3720 B.4: 32 bytes of zeros, and 32 of 0xFF.
    #[test]
    fn rfc3720_vectors() {
        assert_eq!(crc32c(&[0u8; 32]), 0x8A91_36AA);
        assert_eq!(crc32c(&[0xFFu8; 32]), 0x62A8_AB43);
        let ascending: [u8; 32] = core::array::from_fn(|i| u8::try_from(i).unwrap_or(0));
        assert_eq!(crc32c(&ascending), 0x46DD_794E);
    }

    #[test]
    fn chaining_matches_one_shot() {
        for split in 0..=9 {
            let (a, b) = b"123456789".split_at(split);
            assert_eq!(crc32c_seed(crc32c_raw(!0, a), b), 0xE306_9283);
        }
    }

    /// Excluding a range is checksumming zeros in its place.
    #[test]
    fn exclusion_reads_zeros() {
        let data = *b"abcdefghij";
        let mut zeroed = data;
        zeroed[3..7].fill(0);
        assert_eq!(
            crc32c_raw_exclude(!0, &data, 3, 4),
            crc32c_raw(!0, &zeroed)
        );
        // Nothing excluded is the plain CRC; a range past the end zeroes the
        // tail that exists.
        assert_eq!(crc32c_raw_exclude(!0, &data, 3, 0), crc32c_raw(!0, &data));
        let mut tail = data;
        tail[8..].fill(0);
        assert_eq!(crc32c_raw_exclude(!0, &data, 8, 100), crc32c_raw(!0, &tail));
    }
}
