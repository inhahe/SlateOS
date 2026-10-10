//! SDR AVIF whose colour Chrome converts -- BT.2020's primaries, Display
//! P3's, BT.601's, a power of 2.2, and BT.2020 with its curve unspecified
//! (MIAF's default: sRGB's) -- held to Chrome's own pixels for it: each
//! `avifsdr_*.avif` against `avifsdr_*.chrome.png`, a screenshot of headless
//! Chrome 154 showing the picture on an sRGB screen
//! (`tests/data/generate_avif_sdr.py`; design-decisions §1381).
//!
//! Each is 512 patches of one fixed pseudo-random Y'CbCr triple each,
//! lossless 4:4:4. Chrome converts an AVIF's planes on the GPU in floating
//! point, as it does video, and so does `yuv::managed`: every channel is
//! within one of Chrome's, and all but a percent or two exactly -- Chrome's
//! own arithmetic rounds some values within a twentieth of a level of one
//! half the other way.
//!
//! And one, `avifsdr_bt709.avif`, that Chrome does not convert: libavif's
//! own conversion, whose blue weight for BT.709's studio range libyuv caps
//! at 2 where BT.709 says 2.112 -- up to 15 levels from Chrome's floating
//! point in saturated blues and yellows
//! (`known-issues/F-ordinary-video-is-libyuv-s-arithmetic-not-chrome-s.md`),
//! its red and green within two.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]

use imagecodec::{Limits, decode};

/// How many in a hundred channels of a converted picture are exactly
/// Chrome's: measured 98.4% (BT.601, the fewest) to 99.1% (BT.2020) --
/// the very counts `gui/video/codec`'s fixtures of the same codes give.
const EXACT_PERCENT: usize = 98;

fn read(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// `name`'s pixels and Chrome's: how many channels are exact, the worst
/// difference, the worst in red and green, and how many channels.
fn against_chrome(name: &str) -> (usize, u32, u32, usize) {
    let ours = decode(&read(&format!("{name}.avif")), Limits::default())
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    let chrome = decode(&read(&format!("{name}.chrome.png")), Limits::default()).unwrap();
    assert_eq!(
        (ours.width, ours.height),
        (chrome.width, chrome.height),
        "{name}"
    );
    let (mut exact, mut worst, mut worst_red_green) = (0, 0, 0);
    for (i, (&a, &b)) in ours.pixels.iter().zip(&chrome.pixels).enumerate() {
        assert_eq!(a >> 24, 0xff, "{name}: pixel {i} is not opaque");
        for shift in [16, 8, 0] {
            let d = ((a >> shift) & 0xff).abs_diff((b >> shift) & 0xff);
            worst = worst.max(d);
            if shift != 0 {
                worst_red_green = worst_red_green.max(d);
            }
            exact += usize::from(d == 0);
        }
    }
    (exact, worst, worst_red_green, ours.pixels.len() * 3)
}

#[test]
fn sdr_conversions_are_chrome_s_within_a_level() {
    let mut report = String::new();
    let mut short = false;
    for name in [
        "avifsdr_bt2020",
        "avifsdr_p3",
        "avifsdr_bt601",
        "avifsdr_gamma22",
        "avifsdr_bt2020_unsaid_curve",
    ] {
        let (exact, worst, _, channels) = against_chrome(name);
        report += &format!("\n  {name}: {exact} of {channels} exact, worst {worst}");
        assert!(worst <= 1, "{name}: a channel {worst} from Chrome's");
        short |= exact * 100 < channels * EXACT_PERCENT;
    }
    assert!(!short, "under {EXACT_PERCENT}% exactly Chrome's:{report}");
}

#[test]
fn bt709_is_not_converted() {
    let (_, worst, worst_red_green, _) = against_chrome("avifsdr_bt709");
    assert!(worst <= 15, "a channel {worst} from Chrome's");
    assert!(
        worst_red_green <= 2,
        "red or green {worst_red_green} from Chrome's"
    );
}
