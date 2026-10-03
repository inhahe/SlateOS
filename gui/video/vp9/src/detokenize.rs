//! Coefficient tokens: a transform block's quantised coefficients, read with
//! the bool decoder and dequantised.
//!
//! A block's coefficients are coded in a scan order -- zig-zag-like for DCT
//! blocks, by rows or columns for the hybrids -- as tokens: end of block,
//! zero, one, two, three, four, and six categories of larger values with
//! extra bits. Each token is decoded against probabilities chosen by its
//! frequency band and by a context: how large its already-decoded neighbours
//! were. The decoder dequantises as it goes, so what it leaves behind is what
//! the inverse transform takes.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/decoder/vp9_detokenize.c`,
//! `vp9/common/vp9_scan.c`, `vp9_scan.h` and `vp9_entropy.c` (copyright the
//! WebM project authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

use crate::boolread::BoolReader;
use crate::common::{
    ADST_DCT, COEF_BANDS, COEFF_CONTEXTS, DCT_ADST, PIVOT_NODE, TX_4X4, TX_8X8, TX_16X16, TX_32X32,
    TxSize, TxType, UNCONSTRAINED_NODES,
};
use crate::tables;

/// A scan order: the raster position of each coefficient in coding order,
/// and for each, the two earlier positions whose tokens set its context.
#[derive(Clone, Copy, Debug)]
pub struct Scan {
    pub scan: &'static [i16],
    pub neighbors: &'static [i16],
}

/// The scan a transform uses: libvpx's `vp9_scan_orders`. A vertical ADST
/// scans by rows, a horizontal one by columns; 32x32 is always the default.
#[must_use]
pub fn scan_for(tx_size: TxSize, tx_type: TxType) -> Scan {
    let (scan, neighbors): (&'static [i16], &'static [i16]) = match (tx_size, tx_type) {
        (TX_4X4, ADST_DCT) => (&tables::ROW_SCAN_4X4, &tables::ROW_SCAN_4X4_NEIGHBORS),
        (TX_4X4, DCT_ADST) => (&tables::COL_SCAN_4X4, &tables::COL_SCAN_4X4_NEIGHBORS),
        (TX_4X4, _) => (
            &tables::DEFAULT_SCAN_4X4,
            &tables::DEFAULT_SCAN_4X4_NEIGHBORS,
        ),
        (TX_8X8, ADST_DCT) => (&tables::ROW_SCAN_8X8, &tables::ROW_SCAN_8X8_NEIGHBORS),
        (TX_8X8, DCT_ADST) => (&tables::COL_SCAN_8X8, &tables::COL_SCAN_8X8_NEIGHBORS),
        (TX_8X8, _) => (
            &tables::DEFAULT_SCAN_8X8,
            &tables::DEFAULT_SCAN_8X8_NEIGHBORS,
        ),
        (TX_16X16, ADST_DCT) => (&tables::ROW_SCAN_16X16, &tables::ROW_SCAN_16X16_NEIGHBORS),
        (TX_16X16, DCT_ADST) => (&tables::COL_SCAN_16X16, &tables::COL_SCAN_16X16_NEIGHBORS),
        (TX_16X16, _) => (
            &tables::DEFAULT_SCAN_16X16,
            &tables::DEFAULT_SCAN_16X16_NEIGHBORS,
        ),
        _ => (
            &tables::DEFAULT_SCAN_32X32,
            &tables::DEFAULT_SCAN_32X32_NEIGHBORS,
        ),
    };
    Scan { scan, neighbors }
}

/// The probabilities one block decodes with: `coef_probs[tx][plane][ref]`.
pub type BlockProbs = [[[u8; UNCONSTRAINED_NODES]; COEFF_CONTEXTS]; COEF_BANDS];

/// The counts one block adds to: token counts by band and context (zero,
/// one, two-or-more, end of block), and how often each band and context
/// reached an end-of-block decision.
pub struct BlockCounts<'a> {
    pub tokens: &'a mut [[[u32; UNCONSTRAINED_NODES + 1]; COEFF_CONTEXTS]; COEF_BANDS],
    pub eob_branch: &'a mut [[u32; COEFF_CONTEXTS]; COEF_BANDS],
}

/// The extra-bit probabilities of categories 1 to 5.
const CAT1_PROB: [u8; 1] = [159];
const CAT2_PROB: [u8; 2] = [165, 145];
const CAT3_PROB: [u8; 3] = [173, 148, 140];
const CAT4_PROB: [u8; 4] = [176, 155, 140, 135];
const CAT5_PROB: [u8; 5] = [180, 157, 141, 134, 130];

/// The smallest value of each category.
const CAT1_MIN_VAL: i32 = 5;
const CAT2_MIN_VAL: i32 = 7;
const CAT3_MIN_VAL: i32 = 11;
const CAT4_MIN_VAL: i32 = 19;
const CAT5_MIN_VAL: i32 = 35;
const CAT6_MIN_VAL: i32 = 67;

/// Token counts' indices: libvpx's `ZERO_TOKEN`, `ONE_TOKEN`, `TWO_TOKEN`
/// (two or more) and `EOB_MODEL_TOKEN`.
const ZERO_TOKEN: usize = 0;
const ONE_TOKEN: usize = 1;
const TWO_TOKEN: usize = 2;
const EOB_MODEL_TOKEN: usize = 3;

/// Bits read MSB first against `probs`: libvpx's `read_coeff`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "at most 18 bits shifted into an i32"
)]
fn read_coeff(r: &mut BoolReader<'_>, probs: &[u8]) -> i32 {
    let mut val = 0i32;
    for &p in probs {
        val = (val << 1) | r.read(p) as i32;
    }
    val
}

/// A coefficient's context: the rounded mean of its two neighbours' energy
/// classes. libvpx's `get_coef_context`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "energy classes are at most 5 and c at most 1024"
)]
#[inline(always)]
fn coef_context(neighbors: &[i16], token_cache: &[u8; 1024], c: usize) -> usize {
    let cache = |i: usize| {
        neighbors
            .get(i)
            .and_then(|&pos| token_cache.get(pos as usize))
            .copied()
            .unwrap_or(0)
    };
    (1 + usize::from(cache(2 * c)) + usize::from(cache(2 * c + 1))) >> 1
}

/// Decode one transform block's tokens into `dqcoeff`, dequantising with
/// `dq` (DC, then AC): libvpx's `decode_coefs`. Returns the end of block --
/// one past the last coefficient coded.
///
/// `ctx` is the block's initial context, from the blocks above and to its
/// left. `dqcoeff` holds the block's coefficients in raster order and must be
/// zero where nothing is coded: only nonzero tokens are written.
/// `token_cache` is scratch the caller keeps between blocks; each read of it
/// is of a position this block wrote first.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "positions are bounded by max_eob (at most 1024); values are 18-bit categories times 16-bit quantisers, computed in i64"
)]
pub fn decode_coefs(
    r: &mut BoolReader<'_>,
    probs: &BlockProbs,
    mut counts: Option<BlockCounts<'_>>,
    tx_size: TxSize,
    dq: [i16; 2],
    mut ctx: usize,
    scan: &Scan,
    bit_depth: u8,
    dqcoeff: &mut [i32],
    token_cache: &mut [u8; 1024],
) -> usize {
    let max_eob = 16usize << (tx_size.min(TX_32X32) << 1);
    let band_translate: &[u8] = if tx_size == TX_4X4 {
        &tables::COEFBAND_TRANS_4X4
    } else {
        &tables::COEFBAND_TRANS_8X8PLUS
    };
    let dq_shift = u32::from(tx_size == TX_32X32);
    let (cat6_prob, cat6_bits): (&[u8], usize) = match bit_depth {
        12 => (&tables::CAT6_PROB_HIGH12, 18),
        10 => (tables::CAT6_PROB_HIGH12.get(2..).unwrap_or(&[]), 16),
        _ => (tables::CAT6_PROB_HIGH12.get(4..).unwrap_or(&[]), 14),
    };
    let band =
        |c: usize| usize::from(band_translate.get(c).copied().unwrap_or(5)).min(COEF_BANDS - 1);
    let prob_at = |band: usize, ctx: usize| -> [u8; UNCONSTRAINED_NODES] {
        probs
            .get(band)
            .and_then(|b| b.get(ctx))
            .copied()
            .unwrap_or([128; UNCONSTRAINED_NODES])
    };
    let count = |counts: &mut Option<BlockCounts<'_>>, band: usize, ctx: usize, token: usize| {
        if let Some(c) = counts
            && let Some(slot) = c
                .tokens
                .get_mut(band)
                .and_then(|b| b.get_mut(ctx))
                .and_then(|t| t.get_mut(token))
        {
            *slot = slot.wrapping_add(1);
        }
    };

    let mut dqv = i32::from(dq[0]);
    let mut c = 0usize;
    while c < max_eob {
        let mut b = band(c);
        let mut prob = prob_at(b, ctx);
        if let Some(cs) = &mut counts
            && let Some(slot) = cs.eob_branch.get_mut(b).and_then(|e| e.get_mut(ctx))
        {
            *slot = slot.wrapping_add(1);
        }
        if !r.read_bool(prob[0]) {
            count(&mut counts, b, ctx, EOB_MODEL_TOKEN);
            break;
        }
        // Zeros, until something else.
        while !r.read_bool(prob[1]) {
            count(&mut counts, b, ctx, ZERO_TOKEN);
            dqv = i32::from(dq[1]);
            let pos = scan.scan.get(c).map_or(0, |&p| p as usize);
            if let Some(t) = token_cache.get_mut(pos) {
                *t = 0;
            }
            c += 1;
            if c >= max_eob {
                return c; // Zeros to the end: no end-of-block token.
            }
            ctx = coef_context(scan.neighbors, token_cache, c);
            b = band(c);
            prob = prob_at(b, ctx);
        }

        let pos = scan.scan.get(c).map_or(0, |&p| p as usize);
        let (energy, v) = if r.read_bool(prob[PIVOT_NODE]) {
            // Two or more: the Pareto model's tail.
            let p = tables::PARETO8_FULL
                .get(usize::from(prob[PIVOT_NODE].saturating_sub(1)))
                .copied()
                .unwrap_or([128; 8]);
            count(&mut counts, b, ctx, TWO_TOKEN);
            if r.read_bool(p[0]) {
                let (energy, val) = if r.read_bool(p[3]) {
                    let val = if r.read_bool(p[5]) {
                        if r.read_bool(p[7]) {
                            CAT6_MIN_VAL + read_coeff(r, cat6_prob.get(..cat6_bits).unwrap_or(&[]))
                        } else {
                            CAT5_MIN_VAL + read_coeff(r, &CAT5_PROB)
                        }
                    } else if r.read_bool(p[6]) {
                        CAT4_MIN_VAL + read_coeff(r, &CAT4_PROB)
                    } else {
                        CAT3_MIN_VAL + read_coeff(r, &CAT3_PROB)
                    };
                    (5, val)
                } else {
                    let val = if r.read_bool(p[4]) {
                        CAT2_MIN_VAL + read_coeff(r, &CAT2_PROB)
                    } else {
                        CAT1_MIN_VAL + read_coeff(r, &CAT1_PROB)
                    };
                    (4, val)
                };
                // The value may use 18 bits: libvpx multiplies in 64.
                (
                    energy,
                    ((i64::from(val) * i64::from(dqv)) >> dq_shift) as i32,
                )
            } else if r.read_bool(p[1]) {
                let three_or_four = 3 + r.read(p[2]) as i32;
                (3, (three_or_four * dqv) >> dq_shift)
            } else {
                (2, (2 * dqv) >> dq_shift)
            }
        } else {
            count(&mut counts, b, ctx, ONE_TOKEN);
            (1, dqv >> dq_shift)
        };
        if let Some(t) = token_cache.get_mut(pos) {
            *t = energy;
        }
        let v = if r.read_bit() == 1 {
            v.wrapping_neg()
        } else {
            v
        };
        if let Some(d) = dqcoeff.get_mut(pos) {
            *d = v;
        }
        c += 1;
        ctx = coef_context(scan.neighbors, token_cache, c);
        dqv = i32::from(dq[1]);
    }
    c
}

/// Zero the coefficients a block's scan wrote, so the buffer is ready for
/// the next block: what libvpx's `inverse_transform_block_*` clears.
pub fn clear_coefs(dqcoeff: &mut [i32], scan: &Scan, eob: usize) {
    for &pos in scan.scan.iter().take(eob) {
        if let Some(d) = dqcoeff.get_mut(pos as usize) {
            *d = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation
    )]

    use super::*;
    use crate::common::{ADST_ADST, DCT_DCT};

    #[test]
    fn scans_follow_the_transform_direction() {
        assert_eq!(
            scan_for(TX_4X4, DCT_DCT).scan,
            &tables::DEFAULT_SCAN_4X4[..]
        );
        assert_eq!(
            scan_for(TX_4X4, ADST_ADST).scan,
            &tables::DEFAULT_SCAN_4X4[..]
        );
        assert_eq!(scan_for(TX_8X8, ADST_DCT).scan, &tables::ROW_SCAN_8X8[..]);
        assert_eq!(
            scan_for(TX_16X16, DCT_ADST).scan,
            &tables::COL_SCAN_16X16[..]
        );
        assert_eq!(
            scan_for(TX_32X32, ADST_DCT).scan,
            &tables::DEFAULT_SCAN_32X32[..]
        );
    }

    #[test]
    fn every_neighbour_was_scanned_before_its_coefficient() {
        // decode_coefs relies on it: a context reads only tokens this block
        // has already decoded.
        for tx in [TX_4X4, TX_8X8, TX_16X16, TX_32X32] {
            for tx_type in [DCT_DCT, ADST_DCT, DCT_ADST] {
                let s = scan_for(tx, tx_type);
                let n = 16 << (tx << 1);
                let mut seen = vec![false; n];
                for c in 0..n {
                    if c > 0 {
                        for k in 0..2 {
                            let pos = s.neighbors[2 * c + k] as usize;
                            assert!(seen[pos], "{tx}/{tx_type}: position {c} reads {pos} early");
                        }
                    }
                    seen[s.scan[c] as usize] = true;
                }
            }
        }
    }

    #[test]
    fn an_empty_block_reads_one_decision_and_counts_it() {
        // A stream of zeros decodes every bool as 0, so the first EOB check
        // says "end of block".
        let mut r = BoolReader::new(&[0u8; 16]).unwrap();
        let probs = [[[128u8; 3]; 6]; 6];
        let mut tokens = [[[0u32; 4]; 6]; 6];
        let mut eob_branch = [[0u32; 6]; 6];
        let mut dq = vec![0i32; 16];
        let mut cache = [0u8; 1024];
        let eob = decode_coefs(
            &mut r,
            &probs,
            Some(BlockCounts {
                tokens: &mut tokens,
                eob_branch: &mut eob_branch,
            }),
            TX_4X4,
            [8, 10],
            1,
            &scan_for(TX_4X4, DCT_DCT),
            8,
            &mut dq,
            &mut cache,
        );
        assert_eq!(eob, 0);
        assert_eq!(eob_branch[0][1], 1);
        assert_eq!(tokens[0][1][EOB_MODEL_TOKEN], 1);
        assert!(dq.iter().all(|&v| v == 0));
    }
}
