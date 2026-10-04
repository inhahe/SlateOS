//! How fast the decoder is, against libvpx on the same machine.
//!
//! Not a pass/fail test: a measurement, run explicitly --
//!
//! ```text
//! python gui/video/vp9/tools/fetch_vectors.py
//! python gui/video/vp8/tools/make_bench_stream.py
//! cargo test -p vp8 --target x86_64-pc-windows-gnu --release --test bench -- --ignored --nocapture
//! ```
//!
//! -- printing frames per second for 217 frames of 1080p film coded as VP8
//! at 5 Mbit/s (`tools/make_bench_stream.py`). The whole stream is decoded
//! three times, the decoder made fresh each time, and the fastest kept. Only
//! decoding is timed: the frames are read out of their file first.
//!
//! libvpx's numbers, for comparison, are its `vpxdec --noblit --summary -t
//! 1` on the same file, which also times only decoding -- the fastest of five
//! runs -- built from v1.17.0 with `--enable-optimizations`: as plain C
//! (`--target=generic-gnu`), the algorithms this port translates, and with
//! its x86 SIMD (`--target=x86_64-linux-gcc`), what a browser ships. They are
//! the table `LIBVPX` below, with the machine they were measured on;
//! remeasure them, on the same stream, before comparing on another.

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

use vp8::Decoder;

/// libvpx v1.17.0's speed on the benchmark stream, in frames per second:
/// (plain C, x86 SIMD), each on one thread. The fastest of five runs,
/// measured 2026-10-04 on an Intel Core i7-8700K (six cores, twelve
/// threads, 3.7 GHz; WSL 2 Ubuntu, gcc 13) while other work shared it.
const LIBVPX: (f64, f64) = (24.9, 0.0);

#[test]
#[ignore = "a measurement: python gui/video/vp8/tools/make_bench_stream.py, then --release --ignored --nocapture"]
fn bench_vp8_decode() {
    let dir = common::full_suite_dir().expect("target/vp8vectors is missing");
    let v = common::read_vector(&dir.join("bench-film-1080p.ivf"))
        .expect("python gui/video/vp8/tools/make_bench_stream.py");
    let mut best = f64::MAX;
    let mut shown = 0;
    for _ in 0..3 {
        let mut d = Decoder::new();
        shown = 0;
        let start = Instant::now();
        for frame in &v.frames {
            if d.decode(frame).unwrap().is_some() {
                shown += 1;
            }
        }
        best = best.min(start.elapsed().as_secs_f64());
    }
    let fps = shown as f64 / best;
    println!(
        "{}x{}, {shown} pictures: {fps:.1} fps on one thread; libvpx {:.1} as plain C, {:.1} with SIMD",
        v.width, v.height, LIBVPX.0, LIBVPX.1
    );
}
