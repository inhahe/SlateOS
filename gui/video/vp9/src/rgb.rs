//! Pictures in RGB, for callers whose pixels are not YUV -- a screen's, a
//! window's -- as the 4:2:0 YUV VP9 codes: BT.601's weights at limited range
//! ("studio swing": luma 16 to 235), in 8-bit fixed point, libyuv's
//! `ARGBToI420` (version 1924, revision `644251f2`, the one libavif 1.4.2
//! pins): luma from each pixel, chroma from the rounded mean of each 2x2
//! block, a last odd column or row taken twice. (Earlier libyuv averaged a
//! block on x86 as two rounded averages of pairs, `pavgb`, which rounds up
//! more often; 1924 does the exact mean everywhere.)
//!
//! An encoder coding these pictures should say so: [`COLOR_SPACE`], in
//! [`crate::EncoderConfig::color_space`]. The way back to RGB is not here:
//! a decoded picture becomes pixels through `gui/video/yuv` (libavif's
//! conversion, which every picture and frame SlateOS shows goes through) or
//! `gui/video/codec`, which reads the colour space from the stream.
//!
//! Pixels are `0xAARRGGBB` words, as SlateOS's compositor holds them. Alpha
//! is not coded: it is ignored.
//!
//! Translated into Rust from libyuv's `source/row_common.cc` (`RGBToY`,
//! `RGBToU`, `RGBToV`, `ARGBToUVRow_C`) and `source/convert.cc`
//! (`ARGBToI420`), copyright the LibYuv Project Authors, used under libyuv's
//! BSD licence (`licenses/libyuv-LICENSE`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "8-bit samples through fixed-point weights below 2^17, each result clamped or shown in range before it is narrowed"
)]

use crate::{Error, PlaneView};

/// The colour space of the pictures [`argb_to_yuv420`] makes, as an
/// encoder's key frames declare it ([`crate::EncoderConfig::color_space`]):
/// libvpx's `VPX_CS_BT_601`.
pub const COLOR_SPACE: u8 = 1;

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

/// The rounded mean of a 2x2 block's samples, as `ARGBToUVRow_C` takes it:
/// for a last odd column or row, whose block holds each sample twice, the
/// rounded mean of two.
#[inline]
fn mean4(a: i32, b: i32, c: i32, d: i32) -> i32 {
    (a + b + c + d + 2) >> 2
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
        // A last odd row is its own pair (libyuv passes it a stride of 0):
        // the mean of four, each sample twice, is the rounded mean of two,
        // which is what libyuv computes for it.
        let (top, bottom) = (row(2 * cr), row((2 * cr + 1).min(height - 1)));
        for (cc, (u, v)) in us.iter_mut().zip(vs.iter_mut()).enumerate() {
            let x = 2 * cc;
            // A last odd column, likewise.
            let x1 = (x + 1).min(width - 1);
            let px = |s: &[u32], x: usize| channels(s.get(x).copied().unwrap_or(0));
            let (a, b, c, d) = (px(top, x), px(top, x1), px(bottom, x), px(bottom, x1));
            let red = mean4(a[0], b[0], c[0], d[0]);
            let green = mean4(a[1], b[1], c[1], d[1]);
            let blue = mean4(a[2], b[2], c[2], d[2]);
            *u = rgb_to_u(red, green, blue);
            *v = rgb_to_v(red, green, blue);
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

    /// Chroma is the rounded mean of the 2x2 block -- not, as libyuv's x86
    /// code computed it before version 1924, the pairs' rounded averages
    /// averaged.
    #[test]
    fn chroma_is_the_rounded_mean_of_its_block() {
        // Blue only, so U is (112 * b + 0x8000) >> 8 and roundings show: the
        // block 1 2 / 2 4 has the mean 2.25, so 2; pairs down the columns
        // (2, 3) and then across would give 3, and U 129 for 128.
        let p = |b: u32| 0xff00_0000 | b;
        let yuv = argb_to_yuv420(&[p(1), p(2), p(2), p(4)], 2, 2, 2).unwrap();
        assert_eq!(yuv.u[0], rgb_to_u(0, 0, 2));
        assert_ne!(yuv.u[0], rgb_to_u(0, 0, 3), "the pairs' way");
        // An odd size: the last column and row are their own pairs.
        let pic: Vec<u32> = (0..9).map(|i| 0xff00_0000 | (i * 30)).collect();
        let yuv = argb_to_yuv420(&pic, 3, 3, 3).unwrap();
        assert_eq!(yuv.chroma_size(), (2, 2));
        assert_eq!(yuv.u[3], rgb_to_u(0, 0, 240), "the corner is its own block");
        // The last column's two rows, 60 and 150: their rounded mean, 105.
        assert_eq!(yuv.u[1], rgb_to_u(0, 0, 105), "the last column");
        // The last row's two columns, 180 and 210: 195.
        assert_eq!(yuv.u[2], rgb_to_u(0, 0, 195), "the last row");
    }

    /// Through `argb_to_yuv420` and back to RGB as a viewer converts it --
    /// `yuv::reformat`, libavif's conversion, at the colour space
    /// [`COLOR_SPACE`] declares (BT.601, limited range) -- a picture comes
    /// back within a few levels wherever its colour is the same all around,
    /// and black, white and grey within one. Red and green come back within
    /// two (each way rounds); blue within four, because libyuv weighs blue
    /// by 128/64 where BT.601 says 129/64 (it caps the weight unless built
    /// with `LIBYUV_UNLIMITED_DATA`, as neither libavif's copy nor Chrome's
    /// is), so a strong blue comes back a level or two dark. The picture is
    /// blocks of 8x8 of one colour, and only the pixels at least two from a
    /// block's edge are compared: the conversion back upsamples chroma
    /// bilinearly, which near an edge mixes in the next block's colour.
    #[test]
    fn a_round_trip_comes_back_close() {
        let (w, h) = (48usize, 32usize);
        let colour = |r: usize, c: usize| {
            let (br, bc) = ((r / 8) as u32, (c / 8) as u32);
            let red = (bc * 41) & 0xff;
            let green = (br * 53 + bc * 7) & 0xff;
            let blue = (br * 29 + 200 - bc * 3) & 0xff;
            0xff00_0000 | (red << 16) | (green << 8) | blue
        };
        let pic: Vec<u32> = (0..h)
            .flat_map(|r| (0..w).map(move |c| colour(r, c)))
            .collect();
        let back = to_rgb(&argb_to_yuv420(&pic, w, h, w).unwrap());
        // The worst difference on each channel: red, green, blue.
        let mut worst = [0u32; 3];
        for r in 0..h {
            for c in 0..w {
                let interior = (2..6).contains(&(r % 8)) && (2..6).contains(&(c % 8));
                let (a, b) = (pic[r * w + c], back[r * w + c]);
                assert_eq!(b >> 24, 0xff, "opaque");
                if !interior {
                    continue;
                }
                for (worst, shift) in worst.iter_mut().zip([16, 8, 0]) {
                    let (x, y) = ((a >> shift) & 0xff, (b >> shift) & 0xff);
                    *worst = (*worst).max(x.abs_diff(y));
                }
            }
        }
        assert!(
            worst[0] <= 2 && worst[1] <= 2 && worst[2] <= 4,
            "the worst red, green and blue: {worst:?}"
        );
        for grey in [0u32, 0x80, 0xff] {
            let p = 0xff00_0000 | (grey << 16) | (grey << 8) | grey;
            for &b in &to_rgb(&argb_to_yuv420(&[p; 4], 2, 2, 2).unwrap()) {
                assert!((b & 0xff).abs_diff(grey) <= 1, "{grey} came back {b:08x}");
            }
        }
    }

    /// `yuv` back to pixels as a viewer converts it: libavif's conversion
    /// at BT.601, limited range -- [`COLOR_SPACE`] is libvpx's BT.601, which
    /// a decoder reports as H.273's BT.470BG (5).
    fn to_rgb(yuv: &Yuv420) -> Vec<u32> {
        let [y, u, v] = yuv.planes().map(|p| ::yuv::Plane {
            samples: p.data,
            stride: p.stride,
            width: p.width,
            height: p.height,
        });
        ::yuv::reformat::to_argb(&::yuv::reformat::Picture {
            width: yuv.width,
            height: yuv.height,
            depth: 8,
            format: ::yuv::reformat::Format::Yuv420,
            matrix: 5,
            primaries: 1,
            full_range: false,
            y,
            u: Some(u),
            v: Some(v),
            alpha: None,
            alpha_premultiplied: false,
        })
        .unwrap()
    }

    #[test]
    fn bad_sizes_are_refused() {
        assert!(argb_to_yuv420(&[0; 4], 0, 2, 2).is_err());
        assert!(argb_to_yuv420(&[0; 4], 2, 2, 1).is_err());
        assert!(argb_to_yuv420(&[0; 3], 2, 2, 2).is_err());
        assert!(argb_to_yuv420(&[0; 6], 3, 2, 3).is_ok());
    }
}
