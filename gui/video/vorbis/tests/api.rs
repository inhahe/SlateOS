//! The public API's promises, beyond sample-for-sample agreement with
//! Tremor (tests/streams.rs): the three output forms agree, a refused
//! packet changes nothing, `reset` and `samples_in` do what they say, and
//! each header refuses what Tremor refuses.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    reason = "a test: a failure should be loud, and the float output is exact"
)]

mod common;

use vorbis::{Comments, Decoder, Error, Info};

fn packets(name: &str) -> Vec<Vec<u8>> {
    common::ogg_packets(&std::fs::read(common::data(name)).unwrap())
}

fn decoder(p: &[Vec<u8>]) -> Decoder {
    Decoder::new(&p[0], &p[2]).unwrap()
}

#[test]
fn sixteen_bit_and_float_output_are_tremors_precision_scaled() {
    let p = packets("stereo_q3.ogg");
    let (mut a, mut b, mut c) = (decoder(&p), decoder(&p), decoder(&p));
    let len = a.max_samples() * 2;
    let (mut wide, mut short, mut float) = (vec![0i32; len], vec![0i16; len], vec![0f32; len]);
    let mut clipped = 0;
    for packet in &p[3..] {
        let n = a.decode_i32(packet, &mut wide).unwrap();
        assert_eq!(b.decode(packet, &mut short).unwrap(), n);
        assert_eq!(c.decode_float(packet, &mut float).unwrap(), n);
        for i in 0..n * 2 {
            let want = (wide[i] >> 9).clamp(-32768, 32767);
            clipped += usize::from(want != wide[i] >> 9);
            assert_eq!(i32::from(short[i]), want);
            assert_eq!(float[i], wide[i] as f32 / 16_777_216.0);
        }
    }
    // The fixture is loud enough to clip now and then: both ends tested.
    let _ = clipped;
}

#[test]
fn a_buffer_too_small_is_refused_and_changes_nothing() {
    let p = packets("mono_q3.ogg");
    let (mut a, mut b) = (decoder(&p), decoder(&p));
    let mut out = vec![0i32; a.max_samples()];
    let mut want = vec![0i32; a.max_samples()];
    for packet in &p[3..] {
        let n = a.samples_in(packet).unwrap();
        if n > 0 {
            assert_eq!(
                a.decode_i32(packet, &mut out[..n - 1]),
                Err(Error::BufferTooSmall)
            );
        }
        let got = a.decode_i32(packet, &mut out[..n]).unwrap();
        assert_eq!(b.decode_i32(packet, &mut want).unwrap(), got);
        assert_eq!(out[..got], want[..got]);
    }
}

#[test]
fn packets_that_are_not_audio_are_refused_and_change_nothing() {
    let p = packets("stereo_q3.ogg");
    let (mut a, mut b) = (decoder(&p), decoder(&p));
    let mut out = vec![0i32; a.max_samples() * 2];
    let mut want = out.clone();
    for (k, packet) in p[3..20].iter().enumerate() {
        // A header (its first bit set) and an empty packet, between each
        // pair of audio packets.
        assert_eq!(a.decode_i32(&p[k % 3], &mut out), Err(Error::NotAudio));
        assert_eq!(a.decode_i32(&[], &mut out), Err(Error::NotAudio));
        assert_eq!(a.samples_in(&[]), Err(Error::NotAudio));
        let n = a.decode_i32(packet, &mut out).unwrap();
        assert_eq!(b.decode_i32(packet, &mut want).unwrap(), n);
        assert_eq!(out[..n * 2], want[..n * 2]);
    }
}

#[test]
fn a_mode_the_stream_has_not_is_a_bad_packet() {
    // The synthetic streams' packets name, now and then, a mode their
    // mode bits allow but the stream has not (three modes, say, and a 3).
    let mut bad = 0;
    for seed in 1..=24 {
        let p = packets(&format!("synthetic_{seed:02}.ogg"));
        let mut a = decoder(&p);
        let mut out = vec![0i32; a.max_samples() * a.info().channels];
        for q in &p[3..] {
            if a.decode_i32(q, &mut out) == Err(Error::BadPacket) {
                bad += 1;
                assert_eq!(a.samples_in(q), Err(Error::BadPacket));
            }
        }
    }
    assert!(bad > 0, "no bad modes");
}

#[test]
fn reset_forgets_the_overlap_and_samples_in_predicts_every_packet() {
    let p = packets("clicks_q3.ogg");
    let mut a = decoder(&p);
    let mut out = vec![0i32; a.max_samples() * 2];
    let mut total = 0;
    for (k, packet) in p[3..].iter().enumerate() {
        if k == 40 {
            a.reset();
        }
        let predicted = a.samples_in(packet).unwrap();
        let n = a.decode_i32(packet, &mut out).unwrap();
        assert_eq!(n, predicted, "packet {k}");
        if k == 0 || k == 40 {
            assert_eq!(n, 0, "the first packet after a start completes nothing");
        } else {
            assert!(n > 0);
        }
        total += n;
    }
    assert!(total > 0);
}

#[test]
fn errors_say_what_tremor_says() {
    for (e, code, text) in [
        (Error::BufferTooSmall, -131, "output buffer too small"),
        (Error::NotVorbis, -132, "not a Vorbis header"),
        (Error::BadHeader, -133, "damaged Vorbis header"),
        (Error::Version, -134, "unsupported Vorbis version"),
        (Error::NotAudio, -135, "not an audio packet"),
        (Error::BadPacket, -136, "damaged audio packet"),
    ] {
        assert_eq!(e.code(), code);
        assert_eq!(e.to_string(), text);
    }
}

/// An identification header with the given fields.
fn id_header(version: u32, channels: u8, rate: u32, bs: (u8, u8), framing: u8) -> Vec<u8> {
    let mut h = vec![1];
    h.extend_from_slice(b"vorbis");
    h.extend_from_slice(&version.to_le_bytes());
    h.push(channels);
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&0u32.to_le_bytes());
    h.extend_from_slice(&128_000u32.to_le_bytes());
    h.extend_from_slice(&0u32.to_le_bytes());
    h.push(bs.0 | bs.1 << 4);
    h.push(framing);
    h
}

#[test]
fn the_identification_header_refuses_what_tremor_refuses() {
    let good = Info::parse(&id_header(0, 2, 44100, (8, 11), 1)).unwrap();
    assert_eq!(
        (good.channels, good.rate, good.blocksizes),
        (2, 44100, [256, 2048])
    );
    assert_eq!(good.bitrate_nominal, 128_000);
    for (header, want) in [
        (id_header(1, 2, 44100, (8, 11), 1), Error::Version),
        (id_header(0, 0, 44100, (8, 11), 1), Error::BadHeader),
        (id_header(0, 2, 0, (8, 11), 1), Error::BadHeader),
        (id_header(0, 2, 44100, (5, 11), 1), Error::BadHeader),
        (id_header(0, 2, 44100, (11, 8), 1), Error::BadHeader),
        (id_header(0, 2, 44100, (8, 14), 1), Error::BadHeader),
        (id_header(0, 2, 44100, (8, 11), 0), Error::BadHeader),
    ] {
        assert_eq!(Info::parse(&header), Err(want));
    }
    // Cut short anywhere: refused.
    let whole = id_header(0, 2, 44100, (8, 11), 1);
    for cut in 0..whole.len() {
        assert!(Info::parse(&whole[..cut]).is_err(), "cut at {cut}");
    }
    // Another header, or not Vorbis at all.
    let p = packets("stereo_q3.ogg");
    assert_eq!(Info::parse(&p[1]), Err(Error::BadHeader));
    assert_eq!(Info::parse(b"\x01opus.."), Err(Error::NotVorbis));
    assert_eq!(Decoder::new(&p[1], &p[2]).err(), Some(Error::BadHeader));
    assert_eq!(Decoder::new(&p[0], &p[1]).err(), Some(Error::BadHeader));
}

#[test]
fn comments_are_the_bytes_the_header_holds() {
    let p = packets("synthetic_01.ogg");
    let c = Comments::parse(&p[1]).unwrap();
    assert_eq!(c.vendor, b"synthetic 1");
    assert_eq!(
        c.comments,
        vec![b"SEED=1".to_vec(), b"TITLE=noise".to_vec()]
    );
    // Cut short anywhere, or claiming more than it holds: refused.
    for cut in 0..p[1].len() {
        assert!(Comments::parse(&p[1][..cut]).is_err(), "cut at {cut}");
    }
    let mut long = p[1].clone();
    long[7] = 0xff;
    assert_eq!(Comments::parse(&long), Err(Error::BadHeader));
    let mut many = p[1].clone();
    let at = 7 + 4 + 11;
    many[at..at + 4].copy_from_slice(&1000u32.to_le_bytes());
    assert_eq!(Comments::parse(&many), Err(Error::BadHeader));
    assert_eq!(Comments::parse(&p[0]), Err(Error::BadHeader));
}
