//! Text measurement, shared by every widget that needs to know how wide a
//! label is.
//!
//! # Why this exists
//!
//! Widgets used to estimate text width by multiplying a byte count by a fudge
//! factor — `text.len() as f32 * font_size * 0.6` — with each widget picking
//! its own constant: 0.6 in [`menu`], 0.58 in [`disabled`], a flat 7.0 in
//! [`modal`], 7.5 in [`tabs`], 8.0 in [`pathbar`]. That was defensible while
//! the compositor drew a fixed 8x14 cell and threw `font_size` away, because
//! nothing could be measured accurately anyway. It is not defensible now that
//! the compositor draws with a real font: an estimate that disagrees with what
//! is drawn means labels overflow their buttons and text cursors land between
//! characters.
//!
//! It was also wrong in a way the fudge factor cannot fix. `str::len` counts
//! **bytes**, so any non-ASCII text measured 2–4x too wide per character — and
//! non-ASCII text now renders, so that error became visible rather than moot.
//!
//! # How it stays right
//!
//! Everything here measures with [`osfont`]'s [`FontCache`], which is the same
//! type and the same rounding rule the compositor draws with. Measuring and
//! drawing cannot drift apart because there is nothing to keep in sync.
//!
//! [`menu`]: crate::menu
//! [`disabled`]: crate::disabled
//! [`modal`]: crate::modal
//! [`tabs`]: crate::tabs
//! [`pathbar`]: crate::pathbar

use std::sync::{Mutex, OnceLock, PoisonError};

use osfont::system::{FontCache, Weight};

use crate::render::FontWeightHint;

/// The process-wide font cache.
///
/// Global because it is a pure memoization of "what does this size look
/// like": two widgets asking about 14 px regular text must get the same
/// answer, and threading a cache through every `intrinsic_size` call would
/// change the signature of most of the toolkit to say so.
fn cache() -> &'static Mutex<FontCache> {
    static CACHE: OnceLock<Mutex<FontCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(FontCache::new()))
}

/// Runs `f` with the font for `size` and `weight`.
///
/// Poisoning is ignored deliberately. The guarded value is a cache of
/// rasterized glyphs with no cross-entry invariants, so a panic elsewhere in
/// the UI cannot leave it inconsistent — only possibly missing an entry, which
/// the next call rebuilds. Propagating the poison instead would turn one
/// widget's panic into a permanently unmeasurable UI.
fn with_font<R>(
    size: f32,
    weight: FontWeightHint,
    f: impl FnOnce(&mut osfont::system::SystemFont) -> R,
) -> R {
    let mut cache = cache().lock().unwrap_or_else(PoisonError::into_inner);
    f(cache.get(size, weight_of(weight)))
}

/// Translates the toolkit's weight hint into the one `osfont` understands.
///
/// `Light` maps to regular because the built-in face has two weights and no
/// third; rendering it bold would be the opposite of what was asked for.
fn weight_of(hint: FontWeightHint) -> Weight {
    match hint {
        FontWeightHint::Bold => Weight::Bold,
        FontWeightHint::Regular | FontWeightHint::Light => Weight::Regular,
    }
}

/// Width of `text` in pixels, as the compositor will actually draw it.
pub fn measure(text: &str, size: f32, weight: FontWeightHint) -> f32 {
    with_font(size, weight, |font| font.measure(text))
}

/// Width of `text` in pixels at the default weight.
pub fn width(text: &str, size: f32) -> f32 {
    measure(text, size, FontWeightHint::Regular)
}

/// Baseline-to-baseline distance in pixels.
pub fn line_height(size: f32, weight: FontWeightHint) -> f32 {
    with_font(size, weight, |font| font.line_height())
}

/// Distance from the top of a line down to its baseline, in pixels.
///
/// Needed by callers that position text by its top edge, which is most of
/// them, since layout works in boxes.
pub fn ascent(size: f32, weight: FontWeightHint) -> f32 {
    with_font(size, weight, |font| font.metrics().ascent)
}

/// Width of a single `'0'`, for callers laying out columns of digits or
/// treating text as a grid.
///
/// A grid is the wrong model for proportional text, so this is a stopgap for
/// widgets that have not yet been converted to measure real substrings — the
/// terminal-style views where a character grid is genuinely the right model
/// should keep using it.
pub fn digit_advance(size: f32, weight: FontWeightHint) -> f32 {
    measure("0", size, weight)
}

/// The longest prefix of `text` that fits in `max_width`, as a byte index.
///
/// Breaks between characters, never inside one: half a glyph reads as a
/// rendering fault rather than as elided text, and a byte-sliced UTF-8
/// sequence is not text at all.
pub fn fit(text: &str, max_width: f32, size: f32, weight: FontWeightHint) -> usize {
    if max_width <= 0.0 {
        return 0;
    }
    with_font(size, weight, |font| {
        let mut used = 0.0;
        for (idx, ch) in text.char_indices() {
            // Measured per character rather than by re-measuring the whole
            // prefix each step, which would be quadratic in the line length.
            let Some((_, advance)) = font.glyph(ch) else {
                continue;
            };
            if used + advance > max_width {
                return idx;
            }
            used += advance;
        }
        text.len()
    })
}

/// `text` truncated to `max_width`, with `ellipsis` appended if it did not fit.
///
/// The ellipsis is measured too, so the result genuinely fits rather than
/// overflowing by exactly the width of the ellipsis — the bug that makes
/// truncated labels still collide with whatever is next to them.
pub fn elide(
    text: &str,
    max_width: f32,
    ellipsis: &str,
    size: f32,
    weight: FontWeightHint,
) -> String {
    if measure(text, size, weight) <= max_width {
        return text.to_string();
    }
    let room = max_width - measure(ellipsis, size, weight);
    if room <= 0.0 {
        // Not even the ellipsis fits, so anything drawn would overflow.
        return String::new();
    }
    let cut = fit(text, room, size, weight);
    let mut out = text[..cut].to_string();
    out.push_str(ellipsis);
    out
}

/// The character index in `text` nearest to `offset` pixels from its start.
///
/// This is what a click on a line of text means: the caret goes to the closest
/// gap between characters, not to the one the click landed inside, so clicking
/// the right half of a letter puts the caret after it.
pub fn char_index_at(text: &str, offset: f32, size: f32, weight: FontWeightHint) -> usize {
    if offset <= 0.0 {
        return 0;
    }
    with_font(size, weight, |font| {
        let mut x = 0.0;
        for (n, ch) in text.chars().enumerate() {
            let Some((_, advance)) = font.glyph(ch) else {
                continue;
            };
            if offset < x + advance / 2.0 {
                return n;
            }
            x += advance;
        }
        text.chars().count()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measuring_counts_characters_not_bytes() {
        // The bug the old `text.len() as f32 * k` estimate had: a two-byte
        // character measured twice as wide as a one-byte one.
        let ascii = measure("eee", 16.0, FontWeightHint::Regular);
        let accented = measure("ééé", 16.0, FontWeightHint::Regular);
        assert_eq!(
            ascii, accented,
            "'ééé' is 6 bytes and 3 characters; it must measure as 3"
        );
    }

    #[test]
    fn measuring_scales_with_size() {
        let small = measure("Hello", 16.0, FontWeightHint::Regular);
        let big = measure("Hello", 48.0, FontWeightHint::Regular);
        assert!(small > 0.0);
        assert!(
            big > small,
            "48px text measured {big}, 16px measured {small}"
        );
    }

    #[test]
    fn the_empty_string_has_no_width() {
        assert_eq!(measure("", 16.0, FontWeightHint::Regular), 0.0);
    }

    #[test]
    fn fit_breaks_between_characters() {
        let text = "ééé";
        let one = measure("é", 16.0, FontWeightHint::Regular);
        // Room for two characters and a sliver: the third must be dropped
        // whole, and the cut must land on a character boundary.
        let cut = fit(text, one * 2.5, 16.0, FontWeightHint::Regular);
        assert!(text.is_char_boundary(cut), "cut {cut} splits a character");
        assert_eq!(&text[..cut], "éé");
    }

    #[test]
    fn fit_handles_the_degenerate_widths() {
        assert_eq!(fit("abc", 0.0, 16.0, FontWeightHint::Regular), 0);
        assert_eq!(fit("abc", -5.0, 16.0, FontWeightHint::Regular), 0);
        assert_eq!(fit("abc", 1e9, 16.0, FontWeightHint::Regular), 3);
        assert_eq!(fit("", 100.0, 16.0, FontWeightHint::Regular), 0);
    }

    #[test]
    fn elided_text_actually_fits() {
        // The point of measuring the ellipsis: a truncated label that still
        // overflows collides with whatever is beside it.
        let text = "a very long label indeed";
        for max in [10.0, 40.0, 80.0, 160.0] {
            let out = elide(text, max, "...", 16.0, FontWeightHint::Regular);
            assert!(
                measure(&out, 16.0, FontWeightHint::Regular) <= max,
                "{out:?} is wider than {max}"
            );
        }
    }

    #[test]
    fn text_that_fits_is_not_elided() {
        let out = elide("short", 1000.0, "...", 16.0, FontWeightHint::Regular);
        assert_eq!(out, "short");
    }

    #[test]
    fn clicking_a_character_snaps_to_the_nearer_gap() {
        let w = measure("m", 16.0, FontWeightHint::Regular);
        let f = |x| char_index_at("mmm", x, 16.0, FontWeightHint::Regular);
        assert_eq!(f(-10.0), 0, "left of the text is the start");
        assert_eq!(f(0.0), 0);
        assert_eq!(f(w * 0.4), 0, "left half of the first character");
        assert_eq!(f(w * 0.6), 1, "right half of the first character");
        assert_eq!(f(w * 1.6), 2);
        assert_eq!(f(w * 100.0), 3, "past the end is the end");
    }

    #[test]
    fn line_height_exceeds_ascent() {
        // Not a tautology: a face whose descent was folded into the ascent
        // would place every baseline one descent too low.
        for size in [11.0, 16.0, 48.0] {
            let lh = line_height(size, FontWeightHint::Regular);
            let asc = ascent(size, FontWeightHint::Regular);
            assert!(asc > 0.0, "{size}px: ascent {asc}");
            assert!(lh > asc, "{size}px: line height {lh} <= ascent {asc}");
        }
    }

    #[test]
    fn bold_is_wider_than_regular() {
        let regular = measure("lll", 16.0, FontWeightHint::Regular);
        let bold = measure("lll", 16.0, FontWeightHint::Bold);
        assert!(bold >= regular, "bold {bold} < regular {regular}");
    }

    #[test]
    fn light_measures_as_regular() {
        // It renders as regular, so it must measure as regular; the two
        // disagreeing is exactly the class of bug this module removes.
        assert_eq!(
            measure("Light", 16.0, FontWeightHint::Light),
            measure("Light", 16.0, FontWeightHint::Regular)
        );
    }
}
