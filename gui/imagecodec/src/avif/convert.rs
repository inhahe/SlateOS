//! A decoded picture to pixels: libavif 1.3.0's `avifImageYUVToRGB`, for
//! the one destination this crate has -- 8 bits a channel, B G R A in memory
//! (libavif's `AVIF_RGB_FORMAT_BGRA`), straight alpha -- and libavif's
//! default chroma upsampling, which Chrome and Pillow both leave alone.
//!
//! The conversion itself -- libyuv's fixed point where libyuv has constants
//! for the picture's matrix, libavif's floating point everywhere else, and
//! alpha -- is `gui/video/yuv`'s [`yuv::reformat`], which frames of video
//! go through too, so that the two agree. This hands it the picture as
//! libavif's `avifImage` would describe it.

use alloc::vec::Vec;

use super::Error;
use super::decode::{Decoded, Plane, Yuv};
use super::setup::YuvFormat;
use yuv::reformat::{self, Format, Picture, Reformat};

/// Why a picture cannot be converted: `AVIF_RESULT_REFORMAT_FAILED`.
const UNSUPPORTED: Error = Error::Unsupported("AVIF colour matrix");
/// A size that does not fit memory, or planes that do not match it.
const BAD_SIZE: Error = Error::Decode("AVIF picture size");
/// No pixels to make.
const NO_LUMA: Error = Error::Decode("AVIF picture without luma");

/// Converts `decoded` to `0xAARRGGBB` pixels, `width * height` of them, row by
/// row: `avifImageYUVToRGB`.
///
/// # Errors
///
/// [`Error::Unsupported`] for a matrix libavif does not convert (BT.2020
/// constant luminance, SMPTE 2085, ICtCp, reserved values, YCgCo at limited
/// range, the identity matrix with subsampled chroma).
pub(crate) fn to_argb(decoded: &Decoded) -> Result<Vec<u32>, Error> {
    match decoded {
        Decoded::Eight(image) => convert(image),
        Decoded::Deep(image) => convert(image),
    }
}

/// `image` as [`yuv::reformat`] takes it, and converted.
fn convert<T: Reformat>(image: &Yuv<T>) -> Result<Vec<u32>, Error> {
    let [Some(y), u, v] = &image.planes else {
        return Err(NO_LUMA);
    };
    let width = usize::try_from(image.width).map_err(|_| BAD_SIZE)?;
    let height = usize::try_from(image.height).map_err(|_| BAD_SIZE)?;
    let picture = Picture {
        width,
        height,
        depth: image.depth,
        format: match image.format {
            YuvFormat::Yuv444 => Format::Yuv444,
            YuvFormat::Yuv422 => Format::Yuv422,
            YuvFormat::Yuv420 => Format::Yuv420,
            YuvFormat::Yuv400 => Format::Yuv400,
        },
        matrix: image.matrix,
        primaries: image.primaries,
        full_range: image.full_range,
        y: y.view(),
        u: u.as_ref().map(Plane::view),
        v: v.as_ref().map(Plane::view),
        alpha: image.alpha.as_ref().map(Plane::view),
        alpha_premultiplied: image.alpha_premultiplied,
    };
    reformat::to_argb(&picture).map_err(|e| match e {
        reformat::Error::Unsupported => UNSUPPORTED,
        reformat::Error::Size if width == 0 || height == 0 => NO_LUMA,
        reformat::Error::Size => BAD_SIZE,
    })
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests index and size what they build"
)]
mod tests {
    use super::*;
    use alloc::vec;

    fn grey(format: YuvFormat, matrix: u16) -> Yuv<u8> {
        let plane = |w: usize, h: usize| Plane {
            width: w,
            height: h,
            samples: vec![128u8; w * h],
        };
        let chroma = match format {
            YuvFormat::Yuv444 => Some(plane(3, 3)),
            YuvFormat::Yuv422 => Some(plane(2, 3)),
            YuvFormat::Yuv420 => Some(plane(2, 2)),
            YuvFormat::Yuv400 => None,
        };
        Yuv {
            width: 3,
            height: 3,
            depth: 8,
            format,
            full_range: true,
            primaries: 1,
            transfer: 13,
            matrix,
            planes: [Some(plane(3, 3)), chroma.clone(), chroma],
            alpha: None,
            alpha_premultiplied: false,
        }
    }

    /// The picture reaches the conversion whole: mid grey at full range is
    /// mid grey on every path, as `yuv::reformat`'s own tests have it.
    #[test]
    fn a_picture_converts_through_yuv_reformat() {
        for (format, matrix) in [
            (YuvFormat::Yuv420, 1),
            (YuvFormat::Yuv422, 1),
            (YuvFormat::Yuv444, 4),
            (YuvFormat::Yuv400, 2),
        ] {
            let out = to_argb(&Decoded::Eight(grey(format, matrix))).unwrap_or_default();
            assert_eq!(out.len(), 9, "{format:?}");
            for px in out {
                assert_eq!(px >> 24, 255);
                assert!((px & 0xff).abs_diff(128) <= 1, "{format:?} {px:08x}");
            }
        }
    }

    /// libavif's refusals, and a picture with no luma, keep their words.
    #[test]
    fn refusals_say_what_they_said() {
        assert_eq!(
            to_argb(&Decoded::Eight(grey(YuvFormat::Yuv420, 10))),
            Err(UNSUPPORTED)
        );
        let mut empty = grey(YuvFormat::Yuv420, 1);
        empty.planes[0] = None;
        assert_eq!(to_argb(&Decoded::Eight(empty)), Err(NO_LUMA));
        let mut none = grey(YuvFormat::Yuv420, 1);
        none.width = 0;
        assert_eq!(to_argb(&Decoded::Eight(none)), Err(NO_LUMA));
        // A picture larger than its planes is refused, not read short.
        let mut short = grey(YuvFormat::Yuv420, 1);
        short.height = 4;
        assert_eq!(to_argb(&Decoded::Eight(short)), Err(BAD_SIZE));
    }
}
