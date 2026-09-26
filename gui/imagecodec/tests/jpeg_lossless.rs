//! Lossless JPEG (`SOF3`), against libjpeg-turbo.
//!
//! Every fixture is written by `tests/data/generate_jpeg_lossless.py`, which
//! has a small lossless encoder of its own because nothing common writes most
//! of what a decoder has to read: all seven predictors, point transforms,
//! samples of 2 to 8 bits (and the 12- and 16-bit ones the 8-bit interface
//! refuses), one to four components at any sampling, interleaved or in scans
//! of their own, restart intervals -- and damage: a file cut short, a restart
//! marker missing, a difference of 32768, a predictor out of range, a restart
//! interval that is not whole rows, a component no scan carries.
//!
//! Each answer is libjpeg-turbo 3.1.1's, through the 8-bit interface with the
//! colour space this crate asks for (greyscale for greyscale, RGB for RGB,
//! CMYK made RGB as Chrome makes it), or its refusal. The generator also
//! checks that every undamaged file's answer is the picture that went in,
//! which is what lossless means -- so each is a second, independent check on
//! the oracle.
//!
//! One fixture, `jpegll_grey_h2v2_restarts_as_the_standard_says`, is predicted
//! the way the standard says rather than the way libjpeg decodes: libjpeg
//! resets its predictor for the first row of the iMCU row a restart falls in,
//! not for the row after the marker. Its answer is libjpeg's, which is not
//! the picture that went in, and this decoder has to agree with libjpeg.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use imagecodec::{Image, Limits, decode, decode_scaled, dimensions};

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Every lossless fixture's name, sorted.
fn fixtures() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(data(""))
        .unwrap()
        .filter_map(|entry| {
            let name = entry.unwrap().file_name().into_string().ok()?;
            let stem = name.strip_suffix(".jpg")?;
            stem.starts_with("jpegll_").then(|| stem.to_string())
        })
        .collect();
    names.sort();
    names
}

/// libjpeg-turbo's answer for a fixture.
enum Answer {
    Picture(Image),
    Refused(String),
}

fn answer(name: &str) -> Answer {
    let path = data(&format!("{name}.txt"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    if let Some(why) = text.strip_prefix("REFUSED") {
        return Answer::Refused(why.trim().to_string());
    }
    let mut words = text.split_whitespace();
    let width: u32 = words.next().unwrap().parse().unwrap();
    let height: u32 = words.next().unwrap().parse().unwrap();
    let pixels: Vec<u32> = words.map(|w| u32::from_str_radix(w, 16).unwrap()).collect();
    assert_eq!(
        pixels.len(),
        (width * height) as usize,
        "{path}: wrong count"
    );
    Answer::Picture(Image {
        width,
        height,
        pixels,
    })
}

fn read(name: &str) -> Vec<u8> {
    let path = data(&format!("{name}.jpg"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn assert_same(name: &str, got: &Image, want: &Image) {
    assert_eq!(
        (got.width, got.height),
        (want.width, want.height),
        "{name}: a different size"
    );
    for (index, (g, w)) in got.pixels.iter().zip(&want.pixels).enumerate() {
        assert_eq!(
            g,
            w,
            "{name}: pixel ({}, {})",
            index % got.width as usize,
            index / got.width as usize
        );
    }
}

#[test]
fn every_fixture_decodes_as_libjpeg_turbo_decodes_it() {
    let names = fixtures();
    assert!(names.len() >= 50, "only {} fixtures", names.len());
    let (mut decoded, mut refused) = (0, 0);
    for name in &names {
        let ours = decode(&read(name), Limits::default());
        match answer(name) {
            Answer::Picture(want) => {
                let got = ours
                    .unwrap_or_else(|e| panic!("{name}: libjpeg decodes it, this refuses: {e:?}"));
                assert_same(name, &got, &want);
                decoded += 1;
            }
            Answer::Refused(why) => {
                assert!(
                    ours.is_err(),
                    "{name}: libjpeg refuses it ({why}) and this decodes it"
                );
                refused += 1;
            }
        }
    }
    // The generator's own count, so a fixture lost from the directory shows.
    assert_eq!((decoded, refused), (43, 13), "decoded and refused");
}

#[test]
fn a_greyscale_lossless_jpeg_decodes_where_chrome_s_choice_would_refuse_it() {
    // libjpeg converts nothing in a lossless image, so asking it for RGB, as
    // Chrome does for greyscale, fails; asking for greyscale, as GNOME's
    // loader and Pillow do and this crate does, shows the picture.
    let image = decode(&read("jpegll_grey_psv1"), Limits::default()).unwrap();
    assert_eq!((image.width, image.height), (13, 11));
    let &pixel = image.pixels.first().unwrap();
    let grey = pixel & 0xFF;
    assert_eq!(pixel, 0xFF00_0000 | (grey << 16) | (grey << 8) | grey);
}

#[test]
fn samples_narrower_than_a_byte_are_handed_out_as_they_are() {
    // A 4-bit image's samples run from 0 to 15, as libjpeg hands them out.
    let image = decode(&read("jpegll_grey_4bit"), Limits::default()).unwrap();
    assert!(image.pixels.iter().all(|&p| p & 0xFF <= 15));
    assert!(image.pixels.iter().any(|&p| p & 0xFF == 15));
}

/// The mean of each `factor` x `factor` square, truncated, as the crate's
/// thumbnail filter takes it -- written out again here, independently.
fn averaged(image: &Image, factor: u32) -> Image {
    let (w, h) = (image.width, image.height);
    let (ow, oh) = (w.div_ceil(factor), h.div_ceil(factor));
    let mut pixels = Vec::new();
    for oy in 0..oh {
        for ox in 0..ow {
            let mut sums = [0u32; 3];
            let mut n = 0;
            for y in oy * factor..((oy + 1) * factor).min(h) {
                for x in ox * factor..((ox + 1) * factor).min(w) {
                    let p = image.pixels[(y * w + x) as usize];
                    sums[0] += (p >> 16) & 0xFF;
                    sums[1] += (p >> 8) & 0xFF;
                    sums[2] += p & 0xFF;
                    n += 1;
                }
            }
            pixels.push(0xFF00_0000 | ((sums[0] / n) << 16) | ((sums[1] / n) << 8) | (sums[2] / n));
        }
    }
    Image {
        width: ow,
        height: oh,
        pixels,
    }
}

#[test]
fn a_thumbnail_is_the_whole_picture_averaged_down() {
    // A lossless JPEG has no transform to reduce in, and libjpeg decodes it
    // whole whatever scale is asked; the thumbnail is averaged from that.
    for name in ["jpegll_rgb_h2v2", "jpegll_grey_psv4", "jpegll_cmyk"] {
        let bytes = read(name);
        let Answer::Picture(full) = answer(name) else {
            panic!("{name}: refused");
        };
        for (box_w, box_h) in [(4, 4), (7, 3), (1, 1)] {
            let factor = full.width.div_ceil(box_w).max(full.height.div_ceil(box_h));
            let thumb = decode_scaled(&bytes, Limits::default(), box_w, box_h).unwrap();
            assert_same(
                &format!("{name} in {box_w}x{box_h}"),
                &thumb,
                &averaged(&full, factor),
            );
        }
    }
}

#[test]
fn dimensions_are_the_frame_s() {
    assert_eq!(dimensions(&read("jpegll_rgb_h2v2")).unwrap(), (15, 9));
    assert_eq!(dimensions(&read("jpegll_grey_1x7")).unwrap(), (1, 7));
    // The header is read as libjpeg reads it: a 16-bit lossless file has a
    // size, though it does not decode here.
    assert_eq!(
        dimensions(&read("jpegll_grey_16bit_refused")).unwrap(),
        (7, 5)
    );
}
