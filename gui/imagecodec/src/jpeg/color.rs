//! Colour conversion: libjpeg-turbo's `jdcolor.c`.
//!
//! JPEG stores colour as luma and two colour differences (YCbCr), almost
//! always; libjpeg converts to RGB with fixed-point tables built from the
//! JFIF coefficients, rounding once per channel, and the tables here are built
//! the same way so that every pixel comes out the same. The other conversions
//! a decoder is asked for are copies (greyscale to RGB, RGB to RGB, CMYK to
//! CMYK, and "unknown" to itself, which is what libtiff asks for when it wants
//! the samples as stored) or YCCK to CMYK, which is the YCbCr conversion
//! inverted.

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

/// `build_ycc_rgb_table`.
#[derive(Clone)]
pub(super) struct Ycc {
    cr_r: [i32; 256],
    cb_b: [i32; 256],
    cr_g: [i64; 256],
    cb_g: [i64; 256],
}

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

/// `ycc_rgb_convert`, one row.
pub(super) fn ycc_rgb(ycc: &Ycc, y: &[u8], cb: &[u8], cr: &[u8], out: &mut [u8]) {
    for (((pixel, &y), &cb), &cr) in out.chunks_exact_mut(3).zip(y).zip(cb).zip(cr) {
        let [r, g, b] = ycc.rgb(y, cb, cr);
        pixel.copy_from_slice(&[clamp(r), clamp(g), clamp(b)]);
    }
}

/// `ycck_cmyk_convert`, one row: the YCbCr conversion, inverted, and K as it
/// was.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "a sample is 0-255 and the tables hold at most 1.772 * 128 either way, so every sum is within +-512; the table index is a loop counter under 256"
)]
pub(super) fn ycck_cmyk(ycc: &Ycc, y: &[u8], cb: &[u8], cr: &[u8], k: &[u8], out: &mut [u8]) {
    for ((((pixel, &y), &cb), &cr), &k) in out.chunks_exact_mut(4).zip(y).zip(cb).zip(cr).zip(k) {
        let [r, g, b] = ycc.rgb(y, cb, cr);
        pixel.copy_from_slice(&[clamp(255 - r), clamp(255 - g), clamp(255 - b), k]);
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
    fn neutral_chroma_is_grey_and_the_extremes_clamp() {
        let ycc = Ycc::new();
        let mut out = [0u8; 9];
        ycc_rgb(&ycc, &[100, 0, 255], &[128, 255, 0], &[128, 255, 0], &mut out);
        assert_eq!(&out[..3], &[100, 100, 100]);
        // Full blue-difference and red-difference on black: red and blue
        // rise, green clamps at 0 -- and the reverse on white. The shifts
        // round toward minus infinity, as libjpeg's arithmetic shift does:
        // 116130 * 127 + 32768 is 225.5 times 65536, giving 225.
        assert_eq!(&out[3..6], &[178, 0, 225]);
        assert_eq!(&out[6..], &[76, 255, 28]);
    }

    #[test]
    fn ycck_is_the_ycbcr_conversion_inverted() {
        let ycc = Ycc::new();
        let mut rgb = [0u8; 3];
        let mut cmyk = [0u8; 4];
        ycc_rgb(&ycc, &[90], &[140], &[100], &mut rgb);
        ycck_cmyk(&ycc, &[90], &[140], &[100], &[7], &mut cmyk);
        assert_eq!(cmyk, [255 - rgb[0], 255 - rgb[1], 255 - rgb[2], 7]);
    }
}
