//! WebP against libwebp, through Pillow.
//!
//! Every comparison here is to the bit. Lossless WebP stores its encoder's
//! input exactly, alpha and the colour under transparent pixels included; lossy
//! WebP is a VP8 frame, whose decoding is specified to the bit, converted to RGB
//! by libwebp's own arithmetic. The fixtures (`tests/data/generate_webp.py`) are
//! chosen so that between them they use every tool each format has -- unit
//! tests in `webp/lossless.rs` and `webp/lossy.rs` check that they still do.

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

/// A lossy fixture's answer: Pillow's decode, kept as a PNG.
fn png_answer(name: &str) -> imagecodec::Image {
    let path = format!("{}/tests/data/{name}.png", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    decode(&bytes, Limits::default()).unwrap_or_else(|e| panic!("{path}: {e}"))
}

const LOSSY: &[(&str, &str)] = &[
    (
        "webp_lossy_photo",
        "libwebp's defaults: four segments, the normal filter",
    ),
    (
        "webp_lossy_q100",
        "the finest quantiser, and the largest coefficients",
    ),
    (
        "webp_lossy_q5",
        "the coarsest quantiser, and strong filtering",
    ),
    ("webp_lossy_noise", "noise: every subblock mode"),
    ("webp_lossy_big", "many rows and columns of macroblocks"),
    ("webp_lossy_odd", "odd sizes: chroma's last column and row"),
    (
        "webp_lossy_even",
        "even sizes that are not whole macroblocks",
    ),
    ("webp_lossy_1x1", "one pixel"),
    ("webp_lossy_column", "one pixel wide"),
    ("webp_lossy_row", "one pixel high"),
    ("webp_lossy_alpha", "an alpha plane, compressed"),
    (
        "webp_lossy_alpha_levels",
        "an alpha plane the encoder quantised",
    ),
    ("webp_lossy_alpha_raw", "an alpha plane stored raw"),
    ("webp_lossy_simple", "the simple loop filter"),
    ("webp_lossy_unfiltered", "no loop filter"),
    ("webp_lossy_sharp", "the loop filter at its sharpest"),
    ("webp_lossy_one_segment", "no segments"),
    ("webp_lossy_partitions", "eight coefficient partitions"),
    ("webp_lossy_skip", "macroblocks coded as empty"),
    (
        "webp_lossy_segment_deltas",
        "segments by delta, and filter levels clamped once as libwebp does",
    ),
    ("webp_lossy_segments_unmapped", "segments with no map"),
    (
        "webp_lossy_segments_unvalued",
        "segments with no values: libwebp's absolute zeros",
    ),
    ("webp_lossy_strong", "filter level 63, with deltas"),
    ("webp_lossy_quant_deltas", "every quantiser delta"),
    (
        "webp_lossy_colour_space",
        "the colour-space bits, which WebP ignores",
    ),
    (
        "webp_lossy_libvpx",
        "libvpx's frame: its filter deltas, four partitions",
    ),
    (
        "webp_lossy_corrupt_coder",
        "corrupt: a coder pushed past its range, read as libwebp reads it",
    ),
    (
        "webp_lossy_short_by_padding",
        "cut short, and finished by the chunk's padding byte as in libwebp",
    ),
    ("webp_alpha_raw_none", "a raw alpha plane"),
    ("webp_alpha_raw_horizontal", "raw, filtered from the left"),
    ("webp_alpha_raw_vertical", "raw, filtered from above"),
    ("webp_alpha_raw_gradient", "raw, filtered by gradient"),
    ("webp_alpha_lossless_none", "a compressed alpha plane"),
    (
        "webp_alpha_lossless_horizontal",
        "compressed, filtered from the left",
    ),
    (
        "webp_alpha_lossless_vertical",
        "compressed, filtered from above",
    ),
    (
        "webp_alpha_lossless_gradient",
        "compressed, filtered by gradient",
    ),
];

#[test]
fn every_lossy_fixture_decodes_to_exactly_what_libwebp_does() {
    for (name, why) in LOSSY {
        let bytes = read(name);
        let want = png_answer(name);
        let image = decode(&bytes, Limits::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            (image.width, image.height),
            (want.width, want.height),
            "{name}"
        );
        assert_eq!(
            dimensions(&bytes).unwrap(),
            (want.width, want.height),
            "{name}"
        );
        if let Some(at) = image
            .pixels
            .iter()
            .zip(&want.pixels)
            .position(|(a, b)| a != b)
        {
            panic!(
                "{name} ({why}): pixel ({}, {}) is {:08X} where libwebp gives {:08X}",
                at as u32 % want.width,
                at as u32 / want.width,
                image.pixels[at],
                want.pixels[at]
            );
        }
    }
}

/// `bytes`'s first chunk replaced by the first `keep` bytes of its payload,
/// with the chunk and RIFF sizes made to match: a file that is complete as a
/// container and cut short inside its bitstream.
fn cut_inside_the_bitstream(bytes: &[u8], keep: usize) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    let body = 4 + 8 + keep + (keep & 1);
    out.extend_from_slice(&(body as u32).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&bytes[12..16]);
    out.extend_from_slice(&(keep as u32).to_le_bytes());
    out.extend_from_slice(&bytes[20..20 + keep]);
    if keep & 1 == 1 {
        out.push(0);
    }
    out
}

#[test]
fn a_lossy_webp_whose_coefficients_run_out_is_truncated() {
    let full = read("webp_lossy_photo");
    let payload = u32::from_le_bytes(full[16..20].try_into().unwrap()) as usize;
    // Past the headers, the modes and the start of the coefficients: the
    // frame reads until its one partition runs dry, as libwebp does.
    for keep in [payload / 2, payload * 3 / 4, payload - 8] {
        let cut = cut_inside_the_bitstream(&full, keep);
        assert_eq!(
            decode(&cut, Limits::default()),
            Err(ImageError::Truncated),
            "cut to {keep} of {payload}"
        );
    }
    // Before its first partition ends it is refused as a short partition.
    let cut = cut_inside_the_bitstream(&full, 30);
    assert!(decode(&cut, Limits::default()).is_err());
}

#[test]
fn no_truncation_or_bit_flip_of_a_lossy_webp_panics() {
    for (name, cuts, flips) in [
        ("webp_lossy_odd", 1, 3),
        ("webp_lossy_skip", 1, 5),
        ("webp_alpha_lossless_gradient", 3, 11),
        ("webp_lossy_partitions", 17, 97),
        ("webp_lossy_segment_deltas", 7, 41),
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
fn a_lossy_picture_past_the_limits_is_refused_before_it_is_decoded() {
    let bytes = read("webp_lossy_big");
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
fn a_thumbnail_of_a_webp_fits_its_box() {
    let bytes = read("webp_lossless_big");
    let thumb = decode_scaled(&bytes, Limits::default(), 50, 50).unwrap();
    assert_eq!((thumb.width, thumb.height), (50, 36), "203x149 into 50x50");
    let same = decode_scaled(&read("webp_lossless_1x1"), Limits::default(), 50, 50).unwrap();
    assert_eq!((same.width, same.height), (1, 1), "no pixels invented");
    let lossy = decode_scaled(&read("webp_lossy_big"), Limits::default(), 40, 40).unwrap();
    assert_eq!((lossy.width, lossy.height), (40, 29), "131x97 into 40x40");
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
