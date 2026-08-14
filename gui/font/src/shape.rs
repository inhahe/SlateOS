//! Turning a string into the glyphs that get drawn, in the order and at the
//! spacing the font asks for.
//!
//! # Why a run, and not a loop over `chars()`
//!
//! Every text path in the OS used to be some variant of
//!
//! ```text
//! for ch in text.chars() { let g = font.glyph(ch); pen += g.advance; }
//! ```
//!
//! which quietly assumes three things that are all false of real fonts:
//!
//! 1. **One character makes one glyph.** `fi` is one glyph in most serif
//!    faces — the `f`'s hood and the `i`'s dot collide, so the designer drew
//!    a joined form and told the font to use it.
//! 2. **A glyph's width does not depend on its neighbours.** It does; that is
//!    what kerning is.
//! 3. **Every loop that walks the text will make the same assumptions.** They
//!    did not. Adding kerning to `measure` alone left the toolkit's caret
//!    placement and its ellipsis truncation summing unkerned advances, so a
//!    click landed one place and the caret another — on kerned text only, and
//!    by a few pixels, which is the kind of bug that gets reported as "the
//!    text feels wrong".
//!
//! A [`ShapedRun`] answers all three at once. It is produced once per string,
//! and *every* consumer — measuring, drawing, hit-testing, truncating — walks
//! the same list. They cannot disagree, because there is nothing left to
//! disagree about: the advances are already final.
//!
//! # Clusters
//!
//! [`ShapedGlyph::cluster`] is the byte offset in the source string of the
//! first character the glyph came from. It is what makes ligatures safe for
//! the rest of the system: a caret cannot land inside `fi`, because no
//! shaped glyph starts there, and a truncation cannot cut between the `f` and
//! the `i`, because they are one entry. Callers that need character indices
//! rather than byte offsets convert at the edge; byte offsets are what
//! actually slice a string.
//!
//! # What this is not
//!
//! It is not a full shaper. There is no script itemisation, no bidi, no
//! reordering, no mark attachment. It handles the substitutions and the
//! spacing that Latin text needs and that the previous per-character loop got
//! wrong; scripts that need reordering to be legible are not yet supported
//! and would need a real shaper (see `roadmap.md`).

use alloc::vec::Vec;

/// How wide a tab is, in spaces.
///
/// A real tab is a *stop* — an absolute column the pen jumps to — which a
/// shaper cannot compute, because it does not know where in the line the run
/// starts. A fixed multiple of the space advance is the approximation every
/// simple text engine makes, and it is what the built-in face's measurement
/// already assumed; naming it here stops the two backends from each picking
/// their own number. A terminal, which does know its columns, must expand
/// tabs itself before shaping.
pub const TAB_WIDTH_IN_SPACES: f32 = 4.0;

/// Which glyph to draw, in the font that produced it.
///
/// Opaque, and deliberately: an outline face numbers its glyphs and the
/// built-in bitmap face is keyed by character, and a caller that could tell
/// the two apart would start branching on it — which is exactly what
/// [`SystemFont`](crate::system::SystemFont) exists to stop. A key is only
/// meaningful to the font that shaped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphKey(u32);

impl GlyphKey {
    /// A glyph id in an outline face.
    pub(crate) fn outline(gid: u16) -> Self {
        Self(u32::from(gid))
    }

    /// A character in the built-in bitmap face, which has no glyph ids.
    pub(crate) fn bitmap(ch: char) -> Self {
        Self(ch as u32)
    }

    /// The outline glyph id this key holds.
    ///
    /// Zero (`.notdef`) if the value does not fit, which can only happen if a
    /// key from a bitmap font is handed to an outline font — a font mix-up,
    /// for which drawing the missing-glyph box is the right answer.
    pub(crate) fn gid(self) -> u16 {
        u16::try_from(self.0).unwrap_or(0)
    }

    /// The character this key holds, or the replacement character if the
    /// value is not a scalar value — again only reachable by mixing fonts.
    pub(crate) fn ch(self) -> char {
        char::from_u32(self.0).unwrap_or(char::REPLACEMENT_CHARACTER)
    }
}

/// One glyph of a shaped run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Which glyph to draw.
    pub key: GlyphKey,
    /// Byte offset in the shaped string of the first character this glyph
    /// came from. Several glyphs never share a cluster here (nothing yet
    /// decomposes one character into several), but several characters share
    /// one when they ligate.
    pub cluster: usize,
    /// How far to move the pen after drawing this glyph, in pixels.
    ///
    /// The glyph's own advance **plus** any kerning against the glyph that
    /// follows it. Folding the kern into the preceding advance rather than
    /// exposing it separately is what makes the sum of a run equal to its
    /// width, and what stops a caller from drawing a run correctly while
    /// measuring it wrongly.
    pub advance: f32,
    /// How much of `advance` is the correction against the *following* glyph.
    /// Zero on the last glyph of a run, and on every glyph of an unkerned
    /// face.
    ///
    /// It has to be recoverable, because a run that gets cut short loses the
    /// glyph that correction was for. Slicing `"AVATAR"` after the `A` and
    /// drawing the `A` alone must not leave in the pull-in that only made
    /// sense with the `V` behind it — that is a real off-by-a-pixel: the
    /// truncated string measures *wider* than the budget it was cut to fit.
    pub kern_next: f32,
}

/// The glyphs a string turns into, ready to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedRun {
    glyphs: Vec<ShapedGlyph>,
}

impl ShapedRun {
    /// Build a run from its glyphs. Crate-internal: only a font knows how to
    /// produce a correct one.
    pub(crate) fn new(glyphs: Vec<ShapedGlyph>) -> Self {
        Self { glyphs }
    }

    /// The glyphs, in drawing order.
    #[must_use]
    pub fn glyphs(&self) -> &[ShapedGlyph] {
        &self.glyphs
    }

    /// Total width in pixels.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.glyphs.iter().map(|g| g.advance).sum()
    }

    /// Whether the run drew nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// How many glyphs the run has, which is *not* the character count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    /// The byte offset at which to cut the source string so that what remains
    /// is at most `max_width` wide.
    ///
    /// Cuts between glyphs, so a ligature is kept or dropped whole: half of
    /// an `fi` is not a character, and a string sliced there is not valid
    /// UTF-8 either.
    ///
    /// `end` is the source string's length, returned when everything fits —
    /// the run cannot know it, because the last glyph's cluster is where the
    /// last character *starts*.
    #[must_use]
    pub fn fit(&self, max_width: f32, end: usize) -> usize {
        let mut used = 0.0;
        for glyph in &self.glyphs {
            // The caller slices the string here and draws the piece on its
            // own, so the width to test is the prefix's width *alone*: this
            // glyph's kern against the one about to be dropped goes with it.
            if used + glyph.advance - glyph.kern_next > max_width {
                return glyph.cluster;
            }
            used += glyph.advance;
        }
        end
    }

    /// The byte offset at which the longest *suffix* fitting `max_width`
    /// begins.
    ///
    /// The mirror of [`fit`](Self::fit), for the cases where the end of the
    /// string is the part worth keeping — a filesystem path, where the
    /// filename matters and the leading directories do not.
    ///
    /// No `kern_next` correction is needed in this direction: kerning is
    /// charged to the *preceding* glyph, so a suffix's leading glyph never
    /// carried one, and its trailing glyph is the run's own last, whose
    /// `kern_next` is already zero.
    #[must_use]
    pub fn fit_end(&self, max_width: f32, end: usize) -> usize {
        let mut used = 0.0;
        let mut start = end;
        for glyph in self.glyphs.iter().rev() {
            if used + glyph.advance > max_width {
                return start;
            }
            used += glyph.advance;
            start = glyph.cluster;
        }
        0
    }

    /// The byte offset of the gap between glyphs nearest to `offset` pixels
    /// from the start of the run.
    ///
    /// This is what a click on a line of text means: the caret goes to the
    /// closest gap, not to the glyph the click landed inside, so clicking the
    /// right half of a letter puts the caret after it.
    ///
    /// `end` is the source string's length, for a click past the last glyph.
    #[must_use]
    pub fn offset_at(&self, offset: f32, end: usize) -> usize {
        if offset <= 0.0 {
            return self.glyphs.first().map_or(end, |g| g.cluster);
        }
        let mut x = 0.0;
        for glyph in &self.glyphs {
            if offset < x + glyph.advance / 2.0 {
                return glyph.cluster;
            }
            x += glyph.advance;
        }
        end
    }

    /// How far into the run the character at byte offset `at` begins, in
    /// pixels. `end` is the source string's length.
    ///
    /// An offset inside a ligature reports the start of the ligature, since
    /// that is the only place a caret can honestly be drawn: the glyph has no
    /// interior boundary to point at.
    #[must_use]
    pub fn x_of(&self, at: usize, end: usize) -> f32 {
        let mut x = 0.0;
        for (i, glyph) in self.glyphs.iter().enumerate() {
            // A glyph spans from its own cluster up to the next one's, so it
            // is fully behind `at` only once the *following* glyph starts at
            // or before it. Testing this glyph's own cluster instead is what
            // would walk past a ligature and report the caret after it.
            let next = self
                .glyphs
                .get(i.saturating_add(1))
                .map_or(end, |n| n.cluster);
            if next > at {
                return x;
            }
            x += glyph.advance;
        }
        x
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]
mod tests {
    use super::*;

    /// A run of `n` glyphs, each 10 px wide, one byte per character.
    fn run(n: usize) -> ShapedRun {
        ShapedRun::new(
            (0..n)
                .map(|i| ShapedGlyph {
                    key: GlyphKey::bitmap('x'),
                    cluster: i,
                    advance: 10.0,
                    kern_next: 0.0,
                })
                .collect(),
        )
    }

    #[test]
    fn width_is_the_sum_of_the_advances() {
        assert!((run(4).width() - 40.0).abs() < f32::EPSILON);
        assert!(run(0).width().abs() < f32::EPSILON);
        assert!(run(0).is_empty());
    }

    #[test]
    fn fit_cuts_before_the_glyph_that_does_not_fit() {
        let r = run(5);
        assert_eq!(r.fit(25.0, 5), 2);
        // Exactly enough room is enough room.
        assert_eq!(r.fit(30.0, 5), 3);
        assert_eq!(r.fit(1000.0, 5), 5);
        assert_eq!(r.fit(0.0, 5), 0);
    }

    /// A prefix is drawn on its own, so the kern that pulled its last glyph
    /// towards a glyph that is about to be dropped must not be counted.
    ///
    /// Concretely, the bug this pins: `"AVATAR"` at a 10 px budget. The `A`'s
    /// advance is 10.32, but in the run it carries a -0.4 kern against the
    /// `V`, so a naive fit sees 9.9, keeps the `A`, and hands back a string
    /// that measures 10.32 — wider than the budget it was cut to fit.
    #[test]
    fn fit_drops_the_kern_to_the_glyph_it_drops() {
        let r = ShapedRun::new(alloc::vec![
            // "A" — 10.32 wide alone, 9.92 in the run.
            ShapedGlyph {
                key: GlyphKey::bitmap('A'),
                cluster: 0,
                advance: 9.92,
                kern_next: -0.4,
            },
            ShapedGlyph {
                key: GlyphKey::bitmap('V'),
                cluster: 1,
                advance: 10.0,
                kern_next: 0.0,
            },
        ]);
        // The whole run is still 19.92 wide as drawn.
        assert!((r.width() - 19.92).abs() < 0.001);
        // But "A" alone does not fit in 10.
        assert_eq!(r.fit(10.0, 2), 0);
        assert_eq!(r.fit(10.4, 2), 1);
        assert_eq!(r.fit(19.92, 2), 2);
    }

    #[test]
    fn fit_end_keeps_the_tail() {
        let r = run(5);
        assert_eq!(r.fit_end(25.0, 5), 3);
        assert_eq!(r.fit_end(30.0, 5), 2);
        assert_eq!(r.fit_end(1000.0, 5), 0);
        assert_eq!(r.fit_end(0.0, 5), 5);
    }

    #[test]
    fn a_click_goes_to_the_nearest_gap() {
        let r = run(3);
        // Left half of the first glyph.
        assert_eq!(r.offset_at(4.0, 3), 0);
        // Right half of the first glyph: after it.
        assert_eq!(r.offset_at(6.0, 3), 1);
        assert_eq!(r.offset_at(16.0, 3), 2);
        // Past the end.
        assert_eq!(r.offset_at(1000.0, 3), 3);
        assert_eq!(r.offset_at(-5.0, 3), 0);
    }

    #[test]
    fn x_of_walks_to_the_cluster() {
        let r = run(4);
        assert!(r.x_of(0, 4).abs() < f32::EPSILON);
        assert!((r.x_of(2, 4) - 20.0).abs() < f32::EPSILON);
        assert!((r.x_of(4, 4) - 40.0).abs() < f32::EPSILON);
        assert!((r.x_of(99, 4) - 40.0).abs() < f32::EPSILON);
    }

    /// The three queries above have to agree with each other on a run whose
    /// glyphs span several characters, which is the whole point of clusters:
    /// no offset any of them returns may fall inside a ligature.
    #[test]
    fn a_ligature_is_never_split() {
        // "office": o, ffi (one glyph, three chars), c, e.
        let r = ShapedRun::new(alloc::vec![
            ShapedGlyph { key: GlyphKey::bitmap('o'), cluster: 0, advance: 10.0, kern_next: 0.0 },
            ShapedGlyph { key: GlyphKey::bitmap('\u{FB03}'), cluster: 1, advance: 20.0, kern_next: 0.0 },
            ShapedGlyph { key: GlyphKey::bitmap('c'), cluster: 4, advance: 10.0, kern_next: 0.0 },
            ShapedGlyph { key: GlyphKey::bitmap('e'), cluster: 5, advance: 10.0, kern_next: 0.0 },
        ]);
        assert!((r.width() - 50.0).abs() < f32::EPSILON);

        let inside = [2, 3];
        for width in 0..60 {
            let cut = r.fit(f32::from(u16::try_from(width).unwrap()), 6);
            assert!(!inside.contains(&cut), "fit({width}) cut inside the ligature");
        }
        for offset in 0..60 {
            let at = r.offset_at(f32::from(u16::try_from(offset).unwrap()), 6);
            assert!(
                !inside.contains(&at),
                "offset_at({offset}) landed inside the ligature"
            );
        }
        // A caret asked for inside the ligature is drawn at its start.
        assert!((r.x_of(1, 6) - 10.0).abs() < f32::EPSILON);
        assert!((r.x_of(2, 6) - 10.0).abs() < f32::EPSILON);
        assert!((r.x_of(3, 6) - 10.0).abs() < f32::EPSILON);
        assert!((r.x_of(4, 6) - 30.0).abs() < f32::EPSILON);
        assert!((r.x_of(6, 6) - 50.0).abs() < f32::EPSILON);
    }
}
