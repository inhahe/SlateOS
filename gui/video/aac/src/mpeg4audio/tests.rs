#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code, on values the tests choose"
)]

use super::*;

/// Bits written as strings of 0s and 1s, then zero bytes for the reader to
/// look ahead into.
fn bytes(bits: &[&str]) -> Vec<u8> {
    let all = bits.concat();
    let mut out = vec![0u8; all.len().div_ceil(8) + 8];
    for (i, c) in all.bytes().enumerate() {
        if c == b'1' {
            out[i / 8] |= 0x80 >> (i % 8);
        }
    }
    out
}

/// The config `data` holds, read from scratch, how many bits came before
/// its object type's own part, and where the reader stopped.
fn read(data: &[u8], size_bits: usize, sync: bool) -> (Config, Result<i32, Invalid>, u32) {
    let mut c = Config::default();
    let mut gb = Bits::new(data, size_bits).unwrap();
    let r = read_config(&mut c, &mut gb, sync);
    (c, r, gb.count())
}

#[test]
fn lc_stereo() {
    // AOT 2, 44.1 kHz (index 4), two channels; then GA's three flags.
    let data = bytes(&["00010", "0100", "0010", "000"]);
    let (c, r, _) = read(&data, 16, true);
    assert_eq!(r, Ok(13));
    assert_eq!(
        (c.object_type, c.sampling_index, c.sample_rate),
        (aot::AAC_LC, 4, 44100)
    );
    assert_eq!((c.chan_config, c.channels), (2, 2));
    assert_eq!((c.ext_object_type, c.ext_sample_rate), (aot::NULL, 0));
    // No sync extension in so few bits: SBR undecided, PS ruled out for
    // two channels.
    assert_eq!((c.sbr, c.ps), (-1, 0));
}

#[test]
fn explicit_he_aac() {
    // AOT 5 at 24 kHz, two channels, SBR at 48 kHz (index 3), over AOT 2.
    let data = bytes(&["00101", "0110", "0010", "0011", "00010", "000"]);
    let (c, r, _) = read(&data, 32, true);
    assert_eq!(r, Ok(22));
    assert_eq!(
        (c.object_type, c.sample_rate, c.channels),
        (aot::AAC_LC, 24000, 2)
    );
    assert_eq!(
        (
            c.sbr,
            c.ext_object_type,
            c.ext_sampling_index,
            c.ext_sample_rate
        ),
        (1, aot::SBR, 3, 48000)
    );
    assert_eq!(c.ps, 0, "two channels");
}

#[test]
fn explicit_ps_unless_it_looks_like_mp3_on_mp4() {
    // AOT 29 at 24 kHz, mono, SBR at 48 kHz, over AOT 2.
    let data = bytes(&["11101", "0110", "0001", "0011", "00010", "000"]);
    let (c, _, _) = read(&data, 32, true);
    assert_eq!((c.ps, c.sbr, c.object_type), (1, 1, aot::AAC_LC));
    // The same object type where the next bits are the MP3-on-MP4 draft's
    // (of the next three, one of the low two set; of the next nine, the low
    // six clear): read as an object type of its own, with no SBR.
    let data = bytes(&["11101", "0110", "0001", "011000000"]);
    let (c, _, _) = read(&data, 32, true);
    assert_eq!(
        (c.object_type, c.ext_object_type, c.sbr),
        (aot::PS, aot::NULL, -1)
    );
    assert_eq!(c.ps, 0, "implicit PS is for LC alone");
}

#[test]
fn backward_compatible_sbr_and_ps_after_the_config() {
    // al_sbr_ps_04_new.mp4's config, as FATE has it: LC at 16 kHz with a
    // program config element, then the sync extension 0x2b7 and SBR at
    // 32 kHz -- three bits short of room for the PS extension (0x548), so
    // PS is left to the stream to signal.
    let data = [
        0x14, 0x00, 0x06, 0x04, 0x00, 0x00, 0x00, 0x00, 0x56, 0xe5, 0xa8,
    ];
    let (c, r, at) = read(&data, data.len() * 8, true);
    assert_eq!(r, Ok(13));
    assert_eq!(
        (c.object_type, c.sample_rate, c.chan_config),
        (aot::AAC_LC, 16000, 0)
    );
    assert_eq!(
        (c.ext_object_type, c.sbr, c.ext_sample_rate),
        (aot::SBR, 1, 32000)
    );
    assert_eq!((c.ps, at), (-1, 85));
    // With room for the PS extension after it -- mono LC at 16 kHz, GA's
    // flags, then the sync extension found three bits on, SBR at 32 kHz,
    // 0x548, PS on -- PS is as it says.
    let data = bytes(&[
        "00010",
        "1000",
        "0001",
        "000",
        "01010110111",
        "00101",
        "1",
        "0101",
        "10101001000",
        "1",
    ]);
    let (c, _, _) = read(&data, 56, true);
    assert_eq!((c.channels, c.ext_object_type, c.sbr), (1, aot::SBR, 1));
    assert_eq!((c.ext_sample_rate, c.ps), (32000, 1));
    // Without looking for it: nothing found, SBR undecided, implicit PS.
    let (c, _, _) = read(&data, data.len() * 8, false);
    assert_eq!((c.ext_object_type, c.sbr, c.ps), (aot::NULL, -1, -1));
}

#[test]
fn escapes_and_refusals() {
    // An escaped object type (31, then 6 bits: 32 + 10 = 42, USAC) and an
    // escaped sample rate (index 15, then 24 bits).
    let data = bytes(&[
        "11111",
        "001010",
        "1111",
        "000000001011101110000000",
        "0001",
    ]);
    let (c, r, _) = read(&data, 48, false);
    assert!(r.is_ok());
    assert_eq!(
        (c.object_type, c.sampling_index, c.sample_rate),
        (aot::USAC, 15, 48000)
    );
    // Channel configuration 15 is none: refused, with what was read kept.
    let data = bytes(&["00010", "0011", "1111"]);
    let (c, r, _) = read(&data, 13, false);
    assert_eq!(r, Err(Invalid));
    assert_eq!(
        (c.object_type, c.sample_rate, c.chan_config),
        (aot::AAC_LC, 48000, 15)
    );
}
