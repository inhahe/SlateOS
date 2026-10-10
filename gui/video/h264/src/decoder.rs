//! The decoder's state and what drives it: FFmpeg's `H264Context`
//! (`h264dec.h`), `h264dec.c`, and `h264_slice.c`'s handling of pictures --
//! the stream's properties from its parameter sets, starting a frame or a
//! field, each slice's set-up, and choosing which picture to show next.
//!
//! FFmpeg keeps on its codec context (`AVCodecContext`) some of what it
//! decides about a stream -- the pixel format, the coded and cropped
//! sizes, the aspect ratio, the colour -- and reads them back as state:
//! [`CodecState`] is that part of it.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on sizes the SPS bounds and counts under 64"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "indices are under the fixed sizes FFmpeg's checks keep them under: \
              the DPB's slots, 16 frames of delay, 32 slice rows"
)]

use std::sync::Arc;

use crate::picture::{
    AnyFrame, LAST_PIC_FOR_EC, MAX_PICTURE_COUNT, Picture, PixelFormat, Ref, SLOTS,
};
use crate::poc::PocContext;
use crate::ps::{ParamSets, Sps};
use crate::sei::Sei;
use crate::slice::{MAX_MMCO_COUNT, Mmco, PICT_FRAME, SliceHeader};

/// The most frames a DPB holds (`H264_MAX_DPB_FRAMES`).
pub(crate) const MAX_DPB_FRAMES: usize = 16;
/// The slices FFmpeg keeps loop-filter state for at once (`MAX_SLICES`).
pub(crate) const MAX_SLICES: usize = 32;

/// `AVCOL_SPC_RGB`: a matrix that is the identity.
const AVCOL_SPC_RGB: i32 = 0;
/// `AVCOL_SPC_UNSPECIFIED`, `AVCOL_PRI_UNSPECIFIED`, `AVCOL_TRC_UNSPECIFIED`.
const AVCOL_UNSPECIFIED: i32 = 2;
/// `AVCOL_RANGE_MPEG`: studio range.
const AVCOL_RANGE_MPEG: i32 = 1;
/// `AVCOL_RANGE_JPEG`: full range.
const AVCOL_RANGE_JPEG: i32 = 2;

/// What FFmpeg keeps of the stream on its codec context and reads back
/// (`AVCodecContext`), with its defaults (`avcodec_alloc_context3`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CodecState {
    /// How many pictures are held back to be put in display order.
    pub(crate) has_b_frames: i32,
    pub(crate) pix_fmt: PixelFormat,
    pub(crate) coded_width: i32,
    pub(crate) coded_height: i32,
    /// The picture's size after cropping.
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) sample_aspect_ratio: (i32, i32),
    /// H.273's codes, as the VUI gives them; `AVCOL_RANGE_*` for the range.
    pub(crate) color_range: i32,
    pub(crate) color_primaries: i32,
    pub(crate) color_trc: i32,
    pub(crate) colorspace: i32,
    /// `AVChromaLocation`: the VUI's chroma sample type plus 1.
    pub(crate) chroma_sample_location: i32,
    /// Frames a second, reduced (`framerate`), from the VUI's timing.
    pub(crate) framerate: (i32, i32),
}

impl Default for CodecState {
    fn default() -> Self {
        Self {
            has_b_frames: 0,
            pix_fmt: PixelFormat::None,
            coded_width: 0,
            coded_height: 0,
            width: 0,
            height: 0,
            sample_aspect_ratio: (0, 1),
            color_range: 0,
            color_primaries: AVCOL_UNSPECIFIED,
            color_trc: AVCOL_UNSPECIFIED,
            colorspace: AVCOL_UNSPECIFIED,
            chroma_sample_location: 0,
            framerate: (0, 1),
        }
    }
}

/// Why a picture could not be set up or a slice was refused: FFmpeg's
/// error returns, which the caller only tells apart from success.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DecodeError;

/// A slice being decoded (`H264SliceContext`): its header, and what the
/// header leads to.
#[derive(Clone, Debug)]
pub(crate) struct SliceContext {
    pub(crate) hdr: SliceHeader,
    /// Each list's references: 16 frame entries a list, then 32 MBAFF
    /// field entries, two for each frame one.
    pub(crate) ref_list: [Box<[Ref; 48]>; 2],
    /// Its number in the picture, from 1 (`slice_num`).
    pub(crate) slice_num: i32,
    pub(crate) mb_x: i32,
    pub(crate) mb_y: i32,
    pub(crate) resync_mb_x: i32,
    pub(crate) resync_mb_y: i32,
    /// The QP below which the loop filter leaves an edge alone.
    pub(crate) qp_thresh: i32,
    /// Temporal direct mode's scale for each list 0 entry, and for each
    /// MBAFF field entry by field.
    pub(crate) dist_scale_factor: [i32; 32],
    pub(crate) dist_scale_factor_field: [[i32; 32]; 2],
    /// Which list 0 entry each of the co-located picture's references is,
    /// for each of its lists, and the same by field for MBAFF.
    pub(crate) map_col_to_list0: [[i32; 48]; 2],
    pub(crate) map_col_to_list0_field: [[[i32; 48]; 2]; 2],
    /// Which field of the co-located frame a frame's direct prediction
    /// takes, and how far a field's co-located field is from it.
    pub(crate) col_parity: i32,
    pub(crate) col_fieldoff: i32,

    // The macroblock being decoded, and its neighbours.
    pub(crate) prev_mb_skipped: i32,
    pub(crate) next_mb_skipped: i32,
    pub(crate) chroma_pred_mode: i32,
    pub(crate) intra16x16_pred_mode: i32,
    /// Each 4x4 block's intra prediction mode, `scan8`-laid with the
    /// neighbours' above and to the left.
    pub(crate) intra4x4_pred_mode_cache: [i8; 5 * 8],
    /// The neighbouring macroblocks' addresses -- which may be above the
    /// picture, in the tables' margin -- and types, 0 where not available.
    pub(crate) topleft_mb_xy: i32,
    pub(crate) top_mb_xy: i32,
    pub(crate) topright_mb_xy: i32,
    pub(crate) left_mb_xy: [i32; 2],
    pub(crate) topleft_type: u32,
    pub(crate) top_type: u32,
    pub(crate) topright_type: u32,
    pub(crate) left_type: [u32; 2],
    /// Which of `LEFT_BLOCK_OPTIONS` maps the left neighbour's blocks.
    pub(crate) left_block: usize,
    pub(crate) topleft_partition: i32,
    /// Which neighbouring samples intra prediction may read, a bit a 4x4
    /// block.
    pub(crate) topleft_samples_available: u32,
    pub(crate) top_samples_available: u32,
    pub(crate) topright_samples_available: u32,
    pub(crate) left_samples_available: u32,
    /// Row strides of luma and chroma, and the same for the macroblock:
    /// doubled for a field macroblock of an MBAFF frame. In samples.
    pub(crate) linesize: usize,
    pub(crate) uvlinesize: usize,
    pub(crate) mb_linesize: usize,
    pub(crate) mb_uvlinesize: usize,
    pub(crate) mb_xy: i32,
    /// The first macroblock of the next slice.
    pub(crate) next_slice_idx: i32,
    pub(crate) mb_skip_run: i32,
    pub(crate) is_complex: i32,
    /// How many of the top and left neighbours used the 8x8 transform.
    pub(crate) neighbor_transform_size: i32,
    pub(crate) cbp: i32,
    pub(crate) top_cbp: i32,
    pub(crate) left_cbp: i32,
    /// Each 4x4 block's count of coefficients, `scan8`-laid: 64 where a
    /// neighbour is not available.
    pub(crate) non_zero_count_cache: [u8; 15 * 8],
    /// Each list's motion vectors and reference indices, and CABAC's
    /// vector differences, `scan8`-laid.
    pub(crate) mv_cache: [[[i16; 2]; 5 * 8]; 2],
    pub(crate) ref_cache: [[i8; 5 * 8]; 2],
    pub(crate) mvd_cache: [[[u8; 2]; 5 * 8]; 2],
    pub(crate) direct_cache: [u8; 5 * 8],
    pub(crate) sub_mb_type: [u16; 4],
    /// The macroblock's coefficients: 16 for each of 48 blocks, as `int16`
    /// at 8 bits and `int32` deeper -- kept here as `i32` either way, an
    /// 8-bit stream's cut to 16 bits as they are stored -- with room past
    /// them for what a broken stream's scan positions may write.
    pub(crate) mb: Box<[i32; 16 * 48 + 256]>,
    pub(crate) mb_luma_dc: [[i32; 16]; 3],
    /// Where an I_PCM macroblock's samples start, in bits.
    pub(crate) intra_pcm_pos: usize,
}

impl Default for SliceContext {
    fn default() -> Self {
        Self {
            hdr: SliceHeader::default(),
            ref_list: [
                Box::new([Ref::default(); 48]),
                Box::new([Ref::default(); 48]),
            ],
            slice_num: 0,
            mb_x: 0,
            mb_y: 0,
            resync_mb_x: 0,
            resync_mb_y: 0,
            qp_thresh: 0,
            dist_scale_factor: [0; 32],
            dist_scale_factor_field: [[0; 32]; 2],
            map_col_to_list0: [[0; 48]; 2],
            map_col_to_list0_field: [[[0; 48]; 2]; 2],
            col_parity: 0,
            col_fieldoff: 0,
            prev_mb_skipped: 0,
            next_mb_skipped: 0,
            chroma_pred_mode: 0,
            intra16x16_pred_mode: 0,
            intra4x4_pred_mode_cache: [0; 40],
            topleft_mb_xy: 0,
            top_mb_xy: 0,
            topright_mb_xy: 0,
            left_mb_xy: [0; 2],
            topleft_type: 0,
            top_type: 0,
            topright_type: 0,
            left_type: [0; 2],
            left_block: 0,
            topleft_partition: 0,
            topleft_samples_available: 0,
            top_samples_available: 0,
            topright_samples_available: 0,
            left_samples_available: 0,
            linesize: 0,
            uvlinesize: 0,
            mb_linesize: 0,
            mb_uvlinesize: 0,
            mb_xy: 0,
            next_slice_idx: 0,
            mb_skip_run: 0,
            is_complex: 0,
            neighbor_transform_size: 0,
            cbp: 0,
            top_cbp: 0,
            left_cbp: 0,
            non_zero_count_cache: [0; 120],
            mv_cache: [[[0; 2]; 40]; 2],
            ref_cache: [[0; 40]; 2],
            mvd_cache: [[[0; 2]; 40]; 2],
            direct_cache: [0; 40],
            sub_mb_type: [0; 4],
            mb: Box::new([0; 16 * 48 + 256]),
            mb_luma_dc: [[0; 16]; 3],
            intra_pcm_pos: 0,
        }
    }
}

/// The decoder's state (`H264Context`).
#[derive(Debug)]
pub(crate) struct Context {
    /// The DPB's pictures, then the copy kept for concealment.
    pub(crate) pics: Vec<Picture>,
    /// The picture being decoded (`cur_pic_ptr`).
    pub(crate) cur_pic_ptr: Option<usize>,

    pub(crate) ps: ParamSets,
    pub(crate) poc: PocContext,
    pub(crate) sei: Sei,
    pub(crate) avctx: CodecState,

    /// Errors refuse what they would otherwise be passed over in
    /// (`AV_EF_EXPLODE`). Off, as FFmpeg's is by default.
    pub(crate) explode: bool,
    /// Pictures that are not yet recovered are shown anyway
    /// (`AV_CODEC_FLAG_OUTPUT_CORRUPT`). Off by default.
    pub(crate) output_corrupt: bool,

    /// The coded size: whole macroblocks.
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) chroma_x_shift: u32,
    pub(crate) chroma_y_shift: u32,
    /// The picture is no reference (`droppable`).
    pub(crate) droppable: bool,
    pub(crate) context_initialized: bool,
    /// The x264 version that made the stream, or -1 (`x264_build`).
    pub(crate) x264_build: i32,
    pub(crate) postpone_filter: bool,
    pub(crate) picture_idr: bool,
    pub(crate) picture_intra_only: bool,
    pub(crate) crop_left: i32,
    pub(crate) crop_right: i32,
    pub(crate) crop_top: i32,
    pub(crate) crop_bottom: i32,

    /// Each macroblock's slice number, `mb_stride` wide, starting two rows
    /// and a macroblock into the allocation; `0xFFFF` where none has been
    /// decoded (`slice_table`).
    pub(crate) slice_table: Vec<u16>,

    /// A frame of MBAFF macroblock pairs (`mb_aff_frame`).
    pub(crate) mb_aff_frame: bool,
    pub(crate) picture_structure: i32,
    /// The picture is a first field, its second still to come.
    pub(crate) first_field: i32,

    pub(crate) mb_y: i32,
    pub(crate) mb_height: i32,
    pub(crate) mb_width: i32,
    pub(crate) mb_stride: i32,
    pub(crate) mb_num: i32,

    pub(crate) nal_ref_idc: u32,
    pub(crate) nal_unit_type: u32,
    pub(crate) has_slice: bool,
    /// Units come after their lengths (`is_avc`), each length this many
    /// bytes (`nal_length_size`), as an `avcC` configuration says.
    pub(crate) is_avc: bool,
    pub(crate) nal_length_size: usize,

    /// The bit depth and chroma format the tables were set up for.
    pub(crate) bit_depth_luma: i32,
    pub(crate) chroma_format_idc: i32,

    pub(crate) default_ref: [Ref; 2],
    pub(crate) short_ref: [Option<usize>; 32],
    pub(crate) long_ref: [Option<usize>; 32],
    pub(crate) delayed_pic: [Option<usize>; MAX_DPB_FRAMES + 2],
    pub(crate) last_pocs: [i32; MAX_DPB_FRAMES],
    pub(crate) next_output_pic: Option<usize>,
    pub(crate) next_outputed_poc: i32,
    /// `PicOrderCnt_offset` (SMPTE RDD 2006): an IDR's id, or 0.
    pub(crate) poc_offset: i32,

    pub(crate) mmco: [Mmco; MAX_MMCO_COUNT],
    pub(crate) nb_mmco: usize,
    pub(crate) mmco_reset: i32,
    pub(crate) explicit_ref_marking: i32,
    pub(crate) long_ref_count: usize,
    pub(crate) short_ref_count: usize,

    pub(crate) current_slice: i32,
    pub(crate) prev_interlaced_frame: bool,
    pub(crate) valid_recovery_point: bool,
    /// The frame number at which the next frame is whole again, or -1.
    pub(crate) recovery_frame: i32,
    /// `FRAME_RECOVERED_*` bits: why what is decoded can be shown.
    pub(crate) frame_recovered: i32,
    pub(crate) has_recovery_point: bool,
    pub(crate) missing_fields: i32,
    pub(crate) setup_finished: bool,
    pub(crate) cur_chroma_format_idc: i32,
    pub(crate) cur_bit_depth_luma: i32,
    pub(crate) slice_row: [i32; MAX_SLICES],
    pub(crate) width_from_caller: i32,
    pub(crate) height_from_caller: i32,
    /// Lost slices concealed (`enable_er`, on by default without slice
    /// threads).
    pub(crate) enable_er: bool,
    /// Each slice's reference numbers for the loop filter: `[slice][list][2 + ref]`.
    pub(crate) ref2frm: Box<[[[i32; 64]; 2]; MAX_SLICES]>,
    /// An intra picture has come since a gray stand-in (`non_gray`).
    pub(crate) non_gray: i32,
    /// Gray stand-ins are not predicted from where there is better
    /// (`noref_gray`, on by default).
    pub(crate) noref_gray: i32,
    /// Gray stand-ins are not shown (`skip_gray`, off by default).
    pub(crate) skip_gray: i32,
    pub(crate) nb_slice_ctx_queued: usize,
    /// The time and duration of the packet being decoded, which a frame
    /// allocated for it carries (`last_pkt_props`).
    pub(crate) pkt_time: i64,
    pub(crate) pkt_duration: u64,
    /// FFmpeg's `-debug pict` lines, one a slice, for the tests to hold to
    /// FFmpeg's.
    #[cfg(test)]
    pub(crate) debug: Vec<String>,
    /// FFmpeg's `-debug mmco` lines, without their pointers.
    #[cfg(test)]
    pub(crate) debug_mmco: Vec<String>,
}

impl Default for Context {
    /// As FFmpeg's decoder starts (`h264_init_context`, then
    /// `ff_h264_flush_change` from `h264_decode_init`).
    fn default() -> Self {
        let mut sei = Sei::default();
        sei.uninit();
        let mut h = Self {
            pics: vec![Picture::default(); SLOTS],
            cur_pic_ptr: None,
            ps: ParamSets::default(),
            poc: PocContext::default(),
            sei,
            avctx: CodecState::default(),
            explode: false,
            output_corrupt: false,
            width: 0,
            height: 0,
            chroma_x_shift: 0,
            chroma_y_shift: 0,
            droppable: false,
            context_initialized: false,
            x264_build: -1,
            postpone_filter: false,
            picture_idr: false,
            picture_intra_only: false,
            crop_left: 0,
            crop_right: 0,
            crop_top: 0,
            crop_bottom: 0,
            slice_table: Vec::new(),
            mb_aff_frame: false,
            picture_structure: 0,
            first_field: 0,
            mb_y: 0,
            mb_height: 0,
            mb_width: 0,
            mb_stride: 0,
            mb_num: 0,
            nal_ref_idc: 0,
            nal_unit_type: 0,
            has_slice: false,
            is_avc: false,
            nal_length_size: 0,
            bit_depth_luma: 0,
            chroma_format_idc: 0,
            default_ref: [Ref::default(); 2],
            short_ref: [None; 32],
            long_ref: [None; 32],
            delayed_pic: [None; MAX_DPB_FRAMES + 2],
            last_pocs: [i32::MIN; MAX_DPB_FRAMES],
            next_output_pic: None,
            next_outputed_poc: i32::MIN,
            poc_offset: 0,
            mmco: [Mmco::default(); MAX_MMCO_COUNT],
            nb_mmco: 0,
            mmco_reset: 0,
            explicit_ref_marking: 0,
            long_ref_count: 0,
            short_ref_count: 0,
            current_slice: 0,
            prev_interlaced_frame: false,
            valid_recovery_point: false,
            recovery_frame: -1,
            frame_recovered: 0,
            has_recovery_point: false,
            missing_fields: 0,
            setup_finished: false,
            cur_chroma_format_idc: -1,
            cur_bit_depth_luma: 0,
            slice_row: [0; MAX_SLICES],
            width_from_caller: 0,
            height_from_caller: 0,
            enable_er: true,
            ref2frm: Box::new([[[0; 64]; 2]; MAX_SLICES]),
            non_gray: 0,
            noref_gray: 1,
            skip_gray: 0,
            nb_slice_ctx_queued: 0,
            pkt_time: 0,
            pkt_duration: 0,
            #[cfg(test)]
            debug: Vec::new(),
            #[cfg(test)]
            debug_mmco: Vec::new(),
        };
        h.flush_change();
        h
    }
}

/// The greatest common divisor (`av_gcd`), of magnitudes.
fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// `num / den` as a fraction of terms at most `max`, the nearest there is
/// (`av_reduce`): `(num, den)` of the result.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int64 arithmetic; the terms end at most `max`, under 2^31"
)]
pub(crate) fn reduce(num: i64, den: i64, max: i64) -> (i32, i32) {
    let (mut a0n, mut a0d) = (0i64, 1i64);
    let (mut a1n, mut a1d) = (1i64, 0i64);
    let sign = (num < 0) ^ (den < 0);
    let g = gcd(num.unsigned_abs(), den.unsigned_abs()) as i64;
    let (mut num, mut den) = (num, den);
    if g != 0 {
        num = num.wrapping_abs() / g;
        den = den.wrapping_abs() / g;
    }
    if num <= max && den <= max {
        (a1n, a1d) = (num, den);
        den = 0;
    }
    while den != 0 {
        let x = (num as u64) / (den as u64);
        let next_den = num.wrapping_sub(den.wrapping_mul(x as i64));
        let a2n = (x as i64).wrapping_mul(a1n).wrapping_add(a0n);
        let a2d = (x as i64).wrapping_mul(a1d).wrapping_add(a0d);
        if a2n > max || a2d > max {
            let mut x = x as i64;
            if a1n != 0 {
                x = (max - a0n) / a1n;
            }
            if a1d != 0 {
                x = x.min((max - a0d) / a1d);
            }
            if den.wrapping_mul(2 * x * a1d + a0d) > num.wrapping_mul(a1d) {
                (a1n, a1d) = (x * a1n + a0n, x * a1d + a0d);
            }
            break;
        }
        (a0n, a0d) = (a1n, a1d);
        (a1n, a1d) = (a2n, a2d);
        num = den;
        den = next_den;
    }
    let n = a1n as i32;
    (if sign { -n } else { n }, a1d as i32)
}

/// FFmpeg's comparison of two fractions (`av_cmp_q`): 0 when equal, and
/// `INT_MIN` for two with a zero denominator and a zero numerator.
pub(crate) fn cmp_q(a: (i32, i32), b: (i32, i32)) -> i32 {
    let tmp = i64::from(a.0) * i64::from(b.1) - i64::from(b.0) * i64::from(a.1);
    if tmp != 0 {
        #[allow(clippy::cast_possible_truncation, reason = "0 or -1, then | 1")]
        let s = ((tmp ^ i64::from(a.1) ^ i64::from(b.1)) >> 63) as i32;
        s | 1
    } else if b.1 != 0 && a.1 != 0 {
        0
    } else if a.0 != 0 && b.0 != 0 {
        (a.0 >> 31) - (b.0 >> 31)
    } else {
        i32::MIN
    }
}

/// Whether `sar` is an aspect ratio a `width` by `height` picture can be
/// shown at (`av_image_check_sar`).
fn sar_valid(width: i32, height: i32, sar: (i32, i32)) -> bool {
    if sar.1 <= 0 || sar.0 < 0 {
        return false;
    }
    if sar.0 == 0 || sar.0 == sar.1 {
        return true;
    }
    #[allow(clippy::cast_sign_loss, reason = "FFmpeg takes the sizes as unsigned")]
    let (w, h) = (i64::from(width as u32), i64::from(height as u32));
    let scaled = if w > h {
        w * i64::from(sar.0) / i64::from(sar.1)
    } else {
        h * i64::from(sar.1) / i64::from(sar.0)
    };
    scaled > 0
}

impl Context {
    pub(crate) fn field_picture(&self) -> bool {
        self.picture_structure != PICT_FRAME
    }

    /// The current SPS. Set from the first slice's PPS before anything
    /// reads it.
    fn sps(&self) -> Result<Arc<Sps>, DecodeError> {
        self.ps.sps.clone().ok_or(DecodeError)
    }

    /// An instantaneous decoder refresh: no references, the picture order
    /// counts starting again (`idr`).
    pub(crate) fn idr(&mut self) {
        self.remove_all_refs();
        self.poc.prev_frame_num = 0;
        self.poc.prev_frame_num_offset = 0;
        self.poc.prev_poc_msb = 1 << 16;
        self.poc.prev_poc_lsb = -1;
        self.last_pocs = [i32::MIN; MAX_DPB_FRAMES];
    }

    /// The pictures before a discontinuity forgotten (`ff_h264_flush_change`).
    pub(crate) fn flush_change(&mut self) {
        self.next_outputed_poc = i32::MIN;
        self.prev_interlaced_frame = true;
        self.idr();
        self.poc.prev_frame_num = -1;
        if let Some(cur) = self.cur_pic_ptr {
            self.pics[cur].reference = 0;
            let kept: Vec<usize> = self
                .delayed_pic
                .iter()
                .map_while(|d| *d)
                .filter(|&d| d != cur)
                .collect();
            self.delayed_pic = [None; MAX_DPB_FRAMES + 2];
            for (slot, d) in self.delayed_pic.iter_mut().zip(kept) {
                *slot = Some(d);
            }
        }
        self.pics[LAST_PIC_FOR_EC].unref();
        self.first_field = 0;
        self.recovery_frame = -1;
        self.frame_recovered = 0;
        self.current_slice = 0;
        self.mmco_reset = 1;
    }

    /// Every picture let go, as after a seek (`h264_decode_flush`).
    pub(crate) fn flush(&mut self) {
        self.delayed_pic = [None; MAX_DPB_FRAMES + 2];
        self.flush_change();
        self.sei.uninit();
        for pic in &mut self.pics[..MAX_PICTURE_COUNT] {
            pic.unref();
        }
        self.cur_pic_ptr = None;
        self.mb_y = 0;
        self.non_gray = 0;
        self.slice_table = Vec::new();
        self.context_initialized = false;
    }

    /// The pixel format for the current SPS (`get_pixel_format`): by depth
    /// and chroma format, GBR where the matrix is the identity, the
    /// full-range aliases at 8 bits.
    fn get_pixel_format(&self, sps: &Sps) -> Result<PixelFormat, DecodeError> {
        let rgb = self.avctx.colorspace == AVCOL_SPC_RGB;
        let jpeg = self.avctx.color_range == AVCOL_RANGE_JPEG;
        let chroma444 = sps.chroma_format_idc == 3;
        let chroma422 = sps.chroma_format_idc == 2;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "8 to 14"
        )]
        Ok(match sps.bit_depth_luma {
            d @ (9 | 10 | 12 | 14) => {
                let d = d as u8;
                if chroma444 {
                    if rgb {
                        PixelFormat::Gbr(d)
                    } else {
                        PixelFormat::Yuv444(d)
                    }
                } else if chroma422 {
                    PixelFormat::Yuv422(d)
                } else {
                    PixelFormat::Yuv420(d)
                }
            }
            8 => {
                if chroma444 {
                    if rgb {
                        PixelFormat::Gbr(8)
                    } else if jpeg {
                        PixelFormat::Yuvj444
                    } else {
                        PixelFormat::Yuv444(8)
                    }
                } else if chroma422 {
                    if jpeg {
                        PixelFormat::Yuvj422
                    } else {
                        PixelFormat::Yuv422(8)
                    }
                } else if jpeg {
                    PixelFormat::Yuvj420
                } else {
                    PixelFormat::Yuv420(8)
                }
            }
            _ => return Err(DecodeError),
        })
    }

    /// The coded and cropped sizes (`init_dimensions`). The container's
    /// size, where one was given, crops the right and bottom instead when
    /// the stream crops neither the left nor the top and the two agree to
    /// within a macroblock.
    fn init_dimensions(&mut self, sps: &Sps) {
        #[allow(
            clippy::cast_possible_wrap,
            reason = "under the coded size, checked where the SPS is read"
        )]
        let (mut cr, mut cl, mut ct, mut cb) = (
            sps.crop_right as i32,
            sps.crop_left as i32,
            sps.crop_top as i32,
            sps.crop_bottom as i32,
        );
        let mut width = self.width - (cr + cl);
        let mut height = self.height - (ct + cb);
        let align16 = |v: i32| (v + 15) & !15;
        let (wc, hc) = (self.width_from_caller, self.height_from_caller);
        if wc > 0
            && hc > 0
            && sps.crop_top == 0
            && sps.crop_left == 0
            && align16(wc) == align16(width)
            && align16(hc) == align16(height)
            && wc <= width
            && hc <= height
        {
            width = wc;
            height = hc;
            cl = 0;
            ct = 0;
            cr = self.width - width;
            cb = self.height - height;
        } else {
            self.width_from_caller = 0;
            self.height_from_caller = 0;
        }
        self.avctx.coded_width = self.width;
        self.avctx.coded_height = self.height;
        self.avctx.width = width;
        self.avctx.height = height;
        self.crop_right = cr;
        self.crop_left = cl;
        self.crop_top = ct;
        self.crop_bottom = cb;
    }

    /// The decoder set up for a new SPS (`h264_slice_header_init`): the
    /// aspect ratio, the frame rate, tables for its size, its depth.
    fn slice_header_init(&mut self, sps: &Sps) -> Result<(), DecodeError> {
        // ff_set_sar: an aspect ratio the picture cannot be shown at is
        // taken as unknown.
        self.avctx.sample_aspect_ratio =
            if sar_valid(self.avctx.width, self.avctx.height, sps.vui.sar) {
                sps.vui.sar
            } else {
                (0, 1)
            };
        let (cx, cy) = match self.avctx.pix_fmt {
            PixelFormat::Yuv420(_) | PixelFormat::Yuvj420 => (1, 1),
            PixelFormat::Yuv422(_) | PixelFormat::Yuvj422 => (1, 0),
            _ => (0, 0),
        };
        self.chroma_x_shift = cx;
        self.chroma_y_shift = cy;

        if sps.timing_info_present_flag != 0 {
            let mut den = i64::from(sps.time_scale);
            #[allow(clippy::cast_sign_loss, reason = "compared unsigned, as FFmpeg does")]
            if (self.x264_build as u32) < 44 {
                den *= 2;
            }
            let (num, den) = reduce(i64::from(sps.num_units_in_tick) * 2, den, 1 << 30);
            self.avctx.framerate = (den, num);
        }

        self.first_field = 0;
        self.prev_interlaced_frame = true;
        self.alloc_tables()?;

        if sps.bit_depth_luma < 8
            || sps.bit_depth_luma > 14
            || sps.bit_depth_luma == 11
            || sps.bit_depth_luma == 13
        {
            self.slice_table = Vec::new();
            self.context_initialized = false;
            return Err(DecodeError);
        }
        self.cur_bit_depth_luma = sps.bit_depth_luma;
        self.cur_chroma_format_idc = sps.chroma_format_idc;
        self.chroma_format_idc = sps.chroma_format_idc;
        self.bit_depth_luma = sps.bit_depth_luma;
        self.context_initialized = true;
        Ok(())
    }

    /// The tables kept for the picture's size (`ff_h264_alloc_tables`):
    /// the slice table, every entry "none", two rows and a macroblock to
    /// spare above for the neighbours of the first rows.
    fn alloc_tables(&mut self) -> Result<(), DecodeError> {
        #[allow(clippy::cast_sign_loss, reason = "a macroblock count from the SPS")]
        let len = (self.mb_stride * (self.mb_height + 1) + self.mb_stride) as usize;
        let mut table = Vec::new();
        table.try_reserve_exact(len).map_err(|_| DecodeError)?;
        table.resize(len, 0xFFFF);
        self.slice_table = table;
        Ok(())
    }

    /// The current PPS and SPS taken up, and the decoder set up again when
    /// the stream's size, depth, chroma, aspect ratio or colour changes
    /// (`h264_init_ps`).
    pub(crate) fn init_ps(
        &mut self,
        sl: &SliceContext,
        first_slice: bool,
    ) -> Result<(), DecodeError> {
        let mut needs_reinit = false;
        if first_slice {
            self.ps.pps = self
                .ps
                .pps_list
                .get(sl.hdr.pps_id as usize)
                .cloned()
                .flatten();
        }
        let pps = self.ps.pps.clone().ok_or(DecodeError)?;
        if self
            .ps
            .sps
            .as_ref()
            .is_none_or(|s| !Arc::ptr_eq(s, &pps.sps))
        {
            let sps = Arc::clone(&pps.sps);
            if self.mb_width != sps.mb_width
                || self.mb_height != sps.mb_height
                || self.cur_bit_depth_luma != sps.bit_depth_luma
                || self.cur_chroma_format_idc != sps.chroma_format_idc
            {
                needs_reinit = true;
            }
            if self.bit_depth_luma != sps.bit_depth_luma
                || self.chroma_format_idc != sps.chroma_format_idc
            {
                needs_reinit = true;
            }
            self.ps.sps = Some(sps);
        }
        let sps = self.sps()?;

        let mut must_reinit = self.context_initialized
            && (16 * sps.mb_width != self.avctx.coded_width
                || 16 * sps.mb_height != self.avctx.coded_height
                || self.cur_bit_depth_luma != sps.bit_depth_luma
                || self.cur_chroma_format_idc != sps.chroma_format_idc
                || self.mb_width != sps.mb_width
                || self.mb_height != sps.mb_height);
        if self.avctx.pix_fmt == PixelFormat::None
            || self.get_pixel_format(&sps).map(PixelFormat::non_j) != Ok(self.avctx.pix_fmt.non_j())
        {
            must_reinit = true;
        }
        if first_slice && cmp_q(sps.vui.sar, self.avctx.sample_aspect_ratio) != 0 {
            must_reinit = true;
        }

        if !self.setup_finished {
            self.mb_width = sps.mb_width;
            self.mb_height = sps.mb_height;
            self.mb_num = self.mb_width * self.mb_height;
            self.mb_stride = self.mb_width + 1;
            self.chroma_y_shift = u32::from(sps.chroma_format_idc <= 1);
            self.width = 16 * self.mb_width;
            self.height = 16 * self.mb_height;
            self.init_dimensions(&sps);

            let vui = &sps.vui;
            if vui.video_signal_type_present_flag != 0 {
                self.avctx.color_range = if vui.video_full_range_flag > 0 {
                    AVCOL_RANGE_JPEG
                } else {
                    AVCOL_RANGE_MPEG
                };
                if vui.colour_description_present_flag != 0 {
                    #[allow(clippy::cast_possible_wrap, reason = "8-bit codes")]
                    let (pri, trc, mat) = (
                        vui.colour_primaries as i32,
                        vui.transfer_characteristics as i32,
                        vui.matrix_coeffs as i32,
                    );
                    if self.avctx.colorspace != mat {
                        needs_reinit = true;
                    }
                    self.avctx.color_primaries = pri;
                    self.avctx.color_trc = trc;
                    self.avctx.colorspace = mat;
                }
            }
            if let Some(t) = self.sei.alternative_transfer {
                // A characteristic FFmpeg has a name for, and not
                // "unspecified".
                #[allow(clippy::cast_possible_wrap, reason = "an 8-bit code")]
                let t = t as i32;
                if transfer_named(t) && t != AVCOL_UNSPECIFIED {
                    self.avctx.color_trc = t;
                }
            }
        }
        #[allow(clippy::cast_possible_wrap, reason = "0 to 6")]
        let loc = sps.vui.chroma_location as i32;
        self.avctx.chroma_sample_location = loc;

        if !self.context_initialized || must_reinit || needs_reinit {
            let flush_changes = self.context_initialized;
            self.context_initialized = false;
            if flush_changes {
                self.flush_change();
            }
            self.avctx.pix_fmt = self.get_pixel_format(&sps)?;
            self.slice_header_init(&sps)?;
        }
        Ok(())
    }
}

/// Whether FFmpeg has a name for transfer characteristic `t`
/// (`av_color_transfer_name` not NULL): 0 to 18, and V-Log's 256.
fn transfer_named(t: i32) -> bool {
    matches!(t, 0..=18 | 256)
}

impl Context {
    /// Pictures neither references nor waiting to be shown let go -- the
    /// current one too, unless `remove_current` is false
    /// (`release_unused_pictures`).
    pub(crate) fn release_unused_pictures(&mut self, remove_current: bool) {
        for i in 0..MAX_PICTURE_COUNT {
            if self.pics[i].in_use()
                && self.pics[i].reference == 0
                && (remove_current || Some(i) != self.cur_pic_ptr)
            {
                self.pics[i].unref();
            }
        }
    }

    /// A free slot of the DPB (`find_unused_picture`).
    fn find_unused_picture(&self) -> Option<usize> {
        self.pics[..MAX_PICTURE_COUNT]
            .iter()
            .position(|p| !p.in_use())
    }

    /// Picture `slot` given a frame and tables for the current size
    /// (`alloc_picture`).
    fn alloc_picture(&mut self, slot: usize, sps: &Sps) -> Result<(), DecodeError> {
        #[allow(clippy::cast_sign_loss, reason = "coded sizes are positive")]
        let frame = AnyFrame::new(
            self.width as usize,
            self.height as usize,
            self.chroma_x_shift,
            self.chroma_y_shift,
            sps.bit_depth_luma,
        )
        .ok_or(DecodeError)?;
        let needs_fg = self.pics[slot].needs_fg != 0;

        #[allow(clippy::cast_sign_loss, reason = "a macroblock count from the SPS")]
        let (big_mb_num, mb_array_size, b4_array_size) = {
            let stride = self.mb_stride as usize;
            let height = self.mb_height as usize;
            let b4_stride = self.mb_width as usize * 4 + 1;
            (
                stride * (height + 1) + 1,
                stride * height,
                b4_stride * height * 4,
            )
        };
        let mb_stride = self.mb_stride as usize;
        let a = &self.avctx;
        // ff_decode_frame_props: the codec context's properties, and the
        // packet's, onto the new frame; an aspect ratio the coded size
        // cannot be shown at taken as unknown.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            reason = "coded sizes, under 2^16"
        )]
        let (fw, fh) = (frame.size().0 as i32, frame.size().1 as i32);
        let sar = if sar_valid(fw, fh, a.sample_aspect_ratio) {
            a.sample_aspect_ratio
        } else {
            (0, 1)
        };
        let pic = &mut self.pics[slot];
        if needs_fg {
            pic.f_grain = Some(Arc::new(frame.clone()));
        }
        pic.f = Some(Arc::new(frame));
        pic.props.format = a.pix_fmt;
        pic.props.sample_aspect_ratio = sar;
        pic.props.color_range = a.color_range;
        pic.props.color_primaries = a.color_primaries;
        pic.props.color_trc = a.color_trc;
        pic.props.colorspace = a.colorspace;
        pic.props.chroma_location = a.chroma_sample_location;
        pic.props.time = self.pkt_time;
        pic.props.duration = self.pkt_duration;
        pic.qscale_table = Some(Arc::new(vec![0; big_mb_num + mb_stride]));
        pic.mb_type = Some(Arc::new(vec![0; big_mb_num + mb_stride]));
        for list in 0..2 {
            pic.motion_val[list] = Some(Arc::new(vec![[0; 2]; b4_array_size + 4]));
            pic.ref_index[list] = Some(Arc::new(vec![0; 4 * mb_array_size]));
        }
        pic.pps = self.ps.pps.clone();
        pic.mb_width = self.mb_width;
        pic.mb_height = self.mb_height;
        pic.mb_stride = self.mb_stride;
        Ok(())
    }

    /// A new picture to decode into (`h264_frame_start`).
    pub(crate) fn frame_start(&mut self, sl: &SliceContext) -> Result<(), DecodeError> {
        let sps = self.sps()?;
        self.release_unused_pictures(true);
        self.cur_pic_ptr = None;
        let slot = self.find_unused_picture().ok_or(DecodeError)?;
        {
            let pic = &mut self.pics[slot];
            pic.reference = if self.droppable {
                0
            } else {
                self.picture_structure
            };
            pic.field_picture = i32::from(self.picture_structure != PICT_FRAME);
            pic.frame_num = self.poc.frame_num;
            pic.props.key = false;
            pic.mmco_reset = 0;
            pic.recovered = 0;
            pic.invalid_gap = 0;
            pic.sei_recovery_frame_cnt = self.sei.recovery_frame_cnt;
            pic.props.pict_type = sl.hdr.slice_type;
            #[allow(clippy::cast_sign_loss, reason = "crops are under the coded size")]
            let crop = [
                self.crop_left,
                self.crop_right,
                self.crop_top,
                self.crop_bottom,
            ]
            .map(|c| c as u32);
            pic.props.crop = crop;
            pic.needs_fg = i32::from(self.sei.film_grain.as_ref().is_some_and(|f| f.present));
        }
        if self.alloc_picture(slot, &sps).is_err() {
            self.pics[slot].unref();
            return Err(DecodeError);
        }
        self.cur_pic_ptr = Some(slot);

        let pic = &mut self.pics[slot];
        // Marked no reference once allocated, so that an error before it
        // is decoded lets it go.
        pic.reference = 0;
        pic.field_poc = [i32::MAX, i32::MAX];
        self.next_output_pic = None;
        self.postpone_filter = false;
        self.mb_aff_frame = sps.mb_aff != 0 && self.picture_structure == PICT_FRAME;
        if self.sei.x264_build >= 0 {
            self.x264_build = self.sei.x264_build;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
