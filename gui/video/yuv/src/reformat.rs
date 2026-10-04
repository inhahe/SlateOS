//! A picture in YUV to pixels as libavif converts it: libavif 1.3.0's
//! `avifImageYUVToRGB` (`src/reformat.c`, `src/reformat_libyuv.c`,
//! `src/alpha.c`), for the one destination SlateOS draws -- 8 bits a
//! channel, B G R A in memory (libavif's `AVIF_RGB_FORMAT_BGRA`), straight
//! alpha -- and libavif's default chroma upsampling
//! (`AVIF_CHROMA_UPSAMPLING_AUTOMATIC`), which Chrome and Pillow both leave
//! alone. AVIF pictures (`gui/imagecodec`) and frames of video
//! (`gui/video/codec`) both come through here, so that a frame has the
//! colours the same picture would have as an AVIF still.
//!
//! libavif tries libyuv first: when libyuv has constants for the picture's
//! matrix and range (BT.601, BT.709 and BT.2020, and the chromaticity-derived
//! matrix over those primaries), the conversion is libyuv's fixed-point
//! arithmetic ([`crate::convert`]), bilinear for 4:2:0, linear for 4:2:2 --
//! except 12-bit 4:2:0, which libyuv only converts nearest-neighbour, and
//! 12-bit 4:2:2 and 4:4:4 and deep grey, which libavif first cuts to 8 bits.
//! Every other matrix goes to libavif's own floating-point code: a fast path
//! per pixel for 4:4:4 and grey, and for subsampled chroma a slow one that
//! upsamples 9:3:3:1 as it goes. All of it is ported, float for float, since
//! the order of the arithmetic decides the last bit.
//!
//! Alpha is libyuv's where a libyuv function carried it, and otherwise
//! libavif's: copied at 8 bits, rescaled in floating point from 10 or 12. A
//! picture whose colour was premultiplied has it undone afterwards, as a
//! straight-alpha destination asks.
//!
//! Portions of this file are copyright 2019-2020 Joe Drago, from libavif, and
//! used under its BSD-2-Clause licence, whose text travels as
//! `gui/imagecodec/licenses/libavif-LICENSE.txt` (that crate's manifest names
//! libavif, once for the tree, as `scripts/gather-notices.py` requires).

use alloc::vec::Vec;

use crate::convert::{self as libyuv, Constants, Eight, Planes, Ten};
use crate::{Plane, PlaneBuf, Sample};

// H.273's matrix coefficients, as libavif names them.
const MC_IDENTITY: u16 = 0;
const MC_BT709: u16 = 1;
const MC_UNSPECIFIED: u16 = 2;
const MC_RESERVED: u16 = 3;
const MC_FCC: u16 = 4;
const MC_BT470BG: u16 = 5;
const MC_BT601: u16 = 6;
const MC_SMPTE240: u16 = 7;
const MC_YCGCO: u16 = 8;
const MC_BT2020_NCL: u16 = 9;
const MC_BT2020_CL: u16 = 10;
const MC_SMPTE2085: u16 = 11;
const MC_CHROMA_DERIVED_NCL: u16 = 12;
const MC_CHROMA_DERIVED_CL: u16 = 13;
const MC_ICTCP: u16 = 14;
const MC_YCGCO_RE: u16 = 16;
const MC_YCGCO_RO: u16 = 17;
/// `AVIF_MATRIX_COEFFICIENTS_LAST`: 15, reserved, is below it and so is not
/// refused; it converts as BT.601.
const MC_LAST: u16 = 18;

// And its colour primaries.
const CP_BT709: u16 = 1;
const CP_UNSPECIFIED: u16 = 2;
const CP_BT470BG: u16 = 5;
const CP_BT601: u16 = 6;
const CP_BT2020: u16 = 9;

/// How a picture's chroma is sampled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Chroma at the picture's size.
    Yuv444,
    /// Chroma at half the width.
    Yuv422,
    /// Chroma at half the width and half the height.
    Yuv420,
    /// Monochrome: luma only.
    Yuv400,
}

impl Format {
    /// libavif's chroma shift: how far right of the luma size each chroma
    /// size is -- 1 each way for 4:0:0, which libavif counts as 4:2:0.
    const fn shift(self) -> (u32, u32) {
        match self {
            Self::Yuv444 => (0, 0),
            Self::Yuv422 => (1, 0),
            Self::Yuv420 | Self::Yuv400 => (1, 1),
        }
    }
}

/// A picture to convert, as libavif's `avifImage` describes one: its
/// planes, read where they lie (with their strides, as a decoder hands them
/// out), and how to read them.
#[derive(Debug)]
pub struct Picture<'a, T> {
    /// The picture's size: luma's, and the pixels made.
    pub width: usize,
    pub height: usize,
    /// Bits a sample: 8, in `u8`s; or 10, 12 or 16, in `u16`s.
    pub depth: u8,
    pub format: Format,
    /// ITU-T H.273's `MatrixCoefficients`: how Y, U and V encode R, G and B.
    pub matrix: u16,
    /// ITU-T H.273's `ColourPrimaries`, from which the chromaticity-derived
    /// matrices (12 and 13) take their weights.
    pub primaries: u16,
    /// Samples span every code (0 to 255 at 8 bits), rather than the studio
    /// range (luma 16 to 235, chroma 16 to 240).
    pub full_range: bool,
    pub y: Plane<'a, T>,
    /// U and V: absent for monochrome.
    pub u: Option<Plane<'a, T>>,
    pub v: Option<Plane<'a, T>>,
    /// Alpha, at the colour's depth and the picture's size.
    pub alpha: Option<Plane<'a, T>>,
    /// The colour was multiplied by the alpha, and is divided by it here.
    pub alpha_premultiplied: bool,
}

// Views copy whatever their samples are, so not derived (which would ask
// `T: Copy`).
impl<T> Clone for Picture<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Picture<'_, T> {}

/// Why a picture is not converted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A colour description libavif does not convert
    /// (`AVIF_RESULT_REFORMAT_FAILED`): BT.2020 constant luminance, SMPTE
    /// 2085, ICtCp, reserved values, YCgCo at limited range or YCgCo-R at a
    /// depth it does not fit, the identity matrix with subsampled chroma, or
    /// a depth other than 8, 10, 12 and 16.
    Unsupported,
    /// No pixels, more than memory holds, or a plane smaller than the
    /// picture's size says it is.
    Size,
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for u16 {}
}

/// The sample sizes libavif converts, each with libyuv's functions for it.
pub trait Reformat: Sample + sealed::Sealed {
    /// `avifImageYUVToRGBLibYUV` for this sample size: whether libyuv
    /// converted the picture, and whether the alpha it wrote is final.
    #[doc(hidden)]
    fn libyuv(picture: &Picture<'_, Self>, k: &Constants, out: &mut [u32]) -> (bool, bool);
}

impl Reformat for u8 {
    fn libyuv(picture: &Picture<'_, Self>, k: &Constants, out: &mut [u32]) -> (bool, bool) {
        libyuv_eight(picture, k, out)
    }
}

impl Reformat for u16 {
    fn libyuv(picture: &Picture<'_, Self>, k: &Constants, out: &mut [u32]) -> (bool, bool) {
        libyuv_deep(picture, k, out)
    }
}

/// libavif's `avifReformatMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Luma and colour difference, weighted by `kr`, `kg` and `kb`.
    Coefficients,
    /// G, B and R carried as Y, U and V.
    Identity,
    YCgCo,
    YCgCoRe,
    YCgCoRo,
}

/// The YUV half of `avifReformatState`: `avifYUVColorSpaceInfo`.
#[derive(Clone, Copy, Debug)]
struct State {
    mode: Mode,
    kr: f32,
    kg: f32,
    kb: f32,
    depth: u8,
    max_channel: u32,
    bias_y: f32,
    bias_uv: f32,
    range_y: f32,
    range_uv: f32,
    /// libavif's chroma shift, which for 4:0:0 is 1 each way.
    shift_x: u32,
    shift_y: u32,
}

/// `avifPrepareReformatState` with `avifGetYUVColorSpaceInfo`: the checks
/// that decide whether a picture converts at all, and the constants every
/// path reads.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    reason = "depth is 8, 10, 12 or 16, so every shift is by under 16 and every value converted to float is under 2^16, exactly representable"
)]
fn prepare<T>(picture: &Picture<'_, T>) -> Result<State, Error> {
    let matrix = picture.matrix;
    // YCgCo-Re and -Ro carry two and one more bits than the RGB they encode,
    // which is 8 here.
    if (matrix == MC_YCGCO_RE && picture.depth != 10)
        || (matrix == MC_YCGCO_RO && picture.depth != 9)
    {
        return Err(Error::Unsupported);
    }
    if !matches!(picture.depth, 8 | 10 | 12 | 16) {
        return Err(Error::Unsupported);
    }
    let ycgco = matches!(matrix, MC_YCGCO | MC_YCGCO_RE | MC_YCGCO_RO);
    if matrix == MC_RESERVED
        || (ycgco && !picture.full_range)
        || matches!(
            matrix,
            MC_BT2020_CL | MC_SMPTE2085 | MC_CHROMA_DERIVED_CL | MC_ICTCP
        )
        || matrix >= MC_LAST
    {
        return Err(Error::Unsupported);
    }
    if matrix == MC_IDENTITY && !matches!(picture.format, Format::Yuv444 | Format::Yuv400) {
        return Err(Error::Unsupported);
    }
    let mode = match matrix {
        MC_IDENTITY => Mode::Identity,
        MC_YCGCO => Mode::YCgCo,
        MC_YCGCO_RE => Mode::YCgCoRe,
        MC_YCGCO_RO => Mode::YCgCoRo,
        _ => Mode::Coefficients,
    };
    let [kr, kg, kb] = if mode == Mode::Coefficients {
        coefficients(matrix, picture.primaries)
    } else {
        [0.0; 3]
    };
    let depth = u32::from(picture.depth);
    let max_channel = (1u32 << depth) - 1;
    let limited = !picture.full_range;
    let (shift_x, shift_y) = picture.format.shift();
    Ok(State {
        mode,
        kr,
        kg,
        kb,
        depth: picture.depth,
        max_channel,
        bias_y: if limited {
            (16u32 << (depth - 8)) as f32
        } else {
            0.0
        },
        bias_uv: (1u32 << (depth - 1)) as f32,
        range_y: if limited {
            (219u32 << (depth - 8)) as f32
        } else {
            max_channel as f32
        },
        range_uv: if limited {
            (224u32 << (depth - 8)) as f32
        } else {
            max_channel as f32
        },
        shift_x,
        shift_y,
    })
}

/// `avifCalcYUVCoefficients`: `kr`, `kg` and `kb` from the matrix's table
/// entry or, for the chromaticity-derived matrix, from the primaries; BT.601's
/// where neither says.
fn coefficients(matrix: u16, primaries: u16) -> [f32; 3] {
    let from = |kr: f32, kb: f32| [kr, 1.0 - kr - kb, kb];
    match matrix {
        MC_CHROMA_DERIVED_NCL => primaries_coefficients(primaries),
        MC_BT709 => from(0.2126, 0.0722),
        MC_FCC => from(0.30, 0.11),
        MC_BT470BG | MC_BT601 => from(0.299, 0.114),
        MC_SMPTE240 => from(0.212, 0.087),
        MC_BT2020_NCL => from(0.2627, 0.0593),
        _ => from(0.299, 0.114),
    }
}

/// `avifColorPrimariesGetValues`: the primaries' and white point's
/// chromaticities, BT.709's for any libavif does not know.
const fn primaries_values(primaries: u16) -> [f32; 8] {
    match primaries {
        4 => [0.67, 0.33, 0.21, 0.71, 0.14, 0.08, 0.310, 0.316],
        5 => [0.64, 0.33, 0.29, 0.60, 0.15, 0.06, 0.3127, 0.3290],
        6 | 7 => [0.630, 0.340, 0.310, 0.595, 0.155, 0.070, 0.3127, 0.3290],
        8 => [0.681, 0.319, 0.243, 0.692, 0.145, 0.049, 0.310, 0.316],
        9 => [0.708, 0.292, 0.170, 0.797, 0.131, 0.046, 0.3127, 0.3290],
        10 => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.3333, 0.3333],
        11 => [0.680, 0.320, 0.265, 0.690, 0.150, 0.060, 0.314, 0.351],
        12 => [0.680, 0.320, 0.265, 0.690, 0.150, 0.060, 0.3127, 0.3290],
        22 => [0.630, 0.340, 0.295, 0.605, 0.155, 0.077, 0.3127, 0.3290],
        _ => [0.64, 0.33, 0.3, 0.6, 0.15, 0.06, 0.3127, 0.329],
    }
}

/// `avifColorPrimariesComputeYCoeffs`: H.273's equations 32 to 37, in the
/// C's order of operations.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "floating-point arithmetic, which cannot overflow into undefined behaviour"
)]
fn primaries_coefficients(primaries: u16) -> [f32; 3] {
    let [r_x, r_y, g_x, g_y, b_x, b_y, w_x, w_y] = primaries_values(primaries);
    let r_z = 1.0 - (r_x + r_y);
    let g_z = 1.0 - (g_x + g_y);
    let b_z = 1.0 - (b_x + b_y);
    let w_z = 1.0 - (w_x + w_y);
    let denominator = w_y
        * (r_x * (g_y * b_z - b_y * g_z)
            + g_x * (b_y * r_z - r_y * b_z)
            + b_x * (r_y * g_z - g_y * r_z));
    let kr = (r_y
        * (w_x * (g_y * b_z - b_y * g_z)
            + w_y * (b_x * g_z - g_x * b_z)
            + w_z * (g_x * b_y - b_x * g_y)))
        / denominator;
    let kb = (b_y
        * (w_x * (r_y * g_z - g_y * r_z)
            + w_y * (g_x * r_z - r_x * g_z)
            + w_z * (r_x * g_y - g_x * r_y)))
        / denominator;
    [kr, 1.0 - kr - kb, kb]
}

/// `getLibYUVConstants`: libyuv's table for the picture, if it has one.
/// Grey with the identity matrix converts as BT.601.
fn libyuv_constants<T>(picture: &Picture<'_, T>) -> Option<Constants> {
    let matrix = if picture.format == Format::Yuv400 && picture.matrix == MC_IDENTITY {
        MC_BT601
    } else {
        picture.matrix
    };
    let (bt709, bt601, bt2020) = if picture.full_range {
        (libyuv::F709, libyuv::JPEG, libyuv::V2020)
    } else {
        (libyuv::H709, libyuv::I601, libyuv::BT2020)
    };
    match matrix {
        MC_BT709 => Some(bt709),
        MC_BT470BG | MC_BT601 | MC_UNSPECIFIED => Some(bt601),
        MC_BT2020_NCL => Some(bt2020),
        MC_CHROMA_DERIVED_NCL => match picture.primaries {
            CP_BT709 | CP_UNSPECIFIED => Some(bt709),
            CP_BT470BG | CP_BT601 => Some(bt601),
            CP_BT2020 => Some(bt2020),
            _ => None,
        },
        _ => None,
    }
}

/// libyuv did nothing: libavif's own code converts the picture.
const NOT_IMPLEMENTED: (bool, bool) = (false, false);

/// The planes of a picture as a libyuv function takes them, or `None` if a
/// chroma plane is missing.
fn planes<'a, T>(picture: &Picture<'a, T>, with_alpha: bool) -> Option<Planes<'a, T>> {
    Some(Planes {
        y: picture.y,
        u: picture.u?,
        v: picture.v?,
        a: if with_alpha { picture.alpha } else { None },
    })
}

/// `avifImageYUVToRGBLibYUV` for an 8-bit picture: whether it converted, and
/// whether the alpha is final.
fn libyuv_eight(picture: &Picture<'_, u8>, k: &Constants, out: &mut [u32]) -> (bool, bool) {
    let width = picture.width;
    let has_alpha = picture.alpha.is_some();
    if picture.format == Format::Yuv400 {
        libyuv::i400(k, picture.y, width, out);
        // Without alpha in the picture, libyuv's opaque alpha is the answer.
        return (true, !has_alpha);
    }
    let Some(planes) = planes(picture, has_alpha) else {
        return NOT_IMPLEMENTED;
    };
    match picture.format {
        Format::Yuv420 => libyuv::i420_bilinear::<Eight>(k, &planes, width, out),
        Format::Yuv422 => libyuv::i422_linear::<Eight>(k, &planes, width, out),
        Format::Yuv444 | Format::Yuv400 => libyuv::i444::<Eight>(k, &planes, width, out),
    }
    (true, true)
}

/// `avifImageYUVToRGBLibYUV` for a 10- or 12-bit picture: libyuv's deep
/// functions where it has them (all of 10-bit colour; 12-bit 4:2:0 without
/// alpha), and otherwise `avifImageDownshiftTo8bpc` and the 8-bit ones.
fn libyuv_deep(picture: &Picture<'_, u16>, k: &Constants, out: &mut [u32]) -> (bool, bool) {
    let width = picture.width;
    let has_alpha = picture.alpha.is_some();
    match (picture.depth, picture.format) {
        (10, Format::Yuv420 | Format::Yuv422 | Format::Yuv444) => {
            let Some(planes) = planes(picture, has_alpha) else {
                return NOT_IMPLEMENTED;
            };
            match picture.format {
                Format::Yuv420 => libyuv::i420_bilinear::<Ten>(k, &planes, width, out),
                Format::Yuv422 => libyuv::i422_linear::<Ten>(k, &planes, width, out),
                Format::Yuv444 | Format::Yuv400 => libyuv::i444::<Ten>(k, &planes, width, out),
            }
            (true, true)
        }
        (12, Format::Yuv420) => {
            let Some(planes) = planes(picture, false) else {
                return NOT_IMPLEMENTED;
            };
            libyuv::i012(k, &planes, width, out);
            (true, !has_alpha)
        }
        (10 | 12, _) => {
            // Grey takes the 8-bit grey function, which has no alpha, so
            // alpha stays deep; 4:2:2 and 4:4:4 take the 8-bit alpha
            // functions, so it is cut to 8 bits with the rest.
            let with_alpha = has_alpha && picture.format != Format::Yuv400;
            let cut = Downshifted::of(picture, with_alpha);
            let (converted, _) = libyuv_eight(&cut.picture(picture), k, out);
            // Alpha is final if there was none, or if an 8-bit alpha function
            // carried the downshifted copy; grey's leaves it to libavif.
            (converted, !has_alpha || with_alpha)
        }
        _ => NOT_IMPLEMENTED,
    }
}

/// `avifImageDownshiftTo8bpc`'s planes: every one (alpha only if asked)
/// through `Convert16To8Plane`.
struct Downshifted {
    y: PlaneBuf<u8>,
    u: Option<PlaneBuf<u8>>,
    v: Option<PlaneBuf<u8>>,
    alpha: Option<PlaneBuf<u8>>,
}

impl Downshifted {
    fn of(picture: &Picture<'_, u16>, with_alpha: bool) -> Self {
        let cut = |plane: Plane<'_, u16>| libyuv::convert_16_to_8(plane, picture.depth);
        Self {
            y: cut(picture.y),
            u: picture.u.map(cut),
            v: picture.v.map(cut),
            alpha: if with_alpha {
                picture.alpha.map(cut)
            } else {
                None
            },
        }
    }

    /// The 8-bit picture: `like`'s description at 8 bits.
    fn picture(&self, like: &Picture<'_, u16>) -> Picture<'_, u8> {
        Picture {
            width: like.width,
            height: like.height,
            depth: 8,
            format: like.format,
            matrix: like.matrix,
            primaries: like.primaries,
            full_range: like.full_range,
            y: self.y.view(),
            u: self.u.as_ref().map(PlaneBuf::view),
            v: self.v.as_ref().map(PlaneBuf::view),
            alpha: self.alpha.as_ref().map(PlaneBuf::view),
            alpha_premultiplied: like.alpha_premultiplied,
        }
    }
}

/// Whether `plane` has the `width` x `height` samples a conversion reads: as
/// wide and as tall, and every row of them inside its samples -- the last
/// row's start plus the plane's width, which [`Plane::row`] takes whole.
fn covers<T>(plane: &Plane<'_, T>, width: usize, height: usize) -> bool {
    if plane.width < width || plane.height < height {
        return false;
    }
    let Some(last) = height.checked_sub(1) else {
        return true;
    };
    last.checked_mul(plane.stride)
        .and_then(|start| start.checked_add(plane.width))
        .is_some_and(|end| end <= plane.samples.len())
}

/// The size of a chroma plane of a picture of luma size `size`:
/// `(size + shift) >> shift`, as `avifImagePlaneWidth` computes it.
const fn chroma_size(size: usize, shift: u32) -> usize {
    if shift == 0 { size } else { size.div_ceil(2) }
}

/// `picture` converted to `0xAARRGGBB` pixels, `width * height` of them, row
/// by row: `avifImageYUVToRGB`.
///
/// # Errors
///
/// [`Error::Unsupported`] for a colour description libavif does not
/// convert; [`Error::Size`] for no pixels, too many, or a plane smaller
/// than the picture.
pub fn to_argb<T: Reformat>(picture: &Picture<'_, T>) -> Result<Vec<u32>, Error> {
    let mut out = Vec::new();
    to_argb_into(picture, &mut out)?;
    Ok(out)
}

/// [`to_argb`] into `out`, which is cleared first: a player converting frame
/// after frame of one size keeps the one allocation.
///
/// # Errors
///
/// As [`to_argb`]; `out` is then empty.
pub fn to_argb_into<T: Reformat>(
    picture: &Picture<'_, T>,
    out: &mut Vec<u32>,
) -> Result<(), Error> {
    out.clear();
    let state = prepare(picture)?;
    let (width, height) = (picture.width, picture.height);
    let count = width.checked_mul(height).ok_or(Error::Size)?;
    if count == 0 || !covers(&picture.y, width, height) {
        return Err(Error::Size);
    }
    let chroma = (
        chroma_size(width, state.shift_x),
        chroma_size(height, state.shift_y),
    );
    let chroma_ok = |plane: &Option<Plane<'_, T>>| {
        plane
            .as_ref()
            .is_none_or(|p| picture.format == Format::Yuv400 || covers(p, chroma.0, chroma.1))
    };
    let alpha_ok = picture
        .alpha
        .as_ref()
        .is_none_or(|a| covers(a, width, height));
    if !chroma_ok(&picture.u) || !chroma_ok(&picture.v) || !alpha_ok {
        return Err(Error::Size);
    }
    out.try_reserve_exact(count).map_err(|_| Error::Size)?;
    out.resize(count, 0);

    // A straight-alpha destination from premultiplied colour.
    let mut unmultiply = picture.alpha.is_some() && picture.alpha_premultiplied;

    let (converted, alpha_done) = match libyuv_constants(picture) {
        Some(k) => T::libyuv(picture, &k, out),
        None => NOT_IMPLEMENTED,
    };
    if !alpha_done {
        match &picture.alpha {
            Some(alpha) => reformat_alpha(alpha, picture.depth, width, out),
            None => fill_alpha(out),
        }
    }
    if !converted {
        let has_color =
            picture.format != Format::Yuv400 && picture.u.is_some() && picture.v.is_some();
        // None of libavif's fast paths upsample, so only unsubsampled
        // pictures take them.
        let fast = !has_color || picture.format == Format::Yuv444;
        let done = fast
            && match state.mode {
                Mode::Identity => {
                    state.depth == 8
                        && picture.format == Format::Yuv444
                        && picture.full_range
                        && identity_full_range(picture, width, out)
                }
                Mode::Coefficients => {
                    fast_path(picture, &state, has_color, width, out);
                    true
                }
                Mode::YCgCo | Mode::YCgCoRe | Mode::YCgCoRo => false,
            };
        if !done {
            slow_path(picture, &state, unmultiply, width, out);
            // The slow path undoes premultiplication itself.
            unmultiply = false;
        }
    }
    if unmultiply {
        // `avifRGBImageUnpremultiplyAlpha`, which for 8-bit BGRA is libyuv's.
        libyuv::unattenuate(out);
    }
    Ok(())
}

/// `avifReformatAlpha` into the top byte: a copy at 8 bits, and from deeper
/// alpha `(int)(0.5f + a / max * 255)`, in single precision as the C
/// computes it.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    reason = "samples are at most 16 bits, exactly representable as f32; the rounded value is clamped to 0..=255 before it is used"
)]
fn reformat_alpha<T: Sample>(alpha: &Plane<'_, T>, depth: u8, width: usize, out: &mut [u32]) {
    let max = ((1u32 << u32::from(depth).min(16)) - 1) as f32;
    for (j, row) in out.chunks_exact_mut(width.max(1)).enumerate() {
        for (px, &a) in row.iter_mut().zip(alpha.row(j)) {
            let a: u32 = a.into();
            let value = if depth == 8 {
                a.min(255)
            } else {
                let alpha_f = a as f32 / max;
                ((0.5f32 + alpha_f * 255.0) as i32).clamp(0, 255) as u32
            };
            *px = (*px & 0x00ff_ffff) | (value << 24);
        }
    }
}

/// `avifFillAlpha`: opaque.
fn fill_alpha(out: &mut [u32]) {
    for px in out {
        *px |= 0xff00_0000;
    }
}

/// `avifImageIdentity8ToRGB8ColorFullRange`: G, B and R are Y, U and V.
fn identity_full_range<T: Sample>(picture: &Picture<'_, T>, width: usize, out: &mut [u32]) -> bool {
    let (Some(u), Some(v)) = (picture.u, picture.v) else {
        return false;
    };
    for (j, row) in out.chunks_exact_mut(width.max(1)).enumerate() {
        let samples = picture.y.row(j).iter().zip(u.row(j).iter().zip(v.row(j)));
        for (px, (&y, (&u, &v))) in row.iter_mut().zip(samples) {
            let (g, b, r): (u32, u32, u32) = (y.into(), u.into(), v.into());
            *px = (*px & 0xff00_0000) | ((r & 0xff) << 16) | ((g & 0xff) << 8) | (b & 0xff);
        }
    }
    true
}

/// `avifCreateYUVToRGBLookUpTables`: each code point's value as a fraction
/// of its range, luma's and (unless the matrix is the identity, which shares
/// luma's) chroma's.
#[allow(
    clippy::cast_precision_loss,
    reason = "code points are under 2^16, exactly representable as f32"
)]
fn tables(state: &State) -> (Vec<f32>, Vec<f32>) {
    let count = 1u32 << u32::from(state.depth).min(16);
    let luma: Vec<f32> = (0..count)
        .map(|cp| (cp as f32 - state.bias_y) / state.range_y)
        .collect();
    let chroma = if state.mode == Mode::Identity {
        luma.clone()
    } else {
        (0..count)
            .map(|cp| (cp as f32 - state.bias_uv) / state.range_uv)
            .collect()
    };
    (luma, chroma)
}

/// A table's value for a sample, clamped to the depth's range first as the
/// C clamps it "to protect against bad LUT lookups".
fn lookup<T: Sample>(table: &[f32], sample: T, max: u32) -> f32 {
    let index: u32 = sample.into();
    usize::try_from(index.min(max))
        .ok()
        .and_then(|i| table.get(i))
        .copied()
        .unwrap_or(0.0)
}

/// `AVIF_CLAMP(v, 0.0f, 1.0f)`. `f32::clamp` agrees with the C's
/// comparisons everywhere, a NaN passing through and -0.0 staying -0.0.
fn clamp_unit(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// `(uint8_t)(0.5f + (c * 255.0f))`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    reason = "c is within 0..=1 (or NaN, which the C's conversion also makes 0 on x86), so the value is within 0.5..=255.5 and truncates to a byte"
)]
fn to_byte(c: f32) -> u32 {
    u32::from((0.5f32 + c * 255.0) as u8)
}

/// The colour-difference equations shared by the fast and slow paths:
/// `R = Y + 2(1 - kr) Cr`, `B = Y + 2(1 - kb) Cb` and G from the two.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "floating-point arithmetic, which cannot overflow into undefined behaviour"
)]
fn coefficients_rgb(state: &State, y: f32, cb: f32, cr: f32) -> [f32; 3] {
    let (kr, kg, kb) = (state.kr, state.kg, state.kb);
    let r = y + (2.0 * (1.0 - kr)) * cr;
    let b = y + (2.0 * (1.0 - kb)) * cb;
    let g = y - ((2.0 * ((kr * (1.0 - kr) * cr) + (kb * (1.0 - kb) * cb))) / kg);
    [r, g, b]
}

/// `avifImageYUV8ToRGB8Color`, `avifImageYUV16ToRGB8Color` and their `Mono`
/// twins: libavif's per-pixel path for 4:4:4 and grey.
fn fast_path<T: Sample>(
    picture: &Picture<'_, T>,
    state: &State,
    has_color: bool,
    width: usize,
    out: &mut [u32],
) {
    let (luma, chroma) = tables(state);
    let max = state.max_channel;
    for (j, row) in out.chunks_exact_mut(width.max(1)).enumerate() {
        let y_row = picture.y.row(j);
        let (u_row, v_row) = match (picture.u, picture.v) {
            (Some(u), Some(v)) if has_color => (u.row(j), v.row(j)),
            _ => (&[][..], &[][..]),
        };
        for (i, (px, &y)) in row.iter_mut().zip(y_row).enumerate() {
            let y = lookup(&luma, y, max);
            let (cb, cr) = if has_color {
                let at = |r: &[T]| r.get(i).map_or(0.0, |&s| lookup(&chroma, s, max));
                (at(u_row), at(v_row))
            } else {
                (0.0, 0.0)
            };
            let [r, g, b] = coefficients_rgb(state, y, cb, cr);
            *px = (*px & 0xff00_0000)
                | (to_byte(clamp_unit(r)) << 16)
                | (to_byte(clamp_unit(g)) << 8)
                | to_byte(clamp_unit(b));
        }
    }
}

/// `avifImageYUVAnyToRGBAnySlow`: every combination, one pixel at a time --
/// chroma upsampled bilinearly from the four nearest samples, and alpha
/// premultiplication undone in floating point.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "indices stay within the planes by the adjacency rules (checked by get), samples are under 2^16 and exact as f32, and the rounded YCgCo values are small"
)]
fn slow_path<T: Sample>(
    picture: &Picture<'_, T>,
    state: &State,
    unmultiply: bool,
    width: usize,
    out: &mut [u32],
) {
    let (luma, chroma) = tables(state);
    let max = state.max_channel;
    let max_f = max as f32;
    let (u_plane, v_plane) = (picture.u, picture.v);
    let has_color = u_plane.is_some() && v_plane.is_some() && picture.format != Format::Yuv400;
    let height = out.len() / width.max(1);
    let sample = |plane: Option<Plane<'_, T>>, row: usize, col: usize| -> T {
        plane
            .and_then(|p| p.row(row).get(col).copied())
            .unwrap_or_default()
    };
    let (shift_x, shift_y) = (state.shift_x, state.shift_y);
    for (j, row) in out.chunks_exact_mut(width.max(1)).enumerate() {
        let uv_j = if has_color { j >> shift_y } else { 0 };
        let y_row = picture.y.row(j);
        let a_row = picture.alpha.map(|a| a.row(j));
        for (i, (px, &y_sample)) in row.iter_mut().zip(y_row).enumerate() {
            let unorm_y: u32 = y_sample.into();
            let unorm_y = unorm_y.min(max);
            let y = lookup(&luma, y_sample, max);
            let (mut cb, mut cr) = (0.5f32, 0.5f32);
            if has_color {
                let uv_i = i >> shift_x;
                if picture.format == Format::Yuv444 {
                    cb = lookup(&chroma, sample(u_plane, uv_j, uv_i), max);
                    cr = lookup(&chroma, sample(v_plane, uv_j, uv_i), max);
                } else {
                    // The nearest chroma sample, its neighbour along the row
                    // and down the column towards this pixel, and the one
                    // diagonal; at the edges, the nearest again.
                    let adj_col = if i == 0 || (i == width - 1 && i % 2 != 0) {
                        uv_i
                    } else if i % 2 != 0 {
                        uv_i + 1
                    } else {
                        uv_i - 1
                    };
                    let adj_row = if j == 0
                        || (j == height - 1 && j % 2 != 0)
                        || picture.format == Format::Yuv422
                    {
                        uv_j
                    } else if j % 2 != 0 {
                        uv_j + 1
                    } else {
                        uv_j - 1
                    };
                    let weigh = |plane: Option<Plane<'_, T>>| {
                        let at = |r: usize, c: usize| lookup(&chroma, sample(plane, r, c), max);
                        (at(uv_j, uv_i) * (9.0 / 16.0))
                            + (at(uv_j, adj_col) * (3.0 / 16.0))
                            + (at(adj_row, uv_i) * (3.0 / 16.0))
                            + (at(adj_row, adj_col) * (1.0 / 16.0))
                    };
                    cb = weigh(u_plane);
                    cr = weigh(v_plane);
                }
            }
            let [r, g, b] = if has_color {
                match state.mode {
                    Mode::Identity => [cr, y, cb],
                    Mode::YCgCo => {
                        let t = y - cb;
                        [t + cr, y + cb, t - cr]
                    }
                    Mode::YCgCoRe | Mode::YCgCoRo => {
                        let yy = i32::try_from(unorm_y).unwrap_or(0);
                        let cg = floor(cb * max_f + 0.5);
                        let co = floor(cr * max_f + 0.5);
                        let t = yy - (cg >> 1);
                        let g = (t + cg).clamp(0, 255) as f32;
                        let b = (t - (co >> 1)).clamp(0, 255) as f32;
                        let r = clamp_float(b + co as f32, 255.0);
                        [r / 255.0, g / 255.0, b / 255.0]
                    }
                    Mode::Coefficients => coefficients_rgb(state, y, cb, cr),
                }
            } else {
                [y, y, y]
            };
            let (mut r, mut g, mut b) = (clamp_unit(r), clamp_unit(g), clamp_unit(b));
            if unmultiply {
                let a: u32 = a_row.and_then(|a| a.get(i).copied()).map_or(0, Into::into);
                let alpha = clamp_unit(a.min(max) as f32 / max_f);
                if alpha == 0.0 {
                    (r, g, b) = (0.0, 0.0, 0.0);
                } else if alpha < 1.0 {
                    let undo = |c: f32| {
                        let c = c / alpha;
                        if c < 1.0 { c } else { 1.0 }
                    };
                    (r, g, b) = (undo(r), undo(g), undo(b));
                }
            }
            *px = (*px & 0xff00_0000) | (to_byte(r) << 16) | (to_byte(g) << 8) | to_byte(b);
        }
    }
}

/// `(int)floorf(v)` for the small values the YCgCo-R equations round
/// (`avifRoundf` is `floorf(v + 0.5f)`); `core` has no `floor`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::arithmetic_side_effects,
    reason = "v is a chroma fraction times a 16-bit maximum, far inside i32, and the truncated value converts back to f32 exactly"
)]
fn floor(v: f32) -> i32 {
    let t = v as i32;
    if (t as f32) > v { t - 1 } else { t }
}

/// `AVIF_CLAMP(v, 0, max)` of a float against integer bounds, as the YCgCo-R
/// equations use it.
fn clamp_float(v: f32, max: f32) -> f32 {
    if v < 0.0 {
        0.0
    } else if max < v {
        max
    } else {
        v
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::unwrap_used,
    reason = "tests index what they build, compare floats they computed the same way, and fail loudly"
)]
mod tests {
    use super::*;
    use alloc::vec;

    /// Planes for a `width` x `height` picture of `format`, every sample
    /// 128.
    struct Owned {
        y: PlaneBuf<u8>,
        u: PlaneBuf<u8>,
        v: PlaneBuf<u8>,
        format: Format,
        width: usize,
        height: usize,
    }

    fn owned(format: Format, width: usize, height: usize) -> Owned {
        let (cw, ch) = match format {
            Format::Yuv444 => (width, height),
            Format::Yuv422 => (width.div_ceil(2), height),
            Format::Yuv420 => (width.div_ceil(2), height.div_ceil(2)),
            Format::Yuv400 => (0, 0),
        };
        let plane = |w: usize, h: usize| PlaneBuf {
            width: w,
            height: h,
            samples: vec![128u8; w * h],
        };
        Owned {
            y: plane(width, height),
            u: plane(cw, ch),
            v: plane(cw, ch),
            format,
            width,
            height,
        }
    }

    impl Owned {
        fn picture(&self, matrix: u16, full_range: bool) -> Picture<'_, u8> {
            let colour = self.format != Format::Yuv400;
            Picture {
                width: self.width,
                height: self.height,
                depth: 8,
                format: self.format,
                matrix,
                primaries: 1,
                full_range,
                y: self.y.view(),
                u: colour.then(|| self.u.view()),
                v: colour.then(|| self.v.view()),
                alpha: None,
                alpha_premultiplied: false,
            }
        }
    }

    #[test]
    fn matrices_libavif_refuses_are_refused() {
        let p = owned(Format::Yuv444, 2, 2);
        for matrix in [3, 10, 11, 13, 14, 18, 200] {
            assert!(
                prepare(&p.picture(matrix, true)).is_err(),
                "matrix {matrix}"
            );
        }
        // YCgCo only at full range; identity only unsubsampled.
        assert!(prepare(&p.picture(MC_YCGCO, false)).is_err());
        assert!(prepare(&p.picture(MC_YCGCO, true)).is_ok());
        let sub = owned(Format::Yuv420, 2, 2);
        assert!(prepare(&sub.picture(MC_IDENTITY, true)).is_err());
        let grey = owned(Format::Yuv400, 2, 2);
        assert!(prepare(&grey.picture(MC_IDENTITY, true)).is_ok());
        // YCgCo-Re needs 10 bits for 8-bit RGB, and -Ro 9, which AV1 lacks.
        assert!(prepare(&p.picture(MC_YCGCO_RE, true)).is_err());
        assert!(prepare(&p.picture(MC_YCGCO_RO, true)).is_err());
        // 15 is reserved but below libavif's LAST, so it converts as BT.601.
        let fifteen = prepare(&p.picture(15, true)).map(|s| (s.kr, s.kb));
        assert_eq!(fifteen.ok(), Some((0.299, 0.114)));
        assert_eq!(to_argb(&p.picture(10, true)), Err(Error::Unsupported));
    }

    #[test]
    fn libyuv_takes_the_matrices_it_has_constants_for() {
        let sub = owned(Format::Yuv420, 2, 2);
        assert_eq!(libyuv_constants(&sub.picture(1, true)), Some(libyuv::F709));
        assert_eq!(libyuv_constants(&sub.picture(2, false)), Some(libyuv::I601));
        assert_eq!(
            libyuv_constants(&sub.picture(9, false)),
            Some(libyuv::BT2020)
        );
        let grey = owned(Format::Yuv400, 2, 2);
        assert_eq!(libyuv_constants(&grey.picture(0, true)), Some(libyuv::JPEG));
        let full = owned(Format::Yuv444, 2, 2);
        assert_eq!(libyuv_constants(&full.picture(0, true)), None);
        assert_eq!(libyuv_constants(&full.picture(4, true)), None);
        let mut derived = full.picture(12, true);
        derived.primaries = 9;
        assert_eq!(libyuv_constants(&derived), Some(libyuv::V2020));
        derived.primaries = 12;
        assert_eq!(libyuv_constants(&derived), None);
    }

    /// The chromaticity-derived coefficients of BT.709's primaries are
    /// BT.709's table entry, near enough.
    #[test]
    fn primaries_give_their_matrix() {
        let [kr, kg, kb] = primaries_coefficients(CP_BT709);
        assert!(
            (kr - 0.2126).abs() < 1e-3 && (kb - 0.0722).abs() < 1e-3,
            "{kr} {kb}"
        );
        assert!((kr + kg + kb - 1.0).abs() < 1e-6);
    }

    #[test]
    fn grey_and_mid_chroma_convert_to_grey_on_every_path() {
        // libyuv (BT.709), the fast path (FCC, 4:4:4) and the slow path
        // (FCC, 4:2:0) all make mid grey of 128/128/128 at full range.
        for (format, matrix) in [
            (Format::Yuv420, 1),
            (Format::Yuv444, 4),
            (Format::Yuv420, 4),
        ] {
            let p = owned(format, 3, 3);
            let out = to_argb(&p.picture(matrix, true)).unwrap_or_default();
            assert_eq!(out.len(), 9);
            for px in out {
                assert_eq!(px >> 24, 255);
                for shift in [0, 8, 16] {
                    assert!(
                        ((px >> shift) & 0xff).abs_diff(128) <= 1,
                        "{format:?} {matrix} {px:08x}"
                    );
                }
            }
        }
    }

    #[test]
    fn identity_is_a_reordering() {
        let mut p = owned(Format::Yuv444, 1, 1);
        p.y.samples = vec![10];
        p.u.samples = vec![20];
        p.v.samples = vec![30];
        let out = to_argb(&p.picture(MC_IDENTITY, true)).unwrap_or_default();
        // G = Y, B = U, R = V.
        assert_eq!(out, [0xff1e_0a14]);
    }

    #[test]
    fn deep_alpha_is_rescaled_in_floating_point() {
        let alpha = [0u16, 511, 1023];
        let plane = Plane {
            samples: &alpha,
            stride: 3,
            width: 3,
            height: 1,
        };
        let mut out = vec![0u32; 3];
        reformat_alpha(&plane, 10, 3, &mut out);
        // 511 / 1023 * 255 + 0.5 = 127.88: 127.
        assert_eq!(
            out.iter().map(|px| px >> 24).collect::<Vec<_>>(),
            [0, 127, 255]
        );
    }

    #[test]
    fn premultiplied_colour_is_divided_by_alpha() {
        let p = owned(Format::Yuv444, 1, 1);
        let alpha = PlaneBuf {
            width: 1,
            height: 1,
            samples: vec![128u8],
        };
        let mut picture = p.picture(1, true);
        picture.alpha = Some(alpha.view());
        picture.alpha_premultiplied = true;
        let out = to_argb(&picture).unwrap_or_default();
        // Grey 128 at alpha 128 unpremultiplies to about 255.
        assert_eq!(out[0] >> 24, 128);
        assert!((out[0] & 0xff) >= 254, "{:08x}", out[0]);
    }

    /// A decoder's planes have rows longer than the picture: what lies past
    /// each row's width is never read, on libyuv's path, the fast path or
    /// the slow one.
    #[test]
    fn padding_past_a_row_is_not_read() {
        for (format, matrix) in [
            (Format::Yuv420, 1),
            (Format::Yuv444, 4),
            (Format::Yuv420, 4),
        ] {
            let p = owned(format, 3, 3);
            let packed = to_argb(&p.picture(matrix, false)).unwrap();
            // The same samples, each row followed by 5 samples of 0xEE.
            let pad = |plane: &PlaneBuf<u8>| -> Vec<u8> {
                (0..plane.height)
                    .flat_map(|y| {
                        plane
                            .row(y)
                            .iter()
                            .copied()
                            .chain(core::iter::repeat_n(0xEE, 5))
                    })
                    .collect()
            };
            let (y, u, v) = (pad(&p.y), pad(&p.u), pad(&p.v));
            fn view<'a>(samples: &'a [u8], plane: &PlaneBuf<u8>) -> Plane<'a, u8> {
                Plane {
                    samples,
                    stride: plane.width + 5,
                    width: plane.width,
                    height: plane.height,
                }
            }
            let mut padded = p.picture(matrix, false);
            padded.y = view(&y, &p.y);
            padded.u = Some(view(&u, &p.u));
            padded.v = Some(view(&v, &p.v));
            assert_eq!(to_argb(&padded).unwrap(), packed, "{format:?} {matrix}");
        }
    }

    /// A plane smaller than the picture is refused rather than read short.
    #[test]
    fn a_plane_smaller_than_the_picture_is_refused() {
        let p = owned(Format::Yuv420, 4, 4);
        let mut picture = p.picture(1, false);
        picture.width = 5;
        assert_eq!(to_argb(&picture), Err(Error::Size));
        let mut picture = p.picture(1, false);
        let short = &p.u.samples[..3];
        picture.u = Some(Plane {
            samples: short,
            stride: 2,
            width: 2,
            height: 2,
        });
        assert_eq!(to_argb(&picture), Err(Error::Size));
        let mut picture = p.picture(1, false);
        picture.width = 0;
        assert_eq!(to_argb(&picture), Err(Error::Size));
        // A frame's buffer is kept and emptied on refusal.
        let mut out = vec![1u32; 4];
        assert_eq!(to_argb_into(&picture, &mut out), Err(Error::Size));
        assert!(out.is_empty());
    }
}
