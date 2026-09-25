//! TIFF against libtiff itself.
//!
//! Every fixture's answer (`tests/data/generate_tiff.py`) is what libtiff
//! 4.7.1's RGBA reader -- the one image viewers call -- makes of it: its
//! raster exactly, or a refusal. The comparison is exact: TIFF samples are
//! stored, not approximated, and every conversion is integer arithmetic, so
//! a port that agrees agrees to the last bit.
//!
//! libtiff's raster premultiplies alpha and flips rather than turns;
//! [`tiff::decode_libtiff_raster`] reproduces it for the comparison, and the
//! tests below check [`decode`] against it: straight alpha, turned by the
//! orientation.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::path::PathBuf;

use imagecodec::orientation::Orientation;
use imagecodec::{Image, ImageError, Limits, decode, decode_scaled, dimensions, tiff};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

/// Every TIFF fixture, by name.
fn fixtures() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(data_dir())
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name()?.to_str()?.to_owned();
            name.strip_suffix(".tif").map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(data_dir().join(format!("{name}.tif"))).unwrap()
}

/// libtiff's answer: its raster, or `None` for a refusal.
fn answer(name: &str) -> Option<Image> {
    let text = std::fs::read_to_string(data_dir().join(format!("{name}.txt"))).unwrap();
    let mut words = text.split_whitespace();
    let first = words.next().unwrap();
    if first == "REFUSED" {
        return None;
    }
    let width: u32 = first.parse().unwrap();
    let height: u32 = words.next().unwrap().parse().unwrap();
    let pixels: Vec<u32> = words.map(|w| u32::from_str_radix(w, 16).unwrap()).collect();
    assert_eq!(pixels.len(), (width * height) as usize, "{name}");
    Some(Image {
        width,
        height,
        pixels,
    })
}

fn first_difference(name: &str, ours: &Image, want: &Image) {
    assert_eq!(
        (ours.width, ours.height),
        (want.width, want.height),
        "{name}"
    );
    if let Some(at) = ours
        .pixels
        .iter()
        .zip(&want.pixels)
        .position(|(a, b)| a != b)
    {
        panic!(
            "{name}: pixel ({}, {}) is {:08X} where libtiff gives {:08X}",
            at as u32 % want.width,
            at as u32 / want.width,
            ours.pixels[at],
            want.pixels[at]
        );
    }
}

#[test]
fn every_fixture_decodes_or_is_refused_exactly_as_libtiff_does() {
    let names = fixtures();
    assert!(
        names.len() >= 137,
        "{} fixtures: some have gone missing",
        names.len()
    );
    let mut refused = 0;
    for name in &names {
        let bytes = read(name);
        assert!(tiff::is_tiff(&bytes), "{name}");
        match (
            answer(name),
            tiff::decode_libtiff_raster(&bytes, Limits::default()),
        ) {
            (None, Err(_)) => {
                refused += 1;
                assert!(decode(&bytes, Limits::default()).is_err(), "{name}");
            }
            (None, Ok(image)) => panic!(
                "{name}: libtiff refuses it, this decodes it ({}x{})",
                image.width, image.height
            ),
            (Some(_), Err(e)) => panic!("{name}: libtiff decodes it, this refuses it: {e}"),
            (Some(want), Ok(image)) => first_difference(name, &image, &want),
        }
    }
    assert!(
        refused >= 20,
        "only {refused} refusals: the refused fixtures are not reaching the decoder"
    );
}

/// libtiff's premultiplication (`BuildMapUaToAa`).
fn premultiply(v: u32, a: u32) -> u32 {
    (v * a + 127) / 255
}

fn channels(p: u32) -> [u32; 4] {
    [p >> 24, (p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF]
}

#[test]
fn opaque_pictures_decode_to_libtiffs_pixels() {
    for name in fixtures() {
        let bytes = read(&name);
        let (Some(want), Ok(image)) = (answer(&name), decode(&bytes, Limits::default())) else {
            continue;
        };
        if want.pixels.iter().any(|p| p >> 24 != 0xFF)
            || tiff::orientation(&bytes) != Orientation::TopLeft
        {
            continue;
        }
        first_difference(&name, &image, &want);
        assert_eq!(
            dimensions(&bytes).unwrap(),
            (want.width, want.height),
            "{name}"
        );
    }
}

#[test]
fn unassociated_alpha_is_kept_straight_and_premultiplies_to_libtiffs() {
    for name in [
        "tiff_rgba8_unassociated",
        "tiff_rgba16_unassociated",
        "tiff_rgba8_separate_unassociated",
        "tiff_rgba16_lzw_predictor_big_endian",
        "tiff_rgba16_tiled",
        "tiff_corel_alpha_999",
    ] {
        let image = decode(&read(name), Limits::default()).unwrap();
        let want = answer(name).unwrap();
        assert!(
            want.pixels.iter().any(|p| p >> 24 != 0xFF),
            "{name}: no alpha to test"
        );
        for (ours, theirs) in image.pixels.iter().zip(&want.pixels) {
            let [a, r, g, b] = channels(*ours);
            let premultiplied = (a << 24)
                | (premultiply(r, a) << 16)
                | (premultiply(g, a) << 8)
                | premultiply(b, a);
            assert_eq!(premultiplied, *theirs, "{name}: {ours:08X}");
        }
    }
}

#[test]
fn associated_alpha_is_divided_back_out() {
    for name in [
        "tiff_rgba8_associated",
        "tiff_rgba8_separate_associated",
        "tiff_rgba16_associated",
        "tiff_rgba8_unspecified",
    ] {
        let image = decode(&read(name), Limits::default()).unwrap();
        let want = answer(name).unwrap();
        for (ours, theirs) in image.pixels.iter().zip(&want.pixels) {
            let [a, r, g, b] = channels(*ours);
            let [ta, tr, tg, tb] = channels(*theirs);
            assert_eq!(a, ta, "{name}");
            // Straight colour that premultiplies back to what libtiff holds;
            // white where libtiff's colour is brighter than its alpha.
            for (s, t) in [(r, tr), (g, tg), (b, tb)] {
                if t >= ta && ta != 0 {
                    assert_eq!(s, 255, "{name}: {ours:08X} for {theirs:08X}");
                } else if ta != 0 {
                    assert_eq!(premultiply(s, a), t, "{name}: {ours:08X} for {theirs:08X}");
                }
            }
        }
    }
}

#[test]
fn grey_with_alpha_is_as_libtiff_hands_it_over() {
    // libtiff passes a grey picture's samples through, alpha and all:
    // unassociated, that is already straight.
    let name = "tiff_grey_alpha8_unassociated";
    let image = decode(&read(name), Limits::default()).unwrap();
    first_difference(name, &image, &answer(name).unwrap());
}

#[test]
fn every_orientation_turns_the_stored_picture() {
    let upright = decode(&read("tiff_orientation1"), Limits::default()).unwrap();
    for value in 1..=8u16 {
        let name = format!("tiff_orientation{value}");
        let bytes = read(&name);
        let orientation = Orientation::from_value(value).unwrap();
        assert_eq!(tiff::orientation(&bytes), orientation, "{name}");
        let image = decode(&bytes, Limits::default()).unwrap();
        assert_eq!(image, orientation.apply(upright.clone()), "{name}");
        assert_eq!(
            dimensions(&bytes).unwrap(),
            (image.width, image.height),
            "{name}"
        );
        // libtiff's own raster flips 5-8 as 1-4; the viewers turn the rest.
        let raster = tiff::decode_libtiff_raster(&bytes, Limits::default()).unwrap();
        first_difference(&name, &raster, &answer(&name).unwrap());
    }
    let ignored = decode(&read("tiff_orientation9_ignored"), Limits::default()).unwrap();
    assert_eq!(ignored, upright);
}

#[test]
fn a_file_cut_short_is_refused() {
    // libtiff reads the whole of a strip or not at all.
    for name in [
        "tiff_rgb8",
        "tiff_rgb8_lzw",
        "tiff_rgb8_tiled",
        "tiff_grey1_packbits",
    ] {
        let full = read(name);
        for cut in 0..full.len() {
            let _ = decode(&full[..cut], Limits::default());
        }
        assert!(
            decode(&full[..full.len() / 2], Limits::default()).is_err(),
            "{name}"
        );
    }
}

#[test]
fn no_bit_flip_of_a_tiff_panics() {
    for (name, stride) in [
        ("tiff_rgb8_lzw_predictor", 3),
        ("tiff_rgb8_tiled_separate", 5),
        ("tiff_palette4_tiled_lzw", 3),
        ("tiff_rgba16_lzw_predictor_big_endian", 7),
        ("tiff_rgb8_old_style_lzw", 3),
        ("tiff_grey1_packbits", 1),
        ("tiff_rgb16_bigtiff_big_endian_lzw", 5),
        ("tiff_rgb8_zip", 3),
    ] {
        let full = read(name);
        for at in (0..full.len() * 8).step_by(stride) {
            let mut bent = full.clone();
            bent[at / 8] ^= 1 << (at % 8);
            let _ = decode(&bent, Limits::default());
            let _ = dimensions(&bent);
        }
    }
}

#[test]
fn a_tiff_past_the_limits_is_refused_before_it_is_decoded() {
    let bytes = read("tiff_rgb8_tiled");
    let few = Limits {
        max_pixels: 37 * 21 - 1,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, few),
        Err(ImageError::TooLarge { .. })
    ));
    let tight = Limits {
        max_decompressed_bytes: 16 * 16 * 3 - 1,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, tight),
        Err(ImageError::TooLarge { .. })
    ));
    assert!(
        decode(
            &bytes,
            Limits {
                max_decompressed_bytes: 16 * 16 * 3,
                ..Limits::default()
            }
        )
        .is_ok()
    );
}

#[test]
fn a_tiff_thumbnail_fits_its_box() {
    let thumb = decode_scaled(&read("tiff_rgb8_tiled"), Limits::default(), 10, 10).unwrap();
    assert!(thumb.width <= 10 && thumb.height <= 10);
    assert!(thumb.width > thumb.height);
}

#[test]
fn the_size_is_read_from_the_directory_alone() {
    let bytes = read("tiff_rgb8");
    let ifd = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    // Wreck the pixels: the size is still there.
    let mut no_pixels = bytes.clone();
    for b in &mut no_pixels[8..ifd] {
        *b = 0xFF;
    }
    assert_eq!(dimensions(&no_pixels).unwrap(), (13, 11));
    assert_eq!(dimensions(&read("tiff_orientation6")).unwrap(), (11, 13));
}
