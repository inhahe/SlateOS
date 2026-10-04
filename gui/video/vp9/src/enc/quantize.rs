//! Quantisation: a transform block's coefficients divided down to the
//! levels the bitstream carries, and multiplied back up to what the decoder
//! will reconstruct from them.
//!
//! libvpx has two quantisers. The *fast* one (`vp9_quantize_fp`), which its
//! realtime mode uses, rounds each coefficient and divides; the *regular*
//! one (`vpx_quantize_b`) first drops coefficients inside a dead zone (the
//! "zero bin"), and divides with a reciprocal and a shift that rounds as an
//! exact division would. 32x32 blocks have their own forms of each, since
//! their coefficients carry one more bit of scale.
//!
//! The arithmetic follows libvpx's 8-bit build, the one `vpxenc` ships:
//! each coefficient plus its rounding is clamped to 16 bits before it is
//! multiplied. Quantised and dequantised values are kept in 32 bits, as the
//! decoder keeps them, so the encoder reconstructs exactly what the decoder
//! will.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_quantize.c`
//! and `vpx_dsp/quantize.c` (copyright the WebM project authors), used under
//! libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "coefficients and their rounding are clamped to 16 bits before any product, and every factor is below 2^16"
)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "the narrowings are libvpx's own int16_t stores of values its tables keep in range"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "the indices are 0 or 1 -- DC or AC -- into two-element arrays, and a scan's entries into its inverse, which is built at compile time (an entry out of range fails the build)"
)]

use crate::common::{ADST_DCT, DCT_ADST, TX_4X4, TX_8X8, TX_16X16, TxSize, TxType};
use crate::header::{ac_quant, dc_quant};
use crate::tables;

/// The number of quantiser indices: libvpx's `QINDEX_RANGE`.
pub(crate) const QINDEX_RANGE: usize = 256;

/// One plane's quantiser at one index, DC then AC: libvpx's
/// `macroblock_plane` fields and its dequantiser.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct QuantSet {
    /// The regular quantiser's reciprocal, less 2^16, and its shift.
    pub quant: [i16; 2],
    pub quant_shift: [i16; 2],
    /// The fast quantiser's reciprocal (2^16 / q) and rounding.
    pub quant_fp: [i16; 2],
    pub round_fp: [i16; 2],
    /// The regular quantiser's dead zone and rounding.
    pub zbin: [i16; 2],
    pub round: [i16; 2],
    /// The step itself: what a level is multiplied by to reconstruct.
    pub dequant: [i16; 2],
}

/// Every index's quantisers for luma and chroma: libvpx's `QUANTS` and its
/// `y_dequant`/`uv_dequant`, as `vp9_init_quantizer` fills them.
#[derive(Clone, Debug)]
pub(crate) struct Quants {
    pub y: Vec<QuantSet>,
    pub uv: Vec<QuantSet>,
}

/// The frame's quantiser deltas: libvpx's `y_dc_delta_q`, `uv_dc_delta_q`
/// and `uv_ac_delta_q`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Deltas {
    pub y_dc: i32,
    pub uv_dc: i32,
    pub uv_ac: i32,
}

/// libvpx's `invert_quant`: `d`'s reciprocal as a 16-bit multiplier (less
/// 2^16) and a shift, so that `((x * quant >> 16) + x) * shift >> 16` is
/// `x / d`.
fn invert_quant(d: i32) -> (i16, i16) {
    let l = 31 - (d.max(1) as u32).leading_zeros() as i32;
    let m = 1 + (1i64 << (16 + l)) / i64::from(d.max(1));
    ((m - (1 << 16)) as i16, (1i32 << (16 - l)) as i16)
}

/// libvpx's `get_qzbin_factor`, for 8-bit streams.
fn qzbin_factor(q: i32) -> i32 {
    let quant = i32::from(dc_quant(q, 0, 8));
    if q == 0 {
        64
    } else if quant < 148 {
        84
    } else {
        80
    }
}

impl Quants {
    /// libvpx's `vp9_init_quantizer` for 8-bit streams. `sharpness` is the
    /// encoder's (0 to 7), which narrows the dead zone and the rounding.
    pub(crate) fn new(deltas: Deltas, sharpness: i32) -> Self {
        let mut y = vec![QuantSet::default(); QINDEX_RANGE];
        let mut uv = vec![QuantSet::default(); QINDEX_RANGE];
        for (q, (ys, uvs)) in y.iter_mut().zip(uv.iter_mut()).enumerate() {
            let q = q as i32;
            let mut zbin_factor = qzbin_factor(q);
            let mut rounding_factor = if q == 0 { 64 } else { 48 };
            let sharpness_adjustment = 16 * (7 - sharpness) / 7;
            if sharpness > 0 && q > 0 {
                zbin_factor = 64 + sharpness_adjustment;
                rounding_factor = 64 - sharpness_adjustment;
            }
            for i in 0..2 {
                let mut rounding_factor_fp = if i == 0 { 48 } else { 42 };
                if q == 0 {
                    rounding_factor_fp = 64;
                }
                if sharpness > 0 {
                    rounding_factor_fp = 64 - sharpness_adjustment;
                }
                let fill = |set: &mut QuantSet, quant: i32| {
                    let (qv, shift) = invert_quant(quant);
                    set.quant[i] = qv;
                    set.quant_shift[i] = shift;
                    set.quant_fp[i] = ((1 << 16) / quant.max(1)) as i16;
                    set.round_fp[i] = ((rounding_factor_fp * quant) >> 7) as i16;
                    set.zbin[i] = ((zbin_factor * quant + 64) >> 7) as i16;
                    set.round[i] = ((rounding_factor * quant) >> 7) as i16;
                    set.dequant[i] = quant as i16;
                };
                let yq = if i == 0 {
                    dc_quant(q, deltas.y_dc, 8)
                } else {
                    ac_quant(q, 0, 8)
                };
                fill(ys, i32::from(yq));
                let uvq = if i == 0 {
                    dc_quant(q, deltas.uv_dc, 8)
                } else {
                    ac_quant(q, deltas.uv_ac, 8)
                };
                fill(uvs, i32::from(uvq));
            }
        }
        Self { y, uv }
    }

    /// The quantisers of `plane` (0 luma) at `qindex`.
    pub(crate) fn get(&self, plane: usize, qindex: usize) -> QuantSet {
        let table = if plane == 0 { &self.y } else { &self.uv };
        table.get(qindex).copied().unwrap_or_default()
    }
}

/// libvpx's `clamp(v, INT16_MIN, INT16_MAX)`.
#[inline(always)]
fn clamp16(v: i32) -> i32 {
    v.clamp(i32::from(i16::MIN), i32::from(i16::MAX))
}

/// A scan's inverse: each raster position's place in coding order, as
/// libvpx's `iscan` tables give it to its SIMD quantisers.
#[allow(
    clippy::cast_sign_loss,
    reason = "a scan is a permutation of 0..N, checked as the tables are built"
)]
const fn inverse<const N: usize>(scan: &[i16; N]) -> [i16; N] {
    let mut out = [0i16; N];
    let mut i = 0;
    while i < N {
        out[scan[i] as usize] = i as i16;
        i += 1;
    }
    out
}

static ISCAN_DEFAULT_4X4: [i16; 16] = inverse(&tables::DEFAULT_SCAN_4X4);
static ISCAN_ROW_4X4: [i16; 16] = inverse(&tables::ROW_SCAN_4X4);
static ISCAN_COL_4X4: [i16; 16] = inverse(&tables::COL_SCAN_4X4);
static ISCAN_DEFAULT_8X8: [i16; 64] = inverse(&tables::DEFAULT_SCAN_8X8);
static ISCAN_ROW_8X8: [i16; 64] = inverse(&tables::ROW_SCAN_8X8);
static ISCAN_COL_8X8: [i16; 64] = inverse(&tables::COL_SCAN_8X8);
static ISCAN_DEFAULT_16X16: [i16; 256] = inverse(&tables::DEFAULT_SCAN_16X16);
static ISCAN_ROW_16X16: [i16; 256] = inverse(&tables::ROW_SCAN_16X16);
static ISCAN_COL_16X16: [i16; 256] = inverse(&tables::COL_SCAN_16X16);
static ISCAN_DEFAULT_32X32: [i16; 1024] = inverse(&tables::DEFAULT_SCAN_32X32);

/// The inverse of the scan a transform's levels are coded in
/// ([`crate::detokenize::scan_for`]'s): what the quantisers take.
pub(crate) fn iscan_for(tx_size: TxSize, tx_type: TxType) -> &'static [i16] {
    match (tx_size, tx_type) {
        (TX_4X4, ADST_DCT) => &ISCAN_ROW_4X4,
        (TX_4X4, DCT_ADST) => &ISCAN_COL_4X4,
        (TX_4X4, _) => &ISCAN_DEFAULT_4X4,
        (TX_8X8, ADST_DCT) => &ISCAN_ROW_8X8,
        (TX_8X8, DCT_ADST) => &ISCAN_COL_8X8,
        (TX_8X8, _) => &ISCAN_DEFAULT_8X8,
        (TX_16X16, ADST_DCT) => &ISCAN_ROW_16X16,
        (TX_16X16, DCT_ADST) => &ISCAN_COL_16X16,
        (TX_16X16, _) => &ISCAN_DEFAULT_16X16,
        _ => &ISCAN_DEFAULT_32X32,
    }
}

/// A coefficient's signed level from its magnitude's.
#[inline(always)]
fn signed(c: i32, magnitude: impl Fn(i32) -> i32) -> i32 {
    let sign = c >> 31;
    let tmp = magnitude((c ^ sign) - sign);
    (tmp ^ sign) - sign
}

/// What libvpx's quantisers share, run over the coefficients in raster
/// order as its SIMD versions run them: each coefficient's signed level from
/// `level` (given the coefficient and 0 for DC, 1 for AC), its
/// reconstruction (halved, as 32x32's are, with `halve`), and the end of
/// block -- one past the furthest nonzero level in coding order, which
/// `iscan` gives. Every level depends on its own coefficient alone, so the
/// order changes nothing but the speed: libvpx's C versions, which walk the
/// scan, give the same levels and end of block.
///
/// The loop is written for the compiler to vectorise: DC first, so the AC
/// loop's constants are fixed, and the end of block a running maximum of
/// masked positions -- a branch on the level would make the position's
/// load conditional, which keeps the loop scalar.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn quantize_raster(
    coeff: &[i32],
    n: usize,
    iscan: &[i16],
    qcoeff: &mut [i32],
    dqcoeff: &mut [i32],
    dequant: [i32; 2],
    halve: bool,
    level: impl Fn(i32, usize) -> i32,
) -> usize {
    debug_assert!(
        coeff.len() >= n && iscan.len() >= n && qcoeff.len() >= n && dqcoeff.len() >= n,
        "every slice holds the block's coefficients"
    );
    let n = n
        .min(coeff.len())
        .min(iscan.len())
        .min(qcoeff.len())
        .min(dqcoeff.len());
    let (Some(coeff), Some(iscan), Some(qcoeff), Some(dqcoeff)) = (
        coeff.get(..n),
        iscan.get(..n),
        qcoeff.get_mut(..n),
        dqcoeff.get_mut(..n),
    ) else {
        return 0;
    };
    // A coefficient's level and reconstruction, stored; returns one past its
    // place in coding order if the level is nonzero, else 0.
    let one = |c: i32, pos: i16, k: usize, q: &mut i32, dq: &mut i32| {
        let l = level(c, k);
        let d = l * dequant[k];
        *q = l;
        *dq = if halve { d / 2 } else { d };
        (i32::from(pos) + 1) & -i32::from(l != 0)
    };
    let (Some((q0, q_ac)), Some((dq0, dq_ac)), Some((&c0, c_ac)), Some((&pos0, pos_ac))) = (
        qcoeff.split_first_mut(),
        dqcoeff.split_first_mut(),
        coeff.split_first(),
        iscan.split_first(),
    ) else {
        return 0;
    };
    let mut eob = one(c0, pos0, 0, q0, dq0);
    for (((q, dq), &c), &pos) in q_ac.iter_mut().zip(dq_ac).zip(c_ac).zip(pos_ac) {
        eob = eob.max(one(c, pos, 1, q, dq));
    }
    usize::try_from(eob).unwrap_or(0)
}

/// libvpx's `vp9_quantize_fp_c` (and, with `is32`, `vp9_quantize_fp_32x32_c`)
/// over the first `n` coefficients, `iscan` their coding order
/// ([`iscan_for`]). Returns the end of block: one past the last nonzero
/// level, in coding order.
pub(crate) fn quantize_fp(
    coeff: &[i32],
    n: usize,
    qs: &QuantSet,
    qcoeff: &mut [i32],
    dqcoeff: &mut [i32],
    iscan: &[i16],
    is32: bool,
) -> usize {
    let dequant = qs.dequant.map(i32::from);
    let quant = qs.quant_fp.map(i32::from);
    let round = qs.round_fp.map(i32::from);
    if is32 {
        // A coefficient below a quarter step is dropped; the rest round by
        // half as much and divide by twice as much.
        let floor = dequant.map(|d| d >> 2);
        let round = round.map(|r| (r + 1) >> 1);
        quantize_raster(coeff, n, iscan, qcoeff, dqcoeff, dequant, true, |c, k| {
            signed(c, |abs| {
                if abs < floor[k] {
                    0
                } else {
                    (clamp16(abs + round[k]) * quant[k]) >> 15
                }
            })
        })
    } else {
        quantize_raster(coeff, n, iscan, qcoeff, dqcoeff, dequant, false, |c, k| {
            signed(c, |abs| (clamp16(abs + round[k]) * quant[k]) >> 16)
        })
    }
}

/// The regular quantiser's level of a coefficient's magnitude: 0 inside the
/// zero bin, else libvpx's reciprocal multiply (`quant` and its
/// `quant_shift` factor) and its final shift, 16 or (32x32) 15.
#[inline(always)]
fn regular_level<const SHIFT: u32>(
    abs: i32,
    zbin: i32,
    round: i32,
    quant: i32,
    quant_shift: i32,
) -> i32 {
    if abs < zbin {
        return 0;
    }
    let x = clamp16(abs + round);
    ((((x * quant) >> 16) + x) * quant_shift) >> SHIFT
}

/// libvpx's `vpx_quantize_b_c` (and, with `is32`, `vpx_quantize_b_32x32_c`)
/// over the first `n` coefficients, `iscan` their coding order. Returns the
/// end of block.
///
/// libvpx's 4x4-to-16x16 form first finds the last coefficient, in coding
/// order, outside the zero bin and quantises up to it; its 32x32 form skips
/// each one inside. Either way a coefficient inside the zero bin is left 0
/// and every other is quantised, which is all this does.
pub(crate) fn quantize_b(
    coeff: &[i32],
    n: usize,
    qs: &QuantSet,
    qcoeff: &mut [i32],
    dqcoeff: &mut [i32],
    iscan: &[i16],
    is32: bool,
) -> usize {
    let dequant = qs.dequant.map(i32::from);
    let quant = qs.quant.map(i32::from);
    let shift = qs.quant_shift.map(i32::from);
    if is32 {
        let zbin = qs.zbin.map(|z| (i32::from(z) + 1) >> 1);
        let round = qs.round.map(|r| (i32::from(r) + 1) >> 1);
        quantize_raster(coeff, n, iscan, qcoeff, dqcoeff, dequant, true, |c, k| {
            signed(c, |abs| {
                regular_level::<15>(abs, zbin[k], round[k], quant[k], shift[k])
            })
        })
    } else {
        let zbin = qs.zbin.map(i32::from);
        let round = qs.round.map(i32::from);
        quantize_raster(coeff, n, iscan, qcoeff, dqcoeff, dequant, false, |c, k| {
            signed(c, |abs| {
                regular_level::<16>(abs, zbin[k], round[k], quant[k], shift[k])
            })
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a test: a failure should be loud"
    )]

    use super::*;
    use crate::detokenize::scan_for;
    use crate::enc::fdct;

    /// `tools/quantize_reference.c`'s output: (quantiser, tx size, hash);
    /// quantiser 0 the fast one, 1 the regular, each over 40 blocks at each
    /// of ten indices.
    const LIBVPX: [(u8, u8, u64); 8] = [
        (0, 0, 0xcff6_7d8f_c6be_f0ed),
        (0, 1, 0x47e9_0923_3062_f7cc),
        (0, 2, 0x5900_33b0_7dfc_452c),
        (0, 3, 0x53e3_be4d_3115_6d51),
        (1, 0, 0x582e_b59d_c769_cfef),
        (1, 1, 0xc232_9823_e6e0_796b),
        (1, 2, 0x7b69_0cc8_d813_5918),
        (1, 3, 0x741e_8fd1_fabd_90e1),
    ];

    const QINDICES: [usize; 10] = [0, 1, 4, 20, 60, 100, 140, 180, 220, 255];

    struct Lcg(u32);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            self.0 >> 8
        }
    }

    fn fnv(bytes: impl IntoIterator<Item = u8>, mut h: u64) -> u64 {
        for b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }

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
    fn matches_libvpx_on_seeded_blocks_at_every_size_and_a_spread_of_indices() {
        let quants = Quants::new(Deltas::default(), 0);
        let mut rng = Lcg(0x9a4a_7123);
        let mut block = vec![0i16; 64 * 64];
        let (mut qcoeff, mut dqcoeff) = (vec![0i32; 1024], vec![0i32; 1024]);
        for &(quantiser, tx, want) in &LIBVPX {
            let n = 4usize << tx;
            let mut h = 0xcbf2_9ce4_8422_2325u64;
            for &q in &QINDICES {
                let qs = quants.get(0, q);
                for _ in 0..40 {
                    fill(&mut rng, &mut block, n);
                    let mut coeff = vec![0i32; n * n];
                    match tx {
                        0 => fdct::fdct4x4(&block, 64, (&mut coeff[..]).try_into().unwrap()),
                        1 => fdct::fdct8x8(&block, 64, (&mut coeff[..]).try_into().unwrap()),
                        2 => fdct::fdct16x16(&block, 64, (&mut coeff[..]).try_into().unwrap()),
                        _ => {
                            fdct::fdct32x32(
                                &block,
                                64,
                                (&mut coeff[..]).try_into().unwrap(),
                                false,
                            );
                        }
                    }
                    let quantise = if quantiser == 0 {
                        quantize_fp
                    } else {
                        quantize_b
                    };
                    let eob = quantise(
                        &coeff,
                        n * n,
                        &qs,
                        &mut qcoeff,
                        &mut dqcoeff,
                        iscan_for(tx, 0),
                        tx == 3,
                    );
                    h = fnv(qcoeff[..n * n].iter().flat_map(|v| v.to_le_bytes()), h);
                    h = fnv(dqcoeff[..n * n].iter().flat_map(|v| v.to_le_bytes()), h);
                    h = fnv((eob as u16).to_le_bytes(), h);
                }
            }
            assert_eq!(h, want, "quantiser {quantiser}, tx size {tx}");
        }
    }

    /// libvpx's C quantisers as they are written, walking the scan: the
    /// fast (`regular` false) or the regular one, each with its 32x32 form.
    /// Returns the end of block.
    #[allow(clippy::too_many_arguments)]
    fn quantize_by_scan(
        regular: bool,
        coeff: &[i32],
        n: usize,
        qs: &QuantSet,
        qcoeff: &mut [i32],
        dqcoeff: &mut [i32],
        scan: &[i16],
        is32: bool,
    ) -> usize {
        qcoeff[..n].fill(0);
        dqcoeff[..n].fill(0);
        let zbin = |k: usize| {
            let z = i32::from(qs.zbin[k]);
            if is32 { (z + 1) >> 1 } else { z }
        };
        // vpx_quantize_b_c's pre-scan from the end, for 4x4 to 16x16.
        let end = if regular && !is32 {
            (0..n)
                .rev()
                .find(|&i| {
                    let rc = scan[i] as usize;
                    let (c, z) = (coeff[rc], zbin(usize::from(rc != 0)));
                    c >= z || c <= -z
                })
                .map_or(0, |i| i + 1)
        } else {
            n
        };
        let mut eob = 0;
        for (i, &rc) in scan.iter().enumerate().take(end) {
            let rc = rc as usize;
            let k = usize::from(rc != 0);
            let c = coeff[rc];
            let sign = c >> 31;
            let abs = (c ^ sign) - sign;
            let tmp = match (regular, is32) {
                (false, false) => {
                    (clamp16(abs + i32::from(qs.round_fp[k])) * i32::from(qs.quant_fp[k])) >> 16
                }
                (false, true) => {
                    if abs < i32::from(qs.dequant[k]) >> 2 {
                        continue;
                    }
                    let round = (i32::from(qs.round_fp[k]) + 1) >> 1;
                    (clamp16(abs + round) * i32::from(qs.quant_fp[k])) >> 15
                }
                (true, _) => {
                    // vpx_quantize_b_c tests the magnitude; the 32x32 form
                    // keeps what its pre-scan found outside the bin.
                    let inside = if is32 {
                        !(c >= zbin(k) || c <= -zbin(k))
                    } else {
                        abs < zbin(k)
                    };
                    if inside {
                        continue;
                    }
                    let round = if is32 {
                        (i32::from(qs.round[k]) + 1) >> 1
                    } else {
                        i32::from(qs.round[k])
                    };
                    let x = clamp16(abs + round);
                    let t =
                        (((x * i32::from(qs.quant[k])) >> 16) + x) * i32::from(qs.quant_shift[k]);
                    if is32 { t >> 15 } else { t >> 16 }
                }
            };
            let q = (tmp ^ sign) - sign;
            qcoeff[rc] = q;
            dqcoeff[rc] = if is32 {
                q * i32::from(qs.dequant[k]) / 2
            } else {
                q * i32::from(qs.dequant[k])
            };
            if tmp != 0 {
                eob = i + 1;
            }
        }
        eob
    }

    /// Each inverse scan inverts its scan.
    #[test]
    fn the_inverse_scans_invert_the_scans() {
        for tx in 0..4u8 {
            for tx_type in 0..4u8 {
                let (scan, iscan) = (scan_for(tx, tx_type).scan, iscan_for(tx, tx_type));
                assert_eq!(scan.len(), 16 << (2 * tx));
                assert_eq!(iscan.len(), scan.len());
                for (i, &rc) in scan.iter().enumerate() {
                    assert_eq!(usize::try_from(iscan[rc as usize]).unwrap(), i);
                }
            }
        }
    }

    /// Both quantisers, in raster order, give libvpx's levels,
    /// reconstructions and end of block -- for every scan (the hybrid
    /// transforms' rows and columns too), every quantiser index, and blocks
    /// from empty to saturating, coefficients up to past 16 bits.
    #[test]
    fn raster_order_quantises_as_the_scan_does() {
        let quants = Quants::new(Deltas::default(), 0);
        let sharp = Quants::new(
            Deltas {
                y_dc: -5,
                uv_dc: 3,
                uv_ac: -2,
            },
            5,
        );
        let mut rng = Lcg(0x51a7_e00d);
        let (mut q, mut dq) = (vec![0i32; 1024], vec![0i32; 1024]);
        let (mut want_q, mut want_dq) = (vec![0i32; 1024], vec![0i32; 1024]);
        for tx in 0..4u8 {
            let n = 16usize << (2 * tx);
            for tx_type in 0..4u8 {
                let scan = scan_for(tx, tx_type).scan;
                let iscan = iscan_for(tx, tx_type);
                // 0, 17, ... 255: both ends and a spread between.
                for qindex in (0..256).step_by(17) {
                    for (table, plane) in [(&quants, 0), (&quants, 1), (&sharp, 0), (&sharp, 1)] {
                        let qs = table.get(plane, qindex);
                        for shape in 0..6 {
                            // 0 empty, 1 small, 2 sparse, 3 dense, 4 at the
                            // 16-bit limits, 5 beyond them.
                            let coeff: Vec<i32> = (0..n)
                                .map(|i| {
                                    let r = rng.next();
                                    match shape {
                                        0 => 0,
                                        1 => (r % 33) as i32 - 16,
                                        2 if !r.is_multiple_of(7) && i != 0 => 0,
                                        2 | 3 => (r % 4001) as i32 - 2000,
                                        4 => [32767, -32768, 32766, -32767][(r % 4) as usize],
                                        _ => (r % 200_001) as i32 - 100_000,
                                    }
                                })
                                .collect();
                            for regular in [false, true] {
                                let is32 = tx == 3;
                                let quantise = if regular { quantize_b } else { quantize_fp };
                                let eob = quantise(&coeff, n, &qs, &mut q, &mut dq, iscan, is32);
                                let want = quantize_by_scan(
                                    regular,
                                    &coeff,
                                    n,
                                    &qs,
                                    &mut want_q,
                                    &mut want_dq,
                                    scan,
                                    is32,
                                );
                                let what = format!(
                                    "tx {tx} type {tx_type} q {qindex} plane {plane} shape {shape} regular {regular}"
                                );
                                assert_eq!(eob, want, "{what}");
                                assert_eq!(q[..n], want_q[..n], "{what}");
                                assert_eq!(dq[..n], want_dq[..n], "{what}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_tables_are_libvpxs_at_their_ends() {
        let quants = Quants::new(Deltas::default(), 0);
        // q 0: the smallest steps, and lossless's rounding of 64/128.
        let q0 = quants.get(0, 0);
        assert_eq!(q0.dequant, [4, 4]);
        assert_eq!(q0.quant_fp, [16384, 16384]);
        assert_eq!(q0.round_fp, [2, 2]);
        assert_eq!(q0.zbin, [2, 2]);
        // q 255: the largest.
        let q255 = quants.get(0, 255);
        assert_eq!(q255.dequant, [1336, 1828]);
        assert_eq!(
            q255.quant_fp,
            [(65536 / 1336) as i16, (65536 / 1828) as i16]
        );
    }
}
