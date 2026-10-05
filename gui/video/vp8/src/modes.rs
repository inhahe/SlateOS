//! Every macroblock's modes and motion vectors, which a frame codes in its
//! first partition, all of them before any coefficient.
//!
//! A key frame's macroblocks are all intra: a luma mode (one for the
//! macroblock, or one per 4x4 block, coded by its neighbours' modes) and a
//! chroma mode. An inter frame's macroblocks are intra or predict from one of
//! three reference frames, with a motion vector chosen from its neighbours'
//! (nearest, near, zero), coded afresh (new), or one per part of the
//! macroblock split four ways (split).
//!
//! The grid of mode information lasts from frame to frame: a frame that does
//! not update the segment map keeps each macroblock's segment from the frame
//! before. Its first row and column are a border that nothing writes, whose
//! cells read as intra, DC-predicted and still.
//!
//! Translated into Rust from libvpx v1.17.0's `vp8/decoder/decodemv.c` and
//! `vp8/common/findnearmv.h` (copyright the WebM project authors), used under
//! libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "grid cells are indexed by macroblock positions inside the grid and their border; block numbers come from libvpx's split tables (below 16); counts index the mode-context table and are at most 5 by construction"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "edge distances are at most 16384 pixels in eighths; motion vectors wrap at 16 bits as libvpx's shorts do, through explicit wrapping operations"
)]

use crate::boolread::BoolDecoder;
use crate::tables::{
    BMODE_PROB, BMODE_TREE, KF_BMODE_PROB, KF_UV_MODE_PROB, KF_YMODE_PROB, KF_YMODE_TREE,
    MBSPLIT_FILL_COUNT, MBSPLIT_FILL_OFFSET, MBSPLIT_OFFSET, MODE_CONTEXTS, MV_UPDATE_PROBS,
    SMALL_MV_TREE, SUB_MV_REF_PROB3, UV_MODE_TREE, YMODE_TREE,
};

// libvpx's `MB_PREDICTION_MODE`.
pub(crate) const DC_PRED: u8 = 0;
pub(crate) const V_PRED: u8 = 1;
pub(crate) const H_PRED: u8 = 2;
pub(crate) const TM_PRED: u8 = 3;
pub(crate) const B_PRED: u8 = 4;
pub(crate) const NEARESTMV: u8 = 5;
pub(crate) const NEARMV: u8 = 6;
pub(crate) const ZEROMV: u8 = 7;
pub(crate) const NEWMV: u8 = 8;
pub(crate) const SPLITMV: u8 = 9;

// libvpx's `B_PREDICTION_MODE`, the 4x4 intra modes.
pub(crate) const B_DC_PRED: u8 = 0;
pub(crate) const B_TM_PRED: u8 = 1;
pub(crate) const B_VE_PRED: u8 = 2;
pub(crate) const B_HE_PRED: u8 = 3;
pub(crate) const B_LD_PRED: u8 = 4;
pub(crate) const B_RD_PRED: u8 = 5;
pub(crate) const B_VR_PRED: u8 = 6;
pub(crate) const B_VL_PRED: u8 = 7;
pub(crate) const B_HD_PRED: u8 = 8;
pub(crate) const B_HU_PRED: u8 = 9;

// libvpx's `MV_REFERENCE_FRAME`.
pub(crate) const INTRA_FRAME: u8 = 0;
pub(crate) const LAST_FRAME: u8 = 1;
pub(crate) const GOLDEN_FRAME: u8 = 2;
pub(crate) const ALTREF_FRAME: u8 = 3;

/// How far a motion vector may point outside the picture before the
/// prediction stage must clamp it, in eighths of a pixel: libvpx's
/// `LEFT_TOP_MARGIN` and `RIGHT_BOTTOM_MARGIN`.
const MARGIN: i32 = 16 << 3;

/// Offsets into a motion vector component's probabilities: libvpx's
/// `mvpis_short`, `MVPsign`, `MVPshort` and `MVPbits`.
const MVP_IS_SHORT: usize = 0;
const MVP_SIGN: usize = 1;
const MVP_SHORT: usize = 2;
const MVP_BITS: usize = MVP_SHORT + 8 - 1;
/// Bits in a long motion vector component: libvpx's `mvlong_width`.
const MV_LONG_WIDTH: usize = 10;
/// Probabilities per motion vector component: libvpx's `MVPcount`.
pub(crate) const MVP_COUNT: usize = MVP_BITS + MV_LONG_WIDTH;

/// A motion vector, in eighths of a pixel (luma vectors are always even:
/// VP8 codes quarter pixels). libvpx keeps both parts in 16 bits and lets
/// them wrap, and so does this.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Mv {
    pub(crate) row: i16,
    pub(crate) col: i16,
}

impl Mv {
    pub(crate) const ZERO: Self = Self { row: 0, col: 0 };

    fn is_zero(self) -> bool {
        self == Self::ZERO
    }
}

/// One macroblock's modes: libvpx's `MODE_INFO`, with the union of its 4x4
/// blocks' modes and vectors kept as two arrays (no stream can make libvpx
/// read one as the other).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ModeInfo {
    /// `DC_PRED` to `SPLITMV`.
    pub(crate) mode: u8,
    /// The chroma mode of an intra macroblock.
    pub(crate) uv_mode: u8,
    /// `INTRA_FRAME` to `ALTREF_FRAME`.
    pub(crate) ref_frame: u8,
    /// The motion vector; for a split macroblock, its last block's.
    pub(crate) mv: Mv,
    /// How a split macroblock is split: 0 into 16x8 halves, 1 into 8x16, 2
    /// into 8x8 quarters, 3 into sixteen 4x4 blocks.
    pub(crate) partitioning: u8,
    /// No coefficients: coded, or found to be so once they were decoded.
    pub(crate) mb_skip_coeff: bool,
    /// Whether a vector may point far enough outside the picture that the
    /// prediction stage must clamp it.
    pub(crate) need_to_clamp_mvs: bool,
    /// Split or 4x4-predicted: no second-order luma DC block.
    pub(crate) is_4x4: bool,
    /// 0 to 3.
    pub(crate) segment_id: u8,
    /// The 4x4 blocks' intra modes, `B_DC_PRED` to `B_HU_PRED`.
    pub(crate) bmodes: [u8; 16],
    /// The 4x4 blocks' motion vectors.
    pub(crate) bmvs: [Mv; 16],
}

/// Every macroblock's [`ModeInfo`], with a border row above and a border
/// column on the left: libvpx's `mip`, of which `mi` is the first cell inside.
#[derive(Clone, Debug)]
pub(crate) struct ModeGrid {
    pub(crate) cells: Vec<ModeInfo>,
    pub(crate) stride: usize,
    pub(crate) mb_rows: usize,
    pub(crate) mb_cols: usize,
}

impl ModeGrid {
    pub(crate) fn new(mb_cols: usize, mb_rows: usize) -> Self {
        Self {
            cells: vec![ModeInfo::default(); (mb_cols + 1) * (mb_rows + 1)],
            stride: mb_cols + 1,
            mb_rows,
            mb_cols,
        }
    }

    /// The cell of the macroblock in row `mb_row`, column `mb_col`.
    #[inline]
    pub(crate) fn index(&self, mb_row: usize, mb_col: usize) -> usize {
        (mb_row + 1) * self.stride + mb_col + 1
    }

    #[inline]
    pub(crate) fn get(&self, mb_row: usize, mb_col: usize) -> &ModeInfo {
        &self.cells[self.index(mb_row, mb_col)]
    }
}

/// What the frame header says about how modes are coded, read before them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ModeHeader<'a> {
    pub(crate) key_frame: bool,
    /// Whether each macroblock codes its segment.
    pub(crate) update_segment_map: bool,
    pub(crate) segment_tree_probs: [u8; 3],
    /// libvpx's `mb_no_coeff_skip`: whether each macroblock codes whether it
    /// has coefficients.
    pub(crate) skip_coded: bool,
    pub(crate) prob_skip_false: u8,
    pub(crate) prob_intra: u8,
    pub(crate) prob_last: u8,
    pub(crate) prob_gf: u8,
    pub(crate) ymode_prob: &'a [u8; 4],
    pub(crate) uv_mode_prob: &'a [u8; 3],
    pub(crate) mvc: &'a [[u8; MVP_COUNT]; 2],
    /// Whether each reference frame's vectors point backwards in time
    /// (`INTRA_FRAME` and `LAST_FRAME` never do).
    pub(crate) sign_bias: [bool; 4],
}

/// The probabilities the frame header may update before the modes: libvpx's
/// `mb_mode_mv_init`, which reads them. Returns `(skip_coded,
/// prob_skip_false, prob_intra, prob_last, prob_gf)`.
pub(crate) fn read_mode_probs(
    bc: &mut BoolDecoder<'_>,
    key_frame: bool,
    ymode_prob: &mut [u8; 4],
    uv_mode_prob: &mut [u8; 3],
    mvc: &mut [[u8; MVP_COUNT]; 2],
) -> (bool, u8, u8, u8, u8) {
    let skip_coded = bc.read_bit();
    let prob_skip_false = if skip_coded { bc.read_u8(8) } else { 0 };
    if key_frame {
        return (skip_coded, prob_skip_false, 0, 0, 0);
    }
    let prob_intra = bc.read_u8(8);
    let prob_last = bc.read_u8(8);
    let prob_gf = bc.read_u8(8);
    if bc.read_bit() {
        for p in ymode_prob.iter_mut() {
            *p = bc.read_u8(8);
        }
    }
    if bc.read_bit() {
        for p in uv_mode_prob.iter_mut() {
            *p = bc.read_u8(8);
        }
    }
    // libvpx's `read_mvcontexts`.
    for (probs, update) in mvc.iter_mut().zip(&MV_UPDATE_PROBS) {
        for (p, &up) in probs.iter_mut().zip(update) {
            if bc.read(up) {
                let x = bc.read_u8(7);
                *p = if x == 0 { 1 } else { x << 1 };
            }
        }
    }
    (skip_coded, prob_skip_false, prob_intra, prob_last, prob_gf)
}

/// A macroblock's distances to the picture's edges, in eighths of a pixel:
/// libvpx's `mb_to_left_edge` and the rest. Left and top are 0 or negative.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Edges {
    pub(crate) left: i32,
    pub(crate) right: i32,
    pub(crate) top: i32,
    pub(crate) bottom: i32,
}

impl Edges {
    pub(crate) fn of(mb_row: usize, mb_col: usize, mb_rows: usize, mb_cols: usize) -> Self {
        // Positions are below 1024 macroblocks, so these fit an i32.
        let at = |n: usize| i32::try_from(n * 16 * 8).unwrap_or(i32::MAX);
        Self {
            left: -at(mb_col),
            right: at(mb_cols - 1 - mb_col),
            top: -at(mb_row),
            bottom: at(mb_rows - 1 - mb_row),
        }
    }
}

/// Clamp a vector to point at most 16 pixels outside the picture: libvpx's
/// `vp8_clamp_mv2`. Each bound is in reach of a 16-bit vector whenever the
/// test against it can succeed, so the assignments never wrap.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a bound is assigned only when a 16-bit vector lies beyond it, so it fits 16 bits"
)]
fn clamp_mv2(mv: &mut Mv, e: Edges) {
    let col = i32::from(mv.col);
    if col < e.left - MARGIN {
        mv.col = (e.left - MARGIN) as i16;
    } else if col > e.right + MARGIN {
        mv.col = (e.right + MARGIN) as i16;
    }
    let row = i32::from(mv.row);
    if row < e.top - MARGIN {
        mv.row = (e.top - MARGIN) as i16;
    } else if row > e.bottom + MARGIN {
        mv.row = (e.bottom + MARGIN) as i16;
    }
}

/// Whether a vector points more than 16 pixels outside the picture:
/// libvpx's `vp8_check_mv_bounds`, with the margins already applied.
fn outside(mv: Mv, e: Edges) -> bool {
    let (col, row) = (i32::from(mv.col), i32::from(mv.row));
    col < e.left - MARGIN
        || col > e.right + MARGIN
        || row < e.top - MARGIN
        || row > e.bottom + MARGIN
}

/// Flip a neighbour's vector if its reference frame points the other way in
/// time from this macroblock's: libvpx's `mv_bias`.
fn mv_bias(neighbour_ref: u8, this_ref: u8, mv: &mut Mv, sign_bias: [bool; 4]) {
    if sign_bias[usize::from(neighbour_ref & 3)] != sign_bias[usize::from(this_ref & 3)] {
        mv.row = mv.row.wrapping_neg();
        mv.col = mv.col.wrapping_neg();
    }
}

/// One component of a coded vector, in quarter pixels: libvpx's
/// `read_mvcomponent`.
fn read_mv_component(bc: &mut BoolDecoder<'_>, p: &[u8; MVP_COUNT]) -> i32 {
    let mut x: i32;
    if bc.read(p[MVP_IS_SHORT]) {
        x = 0;
        for i in 0..3 {
            x += i32::from(bc.read(p[MVP_BITS + i])) << i;
        }
        // Bit 3 last: it is implied when the higher bits are all clear.
        for i in (4..MV_LONG_WIDTH).rev() {
            x += i32::from(bc.read(p[MVP_BITS + i])) << i;
        }
        if x & 0xfff0 == 0 || bc.read(p[MVP_BITS + 3]) {
            x += 8;
        }
    } else {
        x = i32::from(bc.read_tree(&SMALL_MV_TREE, &p[MVP_SHORT..MVP_SHORT + 7]));
    }
    if x != 0 && bc.read(p[MVP_SIGN]) {
        x = -x;
    }
    x
}

/// A coded vector added to `best`: libvpx's `read_mv` and the additions
/// after it, wrapping at 16 bits.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a component is at most 1023 quarter pixels, so twice it fits 16 bits"
)]
fn read_mv(bc: &mut BoolDecoder<'_>, mvc: &[[u8; MVP_COUNT]; 2], best: Mv) -> Mv {
    // A component is at most 1023 quarter pixels, so doubling it fits.
    let row = (read_mv_component(bc, &mvc[0]) * 2) as i16;
    let col = (read_mv_component(bc, &mvc[1]) * 2) as i16;
    Mv {
        row: row.wrapping_add(best.row),
        col: col.wrapping_add(best.col),
    }
}

/// The 4x4 mode a neighbour offers block `b` above or left of it: libvpx's
/// `above_block_mode` and `left_block_mode`, given the neighbour's cell (or
/// the current one) and the block of it that touches `b`.
fn neighbour_bmode(mi: &ModeInfo, block: usize) -> u8 {
    match mi.mode {
        B_PRED => mi.bmodes[block],
        V_PRED => B_VE_PRED,
        H_PRED => B_HE_PRED,
        TM_PRED => B_TM_PRED,
        _ => B_DC_PRED,
    }
}

/// A key frame's macroblock: libvpx's `read_kf_modes`.
fn read_kf_modes(bc: &mut BoolDecoder<'_>, grid: &ModeGrid, idx: usize, mi: &mut ModeInfo) {
    mi.ref_frame = INTRA_FRAME;
    mi.mode = bc.read_tree(&KF_YMODE_TREE, &KF_YMODE_PROB);
    if mi.mode == B_PRED {
        mi.is_4x4 = true;
        let above = &grid.cells[idx - grid.stride];
        let left = &grid.cells[idx - 1];
        for i in 0..16 {
            let a = if i < 4 {
                neighbour_bmode(above, i + 12)
            } else {
                mi.bmodes[i - 4]
            };
            let l = if i & 3 == 0 {
                neighbour_bmode(left, i + 3)
            } else {
                mi.bmodes[i - 1]
            };
            mi.bmodes[i] =
                bc.read_tree(&BMODE_TREE, &KF_BMODE_PROB[usize::from(a)][usize::from(l)]);
        }
    }
    mi.uv_mode = bc.read_tree(&UV_MODE_TREE, &KF_UV_MODE_PROB);
}

/// A split macroblock's vectors: libvpx's `decode_split_mv`.
fn decode_split_mv(
    bc: &mut BoolDecoder<'_>,
    mi: &mut ModeInfo,
    left_mb: &ModeInfo,
    above_mb: &ModeInfo,
    best: Mv,
    mvc: &[[u8; MVP_COUNT]; 2],
    edges: Edges,
) {
    let (split, num_p): (u8, usize) = if bc.read(110) {
        if bc.read(111) {
            (u8::from(bc.read(150)), 2)
        } else {
            (2, 4)
        }
    } else {
        (3, 16)
    };
    let s = usize::from(split);
    let fill_count = usize::from(MBSPLIT_FILL_COUNT[s]);
    for j in 0..num_p {
        let k = usize::from(MBSPLIT_OFFSET[s][j]);
        let left = if k & 3 == 0 {
            if left_mb.mode == SPLITMV {
                left_mb.bmvs[k + 3]
            } else {
                left_mb.mv
            }
        } else {
            mi.bmvs[k - 1]
        };
        let above = if k >> 2 == 0 {
            if above_mb.mode == SPLITMV {
                above_mb.bmvs[k + 12]
            } else {
                above_mb.mv
            }
        } else {
            mi.bmvs[k - 4]
        };
        let prob = &SUB_MV_REF_PROB3[usize::from(above.is_zero()) << 2
            | usize::from(left.is_zero()) << 1
            | usize::from(left == above)];
        let block_mv = if !bc.read(prob[0]) {
            left
        } else if !bc.read(prob[1]) {
            above
        } else if !bc.read(prob[2]) {
            Mv::ZERO
        } else {
            read_mv(bc, mvc, best)
        };
        mi.need_to_clamp_mvs |= outside(block_mv, edges);
        // Fill this part's blocks now: later parts take their left and above
        // vectors from them.
        for &b in &MBSPLIT_FILL_OFFSET[s][j * fill_count..(j + 1) * fill_count] {
            mi.bmvs[usize::from(b)] = block_mv;
        }
    }
    mi.partitioning = split;
}

/// Where in `near_mvs` and `cnt` each candidate goes: libvpx's `CNT_INTRA`
/// and the rest.
const CNT_INTRA: usize = 0;
const CNT_NEAREST: usize = 1;
const CNT_NEAR: usize = 2;
const CNT_SPLITMV: usize = 3;

/// An inter frame's macroblock: libvpx's `read_mb_modes_mv`.
fn read_mb_modes_mv(
    bc: &mut BoolDecoder<'_>,
    h: &ModeHeader<'_>,
    grid: &ModeGrid,
    idx: usize,
    edges: Edges,
    mi: &mut ModeInfo,
) {
    mi.ref_frame = u8::from(bc.read(h.prob_intra));
    if mi.ref_frame == INTRA_FRAME {
        // The vector the next macroblocks see as this one's.
        mi.mv = Mv::ZERO;
        mi.mode = bc.read_tree(&YMODE_TREE, h.ymode_prob);
        if mi.mode == B_PRED {
            mi.is_4x4 = true;
            for b in &mut mi.bmodes {
                *b = bc.read_tree(&BMODE_TREE, &BMODE_PROB);
            }
        }
        mi.uv_mode = bc.read_tree(&UV_MODE_TREE, h.uv_mode_prob);
        return;
    }

    let above = &grid.cells[idx - grid.stride];
    let left = &grid.cells[idx - 1];
    let above_left = &grid.cells[idx - grid.stride - 1];
    mi.need_to_clamp_mvs = false;
    if bc.read(h.prob_last) {
        mi.ref_frame = 2 + u8::from(bc.read(h.prob_gf));
    }

    // The distinct vectors of the three neighbours, each weighted by how
    // close it is (2 above and left, 1 above-left), zero vectors counted
    // apart.
    let mut near_mvs = [Mv::ZERO; 4];
    let mut cnt = [0usize; 4];
    let mut n = CNT_INTRA;
    if above.ref_frame != INTRA_FRAME {
        if !above.mv.is_zero() {
            n += 1;
            near_mvs[n] = above.mv;
            mv_bias(above.ref_frame, mi.ref_frame, &mut near_mvs[n], h.sign_bias);
        }
        cnt[n] += 2;
    }
    for (neighbour, weight) in [(left, 2), (above_left, 1)] {
        if neighbour.ref_frame != INTRA_FRAME {
            if neighbour.mv.is_zero() {
                cnt[CNT_INTRA] += weight;
            } else {
                let mut this_mv = neighbour.mv;
                mv_bias(neighbour.ref_frame, mi.ref_frame, &mut this_mv, h.sign_bias);
                if this_mv != near_mvs[n] {
                    n += 1;
                    near_mvs[n] = this_mv;
                }
                cnt[n] += weight;
            }
        }
    }

    // `cnt` entries are sums of at most 2 + 2 + 1 (and the merge below adds
    // 1 only to a count of at most 2), within the table's six rows.
    let context = |count: usize, node: usize| MODE_CONTEXTS[count.min(5)][node];
    if !bc.read(context(cnt[CNT_INTRA], 0)) {
        mi.mode = ZEROMV;
        mi.mv = Mv::ZERO;
        return;
    }
    // If there are three distinct vectors, the above-left one can merge with
    // the nearest.
    cnt[CNT_NEAREST] += usize::from(cnt[CNT_SPLITMV] > 0 && near_mvs[n] == near_mvs[CNT_NEAREST]);
    if cnt[CNT_NEAR] > cnt[CNT_NEAREST] {
        cnt.swap(CNT_NEAREST, CNT_NEAR);
        near_mvs.swap(CNT_NEAREST, CNT_NEAR);
    }
    if !bc.read(context(cnt[CNT_NEAREST], 1)) {
        mi.mode = NEARESTMV;
        mi.mv = near_mvs[CNT_NEAREST];
        clamp_mv2(&mut mi.mv, edges);
        return;
    }
    if !bc.read(context(cnt[CNT_NEAR], 2)) {
        mi.mode = NEARMV;
        mi.mv = near_mvs[CNT_NEAR];
        clamp_mv2(&mut mi.mv, edges);
        return;
    }
    // The best vector, which new vectors are coded against.
    let near_index = CNT_INTRA + usize::from(cnt[CNT_NEAREST] >= cnt[CNT_INTRA]);
    clamp_mv2(&mut near_mvs[near_index], edges);
    let best = near_mvs[near_index];
    cnt[CNT_SPLITMV] = (usize::from(above.mode == SPLITMV) + usize::from(left.mode == SPLITMV)) * 2
        + usize::from(above_left.mode == SPLITMV);
    if bc.read(context(cnt[CNT_SPLITMV], 3)) {
        decode_split_mv(bc, mi, left, above, best, h.mvc, edges);
        mi.mv = mi.bmvs[15];
        mi.mode = SPLITMV;
        mi.is_4x4 = true;
    } else {
        mi.mv = read_mv(bc, h.mvc, best);
        // Nearest and near vectors were clamped above; a new one is not, so
        // say whether prediction must.
        mi.need_to_clamp_mvs = outside(mi.mv, edges);
        mi.mode = NEWMV;
    }
}

/// Every macroblock's modes and vectors: libvpx's `vp8_decode_mode_mvs`
/// after `mb_mode_mv_init`.
pub(crate) fn decode_mode_mvs(bc: &mut BoolDecoder<'_>, h: &ModeHeader<'_>, grid: &mut ModeGrid) {
    let (rows, cols) = (grid.mb_rows, grid.mb_cols);
    for mb_row in 0..rows {
        for mb_col in 0..cols {
            let idx = grid.index(mb_row, mb_col);
            let mut mi = grid.cells[idx];
            if h.update_segment_map {
                // libvpx's `read_mb_features`.
                let p = h.segment_tree_probs;
                mi.segment_id = if bc.read(p[0]) {
                    2 + u8::from(bc.read(p[2]))
                } else {
                    u8::from(bc.read(p[1]))
                };
            } else if h.key_frame {
                mi.segment_id = 0;
            }
            mi.mb_skip_coeff = h.skip_coded && bc.read(h.prob_skip_false);
            mi.is_4x4 = false;
            if h.key_frame {
                read_kf_modes(bc, grid, idx, &mut mi);
            } else {
                let edges = Edges::of(mb_row, mb_col, rows, cols);
                read_mb_modes_mv(bc, h, grid, idx, edges, &mut mi);
            }
            grid.cells[idx] = mi;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_has_a_border_above_and_to_the_left() {
        let g = ModeGrid::new(3, 2);
        assert_eq!(g.stride, 4);
        assert_eq!(g.cells.len(), 12);
        assert_eq!(g.index(0, 0), 5);
        // Left of row 1's first macroblock is the cell after row 0's last.
        assert_eq!(g.index(1, 0) - 1, g.index(0, 2) + 1);
        // Above row 0 is the border row.
        assert_eq!(g.index(0, 2) - g.stride, 3);
    }

    #[test]
    fn edges_are_eighths_of_a_pixel_to_each_side() {
        let e = Edges::of(1, 2, 3, 4);
        assert_eq!((e.left, e.right, e.top, e.bottom), (-256, 128, -128, 128));
    }

    #[test]
    fn a_vector_is_clamped_to_sixteen_pixels_outside() {
        let e = Edges::of(0, 0, 2, 2);
        let mut mv = Mv {
            row: -1000,
            col: 1000,
        };
        clamp_mv2(&mut mv, e);
        assert_eq!(
            mv,
            Mv {
                row: -128,
                col: 256
            }
        );
        assert!(!outside(mv, e));
        assert!(outside(Mv { row: -130, col: 0 }, e));
    }

    #[test]
    fn a_flipped_vector_wraps_as_a_short_does() {
        let mut mv = Mv {
            row: i16::MIN,
            col: 5,
        };
        mv_bias(
            GOLDEN_FRAME,
            LAST_FRAME,
            &mut mv,
            [false, false, true, false],
        );
        assert_eq!(
            mv,
            Mv {
                row: i16::MIN,
                col: -5
            }
        );
        let mut same = Mv { row: 3, col: 5 };
        mv_bias(
            GOLDEN_FRAME,
            ALTREF_FRAME,
            &mut same,
            [false, false, true, true],
        );
        assert_eq!(same, Mv { row: 3, col: 5 });
    }

    #[test]
    fn a_neighbours_whole_block_mode_stands_for_its_4x4_modes() {
        let mut mi = ModeInfo {
            mode: V_PRED,
            ..ModeInfo::default()
        };
        assert_eq!(neighbour_bmode(&mi, 12), B_VE_PRED);
        mi.mode = B_PRED;
        mi.bmodes[12] = B_HU_PRED;
        assert_eq!(neighbour_bmode(&mi, 12), B_HU_PRED);
        // The border: zeroed, so DC.
        assert_eq!(neighbour_bmode(&ModeInfo::default(), 3), B_DC_PRED);
    }
}
