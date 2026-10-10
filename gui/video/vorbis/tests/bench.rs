//! How fast the decoder is, against Tremor on the same machine.
//!
//! Not a pass/fail test: a measurement, run explicitly --
//!
//! ```text
//! cargo test -p vorbis --target x86_64-pc-windows-gnu --release --test bench -- --ignored --nocapture
//! ```
//!
//! -- printing how many times faster than real time it decodes the encoded
//! streams in `tests/data` (not the synthetic ones, which are noise), about
//! 26 seconds of sound in 1 to 8 channels. Each stream is decoded three
//! times, the decoder made fresh each time, and the fastest kept; only
//! decoding is timed. `VORBIS_BENCH_PASSES` sets the passes (1 for
//! callgrind).
//!
//! Tremor's numbers, for comparison, are `tools/bench.c`'s, which does the
//! same with Tremor itself; the instruction counts callgrind gives both are
//! in the workspace `Cargo.toml` beside `vorbis`'s profile.

#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    reason = "a measurement: a missing stream or a decode error should stop it loudly"
)]

mod common;

use std::time::Instant;
use vorbis::Decoder;

#[test]
#[ignore = "a measurement: --release --ignored --nocapture"]
fn bench_vorbis_decode() {
    let passes: usize =
        std::env::var("VORBIS_BENCH_PASSES").map_or(3, |p| p.parse().expect("a number"));
    let dir = common::data("");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("tests/data")
        .filter_map(|e| e.expect("an entry").file_name().into_string().ok())
        .filter(|n| n.ends_with(".ogg") && !n.starts_with("synthetic_"))
        .collect();
    names.sort();
    let (mut seconds, mut sound) = (0.0, 0.0);
    for name in &names {
        let packets = common::ogg_packets(&std::fs::read(dir.join(name)).expect("a stream"));
        let mut best = f64::INFINITY;
        let mut samples = 0;
        let mut rate = 0;
        for _ in 0..passes {
            let mut dec = Decoder::new(&packets[0], &packets[2]).expect("headers");
            let channels = dec.info().channels;
            rate = dec.info().rate;
            let mut out = vec![0i16; dec.max_samples() * channels];
            samples = 0;
            let start = Instant::now();
            for p in &packets[3..] {
                samples += dec.decode(p, &mut out).expect("a packet");
            }
            best = best.min(start.elapsed().as_secs_f64());
        }
        let length = samples as f64 / f64::from(rate);
        println!(
            "{name:24} {length:6.2} s of sound, {:7.0} times real time",
            length / best
        );
        seconds += best;
        sound += length;
    }
    println!(
        "all {sound:.2} s of sound: {:.0} times real time",
        sound / seconds
    );
}
