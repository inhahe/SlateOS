#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;
use std::f64::consts::TAU;

fn audio(rate: u32, channels: u16, samples: Vec<f32>, order: ChannelOrder) -> Audio {
    Audio {
        rate,
        channels,
        samples,
        order,
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-6
}

/// **One channel plays from both sides at its own level; two are left and
/// right**, and a half frame at the end of two is dropped.
#[test]
fn one_channel_spreads_and_two_pass() {
    let mono = audio(8000, 1, vec![0.5, -0.25], ChannelOrder::Wav);
    assert_eq!(to_stereo(&mono), [0.5, 0.5, -0.25, -0.25]);
    let stereo = audio(8000, 2, vec![0.1, 0.2, 0.3, 0.4, 0.5], ChannelOrder::Wav);
    assert_eq!(to_stereo(&stereo), [0.1, 0.2, 0.3, 0.4]);
    assert!(to_stereo(&audio(8000, 0, vec![], ChannelOrder::Wav)).is_empty());
}

/// The fold of one frame where only channel `lit` sounds, at full scale.
fn fold_one(order: ChannelOrder, channels: u16, lit: usize) -> (f32, f32) {
    let mut frame = vec![0.0; usize::from(channels)];
    frame[lit] = 1.0;
    let out = to_stereo(&audio(48_000, channels, frame, order));
    (out[0], out[1])
}

/// **5.1 folds by where each speaker stands**: the front pair to their
/// sides, the centre and the surrounds at -3 dB, the low-frequency channel
/// not at all -- all scaled so every channel at once just reaches full
/// scale. WAV puts the centre third, Vorbis second.
#[test]
fn surround_folds_by_where_each_speaker_stands() {
    let scale = 1.0 / (1.0 + 2.0 * HALF_POWER);
    // WAV: FL FR C LFE BL BR.
    assert!(close(fold_one(ChannelOrder::Wav, 6, 0).0, scale));
    assert!(close(fold_one(ChannelOrder::Wav, 6, 0).1, 0.0));
    assert!(close(fold_one(ChannelOrder::Wav, 6, 1).1, scale));
    let (l, r) = fold_one(ChannelOrder::Wav, 6, 2);
    assert!(
        close(l, HALF_POWER * scale) && close(r, HALF_POWER * scale),
        "centre"
    );
    assert_eq!(fold_one(ChannelOrder::Wav, 6, 3), (0.0, 0.0), "LFE");
    assert!(close(
        fold_one(ChannelOrder::Wav, 6, 4).0,
        HALF_POWER * scale
    ));
    assert!(close(
        fold_one(ChannelOrder::Wav, 6, 5).1,
        HALF_POWER * scale
    ));
    // Vorbis: FL C FR BL BR LFE.
    let (l, r) = fold_one(ChannelOrder::Vorbis, 6, 1);
    assert!(
        close(l, HALF_POWER * scale) && close(r, HALF_POWER * scale),
        "centre"
    );
    assert!(
        close(fold_one(ChannelOrder::Vorbis, 6, 2).1, scale),
        "front right"
    );
    assert_eq!(fold_one(ChannelOrder::Vorbis, 6, 5), (0.0, 0.0), "LFE last");
}

/// **No fold clips**: every channel at full scale at once reaches full
/// scale and no further, for every layout this knows and some it does not.
#[test]
fn no_fold_clips() {
    for order in [
        ChannelOrder::Wav,
        ChannelOrder::Vorbis,
        ChannelOrder::Unknown,
    ] {
        for channels in 3..=10u16 {
            let all = audio(48_000, channels, vec![1.0; usize::from(channels)], order);
            let out = to_stereo(&all);
            assert!(
                out[0] <= 1.0 + 1e-6 && out[1] <= 1.0 + 1e-6,
                "{order:?} {channels}: {out:?}"
            );
            assert!(
                out[0] > 0.5 && out[1] > 0.5,
                "{order:?} {channels}: {out:?}"
            );
        }
    }
}

/// **A layout nobody defines folds evenly**: every channel the same to
/// both sides.
#[test]
fn an_unknown_layout_folds_evenly() {
    for lit in 0..4 {
        let (l, r) = fold_one(ChannelOrder::Unknown, 4, lit);
        assert!(close(l, 0.25) && close(r, 0.25), "{lit}: {l} {r}");
    }
}

/// A stereo sine of `freq` Hz at `rate`, `frames` long, both sides alike.
fn sine(freq: f64, rate: u32, frames: usize, level: f64) -> Vec<f32> {
    (0..frames)
        .flat_map(|i| {
            let v = (level * (TAU * freq * i as f64 / f64::from(rate)).sin()) as f32;
            [v, v]
        })
        .collect()
}

/// The left channel's level at `freq` Hz, by correlating with a sine and
/// a cosine there -- one bin of a DFT, over the middle of the sound, away
/// from its ends.
fn level_at(stereo: &[f32], rate: u32, freq: f64) -> f64 {
    let left: Vec<f64> = stereo.iter().step_by(2).map(|&s| f64::from(s)).collect();
    let (from, to) = (left.len() / 4, left.len() * 3 / 4);
    let (mut s, mut c) = (0.0, 0.0);
    for (i, v) in left.iter().enumerate().take(to).skip(from) {
        let phase = TAU * freq * i as f64 / f64::from(rate);
        s += v * phase.sin();
        c += v * phase.cos();
    }
    2.0 * (s * s + c * c).sqrt() / (to - from) as f64
}

/// **Resampling keeps a tone's pitch and level**: 1 kHz at 44.1 kHz is 1 kHz
/// at 48 kHz, at the level it had, and the length is what the rates make
/// it.
#[test]
fn resampling_keeps_a_tones_pitch_and_level() {
    let input = sine(1000.0, 44_100, 44_100, 0.5);
    let out = resample(&input, 44_100, 48_000);
    assert_eq!(out.len(), 2 * 48_000);
    let level = level_at(&out, 48_000, 1000.0);
    assert!((level - 0.5).abs() < 0.005, "level {level}");
    // Not at a neighbouring pitch: what 44.1 played as 48 would be.
    assert!(level_at(&out, 48_000, 1088.4) < 0.01);
}

/// **The length is the rates' ratio, rounded up**, for rates that do not
/// divide.
#[test]
fn the_length_is_the_rates_ratio_rounded_up() {
    let out = resample(&vec![0.0; 2 * 1001], 22_050, 48_000);
    // 1001 * 48000 / 22050 = 2179.04...
    assert_eq!(out.len(), 2 * 2180);
    let same = vec![0.25; 10];
    assert_eq!(
        resample(&same, 48_000, 48_000),
        same,
        "the same rate, untouched"
    );
}

/// **A constant stays exactly constant**, at the ends too, where the
/// kernel runs off the sound: each sample is its weights' average.
#[test]
fn a_constant_stays_constant_to_the_ends() {
    for (from, to) in [(44_100, 48_000), (96_000, 48_000), (8000, 48_000)] {
        let out = resample(&vec![0.3; 2 * 500], from, to);
        assert!(
            out.iter().all(|&s| (s - 0.3).abs() < 1e-5),
            "{from} -> {to}"
        );
    }
}

/// **Coming down, what the lower rate cannot hold is filtered, not
/// folded**: 30 kHz at 96 kHz would alias to 18 kHz at 48; it comes out
/// below -60 dB.
#[test]
fn coming_down_does_not_alias() {
    let input = sine(30_000.0, 96_000, 48_000, 0.5);
    let out = resample(&input, 96_000, 48_000);
    assert!(level_at(&out, 48_000, 18_000.0) < 0.5e-3);
    // A tone the lower rate can hold passes.
    let input = sine(5000.0, 96_000, 48_000, 0.5);
    let out = resample(&input, 96_000, 48_000);
    assert!((level_at(&out, 48_000, 5000.0) - 0.5).abs() < 0.005);
}

/// **Quantizing scales, rounds with dither and clips**: full scale is the
/// top of 16 bits, past it is clipped, silence is within the dither's least
/// bit, a gain past one is one, and a sample that is not a number is
/// silence.
#[test]
fn quantizing_scales_dithers_and_clips() {
    let out = quantize(&[1.0, -1.0, 2.0, -2.0], 1.0);
    assert!(out[0] >= 32_766, "{out:?}");
    assert!(out[1] <= -32_766, "{out:?}");
    assert_eq!(out[2], 32_767, "clipped");
    assert_eq!(out[3], -32_768, "clipped");
    let quiet = quantize(&[0.0; 1000], 1.0);
    assert!(quiet.iter().all(|&s| s.abs() <= 1));
    assert!(quiet.iter().any(|&s| s != 0), "dithered, not truncated");
    let half = quantize(&[0.5; 100], 0.5);
    assert!(
        half.iter().all(|&s| (8190..=8193).contains(&s)),
        "{:?}",
        &half[..4]
    );
    assert_eq!(
        quantize(&[0.5; 10], 3.0),
        quantize(&[0.5; 10], 1.0),
        "gain held to one"
    );
    assert!(
        quantize(&[f32::NAN, f32::INFINITY], 1.0)
            .iter()
            .all(|&s| s.abs() <= 1)
    );
    assert_eq!(
        quantize(&[0.25; 64], 1.0),
        quantize(&[0.25; 64], 1.0),
        "seeded"
    );
}

/// **To the mixer's stream**: two channels at 48 kHz, cut at the longest a
/// sound plays.
#[test]
fn to_native_is_two_channels_at_48k_and_no_longer_than_the_most() {
    let mono = audio(8000, 1, vec![0.1; 8000], ChannelOrder::Wav);
    let native = to_native(&mono, 1.0);
    assert_eq!(native.len(), 2 * 48_000);
    assert!(native[1000..].iter().all(|&s| (3275..=3278).contains(&s)));
    // Cut before resampling: 40 seconds at 100 Hz are 30 at 100 Hz.
    let long = audio(100, 1, vec![0.1; 100 * 40], ChannelOrder::Wav);
    let cut = stereo_within_limit(&long);
    assert_eq!(cut.len(), 2 * 100 * usize::try_from(MAX_SECONDS).unwrap());
    let short = audio(100, 1, vec![0.1; 100], ChannelOrder::Wav);
    assert_eq!(
        stereo_within_limit(&short).len(),
        200,
        "a short sound is not cut"
    );
}
