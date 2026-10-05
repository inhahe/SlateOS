//! The sound fixtures (`tests/data/opus_*`, `tests/data/vorbis_*`, in
//! Matroska, WebM, MP4 and Ogg) played through [`videocodec::Sound`]: every
//! block's time and length held to FFmpeg's (`ffprobe -show_frames`) -- but
//! where FFmpeg's Ogg demuxer is wrong (a Vorbis packet it mistimes, a
//! one-page Vorbis stream, a chained file's restarting times), there to
//! Tremor and the Vorbis I specification -- and every sample to libopus's
//! fixed-point decoder's or Tremor's with FFmpeg's trimming
//! (`tests/data/generate_sound_fixtures.py`, which says where each answer
//! comes from).

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
use std::io::Cursor;
use std::path::PathBuf;

use videocodec::{Error, Sound, SoundCodec};

const FIXTURES: [&str; 28] = [
    "opus_stereo.webm",
    "opus_mono_voip.webm",
    "opus_short_frames.mka",
    "opus_51.webm",
    "vorbis_stereo.webm",
    "vorbis_mono_22k.mka",
    "vorbis_51.webm",
    "opus_mp4_stereo.mp4",
    "opus_mp4_no_edit_list.mp4",
    "opus_mp4_51.mp4",
    "opus_mp4_long_priming.mp4",
    "opus_ogg_stereo.opus",
    "opus_ogg_51.opus",
    "vorbis_ogg_stereo.ogg",
    "vorbis_ogg_mono_22k.ogg",
    "vorbis_ogg_one_page.ogg",
    "opus_ogg_chained.opus",
    "vorbis_ogg_chained.ogg",
    "flac_stereo.flac",
    "flac_24bit.flac",
    "flac_51.mka",
    "flac_mp4.mp4",
    "flac_ogg.oga",
    "mp3_tagged_stereo.mp3",
    "mp3_vbr_mono_22k.mp3",
    "mp2_stereo.mp2",
    "mp3_mka.mka",
    "mp3_mp4.mp4",
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
    rate: u32,
    /// Bits a sample: 16 where the answer does not say.
    bits: u32,
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
        rate: 0,
        bits: 16,
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
                e.rate = f[5].parse().unwrap();
                if f.get(6) == Some(&"bits") {
                    e.bits = f[7].parse().unwrap();
                }
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

/// The codec a fixture's name says it is.
fn codec(name: &str) -> SoundCodec {
    if name.starts_with("opus") {
        SoundCodec::Opus
    } else if name.starts_with("flac") {
        SoundCodec::Flac
    } else if name.starts_with("mp3") || name.starts_with("mp2") {
        SoundCodec::Mp3
    } else {
        SoundCodec::Vorbis
    }
}

/// The blocks' samples as the answers digest them: each little-endian, in
/// as many bytes as its depth needs (FLAC's MD5's layout).
fn pcm_bytes(blocks: &[videocodec::Block], bits: u32) -> Vec<u8> {
    let width = bits.div_ceil(8) as usize;
    blocks
        .iter()
        .flat_map(|b| {
            b.samples
                .iter()
                .flat_map(move |s| s.to_le_bytes().into_iter().take(width))
        })
        .collect()
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
fn every_fixture_plays_as_ffmpeg_and_its_reference_decoder_play_it() {
    for name in FIXTURES {
        let e = expected(name);
        let sound = Sound::open(File::open(data(name)).unwrap()).unwrap();
        let info = sound.info();
        assert_eq!(
            (
                info.track,
                info.channels,
                info.sample_rate,
                info.codec,
                info.bits_per_sample
            ),
            (e.track, e.channels, e.rate, codec(name), e.bits),
            "{name}: the track"
        );
        let got = blocks(name);
        let times: Vec<(i64, usize)> = got
            .iter()
            .map(|b| (b.time, b.samples.len() / e.channels))
            .collect();
        assert_eq!(times, e.blocks, "{name}: the blocks, as FFmpeg times them");
        let pcm = pcm_bytes(&got, e.bits);
        assert_eq!(
            (pcm.len(), fnv1a64(&pcm)),
            (e.bytes, e.digest),
            "{name}: the samples, as libopus, Tremor, libFLAC or minimp3 decodes them"
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
fn snr(ours: &[i32], theirs: &[i32]) -> f64 {
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
    // ceil((t - time) * rate / 10^9) on -- or, past its end, the next block
    // whole -- and every block after it the whole file's, time and length.
    //
    // The samples: after a seek an Opus decoder has had its pre-roll (80
    // ms), not the whole file, and starts off by what it never heard, then
    // converges on the whole decode. CELT predicts each frame's band
    // energies from the last's: measured, some 20 dB of signal to error at
    // the first block after the pre-roll, 5 to 6 dB more each 20 ms, past
    // 60 dB within eight. SILK's pitch predictor feeds back the excitation
    // it decoded before: on this voiced signal, 6 dB at first, 40 to 50 dB
    // two packets on, then the same sound a few tens of dB from the whole
    // decode's waveform for as long as the voicing lasts. Held to: within
    // 30 dB inside ten blocks -- a block out of place is near 0. A Vorbis
    // decoder remembers nothing but the last block, which the pre-roll
    // decodes: its blocks are the whole decode's, to the bit, from the
    // first; a FLAC frame stands alone, and a `.flac` file's reader seeks to
    // the sample; an MPEG audio decoder needs the frames whose main data the
    // bit reservoir holds, which the seek goes back for, and then gives the
    // whole decode's samples to the bit. (A seek that reaches back to the
    // stream's start decodes what opening it decodes, to the bit.)
    for name in [
        "opus_stereo.webm",
        "opus_mono_voip.webm",
        "opus_51.webm",
        "vorbis_stereo.webm",
        "vorbis_mono_22k.mka",
        "vorbis_51.webm",
        "opus_mp4_stereo.mp4",
        "opus_mp4_no_edit_list.mp4",
        "opus_mp4_long_priming.mp4",
        "opus_ogg_stereo.opus",
        "vorbis_ogg_stereo.ogg",
        "vorbis_ogg_mono_22k.ogg",
        "opus_ogg_chained.opus",
        "vorbis_ogg_chained.ogg",
        "flac_stereo.flac",
        "flac_24bit.flac",
        "flac_51.mka",
        "flac_mp4.mp4",
        "flac_ogg.oga",
        "mp3_tagged_stereo.mp3",
        "mp3_vbr_mono_22k.mp3",
        "mp2_stereo.mp2",
        "mp3_mka.mka",
        "mp3_mp4.mp4",
    ] {
        let e = expected(name);
        let channels = e.channels;
        let whole = blocks(name);
        // From the first block's time: a file whose decoder drops its own
        // pre-skip starts later than 0 (an MP4 with no edit list).
        let start = whole[0].time;
        let span = whole.last().unwrap().time - start;
        for t in [1, span * 3 / 10, span / 2 + 1, span * 2 / 3 + 250_000].map(|d| start + d) {
            let i = whole.iter().rposition(|b| b.time <= t).unwrap();
            let b = &whole[i];
            let k = usize::try_from(
                (u128::try_from(t - b.time).unwrap() * u128::from(e.rate)).div_ceil(1_000_000_000),
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
            if codec(name) != SoundCodec::Opus {
                assert_eq!(
                    first.samples, theirs,
                    "{name} at {t}: the first block's samples"
                );
            }
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
                if codec(name) != SoundCodec::Opus {
                    assert_eq!(
                        ours.samples, w.samples,
                        "{name} at {t}: block {next}'s samples"
                    );
                }
                settled |= snr(&ours.samples, &w.samples) >= 30.0;
                next += 1;
            }
            assert!(settled, "{name} at {t}: not within 30 dB inside ten blocks");
        }
    }
}

#[test]
fn a_damaged_vorbis_packet_is_concealed_with_silence() {
    // The 21st packet's first bit set, which makes it no audio packet: its
    // block is silence as long as the last block, at its time; the next
    // overlaps the block before the damaged one (so differs), and every
    // block after that is the whole decode's again.
    let name = "vorbis_51.webm";
    let mut file = std::fs::read(data(name)).unwrap();
    let mut demuxer = matroska::Demuxer::open(Cursor::new(&file)).unwrap();
    let mut packets = Vec::new();
    while let Some(p) = demuxer.next_packet().unwrap() {
        packets.push(p);
    }
    let p = &packets[20];
    let head = &p.data[..p.data.len().min(16)];
    let at = (usize::try_from(p.position).unwrap()..file.len())
        .find(|&i| file[i..].starts_with(head))
        .unwrap();
    file[at] |= 1;
    let whole = blocks(name);
    let mut sound = Sound::open(Cursor::new(file)).unwrap();
    let mut got = Vec::new();
    while let Some(b) = sound.next_block().unwrap() {
        got.push(b);
    }
    assert_eq!(sound.damaged(), 1);
    assert_eq!(
        sound.last_damage(),
        Some(&Error::Vorbis(vorbis::Error::NotAudio))
    );
    // The first packet makes no block: packet 20's is block 19.
    let i = 19;
    assert_eq!(got.len(), whole.len());
    assert_eq!(got[..i], whole[..i], "the blocks before");
    assert_eq!(got[i].time, whole[i].time);
    assert_eq!(
        got[i].samples.len(),
        whole[i - 1].samples.len(),
        "as long as the last block"
    );
    assert!(got[i].samples.iter().all(|&s| s == 0), "silence");
    assert_eq!(got[i + 1].time, whole[i + 1].time);
    assert_eq!(got[i + 2..], whole[i + 2..], "the blocks after the next");
}

#[test]
fn a_file_without_sound_says_so() {
    // A VP8 file with no sound track, among the video fixtures.
    assert_eq!(
        Sound::open(File::open(data("vp8_sd.webm")).unwrap()).err(),
        Some(Error::NoSound)
    );
}
