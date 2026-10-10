#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]

mod fixtures;

use super::*;
use crate::writer::{Syntax, read_unit};
use fixtures::ALL;

/// A trace, read in order: what FFmpeg's `trace_headers` printed for one
/// parameter set -- or, for a set written here, what was written.
struct Trace<'t> {
    fields: &'t [(String, i64)],
    at: usize,
}

impl<'t> Trace<'t> {
    fn new(fields: &'t [(String, i64)]) -> Self {
        Self { fields, at: 0 }
    }

    /// The first element of this name.
    fn get(&self, name: &str) -> Option<i64> {
        self.fields
            .iter()
            .find(|(n, _)| *n == name)
            .map(|&(_, v)| v)
    }

    /// The first element of this name, as an `i32`; 0 when absent.
    fn int(&self, name: &str) -> i32 {
        self.get(name).unwrap_or(0) as i32
    }

    /// The next element named `name` from where the last one was found:
    /// for the repeated ones, read in order.
    fn next(&mut self, name: &str) -> i64 {
        let k = self.fields[self.at..]
            .iter()
            .position(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("no more {name}"));
        self.at += k + 1;
        self.fields[self.at - 1].1
    }

    fn next_prefixed(&mut self, prefix: &str) -> i64 {
        let k = self.fields[self.at..]
            .iter()
            .position(|(n, _)| n.starts_with(prefix))
            .unwrap_or_else(|| panic!("no more {prefix}"));
        self.at += k + 1;
        self.fields[self.at - 1].1
    }
}

/// A sequence and a picture parameter set as they are in a stream, and the
/// trace of each.
struct Sets {
    name: String,
    sps: Vec<u8>,
    pps: Vec<u8>,
    sps_trace: Vec<(String, i64)>,
    pps_trace: Vec<(String, i64)>,
}

fn owned(trace: &[(&str, i64)]) -> Vec<(String, i64)> {
    trace.iter().map(|&(n, v)| (n.to_owned(), v)).collect()
}

/// Every set the tests read: the conformance streams' and those written
/// here for what no conformance stream does.
fn all_sets() -> Vec<Sets> {
    let mut v: Vec<Sets> = ALL
        .iter()
        .map(|f| Sets {
            name: f.name.to_owned(),
            sps: f.sps.to_vec(),
            pps: f.pps.to_vec(),
            sps_trace: owned(f.sps_trace),
            pps_trace: owned(f.pps_trace),
        })
        .collect();
    v.push(written_fallbacks());
    v
}

fn parse(f: &Sets) -> ParamSets {
    let mut ps = ParamSets::default();
    read_unit(&f.sps, |gb, _| decode_sps(gb, &mut ps, false).unwrap());
    read_unit(&f.pps, |gb, nal| {
        decode_pps(gb, &mut ps, nal.size_bits).unwrap();
    });
    ps
}

// ---- parameter sets written here ------------------------------------------

/// A list of `n` values counting up from `start`: plainly not a default.
fn ramp(n: usize, start: u8) -> Vec<u8> {
    (0..n).map(|k| start.wrapping_add(k as u8 % 64)).collect()
}

/// What no conformance stream does: an SPS whose first lists are its own
/// and whose others fall back to them, and a PPS that sends some lists and
/// leaves others to fall back to the SPS's (rule B of Table 7-2).
fn written_fallbacks() -> Sets {
    let mut s = Syntax::default();
    s.u("profile_idc", 8, 100)
        .u("constraint_set0_flag", 1, 0)
        .u("constraint_set1_flag", 1, 0)
        .u("constraint_set2_flag", 1, 0)
        .u("constraint_set3_flag", 1, 0)
        .u("constraint_set4_flag", 1, 0)
        .u("constraint_set5_flag", 1, 0)
        .u("reserved_zero_2bits", 2, 0)
        .u("level_idc", 8, 30)
        .ue("seq_parameter_set_id", 0)
        .ue("chroma_format_idc", 1)
        .ue("bit_depth_luma_minus8", 0)
        .ue("bit_depth_chroma_minus8", 0)
        .u("qpprime_y_zero_transform_bypass_flag", 1, 0)
        .u("seq_scaling_matrix_present_flag", 1, 1)
        .list("seq_scaling_list_present_flag[0]", Some(&ramp(16, 20)))
        .list("seq_scaling_list_present_flag[1]", None)
        .list("seq_scaling_list_present_flag[2]", Some(&[]))
        .list("seq_scaling_list_present_flag[3]", None)
        .list("seq_scaling_list_present_flag[4]", Some(&ramp(16, 40)))
        .list("seq_scaling_list_present_flag[5]", None)
        .list("seq_scaling_list_present_flag[6]", Some(&ramp(64, 11)))
        .list("seq_scaling_list_present_flag[7]", None)
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
    let sps_trace = std::mem::take(&mut s.trace);

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
        .se("chroma_qp_index_offset", 3)
        .u("deblocking_filter_control_present_flag", 1, 1)
        .u("constrained_intra_pred_flag", 1, 0)
        .u("redundant_pic_cnt_present_flag", 1, 0)
        .u("transform_8x8_mode_flag", 1, 1)
        .u("pic_scaling_matrix_present_flag", 1, 1)
        .list("pic_scaling_list_present_flag[0]", None)
        .list("pic_scaling_list_present_flag[1]", None)
        .list("pic_scaling_list_present_flag[2]", Some(&ramp(16, 60)))
        .list("pic_scaling_list_present_flag[3]", None)
        .list("pic_scaling_list_present_flag[4]", Some(&[]))
        .list("pic_scaling_list_present_flag[5]", None)
        .list("pic_scaling_list_present_flag[6]", None)
        .list("pic_scaling_list_present_flag[7]", Some(&ramp(64, 70)))
        .se("second_chroma_qp_index_offset", -2);
    let pps = p.finish(0x68);
    Sets {
        name: "written: lists falling back".to_owned(),
        sps,
        pps,
        sps_trace,
        pps_trace: p.trace,
    }
}

// ---- the scaling lists as the standard builds them ------------------------

/// One list from the trace's deltas, in zigzag order (7.3.2.1.1.1); `None`
/// when its first delta asks for the default list.
fn read_list(t: &mut Trace<'_>, size: usize) -> Option<Vec<u8>> {
    let (mut last, mut next) = (8i64, 8i64);
    let mut list = Vec::with_capacity(size);
    for j in 0..size {
        if next != 0 {
            let delta = t.next_prefixed("delta_scale");
            next = (last + delta + 256) % 256;
            if j == 0 && next == 0 {
                return None;
            }
        }
        let v = if next == 0 { last } else { next };
        list.push(v as u8);
        last = v;
    }
    Some(list)
}

/// A zigzag-ordered list in raster order.
fn raster(list: &[u8]) -> Vec<u8> {
    let scan: &[u8] = if list.len() == 16 {
        &ZIGZAG_SCAN
    } else {
        &ZIGZAG_DIRECT
    };
    let mut m = vec![0u8; list.len()];
    for (j, &v) in list.iter().enumerate() {
        m[usize::from(scan[j])] = v;
    }
    m
}

/// The twelve lists of a set, raster order, by Table 7-2's fall-back rules:
/// rule A (the defaults) for an SPS, and for a PPS whose SPS sends none;
/// rule B (the SPS's own) for a PPS whose SPS does. `parent` is the SPS's
/// lists for a PPS; `count` how many lists the set may send.
fn expected_lists(
    t: &mut Trace<'_>,
    present_flag: &str,
    list_flag: &str,
    count: usize,
    parent: Option<&[Vec<u8>]>,
    parent_sends: bool,
) -> Vec<Vec<u8>> {
    // Lists 0-5 are 4x4, 6-11 8x8 (6 intra Y, 7 inter Y, 8 intra Cb, 9 inter
    // Cb, 10 intra Cr, 11 inter Cr).
    let default = |i: usize| -> Vec<u8> {
        match i {
            0..=2 => DEFAULT_SCALING4[0].to_vec(),
            3..=5 => DEFAULT_SCALING4[1].to_vec(),
            6 | 8 | 10 => DEFAULT_SCALING8[0].to_vec(),
            _ => DEFAULT_SCALING8[1].to_vec(),
        }
    };
    let flat = |i: usize| vec![16u8; if i < 6 { 16 } else { 64 }];
    if t.get(present_flag) != Some(1) {
        // Nothing sent: an SPS's lists are flat, a PPS's its SPS's.
        return (0..12)
            .map(|i| parent.map_or_else(|| flat(i), |p| p[i].clone()))
            .collect();
    }
    let mut lists: Vec<Vec<u8>> = Vec::new();
    for i in 0..12 {
        let sent = i < count && t.next(&format!("{list_flag}[{i}]")) == 1;
        let list = if sent {
            read_list(t, if i < 6 { 16 } else { 64 }).map_or_else(|| default(i), |l| raster(&l))
        } else {
            match i {
                // The first of each kind: rule A's default, rule B's SPS list.
                0 | 3 | 6 | 7 => match (parent, parent_sends) {
                    (Some(p), true) => p[i].clone(),
                    _ => default(i),
                },
                // The rest: the list before of the same kind.
                1 | 2 | 4 | 5 => lists[i - 1].clone(),
                _ if i < count => lists[i - 2].clone(),
                // Not sent by this set at all: an SPS keeps its flat
                // starting value, a PPS its SPS's (FFmpeg's copies).
                _ => parent.map_or_else(|| flat(i), |p| p[i].clone()),
            }
        };
        lists.push(list);
    }
    lists
}

fn lists_of(m4: &[[u8; 16]; 6], m8: &[[u8; 64]; 6]) -> Vec<Vec<u8>> {
    // Ours are kept FFmpeg's way: 8x8 rows 0 and 3 intra and inter Y, then
    // 1, 4 Cb and 2, 5 Cr -- the standard's 6, 7, 8, 9, 10, 11.
    let mut v: Vec<Vec<u8>> = m4.iter().map(|l| l.to_vec()).collect();
    for row in [0, 3, 1, 4, 2, 5] {
        v.push(m8[row].to_vec());
    }
    v
}

// ---- the tests -------------------------------------------------------------

#[test]
fn each_sequence_parameter_set_reads_as_ffmpeg_traces_it() {
    for f in all_sets() {
        let ps = parse(&f);
        let t = Trace::new(&f.sps_trace);
        let sps = ps.sps_list[t.int("seq_parameter_set_id") as usize]
            .as_ref()
            .unwrap_or_else(|| panic!("{}: the SPS is not kept", f.name));
        let name = &f.name;
        assert_eq!(sps.profile_idc, t.int("profile_idc"), "{name}");
        let flags = (0..6).fold(0, |a, k| {
            a | (t.int(&format!("constraint_set{k}_flag")) << k)
        });
        assert_eq!(sps.constraint_set_flags, flags, "{name}");
        assert_eq!(sps.level_idc, t.int("level_idc"), "{name}");
        let chroma = t.get("chroma_format_idc").map_or(1, |v| v as i32);
        assert_eq!(sps.chroma_format_idc, chroma, "{name}");
        assert_eq!(
            sps.bit_depth_luma,
            8 + t.int("bit_depth_luma_minus8"),
            "{name}"
        );
        assert_eq!(
            sps.bit_depth_chroma,
            8 + t.int("bit_depth_chroma_minus8"),
            "{name}"
        );
        assert_eq!(
            sps.transform_bypass,
            t.int("qpprime_y_zero_transform_bypass_flag")
        );
        assert_eq!(
            sps.log2_max_frame_num,
            4 + t.int("log2_max_frame_num_minus4"),
            "{name}"
        );
        assert_eq!(sps.poc_type, t.int("pic_order_cnt_type"), "{name}");
        if sps.poc_type == 0 {
            assert_eq!(
                sps.log2_max_poc_lsb,
                4 + t.int("log2_max_pic_order_cnt_lsb_minus4")
            );
        }
        if sps.poc_type == 1 {
            assert_eq!(
                sps.delta_pic_order_always_zero_flag,
                t.int("delta_pic_order_always_zero_flag")
            );
            assert_eq!(sps.offset_for_non_ref_pic, t.int("offset_for_non_ref_pic"));
            assert_eq!(
                sps.offset_for_top_to_bottom_field,
                t.int("offset_for_top_to_bottom_field")
            );
            let n = t.int("num_ref_frames_in_pic_order_cnt_cycle");
            assert_eq!(sps.poc_cycle_length, n, "{name}");
            for i in 0..n as usize {
                let o = t.int(&format!("offset_for_ref_frame[{i}]"));
                assert_eq!(sps.offset_for_ref_frame[i], o, "{name}");
            }
        }
        assert_eq!(sps.ref_frame_count, t.int("max_num_ref_frames"), "{name}");
        assert_eq!(
            sps.gaps_in_frame_num_allowed_flag,
            t.int("gaps_in_frame_num_allowed_flag")
        );
        let frame_mbs_only = t.int("frame_mbs_only_flag");
        assert_eq!(sps.frame_mbs_only_flag, frame_mbs_only, "{name}");
        assert_eq!(sps.mb_width, t.int("pic_width_in_mbs_minus1") + 1, "{name}");
        assert_eq!(
            sps.mb_height,
            (t.int("pic_height_in_map_units_minus1") + 1) * (2 - frame_mbs_only),
            "{name}: a frame's height, twice a field's"
        );
        assert_eq!(sps.mb_aff, t.int("mb_adaptive_frame_field_flag"), "{name}");
        assert_eq!(
            sps.direct_8x8_inference_flag,
            t.int("direct_8x8_inference_flag")
        );
        assert_eq!(sps.crop, t.int("frame_cropping_flag"), "{name}");
        // In luma samples: two to a chroma sample across 4:2:0 and 4:2:2,
        // down 4:2:0, and two to a field line.
        let step_x = if chroma == 1 || chroma == 2 { 2 } else { 1 };
        let step_y = (2 - frame_mbs_only) * if chroma == 1 { 2 } else { 1 };
        for (side, got, step) in [
            ("left", sps.crop_left, step_x),
            ("right", sps.crop_right, step_x),
            ("top", sps.crop_top, step_y),
            ("bottom", sps.crop_bottom, step_y),
        ] {
            let want = t.int(&format!("frame_crop_{side}_offset")) * step;
            assert_eq!(got as i32, want, "{name}: crop {side}");
        }

        // The video usability information.
        assert_eq!(
            sps.vui_parameters_present_flag,
            t.int("vui_parameters_present_flag")
        );
        let sar = match t.get("aspect_ratio_idc") {
            Some(255) => (t.int("sar_width"), t.int("sar_height")),
            Some(i) => PIXEL_ASPECT[i as usize],
            None => (0, 1),
        };
        assert_eq!(sps.vui.sar, sar, "{name}");
        assert_eq!(
            sps.vui.video_full_range_flag,
            t.get("video_full_range_flag").map_or(-1, |v| v as i32),
            "{name}: -1 for unsaid"
        );
        if t.get("colour_description_present_flag") == Some(1) {
            assert_eq!(sps.vui.matrix_coeffs as i32, t.int("matrix_coefficients"));
        } else {
            assert_eq!(sps.vui.matrix_coeffs, COLOUR_UNSPECIFIED);
        }
        assert_eq!(
            sps.timing_info_present_flag,
            t.int("timing_info_present_flag")
        );
        assert_eq!(sps.num_units_in_tick as i32, t.int("num_units_in_tick"));
        assert_eq!(sps.time_scale as i32, t.int("time_scale"));
        assert_eq!(sps.fixed_frame_rate_flag, t.int("fixed_frame_rate_flag"));
        assert_eq!(
            sps.pic_struct_present_flag,
            t.int("pic_struct_present_flag")
        );
        assert_eq!(
            sps.bitstream_restriction_flag,
            t.int("bitstream_restriction_flag")
        );
        if sps.bitstream_restriction_flag != 0 {
            assert_eq!(
                sps.num_reorder_frames,
                t.int("max_num_reorder_frames"),
                "{name}"
            );
            assert_eq!(
                sps.max_dec_frame_buffering,
                t.int("max_dec_frame_buffering")
            );
        } else if sps.ref_frame_count != 0 {
            // Bounded by the level's buffer, as FFmpeg derives it.
            let mbs = LEVEL_MAX_DPB_MBS
                .iter()
                .find(|(l, _)| *l == sps.level_idc)
                .map(|&(_, m)| m);
            let want = mbs.map_or(15, |m| (m / (sps.mb_width * sps.mb_height)).min(15));
            assert_eq!(sps.num_reorder_frames, want, "{name}");
        }

        // The scaling lists, against the standard's rules.
        let mut lt = Trace::new(&f.sps_trace);
        let count = if chroma == 3 { 12 } else { 8 };
        let want = expected_lists(
            &mut lt,
            "seq_scaling_matrix_present_flag",
            "seq_scaling_list_present_flag",
            count,
            None,
            false,
        );
        let got = lists_of(&sps.scaling_matrix4, &sps.scaling_matrix8);
        assert_eq!(got, want, "{name}: SPS scaling lists");
        assert_eq!(
            sps.scaling_matrix_present,
            t.int("seq_scaling_matrix_present_flag"),
            "{name}"
        );
    }
}

#[test]
fn each_picture_parameter_set_reads_as_ffmpeg_traces_it() {
    for f in all_sets() {
        let ps = parse(&f);
        let t = Trace::new(&f.pps_trace);
        let name = &f.name;
        let pps = ps.pps_list[t.int("pic_parameter_set_id") as usize]
            .as_ref()
            .unwrap_or_else(|| panic!("{name}: the PPS is not kept"));
        let sps = &pps.sps;
        assert_eq!(pps.sps_id as i32, t.int("seq_parameter_set_id"));
        assert_eq!(
            pps.cabac as i32,
            t.int("entropy_coding_mode_flag"),
            "{name}"
        );
        assert_eq!(
            pps.pic_order_present as i32,
            t.int("bottom_field_pic_order_in_frame_present_flag")
        );
        assert_eq!(pps.slice_group_count, t.int("num_slice_groups_minus1") + 1);
        for (l, field) in [
            (0, "num_ref_idx_l0_default_active_minus1"),
            (1, "num_ref_idx_l1_default_active_minus1"),
        ] {
            assert_eq!(pps.ref_count[l] as i32, t.int(field) + 1, "{name}");
        }
        assert_eq!(pps.weighted_pred as i32, t.int("weighted_pred_flag"));
        assert_eq!(pps.weighted_bipred_idc as i32, t.int("weighted_bipred_idc"));
        let qp_bd_offset = 6 * (sps.bit_depth_luma - 8);
        assert_eq!(
            pps.init_qp,
            t.int("pic_init_qp_minus26") + 26 + qp_bd_offset,
            "{name}"
        );
        assert_eq!(
            pps.init_qs,
            t.int("pic_init_qs_minus26") + 26 + qp_bd_offset,
            "{name}"
        );
        let offset = t.int("chroma_qp_index_offset");
        let second = t
            .get("second_chroma_qp_index_offset")
            .map_or(offset, |v| v as i32);
        assert_eq!(pps.chroma_qp_index_offset, [offset, second], "{name}");
        assert_eq!(pps.chroma_qp_diff, i32::from(offset != second));
        assert_eq!(
            pps.deblocking_filter_parameters_present as i32,
            t.int("deblocking_filter_control_present_flag")
        );
        assert_eq!(
            pps.constrained_intra_pred as i32,
            t.int("constrained_intra_pred_flag")
        );
        assert_eq!(
            pps.redundant_pic_cnt_present as i32,
            t.int("redundant_pic_cnt_present_flag")
        );
        assert_eq!(
            pps.transform_8x8_mode as i32,
            t.int("transform_8x8_mode_flag"),
            "{name}"
        );
        assert_eq!(pps.dequant8_coeff.is_some(), pps.transform_8x8_mode != 0);

        // The chroma QP table: the luma QP plus the offset, through the
        // depth's chroma table.
        let max_qp = 51 + qp_bd_offset;
        for (k, off) in [offset, second].into_iter().enumerate() {
            for q in 0..=max_qp {
                let row = (sps.bit_depth_luma - 8) as usize;
                let want = CHROMA_QP[row][(q + off).clamp(0, max_qp) as usize];
                assert_eq!(pps.chroma_qp_table[k][q as usize], want, "{name}");
            }
        }

        // Its scaling lists, by rule B against its SPS's when the SPS sends
        // its own.
        let mut st = Trace::new(&f.sps_trace);
        let chroma = Trace::new(&f.sps_trace)
            .get("chroma_format_idc")
            .map_or(1, |v| v as usize);
        let sps_count = if chroma == 3 { 12 } else { 8 };
        let sps_lists = expected_lists(
            &mut st,
            "seq_scaling_matrix_present_flag",
            "seq_scaling_list_present_flag",
            sps_count,
            None,
            false,
        );
        let sps_sends = Trace::new(&f.sps_trace).get("seq_scaling_matrix_present_flag") == Some(1);
        let mut pt = Trace::new(&f.pps_trace);
        let count = if pps.transform_8x8_mode == 0 {
            6
        } else if chroma == 3 {
            12
        } else {
            8
        };
        let want = expected_lists(
            &mut pt,
            "pic_scaling_matrix_present_flag",
            "pic_scaling_list_present_flag",
            count,
            Some(&sps_lists),
            sps_sends,
        );
        let got = lists_of(&pps.scaling_matrix4, &pps.scaling_matrix8);
        assert_eq!(got, want, "{name}: PPS scaling lists");
    }
}

/// The dequantization factors are the scaling list times the level scale
/// at QP % 6, shifted by QP / 6 (and 2 more for 4x4), transposed: checked
/// at every QP for every fixture with an 8x8 transform.
#[test]
fn the_dequantization_tables_follow_the_scaling_lists() {
    for f in all_sets() {
        let ps = parse(&f);
        let t = Trace::new(&f.pps_trace);
        let pps = ps.pps_list[t.int("pic_parameter_set_id") as usize]
            .as_ref()
            .unwrap();
        let max_qp = (51 + 6 * (pps.sps.bit_depth_luma - 8)) as usize;
        for i in 0..6 {
            for q in 0..=max_qp {
                for y in 0..4 {
                    for x in 0..4 {
                        // The position's class: both coordinates even, both
                        // odd, or one of each.
                        let class = match (x % 2, y % 2) {
                            (0, 0) => 0,
                            (1, 1) => 2,
                            _ => 1,
                        };
                        let level = u32::from(DEQUANT4_COEFF_INIT[q % 6][class]);
                        let want =
                            (level * u32::from(pps.scaling_matrix4[i][y * 4 + x])) << (q / 6 + 2);
                        assert_eq!(pps.dequant4(i, q).unwrap()[x * 4 + y], want, "{}", f.name);
                    }
                }
            }
        }
    }
}

#[test]
fn a_set_the_same_as_the_kept_one_leaves_it_in_place() {
    let f = &fixtures::BASELINE;
    let mut ps = ParamSets::default();
    read_unit(f.sps, |gb, _| decode_sps(gb, &mut ps, false).unwrap());
    let first = Arc::clone(ps.sps_list[0].as_ref().unwrap());
    read_unit(f.sps, |gb, _| decode_sps(gb, &mut ps, false).unwrap());
    assert!(
        Arc::ptr_eq(&first, ps.sps_list[0].as_ref().unwrap()),
        "the same set: kept"
    );
    // Another one -- one bit of level_idc different -- replaces it.
    let mut other = f.sps.to_vec();
    other[3] ^= 1;
    read_unit(&other, |gb, _| decode_sps(gb, &mut ps, false).unwrap());
    assert!(!Arc::ptr_eq(&first, ps.sps_list[0].as_ref().unwrap()));
}

#[test]
fn a_set_ffmpeg_refuses_is_refused() {
    let f = &fixtures::BASELINE;
    let mut ps = ParamSets::default();
    // A PPS before any SPS names a set that is not there.
    read_unit(f.pps, |gb, nal| {
        assert_eq!(
            decode_pps(gb, &mut ps, nal.size_bits).unwrap_err(),
            PsError::Invalid
        );
    });
    // An SPS without its last byte -- three flags and the stop bit -- whose
    // last bit kept then reads as the stop bit: refused, unless truncation
    // is to be ignored, when the flags past the end read as zeros.
    let cut = &f.sps[..f.sps.len() - 1];
    read_unit(cut, |gb, _| {
        assert_eq!(
            decode_sps(&mut gb.clone(), &mut ps, false).unwrap_err(),
            PsError::Invalid
        );
        decode_sps(gb, &mut ps, true).unwrap();
    });
}
