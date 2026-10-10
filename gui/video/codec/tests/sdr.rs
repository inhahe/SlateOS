//! Ordinary (SDR) video whose colour Chrome converts, held to Chrome's own
//! pixels (design-decisions §1381): each fixture's first frame against the
//! screenshot of it in headless Chrome 154 (`NAME.chrome.png`, from
//! `tests/data/generate_sdr_fixtures.py`).
//!
//! Between them the fixtures take the colour from the place Chrome's decoder
//! asks first -- libvpx's the file, dav1d's the bitstream -- and from VP9's
//! one colour-space field, which Chrome reads as a colour whole; they
//! convert BT.601's two primaries, BT.2020's, Display P3's, SMPTE 240M's and
//! a power of 2.2; and one fixture, whose file says its primaries alone,
//! takes VP9's BT.709 whole and is not converted.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]

use std::fs::File;

use videocodec::Video;

/// Each fixture, and whether Chrome converts its colour.
const FIXTURES: [(&str, bool); 12] = [
    ("sdr_vp9_bt601.webm", true),
    ("sdr_vp9_bt709_file_bt601.webm", true),
    ("sdr_vp9_bt709_file_pieces.webm", false),
    ("sdr_file_p3.webm", true),
    ("sdr_file_bt2020.webm", true),
    ("sdr_file_gamma22.webm", true),
    ("sdr_av1_bt601_file_bt709.webm", true),
    ("sdr_av1_file_bt601.webm", true),
    ("vp9_full_range.webm", true),
    ("vp9_444.webm", true),
    ("vp9_smpte240.webm", true),
    ("vp9_colr.mp4", true),
];

/// How many in a hundred channels of a converted frame are exactly
/// Chrome's: Chrome's own arithmetic rounds some values within a twentieth
/// of a level of one half the other way (`yuv`'s `chrome_sdr.py`). Measured
/// 98.4% (BT.601's patches, the fewest) to 99.4% (`vp9_colr.mp4`), 4:2:0
/// pictures among them.
const EXACT_PERCENT: usize = 98;

fn path(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Each first frame is Chrome's within one in every channel, and nearly all
/// of it exactly -- where Chrome converts its colour. The one Chrome does
/// not convert takes the ordinary conversion, libyuv's fixed point
/// (design-decisions §1346), which is not Chrome's floating point to the
/// level: libyuv's blue for BT.709's studio range is 2 times Cb where
/// BT.709's is 2.112 (libyuv caps the weight, as libavif builds it), so a
/// strongly saturated blue or yellow is up to 15 levels from Chrome's
/// (`known-issues/F-ordinary-video-is-libyuv-s-arithmetic-not-chrome-s.md`).
/// Still far from a conversion of its primaries, which moves saturated
/// colours by tens of levels in every channel: its red and green stay
/// within two of Chrome's.
#[test]
fn sdr_first_frames_are_chrome_s() {
    let mut report = String::new();
    let mut short = false;
    for (name, converted) in FIXTURES {
        let mut video =
            Video::open(File::open(path(name)).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let picture = video
            .next_picture()
            .unwrap()
            .unwrap_or_else(|| panic!("{name}: no frame"));
        assert_eq!(
            picture.colour().converted(),
            converted,
            "{name}: {:?}",
            picture.colour()
        );
        let frame = picture.to_frame().unwrap_or_else(|e| panic!("{name}: {e}"));
        let png = std::fs::read(path(&format!("{name}.chrome.png"))).unwrap();
        let chrome = imagecodec::decode(&png, imagecodec::Limits::default()).unwrap();
        assert_eq!(
            (frame.width, frame.height),
            (chrome.width, chrome.height),
            "{name}"
        );
        let (mut exact, mut worst, mut worst_red_green) = (0usize, 0u32, 0u32);
        for (i, (&ours, &theirs)) in frame.pixels.iter().zip(&chrome.pixels).enumerate() {
            assert_eq!(ours >> 24, 0xff, "{name}: pixel {i} is not opaque");
            for shift in [16, 8, 0] {
                let d = ((ours >> shift) & 0xff).abs_diff((theirs >> shift) & 0xff);
                worst = worst.max(d);
                if shift != 0 {
                    worst_red_green = worst_red_green.max(d);
                }
                exact += usize::from(d == 0);
            }
        }
        let channels = frame.pixels.len() * 3;
        report += &format!("\n  {name}: {exact} of {channels} exact, worst {worst}");
        if converted {
            assert!(worst <= 1, "{name}: a channel {worst} from Chrome's");
            short |= exact * 100 < channels * EXACT_PERCENT;
        } else {
            assert!(worst <= 15, "{name}: a channel {worst} from Chrome's");
            assert!(
                worst_red_green <= 2,
                "{name}: red or green {worst_red_green} from Chrome's"
            );
        }
    }
    assert!(!short, "under {EXACT_PERCENT}% exactly Chrome's:{report}");
}

/// VP9's BT.2020 is read whole with the curve of its depth -- BT.2020's own
/// at 10 bits -- as Chrome reads it; the file around this one leaves its
/// transfer unsaid, so VP9's word stands whole.
#[test]
fn vp9_s_bt2020_takes_its_curve_from_its_depth() {
    let name = "vp9_10bit_bt2020.webm";
    let mut video =
        Video::open(File::open(path(name)).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let c = video.next_picture().unwrap().unwrap().colour();
    assert_eq!(
        (c.matrix, c.primaries, c.transfer, c.full_range, c.whole),
        (9, 9, 14, false, true)
    );
}
