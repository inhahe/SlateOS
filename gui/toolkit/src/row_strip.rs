//! A vertical run of rows that are not all the same height, laid out once.
//!
//! A list whose rows all share one height needs no help: `top + i * H` and
//! `(y - top) / H` are visibly each other's inverse, and a change to one is a
//! change you cannot help making to the other. A list whose rows *differ* —
//! a menu with separators in it, a notification list with time-group headers
//! — has no closed form, so it gets walked. And a walk is a loop somebody
//! writes out again the next time they need it.
//!
//! That is not hypothetical. Before this module existed the toolkit's two
//! menu implementations between them carried eight copies of
//!
//! ```text
//! match item {
//!     Separator => SEPARATOR_HEIGHT,
//!     _ => ITEM_HEIGHT,
//! }
//! ```
//!
//! spread over four walks each: one summing the heights for the popup's
//! total, one adding them up to place the rows on screen, one subtracting
//! them back off to answer a click, and one adding them up again to find
//! where a submenu should hang. Four walks of one list is four chances for
//! three of them to be right. The same shape, in five applications, is what
//! `scripts/reintro-row-hit-tests.py` exists to keep from coming back: a
//! renderer and a hit test that agree only by coincidence eventually stop
//! agreeing, and the symptom is a click landing on the row above the one the
//! user is looking at.
//!
//! A [`RowStrip`] is that walk, done once. You hand it the top of the run and
//! the height of each row; it hands back where every row is, and answers which
//! row — if any — is at a given `y`. The renderer draws from [`RowStrip::top`]
//! and the hit test asks [`RowStrip::index_at`], so the rows on screen are the
//! rows that answer.
//!
//! ```
//! use guitk::row_strip::RowStrip;
//!
//! // A menu: two items, a separator, one more item.
//! let strip = RowStrip::new(100.0, [28.0, 28.0, 7.0, 28.0]);
//!
//! assert_eq!(strip.top(2), Some(156.0));
//! assert_eq!(strip.index_at(156.0), Some(2)); // the separator's own top
//! assert_eq!(strip.index_at(163.0), Some(3)); // the row after it
//! assert_eq!(strip.index_at(99.0), None); // above the run
//! assert_eq!(strip.bottom(), 191.0);
//! ```
//!
//! What it deliberately does *not* do is decide whether a row is selectable.
//! A separator has a position and a height like anything else; whether a click
//! on one means something is the caller's rule, not the layout's.

/// Where each row of a variable-height run sits.
///
/// Construct with [`RowStrip::new`]; every other method is a read.
#[derive(Clone, Debug, PartialEq)]
pub struct RowStrip {
    /// Top of the run — also the top of row 0, and the value [`Self::bottom`]
    /// reports for an empty strip.
    origin: f32,
    /// `tops[i]` is the top of row `i`, in the same coordinates as `origin`.
    tops: Vec<f32>,
    /// `heights[i]` is the height of row `i`. Same length as `tops`.
    heights: Vec<f32>,
    /// One past the last row.
    end: f32,
}

impl RowStrip {
    /// Lay out rows of the given heights, the first starting at `origin`.
    ///
    /// A non-finite height would poison every row after it, so one is treated
    /// as zero — the row keeps its place in the list and occupies no space,
    /// which is the only reading that leaves the rows after it where they
    /// were. Negative heights are passed through untouched: they are
    /// nonsense, but silently clamping them would hide the caller's bug
    /// rather than the caller finding it, and [`Self::index_at`] never
    /// answers for a row it cannot fit a point inside.
    #[must_use]
    pub fn new(origin: f32, heights: impl IntoIterator<Item = f32>) -> Self {
        let iter = heights.into_iter();
        let (lower, _) = iter.size_hint();
        let mut tops = Vec::with_capacity(lower);
        let mut kept = Vec::with_capacity(lower);
        let mut y = origin;
        for h in iter {
            let h = if h.is_finite() { h } else { 0.0 };
            tops.push(y);
            kept.push(h);
            y += h;
        }
        Self {
            origin,
            tops,
            heights: kept,
            end: y,
        }
    }

    /// How many rows there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tops.len()
    }

    /// Whether there are no rows at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tops.is_empty()
    }

    /// Top of the run, as handed to [`Self::new`].
    #[must_use]
    pub const fn origin(&self) -> f32 {
        self.origin
    }

    /// Top of row `index`, or `None` if there is no such row.
    #[must_use]
    pub fn top(&self, index: usize) -> Option<f32> {
        self.tops.get(index).copied()
    }

    /// Height of row `index`, or `None` if there is no such row.
    #[must_use]
    pub fn height(&self, index: usize) -> Option<f32> {
        self.heights.get(index).copied()
    }

    /// One past the bottom of the last row — the run's own bottom edge.
    ///
    /// For an empty strip this is [`Self::origin`], not zero: a list with
    /// nothing in it still starts where it starts.
    #[must_use]
    pub const fn bottom(&self) -> f32 {
        self.end
    }

    /// Total height of the run, which is [`Self::bottom`] less
    /// [`Self::origin`].
    #[must_use]
    pub fn total_height(&self) -> f32 {
        self.end - self.origin
    }

    /// Every row's top, in the order laid out.
    #[must_use]
    pub fn tops(&self) -> &[f32] {
        &self.tops
    }

    /// The row containing `y`, if any.
    ///
    /// A row owns its top edge and not its bottom one, so adjacent rows never
    /// both answer for the pixel between them and a click 27.9 px into a
    /// 28-px row is on that row rather than adjacent to the next. `None` for
    /// a point above the first row, at or past [`Self::bottom`], inside a
    /// zero-height row, or non-finite — a hit test that answers for a pixel
    /// the renderer drew nothing at is how a highlight ends up sitting clear
    /// of the pointer that summoned it.
    #[must_use]
    pub fn index_at(&self, y: f32) -> Option<usize> {
        if !y.is_finite() {
            return None;
        }
        self.tops
            .iter()
            .zip(&self.heights)
            .position(|(&top, &h)| y >= top && y < top + h)
    }
}

#[cfg(test)]
// A test module's job is to fail loudly the moment the code under test is
// wrong, so the lints that forbid exactly that in production code are off.
// `float_cmp` in particular: these compare a coordinate against the literal
// the strip was built from, and exact equality is the assertion meant.
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]
mod tests {
    use super::RowStrip;

    #[test]
    fn an_empty_strip_starts_and_ends_where_it_was_put() {
        let strip = RowStrip::new(40.0, []);
        assert!(strip.is_empty());
        assert_eq!(strip.len(), 0);
        assert_eq!(strip.origin(), 40.0);
        assert_eq!(strip.bottom(), 40.0);
        assert_eq!(strip.total_height(), 0.0);
        assert_eq!(strip.index_at(40.0), None);
        assert_eq!(strip.top(0), None);
    }

    #[test]
    fn every_point_of_every_row_names_that_row() {
        let heights = [28.0, 7.0, 28.0, 28.0, 7.0, 13.5];
        let strip = RowStrip::new(100.0, heights);
        for (idx, &h) in heights.iter().enumerate() {
            let top = strip.top(idx).unwrap();
            assert_eq!(strip.height(idx), Some(h));
            // Sweep the row rather than probing its middle: an off-by-one in
            // a walk shows up at the edges and nowhere else.
            for step in 0..16 {
                let probe = top + (step as f32) * h / 16.0;
                assert_eq!(
                    strip.index_at(probe),
                    Some(idx),
                    "row {idx} spans {top}..{} but does not own {probe}",
                    top + h
                );
            }
        }
    }

    #[test]
    fn a_row_owns_its_top_edge_and_not_its_bottom() {
        let strip = RowStrip::new(0.0, [10.0, 10.0, 10.0]);
        assert_eq!(strip.index_at(0.0), Some(0));
        assert_eq!(strip.index_at(9.999), Some(0));
        assert_eq!(strip.index_at(10.0), Some(1));
        assert_eq!(strip.index_at(20.0), Some(2));
        // The bottom edge belongs to the list below, not to the last row.
        assert_eq!(strip.index_at(30.0), None);
        assert_eq!(strip.bottom(), 30.0);
    }

    #[test]
    fn nothing_outside_the_run_names_a_row() {
        let strip = RowStrip::new(100.0, [28.0, 28.0]);
        assert_eq!(strip.index_at(99.999), None);
        assert_eq!(strip.index_at(156.0), None);
        assert_eq!(strip.index_at(-1.0), None);
        assert_eq!(strip.index_at(f32::NAN), None);
        assert_eq!(strip.index_at(f32::INFINITY), None);
        assert_eq!(strip.index_at(f32::NEG_INFINITY), None);
    }

    #[test]
    fn a_zero_height_row_keeps_its_place_but_owns_no_pixel() {
        let strip = RowStrip::new(0.0, [10.0, 0.0, 10.0]);
        assert_eq!(strip.tops(), [0.0, 10.0, 10.0]);
        assert_eq!(
            strip.index_at(10.0),
            Some(2),
            "the zero-height row is skipped"
        );
        assert_eq!(strip.total_height(), 20.0);
    }

    #[test]
    fn a_non_finite_height_is_taken_as_nothing_rather_than_poisoning_the_rest() {
        // The alternative is every row after it landing at NaN, which turns a
        // single bad height into a list that answers for no click at all.
        let strip = RowStrip::new(0.0, [10.0, f32::NAN, 10.0, f32::INFINITY, 10.0]);
        assert_eq!(strip.tops(), [0.0, 10.0, 10.0, 20.0, 20.0]);
        assert_eq!(strip.bottom(), 30.0);
        assert_eq!(strip.index_at(15.0), Some(2));
        assert_eq!(strip.index_at(25.0), Some(4));
    }

    #[test]
    fn the_tops_are_the_running_sum_of_the_heights() {
        // The property the whole type exists to hold: `top(i)` is where a
        // renderer walking the heights would have put row `i`, and
        // `index_at` is that walk inverted.
        let heights = [28.0, 7.0, 28.0, 28.0, 7.0, 28.0, 28.0];
        let strip = RowStrip::new(64.0, heights);
        let mut walked = 64.0;
        for (idx, &h) in heights.iter().enumerate() {
            assert_eq!(strip.top(idx), Some(walked));
            assert_eq!(strip.index_at(walked), Some(idx));
            walked += h;
        }
        assert_eq!(strip.bottom(), walked);
        assert_eq!(strip.total_height(), walked - 64.0);
    }
}
