//! One SILK channel's decoder (`silk_decoder_state`): a frame's side
//! information -- signal type, gains, NLSFs, pitch lags, LTP filters -- and
//! its excitation pulses read from the range decoder, the parameters
//! dequantised, and the excitation put through the long-term (pitch) and
//! short-term (LPC) synthesis filters; or the frame concealed.
//!
//! Translated into Rust from libopus 1.5.2's `silk/init_decoder.c`,
//! `silk/decoder_set_fs.c`, `silk/decode_frame.c`, `silk/decode_indices.c`,
//! `silk/decode_pulses.c`, `silk/shell_coder.c` and `silk/code_signs.c`
//! (their decoders), `silk/decode_parameters.c`, `silk/gain_quant.c`
//! (`silk_gains_dequant`), `silk/decode_pitch.c`, `silk/decode_core.c` and
//! `silk/LPC_analysis_filter.c`, copyright Skype Limited, Xiph.Org and the
//! contributors named in its `COPYING`, used under libopus's BSD licence
//! (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "every index is within the state's buffers at libopus's sizes: frames of at most 320 samples, subframes of 80, an LTP memory of 320 and an output buffer of 480, pitch lags held between 2 and 18 ms -- which keeps every LTP and LPC history index inside its buffer, as libopus's asserts say"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sizes below 500 and the fixed-point arithmetic of libopus, within 32 bits by its design; what C lets overflow wraps through the fixed-point helpers"
)]

use super::LostFlag;
use super::codebook::{
    LTP_GAIN_ICDF_PTRS, LTP_VQ_PTRS_Q7, NLSF_CB_NB_MB, NLSF_CB_WB, NlsfCodebook,
};
use super::fix::{
    add_sat32, div32_varq, fix_const, inverse32_varq, limit, log2lin, lshift, lshift_sat32, rand,
    rshift_round, sat16, smlawb, smulbb, smulwb, smulww,
};
use super::nlsf::{self, MAX_LPC_ORDER, NLSF_QUANT_MAX_AMPLITUDE, bwexpander};
use super::resampler::Resampler;
use super::tables::{
    CB_LAGS_STAGE2, CB_LAGS_STAGE2_10_MS, CB_LAGS_STAGE3, CB_LAGS_STAGE3_10_MS, DELTA_GAIN_ICDF,
    GAIN_ICDF, LSB_ICDF, LTP_PER_INDEX_ICDF, LTPSCALE_ICDF, LTPSCALES_TABLE_Q14, NLSF_EXT_ICDF,
    NLSF_INTERPOLATION_FACTOR_ICDF, PITCH_CONTOUR_10_MS_ICDF, PITCH_CONTOUR_10_MS_NB_ICDF,
    PITCH_CONTOUR_ICDF, PITCH_CONTOUR_NB_ICDF, PITCH_DELTA_ICDF, PITCH_LAG_ICDF,
    PULSES_PER_BLOCK_ICDF, QUANTIZATION_OFFSETS_Q10, RATE_LEVELS_ICDF, SHELL_CODE_TABLE_OFFSETS,
    SHELL_CODE_TABLE0, SHELL_CODE_TABLE1, SHELL_CODE_TABLE2, SHELL_CODE_TABLE3, SIGN_ICDF,
    TYPE_OFFSET_NO_VAD_ICDF, TYPE_OFFSET_VAD_ICDF, UNIFORM4_ICDF, UNIFORM6_ICDF, UNIFORM8_ICDF,
};
use crate::entdec::Decoder as RangeDecoder;

/// `MAX_FRAME_LENGTH`: 20 ms at 16 kHz.
pub(crate) const MAX_FRAME_LENGTH: usize = 320;
/// `MAX_SUB_FRAME_LENGTH`: 5 ms at 16 kHz.
pub(crate) const MAX_SUB_FRAME_LENGTH: usize = 80;
/// `MAX_NB_SUBFR`.
pub(crate) const MAX_NB_SUBFR: usize = 4;
/// `LTP_ORDER`: the pitch filter's taps.
pub(crate) const LTP_ORDER: usize = 5;
/// `MAX_FRAMES_PER_PACKET`: 60 ms of 20 ms frames.
pub(crate) const MAX_FRAMES_PER_PACKET: usize = 3;
/// The signal types.
pub(crate) const TYPE_NO_VOICE_ACTIVITY: i32 = 0;
pub(crate) const TYPE_VOICED: i32 = 2;
/// How a frame's parameters are coded against the frame before.
pub(crate) const CODE_INDEPENDENTLY: i32 = 0;
pub(crate) const CODE_INDEPENDENTLY_NO_LTP_SCALING: i32 = 1;
pub(crate) const CODE_CONDITIONALLY: i32 = 2;
/// `SHELL_CODEC_FRAME_LENGTH`.
const SHELL_LEN: usize = 16;
/// `SILK_MAX_PULSES`, `N_RATE_LEVELS`.
const SILK_MAX_PULSES: i32 = 16;
const N_RATE_LEVELS: usize = 10;
/// The gain quantiser (`gain_quant.c`): `OFFSET`, `INV_SCALE_Q16`, and the
/// index's limits.
const GAIN_OFFSET: i32 = (2 * 128) / 6 + 16 * 128;
const INV_SCALE_Q16: i32 = (65536 * (((88 - 2) * 128) / 6)) / (64 - 1);
const N_LEVELS_QGAIN: i32 = 64;
const MAX_DELTA_GAIN_QUANT: i32 = 36;
const MIN_DELTA_GAIN_QUANT: i32 = -4;
/// `QUANT_LEVEL_ADJUST_Q10`, `BWE_AFTER_LOSS_Q16`.
const QUANT_LEVEL_ADJUST_Q10: i32 = 80;
const BWE_AFTER_LOSS_Q16: i32 = 63570;
/// `LTP_MEM_LENGTH_MS`, `SUB_FRAME_LENGTH_MS`, `MIN_LPC_ORDER`.
const LTP_MEM_LENGTH_MS: i32 = 20;
const SUB_FRAME_LENGTH_MS: i32 = 5;
const MIN_LPC_ORDER: usize = 10;
/// The pitch's range: 2 to 18 ms (`PE_MIN_LAG_MS`, `PE_MAX_LAG_MS`).
const PE_MIN_LAG_MS: i32 = 2;
const PE_MAX_LAG_MS: i32 = 18;

/// A frame's quantisation indices (`SideInfoIndices`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SideInfoIndices {
    pub gains_indices: [i8; MAX_NB_SUBFR],
    pub ltp_index: [i8; MAX_NB_SUBFR],
    pub nlsf_indices: [i8; MAX_LPC_ORDER + 1],
    pub lag_index: i16,
    pub contour_index: i8,
    pub signal_type: i8,
    pub quant_offset_type: i8,
    pub nlsf_interp_coef_q2: i8,
    pub per_index: i8,
    pub ltp_scale_index: i8,
    pub seed: i8,
}

/// A frame's dequantised parameters (`silk_decoder_control`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DecoderControl {
    pub pitch_l: [i32; MAX_NB_SUBFR],
    pub gains_q16: [i32; MAX_NB_SUBFR],
    /// The LPC filters of the first and second half of the frame, Q12.
    pub pred_coef_q12: [[i16; MAX_LPC_ORDER]; 2],
    pub ltp_coef_q14: [i16; LTP_ORDER * MAX_NB_SUBFR],
    pub ltp_scale_q14: i32,
}

/// The concealment's state (`silk_PLC_struct`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PlcState {
    pub pitch_l_q8: i32,
    pub ltp_coef_q14: [i16; LTP_ORDER],
    pub prev_lpc_q12: [i16; MAX_LPC_ORDER],
    pub last_frame_lost: bool,
    pub rand_seed: i32,
    pub rand_scale_q14: i16,
    pub conc_energy: i32,
    pub conc_energy_shift: i32,
    pub prev_ltp_scale_q14: i16,
    pub prev_gain_q16: [i32; 2],
    pub fs_khz: i32,
    pub nb_subfr: usize,
    pub subfr_length: usize,
}

/// Comfort noise's state (`silk_CNG_struct`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct CngState {
    pub exc_buf_q14: [i32; MAX_FRAME_LENGTH],
    pub smth_nlsf_q15: [i16; MAX_LPC_ORDER],
    pub synth_state: [i32; MAX_LPC_ORDER],
    pub smth_gain_q16: i32,
    pub rand_seed: i32,
    pub fs_khz: i32,
}

impl Default for CngState {
    fn default() -> Self {
        Self {
            exc_buf_q14: [0; MAX_FRAME_LENGTH],
            smth_nlsf_q15: [0; MAX_LPC_ORDER],
            synth_state: [0; MAX_LPC_ORDER],
            smth_gain_q16: 0,
            rand_seed: 0,
            fs_khz: 0,
        }
    }
}

/// One channel's decoder state (`silk_decoder_state`).
#[derive(Clone, Debug)]
pub(crate) struct ChannelState {
    pub prev_gain_q16: i32,
    pub exc_q14: [i32; MAX_FRAME_LENGTH],
    pub s_lpc_q14_buf: [i32; MAX_LPC_ORDER],
    pub out_buf: [i16; MAX_FRAME_LENGTH + 2 * MAX_SUB_FRAME_LENGTH],
    pub lag_prev: i32,
    pub last_gain_index: i8,
    pub fs_khz: i32,
    pub fs_api_hz: i32,
    pub nb_subfr: usize,
    pub frame_length: usize,
    pub subfr_length: usize,
    pub ltp_mem_length: usize,
    pub lpc_order: usize,
    pub prev_nlsf_q15: [i16; MAX_LPC_ORDER],
    pub first_frame_after_reset: bool,
    pub pitch_lag_low_bits_icdf: &'static [u8],
    pub pitch_contour_icdf: &'static [u8],
    pub n_frames_decoded: usize,
    pub n_frames_per_packet: usize,
    pub ec_prev_signal_type: i32,
    pub ec_prev_lag_index: i16,
    pub vad_flags: [bool; MAX_FRAMES_PER_PACKET],
    pub lbrr_flag: bool,
    pub lbrr_flags: [bool; MAX_FRAMES_PER_PACKET],
    pub resampler: Resampler,
    pub nlsf_cb: &'static NlsfCodebook,
    pub indices: SideInfoIndices,
    pub cng: CngState,
    pub loss_cnt: i32,
    pub prev_signal_type: i32,
    pub plc: PlcState,
}

impl ChannelState {
    /// `silk_init_decoder`: a channel never used.
    pub(crate) fn new() -> Self {
        let mut s = Self::cleared();
        s.reset();
        s
    }

    /// `silk_reset_decoder`: everything cleared (libopus clears the whole
    /// state from its first field), then the few values that start other
    /// than 0.
    pub(crate) fn reset(&mut self) {
        *self = Self {
            first_frame_after_reset: true,
            prev_gain_q16: 65536,
            ..Self::cleared()
        };
        self.cng_reset();
        self.plc_reset();
    }

    /// Every field 0 (the codebook and tables at the defaults `set_fs`
    /// replaces before any frame is decoded).
    fn cleared() -> Self {
        Self {
            prev_gain_q16: 0,
            exc_q14: [0; MAX_FRAME_LENGTH],
            s_lpc_q14_buf: [0; MAX_LPC_ORDER],
            out_buf: [0; MAX_FRAME_LENGTH + 2 * MAX_SUB_FRAME_LENGTH],
            lag_prev: 0,
            last_gain_index: 0,
            fs_khz: 0,
            fs_api_hz: 0,
            nb_subfr: 0,
            frame_length: 0,
            subfr_length: 0,
            ltp_mem_length: 0,
            lpc_order: 0,
            prev_nlsf_q15: [0; MAX_LPC_ORDER],
            first_frame_after_reset: false,
            pitch_lag_low_bits_icdf: &[],
            pitch_contour_icdf: &[],
            n_frames_decoded: 0,
            n_frames_per_packet: 0,
            ec_prev_signal_type: 0,
            ec_prev_lag_index: 0,
            vad_flags: [false; MAX_FRAMES_PER_PACKET],
            lbrr_flag: false,
            lbrr_flags: [false; MAX_FRAMES_PER_PACKET],
            resampler: Resampler::default(),
            nlsf_cb: &NLSF_CB_NB_MB,
            indices: SideInfoIndices::default(),
            cng: CngState::default(),
            loss_cnt: 0,
            prev_signal_type: 0,
            plc: PlcState::default(),
        }
    }

    /// `silk_decoder_set_fs`: the internal rate (8, 12 or 16 kHz) and the
    /// output rate, the frame's shape following from them and `nb_subfr`.
    pub(crate) fn set_fs(&mut self, fs_khz: i32, fs_api_hz: i32) -> Result<(), ()> {
        self.subfr_length = smulbb(SUB_FRAME_LENGTH_MS, fs_khz) as usize;
        let frame_length = self.nb_subfr * self.subfr_length;
        // The resampler, when either rate changes.
        if self.fs_khz != fs_khz || self.fs_api_hz != fs_api_hz {
            self.resampler = Resampler::new(smulbb(fs_khz, 1000), fs_api_hz).ok_or(())?;
            self.fs_api_hz = fs_api_hz;
        }
        if self.fs_khz != fs_khz || frame_length != self.frame_length {
            let twenty_ms = self.nb_subfr == MAX_NB_SUBFR;
            self.pitch_contour_icdf = match (fs_khz == 8, twenty_ms) {
                (true, true) => &PITCH_CONTOUR_NB_ICDF,
                (true, false) => &PITCH_CONTOUR_10_MS_NB_ICDF,
                (false, true) => &PITCH_CONTOUR_ICDF,
                (false, false) => &PITCH_CONTOUR_10_MS_ICDF,
            };
            if self.fs_khz != fs_khz {
                self.ltp_mem_length = smulbb(LTP_MEM_LENGTH_MS, fs_khz) as usize;
                if fs_khz == 8 || fs_khz == 12 {
                    self.lpc_order = MIN_LPC_ORDER;
                    self.nlsf_cb = &NLSF_CB_NB_MB;
                } else {
                    self.lpc_order = MAX_LPC_ORDER;
                    self.nlsf_cb = &NLSF_CB_WB;
                }
                self.pitch_lag_low_bits_icdf = match fs_khz {
                    16 => &UNIFORM8_ICDF,
                    12 => &UNIFORM6_ICDF,
                    _ => &UNIFORM4_ICDF,
                };
                self.first_frame_after_reset = true;
                self.lag_prev = 100;
                self.last_gain_index = 10;
                self.prev_signal_type = TYPE_NO_VOICE_ACTIVITY;
                self.out_buf.fill(0);
                self.s_lpc_q14_buf.fill(0);
            }
            self.fs_khz = fs_khz;
            self.frame_length = frame_length;
        }
        Ok(())
    }

    /// `silk_decode_frame`: a frame into `out` (`frame_length` samples),
    /// decoded or concealed; the samples written.
    pub(crate) fn decode_frame(
        &mut self,
        dec: &mut RangeDecoder<'_>,
        out: &mut [i16],
        lost: LostFlag,
        cond_coding: i32,
    ) -> usize {
        let l = self.frame_length;
        let mut ctrl = DecoderControl::default();
        let decode = lost == LostFlag::Normal
            || (lost == LostFlag::Lbrr && self.lbrr_flags[self.n_frames_decoded]);
        if decode {
            let mut pulses = vec![0i16; (l + SHELL_LEN - 1) & !(SHELL_LEN - 1)];
            self.decode_indices(
                dec,
                self.n_frames_decoded,
                lost == LostFlag::Lbrr,
                cond_coding,
            );
            decode_pulses(
                dec,
                &mut pulses,
                i32::from(self.indices.signal_type),
                i32::from(self.indices.quant_offset_type),
                l,
            );
            self.decode_parameters(&mut ctrl, cond_coding);
            self.decode_core(&mut ctrl, out, &pulses);
            self.update_out_buf(out);
            self.plc(&mut ctrl, out, false);
            self.loss_cnt = 0;
            self.prev_signal_type = i32::from(self.indices.signal_type);
            // A frame decoded without errors.
            self.first_frame_after_reset = false;
        } else {
            // Extrapolated.
            self.plc(&mut ctrl, out, true);
            self.update_out_buf(out);
        }
        // Comfort noise, and a smooth join of concealed and decoded frames.
        self.cng(&ctrl, out, l);
        self.plc_glue_frames(out, l);
        self.lag_prev = ctrl.pitch_l[self.nb_subfr - 1];
        l
    }

    /// The output buffer moved on by a frame, the frame at its end.
    fn update_out_buf(&mut self, out: &[i16]) {
        let fl = self.frame_length;
        let mv_len = self.ltp_mem_length - fl;
        self.out_buf.copy_within(fl..fl + mv_len, 0);
        self.out_buf[mv_len..mv_len + fl].copy_from_slice(&out[..fl]);
    }

    /// `silk_decode_indices`: a frame's side information.
    pub(crate) fn decode_indices(
        &mut self,
        dec: &mut RangeDecoder<'_>,
        frame_index: usize,
        decode_lbrr: bool,
        cond_coding: i32,
    ) {
        let icdf = |dec: &mut RangeDecoder<'_>, t: &[u8]| dec.icdf(t, 8) as i32;
        // The signal type and quantiser offset.
        let ix = if decode_lbrr || self.vad_flags[frame_index] {
            icdf(dec, &TYPE_OFFSET_VAD_ICDF) + 2
        } else {
            icdf(dec, &TYPE_OFFSET_NO_VAD_ICDF)
        };
        let ind = &mut self.indices;
        ind.signal_type = (ix >> 1) as i8;
        ind.quant_offset_type = (ix & 1) as i8;
        // The gains: the first absolute (its 3 low bits uniform) or a delta.
        if cond_coding == CODE_CONDITIONALLY {
            ind.gains_indices[0] = icdf(dec, &DELTA_GAIN_ICDF) as i8;
        } else {
            let msb = icdf(dec, &GAIN_ICDF[ind.signal_type as usize]);
            ind.gains_indices[0] = lshift(msb, 3) as i8;
            ind.gains_indices[0] =
                (i32::from(ind.gains_indices[0]) + icdf(dec, &UNIFORM8_ICDF)) as i8;
        }
        for i in 1..self.nb_subfr {
            ind.gains_indices[i] = icdf(dec, &DELTA_GAIN_ICDF) as i8;
        }
        // The NLSFs: the first stage, then each residual (with an escape for
        // the extremes).
        let cb = self.nlsf_cb;
        let first_row = (ind.signal_type as usize >> 1) * cb.n_vectors;
        ind.nlsf_indices[0] = icdf(dec, &cb.cb1_icdf[first_row..]) as i8;
        let mut ec_ix = [0i16; MAX_LPC_ORDER];
        let mut pred_q8 = [0u8; MAX_LPC_ORDER];
        nlsf::unpack(&mut ec_ix, &mut pred_q8, cb, ind.nlsf_indices[0] as usize);
        for i in 0..cb.order {
            let mut ix = icdf(dec, &cb.ec_icdf[ec_ix[i] as usize..]);
            if ix == 0 {
                ix -= icdf(dec, &NLSF_EXT_ICDF);
            } else if ix == 2 * NLSF_QUANT_MAX_AMPLITUDE {
                ix += icdf(dec, &NLSF_EXT_ICDF);
            }
            ind.nlsf_indices[i + 1] = (ix - NLSF_QUANT_MAX_AMPLITUDE) as i8;
        }
        ind.nlsf_interp_coef_q2 = if self.nb_subfr == MAX_NB_SUBFR {
            icdf(dec, &NLSF_INTERPOLATION_FACTOR_ICDF) as i8
        } else {
            4
        };
        if i32::from(ind.signal_type) == TYPE_VOICED {
            // The pitch lag: a delta from the last frame's, or absolute.
            let mut absolute = true;
            if cond_coding == CODE_CONDITIONALLY && self.ec_prev_signal_type == TYPE_VOICED {
                let delta = icdf(dec, &PITCH_DELTA_ICDF);
                if delta > 0 {
                    ind.lag_index = (i32::from(self.ec_prev_lag_index) + delta - 9) as i16;
                    absolute = false;
                }
            }
            if absolute {
                ind.lag_index = (icdf(dec, &PITCH_LAG_ICDF) * (self.fs_khz >> 1)) as i16;
                ind.lag_index =
                    (i32::from(ind.lag_index) + icdf(dec, self.pitch_lag_low_bits_icdf)) as i16;
            }
            self.ec_prev_lag_index = ind.lag_index;
            ind.contour_index = icdf(dec, self.pitch_contour_icdf) as i8;
            // The LTP filters' codebook and each subframe's filter.
            ind.per_index = icdf(dec, &LTP_PER_INDEX_ICDF) as i8;
            for k in 0..self.nb_subfr {
                ind.ltp_index[k] = icdf(dec, LTP_GAIN_ICDF_PTRS[ind.per_index as usize]) as i8;
            }
            ind.ltp_scale_index = if cond_coding == CODE_INDEPENDENTLY {
                icdf(dec, &LTPSCALE_ICDF) as i8
            } else {
                0
            };
        }
        self.ec_prev_signal_type = i32::from(ind.signal_type);
        ind.seed = icdf(dec, &UNIFORM4_ICDF) as i8;
    }

    /// `silk_decode_parameters`: the indices dequantised into `ctrl`.
    fn decode_parameters(&mut self, ctrl: &mut DecoderControl, cond_coding: i32) {
        let order = self.lpc_order;
        gains_dequant(
            &mut ctrl.gains_q16,
            &self.indices.gains_indices,
            &mut self.last_gain_index,
            cond_coding == CODE_CONDITIONALLY,
            self.nb_subfr,
        );
        let mut nlsf_q15 = [0i16; MAX_LPC_ORDER];
        nlsf::decode(&mut nlsf_q15, &self.indices.nlsf_indices, self.nlsf_cb);
        nlsf::nlsf2a(&mut ctrl.pred_coef_q12[1], &nlsf_q15, order);
        // No interpolation just after a reset (the rate changed, say).
        if self.first_frame_after_reset {
            self.indices.nlsf_interp_coef_q2 = 4;
        }
        let coef = i32::from(self.indices.nlsf_interp_coef_q2);
        if coef < 4 {
            // The first half's NLSFs between the last frame's and these.
            let mut nlsf0 = [0i16; MAX_LPC_ORDER];
            for i in 0..order {
                let prev = i32::from(self.prev_nlsf_q15[i]);
                nlsf0[i] = (prev + ((coef * (i32::from(nlsf_q15[i]) - prev)) >> 2)) as i16;
            }
            nlsf::nlsf2a(&mut ctrl.pred_coef_q12[0], &nlsf0, order);
        } else {
            let second = ctrl.pred_coef_q12[1];
            ctrl.pred_coef_q12[0][..order].copy_from_slice(&second[..order]);
        }
        self.prev_nlsf_q15[..order].copy_from_slice(&nlsf_q15[..order]);
        // After a loss, bandwidth expansion.
        if self.loss_cnt != 0 {
            bwexpander(&mut ctrl.pred_coef_q12[0][..order], BWE_AFTER_LOSS_Q16);
            bwexpander(&mut ctrl.pred_coef_q12[1][..order], BWE_AFTER_LOSS_Q16);
        }
        if i32::from(self.indices.signal_type) == TYPE_VOICED {
            decode_pitch(
                self.indices.lag_index,
                self.indices.contour_index,
                &mut ctrl.pitch_l,
                self.fs_khz,
                self.nb_subfr,
            );
            let cbk = LTP_VQ_PTRS_Q7[self.indices.per_index as usize];
            for k in 0..self.nb_subfr {
                let row = &cbk[self.indices.ltp_index[k] as usize];
                for i in 0..LTP_ORDER {
                    ctrl.ltp_coef_q14[k * LTP_ORDER + i] = lshift(i32::from(row[i]), 7) as i16;
                }
            }
            ctrl.ltp_scale_q14 =
                i32::from(LTPSCALES_TABLE_Q14[self.indices.ltp_scale_index as usize]);
        } else {
            ctrl.pitch_l[..self.nb_subfr].fill(0);
            ctrl.ltp_coef_q14[..LTP_ORDER * self.nb_subfr].fill(0);
            self.indices.per_index = 0;
            ctrl.ltp_scale_q14 = 0;
        }
    }

    /// `silk_decode_core`: the excitation through the long-term and
    /// short-term synthesis filters, into `xq`.
    fn decode_core(&mut self, ctrl: &mut DecoderControl, xq: &mut [i16], pulses: &[i16]) {
        let ltp_mem = self.ltp_mem_length;
        let fl = self.frame_length;
        let subfr = self.subfr_length;
        let order = self.lpc_order;
        let mut s_ltp = vec![0i16; ltp_mem];
        let mut s_ltp_q15 = vec![0i32; ltp_mem + fl];
        let mut res_q14 = vec![0i32; subfr];
        let mut s_lpc_q14 = vec![0i32; subfr + MAX_LPC_ORDER];
        let offset_q10 = i32::from(
            QUANTIZATION_OFFSETS_Q10[self.indices.signal_type as usize >> 1]
                [self.indices.quant_offset_type as usize],
        );
        let nlsf_interpolation_flag = self.indices.nlsf_interp_coef_q2 < 4;

        // The excitation: pulses, offset, and a pseudo-random sign.
        let mut rand_seed = i32::from(self.indices.seed);
        for i in 0..fl {
            rand_seed = rand(rand_seed);
            let mut e = lshift(i32::from(pulses[i]), 14);
            if e > 0 {
                e -= QUANT_LEVEL_ADJUST_Q10 << 4;
            } else if e < 0 {
                e += QUANT_LEVEL_ADJUST_Q10 << 4;
            }
            e += offset_q10 << 4;
            if rand_seed < 0 {
                e = -e;
            }
            self.exc_q14[i] = e;
            rand_seed = rand_seed.wrapping_add(i32::from(pulses[i]));
        }

        s_lpc_q14[..MAX_LPC_ORDER].copy_from_slice(&self.s_lpc_q14_buf);
        let mut s_ltp_buf_idx = ltp_mem;
        let mut lag = 0i32;
        for k in 0..self.nb_subfr {
            let a_q12 = ctrl.pred_coef_q12[k >> 1];
            let b_at = k * LTP_ORDER;
            let mut signal_type = i32::from(self.indices.signal_type);
            let gain_q10 = ctrl.gains_q16[k] >> 6;
            let mut inv_gain_q31 = inverse32_varq(ctrl.gains_q16[k], 47);
            // The gain's change scales the short-term state.
            let gain_adj_q16 = if ctrl.gains_q16[k] != self.prev_gain_q16 {
                let adj = div32_varq(self.prev_gain_q16, ctrl.gains_q16[k], 16);
                for v in &mut s_lpc_q14[..MAX_LPC_ORDER] {
                    *v = smulww(adj, *v);
                }
                adj
            } else {
                1 << 16
            };
            self.prev_gain_q16 = ctrl.gains_q16[k];
            // No abrupt change from voiced concealment to unvoiced decoding.
            if self.loss_cnt != 0
                && self.prev_signal_type == TYPE_VOICED
                && i32::from(self.indices.signal_type) != TYPE_VOICED
                && k < MAX_NB_SUBFR / 2
            {
                ctrl.ltp_coef_q14[b_at..b_at + LTP_ORDER].fill(0);
                ctrl.ltp_coef_q14[b_at + LTP_ORDER / 2] = fix_const(0.25, 14) as i16;
                signal_type = TYPE_VOICED;
                ctrl.pitch_l[k] = self.lag_prev;
            }
            if signal_type == TYPE_VOICED {
                lag = ctrl.pitch_l[k];
                let lag_n = usize::try_from(lag).unwrap_or(0);
                // Re-whitened with the new filter.
                if k == 0 || (k == 2 && nlsf_interpolation_flag) {
                    let start_idx = ltp_mem.saturating_sub(lag_n + order + LTP_ORDER / 2).max(1);
                    if k == 2 {
                        self.out_buf[ltp_mem..ltp_mem + 2 * subfr]
                            .copy_from_slice(&xq[..2 * subfr]);
                    }
                    lpc_analysis_filter(
                        &mut s_ltp[start_idx..],
                        &self.out_buf[start_idx + k * subfr..],
                        &a_q12,
                        ltp_mem - start_idx,
                        order,
                    );
                    // LTP down-scaling, against dependence between packets.
                    if k == 0 {
                        inv_gain_q31 = lshift(smulwb(inv_gain_q31, ctrl.ltp_scale_q14), 2);
                    }
                    for i in 0..lag_n + LTP_ORDER / 2 {
                        s_ltp_q15[s_ltp_buf_idx - i - 1] =
                            smulwb(inv_gain_q31, i32::from(s_ltp[ltp_mem - i - 1]));
                    }
                } else if gain_adj_q16 != 1 << 16 {
                    // The LTP state follows the gain.
                    for i in 0..lag_n + LTP_ORDER / 2 {
                        let at = s_ltp_buf_idx - i - 1;
                        s_ltp_q15[at] = smulww(gain_adj_q16, s_ltp_q15[at]);
                    }
                }
            }
            let pexc = k * subfr;
            if signal_type == TYPE_VOICED {
                // Long-term prediction (rounded up: SMLAWB rounds down).
                let b = &ctrl.ltp_coef_q14[b_at..b_at + LTP_ORDER];
                let first_lag = s_ltp_buf_idx + LTP_ORDER / 2 - usize::try_from(lag).unwrap_or(0);
                for i in 0..subfr {
                    let pred_lag = first_lag + i;
                    let mut ltp_pred_q13 = 2;
                    for (j, &bj) in b.iter().enumerate() {
                        ltp_pred_q13 = smlawb(ltp_pred_q13, s_ltp_q15[pred_lag - j], i32::from(bj));
                    }
                    res_q14[i] = self.exc_q14[pexc + i].wrapping_add(lshift(ltp_pred_q13, 1));
                    s_ltp_q15[s_ltp_buf_idx] = lshift(res_q14[i], 1);
                    s_ltp_buf_idx += 1;
                }
            } else {
                res_q14.copy_from_slice(&self.exc_q14[pexc..pexc + subfr]);
            }
            for i in 0..subfr {
                // Short-term prediction (rounded as above).
                let mut lpc_pred_q10 = (order >> 1) as i32;
                for j in 0..order {
                    lpc_pred_q10 = smlawb(
                        lpc_pred_q10,
                        s_lpc_q14[MAX_LPC_ORDER + i - j - 1],
                        i32::from(a_q12[j]),
                    );
                }
                s_lpc_q14[MAX_LPC_ORDER + i] = add_sat32(res_q14[i], lshift_sat32(lpc_pred_q10, 4));
                xq[pexc + i] = sat16(rshift_round(
                    smulww(s_lpc_q14[MAX_LPC_ORDER + i], gain_q10),
                    8,
                )) as i16;
            }
            s_lpc_q14.copy_within(subfr..subfr + MAX_LPC_ORDER, 0);
        }
        self.s_lpc_q14_buf
            .copy_from_slice(&s_lpc_q14[..MAX_LPC_ORDER]);
    }
}

/// `silk_gains_dequant`: each subframe's gain (Q16) from its index -- the
/// first absolute (but not more than 16 steps down) or a delta, the deltas
/// above a threshold counting double.
fn gains_dequant(
    gain_q16: &mut [i32; MAX_NB_SUBFR],
    ind: &[i8; MAX_NB_SUBFR],
    prev_ind: &mut i8,
    conditional: bool,
    nb_subfr: usize,
) {
    for k in 0..nb_subfr {
        if k == 0 && !conditional {
            *prev_ind = i32::from(ind[k]).max(i32::from(*prev_ind) - 16) as i8;
        } else {
            let ind_tmp = i32::from(ind[k]) + MIN_DELTA_GAIN_QUANT;
            let threshold = 2 * MAX_DELTA_GAIN_QUANT - N_LEVELS_QGAIN + i32::from(*prev_ind);
            let step = if ind_tmp > threshold {
                lshift(ind_tmp, 1) - threshold
            } else {
                ind_tmp
            };
            // An `opus_int8` +=: wraps before the limit below.
            *prev_ind = (i32::from(*prev_ind) + step) as i8;
        }
        *prev_ind = limit(i32::from(*prev_ind), 0, N_LEVELS_QGAIN - 1) as i8;
        gain_q16[k] =
            log2lin((smulwb(INV_SCALE_Q16, i32::from(*prev_ind)) + GAIN_OFFSET).min(3967));
    }
}

/// `silk_decode_pitch`: each subframe's pitch lag from the frame's lag and
/// its contour.
fn decode_pitch(
    lag_index: i16,
    contour_index: i8,
    pitch_lags: &mut [i32; MAX_NB_SUBFR],
    fs_khz: i32,
    nb_subfr: usize,
) {
    let c = contour_index as usize;
    let at = |k: usize| -> i32 {
        let v = match (fs_khz == 8, nb_subfr == MAX_NB_SUBFR) {
            (true, true) => CB_LAGS_STAGE2.get(k).and_then(|r| r.get(c)),
            (true, false) => CB_LAGS_STAGE2_10_MS.get(k).and_then(|r| r.get(c)),
            (false, true) => CB_LAGS_STAGE3.get(k).and_then(|r| r.get(c)),
            (false, false) => CB_LAGS_STAGE3_10_MS.get(k).and_then(|r| r.get(c)),
        };
        v.map_or(0, |&v| i32::from(v))
    };
    let min_lag = smulbb(PE_MIN_LAG_MS, fs_khz);
    let max_lag = smulbb(PE_MAX_LAG_MS, fs_khz);
    let lag = min_lag + i32::from(lag_index);
    for (k, p) in pitch_lags.iter_mut().enumerate().take(nb_subfr) {
        *p = limit(lag + at(k), min_lag, max_lag);
    }
}

/// `silk_decode_pulses`: a frame's excitation pulses -- a rate level, each
/// 16-sample block's pulse count (with extra low bits for large ones), the
/// shell code splitting each count down to its samples, then the signs.
pub(crate) fn decode_pulses(
    dec: &mut RangeDecoder<'_>,
    pulses: &mut [i16],
    signal_type: i32,
    quant_offset_type: i32,
    frame_length: usize,
) {
    let rate_level = dec.icdf(&RATE_LEVELS_ICDF[(signal_type >> 1) as usize], 8);
    let mut iter = frame_length / SHELL_LEN;
    if iter * SHELL_LEN < frame_length {
        // Only 10 ms at 12 kHz: 120 samples.
        iter += 1;
    }
    let mut sum_pulses = [0i32; MAX_FRAME_LENGTH / SHELL_LEN];
    let mut n_lshifts = [0i32; MAX_FRAME_LENGTH / SHELL_LEN];
    for i in 0..iter {
        sum_pulses[i] = dec.icdf(&PULSES_PER_BLOCK_ICDF[rate_level], 8) as i32;
        // More low bits: after ten, the table shifted to forbid another.
        while sum_pulses[i] == SILK_MAX_PULSES + 1 {
            n_lshifts[i] += 1;
            let from = usize::from(n_lshifts[i] == 10);
            sum_pulses[i] = dec.icdf(&PULSES_PER_BLOCK_ICDF[N_RATE_LEVELS - 1][from..], 8) as i32;
        }
    }
    for i in 0..iter {
        let block = &mut pulses[i * SHELL_LEN..(i + 1) * SHELL_LEN];
        if sum_pulses[i] > 0 {
            shell_decoder(block, dec, sum_pulses[i]);
        } else {
            block.fill(0);
        }
    }
    for i in 0..iter {
        let nls = n_lshifts[i];
        if nls > 0 {
            for p in &mut pulses[i * SHELL_LEN..(i + 1) * SHELL_LEN] {
                let mut abs_q = i32::from(*p);
                for _ in 0..nls {
                    abs_q = lshift(abs_q, 1) + dec.icdf(&LSB_ICDF, 8) as i32;
                }
                *p = abs_q as i16;
            }
            // Marked non-zero, for the signs.
            sum_pulses[i] |= nls << 5;
        }
    }
    decode_signs(
        dec,
        pulses,
        frame_length,
        signal_type,
        quant_offset_type,
        &sum_pulses,
    );
}

/// `decode_split`: a count of pulses split between two halves.
fn decode_split(dec: &mut RangeDecoder<'_>, p: i32, table: &[u8]) -> (i16, i16) {
    if p > 0 {
        let at = usize::from(SHELL_CODE_TABLE_OFFSETS[p as usize]);
        let c1 = dec.icdf(&table[at..], 8) as i32;
        (c1 as i16, (p - c1) as i16)
    } else {
        (0, 0)
    }
}

/// `silk_shell_decoder`: 16 samples' pulses from their total, halved four
/// times, in libopus's order.
fn shell_decoder(p0: &mut [i16], dec: &mut RangeDecoder<'_>, pulses4: i32) {
    let (t0, t1, t2, t3) = (
        &SHELL_CODE_TABLE0,
        &SHELL_CODE_TABLE1,
        &SHELL_CODE_TABLE2,
        &SHELL_CODE_TABLE3,
    );
    let mut p3 = [0i16; 2];
    let mut p2 = [0i16; 4];
    let mut p1 = [0i16; 8];
    let split = |dec: &mut RangeDecoder<'_>, p: i16, t: &[u8]| decode_split(dec, i32::from(p), t);
    (p3[0], p3[1]) = split(dec, pulses4 as i16, t3);
    (p2[0], p2[1]) = split(dec, p3[0], t2);
    (p1[0], p1[1]) = split(dec, p2[0], t1);
    (p0[0], p0[1]) = split(dec, p1[0], t0);
    (p0[2], p0[3]) = split(dec, p1[1], t0);
    (p1[2], p1[3]) = split(dec, p2[1], t1);
    (p0[4], p0[5]) = split(dec, p1[2], t0);
    (p0[6], p0[7]) = split(dec, p1[3], t0);
    (p2[2], p2[3]) = split(dec, p3[1], t2);
    (p1[4], p1[5]) = split(dec, p2[2], t1);
    (p0[8], p0[9]) = split(dec, p1[4], t0);
    (p0[10], p0[11]) = split(dec, p1[5], t0);
    (p1[6], p1[7]) = split(dec, p2[3], t1);
    (p0[12], p0[13]) = split(dec, p1[6], t0);
    (p0[14], p0[15]) = split(dec, p1[7], t0);
}

/// `silk_decode_signs`: each non-zero pulse's sign, by a table chosen from
/// the signal type, quantiser offset and the block's pulse count.
fn decode_signs(
    dec: &mut RangeDecoder<'_>,
    pulses: &mut [i16],
    length: usize,
    signal_type: i32,
    quant_offset_type: i32,
    sum_pulses: &[i32],
) {
    let i = smulbb(7, quant_offset_type + (signal_type << 1)) as usize;
    let icdf_row = &SIGN_ICDF[i..];
    let blocks = (length + SHELL_LEN / 2) >> 4;
    for (b, &p) in sum_pulses.iter().enumerate().take(blocks) {
        if p > 0 {
            let icdf = [icdf_row[(p & 0x1F).min(6) as usize], 0];
            for q in &mut pulses[b * SHELL_LEN..(b + 1) * SHELL_LEN] {
                if *q > 0 {
                    // silk_dec_map: 0 -> -1, 1 -> +1.
                    let sign = lshift(dec.icdf(&icdf, 8) as i32, 1) - 1;
                    *q = (i32::from(*q) * sign) as i16;
                }
            }
        }
    }
}

/// `silk_LPC_analysis_filter`: `input` through the MA filter `b` (Q12,
/// order `d`) into `out`, `len` samples, the first `d` set to 0.
pub(crate) fn lpc_analysis_filter(out: &mut [i16], input: &[i16], b: &[i16], len: usize, d: usize) {
    for ix in d..len {
        let mut out32_q12 = smulbb(i32::from(input[ix - 1]), i32::from(b[0]));
        for j in 1..d {
            // Wrapping, so that two wraps (from invalid streams only)
            // cancel.
            out32_q12 =
                out32_q12.wrapping_add(smulbb(i32::from(input[ix - 1 - j]), i32::from(b[j])));
        }
        let out32_q12 = lshift(i32::from(input[ix]), 12).wrapping_sub(out32_q12);
        out[ix] = sat16(rshift_round(out32_q12, 12)) as i16;
    }
    out[..d].fill(0);
}
