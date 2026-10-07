//! How fast this crate compresses and decompresses, preset by preset, on
//! this machine, in the configuration it ships in (the release profile).
//!
//! The archive manager writes every TAR.XZ at `xz -6` -- up to its 512 MiB
//! budget -- and the kernel's `fcompress` is to use the same encoder, so the
//! encoder's speed is a cost users wait on. The figure to hold it to is XZ
//! Utils 5.2.5's own, single-threaded, on the same input: the port should not
//! be much slower than what it was ported from.
//!
//! Run with `cargo bench -p xz --target x86_64-pc-windows-gnu`. The input is
//! the tests' word salad (`tests/encode.rs`'s `text`), 8 MiB of it, and 2 MiB
//! of random bytes, which LZMA2 stores; `python xz/benches/compare.py` runs
//! `xz` 5.2.5 over the same bytes for the comparison.

// A benchmark over inputs it builds itself: the sizes are constants and the
// word index is masked to the table.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use std::hint::black_box;
use std::time::{Duration, Instant};

/// The tests' generator (`tests/data/generate.py`'s `Rng`).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

/// `generate.py`'s `text`: words and line breaks.
fn text(n: usize, seed: u64) -> Vec<u8> {
    const WORDS: [&[u8]; 8] = [
        b"alpha", b"beta", b"gamma", b"delta", b"epsilon", b"zeta", b"eta", b"theta",
    ];
    let mut r = Rng(seed);
    let mut out = Vec::with_capacity(n + 16);
    while out.len() < n {
        out.extend_from_slice(WORDS[(r.next() % 8) as usize]);
        out.push(if r.next().is_multiple_of(9) {
            b'\n'
        } else {
            b' '
        });
    }
    out.truncate(n);
    out
}

/// `generate.py`'s `random`.
fn random(n: usize, seed: u64) -> Vec<u8> {
    let mut r = Rng(seed);
    (0..n).map(|_| (r.next() & 0xff) as u8).collect()
}

/// The fastest of `runs` timings of `f`.
fn best(runs: u32, mut f: impl FnMut() -> usize) -> (Duration, usize) {
    let mut fastest = Duration::MAX;
    let mut out = 0;
    for _ in 0..runs {
        let started = Instant::now();
        out = black_box(f());
        fastest = fastest.min(started.elapsed());
    }
    (fastest, out)
}

fn mib_per_s(bytes: usize, t: Duration) -> f64 {
    bytes as f64 / (1024.0 * 1024.0) / t.as_secs_f64()
}

fn main() {
    let inputs = [
        ("text, 8 MiB", text(8 << 20, 1)),
        ("random, 2 MiB", random(2 << 20, 2)),
    ];
    println!("| input | preset | compress MiB/s | ratio | decompress MiB/s |");
    println!("|---|---|---|---|---|");
    for (name, data) in &inputs {
        for level in [0, 1, 6, 9] {
            let preset = xz::Preset::new(level).unwrap_or(xz::Preset::DEFAULT);
            let (t_c, _) = best(2, || xz::compress(data, preset).len());
            let packed = xz::compress(data, preset);
            let (t_d, _) = best(3, || xz::decompress(&packed).map_or(0, |d| d.len()));
            println!(
                "| {name} | -{level} | {:.1} | {:.3} | {:.1} |",
                mib_per_s(data.len(), t_c),
                packed.len() as f64 / data.len() as f64,
                mib_per_s(data.len(), t_d)
            );
        }
    }
}
