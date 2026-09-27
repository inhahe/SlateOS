//! Time `ScaledFont::wrap` against the word-by-word rule it replaced
//! (`osfont::testing::wrap_by_words`), on the case lane E measured: 600 words,
//! about 5,000 characters, wrapped into 1,136 px at 14 px.
//!
//! ```text
//! wrap_time <font file> [px] [width]
//! ```
//!
//! Needs the `testing` feature for the old rule. Prints each's time, best of
//! five, and whether they gave the same lines.

// A tool, not production code: a panic is a failed run, which should be loud.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::{Duration, Instant};
use std::{env, fs, process};

use osfont::scaled::ScaledFont;
use osfont::sfnt::Face;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: wrap_time <font file> [px] [width]");
        process::exit(2);
    };
    let px: f32 = args.get(1).map_or(14.0, |a| a.parse().expect("a size"));
    let width: f32 = args.get(2).map_or(1136.0, |a| a.parse().expect("a width"));
    let face = Face::parse(fs::read(path).expect("the font")).expect("a font");
    let font = ScaledFont::new(face, px).expect("a size");
    let words = [
        "the",
        "quick",
        "brown",
        "fox",
        "jumps",
        "over",
        "lazy",
        "dog",
        "while",
        "wrapping",
        "paragraphs",
        "of",
        "ordinary",
        "prose",
        "at",
        "fourteen",
        "pixels",
        "into",
        "a",
        "wide",
        "column",
    ];
    let text: Vec<&str> = words.iter().cycle().take(600).copied().collect();
    let text = text.join(" ");

    let best = |f: &dyn Fn() -> Vec<String>| {
        let mut best = Duration::MAX;
        let mut lines = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            lines = f();
            best = best.min(start.elapsed());
        }
        (best, lines)
    };
    let (fast, fast_lines) = best(&|| font.wrap(&text, width));
    let (slow, slow_lines) =
        best(&|| osfont::testing::wrap_by_words(&text, width, &|s| font.measure(s)));
    println!(
        "{} characters, {} lines: wrap {:?}, word by word {:?} ({:.1}x); same lines: {}",
        text.len(),
        fast_lines.len(),
        fast,
        slow,
        slow.as_secs_f64() / fast.as_secs_f64().max(1e-9),
        fast_lines == slow_lines
    );
}
