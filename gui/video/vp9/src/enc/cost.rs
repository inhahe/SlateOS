//! What coding costs: the bits a decision takes at a probability, in 1/512ths
//! of a bit, the unit every rate the encoder weighs is measured in.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_cost.c` and
//! `vp9_cost.h` (copyright the WebM project authors), used under libvpx's BSD
//! licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

use crate::tables::PROB_COST;

/// Costs are in units of 1/512 bit: libvpx's `VP9_PROB_COST_SHIFT`.
pub(crate) const PROB_COST_SHIFT: u32 = 9;

/// The cost of a 0 at probability `p` (of a 0, out of 256): libvpx's
/// `vp9_cost_zero`.
#[allow(clippy::indexing_slicing, reason = "a u8 indexes a 256-entry table")]
pub(crate) fn cost_zero(p: u8) -> u32 {
    u32::from(PROB_COST[usize::from(p)])
}

/// The cost of a 1 at probability `p`: libvpx's `vp9_cost_one`, the cost of
/// a 0 at `256 - p`. A probability is never 0; if one were, this reads the
/// table's placeholder rather than past its end.
pub(crate) fn cost_one(p: u8) -> u32 {
    cost_zero(p.wrapping_neg())
}

/// The cost of `ct[0]` zeros and `ct[1]` ones at probability `p`: libvpx's
/// `cost_branch256`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "u32 counts times costs below 2^13 fit u64 twice over"
)]
pub(crate) fn cost_branch256(ct: [u32; 2], p: u8) -> u64 {
    u64::from(ct[0]) * u64::from(cost_zero(p)) + u64::from(ct[1]) * u64::from(cost_one(p))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a test: a failure should be loud"
    )]

    use super::*;

    /// The table is -log2(p / 256) in 1/512 bits, rounded: checked against
    /// the formula, so a table that came out wrong cannot pass.
    #[test]
    fn costs_are_minus_log2_of_the_probability() {
        for p in 1u8..=255 {
            let want = (-(f64::from(p) / 256.0).log2() * 512.0).round() as u32;
            assert_eq!(cost_zero(p), want, "p = {p}");
        }
    }

    #[test]
    fn a_one_costs_a_zero_at_the_other_probability() {
        assert_eq!(cost_one(1), cost_zero(255));
        assert_eq!(cost_one(128), 512);
        assert_eq!(cost_branch256([3, 2], 128), 5 * 512);
    }
}
