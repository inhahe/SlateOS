//! Motion vectors as bits: a block's vector coded as its difference from a
//! predicted one, and the vector probabilities' updates in the compressed
//! header.
//!
//! A difference is a joint (which of its two components are nonzero), then
//! each nonzero component as a sign, a magnitude class, the class's integer
//! bits, a quarter-pel fraction and, at high precision, an eighth-pel bit --
//! what the decoder's `read_mv` reads. Where the predicted vector is too
//! long for eighth pixels (`use_mv_hp`) the eighth-pel bit is not coded and
//! the decoder takes it as 1, so such a difference must be even.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_encodemv.c`
//! and `vp9/common/vp9_entropymv.c` (copyright the WebM project authors),
//! used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "vector components are 15-bit, classes below 11, and costs sums of a few 13-bit costs"
)]

use crate::common::{
    CLASS0_BITS, CLASS0_SIZE, MV_CLASS_TREE, MV_CLASS0_TREE, MV_CLASSES, MV_FP_SIZE, MV_FP_TREE,
    MV_JOINT_TREE, MV_JOINTS, Mv,
};
use crate::enc::cost::{PROB_COST_SHIFT, cost_branch256, cost_one, cost_zero};
use crate::enc::subexp::get_binary_prob;
use crate::enc::writer::{BoolWriter, Token, tree_branch_counts};
use crate::probs::{MvComponentProbs, MvCounts, MvProbs};

/// The probability that a vector probability is not updated: libvpx's
/// `MV_UPDATE_PROB`.
const MV_UPDATE_PROB: u8 = 252;

/// The codes of the joint, class and fraction trees, as libvpx's
/// `vp9_entropy_mv_init` makes them (a test checks they still do).
const MV_JOINT_ENCODINGS: [Token; MV_JOINTS] = [
    Token { value: 0, len: 1 },
    Token { value: 2, len: 2 },
    Token { value: 6, len: 3 },
    Token { value: 7, len: 3 },
];
const MV_FP_ENCODINGS: [Token; MV_FP_SIZE] = [
    Token { value: 0, len: 1 },
    Token { value: 2, len: 2 },
    Token { value: 6, len: 3 },
    Token { value: 7, len: 3 },
];
const MV_CLASS_ENCODINGS: [Token; MV_CLASSES] = [
    Token { value: 0, len: 1 },
    Token { value: 2, len: 2 },
    Token { value: 12, len: 4 },
    Token { value: 13, len: 4 },
    Token { value: 28, len: 5 },
    Token { value: 29, len: 5 },
    Token { value: 30, len: 5 },
    Token { value: 124, len: 7 },
    Token { value: 125, len: 7 },
    Token { value: 126, len: 7 },
    Token { value: 127, len: 7 },
];

/// Which components of a difference are nonzero: libvpx's
/// `vp9_get_mv_joint` (0 neither, 1 the column, 2 the row, 3 both).
pub(crate) fn mv_joint(mv: Mv) -> usize {
    match (mv.row != 0, mv.col != 0) {
        (false, false) => 0,
        (false, true) => 1,
        (true, false) => 2,
        (true, true) => 3,
    }
}

/// A magnitude's class and its offset within the class: libvpx's
/// `vp9_get_mv_class`, for `z` = magnitude - 1.
pub(crate) fn mv_class(z: i32) -> (usize, i32) {
    let class = if z >= (CLASS0_SIZE as i32) * 4096 {
        MV_CLASSES - 1
    } else {
        usize::from(
            crate::tables::LOG_IN_BASE_2
                .get(usize::try_from(z >> 3).unwrap_or(0))
                .copied()
                .unwrap_or(10),
        )
    };
    let base = if class == 0 {
        0
    } else {
        (CLASS0_SIZE as i32) << (class + 2)
    };
    (class, z - base)
}

/// One component of a difference: libvpx's `encode_mv_component`.
fn encode_mv_component(w: &mut BoolWriter, comp: i32, p: &MvComponentProbs, usehp: bool) {
    debug_assert!(comp != 0);
    let sign = comp < 0;
    let (class, offset) = mv_class(comp.abs() - 1);
    let d = offset >> 3;
    let fr = (offset >> 1) & 3;
    let hp = offset & 1;
    w.write(sign, p.sign);
    let code = MV_CLASS_ENCODINGS.get(class).copied().unwrap_or_default();
    w.write_token(&MV_CLASS_TREE, &p.classes, code);
    if class == 0 {
        w.write(d != 0, p.class0[0]);
    } else {
        for i in 0..class + CLASS0_BITS - 1 {
            w.write((d >> i) & 1 != 0, p.bits.get(i).copied().unwrap_or(128));
        }
    }
    let fp_probs: &[u8] = if class == 0 {
        p.class0_fp
            .get(usize::try_from(d).unwrap_or(0))
            .map_or(&[], |v| v.as_slice())
    } else {
        &p.fp
    };
    let code = MV_FP_ENCODINGS
        .get(usize::try_from(fr).unwrap_or(0))
        .copied()
        .unwrap_or_default();
    w.write_token(&MV_FP_TREE, fp_probs, code);
    if usehp {
        w.write(hp != 0, if class == 0 { p.class0_hp } else { p.hp });
    }
}

/// Write a block's vector `mv` as its difference from `reference`:
/// libvpx's `vp9_encode_mv`. Eighth pixels are coded only where the frame
/// allows them (`allow_hp`) and the reference is short enough.
pub(crate) fn encode_mv(w: &mut BoolWriter, mv: Mv, reference: Mv, ctx: &MvProbs, allow_hp: bool) {
    let diff = Mv {
        row: mv.row.wrapping_sub(reference.row),
        col: mv.col.wrapping_sub(reference.col),
    };
    let j = mv_joint(diff);
    let usehp = allow_hp && crate::block::use_mv_hp(reference);
    let code = MV_JOINT_ENCODINGS.get(j).copied().unwrap_or_default();
    w.write_token(&MV_JOINT_TREE, &ctx.joints, code);
    if j == 2 || j == 3 {
        encode_mv_component(w, i32::from(diff.row), &ctx.comps[0], usehp);
    }
    if j == 1 || j == 3 {
        encode_mv_component(w, i32::from(diff.col), &ctx.comps[1], usehp);
    }
}

/// libvpx's `update_mv`: replace a probability with what the counts want,
/// made odd and sent in 7 bits, if that saves more than the 7 bits cost.
fn update_mv(w: &mut BoolWriter, ct: [u32; 2], cur_p: &mut u8) {
    let new_p = get_binary_prob(ct[0], ct[1]) | 1;
    let update = cost_branch256(ct, *cur_p) + u64::from(cost_zero(MV_UPDATE_PROB))
        > cost_branch256(ct, new_p) + u64::from(cost_one(MV_UPDATE_PROB)) + (7 << PROB_COST_SHIFT);
    w.write(update, MV_UPDATE_PROB);
    if update {
        *cur_p = new_p;
        w.write_literal(u32::from(new_p >> 1), 7);
    }
}

/// libvpx's `write_mv_update`: a tree's probabilities from its symbols'
/// counts.
fn write_mv_update(w: &mut BoolWriter, tree: &[i8], probs: &mut [u8], counts: &[u32]) {
    let mut branch_ct = [[0u32; 2]; 32];
    tree_branch_counts(tree, counts, &mut branch_ct);
    for (p, &ct) in probs.iter_mut().zip(&branch_ct) {
        update_mv(w, ct, p);
    }
}

/// The compressed header's vector probability updates, written and made:
/// libvpx's `vp9_write_nmv_probs`.
pub(crate) fn write_nmv_probs(
    w: &mut BoolWriter,
    mvc: &mut MvProbs,
    counts: &MvCounts,
    usehp: bool,
) {
    write_mv_update(w, &MV_JOINT_TREE, &mut mvc.joints, &counts.joints);
    for (comp, cc) in mvc.comps.iter_mut().zip(&counts.comps) {
        update_mv(w, cc.sign, &mut comp.sign);
        write_mv_update(w, &MV_CLASS_TREE, &mut comp.classes, &cc.classes);
        write_mv_update(w, &MV_CLASS0_TREE, &mut comp.class0, &cc.class0);
        for (b, &ct) in comp.bits.iter_mut().zip(&cc.bits) {
            update_mv(w, ct, b);
        }
    }
    for (comp, cc) in mvc.comps.iter_mut().zip(&counts.comps) {
        for (p, c) in comp.class0_fp.iter_mut().zip(&cc.class0_fp) {
            write_mv_update(w, &MV_FP_TREE, p, c);
        }
        write_mv_update(w, &MV_FP_TREE, &mut comp.fp, &cc.fp);
    }
    if usehp {
        for (comp, cc) in mvc.comps.iter_mut().zip(&counts.comps) {
            update_mv(w, cc.class0_hp, &mut comp.class0_hp);
            update_mv(w, cc.hp, &mut comp.hp);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::cast_possible_truncation,
        reason = "a test: a failure should be loud"
    )]

    use super::*;
    use crate::boolread::BoolReader;
    use crate::enc::writer::tokens_from_tree;
    use crate::probs::DEFAULT_MV_PROBS;

    #[test]
    fn the_codes_are_the_trees() {
        assert_eq!(
            tokens_from_tree(&MV_JOINT_TREE, MV_JOINTS),
            MV_JOINT_ENCODINGS
        );
        assert_eq!(tokens_from_tree(&MV_FP_TREE, MV_FP_SIZE), MV_FP_ENCODINGS);
        assert_eq!(
            tokens_from_tree(&MV_CLASS_TREE, MV_CLASSES),
            MV_CLASS_ENCODINGS
        );
    }

    /// Every class, every fraction, both precisions: a component written
    /// is the component the decoder reads -- and at low precision, where
    /// the eighth-pel bit is not sent, an even one.
    #[test]
    fn components_read_back() {
        let mut values = vec![];
        for v in (1..=300).chain([511, 512, 1023, 1024, 4095, 8192, 16383]) {
            values.push(v);
            values.push(-v);
        }
        for usehp in [false, true] {
            let mut w = BoolWriter::new();
            let probs = &DEFAULT_MV_PROBS.comps[1];
            let coded: Vec<i32> = values
                .iter()
                .copied()
                .filter(|v| usehp || v % 2 == 0)
                .collect();
            for &v in &coded {
                encode_mv_component(&mut w, v, probs, usehp);
            }
            let data = w.finish();
            let mut r = BoolReader::new(&data).unwrap();
            for &v in &coded {
                assert_eq!(
                    crate::block::read_mv_component(&mut r, probs, usehp),
                    v,
                    "usehp {usehp}"
                );
            }
        }
    }

    /// Updates are sent only where they pay, and the probabilities sent
    /// are odd: a skewed count moves a probability, an even one does not.
    #[test]
    fn probabilities_update_where_it_pays() {
        let mut mvc = DEFAULT_MV_PROBS;
        let counts = MvCounts {
            joints: [4000, 0, 0, 0],
            ..MvCounts::default()
        };
        let mut w = BoolWriter::new();
        write_nmv_probs(&mut w, &mut mvc, &counts, true);
        assert_ne!(mvc.joints[0], DEFAULT_MV_PROBS.joints[0]);
        assert_eq!(mvc.joints[0] % 2, 1);
        assert_eq!(mvc.comps, DEFAULT_MV_PROBS.comps, "nothing counted there");
        assert_eq!(mv_class(0), (0, 0));
        assert_eq!(mv_class(16), (1, 0));
    }
}
