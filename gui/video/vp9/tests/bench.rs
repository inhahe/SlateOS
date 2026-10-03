//! How fast the decoder is, against libvpx on the same machine.
//!
//! Not a pass/fail test: a measurement, run explicitly --
//!
//! ```text
//! python gui/video/vp9/tools/fetch_vectors.py
//! cargo test -p vp9 --target x86_64-pc-windows-gnu --release --test bench -- --ignored --nocapture
//! ```
//!
//! -- printing frames per second for four of libvpx's vectors, on one thread
//! and on as many as the machine has cores: a 1080p film of 217 frames in
//! one tile column, which only one thread can decode; a 1080p one in 4x4
//! tiles and a 4K one in eight tile columns, which threads share; and a CIF one
//! at the finest quantiser, where coefficient decoding dominates. A short
//! vector is decoded over and over, the decoder made fresh each pass as a
//! player would make it, until a pass of passes has taken half a second;
//! the fastest of three such runs is kept. Only decoding is timed: the
//! packets are read out of their file first.
//!
//! libvpx's numbers, for comparison, are its `vpxdec --noblit --summary` on
//! the same files (`tools/fetch_vectors.py`'s copies), which also times only
//! decoding -- the fastest of five runs -- built from v1.17.0 with
//! `--enable-optimizations`: as plain C (`--target=generic-gnu`) on one
//! thread (`-t 1`), the algorithms this port translates; and with its x86
//! SIMD (`--target=x86_64-linux-gcc`), what a browser ships, on one thread
//! and on twelve (`-t 12`, this machine's count). They are the table
//! `LIBVPX` below, with the machine they were measured on; remeasure them
//! on another before comparing.

#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a measurement: a missing vector or a decode error should stop it loudly"
)]

mod common;

use std::time::{Duration, Instant};

use vp9::Decoder;

/// libvpx v1.17.0's speed on each vector, in frames per second: (vector,
/// plain C on one thread, SIMD on one thread, SIMD on twelve). The fastest
/// of five runs, measured 2026-10-03 on an Intel Core i7-8700K (six cores,
/// twelve threads, 3.7 GHz; WSL 2 Ubuntu, gcc 13), the machine the port's
/// own numbers were first taken on, while other work shared it.
const LIBVPX: [(&str, f64, f64, f64); 4] = [
    ("vp90-2-02-size-lf-1920x1080.webm", 17.4, 77.1, 70.1),
    ("vp90-2-08-tile-4x4.webm", 15.1, 70.7, 78.9),
    ("vp90-2-08-tile_1x8_frame_parallel.webm", 6.1, 31.4, 68.7),
    ("vp90-2-00-quantizer-00.webm", 108.5, 119.7, 127.0),
];

/// Decode every packet once, on a fresh decoder; how many pictures came out.
fn pass(packets: &[Vec<u8>], threads: usize) -> usize {
    let mut d = Decoder::new();
    d.set_threads(threads);
    packets
        .iter()
        .filter(|p| d.decode(p).unwrap().is_some())
        .count()
}

/// Frames per second on `threads` threads: passes repeated until they take
/// half a second, the fastest of three such runs.
fn fps(packets: &[Vec<u8>], threads: usize) -> f64 {
    let start = Instant::now();
    let frames = pass(packets, threads);
    let once = start.elapsed().max(Duration::from_micros(1));
    let passes = (Duration::from_millis(500).as_secs_f64() / once.as_secs_f64()).ceil() as usize;
    let passes = passes.max(1);
    (0..3)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..passes {
                pass(packets, threads);
            }
            (frames * passes) as f64 / start.elapsed().as_secs_f64()
        })
        .fold(0.0, f64::max)
}

#[test]
#[ignore = "measurement benchmark; run explicitly with --release --ignored --nocapture"]
fn bench_vp9_decode() {
    let dir = common::full_suite_dir()
        .expect("the full suite is not fetched: python gui/video/vp9/tools/fetch_vectors.py");
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    std::println!(
        "frames per second; libvpx's as measured on the machine `LIBVPX` names\n\
         {:<40} {:>8} {:>8} {:>9} {:>9} {:>11}",
        "vector",
        "1 thread",
        format!("{cores} thr"),
        "libvpx C",
        "SIMD 1",
        "SIMD 12"
    );
    for (name, c1, simd1, simd12) in LIBVPX {
        let v = common::read_vector(&dir.join(name)).unwrap();
        let one = fps(&v.packets, 1);
        let all = fps(&v.packets, cores);
        std::println!("{name:<40} {one:>8.1} {all:>8.1} {c1:>9.1} {simd1:>9.1} {simd12:>11.1}");
    }
}
