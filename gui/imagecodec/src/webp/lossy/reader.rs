//! VP8's boolean entropy decoder (RFC 6386 §7), in libwebp's formulation.
//!
//! Every symbol in a VP8 frame -- header fields, modes, coefficients -- is a
//! sequence of binary decisions, each coded against a probability out of 256
//! by an arithmetic coder. The RFC's decoder keeps a two-byte window and shifts
//! it a bit at a time; this one keeps up to 56 bits loaded and moves a position
//! within them instead, as libwebp does, which is the same arithmetic with
//! fewer loads. The two are exact equivalents: each decision compares the top
//! eight bits of the unconsumed value against the same split, and consumes the
//! same number of bits.
//!
//! # Running out
//!
//! Past the end of its partition the RFC's decoder reads zeros, and so does
//! this one. What it also does is remember that it happened ([`Reader::eof`]),
//! at exactly the point libwebp does -- the first time a decision needs a byte
//! the partition does not have -- because that is how a decoder tells a
//! truncated file from a complete one, and the answer should be the same one
//! libwebp gives for the same file.
//!
//! # Streams no encoder writes
//!
//! A valid stream keeps the coder's value below its range. A corrupt one --
//! a partition whose first byte is `0xFF`, say -- need not, and from then on
//! every formulation of the decoder that is equivalent on valid streams can
//! give a different answer. This one keeps libwebp's on those too, down to
//! the details that decide them: a 64-bit value register that drops what is
//! shifted out of its top, seven-byte loads only while eight bytes remain, a
//! 32-bit window, and coefficient signs read by libwebp's `VP8GetSigned`
//! ([`Reader::sign`]). Each of these was found by decoding thousands of
//! corrupted files here and in libwebp and comparing the pixels
//! (design-decisions.md §1312); `tests/webp.rs` keeps one such file.

/// A boolean decoder over one partition.
pub(super) struct Reader<'a> {
    data: &'a [u8],
    /// Bytes of `data` not yet loaded into `value`.
    next: usize,
    /// Loaded bits not yet consumed. The eight bits the next decision reads
    /// start at bit `bits`.
    value: u64,
    /// How many loaded bits lie below the window the next decision reads;
    /// negative when that window is not yet full.
    bits: i32,
    /// The coder's range, minus one: 127..=254 between decisions.
    range: u32,
    /// Whether a decision has needed a byte past the end of the partition.
    eof: bool,
}

impl<'a> Reader<'a> {
    /// A decoder at the start of `data`.
    pub(super) fn new(data: &'a [u8]) -> Self {
        let mut reader = Self {
            data,
            next: 0,
            value: 0,
            bits: -8,
            range: 254,
            eof: false,
        };
        reader.load();
        reader
    }

    /// Whether the decoder has read past the end of its partition.
    pub(super) const fn eof(&self) -> bool {
        self.eof
    }

    /// Load more of the partition below what is left of `value`: seven bytes
    /// at a time while eight remain (libwebp reads eight and keeps seven),
    /// then one, then -- past the end -- a zero byte, noting the overrun.
    /// What a shift pushes out of the top of `value` is lost, as it is from
    /// libwebp's register; on a valid stream there is nothing there to lose.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "shifts are by 8 or 56, below 64; next and bits move by at most 7 bytes' worth"
    )]
    fn load(&mut self) {
        let eight = self
            .data
            .get(self.next..self.next.saturating_add(8))
            .and_then(|bytes| <[u8; 8]>::try_from(bytes).ok());
        if let Some(bytes) = eight {
            // The first seven of the eight.
            self.value = (self.value << 56) | (u64::from_be_bytes(bytes) >> 8);
            self.bits += 56;
            self.next += 7;
        } else if let Some(&byte) = self.data.get(self.next) {
            self.value = (self.value << 8) | u64::from(byte);
            self.bits += 8;
            self.next += 1;
        } else {
            self.value <<= 8;
            self.bits += 8;
            self.eof = true;
        }
    }

    /// One decision, `true` with probability `(256 - prob) / 256`.
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "bits is non-negative after a load; the window is libwebp's 32-bit one (below 256 on a valid stream); split is at most 253 and at most the range, and the new range at least 1, so its normalising shift is 0..=7"
    )]
    pub(super) fn bit(&mut self, prob: u8) -> bool {
        if self.bits < 0 {
            self.load();
        }
        let position = self.bits as u32;
        let split = (self.range * u32::from(prob)) >> 8;
        let window = (self.value >> position) as u32;
        let (bit, range) = if window > split {
            self.value = self.value.wrapping_sub(u64::from(split + 1) << position);
            (true, self.range - split)
        } else {
            (false, split + 1)
        };
        // Renormalise to 128..=255: shift by 7 - floor(log2(range)).
        let shift = range.leading_zeros() - 24;
        self.range = (range << shift) - 1;
        self.bits -= shift as i32;
        bit
    }

    /// A coefficient's sign, `true` for negative: an even-odds decision, read
    /// as libwebp's `VP8GetSigned` reads it. On a valid stream that is exactly
    /// [`Reader::bit`]`(128)`; on a corrupt one whose window has run past the
    /// range by more than 2^31, libwebp's subtraction wraps and reads `false`,
    /// and so does this.
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::arithmetic_side_effects,
        reason = "bits is non-negative after a load; the window is libwebp's 32-bit one; the decrement of bits is libwebp's fixed one-bit shift"
    )]
    pub(super) fn sign(&mut self) -> bool {
        if self.bits < 0 {
            self.load();
        }
        let position = self.bits as u32;
        let split = self.range >> 1;
        let window = (self.value >> position) as u32;
        let negative = (split.wrapping_sub(window) as i32) < 0;
        self.bits -= 1;
        if negative {
            self.range = self.range.wrapping_sub(1) | 1;
            self.value = self.value.wrapping_sub(u64::from(split + 1) << position);
        } else {
            self.range |= 1;
        }
        negative
    }

    /// An even-odds decision: a flag in the frame header.
    pub(super) fn flag(&mut self) -> bool {
        self.bit(128)
    }

    /// An `n`-bit unsigned field, most significant bit first (the RFC's
    /// `L(n)`).
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "n is at most 8 wherever this is called, so the value stays below 2^8"
    )]
    pub(super) fn literal(&mut self, n: u32) -> u32 {
        let mut value = 0u32;
        for _ in 0..n {
            value = (value << 1) | u32::from(self.flag());
        }
        value
    }

    /// An `n`-bit magnitude followed by a sign bit, `1` meaning negative.
    #[allow(
        clippy::cast_possible_wrap,
        clippy::arithmetic_side_effects,
        reason = "n is at most 7, so the magnitude is below 128 and negating it cannot overflow"
    )]
    pub(super) fn signed(&mut self, n: u32) -> i32 {
        let magnitude = self.literal(n) as i32;
        if self.flag() { -magnitude } else { magnitude }
    }

    /// A signed field that is only present if a flag before it says so; zero
    /// otherwise.
    pub(super) fn optional_signed(&mut self, n: u32) -> i32 {
        if self.flag() { self.signed(n) } else { 0 }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    /// The RFC's boolean encoder (§7.3), to make streams whose every decision
    /// is known.
    struct Writer {
        out: Vec<u8>,
        range: u32,
        bottom: u32,
        bit_count: i32,
    }

    impl Writer {
        fn new() -> Self {
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

        fn write(&mut self, prob: u8, bit: bool) {
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

        fn finish(mut self) -> Vec<u8> {
            for _ in 0..32 {
                self.write(128, false);
            }
            self.out
        }
    }

    /// A deterministic spread of (probability, decision) pairs.
    fn script(n: usize) -> Vec<(u8, bool)> {
        let mut state = 0x1234_5678u32;
        (0..n)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let prob = (state >> 16) as u8;
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                // Decisions follow their probability, roughly, so both the
                // likely and the unlikely branch are exercised at every odds.
                let bit = ((state >> 16) & 0xFF) as u8 >= prob;
                (prob.max(1), bit)
            })
            .collect()
    }

    #[test]
    fn it_reads_back_what_the_rfcs_encoder_wrote() {
        let decisions = script(5000);
        let mut writer = Writer::new();
        for &(prob, bit) in &decisions {
            writer.write(prob, bit);
        }
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes);
        for (i, &(prob, bit)) in decisions.iter().enumerate() {
            assert_eq!(reader.bit(prob), bit, "decision {i}");
        }
        assert!(!reader.eof(), "the encoder's flush covers every decision");
    }

    #[test]
    fn a_sign_is_an_even_odds_decision_on_a_valid_stream() {
        let decisions = script(3000);
        let mut writer = Writer::new();
        for (i, &(prob, bit)) in decisions.iter().enumerate() {
            // Every third decision is a sign, at even odds.
            writer.write(if i % 3 == 2 { 128 } else { prob }, bit);
        }
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes);
        for (i, &(prob, bit)) in decisions.iter().enumerate() {
            let got = if i % 3 == 2 {
                reader.sign()
            } else {
                reader.bit(prob)
            };
            assert_eq!(got, bit, "decision {i}");
        }
    }

    #[test]
    fn literals_and_signed_fields_read_most_significant_bit_first() {
        let mut writer = Writer::new();
        for bit in [true, false, true, true, false, false, true] {
            writer.write(128, bit);
        }
        // A signed field: magnitude 5 in four bits, then the sign, negative.
        for bit in [false, true, false, true, true] {
            writer.write(128, bit);
        }
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.literal(7), 0b101_1001);
        assert_eq!(reader.signed(4), -5);
    }

    #[test]
    fn running_out_is_noticed_and_reads_zeros() {
        // An empty partition has nothing to give from the first decision on.
        let mut empty = Reader::new(&[]);
        assert!(empty.eof(), "libwebp flags an empty partition at once");
        assert!(!empty.bit(128));

        // A stream of decisions whose bytes are then cut: the reader carries
        // on (with zeros) and says so, rather than stopping short.
        let decisions = script(400);
        let mut writer = Writer::new();
        for &(prob, bit) in &decisions {
            writer.write(prob, bit);
        }
        let bytes = writer.finish();
        let cut = &bytes[..bytes.len() / 2];
        let mut reader = Reader::new(cut);
        for &(prob, _) in &decisions {
            reader.bit(prob);
        }
        assert!(reader.eof());
    }

    #[test]
    fn the_overrun_is_flagged_where_libwebp_flags_it() {
        // libwebp notices when a decision starts with the window short of
        // eight bits and no byte left to fill it: once more than 8*(n-1) bits
        // have been shifted out of an n-byte partition. An even-odds decision
        // shifts out exactly one bit -- except the first, from the initial
        // full range, which shifts out one on a `true` and none on a `false`
        // -- so the decision that raises the flag is known exactly.
        for len in 1..=20usize {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let mut reader = Reader::new(&bytes);
            let first = reader.bit(128);
            let mut decisions = 1usize;
            while !reader.eof() {
                reader.bit(128);
                decisions += 1;
                assert!(decisions < 1000);
            }
            let want = 8 * (len - 1) + 3 - usize::from(first);
            assert_eq!(decisions, want, "{len} bytes, first decision {first}");
        }
    }
}
