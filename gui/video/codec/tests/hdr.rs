//! What each video says of its light, held to ffprobe, frame by frame: its
//! colour -- matrix, primaries, transfer and range -- and its HDR metadata,
//! the mastering display and the content light level, the bitstream's kind
//! by kind else the file's, in Matroska, WebM and MP4, from AV1's metadata
//! OBUs and VP9's and AV1's containers (`tests/data/generate_hdr_fixtures.py`).
//!
//! And how each is shown: its first frame held to Chrome's pixels for it
//! (`NAME.chrome.png`, from `tests/data/chrome_hdr.py`), within one.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test: a failure should be loud"
)]

use std::fs::File;

use videocodec::{Light, Picture, Video};

const FIXTURES: [&str; 9] = [
    "hdr10_av1.mkv",
    "cll_av1.mkv",
    "tagged_by_file_av1.mkv",
    "hdr10_vp9.webm",
    "hdr10_vp9.mp4",
    "hdr10_av1.mp4",
    "hlg_vp9.webm",
    "hlg_av1.mp4",
    "partial_vp9.webm",
];

fn path(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// ffprobe's rational `num/den`.
fn rational(s: &str) -> f64 {
    let (n, d) = s.split_once('/').unwrap();
    n.parse::<f64>().unwrap() / d.parse::<f64>().unwrap()
}

/// `got` against ffprobe's rationals: exact for MP4's and AV1's, whose
/// numbers are fractions of their scale, and a hair off for Matroska's
/// floats, which FFmpeg turns into the nearest fraction of terms under 2^31.
fn close(got: &[f64], want: &str, what: &str) {
    let want: Vec<f64> = want.split(',').map(rational).collect();
    assert_eq!(got.len(), want.len(), "{what}");
    for (g, w) in got.iter().zip(&want) {
        assert!(
            (g - w).abs() <= 1e-9 * w.abs().max(1.0),
            "{what}: {g} against {w}"
        );
    }
}

/// One frame's answer: its colour line, and its light.
struct Answer {
    colour: String,
    mastering: Option<String>,
    light: Option<String>,
}

fn answers(name: &str) -> Vec<Answer> {
    let text = std::fs::read_to_string(path(&format!("{name}.colour"))).unwrap();
    let mut out: Vec<Answer> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(rest) = line.strip_prefix("frame ") {
            let (_, colour) = rest.split_once(' ').unwrap();
            out.push(Answer {
                colour: colour.to_owned(),
                mastering: None,
                light: None,
            });
        } else if let Some(m) = line.strip_prefix("  mastering ") {
            out.last_mut().unwrap().mastering = Some(m.to_owned());
        } else if let Some(l) = line.strip_prefix("  light ") {
            out.last_mut().unwrap().light = Some(l.to_owned());
        } else {
            panic!("{name}: a line the test does not know: {line}");
        }
    }
    out
}

/// The picture's colour, as the generator writes ffprobe's.
fn colour(p: &Picture) -> String {
    let c = p.colour();
    let range = if c.full_range { "full" } else { "limited" };
    format!(
        "matrix={} primaries={} transfer={} range={range}",
        c.matrix, c.primaries, c.transfer
    )
}

/// `light` against the frame's answer.
fn check_light(light: Light, a: &Answer, what: &str) {
    match (&a.mastering, light.mastering) {
        (None, None) => {}
        (Some(want), Some(m)) => {
            for field in want.split(' ') {
                let (key, value) = field.split_once('=').unwrap();
                match key {
                    "primaries" => {
                        let c = m.chromaticities.unwrap_or_else(|| {
                            panic!("{what}: no chromaticities, ffprobe's {value}")
                        });
                        close(
                            &[
                                c.red[0], c.red[1], c.green[0], c.green[1], c.blue[0], c.blue[1],
                                c.white[0], c.white[1],
                            ],
                            value,
                            what,
                        );
                    }
                    "luminance" => {
                        let l = m
                            .luminance
                            .unwrap_or_else(|| panic!("{what}: no luminance, ffprobe's {value}"));
                        close(&[l.max, l.min], value, what);
                    }
                    _ => panic!("{what}: a field the test does not know: {key}"),
                }
            }
            // Each half said by ffprobe only where FFmpeg has it.
            assert_eq!(
                m.chromaticities.is_some(),
                want.contains("primaries="),
                "{what}"
            );
            assert_eq!(m.luminance.is_some(), want.contains("luminance="), "{what}");
        }
        (want, got) => panic!("{what}: ffprobe's mastering display {want:?}, ours {got:?}"),
    }
    let ours = light
        .content
        .map(|c| format!("{},{}", c.max_cll, c.max_fall));
    assert_eq!(ours, a.light, "{what}: the content light level");
}

/// Each first frame is Chrome's -- the light the frame says it holds tone
/// mapped as Chrome maps it, BT.2020 carried to sRGB -- within one in every
/// channel, and one apart in under one channel in a thousand: the reference
/// computes in double precision, the conversion in single, as Chrome's own
/// does. The fixtures between them take the light from the bitstream over
/// the file (`hdr10_av1.mkv`, whose file says 2000), a level beside a file's
/// mastering display (`cll_av1.mkv`, whose display says 4000), a display's
/// peak alone (`partial_vp9.webm`, 600), no light at all
/// (`tagged_by_file_av1.mkv`, 1000), and HLG.
#[test]
fn every_first_frame_is_chrome_s() {
    for name in FIXTURES {
        let mut video =
            Video::open(File::open(path(name)).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let frame = video
            .next_picture()
            .unwrap()
            .unwrap_or_else(|| panic!("{name}: no frame"))
            .to_frame()
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let png = std::fs::read(path(&format!("{name}.chrome.png"))).unwrap();
        let chrome = imagecodec::decode(&png, imagecodec::Limits::default()).unwrap();
        assert_eq!(
            (frame.width, frame.height),
            (chrome.width, chrome.height),
            "{name}"
        );
        let (mut off, mut worst) = (0usize, 0u32);
        for (i, (&ours, &theirs)) in frame.pixels.iter().zip(&chrome.pixels).enumerate() {
            assert_eq!(ours >> 24, 0xff, "{name}: pixel {i} is not opaque");
            for shift in [16, 8, 0] {
                let d = ((ours >> shift) & 0xff).abs_diff((theirs >> shift) & 0xff);
                worst = worst.max(d);
                off += usize::from(d != 0);
            }
        }
        let channels = frame.pixels.len() * 3;
        assert!(worst <= 1, "{name}: a channel {worst} from Chrome's");
        assert!(
            off * 1000 < channels,
            "{name}: {off} of {channels} channels one from Chrome's"
        );
    }
}

#[test]
fn every_frame_says_its_light_as_ffmpeg_reads_it() {
    for name in FIXTURES {
        let want = answers(name);
        let mut video =
            Video::open(File::open(path(name)).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut seen = 0;
        while let Some(picture) = video.next_picture().unwrap() {
            let what = format!("{name} frame {seen}");
            let a = want
                .get(seen)
                .unwrap_or_else(|| panic!("{what}: more frames than ffprobe"));
            assert_eq!(colour(&picture), a.colour, "{what}");
            check_light(picture.light(), a, &what);
            seen += 1;
        }
        assert_eq!(seen, want.len(), "{name}: the frames");
    }
}
