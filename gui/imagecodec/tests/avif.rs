//! AVIF against libavif, through Pillow: the container, and the pixels.
//!
//! The container fixtures are libavif's own test files
//! (`tests/data/generate_avif.py` says which and why): between them they reach
//! each rule of the container that decides whether a file opens -- items in
//! `idat`, a `meta` box of size 0, layered pictures, grids with and without
//! alpha grids, tone-mapped HDR items whose metadata libavif does and does not
//! read, image sequences -- and the files libavif refuses, for its reason.
//! Each is held to Pillow's parse of it: whether it opens, its size as shown
//! (turned by `irot` and `imir`, as Chrome turns it) and whether it has alpha.
//!
//! The pixel fixtures (`avifpx_*`, from `tests/data/generate_avif_pixels.py`)
//! are made here, one for each way libavif converts a decoded picture to RGB,
//! and each is held to Pillow's pixels byte for byte.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use imagecodec::{ImageError, Limits, avif, decode, dimensions, pixel_format};

fn read(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}.avif", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Pillow's parse of one fixture.
enum Answer {
    Opens {
        width: u32,
        height: u32,
        alpha: bool,
        orientation: u8,
    },
    /// libavif's name for its refusal.
    Refused(String),
}

fn answers() -> Vec<(String, Answer)> {
    let path = format!(
        "{}/tests/data/avif_container.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines()
        .map(|line| {
            let mut words = line.split(' ');
            let name = words.next().unwrap().to_owned();
            let answer = match words.next().unwrap() {
                "ok" => {
                    let (width, height) = words.next().unwrap().split_once('x').unwrap();
                    let alpha = words.next().unwrap() == "RGBA";
                    let _frames = words.next().unwrap();
                    Answer::Opens {
                        width: width.parse().unwrap(),
                        height: height.parse().unwrap(),
                        alpha,
                        orientation: words.next().unwrap().parse().unwrap(),
                    }
                }
                _ => Answer::Refused(words.collect::<Vec<_>>().join(" ")),
            };
            (name, answer)
        })
        .collect()
}

#[test]
fn every_fixture_is_read_or_refused_as_libavif_reads_or_refuses_it() {
    let answers = answers();
    assert_eq!(answers.len(), 25);
    for (name, answer) in answers {
        let bytes = read(&name);
        assert!(avif::is_avif(&bytes), "{name}");
        match answer {
            Answer::Opens {
                width,
                height,
                alpha,
                orientation,
            } => {
                // None of the fixtures has a crop Chrome would apply; a turn
                // by a quarter swaps the sides.
                let shown = if (5..=8).contains(&orientation) {
                    (height, width)
                } else {
                    (width, height)
                };
                assert_eq!(dimensions(&bytes), Ok(shown), "{name}");
                let format = pixel_format(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(format.has_alpha, alpha, "{name}");
            }
            Answer::Refused(result) => {
                let got = dimensions(&bytes).expect_err(&name);
                let expected = match result.as_str() {
                    "BMFF parsing failed"
                    | "Invalid image grid"
                    | "Invalid tone mapped image item" => {
                        matches!(got, ImageError::Malformed(_))
                    }
                    "Not implemented" => matches!(got, ImageError::Unsupported(_)),
                    "Truncated data" => got == ImageError::Truncated,
                    other => panic!("{name}: an answer this test does not know: {other}"),
                };
                assert!(expected, "{name}: libavif says {result}, this says {got:?}");
            }
        }
    }
}

#[test]
fn the_depth_and_the_channels_are_the_av1_configuration_s() {
    // A 12-bit sequence with an alpha track; an 8-bit picture without alpha.
    let format = pixel_format(&read("avif_colors_animated_12bpc_keyframes_0_2_3")).unwrap();
    assert_eq!(
        (format.bits_per_channel, format.channels, format.has_alpha),
        (12, 4, true)
    );
    let format = pixel_format(&read("avif_white_1x1")).unwrap();
    assert_eq!(
        (format.bits_per_channel, format.channels, format.has_alpha),
        (8, 3, false)
    );
}

#[test]
fn a_file_cut_short_is_truncated_or_malformed_never_misread() {
    let bytes = read("avif_draw_points_idat");
    for len in 0..bytes.len() {
        let cut = &bytes[..len];
        if let Ok(size) = dimensions(cut) {
            // Everything the parse needs may lie before the cut, as long as
            // it is the same answer.
            assert_eq!(size, (33, 11), "cut at {len}");
        }
    }
}

#[test]
fn a_damaged_fixture_is_refused_or_read_but_never_panics() {
    for name in [
        "avif_draw_points_idat",
        "avif_draw_points_idat_progressive",
        "avif_clop_irot_imor",
        "avif_circle_custom_properties",
        "avif_colors_animated_8bpc",
        "avif_supported_gainmap_writer_version_with_extra_bytes",
        "avif_color_grid_alpha_nogrid",
        "avif_color_grid_alpha_grid_gainmap_nogrid",
        "avif_colors_animated_8bpc_audio",
    ] {
        let bytes = read(name);
        let mut damaged = bytes.clone();
        for at in 0..bytes.len() {
            for bit in 0..8 {
                damaged[at] ^= 1 << bit;
                let _ = dimensions(&damaged);
                let _ = pixel_format(&damaged);
                damaged[at] ^= 1 << bit;
            }
        }
        for len in 0..bytes.len() {
            let _ = pixel_format(&bytes[..len]);
        }
    }
}

/// One line of `avif_pixels.txt`: a fixture and what Pillow made of it.
#[cfg(feature = "avif")]
struct PixelAnswer {
    name: String,
    /// `None` when libavif refused to convert it.
    pixels: Option<(bool, u32, u32, u32)>,
}

#[cfg(feature = "avif")]
fn pixel_answers() -> Vec<PixelAnswer> {
    let path = format!("{}/tests/data/avif_pixels.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let words: Vec<&str> = line.split('\t').collect();
            let name = words[0].trim_end_matches(".avif").to_owned();
            let pixels = match words[1] {
                "refused" => None,
                mode => {
                    let (width, height) = words[2].split_once('x').unwrap();
                    Some((
                        mode == "RGBA",
                        width.parse().unwrap(),
                        height.parse().unwrap(),
                        u32::from_str_radix(words[3], 16).unwrap(),
                    ))
                }
            };
            PixelAnswer { name, pixels }
        })
        .collect()
}

/// The pixels as Pillow's bytes: R, G, B, and A when `alpha`.
#[cfg(feature = "avif")]
fn pillow_bytes(pixels: &[u32], alpha: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for &px in pixels {
        let [b, g, r, a] = px.to_le_bytes();
        out.extend_from_slice(&[r, g, b]);
        if alpha {
            out.push(a);
        }
    }
    out
}

/// Every conversion path libavif has for an 8-bit BGRA destination -- libyuv's
/// for BT.601, BT.709 and BT.2020 at each depth and subsampling, libavif's own
/// floating point for the identity, FCC, SMPTE 240, YCgCo and
/// chromaticity-derived matrices, alpha carried by libyuv or rescaled by
/// libavif, premultiplied alpha undone -- held to Pillow's pixels, byte for
/// byte; and the matrices libavif will not convert, refused.
#[cfg(feature = "avif")]
#[test]
fn every_pixel_fixture_decodes_to_pillow_s_pixels() {
    let answers = pixel_answers();
    assert_eq!(answers.len(), 39);
    for answer in answers {
        let name = &answer.name;
        let bytes = read(name);
        let decoded = decode(&bytes, Limits::default());
        match answer.pixels {
            Some((alpha, width, height, crc)) => {
                let image = decoded.unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!((image.width, image.height), (width, height), "{name}");
                let ours = crc32::crc32(&pillow_bytes(&image.pixels, alpha));
                assert_eq!(ours, crc, "{name}: pixels differ from Pillow's");
                if !alpha {
                    assert!(image.pixels.iter().all(|px| px >> 24 == 0xff), "{name}");
                }
            }
            None => {
                assert!(
                    matches!(decoded, Err(ImageError::Unsupported(_))),
                    "{name}: libavif refuses to convert it, this says {decoded:?}"
                );
            }
        }
    }
}

/// Damage reaching the AV1 data as well as the boxes: every byte of a still
/// picture, a picture in `idat`, a cropped and turned one, a sequence, a grid
/// with alpha, a deep one and a premultiplied one, inverted in turn, then cut
/// short at every length. Each must decode or be refused -- never panic, and
/// never produce pixels that disagree with its own size.
#[cfg(feature = "avif")]
#[test]
fn a_damaged_file_decodes_or_is_refused_but_never_panics() {
    for name in [
        "avif_white_1x1",
        "avif_draw_points_idat",
        "avif_clap_irot_imir_non_essential",
        "avif_colors_animated_8bpc",
        "avif_color_grid_alpha_nogrid",
        "avifpx_10_422a_2020_limited",
        "avifpx_8_420a_709_prem",
    ] {
        let bytes = read(name);
        let check = |data: &[u8], what: &str| {
            if let Ok(image) = decode(data, Limits::default()) {
                assert_eq!(
                    image.pixels.len(),
                    image.width as usize * image.height as usize,
                    "{name}: {what}"
                );
            }
        };
        let mut damaged = bytes.clone();
        for at in 0..bytes.len() {
            damaged[at] ^= 0xff;
            check(&damaged, &format!("byte {at} inverted"));
            damaged[at] ^= 0xff;
        }
        for len in 0..bytes.len() {
            check(&bytes[..len], &format!("cut at {len}"));
        }
    }
}

#[test]
fn a_picture_past_the_caller_s_limit_is_refused_before_decoding() {
    let limits = Limits {
        max_pixels: 1024 * 770 - 1,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&read("avif_sofa_grid1x5_420"), limits),
        Err(ImageError::TooLarge { .. })
    ));
}

/// Built without the `avif` feature, the container is still read -- the
/// other tests here pass unchanged -- and decoding is refused by name.
#[cfg(not(feature = "avif"))]
#[test]
fn without_the_avif_feature_decoding_is_refused_by_name() {
    assert_eq!(
        decode(&read("avif_white_1x1"), Limits::default()),
        Err(ImageError::Unsupported("AVIF decoding"))
    );
}
