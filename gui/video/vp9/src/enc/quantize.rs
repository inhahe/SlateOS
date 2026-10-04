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
    reason = "the only indices are 0 or 1 -- DC or AC, `usize::from(rc != 0)` or the table builder's loop -- into two-element arrays"
)]

use crate::header::{ac_quant, dc_quant};

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

/// The coefficient at scan position `i`: its raster index and value, and
/// which of the DC and AC quantisers applies.
#[inline(always)]
fn at(coeff: &[i32], scan: &[i16], i: usize) -> Option<(usize, i32, usize)> {
    let rc = usize::try_from(*scan.get(i)?).ok()?;
    Some((rc, *coeff.get(rc)?, usize::from(rc != 0)))
}

/// Store a quantised level and its reconstruction.
#[inline(always)]
fn put(qcoeff: &mut [i32], dqcoeff: &mut [i32], rc: usize, q: i32, dq: i32) {
    if let (Some(qc), Some(dqc)) = (qcoeff.get_mut(rc), dqcoeff.get_mut(rc)) {
        *qc = q;
        *dqc = dq;
    }
}

/// libvpx's `vp9_quantize_fp_c` (and, with `is32`, `vp9_quantize_fp_32x32_c`)
/// over the first `n` scan positions. Returns the end of block: one past the
/// last nonzero level, in scan order.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "libvpx's realtime speeds quantise inter blocks with this (use_quant_fp off key frames); it is used when the encoder's inter frames are, and checked against libvpx now"
    )
)]
pub(crate) fn quantize_fp(
    coeff: &[i32],
    n: usize,
    qs: &QuantSet,
    qcoeff: &mut [i32],
    dqcoeff: &mut [i32],
    scan: &[i16],
    is32: bool,
) -> usize {
    qcoeff.iter_mut().take(n).for_each(|v| *v = 0);
    dqcoeff.iter_mut().take(n).for_each(|v| *v = 0);
    let mut eob = 0;
    for i in 0..n {
        let Some((rc, c, k)) = at(coeff, scan, i) else {
            continue;
        };
        let sign = c >> 31;
        let abs = (c ^ sign) - sign;
        let tmp = if is32 {
            if abs < i32::from(qs.dequant[k]) >> 2 {
                continue;
            }
            let abs = clamp16(abs + ((i32::from(qs.round_fp[k]) + 1) >> 1));
            (abs * i32::from(qs.quant_fp[k])) >> 15
        } else {
            (clamp16(abs + i32::from(qs.round_fp[k])) * i32::from(qs.quant_fp[k])) >> 16
        };
        let q = (tmp ^ sign) - sign;
        let dq = if is32 {
            q * i32::from(qs.dequant[k]) / 2
        } else {
            q * i32::from(qs.dequant[k])
        };
        put(qcoeff, dqcoeff, rc, q, dq);
        if tmp != 0 {
            eob = i + 1;
        }
    }
    eob
}

/// libvpx's `vpx_quantize_b_c` (and, with `is32`, `vpx_quantize_b_32x32_c`)
/// over the first `n` scan positions. Returns the end of block.
pub(crate) fn quantize_b(
    coeff: &[i32],
    n: usize,
    qs: &QuantSet,
    qcoeff: &mut [i32],
    dqcoeff: &mut [i32],
    scan: &[i16],
    is32: bool,
) -> usize {
    qcoeff.iter_mut().take(n).for_each(|v| *v = 0);
    dqcoeff.iter_mut().take(n).for_each(|v| *v = 0);
    let zbins = if is32 {
        [
            (i32::from(qs.zbin[0]) + 1) >> 1,
            (i32::from(qs.zbin[1]) + 1) >> 1,
        ]
    } else {
        [i32::from(qs.zbin[0]), i32::from(qs.zbin[1])]
    };
    let outside =
        |i: usize| at(coeff, scan, i).is_some_and(|(_, c, k)| c >= zbins[k] || c <= -zbins[k]);
    // The 4x4 to 16x16 form stops at the last coefficient outside the zero
    // bin (libvpx's pre-scan from the end); the 32x32 form skips every one
    // inside it. Both quantise the same coefficients.
    let end = if is32 {
        n
    } else {
        (0..n).rev().find(|&i| outside(i)).map_or(0, |i| i + 1)
    };
    let mut eob = 0;
    for i in 0..end {
        let Some((rc, c, k)) = at(coeff, scan, i) else {
            continue;
        };
        if is32 && !outside(i) {
            continue;
        }
        let sign = c >> 31;
        let abs = (c ^ sign) - sign;
        if !is32 && abs < zbins[k] {
            continue;
        }
        let round = if is32 {
            (i32::from(qs.round[k]) + 1) >> 1
        } else {
            i32::from(qs.round[k])
        };
        let x = clamp16(abs + round);
        let shift = if is32 { 15 } else { 16 };
        let tmp =
            ((((x * i32::from(qs.quant[k])) >> 16) + x) * i32::from(qs.quant_shift[k])) >> shift;
        let q = (tmp ^ sign) - sign;
        let dq = if is32 {
            q * i32::from(qs.dequant[k]) / 2
        } else {
            q * i32::from(qs.dequant[k])
        };
        put(qcoeff, dqcoeff, rc, q, dq);
        if tmp != 0 {
            eob = i + 1;
        }
    }
    eob
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
            let scan = scan_for(tx, 0);
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
                        scan.scan,
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
