//! The frame as bits: the uncompressed header, the compressed header's
//! probability updates, and each tile's partitions, modes, motion vectors
//! and tokens.
//!
//! Everything here writes what the decoder (`header`, `decoder`, `block`,
//! `detokenize`) reads, with the same contexts (`context`); the encoder's
//! decisions are made before any of it is written. Writing the compressed
//! header also changes the frame's probabilities -- each update it codes is
//! made -- and the tiles are then written with the probabilities it left.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_bitstream.c`,
//! `vp9_segmentation.c` (`vp9_choose_segmap_coding_method`),
//! `vp9/common/vp9_entropymode.c` (`tx_counts_to_branch_counts_*`) and
//! `vp9_tile_common.c` (copyright the WebM project authors), used under
//! libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions are mode-info coordinates bounded by the frame (at most 8192 a side), sizes are bytes of one frame, and counts wrap as libvpx's unsigned ones do"
)]

use crate::block::{MiGrid, ModeInfo};
use crate::common::{
    ALLOW_32X32, BLOCK_8X8, BLOCK_64X64, BLOCK_INVALID, BlockSize, COEF_BANDS, COEFF_CONTEXTS,
    GOLDEN_FRAME, INTER_MODE_TREE, INTRA_MODE_TREE, InterpFilter, LAST_FRAME, MAX_SEGMENTS,
    NEARESTMV, NEWMV, PARTITION_HORZ, PARTITION_NONE, PARTITION_SPLIT, PARTITION_TREE,
    PARTITION_VERT, PLANE_TYPES, PredictionMode, REF_TYPES, SEG_LVL_MAX, SEG_LVL_REF_FRAME,
    SEG_LVL_SKIP, SEGMENT_TREE, SWITCHABLE, SWITCHABLE_FILTERS, SWITCHABLE_INTERP_TREE, TX_4X4,
    TX_8X8, TX_16X16, TX_32X32, TX_MODE_SELECT, TX_SIZE_CONTEXTS, TX_SIZES, TxMode, TxSize,
    UNCONSTRAINED_NODES, band_coeff_contexts,
};
use crate::context;
use crate::enc::cost::{cost_one, cost_zero};
use crate::enc::encodeframe::{BlockExt, PartitionContexts, sub_blocks};
use crate::enc::encodemv;
use crate::enc::subexp::{
    self, DIFF_UPDATE_PROB, ENTROPY_NODES, cond_prob_diff_update, get_binary_prob,
};
use crate::enc::tokenize::{
    CATEGORY1_TOKEN, CATEGORY6_TOKEN, CoefTokenCounts, EOB_TOKEN, EOSB_TOKEN, ONE_TOKEN,
    TokenExtra, ZERO_TOKEN, cat6_probs,
};
use crate::enc::writer::{BitWriter, BoolWriter, Token, tree_branch_counts};
use crate::header::{self, LoopFilterParams, Quantization, Segmentation};
use crate::probs::{Accumulate, CoefProbs, Counts, EobBranchCounts, FrameContext, TxCounts};
use crate::tables;

/// libvpx's `VP9_FRAME_MARKER`.
const FRAME_MARKER: u32 = 2;
/// libvpx's `VP9_SYNC_CODE_0` to `_2`.
const SYNC_CODE: [u32; 3] = [0x49, 0x83, 0x42];

/// The codes of the intra modes, inter modes, filters, partitions and
/// coefficient tokens, as libvpx's `vp9_tokens_from_tree` makes them from
/// the trees (a test checks they still do): `intra_mode_encodings`,
/// `inter_mode_encodings` (by mode less `NEARESTMV`),
/// `switchable_interp_encodings`, `partition_encodings` and
/// `vp9_coef_encodings`.
const INTRA_MODE_ENCODINGS: [Token; 10] = [
    Token { value: 0, len: 1 },
    Token { value: 6, len: 3 },
    Token { value: 28, len: 5 },
    Token { value: 30, len: 5 },
    Token { value: 58, len: 6 },
    Token { value: 59, len: 6 },
    Token { value: 126, len: 7 },
    Token { value: 127, len: 7 },
    Token { value: 62, len: 6 },
    Token { value: 2, len: 2 },
];
const INTER_MODE_ENCODINGS: [Token; 4] = [
    Token { value: 2, len: 2 },
    Token { value: 6, len: 3 },
    Token { value: 0, len: 1 },
    Token { value: 7, len: 3 },
];
const SWITCHABLE_INTERP_ENCODINGS: [Token; 3] = [
    Token { value: 0, len: 1 },
    Token { value: 2, len: 2 },
    Token { value: 3, len: 2 },
];
const PARTITION_ENCODINGS: [Token; 4] = [
    Token { value: 0, len: 1 },
    Token { value: 2, len: 2 },
    Token { value: 6, len: 3 },
    Token { value: 7, len: 3 },
];
const COEF_ENCODINGS: [Token; 12] = [
    Token { value: 2, len: 2 },
    Token { value: 6, len: 3 },
    Token { value: 28, len: 5 },
    Token { value: 58, len: 6 },
    Token { value: 59, len: 6 },
    Token { value: 60, len: 6 },
    Token { value: 61, len: 6 },
    Token { value: 124, len: 7 },
    Token { value: 125, len: 7 },
    Token { value: 126, len: 7 },
    Token { value: 127, len: 7 },
    Token { value: 0, len: 1 },
];

/// The coefficient tree: libvpx's `vp9_coef_tree`, end of block first.
pub(crate) const COEF_TREE: [i8; 22] = [
    -11, 2, 0, 4, -1, 6, 8, 12, -2, 10, -3, -4, 14, 16, -5, -6, 18, 20, -7, -8, -9, -10,
];
/// The coefficient tree below the pivot, coded with the Pareto table's
/// probabilities: libvpx's `vp9_coef_con_tree`.
const COEF_CON_TREE: [i8; 16] = [2, 6, -2, 4, -3, -4, 8, 10, -5, -6, 12, 14, -7, -8, -9, -10];

/// How the coefficient probabilities' updates are searched: libvpx's
/// `use_fast_coef_updates`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoefUpdates {
    /// A dry run first, to see whether any update is worth its flags.
    TwoLoop,
    /// One pass, the flags held back until the first update.
    OneLoopReduced,
}

/// The settings of the probability update search: libvpx's speed features
/// `use_fast_coef_updates`, `coeff_prob_appx_step` and whether the
/// transform size search only ever chose 8x8 and smaller
/// (`tx_size_search_method == USE_TX_8X8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UpdateSearch {
    pub coef_updates: CoefUpdates,
    pub coef_step: i64,
    pub tx_8x8_only: bool,
}

/// The loop filter as the encoder keeps it: the header's fields and the
/// deltas last sent, so a frame sends only the ones that changed. libvpx's
/// `struct loopfilter` with its `last_ref_deltas` and `last_mode_deltas`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LoopFilterState {
    pub params: LoopFilterParams,
    pub last_ref_deltas: [i8; 4],
    pub last_mode_deltas: [i8; 2],
}

/// What a frame's uncompressed header says that the encoder's state does
/// not: libvpx's `VP9_COMMON` fields `write_uncompressed_header` reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameHeader {
    pub profile: u8,
    pub bit_depth: u8,
    /// libvpx's `vpx_color_space_t`.
    pub color_space: u8,
    pub full_range: bool,
    pub ss_x: u8,
    pub ss_y: u8,
    pub width: u32,
    pub height: u32,
    pub render_width: u32,
    pub render_height: u32,
    /// A key frame, rather than an inter frame.
    pub key_frame: bool,
    pub show_frame: bool,
    pub error_resilient_mode: bool,
    /// An inter frame's: which saved probability contexts it resets.
    pub reset_frame_context: u8,
    pub refresh_frame_context: bool,
    pub frame_parallel_decoding_mode: bool,
    pub frame_context_idx: u8,
    /// An inter frame's: which of the eight reference slots it refreshes.
    pub refresh_frame_flags: u8,
    /// An inter frame's: the slots `LAST_FRAME`, `GOLDEN_FRAME` and
    /// `ALTREF_FRAME` are in, their sign biases, and which of them are this
    /// frame's size (the first that is gives the frame its size).
    pub ref_slots: [u8; 3],
    pub ref_sign_bias: [bool; 3],
    pub ref_same_size: [bool; 3],
    pub allow_high_precision_mv: bool,
    /// `SWITCHABLE`, or the filter every inter block uses.
    pub interp_filter: InterpFilter,
    pub quant: Quantization,
    pub log2_tile_cols: u32,
    pub log2_tile_rows: u32,
    pub tx_mode: TxMode,
}

impl FrameHeader {
    /// The frame's width in 8x8 cells: libvpx's `mi_cols`.
    pub(crate) fn mi_cols(&self) -> usize {
        (self.width as usize).div_ceil(8)
    }

    /// The frame's height in 8x8 cells: libvpx's `mi_rows`.
    pub(crate) fn mi_rows(&self) -> usize {
        (self.height as usize).div_ceil(8)
    }

    /// Whether every quantiser is zero, so the frame is lossless.
    pub(crate) fn lossless(&self) -> bool {
        self.quant.lossless()
    }

    /// The sign biases as the contexts index them, by reference frame.
    fn sign_bias(&self) -> [bool; 4] {
        let [l, g, a] = self.ref_sign_bias;
        [false, l, g, a]
    }
}

/// What the encoder counted while it coded a frame, for the compressed
/// header's updates and the frame's adaptation: libvpx's `FRAME_COUNTS`
/// (the decoder's [`Counts`]), its `tx_totals`, and `rd_counts.coef_counts`,
/// the token counts the model counts are folded from.
#[derive(Clone, Debug, Default)]
pub(crate) struct EncCounts {
    pub counts: Counts,
    pub tx_totals: [u32; TX_SIZES],
    pub coef: Box<[CoefTokenCounts; TX_SIZES]>,
}

/// Tile columns coded apart count apart; the frame's counts are their sums.
impl Accumulate for EncCounts {
    fn accumulate(&mut self, other: &Self) {
        self.counts.accumulate(&other.counts);
        self.tx_totals.accumulate(&other.tx_totals);
        (*self.coef).accumulate(&other.coef);
    }
}

/// libvpx's `fix_interp_filter`: a frame whose blocks all chose one filter
/// codes that filter once, in its header, rather than per block.
pub(crate) fn fix_interp_filter(filter: InterpFilter, counts: &Counts) -> InterpFilter {
    if filter != SWITCHABLE {
        return filter;
    }
    let used: Vec<usize> = (0..SWITCHABLE_FILTERS)
        .filter(|&f| {
            counts
                .switchable_interp
                .iter()
                .any(|c| c.get(f).copied().unwrap_or(0) > 0)
        })
        .collect();
    match used.as_slice() {
        [only] => InterpFilter::try_from(*only).unwrap_or(SWITCHABLE),
        _ => SWITCHABLE,
    }
}

// --- The uncompressed header ------------------------------------------------------

/// libvpx's `encode_loopfilter`. Sending a changed delta makes it the last
/// one sent.
fn write_loopfilter(wb: &mut BitWriter, lf: &mut LoopFilterState) {
    let p = lf.params;
    wb.write_literal(u32::from(p.filter_level), 6);
    wb.write_literal(u32::from(p.sharpness_level), 3);
    wb.write_bit(p.mode_ref_delta_enabled);
    if p.mode_ref_delta_enabled {
        wb.write_bit(p.mode_ref_delta_update);
        if p.mode_ref_delta_update {
            let send = |wb: &mut BitWriter, delta: i8, last: &mut i8| {
                let changed = delta != *last;
                wb.write_bit(changed);
                if changed {
                    *last = delta;
                    wb.write_literal(u32::from(delta.unsigned_abs()) & 0x3f, 6);
                    wb.write_bit(delta < 0);
                }
            };
            for (&d, last) in p.ref_deltas.iter().zip(lf.last_ref_deltas.iter_mut()) {
                send(wb, d, last);
            }
            for (&d, last) in p.mode_deltas.iter().zip(lf.last_mode_deltas.iter_mut()) {
                send(wb, d, last);
            }
        }
    }
}

/// libvpx's `write_delta_q`.
fn write_delta_q(wb: &mut BitWriter, delta_q: i32) {
    if delta_q == 0 {
        wb.write_bit(false);
    } else {
        wb.write_bit(true);
        wb.write_literal(delta_q.unsigned_abs(), 4);
        wb.write_bit(delta_q < 0);
    }
}

/// libvpx's `encode_quantization`.
#[allow(clippy::cast_sign_loss, reason = "a quantiser index is 0..=255")]
fn write_quantization(wb: &mut BitWriter, q: &Quantization) {
    wb.write_literal(q.base_qindex.clamp(0, 255) as u32, 8);
    write_delta_q(wb, q.y_dc_delta_q);
    write_delta_q(wb, q.uv_dc_delta_q);
    write_delta_q(wb, q.uv_ac_delta_q);
}

/// libvpx's `get_unsigned_bits`: the bits that hold 0 to `max`.
fn unsigned_bits(max: u32) -> u32 {
    if max == 0 {
        0
    } else {
        32 - max.leading_zeros()
    }
}

/// libvpx's `encode_segmentation`. The map's coding -- its tree and
/// prediction probabilities, and whether it is predicted from the last
/// frame's -- must already be chosen ([`choose_segmap_coding_method`]).
#[allow(
    clippy::cast_sign_loss,
    reason = "a feature's magnitude is written, its sign apart"
)]
fn write_segmentation(wb: &mut BitWriter, seg: &Segmentation) {
    wb.write_bit(seg.enabled);
    if !seg.enabled {
        return;
    }
    wb.write_bit(seg.update_map);
    if seg.update_map {
        for &p in &seg.tree_probs {
            wb.write_bit(p != 255);
            if p != 255 {
                wb.write_literal(u32::from(p), 8);
            }
        }
        wb.write_bit(seg.temporal_update);
        if seg.temporal_update {
            for &p in &seg.pred_probs {
                wb.write_bit(p != 255);
                if p != 255 {
                    wb.write_literal(u32::from(p), 8);
                }
            }
        }
    }
    wb.write_bit(seg.update_data);
    if seg.update_data {
        wb.write_bit(seg.abs_delta);
        for id in 0u8..8 {
            for feature in 0..SEG_LVL_MAX {
                let active = seg.feature_active(id, feature);
                wb.write_bit(active);
                if active {
                    let data = seg.data(id, feature);
                    let max = header::seg_feature_data_max(feature);
                    let bits = unsigned_bits(max);
                    if header::seg_feature_signed(feature) {
                        wb.write_literal(data.unsigned_abs(), bits);
                        wb.write_bit(data < 0);
                    } else {
                        wb.write_literal(data.max(0) as u32, bits);
                    }
                }
            }
        }
    }
}

/// libvpx's `write_tile_info`.
fn write_tile_info(wb: &mut BitWriter, mi_cols: usize, log2_cols: u32, log2_rows: u32) {
    let (min_log2, max_log2) = header::tile_n_bits(u32::try_from(mi_cols).unwrap_or(u32::MAX));
    for _ in min_log2..log2_cols {
        wb.write_bit(true);
    }
    if log2_cols < max_log2 {
        wb.write_bit(false);
    }
    wb.write_bit(log2_rows != 0);
    if log2_rows != 0 {
        wb.write_bit(log2_rows != 1);
    }
}

/// libvpx's `write_bitdepth_colorspace_sampling`.
fn write_color_config(wb: &mut BitWriter, h: &FrameHeader) {
    if h.profile >= 2 {
        wb.write_bit(h.bit_depth != 10);
    }
    wb.write_literal(u32::from(h.color_space), 3);
    // libvpx's VPX_CS_SRGB.
    if h.color_space != 7 {
        wb.write_bit(h.full_range);
        if h.profile == 1 || h.profile == 3 {
            wb.write_bit(h.ss_x != 0);
            wb.write_bit(h.ss_y != 0);
            wb.write_bit(false);
        }
    } else {
        wb.write_bit(false);
    }
}

/// libvpx's `write_render_size`.
fn write_render_size(wb: &mut BitWriter, h: &FrameHeader) {
    let scaling_active = h.width != h.render_width || h.height != h.render_height;
    wb.write_bit(scaling_active);
    if scaling_active {
        wb.write_literal(h.render_width - 1, 16);
        wb.write_literal(h.render_height - 1, 16);
    }
}

/// libvpx's `write_interp_filter`.
fn write_interp_filter(wb: &mut BitWriter, filter: InterpFilter) {
    const FILTER_TO_LITERAL: [u32; 4] = [1, 0, 2, 3];
    wb.write_bit(filter == SWITCHABLE);
    if filter != SWITCHABLE {
        wb.write_literal(
            FILTER_TO_LITERAL
                .get(usize::from(filter))
                .copied()
                .unwrap_or(0),
            2,
        );
    }
}

/// libvpx's `write_uncompressed_header`, up to and including the tile
/// information. The compressed header's size follows, written by
/// [`pack_frame`] once it is known.
fn write_uncompressed_header(
    wb: &mut BitWriter,
    h: &FrameHeader,
    lf: &mut LoopFilterState,
    seg: &Segmentation,
) {
    wb.write_literal(FRAME_MARKER, 2);
    // write_profile: the profile's low bit first.
    match h.profile {
        0 => wb.write_literal(0, 2),
        1 => wb.write_literal(2, 2),
        2 => wb.write_literal(1, 2),
        _ => wb.write_literal(6, 3),
    }
    wb.write_bit(false); // show_existing_frame
    wb.write_bit(!h.key_frame); // frame_type
    wb.write_bit(h.show_frame);
    wb.write_bit(h.error_resilient_mode);
    if h.key_frame {
        for code in SYNC_CODE {
            wb.write_literal(code, 8);
        }
        write_color_config(wb, h);
        wb.write_literal(h.width - 1, 16);
        wb.write_literal(h.height - 1, 16);
        write_render_size(wb, h);
    } else {
        // Not intra-only: this encoder's inter frames are shown.
        if !h.show_frame {
            wb.write_bit(false);
        }
        if !h.error_resilient_mode {
            wb.write_literal(u32::from(h.reset_frame_context), 2);
        }
        wb.write_literal(u32::from(h.refresh_frame_flags), 8);
        for (&slot, &bias) in h.ref_slots.iter().zip(&h.ref_sign_bias) {
            wb.write_literal(u32::from(slot), 3);
            wb.write_bit(bias);
        }
        // write_frame_size_with_refs.
        let mut found = false;
        for &same in &h.ref_same_size {
            wb.write_bit(same);
            if same {
                found = true;
                break;
            }
        }
        if !found {
            wb.write_literal(h.width - 1, 16);
            wb.write_literal(h.height - 1, 16);
        }
        write_render_size(wb, h);
        wb.write_bit(h.allow_high_precision_mv);
        write_interp_filter(wb, h.interp_filter);
    }
    if !h.error_resilient_mode {
        wb.write_bit(h.refresh_frame_context);
        wb.write_bit(h.frame_parallel_decoding_mode);
    }
    wb.write_literal(u32::from(h.frame_context_idx), 2);
    write_loopfilter(wb, lf);
    write_quantization(wb, &h.quant);
    write_segmentation(wb, seg);
    write_tile_info(wb, h.mi_cols(), h.log2_tile_cols, h.log2_tile_rows);
}

// --- The segment map's coding -------------------------------------------------------

/// libvpx's `calc_segtree_probs`.
fn calc_segtree_probs(c: &[u32; MAX_SEGMENTS]) -> [u8; 7] {
    let c01 = c[0] + c[1];
    let c23 = c[2] + c[3];
    let c45 = c[4] + c[5];
    let c67 = c[6] + c[7];
    [
        get_binary_prob(c01 + c23, c45 + c67),
        get_binary_prob(c01, c23),
        get_binary_prob(c45, c67),
        get_binary_prob(c[0], c[1]),
        get_binary_prob(c[2], c[3]),
        get_binary_prob(c[4], c[5]),
        get_binary_prob(c[6], c[7]),
    ]
}

/// libvpx's `cost_segmap`: what coding these segments with these
/// probabilities costs.
#[allow(
    clippy::indexing_slicing,
    reason = "fixed indices into an eight-count and a seven-probability array"
)]
fn cost_segmap(c: &[u32; MAX_SEGMENTS], p: &[u8; 7]) -> i64 {
    let z = |p: u8| i64::from(cost_zero(p));
    let o = |p: u8| i64::from(cost_one(p));
    let n = |v: u32| i64::from(v);
    let c01 = c[0] + c[1];
    let c23 = c[2] + c[3];
    let c45 = c[4] + c[5];
    let c67 = c[6] + c[7];
    let c0123 = c01 + c23;
    let c4567 = c45 + c67;
    let mut cost = n(c0123) * z(p[0]) + n(c4567) * o(p[0]);
    if c0123 > 0 {
        cost += n(c01) * z(p[1]) + n(c23) * o(p[1]);
        if c01 > 0 {
            cost += n(c[0]) * z(p[3]) + n(c[1]) * o(p[3]);
        }
        if c23 > 0 {
            cost += n(c[2]) * z(p[4]) + n(c[3]) * o(p[4]);
        }
    }
    if c4567 > 0 {
        cost += n(c45) * z(p[2]) + n(c67) * o(p[2]);
        if c45 > 0 {
            cost += n(c[4]) * z(p[5]) + n(c[5]) * o(p[5]);
        }
        if c67 > 0 {
            cost += n(c[6]) * z(p[6]) + n(c[7]) * o(p[6]);
        }
    }
    cost
}

/// The segment ids a frame's map is predicted from, and its blocks: what
/// `count_segs` reads.
struct SegCounter<'a> {
    last_map: &'a [u8],
    temporal: bool,
    tile_start: usize,
    no_pred: [u32; MAX_SEGMENTS],
    pred_flags: [[u32; 2]; 3],
    unpredicted: [u32; MAX_SEGMENTS],
}

impl SegCounter<'_> {
    /// libvpx's `count_segs`: one block's segment, and whether the last
    /// frame's map predicts it (which the block then records).
    fn count(&mut self, mi: &mut MiGrid, mi_row: usize, mi_col: usize) {
        if mi_row >= mi.mi_rows || mi_col >= mi.mi_cols {
            return;
        }
        let Some(idx) = mi.index_at(mi_row, mi_col) else {
            return;
        };
        let Some(&b) = mi.blocks.get(idx) else {
            return;
        };
        let id = usize::from(b.segment_id).min(MAX_SEGMENTS - 1);
        if let Some(n) = self.no_pred.get_mut(id) {
            *n += 1;
        }
        if !self.temporal {
            return;
        }
        let x_mis = b.bw.min(mi.mi_cols - mi_col);
        let y_mis = b.bh.min(mi.mi_rows - mi_row);
        let mut predicted = MAX_SEGMENTS as u8;
        for y in 0..y_mis {
            let start = (mi_row + y) * mi.mi_cols + mi_col;
            for x in start..start + x_mis {
                predicted = predicted.min(self.last_map.get(x).copied().unwrap_or(0));
            }
        }
        let flag = usize::from(predicted) == id;
        let above = if mi_row > 0 {
            mi.at(mi_row - 1, mi_col)
        } else {
            None
        };
        let left = if mi_col > self.tile_start {
            mi.at(mi_row, mi_col - 1)
        } else {
            None
        };
        let ctx = context::seg_id_pred_context(above, left);
        if let Some(n) = self
            .pred_flags
            .get_mut(ctx)
            .and_then(|p| p.get_mut(usize::from(flag)))
        {
            *n += 1;
        }
        if !flag && let Some(n) = self.unpredicted.get_mut(id) {
            *n += 1;
        }
        if let Some(m) = mi.blocks.get_mut(idx) {
            m.seg_id_predicted = flag;
        }
    }

    /// libvpx's `count_segs_sb`: the blocks of one square, as its partition
    /// cut it.
    fn count_sb(&mut self, mi: &mut MiGrid, mi_row: usize, mi_col: usize, bsize: BlockSize) {
        if mi_row >= mi.mi_rows || mi_col >= mi.mi_cols {
            return;
        }
        let Some(b) = mi.at(mi_row, mi_col).copied() else {
            return;
        };
        let n8 =
            |t: &[u8; 13], s: BlockSize| usize::from(t.get(usize::from(s)).copied().unwrap_or(1));
        let bs = n8(&tables::NUM_8X8_WIDE, bsize);
        let hbs = bs / 2;
        let (bw, bh) = (b.bw, b.bh);
        if bw == bs && bh == bs {
            self.count(mi, mi_row, mi_col);
        } else if bw == bs && bh < bs {
            self.count(mi, mi_row, mi_col);
            self.count(mi, mi_row + hbs, mi_col);
        } else if bw < bs && bh == bs {
            self.count(mi, mi_row, mi_col);
            self.count(mi, mi_row, mi_col + hbs);
        } else {
            let subsize = tables::SUBSIZE
                .get(usize::from(PARTITION_SPLIT))
                .and_then(|s| s.get(usize::from(bsize)))
                .copied()
                .unwrap_or(BLOCK_INVALID);
            if subsize == BLOCK_INVALID || hbs == 0 {
                // An 8x8 cut into 4x4 parts is one block.
                self.count(mi, mi_row, mi_col);
                return;
            }
            for n in 0..4 {
                let (dr, dc) = (hbs * (n >> 1), hbs * (n & 1));
                self.count_sb(mi, mi_row + dr, mi_col + dc, subsize);
            }
        }
    }
}

/// libvpx's `vp9_choose_segmap_coding_method`: code the segment map with
/// the cheaper of plain tree probabilities and, on an inter frame,
/// prediction from the last frame's map. Sets the segmentation's
/// probabilities and whether it predicts, and each block's
/// `seg_id_predicted`.
pub(crate) fn choose_segmap_coding_method(
    seg: &mut Segmentation,
    mi: &mut MiGrid,
    last_map: &[u8],
    key_frame: bool,
    log2_tile_cols: u32,
) {
    seg.tree_probs = [255; 7];
    seg.pred_probs = [255; 3];
    let mi_cols = mi.mi_cols;
    let mut counter = SegCounter {
        last_map,
        temporal: !key_frame,
        tile_start: 0,
        no_pred: [0; MAX_SEGMENTS],
        pred_flags: [[0; 2]; 3],
        unpredicted: [0; MAX_SEGMENTS],
    };
    for tile_col in 0..1usize << log2_tile_cols {
        let offset = |i: usize| {
            header::tile_offset(
                u32::try_from(i).unwrap_or(u32::MAX),
                u32::try_from(mi_cols).unwrap_or(u32::MAX),
                log2_tile_cols,
            ) as usize
        };
        let (start, end) = (offset(tile_col), offset(tile_col + 1));
        counter.tile_start = start;
        let mut mi_row = 0;
        while mi_row < mi.mi_rows {
            let mut mi_col = start;
            while mi_col < end {
                counter.count_sb(mi, mi_row, mi_col, BLOCK_64X64);
                mi_col += 8;
            }
            mi_row += 8;
        }
    }
    let no_pred_tree = calc_segtree_probs(&counter.no_pred);
    let no_pred_cost = cost_segmap(&counter.no_pred, &no_pred_tree);
    let mut t_pred_cost = i64::MAX;
    let mut t_pred_tree = [255u8; 7];
    let mut t_nopred_prob = [255u8; 3];
    if !key_frame {
        t_pred_tree = calc_segtree_probs(&counter.unpredicted);
        t_pred_cost = cost_segmap(&counter.unpredicted, &t_pred_tree);
        for (p, &[c0, c1]) in t_nopred_prob.iter_mut().zip(&counter.pred_flags) {
            *p = get_binary_prob(c0, c1);
            t_pred_cost +=
                i64::from(c0) * i64::from(cost_zero(*p)) + i64::from(c1) * i64::from(cost_one(*p));
        }
    }
    if t_pred_cost < no_pred_cost {
        seg.temporal_update = true;
        seg.tree_probs = t_pred_tree;
        seg.pred_probs = t_nopred_prob;
    } else {
        seg.temporal_update = false;
        seg.tree_probs = no_pred_tree;
    }
}

// --- The compressed header ----------------------------------------------------------

/// One context's transform size counts as counts at each node of the 8x8,
/// 16x16 and 32x32 trees.
type TxBranchCounts = ([[u32; 2]; 1], [[u32; 2]; 2], [[u32; 2]; 3]);

/// libvpx's `tx_counts_to_branch_counts_8x8`, `_16x16` and `_32x32`.
fn tx_branch_counts(t: &TxCounts, ctx: usize) -> TxBranchCounts {
    let c8 = t.p8x8.get(ctx).copied().unwrap_or_default();
    let c16 = t.p16x16.get(ctx).copied().unwrap_or_default();
    let c32 = t.p32x32.get(ctx).copied().unwrap_or_default();
    (
        [[c8[0], c8[1]]],
        [[c16[0], c16[1].wrapping_add(c16[2])], [c16[1], c16[2]]],
        [
            [c32[0], c32[1].wrapping_add(c32[2]).wrapping_add(c32[3])],
            [c32[1], c32[2].wrapping_add(c32[3])],
            [c32[2], c32[3]],
        ],
    )
}

/// libvpx's `encode_txfm_probs`.
fn write_tx_mode(w: &mut BoolWriter, tx_mode: TxMode, fc: &mut FrameContext, counts: &TxCounts) {
    w.write_literal(u32::from(tx_mode.min(ALLOW_32X32)), 2);
    if tx_mode >= ALLOW_32X32 {
        w.write_bit(tx_mode == TX_MODE_SELECT);
    }
    if tx_mode != TX_MODE_SELECT {
        return;
    }
    for ctx in 0..TX_SIZE_CONTEXTS {
        let (b8, _, _) = tx_branch_counts(counts, ctx);
        if let Some(p) = fc.tx_probs.p8x8.get_mut(ctx) {
            for (p, &ct) in p.iter_mut().zip(&b8) {
                cond_prob_diff_update(w, p, ct);
            }
        }
    }
    for ctx in 0..TX_SIZE_CONTEXTS {
        let (_, b16, _) = tx_branch_counts(counts, ctx);
        if let Some(p) = fc.tx_probs.p16x16.get_mut(ctx) {
            for (p, &ct) in p.iter_mut().zip(&b16) {
                cond_prob_diff_update(w, p, ct);
            }
        }
    }
    for ctx in 0..TX_SIZE_CONTEXTS {
        let (_, _, b32) = tx_branch_counts(counts, ctx);
        if let Some(p) = fc.tx_probs.p32x32.get_mut(ctx) {
            for (p, &ct) in p.iter_mut().zip(&b32) {
                cond_prob_diff_update(w, p, ct);
            }
        }
    }
}

/// One transform size's branch counts at every node of the coefficient
/// tree, by plane type, reference type, band and context: libvpx's
/// `vp9_coeff_stats`.
type CoefStats =
    [[[[[[u32; 2]; ENTROPY_NODES]; COEFF_CONTEXTS]; COEF_BANDS]; REF_TYPES]; PLANE_TYPES];

/// libvpx's `build_tree_distribution`: the branch counts of one transform
/// size, the first node's taken from the end-of-block decisions, and the
/// probabilities they want.
#[allow(
    clippy::indexing_slicing,
    reason = "every index is a loop position over arrays of the same fixed dimensions"
)]
fn build_tree_distribution(
    tokens: &CoefTokenCounts,
    eob_branch: &EobBranchCounts,
    stats: &mut CoefStats,
    probs: &mut CoefProbs,
) {
    for i in 0..PLANE_TYPES {
        for j in 0..REF_TYPES {
            for k in 0..COEF_BANDS {
                for l in 0..band_coeff_contexts(k) {
                    let ct = &mut stats[i][j][k][l];
                    tree_branch_counts(&COEF_TREE, &tokens[i][j][k][l], ct);
                    ct[0][1] = eob_branch[i][j][k][l].wrapping_sub(ct[0][0]);
                    for m in 0..UNCONSTRAINED_NODES {
                        probs[i][j][k][l][m] = get_binary_prob(ct[m][0], ct[m][1]);
                    }
                }
            }
        }
    }
}

/// Whether a probability at node `t` should change, and to what: the pivot
/// with the model's search, the others with the plain one. libvpx's inner
/// test in `update_coef_probs_common`.
fn coef_update(
    stats: &[[u32; 2]; ENTROPY_NODES],
    t: usize,
    oldp: u8,
    newp: u8,
    step: i64,
) -> (i64, u8) {
    let mut newp = newp;
    let s = if t == crate::common::PIVOT_NODE {
        subexp::savings_search_model(stats, oldp, &mut newp, DIFF_UPDATE_PROB, step)
    } else {
        subexp::savings_search(
            stats.get(t).copied().unwrap_or_default(),
            oldp,
            &mut newp,
            DIFF_UPDATE_PROB,
        )
    };
    (s, newp)
}

/// libvpx's `update_coef_probs_common`: one transform size's coefficient
/// probability updates, written and made.
#[allow(
    clippy::indexing_slicing,
    reason = "every index is a loop position over arrays of the same fixed dimensions"
)]
fn update_coef_probs_common(
    w: &mut BoolWriter,
    old: &mut CoefProbs,
    stats: &CoefStats,
    new: &CoefProbs,
    search: UpdateSearch,
) {
    let upd = DIFF_UPDATE_PROB;
    let cost_zero = i64::from(cost_zero(upd));
    let positions = || {
        (0..PLANE_TYPES).flat_map(move |i| {
            (0..REF_TYPES).flat_map(move |j| {
                (0..COEF_BANDS).flat_map(move |k| {
                    (0..band_coeff_contexts(k))
                        .flat_map(move |l| (0..UNCONSTRAINED_NODES).map(move |t| (i, j, k, l, t)))
                })
            })
        })
    };
    match search.coef_updates {
        CoefUpdates::TwoLoop => {
            // A dry run: is any update worth its flags?
            let mut savings = 0i64;
            let mut updates = 0;
            for (i, j, k, l, t) in positions() {
                let oldp = old[i][j][k][l][t];
                let (s, newp) = coef_update(
                    &stats[i][j][k][l],
                    t,
                    oldp,
                    new[i][j][k][l][t],
                    search.coef_step,
                );
                if s > 0 && newp != oldp {
                    savings += s - cost_zero;
                    updates += 1;
                } else {
                    savings -= cost_zero;
                }
            }
            if updates == 0 || savings < 0 {
                w.write_bit(false);
                return;
            }
            w.write_bit(true);
            for (i, j, k, l, t) in positions() {
                let oldp = old[i][j][k][l][t];
                let (s, newp) = coef_update(
                    &stats[i][j][k][l],
                    t,
                    oldp,
                    new[i][j][k][l][t],
                    search.coef_step,
                );
                let u = s > 0 && newp != oldp;
                w.write(u, upd);
                if u {
                    subexp::write_prob_diff_update(w, newp, oldp);
                    old[i][j][k][l][t] = newp;
                }
            }
        }
        CoefUpdates::OneLoopReduced => {
            let mut updates = 0usize;
            let mut noupdates_before_first = 0usize;
            for (i, j, k, l, t) in positions() {
                let oldp = old[i][j][k][l][t];
                let (s, newp) = coef_update(
                    &stats[i][j][k][l],
                    t,
                    oldp,
                    new[i][j][k][l][t],
                    search.coef_step,
                );
                let u = s > 0 && newp != oldp;
                updates += usize::from(u);
                if !u && updates == 0 {
                    noupdates_before_first += 1;
                    continue;
                }
                if u && updates == 1 {
                    // The first update: the flag, and the "no" flags held
                    // back until now.
                    w.write_bit(true);
                    for _ in 0..noupdates_before_first {
                        w.write(false, upd);
                    }
                }
                w.write(u, upd);
                if u {
                    subexp::write_prob_diff_update(w, newp, oldp);
                    old[i][j][k][l][t] = newp;
                }
            }
            if updates == 0 {
                w.write_bit(false);
            }
        }
    }
}

/// libvpx's `update_coef_probs`.
fn update_coef_probs(
    w: &mut BoolWriter,
    tx_mode: TxMode,
    fc: &mut FrameContext,
    counts: &EncCounts,
    search: UpdateSearch,
) {
    let max_tx = tables::TX_MODE_TO_BIGGEST_TX_SIZE
        .get(usize::from(tx_mode))
        .copied()
        .unwrap_or(TX_4X4);
    for tx in 0..=usize::from(max_tx).min(TX_SIZES - 1) {
        let total = counts.tx_totals.get(tx).copied().unwrap_or(0);
        if total <= 20 || (tx >= usize::from(TX_16X16) && search.tx_8x8_only) {
            w.write_bit(false);
            continue;
        }
        let mut stats: Box<CoefStats> = Box::default();
        let mut new: CoefProbs = Default::default();
        let (Some(tokens), Some(eob_branch), Some(old)) = (
            counts.coef.get(tx),
            counts.counts.eob_branch.get(tx),
            fc.coef_probs.get_mut(tx),
        ) else {
            w.write_bit(false);
            continue;
        };
        build_tree_distribution(tokens, eob_branch, &mut stats, &mut new);
        update_coef_probs_common(w, old, &stats, &new, search);
    }
}

/// libvpx's `prob_diff_update`: a tree's probabilities from its symbols'
/// counts, each updated if that saves bits.
fn prob_diff_update(w: &mut BoolWriter, tree: &[i8], probs: &mut [u8], counts: &[u32]) {
    let mut branch = [[0u32; 2]; 32];
    tree_branch_counts(tree, counts, &mut branch);
    for (p, &ct) in probs.iter_mut().zip(&branch) {
        cond_prob_diff_update(w, p, ct);
    }
}

/// libvpx's `write_compressed_header`: the transform mode, the coefficient
/// and skip probability updates and, on an inter frame, the updates of
/// everything an inter frame's blocks code.
fn write_compressed_header(
    h: &FrameHeader,
    fc: &mut FrameContext,
    counts: &EncCounts,
    search: UpdateSearch,
) -> Vec<u8> {
    let mut w = BoolWriter::new();
    let c = &counts.counts;
    let tx_mode = if h.lossless() {
        crate::common::ONLY_4X4
    } else {
        write_tx_mode(&mut w, h.tx_mode, fc, &c.tx);
        h.tx_mode
    };
    update_coef_probs(&mut w, tx_mode, fc, counts, search);
    for (p, &ct) in fc.skip_probs.iter_mut().zip(&c.skip) {
        cond_prob_diff_update(&mut w, p, ct);
    }
    if !h.key_frame {
        for (p, ct) in fc.inter_mode_probs.iter_mut().zip(&c.inter_mode) {
            prob_diff_update(&mut w, &INTER_MODE_TREE, p, ct);
        }
        if h.interp_filter == SWITCHABLE {
            for (p, ct) in fc
                .switchable_interp_prob
                .iter_mut()
                .zip(&c.switchable_interp)
            {
                prob_diff_update(&mut w, &SWITCHABLE_INTERP_TREE, p, ct);
            }
        }
        for (p, &ct) in fc.intra_inter_prob.iter_mut().zip(&c.intra_inter) {
            cond_prob_diff_update(&mut w, p, ct);
        }
        // The encoder codes single references only: where the sign biases
        // would allow compound prediction, it says it does not use it.
        if header::compound_reference_allowed(&h.sign_bias()) {
            w.write_bit(false);
        }
        for (p, ct) in fc.single_ref_prob.iter_mut().zip(&c.single_ref) {
            cond_prob_diff_update(&mut w, &mut p[0], ct[0]);
            cond_prob_diff_update(&mut w, &mut p[1], ct[1]);
        }
        for (p, ct) in fc.y_mode_prob.iter_mut().zip(&c.y_mode) {
            prob_diff_update(&mut w, &INTRA_MODE_TREE, p, ct);
        }
        for (p, ct) in fc.partition_prob.iter_mut().zip(&c.partition) {
            prob_diff_update(&mut w, &PARTITION_TREE, p, ct);
        }
        encodemv::write_nmv_probs(&mut w, &mut fc.nmvc, &c.mv, h.allow_high_precision_mv);
    }
    w.finish()
}

// --- Tiles ---------------------------------------------------------------------------

/// One tile's writer: libvpx's `write_modes` and what it calls, with the
/// partition contexts it keeps.
struct TileWriter<'a> {
    h: &'a FrameHeader,
    fc: &'a FrameContext,
    seg: &'a Segmentation,
    mi: &'a MiGrid,
    ext: &'a [BlockExt],
    tokens: &'a [TokenExtra],
    /// The next token to write.
    pos: usize,
    partition: &'a mut PartitionContexts,
    mi_col_start: usize,
    w: BoolWriter,
}

impl TileWriter<'_> {
    /// libvpx's `write_partition`, with the probabilities
    /// `set_partition_probs` chooses: a key frame's fixed ones, an inter
    /// frame's from its context.
    fn write_partition(
        &mut self,
        hbs: usize,
        mi_row: usize,
        mi_col: usize,
        p: u8,
        bsize: BlockSize,
    ) {
        let ctx = self.partition.context(mi_row, mi_col, bsize);
        let probs = if self.h.key_frame {
            tables::KF_PARTITION_PROBS.get(ctx).copied()
        } else {
            self.fc.partition_prob.get(ctx).copied()
        }
        .unwrap_or([128; 3]);
        let has_rows = mi_row + hbs < self.mi.mi_rows;
        let has_cols = mi_col + hbs < self.mi.mi_cols;
        if has_rows && has_cols {
            let t = PARTITION_ENCODINGS
                .get(usize::from(p))
                .copied()
                .unwrap_or_default();
            self.w.write_token(&PARTITION_TREE, &probs, t);
        } else if !has_rows && has_cols {
            debug_assert!(p == PARTITION_SPLIT || p == PARTITION_HORZ);
            self.w.write(p == PARTITION_SPLIT, probs[1]);
        } else if has_rows && !has_cols {
            debug_assert!(p == PARTITION_SPLIT || p == PARTITION_VERT);
            self.w.write(p == PARTITION_SPLIT, probs[2]);
        } else {
            debug_assert_eq!(p, PARTITION_SPLIT);
        }
    }

    /// libvpx's `write_modes_sb`.
    fn write_modes_sb(&mut self, mi_row: usize, mi_col: usize, bsize: BlockSize) {
        if mi_row >= self.mi.mi_rows || mi_col >= self.mi.mi_cols {
            return;
        }
        let Some(m) = self.mi.at(mi_row, mi_col) else {
            debug_assert!(false, "a cell no block covers");
            return;
        };
        let bsl = usize::from(
            tables::B_WIDTH_LOG2
                .get(usize::from(bsize))
                .copied()
                .unwrap_or(0),
        );
        let bs = (1usize << bsl) / 4;
        let partition = tables::PARTITION_LOOKUP
            .get(bsl)
            .and_then(|p| p.get(usize::from(m.sb_type)))
            .copied()
            .unwrap_or(PARTITION_NONE);
        self.write_partition(bs, mi_row, mi_col, partition, bsize);
        let subsize = tables::SUBSIZE
            .get(usize::from(partition))
            .and_then(|s| s.get(usize::from(bsize)))
            .copied()
            .unwrap_or(BLOCK_INVALID);
        if subsize < BLOCK_8X8 {
            self.write_modes_b(mi_row, mi_col);
        } else {
            match partition {
                PARTITION_NONE => self.write_modes_b(mi_row, mi_col),
                PARTITION_HORZ => {
                    self.write_modes_b(mi_row, mi_col);
                    if mi_row + bs < self.mi.mi_rows {
                        self.write_modes_b(mi_row + bs, mi_col);
                    }
                }
                PARTITION_VERT => {
                    self.write_modes_b(mi_row, mi_col);
                    if mi_col + bs < self.mi.mi_cols {
                        self.write_modes_b(mi_row, mi_col + bs);
                    }
                }
                _ => {
                    self.write_modes_sb(mi_row, mi_col, subsize);
                    self.write_modes_sb(mi_row, mi_col + bs, subsize);
                    self.write_modes_sb(mi_row + bs, mi_col, subsize);
                    self.write_modes_sb(mi_row + bs, mi_col + bs, subsize);
                }
            }
        }
        if bsize >= BLOCK_8X8 && (bsize == BLOCK_8X8 || partition != PARTITION_SPLIT) {
            self.partition.update(mi_row, mi_col, subsize, bsize);
        }
    }

    /// libvpx's `write_modes_b`: a block's modes, then its tokens.
    fn write_modes_b(&mut self, mi_row: usize, mi_col: usize) {
        let mi = self.mi;
        let Some(idx) = mi.index_at(mi_row, mi_col) else {
            debug_assert!(false, "a block the grid does not hold");
            return;
        };
        let Some(m) = mi.blocks.get(idx) else {
            return;
        };
        let above = if mi_row > 0 {
            mi.at(mi_row - 1, mi_col)
        } else {
            None
        };
        let left = if mi_col > self.mi_col_start {
            mi.at(mi_row, mi_col - 1)
        } else {
            None
        };
        if self.h.key_frame {
            self.write_mb_modes_kf(m, above, left);
        } else {
            let ext = self.ext.get(idx).copied().unwrap_or_default();
            self.pack_inter_mode_mvs(m, &ext, above, left);
        }
        self.pack_mb_tokens();
    }

    /// libvpx's `write_intra_mode`.
    fn write_intra_mode(&mut self, mode: PredictionMode, probs: &[u8; 9]) {
        let t = INTRA_MODE_ENCODINGS
            .get(usize::from(mode))
            .copied()
            .unwrap_or_default();
        self.w.write_token(&INTRA_MODE_TREE, probs, t);
    }

    /// libvpx's `write_skip`: whether the block is skipped, written unless
    /// its segment skips every block.
    fn write_skip(
        &mut self,
        m: &ModeInfo,
        above: Option<&ModeInfo>,
        left: Option<&ModeInfo>,
    ) -> bool {
        if self.seg.feature_active(m.segment_id, SEG_LVL_SKIP) {
            return true;
        }
        let ctx = context::skip_context(above, left);
        let prob = self.fc.skip_probs.get(ctx).copied().unwrap_or(128);
        self.w.write(m.skip, prob);
        m.skip
    }

    /// libvpx's `write_selected_tx_size`.
    fn write_selected_tx_size(
        &mut self,
        m: &ModeInfo,
        above: Option<&ModeInfo>,
        left: Option<&ModeInfo>,
    ) {
        let max_tx = tables::MAX_TXSIZE
            .get(usize::from(m.sb_type))
            .copied()
            .unwrap_or(TX_4X4);
        let ctx = context::tx_size_context(above, left, max_tx);
        let p = &self.fc.tx_probs;
        let probs: &[u8] = match max_tx {
            TX_8X8 => p.p8x8.get(ctx).map_or(&[], |v| v.as_slice()),
            TX_16X16 => p.p16x16.get(ctx).map_or(&[], |v| v.as_slice()),
            _ => p.p32x32.get(ctx).map_or(&[], |v| v.as_slice()),
        };
        let prob = |i: usize| probs.get(i).copied().unwrap_or(128);
        let tx: TxSize = m.tx_size;
        self.w.write(tx != TX_4X4, prob(0));
        if tx != TX_4X4 && max_tx >= TX_16X16 {
            self.w.write(tx != TX_8X8, prob(1));
            if tx != TX_8X8 && max_tx >= TX_32X32 {
                self.w.write(tx != TX_16X16, prob(2));
            }
        }
    }

    /// libvpx's `write_segment_id`.
    fn write_segment_id(&mut self, id: u8) {
        if self.seg.enabled && self.seg.update_map {
            self.w
                .write_tree(&SEGMENT_TREE, &self.seg.tree_probs, u32::from(id), 3, 0);
        }
    }

    /// libvpx's `write_mb_modes_kf`.
    fn write_mb_modes_kf(
        &mut self,
        m: &ModeInfo,
        above: Option<&ModeInfo>,
        left: Option<&ModeInfo>,
    ) {
        if self.seg.update_map {
            self.write_segment_id(m.segment_id);
        }
        self.write_skip(m, above, left);
        if m.sb_type >= BLOCK_8X8 && self.h.tx_mode == TX_MODE_SELECT && !self.h.lossless() {
            self.write_selected_tx_size(m, above, left);
        }
        let probs = |b: usize| {
            let a = context::above_block_mode(m, above, b);
            let l = context::left_block_mode(m, left, b);
            tables::KF_Y_MODE_PROB
                .get(usize::from(a))
                .and_then(|p| p.get(usize::from(l)))
                .copied()
                .unwrap_or([128; 9])
        };
        if m.sb_type >= BLOCK_8X8 {
            let p = probs(0);
            self.write_intra_mode(m.mode, &p);
        } else {
            for b in sub_blocks(m.sb_type) {
                let p = probs(b);
                let mode = m.bmi.get(b).map_or(m.mode, |s| s.mode);
                self.write_intra_mode(mode, &p);
            }
        }
        let uv = tables::KF_UV_MODE_PROB
            .get(usize::from(m.mode))
            .copied()
            .unwrap_or([128; 9]);
        self.write_intra_mode(m.uv_mode, &uv);
    }

    /// libvpx's `write_ref_frames`, for single prediction.
    fn write_ref_frames(
        &mut self,
        m: &ModeInfo,
        above: Option<&ModeInfo>,
        left: Option<&ModeInfo>,
    ) {
        if self.seg.feature_active(m.segment_id, SEG_LVL_REF_FRAME) {
            return;
        }
        let r = m.ref_frame[0];
        let p1 = context::single_ref_p1_context(above, left);
        let prob1 = self.fc.single_ref_prob.get(p1).map_or(128, |p| p[0]);
        self.w.write(r != LAST_FRAME, prob1);
        if r != LAST_FRAME {
            let p2 = context::single_ref_p2_context(above, left);
            let prob2 = self.fc.single_ref_prob.get(p2).map_or(128, |p| p[1]);
            self.w.write(r != GOLDEN_FRAME, prob2);
        }
    }

    /// libvpx's `pack_inter_mode_mvs`: an inter frame's block -- its
    /// segment (predicted or not), skip flag, intra or inter, transform
    /// size, and then its intra modes or its reference, mode, filter and
    /// vector.
    fn pack_inter_mode_mvs(
        &mut self,
        m: &ModeInfo,
        ext: &BlockExt,
        above: Option<&ModeInfo>,
        left: Option<&ModeInfo>,
    ) {
        let seg = self.seg;
        if seg.update_map {
            if seg.temporal_update {
                let ctx = context::seg_id_pred_context(above, left);
                let prob = seg.pred_probs.get(ctx).copied().unwrap_or(128);
                self.w.write(m.seg_id_predicted, prob);
                if !m.seg_id_predicted {
                    self.write_segment_id(m.segment_id);
                }
            } else {
                self.write_segment_id(m.segment_id);
            }
        }
        let skip = self.write_skip(m, above, left);
        let is_inter = m.is_inter();
        if !seg.feature_active(m.segment_id, SEG_LVL_REF_FRAME) {
            let ctx = context::intra_inter_context(above, left);
            let prob = self.fc.intra_inter_prob.get(ctx).copied().unwrap_or(128);
            self.w.write(is_inter, prob);
        }
        if m.sb_type >= BLOCK_8X8 && self.h.tx_mode == TX_MODE_SELECT && !(is_inter && skip) {
            self.write_selected_tx_size(m, above, left);
        }
        if !is_inter {
            if m.sb_type >= BLOCK_8X8 {
                let group = usize::from(
                    tables::SIZE_GROUP
                        .get(usize::from(m.sb_type))
                        .copied()
                        .unwrap_or(0),
                );
                let p = self.fc.y_mode_prob.get(group).copied().unwrap_or([128; 9]);
                self.write_intra_mode(m.mode, &p);
            } else {
                let p = self.fc.y_mode_prob[0];
                for b in sub_blocks(m.sb_type) {
                    let mode = m.bmi.get(b).map_or(m.mode, |s| s.mode);
                    self.write_intra_mode(mode, &p);
                }
            }
            let uv = self
                .fc
                .uv_mode_prob
                .get(usize::from(m.mode))
                .copied()
                .unwrap_or([128; 9]);
            self.write_intra_mode(m.uv_mode, &uv);
            return;
        }
        self.write_ref_frames(m, above, left);
        let inter_probs = self
            .fc
            .inter_mode_probs
            .get(usize::from(ext.mode_context))
            .copied()
            .unwrap_or([128; 3]);
        if !seg.feature_active(m.segment_id, SEG_LVL_SKIP) && m.sb_type >= BLOCK_8X8 {
            self.write_inter_mode(m.mode, &inter_probs);
        }
        if self.h.interp_filter == SWITCHABLE {
            let ctx = context::switchable_interp_context(above, left);
            let probs = self
                .fc
                .switchable_interp_prob
                .get(ctx)
                .copied()
                .unwrap_or([128; 2]);
            let t = SWITCHABLE_INTERP_ENCODINGS
                .get(usize::from(m.interp_filter))
                .copied()
                .unwrap_or_default();
            self.w.write_token(&SWITCHABLE_INTERP_TREE, &probs, t);
        } else {
            debug_assert_eq!(m.interp_filter, self.h.interp_filter);
        }
        let fc: &FrameContext = self.fc;
        let (nmvc, allow_hp) = (&fc.nmvc, self.h.allow_high_precision_mv);
        if m.sb_type < BLOCK_8X8 {
            // Each sub-block's mode, then its vector if it is new.
            for j in sub_blocks(m.sb_type) {
                let b = m.bmi.get(j).copied().unwrap_or_default();
                self.write_inter_mode(b.mode, &inter_probs);
                if b.mode == NEWMV {
                    encodemv::encode_mv(&mut self.w, b.mv[0], ext.best_mv[0], nmvc, allow_hp);
                }
            }
        } else if m.mode == NEWMV {
            encodemv::encode_mv(&mut self.w, m.mv[0], ext.best_mv[0], nmvc, allow_hp);
        }
    }

    /// libvpx's `write_inter_mode`.
    fn write_inter_mode(&mut self, mode: PredictionMode, probs: &[u8; 3]) {
        let t = INTER_MODE_ENCODINGS
            .get(usize::from(mode.saturating_sub(NEARESTMV)))
            .copied()
            .unwrap_or_default();
        self.w.write_token(&INTER_MODE_TREE, probs, t);
    }

    /// libvpx's `pack_mb_tokens`: one block's tokens, through the end-of-
    /// block-tokens marker after them.
    #[allow(
        clippy::indexing_slicing,
        reason = "probability indices are 0..3 into three-element arrays"
    )]
    fn pack_mb_tokens(&mut self) {
        let cat6 = cat6_probs(self.h.bit_depth);
        let probs_of = |fc: &FrameContext, t: &TokenExtra| -> [u8; UNCONSTRAINED_NODES] {
            let [tx, plane, r, band, ctx] = t.probs.parts();
            fc.coef_probs
                .get(tx)
                .and_then(|p| p.get(plane))
                .and_then(|p| p.get(r))
                .and_then(|p| p.get(band))
                .and_then(|p| p.get(ctx))
                .copied()
                .unwrap_or([128; UNCONSTRAINED_NODES])
        };
        loop {
            let Some(&first) = self.tokens.get(self.pos) else {
                debug_assert!(false, "a block's tokens ran out before their end");
                return;
            };
            if first.token == EOSB_TOKEN {
                self.pos += 1;
                return;
            }
            let ctx = probs_of(self.fc, &first);
            if first.token == EOB_TOKEN {
                self.w.write(false, ctx[0]);
                self.pos += 1;
                continue;
            }
            self.w.write(true, ctx[0]);
            let mut p = first;
            while p.token == ZERO_TOKEN {
                self.w.write(false, probs_of(self.fc, &p)[1]);
                self.pos += 1;
                match self.tokens.get(self.pos) {
                    None => return,
                    Some(t) if t.token == EOSB_TOKEN => {
                        self.pos += 1;
                        return;
                    }
                    Some(&t) => p = t,
                }
            }
            let ctx = probs_of(self.fc, &p);
            self.w.write(true, ctx[1]);
            if p.token == ONE_TOKEN {
                self.w.write(false, ctx[2]);
                self.w.write_bit(p.extra & 1 != 0);
            } else {
                self.w.write(true, ctx[2]);
                let code = COEF_ENCODINGS
                    .get(usize::from(p.token))
                    .copied()
                    .unwrap_or_default();
                let pareto = tables::PARETO8_FULL
                    .get(usize::from(ctx[2]).wrapping_sub(1))
                    .copied()
                    .unwrap_or([128; 8]);
                self.w.write_tree(
                    &COEF_CON_TREE,
                    &pareto,
                    code.value,
                    code.len.saturating_sub(UNCONSTRAINED_NODES as u32),
                    0,
                );
                if p.token >= CATEGORY1_TOKEN {
                    let cat = usize::from(p.token - CATEGORY1_TOKEN);
                    let probs: &[u8] = if p.token == CATEGORY6_TOKEN {
                        cat6
                    } else {
                        crate::enc::tokenize::CAT_PROBS
                            .get(cat)
                            .copied()
                            .unwrap_or(&[])
                    };
                    let v = p.extra >> 1;
                    let n = probs.len();
                    for (i, &prob) in probs.iter().enumerate() {
                        self.w.write((v >> (n - 1 - i)) & 1 != 0, prob);
                    }
                }
                self.w.write_bit(p.extra & 1 != 0);
            }
            self.pos += 1;
        }
    }

    /// libvpx's `write_modes` over one tile.
    fn write_tile(&mut self, rows: core::ops::Range<usize>, cols: core::ops::Range<usize>) {
        let mut mi_row = rows.start;
        while mi_row < rows.end {
            self.partition.new_row();
            let mut mi_col = cols.start;
            while mi_col < cols.end {
                self.write_modes_sb(mi_row, mi_col, BLOCK_64X64);
                mi_col += 8;
            }
            mi_row += 8;
        }
    }
}

/// Everything a frame is written from.
pub(crate) struct Frame<'a> {
    pub header: FrameHeader,
    pub lf: &'a mut LoopFilterState,
    /// The segmentation: its map's coding is chosen here.
    pub seg: &'a mut Segmentation,
    /// The frame's probabilities: its updates are made in them.
    pub fc: &'a mut FrameContext,
    pub counts: &'a EncCounts,
    pub search: UpdateSearch,
    /// The frame's blocks: each records whether its segment is predicted.
    pub mi: &'a mut MiGrid,
    pub ext: &'a [BlockExt],
    /// The last frame's segment map, which an inter frame's may be
    /// predicted from.
    pub last_seg_map: &'a [u8],
    /// Each tile's tokens, tile rows first: an end-of-block-tokens marker
    /// after each block's.
    pub tile_tokens: &'a [Vec<TokenExtra>],
}

/// libvpx's `vp9_pack_bitstream`: the uncompressed header, the compressed
/// header's size and the header itself, then the tiles, each but the last
/// after its size.
pub(crate) fn pack_frame(f: Frame<'_>) -> Vec<u8> {
    let h = f.header;
    if f.seg.enabled && f.seg.update_map {
        choose_segmap_coding_method(f.seg, f.mi, f.last_seg_map, h.key_frame, h.log2_tile_cols);
    }
    let mut wb = BitWriter::new();
    write_uncompressed_header(&mut wb, &h, f.lf, f.seg);
    let compressed = write_compressed_header(&h, f.fc, f.counts, f.search);
    // The compressed header's size, 16 bits: libvpx writes a placeholder
    // and fills it in; here the header is written first.
    wb.write_literal(u32::try_from(compressed.len()).unwrap_or(0).min(0xffff), 16);
    let mut out = wb.finish();
    out.extend_from_slice(&compressed);

    // encode_tiles.
    let mi_cols = h.mi_cols();
    let mi_rows = h.mi_rows();
    let tile_cols = 1usize << h.log2_tile_cols;
    let tile_rows = 1usize << h.log2_tile_rows;
    let mut partition = PartitionContexts::new(mi_cols);
    let offset = |i: usize, mis: usize, log2: u32| {
        header::tile_offset(
            u32::try_from(i).unwrap_or(u32::MAX),
            u32::try_from(mis).unwrap_or(u32::MAX),
            log2,
        ) as usize
    };
    let mi: &MiGrid = f.mi;
    let seg: &Segmentation = f.seg;
    let fc: &FrameContext = f.fc;
    for tile_row in 0..tile_rows {
        for tile_col in 0..tile_cols {
            let tokens = f
                .tile_tokens
                .get(tile_row * tile_cols + tile_col)
                .map_or(&[][..], Vec::as_slice);
            let cols = offset(tile_col, mi_cols, h.log2_tile_cols)
                ..offset(tile_col + 1, mi_cols, h.log2_tile_cols);
            let rows = offset(tile_row, mi_rows, h.log2_tile_rows)
                ..offset(tile_row + 1, mi_rows, h.log2_tile_rows);
            let mut tw = TileWriter {
                h: &h,
                fc,
                seg,
                mi,
                ext: f.ext,
                tokens,
                pos: 0,
                partition: &mut partition,
                mi_col_start: cols.start,
                w: BoolWriter::new(),
            };
            tw.write_tile(rows, cols);
            debug_assert_eq!(tw.pos, tokens.len(), "a tile's tokens were not all written");
            let data = tw.w.finish();
            if tile_col + 1 < tile_cols || tile_row + 1 < tile_rows {
                let size = u32::try_from(data.len()).unwrap_or(u32::MAX);
                out.extend_from_slice(&size.to_be_bytes());
            }
            out.extend_from_slice(&data);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a test: a failure should be loud"
    )]

    use super::*;
    use crate::common::INTRA_MODES;
    use crate::enc::writer::tokens_from_tree;

    #[test]
    fn the_codes_are_the_trees() {
        assert_eq!(
            tokens_from_tree(&INTRA_MODE_TREE, INTRA_MODES),
            INTRA_MODE_ENCODINGS
        );
        assert_eq!(tokens_from_tree(&INTER_MODE_TREE, 4), INTER_MODE_ENCODINGS);
        assert_eq!(
            tokens_from_tree(&SWITCHABLE_INTERP_TREE, 3),
            SWITCHABLE_INTERP_ENCODINGS
        );
        assert_eq!(tokens_from_tree(&PARTITION_TREE, 4), PARTITION_ENCODINGS);
        assert_eq!(tokens_from_tree(&COEF_TREE, 12), COEF_ENCODINGS);
    }

    #[test]
    fn transform_counts_become_branch_counts() {
        let mut t = TxCounts::default();
        t.p8x8[1] = [3, 4];
        t.p16x16[0] = [1, 2, 3];
        t.p32x32[1] = [1, 2, 3, 4];
        let (b8, _, b32) = tx_branch_counts(&t, 1);
        assert_eq!(b8, [[3, 4]]);
        assert_eq!(b32, [[1, 9], [2, 7], [3, 4]]);
        let (_, b16, _) = tx_branch_counts(&t, 0);
        assert_eq!(b16, [[1, 5], [2, 3]]);
    }

    #[test]
    fn tile_info_reads_back() {
        use crate::bits::BitReader;
        // 1920 wide: 240 cells, 0 to 2 log2 columns allowed.
        for (cols, rows) in [(0, 0), (1, 1), (2, 2)] {
            let mut wb = BitWriter::new();
            write_tile_info(&mut wb, 240, cols, rows);
            let data = wb.finish();
            let mut rb = BitReader::new(&data);
            assert_eq!(header::read_tile_info(&mut rb, 240).unwrap(), (cols, rows));
        }
    }

    #[test]
    fn a_frame_of_one_filter_codes_it_once() {
        let mut c = Counts::default();
        assert_eq!(fix_interp_filter(SWITCHABLE, &c), SWITCHABLE);
        c.switchable_interp[2][1] = 5;
        assert_eq!(fix_interp_filter(SWITCHABLE, &c), 1);
        c.switchable_interp[0][0] = 1;
        assert_eq!(fix_interp_filter(SWITCHABLE, &c), SWITCHABLE);
        // A filter the frame already fixed stays.
        assert_eq!(fix_interp_filter(2, &c), 2);
    }

    #[test]
    fn segment_trees_follow_their_counts() {
        // All in segment 0: every node but the first's other branches are
        // empty, and get_binary_prob makes an empty branch 128.
        let p = calc_segtree_probs(&[10, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(p, [255, 255, 128, 255, 128, 128, 128]);
        // A cost is a sum over the nodes the segments take.
        let c = [3, 1, 0, 0, 0, 0, 0, 2];
        let p = calc_segtree_probs(&c);
        let want = 4 * i64::from(cost_zero(p[0]))
            + 2 * i64::from(cost_one(p[0]))
            + 4 * i64::from(cost_zero(p[1]))
            + 3 * i64::from(cost_zero(p[3]))
            + i64::from(cost_one(p[3]))
            + 2 * i64::from(cost_one(p[2]))
            + 2 * i64::from(cost_one(p[6]));
        assert_eq!(cost_segmap(&c, &p), want);
    }
}
