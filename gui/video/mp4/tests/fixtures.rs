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

/// One stream as `ffprobe` printed it: its codec tag, time base, and its
/// picture's size (`WxH`) or its sound's rate and channels (`RHz/C`).
type Stream = (String, u64, u64, String);

/// A fixture's answers: each stream, and the packets.
fn answers(name: &str) -> (Vec<Stream>, Vec<Line>) {
    let base = name.rsplit_once('.').unwrap().0;
    let path = data(&format!("{base}.txt"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let (mut streams, mut packets) = (Vec::new(), Vec::new());
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split(' ').collect();
        match w[0] {
            "stream" => {
                let (num, den) = w[3].split_once('/').unwrap();
                streams.push((
                    w[2].to_owned(),
                    num.parse().unwrap(),
                    den.parse().unwrap(),
                    w[4].to_owned(),
                ));
            }
            "packet" => packets.push(Line {
                stream: w[1].parse().unwrap(),
                pts: w[2].parse().unwrap(),
                dts: w[3].parse().unwrap(),
                duration: w[4].parse().unwrap(),
                size: w[5].parse().unwrap(),
                pos: w[6].parse().unwrap(),
                flags: w[7].to_owned(),
                md5: w[8].to_owned(),
                skip: w[9].parse().unwrap(),
            }),
            other => panic!("{path}: a line this test does not know: {other}"),
        }
    }
    (streams, packets)
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

/// The fixture's streams and every packet -- the stream, times, duration,
/// size, position, flags, bytes and samples to skip -- as FFmpeg's demuxer
/// gives them.
fn demuxes_as_ffmpeg_does(name: &str) {
    let mut d =
        Demuxer::open(File::open(data(name)).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let (streams, expected) = answers(name);
    assert_eq!(d.tracks().len(), streams.len(), "{name}: the streams");
    for (t, (tag, num, den, shape)) in d.tracks().iter().zip(&streams) {
        let ours = String::from_utf8_lossy(&t.codec_tag).into_owned();
        assert_eq!(&ours, tag, "{name}: track {}'s codec tag", t.id);
        let our_shape = match (t.video, t.audio) {
            (Some(v), _) => format!("{}x{}", v.width, v.height),
            (_, Some(a)) => format!("{}Hz/{}", a.sample_rate, a.channels),
            _ => "-".to_owned(),
        };
        assert_eq!(
            &our_shape, shape,
            "{name}: track {}'s picture or sound",
            t.id
        );
        assert_eq!(
            (1, u64::from(t.timescale)),
            (*num, *den),
            "{name}: track {}'s time base",
            t.id
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
    for (i, (g, e)) in got.iter().zip(&expected).enumerate() {
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
fn packets_of_hoov() {
    demuxes_as_ffmpeg_does("hoov.mp4");
}

#[test]
fn packets_of_truncated() {
    demuxes_as_ffmpeg_does("truncated.mp4");
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
