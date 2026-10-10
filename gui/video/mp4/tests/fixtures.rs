//! Every fixture's packets, held to `ffprobe`'s: what FFmpeg's demuxer makes
//! of each (`tests/data/generate_fixtures.py`, which says how each fixture
//! was made and what it is for). A test per fixture, so that a failure names
//! the file.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud, and its numbers are small"
)]

use std::fs::File;

use mp4::{Demuxer, TrackKind};

/// One packet as `ffprobe` printed it.
#[derive(Debug, PartialEq, Eq)]
struct Line {
    stream: usize,
    pts: i64,
    dts: i64,
    duration: i64,
    size: usize,
    pos: u64,
    flags: String,
    md5: String,
    skip: u32,
}

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// One stream as `ffprobe` printed it.
#[derive(Debug)]
struct Stream {
    tag: String,
    time_base: (u64, u64),
    /// Its picture's size (`WxH`) or its sound's rate and channels
    /// (`RHz/C`).
    shape: String,
    /// `duration_ts`: how long it lasts, in its ticks.
    duration: String,
    /// A picture's `r_frame_rate` (`-` for sound).
    rate: String,
}

/// A fixture's answers: each stream, each `look` line's words after the
/// stream's index, the packets, and whether FFmpeg refuses the file.
struct Answers {
    streams: Vec<Stream>,
    looks: Vec<String>,
    packets: Vec<Line>,
    refused: bool,
}

fn answers(name: &str) -> Answers {
    let base = name.rsplit_once('.').unwrap().0;
    let path = data(&format!("{base}.txt"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut a = Answers {
        streams: Vec::new(),
        looks: Vec::new(),
        packets: Vec::new(),
        refused: false,
    };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split(' ').collect();
        match w[0] {
            "stream" => {
                let (num, den) = w[3].split_once('/').unwrap();
                a.streams.push(Stream {
                    tag: w[2].to_owned(),
                    time_base: (num.parse().unwrap(), den.parse().unwrap()),
                    shape: w[4].to_owned(),
                    duration: w[5].to_owned(),
                    rate: w[6].to_owned(),
                });
            }
            "look" => a.looks.push(w[2..].join(" ")),
            "refused" => a.refused = true,
            "packet" => a.packets.push(Line {
                stream: w[1].parse().unwrap(),
                pts: w[2].parse().unwrap(),
                dts: w[3].parse().unwrap(),
                // ffprobe prints a duration of 0 as N/A.
                duration: if w[4] == "N/A" {
                    0
                } else {
                    w[4].parse().unwrap()
                },
                size: w[5].parse().unwrap(),
                pos: w[6].parse().unwrap(),
                flags: w[7].to_owned(),
                md5: w[8].to_owned(),
                skip: w[9].parse().unwrap(),
            }),
            other => panic!("{path}: a line this test does not know: {other}"),
        }
    }
    a
}

/// `num/den` in lowest terms, as ffprobe prints a rate.
fn reduced(num: u64, den: u64) -> String {
    let (mut a, mut b) = (num, den);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    match a {
        0 => format!("{num}/{den}"),
        g => format!("{}/{}", num / g, den / g),
    }
}

/// FFmpeg's names for ITU-T H.273's colour primaries (`libavutil/pixdesc.c`),
/// as ffprobe prints them.
fn primaries_name(v: u16) -> &'static str {
    match v {
        0 | 3 => "reserved",
        1 => "bt709",
        4 => "bt470m",
        5 => "bt470bg",
        6 => "smpte170m",
        7 => "smpte240m",
        8 => "film",
        9 => "bt2020",
        10 => "smpte428",
        11 => "smpte431",
        12 => "smpte432",
        22 => "ebu3213",
        256 => "vgamut",
        _ => "unknown",
    }
}

/// FFmpeg's names for the transfer characteristics.
fn transfer_name(v: u16) -> &'static str {
    match v {
        0 | 3 => "reserved",
        1 => "bt709",
        4 => "bt470m",
        5 => "bt470bg",
        6 => "smpte170m",
        7 => "smpte240m",
        8 => "linear",
        9 => "log100",
        10 => "log316",
        11 => "iec61966-2-4",
        12 => "bt1361e",
        13 => "iec61966-2-1",
        14 => "bt2020-10",
        15 => "bt2020-12",
        16 => "smpte2084",
        17 => "smpte428",
        18 => "arib-std-b67",
        256 => "vlog",
        _ => "unknown",
    }
}

/// FFmpeg's names for the matrix coefficients (its "colour spaces").
fn space_name(v: u16) -> &'static str {
    match v {
        0 => "gbr",
        1 => "bt709",
        3 => "reserved",
        4 => "fcc",
        5 => "bt470bg",
        6 => "smpte170m",
        7 => "smpte240m",
        8 => "ycgco",
        9 => "bt2020nc",
        10 => "bt2020c",
        11 => "smpte2085",
        12 => "chroma-derived-nc",
        13 => "chroma-derived-c",
        14 => "ictcp",
        15 => "ipt-c2",
        16 => "ycgco-re",
        17 => "ycgco-ro",
        _ => "unknown",
    }
}

/// A track's `look` line, after its index, as the generator writes
/// ffprobe's: the pixel's shape (ffprobe shows only a positive one), the
/// colour, the display matrix and the crop.
fn look(t: &mp4::Track) -> String {
    let v = t.video;
    let sar = match v.and_then(|v| v.pixel_aspect) {
        Some((n, d)) if n > 0 && d > 0 => format!("{n}:{d}"),
        _ => "N/A".to_owned(),
    };
    let colour = match v.and_then(|v| v.colour) {
        Some(c) => format!(
            "{}/{}/{}/{}",
            primaries_name(c.primaries),
            transfer_name(c.transfer),
            space_name(c.matrix),
            match c.full_range {
                Some(true) => "pc",
                Some(false) => "tv",
                None => "unknown",
            }
        ),
        None => "unknown/unknown/unknown/unknown".to_owned(),
    };
    let matrix = match v.and_then(|v| v.matrix) {
        Some(m) => m.map(|n| n.to_string()).join(","),
        None => "none".to_owned(),
    };
    let crop = v
        .map_or([0; 4], |v| v.crop)
        .map(|n| n.to_string())
        .join(",");
    let light = match v.and_then(|v| v.content_light) {
        Some(l) => format!("{},{}", l.max_cll, l.max_fall),
        None => "none".to_owned(),
    };
    // FFmpeg's rationals, unreduced: each number over its box's scale.
    let mastering = match v.and_then(|v| v.mastering) {
        Some(m) => {
            let chromaticities = m
                .chromaticities
                .map(|c| format!("{c}/{}", m.chromaticity_scale))
                .join(",");
            format!(
                "{chromaticities};{}/{};{}/{}",
                m.max_luminance, m.max_luminance_scale, m.min_luminance, m.min_luminance_scale
            )
        }
        None => "none".to_owned(),
    };
    format!(
        "sar={sar} colour={colour} matrix={matrix} crop={crop} light={light} mastering={mastering}"
    )
}

/// ffprobe's flags: K a key frame, D discarded, C cut short.
fn flags(p: &mp4::Packet) -> String {
    format!(
        "{}{}{}",
        if p.keyframe { 'K' } else { '_' },
        if p.discard { 'D' } else { '_' },
        if p.corrupt { 'C' } else { '_' }
    )
}

/// The fixture's streams -- codec tag, time base, picture or sound,
/// duration, frame rate, and what a `look` line says of the picture -- and
/// every packet -- the stream, times, duration, size, position, flags, bytes
/// and samples to skip -- as FFmpeg's demuxer gives them; or, for a file
/// FFmpeg refuses, a refusal.
fn demuxes_as_ffmpeg_does(name: &str) {
    let a = answers(name);
    let opened = Demuxer::open(File::open(data(name)).unwrap());
    if a.refused {
        assert!(opened.is_err(), "{name}: FFmpeg refuses it, and it opened");
        return;
    }
    let mut d = opened.unwrap_or_else(|e| panic!("{name}: {e}"));
    let (streams, expected) = (&a.streams, &a.packets);
    assert_eq!(d.tracks().len(), streams.len(), "{name}: the streams");
    for (t, s) in d.tracks().iter().zip(streams) {
        let ours = String::from_utf8_lossy(&t.codec_tag).into_owned();
        assert_eq!(ours, s.tag, "{name}: track {}'s codec tag", t.id);
        let our_shape = match (t.video, t.audio) {
            (Some(v), _) => format!("{}x{}", v.width, v.height),
            (_, Some(a)) => format!("{}Hz/{}", a.sample_rate, a.channels),
            _ => "-".to_owned(),
        };
        assert_eq!(
            our_shape, s.shape,
            "{name}: track {}'s picture or sound",
            t.id
        );
        assert_eq!(
            (1, u64::from(t.timescale)),
            s.time_base,
            "{name}: track {}'s time base",
            t.id
        );
        assert_eq!(
            t.duration.to_string(),
            s.duration,
            "{name}: track {}'s duration",
            t.id
        );
        // Where the crate finds one frame rate, FFmpeg's demuxer found it
        // too; where it does not, ffprobe's figure is its own estimate.
        if let Some(d) = t.video.and_then(|v| v.frame_duration) {
            assert_eq!(
                reduced(u64::from(t.timescale), u64::from(d)),
                s.rate,
                "{name}: track {}'s frame rate",
                t.id
            );
        }
    }
    if !a.looks.is_empty() {
        let ours: Vec<String> = d.tracks().iter().map(look).collect();
        assert_eq!(
            ours, a.looks,
            "{name}: what the tracks say of their pictures"
        );
    }
    let mut got = Vec::new();
    while let Some(p) = d.next_packet().unwrap_or_else(|e| panic!("{name}: {e}")) {
        got.push(Line {
            stream: p.track,
            pts: p.pts,
            dts: p.dts,
            duration: p.duration,
            size: p.data.len(),
            pos: p.position,
            flags: flags(&p),
            md5: md5::md5_hex(&p.data).to_string(),
            skip: p.skip_samples,
        });
    }
    for (i, (g, e)) in got.iter().zip(expected).enumerate() {
        assert_eq!(g, e, "{name}: packet {i}");
    }
    assert_eq!(got.len(), expected.len(), "{name}: the packets");
}

#[test]
fn packets_of_av1() {
    demuxes_as_ffmpeg_does("av1.mp4");
}

#[test]
fn packets_of_faststart() {
    demuxes_as_ffmpeg_does("faststart.mp4");
}

#[test]
fn packets_of_vp9_opus() {
    demuxes_as_ffmpeg_does("vp9_opus.mp4");
}

#[test]
fn packets_of_h264_bframes() {
    demuxes_as_ffmpeg_does("h264_bframes.mp4");
}

#[test]
fn packets_of_h264_negative_cts() {
    demuxes_as_ffmpeg_does("h264_negative_cts.mp4");
}

#[test]
fn packets_of_mpeg4_bframes() {
    demuxes_as_ffmpeg_does("mpeg4_bframes.mp4");
}

#[test]
fn packets_of_aac() {
    demuxes_as_ffmpeg_does("aac.mp4");
}

#[test]
fn packets_of_delayed_audio() {
    demuxes_as_ffmpeg_does("delayed_audio.mp4");
}

#[test]
fn packets_of_fragmented() {
    demuxes_as_ffmpeg_does("fragmented.mp4");
}

#[test]
fn packets_of_fragmented_moof_base() {
    demuxes_as_ffmpeg_does("fragmented_moof_base.mp4");
}

#[test]
fn packets_of_pcm() {
    demuxes_as_ffmpeg_does("pcm.mov");
}

#[test]
fn packets_of_mov_text() {
    demuxes_as_ffmpeg_does("mov_text.mp4");
    // A `tx3g` entry in an `sbtl` track is subtitles, its setup the rest of
    // the entry: FFmpeg's `mov_codec_id` and `mov_parse_stsd_subtitle`.
    let d = Demuxer::open(File::open(data("mov_text.mp4")).unwrap()).unwrap();
    let text = &d.tracks()[1];
    assert_eq!(
        (text.kind, text.codec, &text.codec_tag),
        (TrackKind::Subtitle, mp4::Codec::MovText, b"tx3g")
    );
    // Display flags, justification, background, text box, default style,
    // then the font table: FFmpeg's extradata, as `ffprobe -show_entries
    // stream=extradata_size,extradata_hash -show_data_hash MD5` gives it.
    assert_eq!(&text.config[30 + 4..30 + 8], b"ftab");
    assert_eq!(
        (text.config.len(), md5::md5_hex(&text.config).to_string()),
        (68, "17972bfddf33ec44918b35c84a268606".to_owned())
    );
}

#[test]
fn packets_of_two_edits() {
    demuxes_as_ffmpeg_does("two_edits.mp4");
}

#[test]
fn packets_of_empty_edit() {
    demuxes_as_ffmpeg_does("empty_edit.mp4");
}

#[test]
fn packets_of_edit_mid_gop() {
    demuxes_as_ffmpeg_does("edit_mid_gop.mp4");
}

#[test]
fn packets_of_sound_edit_mid_frame() {
    demuxes_as_ffmpeg_does("sound_edit_mid_frame.mp4");
}

#[test]
fn packets_of_stsc_repair() {
    demuxes_as_ffmpeg_does("stsc_repair.mp4");
}

#[test]
fn packets_of_stts_negative() {
    demuxes_as_ffmpeg_does("stts_negative.mp4");
}

#[test]
fn packets_of_no_stss() {
    demuxes_as_ffmpeg_does("no_stss.mp4");
}

#[test]
fn packets_of_empty_stss() {
    demuxes_as_ffmpeg_does("empty_stss.mp4");
}

#[test]
fn packets_of_stps() {
    demuxes_as_ffmpeg_does("stps.mp4");
}

#[test]
fn packets_of_rap_group() {
    demuxes_as_ffmpeg_does("rap_group.mp4");
}

#[test]
fn packets_of_ctts_tail() {
    demuxes_as_ffmpeg_does("ctts_tail.mp4");
}

#[test]
fn packets_of_two_entries() {
    demuxes_as_ffmpeg_does("two_entries.mp4");
}

#[test]
fn packets_of_two_codecs() {
    demuxes_as_ffmpeg_does("two_codecs.mp4");
}

#[test]
fn packets_of_co64_stz2() {
    demuxes_as_ffmpeg_does("co64_stz2.mp4");
}

#[test]
fn packets_of_stz2_4bit() {
    demuxes_as_ffmpeg_does("stz2_4bit.mp4");
}

#[test]
fn packets_of_largesize() {
    demuxes_as_ffmpeg_does("largesize.mp4");
}

#[test]
fn packets_of_mdat_to_end() {
    demuxes_as_ffmpeg_does("mdat_to_end.mp4");
}

#[test]
fn packets_of_moov_to_end() {
    demuxes_as_ffmpeg_does("moov_to_end.mp4");
}

#[test]
fn packets_of_trak_to_end() {
    demuxes_as_ffmpeg_does("trak_to_end.mp4");
}

#[test]
fn packets_of_edit_before_key_shows() {
    demuxes_as_ffmpeg_does("edit_before_key_shows.mp4");
}

#[test]
fn packets_of_edit_at_shown_key() {
    demuxes_as_ffmpeg_does("edit_at_shown_key.mp4");
}

#[test]
fn packets_of_hoov() {
    demuxes_as_ffmpeg_does("hoov.mp4");
}

#[test]
fn packets_of_truncated() {
    demuxes_as_ffmpeg_does("truncated.mp4");
}

#[test]
fn packets_of_both_rotations() {
    demuxes_as_ffmpeg_does("both_rotations.mp4");
}

#[test]
fn packets_of_clap() {
    demuxes_as_ffmpeg_does("clap.mp4");
}

#[test]
fn packets_of_clap_fraction() {
    demuxes_as_ffmpeg_does("clap_fraction.mp4");
}

#[test]
fn packets_of_clap_infinite() {
    demuxes_as_ffmpeg_does("clap_infinite.mp4");
}

#[test]
fn packets_of_clap_offset() {
    demuxes_as_ffmpeg_does("clap_offset.mp4");
}

#[test]
fn packets_of_clap_outside() {
    demuxes_as_ffmpeg_does("clap_outside.mp4");
}

#[test]
fn packets_of_clap_too_wide() {
    demuxes_as_ffmpeg_does("clap_too_wide.mp4");
}

#[test]
fn packets_of_clap_wider_by_half() {
    demuxes_as_ffmpeg_does("clap_wider_by_half.mp4");
}

#[test]
fn packets_of_clap_twice() {
    demuxes_as_ffmpeg_does("clap_twice.mp4");
}

#[test]
fn packets_of_colr_nclc_after_vpcc() {
    demuxes_as_ffmpeg_does("colr_nclc_after_vpcc.mp4");
}

#[test]
fn packets_of_colr_nclx() {
    demuxes_as_ffmpeg_does("colr_nclx.mp4");
}

#[test]
fn packets_of_colr_prof() {
    demuxes_as_ffmpeg_does("colr_prof.mp4");
}

#[test]
fn packets_of_colr_rare_codes() {
    demuxes_as_ffmpeg_does("colr_rare_codes.mp4");
}

#[test]
fn packets_of_colr_unknown_codes() {
    demuxes_as_ffmpeg_does("colr_unknown_codes.mp4");
}

#[test]
fn packets_of_matrix_stretch() {
    demuxes_as_ffmpeg_does("matrix_stretch.mp4");
}

#[test]
fn packets_of_mirror() {
    demuxes_as_ffmpeg_does("mirror.mp4");
}

#[test]
fn packets_of_movie_rotation() {
    demuxes_as_ffmpeg_does("movie_rotation.mp4");
}

#[test]
fn packets_of_pasp() {
    demuxes_as_ffmpeg_does("pasp.mp4");
}

#[test]
fn packets_of_pasp_no_horizontal() {
    demuxes_as_ffmpeg_does("pasp_no_horizontal.mp4");
}

#[test]
fn packets_of_pasp_no_vertical() {
    demuxes_as_ffmpeg_does("pasp_no_vertical.mp4");
}

#[test]
fn packets_of_pasp_reduced() {
    demuxes_as_ffmpeg_does("pasp_reduced.mp4");
}

#[test]
fn packets_of_rotate_180() {
    demuxes_as_ffmpeg_does("rotate_180.mp4");
}

#[test]
fn packets_of_rotate_270() {
    demuxes_as_ffmpeg_does("rotate_270.mp4");
}

#[test]
fn packets_of_rotate_90() {
    demuxes_as_ffmpeg_does("rotate_90.mp4");
}

#[test]
fn packets_of_tkhd_size() {
    demuxes_as_ffmpeg_does("tkhd_size.mp4");
}

#[test]
fn packets_of_vpcc() {
    demuxes_as_ffmpeg_does("vpcc.mp4");
}

#[test]
fn packets_of_vpcc_init_data() {
    demuxes_as_ffmpeg_does("vpcc_init_data.mp4");
}

#[test]
fn packets_of_vpcc_short() {
    demuxes_as_ffmpeg_does("vpcc_short.mp4");
}

#[test]
fn packets_of_vpcc_version_0() {
    demuxes_as_ffmpeg_does("vpcc_version_0.mp4");
}

// The light: the mastering display and the content light level, each from
// its box or VP9's, the first of two standing; and the boxes FFmpeg refuses
// the file for.

#[test]
fn packets_of_light_mdcv_clli() {
    demuxes_as_ffmpeg_does("light_mdcv_clli.mp4");
}

#[test]
fn packets_of_light_smdm_coll() {
    demuxes_as_ffmpeg_does("light_smdm_coll.mp4");
}

#[test]
fn packets_of_light_versions() {
    demuxes_as_ffmpeg_does("light_versions.mp4");
}

#[test]
fn packets_of_light_first_of_two() {
    demuxes_as_ffmpeg_does("light_first_of_two.mp4");
}

/// A `SmDm` of 5 bytes: its numbers are read on from the boxes after it,
/// as FFmpeg reads them, and those boxes then read as themselves.
#[test]
fn packets_of_light_overread() {
    demuxes_as_ffmpeg_does("light_overread.mp4");
}

#[test]
fn packets_of_light_mdcv_short() {
    demuxes_as_ffmpeg_does("light_mdcv_short.mp4");
}

#[test]
fn packets_of_light_clli_short() {
    demuxes_as_ffmpeg_does("light_clli_short.mp4");
}

#[test]
fn packets_of_light_smdm_empty() {
    demuxes_as_ffmpeg_does("light_smdm_empty.mp4");
}

#[test]
fn packets_of_light_coll_empty() {
    demuxes_as_ffmpeg_does("light_coll_empty.mp4");
}

#[test]
fn packets_of_light_outside_a_track() {
    demuxes_as_ffmpeg_does("light_outside_a_track.mp4");
}

// Tables claiming more samples than the file has bytes for, or than FFmpeg
// can index. The crate holds an index to what the file's length allows,
// FFmpeg to what it can allocate: what either reads is the same.

#[test]
fn packets_of_claims_a_million() {
    demuxes_as_ffmpeg_does("claims_a_million.mp4");
}

#[test]
fn packets_of_claims_ffmpeg_whole_index() {
    demuxes_as_ffmpeg_does("claims_ffmpeg_whole_index.mp4");
}

#[test]
fn packets_of_claims_past_ffmpeg_index() {
    demuxes_as_ffmpeg_does("claims_past_ffmpeg_index.mp4");
}

#[test]
fn packets_of_trun_claims_a_million() {
    demuxes_as_ffmpeg_does("trun_claims_a_million.mp4");
}

#[test]
fn packets_of_trun_ffmpeg_whole_index() {
    demuxes_as_ffmpeg_does("trun_ffmpeg_whole_index.mp4");
}

#[test]
fn packets_of_trun_past_ffmpeg_index() {
    demuxes_as_ffmpeg_does("trun_past_ffmpeg_index.mp4");
}

#[test]
fn packets_of_trun_runs_claim_millions() {
    demuxes_as_ffmpeg_does("trun_runs_claim_millions.mp4");
}

#[test]
fn packets_of_trun_time_overflows() {
    demuxes_as_ffmpeg_does("trun_time_overflows.mp4");
}

#[test]
fn packets_of_trun_run_of_no_size() {
    demuxes_as_ffmpeg_does("trun_run_of_no_size.mp4");
}

#[test]
fn packets_of_chunked_below_ffmpeg_tts_limit() {
    demuxes_as_ffmpeg_does("chunked_below_ffmpeg_tts_limit.mp4");
}

#[test]
fn packets_of_chunked_at_ffmpeg_tts_limit() {
    demuxes_as_ffmpeg_does("chunked_at_ffmpeg_tts_limit.mp4");
}

#[test]
fn packets_of_chunked_ffmpeg_whole_tts() {
    demuxes_as_ffmpeg_does("chunked_ffmpeg_whole_tts.mp4");
}

#[test]
fn packets_of_chunked_past_ffmpeg_tts() {
    demuxes_as_ffmpeg_does("chunked_past_ffmpeg_tts.mp4");
}

#[test]
fn packets_of_chunked_total_wraps() {
    demuxes_as_ffmpeg_does("chunked_total_wraps.mp4");
}

#[test]
fn packets_of_chunked_misaligned_frames() {
    demuxes_as_ffmpeg_does("chunked_misaligned_frames.mp4");
}

#[test]
fn packets_of_stsc_count_negative() {
    demuxes_as_ffmpeg_does("stsc_count_negative.mp4");
}

#[test]
fn packets_of_edits_repeat() {
    demuxes_as_ffmpeg_does("edits_repeat.mp4");
}

/// An edit that never reached its start leaves a dropped frame's duration
/// behind; the next edit's dropped frame counts back by its own.
#[test]
fn packets_of_edits_stale_discards() {
    demuxes_as_ffmpeg_does("edits_stale_discards.mp4");
}

// Files the fuzzer found.

#[test]
fn packets_of_found_tx3g_claims_billions() {
    demuxes_as_ffmpeg_does("found_tx3g_claims_billions.mp4");
}

#[test]
fn packets_of_found_seek_over_discards() {
    demuxes_as_ffmpeg_does("found_seek_over_discards.mp4");
}

/// One seek's answer: where to, in milliseconds, and each packet after it
/// (its stream, pts, dts, flags and samples to skip).
type SeekAnswer = (i64, Vec<(usize, i64, i64, String, u32)>);

/// After a seek to each of the times `generate_fixtures.py` seeks to, the
/// first packets are FFmpeg's.
fn seeks_as_ffmpeg_does(name: &str) {
    let base = name.rsplit_once('.').unwrap().0;
    let path = data(&format!("{base}.seek.txt"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut seeks: Vec<SeekAnswer> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split(' ').collect();
        match w[0] {
            "seek" => seeks.push((w[1].parse().unwrap(), Vec::new())),
            "packet" => seeks.last_mut().unwrap().1.push((
                w[1].parse().unwrap(),
                w[2].parse().unwrap(),
                w[3].parse().unwrap(),
                w[4].to_owned(),
                w[5].parse().unwrap(),
            )),
            other => panic!("{path}: a line this test does not know: {other}"),
        }
    }
    for (ms, expected) in seeks {
        // A fresh demuxer, as each ffprobe run is one.
        let mut d = Demuxer::open(File::open(data(name)).unwrap()).unwrap();
        let track = default_track(&d);
        let scale = i64::from(d.tracks()[track].timescale);
        // FFmpeg's av_rescale(ts, den, AV_TIME_BASE): rounded to nearest.
        let ticks = (ms * 1000 * scale + 500_000) / 1_000_000;
        // ffprobe's seek goes backward from a time after 0 and forward from
        // 0 itself (avformat_seek_file's choice for its bounds).
        if ms > 0 {
            d.seek(track, ticks).unwrap();
        } else {
            d.seek_forward(track, ticks).unwrap();
        }
        let mut got = Vec::new();
        while got.len() < expected.len() {
            let Some(p) = d.next_packet().unwrap() else {
                break;
            };
            got.push((p.track, p.pts, p.dts, flags(&p), p.skip_samples));
        }
        assert_eq!(got, expected, "{name}: after a seek to {ms} ms");
    }
}

/// FFmpeg's default stream (`av_find_default_stream_index`): the video with
/// a size first, then sound with a rate, then the first.
fn default_track(d: &Demuxer<File>) -> usize {
    let score = |t: &mp4::Track| -> i32 {
        let mut s = 0;
        if t.kind == TrackKind::Video {
            // A size, which probing gives even a codec whose size FFmpeg's
            // demuxer leaves to the decoder (MPEG-4 Part 2).
            if t.video.is_some_and(|v| v.width != 0 && v.height != 0) {
                s += 50;
            }
            s += 25;
        }
        if t.kind == TrackKind::Audio && t.audio.is_some_and(|a| a.sample_rate != 0) {
            s += 50;
        }
        s
    };
    let mut best = (0, i32::MIN);
    for (i, t) in d.tracks().iter().enumerate() {
        let s = score(t);
        if s > best.1 {
            best = (i, s);
        }
    }
    best.0
}

#[test]
fn seeks_in_av1() {
    seeks_as_ffmpeg_does("av1.mp4");
}

#[test]
fn seeks_in_vp9_opus() {
    seeks_as_ffmpeg_does("vp9_opus.mp4");
}

#[test]
fn seeks_in_h264_bframes() {
    seeks_as_ffmpeg_does("h264_bframes.mp4");
}

#[test]
fn seeks_in_h264_negative_cts() {
    seeks_as_ffmpeg_does("h264_negative_cts.mp4");
}

#[test]
fn seeks_in_mpeg4_bframes() {
    seeks_as_ffmpeg_does("mpeg4_bframes.mp4");
}

#[test]
fn seeks_in_aac() {
    seeks_as_ffmpeg_does("aac.mp4");
}

#[test]
fn seeks_in_delayed_audio() {
    seeks_as_ffmpeg_does("delayed_audio.mp4");
}

#[test]
fn seeks_in_pcm() {
    seeks_as_ffmpeg_does("pcm.mov");
}

#[test]
fn seeks_in_two_edits() {
    seeks_as_ffmpeg_does("two_edits.mp4");
}

#[test]
fn seeks_in_empty_edit() {
    seeks_as_ffmpeg_does("empty_edit.mp4");
}

#[test]
fn seeks_in_edit_mid_gop() {
    seeks_as_ffmpeg_does("edit_mid_gop.mp4");
}

#[test]
fn seeks_in_sound_edit_mid_frame() {
    seeks_as_ffmpeg_does("sound_edit_mid_frame.mp4");
}

#[test]
fn seeks_in_stps() {
    seeks_as_ffmpeg_does("stps.mp4");
}

#[test]
fn seeks_in_rap_group() {
    seeks_as_ffmpeg_does("rap_group.mp4");
}

#[test]
fn seeks_in_ctts_tail() {
    seeks_as_ffmpeg_does("ctts_tail.mp4");
}

#[test]
fn seeks_in_edit_before_key_shows() {
    seeks_as_ffmpeg_does("edit_before_key_shows.mp4");
}

#[test]
fn seeks_in_edit_at_shown_key() {
    seeks_as_ffmpeg_does("edit_at_shown_key.mp4");
}

/// The fuzzer's file whose seeks walked a run of discarded entries once for
/// each entry of it: a seek back past the start -- the one that took a
/// third of a second -- answered at once.
///
/// Not held to ffprobe's seeks: its edits make an index far past one entry
/// a byte of the file, which the crate holds to that (design-decisions
/// §1364), and FFmpeg's seeks land in what is past it.
#[test]
fn seeks_in_found_seek_over_discards() {
    let mut d = Demuxer::open(File::open(data("found_seek_over_discards.mp4")).unwrap()).unwrap();
    for track in 0..d.tracks().len() {
        let started = std::time::Instant::now();
        d.seek(track, -1).unwrap();
        // FFmpeg's walks took seconds here in a test build; this takes a
        // millisecond, so a second's bound holds on any machine however
        // busy, and still fails the walks.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "track {track}: a seek back past the start took {:?}",
            started.elapsed()
        );
    }
}

/// Every packet of a demuxer, to the end.
fn every_packet<R: std::io::Read + std::io::Seek>(d: &mut Demuxer<R>) -> Vec<mp4::Packet> {
    let mut all = Vec::new();
    while let Some(p) = d.next_packet().unwrap() {
        all.push(p);
    }
    all
}

/// Where a track read alone goes on past where it ends among the others: a
/// track passed over has a sample past the end of the file, whose read ends
/// the reading of every track -- and which, unread, ends nothing, as FFmpeg
/// reads nothing of a stream it discards (`mov_read_packet`). Alone, the
/// track gives its packets among the others, then those after.
const READ_ON_PAST_ANOTHERS_END: &[(&str, usize)] = &[
    // The picture track's samples run past the file's end before the
    // sound's first turn: among the others the sound gives nothing.
    ("found_seek_over_discards.mp4", 1),
];

/// A track read alone (`Demuxer::select_tracks`) gives exactly the packets it
/// gives among the others, in every fixture the demuxer opens: the others'
/// samples, passed over unread, still take their turns. Read ahead little
/// -- as a reader of one track reads (`videocodec`'s sound and subtitles) --
/// or a few bytes at a time, a file reads the same. The one difference,
/// FFmpeg's too, is [`READ_ON_PAST_ANOTHERS_END`].
#[test]
fn a_track_selected_alone_gives_the_packets_it_gives_among_the_others() {
    let mut differ = Vec::new();
    let mut checked = 0;
    for entry in std::fs::read_dir(data("")).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if !(name.ends_with(".mp4") || name.ends_with(".mov")) {
            continue;
        }
        let Ok(mut d) = Demuxer::open(File::open(data(&name)).unwrap()) else {
            continue;
        };
        let all = every_packet(&mut d);
        let mut ahead = Demuxer::open(File::open(data(&name)).unwrap()).unwrap();
        ahead.set_read_ahead(7).unwrap();
        if every_packet(&mut ahead) != all {
            differ.push(format!("{name}: read 7 bytes ahead at a time"));
        }
        for track in 0..d.tracks().len() {
            let mut alone = Demuxer::open(File::open(data(&name)).unwrap()).unwrap();
            alone.select_tracks(Some(&[track]));
            alone.set_read_ahead(1024).unwrap();
            let got = every_packet(&mut alone);
            let want: Vec<_> = all.iter().filter(|p| p.track == track).cloned().collect();
            let reads_on = READ_ON_PAST_ANOTHERS_END.contains(&(name.as_str(), track));
            if reads_on && got.len() > want.len() && got.starts_with(&want) {
                checked += 1;
                continue;
            }
            if got != want {
                differ.push(format!(
                    "{name} track {track}: {} packets, {} wanted",
                    got.len(),
                    want.len()
                ));
            }
            checked += 1;
        }
    }
    assert!(
        differ.is_empty(),
        "{} of {checked} differ:\n{}",
        differ.len(),
        differ.join("\n")
    );
    assert!(checked > 60, "only {checked} tracks checked");
}

/// A source that counts the bytes read from it, and the reads.
struct Counted {
    inner: File,
    read: std::rc::Rc<std::cell::Cell<(u64, u64)>>,
}

impl std::io::Read for Counted {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        let (bytes, reads) = self.read.get();
        self.read.set((bytes + n as u64, reads + 1));
        Ok(n)
    }
}

impl std::io::Seek for Counted {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// A track read alone reads its own samples and nothing more: it gives its
/// own packets alone, and, read a byte ahead at a time, the bytes it reads
/// once open are the bytes of those packets -- the others' samples are not
/// read and let go.
#[test]
fn a_track_selected_alone_reads_its_own_samples_alone() {
    let mut differ = Vec::new();
    let mut checked = 0;
    for entry in std::fs::read_dir(data("")).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if !(name.ends_with(".mp4") || name.ends_with(".mov")) {
            continue;
        }
        let Ok(d) = Demuxer::open(File::open(data(&name)).unwrap()) else {
            continue;
        };
        if d.tracks().len() < 2 {
            continue;
        }
        for track in 0..d.tracks().len() {
            let read = std::rc::Rc::new(std::cell::Cell::new((0, 0)));
            let source = Counted {
                inner: File::open(data(&name)).unwrap(),
                read: read.clone(),
            };
            let mut alone = Demuxer::open(source).unwrap();
            alone.select_tracks(Some(&[track]));
            alone.set_read_ahead(1).unwrap();
            read.set((0, 0));
            let got = every_packet(&mut alone);
            if let Some(p) = got.iter().find(|p| p.track != track) {
                differ.push(format!(
                    "{name} track {track}: gave track {}'s packet",
                    p.track
                ));
            }
            let own: usize = got.iter().map(|p| p.data.len()).sum();
            let (bytes, _) = read.get();
            if bytes != own as u64 {
                differ.push(format!(
                    "{name} track {track}: {bytes} bytes read for {own}"
                ));
            }
            checked += 1;
        }
    }
    assert!(
        differ.is_empty(),
        "{} of {checked} differ:\n{}",
        differ.len(),
        differ.join("\n")
    );
    assert!(checked >= 16, "only {checked} tracks checked");
}

/// How much of a real film each track read alone reads, at each read-ahead:
/// `MP4_FILM=film.mp4 cargo test --release -p mp4 --test fixtures --
/// --ignored --nocapture film`.
#[test]
#[ignore = "a measurement, over a film of the caller's"]
fn film_read_track_by_track() {
    let path = std::env::var("MP4_FILM").expect("MP4_FILM names a film");
    let size = std::fs::metadata(&path).unwrap().len();
    let tracks = Demuxer::open(File::open(&path).unwrap())
        .unwrap()
        .tracks()
        .len();
    let selections = std::iter::once(None).chain((0..tracks).map(Some));
    for selection in selections {
        for ahead in [64 * 1024, 16 * 1024, 4096, 1024] {
            let read = std::rc::Rc::new(std::cell::Cell::new((0, 0)));
            let source = Counted {
                inner: File::open(&path).unwrap(),
                read: read.clone(),
            };
            let mut d = Demuxer::open(source).unwrap();
            d.set_read_ahead(ahead).unwrap();
            if let Some(n) = selection {
                d.select_tracks(Some(&[n]));
            }
            read.set((0, 0));
            let start = std::time::Instant::now();
            let mut packets = 0;
            while d.next_packet().unwrap().is_some() {
                packets += 1;
            }
            let (bytes, reads) = read.get();
            println!(
                "track {selection:?}, {ahead} ahead: {packets} packets, {bytes} of {size} bytes read ({:.1}%) in {reads} reads, {:?}",
                bytes as f64 * 100.0 / size as f64,
                start.elapsed()
            );
        }
    }
}
