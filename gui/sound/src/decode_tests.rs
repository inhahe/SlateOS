#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

//! Lane F's fixtures, read where they are: what each holds is written
//! beside it (`gui/video/vorbis/tests/data/references.txt`, and a `.txt` of
//! its packets beside each Ogg fixture).

use super::*;

const VORBIS_MONO: &[u8] = include_bytes!("../../video/vorbis/tests/data/mono_q3.ogg");
const VORBIS_STEREO_22K: &[u8] =
    include_bytes!("../../video/vorbis/tests/data/native_stereo_22k.ogg");
const VORBIS_QUAD: &[u8] = include_bytes!("../../video/vorbis/tests/data/quad.ogg");
const OPUS_51: &[u8] = include_bytes!("../../video/ogg/tests/data/opus_51.opus");
const OPUS_MONO: &[u8] = include_bytes!("../../video/ogg/tests/data/opus_mono_silk.opus");
const FLAC_IN_OGG: &[u8] = include_bytes!("../../video/ogg/tests/data/flac.oga");

/// The stream's length in frames as the demuxer reads it from the last
/// page, to hold the decode to.
fn demuxed_frames(bytes: &[u8]) -> Option<u64> {
    let demuxer = Demuxer::open(Cursor::new(bytes)).unwrap();
    demuxer.duration(0)
}

fn in_range(audio: &Audio) -> bool {
    audio
        .samples
        .iter()
        .all(|s| s.is_finite() && s.abs() <= 2.0)
}

/// **A Vorbis file decodes at its own rate and channels**, in Vorbis's
/// channel order, its frames what its last page says -- no more than the
/// decoder's every sample (`references.txt`: mono_q3 decodes 88 320 a
/// channel at 44.1 kHz).
#[test]
fn a_vorbis_file_decodes_at_its_own_shape() {
    let a = decode(VORBIS_MONO).unwrap();
    assert_eq!(
        (a.rate, a.channels, a.order),
        (44_100, 1, ChannelOrder::Vorbis)
    );
    let frames = u64::try_from(a.frames()).unwrap();
    assert!(frames <= 88_320 && frames > 80_000, "{frames}");
    if let Some(expected) = demuxed_frames(VORBIS_MONO) {
        assert_eq!(frames, expected, "the last page's length");
    }
    assert!(in_range(&a));
    assert!(
        a.samples.iter().any(|&s| s.abs() > 0.01),
        "sound, not silence"
    );

    let a = decode(VORBIS_STEREO_22K).unwrap();
    assert_eq!((a.rate, a.channels), (22_050, 2));
    assert!(u64::try_from(a.frames()).unwrap() <= 33_792);

    let a = decode(VORBIS_QUAD).unwrap();
    assert_eq!(
        (a.rate, a.channels, a.order),
        (44_100, 4, ChannelOrder::Vorbis)
    );
    assert_eq!(a.samples.len() % 4, 0);
}

/// **An Opus file decodes at 48 kHz, its pre-skip and end padding taken
/// off**: the fixture's packets last 58 392 frames less 312 of pre-skip
/// (`opus_51.txt`, its packets' durations having their padding already
/// off) -- exactly 58 080 frames of 5.1, in Vorbis's order (mapping family
/// 1).
#[test]
fn an_opus_file_decodes_at_48k_trimmed() {
    let a = decode(OPUS_51).unwrap();
    assert_eq!(
        (a.rate, a.channels, a.order),
        (48_000, 6, ChannelOrder::Vorbis)
    );
    assert_eq!(a.frames(), 58_080);
    assert!(in_range(&a));

    let a = decode(OPUS_MONO).unwrap();
    assert_eq!((a.rate, a.channels), (48_000, 1));
    assert_eq!(a.frames(), 66_072 - 312);
}

/// **FLAC in Ogg is refused, not half-played**, and what is neither WAV
/// nor Ogg is not taken for either.
#[test]
fn what_is_not_vorbis_opus_or_wav_is_refused() {
    assert_eq!(decode(FLAC_IN_OGG), Err(DecodeError::NoAudioStream));
    assert_eq!(
        decode(b"ID3\x04\0\0\0\0\0\0"),
        Err(DecodeError::Unrecognised)
    );
    assert_eq!(decode(b""), Err(DecodeError::Unrecognised));
}

/// **A file cut short decodes its whole pages, and one cut before any
/// says it has no sound** -- never a panic. A page is taken only whole,
/// with its CRC right (`gui/video/ogg`), and this fixture's pages are
/// large: three quarters of the file hold 59 of its 124 packets, half of
/// it none past the headers.
#[test]
fn a_file_cut_short_decodes_its_whole_pages() {
    let full = decode(VORBIS_MONO).unwrap().frames();
    let three_quarters = &VORBIS_MONO[..VORBIS_MONO.len() * 3 / 4];
    let a = decode(three_quarters).unwrap();
    assert!(
        a.frames() > full / 4 && a.frames() < full,
        "{} of {full}",
        a.frames()
    );
    assert_eq!(
        decode(&VORBIS_MONO[..VORBIS_MONO.len() / 2]),
        Err(DecodeError::Empty)
    );
    for cut in [0, 30, 60, 200, 1000, 4000] {
        let _ = decode(&VORBIS_MONO[..cut]);
        let _ = decode(&OPUS_51[..cut]);
    }
}

/// **Damage costs only what it touches**: bytes flipped in the middle of
/// a file lose that page (its CRC fails) or that packet, not the sound.
#[test]
fn a_damaged_packet_is_passed_over() {
    let mut damaged = VORBIS_MONO.to_vec();
    let mid = damaged.len() / 2;
    for b in &mut damaged[mid..mid + 64] {
        *b ^= 0x5A;
    }
    let a = decode(&damaged).unwrap();
    assert!(a.frames() > 10_000, "the pages past the damage survive");
    assert!(in_range(&a));
    let mut damaged = OPUS_51.to_vec();
    let mid = damaged.len() / 2;
    for b in &mut damaged[mid..mid + 64] {
        *b ^= 0xA5;
    }
    let a = decode(&damaged).unwrap();
    assert!(a.frames() > 10_000);
    assert!(in_range(&a));
}

/// **The decoded sound plays**: through the conversion to the mixer's
/// stream, a 44.1 kHz mono file is 48 kHz stereo of the same length.
#[test]
fn a_decoded_file_converts_to_the_mixers_stream() {
    let a = decode(VORBIS_MONO).unwrap();
    let native = crate::convert::to_native(&a, 1.0);
    let expected = (a.frames() * 48_000).div_ceil(44_100);
    assert_eq!(native.len(), 2 * expected);
}
