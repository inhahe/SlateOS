//! The realtime inter mode search: libvpx's non-RD pick for a block of an
//! inter frame (`vp9_pick_inter_mode`).
//!
//! The search tries a fixed list of reference and mode pairs -- zero
//! motion, the nearest and near predicted vectors and a new vector, from
//! the last frame and, where it may help, the golden frame -- each judged by
//! a model of its rate and distortion from the residual's variance (or, for
//! large blocks, from a Hadamard transform of it), plus what the mode,
//! reference and vector cost to code. Many candidates are never tried: a
//! mode whose adaptive threshold says it rarely wins, a reference whose
//! predicted vectors match far worse than another's, the golden frame where
//! the last one already matches almost exactly. A block whose residual
//! would quantise to nothing ends the search early. Then, unless an inter
//! mode is already good enough, the intra modes the speed allows.
//!
//! Every shortcut, threshold and stale value of libvpx's is reproduced: the
//! modes chosen are stream decisions, and the encoder must make libvpx's.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_pickmode.c`
//! (`vp9_pick_inter_mode`, `find_predictors`, `combined_motion_search`,
//! `search_new_mv`, `search_filter_ref`, `model_rd_for_sb_y`,
//! `model_rd_for_sb_y_large`, `model_rd_for_sb_uv`, `calculate_tx_size`,
//! `block_variance`, `calculate_variance`, `encode_breakout_test`,
//! `vp9_NEWMV_diff_bias`, `get_force_skip_low_temp_var`,
//! `compute_intra_yprediction`, `init_ref_frame_cost`,
//! `update_thresh_freq_fact`) (copyright the WebM project authors), used
//! under libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "libvpx's arithmetic on rates (sums of 13-bit costs), distortions and variances of 8-bit residuals over at most 64x64, kept in its own integer widths and unsigned wrap-arounds"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "indices are reference frames (0..4), inter mode offsets (0..4), block sizes (0..13) and fixed-size per-block arrays"
)]

use crate::block::{ModeInfo, uv_tx_size};
use crate::common::{
    ALTREF_FRAME, BLOCK_8X8, BLOCK_16X16, BLOCK_32X32, BLOCK_64X64, BLOCK_SIZES, BlockSize,
    DC_PRED, EIGHTTAP, EIGHTTAP_SMOOTH, GOLDEN_FRAME, H_PRED, INTRA_FRAME, InterpFilter,
    LAST_FRAME, Mv, NEARESTMV, NEARMV, NEWMV, PredictionMode, RefFrame, SEG_LVL_REF_FRAME,
    SWITCHABLE, TM_PRED, TX_8X8, TX_16X16, TxSize, V_PRED, ZEROMV,
};
use crate::context;
use crate::enc::cost::cost_bit;
use crate::enc::encodeframe::{BlockModes, FrameEncoder, InterModes};
use crate::enc::mcomp::{self, LumaRef, MV_COST_WEIGHT, MvCosts, MvLimits, QUARTER_PEL, Search};
use crate::enc::partition::{ContentState, NoiseLevel};
use crate::enc::pickmode::block_yrd_full;
use crate::enc::quantize::QuantSet;
use crate::enc::rd::{
    MAX_MODES, ModeCosts, RD_THRESH_INC, RD_THRESH_MAX_FACT, RDDIV_BITS, THR_DC, THR_H_PRED,
    THR_NEARA, THR_NEARESTA, THR_NEARESTG, THR_NEARESTMV, THR_NEARG, THR_NEARMV, THR_NEWA,
    THR_NEWG, THR_NEWMV, THR_TM, THR_V_PRED, THR_ZEROA, THR_ZEROG, THR_ZEROMV, intra_cost_penalty,
    model_rd_from_var_lapndz, rd_less_than_thresh, rdcost,
};
use crate::enc::variance;
use crate::tables;

/// libvpx's `skip_txfm` values: nothing skipped, the whole luma transform
/// (`SKIP_TXFM_AC_DC`), its AC coefficients only.
const SKIP_TXFM_NONE: u8 = 0;
const SKIP_TXFM_AC_DC: u8 = 1;
const SKIP_TXFM_AC_ONLY: u8 = 2;

/// `VP9_LAST_FLAG`, `VP9_GOLD_FLAG` and `VP9_ALT_FLAG`: which references a
/// frame may use.
pub(crate) const LAST_FLAG: u8 = 1;
pub(crate) const GOLD_FLAG: u8 = 2;
pub(crate) const ALT_FLAG: u8 = 4;

/// The flag of a reference: libvpx's `ref_frame_to_flag`.
fn ref_flag(r: RefFrame) -> u8 {
    match r {
        LAST_FRAME => LAST_FLAG,
        GOLDEN_FRAME => GOLD_FLAG,
        _ => ALT_FLAG,
    }
}

/// libvpx's `ref_mode_set`: the pairs the search tries, in order.
const REF_MODE_SET: [(RefFrame, PredictionMode); 12] = [
    (LAST_FRAME, ZEROMV),
    (LAST_FRAME, NEARESTMV),
    (GOLDEN_FRAME, ZEROMV),
    (LAST_FRAME, NEARMV),
    (LAST_FRAME, NEWMV),
    (GOLDEN_FRAME, NEARESTMV),
    (GOLDEN_FRAME, NEARMV),
    (GOLDEN_FRAME, NEWMV),
    (ALTREF_FRAME, ZEROMV),
    (ALTREF_FRAME, NEARESTMV),
    (ALTREF_FRAME, NEARMV),
    (ALTREF_FRAME, NEWMV),
];

/// libvpx's `mode_idx`: each reference's modes' threshold slots, by
/// `mode_offset` (intra: DC, V, H, TM; inter: nearest, near, zero, new).
const MODE_IDX: [[usize; 4]; 4] = [
    [THR_DC, THR_V_PRED, THR_H_PRED, THR_TM],
    [THR_NEARESTMV, THR_NEARMV, THR_ZEROMV, THR_NEWMV],
    [THR_NEARESTG, THR_NEARG, THR_ZEROG, THR_NEWG],
    [THR_NEARESTA, THR_NEARA, THR_ZEROA, THR_NEWA],
];

/// libvpx's `intra_mode_list`.
const INTRA_MODE_LIST: [PredictionMode; 4] = [DC_PRED, V_PRED, H_PRED, TM_PRED];

/// libvpx's `mode_offset`.
fn mode_offset(mode: PredictionMode) -> usize {
    if mode >= NEARESTMV {
        usize::from(mode - NEARESTMV)
    } else {
        match mode {
            V_PRED => 1,
            H_PRED => 2,
            TM_PRED => 3,
            _ => 0,
        }
    }
}

/// libvpx's `pos_shift_16x16`: a 16x16 block's flag in `variance_low`, by
/// its row and column in the superblock.
const POS_SHIFT_16X16: [[usize; 4]; 4] = [
    [9, 10, 13, 14],
    [11, 12, 15, 16],
    [17, 18, 21, 22],
    [19, 20, 23, 24],
];

/// Whether the partitioning found this block's area barely changed since
/// the last frame: libvpx's `get_force_skip_low_temp_var`.
fn force_skip_low_temp_var(
    low: &[bool; 25],
    mi_row: usize,
    mi_col: usize,
    bsize: BlockSize,
) -> bool {
    let i = (mi_row & 7) >> 1;
    let j = (mi_col & 7) >> 1;
    let (top, left) = (mi_row & 7 == 0, mi_col & 7 == 0);
    match bsize {
        BLOCK_64X64 => low[0],
        crate::common::BLOCK_64X32 => {
            if left && top {
                low[1]
            } else if left && !top {
                low[2]
            } else {
                false
            }
        }
        crate::common::BLOCK_32X64 => {
            if left && top {
                low[3]
            } else if !left && top {
                low[4]
            } else {
                false
            }
        }
        BLOCK_32X32 => match (left, top) {
            (true, true) => low[5],
            (false, true) => low[6],
            (true, false) => low[7],
            (false, false) => low[8],
        },
        BLOCK_16X16 => low[POS_SHIFT_16X16[i][j]],
        crate::common::BLOCK_32X16 => {
            let j2 = ((mi_col + 2) & 7) >> 1;
            low[POS_SHIFT_16X16[i][j]] && low[POS_SHIFT_16X16[i][j2]]
        }
        crate::common::BLOCK_16X32 => {
            let i2 = ((mi_row + 2) & 7) >> 1;
            low[POS_SHIFT_16X16[i][j]] && low[POS_SHIFT_16X16[i2][j]]
        }
        _ => false,
    }
}

/// What libvpx's `MACROBLOCK` carries from the superblock's partitioning
/// into its blocks' searches.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SbState {
    /// The superblock's temporal content (`content_state_sb`). (libvpx also
    /// notes here whether the superblock's source changed at all,
    /// `zero_temp_sad_source`, and reads it only for screen content, which
    /// this encoder does not offer.)
    pub content_state: ContentState,
    pub skip_low_source_sad: bool,
    pub lowvar_highsumdiff: bool,
    /// For how many frames the superblock has been static, kept from the
    /// last superblock libvpx partitioned where it did not copy
    /// (`last_sb_high_content`).
    pub last_sb_high_content: i32,
    pub sb_is_skin: bool,
    pub color_sensitivity: [bool; 2],
    pub variance_low: [bool; 25],
    /// Use the superblock's own motion estimate instead of a search
    /// (`sb_use_mv_part`), and the estimate (eighth pixels).
    pub sb_use_mv_part: bool,
    pub sb_mvrow_part: i32,
    pub sb_mvcol_part: i32,
    /// The superblock's estimate per reference, a fourth candidate for the
    /// vector prediction (`x->pred_mv`); `None` is libvpx's `INT16_MAX`.
    pub pred_mv: [Option<Mv>; 4],
}

/// The frame-level settings and state the search reads: libvpx's
/// `VP9_COMP` fields and speed features for the frame.
pub(crate) struct SearchFrame<'a> {
    pub mv_costs: &'a MvCosts,
    pub mode_costs: &'a ModeCosts,
    /// The mode thresholds, by segment and block size (`rd->threshes`).
    pub threshes: &'a [[[i32; MAX_MODES]; BLOCK_SIZES]; 8],
    /// The references' luma, padded, for the searches.
    pub luma: [Option<&'a LumaRef>; 3],
    pub ref_frame_flags: u8,
    pub frames_since_golden: i32,
    pub avg_frame_low_motion: i32,
    /// libvpx's `rc.high_source_sad`.
    pub scene_change: bool,
    /// Most of the scene detection's sampled blocks moved: libvpx's
    /// `svc->high_num_blocks_with_motion`, which a single-layer encode
    /// copies from its rate control and which turns the encode breakout
    /// off.
    pub high_num_blocks_with_motion: bool,
    pub noise_enabled: bool,
    /// The noise estimate's settled level (`noise_estimate.level`).
    pub noise_level: NoiseLevel,
    /// The frame's rate-distortion multiplier, and the boosted segment's.
    pub rdmult: i32,
    pub cr_rdmult: i32,
    /// The encode runs cyclic refresh: libvpx's `aq_mode ==
    /// CYCLIC_REFRESH_AQ`, which holds on every frame -- on a scene cut too,
    /// which codes no segments. Whether a block is boosted is its segment's
    /// to say, and a frame without segments puts every block in segment 0.
    pub cyclic_refresh: bool,
    pub current_video_frame: u32,
    pub skip_encode_frame: bool,
    pub short_circuit_low_temp_var: i32,
    /// What kind of frame the multipliers are for (a golden refresh's are
    /// larger).
    pub rd_frame: crate::enc::rd::RdFrame,
    /// How far a mode's threshold may rise while it keeps losing (libvpx's
    /// `sf->adaptive_rd_thresh`): [`speed8_adaptive_rd_thresh`].
    pub adaptive_rd_thresh: i32,
    /// libvpx's `x->max_partition_size`: a block smaller than it also
    /// tries the vector its last search found as a start (`vp9_mv_pred`).
    /// The variance partitioning leaves the speed's default, 32x32; the
    /// learned partitioning sets 64x64 before each superblock.
    pub max_partition_size: BlockSize,
    pub base_qindex: i32,
    pub frame_width: u32,
    pub frame_height: u32,
}

/// Whether speed 8 searches the interpolation filter on only a chessboard
/// of blocks (libvpx's `sf->cb_pred_filter_search`, which speed 8 sets above
/// 352x288) rather than on every block.
pub(crate) fn speed8_cb_pred_filter_search(width: u32, height: u32) -> bool {
    u64::from(width) * u64::from(height) > 352 * 288
}

/// libvpx's `sf->adaptive_rd_thresh` at speed 8 for one-pass CBR on camera
/// content, the only configuration the encoder offers: 1 above 352x288,
/// where the low-variance short circuit already prunes modes, else 2.
pub(crate) fn speed8_adaptive_rd_thresh(width: u32, height: u32) -> i32 {
    if u64::from(width) * u64::from(height) > 352 * 288 {
        1
    } else {
        2
    }
}

/// The search's result: the modes, libvpx's `x->skip` among them, and the
/// best candidate's rate and distortion (the cyclic refresh weighs them).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Picked {
    pub modes: BlockModes,
    pub rate: i32,
    pub dist: i64,
    /// libvpx's `x->skip_txfm[0]` as the search left it (0 none, 1 the
    /// whole luma transform, 2 its AC only): what the decision trace shows.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "only the decision trace reads it")
    )]
    pub skip_txfm: u8,
}

/// libvpx's `RD_COST`.
#[derive(Clone, Copy, Debug)]
struct RdCost {
    rate: i32,
    dist: i64,
    rdcost: i64,
}

impl RdCost {
    const RESET: Self = Self {
        rate: i32::MAX,
        dist: i64::MAX,
        rdcost: i64::MAX,
    };
}

/// A block's quantiser settings: libvpx's `vp9_init_plane_quantizers` for
/// its segment.
#[derive(Clone, Copy, Debug)]
struct BlockQuant {
    y: QuantSet,
    uv: QuantSet,
    /// `x->errorperbit` and `x->sadperbit16`, from the segment's quantiser.
    errorperbit: i32,
    sadperbit16: i32,
}

/// The model of a luma prediction's rate and distortion: libvpx's
/// `model_rd_for_sb_y`'s and `_large`'s outputs.
#[derive(Clone, Copy, Debug, Default)]
struct YModel {
    rate: i32,
    dist: i64,
    var: u32,
    sse: u32,
    tx_size: TxSize,
    skip_txfm: u8,
}

/// The per-candidate state of a search: the block, its quantisers, the
/// pieces of `x` the helpers read and write.
struct Block<'s, 'a> {
    s: &'s SearchFrame<'a>,
    sb: &'s mut SbState,
    mi_row: usize,
    mi_col: usize,
    bsize: BlockSize,
    segment_id: u8,
    q: BlockQuant,
    rdmult: i32,
    /// The block's luma position and size in pixels.
    x: usize,
    y: usize,
    bw: usize,
    bh: usize,
    /// `x->skip_encode`: intra candidates predict from the source.
    skip_encode: bool,
    /// The candidate's UV predictions are in the reconstruction
    /// (`flag_preduv_computed`).
    preduv: [bool; 2],
}

impl FrameEncoder<'_> {
    /// The luma, U or V source and reconstruction at the block, with their
    /// strides.
    fn planes_at(&self, plane: usize, x: usize, y: usize) -> (&[u8], usize, &[u8], usize) {
        let s = &self.src.planes[plane.min(2)];
        let r = &self.recon.planes[plane.min(2)];
        (
            s.data.get(y * s.stride + x..).unwrap_or(&[]),
            s.stride,
            r.data.get(y * r.stride + x..).unwrap_or(&[]),
            r.stride,
        )
    }
}

/// libvpx's `ac_thr_factor`.
fn ac_thr_factor(speed: i32, width: u32, height: u32, norm_sum: i32) -> i64 {
    if speed >= 8 && norm_sum < 5 {
        if width <= 640 && height <= 480 { 4 } else { 2 }
    } else {
        1
    }
}

impl Block<'_, '_> {
    fn boosted(&self) -> bool {
        self.s.cyclic_refresh && (self.segment_id == 1 || self.segment_id == 2)
    }

    /// libvpx's `calculate_tx_size` at `TX_MODE_SELECT` (the realtime inter
    /// frames' mode), cyclic refresh on.
    fn calculate_tx_size(&self, var: u32, sse: u32, ac_thr: i64, is_intra: bool) -> TxSize {
        let var_thresh = if is_intra { ac_thr as u32 } else { 1 };
        // libvpx's x->source_variance stays UINT_MAX at speed 8.
        let limit_tx = !(self.s.cyclic_refresh && var < var_thresh);
        let max = tables::MAX_TXSIZE[usize::from(self.bsize)].min(crate::common::TX_32X32);
        // libvpx's `var << 2` is unsigned 32-bit, its high bits lost.
        let mut tx = if sse > var << 2 { max } else { TX_8X8 };
        if self.s.cyclic_refresh && limit_tx && self.boosted() {
            tx = TX_8X8;
        } else if tx > TX_16X16 && limit_tx {
            tx = TX_16X16;
        }
        tx
    }
}

/// libvpx's `model_rd_for_sb_y`: the luma prediction in the reconstruction
/// judged by its variance against the source.
fn model_rd_for_sb_y(f: &FrameEncoder<'_>, b: &Block<'_, '_>, is_intra: bool) -> YModel {
    let (src, ss, dst, ds) = f.planes_at(0, b.x, b.y);
    let (var, sse) = variance::variance(src, ss, dst, ds, b.bw, b.bh);
    let zbin = &b.q.y.zbin;
    let dc_thr = (i64::from(zbin[0]) * i64::from(zbin[0])) >> 6;
    let ac_thr = (i64::from(zbin[1]) * i64::from(zbin[1])) >> 6;
    let dc_quant = b.q.y.dequant[0] as u32;
    let ac_quant = b.q.y.dequant[1] as u32;
    let tx_size = b.calculate_tx_size(var, sse, ac_thr, is_intra);
    let unit = tables::TXSIZE_TO_BSIZE[usize::from(tx_size)];
    let bwl = u32::from(tables::B_WIDTH_LOG2[usize::from(b.bsize)]);
    let bhl = u32::from(tables::B_HEIGHT_LOG2[usize::from(b.bsize)]);
    let num_blk_log2 = (bwl - u32::from(tables::B_WIDTH_LOG2[usize::from(unit)]))
        + (bhl - u32::from(tables::B_HEIGHT_LOG2[usize::from(unit)]));
    let sse_tx = sse >> num_blk_log2;
    let var_tx = var >> num_blk_log2;
    let mut skip_dc = false;
    let mut skip_txfm = SKIP_TXFM_NONE;
    if i64::from(var_tx) < ac_thr || var == 0 {
        skip_txfm = SKIP_TXFM_AC_ONLY;
        if i64::from(sse_tx.wrapping_sub(var_tx)) < dc_thr || sse == var {
            skip_txfm = SKIP_TXFM_AC_DC;
        }
    } else if i64::from(sse_tx.wrapping_sub(var_tx)) < dc_thr || sse == var {
        skip_dc = true;
    }
    let mut m = YModel {
        rate: 0,
        dist: 0,
        var,
        sse,
        tx_size,
        skip_txfm,
    };
    if skip_txfm == SKIP_TXFM_AC_DC {
        m.dist = i64::from(sse) << 4;
        return m;
    }
    let n_log2 = u32::from(tables::NUM_PELS_LOG2[usize::from(b.bsize)]);
    if skip_dc {
        m.dist = i64::from(sse.wrapping_sub(var)) << 4;
    } else {
        let (rate, dist) = model_rd_from_var_lapndz(sse.wrapping_sub(var), n_log2, dc_quant >> 3);
        m.rate = rate >> 1;
        m.dist = dist << 3;
    }
    let (rate, dist) = model_rd_from_var_lapndz(var, n_log2, ac_quant >> 3);
    m.rate += rate;
    m.dist += dist << 4;
    m
}

/// libvpx's `block_variance` (8x8 sums and variances over the block) and
/// `calculate_variance` (four into one, a level up).
fn block_variance_8x8(
    src: &[u8],
    ss: usize,
    dst: &[u8],
    ds: usize,
    w: usize,
    h: usize,
    sse8x8: &mut [u32; 64],
    sum8x8: &mut [i32; 64],
    var8x8: &mut [u32; 64],
) -> (u32, i32) {
    let (mut sse, mut sum) = (0u32, 0i32);
    let mut k = 0;
    for i in (0..h).step_by(8) {
        for j in (0..w).step_by(8) {
            let (s8, m8) = variance::sse_sum(
                src.get(i * ss + j..).unwrap_or(&[]),
                ss,
                dst.get(i * ds + j..).unwrap_or(&[]),
                ds,
                8,
                8,
            );
            sse8x8[k] = s8;
            sum8x8[k] = m8;
            sse = sse.wrapping_add(s8);
            sum += m8;
            let k_sqr = ((i64::from(m8) * i64::from(m8)) >> 6) as u32;
            var8x8[k] = s8.abs_diff(k_sqr);
            k += 1;
        }
    }
    (sse, sum)
}

fn calculate_variance(
    bwl: u32,
    bhl: u32,
    unit: BlockSize,
    sse_i: &[u32],
    sum_i: &[i32],
    var_o: &mut [u32],
    sse_o: &mut [u32],
    sum_o: &mut [i32],
) {
    let ubw = u32::from(tables::B_WIDTH_LOG2[usize::from(unit)]);
    let ubh = u32::from(tables::B_HEIGHT_LOG2[usize::from(unit)]);
    let nw = 1usize << (bwl - ubw);
    let nh = 1usize << (bhl - ubh);
    let mut k = 0;
    for i in (0..nh).step_by(2) {
        for j in (0..nw).step_by(2) {
            let at = |r: usize, c: usize| r * nw + c;
            sse_o[k] = sse_i[at(i, j)]
                .wrapping_add(sse_i[at(i, j + 1)])
                .wrapping_add(sse_i[at(i + 1, j)])
                .wrapping_add(sse_i[at(i + 1, j + 1)]);
            sum_o[k] = sum_i[at(i, j)]
                + sum_i[at(i, j + 1)]
                + sum_i[at(i + 1, j)]
                + sum_i[at(i + 1, j + 1)];
            let k_sqr = ((i64::from(sum_o[k]) * i64::from(sum_o[k])) >> (ubw + ubh + 6)) as u32;
            var_o[k] = sse_o[k].abs_diff(k_sqr);
            k += 1;
        }
    }
}

/// libvpx's `model_rd_for_sb_y_large`: for large blocks, the variance per
/// transform block decides which coefficients would quantise away; where
/// all of luma would, chroma is checked too, and if it would as well the
/// search can stop early (`early_term`). Builds the chroma predictions it
/// checks, into the reconstruction.
fn model_rd_for_sb_y_large(
    f: &mut FrameEncoder<'_>,
    b: &mut Block<'_, '_>,
    mi: &ModeInfo,
    early_term: &mut bool,
) -> YModel {
    let dc_quant = b.q.y.dequant[0] as u32;
    let ac_quant = b.q.y.dequant[1] as u32;
    let dc_thr = i64::from((dc_quant * dc_quant) >> 6);
    let mut ac_thr = i64::from((ac_quant * ac_quant) >> 6);
    let bwl = u32::from(tables::B_WIDTH_LOG2[usize::from(b.bsize)]);
    let bhl = u32::from(tables::B_HEIGHT_LOG2[usize::from(b.bsize)]);
    let num8x8 = 1usize << (bwl + bhl - 2);
    let mut sse8x8 = [0u32; 64];
    let mut sum8x8 = [0i32; 64];
    let mut var8x8 = [0u32; 64];
    let (sse, sum) = {
        let (src, ss, dst, ds) = f.planes_at(0, b.x, b.y);
        block_variance_8x8(
            src,
            ss,
            dst,
            ds,
            4 << bwl,
            4 << bhl,
            &mut sse8x8,
            &mut sum8x8,
            &mut var8x8,
        )
    };
    // libvpx truncates the square to 32 bits before shifting.
    let sum_sqr = ((i64::from(sum) * i64::from(sum)) as u32) >> (bwl + bhl + 4);
    let var = sse.abs_diff(sum_sqr);
    ac_thr *= ac_thr_factor(
        8,
        b.s.frame_width,
        b.s.frame_height,
        sum.abs() >> (bwl + bhl),
    );
    let mut tx_size = b.calculate_tx_size(var, sse, ac_thr, false);
    if tx_size < TX_8X8 {
        tx_size = TX_8X8;
    }
    let mut m = YModel {
        rate: 0,
        dist: 0,
        var,
        sse,
        tx_size,
        skip_txfm: SKIP_TXFM_NONE,
    };
    let mut skip_dc = false;
    {
        let mut sse16 = [0u32; 16];
        let mut sum16 = [0i32; 16];
        let mut var16 = [0u32; 16];
        let mut sse32 = [0u32; 4];
        let mut sum32 = [0i32; 4];
        let mut var32 = [0u32; 4];
        if tx_size >= TX_16X16 {
            calculate_variance(
                bwl,
                bhl,
                crate::common::BLOCK_8X8,
                &sse8x8,
                &sum8x8,
                &mut var16,
                &mut sse16,
                &mut sum16,
            );
        }
        if tx_size == crate::common::TX_32X32 {
            calculate_variance(
                bwl,
                bhl,
                crate::common::BLOCK_16X16,
                &sse16,
                &sum16,
                &mut var32,
                &mut sse32,
                &mut sum32,
            );
        }
        let (num, sse_tx, var_tx): (usize, &[u32], &[u32]) = match tx_size {
            TX_8X8 => (num8x8, &sse8x8, &var8x8),
            TX_16X16 => (num8x8 >> 2, &sse16, &var16),
            _ => (num8x8 >> 4, &sse32, &var32),
        };
        let ac_test = (0..num).all(|k| i64::from(var_tx[k]) < ac_thr || var == 0);
        let dc_test =
            (0..num).all(|k| i64::from(sse_tx[k].wrapping_sub(var_tx[k])) < dc_thr || sse == var);
        if ac_test {
            m.skip_txfm = SKIP_TXFM_AC_ONLY;
            if dc_test {
                m.skip_txfm = SKIP_TXFM_AC_DC;
            }
        } else if dc_test {
            skip_dc = true;
        }
    }
    if m.skip_txfm == SKIP_TXFM_AC_DC {
        m.rate = 0;
        m.dist = i64::from(sse) << 4;
        // Chroma's skip test, each plane's prediction built for it.
        let mut skip_uv = [false; 2];
        for i in 1..=2usize {
            let uv_tx = uv_tx_size(b.bsize, tx_size, 1, 1);
            let unit = tables::TXSIZE_TO_BSIZE[usize::from(uv_tx)];
            let uv_bsize = tables::SS_SIZE[usize::from(b.bsize)][1][1];
            let uv_bw = u32::from(tables::B_WIDTH_LOG2[usize::from(uv_bsize)]);
            let uv_bh = u32::from(tables::B_HEIGHT_LOG2[usize::from(uv_bsize)]);
            let sf = (uv_bw - u32::from(tables::B_WIDTH_LOG2[usize::from(unit)]))
                + (uv_bh - u32::from(tables::B_HEIGHT_LOG2[usize::from(unit)]));
            let dq = b.q.uv.dequant;
            let uv_dc_thr = ((dq[0] as u32) * (dq[0] as u32)) >> (6 - sf);
            let uv_ac_thr = ((dq[1] as u32) * (dq[1] as u32)) >> (6 - sf);
            f.predict_inter(b.mi_row, b.mi_col, b.bsize, mi, i..i + 1);
            b.preduv[i - 1] = true;
            let (src, ss, dst, ds) = f.planes_at(i, b.x >> 1, b.y >> 1);
            let (var_uv, sse_uv) = variance::variance(src, ss, dst, ds, b.bw >> 1, b.bh >> 1);
            if (var_uv < uv_ac_thr || var_uv == 0)
                && (sse_uv.wrapping_sub(var_uv) < uv_dc_thr || sse_uv == var_uv)
            {
                skip_uv[i - 1] = true;
            } else {
                break;
            }
        }
        if skip_uv[0] && skip_uv[1] {
            *early_term = true;
        }
        return m;
    }
    let n_log2 = u32::from(tables::NUM_PELS_LOG2[usize::from(b.bsize)]);
    if skip_dc {
        m.rate = 0;
        m.dist = i64::from(sse.wrapping_sub(var)) << 4;
    } else {
        let (rate, dist) = model_rd_from_var_lapndz(sse.wrapping_sub(var), n_log2, dc_quant >> 3);
        m.rate = rate >> 1;
        m.dist = dist << 3;
    }
    let (rate, dist) = model_rd_from_var_lapndz(var, n_log2, ac_quant >> 3);
    m.rate += rate;
    m.dist += dist << 4;
    m
}

/// libvpx's `model_rd_for_sb_uv` over U and V where the superblock is
/// colour-sensitive: their predictions' modelled rate and distortion, and
/// `var_y` and `sse_y` grown by theirs.
fn model_rd_for_sb_uv(
    f: &FrameEncoder<'_>,
    b: &Block<'_, '_>,
    var_y: &mut u32,
    sse_y: &mut u32,
) -> (i32, i64) {
    let uv_bsize = tables::SS_SIZE[usize::from(b.bsize)][1][1];
    let n_log2 = u32::from(tables::NUM_PELS_LOG2[usize::from(uv_bsize)]);
    let (mut rate, mut dist) = (0i32, 0i64);
    let (mut tot_var, mut tot_sse) = (*var_y, *sse_y);
    for i in 1..=2usize {
        if !b.sb.color_sensitivity[i - 1] {
            continue;
        }
        let (src, ss, dst, ds) = f.planes_at(i, b.x >> 1, b.y >> 1);
        let (var, sse) = variance::variance(src, ss, dst, ds, b.bw >> 1, b.bh >> 1);
        tot_var = tot_var.wrapping_add(var);
        tot_sse = tot_sse.wrapping_add(sse);
        let dq = b.q.uv.dequant;
        let (r, d) = model_rd_from_var_lapndz(sse.wrapping_sub(var), n_log2, (dq[0] as u32) >> 3);
        rate += r >> 1;
        dist += d << 3;
        let (r, d) = model_rd_from_var_lapndz(var, n_log2, (dq[1] as u32) >> 3);
        rate += r;
        dist += d << 4;
    }
    *var_y = tot_var;
    *sse_y = tot_sse;
    (rate, dist)
}

/// The inter search of the block `bsize` at (`mi_row`, `mi_col`) in
/// segment `segment_id`: libvpx's `vp9_pick_inter_mode` at speed 8 for a
/// single-layer constant-bitrate encode without lag. `thresh_freq_fact` is
/// the tile's adaptive mode thresholds for `bsize`, updated by the result.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub(crate) fn pick_inter_mode(
    f: &mut FrameEncoder<'_>,
    s: &SearchFrame<'_>,
    sb: &mut SbState,
    thresh_freq_fact: &mut [i32; MAX_MODES],
    mi_row: usize,
    mi_col: usize,
    bsize: BlockSize,
    segment_id: u8,
) -> Picked {
    let seg = f.seg;
    let qindex = seg.qindex(segment_id, s.base_qindex).clamp(0, 255);
    let qi = usize::try_from(qindex).unwrap_or(0);
    let seg_rdmult = crate::enc::rd::compute_rd_mult(qindex, s.rd_frame);
    let q = BlockQuant {
        y: f.quants.get(0, qi),
        uv: f.quants.get(1, qi),
        errorperbit: crate::enc::rd::error_per_bit(seg_rdmult),
        sadperbit16: crate::enc::rd::sad_per_bit16(qindex),
    };
    let n4w = usize::from(tables::NUM_4X4_WIDE[usize::from(bsize)]);
    let n4h = usize::from(tables::NUM_4X4_HIGH[usize::from(bsize)]);
    let mut b = Block {
        s,
        sb,
        mi_row,
        mi_col,
        bsize,
        segment_id,
        q,
        rdmult: s.rdmult,
        x: mi_col * 8,
        y: mi_row * 8,
        bw: 4 * n4w,
        bh: 4 * n4h,
        skip_encode: s.skip_encode_frame && qindex < 115,
        preduv: [false; 2],
    };
    if s.cyclic_refresh && b.boosted() {
        b.rdmult = s.cr_rdmult;
    }
    let rdmult = b.rdmult;
    let rdc = |rate: i32, dist: i64| rdcost(rdmult, RDDIV_BITS, rate, dist);
    let place = f.placement(mi_row, mi_col, bsize);
    let (above, left) = f.neighbours(mi_row, mi_col);
    let a = above.and_then(|i| f.mi.blocks.get(i)).copied();
    let l = left.and_then(|i| f.mi.blocks.get(i)).copied();
    let skip_prob = f.fc.skip_probs[context::skip_context(a.as_ref(), l.as_ref())];
    let interp_ctx = context::switchable_interp_context(a.as_ref(), l.as_ref());
    let switchable_rate = |filter: InterpFilter| {
        s.mode_costs.switchable_interp[interp_ctx][usize::from(filter.min(2))]
    };

    let intra_penalty = intra_cost_penalty(
        bsize,
        s.base_qindex,
        0,
        s.noise_enabled && s.noise_level == NoiseLevel::High,
    );
    let inter_mode_thresh = rdc(intra_penalty, 0);
    let rd_threshes = &s.threshes[usize::from(segment_id.min(7))][usize::from(bsize)];
    let frame_interp = f.inter.map_or(SWITCHABLE, |i| i.interp_filter);
    // cb_pred_filter_search: above 352x288 a chessboard of blocks, the
    // squares changing places every frame, searches the filter; at and
    // below it every block does.
    let bsl = u32::from(tables::MI_WIDTH_LOG2[usize::from(bsize)]);
    let pred_filter_search = frame_interp == SWITCHABLE
        && (!speed8_cb_pred_filter_search(s.frame_width, s.frame_height)
            || ((((mi_row + mi_col) >> bsl) + (s.current_video_frame as usize & 1)) & 1) != 0);
    // filter_ref: the above block's filter, else the left's, if inter.
    let filter_ref = if let Some(m) = a.as_ref().filter(|m| m.is_inter()) {
        m.interp_filter
    } else if let Some(m) = l.as_ref().filter(|m| m.is_inter()) {
        m.interp_filter
    } else {
        frame_interp
    };
    let mut best_rdc = RdCost::RESET;
    let max_tx = tables::MAX_TXSIZE[usize::from(bsize)];
    let biggest = tables::TX_MODE_TO_BIGGEST_TX_SIZE[usize::from(f.h.tx_mode)];
    let mut best_mode = ZEROMV;
    let mut best_ref = LAST_FRAME;
    let mut best_tx_size: TxSize = max_tx.min(biggest);
    let mut best_intra_tx_size: TxSize = best_tx_size;
    let mut best_pred_filter: InterpFilter = EIGHTTAP;
    let mut best_mode_skip_txfm = SKIP_TXFM_NONE;
    let mut best_early_term = false;
    let mut x_skip = false;

    // The references usable.
    let gf_temporal_ref = true;
    let mut usable_ref_frame = if s.frames_since_golden == 0 && gf_temporal_ref {
        LAST_FRAME
    } else {
        GOLDEN_FRAME
    };
    let fslt = if s.short_circuit_low_temp_var != 0 {
        force_skip_low_temp_var(&b.sb.variance_low, mi_row, mi_col, bsize)
    } else {
        false
    };
    if (s.short_circuit_low_temp_var == 1 || s.short_circuit_low_temp_var == 3) && fslt {
        usable_ref_frame = LAST_FRAME;
    }
    let use_golden_nonzeromv = s.ref_frame_flags & GOLD_FLAG != 0 && !fslt;
    let lshc = b.sb.last_sb_high_content;
    if (s.frames_since_golden + 1) < lshc || lshc > 40 || s.frames_since_golden > 120 {
        usable_ref_frame = LAST_FRAME;
    }
    let seg_ref = if seg.feature_active(segment_id, SEG_LVL_REF_FRAME) {
        Some(seg.data(segment_id, SEG_LVL_REF_FRAME))
    } else {
        None
    };
    let thresh_skip_golden = 500u32;
    if seg_ref == Some(i32::from(GOLDEN_FRAME)) {
        usable_ref_frame = GOLDEN_FRAME;
    }

    #[cfg(test)]
    crate::enc::trace::line(|| {
        format!(
            "P {mi_row} {mi_col} bs={bsize} urf={usable_ref_frame} fslt={} lshc={lshc} ipen={intra_penalty} sef={}",
            u8::from(fslt),
            u8::from(b.skip_encode)
        )
    });
    // find_predictors per usable reference.
    let mut frame_mv = [[Mv::ZERO; 4]; 14];
    let mut mode_context = [0u8; 4];
    let mut ref_mvs = [[Mv::ZERO; 2]; 4];
    let mut skip_ref_find_pred = [false; 4];
    let mut ref_frame_skip_mask = 0u8;
    let mut pred_mv_sad = [i32::MAX; 4];
    let mut mv_best_ref_index = [0usize; 4];
    for r in LAST_FRAME..=usable_ref_frame {
        let ri = r as usize;
        skip_ref_find_pred[ri] = s.ref_frame_flags & ref_flag(r) == 0;
        if skip_ref_find_pred[ri] {
            continue;
        }
        frame_mv[usize::from(NEWMV)][ri] = Mv::INVALID;
        frame_mv[usize::from(ZEROMV)][ri] = Mv::ZERO;
        let Some(luma) = s.luma[ri - 1] else {
            ref_frame_skip_mask |= 1 << ri;
            continue;
        };
        let refs = f.mv_refs(mi_row, mi_col, bsize, r);
        frame_mv[usize::from(NEARESTMV)][ri] = refs.nearest;
        frame_mv[usize::from(NEARMV)][ri] = refs.near;
        ref_mvs[ri] = [refs.nearest, refs.near];
        mode_context[ri] = refs.mode_context;
        if bsize >= BLOCK_8X8 && !(fslt && r == GOLDEN_FRAME) {
            let src = &f.src.planes[0];
            let search = Search {
                src: src.data.get(b.y * src.stride + b.x..).unwrap_or(&[]),
                src_stride: src.stride,
                pre: luma,
                x: b.x as i32,
                y: b.y as i32,
                w: b.bw,
                h: b.bh,
            };
            let consider_pred_mv = bsize < s.max_partition_size;
            let (idx, _max_mv, sad) =
                mcomp::mv_pred(&search, ref_mvs[ri], b.sb.pred_mv[ri], consider_pred_mv);
            mv_best_ref_index[ri] = idx;
            pred_mv_sad[ri] = sad;
        }
    }
    #[cfg(test)]
    for r in LAST_FRAME..=usable_ref_frame {
        let ri = r as usize;
        if !skip_ref_find_pred[ri] {
            crate::enc::trace::line(|| {
                format!(
                    "R {r} nearest={},{} near={},{} ctx={} bri={} psad={}",
                    frame_mv[usize::from(NEARESTMV)][ri].row,
                    frame_mv[usize::from(NEARESTMV)][ri].col,
                    frame_mv[usize::from(NEARMV)][ri].row,
                    frame_mv[usize::from(NEARMV)][ri].col,
                    mode_context[ri],
                    mv_best_ref_index[ri],
                    pred_mv_sad[ri]
                )
            });
        }
    }
    if bsize < BLOCK_32X32 {
        b.sb.sb_use_mv_part = false;
    }

    let large_block = if b.sb.content_state == ContentState::VeryHighSad
        || (b.sb.sb_is_skin && s.avg_frame_low_motion > 70)
    {
        bsize > BLOCK_32X32
    } else {
        bsize >= BLOCK_32X32
    };
    let use_model_yrd_large = large_block && !b.boosted() && s.base_qindex != 0;
    let ref_frame_cost = ref_frame_costs(f, a.as_ref(), l.as_ref());

    let mut mode_checked = [[false; 4]; 14];
    let mut sse_zeromv_normalized = u32::MAX;
    let mut best_sse_sofar = u32::MAX;
    let mut best_pred_sad = i32::MAX;
    let bwl = u32::from(tables::B_WIDTH_LOG2[usize::from(bsize)]);
    let bhl = u32::from(tables::B_HEIGHT_LOG2[usize::from(bsize)]);
    let inter_mask: u32 = if bsize >= BLOCK_32X32 {
        (1 << NEARESTMV) | (1 << NEWMV) | (1 << ZEROMV)
    } else {
        u32::MAX
    };

    for (idx, &(ref_frame, this_mode)) in REF_MODE_SET.iter().enumerate() {
        let ri = ref_frame as usize;
        let mi_u = usize::from(this_mode);
        // Each candidate builds its own chroma predictions
        // (flag_preduv_computed is libvpx's per candidate).
        b.preduv = [false; 2];
        if ref_frame > usable_ref_frame || skip_ref_find_pred[ri] {
            continue;
        }
        if let Some(r) = seg_ref
            && r != i32::from(ref_frame)
        {
            continue;
        }
        if ref_frame == GOLDEN_FRAME && sse_zeromv_normalized < thresh_skip_golden {
            continue;
        }
        if s.ref_frame_flags & ref_flag(ref_frame) == 0 {
            continue;
        }
        if inter_mask & (1 << this_mode) == 0 {
            continue;
        }
        if fslt && ref_frame == GOLDEN_FRAME && frame_mv[mi_u][ri] != Mv::ZERO {
            continue;
        }
        if b.sb.content_state != ContentState::VeryHighSad
            && (s.short_circuit_low_temp_var >= 2
                || (s.short_circuit_low_temp_var == 1 && bsize == BLOCK_64X64))
            && fslt
            && ref_frame == LAST_FRAME
            && this_mode == NEWMV
        {
            continue;
        }
        if seg_ref.is_none() {
            if !(frame_mv[mi_u][ri] == Mv::ZERO && ref_frame == LAST_FRAME)
                && usable_ref_frame < ALTREF_FRAME
                && !fslt
                && usable_ref_frame > LAST_FRAME
            {
                let i = if ref_frame == LAST_FRAME {
                    GOLDEN_FRAME
                } else {
                    LAST_FRAME
                } as usize;
                if s.ref_frame_flags & ref_flag(i as RefFrame) != 0
                    && pred_mv_sad[i] < i32::MAX
                    && pred_mv_sad[ri] > (pred_mv_sad[i] << 1)
                {
                    ref_frame_skip_mask |= 1 << ri;
                }
            }
            if ref_frame_skip_mask & (1 << ri) != 0 {
                continue;
            }
        }
        let mode_index = MODE_IDX[ri][mode_offset(this_mode)];
        let mut mode_rd_thresh = if best_mode_skip_txfm != SKIP_TXFM_NONE {
            rd_threshes[mode_index] << 1
        } else {
            rd_threshes[mode_index]
        };
        if ref_frame == GOLDEN_FRAME && s.frames_since_golden > 4 {
            // bias_golden.
            mode_rd_thresh <<= 3;
        }
        if rd_less_than_thresh(
            best_rdc.rdcost,
            mode_rd_thresh,
            thresh_freq_fact[mode_index],
        ) && frame_mv[mi_u][ri] != Mv::ZERO
        {
            continue;
        }
        let mut rate_mv = 0i32;
        if this_mode == NEWMV {
            let ok = search_new_mv(
                f,
                &b,
                &mut frame_mv,
                ref_frame,
                &ref_mvs,
                &mode_context,
                &mv_best_ref_index,
                &pred_mv_sad,
                best_pred_sad,
                &mut rate_mv,
                best_rdc.rdcost,
            );
            if !ok {
                continue;
            }
        }
        // A duplicate zero vector is not searched twice.
        let mut skip_this_mv = false;
        for inter_mv_mode in NEARESTMV..=NEWMV {
            if inter_mv_mode == this_mode {
                continue;
            }
            let im = usize::from(inter_mv_mode);
            if mode_checked[im][ri]
                && frame_mv[mi_u][ri] == frame_mv[im][ri]
                && frame_mv[im][ri] == Mv::ZERO
            {
                skip_this_mv = true;
                break;
            }
        }
        if skip_this_mv {
            continue;
        }
        if use_golden_nonzeromv
            && this_mode == NEWMV
            && ref_frame == LAST_FRAME
            && frame_mv[usize::from(NEWMV)][1] != Mv::INVALID
        {
            let mv = frame_mv[usize::from(NEWMV)][1];
            let search = luma_search(f, &b, s.luma[0]);
            if let Some(search) = search {
                best_pred_sad = search.sad(i32::from(mv.row) >> 3, i32::from(mv.col) >> 3) as i32;
                pred_mv_sad[1] = best_pred_sad;
            }
        }
        if this_mode != NEARESTMV && frame_mv[mi_u][ri] == frame_mv[usize::from(NEARESTMV)][ri] {
            continue;
        }
        let this_mv = frame_mv[mi_u][ri];
        let mut mi = ModeInfo {
            sb_type: bsize,
            mode: this_mode,
            ref_frame: [ref_frame, crate::common::NO_REF_FRAME],
            mv: [this_mv, Mv::ZERO],
            interp_filter: EIGHTTAP,
            segment_id,
            ..ModeInfo::default()
        };
        let subpel = (i32::from(this_mv.row) | i32::from(this_mv.col)) & 7 != 0;
        let mut this_early_term = false;
        let model;
        if (this_mode == NEWMV || filter_ref == SWITCHABLE)
            && pred_filter_search
            && ref_frame == LAST_FRAME
            && subpel
        {
            model = search_filter_ref(
                f,
                &mut b,
                &mut mi,
                use_model_yrd_large,
                &mut this_early_term,
                &switchable_rate,
            );
        } else {
            mi.interp_filter = if filter_ref == SWITCHABLE {
                EIGHTTAP
            } else {
                filter_ref
            };
            f.predict_inter(mi_row, mi_col, bsize, &mi, 0..1);
            model = if use_model_yrd_large {
                model_rd_for_sb_y_large(f, &mut b, &mi, &mut this_early_term)
            } else {
                model_rd_for_sb_y(f, &b, false)
            };
            if ref_frame == LAST_FRAME && this_mv == Mv::ZERO {
                sse_zeromv_normalized = model.sse >> (bwl + bhl);
            }
            if model.sse < best_sse_sofar {
                best_sse_sofar = model.sse;
            }
        }
        mi.tx_size = model.tx_size;
        // libvpx's var_y and sse_y, which the chroma model grows.
        let mut var_y = model.var;
        let mut sse_y = model.sse;
        let mut this_rdc = RdCost {
            rate: model.rate,
            dist: model.dist,
            rdcost: 0,
        };
        let mut skip_txfm = model.skip_txfm;
        if !this_early_term {
            let mut this_sse = i64::from(sse_y);
            let is_skippable = block_yrd(
                f,
                &b,
                &place,
                &mut this_rdc,
                &mut this_sse,
                mi.tx_size,
                false,
            );
            skip_txfm = u8::from(is_skippable);
            if is_skippable {
                this_rdc.rate = cost_bit(skip_prob, true) as i32;
            } else if rdc(this_rdc.rate, this_rdc.dist) < rdc(0, this_sse) {
                this_rdc.rate += cost_bit(skip_prob, false) as i32;
            } else {
                this_rdc.rate = cost_bit(skip_prob, true) as i32;
                this_rdc.dist = this_sse;
                skip_txfm = SKIP_TXFM_AC_DC;
            }
            if frame_interp == SWITCHABLE && subpel {
                this_rdc.rate += switchable_rate(mi.interp_filter);
            }
        } else {
            if frame_interp == SWITCHABLE && subpel {
                this_rdc.rate += switchable_rate(mi.interp_filter);
            }
            this_rdc.rate += cost_bit(skip_prob, true) as i32;
        }
        if !this_early_term && (b.sb.color_sensitivity[0] || b.sb.color_sensitivity[1]) {
            for i in 1..=2usize {
                if b.sb.color_sensitivity[i - 1] && !b.preduv[i - 1] {
                    f.predict_inter(mi_row, mi_col, bsize, &mi, i..i + 1);
                    b.preduv[i - 1] = true;
                }
            }
            let (r, d) = model_rd_for_sb_uv(f, &b, &mut var_y, &mut sse_y);
            this_rdc.rate += r;
            this_rdc.dist += d;
        }
        this_rdc.rate += rate_mv;
        this_rdc.rate += s.mode_costs.inter_mode[usize::from(mode_context[ri])]
            [usize::from(this_mode - NEARESTMV)];
        this_rdc.rate += ref_frame_cost[ri];
        this_rdc.rdcost = rdc(this_rdc.rate, this_rdc.dist);
        newmv_diff_bias(
            s,
            &b,
            a.as_ref(),
            l.as_ref(),
            this_mode,
            &mut this_rdc,
            frame_mv[mi_u][ri],
            ref_frame == LAST_FRAME,
        );
        // Encode breakout: a prediction this close codes nothing.
        if !f.h.lossless() && !s.scene_change && !s.high_num_blocks_with_motion {
            encode_breakout_test(
                f,
                &mut b,
                &mi,
                var_y,
                sse_y,
                &mut this_rdc,
                &mut x_skip,
                s.mode_costs.inter_mode[usize::from(mode_context[ri])]
                    [usize::from(this_mode - NEARESTMV)],
            );
            if x_skip {
                this_rdc.rate += rate_mv;
                this_rdc.rdcost = rdc(this_rdc.rate, this_rdc.dist);
            }
        }
        #[cfg(test)]
        crate::enc::trace::line(|| {
            format!(
                "M r={ref_frame} m={this_mode} mv={},{} f={} tx={} rate={} dist={} rd={} et={} skt={} sk={} rmv={}",
                frame_mv[mi_u][ri].row,
                frame_mv[mi_u][ri].col,
                mi.interp_filter,
                mi.tx_size,
                this_rdc.rate,
                this_rdc.dist,
                this_rdc.rdcost,
                u8::from(this_early_term),
                skip_txfm,
                u8::from(x_skip),
                rate_mv
            )
        });
        mode_checked[mi_u][ri] = true;
        if this_rdc.rdcost < best_rdc.rdcost || x_skip {
            best_rdc = this_rdc;
            best_early_term = this_early_term;
            best_mode = this_mode;
            best_pred_filter = mi.interp_filter;
            best_tx_size = mi.tx_size;
            best_ref = ref_frame;
            best_mode_skip_txfm = skip_txfm;
        }
        if x_skip {
            break;
        }
        if best_early_term && idx > 0 && !s.scene_change {
            x_skip = true;
            break;
        }
    }

    let mut best_mv = frame_mv[usize::from(best_mode)][best_ref as usize];
    let mut best_uv_mode = DC_PRED;

    // The intra modes, when no inter mode is good enough.
    let perform_intra_pred = !matches!(seg_ref, Some(r) if r > 0);
    let max_intra_bsize = BLOCK_32X32;
    if best_rdc.rdcost == i64::MAX
        || (s.scene_change && perform_intra_pred)
        || ((!fslt || bsize < BLOCK_32X32 || b.sb.content_state == ContentState::VeryHighSad)
            && perform_intra_pred
            && !x_skip
            && best_rdc.rdcost > inter_mode_thresh
            && bsize <= max_intra_bsize
            && !b.sb.skip_low_source_sad
            && !b.sb.lowvar_highsumdiff)
    {
        let y_mask: u32 = if bsize > BLOCK_16X16 {
            1 << DC_PRED
        } else {
            (1 << DC_PRED) | (1 << V_PRED) | (1 << H_PRED)
        };
        for &this_mode in &INTRA_MODE_LIST {
            let mode_index = MODE_IDX[0][mode_offset(this_mode)];
            let mode_rd_thresh = rd_threshes[mode_index];
            if y_mask & (1 << this_mode) == 0 {
                continue;
            }
            // rt_intra_dc_only_low_content.
            if this_mode != DC_PRED && b.sb.content_state != ContentState::VeryHighSad {
                continue;
            }
            if rd_less_than_thresh(
                best_rdc.rdcost,
                mode_rd_thresh,
                thresh_freq_fact[mode_index],
            ) {
                continue;
            }
            compute_intra_yprediction(f, &b, &place, this_mode);
            let model = model_rd_for_sb_y(f, &b, true);
            let mut this_rdc = RdCost {
                rate: model.rate,
                dist: model.dist,
                rdcost: 0,
            };
            let tx_size = model.tx_size;
            let mut this_sse = i64::MAX;
            let skippable = block_yrd(f, &b, &place, &mut this_rdc, &mut this_sse, tx_size, true);
            let skip_txfm = if skippable {
                this_rdc.rate = cost_bit(skip_prob, true) as i32;
                SKIP_TXFM_AC_DC
            } else {
                this_rdc.rate += cost_bit(skip_prob, false) as i32;
                SKIP_TXFM_NONE
            };
            this_rdc.rate += s.mode_costs.mbmode[usize::from(this_mode)];
            this_rdc.rate += ref_frame_cost[0];
            this_rdc.rate += intra_penalty;
            this_rdc.rdcost = rdc(this_rdc.rate, this_rdc.dist);
            #[cfg(test)]
            crate::enc::trace::line(|| {
                format!(
                    "I m={this_mode} tx={tx_size} rate={} dist={} rd={} skt={skip_txfm}",
                    this_rdc.rate, this_rdc.dist, this_rdc.rdcost
                )
            });
            if this_rdc.rdcost < best_rdc.rdcost {
                best_rdc = this_rdc;
                best_mode = this_mode;
                best_intra_tx_size = tx_size;
                best_ref = INTRA_FRAME;
                best_uv_mode = this_mode;
                best_mv = Mv::INVALID;
                best_mode_skip_txfm = skip_txfm;
            }
        }
    }

    let is_inter = best_ref != INTRA_FRAME;
    let tx_size = if is_inter {
        best_tx_size
    } else {
        best_intra_tx_size
    };

    // adaptive_rd_thresh: the winner's thresholds fall, the others' rise.
    let best_mode_idx = MODE_IDX[best_ref as usize][mode_offset(best_mode)];
    let mut update = |r: usize, mode: PredictionMode| {
        let thr = MODE_IDX[r][mode_offset(mode)];
        let fact = &mut thresh_freq_fact[thr];
        if thr == best_mode_idx {
            *fact -= *fact >> 4;
        } else {
            *fact = (*fact + RD_THRESH_INC).min(s.adaptive_rd_thresh * RD_THRESH_MAX_FACT);
        }
    };
    if is_inter {
        for mode in NEARESTMV..=NEWMV {
            update(best_ref as usize, mode);
        }
    } else {
        for &mode in &INTRA_MODE_LIST {
            update(0, mode);
        }
    }

    let modes = BlockModes {
        mode: best_mode,
        sub_modes: [best_mode; 4],
        uv_mode: best_uv_mode,
        tx_size,
        inter: is_inter.then_some(InterModes {
            ref_frame: best_ref,
            mode: best_mode,
            mv: best_mv,
            interp_filter: best_pred_filter,
            sub: [(best_mode, best_mv); 4],
        }),
        segment_id,
        skip: x_skip,
        skip_y: best_mode_skip_txfm == SKIP_TXFM_AC_DC,
    };
    let (rate, dist) = if best_rdc.rate == i32::MAX {
        (i32::MAX, i64::MAX)
    } else {
        (best_rdc.rate, best_rdc.dist)
    };
    Picked {
        modes,
        rate,
        dist,
        skip_txfm: best_mode_skip_txfm,
    }
}

/// A [`Search`] of the block over a reference's luma.
fn luma_search<'r>(
    f: &'r FrameEncoder<'_>,
    b: &Block<'_, '_>,
    luma: Option<&'r LumaRef>,
) -> Option<Search<'r>> {
    let pre = luma?;
    let src = &f.src.planes[0];
    Some(Search {
        src: src.data.get(b.y * src.stride + b.x..).unwrap_or(&[]),
        src_stride: src.stride,
        pre,
        x: b.x as i32,
        y: b.y as i32,
        w: b.bw,
        h: b.bh,
    })
}

/// What each reference costs to signal: libvpx's `init_ref_frame_cost`.
fn ref_frame_costs(f: &FrameEncoder<'_>, a: Option<&ModeInfo>, l: Option<&ModeInfo>) -> [i32; 4] {
    let intra_inter_p = f.fc.intra_inter_prob[context::intra_inter_context(a, l)];
    let p1 = f.fc.single_ref_prob[context::single_ref_p1_context(a, l)][0];
    let p2 = f.fc.single_ref_prob[context::single_ref_p2_context(a, l)][1];
    let c = |p: u8, bit: bool| cost_bit(p, bit) as i32;
    let inter = c(intra_inter_p, true);
    [
        c(intra_inter_p, false),
        inter + c(p1, false),
        inter + c(p1, true) + c(p2, false),
        inter + c(p1, true) + c(p2, true),
    ]
}

/// libvpx's `search_new_mv`: the new vector for `ref_frame`, or false where
/// the search finds it not worth trying. LAST searches (the fast diamond
/// from the best predicted vector, then sub-pixel); GOLDEN takes the
/// integral projections' estimate when it is no worse than LAST's
/// prediction, refined to sub-pixel.
#[allow(clippy::too_many_arguments)]
fn search_new_mv(
    f: &FrameEncoder<'_>,
    b: &Block<'_, '_>,
    frame_mv: &mut [[Mv; 4]; 14],
    ref_frame: RefFrame,
    ref_mvs: &[[Mv; 2]; 4],
    mode_context: &[u8; 4],
    mv_best_ref_index: &[usize; 4],
    pred_mv_sad: &[i32; 4],
    best_pred_sad: i32,
    rate_mv: &mut i32,
    best_rd_sofar: i64,
) -> bool {
    let s = b.s;
    let ri = ref_frame as usize;
    let Some(search) = luma_search(f, b, s.luma[ri - 1]) else {
        return false;
    };
    let block_limits = MvLimits::for_block(
        b.mi_row,
        b.mi_col,
        b.bh / 8,
        b.bw / 8,
        f.mi.mi_rows,
        f.mi.mi_cols,
    );
    let ref_mv = ref_mvs[ri][0];
    let allow_hp = f.inter.is_some_and(|i| i.allow_hp);
    let new = usize::from(NEWMV);
    if ref_frame > LAST_FRAME {
        // vp9_int_pro_motion_estimation, CBR with the golden reference.
        if b.bsize < BLOCK_16X16 {
            return false;
        }
        let bwl = u32::from(tables::B_WIDTH_LOG2[usize::from(b.bsize)]);
        let bhl = u32::from(tables::B_HEIGHT_LOG2[usize::from(b.bsize)]);
        let (tmp_sad, mv) =
            mcomp::int_pro_motion_estimation(&search, bwl, bhl, &block_limits, ref_mv);
        let tmp_sad = tmp_sad as i32;
        if tmp_sad > pred_mv_sad[1] {
            return false;
        }
        let n_log2 = i32::from(tables::NUM_PELS_LOG2[usize::from(b.bsize)]);
        if tmp_sad.saturating_add(n_log2 << 4) > best_pred_sad {
            return false;
        }
        *rate_mv = s.mv_costs.mv_bit_cost(mv, ref_mv, MV_COST_WEIGHT);
        let full = Mv {
            row: mv.row >> 3,
            col: mv.col >> 3,
        };
        let sub = mcomp::find_best_sub_pixel_tree(
            &search,
            &block_limits,
            full,
            ref_mv,
            allow_hp,
            b.q.errorperbit,
            QUARTER_PEL,
            s.mv_costs,
        );
        frame_mv[new][ri] = sub.mv;
        return true;
    }
    // combined_motion_search.
    let mut limits = block_limits;
    limits.set_search_range(ref_mv);
    if b.sb.lowvar_highsumdiff {
        limits.col_min = limits.col_min.max(-10);
        limits.row_min = limits.row_min.max(-10);
        limits.col_max = limits.col_max.min(10);
        limits.row_max = limits.row_max.min(10);
    }
    let mvp = if mv_best_ref_index[ri] < 2 {
        ref_mvs[ri][mv_best_ref_index[ri]]
    } else {
        b.sb.pred_mv[ri].unwrap_or(Mv {
            row: i16::MAX,
            col: i16::MAX,
        })
    };
    let mvp_full = Mv {
        row: mvp.row >> 3,
        col: mvp.col >> 3,
    };
    let tmp_mv = if b.sb.sb_use_mv_part {
        Mv {
            row: (b.sb.sb_mvrow_part >> 3) as i16,
            col: (b.sb.sb_mvcol_part >> 3) as i16,
        }
    } else {
        mcomp::fast_dia_search(
            &search,
            &limits,
            mvp_full,
            b.q.sadperbit16,
            ref_mv,
            s.mv_costs,
        )
        .1
    };
    let full_eighths = Mv {
        row: tmp_mv.row.wrapping_mul(8),
        col: tmp_mv.col.wrapping_mul(8),
    };
    *rate_mv = s.mv_costs.mv_bit_cost(full_eighths, ref_mv, MV_COST_WEIGHT);
    let rate_mode =
        s.mode_costs.inter_mode[usize::from(mode_context[ri])][usize::from(NEWMV - NEARESTMV)];
    let rv = rdcost(b.rdmult, RDDIV_BITS, *rate_mv + rate_mode, 0) <= best_rd_sofar;
    let mut out = tmp_mv;
    if rv {
        let sub = mcomp::find_best_sub_pixel_tree(
            &search,
            &block_limits,
            tmp_mv,
            ref_mv,
            allow_hp,
            b.q.errorperbit,
            QUARTER_PEL,
            s.mv_costs,
        );
        out = sub.mv;
        *rate_mv = s.mv_costs.mv_bit_cost(out, ref_mv, MV_COST_WEIGHT);
    }
    frame_mv[new][ri] = out;
    #[cfg(test)]
    crate::enc::trace::line(|| {
        format!(
            "N ref={ref_frame} rv={} mv={},{} rmv={}",
            u8::from(rv),
            out.row,
            out.col,
            *rate_mv
        )
    });
    rv
}

/// libvpx's `search_filter_ref`: the eighth-pixel vector's prediction with
/// the regular and the smooth filter, the cheaper kept. The
/// reconstruction is left holding the kept filter's luma (libvpx's
/// separate prediction buffers); chroma holds what the last model built.
fn search_filter_ref(
    f: &mut FrameEncoder<'_>,
    b: &mut Block<'_, '_>,
    mi: &mut ModeInfo,
    use_model_yrd_large: bool,
    this_early_term: &mut bool,
    switchable_rate: &dyn Fn(InterpFilter) -> i32,
) -> YModel {
    let mut best: Option<(i64, InterpFilter, YModel, bool, [bool; 2])> = None;
    for filter in EIGHTTAP..=EIGHTTAP_SMOOTH {
        mi.interp_filter = filter;
        f.predict_inter(b.mi_row, b.mi_col, b.bsize, mi, 0..1);
        let model = if use_model_yrd_large {
            model_rd_for_sb_y_large(f, b, mi, this_early_term)
        } else {
            model_rd_for_sb_y(f, b, false)
        };
        let rate = model.rate + switchable_rate(filter);
        let cost = rdcost(b.rdmult, RDDIV_BITS, rate, model.dist);
        if best.is_none_or(|(c, ..)| cost < c) {
            best = Some((cost, filter, model, *this_early_term, b.preduv));
        }
    }
    let Some((_, filter, model, early, preduv)) = best else {
        return YModel::default();
    };
    mi.interp_filter = filter;
    *this_early_term = early;
    b.preduv = preduv;
    if filter != EIGHTTAP_SMOOTH {
        // libvpx keeps each filter's prediction; here the kept one is
        // predicted again over the last.
        f.predict_inter(b.mi_row, b.mi_col, b.bsize, mi, 0..1);
    }
    model
}

/// libvpx's `block_yrd` for a block of the search: the simple model's
/// figures for blocks under 32x32 (no transform), else the transform's
/// estimate over the prediction in the reconstruction. Returns whether the
/// block quantises to nothing; `this_sse` as libvpx leaves it.
fn block_yrd(
    f: &FrameEncoder<'_>,
    b: &Block<'_, '_>,
    place: &crate::enc::encodeframe::Placement,
    rdc: &mut RdCost,
    this_sse: &mut i64,
    tx_size: TxSize,
    _is_intra: bool,
) -> bool {
    if b.bsize < BLOCK_32X32 {
        // use_simple_block_yrd: the model's rate and distortion stand.
        *this_sse = i64::from(i32::MAX);
        return false;
    }
    let (src, ss, dst, ds) = f.planes_at(0, b.x, b.y);
    let sse = (*this_sse < i64::MAX).then_some(&mut *this_sse);
    let (rate, dist, skippable) = block_yrd_full(
        src,
        ss,
        dst,
        ds,
        b.bsize,
        tx_size.min(TX_16X16),
        place,
        &b.q.y,
        sse,
    );
    rdc.rate = rate;
    rdc.dist = dist;
    skippable
}

/// libvpx's `vp9_NEWMV_diff_bias`: a new vector far from its neighbours'
/// costs more; and for noisy sources, small motion from the last frame
/// costs less.
#[allow(clippy::too_many_arguments)]
fn newmv_diff_bias(
    s: &SearchFrame<'_>,
    b: &Block<'_, '_>,
    above: Option<&ModeInfo>,
    left: Option<&ModeInfo>,
    this_mode: PredictionMode,
    rdc: &mut RdCost,
    mv: Mv,
    is_last_frame: bool,
) {
    let (mv_row, mv_col) = (i32::from(mv.row), i32::from(mv.col));
    if this_mode == NEWMV {
        let a = above.filter(|m| m.is_inter()).map(|m| m.mv[0]);
        let l = left.filter(|m| m.is_inter()).map(|m| m.mv[0]);
        let (avg_row, avg_col) = match (a, l) {
            (Some(a), Some(l)) => (
                (i32::from(a.row) + i32::from(l.row) + 1) >> 1,
                (i32::from(a.col) + i32::from(l.col) + 1) >> 1,
            ),
            (Some(a), None) => (i32::from(a.row), i32::from(a.col)),
            (None, Some(l)) => (i32::from(l.row), i32::from(l.col)),
            (None, None) => (0, 0),
        };
        let (row_diff, col_diff) = (avg_row - mv_row, avg_col - mv_col);
        if !(-48..=48).contains(&row_diff) || !(-48..=48).contains(&col_diff) {
            rdc.rdcost = if b.bsize > BLOCK_32X32 {
                rdc.rdcost << 1
            } else {
                (3 * rdc.rdcost) >> 1
            };
        }
    }
    let small = |lim: i32| mv_row < lim && mv_row > -lim && mv_col < lim && mv_col > -lim;
    let noisy_still = s.noise_enabled
        && s.noise_level >= NoiseLevel::Medium
        && b.bsize >= BLOCK_32X32
        && is_last_frame
        && small(8);
    let lighting_change = b.sb.lowvar_highsumdiff
        && !b.sb.sb_is_skin
        && b.bsize >= BLOCK_16X16
        && is_last_frame
        && small(16);
    if noisy_still || lighting_change {
        rdc.rdcost = 7 * (rdc.rdcost >> 3);
    }
}

/// libvpx's `encode_breakout_test` with its realtime `encode_breakout` of
/// 0: a prediction with no luma residual at all, and none in chroma either,
/// skips the block (`x->skip`); its rate is then the mode's alone and its
/// distortion the luma's (nothing).
#[allow(clippy::too_many_arguments)]
fn encode_breakout_test(
    f: &mut FrameEncoder<'_>,
    b: &mut Block<'_, '_>,
    mi: &ModeInfo,
    var: u32,
    sse: u32,
    rdc: &mut RdCost,
    x_skip: &mut bool,
    inter_mode_cost: i32,
) {
    // The thresholds are 0: libvpx's x->encode_breakout is.
    let (thresh_ac, thresh_dc) = (0u32, 0u32);
    if !(var <= thresh_ac && sse.wrapping_sub(var) <= thresh_dc) {
        return;
    }
    let (thresh_ac_uv, thresh_dc_uv) = (0u32, 0u32);
    if !b.preduv[0] || !b.preduv[1] {
        f.predict_inter(b.mi_row, b.mi_col, b.bsize, mi, 1..3);
    }
    let (w, h) = (b.bw >> 1, b.bh >> 1);
    let (src, ss, dst, ds) = f.planes_at(1, b.x >> 1, b.y >> 1);
    let (var_u, sse_u) = variance::variance(src, ss, dst, ds, w, h);
    if (var_u << 2) <= thresh_ac_uv && sse_u.wrapping_sub(var_u) <= thresh_dc_uv {
        let (src, ss, dst, ds) = f.planes_at(2, b.x >> 1, b.y >> 1);
        let (var_v, sse_v) = variance::variance(src, ss, dst, ds, w, h);
        if (var_v << 2) <= thresh_ac_uv && sse_v.wrapping_sub(var_v) <= thresh_dc_uv {
            *x_skip = true;
            rdc.rate = inter_mode_cost;
            rdc.dist = i64::from(sse) << 4;
        }
    }
}

/// libvpx's `compute_intra_yprediction`: the block's luma predicted with
/// `mode` at its largest transform size, from the source's pixels where the
/// search skips the encode (`x->skip_encode`), else the reconstruction's.
fn compute_intra_yprediction(
    f: &mut FrameEncoder<'_>,
    b: &Block<'_, '_>,
    place: &crate::enc::encodeframe::Placement,
    mode: PredictionMode,
) {
    let tx_size = tables::MAX_TXSIZE[usize::from(b.bsize)];
    let n4w = i32::from(tables::NUM_4X4_WIDE[usize::from(b.bsize)]);
    let n4h = i32::from(tables::NUM_4X4_HIGH[usize::from(b.bsize)]);
    let max_wide = n4w
        + if place.mb_to_right_edge >= 0 {
            0
        } else {
            place.mb_to_right_edge >> 5
        };
    let max_high = n4h
        + if place.mb_to_bottom_edge >= 0 {
            0
        } else {
            place.mb_to_bottom_edge >> 5
        };
    let step = 1i32 << tx_size;
    let mut row = 0;
    while row < max_high {
        let mut col = 0;
        while col < max_wide {
            f.predict_intra_search(
                place,
                b.bsize,
                row as usize,
                col as usize,
                tx_size,
                mode,
                b.skip_encode,
            );
            col += step;
        }
        row += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_low_variance_flags_are_read_by_position() {
        let mut low = [false; 25];
        low[5] = true;
        assert!(force_skip_low_temp_var(&low, 0, 0, BLOCK_32X32));
        assert!(!force_skip_low_temp_var(&low, 0, 4, BLOCK_32X32));
        low[POS_SHIFT_16X16[1][2]] = true;
        assert!(force_skip_low_temp_var(&low, 2, 4, BLOCK_16X16));
        // A 32x16 needs both of its 16x16 halves low.
        assert!(!force_skip_low_temp_var(
            &low,
            2,
            4,
            crate::common::BLOCK_32X16
        ));
        low[POS_SHIFT_16X16[1][3]] = true;
        assert!(force_skip_low_temp_var(
            &low,
            2,
            4,
            crate::common::BLOCK_32X16
        ));
    }

    #[test]
    fn mode_offsets_are_libvpxs() {
        assert_eq!(mode_offset(NEARESTMV), 0);
        assert_eq!(mode_offset(NEWMV), 3);
        assert_eq!(mode_offset(DC_PRED), 0);
        assert_eq!(mode_offset(TM_PRED), 3);
        assert_eq!(MODE_IDX[2][mode_offset(ZEROMV)], THR_ZEROG);
    }

    #[test]
    fn block_variances_combine_by_quadrant() {
        // Four 8x8 blocks of sums 8, 8, 8, 8 and squares 64 each.
        let sse = [64u32; 4];
        let sum = [8i32; 4];
        let mut var_o = [0u32; 1];
        let mut sse_o = [0u32; 1];
        let mut sum_o = [0i32; 1];
        // A 16x16 block: bwl = bhl = 2 in 4-pixel units; unit 8x8.
        calculate_variance(
            2, 2, BLOCK_8X8, &sse, &sum, &mut var_o, &mut sse_o, &mut sum_o,
        );
        assert_eq!((sse_o[0], sum_o[0]), (256, 32));
        // 32^2 >> (1 + 1 + 6) = 4.
        assert_eq!(var_o[0], 252);
    }

    /// libvpx's speed 8 changes two of its searches at 352x288 and below:
    /// every block searches the filter, and a losing mode's threshold may
    /// climb twice as far. The boundary is the area, not either side.
    #[test]
    fn speed_8_searches_harder_at_and_below_352x288() {
        for (w, h, small) in [
            (1280, 720, false),
            (640, 360, false),
            (353, 288, false),
            (352, 288, true),
            (288, 352, true),
            (320, 240, true),
            (1, 1, true),
            // Wider than 352 and still no more pixels.
            (704, 144, true),
        ] {
            assert_eq!(speed8_cb_pred_filter_search(w, h), !small, "{w}x{h}");
            assert_eq!(
                speed8_adaptive_rd_thresh(w, h),
                if small { 2 } else { 1 },
                "{w}x{h}"
            );
        }
    }
}
