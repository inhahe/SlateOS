//! A CELT frame's band shapes (RFC 6716 §4.3.4): each band's coefficients
//! decoded as PVQ codewords -- split in two, recursively, when a band has more
//! bits than one codeword can use; mid and side for stereo; folded from the
//! bands below, or noise, where it has no pulses -- then scaled by the band
//! energies (`denormalise_bands`), with the anti-collapse noise transients
//! need.
//!
//! **Buffers.** libopus folds from a `norm` buffer that a band's own output
//! may overlap (the second band can fold from data the first band's output
//! occupies, `special_hybrid_folding`), and uses the frame's last band as
//! scratch space. Here the band folded from is copied out before the band is
//! decoded, which is what libopus's own scratch copy does wherever the copy
//! is changed, and reads the same values wherever it is not: the decoded
//! samples are the same.
//!
//! Translated into Rust from libopus 1.5.2's `celt/bands.c` (the decoder's
//! half) and `celt/pitch.h` (`dual_inner_prod`), `FIXED_POINT`, copyright
//! Xiph.Org, Jean-Marc Valin and the contributors named in its `COPYING`,
//! used under libopus's BSD licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "every index is within a band (its N coefficients, at most 176 for a 20 ms frame) or the frame's buffers of 960 coefficients a channel, at the band edges `M * eBands[i]` libopus computes; the per-band tables hold NB_EBANDS entries per channel"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "bit counts in eighths of a frame's at most 10,200 bits, sizes below 1000, and libopus's Q14/Q15 sample arithmetic on 16-bit values, within 32 bits"
)]

use super::energy::E_MEANS;
use super::mathops::{exp2, exp2_frac, ilog2, isqrt32, rsqrt_norm, sqrt};
use super::mode::{
    NB_EBANDS, SHORT_MDCT_SIZE, bits2pulses, cache_top, ebands, get_pulses, log_n, pulses2bits,
};
use super::vq::{alg_unquant, renormalise_vector};
use crate::entdec::{BITRES, Decoder, ilog};
use crate::fixed::{
    NORM_SCALING, Q15ONE, add16, extract16, half32, mac16_16, mult16_16, mult16_16_p15,
    mult16_16_q14, mult16_16_q15, mult16_32_q15, pshr32, qconst16, qconst32, saturate16, shl16,
    shl32, sub16, vshr32,
};

const BITRES_I: i32 = BITRES as i32;
const QTHETA_OFFSET: i32 = 4;
const QTHETA_OFFSET_TWOPHASE: i32 = 16;
/// `SPREAD_AGGRESSIVE`.
pub(crate) const SPREAD_AGGRESSIVE: i32 = 3;
/// `DB_SHIFT`.
const DB_SHIFT: i32 = 10;

/// `celt_lcg_rand`: the decoder's noise generator.
pub(crate) const fn lcg_rand(seed: u32) -> u32 {
    seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// `FRAC_MUL16`.
#[inline]
fn frac_mul16(a: i32, b: i32) -> i32 {
    (16384 + (a as i16 as i32) * (b as i16 as i32)) >> 15
}

/// `bitexact_cos`: a cosine exact on every platform, which the bit
/// allocation depends on.
pub(crate) fn bitexact_cos(x: i32) -> i32 {
    let x = x as i16 as i32;
    let tmp = (4096 + x * x) >> 13;
    let x2 = tmp as i16 as i32;
    let x2 = ((32767 - x2) + frac_mul16(x2, -7651 + frac_mul16(x2, 8277 + frac_mul16(-626, x2))))
        as i16 as i32;
    (1 + x2) as i16 as i32
}

/// `bitexact_log2tan`.
pub(crate) fn bitexact_log2tan(isin: i32, icos: i32) -> i32 {
    let lc = ilog(icos as u32);
    let ls = ilog(isin as u32);
    let icos = shl32(icos, (15 - lc) as u32);
    let isin = shl32(isin, (15 - ls) as u32);
    (ls - lc) * (1 << 11) + frac_mul16(isin, frac_mul16(isin, -2597) + 7932)
        - frac_mul16(icos, frac_mul16(icos, -2597) + 7932)
}

/// `haar1`: one level of the Haar transform across `stride` interleaved
/// blocks of `n0` coefficients.
fn haar1(x: &mut [i32], n0: usize, stride: usize) {
    // libopus's `.70710678f`, the same `f32` as the constant.
    let k = qconst16(std::f32::consts::FRAC_1_SQRT_2, 15);
    let n0 = n0 >> 1;
    for i in 0..stride {
        for j in 0..n0 {
            let a = stride * 2 * j + i;
            let b = stride * (2 * j + 1) + i;
            let tmp1 = mult16_16(k, x[a]);
            let tmp2 = mult16_16(k, x[b]);
            x[a] = extract16(pshr32(tmp1.wrapping_add(tmp2), 15));
            x[b] = extract16(pshr32(tmp1.wrapping_sub(tmp2), 15));
        }
    }
}

/// The widest band, in coefficients: the last of the 20 ms mode's, 22
/// bins at 8 short blocks. Every band-sized buffer is no larger -- the sizes
/// come from the mode's tables, never from a stream.
pub(crate) const MAX_BAND: usize = 176;

/// `ordery_table`: the order of the Hadamard blocks, for 2, 4, 8 and 16.
const ORDERY_TABLE: [usize; 30] = [
    1, 0, 3, 0, 2, 1, 7, 0, 4, 3, 6, 1, 5, 2, 15, 0, 8, 7, 12, 3, 11, 4, 14, 1, 9, 6, 13, 2, 10, 5,
];

/// `deinterleave_hadamard`: from interleaved blocks to blocks one after
/// another (in the Hadamard order where `hadamard`).
fn deinterleave_hadamard(x: &mut [i32], n0: usize, stride: usize, hadamard: bool) {
    let n = n0 * stride;
    let mut tmp = [0i32; MAX_BAND];
    let tmp = &mut tmp[..n];
    for i in 0..stride {
        let row = if hadamard {
            ORDERY_TABLE[stride - 2 + i]
        } else {
            i
        };
        for j in 0..n0 {
            tmp[row * n0 + j] = x[j * stride + i];
        }
    }
    x[..n].copy_from_slice(tmp);
}

/// `interleave_hadamard`: the inverse of [`deinterleave_hadamard`].
fn interleave_hadamard(x: &mut [i32], n0: usize, stride: usize, hadamard: bool) {
    let n = n0 * stride;
    let mut tmp = [0i32; MAX_BAND];
    let tmp = &mut tmp[..n];
    for i in 0..stride {
        let row = if hadamard {
            ORDERY_TABLE[stride - 2 + i]
        } else {
            i
        };
        for j in 0..n0 {
            tmp[j * stride + i] = x[row * n0 + j];
        }
    }
    x[..n].copy_from_slice(tmp);
}

/// `compute_qn`: the resolution of a split's angle.
fn compute_qn(n: i32, b: i32, offset: i32, pulse_cap: i32, stereo: bool) -> i32 {
    const EXP2_TABLE8: [i32; 8] = [16384, 17866, 19483, 21247, 23170, 25267, 27554, 30048];
    let mut n2 = 2 * n - 1;
    if stereo && n == 2 {
        n2 -= 1;
    }
    // `celt_sudiv`: C's division, truncating toward zero.
    let mut qb = (b + n2 * offset).checked_div(n2).unwrap_or(0);
    qb = qb.min(b - pulse_cap - (4 << BITRES));
    qb = qb.min(8 << BITRES);
    if qb < (1 << BITRES >> 1) {
        1
    } else {
        let qn = EXP2_TABLE8[(qb & 0x7) as usize] >> (14 - (qb >> BITRES));
        (qn + 1) >> 1 << 1
    }
}

/// What a band decode shares: `band_ctx`, decoding (its `avoid_split_noise`
/// and `theta_round` are the encoder's, and `resynth` always true here).
struct BandCtx<'d, 'a> {
    ec: &'d mut Decoder<'a>,
    /// The band being decoded.
    i: usize,
    /// The first band coded as intensity stereo.
    intensity: i32,
    spread: i32,
    tf_change: i32,
    /// The bits left in the frame, in eighths.
    remaining_bits: i32,
    /// The noise generator's state.
    seed: u32,
    /// Never invert a stereo band's side (for decoders that downmix).
    disable_inv: bool,
}

/// A split's angle and what follows from it: `split_ctx`.
struct Split {
    inv: bool,
    imid: i32,
    iside: i32,
    delta: i32,
    itheta: i32,
    qalloc: i32,
}

/// `compute_theta`, decoding: the angle of a split of `n` coefficients
/// (`b` the bits for both halves, spent by the angle), and `fill` masked by
/// it.
fn compute_theta(
    ctx: &mut BandCtx<'_, '_>,
    n: i32,
    b: &mut i32,
    big_b: i32,
    b0: i32,
    lm: i32,
    stereo: bool,
    fill: &mut i32,
) -> Split {
    let i = ctx.i;
    let pulse_cap = log_n(i) + lm * (1 << BITRES);
    let offset = (pulse_cap >> 1)
        - if stereo && n == 2 {
            QTHETA_OFFSET_TWOPHASE
        } else {
            QTHETA_OFFSET
        };
    let mut qn = compute_qn(n, *b, offset, pulse_cap, stereo);
    if stereo && i as i32 >= ctx.intensity {
        qn = 1;
    }
    let tell = ctx.ec.tell_frac() as i32;
    let mut itheta = 0;
    let mut inv = false;
    if qn != 1 {
        if stereo && n > 2 {
            // A step: probability p0 up to the middle, 1 after.
            let p0 = 3;
            let x0 = qn / 2;
            let ft = p0 * (x0 + 1) + x0;
            let fs = ctx.ec.decode(ft as u32) as i32;
            let x = if fs < (x0 + 1) * p0 {
                fs / p0
            } else {
                x0 + 1 + (fs - (x0 + 1) * p0)
            };
            let (fl, fh) = if x <= x0 {
                (p0 * x, p0 * (x + 1))
            } else {
                ((x - 1 - x0) + (x0 + 1) * p0, (x - x0) + (x0 + 1) * p0)
            };
            ctx.ec.update(fl as u32, fh as u32, ft as u32);
            itheta = x;
        } else if b0 > 1 || stereo {
            // Uniform.
            itheta = ctx.ec.uint((qn + 1) as u32) as i32;
        } else {
            // Triangular.
            let ft = ((qn >> 1) + 1) * ((qn >> 1) + 1);
            let fm = ctx.ec.decode(ft as u32) as i32;
            let (fs, fl);
            if fm < (((qn >> 1) * ((qn >> 1) + 1)) >> 1) {
                itheta = (isqrt32(8 * fm as u32 + 1) as i32 - 1) >> 1;
                fs = itheta + 1;
                fl = (itheta * (itheta + 1)) >> 1;
            } else {
                itheta = (2 * (qn + 1) - isqrt32(8 * (ft - fm - 1) as u32 + 1) as i32) >> 1;
                fs = qn + 1 - itheta;
                fl = ft - (((qn + 1 - itheta) * (qn + 2 - itheta)) >> 1);
            }
            ctx.ec.update(fl as u32, (fl + fs) as u32, ft as u32);
        }
        itheta = ((itheta * 16384) as u32)
            .checked_div(qn as u32)
            .unwrap_or(0) as i32;
    } else if stereo {
        inv = if *b > 2 << BITRES && ctx.remaining_bits > 2 << BITRES {
            ctx.ec.bit_logp(2)
        } else {
            false
        };
        // Overridden to avoid problems with downmixing.
        if ctx.disable_inv {
            inv = false;
        }
        itheta = 0;
    }
    let qalloc = ctx.ec.tell_frac() as i32 - tell;
    *b -= qalloc;
    let (imid, iside, delta);
    if itheta == 0 {
        imid = 32767;
        iside = 0;
        *fill &= (1 << big_b) - 1;
        delta = -16384;
    } else if itheta == 16384 {
        imid = 0;
        iside = 32767;
        *fill &= ((1 << big_b) - 1) << big_b;
        delta = 16384;
    } else {
        imid = bitexact_cos(itheta);
        iside = bitexact_cos(16384 - itheta);
        // The mid and side split that minimises the squared error.
        delta = frac_mul16((n - 1) << 7, bitexact_log2tan(iside, imid));
    }
    Split {
        inv,
        imid,
        iside,
        delta,
        itheta,
        qalloc,
    }
}

/// `quant_band_n1`: a band of one coefficient a channel: its sign.
fn quant_band_n1(
    ctx: &mut BandCtx<'_, '_>,
    x: &mut [i32],
    y: Option<&mut [i32]>,
    lowband_out: Option<&mut [i32]>,
) -> u32 {
    x[0] = decode_sign(ctx);
    if let Some(y) = y {
        y[0] = decode_sign(ctx);
    }
    if let Some(out) = lowband_out {
        out[0] = x[0] >> 4;
    }
    1
}

/// A one-coefficient band's sign: a raw bit, if the frame has a bit left
/// for it, else positive.
fn decode_sign(ctx: &mut BandCtx<'_, '_>) -> i32 {
    let mut sign = 0;
    if ctx.remaining_bits >= 1 << BITRES {
        sign = ctx.ec.bits(1);
        ctx.remaining_bits -= 1 << BITRES;
    }
    if sign != 0 {
        -NORM_SCALING
    } else {
        NORM_SCALING
    }
}

/// `quant_partition`, decoding: a mono partition of `n` coefficients in
/// `x`, split in two while it has more bits than a codeword uses.
fn quant_partition(
    ctx: &mut BandCtx<'_, '_>,
    x: &mut [i32],
    n: usize,
    b: i32,
    big_b: i32,
    lowband: Option<&[i32]>,
    lm: i32,
    gain: i32,
    fill: i32,
) -> u32 {
    let i = ctx.i;
    let b0 = big_b;
    let mut cm: u32;
    // Split when the band needs 1.5 bits more than its largest codeword
    // takes (`cache[cache[0]]`, which libopus too reads only for LM >= 0).
    if lm != -1 && b > cache_top(i, lm) + 12 && n > 2 {
        let n = n >> 1;
        let (xl, xr) = x.split_at_mut(n);
        let lm = lm - 1;
        let mut fill = fill;
        if big_b == 1 {
            fill = (fill & 1) | (fill << 1);
        }
        let big_b = (big_b + 1) >> 1;
        let mut b = b;
        let s = compute_theta(ctx, n as i32, &mut b, big_b, b0, lm, false, &mut fill);
        let (mid, side) = (s.imid, s.iside);
        let mut delta = s.delta;
        let itheta = s.itheta;
        // More bits to low-energy MDCTs than they would otherwise get.
        if b0 > 1 && itheta & 0x3fff != 0 {
            if itheta > 8192 {
                // A rough model of pre-echo masking.
                delta -= delta >> (4 - lm);
            } else {
                // A forward-masking slope of 1.5 dB per 10 ms.
                delta = 0.min(delta + (((n as i32) << BITRES) >> (5 - lm)));
            }
        }
        let mut mbits = 0.max(b.min((b - delta) / 2));
        let mut sbits = b - mbits;
        ctx.remaining_bits -= s.qalloc;
        let next_lowband2 = lowband.map(|l| &l[n..]);
        let mut rebalance = ctx.remaining_bits;
        if mbits >= sbits {
            cm = quant_partition(
                ctx,
                xl,
                n,
                mbits,
                big_b,
                lowband,
                lm,
                mult16_16_p15(gain, mid),
                fill,
            );
            rebalance = mbits - (rebalance - ctx.remaining_bits);
            if rebalance > 3 << BITRES && itheta != 0 {
                sbits += rebalance - (3 << BITRES);
            }
            cm |= quant_partition(
                ctx,
                xr,
                n,
                sbits,
                big_b,
                next_lowband2,
                lm,
                mult16_16_p15(gain, side),
                fill >> big_b,
            ) << (b0 >> 1);
        } else {
            cm = quant_partition(
                ctx,
                xr,
                n,
                sbits,
                big_b,
                next_lowband2,
                lm,
                mult16_16_p15(gain, side),
                fill >> big_b,
            ) << (b0 >> 1);
            rebalance = sbits - (rebalance - ctx.remaining_bits);
            if rebalance > 3 << BITRES && itheta != 16384 {
                mbits += rebalance - (3 << BITRES);
            }
            cm |= quant_partition(
                ctx,
                xl,
                n,
                mbits,
                big_b,
                lowband,
                lm,
                mult16_16_p15(gain, mid),
                fill,
            );
        }
    } else {
        // The no-split case.
        let mut q = bits2pulses(i, lm, b);
        let mut curr_bits = pulses2bits(i, lm, q);
        ctx.remaining_bits -= curr_bits;
        // Never bust the budget.
        while ctx.remaining_bits < 0 && q > 0 {
            ctx.remaining_bits += curr_bits;
            q -= 1;
            curr_bits = pulses2bits(i, lm, q);
            ctx.remaining_bits -= curr_bits;
        }
        if q != 0 {
            let k = get_pulses(q);
            cm = alg_unquant(x, n, k, ctx.spread, big_b as usize, ctx.ec, gain);
        } else {
            // No pulses: fill the band anyway.
            let cm_mask = ((1u64 << big_b) - 1) as u32;
            let fill = fill as u32 & cm_mask;
            cm = 0;
            if fill == 0 {
                x[..n].fill(0);
            } else {
                match lowband {
                    None => {
                        // Noise.
                        for v in &mut x[..n] {
                            ctx.seed = lcg_rand(ctx.seed);
                            *v = extract16((ctx.seed as i32) >> 20);
                        }
                        cm = cm_mask;
                    }
                    Some(low) => {
                        // The folded spectrum, about 48 dB below the
                        // "normal" folding level.
                        for (v, &l) in x[..n].iter_mut().zip(low) {
                            ctx.seed = lcg_rand(ctx.seed);
                            let tmp = qconst16(1.0 / 256.0, 10);
                            let tmp = if ctx.seed & 0x8000 != 0 { tmp } else { -tmp };
                            *v = extract16(l + tmp);
                        }
                        cm = fill;
                    }
                }
                renormalise_vector(x, n, gain);
            }
        }
    }
    cm
}

/// `quant_band`, decoding: a mono band (or one channel of dual stereo, or a
/// stereo mid), its time-frequency resolution changed for the decode and
/// back, its scaled copy left in `lowband_out` for later bands to fold.
fn quant_band(
    ctx: &mut BandCtx<'_, '_>,
    x: &mut [i32],
    n: usize,
    b: i32,
    big_b: i32,
    lowband: Option<&mut [i32]>,
    lm: i32,
    lowband_out: Option<&mut [i32]>,
    gain: i32,
    fill: i32,
) -> u32 {
    let n0 = n;
    let mut n_b = n;
    let b0 = big_b;
    let long_blocks = b0 == 1;
    let mut big_b = big_b;
    let mut fill = fill;
    let mut tf_change = ctx.tf_change;
    n_b = n_b.checked_div(big_b as usize).unwrap_or(0);
    // One sample.
    if n == 1 {
        return quant_band_n1(ctx, x, None, lowband_out);
    }
    let recombine = if tf_change > 0 { tf_change } else { 0 };
    let mut lowband = lowband;
    // Recombining bands, for more frequency resolution.
    for k in 0..recombine {
        const BIT_INTERLEAVE_TABLE: [i32; 16] = [0, 1, 1, 1, 2, 3, 3, 3, 2, 3, 3, 3, 2, 3, 3, 3];
        if let Some(low) = lowband.as_deref_mut() {
            haar1(low, n >> k, 1 << k);
        }
        fill = BIT_INTERLEAVE_TABLE[(fill & 0xF) as usize]
            | BIT_INTERLEAVE_TABLE[((fill >> 4) & 0xF) as usize] << 2;
    }
    big_b >>= recombine;
    n_b <<= recombine;
    // More time resolution.
    let mut time_divide = 0;
    while n_b & 1 == 0 && tf_change < 0 {
        if let Some(low) = lowband.as_deref_mut() {
            haar1(low, n_b, big_b as usize);
        }
        fill |= fill << big_b;
        big_b <<= 1;
        n_b >>= 1;
        time_divide += 1;
        tf_change += 1;
    }
    let b0 = big_b;
    let n_b0 = n_b;
    // The samples in time order rather than frequency order.
    if b0 > 1
        && let Some(low) = lowband.as_deref_mut()
    {
        deinterleave_hadamard(
            low,
            n_b >> recombine,
            (b0 << recombine) as usize,
            long_blocks,
        );
    }
    let mut cm = quant_partition(ctx, x, n, b, big_b, lowband.as_deref(), lm, gain, fill);

    // The reorganisation undone.
    if b0 > 1 {
        interleave_hadamard(x, n_b >> recombine, (b0 << recombine) as usize, long_blocks);
    }
    // The time-frequency changes undone.
    let mut n_b = n_b0;
    let mut big_b = b0;
    for _ in 0..time_divide {
        big_b >>= 1;
        n_b <<= 1;
        cm |= cm >> big_b;
        haar1(x, n_b, big_b as usize);
    }
    for k in 0..recombine {
        const BIT_DEINTERLEAVE_TABLE: [u32; 16] = [
            0x00, 0x03, 0x0C, 0x0F, 0x30, 0x33, 0x3C, 0x3F, 0xC0, 0xC3, 0xCC, 0xCF, 0xF0, 0xF3,
            0xFC, 0xFF,
        ];
        cm = BIT_DEINTERLEAVE_TABLE[(cm & 0xF) as usize];
        haar1(x, n0 >> k, 1 << k);
    }
    big_b <<= recombine;
    // The output scaled for later folding.
    if let Some(out) = lowband_out {
        let nn = sqrt(shl32(n0 as i32, 22));
        for (o, &v) in out[..n0].iter_mut().zip(&x[..n0]) {
            *o = extract16(mult16_16_q15(nn, v));
        }
    }
    cm & ((1u64 << big_b) - 1) as u32
}

/// `stereo_merge`: mid and side back to left and right.
fn stereo_merge(x: &mut [i32], y: &mut [i32], mid: i32, n: usize) {
    // |X+Y|^2 and |X-Y|^2 as |X|^2 + |Y|^2 +/- sum(xy).
    let mut xp = 0i32;
    let mut side = 0i32;
    for j in 0..n {
        xp = mac16_16(xp, y[j], x[j]);
        side = mac16_16(side, y[j], y[j]);
    }
    // The mid's normalisation compensated.
    xp = mult16_32_q15(mid, xp);
    // Mid and side are Q15, not Q14 as X and Y.
    let mid2 = mid >> 1;
    let el = mult16_16(mid2, mid2)
        .wrapping_add(side)
        .wrapping_sub(xp.wrapping_mul(2));
    let er = mult16_16(mid2, mid2)
        .wrapping_add(side)
        .wrapping_add(xp.wrapping_mul(2));
    if er < qconst32(6e-4, 28) || el < qconst32(6e-4, 28) {
        y[..n].copy_from_slice(&x[..n]);
        return;
    }
    let mut kl = ilog2(el) >> 1;
    let mut kr = ilog2(er) >> 1;
    let lgain = rsqrt_norm(vshr32(el, (kl - 7) << 1));
    let rgain = rsqrt_norm(vshr32(er, (kr - 7) << 1));
    kl = kl.max(7);
    kr = kr.max(7);
    for j in 0..n {
        // The mid scaled (the side already is).
        let l = extract16(mult16_16_p15(mid, x[j]));
        let r = y[j];
        x[j] = extract16(pshr32(mult16_16(lgain, sub16(l, r)), (kl + 1) as u32));
        y[j] = extract16(pshr32(mult16_16(rgain, add16(l, r)), (kr + 1) as u32));
    }
}

/// `quant_band_stereo`, decoding: a stereo band as mid and side.
fn quant_band_stereo(
    ctx: &mut BandCtx<'_, '_>,
    x: &mut [i32],
    y: &mut [i32],
    n: usize,
    b: i32,
    big_b: i32,
    lowband: Option<&mut [i32]>,
    lm: i32,
    lowband_out: Option<&mut [i32]>,
    fill: i32,
) -> u32 {
    if n == 1 {
        return quant_band_n1(ctx, x, Some(y), lowband_out);
    }
    let orig_fill = fill;
    let mut fill = fill;
    let mut b = b;
    let s = compute_theta(ctx, n as i32, &mut b, big_b, big_b, lm, true, &mut fill);
    let (mid, side) = (s.imid, s.iside);
    let itheta = s.itheta;
    let cm;
    if n == 2 {
        // Mid and side are orthogonal: the side takes one bit.
        let mut mbits = b;
        let mut sbits = 0;
        if itheta != 0 && itheta != 16384 {
            sbits = 1 << BITRES;
        }
        mbits -= sbits;
        let c = itheta > 8192;
        ctx.remaining_bits -= s.qalloc + sbits;
        let mut sign = 0;
        if sbits != 0 {
            sign = ctx.ec.bits(1) as i32;
        }
        let sign = 1 - 2 * sign;
        // orig_fill: the side is folded even where itheta == 16384 cleared
        // the low bits of fill.
        {
            let (x2, y2): (&mut [i32], &mut [i32]) = if c {
                (&mut *y, &mut *x)
            } else {
                (&mut *x, &mut *y)
            };
            cm = quant_band(
                ctx,
                x2,
                n,
                mbits,
                big_b,
                lowband,
                lm,
                lowband_out,
                Q15ONE,
                orig_fill,
            );
            // No split for N = 2: cm is 1, or 0 for a collapsed fold, with
            // nothing to mix in from the other channel.
            y2[0] = extract16(-sign * x2[1]);
            y2[1] = extract16(sign * x2[0]);
        }
        x[0] = extract16(mult16_16_q15(mid, x[0]));
        x[1] = extract16(mult16_16_q15(mid, x[1]));
        y[0] = extract16(mult16_16_q15(side, y[0]));
        y[1] = extract16(mult16_16_q15(side, y[1]));
        let tmp = x[0];
        x[0] = extract16(sub16(tmp, y[0]));
        y[0] = add16(tmp, y[0]);
        let tmp = x[1];
        x[1] = extract16(sub16(tmp, y[1]));
        y[1] = add16(tmp, y[1]);
    } else {
        // The normal split.
        let mut mbits = 0.max(b.min((b - s.delta) / 2));
        let mut sbits = b - mbits;
        ctx.remaining_bits -= s.qalloc;
        let mut rebalance = ctx.remaining_bits;
        let mut c;
        if mbits >= sbits {
            // The mid is not scaled: it is needed normalised for folding.
            c = quant_band(
                ctx,
                x,
                n,
                mbits,
                big_b,
                lowband,
                lm,
                lowband_out,
                Q15ONE,
                fill,
            );
            rebalance = mbits - (rebalance - ctx.remaining_bits);
            if rebalance > 3 << BITRES && itheta != 0 {
                sbits += rebalance - (3 << BITRES);
            }
            // The high bits of fill are zero for a stereo split: no folding
            // of the side.
            c |= quant_band(ctx, y, n, sbits, big_b, None, lm, None, side, fill >> big_b);
        } else {
            c = quant_band(ctx, y, n, sbits, big_b, None, lm, None, side, fill >> big_b);
            rebalance = sbits - (rebalance - ctx.remaining_bits);
            if rebalance > 3 << BITRES && itheta != 16384 {
                mbits += rebalance - (3 << BITRES);
            }
            c |= quant_band(
                ctx,
                x,
                n,
                mbits,
                big_b,
                lowband,
                lm,
                lowband_out,
                Q15ONE,
                fill,
            );
        }
        cm = c;
    }
    if n != 2 {
        stereo_merge(x, y, mid, n);
    }
    if s.inv {
        for v in &mut y[..n] {
            *v = extract16(-*v);
        }
    }
    cm
}

/// `special_hybrid_folding`: enough of the first band's folding data
/// repeated to fold the second from (nothing in a CELT-only frame).
fn special_hybrid_folding(
    norm: &mut [i32],
    norm2_at: usize,
    start: usize,
    m: usize,
    dual_stereo: bool,
) {
    let n1 = m * ebands(start + 1).wrapping_sub(ebands(start)) as usize;
    let n2 = m * ebands(start + 2).wrapping_sub(ebands(start + 1)) as usize;
    if n2 > n1 {
        let from = 2 * n1 - n2;
        norm.copy_within(from..from + (n2 - n1), n1);
        if dual_stereo {
            norm.copy_within(norm2_at + from..norm2_at + from + (n2 - n1), norm2_at + n1);
        }
    }
}

/// The frame's parameters `quant_all_bands` decodes with.
pub(crate) struct AllBands<'p> {
    pub start: usize,
    pub end: usize,
    pub pulses: &'p [i32],
    pub short_blocks: bool,
    pub spread: i32,
    pub dual_stereo: bool,
    pub intensity: i32,
    pub tf_res: &'p [i32],
    pub total_bits: i32,
    pub balance: i32,
    pub lm: i32,
    pub coded_bands: usize,
    pub disable_inv: bool,
}

/// The folding buffer [`quant_all_bands`] needs: two channels' worth of
/// every band but the last, at 8 short blocks.
pub(crate) const NORM_SIZE: usize = 2 * 8 * 100;

/// `quant_all_bands`, decoding: every band's shape into `x` (and `y` for
/// stereo), with each band's collapse mask; the noise seed carried on.
/// `norm` is libopus's `_norm`, at least [`NORM_SIZE`] long: no band reads
/// what this frame has not written, so its contents do not matter.
#[allow(
    clippy::too_many_arguments,
    reason = "libopus's own, with its stack buffer passed in"
)]
pub(crate) fn quant_all_bands(
    p: &AllBands<'_>,
    x_: &mut [i32],
    mut y_: Option<&mut [i32]>,
    collapse_masks: &mut [u8],
    ec: &mut Decoder<'_>,
    seed: &mut u32,
    norm: &mut [i32],
) {
    let c = if y_.is_some() { 2 } else { 1 };
    let m = 1usize << p.lm;
    let big_b = if p.short_blocks { m as i32 } else { 1 };
    let norm_offset = m * ebands(p.start) as usize;
    // No norm for the last band: nothing folds from it.
    let norm_len = m * ebands(NB_EBANDS - 1) as usize - norm_offset;
    let norm = &mut norm[..c * norm_len];
    // Where each band's folding source is copied (libopus's
    // `lowband_scratch`): one a channel, for dual stereo.
    let mut low_x = [0i32; MAX_BAND];
    let mut low_y = [0i32; MAX_BAND];
    let norm2_at = norm_len;
    let mut balance = p.balance;
    let mut dual_stereo = p.dual_stereo;
    let mut lowband_offset = 0usize;
    let mut update_lowband = true;
    let mut ctx = BandCtx {
        ec,
        i: 0,
        intensity: p.intensity,
        spread: p.spread,
        tf_change: 0,
        remaining_bits: 0,
        seed: *seed,
        disable_inv: p.disable_inv,
    };
    // Every band of the standard mode is coded (`effEBands` is all 21), so
    // libopus's decoding of bands past `effEBands` into `norm` never arises.
    for i in p.start..p.end {
        ctx.i = i;
        let last = i == p.end - 1;
        let lo = m * ebands(i) as usize;
        let n = m * ebands(i + 1) as usize - lo;
        let tell = ctx.ec.tell_frac() as i32;
        // The bits for this band.
        if i != p.start {
            balance -= tell;
        }
        let remaining_bits = p.total_bits - tell - 1;
        ctx.remaining_bits = remaining_bits;
        let b = if i < p.coded_bands {
            let curr_balance = balance
                .checked_div(3.min(p.coded_bands as i32 - i as i32))
                .unwrap_or(0);
            0.max(16383.min((remaining_bits + 1).min(p.pulses[i] + curr_balance)))
        } else {
            0
        };
        if (lo as i32 - n as i32 >= m as i32 * ebands(p.start) || i == p.start + 1)
            && (update_lowband || lowband_offset == 0)
        {
            lowband_offset = i;
        }
        if i == p.start + 1 {
            special_hybrid_folding(norm, norm2_at, p.start, m, dual_stereo);
        }
        ctx.tf_change = p.tf_res[i];

        // A conservative estimate of the collapse masks of the bands folded
        // from.
        let mut effective_lowband: Option<usize> = None;
        let (mut x_cm, mut y_cm): (u32, u32);
        if lowband_offset != 0 && (p.spread != SPREAD_AGGRESSIVE || big_b > 1 || ctx.tf_change < 0)
        {
            // Never repeat spectral content within one band.
            let eff =
                0.max(m as i32 * ebands(lowband_offset) - norm_offset as i32 - n as i32) as usize;
            effective_lowband = Some(eff);
            let mut fold_start = lowband_offset;
            loop {
                fold_start -= 1;
                if m * (ebands(fold_start) as usize) <= eff + norm_offset {
                    break;
                }
            }
            let mut fold_end = lowband_offset - 1;
            loop {
                fold_end += 1;
                if !(fold_end < i && m * (ebands(fold_end) as usize) < eff + norm_offset + n) {
                    break;
                }
            }
            x_cm = 0;
            y_cm = 0;
            let mut fold_i = fold_start;
            loop {
                x_cm |= u32::from(collapse_masks[fold_i * c]);
                y_cm |= u32::from(collapse_masks[fold_i * c + c - 1]);
                fold_i += 1;
                if fold_i >= fold_end {
                    break;
                }
            }
        } else {
            // The LCG folds: every block non-zero, nearly always.
            x_cm = ((1u64 << big_b) - 1) as u32;
            y_cm = x_cm;
        }

        if dual_stereo && i as i32 == p.intensity {
            // Dual stereo off, for intensity.
            dual_stereo = false;
            for j in 0..lo - norm_offset {
                norm[j] = half32(norm[j].wrapping_add(norm[norm2_at + j]));
            }
        }

        // The band folded from, copied out (see the module documentation);
        // a band's own output, for later bands to fold from, goes to `norm`
        // unless it is the last.
        let lowband_of = |norm: &[i32], at: usize, into: &'_ mut [i32; MAX_BAND]| {
            effective_lowband.map(|e| {
                into[..n].copy_from_slice(&norm[at + e..at + e + n]);
            })
        };
        let out_at = lo - norm_offset;
        let x = &mut x_[lo..lo + n];
        let y = y_.as_deref_mut().map(|y_all| &mut y_all[lo..lo + n]);
        match y {
            Some(y) if dual_stereo => {
                let low = lowband_of(norm, 0, &mut low_x).map(|()| &mut low_x[..n]);
                let out = if last {
                    None
                } else {
                    Some(&mut norm[out_at..out_at + n])
                };
                x_cm = quant_band(
                    &mut ctx,
                    x,
                    n,
                    b / 2,
                    big_b,
                    low,
                    p.lm,
                    out,
                    Q15ONE,
                    x_cm as i32,
                );
                let low2 = lowband_of(norm, norm2_at, &mut low_y).map(|()| &mut low_y[..n]);
                let out2 = if last {
                    None
                } else {
                    Some(&mut norm[norm2_at + out_at..norm2_at + out_at + n])
                };
                y_cm = quant_band(
                    &mut ctx,
                    y,
                    n,
                    b / 2,
                    big_b,
                    low2,
                    p.lm,
                    out2,
                    Q15ONE,
                    y_cm as i32,
                );
            }
            Some(y) => {
                let low = lowband_of(norm, 0, &mut low_x).map(|()| &mut low_x[..n]);
                let out = if last {
                    None
                } else {
                    Some(&mut norm[out_at..out_at + n])
                };
                x_cm = quant_band_stereo(
                    &mut ctx,
                    x,
                    y,
                    n,
                    b,
                    big_b,
                    low,
                    p.lm,
                    out,
                    (x_cm | y_cm) as i32,
                );
                y_cm = x_cm;
            }
            None => {
                let low = lowband_of(norm, 0, &mut low_x).map(|()| &mut low_x[..n]);
                let out = if last {
                    None
                } else {
                    Some(&mut norm[out_at..out_at + n])
                };
                x_cm = quant_band(
                    &mut ctx,
                    x,
                    n,
                    b,
                    big_b,
                    low,
                    p.lm,
                    out,
                    Q15ONE,
                    (x_cm | y_cm) as i32,
                );
                y_cm = x_cm;
            }
        }
        collapse_masks[i * c] = x_cm as u8;
        collapse_masks[i * c + c - 1] = y_cm as u8;
        balance += p.pulses[i] + tell;
        // The folding position moves on only at 1 bit a sample or more.
        update_lowband = b > ((n as i32) << BITRES);
    }
    *seed = ctx.seed;
}

/// `anti_collapse`: noise into the short blocks of a transient frame that
/// got no pulses, so that a band's energy does not collapse.
pub(crate) fn anti_collapse(
    x_: &mut [i32],
    collapse_masks: &[u8],
    lm: i32,
    c: usize,
    size: usize,
    start: usize,
    end: usize,
    log_e: &[i32],
    prev1_log_e: &[i32],
    prev2_log_e: &[i32],
    pulses: &[i32],
    mut seed: u32,
) {
    for i in start..end {
        let n0 = ebands(i + 1) - ebands(i);
        // Depth in eighths of a bit.
        let depth = ((1 + pulses[i]) as u32).checked_div(n0 as u32).unwrap_or(0) as i32 >> lm;
        let thresh32 = exp2(-shl16(depth, (10 - BITRES_I) as u32)) >> 1;
        let thresh = extract16(mult16_32_q15(qconst16(0.5, 15), 32767.min(thresh32)));
        let t = n0 << lm;
        let shift = ilog2(t) >> 1;
        let t = shl32(t, ((7 - shift) << 1) as u32);
        let sqrt_1 = rsqrt_norm(t);
        for ch in 0..c {
            let mut prev1 = prev1_log_e[ch * NB_EBANDS + i];
            let mut prev2 = prev2_log_e[ch * NB_EBANDS + i];
            if c == 1 {
                prev1 = prev1.max(prev1_log_e[NB_EBANDS + i]);
                prev2 = prev2.max(prev2_log_e[NB_EBANDS + i]);
            }
            let ediff = (log_e[ch * NB_EBANDS + i] - prev1.min(prev2)).max(0);
            let mut r = if ediff < 16384 {
                let r32 = exp2(-extract16(ediff)) >> 1;
                extract16(2 * 16383.min(r32))
            } else {
                0
            };
            if lm == 3 {
                r = extract16(mult16_16_q14(23170, 23169.min(r)));
            }
            r = extract16(thresh.min(r) >> 1);
            r = extract16(mult16_16_q15(sqrt_1, r) >> shift);
            let base = ch * size + ((ebands(i) as usize) << lm);
            let mut renormalize = false;
            for k in 0..(1usize << lm) {
                // A collapse.
                if u32::from(collapse_masks[i * c + ch]) & (1 << k) == 0 {
                    // Noise.
                    for j in 0..n0 as usize {
                        seed = lcg_rand(seed);
                        x_[base + (j << lm) + k] = if seed & 0x8000 != 0 { r } else { -r };
                    }
                    renormalize = true;
                }
            }
            // Energy was added: renormalise.
            if renormalize {
                let len = (n0 as usize) << lm;
                renormalise_vector(&mut x_[base..base + len], len, Q15ONE);
            }
        }
    }
}

/// `denormalise_bands`: the unit-norm band shapes `x` scaled by the bands'
/// energies (log2, Q10, before the band means are added back) into `freq`.
pub(crate) fn denormalise_bands(
    x: &[i32],
    freq: &mut [i32],
    band_log_e: &[i32],
    start: usize,
    end: usize,
    m: usize,
    downsample: usize,
    silence: bool,
) {
    let n = m * SHORT_MDCT_SIZE;
    let mut bound = m * ebands(end) as usize;
    if downsample != 1 {
        bound = bound.min(n / downsample);
    }
    let (start, end) = if silence { (0, 0) } else { (start, end) };
    if silence {
        bound = 0;
    }
    let mut f = 0usize;
    let mut xi = m * ebands(start) as usize;
    while f < m * ebands(start) as usize {
        freq[f] = 0;
        f += 1;
    }
    for i in start..end {
        let band_end = m * ebands(i + 1) as usize;
        let lg = saturate16(band_log_e[i].wrapping_add(shl32(E_MEANS[i], 6)));
        // The integer part of the log energy.
        let mut shift = 16 - (lg >> DB_SHIFT);
        let mut g;
        if shift > 31 {
            shift = 0;
            g = 0;
        } else {
            // The fractional part.
            g = extract16(exp2_frac(lg & ((1 << DB_SHIFT) - 1)));
        }
        let mut j = m * ebands(i) as usize;
        if shift < 0 {
            // Extreme gains: a corrupted stream's, capped.
            if shift <= -2 {
                g = 16384;
                shift = -2;
            }
            while j < band_end {
                freq[f] = shl32(mult16_16(x[xi], g), (-shift) as u32);
                f += 1;
                xi += 1;
                j += 1;
            }
        } else {
            while j < band_end {
                freq[f] = mult16_16(x[xi], g) >> shift;
                f += 1;
                xi += 1;
                j += 1;
            }
        }
    }
    for v in &mut freq[bound..n] {
        *v = 0;
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    #[test]
    fn bitexact_cos_and_log2tan_are_libopuss() {
        // Values libopus 1.5.2's own functions return (a C program linked
        // against its fixed-point build), over the angles a split can take:
        // multiples of 16384/256 strictly between 0 and 16384.
        for (x, cos) in [
            (64, 32767),
            (1000, 32618),
            (4096, 30274),
            (8192, 23171),
            (12000, 13371),
            (16320, 200),
        ] {
            assert_eq!(bitexact_cos(x), cos, "cos {x}");
        }
        for (s, c, t) in [
            (23171, 23171, 0),
            (32767, 64, 18402),
            (100, 32000, -17042),
            (12345, 30000, -2631),
        ] {
            assert_eq!(bitexact_log2tan(s, c), t, "log2tan {s} {c}");
        }
    }

    #[test]
    fn haar_and_hadamard_invert() {
        let orig: Vec<i32> = (0..16).map(|v| v * 100 - 700).collect();
        let mut x = orig.clone();
        deinterleave_hadamard(&mut x, 4, 4, true);
        interleave_hadamard(&mut x, 4, 4, true);
        assert_eq!(x, orig);
    }
}
