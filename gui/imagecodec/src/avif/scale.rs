//! Bringing a decoded AV1 frame to the size its container declares: what
//! libavif does when a frame was coded at another size than its `ispe` (or
//! its track's) -- `avifImageScaleWithLimit` (`src/scale.c`), which scales
//! each plane with libyuv's `ScalePlane`, or `ScalePlane_12` for deeper
//! samples, asking for `kFilterBox`.
//!
//! libyuv takes `kFilterBox` as a ceiling, not an order (`ScaleFilterReduce`):
//! the box filter -- each sample the mean of the source samples its box
//! covers -- only where both sides shrink below half; bilinear interpolation
//! otherwise; linear (across only) where the height stays or shrinks to a
//! third, or the source is one row; and nearest-sample where the width stays
//! or shrinks to a third under linear, or the source is one sample wide.
//! Then it picks code by the sizes ([`Method`]): a copy, rows alone, fixed
//! kernels for 3/4, 1/2, 3/8 and 1/4 on both sides and for twice the size,
//! and the general box, bilinear and nearest-sample scalers. Each is ported
//! as libyuv's C computes it: its 16.16 fixed-point steps, its roundings, the
//! samples it picks at the edges, and the sums it lets wrap.
//!
//! The C is libyuv's reference, and what runs wherever libyuv is compiled by
//! MSVC for x86-64 -- Pillow's Windows build among them, whose libavif 1.4.2
//! links libyuv 1924 (`644251f2`), the revision followed here -- since its
//! x86 code is written for GCC and Clang. That x86 code computes the same
//! samples as the C everywhere this reaches but two places, both 8-bit: a
//! plane cut to 3/4 or 3/8 on both sides, whose SSSE3 box rows round
//! otherwise; and a box filter more than 257 rows tall over bright samples,
//! whose 16-bit column sums the C lets wrap and SSE2 holds at their ceiling
//! (white comes out 32 from one and 95 from the other). (Found by building
//! libyuv 1924 both ways and comparing every combination of 25 sizes from 1
//! to 129 on each side, at 8, 10 and 12 bits, and the cases in the tests.)
//!
//! libavif scales 10-bit planes with `ScalePlane_12` as well as 12-bit ones.
//! Its C is `ScalePlane_16`'s, which differs from the 8-bit code only in
//! types ([`Scaled`]): a horizontal blend with all 16 bits of the position's
//! fraction, where 8-bit samples use 7 as libyuv's x86 code does, and box
//! sums in 32 bits, where 8-bit ones are kept in 16 and wrap.
//!
//! Portions of this file are copyright 2011 and 2013 The LibYuv Project
//! Authors, from libyuv's `source/scale.cc`, `source/scale_common.cc`,
//! `source/scale_any.cc` and `source/row_common.cc`, used under libyuv's BSD
//! licence (`licenses/libyuv-LICENSE`).

use alloc::vec;

use super::Error;
use super::decode::{Plane, Sample};

/// A sample libyuv scales: 8-bit through `ScalePlane`, 16-bit through
/// `ScalePlane_12`. The two differ only here.
pub(crate) trait Scaled: Sample {
    /// `ScaleFilterCols`' `BLENDER`: `a` moved towards `b` by `f` / 65536.
    fn blend(a: u32, b: u32, f: u32) -> Self;
    /// `ScaleAddRow`: `sample` added to a column's box sum, held as libyuv
    /// holds it.
    fn add_to_sum(sum: u32, sample: Self) -> u32;
    /// `v` stored in a sample, as C's conversion stores it: the low bits.
    fn store(v: u32) -> Self;
}

impl Scaled for u8 {
    /// The x86 form, which the C copies: the fraction cut to 7 bits, the
    /// step rounded at 1/128.
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "a and b are bytes and f >> 9 under 128, so the product is within +-2^15; the result lies between a and b, so the conversion to a byte is C's (uint8_t) of a byte"
    )]
    fn blend(a: u32, b: u32, f: u32) -> Self {
        let (a, b, f) = (a as i32, b as i32, (f >> 9) as i32);
        (a + ((f * (b - a) + 0x40) >> 7)) as u8
    }

    /// In 16 bits, wrapping: `ScaleAddRow_C`'s `uint16_t` row, which a box
    /// over 257 rows of white overflows.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "a 16-bit sum plus a byte is under 2^17"
    )]
    fn add_to_sum(sum: u32, sample: u8) -> u32 {
        (sum + u32::from(sample)) & 0xffff
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "C's conversion to uint8_t: the low 8 bits"
    )]
    fn store(v: u32) -> u8 {
        v as u8
    }
}

impl Scaled for u16 {
    /// `ScaleFilterCols_16_C`'s, with the whole 16-bit fraction rounded at
    /// 1/65536.
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "16-bit samples and fraction: the product is within +-2^32 in an i64, and the result lies between a and b, so the conversion to 16 bits is C's (uint16_t) of a 16-bit value"
    )]
    fn blend(a: u32, b: u32, f: u32) -> Self {
        let (a, b, f) = (i64::from(a), i64::from(b), i64::from(f));
        (a + ((f * (b - a) + 0x8000) >> 16)) as u16
    }

    /// In 32 bits: `ScaleAddRow_16_C`'s `uint32_t` row.
    fn add_to_sum(sum: u32, sample: u16) -> u32 {
        sum.wrapping_add(u32::from(sample))
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "C's conversion to uint16_t: the low 16 bits"
    )]
    fn store(v: u32) -> u16 {
        v as u16
    }
}

/// libyuv's `FilterMode`, as `ScaleFilterReduce` leaves `kFilterBox`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Filter {
    /// `kFilterNone`: the nearest sample.
    Nearest,
    /// `kFilterLinear`: interpolated across, the nearest row.
    Linear,
    /// `kFilterBilinear`.
    Bilinear,
    /// `kFilterBox`.
    Box,
}

/// `ScaleFilterReduce(..., kFilterBox)`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "sizes are at most 32768, so their small multiples fit any usize"
)]
fn reduce(sw: usize, sh: usize, dw: usize, dh: usize) -> Filter {
    let mut filter = Filter::Box;
    // Either side scaled to half or more: bilinear.
    if dw * 2 >= sw || dh * 2 >= sh {
        filter = Filter::Bilinear;
    }
    if filter == Filter::Bilinear {
        if sh == 1 || dh == sh || dh * 3 == sh {
            filter = Filter::Linear;
        }
        if sw == 1 {
            filter = Filter::Nearest;
        }
    }
    if filter == Filter::Linear && (sw == 1 || dw == sw || dw * 3 == sw) {
        filter = Filter::Nearest;
    }
    filter
}

/// The code `ScalePlane` (or `ScalePlane_12`) runs for a pair of sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Method {
    /// `CopyPlane`: the same size.
    Copy,
    /// `ScalePlaneVertical`: the same width.
    Vertical(Filter),
    /// `ScalePlaneDown34`: 3/4 on both sides.
    Down34,
    /// `ScalePlaneDown2`: half on both sides.
    Down2,
    /// `ScalePlaneDown38`: 3/8 on both sides.
    Down38,
    /// `ScalePlaneDown4`: a quarter on both sides.
    Down4,
    /// `ScalePlaneBox`: both sides below half.
    Box,
    /// `ScalePlaneUp2_Linear` (`_12_Linear`): twice as wide (or one less),
    /// rows the nearest.
    Up2Linear,
    /// `ScalePlaneUp2_Bilinear` (`_12_Bilinear`): twice the size (or one
    /// less) on both sides.
    Up2Bilinear,
    /// `ScalePlaneBilinearUp`: taller.
    BilinearUp(Filter),
    /// `ScalePlaneBilinearDown`: no taller.
    BilinearDown(Filter),
    /// `ScalePlaneSimple`: the nearest sample.
    Simple,
}

/// `ScalePlane`'s choice, which is `ScalePlane_12`'s: that tests for twice
/// the size first, where `ScalePlane_16` tests last, but nothing tested
/// between can hold for twice the size (each needs the width kept or cut).
#[allow(
    clippy::arithmetic_side_effects,
    reason = "sizes are at most 32768, so their small multiples fit any usize"
)]
fn method(sw: usize, sh: usize, dw: usize, dh: usize) -> Method {
    let filter = reduce(sw, sh, dw, dh);
    if (dw, dh) == (sw, sh) {
        return Method::Copy;
    }
    if dw == sw && filter != Filter::Box {
        return Method::Vertical(filter);
    }
    // The fixed ratios. Their filter is fixed too, by the ratio: 3/4 and 1/2
    // reduce kFilterBox to bilinear, 3/8 and 1/4 keep it, so each runs its
    // box rows -- libyuv's code for the other filters cannot be reached
    // from kFilterBox, and is not ported. (libyuv takes 1/4 for kFilterNone
    // as well, which cannot arise either.)
    if dw <= sw && dh <= sh {
        if 4 * dw == 3 * sw && 4 * dh == 3 * sh {
            return Method::Down34;
        }
        if 2 * dw == sw && 2 * dh == sh {
            return Method::Down2;
        }
        if 8 * dw == 3 * sw && 8 * dh == 3 * sh {
            return Method::Down38;
        }
        if 4 * dw == sw && 4 * dh == sh && filter == Filter::Box {
            return Method::Down4;
        }
    }
    if filter == Filter::Box && dh * 2 < sh {
        return Method::Box;
    }
    let twice_as_wide = dw.div_ceil(2) == sw;
    if twice_as_wide && filter == Filter::Linear {
        return Method::Up2Linear;
    }
    if twice_as_wide && dh.div_ceil(2) == sh && matches!(filter, Filter::Bilinear | Filter::Box) {
        return Method::Up2Bilinear;
    }
    match filter {
        Filter::Nearest => Method::Simple,
        _ if dh > sh => Method::BilinearUp(filter),
        _ => Method::BilinearDown(filter),
    }
}

/// `src` scaled to `width` x `height` as libyuv's `ScalePlane` (8-bit) or
/// `ScalePlane_12` (16-bit) does with `kFilterBox`.
///
/// # Errors
///
/// When the new plane's size overflows. Both sizes must be at least 1;
/// libavif checks them before it gets here, and refuses a source over
/// 16384 on a side, which libyuv's fixed point would overflow.
pub(crate) fn scale_plane<T: Scaled>(
    src: &Plane<T>,
    width: usize,
    height: usize,
) -> Result<Plane<T>, Error> {
    let mut dst = Plane::new(width, height)?;
    if src.width == 0 || src.height == 0 || width == 0 || height == 0 {
        return Ok(dst);
    }
    match method(src.width, src.height, width, height) {
        Method::Copy => dst.samples.clone_from(&src.samples),
        Method::Vertical(filter) => vertical(src, &mut dst, filter),
        Method::Down34 => down34(src, &mut dst),
        Method::Down2 => down2(src, &mut dst),
        Method::Down38 => down38(src, &mut dst),
        Method::Down4 => down4(src, &mut dst),
        Method::Box => box_filter(src, &mut dst),
        Method::Up2Linear => up2_linear(src, &mut dst),
        Method::Up2Bilinear => up2_bilinear(src, &mut dst),
        Method::BilinearUp(filter) => bilinear_up(src, &mut dst, filter),
        Method::BilinearDown(filter) => bilinear_down(src, &mut dst, filter),
        Method::Simple => simple(src, &mut dst),
    }
    Ok(dst)
}

/// `FixedDiv`: `num / div` in 16.16 fixed point. `div` is not 0.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "num and div are sizes of at most 32768 and div is at least 1, so the quotient is under 2^31"
)]
fn fixed_div(num: usize, div: usize) -> i32 {
    let (num, div) = (wide(num), wide(div).max(1));
    i32::try_from((num << 16) / div).unwrap_or(i32::MAX)
}

/// `FixedDiv1`: `(num - 1) / (div - 1)` in 16.16 fixed point, a hair under,
/// so that the last step lands short of the last sample. `div` is at least 2.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "num and div are sizes of at most 32768 and div - 1 at least 1, so the quotient is under 2^31"
)]
fn fixed_div1(num: usize, div: usize) -> i32 {
    let (num, div) = (wide(num), wide(div));
    i32::try_from(((num << 16) - 0x0001_0001) / (div - 1).max(1)).unwrap_or(i32::MAX)
}

/// A size as an `i64`: sizes are at most 32768.
fn wide(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// A 16.16 position's whole part, as an index: positions here are never
/// negative.
fn whole(p: i32) -> usize {
    usize::try_from(p >> 16).unwrap_or(0)
}

/// `CENTERSTART(d, -32768)`: the first sample of a step of `d` taken from
/// the middle of its span, less half a sample.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "d is a step of at most 2^30"
)]
fn centred(d: i32) -> i32 {
    (d >> 1) - 32768
}

/// `ScaleSlope`'s start and step, across and down, in 16.16 fixed point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slope {
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
}

/// `ScaleSlope` for a plane that is not mirrored. (Its widening of a
/// one-sample destination for a source 32768 or more across cannot arise:
/// sources are at most 16384.)
fn slope(sw: usize, sh: usize, dw: usize, dh: usize, filter: Filter) -> Slope {
    let mut s = Slope {
        x: 0,
        y: 0,
        dx: 0,
        dy: 0,
    };
    match filter {
        Filter::Box => {
            s.dx = fixed_div(sw, dw);
            s.dy = fixed_div(sh, dh);
        }
        Filter::Bilinear | Filter::Linear => {
            // Upsampling renders the last sample once, from the last pair.
            if dw <= sw {
                s.dx = fixed_div(sw, dw);
                s.x = centred(s.dx);
            } else if sw > 1 && dw > 1 {
                s.dx = fixed_div1(sw, dw);
            }
            if filter == Filter::Linear {
                s.dy = fixed_div(sh, dh);
                s.y = s.dy >> 1;
            } else if dh <= sh {
                s.dy = fixed_div(sh, dh);
                s.y = centred(s.dy);
            } else if sh > 1 && dh > 1 {
                s.dy = fixed_div1(sh, dh);
            }
        }
        Filter::Nearest => {
            s.dx = fixed_div(sw, dw);
            s.dy = fixed_div(sh, dh);
            s.x = s.dx >> 1;
            s.y = s.dy >> 1;
        }
    }
    s
}

/// The destination's rows, top to bottom.
fn rows_mut<T>(dst: &mut Plane<T>) -> core::slice::ChunksExactMut<'_, T> {
    dst.samples.chunks_exact_mut(dst.width.max(1))
}

/// `InterpolateRow_C` (`_16_C`): `top` moved towards `bottom` by `f` / 256,
/// for `f` from 0 to 255; at 0, `top` itself, and `bottom` unread.
/// (`HalfRow`, which libyuv runs at 128, computes the same.)
#[allow(
    clippy::arithmetic_side_effects,
    reason = "16-bit samples times weights of at most 256 sum under 2^25"
)]
fn interpolate_row<T: Scaled>(out: &mut [T], top: &[T], bottom: &[T], f: u32) {
    if f == 0 {
        for (o, &t) in out.iter_mut().zip(top) {
            *o = t;
        }
        return;
    }
    let f0 = 256 - f;
    for ((o, &t), &b) in out.iter_mut().zip(top).zip(bottom) {
        *o = T::store((t.into() * f0 + b.into() * f + 128) >> 8);
    }
}

/// `ScaleFilterCols_C` (`_16_C`): the row `s` resampled at `x`, `x + dx`,
/// and on (16.16), each between the samples either side of it.
///
/// libyuv reads the sample to the right even where the fraction is 0; its
/// callers keep every position short of the last sample, so it is always
/// there.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_sign_loss,
    reason = "positions are under 2^30 and stay so after the last step; the fraction is the low 16 bits"
)]
fn filter_cols<T: Scaled>(out: &mut [T], s: &[T], x: i32, dx: i32) {
    let mut x = x;
    for o in out {
        let xi = whole(x);
        let a = s.get(xi).copied().unwrap_or_default();
        let b = s.get(xi + 1).copied().unwrap_or(a);
        *o = T::blend(a.into(), b.into(), (x & 0xffff) as u32);
        x += dx;
    }
}

/// `ScaleCols_C` (`_16_C`): the row `s` sampled at `x`, `x + dx`, and on.
/// (`ScaleColsUp2_C`, which libyuv runs to double a width, picks the same
/// samples.)
#[allow(
    clippy::arithmetic_side_effects,
    reason = "positions are under 2^30 and stay so after the last step"
)]
fn cols<T: Scaled>(out: &mut [T], s: &[T], x: i32, dx: i32) {
    let mut x = x;
    for o in out {
        *o = s.get(whole(x)).copied().unwrap_or_default();
        x += dx;
    }
}

/// `ScalePlaneVertical` (`_16`): the width kept, each row from the two
/// source rows either side -- or, unfiltered, the one above.
///
/// Its lowest position is a hair above the last row, so a row that lands on
/// the last takes 255/256 of it and 1/256 of the one above.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_sign_loss,
    reason = "sizes are at most 32768, so positions stay under 2^31; the fraction is 8 bits"
)]
fn vertical<T: Scaled>(src: &Plane<T>, dst: &mut Plane<T>, filter: Filter) {
    let (sh, dh) = (src.height, dst.height);
    let (mut y, dy) = if dh <= sh {
        let dy = fixed_div(sh, dh);
        (centred(dy), dy)
    } else if sh > 1 && dh > 1 {
        (0, fixed_div1(sh, dh))
    } else {
        (0, 0)
    };
    let max_y = if sh > 1 {
        i32::try_from(sh - 1).unwrap_or(0) * 65536 - 1
    } else {
        0
    };
    for out in rows_mut(dst) {
        y = y.min(max_y);
        let yi = whole(y);
        let f = if filter == Filter::Nearest {
            0
        } else {
            ((y >> 8) & 255) as u32
        };
        interpolate_row(out, src.row(yi), src.row(yi + 1), f);
        y += dy;
    }
}

/// `ScalePlaneDown2` with `ScaleRowDown2Box_C` (`_16_C`): each sample the
/// rounded mean of a 2x2 block. The source is twice the destination on both
/// sides.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "four 16-bit samples sum under 2^18"
)]
fn down2<T: Scaled>(src: &Plane<T>, dst: &mut Plane<T>) {
    for (j, out) in rows_mut(dst).enumerate() {
        let (s, t) = (src.row(2 * j), src.row(2 * j + 1));
        for ((o, s2), t2) in out.iter_mut().zip(s.chunks_exact(2)).zip(t.chunks_exact(2)) {
            let sum: u32 = s2.iter().chain(t2).map(|&v| v.into()).sum();
            *o = T::store((sum + 2) >> 2);
        }
    }
}

/// `ScalePlaneDown4` with `ScaleRowDown4Box_C` (`_16_C`): each sample the
/// rounded mean of a 4x4 block. The source is four times the destination on
/// both sides.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "sixteen 16-bit samples sum under 2^20"
)]
fn down4<T: Scaled>(src: &Plane<T>, dst: &mut Plane<T>) {
    for (j, out) in rows_mut(dst).enumerate() {
        let block_rows = [0, 1, 2, 3].map(|k| src.row(4 * j + k));
        for (i, o) in out.iter_mut().enumerate() {
            let sum: u32 = block_rows
                .iter()
                .flat_map(|r| r.get(4 * i..4 * i + 4).unwrap_or_default())
                .map(|&v| v.into())
                .sum();
            *o = T::store((sum + 8) >> 4);
        }
    }
}

/// `ScalePlaneDown34` with its box rows: every 4 rows and columns become 3,
/// each output sample weighted 3:1 or 1:1 from the two source samples
/// nearest it, across and then down.
///
/// The sizes are 3/4 on both sides, so the destination is whole groups of
/// three rows and three columns, and libyuv's remainder code never runs.
fn down34<T: Scaled>(src: &Plane<T>, dst: &mut Plane<T>) {
    let width = dst.width;
    for (g, group) in dst
        .samples
        .chunks_exact_mut(width.saturating_mul(3).max(1))
        .enumerate()
    {
        let r = g.saturating_mul(4);
        let (o0, rest) = group.split_at_mut(width);
        let (o1, o2) = rest.split_at_mut(width);
        let row = |k: usize| src.row(r.saturating_add(k));
        // ScaleRowDown34_0_Box: rows 0 and 1, 3:1.
        box34(o0, row(0), row(1), true);
        // ScaleRowDown34_1_Box: rows 1 and 2, 1:1.
        box34(o1, row(1), row(2), false);
        // ScaleRowDown34_0_Box from row 3 with a negative stride: rows 3
        // and 2, 3:1.
        box34(o2, row(3), row(2), true);
    }
}

/// `ScaleRowDown34_0_Box_C` (`three_to_one`) and `ScaleRowDown34_1_Box_C`
/// (`_16_C`): every 4 samples of rows `s` and `t` become 3.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "16-bit samples times weights of at most 3 sum under 2^19"
)]
fn box34<T: Scaled>(out: &mut [T], s: &[T], t: &[T], three_to_one: bool) {
    let across = |p: &[T]| -> [u32; 3] {
        let at = |k: usize| p.get(k).map_or(0, |&v| v.into());
        [
            (at(0) * 3 + at(1) + 2) >> 2,
            (at(1) + at(2) + 1) >> 1,
            (at(2) + at(3) * 3 + 2) >> 2,
        ]
    };
    for ((o, s4), t4) in out
        .chunks_exact_mut(3)
        .zip(s.chunks_exact(4))
        .zip(t.chunks_exact(4))
    {
        for ((o, a), b) in o.iter_mut().zip(across(s4)).zip(across(t4)) {
            *o = T::store(if three_to_one {
                (a * 3 + b + 2) >> 2
            } else {
                (a + b + 1) >> 1
            });
        }
    }
}

/// `ScalePlaneDown38` with its box rows: every 8 rows and columns become 3,
/// in boxes of 3, 3 and 2 on each side, each sample the box's sum times
/// `65536 / count`, truncated.
///
/// The sizes are 3/8 on both sides, so the destination is whole groups of
/// three rows and three columns, and libyuv's remainder code never runs.
fn down38<T: Scaled>(src: &Plane<T>, dst: &mut Plane<T>) {
    let width = dst.width;
    for (g, group) in dst
        .samples
        .chunks_exact_mut(width.saturating_mul(3).max(1))
        .enumerate()
    {
        let r = g.saturating_mul(8);
        let row = |k: usize| src.row(r.saturating_add(k));
        let (o0, rest) = group.split_at_mut(width);
        let (o1, o2) = rest.split_at_mut(width);
        // ScaleRowDown38_3_Box twice, then ScaleRowDown38_2_Box.
        box38(o0, &[row(0), row(1), row(2)]);
        box38(o1, &[row(3), row(4), row(5)]);
        box38(o2, &[row(6), row(7)]);
    }
}

/// `ScaleRowDown38_3_Box_C` (three `rows`) and `ScaleRowDown38_2_Box_C`
/// (two), and their `_16_C`: every 8 samples become 3, from the columns 0-2,
/// 3-5 and 6-7.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "nine 16-bit samples sum under 2^20; the products wrap as the C's unsigned ones would, which they cannot for samples of 16 bits"
)]
fn box38<T: Scaled>(out: &mut [T], rows: &[&[T]]) {
    let n = u32::try_from(rows.len()).unwrap_or(1).max(1);
    // 65536 / 9, 65536 / 6 and 65536 / 4, as libyuv writes them.
    let (wide_box, narrow_box) = (65536 / (3 * n), 65536 / (2 * n));
    for (i, o3) in out.chunks_exact_mut(3).enumerate() {
        let x = 8 * i;
        let sum = |from: usize, count: usize| -> u32 {
            rows.iter()
                .flat_map(|r| r.get(x + from..x + from + count).unwrap_or_default())
                .map(|&v| v.into())
                .sum()
        };
        for (o, (from, count, scale)) in
            o3.iter_mut()
                .zip([(0, 3, wide_box), (3, 3, wide_box), (6, 2, narrow_box)])
        {
            *o = T::store(sum(from, count).wrapping_mul(scale) >> 16);
        }
    }
}

/// `ScalePlaneBox` (`_16`): both sides below half, each sample the mean of
/// the source samples in its box -- the columns and rows its 16.16 span
/// covers, at least one of each -- by multiplying the box's sum by
/// `65536 / count` and dropping 16 bits.
///
/// The sums are kept as libyuv keeps them ([`Scaled::add_to_sum`]): a column
/// of a box over more than 257 rows of bright 8-bit samples wraps.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "sizes are at most 32768, positions under 2^31 and counts at least 1; the sums and products wrap as the C's unsigned ones do"
)]
fn box_filter<T: Scaled>(src: &Plane<T>, dst: &mut Plane<T>) {
    let (sw, sh) = (src.width, src.height);
    let s = slope(sw, sh, dst.width, dst.height, Filter::Box);
    let max_y = i32::try_from(sh).unwrap_or(0) << 16;
    // Both sides shrink below half, so every box is at least 2 wide: the
    // narrower ones `dx >> 16`, the wider one more.
    let narrow = whole(s.dx).max(1);
    let mut sums = vec![0u32; sw];
    let mut y = s.y;
    for out in rows_mut(dst) {
        let iy = whole(y);
        y = (y + s.dy).min(max_y);
        let box_height = whole(y).saturating_sub(iy).max(1);
        sums.fill(0);
        for r in iy..iy + box_height {
            for (sum, &v) in sums.iter_mut().zip(src.row(r)) {
                *sum = T::add_to_sum(*sum, v);
            }
        }
        // ScaleAddCols2_C. (ScaleAddCols1_C, which libyuv runs for a whole
        // step, computes the same: every box is then the narrower width.)
        let scale =
            [narrow, narrow + 1].map(|w| u32::try_from(65536 / (w * box_height)).unwrap_or(0));
        let mut x = s.x;
        for o in out {
            let ix = whole(x);
            x += s.dx;
            let box_width = whole(x).saturating_sub(ix).max(1);
            let sum = sums
                .get(ix..ix + box_width)
                .unwrap_or_default()
                .iter()
                .fold(0u32, |a, &b| a.wrapping_add(b));
            // The box is the narrower width or the wider: index 0 or 1.
            let k = box_width
                .checked_sub(narrow)
                .and_then(|i| scale.get(i))
                .copied()
                .unwrap_or(0);
            *o = T::store(sum.wrapping_mul(k) >> 16);
        }
    }
}

/// `ScaleRowUp2_Bilinear_Any_C` (`_16_Any_C`) for one of its two output
/// rows: twice as wide (or one less), each sample 3:1 from the source row
/// it is `near` and the one `far`, and 3:1 from the two samples either
/// side; the first and last samples from the first and last columns alone.
///
/// With `near` and `far` the same row it is `ScaleRowUp2_Linear_Any_C`:
/// (12a + 4b + 8) >> 4 is (3a + b + 2) >> 2, and an edge (3a + a + 2) >> 2
/// is a.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "16-bit samples times weights summing to 16 are under 2^21"
)]
fn up2_row<T: Scaled>(out: &mut [T], near: &[T], far: &[T]) {
    let at = |r: &[T], k: usize| r.get(k).map_or(0, |&v| v.into());
    let edge = |k: usize| T::store((3 * at(near, k) + at(far, k) + 2) >> 2);
    let width = out.len();
    if let Some(first) = out.first_mut() {
        *first = edge(0);
    }
    // Pairs from (1, 2) to the last before the end: `(width - 1) & !1`
    // samples, from each source pair in turn.
    let work = width.saturating_sub(1) & !1;
    for (x, pair) in out
        .get_mut(1..1 + work)
        .unwrap_or_default()
        .chunks_exact_mut(2)
        .enumerate()
    {
        let (n0, n1, f0, f1) = (at(near, x), at(near, x + 1), at(far, x), at(far, x + 1));
        if let [a, b] = pair {
            *a = T::store((9 * n0 + 3 * n1 + 3 * f0 + f1 + 8) >> 4);
            *b = T::store((3 * n0 + 9 * n1 + f0 + 3 * f1 + 8) >> 4);
        }
    }
    if let Some(last) = out.last_mut() {
        *last = edge((width - 1) / 2);
    }
}

/// `ScalePlaneUp2_Linear` (`_12_Linear`): twice as wide (or one less), the
/// rows the nearest, chosen from the middle of the first by a step of
/// `(sh - 1) / (dh - 1)`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "sizes are at most 32768, so positions stay under 2^31"
)]
fn up2_linear<T: Scaled>(src: &Plane<T>, dst: &mut Plane<T>) {
    let (sh, dh) = (src.height, dst.height);
    if dh == 1 {
        let s = src.row((sh - 1) / 2);
        for out in rows_mut(dst) {
            up2_row(out, s, s);
        }
        return;
    }
    let dy = fixed_div(sh - 1, dh - 1);
    let mut y = (1 << 15) - 1;
    for out in rows_mut(dst) {
        let s = src.row(whole(y));
        up2_row(out, s, s);
        y += dy;
    }
}

/// `ScalePlaneUp2_Bilinear` (`_12_Bilinear`): twice the size (or one less)
/// on both sides. The first row is the first source row's alone; then each
/// pair of source rows makes two, 3:1 and 1:3; and an even height ends with
/// the last source row's alone.
#[allow(clippy::arithmetic_side_effects, reason = "sizes are at most 32768")]
fn up2_bilinear<T: Scaled>(src: &Plane<T>, dst: &mut Plane<T>) {
    let (sh, dh) = (src.height, dst.height);
    let mut rows = rows_mut(dst);
    if let Some(out) = rows.next() {
        up2_row(out, src.row(0), src.row(0));
    }
    for k in 0..sh - 1 {
        let (s, t) = (src.row(k), src.row(k + 1));
        if let Some(out) = rows.next() {
            up2_row(out, s, t);
        }
        if let Some(out) = rows.next() {
            up2_row(out, t, s);
        }
    }
    if dh % 2 == 0 {
        if let Some(out) = rows.next() {
            up2_row(out, src.row(sh - 1), src.row(sh - 1));
        }
    }
}

/// `ScalePlaneBilinearUp` (`_16`): taller. Source rows are resampled across
/// into two buffers as the position reaches them, and each row interpolated
/// between the two (or, `Linear`, the upper one copied).
///
/// Which source row is read next follows libyuv's pointer, which steps only
/// while the position is a row and more above the last.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_sign_loss,
    reason = "sizes are at most 32768, so positions stay under 2^31; the fraction is 8 bits"
)]
fn bilinear_up<T: Scaled>(src: &Plane<T>, dst: &mut Plane<T>, filter: Filter) {
    let (sw, sh, dw, dh) = (src.width, src.height, dst.width, dst.height);
    let s = slope(sw, sh, dw, dh, filter);
    let max_y = i32::try_from(sh - 1).unwrap_or(0) << 16;
    let mut y = s.y.min(max_y);
    // The source row read next.
    let mut next = whole(y);
    let mut bufs = [vec![T::default(); dw], vec![T::default(); dw]];
    let [b0, b1] = &mut bufs;
    filter_cols(b0, src.row(next), s.x, s.dx);
    if sh > 1 {
        next += 1;
    }
    filter_cols(b1, src.row(next), s.x, s.dx);
    if sh > 2 {
        next += 1;
    }
    // The buffer holding the upper row; the other holds the lower.
    let mut top = 0usize;
    let mut last_y = y >> 16;
    for out in rows_mut(dst) {
        let mut yi = y >> 16;
        if yi != last_y {
            if y > max_y {
                y = max_y;
                yi = y >> 16;
                next = whole(y);
            }
            if yi != last_y {
                // The old upper row's buffer takes the new lower row.
                if let Some(buf) = bufs.get_mut(top) {
                    filter_cols(buf, src.row(next), s.x, s.dx);
                }
                top ^= 1;
                last_y = yi;
                if y + 65536 < max_y {
                    next += 1;
                }
            }
        }
        let [b0, b1] = &bufs;
        let (upper, lower) = if top == 0 { (b0, b1) } else { (b1, b0) };
        let f = if filter == Filter::Linear {
            0
        } else {
            ((y >> 8) & 255) as u32
        };
        interpolate_row(out, upper, lower, f);
        y += s.dy;
    }
}

/// `ScalePlaneBilinearDown` (`_16`): no taller. Each row is interpolated
/// from the two source rows either side (or, `Linear`, read from the nearest)
/// and resampled across.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_sign_loss,
    reason = "sizes are at most 32768, so positions stay under 2^31; the fraction is 8 bits"
)]
fn bilinear_down<T: Scaled>(src: &Plane<T>, dst: &mut Plane<T>, filter: Filter) {
    let (sw, sh, dw, dh) = (src.width, src.height, dst.width, dst.height);
    let s = slope(sw, sh, dw, dh, filter);
    let max_y = i32::try_from(sh - 1).unwrap_or(0) << 16;
    let mut y = s.y.min(max_y);
    let mut row = vec![T::default(); sw];
    for out in rows_mut(dst) {
        let yi = whole(y);
        if filter == Filter::Linear {
            filter_cols(out, src.row(yi), s.x, s.dx);
        } else {
            interpolate_row(
                &mut row,
                src.row(yi),
                src.row(yi + 1),
                ((y >> 8) & 255) as u32,
            );
            filter_cols(out, &row, s.x, s.dx);
        }
        y = (y + s.dy).min(max_y);
    }
}

/// `ScalePlaneSimple` (`_16`): the nearest sample, from the middle of each
/// step.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "sizes are at most 32768, so positions stay under 2^31"
)]
fn simple<T: Scaled>(src: &Plane<T>, dst: &mut Plane<T>) {
    let s = slope(
        src.width,
        src.height,
        dst.width,
        dst.height,
        Filter::Nearest,
    );
    let mut y = s.y;
    for out in rows_mut(dst) {
        cols(out, src.row(whole(y)), s.x, s.dx);
        y += s.dy;
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "a test: a failure should be loud, and the reference harness's arithmetic is copied as it is"
    )]

    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    // The expected values come from libyuv 1924 (`644251f2`) built with
    // `LIBYUV_DISABLE_X86` -- its C, as MSVC's x86-64 build runs it -- and
    // `tools/libyuv_scale_reference.cc`, which fills each case's source as
    // `source` does, scales it with `ScalePlane` (8-bit) or `ScalePlane_12`
    // (10- and 12-bit) and `kFilterBox`, and hashes the result as
    // `Fnv::plane` does. Its `cases` mode prints `CASES`' lines.

    /// How a case's source is filled.
    #[derive(Clone, Copy, Debug)]
    enum Fill {
        /// xorshift32 from the case's seed, each sample the draw's top bits.
        Rand,
        /// Every sample the depth's maximum.
        Max,
        /// `(x * 3 + y * 5)`, wrapped to the depth.
        Ramp,
    }

    /// A case's seed: its sizes and depth, folded by 31.
    fn seed(depth: u32, sw: usize, sh: usize, dw: usize, dh: usize) -> u32 {
        [sh, dw, dh]
            .iter()
            .map(|&v| v as u32)
            .chain([depth])
            .fold(sw as u32, |s, v| s.wrapping_mul(31).wrapping_add(v))
            | 1
    }

    /// A case's `sw` x `sh` source of `depth`-bit samples.
    fn source<T: Scaled>(
        depth: u32,
        (sw, sh, dw, dh): (usize, usize, usize, usize),
        fill: Fill,
    ) -> Plane<T> {
        let mut x = seed(depth, sw, sh, dw, dh);
        let top = (1u32 << depth) - 1;
        let mut plane = Plane::new(sw, sh).unwrap();
        for (i, s) in plane.samples.iter_mut().enumerate() {
            let (col, row) = ((i % sw) as u32, (i / sw) as u32);
            let v = match fill {
                Fill::Max => top,
                Fill::Ramp => (col * 3 + row * 5) & top,
                Fill::Rand => {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    x >> (32 - depth)
                }
            };
            *s = T::store(v);
        }
        plane
    }

    /// FNV-1a, 64-bit.
    struct Fnv(u64);

    impl Fnv {
        fn new() -> Self {
            Self(0xcbf2_9ce4_8422_2325)
        }

        /// A plane's samples: a byte each at 8 bits, two (little-endian)
        /// deeper.
        fn plane<T: Scaled>(&mut self, plane: &Plane<T>, depth: u32) {
            let width = if depth == 8 { 1 } else { 2 };
            for &s in &plane.samples {
                let v: u32 = s.into();
                for &b in v.to_le_bytes().iter().take(width) {
                    self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
                }
            }
        }
    }

    /// Scale a case as libavif would at `depth`, and hash the result.
    fn scale_case(fnv: &mut Fnv, depth: u32, size: (usize, usize, usize, usize), fill: Fill) {
        let (_, _, dw, dh) = size;
        if depth == 8 {
            let out = scale_plane(&source::<u8>(depth, size, fill), dw, dh).unwrap();
            fnv.plane(&out, depth);
        } else {
            let out = scale_plane(&source::<u16>(depth, size, fill), dw, dh).unwrap();
            fnv.plane(&out, depth);
        }
    }

    /// Every combination of sizes from 1 to 12 on the four sides, at each
    /// depth libavif scales, hashed together in order.
    #[test]
    fn every_size_to_twelve_scales_as_libyuv_does() {
        for (depth, want) in [
            (8, 0x54dd_ed7c_680a_e94c),
            (10, 0x2aa0_e2f9_039a_350d),
            (12, 0xdd0a_8c2a_ec70_56f7),
        ] {
            let mut fnv = Fnv::new();
            for sw in 1..=12 {
                for sh in 1..=12 {
                    for dw in 1..=12 {
                        for dh in 1..=12 {
                            scale_case(&mut fnv, depth, (sw, sh, dw, dh), Fill::Rand);
                        }
                    }
                }
            }
            assert_eq!(fnv.0, want, "{depth}-bit");
        }
    }

    /// Larger planes through every method: widths past libyuv's 8-, 16- and
    /// 32-sample loops, odd sizes, the fixed ratios, and a box tall enough
    /// to wrap 8-bit sums. (depth, source size, destination size, fill,
    /// hash.)
    const CASES: [(u32, usize, usize, usize, usize, &str, u64); 57] = [
        (8, 17, 9, 17, 9, "rand", 0xbe7e_7b46_1a0d_b0b1),
        (8, 64, 48, 48, 36, "rand", 0xf9f0_da38_54df_cddf),
        (8, 128, 96, 96, 72, "ramp", 0x8b1a_5845_1804_b0a5),
        (8, 130, 66, 65, 33, "rand", 0x5ff7_863b_e501_8b8a),
        (8, 64, 32, 24, 12, "rand", 0xa897_c6ff_42c6_724d),
        (8, 128, 128, 48, 48, "ramp", 0x5378_c0c0_9ef2_50b8),
        (8, 128, 64, 32, 16, "rand", 0xd193_edad_3cf2_c3b2),
        (8, 517, 333, 100, 61, "rand", 0x9a7c_7355_6436_47a0),
        (8, 200, 300, 33, 47, "ramp", 0x1218_ef8e_b82f_ad82),
        (8, 64, 3000, 2, 5, "max", 0xa078_9f1a_6ea3_c68d),
        (8, 100, 50, 200, 50, "rand", 0x17b8_5ec6_be4b_9faf),
        (8, 99, 1, 197, 7, "rand", 0x7bc2_bd1b_24ff_e97e),
        (8, 90, 90, 180, 30, "rand", 0x34d6_2e6a_6cd2_798e),
        (8, 64, 48, 128, 96, "rand", 0xebb2_7dfa_6c11_9e6f),
        (8, 65, 47, 129, 93, "ramp", 0x1cc6_7c22_3357_e1a4),
        (8, 100, 60, 173, 95, "rand", 0x9c73_871b_7ba1_5221),
        (8, 100, 60, 61, 95, "rand", 0x465b_e007_4607_c782),
        (8, 90, 1, 150, 7, "rand", 0x785e_3bae_231a_bdcf),
        (8, 173, 95, 100, 60, "rand", 0xbfd3_8b90_393f_db98),
        (8, 61, 95, 100, 60, "ramp", 0xa0d6_6ca8_6f53_162c),
        (8, 150, 40, 77, 40, "rand", 0xeaf3_c8b6_dcf7_c117),
        (8, 50, 100, 50, 37, "rand", 0x9e8b_6f7f_2e82_98f8),
        (8, 50, 37, 50, 100, "rand", 0x8874_752c_404a_4ee5),
        (8, 50, 90, 50, 30, "rand", 0x32c8_fdab_ae85_14cb),
        (8, 1, 50, 40, 70, "rand", 0x34e7_bfb1_b5eb_60b5),
        (8, 90, 40, 30, 40, "rand", 0x8a7b_da9a_e324_4994),
        (10, 17, 9, 17, 9, "rand", 0x9f01_0027_8043_b1b1),
        (10, 64, 48, 48, 36, "rand", 0x9e80_dab5_163c_bdd8),
        (10, 130, 66, 65, 33, "rand", 0xf1f5_2d0e_0924_c975),
        (10, 128, 128, 48, 48, "ramp", 0xddaf_beb9_58ad_d561),
        (10, 128, 64, 32, 16, "rand", 0x101b_063d_c28d_eda9),
        (10, 517, 333, 100, 61, "rand", 0x2891_0392_db5c_c062),
        (10, 64, 3000, 2, 5, "max", 0x87f5_a37e_6ae2_7449),
        (10, 100, 50, 200, 50, "rand", 0x7737_1e5b_e943_da09),
        (10, 99, 1, 197, 7, "rand", 0xbd5f_e4da_6aa2_f49e),
        (10, 65, 47, 129, 93, "ramp", 0x9891_8884_7973_9ec6),
        (10, 100, 60, 173, 95, "rand", 0x459d_5a2b_d5ca_111b),
        (10, 90, 1, 150, 7, "rand", 0x3b3c_ec3f_32fa_fe4d),
        (10, 173, 95, 100, 60, "rand", 0x717b_0e23_6cf1_4ee4),
        (10, 150, 40, 77, 40, "rand", 0xef17_a68b_4146_aa12),
        (10, 50, 100, 50, 37, "rand", 0xf57a_267a_5533_26d2),
        (10, 50, 90, 50, 30, "rand", 0xbba3_8c26_5418_9ae6),
        (10, 1, 50, 40, 70, "rand", 0xd7b7_db1e_0b90_6615),
        (12, 64, 48, 48, 36, "rand", 0xd215_10e7_f95a_b070),
        (12, 128, 96, 96, 72, "ramp", 0xfde7_e38f_2dc4_18ec),
        (12, 130, 66, 65, 33, "rand", 0xce1d_3146_cefe_b2cc),
        (12, 64, 32, 24, 12, "rand", 0x8e72_d7ea_fe92_5c0c),
        (12, 128, 64, 32, 16, "rand", 0x6742_d442_8cc6_5d49),
        (12, 517, 333, 100, 61, "rand", 0x7c7f_7a52_d317_d51e),
        (12, 200, 300, 33, 47, "ramp", 0x5deb_444b_05d3_9f03),
        (12, 64, 3000, 2, 5, "max", 0x305b_610a_b35b_8609),
        (12, 90, 90, 180, 30, "rand", 0xd9d6_687c_ae74_919d),
        (12, 64, 48, 128, 96, "rand", 0xe8ea_9b81_2116_a3fd),
        (12, 100, 60, 61, 95, "rand", 0x03d5_67b7_8bdc_7dfd),
        (12, 61, 95, 100, 60, "ramp", 0x8cec_1b21_b5a3_f6f4),
        (12, 50, 37, 50, 100, "rand", 0x190e_eb33_ba4c_094a),
        (12, 90, 40, 30, 40, "rand", 0x2a4b_9325_0451_17f8),
    ];

    #[test]
    fn larger_planes_scale_as_libyuv_does() {
        for &(depth, sw, sh, dw, dh, fill, want) in &CASES {
            let fill = match fill {
                "max" => Fill::Max,
                "ramp" => Fill::Ramp,
                _ => Fill::Rand,
            };
            let mut fnv = Fnv::new();
            scale_case(&mut fnv, depth, (sw, sh, dw, dh), fill);
            assert_eq!(fnv.0, want, "{depth}-bit {sw}x{sh} to {dw}x{dh}, {fill:?}");
        }
    }

    /// libyuv's choice of method, at sizes that take each -- and the sizes
    /// to 12 reach all fifteen, so the sweep holds every one to libyuv.
    #[test]
    fn libyuv_picks_its_method_by_the_sizes() {
        use Filter::{Bilinear, Linear, Nearest};
        for ((sw, sh, dw, dh), want) in [
            ((17, 9, 17, 9), Method::Copy),
            ((50, 100, 50, 37), Method::Vertical(Bilinear)),
            // A third as tall makes bilinear linear; the width kept makes
            // linear nearest.
            ((50, 90, 50, 30), Method::Vertical(Nearest)),
            ((64, 48, 48, 36), Method::Down34),
            ((130, 66, 65, 33), Method::Down2),
            ((64, 32, 24, 12), Method::Down38),
            ((128, 64, 32, 16), Method::Down4),
            ((517, 333, 100, 61), Method::Box),
            // Half as wide and tall, or more on one side: no box.
            ((517, 333, 100, 167), Method::BilinearDown(Bilinear)),
            ((100, 50, 200, 50), Method::Up2Linear),
            ((99, 1, 197, 7), Method::Up2Linear),
            ((65, 47, 129, 93), Method::Up2Bilinear),
            ((100, 60, 173, 95), Method::BilinearUp(Bilinear)),
            ((100, 60, 61, 95), Method::BilinearUp(Bilinear)),
            ((90, 1, 150, 7), Method::BilinearUp(Linear)),
            ((173, 95, 100, 60), Method::BilinearDown(Bilinear)),
            ((61, 95, 100, 60), Method::BilinearDown(Bilinear)),
            ((150, 40, 77, 40), Method::BilinearDown(Linear)),
            ((1, 50, 40, 70), Method::Simple),
            ((90, 40, 30, 40), Method::Simple),
        ] {
            assert_eq!(method(sw, sh, dw, dh), want, "{sw}x{sh} to {dw}x{dh}");
        }
        let mut seen: Vec<Method> = Vec::new();
        for sw in 1..=12 {
            for sh in 1..=12 {
                for dw in 1..=12 {
                    for dh in 1..=12 {
                        let m = method(sw, sh, dw, dh);
                        if !seen.contains(&m) {
                            seen.push(m);
                        }
                    }
                }
            }
        }
        assert_eq!(seen.len(), 15, "{seen:?}");
    }

    /// Half the size: the rounded mean of each 2x2 block.
    #[test]
    fn half_the_size_is_each_block_s_rounded_mean() {
        let src = Plane {
            width: 4,
            height: 2,
            samples: vec![1u8, 2, 10, 10, 3, 5, 10, 11],
        };
        // (1 + 2 + 3 + 5 + 2) >> 2 = 3; (10 + 10 + 10 + 11 + 2) >> 2 = 10.
        assert_eq!(scale_plane(&src, 2, 1).unwrap().samples, [3, 10]);
    }

    /// The box filter divides by multiplying by `65536 / count`, truncated,
    /// and keeps 8-bit column sums in 16 bits: a 32 x 600 box of white sums
    /// 600 x 255 = 153000 down each column, which wraps to 21928, times 32
    /// columns, times 65536 / 19200 = 3, over 65536 -- 32. At 12 bits there
    /// is no wrap, but the truncated reciprocal still darkens: 3599 for
    /// 4095.
    #[test]
    fn a_tall_box_wraps_at_eight_bits_and_darkens_at_any() {
        let white = Plane {
            width: 64,
            height: 3000,
            samples: vec![255u8; 64 * 3000],
        };
        let out = scale_plane(&white, 2, 5).unwrap();
        assert!(out.samples.iter().all(|&s| s == 32), "{:?}", out.samples);
        let deep = Plane {
            width: 64,
            height: 3000,
            samples: vec![4095u16; 64 * 3000],
        };
        let out = scale_plane(&deep, 2, 5).unwrap();
        assert!(out.samples.iter().all(|&s| s == 3599), "{:?}", out.samples);
    }

    /// Twice the size: the first and last samples of a row from their
    /// column alone, the rest 3:1 between neighbours on both sides.
    #[test]
    fn twice_the_size_interpolates_between_neighbours() {
        let src = Plane {
            width: 2,
            height: 2,
            samples: vec![0u8, 64, 128, 192],
        };
        let out = scale_plane(&src, 4, 4).unwrap();
        // Row 0 is source row 0 alone: 0, (3*0 + 64 + 2) >> 2 = 16,
        // (0 + 3*64 + 2) >> 2 = 48, 64. Row 1 is 3:1 rows 0 and 1, so its
        // first sample is (3*0 + 128 + 2) >> 2 = 32.
        assert_eq!(&out.samples[..4], &[0, 16, 48, 64]);
        assert_eq!(out.samples[4], 32);
        assert_eq!(&out.samples[12..], &[128, 144, 176, 192]);
    }
}
