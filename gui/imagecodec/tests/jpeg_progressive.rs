//! Progressive JPEGs this repository did not write.
//!
//! Every fixture here was written by Pillow (libjpeg underneath) twice: once
//! baseline, once progressive, at the same quality and chroma subsampling
//! (`tests/data/generate_jpeg.py`). libjpeg's progressive encoder sends exactly
//! the quantised coefficients its baseline one does — progression reorders them
//! and splits them into bit planes, it does not change them — so the strongest
//! test available needs no tolerance at all: **the progressive file must decode
//! to the same pixels as its baseline twin**, and the baseline decoder is one
//! this crate already checks against a reference.
//!
//! That comparison cannot catch a mistake the two decoders share, so each
//! progressive file is also checked against Pillow's own decode of it, to within
//! the inverse-DCT rounding two decoders may differ by.
//!
//! The scan script in every colour fixture is libjpeg's standard progression:
//! an interleaved DC pass with one bit held back, spectral-selection AC passes
//! for luma and each chroma, two successive-approximation refinements of the
//! luma AC, a DC refinement, and a final refinement of every AC band — all four
//! kinds of progressive scan. The greyscale fixture has one component, so even
//! its DC passes are non-interleaved, and `jpeg420r` puts a restart marker
//! every three MCUs inside every pass.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]

use imagecodec::{Image, Limits, decode, decode_scaled, dimensions};

/// Every fixture pair, and what makes it worth having.
const CASES: &[(&str, &str)] = &[
    (
        "jpeg420",
        "4:2:0, the common case: chroma halved both ways, so an MCU is 16x16",
    ),
    ("jpeg422", "4:2:2: chroma halved across only"),
    ("jpeg444", "4:4:4: every component at full resolution"),
    (
        "jpeggrey",
        "one component, so even the DC passes are non-interleaved",
    ),
    (
        "jpegbig420",
        "larger, with partial MCUs on both edges, for the scaled decodes",
    ),
    (
        "jpeg420r",
        "a restart marker every three MCUs inside every pass",
    ),
];

fn read(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}.jpg", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Pillow's decode of a fixture: width, height, then `AARRGGBB` per pixel.
fn answer(name: &str) -> (u32, u32, Vec<u32>) {
    let path = format!("{}/tests/data/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut words = text.split_whitespace();
    let width: u32 = words.next().unwrap().parse().unwrap();
    let height: u32 = words.next().unwrap().parse().unwrap();
    let pixels: Vec<u32> = words
        .map(|w| u32::from_str_radix(w, 16).unwrap())
        .collect();
    assert_eq!(pixels.len(), (width * height) as usize, "{path}: wrong count");
    (width, height, pixels)
}

fn decoded(name: &str) -> Image {
    decode(&read(name), Limits::default()).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn a_progressive_file_decodes_to_exactly_what_its_baseline_twin_does() {
    for (name, why) in CASES {
        let baseline = decoded(&format!("{name}_baseline"));
        let progressive = decoded(&format!("{name}_progressive"));
        assert_eq!(
            (progressive.width, progressive.height),
            (baseline.width, baseline.height),
            "{name} ({why}): a different size"
        );
        let differing = baseline
            .pixels
            .iter()
            .zip(&progressive.pixels)
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            differing, 0,
            "{name} ({why}): {differing} pixels differ from the baseline decode of the same coefficients"
        );
    }
}

/// Fixtures whose chroma is not subsampled, so that no upsampling stands
/// between this decoder and the reference.
///
/// The subsampled ones are not compared here yet: libjpeg interpolates chroma
/// back up (its "fancy" triangle filter) where this crate repeats each sample,
/// and at a sharp colour edge the two differ by far more than rounding. That is
/// a baseline matter, shared by both twins and so invisible to the test above,
/// and it is being fixed on its own.
const NOT_SUBSAMPLED: &[&str] = &["jpeg444", "jpeggrey"];

#[test]
fn a_progressive_file_agrees_with_a_reference_decoder() {
    for (name, why) in CASES.iter().filter(|(name, _)| NOT_SUBSAMPLED.contains(name)) {
        let file = format!("{name}_progressive");
        let image = decoded(&file);
        let (width, height, want) = answer(&file);
        assert_eq!((image.width, image.height), (width, height), "{name}");
        let mut total = 0i64;
        for (index, (got, want)) in image.pixels.iter().zip(&want).enumerate() {
            assert_eq!(got >> 24, 0xFF, "{name}: pixel {index} is not opaque");
            for shift in [16u32, 8, 0] {
                let mine = ((got >> shift) & 0xFF) as i32;
                let theirs = ((want >> shift) & 0xFF) as i32;
                let difference = (mine - theirs).abs();
                assert!(
                    difference <= 2,
                    "{name} ({why}): pixel {index} ({got:08X} against {want:08X}) differs by {difference}: \
                     that is a decode, not a rounding"
                );
                total += i64::from(difference);
            }
        }
        let mean = total as f64 / (image.pixels.len() * 3) as f64;
        assert!(
            mean < 0.15,
            "{name} ({why}): mean channel error {mean:.3} is a bias, not rounding"
        );
    }
}

#[test]
fn a_scaled_progressive_decode_is_the_scaled_baseline_decode() {
    // A thumbnail keeps only the coefficients its scaled transform reads, and
    // only a mask of the rest. Different storage, same picture: the scaled
    // decode of the progressive file must still match its baseline twin's.
    for (name, _) in CASES {
        let baseline = read(&format!("{name}_baseline"));
        let progressive = read(&format!("{name}_progressive"));
        for bound in [4u32, 12, 30, 64, 1000] {
            let from_baseline = decode_scaled(&baseline, Limits::default(), bound, bound).unwrap();
            let from_progressive =
                decode_scaled(&progressive, Limits::default(), bound, bound).unwrap();
            assert_eq!(
                (from_progressive.width, from_progressive.height),
                (from_baseline.width, from_baseline.height),
                "{name} at {bound}"
            );
            assert!(
                from_progressive.pixels == from_baseline.pixels,
                "{name}: the {bound}px thumbnail differs from the baseline one"
            );
        }
    }
}

#[test]
fn the_size_of_a_progressive_file_is_read_from_its_header() {
    for (name, _) in CASES {
        let file = format!("{name}_progressive");
        let (width, height, _) = answer(&file);
        assert_eq!(dimensions(&read(&file)).unwrap(), (width, height), "{name}");
    }
}

/// Where each scan's `SOS` marker is, so a test can cut the file between
/// passes.
fn scan_starts(bytes: &[u8]) -> Vec<usize> {
    bytes
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] == 0xFF && w[1] == 0xDA)
        .map(|(at, _)| at)
        .collect()
}

#[test]
fn a_file_cut_between_passes_shows_the_passes_that_arrived() {
    // What a browser does with a progressive photograph still downloading: a
    // softer picture, the right size, rather than none.
    let full = read("jpegbig420_progressive");
    let complete = decoded("jpegbig420_progressive");
    let starts = scan_starts(&full);
    assert_eq!(starts.len(), 10, "the fixture's scan script changed");
    for passes in [1, 3, 6, 9] {
        let cut = &full[..starts[passes]];
        let image = decode(cut, Limits::default())
            .unwrap_or_else(|e| panic!("{passes} passes did not decode: {e}"));
        assert_eq!(
            (image.width, image.height),
            (complete.width, complete.height)
        );
        // Close to the finished picture, and closer the more passes arrived:
        // each pass only ever adds detail or precision.
        let error: i64 = image
            .pixels
            .iter()
            .zip(&complete.pixels)
            .map(|(a, b)| {
                [16u32, 8, 0]
                    .iter()
                    .map(|s| (((a >> s) & 0xFF) as i64 - ((b >> s) & 0xFF) as i64).abs())
                    .sum::<i64>()
            })
            .sum();
        let mean = error as f64 / (image.pixels.len() * 3) as f64;
        assert!(
            mean < 40.0,
            "{passes} passes gave a picture {mean:.1} levels from the finished one on average"
        );
    }
}

#[test]
fn a_file_cut_in_the_middle_of_a_pass_still_decodes() {
    let full = read("jpeg420_progressive");
    let starts = scan_starts(&full);
    // Halfway through the third pass's data.
    let cut = starts[2] + (starts[3] - starts[2]) / 2;
    let image = decode(&full[..cut], Limits::default()).expect("the earlier passes decode");
    assert_eq!((image.width, image.height), (61, 37));
}

#[test]
fn no_truncation_or_bit_flip_of_a_progressive_file_panics() {
    // Hostile input, the way `png`'s suite checks it: every prefix, and a
    // single bit flipped at a spread of positions through the whole file.
    let full = read("jpeg420r_progressive");
    for cut in 0..full.len() {
        let _ = decode(&full[..cut], Limits::default());
    }
    for at in (0..full.len() * 8).step_by(29) {
        let mut bent = full.clone();
        bent[at / 8] ^= 1 << (at % 8);
        let _ = decode(&bent, Limits::default());
        let _ = decode_scaled(&bent, Limits::default(), 16, 16);
    }
}

#[test]
fn a_progressive_file_past_the_byte_budget_is_refused_before_it_is_decoded() {
    let file = read("jpegbig420_progressive");
    let tight = Limits {
        max_decompressed_bytes: 10_000,
        ..Limits::default()
    };
    assert!(
        decode(&file, tight).is_err(),
        "the coefficient store was allocated past the caller's budget"
    );
    // The same file thumbnails within it: an eighth-scale decode keeps the DC
    // alone, ten bytes a block rather than 136.
    let thumb = decode_scaled(
        &file,
        Limits {
            max_decompressed_bytes: 10_000,
            ..Limits::default()
        },
        26,
        19,
    );
    assert!(thumb.is_ok(), "{thumb:?}");
}
