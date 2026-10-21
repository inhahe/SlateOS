//! Inter prediction: a macroblock's partitions predicted from their
//! references (`h264_mc_template.c` and `mc_dir_part`, `mc_part_std`,
//! `mc_part_weighted` in `h264_mb.c`), with the quarter-sample luma filter
//! (`h264qpel_template.c`), the eighth-sample chroma filter
//! (`h264chroma_template.c`) and weighted prediction
//! (`h264dsp_template.c`).
//!
//! Luma between samples is interpolated by a six-tap filter (1, -5, 20,
//! 20, -5, 1), quarter positions by averaging a half position with its
//! neighbour; chroma bilinearly in eighths. A bi-predicted partition
//! averages its two predictions, or weights them.
//!
//! FFmpeg reads a reference block straight from the picture, or -- where
//! the block and its filter's reach cross the picture's edge -- from a
//! copy with the edge samples repeated outward (`emulated_edge_mc`). Its
//! test for that is conservative, so every read it makes in place is
//! inside the picture; this always copies the reach of the filter, edges
//! repeated, which reads the same samples either way. The copy also lets a
//! field predict from the other field of the frame being decoded.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on samples, vectors and weights, bounded by their ranges"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "window and block indices are under the sizes they were made with; sample \
              indices are clamped into the plane, or the partition's inside the picture"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int and int16 conversions, made where it makes them"
)]

use crate::decoder::{Context, DecodeError, SliceContext};
use crate::mbtype::{
    SCAN8, is_8x16, is_16x8, is_16x16, is_dir, is_sub_4x8, is_sub_8x4, is_sub_8x8,
};
use crate::picture::{Pixel, Ref};

/// A sample clipped to the depth (`CLIP`, `av_clip_pixel`).
fn clip(v: i32, bits: u32) -> i32 {
    v.clamp(0, (1 << bits) - 1)
}

/// Two predictions averaged, rounding up (`rnd_avg`).
fn avg2(a: i32, b: i32) -> i32 {
    (a + b + 1) >> 1
}

/// The largest luma block and its filter's reach: 16 samples and 2 before,
/// 3 after.
const WIN: usize = 16 + 5;

/// The buffers a block's filters work in: a reference window, and the
/// filters' passes.
#[derive(Clone, Debug)]
pub(crate) struct Filters {
    /// The samples a filter reads around a block, edges repeated where they
    /// reach past the picture (FFmpeg's `edge_emu_buffer`).
    win: [i32; WIN * WIN],
    /// The two-dimensional half position's first pass: a row of horizontal
    /// sums for each of the block's rows and the vertical filter's reach
    /// (`pixeltmp`).
    tmp: [i32; WIN * 16],
    /// The horizontal and the vertical half position, a block of each, `w`
    /// a row: what the quarter positions average.
    half_h: [i32; 256],
    half_v: [i32; 256],
}

/// What a macroblock's prediction is made in, kept from macroblock to
/// macroblock so that nothing is allocated for each.
#[derive(Clone, Debug)]
pub(crate) struct McScratch {
    filters: Filters,
    /// A list's prediction of each plane, row by row, `w` samples a row.
    blocks: [[i32; 256]; 3],
    /// The second list's, where weighted bi-prediction keeps both
    /// (FFmpeg's `bipred_scratchpad`).
    kept: [[i32; 256]; 3],
}

impl Default for McScratch {
    fn default() -> Self {
        Self {
            filters: Filters {
                win: [0; WIN * WIN],
                tmp: [0; WIN * 16],
                half_h: [0; 256],
                half_v: [0; 256],
            },
            blocks: [[0; 256]; 3],
            kept: [[0; 256]; 3],
        }
    }
}

/// A reference window's geometry: `w` by `h` samples from (`x0`, `y0`) of
/// a picture `pic_w` by `pic_h` whose sample (0, 0) is `base` in a plane
/// with rows `stride` apart.
#[derive(Clone, Copy)]
struct Reach {
    base: usize,
    stride: usize,
    x0: i32,
    y0: i32,
    w: usize,
    h: usize,
    pic_w: i32,
    pic_h: i32,
}

/// Copy `r`'s samples of `plane` into `win`, `r.w` a row, each clamped into
/// the picture: the edge samples repeated outward, as FFmpeg's
/// `emulated_edge_mc` repeats them.
fn fetch<P: Pixel>(win: &mut [i32], plane: &[P], r: Reach) {
    let xs_inside = r.x0 >= 0 && r.x0 + r.w as i32 <= r.pic_w;
    for j in 0..r.h {
        let y = (r.y0 + j as i32).clamp(0, r.pic_h - 1) as usize;
        let row = r.base + y * r.stride;
        let out = &mut win[j * r.w..(j + 1) * r.w];
        if xs_inside {
            let start = row + r.x0 as usize;
            for (o, s) in out.iter_mut().zip(&plane[start..start + r.w]) {
                *o = s.int();
            }
        } else {
            for (i, o) in out.iter_mut().enumerate() {
                let x = (r.x0 + i as i32).clamp(0, r.pic_w - 1) as usize;
                *o = plane[row + x].int();
            }
        }
    }
}

/// The six-tap filter over the samples at `x - 2` to `x + 3` of a row, or
/// of a column: `s[0]`, `s[step]` and on to `s[5 * step]`.
fn tap6(s: &[i32], step: usize) -> i32 {
    (s[2 * step] + s[3 * step]) * 20 - (s[step] + s[4 * step]) * 5 + (s[0] + s[5 * step])
}

/// A `w` by `h` luma block at quarter-sample position `xy` (`mx & 3` plus
/// four times `my & 3`) from the window `win`, `w + 5` wide with the block
/// two rows and two columns in, into `out`, `w` a row: what FFmpeg's
/// `put_h264_qpel*_mc*` write (`h264qpel_template.c`). FFmpeg covers a 16x8
/// or 8x16 partition with two square calls; every sample's value is the one
/// a single pass gives. Each half position is filtered once and the quarter
/// positions average two of them; the two-dimensional half position keeps
/// its first pass's sums as FFmpeg's `pixeltmp` does: 16 bits to 10-bit
/// samples (10 itself offset to fit), 32 deeper.
fn qpel(s: &mut Filters, w: usize, h: usize, xy: usize, bits: u32, out: &mut [i32]) {
    let ws = w + 5;
    // The window's sample (x, y) from the block's start.
    let at = |x: usize, y: usize| (y + 2) * ws + x + 2;
    let win = &s.win;
    let clip5 = |v: i32| clip((v + 16) >> 5, bits);
    // The horizontal half position at rows `y0..y0 + rows`, into `half_h`.
    let fill_h = |half: &mut [i32], y0: usize, rows: usize| {
        for y in 0..rows {
            for x in 0..w {
                half[y * w + x] = clip5(tap6(&win[at(x, y0 + y) - 2..], 1));
            }
        }
    };
    // The vertical half position at columns `x0..x0 + cols`, into
    // `half_v`, `cols` a row.
    let fill_v = |half: &mut [i32], x0: usize, cols: usize| {
        for y in 0..h {
            for x in 0..cols {
                half[y * cols + x] = clip5(tap6(&win[at(x0 + x, y) - 2 * ws..], ws));
            }
        }
    };
    // The two-dimensional half position, into `out`.
    let fill_hv = |tmp: &mut [i32], out: &mut [i32]| {
        let pad = if bits == 10 {
            -10 * ((1 << bits) - 1)
        } else {
            0
        };
        let narrow = bits <= 10;
        for r in 0..h + 5 {
            for x in 0..w {
                // Rows -2 to h + 2 of the block's columns.
                let t = tap6(&win[(r * ws) + x..], 1) + pad;
                tmp[r * w + x] = (if narrow { i32::from(t as i16) } else { t }) - pad;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let v = tap6(&tmp[y * w + x..], w);
                out[y * w + x] = clip((v + 512) >> 10, bits);
            }
        }
    };
    let g = |x: usize, y: usize| win[at(x, y)];
    let n = w * h;
    match xy {
        0 => {
            for y in 0..h {
                for x in 0..w {
                    out[y * w + x] = g(x, y);
                }
            }
        }
        1 | 3 => {
            fill_h(&mut s.half_h, 0, h);
            let dx = usize::from(xy == 3);
            for y in 0..h {
                for x in 0..w {
                    out[y * w + x] = avg2(g(x + dx, y), s.half_h[y * w + x]);
                }
            }
        }
        2 => fill_h(&mut out[..n], 0, h),
        4 | 12 => {
            fill_v(&mut s.half_v, 0, w);
            let dy = usize::from(xy == 12);
            for y in 0..h {
                for x in 0..w {
                    out[y * w + x] = avg2(g(x, y + dy), s.half_v[y * w + x]);
                }
            }
        }
        8 => fill_v(&mut out[..n], 0, w),
        10 => fill_hv(&mut s.tmp, &mut out[..n]),
        5 | 7 | 13 | 15 => {
            // Diagonal quarters: a horizontal half row (the next row's for
            // 13 and 15) averaged with a vertical half column (the next
            // column's for 7 and 15).
            let (dy, dx) = (usize::from(xy >= 13), usize::from(xy & 3 == 3));
            fill_h(&mut s.half_h, dy, h);
            fill_v(&mut s.half_v, dx, w);
            for i in 0..n {
                out[i] = avg2(s.half_h[i], s.half_v[i]);
            }
        }
        6 | 14 => {
            fill_h(&mut s.half_h, usize::from(xy == 14), h);
            fill_hv(&mut s.tmp, &mut out[..n]);
            for i in 0..n {
                out[i] = avg2(s.half_h[i], out[i]);
            }
        }
        _ => {
            // 9 and 11: the vertical half column (the next one's for 11)
            // averaged with the two-dimensional half position.
            fill_v(&mut s.half_v, usize::from(xy == 11), w);
            fill_hv(&mut s.tmp, &mut out[..n]);
            for i in 0..n {
                out[i] = avg2(s.half_v[i], out[i]);
            }
        }
    }
}

/// A `w` by `h` chroma block at eighth-sample position (`fx`, `fy`),
/// bilinear (`put_h264_chroma_mc*`), from the window `win`, `w + 1` wide,
/// into `out`, `w` a row.
fn chroma_block(win: &[i32], w: usize, h: usize, fx: i32, fy: i32, out: &mut [i32]) {
    let a = (8 - fx) * (8 - fy);
    let b = fx * (8 - fy);
    let c = (8 - fx) * fy;
    let d = fx * fy;
    let ws = w + 1;
    for y in 0..h {
        for x in 0..w {
            let i = y * ws + x;
            let s = a * win[i] + b * win[i + 1] + c * win[i + ws] + d * win[i + ws + 1];
            out[y * w + x] = (s + 32) >> 6;
        }
    }
}

/// A prediction written into the picture -- or, with `avg`, averaged with
/// what the first list's prediction left there.
fn store<P: Pixel>(s: &mut [P], at: usize, stride: usize, w: usize, block: &[i32], avg: bool) {
    for (k, &v) in block.iter().enumerate() {
        let i = at + (k / w) * stride + k % w;
        s[i] = P::from_int(if avg { avg2(s[i].int(), v) } else { v });
    }
}

/// One prediction's weighting (`weight_h264_pixels*`).
#[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
fn weight<P: Pixel>(
    s: &mut [P],
    at: usize,
    stride: usize,
    w: usize,
    h: usize,
    log2_denom: i32,
    weight: i32,
    offset: i32,
    bits: u32,
) {
    let mut offset = ((offset as u32) << (log2_denom as u32 + bits - 8)) as i32;
    if log2_denom != 0 {
        offset += 1 << (log2_denom - 1);
    }
    for y in 0..h {
        for x in 0..w {
            let i = at + y * stride + x;
            s[i] = P::from_int(clip((s[i].int() * weight + offset) >> log2_denom, bits));
        }
    }
}

/// Two predictions weighted together (`biweight_h264_pixels*`): `dst` the
/// first list's, `src` the second's.
#[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
fn biweight<P: Pixel>(
    dst: &mut [P],
    at: usize,
    stride: usize,
    src: &[i32],
    w: usize,
    h: usize,
    log2_denom: i32,
    weightd: i32,
    weights: i32,
    offset: i32,
    bits: u32,
) {
    let offset = (offset as u32) << (bits - 8);
    let offset = ((offset.wrapping_add(1) | 1) << log2_denom as u32) as i32;
    for y in 0..h {
        for x in 0..w {
            let i = at + y * stride + x;
            let v =
                (src[y * w + x] * weights + dst[i].int() * weightd + offset) >> (log2_denom + 1);
            dst[i] = P::from_int(clip(v, bits));
        }
    }
}

/// Where one partition's prediction goes, and how big it is.
#[derive(Clone, Copy)]
struct Part {
    /// Its first 4x4 block, `scan8`-indexed through `SCAN8`.
    n: usize,
    /// Luma width and height.
    w: usize,
    h: usize,
    /// Its place in the macroblock, in 4x4 blocks... as FFmpeg's
    /// `x_offset` and `y_offset` count: half-blocks of 8 luma samples.
    x_offset: usize,
    y_offset: usize,
}

/// The current macroblock's planes, and where the partition being
/// predicted starts in each; the strides are the slice's
/// (`mb_linesize`, `mb_uvlinesize`).
pub(crate) struct Dst<'a, P> {
    pub(crate) planes: [&'a mut [P]; 3],
    pub(crate) at: [usize; 3],
}

impl Context {
    /// The samples of reference `r`'s plane `p`: from the picture being
    /// decoded when the reference is its other field.
    fn ref_plane<'a, P: Pixel>(
        &'a self,
        r: Ref,
        p: usize,
        cur: &'a [P],
    ) -> Result<&'a [P], DecodeError> {
        let parent = r.parent.ok_or(DecodeError)?;
        if Some(parent) == self.cur_pic_ptr {
            return Ok(cur);
        }
        let frame = self.pics[parent].f.as_deref().ok_or(DecodeError)?;
        let fb = P::from_any(frame).ok_or(DecodeError)?;
        Ok(&fb.planes[p].data)
    }

    /// One list's prediction of a partition (`mc_dir_part`): luma, then
    /// chroma at the vector halved (4:2:0, 4:2:2 across) or whole (4:4:4).
    /// Into the planes, put or averaged -- or, with `keep`, for weighted
    /// bi-prediction, left in `scratch`'s kept blocks instead.
    #[allow(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        reason = "one function in FFmpeg, kept whole to read against it"
    )]
    fn mc_dir_part<P: Pixel>(
        &self,
        sl: &SliceContext,
        r: Ref,
        part: Part,
        list: usize,
        dst: &mut Dst<'_, P>,
        avg: bool,
        keep: bool,
        scratch: &mut McScratch,
    ) -> Result<(), DecodeError> {
        let sps = self.ps.sps.as_deref().ok_or(DecodeError)?;
        let bits = sps.bit_depth_luma as u32;
        let mb_field = sl.hdr.mb_field_decoding_flag != 0;
        let src_x_offset = (part.x_offset + 8 * sl.mb_x as usize) as i32;
        let src_y_offset = (part.y_offset + 8 * (sl.mb_y >> i32::from(mb_field)) as usize) as i32;
        let mv = sl.mv_cache[list][SCAN8[part.n]];
        let mx = i32::from(mv[0]) + src_x_offset * 8;
        let mut my = i32::from(mv[1]) + src_y_offset * 8;
        let luma_xy = ((mx & 3) + ((my & 3) << 2)) as usize;
        let full_mx = mx >> 2;
        let full_my = my >> 2;
        let pic_width = 16 * self.mb_width;
        let pic_height = (16 * self.mb_height) >> i32::from(mb_field);
        let linesize = sl.mb_linesize;
        let uvlinesize = sl.mb_uvlinesize;
        // A monochrome stream goes through FFmpeg's 4:2:0 path, its gray
        // chroma predicted like any other.
        let chroma_idc = self.chroma_format_idc.max(1);
        let (w, h) = (part.w, part.h);
        let McScratch {
            filters,
            blocks,
            kept,
        } = scratch;
        let out = if keep { kept } else { blocks };

        // Luma: the block and the filter's reach, two before and three
        // after.
        let luma_reach = |p: usize| Reach {
            base: r.offset[p],
            stride: linesize,
            x0: full_mx - 2,
            y0: full_my - 2,
            w: w + 5,
            h: h + 5,
            pic_w: pic_width,
            pic_h: pic_height,
        };
        let planes = if chroma_idc == 3 { 3 } else { 1 };
        for p in 0..planes {
            fetch(
                &mut filters.win,
                self.ref_plane(r, p, &*dst.planes[p])?,
                luma_reach(p),
            );
            qpel(filters, w, h, luma_xy, bits, &mut out[p]);
        }
        if chroma_idc == 3 {
            if !keep {
                for p in 0..3 {
                    store(dst.planes[p], dst.at[p], linesize, w, &out[p][..w * h], avg);
                }
            }
            return Ok(());
        }

        // Chroma: eighths of a chroma sample; 4:2:2 is full height.
        let ysh = 3 - u32::from(chroma_idc == 2);
        if chroma_idc == 1 && mb_field {
            // A field predicting from the other parity: chroma sits a
            // quarter of a chroma row off.
            my += 2 * ((sl.mb_y & 1) - (r.reference - 1));
        }
        let (cw, ch) = (w / 2, if chroma_idc == 1 { h / 2 } else { h });
        let fx = mx & 7;
        let fy = ((my as u32) << u32::from(chroma_idc == 2)) as i32 & 7;
        for p in 1..3 {
            let reach = Reach {
                base: r.offset[p],
                stride: uvlinesize,
                x0: mx >> 3,
                y0: my >> ysh,
                w: cw + 1,
                h: ch + 1,
                pic_w: pic_width >> 1,
                pic_h: pic_height >> u32::from(chroma_idc == 1),
            };
            fetch(
                &mut filters.win,
                self.ref_plane(r, p, &*dst.planes[p])?,
                reach,
            );
            chroma_block(&filters.win, cw, ch, fx, fy, &mut out[p]);
        }
        if !keep {
            store(dst.planes[0], dst.at[0], linesize, w, &out[0][..w * h], avg);
            for p in 1..3 {
                store(
                    dst.planes[p],
                    dst.at[p],
                    uvlinesize,
                    cw,
                    &out[p][..cw * ch],
                    avg,
                );
            }
        }
        Ok(())
    }

    /// One partition, weighted or not (`mc_part`, `mc_part_std`,
    /// `mc_part_weighted`).
    #[allow(
        clippy::too_many_lines,
        reason = "two functions in FFmpeg, kept together"
    )]
    fn mc_part<P: Pixel>(
        &self,
        sl: &SliceContext,
        part: Part,
        mb_at: [usize; 3],
        dst: &mut Dst<'_, P>,
        scratch: &mut McScratch,
        list0: bool,
        list1: bool,
    ) -> Result<(), DecodeError> {
        let sps = self.ps.sps.as_deref().ok_or(DecodeError)?;
        let bits = sps.bit_depth_luma as u32;
        let chroma_idc = self.chroma_format_idc.max(1);
        let (linesize, uvlinesize) = (sl.mb_linesize, sl.mb_uvlinesize);
        // The partition's place in each plane.
        let (cx, cy) = match chroma_idc {
            3 => (2 * part.x_offset, 2 * part.y_offset),
            2 => (part.x_offset, 2 * part.y_offset),
            _ => (part.x_offset, part.y_offset),
        };
        let c_stride = if chroma_idc == 3 {
            linesize
        } else {
            uvlinesize
        };
        dst.at = [
            mb_at[0] + 2 * part.x_offset + 2 * part.y_offset * linesize,
            mb_at[1] + cx + cy * c_stride,
            mb_at[2] + cx + cy * c_stride,
        ];
        let pwt = &sl.hdr.pwt;
        let refn = |list: usize| sl.ref_cache[list][SCAN8[part.n]];
        let entry = |list: usize| -> Result<Ref, DecodeError> {
            let i = usize::try_from(refn(list)).map_err(|_| DecodeError)?;
            sl.ref_list[list].get(i).copied().ok_or(DecodeError)
        };
        let implicit = |r0: i8, r1: i8| -> Result<i32, DecodeError> {
            let (a, b) = (
                usize::try_from(r0).map_err(|_| DecodeError)?,
                usize::try_from(r1).map_err(|_| DecodeError)?,
            );
            Ok(pwt
                .implicit_weight
                .get(a)
                .and_then(|row| row.get(b))
                .ok_or(DecodeError)?[(sl.mb_y & 1) as usize])
        };
        let weighted = pwt.use_weight == 1
            || (pwt.use_weight == 2 && list0 && list1 && implicit(refn(0), refn(1))? != 32);

        if !weighted {
            let mut avg = false;
            for (list, used) in [(0, list0), (1, list1)] {
                if used {
                    self.mc_dir_part(sl, entry(list)?, part, list, dst, avg, false, scratch)?;
                    avg = true;
                }
            }
            return Ok(());
        }

        let (cw, ch) = match chroma_idc {
            3 => (part.w, part.h),
            2 => (part.w / 2, part.h),
            _ => (part.w / 2, part.h / 2),
        };
        if list0 && list1 {
            let r0 = refn(0);
            let r1 = refn(1);
            self.mc_dir_part(sl, entry(0)?, part, 0, dst, false, false, scratch)?;
            self.mc_dir_part(sl, entry(1)?, part, 1, dst, false, true, scratch)?;
            let [ty, tcb, tcr] = &scratch.kept;
            let (i0, i1) = (
                usize::try_from(r0).map_err(|_| DecodeError)?,
                usize::try_from(r1).map_err(|_| DecodeError)?,
            );
            if pwt.use_weight == 2 {
                let w0 = implicit(r0, r1)?;
                let w1 = 64 - w0;
                biweight(
                    dst.planes[0],
                    dst.at[0],
                    linesize,
                    ty,
                    part.w,
                    part.h,
                    5,
                    w0,
                    w1,
                    0,
                    bits,
                );
                biweight(
                    dst.planes[1],
                    dst.at[1],
                    c_stride,
                    tcb,
                    cw,
                    ch,
                    5,
                    w0,
                    w1,
                    0,
                    bits,
                );
                biweight(
                    dst.planes[2],
                    dst.at[2],
                    c_stride,
                    tcr,
                    cw,
                    ch,
                    5,
                    w0,
                    w1,
                    0,
                    bits,
                );
            } else {
                let lw = |r: usize, list: usize| {
                    pwt.luma_weight.get(r).map(|x| x[list]).ok_or(DecodeError)
                };
                let cwt = |r: usize, list: usize| {
                    pwt.chroma_weight.get(r).map(|x| x[list]).ok_or(DecodeError)
                };
                let (l0, l1) = (lw(i0, 0)?, lw(i1, 1)?);
                biweight(
                    dst.planes[0],
                    dst.at[0],
                    linesize,
                    ty,
                    part.w,
                    part.h,
                    pwt.luma_log2_weight_denom,
                    l0[0],
                    l1[0],
                    l0[1] + l1[1],
                    bits,
                );
                let (c0, c1) = (cwt(i0, 0)?, cwt(i1, 1)?);
                let d = pwt.chroma_log2_weight_denom;
                biweight(
                    dst.planes[1],
                    dst.at[1],
                    c_stride,
                    tcb,
                    cw,
                    ch,
                    d,
                    c0[0][0],
                    c1[0][0],
                    c0[0][1] + c1[0][1],
                    bits,
                );
                biweight(
                    dst.planes[2],
                    dst.at[2],
                    c_stride,
                    tcr,
                    cw,
                    ch,
                    d,
                    c0[1][0],
                    c1[1][0],
                    c0[1][1] + c1[1][1],
                    bits,
                );
            }
            return Ok(());
        }

        let list = usize::from(list1);
        let i = usize::try_from(refn(list)).map_err(|_| DecodeError)?;
        self.mc_dir_part(sl, entry(list)?, part, list, dst, false, false, scratch)?;
        let lw = pwt.luma_weight.get(i).map(|x| x[list]).ok_or(DecodeError)?;
        weight(
            dst.planes[0],
            dst.at[0],
            linesize,
            part.w,
            part.h,
            pwt.luma_log2_weight_denom,
            lw[0],
            lw[1],
            bits,
        );
        if pwt.use_weight_chroma != 0 {
            let c = pwt
                .chroma_weight
                .get(i)
                .map(|x| x[list])
                .ok_or(DecodeError)?;
            let d = pwt.chroma_log2_weight_denom;
            weight(
                dst.planes[1],
                dst.at[1],
                c_stride,
                cw,
                ch,
                d,
                c[0][0],
                c[0][1],
                bits,
            );
            weight(
                dst.planes[2],
                dst.at[2],
                c_stride,
                cw,
                ch,
                d,
                c[1][0],
                c[1][1],
                bits,
            );
        }
        Ok(())
    }

    /// An inter macroblock's prediction, partition by partition
    /// (`hl_motion`).
    #[allow(
        clippy::too_many_arguments,
        reason = "the macroblock's planes and places"
    )]
    pub(crate) fn hl_motion<P: Pixel>(
        &self,
        sl: &mut SliceContext,
        mb_type: u32,
        planes: [&mut [P]; 3],
        dest_y: usize,
        dest_c: [usize; 2],
    ) -> Result<(), DecodeError> {
        // The slice's scratch, made once and lent to each macroblock.
        let mut scratch = sl.mc_scratch.take().unwrap_or_default();
        let result = self.hl_motion_parts(&*sl, &mut scratch, mb_type, planes, dest_y, dest_c);
        sl.mc_scratch = Some(scratch);
        result
    }

    /// [`Self::hl_motion`], with the scratch to predict in.
    fn hl_motion_parts<P: Pixel>(
        &self,
        sl: &SliceContext,
        scratch: &mut McScratch,
        mb_type: u32,
        planes: [&mut [P]; 3],
        dest_y: usize,
        dest_c: [usize; 2],
    ) -> Result<(), DecodeError> {
        // Under H264_MV the line `tools/ffdbg`'s FFDBG_MV prints in FFmpeg,
        // for `tools/h264cmp.py`.
        #[cfg(test)]
        if crate::probe::PROBES.mv {
            let mut line = format!(
                "MV poc={} mb={},{} type={:x} sub={:x},{:x},{:x},{:x}",
                self.cur_pic_ptr.map_or(-1, |c| self.pics[c].poc),
                sl.mb_x,
                sl.mb_y,
                mb_type,
                sl.sub_mb_type[0],
                sl.sub_mb_type[1],
                sl.sub_mb_type[2],
                sl.sub_mb_type[3]
            );
            for list in 0..sl.hdr.list_count as usize {
                line.push_str(&format!(" L{list}"));
                for &s in &SCAN8[..16] {
                    let mv = sl.mv_cache[list][s];
                    line.push_str(&format!(" {}:{},{}", sl.ref_cache[list][s], mv[0], mv[1]));
                }
            }
            eprintln!("{line}");
        }
        let mut dst = Dst { planes, at: [0; 3] };
        let mb_at = [dest_y, dest_c[0], dest_c[1]];
        let dir = |t: u32, part: u32, list: u32| is_dir(t, part, list);
        let part = |n, w, h, x_offset, y_offset| Part {
            n,
            w,
            h,
            x_offset,
            y_offset,
        };
        if is_16x16(mb_type) {
            self.mc_part(
                sl,
                part(0, 16, 16, 0, 0),
                mb_at,
                &mut dst,
                scratch,
                dir(mb_type, 0, 0),
                dir(mb_type, 0, 1),
            )?;
        } else if is_16x8(mb_type) {
            self.mc_part(
                sl,
                part(0, 16, 8, 0, 0),
                mb_at,
                &mut dst,
                scratch,
                dir(mb_type, 0, 0),
                dir(mb_type, 0, 1),
            )?;
            self.mc_part(
                sl,
                part(8, 16, 8, 0, 4),
                mb_at,
                &mut dst,
                scratch,
                dir(mb_type, 1, 0),
                dir(mb_type, 1, 1),
            )?;
        } else if is_8x16(mb_type) {
            self.mc_part(
                sl,
                part(0, 8, 16, 0, 0),
                mb_at,
                &mut dst,
                scratch,
                dir(mb_type, 0, 0),
                dir(mb_type, 0, 1),
            )?;
            self.mc_part(
                sl,
                part(4, 8, 16, 4, 0),
                mb_at,
                &mut dst,
                scratch,
                dir(mb_type, 1, 0),
                dir(mb_type, 1, 1),
            )?;
        } else {
            for i in 0..4 {
                let sub = u32::from(sl.sub_mb_type[i]);
                let n = 4 * i;
                let x_offset = (i & 1) << 2;
                let y_offset = (i & 2) << 1;
                let (l0, l1) = (dir(sub, 0, 0), dir(sub, 0, 1));
                if is_sub_8x8(sub) {
                    self.mc_part(
                        sl,
                        part(n, 8, 8, x_offset, y_offset),
                        mb_at,
                        &mut dst,
                        scratch,
                        l0,
                        l1,
                    )?;
                } else if is_sub_8x4(sub) {
                    self.mc_part(
                        sl,
                        part(n, 8, 4, x_offset, y_offset),
                        mb_at,
                        &mut dst,
                        scratch,
                        l0,
                        l1,
                    )?;
                    self.mc_part(
                        sl,
                        part(n + 2, 8, 4, x_offset, y_offset + 2),
                        mb_at,
                        &mut dst,
                        scratch,
                        l0,
                        l1,
                    )?;
                } else if is_sub_4x8(sub) {
                    self.mc_part(
                        sl,
                        part(n, 4, 8, x_offset, y_offset),
                        mb_at,
                        &mut dst,
                        scratch,
                        l0,
                        l1,
                    )?;
                    self.mc_part(
                        sl,
                        part(n + 1, 4, 8, x_offset + 2, y_offset),
                        mb_at,
                        &mut dst,
                        scratch,
                        l0,
                        l1,
                    )?;
                } else {
                    for j in 0..4 {
                        let (sx, sy) = (x_offset + 2 * (j & 1), y_offset + (j & 2));
                        self.mc_part(
                            sl,
                            part(n + j, 4, 4, sx, sy),
                            mb_at,
                            &mut dst,
                            scratch,
                            l0,
                            l1,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}
