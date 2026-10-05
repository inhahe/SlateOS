//! Tests for pictures: `data:` URLs read, and `<image>` and `feImage`
//! drawing what they hold.
//!
//! The drawings are 20 by 20 user units on 20 by 20 pixels; the pictures are
//! made here, encoded as PNG by `imagecodec` and carried in base64.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use super::super::SvgDocument;
use super::{base64, data_url};

/// `bytes` in base64, padded.
fn encode64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A `data:` URL of a PNG `w` by `h` whose pixel at `(x, y)` is `paint(x, y)`
/// (`0xAARRGGBB`).
fn png_url(w: u32, h: u32, paint: impl Fn(u32, u32) -> u32) -> String {
    let pixels: Vec<u32> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| paint(x, y))
        .collect();
    let png = imagecodec::encode_png(w, h, &pixels).unwrap();
    format!("data:image/png;base64,{}", encode64(&png))
}

fn draw(body: &str) -> Vec<[u8; 4]> {
    let svg = format!(r#"<svg viewBox="0 0 20 20" width="20" height="20">{body}</svg>"#);
    SvgDocument::parse(&svg)
        .unwrap()
        .render(20, 20)
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

fn at(image: &[[u8; 4]], x: usize, y: usize) -> [u8; 4] {
    image[y * 20 + x]
}

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

// ─── URLs ───────────────────────────────────────────────────────────────────

/// **Base64 decodes with or without padding, across line breaks**, and
/// anything else is refused.
#[test]
fn base64_reads_as_written() {
    assert_eq!(base64(b"SGVsbG8=").unwrap(), b"Hello");
    assert_eq!(base64(b"SGVsbG8").unwrap(), b"Hello");
    assert_eq!(base64(b"SGVs\n bG8=\r\n").unwrap(), b"Hello");
    assert_eq!(base64(b"").unwrap(), b"");
    assert!(base64(b"SGVs*bG8=").is_none(), "not base64");
    assert!(base64(b"SGVsbG8=QQ").is_none(), "data after the padding");
    assert!(base64(b"Q").is_none(), "a length no encoder writes");
    for n in 0..20u8 {
        let bytes: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
        assert_eq!(base64(encode64(&bytes).as_bytes()).unwrap(), bytes, "{n}");
    }
}

/// **A `data:` URL gives its bytes, base64 or percent-encoded**; any other
/// URL, and an SVG picture, gives none.
#[test]
fn data_urls_give_their_bytes() {
    assert_eq!(data_url("data:,A%20b").unwrap(), b"A b");
    assert_eq!(data_url("DATA:text/plain;base64,SGk=").unwrap(), b"Hi");
    assert_eq!(
        data_url(" data:image/png;charset=x;base64,SGk= ").unwrap(),
        b"Hi"
    );
    assert!(data_url("picture.png").is_none());
    assert!(data_url("https://example.com/a.png").is_none());
    assert!(data_url("data:image/svg+xml;base64,PHN2Zy8+").is_none());
    assert!(data_url("data:no-comma").is_none());
}

// ─── <image> ────────────────────────────────────────────────────────────────

/// A 2 by 2 picture: red on the left, blue on the right.
fn halves() -> String {
    png_url(2, 2, |x, _| if x == 0 { 0xFFFF_0000 } else { 0xFF00_00FF })
}

/// **An image draws its picture in its viewport**, scaled to it.
#[test]
fn an_image_fills_its_viewport() {
    let image = draw(&format!(
        r#"<image href="{}" x="2" y="2" width="8" height="8" image-rendering="pixelated"/>"#,
        halves()
    ));
    assert_eq!(at(&image, 3, 5), RED);
    assert_eq!(at(&image, 8, 5), BLUE);
    assert_eq!(at(&image, 1, 5), CLEAR);
    assert_eq!(at(&image, 10, 5), CLEAR);
}

/// **A size not said is the picture's own**, and one side said keeps its
/// proportion.
#[test]
fn an_image_takes_its_own_size() {
    let own = draw(&format!(
        r#"<image href="{}" x="4" y="4" image-rendering="pixelated"/>"#,
        halves()
    ));
    assert_eq!(at(&own, 4, 4), RED);
    assert_eq!(at(&own, 5, 5), BLUE);
    assert_eq!(at(&own, 6, 4), CLEAR);
    let wide = draw(&format!(
        r#"<image href="{}" x="0" y="0" width="10" image-rendering="pixelated"/>"#,
        halves()
    ));
    assert_eq!(at(&wide, 2, 8), RED, "10 high, as it is 10 wide");
    assert_eq!(at(&wide, 2, 11), CLEAR);
}

/// **`preserveAspectRatio` fits the picture in its viewport** -- centred
/// and as large as fits by default; stretched under `none`; covering it,
/// and cut to it, under `slice`.
#[test]
fn an_image_is_fitted_by_its_aspect() {
    let image = |aspect: &str| {
        draw(&format!(
            r#"<image href="{}" x="0" y="0" width="20" height="10" preserveAspectRatio="{aspect}" image-rendering="pixelated"/>"#,
            halves()
        ))
    };
    // Meet: 10 by 10, centred -- from 5 to 15 across.
    let meet = image("xMidYMid meet");
    assert_eq!(at(&meet, 3, 5), CLEAR);
    assert_eq!(at(&meet, 6, 5), RED);
    assert_eq!(at(&meet, 13, 5), BLUE);
    assert_eq!(at(&meet, 16, 5), CLEAR);
    // None: stretched to 20 by 10.
    let none = image("none");
    assert_eq!(at(&none, 1, 5), RED);
    assert_eq!(at(&none, 18, 5), BLUE);
    // Slice: 20 by 20, cut to the viewport's 10 rows.
    let slice = image("xMidYMin slice");
    assert_eq!(at(&slice, 1, 5), RED);
    assert_eq!(at(&slice, 1, 12), CLEAR, "cut to the viewport");
}

/// **A picture is read between its pixels unless told not to**: half-way
/// across a red-to-blue picture drawn wide is a mix of the two.
#[test]
fn an_image_is_smooth_by_default() {
    let image = draw(&format!(
        r#"<image href="{}" x="0" y="0" width="20" height="20"/>"#,
        halves()
    ));
    let middle = at(&image, 10, 10);
    assert!(middle[0] > 40 && middle[2] > 40, "{middle:?}");
    assert_eq!(at(&image, 1, 10), RED, "held to its edge, not wrapped");
}

/// **An image is faded by its opacity, transformed with its element, and
/// cut by its clip.**
#[test]
fn an_image_takes_opacity_transform_and_clip() {
    let faded = draw(&format!(
        r#"<image href="{}" width="20" height="20" opacity="0.5" image-rendering="pixelated"/>"#,
        halves()
    ));
    assert!(at(&faded, 2, 2)[3].abs_diff(128) <= 1);
    let moved = draw(&format!(
        r#"<g transform="translate(10 0)"><image href="{}" width="4" height="4" image-rendering="pixelated"/></g>"#,
        halves()
    ));
    assert_eq!(at(&moved, 10, 1), RED);
    assert_eq!(at(&moved, 1, 1), CLEAR);
    let clipped = draw(&format!(
        r#"<clipPath id="c"><rect width="10" height="20"/></clipPath>
           <image href="{}" width="20" height="20" clip-path="url(#c)" image-rendering="pixelated"/>"#,
        halves()
    ));
    assert_eq!(at(&clipped, 5, 5), RED);
    assert_eq!(at(&clipped, 15, 5), CLEAR);
}

/// **A picture's own transparency is kept.**
#[test]
fn an_images_transparency_is_kept() {
    let url = png_url(1, 1, |_, _| 0x8000_FF00);
    let image = draw(&format!(
        r#"<image href="{url}" width="20" height="20" image-rendering="pixelated"/>"#
    ));
    let px = at(&image, 10, 10);
    assert_eq!((px[0], px[1], px[2]), (0, 255, 0));
    assert!(px[3].abs_diff(128) <= 1, "{px:?}");
}

/// **Nothing is drawn for a picture that is not one**: a file, a URL, an
/// SVG, bytes that decode to nothing, or a viewport with no area.
#[test]
fn an_image_that_is_not_one_draws_nothing() {
    for href in [
        "picture.png",
        "https://example.com/a.png",
        "data:image/png;base64,AAAA",
        "data:image/svg+xml;base64,PHN2Zy8+",
    ] {
        let image = draw(&format!(r#"<image href="{href}" width="20" height="20"/>"#));
        assert!(image.iter().all(|px| *px == CLEAR), "{href}");
    }
    let empty = draw(&format!(
        r#"<image href="{}" width="0" height="20"/>"#,
        halves()
    ));
    assert!(empty.iter().all(|px| *px == CLEAR));
    // xlink:href is read too.
    let xlink = draw(&format!(
        r#"<image xmlns:xlink="http://www.w3.org/1999/xlink" xlink:href="{}" width="20" height="20" image-rendering="pixelated"/>"#,
        halves()
    ));
    assert_eq!(at(&xlink, 2, 2), RED);
}

// ─── feImage ────────────────────────────────────────────────────────────────

/// **An `feImage` naming a picture draws it in its subregion**, fitted by
/// its `preserveAspectRatio`, transparent around it.
#[test]
fn an_fe_image_draws_its_picture() {
    let image = draw(&format!(
        r#"<filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="20">
             <feImage href="{}" x="0" y="0" width="20" height="10" image-rendering="pixelated"/>
           </filter>
           <rect width="20" height="20" fill="lime" filter="url(#f)"/>"#,
        halves()
    ));
    // Met into 20 by 10: 10 by 10, from 5 to 15 across.
    assert_eq!(at(&image, 6, 5), RED);
    assert_eq!(at(&image, 13, 5), BLUE);
    assert_eq!(at(&image, 2, 5), CLEAR);
    assert_eq!(
        at(&image, 6, 15),
        CLEAR,
        "the element is what the filter makes"
    );
}
