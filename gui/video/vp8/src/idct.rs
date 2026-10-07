//! The inverse transforms: VP8's 4x4 integer DCT, added to the prediction,
//! and the Walsh-Hadamard transform that carries a macroblock's sixteen
//! luma DC coefficients.
//!
//! Every intermediate is kept in sixteen bits where libvpx keeps it there.
//! A valid stream never overflows them; a crafted one can, and its values
//! wrap here exactly as libvpx's do, so even its garbage is libvpx's.
//!
//! Translated into Rust from libvpx v1.17.0's `vp8/common/idctllm.c`,
//! `vp8/common/dequantize.c` and `vp8/common/idct_blk.c` (copyright the WebM
//! project authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "coefficient arrays are fixed at 16 and indexed by loop positions below 16; pixel positions are 4x4 blocks inside a macroblock inside the plane, which the caller has placed"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "products of two 16-bit values and sums of a few of them fit an i32; plane positions are bounded by the plane"
)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "libvpx stores these intermediates in 16 bits and lets them wrap; the casts are that wrap"
)]

/// `sqrt(2) * cos(pi / 8) - 1` in 16-bit fixed point: libvpx's
/// `cospi8sqrt2minus1`.
const COSPI8SQRT2MINUS1: i32 = 20091;
/// `sqrt(2) * sin(pi / 8)` in 16-bit fixed point: libvpx's `sinpi8sqrt2`.
const SINPI8SQRT2: i32 = 35468;

/// One butterfly of the 4x4 DCT, on `[x0, x1, x2, x3]` = the inputs at
/// rows (or columns) 0, 1, 2 and 3: the outputs before rounding.
#[inline]
fn butterfly(x0: i32, x1: i32, x2: i32, x3: i32) -> [i32; 4] {
    let a1 = x0 + x2;
    let b1 = x0 - x2;
    let c1 = ((x1 * SINPI8SQRT2) >> 16) - (x3 + ((x3 * COSPI8SQRT2MINUS1) >> 16));
    let d1 = (x1 + ((x1 * COSPI8SQRT2MINUS1) >> 16)) + ((x3 * SINPI8SQRT2) >> 16);
    [a1 + d1, b1 + c1, b1 - c1, a1 - d1]
}

/// Add `value` to a pixel, clamped to 0..=255.
#[inline]
fn add_clamped(pixel: u8, value: i32) -> u8 {
    (value + i32::from(pixel)).clamp(0, 255) as u8
}

/// Inverse-transform `input` and add it to the 4x4 block at `pos`: libvpx's
/// `vp8_short_idct4x4llm_c` with the prediction already in place.
pub(crate) fn idct4x4_add(input: &[i16; 16], dst: &mut [u8], pos: usize, stride: usize) {
    let mut tmp = [0i16; 16];
    for i in 0..4 {
        let out = butterfly(
            i32::from(input[i]),
            i32::from(input[4 + i]),
            i32::from(input[8 + i]),
            i32::from(input[12 + i]),
        );
        for (row, v) in out.into_iter().enumerate() {
            tmp[row * 4 + i] = v as i16;
        }
    }
    for row in 0..4 {
        let r = &tmp[row * 4..row * 4 + 4];
        let out = butterfly(
            i32::from(r[0]),
            i32::from(r[1]),
            i32::from(r[2]),
            i32::from(r[3]),
        );
        let at = pos + row * stride;
        for (col, v) in out.into_iter().enumerate() {
            let residual = i32::from(((v + 4) >> 3) as i16);
            dst[at + col] = add_clamped(dst[at + col], residual);
        }
    }
}

/// Add a block whose only coefficient is its DC, `input_dc`: libvpx's
/// `vp8_dc_only_idct_add_c`.
pub(crate) fn dc_only_idct_add(input_dc: i16, dst: &mut [u8], pos: usize, stride: usize) {
    let a1 = (i32::from(input_dc) + 4) >> 3;
    for row in 0..4 {
        let at = pos + row * stride;
        for p in &mut dst[at..at + 4] {
            *p = add_clamped(*p, a1);
        }
    }
}

/// Dequantise `q` by `dq`, inverse-transform it, add it to the block at
/// `pos`, and clear it for the next macroblock: libvpx's
/// `vp8_dequant_idct_add_c`.
pub(crate) fn dequant_idct_add(
    q: &mut [i16; 16],
    dq: &[i16; 16],
    dst: &mut [u8],
    pos: usize,
    stride: usize,
) {
    for (c, d) in q.iter_mut().zip(dq) {
        *c = (i32::from(*d) * i32::from(*c)) as i16;
    }
    idct4x4_add(q, dst, pos, stride);
    *q = [0; 16];
}

/// One block of a macroblock's residual: the full transform if it has more
/// than its DC (`eob > 1`), else the DC alone. Leaves `q` cleared, as libvpx's
/// `vp8_dequant_idct_add_y_block_c` and `_uv_block_c` do for each block.
#[inline]
fn block_add(
    q: &mut [i16; 16],
    dq: &[i16; 16],
    eob: u8,
    dst: &mut [u8],
    pos: usize,
    stride: usize,
) {
    if eob > 1 {
        dequant_idct_add(q, dq, dst, pos, stride);
    } else {
        let dc = (i32::from(q[0]) * i32::from(dq[0])) as i16;
        dc_only_idct_add(dc, dst, pos, stride);
        q[0] = 0;
        q[1] = 0;
    }
}

/// Add the sixteen luma blocks of a macroblock at `pos`: libvpx's
/// `vp8_dequant_idct_add_y_block_c`. `q` and `eobs` are the macroblock's
/// first sixteen blocks.
pub(crate) fn add_y_blocks(
    q: &mut [[i16; 16]],
    dq: &[i16; 16],
    eobs: &[u8],
    dst: &mut [u8],
    pos: usize,
    stride: usize,
) {
    for i in 0..4 {
        for j in 0..4 {
            let b = i * 4 + j;
            block_add(
                &mut q[b],
                dq,
                eobs[b],
                dst,
                pos + i * 4 * stride + j * 4,
                stride,
            );
        }
    }
}

/// Add the four blocks of one chroma plane of a macroblock at `pos`:
/// libvpx's `vp8_dequant_idct_add_uv_block_c`, for one plane.
pub(crate) fn add_uv_blocks(
    q: &mut [[i16; 16]],
    dq: &[i16; 16],
    eobs: &[u8],
    dst: &mut [u8],
    pos: usize,
    stride: usize,
) {
    for i in 0..2 {
        for j in 0..2 {
            let b = i * 2 + j;
            block_add(
                &mut q[b],
                dq,
                eobs[b],
                dst,
                pos + i * 4 * stride + j * 4,
                stride,
            );
        }
    }
}

/// The inverse Walsh-Hadamard transform of a macroblock's second-order
/// block, whose sixteen outputs are the DCs of its sixteen luma blocks:
/// libvpx's `vp8_short_inv_walsh4x4_c`.
pub(crate) fn inv_walsh4x4(input: &[i16; 16], y_blocks: &mut [[i16; 16]]) {
    let mut tmp = [0i16; 16];
    for i in 0..4 {
        let ip0 = i32::from(input[i]);
        let ip4 = i32::from(input[4 + i]);
        let ip8 = i32::from(input[8 + i]);
        let ip12 = i32::from(input[12 + i]);
        let a1 = ip0 + ip12;
        let b1 = ip4 + ip8;
        let c1 = ip4 - ip8;
        let d1 = ip0 - ip12;
        tmp[i] = (a1 + b1) as i16;
        tmp[4 + i] = (c1 + d1) as i16;
        tmp[8 + i] = (a1 - b1) as i16;
        tmp[12 + i] = (d1 - c1) as i16;
    }
    for row in 0..4 {
        let r = &tmp[row * 4..row * 4 + 4];
        let a1 = i32::from(r[0]) + i32::from(r[3]);
        let b1 = i32::from(r[1]) + i32::from(r[2]);
        let c1 = i32::from(r[1]) - i32::from(r[2]);
        let d1 = i32::from(r[0]) - i32::from(r[3]);
        let out = [a1 + b1, c1 + d1, a1 - b1, d1 - c1];
        for (col, v) in out.into_iter().enumerate() {
            y_blocks[row * 4 + col][0] = ((v + 3) >> 3) as i16;
        }
    }
}

/// [`inv_walsh4x4`] of a block whose only coefficient is its DC: libvpx's
/// `vp8_short_inv_walsh4x4_1_c`.
pub(crate) fn inv_walsh4x4_dc(input_dc: i16, y_blocks: &mut [[i16; 16]]) {
    let a1 = ((i32::from(input_dc) + 3) >> 3) as i16;
    for block in y_blocks.iter_mut().take(16) {
        block[0] = a1;
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "a test of 16-bit arithmetic"
    )]

    use super::*;

    /// RFC 6386's reference `vp8_short_idct4x4llm_c` (section 14.3), which
    /// keeps its intermediates in ints: the same as libvpx's for every input
    /// whose intermediates fit 16 bits.
    fn rfc_idct(input: &[i16; 16]) -> [i32; 16] {
        let mut out = [0i32; 16];
        for i in 0..4 {
            let x = |r: usize| i32::from(input[r * 4 + i]);
            let o = butterfly(x(0), x(1), x(2), x(3));
            for r in 0..4 {
                out[r * 4 + i] = o[r];
            }
        }
        let mut res = [0i32; 16];
        for r in 0..4 {
            let o = butterfly(out[r * 4], out[r * 4 + 1], out[r * 4 + 2], out[r * 4 + 3]);
            for c in 0..4 {
                res[r * 4 + c] = (o[c] + 4) >> 3;
            }
        }
        res
    }

    #[test]
    fn the_transform_adds_its_residual_to_the_prediction() {
        let mut s = 7u32;
        for _ in 0..2000 {
            let mut input = [0i16; 16];
            for c in &mut input {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *c = ((s >> 16) as i32 % 2048 - 1024) as i16;
            }
            let want = rfc_idct(&input);
            let mut plane = vec![128u8; 4 * 7];
            idct4x4_add(&input, &mut plane, 1, 7);
            for r in 0..4 {
                for c in 0..4 {
                    assert_eq!(
                        i32::from(plane[1 + r * 7 + c]),
                        (128 + want[r * 4 + c]).clamp(0, 255),
                        "{input:?} at {r},{c}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_dc_alone_is_the_full_transform_of_the_dc() {
        for dc in [-2048i16, -1000, -9, -4, -3, 0, 3, 4, 5, 100, 2047] {
            let mut input = [0i16; 16];
            input[0] = dc;
            let mut full = vec![100u8; 16];
            idct4x4_add(&input, &mut full, 0, 4);
            let mut dc_only = vec![100u8; 16];
            dc_only_idct_add(dc, &mut dc_only, 0, 4);
            assert_eq!(full, dc_only, "dc {dc}");
        }
    }

    #[test]
    fn dequantising_wraps_at_sixteen_bits_as_libvpx_does() {
        let mut q = [0i16; 16];
        q[0] = 2047;
        q[1] = -2048;
        let dq = [157i16; 16];
        let mut plane = vec![0u8; 16];
        dequant_idct_add(&mut q, &dq, &mut plane, 0, 4);
        assert_eq!(
            q, [0; 16],
            "the coefficients are cleared for the next block"
        );
        // 2047 * 157 wraps; the result is still a block of pixels, and the
        // same one as the transform of the wrapped values.
        let mut wrapped = [0i16; 16];
        wrapped[0] = (2047 * 157) as i16;
        wrapped[1] = (-2048 * 157) as i16;
        let mut want = vec![0u8; 16];
        idct4x4_add(&wrapped, &mut want, 0, 4);
        assert_eq!(plane, want);
    }

    #[test]
    fn the_walsh_transform_inverts_the_forward_one() {
        // The forward transform of RFC 6386 section 14.3's description,
        // scaled as the encoder scales it: a DC-only input spreads evenly.
        let mut input = [0i16; 16];
        input[0] = 800;
        let mut blocks = [[0i16; 16]; 16];
        inv_walsh4x4(&input, &mut blocks);
        for b in &blocks {
            assert_eq!(b[0], (800 + 3) >> 3);
        }
        let mut dc_blocks = [[0i16; 16]; 16];
        inv_walsh4x4_dc(800, &mut dc_blocks);
        assert_eq!(blocks, dc_blocks);
    }

    #[test]
    fn a_walsh_transform_of_one_coefficient_has_its_signs() {
        // Coefficient 1 (the first horizontal frequency) gives + + - - by
        // column, the same on every row.
        let mut input = [0i16; 16];
        input[1] = 64;
        let mut blocks = [[0i16; 16]; 16];
        inv_walsh4x4(&input, &mut blocks);
        for row in 0..4 {
            let got: Vec<i16> = (0..4).map(|c| blocks[row * 4 + c][0]).collect();
            assert_eq!(got, [8, 8, -8, -8], "row {row}");
        }
    }
}
