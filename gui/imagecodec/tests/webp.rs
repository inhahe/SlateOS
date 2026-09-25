//! WebP against libwebp, through Pillow.
//!
//! Lossless WebP stores its encoder's input exactly, alpha and the colour under
//! transparent pixels included, so every comparison here is to the bit. The
//! fixtures are libwebp's own output (`tests/data/generate_webp.py`), chosen so
//! that between them they use every tool the lossless format has -- a unit test
//! in `webp/lossless.rs` checks that they still do.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use imagecodec::{ImageError, Limits, decode, decode_scaled, dimensions};

fn read(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}.webp", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Pillow's decode: width, height, pixels.
fn answer(name: &str) -> (u32, u32, Vec<u32>) {
    let path = format!("{}/tests/data/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut words = text.split_whitespace();
    let width: u32 = words.next().unwrap().parse().unwrap();
    let height: u32 = words.next().unwrap().parse().unwrap();
    let pixels: Vec<u32> = words.map(|w| u32::from_str_radix(w, 16).unwrap()).collect();
    assert_eq!(pixels.len(), (width * height) as usize, "{path}");
    (width, height, pixels)
}

const LOSSLESS: &[(&str, &str)] = &[
    ("webp_lossless_photo", "a photograph at the most effort"),
    ("webp_lossless_fast", "the same at the least"),
    ("webp_lossless_big", "larger, with meta prefix codes"),
    (
        "webp_lossless_alpha",
        "alpha, and colour kept under clear pixels",
    ),
    ("webp_lossless_2c", "two colours: eight indices a byte"),
    ("webp_lossless_4c", "four colours: four a byte"),
    ("webp_lossless_13c", "thirteen colours: two a byte"),
    (
        "webp_lossless_100c",
        "a hundred colours: a palette, unbundled",
    ),
    ("webp_lossless_1x1", "one pixel"),
    ("webp_lossless_column", "one pixel wide"),
    ("webp_lossless_row", "one pixel high"),
    (
        "webp_lossless_extended",
        "the extended container, behind a VP8X",
    ),
];

#[test]
fn every_lossless_fixture_decodes_to_exactly_what_libwebp_does() {
    for (name, why) in LOSSLESS {
        let bytes = read(name);
        let (width, height, want) = answer(name);
        let image = decode(&bytes, Limits::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((image.width, image.height), (width, height), "{name}");
        assert_eq!(dimensions(&bytes).unwrap(), (width, height), "{name}");
        if let Some(at) = image.pixels.iter().zip(&want).position(|(a, b)| a != b) {
            panic!(
                "{name} ({why}): pixel ({}, {}) is {:08X} where libwebp gives {:08X}",
                at as u32 % width,
                at as u32 / width,
                image.pixels[at],
                want[at]
            );
        }
    }
}

#[test]
fn a_thumbnail_of_a_webp_fits_its_box() {
    let bytes = read("webp_lossless_big");
    let thumb = decode_scaled(&bytes, Limits::default(), 50, 50).unwrap();
    assert_eq!((thumb.width, thumb.height), (50, 36), "203x149 into 50x50");
    let same = decode_scaled(&read("webp_lossless_1x1"), Limits::default(), 50, 50).unwrap();
    assert_eq!((same.width, same.height), (1, 1), "no pixels invented");
}

#[test]
fn a_picture_past_the_limits_is_refused_before_it_is_decoded() {
    let bytes = read("webp_lossless_big");
    let few = Limits {
        max_pixels: 10_000,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, few),
        Err(ImageError::TooLarge { .. })
    ));
    let tight = Limits {
        max_decompressed_bytes: 50_000,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, tight),
        Err(ImageError::TooLarge { .. })
    ));
}

#[test]
fn no_truncation_or_bit_flip_of_a_lossless_webp_panics() {
    // (file, stride through its prefixes, stride through its bits): every
    // prefix and every third bit of the small ones, a spread of the large one,
    // so the suite stays quick in a debug build.
    for (name, cuts, flips) in [
        ("webp_lossless_4c", 1, 3),
        ("webp_lossless_2c", 1, 3),
        ("webp_lossless_alpha", 5, 37),
        ("webp_lossless_photo", 11, 101),
    ] {
        let full = read(name);
        for cut in (0..full.len()).step_by(cuts) {
            let _ = decode(&full[..cut], Limits::default());
        }
        for at in (0..full.len() * 8).step_by(flips) {
            let mut bent = full.clone();
            bent[at / 8] ^= 1 << (at % 8);
            let _ = decode(&bent, Limits::default());
        }
    }
}

#[test]
fn a_cut_short_lossless_webp_is_truncated_not_a_different_picture() {
    let full = read("webp_lossless_photo");
    // Past the header, before the end: whatever the decoder makes of it, it
    // must not be a picture that passes for the whole one.
    let cut = &full[..full.len() / 2];
    match decode(cut, Limits::default()) {
        Err(ImageError::Truncated | ImageError::Malformed(_)) => {}
        other => panic!("half a file decoded as {other:?}"),
    }
}
