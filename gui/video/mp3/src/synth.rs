//! The polyphase synthesis filterbank: 32 subbands of each time slot into
//! 32 samples -- minimp3's `mp3d_DCT_II`, `mp3d_synth_pair`, `mp3d_synth`,
//! `mp3d_synth_granule` and `mp3d_scale_pcm`, its scalar (non-SIMD) code
//! translated into Rust (minimp3, CC0: `licenses/minimp3-LICENSE`).
//!
//! minimp3's SIMD code sums in another order and rounds its samples half to
//! even; the scalar code is the reference this crate is held to.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "minimp3's arithmetic on indices bounded by its 576-line granules and 2112-float synthesis buffer, translated as it is"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "indices bounded by the granule's 32 subbands of 18 slots and the 33 rows of 64 the filter keeps"
)]
#![allow(
    clippy::excessive_precision,
    clippy::unreadable_literal,
    reason = "minimp3's own constants, as written"
)]

use crate::tables::{G_SEC, G_WIN};

/// `mp3d_DCT_II`: each of the first `n` time slots' 32 subbands (a stride
/// of 18 apart) through a 32-point DCT-II, in place.
fn dct_ii(grbuf: &mut [f32], n: usize) {
    for k in 0..n {
        let mut t = [[0.0f32; 8]; 4];
        let y = |i: usize| k + i * 18;
        for i in 0..8 {
            let x0 = grbuf[y(i)];
            let x1 = grbuf[y(15 - i)];
            let x2 = grbuf[y(16 + i)];
            let x3 = grbuf[y(31 - i)];
            let t0 = x0 + x3;
            let t1 = x1 + x2;
            let t2 = (x1 - x2) * G_SEC[3 * i];
            let t3 = (x0 - x3) * G_SEC[3 * i + 1];
            t[0][i] = t0 + t1;
            t[1][i] = (t0 - t1) * G_SEC[3 * i + 2];
            t[2][i] = t3 + t2;
            t[3][i] = (t3 - t2) * G_SEC[3 * i + 2];
        }
        for x in &mut t {
            let [
                mut x0,
                mut x1,
                mut x2,
                mut x3,
                mut x4,
                mut x5,
                mut x6,
                mut x7,
            ] = *x;
            let mut xt = x0 - x7;
            x0 += x7;
            x7 = x1 - x6;
            x1 += x6;
            x6 = x2 - x5;
            x2 += x5;
            x5 = x3 - x4;
            x3 += x4;
            x4 = x0 - x3;
            x0 += x3;
            x3 = x1 - x2;
            x1 += x2;
            x[0] = x0 + x1;
            x[4] = (x0 - x1) * 0.70710677;
            x5 += x6;
            x6 = (x6 + x7) * 0.70710677;
            x7 += xt;
            x3 = (x3 + x4) * 0.70710677;
            x5 -= x7 * 0.198912367; // rotate by PI/8
            x7 += x5 * 0.382683432;
            x5 -= x7 * 0.198912367;
            x0 = xt - x6;
            xt += x6;
            x[1] = (xt + x7) * 0.50979561;
            x[2] = (x4 + x3) * 0.54119611;
            x[3] = (x0 - x5) * 0.60134488;
            x[5] = (x0 + x5) * 0.89997619;
            x[6] = (x4 - x3) * 1.30656302;
            x[7] = (xt - x7) * 2.56291556;
        }
        let mut at = k;
        for i in 0..7 {
            grbuf[at] = t[0][i];
            grbuf[at + 18] = t[2][i] + t[3][i] + t[3][i + 1];
            grbuf[at + 2 * 18] = t[1][i] + t[1][i + 1];
            grbuf[at + 3 * 18] = t[2][i + 1] + t[3][i] + t[3][i + 1];
            at += 4 * 18;
        }
        grbuf[at] = t[0][7];
        grbuf[at + 18] = t[2][7] + t[3][7];
        grbuf[at + 2 * 18] = t[1][7];
        grbuf[at + 3 * 18] = t[3][7];
    }
}

/// `mp3d_scale_pcm`: a sample to 16 bits, as minimp3's scalar code does it
/// -- `+0.5`, truncated, and one taken from a negative result; which puts
/// a sample between -1.5 and -0.5 at 0, not -1. Kept: it is the reference's
/// output, and a difference of one in 32768 at the quietest.
fn scale_pcm(sample: f32) -> i16 {
    if sample >= 32766.5 {
        return 32767;
    }
    if sample <= -32767.5 {
        return -32768;
    }
    // Within (-32767, 32767): the conversion truncates and cannot saturate.
    let s = (sample + 0.5) as i16;
    s - i16::from(s < 0)
}

/// `mp3d_synth_pair`: the two samples a time slot pair's filter gives at
/// the edges of its window, from the column at `z`.
fn synth_pair(pcm: &mut [i16], at: usize, nch: usize, lins: &[f32], z: usize) {
    let v = |k: usize| lins[z + k * 64];
    let mut a = (v(14) - v(0)) * 29.0;
    a += (v(1) + v(13)) * 213.0;
    a += (v(12) - v(2)) * 459.0;
    a += (v(3) + v(11)) * 2037.0;
    a += (v(10) - v(4)) * 5153.0;
    a += (v(5) + v(9)) * 6574.0;
    a += (v(8) - v(6)) * 37489.0;
    a += v(7) * 75038.0;
    pcm[at] = scale_pcm(a);

    let v = |k: usize| lins[z + 2 + k * 64];
    let mut a = v(14) * 104.0;
    a += v(12) * 1567.0;
    a += v(10) * 9727.0;
    a += v(8) * 64019.0;
    a += v(6) * -9975.0;
    a += v(4) * -45.0;
    a += v(2) * 146.0;
    a += v(0) * -5.0;
    pcm[at + 16 * nch] = scale_pcm(a);
}

/// One row's sums: minimp3's `a` and `b` for the four columns at `at`,
/// through its steps S0(0) S2(1) S1(2) S2(3) S1(4) S2(5) S1(6) S2(7) over
/// the sixteen rows the window `w` weighs, column by column as minimp3's
/// scalar code has them. The sixteen rows' four floats are gathered first,
/// so that the sums index only arrays of known size.
fn row_sums(lins: &[f32], at: usize, w: &[f32; 16]) -> ([f32; 4], [f32; 4]) {
    let mut z = [[0.0f32; 4]; 8];
    let mut y = [[0.0f32; 4]; 8];
    for k in 0..8 {
        z[k].copy_from_slice(&lins[at - k * 64..at - k * 64 + 4]);
        y[k].copy_from_slice(&lins[at - (15 - k) * 64..at - (15 - k) * 64 + 4]);
    }
    let mut sum_a = [0.0f32; 4];
    let mut sum_b = [0.0f32; 4];
    for j in 0..4 {
        sum_b[j] = z[0][j] * w[1] + y[0][j] * w[0];
        sum_a[j] = z[0][j] * w[0] - y[0][j] * w[1];
    }
    for k in 1..8 {
        let (w0, w1) = (w[2 * k], w[2 * k + 1]);
        for j in 0..4 {
            sum_b[j] += z[k][j] * w1 + y[k][j] * w0;
            sum_a[j] += if k % 2 == 1 {
                y[k][j] * w1 - z[k][j] * w0
            } else {
                z[k][j] * w0 - y[k][j] * w1
            };
        }
    }
    (sum_a, sum_b)
}

/// `mp3d_synth`: two time slots (columns `xl` and `xl + 1` of each
/// channel's subbands) into 64 samples a channel at the start of `pcm`,
/// through the filter's rows from `base` in `lins`.
fn synth(grbuf: &[f32], xl: usize, pcm: &mut [i16], nch: usize, lins: &mut [f32], base: usize) {
    let xr = xl + 576 * (nch - 1);
    let dstr = nch - 1;
    let zlin = base + 15 * 64;

    lins[zlin + 4 * 15] = grbuf[xl + 18 * 16];
    lins[zlin + 4 * 15 + 1] = grbuf[xr + 18 * 16];
    lins[zlin + 4 * 15 + 2] = grbuf[xl];
    lins[zlin + 4 * 15 + 3] = grbuf[xr];

    lins[zlin + 4 * 31] = grbuf[xl + 1 + 18 * 16];
    lins[zlin + 4 * 31 + 1] = grbuf[xr + 1 + 18 * 16];
    lins[zlin + 4 * 31 + 2] = grbuf[xl + 1];
    lins[zlin + 4 * 31 + 3] = grbuf[xr + 1];

    synth_pair(pcm, dstr, nch, lins, base + 4 * 15 + 1);
    synth_pair(pcm, dstr + 32 * nch, nch, lins, base + 4 * 15 + 64 + 1);
    synth_pair(pcm, 0, nch, lins, base + 4 * 15);
    synth_pair(pcm, 32 * nch, nch, lins, base + 4 * 15 + 64);

    for (row, i) in (0..15).rev().enumerate() {
        lins[zlin + 4 * i] = grbuf[xl + 18 * (31 - i)];
        lins[zlin + 4 * i + 1] = grbuf[xr + 18 * (31 - i)];
        lins[zlin + 4 * i + 2] = grbuf[xl + 1 + 18 * (31 - i)];
        lins[zlin + 4 * i + 3] = grbuf[xr + 1 + 18 * (31 - i)];
        lins[zlin + 4 * (i + 16)] = grbuf[xl + 1 + 18 * (1 + i)];
        lins[zlin + 4 * (i + 16) + 1] = grbuf[xr + 1 + 18 * (1 + i)];
        lins[zlin + 4 * i - 64 + 2] = grbuf[xl + 18 * (1 + i)];
        lins[zlin + 4 * i - 64 + 3] = grbuf[xr + 18 * (1 + i)];

        let mut w = [0.0f32; 16];
        w.copy_from_slice(&G_WIN[16 * row..16 * row + 16]);
        let (sum_a, sum_b) = row_sums(lins, zlin + 4 * i, &w);

        pcm[dstr + (15 - i) * nch] = scale_pcm(sum_a[1]);
        pcm[dstr + (17 + i) * nch] = scale_pcm(sum_b[1]);
        pcm[(15 - i) * nch] = scale_pcm(sum_a[0]);
        pcm[(17 + i) * nch] = scale_pcm(sum_b[0]);
        pcm[dstr + (47 - i) * nch] = scale_pcm(sum_a[3]);
        pcm[dstr + (49 + i) * nch] = scale_pcm(sum_b[3]);
        pcm[(47 - i) * nch] = scale_pcm(sum_a[2]);
        pcm[(49 + i) * nch] = scale_pcm(sum_b[2]);
    }
}

/// `mp3d_synth_granule`: `nbands` time slots of `nch` channels' subbands
/// (`grbuf`, 576 a channel) into `nbands * 32` interleaved samples a
/// channel, the filter's state carried in `qmf_state` and worked in `lins`.
pub(crate) fn synth_granule(
    qmf_state: &mut [f32; 960],
    grbuf: &mut [f32; 1152],
    nbands: usize,
    nch: usize,
    pcm: &mut [i16],
    lins: &mut [f32; 33 * 64],
) {
    for i in 0..nch {
        dct_ii(&mut grbuf[576 * i..576 * i + 576], nbands);
    }

    lins[..960].copy_from_slice(qmf_state);

    for i in (0..nbands).step_by(2) {
        synth(grbuf, i, &mut pcm[32 * nch * i..], nch, lins, i * 64);
    }
    if nch == 1 {
        // minimp3 keeps only the even rows' floats in mono: the odd ones
        // are the "right" channel's, a copy of the left that is overwritten
        // before it is ever heard (MINIMP3_NONSTANDARD_BUT_LOGICAL unset).
        for i in (0..960).step_by(2) {
            qmf_state[i] = lins[nbands * 64 + i];
        }
    } else {
        qmf_state.copy_from_slice(&lins[nbands * 64..nbands * 64 + 960]);
    }
}
