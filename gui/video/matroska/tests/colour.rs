//! A video track's `Colour`, held to `ffprobe`: its transfer, and the HDR
//! metadata FFmpeg takes of it -- the content light level and the mastering
//! display -- with FFmpeg's rules for which of it to take
//! (`tests/data/generate_fixtures.py`, `colours()`).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "a test: a failure should be loud"
)]

use std::fs::File;

use matroska::{Colour, Demuxer, TrackKind};

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn colour(name: &str) -> Colour {
    let d = Demuxer::open(File::open(data(name)).unwrap()).unwrap();
    d.tracks()
        .iter()
        .find(|t| t.kind == TrackKind::Video)
        .and_then(|t| t.video)
        .and_then(|v| v.colour)
        .unwrap()
}

/// ffprobe's rational `num/den`.
fn rational(s: &str) -> f64 {
    let (n, d) = s.split_once('/').unwrap();
    n.parse::<f64>().unwrap() / d.parse::<f64>().unwrap()
}

/// `got` against ffprobe's rationals: FFmpeg turns each float into the
/// nearest fraction of terms under 2^31 (`av_d2q`), a hair off a float with
/// no such fraction.
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

/// FFmpeg's name for each transfer the fixtures use; "unknown" where it
/// leaves the transfer unsaid -- 2, and the reserved 0 and 3.
fn transfer_name(transfer: u64) -> &'static str {
    match transfer {
        1 => "bt709",
        16 => "smpte2084",
        18 => "arib-std-b67",
        0 | 2 | 3 => "unknown",
        _ => panic!("a transfer the fixtures do not use: {transfer}"),
    }
}

#[test]
fn every_colour_is_read_as_ffmpeg_reads_it() {
    let text = std::fs::read_to_string(data("colours.txt")).unwrap();
    let mut seen = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let mut words = line.split(' ');
        let name = words.next().unwrap();
        let c = colour(name);
        for word in words {
            let (key, want) = word.split_once('=').unwrap();
            let what = format!("{name} {key}");
            match key {
                "transfer" => {
                    assert_eq!(transfer_name(c.transfer_characteristics), want, "{what}");
                }
                "light" => {
                    let got = c.content_light.map_or("none".to_owned(), |l| {
                        format!("{},{}", l.max_cll, l.max_fall)
                    });
                    assert_eq!(got, want, "{what}");
                }
                "chromaticities" => match c.mastering.and_then(|m| m.chromaticities) {
                    None => assert_eq!(want, "none", "{what}"),
                    Some(p) => close(
                        &[
                            p.red[0], p.red[1], p.green[0], p.green[1], p.blue[0], p.blue[1],
                            p.white[0], p.white[1],
                        ],
                        want,
                        &what,
                    ),
                },
                "luminance" => match c.mastering.and_then(|m| m.luminance) {
                    None => assert_eq!(want, "none", "{what}"),
                    Some(l) => close(&[l.max, l.min], want, &what),
                },
                _ => panic!("{what}: a key the test does not know"),
            }
        }
        seen += 1;
    }
    assert!(seen >= 16, "{seen} colours");
}

/// The numbers FFmpeg takes it by are kept as the file gives them: a
/// reserved transfer as 3, and every other code point as written.
#[test]
fn a_colour_keeps_its_numbers() {
    assert_eq!(
        colour("colour_transfer_reserved.mkv").transfer_characteristics,
        3
    );
    let c = colour("colour_hdr10.mkv");
    assert_eq!(
        (
            c.matrix_coefficients,
            c.range,
            c.transfer_characteristics,
            c.primaries
        ),
        (9, 1, 16, 9)
    );
    // Of two, the first.
    let c = colour("colour_twice.mkv");
    assert_eq!((c.matrix_coefficients, c.transfer_characteristics), (9, 16));
    // A Colour without them: unspecified.
    let c = colour("colour_cll_alone.mkv");
    assert_eq!((c.matrix_coefficients, c.primaries, c.range), (2, 2, 0));
}
