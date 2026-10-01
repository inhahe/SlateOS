//! Inflate speed, measured on demand: `INFLATE_BENCH=<zlib stream> cargo test
//! -p deflate --release --test bench -- --ignored --nocapture`.
//!
//! Ignored by default because a timing is only worth reading in a release
//! build, on a quiet machine, next to a reference timed at the same moment --
//! `requests/f-ab-inflate-decodes-a-bit-at-a-time-five-times-slower-than-zlib-ng.md`
//! times zlib-ng through Python on the same stream. It reports the best of
//! 25 runs of the one-shot decoder and of the stream read a PNG row (6001
//! bytes) at a time, and checks both produced the same bytes.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a benchmark that cannot read its input has nothing to say"
)]

use std::time::{Duration, Instant};

fn best_of<T>(runs: usize, mut f: impl FnMut() -> T) -> (Duration, T) {
    let mut best = Duration::MAX;
    let mut last = None;
    for _ in 0..runs {
        let t = Instant::now();
        let v = f();
        best = best.min(t.elapsed());
        last = Some(v);
    }
    (best, last.unwrap())
}

#[test]
#[ignore = "a timing, run on demand with INFLATE_BENCH set"]
fn inflate_speed() {
    let Ok(path) = std::env::var("INFLATE_BENCH") else {
        println!("INFLATE_BENCH is not set: nothing to time");
        return;
    };
    let data = std::fs::read(&path).unwrap();
    let (one_shot, whole) = best_of(25, || deflate::zlib_inflate(&data).unwrap());
    let (streamed, rows) = best_of(25, || {
        let mut s = deflate::zlib_inflate_stream(&data, whole.len()).unwrap();
        let mut row = vec![0u8; 6001];
        let mut out = Vec::with_capacity(whole.len());
        loop {
            let n = s.read(&mut row).unwrap();
            if n == 0 {
                break out;
            }
            out.extend_from_slice(&row[..n]);
        }
    });
    assert!(
        rows == whole,
        "the stream and the one-shot decoder disagree"
    );
    // Where the one-shot time goes: the DEFLATE payload alone, and the
    // Adler-32 of the output.
    let payload = &data[2..data.len() - 4];
    let (raw, _) = best_of(25, || deflate::inflate(payload).unwrap());
    let (adler, _) = best_of(25, || deflate::adler32(&whole));
    println!(
        "  of which: raw inflate {:.1} ms, adler32 {:.1} ms",
        raw.as_secs_f64() * 1000.0,
        adler.as_secs_f64() * 1000.0
    );
    println!(
        "{} compressed bytes -> {} bytes: one-shot {:.1} ms, stream by 6001-byte rows {:.1} ms",
        data.len(),
        whole.len(),
        one_shot.as_secs_f64() * 1000.0,
        streamed.as_secs_f64() * 1000.0
    );
}
