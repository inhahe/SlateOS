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
