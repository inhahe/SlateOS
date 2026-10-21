//! Starting and ending a frame or field, and choosing what to show
//! (`h264_field_start`, `h264_export_frame_props` and
//! `h264_select_output_frame` in `h264_slice.c`, `ff_h264_field_end` in
//! `h264_picture.c`, and the SEI's side data from `h2645_sei.c`).
//!
//! The first slice of a picture decides whether it starts a new frame or
//! is the second field of one begun -- allocating stand-ins for frame
//! numbers the stream skipped on the way -- works out its order counts, and
//! marks how it can be shown. Pictures are then held back in order count
//! until as many have come as the stream may reorder.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on frame numbers under 2^16 and counts under 18"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "slots and delay entries are under their fixed sizes, as FFmpeg's asserts keep them"
)]

use std::sync::Arc;

use crate::decoder::{Context, DecodeError, MAX_DPB_FRAMES, SliceContext, cmp_q};
use crate::h274::{GrainParams, apply_film_grain};
use crate::nal::{NAL_IDR_SLICE, Nal};
use crate::picture::{AnyFrame, DELAYED_PIC_REF, FRAME_RECOVERED_IDR, FRAME_RECOVERED_SEI};
use crate::poc::init_poc;
use crate::ps::Sps;
use crate::sei::{
    PIC_STRUCT_BOTTOM_FIELD, PIC_STRUCT_BOTTOM_TOP, PIC_STRUCT_BOTTOM_TOP_BOTTOM,
    PIC_STRUCT_FRAME_DOUBLING, PIC_STRUCT_FRAME_TRIPLING, PIC_STRUCT_TOP_BOTTOM,
    PIC_STRUCT_TOP_BOTTOM_TOP, PIC_STRUCT_TOP_FIELD, process_picture_timing,
};
use crate::side_data::{MasteringDisplay, SideData, Stereo3d, Stereo3dType, Stereo3dView};
use crate::slice::{PICT_BOTTOM_FIELD, PICT_TOP_FIELD, TYPE_B, TYPE_I};

/// The display matrix of a rotation by `angle` degrees clockwise
/// (`av_display_rotation_set`).
#[allow(
    clippy::cast_possible_truncation,
    reason = "cosines and sines times 2^16, as FFmpeg's CONV_DB casts them"
)]
fn rotation_matrix(angle: f64) -> [i32; 9] {
    let radians = -angle * std::f64::consts::PI / 180.0;
    let (s, c) = radians.sin_cos();
    let conv = |x: f64| (x * 65536.0) as i32;
    [conv(c), conv(-s), 0, conv(s), conv(c), 0, 0, 0, 1 << 30]
}

/// A matrix flipped across, down or both (`av_display_matrix_flip`).
fn flip_matrix(m: &mut [i32; 9], hflip: bool, vflip: bool) {
    let flip = [1 - 2 * i32::from(hflip), 1 - 2 * i32::from(vflip), 1];
    if hflip || vflip {
        for (i, v) in m.iter_mut().enumerate() {
            *v = v.wrapping_mul(flip[i % 3]);
        }
    }
}

/// A time code in SMPTE 12-M's binary form at `rate` frames a second
/// (`av_timecode_get_smpte`).
#[allow(
    clippy::cast_sign_loss,
    reason = "fields clipped to their ranges first"
)]
fn timecode_smpte(rate: (i32, i32), drop: u32, hh: u32, mm: u32, ss: u32, ff: u32) -> u32 {
    let mut tc = 0u32;
    let mut ff = ff;
    if cmp_q(rate, (30, 1)) == 1 {
        if ff % 2 == 1 {
            tc |= if cmp_q(rate, (50, 1)) == 0 {
                1 << 7
            } else {
                1 << 23
            };
        }
        ff /= 2;
    }
    let hh = hh % 24;
    let mm = mm.min(59);
    let ss = ss.min(59);
    let ff = ff % 40;
    tc |= drop << 30;
    tc |= (ff / 10) << 28;
    tc |= (ff % 10) << 24;
    tc |= (ss / 10) << 20;
    tc |= (ss % 10) << 16;
    tc |= (mm / 10) << 12;
    tc |= (mm % 10) << 8;
    tc |= (hh / 10) << 4;
    tc | (hh % 10)
}

/// A binary time code as `hh:mm:ss:ff`, `;` before the frames when it
/// drops frames (`av_timecode_make_smpte_tc_string2`).
fn timecode_text(rate: (i32, i32), tc: u32) -> String {
    let bcd = |b: u32| (b >> 4) * 10 + (b & 15);
    let hh = bcd(tc & 0x3f);
    let mm = bcd(tc >> 8 & 0x7f);
    let ss = bcd(tc >> 16 & 0x7f);
    let mut ff = bcd(tc >> 24 & 0x3f);
    let drop = tc & 1 << 30 != 0;
    if cmp_q(rate, (30, 1)) == 1 {
        ff <<= 1;
        let bit = if cmp_q(rate, (50, 1)) == 0 {
            1 << 7
        } else {
            1 << 23
        };
        ff += u32::from(tc & bit != 0);
    }
    format!(
        "{hh:02}:{mm:02}:{ss:02}{}{ff:02}",
        if drop { ';' } else { ':' }
    )
}

impl Context {
    /// The SEI's side data onto picture `cur` (`ff_h2645_sei_to_frame`,
    /// with `h2645_sei_to_side_data`): what is about the picture -- its
    /// stereo packing, its orientation, captions, the display it was
    /// mastered on, the film grain to show it with -- each message taken as
    /// FFmpeg takes it, the captions, unregistered data and AFD used up as
    /// they are attached.
    fn sei_to_frame(&mut self, cur: usize) {
        // The grain's seed (FFmpeg's `seed` argument): the order count plus
        // the last IDR's id five bits up, an int FFmpeg widens into the
        // uint64_t it keeps it in.
        #[allow(
            clippy::cast_sign_loss,
            reason = "FFmpeg's int widened into a uint64_t"
        )]
        let grain_seed = i64::from(
            self.pics[cur]
                .poc
                .wrapping_add(self.poc_offset.wrapping_shl(5)),
        ) as u64;
        let sei = &mut self.sei;
        let props = &mut self.pics[cur].props;

        let fp = &sei.frame_packing;
        if fp.present && fp.arrangement_type <= 6 && matches!(fp.content_interpretation_type, 1 | 2)
        {
            let kind = match fp.arrangement_type {
                0 => Stereo3dType::Checkerboard,
                1 => Stereo3dType::Columns,
                2 => Stereo3dType::Lines,
                3 if fp.quincunx_sampling_flag != 0 => Stereo3dType::SideBySideQuincunx,
                3 => Stereo3dType::SideBySide,
                4 => Stereo3dType::TopBottom,
                5 => Stereo3dType::FrameSequence,
                _ => Stereo3dType::TwoD,
            };
            let view = match (fp.arrangement_type, fp.current_frame_is_frame0_flag) {
                (5, 0) => Stereo3dView::Right,
                (5, _) => Stereo3dView::Left,
                _ => Stereo3dView::Packed,
            };
            props.side_data.push(SideData::Stereo3d(Stereo3d {
                kind,
                inverted: fp.content_interpretation_type == 2,
                view,
            }));
        }

        let o = &sei.display_orientation;
        if o.present && (o.anticlockwise_rotation != 0 || o.hflip != 0 || o.vflip != 0) {
            let angle = f64::from(o.anticlockwise_rotation) * 360.0 / f64::from(1u32 << 16);
            let (h, v) = (o.hflip != 0, o.vflip != 0);
            let angle = -angle * f64::from(1 - 2 * i32::from(h)) * f64::from(1 - 2 * i32::from(v));
            let mut m = rotation_matrix(angle);
            flip_matrix(&mut m, h, v);
            props.side_data.push(SideData::DisplayMatrix(m));
        }

        // Captions are kept only when a message carried some.
        if !sei.a53_caption.is_empty() {
            props
                .side_data
                .push(SideData::A53Captions(std::mem::take(&mut sei.a53_caption)));
        }
        for u in sei.unregistered.drain(..) {
            props.side_data.push(SideData::Unregistered(u));
        }
        let env = &sei.ambient_viewing;
        if env.present {
            #[allow(
                clippy::cast_possible_wrap,
                reason = "FFmpeg's av_make_q of a uint32_t"
            )]
            let illuminance = env.ambient_illuminance as i32;
            props.side_data.push(SideData::AmbientViewing {
                illuminance: (illuminance, 10000),
                light_x: (i32::from(env.ambient_light_x), 50000),
                light_y: (i32::from(env.ambient_light_y), 50000),
            });
        }
        let md = &sei.mastering_display;
        if md.present != 0 {
            // The SEI orders the primaries green, blue, red.
            let mapping = [2, 0, 1];
            let mut has_primaries = true;
            let mut primaries = [[(0, 0); 2]; 3];
            for (i, &j) in mapping.iter().enumerate() {
                let [x, y] = md.display_primaries[j];
                primaries[i] = [(i32::from(x), 50000), (i32::from(y), 50000)];
                has_primaries &= (5..=37000).contains(&x) && (5..=42000).contains(&y);
            }
            let [wx, wy] = md.white_point;
            has_primaries &= (5..=37000).contains(&wx) && (5..=42000).contains(&wy);
            let has_luminance = (50000..=100_000_000).contains(&md.max_luminance)
                && md.min_luminance <= 50000
                && md.min_luminance < md.max_luminance;
            #[allow(clippy::cast_possible_wrap, reason = "FFmpeg's int of a uint32_t")]
            props
                .side_data
                .push(SideData::MasteringDisplay(MasteringDisplay {
                    display_primaries: primaries,
                    white_point: [(i32::from(wx), 50000), (i32::from(wy), 50000)],
                    min_luminance: (md.min_luminance as i32, 10000),
                    max_luminance: (md.max_luminance as i32, 10000),
                    has_primaries,
                    has_luminance,
                }));
        }
        let cl = &sei.content_light;
        if cl.present != 0 {
            props.side_data.push(SideData::ContentLight {
                max_cll: u32::from(cl.max_content_light_level),
                max_fall: u32::from(cl.max_pic_average_light_level),
            });
        }
        if let Some(afd) = sei.afd.take() {
            #[allow(clippy::cast_possible_truncation, reason = "a 4-bit code")]
            props.side_data.push(SideData::Afd(afd as u8));
        }
        if let Some(lcevc) = sei.lcevc.take() {
            props.side_data.push(SideData::Lcevc(lcevc));
        }
        if let Some(fg) = sei.film_grain.as_mut().filter(|f| f.present) {
            // Made into the picture when it is finished (`field_end`). FFmpeg
            // reads the first of a frame's film grain side data, so a second
            // field's message changes nothing.
            let params = &mut self.pics[cur].film_grain;
            if params.is_none() {
                *params = Some(Box::new(GrainParams {
                    seed: grain_seed,
                    model: (**fg).clone(),
                }));
            }
            // What the message leaves for the pictures after this one.
            fg.present = fg.repetition_period != 0;
        }
    }

    /// How the picture is to be shown, from the picture timing SEI or how
    /// it was coded, and the SEI's side data (`h264_export_frame_props`).
    fn export_frame_props(&mut self, sps: &Sps) -> Result<(), DecodeError> {
        let cur = self.cur_pic_ptr.ok_or(DecodeError)?;
        let mut interlaced = false;
        let mut top_field_first = false;
        self.pics[cur].props.interlaced = false;
        self.pics[cur].props.repeat_pict = 0;

        if self.sei.picture_timing.present
            && process_picture_timing(&mut self.sei.picture_timing, sps).is_err()
        {
            if self.explode {
                return Err(DecodeError);
            }
            self.sei.picture_timing.present = false;
        }

        let field_or_mbaff = self.field_picture() || self.mb_aff_frame;
        let pt = &self.sei.picture_timing;
        let timing = sps.pic_struct_present_flag != 0 && pt.present;
        if timing {
            match pt.pic_struct {
                PIC_STRUCT_TOP_FIELD | PIC_STRUCT_BOTTOM_FIELD => interlaced = true,
                PIC_STRUCT_TOP_BOTTOM | PIC_STRUCT_BOTTOM_TOP => {
                    // A picture of fields, or one that may be soft-telecined
                    // progressive: interlaced as the last one was.
                    interlaced = field_or_mbaff || self.prev_interlaced_frame;
                }
                PIC_STRUCT_TOP_BOTTOM_TOP | PIC_STRUCT_BOTTOM_TOP_BOTTOM => {
                    self.pics[cur].props.repeat_pict = 1;
                }
                PIC_STRUCT_FRAME_DOUBLING => self.pics[cur].props.repeat_pict = 2,
                PIC_STRUCT_FRAME_TRIPLING => self.pics[cur].props.repeat_pict = 4,
                _ => {}
            }
            if pt.ct_type & 3 != 0 && pt.pic_struct <= PIC_STRUCT_BOTTOM_TOP {
                interlaced = pt.ct_type & 2 != 0;
            }
        } else {
            interlaced = field_or_mbaff;
        }
        self.prev_interlaced_frame = interlaced;

        let field_poc = self.pics[cur].field_poc;
        if field_poc[0] != field_poc[1] {
            top_field_first = field_poc[0] < field_poc[1];
        } else if timing {
            // Even a past picture's timing is better than nothing.
            top_field_first = matches!(
                pt.pic_struct,
                PIC_STRUCT_TOP_BOTTOM | PIC_STRUCT_TOP_BOTTOM_TOP
            );
        } else if interlaced {
            top_field_first = true;
        }
        self.pics[cur].props.interlaced = interlaced;
        self.pics[cur].props.top_field_first = top_field_first;

        self.sei_to_frame(cur);

        let pt = &mut self.sei.picture_timing;
        if pt.timecode_cnt > 0 {
            let rate = self.avctx.framerate;
            #[allow(clippy::cast_possible_truncation, reason = "at most 3")]
            let mut codes = vec![pt.timecode_cnt as u32];
            let mut text = None;
            for t in pt.timecode.iter().take(pt.timecode_cnt) {
                let tc = timecode_smpte(rate, t.dropframe, t.hours, t.minutes, t.seconds, t.frame);
                codes.push(tc);
                text = Some(timecode_text(rate, tc));
            }
            let props = &mut self.pics[cur].props;
            props.side_data.push(SideData::S12mTimecode(codes));
            if text.is_some() {
                props.timecode = text;
            }
            pt.timecode_cnt = 0;
        }
        Ok(())
    }

    /// The current picture put among those waiting to be shown, and the
    /// next to show chosen: the lowest order count once as many are
    /// waiting as the stream reorders by (`h264_select_output_frame`).
    fn select_output_frame(&mut self, sps: &Sps) {
        let Some(cur) = self.cur_pic_ptr else {
            return;
        };
        self.pics[cur].mmco_reset = self.mmco_reset;
        self.mmco_reset = 0;

        if sps.bitstream_restriction_flag != 0 {
            self.avctx.has_b_frames = self.avctx.has_b_frames.max(sps.num_reorder_frames);
        }

        let cur_poc = self.pics[cur].poc;
        let mut i = 0;
        loop {
            if i == MAX_DPB_FRAMES || cur_poc < self.last_pocs[i] {
                if i != 0 {
                    self.last_pocs[i - 1] = cur_poc;
                }
                break;
            } else if i != 0 {
                self.last_pocs[i - 1] = self.last_pocs[i];
            }
            i += 1;
        }
        #[allow(
            clippy::cast_possible_wrap,
            clippy::cast_possible_truncation,
            reason = "under 17"
        )]
        let mut out_of_order = (MAX_DPB_FRAMES - i) as i32;
        let last = &self.last_pocs;
        if self.pics[cur].props.pict_type == TYPE_B
            || (last[MAX_DPB_FRAMES - 2] > i32::MIN
                && i64::from(last[MAX_DPB_FRAMES - 1]) - i64::from(last[MAX_DPB_FRAMES - 2]) > 2)
        {
            out_of_order = out_of_order.max(1);
        }
        #[allow(
            clippy::cast_possible_wrap,
            clippy::cast_possible_truncation,
            reason = "16"
        )]
        if out_of_order == MAX_DPB_FRAMES as i32 {
            self.last_pocs = [i32::MIN; MAX_DPB_FRAMES];
            self.last_pocs[0] = cur_poc;
            self.pics[cur].mmco_reset = 1;
        } else if self.avctx.has_b_frames < out_of_order && sps.bitstream_restriction_flag == 0 {
            self.avctx.has_b_frames = out_of_order;
        }

        let pics = self.delayed_pic.iter().take_while(|d| d.is_some()).count();
        if pics >= self.delayed_pic.len() - 1 {
            // FFmpeg asserts there is room: the DPB holds 16 frames.
            return;
        }
        self.delayed_pic[pics] = Some(cur);
        let pics = pics + 1;
        if self.pics[cur].reference == 0 {
            self.pics[cur].reference = DELAYED_PIC_REF;
        }

        let breaks = |me: &Self, p: usize| me.pics[p].props.key || me.pics[p].mmco_reset != 0;
        let Some(first) = self.delayed_pic[0] else {
            return;
        };
        let (mut out, mut out_idx) = (first, 0);
        let mut i = 1;
        while let Some(d) = self.delayed_pic[i] {
            if breaks(self, d) {
                break;
            }
            if self.pics[d].poc < self.pics[out].poc {
                out = d;
                out_idx = i;
            }
            i += 1;
        }
        if self.avctx.has_b_frames == 0 && breaks(self, first) {
            self.next_outputed_poc = i32::MIN;
        }
        let out_of_order = self.pics[out].poc < self.next_outputed_poc;

        #[allow(clippy::cast_sign_loss, reason = "has_b_frames is never negative")]
        let enough = pics > self.avctx.has_b_frames as usize;
        if out_of_order || enough {
            self.pics[out].reference &= !DELAYED_PIC_REF;
            self.delayed_pic.copy_within(out_idx + 1.., out_idx);
            if let Some(last) = self.delayed_pic.last_mut() {
                *last = None;
            }
        }
        if !out_of_order && enough {
            self.next_output_pic = Some(out);
            self.next_outputed_poc =
                if out_idx == 0 && self.delayed_pic[0].is_some_and(|d| breaks(self, d)) {
                    i32::MIN
                } else {
                    self.pics[out].poc
                };
            // A recovery point reached: everything after it in display
            // order is whole.
            self.frame_recovered |= self.pics[out].recovered;
            self.pics[out].recovered |= self.frame_recovered & FRAME_RECOVERED_SEI;
            if self.pics[out].recovered == 0 {
                if self.output_corrupt {
                    self.pics[out].props.corrupt = true;
                } else {
                    self.next_output_pic = None;
                }
            }
        }
    }

    /// The first slice of a frame or field taken up (`h264_field_start`).
    pub(crate) fn field_start(
        &mut self,
        sl: &mut SliceContext,
        nal: &Nal,
        first_slice: bool,
    ) -> Result<(), DecodeError> {
        self.init_ps(sl, first_slice)?;
        let sps = self.ps.sps.clone().ok_or(DecodeError)?;
        if sps.bitstream_restriction_flag != 0 && self.avctx.has_b_frames < sps.num_reorder_frames {
            self.avctx.has_b_frames = sps.num_reorder_frames;
        }

        let last_pic_droppable = self.droppable;
        let last_pic_structure = self.picture_structure;
        self.droppable = nal.ref_idc == 0;
        self.picture_structure = sl.hdr.picture_structure;

        self.poc.frame_num = sl.hdr.frame_num;
        self.poc.poc_lsb = sl.hdr.poc_lsb;
        self.poc.delta_poc_bottom = sl.hdr.delta_poc_bottom;
        self.poc.delta_poc = sl.hdr.delta_poc;

        // FFmpeg also sets it to 0 at a picture whose slices are all intra
        // (`picture_intra_only`), but works that out only when it decodes
        // with frame threads (`get_last_needed_nal`). Single-threaded, as
        // this decoder is held to it, the offset stays the last IDR's.
        if nal.kind == NAL_IDR_SLICE {
            self.poc_offset = sl.hdr.idr_pic_id;
        }

        let max_frame_num = 1 << sps.log2_max_frame_num;
        // A gap longer than the references kept is shortened: frames
        // allocated only to be thrown away would serve nothing.
        if self.poc.frame_num != self.poc.prev_frame_num {
            let mut unwrap_prev = self.poc.prev_frame_num;
            if unwrap_prev > self.poc.frame_num {
                unwrap_prev -= max_frame_num;
            }
            if self.poc.frame_num - unwrap_prev > sps.ref_frame_count {
                unwrap_prev = self.poc.frame_num - sps.ref_frame_count - 1;
                if unwrap_prev < 0 {
                    unwrap_prev += max_frame_num;
                }
                self.poc.prev_frame_num = unwrap_prev;
            }
        }

        if self.first_field != 0 {
            let cur = self.cur_pic_ptr.ok_or(DecodeError)?;
            let paired = self.field_picture()
                && self.picture_structure != last_pic_structure
                && self.pics[cur].frame_num == self.poc.frame_num;
            if paired {
                let complementary = (last_pic_structure == PICT_TOP_FIELD
                    && self.picture_structure == PICT_BOTTOM_FIELD)
                    || (last_pic_structure == PICT_BOTTOM_FIELD
                        && self.picture_structure == PICT_TOP_FIELD);
                if !complementary || last_pic_droppable != self.droppable {
                    self.picture_structure = last_pic_structure;
                    self.droppable = last_pic_droppable;
                    return Err(DecodeError);
                }
            }
        }

        while self.poc.frame_num != self.poc.prev_frame_num
            && self.first_field == 0
            && self.poc.frame_num != (self.poc.prev_frame_num + 1) % max_frame_num
        {
            let prev = if self.short_ref_count != 0 {
                self.short_ref[0]
            } else {
                None
            };
            if sps.gaps_in_frame_num_allowed_flag == 0 {
                self.last_pocs = [i32::MIN; MAX_DPB_FRAMES];
            }
            if self.frame_start(sl).is_err() {
                self.first_field = 0;
                return Err(DecodeError);
            }
            self.poc.prev_frame_num = (self.poc.prev_frame_num + 1) % max_frame_num;
            let gap = self.cur_pic_ptr.ok_or(DecodeError)?;
            self.pics[gap].frame_num = self.poc.prev_frame_num;
            self.pics[gap].invalid_gap = i32::from(sps.gaps_in_frame_num_allowed_flag == 0);

            self.explicit_ref_marking = 0;
            if self.execute_ref_pic_marking().is_err() {
                return Err(DecodeError);
            }
            // A missing reference concealed with the one before it, or
            // left gray when there is none to copy.
            if self.short_ref_count != 0 {
                let Some(first) = self.short_ref[0] else {
                    return Err(DecodeError);
                };
                match prev {
                    Some(p) if self.pics[first].same_shape(&self.pics[p]) => {
                        self.pics[first].f = self.pics[p].f.clone();
                        self.pics[first].props = self.pics[p].props.clone();
                        self.pics[first].film_grain = self.pics[p].film_grain.clone();
                        self.pics[first].poc = self.pics[p].poc.wrapping_add(2);
                        self.pics[first].gray = self.pics[p].gray;
                    }
                    _ if self.frame_recovered == 0 => {
                        let c = [
                            1 << (sps.bit_depth_luma - 1),
                            1 << (sps.bit_depth_chroma - 1),
                            1 << (sps.bit_depth_chroma - 1),
                        ];
                        if let Some(f) = self.pics[first].f.as_mut().and_then(Arc::get_mut) {
                            f.fill(c);
                        }
                        self.pics[first].gray = 1;
                    }
                    _ => {}
                }
                self.pics[first].frame_num = self.poc.prev_frame_num;
            }
        }

        if self.first_field != 0 {
            let cur = self.cur_pic_ptr.ok_or(DecodeError)?;
            if !self.field_picture() || self.picture_structure == last_pic_structure {
                // The field before was unmatched: kept as a reference if it
                // is one, never shown.
                self.missing_fields += 1;
                self.cur_pic_ptr = None;
                self.first_field = i32::from(self.field_picture());
            } else {
                self.missing_fields = 0;
                if self.pics[cur].frame_num != self.poc.frame_num
                    || self.pics[cur].reference & DELAYED_PIC_REF != 0
                {
                    // A different frame, or one already shown and so not to
                    // be drawn into again: this field starts a pair.
                    self.first_field = 1;
                    self.cur_pic_ptr = None;
                } else {
                    self.first_field = 0;
                }
            }
        } else {
            self.first_field = i32::from(self.field_picture());
        }

        if !self.field_picture() || self.first_field != 0 {
            if self.frame_start(sl).is_err() {
                self.first_field = 0;
                return Err(DecodeError);
            }
        } else {
            self.release_unused_pictures(false);
        }

        // Macroblocks can be read before they are decoded where slices are
        // lost: none counts as decoded yet.
        #[allow(clippy::cast_sign_loss, reason = "macroblock counts")]
        let (stride, height) = (self.mb_stride as usize, self.mb_height as usize);
        let base = crate::picture::table_offset(self.mb_stride);
        if self.field_picture() {
            for i in usize::from(self.picture_structure == PICT_BOTTOM_FIELD)..height {
                let n = stride - usize::from(i + 1 == height);
                if let Some(row) = self
                    .slice_table
                    .get_mut(base + i * stride..base + i * stride + n)
                {
                    row.fill(0xFFFF);
                }
            }
        } else if let Some(all) = self
            .slice_table
            .get_mut(base..base + (height * stride).saturating_sub(1))
        {
            all.fill(0xFFFF);
        }

        let cur = self.cur_pic_ptr.ok_or(DecodeError)?;
        let mut field_poc = self.pics[cur].field_poc;
        let mut poc = self.pics[cur].poc;
        init_poc(
            &mut field_poc,
            &mut poc,
            &sps,
            &mut self.poc,
            self.picture_structure,
            nal.ref_idc,
        )
        .map_err(|_| DecodeError)?;
        self.pics[cur].field_poc = field_poc;
        self.pics[cur].poc = poc;

        let n = sl.hdr.nb_mmco;
        self.mmco[..n].copy_from_slice(&sl.hdr.mmco[..n]);
        self.nb_mmco = n;
        self.explicit_ref_marking = sl.hdr.explicit_ref_marking;
        self.picture_idr = nal.kind == NAL_IDR_SLICE;

        let mask = max_frame_num - 1;
        let cnt = self.sei.recovery_frame_cnt;
        if cnt >= 0 {
            if self.poc.frame_num != cnt || sl.hdr.slice_type_nos != TYPE_I {
                self.valid_recovery_point = true;
            }
            if self.recovery_frame < 0 || ((self.recovery_frame - self.poc.frame_num) & mask) > cnt
            {
                self.recovery_frame = (self.poc.frame_num + cnt) & mask;
                if !self.valid_recovery_point {
                    self.recovery_frame = self.poc.frame_num;
                }
            }
        }

        if nal.kind == NAL_IDR_SLICE {
            self.pics[cur].props.key = true;
            self.pics[cur].recovered |= FRAME_RECOVERED_IDR;
            // After an IDR everything in decoding order is whole.
            self.frame_recovered |= FRAME_RECOVERED_IDR;
        }
        if self.recovery_frame == self.poc.frame_num && nal.ref_idc != 0 {
            self.recovery_frame = -1;
            self.pics[cur].recovered |= FRAME_RECOVERED_SEI;
        }
        self.pics[cur].recovered |= self.frame_recovered;

        // A field pair's properties are set by its second field, which
        // the SEI of both have been read for.
        if !self.field_picture() || self.first_field == 0 || self.missing_fields > 1 {
            self.export_frame_props(&sps)?;
            self.select_output_frame(&sps);
        }
        Ok(())
    }

    /// A frame or field finished (`ff_h264_field_end`): a reference marked,
    /// the order count and frame number kept for the next picture's, and
    /// the picture's film grain made -- once both its fields are decoded,
    /// and unless it is finished only to begin the next picture of the same
    /// packet (`in_setup`), where FFmpeg makes none.
    pub(crate) fn field_end(&mut self, in_setup: bool) -> Result<(), DecodeError> {
        let mut err = Ok(());
        self.mb_y = 0;
        if !self.droppable {
            err = self.execute_ref_pic_marking().map_err(|_| DecodeError);
            self.poc.prev_poc_msb = self.poc.poc_msb;
            self.poc.prev_poc_lsb = self.poc.poc_lsb;
        }
        self.poc.prev_frame_num_offset = self.poc.frame_num_offset;
        self.poc.prev_frame_num = self.poc.frame_num;

        let whole = !self.field_picture() || self.first_field == 0;
        if let Some(cur) = self
            .cur_pic_ptr
            .filter(|&c| !in_setup && whole && self.pics[c].needs_fg)
        {
            // A picture whose grain cannot be made is shown without it. In
            // FFmpeg this branch sets the error it returns either way, so a
            // marking error goes unreported here.
            if !self.make_film_grain(cur) {
                self.pics[cur].needs_fg = false;
            }
            err = Ok(());
        }
        self.current_slice = 0;
        err
    }

    /// Picture `cur`'s grain made, as the frame it is shown as (`f_grain`,
    /// `ff_h274_apply_film_grain`): whether it was -- not where the picture
    /// has no grain parameters (FFmpeg's side data was never attached,
    /// decoding having failed first), or they cannot be applied to it.
    fn make_film_grain(&mut self, cur: usize) -> bool {
        let pic = &mut self.pics[cur];
        // Beyond 8 bits the picture is refused, as FFmpeg refuses all but
        // yuv420p.
        let (Some(params), Some(AnyFrame::Eight(input))) =
            (pic.film_grain.as_deref(), pic.f.as_deref())
        else {
            return false;
        };
        match apply_film_grain(input, pic.props.format, params) {
            Ok(grained) => {
                pic.f_grain = Some(Arc::new(AnyFrame::Eight(grained)));
                true
            }
            Err(_) => false,
        }
    }
}
