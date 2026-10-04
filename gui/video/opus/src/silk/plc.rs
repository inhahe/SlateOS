//! A SILK channel's concealment of lost frames -- the last good frame's
//! pitch pulse repeated through its LPC filter with noise mixed in, both
//! fading -- the comfort noise added while frames are lost or the encoder
//! sends none, and the smoothing where a good frame follows concealed ones.
//!
//! Translated into Rust from libopus 1.5.2's `silk/PLC.c` (without deep
//! PLC), `silk/CNG.c` and `silk/sum_sqr_shift.c`, copyright Skype Limited,
//! Xiph.Org and the contributors named in its `COPYING`, used under
//! libopus's BSD licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "every index is within the state's buffers at libopus's sizes, the concealment's pitch lag held to 18 ms of a 20 ms LTP memory as libopus holds it"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sizes below 500 and the fixed-point arithmetic of libopus, within 32 bits by its design; what C lets overflow wraps through the fixed-point helpers"
)]

use super::channel::{
    ChannelState, DecoderControl, LTP_ORDER, MAX_FRAME_LENGTH, MAX_LTP_MEM, MAX_NB_SUBFR,
    MAX_SUB_FRAME_LENGTH, TYPE_NO_VOICE_ACTIVITY, TYPE_VOICED, lpc_analysis_filter,
};
use super::fix::{
    add_sat32, clz32, div32, fix_const, inverse32_varq, lshift, lshift_sat32, rand, rshift_round,
    sat16, smlawb, smulbb, smultt, smulwb, smulww, sqrt_approx,
};
use super::nlsf::{MAX_LPC_ORDER, bwexpander, lpc_inverse_pred_gain, nlsf2a};

/// `NB_ATT`: the attenuations for the first lost frame, then the rest.
const HARM_ATT_Q15: [i32; 2] = [32440, 31130];
const PLC_RAND_ATTENUATE_V_Q15: [i32; 2] = [31130, 26214];
const PLC_RAND_ATTENUATE_UV_Q15: [i32; 2] = [32440, 29491];
/// `PLC.h`'s constants.
const V_PITCH_GAIN_START_MIN_Q14: i32 = 11469;
const V_PITCH_GAIN_START_MAX_Q14: i32 = 15565;
const MAX_PITCH_LAG_MS: i32 = 18;
const RAND_BUF_SIZE: usize = 128;
const RAND_BUF_MASK: i32 = RAND_BUF_SIZE as i32 - 1;
const LOG2_INV_LPC_GAIN_HIGH_THRES: i32 = 3;
const LOG2_INV_LPC_GAIN_LOW_THRES: i32 = 8;
const PITCH_DRIFT_FAC_Q16: i32 = 655;
/// `CNG_BUF_MASK_MAX`, `CNG_GAIN_SMTH_Q16`, `CNG_GAIN_SMTH_THRESHOLD_Q16`,
/// `CNG_NLSF_SMTH_Q16`.
const CNG_BUF_MASK_MAX: i32 = 255;
const CNG_GAIN_SMTH_Q16: i32 = 4634;
const CNG_GAIN_SMTH_THRESHOLD_Q16: i32 = 46396;
const CNG_NLSF_SMTH_Q16: i32 = 16348;

/// `silk_sum_sqr_shift`: the energy of `x`, shifted right by the returned
/// shift so as to fit 32 bits with two bits to spare.
pub(crate) fn sum_sqr_shift(x: &[i16]) -> (i32, i32) {
    let len = x.len();
    let pass = |shift: i32, start: i32| -> i32 {
        let mut nrg = start;
        let mut i = 0;
        while i + 1 < len {
            let tmp = (smulbb(i32::from(x[i]), i32::from(x[i])) as u32).wrapping_add(smulbb(
                i32::from(x[i + 1]),
                i32::from(x[i + 1]),
            )
                as u32);
            nrg = (nrg as u32).wrapping_add(tmp >> shift) as i32;
            i += 2;
        }
        if i < len {
            let tmp = smulbb(i32::from(x[i]), i32::from(x[i])) as u32;
            nrg = (nrg as u32).wrapping_add(tmp >> shift) as i32;
        }
        nrg
    };
    // A first pass with the largest shift there could be, starting from
    // `len` to be conservative with the rounding.
    let shift = 31 - clz32(len as i32);
    let nrg = pass(shift, len as i32);
    let shift = 0.max(shift + 3 - clz32(nrg));
    (pass(shift, 0), shift)
}

impl ChannelState {
    /// `silk_PLC_Reset`.
    pub(crate) fn plc_reset(&mut self) {
        self.plc.pitch_l_q8 = lshift(self.frame_length as i32, 8 - 1);
        self.plc.prev_gain_q16 = [fix_const(1.0, 16); 2];
        self.plc.subfr_length = 20;
        self.plc.nb_subfr = 2;
    }

    /// `silk_PLC`: after a good frame, its parameters noted; for a lost one,
    /// the frame concealed into `frame`.
    pub(crate) fn plc(&mut self, ctrl: &mut DecoderControl, frame: &mut [i16], lost: bool) {
        if self.fs_khz != self.plc.fs_khz {
            self.plc_reset();
            self.plc.fs_khz = self.fs_khz;
        }
        if lost {
            self.plc_conceal(ctrl, frame);
            self.loss_cnt += 1;
        } else {
            self.plc_update(ctrl);
        }
    }

    /// `silk_PLC_update`: what concealing the next frame needs.
    fn plc_update(&mut self, ctrl: &DecoderControl) {
        let nb = self.nb_subfr;
        self.prev_signal_type = i32::from(self.indices.signal_type);
        let mut ltp_gain_q14 = 0i32;
        if i32::from(self.indices.signal_type) == TYPE_VOICED {
            // The last subframe with a pitch pulse: the strongest LTP filter
            // of those within a pitch period of the end.
            let mut j = 0;
            while ((j * self.subfr_length) as i32) < ctrl.pitch_l[nb - 1] {
                if j == nb {
                    break;
                }
                let row = (nb - 1 - j) * LTP_ORDER;
                let temp: i32 = ctrl.ltp_coef_q14[row..row + LTP_ORDER]
                    .iter()
                    .map(|&v| i32::from(v))
                    .sum();
                if temp > ltp_gain_q14 {
                    ltp_gain_q14 = temp;
                    self.plc
                        .ltp_coef_q14
                        .copy_from_slice(&ctrl.ltp_coef_q14[row..row + LTP_ORDER]);
                    self.plc.pitch_l_q8 = lshift(ctrl.pitch_l[nb - 1 - j], 8);
                }
                j += 1;
            }
            // (The filter just copied is replaced: only its gain is kept,
            // at the centre tap.)
            self.plc.ltp_coef_q14 = [0; LTP_ORDER];
            self.plc.ltp_coef_q14[LTP_ORDER / 2] = ltp_gain_q14 as i16;
            // The gain held between 0.7 and 0.95.
            if ltp_gain_q14 < V_PITCH_GAIN_START_MIN_Q14 {
                let scale_q10 = div32(lshift(V_PITCH_GAIN_START_MIN_Q14, 10), ltp_gain_q14.max(1));
                for v in &mut self.plc.ltp_coef_q14 {
                    *v = (smulbb(i32::from(*v), scale_q10) >> 10) as i16;
                }
            } else if ltp_gain_q14 > V_PITCH_GAIN_START_MAX_Q14 {
                let scale_q14 = div32(lshift(V_PITCH_GAIN_START_MAX_Q14, 14), ltp_gain_q14.max(1));
                for v in &mut self.plc.ltp_coef_q14 {
                    *v = (smulbb(i32::from(*v), scale_q14) >> 14) as i16;
                }
            }
        } else {
            self.plc.pitch_l_q8 = lshift(smulbb(self.fs_khz, 18), 8);
            self.plc.ltp_coef_q14 = [0; LTP_ORDER];
        }
        let order = self.lpc_order;
        self.plc.prev_lpc_q12[..order].copy_from_slice(&ctrl.pred_coef_q12[1][..order]);
        self.plc.prev_ltp_scale_q14 = ctrl.ltp_scale_q14 as i16;
        self.plc.prev_gain_q16 = [ctrl.gains_q16[nb - 2], ctrl.gains_q16[nb - 1]];
        self.plc.subfr_length = self.subfr_length;
        self.plc.nb_subfr = nb;
    }

    /// `silk_PLC_energy`: the energies of the last two subframes'
    /// excitations, each with its shift.
    fn plc_energy(&self, prev_gain_q10: [i32; 2]) -> ((i32, i32), (i32, i32)) {
        let subfr = self.subfr_length;
        let mut buf = [0i16; 2 * MAX_SUB_FRAME_LENGTH];
        let buf = &mut buf[..2 * subfr];
        for k in 0..2 {
            for i in 0..subfr {
                let e = self.exc_q14[i + (k + self.nb_subfr - 2) * subfr];
                buf[k * subfr + i] = sat16(smulww(e, prev_gain_q10[k]) >> 8) as i16;
            }
        }
        (sum_sqr_shift(&buf[..subfr]), sum_sqr_shift(&buf[subfr..]))
    }

    /// `silk_PLC_conceal`: a lost frame from the last good one's pitch,
    /// filter and excitation.
    fn plc_conceal(&mut self, ctrl: &mut DecoderControl, frame: &mut [i16]) {
        let ltp_mem = self.ltp_mem_length;
        let fl = self.frame_length;
        let order = self.lpc_order;
        let mut s_ltp_q14 = [0i32; MAX_LTP_MEM + MAX_FRAME_LENGTH];
        let s_ltp_q14 = &mut s_ltp_q14[..ltp_mem + fl];
        let mut s_ltp = [0i16; MAX_LTP_MEM];
        let s_ltp = &mut s_ltp[..ltp_mem];
        let prev_gain_q10 = [
            self.plc.prev_gain_q16[0] >> 6,
            self.plc.prev_gain_q16[1] >> 6,
        ];
        if self.first_frame_after_reset {
            self.plc.prev_lpc_q12 = [0; MAX_LPC_ORDER];
        }
        let ((energy1, shift1), (energy2, shift2)) = self.plc_energy(prev_gain_q10);
        // The quieter of the last two subframes is the noise source.
        let rand_at = if (energy1 >> shift2) < (energy2 >> shift1) {
            ((self.plc.nb_subfr - 1) * self.plc.subfr_length).saturating_sub(RAND_BUF_SIZE)
        } else {
            (self.plc.nb_subfr * self.plc.subfr_length).saturating_sub(RAND_BUF_SIZE)
        };
        let mut rand_scale_q14 = self.plc.rand_scale_q14;
        let att = (self.loss_cnt.max(0) as usize).min(1);
        let harm_gain_q15 = HARM_ATT_Q15[att];
        let mut rand_gain_q15 = if self.prev_signal_type == TYPE_VOICED {
            PLC_RAND_ATTENUATE_V_Q15[att]
        } else {
            PLC_RAND_ATTENUATE_UV_Q15[att]
        };
        // The last filter, bandwidth-expanded (again, each lost frame).
        bwexpander(&mut self.plc.prev_lpc_q12[..order], fix_const(0.99, 16));
        let a_q12 = self.plc.prev_lpc_q12;
        if self.loss_cnt == 0 {
            // The first lost frame.
            rand_scale_q14 = 1 << 14;
            if self.prev_signal_type == TYPE_VOICED {
                // Less noise for voiced frames.
                for &b in &self.plc.ltp_coef_q14 {
                    rand_scale_q14 = (i32::from(rand_scale_q14) - i32::from(b)) as i16;
                }
                rand_scale_q14 = rand_scale_q14.max(3277);
                rand_scale_q14 = (smulbb(
                    i32::from(rand_scale_q14),
                    i32::from(self.plc.prev_ltp_scale_q14),
                ) >> 14) as i16;
            } else {
                // Less noise for unvoiced frames of high LPC gain.
                let inv_gain_q30 = lpc_inverse_pred_gain(&self.plc.prev_lpc_q12[..order]);
                let down = ((1i32 << 30) >> LOG2_INV_LPC_GAIN_HIGH_THRES).min(inv_gain_q30);
                let down = ((1i32 << 30) >> LOG2_INV_LPC_GAIN_LOW_THRES).max(down);
                let down = lshift(down, LOG2_INV_LPC_GAIN_HIGH_THRES);
                rand_gain_q15 = smulwb(down, rand_gain_q15) >> 14;
            }
        }
        let mut rand_seed = self.plc.rand_seed;
        let mut lag = rshift_round(self.plc.pitch_l_q8, 8);
        let mut s_ltp_buf_idx = ltp_mem;
        // The LTP state re-whitened.
        let lag_n = usize::try_from(lag).unwrap_or(0);
        let idx = ltp_mem.saturating_sub(lag_n + order + LTP_ORDER / 2).max(1);
        lpc_analysis_filter(
            &mut s_ltp[idx..],
            &self.out_buf[idx..],
            &a_q12,
            ltp_mem - idx,
            order,
        );
        // And scaled.
        let inv_gain_q30 = inverse32_varq(self.plc.prev_gain_q16[1], 46).min(i32::MAX >> 1);
        for i in idx + order..ltp_mem {
            s_ltp_q14[i] = smulwb(inv_gain_q30, i32::from(s_ltp[i]));
        }
        // LTP synthesis.
        let b_q14 = &mut self.plc.ltp_coef_q14;
        for _ in 0..self.nb_subfr {
            let first_lag = s_ltp_buf_idx + LTP_ORDER / 2 - usize::try_from(lag).unwrap_or(0);
            for pred_lag in first_lag..first_lag + self.subfr_length {
                // Rounded up: SMLAWB rounds down.
                let mut ltp_pred_q12 = 2;
                for (j, &b) in b_q14.iter().enumerate() {
                    ltp_pred_q12 = smlawb(ltp_pred_q12, s_ltp_q14[pred_lag - j], i32::from(b));
                }
                // The excitation: noise from the source subframe.
                rand_seed = rand(rand_seed);
                let ridx = ((rand_seed >> 25) & RAND_BUF_MASK) as usize;
                s_ltp_q14[s_ltp_buf_idx] = lshift(
                    smlawb(
                        ltp_pred_q12,
                        self.exc_q14[rand_at + ridx],
                        i32::from(rand_scale_q14),
                    ),
                    2,
                );
                s_ltp_buf_idx += 1;
            }
            // The LTP gain and the excitation's fade, and the pitch drifting
            // longer.
            for b in b_q14.iter_mut() {
                *b = (smulbb(harm_gain_q15, i32::from(*b)) >> 15) as i16;
            }
            rand_scale_q14 = (smulbb(i32::from(rand_scale_q14), rand_gain_q15) >> 15) as i16;
            self.plc.pitch_l_q8 = smlawb(
                self.plc.pitch_l_q8,
                self.plc.pitch_l_q8,
                PITCH_DRIFT_FAC_Q16,
            );
            self.plc.pitch_l_q8 = self
                .plc
                .pitch_l_q8
                .min(lshift(smulbb(MAX_PITCH_LAG_MS, self.fs_khz), 8));
            lag = rshift_round(self.plc.pitch_l_q8, 8);
        }
        // LPC synthesis, over the LTP output in place, its state at the
        // LTP memory's end.
        let base = ltp_mem - MAX_LPC_ORDER;
        s_ltp_q14[base..ltp_mem].copy_from_slice(&self.s_lpc_q14_buf);
        for i in 0..fl {
            let mut lpc_pred_q10 = (order >> 1) as i32;
            for j in 0..order {
                lpc_pred_q10 = smlawb(
                    lpc_pred_q10,
                    s_ltp_q14[base + MAX_LPC_ORDER + i - j - 1],
                    i32::from(a_q12[j]),
                );
            }
            let at = base + MAX_LPC_ORDER + i;
            s_ltp_q14[at] = add_sat32(s_ltp_q14[at], lshift_sat32(lpc_pred_q10, 4));
            frame[i] = sat16(sat16(rshift_round(
                smulww(s_ltp_q14[at], prev_gain_q10[1]),
                8,
            ))) as i16;
        }
        self.s_lpc_q14_buf
            .copy_from_slice(&s_ltp_q14[base + fl..base + fl + MAX_LPC_ORDER]);
        self.plc.rand_seed = rand_seed;
        self.plc.rand_scale_q14 = rand_scale_q14;
        ctrl.pitch_l = [lag; MAX_NB_SUBFR];
    }

    /// `silk_PLC_glue_frames`: a concealed frame's energy noted; the first
    /// good frame after it faded in from that energy, where it is louder.
    pub(crate) fn plc_glue_frames(&mut self, frame: &mut [i16], length: usize) {
        if self.loss_cnt != 0 {
            let (e, s) = sum_sqr_shift(&frame[..length]);
            self.plc.conc_energy = e;
            self.plc.conc_energy_shift = s;
            self.plc.last_frame_lost = true;
            return;
        }
        if self.plc.last_frame_lost {
            let (mut energy, energy_shift) = sum_sqr_shift(&frame[..length]);
            // The energies on one scale.
            if energy_shift > self.plc.conc_energy_shift {
                self.plc.conc_energy >>= energy_shift - self.plc.conc_energy_shift;
            } else if energy_shift < self.plc.conc_energy_shift {
                energy >>= self.plc.conc_energy_shift - energy_shift;
            }
            if energy > self.plc.conc_energy {
                let lz = clz32(self.plc.conc_energy) - 1;
                self.plc.conc_energy = lshift(self.plc.conc_energy, lz);
                let energy = energy >> 0.max(24 - lz);
                let frac_q24 = div32(self.plc.conc_energy, energy.max(1));
                let mut gain_q16 = lshift(sqrt_approx(frac_q24), 4);
                // Four times steeper, not to miss an onset after DTX.
                let slope_q16 = lshift(div32((1 << 16) - gain_q16, length as i32), 2);
                for v in &mut frame[..length] {
                    *v = smulwb(gain_q16, i32::from(*v)) as i16;
                    gain_q16 += slope_q16;
                    if gain_q16 > 1 << 16 {
                        break;
                    }
                }
            }
        }
        self.plc.last_frame_lost = false;
    }

    /// `silk_CNG_Reset`.
    pub(crate) fn cng_reset(&mut self) {
        let step = div32(i32::from(i16::MAX), self.lpc_order as i32 + 1);
        let mut acc = 0;
        for v in &mut self.cng.smth_nlsf_q15[..self.lpc_order] {
            acc += step;
            *v = acc as i16;
        }
        self.cng.smth_gain_q16 = 0;
        self.cng.rand_seed = 3_176_576;
    }

    /// `silk_CNG`: the noise estimate updated from a good unvoiced frame;
    /// noise added to a lost frame (or one in DTX).
    pub(crate) fn cng(&mut self, ctrl: &DecoderControl, frame: &mut [i16], length: usize) {
        let order = self.lpc_order;
        if self.fs_khz != self.cng.fs_khz {
            self.cng_reset();
            self.cng.fs_khz = self.fs_khz;
        }
        if self.loss_cnt == 0 && self.prev_signal_type == TYPE_NO_VOICE_ACTIVITY {
            // The NLSFs smoothed toward the frame's.
            for i in 0..order {
                let d = i32::from(self.prev_nlsf_q15[i]) - i32::from(self.cng.smth_nlsf_q15[i]);
                self.cng.smth_nlsf_q15[i] =
                    (i32::from(self.cng.smth_nlsf_q15[i]) + smulwb(d, CNG_NLSF_SMTH_Q16)) as i16;
            }
            // The excitation of the loudest subframe into the buffer.
            let mut max_gain_q16 = 0;
            let mut subfr = 0;
            for i in 0..self.nb_subfr {
                if ctrl.gains_q16[i] > max_gain_q16 {
                    max_gain_q16 = ctrl.gains_q16[i];
                    subfr = i;
                }
            }
            let sl = self.subfr_length;
            self.cng
                .exc_buf_q14
                .copy_within(0..(self.nb_subfr - 1) * sl, sl);
            self.cng.exc_buf_q14[..sl].copy_from_slice(&self.exc_q14[subfr * sl..(subfr + 1) * sl]);
            // The gains smoothed; down fast past 3 dB.
            for i in 0..self.nb_subfr {
                self.cng.smth_gain_q16 = self.cng.smth_gain_q16.wrapping_add(smulwb(
                    ctrl.gains_q16[i].wrapping_sub(self.cng.smth_gain_q16),
                    CNG_GAIN_SMTH_Q16,
                ));
                if smulww(self.cng.smth_gain_q16, CNG_GAIN_SMTH_THRESHOLD_Q16) > ctrl.gains_q16[i] {
                    self.cng.smth_gain_q16 = ctrl.gains_q16[i];
                }
            }
        }
        if self.loss_cnt == 0 {
            self.cng.synth_state[..order].fill(0);
            return;
        }
        // Comfort noise: the buffer's excitation at a random walk, shaped by
        // the smoothed NLSFs, at the gain the concealment leaves unfilled.
        let mut sig_q14 = [0i32; MAX_FRAME_LENGTH + MAX_LPC_ORDER];
        let sig_q14 = &mut sig_q14[..length + MAX_LPC_ORDER];
        let mut gain_q16 = smulww(
            i32::from(self.plc.rand_scale_q14),
            self.plc.prev_gain_q16[1],
        );
        let g = self.cng.smth_gain_q16;
        if gain_q16 >= 1 << 21 || g > 1 << 23 {
            gain_q16 = smultt(gain_q16, gain_q16);
            gain_q16 = smultt(g, g).wrapping_sub(lshift(gain_q16, 5));
            gain_q16 = lshift(sqrt_approx(gain_q16), 16);
        } else {
            gain_q16 = smulww(gain_q16, gain_q16);
            gain_q16 = smulww(g, g).wrapping_sub(lshift(gain_q16, 5));
            gain_q16 = lshift(sqrt_approx(gain_q16), 8);
        }
        let gain_q10 = gain_q16 >> 6;
        // `silk_CNG_exc`.
        let mut exc_mask = CNG_BUF_MASK_MAX;
        while exc_mask > length as i32 {
            exc_mask >>= 1;
        }
        let mut seed = self.cng.rand_seed;
        for v in &mut sig_q14[MAX_LPC_ORDER..] {
            seed = rand(seed);
            *v = self.cng.exc_buf_q14[((seed >> 24) & exc_mask) as usize];
        }
        self.cng.rand_seed = seed;
        let mut a_q12 = [0i16; MAX_LPC_ORDER];
        nlsf2a(&mut a_q12, &self.cng.smth_nlsf_q15, order);
        sig_q14[..MAX_LPC_ORDER].copy_from_slice(&self.cng.synth_state);
        for i in 0..length {
            let mut lpc_pred_q10 = (order >> 1) as i32;
            for j in 0..order {
                lpc_pred_q10 = smlawb(
                    lpc_pred_q10,
                    sig_q14[MAX_LPC_ORDER + i - j - 1],
                    i32::from(a_q12[j]),
                );
            }
            let at = MAX_LPC_ORDER + i;
            sig_q14[at] = add_sat32(sig_q14[at], lshift_sat32(lpc_pred_q10, 4));
            let noise = sat16(rshift_round(smulww(sig_q14[at], gain_q10), 8));
            frame[i] = sat16(i32::from(frame[i]) + noise) as i16;
        }
        self.cng
            .synth_state
            .copy_from_slice(&sig_q14[length..length + MAX_LPC_ORDER]);
    }
}

/// The concealment's noise comes from at most this much of the last frame.
const _: () = assert!(RAND_BUF_SIZE <= MAX_FRAME_LENGTH);
