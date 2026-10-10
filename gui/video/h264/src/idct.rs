//! The inverse transforms (`h264idct_template.c`): a 4x4 or 8x8 block of
//! coefficients turned back into a residual and added to the predicted
//! samples, the shortcut for a block with only its DC, the loops over a
//! macroblock's blocks, and the dequantizing transforms of the luma and
//! chroma DC blocks.
//!
//! FFmpeg builds this file once per bit depth. Its 8-bit build keeps
//! coefficients as `int16`, so every store into a block truncates to 16
//! bits; the deeper builds keep `int32`. The arithmetic is FFmpeg's,
//! wrapping where its `unsigned` and `SUINT` operands wrap -- which a
//! valid stream never reaches, and a damaged one must reach the same way.
//! Blocks are stored transposed, as FFmpeg's scans place them.

#![allow(
    clippy::indexing_slicing,
    reason = "block indices are under the 16 or 64 coefficients a block has; sample indices \
              are a block's within its macroblock, inside the picture the caller passes"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int, unsigned and int16 conversions, made where it makes them"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sample offsets inside the picture, and FFmpeg's int arithmetic on coefficients \n              bounded by their 16 or 32 bits -- the transforms themselves wrap explicitly"
)]

use crate::mbtype::SCAN8;
use crate::picture::Pixel;

/// The coefficient and sample arithmetic of one of FFmpeg's builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Depth {
    /// Bits a sample: 8 to 14.
    pub(crate) bits: u32,
}

impl Depth {
    /// A value stored into a block: cut to 16 bits by the 8-bit build.
    fn store(self, v: u32) -> i32 {
        if self.bits > 8 {
            v as i32
        } else {
            i32::from(v as i16)
        }
    }

    /// A sample clipped to the depth's range (`av_clip_pixel`).
    fn clip(self, v: i32) -> i32 {
        v.clamp(0, (1 << self.bits) - 1)
    }

    /// `dst[at]` with `add` added and clipped.
    fn add<P: Pixel>(self, dst: &mut [P], at: usize, add: i32) {
        dst[at] = P::from_int(self.clip(dst[at].int().wrapping_add(add)));
    }
}

/// The 4x4 transform of `block`, added to the 4x4 samples of `dst` from
/// `at`, rows `stride` apart; the block left zero (`ff_h264_idct_add`).
pub(crate) fn idct_add<P: Pixel>(
    d: Depth,
    dst: &mut [P],
    at: usize,
    stride: usize,
    block: &mut [i32],
) {
    block[0] = d.store((block[0] as u32).wrapping_add(1 << 5));
    for i in 0..4 {
        let b = |k: usize| block[i + 4 * k];
        let z0 = (b(0) as u32).wrapping_add(b(2) as u32);
        let z1 = (b(0) as u32).wrapping_sub(b(2) as u32);
        let z2 = ((b(1) >> 1) as u32).wrapping_sub(b(3) as u32);
        let z3 = (b(1) as u32).wrapping_add((b(3) >> 1) as u32);
        block[i] = d.store(z0.wrapping_add(z3));
        block[i + 4] = d.store(z1.wrapping_add(z2));
        block[i + 8] = d.store(z1.wrapping_sub(z2));
        block[i + 12] = d.store(z0.wrapping_sub(z3));
    }
    for i in 0..4 {
        let b = |k: usize| block[k + 4 * i];
        let z0 = (b(0) as u32).wrapping_add(b(2) as u32);
        let z1 = (b(0) as u32).wrapping_sub(b(2) as u32);
        let z2 = ((b(1) >> 1) as u32).wrapping_sub(b(3) as u32);
        let z3 = (b(1) as u32).wrapping_add((b(3) >> 1) as u32);
        let row = |r: usize| at + i + r * stride;
        d.add(dst, row(0), z0.wrapping_add(z3) as i32 >> 6);
        d.add(dst, row(1), z1.wrapping_add(z2) as i32 >> 6);
        d.add(dst, row(2), z1.wrapping_sub(z2) as i32 >> 6);
        d.add(dst, row(3), z0.wrapping_sub(z3) as i32 >> 6);
    }
    block[..16].fill(0);
}

/// One pass of the 8x8 transform over the eight values `get` gives, its
/// results in `put`'s order 0 to 7 (the body of `ff_h264_idct8_add`'s
/// loops, whose two passes differ only in which of their operands are
/// `unsigned` -- which changes nothing once all of it wraps).
fn idct8_pass(get: impl Fn(usize) -> i32) -> [u32; 8] {
    let u = |k: usize| get(k) as u32;
    let a0 = u(0).wrapping_add(u(4));
    let a2 = u(0).wrapping_sub(u(4));
    let a4 = ((get(2) >> 1) as u32).wrapping_sub(u(6));
    let a6 = ((get(6) >> 1) as u32).wrapping_add(u(2));

    let b0 = a0.wrapping_add(a6);
    let b2 = a2.wrapping_add(a4);
    let b4 = a2.wrapping_sub(a4);
    let b6 = a0.wrapping_sub(a6);

    let a1 = u(5)
        .wrapping_sub(u(3))
        .wrapping_sub(u(7))
        .wrapping_sub((get(7) >> 1) as u32) as i32;
    let a3 = u(1)
        .wrapping_add(u(7))
        .wrapping_sub(u(3))
        .wrapping_sub((get(3) >> 1) as u32) as i32;
    let a5 = u(7)
        .wrapping_sub(u(1))
        .wrapping_add(u(5))
        .wrapping_add((get(5) >> 1) as u32) as i32;
    let a7 = u(3)
        .wrapping_add(u(5))
        .wrapping_add(u(1))
        .wrapping_add((get(1) >> 1) as u32) as i32;

    let b1 = ((a7 >> 2) as u32).wrapping_add(a1 as u32);
    let b3 = (a3 as u32).wrapping_add((a5 >> 2) as u32);
    let b5 = ((a3 >> 2) as u32).wrapping_sub(a5 as u32);
    let b7 = (a7 as u32).wrapping_sub((a1 >> 2) as u32);

    [
        b0.wrapping_add(b7),
        b2.wrapping_add(b5),
        b4.wrapping_add(b3),
        b6.wrapping_add(b1),
        b6.wrapping_sub(b1),
        b4.wrapping_sub(b3),
        b2.wrapping_sub(b5),
        b0.wrapping_sub(b7),
    ]
}

/// The 8x8 transform of `block` added to `dst`; the block left zero
/// (`ff_h264_idct8_add`).
pub(crate) fn idct8_add<P: Pixel>(
    d: Depth,
    dst: &mut [P],
    at: usize,
    stride: usize,
    block: &mut [i32],
) {
    block[0] = d.store((block[0] as u32).wrapping_add(32));
    for i in 0..8 {
        let out = idct8_pass(|k| block[i + k * 8]);
        for (k, v) in out.into_iter().enumerate() {
            block[i + k * 8] = d.store(v);
        }
    }
    for i in 0..8 {
        let out = idct8_pass(|k| block[k + i * 8]);
        for (k, v) in out.into_iter().enumerate() {
            d.add(dst, at + i + k * stride, v as i32 >> 6);
        }
    }
    block[..64].fill(0);
}

/// A 4x4 block with only its DC added to `dst` (`ff_h264_idct_dc_add`).
pub(crate) fn idct_dc_add<P: Pixel>(
    d: Depth,
    dst: &mut [P],
    at: usize,
    stride: usize,
    block: &mut [i32],
) {
    let dc = block[0].wrapping_add(32) >> 6;
    block[0] = 0;
    for j in 0..4 {
        for i in 0..4 {
            d.add(dst, at + j * stride + i, dc);
        }
    }
}

/// An 8x8 block with only its DC added to `dst` (`ff_h264_idct8_dc_add`).
pub(crate) fn idct8_dc_add<P: Pixel>(
    d: Depth,
    dst: &mut [P],
    at: usize,
    stride: usize,
    block: &mut [i32],
) {
    let dc = block[0].wrapping_add(32) >> 6;
    block[0] = 0;
    for j in 0..8 {
        for i in 0..8 {
            d.add(dst, at + j * stride + i, dc);
        }
    }
}

/// An inter macroblock's 16 luma 4x4 blocks, each with coefficients
/// transformed -- a block whose one coefficient is its DC by the shortcut
/// (`ff_h264_idct_add16`). `offsets` are each block's place in the
/// macroblock; `nnz` the `scan8`-laid counts.
pub(crate) fn idct_add16<P: Pixel>(
    d: Depth,
    dst: &mut [P],
    at: usize,
    offsets: &[usize],
    block: &mut [i32],
    stride: usize,
    nnz: &[u8],
) {
    for i in 0..16 {
        let n = nnz[SCAN8[i]];
        if n == 0 {
            continue;
        }
        let b = &mut block[i * 16..];
        if n == 1 && b[0] != 0 {
            idct_dc_add(d, dst, at + offsets[i], stride, b);
        } else {
            idct_add(d, dst, at + offsets[i], stride, b);
        }
    }
}

/// An intra macroblock's 16 luma 4x4 blocks: each with coefficients
/// transformed, each with only a DC -- which Intra16x16's DC transform
/// leaves without a count -- by the shortcut (`ff_h264_idct_add16intra`).
pub(crate) fn idct_add16intra<P: Pixel>(
    d: Depth,
    dst: &mut [P],
    at: usize,
    offsets: &[usize],
    block: &mut [i32],
    stride: usize,
    nnz: &[u8],
) {
    for i in 0..16 {
        let b = &mut block[i * 16..];
        if nnz[SCAN8[i]] != 0 {
            idct_add(d, dst, at + offsets[i], stride, b);
        } else if b[0] != 0 {
            idct_dc_add(d, dst, at + offsets[i], stride, b);
        }
    }
}

/// The four 8x8 blocks of a macroblock with the 8x8 transform
/// (`ff_h264_idct8_add4`).
pub(crate) fn idct8_add4<P: Pixel>(
    d: Depth,
    dst: &mut [P],
    at: usize,
    offsets: &[usize],
    block: &mut [i32],
    stride: usize,
    nnz: &[u8],
) {
    for i in (0..16).step_by(4) {
        let n = nnz[SCAN8[i]];
        if n == 0 {
            continue;
        }
        let b = &mut block[i * 16..];
        if n == 1 && b[0] != 0 {
            idct8_dc_add(d, dst, at + offsets[i], stride, b);
        } else {
            idct8_add(d, dst, at + offsets[i], stride, b);
        }
    }
}

/// The 4:2:0 chroma blocks, four of each plane (`ff_h264_idct_add8`):
/// `dst[0]` and `dst[1]` the Cb and Cr planes, `at` the macroblock's place
/// in each.
pub(crate) fn idct_add8<P: Pixel>(
    d: Depth,
    dst: [&mut [P]; 2],
    at: [usize; 2],
    offsets: &[usize],
    block: &mut [i32],
    stride: usize,
    nnz: &[u8],
) {
    for (j, plane) in dst.into_iter().enumerate() {
        let j = j + 1;
        for i in j * 16..j * 16 + 4 {
            let b = &mut block[i * 16..];
            if nnz[SCAN8[i]] != 0 {
                idct_add(d, &mut *plane, at[j - 1] + offsets[i], stride, b);
            } else if b[0] != 0 {
                idct_dc_add(d, &mut *plane, at[j - 1] + offsets[i], stride, b);
            }
        }
    }
}

/// The 4:2:2 chroma blocks, eight of each plane, the lower four's counts
/// and offsets four entries on from their coefficients
/// (`ff_h264_idct_add8_422`).
pub(crate) fn idct_add8_422<P: Pixel>(
    d: Depth,
    dst: [&mut [P]; 2],
    at: [usize; 2],
    offsets: &[usize],
    block: &mut [i32],
    stride: usize,
    nnz: &[u8],
) {
    let planes = dst;
    for j in 1..3 {
        for i in j * 16..j * 16 + 4 {
            let b = &mut block[i * 16..];
            let plane = &mut *planes[j - 1];
            if nnz[SCAN8[i]] != 0 {
                idct_add(d, plane, at[j - 1] + offsets[i], stride, b);
            } else if b[0] != 0 {
                idct_dc_add(d, plane, at[j - 1] + offsets[i], stride, b);
            }
        }
    }
    for j in 1..3 {
        for i in j * 16 + 4..j * 16 + 8 {
            let b = &mut block[i * 16..];
            let plane = &mut *planes[j - 1];
            if nnz[SCAN8[i + 4]] != 0 {
                idct_add(d, plane, at[j - 1] + offsets[i + 4], stride, b);
            } else if b[0] != 0 {
                idct_dc_add(d, plane, at[j - 1] + offsets[i + 4], stride, b);
            }
        }
    }
}

/// The 16 luma DC coefficients of an Intra16x16 macroblock transformed and
/// dequantized by `qmul` into the DC places of its 16 blocks in `output`
/// (`ff_h264_luma_dc_dequant_idct`).
pub(crate) fn luma_dc_dequant_idct(d: Depth, output: &mut [i32], input: &[i32], qmul: i32) {
    const STRIDE: usize = 16;
    const X_OFFSET: [usize; 4] = [0, 2 * STRIDE, 8 * STRIDE, 10 * STRIDE];
    let mut temp = [0i32; 16];
    for i in 0..4 {
        let z0 = input[4 * i].wrapping_add(input[4 * i + 1]);
        let z1 = input[4 * i].wrapping_sub(input[4 * i + 1]);
        let z2 = input[4 * i + 2].wrapping_sub(input[4 * i + 3]);
        let z3 = input[4 * i + 2].wrapping_add(input[4 * i + 3]);
        temp[4 * i] = z0.wrapping_add(z3);
        temp[4 * i + 1] = z0.wrapping_sub(z3);
        temp[4 * i + 2] = z1.wrapping_sub(z2);
        temp[4 * i + 3] = z1.wrapping_add(z2);
    }
    let q = qmul as u32;
    let out = |v: u32| d.store((v.wrapping_mul(q).wrapping_add(128) as i32 >> 8) as u32);
    for (i, offset) in X_OFFSET.into_iter().enumerate() {
        let t = |k: usize| temp[4 * k + i] as u32;
        let z0 = t(0).wrapping_add(t(2));
        let z1 = t(0).wrapping_sub(t(2));
        let z2 = t(1).wrapping_sub(t(3));
        let z3 = t(1).wrapping_add(t(3));
        output[offset] = out(z0.wrapping_add(z3));
        output[STRIDE + offset] = out(z1.wrapping_add(z2));
        output[STRIDE * 4 + offset] = out(z1.wrapping_sub(z2));
        output[STRIDE * 5 + offset] = out(z0.wrapping_sub(z3));
    }
}

/// A 4:2:0 chroma plane's four DC coefficients -- at the DC places of its
/// four blocks -- transformed and dequantized in place
/// (`ff_h264_chroma_dc_dequant_idct`); no rounding before the shift, as
/// FFmpeg has it.
pub(crate) fn chroma_dc_dequant_idct(d: Depth, block: &mut [i32], qmul: i32) {
    const STRIDE: usize = 16 * 2;
    const X_STRIDE: usize = 16;
    let a = block[0] as u32;
    let b = block[X_STRIDE] as u32;
    let c = block[STRIDE] as u32;
    let dd = block[STRIDE + X_STRIDE] as u32;
    let e = a.wrapping_sub(b);
    let a = a.wrapping_add(b);
    let b = c.wrapping_sub(dd);
    let c = c.wrapping_add(dd);
    let q = qmul as u32;
    let out = |v: u32| d.store((v.wrapping_mul(q) as i32 >> 7) as u32);
    block[0] = out(a.wrapping_add(c));
    block[X_STRIDE] = out(e.wrapping_add(b));
    block[STRIDE] = out(a.wrapping_sub(c));
    block[STRIDE + X_STRIDE] = out(e.wrapping_sub(b));
}

/// A 4:2:2 chroma plane's eight DC coefficients transformed and
/// dequantized in place (`ff_h264_chroma422_dc_dequant_idct`).
pub(crate) fn chroma422_dc_dequant_idct(d: Depth, block: &mut [i32], qmul: i32) {
    const STRIDE: usize = 16 * 2;
    const X_STRIDE: usize = 16;
    const X_OFFSET: [usize; 2] = [0, 16];
    let mut temp = [0u32; 8];
    for i in 0..4 {
        let p = block[STRIDE * i] as u32;
        let q = block[STRIDE * i + X_STRIDE] as u32;
        temp[2 * i] = p.wrapping_add(q);
        temp[2 * i + 1] = p.wrapping_sub(q);
    }
    let q = qmul as u32;
    let out = |v: u32| d.store((v.wrapping_mul(q).wrapping_add(128) as i32 >> 8) as u32);
    for (i, offset) in X_OFFSET.into_iter().enumerate() {
        let t = |k: usize| temp[2 * k + i];
        let z0 = t(0).wrapping_add(t(2));
        let z1 = t(0).wrapping_sub(t(2));
        let z2 = t(1).wrapping_sub(t(3));
        let z3 = t(1).wrapping_add(t(3));
        block[offset] = out(z0.wrapping_add(z3));
        block[STRIDE + offset] = out(z1.wrapping_add(z2));
        block[STRIDE * 2 + offset] = out(z1.wrapping_sub(z2));
        block[STRIDE * 3 + offset] = out(z0.wrapping_sub(z3));
    }
}

#[cfg(test)]
mod tests;
