//! Colour conversion: libjpeg-turbo's `jdcolor.c`.
//!
//! JPEG stores colour as luma and two colour differences (YCbCr), almost
//! always; libjpeg converts to RGB with fixed-point tables built from the
//! JFIF coefficients, rounding once per channel. Here the same integers are
//! computed instead of looked up ([`ycc_to_rgb`]) -- a table lookup per
//! channel per pixel is the one thing a vector unit cannot do -- and a test
//! holds them to libjpeg's tables, built as libjpeg builds them, on every
//! one of the 2^24 inputs. The other conversions
//! a decoder is asked for are copies (greyscale to RGB, RGB to RGB, CMYK to
//! CMYK, and "unknown" to itself, which is what libtiff asks for when it wants
//! the samples as stored) or YCCK to CMYK, which is the YCbCr conversion
//! inverted.

#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
mod sse2;

/// A colour space, as libjpeg names them (`J_COLOR_SPACE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColorSpace {
    Unknown,
    Grayscale,
    Rgb,
    YCbCr,
    Cmyk,
    Ycck,
}

impl ColorSpace {
    /// `out_color_components` for an output space: `None` for `Unknown`,
    /// whose count is the frame's.
    pub(super) const fn components(self) -> Option<usize> {
        match self {
            Self::Grayscale => Some(1),
            Self::Rgb | Self::YCbCr => Some(3),
            Self::Cmyk | Self::Ycck => Some(4),
            Self::Unknown => None,
        }
    }
}

/// `SCALEBITS`, and `ONE_HALF`.
const SCALE: u32 = 16;
const HALF: i64 = 1 << (SCALE - 1);

/// `FIX(1.40200)`, `FIX(1.77200)`, `FIX(0.71414)` and `FIX(0.34414)`:
/// `(x * 65536 + 0.5)` truncated.
const FIX_1_40200: i64 = 91881;
const FIX_1_77200: i64 = 116_130;
const FIX_0_71414: i64 = 46802;
const FIX_0_34414: i64 = 22554;

/// `build_ycc_rgb_table`: libjpeg's tables, which [`ycc_to_rgb`] is held
/// to.
#[cfg(test)]
#[derive(Clone)]
pub(super) struct Ycc {
    cr_r: [i32; 256],
    cb_b: [i32; 256],
    cr_g: [i64; 256],
    cb_g: [i64; 256],
}

#[cfg(test)]
impl Ycc {
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "a sample is 0-255 and the tables hold at most 1.772 * 128 either way, so every sum is within +-512; the table index is a loop counter under 256"
    )]
    pub(super) fn new() -> Self {
        let mut tables = Self {
            cr_r: [0; 256],
            cb_b: [0; 256],
            cr_g: [0; 256],
            cb_g: [0; 256],
        };
        for (i, x) in (-128i64..128).enumerate() {
            // `RIGHT_SHIFT` of a signed value: arithmetic.
            tables.cr_r[i] = ((FIX_1_40200 * x + HALF) >> SCALE) as i32;
            tables.cb_b[i] = ((FIX_1_77200 * x + HALF) >> SCALE) as i32;
            tables.cr_g[i] = -FIX_0_71414 * x;
            tables.cb_g[i] = -FIX_0_34414 * x + HALF;
        }
        tables
    }

    /// One pixel's red, green and blue before the range limit.
    #[inline]
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "a sample is 0-255 and the tables hold at most 1.772 * 128 either way, so every sum is within +-512; the table index is a loop counter under 256"
    )]
    fn rgb(&self, y: u8, cb: u8, cr: u8) -> [i32; 3] {
        let y = i32::from(y);
        let (cb, cr) = (usize::from(cb), usize::from(cr));
        let cr_r = self.cr_r.get(cr).copied().unwrap_or(0);
        let cb_b = self.cb_b.get(cb).copied().unwrap_or(0);
        let g = (self.cb_g.get(cb).copied().unwrap_or(0) + self.cr_g.get(cr).copied().unwrap_or(0))
            >> SCALE;
        [y + cr_r, y + g as i32, y + cb_b]
    }
}

/// libjpeg's `sample_range_limit`: clamp to a byte.
#[inline]
const fn clamp(x: i32) -> u8 {
    if x < 0 {
        0
    } else if x > 255 {
        255
    } else {
        x as u8
    }
}

/// One pixel's red, green and blue before the range limit: what libjpeg's
/// tables give, computed.
///
/// Each table entry is `(FIX(c) * x + ONE_HALF) >> 16`. A constant over
/// 2^16 is split into its whole part and the rest -- `FIX(1.402)` is
/// `65536 + 26345`, `FIX(0.71414)` is `65536 - 18734`, `FIX(1.772)` is
/// `2 * 65536 - 14942` -- and the whole part taken out of the shift, which
/// changes nothing (a multiple of 2^16 passes through `>> 16` whole) but
/// leaves every multiplication a 16-bit one. That, and there being no
/// lookup, is what lets a row of pixels be converted side by side.
#[inline(always)]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "a sample is 0-255, so every product is under 2^22 and every sum within +-2^23"
)]
pub(super) const fn ycc_to_rgb(y: u8, cb: u8, cr: u8) -> [i32; 3] {
    let y = y as i32;
    let xb = cb as i32 - 128;
    let xr = cr as i32 - 128;
    let half = HALF as i32;
    let r = y + xr + ((R_CR * xr + half) >> SCALE);
    let g = y - xr + ((G_CB * xb + G_CR * xr + half) >> SCALE);
    let b = y + 2 * xb + ((B_CB * xb + half) >> SCALE);
    [r, g, b]
}

/// The constants [`ycc_to_rgb`] multiplies by: libjpeg's, each less its
/// whole multiple of 2^16 (see there). All under 2^15 in magnitude.
const R_CR: i32 = (FIX_1_40200 - (1 << SCALE)) as i32;
const G_CB: i32 = -FIX_0_34414 as i32;
const G_CR: i32 = ((1 << SCALE) - FIX_0_71414) as i32;
const B_CB: i32 = (FIX_1_77200 - (2 << SCALE)) as i32;
const _: () = assert!(R_CR == 26345 && G_CB == -22554 && G_CR == 18734 && B_CB == -14942);

/// `ycc_rgb_convert`, one row.
pub(super) fn ycc_rgb(y: &[u8], cb: &[u8], cr: &[u8], out: &mut [u8]) {
    for (((pixel, &y), &cb), &cr) in out.chunks_exact_mut(3).zip(y).zip(cb).zip(cr) {
        let [r, g, b] = ycc_to_rgb(y, cb, cr);
        pixel.copy_from_slice(&[clamp(r), clamp(g), clamp(b)]);
    }
}

/// `ycc_rgb_convert`, one row, straight to opaque `0xAARRGGBB` pixels: what
/// [`ycc_rgb`] and then packing its bytes give, without the bytes. Eight
/// pixels at a time in SSE2 where the build has it, the rest one at a time.
pub(super) fn ycc_argb(y: &[u8], cb: &[u8], cr: &[u8], out: &mut [u32]) {
    #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
    // SAFETY: `sse2::ycc_argb` requires SSE2 and nothing else, and this is
    // compiled only where `target_feature = "sse2"` is on for the whole
    // build -- every x86-64 target, SSE2 being part of its baseline.
    let done = unsafe { sse2::ycc_argb(y, cb, cr, out) };
    #[cfg(not(all(target_arch = "x86_64", target_feature = "sse2")))]
    let done = 0;
    let (y, cb, cr) = (
        y.get(done..).unwrap_or_default(),
        cb.get(done..).unwrap_or_default(),
        cr.get(done..).unwrap_or_default(),
    );
    let out = out.get_mut(done..).unwrap_or_default();
    for (((pixel, &y), &cb), &cr) in out.iter_mut().zip(y).zip(cb).zip(cr) {
        let [r, g, b] = ycc_to_rgb(y, cb, cr);
        *pixel = u32::from_be_bytes([0xFF, clamp(r), clamp(g), clamp(b)]);
    }
}

/// `ycck_cmyk_convert`, one row: the YCbCr conversion, inverted, and K as it
/// was.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "a sample is 0-255 and a converted channel within +-512, so every difference is small"
)]
pub(super) fn ycck_cmyk(y: &[u8], cb: &[u8], cr: &[u8], k: &[u8], out: &mut [u8]) {
    for ((((pixel, &y), &cb), &cr), &k) in out.chunks_exact_mut(4).zip(y).zip(cb).zip(cr).zip(k) {
        let [r, g, b] = ycc_to_rgb(y, cb, cr);
        pixel.copy_from_slice(&[clamp(255 - r), clamp(255 - g), clamp(255 - b), k]);
    }
}

/// Opaque grey `0xAARRGGBB` pixels from one row of samples.
pub(super) fn gray_argb(y: &[u8], out: &mut [u32]) {
    for (pixel, &y) in out.iter_mut().zip(y) {
        *pixel = 0xFF00_0000 | u32::from(y).wrapping_mul(0x0001_0101);
    }
}

/// Opaque `0xAARRGGBB` pixels from three rows, one a channel.
pub(super) fn rgb_argb(r: &[u8], g: &[u8], b: &[u8], out: &mut [u32]) {
    for (((pixel, &r), &g), &b) in out.iter_mut().zip(r).zip(g).zip(b) {
        *pixel = u32::from_be_bytes([0xFF, r, g, b]);
    }
}

/// Chrome's CMYK to RGB (`SetPixel<JCS_CMYK>`), one row of interleaved CMYK:
/// the samples are the inverted CMYK Adobe applications write, so each
/// channel is `sample * k / 255`, truncated.
pub(super) fn cmyk_argb(cmyk: &[u8], out: &mut [u32]) {
    for (pixel, p) in out.iter_mut().zip(cmyk.as_chunks::<4>().0) {
        let [c, m, y, k] = *p;
        let k = u32::from(k);
        // A byte times a byte, divided by a constant: no overflow, no zero.
        let channel = |v: u8| (u32::from(v).wrapping_mul(k) / 255) as u8;
        *pixel = u32::from_be_bytes([0xFF, channel(c), channel(m), channel(y)]);
    }
}

/// `gray_rgb_convert`, one row.
pub(super) fn gray_rgb(y: &[u8], out: &mut [u8]) {
    for (pixel, &y) in out.chunks_exact_mut(3).zip(y) {
        pixel.copy_from_slice(&[y, y, y]);
    }
}

/// `null_convert` and its relatives: the components interleaved as they are.
pub(super) fn interleave(components: &[&[u8]], out: &mut [u8]) {
    let n = components.len().max(1);
    for (i, pixel) in out.chunks_exact_mut(n).enumerate() {
        for (slot, component) in pixel.iter_mut().zip(components) {
            *slot = component.get(i).copied().unwrap_or(0);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    #[test]
    fn the_constants_are_libjpegs_fix_of_the_jfif_coefficients() {
        #[allow(clippy::cast_possible_truncation)]
        let fix = |x: f64| (x * 65536.0 + 0.5) as i64;
        assert_eq!(FIX_1_40200, fix(1.40200));
        assert_eq!(FIX_1_77200, fix(1.77200));
        assert_eq!(FIX_0_71414, fix(0.71414));
        assert_eq!(FIX_0_34414, fix(0.34414));
    }

    #[test]
    fn the_arithmetic_is_libjpegs_tables_on_every_input() {
        // All 2^24 (Y, Cb, Cr): the computed channels, before the range
        // limit, against the tables libjpeg builds.
        let ycc = Ycc::new();
        for y in 0..=255u8 {
            for cb in 0..=255u8 {
                for cr in 0..=255u8 {
                    assert_eq!(
                        ycc_to_rgb(y, cb, cr),
                        ycc.rgb(y, cb, cr),
                        "({y}, {cb}, {cr})"
                    );
                }
            }
        }
    }

    #[test]
    fn the_argb_rows_are_the_arithmetic_on_every_input() {
        // Every (Y, Cb, Cr) through the row function, a row of 256 Crs --
        // and a row of 13, so the part the eight-at-a-time path leaves to
        // the one-at-a-time one is checked too.
        let cr: Vec<u8> = (0..=255).collect();
        let mut argb = vec![0u32; 256];
        for y in 0..=255u8 {
            for cb in 0..=255u8 {
                ycc_argb(&[y; 256], &[cb; 256], &cr, &mut argb);
                for (&pixel, &cr) in argb.iter().zip(&cr) {
                    let [r, g, b] = ycc_to_rgb(y, cb, cr);
                    let expect = u32::from_be_bytes([0xFF, clamp(r), clamp(g), clamp(b)]);
                    assert_eq!(pixel, expect, "({y}, {cb}, {cr})");
                }
                let mut short = [0u32; 13];
                ycc_argb(&[y; 13], &[cb; 13], &cr[100..113], &mut short);
                assert_eq!(short[..], argb[100..113]);
            }
        }
    }

    #[test]
    fn the_argb_row_is_the_rgb_row_packed() {
        let y: Vec<u8> = (0..=255).collect();
        let cb: Vec<u8> = (0..=255).rev().collect();
        let cr: Vec<u8> = (0..=255).map(|v: u8| v.wrapping_mul(37)).collect();
        let mut rgb = vec![0u8; 256 * 3];
        ycc_rgb(&y, &cb, &cr, &mut rgb);
        let mut argb = vec![0u32; 256];
        ycc_argb(&y, &cb, &cr, &mut argb);
        for (pixel, p) in argb.iter().zip(rgb.chunks_exact(3)) {
            assert_eq!(*pixel, u32::from_be_bytes([0xFF, p[0], p[1], p[2]]));
        }
    }

    #[test]
    fn neutral_chroma_is_grey_and_the_extremes_clamp() {
        let mut out = [0u8; 9];
        ycc_rgb(&[100, 0, 255], &[128, 255, 0], &[128, 255, 0], &mut out);
        assert_eq!(&out[..3], &[100, 100, 100]);
        // Full blue-difference and red-difference on black: red and blue
        // rise, green clamps at 0 -- and the reverse on white. The shifts
        // round toward minus infinity, as libjpeg's arithmetic shift does:
        // 116130 * 127 + 32768 is 225.5 times 65536, giving 225.
        assert_eq!(&out[3..6], &[178, 0, 225]);
        assert_eq!(&out[6..], &[76, 255, 28]);
    }

    #[test]
    fn inverted_cmyk_is_scaled_by_k() {
        let mut out = [0u32; 3];
        cmyk_argb(
            &[255, 255, 255, 255, 255, 128, 0, 0, 200, 100, 50, 100],
            &mut out,
        );
        // 200 * 100 / 255 = 78.4, truncated.
        assert_eq!(out, [0xFFFF_FFFF, 0xFF00_0000, 0xFF4E_2713]);
    }

    #[test]
    fn ycck_is_the_ycbcr_conversion_inverted() {
        let mut rgb = [0u8; 3];
        let mut cmyk = [0u8; 4];
        ycc_rgb(&[90], &[140], &[100], &mut rgb);
        ycck_cmyk(&[90], &[140], &[100], &[7], &mut cmyk);
        assert_eq!(cmyk, [255 - rgb[0], 255 - rgb[1], 255 - rgb[2], 7]);
    }
}
