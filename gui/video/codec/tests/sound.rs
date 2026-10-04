//! The sound fixtures (`tests/data/opus_*`) played through
//! [`videocodec::Sound`]: every block's time and length held to FFmpeg's
//! (`ffprobe -show_frames`), and every sample to libopus's fixed-point
//! decoder's with FFmpeg's trimming (`tests/data/generate_sound_fixtures.py`,
//! which says where each answer comes from).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "a test: a failure should be loud, and its sizes are small"
)]

use std::fs::File;
use std::path::PathBuf;

use videocodec::{Error, Sound, SoundCodec};

const FIXTURES: [&str; 4] = [
    "opus_stereo.webm",
    "opus_mono_voip.webm",
    "opus_short_frames.mka",
    "opus_51.webm",
];

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

/// A fixture's answer: `NAME.sound.txt`.
struct Expected {
    track: u64,
    channels: usize,
    /// Each block's time (ns) and samples a channel.
    blocks: Vec<(i64, usize)>,
    bytes: usize,
    digest: u64,
}

fn expected(name: &str) -> Expected {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let text = std::fs::read_to_string(data(&format!("{stem}.sound.txt"))).expect("the answer");
    let mut e = Expected {
        track: 0,
        channels: 0,
        blocks: Vec::new(),
        bytes: 0,
        digest: 0,
    };
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f[0] {
            "track" => {
                e.track = f[1].parse().unwrap();
                e.channels = f[3].parse().unwrap();
            }
            "block" => e
                .blocks
                .push((f[1].parse().unwrap(), f[2].parse().unwrap())),
            "pcm" => {
                e.bytes = f[1].parse().unwrap();
                e.digest = u64::from_str_radix(f[2], 16).unwrap();
            }
            other => panic!("{name}: a line {other:?}"),
        }
    }
    e
}

fn fnv1a64(data: &[u8]) -> u64 {
    data.iter().fold(0xCBF2_9CE4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

/// Every block of a file's sound.
fn blocks(name: &str) -> Vec<videocodec::Block> {
    let mut sound = Sound::open(File::open(data(name)).unwrap()).unwrap();
    let mut out = Vec::new();
    while let Some(b) = sound.next_block().unwrap() {
        out.push(b);
    }
    assert_eq!(sound.damaged(), 0, "{name}: no packet is damaged");
    out
}

#[test]
fn every_fixture_plays_as_ffmpeg_and_libopus_play_it() {
    for name in FIXTURES {
        let e = expected(name);
        let sound = Sound::open(File::open(data(name)).unwrap()).unwrap();
        let info = sound.info();
        assert_eq!(
            (info.track, info.channels, info.sample_rate, info.codec),
            (e.track, e.channels, 48000, SoundCodec::Opus),
            "{name}: the track"
        );
        let got = blocks(name);
        let times: Vec<(i64, usize)> = got
            .iter()
            .map(|b| (b.time, b.samples.len() / e.channels))
            .collect();
        assert_eq!(times, e.blocks, "{name}: the blocks, as FFmpeg times them");
        let pcm: Vec<u8> = got
            .iter()
            .flat_map(|b| b.samples.iter().flat_map(|s| s.to_le_bytes()))
            .collect();
        assert_eq!(
            (pcm.len(), fnv1a64(&pcm)),
            (e.bytes, e.digest),
            "{name}: the samples, as libopus decodes them"
        );
    }
}

#[test]
fn a_seek_to_the_start_plays_the_file_from_its_first_sample() {
    // The pre-roll reaches back past the first packet; the samples before
    // 0 -- the pre-skip -- are dropped by their times, as the whole file's
    // are by the codec delay: the same samples, the same times.
    for name in FIXTURES {
        let mut sound = Sound::open(File::open(data(name)).unwrap()).unwrap();
        sound.seek(0).unwrap();
        let mut again = Vec::new();
        while let Some(b) = sound.next_block().unwrap() {
            again.push(b);
        }
        assert_eq!(again, blocks(name), "{name}");
    }
}

/// How far `ours` is from `theirs`, in dB of signal to error (110 for none).
fn snr(ours: &[i16], theirs: &[i16]) -> f64 {
    let (mut err, mut sig) = (0f64, 0f64);
    for (&a, &b) in ours.iter().zip(theirs) {
        err += (f64::from(a) - f64::from(b)).powi(2);
        sig += f64::from(b).powi(2);
    }
    10.0 * (sig.max(1.0) / err.max(1.0)).log10()
}

#[test]
fn a_seek_starts_at_the_first_sample_at_or_after_its_time() {
    // "At or after" by the times the whole file's blocks carry: the block
    // whose time is the last at or before `t`, from its sample
    // ceil((t - time) * 48000 / 10^9) on -- or, past its end, the next block
    // whole -- and every block after it the whole file's, time and length.
    //
    // The samples: after a seek the decoder has had its pre-roll (80 ms),
    // not the whole file, and starts off by what it never heard, then
    // converges on the whole decode. CELT predicts each frame's band
    // energies from the last's: measured, some 20 dB of signal to error at
    // the first block after the pre-roll, 5 to 6 dB more each 20 ms, past
    // 60 dB within eight. SILK's pitch predictor feeds back the excitation
    // it decoded before: on this voiced signal, 6 dB at first, 40 to 50 dB
    // two packets on, then the same sound a few tens of dB from the whole
    // decode's waveform for as long as the voicing lasts. Held to: within
    // 30 dB inside ten blocks -- a block out of place is near 0. (A seek
    // that reaches back to the stream's start decodes what opening it
    // decodes, to the bit.)
    for name in ["opus_stereo.webm", "opus_mono_voip.webm", "opus_51.webm"] {
        let channels = expected(name).channels;
        let whole = blocks(name);
        for t in [1i64, 480_000_000, 1_000_000_001, 1_100_250_000] {
            let i = whole.iter().rposition(|b| b.time <= t).unwrap();
            let b = &whole[i];
            let k = usize::try_from(
                (u128::try_from(t - b.time).unwrap() * 48000).div_ceil(1_000_000_000),
            )
            .unwrap();
            let (theirs, mut next) = if k * channels < b.samples.len() {
                (b.samples[k * channels..].to_vec(), i + 1)
            } else {
                (whole[i + 1].samples.clone(), i + 2)
            };
            let mut sound = Sound::open(File::open(data(name)).unwrap()).unwrap();
            sound.seek(t).unwrap();
            let first = sound.next_block().unwrap().expect("a block after the seek");
            assert!(
                (first.time - t).abs() < 1_000_000,
                "{name} at {t}: the block is at {}",
                first.time
            );
            assert_eq!(
                first.samples.len(),
                theirs.len(),
                "{name} at {t}: the block's length"
            );
            let mut settled = snr(&first.samples, &theirs) >= 30.0;
            for _ in 0..10 {
                let (Some(ours), Some(w)) = (sound.next_block().unwrap(), whole.get(next)) else {
                    break;
                };
                assert_eq!(
                    (ours.time, ours.samples.len()),
                    (w.time, w.samples.len()),
                    "{name} at {t}: block {next}"
                );
                settled |= snr(&ours.samples, &w.samples) >= 30.0;
                next += 1;
            }
            assert!(settled, "{name} at {t}: not within 30 dB inside ten blocks");
        }
    }
}

#[test]
fn a_file_without_sound_says_so() {
    // A VP8 file with no sound track, among the video fixtures.
    assert_eq!(
        Sound::open(File::open(data("vp8_sd.webm")).unwrap()).err(),
        Some(Error::NoSound)
    );
}
