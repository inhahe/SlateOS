//! Motion search: where in a reference frame a block finds its best match.
//!
//! libvpx's realtime speeds search in three steps, each of which this
//! reproduces exactly, because the vector found is a decision the stream
//! records:
//!
//! - a **full-pixel search** from a predicted start: at speed 8 the "fast
//!   diamond", which tries the four neighbours of the best point so far and
//!   walks downhill, scoring each point by its sum of absolute differences
//!   plus what its vector would cost to code;
//! - a **sub-pixel search** around the result: half, then quarter pixels, a
//!   cross of four neighbours and the diagonal between the two better arms,
//!   scored by the variance against the bilinearly interpolated reference
//!   plus the vector's cost;
//! - for the golden reference, an **integral-projection estimate** instead
//!   of the full-pixel search: the block's row and column sums matched
//!   against the reference's, one dimension at a time.
//!
//! The costs come from tables libvpx builds from the frame's vector
//! probabilities, on its own schedule ([`MvCosts`]): what a vector costs in
//! the search is what libvpx thought it cost, stale tables included.
//!
//! A search reads the reference beyond the picture: libvpx's reference
//! buffers repeat their edge pixels 96 pixels out ([`LumaRef`]), and no
//! search reaches further.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_mcomp.c`,
//! `vp9_encodemv.c` (`build_nmv_component_cost_table`), `vp9_encoder.c`
//! (`cal_nmvsadcosts`), `vp9_encoder.h` (`mv_cost`, `mvsad_err_cost`) and
//! `vp9_rd.c` (`vp9_mv_pred`) (copyright the WebM project authors), used
//! under libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "vectors are 15-bit and limited to the search window, positions are inside a padded frame of at most 65536 pixels a side, and costs are sums of a few 13-bit costs; the narrowings are libvpx's own"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "indices are fixed tables' (precision, component, class, fraction, search step, neighbour), each bounded by its loop or its constant; the cost tables and pictures are read through get"
)]

use crate::block::use_mv_hp;
use crate::common::{
    CLASS0_SIZE, MV_CLASS_TREE, MV_CLASS0_TREE, MV_CLASSES, MV_FP_SIZE, MV_FP_TREE, MV_JOINT_TREE,
    MV_JOINTS, MV_OFFSET_BITS, Mv,
};
use crate::enc::cost::{cost_one, cost_zero};
use crate::enc::encodemv::mv_joint;
use crate::enc::rd::cost_tokens;
use crate::enc::variance;
use crate::frame::Plane;
use crate::probs::{MvComponentProbs, MvProbs};

/// libvpx's `MAX_MVSEARCH_STEPS`.
const MAX_MVSEARCH_STEPS: i32 = 11;
/// The furthest a full-pixel search may go from its reference vector:
/// libvpx's `MAX_FULL_PEL_VAL`.
pub(crate) const MAX_FULL_PEL_VAL: i32 = (1 << (MAX_MVSEARCH_STEPS - 1)) - 1;
/// The largest vector component the cost tables hold: libvpx's `MV_MAX`.
pub(crate) const MV_MAX: i32 = (1 << 14) - 1;
/// The range a coded vector must stay in: libvpx's `MV_UPP` and `MV_LOW`.
const MV_UPP: i32 = (1 << 14) - 1;
const MV_LOW: i32 = -(1 << 14);
/// The weight of a vector's cost in a mode's rate: libvpx's
/// `MV_COST_WEIGHT`.
pub(crate) const MV_COST_WEIGHT: i32 = 108;
/// How far an 8-tap filter reaches past a block: libvpx's
/// `VP9_INTERP_EXTEND`.
pub(crate) const VP9_INTERP_EXTEND: i32 = 4;
/// libvpx's `MV_VALS`: every component value from `-MV_MAX` to `MV_MAX`.
const MV_VALS: usize = (MV_MAX as usize) * 2 + 1;
/// libvpx's `QUARTER_PEL` sub-pixel stop, the realtime speeds'.
pub(crate) const QUARTER_PEL: i32 = 1;

/// The window a search may move a block's vector in, in full pixels:
/// libvpx's `MvLimits`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MvLimits {
    pub col_min: i32,
    pub col_max: i32,
    pub row_min: i32,
    pub row_max: i32,
}

impl MvLimits {
    /// The window of a block of `mi_h` x `mi_w` cells at (`mi_row`,
    /// `mi_col`): wherever its prediction still touches the picture, with
    /// the filter's reach. libvpx's `set_offsets`.
    pub(crate) fn for_block(
        mi_row: usize,
        mi_col: usize,
        mi_h: usize,
        mi_w: usize,
        mi_rows: usize,
        mi_cols: usize,
    ) -> Self {
        let (r, c) = (mi_row as i32, mi_col as i32);
        let (h, w) = (mi_h as i32, mi_w as i32);
        let (rows, cols) = (mi_rows as i32, mi_cols as i32);
        Self {
            row_min: -((r + h) * 8 + VP9_INTERP_EXTEND),
            col_min: -((c + w) * 8 + VP9_INTERP_EXTEND),
            row_max: (rows - r) * 8 + VP9_INTERP_EXTEND,
            col_max: (cols - c) * 8 + VP9_INTERP_EXTEND,
        }
    }

    /// Narrow the window to what a vector coded against `mv` can reach:
    /// libvpx's `vp9_set_mv_search_range`.
    pub(crate) fn set_search_range(&mut self, mv: Mv) {
        let (row, col) = (i32::from(mv.row), i32::from(mv.col));
        let col_min =
            ((col >> 3) - MAX_FULL_PEL_VAL + i32::from(col & 7 != 0)).max((MV_LOW >> 3) + 1);
        let row_min =
            ((row >> 3) - MAX_FULL_PEL_VAL + i32::from(row & 7 != 0)).max((MV_LOW >> 3) + 1);
        let col_max = ((col >> 3) + MAX_FULL_PEL_VAL).min((MV_UPP >> 3) - 1);
        let row_max = ((row >> 3) + MAX_FULL_PEL_VAL).min((MV_UPP >> 3) - 1);
        if self.col_min < col_min {
            self.col_min = col_min;
        }
        if self.col_max > col_max {
            self.col_max = col_max;
        }
        if self.row_min < row_min {
            self.row_min = row_min;
        }
        if self.row_max > row_max {
            self.row_max = row_max;
        }
    }

    /// The window in eighth pixels, for a vector coded against `ref_mv`:
    /// libvpx's `vp9_set_subpel_mv_search_range`.
    pub(crate) fn subpel(&self, ref_mv: Mv) -> Self {
        let (rr, rc) = (i32::from(ref_mv.row), i32::from(ref_mv.col));
        Self {
            col_min: (self.col_min * 8)
                .max(rc - MAX_FULL_PEL_VAL * 8)
                .max(MV_LOW + 1),
            col_max: (self.col_max * 8)
                .min(rc + MAX_FULL_PEL_VAL * 8)
                .min(MV_UPP - 1),
            row_min: (self.row_min * 8)
                .max(rr - MAX_FULL_PEL_VAL * 8)
                .max(MV_LOW + 1),
            row_max: (self.row_max * 8)
                .min(rr + MAX_FULL_PEL_VAL * 8)
                .min(MV_UPP - 1),
        }
    }

    /// libvpx's `is_mv_in`.
    fn contains(&self, row: i32, col: i32) -> bool {
        col >= self.col_min && col <= self.col_max && row >= self.row_min && row <= self.row_max
    }

    /// libvpx's `check_bounds`: every point `range` away is inside.
    fn check_bounds(&self, row: i32, col: i32, range: i32) -> bool {
        row - range >= self.row_min
            && row + range <= self.row_max
            && col - range >= self.col_min
            && col + range <= self.col_max
    }

    /// libvpx's `clamp_mv`, which favours the lower bound if they cross.
    pub(crate) fn clamp(&self, mv: Mv) -> Mv {
        let clamp = |v: i32, lo: i32, hi: i32| {
            if v < lo {
                lo
            } else if v > hi {
                hi
            } else {
                v
            }
        };
        Mv {
            row: clamp(i32::from(mv.row), self.row_min, self.row_max) as i16,
            col: clamp(i32::from(mv.col), self.col_min, self.col_max) as i16,
        }
    }
}

/// A luma reference with its edges repeated outward: libvpx's reference
/// buffer after `vpx_extend_frame_inner_borders`, which the searches read
/// beyond the picture.
#[derive(Clone, Debug)]
pub(crate) struct LumaRef {
    data: Vec<u8>,
    stride: usize,
    border: usize,
}

/// How far libvpx's reference buffers repeat their edges: its
/// `VP9INNERBORDERINPIXELS`. Nothing the encoder reads lies further out.
pub(crate) const INNER_BORDER: usize = 96;

impl LumaRef {
    /// The plane `p` with its picture's edge pixels repeated
    /// [`INNER_BORDER`] pixels out on every side.
    pub(crate) fn new(p: &Plane<u8>) -> Self {
        let b = INNER_BORDER;
        let (w, h) = (p.crop_width.max(1), p.crop_height.max(1));
        // libvpx extends from the crop edges out to the aligned size plus
        // the border; this covers the allocated planes' whole area too.
        let (aw, ah) = (p.stride.max(w), p.alloc_height.max(h));
        let stride = aw + 2 * b;
        let rows = ah + 2 * b;
        let mut data = vec![0u8; stride * rows];
        for y in 0..rows {
            let sy = (y as isize - b as isize).clamp(0, h as isize - 1) as usize;
            let src = p.data.get(sy * p.stride..).unwrap_or(&[]);
            let edge_l = src.first().copied().unwrap_or(0);
            let edge_r = src.get(w - 1).copied().unwrap_or(0);
            if let Some(row) = data.get_mut(y * stride..(y + 1) * stride) {
                for (x, d) in row.iter_mut().enumerate() {
                    *d = if x < b {
                        edge_l
                    } else if x - b < w {
                        src.get(x - b).copied().unwrap_or(0)
                    } else {
                        edge_r
                    };
                }
            }
        }
        Self {
            data,
            stride,
            border: b,
        }
    }

    /// The samples from luma pixel (`x`, `y`) on, rows [`Self::stride`]
    /// apart; empty if (`x`, `y`) is outside the border.
    pub(crate) fn from(&self, x: i32, y: i32) -> &[u8] {
        let b = self.border as i64;
        let (x, y) = (i64::from(x) + b, i64::from(y) + b);
        if x < 0 || y < 0 {
            return &[];
        }
        let off = y as usize * self.stride + x as usize;
        self.data.get(off..).unwrap_or(&[])
    }

    pub(crate) fn stride(&self) -> usize {
        self.stride
    }
}

/// What coding a vector costs, as the searches weigh it: libvpx's
/// `x->nmvjointcost` and `x->mvcost` (from the frame's probabilities, one
/// table per precision, each rebuilt only when libvpx rebuilds it), and the
/// fixed tables the full-pixel search weighs vectors by
/// (`x->nmvjointsadcost`, `x->nmvsadcost`).
#[derive(Clone, Debug)]
pub(crate) struct MvCosts {
    joint: [i32; MV_JOINTS],
    /// By precision (quarter, eighth) and component (row, column), the cost
    /// of each value from `-MV_MAX` to `MV_MAX`. A table libvpx has never
    /// built is all zeros, as its `calloc` left it.
    comp: [[Vec<i32>; 2]; 2],
    /// Which precision's table the costs read: libvpx's `x->mvcost`.
    hp: bool,
    sad_comp: Vec<i32>,
}

/// libvpx's `cal_nmvjointsadcost`.
const SAD_JOINT_COST: [i32; MV_JOINTS] = [600, 300, 300, 300];

/// libvpx's `cal_nmvsadcosts`: `(int)(256 * (2 * (log2f(8 * i) + .6)))`,
/// its `log2f` a single-precision logarithm (correctly rounded, as glibc's
/// is; the test pins the table).
fn sad_component_cost(i: i32) -> i32 {
    let l = (8.0f64 * f64::from(i)).log2() as f32;
    (256.0 * (2.0 * (f64::from(l) + 0.6))) as i32
}

impl MvCosts {
    /// The tables as libvpx's compressor starts: the vector costs zero, the
    /// search's fixed tables computed.
    pub(crate) fn new() -> Self {
        let mut sad_comp = vec![0i32; MV_VALS];
        for i in 1..=MV_MAX {
            let z = sad_component_cost(i);
            if let Some(c) = sad_comp.get_mut((MV_MAX + i) as usize) {
                *c = z;
            }
            if let Some(c) = sad_comp.get_mut((MV_MAX - i) as usize) {
                *c = z;
            }
        }
        let zeros = || vec![0i32; MV_VALS];
        Self {
            joint: [0; MV_JOINTS],
            comp: [[zeros(), zeros()], [zeros(), zeros()]],
            hp: false,
            sad_comp,
        }
    }

    /// Point the costs at the table for the frame's precision: libvpx's
    /// `vp9_set_high_precision_mv`.
    pub(crate) fn set_high_precision(&mut self, hp: bool) {
        self.hp = hp;
    }

    /// Build the joint table and the table for precision `usehp` from the
    /// frame's probabilities: libvpx's `vp9_build_nmv_cost_table`.
    pub(crate) fn build(&mut self, probs: &MvProbs, usehp: bool) {
        cost_tokens(&mut self.joint, &probs.joints, &MV_JOINT_TREE);
        let tables = &mut self.comp[usize::from(usehp)];
        for (table, comp) in tables.iter_mut().zip(&probs.comps) {
            build_component_costs(table, comp, usehp);
        }
    }

    /// libvpx's `mv_cost`: a difference's joint and components.
    fn cost(&self, diff: (i32, i32)) -> i32 {
        let joint = mv_joint(Mv {
            row: diff.0 as i16,
            col: diff.1 as i16,
        });
        let table = &self.comp[usize::from(self.hp)];
        let at = |t: &[i32], v: i32| t.get((v + MV_MAX) as usize).copied().unwrap_or(0);
        self.joint.get(joint).copied().unwrap_or(0) + at(&table[0], diff.0) + at(&table[1], diff.1)
    }

    /// The fixed tables' cost of a full-pixel difference.
    fn sad_cost(&self, diff: (i32, i32)) -> u32 {
        let joint = mv_joint(Mv {
            row: diff.0 as i16,
            col: diff.1 as i16,
        });
        let at = |v: i32| {
            self.sad_comp
                .get((v + MV_MAX) as usize)
                .copied()
                .unwrap_or(0)
        };
        (SAD_JOINT_COST.get(joint).copied().unwrap_or(0) + at(diff.0) + at(diff.1)) as u32
    }

    /// A vector's rate against `reference`, weighted: libvpx's
    /// `vp9_mv_bit_cost`.
    pub(crate) fn mv_bit_cost(&self, mv: Mv, reference: Mv, weight: i32) -> i32 {
        let c = self.cost(diff(mv, reference));
        (c * weight + (1 << 6)) >> 7
    }

    /// A vector's rate in the sub-pixel search's error units: libvpx's
    /// `mv_err_cost`.
    pub(crate) fn mv_err_cost(&self, mv: Mv, reference: Mv, error_per_bit: i32) -> u32 {
        let c = i64::from(self.cost(diff(mv, reference))) * i64::from(error_per_bit);
        ((c + (1 << 13)) >> 14) as u32
    }

    /// A full-pixel vector's rate in the full-pixel search's units: libvpx's
    /// `mvsad_err_cost`, in its unsigned arithmetic.
    fn mvsad_err_cost(&self, mv: (i32, i32), reference: Mv, sad_per_bit: i32) -> u32 {
        let d = (
            mv.0 - i32::from(reference.row),
            mv.1 - i32::from(reference.col),
        );
        let c = self.sad_cost(d).wrapping_mul(sad_per_bit as u32);
        c.wrapping_add(1 << 8) >> 9
    }
}

/// A vector from 32-bit components, truncated to libvpx's 16-bit fields.
fn vector(row: i32, col: i32) -> Mv {
    Mv {
        row: row as i16,
        col: col as i16,
    }
}

fn diff(mv: Mv, reference: Mv) -> (i32, i32) {
    (
        i32::from(mv.row) - i32::from(reference.row),
        i32::from(mv.col) - i32::from(reference.col),
    )
}

/// libvpx's `build_nmv_component_cost_table`: the cost of every value of a
/// component, `table` indexed by the value plus `MV_MAX`.
fn build_component_costs(table: &mut [i32], p: &MvComponentProbs, usehp: bool) {
    let mut class_cost = [0i32; MV_CLASSES];
    let mut class0_cost = [0i32; CLASS0_SIZE];
    let mut class0_fp_cost = [[0i32; MV_FP_SIZE]; CLASS0_SIZE];
    let mut fp_cost = [0i32; MV_FP_SIZE];
    let sign_cost = [cost_zero(p.sign) as i32, cost_one(p.sign) as i32];
    cost_tokens(&mut class_cost, &p.classes, &MV_CLASS_TREE);
    cost_tokens(&mut class0_cost, &p.class0, &MV_CLASS0_TREE);
    let mut bits_cost = [[0i32; 2]; MV_OFFSET_BITS];
    for (c, &b) in bits_cost.iter_mut().zip(&p.bits) {
        *c = [cost_zero(b) as i32, cost_one(b) as i32];
    }
    for (c, probs) in class0_fp_cost.iter_mut().zip(&p.class0_fp) {
        cost_tokens(c, probs, &MV_FP_TREE);
    }
    cost_tokens(&mut fp_cost, &p.fp, &MV_FP_TREE);
    let class0_hp_cost = [cost_zero(p.class0_hp) as i32, cost_one(p.class0_hp) as i32];
    let hp_cost = [cost_zero(p.hp) as i32, cost_one(p.hp) as i32];

    let mut set = |v: i32, cost: i32| {
        if let Some(c) = table.get_mut((MV_MAX + v) as usize) {
            *c = cost;
        }
    };
    set(0, 0);
    // MV_CLASS_0.
    for o in 0..(CLASS0_SIZE << 3) as i32 {
        let v = o + 1;
        let d = (o >> 3) as usize;
        let f = ((o >> 1) & 3) as usize;
        let mut cost = class_cost[0] + class0_cost[d] + class0_fp_cost[d][f];
        if usehp {
            cost += class0_hp_cost[(o & 1) as usize];
        }
        set(v, cost + sign_cost[0]);
        set(-v, cost + sign_cost[1]);
    }
    for c in 1..MV_CLASSES {
        for d in 0..(1i32 << c) {
            let b = c + 1 - 1; // c + CLASS0_BITS - 1
            let mut whole_cost = class_cost[c];
            for (i, bc) in bits_cost.iter().enumerate().take(b) {
                whole_cost += bc[((d >> i) & 1) as usize];
            }
            for (f, &fc) in fp_cost.iter().enumerate() {
                let cost = whole_cost + fc;
                let v = ((CLASS0_SIZE as i32) << (c + 2)) + d * 8 + f as i32 * 2 + 1;
                let (even, odd) = if usehp {
                    (hp_cost[0], hp_cost[1])
                } else {
                    (0, 0)
                };
                set(v, cost + even + sign_cost[0]);
                set(-v, cost + even + sign_cost[1]);
                if v + 1 > MV_MAX {
                    break;
                }
                set(v + 1, cost + odd + sign_cost[0]);
                set(-v - 1, cost + odd + sign_cost[1]);
            }
        }
    }
}

/// A block and the reference a search moves it over.
pub(crate) struct Search<'a> {
    /// The block's source, its top-left sample first, rows `src_stride`
    /// apart.
    pub src: &'a [u8],
    pub src_stride: usize,
    /// The reference and the block's own position in it, in luma pixels.
    pub pre: &'a LumaRef,
    pub x: i32,
    pub y: i32,
    pub w: usize,
    pub h: usize,
}

impl Search<'_> {
    /// The reference at full-pixel offset (`row`, `col`) from the block.
    fn at(&self, row: i32, col: i32) -> &[u8] {
        self.pre.from(self.x + col, self.y + row)
    }

    /// The sum of absolute differences at full-pixel offset (`row`, `col`).
    pub(crate) fn sad(&self, row: i32, col: i32) -> u32 {
        variance::sad(
            self.src,
            self.src_stride,
            self.at(row, col),
            self.pre.stride(),
            self.w,
            self.h,
        )
    }

    /// The variance and sum of squared differences at full-pixel offset
    /// (`row`, `col`).
    fn variance(&self, row: i32, col: i32) -> (u32, u32) {
        variance::variance(
            self.at(row, col),
            self.pre.stride(),
            self.src,
            self.src_stride,
            self.w,
            self.h,
        )
    }

    /// The variance and sum of squared differences at eighth-pixel offset
    /// (`row`, `col`), the reference interpolated bilinearly.
    fn subpel_variance(&self, row: i32, col: i32) -> (u32, u32) {
        variance::sub_pixel_variance(
            self.at(row >> 3, col >> 3),
            self.pre.stride(),
            (col & 7) as usize,
            (row & 7) as usize,
            self.src,
            self.src_stride,
            self.w,
            self.h,
        )
    }
}

/// The first scale of libvpx's big diamond: the four neighbours, (row,
/// column), clockwise from the left.
const BIGDIA_CLOSEST: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// libvpx's full-pixel search at speed 8, `FAST_DIAMOND`: its
/// `vp9_pattern_search_sad` on the big diamond's closest scale alone (no
/// initial search, no cost list, vector costs on), from `start` (full
/// pixels; clamped into the window), its vector costs measured from
/// `center` (eighth pixels). Returns the best score and vector.
pub(crate) fn fast_dia_search(
    s: &Search<'_>,
    limits: &MvLimits,
    start: Mv,
    sad_per_bit: i32,
    center: Mv,
    costs: &MvCosts,
) -> (i32, Mv) {
    let fcenter = Mv {
        row: center.row >> 3,
        col: center.col >> 3,
    };
    let start = limits.clamp(start);
    let (mut br, mut bc) = (i32::from(start.row), i32::from(start.col));
    let mut bestsad =
        s.sad(br, bc)
            .wrapping_add(costs.mvsad_err_cost((br, bc), fcenter, sad_per_bit)) as i32;
    // One candidate: libvpx's CHECK_BETTER.
    let check = |r: i32, c: i32, bestsad: &mut i32| -> bool {
        let mut thissad = s.sad(r, c) as i32;
        if thissad < *bestsad {
            thissad += costs.mvsad_err_cost((r, c), fcenter, sad_per_bit) as i32;
            if thissad < *bestsad {
                *bestsad = thissad;
                return true;
            }
        }
        false
    };
    // The scale's four points around the start.
    let mut best_site = None;
    let inside = limits.check_bounds(br, bc, 1);
    for (i, &(dr, dc)) in BIGDIA_CLOSEST.iter().enumerate() {
        let (r, c) = (br + dr, bc + dc);
        if !inside && !limits.contains(r, c) {
            continue;
        }
        if check(r, c, &mut bestsad) {
            best_site = Some(i);
        }
    }
    let Some(mut k) = best_site else {
        return (bestsad, vector(br, bc));
    };
    br += BIGDIA_CLOSEST[k].0;
    bc += BIGDIA_CLOSEST[k].1;
    // Walk on: the best point's own direction and its two neighbours.
    loop {
        let next = [(k + 3) % 4, k, (k + 1) % 4];
        let inside = limits.check_bounds(br, bc, 1);
        let mut best = None;
        for (i, &n) in next.iter().enumerate() {
            let (r, c) = (br + BIGDIA_CLOSEST[n].0, bc + BIGDIA_CLOSEST[n].1);
            if !inside && !limits.contains(r, c) {
                continue;
            }
            if check(r, c, &mut bestsad) {
                best = Some(i);
            }
        }
        let Some(i) = best else {
            break;
        };
        k = next[i];
        br += BIGDIA_CLOSEST[k].0;
        bc += BIGDIA_CLOSEST[k].1;
    }
    (bestsad, vector(br, bc))
}

/// The sub-pixel search's result: the vector (eighth pixels), its score,
/// and the variance and sum of squared differences there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Subpel {
    pub mv: Mv,
    pub besterr: u32,
    pub distortion: u32,
    pub sse: u32,
}

/// libvpx's `search_step_table`: (row, column) steps left, right, up and
/// down, at half, quarter and eighth pixels.
const SEARCH_STEPS: [[(i32, i32); 4]; 3] = [
    [(0, -4), (0, 4), (-4, 0), (4, 0)],
    [(0, -2), (0, 2), (-2, 0), (2, 0)],
    [(0, -1), (0, 1), (-1, 0), (1, 0)],
];

/// libvpx's `vp9_find_best_sub_pixel_tree` with the bilinear variance
/// (`USE_2_TAPS`) and no second-level checks (`subpel_search_level` 0):
/// from the full-pixel `bestmv`, refine to `forced_stop` (eighth pixels at
/// 0, quarter at 1), scoring by variance plus the vector's cost against
/// `ref_mv`. `limits` is the block's window, not the full-pixel search's.
#[allow(clippy::too_many_arguments)]
pub(crate) fn find_best_sub_pixel_tree(
    s: &Search<'_>,
    limits: &MvLimits,
    bestmv: Mv,
    ref_mv: Mv,
    allow_hp: bool,
    error_per_bit: i32,
    forced_stop: i32,
    costs: &MvCosts,
) -> Subpel {
    let sub = limits.subpel(ref_mv);
    let mut round = 3 - forced_stop;
    if !(allow_hp && use_mv_hp(ref_mv)) && round == 3 {
        round = 2;
    }
    let (mut br, mut bc) = (i32::from(bestmv.row) * 8, i32::from(bestmv.col) * 8);
    // setup_center_error.
    let (var, sse) = s.variance(i32::from(bestmv.row), i32::from(bestmv.col));
    let mut out = Subpel {
        mv: vector(br, bc),
        besterr: var.wrapping_add(costs.mv_err_cost(vector(br, bc), ref_mv, error_per_bit)),
        distortion: var,
        sse,
    };
    let in_range = |r: i32, c: i32| {
        c >= sub.col_min && c <= sub.col_max && r >= sub.row_min && r <= sub.row_max
    };
    let mut hstep = 4;
    for steps in SEARCH_STEPS.iter().take(round.clamp(0, 3) as usize) {
        let mut cost_array = [u32::MAX; 5];
        let mut best_idx = None;
        for (idx, &(dr, dc)) in steps.iter().enumerate() {
            let (tr, tc) = (br + dr, bc + dc);
            if in_range(tr, tc) {
                let (thismse, sse) = s.subpel_variance(tr, tc);
                let cost =
                    thismse.wrapping_add(costs.mv_err_cost(vector(tr, tc), ref_mv, error_per_bit));
                cost_array[idx] = cost;
                if cost < out.besterr {
                    best_idx = Some(idx);
                    out.besterr = cost;
                    out.distortion = thismse;
                    out.sse = sse;
                }
            }
        }
        // The diagonal between the better of left and right and the better
        // of up and down.
        let kc = if cost_array[0] <= cost_array[1] {
            -hstep
        } else {
            hstep
        };
        let kr = if cost_array[2] <= cost_array[3] {
            -hstep
        } else {
            hstep
        };
        let (tr, tc) = (br + kr, bc + kc);
        if in_range(tr, tc) {
            let (thismse, sse) = s.subpel_variance(tr, tc);
            let cost =
                thismse.wrapping_add(costs.mv_err_cost(vector(tr, tc), ref_mv, error_per_bit));
            if cost < out.besterr {
                best_idx = Some(4);
                out.besterr = cost;
                out.distortion = thismse;
                out.sse = sse;
            }
        }
        match best_idx {
            Some(4) => {
                br = tr;
                bc = tc;
            }
            Some(i) => {
                br += steps[i].0;
                bc += steps[i].1;
            }
            None => {}
        }
        hstep >>= 1;
    }
    out.mv = vector(br, bc);
    out
}

/// Sixteen column sums of a block `height` rows tall from each of
/// `columns` reference columns, starting `bw / 2` left of the block:
/// libvpx's `vp9_int_pro_motion_estimation`'s 1-D reference sets, and its
/// `vector_match`.
fn vector_match(r: &[i16], src: &[i16], bwl: u32) -> i32 {
    let bw = 4i32 << bwl;
    let var_at = |pos: i32| variance::vector_var(r.get(pos as usize..).unwrap_or(&[]), src, bwl);
    let mut best_sad = i32::MAX;
    let mut offset = 0;
    let mut d = 0;
    while d <= bw {
        let this_sad = var_at(d);
        if this_sad < best_sad {
            best_sad = this_sad;
            offset = d;
        }
        d += 16;
    }
    let mut center = offset;
    for step in [8, 4, 2, 1] {
        for d in [-step, step] {
            let this_pos = offset + d;
            if this_pos < 0 || this_pos > bw {
                continue;
            }
            let this_sad = var_at(this_pos);
            if this_sad < best_sad {
                best_sad = this_sad;
                center = this_pos;
            }
        }
        offset = center;
    }
    center - (bw >> 1)
}

/// The four points around the integral projections' match: up, left,
/// right, down (row, column).
const SEARCH_POS: [(i32, i32); 4] = [(-1, 0), (0, -1), (0, 1), (1, 0)];

/// A coarse motion estimate for a block of `bw` x `bh` (`bwl`, `bhl` their
/// log2 in 4-pixel units): libvpx's `vp9_int_pro_motion_estimation`. The
/// block's column and row sums are matched against the reference's, the
/// result refined by a step either way, and clamped to the sub-pixel window
/// of `limits` for a vector coded against `ref_mv`. Returns the sum of
/// absolute differences at the result and the vector (eighth pixels, whole
/// pixels).
pub(crate) fn int_pro_motion_estimation(
    s: &Search<'_>,
    bwl: u32,
    bhl: u32,
    limits: &MvLimits,
    ref_mv: Mv,
) -> (u32, Mv) {
    let bw = 4usize << bwl;
    let bh = 4usize << bhl;
    let search_width = bw << 1;
    let search_height = bh << 1;
    let norm_factor = 3 + (bw >> 5) as u32;
    let stride = s.pre.stride();
    let mut hbuf = [0i16; 128];
    let mut vbuf = [0i16; 128];
    let mut src_hbuf = [0i16; 64];
    let mut src_vbuf = [0i16; 64];
    // The reference's 1-D sets: columns from bw / 2 left, rows from bh / 2
    // up.
    let half_w = (bw >> 1) as i32;
    let half_h = (bh >> 1) as i32;
    let mut idx = 0;
    while idx < search_width {
        let r = s.pre.from(s.x - half_w + idx as i32, s.y);
        if let Some(h) = hbuf.get_mut(idx..) {
            variance::int_pro_row(h, r, stride, bh);
        }
        idx += 16;
    }
    for (i, v) in vbuf.iter_mut().enumerate().take(search_height) {
        let r = s.pre.from(s.x, s.y - half_h + i as i32);
        *v = variance::int_pro_col(r, bw) >> norm_factor;
    }
    // The source's.
    let mut idx = 0;
    while idx < bw {
        let r = s.src.get(idx..).unwrap_or(&[]);
        if let Some(h) = src_hbuf.get_mut(idx..) {
            variance::int_pro_row(h, r, s.src_stride, bh);
        }
        idx += 16;
    }
    for (i, v) in src_vbuf.iter_mut().enumerate().take(bh) {
        let r = s.src.get(i * s.src_stride..).unwrap_or(&[]);
        *v = variance::int_pro_col(r, bw) >> norm_factor;
    }
    let mut mv_col = vector_match(&hbuf, &src_hbuf, bwl);
    let mut mv_row = vector_match(&vbuf, &src_vbuf, bhl);
    let (this_row, this_col) = (mv_row, mv_col);
    let mut best_sad = s.sad(this_row, this_col);
    let this_sad = SEARCH_POS.map(|(dr, dc)| s.sad(this_row + dr, this_col + dc));
    for (&sad, &(dr, dc)) in this_sad.iter().zip(&SEARCH_POS) {
        if sad < best_sad {
            best_sad = sad;
            mv_row = this_row + dr;
            mv_col = this_col + dc;
        }
    }
    // The diagonal toward the better of up and down and of left and right.
    let diag_row = if this_sad[0] < this_sad[3] {
        this_row - 1
    } else {
        this_row + 1
    };
    let diag_col = if this_sad[1] < this_sad[2] {
        this_col - 1
    } else {
        this_col + 1
    };
    let tmp_sad = s.sad(diag_row, diag_col);
    if best_sad > tmp_sad {
        mv_row = diag_row;
        mv_col = diag_col;
        best_sad = tmp_sad;
    }
    let mv = limits.subpel(ref_mv).clamp(vector(mv_row * 8, mv_col * 8));
    (best_sad, mv)
}

/// The predicted vector that matches best: libvpx's `vp9_mv_pred`, over
/// the block's nearest and near vectors and (for blocks below the largest
/// partition) `pred_mv`, the superblock's own estimate if it has one. The
/// search reads the reference at each vector rounded to whole pixels.
/// Returns the index of the best (0, 1 or 2), the largest vector's whole
/// pixels, and its sum of absolute differences (`i32::MAX` if none was
/// measured).
pub(crate) fn mv_pred(
    s: &Search<'_>,
    ref_mvs: [Mv; 2],
    pred_mv: Option<Mv>,
    consider_pred_mv: bool,
) -> (usize, i32, i32) {
    let candidates = [Some(ref_mvs[0]), Some(ref_mvs[1]), pred_mv];
    let num = if consider_pred_mv { 3 } else { 2 };
    let near_same_nearest = ref_mvs[0] == ref_mvs[1];
    let mut zero_seen = false;
    let mut best_index = 0;
    let mut best_sad = i32::MAX;
    let mut max_mv = 0;
    for (i, mv) in candidates.iter().enumerate().take(num) {
        let Some(mv) = *mv else {
            continue;
        };
        if i == 1 && near_same_nearest {
            continue;
        }
        let (r, c) = (i32::from(mv.row), i32::from(mv.col));
        let fp_row = (r + 3 + i32::from(r >= 0)) >> 3;
        let fp_col = (c + 3 + i32::from(c >= 0)) >> 3;
        max_mv = max_mv.max(r.abs().max(c.abs()) >> 3);
        if fp_row == 0 && fp_col == 0 && zero_seen {
            continue;
        }
        zero_seen |= fp_row == 0 && fp_col == 0;
        let this_sad = s.sad(fp_row, fp_col) as i32;
        if this_sad < best_sad {
            best_sad = this_sad;
            best_index = i;
        }
    }
    (best_index, max_mv, best_sad)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a test: a failure should be loud"
    )]

    use super::*;
    use crate::probs::FrameContext;

    /// The fixed search tables are libvpx's, every value: the FNV-1a hash of
    /// `nmvsadcost[0][1..=MV_MAX]`'s little-endian bytes is the one
    /// `tools/mvsadcost_reference.c`, libvpx's expression in C, gives with
    /// glibc's `log2f`.
    #[test]
    fn the_sad_cost_table_is_libvpxs() {
        let costs = MvCosts::new();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for i in 1..=MV_MAX {
            let v = costs.sad_comp[(MV_MAX + i) as usize];
            assert_eq!(v, costs.sad_comp[(MV_MAX - i) as usize]);
            for b in v.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
        assert_eq!(costs.sad_comp[(MV_MAX + 1) as usize], 1843);
        assert_eq!(costs.sad_comp[(MV_MAX + 2) as usize], 2355);
        assert_eq!(costs.sad_comp[(MV_MAX + 3) as usize], 2654);
        assert_eq!(costs.sad_comp[(MV_MAX + 100) as usize], 5244);
        assert_eq!(costs.sad_comp[(MV_MAX + 16383) as usize], 9011);
        assert_eq!(h, 10_080_772_854_876_135_856);
    }

    /// A component's cost is the cost of the bits that code it: built from
    /// the default probabilities, a value's cost is what the coding path
    /// spends on it.
    #[test]
    fn component_costs_are_the_coded_bits() {
        let fc = FrameContext::defaults();
        let p = &fc.nmvc.comps[0];
        let mut table = vec![0i32; MV_VALS];
        build_component_costs(&mut table, p, true);
        // +1: class 0, integer 0, fraction 0, hp 0, sign +.
        let mut class_cost = [0i32; MV_CLASSES];
        cost_tokens(&mut class_cost, &p.classes, &MV_CLASS_TREE);
        let mut fp = [0i32; MV_FP_SIZE];
        cost_tokens(&mut fp, &p.class0_fp[0], &MV_FP_TREE);
        let want = class_cost[0]
            + cost_zero(p.class0[0]) as i32
            + fp[0]
            + cost_zero(p.class0_hp) as i32
            + cost_zero(p.sign) as i32;
        assert_eq!(table[(MV_MAX + 1) as usize], want);
        assert_eq!(table[MV_MAX as usize], 0);
        // A negative value differs only in the sign.
        assert_eq!(
            table[(MV_MAX - 1) as usize] - table[(MV_MAX + 1) as usize],
            cost_one(p.sign) as i32 - cost_zero(p.sign) as i32
        );
        // Without high precision the odd and even values cost alike.
        build_component_costs(&mut table, p, false);
        assert_eq!(table[(MV_MAX + 17) as usize], table[(MV_MAX + 18) as usize]);
        assert!(table.iter().all(|&c| c >= 0));
    }

    /// An unbuilt table costs nothing, as libvpx's zeroed table does.
    #[test]
    fn an_unbuilt_precision_costs_nothing() {
        let mut costs = MvCosts::new();
        let fc = FrameContext::defaults();
        costs.build(&fc.nmvc, false);
        costs.set_high_precision(false);
        assert!(costs.mv_bit_cost(vector(16, -8), Mv::ZERO, MV_COST_WEIGHT) > 0);
        costs.set_high_precision(true);
        let joint_only = costs.mv_bit_cost(vector(16, -8), Mv::ZERO, MV_COST_WEIGHT);
        assert_eq!(joint_only, (costs.joint[3] * MV_COST_WEIGHT + 64) >> 7);
    }

    #[test]
    fn the_search_window_is_libvpxs() {
        // A 64x64 block at the frame's top-left, 1280x720.
        let l = MvLimits::for_block(0, 0, 8, 8, 90, 160);
        assert_eq!(
            (l.row_min, l.col_min, l.row_max, l.col_max),
            (-68, -68, 724, 1284)
        );
        let mut n = l;
        n.set_search_range(vector(-20, 12));
        // (-20 >> 3) - 1023 + 1 = -1025 is below -68; 12 >> 3 + 1023 = 1024.
        assert_eq!((n.row_min, n.col_max), (-68, 1024));
        let s = l.subpel(vector(-20, 12));
        assert_eq!((s.row_min, s.col_min), (-68 * 8, -68 * 8));
        // The reference vector's reach, inside the block's window.
        assert_eq!(s.col_max, 12 + 1023 * 8);
    }

    /// A reference that is the source moved by (3, -2) pixels: the
    /// diamond walks there, and the sub-pixel search stays on the whole
    /// pixel.
    #[test]
    fn the_searches_find_a_moved_block() {
        let (w, h) = (128usize, 128usize);
        // A bowl: the sums of differences fall toward the match from every
        // side, so the diamond's walk downhill reaches it.
        let pic = |x: usize, y: usize| -> u8 {
            let (dx, dy) = (x as i32 - 64, y as i32 - 64);
            ((dx * dx + dy * dy) / 16).min(255) as u8
        };
        let mut p = Plane {
            data: vec![0u8; w * h],
            stride: w,
            alloc_height: h,
            width: w,
            height: h,
            crop_width: w,
            crop_height: h,
        };
        for y in 0..h {
            for x in 0..w {
                p.data[y * w + x] = pic(x, y);
            }
        }
        let r = LumaRef::new(&p);
        // The source block at (56, 56), around the bowl's bottom (where the
        // slopes run every way, so no direction is flat), is the
        // reference's at (56 - 2, 56 + 3).
        let (bx, by) = (56usize, 56usize);
        let mut src = vec![0u8; 16 * 16];
        for y in 0..16 {
            for x in 0..16 {
                src[y * 16 + x] = pic(bx + x - 2, by + y + 3);
            }
        }
        let s = Search {
            src: &src,
            src_stride: 16,
            pre: &r,
            x: bx as i32,
            y: by as i32,
            w: 16,
            h: 16,
        };
        assert_eq!(s.sad(3, -2), 0);
        let costs = MvCosts::new();
        let limits = MvLimits::for_block(7, 7, 2, 2, 16, 16);
        let (sad, mv) = fast_dia_search(&s, &limits, Mv::ZERO, 1, Mv::ZERO, &costs);
        assert_eq!(mv, vector(3, -2), "sad {sad}");
        let sub =
            find_best_sub_pixel_tree(&s, &limits, mv, Mv::ZERO, false, 1, QUARTER_PEL, &costs);
        assert_eq!(sub.mv, vector(24, -16));
        assert_eq!(sub.distortion, 0);
    }

    #[test]
    fn a_padded_reference_repeats_its_edges() {
        let p = Plane {
            data: vec![1, 2, 3, 4],
            stride: 2,
            alloc_height: 2,
            width: 2,
            height: 2,
            crop_width: 2,
            crop_height: 2,
        };
        let r = LumaRef::new(&p);
        assert_eq!(r.from(-5, -7)[0], 1);
        assert_eq!(r.from(9, -1)[0], 2);
        assert_eq!(r.from(-1, 40)[0], 3);
        assert_eq!(r.from(1, 1)[0], 4);
        assert!(r.from(-(INNER_BORDER as i32) - 1, 0).is_empty());
    }
}
