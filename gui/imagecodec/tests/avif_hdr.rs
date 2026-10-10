//! HDR AVIF -- PQ and HLG -- held to Chrome's own pixels for it: each
//! `avifhdr_*.avif` against `avifhdr_*.chrome.png`, a screenshot of headless
//! Chrome 154 showing the picture on an sRGB screen
//! (`tests/data/generate_avif_hdr.py`; design-decisions §1378).
//!
//! The 4:4:4 pictures are patches of one colour each, PQ's greys from black
//! to 10 000 cd/m2 and colours, HLG's likewise, one of them with a `clli`
//! box saying MaxCLL 4000: their every pixel is Chrome's. The 4:2:0 picture
//! is noise, its every pixel's colour hanging on how the chroma is brought
//! up: its pixels are Chrome's in all but under 2% of channels, and there
//! by one (Chrome brings chroma up on the GPU, whose texture filtering is
//! not quite floating point).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]

use imagecodec::{Limits, decode};

fn read(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// `name`'s pixels and Chrome's: how many channels differ, by how much at
/// most, and of how many.
fn against_chrome(name: &str) -> (usize, u32, usize) {
    let ours = decode(&read(&format!("{name}.avif")), Limits::default())
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    let chrome = decode(&read(&format!("{name}.chrome.png")), Limits::default()).unwrap();
    assert_eq!(
        (ours.width, ours.height),
        (chrome.width, chrome.height),
        "{name}"
    );
    let (mut off, mut worst) = (0, 0);
    for (i, (&a, &b)) in ours.pixels.iter().zip(&chrome.pixels).enumerate() {
        assert_eq!(a >> 24, 0xff, "{name}: pixel {i} is not opaque");
        for shift in [16, 8, 0] {
            let d = ((a >> shift) & 0xff).abs_diff((b >> shift) & 0xff);
            worst = worst.max(d);
            off += usize::from(d != 0);
        }
    }
    (off, worst, ours.pixels.len() * 3)
}

#[test]
fn hdr_patches_are_chrome_s_pixel_for_pixel() {
    for name in ["avifhdr_pq", "avifhdr_hlg", "avifhdr_pq_clli"] {
        let (off, worst, channels) = against_chrome(name);
        assert_eq!(
            off, 0,
            "{name}: {off} of {channels} channels from Chrome's, by up to {worst}"
        );
    }
}

/// The `clli` box's MaxCLL is what the picture is tone mapped by: the same
/// patches without it are tone mapped for 1000 cd/m2, and differ.
#[test]
fn the_content_light_level_moves_the_tone_map() {
    let plain = decode(&read("avifhdr_pq.avif"), Limits::default()).unwrap();
    let clli = decode(&read("avifhdr_pq_clli.avif"), Limits::default()).unwrap();
    assert_ne!(plain.pixels, clli.pixels);
}

#[test]
fn hdr_4_2_0_is_chrome_s_within_one() {
    let (off, worst, channels) = against_chrome("avifhdr_pq_420");
    assert!(worst <= 1, "a channel {worst} from Chrome's");
    assert!(
        off * 50 < channels,
        "{off} of {channels} channels from Chrome's"
    );
}
