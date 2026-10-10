//! Direct prediction's set-up for a slice (`h264_direct.c`): the distance
//! each reference is scaled by in temporal direct mode, which of the
//! co-located picture's references each of the slice's list 0 entries is,
//! and what the current picture keeps of its own references for the
//! pictures that will take it as co-located.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on order counts it clips to 8 bits first"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "reference indices under the slice's counts, which are at most 32; map entries under 48"
)]

use crate::decoder::{Context, SliceContext};
use crate::mbtype::{
    MB_TYPE_8X8, MB_TYPE_8X16, MB_TYPE_16X8, MB_TYPE_16X16, MB_TYPE_DIRECT2, MB_TYPE_INTERLACED,
    MB_TYPE_INTRA_PCM, MB_TYPE_INTRA4X4, MB_TYPE_INTRA16X16, MB_TYPE_L0, MB_TYPE_L0L1,
    MB_TYPE_P0L0, MB_TYPE_P0L1, MB_TYPE_P1L0, MB_TYPE_P1L1, SCAN8, is_8x8, is_16x16, is_direct,
    is_interlaced, is_intra, is_sub_8x8,
};
use crate::mvpred::{PART_NOT_AVAILABLE, fill, mid_pred};
use crate::picture::table_offset;
use crate::slice::{PICT_BOTTOM_FIELD, PICT_FRAME, TYPE_B};

/// An int clamped to an `int8_t` (`av_clip_int8`).
pub(crate) fn clip_int8(a: i32) -> i32 {
    a.clamp(-128, 127)
}

/// An int clamped to a signed `p + 1` bits (`av_clip_intp2`).
fn clip_intp2(a: i32, p: u32) -> i32 {
    a.clamp(-(1 << p), (1 << p) - 1)
}

/// FFmpeg's int of an `int64_t`: the low 32 bits.
#[allow(
    clippy::cast_possible_truncation,
    reason = "C's conversion to int, as FFmpeg's calls make it"
)]
pub(crate) fn low_int(v: i64) -> i32 {
    v as i32
}

/// The scale for list 0 entry `i` in temporal direct mode, the current
/// picture at order count `poc` and the co-located one at `poc1`
/// (`get_scale_factor`): 256 for a long-term reference or no distance.
fn get_scale_factor(h: &Context, sl: &SliceContext, poc: i32, poc1: i32, i: usize) -> i32 {
    let r = sl.ref_list[0][i];
    let poc0 = r.poc;
    let td = clip_int8(low_int(i64::from(poc1) - i64::from(poc0)));
    let long = r.parent.is_some_and(|p| h.pics[p].long_ref != 0);
    if td == 0 || long {
        return 256;
    }
    let tb = clip_int8(low_int(i64::from(poc) - i64::from(poc0)));
    let tx = (16384 + (td.abs() >> 1)) / td;
    clip_intp2((tb * tx + 32) >> 6, 10)
}

impl Context {
    /// Each list 0 entry's scale for temporal direct mode, and each MBAFF
    /// field entry's (`ff_h264_direct_dist_scale_factor`).
    pub(crate) fn direct_dist_scale_factor(&self, sl: &mut SliceContext) {
        let Some(cur) = self.cur_pic_ptr else {
            return;
        };
        let poc = if self.field_picture() {
            self.pics[cur].field_poc[usize::from(self.picture_structure == PICT_BOTTOM_FIELD)]
        } else {
            self.pics[cur].poc
        };
        let poc1 = sl.ref_list[1][0].poc;
        if self.mb_aff_frame {
            if let Some(col) = sl.ref_list[1][0].parent {
                for field in 0..2 {
                    let poc = self.pics[cur].field_poc[field];
                    let poc1 = self.pics[col].field_poc[field];
                    for i in 0..2 * sl.hdr.ref_count[0] as usize {
                        sl.dist_scale_factor_field[field][i ^ field] =
                            get_scale_factor(self, sl, poc, poc1, i + 16);
                    }
                }
            }
        }
        for i in 0..sl.hdr.ref_count[0] as usize {
            sl.dist_scale_factor[i] = get_scale_factor(self, sl, poc, poc1, i);
        }
    }

    /// Which list 0 entry each of the co-located picture's references is,
    /// as one of its fields or MBAFF field entries (`fill_colmap`).
    fn fill_colmap(
        &self,
        sl: &SliceContext,
        list: usize,
        field: usize,
        colfield: usize,
        mbafi: bool,
    ) -> [i32; 48] {
        let mut map = [0; 48];
        let Some(ref1) = sl.ref_list[1][0].parent else {
            return map;
        };
        let ref1 = &self.pics[ref1];
        let start = if mbafi { 16 } else { 0 };
        let end = if mbafi {
            16 + 2 * sl.hdr.ref_count[0] as usize
        } else {
            sl.hdr.ref_count[0] as usize
        };
        let interl = mbafi || self.picture_structure != PICT_FRAME;
        #[allow(clippy::cast_sign_loss, reason = "a reference count, at most 32")]
        let col_count = ref1.ref_count[colfield][list] as usize;
        for rfield in 0..2 {
            for old_ref in 0..col_count {
                let mut poc = ref1.ref_poc[colfield][list][old_ref];
                if !interl {
                    poc |= 3;
                } else if poc & 3 == 3 {
                    poc = (poc & !3) + rfield as i32 + 1;
                }
                for j in start..end {
                    let e = sl.ref_list[0][j];
                    let frame_num = e.parent.map_or(0, |p| self.pics[p].frame_num);
                    if 4 * frame_num + (e.reference & 3) == poc {
                        #[allow(
                            clippy::cast_possible_truncation,
                            clippy::cast_possible_wrap,
                            reason = "under 48"
                        )]
                        let cur_ref = if mbafi {
                            ((j - 16) ^ field) as i32
                        } else {
                            j as i32
                        };
                        if ref1.mbaff != 0 {
                            map[2 * old_ref + (rfield ^ field) + 16] = cur_ref;
                        }
                        if rfield == field || !interl {
                            map[old_ref] = cur_ref;
                        }
                        break;
                    }
                }
            }
        }
        map
    }

    /// What the current picture keeps of the slice's lists for those that
    /// will take it as co-located, and the co-located picture's parity and
    /// reference map for this slice (`ff_h264_direct_ref_list_init`).
    pub(crate) fn direct_ref_list_init(&mut self, sl: &mut SliceContext) {
        let Some(cur) = self.cur_pic_ptr else {
            return;
        };
        let ref1 = sl.ref_list[1][0];
        #[allow(clippy::cast_sign_loss, reason = "0 or 1")]
        let mut sidx = ((self.picture_structure & 1) ^ 1) as usize;
        #[allow(clippy::cast_sign_loss, reason = "0 or 1")]
        let mut ref1sidx = ((ref1.reference & 1) ^ 1) as usize;

        if !self.setup_finished {
            for list in 0..sl.hdr.list_count as usize {
                #[allow(clippy::cast_possible_wrap, reason = "at most 32")]
                let count = sl.hdr.ref_count[list] as i32;
                self.pics[cur].ref_count[sidx][list] = count;
                for j in 0..sl.hdr.ref_count[list] as usize {
                    let e = sl.ref_list[list][j];
                    let frame_num = e.parent.map_or(0, |p| self.pics[p].frame_num);
                    self.pics[cur].ref_poc[sidx][list][j] = 4 * frame_num + (e.reference & 3);
                }
            }
            if self.picture_structure == PICT_FRAME {
                let pic = &mut self.pics[cur];
                pic.ref_count[1] = pic.ref_count[0];
                pic.ref_poc[1] = pic.ref_poc[0];
            }
            if self.current_slice == 0 {
                self.pics[cur].mbaff = i32::from(self.mb_aff_frame);
            }
        }

        sl.col_fieldoff = 0;
        if sl.hdr.list_count != 2 || sl.hdr.ref_count[1] == 0 {
            return;
        }
        let Some(col) = ref1.parent else {
            return;
        };
        if self.picture_structure == PICT_FRAME {
            let cur_poc = i64::from(self.pics[cur].poc);
            let col_poc = self.pics[col].field_poc;
            sl.col_parity = if col_poc == [i32::MAX, i32::MAX] {
                1
            } else {
                i32::from(
                    (i64::from(col_poc[0]) - cur_poc).abs()
                        >= (i64::from(col_poc[1]) - cur_poc).abs(),
                )
            };
            #[allow(clippy::cast_sign_loss, reason = "0 or 1")]
            let parity = sl.col_parity as usize;
            ref1sidx = parity;
            sidx = parity;
        } else if self.picture_structure & ref1.reference == 0 && self.pics[col].mbaff == 0 {
            // A field predicted from the other parity's field.
            sl.col_fieldoff = 2 * ref1.reference - 3;
        }

        if sl.hdr.slice_type_nos != TYPE_B || sl.hdr.direct_spatial_mv_pred != 0 {
            return;
        }
        for list in 0..2 {
            sl.map_col_to_list0[list] = self.fill_colmap(sl, list, sidx, ref1sidx, false);
            if self.mb_aff_frame {
                for field in 0..2 {
                    sl.map_col_to_list0_field[field][list] =
                        self.fill_colmap(sl, list, field, field, true);
                }
            }
        }
    }
}

/// A macroblock type that is 16x16 or intra (`MB_TYPE_16x16_OR_INTRA`).
const MB_TYPE_16X16_OR_INTRA: u32 =
    MB_TYPE_16X16 | MB_TYPE_INTRA4X4 | MB_TYPE_INTRA16X16 | MB_TYPE_INTRA_PCM;

/// Two 16-bit halves of a vector (`pack16to32`), each cut to 16 bits.
#[allow(
    clippy::cast_possible_truncation,
    reason = "FFmpeg stores each half as an int16_t"
)]
fn pack(x: i32, y: i32) -> [i16; 2] {
    [x as i16, y as i16]
}

/// The co-located macroblock's tables, as direct prediction reads them:
/// where its 8x8 references and 4x4 vectors start and how far apart rows
/// of them are.
struct Colocated {
    pic: usize,
    mb_type: [u32; 2],
    /// Index of the first 8x8 reference in each list's `ref_index`.
    ref_at: usize,
    /// Index of the first vector in each list's `motion_val` (its own
    /// allocation, the 4-entry margin included).
    mv_at: usize,
    b8_stride: usize,
    b4_stride: usize,
}

impl Context {
    /// Where the co-located macroblock is and how it is laid out -- a frame
    /// predicted from a field, a field from a frame, or like from like --
    /// and the partitioning that follows from its type
    /// (`pred_spatial_direct_motion` and `pred_temp_direct_motion`'s shared
    /// part). `spatial` picks which variant's flags are added.
    fn colocated(
        &self,
        sl: &SliceContext,
        mb_type: &mut u32,
        sub_mb_type: &mut u32,
        spatial: bool,
    ) -> Option<Colocated> {
        let col = sl.ref_list[1][0].parent?;
        let pic = &self.pics[col];
        let off = table_offset(pic.mb_stride);
        let col_type = |xy: i32| pic.mb_type[(off as isize + xy as isize) as usize];
        let is_b8x8 = is_8x8(*mb_type);
        let mb_stride = self.mb_stride;
        let mut mb_xy = sl.mb_xy;
        let mut b8_stride = 2i32;
        let mut b4_stride = self.b_stride;
        let mut mb_type_col = [0u32; 2];
        let inference = self
            .ps
            .sps
            .as_ref()
            .is_some_and(|s| s.direct_8x8_inference_flag != 0);
        // The temporal variant's partitions predict from both lists.
        let both = if spatial {
            0
        } else {
            MB_TYPE_P0L0 | MB_TYPE_P0L1
        };
        let l0l1 = if spatial { 0 } else { MB_TYPE_L0L1 };

        let mut single_col = false;
        if pic.field_picture != 0 || is_interlaced(col_type(mb_xy)) {
            // AFL/AFR/FR/FL -> AFL/FL
            if !is_interlaced(*mb_type) {
                mb_xy = sl.mb_x + ((sl.mb_y & !1) + sl.col_parity) * mb_stride;
                b8_stride = 0;
            } else {
                mb_xy += mb_stride * sl.col_fieldoff;
            }
            single_col = true;
        } else if is_interlaced(*mb_type) {
            // AFL/FL -> AFR/FR
            mb_xy = sl.mb_x + (sl.mb_y & !1) * mb_stride;
            mb_type_col = [col_type(mb_xy), col_type(mb_xy + mb_stride)];
            b8_stride = 2 + 4 * mb_stride;
            b4_stride *= 6;
            if is_interlaced(mb_type_col[0]) != is_interlaced(mb_type_col[1]) {
                mb_type_col[0] &= !MB_TYPE_INTERLACED;
                mb_type_col[1] &= !MB_TYPE_INTERLACED;
            }
            *sub_mb_type |= MB_TYPE_16X16 | both | MB_TYPE_DIRECT2;
            if mb_type_col[0] & MB_TYPE_16X16_OR_INTRA != 0
                && mb_type_col[1] & MB_TYPE_16X16_OR_INTRA != 0
                && !is_b8x8
            {
                *mb_type |= MB_TYPE_16X8 | l0l1 | MB_TYPE_DIRECT2;
            } else {
                *mb_type |= MB_TYPE_8X8 | l0l1;
            }
        } else {
            single_col = true;
        }
        if single_col {
            // AFR/FR -> AFR/FR, and the field cases above.
            let t = col_type(mb_xy);
            mb_type_col = [t, t];
            *sub_mb_type |= MB_TYPE_16X16 | both | MB_TYPE_DIRECT2;
            if !is_b8x8 && t & MB_TYPE_16X16_OR_INTRA != 0 {
                *mb_type |= MB_TYPE_16X16 | both | MB_TYPE_DIRECT2;
            } else if !is_b8x8 && t & (MB_TYPE_16X8 | MB_TYPE_8X16) != 0 {
                *mb_type |= l0l1 | MB_TYPE_DIRECT2 | (t & (MB_TYPE_16X8 | MB_TYPE_8X16));
            } else {
                if !inference {
                    // B_SUB_4x4.
                    *sub_mb_type = if spatial {
                        sub_mb_type
                            .wrapping_add(MB_TYPE_8X8)
                            .wrapping_sub(MB_TYPE_16X16)
                    } else {
                        MB_TYPE_8X8 | MB_TYPE_P0L0 | MB_TYPE_P0L1 | MB_TYPE_DIRECT2
                    };
                }
                *mb_type |= MB_TYPE_8X8 | l0l1;
            }
        }

        #[allow(clippy::cast_sign_loss, reason = "macroblock addresses in the picture")]
        let mut ref_at = 4 * mb_xy as usize;
        let mut mv_at = 4 + self.mb2b_xy[mb_xy as usize] as usize;
        if b8_stride == 0 && sl.mb_y & 1 != 0 {
            ref_at += 2;
            #[allow(clippy::cast_sign_loss, reason = "a positive stride")]
            {
                mv_at += 2 * b4_stride as usize;
            }
        }
        #[allow(clippy::cast_sign_loss, reason = "positive strides")]
        Some(Colocated {
            pic: col,
            mb_type: mb_type_col,
            ref_at,
            mv_at,
            b8_stride: b8_stride as usize,
            b4_stride: b4_stride as usize,
        })
    }

    /// Spatial direct prediction (`pred_spatial_direct_motion`): each
    /// list's reference the least of the neighbours', its vector their
    /// prediction, zeroed where the co-located block barely moved.
    #[allow(
        clippy::too_many_lines,
        reason = "one function in FFmpeg, kept whole to read against it"
    )]
    fn pred_spatial_direct_motion(&self, sl: &mut SliceContext, mb_type: &mut u32) {
        let is_b8x8 = is_8x8(*mb_type);
        let mut sub_mb_type = MB_TYPE_L0L1;
        let mut r = [0i32; 2];
        let mut mv = [[0i16; 2]; 2];
        let s0 = SCAN8[0];

        for list in 0..2 {
            let left_ref = i32::from(sl.ref_cache[list][s0 - 1]);
            let top_ref = i32::from(sl.ref_cache[list][s0 - 8]);
            let mut refc = i32::from(sl.ref_cache[list][s0 - 8 + 4]);
            let mut c = sl.mv_cache[list][s0 - 8 + 4];
            if refc == i32::from(PART_NOT_AVAILABLE) {
                refc = i32::from(sl.ref_cache[list][s0 - 8 - 1]);
                c = sl.mv_cache[list][s0 - 8 - 1];
            }
            // The least, taken unsigned: a negative one is the largest.
            #[allow(
                clippy::cast_sign_loss,
                clippy::cast_possible_wrap,
                reason = "FFmpeg's unsigned min"
            )]
            let least = (left_ref as u32).min(top_ref as u32).min(refc as u32) as i32;
            r[list] = least;
            if least >= 0 {
                let a = sl.mv_cache[list][s0 - 1];
                let b = sl.mv_cache[list][s0 - 8];
                let match_count = i32::from(left_ref == least)
                    + i32::from(top_ref == least)
                    + i32::from(refc == least);
                mv[list] = if match_count > 1 {
                    let m = |k: usize| mid_pred(i32::from(a[k]), i32::from(b[k]), i32::from(c[k]));
                    pack(m(0), m(1))
                } else if left_ref == least {
                    a
                } else if top_ref == least {
                    b
                } else {
                    c
                };
            } else {
                let mask = !(MB_TYPE_L0 << (2 * list));
                mv[list] = [0, 0];
                r[list] = -1;
                if !is_b8x8 {
                    *mb_type &= mask;
                }
                sub_mb_type &= mask;
            }
        }
        if r[0] < 0 && r[1] < 0 {
            r = [0, 0];
            if !is_b8x8 {
                *mb_type |= MB_TYPE_L0L1;
            }
            sub_mb_type |= MB_TYPE_L0L1;
        }

        #[allow(
            clippy::cast_possible_truncation,
            reason = "a reference index stored as int8"
        )]
        let r8 = [r[0] as i8, r[1] as i8];
        if !is_b8x8 && mv == [[0, 0]; 2] {
            fill(&mut sl.ref_cache[0], s0, 4, 4, r8[0]);
            fill(&mut sl.ref_cache[1], s0, 4, 4, r8[1]);
            fill(&mut sl.mv_cache[0], s0, 4, 4, [0, 0]);
            fill(&mut sl.mv_cache[1], s0, 4, 4, [0, 0]);
            *mb_type = (*mb_type
                & !(MB_TYPE_8X8 | MB_TYPE_16X8 | MB_TYPE_8X16 | MB_TYPE_P1L0 | MB_TYPE_P1L1))
                | MB_TYPE_16X16
                | MB_TYPE_DIRECT2;
            return;
        }

        let Some(col) = self.colocated(sl, mb_type, &mut sub_mb_type, true) else {
            return;
        };
        let pic = &self.pics[col.pic];
        let l1ref = |list: usize, k: usize| pic.ref_index[list][col.ref_at + k];
        let l1mv = |list: usize, k: usize| pic.motion_val[list][col.mv_at + k];
        let long = pic.long_ref != 0;
        #[allow(clippy::cast_sign_loss, reason = "compared unsigned, as FFmpeg does")]
        let x264_ok = self.x264_build as u32 > 33;
        let small = |m: [i16; 2]| i32::from(m[0]).abs() <= 1 && i32::from(m[1]).abs() <= 1;

        if is_interlaced(*mb_type) != is_interlaced(col.mb_type[0]) {
            let mut n = 0;
            for i8 in 0..4 {
                let (x8, y8) = (i8 & 1, i8 >> 1);
                let xy8 = x8 + y8 * col.b8_stride;
                let xy4 = x8 * 3 + y8 * col.b4_stride;
                if is_b8x8 && !is_direct(u32::from(sl.sub_mb_type[i8])) {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation, reason = "flags under 1 << 16")]
                {
                    sl.sub_mb_type[i8] = sub_mb_type as u16;
                }
                fill(&mut sl.ref_cache[0], SCAN8[i8 * 4], 2, 2, r8[0]);
                fill(&mut sl.ref_cache[1], SCAN8[i8 * 4], 2, 2, r8[1]);
                let still = !is_intra(col.mb_type[y8])
                    && !long
                    && ((l1ref(0, xy8) == 0 && small(l1mv(0, xy4)))
                        || (l1ref(0, xy8) < 0 && l1ref(1, xy8) == 0 && small(l1mv(1, xy4))));
                let (a, b) = if still {
                    n += 1;
                    (
                        if r[0] > 0 { mv[0] } else { [0, 0] },
                        if r[1] > 0 { mv[1] } else { [0, 0] },
                    )
                } else {
                    (mv[0], mv[1])
                };
                fill(&mut sl.mv_cache[0], SCAN8[i8 * 4], 2, 2, a);
                fill(&mut sl.mv_cache[1], SCAN8[i8 * 4], 2, 2, b);
            }
            if !is_b8x8 && n & 3 == 0 {
                *mb_type = (*mb_type
                    & !(MB_TYPE_8X8 | MB_TYPE_16X8 | MB_TYPE_8X16 | MB_TYPE_P1L0 | MB_TYPE_P1L1))
                    | MB_TYPE_16X16
                    | MB_TYPE_DIRECT2;
            }
        } else if is_16x16(*mb_type) {
            fill(&mut sl.ref_cache[0], s0, 4, 4, r8[0]);
            fill(&mut sl.ref_cache[1], s0, 4, 4, r8[1]);
            let still = !is_intra(col.mb_type[0])
                && !long
                && ((l1ref(0, 0) == 0 && small(l1mv(0, 0)))
                    || (l1ref(0, 0) < 0 && l1ref(1, 0) == 0 && small(l1mv(1, 0)) && x264_ok));
            let (a, b) = if still {
                (
                    if r[0] > 0 { mv[0] } else { [0, 0] },
                    if r[1] > 0 { mv[1] } else { [0, 0] },
                )
            } else {
                (mv[0], mv[1])
            };
            fill(&mut sl.mv_cache[0], s0, 4, 4, a);
            fill(&mut sl.mv_cache[1], s0, 4, 4, b);
        } else {
            let mut n = 0;
            for i8 in 0..4 {
                let (x8, y8) = (i8 & 1, i8 >> 1);
                if is_b8x8 && !is_direct(u32::from(sl.sub_mb_type[i8])) {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation, reason = "flags under 1 << 16")]
                {
                    sl.sub_mb_type[i8] = sub_mb_type as u16;
                }
                fill(&mut sl.mv_cache[0], SCAN8[i8 * 4], 2, 2, mv[0]);
                fill(&mut sl.mv_cache[1], SCAN8[i8 * 4], 2, 2, mv[1]);
                fill(&mut sl.ref_cache[0], SCAN8[i8 * 4], 2, 2, r8[0]);
                fill(&mut sl.ref_cache[1], SCAN8[i8 * 4], 2, 2, r8[1]);

                // col_zero_flag
                if !is_intra(col.mb_type[0])
                    && !long
                    && (l1ref(0, i8) == 0 || (l1ref(0, i8) < 0 && l1ref(1, i8) == 0 && x264_ok))
                {
                    let list = if l1ref(0, i8) == 0 { 0 } else { 1 };
                    if is_sub_8x8(sub_mb_type) {
                        if small(l1mv(list, x8 * 3 + y8 * 3 * col.b4_stride)) {
                            if r[0] == 0 {
                                fill(&mut sl.mv_cache[0], SCAN8[i8 * 4], 2, 2, [0, 0]);
                            }
                            if r[1] == 0 {
                                fill(&mut sl.mv_cache[1], SCAN8[i8 * 4], 2, 2, [0, 0]);
                            }
                            n += 4;
                        }
                    } else {
                        let mut m = 0;
                        for i4 in 0..4 {
                            let k = x8 * 2 + (i4 & 1) + (y8 * 2 + (i4 >> 1)) * col.b4_stride;
                            if small(l1mv(list, k)) {
                                if r[0] == 0 {
                                    sl.mv_cache[0][SCAN8[i8 * 4 + i4]] = [0, 0];
                                }
                                if r[1] == 0 {
                                    sl.mv_cache[1][SCAN8[i8 * 4 + i4]] = [0, 0];
                                }
                                m += 1;
                            }
                        }
                        if m & 3 == 0 {
                            #[allow(
                                clippy::cast_possible_truncation,
                                reason = "flags under 1 << 16"
                            )]
                            let delta = MB_TYPE_16X16.wrapping_sub(MB_TYPE_8X8) as u16;
                            sl.sub_mb_type[i8] = sl.sub_mb_type[i8].wrapping_add(delta);
                        }
                        n += m;
                    }
                }
            }
            if !is_b8x8 && n & 15 == 0 {
                *mb_type = (*mb_type
                    & !(MB_TYPE_8X8 | MB_TYPE_16X8 | MB_TYPE_8X16 | MB_TYPE_P1L0 | MB_TYPE_P1L1))
                    | MB_TYPE_16X16
                    | MB_TYPE_DIRECT2;
            }
        }
    }

    /// Temporal direct prediction (`pred_temp_direct_motion`): the
    /// co-located block's vector scaled by how far its reference and this
    /// picture are, list 1's the rest of the way.
    #[allow(
        clippy::too_many_lines,
        reason = "one function in FFmpeg, kept whole to read against it"
    )]
    fn pred_temp_direct_motion(&self, sl: &mut SliceContext, mb_type: &mut u32) {
        let is_b8x8 = is_8x8(*mb_type);
        let mut sub_mb_type = 0;
        let Some(col) = self.colocated(sl, mb_type, &mut sub_mb_type, false) else {
            return;
        };
        let pic = &self.pics[col.pic];
        let l1ref = |list: usize, k: usize| pic.ref_index[list][col.ref_at + k];
        let l1mv = |list: usize, k: usize| pic.motion_val[list][col.mv_at + k];

        let field = usize::try_from(sl.mb_y & 1).unwrap_or(0);
        let (map_col_to_list0, dist_scale_factor) = if self.mb_aff_frame && is_interlaced(*mb_type)
        {
            (
                sl.map_col_to_list0_field[field],
                sl.dist_scale_factor_field[field],
            )
        } else {
            (sl.map_col_to_list0, sl.dist_scale_factor)
        };
        #[allow(
            clippy::cast_possible_wrap,
            reason = "a flag shifted into a small offset"
        )]
        let ref_offset = ((pic.mbaff << 4) & (col.mb_type[0] >> 3) as i32) as usize;
        let map = |list: usize, r: i8| {
            #[allow(
                clippy::cast_sign_loss,
                reason = "a reference index FFmpeg uses as one"
            )]
            map_col_to_list0[list][r as usize + ref_offset]
        };
        let scaled = |scale: i32, v: i16| (scale * i32::from(v) + 128) >> 8;
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a reference index stored as int8"
        )]
        let as_ref = |r: i32| r as i8;
        let scale_of = |r: i32| {
            #[allow(clippy::cast_sign_loss, reason = "a mapped reference index")]
            dist_scale_factor[r as usize]
        };

        if is_interlaced(*mb_type) != is_interlaced(col.mb_type[0]) {
            let y_shift = 2 * u32::from(!is_interlaced(*mb_type));
            for i8 in 0..4 {
                let (x8, y8) = (i8 & 1, i8 >> 1);
                if is_b8x8 && !is_direct(u32::from(sl.sub_mb_type[i8])) {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation, reason = "flags under 1 << 16")]
                {
                    sl.sub_mb_type[i8] = sub_mb_type as u16;
                }
                fill(&mut sl.ref_cache[1], SCAN8[i8 * 4], 2, 2, 0);
                if is_intra(col.mb_type[y8]) {
                    fill(&mut sl.ref_cache[0], SCAN8[i8 * 4], 2, 2, 0);
                    fill(&mut sl.mv_cache[0], SCAN8[i8 * 4], 2, 2, [0, 0]);
                    fill(&mut sl.mv_cache[1], SCAN8[i8 * 4], 2, 2, [0, 0]);
                    continue;
                }
                let k8 = x8 + y8 * col.b8_stride;
                let (ref0, list) = if l1ref(0, k8) >= 0 {
                    (map(0, l1ref(0, k8)), 0)
                } else {
                    (map(1, l1ref(1, k8)), 1)
                };
                let scale = scale_of(ref0);
                fill(&mut sl.ref_cache[0], SCAN8[i8 * 4], 2, 2, as_ref(ref0));
                let mv_col = l1mv(list, x8 * 3 + y8 * col.b4_stride);
                let my_col = (i32::from(mv_col[1]) * (1 << y_shift)) / 2;
                let mx = scaled(scale, mv_col[0]);
                let my = (scale * my_col + 128) >> 8;
                fill(&mut sl.mv_cache[0], SCAN8[i8 * 4], 2, 2, pack(mx, my));
                fill(
                    &mut sl.mv_cache[1],
                    SCAN8[i8 * 4],
                    2,
                    2,
                    pack(mx - i32::from(mv_col[0]), my - my_col),
                );
            }
            return;
        }

        // One-to-one vector scaling.
        if is_16x16(*mb_type) {
            fill(&mut sl.ref_cache[1], SCAN8[0], 4, 4, 0);
            let (r, mv0, mv1) = if is_intra(col.mb_type[0]) {
                (0, [0, 0], [0, 0])
            } else {
                let (ref0, list) = if l1ref(0, 0) >= 0 {
                    (map(0, l1ref(0, 0)), 0)
                } else {
                    (map(1, l1ref(1, 0)), 1)
                };
                let scale = scale_of(ref0);
                let mv_col = l1mv(list, 0);
                let l0 = [scaled(scale, mv_col[0]), scaled(scale, mv_col[1])];
                (
                    ref0,
                    pack(l0[0], l0[1]),
                    pack(l0[0] - i32::from(mv_col[0]), l0[1] - i32::from(mv_col[1])),
                )
            };
            fill(&mut sl.ref_cache[0], SCAN8[0], 4, 4, as_ref(r));
            fill(&mut sl.mv_cache[0], SCAN8[0], 4, 4, mv0);
            fill(&mut sl.mv_cache[1], SCAN8[0], 4, 4, mv1);
            return;
        }
        for i8 in 0..4 {
            let (x8, y8) = (i8 & 1, i8 >> 1);
            if is_b8x8 && !is_direct(u32::from(sl.sub_mb_type[i8])) {
                continue;
            }
            #[allow(clippy::cast_possible_truncation, reason = "flags under 1 << 16")]
            {
                sl.sub_mb_type[i8] = sub_mb_type as u16;
            }
            fill(&mut sl.ref_cache[1], SCAN8[i8 * 4], 2, 2, 0);
            if is_intra(col.mb_type[0]) {
                fill(&mut sl.ref_cache[0], SCAN8[i8 * 4], 2, 2, 0);
                fill(&mut sl.mv_cache[0], SCAN8[i8 * 4], 2, 2, [0, 0]);
                fill(&mut sl.mv_cache[1], SCAN8[i8 * 4], 2, 2, [0, 0]);
                continue;
            }
            let (ref0, list) = if l1ref(0, i8) >= 0 {
                (map(0, l1ref(0, i8)), 0)
            } else {
                (map(1, l1ref(1, i8)), 1)
            };
            let scale = scale_of(ref0);
            fill(&mut sl.ref_cache[0], SCAN8[i8 * 4], 2, 2, as_ref(ref0));
            if is_sub_8x8(sub_mb_type) {
                let mv_col = l1mv(list, x8 * 3 + y8 * 3 * col.b4_stride);
                let mx = scaled(scale, mv_col[0]);
                let my = scaled(scale, mv_col[1]);
                fill(&mut sl.mv_cache[0], SCAN8[i8 * 4], 2, 2, pack(mx, my));
                fill(
                    &mut sl.mv_cache[1],
                    SCAN8[i8 * 4],
                    2,
                    2,
                    pack(mx - i32::from(mv_col[0]), my - i32::from(mv_col[1])),
                );
            } else {
                for i4 in 0..4 {
                    let mv_col = l1mv(
                        list,
                        x8 * 2 + (i4 & 1) + (y8 * 2 + (i4 >> 1)) * col.b4_stride,
                    );
                    let l0 = pack(scaled(scale, mv_col[0]), scaled(scale, mv_col[1]));
                    sl.mv_cache[0][SCAN8[i8 * 4 + i4]] = l0;
                    sl.mv_cache[1][SCAN8[i8 * 4 + i4]] = pack(
                        i32::from(l0[0]) - i32::from(mv_col[0]),
                        i32::from(l0[1]) - i32::from(mv_col[1]),
                    );
                }
            }
        }
    }

    /// A direct macroblock's or partition's motion (`ff_h264_pred_direct_motion`).
    pub(crate) fn pred_direct_motion(&self, sl: &mut SliceContext, mb_type: &mut u32) {
        if sl.hdr.direct_spatial_mv_pred != 0 {
            self.pred_spatial_direct_motion(sl, mb_type);
        } else {
            self.pred_temp_direct_motion(sl, mb_type);
        }
    }
}
