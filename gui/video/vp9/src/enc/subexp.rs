//! Probability updates: whether a frame's header should change one of the
//! probabilities it codes with, to what, and how the change is written.
//!
//! A frame may move any probability toward what its own symbols want, at the
//! price of coding the move. The encoder counts what each decision did,
//! searches for the probability that saves the most bits net of that price,
//! and writes it only if the saving is positive. The move itself is coded as
//! a recentred, remapped delta with a terminated sub-exponential code -- what
//! the decoder's `probs::diff_update_prob` reads.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_subexp.c` and
//! `vpx_dsp/prob.h` (copyright the WebM project authors), used under libvpx's
//! BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "probabilities are 1..=255 and their deltas below 255; costs are u32 counts times 13-bit costs, summed in i64 over at most eleven nodes"
)]

use crate::common::{PIVOT_NODE, UNCONSTRAINED_NODES};
use crate::enc::cost::{PROB_COST_SHIFT, cost_branch256, cost_one, cost_zero};
use crate::enc::writer::BoolWriter;
use crate::tables::{MAP_TABLE, PARETO8_FULL, UPDATE_BITS};

/// The probability that a probability is *not* updated: libvpx's
/// `DIFF_UPDATE_PROB`.
pub(crate) const DIFF_UPDATE_PROB: u8 = 252;

/// How many nodes the coefficient tree has: libvpx's `ENTROPY_NODES`. The
/// first [`UNCONSTRAINED_NODES`] have probabilities of their own; the rest
/// follow from the pivot's through the Pareto table.
pub(crate) const ENTROPY_NODES: usize = 11;

/// The fewest bits a probability update can take: libvpx's `MIN_DELP_BITS`.
const MIN_DELP_BITS: i64 = 5;

/// `num / den` as a probability out of 256, clipped to 1..=255: libvpx's
/// `get_prob`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "clipped to 1..=255 before narrowing"
)]
fn get_prob(num: u32, den: u32) -> u8 {
    if den == 0 {
        return 128;
    }
    let p = (u64::from(num) * 256 + u64::from(den >> 1)) / u64::from(den);
    p.clamp(1, 255) as u8
}

/// The probability of a 0 given `n0` zeros and `n1` ones: libvpx's
/// `get_binary_prob`. The total wraps as libvpx's unsigned sum does.
pub(crate) fn get_binary_prob(n0: u32, n1: u32) -> u8 {
    get_prob(n0, n0.wrapping_add(n1))
}

/// libvpx's `recenter_nonneg`.
fn recenter_nonneg(v: i32, m: i32) -> i32 {
    if v > (m << 1) {
        v
    } else if v >= m {
        (v - m) << 1
    } else {
        ((m - v) << 1) - 1
    }
}

/// The code of moving probability `m` to `v`: libvpx's `remap_prob`. The two
/// differ; libvpx asserts as much, and every caller guarantees it.
fn remap_prob(v: u8, m: u8) -> usize {
    let (v, m) = (i32::from(v) - 1, i32::from(m) - 1);
    let i = if (m << 1) <= 255 {
        recenter_nonneg(v, m) - 1
    } else {
        recenter_nonneg(255 - 1 - v, 255 - 1 - m) - 1
    };
    debug_assert!(i >= 0, "a probability update to the same probability");
    usize::try_from(i)
        .ok()
        .and_then(|i| MAP_TABLE.get(i))
        .map_or(0, |&d| usize::from(d))
}

/// What writing the move from `oldp` to `newp` costs: libvpx's
/// `prob_diff_update_cost`.
fn prob_diff_update_cost(newp: u8, oldp: u8) -> i64 {
    let delp = remap_prob(newp, oldp);
    i64::from(UPDATE_BITS.get(delp).copied().unwrap_or(0)) << PROB_COST_SHIFT
}

/// libvpx's `encode_uniform`.
#[allow(
    clippy::cast_sign_loss,
    reason = "v is at least 0: a remapped delta less 64"
)]
fn encode_uniform(w: &mut BoolWriter, v: i32) {
    let m = (1 << 8) - 191;
    if v < m {
        w.write_literal(v as u32, 7);
    } else {
        w.write_literal((m + ((v - m) >> 1)) as u32, 7);
        w.write_literal(((v - m) & 1) as u32, 1);
    }
}

/// libvpx's `encode_term_subexp`.
#[allow(
    clippy::cast_sign_loss,
    reason = "each literal is written only on the branch where it is non-negative"
)]
fn encode_term_subexp(w: &mut BoolWriter, word: i32) {
    let gte = |w: &mut BoolWriter, test: i32| {
        w.write_literal(u32::from(word >= test), 1);
        word >= test
    };
    if !gte(w, 16) {
        w.write_literal(word as u32, 4);
    } else if !gte(w, 32) {
        w.write_literal((word - 16) as u32, 4);
    } else if !gte(w, 64) {
        w.write_literal((word - 32) as u32, 5);
    } else {
        encode_uniform(w, word - 64);
    }
}

/// Write the move from `oldp` to `newp`: libvpx's
/// `vp9_write_prob_diff_update`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "a remapped delta is below 254"
)]
pub(crate) fn write_prob_diff_update(w: &mut BoolWriter, newp: u8, oldp: u8) {
    encode_term_subexp(w, remap_prob(newp, oldp) as i32);
}

/// The cost of signalling an update, net of not: libvpx's `upd_cost`.
fn update_flag_cost(upd: u8) -> i64 {
    i64::from(cost_one(upd)) - i64::from(cost_zero(upd))
}

/// The best probability between `*bestp` and `oldp` for counts `ct`, and
/// what moving there saves net of coding the move (zero, and `*bestp` set to
/// `oldp`, if no move saves anything): libvpx's
/// `vp9_prob_diff_update_savings_search`.
pub(crate) fn savings_search(ct: [u32; 2], oldp: u8, bestp: &mut u8, upd: u8) -> i64 {
    let old_b = cost_branch256(ct, oldp) as i64;
    let mut best_savings = 0i64;
    let mut best_newp = oldp;
    let upd_cost = update_flag_cost(upd);
    if old_b > upd_cost + (MIN_DELP_BITS << PROB_COST_SHIFT) {
        let up = *bestp < oldp;
        let mut newp = *bestp;
        while newp != oldp {
            let new_b = cost_branch256(ct, newp) as i64;
            let update_b = prob_diff_update_cost(newp, oldp) + upd_cost;
            let savings = old_b - new_b - update_b;
            if savings > best_savings {
                best_savings = savings;
                best_newp = newp;
            }
            newp = if up {
                newp.wrapping_add(1)
            } else {
                newp.wrapping_sub(1)
            };
        }
    }
    *bestp = best_newp;
    best_savings
}

/// [`savings_search`] for a coefficient model's pivot: the nodes after it
/// follow its probability through the Pareto table, so their counts are
/// costed too, and the search steps `step` at a time. libvpx's
/// `vp9_prob_diff_update_savings_search_model`; `ct` is every node's counts.
pub(crate) fn savings_search_model(
    ct: &[[u32; 2]; ENTROPY_NODES],
    oldp: u8,
    bestp: &mut u8,
    upd: u8,
    step: i64,
) -> i64 {
    let cost = |p: u8| -> i64 {
        let tail = PARETO8_FULL
            .get(usize::from(p).wrapping_sub(1))
            .copied()
            .unwrap_or([128; 8]);
        let mut b = cost_branch256(ct[PIVOT_NODE], p) as i64;
        for (node, &q) in ct.iter().skip(UNCONSTRAINED_NODES).zip(&tail) {
            b += cost_branch256(*node, q) as i64;
        }
        b
    };
    let step_sign: i64 = if *bestp > oldp { -1 } else { 1 };
    let step = step.max(1) * step_sign;
    let upd_cost = update_flag_cost(upd);
    let old_b = cost(oldp);
    let mut best_savings = 0i64;
    let mut best_newp = oldp;
    if old_b > upd_cost + (MIN_DELP_BITS << PROB_COST_SHIFT) {
        let mut newp = i64::from(*bestp);
        while (newp - i64::from(oldp)) * step_sign < 0 {
            if let Ok(p) = u8::try_from(newp)
                && p >= 1
            {
                let new_b = cost(p);
                let update_b = prob_diff_update_cost(p, oldp) + upd_cost;
                let savings = old_b - new_b - update_b;
                if savings > best_savings {
                    best_savings = savings;
                    best_newp = p;
                }
            }
            newp += step;
        }
    }
    *bestp = best_newp;
    best_savings
}

/// Update `*oldp` if the counts `ct` make it worth it, writing the flag and
/// any move: libvpx's `vp9_cond_prob_diff_update`.
pub(crate) fn cond_prob_diff_update(w: &mut BoolWriter, oldp: &mut u8, ct: [u32; 2]) {
    let mut newp = get_binary_prob(ct[0], ct[1]);
    let savings = savings_search(ct, *oldp, &mut newp, DIFF_UPDATE_PROB);
    if savings > 0 {
        w.write(true, DIFF_UPDATE_PROB);
        write_prob_diff_update(w, newp, *oldp);
        *oldp = newp;
    } else {
        w.write(false, DIFF_UPDATE_PROB);
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
    use crate::boolread::BoolReader;
    use crate::probs::diff_update_prob;

    /// Every move between two probabilities, written, reads back as the new
    /// probability through the decoder's reader.
    #[test]
    fn every_update_reads_back() {
        let mut w = BoolWriter::new();
        let mut pairs = Vec::new();
        for oldp in 1u8..=255 {
            for newp in 1u8..=255 {
                if newp != oldp {
                    w.write(true, DIFF_UPDATE_PROB);
                    write_prob_diff_update(&mut w, newp, oldp);
                    pairs.push((oldp, newp));
                }
            }
        }
        let data = w.finish();
        let mut r = BoolReader::new(&data).unwrap();
        for (oldp, newp) in pairs {
            let mut p = oldp;
            diff_update_prob(&mut r, &mut p);
            assert_eq!(p, newp, "{oldp} -> {newp}");
        }
    }

    /// The update's cost table agrees with the bits the code really takes.
    #[test]
    fn update_bits_count_the_code() {
        for oldp in [1u8, 2, 60, 128, 200, 254, 255] {
            for newp in 1u8..=255 {
                if newp == oldp {
                    continue;
                }
                let delp = remap_prob(newp, oldp) as i32;
                let bits = if delp < 16 {
                    5
                } else if delp < 32 {
                    6
                } else if delp < 64 {
                    8
                } else if delp - 64 < 65 {
                    10
                } else {
                    11
                };
                assert_eq!(
                    prob_diff_update_cost(newp, oldp),
                    bits << PROB_COST_SHIFT,
                    "{oldp} -> {newp}"
                );
            }
        }
    }

    #[test]
    fn a_skewed_count_is_worth_an_update_and_an_even_one_is_not() {
        // 1000 zeros, 10 ones: the probability should rise from 128.
        let mut p = 128;
        let mut w = BoolWriter::new();
        cond_prob_diff_update(&mut w, &mut p, [1000, 10]);
        assert!(p > 200, "moved to {p}");
        // The same counts again: already near the best, the move is not
        // worth its bits.
        let before = p;
        cond_prob_diff_update(&mut w, &mut p, [1000, 10]);
        assert_eq!(p, before);
        // Too few symbols to pay for any move.
        let mut q = 128;
        cond_prob_diff_update(&mut w, &mut q, [3, 0]);
        assert_eq!(q, 128);
        // What was written reads back the same way.
        let data = w.finish();
        let mut r = BoolReader::new(&data).unwrap();
        let mut d = 128;
        diff_update_prob(&mut r, &mut d);
        assert_eq!(d, before);
        diff_update_prob(&mut r, &mut d);
        assert_eq!(d, before);
        let mut e = 128;
        diff_update_prob(&mut r, &mut e);
        assert_eq!(e, 128);
    }

    #[test]
    fn the_search_never_picks_a_move_that_costs_more_than_it_saves() {
        for (ct, oldp) in [
            ([500, 500], 128),
            ([0, 900], 250),
            ([900, 0], 3),
            ([40, 1], 128),
        ] {
            let mut best = get_binary_prob(ct[0], ct[1]);
            let s = savings_search(ct, oldp, &mut best, DIFF_UPDATE_PROB);
            assert!(s >= 0);
            if s > 0 {
                let old = cost_branch256(ct, oldp) as i64;
                let new = cost_branch256(ct, best) as i64;
                assert!(old - new > prob_diff_update_cost(best, oldp));
            } else {
                assert_eq!(best, oldp);
            }
        }
    }

    #[test]
    fn the_model_search_steps_and_stays_in_range() {
        // Counts that want the pivot near 40 from 200, stepping by 4.
        let mut ct = [[0u32; 2]; ENTROPY_NODES];
        ct[PIVOT_NODE] = [300, 2000];
        for node in &mut ct[UNCONSTRAINED_NODES..] {
            *node = [500, 500];
        }
        let mut best = get_binary_prob(300, 2000);
        let from = best;
        let s = savings_search_model(&ct, 200, &mut best, DIFF_UPDATE_PROB, 4);
        assert!(s > 0);
        assert!(best < 200);
        assert_eq!(
            (i32::from(best) - i32::from(from)) % 4,
            0,
            "steps of 4 from {from}"
        );
    }

    #[test]
    fn binary_probabilities_round_and_clip() {
        assert_eq!(get_binary_prob(0, 0), 128);
        assert_eq!(get_binary_prob(1, 1), 128);
        assert_eq!(get_binary_prob(0, 5), 1);
        assert_eq!(get_binary_prob(5, 0), 255);
        assert_eq!(get_binary_prob(1, 2), 85);
    }
}
