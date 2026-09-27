//! Print glyph outlines and boxes as this crate draws them at a variable
//! font's instances, so that `tools/outline_oracle.py` can ask HarfBuzz for
//! the same.
//!
//! Needs the `testing` feature, which is how the boxes -- HarfBuzz's
//! rounding of them -- are reached (`osfont::testing::harfbuzz_extents`).
//!
//! # Input
//!
//! ```text
//! outline_dump <font file> <first gid> <last gid> [<tag>=<value>[,...]] ...
//! ```
//!
//! Each argument after the glyph range is one instance, in user-space axis
//! values; none is the default instance only.
//!
//! # Output
//!
//! One line per glyph per instance: the instance's index, the glyph id, its
//! box (`x_bearing y_bearing width height`, or `-` for none), and its path
//! as `M x y`, `L x y`, `Q cx cy x y`, `C c1x c1y c2x c2y x y` and `Z`, each
//! coordinate the `f32` the outline holds, printed to round-trip.

// A tool, not production code: a panic here is a failed diagnostic run, which
// is exactly the outcome that should be loud.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{self, BufWriter, Write as _};
use std::{env, fs, process};

use osfont::sfnt::{Face, PathCmd};
use osfont::var::Coords;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let [path, first, last, instances @ ..] = args.as_slice() else {
        eprintln!("usage: outline_dump <font> <first gid> <last gid> [tag=value,...] ...");
        process::exit(2);
    };
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let face = Face::parse(bytes).unwrap_or_else(|e| panic!("{path}: {e:?}"));
    let first: u16 = first.parse().expect("a glyph id");
    let last: u16 = last.parse().expect("a glyph id");
    let mut positions = vec![Coords::default()];
    if let Some(axes) = face.variation_axes() {
        positions = instances
            .iter()
            .map(|spec| axes.normalize_tags(&parse(spec)))
            .collect();
        if positions.is_empty() {
            positions.push(axes.default_coords());
        }
    }
    let out = io::stdout();
    let mut out = BufWriter::new(out.lock());
    for (n, coords) in positions.iter().enumerate() {
        for gid in first..=last {
            let extents = osfont::testing::harfbuzz_extents(&face, gid, coords).map_or_else(
                || "-".to_string(),
                |e| format!("{} {} {} {}", e[0], e[1], e[2], e[3]),
            );
            let path = match face.outline_at(gid, coords) {
                Ok(outline) => path_text(&outline.commands),
                Err(e) => format!("error {e:?}"),
            };
            writeln!(out, "{n}\t{gid}\t{extents}\t{path}").unwrap();
        }
    }
    out.flush().unwrap();
}

fn parse(spec: &str) -> Vec<([u8; 4], f32)> {
    spec.split(',')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (tag, value) = pair
                .split_once('=')
                .unwrap_or_else(|| panic!("not tag=value: {pair:?}"));
            let tag: [u8; 4] = tag
                .as_bytes()
                .try_into()
                .unwrap_or_else(|_| panic!("not a four-letter tag: {tag:?}"));
            (tag, value.parse().expect("a value"))
        })
        .collect()
}

fn path_text(commands: &[PathCmd]) -> String {
    let mut s = String::new();
    for cmd in commands {
        let part = match *cmd {
            PathCmd::MoveTo(p) => format!("M {:?} {:?}", p.x, p.y),
            PathCmd::LineTo(p) => format!("L {:?} {:?}", p.x, p.y),
            PathCmd::QuadTo(c, p) => format!("Q {:?} {:?} {:?} {:?}", c.x, c.y, p.x, p.y),
            PathCmd::CurveTo(a, b, p) => {
                format!(
                    "C {:?} {:?} {:?} {:?} {:?} {:?}",
                    a.x, a.y, b.x, b.y, p.x, p.y
                )
            }
            PathCmd::Close => "Z".to_string(),
        };
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&part);
    }
    s
}
