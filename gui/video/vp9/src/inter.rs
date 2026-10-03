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
/// libvpx's `dec_build_inter_predictors_sb`.
pub(crate) fn build_inter_predictors_sb<P: Pixel>(
    frame: &mut FrameBuf<P>,
    refs: &[Option<(&FrameBuf<P>, ScaleFactors)>; 3],
    mi: &ModeInfo,
    pos: &BlockPos,
    bit_depth: u8,
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
        for plane in 0..3usize {
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
            };
            if mi.sb_type < BLOCK_8X8 {
                let n4_w = (pos.bw << 1) >> ss_x;
                let n4_h = (pos.bh << 1) >> ss_y;
                let mut i = 0usize;
                for y in 0..n4_h {
                    for x in 0..n4_w {
                        let mv = average_split_mvs(ss_x, ss_y, mi, r, i);
                        i += 1;
                        predict(frame, reference, &geo, 4 * x as i32, 4 * y as i32, 4, 4, mv);
                    }
                }
            } else {
                predict(frame, reference, &geo, 0, 0, n4w_x4, n4h_x4, mi.mv[r]);
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

/// Predict the `w` x `h` piece at (`x`, `y`) of a block from one reference:
/// libvpx's `dec_build_inter_predictors`.
#[allow(clippy::too_many_arguments)]
fn predict<P: Pixel>(
    frame: &mut FrameBuf<P>,
    reference: &FrameBuf<P>,
    g: &Geometry<'_>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    mv: Mv,
) {
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
    let dst_x = ((g.pos.mi_col * 8) >> g.ss_x) as i32 + x;
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
            let mut mc = vec![P::default(); b_w * b_h];
            build_mc_border(
                ref_plane,
                x0,
                y0,
                b_w,
                b_h,
                frame_width,
                frame_height,
                &mut mc,
            );
            let offset = y_pad * 3 * b_w + x_pad * 3;
            let src = Source {
                data: &mc,
                stride: b_w,
                origin: offset,
            };
            convolve(
                &src,
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
        };
        convolve(
            &src,
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
        let mut mc = vec![P::default(); b_w * b_h];
        build_mc_border(
            ref_plane,
            buf_x,
            buf_y,
            b_w,
            b_h,
            frame_width,
            frame_height,
            &mut mc,
        );
        let src = Source {
            data: &mc,
            stride: b_w,
            origin: 0,
        };
        convolve(
            &src,
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
    for (row, line) in out.chunks_mut(b_w).take(b_h).enumerate() {
        let yy = (y + row as i32).clamp(0, (h - 1).max(0)) as usize;
        let base = yy * stride;
        for (col, p) in line.iter_mut().enumerate() {
            let xx = (x + col as i32).clamp(0, (w - 1).max(0)) as usize;
            *p = plane.data.get(base + xx).copied().unwrap_or_default();
        }
    }
}

/// Where a prediction reads from: `data[origin]` is the reference block's
/// top-left pixel (before the filter's reach), rows `stride` apart.
struct Source<'a, P> {
    data: &'a [P],
    stride: usize,
    origin: usize,
}

impl<P: Pixel> Source<'_, P> {
    /// The sample at (`x`, `y`) relative to the origin; 0 outside the data,
    /// which the callers' geometry never asks for.
    #[inline(always)]
    fn at(&self, x: isize, y: isize) -> i32 {
        let i = self.origin as isize + y * self.stride as isize + x;
        if i < 0 {
            return 0;
        }
        self.data.get(i as usize).map_or(0, |p| p.int())
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

/// libvpx's `ROUND_POWER_OF_TWO(sum, FILTER_BITS)`, clipped.
#[inline(always)]
fn round_clip(sum: i32, max: i32) -> i32 {
    ((sum + 64) >> 7).clamp(0, max)
}

/// The prediction: libvpx's `vpx_convolve8` (and every special case of it),
/// written to `dst` or, for a compound block's second reference, averaged
/// into it.
#[allow(clippy::too_many_arguments)]
fn convolve<P: Pixel>(
    src: &Source<'_, P>,
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
    let mut out = [[0i32; 64]; 64];
    let horizontal = f.x0_q4 != 0 || f.x_step_q4 != 16;
    let vertical = f.y0_q4 != 0 || f.y_step_q4 != 16;
    let taps = |phase: i32| &f.kernel[(phase & SUBPEL_MASK) as usize];

    if !horizontal && !vertical {
        // vpx_convolve_copy.
        for (y, row) in out.iter_mut().enumerate().take(h) {
            for (x, v) in row.iter_mut().enumerate().take(w) {
                *v = src.at(x as isize, y as isize);
            }
        }
    } else if !vertical {
        // vpx_convolve8_horiz: no vertical pass.
        for (y, row) in out.iter_mut().enumerate().take(h) {
            let mut x_q4 = f.x0_q4;
            for v in row.iter_mut().take(w) {
                let sx = (x_q4 >> SUBPEL_BITS) as isize - 3;
                let k = taps(x_q4);
                let mut sum = 0i32;
                for (t, &tap) in k.iter().enumerate() {
                    sum += src.at(sx + t as isize, y as isize) * i32::from(tap);
                }
                *v = round_clip(sum, max);
                x_q4 += f.x_step_q4;
            }
        }
    } else if !horizontal {
        // vpx_convolve8_vert: no horizontal pass.
        for x in 0..w {
            let mut y_q4 = f.y0_q4;
            for row in out.iter_mut().take(h) {
                let sy = (y_q4 >> SUBPEL_BITS) as isize - 3;
                let k = taps(y_q4);
                let mut sum = 0i32;
                for (t, &tap) in k.iter().enumerate() {
                    sum += src.at(x as isize, sy + t as isize) * i32::from(tap);
                }
                row[x] = round_clip(sum, max);
                y_q4 += f.y_step_q4;
            }
        }
    } else {
        // vpx_convolve8: horizontally into an intermediate block that starts
        // three rows up, then vertically from it.
        let intermediate_height =
            ((((h as i32 - 1) * f.y_step_q4 + f.y0_q4) >> SUBPEL_BITS) as usize + SUBPEL_TAPS)
                .min(135);
        let mut temp = vec![0i32; 64 * 135];
        for (iy, row) in temp.chunks_mut(64).take(intermediate_height).enumerate() {
            let y = iy as isize - 3;
            let mut x_q4 = f.x0_q4;
            for v in row.iter_mut().take(w) {
                let sx = (x_q4 >> SUBPEL_BITS) as isize - 3;
                let k = taps(x_q4);
                let mut sum = 0i32;
                for (t, &tap) in k.iter().enumerate() {
                    sum += src.at(sx + t as isize, y) * i32::from(tap);
                }
                *v = round_clip(sum, max);
                x_q4 += f.x_step_q4;
            }
        }
        for x in 0..w {
            let mut y_q4 = f.y0_q4;
            for row in out.iter_mut().take(h) {
                // temp row 3 is the source's row 0.
                let sy = (y_q4 >> SUBPEL_BITS) as usize;
                let k = taps(y_q4);
                let mut sum = 0i32;
                for (t, &tap) in k.iter().enumerate() {
                    let v = temp.get((sy + t) * 64 + x).copied().unwrap_or(0);
                    sum += v * i32::from(tap);
                }
                row[x] = round_clip(sum, max);
                y_q4 += f.y_step_q4;
            }
        }
    }

    for (y, row) in out.iter().enumerate().take(h) {
        let start = dst_start + y * dst_stride;
        let Some(line) = dst.get_mut(start..start + w) else {
            continue;
        };
        for (d, &v) in line.iter_mut().zip(row) {
            *d = if avg {
                // vpx_convolve_avg: ROUND_POWER_OF_TWO(dst + pred, 1).
                P::from_int((d.int() + v + 1) >> 1)
            } else {
                P::from_int(v)
            };
        }
    }
}
