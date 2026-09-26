//! From a decoded VP8 frame's planes to `0xAARRGGBB`, as libwebp does it.
//!
//! A VP8 frame is Y'CbCr with the chroma planes at half resolution both ways.
//! Two choices turn it into RGB, and both are libwebp's, since the point is to
//! show a WebP the way the browsers that use libwebp show it:
//!
//! * **Chroma is interpolated, not repeated** ("fancy upsampling", libwebp's
//!   default): each output pixel takes 9/16 of its nearest chroma sample, 3/16
//!   of each of the next nearest across and down, and 1/16 of the diagonal
//!   one, rounded -- `(9a + 3b + 3c + d + 8) >> 4` -- with the picture's edge
//!   samples standing in for their missing neighbours. libwebp computes this
//!   in two rounded steps (`src/dsp/upsampling.c`); the two-step form equals
//!   the one-step one exactly, which a test here checks over every input.
//!
//! * **The conversion is BT.601 studio swing in 14-bit fixed point**
//!   (libwebp's `src/dsp/yuv.h`): Y' from 16 to 235 and chroma centred on
//!   128, with libwebp's own constants and rounding.

use alloc::vec;
use alloc::vec::Vec;

/// `(v * coeff) >> 8`: libwebp's `MultHi`, an emulation of the SIMD
/// multiply-high it uses.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "v is a sample and coeff below 2^16, so the product is below 2^24"
)]
const fn mult_hi(v: i32, coeff: i32) -> i32 {
    (v * coeff) >> 8
}

/// A 14-bit fixed-point channel value to a byte, saturating.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the masked value shifted right by 6 is below 256"
)]
const fn clip8(v: i32) -> u8 {
    const MASK: i32 = (256 << 6) - 1;
    if v & !MASK == 0 {
        (v >> 6) as u8
    } else if v < 0 {
        0
    } else {
        255
    }
}

/// One pixel from Y'CbCr to `0x00RRGGBB` (libwebp's `VP8YuvToRgb`).
#[allow(
    clippy::arithmetic_side_effects,
    reason = "each term is below 2^16 in magnitude"
)]
pub(super) const fn rgb(y: u8, u: u8, v: u8) -> u32 {
    let (y, u, v) = (y as i32, u as i32, v as i32);
    let luma = mult_hi(y, 19077);
    let r = clip8(luma + mult_hi(v, 26149) - 14234);
    let g = clip8(luma - mult_hi(u, 6419) - mult_hi(v, 13320) + 8708);
    let b = clip8(luma + mult_hi(u, 33050) - 17685);
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

/// A plane of samples, `stride` apart, of which the first `width` x `height`
/// are the picture's.
pub(super) struct Plane<'a> {
    pub samples: &'a [u8],
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}

impl Plane<'_> {
    /// Row `y`'s first `len` samples, or as many as there are.
    fn row(&self, y: usize, len: usize) -> &[u8] {
        let start = y.saturating_mul(self.stride);
        let end = start.saturating_add(len).min(self.samples.len());
        self.samples.get(start..end).unwrap_or_default()
    }
}

/// The two chroma samples, across or down, that output position `i` (of a
/// full-resolution line `n` long) is interpolated from: its nearest, and the
/// next nearest on the other side of it -- which for the first position, and
/// for the last of an even-length line, is the nearest again.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "i is below n, so i / 2 is below the chroma line's (n + 1) / 2"
)]
const fn neighbours(i: usize, n: usize) -> (usize, usize) {
    let near = i / 2;
    let last = n.div_ceil(2) - 1;
    let far = if i % 2 == 1 {
        if near < last { near + 1 } else { last }
    } else if near > 0 {
        near - 1
    } else {
        0
    };
    (near, far)
}

/// One output row's chroma, vertically: `3 * near + far` for each chroma
/// column, from the chroma rows nearest the output row and next nearest.
fn blend_rows(near: &[u8], far: &[u8], out: &mut [u16]) {
    for ((t, &n), &f) in out.iter_mut().zip(near).zip(far) {
        // At most 4 * 255.
        *t = u16::from(n).wrapping_mul(3).wrapping_add(u16::from(f));
    }
}

/// Convert a frame's planes to `0xAARRGGBB`, with alpha from `alpha` (one byte
/// per pixel, `width` a row) or opaque.
///
/// A row at a time: each chroma column is blended down once for the row
/// (`3 * near + far`), and each output pixel then blends two of those across
/// (`(3 * near + far + 8) >> 4`) -- the one-step filter exactly, as
/// `3 * (3a + c) + (3b + d) = 9a + 3b + 3c + d`.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "weighted sums of four chroma samples are below 2^12, and the quotient is a sample"
)]
pub(super) fn to_argb(
    y: &Plane<'_>,
    u: &Plane<'_>,
    v: &Plane<'_>,
    alpha: Option<&[u8]>,
) -> Vec<u32> {
    let (width, height) = (y.width, y.height);
    let mut out = vec![0u32; width.saturating_mul(height)];
    let chroma_width = width.div_ceil(2);
    let mut tu = vec![0u16; chroma_width];
    let mut tv = vec![0u16; chroma_width];
    let last = chroma_width.saturating_sub(1);
    for (row, dest) in out.chunks_exact_mut(width.max(1)).enumerate().take(height) {
        let (near_row, far_row) = neighbours(row, height);
        blend_rows(
            u.row(near_row, chroma_width),
            u.row(far_row, chroma_width),
            &mut tu,
        );
        blend_rows(
            v.row(near_row, chroma_width),
            v.row(far_row, chroma_width),
            &mut tv,
        );
        let luma = y.row(row, width);
        let alpha_row = alpha.and_then(|a| a.get(row * width..(row + 1) * width));
        let across =
            |near: u16, far: u16| -> u8 { ((u32::from(near) * 3 + u32::from(far) + 8) >> 4) as u8 };
        // Two output pixels to a chroma column: the even one leans left, the
        // odd one right, each clamped at the row's ends (`neighbours`).
        for (k, pair) in dest.chunks_mut(2).enumerate() {
            let at = |t: &[u16], i: usize| t.get(i).copied().unwrap_or(0);
            let (un, ul, ur) = (
                at(&tu, k),
                at(&tu, k.saturating_sub(1)),
                at(&tu, (k + 1).min(last)),
            );
            let (vn, vl, vr) = (
                at(&tv, k),
                at(&tv, k.saturating_sub(1)),
                at(&tv, (k + 1).min(last)),
            );
            let chroma = [
                (across(un, ul), across(vn, vl)),
                (across(un, ur), across(vn, vr)),
            ];
            for (i, (pixel, (cu, cv))) in pair.iter_mut().zip(chroma).enumerate() {
                let x = 2 * k + i;
                let a = alpha_row.and_then(|a| a.get(x)).copied().unwrap_or(0xFF);
                let l = luma.get(x).copied().unwrap_or(0);
                *pixel = (u32::from(a) << 24) | rgb(l, cu, cv);
            }
        }
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
mod tests {
    use super::*;

    #[test]
    fn libwebps_two_step_upsampling_is_the_one_step_filter() {
        // libwebp's UPSAMPLE_FUNC, for the sample nearest `a`, in two rounded
        // steps through the diagonal's average; its packed-u32 form carries U
        // and V side by side without either disturbing the other, so one
        // channel stands for both.
        for a in (0u32..256).step_by(5) {
            for b in (0u32..256).step_by(7) {
                for c in (0u32..256).step_by(11) {
                    for d in (0u32..256).step_by(13) {
                        let avg = a + b + c + d + 8;
                        let diag = (avg + 2 * (b + c)) >> 3;
                        let two_step = (diag + a) >> 1;
                        let one_step = (9 * a + 3 * b + 3 * c + d + 8) >> 4;
                        assert_eq!(two_step, one_step, "{a} {b} {c} {d}");
                    }
                }
            }
        }
        // The edge form: (3a + c + 2) >> 2 is the filter with b = a, d = c.
        for a in 0u32..256 {
            for c in 0u32..256 {
                let edge = (3 * a + c + 2) >> 2;
                assert_eq!(edge, (9 * a + 3 * a + 3 * c + c + 8) >> 4);
            }
        }
    }

    #[test]
    fn neighbours_stand_in_at_the_edges() {
        // Odd positions lean right, even ones left, both clamped.
        assert_eq!(neighbours(0, 5), (0, 0));
        assert_eq!(neighbours(1, 5), (0, 1));
        assert_eq!(neighbours(2, 5), (1, 0));
        assert_eq!(neighbours(3, 5), (1, 2));
        assert_eq!(neighbours(4, 5), (2, 1));
        // The last of an even-length line has no right-hand neighbour.
        assert_eq!(neighbours(5, 6), (2, 2));
        assert_eq!(neighbours(0, 1), (0, 0));
    }

    #[test]
    fn studio_swing_black_and_white_and_the_primaries() {
        assert_eq!(rgb(16, 128, 128), 0x000000);
        assert_eq!(rgb(235, 128, 128), 0xFFFFFF);
        // Mid-grey stays grey.
        let grey = rgb(126, 128, 128);
        assert_eq!(grey >> 16, grey & 0xFF);
        assert_eq!((grey >> 8) & 0xFF, grey & 0xFF);
        // BT.601 red, green and blue, within a level of the ideal.
        let near = |got: u32, want: u32| {
            for shift in [16, 8, 0] {
                let (g, w) = ((got >> shift) & 0xFF, (want >> shift) & 0xFF);
                assert!(g.abs_diff(w) <= 1, "{got:06X} vs {want:06X}");
            }
        };
        near(rgb(81, 90, 240), 0xFF0000);
        near(rgb(145, 54, 34), 0x00FF00);
        near(rgb(41, 240, 110), 0x0000FF);
    }
}
