//! A SILK frame's spectral envelope: its normalised line spectral
//! frequencies (NLSFs) decoded from their two-stage codebook, kept apart by
//! at least the codebook's minimum spacing, and turned into LPC coefficients
//! -- bandwidth-expanded until the filter is stable and fits 16 bits.
//!
//! Translated into Rust from libopus 1.5.2's `silk/NLSF_unpack.c`,
//! `silk/NLSF_decode.c`, `silk/NLSF_stabilize.c`, `silk/NLSF2A.c`,
//! `silk/LPC_fit.c`, `silk/LPC_inv_pred_gain.c`, `silk/bwexpander.c`,
//! `silk/bwexpander_32.c` and `silk/sort.c` (the insertion sort the
//! stabiliser falls back on), copyright Skype Limited,
//! Xiph.Org and the contributors named in its `COPYING`, used under
//! libopus's BSD licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "orders are 10 or 16 and the codebooks' rows hold `order` values, the delta table `order + 1`: every index is below those, as libopus's"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "NLSFs in Q15 and coefficients within 32 bits by libopus's own bounds; the sums C lets overflow wrap through the fixed-point helpers"
)]

use super::codebook::NlsfCodebook;
use super::fix::{
    abs, clz32, div32, fix_const, inverse32_varq, limit, lshift, mul, rshift_round, rshift_round64,
    sat16, smlawb, smmul, smulbb, smull, smulww,
};
use super::tables::LSFCOSTAB_FIX_Q12;

/// The largest LPC order.
pub(crate) const MAX_LPC_ORDER: usize = 16;
/// `NLSF_QUANT_MAX_AMPLITUDE`.
pub(crate) const NLSF_QUANT_MAX_AMPLITUDE: i32 = 4;
/// `MAX_LPC_STABILIZE_ITERATIONS`.
const MAX_LPC_STABILIZE_ITERATIONS: i32 = 16;

/// `silk_NLSF_unpack`: for the first-stage vector `cb1_index`, each
/// coefficient's entropy table (its offset in `ec_icdf`) and predictor
/// (Q8).
pub(crate) fn unpack(
    ec_ix: &mut [i16; MAX_LPC_ORDER],
    pred_q8: &mut [u8; MAX_LPC_ORDER],
    cb: &NlsfCodebook,
    cb1_index: usize,
) {
    let order = cb.order;
    let sel = &cb.ec_sel[cb1_index * order / 2..];
    for i in (0..order).step_by(2) {
        let entry = i32::from(sel[i / 2]);
        let amp = 2 * NLSF_QUANT_MAX_AMPLITUDE + 1;
        ec_ix[i] = smulbb((entry >> 1) & 7, amp) as i16;
        pred_q8[i] = cb.pred_q8[i + (entry & 1) as usize * (order - 1)];
        ec_ix[i + 1] = smulbb((entry >> 5) & 7, amp) as i16;
        pred_q8[i + 1] = cb.pred_q8[i + ((entry >> 4) & 1) as usize * (order - 1) + 1];
    }
}

/// `silk_NLSF_residual_dequant`: the second stage's residuals (Q10), each
/// predicted from the next.
fn residual_dequant(
    x_q10: &mut [i16; MAX_LPC_ORDER],
    indices: &[i8],
    pred_coef_q8: &[u8; MAX_LPC_ORDER],
    quant_step_size_q16: i32,
    order: usize,
) {
    let adj = fix_const(0.1, 10);
    let mut out_q10 = 0i32;
    for i in (0..order).rev() {
        let pred_q10 = smulbb(out_q10, i32::from(pred_coef_q8[i] as i16)) >> 8;
        out_q10 = lshift(i32::from(indices[i]), 10);
        if out_q10 > 0 {
            out_q10 -= adj;
        } else if out_q10 < 0 {
            out_q10 += adj;
        }
        out_q10 = smlawb(pred_q10, out_q10, quant_step_size_q16);
        x_q10[i] = out_q10 as i16;
    }
}

/// `silk_NLSF_decode`: the NLSF vector (Q15) of `indices` (the first-stage
/// index, then the residuals), stabilised.
pub(crate) fn decode(nlsf_q15: &mut [i16; MAX_LPC_ORDER], indices: &[i8], cb: &NlsfCodebook) {
    let order = cb.order;
    let mut ec_ix = [0i16; MAX_LPC_ORDER];
    let mut pred_q8 = [0u8; MAX_LPC_ORDER];
    // A first-stage index outside the codebook is impossible from a valid
    // decode (its table has `n_vectors` entries); held to it all the same.
    let cb1 = usize::try_from(indices[0])
        .unwrap_or(0)
        .min(cb.n_vectors - 1);
    unpack(&mut ec_ix, &mut pred_q8, cb, cb1);
    let mut res_q10 = [0i16; MAX_LPC_ORDER];
    residual_dequant(
        &mut res_q10,
        &indices[1..],
        &pred_q8,
        cb.quant_step_size_q16,
        order,
    );
    // The first stage, weighted by the inverse square root, added.
    let element = &cb.cb1_nlsf_q8[cb1 * order..];
    let weight = &cb.cb1_wght_q9[cb1 * order..];
    for i in 0..order {
        let tmp = div32(lshift(i32::from(res_q10[i]), 14), i32::from(weight[i]))
            .wrapping_add(lshift(i32::from(element[i]), 7));
        nlsf_q15[i] = limit(tmp, 0, 32767) as i16;
    }
    stabilize(nlsf_q15, cb.delta_min_q15, order);
}

/// `silk_NLSF_stabilize`: the NLSFs pushed apart until each is at least
/// its minimum distance from its neighbours (and the ends), in up to 20
/// rounds of moving the closest pair, then by force.
pub(crate) fn stabilize(nlsf: &mut [i16; MAX_LPC_ORDER], delta_min: &[i16], l: usize) {
    const MAX_LOOPS: usize = 20;
    let dm = |i: usize| i32::from(delta_min[i]);
    let mut loops = 0;
    while loops < MAX_LOOPS {
        // The smallest distance.
        let mut min_diff = i32::from(nlsf[0]) - dm(0);
        let mut at = 0usize;
        for i in 1..l {
            let diff = i32::from(nlsf[i]) - (i32::from(nlsf[i - 1]) + dm(i));
            if diff < min_diff {
                min_diff = diff;
                at = i;
            }
        }
        let diff = (1 << 15) - (i32::from(nlsf[l - 1]) + dm(l));
        if diff < min_diff {
            min_diff = diff;
            at = l;
        }
        if min_diff >= 0 {
            return;
        }
        if at == 0 {
            // Away from the lower limit.
            nlsf[0] = dm(0) as i16;
        } else if at == l {
            // Away from the upper limit.
            nlsf[l - 1] = ((1 << 15) - dm(l)) as i16;
        } else {
            // Apart about the same centre, within its limits.
            let min_center = (0..at).map(dm).sum::<i32>() + (dm(at) >> 1);
            let max_center = (1 << 15) - ((at + 1)..=l).map(dm).sum::<i32>() - (dm(at) >> 1);
            let center = limit(
                rshift_round(i32::from(nlsf[at - 1]) + i32::from(nlsf[at]), 1),
                min_center,
                max_center,
            ) as i16;
            nlsf[at - 1] = (i32::from(center) - (dm(at) >> 1)) as i16;
            nlsf[at] = (i32::from(nlsf[at - 1]) + dm(at)) as i16;
        }
        loops += 1;
    }
    // The safe fallback: sorted, then spaced from each end.
    insertion_sort(&mut nlsf[..l]);
    nlsf[0] = i32::from(nlsf[0]).max(dm(0)) as i16;
    for i in 1..l {
        let spaced = sat16(i32::from(nlsf[i - 1]) + dm(i));
        nlsf[i] = i32::from(nlsf[i]).max(spaced) as i16;
    }
    nlsf[l - 1] = i32::from(nlsf[l - 1]).min((1 << 15) - dm(l)) as i16;
    for i in (0..l - 1).rev() {
        nlsf[i] = i32::from(nlsf[i]).min(i32::from(nlsf[i + 1]) - dm(i + 1)) as i16;
    }
}

/// `silk_insertion_sort_increasing_all_values_int16`.
fn insertion_sort(a: &mut [i16]) {
    for i in 1..a.len() {
        let value = a[i];
        let mut j = i;
        while j > 0 && value < a[j - 1] {
            a[j] = a[j - 1];
            j -= 1;
        }
        a[j] = value;
    }
}

/// The Q of the polynomials `NLSF2A` builds.
const QA: i32 = 16;

/// `silk_NLSF2A_find_poly`: the polynomial of the cosines at `c_lsf[0]`,
/// `c_lsf[2]`, ... (every other one).
fn find_poly(out: &mut [i32], c_lsf: &[i32], dd: usize) {
    out[0] = lshift(1, QA);
    out[1] = -c_lsf[0];
    for k in 1..dd {
        let ftmp = c_lsf[2 * k];
        out[k + 1] =
            lshift(out[k - 1], 1).wrapping_sub(rshift_round64(smull(ftmp, out[k]), QA) as i32);
        for n in (2..=k).rev() {
            out[n] = out[n].wrapping_add(
                out[n - 2].wrapping_sub(rshift_round64(smull(ftmp, out[n - 1]), QA) as i32),
            );
        }
        out[1] = out[1].wrapping_sub(ftmp);
    }
}

/// `silk_NLSF2A`: the whitening filter (Q12) of NLSFs `nlsf` (Q15), order
/// `d` (10 or 16), made stable.
pub(crate) fn nlsf2a(a_q12: &mut [i16; MAX_LPC_ORDER], nlsf: &[i16], d: usize) {
    // The order that keeps `find_poly` most accurate.
    const ORDERING16: [usize; 16] = [0, 15, 8, 7, 4, 11, 12, 3, 2, 13, 10, 5, 6, 9, 14, 1];
    const ORDERING10: [usize; 10] = [0, 9, 6, 3, 4, 5, 8, 1, 2, 7];
    let ordering: &[usize] = if d == 16 { &ORDERING16 } else { &ORDERING10 };
    let mut cos_lsf_qa = [0i32; 24];
    for k in 0..d {
        // A piecewise-linear 2 * cos(LSF): the integer part indexes the
        // table, the fraction interpolates.
        let n = i32::from(nlsf[k]);
        let f_int = n >> (15 - 7);
        let f_frac = n - lshift(f_int, 15 - 7);
        let f = usize::try_from(f_int).unwrap_or(0).min(127);
        let cos_val = i32::from(LSFCOSTAB_FIX_Q12[f]);
        let delta = i32::from(LSFCOSTAB_FIX_Q12[f + 1]) - cos_val;
        cos_lsf_qa[ordering[k]] = rshift_round(lshift(cos_val, 8) + mul(delta, f_frac), 20 - QA);
    }
    let dd = d >> 1;
    let mut p = [0i32; 13];
    let mut q = [0i32; 13];
    find_poly(&mut p, &cos_lsf_qa, dd);
    find_poly(&mut q, &cos_lsf_qa[1..], dd);
    // The even and odd polynomials into the coefficients (Q17).
    let mut a32_qa1 = [0i32; 24];
    for k in 0..dd {
        let ptmp = p[k + 1].wrapping_add(p[k]);
        let qtmp = q[k + 1].wrapping_sub(q[k]);
        a32_qa1[k] = qtmp.wrapping_neg().wrapping_sub(ptmp);
        a32_qa1[d - k - 1] = qtmp.wrapping_sub(ptmp);
    }
    lpc_fit(a_q12, &mut a32_qa1, 12, QA + 1, d);
    let mut i = 0;
    while lpc_inverse_pred_gain(&a_q12[..d]) == 0 && i < MAX_LPC_STABILIZE_ITERATIONS {
        // Too near unstable: bandwidth expansion on the unscaled
        // coefficients, and measured again.
        bwexpander_32(&mut a32_qa1[..d], 65536 - lshift(2, i));
        for k in 0..d {
            a_q12[k] = rshift_round(a32_qa1[k], QA + 1 - 12) as i16;
        }
        i += 1;
    }
}

/// `silk_LPC_fit`: 32-bit coefficients in Q`q_in` into 16 bits in Q`q_out`,
/// bandwidth-expanded until they fit (clipped after ten rounds, and the
/// input made to match).
pub(crate) fn lpc_fit(a_out: &mut [i16], a_in: &mut [i32], q_out: i32, q_in: i32, d: usize) {
    let mut idx = 0usize;
    let mut rounds = 0;
    while rounds < 10 {
        let mut maxabs = 0;
        for (k, &v) in a_in[..d].iter().enumerate() {
            let absval = abs(v);
            if absval > maxabs {
                maxabs = absval;
                idx = k;
            }
        }
        let maxabs = rshift_round(maxabs, q_in - q_out);
        if maxabs <= i32::from(i16::MAX) {
            break;
        }
        // (i32::MAX >> 14) + i16::MAX.
        let maxabs = maxabs.min(163_838);
        let chirp_q16 = fix_const(0.999, 16).wrapping_sub(div32(
            lshift(maxabs - i32::from(i16::MAX), 14),
            mul(maxabs, idx as i32 + 1) >> 2,
        ));
        bwexpander_32(&mut a_in[..d], chirp_q16);
        rounds += 1;
    }
    if rounds == 10 {
        // Clipped, the input kept in step.
        for k in 0..d {
            a_out[k] = sat16(rshift_round(a_in[k], q_in - q_out)) as i16;
            a_in[k] = lshift(i32::from(a_out[k]), q_in - q_out);
        }
    } else {
        for k in 0..d {
            a_out[k] = rshift_round(a_in[k], q_in - q_out) as i16;
        }
    }
}

/// `LPC_inverse_pred_gain_QA_c`'s Q.
const QA24: i32 = 24;

/// `silk_LPC_inverse_pred_gain`: the inverse of the prediction gain of
/// coefficients `a_q12` (Q30), or 0 if the filter is unstable or gains too
/// much.
pub(crate) fn lpc_inverse_pred_gain(a_q12: &[i16]) -> i32 {
    let order = a_q12.len();
    let mut a = [0i32; 24];
    let mut dc_resp = 0i32;
    for (k, &v) in a_q12.iter().enumerate() {
        dc_resp += i32::from(v);
        a[k] = lshift(i32::from(v), QA24 - 12);
    }
    // An unstable DC response needs no further work.
    if dc_resp >= 4096 {
        return 0;
    }
    inverse_pred_gain_qa(&mut a, order)
}

/// `LPC_inverse_pred_gain_QA_c`.
fn inverse_pred_gain_qa(a: &mut [i32; 24], order: usize) -> i32 {
    let a_limit = fix_const(0.999_75, QA24 as u32);
    // SILK_FIX_CONST(1.0f / MAX_PREDICTION_POWER_GAIN, 30), the float
    // 1e-4f's: the same constant a double gives.
    let min_inv_gain_q30 = 107_374;
    let mut inv_gain_q30 = 1i32 << 30;
    let frac = |a: i32, b: i32, q: i32| rshift_round64(smull(a, b), q) as i32;
    let mut k = order - 1;
    while k > 0 {
        if a[k] > a_limit || a[k] < -a_limit {
            return 0;
        }
        // The reflection coefficient: the negated AR coefficient.
        let rc_q31 = lshift(a[k], 31 - QA24).wrapping_neg();
        let rc_mult1_q30 = (1i32 << 30).wrapping_sub(smmul(rc_q31, rc_q31));
        inv_gain_q30 = lshift(smmul(inv_gain_q30, rc_mult1_q30), 2);
        if inv_gain_q30 < min_inv_gain_q30 {
            return 0;
        }
        let mult2q = 32 - clz32(abs(rc_mult1_q30));
        let rc_mult2 = inverse32_varq(rc_mult1_q30, mult2q + 30);
        // The AR coefficients updated in pairs from both ends (both read
        // before either is written, the middle one of an odd count twice
        // alike); a result past 32 bits means instability.
        for n in 0..(k + 1) >> 1 {
            let tmp1 = a[n];
            let tmp2 = a[k - n - 1];
            let update = |x: i32, y: i32| {
                let t = rshift_round64(
                    smull(x.saturating_sub(frac(y, rc_q31, 31)), rc_mult2),
                    mult2q,
                );
                i32::try_from(t).ok()
            };
            let (Some(first), Some(second)) = (update(tmp1, tmp2), update(tmp2, tmp1)) else {
                return 0;
            };
            a[n] = first;
            a[k - n - 1] = second;
        }
        k -= 1;
    }
    if a[0] > a_limit || a[0] < -a_limit {
        return 0;
    }
    let rc_q31 = lshift(a[0], 31 - QA24).wrapping_neg();
    let rc_mult1_q30 = (1i32 << 30).wrapping_sub(smmul(rc_q31, rc_q31));
    inv_gain_q30 = lshift(smmul(inv_gain_q30, rc_mult1_q30), 2);
    if inv_gain_q30 < min_inv_gain_q30 {
        return 0;
    }
    inv_gain_q30
}

/// `silk_bwexpander`: 16-bit coefficients chirped by `chirp_q16`, rounding
/// (not `SMULWB`, whose bias could make the filter unstable).
pub(crate) fn bwexpander(ar: &mut [i16], mut chirp_q16: i32) {
    let d = ar.len();
    let chirp_minus_one_q16 = chirp_q16 - 65536;
    for v in &mut ar[..d - 1] {
        *v = rshift_round(mul(chirp_q16, i32::from(*v)), 16) as i16;
        chirp_q16 += rshift_round(mul(chirp_q16, chirp_minus_one_q16), 16);
    }
    ar[d - 1] = rshift_round(mul(chirp_q16, i32::from(ar[d - 1])), 16) as i16;
}

/// `silk_bwexpander_32`: 32-bit coefficients chirped by `chirp_q16`.
pub(crate) fn bwexpander_32(ar: &mut [i32], mut chirp_q16: i32) {
    let d = ar.len();
    let chirp_minus_one_q16 = chirp_q16.wrapping_sub(65536);
    for v in &mut ar[..d - 1] {
        *v = smulww(chirp_q16, *v);
        chirp_q16 = chirp_q16.wrapping_add(rshift_round(mul(chirp_q16, chirp_minus_one_q16), 16));
    }
    ar[d - 1] = smulww(chirp_q16, ar[d - 1]);
}
