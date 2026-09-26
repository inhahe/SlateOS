//! BMP against Chrome's own decoder.
//!
//! Every fixture's answer (`tests/data/generate_bmp.py`) is what Chrome's BMP
//! decoder -- image-rs 0.25.10 with Chromium's patches, compiled -- makes of
//! it: the pixels, or a refusal. BMP is uncompressed or run-length encoded, so
//! every comparison is exact, and a refusal must be a refusal here too: Chrome
//! does not show a BMP it cannot finish, so neither may this.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::path::PathBuf;

use imagecodec::{ImageError, Limits, bmp, decode, decode_scaled, dimensions};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

/// Every BMP fixture, by name.
fn fixtures() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(data_dir())
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name()?.to_str()?.to_owned();
            name.strip_suffix(".bmp").map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(data_dir().join(format!("{name}.bmp"))).unwrap()
}

/// Chrome's answer: the picture, or `None` for a refusal.
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
fn every_fixture_decodes_or_is_refused_exactly_as_chrome_does() {
    let names = fixtures();
    assert!(
        names.len() >= 68,
        "{} fixtures: some have gone missing",
        names.len()
    );
    let mut refused = 0;
    for name in &names {
        let bytes = read(name);
        assert!(bmp::is_bmp(&bytes), "{name}");
        let ours = decode(&bytes, Limits::default());
        match (answer(name), ours) {
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
    assert!(
        refused >= 12,
        "only {refused} refusals: the refused fixtures are not reaching the decoder"
    );
}

#[test]
fn a_file_cut_short_anywhere_is_refused() {
    // Chrome shows nothing of a BMP it cannot finish, however little is
    // missing -- the last row's padding included. (A run-length stream is
    // done once its last row ends: the two bytes of an end-of-bitmap code
    // after it are never read.)
    for (name, unread) in [
        ("bmp_24", 0),
        ("bmp_8", 0),
        ("bmp_rle8_no_end_code", 0),
        ("bmp_16_565", 0),
        ("bmp_os2_rle24", 2),
    ] {
        let full = read(name);
        for cut in 0..full.len() - unread {
            assert!(
                decode(&full[..cut], Limits::default()).is_err(),
                "{name} cut to {cut} of {}",
                full.len()
            );
        }
    }
}

#[test]
fn no_bit_flip_of_a_bmp_panics() {
    for (name, stride) in [
        ("bmp_rle8_past_the_row", 1),
        ("bmp_rle4_by_hand", 1),
        ("bmp_os2_rle24", 3),
        ("bmp_32_bitfields_10bit", 7),
        ("bmp_16_4444", 5),
        ("bmp_v5_icc_profile", 1),
        ("bmp_core_1", 3),
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
fn a_bmp_past_the_limits_is_refused_before_it_is_decoded() {
    let bytes = read("bmp_8_palette_overlaps_pixels");
    let few = Limits {
        max_pixels: 40 * 30 - 1,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, few),
        Err(ImageError::TooLarge { .. })
    ));
    let tight = Limits {
        max_decompressed_bytes: 40 * 30 * 4 - 1,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, tight),
        Err(ImageError::TooLarge { .. })
    ));
    let exact = Limits {
        max_pixels: 40 * 30,
        max_decompressed_bytes: 40 * 30 * 4,
    };
    assert!(decode(&bytes, exact).is_ok());
}

#[test]
fn a_bmp_thumbnail_fits_its_box() {
    let thumb = decode_scaled(
        &read("bmp_8_palette_overlaps_pixels"),
        Limits::default(),
        20,
        20,
    )
    .unwrap();
    assert_eq!((thumb.width, thumb.height), (20, 15), "40x30 into 20x20");
}

#[test]
fn the_size_is_read_from_the_headers_alone() {
    // A top-down BMP may be taller than a bottom-up one: Chrome takes the
    // height's size limit before it takes its sign. The pixels are not
    // there, and do not need to be.
    let mut header = b"BM".to_vec();
    header.extend_from_slice(&[0; 8]);
    header.extend_from_slice(&54u32.to_le_bytes());
    header.extend_from_slice(&40u32.to_le_bytes());
    header.extend_from_slice(&3i32.to_le_bytes());
    header.extend_from_slice(&(-100_000i32).to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&24u16.to_le_bytes());
    header.extend_from_slice(&[0; 24]);
    assert_eq!(dimensions(&header).unwrap(), (3, 100_000));
    assert!(decode(&header, Limits::default()).is_err(), "and no pixels");
    // The same height bottom-up is refused.
    header[22..26].copy_from_slice(&100_000i32.to_le_bytes());
    assert!(dimensions(&header).is_err());
}

#[test]
fn a_run_length_stream_is_done_once_its_last_row_ends() {
    // What follows the last row's end-of-line -- the end-of-bitmap code
    // here -- is never read, so a file cut there is whole to Chrome.
    let full = read("bmp_rle8");
    let whole = decode(&full, Limits::default()).unwrap();
    assert_eq!(
        decode(&full[..full.len() - 2], Limits::default()).unwrap(),
        whole
    );
    assert!(decode(&full[..full.len() - 3], Limits::default()).is_err());
}
