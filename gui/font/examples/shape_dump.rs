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
//! <language 1>\t<string 1>
//! ...
//! <language n>\t<string n>
//! <font path 1>
//! <font path 2>
//! ...
//! ```
//!
//! Strings are one per line and so cannot contain a newline, which no shaping
//! question needs. `\uXXXX` escapes are expanded, so the corpus file stays
//! readable when it is full of combining marks.
//!
//! The language is a BCP 47 tag — `tr`, `sr`, `ro-MD` — and is empty for text
//! that names no language. It is a field of its own rather than a second
//! corpus because a language is not a property of the *text*: the same Turkish
//! letters shape differently depending only on whether the caller said they
//! were Turkish, and that difference is precisely what needs an oracle. A line
//! with no tab is read as naming no language, so an older corpus file still
//! means what it did.
//!
//! # Output
//!
//! One tab-separated line per (font, string) pair that shaped to anything:
//!
//! ```text
//! <font path>\t<string index>\t<logical gids>\t<visual gids>\t<logical pos>\t<visual pos>
//! ```
//!
//! Both orders, because HarfBuzz produces only one of them and which one
//! depends on the string. It has no bidi in it: `guess_segment_properties`
//! picks a single direction for the whole buffer, so an all-Arabic string
//! comes back reversed and a string of Latin *containing* Arabic comes back
//! in logical order with the Arabic still backwards. Printing both lets the
//! sweep compare against whichever HarfBuzz actually answered instead of
//! writing off every right-to-left string as a difference.
//!
//! A position is `advance;dx;dy`, one per glyph, in the same order as the
//! glyph ids beside it. Glyph ids alone cannot tell a correctly placed mark
//! from one stacked on the wrong base, and cannot see kerning at all — and
//! kerning and mark placement are exactly what reordering a run disturbs,
//! because the pair a kern belongs to swaps ends. The face is built at its
//! own em size so the scale factor is one and these come out in font units,
//! which is what HarfBuzz reports by default.
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

use osfont::lang::Lang;
use osfont::scaled::ScaledFont;
use osfont::shape::ShapedGlyph;

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
    let corpus: Vec<(Option<Lang>, String)> = lines.by_ref().take(count).map(entry).collect();
    assert_eq!(corpus.len(), count, "corpus is shorter than it claims");

    let out = io::stdout();
    let mut out = BufWriter::new(out.lock());
    for path in lines.filter(|p| !p.trim().is_empty()) {
        let Ok(data) = fs::read(path) else {
            eprintln!("{path}: unreadable");
            continue;
        };
        // Any size shapes to the same glyph ids — `cmap`, `GSUB` and `GPOS`
        // are read unscaled — but not to the same *positions*, so the face is
        // opened twice: once at an arbitrary size to ask what its design grid
        // is, then again at exactly that many pixels, where the scale factor
        // is one and every advance is the integer the font actually stores.
        // Anything else would compare rounding, not shaping.
        let Ok(probe) = ScaledFont::from_bytes(data.clone(), 16.0) else {
            eprintln!("{path}: will not open");
            continue;
        };
        let upem = f32::from(probe.units_per_em());
        drop(probe);
        let Ok(font) = ScaledFont::from_bytes(data, upem) else {
            eprintln!("{path}: will not open at {upem}px");
            continue;
        };
        for (i, (lang, string)) in corpus.iter().enumerate() {
            let run = font.shape_lang(string, *lang);
            let (logical, logical_pos) = dump(run.glyphs().iter());
            let (visual, visual_pos) = dump(run.draw_order());
            writeln!(
                out,
                "{path}\t{i}\t{logical}\t{visual}\t{logical_pos}\t{visual_pos}"
            )
            .unwrap();
        }
    }
    out.flush().unwrap();
}

/// One order of a run, as `<gids>` and `<advance;dx;dy per glyph>`.
///
/// The two come out of one walk because they must describe the same glyphs in
/// the same order: a sweep that lined up positions from one order against ids
/// from the other would report a difference on every right-to-left string and
/// be believed, since the numbers would look plausible.
///
/// Positions are rounded to whole font units. The face is built at its own em
/// size, so they are already integers up to floating-point noise, and printing
/// the noise would only invite a comparison to argue about it.
fn dump<'a>(glyphs: impl Iterator<Item = &'a ShapedGlyph>) -> (String, String) {
    let mut ids = String::new();
    let mut pos = String::new();
    for (n, glyph) in glyphs.enumerate() {
        if n > 0 {
            ids.push(',');
            pos.push(',');
        }
        write!(ids, "{}", glyph.key.raw()).unwrap();
        let (dx, dy) = glyph.offset;
        write!(pos, "{:.0};{:.0};{:.0}", glyph.advance, dx, dy).unwrap();
    }
    (ids, pos)
}

/// Split one corpus line into the language it names and the text itself.
///
/// A tag the crate does not recognise is a corpus bug and not a shaping
/// result, so it stops the run rather than quietly shaping as "no language" —
/// which would look exactly like agreement.
fn entry(line: &str) -> (Option<Lang>, String) {
    let (tag, text) = line.split_once('\t').unwrap_or(("", line));
    let lang = (!tag.is_empty())
        .then(|| Lang::new(tag).unwrap_or_else(|| panic!("not a language tag: {tag:?}")));
    (lang, unescape(text))
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
