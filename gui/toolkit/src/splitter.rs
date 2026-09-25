//! Dividing an area into panes the user can resize by dragging between them.
//!
//! The structural half of `roadmap-detailed.md`'s *Dockable Panel / Splitter
//! Layout Widget*: nested horizontal and vertical splits, draggable dividers,
//! and a minimum size per pane. Rearranging panels, tabs when several share a
//! region, and layout serialisation are the other half and are not here.
//!
//! # Pure functions, not a widget
//!
//! Like [`crate::scrollbar`], this computes geometry and hands it back; the
//! caller keeps the state. That is the pattern the toolkit already uses for
//! the one other draggable divider it has, and it is what makes every case
//! below testable without a compositor -- which matters, because every user
//! interface test on this project runs headless.
//!
//! # Fractions, not pixels
//!
//! A layout is `n` fractions summing to 1. Storing pixels would mean a saved
//! arrangement that only fits the window it was saved on; fractions survive a
//! resize, and the minimum sizes below are what stop them collapsing when the
//! window gets small.
//!
//! # The grab region is larger than the line
//!
//! `roadmap-detailed.md` asks for this explicitly, for splitters by name:
//! *"the drag hit-region extends a few pixels past the visible margin on both
//! sides ... users should never have to pixel-hunt to start a resize"*. So
//! [`divider_at`] takes the drawn thickness and adds [`GRAB_MARGIN`] either
//! side, and a two-pixel line is an eight-pixel target.

use crate::frame::Rect;
use crate::layout::Axis;

/// How far past each side of a drawn divider a grab still counts.
///
/// Three pixels either side of a two-pixel line gives an eight-pixel target,
/// which is comfortable with a mouse and still narrow enough that a click
/// meant for a pane's first row does not start a drag instead.
pub const GRAB_MARGIN: f32 = 3.0;

/// The thickness a divider is normally drawn at.
pub const DIVIDER: f32 = 2.0;

/// Fractions that divide an area into `n` equal panes.
#[must_use]
pub fn even(n: usize) -> Vec<f32> {
    if n == 0 {
        return Vec::new();
    }
    // `1.0 / n` for each: exact for powers of two and close enough otherwise,
    // and `panes` derives the last pane from what is left rather than from its
    // own fraction, so the rounding cannot leave a gap at the edge.
    let share = 1.0 / n as f32;
    vec![share; n]
}

/// The rectangles `fractions` divide `area` into along `axis`.
///
/// The last pane takes whatever remains rather than its own fraction, so a
/// rounding error cannot leave a one-pixel strip of background showing down
/// the edge -- the error lands inside the last pane, where nothing can see it.
#[must_use]
pub fn panes(area: Rect, axis: Axis, fractions: &[f32], divider: f32) -> Vec<Rect> {
    let n = fractions.len();
    if n == 0 {
        return Vec::new();
    }
    let gaps = divider * (n.saturating_sub(1)) as f32;
    let span = match axis {
        Axis::Horizontal => area.w,
        Axis::Vertical => area.h,
    };
    let usable = (span - gaps).max(0.0);

    let mut out = Vec::with_capacity(n);
    let mut offset = 0.0f32;
    for (i, f) in fractions.iter().enumerate() {
        let is_last = i.saturating_add(1) == n;
        let size = if is_last {
            (span - offset).max(0.0)
        } else {
            (usable * f).max(0.0)
        };
        out.push(match axis {
            Axis::Horizontal => Rect::new(area.x + offset, area.y, size, area.h),
            Axis::Vertical => Rect::new(area.x, area.y + offset, area.w, size),
        });
        offset += size + divider;
    }
    out
}

/// The divider rectangles between those panes: one fewer than there are panes.
#[must_use]
pub fn dividers(area: Rect, axis: Axis, fractions: &[f32], divider: f32) -> Vec<Rect> {
    let rects = panes(area, axis, fractions, divider);
    rects
        .windows(2)
        .map(|pair| {
            let first = pair.first().copied().unwrap_or(area);
            match axis {
                Axis::Horizontal => Rect::new(first.x + first.w, area.y, divider, area.h),
                Axis::Vertical => Rect::new(area.x, first.y + first.h, area.w, divider),
            }
        })
        .collect()
}

/// Which divider is under the pointer, if any.
///
/// The hit region is the drawn divider plus [`GRAB_MARGIN`] on each side. When
/// two dividers are close enough for their grab regions to overlap -- a pane
/// squeezed to nothing between them -- the nearer one wins, so the answer is
/// never decided by which happens to be tested first.
#[must_use]
pub fn divider_at(
    area: Rect,
    axis: Axis,
    fractions: &[f32],
    divider: f32,
    x: f32,
    y: f32,
) -> Option<usize> {
    let (pointer, across, across_min, across_max) = match axis {
        Axis::Horizontal => (x, y, area.y, area.y + area.h),
        Axis::Vertical => (y, x, area.x, area.x + area.w),
    };
    if across < across_min || across > across_max {
        return None;
    }

    let mut best: Option<(usize, f32)> = None;
    for (i, rect) in dividers(area, axis, fractions, divider).iter().enumerate() {
        let (start, end) = match axis {
            Axis::Horizontal => (rect.x, rect.x + rect.w),
            Axis::Vertical => (rect.y, rect.y + rect.h),
        };
        if pointer < start - GRAB_MARGIN || pointer > end + GRAB_MARGIN {
            continue;
        }
        let centre = f32::midpoint(start, end);
        let distance = (pointer - centre).abs();
        if best.is_none_or(|(_, d)| distance < d) {
            best = Some((i, distance));
        }
    }
    best.map(|(i, _)| i)
}

/// Move divider `index` so its centre lands at `position`, in pixels from the
/// start of `area` along `axis`.
///
/// Only the two panes either side of the divider change: that is what a
/// splitter does, and moving the others would make a drag at one edge shuffle
/// the whole layout. Answers whether anything moved.
///
/// `min_px` is the smallest each pane may become. A drag past a neighbour's
/// minimum stops there rather than being refused, because a drag that simply
/// does nothing feels broken -- the divider should follow the pointer until it
/// cannot, and then stay.
pub fn resize(
    fractions: &mut [f32],
    index: usize,
    position: f32,
    span: f32,
    divider: f32,
    min_px: &[f32],
) -> bool {
    let n = fractions.len();
    if index.saturating_add(1) >= n || span <= 0.0 {
        return false;
    }
    let gaps = divider * (n.saturating_sub(1)) as f32;
    let usable = span - gaps;
    if usable <= 0.0 {
        return false;
    }

    // Where the pair starts, in pixels: everything before it is unchanged.
    let before: f32 = fractions.iter().take(index).sum::<f32>() * usable;
    let a = fractions.get(index).copied().unwrap_or(0.0);
    let b = fractions
        .get(index.saturating_add(1))
        .copied()
        .unwrap_or(0.0);
    let pair = (a + b) * usable;
    if pair <= 0.0 {
        return false;
    }

    let min_a = min_px.get(index).copied().unwrap_or(0.0);
    let min_b = min_px.get(index.saturating_add(1)).copied().unwrap_or(0.0);
    if min_a + min_b > pair {
        // The pair cannot satisfy both minimums, so there is no position that
        // is not a lie. Left alone rather than split at some ratio nobody
        // asked for.
        return false;
    }

    let wanted = position - before - (divider / 2.0);
    let new_a = wanted.clamp(min_a, pair - min_b);
    let new_b = pair - new_a;

    let changed = (new_a - a * usable).abs() > f32::EPSILON;
    if let Some(slot) = fractions.get_mut(index) {
        *slot = new_a / usable;
    }
    if let Some(slot) = fractions.get_mut(index.saturating_add(1)) {
        *slot = new_b / usable;
    }
    changed
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "a test that indexes out of range should fail loudly at the line that did it"
)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(0.0, 0.0, 300.0, 200.0)
    }

    /// Two even panes share the space, less the divider between them.
    #[test]
    fn two_even_panes_split_the_space() {
        let p = panes(area(), Axis::Horizontal, &even(2), DIVIDER);
        assert_eq!(p.len(), 2);
        assert!((p[0].w - 149.0).abs() < 0.01, "{:?}", p[0]);
        assert!((p[1].x - 151.0).abs() < 0.01, "{:?}", p[1]);
        // The last pane runs to the edge exactly: no background strip.
        assert!((p[1].x + p[1].w - 300.0).abs() < 0.01, "{:?}", p[1]);
    }

    /// The panes never overlap and never leave a gap but the dividers.
    #[test]
    fn panes_tile_the_area_exactly() {
        let f = vec![0.2, 0.5, 0.3];
        let p = panes(area(), Axis::Horizontal, &f, DIVIDER);
        for pair in p.windows(2) {
            let gap = pair[1].x - (pair[0].x + pair[0].w);
            assert!((gap - DIVIDER).abs() < 0.01, "gap was {gap}");
        }
        // EXACT equality, not a tolerance. The last pane takes what is left
        // rather than its own fraction precisely so this holds to the bit; a
        // tolerance here passed against code with the remainder logic removed,
        // which means it was testing nothing. Any float error lands inside the
        // last pane, where no background shows through.
        let last = p.last().expect("three panes");
        assert_eq!(
            last.x + last.w,
            area().x + area().w,
            "the last pane must reach the edge exactly"
        );
    }

    /// The same, with fractions that do not divide cleanly.
    ///
    /// Thirds are not representable in binary, so this is where a
    /// fraction-per-pane implementation drifts and a remainder one does not.
    #[test]
    fn an_uneven_split_still_reaches_the_edge_exactly() {
        for n in [3usize, 7, 11] {
            let p = panes(area(), Axis::Horizontal, &even(n), DIVIDER);
            let last = p.last().expect("panes");
            assert_eq!(
                last.x + last.w,
                area().x + area().w,
                "{n} panes left a gap at the edge"
            );
        }
    }

    /// A vertical split divides height instead of width.
    #[test]
    fn a_vertical_split_divides_height() {
        let p = panes(area(), Axis::Vertical, &even(2), DIVIDER);
        assert!((p[0].h - 99.0).abs() < 0.01, "{:?}", p[0]);
        assert!((p[0].w - 300.0).abs() < 0.01, "a pane spans the other axis");
    }

    /// There is one divider fewer than there are panes.
    #[test]
    fn dividers_sit_between_panes() {
        let d = dividers(area(), Axis::Horizontal, &even(3), DIVIDER);
        assert_eq!(d.len(), 2);
        let p = panes(area(), Axis::Horizontal, &even(3), DIVIDER);
        assert!((d[0].x - (p[0].x + p[0].w)).abs() < 0.01);
    }

    /// The grab region is wider than the line, on both sides.
    ///
    /// The roadmap asks for this by name: a user should never have to
    /// pixel-hunt to start a resize.
    #[test]
    fn a_divider_can_be_grabbed_from_just_beside_it() {
        let f = even(2);
        let d = dividers(area(), Axis::Horizontal, &f, DIVIDER)[0];
        let exact = d.x + d.w / 2.0;

        assert_eq!(
            divider_at(area(), Axis::Horizontal, &f, DIVIDER, exact, 100.0),
            Some(0),
            "the line itself must be grabbable"
        );
        for offset in [-(GRAB_MARGIN), GRAB_MARGIN] {
            assert_eq!(
                divider_at(area(), Axis::Horizontal, &f, DIVIDER, exact + offset, 100.0),
                Some(0),
                "a grab {offset} from the centre should still count"
            );
        }
    }

    /// Far from any divider is not a grab.
    #[test]
    fn the_middle_of_a_pane_is_not_a_divider() {
        let f = even(2);
        assert_eq!(
            divider_at(area(), Axis::Horizontal, &f, DIVIDER, 40.0, 100.0),
            None
        );
    }

    /// Outside the area entirely is not a grab either.
    #[test]
    fn outside_the_area_is_not_a_divider() {
        let f = even(2);
        let d = dividers(area(), Axis::Horizontal, &f, DIVIDER)[0];
        assert_eq!(
            divider_at(area(), Axis::Horizontal, &f, DIVIDER, d.x, 900.0),
            None,
            "a grab below the panes is not a grab on the divider"
        );
    }

    /// Dragging moves the boundary and leaves the rest alone.
    #[test]
    fn a_drag_moves_only_the_pair_it_is_between() {
        let mut f = vec![0.25, 0.25, 0.5];
        let third = f[2];
        assert!(resize(&mut f, 0, 30.0, 300.0, DIVIDER, &[0.0; 3]));
        assert_eq!(f[2], third, "a drag at one edge moved a distant pane");
        assert!((f[0] + f[1] + f[2] - 1.0).abs() < 0.001, "{f:?}");
    }

    /// A pane will not be dragged below its minimum.
    #[test]
    fn a_minimum_stops_the_drag_rather_than_refusing_it() {
        let mut f = even(2);
        // Aim far past the left edge; the divider should stop at the minimum.
        resize(&mut f, 0, -500.0, 300.0, DIVIDER, &[80.0, 0.0]);
        let usable = 300.0 - DIVIDER;
        assert!(
            (f[0] * usable - 80.0).abs() < 0.01,
            "expected to stop at the minimum, got {}",
            f[0] * usable
        );
    }

    /// And the neighbour's minimum stops it from the other side.
    #[test]
    fn the_neighbours_minimum_stops_it_too() {
        let mut f = even(2);
        resize(&mut f, 0, 9_000.0, 300.0, DIVIDER, &[0.0, 60.0]);
        let usable = 300.0 - DIVIDER;
        assert!(
            (f[1] * usable - 60.0).abs() < 0.01,
            "expected the neighbour to keep its minimum, got {}",
            f[1] * usable
        );
    }

    /// When the pair cannot satisfy both minimums, nothing moves.
    ///
    /// Any split would be a lie, and inventing one is worse than leaving the
    /// layout as the user last saw it.
    #[test]
    fn an_impossible_pair_is_left_alone() {
        let mut f = even(2);
        let before = f.clone();
        assert!(!resize(&mut f, 0, 150.0, 100.0, DIVIDER, &[80.0, 80.0]));
        assert_eq!(f, before);
    }

    /// A divider index past the end is refused rather than clamped.
    #[test]
    fn a_divider_that_is_not_there_is_refused() {
        let mut f = even(2);
        assert!(!resize(&mut f, 1, 50.0, 300.0, DIVIDER, &[0.0; 2]));
        assert!(!resize(&mut f, 99, 50.0, 300.0, DIVIDER, &[0.0; 2]));
    }

    /// No panes is not a crash.
    #[test]
    fn an_empty_layout_is_empty_rather_than_a_panic() {
        assert!(panes(area(), Axis::Horizontal, &[], DIVIDER).is_empty());
        assert!(dividers(area(), Axis::Horizontal, &[], DIVIDER).is_empty());
        assert_eq!(
            divider_at(area(), Axis::Horizontal, &[], DIVIDER, 10.0, 10.0),
            None
        );
        assert!(even(0).is_empty());
    }

    /// An area too small for its dividers does not produce negative widths.
    #[test]
    fn a_tiny_area_never_yields_a_negative_pane() {
        let tiny = Rect::new(0.0, 0.0, 1.0, 10.0);
        for r in panes(tiny, Axis::Horizontal, &even(4), DIVIDER) {
            assert!(r.w >= 0.0, "negative width: {r:?}");
        }
    }
}
