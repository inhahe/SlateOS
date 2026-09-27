//! Print where this crate normalizes a variable font's instances -- as
//! HarfBuzz does (`F2Dot14`) and as FreeType does (16.16) -- so that
//! `tools/var_oracle.py` can ask both libraries the same question.
//!
//! Needs the `testing` feature, which is how the FreeType reading is reached
//! (`osfont::testing::freetype_coords`).
//!
//! # Input
//!
//! ```text
//! var_dump <font file> <tag>=<value>[,<tag>=<value>...] ...
//! ```
//!
//! Each further argument is one instance, in user-space axis values; an
//! axis it does not name sits at its default.
//!
//! # Output
//!
//! One line per instance: the `F2Dot14` coordinates, a `|`, and the 16.16
//! ones, each in `fvar` axis order.

// A tool, not production code: a panic here is a failed diagnostic run, which
// is exactly the outcome that should be loud.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{self, BufWriter, Write as _};
use std::{env, fs, process};

use osfont::sfnt::Face;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some((path, instances)) = args.split_first() else {
        eprintln!("usage: var_dump <font> <tag>=<value>[,...] ...");
        process::exit(2);
    };
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let face = Face::parse(bytes).unwrap_or_else(|e| panic!("{path}: {e:?}"));
    let axes = face
        .variation_axes()
        .unwrap_or_else(|| panic!("{path}: not a variable font"));
    let out = io::stdout();
    let mut out = BufWriter::new(out.lock());
    for spec in instances {
        let wanted: Vec<([u8; 4], f32)> = spec
            .split(',')
            .filter(|p| !p.is_empty())
            .map(|pair| {
                let (tag, value) = pair
                    .split_once('=')
                    .unwrap_or_else(|| panic!("not tag=value: {pair:?}"));
                let tag: [u8; 4] = tag
                    .as_bytes()
                    .try_into()
                    .unwrap_or_else(|_| panic!("not a four-letter tag: {tag:?}"));
                (
                    tag,
                    value
                        .parse()
                        .unwrap_or_else(|_| panic!("not a value: {value:?}")),
                )
            })
            .collect();
        let coords = axes.normalize_tags(&wanted);
        let norm: Vec<String> = coords.as_slice().iter().map(i16::to_string).collect();
        let fixed: Vec<String> = osfont::testing::freetype_coords(&coords)
            .iter()
            .map(i32::to_string)
            .collect();
        writeln!(out, "{} | {}", norm.join(" "), fixed.join(" ")).unwrap();
    }
    out.flush().unwrap();
}
