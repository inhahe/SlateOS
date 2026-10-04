//! SILK's resampler, from its internal rate (8, 12 or 16 kHz) to the
//! decoder's (8 to 48 kHz): a copy, a 2x all-pass upsampler, the 2x
//! upsampler followed by a 12-phase FIR interpolator, or a second-order AR
//! filter followed by an FIR decimator -- each with the input delayed by
//! libopus's per-ratio amount, so that SILK lines up with CELT.
//!
//! Translated into Rust from libopus 1.5.2's `silk/resampler.c`,
//! `silk/resampler_private_up2_HQ.c`, `silk/resampler_private_IIR_FIR.c`,
//! `silk/resampler_private_down_FIR.c`, `silk/resampler_private_AR2.c` and
//! `silk/resampler_rom.h`, copyright Skype Limited, Xiph.Org and the
//! contributors named in its `COPYING`, used under libopus's BSD licence
//! (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "every index is within the state's arrays and the batch buffers at libopus's sizes (batches of 10 ms, FIR orders of at most 36), the interpolation indices below the input's length by construction"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sample counts below 960 and filter arithmetic within 32 bits by libopus's own design; what C lets overflow wraps through the fixed-point helpers"
)]

use super::fix::{div32, lshift, mul, rshift_round, sat16, smlabb, smlawb, smulbb, smulwb, smulww};
use super::tables::{
    RESAMPLER_1_2_COEFS, RESAMPLER_2_3_COEFS, RESAMPLER_3_4_COEFS, RESAMPLER_FRAC_FIR_12,
};

/// `silk_resampler_up2_hq_0`, `_1`: the two all-pass chains' coefficients.
const UP2_HQ_0: [i32; 3] = [1746, 14986, 39083 - 65536];
const UP2_HQ_1: [i32; 3] = [6854, 25769, 55542 - 65536];

/// `RESAMPLER_DOWN_ORDER_FIR0`, `_FIR1`, `RESAMPLER_ORDER_FIR_12`. (`_FIR2`,
/// 36 taps, is for 1:3, 1:4 and 1:6, which only an encoder's 24 or 48 kHz
/// input needs: a decoder's SILK runs at 16 kHz at most.)
const DOWN_ORDER_FIR0: usize = 18;
const DOWN_ORDER_FIR1: usize = 24;
const ORDER_FIR_12: usize = 8;
/// A batch is 10 ms of input.
const MAX_BATCH_SIZE_MS: usize = 10;

/// `delay_matrix_dec`: the input's delay, by internal rate (8, 12, 16 kHz)
/// and output rate (8, 12, 16, 24, 48 kHz).
const DELAY_MATRIX_DEC: [[usize; 5]; 3] = [[4, 0, 2, 0, 0], [0, 9, 4, 7, 4], [0, 3, 12, 7, 7]];

/// `rateID`: 8, 12, 16, 24, 48 kHz to 0..4.
const fn rate_id(r: i32) -> usize {
    ((((r >> 12) - (r > 16000) as i32) >> (r > 24000) as i32) - 1) as usize
}

/// Which resampler a state runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Function {
    #[default]
    Copy,
    Up2Hq,
    IirFir,
    DownFir,
}

/// `silk_resampler_state_struct` (its FIR state's union as two arrays: a
/// state runs one function, and a reset clears both).
#[derive(Clone, Debug)]
pub(crate) struct Resampler {
    s_iir: [i32; 6],
    s_fir_i32: [i32; 36],
    s_fir_i16: [i16; 36],
    delay_buf: [i16; 48],
    function: Function,
    batch_size: usize,
    inv_ratio_q16: i32,
    fir_order: usize,
    fir_fracs: i32,
    fs_in_khz: usize,
    fs_out_khz: usize,
    input_delay: usize,
    coefs: &'static [i16],
}

impl Default for Resampler {
    /// A cleared state (`silk_memset( S, 0, ...)`): a copy, of nothing yet.
    fn default() -> Self {
        Self {
            s_iir: [0; 6],
            s_fir_i32: [0; 36],
            s_fir_i16: [0; 36],
            delay_buf: [0; 48],
            function: Function::Copy,
            batch_size: 0,
            inv_ratio_q16: 0,
            fir_order: 0,
            fir_fracs: 0,
            fs_in_khz: 0,
            fs_out_khz: 0,
            input_delay: 0,
            coefs: &[],
        }
    }
}

impl Resampler {
    /// `silk_resampler_init` for a decoder: from `fs_in` (8, 12 or 16 kHz)
    /// to `fs_out` (8, 12, 16, 24 or 48 kHz). `None` for any other rates.
    pub(crate) fn new(fs_in: i32, fs_out: i32) -> Option<Self> {
        if !matches!(fs_in, 8000 | 12000 | 16000)
            || !matches!(fs_out, 8000 | 12000 | 16000 | 24000 | 48000)
        {
            return None;
        }
        let mut s = Self {
            input_delay: DELAY_MATRIX_DEC[rate_id(fs_in)][rate_id(fs_out)],
            fs_in_khz: (fs_in / 1000) as usize,
            fs_out_khz: (fs_out / 1000) as usize,
            ..Self::default()
        };
        s.batch_size = s.fs_in_khz * MAX_BATCH_SIZE_MS;
        let mut up2x = 0;
        if fs_out > fs_in {
            if fs_out == mul(fs_in, 2) {
                // Exactly twice: the 2x upsampler alone.
                s.function = Function::Up2Hq;
            } else {
                s.function = Function::IirFir;
                up2x = 1;
            }
        } else if fs_out < fs_in {
            s.function = Function::DownFir;
            let (fracs, order, coefs): (i32, usize, &'static [i16]) =
                if mul(fs_out, 4) == mul(fs_in, 3) {
                    (3, DOWN_ORDER_FIR0, &RESAMPLER_3_4_COEFS)
                } else if mul(fs_out, 3) == mul(fs_in, 2) {
                    (2, DOWN_ORDER_FIR0, &RESAMPLER_2_3_COEFS)
                } else if mul(fs_out, 2) == fs_in {
                    (1, DOWN_ORDER_FIR1, &RESAMPLER_1_2_COEFS)
                } else {
                    return None;
                };
            s.fir_fracs = fracs;
            s.fir_order = order;
            s.coefs = coefs;
        }
        // The ratio of input to output, rounded up.
        s.inv_ratio_q16 = lshift(div32(lshift(fs_in, 14 + up2x), fs_out), 2);
        while smulww(s.inv_ratio_q16, fs_out) < lshift(fs_in, up2x) {
            s.inv_ratio_q16 += 1;
        }
        Some(s)
    }

    /// `silk_resampler`: `input` (at least a millisecond) into `out`, which
    /// takes `input.len() * fs_out / fs_in` samples.
    pub(crate) fn process(&mut self, out: &mut [i16], input: &[i16]) {
        let in_len = input.len();
        let n_samples = self.fs_in_khz - self.input_delay;
        // The first millisecond through the delay buffer.
        self.delay_buf[self.input_delay..self.input_delay + n_samples]
            .copy_from_slice(&input[..n_samples]);
        let fs_in = self.fs_in_khz;
        let fs_out = self.fs_out_khz;
        let delayed: [i16; 48] = self.delay_buf;
        let rest = &input[n_samples..in_len - (fs_in - n_samples)];
        match self.function {
            Function::Up2Hq => {
                up2_hq(&mut self.s_iir, out, &delayed[..fs_in]);
                up2_hq(&mut self.s_iir, &mut out[fs_out..], rest);
            }
            Function::IirFir => {
                self.iir_fir(out, &delayed[..fs_in]);
                self.iir_fir(&mut out[fs_out..], rest);
            }
            Function::DownFir => {
                self.down_fir(out, &delayed[..fs_in]);
                self.down_fir(&mut out[fs_out..], rest);
            }
            Function::Copy => {
                out[..fs_in].copy_from_slice(&delayed[..fs_in]);
                out[fs_out..fs_out + rest.len()].copy_from_slice(rest);
            }
        }
        // The delay buffer's next contents.
        self.delay_buf[..self.input_delay].copy_from_slice(&input[in_len - self.input_delay..]);
    }

    /// `silk_resampler_private_IIR_FIR`: 2x upsampled, then interpolated by
    /// a 12-phase FIR filter.
    fn iir_fir(&mut self, out: &mut [i16], input: &[i16]) {
        let mut buf = vec![0i16; 2 * self.batch_size + ORDER_FIR_12];
        buf[..ORDER_FIR_12].copy_from_slice(&self.s_fir_i16[..ORDER_FIR_12]);
        let index_increment_q16 = self.inv_ratio_q16;
        let mut at_in = 0;
        let mut at_out = 0;
        let mut in_len = input.len();
        let mut n_samples_in;
        loop {
            n_samples_in = in_len.min(self.batch_size);
            up2_hq(
                &mut self.s_iir,
                &mut buf[ORDER_FIR_12..],
                &input[at_in..at_in + n_samples_in],
            );
            let max_index_q16 = lshift(n_samples_in as i32, 16 + 1);
            let mut index_q16 = 0;
            while index_q16 < max_index_q16 {
                let table_index = smulwb(index_q16 & 0xFFFF, 12) as usize;
                let b = &buf[(index_q16 >> 16) as usize..];
                let lo = &RESAMPLER_FRAC_FIR_12[table_index];
                let hi = &RESAMPLER_FRAC_FIR_12[11 - table_index];
                let mut res_q15 = smulbb(i32::from(b[0]), i32::from(lo[0]));
                res_q15 = smlabb(res_q15, i32::from(b[1]), i32::from(lo[1]));
                res_q15 = smlabb(res_q15, i32::from(b[2]), i32::from(lo[2]));
                res_q15 = smlabb(res_q15, i32::from(b[3]), i32::from(lo[3]));
                res_q15 = smlabb(res_q15, i32::from(b[4]), i32::from(hi[3]));
                res_q15 = smlabb(res_q15, i32::from(b[5]), i32::from(hi[2]));
                res_q15 = smlabb(res_q15, i32::from(b[6]), i32::from(hi[1]));
                res_q15 = smlabb(res_q15, i32::from(b[7]), i32::from(hi[0]));
                out[at_out] = sat16(rshift_round(res_q15, 15)) as i16;
                at_out += 1;
                index_q16 += index_increment_q16;
            }
            at_in += n_samples_in;
            in_len -= n_samples_in;
            if in_len > 0 {
                // The filtered signal's tail to the front, for the next batch.
                buf.copy_within(n_samples_in << 1..(n_samples_in << 1) + ORDER_FIR_12, 0);
            } else {
                break;
            }
        }
        self.s_fir_i16[..ORDER_FIR_12]
            .copy_from_slice(&buf[n_samples_in << 1..(n_samples_in << 1) + ORDER_FIR_12]);
    }

    /// `silk_resampler_private_down_FIR`: a second-order AR filter, then FIR
    /// decimation. (Like libopus, a batch loop that leaves one sample stops:
    /// the decoder's frames never do.)
    fn down_fir(&mut self, out: &mut [i16], input: &[i16]) {
        let order = self.fir_order;
        let mut buf = vec![0i32; self.batch_size + order];
        buf[..order].copy_from_slice(&self.s_fir_i32[..order]);
        let coefs = self.coefs;
        let fir = &coefs[2..];
        let index_increment_q16 = self.inv_ratio_q16;
        let mut at_in = 0;
        let mut at_out = 0;
        let mut in_len = input.len();
        let mut n_samples_in;
        loop {
            n_samples_in = in_len.min(self.batch_size);
            ar2(
                &mut self.s_iir,
                &mut buf[order..],
                &input[at_in..at_in + n_samples_in],
                &coefs[..2],
            );
            let max_index_q16 = lshift(n_samples_in as i32, 16);
            let mut index_q16 = 0;
            while index_q16 < max_index_q16 {
                let b = &buf[(index_q16 >> 16) as usize..];
                let c = |i: usize| i32::from(fir[i]);
                let res_q6 = if order == DOWN_ORDER_FIR0 {
                    // 3:4 and 2:3: a polyphase filter, its phase by the
                    // fractional position.
                    let ind = smulwb(index_q16 & 0xFFFF, self.fir_fracs) as usize;
                    let p = &fir[DOWN_ORDER_FIR0 / 2 * ind..];
                    let mut r = smulwb(b[0], i32::from(p[0]));
                    for j in 1..9 {
                        r = smlawb(r, b[j], i32::from(p[j]));
                    }
                    let p = &fir[DOWN_ORDER_FIR0 / 2 * (self.fir_fracs as usize - 1 - ind)..];
                    for j in 0..9 {
                        r = smlawb(r, b[17 - j], i32::from(p[j]));
                    }
                    r
                } else {
                    // 1:2: a symmetric filter of 24 taps.
                    let mut r = smulwb(b[0].wrapping_add(b[23]), c(0));
                    for j in 1..12 {
                        r = smlawb(r, b[j].wrapping_add(b[23 - j]), c(j));
                    }
                    r
                };
                out[at_out] = sat16(rshift_round(res_q6, 6)) as i16;
                at_out += 1;
                index_q16 += index_increment_q16;
            }
            at_in += n_samples_in;
            in_len -= n_samples_in;
            if in_len > 1 {
                buf.copy_within(n_samples_in..n_samples_in + order, 0);
            } else {
                break;
            }
        }
        self.s_fir_i32[..order].copy_from_slice(&buf[n_samples_in..n_samples_in + order]);
    }
}

/// `silk_resampler_private_up2_HQ`: 2x upsampling by two chains of three
/// all-pass sections (state in Q10).
fn up2_hq(s: &mut [i32; 6], out: &mut [i16], input: &[i16]) {
    for (k, &x) in input.iter().enumerate() {
        let in32 = lshift(i32::from(x), 10);
        for (chain, coef) in [(0usize, &UP2_HQ_0), (3, &UP2_HQ_1)] {
            let y = in32.wrapping_sub(s[chain]);
            let xx = smulwb(y, coef[0]);
            let out32_1 = s[chain].wrapping_add(xx);
            s[chain] = in32.wrapping_add(xx);
            let y = out32_1.wrapping_sub(s[chain + 1]);
            let xx = smulwb(y, coef[1]);
            let out32_2 = s[chain + 1].wrapping_add(xx);
            s[chain + 1] = out32_1.wrapping_add(xx);
            let y = out32_2.wrapping_sub(s[chain + 2]);
            let xx = smlawb(y, y, coef[2]);
            let out32_1 = s[chain + 2].wrapping_add(xx);
            s[chain + 2] = out32_2.wrapping_add(xx);
            out[2 * k + usize::from(chain == 3)] = sat16(rshift_round(out32_1, 10)) as i16;
        }
    }
}

/// `silk_resampler_private_AR2`: a second-order AR filter (`a_q14`), out in
/// Q8.
fn ar2(s: &mut [i32; 6], out_q8: &mut [i32], input: &[i16], a_q14: &[i16]) {
    for (k, &x) in input.iter().enumerate() {
        let out32 = s[0].wrapping_add(lshift(i32::from(x), 8));
        out_q8[k] = out32;
        let out32 = lshift(out32, 2);
        s[0] = smlawb(s[1], out32, i32::from(a_q14[0]));
        s[1] = smulwb(out32, i32::from(a_q14[1]));
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
    fn the_rate_ids_and_delays_are_libopuss() {
        assert_eq!(
            [8000, 12000, 16000, 24000, 48000].map(rate_id),
            [0, 1, 2, 3, 4]
        );
        let r = Resampler::new(16000, 48000).expect("16 to 48 kHz");
        assert_eq!((r.function, r.input_delay), (Function::IirFir, 7));
        let r = Resampler::new(8000, 16000).expect("8 to 16 kHz");
        assert_eq!(r.function, Function::Up2Hq);
        let r = Resampler::new(16000, 12000).expect("16 to 12 kHz");
        assert_eq!((r.function, r.fir_fracs), (Function::DownFir, 3));
        assert!(Resampler::new(44100, 48000).is_none());
    }

    #[test]
    fn a_copy_delays_by_its_matrix_entry() {
        // 12 kHz to 12 kHz: copied, nine samples late.
        let mut r = Resampler::new(12000, 12000).expect("12 to 12 kHz");
        let input: Vec<i16> = (1..=120).collect();
        let mut out = vec![0i16; 120];
        r.process(&mut out, &input);
        assert_eq!(&out[..9], &[0; 9]);
        assert_eq!(&out[9..], &input[..111]);
    }
}
