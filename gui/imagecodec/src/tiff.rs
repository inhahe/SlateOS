//! TIFF, read as libtiff reads it.
//!
//! # Whose TIFF
//!
//! No browser shows a TIFF. The programs that do -- GNOME's image viewer
//! through gdk-pixbuf, and most others on free desktops -- ask libtiff for
//! the picture through its RGBA interface (`TIFFReadRGBAImageOriented`), so
//! that is the reference here: libtiff 4.7.1, built as distributions build
//! it (with zlib, libdeflate and libjpeg), ported rather than reimagined.
//! A file libtiff opens and converts decodes here to the same pixels; a file
//! it refuses is refused. The port covers:
//!
//! - **The directory** ([`dir`]): classic and BigTIFF headers, and
//!   `TIFFReadDirectory`'s rules -- which tags must read cleanly for the
//!   file to open at all, which are dropped when they do not, and its
//!   repairs of what real writers get wrong (a missing or implausible
//!   `StripByteCounts`, colour channels that should have been extra
//!   samples, a palette image with no palette).
//! - **Strips and tiles** ([`read`]): where the bytes are, `FillOrder`, and
//!   the codecs -- none, PackBits, LZW (both styles), Deflate, and CCITT fax
//!   ([`fax`]: Group 3 1-D and 2-D, Group 4, Modified Huffman) -- with the
//!   horizontal predictor and big-endian 16-bit samples.
//! - **Samples to pixels** ([`rgba`]): grey of 1 to 16 bits, palettes, RGB
//!   of 8 and 16 bits with or without alpha, CMYK, `YCbCr` at every
//!   subsampling libtiff converts, and CIE L*a*b*, in contiguous or separate
//!   planes -- the colour conversions ([`color`]) in libtiff's own single
//!   precision, so they agree to the bit.
//!
//! Where libtiff's reader stops at a strip that will not read, this refuses
//! the file: the viewers ask libtiff to stop on the first error, and show
//! nothing.
//!
//! # Orientation
//!
//! The `Orientation` tag is applied truly, all eight values. libtiff's
//! reader applies only its flips, and gdk-pixbuf turns 5 to 8 the rest of
//! the way afterwards; the picture that reaches the screen is the one the
//! tag describes, which is what [`decode`] returns. [`dimensions`] is the
//! size as shown.
//!
//! # Alpha
//!
//! Straight, as everywhere in this crate, and as the file holds it:
//! libtiff premultiplies unassociated alpha into its raster, and hands
//! associated alpha through premultiplied; here the first is kept as it was
//! and the second divided back out. [`decode_libtiff_raster`] gives the
//! raster exactly as libtiff would, for checking against it.
//!
//! # What is not here yet
//!
//! Old-style JPEG, and the rarer codecs (NeXT, ThunderScan, SGI LogLuv,
//! PixarLog), are refused by name. The first page only is read, as libtiff's
//! viewers read it; the others are not reached.

mod color;
mod dir;
mod fax;
mod lzw;
mod read;
mod rgba;

use alloc::vec::Vec;

use crate::orientation::Orientation;
use crate::{Image, ImageError, ImageResult, Limits};

/// Whether `bytes` begins like a TIFF (`II*\0`, `MM\0*`, or a BigTIFF's
/// `+` in place of the `*`).
#[must_use]
pub fn is_tiff(bytes: &[u8]) -> bool {
    dir::is_tiff(bytes)
}

/// The `Orientation` tag of the first image, as libtiff reads it: 1 when
/// absent, or when it names no orientation.
#[must_use]
pub fn orientation(bytes: &[u8]) -> Orientation {
    let read = || -> ImageResult<u16> {
        let (file, offset) = dir::header(bytes)?;
        Ok(dir::read(&file, offset)?.orientation)
    };
    read()
        .ok()
        .and_then(Orientation::from_value)
        .unwrap_or(Orientation::TopLeft)
}

/// The first image's size, as shown -- turned by its orientation.
///
/// # Errors
///
/// [`ImageError::Truncated`] or [`ImageError::Malformed`] for a header or
/// directory libtiff would not open.
pub fn dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    let (file, offset) = dir::header(bytes)?;
    let d = dir::read(&file, offset)?;
    let shown = Orientation::from_value(d.orientation).unwrap_or(Orientation::TopLeft);
    Ok(shown.shown((d.width, d.length)))
}

/// Decode the first image of a TIFF.
///
/// # Errors
///
/// [`ImageError::TooLarge`] past `limits`; otherwise whatever libtiff would
/// refuse the file for, or [`ImageError::Unsupported`] for a kind of TIFF
/// not yet decoded here.
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    let (raster, orientation) = decode_raster(bytes, limits)?;
    let pixels = raster
        .pixels
        .iter()
        .map(|&p| {
            let (r, g, b, a) = (p & 0xFF, (p >> 8) & 0xFF, (p >> 16) & 0xFF, p >> 24);
            let (r, g, b) = if raster.alpha == rgba::Alpha::Associated {
                (
                    unpremultiply(r, a),
                    unpremultiply(g, a),
                    unpremultiply(b, a),
                )
            } else {
                (r, g, b)
            };
            (a << 24) | (r << 16) | (g << 8) | b
        })
        .collect();
    let image = Image {
        width: raster.width,
        height: raster.height,
        pixels,
    };
    Ok(orientation.apply(image))
}

/// Decode the first image scaled to fit `max_w` x `max_h`: decoded whole,
/// then averaged down, as a BMP or GIF is.
///
/// # Errors
///
/// As [`decode`].
pub fn decode_scaled(bytes: &[u8], limits: Limits, max_w: u32, max_h: u32) -> ImageResult<Image> {
    let image = decode(bytes, limits)?;
    if max_w == 0 || max_h == 0 {
        return Ok(image);
    }
    crate::scale::shrink_to_fit(image, max_w, max_h)
}

/// The first image exactly as libtiff's `TIFFReadRGBAImageOriented` returns
/// it with `ORIENTATION_TOPLEFT`: its alpha premultiplied as libtiff
/// premultiplies it, and turned only as libtiff turns -- by flips, 5 to 8
/// read as 1 to 4. The packing is this crate's `0xAARRGGBB`, not libtiff's
/// in-memory `0xAABBGGRR`.
///
/// For checking this port against libtiff itself; a picture to show is
/// [`decode`]'s.
///
/// # Errors
///
/// As [`decode`].
pub fn decode_libtiff_raster(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    let (raster, orientation) = decode_raster(bytes, limits)?;
    let premultiplies = raster.alpha
        == rgba::Alpha::Unassociated {
            libtiff_premultiplies: true,
        };
    let pixels: Vec<u32> = raster
        .pixels
        .iter()
        .map(|&p| {
            let (r, g, b, a) = (p & 0xFF, (p >> 8) & 0xFF, (p >> 16) & 0xFF, p >> 24);
            let (r, g, b) = if premultiplies {
                (
                    rgba::premultiply(r, a),
                    rgba::premultiply(g, a),
                    rgba::premultiply(b, a),
                )
            } else {
                (r, g, b)
            };
            (a << 24) | (r << 16) | (g << 8) | b
        })
        .collect();
    let image = Image {
        width: raster.width,
        height: raster.height,
        pixels,
    };
    // libtiff flips by the orientation's first four; see the module docs.
    let flips = match orientation {
        Orientation::TopLeft | Orientation::LeftTop => Orientation::TopLeft,
        Orientation::TopRight | Orientation::RightTop => Orientation::TopRight,
        Orientation::BottomRight | Orientation::RightBottom => Orientation::BottomRight,
        Orientation::BottomLeft | Orientation::LeftBottom => Orientation::BottomLeft,
    };
    Ok(flips.apply(image))
}

/// The raster, stored orientation, and the orientation to turn it by.
fn decode_raster(bytes: &[u8], limits: Limits) -> ImageResult<(rgba::Raster, Orientation)> {
    let (file, offset) = dir::header(bytes)?;
    let mut d = dir::read(&file, offset)?;
    rgba::jpeg_color_mode(&mut d);
    let pixels = u64::from(d.width).saturating_mul(u64::from(d.length));
    if pixels > limits.max_pixels {
        return Err(ImageError::TooLarge {
            pixels,
            limit: limits.max_pixels,
        });
    }
    // The biggest buffer a strip or tile needs, before any exists.
    let chunk = if d.tiled {
        d.tile_size()
    } else {
        d.strip_size()
    }
    .unwrap_or(u64::MAX);
    let planes: u64 = if d.planar_config == 2 { 4 } else { 1 };
    if chunk.saturating_mul(planes) > limits.max_decompressed_bytes as u64 {
        return Err(ImageError::TooLarge {
            pixels,
            limit: limits.max_pixels,
        });
    }
    let orientation = Orientation::from_value(d.orientation).unwrap_or(Orientation::TopLeft);
    let raster = rgba::read(file, &d, &limits)?;
    Ok((raster, orientation))
}

/// Divide premultiplied colour `c` back out of alpha `a`: the nearest value
/// that premultiplies back to `c` as libtiff premultiplies, and white for
/// colour brighter than its alpha allows.
fn unpremultiply(c: u32, a: u32) -> u32 {
    if a == 0 {
        return if c == 0 { 0 } else { 255 };
    }
    if c >= a {
        return 255;
    }
    // c < a <= 255: no overflow, and a is not zero.
    c.wrapping_mul(255)
        .wrapping_add(a / 2)
        .checked_div(a)
        .unwrap_or(255)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    #[test]
    fn unpremultiplying_then_premultiplying_is_exact_for_every_valid_pair() {
        for a in 1..=255u32 {
            for c in 0..=a {
                let s = unpremultiply(c, a);
                assert!(s <= 255);
                assert_eq!(rgba::premultiply(s, a), c, "c={c} a={a}");
            }
        }
    }
}
