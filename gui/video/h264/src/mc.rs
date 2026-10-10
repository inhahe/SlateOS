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

/// A reference window: the samples a filter reads around a block, edges
/// repeated where they reach past the picture.
struct Window {
    s: Vec<i32>,
    stride: usize,
    /// How far into the window the block starts.
    origin: usize,
}

impl Window {
    /// `w` by `h` samples from (`x0`, `y0`) of a picture `pic_w` by `pic_h`
    /// whose sample (0, 0) is `base` in `plane`, rows `stride` apart.
    #[allow(clippy::too_many_arguments, reason = "emulated_edge_mc's")]
    fn fetch<P: Pixel>(
        plane: &[P],
        base: usize,
        stride: usize,
        x0: i32,
        y0: i32,
        w: usize,
        h: usize,
        pic_w: i32,
        pic_h: i32,
        origin: usize,
    ) -> Self {
        let mut s = Vec::with_capacity(w * h);
        for j in 0..h as i32 {
            let y = (y0 + j).clamp(0, pic_h - 1) as usize;
            let row = base + y * stride;
            for i in 0..w as i32 {
                let x = (x0 + i).clamp(0, pic_w - 1) as usize;
                s.push(plane[row + x].int());
            }
        }
        Self {
            s,
            stride: w,
            origin,
        }
    }

    /// The sample at (`x`, `y`) from the block's start.
    fn g(&self, x: i32, y: i32) -> i32 {
        self.s[(self.origin as i32 + x + y * self.stride as i32) as usize]
    }
}

/// The six-tap filter over a row's samples at `x - 2` to `x + 3`.
fn tap(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32) -> i32 {
    (c + d) * 20 - (b + e) * 5 + (a + f)
}

/// A `w` by `h` luma block at quarter-sample position `xy` (`mx & 3` plus
/// four times `my & 3`): what FFmpeg's `put_h264_qpel*_mc*` write
/// (`h264qpel_template.c`). FFmpeg covers a 16x8 or 8x16 partition with
/// two square calls; every sample's value is the one a single pass gives. The two-dimensional half position keeps its
/// intermediate sums as FFmpeg's `pixeltmp` does: 16 bits to 10-bit
/// samples (10 itself offset to fit), 32 deeper.
fn qpel(win: &Window, w: usize, h: usize, xy: usize, bits: u32) -> Vec<i32> {
    let g = |x: i32, y: i32| win.g(x, y);
    let hf = |x: i32, y: i32| {
        tap(
            g(x - 2, y),
            g(x - 1, y),
            g(x, y),
            g(x + 1, y),
            g(x + 2, y),
            g(x + 3, y),
        )
    };
    let v = |x: i32, y: i32| {
        tap(
            g(x, y - 2),
            g(x, y - 1),
            g(x, y),
            g(x, y + 1),
            g(x, y + 2),
            g(x, y + 3),
        )
    };
    let put_h = |x: i32, y: i32| clip((hf(x, y) + 16) >> 5, bits);
    let put_v = |x: i32, y: i32| clip((v(x, y) + 16) >> 5, bits);
    let pad = if bits == 10 {
        -10 * ((1 << bits) - 1)
    } else {
        0
    };
    let narrow = bits <= 10;
    let tmp = |x: i32, y: i32| {
        let t = hf(x, y) + pad;
        (if narrow { i32::from(t as i16) } else { t }) - pad
    };
    let put_hv = |x: i32, y: i32| {
        clip(
            (tap(
                tmp(x, y - 2),
                tmp(x, y - 1),
                tmp(x, y),
                tmp(x, y + 1),
                tmp(x, y + 2),
                tmp(x, y + 3),
            ) + 512)
                >> 10,
            bits,
        )
    };
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            out.push(match xy {
                0 => g(x, y),
                1 => avg2(g(x, y), put_h(x, y)),
                2 => put_h(x, y),
                3 => avg2(g(x + 1, y), put_h(x, y)),
                4 => avg2(g(x, y), put_v(x, y)),
                5 => avg2(put_h(x, y), put_v(x, y)),
                6 => avg2(put_h(x, y), put_hv(x, y)),
                7 => avg2(put_h(x, y), put_v(x + 1, y)),
                8 => put_v(x, y),
                9 => avg2(put_v(x, y), put_hv(x, y)),
                10 => put_hv(x, y),
                11 => avg2(put_v(x + 1, y), put_hv(x, y)),
                12 => avg2(g(x, y + 1), put_v(x, y)),
                13 => avg2(put_h(x, y + 1), put_v(x, y)),
                14 => avg2(put_h(x, y + 1), put_hv(x, y)),
                _ => avg2(put_h(x, y + 1), put_v(x + 1, y)),
            });
        }
    }
    out
}

/// A `w` by `h` chroma block at eighth-sample position (`fx`, `fy`),
/// bilinear (`put_h264_chroma_mc*`).
fn chroma_block(win: &Window, w: usize, h: usize, fx: i32, fy: i32) -> Vec<i32> {
    let a = (8 - fx) * (8 - fy);
    let b = fx * (8 - fy);
    let c = (8 - fx) * fy;
    let d = fx * fy;
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let s = a * win.g(x, y)
                + b * win.g(x + 1, y)
                + c * win.g(x, y + 1)
                + d * win.g(x + 1, y + 1);
            out.push((s + 32) >> 6);
        }
    }
    out
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
    /// `into` the planes, put or averaged, or -- for weighted bi-
    /// prediction -- `None`, its blocks handed back instead.
    #[allow(
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
    ) -> Result<Option<[Vec<i32>; 3]>, DecodeError> {
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

        // Luma: the block and the filter's reach, two before and three
        // after.
        let luma_block = |plane: &[P], p: usize| -> Vec<i32> {
            let win = Window::fetch(
                plane,
                r.offset[p],
                linesize,
                full_mx - 2,
                full_my - 2,
                w + 5,
                h + 5,
                pic_width,
                pic_height,
                2 * (w + 5) + 2,
            );
            qpel(&win, w, h, luma_xy, bits)
        };

        let y_block = luma_block(self.ref_plane(r, 0, &*dst.planes[0])?, 0);
        if chroma_idc == 3 {
            let cb = luma_block(self.ref_plane(r, 1, &*dst.planes[1])?, 1);
            let cr = luma_block(self.ref_plane(r, 2, &*dst.planes[2])?, 2);
            if keep {
                return Ok(Some([y_block, cb, cr]));
            }
            store(dst.planes[0], dst.at[0], linesize, w, &y_block, avg);
            store(dst.planes[1], dst.at[1], linesize, w, &cb, avg);
            store(dst.planes[2], dst.at[2], linesize, w, &cr, avg);
            return Ok(None);
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
        let (cx0, cy0) = (mx >> 3, my >> ysh);
        let c_w = pic_width >> 1;
        let c_h = pic_height >> u32::from(chroma_idc == 1);
        let chroma = |plane: &[P], p: usize| {
            let win = Window::fetch(
                plane,
                r.offset[p],
                uvlinesize,
                cx0,
                cy0,
                cw + 1,
                ch + 1,
                c_w,
                c_h,
                0,
            );
            chroma_block(&win, cw, ch, fx, fy)
        };
        let cb = chroma(self.ref_plane(r, 1, &*dst.planes[1])?, 1);
        let cr = chroma(self.ref_plane(r, 2, &*dst.planes[2])?, 2);
        if keep {
            return Ok(Some([y_block, cb, cr]));
        }
        store(dst.planes[0], dst.at[0], linesize, w, &y_block, avg);
        store(dst.planes[1], dst.at[1], uvlinesize, cw, &cb, avg);
        store(dst.planes[2], dst.at[2], uvlinesize, cw, &cr, avg);
        Ok(None)
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
                    self.mc_dir_part(sl, entry(list)?, part, list, dst, avg, false)?;
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
            self.mc_dir_part(sl, entry(0)?, part, 0, dst, false, false)?;
            let Some([ty, tcb, tcr]) =
                self.mc_dir_part(sl, entry(1)?, part, 1, dst, false, true)?
            else {
                return Err(DecodeError);
            };
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
                    &ty,
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
                    &tcb,
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
                    &tcr,
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
                    &ty,
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
                    &tcb,
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
                    &tcr,
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
        self.mc_dir_part(sl, entry(list)?, part, list, dst, false, false)?;
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
        let sl = &*sl;
        // Under H264_MV the line `tools/ffdbg`'s FFDBG_MV prints in FFmpeg,
        // for `tools/h264cmp.py`.
        #[cfg(test)]
        if std::env::var_os("H264_MV").is_some() {
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
                dir(mb_type, 0, 0),
                dir(mb_type, 0, 1),
            )?;
        } else if is_16x8(mb_type) {
            self.mc_part(
                sl,
                part(0, 16, 8, 0, 0),
                mb_at,
                &mut dst,
                dir(mb_type, 0, 0),
                dir(mb_type, 0, 1),
            )?;
            self.mc_part(
                sl,
                part(8, 16, 8, 0, 4),
                mb_at,
                &mut dst,
                dir(mb_type, 1, 0),
                dir(mb_type, 1, 1),
            )?;
        } else if is_8x16(mb_type) {
            self.mc_part(
                sl,
                part(0, 8, 16, 0, 0),
                mb_at,
                &mut dst,
                dir(mb_type, 0, 0),
                dir(mb_type, 0, 1),
            )?;
            self.mc_part(
                sl,
                part(4, 8, 16, 4, 0),
                mb_at,
                &mut dst,
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
                        l0,
                        l1,
                    )?;
                } else if is_sub_8x4(sub) {
                    self.mc_part(
                        sl,
                        part(n, 8, 4, x_offset, y_offset),
                        mb_at,
                        &mut dst,
                        l0,
                        l1,
                    )?;
                    self.mc_part(
                        sl,
                        part(n + 2, 8, 4, x_offset, y_offset + 2),
                        mb_at,
                        &mut dst,
                        l0,
                        l1,
                    )?;
                } else if is_sub_4x8(sub) {
                    self.mc_part(
                        sl,
                        part(n, 4, 8, x_offset, y_offset),
                        mb_at,
                        &mut dst,
                        l0,
                        l1,
                    )?;
                    self.mc_part(
                        sl,
                        part(n + 1, 4, 8, x_offset + 2, y_offset),
                        mb_at,
                        &mut dst,
                        l0,
                        l1,
                    )?;
                } else {
                    for j in 0..4 {
                        let (sx, sy) = (x_offset + 2 * (j & 1), y_offset + (j & 2));
                        self.mc_part(sl, part(n + j, 4, 4, sx, sy), mb_at, &mut dst, l0, l1)?;
                    }
                }
            }
        }
        Ok(())
    }
}
