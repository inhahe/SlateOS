//! Fonts built byte by byte, for other crates' tests.
//!
//! A crate that draws text -- the compositor, the toolkit -- sometimes needs
//! to test what it does with a *kind* of glyph, and a real font file is the
//! wrong fixture for that: large, licensed, and absent from a test machine
//! that has not downloaded it. These are complete, parseable faces, as small
//! as the question they answer.
//!
//! Compiled for this crate's own tests and under the `testing` feature, which
//! a dependent enables in its `[dev-dependencies]` so that it never reaches a
//! shipped build:
//!
//! ```toml
//! [dev-dependencies]
//! osfont = { path = "../font", features = ["testing"] }
//! ```

// A fixture assembles literals; a value that does not fit its field is a bug in
// the fixture, and the panic is the report.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    clippy::cast_possible_truncation
)]

use alloc::vec;
use alloc::vec::Vec;

/// The colour of glyph `A` in [`colour_face`], as `0xAARRGGBB`.
pub const COLOUR_FACE_RED: u32 = 0xFFFF_0000;

/// A TrueType face of three glyphs, 1000 units to the em, 800 up and 200
/// down, every glyph advancing 300 units:
///
/// * glyph 0, `.notdef`: empty;
/// * glyph 1, `A`: a square, x 100 to 200 and y 0 to 100, which a version-0
///   `COLR` table paints in [`COLOUR_FACE_RED`] -- a colour glyph;
/// * glyph 2, `B`: the same square with no colour recipe -- an ordinary
///   outline, drawn in the text colour.
///
/// At 100 pixels to the em each square is 10 pixels a side, its left edge 10
/// pixels right of the pen and its top 10 above the baseline.
#[must_use]
pub fn colour_face() -> Vec<u8> {
    let square = {
        let mut g = Vec::new();
        for v in [1i16, 100, 0, 200, 100] {
            g.extend_from_slice(&v.to_be_bytes());
        }
        // endPtsOfContours, then no instructions.
        g.extend_from_slice(&3u16.to_be_bytes());
        g.extend_from_slice(&0u16.to_be_bytes());
        // Four on-curve points with 16-bit deltas.
        g.extend_from_slice(&[0x01; 4]);
        for v in [100i16, 100, 0, -100, 0, 0, 100, 0] {
            g.extend_from_slice(&v.to_be_bytes());
        }
        g
    };
    let mut glyf = square.clone();
    glyf.extend_from_slice(&square);
    let mut loca = Vec::new();
    for offset in [0u32, 0, square.len() as u32, 2 * square.len() as u32] {
        loca.extend_from_slice(&offset.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    // Long `loca` offsets.
    head[50..52].copy_from_slice(&1i16.to_be_bytes());
    let mut hhea = vec![0u8; 36];
    hhea[4..6].copy_from_slice(&800i16.to_be_bytes());
    hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
    hhea[10..12].copy_from_slice(&300u16.to_be_bytes());
    hhea[34..36].copy_from_slice(&3u16.to_be_bytes());
    let mut maxp = vec![0u8; 6];
    maxp[4..6].copy_from_slice(&3u16.to_be_bytes());
    let mut hmtx = Vec::new();
    for lsb in [0i16, 100, 100] {
        hmtx.extend_from_slice(&300u16.to_be_bytes());
        hmtx.extend_from_slice(&lsb.to_be_bytes());
    }

    // `cmap`: one format-4 subtable, A and B to glyphs 1 and 2.
    let mut cmap = Vec::new();
    for v in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&v.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    for v in [4u16, 32, 0, 4, 4, 1, 0, 0x42, 0xFFFF, 0, 0x41, 0xFFFF] {
        cmap.extend_from_slice(&v.to_be_bytes());
    }
    for v in [1u16.wrapping_sub(0x41), 1, 0, 0] {
        cmap.extend_from_slice(&v.to_be_bytes());
    }

    // `COLR` version 0: glyph 1 is one layer, itself, in palette entry 0.
    let mut colr = Vec::new();
    for v in [0u16, 1] {
        colr.extend_from_slice(&v.to_be_bytes());
    }
    colr.extend_from_slice(&14u32.to_be_bytes());
    colr.extend_from_slice(&20u32.to_be_bytes());
    colr.extend_from_slice(&1u16.to_be_bytes());
    for v in [1u16, 0, 1, 1, 0] {
        colr.extend_from_slice(&v.to_be_bytes());
    }
    // `CPAL`: one palette of one entry, stored blue, green, red, alpha.
    let mut cpal = Vec::new();
    for v in [0u16, 1, 1, 1] {
        cpal.extend_from_slice(&v.to_be_bytes());
    }
    cpal.extend_from_slice(&14u32.to_be_bytes());
    cpal.extend_from_slice(&0u16.to_be_bytes());
    let [a, r, g, b] = COLOUR_FACE_RED.to_be_bytes();
    cpal.extend_from_slice(&[b, g, r, a]);

    assemble(&[
        (*b"COLR", colr),
        (*b"CPAL", cpal),
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
    ])
}

/// An sfnt file around `tables`: the directory, then each table padded to
/// four bytes. Checksums are left zero; nothing here reads them.
fn assemble(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        let pad = (4 - data.len() % 4) % 4;
        body.extend(core::iter::repeat_n(0u8, pad));
        offset += data.len() + pad;
    }
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colr;
    use crate::sfnt::Face;
    use crate::var::Coords;

    #[test]
    fn the_colour_face_parses_and_paints_a_red_a_and_an_outline_b() {
        let face = Face::parse(colour_face()).unwrap();
        assert_eq!(face.units_per_em(), 1000);
        assert_eq!(face.glyph_index('A'), Some(1));
        assert_eq!(face.glyph_index('B'), Some(2));
        let a = colr::render(&face, 1, 0.1, &Coords::default(), 0xFF00_0000).unwrap();
        assert_eq!((a.left, a.top, a.width, a.height), (10, -10, 10, 10));
        assert!(a.pixels.iter().all(|&p| p == COLOUR_FACE_RED));
        assert!(colr::render(&face, 2, 0.1, &Coords::default(), 0xFF00_0000).is_none());
        assert!(!face.outline(2).unwrap().is_empty());
    }
}
