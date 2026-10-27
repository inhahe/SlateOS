//! A macroblock's neighbours and the caches decoding it reads, and motion
//! vector prediction (`h264_mvpred.h`).
//!
//! Before a macroblock is decoded FFmpeg finds its neighbours -- above,
//! left, above-left, above-right, which in an MBAFF frame depend on whether
//! this pair and theirs are fields -- and copies what it needs of them into
//! small `scan8`-laid caches: their intra modes, coefficient counts, motion
//! vectors and reference indices. Prediction then reads only the caches,
//! and what the macroblock decodes is written back to the picture's tables
//! when it is done.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on macroblock addresses and vectors; vectors wrap as int16"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "cache indices are scan8's, under 5 * 8 or 15 * 8; table indices are macroblock \
              addresses FFmpeg reads only where the neighbour is available, or into the tables' \
              margin above the picture"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int, int8 and int16 conversions, made where it makes them"
)]

use crate::decoder::{Context, SliceContext};
use crate::mbtype::{
    MB_TYPE_8X8, MB_TYPE_8X16, MB_TYPE_16X8, MB_TYPE_16X16, MB_TYPE_DIRECT2, MB_TYPE_INTERLACED,
    MB_TYPE_L0L1, MB_TYPE_P0L0, MB_TYPE_P1L0, MB_TYPE_SKIP, SCAN8, is_8x8, is_8x8dct, is_direct,
    is_inter, is_interlaced, is_intra, is_intra4x4, is_skip, uses_list,
};
use crate::picture::table_offset;
use crate::slice::TYPE_B;

/// A partition no reference may come from: off the picture or the slice
/// (`PART_NOT_AVAILABLE`).
pub(crate) const PART_NOT_AVAILABLE: i8 = -2;
/// A neighbour that does not use the list (`LIST_NOT_USED`).
pub(crate) const LIST_NOT_USED: i8 = -1;

/// `DC_PRED`, what a 4x4 block predicts from when a neighbour's mode is
/// unavailable.
pub(crate) const DC_PRED: i32 = 2;

/// Which of the left neighbour's 4x4 blocks border this macroblock's rows,
/// by how the pair and its left pair are fields or frames, and where their
/// coefficient counts are (`left_block_options`).
#[rustfmt::skip]
pub(crate) const LEFT_BLOCK_OPTIONS: [[u8; 16]; 4] = [
    [0, 1, 2, 3, 7, 10, 8, 11, 3, 3 + 4, 3 + 2 * 4, 3 + 3 * 4, 1 + 4 * 4, 1 + 8 * 4, 1 + 5 * 4, 1 + 9 * 4],
    [2, 2, 3, 3, 8, 11, 8, 11, 3 + 2 * 4, 3 + 2 * 4, 3 + 3 * 4, 3 + 3 * 4, 1 + 5 * 4, 1 + 9 * 4, 1 + 5 * 4, 1 + 9 * 4],
    [0, 0, 1, 1, 7, 10, 7, 10, 3, 3, 3 + 4, 3 + 4, 1 + 4 * 4, 1 + 8 * 4, 1 + 4 * 4, 1 + 8 * 4],
    [0, 2, 0, 2, 7, 10, 7, 10, 3, 3 + 2 * 4, 3, 3 + 2 * 4, 1 + 4 * 4, 1 + 8 * 4, 1 + 4 * 4, 1 + 8 * 4],
];

/// The median of three (`mid_pred`).
pub(crate) fn mid_pred(a: i32, b: i32, c: i32) -> i32 {
    let mut b = b;
    if a > b {
        if c > b {
            b = if c > a { a } else { c };
        }
    } else if b > c {
        b = if c > a { c } else { a };
    }
    b
}

/// A `w` by `h` block of `cache`, rows 8 apart from `at`, set to `v`
/// (`fill_rectangle` on a cache).
pub(crate) fn fill<T: Copy>(cache: &mut [T], at: usize, w: usize, h: usize, v: T) {
    for row in 0..h {
        let start = at + row * 8;
        cache[start..start + w].fill(v);
    }
}

/// Where in a picture's per-macroblock tables macroblock `mb_xy` is, the
/// tables starting two rows and a macroblock in.
pub(crate) fn tab(off: usize, mb_xy: i32) -> usize {
    (off as isize + mb_xy as isize) as usize
}

impl Context {
    /// The current picture's slot. Present while a slice is decoded.
    fn cur(&self) -> usize {
        self.cur_pic_ptr.unwrap_or(0)
    }

    /// Macroblock `mb_xy`'s type in the current picture.
    pub(crate) fn cur_mb_type(&self, mb_xy: i32) -> u32 {
        let pic = &self.pics[self.cur()];
        pic.mb_type[tab(table_offset(pic.mb_stride), mb_xy)]
    }

    /// The predicted intra 4x4 mode of block `n`: the lower of the left and
    /// top blocks', DC where either is unavailable (`pred_intra_mode`).
    pub(crate) fn pred_intra_mode(sl: &SliceContext, n: usize) -> i32 {
        let index8 = SCAN8[n];
        let left = i32::from(sl.intra4x4_pred_mode_cache[index8 - 1]);
        let top = i32::from(sl.intra4x4_pred_mode_cache[index8 - 8]);
        let min = left.min(top);
        if min < 0 { DC_PRED } else { min }
    }

    /// The macroblock's bottom row and right column of 4x4 modes kept for
    /// the macroblocks below and right (`write_back_intra_pred_mode`).
    pub(crate) fn write_back_intra_pred_mode(&mut self, sl: &SliceContext) {
        let at = self.mb2br_xy[sl.mb_xy as usize] as usize;
        let c = &sl.intra4x4_pred_mode_cache;
        let i4x4 = &mut self.intra4x4_pred_mode[at..at + 7];
        i4x4[..4].copy_from_slice(&c[4 + 8 * 4..8 + 8 * 4]);
        i4x4[4] = c[7 + 8 * 3];
        i4x4[5] = c[7 + 8 * 2];
        i4x4[6] = c[7 + 8];
    }

    /// The macroblock's coefficient counts kept for the loop filter and the
    /// macroblocks after it (`write_back_non_zero_count`).
    pub(crate) fn write_back_non_zero_count(&mut self, sl: &SliceContext) {
        let c = &sl.non_zero_count_cache;
        let nnz = &mut self.non_zero_count[sl.mb_xy as usize];
        for (dst, row) in [
            (0, 1),
            (4, 2),
            (8, 3),
            (12, 4),
            (16, 6),
            (20, 7),
            (32, 11),
            (36, 12),
        ] {
            nnz[dst..dst + 4].copy_from_slice(&c[4 + 8 * row..8 + 8 * row]);
        }
        if self.chroma_y_shift == 0 {
            for (dst, row) in [(24, 8), (28, 9), (40, 13), (44, 14)] {
                nnz[dst..dst + 4].copy_from_slice(&c[4 + 8 * row..8 + 8 * row]);
            }
        }
    }

    /// One list's vectors and references -- and for CABAC its vector
    /// differences -- written to the picture's tables
    /// (`write_back_motion_list`).
    fn write_back_motion_list(
        &mut self,
        sl: &SliceContext,
        b_xy: usize,
        b8_xy: usize,
        mb_type: u32,
        list: usize,
        cabac: bool,
    ) {
        let b_stride = self.b_stride as usize;
        let cur = self.cur();
        let mv = &mut self.pics[cur].motion_val[list];
        for row in 0..4 {
            let src = SCAN8[0] + 8 * row;
            let dst = 4 + b_xy + row * b_stride;
            mv[dst..dst + 4].copy_from_slice(&sl.mv_cache[list][src..src + 4]);
        }
        if cabac {
            let at = self.mb2br_xy[sl.mb_xy as usize] as usize;
            let mvd = &mut self.mvd_table[list][at..at + 8];
            if is_skip(mb_type) {
                mvd.fill([0, 0]);
            } else {
                let c = &sl.mvd_cache[list];
                mvd[..4].copy_from_slice(&c[SCAN8[0] + 8 * 3..SCAN8[0] + 8 * 3 + 4]);
                mvd[6] = c[SCAN8[0] + 3];
                mvd[5] = c[SCAN8[0] + 3 + 8];
                mvd[4] = c[SCAN8[0] + 3 + 16];
            }
        }
        let r = &mut self.pics[cur].ref_index[list];
        let c = &sl.ref_cache[list];
        r[b8_xy] = c[SCAN8[0]];
        r[b8_xy + 1] = c[SCAN8[4]];
        r[b8_xy + 2] = c[SCAN8[8]];
        r[b8_xy + 3] = c[SCAN8[12]];
    }

    /// The macroblock's motion written to the picture's tables
    /// (`write_back_motion`).
    pub(crate) fn write_back_motion(&mut self, sl: &SliceContext, mb_type: u32, cabac: bool) {
        let b_stride = self.b_stride as usize;
        let b_xy = 4 * sl.mb_x as usize + 4 * sl.mb_y as usize * b_stride;
        let b8_xy = 4 * sl.mb_xy as usize;
        if uses_list(mb_type, 0) {
            self.write_back_motion_list(sl, b_xy, b8_xy, mb_type, 0, cabac);
        } else {
            let cur = self.cur();
            self.pics[cur].ref_index[0][b8_xy..b8_xy + 4].fill(LIST_NOT_USED);
        }
        if uses_list(mb_type, 1) {
            self.write_back_motion_list(sl, b_xy, b8_xy, mb_type, 1, cabac);
        }
        if sl.hdr.slice_type_nos == TYPE_B && cabac && is_8x8(mb_type) {
            let d = &mut self.direct_table[4 * sl.mb_xy as usize..4 * sl.mb_xy as usize + 4];
            d[1] = (sl.sub_mb_type[1] >> 1) as u8;
            d[2] = (sl.sub_mb_type[2] >> 1) as u8;
            d[3] = (sl.sub_mb_type[3] >> 1) as u8;
        }
    }

    /// Whether the 8x8 transform may go with this 8x8 macroblock's
    /// partitions: none smaller than 8x8, nor direct where 8x8 inference is
    /// off (`get_dct8x8_allowed`).
    pub(crate) fn get_dct8x8_allowed(&self, sl: &SliceContext) -> bool {
        let inference = self
            .ps
            .sps
            .as_ref()
            .is_some_and(|s| s.direct_8x8_inference_flag != 0);
        let mut mask = MB_TYPE_16X8 | MB_TYPE_8X16 | MB_TYPE_8X8;
        if !inference {
            mask |= MB_TYPE_DIRECT2;
        }
        sl.sub_mb_type.iter().all(|&t| u32::from(t) & mask == 0)
    }

    /// The above-right neighbour's reference and where its vector is in the
    /// cache, or the above-left's where it is unavailable
    /// (`fetch_diagonal_mv`). In an MBAFF frame the left pair's vector
    /// stands in for some, scaled between frame and field and put in a
    /// spare cache entry.
    fn fetch_diagonal_mv(
        &self,
        sl: &mut SliceContext,
        i: usize,
        list: usize,
        part_width: usize,
    ) -> (i32, usize) {
        let unavailable = i32::from(PART_NOT_AVAILABLE);
        let topright_ref = i32::from(sl.ref_cache[list][i - 8 + part_width]);
        if self.mb_aff_frame
            && topright_ref == unavailable
            && i >= SCAN8[0] + 8
            && i & 7 == 4
            && sl.ref_cache[list][SCAN8[0] - 1] != PART_NOT_AVAILABLE
        {
            let spare = SCAN8[0] - 2;
            sl.mv_cache[list][spare] = [0, 0];
            let mb_field = sl.hdr.mb_field_decoding_flag != 0;
            let left_interlaced = is_interlaced(sl.left_type[0]);
            if !mb_field && left_interlaced {
                let xy = sl.left_mb_xy[0] + self.mb_stride;
                let y4 = (sl.mb_y & 1) * 2 + (i as i32 >> 5);
                return (self.diag_mv(sl, list, xy, y4, true), spare);
            }
            if mb_field && !left_interlaced {
                let xy = sl.left_mb_xy[usize::from(i >= 36)];
                let y4 = (i as i32 >> 2) & 3;
                return (self.diag_mv(sl, list, xy, y4, false), spare);
            }
        }
        if topright_ref != unavailable {
            (topright_ref, i - 8 + part_width)
        } else {
            (i32::from(sl.ref_cache[list][i - 8 - 1]), i - 8 - 1)
        }
    }

    /// The left pair's vector at `y4` of macroblock `xy` into the spare
    /// cache entry, and its reference, scaled from field to frame or back
    /// (`SET_DIAG_MV`).
    fn diag_mv(
        &self,
        sl: &mut SliceContext,
        list: usize,
        xy: i32,
        y4: i32,
        field_to_frame: bool,
    ) -> i32 {
        let pic = &self.pics[self.cur()];
        let off = table_offset(pic.mb_stride);
        let mb_type = pic.mb_type[tab(off, xy + (y4 >> 2) * self.mb_stride)];
        if !uses_list(mb_type, list as u32) {
            return i32::from(LIST_NOT_USED);
        }
        let mv = pic.motion_val[list]
            [(4 + self.mb2b_xy[xy as usize] as i32 + 3 + y4 * self.b_stride) as usize];
        let r = i32::from(pic.ref_index[list][(4 * xy + 1 + (y4 & !1)) as usize]);
        let spare = SCAN8[0] - 2;
        sl.mv_cache[list][spare][0] = mv[0];
        if field_to_frame {
            sl.mv_cache[list][spare][1] = (i32::from(mv[1]) * 2) as i16;
            r >> 1
        } else {
            sl.mv_cache[list][spare][1] = (i32::from(mv[1]) / 2) as i16;
            r * 2
        }
    }

    /// The predicted vector of the partition at block `n`, `part_width`
    /// blocks wide, predicting from `ref_` in `list`: the median of the
    /// left, top and diagonal neighbours, or the one neighbour of the same
    /// reference (`pred_motion`).
    pub(crate) fn pred_motion(
        &self,
        sl: &mut SliceContext,
        n: usize,
        part_width: usize,
        list: usize,
        ref_: i32,
    ) -> (i32, i32) {
        let index8 = SCAN8[n];
        let top_ref = i32::from(sl.ref_cache[list][index8 - 8]);
        let left_ref = i32::from(sl.ref_cache[list][index8 - 1]);
        let (diagonal_ref, c) = self.fetch_diagonal_mv(sl, index8, list, part_width);
        let a = sl.mv_cache[list][index8 - 1].map(i32::from);
        let b = sl.mv_cache[list][index8 - 8].map(i32::from);
        let c = sl.mv_cache[list][c].map(i32::from);
        let match_count = i32::from(diagonal_ref == ref_)
            + i32::from(top_ref == ref_)
            + i32::from(left_ref == ref_);
        let unavailable = i32::from(PART_NOT_AVAILABLE);
        let median = || (mid_pred(a[0], b[0], c[0]), mid_pred(a[1], b[1], c[1]));
        if match_count > 1 {
            median()
        } else if match_count == 1 {
            if left_ref == ref_ {
                (a[0], a[1])
            } else if top_ref == ref_ {
                (b[0], b[1])
            } else {
                (c[0], c[1])
            }
        } else if top_ref == unavailable && diagonal_ref == unavailable && left_ref != unavailable {
            (a[0], a[1])
        } else {
            median()
        }
    }

    /// A 16x8 partition's predicted vector: the top one's from above, the
    /// bottom one's from the left, where they use the same reference
    /// (`pred_16x8_motion`).
    pub(crate) fn pred_16x8_motion(
        &self,
        sl: &mut SliceContext,
        n: usize,
        list: usize,
        ref_: i32,
    ) -> (i32, i32) {
        let neighbour = if n == 0 { SCAN8[0] - 8 } else { SCAN8[8] - 1 };
        if i32::from(sl.ref_cache[list][neighbour]) == ref_ {
            let mv = sl.mv_cache[list][neighbour];
            return (i32::from(mv[0]), i32::from(mv[1]));
        }
        self.pred_motion(sl, n, 4, list, ref_)
    }

    /// An 8x16 partition's predicted vector: the left one's from the left,
    /// the right one's from the diagonal, where they use the same reference
    /// (`pred_8x16_motion`).
    pub(crate) fn pred_8x16_motion(
        &self,
        sl: &mut SliceContext,
        n: usize,
        list: usize,
        ref_: i32,
    ) -> (i32, i32) {
        if n == 0 {
            let left = SCAN8[0] - 1;
            if i32::from(sl.ref_cache[list][left]) == ref_ {
                let mv = sl.mv_cache[list][left];
                return (i32::from(mv[0]), i32::from(mv[1]));
            }
        } else {
            let (diagonal_ref, c) = self.fetch_diagonal_mv(sl, SCAN8[4], list, 2);
            if diagonal_ref == ref_ {
                let mv = sl.mv_cache[list][c];
                return (i32::from(mv[0]), i32::from(mv[1]));
            }
        }
        self.pred_motion(sl, n, 2, list, ref_)
    }

    /// A neighbour's list 0 reference and vector for a P_SKIP prediction,
    /// scaled between frame and field in an MBAFF frame (`FIX_MV_MBAFF`).
    fn fix_mv_mbaff(&self, sl: &SliceContext, kind: u32, r: i32, mv: [i16; 2]) -> (i32, [i32; 2]) {
        let mut r = r;
        let mut mv = mv;
        if self.mb_aff_frame {
            if sl.hdr.mb_field_decoding_flag != 0 {
                if !is_interlaced(kind) {
                    r <<= 1;
                    mv[1] /= 2;
                }
            } else if is_interlaced(kind) {
                r >>= 1;
                mv[1] = mv[1].wrapping_mul(2);
            }
        }
        (r, mv.map(i32::from))
    }

    /// A P_SKIP macroblock's vector: none where the left or top neighbour
    /// is unavailable or uses reference 0 with no motion, else the usual
    /// prediction (`pred_pskip_motion`).
    pub(crate) fn pred_pskip_motion(&self, sl: &mut SliceContext) {
        let pic = &self.pics[self.cur()];
        let refs = &pic.ref_index[0];
        let mv = &pic.motion_val[0];
        let b_stride = self.b_stride;
        let left_block = &LEFT_BLOCK_OPTIONS[sl.left_block];
        fill(&mut sl.ref_cache[0], SCAN8[0], 4, 4, 0);

        let zero = |sl: &mut SliceContext| fill(&mut sl.mv_cache[0], SCAN8[0], 4, 4, [0i16, 0]);
        let at_mv = |b: i32| mv[(4 + b) as usize];

        let (left_ref, a) = if uses_list(sl.left_type[0], 0) {
            let lxy = sl.left_mb_xy[0];
            let r = i32::from(refs[(4 * lxy + 1 + (i32::from(left_block[0]) & !1)) as usize]);
            let m =
                at_mv(self.mb2b_xy[lxy as usize] as i32 + 3 + b_stride * i32::from(left_block[0]));
            let (r, m) = self.fix_mv_mbaff(sl, sl.left_type[0], r, m);
            if r == 0 && m == [0, 0] {
                return zero(sl);
            }
            (r, m)
        } else if sl.left_type[0] != 0 {
            (i32::from(LIST_NOT_USED), [0, 0])
        } else {
            return zero(sl);
        };

        let (top_ref, b) = if uses_list(sl.top_type, 0) {
            let txy = sl.top_mb_xy;
            let r = i32::from(refs[(4 * txy + 2) as usize]);
            let m = at_mv(self.mb2b_xy[txy as usize] as i32 + 3 * b_stride);
            let (r, m) = self.fix_mv_mbaff(sl, sl.top_type, r, m);
            if r == 0 && m == [0, 0] {
                return zero(sl);
            }
            (r, m)
        } else if sl.top_type != 0 {
            (i32::from(LIST_NOT_USED), [0, 0])
        } else {
            return zero(sl);
        };

        let (diagonal_ref, c) = if uses_list(sl.topright_type, 0) {
            let xy = sl.topright_mb_xy;
            let r = i32::from(refs[(4 * xy + 2) as usize]);
            let m = at_mv(self.mb2b_xy[xy as usize] as i32 + 3 * b_stride);
            self.fix_mv_mbaff(sl, sl.topright_type, r, m)
        } else if sl.topright_type != 0 {
            (i32::from(LIST_NOT_USED), [0, 0])
        } else if uses_list(sl.topleft_type, 0) {
            let xy = sl.topleft_mb_xy;
            let part = sl.topleft_partition;
            let r = i32::from(refs[(4 * xy + 1 + (part & 2)) as usize]);
            let m =
                at_mv(self.mb2b_xy[xy as usize] as i32 + 3 + b_stride + (part & (2 * b_stride)));
            self.fix_mv_mbaff(sl, sl.topleft_type, r, m)
        } else if sl.topleft_type != 0 {
            (i32::from(LIST_NOT_USED), [0, 0])
        } else {
            (i32::from(PART_NOT_AVAILABLE), [0, 0])
        };

        let match_count =
            i32::from(diagonal_ref == 0) + i32::from(top_ref == 0) + i32::from(left_ref == 0);
        let (mx, my) = if match_count == 1 {
            if left_ref == 0 {
                (a[0], a[1])
            } else if top_ref == 0 {
                (b[0], b[1])
            } else {
                (c[0], c[1])
            }
        } else {
            (mid_pred(a[0], b[0], c[0]), mid_pred(a[1], b[1], c[1]))
        };
        fill(&mut sl.mv_cache[0], SCAN8[0], 4, 4, [mx as i16, my as i16]);
    }
}

impl Context {
    /// The macroblock's neighbours: their addresses -- which in an MBAFF
    /// frame depend on whether this pair and theirs are fields -- and their
    /// types, 0 for one in another slice (`fill_decode_neighbors`).
    pub(crate) fn fill_decode_neighbors(&self, sl: &mut SliceContext, mb_type: u32) {
        let mb_xy = sl.mb_xy;
        let mb_stride = self.mb_stride;
        sl.topleft_partition = -1;
        let mb_field = sl.hdr.mb_field_decoding_flag;
        let mut top_xy = mb_xy - (mb_stride << mb_field);
        let mut topleft_xy = top_xy - 1;
        let mut topright_xy = top_xy + 1;
        let mut left_xy = [mb_xy - 1; 2];
        sl.left_block = 0;
        if self.mb_aff_frame {
            let left_mb_field_flag = is_interlaced(self.cur_mb_type(mb_xy - 1));
            let curr_mb_field_flag = is_interlaced(mb_type);
            if sl.mb_y & 1 != 0 {
                if left_mb_field_flag != curr_mb_field_flag {
                    left_xy = [mb_xy - mb_stride - 1; 2];
                    if curr_mb_field_flag {
                        left_xy[1] += mb_stride;
                        sl.left_block = 3;
                    } else {
                        topleft_xy += mb_stride;
                        // The top-left vector from the middle of the
                        // macroblock, where every other mode takes the
                        // bottom-right partition's.
                        sl.topleft_partition = 0;
                        sl.left_block = 1;
                    }
                }
            } else {
                if curr_mb_field_flag {
                    let frame_pair =
                        |xy: i32| mb_stride & (((self.cur_mb_type(xy) >> 7) & 1) as i32 - 1);
                    topleft_xy += frame_pair(top_xy - 1);
                    topright_xy += frame_pair(top_xy + 1);
                    top_xy += frame_pair(top_xy);
                }
                if left_mb_field_flag != curr_mb_field_flag {
                    if curr_mb_field_flag {
                        left_xy[1] += mb_stride;
                        sl.left_block = 3;
                    } else {
                        sl.left_block = 2;
                    }
                }
            }
        }

        sl.topleft_mb_xy = topleft_xy;
        sl.top_mb_xy = top_xy;
        sl.topright_mb_xy = topright_xy;
        sl.left_mb_xy = left_xy;
        sl.topleft_type = self.cur_mb_type(topleft_xy);
        sl.top_type = self.cur_mb_type(top_xy);
        sl.topright_type = self.cur_mb_type(topright_xy);
        sl.left_type = [self.cur_mb_type(left_xy[0]), self.cur_mb_type(left_xy[1])];

        let slice_of = |xy: i32| self.slice_table[tab(table_offset(mb_stride), xy)];
        #[allow(
            clippy::cast_sign_loss,
            reason = "slice numbers fit the table's 16 bits"
        )]
        let this = sl.slice_num as u16;
        if slice_of(topleft_xy) != this {
            sl.topleft_type = 0;
            if slice_of(top_xy) != this {
                sl.top_type = 0;
            }
            if slice_of(left_xy[0]) != this {
                sl.left_type = [0, 0];
            }
        }
        if slice_of(topright_xy) != this {
            sl.topright_type = 0;
        }
    }

    /// The caches filled from the neighbours (`fill_decode_caches`): which
    /// samples intra prediction may read, the 4x4 modes, the coefficient
    /// counts, CABAC's coded block patterns, and each list's vectors,
    /// references, vector differences and direct flags -- in an MBAFF frame
    /// scaled between frame and field.
    #[allow(
        clippy::too_many_lines,
        reason = "one function in FFmpeg, kept whole to read against it"
    )]
    pub(crate) fn fill_decode_caches(&self, sl: &mut SliceContext, mb_type: u32, cabac: bool) {
        let top_xy = sl.top_mb_xy;
        let left_xy = sl.left_mb_xy;
        let (topleft_type, top_type, topright_type) =
            (sl.topleft_type, sl.top_type, sl.topright_type);
        let left_type = sl.left_type;
        let left_block = LEFT_BLOCK_OPTIONS[sl.left_block];
        let Some(pps) = self.ps.pps.as_deref() else {
            return;
        };
        let chroma444 = self.chroma_format_idc == 3;
        let chroma422 = self.chroma_format_idc == 2;

        if !is_skip(mb_type) {
            if is_intra(mb_type) {
                // With constrained intra prediction only intra neighbours
                // count.
                let type_mask: u32 = if pps.constrained_intra_pred != 0 {
                    7
                } else {
                    u32::MAX
                };
                sl.topleft_samples_available = 0xFFFF;
                sl.top_samples_available = 0xFFFF;
                sl.left_samples_available = 0xFFFF;
                sl.topright_samples_available = 0xEEEA;
                if top_type & type_mask == 0 {
                    sl.topleft_samples_available = 0xB3FF;
                    sl.top_samples_available = 0x33FF;
                    sl.topright_samples_available = 0x26EA;
                }
                if is_interlaced(mb_type) != is_interlaced(left_type[0]) {
                    if is_interlaced(mb_type) {
                        if left_type[0] & type_mask == 0 {
                            sl.topleft_samples_available &= 0xDFFF;
                            sl.left_samples_available &= 0x5FFF;
                        }
                        if left_type[1] & type_mask == 0 {
                            sl.topleft_samples_available &= 0xFF5F;
                            sl.left_samples_available &= 0xFF5F;
                        }
                    } else {
                        let left_typei = self.cur_mb_type(left_xy[0] + self.mb_stride);
                        if !(left_typei & type_mask != 0 && left_type[0] & type_mask != 0) {
                            sl.topleft_samples_available &= 0xDF5F;
                            sl.left_samples_available &= 0x5F5F;
                        }
                    }
                } else if left_type[0] & type_mask == 0 {
                    sl.topleft_samples_available &= 0xDF5F;
                    sl.left_samples_available &= 0x5F5F;
                }
                if topleft_type & type_mask == 0 {
                    sl.topleft_samples_available &= 0x7FFF;
                }
                if topright_type & type_mask == 0 {
                    sl.topright_samples_available &= 0xFBFF;
                }

                if is_intra4x4(mb_type) {
                    let c = &mut sl.intra4x4_pred_mode_cache;
                    if is_intra4x4(top_type) {
                        let at = self.mb2br_xy[top_xy as usize] as usize;
                        c[4..8].copy_from_slice(&self.intra4x4_pred_mode[at..at + 4]);
                    } else {
                        let v = 2 - 3 * i8::from(top_type & type_mask == 0);
                        c[4..8].fill(v);
                    }
                    for i in 0..2 {
                        let row = 3 + 8 + 2 * 8 * i;
                        if is_intra4x4(left_type[i]) {
                            let at = self.mb2br_xy[left_xy[i] as usize] as usize;
                            let mode = &self.intra4x4_pred_mode[at..at + 7];
                            c[row] = mode[6 - usize::from(left_block[2 * i])];
                            c[row + 8] = mode[6 - usize::from(left_block[1 + 2 * i])];
                        } else {
                            let v = 2 - 3 * i8::from(left_type[i] & type_mask == 0);
                            c[row] = v;
                            c[row + 8] = v;
                        }
                    }
                }
            }

            let nnz_cache = &mut sl.non_zero_count_cache;
            if top_type != 0 {
                let nnz = &self.non_zero_count[top_xy as usize];
                nnz_cache[4..8].copy_from_slice(&nnz[12..16]);
                if self.chroma_y_shift == 0 {
                    nnz_cache[4 + 8 * 5..8 + 8 * 5].copy_from_slice(&nnz[28..32]);
                    nnz_cache[4 + 8 * 10..8 + 8 * 10].copy_from_slice(&nnz[44..48]);
                } else {
                    nnz_cache[4 + 8 * 5..8 + 8 * 5].copy_from_slice(&nnz[20..24]);
                    nnz_cache[4 + 8 * 10..8 + 8 * 10].copy_from_slice(&nnz[36..40]);
                }
            } else {
                let top_empty = if cabac && !is_intra(mb_type) { 0 } else { 0x40 };
                nnz_cache[4..8].fill(top_empty);
                nnz_cache[4 + 8 * 5..8 + 8 * 5].fill(top_empty);
                nnz_cache[4 + 8 * 10..8 + 8 * 10].fill(top_empty);
            }
            for i in 0..2 {
                if left_type[i] != 0 {
                    let nnz = &self.non_zero_count[left_xy[i] as usize];
                    let b0 = usize::from(left_block[8 + 2 * i]);
                    let b1 = usize::from(left_block[8 + 1 + 2 * i]);
                    nnz_cache[3 + 8 + 2 * 8 * i] = nnz[b0];
                    nnz_cache[3 + 8 * 2 + 2 * 8 * i] = nnz[b1];
                    if chroma444 {
                        nnz_cache[3 + 8 * 6 + 2 * 8 * i] = nnz[b0 + 4 * 4];
                        nnz_cache[3 + 8 * 7 + 2 * 8 * i] = nnz[b1 + 4 * 4];
                        nnz_cache[3 + 8 * 11 + 2 * 8 * i] = nnz[b0 + 8 * 4];
                        nnz_cache[3 + 8 * 12 + 2 * 8 * i] = nnz[b1 + 8 * 4];
                    } else if chroma422 {
                        nnz_cache[3 + 8 * 6 + 2 * 8 * i] = nnz[b0 - 2 + 4 * 4];
                        nnz_cache[3 + 8 * 7 + 2 * 8 * i] = nnz[b1 - 2 + 4 * 4];
                        nnz_cache[3 + 8 * 11 + 2 * 8 * i] = nnz[b0 - 2 + 8 * 4];
                        nnz_cache[3 + 8 * 12 + 2 * 8 * i] = nnz[b1 - 2 + 8 * 4];
                    } else {
                        nnz_cache[3 + 8 * 6 + 8 * i] = nnz[usize::from(left_block[8 + 4 + 2 * i])];
                        nnz_cache[3 + 8 * 11 + 8 * i] = nnz[usize::from(left_block[8 + 5 + 2 * i])];
                    }
                } else {
                    let v = if cabac && !is_intra(mb_type) { 0 } else { 64 };
                    for row in [1, 2, 6, 7, 11, 12] {
                        nnz_cache[3 + 8 * row + 2 * 8 * i] = v;
                    }
                }
            }

            if cabac {
                sl.top_cbp = if top_type != 0 {
                    i32::from(self.cbp_table[top_xy as usize])
                } else if is_intra(mb_type) {
                    0x7CF
                } else {
                    0x00F
                };
                sl.left_cbp = if left_type[0] != 0 {
                    let top = i32::from(self.cbp_table[left_xy[0] as usize]);
                    let bottom = i32::from(self.cbp_table[left_xy[1] as usize]);
                    (top & 0x7F0)
                        | ((top >> (i32::from(left_block[0]) & !1)) & 2)
                        | (((bottom >> (i32::from(left_block[2]) & !1)) & 2) << 2)
                } else if is_intra(mb_type) {
                    0x7CF
                } else {
                    0x00F
                };
            }
        }

        if is_inter(mb_type) || (is_direct(mb_type) && sl.hdr.direct_spatial_mv_pred != 0) {
            self.fill_motion_caches(sl, mb_type, cabac, left_block);
        }

        sl.neighbor_transform_size =
            i32::from(is_8x8dct(top_type)) + i32::from(is_8x8dct(left_type[0]));
    }

    /// The motion half of `fill_decode_caches`: each list's neighbouring
    /// vectors and references, and for CABAC their vector differences and
    /// direct flags.
    #[allow(clippy::too_many_lines, reason = "one block of FFmpeg's function")]
    fn fill_motion_caches(
        &self,
        sl: &mut SliceContext,
        mb_type: u32,
        cabac: bool,
        left_block: [u8; 16],
    ) {
        let (topleft_xy, top_xy, topright_xy) = (sl.topleft_mb_xy, sl.top_mb_xy, sl.topright_mb_xy);
        let left_xy = sl.left_mb_xy;
        let (topleft_type, top_type, topright_type) =
            (sl.topleft_type, sl.top_type, sl.topright_type);
        let left_type = sl.left_type;
        let b_stride = self.b_stride;
        let pic = &self.pics[self.cur()];
        let s0 = SCAN8[0];
        let unavailable_or = |t: u32| {
            if t != 0 {
                LIST_NOT_USED
            } else {
                PART_NOT_AVAILABLE
            }
        };

        for list in 0..sl.hdr.list_count as usize {
            if !uses_list(mb_type, list as u32) {
                continue;
            }
            let refs = &pic.ref_index[list];
            let mv = &pic.motion_val[list];
            let at_mv = |b: i32| mv[(4 + b) as usize];
            let ref_cache = &mut sl.ref_cache[list];
            let mv_cache = &mut sl.mv_cache[list];

            if uses_list(top_type, list as u32) {
                let b_xy = self.mb2b_xy[top_xy as usize] as i32 + 3 * b_stride;
                for k in 0..4 {
                    mv_cache[s0 - 8 + k] = at_mv(b_xy + k as i32);
                }
                let r2 = refs[(4 * top_xy + 2) as usize];
                let r3 = refs[(4 * top_xy + 3) as usize];
                ref_cache[s0 - 8..s0 - 6].fill(r2);
                ref_cache[s0 - 6..s0 - 4].fill(r3);
            } else {
                mv_cache[s0 - 8..s0 - 4].fill([0, 0]);
                ref_cache[s0 - 8..s0 - 4].fill(unavailable_or(top_type));
            }

            if mb_type & (MB_TYPE_16X8 | MB_TYPE_8X8) != 0 {
                for i in 0..2 {
                    let idx = s0 - 1 + i * 2 * 8;
                    if uses_list(left_type[i], list as u32) {
                        let b_xy = self.mb2b_xy[left_xy[i] as usize] as i32 + 3;
                        let b8_xy = 4 * left_xy[i] + 1;
                        let l0 = i32::from(left_block[i * 2]);
                        let l1 = i32::from(left_block[1 + i * 2]);
                        mv_cache[idx] = at_mv(b_xy + b_stride * l0);
                        mv_cache[idx + 8] = at_mv(b_xy + b_stride * l1);
                        ref_cache[idx] = refs[(b8_xy + (l0 & !1)) as usize];
                        ref_cache[idx + 8] = refs[(b8_xy + (l1 & !1)) as usize];
                    } else {
                        mv_cache[idx] = [0, 0];
                        mv_cache[idx + 8] = [0, 0];
                        let v = unavailable_or(left_type[i]);
                        ref_cache[idx] = v;
                        ref_cache[idx + 8] = v;
                    }
                }
            } else if uses_list(left_type[0], list as u32) {
                let b_xy = self.mb2b_xy[left_xy[0] as usize] as i32 + 3;
                let b8_xy = 4 * left_xy[0] + 1;
                let l0 = i32::from(left_block[0]);
                mv_cache[s0 - 1] = at_mv(b_xy + b_stride * l0);
                ref_cache[s0 - 1] = refs[(b8_xy + (l0 & !1)) as usize];
            } else {
                mv_cache[s0 - 1] = [0, 0];
                ref_cache[s0 - 1] = unavailable_or(left_type[0]);
            }

            if uses_list(topright_type, list as u32) {
                let b_xy = self.mb2b_xy[topright_xy as usize] as i32 + 3 * b_stride;
                mv_cache[s0 + 4 - 8] = at_mv(b_xy);
                ref_cache[s0 + 4 - 8] = refs[(4 * topright_xy + 2) as usize];
            } else {
                mv_cache[s0 + 4 - 8] = [0, 0];
                ref_cache[s0 + 4 - 8] = unavailable_or(topright_type);
            }
            if ref_cache[s0 + 2 - 8] < 0 || ref_cache[s0 + 4 - 8] < 0 {
                if uses_list(topleft_type, list as u32) {
                    let part = sl.topleft_partition;
                    let b_xy = self.mb2b_xy[topleft_xy as usize] as i32
                        + 3
                        + b_stride
                        + (part & (2 * b_stride));
                    let b8_xy = 4 * topleft_xy + 1 + (part & 2);
                    mv_cache[s0 - 1 - 8] = at_mv(b_xy);
                    ref_cache[s0 - 1 - 8] = refs[b8_xy as usize];
                } else {
                    mv_cache[s0 - 1 - 8] = [0, 0];
                    ref_cache[s0 - 1 - 8] = unavailable_or(topleft_type);
                }
            }

            if mb_type & (MB_TYPE_SKIP | MB_TYPE_DIRECT2) != 0 && !self.mb_aff_frame {
                continue;
            }

            if mb_type & (MB_TYPE_SKIP | MB_TYPE_DIRECT2) == 0 {
                ref_cache[s0 + 2] = PART_NOT_AVAILABLE;
                ref_cache[s0 + 2 + 8 * 2] = PART_NOT_AVAILABLE;
                mv_cache[s0 + 2] = [0, 0];
                mv_cache[s0 + 2 + 8 * 2] = [0, 0];

                if cabac {
                    let mvd_cache = &mut sl.mvd_cache[list];
                    let mvd = &self.mvd_table[list];
                    if uses_list(top_type, list as u32) {
                        let b_xy = self.mb2br_xy[top_xy as usize] as usize;
                        mvd_cache[s0 - 8..s0 - 4].copy_from_slice(&mvd[b_xy..b_xy + 4]);
                    } else {
                        mvd_cache[s0 - 8..s0 - 4].fill([0, 0]);
                    }
                    for (half, rows) in [(0usize, [0usize, 1]), (1, [2, 3])] {
                        if uses_list(left_type[half], list as u32) {
                            let b_xy = self.mb2br_xy[left_xy[half] as usize] as usize + 6;
                            for r in rows {
                                mvd_cache[s0 - 1 + r * 8] = mvd[b_xy - usize::from(left_block[r])];
                            }
                        } else {
                            for r in rows {
                                mvd_cache[s0 - 1 + r * 8] = [0, 0];
                            }
                        }
                    }
                    mvd_cache[s0 + 2] = [0, 0];
                    mvd_cache[s0 + 2 + 8 * 2] = [0, 0];

                    if sl.hdr.slice_type_nos == TYPE_B {
                        let direct_cache = &mut sl.direct_cache;
                        let direct_table = &self.direct_table;
                        let sixteen = (MB_TYPE_16X16 >> 1) as u8;
                        let direct = (MB_TYPE_DIRECT2 >> 1) as u8;
                        fill(direct_cache, s0, 4, 4, sixteen);
                        if is_direct(top_type) {
                            direct_cache[s0 - 8..s0 - 4].fill(direct);
                        } else if is_8x8(top_type) {
                            let b8_xy = 4 * top_xy as usize;
                            direct_cache[s0 - 8] = direct_table[b8_xy + 2];
                            direct_cache[s0 + 2 - 8] = direct_table[b8_xy + 3];
                        } else {
                            direct_cache[s0 - 8..s0 - 4].fill(sixteen);
                        }
                        for (half, row) in [(0usize, 0usize), (1, 2)] {
                            direct_cache[s0 - 1 + row * 8] = if is_direct(left_type[half]) {
                                direct
                            } else if is_8x8(left_type[half]) {
                                let at = 4 * left_xy[half] + 1 + (i32::from(left_block[row]) & !1);
                                direct_table[at as usize]
                            } else {
                                sixteen
                            };
                        }
                    }
                }
            }

            if self.mb_aff_frame {
                let neighbours = [
                    (s0 - 1 - 8, topleft_type),
                    (s0 - 8, top_type),
                    (s0 + 1 - 8, top_type),
                    (s0 + 2 - 8, top_type),
                    (s0 + 3 - 8, top_type),
                    (s0 + 4 - 8, topright_type),
                    (s0 - 1, left_type[0]),
                    (s0 - 1 + 8, left_type[0]),
                    (s0 - 1 + 2 * 8, left_type[1]),
                    (s0 - 1 + 3 * 8, left_type[1]),
                ];
                let to_field = sl.hdr.mb_field_decoding_flag != 0;
                for (idx, kind) in neighbours {
                    let r = sl.ref_cache[list][idx];
                    if r < 0 || is_interlaced(kind) == to_field {
                        continue;
                    }
                    // MAP_F2F: a frame neighbour's motion for a field
                    // macroblock, or a field neighbour's for a frame one.
                    if to_field {
                        sl.ref_cache[list][idx] = r.wrapping_mul(2);
                        sl.mv_cache[list][idx][1] /= 2;
                        sl.mvd_cache[list][idx][1] >>= 1;
                    } else {
                        sl.ref_cache[list][idx] = r >> 1;
                        sl.mv_cache[list][idx][1] = sl.mv_cache[list][idx][1].wrapping_mul(2);
                        sl.mvd_cache[list][idx][1] <<= 1;
                    }
                }
            }
        }
    }
}

impl Context {
    /// A skipped macroblock (`decode_mb_skip`): no coefficients; in a P
    /// slice one 16x16 partition predicting from reference 0 at the
    /// predicted vector, in a B slice direct. `cabac` as the caller's
    /// entropy coder (FFmpeg's `CABAC(h)` where the header is included).
    pub(crate) fn decode_mb_skip(&mut self, sl: &mut SliceContext, cabac: bool) {
        let mb_xy = sl.mb_xy;
        self.non_zero_count[mb_xy as usize] = [0; 48];
        let mut mb_type = 0;
        if sl.hdr.mb_field_decoding_flag != 0 {
            mb_type |= MB_TYPE_INTERLACED;
        }
        if sl.hdr.slice_type_nos == TYPE_B {
            // Only for filling the caches: direct prediction sets the real
            // type.
            mb_type |= MB_TYPE_L0L1 | MB_TYPE_DIRECT2 | MB_TYPE_SKIP;
            if sl.hdr.direct_spatial_mv_pred != 0 {
                self.fill_decode_neighbors(sl, mb_type);
                self.fill_decode_caches(sl, mb_type, cabac);
            }
            self.pred_direct_motion(sl, &mut mb_type);
            mb_type |= MB_TYPE_SKIP;
        } else {
            mb_type |= MB_TYPE_16X16 | MB_TYPE_P0L0 | MB_TYPE_P1L0 | MB_TYPE_SKIP;
            self.fill_decode_neighbors(sl, mb_type);
            self.pred_pskip_motion(sl);
        }
        self.write_back_motion(sl, mb_type, cabac);
        let cur = self.cur();
        let at = tab(table_offset(self.pics[cur].mb_stride), mb_xy);
        self.pics[cur].mb_type[at] = mb_type;
        self.pics[cur].qscale_table[at] = sl.hdr.qscale as i8;
        self.slice_table[tab(table_offset(self.mb_stride), mb_xy)] = sl.slice_num as u16;
        sl.prev_mb_skipped = 1;
    }
}
