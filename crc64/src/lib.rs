//! CRC-64 over the ECMA-182 polynomial `0x42F0E1EBA9EA3693`, MSB first.
//!
//! Two framings of it are in use, and they are the two util-linux carries in
//! `lib/crc64.c` (itself Lammert Bies' libcrc):
//!
//! - [`crc64_we`] — CRC-64/WE: seed all ones, result inverted. bcache's
//!   superblock checksum and one of bcachefs's three.
//! - [`crc64_ecma`] — CRC-64/ECMA-182: seed zero, no inversion.
//!
//! [`crc64_raw`] is the bare accumulator both are built on, for a caller that
//! checksums a range in pieces.
//!
//! Not to be confused with CRC-64/XZ (the *reflected* form of the same
//! polynomial, which xz uses): same polynomial, different bit order, and no
//! input produces the same value under both.
//!
//! # References
//!
//! - ECMA-182, annex C
//! - util-linux 2.39.3 `lib/crc64.c` (`ul_crc64_we`, `ul_crc64_ecma`)

#![no_std]

/// The polynomial, MSB-first.
const POLY: u64 = 0x42F0_E1EB_A9EA_3693;

/// The MSB-first lookup table, built at compile time.
const TABLE: [u64; 256] = {
    let mut table = [0u64; 256];
    let mut i = 0u64;
    while i < 256 {
        let mut crc = i << 56;
        let mut bit = 0;
        while bit < 8 {
            if crc & (1 << 63) != 0 {
                crc = (crc << 1) ^ POLY;
            } else {
                crc <<= 1;
            }
            bit += 1;
        }
        // `i` is bounded by the `while` above; a `const` block cannot use
        // `get_mut`.
        #[allow(clippy::indexing_slicing)]
        {
            table[i as usize] = crc;
        }
        i += 1;
    }
    table
};

/// The bare accumulator: `seed` folded over `data`, no inversion.
/// util-linux's `ul_update_crc64`, a byte at a time.
#[must_use]
pub fn crc64_raw(seed: u64, data: &[u8]) -> u64 {
    data.iter().fold(seed, |crc, &b| {
        let idx = ((crc >> 56) ^ u64::from(b)) & 0xFF;
        // Masked to eight bits, so always in range.
        #[allow(clippy::indexing_slicing)]
        let t = TABLE[idx as usize];
        (crc << 8) ^ t
    })
}

/// CRC-64/WE: seed all ones, result inverted. `ul_crc64_we`.
///
/// ```
/// assert_eq!(crc64::crc64_we(b"123456789"), 0x62EC_59E3_F1A4_F00A);
/// ```
#[must_use]
pub fn crc64_we(data: &[u8]) -> u64 {
    crc64_raw(!0, data) ^ !0
}

/// CRC-64/ECMA-182: seed zero, no inversion. `ul_crc64_ecma`.
///
/// ```
/// assert_eq!(crc64::crc64_ecma(b"123456789"), 0x6C40_DF5F_0B49_7347);
/// ```
#[must_use]
pub fn crc64_ecma(data: &[u8]) -> u64 {
    crc64_raw(0, data)
}

#[cfg(test)]
mod tests {
    use super::{TABLE, crc64_ecma, crc64_raw, crc64_we};

    /// The catalogue's check values for both framings.
    #[test]
    fn check_values() {
        assert_eq!(crc64_we(b"123456789"), 0x62EC_59E3_F1A4_F00A);
        assert_eq!(crc64_ecma(b"123456789"), 0x6C40_DF5F_0B49_7347);
    }

    /// Values computed by util-linux 2.39.3's own `lib/crc64.c`, compiled and
    /// run on Linux.
    #[test]
    fn util_linux_values() {
        assert_eq!(crc64_we(b""), 0);
        assert_eq!(crc64_we(b"a"), 0xce73_f427_acc0_a99a);
        assert_eq!(crc64_we(b"abc"), 0x048b_813a_f9f4_9702);
        assert_eq!(
            crc64_we(b"The quick brown fox jumps over the lazy dog"),
            0xbcd8_bb36_6d25_6116
        );
        assert_eq!(crc64_ecma(b"a"), 0x548f_1201_6245_1c62);
        assert_eq!(crc64_ecma(b"abc"), 0x6650_1a34_9a0e_0855);
    }

    /// The table's second entry is the polynomial itself, as libcrc's
    /// hand-written table begins.
    #[test]
    fn table_starts_like_libcrc() {
        assert_eq!(TABLE[0], 0);
        assert_eq!(TABLE[1], 0x42F0_E1EB_A9EA_3693);
        assert_eq!(TABLE[2], 0x85E1_C3D7_53D4_6D26);
        assert_eq!(TABLE[255], 0x9AFC_E626_CE85_B507);
    }

    #[test]
    fn chaining_matches_one_shot() {
        for split in 0..=9 {
            let (a, b) = b"123456789".split_at(split);
            assert_eq!(crc64_raw(crc64_raw(!0, a), b) ^ !0, crc64_we(b"123456789"));
        }
    }
}
