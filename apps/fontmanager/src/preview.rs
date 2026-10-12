//! A line of text in a family, drawn into a picture: the preview.
//!
//! Drawn here -- shaped and rasterized by the font crate every program draws
//! with (`osfont::scaled`) -- and handed to the compositor as pixels, as a
//! picture viewer hands it a photograph, rather than as text in the family
//! (`RenderCommand::PushFont` with `FontFamily::Named`). The compositor finds
//! a named family in the font index it built when it started, so text named
//! in a family installed since -- what this program installs -- would come
//! out in the interface's face; and a name longer than
//! `FamilyName::MAX_LEN` cannot be named at all. The face drawn here is read
//! from this program's own index, read again after every change.
//!
//! Drawn on the panel's own colour rather than on nothing: the rasterizer
//! writes every pixel it touches opaque, so a transparent picture would come
//! back with its glyphs' smoothed edges dark against the panel.

use std::sync::Arc;

use osfont::scaled::{ScaledFont, Target};
use osfont::sfnt::Face;

/// Room around the text, in pixels, so a descender or an accent is not cut
/// at the picture's edge.
const PAD: f32 = 4.0;

/// The tallest a preview picture is, in pixels: a face with absurd metrics
/// gets a picture this tall rather than one that exhausts memory.
const MAX_HEIGHT: f32 = 512.0;

/// A picture of text: `width` by `height` pixels, `0xAARRGGBB`, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixels, `width * height` of them.
    pub pixels: Vec<u32>,
}

/// `text` in `face` at `size` pixels to the em, drawn in `ink` on
/// `background`, as a picture `width` pixels wide and one line of the face
/// tall -- what is wider is cut at the edge. `None` if the face cannot be
/// drawn at that size, or the picture would be empty. The face is shared:
/// one parse serves every size it is drawn at.
#[must_use]
pub fn line(
    face: &Arc<Face>,
    text: &str,
    size: f32,
    width: u32,
    background: u32,
    ink: u32,
) -> Option<Picture> {
    // `shared` refuses only a size that is not finite and positive: a size no
    // face can be drawn at, which is this function's `None`.
    let mut font = ScaledFont::shared(Arc::clone(face), size).ok()?;
    let metrics = font.metrics();
    let (ascent, descent) = (metrics.ascent, metrics.descent);
    let tall = (ascent + descent + 2.0 * PAD).ceil();
    if !tall.is_finite() || tall < 1.0 || width == 0 {
        return None;
    }
    // Finite and at least one, so the cast keeps it; clamped first, so a
    // face whose metrics say a mile is drawn a picture high.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let height = tall.min(MAX_HEIGHT) as u32;
    let count = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?;
    let mut pixels = vec![background; count];
    let mut target = Target {
        buffer: &mut pixels,
        stride: width,
        height,
        color: ink,
    };
    font.draw_text(text, &mut target, PAD, PAD + ascent);
    Some(Picture {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;

    const PAPER: u32 = 0xFF10_1010;
    const INK: u32 = 0xFFF0_F0F0;

    /// **A family's line is drawn in its own glyphs**: the test font's `A`
    /// is a square, so a line of `A`s is ink where the squares are and the
    /// panel's colour around them -- and a line of a character the font has
    /// no glyph for draws no square.
    #[test]
    fn a_line_is_drawn_in_the_familys_own_glyphs() {
        let face = Arc::new(
            Face::parse(crate::library::tests::test_font(
                "Alpha", false, false, false,
            ))
            .unwrap(),
        );
        let picture = line(&face, "AAA", 100.0, 200, PAPER, INK).expect("drawn");
        assert_eq!(picture.width, 200);
        assert_eq!(
            picture.pixels.len(),
            usize::try_from(picture.width * picture.height).unwrap()
        );
        let inked = picture.pixels.iter().filter(|p| **p == INK).count();
        assert!(inked > 100, "no square was drawn: {inked} pixels of ink");
        assert_eq!(
            picture.pixels[0], PAPER,
            "the corner is not the panel's colour"
        );

        let face = Arc::new(
            Face::parse(crate::library::tests::test_font(
                "Alpha", false, false, false,
            ))
            .unwrap(),
        );
        let blank = line(&face, "zzz", 100.0, 200, PAPER, INK).expect("drawn");
        assert!(
            blank.pixels.iter().all(|p| *p != INK),
            "a character the face has no glyph for drew one"
        );
    }

    /// **The picture is one line of the face tall**, its ascent and descent
    /// and the room around them, and nothing for no width.
    #[test]
    fn the_picture_is_one_line_tall() {
        let face = Arc::new(
            Face::parse(crate::library::tests::test_font(
                "Alpha", false, false, false,
            ))
            .unwrap(),
        );
        // The test font: 800 up and 200 down in 1000 units, at 50 pixels to
        // the em -- 40 and 10 -- and four pixels of room either side.
        let picture = line(&face, "A", 50.0, 100, PAPER, INK).expect("drawn");
        assert_eq!(picture.height, 58);
        let face = Arc::new(
            Face::parse(crate::library::tests::test_font(
                "Alpha", false, false, false,
            ))
            .unwrap(),
        );
        assert!(line(&face, "A", 50.0, 0, PAPER, INK).is_none());
    }
}
