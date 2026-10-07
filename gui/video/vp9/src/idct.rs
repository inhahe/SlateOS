//! The inverse transforms: a block's dequantised coefficients back to the
//! residual added to its prediction.
//!
//! Four sizes of DCT (4 to 32 points), three of ADST (4, 8 and 16), and for
//! lossless frames the 4-point Walsh-Hadamard transform. Each 2-D transform is
//! the 1-D one over the rows and then over the columns, rounded and added to
//! the prediction with clipping. The integer butterflies are libvpx's, line by
//! line, so the result is libvpx's bit for bit.
//!
//! # One set of transforms for every bit depth
//!
//! libvpx keeps two copies of each transform: one for 8-bit streams that
//! narrows its intermediate values to 16 bits, and one for 10- and 12-bit
//! streams that keeps 32 and gives up (outputs zero) on inputs beyond 2^25.
//! For every stream the VP9 specification allows, the two compute the same
//! numbers: the specification requires every intermediate value to fit in
//! `8 + bit depth` bits, so the narrowing never narrows and the cutoff never
//! cuts. They differ only on streams that break that rule -- and there libvpx
//! itself is not one answer, since its SIMD versions saturate where its C
//! versions wrap. So this port keeps one copy, 32-bit, with every operation a
//! damaged stream could overflow written to wrap: libvpx's result on every
//! valid stream, a deterministic one on every other, and never a panic.
//!
//! libvpx also chooses among cheaper versions of each transform by how many
//! coefficients a block has (`vp9_idct8x8_add` and its siblings). Those are
//! exact shortcuts of the full transform -- a row of zeros transforms to
//! zeros -- so this port takes the same shortcuts by testing for the zeros
//! instead: rows that are all zero are skipped, and a block with only its DC
//! coefficient is added as one constant.
//!
//! Translated into Rust from libvpx v1.17.0's `vpx_dsp/inv_txfm.c`,
//! `vpx_dsp/inv_txfm.h`, `vpx_dsp/txfm_common.h` and `vp9/common/vp9_idct.c`
//! (copyright the WebM project authors), used under libvpx's BSD licence and
//! patent grant (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "i32 arithmetic here is written with wrapping operations; the i64 arithmetic only sums a few products of an i32 and a 14-bit constant, which cannot approach i64's range"
)]

use crate::common::{
    ADST_ADST, ADST_DCT, DCT_ADST, DCT_DCT, TX_4X4, TX_8X8, TX_16X16, TxSize, TxType,
};
use crate::frame::Pixel;

// --- Constants: libvpx's txfm_common.h --------------------------------------------

const C1: i32 = 16364;
const C2: i32 = 16305;
const C3: i32 = 16207;
const C4: i32 = 16069;
const C5: i32 = 15893;
const C6: i32 = 15679;
const C7: i32 = 15426;
const C8: i32 = 15137;
const C9: i32 = 14811;
const C10: i32 = 14449;
const C11: i32 = 14053;
const C12: i32 = 13623;
const C13: i32 = 13160;
const C14: i32 = 12665;
const C15: i32 = 12140;
const C16: i32 = 11585;
const C17: i32 = 11003;
const C18: i32 = 10394;
const C19: i32 = 9760;
const C20: i32 = 9102;
const C21: i32 = 8423;
const C22: i32 = 7723;
const C23: i32 = 7005;
const C24: i32 = 6270;
const C25: i32 = 5520;
const C26: i32 = 4756;
const C27: i32 = 3981;
const C28: i32 = 3196;
const C29: i32 = 2404;
const C30: i32 = 1606;
const C31: i32 = 804;

const SINPI_1_9: i32 = 5283;
const SINPI_2_9: i32 = 9929;
const SINPI_3_9: i32 = 13377;
const SINPI_4_9: i32 = 15212;

/// The lossless transform's input scaling: libvpx's `UNIT_QUANT_SHIFT`.
const UNIT_QUANT_SHIFT: u32 = 2;

// --- Arithmetic --------------------------------------------------------------------

/// A coefficient times a constant, widened: the products libvpx forms in
/// `tran_high_t`.
#[inline(always)]
fn mul(a: i32, c: i32) -> i64 {
    i64::from(a) * i64::from(c)
}

/// libvpx's `WRAPLOW(dct_const_round_shift(x))`: round off the constants'
/// 14 fractional bits, then keep 32.
#[inline(always)]
fn rs(x: i64) -> i32 {
    (x.wrapping_add(1 << 13) >> 14) as i32
}

#[inline(always)]
fn add(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

#[inline(always)]
fn sub(a: i32, b: i32) -> i32 {
    a.wrapping_sub(b)
}

#[inline(always)]
fn neg(a: i32) -> i32 {
    a.wrapping_neg()
}

/// libvpx's `ROUND_POWER_OF_TWO`, on an `int`.
#[inline(always)]
fn round_shift(v: i32, n: u32) -> i32 {
    v.wrapping_add(1 << (n - 1)) >> n
}

// --- 4 points ------------------------------------------------------------------------

/// libvpx's `idct4_c`.
fn idct4(i: &[i32; 4]) -> [i32; 4] {
    let s0 = rs(mul(add(i[0], i[2]), C16));
    let s1 = rs(mul(sub(i[0], i[2]), C16));
    let s2 = rs(mul(i[1], C24) - mul(i[3], C8));
    let s3 = rs(mul(i[1], C8) + mul(i[3], C24));
    [add(s0, s3), add(s1, s2), sub(s1, s2), sub(s0, s3)]
}

/// libvpx's `iadst4_c`.
fn iadst4(i: &[i32; 4]) -> [i32; 4] {
    let [x0, x1, x2, x3] = *i;
    if x0 | x1 | x2 | x3 == 0 {
        return [0; 4];
    }
    let s0 = mul(x0, SINPI_1_9);
    let s1 = mul(x0, SINPI_2_9);
    let s2 = mul(x1, SINPI_3_9);
    let s3 = mul(x2, SINPI_4_9);
    let s4 = mul(x2, SINPI_1_9);
    let s5 = mul(x3, SINPI_2_9);
    let s6 = mul(x3, SINPI_4_9);
    let s7 = add(sub(x0, x2), x3);

    let s0 = s0 + s3 + s5;
    let s1 = s1 - s4 - s6;
    let s3 = s2;
    let s2 = mul(s7, SINPI_3_9);
    [rs(s0 + s3), rs(s1 + s3), rs(s2), rs(s0 + s1 - s3)]
}

// --- 8 points -------------------------------------------------------------------------

/// libvpx's `idct8_c` (as `vpx_highbd_idct8_c` lays it out: the even half is
/// the 4-point DCT).
fn idct8(i: &[i32; 8]) -> [i32; 8] {
    // Stage 1.
    let e = idct4(&[i[0], i[2], i[4], i[6]]);
    let s4 = rs(mul(i[1], C28) - mul(i[7], C4));
    let s7 = rs(mul(i[1], C4) + mul(i[7], C28));
    let s5 = rs(mul(i[5], C12) - mul(i[3], C20));
    let s6 = rs(mul(i[5], C20) + mul(i[3], C12));
    // Stage 2, odd half.
    let t4 = add(s4, s5);
    let t5 = sub(s4, s5);
    let t6 = add(neg(s6), s7);
    let t7 = add(s6, s7);
    // Stage 3, odd half.
    let u5 = rs(mul(sub(t6, t5), C16));
    let u6 = rs(mul(add(t5, t6), C16));
    // Stage 4.
    [
        add(e[0], t7),
        add(e[1], u6),
        add(e[2], u5),
        add(e[3], t4),
        sub(e[3], t4),
        sub(e[2], u5),
        sub(e[1], u6),
        sub(e[0], t7),
    ]
}

/// libvpx's `iadst8_c`.
fn iadst8(i: &[i32; 8]) -> [i32; 8] {
    let (x0, x1, x2, x3) = (i[7], i[0], i[5], i[2]);
    let (x4, x5, x6, x7) = (i[3], i[4], i[1], i[6]);
    if x0 | x1 | x2 | x3 | x4 | x5 | x6 | x7 == 0 {
        return [0; 8];
    }
    // Stage 1.
    let s0 = mul(x0, C2) + mul(x1, C30);
    let s1 = mul(x0, C30) - mul(x1, C2);
    let s2 = mul(x2, C10) + mul(x3, C22);
    let s3 = mul(x2, C22) - mul(x3, C10);
    let s4 = mul(x4, C18) + mul(x5, C14);
    let s5 = mul(x4, C14) - mul(x5, C18);
    let s6 = mul(x6, C26) + mul(x7, C6);
    let s7 = mul(x6, C6) - mul(x7, C26);
    let (x0, x1, x2, x3) = (rs(s0 + s4), rs(s1 + s5), rs(s2 + s6), rs(s3 + s7));
    let (x4, x5, x6, x7) = (rs(s0 - s4), rs(s1 - s5), rs(s2 - s6), rs(s3 - s7));
    // Stage 2.
    let s4 = mul(x4, C8) + mul(x5, C24);
    let s5 = mul(x4, C24) - mul(x5, C8);
    let s6 = -mul(x6, C24) + mul(x7, C8);
    let s7 = mul(x6, C8) + mul(x7, C24);
    let (x0, x1, x2, x3) = (add(x0, x2), add(x1, x3), sub(x0, x2), sub(x1, x3));
    let (x4, x5, x6, x7) = (rs(s4 + s6), rs(s5 + s7), rs(s4 - s6), rs(s5 - s7));
    // Stage 3.
    let x2n = rs(mul(add(x2, x3), C16));
    let x3n = rs(mul(sub(x2, x3), C16));
    let x6n = rs(mul(add(x6, x7), C16));
    let x7n = rs(mul(sub(x6, x7), C16));
    [x0, neg(x4), x6n, neg(x2n), x3n, neg(x7n), x5, neg(x1)]
}

// --- 16 points ------------------------------------------------------------------------

/// libvpx's `idct16_c`.
#[allow(
    clippy::similar_names,
    reason = "the step arrays are libvpx's own, kept so the port reads against it"
)]
fn idct16(input: &[i32; 16]) -> [i32; 16] {
    let mut s1 = [0i32; 16];
    let mut s2 = [0i32; 16];
    // Stage 1.
    s1[0] = input[0];
    s1[1] = input[8];
    s1[2] = input[4];
    s1[3] = input[12];
    s1[4] = input[2];
    s1[5] = input[10];
    s1[6] = input[6];
    s1[7] = input[14];
    s1[8] = input[1];
    s1[9] = input[9];
    s1[10] = input[5];
    s1[11] = input[13];
    s1[12] = input[3];
    s1[13] = input[11];
    s1[14] = input[7];
    s1[15] = input[15];

    // Stage 2.
    s2[0] = s1[0];
    s2[1] = s1[1];
    s2[2] = s1[2];
    s2[3] = s1[3];
    s2[4] = s1[4];
    s2[5] = s1[5];
    s2[6] = s1[6];
    s2[7] = s1[7];
    s2[8] = rs(mul(s1[8], C30) - mul(s1[15], C2));
    s2[15] = rs(mul(s1[8], C2) + mul(s1[15], C30));
    s2[9] = rs(mul(s1[9], C14) - mul(s1[14], C18));
    s2[14] = rs(mul(s1[9], C18) + mul(s1[14], C14));
    s2[10] = rs(mul(s1[10], C22) - mul(s1[13], C10));
    s2[13] = rs(mul(s1[10], C10) + mul(s1[13], C22));
    s2[11] = rs(mul(s1[11], C6) - mul(s1[12], C26));
    s2[12] = rs(mul(s1[11], C26) + mul(s1[12], C6));

    // Stage 3.
    s1[0] = s2[0];
    s1[1] = s2[1];
    s1[2] = s2[2];
    s1[3] = s2[3];
    s1[4] = rs(mul(s2[4], C28) - mul(s2[7], C4));
    s1[7] = rs(mul(s2[4], C4) + mul(s2[7], C28));
    s1[5] = rs(mul(s2[5], C12) - mul(s2[6], C20));
    s1[6] = rs(mul(s2[5], C20) + mul(s2[6], C12));
    s1[8] = add(s2[8], s2[9]);
    s1[9] = sub(s2[8], s2[9]);
    s1[10] = add(neg(s2[10]), s2[11]);
    s1[11] = add(s2[10], s2[11]);
    s1[12] = add(s2[12], s2[13]);
    s1[13] = sub(s2[12], s2[13]);
    s1[14] = add(neg(s2[14]), s2[15]);
    s1[15] = add(s2[14], s2[15]);

    // Stage 4.
    s2[0] = rs(mul(add(s1[0], s1[1]), C16));
    s2[1] = rs(mul(sub(s1[0], s1[1]), C16));
    s2[2] = rs(mul(s1[2], C24) - mul(s1[3], C8));
    s2[3] = rs(mul(s1[2], C8) + mul(s1[3], C24));
    s2[4] = add(s1[4], s1[5]);
    s2[5] = sub(s1[4], s1[5]);
    s2[6] = add(neg(s1[6]), s1[7]);
    s2[7] = add(s1[6], s1[7]);
    s2[8] = s1[8];
    s2[15] = s1[15];
    s2[9] = rs(mul(neg(s1[9]), C8) + mul(s1[14], C24));
    s2[14] = rs(mul(s1[9], C24) + mul(s1[14], C8));
    s2[10] = rs(mul(neg(s1[10]), C24) - mul(s1[13], C8));
    s2[13] = rs(mul(neg(s1[10]), C8) + mul(s1[13], C24));
    s2[11] = s1[11];
    s2[12] = s1[12];

    // Stage 5.
    s1[0] = add(s2[0], s2[3]);
    s1[1] = add(s2[1], s2[2]);
    s1[2] = sub(s2[1], s2[2]);
    s1[3] = sub(s2[0], s2[3]);
    s1[4] = s2[4];
    s1[5] = rs(mul(sub(s2[6], s2[5]), C16));
    s1[6] = rs(mul(add(s2[5], s2[6]), C16));
    s1[7] = s2[7];
    s1[8] = add(s2[8], s2[11]);
    s1[9] = add(s2[9], s2[10]);
    s1[10] = sub(s2[9], s2[10]);
    s1[11] = sub(s2[8], s2[11]);
    s1[12] = add(neg(s2[12]), s2[15]);
    s1[13] = add(neg(s2[13]), s2[14]);
    s1[14] = add(s2[13], s2[14]);
    s1[15] = add(s2[12], s2[15]);

    // Stage 6.
    s2[0] = add(s1[0], s1[7]);
    s2[1] = add(s1[1], s1[6]);
    s2[2] = add(s1[2], s1[5]);
    s2[3] = add(s1[3], s1[4]);
    s2[4] = sub(s1[3], s1[4]);
    s2[5] = sub(s1[2], s1[5]);
    s2[6] = sub(s1[1], s1[6]);
    s2[7] = sub(s1[0], s1[7]);
    s2[8] = s1[8];
    s2[9] = s1[9];
    s2[10] = rs(mul(add(neg(s1[10]), s1[13]), C16));
    s2[13] = rs(mul(add(s1[10], s1[13]), C16));
    s2[11] = rs(mul(add(neg(s1[11]), s1[12]), C16));
    s2[12] = rs(mul(add(s1[11], s1[12]), C16));
    s2[14] = s1[14];
    s2[15] = s1[15];

    // Stage 7.
    [
        add(s2[0], s2[15]),
        add(s2[1], s2[14]),
        add(s2[2], s2[13]),
        add(s2[3], s2[12]),
        add(s2[4], s2[11]),
        add(s2[5], s2[10]),
        add(s2[6], s2[9]),
        add(s2[7], s2[8]),
        sub(s2[7], s2[8]),
        sub(s2[6], s2[9]),
        sub(s2[5], s2[10]),
        sub(s2[4], s2[11]),
        sub(s2[3], s2[12]),
        sub(s2[2], s2[13]),
        sub(s2[1], s2[14]),
        sub(s2[0], s2[15]),
    ]
}

/// libvpx's `iadst16_c`.
#[allow(
    clippy::many_single_char_names,
    clippy::similar_names,
    reason = "the variable names are libvpx's, kept so the port reads against it"
)]
fn iadst16(input: &[i32; 16]) -> [i32; 16] {
    let x0 = input[15];
    let x1 = input[0];
    let x2 = input[13];
    let x3 = input[2];
    let x4 = input[11];
    let x5 = input[4];
    let x6 = input[9];
    let x7 = input[6];
    let x8 = input[7];
    let x9 = input[8];
    let x10 = input[5];
    let x11 = input[10];
    let x12 = input[3];
    let x13 = input[12];
    let x14 = input[1];
    let x15 = input[14];
    if input.iter().all(|&v| v == 0) {
        return [0; 16];
    }

    // Stage 1.
    let s0 = mul(x0, C1) + mul(x1, C31);
    let s1 = mul(x0, C31) - mul(x1, C1);
    let s2 = mul(x2, C5) + mul(x3, C27);
    let s3 = mul(x2, C27) - mul(x3, C5);
    let s4 = mul(x4, C9) + mul(x5, C23);
    let s5 = mul(x4, C23) - mul(x5, C9);
    let s6 = mul(x6, C13) + mul(x7, C19);
    let s7 = mul(x6, C19) - mul(x7, C13);
    let s8 = mul(x8, C17) + mul(x9, C15);
    let s9 = mul(x8, C15) - mul(x9, C17);
    let s10 = mul(x10, C21) + mul(x11, C11);
    let s11 = mul(x10, C11) - mul(x11, C21);
    let s12 = mul(x12, C25) + mul(x13, C7);
    let s13 = mul(x12, C7) - mul(x13, C25);
    let s14 = mul(x14, C29) + mul(x15, C3);
    let s15 = mul(x14, C3) - mul(x15, C29);

    let x0 = rs(s0 + s8);
    let x1 = rs(s1 + s9);
    let x2 = rs(s2 + s10);
    let x3 = rs(s3 + s11);
    let x4 = rs(s4 + s12);
    let x5 = rs(s5 + s13);
    let x6 = rs(s6 + s14);
    let x7 = rs(s7 + s15);
    let x8 = rs(s0 - s8);
    let x9 = rs(s1 - s9);
    let x10 = rs(s2 - s10);
    let x11 = rs(s3 - s11);
    let x12 = rs(s4 - s12);
    let x13 = rs(s5 - s13);
    let x14 = rs(s6 - s14);
    let x15 = rs(s7 - s15);

    // Stage 2.
    let s8 = mul(x8, C4) + mul(x9, C28);
    let s9 = mul(x8, C28) - mul(x9, C4);
    let s10 = mul(x10, C20) + mul(x11, C12);
    let s11 = mul(x10, C12) - mul(x11, C20);
    let s12 = mul(neg(x12), C28) + mul(x13, C4);
    let s13 = mul(x12, C4) + mul(x13, C28);
    let s14 = mul(neg(x14), C12) + mul(x15, C20);
    let s15 = mul(x14, C20) + mul(x15, C12);

    let (y0, y1, y2, y3) = (add(x0, x4), add(x1, x5), add(x2, x6), add(x3, x7));
    let (y4, y5, y6, y7) = (sub(x0, x4), sub(x1, x5), sub(x2, x6), sub(x3, x7));
    let y8 = rs(s8 + s12);
    let y9 = rs(s9 + s13);
    let y10 = rs(s10 + s14);
    let y11 = rs(s11 + s15);
    let y12 = rs(s8 - s12);
    let y13 = rs(s9 - s13);
    let y14 = rs(s10 - s14);
    let y15 = rs(s11 - s15);

    // Stage 3.
    let s4 = mul(y4, C8) + mul(y5, C24);
    let s5 = mul(y4, C24) - mul(y5, C8);
    let s6 = mul(neg(y6), C24) + mul(y7, C8);
    let s7 = mul(y6, C8) + mul(y7, C24);
    let s12 = mul(y12, C8) + mul(y13, C24);
    let s13 = mul(y12, C24) - mul(y13, C8);
    let s14 = mul(neg(y14), C24) + mul(y15, C8);
    let s15 = mul(y14, C8) + mul(y15, C24);

    let z0 = add(y0, y2);
    let z1 = add(y1, y3);
    let z2 = sub(y0, y2);
    let z3 = sub(y1, y3);
    let z4 = rs(s4 + s6);
    let z5 = rs(s5 + s7);
    let z6 = rs(s4 - s6);
    let z7 = rs(s5 - s7);
    let z8 = add(y8, y10);
    let z9 = add(y9, y11);
    let z10 = sub(y8, y10);
    let z11 = sub(y9, y11);
    let z12 = rs(s12 + s14);
    let z13 = rs(s13 + s15);
    let z14 = rs(s12 - s14);
    let z15 = rs(s13 - s15);

    // Stage 4.
    let w2 = rs(mul(add(z2, z3), -C16));
    let w3 = rs(mul(sub(z2, z3), C16));
    let w6 = rs(mul(add(z6, z7), C16));
    let w7 = rs(mul(add(neg(z6), z7), C16));
    let w10 = rs(mul(add(z10, z11), C16));
    let w11 = rs(mul(add(neg(z10), z11), C16));
    let w14 = rs(mul(add(z14, z15), -C16));
    let w15 = rs(mul(sub(z14, z15), C16));

    [
        z0,
        neg(z8),
        z12,
        neg(z4),
        w6,
        w14,
        w10,
        w2,
        w3,
        w11,
        w15,
        w7,
        z5,
        neg(z13),
        z9,
        neg(z1),
    ]
}

// --- 32 points -------------------------------------------------------------------------

/// libvpx's `idct32_c`.
#[allow(
    clippy::indexing_slicing,
    clippy::similar_names,
    reason = "the step arrays are libvpx's own, kept so the port reads against it; the final loop's k is below 16, so 31 - k is in range"
)]
fn idct32(input: &[i32; 32]) -> [i32; 32] {
    let mut s1 = [0i32; 32];
    let mut s2 = [0i32; 32];

    // Stage 1.
    s1[0] = input[0];
    s1[1] = input[16];
    s1[2] = input[8];
    s1[3] = input[24];
    s1[4] = input[4];
    s1[5] = input[20];
    s1[6] = input[12];
    s1[7] = input[28];
    s1[8] = input[2];
    s1[9] = input[18];
    s1[10] = input[10];
    s1[11] = input[26];
    s1[12] = input[6];
    s1[13] = input[22];
    s1[14] = input[14];
    s1[15] = input[30];
    s1[16] = rs(mul(input[1], C31) - mul(input[31], C1));
    s1[31] = rs(mul(input[1], C1) + mul(input[31], C31));
    s1[17] = rs(mul(input[17], C15) - mul(input[15], C17));
    s1[30] = rs(mul(input[17], C17) + mul(input[15], C15));
    s1[18] = rs(mul(input[9], C23) - mul(input[23], C9));
    s1[29] = rs(mul(input[9], C9) + mul(input[23], C23));
    s1[19] = rs(mul(input[25], C7) - mul(input[7], C25));
    s1[28] = rs(mul(input[25], C25) + mul(input[7], C7));
    s1[20] = rs(mul(input[5], C27) - mul(input[27], C5));
    s1[27] = rs(mul(input[5], C5) + mul(input[27], C27));
    s1[21] = rs(mul(input[21], C11) - mul(input[11], C21));
    s1[26] = rs(mul(input[21], C21) + mul(input[11], C11));
    s1[22] = rs(mul(input[13], C19) - mul(input[19], C13));
    s1[25] = rs(mul(input[13], C13) + mul(input[19], C19));
    s1[23] = rs(mul(input[29], C3) - mul(input[3], C29));
    s1[24] = rs(mul(input[29], C29) + mul(input[3], C3));

    // Stage 2.
    s2[0] = s1[0];
    s2[1] = s1[1];
    s2[2] = s1[2];
    s2[3] = s1[3];
    s2[4] = s1[4];
    s2[5] = s1[5];
    s2[6] = s1[6];
    s2[7] = s1[7];
    s2[8] = rs(mul(s1[8], C30) - mul(s1[15], C2));
    s2[15] = rs(mul(s1[8], C2) + mul(s1[15], C30));
    s2[9] = rs(mul(s1[9], C14) - mul(s1[14], C18));
    s2[14] = rs(mul(s1[9], C18) + mul(s1[14], C14));
    s2[10] = rs(mul(s1[10], C22) - mul(s1[13], C10));
    s2[13] = rs(mul(s1[10], C10) + mul(s1[13], C22));
    s2[11] = rs(mul(s1[11], C6) - mul(s1[12], C26));
    s2[12] = rs(mul(s1[11], C26) + mul(s1[12], C6));
    s2[16] = add(s1[16], s1[17]);
    s2[17] = sub(s1[16], s1[17]);
    s2[18] = add(neg(s1[18]), s1[19]);
    s2[19] = add(s1[18], s1[19]);
    s2[20] = add(s1[20], s1[21]);
    s2[21] = sub(s1[20], s1[21]);
    s2[22] = add(neg(s1[22]), s1[23]);
    s2[23] = add(s1[22], s1[23]);
    s2[24] = add(s1[24], s1[25]);
    s2[25] = sub(s1[24], s1[25]);
    s2[26] = add(neg(s1[26]), s1[27]);
    s2[27] = add(s1[26], s1[27]);
    s2[28] = add(s1[28], s1[29]);
    s2[29] = sub(s1[28], s1[29]);
    s2[30] = add(neg(s1[30]), s1[31]);
    s2[31] = add(s1[30], s1[31]);

    // Stage 3.
    s1[0] = s2[0];
    s1[1] = s2[1];
    s1[2] = s2[2];
    s1[3] = s2[3];
    s1[4] = rs(mul(s2[4], C28) - mul(s2[7], C4));
    s1[7] = rs(mul(s2[4], C4) + mul(s2[7], C28));
    s1[5] = rs(mul(s2[5], C12) - mul(s2[6], C20));
    s1[6] = rs(mul(s2[5], C20) + mul(s2[6], C12));
    s1[8] = add(s2[8], s2[9]);
    s1[9] = sub(s2[8], s2[9]);
    s1[10] = add(neg(s2[10]), s2[11]);
    s1[11] = add(s2[10], s2[11]);
    s1[12] = add(s2[12], s2[13]);
    s1[13] = sub(s2[12], s2[13]);
    s1[14] = add(neg(s2[14]), s2[15]);
    s1[15] = add(s2[14], s2[15]);
    s1[16] = s2[16];
    s1[31] = s2[31];
    s1[17] = rs(mul(neg(s2[17]), C4) + mul(s2[30], C28));
    s1[30] = rs(mul(s2[17], C28) + mul(s2[30], C4));
    s1[18] = rs(mul(neg(s2[18]), C28) - mul(s2[29], C4));
    s1[29] = rs(mul(neg(s2[18]), C4) + mul(s2[29], C28));
    s1[19] = s2[19];
    s1[20] = s2[20];
    s1[21] = rs(mul(neg(s2[21]), C20) + mul(s2[26], C12));
    s1[26] = rs(mul(s2[21], C12) + mul(s2[26], C20));
    s1[22] = rs(mul(neg(s2[22]), C12) - mul(s2[25], C20));
    s1[25] = rs(mul(neg(s2[22]), C20) + mul(s2[25], C12));
    s1[23] = s2[23];
    s1[24] = s2[24];
    s1[27] = s2[27];
    s1[28] = s2[28];

    // Stage 4.
    s2[0] = rs(mul(add(s1[0], s1[1]), C16));
    s2[1] = rs(mul(sub(s1[0], s1[1]), C16));
    s2[2] = rs(mul(s1[2], C24) - mul(s1[3], C8));
    s2[3] = rs(mul(s1[2], C8) + mul(s1[3], C24));
    s2[4] = add(s1[4], s1[5]);
    s2[5] = sub(s1[4], s1[5]);
    s2[6] = add(neg(s1[6]), s1[7]);
    s2[7] = add(s1[6], s1[7]);
    s2[8] = s1[8];
    s2[15] = s1[15];
    s2[9] = rs(mul(neg(s1[9]), C8) + mul(s1[14], C24));
    s2[14] = rs(mul(s1[9], C24) + mul(s1[14], C8));
    s2[10] = rs(mul(neg(s1[10]), C24) - mul(s1[13], C8));
    s2[13] = rs(mul(neg(s1[10]), C8) + mul(s1[13], C24));
    s2[11] = s1[11];
    s2[12] = s1[12];
    s2[16] = add(s1[16], s1[19]);
    s2[17] = add(s1[17], s1[18]);
    s2[18] = sub(s1[17], s1[18]);
    s2[19] = sub(s1[16], s1[19]);
    s2[20] = add(neg(s1[20]), s1[23]);
    s2[21] = add(neg(s1[21]), s1[22]);
    s2[22] = add(s1[21], s1[22]);
    s2[23] = add(s1[20], s1[23]);
    s2[24] = add(s1[24], s1[27]);
    s2[25] = add(s1[25], s1[26]);
    s2[26] = sub(s1[25], s1[26]);
    s2[27] = sub(s1[24], s1[27]);
    s2[28] = add(neg(s1[28]), s1[31]);
    s2[29] = add(neg(s1[29]), s1[30]);
    s2[30] = add(s1[29], s1[30]);
    s2[31] = add(s1[28], s1[31]);

    // Stage 5.
    s1[0] = add(s2[0], s2[3]);
    s1[1] = add(s2[1], s2[2]);
    s1[2] = sub(s2[1], s2[2]);
    s1[3] = sub(s2[0], s2[3]);
    s1[4] = s2[4];
    s1[5] = rs(mul(sub(s2[6], s2[5]), C16));
    s1[6] = rs(mul(add(s2[5], s2[6]), C16));
    s1[7] = s2[7];
    s1[8] = add(s2[8], s2[11]);
    s1[9] = add(s2[9], s2[10]);
    s1[10] = sub(s2[9], s2[10]);
    s1[11] = sub(s2[8], s2[11]);
    s1[12] = add(neg(s2[12]), s2[15]);
    s1[13] = add(neg(s2[13]), s2[14]);
    s1[14] = add(s2[13], s2[14]);
    s1[15] = add(s2[12], s2[15]);
    s1[16] = s2[16];
    s1[17] = s2[17];
    s1[18] = rs(mul(neg(s2[18]), C8) + mul(s2[29], C24));
    s1[29] = rs(mul(s2[18], C24) + mul(s2[29], C8));
    s1[19] = rs(mul(neg(s2[19]), C8) + mul(s2[28], C24));
    s1[28] = rs(mul(s2[19], C24) + mul(s2[28], C8));
    s1[20] = rs(mul(neg(s2[20]), C24) - mul(s2[27], C8));
    s1[27] = rs(mul(neg(s2[20]), C8) + mul(s2[27], C24));
    s1[21] = rs(mul(neg(s2[21]), C24) - mul(s2[26], C8));
    s1[26] = rs(mul(neg(s2[21]), C8) + mul(s2[26], C24));
    s1[22] = s2[22];
    s1[23] = s2[23];
    s1[24] = s2[24];
    s1[25] = s2[25];
    s1[30] = s2[30];
    s1[31] = s2[31];

    // Stage 6.
    s2[0] = add(s1[0], s1[7]);
    s2[1] = add(s1[1], s1[6]);
    s2[2] = add(s1[2], s1[5]);
    s2[3] = add(s1[3], s1[4]);
    s2[4] = sub(s1[3], s1[4]);
    s2[5] = sub(s1[2], s1[5]);
    s2[6] = sub(s1[1], s1[6]);
    s2[7] = sub(s1[0], s1[7]);
    s2[8] = s1[8];
    s2[9] = s1[9];
    s2[10] = rs(mul(add(neg(s1[10]), s1[13]), C16));
    s2[13] = rs(mul(add(s1[10], s1[13]), C16));
    s2[11] = rs(mul(add(neg(s1[11]), s1[12]), C16));
    s2[12] = rs(mul(add(s1[11], s1[12]), C16));
    s2[14] = s1[14];
    s2[15] = s1[15];
    s2[16] = add(s1[16], s1[23]);
    s2[17] = add(s1[17], s1[22]);
    s2[18] = add(s1[18], s1[21]);
    s2[19] = add(s1[19], s1[20]);
    s2[20] = sub(s1[19], s1[20]);
    s2[21] = sub(s1[18], s1[21]);
    s2[22] = sub(s1[17], s1[22]);
    s2[23] = sub(s1[16], s1[23]);
    s2[24] = add(neg(s1[24]), s1[31]);
    s2[25] = add(neg(s1[25]), s1[30]);
    s2[26] = add(neg(s1[26]), s1[29]);
    s2[27] = add(neg(s1[27]), s1[28]);
    s2[28] = add(s1[27], s1[28]);
    s2[29] = add(s1[26], s1[29]);
    s2[30] = add(s1[25], s1[30]);
    s2[31] = add(s1[24], s1[31]);

    // Stage 7.
    s1[0] = add(s2[0], s2[15]);
    s1[1] = add(s2[1], s2[14]);
    s1[2] = add(s2[2], s2[13]);
    s1[3] = add(s2[3], s2[12]);
    s1[4] = add(s2[4], s2[11]);
    s1[5] = add(s2[5], s2[10]);
    s1[6] = add(s2[6], s2[9]);
    s1[7] = add(s2[7], s2[8]);
    s1[8] = sub(s2[7], s2[8]);
    s1[9] = sub(s2[6], s2[9]);
    s1[10] = sub(s2[5], s2[10]);
    s1[11] = sub(s2[4], s2[11]);
    s1[12] = sub(s2[3], s2[12]);
    s1[13] = sub(s2[2], s2[13]);
    s1[14] = sub(s2[1], s2[14]);
    s1[15] = sub(s2[0], s2[15]);
    s1[16] = s2[16];
    s1[17] = s2[17];
    s1[18] = s2[18];
    s1[19] = s2[19];
    s1[20] = rs(mul(add(neg(s2[20]), s2[27]), C16));
    s1[27] = rs(mul(add(s2[20], s2[27]), C16));
    s1[21] = rs(mul(add(neg(s2[21]), s2[26]), C16));
    s1[26] = rs(mul(add(s2[21], s2[26]), C16));
    s1[22] = rs(mul(add(neg(s2[22]), s2[25]), C16));
    s1[25] = rs(mul(add(s2[22], s2[25]), C16));
    s1[23] = rs(mul(add(neg(s2[23]), s2[24]), C16));
    s1[24] = rs(mul(add(s2[23], s2[24]), C16));
    s1[28] = s2[28];
    s1[29] = s2[29];
    s1[30] = s2[30];
    s1[31] = s2[31];

    // The final stage: each output is the sum or difference of one pair.
    let mut out = [0i32; 32];
    for k in 0..16 {
        out[k] = add(s1[k], s1[31 - k]);
        out[31 - k] = sub(s1[k], s1[31 - k]);
    }
    out
}

// --- Two dimensions ----------------------------------------------------------------------

/// The `N`-point transforms a block of `N` x `N` runs, as libvpx's
/// `transform_2d` holds them: `cols` vertically, `rows` horizontally.
struct Pair<const N: usize> {
    cols: fn(&[i32; N]) -> [i32; N],
    rows: fn(&[i32; N]) -> [i32; N],
}

/// The pair a transform type names at 4 points: libvpx's `IHT_4`.
fn pair4(tx_type: TxType) -> Pair<4> {
    match tx_type {
        ADST_DCT => Pair {
            cols: iadst4,
            rows: idct4,
        },
        DCT_ADST => Pair {
            cols: idct4,
            rows: iadst4,
        },
        ADST_ADST => Pair {
            cols: iadst4,
            rows: iadst4,
        },
        _ => Pair {
            cols: idct4,
            rows: idct4,
        },
    }
}

/// libvpx's `IHT_8`.
fn pair8(tx_type: TxType) -> Pair<8> {
    match tx_type {
        ADST_DCT => Pair {
            cols: iadst8,
            rows: idct8,
        },
        DCT_ADST => Pair {
            cols: idct8,
            rows: iadst8,
        },
        ADST_ADST => Pair {
            cols: iadst8,
            rows: iadst8,
        },
        _ => Pair {
            cols: idct8,
            rows: idct8,
        },
    }
}

/// libvpx's `IHT_16`.
fn pair16(tx_type: TxType) -> Pair<16> {
    match tx_type {
        ADST_DCT => Pair {
            cols: iadst16,
            rows: idct16,
        },
        DCT_ADST => Pair {
            cols: idct16,
            rows: iadst16,
        },
        ADST_ADST => Pair {
            cols: iadst16,
            rows: iadst16,
        },
        _ => Pair {
            cols: idct16,
            rows: idct16,
        },
    }
}

/// Clip `pixel + delta` to `0..=max`: libvpx's `clip_pixel_add` and
/// `highbd_clip_pixel_add`.
#[inline(always)]
fn clip_add<P: Pixel>(pixel: &mut P, delta: i32, max: i32) {
    *pixel = P::from_int(pixel.int().wrapping_add(delta).clamp(0, max));
}

/// Run `pair` over the `N` x `N` coefficients (row-major) and add the result,
/// rounded by `shift` bits, to the block at the start of `dst`.
#[allow(
    clippy::indexing_slicing,
    reason = "every index is a loop position below N into an N-element array"
)]
fn transform_add<P: Pixel, const N: usize>(
    coeffs: &[i32],
    pair: &Pair<N>,
    shift: u32,
    dst: &mut [P],
    stride: usize,
    max: i32,
) {
    let mut tmp = [[0i32; N]; N];
    let mut any = false;
    for (row, src) in tmp.iter_mut().zip(coeffs.chunks_exact(N)) {
        // A row of zeros transforms to zeros, ADST and DCT alike.
        if src.iter().any(|&c| c != 0) {
            let mut input = [0i32; N];
            input.copy_from_slice(src);
            *row = (pair.rows)(&input);
            any = true;
        }
    }
    if !any {
        return;
    }
    let mut out = [[0i32; N]; N];
    for c in 0..N {
        let mut col = [0i32; N];
        for r in 0..N {
            col[r] = tmp[r][c];
        }
        let res = (pair.cols)(&col);
        for r in 0..N {
            out[r][c] = res[r];
        }
    }
    for (line, res) in dst.chunks_mut(stride).zip(&out) {
        for (pixel, &v) in line.iter_mut().zip(res) {
            clip_add(pixel, round_shift(v, shift), max);
        }
    }
}

/// Add `delta` to every pixel of the `n` x `n` block at the start of `dst`.
fn add_constant<P: Pixel>(dst: &mut [P], stride: usize, n: usize, delta: i32, max: i32) {
    for line in dst.chunks_mut(stride).take(n) {
        for pixel in line.iter_mut().take(n) {
            clip_add(pixel, delta, max);
        }
    }
}

/// The DC-only 2-D DCT of `n` points: libvpx's `vpx_idct*_1_add`, which is
/// the full transform's result when only the DC coefficient is nonzero.
fn dc_only<P: Pixel>(dc: i32, shift: u32, dst: &mut [P], stride: usize, n: usize, max: i32) {
    let out = rs(mul(dc, C16));
    let out = rs(mul(out, C16));
    add_constant(dst, stride, n, round_shift(out, shift), max);
}

/// The lossless 4x4 Walsh-Hadamard transform: libvpx's
/// `vpx_iwht4x4_16_add_c` (its DC-only `_1` version computes the same).
#[allow(
    clippy::indexing_slicing,
    clippy::many_single_char_names,
    reason = "fixed 4x4 arrays indexed by loop positions below 4; the names are libvpx's"
)]
fn iwht4x4_add<P: Pixel>(coeffs: &[i32], dst: &mut [P], stride: usize, max: i32) {
    let mut tmp = [[0i32; 4]; 4];
    for (row, ip) in tmp.iter_mut().zip(coeffs.chunks_exact(4)) {
        let mut a = ip[0] >> UNIT_QUANT_SHIFT;
        let mut c = ip[1] >> UNIT_QUANT_SHIFT;
        let mut d = ip[2] >> UNIT_QUANT_SHIFT;
        let mut b = ip[3] >> UNIT_QUANT_SHIFT;
        a = add(a, c);
        d = sub(d, b);
        let e = sub(a, d) >> 1;
        b = sub(e, b);
        c = sub(e, c);
        a = sub(a, b);
        d = add(d, c);
        *row = [a, b, c, d];
    }
    for i in 0..4 {
        let mut a = tmp[0][i];
        let mut c = tmp[1][i];
        let mut d = tmp[2][i];
        let mut b = tmp[3][i];
        a = add(a, c);
        d = sub(d, b);
        let e = sub(a, d) >> 1;
        b = sub(e, b);
        c = sub(e, c);
        a = sub(a, b);
        d = add(d, c);
        for (line, v) in dst.chunks_mut(stride).zip([a, b, c, d]) {
            if let Some(pixel) = line.get_mut(i) {
                clip_add(pixel, v, max);
            }
        }
    }
}

/// Inverse-transform a block's coefficients and add them to its prediction:
/// libvpx's `inverse_transform_block_inter` and `_intra`.
///
/// `coeffs` holds the dequantised coefficients in raster order, `n` x `n` of
/// them for a transform of `n` points; `eob` is how many the scan decoded,
/// at least 1. `dst` starts at the block's top-left pixel; rows are `stride`
/// apart. `lossless` selects the Walsh-Hadamard transform, which only 4x4
/// blocks use. 32x32 blocks are always DCT both ways.
pub fn inverse_transform_add<P: Pixel>(
    tx_size: TxSize,
    tx_type: TxType,
    lossless: bool,
    eob: usize,
    coeffs: &[i32],
    dst: &mut [P],
    stride: usize,
    bit_depth: u8,
) {
    let max = (1i32 << bit_depth.clamp(8, 12)) - 1;
    if lossless {
        iwht4x4_add(coeffs, dst, stride, max);
        return;
    }
    let dc = coeffs.first().copied().unwrap_or(0);
    // A DC-only DCT is a constant: libvpx's eob == 1 shortcuts.
    let dct_dc_only = eob <= 1 && (tx_type == DCT_DCT || tx_size > TX_16X16);
    match tx_size {
        TX_4X4 => {
            if dct_dc_only {
                dc_only(dc, 4, dst, stride, 4, max);
            } else {
                transform_add(coeffs, &pair4(tx_type), 4, dst, stride, max);
            }
        }
        TX_8X8 => {
            if dct_dc_only {
                dc_only(dc, 5, dst, stride, 8, max);
            } else {
                transform_add(coeffs, &pair8(tx_type), 5, dst, stride, max);
            }
        }
        TX_16X16 => {
            if dct_dc_only {
                dc_only(dc, 6, dst, stride, 16, max);
            } else {
                transform_add(coeffs, &pair16(tx_type), 6, dst, stride, max);
            }
        }
        _ => {
            if dct_dc_only {
                dc_only(dc, 6, dst, stride, 32, max);
            } else {
                let pair = Pair::<32> {
                    cols: idct32,
                    rows: idct32,
                };
                transform_add(coeffs, &pair, 6, dst, stride, max);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::cast_possible_truncation
    )]

    use super::*;

    /// A slow, independent inverse DCT in floating point, to check the
    /// integer one against: the transforms are scaled so that the 2-D
    /// result is the textbook inverse DCT times 1 (4x4 shifted by 4,
    /// 8x8 by 5, and so on).
    fn float_idct_2d(coeffs: &[i32], n: usize) -> Vec<f64> {
        let pi = std::f64::consts::PI;
        let c = |k: usize| if k == 0 { (0.5f64).sqrt() } else { 1.0 };
        let mut out = vec![0.0; n * n];
        for y in 0..n {
            for x in 0..n {
                let mut s = 0.0;
                for v in 0..n {
                    for u in 0..n {
                        let coef = f64::from(coeffs[v * n + u]);
                        s += c(u)
                            * c(v)
                            * coef
                            * ((2 * x + 1) as f64 * u as f64 * pi / (2 * n) as f64).cos()
                            * ((2 * y + 1) as f64 * v as f64 * pi / (2 * n) as f64).cos();
                    }
                }
                out[y * n + x] = s;
            }
        }
        out
    }

    fn run(tx: TxSize, tx_type: TxType, coeffs: &[i32]) -> Vec<i32> {
        let n = 4 << tx;
        // Mid-grey at 12 bits: room both ways for the residual.
        let mut dst = vec![2048u16; n * n];
        inverse_transform_add(tx, tx_type, false, 2, coeffs, &mut dst, n, 12);
        dst.iter().map(|&p| i32::from(p) - 2048).collect()
    }

    #[test]
    fn every_dct_size_matches_a_float_reference_closely() {
        // The integer DCT approximates the float one; the residuals agree to
        // within a small rounding error, which checks every butterfly's sign
        // and constant at once.
        for tx in [TX_4X4, TX_8X8, TX_16X16, 3] {
            let n = 4usize << tx;
            // libvpx's 1-D inverse DCTs are unnormalised -- the DC basis
            // vector is 1/sqrt(2), the rest cosines of amplitude 1 -- and the
            // 2-D result is rounded down by the size's shift.
            let shift = [4, 5, 6, 6][usize::from(tx)];
            let gain = f64::from(1u32 << shift);
            let mut seed = 0x1234_5678u32;
            for _ in 0..8 {
                let coeffs: Vec<i32> = (0..n * n)
                    .map(|_| {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        ((seed >> 16) % 201) as i32 - 100
                    })
                    .collect();
                let got = run(tx, DCT_DCT, &coeffs);
                let want = float_idct_2d(&coeffs, n);
                for (g, w) in got.iter().zip(&want) {
                    let w = w / gain;
                    assert!(
                        (f64::from(*g) - w).abs() <= 2.0,
                        "{n}x{n}: got {g}, float {w}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_dc_only_block_takes_the_shortcut_libvpx_takes() {
        // vpx_idct8x8_1_add_c: out = rs(1000 * 11585); out = rs(out * 11585);
        // a1 = round(out, 5).
        let out = rs(mul(rs(mul(1000, C16)), C16));
        let a1 = round_shift(out, 5);
        let mut coeffs = vec![0i32; 64];
        coeffs[0] = 1000;
        let mut dst = vec![100u8; 64];
        inverse_transform_add(TX_8X8, DCT_DCT, false, 1, &coeffs, &mut dst, 8, 8);
        assert!(dst.iter().all(|&p| i32::from(p) == 100 + a1));
        // ...and the full transform gives the same.
        let mut full = vec![100u8; 64];
        inverse_transform_add(TX_8X8, DCT_DCT, false, 2, &coeffs, &mut full, 8, 8);
        assert_eq!(dst, full);
    }

    #[test]
    fn the_shortcut_and_the_full_transform_agree_at_every_size() {
        for tx in [TX_4X4, TX_8X8, TX_16X16, 3] {
            let n = 4usize << tx;
            for dc in [-5000, -77, 1, 63, 4095] {
                let mut coeffs = vec![0i32; n * n];
                coeffs[0] = dc;
                let mut a = vec![512u16; n * n];
                let mut b = a.clone();
                inverse_transform_add(tx, DCT_DCT, false, 1, &coeffs, &mut a, n, 10);
                inverse_transform_add(tx, DCT_DCT, false, 9, &coeffs, &mut b, n, 10);
                assert_eq!(a, b, "{n}x{n}, dc {dc}");
            }
        }
    }

    #[test]
    fn an_adst_differs_from_a_dct_and_each_is_its_own() {
        let mut coeffs = vec![0i32; 16];
        coeffs[1] = 300;
        coeffs[4] = -200;
        let dct = run(TX_4X4, DCT_DCT, &coeffs);
        let adst_dct = run(TX_4X4, ADST_DCT, &coeffs);
        let dct_adst = run(TX_4X4, DCT_ADST, &coeffs);
        assert_ne!(dct, adst_dct);
        assert_ne!(adst_dct, dct_adst);
    }

    #[test]
    fn iadst4_matches_libvpx_by_hand() {
        // iadst4_c with input [100, 0, 0, 0]:
        // s0 = 5283*100, s1 = 9929*100, s7 = 100, s2 = 13377*100, s3 = 0.
        let got = iadst4(&[100, 0, 0, 0]);
        assert_eq!(
            got,
            [
                rs(528_300),
                rs(992_900),
                rs(1_337_700),
                rs(528_300 + 992_900)
            ]
        );
    }

    #[test]
    fn lossless_round_trips_libvpx_walsh_hadamard() {
        // vpx_fwht4x4 of a known block, then the inverse here, must give the
        // block back exactly: the lossless transform is lossless.
        let block: [i32; 16] = [5, -3, 0, 7, 2, 2, 2, 2, -8, 1, 0, 4, 9, -9, 3, 3];
        let coeffs = fwht4x4(&block);
        let mut dst = vec![100u8; 16];
        inverse_transform_add(TX_4X4, DCT_DCT, true, 16, &coeffs, &mut dst, 4, 8);
        let got: Vec<i32> = dst.iter().map(|&p| i32::from(p) - 100).collect();
        assert_eq!(got, block);
    }

    /// libvpx's `vp9_fwht4x4_c` (`vp9/encoder/vp9_dct.c`), the lossless
    /// forward transform, to round-trip against: columns first, written as
    /// columns, then rows.
    fn fwht4x4(input: &[i32; 16]) -> Vec<i32> {
        let mut out = vec![0i32; 16];
        for i in 0..4 {
            let (mut a1, mut b1, mut c1, mut d1) =
                (input[i], input[4 + i], input[8 + i], input[12 + i]);
            a1 += b1;
            d1 -= c1;
            let e1 = (a1 - d1) >> 1;
            b1 = e1 - b1;
            c1 = e1 - c1;
            a1 -= c1;
            d1 += b1;
            out[i] = a1;
            out[4 + i] = c1;
            out[8 + i] = d1;
            out[12 + i] = b1;
        }
        for i in 0..4 {
            let (mut a1, mut b1, mut c1, mut d1) =
                (out[4 * i], out[4 * i + 1], out[4 * i + 2], out[4 * i + 3]);
            a1 += b1;
            d1 -= c1;
            let e1 = (a1 - d1) >> 1;
            b1 = e1 - b1;
            c1 = e1 - c1;
            a1 -= c1;
            d1 += b1;
            out[4 * i] = a1 * 4;
            out[4 * i + 1] = c1 * 4;
            out[4 * i + 2] = d1 * 4;
            out[4 * i + 3] = b1 * 4;
        }
        out
    }

    /// libvpx's results on the seeded blocks: `tools/idct_reference.c`'s
    /// output, as (bit depth, transform size, transform type, sparse, hash).
    const LIBVPX_BLOCKS: [(u8, u8, u8, bool, u64); 78] = [
        (8, 0, 0, false, 0xf687b631af318272u64),
        (8, 0, 0, true, 0xdf2e031033b631f4u64),
        (8, 0, 1, false, 0x70134b37d3735f3bu64),
        (8, 0, 1, true, 0x31d70ef72fad0482u64),
        (8, 0, 2, false, 0xb58827730327a720u64),
        (8, 0, 2, true, 0x2e0684b568ac3d5eu64),
        (8, 0, 3, false, 0x4e9843c7565c9485u64),
        (8, 0, 3, true, 0xc424bc87045feeebu64),
        (8, 1, 0, false, 0xe0a41feab5c3b73bu64),
        (8, 1, 0, true, 0xe535eaa43312c593u64),
        (8, 1, 1, false, 0x8a1ec8d142c98e26u64),
        (8, 1, 1, true, 0x5a0ed44f282b8135u64),
        (8, 1, 2, false, 0x5830f9b223d0c44du64),
        (8, 1, 2, true, 0x981dbc4c44bc3639u64),
        (8, 1, 3, false, 0x22e587c064b58142u64),
        (8, 1, 3, true, 0x09f37e931bebda09u64),
        (8, 2, 0, false, 0x17b4fb46380da7deu64),
        (8, 2, 0, true, 0x953b9e5b86915de4u64),
        (8, 2, 1, false, 0xee19c07813fb2f92u64),
        (8, 2, 1, true, 0xe81e8ed68c3e3223u64),
        (8, 2, 2, false, 0x9174fc6e2be37832u64),
        (8, 2, 2, true, 0xaab018d06b9a17ffu64),
        (8, 2, 3, false, 0xbc6e70e8b18d5ef7u64),
        (8, 2, 3, true, 0x8562823e1ac522b3u64),
        (8, 3, 0, false, 0x66a2e6132dcee270u64),
        (8, 3, 0, true, 0x1af8111578328bd2u64),
        (10, 0, 0, false, 0x4c1e9772b1143401u64),
        (10, 0, 0, true, 0xbc5b2a973cea1382u64),
        (10, 0, 1, false, 0x1b20222ea0943468u64),
        (10, 0, 1, true, 0x4ae2c4cdafc5c53cu64),
        (10, 0, 2, false, 0x727ac0adc7db7974u64),
        (10, 0, 2, true, 0x83148bbb8e4a5f10u64),
        (10, 0, 3, false, 0x844a8149529a4e5du64),
        (10, 0, 3, true, 0x135d8ebb4c7ebfa9u64),
        (10, 1, 0, false, 0x514bbbd5a69ae31bu64),
        (10, 1, 0, true, 0xaac9586ff7884bd0u64),
        (10, 1, 1, false, 0xdbd739f530cb3772u64),
        (10, 1, 1, true, 0x63a650d645a7c171u64),
        (10, 1, 2, false, 0x8fbab90a27f50e1au64),
        (10, 1, 2, true, 0xee50537d32e8f284u64),
        (10, 1, 3, false, 0x2cfe8b216d9ee526u64),
        (10, 1, 3, true, 0x10e0bddf177c8ac3u64),
        (10, 2, 0, false, 0x81e505a1702e5ed6u64),
        (10, 2, 0, true, 0x57ebedf38f87fed9u64),
        (10, 2, 1, false, 0x374dcde9f9809af9u64),
        (10, 2, 1, true, 0x89c24b4cba843391u64),
        (10, 2, 2, false, 0x661c7f5045b7dd2eu64),
        (10, 2, 2, true, 0x83a0cd1aecc688eeu64),
        (10, 2, 3, false, 0x59ee901569f525e0u64),
        (10, 2, 3, true, 0xef198ffd78954c4eu64),
        (10, 3, 0, false, 0xb00c881da089c3fdu64),
        (10, 3, 0, true, 0x3fd5cfca0df94453u64),
        (12, 0, 0, false, 0x6f9bb2d9a0f3f732u64),
        (12, 0, 0, true, 0xdf5557659fccbb6fu64),
        (12, 0, 1, false, 0x8a4b94be53f5eaa5u64),
        (12, 0, 1, true, 0x6ceb55d8fe335ad0u64),
        (12, 0, 2, false, 0xe5c9b477e748050fu64),
        (12, 0, 2, true, 0x909815170e440c05u64),
        (12, 0, 3, false, 0x50d9617e98a2cde7u64),
        (12, 0, 3, true, 0x53df3f597e9c776eu64),
        (12, 1, 0, false, 0x1d7662bfdc526e11u64),
        (12, 1, 0, true, 0xc942a511f3baeee6u64),
        (12, 1, 1, false, 0x80da6abc22c50a8cu64),
        (12, 1, 1, true, 0x47d5a21b716557f2u64),
        (12, 1, 2, false, 0x2798af2abbd95852u64),
        (12, 1, 2, true, 0x56a351e1dd4d121fu64),
        (12, 1, 3, false, 0xb9b5a2adff4f562du64),
        (12, 1, 3, true, 0xbda07da77c923f9au64),
        (12, 2, 0, false, 0x4251eb041f282d4bu64),
        (12, 2, 0, true, 0xdb2d3cd44e639398u64),
        (12, 2, 1, false, 0xf3accb684a87de8du64),
        (12, 2, 1, true, 0x6f6ce75201c1fccau64),
        (12, 2, 2, false, 0xfd9fbc3e69f5b47au64),
        (12, 2, 2, true, 0x9350e7860b20bc4bu64),
        (12, 2, 3, false, 0xf4bc2800f047f9ebu64),
        (12, 2, 3, true, 0x174af32e801d363bu64),
        (12, 3, 0, false, 0xbb9349f5333f39b6u64),
        (12, 3, 0, true, 0x487c6c1f245fe5e2u64),
    ];

    /// libvpx's Walsh-Hadamard results, as (bit depth, hash).
    const LIBVPX_WHT: [(u8, u64); 3] = [
        (8, 0x0bc62d6821887a5au64),
        (10, 0x1d962a876395bd2du64),
        (12, 0x4970cac0160d5137u64),
    ];

    /// libvpx's generator in `tools/idct_reference.c`.
    struct Lcg(u32);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            self.0 >> 8
        }
        /// A value in `-bound..=bound`.
        fn coef(&mut self, bound: i32) -> i32 {
            (self.next() % (2 * bound + 1) as u32) as i32 - bound
        }
    }

    fn fnv(bytes: impl IntoIterator<Item = u8>, mut h: u64) -> u64 {
        for b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }

    fn default_scan(tx: TxSize) -> &'static [i16] {
        match tx {
            TX_4X4 => &crate::tables::DEFAULT_SCAN_4X4,
            TX_8X8 => &crate::tables::DEFAULT_SCAN_8X8,
            TX_16X16 => &crate::tables::DEFAULT_SCAN_16X16,
            _ => &crate::tables::DEFAULT_SCAN_32X32,
        }
    }

    #[test]
    fn matches_libvpx_on_seeded_blocks_at_every_size_type_and_depth() {
        // Replays `tools/idct_reference.c` call for call: the same blocks,
        // the same order, the same hash. Every transform, both the dense
        // case and libvpx's eob shortcuts, at 8, 10 and 12 bits.
        let bound8 = [2048, 512, 128, 32];
        let mut rng = Lcg(0x5eed_1234);
        let mut blocks = LIBVPX_BLOCKS.iter();
        let mut whts = LIBVPX_WHT.iter();
        for bd in [8u8, 10, 12] {
            for tx in [TX_4X4, TX_8X8, TX_16X16, 3] {
                let n = 4usize << tx;
                let types: &[TxType] = if tx == 3 { &[DCT_DCT] } else { &[0, 1, 2, 3] };
                for &tx_type in types {
                    for sparse in [false, true] {
                        let mut h = 0xcbf2_9ce4_8422_2325u64;
                        for _ in 0..64 {
                            let b = bound8[usize::from(tx)] << (bd - 8);
                            let mut coeffs = vec![0i32; n * n];
                            let mut eob = n * n;
                            if sparse {
                                let eobs = [1, 2, 10, 12, 34, 38, 135];
                                eob = eobs[(rng.next() % 7) as usize].min(n * n);
                                for &pos in &default_scan(tx)[..eob] {
                                    coeffs[pos as usize] = rng.coef(b);
                                }
                            } else {
                                for c in &mut coeffs {
                                    *c = rng.coef(b);
                                }
                            }
                            let mut d8 = vec![0u8; n * n];
                            let mut d16 = vec![0u16; n * n];
                            for (p8, p16) in d8.iter_mut().zip(&mut d16) {
                                let r = rng.next();
                                *p8 = r as u8;
                                *p16 = (r & ((1 << bd) - 1)) as u16;
                            }
                            if bd == 8 {
                                inverse_transform_add(
                                    tx, tx_type, false, eob, &coeffs, &mut d8, n, 8,
                                );
                                h = fnv(d8.iter().copied(), h);
                            } else {
                                inverse_transform_add(
                                    tx, tx_type, false, eob, &coeffs, &mut d16, n, bd,
                                );
                                h = fnv(d16.iter().flat_map(|v| v.to_le_bytes()), h);
                            }
                        }
                        let &(want_bd, want_tx, want_type, want_sparse, want) =
                            blocks.next().unwrap();
                        assert_eq!(
                            (want_bd, want_tx, want_type, want_sparse),
                            (bd, tx, tx_type, sparse)
                        );
                        assert_eq!(
                            h,
                            want,
                            "{bd}-bit {n}x{n}, type {tx_type}, {}",
                            if sparse { "sparse" } else { "dense" }
                        );
                    }
                }
            }
            // The Walsh-Hadamard transform: alternately full and DC-only.
            let mut h = 0xcbf2_9ce4_8422_2325u64;
            for k in 0..64 {
                let eob = if k & 1 == 1 { 1 } else { 16 };
                let mut coeffs = [0i32; 16];
                for &pos in &crate::tables::DEFAULT_SCAN_4X4[..eob] {
                    coeffs[pos as usize] = rng.coef(1020 << (bd - 8));
                }
                let mut d8 = [0u8; 16];
                let mut d16 = [0u16; 16];
                for (p8, p16) in d8.iter_mut().zip(&mut d16) {
                    let r = rng.next();
                    *p8 = r as u8;
                    *p16 = (r & ((1 << bd) - 1)) as u16;
                }
                if bd == 8 {
                    inverse_transform_add(TX_4X4, DCT_DCT, true, eob, &coeffs, &mut d8, 4, 8);
                    h = fnv(d8.iter().copied(), h);
                } else {
                    inverse_transform_add(TX_4X4, DCT_DCT, true, eob, &coeffs, &mut d16, 4, bd);
                    h = fnv(d16.iter().flat_map(|v| v.to_le_bytes()), h);
                }
            }
            let &(want_bd, want) = whts.next().unwrap();
            assert_eq!(want_bd, bd);
            assert_eq!(h, want, "{bd}-bit Walsh-Hadamard");
        }
        assert!(blocks.next().is_none() && whts.next().is_none());
    }

    #[test]
    fn a_hostile_block_wraps_and_never_panics() {
        for tx in [TX_4X4, TX_8X8, TX_16X16, 3] {
            for tx_type in [DCT_DCT, ADST_DCT, DCT_ADST, ADST_ADST] {
                let n = 4usize << tx;
                let coeffs = vec![i32::MIN; n * n];
                let mut dst = vec![0u8; n * n];
                inverse_transform_add(tx, tx_type, false, n * n, &coeffs, &mut dst, n, 8);
                let coeffs = vec![i32::MAX; n * n];
                inverse_transform_add(tx, tx_type, false, n * n, &coeffs, &mut dst, n, 8);
            }
        }
        let coeffs = vec![i32::MIN; 16];
        let mut dst = vec![0u8; 16];
        inverse_transform_add(TX_4X4, DCT_DCT, true, 16, &coeffs, &mut dst, 4, 8);
    }
}
