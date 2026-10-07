//! The range encoder: liblzma's `range_encoder.h`.
//!
//! liblzma's encoder does not encode a bit when it is asked to. `rc_bit` and
//! its kin *queue* the bit with a pointer to its probability, and `rc_encode`
//! codes the queue -- and updates the probabilities -- at the top of the next
//! pass of the encoding loop. Between the two, while one symbol is being
//! queued, the length coder may refresh its price table
//! (`length_update_prices`) from probabilities that do not yet count this
//! symbol's bits. The prices steer the encoder's next choices, so a range
//! encoder that updated as it went would choose differently and write other
//! bytes. The queue is kept: probabilities are named by their index in the
//! model's one array, and [`RangeEncoder::encode`] is handed the array.

// `low` holds 33 bits at most, `cache_size` counts pending bytes, and a
// probability is below 2048: liblzma's arithmetic, with no overflow in it.
#![allow(clippy::arithmetic_side_effects)]

use alloc::vec::Vec;

use crate::lzma::{BIT_MODEL_TOTAL, BIT_MODEL_TOTAL_BITS, MOVE_BITS, RC_TOP};

/// `RC_SYMBOLS_MAX`: a match with a long distance and length, then a flush.
const SYMBOLS_MAX: usize = 58;

/// `RC_SHIFT_BITS`
const SHIFT_BITS: u32 = 8;

/// One queued symbol (liblzma's `RC_BIT_0` ... `RC_FLUSH`).
#[derive(Debug, Clone, Copy)]
enum Symbol {
    /// A bit coded with the probability at this index of the model.
    Bit { prob: usize, one: bool },
    /// A bit with even odds.
    Direct { one: bool },
    /// One of the five flushes that end a stream.
    Flush,
}

/// `lzma_range_encoder`
pub(crate) struct RangeEncoder {
    low: u64,
    cache_size: u64,
    range: u32,
    cache: u8,
    queue: Vec<Symbol>,
}

impl RangeEncoder {
    /// A reset encoder (`rc_reset`).
    pub(crate) fn new() -> Self {
        Self {
            low: 0,
            cache_size: 1,
            range: u32::MAX,
            cache: 0,
            queue: Vec::with_capacity(SYMBOLS_MAX),
        }
    }

    /// `rc_reset`
    pub(crate) fn reset(&mut self) {
        self.low = 0;
        self.cache_size = 1;
        self.range = u32::MAX;
        self.cache = 0;
        self.queue.clear();
    }

    /// `rc_bit`: queues a bit coded with the probability at `prob`.
    pub(crate) fn bit(&mut self, prob: usize, bit: u32) {
        self.queue.push(Symbol::Bit {
            prob,
            one: bit != 0,
        });
    }

    /// `rc_bittree`: `symbol`'s low `bit_count` bits, high bit first, through
    /// the tree whose root is at `base + 1`.
    pub(crate) fn bittree(&mut self, base: usize, bit_count: u32, symbol: u32) {
        let mut model_index = 1usize;
        for i in (0..bit_count).rev() {
            let b = (symbol >> i) & 1;
            self.bit(base.wrapping_add(model_index), b);
            model_index = (model_index << 1) | b as usize;
        }
    }

    /// `rc_bittree_reverse`: `symbol`'s low `bit_count` bits, low bit first,
    /// through the tree whose root is at `base + 1`.
    pub(crate) fn bittree_reverse(&mut self, base: usize, bit_count: u32, symbol: u32) {
        let mut model_index = 1usize;
        let mut symbol = symbol;
        for _ in 0..bit_count {
            let b = symbol & 1;
            symbol >>= 1;
            self.bit(base.wrapping_add(model_index), b);
            model_index = (model_index << 1) | b as usize;
        }
    }

    /// `rc_direct`: `value`'s low `bit_count` bits, high bit first, at even
    /// odds.
    pub(crate) fn direct(&mut self, value: u32, bit_count: u32) {
        for i in (0..bit_count).rev() {
            self.queue.push(Symbol::Direct {
                one: (value >> i) & 1 != 0,
            });
        }
    }

    /// `rc_flush`: queues the five shifts that write out what is left.
    pub(crate) fn flush(&mut self) {
        for _ in 0..5 {
            self.queue.push(Symbol::Flush);
        }
    }

    /// `rc_pending`: the bytes the encoder still holds -- how much a flush
    /// would add to the output.
    pub(crate) const fn pending(&self) -> u64 {
        self.cache_size.wrapping_add(5 - 1)
    }

    /// `rc_shift_low`
    fn shift_low(&mut self, out: &mut Vec<u8>) {
        if (self.low as u32) < 0xff00_0000 || (self.low >> 32) != 0 {
            // A carry out of `low` lands on the cached byte and turns every
            // pending 0xFF after it into 0x00.
            let carry = (self.low >> 32) as u8;
            loop {
                out.push(self.cache.wrapping_add(carry));
                self.cache = 0xff;
                self.cache_size = self.cache_size.wrapping_sub(1);
                if self.cache_size == 0 {
                    break;
                }
            }
            self.cache = ((self.low >> 24) & 0xff) as u8;
        }
        self.cache_size = self.cache_size.wrapping_add(1);
        self.low = (self.low & 0x00ff_ffff) << SHIFT_BITS;
    }

    /// `rc_encode`: codes the queue into `out`, updating the probabilities
    /// in `probs` as it goes. A flush writes the encoder's last bytes and
    /// leaves it reset for the next LZMA2 chunk.
    pub(crate) fn encode(&mut self, probs: &mut [u16], out: &mut Vec<u8>) {
        let queue = core::mem::take(&mut self.queue);
        for (i, &symbol) in queue.iter().enumerate() {
            if self.range < RC_TOP {
                self.shift_low(out);
                self.range <<= SHIFT_BITS;
            }
            match symbol {
                Symbol::Bit { prob, one } => {
                    let Some(p) = probs.get_mut(prob) else {
                        continue;
                    };
                    let pv = u32::from(*p);
                    // `bound` is below the range: a probability is below 2048.
                    let bound = (self.range >> BIT_MODEL_TOTAL_BITS).wrapping_mul(pv);
                    if one {
                        self.low = self.low.wrapping_add(u64::from(bound));
                        self.range = self.range.wrapping_sub(bound);
                        *p = (pv - (pv >> MOVE_BITS)) as u16;
                    } else {
                        self.range = bound;
                        *p = pv.wrapping_add((BIT_MODEL_TOTAL - pv) >> MOVE_BITS) as u16;
                    }
                }
                Symbol::Direct { one } => {
                    self.range >>= 1;
                    if one {
                        self.low = self.low.wrapping_add(u64::from(self.range));
                    }
                }
                Symbol::Flush => {
                    // No more normalisation: the rest of the queue is
                    // flushes, each a shift.
                    self.range = u32::MAX;
                    for _ in i..queue.len() {
                        self.shift_low(out);
                    }
                    self.reset();
                    self.queue = queue;
                    self.queue.clear();
                    return;
                }
            }
        }
        self.queue = queue;
        self.queue.clear();
    }
}
