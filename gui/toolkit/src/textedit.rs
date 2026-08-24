//! The parts of a single-line editable text field that every one of them needs:
//! a caret, a selection, and a scroll offset that keeps the caret in the box.
//!
//! # Why this is a module and not a copy in each field
//!
//! The toolkit has five single-line text fields — `WidgetKind::TextInput`, the
//! `InputDialog`, the shell's path bar, the launcher's search box and the
//! desktop's Run dialog. Before this module existed they had five different
//! answers to "where is the caret drawn?", and two of those answers were "it is
//! not" (`known-issues.md`,
//! `TD-C-TWO-TOOLKIT-TEXT-FIELDS-DRAW-NO-CARET-AT-ALL`). A caret is not a
//! feature you can get subtly different in five places and still have a
//! toolkit: a user who selects text in one field and finds the highlight a
//! different colour, or the scrolling a different shape, in the field next to it
//! is looking at two applications, not one.
//!
//! So the drawing lives here once, and each field supplies what only it knows:
//! what string to draw (a password field draws a row of marks, not its
//! contents), where the caret is in that string, where the box is, and whether
//! it has the focus.
//!
//! # What stays with the field
//!
//! Everything about *editing*. Insert, delete and caret motion belong to the
//! widget, because the rules differ: `WidgetKind::TextInput` moves its caret by
//! the screen (`design-decisions.md` §541) while a password field moves it by
//! the string (§543), and only the field knows which it is. This module owns
//! the three primitives that are the same everywhere — planting a selection
//! anchor, cutting the selected range out, and deciding how far to scroll — and
//! the drawing that consumes them.

use crate::color::Color;
use crate::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow, TextSpan};
use crate::style::CornerRadii;
use crate::text::TextCursor;

/// What a text field's selection is painted in, and what selected text is
/// drawn in over it. The same accent the checkbox tick already uses.
pub const SELECTION_BACKGROUND: Color = Color::from_hex(0x0078D7);
/// The colour selected text is drawn in, over [`SELECTION_BACKGROUND`].
pub const SELECTION_FOREGROUND: Color = Color::WHITE;

/// How close to the right edge of the field the caret is allowed to sit before
/// the text starts scrolling under it. One pixel is the caret's own width; two
/// keeps it from being drawn half on the border.
const CARET_EDGE_MARGIN: f32 = 2.0;

/// Plant or drop a selection anchor as an arrow key is about to move the caret.
///
/// Call it *before* the move, with the caret's current position, because the
/// anchor is where the selection started — the place the caret is leaving, not
/// the place it is going to.
pub fn begin_or_end_selection(shift: bool, cursor: TextCursor, anchor: &mut Option<usize>) {
    if shift {
        anchor.get_or_insert(cursor.byte());
    } else {
        *anchor = None;
    }
}

/// The selected byte range, low end first, or `None` if nothing is selected.
///
/// An anchor equal to the caret is not a selection: it is where a Shift+Arrow
/// is about to start one from, and treating it as a zero-width selection would
/// make "is there a selection?" true for a field the user has only tabbed into.
#[must_use]
pub fn selected_range(cursor: TextCursor, anchor: Option<usize>) -> Option<(usize, usize)> {
    let start = anchor?;
    let here = cursor.byte();
    let range = (start.min(here), start.max(here));
    (range.0 < range.1).then_some(range)
}

/// Remove the selected range, if there is one, leaving the caret at its start.
///
/// Returns whether anything was deleted, which is how callers tell "the
/// selection was the edit" from "there was no selection, do the ordinary
/// thing". The anchor is cleared either way: after this there is nothing left
/// to be selected, and an anchor pointing into text that has been removed is an
/// offset that can be past the end of the string.
pub fn delete_selection(
    value: &mut String,
    cursor: &mut TextCursor,
    anchor: &mut Option<usize>,
) -> bool {
    let Some(start) = anchor.take() else {
        return false;
    };
    let here = cursor.byte();
    let (from, to) = (start.min(here), start.max(here));
    // Offsets that are not on a character boundary, or that run past the end,
    // cannot arise from the code above — but `String::drain` panics on them,
    // and a panic in a text field takes the whole application down over a
    // keystroke. Refusing the edit is the only outcome the user can recover
    // from.
    if from >= to || !value.is_char_boundary(from) || !value.is_char_boundary(to) {
        return false;
    }
    value.drain(from..to);
    *cursor = TextCursor::from(from);
    true
}

/// How far left to shift a field's text so the caret stays inside the box.
///
/// Deliberately computed fresh each frame from the caret's position, rather
/// than remembered across frames and adjusted by the minimum needed. A stored
/// offset is a second source of truth about where the text is, and it goes
/// stale in every direction: the value can be replaced from code, the field can
/// be resized, the font can change, and each of those leaves an offset that
/// scrolls the text to somewhere the caret is not. The cost is that moving the
/// caret leftwards through a long string scrolls the view further than strictly
/// necessary; the benefit is that "the caret is visible" is true by
/// construction rather than by everyone remembering to maintain it.
/// See `design-decisions.md` §546.
///
/// Nothing scrolls while the text fits, which is the overwhelmingly common
/// case, so this is `0.0` almost always.
#[must_use]
pub fn horizontal_scroll(text_width: f32, avail: f32, caret_px: f32) -> f32 {
    if text_width <= avail {
        return 0.0;
    }
    (caret_px - avail + CARET_EDGE_MARGIN).clamp(0.0, text_width - avail)
}

/// Draw the caret as a vertical rule of the line's height.
pub fn push_caret(tree: &mut RenderTree, x: f32, y: f32, line_h: f32, color: Color) {
    tree.push(RenderCommand::Line {
        x1: x,
        y1: y,
        x2: x,
        y2: y + line_h,
        color,
        width: 1.0,
    });
}

/// One single-line field's contents, ready to be drawn.
///
/// `text` is what appears on screen, which for a password field is the row of
/// marks and not the secret; `cursor` and `selection_anchor` are offsets into
/// *that* string. Keeping the mapping in the caller is what lets this function
/// stay ignorant of masking: it measures and colours exactly what it draws, so
/// the caret cannot land between two marks that stand for one character.
pub struct SingleLine<'a> {
    /// The string to draw.
    pub text: &'a str,
    /// Caret position, as an offset into `text`.
    pub cursor: TextCursor,
    /// Where a selection started, as an offset into `text`.
    pub selection_anchor: Option<usize>,
    /// Whether to draw a caret at all. A caret in an unfocused field is a
    /// caret that lies about where the next keystroke goes.
    pub focused: bool,
    /// Left edge of the area the text may occupy.
    pub x: f32,
    /// Top edge of the area the text may occupy.
    pub y: f32,
    /// Width of that area. Text wider than this scrolls under the caret.
    pub width: f32,
    /// Height of one line: the height of the selection boxes and the caret.
    pub line_height: f32,
    /// Font size the text is measured and drawn at.
    pub font_size: f32,
    /// Weight the text is measured and drawn at.
    pub weight: FontWeightHint,
    /// Colour of unselected text, and of the caret.
    pub color: Color,
}

/// Draw a field's selection, text and caret, clipped to the field.
///
/// The three have to be drawn together because they share one scroll offset:
/// computing it twice — once to place the text and once to place the caret —
/// is how a caret ends up a few pixels away from the character it is in front
/// of when the field is scrolled.
pub fn draw(tree: &mut RenderTree, f: &SingleLine<'_>) {
    let caret_px = crate::text::caret_x(f.text, f.cursor, f.font_size, f.weight);
    let text_w = crate::text::measure(f.text, f.font_size, f.weight);
    let scroll = horizontal_scroll(text_w, f.width, caret_px);

    // Everything below is drawn shifted left by `scroll`, so the head of a long
    // string lands outside the field. The clip is what keeps it from being
    // painted over whatever is to the left — without it, scrolling one field
    // would smear its text across the rest of the form.
    tree.clip(f.x, f.y, f.width, f.line_height);
    let origin = f.x - scroll;

    let range = selected_range(f.cursor, f.selection_anchor);
    if let Some((from, to)) = range {
        // `selection_boxes`, and not one rectangle from `x_of(from)` to
        // `x_of(to)`: where the range crosses a change of direction it is two
        // runs on screen with unselected text between them, and the single
        // rectangle would highlight characters nobody selected.
        for (left, width) in crate::text::selection_boxes(f.text, from, to, f.font_size, f.weight) {
            tree.push(RenderCommand::FillRect {
                x: origin + left,
                y: f.y,
                width,
                height: f.line_height,
                color: SELECTION_BACKGROUND,
                corner_radii: CornerRadii::ZERO,
            });
        }
    }

    // `RichText` rather than three `Text` commands, because cutting the string
    // at the selection's edges and drawing the pieces end to end assumes screen
    // order is byte order — true right up until the text mixes directions, at
    // which point the pieces belong interleaved and there is no `x` at which one
    // of them draws correctly. Colour has to be a property of a glyph. See
    // `RenderCommand::RichText`.
    let spans = range.map_or_else(Vec::new, |(from, to)| {
        let mut spans = Vec::with_capacity(2);
        if from > 0 {
            spans.push(TextSpan {
                end: u32::try_from(from).unwrap_or(u32::MAX),
                color: f.color,
            });
        }
        spans.push(TextSpan {
            end: u32::try_from(to).unwrap_or(u32::MAX),
            color: SELECTION_FOREGROUND,
        });
        spans
    });
    tree.push(RenderCommand::RichText {
        x: origin,
        y: f.y,
        text: f.text.to_string(),
        spans,
        color: f.color,
        font_size: f.font_size,
        font_weight: f.weight,
        // No `max_width`: the clip above is what bounds this, and an ellipsis
        // would mark a cut that the scroll offset means is not a cut — the rest
        // is reachable by moving the caret, which is exactly what a truncation
        // mark tells the reader it is not.
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    if f.focused {
        push_caret(tree, origin + caret_px, f.y, f.line_height, f.color);
    }
    tree.unclip();
}

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes. `float_cmp` in particular: the
    // scroll offset is arithmetic on exactly-representable values, and the
    // whole point of the assertions is that it lands on the right one.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    #[test]
    fn nothing_scrolls_while_the_text_fits() {
        assert_eq!(horizontal_scroll(40.0, 100.0, 40.0), 0.0);
        assert_eq!(horizontal_scroll(100.0, 100.0, 100.0), 0.0);
    }

    #[test]
    fn the_scroll_never_runs_past_either_end_of_the_text() {
        // Caret at the very start of a long string: no scroll, or the text
        // would be dragged in from the left with blank space behind it.
        assert_eq!(horizontal_scroll(500.0, 100.0, 0.0), 0.0);
        // Caret at the very end: exactly enough to bring the end into view and
        // not one pixel more.
        assert_eq!(horizontal_scroll(500.0, 100.0, 500.0), 400.0);
    }

    #[test]
    fn an_anchor_sitting_on_the_caret_is_not_a_selection() {
        // Tabbing into a field and pressing Shift plants an anchor where the
        // caret already is. Reporting that as a selection would make the next
        // keystroke delete a zero-width range -- harmless -- but also paint a
        // selection box of no width and colour a span of no characters, and
        // would make `has_selection` true for a field nobody has selected
        // anything in.
        assert_eq!(selected_range(TextCursor::from(3), Some(3)), None);
        assert_eq!(selected_range(TextCursor::from(3), None), None);
        assert_eq!(selected_range(TextCursor::from(3), Some(7)), Some((3, 7)));
        // The anchor may be on either side of the caret: dragging or
        // shift-arrowing leftwards puts it after.
        assert_eq!(selected_range(TextCursor::from(7), Some(3)), Some((3, 7)));
    }

    #[test]
    fn cutting_a_selection_leaves_the_caret_where_the_text_was_taken_from() {
        let mut value = String::from("hello world");
        let mut cursor = TextCursor::from(5);
        let mut anchor = Some(0);
        assert!(delete_selection(&mut value, &mut cursor, &mut anchor));
        assert_eq!(value, " world");
        assert_eq!(cursor.byte(), 0);
        assert_eq!(
            anchor, None,
            "the anchor must not survive the text it named"
        );
    }

    #[test]
    fn cutting_nothing_reports_that_it_cut_nothing() {
        // The callers branch on this: `false` means "no selection, do the
        // ordinary edit", and a `true` here would swallow the backspace.
        let mut value = String::from("hello");
        let mut cursor = TextCursor::from(2);
        let mut anchor = None;
        assert!(!delete_selection(&mut value, &mut cursor, &mut anchor));
        assert_eq!(value, "hello");
        assert_eq!(cursor.byte(), 2);
    }

    #[test]
    fn a_cut_refuses_an_offset_that_is_not_a_character_boundary() {
        // Not reachable through the widgets, which only ever move the caret to
        // boundaries the text named -- but `String::drain` panics on such an
        // offset, and a panic here takes the application down over a keystroke.
        let mut value = String::from("aé b");
        let mut cursor = TextCursor::from(2); // inside the two-byte 'é'
        let mut anchor = Some(0);
        assert!(!delete_selection(&mut value, &mut cursor, &mut anchor));
        assert_eq!(value, "aé b", "a refused cut must leave the text alone");
    }

    #[test]
    fn a_selection_anchor_is_planted_once_and_dropped_by_a_bare_arrow() {
        let mut anchor = None;
        begin_or_end_selection(true, TextCursor::from(4), &mut anchor);
        assert_eq!(anchor, Some(4));
        // A second Shift+Arrow must not re-plant it: the selection grows from
        // where it started, not from wherever the caret happens to be now.
        begin_or_end_selection(true, TextCursor::from(6), &mut anchor);
        assert_eq!(anchor, Some(4));
        begin_or_end_selection(false, TextCursor::from(6), &mut anchor);
        assert_eq!(anchor, None);
    }
}
