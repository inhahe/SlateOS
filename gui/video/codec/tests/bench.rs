//! How fast a file plays through [`Video`]: frames a second, decoding and
//! converting, against decoding alone -- so that the conversion's share of
//! a frame's time is in plain view.
//!
//! Not a pass/fail test: a measurement, run explicitly --
//!
//! ```text
//! python gui/video/vp9/tools/fetch_vectors.py
//! cargo test -p videocodec --target x86_64-pc-windows-gnu --release --test bench -- --ignored --nocapture
//! ```
//!
//! -- on two of libvpx's 1080p test vectors, fetched into
//! `target/vp9vectors`: a film in one tile column (so the VP9 decoder runs
//! on one thread, and the frame rate is a single core's), and one in 4x4
//! tiles, which the decoder's threads share. Each is played through whole,
//! three times, and the fastest kept. One thread converts; the decoder uses
//! the machine's cores. The conversion alone is `gui/video/yuv`'s
//! `tests/bench.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::arithmetic_side_effects,
    reason = "a measurement: a missing vector or an error should stop it loudly, and it counts frames"
)]

use std::fs::File;
use std::path::PathBuf;
use std::time::Instant;

use videocodec::Video;

/// The fetched vectors' directory, as `fetch_vectors.py` makes it.
fn vectors() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../target/vp9vectors")
        .canonicalize()
        .unwrap_or_default()
}

/// Frames a second playing `path` through: with every picture converted,
/// or (`convert` false) only decoded. The fastest of three plays.
fn fps(path: &PathBuf, convert: bool) -> (f64, usize) {
    let mut best = 0.0f64;
    let mut frames = 0;
    for _ in 0..3 {
        let mut video = Video::open(File::open(path).unwrap()).unwrap();
        let start = Instant::now();
        frames = 0;
        if convert {
            while video.next_frame().unwrap().is_some() {
                frames += 1;
            }
        } else {
            while video.next_picture().unwrap().is_some() {
                frames += 1;
            }
        }
        best = best.max(frames as f64 / start.elapsed().as_secs_f64());
    }
    (best, frames)
}

#[test]
#[ignore = "a measurement: run with --release --ignored --nocapture"]
fn bench_play() {
    let dir = vectors();
    for name in [
        "vp90-2-02-size-lf-1920x1080.webm",
        "vp90-2-08-tile-4x4.webm",
    ] {
        let path = dir.join(name);
        if !path.is_file() {
            println!("{name}: not fetched (python gui/video/vp9/tools/fetch_vectors.py)");
            continue;
        }
        let (decoded, frames) = fps(&path, false);
        let (shown, _) = fps(&path, true);
        let per_frame = 1000.0 / shown - 1000.0 / decoded;
        println!(
            "{name}: {frames} frames; decoded {decoded:.1} a second, decoded and \
             converted {shown:.1} -- the conversion {per_frame:.1} ms a frame"
        );
    }
}
