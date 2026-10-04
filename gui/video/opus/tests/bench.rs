//! How fast the decoder is, against libopus on the same machine.
//!
//! Not a pass/fail test: a measurement, run explicitly --
//!
//! ```text
//! cargo test -p opus --target x86_64-pc-windows-gnu --release --test bench -- --ignored --nocapture
//! ```
//!
//! -- printing how many times faster than real time it decodes every stream
//! in `tests/data` at 48 kHz (the single ones in stereo): SILK, hybrid and
//! CELT in turn, about two and a half minutes of sound. Each stream is
//! decoded three times, the decoder made fresh each time, and the fastest
//! kept; only decoding is timed. `OPUS_BENCH_PASSES` sets the passes (1 for
//! callgrind).
//!
//! libopus's numbers, for comparison, are `tools/bench.c`'s, which does the
//! same with libopus 1.5.2's fixed-point build. They are the table
//! `LIBOPUS` below, with the machine they were measured on; remeasure them
//! before comparing on another.

#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "a measurement: a missing stream or a decode error should stop it loudly"
)]

mod common;

use common::{data_dir, packets};
use opus::{AnyDecoder, Decoder, Head};
use std::time::Instant;

/// libopus 1.5.2 (fixed point, gcc 13 -O2) on the same streams with
/// `tools/bench.c`, in times real time: SILK and hybrid streams, CELT
/// streams, the streams that switch between them, multistream streams, all. See the workspace `Cargo.toml`
/// beside `opus`'s profile for callgrind's instruction counts, which other
/// work on the machine does not move.
const LIBOPUS: [(&str, f64); 5] = [
    ("silk+hybrid", 0.0),
    ("celt", 0.0),
    ("mixed", 0.0),
    ("multistream", 0.0),
    ("all", 0.0),
];

/// Which of the bench's groups a stream is in.
fn group(name: &str, multistream: bool) -> &'static str {
    if multistream {
        "multistream"
    } else if name.starts_with("silk") || name.starts_with("hybrid") {
        "silk+hybrid"
    } else if name.starts_with("celt") {
        "celt"
    } else {
        // The switching and frame-size streams, every mode in turn.
        "mixed"
    }
}

#[test]
#[ignore = "a measurement: --release --ignored --nocapture"]
fn bench_opus_decode() {
    let passes: usize =
        std::env::var("OPUS_BENCH_PASSES").map_or(3, |p| p.parse().expect("a number"));
    let mut names: Vec<String> = std::fs::read_dir(data_dir())
        .expect("tests/data")
        .filter_map(|e| {
            e.expect("an entry")
                .file_name()
                .into_string()
                .ok()?
                .strip_suffix(".bit")
                .map(str::to_owned)
        })
        .collect();
    names.sort();
    // (group, seconds of sound, seconds decoding)
    let mut totals: Vec<(&str, f64, f64)> = Vec::new();
    for name in &names {
        let bit = std::fs::read(data_dir().join(format!("{name}.bit"))).expect("the stream");
        let head = std::fs::read(data_dir().join(format!("{name}.head"))).ok();
        let stream: Vec<&[u8]> = packets(&bit).into_iter().map(|p| p.data).collect();
        let mut best = f64::MAX;
        let mut samples = 0;
        for _ in 0..passes {
            let mut dec = match &head {
                Some(h) => Head::parse(h)
                    .expect("an OpusHead")
                    .decoder(48000)
                    .expect("a decoder"),
                None => AnyDecoder::Single(Box::new(Decoder::new(48000, 2).expect("a decoder"))),
            };
            let mut pcm = vec![0i16; 5760 * dec.channels()];
            samples = 0;
            let start = Instant::now();
            for p in &stream {
                samples += dec.decode(Some(p), &mut pcm, false).expect("a packet");
            }
            best = best.min(start.elapsed().as_secs_f64());
        }
        let g = group(name, head.is_some());
        let seconds = samples as f64 / 48000.0;
        println!(
            "{name:20} {seconds:6.1} s of sound: {:7.0}x real time",
            seconds / best
        );
        match totals.iter_mut().find(|t| t.0 == g) {
            Some(t) => {
                t.1 += seconds;
                t.2 += best;
            }
            None => totals.push((g, seconds, best)),
        }
    }
    let all = totals
        .iter()
        .fold(("all", 0.0, 0.0), |a, t| ("all", a.1 + t.1, a.2 + t.2));
    totals.push(all);
    for (g, seconds, time) in &totals {
        let theirs = LIBOPUS
            .iter()
            .find(|l| l.0 == *g)
            .map_or(String::new(), |l| format!("; libopus {:.0}x", l.1));
        println!(
            "{g:12} {seconds:6.1} s of sound in {:.3} s: {:7.0}x real time{theirs}",
            time,
            seconds / time
        );
    }
}
