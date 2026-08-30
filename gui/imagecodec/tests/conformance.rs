//! Decoding files that this repository did not write.
//!
//! # Why this exists next to a test module that already has fifty tests
//!
//! Every test in `src/png.rs` decodes a PNG that `src/png.rs`'s own helpers
//! built. That is the only way to test a *rule* — a deliberately truncated
//! chunk, a colour type of 7, a CRC that does not match — and those tests earn
//! their place. But they share one blind spot with every self-built fixture: if
//! the decoder and the fixture builder read RFC 2083 the same wrong way, they
//! agree, and the suite is green on a picture no other program can read.
//!
//! The files in `tests/data/` were written by Pillow (libpng, the reference
//! implementation) and by ImageMagick. Nothing in this tree chose their bytes:
//! the per-row filter choices, the Huffman tables, the chunk order, the
//! ancillary chunks (`gAMA`, `cHRM`, `bKGD`, `tIME`, `tEXt`) our helpers never
//! emit, and the Adam7 packing are all decisions of code we do not own — and
//! several of them are decisions our helpers *cannot* make, since those always
//! use filter 0 and stored DEFLATE blocks.
//!
//! The `.txt` beside each file is the expected answer, taken from Pillow's
//! **decoder** rather than from ours. So each case compares two independent
//! implementations of the entire path, and a disagreement is a real one.
//!
//! `tests/data/generate.py` regenerates the pair; see its docstring.

// The same suppressions every `#[cfg(test)]` module in this workspace carries.
// A test is the one place where panicking on bad data is the *point*: a fixture
// that will not load or an answer file with the wrong number of words is a
// broken test, and a broken test should stop rather than quietly assert less.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use imagecodec::{Limits, decode, dimensions};

/// Every fixture, and what makes it worth having.
///
/// Listed explicitly rather than discovered by walking the directory: a
/// `read_dir` that returned nothing would make this file pass with zero cases
/// tested, which is the one outcome a conformance suite must never produce
/// quietly.
const CASES: &[(&str, &str)] = &[
    (
        "gray1",
        "1-bit greyscale — eight pixels to a byte, MSB first",
    ),
    ("gray2", "2-bit greyscale — four to a byte"),
    ("gray4", "4-bit greyscale — two to a byte"),
    (
        "gray8",
        "8-bit greyscale — one sample reaching three channels",
    ),
    (
        "gray16",
        "16-bit greyscale — the high byte is the one that survives",
    ),
    ("graya8", "greyscale with an alpha channel"),
    ("palette4", "4-bit palette indices — two per byte"),
    ("palette8_trns", "8-bit palette with per-entry transparency"),
    ("rgb8", "truecolour, the commonest wallpaper there is"),
    ("rgba8", "truecolour with straight alpha"),
    (
        "rgb8_filtered",
        "libpng picking a filter per row, and dynamic Huffman",
    ),
    ("gray8_interlaced", "Adam7, greyscale"),
    ("rgb8_interlaced", "Adam7, truecolour"),
    ("palette8_interlaced", "Adam7, palette"),
    ("rgba8_interlaced", "Adam7, truecolour with alpha"),
];

/// `(width, height, pixels)` as the fixture's `.txt` states them.
fn expected(name: &str) -> (u32, u32, Vec<u32>) {
    let path = format!("{}/tests/data/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut words = text.split_ascii_whitespace();
    let w: u32 = words.next().expect("width").parse().expect("width");
    let h: u32 = words.next().expect("height").parse().expect("height");
    let pixels: Vec<u32> = words
        .map(|word| u32::from_str_radix(word, 16).expect("an AARRGGBB word"))
        .collect();
    assert_eq!(
        pixels.len() as u32,
        w * h,
        "{name}.txt lists the wrong number of pixels"
    );
    (w, h, pixels)
}

fn file(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}.png", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn every_fixture_decodes_to_what_libpng_says_it_is() {
    assert!(!CASES.is_empty(), "a conformance suite with no cases");
    for &(name, what) in CASES {
        let (w, h, want) = expected(name);
        let got = decode(&file(name), Limits::default())
            .unwrap_or_else(|e| panic!("{name} ({what}): {e}"));
        assert_eq!((got.width, got.height), (w, h), "{name}: wrong size");

        // Reported per pixel rather than as a whole-vector inequality: a
        // 63-pixel diff printed in full is unreadable, and the *first* pixel
        // that differs is what names the bug — column 0 means the filter,
        // row 1 means the previous-row wiring, a scattered set means Adam7.
        for (i, (&g, &e)) in got.pixels.iter().zip(want.iter()).enumerate() {
            assert_eq!(
                g,
                e,
                "{name} ({what}): pixel {i} at ({}, {}) is {g:08X}, libpng says {e:08X}",
                i as u32 % w,
                i as u32 / w
            );
        }
    }
}

#[test]
fn dimensions_agrees_with_the_full_decode_on_every_fixture() {
    // The header reader and the pixel decoder are separate code paths that a
    // file manager uses interchangeably — the detail column comes from one and
    // the thumbnail from the other. They must never disagree about the size of
    // the same file.
    for &(name, what) in CASES {
        let bytes = file(name);
        let (w, h) = dimensions(&bytes).unwrap_or_else(|e| panic!("{name} ({what}): {e}"));
        let img = decode(&bytes, Limits::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((w, h), (img.width, img.height), "{name}");
    }
}

#[test]
fn a_real_file_truncated_at_every_length_is_an_error_and_never_a_panic() {
    // The same sweep `src/png.rs` runs, but over files with dynamic Huffman
    // tables, ancillary chunks and Adam7 passes — every one of which is a
    // structure our hand-built fixtures never contain, and therefore a length
    // at which our hand-built sweep can never cut.
    for &(name, _) in CASES {
        let bytes = file(name);
        for cut in 0..bytes.len() {
            // The result is uninteresting; not unwinding is the assertion.
            let _ = decode(&bytes[..cut], Limits::default());
            let _ = dimensions(&bytes[..cut]);
        }
    }
}

#[test]
fn a_real_file_with_any_single_byte_corrupted_is_an_error_and_never_a_panic() {
    // Flipping the top bit of one byte at a time: enough to break a CRC, a
    // Huffman code, a filter type, a palette length or a declared dimension,
    // and cheap enough to run over every byte of every fixture.
    for &(name, _) in CASES {
        let bytes = file(name);
        for i in 0..bytes.len() {
            let mut broken = bytes.clone();
            broken[i] ^= 0x80;
            let _ = decode(&broken, Limits::default());
            let _ = dimensions(&broken);
        }
    }
}

#[test]
fn a_limit_below_a_real_files_size_refuses_it_from_the_header() {
    // `Limits` has to bite before the pixel buffer exists, and the fixtures are
    // 9x7 = 63 pixels, so a 62-pixel ceiling is one pixel too small.
    let limits = Limits {
        max_pixels: 62,
        ..Limits::default()
    };
    for &(name, _) in CASES {
        let e = decode(&file(name), limits).expect_err(&format!("{name} should be refused"));
        assert_eq!(
            e,
            imagecodec::ImageError::TooLarge {
                pixels: 63,
                limit: 62
            },
            "{name}"
        );
    }
}

#[test]
fn the_bytes_handed_to_the_compositor_are_four_per_pixel_and_little_endian() {
    // What `Connection::upload_image` sends and what `normalize` reads back.
    let img = decode(&file("rgba8"), Limits::default()).expect("rgba8");
    let bytes = img.to_argb_bytes();
    assert_eq!(bytes.len(), img.pixels.len() * 4);
    assert_eq!(img.stride(), img.width * 4);
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        assert_eq!(
            word, img.pixels[i],
            "pixel {i} did not survive the round trip"
        );
    }
}
