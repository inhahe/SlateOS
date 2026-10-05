//! The integrity checks a `.xz` block may carry: liblzma's `check.c`,
//! `crc64_fast.c` and `crc64_tablegen.c`.
//!
//! The format names sixteen check IDs and fixes each one's size (none, then
//! three of 4 bytes, three of 8, three of 16, three of 32 and three of 64), so
//! a reader can step over a check it cannot compute. Three are defined --
//! CRC-32, CRC-64 and SHA-256 -- and liblzma 1.0 through 5.x computes those;
//! the rest it skips unverified, and `xz -d` says so. This does the same.
//!
//! CRC-32 is the `crc32` crate's (the reflected IEEE polynomial, as gzip's),
//! SHA-256 the `sha2` crate's. CRC-64 is here: xz's is CRC-64/XZ, the
//! *reflected* ECMA-182 polynomial, which the `crc64` crate deliberately does
//! not provide -- its two framings run the same polynomial most significant
//! bit first, and no input gives the same value under both.

use alloc::boxed::Box;

use sha2::Sha256;

/// A check ID from a stream's flags: 0 to 15.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Check(u8);

impl Check {
    /// No check (ID 0x00).
    pub const NONE: Self = Self(0x00);
    /// CRC-32 (ID 0x01).
    pub const CRC32: Self = Self(0x01);
    /// CRC-64/XZ (ID 0x04), what `xz` writes unless told otherwise.
    pub const CRC64: Self = Self(0x04);
    /// SHA-256 (ID 0x0A).
    pub const SHA256: Self = Self(0x0a);

    /// The check with ID `id`, if `id` is one of the sixteen the format has.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<Self> {
        if id <= 0x0f { Some(Self(id)) } else { None }
    }

    /// The check's ID.
    #[must_use]
    pub const fn id(self) -> u8 {
        self.0
    }

    /// The size of the check field, in bytes (`lzma_check_size`): fixed by
    /// the ID even for the IDs no check is defined for.
    #[must_use]
    pub const fn size(self) -> usize {
        match self.0 {
            0 => 0,
            1..=3 => 4,
            4..=6 => 8,
            7..=9 => 16,
            10..=12 => 32,
            _ => 64,
        }
    }

    /// Whether this crate can compute the check (`lzma_check_is_supported`):
    /// a block with any other is decoded without its integrity verified.
    #[must_use]
    pub const fn is_supported(self) -> bool {
        matches!(self.0, 0x00 | 0x01 | 0x04 | 0x0a)
    }
}

/// CRC-64/XZ's table: the reflected ECMA-182 polynomial, built as
/// `crc64_tablegen.c` builds the first of its four.
const CRC64_TABLE: [u64; 256] = {
    const POLY: u64 = 0xc96c_5795_d787_0f42;
    let mut table = [0u64; 256];
    let mut i = 0u64;
    while i < 256 {
        let mut crc = i;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ POLY
            } else {
                crc >> 1
            };
            bit += 1;
        }
        // A `const` block cannot use `get_mut`; `i` is below 256 by the
        // loop's condition.
        #[allow(clippy::indexing_slicing)]
        {
            table[i as usize] = crc;
        }
        i += 1;
    }
    table
};

/// `lzma_crc64`: continues the CRC-64/XZ `crc` over `data`.
pub(crate) fn crc64(data: &[u8], crc: u64) -> u64 {
    let mut c = !crc;
    for &b in data {
        let i = usize::from((c as u8) ^ b);
        // `i` is a byte, and the table has 256 entries.
        #[allow(clippy::indexing_slicing)]
        let entry = CRC64_TABLE[i];
        c = entry ^ (c >> 8);
    }
    !c
}

/// `lzma_crc32`: continues the CRC-32 `crc` over `data`.
pub(crate) fn crc32(data: &[u8], crc: u32) -> u32 {
    crc32::crc32_seed(!crc, data)
}

/// A check being computed over a block's output (`lzma_check_state`).
pub(crate) enum CheckState {
    Unverified,
    Crc32(u32),
    Crc64(u64),
    Sha256(Box<Sha256>),
}

impl CheckState {
    /// `lzma_check_init`.
    pub(crate) fn new(check: Check) -> Self {
        match check {
            Check::CRC32 => Self::Crc32(0),
            Check::CRC64 => Self::Crc64(0),
            Check::SHA256 => Self::Sha256(Box::new(Sha256::new())),
            _ => Self::Unverified,
        }
    }

    /// `lzma_check_update`.
    pub(crate) fn update(&mut self, data: &[u8]) {
        match self {
            Self::Unverified => {}
            Self::Crc32(c) => *c = crc32(data, *c),
            Self::Crc64(c) => *c = crc64(data, *c),
            Self::Sha256(s) => s.update(data),
        }
    }

    /// `lzma_check_finish`: the check's bytes as the format stores them
    /// (the CRCs little-endian), or `None` for a check that was not computed.
    pub(crate) fn finish(self) -> Option<alloc::vec::Vec<u8>> {
        match self {
            Self::Unverified => None,
            Self::Crc32(c) => Some(c.to_le_bytes().to_vec()),
            Self::Crc64(c) => Some(c.to_le_bytes().to_vec()),
            Self::Sha256(s) => Some(s.finalize().to_vec()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// The catalogue check values.
    #[test]
    fn the_crcs_are_the_catalogues() {
        assert_eq!(crc64(b"123456789", 0), 0x995d_c9bb_df19_39fa);
        assert_eq!(crc32(b"123456789", 0), 0xcbf4_3926);
        // Continuing a CRC over a second piece is the CRC of the whole.
        assert_eq!(crc64(b"56789", crc64(b"1234", 0)), crc64(b"123456789", 0));
        assert_eq!(crc32(b"56789", crc32(b"1234", 0)), crc32(b"123456789", 0));
    }

    #[test]
    fn every_id_has_its_size() {
        let sizes: alloc::vec::Vec<usize> = (0..16)
            .map(|id| Check::from_id(id).unwrap().size())
            .collect();
        assert_eq!(
            sizes,
            [0, 4, 4, 4, 8, 8, 8, 16, 16, 16, 32, 32, 32, 64, 64, 64]
        );
        assert_eq!(Check::from_id(16), None);
        let supported: alloc::vec::Vec<u8> = (0..16)
            .filter(|&id| Check::from_id(id).unwrap().is_supported())
            .collect();
        assert_eq!(supported, [0, 1, 4, 10]);
    }
}
