//! The bool decoder: VP9's arithmetic decoder, which reads everything after the
//! uncompressed header.
//!
//! Each call decodes one binary decision against a probability (out of 256)
//! that the decision is 0. Trees of such decisions decode the multi-way
//! symbols: modes, partitions, coefficient tokens.
//!
//! This is libvpx's `vpx_reader` translated as it is, 64-bit window and all,
//! rather than a reader written to the specification, because two of its
//! behaviours are observable and a different reader would differ:
//!
//! - Past the end of its data it shifts in zeros and keeps decoding, as if the
//!   stream were padded with zero bytes. Valid streams rely on it at their
//!   tail.
//! - [`BoolReader::has_error`] says when decoding has gone *beyond* that tail.
//!   libvpx marks a tile corrupt by it, and which frames of a damaged file
//!   decode depends on exactly when it turns true.
//!
//! Translated into Rust from libvpx v1.17.0's `vpx_dsp/bitreader.c`,
//! `vpx_dsp/bitreader.h` and `vpx_dsp/prob.c` (copyright the WebM project
//! authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

use crate::Error;

/// The width of the window, `BD_VALUE_SIZE`: libvpx's on a 64-bit machine.
const WINDOW_BITS: i32 = 64;

/// libvpx's `LOTS_OF_BITS`: added to the bit count when the data runs out, so
/// that the count can say whether reads have gone past it.
const LOTS_OF_BITS: i32 = 0x4000_0000;

/// For a range, how far to shift it left to bring its top bit to bit 7:
/// libvpx's `vpx_norm`.
#[allow(
    clippy::indexing_slicing,
    reason = "evaluated at compile time, where an index out of range fails the build"
)]
const NORM: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut i = 1;
    while i < 256 {
        // The number of leading zeros of `i` as an 8-bit value.
        let mut shift = 0u8;
        let mut v = i;
        while v < 128 {
            v <<= 1;
            shift += 1;
        }
        table[i] = shift;
        i += 1;
    }
    table
};

/// A tree of binary decisions: libvpx's `vpx_tree_index` arrays. Index pairs
/// name the next node (positive, even) or a leaf (its symbol negated, or 0).
pub type Tree = [i8];

/// The arithmetic decoder over one partition of a frame.
#[derive(Clone, Debug)]
pub struct BoolReader<'a> {
    data: &'a [u8],
    /// The next unread byte of `data`.
    pos: usize,
    /// The window: the decoding window in its top byte, buffered bits below.
    value: u64,
    /// The bits buffered below the top byte, less 8; past the end, plus
    /// [`LOTS_OF_BITS`].
    count: i32,
    range: u32,
}

impl<'a> BoolReader<'a> {
    /// Start decoding `data`, reading its marker bit.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupt`] if the marker bit is set: libvpx refuses the
    /// partition, and so does this.
    pub fn new(data: &'a [u8]) -> Result<Self, Error> {
        let mut r = Self {
            data,
            pos: 0,
            value: 0,
            count: -8,
            range: 255,
        };
        r.fill();
        if r.read_bit() == 0 {
            Ok(r)
        } else {
            Err(Error::Corrupt("a partition's marker bit is set"))
        }
    }

    /// Refill the window from the data: `vpx_reader_fill`.
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "the shifts and counts stay within the 64-bit window as in libvpx: `shift` is 48 - count with count in -8..=0 here, and `bits_left` is compared before it is narrowed"
    )]
    fn fill(&mut self) {
        let rest = self.data.get(self.pos..).unwrap_or(&[]);
        let bytes_left = rest.len();
        let bits_left = bytes_left.saturating_mul(8);
        let mut shift = WINDOW_BITS - 8 - (self.count + 8);
        let mut value = self.value;
        let mut count = self.count;
        if bits_left > WINDOW_BITS as usize {
            // Eight bytes are there: take whole bytes, as many as fit.
            let bits = (shift & !7) + 8;
            let mut word = [0u8; 8];
            if let Some(eight) = rest.get(..8) {
                word.copy_from_slice(eight);
            }
            let big_endian = u64::from_be_bytes(word);
            let nv = big_endian >> (WINDOW_BITS - bits);
            count += bits;
            self.pos = self.pos.saturating_add((bits >> 3) as usize);
            value |= nv << (shift & 7);
        } else {
            let bits_over = shift + 8 - bits_left as i32;
            let mut loop_end = 0;
            if bits_over >= 0 {
                count += LOTS_OF_BITS;
                loop_end = bits_over;
            }
            if bits_over < 0 || bits_left > 0 {
                let mut taken = 0usize;
                while shift >= loop_end {
                    count += 8;
                    let byte = rest.get(taken).copied().unwrap_or(0);
                    value |= u64::from(byte) << shift;
                    taken += 1;
                    shift -= 8;
                }
                self.pos = self.pos.saturating_add(taken);
            }
        }
        self.value = value;
        self.count = count;
    }

    /// Decode one decision whose probability of being 0 is `prob`/256:
    /// `vpx_read`.
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_wrap,
        reason = "range is 128..=255 and prob 0..=255, so split fits; the window arithmetic is libvpx's, on a 64-bit value with the split in its top byte"
    )]
    pub fn read(&mut self, prob: u8) -> u32 {
        let prob = u32::from(prob);
        let split = (self.range * prob + (256 - prob)) >> 8;
        if self.count < 0 {
            self.fill();
        }
        let mut value = self.value;
        let mut range = split;
        let bigsplit = u64::from(split) << (WINDOW_BITS - 8);
        let mut bit = 0;
        if value >= bigsplit {
            range = self.range - split;
            value -= bigsplit;
            bit = 1;
        }
        let shift = NORM.get(range as usize & 0xFF).copied().unwrap_or(0);
        self.range = range << shift;
        self.value = value << shift;
        self.count -= i32::from(shift);
        bit
    }

    /// One decision at even odds.
    pub fn read_bit(&mut self) -> u32 {
        self.read(128)
    }

    /// Whether one decision at `prob` came out 1.
    pub fn read_bool(&mut self, prob: u8) -> bool {
        self.read(prob) == 1
    }

    /// `bits` even-odds decisions as a number, most significant first.
    pub fn read_literal(&mut self, bits: u32) -> u32 {
        let mut literal = 0;
        for _ in 0..bits.min(32) {
            literal = (literal << 1) | self.read_bit();
        }
        literal
    }

    /// A symbol from `tree`, deciding each node with `probs[node / 2]`:
    /// `vpx_read_tree`. A tree or probabilities that do not fit each other end
    /// the walk at symbol 0, never out of bounds.
    #[allow(
        clippy::cast_sign_loss,
        clippy::arithmetic_side_effects,
        reason = "a node index is non-negative and below the tree's length, checked by get"
    )]
    pub fn read_tree(&mut self, tree: &Tree, probs: &[u8]) -> u32 {
        let mut i: i8 = 0;
        loop {
            let node = i as usize;
            let prob = probs.get(node >> 1).copied().unwrap_or(128);
            let next = self.read(prob) as usize;
            match tree.get(node + next) {
                Some(&n) if n > 0 => i = n,
                Some(&n) => return u32::from(n.unsigned_abs()),
                None => return 0,
            }
        }
    }

    /// Whether decoding has gone beyond the zero padding past the end of the
    /// data: `vpx_reader_has_error`.
    #[must_use]
    pub const fn has_error(&self) -> bool {
        self.count > WINDOW_BITS && self.count < LOTS_OF_BITS
    }

    /// Where the coded data ends: the first byte this reader has not used.
    /// `vpx_reader_find_end`, which gives back the bytes buffered but not yet
    /// decoded.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "count only falls, and pos only by bytes it was raised by"
    )]
    pub fn find_end(&mut self) -> usize {
        while self.count > 8 && self.count < WINDOW_BITS {
            self.count -= 8;
            self.pos = self.pos.saturating_sub(1);
        }
        self.pos
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss
    )]

    use super::*;

    /// A bool *encoder*, libvpx's `vpx_writer` (`vpx_dsp/bitwriter.c`),
    /// written here only to make streams the decoder must read back.
    struct Writer {
        out: Vec<u8>,
        low: u32,
        range: u32,
        count: i32,
    }

    impl Writer {
        fn new() -> Self {
            let mut w = Self {
                out: Vec::new(),
                low: 0,
                range: 255,
                count: -24,
            };
            w.write(0, 128); // the marker bit
            w
        }

        fn write(&mut self, bit: u32, prob: u8) {
            let split = 1 + (((self.range - 1) * u32::from(prob)) >> 8);
            let mut range = split;
            let mut low = self.low;
            if bit != 0 {
                low += split;
                range = self.range - split;
            }
            let mut shift = i32::from(NORM[range as usize]);
            range <<= shift;
            let mut count = self.count + shift;
            if count >= 0 {
                let offset = shift - count;
                if (low << (offset - 1)) & 0x8000_0000 != 0 {
                    let mut x = self.out.len() as i32 - 1;
                    while x >= 0 && self.out[x as usize] == 0xff {
                        self.out[x as usize] = 0;
                        x -= 1;
                    }
                    self.out[x as usize] += 1;
                }
                self.out.push((low >> (24 - offset)) as u8);
                low <<= offset;
                shift = count;
                low &= 0xff_ffff;
                count -= 8;
            }
            low <<= shift;
            self.count = count;
            self.low = low;
            self.range = range;
        }

        fn finish(mut self) -> Vec<u8> {
            for _ in 0..32 {
                self.write(0, 128);
            }
            // libvpx avoids a superframe marker at the end; irrelevant here.
            self.out
        }
    }

    #[test]
    fn the_norm_table_is_libvpxs() {
        assert_eq!(NORM[0], 0);
        assert_eq!(NORM[1], 7);
        assert_eq!(NORM[2], 6);
        assert_eq!(NORM[3], 6);
        assert_eq!(NORM[127], 1);
        assert_eq!(NORM[128], 0);
        assert_eq!(NORM[255], 0);
    }

    #[test]
    fn what_was_written_is_read_back_at_every_probability() {
        let mut w = Writer::new();
        let mut expect = Vec::new();
        let mut seed = 12345u32;
        for i in 0..20_000u32 {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let prob = ((seed >> 16) & 0xFF).max(1) as u8;
            let bit = (seed >> 9) & 1 ^ u32::from(i % 7 == 0);
            w.write(bit, prob);
            expect.push((bit, prob));
        }
        let data = w.finish();
        let mut r = BoolReader::new(&data).unwrap();
        for (i, &(bit, prob)) in expect.iter().enumerate() {
            assert_eq!(r.read(prob), bit, "decision {i}");
        }
        assert!(!r.has_error());
    }

    #[test]
    fn a_set_marker_bit_is_refused() {
        // A single 0xFF byte decodes a 1 first at even odds.
        assert!(BoolReader::new(&[0xFF]).is_err());
    }

    #[test]
    fn decoding_far_past_the_end_is_an_error_and_never_a_panic() {
        let mut w = Writer::new();
        for _ in 0..16 {
            w.write(1, 200);
        }
        let data = w.finish();
        let mut r = BoolReader::new(&data).unwrap();
        for _ in 0..16 {
            assert_eq!(r.read(200), 1);
        }
        assert!(!r.has_error(), "the stream's own padding is not an error");
        for _ in 0..10_000 {
            r.read(128);
        }
        assert!(r.has_error());
    }

    #[test]
    fn an_empty_partition_reads_zeros() {
        let mut r = BoolReader::new(&[]).unwrap();
        assert_eq!(r.read_literal(8), 0);
    }

    #[test]
    fn a_tree_walks_to_its_leaves() {
        // The three-symbol tree {-0, 2, -1, -2}.
        const TREE: [i8; 4] = [0, 2, -1, -2];
        let mut w = Writer::new();
        // Symbol 0: first decision 0. Symbol 2: decisions 1, 1.
        w.write(0, 100);
        w.write(1, 100);
        w.write(1, 50);
        w.write(1, 100);
        w.write(0, 50);
        let data = w.finish();
        let mut r = BoolReader::new(&data).unwrap();
        let probs = [100u8, 50];
        assert_eq!(r.read_tree(&TREE, &probs), 0);
        assert_eq!(r.read_tree(&TREE, &probs), 2);
        assert_eq!(r.read_tree(&TREE, &probs), 1);
    }
}
