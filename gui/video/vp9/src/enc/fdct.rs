//! The forward transforms: a block of prediction residual to coefficients.
//!
//! Each is libvpx's C, function for function: the DCTs of
//! `vpx_dsp/fwd_txfm.c` and the hybrid DCT/ADST transforms and lossless
//! Walsh-Hadamard of `vp9/encoder/vp9_dct.c`. They are not simply the
//! inverse transforms run backwards: each scales and rounds between its
//! passes in its own way, and the bitstream depends on it, so every rounding
//! is kept.
//!
//! The arithmetic is 64-bit with 32-bit coefficients, as in libvpx's
//! high-bit-depth build; its 8-bit build keeps 32 and 16, and gives the same
//! numbers on every residual an 8-bit picture can have.
//!
//! Translated into Rust from libvpx v1.17.0's `vpx_dsp/fwd_txfm.c` and
//! `vp9/encoder/vp9_dct.c` (copyright the WebM project authors), used under
//! libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "residuals are at most 12 bits; libvpx's transforms are designed so that no 64-bit intermediate overflows"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "every index is a loop position within a fixed-size array or a block of the transform's own size, which callers pass whole"
)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "outputs fit the 32-bit coefficients they are stored in, as libvpx's casts to tran_low_t assume"
)]

use crate::common::{ADST_ADST, ADST_DCT, DCT_ADST, TxType};

/// `cospi_k_64`: round(16384 * cos(k * pi / 64)), index k.
const COSPI: [i64; 32] = [
    16384, 16364, 16305, 16207, 16069, 15893, 15679, 15426, 15137, 14811, 14449, 14053, 13623,
    13160, 12665, 12140, 11585, 11003, 10394, 9760, 9102, 8423, 7723, 7005, 6270, 5520, 4756, 3981,
    3196, 2404, 1606, 804,
];

const SINPI_1_9: i64 = 5283;
const SINPI_2_9: i64 = 9929;
const SINPI_3_9: i64 = 13377;
const SINPI_4_9: i64 = 15212;

/// libvpx's `UNIT_QUANT_FACTOR`: the lossless transform's scale.
const UNIT_QUANT_FACTOR: i64 = 4;

/// libvpx's `fdct_round_shift` (and `dct_32_round`):
/// `ROUND_POWER_OF_TWO(x, DCT_CONST_BITS)`.
#[inline(always)]
fn rs(x: i64) -> i64 {
    (x + (1 << 13)) >> 14
}

#[inline(always)]
fn c(k: usize) -> i64 {
    COSPI[k]
}

// --- vpx_dsp/fwd_txfm.c ----------------------------------------------------------------------

/// libvpx's `vpx_fdct4x4_c`.
pub(crate) fn fdct4x4(input: &[i16], stride: usize, output: &mut [i32; 16]) {
    let mut intermediate = [0i64; 16];
    for pass in 0..2 {
        for i in 0..4 {
            let mut in_high = [0i64; 4];
            if pass == 0 {
                for (k, v) in in_high.iter_mut().enumerate() {
                    *v = i64::from(input[k * stride + i]) * 16;
                }
                if i == 0 && in_high[0] != 0 {
                    in_high[0] += 1;
                }
            } else {
                for (k, v) in in_high.iter_mut().enumerate() {
                    *v = intermediate[k * 4 + i];
                }
            }
            let step = [
                in_high[0] + in_high[3],
                in_high[1] + in_high[2],
                in_high[1] - in_high[2],
                in_high[0] - in_high[3],
            ];
            let out = [
                rs((step[0] + step[1]) * c(16)),
                rs(step[2] * c(24) + step[3] * c(8)),
                rs((step[0] - step[1]) * c(16)),
                rs(-step[2] * c(8) + step[3] * c(24)),
            ];
            for (k, &v) in out.iter().enumerate() {
                if pass == 0 {
                    intermediate[i * 4 + k] = v;
                } else {
                    output[i * 4 + k] = v as i32;
                }
            }
        }
    }
    for v in output.iter_mut() {
        *v = (*v + 1) >> 2;
    }
}

/// The 4-point DCT of an 8-point one's even half, and its odd half: the core
/// both `fdct8x8` passes and `vp9_dct.c`'s `fdct8` share.
#[inline(always)]
fn fdct8_core(s: [i64; 8]) -> [i64; 8] {
    let [s0, s1, s2, s3, s4, s5, s6, s7] = s;
    // fdct4(step, step);
    let x0 = s0 + s3;
    let x1 = s1 + s2;
    let x2 = s1 - s2;
    let x3 = s0 - s3;
    let mut out = [0i64; 8];
    out[0] = rs((x0 + x1) * c(16));
    out[2] = rs(x2 * c(24) + x3 * c(8));
    out[4] = rs((x0 - x1) * c(16));
    out[6] = rs(-x2 * c(8) + x3 * c(24));
    // Stage 2.
    let t2 = rs((s6 - s5) * c(16));
    let t3 = rs((s6 + s5) * c(16));
    // Stage 3.
    let x0 = s4 + t2;
    let x1 = s4 - t2;
    let x2 = s7 - t3;
    let x3 = s7 + t3;
    // Stage 4.
    out[1] = rs(x0 * c(28) + x3 * c(4));
    out[3] = rs(x2 * c(12) + x1 * -c(20));
    out[5] = rs(x1 * c(12) + x2 * c(20));
    out[7] = rs(x3 * c(28) + x0 * -c(4));
    out
}

// --- Eight columns at once ---------------------------------------------------------------------
//
// The 8x8 DCT is the encoder's commonest transform, so it is also written
// the way libvpx's SSE2 version is: each pass transforms all eight columns
// together, a row of the block being eight lanes, and stores each column's
// coefficients as a row -- the transpose between the passes. A pass is a
// loop over the columns whose body is one column's transform, which the
// compiler's loop vectoriser runs four (or eight) columns to an instruction,
// making the transposing stores shuffles; written as operations on whole
// rows instead, it stays scalar. The arithmetic is libvpx's 8-bit build's --
// 32-bit intermediates, which hold every value an 8-bit residual can produce
// (the largest product is below 2^30) -- so the coefficients are
// `fdct8_core`'s exactly, which the tests check.

/// One row of eight lanes.
type Lanes = [i32; 8];

/// `cospi_k_64` in 32 bits.
#[inline(always)]
fn c32(k: usize) -> i32 {
    COSPI[k] as i32
}

/// `fdct_round_shift` in 32 bits.
#[inline(always)]
fn rs32(x: i32) -> i32 {
    (x + (1 << 13)) >> 14
}

/// One pass of `vpx_fdct8x8_c` over eight columns, `rows` the block's rows
/// (lane `i` of each is column `i`), each sum and difference times `scale`:
/// `fdct8_core` for every column. Returns column `i`'s coefficients as row
/// `i` -- libvpx's intermediate, whose columns its second pass reads, as
/// this one reads `rows`'.
#[inline(always)]
fn fdct8x8_pass(rows: &[Lanes; 8], scale: i32) -> [Lanes; 8] {
    let mut out = [[0; 8]; 8];
    for (i, o) in out.iter_mut().enumerate() {
        let p = |r: usize| rows[r][i];
        let s0 = (p(0) + p(7)) * scale;
        let s1 = (p(1) + p(6)) * scale;
        let s2 = (p(2) + p(5)) * scale;
        let s3 = (p(3) + p(4)) * scale;
        let s4 = (p(3) - p(4)) * scale;
        let s5 = (p(2) - p(5)) * scale;
        let s6 = (p(1) - p(6)) * scale;
        let s7 = (p(0) - p(7)) * scale;
        let x0 = s0 + s3;
        let x1 = s1 + s2;
        let x2 = s1 - s2;
        let x3 = s0 - s3;
        let t2 = rs32((s6 - s5) * c32(16));
        let t3 = rs32((s6 + s5) * c32(16));
        let y0 = s4 + t2;
        let y1 = s4 - t2;
        let y2 = s7 - t3;
        let y3 = s7 + t3;
        *o = [
            rs32((x0 + x1) * c32(16)),
            rs32(y0 * c32(28) + y3 * c32(4)),
            rs32(x2 * c32(24) + x3 * c32(8)),
            rs32(y2 * c32(12) + y1 * -c32(20)),
            rs32((x0 - x1) * c32(16)),
            rs32(y1 * c32(12) + y2 * c32(20)),
            rs32(-x2 * c32(8) + x3 * c32(24)),
            rs32(y3 * c32(28) + y0 * -c32(4)),
        ];
    }
    out
}

/// libvpx's `vpx_fdct8x8_c`.
pub(crate) fn fdct8x8(input: &[i16], stride: usize, output: &mut [i32; 64]) {
    let mut rows = [[0; 8]; 8];
    for (k, row) in rows.iter_mut().enumerate() {
        // Callers pass whole blocks; a row past the slice reads as 0.
        let src: &[i16; 8] = input
            .get(k * stride..k * stride + 8)
            .and_then(|s| s.try_into().ok())
            .unwrap_or(&[0; 8]);
        for (v, &s) in row.iter_mut().zip(src) {
            *v = i32::from(s);
        }
    }
    let second = fdct8x8_pass(&fdct8x8_pass(&rows, 4), 1);
    for (out, row) in output.chunks_exact_mut(8).zip(&second) {
        for (o, &v) in out.iter_mut().zip(row) {
            // C's division, which truncates toward zero.
            *o = v / 2;
        }
    }
}

/// The odd half of a 16-point DCT, from `step1` (its 8 differences): the
/// outputs 1, 3, ..., 15. Shared by `fdct16x16` and `fdct16`.
#[inline(always)]
fn fdct16_odd(step1: [i64; 8]) -> [i64; 8] {
    let mut step1 = step1;
    let mut step2 = [0i64; 8];
    let mut step3 = [0i64; 8];
    // Step 2.
    step2[2] = rs((step1[5] - step1[2]) * c(16));
    step2[3] = rs((step1[4] - step1[3]) * c(16));
    step2[4] = rs((step1[4] + step1[3]) * c(16));
    step2[5] = rs((step1[5] + step1[2]) * c(16));
    // Step 3.
    step3[0] = step1[0] + step2[3];
    step3[1] = step1[1] + step2[2];
    step3[2] = step1[1] - step2[2];
    step3[3] = step1[0] - step2[3];
    step3[4] = step1[7] - step2[4];
    step3[5] = step1[6] - step2[5];
    step3[6] = step1[6] + step2[5];
    step3[7] = step1[7] + step2[4];
    // Step 4.
    step2[1] = rs(step3[1] * -c(8) + step3[6] * c(24));
    step2[2] = rs(step3[2] * c(24) + step3[5] * c(8));
    step2[5] = rs(step3[2] * c(8) - step3[5] * c(24));
    step2[6] = rs(step3[1] * c(24) + step3[6] * c(8));
    // Step 5.
    step1[0] = step3[0] + step2[1];
    step1[1] = step3[0] - step2[1];
    step1[2] = step3[3] + step2[2];
    step1[3] = step3[3] - step2[2];
    step1[4] = step3[4] - step2[5];
    step1[5] = step3[4] + step2[5];
    step1[6] = step3[7] - step2[6];
    step1[7] = step3[7] + step2[6];
    // Step 6: outputs 1, 9, 5, 13, 3, 11, 7, 15, here in order 1, 3, ..., 15.
    [
        rs(step1[0] * c(30) + step1[7] * c(2)),
        rs(step1[3] * -c(26) + step1[4] * c(6)),
        rs(step1[2] * c(22) + step1[5] * c(10)),
        rs(step1[1] * -c(18) + step1[6] * c(14)),
        rs(step1[1] * c(14) + step1[6] * c(18)),
        rs(step1[2] * -c(10) + step1[5] * c(22)),
        rs(step1[3] * c(6) + step1[4] * c(26)),
        rs(step1[0] * -c(2) + step1[7] * c(30)),
    ]
}

/// The even half of a 16-point DCT, from its 8 sums: the outputs 0, 2, ...,
/// 14. libvpx's `fdct8` inlined in its 16-point transforms, which writes the
/// fdct4's middle outputs in the opposite order to `fdct8_core`'s.
#[inline(always)]
fn fdct16_even(input: [i64; 8]) -> [i64; 8] {
    let s0 = input[0] + input[7];
    let s1 = input[1] + input[6];
    let s2 = input[2] + input[5];
    let s3 = input[3] + input[4];
    let s4 = input[3] - input[4];
    let s5 = input[2] - input[5];
    let s6 = input[1] - input[6];
    let s7 = input[0] - input[7];
    let x0 = s0 + s3;
    let x1 = s1 + s2;
    let x2 = s1 - s2;
    let x3 = s0 - s3;
    let mut out = [0i64; 8];
    out[0] = rs((x0 + x1) * c(16));
    out[2] = rs(x3 * c(8) + x2 * c(24));
    out[4] = rs((x0 - x1) * c(16));
    out[6] = rs(x3 * c(24) - x2 * c(8));
    let t2 = rs((s6 - s5) * c(16));
    let t3 = rs((s6 + s5) * c(16));
    let x0 = s4 + t2;
    let x1 = s4 - t2;
    let x2 = s7 - t3;
    let x3 = s7 + t3;
    out[1] = rs(x0 * c(28) + x3 * c(4));
    out[3] = rs(x2 * c(12) + x1 * -c(20));
    out[5] = rs(x1 * c(12) + x2 * c(20));
    out[7] = rs(x3 * c(28) + x0 * -c(4));
    out
}

/// libvpx's `vpx_fdct16x16_c`.
pub(crate) fn fdct16x16(input: &[i16], stride: usize, output: &mut [i32; 256]) {
    let mut intermediate = [0i64; 256];
    for pass in 0..2 {
        for i in 0..16 {
            let px = |k: usize| -> i64 {
                if pass == 0 {
                    i64::from(input[k * stride + i]) * 4
                } else {
                    (intermediate[k * 16 + i] + 1) >> 2
                }
            };
            let in_high: [i64; 8] = core::array::from_fn(|k| px(k) + px(15 - k));
            let step1: [i64; 8] = core::array::from_fn(|k| px(7 - k) - px(8 + k));
            let even = fdct16_even(in_high);
            let odd = fdct16_odd(step1);
            for k in 0..8 {
                let (e, o) = (even[k], odd[k]);
                if pass == 0 {
                    intermediate[i * 16 + 2 * k] = e;
                    intermediate[i * 16 + 2 * k + 1] = o;
                } else {
                    output[i * 16 + 2 * k] = e as i32;
                    output[i * 16 + 2 * k + 1] = o as i32;
                }
            }
        }
    }
}

/// libvpx's `half_round_shift`.
#[inline(always)]
fn half_round_shift(x: i64) -> i64 {
    (x + 1 + i64::from(x < 0)) >> 2
}

/// libvpx's `vpx_fdct32`: one 32-point pass. `round` scales the middle by
/// a quarter, as the rate-distortion variant's second pass does.
#[allow(clippy::too_many_lines, reason = "libvpx's butterfly, stage by stage")]
fn fdct32(input: &[i64; 32], output: &mut [i64; 32], round: bool) {
    let mut step = [0i64; 32];
    let o = output;
    // Stage 1.
    for k in 0..16 {
        step[k] = input[k] + input[31 - k];
        step[16 + k] = -input[16 + k] + input[15 - k];
    }
    // Stage 2.
    for k in 0..8 {
        o[k] = step[k] + step[15 - k];
        o[8 + k] = -step[8 + k] + step[7 - k];
    }
    o[16] = step[16];
    o[17] = step[17];
    o[18] = step[18];
    o[19] = step[19];
    o[20] = rs((-step[20] + step[27]) * c(16));
    o[21] = rs((-step[21] + step[26]) * c(16));
    o[22] = rs((-step[22] + step[25]) * c(16));
    o[23] = rs((-step[23] + step[24]) * c(16));
    o[24] = rs((step[24] + step[23]) * c(16));
    o[25] = rs((step[25] + step[22]) * c(16));
    o[26] = rs((step[26] + step[21]) * c(16));
    o[27] = rs((step[27] + step[20]) * c(16));
    o[28] = step[28];
    o[29] = step[29];
    o[30] = step[30];
    o[31] = step[31];
    // Keep the intermediate values within 16 bits.
    if round {
        for v in o.iter_mut() {
            *v = half_round_shift(*v);
        }
    }
    // Stage 3.
    step[0] = o[0] + o[7];
    step[1] = o[1] + o[6];
    step[2] = o[2] + o[5];
    step[3] = o[3] + o[4];
    step[4] = -o[4] + o[3];
    step[5] = -o[5] + o[2];
    step[6] = -o[6] + o[1];
    step[7] = -o[7] + o[0];
    step[8] = o[8];
    step[9] = o[9];
    step[10] = rs((-o[10] + o[13]) * c(16));
    step[11] = rs((-o[11] + o[12]) * c(16));
    step[12] = rs((o[12] + o[11]) * c(16));
    step[13] = rs((o[13] + o[10]) * c(16));
    step[14] = o[14];
    step[15] = o[15];
    step[16] = o[16] + o[23];
    step[17] = o[17] + o[22];
    step[18] = o[18] + o[21];
    step[19] = o[19] + o[20];
    step[20] = -o[20] + o[19];
    step[21] = -o[21] + o[18];
    step[22] = -o[22] + o[17];
    step[23] = -o[23] + o[16];
    step[24] = -o[24] + o[31];
    step[25] = -o[25] + o[30];
    step[26] = -o[26] + o[29];
    step[27] = -o[27] + o[28];
    step[28] = o[28] + o[27];
    step[29] = o[29] + o[26];
    step[30] = o[30] + o[25];
    step[31] = o[31] + o[24];
    // Stage 4.
    o[0] = step[0] + step[3];
    o[1] = step[1] + step[2];
    o[2] = -step[2] + step[1];
    o[3] = -step[3] + step[0];
    o[4] = step[4];
    o[5] = rs((-step[5] + step[6]) * c(16));
    o[6] = rs((step[6] + step[5]) * c(16));
    o[7] = step[7];
    o[8] = step[8] + step[11];
    o[9] = step[9] + step[10];
    o[10] = -step[10] + step[9];
    o[11] = -step[11] + step[8];
    o[12] = -step[12] + step[15];
    o[13] = -step[13] + step[14];
    o[14] = step[14] + step[13];
    o[15] = step[15] + step[12];
    o[16] = step[16];
    o[17] = step[17];
    o[18] = rs(step[18] * -c(8) + step[29] * c(24));
    o[19] = rs(step[19] * -c(8) + step[28] * c(24));
    o[20] = rs(step[20] * -c(24) + step[27] * -c(8));
    o[21] = rs(step[21] * -c(24) + step[26] * -c(8));
    o[22] = step[22];
    o[23] = step[23];
    o[24] = step[24];
    o[25] = step[25];
    o[26] = rs(step[26] * c(24) + step[21] * -c(8));
    o[27] = rs(step[27] * c(24) + step[20] * -c(8));
    o[28] = rs(step[28] * c(8) + step[19] * c(24));
    o[29] = rs(step[29] * c(8) + step[18] * c(24));
    o[30] = step[30];
    o[31] = step[31];
    // Stage 5.
    step[0] = rs((o[0] + o[1]) * c(16));
    step[1] = rs((-o[1] + o[0]) * c(16));
    step[2] = rs(o[2] * c(24) + o[3] * c(8));
    step[3] = rs(o[3] * c(24) - o[2] * c(8));
    step[4] = o[4] + o[5];
    step[5] = -o[5] + o[4];
    step[6] = -o[6] + o[7];
    step[7] = o[7] + o[6];
    step[8] = o[8];
    step[9] = rs(o[9] * -c(8) + o[14] * c(24));
    step[10] = rs(o[10] * -c(24) + o[13] * -c(8));
    step[11] = o[11];
    step[12] = o[12];
    step[13] = rs(o[13] * c(24) + o[10] * -c(8));
    step[14] = rs(o[14] * c(8) + o[9] * c(24));
    step[15] = o[15];
    step[16] = o[16] + o[19];
    step[17] = o[17] + o[18];
    step[18] = -o[18] + o[17];
    step[19] = -o[19] + o[16];
    step[20] = -o[20] + o[23];
    step[21] = -o[21] + o[22];
    step[22] = o[22] + o[21];
    step[23] = o[23] + o[20];
    step[24] = o[24] + o[27];
    step[25] = o[25] + o[26];
    step[26] = -o[26] + o[25];
    step[27] = -o[27] + o[24];
    step[28] = -o[28] + o[31];
    step[29] = -o[29] + o[30];
    step[30] = o[30] + o[29];
    step[31] = o[31] + o[28];
    // Stage 6.
    o[0] = step[0];
    o[1] = step[1];
    o[2] = step[2];
    o[3] = step[3];
    o[4] = rs(step[4] * c(28) + step[7] * c(4));
    o[5] = rs(step[5] * c(12) + step[6] * c(20));
    o[6] = rs(step[6] * c(12) + step[5] * -c(20));
    o[7] = rs(step[7] * c(28) + step[4] * -c(4));
    o[8] = step[8] + step[9];
    o[9] = -step[9] + step[8];
    o[10] = -step[10] + step[11];
    o[11] = step[11] + step[10];
    o[12] = step[12] + step[13];
    o[13] = -step[13] + step[12];
    o[14] = -step[14] + step[15];
    o[15] = step[15] + step[14];
    o[16] = step[16];
    o[17] = rs(step[17] * -c(4) + step[30] * c(28));
    o[18] = rs(step[18] * -c(28) + step[29] * -c(4));
    o[19] = step[19];
    o[20] = step[20];
    o[21] = rs(step[21] * -c(20) + step[26] * c(12));
    o[22] = rs(step[22] * -c(12) + step[25] * -c(20));
    o[23] = step[23];
    o[24] = step[24];
    o[25] = rs(step[25] * c(12) + step[22] * -c(20));
    o[26] = rs(step[26] * c(20) + step[21] * c(12));
    o[27] = step[27];
    o[28] = step[28];
    o[29] = rs(step[29] * c(28) + step[18] * -c(4));
    o[30] = rs(step[30] * c(4) + step[17] * c(28));
    o[31] = step[31];
    // Stage 7.
    step[..8].copy_from_slice(&o[..8]);
    step[8] = rs(o[8] * c(30) + o[15] * c(2));
    step[9] = rs(o[9] * c(14) + o[14] * c(18));
    step[10] = rs(o[10] * c(22) + o[13] * c(10));
    step[11] = rs(o[11] * c(6) + o[12] * c(26));
    step[12] = rs(o[12] * c(6) + o[11] * -c(26));
    step[13] = rs(o[13] * c(22) + o[10] * -c(10));
    step[14] = rs(o[14] * c(14) + o[9] * -c(18));
    step[15] = rs(o[15] * c(30) + o[8] * -c(2));
    step[16] = o[16] + o[17];
    step[17] = -o[17] + o[16];
    step[18] = -o[18] + o[19];
    step[19] = o[19] + o[18];
    step[20] = o[20] + o[21];
    step[21] = -o[21] + o[20];
    step[22] = -o[22] + o[23];
    step[23] = o[23] + o[22];
    step[24] = o[24] + o[25];
    step[25] = -o[25] + o[24];
    step[26] = -o[26] + o[27];
    step[27] = o[27] + o[26];
    step[28] = o[28] + o[29];
    step[29] = -o[29] + o[28];
    step[30] = -o[30] + o[31];
    step[31] = o[31] + o[30];
    // Final stage: the outputs bit-reversed.
    o[0] = step[0];
    o[16] = step[1];
    o[8] = step[2];
    o[24] = step[3];
    o[4] = step[4];
    o[20] = step[5];
    o[12] = step[6];
    o[28] = step[7];
    o[2] = step[8];
    o[18] = step[9];
    o[10] = step[10];
    o[26] = step[11];
    o[6] = step[12];
    o[22] = step[13];
    o[14] = step[14];
    o[30] = step[15];
    o[1] = rs(step[16] * c(31) + step[31] * c(1));
    o[17] = rs(step[17] * c(15) + step[30] * c(17));
    o[9] = rs(step[18] * c(23) + step[29] * c(9));
    o[25] = rs(step[19] * c(7) + step[28] * c(25));
    o[5] = rs(step[20] * c(27) + step[27] * c(5));
    o[21] = rs(step[21] * c(11) + step[26] * c(21));
    o[13] = rs(step[22] * c(19) + step[25] * c(13));
    o[29] = rs(step[23] * c(3) + step[24] * c(29));
    o[3] = rs(step[24] * c(3) + step[23] * -c(29));
    o[19] = rs(step[25] * c(19) + step[22] * -c(13));
    o[11] = rs(step[26] * c(11) + step[21] * -c(21));
    o[27] = rs(step[27] * c(27) + step[20] * -c(5));
    o[7] = rs(step[28] * c(7) + step[19] * -c(25));
    o[23] = rs(step[29] * c(23) + step[18] * -c(9));
    o[15] = rs(step[30] * c(15) + step[17] * -c(17));
    o[31] = rs(step[31] * c(31) + step[16] * -c(1));
}

/// libvpx's `vpx_fdct32x32_c` and, with `rd`, `vpx_fdct32x32_rd_c` -- the
/// variant whose second pass keeps 16 bits, used where only an estimate of
/// the cost is wanted.
pub(crate) fn fdct32x32(input: &[i16], stride: usize, output: &mut [i32; 1024], rd: bool) {
    let mut out = [0i64; 1024];
    let mut temp_in = [0i64; 32];
    let mut temp_out = [0i64; 32];
    // Columns.
    for i in 0..32 {
        for j in 0..32 {
            temp_in[j] = i64::from(input[j * stride + i]) * 4;
        }
        fdct32(&temp_in, &mut temp_out, false);
        for j in 0..32 {
            out[j * 32 + i] = (temp_out[j] + 1 + i64::from(temp_out[j] > 0)) >> 2;
        }
    }
    // Rows.
    for i in 0..32 {
        temp_in.copy_from_slice(&out[i * 32..i * 32 + 32]);
        fdct32(&temp_in, &mut temp_out, rd);
        for j in 0..32 {
            output[i * 32 + j] = if rd {
                temp_out[j] as i32
            } else {
                ((temp_out[j] + 1 + i64::from(temp_out[j] < 0)) >> 2) as i32
            };
        }
    }
}

// --- vp9/encoder/vp9_dct.c ---------------------------------------------------------------------

/// A 1-D transform of `N` points: one of the hybrid transforms' halves.
type Fn1d<const N: usize> = fn(&[i64; N]) -> [i64; N];

fn fdct4(i: &[i64; 4]) -> [i64; 4] {
    let step = [i[0] + i[3], i[1] + i[2], i[1] - i[2], i[0] - i[3]];
    [
        rs((step[0] + step[1]) * c(16)),
        rs(step[2] * c(24) + step[3] * c(8)),
        rs((step[0] - step[1]) * c(16)),
        rs(-step[2] * c(8) + step[3] * c(24)),
    ]
}

fn fdct8(i: &[i64; 8]) -> [i64; 8] {
    fdct8_core([
        i[0] + i[7],
        i[1] + i[6],
        i[2] + i[5],
        i[3] + i[4],
        i[3] - i[4],
        i[2] - i[5],
        i[1] - i[6],
        i[0] - i[7],
    ])
}

fn fdct16(i: &[i64; 16]) -> [i64; 16] {
    let even = fdct16_even(core::array::from_fn(|k| i[k] + i[15 - k]));
    let odd = fdct16_odd(core::array::from_fn(|k| i[7 - k] - i[8 + k]));
    core::array::from_fn(|k| if k % 2 == 0 { even[k / 2] } else { odd[k / 2] })
}

fn fadst4(i: &[i64; 4]) -> [i64; 4] {
    let [x0, x1, x2, x3] = *i;
    if x0 | x1 | x2 | x3 == 0 {
        return [0; 4];
    }
    let s0 = SINPI_1_9 * x0;
    let s1 = SINPI_4_9 * x0;
    let s2 = SINPI_2_9 * x1;
    let s3 = SINPI_1_9 * x1;
    let s4 = SINPI_3_9 * x2;
    let s5 = SINPI_4_9 * x3;
    let s6 = SINPI_2_9 * x3;
    let s7 = x0 + x1 - x3;
    let x0 = s0 + s2 + s5;
    let x1 = SINPI_3_9 * s7;
    let x2 = s1 - s3 + s6;
    let x3 = s4;
    [rs(x0 + x3), rs(x1), rs(x2 - x3), rs(x2 - x0 + x3)]
}

fn fadst8(i: &[i64; 8]) -> [i64; 8] {
    let (x0, x1, x2, x3) = (i[7], i[0], i[5], i[2]);
    let (x4, x5, x6, x7) = (i[3], i[4], i[1], i[6]);
    // Stage 1.
    let s0 = c(2) * x0 + c(30) * x1;
    let s1 = c(30) * x0 - c(2) * x1;
    let s2 = c(10) * x2 + c(22) * x3;
    let s3 = c(22) * x2 - c(10) * x3;
    let s4 = c(18) * x4 + c(14) * x5;
    let s5 = c(14) * x4 - c(18) * x5;
    let s6 = c(26) * x6 + c(6) * x7;
    let s7 = c(6) * x6 - c(26) * x7;
    let x0 = rs(s0 + s4);
    let x1 = rs(s1 + s5);
    let x2 = rs(s2 + s6);
    let x3 = rs(s3 + s7);
    let x4 = rs(s0 - s4);
    let x5 = rs(s1 - s5);
    let x6 = rs(s2 - s6);
    let x7 = rs(s3 - s7);
    // Stage 2.
    let (s0, s1, s2, s3) = (x0, x1, x2, x3);
    let s4 = c(8) * x4 + c(24) * x5;
    let s5 = c(24) * x4 - c(8) * x5;
    let s6 = -c(24) * x6 + c(8) * x7;
    let s7 = c(8) * x6 + c(24) * x7;
    let x0 = s0 + s2;
    let x1 = s1 + s3;
    let x2 = s0 - s2;
    let x3 = s1 - s3;
    let x4 = rs(s4 + s6);
    let x5 = rs(s5 + s7);
    let x6 = rs(s4 - s6);
    let x7 = rs(s5 - s7);
    // Stage 3.
    let s2 = c(16) * (x2 + x3);
    let s3 = c(16) * (x2 - x3);
    let s6 = c(16) * (x6 + x7);
    let s7 = c(16) * (x6 - x7);
    let (x2, x3, x6, x7) = (rs(s2), rs(s3), rs(s6), rs(s7));
    [x0, -x4, x6, -x2, x3, -x7, x5, -x1]
}

#[allow(clippy::too_many_lines, reason = "libvpx's butterfly, stage by stage")]
fn fadst16(i: &[i64; 16]) -> [i64; 16] {
    let (x0, x1, x2, x3) = (i[15], i[0], i[13], i[2]);
    let (x4, x5, x6, x7) = (i[11], i[4], i[9], i[6]);
    let (x8, x9, x10, x11) = (i[7], i[8], i[5], i[10]);
    let (x12, x13, x14, x15) = (i[3], i[12], i[1], i[14]);
    // Stage 1.
    let s0 = x0 * c(1) + x1 * c(31);
    let s1 = x0 * c(31) - x1 * c(1);
    let s2 = x2 * c(5) + x3 * c(27);
    let s3 = x2 * c(27) - x3 * c(5);
    let s4 = x4 * c(9) + x5 * c(23);
    let s5 = x4 * c(23) - x5 * c(9);
    let s6 = x6 * c(13) + x7 * c(19);
    let s7 = x6 * c(19) - x7 * c(13);
    let s8 = x8 * c(17) + x9 * c(15);
    let s9 = x8 * c(15) - x9 * c(17);
    let s10 = x10 * c(21) + x11 * c(11);
    let s11 = x10 * c(11) - x11 * c(21);
    let s12 = x12 * c(25) + x13 * c(7);
    let s13 = x12 * c(7) - x13 * c(25);
    let s14 = x14 * c(29) + x15 * c(3);
    let s15 = x14 * c(3) - x15 * c(29);
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
    let (s0, s1, s2, s3, s4, s5, s6, s7) = (x0, x1, x2, x3, x4, x5, x6, x7);
    let s8 = x8 * c(4) + x9 * c(28);
    let s9 = x8 * c(28) - x9 * c(4);
    let s10 = x10 * c(20) + x11 * c(12);
    let s11 = x10 * c(12) - x11 * c(20);
    let s12 = -x12 * c(28) + x13 * c(4);
    let s13 = x12 * c(4) + x13 * c(28);
    let s14 = -x14 * c(12) + x15 * c(20);
    let s15 = x14 * c(20) + x15 * c(12);
    let x0 = s0 + s4;
    let x1 = s1 + s5;
    let x2 = s2 + s6;
    let x3 = s3 + s7;
    let x4 = s0 - s4;
    let x5 = s1 - s5;
    let x6 = s2 - s6;
    let x7 = s3 - s7;
    let x8 = rs(s8 + s12);
    let x9 = rs(s9 + s13);
    let x10 = rs(s10 + s14);
    let x11 = rs(s11 + s15);
    let x12 = rs(s8 - s12);
    let x13 = rs(s9 - s13);
    let x14 = rs(s10 - s14);
    let x15 = rs(s11 - s15);
    // Stage 3.
    let (s0, s1, s2, s3) = (x0, x1, x2, x3);
    let s4 = x4 * c(8) + x5 * c(24);
    let s5 = x4 * c(24) - x5 * c(8);
    let s6 = -x6 * c(24) + x7 * c(8);
    let s7 = x6 * c(8) + x7 * c(24);
    let (s8, s9, s10, s11) = (x8, x9, x10, x11);
    let s12 = x12 * c(8) + x13 * c(24);
    let s13 = x12 * c(24) - x13 * c(8);
    let s14 = -x14 * c(24) + x15 * c(8);
    let s15 = x14 * c(8) + x15 * c(24);
    let x0 = s0 + s2;
    let x1 = s1 + s3;
    let x2 = s0 - s2;
    let x3 = s1 - s3;
    let x4 = rs(s4 + s6);
    let x5 = rs(s5 + s7);
    let x6 = rs(s4 - s6);
    let x7 = rs(s5 - s7);
    let x8 = s8 + s10;
    let x9 = s9 + s11;
    let x10 = s8 - s10;
    let x11 = s9 - s11;
    let x12 = rs(s12 + s14);
    let x13 = rs(s13 + s15);
    let x14 = rs(s12 - s14);
    let x15 = rs(s13 - s15);
    // Stage 4.
    let x2_ = rs(-c(16) * (x2 + x3));
    let x3_ = rs(c(16) * (x2 - x3));
    let x6_ = rs(c(16) * (x6 + x7));
    let x7_ = rs(c(16) * (-x6 + x7));
    let x10_ = rs(c(16) * (x10 + x11));
    let x11_ = rs(c(16) * (-x10 + x11));
    let x14_ = rs(-c(16) * (x14 + x15));
    let x15_ = rs(c(16) * (x14 - x15));
    [
        x0, -x8, x12, -x4, x6_, x14_, x10_, x2_, x3_, x11_, x15_, x7_, x5, -x13, x9, -x1,
    ]
}

/// The columns' and rows' transforms of a hybrid transform type: libvpx's
/// `FHT_4`, `FHT_8` and `FHT_16` (`{ cols, rows }`).
fn pick<const N: usize>(tx_type: TxType, dct: Fn1d<N>, adst: Fn1d<N>) -> (Fn1d<N>, Fn1d<N>) {
    match tx_type {
        ADST_DCT => (adst, dct),
        DCT_ADST => (dct, adst),
        ADST_ADST => (adst, adst),
        _ => (dct, dct),
    }
}

/// libvpx's `vp9_fht4x4_c`.
pub(crate) fn fht4x4(input: &[i16], stride: usize, output: &mut [i32; 16], tx_type: TxType) {
    if !matches!(tx_type, ADST_DCT | DCT_ADST | ADST_ADST) {
        fdct4x4(input, stride, output);
        return;
    }
    let (cols, rows) = pick::<4>(tx_type, fdct4, fadst4);
    let mut out = [0i64; 16];
    for i in 0..4 {
        let mut temp_in: [i64; 4] = core::array::from_fn(|j| i64::from(input[j * stride + i]) * 16);
        if i == 0 && temp_in[0] != 0 {
            temp_in[0] += 1;
        }
        let temp_out = cols(&temp_in);
        for j in 0..4 {
            out[j * 4 + i] = temp_out[j];
        }
    }
    for i in 0..4 {
        let temp_in: [i64; 4] = core::array::from_fn(|j| out[j + i * 4]);
        let temp_out = rows(&temp_in);
        for j in 0..4 {
            output[j + i * 4] = ((temp_out[j] + 1) >> 2) as i32;
        }
    }
}

/// libvpx's `vp9_fht8x8_c`.
pub(crate) fn fht8x8(input: &[i16], stride: usize, output: &mut [i32; 64], tx_type: TxType) {
    if !matches!(tx_type, ADST_DCT | DCT_ADST | ADST_ADST) {
        fdct8x8(input, stride, output);
        return;
    }
    let (cols, rows) = pick::<8>(tx_type, fdct8, fadst8);
    let mut out = [0i64; 64];
    for i in 0..8 {
        let temp_in: [i64; 8] = core::array::from_fn(|j| i64::from(input[j * stride + i]) * 4);
        let temp_out = cols(&temp_in);
        for j in 0..8 {
            out[j * 8 + i] = temp_out[j];
        }
    }
    for i in 0..8 {
        let temp_in: [i64; 8] = core::array::from_fn(|j| out[j + i * 8]);
        let temp_out = rows(&temp_in);
        for j in 0..8 {
            output[j + i * 8] = ((temp_out[j] + i64::from(temp_out[j] < 0)) >> 1) as i32;
        }
    }
}

/// libvpx's `vp9_fht16x16_c`.
pub(crate) fn fht16x16(input: &[i16], stride: usize, output: &mut [i32; 256], tx_type: TxType) {
    if !matches!(tx_type, ADST_DCT | DCT_ADST | ADST_ADST) {
        fdct16x16(input, stride, output);
        return;
    }
    let (cols, rows) = pick::<16>(tx_type, fdct16, fadst16);
    let mut out = [0i64; 256];
    for i in 0..16 {
        let temp_in: [i64; 16] = core::array::from_fn(|j| i64::from(input[j * stride + i]) * 4);
        let temp_out = cols(&temp_in);
        for j in 0..16 {
            out[j * 16 + i] = (temp_out[j] + 1 + i64::from(temp_out[j] < 0)) >> 2;
        }
    }
    for i in 0..16 {
        let temp_in: [i64; 16] = core::array::from_fn(|j| out[j + i * 16]);
        let temp_out = rows(&temp_in);
        for j in 0..16 {
            output[j + i * 16] = temp_out[j] as i32;
        }
    }
}

/// libvpx's `vp9_fwht4x4_c`: the lossless transform, exactly reversible.
pub(crate) fn fwht4x4(input: &[i16], stride: usize, output: &mut [i32; 16]) {
    let mut tmp = [0i64; 16];
    for i in 0..4 {
        let mut a1 = i64::from(input[i]);
        let mut b1 = i64::from(input[stride + i]);
        let mut c1 = i64::from(input[2 * stride + i]);
        let mut d1 = i64::from(input[3 * stride + i]);
        a1 += b1;
        d1 -= c1;
        let e1 = (a1 - d1) >> 1;
        b1 = e1 - b1;
        c1 = e1 - c1;
        a1 -= c1;
        d1 += b1;
        tmp[i] = a1;
        tmp[4 + i] = c1;
        tmp[8 + i] = d1;
        tmp[12 + i] = b1;
    }
    for i in 0..4 {
        let mut a1 = tmp[4 * i];
        let mut b1 = tmp[4 * i + 1];
        let mut c1 = tmp[4 * i + 2];
        let mut d1 = tmp[4 * i + 3];
        a1 += b1;
        d1 -= c1;
        let e1 = (a1 - d1) >> 1;
        b1 = e1 - b1;
        c1 = e1 - c1;
        a1 -= c1;
        d1 += b1;
        output[4 * i] = (a1 * UNIT_QUANT_FACTOR) as i32;
        output[4 * i + 1] = (c1 * UNIT_QUANT_FACTOR) as i32;
        output[4 * i + 2] = (d1 * UNIT_QUANT_FACTOR) as i32;
        output[4 * i + 3] = (b1 * UNIT_QUANT_FACTOR) as i32;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a test: a failure should be loud")]

    use super::*;

    /// `tools/fdct_reference.c`'s output: (transform, type, hash) over 200
    /// blocks each. Transforms: 0 `fdct4x4`, 1 `fdct8x8`, 2 `fdct16x16`,
    /// 3 `fdct32x32`, 4 its rate-distortion form, 5-7 the hybrid 4x4, 8x8 and
    /// 16x16 at each type, 8 the lossless Walsh-Hadamard.
    const LIBVPX: [(u8, u8, u64); 18] = [
        (0, 0, 0xcfcd_9405_9570_e7ab),
        (1, 0, 0xeda3_a1f3_9b23_589d),
        (2, 0, 0xd8b0_5a24_3630_5038),
        (3, 0, 0x2773_d2a0_69c5_9465),
        (4, 0, 0xceb2_e06c_119c_932b),
        (5, 0, 0xabec_4ff7_2756_80b2),
        (5, 1, 0xa30a_6460_d859_9290),
        (5, 2, 0xb305_6c42_f4b7_4cfb),
        (5, 3, 0x5f14_a272_caae_df8e),
        (6, 0, 0xf3cb_973f_6c87_6a27),
        (6, 1, 0x13f0_9c1c_aa5a_33af),
        (6, 2, 0x4700_8c25_b04f_bef2),
        (6, 3, 0xe847_329e_f212_a74b),
        (7, 0, 0x162f_c9a7_3365_b613),
        (7, 1, 0x55ea_74ff_180a_acb0),
        (7, 2, 0x75ac_d89d_5191_0749),
        (7, 3, 0xd3a3_8575_953f_87d1),
        (8, 0, 0x5f22_6f77_4015_f257),
    ];

    struct Lcg(u32);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            self.0 >> 8
        }
    }

    fn fnv(coefs: &[i32], mut h: u64) -> u64 {
        for &v in coefs {
            for b in v.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x100_0000_01b3);
            }
        }
        h
    }

    /// As the reference's `fill`: random residuals, or one block in eight at
    /// the extremes.
    fn fill(rng: &mut Lcg, block: &mut [i16], n: usize) {
        let extreme = rng.next().is_multiple_of(8);
        for r in 0..n {
            for c in 0..n {
                block[r * 64 + c] = if extreme {
                    if rng.next() & 1 != 0 { 255 } else { -255 }
                } else {
                    (rng.next() % 511) as i16 - 255
                };
            }
        }
    }

    #[test]
    fn matches_libvpx_on_seeded_blocks_of_every_transform_and_type() {
        let mut rng = Lcg(0xfdc7_a5ed);
        let mut block = vec![0i16; 64 * 64];
        for &(t, tx_type, want) in &LIBVPX {
            let mut h = 0xcbf2_9ce4_8422_2325u64;
            for _ in 0..200 {
                let coefs: Vec<i32> = match t {
                    0 | 5 | 8 => {
                        fill(&mut rng, &mut block, 4);
                        let mut out = [0; 16];
                        match t {
                            0 => fdct4x4(&block, 64, &mut out),
                            5 => fht4x4(&block, 64, &mut out, tx_type),
                            _ => fwht4x4(&block, 64, &mut out),
                        }
                        out.to_vec()
                    }
                    1 | 6 => {
                        fill(&mut rng, &mut block, 8);
                        let mut out = [0; 64];
                        if t == 1 {
                            fdct8x8(&block, 64, &mut out);
                        } else {
                            fht8x8(&block, 64, &mut out, tx_type);
                        }
                        out.to_vec()
                    }
                    2 | 7 => {
                        fill(&mut rng, &mut block, 16);
                        let mut out = [0; 256];
                        if t == 2 {
                            fdct16x16(&block, 64, &mut out);
                        } else {
                            fht16x16(&block, 64, &mut out, tx_type);
                        }
                        out.to_vec()
                    }
                    _ => {
                        fill(&mut rng, &mut block, 32);
                        let mut out = [0; 1024];
                        fdct32x32(&block, 64, &mut out, t == 4);
                        out.to_vec()
                    }
                };
                h = fnv(&coefs, h);
            }
            assert_eq!(h, want, "transform {t}, type {tx_type}");
        }
    }

    /// `vpx_fdct8x8_c` as it is written -- one column at a time, 64-bit --
    /// for the eight-lane version to be checked against.
    fn fdct8x8_by_column(input: &[i16], stride: usize) -> [i32; 64] {
        let mut intermediate = [0i64; 64];
        let mut output = [0i32; 64];
        for pass in 0..2 {
            for i in 0..8 {
                let px = |k: usize| -> i64 {
                    if pass == 0 {
                        i64::from(input[k * stride + i])
                    } else {
                        intermediate[k * 8 + i]
                    }
                };
                let scale = if pass == 0 { 4 } else { 1 };
                let s = [
                    (px(0) + px(7)) * scale,
                    (px(1) + px(6)) * scale,
                    (px(2) + px(5)) * scale,
                    (px(3) + px(4)) * scale,
                    (px(3) - px(4)) * scale,
                    (px(2) - px(5)) * scale,
                    (px(1) - px(6)) * scale,
                    (px(0) - px(7)) * scale,
                ];
                for (k, &v) in fdct8_core(s).iter().enumerate() {
                    if pass == 0 {
                        intermediate[i * 8 + k] = v;
                    } else {
                        output[i * 8 + k] = (v / 2) as i32;
                    }
                }
            }
        }
        output
    }

    /// The eight-lane 8x8 DCT is libvpx's column-at-a-time one: on random
    /// residuals, on every block of the two extremes in each sample's
    /// sign pattern along a row or a column, and on the blocks that drive
    /// each coefficient furthest (the residual the sign of its basis).
    #[test]
    fn the_eight_lane_dct_is_the_column_dct() {
        let mut rng = Lcg(0x008a_8e11);
        let mut block = vec![0i16; 8 * 8];
        let check = |block: &[i16]| {
            let mut got = [0i32; 64];
            fdct8x8(block, 8, &mut got);
            assert_eq!(got, fdct8x8_by_column(block, 8), "{block:?}");
        };
        for _ in 0..20_000 {
            for v in &mut block {
                *v = (rng.next() % 511) as i16 - 255;
            }
            check(&block);
        }
        // Each row (and each column) one of 256 sign patterns of +-255.
        for pattern in 0..256u32 {
            for r in 0..8 {
                for c in 0..8 {
                    let bit = |i: usize| (pattern >> i) & 1 != 0;
                    block[r * 8 + c] = if bit(c) ^ (r % 2 == 1) { 255 } else { -255 };
                }
            }
            check(&block);
            for r in 0..8 {
                for c in 0..8 {
                    let bit = |i: usize| (pattern >> i) & 1 != 0;
                    block[r * 8 + c] = if bit(r) ^ (c % 2 == 1) { 255 } else { -255 };
                }
            }
            check(&block);
        }
        // The residual each coefficient's basis function's sign: the most
        // that coefficient can be.
        let cos = |n: u32, k: u32| {
            (f64::from(2 * n + 1) * f64::from(k) * std::f64::consts::PI / 16.0).cos()
        };
        for u in 0..8u32 {
            for v in 0..8u32 {
                for sign in [255i16, -255] {
                    for r in 0..8u32 {
                        for c in 0..8u32 {
                            let positive = cos(r, u) * cos(c, v) >= 0.0;
                            block[(r * 8 + c) as usize] = if positive { sign } else { -sign };
                        }
                    }
                    check(&block);
                }
            }
        }
    }

    #[test]
    fn the_lossless_transform_inverts_exactly() {
        // The decoder's inverse Walsh-Hadamard takes back what this gives.
        let mut rng = Lcg(7);
        let mut block = vec![0i16; 64 * 4];
        for _ in 0..100 {
            fill(&mut rng, &mut block, 4);
            let mut coefs = [0; 16];
            fwht4x4(&block, 64, &mut coefs);
            let mut recon = vec![0u8; 16];
            // Reconstruct onto a mid-grey prediction offset to keep the
            // residual inside 0..=255.
            let pred: Vec<u8> = (0..16).map(|_| 128u8).collect();
            recon.copy_from_slice(&pred);
            crate::idct::inverse_transform_add::<u8>(0, 0, true, 16, &coefs, &mut recon, 4, 8);
            for r in 0..4 {
                for c in 0..4 {
                    let want = (128 + i32::from(block[r * 64 + c])).clamp(0, 255);
                    assert_eq!(i32::from(recon[r * 4 + c]), want);
                }
            }
        }
    }
}
