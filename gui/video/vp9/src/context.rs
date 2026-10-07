//! The contexts a block's decisions are coded with: which probabilities each
//! one uses, chosen by what the blocks above and to the left decided.
//!
//! The decoder reads with these and the encoder writes with them, so both
//! take them from here: a context computed two ways is a stream that decodes
//! to garbage. `None` is a neighbour that is not there -- off the top of the
//! frame, or left of the tile.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/common/vp9_pred_common.c`,
//! `vp9_pred_common.h`, `vp9_blockd.c` and `vp9_onyxc_int.h` (copyright the
//! WebM project authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "contexts are sums of a few booleans and small indices"
)]

use crate::block::ModeInfo;
use crate::common::{
    ALTREF_FRAME, DC_PRED, GOLDEN_FRAME, InterpFilter, LAST_FRAME, MAX_REF_FRAMES, PredictionMode,
    RefFrame, SWITCHABLE_FILTERS, TxSize,
};

/// libvpx's `vp9_get_skip_context`.
pub(crate) fn skip_context(a: Option<&ModeInfo>, l: Option<&ModeInfo>) -> usize {
    usize::from(a.is_some_and(|m| m.skip)) + usize::from(l.is_some_and(|m| m.skip))
}

/// libvpx's `get_tx_size_context`. `max_tx` is the block's largest
/// transform; a skipped neighbour counts as having used it.
pub(crate) fn tx_size_context(a: Option<&ModeInfo>, l: Option<&ModeInfo>, max_tx: TxSize) -> usize {
    let mut above = a.map_or(max_tx, |m| if m.skip { max_tx } else { m.tx_size });
    let mut left = l.map_or(max_tx, |m| if m.skip { max_tx } else { m.tx_size });
    if l.is_none() {
        left = above;
    }
    if a.is_none() {
        above = left;
    }
    usize::from(above + left > max_tx)
}

/// libvpx's `get_intra_inter_context`.
pub(crate) fn intra_inter_context(a: Option<&ModeInfo>, l: Option<&ModeInfo>) -> usize {
    match (a, l) {
        (Some(a), Some(l)) => {
            let (ai, li) = (!a.is_inter(), !l.is_inter());
            if ai && li { 3 } else { usize::from(ai || li) }
        }
        (Some(e), None) | (None, Some(e)) => 2 * usize::from(!e.is_inter()),
        (None, None) => 0,
    }
}

/// libvpx's `get_pred_context_switchable_interp`. A neighbour that is not
/// there counts as the out-of-range filter `SWITCHABLE_FILTERS`, which is
/// also what an intra block in an inter frame records as its filter.
pub(crate) fn switchable_interp_context(a: Option<&ModeInfo>, l: Option<&ModeInfo>) -> usize {
    let sw = SWITCHABLE_FILTERS as InterpFilter;
    let kind = |m: Option<&ModeInfo>| m.map_or(sw, |m| m.interp_filter);
    let (left, above) = (kind(l), kind(a));
    usize::from(if left == above {
        left
    } else if left == sw {
        above
    } else if above == sw {
        left
    } else {
        sw
    })
}

/// libvpx's `vp9_get_pred_context_seg_id`.
pub(crate) fn seg_id_pred_context(a: Option<&ModeInfo>, l: Option<&ModeInfo>) -> usize {
    usize::from(a.is_some_and(|m| m.seg_id_predicted))
        + usize::from(l.is_some_and(|m| m.seg_id_predicted))
}

/// libvpx's `vp9_above_block_mode`: the mode above sub-block `b` (0 to 3).
pub(crate) fn above_block_mode(
    cur: &ModeInfo,
    above: Option<&ModeInfo>,
    b: usize,
) -> PredictionMode {
    if b == 0 || b == 1 {
        match above {
            Some(a) if !a.is_inter() => a.y_mode(b + 2),
            _ => DC_PRED,
        }
    } else {
        cur.bmi.get(b - 2).map_or(DC_PRED, |m| m.mode)
    }
}

/// libvpx's `vp9_left_block_mode`: the mode left of sub-block `b`.
pub(crate) fn left_block_mode(cur: &ModeInfo, left: Option<&ModeInfo>, b: usize) -> PredictionMode {
    if b == 0 || b == 2 {
        match left {
            Some(l) if !l.is_inter() => l.y_mode(b + 1),
            _ => DC_PRED,
        }
    } else {
        cur.bmi.get(b - 1).map_or(DC_PRED, |m| m.mode)
    }
}

/// libvpx's `vp9_get_reference_mode_context`. `fixed` is the frame's
/// `comp_fixed_ref`.
pub(crate) fn reference_mode_context(
    fixed: RefFrame,
    a: Option<&ModeInfo>,
    l: Option<&ModeInfo>,
) -> usize {
    match (a, l) {
        (Some(a), Some(l)) => {
            if !a.has_second_ref() && !l.has_second_ref() {
                usize::from((a.ref_frame[0] == fixed) ^ (l.ref_frame[0] == fixed))
            } else if !a.has_second_ref() {
                2 + usize::from(a.ref_frame[0] == fixed || !a.is_inter())
            } else if !l.has_second_ref() {
                2 + usize::from(l.ref_frame[0] == fixed || !l.is_inter())
            } else {
                4
            }
        }
        (Some(e), None) | (None, Some(e)) => {
            if e.has_second_ref() {
                3
            } else {
                usize::from(e.ref_frame[0] == fixed)
            }
        }
        (None, None) => 1,
    }
}

/// libvpx's `vp9_get_pred_context_comp_ref_p`. `fixed` and `var` are the
/// frame's `comp_fixed_ref` and `comp_var_ref`.
#[allow(
    clippy::indexing_slicing,
    reason = "var_ref_idx is 0 or 1, indexing a block's two references"
)]
pub(crate) fn comp_ref_context(
    fixed: RefFrame,
    var: [RefFrame; 2],
    sign_bias: &[bool; MAX_REF_FRAMES],
    a: Option<&ModeInfo>,
    l: Option<&ModeInfo>,
) -> usize {
    let fix_ref_idx = usize::from(
        usize::try_from(fixed)
            .ok()
            .and_then(|f| sign_bias.get(f))
            .copied()
            .unwrap_or(false),
    );
    let var_ref_idx = 1 - fix_ref_idx;
    let [var0, var1] = var;
    match (a, l) {
        (Some(a), Some(l)) => {
            let (ai, li) = (!a.is_inter(), !l.is_inter());
            if ai && li {
                2
            } else if ai || li {
                let e = if ai { l } else { a };
                if e.has_second_ref() {
                    1 + 2 * usize::from(e.ref_frame[var_ref_idx] != var1)
                } else {
                    1 + 2 * usize::from(e.ref_frame[0] != var1)
                }
            } else {
                let l_sg = !l.has_second_ref();
                let a_sg = !a.has_second_ref();
                let vrfa = if a_sg {
                    a.ref_frame[0]
                } else {
                    a.ref_frame[var_ref_idx]
                };
                let vrfl = if l_sg {
                    l.ref_frame[0]
                } else {
                    l.ref_frame[var_ref_idx]
                };
                if vrfa == vrfl && var1 == vrfa {
                    0
                } else if l_sg && a_sg {
                    if (vrfa == fixed && vrfl == var0) || (vrfl == fixed && vrfa == var0) {
                        4
                    } else if vrfa == vrfl {
                        3
                    } else {
                        1
                    }
                } else if l_sg || a_sg {
                    let vrfc = if l_sg { vrfa } else { vrfl };
                    let rfs = if a_sg { vrfa } else { vrfl };
                    if vrfc == var1 && rfs != var1 {
                        1
                    } else if rfs == var1 && vrfc != var1 {
                        2
                    } else {
                        4
                    }
                } else if vrfa == vrfl {
                    4
                } else {
                    2
                }
            }
        }
        (Some(e), None) | (None, Some(e)) => {
            if !e.is_inter() {
                2
            } else if e.has_second_ref() {
                4 * usize::from(e.ref_frame[var_ref_idx] != var1)
            } else {
                3 * usize::from(e.ref_frame[0] != var1)
            }
        }
        (None, None) => 2,
    }
}

/// libvpx's `vp9_get_pred_context_single_ref_p1`: last or not.
pub(crate) fn single_ref_p1_context(a: Option<&ModeInfo>, l: Option<&ModeInfo>) -> usize {
    let last = |f: RefFrame| f == LAST_FRAME;
    match (a, l) {
        (Some(a), Some(l)) => {
            let (ai, li) = (!a.is_inter(), !l.is_inter());
            if ai && li {
                2
            } else if ai || li {
                let e = if ai { l } else { a };
                if e.has_second_ref() {
                    1 + usize::from(last(e.ref_frame[0]) || last(e.ref_frame[1]))
                } else {
                    4 * usize::from(last(e.ref_frame[0]))
                }
            } else {
                let (a2, l2) = (a.has_second_ref(), l.has_second_ref());
                let (a0, a1, l0, l1) = (
                    a.ref_frame[0],
                    a.ref_frame[1],
                    l.ref_frame[0],
                    l.ref_frame[1],
                );
                if a2 && l2 {
                    1 + usize::from(last(a0) || last(a1) || last(l0) || last(l1))
                } else if a2 || l2 {
                    let rfs = if a2 { l0 } else { a0 };
                    let (crf1, crf2) = if a2 { (a0, a1) } else { (l0, l1) };
                    if last(rfs) {
                        3 + usize::from(last(crf1) || last(crf2))
                    } else {
                        usize::from(last(crf1) || last(crf2))
                    }
                } else {
                    2 * usize::from(last(a0)) + 2 * usize::from(last(l0))
                }
            }
        }
        (Some(e), None) | (None, Some(e)) => {
            if !e.is_inter() {
                2
            } else if e.has_second_ref() {
                1 + usize::from(last(e.ref_frame[0]) || last(e.ref_frame[1]))
            } else {
                4 * usize::from(last(e.ref_frame[0]))
            }
        }
        (None, None) => 2,
    }
}

/// libvpx's `vp9_get_pred_context_single_ref_p2`: golden or altref.
pub(crate) fn single_ref_p2_context(a: Option<&ModeInfo>, l: Option<&ModeInfo>) -> usize {
    let golden = |f: RefFrame| f == GOLDEN_FRAME;
    let last = |f: RefFrame| f == LAST_FRAME;
    match (a, l) {
        (Some(a), Some(l)) => {
            let (ai, li) = (!a.is_inter(), !l.is_inter());
            if ai && li {
                2
            } else if ai || li {
                let e = if ai { l } else { a };
                if e.has_second_ref() {
                    1 + 2 * usize::from(golden(e.ref_frame[0]) || golden(e.ref_frame[1]))
                } else if last(e.ref_frame[0]) {
                    3
                } else {
                    4 * usize::from(golden(e.ref_frame[0]))
                }
            } else {
                let (a2, l2) = (a.has_second_ref(), l.has_second_ref());
                let (a0, a1, l0, l1) = (
                    a.ref_frame[0],
                    a.ref_frame[1],
                    l.ref_frame[0],
                    l.ref_frame[1],
                );
                if a2 && l2 {
                    if a0 == l0 && a1 == l1 {
                        3 * usize::from(golden(a0) || golden(a1) || golden(l0) || golden(l1))
                    } else {
                        2
                    }
                } else if a2 || l2 {
                    let rfs = if a2 { l0 } else { a0 };
                    let (crf1, crf2) = if a2 { (a0, a1) } else { (l0, l1) };
                    if golden(rfs) {
                        3 + usize::from(golden(crf1) || golden(crf2))
                    } else if rfs == ALTREF_FRAME {
                        usize::from(golden(crf1) || golden(crf2))
                    } else {
                        1 + 2 * usize::from(golden(crf1) || golden(crf2))
                    }
                } else if last(a0) && last(l0) {
                    3
                } else if last(a0) || last(l0) {
                    let edge0 = if last(a0) { l0 } else { a0 };
                    4 * usize::from(golden(edge0))
                } else {
                    2 * usize::from(golden(a0)) + 2 * usize::from(golden(l0))
                }
            }
        }
        (Some(e), None) | (None, Some(e)) => {
            if !e.is_inter() || (last(e.ref_frame[0]) && !e.has_second_ref()) {
                2
            } else if !e.has_second_ref() {
                4 * usize::from(golden(e.ref_frame[0]))
            } else {
                3 * usize::from(golden(e.ref_frame[0]) || golden(e.ref_frame[1]))
            }
        }
        (None, None) => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{BLOCK_8X8, INTRA_FRAME, NO_REF_FRAME, TX_8X8, TX_16X16, TX_32X32};

    fn intra(skip: bool, tx_size: TxSize) -> ModeInfo {
        ModeInfo {
            sb_type: BLOCK_8X8,
            skip,
            tx_size,
            ref_frame: [INTRA_FRAME, NO_REF_FRAME],
            ..ModeInfo::default()
        }
    }

    fn inter(r0: RefFrame, r1: RefFrame) -> ModeInfo {
        ModeInfo {
            ref_frame: [r0, r1],
            ..ModeInfo::default()
        }
    }

    #[test]
    fn skip_counts_skipped_neighbours() {
        let s = intra(true, TX_8X8);
        let n = intra(false, TX_8X8);
        assert_eq!(skip_context(None, None), 0);
        assert_eq!(skip_context(Some(&s), None), 1);
        assert_eq!(skip_context(Some(&s), Some(&s)), 2);
        assert_eq!(skip_context(Some(&n), Some(&s)), 1);
    }

    #[test]
    fn transform_size_context_fills_in_a_missing_neighbour() {
        let small = intra(false, TX_8X8);
        let skipped = intra(true, TX_8X8);
        // Both missing: max + max > max.
        assert_eq!(tx_size_context(None, None, TX_16X16), 1);
        // One small neighbour stands for both: 1 + 1 > 2 is false.
        assert_eq!(tx_size_context(Some(&small), None, TX_16X16), 0);
        // A skipped neighbour counts as the largest.
        assert_eq!(tx_size_context(Some(&skipped), Some(&small), TX_16X16), 1);
        assert_eq!(tx_size_context(Some(&small), Some(&small), TX_32X32), 0);
    }

    #[test]
    fn intra_inter_and_reference_contexts() {
        let i = intra(false, TX_8X8);
        let last = inter(LAST_FRAME, NO_REF_FRAME);
        let gold = inter(GOLDEN_FRAME, NO_REF_FRAME);
        assert_eq!(intra_inter_context(Some(&i), Some(&i)), 3);
        assert_eq!(intra_inter_context(Some(&i), Some(&last)), 1);
        assert_eq!(intra_inter_context(None, Some(&i)), 2);
        assert_eq!(intra_inter_context(None, None), 0);
        assert_eq!(single_ref_p1_context(Some(&last), Some(&last)), 4);
        assert_eq!(single_ref_p1_context(None, None), 2);
        assert_eq!(single_ref_p2_context(Some(&gold), Some(&gold)), 4);
        assert_eq!(single_ref_p2_context(Some(&last), None), 2);
    }
}
