//! The CELT decoder (RFC 6716 §4.3): one frame's header, energies, bit
//! allocation and band shapes decoded, turned to samples by the inverse
//! MDCT, put through the pitch post-filter and de-emphasised -- or, for a
//! frame that is missing, concealed: the last pitch period repeated through
//! an LPC filter while the loss is short, noise at the last band energies
//! once it is long.
//!
//! Translated into Rust from libopus 1.5.2's `celt/celt_decoder.c` and
//! `celt/celt.c` (`comb_filter`, `init_caps`, `tf_select_table`), built
//! `FIXED_POINT` without deep PLC, copyright Xiph.Org, Jean-Marc Valin,
//! Gregory Maxwell and the contributors named in its `COPYING`, used under
//! libopus's BSD licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "every index is within the decoder's buffers at libopus's sizes: DECODE_BUFFER_SIZE + overlap samples a channel, frames of 120 << LM samples, the bands' per-channel tables of NB_EBANDS, and history the pitch periods (at most 1024) reach back into, which the buffer's 2048 - N samples before the frame always hold"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sizes below 2200, band counts and LM small; bit counts within a frame's 10,200 bits in eighths; the sample arithmetic wraps where C's would overflow, through the fixed-point helpers"
)]

use super::bands::{
    AllBands, NORM_SIZE, anti_collapse, denormalise_bands, lcg_rand, quant_all_bands,
};
use super::energy::{unquant_coarse, unquant_finalise, unquant_fine};
use super::kiss_fft::Cpx;
use super::lpc::{LPC_ORDER, autocorr, fir, iir_in_place, lpc, maxabs16};
use super::mathops::{frac_div32, sqrt, zlog2};
use super::mdct::backward;
use super::mode::{
    EFF_EBANDS, MAX_LM, NB_EBANDS, OVERLAP, PREEMPH, SHORT_MDCT_SIZE, cache_cap, ebands, window,
};
use super::pitch::{pitch_downsample, pitch_search};
use super::rate::compute_allocation;
use super::vq::renormalise_vector;
use crate::entdec::{BITRES, Decoder};
use crate::error::Error;
use crate::fixed::{
    DB_SHIFT, Q15ONE, SIG_SAT, SIG_SHIFT, extract16, half32, mult16_16, mult16_16_p15,
    mult16_16_q15, mult16_32_q15, qconst16, saturate, saturate16, shl32, sig2word16, sround16,
};

/// Samples of history a channel keeps (`DECODE_BUFFER_SIZE`).
pub(crate) const DECODE_BUFFER_SIZE: usize = 2048;
/// `buf`, taken out of its owner, at least `len` long (zero-filled where it
/// grows; what it held is kept).
fn taken(buf: &mut Vec<i32>, len: usize) -> Vec<i32> {
    let mut v = std::mem::take(buf);
    if v.len() < len {
        v.resize(len, 0);
    }
    v
}

/// A frame's samples, a channel, at most: 20 ms at 48 kHz.
const MAX_FRAME: usize = SHORT_MDCT_SIZE << MAX_LM;
/// The longest pitch period (`MAX_PERIOD`).
const MAX_PERIOD: usize = 1024;
/// The concealment's longest and shortest pitch lag (66.67 Hz, 480 Hz).
const PLC_PITCH_LAG_MAX: usize = 720;
const PLC_PITCH_LAG_MIN: usize = 100;
/// The post-filter's shortest period (`COMBFILTER_MINPERIOD`).
const COMBFILTER_MINPERIOD: i32 = 15;
/// `SPREAD_NORMAL`.
const SPREAD_NORMAL: i32 = 2;
/// The largest packet (`len > 1275` is refused).
const MAX_PACKET: usize = 1275;

/// `trim_icdf`, `spread_icdf`, `tapset_icdf`.
const TRIM_ICDF: [u8; 11] = [126, 124, 119, 109, 87, 41, 19, 9, 4, 2, 0];
const SPREAD_ICDF: [u8; 4] = [25, 23, 2, 0];
const TAPSET_ICDF: [u8; 3] = [2, 1, 0];

/// `tf_select_table`: the time-frequency resolution change of each band,
/// by frame size and `4 * isTransient + 2 * tf_select + per_band_flag`;
/// positive is finer in frequency, negative finer in time.
const TF_SELECT_TABLE: [[i32; 8]; 4] = [
    [0, -1, 0, -1, 0, -1, 0, -1],
    [0, -1, 0, -2, 1, 0, 1, -1],
    [0, -2, 0, -3, 2, 0, 1, -1],
    [0, -2, 0, -3, 3, 0, 1, -1],
];

/// The post-filter's taps for each of its three tapsets (`gains`), Q15:
/// libopus's float literals as written, which `qconst16` rounds to `f32`
/// as C does.
#[allow(clippy::excessive_precision, reason = "libopus's literals, verbatim")]
const COMB_GAINS: [[i32; 3]; 3] = [
    [
        qconst16(0.306_640_625, 15),
        qconst16(0.217_041_015_6, 15),
        qconst16(0.129_638_671_9, 15),
    ],
    [
        qconst16(0.463_867_187_5, 15),
        qconst16(0.268_066_406_2, 15),
        qconst16(0.0, 15),
    ],
    [
        qconst16(0.799_804_687_5, 15),
        qconst16(0.100_097_656_2, 15),
        qconst16(0.0, 15),
    ],
];

/// Where [`comb_filter`] reads its input and writes its output -- a trait,
/// so that each kind gets a loop of its own, with no choice made per
/// sample.
trait CombIo {
    /// The input at `k` (negative: history).
    fn x(&self, k: isize) -> i32;
    fn set(&mut self, i: usize, v: i32);
    /// The output from `from` to `n` the input unchanged.
    fn copy(&mut self, from: usize, n: usize);
}

/// `buf[at + i]` read and written: the decoder's post-filter, which reads
/// back samples it has already filtered (an IIR filter). The history the
/// period reaches back to lies before `at`.
struct InPlace<'a> {
    buf: &'a mut [i32],
    at: usize,
}

impl CombIo for InPlace<'_> {
    #[inline]
    fn x(&self, k: isize) -> i32 {
        self.buf[self.at.wrapping_add_signed(k)]
    }

    #[inline]
    fn set(&mut self, i: usize, v: i32) {
        self.buf[self.at + i] = v;
    }

    #[inline]
    fn copy(&mut self, _from: usize, _n: usize) {}
}

/// `x[at + i]` read, `y[i]` written: the concealment's pre-filter (an FIR
/// filter).
struct Into<'a> {
    y: &'a mut [i32],
    x: &'a [i32],
    at: usize,
}

impl CombIo for Into<'_> {
    #[inline]
    fn x(&self, k: isize) -> i32 {
        self.x[self.at.wrapping_add_signed(k)]
    }

    #[inline]
    fn set(&mut self, i: usize, v: i32) {
        self.y[i] = v;
    }

    #[inline]
    fn copy(&mut self, from: usize, n: usize) {
        self.y[from..n].copy_from_slice(&self.x[self.at + from..self.at + n]);
    }
}

/// `comb_filter`: `n` samples through the pitch filter of period `t1`,
/// gain `g1` and taps `tapset1`, cross-faded over `overlap` samples (by
/// `window` squared) from the filter of `t0`, `g0` and `tapset0`.
#[allow(
    clippy::too_many_arguments,
    reason = "libopus's own parameters: both filters' period, gain and taps, and the cross-fade"
)]
fn comb_filter(
    mut io: impl CombIo,
    t0: i32,
    t1: i32,
    n: usize,
    g0: i32,
    g1: i32,
    tapset0: usize,
    tapset1: usize,
    window: &[i16],
    overlap: usize,
) {
    if g0 == 0 && g1 == 0 {
        io.copy(0, n);
        return;
    }
    // A gain of zero leaves its period 0: at least 2 keeps the taps on data.
    let t0 = t0.max(COMBFILTER_MINPERIOD) as isize;
    let t1 = t1.max(COMBFILTER_MINPERIOD) as isize;
    let g00 = extract16(mult16_16_p15(g0, COMB_GAINS[tapset0][0]));
    let g01 = extract16(mult16_16_p15(g0, COMB_GAINS[tapset0][1]));
    let g02 = extract16(mult16_16_p15(g0, COMB_GAINS[tapset0][2]));
    let g10 = extract16(mult16_16_p15(g1, COMB_GAINS[tapset1][0]));
    let g11 = extract16(mult16_16_p15(g1, COMB_GAINS[tapset1][1]));
    let g12 = extract16(mult16_16_p15(g1, COMB_GAINS[tapset1][2]));
    let mut x1 = io.x(-t1 + 1);
    let mut x2 = io.x(-t1);
    let mut x3 = io.x(-t1 - 1);
    let mut x4 = io.x(-t1 - 2);
    // An unchanged filter needs no cross-fade.
    let overlap = if g0 == g1 && t0 == t1 && tapset0 == tapset1 {
        0
    } else {
        overlap
    };
    for i in 0..overlap {
        let ii = i as isize;
        let x0 = io.x(ii - t1 + 2);
        let f = extract16(mult16_16_q15(i32::from(window[i]), i32::from(window[i])));
        let nf = Q15ONE - f;
        let y = io
            .x(ii)
            .wrapping_add(mult16_32_q15(mult16_16_q15(nf, g00), io.x(ii - t0)))
            .wrapping_add(mult16_32_q15(
                mult16_16_q15(nf, g01),
                io.x(ii - t0 + 1).wrapping_add(io.x(ii - t0 - 1)),
            ))
            .wrapping_add(mult16_32_q15(
                mult16_16_q15(nf, g02),
                io.x(ii - t0 + 2).wrapping_add(io.x(ii - t0 - 2)),
            ))
            .wrapping_add(mult16_32_q15(mult16_16_q15(f, g10), x2))
            .wrapping_add(mult16_32_q15(mult16_16_q15(f, g11), x1.wrapping_add(x3)))
            .wrapping_add(mult16_32_q15(mult16_16_q15(f, g12), x0.wrapping_add(x4)));
        io.set(i, saturate(y, SIG_SAT));
        x4 = x3;
        x3 = x2;
        x2 = x1;
        x1 = x0;
    }
    if g1 == 0 {
        io.copy(overlap, n);
        return;
    }
    // The rest with the constant filter (`comb_filter_const_c`, the
    // portable C version: libopus's ARM assembly rounds differently).
    let from = overlap as isize;
    let mut x4 = io.x(from - t1 - 2);
    let mut x3 = io.x(from - t1 - 1);
    let mut x2 = io.x(from - t1);
    let mut x1 = io.x(from - t1 + 1);
    for i in overlap..n {
        let ii = i as isize;
        let x0 = io.x(ii - t1 + 2);
        let y = io
            .x(ii)
            .wrapping_add(mult16_32_q15(g10, x2))
            .wrapping_add(mult16_32_q15(g11, x1.wrapping_add(x3)))
            .wrapping_add(mult16_32_q15(g12, x0.wrapping_add(x4)));
        io.set(i, saturate(y, SIG_SAT));
        x4 = x3;
        x3 = x2;
        x2 = x1;
        x1 = x0;
    }
}

/// `tf_decode`: each band's time-frequency change into `tf_res`.
fn tf_decode(
    start: usize,
    end: usize,
    is_transient: bool,
    tf_res: &mut [i32],
    lm: usize,
    dec: &mut Decoder<'_>,
) {
    let mut budget = dec.storage.wrapping_mul(8);
    let mut tell = dec.tell() as u32;
    let mut logp: u32 = if is_transient { 2 } else { 4 };
    let tf_select_rsv = lm > 0 && tell.wrapping_add(logp + 1) <= budget;
    budget -= u32::from(tf_select_rsv);
    let mut tf_changed = 0usize;
    let mut curr = 0usize;
    for res in &mut tf_res[start..end] {
        if tell.wrapping_add(logp) <= budget {
            curr ^= usize::from(dec.bit_logp(logp));
            tell = dec.tell() as u32;
            tf_changed |= curr;
        }
        *res = curr as i32;
        logp = if is_transient { 4 } else { 5 };
    }
    let t = 4 * usize::from(is_transient);
    let mut tf_select = 0;
    if tf_select_rsv
        && TF_SELECT_TABLE[lm][t + tf_changed] != TF_SELECT_TABLE[lm][t + 2 + tf_changed]
    {
        tf_select = usize::from(dec.bit_logp(1));
    }
    for res in &mut tf_res[start..end] {
        *res = TF_SELECT_TABLE[lm][t + 2 * tf_select + *res as usize];
    }
}

/// `init_caps`: each band's most bits, in eighths.
fn init_caps(cap: &mut [i32], lm: usize, c: usize) {
    for (i, v) in cap.iter_mut().enumerate().take(NB_EBANDS) {
        let n = (ebands(i + 1) - ebands(i)) << lm;
        *v = ((cache_cap(NB_EBANDS * (2 * lm + c - 1) + i) + 64) * c as i32 * n) >> 2;
    }
}

/// A CELT decoder's state: `OpusCustomDecoder`, in the standard mode.
pub(crate) struct CeltDecoder {
    /// The channels decoded into (`channels`).
    channels: usize,
    /// The channels the stream codes (`stream_channels`), which the Opus
    /// decoder sets per packet.
    stream_channels: usize,
    /// 48 kHz over the output rate.
    downsample: usize,
    /// The first and one past the last band coded.
    start: usize,
    end: usize,
    /// Never invert a stereo band's side (set for mono output: a downmix
    /// would cancel it).
    disable_inv: bool,

    // What a reset clears.
    /// The previous frame's final range, the noise generator's seed.
    rng: u32,
    /// Set when a frame read a value out of its range.
    error: bool,
    last_pitch_index: usize,
    /// Samples (at 400 Hz, `1 << LM` a frame) concealed since the last good
    /// frame, held at 10,000.
    loss_duration: i32,
    /// Conceal with noise until two good frames have come in a row.
    skip_plc: bool,
    postfilter_period: i32,
    postfilter_period_old: i32,
    postfilter_gain: i32,
    postfilter_gain_old: i32,
    postfilter_tapset: usize,
    postfilter_tapset_old: usize,
    /// Concealed audio is waiting to be faded into the next frame.
    prefilter_and_fold: bool,
    /// The de-emphasis filter's memory, each channel.
    preemph_mem: [i32; 2],
    /// Each channel's signal: DECODE_BUFFER_SIZE samples of history, then
    /// the overlap the next frame adds to.
    decode_mem: Vec<Vec<i32>>,
    /// The concealment's LPC coefficients, each channel.
    lpc: [[i32; LPC_ORDER]; 2],
    /// Band energies (log2, Q10): this frame's, the last two frames', and
    /// the background (noise floor) estimate.
    old_band_e: [i32; 2 * NB_EBANDS],
    old_log_e: [i32; 2 * NB_EBANDS],
    old_log_e2: [i32; 2 * NB_EBANDS],
    background_log_e: [i32; 2 * NB_EBANDS],
    /// The inverse MDCT's FFT buffer.
    fft_scratch: Vec<Cpx>,
    /// Buffers a frame fills before it reads them -- libopus's stack arrays
    /// `X`, `_norm` and `freq` -- kept between frames so that one costs no
    /// allocation and no clearing. Each is taken out for the frame and put
    /// back after; one lost to an error on the way is made again.
    scratch_x: Vec<i32>,
    scratch_norm: Vec<i32>,
    scratch_freq: Vec<i32>,
    scratch_freq2: Vec<i32>,
}

impl CeltDecoder {
    /// `celt_decoder_init`: a decoder of `channels` (1 or 2) at
    /// `sampling_rate` (8, 12, 16, 24 or 48 kHz).
    pub(crate) fn new(sampling_rate: u32, channels: usize) -> Result<Self, Error> {
        let downsample = match sampling_rate {
            48000 => 1,
            24000 => 2,
            16000 => 3,
            12000 => 4,
            8000 => 6,
            _ => return Err(Error::BadArgument),
        };
        if !(1..=2).contains(&channels) {
            return Err(Error::BadArgument);
        }
        let mut st = Self {
            channels,
            stream_channels: channels,
            downsample,
            start: 0,
            end: EFF_EBANDS,
            disable_inv: channels == 1,
            rng: 0,
            error: false,
            last_pitch_index: 0,
            loss_duration: 0,
            skip_plc: false,
            postfilter_period: 0,
            postfilter_period_old: 0,
            postfilter_gain: 0,
            postfilter_gain_old: 0,
            postfilter_tapset: 0,
            postfilter_tapset_old: 0,
            prefilter_and_fold: false,
            preemph_mem: [0; 2],
            decode_mem: vec![vec![0; DECODE_BUFFER_SIZE + OVERLAP]; channels],
            lpc: [[0; LPC_ORDER]; 2],
            old_band_e: [0; 2 * NB_EBANDS],
            old_log_e: [0; 2 * NB_EBANDS],
            old_log_e2: [0; 2 * NB_EBANDS],
            background_log_e: [0; 2 * NB_EBANDS],
            fft_scratch: Vec::new(),
            scratch_x: Vec::new(),
            scratch_norm: Vec::new(),
            scratch_freq: Vec::new(),
            scratch_freq2: Vec::new(),
        };
        st.reset();
        Ok(st)
    }

    /// `OPUS_RESET_STATE`: everything a stream has built up, cleared.
    pub(crate) fn reset(&mut self) {
        self.rng = 0;
        self.error = false;
        self.last_pitch_index = 0;
        self.loss_duration = 0;
        self.postfilter_period = 0;
        self.postfilter_period_old = 0;
        self.postfilter_gain = 0;
        self.postfilter_gain_old = 0;
        self.postfilter_tapset = 0;
        self.postfilter_tapset_old = 0;
        self.prefilter_and_fold = false;
        self.preemph_mem = [0; 2];
        for ch in &mut self.decode_mem {
            ch.fill(0);
        }
        self.lpc = [[0; LPC_ORDER]; 2];
        self.old_band_e = [0; 2 * NB_EBANDS];
        self.old_log_e = [-qconst16(28.0, DB_SHIFT); 2 * NB_EBANDS];
        self.old_log_e2 = [-qconst16(28.0, DB_SHIFT); 2 * NB_EBANDS];
        self.background_log_e = [0; 2 * NB_EBANDS];
        self.skip_plc = true;
    }

    /// `CELT_SET_START_BAND`: 17 for a hybrid frame's CELT layer, else 0.
    pub(crate) fn set_start_band(&mut self, band: usize) {
        self.start = band.min(NB_EBANDS - 1);
    }

    /// `CELT_SET_END_BAND`: the bands coded, by the packet's bandwidth.
    pub(crate) fn set_end_band(&mut self, band: usize) {
        self.end = band.clamp(1, NB_EBANDS);
    }

    /// `CELT_SET_CHANNELS`: the channels the stream codes.
    pub(crate) fn set_stream_channels(&mut self, channels: usize) {
        self.stream_channels = channels.clamp(1, 2);
    }

    /// `OPUS_SET_PHASE_INVERSION_DISABLED`.
    pub(crate) fn set_phase_inversion_disabled(&mut self, disabled: bool) {
        self.disable_inv = disabled;
    }

    /// `OPUS_GET_FINAL_RANGE`: the last frame's final range.
    pub(crate) fn final_range(&self) -> u32 {
        self.rng
    }

    /// `OPUS_GET_PITCH`: the post-filter's current period.
    pub(crate) fn pitch(&self) -> i32 {
        self.postfilter_period
    }

    /// `celt_decode_with_ec`: one frame of `frame_size` samples a channel
    /// (at the output rate) into `pcm`, interleaved -- added to what `pcm`
    /// holds, saturating, where `accum` (a hybrid frame's CELT layer over
    /// its SILK layer). `data` is the frame (`None`, or a byte or less, to
    /// conceal a lost one); `dec` the range decoder to continue, where
    /// another layer has begun the frame. The samples written a channel.
    pub(crate) fn decode<'a>(
        &mut self,
        data: Option<&'a [u8]>,
        pcm: &mut [i16],
        frame_size: usize,
        dec: Option<&mut Decoder<'a>>,
        accum: bool,
    ) -> Result<usize, Error> {
        let cc = self.channels;
        let frame_size = frame_size * self.downsample;
        let lm = (0..=MAX_LM as usize)
            .find(|&lm| SHORT_MDCT_SIZE << lm == frame_size)
            .ok_or(Error::BadArgument)?;
        let m = 1usize << lm;
        let len = data.map_or(0, <[u8]>::len);
        if len > MAX_PACKET {
            return Err(Error::BadArgument);
        }
        let out_len = frame_size / self.downsample;
        if pcm.len() < out_len * cc {
            return Err(Error::BufferTooSmall);
        }
        let n = m * SHORT_MDCT_SIZE;
        let start = self.start;
        let end = self.end;
        let eff_end = end.min(EFF_EBANDS);

        let data = match data {
            Some(d) if d.len() > 1 => d,
            _ => {
                self.decode_lost(n, lm);
                self.deemphasis(pcm, n, accum);
                return Ok(out_len);
            }
        };
        // Two good frames in a row before the pitch-based concealment again.
        if self.loss_duration == 0 {
            self.skip_plc = false;
        }
        let mut own;
        let dec = match dec {
            Some(d) => d,
            None => {
                own = Decoder::new(data);
                &mut own
            }
        };
        let c = self.stream_channels;
        if c == 1 {
            for i in 0..NB_EBANDS {
                self.old_band_e[i] = self.old_band_e[i].max(self.old_band_e[NB_EBANDS + i]);
            }
        }

        let mut total_bits = (len * 8) as i32;
        let mut tell = dec.tell();
        let silence = if tell >= total_bits {
            true
        } else if tell == 1 {
            dec.bit_logp(15)
        } else {
            false
        };
        if silence {
            // The rest of the frame counted as read.
            tell = (len * 8) as i32;
            dec.pretend_read(tell);
        }

        let mut postfilter_gain = 0;
        let mut postfilter_pitch = 0;
        let mut postfilter_tapset = 0;
        if start == 0 && tell + 16 <= total_bits {
            if dec.bit_logp(1) {
                let octave = dec.uint(6);
                postfilter_pitch = ((16 << octave) + dec.bits(4 + octave)) as i32 - 1;
                let qg = dec.bits(3) as i32;
                if dec.tell() + 2 <= total_bits {
                    postfilter_tapset = dec.icdf(&TAPSET_ICDF, 2);
                }
                postfilter_gain = qconst16(0.093_75, 15) * (qg + 1);
            }
            tell = dec.tell();
        }

        let is_transient = if lm > 0 && tell + 3 <= total_bits {
            let t = dec.bit_logp(3);
            tell = dec.tell();
            t
        } else {
            false
        };
        let short_blocks = is_transient;

        // The global flags.
        let intra_ener = if tell + 3 <= total_bits {
            dec.bit_logp(3)
        } else {
            false
        };
        // After a loss, an energy prediction made safe from loud artefacts.
        if !intra_ener && self.loss_duration != 0 {
            self.recover_energy(lm, start, end);
        }
        unquant_coarse(start, end, &mut self.old_band_e, intra_ener, dec, c, lm);

        let mut tf_res = [0i32; NB_EBANDS];
        tf_decode(start, end, is_transient, &mut tf_res, lm, dec);

        tell = dec.tell();
        let spread_decision = if tell + 4 <= total_bits {
            dec.icdf(&SPREAD_ICDF, 5) as i32
        } else {
            SPREAD_NORMAL
        };

        let mut cap = [0i32; NB_EBANDS];
        init_caps(&mut cap, lm, c);

        let mut offsets = [0i32; NB_EBANDS];
        let mut dynalloc_logp = 6;
        total_bits <<= BITRES;
        let mut tell = dec.tell_frac() as i32;
        for i in start..end {
            let width = (c as i32 * (ebands(i + 1) - ebands(i))) << lm;
            // Six bits, but no more than a bit a sample and no less than an
            // eighth.
            let quanta = (width << BITRES).min((6 << BITRES).max(width));
            let mut dynalloc_loop_logp = dynalloc_logp;
            let mut boost = 0;
            while tell + (dynalloc_loop_logp << BITRES) < total_bits && boost < cap[i] {
                let flag = dec.bit_logp(dynalloc_loop_logp as u32);
                tell = dec.tell_frac() as i32;
                if !flag {
                    break;
                }
                boost += quanta;
                total_bits -= quanta;
                dynalloc_loop_logp = 1;
            }
            offsets[i] = boost;
            // Boosting more likely once one band has been.
            if boost > 0 {
                dynalloc_logp = 2.max(dynalloc_logp - 1);
            }
        }

        let alloc_trim = if tell + (6 << BITRES) <= total_bits {
            dec.icdf(&TRIM_ICDF, 7) as i32
        } else {
            5
        };

        let mut bits = (((len * 8) as i32) << BITRES)
            .wrapping_sub(dec.tell_frac() as i32)
            .wrapping_sub(1);
        let anti_collapse_rsv = if is_transient && lm >= 2 && bits >= ((lm as i32 + 2) << BITRES) {
            1 << BITRES
        } else {
            0
        };
        bits -= anti_collapse_rsv;

        let mut pulses = [0i32; NB_EBANDS];
        let mut fine_quant = [0i32; NB_EBANDS];
        let mut fine_priority = [0i32; NB_EBANDS];
        let alloc = compute_allocation(
            start,
            end,
            &offsets,
            &cap,
            alloc_trim,
            bits,
            &mut pulses,
            &mut fine_quant,
            &mut fine_priority,
            c as i32,
            lm as i32,
            dec,
        );

        unquant_fine(start, end, &mut self.old_band_e, &fine_quant, dec, c);

        for ch in &mut self.decode_mem {
            ch.copy_within(n..DECODE_BUFFER_SIZE + OVERLAP, 0);
        }

        // The band shapes.
        let mut collapse_masks = [0u8; 2 * NB_EBANDS];
        // libopus's `X`: nothing reads a coefficient before the bands write
        // it, so last frame's are no matter.
        let mut x_buf = taken(&mut self.scratch_x, 2 * MAX_FRAME);
        let mut norm_buf = taken(&mut self.scratch_norm, NORM_SIZE);
        let x = &mut x_buf[..c * n];
        {
            let (x_, y_) = x.split_at_mut(n);
            let params = AllBands {
                start,
                end,
                pulses: &pulses,
                short_blocks,
                spread: spread_decision,
                dual_stereo: alloc.dual_stereo,
                intensity: alloc.intensity,
                tf_res: &tf_res,
                total_bits: ((len * (8 << BITRES)) as i32) - anti_collapse_rsv,
                balance: alloc.balance,
                lm: lm as i32,
                coded_bands: alloc.coded_bands,
                disable_inv: self.disable_inv,
            };
            quant_all_bands(
                &params,
                x_,
                if c == 2 { Some(y_) } else { None },
                &mut collapse_masks,
                dec,
                &mut self.rng,
                &mut norm_buf,
            );
        }
        self.scratch_norm = norm_buf;

        let anti_collapse_on = anti_collapse_rsv > 0 && dec.bits(1) != 0;

        let bits_left = (len * 8) as i32 - dec.tell();
        unquant_finalise(
            start,
            end,
            &mut self.old_band_e,
            &fine_quant,
            &fine_priority,
            bits_left,
            dec,
            c,
        );

        if anti_collapse_on {
            anti_collapse(
                x,
                &collapse_masks,
                lm as i32,
                c,
                n,
                start,
                end,
                &self.old_band_e,
                &self.old_log_e,
                &self.old_log_e2,
                &pulses,
                self.rng,
            );
        }

        if silence {
            for e in &mut self.old_band_e[..c * NB_EBANDS] {
                *e = -qconst16(28.0, DB_SHIFT);
            }
        }
        if self.prefilter_and_fold {
            self.prefilter_and_fold(n);
        }
        self.synthesis(x, n, start, eff_end, c, is_transient, lm, silence);
        self.scratch_x = x_buf;

        let out_at = DECODE_BUFFER_SIZE - n;
        let window = window();
        for ch in &mut self.decode_mem {
            self.postfilter_period = self.postfilter_period.max(COMBFILTER_MINPERIOD);
            self.postfilter_period_old = self.postfilter_period_old.max(COMBFILTER_MINPERIOD);
            comb_filter(
                InPlace {
                    buf: ch,
                    at: out_at,
                },
                self.postfilter_period_old,
                self.postfilter_period,
                SHORT_MDCT_SIZE,
                self.postfilter_gain_old,
                self.postfilter_gain,
                self.postfilter_tapset_old,
                self.postfilter_tapset,
                window,
                OVERLAP,
            );
            if lm != 0 {
                comb_filter(
                    InPlace {
                        buf: ch,
                        at: out_at + SHORT_MDCT_SIZE,
                    },
                    self.postfilter_period,
                    postfilter_pitch,
                    n - SHORT_MDCT_SIZE,
                    self.postfilter_gain,
                    postfilter_gain,
                    self.postfilter_tapset,
                    postfilter_tapset,
                    window,
                    OVERLAP,
                );
            }
        }
        self.postfilter_period_old = self.postfilter_period;
        self.postfilter_gain_old = self.postfilter_gain;
        self.postfilter_tapset_old = self.postfilter_tapset;
        self.postfilter_period = postfilter_pitch;
        self.postfilter_gain = postfilter_gain;
        self.postfilter_tapset = postfilter_tapset;
        if lm != 0 {
            self.postfilter_period_old = self.postfilter_period;
            self.postfilter_gain_old = self.postfilter_gain;
            self.postfilter_tapset_old = self.postfilter_tapset;
        }

        if c == 1 {
            self.old_band_e.copy_within(0..NB_EBANDS, NB_EBANDS);
        }
        if is_transient {
            for (l, &e) in self.old_log_e.iter_mut().zip(&self.old_band_e) {
                *l = (*l).min(e);
            }
        } else {
            self.old_log_e2 = self.old_log_e;
            self.old_log_e = self.old_band_e;
        }
        // The noise floor may rise 2.4 dB a second -- in DTX, by all the
        // missing packets' worth at once.
        let max_background_increase =
            160.min(self.loss_duration + m as i32) * qconst16(0.001, DB_SHIFT);
        for (b, &e) in self.background_log_e.iter_mut().zip(&self.old_band_e) {
            *b = extract16((*b + max_background_increase).min(e));
        }
        // In case start or end were to change.
        for ch in 0..2 {
            for i in (0..start).chain(end..NB_EBANDS) {
                let k = ch * NB_EBANDS + i;
                self.old_band_e[k] = 0;
                self.old_log_e[k] = -qconst16(28.0, DB_SHIFT);
                self.old_log_e2[k] = -qconst16(28.0, DB_SHIFT);
            }
        }
        self.rng = dec.range();

        self.deemphasis(pcm, n, accum);
        self.loss_duration = 0;
        self.prefilter_and_fold = false;
        if dec.tell() > 8 * len as i32 {
            return Err(Error::Internal);
        }
        if dec.error() {
            self.error = true;
        }
        Ok(out_len)
    }

    /// The energy prediction after a loss: bands whose energy was falling
    /// keep falling, the others take their lowest of the last frames; short
    /// frames, which fluctuate more, a little lower still.
    fn recover_energy(&mut self, lm: usize, start: usize, end: usize) {
        let missing = 10.min(self.loss_duration >> lm);
        let safety = match lm {
            0 => qconst16(1.5, DB_SHIFT),
            1 => qconst16(0.5, DB_SHIFT),
            _ => 0,
        };
        for ch in 0..2 {
            for i in start..end {
                let k = ch * NB_EBANDS + i;
                let (e0, e1, e2) = (self.old_band_e[k], self.old_log_e[k], self.old_log_e2[k]);
                if e0 < e1.max(e2) {
                    // Falling: continue the trend.
                    let slope = (e1 - e0).max(half32(e2 - e0));
                    let e0 = e0 - 0.max((1 + missing).wrapping_mul(slope));
                    self.old_band_e[k] = extract16((-qconst16(20.0, DB_SHIFT)).max(e0));
                } else {
                    self.old_band_e[k] = e0.min(e1).min(e2);
                }
                self.old_band_e[k] = extract16(self.old_band_e[k] - safety);
            }
        }
    }

    /// `celt_synthesis`: the band shapes `x` (`c` channels of `n`) scaled
    /// by the band energies and inverse-transformed into each output
    /// channel's frame, a mono stream copied to both channels and a stereo
    /// one mixed down for one.
    #[allow(
        clippy::too_many_arguments,
        reason = "libopus's own parameters for the frame being synthesised"
    )]
    fn synthesis(
        &mut self,
        x: &[i32],
        n: usize,
        start: usize,
        eff_end: usize,
        c: usize,
        is_transient: bool,
        lm: usize,
        silence: bool,
    ) {
        let cc = self.channels;
        let m = 1usize << lm;
        let (blocks, nb, shift) = if is_transient {
            (m, SHORT_MDCT_SIZE, MAX_LM as usize)
        } else {
            (1, SHORT_MDCT_SIZE << lm, MAX_LM as usize - lm)
        };
        let out_at = DECODE_BUFFER_SIZE - n;
        let window = window();
        let ds = self.downsample;
        let mut freq_buf = taken(&mut self.scratch_freq, MAX_FRAME);
        let mut freq2_buf = taken(&mut self.scratch_freq2, MAX_FRAME);
        let freq = &mut freq_buf[..n];
        let imdct = |freq: &[i32], out: &mut Vec<i32>, scratch: &mut Vec<Cpx>| {
            for b in 0..blocks {
                backward(
                    &freq[b..],
                    &mut out[out_at + nb * b..],
                    window,
                    OVERLAP,
                    shift,
                    blocks,
                    scratch,
                );
            }
        };
        if cc == 2 && c == 1 {
            // A mono stream copied to both channels.
            denormalise_bands(x, freq, &self.old_band_e, start, eff_end, m, ds, silence);
            let [left, right] = &mut self.decode_mem[..] else {
                return;
            };
            imdct(freq, left, &mut self.fft_scratch);
            imdct(freq, right, &mut self.fft_scratch);
        } else if cc == 1 && c == 2 {
            // A stereo stream mixed down.
            let freq2 = &mut freq2_buf[..n];
            denormalise_bands(x, freq, &self.old_band_e, start, eff_end, m, ds, silence);
            denormalise_bands(
                &x[n..],
                freq2,
                &self.old_band_e[NB_EBANDS..],
                start,
                eff_end,
                m,
                ds,
                silence,
            );
            for (f, &f2) in freq.iter_mut().zip(freq2.iter()) {
                *f = half32(*f).wrapping_add(half32(f2));
            }
            imdct(freq, &mut self.decode_mem[0], &mut self.fft_scratch);
        } else {
            for ch in 0..cc {
                denormalise_bands(
                    &x[ch * n..],
                    freq,
                    &self.old_band_e[ch * NB_EBANDS..],
                    start,
                    eff_end,
                    m,
                    ds,
                    silence,
                );
                imdct(freq, &mut self.decode_mem[ch], &mut self.fft_scratch);
            }
        }
        // Saturated, so that the post-filter cannot overflow.
        for ch in &mut self.decode_mem {
            for v in &mut ch[out_at..out_at + n] {
                *v = saturate(*v, SIG_SAT);
            }
        }
        self.scratch_freq = freq_buf;
        self.scratch_freq2 = freq2_buf;
    }

    /// `deemphasis`: the frame's `n` samples a channel through the
    /// de-emphasis filter into `pcm` (interleaved, 16-bit, every
    /// `downsample`th sample) -- or added to `pcm`, saturating, for `accum`.
    fn deemphasis(&mut self, pcm: &mut [i16], n: usize, accum: bool) {
        let cc = self.channels;
        let coef0 = PREEMPH[0];
        let ds = self.downsample;
        let out_at = DECODE_BUFFER_SIZE - n;
        // The common case, at full rate and not added to SILK's samples:
        // straight through, both channels at once when there are two
        // (libopus's `deemphasis_stereo_simple`; the same arithmetic).
        if ds == 1 && !accum {
            if let [x0, x1] = &self.decode_mem[..] {
                let (x0, x1) = (&x0[out_at..out_at + n], &x1[out_at..out_at + n]);
                let [mut m0, mut m1] = self.preemph_mem;
                for ((out, &a), &b) in pcm.chunks_exact_mut(2).zip(x0).zip(x1) {
                    let tmp0 = a.wrapping_add(m0);
                    let tmp1 = b.wrapping_add(m1);
                    m0 = mult16_32_q15(coef0, tmp0);
                    m1 = mult16_32_q15(coef0, tmp1);
                    out[0] = sig2word16(tmp0) as i16;
                    out[1] = sig2word16(tmp1) as i16;
                }
                self.preemph_mem = [m0, m1];
                return;
            }
            if let [x0] = &self.decode_mem[..] {
                let mut m = self.preemph_mem[0];
                for (out, &a) in pcm.iter_mut().zip(&x0[out_at..out_at + n]) {
                    let tmp = a.wrapping_add(m);
                    m = mult16_32_q15(coef0, tmp);
                    *out = sig2word16(tmp) as i16;
                }
                self.preemph_mem[0] = m;
                return;
            }
        }
        for c in 0..cc {
            let x = &self.decode_mem[c][out_at..out_at + n];
            let mut mem = self.preemph_mem[c];
            // This channel's samples of the interleaved output.
            let mut y = pcm.get_mut(c..).unwrap_or_default().iter_mut().step_by(cc);
            // Every `ds`th sample is kept, without filtering first: the band
            // limit leaves nothing to alias. Each block of `ds` samples is
            // filtered through, the first of it kept (libopus filters all of
            // them into a scratch buffer, then picks; the same samples).
            for block in x.chunks(ds) {
                let mut kept = 0;
                for (k, &v) in block.iter().enumerate() {
                    let tmp = v.wrapping_add(mem);
                    mem = mult16_32_q15(coef0, tmp);
                    if k == 0 {
                        kept = tmp;
                    }
                }
                if let Some(out) = y.next() {
                    *out = if accum {
                        saturate16(i32::from(*out) + sig2word16(kept)) as i16
                    } else {
                        sig2word16(kept) as i16
                    };
                }
            }
            self.preemph_mem[c] = mem;
        }
    }

    /// `prefilter_and_fold`: the concealed audio's overlap through the
    /// pre-filter (the next frame's post-filter will run over it again) and
    /// folded as the MDCT's TDAC would, to blend with the next frame.
    fn prefilter_and_fold(&mut self, n: usize) {
        let window = window();
        let mut etmp = [0i32; OVERLAP];
        for ch in &mut self.decode_mem {
            comb_filter(
                Into {
                    y: &mut etmp,
                    x: ch.as_slice(),
                    at: DECODE_BUFFER_SIZE - n,
                },
                self.postfilter_period_old,
                self.postfilter_period,
                OVERLAP,
                -self.postfilter_gain_old,
                -self.postfilter_gain,
                self.postfilter_tapset_old,
                self.postfilter_tapset,
                &[],
                0,
            );
            for i in 0..OVERLAP / 2 {
                ch[DECODE_BUFFER_SIZE - n + i] =
                    mult16_32_q15(i32::from(window[i]), etmp[OVERLAP - 1 - i])
                        .wrapping_add(mult16_32_q15(i32::from(window[OVERLAP - i - 1]), etmp[i]));
            }
        }
    }

    /// `celt_plc_pitch_search`: the period the concealment repeats.
    fn plc_pitch_search(&self) -> usize {
        let mut lp = vec![0i32; DECODE_BUFFER_SIZE >> 1];
        let chans: Vec<&[i32]> = self.decode_mem.iter().map(Vec::as_slice).collect();
        pitch_downsample(&chans, &mut lp, DECODE_BUFFER_SIZE);
        let pitch = pitch_search(
            &lp[PLC_PITCH_LAG_MAX >> 1..],
            &lp,
            DECODE_BUFFER_SIZE - PLC_PITCH_LAG_MAX,
            PLC_PITCH_LAG_MAX - PLC_PITCH_LAG_MIN,
        );
        // `pitch_search` returns 0 to 619 here.
        PLC_PITCH_LAG_MAX
            - usize::try_from(pitch)
                .unwrap_or(0)
                .min(PLC_PITCH_LAG_MAX - 1)
    }

    /// `celt_decode_lost`: a missing frame of `n` samples concealed.
    fn decode_lost(&mut self, n: usize, lm: usize) {
        let cc = self.channels;
        let loss_duration = self.loss_duration;
        let start = self.start;
        let window = window();
        let noise_based = loss_duration >= 40 || start != 0 || self.skip_plc;
        if noise_based {
            // Noise at the last energies, decaying to the noise floor.
            let end = self.end;
            let eff_end = start.max(end.min(EFF_EBANDS));
            let mut x = vec![0i32; cc * n];
            for ch in &mut self.decode_mem {
                ch.copy_within(n..DECODE_BUFFER_SIZE + OVERLAP, 0);
            }
            if self.prefilter_and_fold {
                self.prefilter_and_fold(n);
            }
            let decay = if loss_duration == 0 {
                qconst16(1.5, DB_SHIFT)
            } else {
                qconst16(0.5, DB_SHIFT)
            };
            for ch in 0..cc {
                for i in start..end {
                    let k = ch * NB_EBANDS + i;
                    self.old_band_e[k] =
                        extract16(self.background_log_e[k].max(self.old_band_e[k] - decay));
                }
            }
            let mut seed = self.rng;
            for ch in 0..cc {
                for i in start..eff_end {
                    let boffs = n * ch + ((ebands(i) as usize) << lm);
                    let blen = ((ebands(i + 1) - ebands(i)) as usize) << lm;
                    for v in &mut x[boffs..boffs + blen] {
                        seed = lcg_rand(seed);
                        *v = extract16((seed as i32) >> 20);
                    }
                    renormalise_vector(&mut x[boffs..], blen, Q15ONE);
                }
            }
            self.rng = seed;
            self.synthesis(&x, n, start, eff_end, cc, false, lm, false);
            self.prefilter_and_fold = false;
            // The pitch-based concealment waits for two good frames.
            self.skip_plc = true;
        } else {
            // The last pitch period repeated through the LPC filter.
            let mut fade = Q15ONE;
            let pitch_index = if loss_duration == 0 {
                let p = self.plc_pitch_search();
                self.last_pitch_index = p;
                p
            } else {
                fade = qconst16(0.8, 15);
                self.last_pitch_index
            };
            // The excitation for two pitch periods, to look for a decaying
            // signal, but no more than MAX_PERIOD.
            let exc_length = (2 * pitch_index).min(MAX_PERIOD);
            let mut exc = vec![0i32; MAX_PERIOD + LPC_ORDER];
            let mut fir_tmp = vec![0i32; exc_length];
            for ch in 0..cc {
                let buf = &mut self.decode_mem[ch];
                for (i, e) in exc.iter_mut().enumerate() {
                    *e = sround16(
                        buf[DECODE_BUFFER_SIZE - MAX_PERIOD - LPC_ORDER + i],
                        SIG_SHIFT,
                    );
                }
                // `exc[LPC_ORDER + i]` is libopus's `exc[i]`.
                const E: usize = LPC_ORDER;
                if loss_duration == 0 {
                    // The LPC of the last MAX_PERIOD samples before the loss.
                    let mut ac = [0i32; LPC_ORDER + 1];
                    autocorr(&exc[E..], &mut ac, window, OVERLAP, LPC_ORDER, MAX_PERIOD);
                    // A noise floor of -40 dB.
                    ac[0] = ac[0].wrapping_add(ac[0] >> 13);
                    // Lag windowing, for a stable Levinson-Durbin recursion.
                    for (i, a) in ac.iter_mut().enumerate().skip(1) {
                        let i = i as i32;
                        *a = a.wrapping_sub(mult16_32_q15(2 * i * i, *a));
                    }
                    let lpc_c = &mut self.lpc[ch];
                    lpc(lpc_c, &ac, LPC_ORDER);
                    // Bandwidth expansion until the IIR filter cannot overflow:
                    // 32768 * sum(abs(filter)) < 2^31.
                    loop {
                        let sum = lpc_c
                            .iter()
                            .fold(qconst16(1.0, SIG_SHIFT), |s, &v| s + extract16(v).abs());
                        if sum < 65535 {
                            break;
                        }
                        let mut tmp = Q15ONE;
                        for v in lpc_c.iter_mut() {
                            tmp = extract16(mult16_16_q15(qconst16(0.99, 15), tmp));
                            *v = extract16(mult16_16_q15(*v, tmp));
                        }
                    }
                }
                let lpc_c = &self.lpc[ch];
                // The excitation of the exc_length samples before the loss.
                fir(
                    &exc,
                    E + MAX_PERIOD - exc_length,
                    lpc_c,
                    &mut fir_tmp,
                    exc_length,
                    LPC_ORDER,
                );
                exc[E + MAX_PERIOD - exc_length..E + MAX_PERIOD].copy_from_slice(&fir_tmp);

                // Whether the waveform is decaying, and how fast: so as not
                // to add energy concealing a decaying segment.
                let decay = {
                    let mut e1 = 1i32;
                    let mut e2 = 1i32;
                    let tail = &exc[E + MAX_PERIOD - exc_length..E + MAX_PERIOD];
                    let shift = 0.max(2 * zlog2(maxabs16(tail)) - 20) as u32;
                    let decay_length = exc_length >> 1;
                    for i in 0..decay_length {
                        let e = exc[E + MAX_PERIOD - decay_length + i];
                        e1 = e1.wrapping_add(mult16_16(e, e) >> shift);
                        let e = exc[E + MAX_PERIOD - 2 * decay_length + i];
                        e2 = e2.wrapping_add(mult16_16(e, e) >> shift);
                    }
                    e1 = e1.min(e2);
                    extract16(sqrt(frac_div32(e1 >> 1, e2)))
                };

                // The memory a frame to the left, for the new frame (the
                // overlap past the end is not used).
                buf.copy_within(n..DECODE_BUFFER_SIZE, 0);

                // The excitation's last period repeated, each period scaled
                // down by `decay` again.
                let extrapolation_offset = MAX_PERIOD - pitch_index;
                // A whole MDCT window, overlap/2 samples each side included.
                let extrapolation_len = n + OVERLAP;
                // Fading too, after the first lost frame.
                let mut attenuation = extract16(mult16_16_q15(fade, decay));
                let mut s1 = 0i32;
                let mut j = 0usize;
                for i in 0..extrapolation_len {
                    if j >= pitch_index {
                        j -= pitch_index;
                        attenuation = extract16(mult16_16_q15(attenuation, decay));
                    }
                    buf[DECODE_BUFFER_SIZE - n + i] = shl32(
                        mult16_16_q15(attenuation, exc[E + extrapolation_offset + j]),
                        SIG_SHIFT,
                    );
                    // The energy of the decoded signal whose excitation is
                    // copied.
                    let tmp = sround16(
                        buf[DECODE_BUFFER_SIZE - MAX_PERIOD - n + extrapolation_offset + j],
                        SIG_SHIFT,
                    );
                    s1 = s1.wrapping_add(mult16_16(tmp, tmp) >> 10);
                    j += 1;
                }
                // The synthesis filter's memory: the last decoded samples
                // before the overlap, for a continuous signal.
                let mut lpc_mem = [0i32; LPC_ORDER];
                for (i, mem) in lpc_mem.iter_mut().enumerate() {
                    *mem = sround16(buf[DECODE_BUFFER_SIZE - n - 1 - i], SIG_SHIFT);
                }
                // The excitation back into a signal.
                iir_in_place(
                    buf,
                    DECODE_BUFFER_SIZE - n,
                    lpc_c,
                    extrapolation_len,
                    LPC_ORDER,
                    &lpc_mem,
                );
                for v in
                    &mut buf[DECODE_BUFFER_SIZE - n..DECODE_BUFFER_SIZE - n + extrapolation_len]
                {
                    *v = saturate(*v, SIG_SAT);
                }

                // Attenuated where the synthesis came out louder than the
                // signal it continues, as it can when the signal changes
                // within the window.
                let mut s2 = 0i32;
                for &v in &buf[DECODE_BUFFER_SIZE - n..DECODE_BUFFER_SIZE - n + extrapolation_len] {
                    let tmp = sround16(v, SIG_SHIFT);
                    s2 = s2.wrapping_add(mult16_16(tmp, tmp) >> 10);
                }
                let frame =
                    &mut buf[DECODE_BUFFER_SIZE - n..DECODE_BUFFER_SIZE - n + extrapolation_len];
                if s1 <= s2 >> 2 {
                    // An explosion in the synthesis.
                    frame.fill(0);
                } else if s1 < s2 {
                    let ratio = extract16(sqrt(frac_div32((s1 >> 1) + 1, s2 + 1)));
                    for (i, v) in frame.iter_mut().enumerate() {
                        let g = if i < OVERLAP {
                            extract16(Q15ONE - mult16_16_q15(i32::from(window[i]), Q15ONE - ratio))
                        } else {
                            ratio
                        };
                        *v = mult16_32_q15(g, *v);
                    }
                }
            }
            self.prefilter_and_fold = true;
        }
        // Held to something large, against wrapping round.
        self.loss_duration = 10000.min(loss_duration + (1 << lm));
    }
}
