//! SILK (RFC 6716 §4.2): Opus's linear-prediction layer, for speech and for
//! the low band of a hybrid frame. A packet holds one to three frames of 10
//! or 20 ms a channel, coded as voice-activity and redundancy flags, side
//! information (gains, line spectral frequencies, pitch) and excitation
//! pulses, decoded at 8, 12 or 16 kHz and resampled to the output rate;
//! stereo as mid and side.
//!
//! This is libopus 1.5.2's fixed-point decoder, translated: `silk/dec_API.c`
//! here, the rest in the submodules (without deep PLC or OSCE), copyright
//! Skype Limited, Xiph.Org and the contributors named in its `COPYING`, used
//! under libopus's BSD licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "channel indices below the internal channel count (1 or 2), frame indices below the frames a packet holds (at most 3), and buffers of a frame and two samples of history -- as libopus's"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sample counts below 960 a channel"
)]

mod channel;
pub(crate) mod codebook;
mod fix;
mod nlsf;
mod plc;
mod resampler;
mod stereo;
mod tables;

use crate::entdec::Decoder as RangeDecoder;
use channel::{
    CODE_CONDITIONALLY, CODE_INDEPENDENTLY, CODE_INDEPENDENTLY_NO_LTP_SCALING, ChannelState,
    MAX_FRAME_LENGTH, MAX_FRAMES_PER_PACKET, TYPE_NO_VOICE_ACTIVITY, TYPE_VOICED,
};
use codebook::LBRR_FLAGS_ICDF_PTR;
use stereo::StereoState;

/// How a SILK frame is to be decoded (`lostFlag`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LostFlag {
    /// `FLAG_DECODE_NORMAL`.
    Normal,
    /// `FLAG_PACKET_LOST`: concealed.
    Lost,
    /// `FLAG_DECODE_LBRR`: the low-bitrate redundancy (FEC).
    Lbrr,
}

/// `silk_DecControlStruct`: what the Opus layer tells SILK, and SILK tells
/// it back.
#[derive(Clone, Debug)]
pub(crate) struct DecControl {
    /// The channels decoded into.
    pub channels_api: usize,
    /// The channels the packet codes.
    pub channels_internal: usize,
    /// The output rate.
    pub api_sample_rate: i32,
    /// SILK's own rate: 8, 12 or 16 kHz.
    pub internal_sample_rate: i32,
    /// The packet's length: 10, 20, 40 or 60 ms.
    pub payload_size_ms: i32,
    /// The last frame's pitch lag, at 48 kHz (0 unvoiced).
    pub prev_pitch_lag: i32,
}

/// The SILK decoder (`silk_decoder`).
pub(crate) struct SilkDecoder {
    channels: [ChannelState; 2],
    stereo: StereoState,
    n_channels_api: usize,
    n_channels_internal: usize,
    prev_decode_only_middle: bool,
}

impl SilkDecoder {
    /// `silk_InitDecoder`.
    pub(crate) fn new() -> Self {
        Self {
            channels: [ChannelState::new(), ChannelState::new()],
            stereo: StereoState::default(),
            n_channels_api: 0,
            n_channels_internal: 0,
            prev_decode_only_middle: false,
        }
    }

    /// `silk_ResetDecoder`: the channels and the stereo state cleared (the
    /// channel counts kept, as libopus keeps them).
    pub(crate) fn reset(&mut self) {
        for ch in &mut self.channels {
            ch.reset();
        }
        self.stereo = StereoState::default();
        self.prev_decode_only_middle = false;
    }

    /// `silk_Decode`: one frame of a packet into `out` (interleaved,
    /// `channels_api` channels at the output rate); the samples a channel
    /// written. `new_packet` on the packet's first frame. An error where
    /// libopus's would return one: a packet length or rate SILK has none
    /// of.
    pub(crate) fn decode(
        &mut self,
        ctl: &mut DecControl,
        lost: LostFlag,
        new_packet: bool,
        dec: &mut RangeDecoder<'_>,
        out: &mut [i16],
    ) -> Result<usize, ()> {
        let n_int = ctl.channels_internal.clamp(1, 2);
        let api = ctl.channels_api.clamp(1, 2);
        let mut decode_only_middle = false;
        if new_packet {
            for ch in &mut self.channels[..n_int] {
                ch.n_frames_decoded = 0;
            }
        }
        // A switch from mono to stereo in the stream: the second channel
        // starts afresh.
        if n_int > self.n_channels_internal {
            self.channels[1] = ChannelState::new();
        }
        let stereo_to_mono = n_int == 1
            && self.n_channels_internal == 2
            && ctl.internal_sample_rate == 1000 * self.channels[0].fs_khz;
        if self.channels[0].n_frames_decoded == 0 {
            for ch in &mut self.channels[..n_int] {
                let (frames, nb_subfr) = match ctl.payload_size_ms {
                    // No length (a loss): 10 ms.
                    0 | 10 => (1, 2),
                    20 => (1, 4),
                    40 => (2, 4),
                    60 => (3, 4),
                    _ => return Err(()),
                };
                ch.n_frames_per_packet = frames;
                ch.nb_subfr = nb_subfr;
                let fs_khz = (ctl.internal_sample_rate >> 10) + 1;
                if !matches!(fs_khz, 8 | 12 | 16) {
                    return Err(());
                }
                ch.set_fs(fs_khz, ctl.api_sample_rate)?;
            }
        }
        if api == 2 && n_int == 2 && (self.n_channels_api == 1 || self.n_channels_internal == 1) {
            self.stereo.pred_prev_q13 = [0; 2];
            self.stereo.s_side = [0; 2];
            self.channels[1].resampler = self.channels[0].resampler.clone();
        }
        self.n_channels_api = api;
        self.n_channels_internal = n_int;
        if !(8000..=48000).contains(&ctl.api_sample_rate) {
            return Err(());
        }

        let mut ms_pred_q13 = [0i32; 2];
        if lost != LostFlag::Lost && self.channels[0].n_frames_decoded == 0 {
            // The packet's first frame: the voice-activity and redundancy
            // flags.
            for ch in &mut self.channels[..n_int] {
                for i in 0..ch.n_frames_per_packet {
                    ch.vad_flags[i] = dec.bit_logp(1);
                }
                ch.lbrr_flag = dec.bit_logp(1);
            }
            for ch in &mut self.channels[..n_int] {
                ch.lbrr_flags = [false; MAX_FRAMES_PER_PACKET];
                if ch.lbrr_flag {
                    if ch.n_frames_per_packet == 1 {
                        ch.lbrr_flags[0] = true;
                    } else {
                        let table = LBRR_FLAGS_ICDF_PTR[ch.n_frames_per_packet - 2];
                        let symbol = dec.icdf(table, 8) + 1;
                        for i in 0..ch.n_frames_per_packet {
                            ch.lbrr_flags[i] = (symbol >> i) & 1 != 0;
                        }
                    }
                }
            }
            if lost == LostFlag::Normal {
                // Decoding normally: the redundancy read and passed over.
                for i in 0..self.channels[0].n_frames_per_packet {
                    for n in 0..n_int {
                        if !self.channels[n].lbrr_flags[i] {
                            continue;
                        }
                        if n_int == 2 && n == 0 {
                            ms_pred_q13 = stereo::decode_pred(dec);
                            if !self.channels[1].lbrr_flags[i] {
                                decode_only_middle = stereo::decode_mid_only(dec);
                            }
                        }
                        // Coded against the frame before, where it has one.
                        let cond = if i > 0 && self.channels[n].lbrr_flags[i - 1] {
                            CODE_CONDITIONALLY
                        } else {
                            CODE_INDEPENDENTLY
                        };
                        let ch = &mut self.channels[n];
                        ch.decode_indices(dec, i, true, cond);
                        let mut pulses = [0i16; MAX_FRAME_LENGTH];
                        channel::decode_pulses(
                            dec,
                            &mut pulses,
                            i32::from(ch.indices.signal_type),
                            i32::from(ch.indices.quant_offset_type),
                            ch.frame_length,
                        );
                    }
                }
            }
        }
        let fd = self.channels[0].n_frames_decoded;
        let flag = |flags: &[bool; MAX_FRAMES_PER_PACKET], i: usize| {
            flags.get(i).copied().unwrap_or(false)
        };
        // The mid/side predictors.
        if n_int == 2 {
            if lost == LostFlag::Normal
                || (lost == LostFlag::Lbrr && flag(&self.channels[0].lbrr_flags, fd))
            {
                ms_pred_q13 = stereo::decode_pred(dec);
                // For redundancy, the mid-only flag only where the side has
                // no redundancy.
                decode_only_middle = if (lost == LostFlag::Normal
                    && !flag(&self.channels[1].vad_flags, fd))
                    || (lost == LostFlag::Lbrr && !flag(&self.channels[1].lbrr_flags, fd))
                {
                    stereo::decode_mid_only(dec)
                } else {
                    false
                };
            } else {
                ms_pred_q13 = self.stereo.pred_prev_q13.map(i32::from);
            }
        }
        // The side's prediction memory reset for its first coded frame.
        if n_int == 2 && !decode_only_middle && self.prev_decode_only_middle {
            let side = &mut self.channels[1];
            side.out_buf.fill(0);
            side.s_lpc_q14_buf.fill(0);
            side.lag_prev = 100;
            side.last_gain_index = 10;
            side.prev_signal_type = TYPE_NO_VOICE_ACTIVITY;
            side.first_frame_after_reset = true;
        }

        // Each channel's frame, after two samples of history.
        let mut tmp = [[0i16; MAX_FRAME_LENGTH + 2]; 2];
        let has_side = if lost == LostFlag::Normal {
            !decode_only_middle
        } else {
            !self.prev_decode_only_middle
                || (n_int == 2
                    && lost == LostFlag::Lbrr
                    && flag(
                        &self.channels[1].lbrr_flags,
                        self.channels[1].n_frames_decoded,
                    ))
        };
        let mut n_samples_out_dec = 0;
        for n in 0..n_int {
            if n == 0 || has_side {
                // Channel 0's count, already advanced for the mid when the
                // side is decoded: the same frame.
                let frame_index = self.channels[0].n_frames_decoded as isize - n as isize;
                let cond = if frame_index <= 0 {
                    CODE_INDEPENDENTLY
                } else if lost == LostFlag::Lbrr {
                    if flag(&self.channels[n].lbrr_flags, frame_index as usize - 1) {
                        CODE_CONDITIONALLY
                    } else {
                        CODE_INDEPENDENTLY
                    }
                } else if n > 0 && self.prev_decode_only_middle {
                    // A side frame skipped in this packet: no LTP scaling
                    // needed, the LTP state being well defined.
                    CODE_INDEPENDENTLY_NO_LTP_SCALING
                } else {
                    CODE_CONDITIONALLY
                };
                n_samples_out_dec =
                    self.channels[n].decode_frame(dec, &mut tmp[n][2..], lost, cond);
            } else {
                tmp[n][2..2 + n_samples_out_dec].fill(0);
            }
            self.channels[n].n_frames_decoded += 1;
        }

        if api == 2 && n_int == 2 {
            let [mid, side] = &mut tmp;
            stereo::ms_to_lr(
                &mut self.stereo,
                mid,
                side,
                ms_pred_q13,
                self.channels[0].fs_khz,
                n_samples_out_dec,
            );
        } else {
            // The mid's two samples of history kept.
            tmp[0][..2].copy_from_slice(&self.stereo.s_mid);
            self.stereo
                .s_mid
                .copy_from_slice(&tmp[0][n_samples_out_dec..n_samples_out_dec + 2]);
        }

        let fs_khz = self.channels[0].fs_khz;
        let n_samples_out = usize::try_from(
            (n_samples_out_dec as i64 * i64::from(ctl.api_sample_rate)) / i64::from(fs_khz * 1000),
        )
        .map_err(|_| ())?;
        if out.len() < n_samples_out * api {
            return Err(());
        }
        // Resampled to the output rate (from one sample into the history,
        // as libopus aligns SILK with CELT), and interleaved.
        // A SILK frame is 20 ms at most: 960 samples at 48 kHz.
        let mut resampled = [0i16; 960];
        let resampled = &mut resampled[..n_samples_out];
        for n in 0..api.min(n_int) {
            let input = &tmp[n][1..=n_samples_out_dec];
            if api == 2 {
                self.channels[n].resampler.process(resampled, input);
                for (i, &s) in resampled.iter().enumerate() {
                    out[n + 2 * i] = s;
                }
            } else {
                self.channels[n]
                    .resampler
                    .process(&mut out[..n_samples_out], input);
            }
        }
        // Two channels from a mono stream.
        if api == 2 && n_int == 1 {
            if stereo_to_mono {
                // The right channel resampled too, for a stream just
                // collapsed from stereo.
                self.channels[1]
                    .resampler
                    .process(resampled, &tmp[0][1..=n_samples_out_dec]);
                for (i, &s) in resampled.iter().enumerate() {
                    out[1 + 2 * i] = s;
                }
            } else {
                for i in 0..n_samples_out {
                    out[1 + 2 * i] = out[2 * i];
                }
            }
        }
        // The pitch lag at 48 kHz.
        ctl.prev_pitch_lag = if self.channels[0].prev_signal_type == TYPE_VOICED {
            const MULT: [i32; 3] = [6, 4, 3];
            self.channels[0].lag_prev * MULT[((fs_khz - 8) >> 2) as usize]
        } else {
            0
        };
        if lost == LostFlag::Lost {
            // No gain clamping after a loss, so that a falling energy does
            // not bounce back.
            for ch in &mut self.channels[..self.n_channels_internal] {
                ch.last_gain_index = 10;
            }
        } else {
            self.prev_decode_only_middle = decode_only_middle;
        }
        Ok(n_samples_out)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "a test: a failure should be loud, and its casts are the C program's"
)]
mod reference {
    //! SILK's functions held to libopus's own on inputs no stream makes often
    //! enough -- LSFs crowded together and at the ends, filters of any shape,
    //! every magnitude -- their results digested and compared with what
    //! `tools/functions.c` digested libopus 1.5.2's fixed-point build's to.

    use super::codebook::{NLSF_CB_NB_MB, NLSF_CB_WB};
    use super::fix::{div32_varq, inverse32_varq, log2lin, sqrt_approx};
    use super::nlsf::{MAX_LPC_ORDER, lpc_inverse_pred_gain, nlsf2a, stabilize};
    use super::plc::sum_sqr_shift;
    use crate::testutil::{Digest, Xorshift};

    /// `silk_NLSF_stabilize` and `silk_NLSF2A` (and through it
    /// `silk_LPC_fit` and the stability test's bandwidth expansion, to their
    /// last resorts), and `silk_LPC_inverse_pred_gain` on filters stable or
    /// not.
    #[test]
    fn nlsf_agrees_with_libopus() {
        let mut rng = Xorshift(0x8CB9_2BA7_2F3D_8DD7);
        let mut d = Digest::new();
        for _ in 0..100_000 {
            let order = if rng.next() & 1 != 0 { 16 } else { 10 };
            let kind = rng.below(4);
            let mut nlsf = [0i16; MAX_LPC_ORDER];
            match kind {
                0 | 3 => {
                    for v in &mut nlsf[..order] {
                        *v = rng.below(32768) as i16;
                    }
                }
                1 => {
                    // Pairs crowded together: peaky spectra.
                    for k in (0..order).step_by(2) {
                        let base = rng.below(32700) as i32;
                        let gap = rng.below(64) as i32;
                        nlsf[k] = base as i16;
                        nlsf[k + 1] = (base + gap) as i16;
                    }
                }
                _ => {
                    // Crowded at the ends.
                    for v in &mut nlsf[..order] {
                        let x = rng.below(600) as i32;
                        let low = rng.next() & 1 != 0;
                        *v = (if low { x } else { 32767 - x }) as i16;
                    }
                }
            }
            let mut a = [0i16; MAX_LPC_ORDER];
            if kind != 3 {
                // Sorted, as a codebook's NLSFs are.
                nlsf[..order].sort_unstable();
                nlsf2a(&mut a, &nlsf[..order], order);
                for &v in &a[..order] {
                    d.add(i32::from(v));
                }
            }
            let mut stable = nlsf;
            let delta_min = if order == 16 {
                NLSF_CB_WB.delta_min_q15
            } else {
                NLSF_CB_NB_MB.delta_min_q15
            };
            stabilize(&mut stable, delta_min, order);
            for &v in &stable[..order] {
                d.add(i32::from(v));
            }
            a = [0; MAX_LPC_ORDER];
            nlsf2a(&mut a, &stable[..order], order);
            for &v in &a[..order] {
                d.add(i32::from(v));
            }
            let shift = rng.below(16);
            for v in &mut a[..order] {
                *v = rng.sample(shift) as i16;
            }
            d.add(lpc_inverse_pred_gain(&a[..order]));
        }
        d.check("silk_nlsf", 3_675_552, 0x8b25_8861_5090_a2f2)
            .unwrap();
    }

    /// `silk_log2lin` over its whole domain and past it, `silk_SQRT_APPROX`,
    /// `silk_DIV32_varQ` and `silk_INVERSE32_varQ` at every magnitude and
    /// precision, and `silk_sum_sqr_shift` on vectors of every length, odd
    /// ones too.
    #[test]
    fn silk_math_agrees_with_libopus() {
        let mut rng = Xorshift(0xF135_7AEA_2E62_A9C5);
        let mut d = Digest::new();
        for x in -10..4000 {
            d.add(log2lin(x));
        }
        for _ in 0..100_000 {
            let bits = rng.next();
            let shift = rng.below(32);
            d.add(sqrt_approx(bits as i32 >> shift));
        }
        for _ in 0..100_000 {
            let a_bits = rng.next();
            let a_shift = rng.below(32);
            let b_bits = rng.next();
            let b_shift = rng.below(32);
            let q = rng.below(31) as i32;
            let a = (a_bits | 1) as i32 >> a_shift;
            let b = (b_bits | 1) as i32 >> b_shift;
            if a == i32::MIN || b == i32::MIN || b == 0 {
                continue;
            }
            d.add(div32_varq(a, b, q));
            d.add(inverse32_varq(b, 1 + q));
        }
        let mut v = [0i16; 400];
        for _ in 0..20000 {
            let len = 1 + rng.below(400) as usize;
            let shift = rng.below(16);
            for x in &mut v[..len] {
                *x = rng.sample(shift) as i16;
            }
            let (energy, shift) = sum_sqr_shift(&v[..len]);
            d.add(energy);
            d.add(shift);
        }
        d.check("silk_math", 337_834, 0x5eab_526c_f22e_b3c5)
            .unwrap();
    }
}
