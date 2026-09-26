//! Tests: each container, each tag, and the hostile files.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::cast_possible_truncation
)]

use super::*;

/// A directory entry's value, for the builder: bytes of a type and count, or
/// a pointer to another directory by its place in the list.
enum V {
    Bytes(u16, u32, Vec<u8>),
    Dir(usize),
}

fn u16s(little: bool, values: &[u16]) -> V {
    let bytes = values
        .iter()
        .flat_map(|v| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        })
        .collect();
    V::Bytes(3, values.len() as u32, bytes)
}

fn u32s(little: bool, values: &[u32]) -> V {
    let bytes = values
        .iter()
        .flat_map(|v| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        })
        .collect();
    V::Bytes(4, values.len() as u32, bytes)
}

fn rationals(little: bool, values: &[(u32, u32)]) -> V {
    let bytes = values
        .iter()
        .flat_map(|&(n, d)| {
            let (n, d) = if little {
                (n.to_le_bytes(), d.to_le_bytes())
            } else {
                (n.to_be_bytes(), d.to_be_bytes())
            };
            n.into_iter().chain(d)
        })
        .collect();
    V::Bytes(5, values.len() as u32, bytes)
}

fn srational(little: bool, n: i32, d: i32) -> V {
    let bytes = if little {
        [n.to_le_bytes(), d.to_le_bytes()].concat()
    } else {
        [n.to_be_bytes(), d.to_be_bytes()].concat()
    };
    V::Bytes(10, 1, bytes)
}

fn ascii(text: &[u8]) -> V {
    let mut bytes = text.to_vec();
    bytes.push(0);
    V::Bytes(2, bytes.len() as u32, bytes)
}

/// A TIFF structure holding `dirs`, the first being IFD0.
fn tiff(little: bool, dirs: &[Vec<(u16, V)>]) -> Vec<u8> {
    let w16 = |v: u16| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let w32 = |v: u32| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let mut dir_at = Vec::new();
    let mut at = 8_usize;
    for d in dirs {
        dir_at.push(at);
        at += 2 + 12 * d.len() + 4;
    }
    let data_start = at;
    let mut out: Vec<u8> = if little {
        b"II".to_vec()
    } else {
        b"MM".to_vec()
    };
    out.extend_from_slice(&w16(42));
    out.extend_from_slice(&w32(8));
    let mut data = Vec::new();
    for d in dirs {
        out.extend_from_slice(&w16(d.len() as u16));
        for (tag, value) in d {
            out.extend_from_slice(&w16(*tag));
            match value {
                V::Bytes(kind, count, bytes) => {
                    out.extend_from_slice(&w16(*kind));
                    out.extend_from_slice(&w32(*count));
                    if bytes.len() <= 4 {
                        let mut field = bytes.clone();
                        field.resize(4, 0);
                        out.extend_from_slice(&field);
                    } else {
                        out.extend_from_slice(&w32((data_start + data.len()) as u32));
                        data.extend_from_slice(bytes);
                        if data.len() % 2 == 1 {
                            data.push(0);
                        }
                    }
                }
                V::Dir(i) => {
                    out.extend_from_slice(&w16(4));
                    out.extend_from_slice(&w32(1));
                    out.extend_from_slice(&w32(dir_at[*i] as u32));
                }
            }
        }
        out.extend_from_slice(&w32(0));
    }
    out.extend(data);
    out
}

/// A camera's EXIF, in the byte order asked for.
fn camera(little: bool) -> Vec<u8> {
    tiff(
        little,
        &[
            vec![
                (0x010F, ascii(b"Canon")),
                (0x0110, ascii(b"Canon EOS R5")),
                (0x0112, u16s(little, &[6])),
                (0x0131, ascii(b"Firmware 1.8.1")),
                (0x8769, V::Dir(1)),
                (0x8825, V::Dir(2)),
            ],
            vec![
                (0x829A, rationals(little, &[(10, 2500)])),
                (0x829D, rationals(little, &[(28, 10)])),
                (0x8822, u16s(little, &[3])),
                (0x8827, u16s(little, &[400])),
                (0x9003, ascii(b"2025:06:15 14:30:22")),
                (0x9204, srational(little, -2, 3)),
                (0x9207, u16s(little, &[5])),
                (0x9209, u16s(little, &[0x19])),
                (0x920A, rationals(little, &[(50, 1)])),
                (0xA001, u16s(little, &[1])),
                (0xA002, u32s(little, &[8192])),
                (0xA003, u32s(little, &[5464])),
                (0xA403, u16s(little, &[0])),
                (0xA434, ascii(b"RF 24-70mm F2.8 L IS USM")),
            ],
            vec![
                (1, ascii(b"N")),
                (2, rationals(little, &[(37, 1), (46, 1), (2964, 100)])),
                (3, ascii(b"W")),
                (4, rationals(little, &[(122, 1), (25, 1), (984, 100)])),
                (5, V::Bytes(1, 1, vec![1])),
                (6, rationals(little, &[(16, 1)])),
            ],
        ],
    )
}

fn assert_camera(exif: &ExifData) {
    assert_eq!(exif.camera_make.as_deref(), Some("Canon"));
    assert_eq!(exif.camera_model.as_deref(), Some("Canon EOS R5"));
    assert_eq!(exif.camera().as_deref(), Some("Canon EOS R5"));
    assert_eq!(exif.orientation, Some(6));
    assert_eq!(exif.software.as_deref(), Some("Firmware 1.8.1"));
    assert_eq!(
        exif.shutter_speed.as_deref(),
        Some("1/250"),
        "in lowest terms"
    );
    assert_eq!(exif.aperture, Some(2.8));
    assert_eq!(exif.exposure_program.as_deref(), Some("Aperture priority"));
    assert_eq!(exif.iso, Some(400));
    assert_eq!(exif.date_taken.as_deref(), Some("2025:06:15 14:30:22"));
    assert_eq!(exif.exposure_bias, Some(-2.0 / 3.0));
    assert_eq!(exif.metering_mode.as_deref(), Some("Multi-segment"));
    assert_eq!(exif.flash_fired, Some(true));
    assert_eq!(exif.focal_length_mm, Some(50.0));
    assert_eq!(exif.color_space.as_deref(), Some("sRGB"));
    assert_eq!((exif.width, exif.height), (Some(8192), Some(5464)));
    assert_eq!(exif.white_balance.as_deref(), Some("Auto"));
    assert_eq!(exif.lens.as_deref(), Some("RF 24-70mm F2.8 L IS USM"));
    let lat = exif.gps_latitude.unwrap();
    let lon = exif.gps_longitude.unwrap();
    assert!((lat - 37.7749).abs() < 1e-4, "{lat}");
    assert!((lon + 122.4194).abs() < 1e-4, "west is negative: {lon}");
    assert_eq!(
        exif.gps_altitude,
        Some(-16.0),
        "below sea level is negative"
    );
}

/// A JPEG: SOI, an APP0, the APP1 holding `tiff`, a scan.
fn jpeg_with(tiff: &[u8]) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 16];
    out.extend_from_slice(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
    out.extend_from_slice(&[0xFF, 0xE1]);
    let len = (2 + 6 + tiff.len()) as u16;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(tiff);
    out.extend_from_slice(&[0xFF, 0xDA, 0, 2, 0x12, 0x34, 0xFF, 0xD9]);
    out
}

/// **A JPEG's EXIF is read from its APP1 segment**, little-endian and
/// big-endian alike: every tag this reads.
#[test]
fn a_jpeg_camera_file_is_read_whole() {
    for little in [true, false] {
        let exif = read(&jpeg_with(&camera(little)));
        assert_camera(&exif);
    }
}

/// `Exif\0\0` inside the scan is picture data, not EXIF: it was searched for
/// anywhere in the file.
#[test]
fn exif_bytes_in_the_picture_data_are_not_exif() {
    let mut data = vec![0xFF, 0xD8, 0xFF, 0xDA, 0, 2];
    data.extend_from_slice(b"Exif\0\0");
    data.extend_from_slice(&camera(true));
    data.extend_from_slice(&[0xFF, 0xD9]);
    assert!(read(&data).is_empty());
}

fn png_chunk(kind: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = (body.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

/// A PNG's EXIF is its `eXIf` chunk -- before the image data, where Chrome
/// reads it, and not after.
#[test]
fn a_png_exif_chunk_before_the_image_data_is_read() {
    let signature = b"\x89PNG\r\n\x1a\n".to_vec();
    let ihdr = png_chunk(b"IHDR", &[0; 13]);
    let exif = png_chunk(b"eXIf", &camera(false));
    let idat = png_chunk(b"IDAT", &[1, 2, 3]);
    let before = [signature.clone(), ihdr.clone(), exif.clone(), idat.clone()].concat();
    assert_camera(&read(&before));
    let after = [signature, ihdr, idat, exif].concat();
    assert!(read(&after).is_empty(), "read after the image data");
}

/// A WebP's `EXIF` chunk, with or without the JPEG-style prefix some writers
/// leave on it; odd-length chunks before it are padded.
#[test]
fn a_webp_exif_chunk_is_read() {
    for prefix in [&b""[..], &b"Exif\0\0"[..]] {
        let body = [prefix, &camera(true)].concat();
        let mut data = b"RIFF\0\0\0\0WEBP".to_vec();
        data.extend_from_slice(b"VP8X");
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&[1, 2, 3, 0]);
        data.extend_from_slice(b"EXIF");
        data.extend_from_slice(&(body.len() as u32).to_le_bytes());
        data.extend_from_slice(&body);
        assert_camera(&read(&data));
    }
}

/// A TIFF file is its own structure.
#[test]
fn a_tiff_file_is_read_as_it_is() {
    assert_camera(&read(&camera(false)));
    assert!(read(b"not a picture at all").is_empty());
}

/// **A directory pointing at itself is read once**: the recursion that
/// crashed the photo manager on import.
#[test]
fn a_directory_pointing_at_itself_is_read_once() {
    let data = tiff(
        true,
        &[vec![(0x010F, ascii(b"Canon")), (0x8769, V::Dir(0))]],
    );
    assert_eq!(read(&data).camera_make.as_deref(), Some("Canon"));
    // Nor is an Exif pointer followed from the Exif directory.
    let deeper = tiff(
        true,
        &[
            vec![(0x8769, V::Dir(1))],
            vec![(0x8827, u16s(true, &[100])), (0x8769, V::Dir(2))],
            vec![(0x010F, ascii(b"Deeper"))],
        ],
    );
    let exif = read(&deeper);
    assert_eq!(exif.iso, Some(100));
    assert_eq!(exif.camera_make, None, "a pointer below IFD0 was followed");
}

/// A value that runs past the end is left out; so is one of the wrong type;
/// an orientation outside 1 to 8 is no orientation.
#[test]
fn a_value_out_of_bounds_or_of_the_wrong_type_is_left_out() {
    let mut data = tiff(
        true,
        &[vec![
            (0x010F, u16s(true, &[7])),
            (0x0112, u16s(true, &[9])),
            (0x0110, ascii(b"A model name longer than four bytes")),
        ]],
    );
    // Cut the model's text off.
    data.truncate(data.len() - 10);
    let exif = read(&data);
    assert_eq!(exif.camera_make, None, "a number was taken for text");
    assert_eq!(exif.orientation, None);
    assert_eq!(exif.camera_model, None, "text past the end was read");
}

/// Text that is not UTF-8 is shown by its bytes, never decoded lossily.
#[test]
fn text_that_is_not_utf8_is_shown_by_its_bytes() {
    let exif = read(&tiff(true, &[vec![(0x8298, ascii(b"\xA9 Ren\xE9"))]]));
    let shown = exif.copyright.expect("the copyright");
    assert!(!shown.contains('\u{FFFD}'), "{shown}");
    assert!(shown.contains("Ren"), "{shown}");
}

/// An exposure of a second or more is written in seconds.
#[test]
fn a_long_exposure_is_written_in_seconds() {
    let exif = read(&tiff(
        true,
        &[
            vec![(0x8769, V::Dir(1))],
            vec![(0x829A, rationals(true, &[(5, 2)]))],
        ],
    ));
    assert_eq!(exif.shutter_speed.as_deref(), Some("2.5"));
    assert_eq!(shutter(0, 1), None);
    assert_eq!(shutter(1, 0), None);
    assert_eq!(shutter(3, 3000).as_deref(), Some("1/1000"));
}

/// The camera's name: the model alone when it already says the maker.
#[test]
fn the_camera_is_named_once() {
    let named = |make: Option<&str>, model: Option<&str>| {
        ExifData {
            camera_make: make.map(str::to_owned),
            camera_model: model.map(str::to_owned),
            ..ExifData::empty()
        }
        .camera()
    };
    assert_eq!(
        named(Some("NIKON CORPORATION"), Some("NIKON D850")).as_deref(),
        Some("NIKON D850")
    );
    assert_eq!(
        named(Some("Apple"), Some("iPhone 12")).as_deref(),
        Some("Apple iPhone 12")
    );
    assert_eq!(named(None, Some("X100V")).as_deref(), Some("X100V"));
    assert_eq!(named(None, None), None);
}

/// Hostile structures end in nothing, not a panic.
#[test]
fn damaged_files_are_read_as_nothing_without_a_panic() {
    let whole = jpeg_with(&camera(true));
    for cut in 0..whole.len() {
        let _ = read(&whole[..cut]);
    }
    for at in 0..whole.len() {
        let mut bent = whole.clone();
        bent[at] ^= 0xFF;
        let _ = read(&bent);
    }
}
