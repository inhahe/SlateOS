//! The decoder's public face, on streams written here syntax element by
//! syntax element: pictures of I_PCM macroblocks, whose samples are written
//! out whole, so what each picture must hold is known without FFmpeg.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "test code, on values the tests choose"
)]

use crate::writer::Syntax;
use crate::{Decoder, Error, Picture};

/// What a stream's one picture is made of.
#[derive(Clone, Copy)]
struct Shape {
    /// Its width and height in macroblocks.
    mbs: (u32, u32),
    /// The sample aspect ratio its SPS's VUI gives, if any.
    sar: Option<(u16, u16)>,
    /// Slice groups in its PPS (FMO): more than one is what FFmpeg does not
    /// decode.
    slice_groups: u32,
}

const PLAIN: Shape = Shape {
    mbs: (1, 1),
    sar: None,
    slice_groups: 1,
};

/// Luma sample `i` of macroblock `mb`: never 0, so that no run of zeros
/// needs an escape and an empty plane is told from a written one.
fn luma(mb: u32, i: u32) -> u8 {
    (16 + (mb * 37 + i * 7) % 200) as u8
}

/// A Baseline stream, Annex B, of one IDR picture whose macroblocks are all
/// I_PCM -- an SPS, a PPS, `extra` units (each whole, header first), then
/// the slice.
fn stream(shape: Shape, extra: &[&[u8]]) -> Vec<u8> {
    let (sps, pps) = parameter_sets(shape);
    let slice = i_pcm_slice(shape, 0, Some(0));
    let units: Vec<&[u8]> = [sps.as_slice(), pps.as_slice()]
        .into_iter()
        .chain(extra.iter().copied())
        .chain([slice.as_slice()])
        .collect();
    annex_b(&units)
}

/// `units` (each whole, header first) after start codes.
fn annex_b(units: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for unit in units {
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(unit);
    }
    out
}

/// The SPS and PPS of a Baseline stream of `shape`'s pictures, each
/// header first: frame numbers of 4 bits, order counts that follow them
/// (type 2), one reference frame.
fn parameter_sets(shape: Shape) -> (Vec<u8>, Vec<u8>) {
    let (w, h) = shape.mbs;
    let mut sps = Syntax::default();
    sps.u("profile_idc", 8, 66)
        .u("constraint_set_flags", 8, 0)
        .u("level_idc", 8, 30)
        .ue("seq_parameter_set_id", 0)
        .ue("log2_max_frame_num_minus4", 0)
        .ue("pic_order_cnt_type", 2)
        .ue("max_num_ref_frames", 1)
        .u("gaps_in_frame_num_value_allowed_flag", 1, 0)
        .ue("pic_width_in_mbs_minus1", i64::from(w - 1))
        .ue("pic_height_in_map_units_minus1", i64::from(h - 1))
        .u("frame_mbs_only_flag", 1, 1)
        .u("direct_8x8_inference_flag", 1, 1)
        .u("frame_cropping_flag", 1, 0);
    match shape.sar {
        None => {
            sps.u("vui_parameters_present_flag", 1, 0);
        }
        Some((num, den)) => {
            sps.u("vui_parameters_present_flag", 1, 1)
                .u("aspect_ratio_info_present_flag", 1, 1)
                .u("aspect_ratio_idc", 8, 255)
                .u("sar_width", 16, i64::from(num))
                .u("sar_height", 16, i64::from(den))
                .u("overscan_info_present_flag", 1, 0)
                .u("video_signal_type_present_flag", 1, 0)
                .u("chroma_loc_info_present_flag", 1, 0)
                .u("timing_info_present_flag", 1, 0)
                .u("nal_hrd_parameters_present_flag", 1, 0)
                .u("vcl_hrd_parameters_present_flag", 1, 0)
                .u("pic_struct_present_flag", 1, 0)
                .u("bitstream_restriction_flag", 1, 0);
        }
    }
    let mut pps = Syntax::default();
    pps.ue("pic_parameter_set_id", 0)
        .ue("seq_parameter_set_id", 0)
        .u("entropy_coding_mode_flag", 1, 0)
        .u("bottom_field_pic_order_in_frame_present_flag", 1, 0)
        .ue("num_slice_groups_minus1", i64::from(shape.slice_groups - 1));
    if shape.slice_groups > 1 {
        // Interleaved slice groups, a run of one macroblock each.
        pps.ue("slice_group_map_type", 0);
        for _ in 0..shape.slice_groups {
            pps.ue("run_length_minus1", 0);
        }
    }
    pps.ue("num_ref_idx_l0_default_active_minus1", 0)
        .ue("num_ref_idx_l1_default_active_minus1", 0)
        .u("weighted_pred_flag", 1, 0)
        .u("weighted_bipred_idc", 2, 0)
        .se("pic_init_qp_minus26", 0)
        .se("pic_init_qs_minus26", 0)
        .se("chroma_qp_index_offset", 0)
        .u("deblocking_filter_control_present_flag", 1, 1)
        .u("constrained_intra_pred_flag", 1, 0)
        .u("redundant_pic_cnt_present_flag", 1, 0);
    (sps.finish(0x67), pps.finish(0x68))
}

/// The one slice, header first, of picture `frame_num` of `shape`: an IDR
/// picture of `idr_pic_id`, or for `None` a reference I picture after
/// one. Its macroblocks are all I_PCM: luma sample `i` of macroblock `mb`
/// is `luma(mb + 16 * frame_num, i)`, every chroma sample 128.
fn i_pcm_slice(shape: Shape, frame_num: u32, idr_pic_id: Option<u32>) -> Vec<u8> {
    let (w, h) = shape.mbs;
    let mut slice = Syntax::default();
    slice
        .ue("first_mb_in_slice", 0)
        .ue("slice_type", 7)
        .ue("pic_parameter_set_id", 0)
        .u("frame_num", 4, i64::from(frame_num));
    match idr_pic_id {
        Some(id) => {
            slice
                .ue("idr_pic_id", i64::from(id))
                .u("no_output_of_prior_pics_flag", 1, 0)
                .u("long_term_reference_flag", 1, 0);
        }
        None => {
            slice.u("adaptive_ref_pic_marking_mode_flag", 1, 0);
        }
    }
    slice
        .se("slice_qp_delta", 0)
        .ue("disable_deblocking_filter_idc", 1);
    for mb in 0..w * h {
        slice.ue("mb_type", 25);
        while slice.position() % 8 != 0 {
            slice.u("pcm_alignment_zero_bit", 1, 0);
        }
        for i in 0..256 {
            slice.u(
                "pcm_sample_luma",
                8,
                i64::from(luma(mb + 16 * frame_num, i)),
            );
        }
        for _ in 0..128 {
            slice.u("pcm_sample_chroma", 8, 128);
        }
    }
    slice.finish(if idr_pic_id.is_some() { 0x65 } else { 0x61 })
}

/// Every picture `data` decodes to, as one packet, then drained.
fn pictures(d: &mut Decoder, data: &[u8]) -> Result<Vec<Picture>, Error> {
    let mut out: Vec<Picture> = d.decode(data, 0, 1)?.into_iter().collect();
    while let Some(p) = d.drain() {
        out.push(p);
    }
    Ok(out)
}

/// The luma plane's rows, cropped, as bytes.
fn luma_plane(p: &Picture) -> Vec<u8> {
    let plane = p.plane8(0).unwrap();
    (0..plane.height)
        .flat_map(|y| plane.data[y * plane.stride..y * plane.stride + plane.width].to_vec())
        .collect()
}

/// What `stream`'s I_PCM picture of one macroblock holds in luma, row by
/// row.
fn written_luma() -> Vec<u8> {
    (0..256).map(|i| luma(0, i)).collect()
}

#[test]
fn an_i_pcm_picture_is_its_samples() {
    let mut d = Decoder::new();
    let got = pictures(&mut d, &stream(PLAIN, &[])).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].size(), (16, 16));
    assert_eq!(luma_plane(&got[0]), written_luma());
}

/// A film grain characteristics SEI with no component's model (cancel flag
/// 0, model 0, no colour description, blending 0, scale 0, repetition
/// period 0): FFmpeg's decoder marks the picture for grain, then makes the
/// grain with nothing to add, each plane a copy.
const FILM_GRAIN_SEI: [u8; 6] = [0x06, 19, 2, 0x00, 0x04, 0x80];

#[test]
fn a_picture_with_film_grain_but_no_model_is_shown_as_decoded() {
    let mut d = Decoder::new();
    let got = pictures(&mut d, &stream(PLAIN, &[&FILM_GRAIN_SEI])).unwrap();
    assert_eq!(got.len(), 1);
    // The decoded samples, not a blank frame standing in for grain.
    assert_eq!(luma_plane(&got[0]), written_luma());
}

/// A band of brightness in a film grain model: its lower and upper bound,
/// then the strength and the two cut-off frequencies it is given.
type Band = (i64, i64, i64, i64, i64);

/// The grain the film grain tests give: luma in two bands, Cb in one, Cr
/// none.
const GRAIN_BANDS: [Option<&[Band]>; 3] = [
    Some(&[(0, 100, 60, 6, 9), (101, 255, -45, 10, 4)]),
    Some(&[(0, 255, 40, 3, 3)]),
    None,
];

/// A film grain characteristics SEI, header first: model `model_id` (0
/// frequency filtering, 1 auto-regression), no colour description,
/// additive blending, a scale of 3, [`GRAIN_BANDS`], and a repetition
/// period of 1: until the next IDR or message.
fn film_grain_sei(model_id: i64) -> Vec<u8> {
    let payload = |s: &mut Syntax| {
        s.u("film_grain_characteristics_cancel_flag", 1, 0)
            .u("film_grain_model_id", 2, model_id)
            .u("separate_colour_description_present_flag", 1, 0)
            .u("blending_mode_id", 2, 0)
            .u("log2_scale_factor", 4, 3);
        for b in &GRAIN_BANDS {
            s.u("comp_model_present_flag", 1, i64::from(b.is_some()));
        }
        for b in GRAIN_BANDS.iter().flatten() {
            s.u("num_intensity_intervals_minus1", 8, b.len() as i64 - 1)
                .u("num_model_values_minus1", 3, 2);
            for &(lo, hi, strength, cut_h, cut_v) in *b {
                s.u("intensity_interval_lower_bound", 8, lo)
                    .u("intensity_interval_upper_bound", 8, hi)
                    .se("comp_model_value", strength)
                    .se("comp_model_value", cut_h)
                    .se("comp_model_value", cut_v);
            }
        }
        s.ue("film_grain_characteristics_repetition_period", 1);
    };
    // The payload's size in bytes: its bits, then a 1 and zeros to the byte.
    let mut measure = Syntax::default();
    payload(&mut measure);
    let size = (measure.position() - 8) / 8 + 1;
    assert!(size < 255, "one byte of payload size");
    let mut sei = Syntax::default();
    sei.u("payload_type", 8, 19)
        .u("payload_size", 8, size as i64);
    payload(&mut sei);
    sei.u("payload_bit_equal_to_one", 1, 1);
    while sei.position() % 8 != 0 {
        sei.u("payload_bit_equal_to_zero", 1, 0);
    }
    sei.finish(0x06)
}

/// Three I_PCM pictures of 2x2 macroblocks, the first an IDR of id 3, each
/// a packet of its own, `sei` before the slice of each picture `with_sei`
/// says; each shown picture summed as FFmpeg's `framecrc` muxer sums it.
/// The sums the tests hold them to are FFmpeg 8.1.3's (`tools/ffdbg`'s
/// build): with `H264_WRITE_GRAIN=<directory>` each test writes its stream
/// there as `<name>.h264`, and `ffmpeg -threads 1 -i <file> -f framecrc -`
/// sums it.
fn grain_sums(name: &str, sei: &[u8], with_sei: [bool; 3]) -> Vec<u32> {
    let shape = Shape {
        mbs: (2, 2),
        ..PLAIN
    };
    let (sps, pps) = parameter_sets(shape);
    let packets: Vec<Vec<u8>> = (0..3u32)
        .map(|k| {
            let slice = i_pcm_slice(shape, k, (k == 0).then_some(3));
            let mut units: Vec<&[u8]> = Vec::new();
            if k == 0 {
                units.extend([sps.as_slice(), pps.as_slice()]);
            }
            if with_sei[k as usize] {
                units.push(sei);
            }
            units.push(&slice);
            annex_b(&units)
        })
        .collect();
    if let Some(dir) = std::env::var_os("H264_WRITE_GRAIN") {
        let path = std::path::Path::new(&dir).join(format!("{name}.h264"));
        std::fs::write(path, packets.concat()).unwrap();
    }
    let mut d = Decoder::new();
    let mut got = Vec::new();
    for (k, packet) in packets.iter().enumerate() {
        got.extend(d.decode(packet, k as i64, 1).unwrap());
    }
    while let Some(p) = d.drain() {
        got.push(p);
    }
    got.iter()
        .map(|p| crate::decoder::tests::adler32(0, &crate::decoder::tests::picture_bytes(p)))
        .collect()
}

/// [`grain_sums`]' pictures without grain, as FFmpeg shows them.
const UNGRAINED: [u32; 3] = [0x68d2_cd96, 0x1866_ce66, 0x409e_ce6e];

#[test]
fn pictures_with_film_grain_are_shown_with_ffmpegs_grain() {
    // Each shown with grain seeded by its own order count, and the IDR's
    // by its id too.
    let sums = grain_sums("each", &film_grain_sei(0), [true; 3]);
    assert_eq!(sums, [0x544b_cb2d, 0xb6cb_cc00, 0x0104_ccb0]);
}

#[test]
fn a_film_grain_message_does_not_outlast_its_packet() {
    // FFmpeg forgets the message as the next packet begins, whatever its
    // repetition period says: only the first picture has grain.
    let sums = grain_sums("first", &film_grain_sei(0), [true, false, false]);
    assert_eq!(sums, [0x544b_cb2d, UNGRAINED[1], UNGRAINED[2]]);
}

#[test]
fn pictures_whose_grain_cannot_be_made_are_shown_as_decoded() {
    // Auto-regression, which FFmpeg does not synthesize: each picture is
    // marked for grain, refused it, and shown without.
    let sums = grain_sums("refused", &film_grain_sei(1), [true; 3]);
    assert_eq!(sums, UNGRAINED);
}

#[test]
fn pictures_past_the_limit_are_refused_as_too_large() {
    let big = Shape {
        mbs: (2, 2),
        ..PLAIN
    };
    let mut d = Decoder::new();
    d.set_max_pixels(32 * 32 - 1);
    assert_eq!(
        pictures(&mut d, &stream(big, &[])).err(),
        Some(Error::TooLarge)
    );
    let mut d = Decoder::new();
    d.set_max_pixels(32 * 32);
    let got = pictures(&mut d, &stream(big, &[])).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].size(), (32, 32));
}

#[test]
fn slice_groups_are_refused_as_unsupported() {
    let fmo = Shape {
        mbs: (2, 1),
        slice_groups: 2,
        ..PLAIN
    };
    let mut d = Decoder::new();
    assert_eq!(
        pictures(&mut d, &stream(fmo, &[])).err(),
        Some(Error::Unsupported)
    );
}

#[test]
fn an_aspect_ratio_the_picture_cannot_be_shown_at_is_unknown() {
    // 32x16 at 4000:1: shrinking the height by 4000 leaves no row, so
    // FFmpeg takes the ratio as unknown (`av_image_check_sar`), though the
    // wider side scaled by it would keep samples.
    let wide = Shape {
        mbs: (2, 1),
        sar: Some((4000, 1)),
        ..PLAIN
    };
    let mut d = Decoder::new();
    let got = pictures(&mut d, &stream(wide, &[])).unwrap();
    assert_eq!(got[0].sample_aspect_ratio(), (0, 1));
    // 2:1 is a ratio the picture can be shown at.
    let fine = Shape {
        sar: Some((2, 1)),
        ..wide
    };
    let mut d = Decoder::new();
    let got = pictures(&mut d, &stream(fine, &[])).unwrap();
    assert_eq!(got[0].sample_aspect_ratio(), (2, 1));
}

#[test]
fn the_reorder_delay_is_at_most_what_the_decoder_holds() {
    let mut d = Decoder::new();
    d.set_reorder_delay(20);
    assert_eq!(d.h.avctx.has_b_frames, 16);
    d.set_reorder_delay(3);
    assert_eq!(d.h.avctx.has_b_frames, 3);
}
