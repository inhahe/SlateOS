//! Picture order counts: where each picture -- each field -- falls in
//! display order (`ff_h264_init_poc` in `h264_parse.c`, and the state it
//! keeps, `H264POCContext`).
//!
//! A stream carries its pictures in decoding order. The picture order
//! count is the number that puts them back in display order, worked out
//! one of three ways the sequence parameter set chooses: from bits each
//! slice carries (type 0), from a cycle of expected increments (type 1),
//! or from `frame_num` alone (type 2).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic; where it can overflow it is written wrapping"
)]

use crate::ps::Sps;
use crate::slice::{PICT_BOTTOM_FIELD, PICT_FRAME, PICT_TOP_FIELD};

/// What picture order counts are worked out from, kept from one picture to
/// the next (`H264POCContext`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PocContext {
    pub(crate) poc_lsb: i32,
    pub(crate) poc_msb: i32,
    pub(crate) delta_poc_bottom: i32,
    pub(crate) delta_poc: [i32; 2],
    pub(crate) frame_num: i32,
    /// `poc_msb` of the last reference picture, for type 0.
    pub(crate) prev_poc_msb: i32,
    /// `poc_lsb` of the last reference picture, for type 0; below 0 when
    /// there has been none since the last IDR.
    pub(crate) prev_poc_lsb: i32,
    /// For types 1 and 2.
    pub(crate) frame_num_offset: i32,
    pub(crate) prev_frame_num_offset: i32,
    /// `frame_num` of the last picture, for types 1 and 2.
    pub(crate) prev_frame_num: i32,
}

impl Default for PocContext {
    /// As a decoder starts: what `h264_init_context` sets, then
    /// `ff_h264_flush_change`.
    fn default() -> Self {
        Self {
            poc_lsb: 0,
            poc_msb: 0,
            delta_poc_bottom: 0,
            delta_poc: [0, 0],
            frame_num: 0,
            prev_poc_msb: 1 << 16,
            prev_poc_lsb: -1,
            frame_num_offset: 0,
            prev_frame_num_offset: 0,
            prev_frame_num: -1,
        }
    }
}

/// A picture order count past what an `int` holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PocOverflow;

/// The order counts of the picture `pc` describes (`ff_h264_init_poc`):
/// each field it has into `field_poc` -- a field picture leaves the other
/// field's as it was -- and the lower of the two into `poc`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "checked against the int range first, as FFmpeg checks"
)]
pub(crate) fn init_poc(
    field_poc: &mut [i32; 2],
    poc: &mut i32,
    sps: &Sps,
    pc: &mut PocContext,
    picture_structure: i32,
    nal_ref_idc: u32,
) -> Result<(), PocOverflow> {
    let max_frame_num = 1i32 << sps.log2_max_frame_num;
    let mut fp = [0i64; 2];

    pc.frame_num_offset = pc.prev_frame_num_offset;
    if pc.frame_num < pc.prev_frame_num {
        pc.frame_num_offset = pc.frame_num_offset.wrapping_add(max_frame_num);
    }

    if sps.poc_type == 0 {
        let max_poc_lsb = 1i32 << sps.log2_max_poc_lsb;
        if pc.prev_poc_lsb < 0 {
            pc.prev_poc_lsb = pc.poc_lsb;
        }
        let gap = pc.prev_poc_lsb.wrapping_sub(pc.poc_lsb);
        pc.poc_msb = if pc.poc_lsb < pc.prev_poc_lsb && gap >= max_poc_lsb / 2 {
            pc.prev_poc_msb.wrapping_add(max_poc_lsb)
        } else if pc.poc_lsb > pc.prev_poc_lsb && gap < -max_poc_lsb / 2 {
            pc.prev_poc_msb.wrapping_sub(max_poc_lsb)
        } else {
            pc.prev_poc_msb
        };
        let both = i64::from(pc.poc_msb.wrapping_add(pc.poc_lsb));
        fp = [both, both];
        if picture_structure == PICT_FRAME {
            fp[1] += i64::from(pc.delta_poc_bottom);
        }
    } else if sps.poc_type == 1 {
        let cycle = sps.poc_cycle_length;
        let mut abs_frame_num = if cycle != 0 {
            pc.frame_num_offset.wrapping_add(pc.frame_num)
        } else {
            0
        };
        if nal_ref_idc == 0 && abs_frame_num > 0 {
            abs_frame_num -= 1;
        }

        let offsets = sps.offset_for_ref_frame.iter().map(|&o| i64::from(o));
        let cycle_len = usize::try_from(cycle).unwrap_or(0);
        let expected_delta_per_cycle = offsets
            .clone()
            .take(cycle_len)
            .fold(0i64, i64::wrapping_add);

        let mut expected = 0i64;
        if abs_frame_num > 0 {
            // Only with a cycle: without one the frame number is 0.
            let cycles = (abs_frame_num - 1) / cycle;
            let in_cycle = (abs_frame_num - 1) % cycle;
            expected = i64::from(cycles).wrapping_mul(expected_delta_per_cycle);
            let through = usize::try_from(in_cycle).unwrap_or(0) + 1;
            expected = offsets.take(through).fold(expected, i64::wrapping_add);
        }
        if nal_ref_idc == 0 {
            expected = expected.wrapping_add(i64::from(sps.offset_for_non_ref_pic));
        }
        fp[0] = expected.wrapping_add(i64::from(pc.delta_poc[0]));
        fp[1] = fp[0].wrapping_add(i64::from(sps.offset_for_top_to_bottom_field));
        if picture_structure == PICT_FRAME {
            fp[1] = fp[1].wrapping_add(i64::from(pc.delta_poc[1]));
        }
    } else {
        let mut both = pc
            .frame_num_offset
            .wrapping_add(pc.frame_num)
            .wrapping_mul(2);
        if nal_ref_idc == 0 {
            both = both.wrapping_sub(1);
        }
        fp = [i64::from(both), i64::from(both)];
    }

    if fp.iter().any(|&p| i64::from(p as i32) != p) {
        return Err(PocOverflow);
    }
    if picture_structure != PICT_BOTTOM_FIELD {
        field_poc[0] = fp[0] as i32;
    }
    if picture_structure != PICT_TOP_FIELD {
        field_poc[1] = fp[1] as i32;
    }
    *poc = field_poc[0].min(field_poc[1]);
    Ok(())
}

#[cfg(test)]
mod tests;
