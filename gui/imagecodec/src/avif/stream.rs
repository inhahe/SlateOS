//! A bounds-checked reader over the bytes of one box: libavif's `avifROStream`
//! (`src/stream.c`), whose every failure this reproduces, since whether a
//! damaged file is refused -- and where -- is part of what is being matched.
//!
//! Big-endian throughout, as ISO BMFF is. Bits are read most significant
//! first, and the first bit of a byte books the whole byte, so a field of
//! bits followed by whole bytes reads as the format lays it out. libavif
//! asserts that every whole-byte read starts at a byte boundary; the parsers
//! here, like libavif's, only ever read bits in whole bytes' worth.
//!
//! Portions of this file are copyright 2019 Joe Drago, from libavif, and
//! used under its BSD-2-Clause licence: `licenses/libavif-LICENSE.txt`.

/// A read went past the end of the box, or found a value the format rules
/// out. Every parse failure below is one of these, and the caller only needs
/// to know that the box is malformed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Truncated;

/// A cursor over one box's payload.
#[derive(Clone, Debug)]
pub(super) struct Stream<'a> {
    data: &'a [u8],
    offset: usize,
    /// How many bits of the byte before `offset` have been read; 0 when the
    /// stream is at a byte boundary.
    used_bits: u8,
}

/// A box header: its type, and the length of its payload -- `None` for a box
/// of size 0, which runs to the end of the file (legal only at the top
/// level).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BoxHeader {
    pub(super) kind: [u8; 4],
    pub(super) size: Option<usize>,
}

impl<'a> Stream<'a> {
    pub(super) const fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            offset: 0,
            used_bits: 0,
        }
    }

    /// How far into the box the stream is.
    pub(super) const fn offset(&self) -> usize {
        self.offset
    }

    pub(super) fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.offset)
    }

    /// `avifROStreamHasBytesLeft`.
    pub(super) fn has_bytes_left(&self, count: usize) -> bool {
        count <= self.remaining()
    }

    /// The bytes from here to the end.
    pub(super) fn rest(&self) -> &'a [u8] {
        self.data.get(self.offset..).unwrap_or_default()
    }

    pub(super) fn skip(&mut self, count: usize) -> Result<(), Truncated> {
        if !self.has_bytes_left(count) {
            return Err(Truncated);
        }
        self.offset = self.offset.saturating_add(count);
        Ok(())
    }

    pub(super) fn bytes(&mut self, count: usize) -> Result<&'a [u8], Truncated> {
        let end = self.offset.checked_add(count).ok_or(Truncated)?;
        let out = self.data.get(self.offset..end).ok_or(Truncated)?;
        self.offset = end;
        Ok(out)
    }

    pub(super) fn array<const N: usize>(&mut self) -> Result<[u8; N], Truncated> {
        self.bytes(N)?.try_into().map_err(|_| Truncated)
    }

    pub(super) fn u8(&mut self) -> Result<u8, Truncated> {
        let [byte] = self.array()?;
        Ok(byte)
    }

    pub(super) fn u16(&mut self) -> Result<u16, Truncated> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    pub(super) fn u32(&mut self) -> Result<u32, Truncated> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    pub(super) fn u64(&mut self) -> Result<u64, Truncated> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    /// `avifROStreamReadUX8`: an unsigned number of `size` bytes -- 0, 1, 2, 4
    /// or 8 -- zero for a size of 0, and a failure for any other size.
    pub(super) fn ux8(&mut self, size: u32) -> Result<u64, Truncated> {
        Ok(match size {
            0 => 0,
            1 => u64::from(self.u8()?),
            2 => u64::from(self.u16()?),
            4 => u64::from(self.u32()?),
            8 => self.u64()?,
            _ => return Err(Truncated),
        })
    }

    /// `avifROStreamReadBitsU32`: `count` bits, at most 32, most significant
    /// first.
    pub(super) fn bits(&mut self, count: u8) -> Result<u32, Truncated> {
        if count > 32 {
            return Err(Truncated);
        }
        let mut left = count;
        let mut value = 0u32;
        while left > 0 {
            if self.used_bits == 0 {
                // Book a new partial byte.
                self.skip(1)?;
            }
            // `skip` has just moved past the byte, or an earlier read did.
            let packed = self
                .data
                .get(self.offset.wrapping_sub(1))
                .copied()
                .ok_or(Truncated)?;
            let take = left.min(8u8.saturating_sub(self.used_bits));
            self.used_bits = self.used_bits.saturating_add(take);
            left = left.saturating_sub(take);
            // `take` is 1 to 8 and `left` below 32: neither shift can lose a
            // bit it should keep.
            let shifted = u32::from(packed).wrapping_shr(u32::from(8u8.saturating_sub(self.used_bits)));
            let mask = 1u32.wrapping_shl(u32::from(take)).wrapping_sub(1);
            value |= (shifted & mask).wrapping_shl(u32::from(left));
            if self.used_bits == 8 {
                self.used_bits = 0;
            }
        }
        Ok(value)
    }

    /// `avifROStreamReadString`: a NUL-terminated string, its bytes without
    /// the NUL. A string with no terminator before the end fails.
    pub(super) fn string(&mut self) -> Result<&'a [u8], Truncated> {
        let rest = self.rest();
        let len = rest.iter().position(|&b| b == 0).ok_or(Truncated)?;
        let out = rest.get(..len).ok_or(Truncated)?;
        self.skip(len.saturating_add(1))?;
        Ok(out)
    }

    /// `avifROStreamReadVersionAndFlags`: a full box's version and flags.
    pub(super) fn version_and_flags(&mut self) -> Result<(u8, u32), Truncated> {
        let [version, a, b, c] = self.array()?;
        Ok((version, u32::from_be_bytes([0, a, b, c])))
    }

    /// `avifROStreamReadAndEnforceVersion`: the flags of a full box whose
    /// version must be `version`.
    pub(super) fn enforce_version(&mut self, version: u8) -> Result<u32, Truncated> {
        let (got, flags) = self.version_and_flags()?;
        if got == version { Ok(flags) } else { Err(Truncated) }
    }

    /// `avifROStreamReadBoxHeaderPartial`: a box header, `top_level` saying
    /// whether a size of 0 (to the end of the file) is allowed.
    pub(super) fn box_header_partial(&mut self, top_level: bool) -> Result<BoxHeader, Truncated> {
        let start = self.offset;
        let small = self.u32()?;
        let kind = self.array::<4>()?;
        let mut size = u64::from(small);
        if size == 1 {
            size = self.u64()?;
        }
        if &kind == b"uuid" {
            self.skip(16)?; // usertype
        }
        let read = u64::try_from(self.offset.saturating_sub(start)).map_err(|_| Truncated)?;
        if size == 0 {
            return if top_level {
                Ok(BoxHeader { kind, size: None })
            } else {
                Err(Truncated)
            };
        }
        let payload = size.checked_sub(read).ok_or(Truncated)?;
        let payload = usize::try_from(payload).map_err(|_| Truncated)?;
        Ok(BoxHeader {
            kind,
            size: Some(payload),
        })
    }

    /// `avifROStreamReadBoxHeader`: a child box's header, whose payload must
    /// fit in what is left of its parent. Its size is always known.
    pub(super) fn box_header(&mut self) -> Result<([u8; 4], usize), Truncated> {
        let header = self.box_header_partial(false)?;
        let size = header.size.ok_or(Truncated)?;
        if size > self.remaining() {
            return Err(Truncated);
        }
        Ok((header.kind, size))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail loudly on malformed fixtures")]

    use super::*;

    #[test]
    fn bits_read_most_significant_first_and_book_whole_bytes() {
        let mut s = Stream::new(&[0b1010_0110, 0x12, 0x34]);
        assert_eq!(s.bits(1).unwrap(), 1);
        assert_eq!(s.bits(7).unwrap(), 0b010_0110);
        assert_eq!(s.offset(), 1);
        assert_eq!(s.u16().unwrap(), 0x1234);
        let mut s = Stream::new(&[0xAB, 0xCD]);
        assert_eq!(s.bits(12).unwrap(), 0xABC);
        assert_eq!(s.bits(4).unwrap(), 0xD);
        assert!(s.bits(1).is_err());
        // Thirty-two bits across five bytes, starting mid-byte.
        let mut s = Stream::new(&[0x0F, 0xFF, 0x00, 0xFF, 0x00]);
        assert_eq!(s.bits(4).unwrap(), 0);
        assert_eq!(s.bits(32).unwrap(), 0xFFF0_0FF0);
        assert!(s.bits(33).is_err());
        assert_eq!(s.bits(0).unwrap(), 0);
    }

    #[test]
    fn a_string_needs_its_terminator() {
        let mut s = Stream::new(b"pict\0rest");
        assert_eq!(s.string().unwrap(), b"pict");
        assert_eq!(s.rest(), b"rest");
        assert!(Stream::new(b"none").string().is_err());
        assert_eq!(Stream::new(b"\0").string().unwrap(), b"");
    }

    #[test]
    fn a_box_header_is_its_payload_length_and_a_zero_size_only_at_the_top() {
        let mut s = Stream::new(&[0, 0, 0, 12, b'f', b't', b'y', b'p', 1, 2, 3, 4]);
        assert_eq!(s.box_header().unwrap(), (*b"ftyp", 4));
        // A size of 0 runs to the end of the file: a top-level box only.
        let zero = [0, 0, 0, 0, b'm', b'd', b'a', b't'];
        assert_eq!(
            Stream::new(&zero).box_header_partial(true).unwrap(),
            BoxHeader {
                kind: *b"mdat",
                size: None
            }
        );
        assert!(Stream::new(&zero).box_header().is_err());
        // A 64-bit size, and a size smaller than its own header.
        let large = [0, 0, 0, 1, b'f', b'r', b'e', b'e', 0, 0, 0, 0, 0, 0, 0, 17, 9];
        assert_eq!(Stream::new(&large).box_header().unwrap(), (*b"free", 1));
        assert!(
            Stream::new(&[0, 0, 0, 7, b'f', b'r', b'e', b'e'])
                .box_header()
                .is_err()
        );
        // A child box larger than what is left of its parent.
        assert!(
            Stream::new(&[0, 0, 0, 9, b'f', b'r', b'e', b'e'])
                .box_header()
                .is_err()
        );
        // A `uuid` box's header carries sixteen more bytes.
        let mut uuid = [0u8; 24];
        uuid[3] = 24;
        uuid[4..8].copy_from_slice(b"uuid");
        assert_eq!(Stream::new(&uuid).box_header().unwrap(), (*b"uuid", 0));
    }

    #[test]
    fn a_number_of_bytes_is_zero_one_two_four_or_eight() {
        let mut s = Stream::new(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(s.ux8(0).unwrap(), 0);
        assert_eq!(s.ux8(1).unwrap(), 1);
        assert_eq!(s.ux8(4).unwrap(), 0x0203_0405);
        assert!(s.ux8(3).is_err());
    }

    #[test]
    fn a_full_box_version_is_enforced_and_its_flags_kept() {
        assert_eq!(Stream::new(&[0, 0, 1, 2]).enforce_version(0), Ok(0x102));
        assert_eq!(Stream::new(&[1, 0, 0, 0]).enforce_version(0), Err(Truncated));
        assert_eq!(
            Stream::new(&[3, 0xAA, 0xBB, 0xCC]).version_and_flags(),
            Ok((3, 0x00AA_BBCC))
        );
    }
}
