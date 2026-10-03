//! VP9's probabilities: what each frame decodes with, how a frame's header
//! updates them, and how the symbols a frame decoded adapt them for the next.
//!
//! A frame starts from one of four saved contexts ([`FrameContext`]), applies
//! the deltas its compressed header carries ([`diff_update_prob`]), decodes,
//! and -- unless it is error resilient or frame-parallel -- blends what it
//! actually saw ([`Counts`]) into the probabilities it started from
//! ([`adapt_coef_probs`], [`adapt_mode_probs`], [`adapt_mv_probs`]). The
//! result may be saved back for later frames. All of it is integer arithmetic
//! specified to the bit, which is why it is ported rather than rewritten.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/common/vp9_entropy.c`,
//! `vp9_entropymode.c`, `vp9_entropymv.c`, `vpx_dsp/prob.c`,
//! `vpx_dsp/prob.h` and `vp9/decoder/vp9_dsubexp.c` (copyright the WebM
//! project authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

use crate::boolread::BoolReader;
use crate::common::{
    BLOCK_SIZE_GROUPS, CLASS0_SIZE, COEF_BANDS, COEFF_CONTEXTS, COMP_INTER_CONTEXTS,
    INTER_MODE_CONTEXTS, INTER_MODE_TREE, INTER_MODES, INTRA_INTER_CONTEXTS, INTRA_MODE_TREE,
    INTRA_MODES, MV_CLASS_TREE, MV_CLASS0_TREE, MV_CLASSES, MV_FP_SIZE, MV_FP_TREE, MV_JOINT_TREE,
    MV_JOINTS, MV_OFFSET_BITS, PARTITION_CONTEXTS, PARTITION_TREE, PARTITION_TYPES, PLANE_TYPES,
    REF_CONTEXTS, REF_TYPES, SKIP_CONTEXTS, SWITCHABLE_FILTER_CONTEXTS, SWITCHABLE_FILTERS,
    SWITCHABLE_INTERP_TREE, TX_SIZE_CONTEXTS, TX_SIZES, UNCONSTRAINED_NODES, band_coeff_contexts,
};
use crate::tables;

/// The model probabilities of one transform size: by plane type, reference
/// type, band and context, three nodes each.
pub type CoefProbs =
    [[[[[u8; UNCONSTRAINED_NODES]; COEFF_CONTEXTS]; COEF_BANDS]; REF_TYPES]; PLANE_TYPES];

/// Transform size probabilities, by context: libvpx's `struct tx_probs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TxProbs {
    pub p32x32: [[u8; 3]; TX_SIZE_CONTEXTS],
    pub p16x16: [[u8; 2]; TX_SIZE_CONTEXTS],
    pub p8x8: [[u8; 1]; TX_SIZE_CONTEXTS],
}

/// One motion vector component's probabilities: libvpx's `nmv_component`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MvComponentProbs {
    pub sign: u8,
    pub classes: [u8; MV_CLASSES - 1],
    pub class0: [u8; CLASS0_SIZE - 1],
    pub bits: [u8; MV_OFFSET_BITS],
    pub class0_fp: [[u8; MV_FP_SIZE - 1]; CLASS0_SIZE],
    pub fp: [u8; MV_FP_SIZE - 1],
    pub class0_hp: u8,
    pub hp: u8,
}

/// Motion vector probabilities: libvpx's `nmv_context`. Component 0 is the
/// row, 1 the column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MvProbs {
    pub joints: [u8; MV_JOINTS - 1],
    pub comps: [MvComponentProbs; 2],
}

/// Everything a frame decodes with: libvpx's `FRAME_CONTEXT`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameContext {
    pub y_mode_prob: [[u8; INTRA_MODES - 1]; BLOCK_SIZE_GROUPS],
    pub uv_mode_prob: [[u8; INTRA_MODES - 1]; INTRA_MODES],
    pub partition_prob: [[u8; PARTITION_TYPES - 1]; PARTITION_CONTEXTS],
    pub coef_probs: [CoefProbs; TX_SIZES],
    pub switchable_interp_prob: [[u8; SWITCHABLE_FILTERS - 1]; SWITCHABLE_FILTER_CONTEXTS],
    pub inter_mode_probs: [[u8; 3]; INTER_MODE_CONTEXTS],
    pub intra_inter_prob: [u8; INTRA_INTER_CONTEXTS],
    pub comp_inter_prob: [u8; COMP_INTER_CONTEXTS],
    pub single_ref_prob: [[u8; 2]; REF_CONTEXTS],
    pub comp_ref_prob: [u8; REF_CONTEXTS],
    pub tx_probs: TxProbs,
    pub skip_probs: [u8; SKIP_CONTEXTS],
    pub nmvc: MvProbs,
    /// Whether this context has been set up: decoding from one that has not
    /// is an error, as in libvpx.
    pub initialized: bool,
}

/// libvpx's `default_tx_probs`.
const DEFAULT_TX_PROBS: TxProbs = TxProbs {
    p32x32: [[3, 136, 37], [5, 52, 13]],
    p16x16: [[20, 152], [15, 101]],
    p8x8: [[100], [66]],
};

/// libvpx's `default_nmv_context`.
pub const DEFAULT_MV_PROBS: MvProbs = MvProbs {
    joints: [32, 64, 96],
    comps: [
        // Vertical.
        MvComponentProbs {
            sign: 128,
            classes: [224, 144, 192, 168, 192, 176, 192, 198, 198, 245],
            class0: [216],
            bits: [136, 140, 148, 160, 176, 192, 224, 234, 234, 240],
            class0_fp: [[128, 128, 64], [96, 112, 64]],
            fp: [64, 96, 64],
            class0_hp: 160,
            hp: 128,
        },
        // Horizontal.
        MvComponentProbs {
            sign: 128,
            classes: [216, 128, 176, 160, 176, 176, 192, 198, 198, 208],
            class0: [208],
            bits: [136, 140, 148, 160, 176, 192, 224, 234, 234, 240],
            class0_fp: [[128, 128, 64], [96, 112, 64]],
            fp: [64, 96, 64],
            class0_hp: 160,
            hp: 128,
        },
    ],
};

impl FrameContext {
    /// The defaults every key frame and error-resilient frame starts from:
    /// libvpx's `vp9_default_coef_probs`, `init_mode_probs` and
    /// `vp9_init_mv_probs`, marked initialised.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            y_mode_prob: tables::DEFAULT_IF_Y_PROBS,
            uv_mode_prob: tables::DEFAULT_IF_UV_PROBS,
            partition_prob: tables::DEFAULT_PARTITION_PROBS,
            coef_probs: [
                tables::DEFAULT_COEF_PROBS_4X4,
                tables::DEFAULT_COEF_PROBS_8X8,
                tables::DEFAULT_COEF_PROBS_16X16,
                tables::DEFAULT_COEF_PROBS_32X32,
            ],
            switchable_interp_prob: tables::DEFAULT_SWITCHABLE_INTERP_PROB,
            inter_mode_probs: tables::DEFAULT_INTER_MODE_PROBS,
            intra_inter_prob: tables::DEFAULT_INTRA_INTER_P,
            comp_inter_prob: tables::DEFAULT_COMP_INTER_P,
            single_ref_prob: tables::DEFAULT_SINGLE_REF_P,
            comp_ref_prob: tables::DEFAULT_COMP_REF_P,
            tx_probs: DEFAULT_TX_PROBS,
            skip_probs: tables::DEFAULT_SKIP_PROBS,
            nmvc: DEFAULT_MV_PROBS,
            initialized: true,
        }
    }

    /// A context no frame has set up: what the four saved contexts hold until
    /// the first key frame.
    #[must_use]
    pub fn uninitialized() -> Self {
        Self {
            initialized: false,
            ..Self::defaults()
        }
    }
}

/// Transform size counts: libvpx's `struct tx_counts`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TxCounts {
    pub p32x32: [[u32; 4]; TX_SIZE_CONTEXTS],
    pub p16x16: [[u32; 3]; TX_SIZE_CONTEXTS],
    pub p8x8: [[u32; 2]; TX_SIZE_CONTEXTS],
}

/// One MV component's counts: libvpx's `nmv_component_counts`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MvComponentCounts {
    pub sign: [u32; 2],
    pub classes: [u32; MV_CLASSES],
    pub class0: [u32; CLASS0_SIZE],
    pub bits: [[u32; 2]; MV_OFFSET_BITS],
    pub class0_fp: [[u32; MV_FP_SIZE]; CLASS0_SIZE],
    pub fp: [u32; MV_FP_SIZE],
    pub class0_hp: [u32; 2],
    pub hp: [u32; 2],
}

/// MV counts: libvpx's `nmv_context_counts`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MvCounts {
    pub joints: [u32; MV_JOINTS],
    pub comps: [MvComponentCounts; 2],
}

/// Coefficient counts of one transform size, by plane, reference, band and
/// context: zero, one, two-or-more, and end of block.
pub type CoefCounts =
    [[[[[u32; UNCONSTRAINED_NODES + 1]; COEFF_CONTEXTS]; COEF_BANDS]; REF_TYPES]; PLANE_TYPES];

/// How often each end-of-block decision was made, by transform size, plane,
/// reference, band and context.
pub type EobBranchCounts = [[[[u32; COEFF_CONTEXTS]; COEF_BANDS]; REF_TYPES]; PLANE_TYPES];

/// What a frame decoded, counted, for adaptation: libvpx's `FRAME_COUNTS`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub y_mode: [[u32; INTRA_MODES]; BLOCK_SIZE_GROUPS],
    pub uv_mode: [[u32; INTRA_MODES]; INTRA_MODES],
    pub partition: [[u32; PARTITION_TYPES]; PARTITION_CONTEXTS],
    pub coef: [CoefCounts; TX_SIZES],
    pub eob_branch: [EobBranchCounts; TX_SIZES],
    pub switchable_interp: [[u32; SWITCHABLE_FILTERS]; SWITCHABLE_FILTER_CONTEXTS],
    pub inter_mode: [[u32; INTER_MODES]; INTER_MODE_CONTEXTS],
    pub intra_inter: [[u32; 2]; INTRA_INTER_CONTEXTS],
    pub comp_inter: [[u32; 2]; COMP_INTER_CONTEXTS],
    pub single_ref: [[[u32; 2]; 2]; REF_CONTEXTS],
    pub comp_ref: [[u32; 2]; REF_CONTEXTS],
    pub tx: TxCounts,
    pub skip: [[u32; 2]; SKIP_CONTEXTS],
    pub mv: MvCounts,
}

// --- Merging counts into probabilities -----------------------------------------

/// `num / den` as a probability out of 256, clipped to 1..=255: libvpx's
/// `get_prob`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::arithmetic_side_effects,
    reason = "u32 operands widened to u64 cannot overflow; den is checked nonzero; clipped to 1..=255 before the narrowing"
)]
fn get_prob(num: u32, den: u32) -> u8 {
    if den == 0 {
        return 128;
    }
    let p = (u64::from(num) * 256 + u64::from(den >> 1)) / u64::from(den);
    p.clamp(1, 255) as u8
}

/// `pre` moved `factor`/256 of the way toward `prob`: libvpx's
/// `weighted_prob`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::arithmetic_side_effects,
    reason = "probabilities are 0..=255 and factor 0..=256, so the blend fits u32 and lands back in 0..=255"
)]
fn weighted_prob(pre: u8, prob: u8, factor: u32) -> u8 {
    let blend = u32::from(pre) * (256 - factor.min(256)) + u32::from(prob) * factor.min(256);
    ((blend + 128) >> 8) as u8
}

/// libvpx's `merge_probs`, the coefficient adaptation's blend.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "count_sat is a positive constant and the count is capped at it"
)]
fn merge_probs(pre: u8, ct: [u32; 2], count_sat: u32, max_update_factor: u32) -> u8 {
    let prob = get_prob(ct[0], ct[0].saturating_add(ct[1]));
    let count = ct[0].saturating_add(ct[1]).min(count_sat);
    let factor = max_update_factor * count / count_sat;
    weighted_prob(pre, prob, factor)
}

/// libvpx's `count_to_update_factor`.
const COUNT_TO_UPDATE_FACTOR: [u32; 21] = [
    0, 6, 12, 19, 25, 32, 38, 44, 51, 57, 64, 70, 76, 83, 89, 96, 102, 108, 115, 121, 128,
];

/// libvpx's `mode_mv_merge_probs`, the mode and MV adaptation's blend.
fn mode_mv_merge_probs(pre: u8, ct: [u32; 2]) -> u8 {
    let den = ct[0].saturating_add(ct[1]);
    if den == 0 {
        return pre;
    }
    let count = den.min(20) as usize;
    let factor = COUNT_TO_UPDATE_FACTOR.get(count).copied().unwrap_or(128);
    weighted_prob(pre, get_prob(ct[0], den), factor)
}

/// Adapt the probabilities of `tree` from `counts` of its leaves: libvpx's
/// `vpx_tree_merge_probs`. Returns the count under node `i`.
#[allow(
    clippy::cast_sign_loss,
    reason = "a tree's node indices are non-negative, its leaves non-positive"
)]
fn tree_merge(i: usize, tree: &[i8], pre: &[u8], counts: &[u32], probs: &mut [u8]) -> u32 {
    let side = |node: Option<&i8>, probs: &mut [u8]| -> u32 {
        match node {
            Some(&n) if n > 0 => tree_merge(n as usize, tree, pre, counts, probs),
            Some(&n) => counts.get(n.unsigned_abs() as usize).copied().unwrap_or(0),
            None => 0,
        }
    };
    let left = side(tree.get(i), probs);
    let right = side(tree.get(i.saturating_add(1)), probs);
    if let (Some(p), Some(&pre_p)) = (probs.get_mut(i >> 1), pre.get(i >> 1)) {
        *p = mode_mv_merge_probs(pre_p, [left, right]);
    }
    left.saturating_add(right)
}

/// [`tree_merge`] from the root.
pub fn tree_merge_probs(tree: &[i8], pre: &[u8], counts: &[u32], probs: &mut [u8]) {
    tree_merge(0, tree, pre, counts, probs);
}

/// Adapt the coefficient probabilities: libvpx's `vp9_adapt_coef_probs`.
#[allow(
    clippy::indexing_slicing,
    reason = "every index is a loop position over an array of the same fixed dimensions as the one indexed"
)]
/// `intra_only` is the frame's own kind, `after_key` whether the frame before
/// it was a key frame, which adapts faster.
pub fn adapt_coef_probs(
    fc: &mut FrameContext,
    pre: &FrameContext,
    counts: &Counts,
    intra_only: bool,
    after_key: bool,
) {
    let (count_sat, update_factor) = if intra_only {
        (24, 112)
    } else if after_key {
        (24, 128)
    } else {
        (24, 112)
    };
    for (t, probs_t) in fc.coef_probs.iter_mut().enumerate() {
        let (Some(pre_t), Some(counts_t), Some(eob_t)) = (
            pre.coef_probs.get(t),
            counts.coef.get(t),
            counts.eob_branch.get(t),
        ) else {
            continue;
        };
        for (i, probs_i) in probs_t.iter_mut().enumerate() {
            for (j, probs_j) in probs_i.iter_mut().enumerate() {
                for (k, probs_k) in probs_j.iter_mut().enumerate() {
                    for (l, probs_l) in probs_k.iter_mut().enumerate().take(band_coeff_contexts(k))
                    {
                        let c = counts_t[i][j][k][l];
                        let neob = c[3];
                        let eob = eob_t[i][j][k][l];
                        let branch = [
                            [neob, eob.wrapping_sub(neob)],
                            [c[0], c[1].wrapping_add(c[2])],
                            [c[1], c[2]],
                        ];
                        for (m, p) in probs_l.iter_mut().enumerate() {
                            *p = merge_probs(
                                pre_t[i][j][k][l][m],
                                branch[m],
                                count_sat,
                                update_factor,
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Adapt the mode probabilities: libvpx's `vp9_adapt_mode_probs`.
#[allow(
    clippy::indexing_slicing,
    reason = "the transform-size indices run over TX_SIZE_CONTEXTS and fixed branch arrays of the arrays' own lengths"
)]
pub fn adapt_mode_probs(
    fc: &mut FrameContext,
    pre: &FrameContext,
    counts: &Counts,
    switchable_interp: bool,
    tx_mode_select: bool,
) {
    for (p, (&q, &c)) in fc
        .intra_inter_prob
        .iter_mut()
        .zip(pre.intra_inter_prob.iter().zip(&counts.intra_inter))
    {
        *p = mode_mv_merge_probs(q, c);
    }
    for (p, (&q, &c)) in fc
        .comp_inter_prob
        .iter_mut()
        .zip(pre.comp_inter_prob.iter().zip(&counts.comp_inter))
    {
        *p = mode_mv_merge_probs(q, c);
    }
    for (p, (&q, &c)) in fc
        .comp_ref_prob
        .iter_mut()
        .zip(pre.comp_ref_prob.iter().zip(&counts.comp_ref))
    {
        *p = mode_mv_merge_probs(q, c);
    }
    for (p2, (q2, c2)) in fc
        .single_ref_prob
        .iter_mut()
        .zip(pre.single_ref_prob.iter().zip(&counts.single_ref))
    {
        for (p, (&q, &c)) in p2.iter_mut().zip(q2.iter().zip(c2)) {
            *p = mode_mv_merge_probs(q, c);
        }
    }
    for (p, (q, c)) in fc
        .inter_mode_probs
        .iter_mut()
        .zip(pre.inter_mode_probs.iter().zip(&counts.inter_mode))
    {
        tree_merge_probs(&INTER_MODE_TREE, q, c, p);
    }
    for (p, (q, c)) in fc
        .y_mode_prob
        .iter_mut()
        .zip(pre.y_mode_prob.iter().zip(&counts.y_mode))
    {
        tree_merge_probs(&INTRA_MODE_TREE, q, c, p);
    }
    for (p, (q, c)) in fc
        .uv_mode_prob
        .iter_mut()
        .zip(pre.uv_mode_prob.iter().zip(&counts.uv_mode))
    {
        tree_merge_probs(&INTRA_MODE_TREE, q, c, p);
    }
    for (p, (q, c)) in fc
        .partition_prob
        .iter_mut()
        .zip(pre.partition_prob.iter().zip(&counts.partition))
    {
        tree_merge_probs(&PARTITION_TREE, q, c, p);
    }
    if switchable_interp {
        for (p, (q, c)) in fc.switchable_interp_prob.iter_mut().zip(
            pre.switchable_interp_prob
                .iter()
                .zip(&counts.switchable_interp),
        ) {
            tree_merge_probs(&SWITCHABLE_INTERP_TREE, q, c, p);
        }
    }
    if tx_mode_select {
        for i in 0..TX_SIZE_CONTEXTS {
            let c8 = counts.tx.p8x8[i];
            fc.tx_probs.p8x8[i][0] = mode_mv_merge_probs(pre.tx_probs.p8x8[i][0], [c8[0], c8[1]]);

            let c16 = counts.tx.p16x16[i];
            let branch16 = [[c16[0], c16[1].wrapping_add(c16[2])], [c16[1], c16[2]]];
            for (j, b) in branch16.iter().enumerate() {
                fc.tx_probs.p16x16[i][j] = mode_mv_merge_probs(pre.tx_probs.p16x16[i][j], *b);
            }

            let c32 = counts.tx.p32x32[i];
            let branch32 = [
                [c32[0], c32[1].wrapping_add(c32[2]).wrapping_add(c32[3])],
                [c32[1], c32[2].wrapping_add(c32[3])],
                [c32[2], c32[3]],
            ];
            for (j, b) in branch32.iter().enumerate() {
                fc.tx_probs.p32x32[i][j] = mode_mv_merge_probs(pre.tx_probs.p32x32[i][j], *b);
            }
        }
    }
    for (p, (&q, &c)) in fc
        .skip_probs
        .iter_mut()
        .zip(pre.skip_probs.iter().zip(&counts.skip))
    {
        *p = mode_mv_merge_probs(q, c);
    }
}

/// Adapt the MV probabilities: libvpx's `vp9_adapt_mv_probs`.
pub fn adapt_mv_probs(fc: &mut FrameContext, pre: &FrameContext, counts: &Counts, allow_hp: bool) {
    let (fcm, prem, c) = (&mut fc.nmvc, &pre.nmvc, &counts.mv);
    tree_merge_probs(&MV_JOINT_TREE, &prem.joints, &c.joints, &mut fcm.joints);
    for ((comp, pre_comp), cc) in fcm.comps.iter_mut().zip(&prem.comps).zip(&c.comps) {
        comp.sign = mode_mv_merge_probs(pre_comp.sign, cc.sign);
        tree_merge_probs(
            &MV_CLASS_TREE,
            &pre_comp.classes,
            &cc.classes,
            &mut comp.classes,
        );
        tree_merge_probs(
            &MV_CLASS0_TREE,
            &pre_comp.class0,
            &cc.class0,
            &mut comp.class0,
        );
        for (p, (&q, &b)) in comp.bits.iter_mut().zip(pre_comp.bits.iter().zip(&cc.bits)) {
            *p = mode_mv_merge_probs(q, b);
        }
        for (p, (q, b)) in comp
            .class0_fp
            .iter_mut()
            .zip(pre_comp.class0_fp.iter().zip(&cc.class0_fp))
        {
            tree_merge_probs(&MV_FP_TREE, q, b, p);
        }
        tree_merge_probs(&MV_FP_TREE, &pre_comp.fp, &cc.fp, &mut comp.fp);
        if allow_hp {
            comp.class0_hp = mode_mv_merge_probs(pre_comp.class0_hp, cc.class0_hp);
            comp.hp = mode_mv_merge_probs(pre_comp.hp, cc.hp);
        }
    }
}

// --- Probability updates in the compressed header ------------------------------

/// libvpx's `DIFF_UPDATE_PROB`: the probability that a probability is updated.
const DIFF_UPDATE_PROB: u8 = 252;

/// libvpx's `inv_recenter_nonneg`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "v is at most 254 and m at most 254, so every sum and difference fits"
)]
const fn inv_recenter_nonneg(v: i32, m: i32) -> i32 {
    if v > 2 * m {
        v
    } else if v & 1 != 0 {
        m - ((v + 1) >> 1)
    } else {
        m + (v >> 1)
    }
}

/// libvpx's `inv_remap_prob`: the coded index `v` as a new probability near
/// the old one, `m`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "v indexes a 255-entry table through get and m is a probability, 1..=255; the results stay within 1..=255"
)]
fn inv_remap_prob(v: u32, m: u8) -> u8 {
    let v = i32::from(
        tables::INV_MAP_TABLE
            .get(v as usize)
            .copied()
            .unwrap_or(253),
    );
    let m = i32::from(m) - 1;
    let p = if (m << 1) <= 255 {
        1 + inv_recenter_nonneg(v, m)
    } else {
        255 - inv_recenter_nonneg(v, 255 - 1 - m)
    };
    u8::try_from(p.clamp(0, 255)).unwrap_or(255)
}

/// libvpx's `decode_uniform`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "seven bits and one more, so the sum is at most 254"
)]
fn decode_uniform(r: &mut BoolReader<'_>) -> u32 {
    let m = (1 << 8) - 191;
    let v = r.read_literal(7);
    if v < m {
        v
    } else {
        (v << 1) - m + r.read_bit()
    }
}

/// libvpx's `decode_term_subexp`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "at most 5 literal bits plus 64, or the uniform code's 254 plus 64"
)]
fn decode_term_subexp(r: &mut BoolReader<'_>) -> u32 {
    if r.read_bit() == 0 {
        return r.read_literal(4);
    }
    if r.read_bit() == 0 {
        return r.read_literal(4) + 16;
    }
    if r.read_bit() == 0 {
        return r.read_literal(5) + 32;
    }
    decode_uniform(r) + 64
}

/// Read a possible update to `p`: libvpx's `vp9_diff_update_prob`.
pub fn diff_update_prob(r: &mut BoolReader<'_>, p: &mut u8) {
    if r.read_bool(DIFF_UPDATE_PROB) {
        let delp = decode_term_subexp(r);
        *p = inv_remap_prob(delp, *p);
    }
}

/// libvpx's `MV_UPDATE_PROB`.
const MV_UPDATE_PROB: u8 = 252;

/// Read possible updates to MV probabilities, each a 7-bit value made odd:
/// libvpx's `update_mv_probs`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "seven bits shifted left once and made odd is at most 255"
)]
pub fn update_mv_probs(r: &mut BoolReader<'_>, probs: &mut [u8]) {
    for p in probs {
        if r.read_bool(MV_UPDATE_PROB) {
            *p = ((r.read_literal(7) << 1) | 1) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn get_prob_rounds_and_clips_as_libvpx() {
        assert_eq!(get_prob(0, 10), 1, "never zero");
        assert_eq!(get_prob(10, 10), 255, "never 256");
        assert_eq!(get_prob(1, 2), 128);
        assert_eq!(get_prob(1, 3), 85);
        assert_eq!(get_prob(0, 0), 128);
    }

    #[test]
    fn mode_mv_merging_needs_counts_to_move() {
        assert_eq!(mode_mv_merge_probs(100, [0, 0]), 100);
        // Twenty or more samples move the full 128/256 of the way.
        assert_eq!(
            mode_mv_merge_probs(100, [100, 0]),
            ((100u32 * 128 + 255 * 128 + 128) >> 8) as u8
        );
    }

    #[test]
    fn a_tree_merge_writes_one_probability_per_node() {
        let pre = [128u8; 3];
        let mut probs = [0u8; 3];
        // Partition tree: NONE seen 20 times, nothing else.
        tree_merge_probs(&PARTITION_TREE, &pre, &[20, 0, 0, 0], &mut probs);
        assert_eq!(probs[0], ((128u32 * 128 + 255 * 128 + 128) >> 8) as u8);
        assert_eq!(probs[1], 128, "no counts below the first node");
        assert_eq!(probs[2], 128);
    }

    #[test]
    fn inv_remap_prob_matches_libvpx_at_its_corners() {
        // inv_map_table[0] = 7, m = 128 - 1 = 127, (127 << 1) <= 255:
        // 1 + inv_recenter_nonneg(7, 127) = 1 + (127 - 4) = 124.
        assert_eq!(inv_remap_prob(0, 128), 124);
        // The last index maps to 253 by the table's own repetition.
        assert_eq!(inv_remap_prob(254, 1), 254);
        // m above the midpoint mirrors.
        assert_eq!(
            inv_remap_prob(0, 200),
            255 - inv_recenter_nonneg(7, 255 - 1 - 199) as u8
        );
    }

    #[test]
    fn defaults_are_initialized_and_uninitialized_is_not() {
        assert!(FrameContext::defaults().initialized);
        assert!(!FrameContext::uninitialized().initialized);
        assert_eq!(FrameContext::defaults().skip_probs, [192, 128, 64]);
    }
}
