//! A FLAC bit reader: most significant bit first over a byte slice, as
//! libFLAC's `bitreader.c` reads -- raw fields of up to 64 bits, unary
//! counts, Rice-coded residual blocks and the frame header's UTF-8-coded
//! numbers.
//!
//! Every read either succeeds whole or fails with [`Eof`], which says only
//! that the slice ended: the caller, which knows whether more of the file
//! can be had, decides whether that is the stream's end or a reason to
//! read on and try again.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "bit positions within a slice of at most a few megabytes, and shifts by counts checked below 64"
)]

/// The slice ended before the read did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Eof;

/// What reading a Rice block found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RiceError {
    /// The slice ended.
    Eof,
    /// A symbol whose value cannot fit 32 bits: the stream is damaged.
    Invalid,
}

impl From<Eof> for RiceError {
    fn from(_: Eof) -> Self {
        Self::Eof
    }
}

/// A reader over `data`, from bit `pos`.
#[derive(Clone, Debug)]
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    /// Bits consumed from the slice's start.
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Bytes consumed, the last partly consumed one counted whole.
    pub(crate) fn bytes_consumed(&self) -> usize {
        self.pos.div_ceil(8)
    }

    pub(crate) fn is_byte_aligned(&self) -> bool {
        self.pos.is_multiple_of(8)
    }

    /// Bits to the next byte boundary.
    pub(crate) fn bits_to_alignment(&self) -> u32 {
        ((8 - self.pos % 8) % 8) as u32
    }

    fn bits_left(&self) -> usize {
        self.data.len() * 8 - self.pos.min(self.data.len() * 8)
    }

    /// `bits` bits (0 to 64) as an unsigned number.
    pub(crate) fn read_u64(&mut self, bits: u32) -> Result<u64, Eof> {
        if bits == 0 {
            return Ok(0);
        }
        if (bits as usize) > self.bits_left() {
            return Err(Eof);
        }
        let mut value = 0u64;
        let mut left = bits;
        while left > 0 {
            let byte = self.data.get(self.pos / 8).copied().ok_or(Eof)?;
            let offset = (self.pos % 8) as u32;
            let take = (8 - offset).min(left);
            let chunk = (u64::from(byte) >> (8 - offset - take)) & ((1u64 << take) - 1);
            value = if take == 64 {
                chunk
            } else {
                (value << take) | chunk
            };
            self.pos += take as usize;
            left -= take;
        }
        Ok(value)
    }

    /// `bits` bits (0 to 32) as an unsigned number.
    pub(crate) fn read_u32(&mut self, bits: u32) -> Result<u32, Eof> {
        // At most 32 bits were read, so the value fits.
        Ok(self.read_u64(bits.min(32))? as u32)
    }

    /// `bits` bits (1 to 64), sign-extended.
    pub(crate) fn read_i64(&mut self, bits: u32) -> Result<i64, Eof> {
        let raw = self.read_u64(bits)?;
        if bits == 0 || bits >= 64 {
            return Ok(raw as i64);
        }
        let mask = 1u64 << (bits - 1);
        Ok((raw ^ mask).wrapping_sub(mask) as i64)
    }

    /// `bits` bits (1 to 32), sign-extended to 32 bits.
    pub(crate) fn read_i32(&mut self, bits: u32) -> Result<i32, Eof> {
        // A field of at most 32 bits, sign-extended, fits 32 bits.
        Ok(self.read_i64(bits.min(32))? as i32)
    }

    /// The zero bits before the next one bit, which is consumed too.
    pub(crate) fn read_unary(&mut self) -> Result<u32, Eof> {
        let mut count = 0u32;
        loop {
            let byte = self.data.get(self.pos / 8).copied().ok_or(Eof)?;
            let offset = self.pos % 8;
            let rest = byte << offset;
            if rest == 0 {
                count = count.wrapping_add((8 - offset) as u32);
                self.pos += 8 - offset;
                continue;
            }
            let zeros = rest.leading_zeros();
            count = count.wrapping_add(zeros);
            self.pos += zeros as usize + 1;
            return Ok(count);
        }
    }

    /// `out.len()` Rice-coded signed values with parameter `k` (below 32).
    /// A symbol whose quotient is past what 32 bits can hold
    /// (`u32::MAX >> k`) is invalid, as libFLAC refuses it -- for `k` of
    /// 1 and up; for 0, any length of unary code is taken.
    pub(crate) fn read_rice_block(&mut self, out: &mut [i32], k: u32) -> Result<(), RiceError> {
        let limit = u32::MAX >> k;
        for v in out.iter_mut() {
            let msbs = self.read_unary()?;
            if k > 0 && msbs > limit {
                return Err(RiceError::Invalid);
            }
            let lsbs = self.read_u32(k)?;
            let x = msbs.wrapping_shl(k) | lsbs;
            *v = ((x >> 1) as i32) ^ -((x & 1) as i32);
        }
        Ok(())
    }

    /// A frame header's coded number (FLAC's extension of UTF-8: a frame
    /// number in up to six bytes, a sample number -- `wide` -- in up to
    /// seven), each byte read also appended to `raw` for the header's CRC.
    /// `None` for a byte sequence that is not one.
    pub(crate) fn read_utf8(&mut self, raw: &mut Vec<u8>, wide: bool) -> Result<Option<u64>, Eof> {
        let first = self.read_u32(8)?;
        raw.push(first as u8);
        let (mut value, extra) = match first {
            x if x & 0x80 == 0 => (u64::from(x), 0),
            x if x & 0xE0 == 0xC0 => (u64::from(x & 0x1F), 1),
            x if x & 0xF0 == 0xE0 => (u64::from(x & 0x0F), 2),
            x if x & 0xF8 == 0xF0 => (u64::from(x & 0x07), 3),
            x if x & 0xFC == 0xF8 => (u64::from(x & 0x03), 4),
            x if x & 0xFE == 0xFC => (u64::from(x & 0x01), 5),
            0xFE if wide => (0, 6),
            _ => return Ok(None),
        };
        for _ in 0..extra {
            let x = self.read_u32(8)?;
            raw.push(x as u8);
            if x & 0xC0 != 0x80 {
                return Ok(None);
            }
            value = (value << 6) | u64::from(x & 0x3F);
        }
        Ok(Some(value))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "a test: a failure should be loud")]
mod tests {
    use super::*;

    #[test]
    fn fields_are_read_most_significant_bit_first() {
        let data = [0b1010_1100, 0b0101_0011, 0xff, 0x00];
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_u32(1).unwrap(), 1);
        assert_eq!(r.read_u32(3).unwrap(), 0b010);
        assert_eq!(r.read_u32(8).unwrap(), 0b1100_0101);
        assert_eq!(r.read_i32(4).unwrap(), 0b0011);
        assert_eq!(r.read_i32(4).unwrap(), -1);
        assert!(!r.is_byte_aligned() || r.bytes_consumed() == 3);
        assert_eq!(r.read_u64(12).unwrap(), 0xf00);
        assert_eq!(r.read_u32(1), Err(Eof));
    }

    #[test]
    fn unary_and_rice_codes() {
        // 0001, then 1: three zeros, then none.
        let data = [0b0001_1000];
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_unary().unwrap(), 3);
        assert_eq!(r.read_unary().unwrap(), 0);
        // Rice, k = 2: 5 is 0b101 -> msbs 1, lsbs 01 -> "01" "01"; zigzag 5
        // is -3. 2 is "1" "10" -> zigzag +1.
        let data = [0b0101_1100];
        let mut r = BitReader::new(&data);
        let mut out = [0i32; 2];
        r.read_rice_block(&mut out, 2).unwrap();
        assert_eq!(out, [-3, 1]);
    }

    #[test]
    fn coded_numbers_are_flacs_utf8() {
        let mut raw = Vec::new();
        assert_eq!(
            BitReader::new(&[0x41]).read_utf8(&mut raw, false).unwrap(),
            Some(0x41)
        );
        let mut raw = Vec::new();
        assert_eq!(
            BitReader::new(&[0xC3, 0xA9])
                .read_utf8(&mut raw, false)
                .unwrap(),
            Some(0xE9)
        );
        assert_eq!(raw, [0xC3, 0xA9]);
        let mut raw = Vec::new();
        assert_eq!(
            BitReader::new(&[0xC3, 0x29])
                .read_utf8(&mut raw, false)
                .unwrap(),
            None
        );
        let mut raw = Vec::new();
        assert_eq!(
            BitReader::new(&[0xFF]).read_utf8(&mut raw, true).unwrap(),
            None
        );
        let mut raw = Vec::new();
        assert_eq!(
            BitReader::new(&[0xFE, 0x80, 0x80, 0x80, 0x80, 0x80, 0x81])
                .read_utf8(&mut raw, true)
                .unwrap(),
            Some(1)
        );
        let mut raw = Vec::new();
        assert_eq!(
            BitReader::new(&[0xFE, 0x80])
                .read_utf8(&mut raw, false)
                .unwrap(),
            None
        );
        let mut raw = Vec::new();
        assert_eq!(BitReader::new(&[0xE2]).read_utf8(&mut raw, false), Err(Eof));
    }
}
