//! A slice's header, read as FFmpeg reads it (`h264_slice_header_parse` in
//! `h264_slice.c`, with `ff_h264_parse_ref_count` and
//! `ff_h264_pred_weight_table` from `h264_parse.c` and the reference list
//! modifications and marking from `h264_refs.c`).
//!
//! FFmpeg reads each slice's header into a slice context it keeps from one
//! slice to the next, and a header leaves some fields as the last one left
//! them -- an IDR picture's id when this slice is not an IDR's, CABAC's
//! table when this slice is CAVLC or intra. [`SliceHeader`] is that part of
//! the context, kept the same way: [`SliceHeader::parse`] overwrites what
//! FFmpeg's overwrites and nothing else.

#![allow(
    clippy::indexing_slicing,
    reason = "indices are under the fixed sizes they index: reference indices under the \
              reference counts, which are checked against 32 first; MMCOs under 66; MBAFF \
              weights at 16 + 2i + 1 for i under 16"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on values it bounds first; its unsigned arithmetic is \
              written wrapping"
)]

use crate::bits::Bits;
use crate::nal::{NAL_IDR_SLICE, Nal};
use crate::ps::{ParamSets, Pps, Sps};

/// A picture's top field alone (`PICT_TOP_FIELD`).
pub(crate) const PICT_TOP_FIELD: i32 = 1;
/// Its bottom field alone (`PICT_BOTTOM_FIELD`).
pub(crate) const PICT_BOTTOM_FIELD: i32 = 2;
/// A whole frame (`PICT_FRAME`).
pub(crate) const PICT_FRAME: i32 = 3;

/// `AV_PICTURE_TYPE_I`: intra.
pub(crate) const TYPE_I: i32 = 1;
/// `AV_PICTURE_TYPE_P`: predicted from one list.
pub(crate) const TYPE_P: i32 = 2;
/// `AV_PICTURE_TYPE_B`: predicted from two.
pub(crate) const TYPE_B: i32 = 3;
/// `AV_PICTURE_TYPE_SI`: switching intra.
pub(crate) const TYPE_SI: i32 = 5;
/// `AV_PICTURE_TYPE_SP`: switching predicted.
pub(crate) const TYPE_SP: i32 = 6;

/// `slice_type` modulo 5 as a picture type (`ff_h264_golomb_to_pict_type`).
const GOLOMB_TO_PICT_TYPE: [i32; 5] = [TYPE_P, TYPE_B, TYPE_I, TYPE_SP, TYPE_SI];

/// The most memory management operations a slice may carry
/// (`H264_MAX_MMCO_COUNT`).
pub(crate) const MAX_MMCO_COUNT: usize = 66;

/// `MMCO_END` and the rest (`MMCOOpcode`).
pub(crate) const MMCO_END: u32 = 0;
pub(crate) const MMCO_SHORT2UNUSED: u32 = 1;
pub(crate) const MMCO_LONG2UNUSED: u32 = 2;
pub(crate) const MMCO_SHORT2LONG: u32 = 3;
pub(crate) const MMCO_SET_MAX_LONG: u32 = 4;
pub(crate) const MMCO_RESET: u32 = 5;
pub(crate) const MMCO_LONG: u32 = 6;

/// Why a slice header was refused. FFmpeg returns different codes, but its
/// caller only asks whether there was one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SliceError;

/// One memory management control operation (`MMCO`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Mmco {
    pub(crate) opcode: u32,
    /// The picture number it names, wrapped to the current picture's range
    /// (`short_pic_num`).
    pub(crate) short_pic_num: i32,
    /// A long-term index, picture number or count (`long_arg`).
    pub(crate) long_arg: i32,
}

/// One reference list modification (`ref_modifications[list][i]`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RefModification {
    pub(crate) op: u8,
    pub(crate) val: u32,
}

/// Explicit weighted prediction's weights and offsets
/// (`H264PredWeightTable`). Entries 16 to 47 are an MBAFF frame's field
/// references: two for each frame reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PredWeightTable {
    pub(crate) use_weight: i32,
    pub(crate) use_weight_chroma: i32,
    pub(crate) luma_log2_weight_denom: i32,
    pub(crate) chroma_log2_weight_denom: i32,
    pub(crate) luma_weight_flag: [i32; 2],
    pub(crate) chroma_weight_flag: [i32; 2],
    /// `[ref][list][weight, offset]`.
    pub(crate) luma_weight: [[[i32; 2]; 2]; 48],
    /// `[ref][list][Cb, Cr][weight, offset]`.
    pub(crate) chroma_weight: [[[[i32; 2]; 2]; 2]; 48],
    /// Implicit weights, worked out when a slice is set up:
    /// `[ref0][ref1][frame or field]`; a row for each list 0 reference.
    pub(crate) implicit_weight: Box<[[[i32; 2]; 48]]>,
}

impl Default for PredWeightTable {
    fn default() -> Self {
        Self {
            use_weight: 0,
            use_weight_chroma: 0,
            luma_log2_weight_denom: 0,
            chroma_log2_weight_denom: 0,
            luma_weight_flag: [0; 2],
            chroma_weight_flag: [0; 2],
            luma_weight: [[[0; 2]; 2]; 48],
            chroma_weight: [[[[0; 2]; 2]; 2]; 48],
            implicit_weight: vec![[[0; 2]; 48]; 48].into_boxed_slice(),
        }
    }
}

/// The part of FFmpeg's slice context a slice header writes
/// (`H264SliceContext`), kept from one slice to the next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SliceHeader {
    pub(crate) first_mb_addr: u32,
    /// The slice's picture type (`AV_PICTURE_TYPE_*`).
    pub(crate) slice_type: i32,
    /// The same with SI and SP taken as I and P (`slice_type_nos`).
    pub(crate) slice_type_nos: i32,
    /// Whether every slice of the picture has this type (`slice_type`
    /// over 4).
    pub(crate) slice_type_fixed: i32,
    pub(crate) pps_id: u32,
    pub(crate) frame_num: i32,
    pub(crate) mb_mbaff: i32,
    /// A frame, or which field (`PICT_*`).
    pub(crate) picture_structure: i32,
    pub(crate) mb_field_decoding_flag: i32,
    pub(crate) curr_pic_num: i32,
    pub(crate) max_pic_num: i32,
    pub(crate) idr_pic_id: i32,
    pub(crate) poc_lsb: i32,
    pub(crate) delta_poc_bottom: i32,
    pub(crate) delta_poc: [i32; 2],
    pub(crate) redundant_pic_count: i32,
    pub(crate) direct_spatial_mv_pred: i32,
    /// How many reference lists the slice uses: 0, 1 or 2.
    pub(crate) list_count: u32,
    pub(crate) ref_count: [u32; 2],
    pub(crate) ref_modifications: [[RefModification; 32]; 2],
    pub(crate) nb_ref_modifications: [usize; 2],
    pub(crate) pwt: PredWeightTable,
    pub(crate) explicit_ref_marking: i32,
    pub(crate) mmco: [Mmco; MAX_MMCO_COUNT],
    pub(crate) nb_mmco: usize,
    pub(crate) cabac_init_idc: i32,
    pub(crate) last_qscale_diff: i32,
    pub(crate) qscale: i32,
    pub(crate) chroma_qp: [i32; 2],
    /// 0 off, 1 on, 2 on but not across slice edges (the stream's
    /// `disable_deblocking_filter_idc` with 0 and 1 swapped).
    pub(crate) deblocking_filter: i32,
    pub(crate) slice_alpha_c0_offset: i32,
    pub(crate) slice_beta_offset: i32,
}

impl Default for SliceHeader {
    fn default() -> Self {
        Self {
            first_mb_addr: 0,
            slice_type: 0,
            slice_type_nos: 0,
            slice_type_fixed: 0,
            pps_id: 0,
            frame_num: 0,
            mb_mbaff: 0,
            picture_structure: 0,
            mb_field_decoding_flag: 0,
            curr_pic_num: 0,
            max_pic_num: 0,
            idr_pic_id: 0,
            poc_lsb: 0,
            delta_poc_bottom: 0,
            delta_poc: [0; 2],
            redundant_pic_count: 0,
            direct_spatial_mv_pred: 0,
            list_count: 0,
            ref_count: [0; 2],
            ref_modifications: [[RefModification::default(); 32]; 2],
            nb_ref_modifications: [0; 2],
            pwt: PredWeightTable::default(),
            explicit_ref_marking: 0,
            mmco: [Mmco::default(); MAX_MMCO_COUNT],
            nb_mmco: 0,
            cabac_init_idc: 0,
            last_qscale_diff: 0,
            qscale: 0,
            chroma_qp: [0; 2],
            deblocking_filter: 0,
            slice_alpha_c0_offset: 0,
            slice_beta_offset: 0,
        }
    }
}

/// What the header needs of the picture being decoded.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PictureState {
    /// Whether this is the picture's first slice.
    pub(crate) first_slice: bool,
    /// The picture's `frame_num`, which every later slice must repeat
    /// (`h->poc.frame_num`).
    pub(crate) frame_num: i32,
}

impl SliceHeader {
    /// Read a slice header from `gb`, past the unit's header
    /// (`h264_slice_header_parse`). On refusal the fields read so far keep
    /// what was read, as FFmpeg's do.
    #[allow(
        clippy::too_many_lines,
        reason = "one function in FFmpeg, kept whole so it reads line for line against it"
    )]
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "FFmpeg's int and unsigned, converted where it converts"
    )]
    pub(crate) fn parse(
        &mut self,
        gb: &mut Bits<'_>,
        nal: &Nal,
        ps: &ParamSets,
        picture: PictureState,
    ) -> Result<(), SliceError> {
        self.first_mb_addr = gb.ue_long();

        let mut slice_type = gb.ue_31();
        if slice_type > 9 {
            return Err(SliceError);
        }
        if slice_type > 4 {
            slice_type -= 5;
            self.slice_type_fixed = 1;
        } else {
            self.slice_type_fixed = 0;
        }
        let slice_type = GOLOMB_TO_PICT_TYPE[slice_type as usize];
        self.slice_type = slice_type;
        self.slice_type_nos = slice_type & 3;

        if nal.kind == NAL_IDR_SLICE && self.slice_type_nos != TYPE_I {
            return Err(SliceError);
        }

        self.pps_id = gb.ue() as u32;
        let Some(pps) = ps
            .pps_list
            .get(self.pps_id as usize)
            .and_then(Option::as_deref)
        else {
            return Err(SliceError);
        };
        let sps = &*pps.sps;

        #[allow(clippy::cast_sign_loss, reason = "4 to 16 bits")]
        let frame_num = gb.bits(sps.log2_max_frame_num as u32) as i32;
        self.frame_num = frame_num;
        if !picture.first_slice && picture.frame_num != self.frame_num {
            return Err(SliceError);
        }

        self.mb_mbaff = 0;

        let picture_structure = if sps.frame_mbs_only_flag != 0 {
            PICT_FRAME
        } else {
            if sps.direct_8x8_inference_flag == 0 && slice_type == TYPE_B {
                // "This stream was generated by a broken encoder".
                return Err(SliceError);
            }
            if gb.flag() {
                PICT_TOP_FIELD + gb.bit() as i32
            } else {
                PICT_FRAME
            }
        };
        self.picture_structure = picture_structure;
        self.mb_field_decoding_flag = i32::from(picture_structure != PICT_FRAME);

        if picture_structure == PICT_FRAME {
            self.curr_pic_num = self.frame_num;
            self.max_pic_num = 1 << sps.log2_max_frame_num;
        } else {
            self.curr_pic_num = 2 * self.frame_num + 1;
            self.max_pic_num = 1 << (sps.log2_max_frame_num + 1);
        }

        if nal.kind == NAL_IDR_SLICE {
            let idr_pic_id = gb.ue_long();
            // An id past 65535 is ignored; the last one stays.
            if idr_pic_id < 65536 {
                self.idr_pic_id = idr_pic_id as i32;
            }
        }

        self.poc_lsb = 0;
        self.delta_poc_bottom = 0;
        if sps.poc_type == 0 {
            #[allow(clippy::cast_sign_loss, reason = "4 to 16 bits")]
            let lsb = gb.bits(sps.log2_max_poc_lsb as u32) as i32;
            self.poc_lsb = lsb;
            if pps.pic_order_present == 1 && picture_structure == PICT_FRAME {
                self.delta_poc_bottom = gb.se();
            }
        }

        self.delta_poc = [0, 0];
        if sps.poc_type == 1 && sps.delta_pic_order_always_zero_flag == 0 {
            self.delta_poc[0] = gb.se();
            if pps.pic_order_present == 1 && picture_structure == PICT_FRAME {
                self.delta_poc[1] = gb.se();
            }
        }

        self.redundant_pic_count = 0;
        if pps.redundant_pic_cnt_present != 0 {
            self.redundant_pic_count = gb.ue();
        }

        if self.slice_type_nos == TYPE_B {
            self.direct_spatial_mv_pred = gb.bit() as i32;
        }

        self.parse_ref_count(gb, pps, picture_structure)?;

        if self.slice_type_nos != TYPE_I && self.decode_ref_pic_list_reordering(gb).is_err() {
            self.ref_count = [0, 0];
            return Err(SliceError);
        }

        self.pwt.use_weight = 0;
        self.pwt.luma_weight_flag = [0; 2];
        self.pwt.chroma_weight_flag = [0; 2];
        if (pps.weighted_pred != 0 && self.slice_type_nos == TYPE_P)
            || (pps.weighted_bipred_idc == 1 && self.slice_type_nos == TYPE_B)
        {
            self.pred_weight_table(gb, sps, picture_structure)?;
        }

        self.explicit_ref_marking = 0;
        if nal.ref_idc != 0 {
            // A refused marking is taken as far as it was read, and the
            // header read on, unless errors are to explode (they are not).
            let _ = self.decode_ref_pic_marking(gb, nal);
        }

        if self.slice_type_nos != TYPE_I && pps.cabac != 0 {
            let tmp = gb.ue_31();
            if tmp > 2 {
                return Err(SliceError);
            }
            self.cabac_init_idc = tmp as i32;
        }

        self.last_qscale_diff = 0;
        let tmp = (pps.init_qp as u32).wrapping_add(gb.se() as u32);
        if tmp > (51 + 6 * (sps.bit_depth_luma - 8)) as u32 {
            return Err(SliceError);
        }
        self.qscale = tmp as i32;
        self.chroma_qp = [
            i32::from(pps.chroma_qp_table[0][tmp as usize]),
            i32::from(pps.chroma_qp_table[1][tmp as usize]),
        ];
        if self.slice_type == TYPE_SP {
            gb.bit(); // sp_for_switch_flag
        }
        if self.slice_type == TYPE_SP || self.slice_type == TYPE_SI {
            gb.se(); // slice_qs_delta
        }

        self.deblocking_filter = 1;
        self.slice_alpha_c0_offset = 0;
        self.slice_beta_offset = 0;
        if pps.deblocking_filter_parameters_present != 0 {
            let tmp = gb.ue_31();
            if tmp > 2 {
                return Err(SliceError);
            }
            self.deblocking_filter = tmp as i32;
            if self.deblocking_filter < 2 {
                self.deblocking_filter ^= 1; // 1 <-> 0
            }
            if self.deblocking_filter != 0 {
                let alpha = gb.se();
                let beta = gb.se();
                if !(-6..=6).contains(&alpha) || !(-6..=6).contains(&beta) {
                    return Err(SliceError);
                }
                self.slice_alpha_c0_offset = alpha * 2;
                self.slice_beta_offset = beta * 2;
            }
        }
        Ok(())
    }

    /// `ff_h264_parse_ref_count`: the picture parameter set's counts,
    /// overridden by the slice's; refused past 16 references a frame
    /// (32 a field), list 1's taken as none when only it is over.
    #[allow(clippy::cast_sign_loss, reason = "FFmpeg's int stored as unsigned")]
    fn parse_ref_count(
        &mut self,
        gb: &mut Bits<'_>,
        pps: &Pps,
        picture_structure: i32,
    ) -> Result<(), SliceError> {
        let mut ref_count = [pps.ref_count[0], pps.ref_count[1]];
        let list_count;
        if self.slice_type_nos == TYPE_I {
            list_count = 0;
            ref_count = [0, 0];
        } else {
            let max: u32 = if picture_structure == PICT_FRAME {
                15
            } else {
                31
            };
            if gb.flag() {
                // num_ref_idx_active_override_flag
                ref_count[0] = gb.ue().wrapping_add(1) as u32;
                ref_count[1] = if self.slice_type_nos == TYPE_B {
                    gb.ue().wrapping_add(1) as u32
                } else {
                    // The full range is the standard's for P too, even for
                    // frames.
                    1
                };
            }
            list_count = if self.slice_type_nos == TYPE_B { 2 } else { 1 };
            if ref_count[0].wrapping_sub(1) > max
                || (list_count == 2 && ref_count[1].wrapping_sub(1) > max)
            {
                self.ref_count = [0, 0];
                self.list_count = 0;
                return Err(SliceError);
            } else if ref_count[1].wrapping_sub(1) > max {
                ref_count[1] = 0;
            }
        }
        self.ref_count = ref_count;
        self.list_count = list_count;
        Ok(())
    }

    /// `ff_h264_decode_ref_pic_list_reordering`: the modifications of each
    /// list, as they come.
    fn decode_ref_pic_list_reordering(&mut self, gb: &mut Bits<'_>) -> Result<(), SliceError> {
        self.nb_ref_modifications = [0, 0];
        for list in 0..self.list_count as usize {
            if !gb.flag() {
                continue; // ref_pic_list_modification_flag_l[01]
            }
            for index in 0.. {
                let op = gb.ue_31();
                if op == 3 {
                    break;
                }
                if index >= self.ref_count[list] as usize || op > 2 {
                    return Err(SliceError);
                }
                #[allow(clippy::cast_possible_truncation, reason = "0 to 2")]
                let op = op as u8;
                self.ref_modifications[list][index] = RefModification {
                    op,
                    val: gb.ue_long(),
                };
                self.nb_ref_modifications[list] += 1;
            }
        }
        Ok(())
    }

    /// `ff_h264_pred_weight_table`: explicit weights and offsets for each
    /// reference of each list the slice uses, a weight out of an `int8_t`'s
    /// range refusing the slice; an MBAFF frame's field references copied
    /// from their frame's.
    #[allow(clippy::cast_sign_loss, reason = "FFmpeg's int compared as unsigned")]
    fn pred_weight_table(
        &mut self,
        gb: &mut Bits<'_>,
        sps: &Sps,
        picture_structure: i32,
    ) -> Result<(), SliceError> {
        let pwt = &mut self.pwt;
        pwt.use_weight = 0;
        pwt.use_weight_chroma = 0;

        pwt.luma_log2_weight_denom = gb.ue_31() as i32;
        if pwt.luma_log2_weight_denom as u32 > 7 {
            pwt.luma_log2_weight_denom = 0;
        }
        let luma_def = 1 << pwt.luma_log2_weight_denom;
        // Never read without chroma: FFmpeg leaves it uninitialised then.
        let mut chroma_def = 0;
        if sps.chroma_format_idc != 0 {
            pwt.chroma_log2_weight_denom = gb.ue_31() as i32;
            if pwt.chroma_log2_weight_denom as u32 > 7 {
                pwt.chroma_log2_weight_denom = 0;
            }
            chroma_def = 1 << pwt.chroma_log2_weight_denom;
        }

        let fits_i8 = |v: i32| i32::from(v as i8) == v;
        for list in 0..2 {
            pwt.luma_weight_flag[list] = 0;
            pwt.chroma_weight_flag[list] = 0;
            for i in 0..self.ref_count[list] as usize {
                if gb.flag() {
                    pwt.luma_weight[i][list] = [gb.se(), gb.se()];
                    if !fits_i8(pwt.luma_weight[i][list][0])
                        || !fits_i8(pwt.luma_weight[i][list][1])
                    {
                        return Err(SliceError);
                    }
                    if pwt.luma_weight[i][list] != [luma_def, 0] {
                        pwt.use_weight = 1;
                        pwt.luma_weight_flag[list] = 1;
                    }
                } else {
                    pwt.luma_weight[i][list] = [luma_def, 0];
                }

                if sps.chroma_format_idc != 0 {
                    if gb.flag() {
                        for j in 0..2 {
                            pwt.chroma_weight[i][list][j] = [gb.se(), gb.se()];
                            let [w, o] = pwt.chroma_weight[i][list][j];
                            if !fits_i8(w) || !fits_i8(o) {
                                pwt.chroma_weight[i][list][j] = [chroma_def, 0];
                                return Err(SliceError);
                            }
                            if [w, o] != [chroma_def, 0] {
                                pwt.use_weight_chroma = 1;
                                pwt.chroma_weight_flag[list] = 1;
                            }
                        }
                    } else {
                        pwt.chroma_weight[i][list] = [[chroma_def, 0]; 2];
                    }
                }

                // For MBAFF: each frame reference's two fields.
                if picture_structure == PICT_FRAME {
                    let w = pwt.luma_weight[i][list];
                    pwt.luma_weight[16 + 2 * i][list] = w;
                    pwt.luma_weight[16 + 2 * i + 1][list] = w;
                    if sps.chroma_format_idc != 0 {
                        let c = pwt.chroma_weight[i][list];
                        pwt.chroma_weight[16 + 2 * i][list] = c;
                        pwt.chroma_weight[16 + 2 * i + 1][list] = c;
                    }
                }
            }
            if self.slice_type_nos != TYPE_B {
                break;
            }
        }
        pwt.use_weight = i32::from(pwt.use_weight != 0 || pwt.use_weight_chroma != 0);
        Ok(())
    }

    /// `ff_h264_decode_ref_pic_marking`: an IDR's long-term flag, or the
    /// slice's operations until `MMCO_END`. An operation FFmpeg refuses
    /// stops the reading, the operations before it kept.
    #[allow(clippy::cast_possible_wrap, reason = "FFmpeg's unsigned taken as int")]
    fn decode_ref_pic_marking(&mut self, gb: &mut Bits<'_>, nal: &Nal) -> Result<(), SliceError> {
        let mut nb_mmco = 0;
        if nal.kind == NAL_IDR_SLICE {
            gb.skip(1); // no_output_of_prior_pics_flag ("broken_link")
            if gb.flag() {
                // long_term_reference_flag
                self.mmco[0].opcode = MMCO_LONG;
                self.mmco[0].long_arg = 0;
                nb_mmco = 1;
            }
            self.explicit_ref_marking = 1;
        } else {
            self.explicit_ref_marking = gb.bit() as i32;
            if self.explicit_ref_marking != 0 {
                let mut i = 0;
                while i < MAX_MMCO_COUNT {
                    let opcode = gb.ue_31();
                    self.mmco[i].opcode = opcode;
                    if opcode == MMCO_SHORT2UNUSED || opcode == MMCO_SHORT2LONG {
                        let diff = gb.ue_long();
                        self.mmco[i].short_pic_num = ((self.curr_pic_num as u32)
                            .wrapping_sub(diff)
                            .wrapping_sub(1)
                            & (self.max_pic_num as u32).wrapping_sub(1))
                            as i32;
                    }
                    if matches!(
                        opcode,
                        MMCO_SHORT2LONG | MMCO_LONG2UNUSED | MMCO_LONG | MMCO_SET_MAX_LONG
                    ) {
                        let long_arg = gb.ue_31();
                        let field = self.picture_structure != PICT_FRAME;
                        if long_arg >= 32
                            || (long_arg >= 16
                                && !(opcode == MMCO_SET_MAX_LONG && long_arg == 16)
                                && !(opcode == MMCO_LONG2UNUSED && field))
                        {
                            self.nb_mmco = i;
                            return Err(SliceError);
                        }
                        self.mmco[i].long_arg = long_arg as i32;
                    }
                    if opcode > MMCO_LONG {
                        self.nb_mmco = i;
                        return Err(SliceError);
                    }
                    if opcode == MMCO_END {
                        break;
                    }
                    i += 1;
                }
                nb_mmco = i;
            }
        }
        self.nb_mmco = nb_mmco;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
