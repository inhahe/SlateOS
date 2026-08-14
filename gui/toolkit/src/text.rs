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

/// The x at which to draw `text` so that it is centred on `center`.
///
/// Centring is by far the most common thing callers wanted a width for — and
/// the thing the old estimates got most visibly wrong, since the error in a
/// guessed width is halved into the offset and so grows with the label. Having
/// it here means an app centres text by saying so, rather than by re-deriving
/// "measure, halve, subtract" and picking its own fudge factor on the way.
pub fn center_x(text: &str, center: f32, size: f32, weight: FontWeightHint) -> f32 {
    center - measure(text, size, weight) / 2.0
}

/// The x at which to draw `text` so that it ends at `right`.
pub fn right_x(text: &str, right: f32, size: f32, weight: FontWeightHint) -> f32 {
    right - measure(text, size, weight)
}

/// Width of a box that holds `text` with `padding` px of space on each side.
///
/// Buttons, tabs, chips, badges and pills are all this shape, and before this
/// existed every one of them wrote `label.len() as f32 * 8.0 + 16.0` — a byte
/// count, so any label with a non-ASCII character in it got a box two to three
/// times too wide. Naming the shape means the padding stays a padding and the
/// width stays a width.
pub fn padded_width(text: &str, padding: f32, size: f32, weight: FontWeightHint) -> f32 {
    measure(text, size, weight) + padding * 2.0
}

/// Width of a box that holds `text` at *whichever* weight it ends up drawn at.
///
/// For a strip whose selected item is drawn bold and the rest regular. Sizing
/// each item to the weight it currently has makes the whole strip shuffle
/// sideways every time the selection moves, because the selected item grows and
/// pushes its neighbours along; sizing them all to the widest weight they can
/// take keeps the layout still and still fits the text.
pub fn padded_width_any_weight(text: &str, padding: f32, size: f32) -> f32 {
    let bold = measure(text, size, FontWeightHint::Bold);
    let regular = measure(text, size, FontWeightHint::Regular);
    bold.max(regular) + padding * 2.0
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

/// The longest *suffix* of `text` that fits in `max_width`, as the byte index
/// the suffix starts at.
///
/// The mirror of [`fit`], for the cases where the end of the string is the part
/// worth keeping — a filesystem path, where the filename matters and the
/// leading directories do not, is the usual one. Like [`fit`] it breaks between
/// characters: an index into the middle of a UTF-8 sequence is not a string
/// boundary, and slicing there is an abort rather than a cosmetic fault.
pub fn fit_end(text: &str, max_width: f32, size: f32, weight: FontWeightHint) -> usize {
    if max_width <= 0.0 {
        return text.len();
    }
    with_font(size, weight, |font| {
        let mut used = 0.0;
        for (idx, ch) in text.char_indices().rev() {
            let Some((_, advance)) = font.glyph(ch) else {
                continue;
            };
            if used + advance > max_width {
                // `idx` is the character that did not fit, so the suffix that
                // does starts after it.
                return idx.saturating_add(ch.len_utf8());
            }
            used += advance;
        }
        0
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

/// `text` truncated from its *start* to `max_width`, with `ellipsis` prepended
/// if it did not fit.
///
/// The mirror of [`elide`], for strings whose tail carries the information. A
/// path elided the usual way reads `/home/user/projects/very/deep...`, which
/// tells the reader nothing they wanted; elided from the start it reads
/// `...deep/notes.txt`, which names the file.
pub fn elide_start(
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
    let start = fit_end(text, room, size, weight);
    let mut out = ellipsis.to_string();
    out.push_str(&text[start..]);
    out
}

/// `text` broken into lines no wider than `max_width`, breaking at spaces.
///
/// Callers need this because [`RenderCommand::Text`] does **not** wrap: the
/// compositor truncates at `max_width`, dropping whole glyphs off the end of
/// the one line it draws. So a caller with a paragraph to show has to wrap it
/// itself and emit one command per line — and, crucially, has to reserve height
/// for the same lines it emits. Deriving the height from anything else (a byte
/// count over a guessed characters-per-line, say) is how a list of paragraphs
/// ends up with items overlapping each other.
///
/// A word longer than `max_width` gets its own over-long line rather than being
/// cut mid-word; breaking inside a word is a per-script decision that belongs to
/// a real line breaker. Existing newlines always break.
///
/// [`RenderCommand::Text`]: crate::render::RenderCommand::Text
pub fn wrap(text: &str, max_width: f32, size: f32, weight: FontWeightHint) -> Vec<String> {
    if max_width <= 0.0 {
        // Nothing fits, and the greedy rule below would answer that with one
        // word per line — an unbounded list for a box that cannot show it.
        // Reporting the paragraphs unwrapped keeps the line count meaningful.
        return text.split('\n').map(str::to_string).collect();
    }
    with_font(size, weight, |font| font.wrap(text, max_width))
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
    fn start_elided_text_actually_fits() {
        let path = "/home/user/projects/some/rather/deep/tree/notes.txt";
        for max in [10.0, 40.0, 80.0, 160.0] {
            let out = elide_start(path, max, "...", 16.0, FontWeightHint::Regular);
            assert!(
                measure(&out, 16.0, FontWeightHint::Regular) <= max,
                "{out:?} is wider than {max}"
            );
        }
    }

    #[test]
    fn start_eliding_keeps_the_end() {
        // The whole point for a path: the filename survives, the leading
        // directories are what get dropped.
        let path = "/home/user/projects/some/rather/deep/tree/notes.txt";
        let out = elide_start(path, 160.0, "...", 16.0, FontWeightHint::Regular);
        assert!(out.starts_with("..."), "{out:?} should be marked as cut");
        assert!(out.ends_with("notes.txt"), "{out:?} lost the filename");
    }

    #[test]
    fn start_eliding_breaks_between_characters() {
        // A byte index into the middle of a UTF-8 sequence is not a string
        // boundary; slicing there aborts rather than looking wrong, so this is
        // a crash test, not a layout one.
        let path = "/home/user/projets/déjà-vu/résumé-final.txt";
        for max in [4.0, 9.0, 17.0, 33.0, 65.0, 129.0] {
            let out = elide_start(path, max, "…", 16.0, FontWeightHint::Regular);
            assert!(measure(&out, 16.0, FontWeightHint::Regular) <= max, "{out:?} > {max}");
        }
    }

    #[test]
    fn fit_end_is_the_mirror_of_fit() {
        let s = "abcdef";
        let w = measure("abc", 16.0, FontWeightHint::Regular);
        // Room for exactly three characters, taken from the right.
        assert_eq!(&s[fit_end(s, w, 16.0, FontWeightHint::Regular)..], "def");
        assert_eq!(fit_end(s, 1e9, 16.0, FontWeightHint::Regular), 0, "all of it fits");
        assert_eq!(
            fit_end(s, -5.0, 16.0, FontWeightHint::Regular),
            s.len(),
            "no room means an empty suffix"
        );
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
    fn centering_puts_equal_space_on_both_sides() {
        let text = "centred";
        let (size, weight) = (16.0, FontWeightHint::Regular);
        let x = center_x(text, 100.0, size, weight);
        let left = x;
        let right = x + measure(text, size, weight);
        assert!(
            (100.0 - left - (right - 100.0)).abs() < 0.01,
            "{left}..{right} is not centred on 100"
        );
    }

    #[test]
    fn centering_is_not_biased_by_byte_length() {
        // The bug the old estimates had: an accented label measured twice as
        // wide, so centring it pushed it half a label to the left.
        let (size, weight) = (16.0, FontWeightHint::Regular);
        assert_eq!(
            center_x("eee", 100.0, size, weight),
            center_x("ééé", 100.0, size, weight)
        );
    }

    #[test]
    fn right_alignment_ends_where_asked() {
        let text = "right";
        let (size, weight) = (16.0, FontWeightHint::Regular);
        let x = right_x(text, 250.0, size, weight);
        assert!((x + measure(text, size, weight) - 250.0).abs() < 0.01);
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
    fn a_padded_box_holds_its_text_and_its_padding() {
        let label = "Preferences";
        let w = padded_width(label, 12.0, 13.0, FontWeightHint::Regular);
        assert!((w - measure(label, 13.0, FontWeightHint::Regular) - 24.0).abs() < 0.01);
        // Zero padding is the bare text, not a special case.
        assert!(
            (padded_width(label, 0.0, 13.0, FontWeightHint::Regular)
                - measure(label, 13.0, FontWeightHint::Regular))
            .abs()
                < 0.01
        );
    }

    #[test]
    fn a_padded_box_is_not_sized_by_byte_length() {
        // Same glyph count, three times the bytes. Sized the old way the second
        // box was three times the first; measured they are comparable.
        let ascii = padded_width("aaaa", 10.0, 13.0, FontWeightHint::Regular);
        let wide = padded_width("ええええ", 10.0, 13.0, FontWeightHint::Regular);
        assert!(wide < ascii * 3.0, "{ascii} vs {wide}");
    }

    #[test]
    fn an_any_weight_box_fits_both_weights() {
        for label in ["Visual", "Magnifier", "Keyboard & Mouse"] {
            let w = padded_width_any_weight(label, 9.0, 12.0);
            for weight in [FontWeightHint::Bold, FontWeightHint::Regular] {
                assert!(
                    measure(label, 12.0, weight) + 18.0 <= w + 0.01,
                    "{label:?} overflows at {weight:?}"
                );
            }
        }
    }

    #[test]
    fn an_any_weight_box_does_not_change_with_the_weight() {
        // The point of it: the box a tab gets must not depend on whether that
        // tab happens to be the selected one, or the strip walks sideways.
        let a = padded_width_any_weight("Audio", 9.0, 12.0);
        let b = padded_width_any_weight("Audio", 9.0, 12.0);
        assert_eq!(a, b);
        assert!(a >= padded_width("Audio", 9.0, 12.0, FontWeightHint::Bold) - 0.01);
        assert!(a >= padded_width("Audio", 9.0, 12.0, FontWeightHint::Regular) - 0.01);
    }

    #[test]
    fn wrapped_lines_fit_the_width_they_were_given() {
        let text = "the quick brown fox jumps over the lazy dog and keeps on running";
        for max in [60.0, 120.0, 240.0] {
            for line in wrap(text, max, 11.0, FontWeightHint::Regular) {
                // A lone over-long word is allowed past the limit — it is not
                // broken mid-word — but a line that combined words is not.
                if line.split_whitespace().count() < 2 {
                    continue;
                }
                assert!(
                    measure(&line, 11.0, FontWeightHint::Regular) <= max,
                    "{line:?} is wider than the {max}px box it was wrapped into"
                );
            }
        }
    }

    #[test]
    fn wrapping_never_loses_a_word() {
        let text = "Permission is hereby granted, free of charge, to any person";
        let lines = wrap(text, 90.0, 11.0, FontWeightHint::Regular);
        assert_eq!(
            lines.join(" ").split_whitespace().collect::<Vec<_>>(),
            text.split_whitespace().collect::<Vec<_>>()
        );
    }

    #[test]
    fn wrapping_honours_existing_newlines() {
        // A blank line between paragraphs has to survive, or a licence's
        // structure collapses into one run-on block.
        let lines = wrap("first\n\nsecond", 1000.0, 11.0, FontWeightHint::Regular);
        assert_eq!(lines, vec!["first", "", "second"]);
    }

    #[test]
    fn wrapping_is_not_decided_by_byte_length() {
        // Same glyph count, twice the bytes. Wrapped on a byte count the
        // accented text would break into twice as many lines.
        let ascii = wrap("aaa aaa aaa aaa aaa aaa", 80.0, 11.0, FontWeightHint::Regular);
        let accented = wrap("ééé ééé ééé ééé ééé ééé", 80.0, 11.0, FontWeightHint::Regular);
        assert_eq!(ascii.len(), accented.len());
    }

    #[test]
    fn wrapping_into_no_width_does_not_explode() {
        // The degenerate case: a greedy wrap would answer with one word per
        // line, so a paragraph in a zero-width box would report a line count
        // proportional to its word count.
        let lines = wrap("a b c d e f g", 0.0, 11.0, FontWeightHint::Regular);
        assert_eq!(lines, vec!["a b c d e f g"]);
    }

    #[test]
    fn a_narrower_box_never_needs_fewer_lines() {
        let text = "the quick brown fox jumps over the lazy dog";
        let mut previous = usize::MAX;
        for max in [400.0, 200.0, 100.0, 50.0] {
            let n = wrap(text, max, 11.0, FontWeightHint::Regular).len();
            assert!(n >= 1);
            assert!(
                n >= previous || previous == usize::MAX,
                "{max}px needed {n} lines, but a wider box needed {previous}"
            );
            previous = n;
        }
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
