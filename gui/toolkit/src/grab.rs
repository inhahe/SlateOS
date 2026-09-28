//! How far past what is drawn a thing you drag can still be taken hold of.
//!
//! `roadmap-detailed.md` §3.5 asks for this twice. *Forgiving drag margins*:
//! "users should never have to pixel-hunt to start a resize/reposition drag".
//! *Generous hit-regions for draggable control handles*: "the *interactive*
//! region is decoupled from and larger than the *drawn* region ...
//! implemented once in the toolkit's hit-testing layer so every widget
//! inherits it uniformly". This module is the once: the two rules, their
//! numbers, and the arithmetic, which [`crate::splitter`] and
//! [`crate::slider`] use and anything else draggable should.
//!
//! # Two rules, because there are two kinds of draggable thing
//!
//! **An edge between two areas** -- a splitter's divider, a column's border in
//! a table's header, a window's frame -- has something clickable on both sides
//! of it. Its region can only grow a little, or a click meant for the first row
//! of the pane beside it starts a resize instead: [`edge`] adds
//! [`EDGE_MARGIN`] either side, across the edge only.
//!
//! **A handle in space of its own** -- a slider's thumb and track, a
//! scrollbar's thumb, a resize grip -- has nothing else to click where it
//! sits, so its region can be as large as a target ought to be: [`handle`]
//! adds [`HANDLE_MARGIN`] on every side, then grows any direction still short
//! of [`MIN_TARGET`] to it, centred on what is drawn.
//!
//! **A handle that runs in a track of its own** -- a scrollbar's thumb -- is
//! between the two. Along the track the handle can grow, since the rest of the
//! track is the scrollbar's too; across it the track is already the whole
//! column the scrollbar owns, and growing past it would take clicks from the
//! list beside it. [`in_track`] grows the thumb along the track only, and
//! never outside it. (A scrollbar is narrower than [`MIN_TARGET`]; that is the
//! width every desktop draws one, and making the column wider is a layout
//! choice, not a hit-test one.)
//!
//! Where two handles' regions overlap -- two sliders stacked close, a thumb at
//! the end of its track beside another control's -- [`nearest`] gives the
//! press to the one whose *drawn* shape is closer, so the answer never depends
//! on which was tested first.
//!
//! # In logical pixels, so the regions scale with everything else
//!
//! Every number here is in the toolkit's logical pixels, as all of its geometry
//! is (see [`crate::scaling`]). Whatever enlarges a program's drawing for a
//! dense display enlarges these regions with it, so a margin stays the same
//! size to a hand. A margin counted in device pixels would shrink to half on a
//! display at 200% -- the display on which a small target is already hardest
//! to hit.
//!
//! # What a caller does with a region
//!
//! Tests the pointer against it, or records it as the control's hit box
//! ([`Frame::hit`](crate::frame::Frame::hit)), *instead of* the drawn
//! rectangle. The drawing does not change: the point is that what the user
//! sees and what the pointer can take hold of are two different sizes.

use crate::frame::Rect;
use crate::layout::Axis;

/// How far past each side of a drawn edge a press still takes hold of it.
///
/// Three pixels either side of a two-pixel divider is an eight-pixel target:
/// comfortable with a mouse, and still narrow enough that a click meant for
/// the row beside the edge does not start a drag instead. It is the value the
/// splitter chose first; `splitter::GRAB_MARGIN` is this constant.
pub const EDGE_MARGIN: f32 = 3.0;

/// How far past a handle's drawn edge its region always reaches, however
/// large the handle is drawn.
///
/// A pointer on the rim of a thumb, or just past it, is where a hand lands
/// when it overshoots, and it means the thumb.
pub const HANDLE_MARGIN: f32 = 4.0;

/// The smallest a handle's region may be, in either direction.
///
/// WCAG 2.2's success criterion 2.5.8, *Target Size (Minimum)*: a pointer
/// target of at least 24 by 24 CSS pixels. A slider's four-pixel track and its
/// twelve-pixel thumb are both well under that as drawn, and neither needs to
/// be drawn larger to be seen -- only to be hit.
pub const MIN_TARGET: f32 = 24.0;

/// The region a press takes hold of an edge drawn at `drawn` in.
///
/// `axis` is the direction the edge is dragged in: `Horizontal` for the
/// divider between side-by-side panes (a vertical line, moved left and right),
/// `Vertical` for one between stacked panes. The region grows across the edge
/// by [`EDGE_MARGIN`] either side and not at all along it, because along it
/// are the panes' own edges, and they are not the divider's.
#[must_use]
pub fn edge(drawn: Rect, axis: Axis) -> Rect {
    let drawn = sane(drawn);
    match axis {
        Axis::Horizontal => Rect::new(
            drawn.x - EDGE_MARGIN,
            drawn.y,
            drawn.w + EDGE_MARGIN * 2.0,
            drawn.h,
        ),
        Axis::Vertical => Rect::new(
            drawn.x,
            drawn.y - EDGE_MARGIN,
            drawn.w,
            drawn.h + EDGE_MARGIN * 2.0,
        ),
    }
}

/// The region a press takes hold of a handle drawn at `drawn` in.
///
/// `drawn` grown by [`HANDLE_MARGIN`] on every side, and further in any
/// direction that would otherwise be narrower than [`MIN_TARGET`] -- equally
/// on both sides, so the region stays centred on what the user is aiming at.
#[must_use]
pub fn handle(drawn: Rect) -> Rect {
    let drawn = sane(drawn);
    let grow = |extent: f32| HANDLE_MARGIN.max((MIN_TARGET - extent) / 2.0);
    let (gx, gy) = (grow(drawn.w), grow(drawn.h));
    Rect::new(
        drawn.x - gx,
        drawn.y - gy,
        drawn.w + gx * 2.0,
        drawn.h + gy * 2.0,
    )
}

/// The region a press takes hold of a thumb drawn at `thumb` in, when it runs
/// along `track` in the direction `axis`.
///
/// Grown along the track by [`HANDLE_MARGIN`] each way, and further to
/// [`MIN_TARGET`] if the thumb is shorter; as wide as the track across it; and
/// never outside the track. So a press on the track a few pixels past the
/// thumb's end takes hold of the thumb rather than paging the list, and a
/// press in the list beside the scrollbar is still the list's.
#[must_use]
pub fn in_track(thumb: Rect, track: Rect, axis: Axis) -> Rect {
    let (thumb, track) = (sane(thumb), sane(track));
    let grow = |extent: f32| HANDLE_MARGIN.max((MIN_TARGET - extent) / 2.0);
    let grown = match axis {
        Axis::Horizontal => {
            let g = grow(thumb.w);
            Rect::new(thumb.x - g, track.y, thumb.w + g * 2.0, track.h)
        }
        Axis::Vertical => {
            let g = grow(thumb.h);
            Rect::new(track.x, thumb.y - g, track.w, thumb.h + g * 2.0)
        }
    };
    grown
        .intersect(track)
        .unwrap_or(Rect::new(track.x, track.y, 0.0, 0.0))
}

/// How far `(x, y)` is from the nearest point of `rect`: zero inside it.
///
/// The measure [`nearest`] ranks by. Straight-line distance rather than the
/// larger of the two axis distances, so that a press diagonally off a corner
/// is not treated as nearer than one the same distance straight off a side.
#[must_use]
pub fn distance(rect: Rect, x: f32, y: f32) -> f32 {
    // A pointer that is not at a number is nowhere near anything. Asked here
    // rather than of the result, because `f32::max` returns its other operand
    // when one is NaN: the arithmetic below would quietly call it zero -- on
    // the rectangle -- instead.
    if x.is_nan() || y.is_nan() {
        return f32::INFINITY;
    }
    let rect = sane(rect);
    let dx = (rect.x - x).max(x - rect.right()).max(0.0);
    let dy = (rect.y - y).max(y - rect.bottom()).max(0.0);
    dx.hypot(dy)
}

/// Which of several handles a press at `(x, y)` takes hold of.
///
/// Each candidate is its target and its *drawn* rectangle. The press belongs to
/// a handle when it lands in that handle's [`handle`] region; when it lands in
/// several, to the one whose drawn rectangle is nearest, and on a tie to the
/// earlier candidate -- so the answer is decided by geometry and, only where
/// geometry cannot decide, by an order the caller chose on purpose.
#[must_use]
pub fn nearest<T>(candidates: impl IntoIterator<Item = (T, Rect)>, x: f32, y: f32) -> Option<T> {
    let mut best: Option<(T, f32)> = None;
    for (target, drawn) in candidates {
        if !handle(drawn).contains(x, y) {
            continue;
        }
        let d = distance(drawn, x, y);
        if best.as_ref().is_none_or(|(_, b)| d < *b) {
            best = Some((target, d));
        }
    }
    best.map(|(target, _)| target)
}

/// `rect` with a negative or non-finite size read as empty and a non-finite
/// corner as the origin, so the arithmetic above cannot turn one bad number
/// from a caller's layout into a region covering the whole window.
fn sane(rect: Rect) -> Rect {
    let finite_or = |v: f32, or: f32| if v.is_finite() { v } else { or };
    Rect::new(
        finite_or(rect.x, 0.0),
        finite_or(rect.y, 0.0),
        finite_or(rect.w, 0.0).max(0.0),
        finite_or(rect.h, 0.0).max(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    /// A two-pixel divider is an eight-pixel target across, and no larger
    /// along its length.
    #[test]
    fn an_edge_grows_across_itself_only() {
        let divider = Rect::new(100.0, 10.0, 2.0, 300.0);
        let r = edge(divider, Axis::Horizontal);
        assert!(close(r.x, 97.0) && close(r.w, 8.0), "{r:?}");
        assert!(close(r.y, 10.0) && close(r.h, 300.0), "{r:?}");

        let divider = Rect::new(10.0, 100.0, 300.0, 2.0);
        let r = edge(divider, Axis::Vertical);
        assert!(close(r.y, 97.0) && close(r.h, 8.0), "{r:?}");
        assert!(close(r.x, 10.0) && close(r.w, 300.0), "{r:?}");
    }

    /// A small handle is grown to the minimum target in both directions,
    /// centred on itself.
    #[test]
    fn a_small_handle_is_grown_to_the_minimum_target_around_its_centre() {
        let thumb = Rect::new(50.0, 50.0, 12.0, 12.0);
        let r = handle(thumb);
        assert!(close(r.w, MIN_TARGET) && close(r.h, MIN_TARGET), "{r:?}");
        assert!(
            close(r.x + r.w / 2.0, 56.0) && close(r.y + r.h / 2.0, 56.0),
            "{r:?}"
        );
    }

    /// A handle already larger than the minimum still gets the margin: the rim
    /// and just past it mean the handle, however big it is drawn.
    #[test]
    fn a_large_handle_still_gets_the_margin() {
        let grip = Rect::new(0.0, 0.0, 40.0, 30.0);
        let r = handle(grip);
        assert!(
            close(r.x, -HANDLE_MARGIN) && close(r.w, 40.0 + HANDLE_MARGIN * 2.0),
            "{r:?}"
        );
        assert!(
            close(r.y, -HANDLE_MARGIN) && close(r.h, 30.0 + HANDLE_MARGIN * 2.0),
            "{r:?}"
        );
    }

    /// A long thin track is grown across to the minimum and by the margin along.
    #[test]
    fn a_thin_track_is_grown_across_to_the_minimum() {
        let track = Rect::new(0.0, 100.0, 150.0, 4.0);
        let r = handle(track);
        assert!(close(r.h, MIN_TARGET), "{r:?}");
        assert!(close(r.w, 150.0 + HANDLE_MARGIN * 2.0), "{r:?}");
        assert!(close(r.y + r.h / 2.0, 102.0), "centred on the track: {r:?}");
    }

    /// A scrollbar thumb grows along its track only, and never out of it.
    #[test]
    fn a_thumb_in_a_track_grows_along_it_and_stays_inside_it() {
        let track = Rect::new(390.0, 0.0, 10.0, 300.0);
        let thumb = Rect::new(390.0, 100.0, 10.0, 40.0);
        let r = in_track(thumb, track, Axis::Vertical);
        assert!(
            close(r.x, 390.0) && close(r.w, 10.0),
            "not wider than the track: {r:?}"
        );
        assert!(
            close(r.y, 100.0 - HANDLE_MARGIN) && close(r.h, 40.0 + HANDLE_MARGIN * 2.0),
            "{r:?}"
        );

        // A short thumb reaches the minimum along the track.
        let short = Rect::new(390.0, 100.0, 10.0, 12.0);
        let r = in_track(short, track, Axis::Vertical);
        assert!(close(r.h, MIN_TARGET), "{r:?}");

        // At the top of the track the region stops there.
        let top = Rect::new(390.0, 0.0, 10.0, 40.0);
        let r = in_track(top, track, Axis::Vertical);
        assert!(close(r.y, 0.0) && close(r.h, 40.0 + HANDLE_MARGIN), "{r:?}");

        // Horizontal: the same, turned.
        let track = Rect::new(0.0, 290.0, 300.0, 10.0);
        let thumb = Rect::new(50.0, 290.0, 30.0, 10.0);
        let r = in_track(thumb, track, Axis::Horizontal);
        assert!(close(r.y, 290.0) && close(r.h, 10.0), "{r:?}");
        assert!(
            close(r.x, 50.0 - HANDLE_MARGIN) && close(r.w, 30.0 + HANDLE_MARGIN * 2.0),
            "{r:?}"
        );
    }

    /// Distance is zero inside, straight-line outside, and never NaN.
    #[test]
    fn distance_is_zero_inside_and_euclidean_outside() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(close(distance(r, 5.0, 5.0), 0.0));
        assert!(close(distance(r, 13.0, 5.0), 3.0));
        assert!(
            close(distance(r, 13.0, 14.0), 5.0),
            "a 3-4-5 triangle off the corner"
        );
        assert!(distance(r, f32::NAN, 5.0).is_infinite());
    }

    /// Two handles whose regions overlap: the press goes to the one drawn
    /// nearer to it, whichever order they are listed in.
    #[test]
    fn overlapping_regions_go_to_the_nearer_drawn_handle() {
        // Two 12-pixel thumbs with a 4-pixel gap between them: a's region runs
        // to x = 18 and b's from x = 10, so they overlap from 10 to 18.
        let a = Rect::new(0.0, 0.0, 12.0, 12.0);
        let b = Rect::new(16.0, 0.0, 12.0, 12.0);
        assert!(handle(a).contains(13.0, 6.0) && handle(b).contains(13.0, 6.0));
        assert!(handle(a).contains(15.0, 6.0) && handle(b).contains(15.0, 6.0));
        // x = 13 is 1 from a's edge (12) and 3 from b's (16).
        assert_eq!(nearest([(1, a), (2, b)], 13.0, 6.0), Some(1));
        assert_eq!(nearest([(2, b), (1, a)], 13.0, 6.0), Some(1));
        // x = 15 is 3 from a and 1 from b.
        assert_eq!(nearest([(1, a), (2, b)], 15.0, 6.0), Some(2));
        assert_eq!(nearest([(2, b), (1, a)], 15.0, 6.0), Some(2));
        // Outside both regions: nothing.
        assert_eq!(nearest([(1, a), (2, b)], 100.0, 6.0), None);
    }

    /// A tie goes to the earlier candidate, as documented.
    #[test]
    fn a_tie_goes_to_the_earlier_candidate() {
        let a = Rect::new(0.0, 0.0, 12.0, 12.0);
        let b = Rect::new(20.0, 0.0, 12.0, 12.0);
        assert_eq!(nearest([(1, a), (2, b)], 16.0, 6.0), Some(1));
        assert_eq!(nearest([(2, b), (1, a)], 16.0, 6.0), Some(2));
    }

    /// A layout that produced a negative or non-finite size gets an empty
    /// shape grown by the rules, not a region spanning the window.
    #[test]
    fn a_bad_rectangle_cannot_become_a_huge_region() {
        let r = handle(Rect::new(10.0, 10.0, f32::INFINITY, -5.0));
        assert!(close(r.w, MIN_TARGET) && close(r.h, MIN_TARGET), "{r:?}");
        let r = edge(Rect::new(f32::NAN, 0.0, 2.0, 10.0), Axis::Horizontal);
        assert!(r.x.is_finite() && close(r.w, 8.0), "{r:?}");
    }
}
