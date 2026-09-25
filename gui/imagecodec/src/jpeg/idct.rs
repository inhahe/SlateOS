//! The inverse DCT: libjpeg-turbo's "accurate integer" transform
//! (`jidctint.c`'s `jpeg_idct_islow`) and the reduced-size transforms scaled
//! decoding uses (`jidctred.c`'s 4x4, 2x2 and 1x1).
//!
//! libjpeg-turbo's default method, and the one Chrome, Pillow and every
//! desktop decode with. It is fixed-point throughout -- constants scaled by
//! 2^13, an intermediate scaled by 2^2, rounding by adding half before each
//! shift -- so reproducing its arithmetic operation for operation reproduces
//! its output exactly. That includes the arithmetic of a corrupt file:
//! products in 64 bits (libjpeg's `JLONG` on the 64-bit Linux the reference is
//! built on), the intermediate truncated to 32 bits where libjpeg stores it in
//! an `int`, and the final value taken modulo 1024 before the range limit, so a
//! wildly out-of-range sample wraps as libjpeg's does rather than clamping.
//!
//! libjpeg-turbo's SIMD builds compute the same thing in 16-bit lanes, and so
//! agree with this wherever nothing overflows 16 bits -- in every valid file,
//! and in most corrupt ones. Where a corrupt file's coefficients do overflow,
//! the SIMD and plain C builds of libjpeg-turbo differ from each other; this
//! follows the C code, which is the reference and is the same on every
//! machine.
//!
//! Each transform skips work for columns and rows whose AC terms are all zero,
//! as libjpeg's do. Those shortcuts compute what the full arithmetic would for
//! any value that does not overflow; they are kept because where one does
//! overflow they are what libjpeg computes.

const CONST_BITS: u32 = 13;
const PASS1_BITS: u32 = 2;

const FIX_0_211164243: i64 = 1730;
const FIX_0_298631336: i64 = 2446;
const FIX_0_390180644: i64 = 3196;
const FIX_0_509795579: i64 = 4176;
const FIX_0_541196100: i64 = 4433;
const FIX_0_601344887: i64 = 4926;
const FIX_0_720959822: i64 = 5906;
const FIX_0_765366865: i64 = 6270;
const FIX_0_850430095: i64 = 6967;
const FIX_0_899976223: i64 = 7373;
const FIX_1_061594337: i64 = 8697;
const FIX_1_175875602: i64 = 9633;
const FIX_1_272758580: i64 = 10426;
const FIX_1_451774981: i64 = 11893;
const FIX_1_501321110: i64 = 12299;
const FIX_1_847759065: i64 = 15137;
const FIX_1_961570560: i64 = 16069;
const FIX_2_053119869: i64 = 16819;
const FIX_2_172734803: i64 = 17799;
const FIX_2_562915447: i64 = 20995;
const FIX_3_072711026: i64 = 25172;
const FIX_3_624509785: i64 = 29692;

/// `DESCALE`: shift right by `n` with rounding.
#[inline]
const fn descale(x: i64, n: u32) -> i64 {
    x.wrapping_add(1i64 << n.wrapping_sub(1)) >> n
}

/// `LEFT_SHIFT`, on a 64-bit `JLONG`.
#[inline]
const fn left(x: i64, n: u32) -> i64 {
    x.wrapping_shl(n)
}

/// `DEQUANTIZE`: an `int` product (it cannot overflow: a 16-bit coefficient
/// times a 16-bit quantiser).
#[inline]
fn dequantize(coef: i16, quant: u16) -> i64 {
    i64::from(i32::from(coef).wrapping_mul(i32::from(quant)))
}

/// The post-IDCT range limit: the value modulo 1024 as a signed 10-bit
/// number, level-shifted by 128 and clamped to a byte (`range_limit[x &
/// RANGE_MASK]` over libjpeg's table).
#[inline]
const fn limit(x: i64) -> u8 {
    let masked = (x & 1023) as i32;
    let signed = if masked >= 512 { masked.wrapping_sub(1024) } else { masked };
    let shifted = signed.wrapping_add(128);
    if shifted < 0 {
        0
    } else if shifted > 255 {
        255
    } else {
        shifted as u8
    }
}

/// Where a transform writes: `out[at + row * stride + col]`.
pub(super) struct Target<'a> {
    pub(super) out: &'a mut [u8],
    pub(super) at: usize,
    pub(super) stride: usize,
}

impl Target<'_> {
    #[inline]
    fn put(&mut self, row: usize, col: usize, value: u8) {
        let index = self
            .at
            .wrapping_add(row.wrapping_mul(self.stride))
            .wrapping_add(col);
        if let Some(slot) = self.out.get_mut(index) {
            *slot = value;
        }
    }
}

/// The transform for a block reconstructed at `size` samples a side (1, 2, 4
/// or 8; anything else writes nothing, which the caller never asks for).
pub(super) fn inverse(size: usize, coef: &[i16; 64], quant: &[u16; 64], target: &mut Target<'_>) {
    match size {
        8 => islow(coef, quant, target),
        4 => idct_4x4(coef, quant, target),
        2 => idct_2x2(coef, quant, target),
        1 => idct_1x1(coef, quant, target),
        _ => {}
    }
}

/// The coefficients each transform reads, in natural order: a block kept for
/// a reduced transform need keep only these (and whether each of the others
/// is zero).
pub(super) const fn reads(size: usize, position: usize) -> bool {
    let (row, col) = (position >> 3, position & 7);
    match size {
        1 => position == 0,
        2 => matches!(row, 0 | 1 | 3 | 5 | 7) && matches!(col, 0 | 1 | 3 | 5 | 7),
        4 => row != 4 && col != 4,
        _ => true,
    }
}

#[inline]
fn at(values: &[i16; 64], row: usize, col: usize) -> i16 {
    values.get(row.wrapping_mul(8).wrapping_add(col)).copied().unwrap_or(0)
}

#[inline]
fn q(values: &[u16; 64], row: usize, col: usize) -> u16 {
    values.get(row.wrapping_mul(8).wrapping_add(col)).copied().unwrap_or(0)
}

/// `jpeg_idct_islow`.
#[allow(
    clippy::similar_names,
    reason = "libjpeg's own variable names, kept for comparison"
)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "a coefficient and a quantiser are 16 bits each, so a product is under 2^31, and sums of a few of them times constants under 2^15 stay under 2^52: libjpeg's 64-bit arithmetic cannot overflow. Indices are loop counters under 8 into arrays of 64"
)]
fn islow(coef: &[i16; 64], quant: &[u16; 64], target: &mut Target<'_>) {
    let mut ws = [0i32; 64];
    // Pass 1: columns.
    for col in 0..8 {
        let c = |row: usize| at(coef, row, col);
        let qv = |row: usize| q(quant, row, col);
        if (1..8).all(|row| c(row) == 0) {
            let dc = left(dequantize(c(0), qv(0)), PASS1_BITS) as i32;
            for row in 0..8 {
                ws[row * 8 + col] = dc;
            }
            continue;
        }
        let z2 = dequantize(c(2), qv(2));
        let z3 = dequantize(c(6), qv(6));
        let z1 = (z2 + z3) * FIX_0_541196100;
        let tmp2 = z1 + z3 * -FIX_1_847759065;
        let tmp3 = z1 + z2 * FIX_0_765366865;
        let z2 = dequantize(c(0), qv(0));
        let z3 = dequantize(c(4), qv(4));
        let tmp0 = left(z2 + z3, CONST_BITS);
        let tmp1 = left(z2 - z3, CONST_BITS);
        let tmp10 = tmp0 + tmp3;
        let tmp13 = tmp0 - tmp3;
        let tmp11 = tmp1 + tmp2;
        let tmp12 = tmp1 - tmp2;
        let (mut tmp0, mut tmp1, mut tmp2, mut tmp3) = (
            dequantize(c(7), qv(7)),
            dequantize(c(5), qv(5)),
            dequantize(c(3), qv(3)),
            dequantize(c(1), qv(1)),
        );
        let z1 = tmp0 + tmp3;
        let z2 = tmp1 + tmp2;
        let z3 = tmp0 + tmp2;
        let z4 = tmp1 + tmp3;
        let z5 = (z3 + z4) * FIX_1_175875602;
        tmp0 *= FIX_0_298631336;
        tmp1 *= FIX_2_053119869;
        tmp2 *= FIX_3_072711026;
        tmp3 *= FIX_1_501321110;
        let z1 = z1 * -FIX_0_899976223;
        let z2 = z2 * -FIX_2_562915447;
        let z3 = z3 * -FIX_1_961570560 + z5;
        let z4 = z4 * -FIX_0_390180644 + z5;
        tmp0 += z1 + z3;
        tmp1 += z2 + z4;
        tmp2 += z2 + z3;
        tmp3 += z1 + z4;
        let d = CONST_BITS - PASS1_BITS;
        ws[col] = descale(tmp10 + tmp3, d) as i32;
        ws[7 * 8 + col] = descale(tmp10 - tmp3, d) as i32;
        ws[8 + col] = descale(tmp11 + tmp2, d) as i32;
        ws[6 * 8 + col] = descale(tmp11 - tmp2, d) as i32;
        ws[2 * 8 + col] = descale(tmp12 + tmp1, d) as i32;
        ws[5 * 8 + col] = descale(tmp12 - tmp1, d) as i32;
        ws[3 * 8 + col] = descale(tmp13 + tmp0, d) as i32;
        ws[4 * 8 + col] = descale(tmp13 - tmp0, d) as i32;
    }
    // Pass 2: rows.
    let d = CONST_BITS + PASS1_BITS + 3;
    for row in 0..8 {
        let w = |col: usize| i64::from(ws[row * 8 + col]);
        if (1..8).all(|col| w(col) == 0) {
            let dc = limit(descale(w(0), PASS1_BITS + 3));
            for col in 0..8 {
                target.put(row, col, dc);
            }
            continue;
        }
        let z2 = w(2);
        let z3 = w(6);
        let z1 = (z2 + z3) * FIX_0_541196100;
        let tmp2 = z1 + z3 * -FIX_1_847759065;
        let tmp3 = z1 + z2 * FIX_0_765366865;
        let tmp0 = left(w(0) + w(4), CONST_BITS);
        let tmp1 = left(w(0) - w(4), CONST_BITS);
        let tmp10 = tmp0 + tmp3;
        let tmp13 = tmp0 - tmp3;
        let tmp11 = tmp1 + tmp2;
        let tmp12 = tmp1 - tmp2;
        let (mut tmp0, mut tmp1, mut tmp2, mut tmp3) = (w(7), w(5), w(3), w(1));
        let z1 = tmp0 + tmp3;
        let z2 = tmp1 + tmp2;
        let z3 = tmp0 + tmp2;
        let z4 = tmp1 + tmp3;
        let z5 = (z3 + z4) * FIX_1_175875602;
        tmp0 *= FIX_0_298631336;
        tmp1 *= FIX_2_053119869;
        tmp2 *= FIX_3_072711026;
        tmp3 *= FIX_1_501321110;
        let z1 = z1 * -FIX_0_899976223;
        let z2 = z2 * -FIX_2_562915447;
        let z3 = z3 * -FIX_1_961570560 + z5;
        let z4 = z4 * -FIX_0_390180644 + z5;
        tmp0 += z1 + z3;
        tmp1 += z2 + z4;
        tmp2 += z2 + z3;
        tmp3 += z1 + z4;
        target.put(row, 0, limit(descale(tmp10 + tmp3, d)));
        target.put(row, 7, limit(descale(tmp10 - tmp3, d)));
        target.put(row, 1, limit(descale(tmp11 + tmp2, d)));
        target.put(row, 6, limit(descale(tmp11 - tmp2, d)));
        target.put(row, 2, limit(descale(tmp12 + tmp1, d)));
        target.put(row, 5, limit(descale(tmp12 - tmp1, d)));
        target.put(row, 3, limit(descale(tmp13 + tmp0, d)));
        target.put(row, 4, limit(descale(tmp13 - tmp0, d)));
    }
}

/// `jpeg_idct_4x4`.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "a coefficient and a quantiser are 16 bits each, so a product is under 2^31, and sums of a few of them times constants under 2^15 stay under 2^52: libjpeg's 64-bit arithmetic cannot overflow. Indices are loop counters under 8 into arrays of 64"
)]
fn idct_4x4(coef: &[i16; 64], quant: &[u16; 64], target: &mut Target<'_>) {
    let mut ws = [0i32; 8 * 4];
    for col in 0..8 {
        // Column 4 is never read by the second pass.
        if col == 4 {
            continue;
        }
        let c = |row: usize| at(coef, row, col);
        let qv = |row: usize| q(quant, row, col);
        if [1, 2, 3, 5, 6, 7].iter().all(|&row| c(row) == 0) {
            let dc = left(dequantize(c(0), qv(0)), PASS1_BITS) as i32;
            for row in 0..4 {
                ws[row * 8 + col] = dc;
            }
            continue;
        }
        let tmp0 = left(dequantize(c(0), qv(0)), CONST_BITS + 1);
        let z2 = dequantize(c(2), qv(2));
        let z3 = dequantize(c(6), qv(6));
        let tmp2 = z2 * FIX_1_847759065 + z3 * -FIX_0_765366865;
        let tmp10 = tmp0 + tmp2;
        let tmp12 = tmp0 - tmp2;
        let z1 = dequantize(c(7), qv(7));
        let z2 = dequantize(c(5), qv(5));
        let z3 = dequantize(c(3), qv(3));
        let z4 = dequantize(c(1), qv(1));
        let tmp0 = z1 * -FIX_0_211164243
            + z2 * FIX_1_451774981
            + z3 * -FIX_2_172734803
            + z4 * FIX_1_061594337;
        let tmp2 = z1 * -FIX_0_509795579
            + z2 * -FIX_0_601344887
            + z3 * FIX_0_899976223
            + z4 * FIX_2_562915447;
        let d = CONST_BITS - PASS1_BITS + 1;
        ws[col] = descale(tmp10 + tmp2, d) as i32;
        ws[3 * 8 + col] = descale(tmp10 - tmp2, d) as i32;
        ws[8 + col] = descale(tmp12 + tmp0, d) as i32;
        ws[2 * 8 + col] = descale(tmp12 - tmp0, d) as i32;
    }
    let d = CONST_BITS + PASS1_BITS + 3 + 1;
    for row in 0..4 {
        let w = |col: usize| i64::from(ws[row * 8 + col]);
        if [1, 2, 3, 5, 6, 7].iter().all(|&col| w(col) == 0) {
            let dc = limit(descale(w(0), PASS1_BITS + 3));
            for col in 0..4 {
                target.put(row, col, dc);
            }
            continue;
        }
        let tmp0 = left(w(0), CONST_BITS + 1);
        let tmp2 = w(2) * FIX_1_847759065 + w(6) * -FIX_0_765366865;
        let tmp10 = tmp0 + tmp2;
        let tmp12 = tmp0 - tmp2;
        let (z1, z2, z3, z4) = (w(7), w(5), w(3), w(1));
        let tmp0 = z1 * -FIX_0_211164243
            + z2 * FIX_1_451774981
            + z3 * -FIX_2_172734803
            + z4 * FIX_1_061594337;
        let tmp2 = z1 * -FIX_0_509795579
            + z2 * -FIX_0_601344887
            + z3 * FIX_0_899976223
            + z4 * FIX_2_562915447;
        target.put(row, 0, limit(descale(tmp10 + tmp2, d)));
        target.put(row, 3, limit(descale(tmp10 - tmp2, d)));
        target.put(row, 1, limit(descale(tmp12 + tmp0, d)));
        target.put(row, 2, limit(descale(tmp12 - tmp0, d)));
    }
}

/// `jpeg_idct_2x2`.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "a coefficient and a quantiser are 16 bits each, so a product is under 2^31, and sums of a few of them times constants under 2^15 stay under 2^52: libjpeg's 64-bit arithmetic cannot overflow. Indices are loop counters under 8 into arrays of 64"
)]
fn idct_2x2(coef: &[i16; 64], quant: &[u16; 64], target: &mut Target<'_>) {
    let mut ws = [0i32; 8 * 2];
    for col in 0..8 {
        // Columns 2, 4 and 6 are never read by the second pass.
        if matches!(col, 2 | 4 | 6) {
            continue;
        }
        let c = |row: usize| at(coef, row, col);
        let qv = |row: usize| q(quant, row, col);
        if [1, 3, 5, 7].iter().all(|&row| c(row) == 0) {
            let dc = left(dequantize(c(0), qv(0)), PASS1_BITS) as i32;
            ws[col] = dc;
            ws[8 + col] = dc;
            continue;
        }
        let tmp10 = left(dequantize(c(0), qv(0)), CONST_BITS + 2);
        let tmp0 = dequantize(c(7), qv(7)) * -FIX_0_720959822
            + dequantize(c(5), qv(5)) * FIX_0_850430095
            + dequantize(c(3), qv(3)) * -FIX_1_272758580
            + dequantize(c(1), qv(1)) * FIX_3_624509785;
        let d = CONST_BITS - PASS1_BITS + 2;
        ws[col] = descale(tmp10 + tmp0, d) as i32;
        ws[8 + col] = descale(tmp10 - tmp0, d) as i32;
    }
    let d = CONST_BITS + PASS1_BITS + 3 + 2;
    for row in 0..2 {
        let w = |col: usize| i64::from(ws[row * 8 + col]);
        if [1, 3, 5, 7].iter().all(|&col| w(col) == 0) {
            let dc = limit(descale(w(0), PASS1_BITS + 3));
            target.put(row, 0, dc);
            target.put(row, 1, dc);
            continue;
        }
        let tmp10 = left(w(0), CONST_BITS + 2);
        let tmp0 = w(7) * -FIX_0_720959822
            + w(5) * FIX_0_850430095
            + w(3) * -FIX_1_272758580
            + w(1) * FIX_3_624509785;
        target.put(row, 0, limit(descale(tmp10 + tmp0, d)));
        target.put(row, 1, limit(descale(tmp10 - tmp0, d)));
    }
}

/// `jpeg_idct_1x1`: the block's mean, a descaled DC.
fn idct_1x1(coef: &[i16; 64], quant: &[u16; 64], target: &mut Target<'_>) {
    let dc = i32::from(coef[0]).wrapping_mul(i32::from(quant[0]));
    let value = descale(i64::from(dc), 3) as i32;
    target.put(0, 0, limit(i64::from(value)));
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn run(size: usize, coef: &[i16; 64], quant: &[u16; 64]) -> [u8; 64] {
        let mut out = [0u8; 64];
        let mut target = Target {
            out: &mut out,
            at: 0,
            stride: 8,
        };
        inverse(size, coef, quant, &mut target);
        out
    }

    #[test]
    fn a_dc_only_block_is_flat_at_every_size() {
        let mut coef = [0i16; 64];
        coef[0] = 40;
        let quant = [2u16; 64];
        // 40 * 2 / 8 = 10 above mid-grey.
        for size in [1, 2, 4, 8] {
            let out = run(size, &coef, &quant);
            for row in 0..size {
                for col in 0..size {
                    assert_eq!(out[row * 8 + col], 138, "size {size}");
                }
            }
        }
    }

    #[test]
    fn the_range_limit_wraps_as_libjpegs_table_does() {
        assert_eq!(limit(0), 128);
        assert_eq!(limit(127), 255);
        assert_eq!(limit(200), 255);
        assert_eq!(limit(511), 255);
        assert_eq!(limit(512), 0);
        assert_eq!(limit(-128), 0);
        assert_eq!(limit(-1), 127);
        // Past the table's reach the value wraps: 1024 + 5 reads as 5.
        assert_eq!(limit(1029), 133);
    }

    #[test]
    fn a_reduced_transform_reads_only_what_it_keeps() {
        // Changing a coefficient a transform does not read changes nothing.
        let mut coef = [0i16; 64];
        for (i, c) in coef.iter_mut().enumerate() {
            *c = (i as i16 * 7) % 23 - 11;
        }
        let quant = [3u16; 64];
        for size in [1, 2, 4] {
            let before = run(size, &coef, &quant);
            let mut changed = coef;
            for (position, c) in changed.iter_mut().enumerate() {
                if !reads(size, position) {
                    *c = c.wrapping_add(17);
                }
            }
            assert_eq!(before, run(size, &changed, &quant), "size {size}");
        }
    }
}
