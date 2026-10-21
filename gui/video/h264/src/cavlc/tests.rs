//! The residual reader held to FFmpeg's where a damaged stream runs a
//! block's zeros out before its scan's start (`decode_residual`'s
//! STORE_BLOCK): the positions read there can lie past the block, and
//! FFmpeg stores the coefficients there, dequantized by the QP rows after
//! the block's -- where a later block's transform takes them up.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_wrap,
    reason = "test code, on values the test chooses"
)]

use super::TABLES;
use crate::decoder::{Context, SliceContext};
use crate::mbtype::{MB_TYPE_16X16, MB_TYPE_L0};
use crate::ps::{ParamSets, decode_pps, decode_sps};
use crate::scan::{FIELD, FIELD8X8_CAVLC};
use crate::writer::{Syntax, read_unit};
use ffbits::Bits;

/// A Main profile SPS and a CAVLC PPS: 4:2:0 at 8 bits, flat scaling.
fn sets() -> ParamSets {
    let mut s = Syntax::default();
    s.u("profile_idc", 8, 77)
        .u("constraint_flags", 6, 0)
        .u("reserved_zero_2bits", 2, 0)
        .u("level_idc", 8, 30)
        .ue("seq_parameter_set_id", 0)
        .ue("log2_max_frame_num_minus4", 0)
        .ue("pic_order_cnt_type", 2)
        .ue("max_num_ref_frames", 1)
        .u("gaps_in_frame_num_allowed_flag", 1, 0)
        .ue("pic_width_in_mbs_minus1", 10)
        .ue("pic_height_in_map_units_minus1", 8)
        .u("frame_mbs_only_flag", 1, 1)
        .u("direct_8x8_inference_flag", 1, 1)
        .u("frame_cropping_flag", 1, 0)
        .u("vui_parameters_present_flag", 1, 0);
    let sps = s.finish(0x67);
    let mut p = Syntax::default();
    p.ue("pic_parameter_set_id", 0)
        .ue("seq_parameter_set_id", 0)
        .u("entropy_coding_mode_flag", 1, 0)
        .u("bottom_field_pic_order_in_frame_present_flag", 1, 0)
        .ue("num_slice_groups_minus1", 0)
        .ue("num_ref_idx_l0_default_active_minus1", 0)
        .ue("num_ref_idx_l1_default_active_minus1", 0)
        .u("weighted_pred_flag", 1, 0)
        .u("weighted_bipred_idc", 2, 0)
        .se("pic_init_qp_minus26", 0)
        .se("pic_init_qs_minus26", 0)
        .se("chroma_qp_index_offset", 0)
        .u("deblocking_filter_control_present_flag", 1, 1)
        .u("constrained_intra_pred_flag", 1, 0)
        .u("redundant_pic_cnt_present_flag", 1, 0);
    let pps = p.finish(0x68);
    let mut ps = ParamSets::default();
    read_unit(&sps, |gb, _| decode_sps(gb, &mut ps, false).unwrap());
    read_unit(&pps, |gb, nal| {
        decode_pps(gb, &mut ps, nal.size_bits).unwrap();
    });
    ps
}

/// Bits written as strings of 0s and 1s, then zero bytes for the reader to
/// look ahead into.
fn bytes(bits: &[&str]) -> Vec<u8> {
    let all = bits.concat();
    let mut out = vec![0u8; all.len().div_ceil(8) + 8];
    for (i, c) in all.bytes().enumerate() {
        if c == b'1' {
            out[i / 8] |= 0x80 >> (i % 8);
        }
    }
    out
}

#[test]
fn coefficients_read_before_the_scan_are_stored_past_the_block_as_ffmpeg_stores_them() {
    let mut ps = sets();
    ps.pps = ps.pps_list[0].clone();
    let pps = ps.pps.clone().unwrap();
    let h = Context {
        ps,
        ..Context::default()
    };
    let mut sl = SliceContext::default();
    sl.hdr.qscale = 26;
    // An inter field macroblock's first 4x4 block, its 8x8 coded: two
    // coefficients, both trailing ones (+1, +1: coeff_token 001, signs 00);
    // seven zeros before the last (total_zeros 0011); then a run of
    // fourteen before the other (run_before 00000000001), which takes the
    // zeros to -7 -- FFmpeg's "negative number of zero coeffs", after its
    // stores. The second coefficient's position is read seven places before
    // the field scan, in the CAVLC 8x8 zigzag's tail.
    let data = bytes(&["001", "00", "0011", "00000000001"]);
    let mut gb = Bits::whole(&data).unwrap();
    let t = TABLES.as_ref().unwrap();
    let mb_type = MB_TYPE_16X16 | MB_TYPE_L0;
    let r = h.decode_luma_residual(t, &mut sl, &mut gb, FIELD, FIELD8X8_CAVLC, mb_type, 1, 0);
    assert!(r.is_err(), "the zeros run out: a decode error");

    let first = usize::from(h.scans.all[FIELD + 8]);
    let stray = usize::from(h.scans.all[FIELD - 7]);
    assert!(
        stray >= 16,
        "the position read before the scan lies past the block: {stray}"
    );
    // Inter luma's list (3); a factor past the row is the next QPs'.
    let dequantized = |k: usize| {
        let factor = pps.dequant4(3, 26 + k / 16).unwrap()[k % 16];
        ((factor + 32) >> 6) as i32
    };
    assert_eq!(
        sl.mb[first],
        dequantized(first),
        "the coefficient inside the block"
    );
    assert_eq!(
        sl.mb[stray],
        dequantized(stray),
        "the coefficient past it, by the factors past the block's row"
    );
    assert_eq!(
        sl.mb.iter().filter(|&&c| c != 0).count(),
        2,
        "nothing else stored"
    );
}
