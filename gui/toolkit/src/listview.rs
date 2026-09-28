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
//! Like [`crate::step`], the methods take the list's *length* rather than the
//! list. A viewport that cached the length would be one more thing to keep in
//! step, and the lists this serves are recomputed on demand — a filtered
//! clipboard history, a search result — so the length it cached would routinely
//! be the wrong one.
//!
//! # The keys
//!
//! [`ListKey`] is how every list reads the keyboard: the arrows a row at a
//! time, Page Up and Page Down a windowful, Home and End (with or without
//! Ctrl) the first and last row. The operator put Page Up, Page Down, Home,
//! End, Ctrl+Home and Ctrl+End on in every program where they mean something
//! (`design-decisions.md` §1416). Before there was one reading of them the
//! menus, the menu bar, the start menu, the login screen's users and several
//! other lists answered only the arrows, each list reading keys its own way.

use crate::event::{Key, KeyEvent};
use crate::scroll_window;
use crate::step;
use core::ops::Range;

/// A key that moves through a list, as every list reads it.
///
/// See the module's `# The keys`. [`ListKey::of`] reads one from a key press;
/// [`ListKey::target`] says where it lands in a list of a given length, for a
/// list that keeps its own selection; [`ListViewport::go`] carries it out on a
/// viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKey {
    /// Up: the row before.
    Previous,
    /// Down: the row after.
    Next,
    /// Page Up: a windowful towards the top.
    PageUp,
    /// Page Down: a windowful towards the bottom.
    PageDown,
    /// Home, or Ctrl+Home: the first row.
    First,
    /// End, or Ctrl+End: the last row.
    Last,
}

impl ListKey {
    /// The movement `key` asks of a list, if it asks one.
    ///
    /// - **Home and End, with or without Ctrl**, are the ends. A list has no
    ///   line for plain Home to be the start of, so the two spellings -- the
    ///   one a text habit reaches for and the one a document habit does --
    ///   mean the same thing here.
    /// - **Page Up and Page Down without Ctrl.** Ctrl+Page Up and Ctrl+Page
    ///   Down are how a program with tabs changes tab; a list inside one must
    ///   leave them to it.
    /// - **Nothing with Alt or Super**: those chords are shortcuts.
    /// - **Shift is let through**, for a list that extends a selection with it.
    ///
    /// A release is not a movement.
    #[must_use]
    pub fn of(key: &KeyEvent) -> Option<Self> {
        if !key.pressed || key.modifiers.alt || key.modifiers.super_key {
            return None;
        }
        let ctrl = key.modifiers.ctrl;
        match key.key {
            Key::Up => Some(Self::Previous),
            Key::Down => Some(Self::Next),
            Key::PageUp if !ctrl => Some(Self::PageUp),
            Key::PageDown if !ctrl => Some(Self::PageDown),
            Key::Home => Some(Self::First),
            Key::End => Some(Self::Last),
            _ => None,
        }
    }

    /// Where this movement lands in a list of `len` rows, from row `from` --
    /// `None` when nothing is picked -- with `page` rows to a windowful.
    ///
    /// Clamped at both ends and never wrapping, like every list in the shell:
    /// Page Down near the bottom lands on the last row, not past it or back at
    /// the top. A `from` left over from a longer list is brought into range
    /// first (as [`step::clamped_before`] does), so it steps to a real row.
    /// With nothing picked, every movement but [`Last`](Self::Last) lands on
    /// the first row. `None` only for an empty list; a `page` of 0 counts as
    /// 1.
    #[must_use]
    pub fn target(self, from: Option<usize>, len: usize, page: usize) -> Option<usize> {
        let last = len.checked_sub(1)?;
        let page = page.max(1);
        let Some(from) = from.map(|i| i.min(last)) else {
            return Some(if self == Self::Last { last } else { 0 });
        };
        Some(match self {
            Self::First => 0,
            Self::Last => last,
            Self::Previous => step::clamped_before(len, from),
            Self::Next => step::clamped_after(len, from),
            Self::PageUp => from.saturating_sub(page),
            Self::PageDown => from.saturating_add(page).min(last),
        })
    }

    /// [`target`](Self::target), for a list some of whose rows cannot be
    /// chosen -- a menu's separators and greyed-out rows: the nearest row that
    /// can be, looking first the way the key moves and then back the other
    /// way. Home looks down from the top and End up from the bottom, so both
    /// land on the first and last rows that can be chosen. `None` when no row
    /// can be.
    #[must_use]
    pub fn target_where(
        self,
        from: Option<usize>,
        len: usize,
        page: usize,
        choosable: impl Fn(usize) -> bool,
    ) -> Option<usize> {
        let target = self.target(from, len, page)?;
        let ahead = |at: usize| (at..len).find(|&i| choosable(i));
        let behind = |at: usize| (0..=at).rev().find(|&i| choosable(i));
        if matches!(self, Self::Next | Self::PageDown | Self::First) {
            ahead(target).or_else(|| behind(target))
        } else {
            behind(target).or_else(|| ahead(target))
        }
    }
}

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
/// changed length) can violate the second on paper; [`Self::visible_range`]
/// re-applies the clamp against the `len` it is given, so nothing downstream
/// can observe it.
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
    ///
    /// Deliberately re-derived from `len` on every call rather than trusting
    /// [`Self::first_visible`]: a list can shrink between the call that last
    /// ran `reveal` and the render that asks what to draw, and this is
    /// the render's last chance to notice. Delegating to
    /// [`scroll_window::visible_count`] is what makes "shrank underneath us"
    /// show the *last page* rather than blank space — the same rule the
    /// stateless panels apply, from the same three lines.
    #[must_use]
    pub fn visible_range(&self, len: usize) -> Range<usize> {
        let rows = scroll_window::visible_count(len, self.height, self.first_visible);
        rows.start..rows.end()
    }

    /// Scrolls by `delta` rows **without** moving the selection.
    ///
    /// This is the wheel's door, and it is deliberately the one operation here
    /// that leaves `selected` alone. Every other method reveals the selection
    /// as a consequence; a wheel that did so could not scroll at all, because
    /// the view would snap back to the picked row on the next call. Scrolling
    /// the selection off screen is what a file manager does and what a user
    /// expects -- the picked row is still picked, and any key that moves it
    /// brings the view back with it.
    ///
    /// Clamped through [`scroll_window::shift`], so a delta past either end
    /// stops at the end rather than wrapping or saturating into a blank view.
    pub fn scroll_by(&mut self, delta: isize, len: usize) {
        let last_page = len.saturating_sub(self.height);
        self.first_visible = scroll_window::shift(self.first_visible, delta).min(last_page);
    }

    /// Scrolls so `first` is the top row, without moving the selection.
    ///
    /// The absolute companion to [`scroll_by`](Self::scroll_by), and the one a
    /// dragged scrollbar thumb needs: a drag names a position outright rather
    /// than a delta, and turning it into a delta would accumulate the rounding
    /// of every intermediate frame.
    pub fn scroll_to(&mut self, first: usize, len: usize) {
        self.first_visible = first.min(len.saturating_sub(self.height));
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

    /// Carry out a list key: the arrows, the page keys, and the ends. See
    /// [`ListKey`].
    pub fn go(&mut self, key: ListKey, len: usize) {
        match key {
            ListKey::Previous => self.select_prev(len),
            ListKey::Next => self.select_next(len),
            ListKey::PageUp => self.page_up(len),
            ListKey::PageDown => self.page_down(len),
            ListKey::First => self.select(Some(0), len),
            ListKey::Last => self.select(len.checked_sub(1), len),
        }
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

    use super::{ListKey, ListViewport};
    use crate::event::{Key, KeyEvent, Modifiers};
    use crate::scroll_window;
    use randrange::{RandomSource, SeededRng};

    fn press(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: String::new(),
        }
    }

    /// The operator's six (§1416), and the arrows, read the same by every list.
    #[test]
    fn the_list_keys_are_read_one_way() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        let cases = [
            (Key::Up, Modifiers::NONE, Some(ListKey::Previous)),
            (Key::Down, Modifiers::NONE, Some(ListKey::Next)),
            (Key::PageUp, Modifiers::NONE, Some(ListKey::PageUp)),
            (Key::PageDown, Modifiers::NONE, Some(ListKey::PageDown)),
            (Key::Home, Modifiers::NONE, Some(ListKey::First)),
            (Key::End, Modifiers::NONE, Some(ListKey::Last)),
            (Key::Home, ctrl, Some(ListKey::First)),
            (Key::End, ctrl, Some(ListKey::Last)),
            (Key::PageDown, shift, Some(ListKey::PageDown)),
            // Ctrl+Page Up/Down change tab in a program that has tabs.
            (Key::PageUp, ctrl, None),
            (Key::PageDown, ctrl, None),
            // Alt and Super chords are shortcuts.
            (Key::Home, Modifiers::alt(), None),
            (Key::End, Modifiers::super_key(), None),
            (Key::Left, Modifiers::NONE, None),
        ];
        for (key, modifiers, want) in cases {
            assert_eq!(
                ListKey::of(&press(key, modifiers)),
                want,
                "{key:?} with {modifiers:?}"
            );
        }
        let mut released = press(Key::Home, Modifiers::NONE);
        released.pressed = false;
        assert_eq!(ListKey::of(&released), None, "a release moved the list");
    }

    /// Where each key lands: clamped, never wrapping, a stale row brought into
    /// range first, nothing picked landing on the first row.
    #[test]
    fn each_list_key_lands_where_it_should() {
        use ListKey::{First, Last, Next, PageDown, PageUp, Previous};
        let len = 20;
        let page = 5;
        let cases = [
            (Previous, Some(0), 0),
            (Previous, Some(7), 6),
            (Next, Some(7), 8),
            (Next, Some(19), 19),
            (PageUp, Some(7), 2),
            (PageUp, Some(3), 0),
            (PageDown, Some(7), 12),
            (PageDown, Some(17), 19),
            (First, Some(7), 0),
            (Last, Some(7), 19),
            (First, None, 0),
            (Last, None, 19),
            (Next, None, 0),
            (PageDown, None, 0),
            (Previous, Some(50), 18),
            (PageDown, Some(50), 19),
        ];
        for (key, from, want) in cases {
            assert_eq!(
                key.target(from, len, page),
                Some(want),
                "{key:?} from {from:?}"
            );
        }
        assert_eq!(Last.target(Some(3), 0, page), None, "an empty list");
        assert_eq!(
            PageDown.target(Some(3), len, 0),
            Some(4),
            "a page of 0 is 1"
        );
    }

    /// Where some rows cannot be chosen, each key takes the nearest that can,
    /// the way it moves first: Home the first choosable row, End the last, a
    /// page that lands on a separator the next row on.
    #[test]
    fn keys_skip_the_rows_that_cannot_be_chosen() {
        use ListKey::{First, Last, PageDown, PageUp};
        // Rows 0, 1, 8 and 9 cannot be chosen.
        let choosable = |i: usize| (2..8).contains(&i);
        assert_eq!(First.target_where(Some(5), 10, 3, choosable), Some(2));
        assert_eq!(Last.target_where(Some(5), 10, 3, choosable), Some(7));
        assert_eq!(PageDown.target_where(Some(5), 10, 3, choosable), Some(7));
        assert_eq!(PageUp.target_where(Some(3), 10, 3, choosable), Some(2));
        assert_eq!(
            Last.target_where(None, 10, 3, |_| false),
            None,
            "nothing to choose"
        );
        // A page landing on a row that cannot be chosen, with rows that can on
        // both sides of it: onward first, not back.
        let gap = |i: usize| i != 5;
        assert_eq!(PageDown.target_where(Some(2), 10, 3, gap), Some(6));
        assert_eq!(PageUp.target_where(Some(8), 10, 3, gap), Some(4));
    }

    /// The viewport carries out every key and keeps the row it lands on in
    /// view.
    #[test]
    fn the_viewport_goes_where_each_key_says() {
        let len = 30;
        let mut view = ListViewport::default();
        view.set_height(10, len);
        view.go(ListKey::Last, len);
        assert_eq!(view.selected(), Some(29));
        assert!(view.visible_range(len).contains(&29));
        view.go(ListKey::PageUp, len);
        assert_eq!(view.selected(), Some(19));
        view.go(ListKey::First, len);
        assert_eq!(view.selected(), Some(0));
        assert_eq!(view.visible_range(len).start, 0);
        view.go(ListKey::PageDown, len);
        assert_eq!(view.selected(), Some(10));
        view.go(ListKey::Next, len);
        view.go(ListKey::Previous, len);
        assert_eq!(view.selected(), Some(10));
        assert_consistent(&view, len);

        let mut empty = ListViewport::default();
        empty.go(ListKey::Last, 0);
        assert_eq!(empty.selected(), None);
    }

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
        // Empty, and empty *at the top*: when no row fits, where the window
        // would have been looking is not a meaningful answer, so both this and
        // `scroll_window::visible_count` give the one canonical empty range
        // rather than an empty one at whatever index the selection reached.
        assert_eq!(view.visible_range(10), 0..0);
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
    fn a_list_that_shrinks_without_telling_the_viewport_shows_its_last_page() {
        // `reveal` applies the last-page clamp, but only the mutating methods
        // call it. A list that is recomputed each frame -- a filtered clipboard
        // history, a search result -- can shrink between the keypress that
        // moved the selection and the render that asks what to draw, with no
        // mutating call in between. `visible_range` used to clamp only to
        // `len`, so it answered `9..9`: a pane that had gone blank, with a
        // scrollbar the user could not move because nothing was writing the
        // offset either.
        let mut view = ListViewport::new(4);
        view.select(Some(9), 10);
        assert_eq!(view.visible_range(10), 6..10);
        // Three rows remain. Nothing has told the viewport.
        assert_eq!(view.visible_range(3), 0..3);
        assert_eq!(
            view.visible_range(8),
            4..8,
            "the last page of a shorter list"
        );
        // ... and the stored field is untouched, so the next mutating call
        // still sees what the user actually scrolled to.
        assert_eq!(view.first_visible(), 6);
    }

    #[test]
    fn the_two_windowing_answers_are_the_same_answer() {
        // `visible_range` delegates to `scroll_window::visible_count`, so this
        // cannot fail by drift -- it fails if someone reimplements one of them.
        for height in [0usize, 1, 3, 7] {
            for len in [0usize, 1, 2, 7, 8, 50] {
                for first in [0usize, 1, 6, 49, 50, 51, usize::MAX] {
                    let mut view = ListViewport::new(height);
                    // Reach `first_visible` without `reveal` clamping it: a
                    // long list to scroll into, then ask about a shorter one.
                    view.select(Some(first), first.saturating_add(height));
                    let rows = scroll_window::visible_count(len, height, view.first_visible());
                    assert_eq!(
                        view.visible_range(len),
                        rows.start..rows.end(),
                        "height {height}, len {len}, first {first}"
                    );
                }
            }
        }
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
