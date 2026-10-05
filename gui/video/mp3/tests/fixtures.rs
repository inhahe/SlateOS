//! Every stream in `tests/data` decoded as minimp3 decodes it, sample for
//! sample: each `decode_frame` call's bytes, samples, frame facts and the
//! hash of its samples against the answer `tools/reference.c` gave
//! (`tests/data/generate_fixtures.py` made both).

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "a test: a panic is a failed test"
)]

mod common;

use std::path::Path;

fn fixture(name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let stream = ["mp3", "mp2", "mp1"]
        .iter()
        .map(|ext| dir.join(format!("{name}.{ext}")))
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("no stream for {name}"));
    common::check(&stream, &dir.join(format!("{name}.txt")));
}

macro_rules! fixtures {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                fixture(stringify!($name));
            }
        )*
    };
}

fixtures!(
    // Layer III by LAME: MPEG-1, -2 and -2.5 at every rate, mono, stereo,
    // joint stereo and dual channel, CBR and VBR, short blocks, no
    // reservoir, free format, CRC; an ID3v2 tag and a Xing frame in front.
    lame_cbr128_joint_44100,
    lame_cbr320_stereo_48000,
    lame_vbr_mono_32000,
    lame_mpeg2_24000_joint,
    lame_mpeg2_22050_stereo,
    lame_mpeg2_16000_mono,
    lame_mpeg25_12000_stereo,
    lame_mpeg25_11025_joint,
    lame_mpeg25_8000_mono,
    lame_attacks_44100,
    lame_attacks_mpeg2_22050,
    lame_noreservoir_44100,
    lame_tagged_44100,
    lame_tagged_mono_48000,
    lame_tagged_mono_22050,
    lame_vbr_tagged_44100,
    lame_freeformat_44100,
    lame_freeformat_mpeg2_24000,
    lame_crc_dual_48000,
    lame_crc_mpeg25_11025,
    // Layer III by shine.
    shine_128_44100,
    shine_mono_32000,
    // Layer II by FFmpeg's encoder and by twolame: MPEG-1 and -2, joint
    // stereo's intensity bands, dual channel, CRC.
    mp2_192_stereo_48000,
    mp2_56_mono_44100,
    mp2_32_stereo_32000,
    mp2_mpeg2_22050_mono,
    mp2_mpeg2_16000_stereo,
    twolame_joint_44100,
    twolame_crc_dual_32000,
    twolame_mpeg2_24000_joint,
    // Layer I, every field at every value it may take: stereo, joint stereo
    // at each intensity bound, dual channel, mono, CRC, free format.
    layer1_stereo_48000,
    layer1_joint_44100,
    layer1_dual_32000,
    layer1_mono_crc_44100,
    layer1_free_48000,
    // Damage: minimp3's sync, resync and refusals, followed exactly.
    damaged_bitflips,
    damaged_headers,
    damaged_garbage,
    damaged_cut,
    damaged_truncated,
    damaged_tags,
    damaged_layer2_bitflips,
    damaged_layer2_garbage,
    damaged_layer1_bitflips,
    damaged_freeformat_cut,
    // The file's start and end: ID3v2 tags, junk, a VBRI frame, a joined
    // file, a file of one frame.
    id3v2_tags_in_front,
    junk_in_front,
    vbri_in_front,
    joined_after_tagged,
    one_frame,
);

/// Every answer in `tests/data` has its test above, so that a fixture added
/// to the generator cannot go unchecked.
#[test]
fn every_fixture_is_tested() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let source =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures.rs"))
            .expect("tests/fixtures.rs");
    for entry in std::fs::read_dir(&dir).expect("tests/data") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|e| e == "txt") {
            let name = path.file_stem().and_then(|s| s.to_str()).expect("a name");
            assert!(
                source.contains(&format!("    {name},")),
                "{name} has no test in tests/fixtures.rs"
            );
        }
    }
}
