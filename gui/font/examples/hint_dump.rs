//! Print where this crate's auto-hinter puts each glyph's points, so that
//! FreeType's auto-hinter can be asked the same question.
//!
//! This is one half of the FreeType cross-check; the other half is
//! `tools/hint_oracle.py`, which drives this and compares. The hinter in
//! `src/hint/` is a port of FreeType's, so the two must agree point for
//! point, and a disagreement is a porting mistake -- the kind no unit test
//! finds, because every position the hinter can produce is a legal one.
//!
//! # Input
//!
//! ```text
//! hint_dump [--var <tag>=<value>,...] <font file> <sizes> [<glyph id>...]
//! ```
//!
//! `<sizes>` is a comma-separated list of pixel sizes. Without glyph ids,
//! every glyph of the face is printed. `--var` picks a variable font's
//! instance by user-space axis values (`wght=700,wdth=87.5`), leaving the
//! axes it does not name at their defaults.
//!
//! # Output
//!
//! One line per (size, glyph):
//!
//! ```text
//! <px> <gid> <style> <nonbase> <n> <x>,<y>,<on> ...
//! ```
//!
//! `<style>` is FreeType's name for the style the glyph was sorted into
//! (`latn_dflt`, `hani_dflt`, ...), `<nonbase>` is 1 for a glyph never snapped
//! to a zone, and each point is in 1/64 pixel, y up, `<on>` 1 for an on-curve
//! point. A glyph drawn unhinted has `<n>` 0 and no points.

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

use osfont::raster::Rendering;
use osfont::scaled::ScaledFont;
use osfont::sfnt::Face;

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    // `--var tag=value,...`: a variable font's instance, in user-space axis
    // values, the axes not named at their defaults.
    let mut variations: Vec<([u8; 4], f32)> = Vec::new();
    if let Some(i) = args.iter().position(|a| a == "--var") {
        let spec = args
            .get(i + 1)
            .cloned()
            .unwrap_or_else(|| panic!("--var needs tag=value[,tag=value...]"));
        args.drain(i..=i + 1);
        for pair in spec.split(',') {
            let (tag, value) = pair
                .split_once('=')
                .unwrap_or_else(|| panic!("not tag=value: {pair:?}"));
            let tag: [u8; 4] = tag
                .as_bytes()
                .try_into()
                .unwrap_or_else(|_| panic!("not a four-letter axis tag: {tag:?}"));
            let value = value
                .parse()
                .unwrap_or_else(|_| panic!("not an axis value: {value:?}"));
            variations.push((tag, value));
        }
    }
    let mut args = args.into_iter();
    let (Some(path), Some(sizes)) = (args.next(), args.next()) else {
        eprintln!("usage: hint_dump [--var tag=value,...] <font> <px>[,<px>...] [<gid>...]");
        process::exit(2);
    };
    let sizes: Vec<f32> = sizes
        .split(',')
        .map(|s| {
            s.parse()
                .unwrap_or_else(|_| panic!("not a pixel size: {s:?}"))
        })
        .collect();
    let gids: Vec<u16> = args
        .map(|g| {
            g.parse()
                .unwrap_or_else(|_| panic!("not a glyph id: {g:?}"))
        })
        .collect();
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let face = Face::parse(bytes).unwrap_or_else(|e| panic!("{path}: {e:?}"));
    let gids = if gids.is_empty() {
        (0..face.num_glyphs()).collect()
    } else {
        gids
    };
    let coords = (!variations.is_empty()).then(|| {
        face.variation_axes()
            .unwrap_or_else(|| panic!("{path}: --var for a face with no variation axes"))
            .normalize_tags(&variations)
    });
    let face = std::sync::Arc::new(face);
    let out = io::stdout();
    let mut out = BufWriter::new(out.lock());
    for px in sizes {
        let mut font = ScaledFont::shared(face.clone(), px).expect("a valid size");
        if let Some(coords) = &coords {
            font.set_variations(coords.clone());
        }
        font.set_rendering(Rendering {
            hinting: true,
            ..Rendering::default()
        });
        for &gid in &gids {
            let (style, nonbase) = font.hint_style(gid).unwrap_or(("-", false));
            let points = font.hinted_points(gid).unwrap_or_default();
            let mut line = format!("{px} {gid} {style} {} {}", u8::from(nonbase), points.len());
            for (x, y, on) in points {
                write!(
                    line,
                    " {:.2},{},{}",
                    x * 64.0,
                    (y * 64.0).round(),
                    u8::from(on)
                )
                .unwrap();
            }
            writeln!(out, "{line}").unwrap();
        }
    }
    out.flush().unwrap();
}
