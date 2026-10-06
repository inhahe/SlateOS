//! A decoded sound made the kernel mixer's stream: two channels, 48 kHz,
//! signed 16-bit, at the volume asked ([`to_native`]).
//!
//! # Channels
//!
//! One channel plays from both speakers at its own level. Two are left and
//! right. More are folded to two by where each speaker stands
//! ([`ChannelOrder`] says, since WAV and Vorbis order their channels
//! differently): the front pair to their sides, the centre to both at -3 dB,
//! the surround channels to their side at -3 dB, a back centre to both at
//! -6 dB, and the low-frequency channel not at all -- a system sound has
//! nothing in it a desk's speakers would miss. The sum is then scaled so
//! that every channel at full scale together reaches full scale and no
//! further: a fold that clipped would add distortion the file never had.
//!
//! # Rate
//!
//! A windowed-sinc filter -- a Kaiser window, [`ZERO_CROSSINGS`] zero
//! crossings each side, about 90 dB of stopband -- its cutoff just under the
//! lower of the two Nyquist frequencies, so nothing above what 48 kHz can
//! hold folds back as an alias when a higher rate comes down. The kernel is
//! tabulated once and read with linear interpolation; each output sample is
//! divided by the sum of the weights it used, so a constant stays exactly
//! constant, at the ends too, where the kernel runs off the sound.
//!
//! # Samples
//!
//! Scaled by the volume, then rounded to 16 bits with triangular dither --
//! two uniform random values a sample, together a least significant bit
//! either way -- so a quiet tail decays into noise rather than into the
//! steps of a staircase. The noise is seeded, so a sound renders the same
//! every time.

use crate::{Audio, MAX_SECONDS};
use std::sync::OnceLock;

/// The mixer's rate.
pub const NATIVE_RATE: u32 = crate::pcm::RATE;

/// Which channel is which past the first two: WAV and Vorbis lay five
/// channels out differently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChannelOrder {
    /// WAV's and SMPTE's: front left, front right, centre, low frequency,
    /// back left, back right, side left, side right.
    #[default]
    Wav,
    /// Vorbis's, and Opus's with mapping family 1: for five channels front
    /// left, centre, front right, back left, back right.
    Vorbis,
    /// No layout the file defines -- Opus's mapping families 2, 3 and 255:
    /// past two channels, every channel folds evenly into both sides.
    Unknown,
}

/// Where a channel's speaker stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Speaker {
    FrontLeft,
    FrontRight,
    Centre,
    LowFrequency,
    BackLeft,
    BackRight,
    SideLeft,
    SideRight,
    BackCentre,
}

use Speaker::{
    BackCentre as Bc, BackLeft as Bl, BackRight as Br, Centre as C, FrontLeft as Fl,
    FrontRight as Fr, LowFrequency as Lfe, SideLeft as Sl, SideRight as Sr,
};

/// The speakers of `channels` channels in `order`, for three to eight.
fn speakers(order: ChannelOrder, channels: u16) -> Option<&'static [Speaker]> {
    Some(match (order, channels) {
        (ChannelOrder::Wav, 3) => &[Fl, Fr, C],
        (ChannelOrder::Wav, 4) => &[Fl, Fr, Bl, Br],
        (ChannelOrder::Wav, 5) => &[Fl, Fr, C, Bl, Br],
        (ChannelOrder::Wav, 6) => &[Fl, Fr, C, Lfe, Bl, Br],
        (ChannelOrder::Wav, 7) => &[Fl, Fr, C, Lfe, Bc, Sl, Sr],
        (ChannelOrder::Wav, 8) => &[Fl, Fr, C, Lfe, Bl, Br, Sl, Sr],
        (ChannelOrder::Vorbis, 3) => &[Fl, C, Fr],
        (ChannelOrder::Vorbis, 4) => &[Fl, Fr, Bl, Br],
        (ChannelOrder::Vorbis, 5) => &[Fl, C, Fr, Bl, Br],
        (ChannelOrder::Vorbis, 6) => &[Fl, C, Fr, Bl, Br, Lfe],
        (ChannelOrder::Vorbis, 7) => &[Fl, C, Fr, Sl, Sr, Bc, Lfe],
        (ChannelOrder::Vorbis, 8) => &[Fl, C, Fr, Sl, Sr, Bl, Br, Lfe],
        _ => return None,
    })
}

/// -3 dB.
const HALF_POWER: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// How much of a speaker goes to the left and to the right.
fn gains(speaker: Speaker) -> (f32, f32) {
    match speaker {
        Fl => (1.0, 0.0),
        Fr => (0.0, 1.0),
        C => (HALF_POWER, HALF_POWER),
        Bl | Sl => (HALF_POWER, 0.0),
        Br | Sr => (0.0, HALF_POWER),
        Bc => (0.5, 0.5),
        Lfe => (0.0, 0.0),
    }
}

/// `audio` as two channels, interleaved left and right, at its own rate.
///
/// Channel counts past eight have no layout this knows, and are folded
/// evenly into both sides -- heard, if not where they were meant to be.
#[must_use]
pub fn to_stereo(audio: &Audio) -> Vec<f32> {
    let channels = usize::from(audio.channels);
    match channels {
        0 => Vec::new(),
        1 => audio.samples.iter().flat_map(|&s| [s, s]).collect(),
        2 => {
            let mut out = audio.samples.clone();
            // A last frame cut short of its second sample is dropped.
            out.truncate(audio.samples.len() & !1);
            out
        }
        _ => {
            let weights: Vec<(f32, f32)> = speakers(audio.order, audio.channels).map_or_else(
                || vec![(1.0, 1.0); channels],
                |layout| layout.iter().map(|&sp| gains(sp)).collect(),
            );
            let left: f32 = weights.iter().map(|w| w.0).sum();
            let right: f32 = weights.iter().map(|w| w.1).sum();
            let scale = 1.0 / left.max(right).max(1.0);
            audio
                .samples
                .chunks_exact(channels)
                .flat_map(|frame| {
                    let (l, r) = frame
                        .iter()
                        .zip(&weights)
                        .fold((0.0f32, 0.0f32), |(l, r), (&s, &(wl, wr))| {
                            (l + s * wl, r + s * wr)
                        });
                    [l * scale, r * scale]
                })
                .collect()
        }
    }
}

/// Zero crossings of the sinc each side of its centre, at the cutoff.
pub const ZERO_CROSSINGS: usize = 16;

/// Table entries a zero crossing.
const TABLE_STEPS: usize = 512;

/// The Kaiser window's shape: about 90 dB of stopband.
const KAISER_BETA: f64 = 8.6;

/// The cutoff, as a fraction of the lower Nyquist frequency: just under it,
/// so the filter's transition is past by the time aliasing would begin.
const ROLL_OFF: f64 = 0.97;

/// The zeroth-order modified Bessel function of the first kind, the Kaiser
/// window's: its power series, to the precision of an `f64`.
fn bessel_i0(x: f64) -> f64 {
    let half = x / 2.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..64u32 {
        let k = f64::from(k);
        term *= (half / k) * (half / k);
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// The windowed sinc, tabulated: entry `i` is the kernel at `i /
/// TABLE_STEPS` zero crossings from its centre, out to [`ZERO_CROSSINGS`].
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "indices of a table of 8193, far inside f64's exact range; the values are within -1..=1"
)]
fn kernel() -> &'static [f32] {
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let len = ZERO_CROSSINGS.saturating_mul(TABLE_STEPS).saturating_add(1);
        let width = ZERO_CROSSINGS as f64;
        let norm = bessel_i0(KAISER_BETA);
        (0..len)
            .map(|i| {
                let d = i as f64 / TABLE_STEPS as f64;
                let sinc = if d == 0.0 {
                    1.0
                } else {
                    let x = std::f64::consts::PI * d;
                    x.sin() / x
                };
                let r = (d / width).min(1.0);
                let window = bessel_i0(KAISER_BETA * (1.0 - r * r).max(0.0).sqrt()) / norm;
                (sinc * window) as f32
            })
            .collect()
    })
}

/// The kernel `d` zero crossings from its centre, read from the table with
/// linear interpolation; nought past its end.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "d is finite and non-negative, and the index is checked against the table"
)]
fn kernel_at(table: &[f32], d: f64) -> f32 {
    let pos = d * TABLE_STEPS as f64;
    let i = pos.floor() as usize;
    let frac = (pos - pos.floor()) as f32;
    match (table.get(i), table.get(i.saturating_add(1))) {
        (Some(&a), Some(&b)) => a + (b - a) * frac,
        (Some(&a), None) => a,
        _ => 0.0,
    }
}

/// `stereo` -- interleaved left and right at `from` frames a second -- at
/// `to` frames a second. See the module's "Rate".
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "frame positions are far below 2^52, and every conversion to an index is floored and range-checked"
)]
pub fn resample(stereo: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || from == 0 || to == 0 {
        return stereo.to_vec();
    }
    let frames_in = stereo.len() / 2;
    // ceil(frames_in * to / from), in u64 so a long sound cannot overflow.
    let frames_out = u64::try_from(frames_in)
        .unwrap_or(u64::MAX)
        .saturating_mul(u64::from(to))
        .div_ceil(u64::from(from));
    let frames_out = usize::try_from(frames_out).unwrap_or(usize::MAX);
    let step = f64::from(from) / f64::from(to);
    let cutoff = (f64::from(to) / f64::from(from)).min(1.0) * ROLL_OFF;
    let half_width = ZERO_CROSSINGS as f64 / cutoff;
    let table = kernel();
    let mut out = Vec::with_capacity(frames_out.saturating_mul(2));
    for j in 0..frames_out {
        let x = j as f64 * step;
        let first = (x - half_width).ceil().max(0.0) as usize;
        let last = ((x + half_width).floor() as usize).min(frames_in.saturating_sub(1));
        let (mut left, mut right, mut total) = (0.0f64, 0.0f64, 0.0f64);
        for n in first..=last {
            let w = f64::from(kernel_at(table, (x - n as f64).abs() * cutoff));
            if let (Some(&l), Some(&r)) = (
                stereo.get(n.saturating_mul(2)),
                stereo.get(n.saturating_mul(2).saturating_add(1)),
            ) {
                left += f64::from(l) * w;
                right += f64::from(r) * w;
                total += w;
            }
        }
        if total.abs() > 1e-9 {
            out.push((left / total) as f32);
            out.push((right / total) as f32);
        } else {
            out.push(0.0);
            out.push(0.0);
        }
    }
    out
}

/// A small, seeded source of noise for the dither: xorshift32.
#[derive(Clone, Copy, Debug)]
struct Noise(u32);

impl Noise {
    /// A uniform value in 0..1: the top 24 bits of the next state, which
    /// an `f32` holds exactly.
    #[allow(clippy::cast_precision_loss, reason = "24 bits fit an f32's mantissa")]
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x.wrapping_shl(13);
        x ^= x.wrapping_shr(17);
        x ^= x.wrapping_shl(5);
        self.0 = x;
        x.wrapping_shr(8) as f32 / 16_777_216.0
    }
}

/// The dither's seed: any odd constant -- xorshift must not start at nought.
const DITHER_SEED: u32 = 0x2545_F491;

/// `stereo`, scaled by `gain`, rounded to 16-bit samples with triangular
/// dither, and clipped to their range.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "rounded and clamped to i16's range first"
)]
pub fn quantize(stereo: &[f32], gain: f32) -> Vec<i16> {
    let gain = if gain.is_finite() {
        gain.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut noise = Noise(DITHER_SEED);
    stereo
        .iter()
        .map(|&s| {
            let s = if s.is_finite() { s } else { 0.0 };
            let dither = noise.next() + noise.next() - 1.0;
            let v = (s * gain * 32767.0 + dither).round();
            v.clamp(-32768.0, 32767.0) as i16
        })
        .collect()
}

/// `audio` as two channels at its own rate, cut at [`MAX_SECONDS`] -- before
/// resampling, so a long sound costs no more than the most that plays.
#[must_use]
pub fn stereo_within_limit(audio: &Audio) -> Vec<f32> {
    let mut stereo = to_stereo(audio);
    let longest = usize::try_from(audio.rate)
        .unwrap_or(usize::MAX)
        .saturating_mul(usize::try_from(MAX_SECONDS).unwrap_or(usize::MAX))
        .saturating_mul(2);
    stereo.truncate(longest);
    stereo
}

/// `audio` as the kernel mixer plays it: two channels, 48 kHz, 16-bit, at
/// `volume` (0 to 1), no longer than [`MAX_SECONDS`].
#[must_use]
pub fn to_native(audio: &Audio, volume: f32) -> Vec<i16> {
    quantize(
        &resample(&stereo_within_limit(audio), audio.rate, NATIVE_RATE),
        volume,
    )
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
