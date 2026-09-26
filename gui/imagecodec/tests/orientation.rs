//! EXIF orientation, applied as Chrome applies it.
//!
//! One picture, stored under each orientation as a JPEG and as a PNG
//! (`tests/data/generate_orientation.py`), against Pillow's decode turned by
//! `ImageOps.exif_transpose`; and the cases where Chrome reads the orientation
//! otherwise than Pillow, against Chrome's answer. JPEGs and PNGs alike are
//! compared exactly.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]

mod common;

use imagecodec::orientation::Orientation;
use imagecodec::{Limits, decode, decode_scaled, dimensions, jpeg, png};

fn read_png(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}.png", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn every_orientation_of_a_jpeg_is_shown_as_pillow_shows_it() {
    let upright = decode(&common::read("orient_jpeg_1"), Limits::default()).unwrap();
    for value in 1..=8u16 {
        let name = format!("orient_jpeg_{value}");
        let bytes = common::read(&name);
        let image = decode(&bytes, Limits::default()).unwrap();
        common::assert_exact(&name, &image, &common::answer(&name));
        assert_eq!(
            dimensions(&bytes).unwrap(),
            (image.width, image.height),
            "{name}"
        );
        // The same compressed pixels, so exactly the upright picture turned.
        let orientation = Orientation::from_value(value).unwrap();
        assert_eq!(jpeg::orientation(&bytes), orientation, "{name}");
        assert_eq!(image, orientation.apply(upright.clone()), "{name}");
    }
}

#[test]
fn every_orientation_of_a_png_is_shown_exactly_as_pillow_shows_it() {
    for value in 1..=8u16 {
        let name = format!("orient_png_{value}");
        let bytes = read_png(&name);
        let image = decode(&bytes, Limits::default()).unwrap();
        let want = common::answer(&name);
        assert_eq!(
            (image.width, image.height),
            (want.width, want.height),
            "{name}"
        );
        assert_eq!(image.pixels, want.pixels, "{name}");
        assert_eq!(
            dimensions(&bytes).unwrap(),
            (want.width, want.height),
            "{name}"
        );
        assert_eq!(
            png::orientation(&bytes),
            Orientation::from_value(value).unwrap()
        );
    }
}

#[test]
fn where_chrome_reads_orientation_otherwise_than_pillow_this_reads_it_as_chrome_does() {
    for (name, orientation) in [
        ("orient_jpeg_sub_ifd", Orientation::RightTop),
        ("orient_jpeg_long", Orientation::TopLeft),
        ("orient_jpeg_big_endian", Orientation::LeftBottom),
        ("orient_jpeg_invalid", Orientation::TopLeft),
        ("orient_jpeg_two_exif", Orientation::RightTop),
    ] {
        let bytes = common::read(name);
        assert_eq!(jpeg::orientation(&bytes), orientation, "{name}");
        let image = decode(&bytes, Limits::default()).unwrap();
        common::assert_exact(name, &image, &common::answer(name));
    }
    let bytes = read_png("orient_png_exif_after_idat");
    assert_eq!(png::orientation(&bytes), Orientation::TopLeft);
    let image = decode(&bytes, Limits::default()).unwrap();
    assert_eq!(
        image.pixels,
        common::answer("orient_png_exif_after_idat").pixels
    );
}

#[test]
fn a_thumbnail_fits_its_box_the_way_up_it_is_shown() {
    // Shown 16 wide and 24 tall: in a square box and in one wider than tall
    // alike, the thumbnail is taller than wide and inside the box -- not the
    // stored 24x16 picture fitted to the box and then turned, which would
    // overflow a wide box once turned.
    for bytes in [common::read("orient_jpeg_6"), read_png("orient_png_6")] {
        for (w, h) in [(10, 10), (20, 10)] {
            let thumb = decode_scaled(&bytes, Limits::default(), w, h).unwrap();
            assert!(
                thumb.width <= w && thumb.height <= h,
                "{}x{} in {w}x{h}",
                thumb.width,
                thumb.height
            );
            assert!(
                thumb.height > thumb.width,
                "{}x{}: shown tall",
                thumb.width,
                thumb.height
            );
        }
    }
}
