//! A slice taken up: its header read, the picture it belongs to started
//! or continued, and the slice set up for its macroblocks
//! (`ff_h264_queue_decode_slice`, `h264_slice_init` and
//! `implicit_weight_table` in `h264_slice.c`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on macroblock addresses it checks against the picture first"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "reference indices under the slice's counts, at most 32; slice numbers masked to 32"
)]

use std::sync::Arc;

use crate::bits::Bits;
use crate::decoder::{Context, DecodeError, MAX_SLICES, SliceContext};
use crate::direct::{clip_int8, low_int};
use crate::nal::{NAL_IDR_SLICE, Nal};
use crate::slice::{PICT_BOTTOM_FIELD, PICT_FRAME, PictureState, TYPE_B, TYPE_I};

impl Context {
    /// The implicit weights of each pair of references, by how far each is
    /// from the current picture or, for `field` 0 or 1, from that field of
    /// an MBAFF frame (`implicit_weight_table`).
    fn implicit_weight_table(&self, sl: &mut SliceContext, field: Option<usize>) {
        let Some(cur) = self.cur_pic_ptr else {
            return;
        };
        sl.hdr.pwt.luma_weight_flag = [0; 2];
        sl.hdr.pwt.chroma_weight_flag = [0; 2];
        let (cur_poc, ref_start, ref_count0, ref_count1);
        if let Some(field) = field {
            cur_poc = self.pics[cur].field_poc[field];
            ref_start = 16;
            ref_count0 = 16 + 2 * sl.hdr.ref_count[0] as usize;
            ref_count1 = 16 + 2 * sl.hdr.ref_count[1] as usize;
        } else {
            cur_poc = if self.picture_structure == PICT_FRAME {
                self.pics[cur].poc
            } else {
                #[allow(clippy::cast_sign_loss, reason = "a field's structure, 1 or 2")]
                let f = (self.picture_structure - 1) as usize;
                self.pics[cur].field_poc[f]
            };
            if sl.hdr.ref_count == [1, 1]
                && !self.mb_aff_frame
                && i64::from(sl.ref_list[0][0].poc) + i64::from(sl.ref_list[1][0].poc)
                    == 2 * i64::from(cur_poc)
            {
                sl.hdr.pwt.use_weight = 0;
                sl.hdr.pwt.use_weight_chroma = 0;
                return;
            }
            ref_start = 0;
            ref_count0 = sl.hdr.ref_count[0] as usize;
            ref_count1 = sl.hdr.ref_count[1] as usize;
        }

        let pwt = &mut sl.hdr.pwt;
        pwt.use_weight = 2;
        pwt.use_weight_chroma = 2;
        pwt.luma_log2_weight_denom = 5;
        pwt.chroma_log2_weight_denom = 5;
        let long = |r: crate::picture::Ref| r.parent.is_some_and(|p| self.pics[p].long_ref != 0);
        for ref0 in ref_start..ref_count0 {
            let r0 = sl.ref_list[0][ref0];
            let poc0 = i64::from(r0.poc);
            for ref1 in ref_start..ref_count1 {
                let r1 = sl.ref_list[1][ref1];
                let mut w = 32;
                if !long(r0) && !long(r1) {
                    let td = clip_int8(low_int(i64::from(r1.poc) - poc0));
                    if td != 0 {
                        let tb = clip_int8(low_int(i64::from(cur_poc) - poc0));
                        let tx = (16384 + (td.abs() >> 1)) / td;
                        let dist_scale_factor = (tb * tx + 32) >> 8;
                        if (-64..=128).contains(&dist_scale_factor) {
                            w = 64 - dist_scale_factor;
                        }
                    }
                }
                match field {
                    None => pwt.implicit_weight[ref0][ref1] = [w, w],
                    Some(f) => pwt.implicit_weight[ref0][ref1][f] = w,
                }
            }
        }
    }

    /// The slice set up for its macroblocks (`h264_slice_init`): where it
    /// starts, its reference lists, implicit weights, direct prediction,
    /// the loop filter's thresholds and reference numbers, and whether it
    /// predicts from a gray stand-in.
    fn slice_init(&mut self, sl: &mut SliceContext, nal: &Nal) -> Result<(), DecodeError> {
        if self.picture_idr && nal.kind != NAL_IDR_SLICE {
            return Err(DecodeError);
        }
        let field_or_mbaff = u32::from(self.picture_structure != PICT_FRAME || self.mb_aff_frame);
        #[allow(clippy::cast_sign_loss, reason = "a macroblock count")]
        let mb_num = self.mb_num as u32;
        let first = sl.hdr.first_mb_addr;
        if first << field_or_mbaff >= mb_num || first >= mb_num {
            return Err(DecodeError);
        }
        #[allow(clippy::cast_possible_wrap, reason = "under the macroblock count")]
        let first = first as i32;
        sl.mb_x = first % self.mb_width;
        sl.mb_y = (first / self.mb_width) << field_or_mbaff;
        if self.picture_structure == PICT_BOTTOM_FIELD {
            sl.mb_y += 1;
        }
        sl.resync_mb_x = sl.mb_x;
        sl.resync_mb_y = sl.mb_y;

        self.build_ref_list(sl).map_err(|_| DecodeError)?;

        let pps = self.ps.pps.clone().ok_or(DecodeError)?;
        if pps.weighted_bipred_idc == 2 && sl.hdr.slice_type_nos == TYPE_B {
            self.implicit_weight_table(sl, None);
            if self.mb_aff_frame {
                self.implicit_weight_table(sl, Some(0));
                self.implicit_weight_table(sl, Some(1));
            }
        }
        if sl.hdr.slice_type_nos == TYPE_B && sl.hdr.direct_spatial_mv_pred == 0 {
            self.direct_dist_scale_factor(sl);
        }
        self.direct_ref_list_init(sl);

        let sps = self.ps.sps.clone().ok_or(DecodeError)?;
        sl.qp_thresh = 15
            - sl.hdr.slice_alpha_c0_offset.min(sl.hdr.slice_beta_offset)
            - 0.max(pps.chroma_qp_index_offset[0])
                .max(pps.chroma_qp_index_offset[1])
            + 6 * (sps.bit_depth_luma - 8);

        // The slice table holds 16 bits, 0xFFFF meaning none.
        if self.current_slice >= 0xFFFE {
            return Err(DecodeError);
        }
        self.current_slice += 1;
        sl.slice_num = self.current_slice;
        #[allow(clippy::cast_sign_loss, reason = "a slice number, from 1")]
        let slot = sl.slice_num as usize;
        if slot != 0 {
            self.slice_row[(slot - 1) & (MAX_SLICES - 1)] = sl.resync_mb_y;
        }

        for j in 0..2 {
            let mut id_list = [60i32; 16];
            for (i, id) in id_list.iter_mut().enumerate() {
                if j < sl.hdr.list_count as usize && i < sl.hdr.ref_count[j] as usize {
                    let Some(buf) = sl.ref_list[j][i]
                        .parent
                        .and_then(|p| self.pics[p].f.clone())
                    else {
                        continue;
                    };
                    let same = |p: Option<usize>| {
                        p.and_then(|p| self.pics[p].f.as_ref())
                            .is_some_and(|f| Arc::ptr_eq(f, &buf))
                    };
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_possible_wrap,
                        reason = "under 48"
                    )]
                    if let Some(k) = (0..self.short_ref_count).find(|&k| same(self.short_ref[k])) {
                        *id = k as i32;
                    }
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_possible_wrap,
                        reason = "under 48"
                    )]
                    if let Some(k) = (0..self.long_ref_count).find(|&k| same(self.long_ref[k])) {
                        *id = (self.short_ref_count + k) as i32;
                    }
                }
            }
            let ref2frm = &mut self.ref2frm[slot & (MAX_SLICES - 1)][j];
            ref2frm[0] = -1;
            ref2frm[1] = -1;
            for i in 0..16 {
                ref2frm[i + 2] = 4 * id_list[i] + (sl.ref_list[j][i].reference & 3);
            }
            ref2frm[18] = -1;
            ref2frm[19] = -1;
            for i in 16..48 {
                ref2frm[i + 4] = 4 * id_list[(i - 16) >> 1] + (sl.ref_list[j][i].reference & 3);
            }
        }

        let cur = self.cur_pic_ptr.ok_or(DecodeError)?;
        if sl.hdr.slice_type_nos == TYPE_I {
            self.pics[cur].gray = 0;
            self.non_gray = 1;
        } else {
            let mut gray = 0;
            for j in 0..sl.hdr.list_count as usize {
                for i in 0..sl.hdr.ref_count[j] as usize {
                    gray |= sl.ref_list[j][i].parent.map_or(0, |p| self.pics[p].gray);
                }
            }
            self.pics[cur].gray = gray;
        }
        #[cfg(test)]
        self.debug_slice(sl, nal);
        Ok(())
    }

    /// A slice's header read and the slice queued to be decoded
    /// (`ff_h264_queue_decode_slice`): a redundant picture's passed over, a
    /// new field or frame started where the slice begins one.
    pub(crate) fn queue_decode_slice(
        &mut self,
        sl: &mut SliceContext,
        nal: &Nal,
        gb: &mut Bits<'_>,
    ) -> Result<(), DecodeError> {
        let first_slice = self.current_slice == 0;
        let picture = PictureState {
            first_slice,
            frame_num: self.poc.frame_num,
        };
        if sl.hdr.parse(gb, nal, &self.ps, picture).is_err() {
            // A slice whose picture parameter set was refused as something
            // FFmpeg does not decode: the packet fails for that reason.
            let id = sl.hdr.pps_id as usize;
            if self.ps.pps_list.get(id).is_some_and(Option::is_none)
                && self.ps.unsupported_pps.get(id) == Some(&true)
            {
                self.refusal = Some(crate::Error::Unsupported);
            }
            return Err(DecodeError);
        }

        if sl.hdr.redundant_pic_count > 0 {
            sl.hdr.ref_count = [0, 0];
            return Ok(());
        }
        if (sl.hdr.first_mb_addr == 0 || self.current_slice == 0) && self.setup_finished {
            return Err(DecodeError);
        }

        if sl.hdr.first_mb_addr == 0 {
            // This slice starts a new field. FFmpeg first decodes the
            // slices it has queued; with one slice context, as here, each
            // was decoded as it was queued.
            if self.current_slice != 0 {
                if self.cur_pic_ptr.is_some() && self.field_picture() && self.first_field != 0 {
                    self.field_end()?;
                } else if self.cur_pic_ptr.is_some()
                    && !self.field_picture()
                    && self.first_field == 0
                    && self.nal_unit_type == NAL_IDR_SLICE
                {
                    // Broken frame packetizing.
                    let r = self.field_end();
                    self.cur_pic_ptr = None;
                    r?;
                } else {
                    return Err(DecodeError);
                }
            }
            if self.first_field == 0 {
                self.cur_pic_ptr = None;
            }
        }

        if !first_slice {
            let pps = self
                .ps
                .pps_list
                .get(sl.hdr.pps_id as usize)
                .cloned()
                .flatten();
            let (Some(cur), Some(new)) = (self.ps.pps.as_ref(), pps) else {
                return Err(DecodeError);
            };
            if cur.sps_id != new.sps_id || cur.transform_8x8_mode != new.transform_8x8_mode {
                return Err(DecodeError);
            }
            if self
                .ps
                .sps
                .as_ref()
                .is_none_or(|s| !Arc::ptr_eq(s, &new.sps))
            {
                return Err(DecodeError);
            }
        }

        if self.current_slice == 0 {
            self.field_start(sl, nal, first_slice)?;
        } else if self.picture_structure != sl.hdr.picture_structure
            || self.droppable != (nal.ref_idc == 0)
            || self.cur_pic_ptr.is_none()
        {
            return Err(DecodeError);
        }

        self.slice_init(sl, nal)?;
        self.nb_slice_ctx_queued += 1;
        Ok(())
    }

    /// The queued slice decoded, its macroblocks read from `gb` where its
    /// header left off (`ff_h264_execute_decode_slices` with one slice
    /// context: FFmpeg's without slice threads).
    pub(crate) fn execute_decode_slices(
        &mut self,
        sl: &mut SliceContext,
        gb: &mut Bits<'_>,
    ) -> Result<(), DecodeError> {
        sl.next_slice_idx = i32::MAX;
        if self.nb_slice_ctx_queued < 1 {
            return Ok(());
        }
        // One slice context: the slice may run to the picture's end.
        sl.next_slice_idx = self.mb_width * self.mb_height;
        self.postpone_filter = false;
        let r = self.decode_slice(sl, gb);
        self.mb_y = sl.mb_y;
        self.nb_slice_ctx_queued = 0;
        r
    }

    /// A slice's macroblocks decoded (`decode_slice`, in `slicedata`).
    fn decode_slice(
        &mut self,
        sl: &mut SliceContext,
        gb: &mut Bits<'_>,
    ) -> Result<(), DecodeError> {
        sl.alloc_top_borders(usize::try_from(self.mb_width).unwrap_or(0));
        let r = self.decode_slice_data(sl, gb);
        #[cfg(test)]
        if r.is_err() {
            let at = format!(
                "slice {} stopped at macroblock ({}, {})",
                sl.slice_num, sl.mb_x, sl.mb_y
            );
            self.debug_errors.push(at);
        }
        r
    }
}

#[cfg(test)]
impl Context {
    /// FFmpeg's `-debug pict` line for a slice (`h264_slice_init`).
    fn debug_slice(&mut self, sl: &SliceContext, nal: &Nal) {
        use crate::slice::{TYPE_P, TYPE_SI, TYPE_SP};
        let hdr = &sl.hdr;
        let structure = match self.picture_structure {
            PICT_FRAME => 'F',
            crate::slice::PICT_TOP_FIELD => 'T',
            _ => 'B',
        };
        let kind = match hdr.slice_type {
            TYPE_I => 'I',
            TYPE_P => 'P',
            TYPE_B => 'B',
            TYPE_SI => 'i',
            TYPE_SP => 'p',
            _ => '?',
        };
        let fp = self.cur_pic_ptr.map_or([0, 0], |c| self.pics[c].field_poc);
        let line = format!(
            "slice:{} {} mb:{} {}{}{} frame:{} poc:{}/{} ref:{}/{} qp:{} loop:{}:{}:{} weight:{}{} {}",
            sl.slice_num,
            structure,
            sl.mb_y * self.mb_width + sl.mb_x,
            kind,
            if hdr.slice_type_fixed != 0 {
                " fix"
            } else {
                ""
            },
            if nal.kind == NAL_IDR_SLICE {
                " IDR"
            } else {
                ""
            },
            self.poc.frame_num,
            fp[0],
            fp[1],
            hdr.ref_count[0],
            hdr.ref_count[1],
            hdr.qscale,
            hdr.deblocking_filter,
            hdr.slice_alpha_c0_offset,
            hdr.slice_beta_offset,
            hdr.pwt.use_weight,
            if hdr.pwt.use_weight == 1 && hdr.pwt.use_weight_chroma != 0 {
                "c"
            } else {
                ""
            },
            if hdr.slice_type == TYPE_B {
                if hdr.direct_spatial_mv_pred != 0 {
                    "SPAT"
                } else {
                    "TEMP"
                }
            } else {
                ""
            },
        );
        self.debug.push(line);
    }
}
