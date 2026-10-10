//! A slice's macroblocks decoded, made samples and filtered (`decode_slice`,
//! `loop_filter`, `backup_mb_border`, `predict_field_decoding_flag` in
//! `h264_slice.c`).
//!
//! Macroblocks are decoded one at a time (in an MBAFF frame a pair at a
//! time), each made samples at once; when a row is done the loop filter
//! runs along it, first keeping each macroblock's bottom row unfiltered for
//! the intra prediction of the row below.
//!
//! The picture being decoded is taken out of its slot for the slice and
//! written through unique access: nothing else holds it while it is
//! decoded (the extra reference FFmpeg keeps for error concealment is an
//! alias here until the picture is done -- `Context::last_pic_for_ec`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "macroblock coordinates and sample offsets inside the picture"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "sample indices are a macroblock's rows inside the picture; border indices are \
              under the macroblock width the borders were sized for"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int conversions of macroblock coordinates, made where it makes them"
)]

use std::sync::Arc;

use crate::bits::Bits;
use crate::cabacmb::Data;
use crate::decoder::{Context, DecodeError, SliceContext};
use crate::loopfilter::Planes;
use crate::mbtype::is_interlaced;
use crate::mvpred::tab;
use crate::picture::{AnyFrame, FrameBuf, Pixel, table_offset};
use crate::slice::PICT_FRAME;

impl Context {
    /// A slice's macroblocks (`decode_slice`), from where its header left
    /// `sl` to its end, into the picture being decoded.
    pub(crate) fn decode_slice_data(
        &mut self,
        sl: &mut SliceContext,
        gb: &mut Bits<'_>,
    ) -> Result<(), DecodeError> {
        let cur = self.cur_pic_ptr.ok_or(DecodeError)?;
        let mut frame = self.pics[cur].f.take().ok_or(DecodeError)?;
        // Nothing else holds a picture while it is decoded; were something
        // to, it would keep the samples as they were.
        let result = self.decode_slice_into(sl, gb, Arc::make_mut(&mut frame));
        self.pics[cur].f = Some(frame);
        result
    }

    fn decode_slice_into(
        &mut self,
        sl: &mut SliceContext,
        gb: &mut Bits<'_>,
        frame: &mut AnyFrame,
    ) -> Result<(), DecodeError> {
        let (linesize, uvlinesize) = match frame {
            AnyFrame::Eight(f) => (f.planes[0].stride, f.planes[1].stride),
            AnyFrame::High(f) => (f.planes[0].stride, f.planes[1].stride),
        };
        sl.linesize = linesize;
        sl.uvlinesize = uvlinesize;
        sl.mb_skip_run = -1;
        let mut lf_x_start = sl.mb_x;
        let orig_deblock = sl.hdr.deblocking_filter;
        if self.postpone_filter {
            sl.hdr.deblocking_filter = 0;
        }
        let pps = self.ps.pps.clone().ok_or(DecodeError)?;
        let field_or_mbaff = self.picture_structure != PICT_FRAME || self.mb_aff_frame;
        if pps.cabac != 0 {
            gb.align();
            let d = Data {
                buf: gb.buffer(),
                origin: gb.origin(),
            };
            sl.cabac.init(
                d.buf,
                d.origin,
                (gb.count() / 8) as isize,
                ((gb.left() + 7) / 8) as isize,
            )?;
            self.init_cabac_states(sl)?;
            loop {
                if sl.mb_x + sl.mb_y * self.mb_width >= sl.next_slice_idx {
                    // "Slice overlaps with next".
                    return Err(DecodeError);
                }
                let mut ret = self.decode_mb_cabac(sl, d);
                if ret.is_ok() {
                    self.hl_decode_mb(sl, frame, d.buf)?;
                }
                if ret.is_ok() && self.mb_aff_frame {
                    sl.mb_y += 1;
                    ret = self.decode_mb_cabac(sl, d);
                    if ret.is_ok() {
                        self.hl_decode_mb(sl, frame, d.buf)?;
                    }
                    sl.mb_y -= 1;
                }
                let eos = sl.cabac.terminate(d.buf);
                // A slice may read a little past its data (the workaround
                // for truncated slices is off, as it is by default), but no
                // more than four bytes.
                if ret.is_err() || sl.cabac.pos > sl.cabac.end + 4 {
                    return Err(DecodeError);
                }
                sl.mb_x += 1;
                if sl.mb_x >= self.mb_width {
                    self.loop_filter(sl, frame, lf_x_start, sl.mb_x);
                    sl.mb_x = 0;
                    lf_x_start = 0;
                    sl.mb_y += 1;
                    if field_or_mbaff {
                        sl.mb_y += 1;
                        if self.mb_aff_frame && sl.mb_y < self.mb_height {
                            self.predict_field_decoding_flag(sl);
                        }
                    }
                }
                if eos != 0 || sl.mb_y >= self.mb_height {
                    if sl.mb_x > lf_x_start {
                        self.loop_filter(sl, frame, lf_x_start, sl.mb_x);
                    }
                    sl.hdr.deblocking_filter = orig_deblock;
                    return Ok(());
                }
            }
        }

        // Macroblocks are read as h264_cavlc.c reads them.
        gb.set_unchecked(true);
        let nal = gb.buffer();
        loop {
            if sl.mb_x + sl.mb_y * self.mb_width >= sl.next_slice_idx {
                // "Slice overlaps with next".
                return Err(DecodeError);
            }
            let mut ret = self.decode_mb_cavlc(sl, gb);
            if ret.is_ok() {
                self.hl_decode_mb(sl, frame, nal)?;
            }
            if ret.is_ok() && self.mb_aff_frame {
                sl.mb_y += 1;
                ret = self.decode_mb_cavlc(sl, gb);
                if ret.is_ok() {
                    self.hl_decode_mb(sl, frame, nal)?;
                }
                sl.mb_y -= 1;
            }
            ret?;

            sl.mb_x += 1;
            if sl.mb_x >= self.mb_width {
                self.loop_filter(sl, frame, lf_x_start, sl.mb_x);
                sl.mb_x = 0;
                lf_x_start = 0;
                sl.mb_y += 1;
                if field_or_mbaff {
                    sl.mb_y += 1;
                    if self.mb_aff_frame && sl.mb_y < self.mb_height {
                        self.predict_field_decoding_flag(sl);
                    }
                }
                if sl.mb_y >= self.mb_height {
                    // Bits left over end the slice well unless errors are
                    // looked for aggressively, which they are not.
                    sl.hdr.deblocking_filter = orig_deblock;
                    return if gb.left() >= 0 {
                        Ok(())
                    } else {
                        Err(DecodeError)
                    };
                }
            }

            if gb.left() <= 0 && sl.mb_skip_run <= 0 {
                if gb.left() == 0 {
                    if sl.mb_x > lf_x_start {
                        self.loop_filter(sl, frame, lf_x_start, sl.mb_x);
                    }
                    sl.hdr.deblocking_filter = orig_deblock;
                    return Ok(());
                }
                return Err(DecodeError);
            }
        }
    }

    /// The loop filter along a row of macroblocks, `start_x` to `end_x`
    /// (`loop_filter`), each one's unfiltered bottom row kept first.
    fn loop_filter(&self, sl: &mut SliceContext, frame: &mut AnyFrame, start_x: i32, end_x: i32) {
        match frame {
            AnyFrame::Eight(f) => self.loop_filter_t(sl, f, start_x, end_x),
            AnyFrame::High(f) => self.loop_filter_t(sl, f, start_x, end_x),
        }
    }

    fn loop_filter_t<P: Pixel>(
        &self,
        sl: &mut SliceContext,
        fb: &mut FrameBuf<P>,
        start_x: i32,
        end_x: i32,
    ) {
        let mbaff = i32::from(self.mb_aff_frame);
        let end_mb_y = sl.mb_y + mbaff;
        let old_slice_type = sl.hdr.slice_type;
        let block_h = 16 >> self.chroma_y_shift;
        let chroma444 = self.chroma_format_idc == 3;
        if self.postpone_filter {
            return;
        }
        if sl.hdr.deblocking_filter != 0 {
            let [py, pcb, pcr] = &mut fb.planes;
            for mb_x in start_x..end_x {
                for mb_y in end_mb_y - mbaff..=end_mb_y {
                    let mb_xy = mb_x + mb_y * self.mb_stride;
                    sl.mb_xy = mb_xy;
                    let mb_type = self.cur_type(mb_xy);
                    if self.mb_aff_frame {
                        let field = i32::from(is_interlaced(mb_type));
                        sl.hdr.mb_mbaff = field;
                        sl.hdr.mb_field_decoding_flag = field;
                    }
                    sl.mb_x = mb_x;
                    sl.mb_y = mb_y;
                    let (x, y) = (mb_x as usize, mb_y as usize);
                    let mut dest_y = x * 16 + y * sl.linesize * 16;
                    let mut dest_c =
                        x * (8 << usize::from(chroma444)) + y * sl.uvlinesize * block_h;
                    let (linesize, uvlinesize) = if sl.hdr.mb_field_decoding_flag != 0 {
                        if y & 1 != 0 {
                            dest_y -= sl.linesize * 15;
                            dest_c -= sl.uvlinesize * (block_h - 1);
                        }
                        (sl.linesize * 2, sl.uvlinesize * 2)
                    } else {
                        (sl.linesize, sl.uvlinesize)
                    };
                    sl.mb_linesize = linesize;
                    sl.mb_uvlinesize = uvlinesize;
                    self.backup_mb_border(
                        sl,
                        [&py.data, &pcb.data, &pcr.data],
                        dest_y,
                        dest_c,
                        linesize,
                        uvlinesize,
                    );
                    if self.fill_filter_caches(sl, mb_type) {
                        continue;
                    }
                    let qp = self.cur_qp(mb_xy);
                    sl.hdr.chroma_qp = [self.chroma_qp(0, qp), self.chroma_qp(1, qp)];
                    let mut img = Planes {
                        y: &mut py.data,
                        cb: &mut pcb.data,
                        cr: &mut pcr.data,
                        at: [dest_y, dest_c, dest_c],
                    };
                    self.filter_mb(sl, &mut img, mb_x, mb_y, linesize, uvlinesize);
                    // Under H264_LF the line `tools/ffdbg`'s FFDBG_LF prints
                    // in FFmpeg, for `tools/h264cmp.py`: the macroblock's
                    // own samples once its edges are filtered.
                    #[cfg(test)]
                    if std::env::var_os("H264_LF").is_some() {
                        let cols = 8 << usize::from(chroma444);
                        let wide = std::mem::size_of::<P>() > 1;
                        let sum =
                            |plane: &[P], at: usize, stride: usize, rows: usize, cols: usize| {
                                let (mut a, mut b) = (1u32, 0u32);
                                for r in 0..rows {
                                    for c in 0..cols {
                                        let v = plane[at + r * stride + c].int() as u32;
                                        let bytes: &[u32] =
                                            if wide { &[v & 0xFF, v >> 8] } else { &[v] };
                                        for &byte in bytes {
                                            a = (a + byte) % 65521;
                                            b = (b + a) % 65521;
                                        }
                                    }
                                }
                                (b << 16) | a
                            };
                        eprintln!(
                            "LF poc={} mb={},{} sum={:08x},{:08x},{:08x}",
                            self.cur_pic_ptr.map_or(-1, |c| self.pics[c].poc),
                            mb_x,
                            mb_y,
                            sum(&py.data, dest_y, linesize, 16, 16),
                            sum(&pcb.data, dest_c, uvlinesize, block_h, cols),
                            sum(&pcr.data, dest_c, uvlinesize, block_h, cols)
                        );
                    }
                }
            }
        }
        sl.hdr.slice_type = old_slice_type;
        sl.mb_x = end_x;
        sl.mb_y = end_mb_y - mbaff;
        sl.hdr.chroma_qp = [
            self.chroma_qp(0, sl.hdr.qscale),
            self.chroma_qp(1, sl.hdr.qscale),
        ];
    }

    /// A macroblock's bottom row kept unfiltered before the filter runs
    /// (`backup_mb_border`): the row the macroblock below predicts from --
    /// in an MBAFF frame, the rows each field of the pair below does.
    fn backup_mb_border<P: Pixel>(
        &self,
        sl: &mut SliceContext,
        [y, cb, cr]: [&[P]; 3],
        dest_y: usize,
        dest_c: usize,
        linesize: usize,
        uvlinesize: usize,
    ) {
        let mb_x = sl.mb_x as usize;
        let chroma444 = self.chroma_format_idc == 3;
        let chroma422 = self.chroma_format_idc == 2;
        // The rows kept: luma's last, and chroma's.
        let keep = |t: &mut [u16; 48], luma_row: usize, chroma_row: usize| {
            for k in 0..16 {
                t[k] = y[dest_y + luma_row * linesize + k].int() as u16;
            }
            let (n, cr_at) = if chroma444 { (16, 32) } else { (8, 24) };
            for k in 0..n {
                t[16 + k] = cb[dest_c + chroma_row * uvlinesize + k].int() as u16;
                t[cr_at + k] = cr[dest_c + chroma_row * uvlinesize + k].int() as u16;
            }
        };
        let last_c = if chroma444 || chroma422 { 15 } else { 7 };
        let mut top_idx = 1;
        if self.mb_aff_frame {
            if sl.mb_y & 1 != 0 {
                if sl.hdr.mb_mbaff == 0 {
                    // The bottom macroblock of a frame pair: the row above
                    // its last too, for a field pair below.
                    keep(&mut sl.top_borders[0][mb_x], 14, last_c - 1);
                }
            } else if sl.hdr.mb_mbaff != 0 {
                top_idx = 0;
            } else {
                return;
            }
        }
        keep(&mut sl.top_borders[top_idx][mb_x], 15, last_c);
    }

    /// Whether the next pair of an MBAFF frame is a field pair, guessed
    /// from its neighbours for skipped pairs (`predict_field_decoding_flag`).
    fn predict_field_decoding_flag(&self, sl: &mut SliceContext) {
        let mb_xy = sl.mb_x + sl.mb_y * self.mb_stride;
        let off = table_offset(self.mb_stride);
        let ours = |xy: i32| self.slice_table[tab(off, xy)] == sl.slice_num as u16;
        let mb_type = if ours(mb_xy - 1) {
            self.cur_type(mb_xy - 1)
        } else if ours(mb_xy - self.mb_stride) {
            self.cur_type(mb_xy - self.mb_stride)
        } else {
            0
        };
        let field = i32::from(is_interlaced(mb_type));
        sl.hdr.mb_mbaff = field;
        sl.hdr.mb_field_decoding_flag = field;
    }
}
