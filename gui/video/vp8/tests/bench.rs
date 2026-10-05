//! How fast the decoder is, against libvpx on the same machine.
//!
//! Not a pass/fail test: a measurement, run explicitly --
//!
//! ```text
//! python gui/video/vp9/tools/fetch_vectors.py
//! python gui/video/vp8/tools/make_bench_stream.py --vpxenc <libvpx's vpxenc, with VP8>
//! cargo test -p vp8 --target x86_64-pc-windows-gnu --release --test bench -- --ignored --nocapture
//! ```
//!
//! -- printing frames per second for 217 frames of 1080p film coded as VP8
//! at 5 Mbit/s (`tools/make_bench_stream.py`), twice: as most encoders code
//! it, in one token partition, which decodes on one thread; and in eight,
//! which decodes on up to eight. The whole stream is decoded three times,
//! the decoder made fresh each time, and the fastest kept. Only decoding is
//! timed: the frames are read out of their file first.
//!
//! libvpx's numbers, for comparison, are its `vpxdec --noblit --summary -t
//! N` on the same files, which also times only decoding -- the fastest of
//! five runs -- built from v1.17.0 with `--enable-optimizations`: as plain C
//! (`--target=generic-gnu`), the algorithms this port translates, and with
//! its x86 SIMD (`--target=x86_64-linux-gcc`), what a browser ships. They are
//! the tables below, with the machine they were measured on; remeasure them,
//! on the same streams, before comparing on another.

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

/// libvpx v1.17.0's speed on the one-partition stream, in frames per
/// second: (plain C, x86 SIMD), each on one thread. The fastest of five
/// runs, measured 2026-10-04 on an Intel Core i7-8700K (six cores, twelve
/// threads, 3.7 GHz; WSL 2 Ubuntu, gcc 13) while other work shared it --
/// heavily enough that the same runs ranged over a factor of two. The port
/// made 30.8 on the same machine, under the same load, and 40.0 at its
/// best in WSL; callgrind's instruction counts, which the load does not
/// move, are in the workspace `Cargo.toml` beside `vp8`'s profile.
const LIBVPX: (f64, f64) = (24.9, 110.2);

/// libvpx v1.17.0's speed on the eight-partition stream, by thread count:
/// (threads, plain C, x86 SIMD), measured as [`LIBVPX`] was, on 2026-10-05.
/// Eight threads ran slower than four: the six cores were shared with other
/// work, and the threads beyond six share cores with the others.
const LIBVPX_THREADED: [(usize, f64, f64); 4] = [
    (1, 28.1, 136.4),
    (2, 44.2, 191.8),
    (4, 61.0, 245.5),
    (8, 28.8, 161.4),
];

/// The fastest of three decodes of `frames`, on up to `threads` threads:
/// pictures a second.
fn fps(frames: &[Vec<u8>], threads: usize) -> f64 {
    let mut best = f64::MAX;
    let mut shown = 0;
    for _ in 0..3 {
        let mut d = Decoder::new();
        d.set_threads(threads);
        shown = 0;
        let start = Instant::now();
        for frame in frames {
            if d.decode(frame).unwrap().is_some() {
                shown += 1;
            }
        }
        best = best.min(start.elapsed().as_secs_f64());
    }
    f64::from(shown) / best
}

#[test]
#[ignore = "a measurement: python gui/video/vp8/tools/make_bench_stream.py, then --release --ignored --nocapture"]
fn bench_vp8_decode() {
    let dir = common::full_suite_dir().expect("target/vp8vectors is missing");
    let v = common::read_vector(&dir.join("bench-film-1080p.ivf"))
        .expect("python gui/video/vp8/tools/make_bench_stream.py");
    println!(
        "{}x{}, one partition: {:.1} fps on one thread; libvpx {:.1} as plain C, {:.1} with SIMD",
        v.width,
        v.height,
        fps(&v.frames, 1),
        LIBVPX.0,
        LIBVPX.1
    );
    let v = common::read_vector(&dir.join("bench-film-1080p-parts8.ivf"))
        .expect("python gui/video/vp8/tools/make_bench_stream.py --vpxenc <path>");
    for (threads, c, simd) in LIBVPX_THREADED {
        println!(
            "{}x{}, eight partitions: {:.1} fps on {threads} threads; libvpx {c:.1} as plain C, {simd:.1} with SIMD",
            v.width,
            v.height,
            fps(&v.frames, threads),
        );
    }
}
