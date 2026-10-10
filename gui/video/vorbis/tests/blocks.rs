//! `Blocks`, the container's view of a packet: it refuses what the decoder
//! refuses, and the block sizes it reads are the ones the decoder completes
//! samples by -- for every packet of every fixture, damaged ones included.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "a test: a failure should be loud"
)]

mod common;

use vorbis::{Blocks, Decoder, Error};

/// Every fixture's name.
fn fixtures() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(common::data(""))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".ogg"))
        .collect();
    names.sort();
    assert!(names.len() >= 40, "fixtures missing: {names:?}");
    names
}

fn packets(name: &str) -> Vec<Vec<u8>> {
    common::ogg_packets(&std::fs::read(common::data(name)).unwrap())
}

/// Each packet decoded, and its block read: the decoder refuses a packet
/// exactly when `Blocks` does, with the same error, and completes a quarter
/// of the last block it took and a quarter of this one.
fn agree(name: &str, packets: &[Vec<u8>]) -> usize {
    let mut decoder = Decoder::new(&packets[0], &packets[2]).unwrap();
    let blocks = Blocks::new(&packets[0], &packets[2]).unwrap();
    let mut out = vec![0i32; decoder.max_samples() * decoder.info().channels];
    let mut last: Option<usize> = None;
    let mut checked = 0;
    for (k, p) in packets.iter().enumerate().skip(3) {
        let read = blocks.block(p);
        match decoder.decode_i32(p, &mut out) {
            Err(e) => assert_eq!(read, Err(e), "{name} packet {k}"),
            Ok(n) => {
                let block = read.unwrap_or_else(|e| panic!("{name} packet {k}: {e}"));
                assert_eq!(blocks.mode_size(block.mode), Some(block.size));
                let want = last.map_or(0, |l| l / 4 + block.size / 4);
                assert_eq!(n, want, "{name} packet {k}");
                assert_eq!(block.size, blocks.sizes()[usize::from(block.long)]);
                assert_eq!(block.previous.is_some(), block.long);
                assert_eq!(block.next.is_some(), block.long);
                last = Some(block.size);
                checked += 1;
            }
        }
    }
    checked
}

#[test]
fn every_packet_reads_as_the_decoder_takes_it() {
    let mut total = 0;
    for name in fixtures() {
        let p = packets(&name);
        match Decoder::new(&p[0], &p[2]) {
            Ok(_) => total += agree(&name, &p),
            Err(e) => assert_eq!(Blocks::new(&p[0], &p[2]), Err(e), "{name}"),
        }
    }
    assert!(total > 3000, "{total} packets");
}

#[test]
fn damaged_packets_are_refused_as_the_decoder_refuses_them() {
    for name in [
        "stereo_q3.ogg",
        "surround51.ogg",
        "synthetic_05.ogg",
        "tiny.ogg",
    ] {
        let p = packets(name);
        for seed in 1..=8 {
            let mut lcg = common::Lcg(seed);
            let mut damaged = p[..3].to_vec();
            for q in &p[3..] {
                let mut q = q.clone();
                if let common::Damage::Kept = common::damage(&mut lcg, &mut q, 1 << 20) {
                    damaged.push(q);
                }
            }
            agree(name, &damaged);
        }
    }
}

#[test]
fn an_encoders_window_bits_tell_the_truth() {
    // In a stream an encoder wrote (libvorbis, or FFmpeg's own), a long
    // block's window bits name the blocks actually either side of it.
    let (mut long, mut switching) = (0, 0);
    for name in fixtures().iter().filter(|n| !n.starts_with("synthetic")) {
        let p = packets(name);
        let blocks = Blocks::new(&p[0], &p[2]).unwrap();
        let read: Vec<_> = p[3..].iter().map(|q| blocks.block(q).unwrap()).collect();
        for w in read.windows(3) {
            if let (Some(previous), Some(next)) = (w[1].previous, w[1].next) {
                assert_eq!((previous, next), (w[0].size, w[2].size), "{name}");
                long += 1;
            }
        }
        let [short, long_size] = blocks.sizes();
        if short != long_size && read.iter().any(|b| b.long) && read.iter().any(|b| !b.long) {
            switching += 1;
        }
    }
    assert!(long > 500, "{long} long blocks");
    assert!(switching > 3, "{switching} streams switch block sizes");
}

#[test]
fn headers_and_strays_are_not_audio() {
    let p = packets("stereo_q3.ogg");
    let blocks = Blocks::new(&p[0], &p[2]).unwrap();
    assert_eq!(blocks.mode_count(), 2);
    assert_eq!(blocks.mode_size(2), None);
    for header in &p[..3] {
        assert_eq!(blocks.block(header), Err(Error::NotAudio));
    }
    assert_eq!(blocks.block(&[]), Err(Error::NotAudio));
    // Mode 1 (long) in a one-byte packet: its window bits fit, the type,
    // mode and window bits being four.
    let b = blocks.block(&[0b0000_0010]).unwrap();
    assert_eq!((b.mode, b.size), (1, blocks.sizes()[1]));
    assert_eq!(
        (b.previous, b.next),
        (Some(blocks.sizes()[0]), Some(blocks.sizes()[0]))
    );
    let b = blocks.block(&[0b0000_1110]).unwrap();
    assert_eq!(
        (b.previous, b.next),
        (Some(blocks.sizes()[1]), Some(blocks.sizes()[1]))
    );
    // Headers in the wrong places are refused as the decoder refuses them.
    assert_eq!(Blocks::new(&p[2], &p[0]), Err(Error::BadHeader));
    assert_eq!(Blocks::new(&p[0], b"not vorbis"), Err(Error::NotVorbis));
}
