//! SILK's stereo: mid and side, the side predicted from the mid by two
//! coefficients interpolated over the first 8 ms of each frame, back to
//! left and right.
//!
//! Translated into Rust from libopus 1.5.2's `silk/stereo_decode_pred.c`
//! and `silk/stereo_MS_to_LR.c`, copyright Skype Limited, Xiph.Org and the
//! contributors named in its `COPYING`, used under libopus's BSD licence
//! (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "the channels' buffers hold the frame and the two samples before it; the predictor tables are indexed within their 16 entries by values the range decoder bounds"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "16-bit samples and Q13 predictors, within 32 bits by libopus's design"
)]

use super::fix::{fix_const, lshift, rshift_round, sat16, smlabb, smlawb, smulbb, smulwb};
use super::tables::{
    STEREO_ONLY_CODE_MID_ICDF, STEREO_PRED_JOINT_ICDF, STEREO_PRED_QUANT_Q13, UNIFORM3_ICDF,
    UNIFORM5_ICDF,
};
use crate::entdec::Decoder as RangeDecoder;

/// `STEREO_QUANT_SUB_STEPS`, `STEREO_INTERP_LEN_MS`.
const STEREO_QUANT_SUB_STEPS: f64 = 5.0;
const STEREO_INTERP_LEN_MS: i32 = 8;

/// `stereo_dec_state`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StereoState {
    pub pred_prev_q13: [i16; 2],
    pub s_mid: [i16; 2],
    pub s_side: [i16; 2],
}

/// `silk_stereo_decode_pred`: the two predictors (Q13), the first less the
/// second.
pub(crate) fn decode_pred(dec: &mut RangeDecoder<'_>) -> [i32; 2] {
    let n = dec.icdf(&STEREO_PRED_JOINT_ICDF, 8) as i32;
    let mut ix = [[0i32; 3]; 2];
    ix[0][2] = n / 5;
    ix[1][2] = n - 5 * ix[0][2];
    for row in &mut ix {
        row[0] = dec.icdf(&UNIFORM3_ICDF, 8) as i32;
        row[1] = dec.icdf(&UNIFORM5_ICDF, 8) as i32;
    }
    let step = fix_const(0.5 / STEREO_QUANT_SUB_STEPS, 16);
    let mut pred_q13 = [0i32; 2];
    for (p, row) in pred_q13.iter_mut().zip(&mut ix) {
        row[0] += 3 * row[2];
        let at = row[0] as usize;
        let low_q13 = i32::from(STEREO_PRED_QUANT_Q13[at]);
        let step_q13 = smulwb(i32::from(STEREO_PRED_QUANT_Q13[at + 1]) - low_q13, step);
        *p = smlabb(low_q13, step_q13, 2 * row[1] + 1);
    }
    pred_q13[0] -= pred_q13[1];
    pred_q13
}

/// `silk_stereo_decode_mid_only`: whether only the mid is coded.
pub(crate) fn decode_mid_only(dec: &mut RangeDecoder<'_>) -> bool {
    dec.icdf(&STEREO_ONLY_CODE_MID_ICDF, 8) != 0
}

/// `silk_stereo_MS_to_LR`: mid (`x1`) and side (`x2`), each the frame of
/// `frame_length` after two samples of history, to left and right.
pub(crate) fn ms_to_lr(
    state: &mut StereoState,
    x1: &mut [i16],
    x2: &mut [i16],
    pred_q13: [i32; 2],
    fs_khz: i32,
    frame_length: usize,
) {
    // The two samples before the frame, and the frame's last two kept.
    x1[..2].copy_from_slice(&state.s_mid);
    x2[..2].copy_from_slice(&state.s_side);
    state
        .s_mid
        .copy_from_slice(&x1[frame_length..frame_length + 2]);
    state
        .s_side
        .copy_from_slice(&x2[frame_length..frame_length + 2]);
    // The predictors interpolated over the first 8 ms, and the prediction
    // added to the side.
    let mut pred0_q13 = i32::from(state.pred_prev_q13[0]);
    let mut pred1_q13 = i32::from(state.pred_prev_q13[1]);
    let interp = (STEREO_INTERP_LEN_MS * fs_khz) as usize;
    let denom_q16 = (1i32 << 16) / (STEREO_INTERP_LEN_MS * fs_khz);
    let delta0_q13 = rshift_round(
        smulbb(pred_q13[0] - i32::from(state.pred_prev_q13[0]), denom_q16),
        16,
    );
    let delta1_q13 = rshift_round(
        smulbb(pred_q13[1] - i32::from(state.pred_prev_q13[1]), denom_q16),
        16,
    );
    let mut predict = |n: usize, p0: i32, p1: i32| {
        let sum = lshift(
            (i32::from(x1[n]) + i32::from(x1[n + 2])).wrapping_add(lshift(i32::from(x1[n + 1]), 1)),
            9,
        );
        let sum = smlawb(lshift(i32::from(x2[n + 1]), 8), sum, p0);
        let sum = smlawb(sum, lshift(i32::from(x1[n + 1]), 11), p1);
        x2[n + 1] = sat16(rshift_round(sum, 8)) as i16;
    };
    for n in 0..interp.min(frame_length) {
        pred0_q13 += delta0_q13;
        pred1_q13 += delta1_q13;
        predict(n, pred0_q13, pred1_q13);
    }
    for n in interp..frame_length {
        predict(n, pred_q13[0], pred_q13[1]);
    }
    state.pred_prev_q13 = [pred_q13[0] as i16, pred_q13[1] as i16];
    // Left and right.
    for n in 0..frame_length {
        let sum = i32::from(x1[n + 1]) + i32::from(x2[n + 1]);
        let diff = i32::from(x1[n + 1]) - i32::from(x2[n + 1]);
        x1[n + 1] = sat16(sum) as i16;
        x2[n + 1] = sat16(diff) as i16;
    }
}
