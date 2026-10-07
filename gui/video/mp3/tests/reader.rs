//! The reader held to FFmpeg: every fixture FFmpeg opens gives ffprobe's
//! packets -- times, durations, sizes, positions, and the skips and
//! padding FFmpeg gives them as side data -- and ffprobe's stream start and
//! duration (`tests/data/NAME.packets`, from `generate_fixtures.py`).
//! Those FFmpeg does not open -- free format, a file of one frame -- give
//! minimp3's frames. Seeks land on a packet the read-through had, early
//! enough, and go on as it went; and the probe tells MPEG audio from
//! other files.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a panic is a failed test"
)]

use std::fs::File;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use mp3::{Packet, Reader, TICKS_PER_SECOND};

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

/// The fixture `name`'s stream file.
fn stream(name: &str) -> PathBuf {
    ["mp3", "mp2", "mp1"]
        .iter()
        .map(|ext| data_dir().join(format!("{name}.{ext}")))
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("no stream for {name}"))
}

fn packets(path: &Path) -> (Reader<File>, Vec<Packet>) {
    let mut reader = Reader::open(File::open(path).unwrap()).unwrap();
    let mut out = Vec::new();
    let first = reader.next_packet().unwrap();
    out.extend(first);
    while let Some(p) = reader.next_packet().unwrap() {
        out.push(p);
    }
    (reader, out)
}

/// Files without a frame count whose bit rate varies: FFmpeg estimates their
/// duration at its parser's average over the frames it happened to read
/// while finding the stream's parameters, the reader at the first frame's
/// rate. Their packets are held to ffprobe's all the same.
const ESTIMATED_VBR: [&str; 2] = ["lame_vbr_mono_32000", "damaged_cut"];

#[test]
fn every_file_ffmpeg_opens_gives_ffprobes_packets() {
    let mut checked = 0;
    for entry in std::fs::read_dir(data_dir()).unwrap() {
        let answer = entry.unwrap().path();
        if answer.extension().is_none_or(|e| e != "packets") {
            continue;
        }
        let name = answer.file_stem().unwrap().to_str().unwrap().to_owned();
        let expected = std::fs::read_to_string(&answer).unwrap();
        let mut expected = expected.lines();
        let (reader, got) = packets(&stream(&name));
        let info = reader.info();
        let ticks = info
            .first
            .map_or(1, |h| TICKS_PER_SECOND / u64::from(h.sample_rate));
        let duration = info.duration.map_or("-".to_owned(), |d| d.to_string());
        let stream_line = format!("stream {} {duration}", info.start_skip * ticks);
        let want = expected.next().unwrap();
        if !ESTIMATED_VBR.contains(&name.as_str()) {
            assert_eq!(stream_line, want, "{name}: the stream's start and duration");
        }
        let mut n = 0;
        for (k, p) in got.iter().enumerate() {
            let line = format!(
                "packet {} {} {} {} {} {}",
                p.pts,
                p.duration,
                p.data.len(),
                p.position,
                p.skip_samples,
                p.discard_padding
            );
            assert_eq!(Some(line.as_str()), expected.next(), "{name}: packet {k}");
            n += 1;
        }
        assert_eq!(
            expected.next(),
            None,
            "{name}: ffprobe has more than {n} packets"
        );
        checked += 1;
    }
    assert!(checked >= 45, "{checked} fixtures with ffprobe's packets");
}

/// The fixtures FFmpeg does not open give minimp3's frames: where each
/// starts and how long it is.
#[test]
fn files_ffmpeg_refuses_give_minimp3s_frames() {
    for name in [
        "lame_freeformat_44100",
        "lame_freeformat_mpeg2_24000",
        "layer1_free_48000",
        "one_frame",
    ] {
        assert!(
            !data_dir().join(format!("{name}.packets")).exists(),
            "{name}: FFmpeg opens it now"
        );
        let (reader, got) = packets(&stream(name));
        let frames: Vec<(u64, usize)> = got.iter().map(|p| (p.position, p.data.len())).collect();
        let answer = std::fs::read_to_string(data_dir().join(format!("{name}.txt"))).unwrap();
        let minimp3: Vec<(u64, usize)> = answer
            .lines()
            .filter(|l| l.starts_with("frame "))
            .map(|l| {
                let f: Vec<&str> = l.split(' ').collect();
                (f[1].parse().unwrap(), f[2].parse().unwrap())
            })
            .collect();
        assert_eq!(frames, minimp3, "{name}");
        assert!(
            got.iter().all(|p| p.frame == Some(0)),
            "{name}: every packet is a whole frame"
        );
        assert_eq!(
            reader.info().free_format.is_some(),
            name != "one_frame",
            "{name}"
        );
    }
}

#[test]
fn a_frame_is_where_its_packet_ends() {
    // With junk before a frame, the frame is the packet's end; at the end of
    // a file cut short, what is left holds no whole frame.
    let (_, got) = packets(&stream("damaged_garbage"));
    let glued: Vec<&Packet> = got
        .iter()
        .filter(|p| p.frame.is_some_and(|f| f > 0))
        .collect();
    assert!(
        !glued.is_empty(),
        "the garbage is taken with the frames after it"
    );
    for p in &glued {
        let frame = p.frame_bytes().unwrap();
        assert_eq!(frame[0], 0xff);
    }
    let (_, got) = packets(&stream("damaged_truncated"));
    let last = got.last().unwrap();
    assert_eq!(last.frame, None, "the cut-short last frame is not a frame");
}

/// A seek to a time lands on a packet the read-through had, at or before
/// the time and far enough before it for the bit reservoir, and the packets
/// go on from it as the read-through's did.
#[test]
fn seeks_land_early_enough_and_go_on_as_reading_through_did() {
    for name in [
        "lame_tagged_44100",
        "lame_vbr_tagged_44100",
        "lame_mpeg25_8000_mono",
        "mp2_192_stereo_48000",
        "damaged_garbage",
        "lame_freeformat_44100",
    ] {
        let (_, all) = packets(&stream(name));
        let mut reader = Reader::open(File::open(stream(name)).unwrap()).unwrap();
        let end = all.last().unwrap().pts + all.last().unwrap().duration;
        for step in 0..=10 {
            let target = end * step / 10;
            reader.seek(target).unwrap();
            let first = reader.next_packet().unwrap();
            let Some(first) = first else {
                panic!("{name}: nothing after a seek to {target}");
            };
            let at = all
                .iter()
                .position(|p| p.position == first.position)
                .unwrap_or_else(|| panic!("{name}: a seek to {target} landed between packets"));
            assert!(first.pts <= target.max(0), "{name}: landed after {target}");
            // The packet holding the time, and how far before it the seek
            // landed: past at least 511 bytes of the frames before the one
            // before it, or at the start.
            let holding = all.iter().rposition(|p| p.pts <= target).unwrap_or(0);
            let before: usize = all[at..holding.saturating_sub(1)]
                .iter()
                .map(|p| p.data.len().saturating_sub(38))
                .sum();
            assert!(
                at == 0 || before >= 511,
                "{name}: a seek to {target} landed {before} bytes back"
            );
            // On as the read-through went.
            assert_eq!(
                first, all[at],
                "{name}: the packet a seek to {target} landed on"
            );
            for (k, want) in all[at + 1..].iter().take(30).enumerate() {
                let got = reader.next_packet().unwrap();
                assert_eq!(
                    got.as_ref(),
                    Some(want),
                    "{name}: packet {k} after a seek to {target}"
                );
            }
        }
    }
}

#[test]
fn the_probe_knows_mpeg_audio_from_other_files() {
    let mut count = 0;
    for entry in std::fs::read_dir(data_dir()).unwrap() {
        let path = entry.unwrap().path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !["mp1", "mp2", "mp3"].contains(&ext) {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
        // A file of one frame scores too low for FFmpeg's probe to take; the
        // free-format ones are not MPEG audio to it at all.
        if [
            "one_frame",
            "lame_freeformat_44100",
            "lame_freeformat_mpeg2_24000",
            "layer1_free_48000",
            "damaged_freeformat_cut",
        ]
        .contains(&name.as_str())
        {
            continue;
        }
        assert!(
            mp3::is_mpeg_audio(&mut File::open(&path).unwrap()).unwrap(),
            "{name}"
        );
        count += 1;
    }
    assert!(count >= 40);
    // Other formats, and noise.
    let video = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for other in [
        "flac/tests/data/s12_stereo.flac",
        "flac/tests/data/id3v2_in_front.flac",
        "ogg/tests/data/vorbis_51.ogg",
        "ogg/tests/data/opus_51.opus",
    ] {
        let path = video.join(other);
        assert!(
            !mp3::is_mpeg_audio(&mut File::open(&path).unwrap()).unwrap(),
            "{other}"
        );
    }
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let noise: Vec<u8> = (0..300_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect();
    assert!(
        !mp3::is_mpeg_audio(&mut Cursor::new(&noise)).unwrap(),
        "noise"
    );
    assert!(
        !mp3::is_mpeg_audio(&mut Cursor::new(vec![0u8; 100_000])).unwrap(),
        "silence"
    );
    assert!(
        !mp3::is_mpeg_audio(&mut Cursor::new(Vec::<u8>::new())).unwrap(),
        "nothing"
    );
}
