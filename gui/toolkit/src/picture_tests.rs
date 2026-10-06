#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use crate::color::Color;

/// A `width` by `height` picture, every pixel `argb`.
fn solid(width: u32, height: u32, argb: u32) -> Picture {
    let count = (width * height) as usize;
    Picture::new(Image {
        width,
        height,
        pixels: vec![argb; count],
    })
    .expect("a picture")
}

/// **Every picture has a number of its own, and a copy is the same
/// picture**: two made from the same pixels are two pictures, under two
/// numbers far up the id space; a copy has its original's number and is
/// equal to it.
#[test]
fn a_picture_has_a_number_of_its_own() {
    let a = solid(2, 2, 0xFF00_00FF);
    let b = solid(2, 2, 0xFF00_00FF);
    assert_ne!(a.id(), b.id());
    assert_ne!(a, b, "the same pixels are not the same picture");
    assert!(a.id() >= FIRST_ID && b.id() >= FIRST_ID);
    let copy = a.clone();
    assert_eq!(copy.id(), a.id());
    assert_eq!(copy, a);
}

/// **A picture of nothing, or of fewer pixels than its size, is no
/// picture**: no window could be given it.
#[test]
fn a_picture_with_no_pixels_is_refused() {
    let empty = Image {
        width: 0,
        height: 3,
        pixels: Vec::new(),
    };
    assert!(Picture::new(empty).is_none());
    let short = Image {
        width: 2,
        height: 2,
        pixels: vec![0; 3],
    };
    assert!(Picture::new(short).is_none());
    assert!(Picture::from_canvas(&Canvas::empty()).is_none());
}

/// **A picture written as a PNG reads back as itself**, and bytes that are
/// no picture say so rather than making one.
#[test]
fn a_picture_is_written_and_read_again() {
    let mut pixels = vec![0xFF11_2233; 6];
    pixels[4] = 0x8044_5566;
    let picture = Picture::new(Image {
        width: 3,
        height: 2,
        pixels: pixels.clone(),
    })
    .unwrap();
    let png = picture.to_png().expect("encoded");
    let back = Picture::decode(&png).expect("decoded");
    assert_eq!((back.width(), back.height()), (3, 2));
    assert_eq!(back.pixels(), &pixels[..]);
    assert_ne!(back, picture, "read again, it is a picture of its own");
    assert_eq!(
        Picture::decode(b"not a picture at all"),
        Err(ImageError::UnknownFormat)
    );
}

/// **A canvas's colours become a picture's pixels, and a picture's pixels
/// go to a window in the window's byte order**: blue, green, red, alpha.
#[test]
fn a_canvas_becomes_a_picture_a_window_can_take() {
    let canvas = Canvas::filled(2, 1, Color::rgba(0x10, 0x20, 0x30, 0x40));
    let picture = Picture::from_canvas(&canvas).unwrap();
    assert_eq!(picture.pixels(), &[0x4010_2030, 0x4010_2030]);
    assert_eq!(
        picture.wire_bytes().as_slice(),
        &[0x30, 0x20, 0x10, 0x40, 0x30, 0x20, 0x10, 0x40]
    );
}

/// **A picture wider than its box is shown at the box's width, its shape
/// kept; a narrower one at its own size.**
#[test]
fn a_picture_is_fitted_to_its_box() {
    let picture = solid(200, 100, 0xFF00_0000);
    assert_eq!(picture.fitted(100.0), (100.0, 50.0));
    assert_eq!(picture.fitted(300.0), (200.0, 100.0));
    assert_eq!(picture.fitted(200.0), (200.0, 100.0));
    assert_eq!(picture.fitted(0.0), (200.0, 100.0), "no box: its own size");
}

/// **A window is given each picture shown once, and gives each back once
/// nothing shows it -- the giving back first.**
#[test]
fn a_window_is_given_what_is_shown() {
    let (a, b, c) = (
        solid(1, 1, 0xFF00_0000),
        solid(1, 1, 0xFF00_0000),
        solid(1, 1, 0xFF00_0000),
    );
    let mut uploads = Uploads::new();
    assert_eq!(uploads.changes([&a]), [Change::Upload(a.clone())]);
    assert!(uploads.holds(a.id()));
    assert_eq!(uploads.changes([&a]), [], "held already");
    assert_eq!(
        uploads.changes([&a, &b, &b]),
        [Change::Upload(b.clone())],
        "shown twice, given once"
    );
    assert_eq!(uploads.changes([&b]), [Change::Drop(a.id())]);
    assert!(!uploads.holds(a.id()));
    assert_eq!(
        uploads.changes([&c]),
        [Change::Drop(b.id()), Change::Upload(c.clone())],
        "what goes back goes first"
    );
    uploads.forget();
    assert_eq!(
        uploads.changes([&c]),
        [Change::Upload(c.clone())],
        "a window that lost its pictures is given them again"
    );
    assert_eq!(uploads.changes([]), [Change::Drop(c.id())]);
}
