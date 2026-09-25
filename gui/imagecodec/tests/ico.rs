//! ICO and CUR against Chrome's rules.
//!
//! Icons Pillow writes are answered by Pillow, which reads them as Chrome
//! does; icons written by hand are answered by Chrome's rules applied to the
//! pixels, palettes and masks they were made from, and the generator
//! (`tests/data/generate_ico.py`) checks that each rule it tests is one Pillow
//! gets differently. Every comparison is exact.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::path::PathBuf;

use imagecodec::{ImageError, Limits, decode, decode_scaled, dimensions, ico};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn fixtures() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(data_dir())
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name()?.to_str()?.to_owned();
            name.strip_suffix(".ico").map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(data_dir().join(format!("{name}.ico"))).unwrap()
}

fn answer(name: &str) -> Option<(u32, u32, Vec<u32>)> {
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
    Some((width, height, pixels))
}

#[test]
fn every_icon_decodes_or_is_refused_as_chrome_does() {
    let names = fixtures();
    assert!(
        names.len() >= 25,
        "{} fixtures: some have gone missing",
        names.len()
    );
    let mut refused = 0;
    for name in &names {
        let bytes = read(name);
        assert!(ico::is_ico(&bytes), "{name}");
        match (answer(name), decode(&bytes, Limits::default())) {
            (None, Err(_)) => refused += 1,
            (None, Ok(image)) => panic!(
                "{name}: Chrome refuses it, this decodes it ({}x{})",
                image.width, image.height
            ),
            (Some(_), Err(e)) => panic!("{name}: Chrome decodes it, this refuses it: {e}"),
            (Some((width, height, want)), Ok(image)) => {
                assert_eq!((image.width, image.height), (width, height), "{name}");
                if let Some(at) = image.pixels.iter().zip(&want).position(|(a, b)| a != b) {
                    panic!(
                        "{name}: pixel ({}, {}) is {:08X} where Chrome gives {:08X}",
                        at as u32 % width,
                        at as u32 / width,
                        image.pixels[at],
                        want[at]
                    );
                }
                assert_eq!(dimensions(&bytes).unwrap(), (width, height), "{name}");
            }
        }
    }
    assert!(refused >= 8, "only {refused} refusals");
}

#[test]
fn an_icon_cut_short_anywhere_is_refused() {
    for name in ["ico_bmp_8", "ico_bmp_24", "ico_bmp_rle8", "ico_pillow_png"] {
        let full = read(name);
        for cut in 0..full.len() {
            assert!(
                decode(&full[..cut], Limits::default()).is_err(),
                "{name} cut to {cut} of {}",
                full.len()
            );
        }
    }
}

#[test]
fn no_bit_flip_of_an_icon_panics() {
    for (name, stride) in [
        ("ico_bmp_rle8", 1),
        ("ico_bmp_32_alpha_late", 7),
        ("ico_bmp_565", 5),
        ("ico_best_is_deepest", 11),
        ("ico_pillow_png", 13),
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
fn an_icon_past_the_limits_is_refused() {
    let bytes = read("ico_pillow_png");
    let few = Limits {
        max_pixels: 48 * 48 - 1,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, few),
        Err(ImageError::TooLarge { .. })
    ));
    let bmp = read("ico_bmp_24");
    let tight = Limits {
        max_decompressed_bytes: 16 * 16 * 4 - 1,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bmp, tight),
        Err(ImageError::TooLarge { .. })
    ));
}

#[test]
fn an_icon_thumbnail_fits_its_box() {
    let thumb = decode_scaled(&read("ico_pillow_png"), Limits::default(), 24, 24).unwrap();
    assert_eq!((thumb.width, thumb.height), (24, 24));
}
