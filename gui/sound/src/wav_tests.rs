#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;

/// A `fmt ` chunk's body.
fn fmt_body(tag: u16, channels: u16, rate: u32, bits: u16) -> Vec<u8> {
    let block = channels * bits.div_ceil(8);
    let mut b = Vec::new();
    b.extend_from_slice(&tag.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
    b.extend_from_slice(&block.to_le_bytes());
    b.extend_from_slice(&bits.to_le_bytes());
    b
}

/// An extensible `fmt ` chunk's body, its GUID saying `tag`.
fn fmt_extensible(tag: u16, channels: u16, rate: u32, bits: u16) -> Vec<u8> {
    let mut b = fmt_body(FORMAT_EXTENSIBLE, channels, rate, bits);
    b.extend_from_slice(&22u16.to_le_bytes());
    b.extend_from_slice(&bits.to_le_bytes());
    b.extend_from_slice(&0x3u32.to_le_bytes());
    b.extend_from_slice(&tag.to_le_bytes());
    b.extend_from_slice(&[0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71]);
    b
}

fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut c = Vec::new();
    c.extend_from_slice(id);
    c.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
    c.extend_from_slice(body);
    if body.len() % 2 == 1 {
        c.push(0);
    }
    c
}

fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut f = b"RIFF".to_vec();
    f.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_le_bytes());
    f.extend_from_slice(b"WAVE");
    f.extend_from_slice(&body);
    f
}

fn wav(fmt: Vec<u8>, data: &[u8]) -> Vec<u8> {
    riff(&[chunk(b"fmt ", &fmt), chunk(b"data", data)])
}

/// **Every sample format reads to 1.0 full scale**: 8-bit unsigned, 16-,
/// 24- and 32-bit signed, 32- and 64-bit float.
#[test]
fn every_sample_format_reads_to_full_scale() {
    let a = read(&wav(fmt_body(1, 1, 8000, 8), &[0, 128, 255])).unwrap();
    assert_eq!(a.samples, [-1.0, 0.0, 127.0 / 128.0]);

    let s16: Vec<u8> = [i16::MIN, 0, 16_384]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let a = read(&wav(fmt_body(1, 1, 8000, 16), &s16)).unwrap();
    assert_eq!(a.samples, [-1.0, 0.0, 0.5]);

    // 24-bit: the top byte's sign carries.
    let a = read(&wav(
        fmt_body(1, 1, 8000, 24),
        &[0, 0, 0x80, 0xFF, 0xFF, 0xFF, 0, 0, 0x40],
    ))
    .unwrap();
    assert_eq!(a.samples, [-1.0, -1.0 / 8_388_608.0, 0.5]);

    let s32: Vec<u8> = [i32::MIN, i32::MAX / 2 + 1]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let a = read(&wav(fmt_body(1, 1, 8000, 32), &s32)).unwrap();
    assert_eq!(a.samples, [-1.0, 0.5]);

    let f32s: Vec<u8> = [0.25f32, -0.75]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let a = read(&wav(fmt_body(3, 1, 8000, 32), &f32s)).unwrap();
    assert_eq!(a.samples, [0.25, -0.75]);

    let f64s: Vec<u8> = [0.125f64, -1.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let a = read(&wav(fmt_body(3, 1, 8000, 64), &f64s)).unwrap();
    assert_eq!(a.samples, [0.125, -1.0]);
}

/// **An extensible file reads as the format its GUID names**, and its
/// shape -- channels, rate, WAV's channel order -- comes through.
#[test]
fn an_extensible_file_reads_as_its_guids_format() {
    let s16: Vec<u8> = [100i16, -100, 200, -200]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let a = read(&wav(fmt_extensible(1, 2, 44_100, 16), &s16)).unwrap();
    assert_eq!(
        (a.rate, a.channels, a.order),
        (44_100, 2, ChannelOrder::Wav)
    );
    assert_eq!(a.frames(), 2);
    let f: Vec<u8> = [0.5f32].iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(
        read(&wav(fmt_extensible(3, 1, 8000, 32), &f))
            .unwrap()
            .samples,
        [0.5]
    );
}

/// **A streaming writer's data size -- nought, or past the end -- takes
/// the rest of the file**, whole frames of it.
#[test]
fn a_streaming_writers_size_takes_the_rest() {
    let mut f = riff(&[chunk(b"fmt ", &fmt_body(1, 2, 8000, 16))]);
    f.extend_from_slice(b"data");
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(&[1, 0, 2, 0, 3, 0, 4, 0, 5, 0]);
    let a = read(&f).unwrap();
    assert_eq!(a.frames(), 2, "the half frame at the end is dropped");
    let at = f.len() - 14;
    f[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(read(&f).unwrap().frames(), 2);
}

/// **Chunks before the data are passed over, odd ones by their padding.**
#[test]
fn chunks_before_the_data_are_passed_over() {
    let f = riff(&[
        chunk(b"fmt ", &fmt_body(1, 1, 8000, 16)),
        chunk(b"LIST", b"odd"),
        chunk(b"fact", &[1, 2, 3, 4]),
        chunk(b"data", &[0, 0x40]),
    ]);
    assert_eq!(read(&f).unwrap().samples, [0.5]);
}

/// **What this cannot read is refused, saying why**: not RIFF, samples
/// before their format, a chunk claiming more than the file, no data, a
/// compressed format, odd sizes, and channel counts, rates and frame sizes
/// that do not hold together.
#[test]
fn what_cannot_be_read_is_refused() {
    assert_eq!(read(b"OggS...."), Err(DecodeError::Unrecognised));
    assert_eq!(read(b"RIFF\0\0\0\0AVI "), Err(DecodeError::Unrecognised));
    let data_first = riff(&[
        chunk(b"data", &[0, 0]),
        chunk(b"fmt ", &fmt_body(1, 1, 8000, 16)),
    ]);
    assert!(matches!(read(&data_first), Err(DecodeError::Wav(_))));
    let mut liar = riff(&[chunk(b"fmt ", &fmt_body(1, 1, 8000, 16))]);
    liar.extend_from_slice(b"junk");
    liar.extend_from_slice(&1000u32.to_le_bytes());
    assert!(matches!(read(&liar), Err(DecodeError::Wav(_))));
    assert!(matches!(
        read(&riff(&[chunk(b"fmt ", &fmt_body(1, 1, 8000, 16))])),
        Err(DecodeError::Wav(_))
    ));
    for bad in [
        fmt_body(2, 1, 8000, 4),
        fmt_body(1, 1, 8000, 12),
        fmt_body(3, 1, 8000, 16),
        fmt_body(1, 0, 8000, 16),
        fmt_body(1, 9, 8000, 16),
        fmt_body(1, 1, 0, 16),
        fmt_body(1, 1, 800_000, 16),
    ] {
        assert!(matches!(
            read(&wav(bad, &[0; 32])),
            Err(DecodeError::Wav(_))
        ));
    }
    let mut misaligned = fmt_body(1, 2, 8000, 16);
    misaligned[12..14].copy_from_slice(&3u16.to_le_bytes());
    assert!(matches!(
        read(&wav(misaligned, &[0; 32])),
        Err(DecodeError::Wav(_))
    ));
    assert!(
        matches!(read(&wav(vec![1, 0, 1], &[0; 4])), Err(DecodeError::Wav(_))),
        "short fmt"
    );
}

/// **A sample that is not a number reads as silence.**
#[test]
fn a_sample_that_is_not_a_number_is_silence() {
    let f: Vec<u8> = [f32::NAN, f32::INFINITY, 0.5]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    assert_eq!(
        read(&wav(fmt_body(3, 1, 8000, 32), &f)).unwrap().samples,
        [0.0, 0.0, 0.5]
    );
}

/// **A long file is cut at the most a sound plays**, before the rest is
/// read.
#[test]
fn a_long_file_is_cut() {
    let seconds = usize::try_from(MAX_SECONDS).unwrap() + 2;
    let data = vec![0u8; 100 * seconds];
    let a = read(&wav(fmt_body(1, 1, 100, 8), &data)).unwrap();
    assert_eq!(a.frames(), 100 * usize::try_from(MAX_SECONDS).unwrap());
    assert_eq!(a.duration_ms(), u64::from(MAX_SECONDS) * 1000);
}
