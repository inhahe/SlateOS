//! The decoder's interface at its edges: no input, input with no frame in
//! it, a frame looked at without decoding it, a reset, a frame on its own.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a panic is a failed test"
)]

mod common;

use std::path::Path;

use mp3::{Decoder, FrameInfo, MAX_SAMPLES_PER_FRAME};

fn data(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name),
    )
    .unwrap()
}

#[test]
fn no_input_takes_nothing() {
    let mut dec = Decoder::new();
    let mut pcm = [0i16; MAX_SAMPLES_PER_FRAME];
    assert_eq!(
        dec.decode_frame(&[], Some(&mut pcm)),
        (0, FrameInfo::default())
    );
}

#[test]
fn input_without_a_frame_is_passed_over_whole() {
    let mut dec = Decoder::new();
    let mut pcm = [0i16; MAX_SAMPLES_PER_FRAME];
    for len in [1usize, 3, 4, 5, 100, 5000] {
        // Bytes that never make a valid header: no 0xff in them at all.
        let junk: Vec<u8> = (0..len).map(|i| (i * 7 % 255) as u8).collect();
        let (samples, info) = dec.decode_frame(&junk, Some(&mut pcm));
        assert_eq!(samples, 0);
        assert_eq!(
            info,
            FrameInfo {
                frame_bytes: len,
                ..FrameInfo::default()
            },
            "{len} bytes"
        );
    }
}

#[test]
fn looking_without_decoding_gives_the_frame_and_changes_nothing() {
    let file = data("lame_cbr128_joint_44100.mp3");
    let mut dec = Decoder::new();
    let (samples, info) = dec.decode_frame(&file, None);
    assert_eq!(samples, 1152);
    assert_eq!(
        (
            info.frame_offset,
            info.channels,
            info.hz,
            info.layer,
            info.bitrate_kbps
        ),
        (0, 2, 44100, 3, 128)
    );
    assert_eq!(info.frame_bytes, 417);
    // Decoding afterwards is decoding from the start: nothing was taken
    // into the reservoir by the look.
    let mut looked = Decoder::new();
    let mut fresh = Decoder::new();
    let mut a = [0i16; MAX_SAMPLES_PER_FRAME];
    let mut b = [0i16; MAX_SAMPLES_PER_FRAME];
    looked.decode_frame(&file, None);
    let mut pos = 0;
    for _ in 0..5 {
        let x = looked.decode_frame(&file[pos..], Some(&mut a));
        let y = fresh.decode_frame(&file[pos..], Some(&mut b));
        assert_eq!(x, y);
        assert_eq!(a[..x.0 * 2], b[..y.0 * 2]);
        pos += x.1.frame_bytes;
    }
}

#[test]
fn a_reset_decodes_as_a_new_decoder_does() {
    let file = data("lame_attacks_44100.mp3");
    let mut dec = Decoder::new();
    let mut pcm = [0i16; MAX_SAMPLES_PER_FRAME];
    let first = common::lines(&file);
    // Halfway through, a reset and back to the start: the same lines.
    let mut pos = 0;
    for _ in 0..20 {
        pos += dec.decode_frame(&file[pos..], Some(&mut pcm)).1.frame_bytes;
    }
    dec.reset();
    let mut again = Vec::new();
    pos = 0;
    loop {
        let (samples, info) = dec.decode_frame(&file[pos..], Some(&mut pcm));
        if info.frame_bytes == 0 {
            again.push(format!("end {pos}"));
            break;
        }
        again.push(format!(
            "frame {pos} {} {samples} {} {} {} {} {:016x}",
            info.frame_bytes,
            info.channels,
            info.hz,
            info.layer,
            info.bitrate_kbps,
            common::fnv(&pcm[..samples * info.channels])
        ));
        pos += info.frame_bytes;
    }
    assert_eq!(again, first);
}

#[test]
fn a_frame_alone_is_decoded_and_a_cut_one_passed_over() {
    // A Layer II frame needs nothing before it: given exactly its bytes, it
    // decodes (minimp3 trusts a frame that fills its input); given one byte
    // fewer, there is no frame.
    let file = data("mp2_192_stereo_48000.mp2");
    let mut pcm = [0i16; MAX_SAMPLES_PER_FRAME];
    let (_, first) = Decoder::new().decode_frame(&file, None);
    let size = first.frame_bytes;
    assert_eq!(size, 576);
    let (samples, info) = Decoder::new().decode_frame(&file[..size], Some(&mut pcm));
    assert_eq!((samples, info.frame_bytes), (1152, size));
    let (samples, info) = Decoder::new().decode_frame(&file[..size - 1], Some(&mut pcm));
    assert_eq!((samples, info.frame_bytes), (0, size - 1));
}
