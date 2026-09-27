//! `pixel_format`: how each format's headers say its pixels are stored.
//!
//! The expectations were checked against answers this crate did not compute:
//! Pillow's mode and transparency for each fixture that Pillow opens (`1`,
//! `L`, `I;16`, `LA`, `P`, `RGB`, `RGBA`, `CMYK`), and for the bit depths
//! Pillow's modes do not keep -- a PNG's `IHDR`, a JPEG's `SOF`, a BMP's
//! `biBitCount` and compression, a GIF's screen descriptor -- the header bytes
//! read by hand.

#![allow(clippy::unwrap_used, clippy::panic)]

use imagecodec::{ColourModel, ImageError, PixelFormat, dimensions, pixel_format};

use ColourModel::{Cmyk, Colour, Grey};

/// A fixture's bytes, from `tests/data/<name>` (extension included).
fn read(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A format, as the table spells it.
const fn f(
    bits_per_channel: u8,
    bits_per_pixel: u32,
    channels: u16,
    model: ColourModel,
    palette: bool,
    has_alpha: bool,
) -> PixelFormat {
    PixelFormat {
        bits_per_channel,
        bits_per_pixel,
        channels,
        model,
        palette,
        has_alpha,
    }
}

const YES: bool = true;
const NO: bool = false;

/// Every fixture, and what its headers say.
const TABLE: &[(&str, PixelFormat)] = &[
    // PNG: IHDR's depth and colour type, and tRNS before IDAT.
    ("gray1.png", f(1, 1, 1, Grey, NO, NO)),
    ("gray16.png", f(16, 16, 1, Grey, NO, NO)),
    ("graya8.png", f(8, 16, 2, Grey, NO, YES)),
    ("palette4.png", f(4, 4, 1, Colour, YES, NO)),
    ("palette8_trns.png", f(8, 8, 1, Colour, YES, YES)),
    ("rgb8.png", f(8, 24, 3, Colour, NO, NO)),
    ("rgba8.png", f(8, 32, 4, Colour, NO, YES)),
    // JPEG: SOF's precision and components, as libjpeg names them.
    ("jpeggrey_baseline.jpg", f(8, 8, 1, Grey, NO, NO)),
    ("jpeg420_baseline.jpg", f(8, 24, 3, Colour, NO, NO)),
    ("jpegll_grey_psv1.jpg", f(8, 8, 1, Grey, NO, NO)),
    ("jpegll_cmyk.jpg", f(8, 32, 4, Cmyk, NO, NO)),
    // GIF: always a palette; the global table's size.
    ("gif_photo.gif", f(8, 8, 1, Colour, YES, NO)),
    ("gif_four.gif", f(2, 2, 1, Colour, YES, NO)),
    ("gif_anim_clear.gif", f(2, 2, 1, Colour, YES, YES)),
    // Without a global table, the first image's own table; without either,
    // the first image's LZW code size.
    ("gif_local_palettes.gif", f(2, 2, 1, Colour, YES, NO)),
    ("gif_no_palette.gif", f(8, 8, 1, Colour, YES, NO)),
    // BMP: a palette of the bit count, or the masks' widths.
    ("bmp_1.bmp", f(1, 1, 1, Colour, YES, NO)),
    ("bmp_24.bmp", f(8, 24, 3, Colour, NO, NO)),
    ("bmp_16_565.bmp", f(6, 16, 3, Colour, NO, NO)),
    ("bmp_16_555.bmp", f(5, 15, 3, Colour, NO, NO)),
    ("bmp_32_alphabitfields.bmp", f(8, 32, 4, Colour, NO, YES)),
    (
        "bmp_16_1555_alphabitfields.bmp",
        f(5, 16, 4, Colour, NO, YES),
    ),
    // WebP: eight-bit colour, alpha as WebPGetFeatures finds it.
    ("webp_lossy_simple.webp", f(8, 24, 3, Colour, NO, NO)),
    ("webp_lossy_alpha.webp", f(8, 32, 4, Colour, NO, YES)),
    ("webp_lossless_alpha.webp", f(8, 32, 4, Colour, NO, YES)),
    ("webp_lossless_photo.webp", f(8, 24, 3, Colour, NO, NO)),
    // ICO: the best image's bitmap, with its mask; or its PNG.
    ("ico_bmp_1.ico", f(1, 1, 1, Colour, YES, YES)),
    ("ico_bmp_24.ico", f(8, 24, 3, Colour, NO, YES)),
    ("ico_bmp_32_alpha.ico", f(8, 32, 4, Colour, NO, YES)),
    ("ico_bmp_565.ico", f(6, 16, 3, Colour, NO, YES)),
    ("ico_pillow_png.ico", f(8, 32, 4, Colour, NO, YES)),
    // TIFF: BitsPerSample and SamplesPerPixel, the photometric
    // interpretation, and the alpha ExtraSamples gives.
    ("tiff_cmyk8.tif", f(8, 32, 4, Cmyk, NO, NO)),
    // CMYK and an unassociated alpha sample (ExtraSamples 2).
    ("tiff_cmyk8_five_samples.tif", f(8, 40, 5, Cmyk, NO, YES)),
    ("tiff_grey1.tif", f(1, 1, 1, Grey, NO, NO)),
    ("tiff_grey16.tif", f(16, 16, 1, Grey, NO, NO)),
    ("tiff_fax_g4_no_photometric.tif", f(1, 1, 1, Grey, NO, NO)),
    (
        "tiff_pillow_p_adobe_deflate.tif",
        f(8, 8, 1, Colour, YES, NO),
    ),
    (
        "tiff_pillow_rgb_adobe_deflate.tif",
        f(8, 24, 3, Colour, NO, NO),
    ),
    (
        "tiff_pillow_rgba_adobe_deflate.tif",
        f(8, 32, 4, Colour, NO, YES),
    ),
    ("tiff_corel_alpha_999.tif", f(8, 32, 4, Colour, NO, YES)),
];

#[test]
fn every_format_reports_how_its_pixels_are_stored() {
    let mut wrong = Vec::new();
    for &(name, want) in TABLE {
        match pixel_format(&read(name)) {
            Ok(got) if got == want => {}
            got => wrong.push(format!("{name}: {got:?}, want {want:?}")),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A picture `pixel_format` reads is one `dimensions` reads -- both read the
/// same headers -- and what is not a picture, or not enough of one, is an
/// error to both.
#[test]
fn it_reads_what_dimensions_reads() {
    for &(name, _) in TABLE {
        let bytes = read(name);
        assert!(dimensions(&bytes).is_ok(), "{name}");
    }
    assert!(matches!(
        pixel_format(b"not a picture"),
        Err(ImageError::UnknownFormat)
    ));
    let cut = read("gif_truncated.gif");
    let head = cut.get(..8).unwrap();
    assert!(pixel_format(head).is_err());
    assert!(dimensions(head).is_err());
}
