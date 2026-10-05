//! Damaged packets and odd calls: errors or noise, never a panic -- in a
//! debug build, where integer overflow traps, so that every sum C lets wrap
//! is shown to wrap here too.
//!
//! `tests/streams.rs` holds damaged streams' decodes to libopus's, through
//! `opus_demo`'s loop. This goes where that loop does not: concealment of
//! every length from 2.5 to 60 ms and in-band FEC asked of any packet at any
//! point, through decoders of every rate and channel count, multistream ones
//! too, on the test streams damaged at random and on made-up packets.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test: a failure should be loud, and its sizes are small"
)]

mod common;

use common::{data_dir, packets};
use opus::{AnyDecoder, Decoder, Head};

/// A small generator, so that a failure is repeatable.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

/// Every packet of `stream`, damaged at random, through `dec`, with
/// concealment and FEC calls of random lengths between.
fn survive(dec: &mut AnyDecoder, rate: u32, stream: &[Vec<u8>], rng: &mut Lcg) {
    let channels = dec.channels();
    let mut pcm = vec![0i16; 5760 * channels];
    for p in stream {
        let mut p = p.clone();
        match rng.below(6) {
            // A few bytes flipped.
            0 | 1 => {
                for _ in 0..=rng.below(4) {
                    if !p.is_empty() {
                        let i = rng.below(p.len());
                        p[i] ^= 1 << rng.below(8);
                    }
                }
            }
            // Cut short.
            2 => p.truncate(rng.below(p.len() + 1)),
            // Random bytes, the TOC kept.
            3 => {
                for b in p.iter_mut().skip(1) {
                    *b = rng.next() as u8;
                }
            }
            _ => {}
        }
        // Whatever the result, no panic; and a success writes no more than
        // the buffer holds.
        if let Ok(n) = dec.decode(Some(&p), &mut pcm, false) {
            assert!(n * channels <= pcm.len());
        }
        let rate = rate as usize;
        match rng.below(8) {
            // A loss concealed, of 2.5 to 60 ms.
            0 => {
                let ms = [
                    rate / 400,
                    rate / 200,
                    rate / 100,
                    rate / 50,
                    rate / 25,
                    rate * 60 / 1000,
                ];
                let n = ms[rng.below(ms.len())];
                // An error is as good as audio here: only a panic fails.
                let _ = dec.decode(None, &mut pcm[..n * channels], false);
            }
            // The packet's redundancy for one before it, of any length.
            1 => {
                let n = (rate / 400 * (1 + rng.below(24))).min(pcm.len() / channels);
                let _ = dec.decode(Some(&p), &mut pcm[..n * channels], true);
            }
            // A buffer of any length, even one too short.
            2 => {
                let n = rng.below(pcm.len() / channels + 1);
                let _ = dec.decode(Some(&p), &mut pcm[..n * channels], false);
            }
            _ => {}
        }
    }
}

const RATES: [u32; 5] = [48000, 24000, 16000, 12000, 8000];

#[test]
fn made_up_packets_do_not_panic() {
    // Every TOC, with payloads of every length to 64 and a few longer, all
    // random.
    let mut rng = Lcg(1);
    let mut stream = Vec::new();
    for toc in 0..=255u8 {
        for len in (0..64).chain([100, 300, 1000, 1275, 1500]) {
            let mut p = vec![toc];
            p.extend((0..len).map(|_| rng.next() as u8));
            stream.push(p);
        }
    }
    for rate in RATES {
        for channels in [1, 2] {
            let mut dec =
                AnyDecoder::Single(Box::new(Decoder::new(rate, channels).expect("a decoder")));
            survive(&mut dec, rate, &stream, &mut rng);
        }
    }
}

#[test]
fn damaged_streams_do_not_panic() {
    let mut names: Vec<String> = std::fs::read_dir(data_dir())
        .expect("tests/data")
        .filter_map(|e| {
            let name = e
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("a UTF-8 name");
            name.strip_suffix(".bit").map(str::to_owned)
        })
        .collect();
    names.sort();
    assert!(!names.is_empty());
    let mut rng = Lcg(2);
    for name in &names {
        let bit = std::fs::read(data_dir().join(format!("{name}.bit"))).expect("the stream");
        let stream: Vec<Vec<u8>> = packets(&bit).iter().map(|p| p.data.to_vec()).collect();
        let head = std::fs::read(data_dir().join(format!("{name}.head"))).ok();
        for rate in RATES {
            match &head {
                Some(h) => {
                    let mut dec = Head::parse(h)
                        .expect("an OpusHead")
                        .decoder(rate)
                        .expect("a decoder");
                    survive(&mut dec, rate, &stream, &mut rng);
                }
                None => {
                    for channels in [1, 2] {
                        let mut dec = AnyDecoder::Single(Box::new(
                            Decoder::new(rate, channels).expect("a decoder"),
                        ));
                        survive(&mut dec, rate, &stream, &mut rng);
                    }
                }
            }
        }
    }
}
