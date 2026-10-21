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
use crate::ps::{decode_pps, decode_sps};
use crate::writer::{Syntax, read_unit};
use fixtures::ALL;

/// The first slice of a picture: no `frame_num` to repeat.
const FIRST: PictureState = PictureState {
    first_slice: true,
    frame_num: 0,
};

/// One header's trace, taken element by element in the order FFmpeg's
/// `trace_headers` printed it.
struct Cursor<'t> {
    fields: &'t [(&'t str, i64)],
    at: usize,
}

impl<'t> Cursor<'t> {
    fn new(fields: &'t [(&'t str, i64)]) -> Self {
        Self { fields, at: 0 }
    }

    /// The next element, which must be `name`.
    fn take(&mut self, name: &str) -> i64 {
        let &(n, v) = self
            .fields
            .get(self.at)
            .unwrap_or_else(|| panic!("the trace ends before {name}"));
        assert_eq!(n, name, "the trace's next element");
        self.at += 1;
        v
    }

    /// The next element if it is `name`.
    fn take_if(&mut self, name: &str) -> Option<i64> {
        match self.fields.get(self.at) {
            Some(&(n, v)) if n == name => {
                self.at += 1;
                Some(v)
            }
            _ => None,
        }
    }

    fn rest(&self) -> &[(&'t str, i64)] {
        &self.fields[self.at..]
    }
}

/// Check what `sl` read against the trace of the same header.
fn check(sl: &SliceHeader, t: &mut Cursor<'_>, nal: &Nal, ps: &ParamSets, at: &str) {
    t.take("forbidden_zero_bit");
    assert_eq!(t.take("nal_ref_idc"), i64::from(nal.ref_idc), "{at}");
    assert_eq!(t.take("nal_unit_type"), i64::from(nal.kind), "{at}");
    assert_eq!(
        t.take("first_mb_in_slice"),
        i64::from(sl.first_mb_addr),
        "{at}"
    );
    let raw = t.take("slice_type");
    assert_eq!(
        sl.slice_type,
        GOLOMB_TO_PICT_TYPE[(raw % 5) as usize],
        "{at}"
    );
    assert_eq!(sl.slice_type_nos, sl.slice_type & 3, "{at}");
    assert_eq!(sl.slice_type_fixed, i32::from(raw > 4), "{at}");
    assert_eq!(t.take("pic_parameter_set_id"), i64::from(sl.pps_id), "{at}");
    let pps = ps.pps_list[sl.pps_id as usize].as_deref().unwrap();
    let sps = &*pps.sps;
    assert_eq!(t.take("frame_num"), i64::from(sl.frame_num), "{at}");

    let structure = match t.take_if("field_pic_flag") {
        Some(1) => PICT_TOP_FIELD + t.take("bottom_field_flag") as i32,
        _ => PICT_FRAME,
    };
    assert_eq!(sl.picture_structure, structure, "{at}");
    assert_eq!(
        sl.mb_field_decoding_flag,
        i32::from(structure != PICT_FRAME),
        "{at}"
    );
    assert_eq!(sl.mb_mbaff, 0, "{at}");
    let (curr, max) = if structure == PICT_FRAME {
        (sl.frame_num, 1 << sps.log2_max_frame_num)
    } else {
        (2 * sl.frame_num + 1, 1 << (sps.log2_max_frame_num + 1))
    };
    assert_eq!((sl.curr_pic_num, sl.max_pic_num), (curr, max), "{at}");

    if let Some(id) = t.take_if("idr_pic_id") {
        assert_eq!(i64::from(sl.idr_pic_id), id, "{at}");
    }
    let poc = [
        t.take_if("pic_order_cnt_lsb").unwrap_or(0),
        t.take_if("delta_pic_order_cnt_bottom").unwrap_or(0),
        t.take_if("delta_pic_order_cnt[0]").unwrap_or(0),
        t.take_if("delta_pic_order_cnt[1]").unwrap_or(0),
    ];
    assert_eq!(
        poc,
        [
            sl.poc_lsb,
            sl.delta_poc_bottom,
            sl.delta_poc[0],
            sl.delta_poc[1]
        ]
        .map(i64::from),
        "{at}: POC"
    );
    assert_eq!(
        t.take_if("redundant_pic_cnt").unwrap_or(0),
        i64::from(sl.redundant_pic_count),
        "{at}"
    );
    if let Some(direct) = t.take_if("direct_spatial_mv_pred_flag") {
        assert_eq!(i64::from(sl.direct_spatial_mv_pred), direct, "{at}");
    }

    let lists = match sl.slice_type_nos {
        TYPE_I => 0,
        TYPE_B => 2,
        _ => 1,
    };
    assert_eq!(sl.list_count, lists, "{at}");
    if lists == 0 {
        assert_eq!(sl.ref_count, [0, 0], "{at}");
    } else {
        let mut count = pps.ref_count;
        if t.take("num_ref_idx_active_override_flag") == 1 {
            count[0] = t.take("num_ref_idx_l0_active_minus1") as u32 + 1;
            if lists == 2 {
                count[1] = t.take("num_ref_idx_l1_active_minus1") as u32 + 1;
            }
        }
        assert_eq!(sl.ref_count[0], count[0], "{at}");
        if lists == 2 {
            assert_eq!(sl.ref_count[1], count[1], "{at}");
        }
    }

    for list in 0..lists as usize {
        let mut n = 0;
        if t.take(&format!("ref_pic_list_modification_flag_l{list}")) == 1 {
            loop {
                let op = t.take("modification_of_pic_nums_idc");
                if op == 3 {
                    break;
                }
                let val = t.take(if op == 2 {
                    "long_term_pic_num"
                } else {
                    "abs_diff_pic_num_minus1"
                });
                assert_eq!(
                    sl.ref_modifications[list][n],
                    RefModification {
                        op: op as u8,
                        val: val as u32
                    },
                    "{at}: list {list} modification {n}"
                );
                n += 1;
            }
        }
        assert_eq!(sl.nb_ref_modifications[list], n, "{at}");
    }

    if (pps.weighted_pred != 0 && sl.slice_type_nos == TYPE_P)
        || (pps.weighted_bipred_idc == 1 && sl.slice_type_nos == TYPE_B)
    {
        check_weights(sl, t, sps, at);
    } else {
        assert_eq!(sl.pwt.use_weight, 0, "{at}");
        assert_eq!(sl.pwt.luma_weight_flag, [0, 0], "{at}");
        assert_eq!(sl.pwt.chroma_weight_flag, [0, 0], "{at}");
    }

    if nal.ref_idc == 0 {
        assert_eq!(sl.explicit_ref_marking, 0, "{at}");
    } else {
        check_marking(sl, t, nal, at);
    }

    if let Some(idc) = t.take_if("cabac_init_idc") {
        assert_eq!(i64::from(sl.cabac_init_idc), idc, "{at}");
    }
    let qp = i64::from(pps.init_qp) + t.take("slice_qp_delta");
    assert_eq!(i64::from(sl.qscale), qp, "{at}");
    assert_eq!(
        sl.chroma_qp,
        [0, 1].map(|k| i32::from(pps.chroma_qp_table[k][qp as usize])),
        "{at}"
    );
    t.take_if("sp_for_switch_flag");
    t.take_if("slice_qs_delta");

    match t.take_if("disable_deblocking_filter_idc") {
        None => assert_eq!(sl.deblocking_filter, 1, "{at}"),
        Some(idc) => {
            let filter = if idc < 2 { idc ^ 1 } else { idc };
            assert_eq!(i64::from(sl.deblocking_filter), filter, "{at}");
        }
    }
    let alpha = t.take_if("slice_alpha_c0_offset_div2").unwrap_or(0);
    let beta = t.take_if("slice_beta_offset_div2").unwrap_or(0);
    assert_eq!(
        [sl.slice_alpha_c0_offset, sl.slice_beta_offset].map(i64::from),
        [alpha * 2, beta * 2],
        "{at}"
    );
    assert!(t.rest().is_empty(), "{at}: not read: {:?}", t.rest());
}

fn check_weights(sl: &SliceHeader, t: &mut Cursor<'_>, sps: &Sps, at: &str) {
    let pwt = &sl.pwt;
    let denom = t.take("luma_log2_weight_denom");
    assert_eq!(i64::from(pwt.luma_log2_weight_denom), denom, "{at}");
    let luma_def = [1 << denom, 0];
    let chroma = sps.chroma_format_idc != 0;
    let chroma_def = if chroma {
        let denom = t.take("chroma_log2_weight_denom");
        assert_eq!(i64::from(pwt.chroma_log2_weight_denom), denom, "{at}");
        [1 << denom, 0]
    } else {
        [0, 0]
    };
    let lists = if sl.slice_type_nos == TYPE_B { 2 } else { 1 };
    let (mut any_luma, mut any_chroma) = (false, false);
    for list in 0..lists {
        let (mut luma_flag, mut chroma_flag) = (false, false);
        for i in 0..sl.ref_count[list] as usize {
            let luma = if t.take(&format!("luma_weight_l{list}_flag[{i}]")) == 1 {
                [
                    t.take(&format!("luma_weight_l{list}[{i}]")),
                    t.take(&format!("luma_offset_l{list}[{i}]")),
                ]
            } else {
                luma_def
            };
            assert_eq!(pwt.luma_weight[i][list].map(i64::from), luma, "{at}");
            luma_flag |= luma != luma_def;
            if chroma {
                let sent = t.take(&format!("chroma_weight_l{list}_flag[{i}]")) == 1;
                for j in 0..2 {
                    let c = if sent {
                        [
                            t.take(&format!("chroma_weight_l{list}[{i}][{j}]")),
                            t.take(&format!("chroma_offset_l{list}[{i}][{j}]")),
                        ]
                    } else {
                        chroma_def
                    };
                    assert_eq!(pwt.chroma_weight[i][list][j].map(i64::from), c, "{at}");
                    chroma_flag |= c != chroma_def;
                }
            }
            if sl.picture_structure == PICT_FRAME {
                for f in [16 + 2 * i, 16 + 2 * i + 1] {
                    assert_eq!(pwt.luma_weight[f][list], pwt.luma_weight[i][list], "{at}");
                    if chroma {
                        assert_eq!(
                            pwt.chroma_weight[f][list], pwt.chroma_weight[i][list],
                            "{at}"
                        );
                    }
                }
            }
        }
        assert_eq!(pwt.luma_weight_flag[list], i32::from(luma_flag), "{at}");
        assert_eq!(pwt.chroma_weight_flag[list], i32::from(chroma_flag), "{at}");
        any_luma |= luma_flag;
        any_chroma |= chroma_flag;
    }
    assert_eq!(pwt.use_weight_chroma, i32::from(any_chroma), "{at}");
    assert_eq!(pwt.use_weight, i32::from(any_luma || any_chroma), "{at}");
}

fn check_marking(sl: &SliceHeader, t: &mut Cursor<'_>, nal: &Nal, at: &str) {
    if nal.kind == NAL_IDR_SLICE {
        t.take("no_output_of_prior_pics_flag");
        let long = t.take("long_term_reference_flag");
        assert_eq!(sl.explicit_ref_marking, 1, "{at}");
        if long == 1 {
            assert_eq!(sl.nb_mmco, 1, "{at}");
            assert_eq!(
                (sl.mmco[0].opcode, sl.mmco[0].long_arg),
                (MMCO_LONG, 0),
                "{at}"
            );
        } else {
            assert_eq!(sl.nb_mmco, 0, "{at}");
        }
        return;
    }
    let adaptive = t.take("adaptive_ref_pic_marking_mode_flag");
    assert_eq!(i64::from(sl.explicit_ref_marking), adaptive, "{at}");
    let mut n = 0;
    if adaptive == 1 {
        loop {
            let op = t.take("memory_management_control_operation");
            let mmco = sl.mmco[n];
            assert_eq!(i64::from(mmco.opcode), op, "{at}: operation {n}");
            if op == 0 {
                break;
            }
            if op == 1 || op == 3 {
                let diff = t.take("difference_of_pic_nums_minus1");
                let num = (i64::from(sl.curr_pic_num) - diff - 1) & (i64::from(sl.max_pic_num) - 1);
                assert_eq!(i64::from(mmco.short_pic_num), num, "{at}: operation {n}");
            }
            let arg = match op {
                2 => Some(t.take("long_term_pic_num")),
                3 | 6 => Some(t.take("long_term_frame_idx")),
                4 => Some(t.take("max_long_term_frame_idx_plus1")),
                _ => None,
            };
            if let Some(arg) = arg {
                assert_eq!(i64::from(mmco.long_arg), arg, "{at}: operation {n}");
            }
            n += 1;
        }
    }
    assert_eq!(sl.nb_mmco, n, "{at}");
}

#[test]
fn each_slice_header_reads_as_ffmpeg_traces_it() {
    let mut slices = 0;
    for f in ALL {
        let mut ps = ParamSets::default();
        let mut sl = SliceHeader::default();
        for (k, u) in f.units.iter().enumerate() {
            match u.bytes[0] & 0x1F {
                7 => read_unit(u.bytes, |gb, _| decode_sps(gb, &mut ps, false).unwrap()),
                8 => read_unit(u.bytes, |gb, nal| {
                    decode_pps(gb, &mut ps, nal.size_bits).unwrap();
                }),
                _ => {
                    let at = format!("{}, unit {k}", f.name);
                    read_unit(u.bytes, |gb, nal| {
                        sl.parse(gb, nal, &ps, FIRST)
                            .unwrap_or_else(|e| panic!("{at}: {e:?}"));
                        assert_eq!(gb.count(), u.end, "{at}: where the header ends");
                        check(&sl, &mut Cursor::new(u.trace), nal, &ps, &at);
                    });
                    slices += 1;
                }
            }
        }
    }
    assert!(slices >= 50, "{slices} slices");
}

// ---- headers written here -------------------------------------------------

/// What a written sequence and picture parameter set say.
#[derive(Clone, Copy)]
struct Opts {
    /// 77 Main, or 100 High for what only High may say.
    profile: i64,
    chroma_format_idc: i64,
    bit_depth: i64,
    log2_max_frame_num: i64,
    poc_type: i64,
    delta_pic_order_always_zero: i64,
    frame_mbs_only: i64,
    mb_aff: i64,
    direct_8x8: i64,
    cabac: i64,
    pic_order_present: i64,
    ref_count: [i64; 2],
    weighted_pred: i64,
    weighted_bipred_idc: i64,
    init_qp_minus26: i64,
    deblocking_params: i64,
    redundant: i64,
}

const MAIN: Opts = Opts {
    profile: 77,
    chroma_format_idc: 1,
    bit_depth: 8,
    log2_max_frame_num: 4,
    poc_type: 2,
    delta_pic_order_always_zero: 0,
    frame_mbs_only: 1,
    mb_aff: 0,
    direct_8x8: 1,
    cabac: 0,
    pic_order_present: 0,
    ref_count: [1, 1],
    weighted_pred: 0,
    weighted_bipred_idc: 0,
    init_qp_minus26: 0,
    deblocking_params: 0,
    redundant: 0,
};

/// The parameter sets `o` describes, read.
fn sets(o: Opts) -> ParamSets {
    let mut s = Syntax::default();
    s.u("profile_idc", 8, o.profile)
        .u("constraint_set_flags", 6, 0)
        .u("reserved_zero_2bits", 2, 0)
        .u("level_idc", 8, 40)
        .ue("seq_parameter_set_id", 0);
    if o.profile == 100 {
        s.ue("chroma_format_idc", o.chroma_format_idc);
        if o.chroma_format_idc == 3 {
            s.u("separate_colour_plane_flag", 1, 0);
        }
        s.ue("bit_depth_luma_minus8", o.bit_depth - 8)
            .ue("bit_depth_chroma_minus8", o.bit_depth - 8)
            .u("qpprime_y_zero_transform_bypass_flag", 1, 0)
            .u("seq_scaling_matrix_present_flag", 1, 0);
    }
    s.ue("log2_max_frame_num_minus4", o.log2_max_frame_num - 4)
        .ue("pic_order_cnt_type", o.poc_type);
    if o.poc_type == 0 {
        s.ue("log2_max_pic_order_cnt_lsb_minus4", 2);
    } else if o.poc_type == 1 {
        s.u(
            "delta_pic_order_always_zero_flag",
            1,
            o.delta_pic_order_always_zero,
        )
        .se("offset_for_non_ref_pic", 0)
        .se("offset_for_top_to_bottom_field", 0)
        .ue("num_ref_frames_in_pic_order_cnt_cycle", 1)
        .se("offset_for_ref_frame[0]", 2);
    }
    s.ue("max_num_ref_frames", 4)
        .u("gaps_in_frame_num_value_allowed_flag", 1, 0)
        .ue("pic_width_in_mbs_minus1", 10)
        .ue("pic_height_in_map_units_minus1", 8)
        .u("frame_mbs_only_flag", 1, o.frame_mbs_only);
    if o.frame_mbs_only == 0 {
        s.u("mb_adaptive_frame_field_flag", 1, o.mb_aff);
    }
    s.u("direct_8x8_inference_flag", 1, o.direct_8x8)
        .u("frame_cropping_flag", 1, 0)
        .u("vui_parameters_present_flag", 1, 0);
    let sps = s.finish(0x67);

    let mut p = Syntax::default();
    p.ue("pic_parameter_set_id", 0)
        .ue("seq_parameter_set_id", 0)
        .u("entropy_coding_mode_flag", 1, o.cabac)
        .u(
            "bottom_field_pic_order_in_frame_present_flag",
            1,
            o.pic_order_present,
        )
        .ue("num_slice_groups_minus1", 0)
        .ue("num_ref_idx_l0_default_active_minus1", o.ref_count[0] - 1)
        .ue("num_ref_idx_l1_default_active_minus1", o.ref_count[1] - 1)
        .u("weighted_pred_flag", 1, o.weighted_pred)
        .u("weighted_bipred_idc", 2, o.weighted_bipred_idc)
        .se("pic_init_qp_minus26", o.init_qp_minus26)
        .se("pic_init_qs_minus26", 0)
        .se("chroma_qp_index_offset", 0)
        .u(
            "deblocking_filter_control_present_flag",
            1,
            o.deblocking_params,
        )
        .u("constrained_intra_pred_flag", 1, 0)
        .u("redundant_pic_cnt_present_flag", 1, o.redundant);
    let pps = p.finish(0x68);

    let mut ps = ParamSets::default();
    read_unit(&sps, |gb, _| decode_sps(gb, &mut ps, false).unwrap());
    read_unit(&pps, |gb, nal| {
        decode_pps(gb, &mut ps, nal.size_bits).unwrap();
    });
    ps
}

/// A header's opening: `first_mb_in_slice` 0, the type as written, PPS 0
/// and `frame_num` in `bits` bits.
fn opening(slice_type: i64, frame_num: i64, bits: u32) -> Syntax {
    let mut s = Syntax::default();
    s.ue("first_mb_in_slice", 0)
        .ue("slice_type", slice_type)
        .ue("pic_parameter_set_id", 0)
        .u("frame_num", bits, frame_num);
    s
}

/// A written header's unit: slice data after it, a NAL header in front.
/// Also where the header ends.
fn unit(s: &mut Syntax, ref_idc: u8, kind: u8) -> (Vec<u8>, usize) {
    let end = s.position();
    s.u("slice_data", 8, 0xA5);
    (s.finish((ref_idc << 5) | kind), end)
}

/// Read a written header into `sl`: the result, and whether the reading
/// stopped where the header ends.
fn parse(
    sl: &mut SliceHeader,
    ps: &ParamSets,
    picture: PictureState,
    (unit, end): &(Vec<u8>, usize),
) -> (Result<(), SliceError>, bool) {
    read_unit(unit, |gb, nal| {
        let r = sl.parse(gb, nal, ps, picture);
        (r, gb.count() as usize == *end)
    })
}

/// Read a written header that must read to its end.
fn parse_ok(sl: &mut SliceHeader, ps: &ParamSets, u: &(Vec<u8>, usize)) {
    let (r, at_end) = parse(sl, ps, FIRST, u);
    assert_eq!(r, Ok(()));
    assert!(at_end, "the header read to its end");
}

/// A P slice of `MAIN` with its marking and QP written, and `between`
/// written between `frame_num` and the reference count.
fn p_slice(between: impl FnOnce(&mut Syntax)) -> (Vec<u8>, usize) {
    let mut s = opening(5, 1, 4);
    between(&mut s);
    s.u("num_ref_idx_active_override_flag", 1, 0)
        .u("ref_pic_list_modification_flag_l0", 1, 0)
        .u("adaptive_ref_pic_marking_mode_flag", 1, 0)
        .se("slice_qp_delta", 0);
    unit(&mut s, 2, 1)
}

/// A whole IDR header of `slice_type` (as written) naming PPS `pps_id`,
/// so that nothing refuses it but what a test means to.
fn idr(slice_type: i64, pps_id: i64) -> (Vec<u8>, usize) {
    let mut s = Syntax::default();
    s.ue("first_mb_in_slice", 0)
        .ue("slice_type", slice_type)
        .ue("pic_parameter_set_id", pps_id)
        .u("frame_num", 4, 0)
        .ue("idr_pic_id", 0);
    if slice_type % 5 == 0 {
        s.u("num_ref_idx_active_override_flag", 1, 0)
            .u("ref_pic_list_modification_flag_l0", 1, 0);
    }
    s.u("no_output_of_prior_pics_flag", 1, 0)
        .u("long_term_reference_flag", 1, 0)
        .se("slice_qp_delta", 0);
    unit(&mut s, 3, 5)
}

#[test]
fn a_header_ffmpeg_refuses_early_is_refused() {
    let ps = sets(MAIN);
    let mut sl = SliceHeader::default();
    parse_ok(&mut sl, &ps, &idr(7, 0));
    // slice_type past 9.
    assert_eq!(parse(&mut sl, &ps, FIRST, &idr(10, 0)).0, Err(SliceError));
    // A P slice in an IDR's unit -- which would read, were it not one.
    assert_eq!(parse(&mut sl, &ps, FIRST, &idr(0, 0)).0, Err(SliceError));
    let mut s = opening(0, 0, 4);
    s.u("num_ref_idx_active_override_flag", 1, 0)
        .u("ref_pic_list_modification_flag_l0", 1, 0)
        .u("adaptive_ref_pic_marking_mode_flag", 1, 0)
        .se("slice_qp_delta", 0);
    parse_ok(&mut sl, &ps, &unit(&mut s, 3, 1));
    // A PPS that was never sent, and one past the 256 there may be.
    for id in [1, 256] {
        assert_eq!(parse(&mut sl, &ps, FIRST, &idr(7, id)).0, Err(SliceError));
    }
    // A later slice of the picture whose frame_num is not the picture's.
    let later = |frame_num| PictureState {
        first_slice: false,
        frame_num,
    };
    let u = p_slice(|_| {});
    assert_eq!(parse(&mut sl, &ps, later(2), &u).0, Err(SliceError));
    let (r, at_end) = parse(&mut sl, &ps, later(1), &u);
    assert_eq!(r, Ok(()));
    assert!(at_end);
}

#[test]
fn a_b_slice_needs_8x8_inference_where_there_may_be_fields() {
    let b_frame = || {
        let mut s = opening(6, 1, 4);
        s.u("field_pic_flag", 1, 0)
            .u("direct_spatial_mv_pred_flag", 1, 1)
            .u("num_ref_idx_active_override_flag", 1, 0)
            .u("ref_pic_list_modification_flag_l0", 1, 0)
            .u("ref_pic_list_modification_flag_l1", 1, 0)
            .se("slice_qp_delta", 0);
        unit(&mut s, 0, 1)
    };
    let fields = Opts {
        frame_mbs_only: 0,
        ..MAIN
    };
    let mut sl = SliceHeader::default();
    parse_ok(&mut sl, &sets(fields), &b_frame());
    assert_eq!(sl.direct_spatial_mv_pred, 1);
    let ps = sets(Opts {
        direct_8x8: 0,
        ..fields
    });
    assert_eq!(parse(&mut sl, &ps, FIRST, &b_frame()).0, Err(SliceError));
    // A P slice there reads.
    let u = p_slice(|s| {
        s.u("field_pic_flag", 1, 0);
    });
    parse_ok(&mut sl, &ps, &u);
    // So does the B slice where every picture is a frame.
    let ps = sets(Opts {
        direct_8x8: 0,
        ..MAIN
    });
    let mut s = opening(6, 1, 4);
    s.u("direct_spatial_mv_pred_flag", 1, 0)
        .u("num_ref_idx_active_override_flag", 1, 0)
        .u("ref_pic_list_modification_flag_l0", 1, 0)
        .u("ref_pic_list_modification_flag_l1", 1, 0)
        .se("slice_qp_delta", 0);
    parse_ok(&mut sl, &ps, &unit(&mut s, 0, 1));
    assert_eq!(sl.direct_spatial_mv_pred, 0);
}

#[test]
fn fields_number_their_pictures_twice_as_finely() {
    let ps = sets(Opts {
        frame_mbs_only: 0,
        mb_aff: 1,
        ..MAIN
    });
    let mut sl = SliceHeader::default();
    for (field, bottom, structure) in [
        (0, 0, PICT_FRAME),
        (1, 0, PICT_TOP_FIELD),
        (1, 1, PICT_BOTTOM_FIELD),
    ] {
        let u = p_slice(|s| {
            s.u("field_pic_flag", 1, field);
            if field == 1 {
                s.u("bottom_field_flag", 1, bottom);
            }
        });
        parse_ok(&mut sl, &ps, &u);
        assert_eq!(sl.picture_structure, structure);
        assert_eq!(sl.mb_field_decoding_flag, i32::from(field == 1));
        let expect = if field == 1 { (3, 32) } else { (1, 16) };
        assert_eq!((sl.curr_pic_num, sl.max_pic_num), expect);
    }
}

#[test]
fn an_idr_pic_id_past_65535_and_a_non_idr_leave_the_last_one() {
    let ps = sets(MAIN);
    let mut sl = SliceHeader::default();
    let idr = |id| {
        let mut s = opening(7, 0, 4);
        s.ue("idr_pic_id", id)
            .u("no_output_of_prior_pics_flag", 1, 0)
            .u("long_term_reference_flag", 1, 0)
            .se("slice_qp_delta", 0);
        unit(&mut s, 3, 5)
    };
    parse_ok(&mut sl, &ps, &idr(65535));
    assert_eq!(sl.idr_pic_id, 65535);
    parse_ok(&mut sl, &ps, &idr(65536));
    assert_eq!(sl.idr_pic_id, 65535);
    parse_ok(&mut sl, &ps, &p_slice(|_| {}));
    assert_eq!(sl.idr_pic_id, 65535);
    parse_ok(&mut sl, &ps, &idr(7));
    assert_eq!(sl.idr_pic_id, 7);
}

#[test]
fn picture_order_count_syntax_is_read_where_the_sets_say() {
    let mut sl = SliceHeader::default();
    // Type 0, a bottom field order sent: an LSB and, for a frame, a delta.
    let ps = sets(Opts {
        poc_type: 0,
        pic_order_present: 1,
        frame_mbs_only: 0,
        ..MAIN
    });
    let u = p_slice(|s| {
        s.u("field_pic_flag", 1, 0)
            .u("pic_order_cnt_lsb", 6, 37)
            .se("delta_pic_order_cnt_bottom", -3);
    });
    parse_ok(&mut sl, &ps, &u);
    assert_eq!((sl.poc_lsb, sl.delta_poc_bottom), (37, -3));
    // ...but not for a field, and a header without them clears them.
    let u = p_slice(|s| {
        s.u("field_pic_flag", 1, 1)
            .u("bottom_field_flag", 1, 0)
            .u("pic_order_cnt_lsb", 6, 5);
    });
    parse_ok(&mut sl, &ps, &u);
    assert_eq!((sl.poc_lsb, sl.delta_poc_bottom), (5, 0));
    parse_ok(&mut sl, &sets(MAIN), &p_slice(|_| {}));
    assert_eq!((sl.poc_lsb, sl.delta_poc_bottom), (0, 0));

    // Type 1: two deltas, the second only with a bottom field order sent.
    let o = Opts {
        poc_type: 1,
        ..MAIN
    };
    let u = p_slice(|s| {
        s.se("delta_pic_order_cnt[0]", 4);
    });
    parse_ok(&mut sl, &sets(o), &u);
    assert_eq!(sl.delta_poc, [4, 0]);
    let u = p_slice(|s| {
        s.se("delta_pic_order_cnt[0]", -1)
            .se("delta_pic_order_cnt[1]", 9);
    });
    let ps = sets(Opts {
        pic_order_present: 1,
        ..o
    });
    parse_ok(&mut sl, &ps, &u);
    assert_eq!(sl.delta_poc, [-1, 9]);
    // None at all when they are always zero.
    let ps = sets(Opts {
        delta_pic_order_always_zero: 1,
        pic_order_present: 1,
        ..o
    });
    parse_ok(&mut sl, &ps, &p_slice(|_| {}));
    assert_eq!(sl.delta_poc, [0, 0]);

    // A redundant picture's count, where the PPS says there are any.
    let ps = sets(Opts {
        redundant: 1,
        ..MAIN
    });
    parse_ok(
        &mut sl,
        &ps,
        &p_slice(|s| {
            s.ue("redundant_pic_cnt", 2);
        }),
    );
    assert_eq!(sl.redundant_pic_count, 2);
    parse_ok(&mut sl, &sets(MAIN), &p_slice(|_| {}));
    assert_eq!(sl.redundant_pic_count, 0);
}

/// A slice of type `kind` (as written) with its counts written by `counts`:
/// no modifications, marking or weights. `structure` is what follows
/// `frame_num`: nothing where every picture is a frame, `[0]` for a frame
/// where there may be fields, `[1, bottom]` for a field.
fn counted(
    ps: &ParamSets,
    kind: i64,
    structure: &[i64],
    counts: impl FnOnce(&mut Syntax),
) -> SliceHeader {
    let mut sl = SliceHeader::default();
    let b = kind % 5 == 1;
    let mut s = opening(kind, 1, 4);
    if let Some(&field) = structure.first() {
        s.u("field_pic_flag", 1, field);
    }
    if let Some(&bottom) = structure.get(1) {
        s.u("bottom_field_flag", 1, bottom);
    }
    if b {
        s.u("direct_spatial_mv_pred_flag", 1, 0);
    }
    counts(&mut s);
    s.u("ref_pic_list_modification_flag_l0", 1, 0);
    if b {
        s.u("ref_pic_list_modification_flag_l1", 1, 0);
    }
    s.se("slice_qp_delta", 0);
    parse_ok(&mut sl, ps, &unit(&mut s, 0, 1));
    sl
}

#[test]
fn reference_counts_are_the_sets_unless_the_slice_says() {
    let ps = sets(Opts {
        ref_count: [3, 2],
        ..MAIN
    });
    let none = |s: &mut Syntax| {
        s.u("num_ref_idx_active_override_flag", 1, 0);
    };
    let sl = counted(&ps, 5, &[], none);
    assert_eq!((sl.list_count, sl.ref_count), (1, [3, 2]));
    let sl = counted(&ps, 6, &[], none);
    assert_eq!((sl.list_count, sl.ref_count), (2, [3, 2]));
    // A P slice overriding says list 0's count, and list 1's becomes 1.
    let sl = counted(&ps, 5, &[], |s| {
        s.u("num_ref_idx_active_override_flag", 1, 1)
            .ue("num_ref_idx_l0_active_minus1", 15);
    });
    assert_eq!(sl.ref_count, [16, 1]);
    let sl = counted(&ps, 6, &[], |s| {
        s.u("num_ref_idx_active_override_flag", 1, 1)
            .ue("num_ref_idx_l0_active_minus1", 0)
            .ue("num_ref_idx_l1_active_minus1", 15);
    });
    assert_eq!(sl.ref_count, [1, 16]);
    // An intra slice has neither list.
    let mut sl = SliceHeader::default();
    let mut s = opening(7, 1, 4);
    s.se("slice_qp_delta", 0);
    parse_ok(&mut sl, &ps, &unit(&mut s, 0, 1));
    assert_eq!((sl.list_count, sl.ref_count), (0, [0, 0]));

    // Past 16 for a frame refuses the slice, and leaves no references.
    for (kind, l0, l1) in [(5, 16, 0), (6, 0, 16)] {
        let mut sl = SliceHeader {
            ref_count: [5, 5],
            list_count: 2,
            ..SliceHeader::default()
        };
        let mut s = opening(kind, 1, 4);
        if kind == 6 {
            s.u("direct_spatial_mv_pred_flag", 1, 0);
        }
        s.u("num_ref_idx_active_override_flag", 1, 1)
            .ue("num_ref_idx_l0_active_minus1", l0);
        if kind == 6 {
            s.ue("num_ref_idx_l1_active_minus1", l1);
        }
        let u = unit(&mut s, 0, 1);
        assert_eq!(parse(&mut sl, &ps, FIRST, &u).0, Err(SliceError));
        assert_eq!((sl.list_count, sl.ref_count), (0, [0, 0]));
    }

    // A P slice whose PPS gives list 1 more than a frame may have keeps
    // none in it; a field may have 32.
    let o = Opts {
        ref_count: [1, 20],
        frame_mbs_only: 0,
        ..MAIN
    };
    let ps = sets(o);
    let sl = counted(&ps, 5, &[0], none);
    assert_eq!(sl.ref_count, [1, 0]);
    let sl = counted(&ps, 5, &[1, 0], none);
    assert_eq!(sl.ref_count, [1, 20]);
    let sl = counted(&ps, 6, &[1, 0], |s| {
        s.u("num_ref_idx_active_override_flag", 1, 1)
            .ue("num_ref_idx_l0_active_minus1", 31)
            .ue("num_ref_idx_l1_active_minus1", 31);
    });
    assert_eq!(sl.ref_count, [32, 32]);
}

#[test]
fn list_modifications_are_kept_as_they_come_and_refused_past_the_count() {
    let ps = sets(Opts {
        ref_count: [2, 1],
        ..MAIN
    });
    let mut sl = SliceHeader::default();
    let mods = |list: &[(i64, i64)], end: bool| {
        let mut s = opening(5, 1, 4);
        s.u("num_ref_idx_active_override_flag", 1, 0)
            .u("ref_pic_list_modification_flag_l0", 1, 1);
        for &(op, val) in list {
            s.ue("modification_of_pic_nums_idc", op).ue("value", val);
        }
        if end {
            s.ue("modification_of_pic_nums_idc", 3);
        }
        s.u("adaptive_ref_pic_marking_mode_flag", 1, 0)
            .se("slice_qp_delta", 0);
        unit(&mut s, 2, 1)
    };
    parse_ok(&mut sl, &ps, &mods(&[(0, 4), (2, 1)], true));
    assert_eq!(sl.nb_ref_modifications, [2, 0]);
    assert_eq!(
        sl.ref_modifications[0][..2],
        [
            RefModification { op: 0, val: 4 },
            RefModification { op: 2, val: 1 }
        ]
    );
    // A third, past the two references: refused, and no references left.
    let r = parse(&mut sl, &ps, FIRST, &mods(&[(1, 0), (1, 0), (1, 0)], true)).0;
    assert_eq!(r, Err(SliceError));
    assert_eq!(sl.ref_count, [0, 0]);
    // An operation past 3.
    let r = parse(&mut sl, &ps, FIRST, &mods(&[(4, 0)], true)).0;
    assert_eq!(r, Err(SliceError));
    assert_eq!(sl.ref_count, [0, 0]);
    // A header without modifications counts none, whatever came before.
    parse_ok(&mut sl, &ps, &mods(&[(1, 9)], true));
    assert_eq!(sl.nb_ref_modifications, [1, 0]);
    parse_ok(&mut sl, &ps, &p_slice(|_| {}));
    assert_eq!(sl.nb_ref_modifications, [0, 0]);
}

/// A weighted P slice of `ps` with two references, `weights` written for
/// its table.
fn weighted(
    ps: &ParamSets,
    sl: &mut SliceHeader,
    weights: impl FnOnce(&mut Syntax),
) -> Result<(), SliceError> {
    let mut s = opening(5, 1, 4);
    s.u("num_ref_idx_active_override_flag", 1, 1)
        .ue("num_ref_idx_l0_active_minus1", 1)
        .u("ref_pic_list_modification_flag_l0", 1, 0);
    weights(&mut s);
    s.u("adaptive_ref_pic_marking_mode_flag", 1, 0)
        .se("slice_qp_delta", 0);
    let (r, at_end) = parse(sl, ps, FIRST, &unit(&mut s, 2, 1));
    if r.is_ok() {
        assert!(at_end, "the header read to its end");
    }
    r
}

#[test]
fn weights_are_read_checked_and_copied_for_mbaff() {
    let o = Opts {
        weighted_pred: 1,
        ..MAIN
    };
    let ps = sets(o);
    let mut sl = SliceHeader::default();
    // Reference 0 weighted in luma, reference 1 in chroma.
    weighted(&ps, &mut sl, |s| {
        s.ue("luma_log2_weight_denom", 5)
            .ue("chroma_log2_weight_denom", 3)
            .u("luma_weight_l0_flag[0]", 1, 1)
            .se("luma_weight_l0[0]", -7)
            .se("luma_offset_l0[0]", 12)
            .u("chroma_weight_l0_flag[0]", 1, 0)
            .u("luma_weight_l0_flag[1]", 1, 0)
            .u("chroma_weight_l0_flag[1]", 1, 1)
            .se("chroma_weight_l0[1][0]", 8)
            .se("chroma_offset_l0[1][0]", 0)
            .se("chroma_weight_l0[1][1]", 8)
            .se("chroma_offset_l0[1][1]", -2);
    })
    .unwrap();
    let pwt = &sl.pwt;
    assert_eq!(
        (pwt.luma_log2_weight_denom, pwt.chroma_log2_weight_denom),
        (5, 3)
    );
    assert_eq!(pwt.luma_weight[0][0], [-7, 12]);
    assert_eq!(pwt.luma_weight[1][0], [32, 0]);
    assert_eq!(pwt.chroma_weight[0][0], [[8, 0], [8, 0]]);
    assert_eq!(pwt.chroma_weight[1][0], [[8, 0], [8, -2]]);
    assert_eq!(
        (pwt.luma_weight_flag, pwt.chroma_weight_flag),
        ([1, 0], [1, 0])
    );
    assert_eq!((pwt.use_weight, pwt.use_weight_chroma), (1, 1));
    // A frame's references weigh their fields the same.
    assert_eq!(pwt.luma_weight[16][0], [-7, 12]);
    assert_eq!(pwt.luma_weight[17][0], [-7, 12]);
    assert_eq!(pwt.chroma_weight[19][0], [[8, 0], [8, -2]]);

    // Chroma weighed alone weighs the slice too.
    weighted(&ps, &mut sl, |s| {
        s.ue("luma_log2_weight_denom", 0)
            .ue("chroma_log2_weight_denom", 0)
            .u("luma_weight_l0_flag[0]", 1, 0)
            .u("chroma_weight_l0_flag[0]", 1, 1)
            .se("chroma_weight_l0[0][0]", 2)
            .se("chroma_offset_l0[0][0]", 0)
            .se("chroma_weight_l0[0][1]", 1)
            .se("chroma_offset_l0[0][1]", 0)
            .u("luma_weight_l0_flag[1]", 1, 0)
            .u("chroma_weight_l0_flag[1]", 1, 0);
    })
    .unwrap();
    let pwt = &sl.pwt;
    assert_eq!(
        (pwt.luma_weight_flag, pwt.chroma_weight_flag),
        ([0, 0], [1, 0])
    );
    assert_eq!((pwt.use_weight, pwt.use_weight_chroma), (1, 1));

    // Weights sent but equal to the defaults weigh nothing; denominators
    // past 7 read as 0.
    weighted(&ps, &mut sl, |s| {
        s.ue("luma_log2_weight_denom", 8)
            .ue("chroma_log2_weight_denom", 9)
            .u("luma_weight_l0_flag[0]", 1, 1)
            .se("luma_weight_l0[0]", 1)
            .se("luma_offset_l0[0]", 0)
            .u("chroma_weight_l0_flag[0]", 1, 1);
        for _ in 0..2 {
            s.se("chroma_weight", 1).se("chroma_offset", 0);
        }
        s.u("luma_weight_l0_flag[1]", 1, 0)
            .u("chroma_weight_l0_flag[1]", 1, 0);
    })
    .unwrap();
    let pwt = &sl.pwt;
    assert_eq!(
        (pwt.luma_log2_weight_denom, pwt.chroma_log2_weight_denom),
        (0, 0)
    );
    assert_eq!((pwt.use_weight, pwt.use_weight_chroma), (0, 0));
    assert_eq!(
        (pwt.luma_weight_flag, pwt.chroma_weight_flag),
        ([0, 0], [0, 0])
    );

    // A weight past an int8_t refuses the slice; a chroma one is put back
    // to the default first.
    let r = weighted(&ps, &mut sl, |s| {
        s.ue("luma_log2_weight_denom", 0)
            .ue("chroma_log2_weight_denom", 0)
            .u("luma_weight_l0_flag[0]", 1, 1)
            .se("luma_weight_l0[0]", 128)
            .se("luma_offset_l0[0]", 0);
    });
    assert_eq!(r, Err(SliceError));
    let r = weighted(&ps, &mut sl, |s| {
        s.ue("luma_log2_weight_denom", 0)
            .ue("chroma_log2_weight_denom", 2)
            .u("luma_weight_l0_flag[0]", 1, 0)
            .u("chroma_weight_l0_flag[0]", 1, 1)
            .se("chroma_weight_l0[0][0]", 3)
            .se("chroma_offset_l0[0][0]", -129);
    });
    assert_eq!(r, Err(SliceError));
    assert_eq!(sl.pwt.chroma_weight[0][0][0], [4, 0]);

    // Monochrome: no chroma denominator or weights at all.
    let ps = sets(Opts {
        profile: 100,
        chroma_format_idc: 0,
        ..o
    });
    weighted(&ps, &mut sl, |s| {
        s.ue("luma_log2_weight_denom", 2)
            .u("luma_weight_l0_flag[0]", 1, 0)
            .u("luma_weight_l0_flag[1]", 1, 1)
            .se("luma_weight_l0[1]", 5)
            .se("luma_offset_l0[1]", 1);
    })
    .unwrap();
    assert_eq!(sl.pwt.luma_weight[1][0], [5, 1]);
    assert_eq!(sl.pwt.use_weight_chroma, 0);

    // A field's references are its own: nothing is copied.
    let ps = sets(Opts {
        frame_mbs_only: 0,
        ..o
    });
    sl.pwt.luma_weight[16][0] = [0, 0];
    let mut s = opening(5, 1, 4);
    s.u("field_pic_flag", 1, 1)
        .u("bottom_field_flag", 1, 1)
        .u("num_ref_idx_active_override_flag", 1, 0)
        .u("ref_pic_list_modification_flag_l0", 1, 0)
        .ue("luma_log2_weight_denom", 0)
        .ue("chroma_log2_weight_denom", 0)
        .u("luma_weight_l0_flag[0]", 1, 1)
        .se("luma_weight_l0[0]", 3)
        .se("luma_offset_l0[0]", 3)
        .u("chroma_weight_l0_flag[0]", 1, 0)
        .u("adaptive_ref_pic_marking_mode_flag", 1, 0)
        .se("slice_qp_delta", 0);
    parse_ok(&mut sl, &ps, &unit(&mut s, 2, 1));
    assert_eq!(sl.pwt.luma_weight[0][0], [3, 3]);
    assert_eq!(sl.pwt.luma_weight[16][0], [0, 0]);
}

/// A P frame of `MAIN` marked by `marking`, read: the result, whether it
/// read to its end, and the header.
fn marked(marking: impl FnOnce(&mut Syntax)) -> (Result<(), SliceError>, bool, SliceHeader) {
    marked_as(&[0], marking)
}

/// The same with `structure` after `frame_num`, as for `counted`.
fn marked_as(
    structure: &[i64],
    marking: impl FnOnce(&mut Syntax),
) -> (Result<(), SliceError>, bool, SliceHeader) {
    let ps = sets(Opts {
        frame_mbs_only: 0,
        ..MAIN
    });
    let mut sl = SliceHeader::default();
    let mut s = opening(5, 3, 4);
    for (k, &v) in structure.iter().enumerate() {
        s.u(
            if k == 0 {
                "field_pic_flag"
            } else {
                "bottom_field_flag"
            },
            1,
            v,
        );
    }
    s.u("num_ref_idx_active_override_flag", 1, 0)
        .u("ref_pic_list_modification_flag_l0", 1, 0);
    marking(&mut s);
    s.se("slice_qp_delta", 0);
    let (r, at_end) = parse(&mut sl, &ps, FIRST, &unit(&mut s, 2, 1));
    (r, at_end, sl)
}

#[test]
fn memory_management_is_read_until_it_ends_or_is_refused() {
    // Every operation, then the end.
    let (r, at_end, sl) = marked(|s| {
        s.u("adaptive_ref_pic_marking_mode_flag", 1, 1)
            .ue("op", 1)
            .ue("difference_of_pic_nums_minus1", 5)
            .ue("op", 2)
            .ue("long_term_pic_num", 3)
            .ue("op", 3)
            .ue("difference_of_pic_nums_minus1", 0)
            .ue("long_term_frame_idx", 15)
            .ue("op", 4)
            .ue("max_long_term_frame_idx_plus1", 16)
            .ue("op", 5)
            .ue("op", 6)
            .ue("long_term_frame_idx", 2)
            .ue("op", 0);
    });
    assert_eq!(r, Ok(()));
    assert!(at_end);
    assert_eq!(sl.explicit_ref_marking, 1);
    assert_eq!(sl.nb_mmco, 6);
    let ops: Vec<_> = sl.mmco[..7].iter().map(|m| m.opcode).collect();
    assert_eq!(ops, [1, 2, 3, 4, 5, 6, 0]);
    // frame_num 3 less 5 less 1, wrapped to 16 picture numbers.
    assert_eq!(sl.mmco[0].short_pic_num, 13);
    assert_eq!(sl.mmco[2].short_pic_num, 2);
    let args: Vec<_> = [1, 2, 3, 5].iter().map(|&k| sl.mmco[k].long_arg).collect();
    assert_eq!(args, [3, 15, 16, 2]);

    // A long-term index of 16 is refused unless it is a maximum, or a
    // field's long-term picture number; 32 always -- which is how 35 and
    // up read, the reader seeing only a code's first 9 bits (31 to 34 all
    // read as 31). A refused operation stops the reading without refusing
    // the header -- which then reads on from wherever the operations
    // stopped.
    for (op, arg, ok) in [
        (4, 16, true),
        (2, 16, false),
        (6, 16, false),
        (4, 63, false),
    ] {
        let (r, _, sl) = marked(|s| {
            s.u("adaptive_ref_pic_marking_mode_flag", 1, 1)
                .ue("op", 1)
                .ue("difference_of_pic_nums_minus1", 0)
                .ue("op", op)
                .ue("arg", arg)
                .ue("op", 0);
        });
        assert_eq!(r, Ok(()), "operation {op} {arg}");
        assert_eq!(sl.nb_mmco, if ok { 2 } else { 1 }, "operation {op} {arg}");
    }
    // A field's long-term picture numbers run to 31.
    for (arg, read, ok) in [
        (31, 31, true),
        (34, 31, true),
        (35, 32, false),
        (63, 32, false),
    ] {
        let (r, _, sl) = marked_as(&[1, 1], |s| {
            s.u("adaptive_ref_pic_marking_mode_flag", 1, 1)
                .ue("op", 2)
                .ue("long_term_pic_num", arg)
                .ue("op", 0);
        });
        assert_eq!(r, Ok(()));
        assert_eq!(
            sl.nb_mmco,
            usize::from(ok),
            "long-term picture number {arg}"
        );
        if ok {
            assert_eq!(sl.mmco[0].long_arg, read);
        }
    }
    // An opcode past 6 likewise.
    let (r, _, sl) = marked(|s| {
        s.u("adaptive_ref_pic_marking_mode_flag", 1, 1)
            .ue("op", 5)
            .ue("op", 7);
    });
    assert_eq!(r, Ok(()));
    assert_eq!(sl.nb_mmco, 1);

    // 66 operations without an end are all kept, and the header read on.
    let (r, at_end, sl) = marked(|s| {
        s.u("adaptive_ref_pic_marking_mode_flag", 1, 1);
        for _ in 0..MAX_MMCO_COUNT {
            s.ue("op", 5);
        }
    });
    assert_eq!(r, Ok(()));
    assert!(at_end);
    assert_eq!(sl.nb_mmco, MAX_MMCO_COUNT);

    // No adaptive marking: none, whatever came before.
    let (_, at_end, sl) = marked(|s| {
        s.u("adaptive_ref_pic_marking_mode_flag", 1, 0);
    });
    assert!(at_end);
    assert_eq!((sl.explicit_ref_marking, sl.nb_mmco), (0, 0));
}

#[test]
fn a_long_term_idr_is_one_operation_and_the_rest_are_marked_explicitly() {
    let ps = sets(MAIN);
    let mut sl = SliceHeader::default();
    for long in [1, 0] {
        let mut s = opening(7, 0, 4);
        s.ue("idr_pic_id", 0)
            .u("no_output_of_prior_pics_flag", 1, 1)
            .u("long_term_reference_flag", 1, long)
            .se("slice_qp_delta", 0);
        parse_ok(&mut sl, &ps, &unit(&mut s, 3, 5));
        assert_eq!(sl.explicit_ref_marking, 1);
        assert_eq!(sl.nb_mmco, long as usize);
        if long == 1 {
            assert_eq!((sl.mmco[0].opcode, sl.mmco[0].long_arg), (MMCO_LONG, 0));
        }
    }
    // A unit no picture refers to carries no marking at all.
    let mut s = opening(5, 1, 4);
    s.u("num_ref_idx_active_override_flag", 1, 0)
        .u("ref_pic_list_modification_flag_l0", 1, 0)
        .se("slice_qp_delta", 0);
    sl.explicit_ref_marking = 1;
    parse_ok(&mut sl, &ps, &unit(&mut s, 0, 1));
    assert_eq!(sl.explicit_ref_marking, 0);
}

#[test]
fn cabac_qp_and_deblocking_are_read_and_bounded() {
    let o = Opts {
        cabac: 1,
        deblocking_params: 1,
        init_qp_minus26: 0,
        ..MAIN
    };
    let ps = sets(o);
    let mut sl = SliceHeader::default();
    let tail = |cabac: Option<i64>, qp_delta: i64, deblock: &[i64]| {
        let mut s = opening(5, 1, 4);
        s.u("num_ref_idx_active_override_flag", 1, 0)
            .u("ref_pic_list_modification_flag_l0", 1, 0);
        if let Some(c) = cabac {
            s.ue("cabac_init_idc", c);
        }
        s.se("slice_qp_delta", qp_delta);
        if let Some((&idc, offsets)) = deblock.split_first() {
            s.ue("disable_deblocking_filter_idc", idc);
            for &o in offsets {
                s.se("offset_div2", o);
            }
        }
        unit(&mut s, 0, 1)
    };
    parse_ok(&mut sl, &ps, &tail(Some(2), 25, &[0, -6, 6]));
    assert_eq!((sl.cabac_init_idc, sl.qscale), (2, 51));
    assert_eq!(
        (
            sl.deblocking_filter,
            sl.slice_alpha_c0_offset,
            sl.slice_beta_offset
        ),
        (1, -12, 12)
    );
    // Off: no offsets follow, and they read as 0.
    parse_ok(&mut sl, &ps, &tail(Some(0), -26, &[1]));
    assert_eq!((sl.qscale, sl.deblocking_filter), (0, 0));
    assert_eq!((sl.slice_alpha_c0_offset, sl.slice_beta_offset), (0, 0));
    // Within slices only.
    parse_ok(&mut sl, &ps, &tail(Some(1), 0, &[2, 1, -1]));
    assert_eq!(
        (
            sl.deblocking_filter,
            sl.slice_alpha_c0_offset,
            sl.slice_beta_offset
        ),
        (2, 2, -2)
    );
    for bad in [
        tail(Some(3), 0, &[0, 0, 0]),
        tail(Some(0), 26, &[0, 0, 0]),
        tail(Some(0), -27, &[0, 0, 0]),
        tail(Some(0), 0, &[3]),
        tail(Some(0), 0, &[0, 7, 0]),
        tail(Some(0), 0, &[0, 0, -7]),
    ] {
        assert_eq!(parse(&mut sl, &ps, FIRST, &bad).0, Err(SliceError));
    }

    // An intra slice leaves cabac_init_idc as the last slice left it.
    parse_ok(&mut sl, &ps, &tail(Some(2), 0, &[1]));
    let mut s = opening(7, 1, 4);
    s.se("slice_qp_delta", 0)
        .ue("disable_deblocking_filter_idc", 1);
    parse_ok(&mut sl, &ps, &unit(&mut s, 0, 1));
    assert_eq!(sl.cabac_init_idc, 2);

    // Ten bits: QP runs to 63.
    let ps = sets(Opts {
        profile: 100,
        bit_depth: 10,
        ..o
    });
    parse_ok(&mut sl, &ps, &tail(Some(0), 25, &[1]));
    assert_eq!(sl.qscale, 63);
    assert_eq!(
        parse(&mut sl, &ps, FIRST, &tail(Some(0), 26, &[1])).0,
        Err(SliceError)
    );
}

#[test]
fn sp_and_si_slices_carry_their_own_qp() {
    let ps = sets(MAIN);
    let mut sl = SliceHeader::default();
    // SP: the switch flag and a QS delta after the QP.
    let mut s = opening(3, 1, 4);
    s.u("num_ref_idx_active_override_flag", 1, 0)
        .u("ref_pic_list_modification_flag_l0", 1, 0)
        .se("slice_qp_delta", 1)
        .u("sp_for_switch_flag", 1, 1)
        .se("slice_qs_delta", -4);
    parse_ok(&mut sl, &ps, &unit(&mut s, 0, 1));
    assert_eq!((sl.slice_type, sl.slice_type_nos), (TYPE_SP, TYPE_P));
    // SI: a QS delta only.
    let mut s = opening(9, 1, 4);
    s.se("slice_qp_delta", 0).se("slice_qs_delta", 3);
    parse_ok(&mut sl, &ps, &unit(&mut s, 0, 1));
    assert_eq!(
        (sl.slice_type, sl.slice_type_nos, sl.slice_type_fixed),
        (TYPE_SI, TYPE_I, 1)
    );
}
