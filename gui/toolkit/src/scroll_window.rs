//! Which rows of a scrollable list actually fit in the space it is drawn into.
//!
//! Several settings panels draw a variable-length list of fixed-height rows into
//! a panel of fixed height, and each carries a `scroll_offset` field that the
//! outside world is free to set. Left to themselves they each got it wrong in a
//! different way:
//!
//! - `touchpad.rs` and `bluetooth.rs` looped over the whole list and never
//!   consulted the panel height at all, so a list longer than the panel drew
//!   straight through the bottom edge and over whatever was beneath it. Their
//!   `scroll_offset` fields were never read, so the list could not be scrolled
//!   back into view either — the tail was simply unreachable.
//! - `window_rules.rs` did bound its list, but by way of an `end - start`
//!   subtraction that underflowed when the list shrank below the offset.
//!
//! All three want the same three-line answer, so it lives here once and is
//! tested once. The policy choices worth naming, because they are the ones a
//! caller would otherwise have to re-derive:
//!
//! - **A partially-visible row is not visible.** Capacity truncates rather than
//!   rounding up, so nothing is ever drawn across the bottom edge. A panel that
//!   cannot fit even one row shows none.
//! - **An offset past the end shows the last page, not an empty list.** The
//!   offset is a request, not an index — clamping it to `total - capacity` means
//!   a list that shrinks under a stale offset scrolls up to meet it instead of
//!   going blank. This is the case that used to underflow.
//! - **Nothing here mutates the offset.** `visible` is a pure function of its
//!   arguments so that `render(&self)` can call it; the clamping is applied to
//!   the *result*, and the caller's stored offset is left alone. A caller that
//!   wants the stored value clamped too can use [`Rows::start`].
//!
//! # Which of these two do I want?
//!
//! [`ListViewport`] answers a *larger* question — where a keyboard-navigated
//! list is looking **and which row is picked** — by mutating both together, so
//! that the picked row is never off screen. Reach for it when arrow keys move a
//! selection.
//!
//! This module is the stateless half: given a length, a height and an offset
//! somebody else owns, which rows are on screen? Reach for it when the panel
//! renders from `&self` and cannot write anything back, or when there is no
//! selection to keep visible — a settings panel listing bound gestures, or
//! paired Bluetooth devices.
//!
//! The two cannot drift, because [`ListViewport::visible_range`] is implemented
//! in terms of [`visible_count`]: the last-page clamp below is the one they both
//! apply. It was written here first and was missing there, which is how a
//! `ListViewport` over a list that shrank between one render and the next came
//! to report an empty range.
//!
//! [`ListViewport`]: crate::listview::ListViewport
//! [`ListViewport::visible_range`]: crate::listview::ListViewport::visible_range

/// The half-open row range `start .. start + count` that fits in the space
/// available, given the offset the caller asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rows {
    /// Index of the first visible row. Never past the end of a non-empty list.
    pub start: usize,
    /// How many rows fit. Zero when the list is empty or the space is too small
    /// for a whole row.
    pub count: usize,
}

impl Rows {
    /// The empty range — nothing fits, or there is nothing to fit.
    pub const NONE: Self = Self { start: 0, count: 0 };

    /// One past the last visible row, for slicing: `list.get(r.start..r.end())`.
    #[must_use]
    pub const fn end(self) -> usize {
        self.start.saturating_add(self.count)
    }

    /// Whether any row is visible.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.count == 0
    }
}

/// How many whole rows of height `row_h` fit into `avail_h` pixels.
///
/// Returns 0 for a non-finite or non-positive `row_h` rather than dividing by
/// it — a caller that computes its row height from a font size could hand us a
/// zero, and "no rows fit" is a drawable answer where an infinite count is not.
#[must_use]
pub fn capacity(row_h: f32, avail_h: f32) -> usize {
    if !row_h.is_finite() || row_h <= 0.0 || !avail_h.is_finite() || avail_h < row_h {
        return 0;
    }
    // Both operands are finite and positive and the quotient is at least 1, so
    // the floor is a non-negative real; the saturating cast pins any absurd
    // magnitude to `usize::MAX` rather than wrapping.
    let fit = (avail_h / row_h).floor();
    fit as usize
}

/// The visible slice of a `total`-row list drawn at `row_h` per row into
/// `avail_h` pixels, when the caller has scrolled to `offset`.
///
/// See the module docs for the three policies this encodes.
#[must_use]
pub fn visible(total: usize, row_h: f32, avail_h: f32, offset: usize) -> Rows {
    visible_count(total, capacity(row_h, avail_h), offset)
}

/// The visible slice of a `total`-row list when exactly `capacity` rows fit.
///
/// The row-count form, for panels that carry an explicit "how many rows I show"
/// setting rather than deriving one from a pixel height. [`visible`] is this
/// function with the capacity computed from a height.
#[must_use]
pub fn visible_count(total: usize, capacity: usize, offset: usize) -> Rows {
    if capacity == 0 || total == 0 {
        return Rows::NONE;
    }
    // Clamp rather than index: an offset left over from a longer list scrolls
    // up to show the last full page instead of rendering nothing.
    let max_start = total.saturating_sub(capacity);
    let start = offset.min(max_start);
    let count = capacity.min(total.saturating_sub(start));
    Rows { start, count }
}

/// The visible slice of a list whose rows are *not* all the same height.
///
/// `heights[i]` is the vertical space row `i` needs. Used by panels that
/// interleave section headings with entries, where treating a heading as a row
/// of its own is what keeps a scrolled list from starting mid-section with no
/// label above it.
///
/// Same three policies as [`visible`]: a row that does not wholly fit is not
/// shown, an offset past the last full page is pulled back to it, and the
/// caller's offset is not mutated. A row whose height is negative or
/// non-finite is treated as not fitting, which stops a `NaN` from silently
/// making every subsequent comparison false and admitting the whole list.
#[must_use]
pub fn visible_variable(heights: &[f32], avail_h: f32, offset: usize) -> Rows {
    if heights.is_empty() || !avail_h.is_finite() || avail_h <= 0.0 {
        return Rows::NONE;
    }

    // Walk backwards to find where the last full page begins. Doing it from the
    // end is what makes "scrolled past the end" show a full page rather than a
    // ragged tail.
    let mut used = 0.0_f32;
    let mut max_start = heights.len();
    for (i, h) in heights.iter().enumerate().rev() {
        if !h.is_finite() || *h < 0.0 || used + h > avail_h {
            break;
        }
        used += h;
        max_start = i;
    }

    let start = offset.min(max_start);
    let mut used = 0.0_f32;
    let mut count = 0_usize;
    for h in heights.get(start..).unwrap_or_default() {
        if !h.is_finite() || *h < 0.0 || used + h > avail_h {
            break;
        }
        used += h;
        count = count.saturating_add(1);
    }

    if count == 0 {
        return Rows::NONE;
    }
    Rows { start, count }
}

/// Move a row index by a signed number of rows, stopping at the top.
///
/// The bottom is deliberately *not* clamped here: how far a list can scroll
/// depends on how tall the panel it is drawn into turns out to be, which the
/// key handler that calls this does not know. An offset past the end is not an
/// error — [`visible`] and [`visible_count`] show the last page for one — so
/// the only end that needs guarding is zero, where an unsigned index would
/// otherwise wrap to the far end of the list.
///
/// Written once here because the obvious spelling has a bug in it:
/// `offset - (-delta) as usize` overflows for `isize::MIN`, whose negation is
/// not representable. [`isize::unsigned_abs`] is the operation that does not.
///
/// ```
/// use guitk::scroll_window::shift;
/// assert_eq!(shift(10, 3), 13);
/// assert_eq!(shift(10, -3), 7);
/// assert_eq!(shift(2, -10), 0, "scrolling up past the top stays at the top");
/// assert_eq!(shift(0, isize::MIN), 0, "and does not wrap");
/// ```
#[must_use]
pub fn shift(offset: usize, delta: isize) -> usize {
    if delta >= 0 {
        offset.saturating_add(delta.unsigned_abs())
    } else {
        offset.saturating_sub(delta.unsigned_abs())
    }
}

#[cfg(test)]
mod tests {
    use super::{Rows, capacity, shift, visible, visible_count, visible_variable};

    /// The top is the only end `shift` guards, and it guards it for every
    /// negative delta including the one whose negation does not exist.
    #[test]
    fn scrolling_up_past_the_top_stays_at_the_top() {
        for delta in [-1, -2, -1000, isize::MIN + 1, isize::MIN] {
            assert_eq!(shift(0, delta), 0, "delta {delta}");
            assert_eq!(shift(3, delta).min(3), shift(3, delta), "delta {delta}");
        }
        assert_eq!(shift(3, -2), 1);
    }

    /// The bottom is not `shift`'s to guard, and the windowing functions are
    /// the reason it does not need to be: an offset past the end is a
    /// last-page request, not an error, so a key handler that cannot know the
    /// panel height is free to run the number up.
    #[test]
    fn an_offset_run_past_the_end_is_still_a_last_page() {
        let far = shift(0, isize::MAX);
        assert!(far > 100);
        assert_eq!(visible_count(10, 4, far), Rows { start: 6, count: 4 });
        assert_eq!(visible(10, 20.0, 80.0, far), Rows { start: 6, count: 4 });
    }

    #[test]
    fn a_list_that_fits_is_shown_whole() {
        let r = visible(4, 20.0, 100.0, 0);
        assert_eq!(r, Rows { start: 0, count: 4 });
        assert_eq!(r.end(), 4);
    }

    #[test]
    fn a_list_longer_than_the_space_is_cut_at_the_last_whole_row() {
        // 100px of room at 26px a row is three whole rows and a 22px remainder;
        // the remainder must not be drawn, or it crosses the bottom edge.
        let r = visible(50, 26.0, 100.0, 0);
        assert_eq!(r, Rows { start: 0, count: 3 });
        assert!(
            (r.count as f32) * 26.0 <= 100.0,
            "the visible rows must fit inside the space given"
        );
    }

    #[test]
    fn scrolling_by_one_moves_the_window_by_exactly_one() {
        // The property that fails if an offset is accepted but ignored.
        let mut last = visible(50, 20.0, 100.0, 0);
        for offset in 1..=10 {
            let r = visible(50, 20.0, 100.0, offset);
            assert_eq!(
                r.start,
                last.start + 1,
                "offset {offset} should advance the window by one row"
            );
            assert_eq!(r.count, 5, "a full page stays full while scrolling");
            last = r;
        }
    }

    #[test]
    fn an_offset_past_the_end_shows_the_last_page_not_an_empty_list() {
        // 7 rows, 5 fit: the furthest meaningful offset is 2.
        for offset in [2, 3, 7, 8, 1000, usize::MAX] {
            let r = visible(7, 20.0, 100.0, offset);
            assert_eq!(
                r,
                Rows { start: 2, count: 5 },
                "offset {offset} should pin to the last full page"
            );
            assert_eq!(r.end(), 7, "the last page ends at the end of the list");
        }
    }

    #[test]
    fn a_short_list_under_a_large_offset_still_starts_at_zero() {
        // The case that used to underflow `end - start` in window_rules.rs:
        // the offset outlives the list it was taken against.
        let r = visible(2, 20.0, 100.0, 900);
        assert_eq!(r, Rows { start: 0, count: 2 });
    }

    #[test]
    fn an_empty_list_is_empty_at_any_offset() {
        for offset in [0, 1, usize::MAX] {
            assert_eq!(visible(0, 20.0, 100.0, offset), Rows::NONE);
        }
    }

    #[test]
    fn space_too_small_for_one_row_shows_none_rather_than_a_clipped_row() {
        for avail in [19.9_f32, 1.0, 0.0, -50.0] {
            let r = visible(10, 20.0, avail, 0);
            assert!(
                r.is_empty(),
                "{avail}px of room fits no whole 20px row, but got {r:?}"
            );
        }
    }

    #[test]
    fn a_degenerate_row_height_fits_nothing_rather_than_dividing_by_zero() {
        for row_h in [0.0_f32, -4.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                capacity(row_h, 500.0),
                0,
                "row height {row_h} must not produce a row count"
            );
            assert_eq!(visible(10, row_h, 500.0, 0), Rows::NONE);
        }
    }

    #[test]
    fn a_non_finite_available_height_fits_nothing() {
        for avail in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(capacity(20.0, avail), 0, "available height {avail}");
        }
    }

    #[test]
    fn variable_heights_agree_with_the_uniform_case_when_they_are_uniform() {
        // The two functions must not disagree, or a panel's behaviour would
        // depend on which one its author happened to reach for.
        let heights = [26.0_f32; 12];
        for avail in [0.0_f32, 25.0, 26.0, 100.0, 312.0, 1000.0] {
            for offset in [0_usize, 1, 4, 11, 12, 99] {
                assert_eq!(
                    visible_variable(&heights, avail, offset),
                    visible(heights.len(), 26.0, avail, offset),
                    "avail {avail}, offset {offset}"
                );
            }
        }
    }

    #[test]
    fn variable_rows_are_cut_where_the_next_one_would_not_fit() {
        // 30 + 48 + 48 = 126 fits in 130; the next 48 does not.
        let heights = [30.0, 48.0, 48.0, 48.0, 30.0, 48.0];
        let r = visible_variable(&heights, 130.0, 0);
        assert_eq!(r, Rows { start: 0, count: 3 });
        let used: f32 = heights.iter().take(r.count).sum();
        assert!(used <= 130.0, "{used} exceeds the 130px available");
    }

    #[test]
    fn a_variable_offset_past_the_end_shows_the_last_full_page() {
        let heights = [30.0, 48.0, 48.0, 48.0, 30.0, 48.0];
        // From the end: 48 + 30 + 48 = 126 fits in 130, adding 48 would not.
        for offset in [3, 4, 6, 100, usize::MAX] {
            let r = visible_variable(&heights, 130.0, offset);
            assert_eq!(r, Rows { start: 3, count: 3 }, "offset {offset}");
            assert_eq!(r.end(), heights.len(), "the last page reaches the end");
        }
    }

    #[test]
    fn a_variable_list_too_tall_for_even_one_row_shows_none() {
        assert_eq!(visible_variable(&[80.0, 20.0], 40.0, 0), Rows::NONE);
        assert_eq!(visible_variable(&[], 400.0, 0), Rows::NONE);
    }

    #[test]
    fn a_non_finite_variable_row_height_stops_the_window_rather_than_poisoning_it() {
        // Without the explicit guard, `used + NaN > avail` is false, so the row
        // would be accepted and every later comparison would be false too —
        // admitting the entire list into a panel that fits three rows.
        for bad in [f32::NAN, f32::INFINITY, -10.0] {
            let heights = [20.0, 20.0, bad, 20.0, 20.0];
            let r = visible_variable(&heights, 1000.0, 0);
            assert_eq!(r, Rows { start: 0, count: 2 }, "bad height {bad}");
        }
    }

    #[test]
    fn the_variable_range_is_always_a_valid_slice() {
        let heights: Vec<f32> = (0..25)
            .map(|i| if i % 3 == 0 { 30.0 } else { 48.0 })
            .collect();
        for len in [0_usize, 1, 2, 24, 25] {
            let hs = heights.get(..len).unwrap_or_default();
            for offset in [0_usize, 1, 7, 24, 25, 26, usize::MAX] {
                for avail in [0.0_f32, 29.0, 30.0, 200.0, 1e9] {
                    let r = visible_variable(hs, avail, offset);
                    assert!(r.end() <= len, "{r:?} runs past a list of {len}");
                    assert!(
                        hs.get(r.start..r.end()).is_some(),
                        "{r:?} invalid for {len}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_visible_range_is_always_a_valid_slice_of_the_list() {
        // The invariant every caller relies on: `list.get(start..end())` is Some.
        let list: Vec<usize> = (0..40).collect();
        for total in [0_usize, 1, 3, 39, 40] {
            for offset in [0_usize, 1, 5, 39, 40, 41, usize::MAX] {
                for avail in [0.0_f32, 25.9, 26.0, 130.0, 1e9] {
                    let r = visible(total, 26.0, avail, offset);
                    assert!(r.end() <= total, "{r:?} runs past a list of {total}");
                    assert!(
                        list.get(r.start..r.end()).is_some(),
                        "{r:?} is not a valid slice of a {total}-row list"
                    );
                }
            }
        }
    }
}
