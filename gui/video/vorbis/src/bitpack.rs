//! Vorbis's bit reader: libogg's `oggpack_*` (LSb first), with its handling
//! of a packet's end exactly -- a read past the end returns -1 and leaves
//! the reader at the end, so that every read after it returns -1 too, which
//! Tremor's decoders test for.
//!
//! Translated into Rust from libogg's `src/bitwise.c` (the reading half),
//! copyright Xiph.Org, used under its BSD licence
//! (`licenses/tremor-COPYING`, the same terms).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions within a packet: a byte offset no larger than the packet's length, plus at most 8; bit counts of at most 40"
)]

/// A packet being read, bit by bit.
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    /// The byte the next bit is in.
    endbyte: usize,
    /// The next bit's place in it, 0 to 7.
    endbit: u32,
}

impl<'a> BitReader<'a> {
    /// `oggpack_readinit`.
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            endbyte: 0,
            endbit: 0,
        }
    }

    /// The low `bits` (0 to 32) of the stream from here, not consumed; -1
    /// where the packet has fewer (`oggpack_look`).
    #[inline]
    pub(crate) fn look(&self, bits: u32) -> i64 {
        if bits > 32 {
            return -1;
        }
        let mask = if bits == 32 {
            0xffff_ffff
        } else {
            (1u64 << bits) - 1
        };
        // The main path, eight bytes at hand: libogg assembles the bits it
        // needs a byte at a time, which, masked, is the same number.
        if let Some(word) = self.data.get(self.endbyte..self.endbyte + 8) {
            let mut b = [0u8; 8];
            b.copy_from_slice(word);
            return ((u64::from_le_bytes(b) >> self.endbit) & mask) as i64;
        }
        let total = bits + self.endbit;
        let storage = self.data.len();
        if self.endbyte + 4 >= storage {
            // Not the main path: is there enough?
            if self.endbyte + ((total as usize + 7) >> 3) > storage {
                return -1;
            }
            if total == 0 {
                return 0;
            }
        }
        let byte = |k: usize| u64::from(self.data.get(self.endbyte + k).copied().unwrap_or(0));
        let mut ret = byte(0) >> self.endbit;
        if total > 8 {
            ret |= byte(1) << (8 - self.endbit);
            if total > 16 {
                ret |= byte(2) << (16 - self.endbit);
                if total > 24 {
                    ret |= byte(3) << (24 - self.endbit);
                    if total > 32 && self.endbit != 0 {
                        ret |= byte(4) << (32 - self.endbit);
                    }
                }
            }
        }
        (ret & mask) as i64
    }

    /// Past `bits` bits (`oggpack_adv`): to the end, if there are not that
    /// many.
    pub(crate) fn adv(&mut self, bits: u32) {
        let total = bits as usize + self.endbit as usize;
        if self.endbyte + ((total + 7) >> 3) > self.data.len() {
            self.overflow();
            return;
        }
        self.endbyte += total / 8;
        self.endbit = (total & 7) as u32;
    }

    /// The next `bits` (0 to 32), consumed; -1 where the packet has fewer,
    /// the reader then left at its end (`oggpack_read`).
    pub(crate) fn read(&mut self, bits: u32) -> i64 {
        let v = self.look(bits);
        if v < 0 {
            self.overflow();
            return -1;
        }
        let total = bits as usize + self.endbit as usize;
        self.endbyte += total / 8;
        self.endbit = (total & 7) as u32;
        v
    }

    /// The bytes read, a part-read one counted (`oggpack_bytes`).
    pub(crate) fn bytes(&self) -> usize {
        self.endbyte + (self.endbit as usize).div_ceil(8)
    }

    /// The whole bytes not yet touched (`storage - oggpack_bytes`): -1 once
    /// a read has run past the end.
    pub(crate) fn storage_left(&self) -> i64 {
        self.data.len() as i64 - self.bytes() as i64
    }

    /// libogg's state after a read past the end.
    fn overflow(&mut self) {
        self.endbyte = self.data.len();
        self.endbit = 1;
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    #[test]
    fn bits_come_low_first() {
        let mut b = BitReader::new(&[0b1010_1100, 0xff]);
        assert_eq!(b.read(2), 0);
        assert_eq!(b.read(3), 0b011);
        assert_eq!(b.read(5), 0b11_101);
        assert_eq!(b.bytes(), 2);
    }

    #[test]
    fn past_the_end_is_minus_one_from_then_on() {
        let mut b = BitReader::new(&[0xff]);
        assert_eq!(b.read(7), 0x7f);
        assert_eq!(b.read(2), -1);
        assert_eq!(b.read(0), -1, "even nothing, once at the end");
        assert_eq!(b.look(1), -1);
    }

    #[test]
    fn thirty_two_bits_at_an_offset_read_five_bytes() {
        let mut b = BitReader::new(&[0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a]);
        assert_eq!(b.read(4), 0);
        // 0x9a_7856_3412_f0 shifted right by 4, low 32 bits.
        assert_eq!(b.read(32), 0x8563_412f);
    }
}
