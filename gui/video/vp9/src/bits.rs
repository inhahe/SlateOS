//! The uncompressed header's bit reader: plain bits, most significant first.
//!
//! The first part of every VP9 frame -- its type, size, references, filters,
//! quantisers, segmentation and tiles -- is written as ordinary bits rather
//! than arithmetic-coded, so that a demuxer can learn a frame's size and kind
//! without running the entropy decoder. This reads it.
//!
//! Reading past the end answers zero and remembers that it happened
//! ([`BitReader::overran`]), as libvpx's reader calls its error handler and
//! answers zero: the header's parser then stops at the first field it cannot
//! trust instead of every caller checking every bit.
//!
//! Translated into Rust from libvpx v1.17.0's `vpx_dsp/bitreader_buffer.c`
//! (copyright the WebM project authors), used under libvpx's BSD licence and
//! patent grant (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

/// A reader of plain bits over one frame's bytes.
#[derive(Clone, Debug)]
pub struct BitReader<'a> {
    data: &'a [u8],
    /// Bits read so far, from the start of `data`.
    bit_offset: usize,
    /// Whether a read went past the end of `data`.
    overran: bool,
}

impl<'a> BitReader<'a> {
    /// A reader at the first bit of `data`.
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            bit_offset: 0,
            overran: false,
        }
    }

    /// One bit, or zero past the end.
    pub fn bit(&mut self) -> u32 {
        let byte = self.bit_offset >> 3;
        // 7 - (offset mod 8), without arithmetic: the low three bits inverted.
        let shift = !self.bit_offset & 7;
        match self.data.get(byte) {
            Some(&b) => {
                self.bit_offset = self.bit_offset.saturating_add(1);
                u32::from(b >> shift) & 1
            }
            None => {
                self.overran = true;
                0
            }
        }
    }

    /// Whether the next bit is set: [`Self::bit`] as a boolean.
    pub fn flag(&mut self) -> bool {
        self.bit() == 1
    }

    /// `bits` bits as an unsigned number, most significant first. `bits` is
    /// at most 32, as every field of the header is.
    pub fn literal(&mut self, bits: u32) -> u32 {
        let mut value = 0u32;
        for _ in 0..bits.min(32) {
            value = (value << 1) | self.bit();
        }
        value
    }

    /// A `bits`-bit magnitude followed by a sign bit, as the header writes its
    /// signed deltas.
    #[allow(
        clippy::cast_possible_wrap,
        reason = "every signed field of the header is at most 6 bits wide"
    )]
    pub fn signed_literal(&mut self, bits: u32) -> i32 {
        let value = self.literal(bits.min(30)) as i32;
        if self.flag() {
            value.wrapping_neg()
        } else {
            value
        }
    }

    /// How many bytes the bits read so far occupy, a partial byte counting
    /// whole: where the compressed header starts.
    #[must_use]
    pub const fn bytes_read(&self) -> usize {
        self.bit_offset.div_ceil(8)
    }

    /// Whether any read went past the end of the data.
    #[must_use]
    pub const fn overran(&self) -> bool {
        self.overran
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn bits_come_most_significant_first() {
        let mut r = BitReader::new(&[0b1010_0000, 0xFF]);
        assert_eq!(r.bit(), 1);
        assert_eq!(r.bit(), 0);
        assert_eq!(r.literal(2), 0b10);
        assert_eq!(r.literal(4), 0);
        assert_eq!(r.literal(8), 0xFF);
        assert!(!r.overran());
        assert_eq!(r.bytes_read(), 2);
    }

    #[test]
    fn a_literal_may_straddle_bytes() {
        let mut r = BitReader::new(&[0x0F, 0xF0]);
        assert_eq!(r.literal(4), 0);
        assert_eq!(r.literal(8), 0xFF);
        assert_eq!(r.bytes_read(), 2, "a partial byte counts whole");
    }

    #[test]
    fn a_signed_literal_reads_its_sign_after_its_magnitude() {
        // 000101 then sign 1: -5; then 000011 sign 0: +3.
        let mut r = BitReader::new(&[0b0001_0110, 0b0001_1000]);
        assert_eq!(r.signed_literal(6), -5);
        assert_eq!(r.signed_literal(6), 3);
    }

    #[test]
    fn reading_past_the_end_answers_zero_and_says_so() {
        let mut r = BitReader::new(&[0xFF]);
        assert_eq!(r.literal(8), 0xFF);
        assert!(!r.overran());
        assert_eq!(r.literal(3), 0);
        assert!(r.overran());
        assert_eq!(r.bytes_read(), 1, "the bits past the end were not read");
    }
}
