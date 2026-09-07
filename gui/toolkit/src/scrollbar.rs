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
//! Six places in this tree drew a scrollbar and dragged a thumb, each with its
//! own copy of the same two formulas: `gui/toolkit`'s file dialog, menu and
//! menubar, the desktop shell and its window-peek strip, and
//! `apps/dictionary`. Two of them had already drifted on the minimum thumb
//! height. Six copies of one formula is six chances to write a different one —
//! the same reasoning that collapsed the glob matchers (design-decisions 555).
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
    let shown = (capacity as f32 / total as f32).clamp(0.0, 1.0);
    let thumb_h = (track.h * shown).clamp(MIN_THUMB.min(track.h), track.h);
    let hidden = total.saturating_sub(capacity);
    #[expect(
        clippy::cast_precision_loss,
        reason = "as above; the ratio is what matters and it is exact enough"
    )]
    let position = if hidden == 0 {
        0.0
    } else {
        (first as f32 / hidden as f32).clamp(0.0, 1.0)
    };
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
