//! How long converting one 1920x1080 picture to pixels takes, by each path
//! `reformat` can take -- the cost every frame of HD video pays on its way to
//! the screen.
//!
//! Not a pass/fail test: a measurement, run explicitly --
//!
//! ```text
//! cargo test -p yuv --target x86_64-pc-windows-gnu --release --test bench -- --ignored --nocapture
//! ```
//!
//! -- printing milliseconds a frame for each path, the fastest of three runs
//! of repeated conversions lasting half a second each. The planes are
//! pseudo-random samples, so no path's work is cut short by its content.
//!
//! libavif's own conversion of the same 8-bit 4:2:0 BT.709 picture, for
//! comparison: `LIBAVIF` below, measured with
//! `gui/video/codec/tools/libavif_reformat_reference.c` on fifty frames,
//! linked against libyuv's C (as this port follows) and against libyuv's x86
//! SIMD (what Chrome and Linux builds run).

#![allow(
    clippy::unwrap_used,
    clippy::cast_precision_loss,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a measurement: a failure should stop it loudly"
)]

use std::time::{Duration, Instant};

use yuv::reformat::{self, Format, Picture, Reformat};
use yuv::{Plane, PlaneBuf};

const WIDTH: usize = 1920;
const HEIGHT: usize = 1080;

/// libavif 1.3.0's `avifImageYUVToRGB` on a 1920x1080 8-bit 4:2:0 BT.709
/// limited-range picture, milliseconds a frame: (libyuv's C, libyuv's x86
/// SIMD). The best of three runs of fifty frames, measured 2026-10-04 on an
/// Intel Core i7-8700K (3.7 GHz; WSL 2 Ubuntu, gcc 13, -O2), one thread,
/// while other work shared the machine; each includes about 0.8 ms of the
/// harness reading its input.
const LIBAVIF: (f64, f64) = (18.2, 5.9);

/// A sample size, made from the top bits of a random word.
trait Noise: Reformat {
    fn from_bits(v: u32) -> Self;
}

impl Noise for u8 {
    fn from_bits(v: u32) -> Self {
        v as Self
    }
}

impl Noise for u16 {
    fn from_bits(v: u32) -> Self {
        v as Self
    }
}

/// `width` x `height` samples from xorshift32, each the top `depth` bits.
fn noise<T: Noise>(width: usize, height: usize, depth: u32, seed: u32) -> PlaneBuf<T> {
    let mut s = seed | 1;
    let mut plane = PlaneBuf::new(width, height).unwrap();
    for sample in &mut plane.samples {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        *sample = T::from_bits(s >> (32 - depth));
    }
    plane
}

/// Milliseconds to convert `picture` once: the fastest of three runs, each
/// as many conversions as fill half a second.
fn millis<T: Reformat>(picture: &Picture<'_, T>) -> f64 {
    let mut out = Vec::new();
    reformat::to_argb_into(picture, &mut out).unwrap();
    let mut best = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        let mut n = 0u32;
        while start.elapsed() < Duration::from_millis(500) {
            reformat::to_argb_into(picture, &mut out).unwrap();
            n += 1;
        }
        best = best.min(start.elapsed().as_secs_f64() * 1000.0 / f64::from(n));
    }
    best
}

struct Case {
    name: &'static str,
    format: Format,
    depth: u8,
    matrix: u16,
    alpha: bool,
}

const CASES: [Case; 8] = [
    Case {
        name: "8-bit 4:2:0 BT.709 (libyuv, bilinear)",
        format: Format::Yuv420,
        depth: 8,
        matrix: 1,
        alpha: false,
    },
    Case {
        name: "8-bit 4:2:0 BT.709 with alpha",
        format: Format::Yuv420,
        depth: 8,
        matrix: 1,
        alpha: true,
    },
    Case {
        name: "8-bit 4:2:2 BT.709 (libyuv, linear)",
        format: Format::Yuv422,
        depth: 8,
        matrix: 1,
        alpha: false,
    },
    Case {
        name: "8-bit 4:4:4 BT.709 (libyuv)",
        format: Format::Yuv444,
        depth: 8,
        matrix: 1,
        alpha: false,
    },
    Case {
        name: "10-bit 4:2:0 BT.2020 (libyuv)",
        format: Format::Yuv420,
        depth: 10,
        matrix: 9,
        alpha: false,
    },
    Case {
        name: "12-bit 4:2:0 BT.709 (libyuv, nearest)",
        format: Format::Yuv420,
        depth: 12,
        matrix: 1,
        alpha: false,
    },
    Case {
        name: "8-bit 4:4:4 FCC (libavif's fast path)",
        format: Format::Yuv444,
        depth: 8,
        matrix: 4,
        alpha: false,
    },
    Case {
        name: "8-bit 4:2:0 SMPTE 240M (libavif's slow path)",
        format: Format::Yuv420,
        depth: 8,
        matrix: 7,
        alpha: false,
    },
];

fn chroma(format: Format) -> (usize, usize) {
    match format {
        Format::Yuv444 | Format::Yuv400 => (WIDTH, HEIGHT),
        Format::Yuv422 => (WIDTH / 2, HEIGHT),
        Format::Yuv420 => (WIDTH / 2, HEIGHT / 2),
    }
}

fn run<T: Noise>(case: &Case) -> f64 {
    let depth = u32::from(case.depth);
    let (cw, ch) = chroma(case.format);
    let y: PlaneBuf<T> = noise(WIDTH, HEIGHT, depth, 1);
    let u: PlaneBuf<T> = noise(cw, ch, depth, 2);
    let v: PlaneBuf<T> = noise(cw, ch, depth, 3);
    let a: PlaneBuf<T> = noise(WIDTH, HEIGHT, depth, 4);
    let alpha: Option<Plane<'_, T>> = case.alpha.then(|| a.view());
    millis(&Picture {
        width: WIDTH,
        height: HEIGHT,
        depth: case.depth,
        format: case.format,
        matrix: case.matrix,
        primaries: 1,
        full_range: false,
        y: y.view(),
        u: Some(u.view()),
        v: Some(v.view()),
        alpha,
        alpha_premultiplied: false,
    })
}

#[test]
#[ignore = "a measurement: run with --release --ignored --nocapture"]
fn bench_reformat() {
    println!("1920x1080 to 0xAARRGGBB, milliseconds a frame (one thread):");
    for case in &CASES {
        let ms = if case.depth == 8 {
            run::<u8>(case)
        } else {
            run::<u16>(case)
        };
        println!("  {ms:7.2}  {}", case.name);
    }
    println!(
        "  libavif, 8-bit 4:2:0 BT.709: {:.2} (libyuv's C), {:.2} (libyuv's x86 SIMD)",
        LIBAVIF.0, LIBAVIF.1
    );
}
