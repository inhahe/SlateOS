//! The two writers a VP9 frame is made with: the bool encoder, VP9's
//! arithmetic coder, for everything after the uncompressed header; and a
//! plain bit writer for the uncompressed header itself.
//!
//! The bool encoder is the exact inverse of [`crate::boolread::BoolReader`]:
//! each call codes one binary decision against a probability (out of 256)
//! that it is 0, and trees of decisions code the multi-way symbols.
//!
//! Translated into Rust from libvpx v1.17.0's `vpx_dsp/bitwriter.c`,
//! `vpx_dsp/bitwriter.h`, `vpx_dsp/bitwriter_buffer.c` and
//! `vp9/encoder/vp9_treewriter.c` (copyright the WebM project authors), used
//! under libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "the coder's state is a 24-bit low value and an 8-bit range, shifted by at most 7; counts are bounded by the stream's length"
)]

use crate::boolread::Tree;

/// libvpx's `vpx_writer`: the bool encoder.
///
/// libvpx writes into a buffer of fixed size and records an error if it
/// would overflow; this one grows its buffer instead, so it never fails.
#[derive(Debug)]
pub(crate) struct BoolWriter {
    out: Vec<u8>,
    low: u32,
    range: u32,
    count: i32,
}

impl BoolWriter {
    /// libvpx's `vpx_start_encode`: begins with the marker bit, a 0.
    pub(crate) fn new() -> Self {
        let mut w = Self {
            out: Vec::new(),
            low: 0,
            range: 255,
            count: -24,
        };
        w.write(false, 128);
        w
    }

    /// libvpx's `vpx_write`: one decision, `bit`, against `prob`, the
    /// probability out of 256 that it is 0.
    pub(crate) fn write(&mut self, bit: bool, prob: u8) {
        let split = 1 + (((self.range - 1) * u32::from(prob)) >> 8);
        let mut range = split;
        let mut low = self.low;
        if bit {
            low += split;
            range = self.range - split;
        }
        // libvpx's vpx_norm: leading zeros of the 8-bit range.
        let mut shift = (range as u8).leading_zeros() as i32;
        range <<= shift;
        let mut count = self.count + shift;
        if count >= 0 {
            let offset = shift - count;
            if (low << (offset - 1)) & 0x8000_0000 != 0 {
                // Carry into the bytes already written. libvpx's own
                // comment asks how to prove one exists to take it: the
                // marker bit, a 0 at probability one half, guarantees the
                // first byte is below 0x80, so a carry always stops there.
                for b in self.out.iter_mut().rev() {
                    if *b == 0xff {
                        *b = 0;
                    } else {
                        *b += 1;
                        break;
                    }
                }
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

    /// libvpx's `vpx_write_bit`: a decision at probability one half.
    pub(crate) fn write_bit(&mut self, bit: bool) {
        self.write(bit, 128);
    }

    /// libvpx's `vpx_write_literal`: `bits` bits of `data`, most
    /// significant first.
    pub(crate) fn write_literal(&mut self, data: u32, bits: u32) {
        for b in (0..bits).rev() {
            self.write_bit((data >> b) & 1 != 0);
        }
    }

    /// libvpx's `vp9_write_tree`: the `len` low bits of `bits`, most
    /// significant first, down `tree` from node `i`.
    pub(crate) fn write_tree(&mut self, tree: &Tree, probs: &[u8], bits: u32, len: u32, i: usize) {
        let mut i = i;
        for b in (0..len).rev() {
            let bit = (bits >> b) & 1;
            self.write(bit != 0, probs.get(i >> 1).copied().unwrap_or(128));
            let next = tree.get(i + bit as usize).copied().unwrap_or(0);
            if next <= 0 {
                break;
            }
            i = next as usize;
        }
    }

    /// libvpx's `vp9_write_token`: a symbol, as `tokens_from_tree` codes it.
    pub(crate) fn write_token(&mut self, tree: &Tree, probs: &[u8], token: Token) {
        self.write_tree(tree, probs, token.value, token.len, 0);
    }

    /// libvpx's `vpx_stop_encode`: 32 more zero bits to flush the coder,
    /// then a zero byte if the last would look like a superframe index's
    /// marker.
    pub(crate) fn finish(mut self) -> Vec<u8> {
        for _ in 0..32 {
            self.write_bit(false);
        }
        if self.out.last().is_some_and(|&b| b & 0xe0 == 0xc0) {
            self.out.push(0);
        }
        self.out
    }
}

/// A symbol's code in a tree: libvpx's `vp9_token`, `len` bits of `value`
/// taken from the most significant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Token {
    pub value: u32,
    pub len: u32,
}

/// libvpx's `vp9_tokens_from_tree`: every leaf's code. `n` is the number of
/// symbols (leaves). The writers use the codes as constants; tests check
/// them against the trees with this.
#[cfg(test)]
pub(crate) fn tokens_from_tree(tree: &Tree, n: usize) -> Vec<Token> {
    let mut tokens = vec![Token::default(); n];
    tree2tok(&mut tokens, tree, 0, 0, 0);
    tokens
}

#[cfg(test)]
fn tree2tok(tokens: &mut [Token], tree: &Tree, mut i: usize, v: u32, l: u32) {
    let mut v = v + v;
    let l = l + 1;
    loop {
        let j = tree.get(i).copied().unwrap_or(0);
        i += 1;
        if j <= 0 {
            if let Some(t) = tokens.get_mut(j.unsigned_abs() as usize) {
                *t = Token { value: v, len: l };
            }
        } else {
            tree2tok(tokens, tree, j as usize, v, l);
        }
        v += 1;
        if v & 1 == 0 {
            break;
        }
    }
}

/// libvpx's `vp9_tree_probs_from_distribution`: from how often each symbol
/// occurred, how often each node of `tree` went each way.
pub(crate) fn tree_branch_counts(tree: &Tree, num_events: &[u32], branch_ct: &mut [[u32; 2]]) {
    convert_distribution(0, tree, branch_ct, num_events);
}

fn convert_distribution(i: usize, tree: &Tree, branch_ct: &mut [[u32; 2]], events: &[u32]) -> u32 {
    let side = |node: i8, branch_ct: &mut [[u32; 2]]| -> u32 {
        if node <= 0 {
            events
                .get(node.unsigned_abs() as usize)
                .copied()
                .unwrap_or(0)
        } else {
            convert_distribution(node as usize, tree, branch_ct, events)
        }
    };
    let left = side(tree.get(i).copied().unwrap_or(0), branch_ct);
    let right = side(tree.get(i + 1).copied().unwrap_or(0), branch_ct);
    if let Some(ct) = branch_ct.get_mut(i >> 1) {
        *ct = [left, right];
    }
    left.wrapping_add(right)
}

/// libvpx's `vpx_write_bit_buffer`: plain bits, most significant first, for
/// the uncompressed header.
#[derive(Debug, Default)]
pub(crate) struct BitWriter {
    out: Vec<u8>,
    bits: usize,
}

impl BitWriter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// libvpx's `vpx_wb_write_bit`.
    pub(crate) fn write_bit(&mut self, bit: bool) {
        let q = 7 - (self.bits % 8);
        if q == 7 {
            self.out.push(0);
        }
        if bit && let Some(last) = self.out.last_mut() {
            *last |= 1 << q;
        }
        self.bits += 1;
    }

    /// libvpx's `vpx_wb_write_literal`: `bits` bits of `data`.
    pub(crate) fn write_literal(&mut self, data: u32, bits: u32) {
        for b in (0..bits).rev() {
            self.write_bit((data >> b) & 1 != 0);
        }
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.out
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::cast_possible_truncation,
        reason = "a test: a failure should be loud"
    )]

    use super::*;
    use crate::bits::BitReader;
    use crate::boolread::BoolReader;

    struct Lcg(u32);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            self.0 >> 8
        }
    }

    #[test]
    fn what_the_bool_writer_writes_the_reader_reads_back() {
        let mut rng = Lcg(0x0b00_1e40);
        let mut w = BoolWriter::new();
        let mut written = Vec::new();
        for _ in 0..20_000 {
            let prob = (rng.next() % 255 + 1) as u8;
            // Bits skewed toward the probability, as real streams are.
            let bit = (rng.next() % 256) >= u32::from(prob);
            w.write(bit, prob);
            written.push((bit, prob));
        }
        let data = w.finish();
        let mut r = BoolReader::new(&data).unwrap();
        for (i, &(bit, prob)) in written.iter().enumerate() {
            assert_eq!(r.read_bool(prob), bit, "decision {i}");
        }
        assert!(!r.has_error());
    }

    #[test]
    fn carries_run_back_through_ff_bytes() {
        // Long runs of the likelier symbol at extreme probabilities drive
        // the low value to the top and force carries through 0xff bytes.
        let mut w = BoolWriter::new();
        let mut written = Vec::new();
        for i in 0..4000u32 {
            let (bit, prob) = if i % 97 < 90 { (true, 1) } else { (false, 255) };
            w.write(bit, prob);
            written.push((bit, prob));
        }
        let data = w.finish();
        let mut r = BoolReader::new(&data).unwrap();
        for &(bit, prob) in &written {
            assert_eq!(r.read_bool(prob), bit);
        }
    }

    #[test]
    fn the_last_byte_never_looks_like_a_superframe_marker() {
        for seed in 0..200 {
            let mut rng = Lcg(seed);
            let mut w = BoolWriter::new();
            for _ in 0..(rng.next() % 64) {
                let p = (rng.next() % 255 + 1) as u8;
                w.write(rng.next() & 1 != 0, p);
            }
            let data = w.finish();
            assert_ne!(data.last().unwrap() & 0xe0, 0xc0);
        }
    }

    #[test]
    fn tree_symbols_round_trip() {
        // The partition tree: 4 symbols.
        let tree: &Tree = &[-0, 2, -1, 4, -2, -3];
        let tokens = tokens_from_tree(tree, 4);
        assert_eq!(
            tokens,
            vec![
                Token { value: 0, len: 1 },
                Token { value: 2, len: 2 },
                Token { value: 6, len: 3 },
                Token { value: 7, len: 3 },
            ]
        );
        let probs = [100u8, 60, 200];
        let mut w = BoolWriter::new();
        let symbols = [0usize, 3, 1, 2, 2, 0, 3];
        for &s in &symbols {
            w.write_token(tree, &probs, tokens[s]);
        }
        let data = w.finish();
        let mut r = BoolReader::new(&data).unwrap();
        for &s in &symbols {
            assert_eq!(r.read_tree(tree, &probs) as usize, s);
        }
    }

    #[test]
    fn branch_counts_add_up_the_symbols_under_each_node() {
        let tree: &Tree = &[-0, 2, -1, 4, -2, -3];
        let mut ct = [[0u32; 2]; 3];
        tree_branch_counts(tree, &[5, 7, 11, 13], &mut ct);
        assert_eq!(ct, [[5, 31], [7, 24], [11, 13]]);
    }

    #[test]
    fn the_bit_writer_writes_what_the_header_reader_reads() {
        let mut w = BitWriter::new();
        w.write_literal(2, 2);
        w.write_bit(true);
        w.write_literal(0x49_83_42, 24);
        // A delta as the header writes one: magnitude, then sign.
        w.write_literal(13, 6);
        w.write_bit(true);
        w.write_literal(9, 4);
        w.write_bit(false);
        let data = w.finish();
        assert_eq!(data.len(), 5);
        let mut r = BitReader::new(&data);
        assert_eq!(r.literal(2), 2);
        assert_eq!(r.bit(), 1);
        assert_eq!(r.literal(24), 0x49_83_42);
        assert_eq!(r.literal(6), 13);
        assert_eq!(r.bit(), 1);
        assert_eq!(r.literal(4), 9);
        assert_eq!(r.bit(), 0);
    }
}
