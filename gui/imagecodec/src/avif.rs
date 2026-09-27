//! AVIF (AV1 Image File Format): an AV1 video key frame -- or a grid of them,
//! or a sequence -- in a HEIF container.
//!
//! # What this reads, and whose rules
//!
//! The container is read as libavif 1.3.0 reads it (`avif/container.rs`,
//! `avif/movie.rs`, `avif/setup.rs`), because libavif is the reader behind
//! Pillow and GNOME, and Chrome's reader is a line-by-line Rust port of it:
//! every rule it applies is applied here, in its order, so a file this shows
//! is a file they show and a file they refuse is refused for their reason.
//!
//! Where libavif leaves a choice to the program using it, the choice is
//! Chrome's, since a browser is where most AVIF files are seen:
//!
//! * `pixi` may be missing (libheif before 1.12 did not write it), and a
//!   `clap` crop that does not work out is ignored rather than refused --
//!   Pillow makes both choices too;
//! * Exif and XMP are not read, so a damaged Exif block costs nothing (Pillow
//!   reads them, and refuses the picture);
//! * the picture is shown turned and mirrored as its `irot` and `imir` say,
//!   and cropped as its `clap` says when the crop starts at the top left --
//!   Chrome's rule against crops that hide part of a picture from a viewer
//!   who then shares the file (Pillow passes the turn on as an Exif
//!   orientation and does not crop).
//!
//! The file's colour (`colr`) is found as libavif finds it -- from the
//! container, or from the AV1 sequence header when the container is silent
//! -- and used to convert YUV to RGB. It is not otherwise applied: like the
//! rest of this crate, AVIF has no colour management to feed yet.
//!
//! # Decoding, and the pixels
//!
//! The AV1 frames are decoded by rav1d, the Rust port of dav1d -- the decoder
//! libavif, Chrome and Pillow use -- driven as libavif drives dav1d
//! (`avif/decode.rs`): the same settings, the same send-and-drain loop, a
//! grid's tiles checked and put together by libavif's rules. A frame whose
//! size, depth or chroma layout is not the container's is refused, as Chrome
//! refuses it.
//!
//! The conversion to pixels is libavif's `avifImageYUVToRGB` as Chrome calls
//! it -- 8-bit BGRA, straight alpha, libavif's default upsampling -- with
//! libyuv's fixed-point arithmetic where libavif hands the picture to libyuv
//! and libavif's floating point where it does not (`avif/convert.rs`,
//! `avif/libyuv.rs`). Held to Pillow's decode of the same files, it agrees to
//! the bit wherever Pillow takes the same path: every picture with alpha, and
//! every 8-bit colour one (without alpha, Pillow asks libavif for 24-bit RGB,
//! which libyuv converts from deep or grey pictures by other routes).
//!
//! Not yet: a sequence decodes to its first frame only, and a frame coded at
//! another size than its item's `ispe` -- which libavif rescales with libyuv's
//! box filter -- is refused as unsupported.
//!
//! # Hostile input
//!
//! Every box is bounded by its parent before it is read, and the parsers
//! allocate only in proportion to bytes the file actually contains -- where
//! libavif would materialise a million samples from a dozen bytes of table,
//! or 65535 empty extents per item, this walks or collapses them. A picture's
//! size is checked against libavif's own limits (16384 x 16384 pixels, 32768
//! on a side) as libavif checks it, and against [`Limits`] before anything
//! the size implies is allocated.
//!
//! Portions of this module are copyright 2019 Joe Drago, from libavif, and
//! used under its BSD-2-Clause licence: `licenses/libavif-LICENSE.txt`.

use crate::orientation::Orientation;
use crate::{ColourModel, Image, ImageError, ImageResult, Limits, PixelFormat};

mod container;
#[cfg(feature = "avif")]
mod convert;
#[cfg(feature = "avif")]
mod decode;
#[cfg(feature = "avif")]
mod libyuv;
mod movie;
mod obu;
mod setup;
mod stream;

/// libavif's `avifResult`, as far as reading a file can produce one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    /// `AVIF_RESULT_BMFF_PARSE_FAILED`: a box is malformed or breaks a rule.
    Parse(&'static str),
    /// `AVIF_RESULT_TRUNCATED_DATA`.
    Truncated,
    /// `AVIF_RESULT_INVALID_FTYP`: not an AVIF file after all.
    NotAvif,
    /// `AVIF_RESULT_NO_CONTENT`: nothing to decode where something must be.
    NoContent(&'static str),
    /// `AVIF_RESULT_MISSING_IMAGE_ITEM`.
    MissingImage,
    /// `AVIF_RESULT_INVALID_IMAGE_GRID`.
    Grid(&'static str),
    /// `AVIF_RESULT_INVALID_TONE_MAPPED_IMAGE`: a broken HDR gain map, which
    /// libavif refuses the whole picture for even when it would not use it.
    ToneMap(&'static str),
    /// `AVIF_RESULT_NOT_IMPLEMENTED`, and libavif's refusals of files it
    /// declines to handle.
    Unsupported(&'static str),
    /// `AVIF_RESULT_DECODE_ALPHA_FAILED` before decoding: no ID left for the
    /// alpha grid libavif would make up.
    Alpha(&'static str),
    /// `AVIF_RESULT_DECODE_COLOR_FAILED` and `..._ALPHA_FAILED`: the AV1
    /// decoder refused a frame, or produced one the container does not
    /// describe.
    #[cfg_attr(
        not(feature = "avif"),
        expect(dead_code, reason = "only decoding produces it")
    )]
    Decode(&'static str),
}

impl From<Error> for ImageError {
    fn from(error: Error) -> Self {
        match error {
            Error::Parse(what)
            | Error::NoContent(what)
            | Error::Alpha(what)
            | Error::Grid(what)
            | Error::ToneMap(what)
            | Error::Decode(what) => Self::Malformed(what),
            Error::Truncated => Self::Truncated,
            Error::NotAvif => Self::UnknownFormat,
            Error::MissingImage => Self::Malformed("AVIF primary item"),
            Error::Unsupported(what) => Self::Unsupported(what),
        }
    }
}

/// libavif's default `imageSizeLimit`: the most pixels a picture may have.
pub(crate) const IMAGE_SIZE_LIMIT: u32 = 16384 * 16384;
/// libavif's default `imageDimensionLimit`: the most pixels on a side.
pub(crate) const IMAGE_DIMENSION_LIMIT: u32 = 32768;
/// libavif's default `imageCountLimit`: the most frames a sequence may have.
pub(crate) const IMAGE_COUNT_LIMIT: u32 = 12 * 3600 * 60;

/// `avifDimensionsTooLarge` with libavif's default limits. `height` must not
/// be 0, as at every caller in libavif.
pub(crate) fn too_large(width: u32, height: u32) -> bool {
    match IMAGE_SIZE_LIMIT.checked_div(height) {
        Some(most) if width <= most => {
            width > IMAGE_DIMENSION_LIMIT || height > IMAGE_DIMENSION_LIMIT
        }
        _ => true,
    }
}

/// Whether `bytes` begin as an AVIF file: an `ftyp` box first whose brands
/// include `avif` (a picture) or `avis` (a sequence).
#[must_use]
pub fn is_avif(bytes: &[u8]) -> bool {
    container::sniff(bytes)
}

/// An AVIF's size as it is shown: cropped by a `clap` Chrome would apply, and
/// turned by its `irot`, from the container alone.
///
/// # Errors
///
/// What libavif reports parsing the file -- [`ImageError::Truncated`], or
/// [`ImageError::Malformed`] naming the box at fault -- and
/// [`ImageError::Unsupported`] where libavif declines.
pub fn dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    let picture = setup::Picture::read(bytes)?;
    Ok(picture.orientation().shown(picture.shown_size()))
}

/// How the picture stores its pixels: 8, 10 or 12 bits a channel, grey for a
/// 4:0:0 picture and colour otherwise, and alpha when there is an alpha
/// image.
///
/// # Errors
///
/// As [`dimensions`].
pub fn pixel_format(bytes: &[u8]) -> ImageResult<PixelFormat> {
    let picture = setup::Picture::read(bytes)?;
    let grey = picture.format == setup::YuvFormat::Yuv400;
    let has_alpha = picture.alpha.is_some();
    let channels = match (grey, has_alpha) {
        (true, false) => 1,
        (true, true) => 2,
        (false, false) => 3,
        (false, true) => 4,
    };
    Ok(PixelFormat::uniform(
        picture.depth,
        channels,
        if grey {
            ColourModel::Grey
        } else {
            ColourModel::Colour
        },
        false,
        has_alpha,
    ))
}

/// Decode an AVIF: its picture, or a sequence's first frame -- the frame
/// decoded by rav1d as libavif drives dav1d (`avif/decode.rs`), cropped and
/// converted to pixels as Chrome has libavif convert them
/// (`avif/convert.rs`), and turned as it is shown.
///
/// # Errors
///
/// As [`dimensions`]; [`ImageError::TooLarge`] past `limits`;
/// [`ImageError::Malformed`] when the AV1 data does not decode, or decodes to
/// a frame of another size, depth or chroma layout than the container says;
/// and [`ImageError::Unsupported`] for a colour matrix libavif does not
/// convert, or a frame that would have to be rescaled to its `ispe` size.
#[cfg(feature = "avif")]
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    let picture = setup::Picture::read(bytes)?;
    picture.check(limits)?;
    let frame = decode::decode(&picture, 0)?;
    let frame = match picture.crop() {
        Some(rect) => frame
            .view(rect.x, rect.y, rect.width, rect.height)
            .ok_or(Error::Decode("AVIF clean aperture"))?,
        None => frame,
    };
    let pixels = convert::to_argb(&frame)?;
    let (width, height) = frame.size();
    Ok(picture.orientation().apply(Image {
        width,
        height,
        pixels,
    }))
}

/// Decode an AVIF -- which, built without the `avif` feature, this crate
/// cannot: the container is read and checked, and the picture reported
/// unsupported.
///
/// # Errors
///
/// As [`dimensions`], [`ImageError::TooLarge`] past `limits`, and otherwise
/// [`ImageError::Unsupported`].
#[cfg(not(feature = "avif"))]
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    let picture = setup::Picture::read(bytes)?;
    picture.check(limits)?;
    Err(ImageError::Unsupported("AVIF decoding"))
}

/// Decode an AVIF already scaled to fit `max_w` x `max_h`: [`decode()`], then
/// the crate's box filter.
///
/// # Errors
///
/// As [`decode()`].
pub fn decode_scaled(bytes: &[u8], limits: Limits, max_w: u32, max_h: u32) -> ImageResult<Image> {
    let image = decode(bytes, limits)?;
    crate::scale::shrink_to_fit(image, max_w, max_h)
}

impl setup::Picture<'_> {
    /// The EXIF orientation Chrome shows the picture with: its
    /// `kAxisAngleToOrientation` table, which is `irot` then `imir` as MIAF
    /// orders them.
    pub(crate) const fn orientation(&self) -> Orientation {
        let angle = match self.irot {
            Some(angle) => angle & 3,
            None => 0,
        };
        let value = match (self.imir, angle) {
            (None, 0) => 1,
            (None, 1) => 8,
            (None, 2) => 3,
            (None, _) => 6,
            // Top and bottom exchanged.
            (Some(0), 0) => 4,
            (Some(0), 1) => 5,
            (Some(0), 2) => 2,
            (Some(0), _) => 7,
            // Left and right exchanged.
            (Some(_), 0) => 2,
            (Some(_), 1) => 7,
            (Some(_), 2) => 4,
            (Some(_), _) => 5,
        };
        match Orientation::from_value(value) {
            Some(orientation) => orientation,
            None => Orientation::TopLeft,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail loudly")]

    use super::*;

    #[test]
    fn libavif_s_size_limits_are_its_defaults() {
        assert!(!too_large(16384, 16384));
        assert!(too_large(16385, 16384));
        assert!(!too_large(32768, 8192));
        assert!(too_large(32769, 1));
        assert!(too_large(1, 32769));
        assert!(too_large(1, 0));
    }
}
