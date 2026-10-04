//! The bool decoder: VP8's arithmetic decoder, which reads everything in a
//! frame after its first three (or, for a key frame, ten) bytes.
//!
//! Each call decodes one binary decision against a probability (out of 256)
//! that the decision is 0. Trees of such decisions decode the multi-way
//! symbols: modes, segment numbers, motion vector components.
//!
//! This is libvpx's `BOOL_DECODER` translated as it is, 64-bit window and
//! all, rather than a reader written to the specification, because two of its
//! behaviours are observable and a different reader would differ:
//!
//! - Past the end of its data it shifts in zeros and keeps decoding, as if
//!   the partition were padded with zero bytes. Valid streams rely on it at
//!   their tail.
//! - [`BoolDecoder::has_error`] says when decoding has gone *beyond* that
//!   tail. libvpx marks a frame corrupt by it, skips the coefficients of a
//!   macroblock whose partition has run dry, and keeps the previous token
//!   partition count when the count itself cannot be read.
//!
//! [`BoolDecoder::read_signed`] is the coefficient decoder's sign, which
//! libvpx takes from libwebp: it halves the range rather than splitting it
//! by a probability of 128, and differs from [`BoolDecoder::read_bit`] in
//! the low bits of what it leaves behind.
//!
//! Translated into Rust from libvpx v1.17.0's `vp8/decoder/dboolhuff.c`,
//! `vp8/decoder/dboolhuff.h`, `vp8/decoder/treereader.h` and the `GetSigned`
//! of `vp8/decoder/detokenize.c` (copyright the WebM project authors), used
//! under libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

use crate::tables::NORM;

/// The width of the window, `VP8_BD_VALUE_SIZE`: libvpx's on a 64-bit
/// machine, where the test vectors' fingerprints were made.
const WINDOW_BITS: i32 = 64;

/// libvpx's `VP8_LOTS_OF_BITS`: added to the bit count when the data runs
/// out, so that the count can say whether reads have gone past it.
const LOTS_OF_BITS: i32 = 0x4000_0000;

/// A tree of binary decisions: libvpx's `vp8_tree_index` arrays. Index pairs
/// name the next node (positive, even) or a leaf (its symbol negated, or 0).
pub(crate) type Tree = [i8];

/// The arithmetic decoder over one partition of a frame.
#[derive(Clone, Debug)]
pub(crate) struct BoolDecoder<'a> {
    data: &'a [u8],
    /// How many bytes of `data` have gone into `value`: libvpx's
    /// `user_buffer`, as an offset.
    pos: usize,
    /// The window: the next bits of the partition, top-aligned.
    value: u64,
    /// How many bits of the window beyond the top byte hold data, less 8;
    /// below 0, the window needs filling. [`LOTS_OF_BITS`] is added once the
    /// data has run out.
    count: i32,
    /// The current interval's width, 128 to 255 between decisions (256 just
    /// after [`BoolDecoder::read_signed`]).
    range: u32,
}

impl<'a> BoolDecoder<'a> {
    /// A decoder over `data`: libvpx's `vp8dx_start_decode`. Empty data is
    /// allowed and decodes as zeros, as libvpx's does for a missing
    /// partition.
    pub(crate) fn new(data: &'a [u8]) -> Self {
        let mut d = Self {
            data,
            pos: 0,
            value: 0,
            count: -8,
            range: 255,
        };
        d.fill();
        d
    }

    /// Load bytes into the window until it holds as many as fit, or the data
    /// runs out: libvpx's `vp8dx_bool_decoder_fill`.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "`count` stays below LOTS_OF_BITS plus the window, and shifts and counts are below 64; byte counts are taken in i64, which no slice length overflows"
    )]
    fn fill(&mut self) {
        let mut shift = WINDOW_BITS - 8 - (self.count + 8);
        let bytes_left = self.data.len().saturating_sub(self.pos);
        // libvpx takes the bit count as an int; i64 gives the same answer for
        // every partition smaller than 256 MiB and a sane one beyond.
        let bits_left = i64::try_from(bytes_left)
            .unwrap_or(i64::MAX / 16)
            .saturating_mul(8);
        let x = i64::from(shift) + 8 - bits_left;
        let mut loop_end: i64 = 0;
        if x >= 0 {
            self.count += LOTS_OF_BITS;
            loop_end = x;
        }
        if x < 0 || bits_left != 0 {
            // At most `bytes_left` bytes: when the data runs out within the
            // window, `loop_end` stops the loop at its last byte.
            while i64::from(shift) >= loop_end {
                let Some(&byte) = self.data.get(self.pos) else {
                    break;
                };
                self.count += 8;
                self.value |= u64::from(byte) << shift;
                self.pos += 1;
                shift -= 8;
            }
        }
    }

    /// One decision, 1 with probability `(256 - prob) / 256`: libvpx's
    /// `vp8dx_decode_bool`.
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "range is 1 to 256 and prob below 256, so the split is 1 to 255 and never above range; the window shift is below 64; the table's index is masked to its 256 entries"
    )]
    #[inline]
    pub(crate) fn read(&mut self, prob: u8) -> bool {
        let split = 1 + (((self.range - 1) * u32::from(prob)) >> 8);
        if self.count < 0 {
            self.fill();
        }
        let bigsplit = u64::from(split) << (WINDOW_BITS - 8);
        let mut range = split;
        let mut value = self.value;
        let bit = value >= bigsplit;
        if bit {
            range = self.range - split;
            value -= bigsplit;
        }
        // `range` is 1 to 255 here, so its low byte is all of it.
        let shift = NORM[(range & 0xff) as usize];
        self.range = range << shift;
        self.value = value << shift;
        self.count -= i32::from(shift);
        bit
    }

    /// A decision at even odds: libvpx's `vp8_read_bit`.
    #[inline]
    pub(crate) fn read_bit(&mut self) -> bool {
        self.read(128)
    }

    /// `bits` (at most 8) bits at even odds, most significant first:
    /// libvpx's `vp8_decode_value`, for the widths VP8 uses.
    pub(crate) fn read_u8(&mut self, bits: u32) -> u8 {
        debug_assert!(bits <= 8);
        let mut z = 0u8;
        for bit in (0..bits.min(8)).rev() {
            z |= u8::from(self.read_bit()) << bit;
        }
        z
    }

    /// A coefficient's sign, applied to its magnitude `v`: libvpx's
    /// `GetSigned`, which halves the range instead of splitting it by a
    /// probability. On a damaged stream the window can overflow here; it
    /// wraps, as libvpx's does.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "range is 128 to 255 when a sign is read (each token read leaves it so), so the halves and the doubling stay within 256; the count stays above i32::MIN as each read adds back what it takes"
    )]
    #[inline]
    pub(crate) fn read_signed(&mut self, v: i32) -> i32 {
        let split = (self.range + 1) >> 1;
        let bigsplit = u64::from(split) << (WINDOW_BITS - 8);
        if self.count < 0 {
            self.fill();
        }
        let signed = if self.value < bigsplit {
            self.range = split;
            v
        } else {
            self.range -= split;
            self.value -= bigsplit;
            -v
        };
        self.range += self.range;
        self.value = self.value.wrapping_add(self.value);
        self.count -= 1;
        signed
    }

    /// Whether decoding has read past the end of the data: libvpx's
    /// `vp8dx_bool_error`.
    pub(crate) fn has_error(&self) -> bool {
        self.count > WINDOW_BITS && self.count < LOTS_OF_BITS
    }

    /// A symbol coded as a path through `tree`, whose node `i` is decided
    /// with probability `probs[i / 2]`: libvpx's `vp8_treed_read`.
    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "the trees and probability arrays are libvpx's constants, whose nodes index within the tree and whose halves index within the probabilities; a leaf is never below -127"
    )]
    pub(crate) fn read_tree(&mut self, tree: &Tree, probs: &[u8]) -> u8 {
        let mut i: i8 = 0;
        loop {
            let node = i as usize;
            let bit = usize::from(self.read(probs[node >> 1]));
            i = tree[node + bit];
            if i <= 0 {
                return (-i) as u8;
            }
        }
    }
}

/// A bool encoder, for tests that need streams whose decisions are known.
#[cfg(test)]
pub(crate) mod test_encoder {
    #![allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "a test helper: a failure should be loud"
    )]

    /// The bool encoder of RFC 6386 section 7.3, to make streams whose
    /// decisions are known.
    pub(crate) struct Encoder {
        out: Vec<u8>,
        range: u32,
        bottom: u32,
        bit_count: i32,
    }

    impl Encoder {
        pub(crate) fn new() -> Self {
            Self {
                out: Vec::new(),
                range: 255,
                bottom: 0,
                bit_count: 24,
            }
        }

        fn add_one_to_output(&mut self) {
            let mut i = self.out.len();
            while i > 0 {
                i -= 1;
                if self.out[i] == 255 {
                    self.out[i] = 0;
                } else {
                    self.out[i] += 1;
                    break;
                }
            }
        }

        pub(crate) fn write(&mut self, prob: u8, bit: bool) {
            let split = 1 + (((self.range - 1) * u32::from(prob)) >> 8);
            if bit {
                self.bottom = self.bottom.wrapping_add(split);
                self.range -= split;
            } else {
                self.range = split;
            }
            while self.range < 128 {
                self.range <<= 1;
                if self.bottom & (1 << 31) != 0 {
                    self.add_one_to_output();
                }
                self.bottom <<= 1;
                self.bit_count -= 1;
                if self.bit_count == 0 {
                    self.out.push((self.bottom >> 24) as u8);
                    self.bottom &= (1 << 24) - 1;
                    self.bit_count = 8;
                }
            }
        }

        pub(crate) fn finish(mut self) -> Vec<u8> {
            for _ in 0..32 {
                self.write(128, false);
            }
            self.out
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "a test: a failure should be loud"
    )]

    use super::BoolDecoder;
    use super::test_encoder::Encoder;

    fn decisions(n: usize) -> Vec<(u8, bool)> {
        // A fixed pseudo-random mix of probabilities and outcomes.
        let mut s = 0x1234_5678_u32;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let prob = ((s >> 8) & 0xff) as u8;
                let bit = (s >> 20) & 0xff > u32::from(prob);
                (prob, bit)
            })
            .collect()
    }

    #[test]
    fn decisions_come_back_as_they_were_written() {
        let want = decisions(10_000);
        let mut e = Encoder::new();
        for &(p, b) in &want {
            e.write(p, b);
        }
        let data = e.finish();
        let mut d = BoolDecoder::new(&data);
        for (i, &(p, b)) in want.iter().enumerate() {
            assert_eq!(d.read(p), b, "decision {i}");
        }
        assert!(!d.has_error());
    }

    #[test]
    fn a_literal_reads_most_significant_bit_first() {
        let mut e = Encoder::new();
        for bit in [true, false, true, true, false, false, true] {
            e.write(128, bit);
        }
        let data = e.finish();
        let mut d = BoolDecoder::new(&data);
        assert_eq!(d.read_u8(7), 0b101_1001);
    }

    #[test]
    fn empty_data_is_an_error_from_the_start_and_reads_zeros() {
        // libvpx's reading of a missing partition: no data at all is already
        // "past the end", and every decision is a 0.
        let mut d = BoolDecoder::new(&[]);
        assert!(d.has_error());
        for _ in 0..64 {
            assert!(!d.read_bit());
        }
        assert!(d.has_error());
    }

    #[test]
    fn reading_past_the_data_is_an_error_but_reading_to_it_is_not() {
        // Three bytes: the first is the decoder's top byte, and the other
        // sixteen bits can be shifted in before the padding starts. A
        // decision at even odds shifts by at most one bit.
        let data = [0x55u8, 0xaa, 0x55];
        let mut d = BoolDecoder::new(&data);
        assert!(!d.has_error());
        for _ in 0..16 {
            d.read_bit();
        }
        assert!(
            !d.has_error(),
            "sixteen decisions shift in at most the data"
        );
        let mut errored = false;
        for _ in 0..200 {
            d.read_bit();
            errored |= d.has_error();
        }
        assert!(errored, "reading far past three bytes must say so");
    }

    #[test]
    fn a_tree_reads_its_leaves() {
        // The uv mode tree: DC 0; V 10; H 110; TM 111.
        let tree = &crate::tables::UV_MODE_TREE;
        let probs = [128u8, 128, 128];
        for (bits, leaf) in [
            (&[false][..], 0u8),
            (&[true, false][..], 1),
            (&[true, true, false][..], 2),
            (&[true, true, true][..], 3),
        ] {
            let mut e = Encoder::new();
            for &b in bits {
                e.write(128, b);
            }
            let data = e.finish();
            let mut d = BoolDecoder::new(&data);
            assert_eq!(d.read_tree(tree, &probs), leaf);
        }
    }

    #[test]
    fn a_sign_halves_the_range() {
        // After a decision at the start of a stream of 0xff bytes, the sign
        // is negative: the window is above the half.
        let data = [0xffu8; 16];
        let mut d = BoolDecoder::new(&data);
        assert!(d.read(1));
        assert_eq!(d.read_signed(5), -5);
        let data = [0u8; 16];
        let mut d = BoolDecoder::new(&data);
        assert!(!d.read(255));
        assert_eq!(d.read_signed(5), 5);
    }
}
