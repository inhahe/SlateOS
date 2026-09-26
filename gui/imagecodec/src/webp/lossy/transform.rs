//! VP8's inverse transforms (RFC 6386 §14.3, §14.4), as libwebp computes them.
//!
//! Two transforms turn coefficients back into samples: a 4x4 inverse DCT for
//! every block, and -- for a macroblock predicted as a whole -- a 4x4 inverse
//! Walsh-Hadamard transform that first rebuilds the sixteen blocks' DC
//! coefficients from one "second-order" block. Both are integer arithmetic
//! specified to the bit, so every correct decoder agrees exactly.
//!
//! # Hostile coefficients
//!
//! A coefficient is a quantised level times a dequantisation factor, stored in
//! sixteen bits as libwebp stores it; a crafted stream can make that product
//! wrap, and the transforms' sums then exceed what a valid stream ever
//! produces. libwebp computes them in C `int`, where the overflow is undefined
//! and in practice wraps; this module wraps explicitly, so a hostile stream
//! decodes to the same garbage libwebp shows and never panics.

/// `x * sqrt(2) * cos(pi/8)` in the RFC's fixed point: `x + (x * 20091 >> 16)`.
const fn mul1(x: i32) -> i32 {
    (x.wrapping_mul(20091) >> 16).wrapping_add(x)
}

/// `x * sqrt(2) * sin(pi/8)` in the RFC's fixed point: `x * 35468 >> 16`.
const fn mul2(x: i32) -> i32 {
    x.wrapping_mul(35468) >> 16
}

/// `value` clamped to a sample.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=255 first"
)]
pub(super) const fn clamp(value: i32) -> u8 {
    if value < 0 {
        0
    } else if value > 255 {
        255
    } else {
        value as u8
    }
}

/// Inverse-DCT `coeffs` (a block in raster order) and add the result to the
/// 4x4 block of `pixels` whose top-left sample is `at`, rows `stride` apart.
///
/// Columns first, then rows, each with the RFC's butterfly; the rounding
/// `+ 4` rides on the DC term of the second pass, which reaches every output.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "coeffs and tmp are fixed 16-element arrays indexed by 0..4 plus multiples of 4 below 16, which is all the index arithmetic does; the pixel rows are taken with get and skipped if absent"
)]
pub(super) fn add_idct(coeffs: &[i16; 16], pixels: &mut [u8], at: usize, stride: usize) {
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let c0 = i32::from(coeffs[i]);
        let c4 = i32::from(coeffs[4 + i]);
        let c8 = i32::from(coeffs[8 + i]);
        let c12 = i32::from(coeffs[12 + i]);
        let a = c0.wrapping_add(c8);
        let b = c0.wrapping_sub(c8);
        let c = mul2(c4).wrapping_sub(mul1(c12));
        let d = mul1(c4).wrapping_add(mul2(c12));
        tmp[4 * i] = a.wrapping_add(d);
        tmp[4 * i + 1] = b.wrapping_add(c);
        tmp[4 * i + 2] = b.wrapping_sub(c);
        tmp[4 * i + 3] = a.wrapping_sub(d);
    }
    for r in 0..4 {
        let dc = tmp[r].wrapping_add(4);
        let a = dc.wrapping_add(tmp[8 + r]);
        let b = dc.wrapping_sub(tmp[8 + r]);
        let c = mul2(tmp[4 + r]).wrapping_sub(mul1(tmp[12 + r]));
        let d = mul1(tmp[4 + r]).wrapping_add(mul2(tmp[12 + r]));
        let start = at.wrapping_add(r.wrapping_mul(stride));
        let Some(row) = pixels.get_mut(start..start.wrapping_add(4)) else {
            continue;
        };
        for (sample, delta) in row.iter_mut().zip([
            a.wrapping_add(d),
            b.wrapping_add(c),
            b.wrapping_sub(c),
            a.wrapping_sub(d),
        ]) {
            *sample = clamp(i32::from(*sample).wrapping_add(delta >> 3));
        }
    }
}

/// Add a block whose only coefficient is its DC: every sample moves by the
/// same `(dc + 4) >> 3`, which is what [`add_idct`] computes for such a block,
/// without the multiplications.
pub(super) fn add_dc(dc: i16, pixels: &mut [u8], at: usize, stride: usize) {
    let delta = i32::from(dc).wrapping_add(4) >> 3;
    for r in 0..4usize {
        let start = at.wrapping_add(r.wrapping_mul(stride));
        if let Some(row) = pixels.get_mut(start..start.wrapping_add(4)) {
            for sample in row {
                *sample = clamp(i32::from(*sample).wrapping_add(delta));
            }
        }
    }
}

/// The inverse Walsh-Hadamard transform of a macroblock's second-order block:
/// sixteen DC coefficients, one per luma block in raster order, rounded as
/// the RFC rounds them (`+ 3 >> 3`) and kept, as libwebp keeps them, in
/// sixteen bits.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "fixed 16-element arrays indexed by 0..4 plus multiples of 4 below 16, which is all the index arithmetic does; the narrowing to i16 is libwebp's own"
)]
pub(super) fn inverse_wht(input: &[i16; 16]) -> [i16; 16] {
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let i0 = i32::from(input[i]);
        let i4 = i32::from(input[4 + i]);
        let i8 = i32::from(input[8 + i]);
        let i12 = i32::from(input[12 + i]);
        let a0 = i0.wrapping_add(i12);
        let a1 = i4.wrapping_add(i8);
        let a2 = i4.wrapping_sub(i8);
        let a3 = i0.wrapping_sub(i12);
        tmp[i] = a0.wrapping_add(a1);
        tmp[8 + i] = a0.wrapping_sub(a1);
        tmp[4 + i] = a3.wrapping_add(a2);
        tmp[12 + i] = a3.wrapping_sub(a2);
    }
    let mut out = [0i16; 16];
    for i in 0..4 {
        let dc = tmp[4 * i].wrapping_add(3);
        let a0 = dc.wrapping_add(tmp[4 * i + 3]);
        let a1 = tmp[4 * i + 1].wrapping_add(tmp[4 * i + 2]);
        let a2 = tmp[4 * i + 1].wrapping_sub(tmp[4 * i + 2]);
        let a3 = dc.wrapping_sub(tmp[4 * i + 3]);
        out[4 * i] = (a0.wrapping_add(a1) >> 3) as i16;
        out[4 * i + 1] = (a3.wrapping_add(a2) >> 3) as i16;
        out[4 * i + 2] = (a0.wrapping_sub(a1) >> 3) as i16;
        out[4 * i + 3] = (a3.wrapping_sub(a2) >> 3) as i16;
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

    /// The RFC's own inverse DCT (§14.4, `short_idct4x4llm_c`), transcribed
    /// with its sixteen-bit intermediate, for valid-range coefficients.
    fn rfc_idct(input: &[i16; 16]) -> [i32; 16] {
        let (c1, s1) = (20091i32, 35468i32);
        let mut out = [0i16; 16];
        for i in 0..4 {
            let ip = |k: usize| i32::from(input[i + k]);
            let a1 = ip(0) + ip(8);
            let b1 = ip(0) - ip(8);
            let t1 = (ip(4) * s1) >> 16;
            let t2 = ip(12) + ((ip(12) * c1) >> 16);
            let cc = t1 - t2;
            let t1 = ip(4) + ((ip(4) * c1) >> 16);
            let t2 = (ip(12) * s1) >> 16;
            let dd = t1 + t2;
            out[i] = (a1 + dd) as i16;
            out[12 + i] = (a1 - dd) as i16;
            out[4 + i] = (b1 + cc) as i16;
            out[8 + i] = (b1 - cc) as i16;
        }
        let mut result = [0i32; 16];
        for r in 0..4 {
            let ip = |k: usize| i32::from(out[4 * r + k]);
            let a1 = ip(0) + ip(2);
            let b1 = ip(0) - ip(2);
            let t1 = (ip(1) * s1) >> 16;
            let t2 = ip(3) + ((ip(3) * c1) >> 16);
            let cc = t1 - t2;
            let t1 = ip(1) + ((ip(1) * c1) >> 16);
            let t2 = (ip(3) * s1) >> 16;
            let dd = t1 + t2;
            result[4 * r] = (a1 + dd + 4) >> 3;
            result[4 * r + 3] = (a1 - dd + 4) >> 3;
            result[4 * r + 1] = (b1 + cc + 4) >> 3;
            result[4 * r + 2] = (b1 - cc + 4) >> 3;
        }
        result
    }

    /// Coefficients a valid stream can produce: levels up to 2048 times
    /// factors up to 157 would overflow sixteen bits, so the spread here stays
    /// inside what the RFC's own sixteen-bit intermediate holds.
    fn blocks() -> impl Iterator<Item = [i16; 16]> {
        let mut state = 7u32;
        (0..3000).map(move |k| {
            let mut block = [0i16; 16];
            let live = 1 + k % 16;
            for c in block.iter_mut().take(live) {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let range = if k % 3 == 0 { 2048 } else { 300 };
                *c = ((state >> 8) % (2 * range + 1)) as i16 - range as i16;
            }
            block
        })
    }

    #[test]
    fn the_idct_is_the_rfcs() {
        for block in blocks() {
            let want = rfc_idct(&block);
            // Added to mid-grey, and clamped, as reconstruction does.
            let mut pixels = [128u8; 16];
            add_idct(&block, &mut pixels, 0, 4);
            for (i, (&got, &residue)) in pixels.iter().zip(&want).enumerate() {
                assert_eq!(got, clamp(128 + residue), "{block:?} sample {i}");
            }
        }
    }

    #[test]
    fn a_dc_only_block_moves_every_sample_alike() {
        for dc in [-2048i16, -300, -5, -4, -3, 0, 3, 4, 5, 300, 2047] {
            let mut block = [0i16; 16];
            block[0] = dc;
            let mut full = [100u8; 16];
            add_idct(&block, &mut full, 0, 4);
            let mut quick = [100u8; 16];
            add_dc(dc, &mut quick, 0, 4);
            assert_eq!(full, quick, "dc {dc}");
        }
    }

    #[test]
    fn the_wht_is_the_rfcs() {
        // `vp8_short_inv_walsh4x4_c` (RFC 6386 §14.3), transcribed.
        fn rfc_wht(input: &[i16; 16]) -> [i16; 16] {
            let mut out = [0i32; 16];
            for i in 0..4 {
                let ip = |k: usize| i32::from(input[i + k]);
                let a1 = ip(0) + ip(12);
                let b1 = ip(4) + ip(8);
                let c1 = ip(4) - ip(8);
                let d1 = ip(0) - ip(12);
                out[i] = a1 + b1;
                out[4 + i] = c1 + d1;
                out[8 + i] = a1 - b1;
                out[12 + i] = d1 - c1;
            }
            let mut result = [0i16; 16];
            for r in 0..4 {
                let ip = |k: usize| out[4 * r + k];
                let a1 = ip(0) + ip(3);
                let b1 = ip(1) + ip(2);
                let c1 = ip(1) - ip(2);
                let d1 = ip(0) - ip(3);
                result[4 * r] = ((a1 + b1 + 3) >> 3) as i16;
                result[4 * r + 1] = ((c1 + d1 + 3) >> 3) as i16;
                result[4 * r + 2] = ((a1 - b1 + 3) >> 3) as i16;
                result[4 * r + 3] = ((d1 - c1 + 3) >> 3) as i16;
            }
            result
        }
        for block in blocks() {
            assert_eq!(inverse_wht(&block), rfc_wht(&block), "{block:?}");
        }
        // A DC alone spreads evenly, as libwebp's shortcut for it assumes.
        for dc in [-4000i16, -9, -8, -3, 0, 1, 5, 8, 4000] {
            let mut block = [0i16; 16];
            block[0] = dc;
            let spread = inverse_wht(&block);
            assert!(spread.iter().all(|&v| v == (dc + 3) >> 3), "dc {dc}");
        }
    }

    #[test]
    fn hostile_coefficients_wrap_rather_than_panic() {
        let block = [i16::MAX; 16];
        let mut pixels = [0u8; 16];
        add_idct(&block, &mut pixels, 0, 4);
        let _ = inverse_wht(&[i16::MIN; 16]);
        add_dc(i16::MAX, &mut pixels, 0, 4);
    }
}
