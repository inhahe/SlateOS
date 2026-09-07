//! Where a scrollbar's thumb sits, and where a drag of it lands.
//!
//! # Why this is a module and not a widget
//!
//! Nothing here draws. A scrollbar's *appearance* belongs to whatever is
//! drawing the list — the file dialog paints one colour, a menu another, an
//! application its own — and a widget that owned the pixels would have to grow
//! a palette argument, a corner radius, and an opinion about hover states
//! before any of its three callers could use it. What they actually share is
//! two pieces of arithmetic, and those are what is here.
//!
//! # Why it exists at all
//!
//! Six places in this tree drew a scrollbar with their own copy of the same
//! formula. Five now call this module: `gui/toolkit`'s file dialog, `menu` and
//! `menubar`, the desktop shell's start menu, and `apps/dictionary`. Six copies
//! of one formula is six chances to write a different one — the same reasoning
//! that collapsed the glob matchers (design-decisions 555).
//!
//! **The sixth is `apps/spreadsheet`, and it is deliberately left alone.** Its
//! scrollbar is generic over the axis — one function draws both the vertical
//! and the horizontal bar from a `length` — where everything here is vertical,
//! reading `track.h` and `track.y` by name. Converting it means either
//! generalising this module to an axis-agnostic span or splitting that function
//! in two, and both are design decisions rather than mechanical substitutions.
//!
//! (A note on the count, because this module's own history has it wrong. The
//! doc first said six and named `gui/desktop/src/window_peek.rs` as one of
//! them; that file has no scrollbar. The grep behind the number had matched
//! `max_thumb_height` and `MIN_THUMBNAIL_WIDTH`, which size a window *preview
//! image*. It then said five, having dropped `window_peek` without looking for
//! what else the first grep had missed — which was `apps/spreadsheet`. Six was
//! right by accident and wrong in its membership, twice. Enumerate, then count.)
//!
//! **They had drifted, and on the part that shows.** Each had a different rule
//! for the smallest a thumb may get: 20 px in the dialog, 16 px in `menu` and
//! `menubar`, half a row in the shell, five per cent of the pane in
//! `dictionary`. That is why [`thumb_of`] takes the floor as an argument
//! instead of imposing [`MIN_THUMB`]: a menu is not a file dialog, and
//! collapsing five deliberate-looking choices into one would be a visual change
//! smuggled in under a refactor. What is shared is the arithmetic.
//!
//! The extraction started from the file dialog's copy, which was the most
//! carefully documented and the only one with tests for the end-of-list case;
//! its tests are what guard that this module says the same thing it did.

use crate::frame::Rect;

/// Width a vertical scrollbar wants, when the list is long enough to have one.
///
/// A suggestion rather than a rule: a caller drawing into a tight pane may use
/// its own, and nothing here reads it.
pub const WIDTH: f32 = 10.0;

/// Shortest the thumb may get.
///
/// A thumb sized strictly in proportion to the visible fraction of a very long
/// listing shrinks to a couple of pixels, which is both invisible and too small
/// to grab. Every real scrollbar imposes a floor for the same reason; the cost
/// is that the thumb's *size* stops being a faithful proportion once the list
/// is long, which nobody reads it for, while its *position* stays exact.
pub const MIN_THUMB: f32 = 20.0;

/// Whether a list this long needs a scrollbar at all.
///
/// A caller that draws one unconditionally leaves a permanent grey stripe
/// beside a three-item list, and one that forgets to ask draws a full-height
/// thumb that cannot move — both of which read as "broken" rather than "empty".
#[must_use]
pub fn needed(total: usize, capacity: usize) -> bool {
    total > capacity
}

/// The thumb's rectangle inside `track`, for a list of `total` rows showing
/// `capacity` of them starting at row `first`.
///
/// Size shows how much of the listing is on screen; position shows where in it.
/// The two are computed separately because [`MIN_THUMB`] makes the size stop
/// being proportional for a long listing while the position must stay exact — a
/// thumb that reached the bottom of its track only when the last row was
/// reached, but sat at 90% when the listing was at its end, would be worse than
/// no thumb at all.
///
/// A `total` of zero yields a full-height thumb rather than a division by zero:
/// an empty list is entirely on screen.
#[must_use]
pub fn thumb(track: Rect, total: usize, capacity: usize, first: usize) -> Rect {
    if total == 0 {
        return track;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a row count large enough to lose f32 precision is 16M rows"
    )]
    let shown = capacity as f32 / total as f32;
    let hidden = total.saturating_sub(capacity);
    #[expect(
        clippy::cast_precision_loss,
        reason = "as above; the ratio is what matters and it is exact enough"
    )]
    let position = if hidden == 0 {
        0.0
    } else {
        first as f32 / hidden as f32
    };
    thumb_of(track, shown, position, MIN_THUMB)
}

/// The thumb's rectangle from two fractions, with an explicit size floor.
///
/// The shape underneath [`thumb`], for the callers that do not count rows: a
/// menu scrolls by pixels, and a pane by a fraction of its content. `shown` is
/// how much of the content is on screen and `position` how far through it the
/// view has travelled, both clamped to `0.0..=1.0`.
///
/// `min_thumb` is an argument rather than [`MIN_THUMB`] because the callers
/// disagree about it on purpose -- a menu's floor is smaller than a file
/// dialog's -- and imposing one value would be a visual change wearing a
/// refactor's clothes.
///
/// A non-finite fraction is read as zero. These come from a division whose
/// denominator a caller may not have checked, and a `NaN` reaching the
/// multiply would put the thumb at a position that compares false against
/// every bound.
#[must_use]
pub fn thumb_of(track: Rect, shown: f32, position: f32, min_thumb: f32) -> Rect {
    let shown = if shown.is_finite() {
        shown.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let position = if position.is_finite() {
        position.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb_h = (track.h * shown).clamp(min_thumb.min(track.h), track.h);
    Rect::new(
        track.x,
        track.y + (track.h - thumb_h) * position,
        track.w,
        thumb_h,
    )
}

/// The first visible row a thumb drag lands on.
///
/// `grab` is how far below the thumb's own top the pointer took hold, which is
/// what stops the thumb jumping under the pointer on the first move: the thumb
/// follows the grab point, not the pointer.
///
/// Returns `None` when there is nothing to scroll — an empty track, a thumb as
/// tall as its track, or a list that fits — so a caller does not move the view
/// to row zero on a drag that meant nothing.
#[must_use]
pub fn first_from_drag(
    track: Rect,
    thumb_h: f32,
    grab: f32,
    pointer_y: f32,
    total: usize,
    capacity: usize,
) -> Option<usize> {
    let span = track.h - thumb_h;
    let hidden = total.saturating_sub(capacity);
    if span <= 0.0 || hidden == 0 {
        return None;
    }
    let fraction = ((pointer_y - grab - track.y) / span).clamp(0.0, 1.0);
    #[expect(
        clippy::cast_precision_loss,
        reason = "the hidden-row count is far inside f32's exact range"
    )]
    let rows = fraction * hidden as f32;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "rounded, and bounded by `hidden` because `fraction` is clamped"
    )]
    let first = rows.round() as usize;
    Some(first.min(hidden))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    const TRACK: Rect = Rect {
        x: 100.0,
        y: 0.0,
        w: WIDTH,
        h: 200.0,
    };

    #[test]
    fn a_list_that_fits_needs_no_scrollbar() {
        assert!(!needed(10, 10));
        assert!(!needed(3, 10));
        assert!(needed(11, 10));
    }

    #[test]
    fn the_thumb_fills_the_track_when_everything_is_on_screen() {
        let t = thumb(TRACK, 10, 10, 0);
        assert_eq!(t.h, TRACK.h);
        assert_eq!(t.y, TRACK.y);
    }

    #[test]
    fn an_empty_list_does_not_divide_by_zero() {
        let t = thumb(TRACK, 0, 10, 0);
        assert_eq!(t.h, TRACK.h);
    }

    #[test]
    fn the_thumb_sits_at_the_end_when_the_list_is_at_its_end() {
        // The property the size floor puts at risk. A very long list gives a
        // thumb pinned to `MIN_THUMB`, so its *size* is no longer a faithful
        // proportion -- but its *position* must still reach the bottom of the
        // track exactly when the last row is showing, or the bar lies about
        // where you are.
        let total = 10_000;
        let capacity = 20;
        let t = thumb(TRACK, total, capacity, total - capacity);
        assert_eq!(
            t.h, MIN_THUMB,
            "a huge list should pin the thumb to the floor"
        );
        assert!(
            (t.y + t.h - (TRACK.y + TRACK.h)).abs() < 0.01,
            "the thumb stopped at {} instead of the track's end {}",
            t.y + t.h,
            TRACK.y + TRACK.h
        );
    }

    #[test]
    fn the_floor_is_the_callers_and_not_this_modules() {
        // Five callers disagree about the smallest a thumb may be -- 20 px in
        // the file dialog, 16 in the menus, half a row in the shell, three bar
        // widths in the dictionary -- and each is a deliberate choice about
        // its own surface. Imposing one would be a visual change hidden in a
        // refactor, so the floor is an argument.
        let tiny = thumb_of(TRACK, 0.001, 0.0, 4.0);
        assert_eq!(tiny.h, 4.0, "the caller's floor was not honoured");
        let bigger = thumb_of(TRACK, 0.001, 0.0, 40.0);
        assert_eq!(bigger.h, 40.0);
    }

    #[test]
    fn a_floor_taller_than_the_track_does_not_overflow_it() {
        let t = thumb_of(TRACK, 0.01, 1.0, TRACK.h * 4.0);
        assert_eq!(t.h, TRACK.h);
        assert_eq!(t.y, TRACK.y, "a full-height thumb has nowhere to travel");
    }

    #[test]
    fn a_non_finite_fraction_does_not_put_the_thumb_nowhere() {
        // Both fractions come from a division whose denominator the caller may
        // not have checked -- content height, a row count, a max-scroll of
        // zero. A NaN reaching the multiply compares false against every bound,
        // so the thumb would be drawn at a position nothing could clamp.
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let t = thumb_of(TRACK, bad, 0.5, MIN_THUMB);
            assert!(
                t.h.is_finite() && t.y.is_finite(),
                "shown={bad} produced {t:?}"
            );
            let t = thumb_of(TRACK, 0.5, bad, MIN_THUMB);
            assert!(
                t.h.is_finite() && t.y.is_finite(),
                "position={bad} produced {t:?}"
            );
            assert!(t.y >= TRACK.y && t.y + t.h <= TRACK.y + TRACK.h + 0.01);
        }
    }

    #[test]
    fn the_thumb_never_leaves_its_track() {
        for first in [0, 1, 50, 499, 500, 10_000] {
            let t = thumb(TRACK, 500, 20, first);
            assert!(t.y >= TRACK.y, "thumb above the track at first={first}");
            assert!(
                t.y + t.h <= TRACK.y + TRACK.h + 0.01,
                "thumb below the track at first={first}"
            );
        }
    }

    #[test]
    fn dragging_to_the_bottom_shows_the_last_page() {
        let t = thumb(TRACK, 100, 10, 0);
        let first = first_from_drag(TRACK, t.h, 0.0, TRACK.y + TRACK.h, 100, 10);
        assert_eq!(
            first,
            Some(90),
            "a drag to the bottom should show the last page"
        );
    }

    #[test]
    fn dragging_above_the_track_shows_the_first_page() {
        let t = thumb(TRACK, 100, 10, 50);
        let first = first_from_drag(TRACK, t.h, 0.0, TRACK.y - 500.0, 100, 10);
        assert_eq!(first, Some(0));
    }

    #[test]
    fn the_grab_offset_keeps_the_thumb_under_the_pointer() {
        // Grabbing the thumb halfway down and moving nowhere must not move the
        // view: without the offset the thumb's *top* would jump to the pointer.
        let t = thumb(TRACK, 100, 10, 40);
        let grab = t.h / 2.0;
        let first = first_from_drag(TRACK, t.h, grab, t.y + grab, 100, 10);
        assert_eq!(first, Some(40), "a drag that moved nowhere moved the view");
    }

    #[test]
    fn a_drag_with_nothing_to_scroll_reports_nothing() {
        assert_eq!(first_from_drag(TRACK, TRACK.h, 0.0, 50.0, 10, 10), None);
        assert_eq!(first_from_drag(TRACK, 20.0, 0.0, 50.0, 5, 10), None);
    }
}
