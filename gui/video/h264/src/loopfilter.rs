//! The deblocking filter (`h264_loopfilter.c`, with its edge filters from
//! `h264dsp_template.c` and `fill_filter_caches` from `h264_slice.c`).
//!
//! After a row of macroblocks is decoded, each 4x4 block edge is smoothed
//! where the samples either side of it differ by less than the QP says a
//! real edge would: hardest (strength 4) at an intra macroblock's border,
//! less inside one (3) or where coefficients were coded (2), least where
//! the two sides predict from different references or vectors more than a
//! sample apart (1), not at all elsewhere (0). `alpha` and `beta` bound the
//! steps that count as blocking; `tc0` bounds how far a sample moves.
//!
//! This is FFmpeg's general filter, `ff_h264_filter_mb`: its fast path needs
//! a SIMD strength routine its C build does not have, and so falls back to
//! this one there -- as the reference this decoder is held to runs.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on samples, QPs and table indices it bounds"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "sample indices are a macroblock's edges and the three samples either side, \
              inside the picture; cache indices are scan8's; table indices are QPs FFmpeg's \
              ranges bound, read through get where a damaged stream could push them"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int, unsigned and int8 conversions, made where it makes them"
)]

use crate::decoder::{Context, MAX_SLICES, SliceContext};
use crate::mbtype::{
    MB_TYPE_8X16, MB_TYPE_16X16, SCAN8, is_8x8dct, is_direct, is_inter, is_interlaced, is_intra,
    uses_list,
};
use crate::mvpred::{LIST_NOT_USED, fill, tab};
use crate::picture::{Pixel, table_offset};
use crate::slice::PICT_FRAME;

/// Which of the left neighbours is the top and which the bottom one
/// (`LTOP`, `LBOT`).
const LTOP: usize = 0;
const LBOT: usize = 1;

/// `h264_loop_filter_luma`: up to three samples each side of an edge moved
/// toward each other, by at most `tc0` (per four-sample segment, -1 to
/// skip it) plus one for each side smooth enough. `xstride` steps across
/// the edge, `ystride` along it.
#[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
fn luma<P: Pixel>(
    s: &mut [P],
    at: usize,
    xstride: isize,
    ystride: isize,
    inner_iters: usize,
    alpha: i32,
    beta: i32,
    tc0: [i8; 4],
    bits: u32,
) {
    let alpha = alpha << (bits - 8);
    let beta = beta << (bits - 8);
    let max = (1 << bits) - 1;
    let mut pix = at as isize;
    for tc in tc0 {
        let tc_orig = i32::from(tc) * (1 << (bits - 8));
        if tc_orig < 0 {
            pix += inner_iters as isize * ystride;
            continue;
        }
        for _ in 0..inner_iters {
            let px = |k: isize| (pix + k * xstride) as usize;
            let p0 = s[px(-1)].int();
            let p1 = s[px(-2)].int();
            let p2 = s[px(-3)].int();
            let q0 = s[px(0)].int();
            let q1 = s[px(1)].int();
            let q2 = s[px(2)].int();
            if (p0 - q0).abs() < alpha && (p1 - p0).abs() < beta && (q1 - q0).abs() < beta {
                let mut tc = tc_orig;
                if (p2 - p0).abs() < beta {
                    if tc_orig != 0 {
                        let d = (((p2 + ((p0 + q0 + 1) >> 1)) >> 1) - p1).clamp(-tc_orig, tc_orig);
                        s[px(-2)] = P::from_int(p1 + d);
                    }
                    tc += 1;
                }
                if (q2 - q0).abs() < beta {
                    if tc_orig != 0 {
                        let d = (((q2 + ((p0 + q0 + 1) >> 1)) >> 1) - q1).clamp(-tc_orig, tc_orig);
                        s[px(1)] = P::from_int(q1 + d);
                    }
                    tc += 1;
                }
                let delta = ((((q0 - p0) * 4) + (p1 - q1) + 4) >> 3).clamp(-tc, tc);
                s[px(-1)] = P::from_int((p0 + delta).clamp(0, max));
                s[px(0)] = P::from_int((q0 - delta).clamp(0, max));
            }
            pix += ystride;
        }
    }
}

/// `h264_loop_filter_luma_intra`: the strong filter of an intra
/// macroblock's border, up to three samples each side replaced by
/// averages where the edge is flat enough.
#[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
fn luma_intra<P: Pixel>(
    s: &mut [P],
    at: usize,
    xstride: isize,
    ystride: isize,
    inner_iters: usize,
    alpha: i32,
    beta: i32,
    bits: u32,
) {
    let alpha = alpha << (bits - 8);
    let beta = beta << (bits - 8);
    let mut pix = at as isize;
    for _ in 0..4 * inner_iters {
        let px = |k: isize| (pix + k * xstride) as usize;
        let p2 = s[px(-3)].int();
        let p1 = s[px(-2)].int();
        let p0 = s[px(-1)].int();
        let q0 = s[px(0)].int();
        let q1 = s[px(1)].int();
        let q2 = s[px(2)].int();
        if (p0 - q0).abs() < alpha && (p1 - p0).abs() < beta && (q1 - q0).abs() < beta {
            if (p0 - q0).abs() < ((alpha >> 2) + 2) {
                if (p2 - p0).abs() < beta {
                    let p3 = s[px(-4)].int();
                    s[px(-1)] = P::from_int((p2 + 2 * p1 + 2 * p0 + 2 * q0 + q1 + 4) >> 3);
                    s[px(-2)] = P::from_int((p2 + p1 + p0 + q0 + 2) >> 2);
                    s[px(-3)] = P::from_int((2 * p3 + 3 * p2 + p1 + p0 + q0 + 4) >> 3);
                } else {
                    s[px(-1)] = P::from_int((2 * p1 + p0 + q1 + 2) >> 2);
                }
                if (q2 - q0).abs() < beta {
                    let q3 = s[px(3)].int();
                    s[px(0)] = P::from_int((p1 + 2 * p0 + 2 * q0 + 2 * q1 + q2 + 4) >> 3);
                    s[px(1)] = P::from_int((p0 + q0 + q1 + q2 + 2) >> 2);
                    s[px(2)] = P::from_int((2 * q3 + 3 * q2 + q1 + q0 + p0 + 4) >> 3);
                } else {
                    s[px(0)] = P::from_int((2 * q1 + q0 + p1 + 2) >> 2);
                }
            } else {
                s[px(-1)] = P::from_int((2 * p1 + p0 + q1 + 2) >> 2);
                s[px(0)] = P::from_int((2 * q1 + q0 + p1 + 2) >> 2);
            }
        }
        pix += ystride;
    }
}

/// `h264_loop_filter_chroma`: one sample each side moved toward the
/// other by at most `tc0`, 0 to skip a segment.
#[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
fn chroma<P: Pixel>(
    s: &mut [P],
    at: usize,
    xstride: isize,
    ystride: isize,
    inner_iters: usize,
    alpha: i32,
    beta: i32,
    tc0: [i8; 4],
    bits: u32,
) {
    let alpha = alpha << (bits - 8);
    let beta = beta << (bits - 8);
    let max = (1 << bits) - 1;
    let mut pix = at as isize;
    for tc0 in tc0 {
        let tc = ((tc0 as u32).wrapping_sub(1) << (bits - 8)).wrapping_add(1) as i32;
        if tc <= 0 {
            pix += inner_iters as isize * ystride;
            continue;
        }
        for _ in 0..inner_iters {
            let px = |k: isize| (pix + k * xstride) as usize;
            let p0 = s[px(-1)].int();
            let p1 = s[px(-2)].int();
            let q0 = s[px(0)].int();
            let q1 = s[px(1)].int();
            if (p0 - q0).abs() < alpha && (p1 - p0).abs() < beta && (q1 - q0).abs() < beta {
                let delta = ((((q0 - p0) * 4) + (p1 - q1) + 4) >> 3).clamp(-tc, tc);
                s[px(-1)] = P::from_int((p0 + delta).clamp(0, max));
                s[px(0)] = P::from_int((q0 - delta).clamp(0, max));
            }
            pix += ystride;
        }
    }
}

/// `h264_loop_filter_chroma_intra`.
#[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
fn chroma_intra<P: Pixel>(
    s: &mut [P],
    at: usize,
    xstride: isize,
    ystride: isize,
    inner_iters: usize,
    alpha: i32,
    beta: i32,
    bits: u32,
) {
    let alpha = alpha << (bits - 8);
    let beta = beta << (bits - 8);
    let mut pix = at as isize;
    for _ in 0..4 * inner_iters {
        let px = |k: isize| (pix + k * xstride) as usize;
        let p0 = s[px(-1)].int();
        let p1 = s[px(-2)].int();
        let q0 = s[px(0)].int();
        let q1 = s[px(1)].int();
        if (p0 - q0).abs() < alpha && (p1 - p0).abs() < beta && (q1 - q0).abs() < beta {
            s[px(-1)] = P::from_int((2 * p1 + p0 + q1 + 2) >> 2);
            s[px(0)] = P::from_int((2 * q1 + q0 + p1 + 2) >> 2);
        }
        pix += ystride;
    }
}

/// The direction an edge runs, and with it how FFmpeg's `h264dsp` walks
/// it: a vertical edge (`h_loop_filter_*`) is crossed one sample at a time
/// and run down a row at a time; a horizontal one (`v_loop_filter_*`) the
/// other way about.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Vertical,
    Horizontal,
}

impl Dir {
    fn strides(self, stride: usize) -> (isize, isize) {
        match self {
            Self::Vertical => (1, stride as isize),
            Self::Horizontal => (stride as isize, 1),
        }
    }
}

/// What filtering an edge needs beyond its samples: the slice's offsets
/// into the alpha and beta tables, with the bit depth's QP offset taken
/// off, and the depth.
#[derive(Clone, Copy)]
struct Lf {
    a: i32,
    b: i32,
    bits: u32,
}

impl Lf {
    /// The tables' row and the alpha and beta for an edge at `qp`; `None`
    /// where either is 0, which leaves the edge alone.
    fn params(self, qp: i32) -> Option<(usize, i32, i32)> {
        let index_a = usize::try_from(qp + self.a).ok()?;
        let alpha = i32::from(*ALPHA_TABLE.get(index_a)?);
        let beta = i32::from(*BETA_TABLE.get(usize::try_from(qp + self.b).ok()?)?);
        (alpha != 0 && beta != 0).then_some((index_a, alpha, beta))
    }

    /// A luma edge (`filter_mb_edgev`, `filter_mb_edgeh`,
    /// `filter_mb_mbaff_edgev`): the strong filter where `intra` allows it
    /// and the first strength is 4, else the normal one. `inner_iters`
    /// samples a strength: 4, or 2 for one field of an MBAFF pair's edge.
    #[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
    fn luma_edge<P: Pixel>(
        self,
        s: &mut [P],
        at: usize,
        stride: usize,
        dir: Dir,
        inner_iters: usize,
        bs: [i16; 4],
        qp: i32,
        intra: bool,
    ) {
        let Some((index_a, alpha, beta)) = self.params(qp) else {
            return;
        };
        let (xs, ys) = dir.strides(stride);
        if bs[0] < 4 || !intra {
            let tc = bs.map(|b| TC0_TABLE[index_a][b as usize] as i8);
            luma(s, at, xs, ys, inner_iters, alpha, beta, tc, self.bits);
        } else {
            luma_intra(s, at, xs, ys, inner_iters, alpha, beta, self.bits);
        }
    }

    /// A chroma edge (`filter_mb_edgecv`, `filter_mb_edgech`,
    /// `filter_mb_mbaff_edgecv`): tc0 one more than luma's, its unsigned
    /// 255 for "skip" wrapping to 0.
    #[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
    fn chroma_edge<P: Pixel>(
        self,
        s: &mut [P],
        at: usize,
        stride: usize,
        dir: Dir,
        inner_iters: usize,
        bs: [i16; 4],
        qp: i32,
        intra: bool,
    ) {
        let Some((index_a, alpha, beta)) = self.params(qp) else {
            return;
        };
        let (xs, ys) = dir.strides(stride);
        if bs[0] < 4 || !intra {
            let tc = bs.map(|b| (u16::from(TC0_TABLE[index_a][b as usize]) + 1) as u8 as i8);
            chroma(s, at, xs, ys, inner_iters, alpha, beta, tc, self.bits);
        } else {
            chroma_intra(s, at, xs, ys, inner_iters, alpha, beta, self.bits);
        }
    }
}

/// A picture's three planes being filtered, and where the macroblock
/// starts in each.
pub(crate) struct Planes<'a, P> {
    pub(crate) y: &'a mut [P],
    pub(crate) cb: &'a mut [P],
    pub(crate) cr: &'a mut [P],
    pub(crate) at: [usize; 3],
}

/// Whether two neighbouring blocks predict differently enough to filter
/// their edge (`check_mv`): another reference, or a vector a sample or
/// more apart horizontally, `mvy_limit` quarter samples vertically -- in
/// a B slice either pairing of the two lists.
fn check_mv(sl: &SliceContext, b_idx: usize, bn_idx: usize, mvy_limit: i32) -> bool {
    let (r, mv) = (&sl.ref_cache, &sl.mv_cache);
    let apart = |a: [i16; 2], b: [i16; 2]| {
        (i32::from(a[0]) - i32::from(b[0]) + 3) as u32 >= 7
            || (i32::from(a[1]) - i32::from(b[1])).abs() >= mvy_limit
    };
    let mut v = r[0][b_idx] != r[0][bn_idx];
    if !v && r[0][b_idx] != -1 {
        v = apart(mv[0][b_idx], mv[0][bn_idx]);
    }
    if sl.hdr.list_count == 2 {
        if !v {
            v = r[1][b_idx] != r[1][bn_idx] || apart(mv[1][b_idx], mv[1][bn_idx]);
        }
        if v {
            if r[0][b_idx] != r[1][bn_idx] || r[1][b_idx] != r[0][bn_idx] {
                return true;
            }
            return apart(mv[0][b_idx], mv[1][bn_idx]) || apart(mv[1][b_idx], mv[0][bn_idx]);
        }
    }
    v
}

impl Context {
    fn cur_slot(&self) -> usize {
        self.cur_pic_ptr.unwrap_or(0)
    }

    /// Macroblock `xy`'s QP in the current picture.
    pub(crate) fn cur_qp(&self, xy: i32) -> i32 {
        let pic = &self.pics[self.cur_slot()];
        i32::from(pic.qscale_table[tab(table_offset(pic.mb_stride), xy)])
    }

    /// Macroblock `xy`'s type in the current picture.
    pub(crate) fn cur_type(&self, xy: i32) -> u32 {
        let pic = &self.pics[self.cur_slot()];
        pic.mb_type[tab(table_offset(pic.mb_stride), xy)]
    }

    /// `get_chroma_qp`.
    pub(crate) fn chroma_qp(&self, t: usize, qp: i32) -> i32 {
        self.ps
            .pps
            .as_deref()
            .and_then(|pps| pps.chroma_qp_table[t].get(qp as usize))
            .map_or(0, |&q| i32::from(q))
    }

    /// The filter for one macroblock (`ff_h264_filter_mb`): in an MBAFF
    /// frame beside a left pair of the other kind, the left edge's eight
    /// strengths and two QPs first; then the vertical edges and the
    /// horizontal ones.
    #[allow(
        clippy::too_many_lines,
        reason = "one function in FFmpeg, kept whole to read against it"
    )]
    pub(crate) fn filter_mb<P: Pixel>(
        &self,
        sl: &SliceContext,
        img: &mut Planes<'_, P>,
        mb_x: i32,
        mb_y: i32,
        linesize: usize,
        uvlinesize: usize,
    ) {
        let Some(sps) = self.ps.sps.as_deref() else {
            return;
        };
        let mb_xy = mb_x + mb_y * self.mb_stride;
        let mb_type = self.cur_type(mb_xy);
        let mvy_limit = if is_interlaced(mb_type) { 2 } else { 4 };
        let chroma = self.chroma_format_idc != 0;
        let qp_bd_offset = 6 * (sps.bit_depth_luma - 8);
        let lf = Lf {
            a: 52 + sl.hdr.slice_alpha_c0_offset - qp_bd_offset,
            b: 52 + sl.hdr.slice_beta_offset - qp_bd_offset,
            bits: sps.bit_depth_luma as u32,
        };
        let mb_field = sl.hdr.mb_field_decoding_flag != 0;
        let mut first_vertical_edge_done = false;

        if self.mb_aff_frame
            && is_interlaced(mb_type ^ sl.left_type[LTOP])
            && sl.left_type[LTOP] != 0
        {
            // The first vertical edge of an MBAFF pair beside one of the
            // other kind: 8 strengths and 2 QPs.
            first_vertical_edge_done = true;
            let mut bs = [4i16; 8];
            if !is_intra(mb_type) {
                const OFFSET: [[[u8; 8]; 2]; 2] = [
                    [[3, 3, 3, 3, 7, 7, 7, 7], [11, 11, 11, 11, 15, 15, 15, 15]],
                    [[3, 7, 11, 15, 3, 7, 11, 15], [3, 7, 11, 15, 3, 7, 11, 15]],
                ];
                let off = &OFFSET[usize::from(mb_field)][(mb_y & 1) as usize];
                let cabac = self.ps.pps.as_deref().is_some_and(|p| p.cabac != 0);
                for (i, slot) in bs.iter_mut().enumerate() {
                    let j = if mb_field { i >> 2 } else { i & 1 };
                    let mbn_xy = sl.left_mb_xy[j];
                    let mbn_type = sl.left_type[j];
                    *slot = if is_intra(mbn_type) {
                        4
                    } else {
                        let theirs = if !cabac && is_8x8dct(mbn_type) {
                            let bit = if if mb_field { i & 2 != 0 } else { mb_y & 1 != 0 } {
                                8
                            } else {
                                2
                            };
                            u32::from(self.cbp_table[mbn_xy as usize] & (bit << 12)) != 0
                        } else {
                            self.non_zero_count[mbn_xy as usize][usize::from(off[i])] != 0
                        };
                        1 + i16::from(sl.non_zero_count_cache[12 + 8 * (i >> 1)] != 0 || theirs)
                    };
                }
            }
            let mb_qp = self.cur_qp(mb_xy);
            let mbn_qp = [self.cur_qp(sl.left_mb_xy[0]), self.cur_qp(sl.left_mb_xy[1])];
            let qp = mbn_qp.map(|n| (mb_qp + n + 1) >> 1);
            let bqp = mbn_qp.map(|n| (self.chroma_qp(0, mb_qp) + self.chroma_qp(0, n) + 1) >> 1);
            let rqp = mbn_qp.map(|n| (self.chroma_qp(1, mb_qp) + self.chroma_qp(1, n) + 1) >> 1);
            let [y_at, cb_at, cr_at] = img.at;
            let half = |k: usize| -> [i16; 4] { [bs[k], bs[k + 1], bs[k + 2], bs[k + 3]] };
            let every_other = |k: usize| -> [i16; 4] { [bs[k], bs[k + 2], bs[k + 4], bs[k + 6]] };
            let chroma444 = self.chroma_format_idc == 3;
            let chroma422 = self.chroma_format_idc == 2;
            let v = Dir::Vertical;
            if mb_field {
                lf.luma_edge(img.y, y_at, linesize, v, 2, half(0), qp[0], true);
                lf.luma_edge(
                    img.y,
                    y_at + 8 * linesize,
                    linesize,
                    v,
                    2,
                    half(4),
                    qp[1],
                    true,
                );
                if chroma {
                    if chroma444 {
                        lf.luma_edge(img.cb, cb_at, uvlinesize, v, 2, half(0), bqp[0], true);
                        lf.luma_edge(
                            img.cb,
                            cb_at + 8 * uvlinesize,
                            uvlinesize,
                            v,
                            2,
                            half(4),
                            bqp[1],
                            true,
                        );
                        lf.luma_edge(img.cr, cr_at, uvlinesize, v, 2, half(0), rqp[0], true);
                        lf.luma_edge(
                            img.cr,
                            cr_at + 8 * uvlinesize,
                            uvlinesize,
                            v,
                            2,
                            half(4),
                            rqp[1],
                            true,
                        );
                    } else {
                        // 4:2:2's chroma is twice as tall: its halves are 8
                        // rows apart and 2 rows a strength.
                        let (down, inner) = if chroma422 { (8, 2) } else { (4, 1) };
                        lf.chroma_edge(img.cb, cb_at, uvlinesize, v, inner, half(0), bqp[0], true);
                        lf.chroma_edge(
                            img.cb,
                            cb_at + down * uvlinesize,
                            uvlinesize,
                            v,
                            inner,
                            half(4),
                            bqp[1],
                            true,
                        );
                        lf.chroma_edge(img.cr, cr_at, uvlinesize, v, inner, half(0), rqp[0], true);
                        lf.chroma_edge(
                            img.cr,
                            cr_at + down * uvlinesize,
                            uvlinesize,
                            v,
                            inner,
                            half(4),
                            rqp[1],
                            true,
                        );
                    }
                }
            } else {
                lf.luma_edge(img.y, y_at, 2 * linesize, v, 2, every_other(0), qp[0], true);
                lf.luma_edge(
                    img.y,
                    y_at + linesize,
                    2 * linesize,
                    v,
                    2,
                    every_other(1),
                    qp[1],
                    true,
                );
                if chroma {
                    if chroma444 {
                        lf.luma_edge(
                            img.cb,
                            cb_at,
                            2 * uvlinesize,
                            v,
                            2,
                            every_other(0),
                            bqp[0],
                            true,
                        );
                        lf.luma_edge(
                            img.cb,
                            cb_at + uvlinesize,
                            2 * uvlinesize,
                            v,
                            2,
                            every_other(1),
                            bqp[1],
                            true,
                        );
                        lf.luma_edge(
                            img.cr,
                            cr_at,
                            2 * uvlinesize,
                            v,
                            2,
                            every_other(0),
                            rqp[0],
                            true,
                        );
                        lf.luma_edge(
                            img.cr,
                            cr_at + uvlinesize,
                            2 * uvlinesize,
                            v,
                            2,
                            every_other(1),
                            rqp[1],
                            true,
                        );
                    } else {
                        let inner = if chroma422 { 2 } else { 1 };
                        lf.chroma_edge(
                            img.cb,
                            cb_at,
                            2 * uvlinesize,
                            v,
                            inner,
                            every_other(0),
                            bqp[0],
                            true,
                        );
                        lf.chroma_edge(
                            img.cb,
                            cb_at + uvlinesize,
                            2 * uvlinesize,
                            v,
                            inner,
                            every_other(1),
                            bqp[1],
                            true,
                        );
                        lf.chroma_edge(
                            img.cr,
                            cr_at,
                            2 * uvlinesize,
                            v,
                            inner,
                            every_other(0),
                            rqp[0],
                            true,
                        );
                        lf.chroma_edge(
                            img.cr,
                            cr_at + uvlinesize,
                            2 * uvlinesize,
                            v,
                            inner,
                            every_other(1),
                            rqp[1],
                            true,
                        );
                    }
                }
            }
        }

        for dir in [Dir::Vertical, Dir::Horizontal] {
            let done = dir == Dir::Vertical && first_vertical_edge_done;
            self.filter_mb_dir(
                sl, img, mb_y, linesize, uvlinesize, mb_xy, mb_type, mvy_limit, done, lf, chroma,
                dir,
            );
        }
    }

    /// One direction's edges of a macroblock (`filter_mb_dir`): the edge
    /// with the neighbour (above, or left) first, then the inner ones --
    /// all four, or only those between the partitions and transform blocks
    /// that can differ.
    #[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
    #[allow(
        clippy::too_many_lines,
        reason = "one function in FFmpeg, kept whole to read against it"
    )]
    fn filter_mb_dir<P: Pixel>(
        &self,
        sl: &SliceContext,
        img: &mut Planes<'_, P>,
        mb_y: i32,
        linesize: usize,
        uvlinesize: usize,
        mb_xy: i32,
        mb_type: u32,
        mvy_limit: i32,
        first_vertical_edge_done: bool,
        lf: Lf,
        chroma: bool,
        dir: Dir,
    ) {
        let d = usize::from(dir == Dir::Horizontal);
        let chroma444 = self.chroma_format_idc == 3;
        let chroma422 = self.chroma_format_idc == 2;
        let (mbm_xy, mbm_type) = if d == 0 {
            (mb_xy - 1, sl.left_type[LTOP])
        } else {
            (sl.top_mb_xy, sl.top_type)
        };
        let [y_at, cb_at, cr_at] = img.at;
        // How often a strength from vectors needs looking at again, across
        // the macroblock and along an edge.
        const MASK_EDGE_TAB: [[u32; 8]; 2] = [[0, 3, 3, 3, 1, 1, 1, 1], [0, 3, 1, 1, 3, 3, 3, 3]];
        let mask_edge = MASK_EDGE_TAB[d][((mb_type >> 3) & 7) as usize];
        let edges = if mask_edge == 3 && sl.cbp & 15 == 0 {
            1
        } else {
            4
        };
        let mask_par0 = mb_type & (MB_TYPE_16X16 | (MB_TYPE_8X16 >> d)) != 0;
        let step = if d == 0 { 1 } else { 8 };

        // Luma and chroma's edge at `offset` samples across, with chroma's
        // edge (the half-way one in chroma's smaller block) where there is
        // one.
        let edge = |img: &mut Planes<'_, P>,
                    bs: [i16; 4],
                    qp: i32,
                    cqp: [i32; 2],
                    k: usize,
                    intra: bool,
                    luma: bool,
                    chroma_k: Option<usize>| {
            let (across_y, across_c) = if d == 0 {
                (1, 1)
            } else {
                (linesize, uvlinesize)
            };
            if luma {
                lf.luma_edge(
                    img.y,
                    y_at + 4 * k * across_y,
                    linesize,
                    dir,
                    4,
                    bs,
                    qp,
                    intra,
                );
            }
            if !chroma {
                return;
            }
            if chroma444 {
                lf.luma_edge(
                    img.cb,
                    cb_at + 4 * k * across_c,
                    uvlinesize,
                    dir,
                    4,
                    bs,
                    cqp[0],
                    intra,
                );
                lf.luma_edge(
                    img.cr,
                    cr_at + 4 * k * across_c,
                    uvlinesize,
                    dir,
                    4,
                    bs,
                    cqp[1],
                    intra,
                );
            } else if let Some(ck) = chroma_k {
                // A vertical chroma edge of 4:2:2 runs 16 rows: four
                // samples a strength.
                let inner = if d == 0 && chroma422 { 4 } else { 2 };
                lf.chroma_edge(
                    img.cb,
                    cb_at + ck * across_c,
                    uvlinesize,
                    dir,
                    inner,
                    bs,
                    cqp[0],
                    intra,
                );
                lf.chroma_edge(
                    img.cr,
                    cr_at + ck * across_c,
                    uvlinesize,
                    dir,
                    inner,
                    bs,
                    cqp[1],
                    intra,
                );
            }
        };

        if mbm_type != 0 && !first_vertical_edge_done {
            if self.mb_aff_frame && d == 1 && mb_y & 1 == 0 && is_interlaced(mbm_type & !mb_type) {
                // A frame macroblock under a field pair: its top edge
                // filtered twice, once against each field.
                let mut mbn_xy = mb_xy - 2 * self.mb_stride;
                let cabac = self.ps.pps.as_deref().is_some_and(|p| p.cabac != 0);
                for j in 0..2usize {
                    let mbn_type = self.cur_type(mbn_xy);
                    let bs: [i16; 4] = if is_intra(mb_type | mbn_type) {
                        [3; 4]
                    } else if !cabac && is_8x8dct(mbn_type) {
                        let cbp = self.cbp_table[mbn_xy as usize];
                        core::array::from_fn(|i| {
                            let bit = if i < 2 { 0x4000 } else { 0x8000 };
                            1 + i16::from(
                                cbp & bit != 0 || sl.non_zero_count_cache[SCAN8[0] + i] != 0,
                            )
                        })
                    } else {
                        let nnz = &self.non_zero_count[mbn_xy as usize][12..16];
                        core::array::from_fn(|i| {
                            1 + i16::from(sl.non_zero_count_cache[SCAN8[0] + i] | nnz[i] != 0)
                        })
                    };
                    let qp = (self.cur_qp(mb_xy) + self.cur_qp(mbn_xy) + 1) >> 1;
                    let mbn_qp = self.cur_qp(mbn_xy);
                    let cqp = [
                        (sl.hdr.chroma_qp[0] + self.chroma_qp(0, mbn_qp) + 1) >> 1,
                        (sl.hdr.chroma_qp[1] + self.chroma_qp(1, mbn_qp) + 1) >> 1,
                    ];
                    let h = Dir::Horizontal;
                    lf.luma_edge(
                        img.y,
                        y_at + j * linesize,
                        2 * linesize,
                        h,
                        4,
                        bs,
                        qp,
                        false,
                    );
                    if chroma {
                        if chroma444 {
                            lf.luma_edge(
                                img.cb,
                                cb_at + j * uvlinesize,
                                2 * uvlinesize,
                                h,
                                4,
                                bs,
                                cqp[0],
                                false,
                            );
                            lf.luma_edge(
                                img.cr,
                                cr_at + j * uvlinesize,
                                2 * uvlinesize,
                                h,
                                4,
                                bs,
                                cqp[1],
                                false,
                            );
                        } else {
                            lf.chroma_edge(
                                img.cb,
                                cb_at + j * uvlinesize,
                                2 * uvlinesize,
                                h,
                                2,
                                bs,
                                cqp[0],
                                false,
                            );
                            lf.chroma_edge(
                                img.cr,
                                cr_at + j * uvlinesize,
                                2 * uvlinesize,
                                h,
                                2,
                                bs,
                                cqp[1],
                                false,
                            );
                        }
                    }
                    mbn_xy += self.mb_stride;
                }
            } else {
                let mut bs = [0i16; 4];
                if is_intra(mb_type | mbm_type) {
                    bs = [3; 4];
                    if !is_interlaced(mb_type | mbm_type)
                        || ((self.mb_aff_frame || self.picture_structure != PICT_FRAME) && d == 0)
                    {
                        bs = [4; 4];
                    }
                } else {
                    let mut mv_done = false;
                    if d == 1 && self.mb_aff_frame && is_interlaced(mb_type ^ mbm_type) {
                        bs = [1; 4];
                        mv_done = true;
                    } else if mask_par0 && mbm_type & (MB_TYPE_16X16 | (MB_TYPE_8X16 >> d)) != 0 {
                        let b_idx = 8 + 4;
                        bs = [i16::from(check_mv(sl, b_idx, b_idx - step, mvy_limit)); 4];
                        mv_done = true;
                    }
                    for (i, slot) in bs.iter_mut().enumerate() {
                        let (x, y) = if d == 0 { (0, i) } else { (i, 0) };
                        let b_idx = 8 + 4 + x + 8 * y;
                        let bn_idx = b_idx - step;
                        if sl.non_zero_count_cache[b_idx] | sl.non_zero_count_cache[bn_idx] != 0 {
                            *slot = 2;
                        } else if !mv_done {
                            *slot = i16::from(check_mv(sl, b_idx, bn_idx, mvy_limit));
                        }
                    }
                }
                if bs.iter().map(|&b| i32::from(b)).sum::<i32>() != 0 {
                    let mbm_qp = self.cur_qp(mbm_xy);
                    let qp = (self.cur_qp(mb_xy) + mbm_qp + 1) >> 1;
                    let cqp = [
                        (sl.hdr.chroma_qp[0] + self.chroma_qp(0, mbm_qp) + 1) >> 1,
                        (sl.hdr.chroma_qp[1] + self.chroma_qp(1, mbm_qp) + 1) >> 1,
                    ];
                    edge(img, bs, qp, cqp, 0, true, true, Some(0));
                }
            }
        }

        for e in 1..edges {
            let deblock_edge = !is_8x8dct(mb_type & ((e as u32) << 24));
            if !deblock_edge && (!chroma422 || d == 0) {
                continue;
            }
            let mut bs = [0i16; 4];
            if is_intra(mb_type) {
                bs = [3; 4];
            } else {
                let mut mv_done = false;
                if e & mask_edge as usize != 0 {
                    mv_done = true;
                } else if mask_par0 {
                    let b_idx = 8 + 4 + e * step;
                    bs = [i16::from(check_mv(sl, b_idx, b_idx - step, mvy_limit)); 4];
                    mv_done = true;
                }
                for (i, slot) in bs.iter_mut().enumerate() {
                    let (x, y) = if d == 0 { (e, i) } else { (i, e) };
                    let b_idx = 8 + 4 + x + 8 * y;
                    let bn_idx = b_idx - step;
                    if sl.non_zero_count_cache[b_idx] | sl.non_zero_count_cache[bn_idx] != 0 {
                        *slot = 2;
                    } else if !mv_done {
                        *slot = i16::from(check_mv(sl, b_idx, bn_idx, mvy_limit));
                    }
                }
                if bs == [0; 4] {
                    continue;
                }
            }
            let qp = self.cur_qp(mb_xy);
            let cqp = sl.hdr.chroma_qp;
            if d == 1 && chroma422 {
                // 4:2:2's chroma has a horizontal edge every 4 of its 16
                // rows: each luma edge's, the 8x8 transform's skipped ones
                // included.
                if deblock_edge {
                    lf.luma_edge(
                        img.y,
                        y_at + 4 * e * linesize,
                        linesize,
                        dir,
                        4,
                        bs,
                        qp,
                        false,
                    );
                }
                if chroma {
                    lf.chroma_edge(
                        img.cb,
                        cb_at + 4 * e * uvlinesize,
                        uvlinesize,
                        dir,
                        2,
                        bs,
                        cqp[0],
                        false,
                    );
                    lf.chroma_edge(
                        img.cr,
                        cr_at + 4 * e * uvlinesize,
                        uvlinesize,
                        dir,
                        2,
                        bs,
                        cqp[1],
                        false,
                    );
                }
            } else {
                let chroma_k = (e & 1 == 0).then_some(2 * e);
                edge(img, bs, qp, cqp, e, false, true, chroma_k);
            }
        }
    }

    /// One list's caches for the filter (`fill_filter_caches_inter`): the
    /// neighbours' and the macroblock's references, as pictures rather
    /// than list indices (`ref2frm`), and vectors.
    #[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
    fn fill_filter_caches_inter(
        &self,
        sl: &mut SliceContext,
        mb_type: u32,
        top_xy: i32,
        left_xy: [i32; 2],
        top_type: u32,
        left_type: [u32; 2],
        mb_xy: i32,
        list: usize,
    ) {
        let b_stride = self.b_stride as usize;
        let pic = &self.pics[self.cur_slot()];
        let mbaff_base = if sl.hdr.mb_mbaff != 0 { 20 } else { 2 };
        let s0 = SCAN8[0];
        let ref2frm = |xy_slice: u16, r: i8| -> i8 {
            let table = &self.ref2frm[usize::from(xy_slice) & (MAX_SLICES - 1)][list];
            table[(mbaff_base as isize + isize::from(r)) as usize] as i8
        };
        let slice_of = |xy: i32| self.slice_table[tab(table_offset(self.mb_stride), xy)];
        if is_inter(mb_type) || is_direct(mb_type) {
            if uses_list(top_type, list as u32) {
                let b_xy = self.mb2b_xy[top_xy as usize] as usize + 3 * b_stride;
                let b8_xy = 4 * top_xy as usize + 2;
                let s = slice_of(top_xy);
                sl.mv_cache[list][s0 - 8..s0 - 4]
                    .copy_from_slice(&pic.motion_val[list][4 + b_xy..4 + b_xy + 4]);
                let r0 = ref2frm(s, pic.ref_index[list][b8_xy]);
                let r1 = ref2frm(s, pic.ref_index[list][b8_xy + 1]);
                sl.ref_cache[list][s0 - 8..s0 - 4].copy_from_slice(&[r0, r0, r1, r1]);
            } else {
                sl.mv_cache[list][s0 - 8..s0 - 4].fill([0, 0]);
                sl.ref_cache[list][s0 - 8..s0 - 4].fill(LIST_NOT_USED);
            }
            if !is_interlaced(mb_type ^ left_type[LTOP]) {
                if uses_list(left_type[LTOP], list as u32) {
                    let b_xy = self.mb2b_xy[left_xy[LTOP] as usize] as usize + 3;
                    let b8_xy = 4 * left_xy[LTOP] as usize + 1;
                    let s = slice_of(left_xy[LTOP]);
                    for k in 0..4 {
                        sl.mv_cache[list][s0 - 1 + 8 * k] =
                            pic.motion_val[list][4 + b_xy + b_stride * k];
                    }
                    let r0 = ref2frm(s, pic.ref_index[list][b8_xy]);
                    let r1 = ref2frm(s, pic.ref_index[list][b8_xy + 2]);
                    for (k, r) in [r0, r0, r1, r1].into_iter().enumerate() {
                        sl.ref_cache[list][s0 - 1 + 8 * k] = r;
                    }
                } else {
                    for k in 0..4 {
                        sl.mv_cache[list][s0 - 1 + 8 * k] = [0, 0];
                        sl.ref_cache[list][s0 - 1 + 8 * k] = LIST_NOT_USED;
                    }
                }
            }
        }

        if !uses_list(mb_type, list as u32) {
            fill(&mut sl.mv_cache[list], s0, 4, 4, [0, 0]);
            fill(&mut sl.ref_cache[list], s0, 4, 4, LIST_NOT_USED);
            return;
        }
        let r = &pic.ref_index[list][4 * mb_xy as usize..4 * mb_xy as usize + 4];
        let own = sl.slice_num as u16;
        let refs = [
            ref2frm(own, r[0]),
            ref2frm(own, r[1]),
            ref2frm(own, r[2]),
            ref2frm(own, r[3]),
        ];
        for (row, pair) in [
            (0, [refs[0], refs[1]]),
            (1, [refs[0], refs[1]]),
            (2, [refs[2], refs[3]]),
            (3, [refs[2], refs[3]]),
        ] {
            let at = s0 + 8 * row;
            sl.ref_cache[list][at..at + 4].copy_from_slice(&[pair[0], pair[0], pair[1], pair[1]]);
        }
        let mv = &pic.motion_val[list];
        let base = 4 + 4 * sl.mb_x as usize + 4 * sl.mb_y as usize * b_stride;
        for row in 0..4 {
            let at = s0 + 8 * row;
            let from = base + row * b_stride;
            sl.mv_cache[list][at..at + 4].copy_from_slice(&mv[from..from + 4]);
        }
    }

    /// The caches the filter reads, from the picture's tables
    /// (`fill_filter_caches`): `true` where the macroblock and its
    /// neighbours' QPs are low enough that no edge would change.
    #[allow(
        clippy::too_many_lines,
        reason = "one function in FFmpeg, kept whole to read against it"
    )]
    pub(crate) fn fill_filter_caches(&self, sl: &mut SliceContext, mb_type: u32) -> bool {
        let mb_xy = sl.mb_xy;
        let mb_field = sl.hdr.mb_field_decoding_flag;
        let mut top_xy = mb_xy - (self.mb_stride << mb_field);
        let mut left_xy = [mb_xy - 1; 2];
        if self.mb_aff_frame {
            let left_mb_field_flag = is_interlaced(self.cur_type(mb_xy - 1));
            let curr_mb_field_flag = is_interlaced(mb_type);
            if sl.mb_y & 1 != 0 {
                if left_mb_field_flag != curr_mb_field_flag {
                    left_xy[LTOP] -= self.mb_stride;
                }
            } else {
                if curr_mb_field_flag {
                    top_xy += self.mb_stride & (((self.cur_type(top_xy) >> 7) & 1) as i32 - 1);
                }
                if left_mb_field_flag != curr_mb_field_flag {
                    left_xy[LBOT] += self.mb_stride;
                }
            }
        }
        sl.top_mb_xy = top_xy;
        sl.left_mb_xy = left_xy;

        // At a low enough QP filtering would change nothing: a
        // conservative estimate, as FFmpeg makes it.
        let qp_thresh = sl.qp_thresh;
        let qp = self.cur_qp(mb_xy);
        let low = |xy: i32| ((qp + self.cur_qp(xy) + 1) >> 1) <= qp_thresh;
        if qp <= qp_thresh
            && (left_xy[LTOP] < 0 || low(left_xy[LTOP]))
            && (top_xy < 0 || low(top_xy))
        {
            if !self.mb_aff_frame {
                return true;
            }
            if (left_xy[LTOP] < 0 || low(left_xy[LBOT]))
                && (top_xy < self.mb_stride || low(top_xy - self.mb_stride))
            {
                return true;
            }
        }

        let mut top_type = self.cur_type(top_xy);
        let mut left_type = [self.cur_type(left_xy[LTOP]), self.cur_type(left_xy[LBOT])];
        let slice_of = |xy: i32| self.slice_table[tab(table_offset(self.mb_stride), xy)];
        if sl.hdr.deblocking_filter == 2 {
            if slice_of(top_xy) != sl.slice_num as u16 {
                top_type = 0;
            }
            if slice_of(left_xy[LBOT]) != sl.slice_num as u16 {
                left_type = [0, 0];
            }
        } else {
            if slice_of(top_xy) == 0xFFFF {
                top_type = 0;
            }
            if slice_of(left_xy[LBOT]) == 0xFFFF {
                left_type = [0, 0];
            }
        }
        sl.top_type = top_type;
        sl.left_type = left_type;

        if is_intra(mb_type) {
            return false;
        }

        self.fill_filter_caches_inter(sl, mb_type, top_xy, left_xy, top_type, left_type, mb_xy, 0);
        if sl.hdr.list_count == 2 {
            self.fill_filter_caches_inter(
                sl, mb_type, top_xy, left_xy, top_type, left_type, mb_xy, 1,
            );
        }

        let nnz = &self.non_zero_count[mb_xy as usize];
        let cache = &mut sl.non_zero_count_cache;
        for row in 0..4 {
            cache[4 + 8 * (row + 1)..8 + 8 * (row + 1)].copy_from_slice(&nnz[4 * row..4 * row + 4]);
        }
        sl.cbp = i32::from(self.cbp_table[mb_xy as usize]);
        if top_type != 0 {
            let top = &self.non_zero_count[top_xy as usize];
            cache[4..8].copy_from_slice(&top[12..16]);
        }
        if left_type[LTOP] != 0 {
            let left = &self.non_zero_count[left_xy[LTOP] as usize];
            for row in 0..4 {
                cache[3 + 8 * (row + 1)] = left[3 + 4 * row];
            }
        }

        // CAVLC's 8x8 transform counts what residual decoding needs, not
        // what the filter does: the filter takes the coded block pattern's
        // bits, kept where FFmpeg keeps them (only whether one is set is
        // read).
        let cabac = self.ps.pps.as_deref().is_some_and(|p| p.cabac != 0);
        let transform_8x8 = self
            .ps
            .pps
            .as_deref()
            .is_some_and(|p| p.transform_8x8_mode != 0);
        if !cabac && transform_8x8 {
            let bit = |xy: i32, mask: u16| ((self.cbp_table[xy as usize] & mask) >> 12) as u8;
            if is_8x8dct(top_type) {
                let l = bit(top_xy, 0x4000);
                let r = bit(top_xy, 0x8000);
                cache[4..8].copy_from_slice(&[l, l, r, r]);
            }
            if is_8x8dct(left_type[LTOP]) {
                let v = bit(left_xy[LTOP], 0x2000);
                cache[3 + 8] = v;
                cache[3 + 16] = v;
            }
            if is_8x8dct(left_type[LBOT]) {
                let v = bit(left_xy[LBOT], 0x8000);
                cache[3 + 24] = v;
                cache[3 + 32] = v;
            }
            if is_8x8dct(mb_type) {
                for (block, mask) in [(0usize, 0x1000), (4, 0x2000), (8, 0x4000), (12, 0x8000)] {
                    let v = ((sl.cbp & mask) >> 12) as u8;
                    for k in 0..4 {
                        cache[SCAN8[block + k]] = v;
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests;

// The tables of h264_loopfilter.c; tc0's -1 entries are its uint8 255.
#[rustfmt::skip]
const ALPHA_TABLE: [u8; 156] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 4, 4, 5, 6, 7, 8, 9, 10, 12, 13,
    15, 17, 20, 22, 25, 28, 32, 36, 40, 45, 50, 56, 63,
    71, 80, 90, 101, 113, 127, 144, 162, 182, 203, 226, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
];
#[rustfmt::skip]
const BETA_TABLE: [u8; 156] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4,
    6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12,
    12, 13, 13, 14, 14, 15, 15, 16, 16, 17, 17, 18, 18,
    18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
    18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
    18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
    18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
];
#[rustfmt::skip]
const TC0_TABLE: [[u8; 4]; 156] = [
    [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0],
    [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0],
    [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0],
    [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0],
    [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0],
    [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0],
    [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0],
    [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0],
    [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0],
    [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0],
    [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0],
    [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 0], [255, 0, 0, 1], [255, 0, 0, 1], [255, 0, 0, 1],
    [255, 0, 0, 1], [255, 0, 1, 1], [255, 0, 1, 1], [255, 1, 1, 1], [255, 1, 1, 1], [255, 1, 1, 1],
    [255, 1, 1, 1], [255, 1, 1, 2], [255, 1, 1, 2], [255, 1, 1, 2], [255, 1, 1, 2], [255, 1, 2, 3],
    [255, 1, 2, 3], [255, 2, 2, 3], [255, 2, 2, 4], [255, 2, 3, 4], [255, 2, 3, 4], [255, 3, 3, 5],
    [255, 3, 4, 6], [255, 3, 4, 6], [255, 4, 5, 7], [255, 4, 5, 8], [255, 4, 6, 9], [255, 5, 7, 10],
    [255, 6, 8, 11], [255, 6, 8, 13], [255, 7, 10, 14], [255, 8, 11, 16], [255, 9, 12, 18], [255, 10, 13, 20],
    [255, 11, 15, 23], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25],
    [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25],
    [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25],
    [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25],
    [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25],
    [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25],
    [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25],
    [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25],
    [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25], [255, 13, 17, 25],
];
