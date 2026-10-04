//! Every fixture played through [`Video`], frame by frame, held to the
//! answers `tests/data/generate_fixtures.py` made without this crate: the
//! frames, times, durations and key frames ffprobe shows, and each frame's
//! pixels as libavif converts the planes ffmpeg's decoders make. That script
//! says where every answer comes from, and why each fixture is there.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]

use std::collections::HashMap;
use std::fs::File;
use std::io::Cursor;

use videocodec::{Frame, SeekMode, Video};

/// Every fixture, each a path the conversion or the file can take.
const FIXTURES: [&str; 15] = [
    "vp9_sd_untagged.webm",
    "vp9_hd_untagged.webm",
    "vp9_bt709_sd.webm",
    "vp9_full_range.webm",
    "vp9_10bit_bt2020.webm",
    "vp9_444.webm",
    "vp9_440.webm",
    "vp9_422_12bit.webm",
    "vp9_gbr.webm",
    "vp9_smpte240.webm",
    "vp9_alpha.webm",
    "av1_bt709.webm",
    "av1_mono.webm",
    "av1_444_10bit.webm",
    "vp9_cropped.mkv",
];

/// One frame as the answers give it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    time: i64,
    duration: u64,
    key: bool,
    size: String,
    md5: String,
}

fn path(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// A fixture's `info` line, as words, and its frames.
fn answers(name: &str) -> (HashMap<String, String>, Vec<Line>) {
    let base = name.rsplit_once('.').unwrap().0;
    let file = path(&format!("{base}.txt"));
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{file}: {e}"));
    let (mut info, mut frames) = (HashMap::new(), Vec::new());
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split(' ').collect();
        match w[0] {
            "info" => {
                for pair in &w[1..] {
                    let (k, v) = pair.split_once('=').unwrap();
                    info.insert(k.to_owned(), v.to_owned());
                }
            }
            "frame" => frames.push(Line {
                time: w[1].parse().unwrap(),
                duration: w[2].parse().unwrap(),
                key: w[3] == "K",
                size: w[4].to_owned(),
                md5: w[5].to_owned(),
            }),
            other => panic!("{file}: a line this test does not know: {other}"),
        }
    }
    (info, frames)
}

/// A frame as the answers write it: its pixels' MD5 over their bytes, B G R
/// A, as the words are in memory on a little-endian machine.
fn line(f: &Frame) -> Line {
    let bytes: Vec<u8> = f.pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
    Line {
        time: f.time,
        duration: f.duration,
        key: f.keyframe,
        size: format!("{}x{}", f.width, f.height),
        md5: md5::md5_hex(&bytes).to_string(),
    }
}

fn open(name: &str) -> Video<File> {
    Video::open(File::open(path(name)).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// What the file says of its video, and every frame -- its time, duration,
/// key-frame flag, size, and every pixel -- as ffprobe and libavif have it.
#[test]
fn every_fixture_plays_as_ffmpeg_and_libavif_show_it() {
    for name in FIXTURES {
        let (info, want) = answers(name);
        let mut video = open(name);
        let i = *video.info();
        let said = [
            ("track", i.track.to_string()),
            ("codec", i.codec.to_string().to_lowercase()),
            ("size", format!("{}x{}", i.width, i.height)),
            (
                "display",
                format!("{}x{}", i.display_width, i.display_height),
            ),
            (
                "frame_duration",
                i.frame_duration.map_or("none".into(), |d| d.to_string()),
            ),
            (
                "duration",
                i.duration.map_or("none".into(), |d| d.to_string()),
            ),
            ("alpha", u8::from(i.alpha).to_string()),
        ];
        for (key, value) in said {
            assert_eq!(info[key], value, "{name}: {key}");
        }
        let mut got = Vec::new();
        while let Some(frame) = video.next_frame().unwrap_or_else(|e| panic!("{name}: {e}")) {
            assert_eq!(frame.pixels.len(), (frame.width * frame.height) as usize);
            got.push(line(&frame));
        }
        assert_eq!(video.damaged(), 0, "{name}: {:?}", video.last_damage());
        for (k, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(g, w, "{name}: frame {k}");
        }
        assert_eq!(got.len(), want.len(), "{name}: the frames");
    }
}

/// A picture taken unconverted and converted later is the frame
/// `next_frame` gives -- crop and all.
#[test]
fn a_picture_converted_later_is_the_same_frame() {
    for name in ["vp9_cropped.mkv", "av1_444_10bit.webm", "vp9_alpha.webm"] {
        let (_, want) = answers(name);
        let mut video = open(name);
        let mut k = 0;
        while let Some(picture) = video.next_picture().unwrap() {
            let frame = video.convert(&picture).unwrap();
            assert_eq!(line(&frame), want[k], "{name}: frame {k}");
            k += 1;
        }
        assert_eq!(k, want.len());
    }
}

/// An exact seek gives the frame showing at the time sought -- the latest
/// at or before it, or the first for a time before them all -- and then the
/// frames after it; a key-frame seek gives the latest key frame at or before
/// the time, and the frames after that.
#[test]
fn every_seek_lands_on_the_frame_showing_then() {
    for name in FIXTURES {
        let (_, want) = answers(name);
        let mut video = open(name);
        let last = want.last().unwrap().time;
        for target in [
            -5_000_000,
            0,
            1,
            39_999_999,
            40_000_000,
            130_000_000,
            200_000_000,
            last,
            last + 1_000_000_000,
        ] {
            video.seek(target, SeekMode::Exact).unwrap();
            let k = want.iter().rposition(|l| l.time <= target).unwrap_or(0);
            for (n, expected) in want[k..].iter().take(3).enumerate() {
                let frame = video.next_frame().unwrap().expect("a frame");
                assert_eq!(
                    &line(&frame),
                    expected,
                    "{name}: frame {n} after an exact seek to {target}"
                );
            }
            if k + 3 > want.len() {
                assert!(
                    video.next_frame().unwrap().is_none(),
                    "{name}: past the end"
                );
            }

            video.seek(target, SeekMode::KeyFrame).unwrap();
            let first = line(&video.next_frame().unwrap().expect("a frame"));
            let at = want
                .iter()
                .position(|l| *l == first)
                .unwrap_or_else(|| panic!("{name}: a seek to {target} gave {first:?}"));
            assert!(
                want[at].key,
                "{name}: a seek to {target} landed on a frame not key"
            );
            let key_before = want
                .iter()
                .rposition(|l| l.key && l.time <= target)
                .unwrap_or(0);
            assert_eq!(at, key_before, "{name}: the key frame before {target}");
            for expected in want[at + 1..].iter().take(2) {
                assert_eq!(
                    &line(&video.next_frame().unwrap().expect("a frame")),
                    expected
                );
            }
        }
    }
}

/// Every byte of the headers inverted in turn, bytes through the rest every
/// so often, and the file cut short at many lengths: each plays what it can
/// and ends, or is refused -- never a panic, never more frames than the file
/// holds, and a seek in each works or is refused.
#[test]
fn a_damaged_file_plays_what_it_can_and_never_panics() {
    for name in [
        "vp9_alpha.webm",
        "av1_444_10bit.webm",
        "vp9_cropped.mkv",
        "vp9_440.webm",
    ] {
        let bytes = std::fs::read(path(name)).unwrap();
        let play = |data: &[u8]| {
            let Ok(mut video) = Video::open(Cursor::new(data.to_vec())) else {
                return;
            };
            let mut frames = 0;
            while let Ok(Some(_)) = video.next_frame() {
                frames += 1;
                assert!(frames <= 16, "{name}: frames from nowhere");
            }
            if video.seek(100_000_000, SeekMode::Exact).is_ok() {
                while let Ok(Some(_)) = video.next_frame() {
                    frames += 1;
                    assert!(frames <= 32, "{name}: frames from nowhere after a seek");
                }
            }
        };
        let mut damaged = bytes.clone();
        let headers = 600.min(bytes.len());
        let positions = (0..headers).chain((headers..bytes.len()).step_by(61));
        for at in positions {
            damaged[at] ^= 0xff;
            play(&damaged);
            damaged[at] ^= 0xff;
        }
        for len in (0..bytes.len()).step_by(37) {
            play(&bytes[..len]);
        }
    }
}

/// A player decodes on a thread of its own: everything it holds moves to one.
#[test]
fn a_video_moves_to_another_thread() {
    fn movable<T: Send>() {}
    movable::<Video<File>>();
    movable::<videocodec::Picture>();
    movable::<videocodec::Decoder>();
    movable::<Frame>();
    let mut video = open("av1_bt709.webm");
    let frame = std::thread::spawn(move || video.next_frame().unwrap().unwrap())
        .join()
        .unwrap();
    assert_eq!(line(&frame), answers("av1_bt709.webm").1[0]);
}

/// A file that is not a video is refused, and so is a track the file does
/// not have.
#[test]
fn a_file_without_the_video_asked_for_is_refused() {
    assert!(matches!(
        Video::open(Cursor::new(b"not a video".to_vec())),
        Err(videocodec::Error::Container(_))
    ));
    let file = File::open(path("vp9_sd_untagged.webm")).unwrap();
    assert!(matches!(
        Video::open_track(file, 2),
        Err(videocodec::Error::NoVideo)
    ));
    let file = File::open(path("vp9_sd_untagged.webm")).unwrap();
    assert_eq!(Video::open_track(file, 1).unwrap().info().track, 1);
}
