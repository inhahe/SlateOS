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
use crate::nal::{Flags, Splitter};
use crate::ps::decode_sps;
use fixtures::ALL;

/// `unit` split from a packet of its own, and read past its header.
fn with_unit<R>(unit: &[u8], f: impl FnOnce(&mut Bits<'_>) -> R) -> R {
    let mut packet = vec![0, 0, 0, 1];
    packet.extend_from_slice(unit);
    let mut s = Splitter::default();
    s.split(&packet, 4, Flags::default()).unwrap();
    let nal = s.nals()[0];
    let mut gb = s.reader(&packet, &nal).unwrap();
    f(&mut gb)
}

fn get(trace: &[(&str, i64)], name: &str) -> Option<i64> {
    trace.iter().find(|(n, _)| *n == name).map(|&(_, v)| v)
}

/// Every element whose name, its index taken off, is `name`, in order.
fn all(trace: &[(&str, i64)], name: &str) -> Vec<i64> {
    trace
        .iter()
        .filter(|(n, _)| n.split('[').next() == Some(name))
        .map(|&(_, v)| v)
        .collect()
}

#[test]
fn each_sei_unit_reads_as_ffmpeg_traces_it() {
    for f in ALL {
        let name = f.name;
        let t = f.trace;
        let mut ps = ParamSets::default();
        with_unit(f.sps, |gb| decode_sps(gb, &mut ps, false).unwrap());
        let mut sei = Sei::default();
        with_unit(f.sei, |gb| sei.decode(gb, &ps)).unwrap();

        assert_eq!(
            i64::from(sei.recovery_frame_cnt),
            get(t, "recovery_frame_cnt").unwrap_or(-1),
            "{name}: recovery point"
        );

        if let Some(kind) = get(t, "frame_packing_arrangement_type") {
            let p = &sei.frame_packing;
            assert!(p.present, "{name}");
            assert_eq!(i64::from(p.arrangement_type), kind, "{name}");
            assert_eq!(
                i64::from(p.content_interpretation_type),
                get(t, "content_interpretation_type").unwrap()
            );
            assert_eq!(
                i64::from(p.current_frame_is_frame0_flag),
                get(t, "current_frame_is_frame0_flag").unwrap()
            );
        } else {
            assert!(!sei.frame_packing.present, "{name}");
        }

        if let Some(cancel) = get(t, "display_orientation_cancel_flag") {
            let d = &sei.display_orientation;
            assert_eq!(d.present, cancel == 0, "{name}");
            assert_eq!(i64::from(d.hflip), get(t, "hor_flip").unwrap());
            assert_eq!(i64::from(d.vflip), get(t, "ver_flip").unwrap());
            assert_eq!(
                i64::from(d.anticlockwise_rotation),
                get(t, "anticlockwise_rotation").unwrap()
            );
        }

        if let Some(max) = get(t, "max_display_mastering_luminance") {
            let m = &sei.mastering_display;
            assert_eq!(m.present, 2, "{name}");
            let xs = all(t, "display_primaries_x");
            let ys = all(t, "display_primaries_y");
            for c in 0..3 {
                assert_eq!(i64::from(m.display_primaries[c][0]), xs[c], "{name}");
                assert_eq!(i64::from(m.display_primaries[c][1]), ys[c], "{name}");
            }
            assert_eq!(
                i64::from(m.white_point[0]),
                get(t, "white_point_x").unwrap()
            );
            assert_eq!(
                i64::from(m.white_point[1]),
                get(t, "white_point_y").unwrap()
            );
            assert_eq!(i64::from(m.max_luminance), max);
            assert_eq!(
                i64::from(m.min_luminance),
                get(t, "min_display_mastering_luminance").unwrap()
            );
        }

        // The x264 version: the text after the UUID, read here by hand.
        let text: Vec<u8> = all(t, "user_data_payload_byte")
            .into_iter()
            .map(|b| b as u8)
            .collect();
        let build = text
            .strip_prefix(b"x264 - core ")
            .map(|rest| {
                let digits: String = rest
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .map(|&b| char::from(b))
                    .collect();
                digits.parse::<i32>().unwrap()
            })
            .unwrap_or(-1);
        assert_eq!(sei.x264_build, build, "{name}: x264 version");
        assert_eq!(sei.unregistered.len(), usize::from(!text.is_empty()));

        // Registered user data: captions, or an active format description.
        let t35 = all(t, "itu_t_t35_payload_byte");
        if get(t, "itu_t_t35_country_code") == Some(0xB5) && t35.len() >= 6 {
            let id: Vec<u8> = t35[2..6].iter().map(|&b| b as u8).collect();
            let data: Vec<u8> = t35[6..].iter().map(|&b| b as u8).collect();
            if id == b"GA94" && data[0] == 3 && data[1] & 0x40 != 0 {
                let n = usize::from(data[1] & 0x1F) * 3;
                assert_eq!(sei.a53_caption, data[3..3 + n], "{name}: captions");
            }
        }

        // Picture timing, read against the SPS.
        if get(t, "pic_struct").is_some() || get(t, "cpb_removal_delay").is_some() {
            assert!(sei.picture_timing.present, "{name}");
            let sps = ps.sps_list.iter().flatten().next().unwrap();
            process_picture_timing(&mut sei.picture_timing, sps).unwrap();
            if let Some(s) = get(t, "pic_struct") {
                assert_eq!(i64::from(sei.picture_timing.pic_struct), s, "{name}");
            }
        }
    }
}

/// An SEI unit of these messages, each `(type, payload)`, with the stop bit
/// after them.
fn sei_unit(messages: &[(u32, &[u8])]) -> Vec<u8> {
    let mut out = vec![0x06];
    for &(kind, payload) in messages {
        let mut k = kind;
        while k >= 255 {
            out.push(255);
            k -= 255;
        }
        out.push(k as u8);
        let mut s = payload.len();
        while s >= 255 {
            out.push(255);
            s -= 255;
        }
        out.push(s as u8);
        out.extend_from_slice(payload);
    }
    out.push(0x80);
    // The escapes a stream would carry.
    let mut escaped = Vec::new();
    let mut zeros = 0;
    for &b in &out {
        if zeros >= 2 && b <= 3 {
            escaped.push(3);
            zeros = 0;
        }
        escaped.push(b);
        zeros = if b == 0 { zeros + 1 } else { 0 };
    }
    escaped
}

fn decode(unit: &[u8], sei: &mut Sei) -> Result<(), SeiError> {
    let ps = ParamSets::default();
    with_unit(unit, |gb| sei.decode(gb, &ps))
}

/// A recovery point of `n` frames: ue(n), then four flag bits, padded.
fn recovery(n: u8) -> Vec<u8> {
    // n < 7: ue(n) is 00 1xx or shorter; write it by hand for small n.
    match n {
        0 => vec![0b1000_0000],
        3 => vec![0b0010_0000],
        _ => panic!("not needed"),
    }
}

#[test]
fn a_damaged_message_stops_the_reading_and_what_came_before_is_kept() {
    let mut sei = Sei::default();
    let mut unit = sei_unit(&[(6, &recovery(3))]);
    // A second message claiming more bytes than there are. (Its header and
    // two bytes: with two or fewer left, FFmpeg's loop would not even
    // begin it.)
    unit.pop(); // the stop bit
    unit.extend_from_slice(&[6, 9, 0x11, 0x22, 0x80]);
    assert_eq!(decode(&unit, &mut sei), Err(SeiError::Invalid));
    assert_eq!(sei.recovery_frame_cnt, 3, "the first message is kept");

    // A message its own parser refuses -- user data of three bytes -- stops
    // the reading as well: the recovery point after it is never read.
    let mut sei = Sei::default();
    let unit = sei_unit(&[(5, &[1, 2, 3]), (6, &recovery(3))]);
    assert_eq!(decode(&unit, &mut sei), Err(SeiError::Invalid));
    assert_eq!(sei.recovery_frame_cnt, -1, "nothing after it is read");
}

#[test]
fn a_buffering_period_naming_a_missing_sps_is_passed_over() {
    let mut sei = Sei::default();
    // sps_id 3 (ue: 00100), no SPS of that id.
    let unit = sei_unit(&[(0, &[0b0010_0000]), (6, &recovery(0))]);
    assert_eq!(decode(&unit, &mut sei), Err(SeiError::PsNotFound));
    assert_eq!(sei.recovery_frame_cnt, 0, "read on past it");
}

#[test]
fn messages_of_types_and_sizes_past_254_are_read() {
    let mut sei = Sei::default();
    // An unknown type 300 of 260 bytes, then a recovery point.
    let big = vec![0x55u8; 260];
    let unit = sei_unit(&[(300, &big), (6, &recovery(3))]);
    decode(&unit, &mut sei).unwrap();
    assert_eq!(sei.recovery_frame_cnt, 3);
}

#[test]
fn the_x264_version_is_read_as_sscanf_reads_it() {
    for (text, want) in [
        (&b"x264 - core 148 r2643 5c65704"[..], Some(148)),
        (b"x264-core148", Some(148)),
        (b"x264 \t-\ncore   9", Some(9)),
        (b"x264 - core +12", Some(12)),
        (b"x264 - core -5", Some(-5)),
        (b" x264 - core 148", None),
        (b"x264 - core abc", None),
        (b"x264 - cor 148", None),
        (b"X264 - core 148", None),
    ] {
        assert_eq!(x264_core(text), want, "{}", String::from_utf8_lossy(text));
    }
    let uuid = [0xDCu8; 16];
    let user = |text: &[u8]| {
        let mut p = uuid.to_vec();
        p.extend_from_slice(text);
        p
    };
    let mut sei = Sei::default();
    decode(&sei_unit(&[(5, &user(b"x264 - core 148"))]), &mut sei).unwrap();
    assert_eq!(sei.x264_build, 148);
    // A build of nought is no build; a later unit without one keeps it, and
    // so does clearing for the next access unit.
    decode(&sei_unit(&[(5, &user(b"x264 - core 0"))]), &mut sei).unwrap();
    assert_eq!(sei.x264_build, 148);
    sei.uninit();
    assert_eq!(sei.x264_build, 148);
    // x264's own builds numbered 0000-something: build 67.
    decode(&sei_unit(&[(5, &user(b"x264 - core 00001"))]), &mut sei).unwrap();
    assert_eq!(sei.x264_build, 67);
    // The text ends at a NUL.
    let mut s = Sei::default();
    decode(&sei_unit(&[(5, &user(b"x264 - core \x0099"))]), &mut s).unwrap();
    assert_eq!(s.x264_build, -1);
    // Fewer than 16 bytes is no user data at all.
    let mut s = Sei::default();
    assert_eq!(
        decode(&sei_unit(&[(5, &[1, 2, 3])]), &mut s),
        Err(SeiError::Invalid)
    );
}

#[test]
fn captions_are_three_bytes_each_and_a_short_message_is_refused() {
    let caption = |cc: &[u8], count: u8| {
        let mut p = vec![0xB5, 0x00, 0x31];
        p.extend_from_slice(b"GA94");
        p.extend_from_slice(&[0x03, 0x40 | count, 0xFF]);
        p.extend_from_slice(cc);
        p.push(0xFF); // marker bits
        p
    };
    let mut sei = Sei::default();
    let unit = sei_unit(&[
        (4, &caption(&[0xFC, 0x80, 0x80, 0xFD, 0x20, 0x21], 2)),
        (4, &caption(&[0xFC, 0x41, 0x42], 1)),
    ]);
    decode(&unit, &mut sei).unwrap();
    assert_eq!(
        sei.a53_caption,
        [0xFC, 0x80, 0x80, 0xFD, 0x20, 0x21, 0xFC, 0x41, 0x42],
        "a second field's captions follow the first's"
    );
    // A count running past the bytes there are.
    let mut sei = Sei::default();
    let unit = sei_unit(&[(4, &caption(&[0xFC, 0x80, 0x80], 2))]);
    assert_eq!(decode(&unit, &mut sei), Err(SeiError::Invalid));
    // An active format description.
    let mut sei = Sei::default();
    let mut afd = vec![0xB5, 0x00, 0x31];
    afd.extend_from_slice(b"DTG1");
    afd.extend_from_slice(&[0x41, 0xF9]);
    decode(&sei_unit(&[(4, &afd)]), &mut sei).unwrap();
    assert_eq!(sei.afd, Some(9));
}

#[test]
fn film_grain_parameters_are_read() {
    // Written bit by bit: cancel 0, model 1, a colour description or none
    // (read past either way), blending 0, scale 6, components 1 0 1;
    // component 0 two intervals of two values, component 2 one interval of
    // one value; repetition 0.
    for colour in [false, true] {
        film_grain_parameters_are_read_with(colour);
    }
}

fn film_grain_parameters_are_read_with(colour: bool) {
    let mut bits: Vec<(u32, u32)> = vec![(0, 1), (1, 2), (u32::from(colour), 1)];
    if colour {
        // Bit depths 10 and 9, full range, primaries 9, transfer 16, matrix 9.
        bits.extend([(2, 3), (1, 3), (1, 1), (9, 8), (16, 8), (9, 8)]);
    }
    bits.extend([(0, 2), (6, 4), (1, 1), (0, 1), (1, 1)]);
    let se = |v: i32| -> (u32, u32) {
        // A signed Exp-Golomb code as (bits, length).
        let code = (if v > 0 { 2 * v - 1 } else { -2 * v }) as u32 + 1;
        let n = 32 - code.leading_zeros();
        (code, 2 * n - 1)
    };
    bits.extend([(1, 8), (1, 3)]); // two intervals, two values
    for (lo, hi, a, b) in [(10, 50, 3, -2), (60, 200, -7, 0)] {
        bits.extend([(lo, 8), (hi, 8), se(a), se(b)]);
    }
    bits.extend([(0, 8), (0, 3), (20, 8), (30, 8), se(5)]); // one interval, one value
    bits.push((1, 1)); // repetition period 0: ue "1"
    let mut out: Vec<u8> = Vec::new();
    let mut len = 0usize;
    for (v, n) in bits {
        for k in (0..n).rev() {
            if len.is_multiple_of(8) {
                out.push(0);
            }
            if (v >> k) & 1 != 0 {
                *out.last_mut().unwrap() |= 0x80 >> (len % 8);
            }
            len += 1;
        }
    }
    let mut sei = Sei::default();
    decode(&sei_unit(&[(19, &out)]), &mut sei).unwrap();
    let fg = sei.film_grain.as_ref().unwrap();
    assert!(fg.present);
    let with = format!("colour description: {colour}");
    assert_eq!((fg.model_id, fg.log2_scale_factor), (1, 6), "{with}");
    assert_eq!(fg.comp_model_present_flag, [1, 0, 1], "{with}");
    assert_eq!(fg.num_intensity_intervals, [2, 0, 1], "{with}");
    assert_eq!(fg.num_model_values, [2, 0, 1], "{with}");
    assert_eq!(fg.intensity_interval_lower_bound[0][1], 60);
    assert_eq!(fg.intensity_interval_upper_bound[0][1], 200);
    assert_eq!(fg.comp_model_value[0][0][..2], [3, -2]);
    assert_eq!(fg.comp_model_value[0][1][..2], [-7, 0]);
    assert_eq!(fg.comp_model_value[2][0][0], 5, "{with}");

    // A cancel flag: the message is kept, saying nothing.
    let mut sei = Sei::default();
    decode(&sei_unit(&[(19, &[0x80])]), &mut sei).unwrap();
    assert!(!sei.film_grain.as_ref().unwrap().present);
}

#[test]
fn clearing_for_the_next_access_unit_clears_what_ffmpeg_clears() {
    let f = &fixtures::MANY;
    let mut ps = ParamSets::default();
    with_unit(f.sps, |gb| decode_sps(gb, &mut ps, false).unwrap());
    let mut sei = Sei::default();
    with_unit(f.sei, |gb| sei.decode(gb, &ps)).unwrap();
    let sps = ps.sps_list.iter().flatten().next().unwrap();
    process_picture_timing(&mut sei.picture_timing, sps).unwrap();
    let before = sei.clone();
    sei.recovery_frame_cnt = 5;
    sei.uninit();
    assert_eq!(sei.recovery_frame_cnt, -1);
    assert!(!sei.picture_timing.present);
    assert!(!sei.display_orientation.present);
    assert_eq!(sei.mastering_display.present, 0);
    // What FFmpeg leaves: the structure and time codes, the orientation's
    // and the display's values.
    assert_eq!(
        sei.picture_timing.pic_struct,
        before.picture_timing.pic_struct
    );
    assert_eq!(sei.picture_timing.payload, before.picture_timing.payload);
    assert_eq!(
        sei.display_orientation.anticlockwise_rotation,
        before.display_orientation.anticlockwise_rotation
    );
    assert_eq!(
        sei.mastering_display.display_primaries,
        before.mastering_display.display_primaries
    );
}

#[test]
fn a_picture_structure_past_frame_tripling_is_refused() {
    let f = &fixtures::BROADCAST;
    let mut ps = ParamSets::default();
    with_unit(f.sps, |gb| decode_sps(gb, &mut ps, false).unwrap());
    let sps = ps.sps_list.iter().flatten().next().unwrap();
    assert_eq!(
        sps.pic_struct_present_flag, 1,
        "the fixture must carry pic_struct"
    );
    let hrd = sps.nal_hrd_parameters_present_flag != 0 || sps.vcl_hrd_parameters_present_flag != 0;
    // pic_struct 9, after the delays when the SPS has them.
    let delay_bits = if hrd {
        (sps.cpb_removal_delay_length + sps.dpb_output_delay_length) as usize
    } else {
        0
    };
    let mut bits = vec![0u8; delay_bits];
    bits.extend([1, 0, 0, 1]);
    let mut payload = vec![0u8; bits.len().div_ceil(8) + 1];
    for (k, &b) in bits.iter().enumerate() {
        payload[k / 8] |= b << (7 - k % 8);
    }
    let mut pt = PictureTiming {
        payload,
        present: true,
        ..PictureTiming::default()
    };
    assert_eq!(process_picture_timing(&mut pt, sps), Err(SeiError::Invalid));
}
