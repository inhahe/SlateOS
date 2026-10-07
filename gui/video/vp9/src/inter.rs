//! Inter prediction: a block predicted from reference frames, moved by its
//! motion vectors.
//!
//! A vector is in eighth pixels for luma (sixteenths for subsampled chroma,
//! after doubling), so most predictions interpolate: an 8-tap filter -- one
//! of three shapes, or bilinear -- horizontally, then vertically, each pass
//! rounded and clipped. A reference of a different size is scaled on the fly:
//! each output pixel steps through the reference by the size ratio, in
//! sixteenths. A compound block predicts from two references and averages.
//!
//! Where a reference block (with the filter's reach) crosses the reference
//! frame's edge, libvpx builds a copy with the edge pixels repeated outward
//! and predicts from that (`build_mc_border`); everywhere else it reads the
//! frame directly. Both are here, decided exactly as libvpx decides, because
//! which one runs decides pixels near the edge.
//!
//! Every libvpx prediction function -- copy, horizontal, vertical, 2-D,
//! scaled, and their averaging forms -- is the 2-D filter with some passes
//! the identity (tap 3 alone, weight 128), so this has one filter and skips
//! the identity passes.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/decoder/vp9_decodeframe.c`
//! (`dec_build_inter_predictors`, `extend_and_predict`, `build_mc_border`),
//! `vp9/common/vp9_reconinter.c`, `vp9_scale.c` and `vpx_dsp/vpx_convolve.c`
//! (copyright the WebM project authors), used under libvpx's BSD licence and
//! patent grant (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions are within frames of at most 65536 pixels in 1/16 units; filter sums are eight 12-bit samples times 7-bit taps"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "filter and block arrays are indexed by loop positions within their fixed sizes; frame samples are read through get"
)]

use crate::Error;
use crate::block::{BlockPos, ModeInfo};
use crate::common::{BILINEAR, BLOCK_8X8, EIGHTTAP_SHARP, EIGHTTAP_SMOOTH, LAST_FRAME, Mv};
use crate::frame::{FrameBuf, Pixel};
use crate::tables;

/// libvpx's `REF_SCALE_SHIFT`: scale factors are 14-bit fixed point.
const REF_SCALE_SHIFT: u32 = 14;
const REF_NO_SCALE: i32 = 1 << REF_SCALE_SHIFT;
const REF_INVALID_SCALE: i32 = -1;
const SUBPEL_BITS: u32 = 4;
const SUBPEL_MASK: i32 = 15;
const SUBPEL_SHIFTS: i32 = 16;
const SUBPEL_TAPS: usize = 8;
/// How far an 8-tap filter reaches past the block on the far side.
const VP9_INTERP_EXTEND: i32 = 4;

/// How a reference frame maps onto the current one: libvpx's
/// `scale_factors`, less its function pointers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScaleFactors {
    pub x_scale_fp: i32,
    pub y_scale_fp: i32,
    pub x_step_q4: i32,
    pub y_step_q4: i32,
}

impl ScaleFactors {
    /// A reference that may not be used.
    #[must_use]
    pub const fn invalid() -> Self {
        Self {
            x_scale_fp: REF_INVALID_SCALE,
            y_scale_fp: REF_INVALID_SCALE,
            x_step_q4: 16,
            y_step_q4: 16,
        }
    }

    /// libvpx's `vp9_setup_scale_factors_for_frame`.
    #[must_use]
    pub fn new(other_w: u32, other_h: u32, this_w: u32, this_h: u32) -> Self {
        if !crate::header::valid_ref_frame_size(other_w, other_h, this_w, this_h) {
            return Self::invalid();
        }
        let fp = |other: u32, this: u32| {
            ((i64::from(other) << REF_SCALE_SHIFT) / i64::from(this.max(1))) as i32
        };
        let mut sf = Self {
            x_scale_fp: fp(other_w, this_w),
            y_scale_fp: fp(other_h, this_h),
            x_step_q4: 16,
            y_step_q4: 16,
        };
        sf.x_step_q4 = sf.scale_x(16);
        sf.y_step_q4 = sf.scale_y(16);
        sf
    }

    /// libvpx's `vp9_is_valid_scale`.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.x_scale_fp != REF_INVALID_SCALE && self.y_scale_fp != REF_INVALID_SCALE
    }

    /// libvpx's `vp9_is_scaled`.
    #[must_use]
    pub fn is_scaled(&self) -> bool {
        self.is_valid() && (self.x_scale_fp != REF_NO_SCALE || self.y_scale_fp != REF_NO_SCALE)
    }

    /// libvpx's `scaled_x`.
    fn scale_x(&self, v: i32) -> i32 {
        ((i64::from(v) * i64::from(self.x_scale_fp)) >> REF_SCALE_SHIFT) as i32
    }

    /// libvpx's `scaled_y`.
    fn scale_y(&self, v: i32) -> i32 {
        ((i64::from(v) * i64::from(self.y_scale_fp)) >> REF_SCALE_SHIFT) as i32
    }

    /// libvpx's `scale_value_x`: scaled, or the value itself.
    fn value_x(&self, v: i32) -> i32 {
        if self.is_scaled() { self.scale_x(v) } else { v }
    }

    fn value_y(&self, v: i32) -> i32 {
        if self.is_scaled() { self.scale_y(v) } else { v }
    }
}

/// The filter a block's interpolation type names: libvpx's
/// `vp9_filter_kernels`.
fn kernel(filter: u8) -> &'static [[i16; 8]; 16] {
    match filter {
        EIGHTTAP_SMOOTH => &tables::SUB_PEL_FILTERS_8LP,
        EIGHTTAP_SHARP => &tables::SUB_PEL_FILTERS_8S,
        BILINEAR => &tables::BILINEAR_FILTERS,
        _ => &tables::SUB_PEL_FILTERS_8,
    }
}

/// Predict an inter block, every plane, from its one or two references:
/// libvpx's `dec_build_inter_predictors_sb`. `frame` is the frame, or a
/// strip of it beginning `x0` luma pixels in.
pub(crate) fn build_inter_predictors_sb<P: Pixel>(
    frame: &mut FrameBuf<P>,
    x0: usize,
    refs: &[Option<(&FrameBuf<P>, ScaleFactors)>; 3],
    mi: &ModeInfo,
    pos: &BlockPos,
    bit_depth: u8,
    scratch: &mut McScratch<P>,
) -> Result<(), Error> {
    build_inter_predictors(frame, x0, refs, mi, pos, bit_depth, scratch, 0..3)
}

/// [`build_inter_predictors_sb`] for the planes in `planes` only: libvpx's
/// `vp9_build_inter_predictors_sby` (0..1), `_sbuv` (1..3) and `_sbp`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_inter_predictors<P: Pixel>(
    frame: &mut FrameBuf<P>,
    x0: usize,
    refs: &[Option<(&FrameBuf<P>, ScaleFactors)>; 3],
    mi: &ModeInfo,
    pos: &BlockPos,
    bit_depth: u8,
    scratch: &mut McScratch<P>,
    planes: core::ops::Range<usize>,
) -> Result<(), Error> {
    let k = kernel(mi.interp_filter);
    let max = (1i32 << bit_depth.clamp(8, 12)) - 1;
    let mi_x = (pos.mi_col * 8) as i32;
    let mi_y = (pos.mi_row * 8) as i32;
    for r in 0..=usize::from(mi.has_second_ref()) {
        let rf = mi.ref_frame[r];
        let slot = (rf - LAST_FRAME).clamp(0, 2) as usize;
        let Some((reference, sf)) = refs[slot] else {
            return Err(Error::Corrupt(
                "a block names a reference frame the frame has none of",
            ));
        };
        if !sf.is_valid() {
            return Err(Error::Unsupported(
                "a reference frame has invalid dimensions",
            ));
        }
        for plane in planes.clone() {
            let (ss_x, ss_y) = if plane == 0 {
                (0, 0)
            } else {
                (u32::from(frame.ss_x), u32::from(frame.ss_y))
            };
            let n4w_x4 = (4 * ((pos.bw << 1) >> ss_x)) as i32;
            let n4h_x4 = (4 * ((pos.bh << 1) >> ss_y)) as i32;
            let geo = Geometry {
                plane,
                ss_x,
                ss_y,
                bw: n4w_x4,
                bh: n4h_x4,
                mi_x,
                mi_y,
                pos,
                sf,
                kernel: k,
                avg: r == 1,
                max,
                dst_x0: (x0 >> ss_x) as i32,
            };
            if mi.sb_type < BLOCK_8X8 {
                let n4_w = (pos.bw << 1) >> ss_x;
                let n4_h = (pos.bh << 1) >> ss_y;
                let mut i = 0usize;
                for y in 0..n4_h {
                    for x in 0..n4_w {
                        let mv = average_split_mvs(ss_x, ss_y, mi, r, i);
                        i += 1;
                        let piece = (4 * x as i32, 4 * y as i32, 4, 4);
                        predict(frame, reference, &geo, piece, mv, scratch);
                    }
                }
            } else {
                let piece = (0, 0, n4w_x4, n4h_x4);
                predict(frame, reference, &geo, piece, mi.mv[r], scratch);
            }
        }
    }
    Ok(())
}

/// libvpx's `average_split_mvs`: a sub-8x8 block's vector for a chroma
/// block that covers several luma sub-blocks.
fn average_split_mvs(ss_x: u32, ss_y: u32, mi: &ModeInfo, r: usize, block: usize) -> Mv {
    let mv = |b: usize| mi.bmi.get(b).map_or(Mv::ZERO, |m| m.mv[r]);
    let q2 = |v: i32| (if v < 0 { v - 1 } else { v + 1 }) / 2;
    let q4 = |v: i32| (if v < 0 { v - 2 } else { v + 2 }) / 4;
    let sum = |bs: &[usize]| -> (i32, i32) {
        bs.iter().fold((0, 0), |(r, c), &b| {
            (r + i32::from(mv(b).row), c + i32::from(mv(b).col))
        })
    };
    match (ss_x > 0, ss_y > 0) {
        (false, false) => mv(block),
        (false, true) => {
            let (r, c) = sum(&[block, block + 2]);
            Mv {
                row: q2(r) as i16,
                col: q2(c) as i16,
            }
        }
        (true, false) => {
            let (r, c) = sum(&[block, block + 1]);
            Mv {
                row: q2(r) as i16,
                col: q2(c) as i16,
            }
        }
        (true, true) => {
            let (r, c) = sum(&[0, 1, 2, 3]);
            Mv {
                row: q4(r) as i16,
                col: q4(c) as i16,
            }
        }
    }
}

/// The parts of a prediction common to every piece of a block in a plane.
struct Geometry<'a> {
    plane: usize,
    ss_x: u32,
    ss_y: u32,
    /// The block's size in this plane, in pixels.
    bw: i32,
    bh: i32,
    /// The block's position in luma pixels.
    mi_x: i32,
    mi_y: i32,
    pos: &'a BlockPos,
    sf: ScaleFactors,
    kernel: &'static [[i16; 8]; 16],
    /// The second reference of a compound block: average into what the
    /// first predicted.
    avg: bool,
    max: i32,
    /// Where the destination's column 0 is in the frame, in this plane:
    /// 0, or a strip's left edge.
    dst_x0: i32,
}

/// libvpx's `clamp_mv_to_umv_border_sb`: a vector so far out that no visible
/// pixel is used is cut back, with the same result.
fn clamp_mv_to_umv_border_sb(pos: &BlockPos, mv: Mv, bw: i32, bh: i32, ss_x: u32, ss_y: u32) -> Mv {
    let spel_left = (VP9_INTERP_EXTEND + bw) << SUBPEL_BITS;
    let spel_right = spel_left - SUBPEL_SHIFTS;
    let spel_top = (VP9_INTERP_EXTEND + bh) << SUBPEL_BITS;
    let spel_bottom = spel_top - SUBPEL_SHIFTS;
    let row = (i32::from(mv.row) * (1 << (1 - ss_y))) as i16;
    let col = (i32::from(mv.col) * (1 << (1 - ss_x))) as i16;
    let clamp = |v: i16, lo: i32, hi: i32| i32::from(v).clamp(lo, hi.max(lo)) as i16;
    Mv {
        col: clamp(
            col,
            pos.mb_to_left_edge * (1 << (1 - ss_x)) - spel_left,
            pos.mb_to_right_edge * (1 << (1 - ss_x)) + spel_right,
        ),
        row: clamp(
            row,
            pos.mb_to_top_edge * (1 << (1 - ss_y)) - spel_top,
            pos.mb_to_bottom_edge * (1 << (1 - ss_y)) + spel_bottom,
        ),
    }
}

/// Predict the `w` x `h` piece at (`x`, `y`) of a block -- `piece` is
/// `(x, y, w, h)` -- from one reference: libvpx's
/// `dec_build_inter_predictors`.
fn predict<P: Pixel>(
    frame: &mut FrameBuf<P>,
    reference: &FrameBuf<P>,
    g: &Geometry<'_>,
    piece: (i32, i32, i32, i32),
    mv: Mv,
    scratch: &mut McScratch<P>,
) {
    let (x, y, w, h) = piece;
    let McScratch {
        temp,
        border,
        zeros,
    } = scratch;
    let plane = g.plane;
    let ref_plane = &reference.planes[plane.min(2)];
    let frame_width = ref_plane.crop_width as i32;
    let frame_height = ref_plane.crop_height as i32;
    let sf = &g.sf;
    let is_scaled = sf.is_scaled();

    let (mut x0, mut y0, mut x0_16, mut y0_16, scaled_mv_row, scaled_mv_col, xs, ys);
    if is_scaled {
        let mv_q4 = clamp_mv_to_umv_border_sb(g.pos, mv, g.bw, g.bh, g.ss_x, g.ss_y);
        // The containing block's position, in pixels of this plane.
        let x_start = (-g.pos.mb_to_left_edge) >> (3 + g.ss_x);
        let y_start = (-g.pos.mb_to_top_edge) >> (3 + g.ss_y);
        // The piece's position in sixteenths, mapped into the reference.
        x0_16 = sf.value_x((x_start + x) << SUBPEL_BITS);
        y0_16 = sf.value_y((y_start + y) << SUBPEL_BITS);
        x0 = sf.value_x(x_start + x);
        y0 = sf.value_y(y_start + y);
        // vp9_scale_mv, at the luma position of the block plus the piece's
        // offset in this plane -- libvpx's mix, kept.
        let x_off_q4 = sf.scale_x((g.mi_x + x) << SUBPEL_BITS) & SUBPEL_MASK;
        let y_off_q4 = sf.scale_y((g.mi_y + y) << SUBPEL_BITS) & SUBPEL_MASK;
        scaled_mv_row = sf.scale_y(i32::from(mv_q4.row)) + y_off_q4;
        scaled_mv_col = sf.scale_x(i32::from(mv_q4.col)) + x_off_q4;
        xs = sf.x_step_q4;
        ys = sf.y_step_q4;
    } else {
        x0 = ((-g.pos.mb_to_left_edge) >> (3 + g.ss_x)) + x;
        y0 = ((-g.pos.mb_to_top_edge) >> (3 + g.ss_y)) + y;
        x0_16 = x0 << SUBPEL_BITS;
        y0_16 = y0 << SUBPEL_BITS;
        scaled_mv_row = i32::from(mv.row) * (1 << (1 - g.ss_y));
        scaled_mv_col = i32::from(mv.col) * (1 << (1 - g.ss_x));
        xs = 16;
        ys = 16;
    }
    let subpel_x = scaled_mv_col & SUBPEL_MASK;
    let subpel_y = scaled_mv_row & SUBPEL_MASK;

    // The top-left of the reference block: where libvpx's `buf_ptr` points,
    // before the border check below pads it.
    x0 += scaled_mv_col >> SUBPEL_BITS;
    y0 += scaled_mv_row >> SUBPEL_BITS;
    x0_16 += scaled_mv_col;
    y0_16 += scaled_mv_row;
    let (buf_x, buf_y) = (x0, y0);

    // The destination: this piece of the block in the current frame.
    let dst_plane = &mut frame.planes[plane.min(2)];
    let dst_x = ((g.pos.mi_col * 8) >> g.ss_x) as i32 + x - g.dst_x0;
    let dst_y = ((g.pos.mi_row * 8) >> g.ss_y) as i32 + y;
    let dst_stride = dst_plane.stride;
    let dst_start = dst_y as usize * dst_stride + dst_x as usize;
    let (w, h) = (w.max(0) as usize, h.max(0) as usize);

    let filters = Filters {
        kernel: g.kernel,
        x0_q4: subpel_x,
        x_step_q4: xs,
        y0_q4: subpel_y,
        y_step_q4: ys,
    };

    // Extend the border if there is motion, or the frame is not a multiple
    // of 8 pixels -- and the reference block reaches past its edge.
    if is_scaled
        || scaled_mv_col != 0
        || scaled_mv_row != 0
        || frame_width & 7 != 0
        || frame_height & 7 != 0
    {
        let mut y1 = ((y0_16 + (h as i32 - 1) * ys) >> SUBPEL_BITS) + 1;
        let mut x1 = ((x0_16 + (w as i32 - 1) * xs) >> SUBPEL_BITS) + 1;
        let (mut x_pad, mut y_pad) = (0, 0);
        if subpel_x != 0 || sf.x_step_q4 != SUBPEL_SHIFTS {
            x0 -= VP9_INTERP_EXTEND - 1;
            x1 += VP9_INTERP_EXTEND;
            x_pad = 1;
        }
        if subpel_y != 0 || sf.y_step_q4 != SUBPEL_SHIFTS {
            y0 -= VP9_INTERP_EXTEND - 1;
            y1 += VP9_INTERP_EXTEND;
            y_pad = 1;
        }
        if x0 < 0
            || x0 > frame_width - 1
            || x1 < 0
            || x1 > frame_width - 1
            || y0 < 0
            || y0 > frame_height - 1
            || y1 < 0
            || y1 > frame_height - 1
        {
            // extend_and_predict: the block with the frame's edge pixels
            // repeated, then the prediction from it.
            let b_w = (x1 - x0 + 1).max(1) as usize;
            let b_h = (y1 - y0 + 1).max(1) as usize;
            border.resize(b_w * b_h, P::default());
            build_mc_border(
                ref_plane,
                x0,
                y0,
                b_w,
                b_h,
                frame_width,
                frame_height,
                border,
            );
            let offset = y_pad * 3 * b_w + x_pad * 3;
            let src = Source {
                data: border,
                stride: b_w,
                origin: offset,
                zeros,
            };
            convolve(
                &src,
                temp,
                &mut dst_plane.data,
                dst_start,
                dst_stride,
                &filters,
                w,
                h,
                g.avg,
                g.max,
            );
            return;
        }
    }

    // Read the reference directly: the block (with the filter's reach) is
    // inside the frame, or does not move.
    let ref_stride = ref_plane.stride;
    let origin = buf_y as isize * ref_stride as isize + buf_x as isize;
    // Inside the frame, the filter's reach is too. A block that does not
    // move may reach past the reference's picture, but only where the
    // current frame's block does, which a reference of the same size has
    // room for; anything else is answered from the edge-extended copy.
    let reach_ok = buf_x >= 0
        && buf_y >= 0
        && (buf_x as usize + w) <= ref_stride
        && (buf_y as usize + h) <= ref_plane.alloc_height;
    if reach_ok {
        let src = Source {
            data: &ref_plane.data,
            stride: ref_stride,
            origin: origin as usize,
            zeros,
        };
        convolve(
            &src,
            temp,
            &mut dst_plane.data,
            dst_start,
            dst_stride,
            &filters,
            w,
            h,
            g.avg,
            g.max,
        );
    } else {
        let b_w = w.max(1);
        let b_h = h.max(1);
        border.resize(b_w * b_h, P::default());
        build_mc_border(
            ref_plane,
            buf_x,
            buf_y,
            b_w,
            b_h,
            frame_width,
            frame_height,
            border,
        );
        let src = Source {
            data: border,
            stride: b_w,
            origin: 0,
            zeros,
        };
        convolve(
            &src,
            temp,
            &mut dst_plane.data,
            dst_start,
            dst_stride,
            &filters,
            w,
            h,
            g.avg,
            g.max,
        );
    }
}

/// libvpx's `build_mc_border`: copy the `b_w` x `b_h` block at (`x`, `y`)
/// of `plane`, repeating the edge pixels of its `w` x `h` picture outward.
/// Each row is a run repeating the left edge, a copy, and a run repeating
/// the right edge, as libvpx builds it.
#[allow(clippy::too_many_arguments)]
fn build_mc_border<P: Pixel>(
    plane: &crate::frame::Plane<P>,
    x: i32,
    y: i32,
    b_w: usize,
    b_h: usize,
    w: i32,
    h: i32,
    out: &mut [P],
) {
    let stride = plane.stride;
    let (w, h) = (w.max(1), h.max(1));
    let bw = b_w as i32;
    let left = (-x).clamp(0, bw) as usize;
    let right = (x + bw - w).clamp(0, bw) as usize;
    let copy = b_w.saturating_sub(left + right);
    // The first column copied, and the picture's last.
    let first = (x + left as i32).max(0) as usize;
    let last = (w - 1) as usize;
    for (row, line) in out.chunks_exact_mut(b_w).take(b_h).enumerate() {
        let yy = (y + row as i32).clamp(0, h - 1) as usize;
        let Some(src) = plane.data.get(yy * stride..) else {
            line.fill(P::default());
            continue;
        };
        let (l, rest) = line.split_at_mut(left);
        let (c, r) = rest.split_at_mut(copy);
        l.fill(src.first().copied().unwrap_or_default());
        match src.get(first..first + copy) {
            Some(s) => c.copy_from_slice(s),
            None => c.fill(P::default()),
        }
        r.fill(src.get(last).copied().unwrap_or_default());
    }
}

/// Scratch space a frame's inter predictions reuse from block to block, so
/// that predicting allocates nothing.
pub(crate) struct McScratch<P> {
    /// The 2-D filter's horizontal pass: up to 135 rows of 64 (libvpx's
    /// `temp[64 * 135]`), clipped samples as libvpx keeps them.
    temp: Vec<P>,
    /// An edge-extended copy of a reference block (`build_mc_border`).
    border: Vec<P>,
    /// Zeros, read in place of a row the geometry never reaches outside its
    /// frame -- so a filter loop needs no check per sample.
    zeros: Vec<P>,
}

impl<P: Pixel> McScratch<P> {
    pub(crate) fn new() -> Self {
        Self {
            temp: vec![P::default(); 64 * 135],
            border: Vec::new(),
            zeros: vec![P::default(); ROW_SPAN_MAX],
        }
    }
}

/// The widest run of reference samples one output row reads: the 64th
/// output's position -- outputs step at most two samples, a reference being
/// at most twice the frame's size (`valid_ref_frame_size`), from a start of
/// at most 15/16 -- plus the 8-tap filter's reach.
const ROW_SPAN_MAX: usize = ((15 + 63 * 32) >> SUBPEL_BITS) + SUBPEL_TAPS;

/// Where a prediction reads from: `data[origin]` is the reference block's
/// top-left pixel (before the filter's reach), rows `stride` apart.
struct Source<'a, P> {
    data: &'a [P],
    stride: usize,
    origin: usize,
    /// Read in place of a row outside `data`.
    zeros: &'a [P],
}

impl<'a, P: Pixel> Source<'a, P> {
    /// `len` samples of row `y` from column `x`, both relative to the
    /// origin: one bounds check for the run. Zeros if it leaves the data,
    /// which the callers' geometry never asks for.
    #[inline(always)]
    fn row(&self, x: isize, y: isize, len: usize) -> &'a [P] {
        let start = self.origin as isize + y * self.stride as isize + x;
        usize::try_from(start)
            .ok()
            .and_then(|s| self.data.get(s..s + len))
            .or_else(|| self.zeros.get(..len))
            .unwrap_or_default()
    }
}

/// A prediction's filter positions: libvpx's convolve arguments.
struct Filters {
    kernel: &'static [[i16; 8]; 16],
    x0_q4: i32,
    x_step_q4: i32,
    y0_q4: i32,
    y_step_q4: i32,
}

/// libvpx's `ROUND_POWER_OF_TWO(sum, FILTER_BITS)`: the bias added before
/// the shift.
const ROUND: i32 = 64;

/// One row of the horizontal filter into `acc`, before the rounding shift:
/// libvpx's `convolve_horiz` for one row. `src` starts three samples left of
/// the first output. Unscaled, every output has the same phase, and the sum
/// is taken one tap at a time across the row, which the compiler vectorises.
#[inline(always)]
fn filter_h<P: Pixel>(src: &[P], f: &Filters, acc: &mut [i32]) {
    if f.x_step_q4 == 16 {
        let k = &f.kernel[(f.x0_q4 & SUBPEL_MASK) as usize];
        acc.fill(ROUND);
        for (t, &tap) in k.iter().enumerate() {
            let tap = i32::from(tap);
            let s = src.get(t..).unwrap_or_default();
            for (a, &p) in acc.iter_mut().zip(s) {
                *a += p.int() * tap;
            }
        }
    } else {
        let mut x_q4 = f.x0_q4;
        for a in acc.iter_mut() {
            let k = &f.kernel[(x_q4 & SUBPEL_MASK) as usize];
            let at = (x_q4 >> SUBPEL_BITS) as usize;
            let s = src.get(at..at + SUBPEL_TAPS).unwrap_or_default();
            *a = ROUND
                + s.iter()
                    .zip(k)
                    .map(|(&p, &tap)| p.int() * i32::from(tap))
                    .sum::<i32>();
            x_q4 += f.x_step_q4;
        }
    }
}

/// One row of the vertical filter into `acc`, before the rounding shift,
/// from the eight rows it reads: libvpx's `convolve_vert` for one row.
#[inline(always)]
fn filter_v<P: Pixel>(rows: [&[P]; SUBPEL_TAPS], k: &[i16; 8], acc: &mut [i32]) {
    acc.fill(ROUND);
    for (row, &tap) in rows.iter().zip(k) {
        let tap = i32::from(tap);
        for (a, &p) in acc.iter_mut().zip(*row) {
            *a += p.int() * tap;
        }
    }
}

/// Write a row of filter sums to `dst`, shifted and clipped -- or, for a
/// compound block's second reference, averaged into what is there
/// (libvpx's `vpx_convolve_avg`: `ROUND_POWER_OF_TWO(dst + pred, 1)`).
#[inline(always)]
fn store<P: Pixel>(dst: &mut [P], acc: &[i32], avg: bool, max: i32) {
    if avg {
        for (d, &a) in dst.iter_mut().zip(acc) {
            *d = P::from_int((d.int() + (a >> 7).clamp(0, max) + 1) >> 1);
        }
    } else {
        for (d, &a) in dst.iter_mut().zip(acc) {
            *d = P::from_int((a >> 7).clamp(0, max));
        }
    }
}

// --- 8-bit samples, unscaled: the common case, in 16-bit lanes ------------------------------
//
// Baseline x86-64 (SSE2) multiplies 16-bit lanes eight at a time and has no
// 32-bit multiply, so the 8-bit filters take their sums in 16 bits. A sample
// times a tap fits (255 * 128 < 2^15); a whole sum need not, but once
// `SUM_OFFSET` is added it lies in 0..=65535 for every kernel -- checked
// below, at compile time -- so wrapping arithmetic modulo 2^16 gives it
// exactly. The results are libvpx's, bit for bit.

/// Added to every 8-bit sum so that it is never negative and never past
/// 65535. A multiple of 128, so the rounding shift can be taken before it is
/// removed: `(sum + ROUND + SUM_OFFSET) >> 7` is `((sum + ROUND) >> 7) + 128`.
const SUM_OFFSET: i32 = 128 * 128;

/// Every kernel's sums, offset, fit 16 bits: the most negative sum is 255
/// times the negative taps, the most positive 255 times the positive ones.
const _: () = {
    let kernels = [
        &tables::SUB_PEL_FILTERS_8,
        &tables::SUB_PEL_FILTERS_8LP,
        &tables::SUB_PEL_FILTERS_8S,
        &tables::BILINEAR_FILTERS,
    ];
    let mut i = 0;
    while i < kernels.len() {
        let mut phase = 0;
        while phase < 16 {
            let (mut pos, mut neg, mut t) = (0i32, 0i32, 0);
            while t < 8 {
                let tap = kernels[i][phase][t] as i32;
                if tap > 0 {
                    pos += tap;
                } else {
                    neg += tap;
                }
                t += 1;
            }
            assert!(255 * neg + SUM_OFFSET >= 0);
            assert!(255 * pos + SUM_OFFSET + ROUND <= 65535);
            phase += 1;
        }
        i += 1;
    }
};

/// A 16-bit sum, offset and rounded, as the clipped sample: libvpx's
/// `clip_pixel(ROUND_POWER_OF_TWO(sum, FILTER_BITS))`. In 16-bit lanes:
/// a shift, a subtraction and a saturating pack.
#[inline(always)]
fn finish_u8(acc: u16) -> u8 {
    ((acc >> 7) as i16 - (SUM_OFFSET >> 7) as i16).clamp(0, 255) as u8
}

/// A kernel's taps, each repeated across eight lanes: made once per block,
/// so the rows do not each broadcast them again.
type Taps = [[u16; 8]; SUBPEL_TAPS];

fn splat(k: &[i16; 8]) -> Taps {
    core::array::from_fn(|t| [k[t] as u16; 8])
}

/// Add `tap` times `src` into `acc`, lane by lane, eight lanes at a time
/// (fewer, for a block four wide).
#[inline(always)]
fn mac_u8(acc: &mut [u16], src: &[u8], tap: &[u16; 8]) {
    for (a8, s8) in acc.chunks_mut(8).zip(src.chunks(8)) {
        for ((a, &p), &k) in a8.iter_mut().zip(s8).zip(tap) {
            *a = a.wrapping_add(u16::from(p).wrapping_mul(k));
        }
    }
}

/// One row of the horizontal filter at one phase, `W` outputs: output `j`
/// from `src[j..j + 8]`, so `src` starts three samples left of the first
/// output. A short `src`, which the geometry never gives, reads as zeros.
///
/// Each tap is bounds-checked on its own. That is not caution: with a single
/// check for all eight, the compiler sees each output as a dot product of
/// eight contiguous samples with the eight taps and vectorises *that* -- a
/// sum across lanes per output, several times the work -- where a check per
/// tap leaves it one multiply-add per tap across the outputs.
#[inline(always)]
fn h_row_u8<const W: usize>(src: &[u8], taps: &Taps) -> [u16; W] {
    let mut acc = [(SUM_OFFSET + ROUND) as u16; W];
    for (t, tap) in taps.iter().enumerate() {
        if let Some(s) = src.get(t..t + W) {
            mac_u8(&mut acc, s, tap);
        }
    }
    acc
}

/// One row of the vertical filter at one phase, `W` outputs, from the eight
/// rows it reads. A short row reads as zeros.
#[inline(always)]
fn v_row_u8<const W: usize>(rows: &[&[u8]; SUBPEL_TAPS], taps: &Taps) -> [u16; W] {
    let mut acc = [(SUM_OFFSET + ROUND) as u16; W];
    for (row, tap) in rows.iter().zip(taps) {
        if let Some(s) = row.get(..W) {
            mac_u8(&mut acc, s, tap);
        }
    }
    acc
}

/// Write a row of filter results to `dst`, or average them into it
/// (libvpx's `vpx_convolve_avg`: `ROUND_POWER_OF_TWO(dst + pred, 1)`).
/// Written straight into `dst`, the clip vectorises to a shift, a saturating
/// subtraction, a minimum and a pack.
#[inline(always)]
fn put_u8<const W: usize>(dst: &mut [u8], acc: &[u16; W], avg: bool) {
    let Some(dst) = dst.get_mut(..W) else {
        return;
    };
    if avg {
        for (d, &a) in dst.iter_mut().zip(acc) {
            *d = ((u16::from(*d) + u16::from(finish_u8(a)) + 1) >> 1) as u8;
        }
    } else {
        for (d, &a) in dst.iter_mut().zip(acc) {
            *d = finish_u8(a);
        }
    }
}

/// `convolve` for 8-bit samples, an unscaled reference and a block `W`
/// samples wide. With the width a constant, every row has a length the
/// compiler knows, and each pass is a handful of vector operations per row.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn convolve_u8<const W: usize>(
    src: &Source<'_, u8>,
    temp: &mut [u8],
    dst: &mut [u8],
    dst_start: usize,
    dst_stride: usize,
    f: &Filters,
    h: usize,
    avg: bool,
) {
    let kx = &splat(&f.kernel[(f.x0_q4 & SUBPEL_MASK) as usize]);
    let ky = &splat(&f.kernel[(f.y0_q4 & SUBPEL_MASK) as usize]);
    match (f.x0_q4 != 0, f.y0_q4 != 0) {
        (false, false) => {
            // vpx_convolve_copy, or vpx_convolve_avg.
            for y in 0..h {
                let s = src.row(0, y as isize, W);
                let Some(d) = dst
                    .get_mut(dst_start + y * dst_stride..)
                    .and_then(|d| d.get_mut(..W))
                else {
                    continue;
                };
                if avg {
                    for (d, &p) in d.iter_mut().zip(s) {
                        *d = ((u16::from(*d) + u16::from(p) + 1) >> 1) as u8;
                    }
                } else if d.len() == s.len() {
                    d.copy_from_slice(s);
                }
            }
        }
        (true, false) => {
            for y in 0..h {
                let acc = h_row_u8::<W>(src.row(-3, y as isize, W + SUBPEL_TAPS - 1), kx);
                if let Some(d) = dst.get_mut(dst_start + y * dst_stride..) {
                    put_u8(d, &acc, avg);
                }
            }
        }
        (false, true) => {
            for y in 0..h {
                let rows = core::array::from_fn(|t| src.row(0, (y + t) as isize - 3, W));
                let acc = v_row_u8::<W>(&rows, ky);
                if let Some(d) = dst.get_mut(dst_start + y * dst_stride..) {
                    put_u8(d, &acc, avg);
                }
            }
        }
        (true, true) => {
            // Horizontally into seven more rows than the block, starting
            // three above it, `W` apart; then vertically from those.
            for (iy, out) in temp
                .chunks_exact_mut(W)
                .take(h + SUBPEL_TAPS - 1)
                .enumerate()
            {
                let acc = h_row_u8::<W>(src.row(-3, iy as isize - 3, W + SUBPEL_TAPS - 1), kx);
                put_u8(out, &acc, false);
            }
            let temp: &[u8] = temp;
            for y in 0..h {
                let rows = core::array::from_fn(|t| temp.get((y + t) * W..).unwrap_or_default());
                let acc = v_row_u8::<W>(&rows, ky);
                if let Some(d) = dst.get_mut(dst_start + y * dst_stride..) {
                    put_u8(d, &acc, avg);
                }
            }
        }
    }
}

/// The prediction: libvpx's `vpx_convolve8` (and every special case of it),
/// written to `dst` or, for a compound block's second reference, averaged
/// into it. `temp` is the 2-D filter's intermediate block.
#[allow(clippy::too_many_arguments)]
fn convolve<P: Pixel>(
    src: &Source<'_, P>,
    temp: &mut [P],
    dst: &mut [P],
    dst_start: usize,
    dst_stride: usize,
    f: &Filters,
    w: usize,
    h: usize,
    avg: bool,
    max: i32,
) {
    let w = w.min(64);
    let h = h.min(64);
    if f.x_step_q4 == 16
        && f.y_step_q4 == 16
        && let (Some(data), Some(zeros), Some(temp8), Some(dst8)) = (
            P::bytes(src.data),
            P::bytes(src.zeros),
            P::bytes_mut(temp),
            P::bytes_mut(dst),
        )
    {
        let src8 = Source {
            data,
            stride: src.stride,
            origin: src.origin,
            zeros,
        };
        let args = (&src8, temp8, dst8, dst_start, dst_stride, f, h, avg);
        let (s, t, d, ds, dss, f, h, avg) = args;
        match w {
            4 => return convolve_u8::<4>(s, t, d, ds, dss, f, h, avg),
            8 => return convolve_u8::<8>(s, t, d, ds, dss, f, h, avg),
            16 => return convolve_u8::<16>(s, t, d, ds, dss, f, h, avg),
            32 => return convolve_u8::<32>(s, t, d, ds, dss, f, h, avg),
            64 => return convolve_u8::<64>(s, t, d, ds, dss, f, h, avg),
            // Every block is 4 to 64 samples wide, a power of two; anything
            // else takes the general path below.
            _ => {}
        }
    }
    let horizontal = f.x0_q4 != 0 || f.x_step_q4 != 16;
    let vertical = f.y0_q4 != 0 || f.y_step_q4 != 16;
    // The source samples one row of the horizontal filter reads.
    let span = ((((w as i32 - 1) * f.x_step_q4 + f.x0_q4) >> SUBPEL_BITS) as usize + SUBPEL_TAPS)
        .min(ROW_SPAN_MAX);
    let mut acc = [0i32; 64];
    let acc = &mut acc[..w];

    if !horizontal && !vertical {
        // vpx_convolve_copy.
        for y in 0..h {
            let s = src.row(0, y as isize, w);
            let Some(d) = dst
                .get_mut(dst_start + y * dst_stride..)
                .and_then(|d| d.get_mut(..w))
            else {
                continue;
            };
            if avg {
                for (d, &p) in d.iter_mut().zip(s) {
                    *d = P::from_int((d.int() + p.int() + 1) >> 1);
                }
            } else if d.len() == s.len() {
                d.copy_from_slice(s);
            }
        }
    } else if !vertical {
        // vpx_convolve8_horiz: no vertical pass.
        for y in 0..h {
            filter_h(src.row(-3, y as isize, span), f, acc);
            let Some(d) = dst
                .get_mut(dst_start + y * dst_stride..)
                .and_then(|d| d.get_mut(..w))
            else {
                continue;
            };
            store(d, acc, avg, max);
        }
    } else if !horizontal {
        // vpx_convolve8_vert: no horizontal pass, the rows read straight
        // from the source, starting three above the output's.
        for y in 0..h {
            let y_q4 = f.y0_q4 + y as i32 * f.y_step_q4;
            let top = (y_q4 >> SUBPEL_BITS) as isize - 3;
            let rows = core::array::from_fn(|t| src.row(0, top + t as isize, w));
            filter_v(rows, &f.kernel[(y_q4 & SUBPEL_MASK) as usize], acc);
            let Some(d) = dst
                .get_mut(dst_start + y * dst_stride..)
                .and_then(|d| d.get_mut(..w))
            else {
                continue;
            };
            store(d, acc, avg, max);
        }
    } else {
        // vpx_convolve8: horizontally into an intermediate block that starts
        // three rows up, then vertically from it.
        let intermediate_height =
            ((((h as i32 - 1) * f.y_step_q4 + f.y0_q4) >> SUBPEL_BITS) as usize + SUBPEL_TAPS)
                .min(135);
        for (iy, out) in temp
            .chunks_exact_mut(64)
            .take(intermediate_height)
            .enumerate()
        {
            filter_h(src.row(-3, iy as isize - 3, span), f, acc);
            for (o, &a) in out.iter_mut().zip(acc.iter()) {
                *o = P::from_int((a >> 7).clamp(0, max));
            }
        }
        let temp: &[P] = temp;
        for y in 0..h {
            let y_q4 = f.y0_q4 + y as i32 * f.y_step_q4;
            // Intermediate row 3 is the source's row 0, so the filter's
            // reach starts at the output row's own index.
            let top = (y_q4 >> SUBPEL_BITS) as usize;
            let rows = core::array::from_fn(|t| {
                let start = (top + t) * 64;
                temp.get(start..start + w).unwrap_or_default()
            });
            filter_v(rows, &f.kernel[(y_q4 & SUBPEL_MASK) as usize], acc);
            let Some(d) = dst
                .get_mut(dst_start + y * dst_stride..)
                .and_then(|d| d.get_mut(..w))
            else {
                continue;
            };
            store(d, acc, avg, max);
        }
    }
}
