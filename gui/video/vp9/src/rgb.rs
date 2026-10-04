//! Pictures in RGB, for callers whose pixels are not YUV -- a screen's, a
//! window's. VP9 codes 4:2:0 YUV; these convert to it and back with BT.601's
//! weights at limited range ("studio swing": luma 16 to 235), in 8-bit fixed
//! point.
//!
//! To YUV is libyuv's `ARGBToI420` as its x86 code computes it: luma from
//! each pixel, chroma from each 2x2 block averaged as two rounded averages of
//! pairs (`pavgb`), down the columns and then across, a last odd column or
//! row averaged with itself. Back to RGB is the textbook inverse, each chroma
//! sample serving its 2x2 block. A picture through both comes back within a
//! few levels, except where colour changes inside a 2x2 block -- which no
//! 4:2:0 coding keeps.
//!
//! Pixels are `0xAARRGGBB` words, as SlateOS's compositor holds them. Alpha
//! is not coded: it is ignored on the way in, and opaque on the way out.
//!
//! The forward conversion is translated into Rust from libyuv's
//! `source/row_common.cc` (`RGBToY`, `RGBToU`, `RGBToV`, `ARGBToUVRow_C`'s
//! `LIBYUV_ARGBTOUV_PAVGB` form) and `source/convert.cc` (`ARGBToI420`),
//! copyright the LibYuv Project Authors, used under libyuv's BSD licence
//! (`licenses/libyuv-LICENSE`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "8-bit samples through fixed-point weights below 2^17, each result clamped or shown in range before it is narrowed"
)]

use crate::{Error, PlaneView};

/// A 4:2:0 picture of 8-bit samples: luma `width` x `height`, chroma half
/// each way, rounded up -- what [`crate::Encoder`] takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Yuv420 {
    pub width: usize,
    pub height: usize,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

impl Yuv420 {
    /// The chroma planes' width and height.
    #[must_use]
    pub fn chroma_size(&self) -> (usize, usize) {
        (self.width.div_ceil(2), self.height.div_ceil(2))
    }

    /// The planes as [`crate::Encoder::encode`] takes them.
    #[must_use]
    pub fn planes(&self) -> [PlaneView<'_, u8>; 3] {
        let (cw, ch) = self.chroma_size();
        [
            PlaneView {
                data: &self.y,
                stride: self.width,
                width: self.width,
                height: self.height,
            },
            PlaneView {
                data: &self.u,
                stride: cw,
                width: cw,
                height: ch,
            },
            PlaneView {
                data: &self.v,
                stride: cw,
                width: cw,
                height: ch,
            },
        ]
    }
}

/// libyuv's `RGBToY`: luma from 16 to 235.
#[inline]
fn rgb_to_y(r: i32, g: i32, b: i32) -> u8 {
    ((66 * r + 129 * g + 25 * b + 0x1080) >> 8) as u8
}

/// libyuv's `RGBToU`: from 16 to 240, 128 grey.
#[inline]
fn rgb_to_u(r: i32, g: i32, b: i32) -> u8 {
    ((112 * b - 74 * g - 38 * r + 0x8000) >> 8) as u8
}

/// libyuv's `RGBToV`.
#[inline]
fn rgb_to_v(r: i32, g: i32, b: i32) -> u8 {
    ((112 * r - 94 * g - 18 * b + 0x8000) >> 8) as u8
}

/// libyuv's `AVGB`: `pavgb`'s rounded average.
#[inline]
fn avg(a: i32, b: i32) -> i32 {
    (a + b + 1) >> 1
}

/// A pixel's red, green and blue.
#[inline]
fn channels(p: u32) -> [i32; 3] {
    [
        ((p >> 16) & 0xff) as i32,
        ((p >> 8) & 0xff) as i32,
        (p & 0xff) as i32,
    ]
}

/// The picture `argb` -- `width` x `height` pixels, rows `stride` pixels
/// apart, `0xAARRGGBB` -- as 4:2:0 YUV: libyuv's `ARGBToI420`.
///
/// # Errors
///
/// [`Error::Unsupported`] if a dimension is 0, the stride is less than the
/// width, or `argb` is too short for them.
pub fn argb_to_yuv420(
    argb: &[u32],
    width: usize,
    height: usize,
    stride: usize,
) -> Result<Yuv420, Error> {
    if width == 0 || height == 0 || stride < width {
        return Err(Error::Unsupported(
            "an RGB picture of no size, or a short stride",
        ));
    }
    let need = (height - 1)
        .checked_mul(stride)
        .and_then(|n| n.checked_add(width))
        .ok_or(Error::Unsupported("an RGB picture too large"))?;
    if argb.len() < need {
        return Err(Error::Unsupported(
            "an RGB picture's pixels are too few for its size",
        ));
    }
    let (cw, ch) = (width.div_ceil(2), height.div_ceil(2));
    let mut out = Yuv420 {
        width,
        height,
        y: vec![0; width * height],
        u: vec![0; cw * ch],
        v: vec![0; cw * ch],
    };
    let row = |r: usize| argb.get(r * stride..r * stride + width).unwrap_or(&[]);
    for (r, dst) in out.y.chunks_exact_mut(width).enumerate() {
        for (d, &p) in dst.iter_mut().zip(row(r)) {
            let [red, green, blue] = channels(p);
            *d = rgb_to_y(red, green, blue);
        }
    }
    for (cr, (us, vs)) in out
        .u
        .chunks_exact_mut(cw)
        .zip(out.v.chunks_exact_mut(cw))
        .enumerate()
    {
        // A last odd row is averaged with itself (libyuv passes it a stride
        // of 0).
        let (top, bottom) = (row(2 * cr), row((2 * cr + 1).min(height - 1)));
        for (cc, (u, v)) in us.iter_mut().zip(vs.iter_mut()).enumerate() {
            let x = 2 * cc;
            // A last odd column, likewise.
            let x1 = (x + 1).min(width - 1);
            let px = |s: &[u32], x: usize| channels(s.get(x).copied().unwrap_or(0));
            let (a, b, c, d) = (px(top, x), px(bottom, x), px(top, x1), px(bottom, x1));
            // Down the columns, then across: libyuv's order.
            let mean = |p: i32, q: i32, r: i32, s: i32| avg(avg(p, q), avg(r, s));
            let red = mean(a[0], b[0], c[0], d[0]);
            let green = mean(a[1], b[1], c[1], d[1]);
            let blue = mean(a[2], b[2], c[2], d[2]);
            *u = rgb_to_u(red, green, blue);
            *v = rgb_to_v(red, green, blue);
        }
    }
    Ok(out)
}

/// A 4:2:0 picture of 8-bit samples -- such as [`crate::Picture::plane8`]'s
/// planes -- as `0xFFRRGGBB` pixels, `width * height` of them, row by row:
/// BT.601's inverse at limited range, each chroma sample serving its 2x2
/// block.
///
/// # Errors
///
/// [`Error::Unsupported`] if the chroma planes are not half the luma's size,
/// rounded up, or a plane's data is too short for its stride.
pub fn yuv420_to_argb(planes: [PlaneView<'_, u8>; 3]) -> Result<Vec<u32>, Error> {
    let [y, u, v] = planes;
    let (w, h) = (y.width, y.height);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    if (u.width, u.height, v.width, v.height) != (cw, ch, cw, ch) {
        return Err(Error::Unsupported("chroma planes not half the luma's size"));
    }
    let fits = |p: &PlaneView<'_, u8>| {
        p.width == 0
            || p.height == 0
            || ((p.height - 1).checked_mul(p.stride))
                .and_then(|n| n.checked_add(p.width))
                .is_some_and(|n| n <= p.data.len())
    };
    if !(fits(&y) && fits(&u) && fits(&v)) || y.stride < w || u.stride < cw || v.stride < cw {
        return Err(Error::Unsupported(
            "a plane's data is too short for its stride",
        ));
    }
    let mut out = vec![0u32; w * h];
    for (r, dst) in out.chunks_exact_mut(w.max(1)).enumerate() {
        let ys = y.data.get(r * y.stride..).unwrap_or(&[]);
        let us = u.data.get((r / 2) * u.stride..).unwrap_or(&[]);
        let vs = v.data.get((r / 2) * v.stride..).unwrap_or(&[]);
        for (x, d) in dst.iter_mut().enumerate() {
            let c = i32::from(ys.get(x).copied().unwrap_or(16)) - 16;
            let dd = i32::from(us.get(x / 2).copied().unwrap_or(128)) - 128;
            let e = i32::from(vs.get(x / 2).copied().unwrap_or(128)) - 128;
            let clip = |n: i32| ((n + 128) >> 8).clamp(0, 255) as u32;
            let red = clip(298 * c + 409 * e);
            let green = clip(298 * c - 100 * dd - 208 * e);
            let blue = clip(298 * c + 516 * dd);
            *d = 0xff00_0000 | (red << 16) | (green << 8) | blue;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a test: a failure should be loud"
    )]

    use super::*;

    /// libyuv's own numbers for its primaries: black, white, the three
    /// colours and grey.
    #[test]
    fn the_weights_are_libyuvs() {
        let one = |p: u32| {
            let yuv = argb_to_yuv420(&[p], 1, 1, 1).unwrap();
            (yuv.y[0], yuv.u[0], yuv.v[0])
        };
        assert_eq!(one(0xff00_0000), (16, 128, 128));
        assert_eq!(one(0xffff_ffff), (235, 128, 128));
        assert_eq!(one(0xffff_0000), (82, 90, 239));
        assert_eq!(one(0xff00_ff00), (144, 54, 34));
        assert_eq!(one(0xff00_00ff), (41, 239, 110));
        assert_eq!(one(0xff80_8080), (126, 128, 128));
    }

    /// Chroma is the 2x2 block's pairs averaged down, then across, each
    /// average rounded up at the half -- not the four's exact mean.
    #[test]
    fn chroma_averages_pairs_down_then_across() {
        // Blue only, so U is 112 * b / 256 + 128 and every rounding shows:
        // columns (1, 2) and (2, 2) average to 2 and 2, then 2 -- where the
        // exact mean of 1, 2, 2, 2 is 1.75.
        let p = |b: u32| 0xff00_0000 | b;
        let yuv = argb_to_yuv420(&[p(1), p(2), p(2), p(2)], 2, 2, 2).unwrap();
        assert_eq!(yuv.u[0], rgb_to_u(0, 0, 2));
        // An odd size: the last column and row average with themselves.
        let pic: Vec<u32> = (0..9).map(|i| 0xff00_0000 | (i * 30)).collect();
        let yuv = argb_to_yuv420(&pic, 3, 3, 3).unwrap();
        assert_eq!(yuv.chroma_size(), (2, 2));
        assert_eq!(yuv.u[3], rgb_to_u(0, 0, 240), "the corner is its own block");
        assert_eq!(
            yuv.u[1],
            rgb_to_u(0, 0, avg(60, 150)),
            "the last column's pairs"
        );
    }

    /// Through both, a picture whose colour is constant on each 2x2 block
    /// comes back within three levels on every channel -- each way rounds,
    /// the forward way down -- and black, white and grey within one.
    #[test]
    fn a_round_trip_comes_back_close() {
        let (w, h) = (34usize, 18usize);
        let mut pic = vec![0u32; w * h];
        for r in 0..h {
            for c in 0..w {
                let (br, bc) = ((r / 2) as u32, (c / 2) as u32);
                let red = (bc * 15) & 0xff;
                let green = (br * 29 + bc * 7) & 0xff;
                let blue = (br * 13 + 200 - bc) & 0xff;
                pic[r * w + c] = 0xff00_0000 | (red << 16) | (green << 8) | blue;
            }
        }
        let yuv = argb_to_yuv420(&pic, w, h, w).unwrap();
        let back = yuv420_to_argb(yuv.planes()).unwrap();
        for (i, (&a, &b)) in pic.iter().zip(&back).enumerate() {
            for shift in [0, 8, 16] {
                let (x, y) = ((a >> shift) & 0xff, (b >> shift) & 0xff);
                // Colours a long way out of the limited range's gamut lose
                // more; these stay inside it.
                assert!(x.abs_diff(y) <= 3, "pixel {i}: {a:08x} came back {b:08x}");
            }
            assert_eq!(b >> 24, 0xff, "opaque");
        }
        for grey in [0u32, 0x80, 0xff] {
            let p = 0xff00_0000 | (grey << 16) | (grey << 8) | grey;
            let back = yuv420_to_argb(argb_to_yuv420(&[p; 4], 2, 2, 2).unwrap().planes()).unwrap();
            for &b in &back {
                assert!((b & 0xff).abs_diff(grey) <= 1, "{grey} came back {b:08x}");
            }
        }
    }

    #[test]
    fn bad_sizes_are_refused() {
        assert!(argb_to_yuv420(&[0; 4], 0, 2, 2).is_err());
        assert!(argb_to_yuv420(&[0; 4], 2, 2, 1).is_err());
        assert!(argb_to_yuv420(&[0; 3], 2, 2, 2).is_err());
        let yuv = argb_to_yuv420(&[0; 6], 3, 2, 3).unwrap();
        let mut planes = yuv.planes();
        planes[1].width = 1;
        assert!(yuv420_to_argb(planes).is_err());
    }
}
