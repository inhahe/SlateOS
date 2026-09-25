//! Time a decode, and optionally dump the pixels for a before/after comparison:
//! `cargo run --release -p imagecodec --example time_decode -- <file> [dump-prefix]`.
//!
//! A developer's tool, not a test: it prints timings and writes files, and it
//! is how the decoder's before/after comparisons were taken (see
//! `jpeg::decode_scaled`'s *Measured*).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects
)]
fn dump(prefix: &str, name: &str, img: &imagecodec::Image) {
    let mut out = Vec::with_capacity(img.pixels.len() * 4 + 8);
    out.extend_from_slice(&img.width.to_le_bytes());
    out.extend_from_slice(&img.height.to_le_bytes());
    for p in &img.pixels {
        out.extend_from_slice(&p.to_le_bytes());
    }
    std::fs::write(format!("{prefix}-{name}.bin"), out).unwrap();
}
fn main() {
    let path = std::env::args().nth(1).expect("a file");
    let prefix = std::env::args().nth(2);
    let bytes = std::fs::read(&path).expect("read");
    let limits = imagecodec::Limits {
        max_pixels: 100_000_000,
        max_decompressed_bytes: 1 << 31,
    };
    let t = std::time::Instant::now();
    let img = imagecodec::decode(&bytes, limits).expect("decode");
    println!("decode {}x{} in {:?}", img.width, img.height, t.elapsed());
    if imagecodec::gif::is_gif(&bytes) {
        // Every frame of an animation, composited, as a viewer plays it.
        let t = std::time::Instant::now();
        let mut animation = imagecodec::gif::Animation::new(&bytes, limits).expect("animation");
        let mut frames = 0u32;
        while animation.next_frame().expect("frame").is_some() {
            frames += 1;
        }
        println!("animation: {frames} frames in {:?}", t.elapsed());
    }
    if let Some(p) = &prefix {
        dump(p, "full", &img);
    }
    // Eighth scale (for a large photograph, the first three), then a quarter and
    // a half of a 4000x5333 one: each of the reduced transforms.
    for (w, h) in [
        (128u32, 128u32),
        (400, 300),
        (97, 61),
        (1000, 1334),
        (2000, 2667),
    ] {
        let t = std::time::Instant::now();
        let img = imagecodec::decode_scaled(&bytes, limits, w, h).expect("scaled");
        println!(
            "decode_scaled({w},{h}) -> {}x{} in {:?}",
            img.width,
            img.height,
            t.elapsed()
        );
        if let Some(p) = &prefix {
            dump(p, &format!("s{w}x{h}"), &img);
        }
    }
}
