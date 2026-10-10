//! A decoded macroblock made samples (`h264_mb.c`, `h264_mb_template.c`):
//! an I_PCM macroblock's samples copied; an intra one predicted from its
//! neighbours block by block, each block's residual added before the next
//! is predicted; an inter one predicted from its references (`mc`) and its
//! residual added; chroma's DC transforms and residual after.
//!
//! FFmpeg has a "simple" and a "complex" build of this code for speed; they
//! differ only in what they leave out, and this is the complex one.
//!
//! Intra prediction must see its neighbours as decoded, not as the loop
//! filter later leaves them: the filter runs a row behind, so the samples
//! above have been filtered by then. Before an intra macroblock is
//! predicted the row above is swapped for the unfiltered copy kept when it
//! was filtered (`backup_mb_border` in `slicedata`), and swapped back after
//! (`xchg_mb_border`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "sample offsets within the picture, and FFmpeg's int arithmetic on them"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "sample indices are the macroblock's and its neighbours' inside the picture; \
              block indices are under 48; cache indices are scan8's"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int conversions, made where it makes them"
)]

use crate::bits::Bits;
use crate::decoder::{Context, DecodeError, SliceContext};
use crate::idct::{
    Depth, chroma_dc_dequant_idct, chroma422_dc_dequant_idct, idct_add, idct_add8, idct_add8_422,
    idct_add16, idct_add16intra, idct_dc_add, idct8_add, idct8_add4, idct8_dc_add,
    luma_dc_dequant_idct,
};
use crate::mbtype::{
    CHROMA_DC_BLOCK_INDEX, LUMA_DC_BLOCK_INDEX, SCAN8, is_8x8dct, is_16x16, is_intra, is_intra_pcm,
    is_intra4x4, is_intra16x16, uses_list,
};
use crate::mvpred::{fill, tab};
use crate::picture::{AnyFrame, FrameBuf, Pixel, table_offset};
use crate::pred::{
    DIAG_DOWN_LEFT_PRED, HOR_PRED8X8, VERT_LEFT_PRED, VERT_PRED8X8, pred_blocks_add, pred_chroma,
    pred4x4, pred4x4_add, pred8x8l, pred8x8l_add, pred8x8l_filter_add, pred16x16,
};

/// `ff_h264_mb_sizes`: a macroblock's samples by chroma format.
const MB_SIZES: [usize; 4] = [256, 384, 512, 768];

/// `ff_h264_add_pixels4` and `_8`: lossless residual added, wrapping as
/// the sample type does; the block left zero.
fn add_pixels<P: Pixel>(n: usize, s: &mut [P], at: usize, stride: usize, block: &mut [i32]) {
    for y in 0..n {
        for x in 0..n {
            let i = at + x + y * stride;
            s[i] = P::from_int(s[i].int().wrapping_add(block[x + n * y]));
        }
    }
    block[..n * n].fill(0);
}

/// Where a macroblock's samples start in each plane, its row strides and
/// which half of `block_offset` its blocks read.
#[derive(Clone, Copy)]
struct Dest {
    y: usize,
    cb: usize,
    cr: usize,
    linesize: usize,
    uvlinesize: usize,
    /// 0 for a frame macroblock, 48 for a field one.
    block_offset: usize,
}

impl Context {
    /// `ff_h264_hl_decode_mb`: the macroblock `sl` was left at, made
    /// samples in `frame`. `nal` is the slice's data, which an I_PCM
    /// macroblock's samples are read from.
    pub(crate) fn hl_decode_mb(
        &self,
        sl: &mut SliceContext,
        frame: &mut AnyFrame,
        nal: &[u8],
    ) -> Result<(), DecodeError> {
        let r = match frame {
            AnyFrame::Eight(f) => self.hl_decode_mb_t(sl, f, nal),
            AnyFrame::High(f) => self.hl_decode_mb_t(sl, f, nal),
        };
        #[cfg(test)]
        if r.is_ok() && std::env::var_os("H264_MB").is_some() {
            self.debug_mb_sums(sl, frame);
        }
        r
    }

    /// Under H264_MB, the line `tools/ffdbg`'s FFDBG_MB prints in FFmpeg,
    /// for `tools/h264cmp.py`: the macroblock's type, cbp, qp and an
    /// Adler-32 of its samples in each plane, as bytes, before the loop
    /// filter.
    #[cfg(test)]
    fn debug_mb_sums(&self, sl: &SliceContext, frame: &AnyFrame) {
        fn sum<P: Pixel>(
            plane: &[P],
            at: usize,
            stride: usize,
            rows: usize,
            cols: usize,
            wide: bool,
        ) -> u32 {
            let (mut a, mut b) = (1u32, 0u32);
            for y in 0..rows {
                for x in 0..cols {
                    let v = plane[at + y * stride + x].int() as u32;
                    let bytes: &[u32] = if wide { &[v & 0xFF, v >> 8] } else { &[v] };
                    for &byte in bytes {
                        a = (a + byte) % 65521;
                        b = (b + a) % 65521;
                    }
                }
            }
            (b << 16) | a
        }
        fn sums<P: Pixel>(
            h: &Context,
            sl: &SliceContext,
            fb: &FrameBuf<P>,
            wide: bool,
        ) -> [u32; 3] {
            let (mb_x, mb_y) = (sl.mb_x as usize, sl.mb_y as usize);
            let c444 = h.chroma_format_idc == 3;
            let block_h = 16 >> h.chroma_y_shift;
            let (mut ls, mut uvls) = (sl.linesize, sl.uvlinesize);
            let mut d = [mb_x * 16 + mb_y * ls * 16, 0, 0];
            let (rows, cols) = if c444 {
                d[1] = d[0];
                ([16, 16, 16], [16, 16, 16])
            } else {
                d[1] = mb_x * 8 + mb_y * uvls * block_h;
                ([16, block_h, block_h], [16, 8, 8])
            };
            d[2] = d[1];
            if sl.hdr.mb_field_decoding_flag != 0 {
                if mb_y & 1 != 0 {
                    d[0] -= ls * 15;
                    let c = if c444 { ls * 15 } else { uvls * (block_h - 1) };
                    d[1] -= c;
                    d[2] -= c;
                }
                ls *= 2;
                uvls *= 2;
            }
            std::array::from_fn(|p| {
                let stride = if p > 0 && !c444 { uvls } else { ls };
                sum(&fb.planes[p].data, d[p], stride, rows[p], cols[p], wide)
            })
        }
        let s = match frame {
            AnyFrame::Eight(f) => sums(self, sl, f, false),
            AnyFrame::High(f) => sums(self, sl, f, true),
        };
        let mb_type = self.cur_pic_ptr.map_or(0, |c| {
            let pic = &self.pics[c];
            pic.mb_type[tab(table_offset(pic.mb_stride), sl.mb_xy)]
        });
        eprintln!(
            "MB poc={} mb={},{} type={:x} cbp={:x} qp={} sum={:08x},{:08x},{:08x}",
            self.cur_pic_ptr.map_or(-1, |c| self.pics[c].poc),
            sl.mb_x,
            sl.mb_y,
            mb_type,
            sl.cbp,
            sl.hdr.qscale,
            s[0],
            s[1],
            s[2]
        );
    }

    fn hl_decode_mb_t<P: Pixel>(
        &self,
        sl: &mut SliceContext,
        fb: &mut FrameBuf<P>,
        nal: &[u8],
    ) -> Result<(), DecodeError> {
        let mb_x = sl.mb_x as usize;
        let mb_y = sl.mb_y as usize;
        let mb_xy = sl.mb_xy;
        let cur = self.cur_pic_ptr.ok_or(DecodeError)?;
        let pic = &self.pics[cur];
        let mb_type = pic.mb_type[tab(table_offset(pic.mb_stride), mb_xy)];
        let chroma444 = self.chroma_format_idc == 3;
        let block_h = 16 >> self.chroma_y_shift;
        let chroma_w = if chroma444 { 16 } else { 8 };

        let mut d = Dest {
            y: mb_x * 16 + mb_y * sl.linesize * 16,
            cb: mb_x * chroma_w + mb_y * sl.uvlinesize * block_h,
            cr: 0,
            linesize: sl.linesize,
            uvlinesize: sl.uvlinesize,
            block_offset: 0,
        };
        if chroma444 {
            d.cb = mb_x * 16 + mb_y * sl.linesize * 16;
        }
        d.cr = d.cb;

        if sl.hdr.mb_field_decoding_flag != 0 {
            d.linesize = sl.linesize * 2;
            d.uvlinesize = sl.uvlinesize * 2;
            d.block_offset = 48;
            if mb_y & 1 != 0 {
                d.y -= sl.linesize * 15;
                let c = if chroma444 {
                    sl.linesize * 15
                } else {
                    sl.uvlinesize * (block_h - 1)
                };
                d.cb -= c;
                d.cr -= c;
            }
            if self.mb_aff_frame {
                // A field macroblock of an MBAFF frame predicts from the
                // field entries of the lists: 16 on, the parity its own.
                for list in 0..sl.hdr.list_count as usize {
                    if !uses_list(mb_type, list as u32) {
                        continue;
                    }
                    let parity = (mb_y & 1) as i8;
                    if is_16x16(mb_type) {
                        let r = sl.ref_cache[list][SCAN8[0]];
                        fill(&mut sl.ref_cache[list], SCAN8[0], 4, 4, (16 + r) ^ parity);
                    } else {
                        for i in (0..16).step_by(4) {
                            let r = sl.ref_cache[list][SCAN8[i]];
                            if r >= 0 {
                                fill(&mut sl.ref_cache[list], SCAN8[i], 2, 2, (16 + r) ^ parity);
                            }
                        }
                    }
                }
            }
        }
        if chroma444 {
            d.uvlinesize = d.linesize;
        }
        sl.mb_linesize = d.linesize;
        sl.mb_uvlinesize = d.uvlinesize;

        let sps = self.ps.sps.as_deref().ok_or(DecodeError)?;
        let depth = Depth {
            bits: sps.bit_depth_luma as u32,
        };
        let [py, pcb, pcr] = &mut fb.planes;
        let (y, cb, cr) = (&mut py.data[..], &mut pcb.data[..], &mut pcr.data[..]);

        if is_intra_pcm(mb_type) {
            self.intra_pcm(sl, [y, cb, cr], d, nal)?;
            return Ok(());
        }

        let transform_bypass = sl.hdr.qscale == 0 && sps.transform_bypass != 0;
        if is_intra(mb_type) {
            if sl.hdr.deblocking_filter != 0 {
                self.xchg_mb_border(sl, [&mut *y, &mut *cb, &mut *cr], d, true);
            }
            if chroma444 {
                for (p, plane, at) in [(0, &mut *y, d.y), (1, &mut *cb, d.cb), (2, &mut *cr, d.cr)]
                {
                    self.predict_luma(sl, mb_type, transform_bypass, d, plane, at, p, depth)?;
                }
            } else {
                // A monochrome stream's chroma is predicted too: its mode
                // is DC 128, which keeps its planes gray.
                let tall = self.chroma_format_idc == 2;
                pred_chroma(
                    sl.chroma_pred_mode,
                    cb,
                    d.cb,
                    d.uvlinesize,
                    tall,
                    depth.bits,
                );
                pred_chroma(
                    sl.chroma_pred_mode,
                    cr,
                    d.cr,
                    d.uvlinesize,
                    tall,
                    depth.bits,
                );
                self.predict_luma(sl, mb_type, transform_bypass, d, y, d.y, 0, depth)?;
            }
            if sl.hdr.deblocking_filter != 0 {
                self.xchg_mb_border(sl, [&mut *y, &mut *cb, &mut *cr], d, false);
            }
        } else {
            self.hl_motion(
                sl,
                mb_type,
                [&mut *y, &mut *cb, &mut *cr],
                d.y,
                [d.cb, d.cr],
            )?;
        }

        if chroma444 {
            for (p, plane, at) in [(0, &mut *y, d.y), (1, &mut *cb, d.cb), (2, &mut *cr, d.cr)] {
                self.idct_luma(sl, mb_type, transform_bypass, d, plane, at, p, depth);
            }
            return Ok(());
        }
        self.idct_luma(sl, mb_type, transform_bypass, d, y, d.y, 0, depth);

        if sl.cbp & 0x30 == 0 {
            return Ok(());
        }
        let chroma422 = self.chroma_format_idc == 2;
        let offsets = &self.block_offset[d.block_offset..];
        if transform_bypass {
            if is_intra(mb_type)
                && sps.profile_idc == 244
                && (sl.chroma_pred_mode == VERT_PRED8X8 || sl.chroma_pred_mode == HOR_PRED8X8)
            {
                let vertical = sl.chroma_pred_mode == VERT_PRED8X8;
                let n = if chroma422 { 8 } else { 4 };
                // `pred8x8_add` and `pred8x16_add`: offsets from 16 and 32,
                // the 4:2:2 lower half's four on from its coefficients.
                for (j, plane, at) in [(1usize, &mut *cb, d.cb), (2, &mut *cr, d.cr)] {
                    let base = 16 * j;
                    let blocks = (0..n).map(|i| (base + if i < 4 { i } else { i + 4 }, i));
                    let coefficients = &mut sl.mb[16 * 16 * j..];
                    pred_blocks_add(
                        vertical,
                        plane,
                        at,
                        offsets,
                        blocks,
                        coefficients,
                        d.uvlinesize,
                    );
                }
            } else {
                for (j, plane, at) in [(1usize, &mut *cb, d.cb), (2, &mut *cr, d.cr)] {
                    for i in j * 16..j * 16 + 4 {
                        if sl.non_zero_count_cache[SCAN8[i]] != 0 || sl.mb[i * 16] != 0 {
                            add_pixels(
                                4,
                                plane,
                                at + offsets[i],
                                d.uvlinesize,
                                &mut sl.mb[i * 16..],
                            );
                        }
                    }
                    if chroma422 {
                        for i in j * 16 + 4..j * 16 + 8 {
                            if sl.non_zero_count_cache[SCAN8[i + 4]] != 0 || sl.mb[i * 16] != 0 {
                                add_pixels(
                                    4,
                                    plane,
                                    at + offsets[i + 4],
                                    d.uvlinesize,
                                    &mut sl.mb[i * 16..],
                                );
                            }
                        }
                    }
                }
            }
            return Ok(());
        }

        let pps = self.ps.pps.as_deref().ok_or(DecodeError)?;
        let bump = if chroma422 { 3 } else { 0 };
        let qp = [sl.hdr.chroma_qp[0] + bump, sl.hdr.chroma_qp[1] + bump];
        let intra = is_intra(mb_type);
        for (k, list) in [
            (0usize, if intra { 1 } else { 4 }),
            (1, if intra { 2 } else { 5 }),
        ] {
            if sl.non_zero_count_cache[SCAN8[CHROMA_DC_BLOCK_INDEX + k]] == 0 {
                continue;
            }
            let qmul = pps
                .dequant4(list, usize::try_from(qp[k]).map_err(|_| DecodeError)?)
                .ok_or(DecodeError)?[0];
            let block = &mut sl.mb[16 * 16 * (k + 1)..];
            if chroma422 {
                chroma422_dc_dequant_idct(depth, block, qmul as i32);
            } else {
                chroma_dc_dequant_idct(depth, block, qmul as i32);
            }
        }
        let planes = [&mut *cb, &mut *cr];
        if chroma422 {
            idct_add8_422(
                depth,
                planes,
                [d.cb, d.cr],
                offsets,
                &mut sl.mb[..],
                d.uvlinesize,
                &sl.non_zero_count_cache,
            );
        } else {
            idct_add8(
                depth,
                planes,
                [d.cb, d.cr],
                offsets,
                &mut sl.mb[..],
                d.uvlinesize,
                &sl.non_zero_count_cache,
            );
        }
        Ok(())
    }

    /// An I_PCM macroblock's samples, as coded: bytes at 8 bits, else read
    /// at the luma depth; a monochrome stream's chroma set to the middle.
    fn intra_pcm<P: Pixel>(
        &self,
        sl: &SliceContext,
        [y, cb, cr]: [&mut [P]; 3],
        d: Dest,
        nal: &[u8],
    ) -> Result<(), DecodeError> {
        let sps = self.ps.sps.as_deref().ok_or(DecodeError)?;
        let bit_depth = sps.bit_depth_luma;
        let chroma = usize::try_from(sps.chroma_format_idc).map_err(|_| DecodeError)?;
        let block_h = 16 >> self.chroma_y_shift;
        let chroma444 = chroma == 3;
        let src = nal.get(sl.intra_pcm_pos..).ok_or(DecodeError)?;
        let size = MB_SIZES.get(chroma).ok_or(DecodeError)? * bit_depth as usize;
        // Samples in coded order: luma rows, then Cb's, then Cr's.
        let mut next: Box<dyn FnMut() -> i32 + '_> = if self.pixel_shift != 0 {
            let mut gb = Bits::new(src, size).ok_or(DecodeError)?;
            gb.set_unchecked(true);
            Box::new(move || gb.bits(bit_depth as u32) as i32)
        } else {
            let mut at = 0usize;
            Box::new(move || {
                let v = src.get(at).copied().unwrap_or(0);
                at += 1;
                i32::from(v)
            })
        };
        let (cw, ch) = if chroma444 { (16, 16) } else { (8, block_h) };
        for row in 0..16 {
            for x in 0..16 {
                y[d.y + row * d.linesize + x] = P::from_int(next());
            }
        }
        if chroma == 0 {
            let mid = 1 << (bit_depth - 1);
            for row in 0..block_h {
                for x in 0..8 {
                    cb[d.cb + row * d.uvlinesize + x] = P::from_int(mid);
                    cr[d.cr + row * d.uvlinesize + x] = P::from_int(mid);
                }
            }
            return Ok(());
        }
        for (plane, at) in [(cb, d.cb), (cr, d.cr)] {
            for row in 0..ch {
                for x in 0..cw {
                    plane[at + row * d.uvlinesize + x] = P::from_int(next());
                }
            }
        }
        Ok(())
    }
}

/// Eight samples of the unfiltered row kept in `t` swapped with the
/// picture's at `at` -- or, without `swap`, the kept ones copied over the
/// picture's (FFmpeg's `XCHG`).
fn xchg8<P: Pixel>(t: &mut [u16], s: &mut [P], at: usize, swap: bool) {
    for (k, kept) in t.iter_mut().enumerate().take(8) {
        let picture = s[at + k].int();
        s[at + k] = P::from_int(i32::from(*kept));
        if swap {
            *kept = picture as u16;
        }
    }
}

impl Context {
    /// An intra macroblock's luma (or, in 4:4:4, a plane's) predicted
    /// (`hl_decode_mb_predict_luma`): 4x4 and 8x8 blocks each predicted
    /// and their residual added before the next, Intra16x16 predicted
    /// whole and its DC transform put in its blocks' DC places.
    #[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
    #[allow(
        clippy::too_many_lines,
        reason = "one function in FFmpeg, kept whole to read against it"
    )]
    fn predict_luma<P: Pixel>(
        &self,
        sl: &mut SliceContext,
        mb_type: u32,
        transform_bypass: bool,
        d: Dest,
        s: &mut [P],
        dest: usize,
        p: usize,
        depth: Depth,
    ) -> Result<(), DecodeError> {
        let sps = self.ps.sps.as_deref().ok_or(DecodeError)?;
        let pps = self.ps.pps.as_deref().ok_or(DecodeError)?;
        let qscale = if p == 0 {
            sl.hdr.qscale
        } else {
            sl.hdr.chroma_qp[p - 1]
        };
        let offsets = &self.block_offset[d.block_offset + 16 * p..];
        let linesize = d.linesize;
        let bits = depth.bits;
        // High 4:4:4 Predictive codes lossless vertical and horizontal
        // prediction with the residual summed along the prediction.
        let lossless_pred = transform_bypass && sps.profile_idc == 244;

        if !is_intra4x4(mb_type) {
            pred16x16(sl.intra16x16_pred_mode, s, dest, linesize, bits);
            if sl.non_zero_count_cache[SCAN8[LUMA_DC_BLOCK_INDEX + p]] != 0 {
                if transform_bypass {
                    const DC_MAPPING: [usize; 16] = [
                        0, 16, 64, 80, 32, 48, 96, 112, 128, 144, 192, 208, 160, 176, 224, 240,
                    ];
                    for (i, &at) in DC_MAPPING.iter().enumerate() {
                        sl.mb[p * 256 + at] = sl.mb_luma_dc[p][i];
                    }
                } else {
                    let qp = usize::try_from(qscale).map_err(|_| DecodeError)?;
                    let qmul = pps.dequant4(p, qp).ok_or(DecodeError)?[0];
                    luma_dc_dequant_idct(
                        depth,
                        &mut sl.mb[p * 256..],
                        &sl.mb_luma_dc[p],
                        qmul as i32,
                    );
                }
            }
            return Ok(());
        }

        if is_8x8dct(mb_type) {
            for i in (0..16).step_by(4) {
                let ptr = dest + offsets[i];
                let dir = sl.intra4x4_pred_mode_cache[SCAN8[i]];
                let coefficients = i * 16 + p * 256;
                let has_topleft = (sl.topleft_samples_available << i) & 0x8000 != 0;
                let has_topright = (sl.topright_samples_available << i) & 0x4000 != 0;
                if lossless_pred && dir <= 1 {
                    // x264 before build 151 coded these from the
                    // unfiltered edge; FFmpeg decodes its streams so.
                    #[allow(clippy::cast_sign_loss, reason = "compared unsigned, as FFmpeg does")]
                    let old_x264 = (self.x264_build as u32) < 151;
                    let block = &mut sl.mb[coefficients..];
                    if old_x264 {
                        pred8x8l_add(dir == 0, s, ptr, linesize, block);
                    } else {
                        pred8x8l_filter_add(
                            dir == 0,
                            s,
                            ptr,
                            linesize,
                            block,
                            has_topleft,
                            has_topright,
                        );
                    }
                    continue;
                }
                let nnz = sl.non_zero_count_cache[SCAN8[i + p * 16]];
                pred8x8l(dir, s, ptr, linesize, has_topleft, has_topright, bits);
                if nnz != 0 {
                    let block = &mut sl.mb[coefficients..];
                    if transform_bypass {
                        add_pixels(8, s, ptr, linesize, block);
                    } else if nnz == 1 && block[0] != 0 {
                        idct8_dc_add(depth, s, ptr, linesize, block);
                    } else {
                        idct8_add(depth, s, ptr, linesize, block);
                    }
                }
            }
            return Ok(());
        }

        for i in 0..16 {
            let ptr = dest + offsets[i];
            let dir = sl.intra4x4_pred_mode_cache[SCAN8[i]];
            let coefficients = i * 16 + p * 256;
            if lossless_pred && dir <= 1 {
                pred4x4_add(dir == 0, s, ptr, linesize, &mut sl.mb[coefficients..]);
                continue;
            }
            // The samples above-right, for the two modes that read them: or
            // the top row's last repeated where they are not available.
            let topright = if dir == DIAG_DOWN_LEFT_PRED || dir == VERT_LEFT_PRED {
                if (sl.topright_samples_available << i) & 0x8000 == 0 {
                    [s[ptr + 3 - linesize].int(); 4]
                } else {
                    core::array::from_fn(|k| s[ptr + 4 + k - linesize].int())
                }
            } else {
                [0; 4]
            };
            pred4x4(dir, s, ptr, linesize, topright, bits);
            let nnz = sl.non_zero_count_cache[SCAN8[i + p * 16]];
            if nnz != 0 {
                let block = &mut sl.mb[coefficients..];
                if transform_bypass {
                    add_pixels(4, s, ptr, linesize, block);
                } else if nnz == 1 && block[0] != 0 {
                    idct_dc_add(depth, s, ptr, linesize, block);
                } else {
                    idct_add(depth, s, ptr, linesize, block);
                }
            }
        }
        Ok(())
    }

    /// A non-Intra4x4 macroblock's luma (or a 4:4:4 plane's) residual
    /// added (`hl_decode_mb_idct_luma`).
    #[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
    fn idct_luma<P: Pixel>(
        &self,
        sl: &mut SliceContext,
        mb_type: u32,
        transform_bypass: bool,
        d: Dest,
        s: &mut [P],
        dest: usize,
        p: usize,
        depth: Depth,
    ) {
        if is_intra4x4(mb_type) {
            return;
        }
        let offsets = &self.block_offset[d.block_offset + 16 * p..];
        let linesize = d.linesize;
        let nnz = &sl.non_zero_count_cache[p * 5 * 8..];
        let coefficients = &mut sl.mb[p * 256..];
        if is_intra16x16(mb_type) {
            if !transform_bypass {
                idct_add16intra(depth, s, dest, offsets, coefficients, linesize, nnz);
                return;
            }
            let lossless_pred = self
                .ps
                .sps
                .as_deref()
                .is_some_and(|sps| sps.profile_idc == 244);
            let mode = sl.intra16x16_pred_mode;
            if lossless_pred && (mode == VERT_PRED8X8 || mode == HOR_PRED8X8) {
                let blocks = (0..16).map(|i| (i, i));
                pred_blocks_add(
                    mode == VERT_PRED8X8,
                    s,
                    dest,
                    offsets,
                    blocks,
                    coefficients,
                    linesize,
                );
            } else {
                for i in 0..16 {
                    if nnz[SCAN8[i]] != 0 || coefficients[i * 16] != 0 {
                        add_pixels(
                            4,
                            s,
                            dest + offsets[i],
                            linesize,
                            &mut coefficients[i * 16..],
                        );
                    }
                }
            }
        } else if sl.cbp & 15 != 0 {
            if transform_bypass {
                let n = if is_8x8dct(mb_type) { 8 } else { 4 };
                for i in (0..16).step_by(if n == 8 { 4 } else { 1 }) {
                    if nnz[SCAN8[i]] != 0 {
                        add_pixels(
                            n,
                            s,
                            dest + offsets[i],
                            linesize,
                            &mut coefficients[i * 16..],
                        );
                    }
                }
            } else if is_8x8dct(mb_type) {
                idct8_add4(depth, s, dest, offsets, coefficients, linesize, nnz);
            } else {
                idct_add16(depth, s, dest, offsets, coefficients, linesize, nnz);
            }
        }
    }

    /// The row above an intra macroblock swapped with its unfiltered copy
    /// before prediction (`xchg` true), and back after (`xchg_mb_border`):
    /// the eight samples above-left, the sixteen above, the eight
    /// above-right, and chroma's. Without `xchg` the first eight luma
    /// samples are copied back rather than swapped, as FFmpeg does: no
    /// one reads their unfiltered copy after this macroblock.
    fn xchg_mb_border<P: Pixel>(
        &self,
        sl: &mut SliceContext,
        [y, cb, cr]: [&mut [P]; 3],
        d: Dest,
        xchg: bool,
    ) {
        let mb_field = sl.hdr.mb_field_decoding_flag;
        let mut top_idx = 1;
        if self.mb_aff_frame {
            if sl.mb_y & 1 != 0 {
                if sl.hdr.mb_mbaff == 0 {
                    return;
                }
            } else {
                top_idx = if sl.hdr.mb_mbaff != 0 { 0 } else { 1 };
            }
        }
        let (deblock_topleft, deblock_top) = if sl.hdr.deblocking_filter == 2 {
            let xy = sl.mb_xy - 1 - (self.mb_stride << mb_field);
            let same =
                self.slice_table[tab(table_offset(self.mb_stride), xy)] == sl.slice_num as u16;
            (same, sl.top_type != 0)
        } else {
            (sl.mb_x > 0, sl.mb_y > i32::from(mb_field != 0))
        };
        if !deblock_top {
            return;
        }
        let mb_x = sl.mb_x as usize;
        let more_right = sl.mb_x + 1 < self.mb_width;
        // The sample above-left of the macroblock in each plane.
        let src_y = d.y - d.linesize - 1;
        let src_cb = d.cb - d.uvlinesize - 1;
        let src_cr = d.cr - d.uvlinesize - 1;
        let borders = &mut sl.top_borders[top_idx];
        let chroma = self.chroma_format_idc != 0;

        if deblock_topleft {
            xchg8(&mut borders[mb_x - 1][8..], y, src_y - 7, true);
        }
        xchg8(&mut borders[mb_x][0..], y, src_y + 1, xchg);
        xchg8(&mut borders[mb_x][8..], y, src_y + 9, true);
        if more_right {
            xchg8(&mut borders[mb_x + 1][0..], y, src_y + 17, true);
        }
        if !chroma {
            return;
        }
        if self.chroma_format_idc == 3 {
            if deblock_topleft {
                xchg8(&mut borders[mb_x - 1][24..], cb, src_cb - 7, true);
                xchg8(&mut borders[mb_x - 1][40..], cr, src_cr - 7, true);
            }
            xchg8(&mut borders[mb_x][16..], cb, src_cb + 1, xchg);
            xchg8(&mut borders[mb_x][24..], cb, src_cb + 9, true);
            xchg8(&mut borders[mb_x][32..], cr, src_cr + 1, xchg);
            xchg8(&mut borders[mb_x][40..], cr, src_cr + 9, true);
            if more_right {
                xchg8(&mut borders[mb_x + 1][16..], cb, src_cb + 17, true);
                xchg8(&mut borders[mb_x + 1][32..], cr, src_cr + 17, true);
            }
        } else {
            if deblock_topleft {
                xchg8(&mut borders[mb_x - 1][16..], cb, src_cb - 7, true);
                xchg8(&mut borders[mb_x - 1][24..], cr, src_cr - 7, true);
            }
            xchg8(&mut borders[mb_x][16..], cb, src_cb + 1, true);
            xchg8(&mut borders[mb_x][24..], cr, src_cr + 1, true);
        }
    }
}
