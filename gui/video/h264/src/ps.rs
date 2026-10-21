//! Sequence and picture parameter sets, read as FFmpeg reads them
//! (`h264_ps.c`, with `h2645_vui.c` for the video usability information).
//!
//! A sequence parameter set (SPS) says what a stream's pictures are -- their
//! size in macroblocks, how frames are numbered and ordered, whether fields
//! are coded, the bit depth and chroma format, the scaling lists -- and a
//! picture parameter set (PPS) how one picture's slices are coded: CABAC or
//! CAVLC, the default references and QP, weighted prediction, the 8x8
//! transform. Each is kept by its id in [`ParamSets`] until another with the
//! same id replaces it.
//!
//! Values are kept in FFmpeg's types -- `int` as `i32`, `unsigned` as `u32`
//! -- and worked with its arithmetic, wrapping where it wraps: a damaged
//! set is refused exactly where FFmpeg refuses it and kept where FFmpeg
//! keeps it, which is part of what a damaged stream decodes to.

#![allow(
    clippy::indexing_slicing,
    reason = "indices are list positions under the fixed sizes they index: scaling lists by \
              scans of their own length, rows by ids range-checked first, QPs under QP_MAX_NUM"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on values it has bounded first (ids, depths, sizes in \
              macroblocks under INT_MAX / 16); unbounded sums are written wrapping"
)]

use std::sync::Arc;

use crate::Error;
use crate::tables::{
    CHROMA_QP, DEFAULT_SCALING4, DEFAULT_SCALING8, DEQUANT4_COEFF_INIT, DEQUANT8_COEFF_INIT,
    DEQUANT8_COEFF_INIT_SCAN, LEVEL_MAX_DPB_MBS, PIXEL_ASPECT, QP_MAX_NUM, QUANT_DIV6, QUANT_REM6,
    ZIGZAG_DIRECT, ZIGZAG_SCAN,
};
use ffbits::Bits;

/// How many sequence parameter sets a stream may have (`MAX_SPS_COUNT`).
pub(crate) const MAX_SPS_COUNT: usize = 32;
/// How many picture parameter sets (`MAX_PPS_COUNT`).
pub(crate) const MAX_PPS_COUNT: usize = 256;
/// The most frames a decoded picture buffer holds (`H264_MAX_DPB_FRAMES`).
pub(crate) const MAX_DPB_FRAMES: i32 = 16;
/// The bound of `log2_max_frame_num` (`MAX_LOG2_MAX_FRAME_NUM`).
const MAX_LOG2_MAX_FRAME_NUM: i32 = 12 + 4;
const MIN_LOG2_MAX_FRAME_NUM: i32 = 4;
/// The most bytes of a set kept whole (`sizeof(sps->data)`).
const DATA_MAX: usize = 4096;

/// `aspect_ratio_idc` naming a pixel shape given in full.
const EXTENDED_SAR: u32 = 255;

/// `AVCOL_SPC_UNSPECIFIED`, `AVCOL_PRI_UNSPECIFIED`, `AVCOL_TRC_UNSPECIFIED`.
pub(crate) const COLOUR_UNSPECIFIED: u32 = 2;
/// `AVCHROMA_LOC_UNSPECIFIED` and `AVCHROMA_LOC_LEFT`.
pub(crate) const CHROMA_LOC_UNSPECIFIED: u32 = 0;
pub(crate) const CHROMA_LOC_LEFT: u32 = 1;

/// What the video usability information says about pictures (`H2645VUI`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Vui {
    /// The pixel's shape, numerator and denominator (`sar`).
    pub(crate) sar: (i32, i32),
    pub(crate) aspect_ratio_idc: u32,
    pub(crate) aspect_ratio_info_present_flag: u32,
    pub(crate) overscan_info_present_flag: u32,
    pub(crate) video_signal_type_present_flag: u32,
    /// 1 for full range, 0 for limited, -1 for unsaid.
    pub(crate) video_full_range_flag: i32,
    pub(crate) colour_description_present_flag: u32,
    /// `AVColorPrimaries`: an unknown code is "unspecified".
    pub(crate) colour_primaries: u32,
    /// `AVColorTransferCharacteristic`.
    pub(crate) transfer_characteristics: u32,
    /// `AVColorSpace`: which matrix turns the samples into RGB.
    pub(crate) matrix_coeffs: u32,
    pub(crate) chroma_loc_info_present_flag: u32,
    pub(crate) chroma_sample_loc_type_top_field: u32,
    /// `AVChromaLocation`.
    pub(crate) chroma_location: u32,
}

/// A sequence parameter set (`SPS`), field for field as FFmpeg keeps it,
/// less what nothing in its software decoder reads: those syntax elements
/// are read past, by name, where they stand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sps {
    pub(crate) sps_id: u32,
    pub(crate) profile_idc: i32,
    pub(crate) level_idc: i32,
    pub(crate) chroma_format_idc: i32,
    /// `qpprime_y_zero_transform_bypass_flag`: lossless macroblocks at QP 0.
    pub(crate) transform_bypass: i32,
    pub(crate) log2_max_frame_num: i32,
    pub(crate) poc_type: i32,
    /// `log2_max_pic_order_cnt_lsb_minus4 + 4`.
    pub(crate) log2_max_poc_lsb: i32,
    pub(crate) delta_pic_order_always_zero_flag: i32,
    pub(crate) offset_for_non_ref_pic: i32,
    pub(crate) offset_for_top_to_bottom_field: i32,
    /// `num_ref_frames_in_pic_order_cnt_cycle`.
    pub(crate) poc_cycle_length: i32,
    /// `num_ref_frames`.
    pub(crate) ref_frame_count: i32,
    pub(crate) gaps_in_frame_num_allowed_flag: i32,
    /// The width in macroblocks.
    pub(crate) mb_width: i32,
    /// The height in macroblocks of a frame -- twice a field's when fields
    /// are coded.
    pub(crate) mb_height: i32,
    pub(crate) frame_mbs_only_flag: i32,
    /// `mb_adaptive_frame_field_flag`.
    pub(crate) mb_aff: i32,
    pub(crate) direct_8x8_inference_flag: i32,
    /// `frame_cropping_flag`.
    pub(crate) crop: i32,
    /// The crop, already in luma samples.
    pub(crate) crop_left: u32,
    pub(crate) crop_right: u32,
    pub(crate) crop_top: u32,
    pub(crate) crop_bottom: u32,
    pub(crate) vui_parameters_present_flag: i32,
    pub(crate) vui: Vui,
    pub(crate) timing_info_present_flag: i32,
    pub(crate) num_units_in_tick: u32,
    pub(crate) time_scale: u32,
    pub(crate) offset_for_ref_frame: Box<[i32; 256]>,
    pub(crate) bitstream_restriction_flag: i32,
    pub(crate) num_reorder_frames: i32,
    pub(crate) scaling_matrix_present: i32,
    pub(crate) scaling_matrix4: [[u8; 16]; 6],
    pub(crate) scaling_matrix8: Box<[[u8; 64]; 6]>,
    pub(crate) nal_hrd_parameters_present_flag: i32,
    pub(crate) vcl_hrd_parameters_present_flag: i32,
    pub(crate) pic_struct_present_flag: i32,
    pub(crate) time_offset_length: i32,
    pub(crate) cpb_removal_delay_length: i32,
    pub(crate) dpb_output_delay_length: i32,
    pub(crate) bit_depth_luma: i32,
    pub(crate) bit_depth_chroma: i32,
    /// `residual_colour_transform_flag` (separate colour planes).
    pub(crate) residual_color_transform_flag: i32,
    /// `constraint_set0_flag` to `constraint_set5_flag`, bit 0 first.
    pub(crate) constraint_set_flags: i32,
    /// The set's bytes as it was read (`data`, `data_size`): part of what
    /// makes one set the same as another.
    pub(crate) data: Vec<u8>,
}

impl Sps {
    /// A set with FFmpeg's starting values (`av_refstruct_allocz`, then
    /// the defaults `ff_h264_decode_seq_parameter_set` sets before reading).
    pub(crate) fn zeroed() -> Self {
        Self {
            sps_id: 0,
            profile_idc: 0,
            level_idc: 0,
            chroma_format_idc: 0,
            transform_bypass: 0,
            log2_max_frame_num: 0,
            poc_type: 0,
            log2_max_poc_lsb: 0,
            delta_pic_order_always_zero_flag: 0,
            offset_for_non_ref_pic: 0,
            offset_for_top_to_bottom_field: 0,
            poc_cycle_length: 0,
            ref_frame_count: 0,
            gaps_in_frame_num_allowed_flag: 0,
            mb_width: 0,
            mb_height: 0,
            frame_mbs_only_flag: 0,
            mb_aff: 0,
            direct_8x8_inference_flag: 0,
            crop: 0,
            crop_left: 0,
            crop_right: 0,
            crop_top: 0,
            crop_bottom: 0,
            vui_parameters_present_flag: 0,
            vui: Vui::default(),
            timing_info_present_flag: 0,
            num_units_in_tick: 0,
            time_scale: 0,
            offset_for_ref_frame: Box::new([0; 256]),
            bitstream_restriction_flag: 0,
            num_reorder_frames: 0,
            scaling_matrix_present: 0,
            scaling_matrix4: [[0; 16]; 6],
            scaling_matrix8: Box::new([[0; 64]; 6]),
            nal_hrd_parameters_present_flag: 0,
            vcl_hrd_parameters_present_flag: 0,
            pic_struct_present_flag: 0,
            time_offset_length: 0,
            cpb_removal_delay_length: 0,
            dpb_output_delay_length: 0,
            bit_depth_luma: 0,
            bit_depth_chroma: 0,
            residual_color_transform_flag: 0,
            constraint_set_flags: 0,
            data: Vec::new(),
        }
    }
}

/// How many QPs each list has factors for: row `list * QPS + qp` of a
/// dequantization table.
const QPS: usize = QP_MAX_NUM + 1;

/// A picture parameter set (`PPS`).
///
/// FFmpeg's, but for what only its hardware decoders read: the set's own
/// bytes and the slice group map type. The decoder reads the scaling lists
/// through the dequantization tables made from them. Six fields only the
/// tests read, holding the parse to FFmpeg's `trace_headers`: the id, the
/// slice group count (always 1, slice groups being refused), `init_qs`,
/// the two scaling lists, and `chroma_qp_diff`, which FFmpeg reads only to
/// pick its assembly's loop filter.
#[derive(Clone, Debug)]
#[allow(
    dead_code,
    reason = "the six fields named above, read only by the tests that hold the parse to \
              FFmpeg's trace_headers"
)]
pub(crate) struct Pps {
    pub(crate) pps_id: u32,
    pub(crate) sps_id: u32,
    /// `entropy_coding_mode_flag`: CABAC, else CAVLC.
    pub(crate) cabac: u32,
    /// `bottom_field_pic_order_in_frame_present_flag`.
    pub(crate) pic_order_present: u32,
    /// `num_slice_groups_minus1 + 1`, as FFmpeg's int adds it.
    pub(crate) slice_group_count: i32,
    /// `num_ref_idx_l0/l1_default_active_minus1 + 1`.
    pub(crate) ref_count: [u32; 2],
    pub(crate) weighted_pred: u32,
    pub(crate) weighted_bipred_idc: u32,
    /// `pic_init_qp_minus26 + 26`, raised by the bit depth's QP offset.
    pub(crate) init_qp: i32,
    pub(crate) init_qs: i32,
    pub(crate) chroma_qp_index_offset: [i32; 2],
    pub(crate) deblocking_filter_parameters_present: u32,
    pub(crate) constrained_intra_pred: u32,
    pub(crate) redundant_pic_cnt_present: u32,
    pub(crate) transform_8x8_mode: u32,
    pub(crate) scaling_matrix4: [[u8; 16]; 6],
    pub(crate) scaling_matrix8: Box<[[u8; 64]; 6]>,
    /// The chroma QP for each luma QP, for Cb and Cr, the offsets applied.
    pub(crate) chroma_qp_table: [[u8; QP_MAX_NUM + 1]; 2],
    /// Whether Cb's and Cr's offsets differ.
    pub(crate) chroma_qp_diff: i32,
    /// The dequantization factors, laid out as the inverse transform reads
    /// them and as FFmpeg's `PPS` holds them, end to end: six 4x4 buffers
    /// of a row per QP (`dequant4_buffer`), then six 8x8 ones
    /// (`dequant8_buffer`). Only the QPs the bit depth reaches are filled,
    /// and a list whose scaling matrix is an earlier list's reads that
    /// list's buffer, leaving its own zero: a damaged CAVLC block can read
    /// its row's factors past the row's end (see [`Pps::dequant4_from`]),
    /// and gets what FFmpeg's memory holds there.
    pub(crate) dequant: Box<[u32]>,
    /// The 4x4 buffer each list reads (`dequant4_coeff[list]`).
    pub(crate) dequant4_list: [usize; 6],
    /// The 8x8 buffer each list reads, with the 8x8 transform only:
    /// FFmpeg's pointers are null without it, and its buffers zero.
    pub(crate) dequant8_list: Option<[usize; 6]>,
    /// The sequence parameter set the picture's was read against.
    pub(crate) sps: Arc<Sps>,
}

/// Where the 8x8 buffers start in [`Pps::dequant`].
const DEQUANT8_AT: usize = 6 * QPS * 16;

impl Pps {
    /// List `list`'s 4x4 factors at `qp` (`dequant4_coeff[list][qp]`).
    pub(crate) fn dequant4(&self, list: usize, qp: usize) -> Option<&[u32; 16]> {
        self.dequant4_from(list, qp)?.first_chunk()
    }

    /// The factors from list `list`'s 4x4 row at `qp` to the end of the
    /// tables: what FFmpeg's `qmul[k]` reads for every `k`, those past the
    /// row's 16 coming from the rows after it -- as a damaged CAVLC block's
    /// scan reads them (`decode_residual`).
    pub(crate) fn dequant4_from(&self, list: usize, qp: usize) -> Option<&[u32]> {
        if qp >= QPS {
            return None;
        }
        let buffer = *self.dequant4_list.get(list)?;
        self.dequant.get((buffer * QPS + qp) * 16..)
    }

    /// List `list`'s 8x8 factors at `qp` (`dequant8_coeff[list][qp]`).
    pub(crate) fn dequant8(&self, list: usize, qp: usize) -> Option<&[u32; 64]> {
        if qp >= QPS {
            return None;
        }
        let buffer = *self.dequant8_list.as_ref()?.get(list)?;
        self.dequant
            .get(DEQUANT8_AT + (buffer * QPS + qp) * 64..)?
            .first_chunk()
    }
}

/// Every parameter set a stream has given, by id, and those the current
/// picture uses (`H264ParamSets`).
#[derive(Clone, Debug)]
pub(crate) struct ParamSets {
    pub(crate) sps_list: Vec<Option<Arc<Sps>>>,
    pub(crate) pps_list: Vec<Option<Arc<Pps>>>,
    /// Which ids' last picture parameter set was refused as something
    /// FFmpeg does not decode ([`PsError::Unsupported`]): what a slice that
    /// finds no set under its id is told.
    pub(crate) unsupported_pps: Vec<bool>,
    /// The picture parameter set in use.
    pub(crate) pps: Option<Arc<Pps>>,
    /// Its sequence parameter set.
    pub(crate) sps: Option<Arc<Sps>>,
}

impl Default for ParamSets {
    fn default() -> Self {
        Self {
            sps_list: vec![None; MAX_SPS_COUNT],
            pps_list: vec![None; MAX_PPS_COUNT],
            unsupported_pps: vec![false; MAX_PPS_COUNT],
            pps: None,
            sps: None,
        }
    }
}

/// Why a parameter set was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PsError {
    /// Damaged, or out of range (`AVERROR_INVALIDDATA`).
    Invalid,
    /// Valid, but a feature FFmpeg does not decode -- slice groups, an 11-
    /// or 13-bit depth (`AVERROR_PATCHWELCOME`).
    Unsupported,
}

impl From<PsError> for Error {
    fn from(e: PsError) -> Self {
        match e {
            PsError::Invalid => Self::InvalidData,
            PsError::Unsupported => Self::Unsupported,
        }
    }
}

/// `(i32)(se + 26U + offset)`: FFmpeg's unsigned sum, taken back as int.
#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "FFmpeg's wrap"
)]
fn plus_26(se: i32, offset: i32) -> i32 {
    (se as u32).wrapping_add(26).wrapping_add(offset as u32) as i32
}

/// The codes `colour_primaries` may name; any other is "unspecified"
/// (`av_color_primaries_name` has none for it, or it is reserved).
fn primaries_known(v: u32) -> bool {
    matches!(v, 1 | 2 | 4..=12 | 22)
}

fn transfer_known(v: u32) -> bool {
    matches!(v, 1 | 2 | 4..=18)
}

fn matrix_known(v: u32) -> bool {
    matches!(v, 0..=2 | 4..=17)
}

/// The video usability information both H.264 and HEVC begin with
/// (`ff_h2645_decode_common_vui_params`).
fn decode_common_vui(gb: &mut Bits<'_>, vui: &mut Vui) {
    vui.aspect_ratio_info_present_flag = gb.bit();
    if vui.aspect_ratio_info_present_flag != 0 {
        vui.aspect_ratio_idc = gb.bits(8);
        if let Some(&sar) = PIXEL_ASPECT.get(vui.aspect_ratio_idc as usize) {
            vui.sar = sar;
        } else if vui.aspect_ratio_idc == EXTENDED_SAR {
            #[allow(clippy::cast_possible_wrap, reason = "16 bits")]
            let sar = (gb.bits(16) as i32, gb.bits(16) as i32);
            vui.sar = sar;
        }
        // An unknown index leaves the shape as it was.
    } else {
        vui.sar = (0, 1);
    }

    vui.overscan_info_present_flag = gb.bit();
    if vui.overscan_info_present_flag != 0 {
        gb.bit(); // overscan_appropriate_flag
    }

    vui.video_signal_type_present_flag = gb.bit();
    if vui.video_signal_type_present_flag != 0 {
        gb.bits(3); // video_format
        #[allow(clippy::cast_possible_wrap, reason = "one bit")]
        let full = gb.bit() as i32;
        vui.video_full_range_flag = full;
        vui.colour_description_present_flag = gb.bit();
        if vui.colour_description_present_flag != 0 {
            vui.colour_primaries = gb.bits(8);
            vui.transfer_characteristics = gb.bits(8);
            vui.matrix_coeffs = gb.bits(8);
            if !primaries_known(vui.colour_primaries) {
                vui.colour_primaries = COLOUR_UNSPECIFIED;
            }
            if !transfer_known(vui.transfer_characteristics) {
                vui.transfer_characteristics = COLOUR_UNSPECIFIED;
            }
            if !matrix_known(vui.matrix_coeffs) {
                vui.matrix_coeffs = COLOUR_UNSPECIFIED;
            }
        }
    }

    vui.chroma_loc_info_present_flag = gb.bit();
    if vui.chroma_loc_info_present_flag != 0 {
        vui.chroma_sample_loc_type_top_field = gb.ue_31();
        gb.ue_31(); // chroma_sample_loc_type_bottom_field
        vui.chroma_location = if vui.chroma_sample_loc_type_top_field <= 5 {
            vui.chroma_sample_loc_type_top_field + 1
        } else {
            CHROMA_LOC_UNSPECIFIED
        };
    } else {
        vui.chroma_location = CHROMA_LOC_LEFT;
    }
}

/// `decode_hrd_parameters`: the hypothetical reference decoder's
/// parameters, of which what reading the picture timing SEI needs is kept
/// -- its fields' lengths. The count of buffers is checked; it, the bit
/// rates and buffer sizes, and the buffering period's delay length FFmpeg
/// keeps beside them only its parser and hardware decoders read.
#[allow(clippy::cast_possible_wrap, reason = "fields of 5 bits")]
fn decode_hrd_parameters(gb: &mut Bits<'_>, sps: &mut Sps) -> Result<(), PsError> {
    let cpb_count = gb.ue_31() + 1;
    if cpb_count > 32 {
        return Err(PsError::Invalid);
    }
    gb.bits(4); // bit_rate_scale
    gb.bits(4); // cpb_size_scale
    for _ in 0..cpb_count {
        gb.ue_long(); // bit_rate_value_minus1
        gb.ue_long(); // cpb_size_value_minus1
        gb.bit(); // cbr_flag
    }
    gb.skip(5); // initial_cpb_removal_delay_length_minus1
    sps.cpb_removal_delay_length = gb.bits(5) as i32 + 1;
    sps.dpb_output_delay_length = gb.bits(5) as i32 + 1;
    sps.time_offset_length = gb.bits(5) as i32;
    Ok(())
}

/// `decode_vui_parameters`. A VUI that runs out of bits where its timing
/// would begin is taken as it stands.
#[allow(clippy::cast_possible_wrap, reason = "flags and Golomb codes under 33")]
fn decode_vui_parameters(gb: &mut Bits<'_>, sps: &mut Sps) -> Result<(), PsError> {
    decode_common_vui(gb, &mut sps.vui);

    if gb.show(1) != 0 && gb.left() < 10 {
        // "Truncated VUI".
        return Ok(());
    }

    sps.timing_info_present_flag = gb.bit() as i32;
    if sps.timing_info_present_flag != 0 {
        let num_units_in_tick = gb.bits_long(32);
        let time_scale = gb.bits_long(32);
        if num_units_in_tick == 0 || time_scale == 0 {
            sps.timing_info_present_flag = 0;
        } else {
            sps.num_units_in_tick = num_units_in_tick;
            sps.time_scale = time_scale;
        }
        gb.bit(); // fixed_frame_rate_flag
    }

    sps.nal_hrd_parameters_present_flag = gb.bit() as i32;
    if sps.nal_hrd_parameters_present_flag != 0 {
        decode_hrd_parameters(gb, sps)?;
    }
    sps.vcl_hrd_parameters_present_flag = gb.bit() as i32;
    if sps.vcl_hrd_parameters_present_flag != 0 {
        decode_hrd_parameters(gb, sps)?;
    }
    if sps.nal_hrd_parameters_present_flag != 0 || sps.vcl_hrd_parameters_present_flag != 0 {
        gb.bit(); // low_delay_hrd_flag
    }
    sps.pic_struct_present_flag = gb.bit() as i32;
    if gb.left() == 0 {
        return Ok(());
    }
    sps.bitstream_restriction_flag = gb.bit() as i32;
    if sps.bitstream_restriction_flag != 0 {
        gb.bit(); // motion_vectors_over_pic_boundaries_flag
        gb.ue_31(); // max_bytes_per_pic_denom
        gb.ue_31(); // max_bits_per_mb_denom
        gb.ue_31(); // log2_max_mv_length_horizontal
        gb.ue_31(); // log2_max_mv_length_vertical
        sps.num_reorder_frames = gb.ue_31() as i32;
        gb.ue_31(); // max_dec_frame_buffering

        if gb.left() < 0 {
            sps.num_reorder_frames = 0;
            sps.bitstream_restriction_flag = 0;
        }

        #[allow(clippy::cast_sign_loss, reason = "FFmpeg's (unsigned) comparison")]
        if sps.num_reorder_frames as u32 > 16 {
            sps.num_reorder_frames = 16;
            return Err(PsError::Invalid);
        }
    }
    Ok(())
}

/// One scaling list (`decode_scaling_list`): absent, a copy of `fallback`;
/// present but starting with a zero delta, a copy of `jvt`; otherwise its
/// deltas, in zigzag order. FFmpeg also notes which lists were present, a
/// mask only its hardware decoders read; it is not kept here.
fn decode_scaling_list(
    gb: &mut Bits<'_>,
    factors: &mut [u8],
    jvt: &[u8],
    fallback: &[u8],
) -> Result<(), PsError> {
    let size = factors.len();
    let scan: &[u8] = if size == 16 {
        &ZIGZAG_SCAN
    } else {
        &ZIGZAG_DIRECT
    };
    if gb.bit() == 0 {
        factors.copy_from_slice(&fallback[..size]);
        return Ok(());
    }
    let (mut last, mut next) = (8i32, 8i32);
    for i in 0..size {
        if next != 0 {
            let v = gb.se();
            if !(-128..=127).contains(&v) {
                return Err(PsError::Invalid);
            }
            next = (last + v) & 0xff;
        }
        if i == 0 && next == 0 {
            factors.copy_from_slice(&jvt[..size]);
            break;
        }
        if next != 0 {
            last = next;
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "0 to 255"
        )]
        let f = last as u8;
        factors[usize::from(scan[i])] = f;
    }
    Ok(())
}

/// The scaling lists of a set (`decode_scaling_matrices`): `Ok(1)` when an
/// SPS's were read, `Ok(0)` when none were or a PPS's were, an error when a
/// delta is out of range -- reading on through every list either way, as
/// FFmpeg's `ret |=` does.
fn decode_scaling_matrices(
    gb: &mut Bits<'_>,
    sps: &Sps,
    transform_8x8_mode: bool,
    is_sps: bool,
    present: bool,
    m4: &mut [[u8; 16]; 6],
    m8: &mut [[u8; 64]; 6],
) -> Result<i32, PsError> {
    let fallback_sps = !is_sps && sps.scaling_matrix_present != 0;
    let fb4: [[u8; 16]; 2] = if fallback_sps {
        [sps.scaling_matrix4[0], sps.scaling_matrix4[3]]
    } else {
        DEFAULT_SCALING4
    };
    let fb8: [[u8; 64]; 2] = if fallback_sps {
        [sps.scaling_matrix8[0], sps.scaling_matrix8[3]]
    } else {
        DEFAULT_SCALING8
    };
    if !present {
        return Ok(0);
    }
    // (list, default list, list it falls back to when absent), in FFmpeg's
    // order. A later list's fallback is an earlier list as just read, so
    // each is copied out when its turn comes.
    let mut failed = false;
    for (i, jvt, fallback) in [
        (0, 0, None),    // intra Y
        (1, 0, Some(0)), // intra Cr
        (2, 0, Some(1)), // intra Cb
        (3, 1, None),    // inter Y
        (4, 1, Some(3)), // inter Cr
        (5, 1, Some(4)), // inter Cb
    ] {
        let fb = fallback.map_or(fb4[jvt], |k: usize| m4[k]);
        failed |= decode_scaling_list(gb, &mut m4[i], &DEFAULT_SCALING4[jvt], &fb).is_err();
    }
    if is_sps || transform_8x8_mode {
        let mut lists = vec![(0, 0, None), (3, 1, None)]; // intra Y, inter Y
        if sps.chroma_format_idc == 3 {
            lists.extend([
                (1, 0, Some(0)), // intra Cr
                (4, 1, Some(3)), // inter Cr
                (2, 0, Some(1)), // intra Cb
                (5, 1, Some(4)), // inter Cb
            ]);
        }
        for (i, jvt, fallback) in lists {
            let fb = fallback.map_or(fb8[jvt], |k: usize| m8[k]);
            failed |= decode_scaling_list(gb, &mut m8[i], &DEFAULT_SCALING8[jvt], &fb).is_err();
        }
    }
    if failed {
        return Err(PsError::Invalid);
    }
    Ok(i32::from(is_sps))
}

/// Whether a picture `w` by `h` is one FFmpeg will allocate
/// (`av_image_check_size` with no pixel format: eight bytes a pixel, plus
/// a margin, within an `int`).
fn image_size_ok(w: u64, h: u64) -> bool {
    let stride = 8 * w + 128 * 8;
    w != 0 && h != 0 && stride < i32::MAX as u64 && stride * (h + 128) < i32::MAX as u64
}

/// The bytes a set keeps of itself (`data`, `data_size`): the reader's
/// buffer, rounded up to whole bytes, at most 4096, with the stop bit put
/// back when the syntax ended on a byte boundary.
fn keep_data(gb: &Bits<'_>) -> Vec<u8> {
    let size = (gb.byte_size(true) as usize).min(DATA_MAX);
    let mut data: Vec<u8> = (0..size)
        .map(|k| gb.buffer().get(k).copied().unwrap_or(0))
        .collect();
    if gb.size() & 7 == 0 && data.len() < DATA_MAX {
        data.push(0x80);
    }
    data
}

/// Read a sequence parameter set from where `gb` stands and keep it by its
/// id (`ff_h264_decode_seq_parameter_set`). A set the same as the one
/// already kept under its id -- every field, and its bytes -- leaves that
/// one in place, so whatever holds it still holds the current set.
///
/// With `ignore_truncation`, a set that reads past its end is kept anyway.
#[allow(
    clippy::too_many_lines,
    reason = "one function in FFmpeg, kept whole so it reads line for line against it"
)]
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int and unsigned, converted where it converts"
)]
pub(crate) fn decode_sps(
    gb: &mut Bits<'_>,
    ps: &mut ParamSets,
    ignore_truncation: bool,
) -> Result<(), PsError> {
    let mut sps = Sps::zeroed();
    sps.data = keep_data(gb);

    let profile_idc = gb.bits(8) as i32;
    let mut constraint_set_flags = 0;
    for k in 0..6 {
        constraint_set_flags |= (gb.bit() as i32) << k;
    }
    gb.skip(2); // reserved_zero_2bits
    let level_idc = gb.bits(8) as i32;
    let sps_id = gb.ue_31();
    if sps_id as usize >= MAX_SPS_COUNT {
        return Err(PsError::Invalid);
    }

    sps.sps_id = sps_id;
    sps.time_offset_length = 24;
    sps.profile_idc = profile_idc;
    sps.constraint_set_flags = constraint_set_flags;
    sps.level_idc = level_idc;
    sps.vui.video_full_range_flag = -1;
    sps.scaling_matrix4 = [[16; 16]; 6];
    sps.scaling_matrix8 = Box::new([[16; 64]; 6]);
    sps.scaling_matrix_present = 0;
    sps.vui.matrix_coeffs = COLOUR_UNSPECIFIED;

    if matches!(
        sps.profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 144
    ) {
        sps.chroma_format_idc = gb.ue_31() as i32;
        if sps.chroma_format_idc as u32 > 3 {
            return Err(PsError::Invalid);
        } else if sps.chroma_format_idc == 3 {
            sps.residual_color_transform_flag = gb.bit() as i32;
            if sps.residual_color_transform_flag != 0 {
                // Separate colour planes, which FFmpeg does not decode.
                return Err(PsError::Invalid);
            }
        }
        sps.bit_depth_luma = gb.ue_31() as i32 + 8;
        sps.bit_depth_chroma = gb.ue_31() as i32 + 8;
        if sps.bit_depth_chroma != sps.bit_depth_luma {
            return Err(PsError::Invalid);
        }
        if !(8..=14).contains(&sps.bit_depth_luma) || !(8..=14).contains(&sps.bit_depth_chroma) {
            return Err(PsError::Invalid);
        }
        sps.transform_bypass = gb.bit() as i32;
        let present = gb.flag();
        let mut m4 = sps.scaling_matrix4;
        let mut m8 = *sps.scaling_matrix8;
        let ret = decode_scaling_matrices(gb, &sps, false, true, present, &mut m4, &mut m8)?;
        sps.scaling_matrix4 = m4;
        sps.scaling_matrix8 = Box::new(m8);
        sps.scaling_matrix_present |= ret;
    } else {
        sps.chroma_format_idc = 1;
        sps.bit_depth_luma = 8;
        sps.bit_depth_chroma = 8;
    }

    let log2_max_frame_num_minus4 = gb.ue_31() as i32;
    if !(MIN_LOG2_MAX_FRAME_NUM - 4..=MAX_LOG2_MAX_FRAME_NUM - 4)
        .contains(&log2_max_frame_num_minus4)
    {
        return Err(PsError::Invalid);
    }
    sps.log2_max_frame_num = log2_max_frame_num_minus4 + 4;

    sps.poc_type = gb.ue_31() as i32;
    if sps.poc_type == 0 {
        let t = gb.ue_31();
        if t > 12 {
            return Err(PsError::Invalid);
        }
        sps.log2_max_poc_lsb = t as i32 + 4;
    } else if sps.poc_type == 1 {
        sps.delta_pic_order_always_zero_flag = gb.bit() as i32;
        sps.offset_for_non_ref_pic = gb.se_long();
        sps.offset_for_top_to_bottom_field = gb.se_long();
        if sps.offset_for_non_ref_pic == i32::MIN || sps.offset_for_top_to_bottom_field == i32::MIN
        {
            return Err(PsError::Invalid);
        }
        sps.poc_cycle_length = gb.ue();
        if sps.poc_cycle_length as u32 >= 256 {
            return Err(PsError::Invalid);
        }
        for i in 0..sps.poc_cycle_length as usize {
            sps.offset_for_ref_frame[i] = gb.se_long();
            if sps.offset_for_ref_frame[i] == i32::MIN {
                return Err(PsError::Invalid);
            }
        }
    } else if sps.poc_type != 2 {
        return Err(PsError::Invalid);
    }

    sps.ref_frame_count = gb.ue_31() as i32;
    if sps.ref_frame_count > MAX_DPB_FRAMES {
        return Err(PsError::Invalid);
    }
    sps.gaps_in_frame_num_allowed_flag = gb.bit() as i32;
    sps.mb_width = gb.ue().wrapping_add(1);
    sps.mb_height = gb.ue().wrapping_add(1);

    sps.frame_mbs_only_flag = gb.bit() as i32;

    if sps.mb_height as u32 >= (i32::MAX as u32) / 2 {
        return Err(PsError::Invalid);
    }
    sps.mb_height *= 2 - sps.frame_mbs_only_flag;

    sps.mb_aff = if sps.frame_mbs_only_flag == 0 {
        gb.bit() as i32
    } else {
        0
    };

    if sps.mb_width as u32 >= (i32::MAX as u32) / 16
        || sps.mb_height as u32 >= (i32::MAX as u32) / 16
        || !image_size_ok(
            16 * u64::from(sps.mb_width as u32),
            16 * u64::from(sps.mb_height as u32),
        )
    {
        return Err(PsError::Invalid);
    }

    sps.direct_8x8_inference_flag = gb.bit() as i32;

    sps.crop = gb.bit() as i32;
    if sps.crop != 0 {
        let crop_left = gb.ue() as u32;
        let crop_right = gb.ue() as u32;
        let crop_top = gb.ue() as u32;
        let crop_bottom = gb.ue() as u32;
        let width = 16 * sps.mb_width;
        let height = 16 * sps.mb_height;

        let vsub = i32::from(sps.chroma_format_idc == 1);
        let hsub = i32::from(sps.chroma_format_idc == 1 || sps.chroma_format_idc == 2);
        let step_x = 1u32 << hsub;
        let step_y = ((2 - sps.frame_mbs_only_flag) << vsub) as u32;

        let limit_x = (i32::MAX as u32) / 4 / step_x;
        let limit_y = (i32::MAX as u32) / 4 / step_y;
        // Unsigned, as C compares an unsigned sum with the int width.
        if crop_left > limit_x
            || crop_right > limit_x
            || crop_top > limit_y
            || crop_bottom > limit_y
            || (crop_left + crop_right) * step_x >= width as u32
            || (crop_top + crop_bottom) * step_y >= height as u32
        {
            return Err(PsError::Invalid);
        }
        sps.crop_left = crop_left * step_x;
        sps.crop_right = crop_right * step_x;
        sps.crop_top = crop_top * step_y;
        sps.crop_bottom = crop_bottom * step_y;
    } else {
        sps.crop_left = 0;
        sps.crop_right = 0;
        sps.crop_top = 0;
        sps.crop_bottom = 0;
        sps.crop = 0;
    }

    sps.vui_parameters_present_flag = gb.bit() as i32;
    if sps.vui_parameters_present_flag != 0 {
        decode_vui_parameters(gb, &mut sps)?;
    }

    if gb.left() < 0 && !ignore_truncation {
        return Err(PsError::Invalid);
    }

    // Without the reorder depth, the level's buffer size bounds it.
    if sps.bitstream_restriction_flag == 0 && sps.ref_frame_count != 0 {
        sps.num_reorder_frames = MAX_DPB_FRAMES - 1;
        if let Some(&(_, mbs)) = LEVEL_MAX_DPB_MBS.iter().find(|(l, _)| *l == sps.level_idc) {
            sps.num_reorder_frames =
                (mbs / (sps.mb_width * sps.mb_height)).min(sps.num_reorder_frames);
        }
    }

    if sps.vui.sar.1 == 0 {
        sps.vui.sar.1 = 1;
    }

    let id = sps_id as usize;
    if ps.sps_list[id].as_deref() != Some(&sps) {
        ps.sps_list[id] = Some(Arc::new(sps));
    }
    Ok(())
}

/// `more_rbsp_data_in_pps`: Baseline, Main and Extended streams that say
/// they keep to their profile carry no PPS extension, whatever bits follow.
fn more_rbsp_data_in_pps(sps: &Sps) -> bool {
    !(matches!(sps.profile_idc, 66 | 77 | 88) && (sps.constraint_set_flags & 7) != 0)
}

/// `build_qp_table`: the chroma QP for every luma QP, offset by `index`.
fn build_qp_table(index: i32, depth: i32) -> [u8; QP_MAX_NUM + 1] {
    let max_qp = 51 + 6 * (depth - 8);
    let mut t = [0u8; QP_MAX_NUM + 1];
    for (i, slot) in t.iter_mut().enumerate().take((max_qp + 1) as usize) {
        #[allow(
            clippy::cast_possible_wrap,
            clippy::cast_possible_truncation,
            reason = "under 88"
        )]
        let q = (i as i32 + index).clamp(0, max_qp) as usize;
        *slot = CHROMA_QP[(depth - 8) as usize][q];
    }
    t
}

/// The buffer each of six scaling lists reads: the first list with the same
/// matrix, which is the list's own when no earlier one has it.
fn dequant_buffers<const N: usize>(m: &[[u8; N]; 6]) -> [usize; 6] {
    std::array::from_fn(|i| (0..i).find(|&j| m[j] == m[i]).unwrap_or(i))
}

/// `init_dequant_tables`: [`Pps::dequant`] and the buffer each list reads,
/// 4x4 and -- with the 8x8 transform -- 8x8. `bypass`: the stream may
/// bypass the transform, so QP 0's factors are all 64, a shift of six the
/// inverse transform's own undoes.
fn init_dequant(
    m4: &[[u8; 16]; 6],
    m8: &[[u8; 64]; 6],
    depth: i32,
    transform_8x8: bool,
    bypass: bool,
) -> (Box<[u32]>, [usize; 6], Option<[usize; 6]>) {
    let max_qp = (51 + 6 * (depth - 8)) as usize;
    let mut t = vec![0u32; DEQUANT8_AT + 6 * QPS * 64].into_boxed_slice();
    // `init_dequant4_coeff_table`, in the transposed order the inverse
    // transform reads.
    let list4 = dequant_buffers(m4);
    for (i, list) in m4.iter().enumerate().filter(|&(i, _)| list4[i] == i) {
        for q in 0..=max_qp {
            let shift = u32::from(QUANT_DIV6[q]) + 2;
            let idx = usize::from(QUANT_REM6[q]);
            let row = (i * QPS + q) * 16;
            for (x, &factor) in list.iter().enumerate() {
                let init = u32::from(DEQUANT4_COEFF_INIT[idx][(x & 1) + ((x >> 2) & 1)]);
                t[row + ((x >> 2) | ((x << 2) & 0xF))] = (init * u32::from(factor)) << shift;
            }
        }
    }
    // `init_dequant8_coeff_table`.
    let list8 = transform_8x8.then(|| dequant_buffers(m8));
    if let Some(list8) = &list8 {
        for (i, list) in m8.iter().enumerate().filter(|&(i, _)| list8[i] == i) {
            for q in 0..=max_qp {
                let shift = u32::from(QUANT_DIV6[q]);
                let idx = usize::from(QUANT_REM6[q]);
                let row = DEQUANT8_AT + (i * QPS + q) * 64;
                for (x, &factor) in list.iter().enumerate() {
                    let kind = usize::from(DEQUANT8_COEFF_INIT_SCAN[((x >> 1) & 12) | (x & 3)]);
                    let init = u32::from(DEQUANT8_COEFF_INIT[idx][kind]);
                    t[row + ((x >> 3) | ((x & 7) << 3))] = (init * u32::from(factor)) << shift;
                }
            }
        }
    }
    if bypass {
        // Through each list's buffer, as FFmpeg writes through its pointer.
        for i in 0..6 {
            let row = list4[i] * QPS * 16;
            t[row..row + 16].fill(1 << 6);
            if let Some(list8) = &list8 {
                let row = DEQUANT8_AT + list8[i] * QPS * 64;
                t[row..row + 64].fill(1 << 6);
            }
        }
    }
    (t, list4, list8)
}

/// Read a picture parameter set and keep it by its id
/// (`ff_h264_decode_picture_parameter_set`); `bit_length` is the unit's
/// syntax in bits, which says whether the High profiles' extension
/// follows.
#[allow(
    clippy::too_many_lines,
    reason = "one function in FFmpeg, kept whole so it reads line for line against it"
)]
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int and unsigned, converted where it converts"
)]
pub(crate) fn decode_pps(
    gb: &mut Bits<'_>,
    ps: &mut ParamSets,
    bit_length: usize,
) -> Result<(), PsError> {
    let pps_id = gb.ue() as u32;
    if pps_id as usize >= MAX_PPS_COUNT {
        return Err(PsError::Invalid);
    }

    let sps_id = gb.ue_31();
    let Some(sps) = ps.sps_list.get(sps_id as usize).and_then(Clone::clone) else {
        return Err(PsError::Invalid);
    };
    if sps.bit_depth_luma > 14 {
        return Err(PsError::Invalid);
    } else if sps.bit_depth_luma == 11 || sps.bit_depth_luma == 13 {
        ps.unsupported_pps[pps_id as usize] = true;
        return Err(PsError::Unsupported);
    }

    let cabac = gb.bit();
    let pic_order_present = gb.bit();
    let slice_group_count = gb.ue().wrapping_add(1);
    if slice_group_count > 1 {
        // Slice groups (FMO), which FFmpeg does not decode.
        ps.unsupported_pps[pps_id as usize] = true;
        return Err(PsError::Unsupported);
    }
    let ref_count = [
        (gb.ue() as u32).wrapping_add(1),
        (gb.ue() as u32).wrapping_add(1),
    ];
    if ref_count[0].wrapping_sub(1) > 31 || ref_count[1].wrapping_sub(1) > 31 {
        return Err(PsError::Invalid);
    }

    let qp_bd_offset = 6 * (sps.bit_depth_luma - 8);
    let weighted_pred = gb.bit();
    let weighted_bipred_idc = gb.bits(2);
    let init_qp = plus_26(gb.se(), qp_bd_offset);
    let init_qs = plus_26(gb.se(), qp_bd_offset);
    let mut chroma_qp_index_offset = [gb.se(), 0];
    if !(-12..=12).contains(&chroma_qp_index_offset[0]) {
        return Err(PsError::Invalid);
    }

    let deblocking_filter_parameters_present = gb.bit();
    let constrained_intra_pred = gb.bit();
    let redundant_pic_cnt_present = gb.bit();

    let mut transform_8x8_mode = 0;
    let mut m4 = sps.scaling_matrix4;
    let mut m8 = *sps.scaling_matrix8;

    let bits_left = bit_length as i64 - i64::from(gb.count());
    if bits_left > 0 && more_rbsp_data_in_pps(&sps) {
        transform_8x8_mode = gb.bit();
        let pic_scaling_matrix_present_flag = gb.bit();
        decode_scaling_matrices(
            gb,
            &sps,
            transform_8x8_mode != 0,
            false,
            pic_scaling_matrix_present_flag != 0,
            &mut m4,
            &mut m8,
        )?;
        chroma_qp_index_offset[1] = gb.se();
        if !(-12..=12).contains(&chroma_qp_index_offset[1]) {
            return Err(PsError::Invalid);
        }
    } else {
        chroma_qp_index_offset[1] = chroma_qp_index_offset[0];
    }

    let chroma_qp_table = [
        build_qp_table(chroma_qp_index_offset[0], sps.bit_depth_luma),
        build_qp_table(chroma_qp_index_offset[1], sps.bit_depth_luma),
    ];

    let (dequant, dequant4_list, dequant8_list) = init_dequant(
        &m4,
        &m8,
        sps.bit_depth_luma,
        transform_8x8_mode != 0,
        sps.transform_bypass != 0,
    );

    let pps = Pps {
        pps_id,
        sps_id,
        cabac,
        pic_order_present,
        slice_group_count,
        ref_count,
        weighted_pred,
        weighted_bipred_idc,
        init_qp,
        init_qs,
        chroma_qp_index_offset,
        deblocking_filter_parameters_present,
        constrained_intra_pred,
        redundant_pic_cnt_present,
        transform_8x8_mode,
        scaling_matrix4: m4,
        scaling_matrix8: Box::new(m8),
        chroma_qp_table,
        chroma_qp_diff: i32::from(chroma_qp_index_offset[0] != chroma_qp_index_offset[1]),
        dequant,
        dequant4_list,
        dequant8_list,
        sps,
    };
    ps.pps_list[pps_id as usize] = Some(Arc::new(pps));
    ps.unsupported_pps[pps_id as usize] = false;
    Ok(())
}

#[cfg(test)]
mod tests;
