//! How fast the decoder and the encoder are, against libvpx on the same
//! machine.
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
//!
//! `bench_vp9_encode` does the same for the encoder: each reference encode's
//! input (`tests/data/encoder/README.md`) encoded at its settings, only the
//! encoding timed, against libvpx's `vpxenc` -- on one thread
//! (`LIBVPX_ENCODE`), and for the references in tile columns on every core
//! too (`LIBVPX_ENCODE_TILES`).

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

use vp9::{Decoder, Encoder, EncoderConfig, PlaneView};

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

/// libvpx v1.17.0's realtime encoder on each reference input, in frames per
/// second: (reference, plain C, SIMD), both on one thread. Its `vpxenc` with
/// the references' settings (`tests/data/encoder/README.md`), raw I420 in,
/// timed whole -- reading the input is a percent or two of it -- the fastest
/// of five runs, built as `LIBVPX`'s builds are with the encoder enabled
/// (its SIMD build makes the C build's bytes), measured 2026-10-04 on the
/// machine `LIBVPX` names while a boot test shared it -- as did the port's
/// first numbers: 20.9, 55.5 and 123.1.
const LIBVPX_ENCODE: [(&str, f64, f64); 3] = [
    ("rt8 (1280x720)", 28.9, 80.5),
    ("rt8cut (651x357)", 75.1, 275.3),
    ("rt8small (350x286)", 154.8, 638.1),
];

/// The same, for the references in tile columns (`rt8tiles`, four columns;
/// `rt8cuttiles`, two): (reference, plain C on one thread, SIMD on one, SIMD
/// on one thread per column -- `vpxenc --threads`), measured as
/// `LIBVPX_ENCODE` was, a boot test sharing the machine.
const LIBVPX_ENCODE_TILES: [(&str, f64, f64, f64); 2] = [
    ("rt8tiles (4 columns)", 24.4, 78.5, 117.2),
    ("rt8cuttiles (2 columns)", 60.1, 233.3, 281.4),
];

/// The first `count` pictures the full suite's vector `name` shows, as I420.
fn shown(name: &str, count: usize) -> Option<Vec<common::I420>> {
    let dir = common::full_suite_dir()?;
    let v = common::read_vector(&dir.join(name)).ok()?;
    let mut d = Decoder::new();
    let mut out = Vec::new();
    for p in &v.packets {
        let Some(picture) = d.decode(p).ok()? else {
            continue;
        };
        let planes = [0, 1, 2].map(|i| {
            let view = picture.plane8(i).unwrap();
            (0..view.height)
                .flat_map(|y| &view.data[y * view.stride..][..view.width])
                .copied()
                .collect::<Vec<u8>>()
        });
        out.push(common::I420 {
            width: picture.width() as usize,
            height: picture.height() as usize,
            planes,
        });
        if out.len() == count {
            return Some(out);
        }
    }
    None
}

/// Encode every picture once, on a fresh encoder made with `config`, on at
/// most `threads` threads.
fn encode_pass(input: &[common::I420], config: EncoderConfig, threads: usize) {
    let mut e = Encoder::new(config).unwrap();
    e.set_threads(threads);
    for p in input {
        let views = [0, 1, 2].map(|i| {
            let (width, height) = p.plane_size(i);
            PlaneView {
                data: &p.planes[i],
                stride: width,
                width,
                height,
            }
        });
        e.encode(views).unwrap();
    }
}

/// Frames per second encoding `input` on `threads` threads: passes repeated
/// until they take half a second, the fastest of three such runs.
fn encode_fps(input: &[common::I420], config: EncoderConfig, threads: usize) -> f64 {
    let start = Instant::now();
    encode_pass(input, config, threads);
    let once = start.elapsed().max(Duration::from_micros(1));
    let passes = (Duration::from_millis(500).as_secs_f64() / once.as_secs_f64()).ceil() as usize;
    let passes = passes.max(1);
    (0..3)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..passes {
                encode_pass(input, config, threads);
            }
            (input.len() * passes) as f64 / start.elapsed().as_secs_f64()
        })
        .fold(0.0, f64::max)
}

/// How fast the encoder is: each reference input encoded with the
/// encoder's own decisions, at the reference's settings, on one thread and
/// on every core -- against `LIBVPX_ENCODE` and `LIBVPX_ENCODE_TILES`. Only
/// a picture of several tile columns encodes on more than one thread.
///
/// The figures for every core need an idle machine. A frame meets its
/// threads three times -- the columns' coding, the loop filter, the columns'
/// bitstreams -- and on a loaded machine each meeting waits for a thread the
/// scheduler has not run yet: with other builds running, two threads have
/// measured slower than one while the process got under half a core.
#[test]
#[ignore = "measurement benchmark; run explicitly with --release --ignored --nocapture"]
fn bench_vp9_encode() {
    let first = shown("vp90-2-22-svc_1280x720_1.webm", 30)
        .expect("the full suite is not fetched: python gui/video/vp9/tools/fetch_vectors.py");
    let cut = common::cut_reference_input(shown).unwrap();
    let small = common::small_reference_input(shown).unwrap();
    let size = |w: usize, h: usize| (w as u32, h as u32);
    let (cw, ch) = size(common::CUT_WIDTH, common::CUT_HEIGHT);
    let (sw, sh) = size(common::SMALL_WIDTH, common::SMALL_HEIGHT);
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    // The references' settings: one tile column, as vpxenc made them.
    let one_column = |w: u32, h: u32, kbps: u32| EncoderConfig {
        tile_columns: 0,
        ..EncoderConfig::realtime(w, h, kbps)
    };
    let runs = [
        (&first, one_column(1280, 720, 1000)),
        (&cut, one_column(cw, ch, 600)),
        (&small, one_column(sw, sh, 200)),
    ];
    std::println!(
        "frames per second; libvpx's as measured on the machine `LIBVPX` names\n\
         {:<24} {:>8} {:>9} {:>9}",
        "reference",
        "port",
        "libvpx C",
        "SIMD"
    );
    for ((input, config), (name, c, simd)) in runs.into_iter().zip(LIBVPX_ENCODE) {
        let port = encode_fps(input, config, 1);
        std::println!("{name:<24} {port:>8.1} {c:>9.1} {simd:>9.1}");
    }
    let tiled = [
        (
            &first,
            EncoderConfig {
                tile_columns: 2,
                ..EncoderConfig::realtime(1280, 720, 1000)
            },
        ),
        (
            &cut,
            EncoderConfig {
                tile_columns: 1,
                ..EncoderConfig::realtime(cw, ch, 600)
            },
        ),
    ];
    std::println!(
        "\nin tile columns, on one thread and on {cores} (the port), on one and on \
         one per column (libvpx)\n\
         {:<24} {:>8} {:>8} {:>9} {:>9} {:>9}",
        "reference",
        "port 1",
        "port all",
        "libvpx C",
        "SIMD 1",
        "SIMD all"
    );
    for ((input, config), (name, c, simd, simd_all)) in tiled.into_iter().zip(LIBVPX_ENCODE_TILES) {
        let one = encode_fps(input, config, 1);
        let all = encode_fps(input, config, cores);
        std::println!("{name:<24} {one:>8.1} {all:>8.1} {c:>9.1} {simd:>9.1} {simd_all:>9.1}");
    }
}
