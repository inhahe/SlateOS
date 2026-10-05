//! Prices: what a bit, a bit-tree symbol or a run of direct bits costs to
//! encode, in sixteenths of a bit -- liblzma's `price.h`, `price_table.c` and
//! `fastpos.h`. The normal-mode encoder compares these sums to choose between
//! literals, matches and repeats; they are estimates, and the estimate is part
//! of the output, so they are liblzma's to the last entry.

// Prices are sums of a few hundred entries under 128.
#![allow(clippy::arithmetic_side_effects)]

use crate::lzma::BIT_MODEL_TOTAL;

/// `RC_INFINITY_PRICE`: a price no real choice reaches.
pub(crate) const INFINITY: u32 = 1 << 30;

/// `RC_MOVE_REDUCING_BITS`: the table has an entry per sixteen probabilities.
const MOVE_REDUCING_BITS: u32 = 4;

/// `RC_BIT_PRICE_SHIFT_BITS`: prices are in sixteenths of a bit.
const BIT_PRICE_SHIFT_BITS: u32 = 4;

/// `lzma_rc_prices`, as liblzma's `price_tablegen.c` generates it: the cost
/// of a 0 bit with probability `(i * 16 + 8) / 2048`, in sixteenths of a bit.
/// `the_price_table_is_price_tablegens` regenerates it.
const PRICES: [u8; 128] = [
    128, 103, 91, 84, 78, 73, 69, 66, 63, 61, 58, 56, 54, 52, 51, 49, //
    48, 46, 45, 44, 43, 42, 41, 40, 39, 38, 37, 36, 35, 34, 34, 33, //
    32, 31, 31, 30, 29, 29, 28, 28, 27, 26, 26, 25, 25, 24, 24, 23, //
    23, 22, 22, 22, 21, 21, 20, 20, 19, 19, 19, 18, 18, 17, 17, 17, //
    16, 16, 16, 15, 15, 15, 14, 14, 14, 13, 13, 13, 12, 12, 12, 11, //
    11, 11, 11, 10, 10, 10, 10, 9, 9, 9, 9, 8, 8, 8, 8, 7, //
    7, 7, 7, 6, 6, 6, 6, 5, 5, 5, 5, 5, 4, 4, 4, 4, //
    3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, //
];

/// The table entry for a probability already flipped for the bit: a
/// probability is below 2048, so its top seven bits index the table.
fn lookup(flipped: u32) -> u32 {
    // The index is masked to the table's 128 entries.
    #[allow(clippy::indexing_slicing)]
    let price = PRICES[((flipped >> MOVE_REDUCING_BITS) & 0x7f) as usize];
    u32::from(price)
}

/// `rc_bit_price`
pub(crate) fn bit(prob: u16, bit: u32) -> u32 {
    lookup(u32::from(prob) ^ (0u32.wrapping_sub(bit) & (BIT_MODEL_TOTAL - 1)))
}

/// `rc_bit_0_price`
pub(crate) fn bit0(prob: u16) -> u32 {
    lookup(u32::from(prob))
}

/// `rc_bit_1_price`
pub(crate) fn bit1(prob: u16) -> u32 {
    lookup(u32::from(prob) ^ (BIT_MODEL_TOTAL - 1))
}

/// The probability at `i` of a model slice; indices are bounded by the
/// tree sizes the callers pass.
fn at(probs: &[u16], i: u32) -> u16 {
    probs.get(i as usize).copied().unwrap_or(0)
}

/// `rc_bittree_price`: `symbol` through the tree whose root is `probs[1]`.
pub(crate) fn bittree(probs: &[u16], bit_levels: u32, symbol: u32) -> u32 {
    let mut price = 0u32;
    let mut symbol = symbol.wrapping_add(1 << bit_levels);
    loop {
        let b = symbol & 1;
        symbol >>= 1;
        price = price.wrapping_add(bit(at(probs, symbol), b));
        if symbol == 1 {
            return price;
        }
    }
}

/// `rc_bittree_reverse_price`: `symbol`, low bit first, through the tree
/// whose root is `probs[1]`.
pub(crate) fn bittree_reverse(probs: &[u16], bit_levels: u32, symbol: u32) -> u32 {
    let mut price = 0u32;
    let mut model_index = 1u32;
    let mut symbol = symbol;
    for _ in 0..bit_levels {
        let b = symbol & 1;
        symbol >>= 1;
        price = price.wrapping_add(bit(at(probs, model_index), b));
        model_index = (model_index << 1) | b;
    }
    price
}

/// `rc_direct_price`: direct bits cost a bit each.
pub(crate) const fn direct(bits: u32) -> u32 {
    bits << BIT_PRICE_SHIFT_BITS
}

/// `get_dist_slot`: the six-bit slot of a match distance -- its two highest
/// bits and their position (`fastpos.h`, the `HAVE_SMALL` form, which is the
/// table form's definition).
pub(crate) const fn dist_slot(dist: u32) -> u32 {
    if dist <= 4 {
        dist
    } else {
        // dist > 4, so its top bit is at 2 or above.
        let i = dist.ilog2();
        (i + i) + ((dist >> (i - 1)) & 1)
    }
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use alloc::vec;

    /// `price_tablegen.c`'s `init_price_table`, run here: the table is the
    /// generator's.
    #[test]
    fn the_price_table_is_price_tablegens() {
        const CYCLES_BITS: u32 = BIT_PRICE_SHIFT_BITS;
        let mut i = (1u32 << MOVE_REDUCING_BITS) / 2;
        let mut n = 0;
        while i < BIT_MODEL_TOTAL {
            let mut w = i;
            let mut bit_count = 0u32;
            for _ in 0..CYCLES_BITS {
                w *= w;
                bit_count <<= 1;
                while w >= 1 << 16 {
                    w >>= 1;
                    bit_count += 1;
                }
            }
            let price = (11 << CYCLES_BITS) - 15 - bit_count;
            assert_eq!(u32::from(PRICES[n]), price, "entry {n}");
            n += 1;
            i += 1 << MOVE_REDUCING_BITS;
        }
        assert_eq!(n, PRICES.len());
    }

    /// `fastpos_tablegen.c`'s table, and its `fastpos_result` past it.
    #[test]
    fn distance_slots_are_fastposs() {
        let mut table = vec![0u8; 1 << 13];
        table[1] = 1;
        let mut c = 2;
        for slot in 2u8..26 {
            for _ in 0..1 << ((slot >> 1) - 1) {
                table[c] = slot;
                c += 1;
            }
        }
        for (dist, &slot) in table.iter().enumerate() {
            assert_eq!(dist_slot(dist as u32), u32::from(slot), "{dist}");
        }
        for dist in [1u32 << 13, (1 << 13) + 5, 1 << 20, 3 << 25, u32::MAX] {
            let shift = if dist < 1 << 25 { 12 } else { 24 };
            let want = u32::from(table[(dist >> shift) as usize]) + 2 * shift;
            assert_eq!(dist_slot(dist), want, "{dist}");
        }
    }
}
