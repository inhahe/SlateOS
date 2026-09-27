//! Tests: every picture written is read back, pixel for pixel, by the
//! decoder the system uses.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use super::*;

/// An opaque picture whose every pixel differs from its neighbours.
fn opaque(w: u32, h: u32) -> Vec<u32> {
    (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                0xFF00_0000
                    | (((x * 7) % 256) << 16)
                    | (((y * 11) % 256) << 8)
                    | (((x + y) * 3) % 256)
            })
        })
        .collect()
}

fn decoded(png: &[u8]) -> imagecodec::Image {
    imagecodec::decode(png, imagecodec::Limits::default()).expect("the PNG decodes")
}

/// The chunks of a PNG: kind and data, each CRC checked.
fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(&png[..8], SIGNATURE);
    let mut out = Vec::new();
    let mut at = 8;
    while at < png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = png[at + 4..at + 8].try_into().unwrap();
        let data = png[at + 8..at + 8 + len].to_vec();
        let crc = u32::from_be_bytes(png[at + 8 + len..at + 12 + len].try_into().unwrap());
        assert_eq!(
            crc,
            crc32::crc32(&png[at + 4..at + 8 + len]),
            "{kind:?}'s CRC"
        );
        out.push((kind, data));
        at += 12 + len;
    }
    out
}

/// The filter byte of each row.
fn filters(png: &[u8], row_len: usize) -> Vec<u8> {
    let idat: Vec<u8> = chunks(png)
        .into_iter()
        .filter(|(k, _)| k == b"IDAT")
        .flat_map(|(_, d)| d)
        .collect();
    let raw = deflate::zlib_inflate(&idat).expect("the IDAT inflates");
    raw.chunks(row_len + 1).map(|row| row[0]).collect()
}

/// **An opaque picture comes back as it went**, written as RGB.
#[test]
fn an_opaque_picture_round_trips_as_rgb() {
    let pixels = opaque(37, 23);
    let png = encode(37, 23, &pixels).unwrap();
    let back = decoded(&png);
    assert_eq!((back.width, back.height), (37, 23));
    assert_eq!(back.pixels, pixels);
    let (kind, ihdr) = &chunks(&png)[0];
    assert_eq!(kind, b"IHDR");
    assert_eq!(&ihdr[8..13], &[8, 2, 0, 0, 0], "8-bit RGB, not interlaced");
}

/// **Transparency comes back as it went**, straight alpha, written as RGBA.
#[test]
fn a_transparent_picture_round_trips_as_rgba() {
    let mut pixels = opaque(20, 10);
    pixels[3] = 0x8012_3456;
    pixels[50] = 0x0000_0000;
    pixels[51] = 0x01FF_FFFF;
    let png = encode(20, 10, &pixels).unwrap();
    assert_eq!(decoded(&png).pixels, pixels);
    assert_eq!(&chunks(&png)[0].1[8..10], &[8, 6], "8-bit RGBA");
}

/// A pixel wide, a pixel tall, and one pixel.
#[test]
fn thin_pictures_round_trip() {
    for (w, h) in [(1, 1), (257, 1), (1, 257)] {
        let pixels = opaque(w, h);
        assert_eq!(
            decoded(&encode(w, h, &pixels).unwrap()).pixels,
            pixels,
            "{w}x{h}"
        );
    }
}

/// Each row is filtered as libpng's heuristic would: along a gradient, by
/// the left neighbour; under a row the same as it, by the row above.
#[test]
fn each_row_is_filtered_by_what_it_resembles() {
    // Every row the same smooth ramp: the first row is best by Sub, every
    // other by Up (all zeros).
    let ramp: Vec<u32> = (0..64u32)
        .flat_map(|_| (0..64u32).map(|x| 0xFF00_0000 | ((x * 4) << 16) | ((x * 4) << 8) | (x * 4)))
        .collect();
    let png = encode(64, 64, &ramp).unwrap();
    let used = filters(&png, 64 * 3);
    assert_eq!(used[0], 1, "the first row by Sub");
    assert!(used[1..].iter().all(|&f| f == 2), "{used:?}");
    assert_eq!(decoded(&png).pixels, ramp);
    // A noisy picture exercises the other three; whatever they choose, it
    // must decode.
    let noisy: Vec<u32> = (0..40 * 40u32)
        .map(|i| 0xFF00_0000 | (i.wrapping_mul(2_654_435_761) >> 8))
        .collect();
    assert_eq!(decoded(&encode(40, 40, &noisy).unwrap()).pixels, noisy);
}

/// Every filter, on its own, is undone by the decoder's rule: the heuristic
/// cannot hide a wrong filter by never choosing it.
#[test]
fn every_filter_inverts() {
    let row: Vec<u8> = (0..30u8).map(|i| i.wrapping_mul(37)).collect();
    let above: Vec<u8> = (0..30u8).map(|i| i.wrapping_mul(91)).collect();
    for filter in 0..=4 {
        let mut out = vec![0; 30];
        apply_filter(filter, &row, &above, 3, &mut out);
        // Undo it as a decoder does.
        let mut back = vec![0_u8; 30];
        for i in 0..30 {
            let a = if i >= 3 { back[i - 3] } else { 0 };
            let b = above[i];
            let c = if i >= 3 { above[i - 3] } else { 0 };
            let predicted = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => u16::midpoint(u16::from(a), u16::from(b)) as u8,
                _ => paeth(a, b, c),
            };
            back[i] = out[i].wrapping_add(predicted);
        }
        assert_eq!(back, row, "filter {filter}");
    }
    assert_eq!(paeth(10, 20, 15), 15, "p = 15, nearest is above-left");
    assert_eq!(paeth(10, 20, 10), 20);
    assert_eq!(paeth(20, 10, 10), 20);
    // Above and above-left equally near: above.
    assert_eq!(paeth(10, 40, 20), 40);
}

/// A flat picture compresses to almost nothing.
#[test]
fn a_flat_picture_is_small() {
    let png = encode(256, 256, &vec![0xFF33_6699; 256 * 256]).unwrap();
    assert!(png.len() < 2048, "{} bytes", png.len());
}

/// No picture, a wrong count of pixels, a side past PNG's limit: refused.
#[test]
fn a_picture_that_cannot_be_a_png_is_refused() {
    assert_eq!(encode(0, 5, &[]), Err(EncodeError::Empty));
    assert_eq!(encode(5, 0, &[]), Err(EncodeError::Empty));
    assert_eq!(
        encode(2, 2, &[0; 3]),
        Err(EncodeError::WrongLength {
            expected: 4,
            got: 3
        })
    );
    assert_eq!(
        encode(2, 2, &[0; 5]),
        Err(EncodeError::WrongLength {
            expected: 4,
            got: 5
        })
    );
    assert_eq!(encode(MAX_SIDE + 1, 1, &[]), Err(EncodeError::TooLarge));
}
