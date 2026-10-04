//! libyuv's conversions from YUV to ARGB: the ones libavif calls
//! (`src/reformat_libyuv.c`) to turn a decoded AVIF into pixels, and which a
//! video frame is drawn with. Ported at the revision libavif 1.3.0 pins
//! (`4db2af62`, version 1909), and unchanged in 1924 (`644251f2`, libavif
//! 1.4.2's).
//!
//! Ported: the row conversions (`source/row_common.cc`: `YuvPixel`,
//! `YuvPixel10`, `YuvPixel12`, `YPixel` and the `I444`, `I410`, `I212` and
//! `I400` rows, with their alpha variants), the chroma upsamplers
//! (`source/scale_common.cc` and the `Any` wrappers in `source/scale_any.cc`,
//! which are what handle a row's two ends), the drivers that walk a picture's
//! rows through them (`source/convert_argb.cc`), `Convert16To8Plane` and
//! `ARGBUnattenuate` (`source/planar_functions.cc`).
//!
//! libyuv keeps each row function twice: in C, and in SIMD for each
//! architecture, the C written to reproduce the x86 SIMD sample for sample --
//! down to the order in which the x86 code reads its constant tables, which is
//! why the C has a separate x86 formulation. That formulation is what is
//! ported. The one function whose C does not match the x86 SIMD is
//! `ARGBUnattenuate` (see [`unattenuate`]).
//!
//! Pixels come out as `0xAARRGGBB` words: libyuv's "ARGB" names a
//! 32-bit word, B, G, R, A in memory, which is that word on a little-endian
//! machine. libavif's `AVIF_RGB_FORMAT_BGRA` is the same layout, and is what
//! reaches these functions with the `kYuv*` constants; its RGBA (Pillow's) gets
//! the same functions with the U and V planes swapped and the `kYvu*`
//! constants, which computes the same values with red and blue exchanged.
//!
//! Portions of this file are copyright 2011, 2013 and 2015 The LibYuv Project
//! Authors, from libyuv, and used under its BSD licence:
//! `licenses/libyuv-LICENSE`.

use alloc::vec;

use crate::{Plane, PlaneBuf};

/// A `YuvConstants` table, as the x86 code reads it: the Y scale and bias and
/// the four chroma weights, each weight in 1/64ths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Constants {
    /// `kYToRgb`: luma's scale, in 1/65536ths of 64/257ths.
    yg: u32,
    /// `kYBiasToRgb`: luma's offset, with the rounding for the final `>> 6`.
    yb: i32,
    /// `kUVToB`: U's weight in blue.
    ub: i32,
    /// `kUVToG[0]`: U's weight in green.
    ug: i32,
    /// `kUVToG[1]`: V's weight in green.
    vg: i32,
    /// `kUVToR[1]`: V's weight in red.
    vr: i32,
}

impl Constants {
    /// `MAKEYUVCONSTANTS(name, YG, YB, UB, UG, VG, VR)`.
    const fn new(yg: u32, yb: i32, ub: i32, ug: i32, vg: i32, vr: i32) -> Self {
        Self {
            yg,
            yb,
            ub,
            ug,
            vg,
            vr,
        }
    }
}

// The six tables libavif chooses from. The blue weights of the limited-range
// ones are 128 rather than the rounded 129, 135 and 137: libyuv caps them
// unless it is built with `LIBYUV_UNLIMITED_DATA`, which libavif's copy is not.

/// `kYuvI601Constants`: BT.601, limited range.
pub const I601: Constants = Constants::new(18997, -1160, 128, 25, 52, 102);
/// `kYuvJPEGConstants`: BT.601, full range.
pub const JPEG: Constants = Constants::new(16320, 32, 113, 22, 46, 90);
/// `kYuvH709Constants`: BT.709, limited range.
pub const H709: Constants = Constants::new(18997, -1160, 128, 14, 34, 115);
/// `kYuvF709Constants`: BT.709, full range.
pub const F709: Constants = Constants::new(16320, 32, 119, 12, 30, 101);
/// `kYuv2020Constants`: BT.2020, limited range.
pub const BT2020: Constants = Constants::new(19003, -1160, 128, 12, 42, 107);
/// `kYuvV2020Constants`: BT.2020, full range.
pub const V2020: Constants = Constants::new(16320, 32, 120, 11, 37, 94);

/// libyuv's `Clamp`: a value to a byte.
const fn clamp(v: i32) -> u32 {
    if v < 0 {
        0
    } else if v > 255 {
        255
    } else {
        v.unsigned_abs()
    }
}

/// `clamp255` of a value that cannot be negative.
const fn clamp255(v: u32) -> u32 {
    if v > 255 { 255 } else { v }
}

/// Alpha, as the top byte of a pixel.
const OPAQUE: u32 = 0xff00_0000;

/// `CALC_RGB16` and the final `>> 6` of `YuvPixel`: a pixel from luma
/// already widened to 16 bits (`y32`) and 8-bit chroma. Alpha is left zero.
#[inline]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "u and v are bytes and the weights under 256, so the chroma terms are within +-2^16; y32 * yg is the C's unsigned product, wrapping as it does, and shifted down to 16 bits"
)]
fn yuv_pixel(k: &Constants, y32: u32, u: u32, v: u32) -> u32 {
    // (uint32_t)(y32 * yg) >> 16: at most 0xffff, so the conversion is exact.
    let y1 = i32::try_from(y32.wrapping_mul(k.yg) >> 16).unwrap_or(0) + k.yb;
    // `(int8_t)u - 0x80`, which is `u - 128` for every byte.
    let ui = i32::try_from(u).unwrap_or(0) - 128;
    let vi = i32::try_from(v).unwrap_or(0) - 128;
    let b = y1 + ui * k.ub;
    let g = y1 - (ui * k.ug + vi * k.vg);
    let r = y1 + vi * k.vr;
    (clamp(r >> 6) << 16) | (clamp(g >> 6) << 8) | clamp(b >> 6)
}

/// A sample size, and how libyuv brings its samples to the 16-bit luma and
/// 8-bit chroma and alpha that [`yuv_pixel`] takes.
pub trait Depth {
    /// The sample: `u8` or `u16`.
    type Sample: Copy + Default + Into<u32>;

    /// Luma widened to 16 bits by repeating its top bits below it.
    fn y32(y: Self::Sample) -> u32;

    /// Chroma cut to 8 bits.
    fn uv8(c: Self::Sample) -> u32;

    /// Alpha cut to 8 bits.
    fn a8(a: Self::Sample) -> u32;

    /// A value the upsamplers computed, which fits by construction: a
    /// weighted average of samples.
    fn narrow(v: u32) -> Self::Sample;
}

/// 8-bit samples: `YuvPixel`.
pub struct Eight;

/// 10-bit samples: `YuvPixel10`.
pub struct Ten;

/// 12-bit samples: `YuvPixel12`.
pub struct Twelve;

impl Depth for Eight {
    type Sample = u8;

    #[allow(
        clippy::arithmetic_side_effects,
        reason = "a byte times 0x0101 is at most 0xffff"
    )]
    fn y32(y: u8) -> u32 {
        u32::from(y) * 0x0101
    }

    fn uv8(c: u8) -> u32 {
        u32::from(c)
    }

    fn a8(a: u8) -> u32 {
        u32::from(a)
    }

    fn narrow(v: u32) -> u8 {
        u8::try_from(v).unwrap_or(u8::MAX)
    }
}

impl Depth for Ten {
    type Sample = u16;

    #[allow(
        clippy::arithmetic_side_effects,
        reason = "a 16-bit sample shifted left by 6 is under 2^22"
    )]
    fn y32(y: u16) -> u32 {
        let y = u32::from(y);
        (y << 6) | (y >> 4)
    }

    fn uv8(c: u16) -> u32 {
        clamp255(u32::from(c) >> 2)
    }

    fn a8(a: u16) -> u32 {
        clamp255(u32::from(a) >> 2)
    }

    fn narrow(v: u32) -> u16 {
        u16::try_from(v).unwrap_or(u16::MAX)
    }
}

impl Depth for Twelve {
    type Sample = u16;

    #[allow(
        clippy::arithmetic_side_effects,
        reason = "a 16-bit sample shifted left by 4 is under 2^20"
    )]
    fn y32(y: u16) -> u32 {
        let y = u32::from(y);
        (y << 4) | (y >> 8)
    }

    fn uv8(c: u16) -> u32 {
        clamp255(u32::from(c) >> 4)
    }

    fn a8(a: u16) -> u32 {
        clamp255(u32::from(a) >> 4)
    }

    fn narrow(v: u32) -> u16 {
        u16::try_from(v).unwrap_or(u16::MAX)
    }
}

/// `I444ToARGBRow_C`, `I444AlphaToARGBRow_C`, `I410ToARGBRow_C` and
/// `I410AlphaToARGBRow_C`: a row whose chroma is at full width -- as it is
/// in 4:4:4, and after the upsamplers.
fn row_444<D: Depth>(
    k: &Constants,
    y: &[D::Sample],
    u: &[D::Sample],
    v: &[D::Sample],
    a: Option<&[D::Sample]>,
    out: &mut [u32],
) {
    let pixels = out.iter_mut().zip(y).zip(u.iter().zip(v));
    match a {
        Some(a) => {
            for (((out, &y), (&u, &v)), &a) in pixels.zip(a) {
                *out = yuv_pixel(k, D::y32(y), D::uv8(u), D::uv8(v)) | (D::a8(a) << 24);
            }
        }
        None => {
            for ((out, &y), (&u, &v)) in pixels {
                *out = yuv_pixel(k, D::y32(y), D::uv8(u), D::uv8(v)) | OPAQUE;
            }
        }
    }
}

/// `I212ToARGBRow_C`: a 12-bit row whose chroma is at half width, each
/// chroma sample used for the two pixels over it -- nearest-neighbour, the
/// only way libyuv converts 12-bit 4:2:0 (`I012ToARGBMatrix`).
fn row_212(k: &Constants, y: &[u16], u: &[u16], v: &[u16], out: &mut [u32]) {
    for (pair, (y, (&u, &v))) in out.chunks_mut(2).zip(y.chunks(2).zip(u.iter().zip(v))) {
        for (out, &y) in pair.iter_mut().zip(y) {
            *out = yuv_pixel(k, Twelve::y32(y), Twelve::uv8(u), Twelve::uv8(v)) | OPAQUE;
        }
    }
}

/// `I400ToARGBRow_C` (`YPixel`): grey, from luma alone.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "y * 0x0101 * yg is under 2^31, as it is in the C's int arithmetic, and the bias is small"
)]
fn row_400(k: &Constants, y: &[u8], out: &mut [u32]) {
    for (out, &y) in out.iter_mut().zip(y) {
        let y1 = (u32::from(y) * 0x0101 * k.yg) >> 16;
        let grey = clamp((i32::try_from(y1).unwrap_or(0) + k.yb) >> 6);
        *out = OPAQUE | (grey << 16) | (grey << 8) | grey;
    }
}

/// `ScaleRowUp2_Linear_Any_C` and its 16-bit twin: a chroma row doubled in
/// width to `dst.len()`, each new sample 3:1 of its two nearest, and the two
/// ends copied.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "samples are at most 16 bits, so every weighted sum is under 2^19; w is at least 1"
)]
fn up2_linear<D: Depth>(src: &[D::Sample], dst: &mut [D::Sample]) {
    let w = dst.len();
    let (Some(&first), Some(first_out)) = (src.first(), dst.first_mut()) else {
        return;
    };
    *first_out = first;
    let work = (w - 1) & !1;
    if let Some(inner) = dst.get_mut(1..=work) {
        for (pair, s) in inner.chunks_exact_mut(2).zip(src.windows(2)) {
            let (&[s0, s1], [d0, d1]) = (s, pair) else {
                continue;
            };
            let (s0, s1): (u32, u32) = (s0.into(), s1.into());
            *d0 = D::narrow((s0 * 3 + s1 + 2) >> 2);
            *d1 = D::narrow((s0 + s1 * 3 + 2) >> 2);
        }
    }
    if let (Some(&last), Some(last_out)) = (src.get((w - 1) / 2), dst.last_mut()) {
        *last_out = last;
    }
}

/// `ScaleRowUp2_Bilinear_Any_C` and its 16-bit twin: two chroma rows
/// (`sa` above `sb`) to the two output rows between them, 9:3:3:1 of the four
/// nearest samples; each row's two ends are 3:1 of the column there.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "samples are at most 16 bits, so every weighted sum is under 2^21; w is at least 1"
)]
fn up2_bilinear<D: Depth>(
    sa: &[D::Sample],
    sb: &[D::Sample],
    da: &mut [D::Sample],
    db: &mut [D::Sample],
) {
    let w = da.len();
    let at = |row: &[D::Sample], i: usize| -> u32 { row.get(i).map_or(0, |&s| s.into()) };
    let (Some(da0), Some(db0)) = (da.first_mut(), db.first_mut()) else {
        return;
    };
    let (a0, b0) = (at(sa, 0), at(sb, 0));
    *da0 = D::narrow((a0 * 3 + b0 + 2) >> 2);
    *db0 = D::narrow((a0 + b0 * 3 + 2) >> 2);
    let work = (w - 1) & !1;
    if let (Some(inner_a), Some(inner_b)) = (da.get_mut(1..=work), db.get_mut(1..=work)) {
        let outputs = inner_a.chunks_exact_mut(2).zip(inner_b.chunks_exact_mut(2));
        for ((pa, pb), (s, t)) in outputs.zip(sa.windows(2).zip(sb.windows(2))) {
            let (&[s0, s1], &[t0, t1], [above0, above1], [below0, below1]) = (s, t, pa, pb) else {
                continue;
            };
            let (s0, s1, t0, t1): (u32, u32, u32, u32) =
                (s0.into(), s1.into(), t0.into(), t1.into());
            *above0 = D::narrow((s0 * 9 + s1 * 3 + t0 * 3 + t1 + 8) >> 4);
            *above1 = D::narrow((s0 * 3 + s1 * 9 + t0 + t1 * 3 + 8) >> 4);
            *below0 = D::narrow((s0 * 3 + s1 + t0 * 9 + t1 * 3 + 8) >> 4);
            *below1 = D::narrow((s0 + s1 * 3 + t0 * 3 + t1 * 9 + 8) >> 4);
        }
    }
    let last = (w - 1) / 2;
    let (a, b) = (at(sa, last), at(sb, last));
    if let Some(out) = da.last_mut() {
        *out = D::narrow((a * 3 + b + 2) >> 2);
    }
    if let Some(out) = db.last_mut() {
        *out = D::narrow((a + b * 3 + 2) >> 2);
    }
}

/// A picture's planes, as a libyuv driver takes them.
#[derive(Debug)]
pub struct Planes<'a, T> {
    pub y: Plane<'a, T>,
    pub u: Plane<'a, T>,
    pub v: Plane<'a, T>,
    /// Present for the `*Alpha*` drivers, which carry it into the pixels.
    pub a: Option<Plane<'a, T>>,
}

impl<T> Clone for Planes<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Planes<'_, T> {}

impl<T> Planes<'_, T> {
    fn alpha_row(&self, row: usize) -> Option<&[T]> {
        self.a.map(|a| a.row(row))
    }
}

/// `I420ToARGBMatrixBilinear` / `I420AlphaToARGBMatrixBilinear` (8-bit) and
/// `I010ToARGBMatrixBilinear` / `I010AlphaToARGBMatrixBilinear` (10-bit):
/// 4:2:0, chroma upsampled bilinearly.
///
/// The first row takes the first chroma row alone; every later pair of rows
/// takes the two chroma rows around it, 3:1 and 1:3; and a last row left over
/// (an even height) takes the last chroma row alone again.
pub fn i420_bilinear<D: Depth>(
    k: &Constants,
    planes: &Planes<'_, D::Sample>,
    width: usize,
    out: &mut [u32],
) {
    let mut u1 = vec![D::Sample::default(); width];
    let mut u2 = vec![D::Sample::default(); width];
    let mut v1 = vec![D::Sample::default(); width];
    let mut v2 = vec![D::Sample::default(); width];
    let height = out.len().checked_div(width).unwrap_or(0);
    let mut rows = out.chunks_exact_mut(width.max(1)).enumerate();
    let Some((_, first)) = rows.next() else {
        return;
    };
    up2_linear::<D>(planes.u.row(0), &mut u1);
    up2_linear::<D>(planes.v.row(0), &mut v1);
    row_444::<D>(k, planes.y.row(0), &u1, &v1, planes.alpha_row(0), first);
    let mut chroma = 0usize;
    // `for (y = 0; y < height - 2; y += 2)`, one chroma row a turn.
    let pairs = height.saturating_sub(1) / 2;
    for _ in 0..pairs {
        let next = chroma.saturating_add(1);
        up2_bilinear::<D>(planes.u.row(chroma), planes.u.row(next), &mut u1, &mut u2);
        up2_bilinear::<D>(planes.v.row(chroma), planes.v.row(next), &mut v1, &mut v2);
        for (u, v) in [(&u1, &v1), (&u2, &v2)] {
            if let Some((row, out)) = rows.next() {
                row_444::<D>(k, planes.y.row(row), u, v, planes.alpha_row(row), out);
            }
        }
        chroma = next;
    }
    if let Some((row, out)) = rows.next() {
        // The even height's last row.
        up2_linear::<D>(planes.u.row(chroma), &mut u1);
        up2_linear::<D>(planes.v.row(chroma), &mut v1);
        row_444::<D>(k, planes.y.row(row), &u1, &v1, planes.alpha_row(row), out);
    }
}

/// `I422ToARGBMatrixLinear` / `I422AlphaToARGBMatrixLinear` (8-bit) and
/// `I210ToARGBMatrixLinear` / `I210AlphaToARGBMatrixLinear` (10-bit): 4:2:2,
/// each chroma row upsampled across.
pub fn i422_linear<D: Depth>(
    k: &Constants,
    planes: &Planes<'_, D::Sample>,
    width: usize,
    out: &mut [u32],
) {
    let mut u = vec![D::Sample::default(); width];
    let mut v = vec![D::Sample::default(); width];
    for (row, out) in out.chunks_exact_mut(width.max(1)).enumerate() {
        up2_linear::<D>(planes.u.row(row), &mut u);
        up2_linear::<D>(planes.v.row(row), &mut v);
        row_444::<D>(k, planes.y.row(row), &u, &v, planes.alpha_row(row), out);
    }
}

/// `I444ToARGBMatrix` / `I444AlphaToARGBMatrix` (8-bit) and
/// `I410ToARGBMatrix` / `I410AlphaToARGBMatrix` (10-bit): 4:4:4.
pub fn i444<D: Depth>(
    k: &Constants,
    planes: &Planes<'_, D::Sample>,
    width: usize,
    out: &mut [u32],
) {
    for (row, out) in out.chunks_exact_mut(width.max(1)).enumerate() {
        row_444::<D>(
            k,
            planes.y.row(row),
            planes.u.row(row),
            planes.v.row(row),
            planes.alpha_row(row),
            out,
        );
    }
}

/// `I012ToARGBMatrix`: 12-bit 4:2:0, each chroma sample used for the 2x2
/// pixels over it. Never with alpha: libyuv has no such function, so libavif
/// adds the alpha itself.
pub fn i012(k: &Constants, planes: &Planes<'_, u16>, width: usize, out: &mut [u32]) {
    for (row, out) in out.chunks_exact_mut(width.max(1)).enumerate() {
        let chroma = row / 2;
        row_212(
            k,
            planes.y.row(row),
            planes.u.row(chroma),
            planes.v.row(chroma),
            out,
        );
    }
}

/// `I400ToARGBMatrix`: grey. Never with alpha: libavif adds it.
pub fn i400(k: &Constants, y: Plane<'_, u8>, width: usize, out: &mut [u32]) {
    for (row, out) in out.chunks_exact_mut(width.max(1)).enumerate() {
        row_400(k, y.row(row), out);
    }
}

/// `Convert16To8Plane` with libavif's scale, `1 << (24 - depth)`: a 10- or
/// 12-bit plane cut to 8 bits, as libavif does before handing a picture libyuv
/// has no deep function for to an 8-bit one.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "depth is 10 or 12, so the scale is 2^14 or 2^12 and a 16-bit sample times it is under 2^31"
)]
pub fn convert_16_to_8(plane: Plane<'_, u16>, depth: u8) -> PlaneBuf<u8> {
    let scale = 1u32 << 24u32.saturating_sub(u32::from(depth)).min(16);
    PlaneBuf {
        width: plane.width,
        height: plane.height,
        samples: (0..plane.height)
            .flat_map(|y| plane.row(y))
            .map(|&s| Eight::narrow(clamp255((u32::from(s) * scale) >> 16)))
            .collect(),
    }
}

/// `fixed_invtbl8[a] & 0xffff`: `0x10000 / a` in 8.8 fixed point, with the
/// table's three special entries -- 0 for no alpha, `0xffff` for 1 (where
/// `0x10000` would not fit), and `0x100` for opaque.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the division is by an alpha of at least 2"
)]
const fn inverse(a: u32) -> u32 {
    match a {
        0 => 0,
        1 => 0xffff,
        255.. => 0x100,
        a => 0x10000 / a,
    }
}

/// `ARGBUnattenuate` as `ARGBUnattenuateRow_C` computes it: each colour
/// channel divided by alpha in 8.8 fixed point, `(c * (0x10000 / a)) >> 8`.
///
/// This is the one row whose C and x86 SIMD differ: the SSE2 and AVX2 code
/// widens the channel to 16 bits by repeating it (`c * 257`) and shifts by
/// 16, which rounds up where this rounds down about as often as not. The C is
/// what runs where libyuv is compiled by MSVC for x86-64 -- Pillow's Windows
/// build, among others, since libyuv's x86 row code is written for GCC and
/// Clang -- and the C is libyuv's reference, so it is what is ported. The
/// file this matters for is rare: straight alpha only needs undoing when a
/// file says its colour was premultiplied (a `prem` reference).
#[allow(
    clippy::arithmetic_side_effects,
    reason = "a channel is a byte and the inverse at most 0xffff, so the product is under 2^24"
)]
pub fn unattenuate(pixels: &mut [u32]) {
    for pixel in pixels {
        let a = *pixel >> 24;
        let ia = inverse(a);
        let channel = |shift: u32| clamp255((((*pixel >> shift) & 0xff) * ia) >> 8) << shift;
        *pixel = (a << 24) | channel(16) | channel(8) | channel(0);
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests index fixed arrays they build"
)]
mod tests {
    use super::*;

    fn plane<T: Clone>(width: usize, samples: &[T]) -> PlaneBuf<T> {
        PlaneBuf {
            width,
            height: samples.len() / width,
            samples: samples.to_vec(),
        }
    }

    /// `YuvPixel` by hand for full-range BT.601 grey and the two chroma
    /// extremes, against libyuv's own figures.
    #[test]
    fn yuv_pixel_matches_the_x86_arithmetic() {
        // Mid grey: y1 = (128 * 257 * 16320) >> 16 + 32 = 8223; >> 6 = 128.
        assert_eq!(yuv_pixel(&JPEG, Eight::y32(128), 128, 128), 0x0080_8080);
        // White and black, limited range: 235 and 16 are the ends.
        assert_eq!(yuv_pixel(&I601, Eight::y32(235), 128, 128), 0x00ff_ffff);
        assert_eq!(yuv_pixel(&I601, Eight::y32(16), 128, 128), 0x0000_0000);
        // Full blue chroma saturates blue and leaves red at luma.
        let px = yuv_pixel(&JPEG, Eight::y32(128), 255, 128);
        assert_eq!(px & 0xff, 255);
        assert_eq!((px >> 16) & 0xff, 128);
    }

    /// A 10-bit sample is the 8-bit one shifted up two bits, and converts to
    /// the same pixel when its low bits are zero.
    #[test]
    fn ten_bit_samples_match_their_eight_bit_equivalents() {
        for (y, u, v) in [(16u16, 128u16, 128u16), (200, 30, 220), (235, 240, 16)] {
            let eight = yuv_pixel(&H709, Eight::y32(y as u8), u32::from(u), u32::from(v));
            let ten = yuv_pixel(&H709, Ten::y32(y << 2), Ten::uv8(u << 2), Ten::uv8(v << 2));
            // y32 differs in the repeated low bits only, which the >> 16 of
            // the product mostly drops; the chroma is exact.
            let diff = |shift: u32| ((eight >> shift) & 0xff).abs_diff((ten >> shift) & 0xff);
            assert!(diff(0) <= 1 && diff(8) <= 1 && diff(16) <= 1, "{y} {u} {v}");
        }
    }

    #[test]
    fn linear_upsampling_copies_the_ends_and_weights_the_middle() {
        let mut out = [0u8; 6];
        up2_linear::<Eight>(&[0, 100, 200], &mut out);
        // 0, (3*0+100+2)>>2, (0+300+2)>>2, (300+200+2)>>2, (100+600+2)>>2, 200.
        assert_eq!(out, [0, 25, 75, 125, 175, 200]);
        // An odd width's last sample is the chroma sample under it.
        let mut odd = [0u8; 5];
        up2_linear::<Eight>(&[0, 100, 200], &mut odd);
        assert_eq!(odd, [0, 25, 75, 125, 200]);
        let mut one = [0u8; 1];
        up2_linear::<Eight>(&[77], &mut one);
        assert_eq!(one, [77]);
    }

    #[test]
    fn bilinear_upsampling_weights_nine_three_three_one() {
        let (mut a, mut b) = ([0u16; 4], [0u16; 4]);
        up2_bilinear::<Ten>(&[0, 160], &[320, 480], &mut a, &mut b);
        // Ends: (3*0+320+2)>>2 = 80, (0+960+2)>>2 = 240; last column
        // (3*160+480+2)>>2 = 240, (160+1440+2)>>2 = 400.
        assert_eq!(a[0], 80);
        assert_eq!(b[0], 240);
        assert_eq!(a[3], 240);
        assert_eq!(b[3], 400);
        // Middle: (0*9 + 160*3 + 320*3 + 480 + 8) >> 4 = 120.
        assert_eq!(a[1], 120);
        assert_eq!(a[2], (160 * 9 + 320 + 480 * 3 + 8) >> 4);
        assert_eq!(b[1], (160 + 320 * 9 + 480 * 3 + 8) >> 4);
        assert_eq!(b[2], (160 * 3 + 320 * 3 + 480 * 9 + 8) >> 4);
    }

    /// The 4:2:0 driver's row schedule: the first and (for an even height)
    /// last rows see one chroma row; the rest see two, 3:1 and 1:3.
    #[test]
    fn i420_rows_take_their_chroma_as_libyuv_does() {
        // 2x4 luma, 1x2 chroma: rows 0 and 3 take chroma rows 0 and 1 alone,
        // rows 1 and 2 blend them.
        let y = plane(2, &[128u8; 8]);
        let u = plane(1, &[128u8, 128]);
        let v = plane(1, &[0u8, 255]);
        let planes = Planes {
            y: y.view(),
            u: u.view(),
            v: v.view(),
            a: None,
        };
        let mut out = vec![0u32; 8];
        i420_bilinear::<Eight>(&JPEG, &planes, 2, &mut out);
        let red = |px: u32| (px >> 16) & 0xff;
        let expect = |v: u8| red(yuv_pixel(&JPEG, Eight::y32(128), 128, u32::from(v)));
        // (3*0 + 255 + 2) >> 2 = 64 and (0 + 3*255 + 2) >> 2 = 191.
        assert_eq!(red(out[0]), expect(0));
        assert_eq!(red(out[2]), expect(64));
        assert_eq!(red(out[4]), expect(191));
        assert_eq!(red(out[6]), expect(255));
        assert!(out.iter().all(|&px| px >> 24 == 255));
    }

    /// An odd height has no lone last row: its last pair of rows blends.
    #[test]
    fn i420_odd_height_ends_on_a_blended_pair() {
        let y = plane(1, &[128u8; 3]);
        let u = plane(1, &[128u8, 128]);
        let v = plane(1, &[0u8, 255]);
        let planes = Planes {
            y: y.view(),
            u: u.view(),
            v: v.view(),
            a: None,
        };
        let mut out = vec![0u32; 3];
        i420_bilinear::<Eight>(&JPEG, &planes, 1, &mut out);
        let red = |px: u32| (px >> 16) & 0xff;
        let expect = |v: u8| red(yuv_pixel(&JPEG, Eight::y32(128), 128, u32::from(v)));
        assert_eq!(red(out[2]), expect(191));
    }

    #[test]
    fn alpha_rows_carry_alpha_and_ten_bit_alpha_is_cut() {
        let y = plane(2, &[512u16, 512]);
        let c = plane(2, &[512u16, 512]);
        let a = plane(2, &[1023u16, 4]);
        let planes = Planes {
            y: y.view(),
            u: c.view(),
            v: c.view(),
            a: Some(a.view()),
        };
        let mut out = vec![0u32; 2];
        i444::<Ten>(&V2020, &planes, 2, &mut out);
        assert_eq!(out[0] >> 24, 255);
        assert_eq!(out[1] >> 24, 1);
    }

    #[test]
    fn twelve_bit_420_is_nearest_neighbour() {
        let y = plane(3, &[2048u16; 6]);
        let u = plane(2, &[2048u16, 2048, 2048, 2048]);
        let v = plane(2, &[0u16, 4095, 0, 4095]);
        let planes = Planes {
            y: y.view(),
            u: u.view(),
            v: v.view(),
            a: None,
        };
        let mut out = vec![0u32; 6];
        i012(&BT2020, &planes, 3, &mut out);
        assert_eq!(out[0], out[1]);
        assert_ne!(out[1], out[2]);
        assert_eq!(out[0], out[3]);
    }

    #[test]
    fn grey_is_ypixel() {
        let y = plane(3, &[0u8, 128, 255]);
        let mut out = vec![0u32; 3];
        i400(&JPEG, y.view(), 3, &mut out);
        assert_eq!(out, [0xff00_0000, 0xff80_8080, 0xffff_ffff]);
    }

    #[test]
    fn deep_planes_cut_to_eight_bits_by_shifting() {
        let ten = plane(3, &[0u16, 513, 1023]);
        assert_eq!(convert_16_to_8(ten.view(), 10).samples, [0, 128, 255]);
        let twelve = plane(2, &[4095u16, 4096]);
        assert_eq!(convert_16_to_8(twelve.view(), 12).samples, [255, 255]);
    }

    #[test]
    fn unattenuate_divides_by_alpha_in_fixed_point() {
        // 100 at alpha 200: (100 * 327) >> 8 = 127, where the SIMD's
        // (100 * 257 * 327) >> 16 would say 128.
        let mut px = [0xc864_6464u32, 0x0012_3456, 0xff01_02ff, 0x0105_0000];
        unattenuate(&mut px);
        assert_eq!(px[0], 0xc87f_7f7f);
        // Alpha 0 clears the colour; alpha 255 multiplies by 256/256.
        assert_eq!(px[1], 0x0000_0000);
        assert_eq!(px[2], 0xff01_02ff);
        // Alpha 1: 0xffff / 256 saturates.
        assert_eq!(px[3], 0x01ff_0000);
    }
}
