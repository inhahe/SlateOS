//! A macroblock's coefficients: up to 25 blocks of 16, coded as tokens in
//! the frame's token partitions.
//!
//! Each block's tokens are coded with probabilities chosen by the block's
//! type (luma without its DC, the second-order DC block, chroma, or luma
//! with its DC), the coefficient's band, and how busy the neighbourhood is:
//! for the first coefficient, how many of the blocks above and to the left
//! had any; for the rest, how big the previous coefficient was.
//!
//! libvpx's reader here is libwebp's (`GetCoeffs`), which reads the same
//! tokens as the RFC's tree but in an unrolled order, and takes a
//! coefficient's sign with [`BoolDecoder::read_signed`].
//!
//! Translated into Rust from libvpx v1.17.0's `vp8/decoder/detokenize.c`
//! (copyright the WebM project authors, in part from libwebp, copyright
//! Google Inc.), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "positions are below 17 by the loop's own test, bands index an 8-band table, contexts are sums of two flags, and block numbers are below 25"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "coefficient magnitudes are below 2115 and positions below 17"
)]

use crate::boolread::BoolDecoder;
use crate::tables::{BANDS, CAT3, CAT4, CAT5, CAT6, ZIGZAG};

/// The coefficient probabilities: by block type, band, context and tree
/// node.
pub(crate) type CoefProbs = [[[[u8; 11]; 3]; 8]; 4];

/// The probabilities of one block type: by band, context and node.
type TypeProbs = [[[u8; 11]; 3]; 8];

/// The block types' indices in [`CoefProbs`].
const TYPE_Y_NO_DC: usize = 0;
const TYPE_Y2: usize = 1;
const TYPE_UV: usize = 2;
const TYPE_Y_WITH_DC: usize = 3;

/// The extra bits of categories 3 to 6, each list ending in 0.
const CAT3456: [&[u8]; 4] = [&CAT3, &CAT4, &CAT5, &CAT6];

/// How much each context byte says: whether the block above (or left) at
/// that place had coefficients. Four luma columns, two for each chroma
/// plane, and the second-order block: libvpx's `ENTROPY_CONTEXT_PLANES`.
pub(crate) type Context = [u8; 9];

/// One block's tokens from position `n` (0, or 1 for a luma block whose DC
/// is in the second-order block) into `out`, in raster order: libvpx's
/// `GetCoeffs`. Returns the position after the last token read, 0 if the
/// block has none.
fn get_coeffs(
    bc: &mut BoolDecoder<'_>,
    prob: &TypeProbs,
    ctx: usize,
    n: usize,
    out: &mut [i16; 16],
) -> usize {
    // Positions 0 and 1 are bands 0 and 1, so `prob[n]` is the band's.
    let mut p = &prob[n][ctx];
    if !bc.read(p[0]) {
        // The first end-of-block, before any token: no coefficients.
        return 0;
    }
    let mut n = n;
    loop {
        n += 1;
        if !bc.read(p[1]) {
            // A zero.
            p = &prob[usize::from(BANDS[n])][0];
        } else {
            let v: i32;
            if !bc.read(p[2]) {
                p = &prob[usize::from(BANDS[n])][1];
                v = 1;
            } else {
                if !bc.read(p[3]) {
                    v = if !bc.read(p[4]) {
                        2
                    } else {
                        3 + i32::from(bc.read(p[5]))
                    };
                } else if !bc.read(p[6]) {
                    v = if !bc.read(p[7]) {
                        5 + i32::from(bc.read(159))
                    } else {
                        7 + 2 * i32::from(bc.read(165)) + i32::from(bc.read(145))
                    };
                } else {
                    let bit1 = usize::from(bc.read(p[8]));
                    let bit0 = usize::from(bc.read(p[9 + bit1]));
                    let cat = 2 * bit1 + bit0;
                    let mut extra = 0i32;
                    for &prob in CAT3456[cat].iter().take_while(|&&p| p != 0) {
                        extra = 2 * extra + i32::from(bc.read(prob));
                    }
                    v = extra + 3 + (8 << cat);
                }
                p = &prob[usize::from(BANDS[n])][2];
            }
            // At most 2114, so the signed value fits 16 bits.
            out[usize::from(ZIGZAG[n - 1])] = bc.read_signed(v) as i16;
            if n == 16 || !bc.read(p[0]) {
                return n;
            }
        }
        if n == 16 {
            return 16;
        }
    }
}

/// A macroblock's coefficients into `qcoeff` (blocks 0 to 15 luma, 16 to 19
/// U, 20 to 23 V, 24 the second-order block), each block's end position into
/// `eobs`: libvpx's `vp8_decode_mb_tokens`. `is_4x4` macroblocks have no
/// second-order block. Returns the total of the end positions, which is 0
/// exactly when there are no coefficients at all.
#[allow(
    clippy::cast_possible_truncation,
    reason = "end positions are at most 17"
)]
pub(crate) fn decode_mb_tokens(
    bc: &mut BoolDecoder<'_>,
    probs: &CoefProbs,
    is_4x4: bool,
    above: &mut Context,
    left: &mut Context,
    qcoeff: &mut [[i16; 16]; 25],
    eobs: &mut [u8; 25],
) -> i32 {
    let mut eobtotal: i32 = 0;
    let (luma_probs, skip_dc) = if is_4x4 {
        (&probs[TYPE_Y_WITH_DC], 0)
    } else {
        let ctx = usize::from(above[8] + left[8]);
        let nonzeros = get_coeffs(bc, &probs[TYPE_Y2], ctx, 0, &mut qcoeff[24]);
        let has = u8::from(nonzeros > 0);
        above[8] = has;
        left[8] = has;
        eobs[24] = nonzeros as u8;
        // The luma blocks' end positions below count their absent DC; this
        // takes those sixteen back off.
        eobtotal += nonzeros as i32 - 16;
        (&probs[TYPE_Y_NO_DC], 1)
    };
    for i in 0..16 {
        let (a, l) = (i & 3, (i & 0xc) >> 2);
        let ctx = usize::from(above[a] + left[l]);
        let nonzeros = get_coeffs(bc, luma_probs, ctx, skip_dc, &mut qcoeff[i]);
        let has = u8::from(nonzeros > 0);
        above[a] = has;
        left[l] = has;
        let eob = nonzeros + skip_dc;
        eobs[i] = eob as u8;
        eobtotal += eob as i32;
    }
    for i in 16..24 {
        let plane = usize::from(i > 19) << 1;
        let a = 4 + plane + (i & 1);
        let l = 4 + plane + usize::from((i & 3) > 1);
        let ctx = usize::from(above[a] + left[l]);
        let nonzeros = get_coeffs(bc, &probs[TYPE_UV], ctx, 0, &mut qcoeff[i]);
        let has = u8::from(nonzeros > 0);
        above[a] = has;
        left[l] = has;
        eobs[i] = nonzeros as u8;
        eobtotal += nonzeros as i32;
    }
    eobtotal
}

/// A macroblock with no coefficients clears the contexts it would have set:
/// libvpx's `vp8_reset_mb_tokens_context`. The second-order block's context
/// is left alone by a macroblock that has no second-order block.
pub(crate) fn reset_mb_tokens_context(is_4x4: bool, above: &mut Context, left: &mut Context) {
    above[..8].fill(0);
    left[..8].fill(0);
    if !is_4x4 {
        above[8] = 0;
        left[8] = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_immediate_end_of_block_is_no_coefficients() {
        // A stream of zero bytes reads every decision as 0, which for a
        // block's first node is the end of block.
        let data = [0u8; 64];
        let mut bc = BoolDecoder::new(&data);
        let probs = [[[[128u8; 11]; 3]; 8]; 4];
        let mut above = [0u8; 9];
        let mut left = [0u8; 9];
        let mut q = [[0i16; 16]; 25];
        let mut eobs = [9u8; 25];
        let total = decode_mb_tokens(
            &mut bc, &probs, false, &mut above, &mut left, &mut q, &mut eobs,
        );
        assert_eq!(total, 0);
        assert_eq!(eobs[24], 0);
        assert!(
            eobs[..16].iter().all(|&e| e == 1),
            "a luma block counts its absent DC"
        );
        assert!(eobs[16..24].iter().all(|&e| e == 0));
        assert_eq!(q, [[0; 16]; 25]);
    }

    #[test]
    fn contexts_reset_only_what_the_macroblock_has() {
        let mut above = [1u8; 9];
        let mut left = [1u8; 9];
        reset_mb_tokens_context(true, &mut above, &mut left);
        assert_eq!(above, [0, 0, 0, 0, 0, 0, 0, 0, 1]);
        reset_mb_tokens_context(false, &mut above, &mut left);
        assert_eq!(left, [0; 9]);
    }

    #[test]
    fn tokens_read_as_coded() {
        // The largest category-6 value, negative; a zero; a one; the end.
        let mut e = crate::boolread::test_encoder::Encoder::new();
        for bit in [true; 7] {
            // Not the end, not zero, not one, high, categories 3 to 6,
            // category 5 or 6, category 6.
            e.write(128, bit);
        }
        for &p in crate::tables::CAT6.iter().take_while(|&&p| p != 0) {
            e.write(p, true);
        }
        // The sign, which `read_signed` reads as an even-odds decision
        // reads it whenever a range follows a token.
        e.write(128, true);
        e.write(128, true); // not the end
        e.write(128, false); // a zero
        e.write(128, true); // not zero
        e.write(128, false); // one
        e.write(128, false); // positive
        e.write(128, false); // the end
        let data = e.finish();
        let mut bc = BoolDecoder::new(&data);
        let probs = [[[128u8; 11]; 3]; 8];
        let mut out = [0i16; 16];
        assert_eq!(get_coeffs(&mut bc, &probs, 0, 0, &mut out), 3);
        let mut want = [0i16; 16];
        want[usize::from(ZIGZAG[0])] = -(67 + 2047);
        want[usize::from(ZIGZAG[2])] = 1;
        assert_eq!(out, want);
    }
}
