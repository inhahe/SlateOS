//! Print what this crate's shaper does, so another shaper can be asked the
//! same question.
//!
//! This is one half of the HarfBuzz cross-check; the other half is
//! `tools/harfbuzz_sweep.py`, which drives this and compares. It exists as a
//! committed example rather than a scratch script because the oracle has
//! already found two bugs that 250-odd green unit tests could not, both for
//! the same reason: "this face has no glyph for that" is a *legal* answer, so
//! no self-consistency check can tell it apart from the truth. An oracle that
//! only exists in someone's temp directory is one that is not run again.
//!
//! # Input
//!
//! One UTF-8 file, named on the command line:
//!
//! ```text
//! <n>
//! <string 1>
//! ...
//! <string n>
//! <font path 1>
//! <font path 2>
//! ...
//! ```
//!
//! Strings are one per line and so cannot contain a newline, which no shaping
//! question needs. `\uXXXX` escapes are expanded, so the corpus file stays
//! readable when it is full of combining marks.
//!
//! # Output
//!
//! One tab-separated line per (font, string) pair that shaped to anything:
//!
//! ```text
//! <font path>\t<string index>\t<gid>,<gid>,...
//! ```
//!
//! A font that fails to open is reported on stderr and skipped, since the
//! point of the sweep is the faces that *do* open.

// A tool, not production code: a panic here is a failed diagnostic run, which
// is exactly the outcome that should be loud.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::fmt::Write as _;
use std::io::{self, BufWriter, Write as _};
use std::{env, fs, process};

use osfont::scaled::ScaledFont;

fn main() {
    let Some(input) = env::args().nth(1) else {
        eprintln!("usage: shape_dump <input-file>");
        process::exit(2);
    };
    let text = match fs::read_to_string(&input) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("{input}: {err}");
            process::exit(2);
        }
    };

    let mut lines = text.lines();
    let count: usize = lines
        .next()
        .and_then(|n| n.trim().parse().ok())
        .expect("first line must be the number of corpus strings");
    let corpus: Vec<String> = lines.by_ref().take(count).map(unescape).collect();
    assert_eq!(corpus.len(), count, "corpus is shorter than it claims");

    let out = io::stdout();
    let mut out = BufWriter::new(out.lock());
    for path in lines.filter(|p| !p.trim().is_empty()) {
        let Ok(data) = fs::read(path) else {
            eprintln!("{path}: unreadable");
            continue;
        };
        // A size is needed to build a `ScaledFont` but not to shape: shaping
        // reads `cmap`, `GSUB` and `GPOS`, none of which is scaled. Any size
        // gives the same glyph ids.
        let Ok(font) = ScaledFont::from_bytes(data, 16.0) else {
            eprintln!("{path}: will not open");
            continue;
        };
        for (i, string) in corpus.iter().enumerate() {
            let run = font.shape(string);
            let mut gids = String::new();
            for (n, glyph) in run.glyphs().iter().enumerate() {
                if n > 0 {
                    gids.push(',');
                }
                write!(gids, "{}", glyph.key.raw()).unwrap();
            }
            writeln!(out, "{path}\t{i}\t{gids}").unwrap();
        }
    }
    out.flush().unwrap();
}

/// Expand `\uXXXX` and `\\`, so a corpus of combining marks is still legible.
fn unescape(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                let cp = u32::from_str_radix(&hex, 16)
                    .unwrap_or_else(|_| panic!("bad \\u escape: {hex:?}"));
                out.push(char::from_u32(cp).unwrap_or(char::REPLACEMENT_CHARACTER));
            }
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}
