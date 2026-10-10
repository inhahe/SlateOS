//! Error concealment (`error_resilience.c`, with the H.264 decoder's side
//! of it from `h264dec.c` and `h264_slice.c`): the macroblocks of a picture
//! that no slice decoded, or that a slice failed partway through, guessed
//! from what is around them -- their motion from their neighbours' and the
//! picture before's, their samples predicted with it or, failing that, flat
//! from the neighbouring blocks' averages -- and their edges smoothed.
//!
//! Each slice reports what it decoded ([`Er::add_slice`]): from where to
//! where its macroblocks came out whole, and whether it ended or failed.
//! When a packet's slices are done, everything not covered is concealed
//! ([`Context::conceal`]). FFmpeg conceals frame pictures only; a field
//! picture's lost macroblocks keep what its buffer held.
//!
//! FFmpeg's routines here read and write a picture's samples as bytes
//! (`uint8_t *`), whatever their size: at more than 8 bits a macroblock
//! 16 samples wide is 8 samples to them, and a value they store lands in one
//! byte of a sample. So do these ([`AnyFrame::er_byte`]), so that a damaged
//! picture of more than 8 bits comes out as FFmpeg's does.
//!
//! What FFmpeg's routines do only for other codecs is left out: data
//! partitioning (H.264's partitions are not decoded), skip and intra tables
//! of the MPEG decoders, temporal direct vectors, and MPEG-2's last-row
//! exception. FFmpeg's default strategy is kept: vectors guessed
//! iteratively and damaged edges deblocked (`-ec guess_mvs+deblock`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "macroblock and block coordinates inside the picture, and sums of sample values"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "table indices are macroblocks and blocks of the picture the tables were sized for, \
              sample indices bytes inside its planes"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int conversions: vectors and DC values stored into int16_t, indices and \
              counts converted where it converts them"
)]

use std::sync::Arc;

use crate::decoder::{Context, SliceContext};
use crate::mbtype::{MB_TYPE_16X16, MB_TYPE_INTRA4X4, MB_TYPE_L0, SCAN8, is_inter, is_intra};
use crate::picture::{AnyFrame, LAST_PIC_FOR_EC, Ref, table_offset};
use crate::slice::{PICT_FRAME, TYPE_B, TYPE_I};

/// The first macroblock after a resynchronisation point: a slice's first.
const VP_START: u8 = 1;
/// A macroblock's residual, DC or vectors are damaged.
const ER_AC_ERROR: u8 = 2;
const ER_DC_ERROR: u8 = 4;
const ER_MV_ERROR: u8 = 8;
/// A slice's last macroblock, where its residual, DC and vectors end well.
const ER_AC_END: u8 = 16;
const ER_DC_END: u8 = 32;
const ER_MV_END: u8 = 64;
/// All three damaged; all three ended (`ER_MB_ERROR`, `ER_MB_END`).
pub(crate) const ER_MB_ERROR: u8 = ER_AC_ERROR | ER_DC_ERROR | ER_MV_ERROR;
pub(crate) const ER_MB_END: u8 = ER_AC_END | ER_DC_END | ER_MV_END;

/// How far a macroblock is in vector guessing: settled, guessed again with
/// a different vector, guessed with the same, or queued.
const MV_FROZEN: u8 = 8;
const MV_CHANGED: u8 = 4;
const MV_UNCHANGED: u8 = 2;
const MV_LISTED: u8 = 1;

/// `FF_DECODE_ERROR_CONCEALMENT_ACTIVE`: part of the picture is guessed.
pub(crate) const DECODE_ERROR_CONCEALMENT_ACTIVE: i32 = 4;

/// What concealment keeps for a picture size (`ERContext`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Er {
    /// Each macroblock's place in the tables, in decoding order, and one
    /// past the last (`mb_index2xy`).
    mb_index2xy: Vec<usize>,
    mb_num: usize,
    mb_width: usize,
    mb_height: usize,
    mb_stride: usize,
    /// Row stride of the luma DC table: two 8x8 blocks a macroblock and one
    /// to spare (`b8_stride`).
    b8_stride: usize,
    /// What is still undecoded, three a macroblock (its DC, residual and
    /// vectors); `i32::MAX` once anything failed (`error_count`).
    error_count: i32,
    /// A slice failed, or the slice before one did not end where it began
    /// (`error_occurred`): the picture is marked for it.
    pub(crate) error_occurred: bool,
    /// Each macroblock's [`VP_START`] and `ER_*` bits, `mb_stride` wide
    /// (`error_status_table`).
    error_status_table: Vec<u8>,
    /// Vector guessing's scratch (`er_temp_buffer`): the macroblocks to
    /// visit in this round and the next, and each one's state.
    blocklist: Vec<[usize; 2]>,
    next_blocklist: Vec<[usize; 2]>,
    fixed: Vec<u8>,
    /// The DC of each 8x8 luma block and of each macroblock's chroma, with a
    /// margin around each plane (`dc_val_base`); where each plane's first
    /// block is in it (`dc_val`). Each starts 1024 and is written in full
    /// every time a picture is concealed.
    dc_val_base: Vec<i16>,
    dc_val: [usize; 3],
}

impl Er {
    /// The tables for pictures of `mb_width` by `mb_height` macroblocks,
    /// rows `mb_stride` apart (the share of `ff_h264_alloc_tables`, and
    /// `ff_er_init`). `None` past what can be allocated.
    pub(crate) fn new(mb_width: usize, mb_height: usize, mb_stride: usize) -> Option<Self> {
        fn table<T: Clone>(len: usize, v: T) -> Option<Vec<T>> {
            let mut t = Vec::new();
            t.try_reserve_exact(len).ok()?;
            t.resize(len, v);
            Some(t)
        }
        let mb_num = mb_width.checked_mul(mb_height)?;
        let mb_array_size = mb_stride.checked_mul(mb_height)?;
        let big_mb_num = mb_stride.checked_mul(mb_height + 1)?;
        let y_size = (2 * mb_width + 1).checked_mul(2 * mb_height + 1)?;
        let yc_size = y_size.checked_add(2 * big_mb_num)?;
        let mut mb_index2xy = table(mb_num + 1, 0)?;
        for y in 0..mb_height {
            for x in 0..mb_width {
                mb_index2xy[x + y * mb_width] = x + y * mb_stride;
            }
        }
        mb_index2xy[mb_num] = (mb_height.checked_sub(1)?) * mb_stride + mb_width;
        Some(Self {
            mb_index2xy,
            mb_num,
            mb_width,
            mb_height,
            mb_stride,
            b8_stride: mb_width * 2 + 1,
            error_count: 0,
            error_occurred: false,
            error_status_table: table(mb_array_size, 0)?,
            blocklist: Vec::with_capacity(mb_array_size),
            next_blocklist: Vec::with_capacity(mb_array_size),
            fixed: table(mb_array_size, 0)?,
            dc_val_base: table(yc_size, 1024)?,
            dc_val: [
                mb_width * 2 + 2,
                y_size + mb_stride + 1,
                y_size + mb_stride + 1 + big_mb_num,
            ],
        })
    }

    /// A new frame: every macroblock lost until a slice says otherwise
    /// (`ff_er_frame_start`).
    pub(crate) fn frame_start(&mut self) {
        self.error_status_table
            .fill(ER_MB_ERROR | VP_START | ER_MB_END);
        self.error_count = 3 * self.mb_num as i32;
        self.error_occurred = false;
    }

    /// A slice that decoded macroblocks `startx,starty` up to `endx,endy`,
    /// where it ended with `status` (`ff_er_add_slice`): its macroblocks
    /// before the end are whole, as far as `status` covers; `endx` may be
    /// -1, the end of the row before.
    ///
    /// FFmpeg also checks here whether the slice before ended well, but only
    /// while concealment holds a current picture, which for H.264 it never
    /// does while slices are decoded (`h264_field_start` lets it go); the
    /// check H.264 makes is [`Er::check_slice_start`].
    pub(crate) fn add_slice(&mut self, startx: i32, starty: i32, endx: i32, endy: i32, status: u8) {
        if self.mb_num == 0 {
            return;
        }
        let (mb_width, mb_num) = (self.mb_width as i32, self.mb_num as i32);
        let start_i = (startx + starty * mb_width).clamp(0, mb_num - 1);
        let end_i = (endx + endy * mb_width).clamp(0, mb_num);
        let start_xy = self.mb_index2xy[start_i as usize];
        let end_xy = self.mb_index2xy[end_i as usize];
        if start_i > end_i || start_xy > end_xy {
            // "internal error, slice end before start".
            return;
        }
        let mut mask = !i32::from(VP_START);
        let mut count = |mask: &mut i32, bits: u8| {
            if status & bits != 0 {
                *mask &= !i32::from(bits);
                self.error_count = self.error_count.wrapping_add(start_i - end_i - 1);
            }
        };
        count(&mut mask, ER_AC_ERROR | ER_AC_END);
        count(&mut mask, ER_DC_ERROR | ER_DC_END);
        count(&mut mask, ER_MV_ERROR | ER_MV_END);
        if status & ER_MB_ERROR != 0 {
            self.error_occurred = true;
            self.error_count = i32::MAX;
        }
        let table = &mut self.error_status_table;
        if mask == !0x7F {
            table[start_xy..end_xy].fill(0);
        } else {
            for e in &mut table[start_xy..end_xy] {
                *e &= mask as u8;
            }
        }
        if end_i == mb_num {
            self.error_count = i32::MAX;
        } else {
            table[end_xy] &= mask as u8;
            table[end_xy] |= status;
        }
        table[start_xy] |= VP_START;
    }

    /// A slice of a frame starting at `x,y`: if the slice before did not end
    /// right before it, slices are missing (`decode_slice`'s check).
    pub(crate) fn check_slice_start(&mut self, x: i32, y: i32) {
        if self.mb_num == 0 {
            return;
        }
        let start_i = (x + y * self.mb_width as i32).clamp(0, self.mb_num as i32 - 1) as usize;
        if start_i != 0 {
            let prev = self.error_status_table[self.mb_index2xy[start_i - 1]] & !VP_START;
            if prev != ER_MV_END | ER_DC_END | ER_AC_END {
                self.error_occurred = true;
            }
        }
    }

    /// A DC plane's values: plane `p`'s, from its first block.
    fn dc(&mut self, p: usize) -> &mut [i16] {
        &mut self.dc_val_base[self.dc_val[p]..]
    }
}

/// Whether the error bits mark a macroblock's DC and vectors both damaged.
fn dc_and_mv_damaged(error: u8) -> bool {
    error & ER_DC_ERROR != 0 && error & ER_MV_ERROR != 0
}

impl AnyFrame {
    /// Byte `i` of plane `p` as error concealment reads it, through a
    /// `uint8_t *`: at more than 8 bits each sample is two bytes, low first.
    fn er_byte(&self, p: usize, i: usize) -> i32 {
        match self {
            Self::Eight(f) => i32::from(f.planes[p].data[i]),
            Self::High(f) => i32::from(f.planes[p].data[i / 2].to_le_bytes()[i & 1]),
        }
    }

    /// The same, written.
    fn er_set_byte(&mut self, p: usize, i: usize, v: u8) {
        match self {
            Self::Eight(f) => f.planes[p].data[i] = v,
            Self::High(f) => {
                let s = &mut f.planes[p].data[i / 2];
                let mut b = s.to_le_bytes();
                b[i & 1] = v;
                *s = u16::from_le_bytes(b);
            }
        }
    }

    /// Plane `p`'s row stride in bytes (`linesize`).
    fn er_linesize(&self, p: usize) -> usize {
        match self {
            Self::Eight(f) => f.planes[p].stride,
            Self::High(f) => f.planes[p].stride * 2,
        }
    }

    /// The sum of absolute differences of the 16x16 bytes at `a` in this
    /// frame's luma and at `b` in `other`'s, rows `stride` apart
    /// (`pix_abs16_c`).
    fn er_sad16(&self, a: usize, other: &Self, b: usize, stride: usize) -> i32 {
        let mut sum = 0;
        for y in 0..16 {
            for x in 0..16 {
                sum += (self.er_byte(0, a + y * stride + x) - other.er_byte(0, b + y * stride + x))
                    .abs();
            }
        }
        sum
    }
}

/// `av_clip_uint8`, as `ff_crop_tab` gives it.
fn clip_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

impl Context {
    /// The picture's lost macroblocks concealed, once a packet's slices are
    /// decoded (`h264_decode_frame`'s block after `decode_nal_units`, then
    /// `ff_er_frame_end`). `sl` is the last slice's context: its reference
    /// lists are what concealment predicts from, the first entry of each
    /// the pictures before and after. A picture whose slices have no
    /// references predicts from the one kept for it when the references were
    /// last dropped (`last_pic_for_ec`), put in list 0 for the purpose.
    pub(crate) fn conceal(&mut self, sl: &mut SliceContext) {
        if self.picture_structure != PICT_FRAME || self.current_slice == 0 {
            return;
        }
        let Some(cur) = self.cur_pic_ptr else {
            return;
        };
        let use_last_pic = self.pics[LAST_PIC_FOR_EC].in_use() && sl.hdr.ref_count[0] == 0;
        let last = if use_last_pic {
            let ec = &self.pics[LAST_PIC_FOR_EC];
            let r = &mut sl.ref_list[0][0];
            r.parent = Some(LAST_PIC_FOR_EC);
            r.offset = [0; 3];
            r.stride =
                ec.f.as_deref()
                    .map_or([0; 3], |f| [f.stride(0), f.stride(1), f.stride(2)]);
            r.reference = ec.reference;
            Some(LAST_PIC_FOR_EC)
        } else if sl.hdr.ref_count[0] != 0 {
            sl.ref_list[0][0].parent
        } else {
            None
        };
        let next = if sl.hdr.ref_count[1] != 0 {
            sl.ref_list[1][0].parent
        } else {
            None
        };
        let flags = self.er_frame_end(sl, cur, last, next);
        self.pics[cur].props.decode_error_flags |= flags;
        if use_last_pic {
            sl.ref_list[0][0] = Ref::default();
        }
    }

    /// Picture `cur` concealed from `last` and `next` (`ff_er_frame_end`);
    /// the `FF_DECODE_ERROR_*` bits it earns.
    ///
    /// The current picture is never its own reference here: it joins the
    /// reference lists when its decoding ends (`ff_h264_field_end`), after
    /// this. Its frame is taken out of its slot while it is concealed.
    fn er_frame_end(
        &mut self,
        sl: &mut SliceContext,
        cur: usize,
        last: Option<usize>,
        next: Option<usize>,
    ) -> i32 {
        if self.er.error_count == 0
            || self.er.mb_num == 0
            || self.pics[cur].field_picture != 0
            || !self.pics[cur].in_use()
        {
            return 0;
        }
        #[cfg(test)]
        if std::env::var_os("H264_ER").is_some() {
            eprintln!(
                "ER concealing: error_count={} mb_num={}",
                self.er.error_count, self.er.mb_num
            );
        }
        // A picture of another size or layout is no use.
        let usable = |p: Option<usize>| p.filter(|&p| self.pics[p].same_shape(&self.pics[cur]));
        let (last, next) = (usable(last), usable(next));
        let Some(mut frame) = self.pics[cur].f.take() else {
            return 0;
        };
        let flags = self.er_conceal_frame(sl, Arc::make_mut(&mut frame), cur, last, next);
        self.pics[cur].f = Some(frame);
        flags
    }

    fn er_conceal_frame(
        &mut self,
        sl: &mut SliceContext,
        frame: &mut AnyFrame,
        cur: usize,
        last: Option<usize>,
        next: Option<usize>,
    ) -> i32 {
        let er = &mut self.er;
        let (mb_num, mb_width, mb_height, mb_stride) =
            (er.mb_num, er.mb_width, er.mb_height, er.mb_stride);
        // Pictures here always have samples: the slots given are in use.
        let has_data = |p: Option<usize>| p.is_some_and(|p| self.pics[p].in_use());
        let (have_last, have_next) = (has_data(last), has_data(next));

        // Overlapping slices: a macroblock after a slice's start that no
        // end or error reaches from behind is damaged.
        let table = &mut er.error_status_table;
        for error_type in 1..=3 {
            let mut end_ok = false;
            for i in (0..mb_num).rev() {
                let mb_xy = er.mb_index2xy[i];
                let error = table[mb_xy];
                if error & (1 << error_type) != 0 || error & (8 << error_type) != 0 {
                    end_ok = true;
                }
                if !end_ok {
                    table[mb_xy] |= 1 << error_type;
                }
                if error & VP_START != 0 {
                    end_ok = false;
                }
            }
        }

        // Missing slices, when errors are to be taken hard: from the end
        // back, everything before an ended slice followed by one never
        // decoded is damaged (FFmpeg's "+ 100" kept).
        if self.explode {
            let mut end_ok = true;
            let mut i = mb_num as i64 - 2;
            while i >= (mb_width + 100) as i64 {
                let mb_xy = er.mb_index2xy[i as usize];
                let error1 = table[mb_xy];
                let error2 = table[er.mb_index2xy[i as usize + 1]];
                if error1 & VP_START != 0 {
                    end_ok = true;
                }
                if error2 == VP_START | ER_MB_ERROR | ER_MB_END
                    && error1 != VP_START | ER_MB_ERROR | ER_MB_END
                    && error1 & (ER_AC_END | ER_DC_END | ER_MV_END) != 0
                {
                    end_ok = false;
                }
                if !end_ok {
                    table[mb_xy] |= ER_MB_ERROR;
                }
                i -= 1;
            }
        }

        // Errors marked backward: the 50 macroblocks before one are suspect
        // too, back to their slice's start.
        let mut distance: i32 = 9_999_999;
        for error_type in 1..=3 {
            for i in (0..mb_num).rev() {
                let mb_xy = er.mb_index2xy[i];
                let error = table[mb_xy];
                distance += 1;
                if error & (1 << error_type) != 0 {
                    distance = 0;
                }
                if distance < 50 {
                    table[mb_xy] |= 1 << error_type;
                }
                if error & VP_START != 0 {
                    distance = 9_999_999;
                }
            }
        }

        // Errors marked forward to the end of their slice.
        let mut error = 0;
        for i in 0..mb_num {
            let mb_xy = er.mb_index2xy[i];
            let old = table[mb_xy];
            if old & VP_START != 0 {
                error = old & ER_MB_ERROR;
            } else {
                error |= old & ER_MB_ERROR;
                table[mb_xy] |= error;
            }
        }

        // Without partitions, damage to any part is damage to all three.
        for i in 0..mb_num {
            let mb_xy = er.mb_index2xy[i];
            if table[mb_xy] & ER_MB_ERROR != 0 {
                table[mb_xy] |= ER_MB_ERROR;
            }
        }

        let flags = DECODE_ERROR_CONCEALMENT_ACTIVE;
        let is_intra_likely = self.er_is_intra_more_likely(frame, cur, last);

        // The damaged macroblocks take the likelier type.
        let guessed_mb_type = if is_intra_likely {
            MB_TYPE_INTRA4X4
        } else {
            MB_TYPE_16X16 | MB_TYPE_L0
        };
        let off = table_offset(self.pics[cur].mb_stride);
        for i in 0..mb_num {
            let mb_xy = self.er.mb_index2xy[i];
            if dc_and_mv_damaged(self.er.error_status_table[mb_xy]) {
                self.pics[cur].mb_type[off + mb_xy] = guessed_mb_type;
            }
        }
        // With nothing to predict from, every macroblock is intra.
        if !have_last && !have_next {
            for i in 0..mb_num {
                let mb_xy = self.er.mb_index2xy[i];
                let t = &mut self.pics[cur].mb_type[off + mb_xy];
                if !is_intra(*t) {
                    *t = MB_TYPE_INTRA4X4;
                }
            }
        }

        // FFmpeg next predicts inter macroblocks whose vectors survived but
        // whose residual did not; without partitions there are none, their
        // three error bits being set together above.

        if self.pics[cur].props.pict_type == TYPE_B {
            // A B picture's damaged inter macroblocks: zero vectors.
            for mb_y in 0..mb_height {
                for mb_x in 0..mb_width {
                    let mb_xy = mb_x + mb_y * mb_stride;
                    let error = self.er.error_status_table[mb_xy];
                    if is_intra(self.pics[cur].mb_type[off + mb_xy])
                        || error & ER_MV_ERROR == 0
                        || error & ER_AC_ERROR == 0
                    {
                        continue;
                    }
                    self.er_decode_mb(sl, frame, cur, 0, [0, 0], mb_x, mb_y);
                }
            }
        } else {
            self.er_guess_mv(sl, frame, cur, last, next);
        }

        // Every macroblock's DC, from its samples as they now are.
        let ls = [
            frame.er_linesize(0),
            frame.er_linesize(1),
            frame.er_linesize(2),
        ];
        let b8_stride = self.er.b8_stride;
        for mb_y in 0..mb_height {
            for mb_x in 0..mb_width {
                let dest_y = mb_x * 16 + mb_y * 16 * ls[0];
                let dest_c = [mb_x * 8 + mb_y * 8 * ls[1], mb_x * 8 + mb_y * 8 * ls[2]];
                for n in 0..4 {
                    let mut dc = 0;
                    for y in 0..8 {
                        for x in 0..8 {
                            dc += frame
                                .er_byte(0, dest_y + x + (n & 1) * 8 + (y + (n >> 1) * 8) * ls[0]);
                        }
                    }
                    let at = mb_x * 2 + mb_y * 2 * b8_stride + (n & 1) + (n >> 1) * b8_stride;
                    self.er.dc(0)[at] = ((dc + 4) >> 3) as i16;
                }
                let (mut dcu, mut dcv) = (0, 0);
                for y in 0..8 {
                    for x in 0..8 {
                        dcu += frame.er_byte(1, dest_c[0] + x + y * ls[1]);
                        dcv += frame.er_byte(2, dest_c[1] + x + y * ls[2]);
                    }
                }
                self.er.dc(1)[mb_x + mb_y * mb_stride] = ((dcu + 4) >> 3) as i16;
                self.er.dc(2)[mb_x + mb_y * mb_stride] = ((dcv + 4) >> 3) as i16;
            }
        }

        // The DC of damaged intra blocks guessed from around them, and the
        // luma's smoothed.
        let mb_type = &self.pics[cur].mb_type[off..];
        self.er
            .guess_dc(mb_type, 0, mb_width * 2, mb_height * 2, b8_stride, true);
        self.er
            .guess_dc(mb_type, 1, mb_width, mb_height, mb_stride, false);
        self.er
            .guess_dc(mb_type, 2, mb_width, mb_height, mb_stride, false);
        filter181(self.er.dc(0), mb_width * 2, mb_height * 2, b8_stride);

        // Damaged intra macroblocks drawn flat at their DC.
        for mb_y in 0..mb_height {
            for mb_x in 0..mb_width {
                let mb_xy = mb_x + mb_y * mb_stride;
                if is_inter(mb_type[mb_xy]) || self.er.error_status_table[mb_xy] & ER_AC_ERROR == 0
                {
                    continue;
                }
                self.er.put_dc(frame, ls, mb_x, mb_y);
            }
        }

        // The edges of damaged blocks smoothed, across then down.
        let mv = &self.pics[cur].motion_val[0];
        let er = &self.er;
        er.h_block_filter(
            mb_type,
            mv,
            frame,
            0,
            mb_width * 2,
            mb_height * 2,
            ls[0],
            true,
        );
        er.v_block_filter(
            mb_type,
            mv,
            frame,
            0,
            mb_width * 2,
            mb_height * 2,
            ls[0],
            true,
        );
        er.h_block_filter(mb_type, mv, frame, 1, mb_width, mb_height, ls[1], false);
        er.h_block_filter(mb_type, mv, frame, 2, mb_width, mb_height, ls[2], false);
        er.v_block_filter(mb_type, mv, frame, 1, mb_width, mb_height, ls[1], false);
        er.v_block_filter(mb_type, mv, frame, 2, mb_width, mb_height, ls[2], false);
        flags
    }

    /// Whether the damaged macroblocks are likelier intra than inter
    /// (`is_intra_more_likely`): judged on an intra picture by how far its
    /// sound macroblocks are from the picture before's against how far that
    /// picture's are from their own neighbours below, on any other by
    /// whether its sound macroblocks are mostly intra.
    fn er_is_intra_more_likely(&self, frame: &AnyFrame, cur: usize, last: Option<usize>) -> bool {
        let Some(last_frame) = last.and_then(|l| self.pics[l].f.as_deref()) else {
            // Nothing before to predict from: spatial prediction.
            return true;
        };
        let er = &self.er;
        let undamaged = (0..er.mb_num)
            .filter(|&i| !dc_and_mv_damaged(er.error_status_table[er.mb_index2xy[i]]))
            .count();
        if undamaged < 5 {
            // Almost all damaged: temporal prediction.
            return false;
        }
        let skip_amount = (undamaged / 50).max(1);
        let off = table_offset(self.pics[cur].mb_stride);
        let intra_picture = self.pics[cur].props.pict_type == TYPE_I;
        let ls = frame.er_linesize(0);
        let mut is_intra_likely = 0;
        let mut j = 0;
        for mb_y in 0..er.mb_height.saturating_sub(1) {
            for mb_x in 0..er.mb_width {
                let mb_xy = mb_x + mb_y * er.mb_stride;
                if dc_and_mv_damaged(er.error_status_table[mb_xy]) {
                    continue;
                }
                j += 1;
                if j % skip_amount != 0 {
                    continue;
                }
                if intra_picture {
                    let at = mb_x * 16 + mb_y * 16 * ls;
                    is_intra_likely += last_frame.er_sad16(at, frame, at, ls);
                    is_intra_likely -= last_frame.er_sad16(at, last_frame, at + ls * 16, ls);
                } else if is_intra(self.pics[cur].mb_type[off + mb_xy]) {
                    is_intra_likely += 1;
                } else {
                    is_intra_likely -= 1;
                }
            }
        }
        is_intra_likely > 0
    }

    /// Vectors guessed for the damaged inter macroblocks of a P picture
    /// (`guess_mv`): from their neighbours' -- each one's, their mean and
    /// median, zero, and its own from the picture before -- whichever
    /// predicts samples that join its neighbours' best, working inward from
    /// the sound macroblocks a ring at a time.
    #[allow(
        clippy::too_many_lines,
        reason = "one routine of FFmpeg's, kept whole to be read against it"
    )]
    fn er_guess_mv(
        &mut self,
        sl: &mut SliceContext,
        frame: &mut AnyFrame,
        cur: usize,
        last: Option<usize>,
        next: Option<usize>,
    ) {
        let (mb_stride, mb_width) = (self.er.mb_stride, self.er.mb_width);
        let mut mb_height = self.er.mb_height;
        for p in [last, next].into_iter().flatten() {
            if let Some(f) = self.pics[p].f.as_deref() {
                mb_height = mb_height.min((f.size().1 + 15) >> 4);
            }
        }
        // H.264's vectors: four a macroblock across and down.
        let (mot_step, mot_stride) = (4, mb_width * 4);
        let off = table_offset(self.pics[cur].mb_stride);
        // The picture before's vectors: for the picture kept for
        // concealment, those of the picture it holds.
        let last_mvs = last.filter(|&l| self.pics[l].in_use() && self.list0_tables(l).is_some());

        let mut fixed = std::mem::take(&mut self.er.fixed);
        let mut num_avail = 0;
        for i in 0..mb_width * mb_height {
            let mb_xy = self.er.mb_index2xy[i];
            let error = self.er.error_status_table[mb_xy];
            let mut f = 0;
            if is_intra(self.pics[cur].mb_type[off + mb_xy]) || error & ER_MV_ERROR == 0 {
                f = MV_FROZEN;
            }
            fixed[mb_xy] = f;
            if f == MV_FROZEN {
                num_avail += 1;
            } else if let Some(l) = last_mvs {
                let (mb_y, mb_x) = (mb_xy / mb_stride, mb_xy % mb_stride);
                let mot_index = (mb_x + mb_y * mot_stride) * mot_step;
                if let Some((mv, r)) = self
                    .list0_tables(l)
                    .map(|(mvs, refs)| (mvs[4 + mot_index], refs[4 * mb_xy]))
                {
                    self.pics[cur].motion_val[0][4 + mot_index] = mv;
                    self.pics[cur].ref_index[0][4 * mb_xy] = r;
                }
            }
        }

        if num_avail <= mb_width.max(mb_height) / 2 {
            // Too little to guess from: zero vectors.
            for mb_y in 0..mb_height {
                for mb_x in 0..mb_width {
                    let mb_xy = mb_x + mb_y * mb_stride;
                    if is_intra(self.pics[cur].mb_type[off + mb_xy])
                        || self.er.error_status_table[mb_xy] & ER_MV_ERROR == 0
                    {
                        continue;
                    }
                    self.er_decode_mb(sl, frame, cur, 0, [0, 0], mb_x, mb_y);
                }
            }
            self.er.fixed = fixed;
            return;
        }

        let mut blocklist = std::mem::take(&mut self.er.blocklist);
        let mut next_blocklist = std::mem::take(&mut self.er.next_blocklist);
        let add = |list: &mut Vec<[usize; 2]>,
                   fixed: &mut [u8],
                   mb_x: usize,
                   mb_y: usize,
                   mb_xy: usize| {
            if fixed[mb_xy] == 0 {
                fixed[mb_xy] = MV_LISTED;
                list.push([mb_x, mb_y]);
            }
        };
        blocklist.clear();
        for mb_y in 0..mb_height {
            for mb_x in 0..mb_width {
                let mb_xy = mb_x + mb_y * mb_stride;
                if fixed[mb_xy] == MV_FROZEN {
                    if mb_x > 0 {
                        add(&mut blocklist, &mut fixed, mb_x - 1, mb_y, mb_xy - 1);
                    }
                    if mb_y > 0 {
                        add(
                            &mut blocklist,
                            &mut fixed,
                            mb_x,
                            mb_y - 1,
                            mb_xy - mb_stride,
                        );
                    }
                    if mb_x + 1 < mb_width {
                        add(&mut blocklist, &mut fixed, mb_x + 1, mb_y, mb_xy + 1);
                    }
                    if mb_y + 1 < mb_height {
                        add(
                            &mut blocklist,
                            &mut fixed,
                            mb_x,
                            mb_y + 1,
                            mb_xy + mb_stride,
                        );
                    }
                }
            }
        }

        loop {
            let mut none_left = true;
            let mut changed = true;
            let mut pass = 0;
            while (changed || pass < 2) && pass < 10 {
                changed = false;
                for &[mb_x, mb_y] in &blocklist {
                    let mb_xy = mb_x + mb_y * mb_stride;
                    if (mb_x ^ mb_y ^ pass) & 1 != 0 {
                        continue;
                    }
                    // The neighbours inside the picture -- left, right, up,
                    // down: each one's macroblock and first vector.
                    let mot_index = (mb_x + mb_y * mot_stride) * mot_step;
                    let neighbours = [
                        (mb_x > 0).then(|| (mb_xy - 1, mot_index - mot_step)),
                        (mb_x + 1 < mb_width).then(|| (mb_xy + 1, mot_index + mot_step)),
                        (mb_y > 0).then(|| (mb_xy - mb_stride, mot_index - mot_stride * mot_step)),
                        (mb_y + 1 < mb_height)
                            .then(|| (mb_xy + mb_stride, mot_index + mot_stride * mot_step)),
                    ];
                    let states = neighbours
                        .iter()
                        .flatten()
                        .fold(0, |s, &(xy, _)| s | fixed[xy]);
                    if states & MV_CHANGED == 0 && pass > 1 {
                        continue;
                    }
                    none_left = false;
                    // Which are settled or already guessed (state above 1).
                    let known = neighbours.map(|n| n.filter(|&(xy, _)| fixed[xy] > 1));
                    let pic = &self.pics[cur];
                    // Their vectors and references, as candidates.
                    let mut mv_predictor = [[0i32; 2]; 8];
                    let mut refs = [0i32; 8];
                    let mut pred_count = 0;
                    for (xy, k) in known.into_iter().flatten() {
                        let mv = pic.motion_val[0][4 + k];
                        mv_predictor[pred_count] = [i32::from(mv[0]), i32::from(mv[1])];
                        refs[pred_count] = i32::from(pic.ref_index[0][4 * xy]);
                        pred_count += 1;
                    }
                    if pred_count == 0 {
                        continue;
                    }

                    if pred_count > 1 {
                        // Their mean and median, unless their references
                        // differ.
                        let (mut sum_x, mut sum_y, mut sum_r) = (0, 0, 0);
                        let mut same_refs = true;
                        let mut j = 0;
                        while j < pred_count {
                            sum_x += mv_predictor[j][0];
                            sum_y += mv_predictor[j][1];
                            sum_r += refs[j];
                            if j > 0 && refs[j] != refs[j - 1] {
                                same_refs = false;
                                break;
                            }
                            j += 1;
                        }
                        if same_refs {
                            let n = j as i32;
                            mv_predictor[pred_count] = [sum_x / n, sum_y / n];
                            refs[pred_count] = sum_r / n;
                            let (mut min_x, mut min_y, mut min_r, mut max_x, mut max_y, mut max_r) =
                                if pred_count >= 3 {
                                    (99999, 99999, 99999, -99999, -99999, -99999)
                                } else {
                                    (0, 0, 0, 0, 0, 0)
                                };
                            for k in 0..pred_count {
                                max_x = max_x.max(mv_predictor[k][0]);
                                max_y = max_y.max(mv_predictor[k][1]);
                                max_r = max_r.max(refs[k]);
                                min_x = min_x.min(mv_predictor[k][0]);
                                min_y = min_y.min(mv_predictor[k][1]);
                                min_r = min_r.min(refs[k]);
                            }
                            let mut median = [sum_x - max_x - min_x, sum_y - max_y - min_y];
                            let mut median_r = sum_r - max_r - min_r;
                            if pred_count == 4 {
                                median = [median[0] / 2, median[1] / 2];
                                median_r /= 2;
                            }
                            mv_predictor[pred_count + 1] = median;
                            refs[pred_count + 1] = median_r;
                            pred_count += 2;
                        }
                    }
                    // Zero, and its own vector as it stands.
                    mv_predictor[pred_count] = [0, 0];
                    refs[pred_count] = 0;
                    pred_count += 1;
                    let prev = pic.motion_val[0][4 + mot_index];
                    let (prev_x, prev_y) = (i32::from(prev[0]), i32::from(prev[1]));
                    let prev_ref = i32::from(pic.ref_index[0][4 * mb_xy]);
                    mv_predictor[pred_count] = [prev_x, prev_y];
                    refs[pred_count] = prev_ref;
                    pred_count += 1;

                    // Each predicted, and scored by how its edges join the
                    // settled neighbours'; ties to the later.
                    let ls = frame.er_linesize(0);
                    let src = mb_x * 16 + mb_y * 16 * ls;
                    let mut best_pred = 0;
                    let mut best_score = 256 * 256 * 256 * 64;
                    for j in 0..pred_count {
                        let mv = mv_predictor[j];
                        self.pics[cur].motion_val[0][4 + mot_index] = [mv[0] as i16, mv[1] as i16];
                        if refs[j] < 0 {
                            // A predictor from an intra or unavailable block.
                            continue;
                        }
                        self.er_decode_mb(sl, frame, cur, refs[j], mv, mb_x, mb_y);
                        let b = |i: usize| frame.er_byte(0, i);
                        let mut score = 0;
                        if known[0].is_some() {
                            for k in 0..16 {
                                score += (b(src + k * ls - 1) - b(src + k * ls)).abs();
                            }
                        }
                        if known[1].is_some() {
                            for k in 0..16 {
                                score += (b(src + k * ls + 15) - b(src + k * ls + 16)).abs();
                            }
                        }
                        if known[2].is_some() {
                            for k in 0..16 {
                                score += (b(src + k - ls) - b(src + k)).abs();
                            }
                        }
                        if known[3].is_some() {
                            for k in 0..16 {
                                score += (b(src + k + ls * 15) - b(src + k + ls * 16)).abs();
                            }
                        }
                        if score <= best_score {
                            best_score = score;
                            best_pred = j;
                        }
                    }
                    let best = mv_predictor[best_pred];
                    let stored = [best[0] as i16, best[1] as i16];
                    for i in 0..mot_step {
                        for j in 0..mot_step {
                            self.pics[cur].motion_val[0][4 + mot_index + i + j * mot_stride] =
                                stored;
                        }
                    }
                    self.er_decode_mb(sl, frame, cur, refs[best_pred], best, mb_x, mb_y);
                    if best[0] != prev_x || best[1] != prev_y {
                        fixed[mb_xy] = MV_CHANGED;
                        changed = true;
                    } else {
                        fixed[mb_xy] = MV_UNCHANGED;
                    }
                }
                pass += 1;
            }

            if none_left {
                break;
            }
            // The next ring: the neighbours of this one, which is settled.
            next_blocklist.clear();
            for &[mb_x, mb_y] in &blocklist {
                let mb_xy = mb_x + mb_y * mb_stride;
                if fixed[mb_xy] & (MV_CHANGED | MV_UNCHANGED | MV_FROZEN) != 0 {
                    fixed[mb_xy] = MV_FROZEN;
                    if mb_x > 0 {
                        add(&mut next_blocklist, &mut fixed, mb_x - 1, mb_y, mb_xy - 1);
                    }
                    if mb_y > 0 {
                        add(
                            &mut next_blocklist,
                            &mut fixed,
                            mb_x,
                            mb_y - 1,
                            mb_xy - mb_stride,
                        );
                    }
                    if mb_x + 1 < mb_width {
                        add(&mut next_blocklist, &mut fixed, mb_x + 1, mb_y, mb_xy + 1);
                    }
                    if mb_y + 1 < mb_height {
                        add(
                            &mut next_blocklist,
                            &mut fixed,
                            mb_x,
                            mb_y + 1,
                            mb_xy + mb_stride,
                        );
                    }
                }
            }
            std::mem::swap(&mut blocklist, &mut next_blocklist);
        }
        self.er.fixed = fixed;
        self.er.blocklist = blocklist;
        self.er.next_blocklist = next_blocklist;
    }

    /// Macroblock `mb_x,mb_y` predicted from list 0's reference `r` with
    /// vector `mv` and no residual (`h264_er_decode_mb`), through the
    /// slice context the last slice left -- its other state as that left
    /// it, as FFmpeg's is.
    #[allow(
        clippy::too_many_arguments,
        reason = "FFmpeg's callback, which takes the macroblock and its prediction"
    )]
    fn er_decode_mb(
        &mut self,
        sl: &mut SliceContext,
        frame: &mut AnyFrame,
        cur: usize,
        r: i32,
        mv: [i32; 2],
        mb_x: usize,
        mb_y: usize,
    ) {
        sl.mb_x = mb_x as i32;
        sl.mb_y = mb_y as i32;
        sl.mb_xy = (mb_x + mb_y * self.mb_stride as usize) as i32;
        sl.non_zero_count_cache = [0; 120];
        // References may differ from slice to slice; FFmpeg takes the last
        // slice's, and the first where one is out of its list or empty.
        let mut r = r as usize;
        if r >= sl.hdr.ref_count[0] as usize {
            r = 0;
        }
        let has_data = |e: &Ref| e.parent.is_some_and(|p| self.pics[p].in_use());
        if !has_data(&sl.ref_list[0][r]) {
            r = 0;
        }
        if sl.ref_list[0][r].reference & 3 != 3 {
            // "Reference invalid".
            return;
        }
        let at = 4 * sl.mb_xy as usize;
        self.pics[cur].ref_index[0][at..at + 4].fill(r as i8);
        let packed = [mv[0] as i16, mv[1] as i16];
        for y in 0..4 {
            for x in 0..4 {
                sl.ref_cache[0][SCAN8[0] + x + 8 * y] = r as i8;
                sl.mv_cache[0][SCAN8[0] + x + 8 * y] = packed;
            }
        }
        sl.hdr.mb_mbaff = 0;
        sl.hdr.mb_field_decoding_flag = 0;
        // The macroblock is inter: no I_PCM samples to read, and nothing
        // else for the reconstruction to refuse.
        let _ = self.hl_decode_mb(sl, frame, &[]);
    }
}

impl Er {
    /// The DC of each damaged intra block of plane `p` guessed from the
    /// nearest sound block in each direction, nearer ones weighing more
    /// (`guess_dc`). `w` and `h` are in blocks -- 8x8 luma blocks, or
    /// macroblocks for chroma -- `stride` the DC plane's row stride.
    fn guess_dc(
        &mut self,
        mb_type: &[u32],
        p: usize,
        w: usize,
        h: usize,
        stride: usize,
        is_luma: bool,
    ) {
        let shift = usize::from(is_luma);
        let mut col = vec![[0i16; 4]; stride * h];
        let mut dist = vec![[0u32; 4]; stride * h];
        let sound = |er: &Self, b_x: usize, b_y: usize| {
            let mb_index = (b_x >> shift) + (b_y >> shift) * er.mb_stride;
            !is_intra(mb_type[mb_index]) || er.error_status_table[mb_index] & ER_DC_ERROR == 0
        };
        let dc_plane = self.dc_val[p];
        let dc = |er: &Self, at: usize| er.dc_val_base[dc_plane + at];
        for b_y in 0..h {
            let (mut color, mut distance) = (1024i16, -1i64);
            for b_x in 0..w {
                if sound(self, b_x, b_y) {
                    color = dc(self, b_x + b_y * stride);
                    distance = b_x as i64;
                }
                col[b_x + b_y * stride][1] = color;
                dist[b_x + b_y * stride][1] = if distance >= 0 {
                    (b_x as i64 - distance) as u32
                } else {
                    9999
                };
            }
            let (mut color, mut distance) = (1024i16, -1i64);
            for b_x in (0..w).rev() {
                if sound(self, b_x, b_y) {
                    color = dc(self, b_x + b_y * stride);
                    distance = b_x as i64;
                }
                col[b_x + b_y * stride][0] = color;
                dist[b_x + b_y * stride][0] = if distance >= 0 {
                    (distance - b_x as i64) as u32
                } else {
                    9999
                };
            }
        }
        for b_x in 0..w {
            let (mut color, mut distance) = (1024i16, -1i64);
            for b_y in 0..h {
                if sound(self, b_x, b_y) {
                    color = dc(self, b_x + b_y * stride);
                    distance = b_y as i64;
                }
                col[b_x + b_y * stride][3] = color;
                dist[b_x + b_y * stride][3] = if distance >= 0 {
                    (b_y as i64 - distance) as u32
                } else {
                    9999
                };
            }
            let (mut color, mut distance) = (1024i16, -1i64);
            for b_y in (0..h).rev() {
                if sound(self, b_x, b_y) {
                    color = dc(self, b_x + b_y * stride);
                    distance = b_y as i64;
                }
                col[b_x + b_y * stride][2] = color;
                dist[b_x + b_y * stride][2] = if distance >= 0 {
                    (distance - b_y as i64) as u32
                } else {
                    9999
                };
            }
        }
        for b_y in 0..h {
            for b_x in 0..w {
                let mb_index = (b_x >> shift) + (b_y >> shift) * self.mb_stride;
                if is_inter(mb_type[mb_index])
                    || self.error_status_table[mb_index] & ER_DC_ERROR == 0
                {
                    continue;
                }
                let (mut guess, mut weight_sum) = (0i64, 0i64);
                for j in 0..4 {
                    let weight =
                        256 * 256 * 256 * 16 / i64::from(dist[b_x + b_y * stride][j].max(1));
                    guess += weight * i64::from(col[b_x + b_y * stride][j]);
                    weight_sum += weight;
                }
                guess = (guess + weight_sum / 2) / weight_sum;
                self.dc_val_base[dc_plane + b_x + b_y * stride] = guess as i16;
            }
        }
    }

    /// Macroblock `mb_x,mb_y` drawn flat: each 8x8 luma block at its DC,
    /// and an 8x8 block of each chroma plane at theirs (`put_dc`).
    fn put_dc(&mut self, frame: &mut AnyFrame, ls: [usize; 3], mb_x: usize, mb_y: usize) {
        let dest_y = mb_x * 16 + mb_y * 16 * ls[0];
        let dest_c = [mb_x * 8 + mb_y * 8 * ls[1], mb_x * 8 + mb_y * 8 * ls[2]];
        let b8_stride = self.b8_stride;
        for i in 0..4 {
            let dc = i32::from(self.dc(0)[mb_x * 2 + (i & 1) + (mb_y * 2 + (i >> 1)) * b8_stride])
                .clamp(0, 2040);
            for y in 0..8 {
                for x in 0..8 {
                    let at = dest_y + x + (i & 1) * 8 + (y + (i >> 1) * 8) * ls[0];
                    frame.er_set_byte(0, at, (dc / 8) as u8);
                }
            }
        }
        let mb_stride = self.mb_stride;
        let dcu = i32::from(self.dc(1)[mb_x + mb_y * mb_stride]).clamp(0, 2040);
        let dcv = i32::from(self.dc(2)[mb_x + mb_y * mb_stride]).clamp(0, 2040);
        for y in 0..8 {
            for x in 0..8 {
                frame.er_set_byte(1, dest_c[0] + x + y * ls[1], (dcu / 8) as u8);
                frame.er_set_byte(2, dest_c[1] + x + y * ls[2], (dcv / 8) as u8);
            }
        }
    }

    /// The vertical edges between 8x8 blocks of plane `p` smoothed where
    /// either side is damaged, unless both are inter with near vectors
    /// (`h_block_filter`). `w` and `h` are in blocks, `stride` in bytes.
    #[allow(
        clippy::too_many_arguments,
        reason = "FFmpeg's filter, which takes the plane and its geometry"
    )]
    fn h_block_filter(
        &self,
        mb_type: &[u32],
        motion_val: &[[i16; 2]],
        frame: &mut AnyFrame,
        p: usize,
        w: usize,
        h: usize,
        stride: usize,
        is_luma: bool,
    ) {
        let shift = usize::from(is_luma);
        let mvx_stride = 4 >> shift;
        let mvy_stride = self.mb_width * 4 * mvx_stride;
        for b_y in 0..h {
            for b_x in 0..w.saturating_sub(1) {
                let left = (b_x >> shift) + (b_y >> shift) * self.mb_stride;
                let right = ((b_x + 1) >> shift) + (b_y >> shift) * self.mb_stride;
                let left_damage = self.error_status_table[left] & ER_MB_ERROR != 0;
                let right_damage = self.error_status_table[right] & ER_MB_ERROR != 0;
                if !(left_damage || right_damage) {
                    continue;
                }
                let left_mv = motion_val[4 + mvy_stride * b_y + mvx_stride * b_x];
                let right_mv = motion_val[4 + mvy_stride * b_y + mvx_stride * (b_x + 1)];
                if !is_intra(mb_type[left])
                    && !is_intra(mb_type[right])
                    && (i32::from(left_mv[0]) - i32::from(right_mv[0])).abs()
                        + (i32::from(left_mv[1]) + i32::from(right_mv[1])).abs()
                        < 2
                {
                    continue;
                }
                let offset = b_x * 8 + b_y * stride * 8;
                for y in 0..8 {
                    let at = |k: usize| offset + k + y * stride;
                    edge(
                        frame,
                        p,
                        [at(4), at(5), at(6), at(7), at(8), at(9), at(10), at(11)],
                        [left_damage, right_damage],
                    );
                }
            }
        }
    }

    /// The horizontal edges, the same way (`v_block_filter`).
    #[allow(
        clippy::too_many_arguments,
        reason = "FFmpeg's filter, which takes the plane and its geometry"
    )]
    fn v_block_filter(
        &self,
        mb_type: &[u32],
        motion_val: &[[i16; 2]],
        frame: &mut AnyFrame,
        p: usize,
        w: usize,
        h: usize,
        stride: usize,
        is_luma: bool,
    ) {
        let shift = usize::from(is_luma);
        let mvx_stride = 4 >> shift;
        let mvy_stride = self.mb_width * 4 * mvx_stride;
        for b_y in 0..h.saturating_sub(1) {
            for b_x in 0..w {
                let top = (b_x >> shift) + (b_y >> shift) * self.mb_stride;
                let bottom = (b_x >> shift) + ((b_y + 1) >> shift) * self.mb_stride;
                let top_damage = self.error_status_table[top] & ER_MB_ERROR != 0;
                let bottom_damage = self.error_status_table[bottom] & ER_MB_ERROR != 0;
                if !(top_damage || bottom_damage) {
                    continue;
                }
                let top_mv = motion_val[4 + mvy_stride * b_y + mvx_stride * b_x];
                let bottom_mv = motion_val[4 + mvy_stride * (b_y + 1) + mvx_stride * b_x];
                if !is_intra(mb_type[top])
                    && !is_intra(mb_type[bottom])
                    && (i32::from(top_mv[0]) - i32::from(bottom_mv[0])).abs()
                        + (i32::from(top_mv[1]) + i32::from(bottom_mv[1])).abs()
                        < 2
                {
                    continue;
                }
                let offset = b_x * 8 + b_y * stride * 8;
                for x in 0..8 {
                    let at = |k: usize| offset + x + k * stride;
                    edge(
                        frame,
                        p,
                        [at(4), at(5), at(6), at(7), at(8), at(9), at(10), at(11)],
                        [top_damage, bottom_damage],
                    );
                }
            }
        }
    }
}

/// One line across an edge of plane `p` smoothed: `at` are the bytes four
/// before it to four after, `damage` whether each side is damaged. The step
/// at the edge beyond what its neighbours step is spread over the damaged
/// side's four -- over a sound side too, more gently, when only one is.
fn edge(frame: &mut AnyFrame, p: usize, at: [usize; 8], damage: [bool; 2]) {
    // Each byte written is computed from its own value alone, so all are
    // read first.
    let px = at.map(|i| frame.er_byte(p, i));
    let a = px[3] - px[2];
    let b = px[4] - px[3];
    let c = px[5] - px[4];
    let mut d = (b.abs() - ((a.abs() + c.abs() + 1) >> 1)).max(0);
    if b < 0 {
        d = -d;
    }
    if d == 0 {
        return;
    }
    if !(damage[0] && damage[1]) {
        d = d * 16 / 9;
    }
    if damage[0] {
        for (k, m) in [(3, 7), (2, 5), (1, 3), (0, 1)] {
            frame.er_set_byte(p, at[k], clip_u8(px[k] + ((d * m) >> 4)));
        }
    }
    if damage[1] {
        for (k, m) in [(4, 7), (5, 5), (6, 3), (7, 1)] {
            frame.er_set_byte(p, at[k], clip_u8(px[k] - ((d * m) >> 4)));
        }
    }
}

/// The luma DC plane smoothed, rows then columns, its border left alone
/// (`filter181`): each value from itself and its two neighbours, the
/// earlier one as it was before this pass changed it.
fn filter181(data: &mut [i16], width: usize, height: usize, stride: usize) {
    let f = |prev: i32, v: i32, next: i32| {
        let dc = -prev + v * 8 - next;
        ((i64::from(dc.clamp(i32::MIN / 10923, i32::MAX / 10923 - 32768)) * 10923 + 32768) >> 16)
            as i16
    };
    for y in 1..height.saturating_sub(1) {
        let mut prev = i32::from(data[y * stride]);
        for x in 1..width.saturating_sub(1) {
            let at = x + y * stride;
            let dc = f(prev, i32::from(data[at]), i32::from(data[at + 1]));
            prev = i32::from(data[at]);
            data[at] = dc;
        }
    }
    for x in 1..width.saturating_sub(1) {
        let mut prev = i32::from(data[x]);
        for y in 1..height.saturating_sub(1) {
            let at = x + y * stride;
            let dc = f(prev, i32::from(data[at]), i32::from(data[at + stride]));
            prev = i32::from(data[at]);
            data[at] = dc;
        }
    }
}

#[cfg(test)]
mod tests;
