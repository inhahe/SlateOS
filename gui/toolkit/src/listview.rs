//! A list's selection and the window of it that is on screen, kept in step.
//!
//! Every scrolling list in the shell has the same two pieces of state — which
//! row is picked, and which row is drawn first — and the same rule binding
//! them: **the selected row is always visible**. Written out by hand at each
//! call site, that rule turns into a pair of `if`s per movement key, each
//! computing a scroll position from a selection index that some earlier
//! statement, but not the compiler, had established was in range. The
//! clipboard viewer's copy read
//!
//! ```text
//! self.scroll_offset = (i + 1).saturating_sub(self.max_visible - 1);
//! ```
//!
//! which panics whenever the window height is zero — a public field that
//! nothing clamped — and which never scrolled *down* to reach a selection that
//! had ended up below the window, only up to one above it.
//!
//! [`ListViewport`] holds the rule instead. Every operation that can move the
//! selection re-establishes it before returning, so there is no window in which
//! the two disagree and no bound left for a caller to prove.
//!
//! Like [`crate::cycle`], the methods take the list's *length* rather than the
//! list. A viewport that cached the length would be one more thing to keep in
//! step, and the lists this serves are recomputed on demand — a filtered
//! clipboard history, a search result — so the length it cached would routinely
//! be the wrong one.

use core::ops::Range;

/// Where a scrolling list is looking, and which of its rows is picked.
///
/// # Invariants
///
/// Maintained by every method that takes a `len`, and depended on by
/// [`Self::visible_range`]:
///
/// - the selection, if any, is below `len` and inside the visible window;
/// - the first visible row is at or below `len.saturating_sub(height)`, so the
///   window is never scrolled past the end of a list long enough to fill it.
///
/// A viewport that has not been shown a `len` yet (or whose list has since
/// grown) can violate the second on paper; [`Self::visible_range`] clamps to
/// the `len` it is given, so nothing downstream can observe it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListViewport {
    /// Index of the first row drawn.
    first_visible: usize,
    /// How many rows fit on screen. Zero is legal and means none do — a pane
    /// too short to show a list is not an error, and the type has to survive
    /// it rather than divide by it.
    height: usize,
    /// The picked row, if any.
    selected: Option<usize>,
}

impl ListViewport {
    /// A viewport `height` rows tall, scrolled to the top with nothing picked.
    #[must_use]
    pub const fn new(height: usize) -> Self {
        Self {
            first_visible: 0,
            height,
            selected: None,
        }
    }

    /// How many rows fit on screen.
    #[must_use]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Index of the first row drawn.
    #[must_use]
    pub const fn first_visible(&self) -> usize {
        self.first_visible
    }

    /// The picked row, if any.
    #[must_use]
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Scrolls back to the top and picks nothing.
    ///
    /// For when the list underneath has been replaced rather than edited — a
    /// new search query, a reopened popup — where keeping a position into the
    /// old list would be meaningless rather than merely stale.
    pub const fn reset(&mut self) {
        self.first_visible = 0;
        self.selected = None;
    }

    /// Resizes the window, scrolling if that would have hidden the selection.
    pub fn set_height(&mut self, height: usize, len: usize) {
        self.height = height;
        self.reveal(len);
    }

    /// The rows on screen, as indices into a list of `len` rows.
    ///
    /// Always a valid range for a list that long, so a caller can slice with it
    /// or zip it against the rows it drew.
    #[must_use]
    pub fn visible_range(&self, len: usize) -> Range<usize> {
        let start = self.first_visible.min(len);
        let end = start.saturating_add(self.height).min(len);
        start..end
    }

    /// Picks `index`, clamped into the list, and scrolls to show it.
    ///
    /// `None` picks nothing but leaves the scroll position alone, which is what
    /// a list wants when the selection is cleared without the view moving.
    pub fn select(&mut self, index: Option<usize>, len: usize) {
        self.selected = index;
        self.reveal(len);
    }

    /// Moves the selection one row towards the top, or picks the first row if
    /// nothing was picked. Stops at the top rather than wrapping.
    pub fn select_prev(&mut self, len: usize) {
        self.selected = Some(match self.selected {
            Some(index) => index.saturating_sub(1),
            None => 0,
        });
        self.reveal(len);
    }

    /// Moves the selection one row towards the bottom, or picks the first row
    /// if nothing was picked. Stops at the last row rather than wrapping.
    pub fn select_next(&mut self, len: usize) {
        self.selected = Some(match self.selected {
            Some(index) => index.saturating_add(1),
            None => 0,
        });
        self.reveal(len);
    }

    /// Scrolls one windowful towards the top, carrying the selection with it.
    pub fn page_up(&mut self, len: usize) {
        let step = self.height.max(1);
        self.selected = Some(match self.selected {
            Some(index) => index.saturating_sub(step),
            None => 0,
        });
        self.reveal(len);
    }

    /// Scrolls one windowful towards the bottom, carrying the selection.
    pub fn page_down(&mut self, len: usize) {
        let step = self.height.max(1);
        self.selected = Some(match self.selected {
            Some(index) => index.saturating_add(step),
            None => 0,
        });
        self.reveal(len);
    }

    /// Restores both invariants: clamps the selection into the list, scrolls
    /// the window until it contains the selection, and stops the window
    /// hanging off the end of the list.
    ///
    /// This is the whole point of the type, and it is the only place the two
    /// fields are allowed to be written together.
    fn reveal(&mut self, len: usize) {
        // An empty list has no row to select and no position to scroll to.
        let Some(last) = len.checked_sub(1) else {
            self.first_visible = 0;
            self.selected = None;
            return;
        };

        if let Some(selected) = self.selected {
            let selected = selected.min(last);
            self.selected = Some(selected);

            // Above the window: pull the top down to the selection.
            self.first_visible = self.first_visible.min(selected);

            // Below the window: push the top up until the selection is the
            // last row drawn. A window `height` rows tall ending at `selected`
            // starts at `selected + 1 - height`; `checked_sub` returning
            // `None` means the window is already tall enough to reach row 0
            // from there, so there is nothing to push.
            if let Some(top) = selected
                .checked_add(1)
                .and_then(|past_end| past_end.checked_sub(self.height))
            {
                self.first_visible = self.first_visible.max(top);
            }
        }

        // Never leave blank rows at the bottom of a list long enough to fill
        // the window. This can only lower `first_visible`, and never below the
        // selection: a selection at `last` needs a top of at most
        // `len - height`, which is exactly this bound.
        self.first_visible = self.first_visible.min(len.saturating_sub(self.height));
    }
}

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::ListViewport;
    use randrange::{RandomSource, SeededRng};

    /// Both invariants, checked after every operation in every test below.
    fn assert_consistent(view: &ListViewport, len: usize) {
        let visible = view.visible_range(len);
        assert!(visible.end <= len, "window runs past the end of the list");
        assert!(visible.start <= visible.end, "window is inside out");
        if let Some(selected) = view.selected() {
            assert!(
                selected < len,
                "selection {selected} is outside a {len}-row list"
            );
            if view.height() > 0 {
                assert!(
                    visible.contains(&selected),
                    "selection {selected} is outside the visible {visible:?}"
                );
            }
        }
        if len >= view.height() {
            assert_eq!(
                visible.len(),
                view.height(),
                "a list long enough to fill the window left blank rows"
            );
        }
    }

    #[test]
    fn a_fresh_viewport_shows_the_top_and_picks_nothing() {
        let view = ListViewport::new(8);
        assert_eq!(view.selected(), None);
        assert_eq!(view.first_visible(), 0);
        assert_eq!(view.visible_range(100), 0..8);
        assert_eq!(view.visible_range(3), 0..3);
        assert_eq!(view.visible_range(0), 0..0);
    }

    #[test]
    fn moving_down_past_the_bottom_of_the_window_scrolls_it() {
        let mut view = ListViewport::new(4);
        for expected in 0..4 {
            view.select_next(10);
            assert_eq!(view.selected(), Some(expected));
            assert_eq!(view.visible_range(10), 0..4, "no scroll needed yet");
            assert_consistent(&view, 10);
        }
        // Row 4 is one past the bottom, so the window steps down by one.
        view.select_next(10);
        assert_eq!(view.selected(), Some(4));
        assert_eq!(view.visible_range(10), 1..5);
        assert_consistent(&view, 10);
    }

    #[test]
    fn moving_up_past_the_top_of_the_window_scrolls_it() {
        let mut view = ListViewport::new(4);
        view.select(Some(9), 10);
        assert_eq!(view.visible_range(10), 6..10);
        for expected in (6..=8).rev() {
            view.select_prev(10);
            assert_eq!(view.selected(), Some(expected));
            assert_eq!(view.visible_range(10), 6..10, "no scroll needed yet");
        }
        view.select_prev(10);
        assert_eq!(view.selected(), Some(5));
        assert_eq!(view.visible_range(10), 5..9);
        assert_consistent(&view, 10);
    }

    #[test]
    fn the_selection_stops_at_both_ends_rather_than_wrapping() {
        let mut view = ListViewport::new(4);
        for _ in 0..20 {
            view.select_next(10);
        }
        assert_eq!(view.selected(), Some(9));
        for _ in 0..20 {
            view.select_prev(10);
        }
        assert_eq!(view.selected(), Some(0));
        assert_eq!(view.visible_range(10), 0..4);
        assert_consistent(&view, 10);
    }

    #[test]
    fn an_empty_list_has_nothing_to_select() {
        let mut view = ListViewport::new(4);
        view.select(Some(3), 8);
        assert_eq!(view.selected(), Some(3));
        // The list emptied underneath it.
        view.select_next(0);
        assert_eq!(view.selected(), None);
        assert_eq!(view.first_visible(), 0);
        assert_eq!(view.visible_range(0), 0..0);
        view.select_prev(0);
        assert_eq!(view.selected(), None);
        assert_consistent(&view, 0);
    }

    #[test]
    fn a_shrinking_list_pulls_the_selection_and_the_window_back() {
        let mut view = ListViewport::new(4);
        view.select(Some(99), 100);
        assert_eq!(view.visible_range(100), 96..100);
        // A search narrows the list to three rows.
        view.select(view.selected(), 3);
        assert_eq!(view.selected(), Some(2));
        assert_eq!(view.visible_range(3), 0..3);
        assert_consistent(&view, 3);
    }

    #[test]
    fn a_zero_height_window_is_survivable() {
        // The clipboard viewer's `max_visible` was a public field with nothing
        // clamping it, and its scroll formula subtracted one from it.
        let mut view = ListViewport::new(0);
        view.select_next(10);
        view.select_next(10);
        view.select_prev(10);
        view.page_down(10);
        view.page_up(10);
        assert_eq!(
            view.visible_range(10),
            view.first_visible()..view.first_visible()
        );
        assert_consistent(&view, 10);
        // Growing the window brings the selection back into view.
        view.set_height(3, 10);
        let visible = view.visible_range(10);
        assert!(visible.contains(&view.selected().expect("something is picked")));
        assert_consistent(&view, 10);
    }

    #[test]
    fn shrinking_the_window_keeps_the_selection_on_screen() {
        let mut view = ListViewport::new(10);
        view.select(Some(7), 20);
        assert_eq!(view.visible_range(20), 0..10);
        view.set_height(3, 20);
        assert_eq!(view.visible_range(20), 5..8);
        assert_consistent(&view, 20);
    }

    #[test]
    fn a_window_taller_than_the_list_shows_all_of_it_from_the_top() {
        let mut view = ListViewport::new(20);
        view.select(Some(2), 5);
        assert_eq!(view.visible_range(5), 0..5);
        assert_eq!(view.first_visible(), 0);
        assert_consistent(&view, 5);
    }

    #[test]
    fn paging_moves_a_windowful_at_a_time() {
        let mut view = ListViewport::new(5);
        view.select(Some(0), 100);
        view.page_down(100);
        assert_eq!(view.selected(), Some(5));
        view.page_down(100);
        assert_eq!(view.selected(), Some(10));
        view.page_up(100);
        assert_eq!(view.selected(), Some(5));
        assert_consistent(&view, 100);
        // Paging in a zero-height window still moves, one row at a time,
        // rather than standing still forever.
        let mut flat = ListViewport::new(0);
        flat.select(Some(4), 100);
        flat.page_down(100);
        assert_eq!(flat.selected(), Some(5));
    }

    #[test]
    fn resetting_forgets_the_position_entirely() {
        let mut view = ListViewport::new(4);
        view.select(Some(50), 100);
        view.reset();
        assert_eq!(view.selected(), None);
        assert_eq!(view.first_visible(), 0);
        assert_consistent(&view, 100);
    }

    #[test]
    fn extreme_indices_do_not_overflow() {
        let mut view = ListViewport::new(usize::MAX);
        view.select(Some(usize::MAX), usize::MAX);
        assert_eq!(view.selected(), Some(usize::MAX - 1));
        view.select_next(usize::MAX);
        view.page_down(usize::MAX);
        view.page_up(usize::MAX);
        view.select_prev(usize::MAX);
        assert_consistent(&view, usize::MAX);

        let mut tall = ListViewport::new(4);
        tall.select(Some(usize::MAX), usize::MAX);
        assert_eq!(tall.visible_range(usize::MAX).end, usize::MAX);
        assert_consistent(&tall, usize::MAX);
    }

    #[test]
    fn every_sequence_of_moves_leaves_the_selection_visible() {
        // Walks a deterministic pseudo-random mix of operations against lists
        // and window heights of every awkward size, checking both invariants
        // after each step.
        //
        // The mix used to come from an LCG inlined right here -- the same
        // multiplier and increment that had been copy-pasted into sixteen app
        // crates, written out a seventeenth time in the toolkit that could
        // have offered them the shared one. It drew with `(seed >> 33) % 6`,
        // which is not the broken reduction (the shift discards the counter
        // bits first), so this walk was not actually biased. It is replaced
        // anyway: a test that generates its own randomness by hand is a test
        // whose coverage nobody has checked, and `randrange` is a direct
        // dependency of this crate.
        for height in [0usize, 1, 2, 3, 7] {
            for len in [0usize, 1, 2, 3, 6, 7, 8, 50] {
                let mut view = ListViewport::new(height);
                let mut rng = SeededRng::new(0x2545_F491_4F6C_DD1D);
                for step in 0..200u32 {
                    match rng.below(6) {
                        0 => view.select_next(len),
                        1 => view.select_prev(len),
                        2 => view.page_down(len),
                        3 => view.page_up(len),
                        4 => view.select(Some((step as usize).wrapping_mul(7)), len),
                        _ => view.select(None, len),
                    }
                    assert_consistent(&view, len);
                }
            }
        }
    }
}
