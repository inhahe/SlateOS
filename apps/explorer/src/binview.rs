//! The recycle bin, shown in the file pane in place of a folder.
//!
//! Opened by `explorer --recycle-bin` -- what the desktop's Recycle Bin icon
//! runs -- and by the sidebar's "Recycle Bin". One row per recycled item,
//! under its original name and the folder it was deleted from, with when it
//! was deleted and how big it is; and Restore, Delete permanently and Empty
//! recycle bin. That is everything lane C's request asked for
//! (`requests/c-e-the-recycle-bin-icon-has-nowhere-to-open.md`): until this,
//! the file manager put things in the bin and nothing could show what was in
//! it, so a file deleted more than one Undo ago could not be found again.
//!
//! # Why not a folder
//!
//! The bin keeps each item as `~/.recycle/<id>/data`, beside a `meta.txt`
//! saying where it came from. Listing that folder would show folders named by
//! ids -- `notes.txt_1a2b…` -- each holding a file called `data`, which is the
//! one listing nobody can find a file in; and a Paste into it would put a
//! file where the bin cannot see it. So this is a view of what
//! [`RecycleBin::list`] answers, and its rows are entries, not paths.
//!
//! # What lives here
//!
//! The view's state -- the entries, which are chosen, the keyboard's row and
//! the scroll -- its layout, its drawing and the words each row shows. It does
//! no I/O but `RecycleBin::list`. Carrying out Restore and Delete permanently
//! is `ExplorerState`'s, because those report through the window's status line
//! and dialogs like every other file operation, and ask through its modal.

use std::collections::{BTreeSet, HashSet};
use std::time::SystemTime;

use appearance::Palette;
use guitk::disabled::DisabledState;
use guitk::frame::Rect;
use guitk::listview::ListViewport;
use guitk::render::{FontWeightHint, RenderTree};
use guitk::scroll_window;
use guitk::scrollbar;
use guitk::style::CornerRadii;
use guitk::theme::with_alpha;
use pathtext::ShowPath;
use recyclebin::{RecycleBin, RecycleEntry};

use crate::columns::format_datetime;

/// Height of the strip of actions across the top of the view.
pub const BAR_H: f32 = 40.0;
/// Height of the column headings.
pub const HEADING_H: f32 = 24.0;
/// Height of one row.
pub const ROW_H: f32 = 24.0;
/// Height of an action button.
const BUTTON_H: f32 = 28.0;
/// The space around and between the buttons.
const GAP: f32 = 8.0;
/// Size of the text in rows and on buttons.
const TEXT: f32 = 12.0;
/// Size of the column headings.
const HEADING_TEXT: f32 = 11.0;

/// The columns, left to right: heading, and share of the table's width.
///
/// "Deleted from" rather than "Folder" or "Location": the folder is where the
/// item *was*, and where Restore puts it back, which a bare "Location" leaves
/// to be guessed.
const COLUMNS: [(&str, f32); 4] = [
    ("Name", 0.30),
    ("Deleted from", 0.36),
    ("Deleted", 0.20),
    ("Size", 0.14),
];

/// An action in the view's bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinButton {
    /// Put the chosen items back where they were deleted from.
    Restore,
    /// Erase the chosen items; asks first.
    DeleteForever,
    /// Erase everything in the bin; asks first.
    Empty,
}

impl BinButton {
    /// Every button, in the order they are drawn -- walked by a test that
    /// checks each one can be pressed.
    #[cfg(test)]
    pub const ALL: [Self; 3] = [Self::Restore, Self::DeleteForever, Self::Empty];

    /// The words on its face.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Restore => "Restore",
            Self::DeleteForever => "Delete permanently",
            Self::Empty => "Empty recycle bin",
        }
    }

    /// Whether pressing it erases something for good.
    const fn erases(self) -> bool {
        matches!(self, Self::DeleteForever | Self::Empty)
    }
}

/// What a point in the view is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinHit {
    /// One of the actions.
    Button(BinButton),
    /// The row showing entry `index`.
    Row(usize),
    /// Anywhere else in the view: the headings, the gaps, below the last row.
    Blank,
}

/// Where everything in the view is drawn, for a pane.
///
/// The one place it is worked out: the painter and the hit test both ask
/// here, so a row cannot be drawn in one place and clicked in another.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BinLayout {
    /// The actions, left to right: Restore, Delete permanently, Empty.
    pub buttons: [(BinButton, Rect); 3],
    /// The column headings.
    pub heading: Rect,
    /// Where the rows go, scrollbar included.
    pub rows: Rect,
}

impl BinLayout {
    /// The layout of the view filling `pane`.
    #[must_use]
    pub fn for_pane(pane: Rect) -> Self {
        let y = pane.y + (BAR_H - BUTTON_H) / 2.0;
        let width = |button: BinButton| {
            guitk::text::measure(button.label(), TEXT, FontWeightHint::Regular) + 3.0 * GAP
        };
        let restore = Rect::new(pane.x + GAP, y, width(BinButton::Restore), BUTTON_H);
        let forever = Rect::new(
            restore.x + restore.w + GAP,
            y,
            width(BinButton::DeleteForever),
            BUTTON_H,
        );
        // Empty at the far end, apart from the two that act on the choice:
        // it acts on everything, and should not be where a hand reaching for
        // "Delete permanently" lands. On a pane too narrow to hold the gap it
        // follows the others rather than overlapping them.
        let empty_w = width(BinButton::Empty);
        let empty_x = (pane.x + pane.w - GAP - empty_w).max(forever.x + forever.w + 3.0 * GAP);
        let empty = Rect::new(empty_x, y, empty_w, BUTTON_H);
        let heading = Rect::new(pane.x, pane.y + BAR_H, pane.w, HEADING_H);
        let top = pane.y + BAR_H + HEADING_H;
        let rows = Rect::new(pane.x, top, pane.w, (pane.y + pane.h - top).max(0.0));
        Self {
            buttons: [
                (BinButton::Restore, restore),
                (BinButton::DeleteForever, forever),
                (BinButton::Empty, empty),
            ],
            heading,
            rows,
        }
    }

    /// How many rows fit.
    #[must_use]
    pub fn capacity(&self) -> usize {
        scroll_window::capacity(ROW_H, self.rows.h)
    }

    /// The table's width: the rows' less the scrollbar's, which is kept free
    /// whether or not a bar is drawn, so the columns do not shift when the
    /// bin grows past a screenful.
    fn table_w(&self) -> f32 {
        (self.rows.w - scrollbar::WIDTH).max(0.0)
    }

    /// Each column's left edge and width.
    fn columns(&self) -> [(f32, f32); 4] {
        let table_w = self.table_w();
        let mut x = self.rows.x;
        COLUMNS.map(|(_, share)| {
            let w = table_w * share;
            let column = (x, w);
            x += w;
            column
        })
    }
}

/// The recycle bin as the file pane shows it.
#[derive(Debug)]
pub struct BinView {
    /// What [`RecycleBin::list`] answered: newest first, a damaged entry last.
    entries: Vec<RecycleEntry>,
    /// The chosen entries, by id.
    ///
    /// Ids and not row numbers, because every action reloads the list and a
    /// row number is only good for the list it was taken from: after a
    /// restore, "row 3" is a different file. An id names one entry for as
    /// long as that entry exists, and when it stops existing it drops out of
    /// the choice at the next reload rather than naming its neighbour.
    chosen: BTreeSet<String>,
    /// The row the keyboard is on, and the scroll that keeps it in sight.
    viewport: ListViewport,
    /// Where a Shift+arrow run starts from.
    anchor: Option<usize>,
    /// Why the bin could not be read, when it could not.
    error: Option<String>,
    /// The status line's summary, kept current by everything that changes
    /// what it describes.
    summary: String,
}

impl BinView {
    /// Read `bin` and show it, scrolled to the top, with nothing chosen.
    #[must_use]
    pub fn open(bin: &RecycleBin) -> Self {
        let mut view = Self {
            entries: Vec::new(),
            chosen: BTreeSet::new(),
            viewport: ListViewport::new(0),
            anchor: None,
            error: None,
            summary: String::new(),
        };
        view.reload(bin);
        view
    }

    /// Read the bin again.
    ///
    /// The choice keeps the entries that are still there; the keyboard's row
    /// stays where it was, so after a restore it rests on the item that slid
    /// up into the restored one's place.
    pub fn reload(&mut self, bin: &RecycleBin) {
        match bin.list() {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(e) => {
                self.entries.clear();
                self.error = Some(format!("The recycle bin could not be read: {e}"));
            }
        }
        let present: HashSet<&str> = self.entries.iter().map(|e| e.id.as_str()).collect();
        self.chosen.retain(|id| present.contains(id.as_str()));
        let len = self.entries.len();
        self.anchor = self.anchor.filter(|&a| a < len);
        let cursor = self.viewport.selected();
        self.viewport.select(cursor, len);
        self.refresh_summary();
    }

    /// Every entry, in the order drawn.
    #[must_use]
    pub fn entries(&self) -> &[RecycleEntry] {
        &self.entries
    }

    /// The chosen entries, in the order drawn.
    #[must_use]
    pub fn chosen(&self) -> Vec<&RecycleEntry> {
        self.entries
            .iter()
            .filter(|e| self.chosen.contains(&e.id))
            .collect()
    }

    /// Whether entry `index` is chosen.
    #[must_use]
    pub fn is_chosen(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|e| self.chosen.contains(&e.id))
    }

    /// The row the keyboard is on.
    #[cfg(test)]
    #[must_use]
    pub fn cursor(&self) -> Option<usize> {
        self.viewport.selected()
    }

    /// The first row drawn.
    #[must_use]
    pub fn first_visible(&self) -> usize {
        self.viewport.first_visible()
    }

    /// The status line's description of the bin.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Make the scroll fit a pane `capacity` rows tall.
    ///
    /// Only when the height has changed: setting it also scrolls the keyboard's
    /// row back into sight, and doing that on every event would undo every
    /// turn of the wheel the moment it was made.
    pub fn fit(&mut self, capacity: usize) {
        if self.viewport.height() != capacity {
            self.viewport.set_height(capacity, self.entries.len());
        }
    }

    /// Choose entry `index` alone, and put the keyboard on it.
    pub fn choose_only(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        self.chosen.clear();
        self.chosen.insert(entry.id.clone());
        self.anchor = Some(index);
        self.viewport.select(Some(index), self.entries.len());
        self.refresh_summary();
    }

    /// Choose everything.
    pub fn choose_all(&mut self) {
        self.chosen = self.entries.iter().map(|e| e.id.clone()).collect();
        self.refresh_summary();
    }

    /// Choose nothing: a click on empty space. The keyboard stays where it
    /// was, so the next arrow moves on from there.
    pub fn clear_choice(&mut self) {
        self.chosen.clear();
        self.anchor = None;
        self.refresh_summary();
    }

    /// Move the keyboard `delta` rows. With `extend`, the choice becomes the
    /// run from where the last plain move or click left it to the new row --
    /// Shift+arrow; without, the new row alone.
    ///
    /// The first move with nothing under the keyboard lands on the first row
    /// going down and the last going up, rather than stepping from a row that
    /// is not there.
    pub fn step(&mut self, delta: isize, extend: bool) {
        let Some(last) = self.entries.len().checked_sub(1) else {
            return;
        };
        let to = match (self.viewport.selected(), delta.is_negative()) {
            (None, true) => last,
            (None, false) => 0,
            (Some(at), true) => at.saturating_sub(delta.unsigned_abs()),
            (Some(at), false) => at.saturating_add(delta.unsigned_abs()).min(last),
        };
        self.go_to(to, extend);
    }

    /// Put the keyboard on row `index` (clamped into the list), choosing it
    /// alone or, with `extend`, the run from the anchor to it.
    pub fn go_to(&mut self, index: usize, extend: bool) {
        let Some(last) = self.entries.len().checked_sub(1) else {
            return;
        };
        let index = index.min(last);
        if !extend {
            self.choose_only(index);
            return;
        }
        let anchor = *self.anchor.get_or_insert(index);
        let (low, high) = if anchor <= index {
            (anchor, index)
        } else {
            (index, anchor)
        };
        self.chosen = self
            .entries
            .iter()
            .skip(low)
            .take(high.saturating_sub(low).saturating_add(1))
            .map(|e| e.id.clone())
            .collect();
        self.viewport.select(Some(index), self.entries.len());
        self.refresh_summary();
    }

    /// Scroll by `rows` without moving the keyboard: the wheel.
    pub fn scroll_by(&mut self, rows: isize) {
        self.viewport.scroll_by(rows, self.entries.len());
    }

    /// Scroll so `first` is the top row: a dragged scrollbar.
    pub fn scroll_to(&mut self, first: usize) {
        self.viewport.scroll_to(first, self.entries.len());
    }

    /// Whether a button has anything to do, and if not, why not.
    #[must_use]
    pub fn button_state(&self, button: BinButton) -> DisabledState {
        let reason = match button {
            BinButton::Restore if self.chosen.is_empty() => Some("Choose what to put back"),
            BinButton::Restore if !self.chosen().iter().any(|e| e.is_readable()) => {
                Some("A damaged entry does not say where it came from, so it cannot be put back")
            }
            BinButton::DeleteForever if self.chosen.is_empty() => Some("Choose what to delete"),
            BinButton::Empty if self.entries.is_empty() => Some("The recycle bin is empty"),
            _ => None,
        };
        reason.map_or(DisabledState::Enabled, |reason| DisabledState::Disabled {
            reason: Some(reason.to_string()),
        })
    }

    /// What `(x, y)` is over, for the view filling `pane`; `None` outside it.
    #[must_use]
    pub fn hit(&self, pane: Rect, x: f32, y: f32) -> Option<BinHit> {
        if !pane.contains(x, y) {
            return None;
        }
        let layout = BinLayout::for_pane(pane);
        if let Some((button, _)) = layout.buttons.iter().find(|(_, r)| r.contains(x, y)) {
            return Some(BinHit::Button(*button));
        }
        if !layout.rows.contains(x, y) || x >= layout.rows.x + layout.table_w() {
            return Some(BinHit::Blank);
        }
        let rows = self.visible_rows(&layout);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a non-negative offset within the rows area, divided by a row height, is a small row count"
        )]
        let row = ((y - layout.rows.y) / ROW_H) as usize;
        Some(match rows.start.checked_add(row) {
            Some(index) if index < rows.end() => BinHit::Row(index),
            _ => BinHit::Blank,
        })
    }

    /// The rows on screen: the same calculation for the painter and for
    /// [`Self::hit`].
    fn visible_rows(&self, layout: &BinLayout) -> scroll_window::Rows {
        scroll_window::visible_count(
            self.entries.len(),
            layout.capacity(),
            self.viewport.first_visible(),
        )
    }

    /// Draw the view into `pane`.
    pub fn render(&self, tree: &mut RenderTree, palette: &Palette, pane: Rect) {
        let layout = BinLayout::for_pane(pane);
        tree.clip(pane.x, pane.y, pane.w, pane.h);
        tree.fill_rect(pane.x, pane.y, pane.w, pane.h, palette.mantle);

        // The actions.
        tree.fill_rect(pane.x, pane.y, pane.w, BAR_H, palette.crust);
        for (button, rect) in layout.buttons {
            let enabled = self.button_state(button).is_enabled();
            tree.fill_rounded_rect(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                palette.surface0,
                CornerRadii::all(4.0),
            );
            // Off looks off; an action that erases is marked in red while it
            // is live, so "Delete permanently" is not read as "Restore".
            let ink = if !enabled {
                palette.overlay0
            } else if button.erases() {
                palette.ink(palette.red)
            } else {
                palette.text
            };
            tree.text_in(
                rect.x + 1.5 * GAP,
                rect.y + (BUTTON_H - TEXT) / 2.0 - 1.0,
                rect.w - 2.0 * GAP,
                button.label(),
                ink,
                TEXT,
            );
        }

        // The headings.
        tree.fill_rect(
            layout.heading.x,
            layout.heading.y,
            layout.heading.w,
            layout.heading.h,
            palette.crust,
        );
        for ((title, _), (x, w)) in COLUMNS.iter().zip(layout.columns()) {
            tree.text_in(
                x + GAP,
                layout.heading.y + (HEADING_H - HEADING_TEXT) / 2.0,
                (w - 1.5 * GAP).max(0.0),
                title,
                palette.subtext0,
                HEADING_TEXT,
            );
        }

        // The rows, or why there are none.
        let message = match (&self.error, self.entries.is_empty()) {
            (Some(error), _) => Some(error.as_str()),
            (None, true) => Some("The recycle bin is empty."),
            (None, false) => None,
        };
        if let Some(message) = message {
            tree.text_in(
                layout.rows.x + 2.0 * GAP,
                layout.rows.y + 2.0 * GAP,
                (layout.rows.w - 4.0 * GAP).max(0.0),
                message,
                palette.subtext0,
                TEXT,
            );
        } else {
            self.render_rows(tree, palette, &layout);
        }
        tree.unclip();
    }

    /// One line per entry on screen, striped by its place in the whole list
    /// so the banding does not crawl as the view scrolls.
    fn render_rows(&self, tree: &mut RenderTree, palette: &Palette, layout: &BinLayout) {
        let columns = layout.columns();
        let rows = self.visible_rows(layout);
        for (row, index) in (rows.start..rows.end()).enumerate() {
            let Some(entry) = self.entries.get(index) else {
                break;
            };
            #[allow(
                clippy::cast_precision_loss,
                reason = "a row on screen: a handful, far inside f32's exact range"
            )]
            let y = layout.rows.y + row as f32 * ROW_H;
            let table_w = layout.table_w();
            if self.chosen.contains(&entry.id) {
                tree.fill_rect(
                    layout.rows.x,
                    y,
                    table_w,
                    ROW_H,
                    with_alpha(palette.accent, 40),
                );
            } else if index % 2 == 1 {
                tree.fill_rect(layout.rows.x, y, table_w, ROW_H, palette.base);
            }
            if self.viewport.selected() == Some(index) {
                tree.stroke_rect(layout.rows.x, y, table_w, ROW_H, palette.surface2, 1.0);
            }
            // A damaged entry's unknowns are drawn as such, in the quieter
            // ink, so "Unknown" is not read as a folder called Unknown.
            let ink = if entry.is_readable() {
                palette.text
            } else {
                palette.subtext0
            };
            let cells = [
                entry.display_name(),
                folder_text(entry),
                deleted_text(entry),
                size_text(entry),
            ];
            for (text, (x, w)) in cells.iter().zip(columns) {
                tree.text_in(
                    x + GAP,
                    y + (ROW_H - TEXT) / 2.0,
                    (w - 1.5 * GAP).max(0.0),
                    text,
                    ink,
                    TEXT,
                );
            }
        }
    }

    /// Recompute the status line's summary.
    fn refresh_summary(&mut self) {
        self.summary = if let Some(error) = &self.error {
            error.clone()
        } else if self.entries.is_empty() {
            "The recycle bin is empty".to_string()
        } else {
            let mut summary = format!("{} in the recycle bin", items(self.entries.len()));
            if !self.chosen.is_empty() {
                summary.push_str(&format!(", {} chosen", self.chosen.len()));
            }
            summary
        };
    }
}

/// "1 item", "2 items".
#[must_use]
pub fn items(count: usize) -> String {
    if count == 1 {
        "1 item".to_string()
    } else {
        format!("{count} items")
    }
}

/// The folder an entry was deleted from, as the user reads it -- where
/// Restore puts it back.
#[must_use]
pub fn folder_text(entry: &RecycleEntry) -> String {
    match &entry.original_path {
        Some(path) => path
            .parent()
            .map_or_else(String::new, |folder| folder.shown().to_string()),
        None => "Unknown".to_string(),
    }
}

/// When an entry was deleted.
#[must_use]
pub fn deleted_text(entry: &RecycleEntry) -> String {
    entry
        .recycled_at
        .and_then(|at| at.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or_else(|| "Unknown".to_string(), |d| format_datetime(d.as_secs()))
}

/// How big an entry is: a file's size; a folder or a link says which it is,
/// since a folder's own byte count is not what a size means and a link's is
/// not what it names.
#[must_use]
pub fn size_text(entry: &RecycleEntry) -> String {
    if entry.is_link {
        "Link".to_string()
    } else if entry.is_dir {
        "Folder".to_string()
    } else {
        guitk::bytes::iec(entry.size)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn entry(id: &str, path: Option<&str>, secs: u64) -> RecycleEntry {
        RecycleEntry {
            id: id.to_string(),
            original_path: path.map(PathBuf::from),
            recycled_at: path
                .and_then(|_| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs))),
            size: 1536,
            is_dir: false,
            is_link: false,
        }
    }

    fn view_of(entries: Vec<RecycleEntry>) -> BinView {
        let mut view = BinView {
            entries,
            chosen: BTreeSet::new(),
            viewport: ListViewport::new(0),
            anchor: None,
            error: None,
            summary: String::new(),
        };
        view.refresh_summary();
        view
    }

    fn three() -> BinView {
        view_of(vec![
            entry("c", Some("/home/u/Documents/c.txt"), 300),
            entry("b", Some("/home/u/b.txt"), 200),
            entry("a", Some("/home/u/Pictures/a.png"), 100),
        ])
    }

    fn chosen_ids(view: &BinView) -> Vec<&str> {
        view.chosen().iter().map(|e| e.id.as_str()).collect()
    }

    #[test]
    fn a_row_says_where_it_was_deleted_from_and_when() {
        let e = entry("x", Some("/home/u/Documents/report.txt"), 86_400);
        assert_eq!(e.display_name(), "report.txt");
        assert_eq!(folder_text(&e), "/home/u/Documents");
        assert_eq!(deleted_text(&e), format_datetime(86_400));
        assert_eq!(size_text(&e), guitk::bytes::iec(1536));
    }

    #[test]
    fn a_folder_and_a_link_say_what_they_are_instead_of_a_size() {
        let mut folder = entry("f", Some("/home/u/photos"), 1);
        folder.is_dir = true;
        assert_eq!(size_text(&folder), "Folder");
        let mut link = entry("l", Some("/home/u/shortcut"), 1);
        link.is_link = true;
        assert_eq!(size_text(&link), "Link");
    }

    #[test]
    fn a_damaged_entry_says_unknown_rather_than_guessing() {
        let e = entry("damaged", None, 0);
        assert_eq!(folder_text(&e), "Unknown");
        assert_eq!(deleted_text(&e), "Unknown");
        assert!(
            !e.display_name().contains("damaged_"),
            "{}",
            e.display_name()
        );
    }

    #[test]
    fn choosing_a_row_chooses_it_alone() {
        let mut view = three();
        view.choose_only(1);
        assert_eq!(chosen_ids(&view), ["b"]);
        view.choose_only(2);
        assert_eq!(chosen_ids(&view), ["a"]);
        assert_eq!(view.cursor(), Some(2));
        view.choose_only(9);
        assert_eq!(
            chosen_ids(&view),
            ["a"],
            "a row that is not there chose nothing"
        );
    }

    #[test]
    fn the_arrows_move_the_choice_and_shift_extends_it() {
        let mut view = three();
        view.step(1, false);
        assert_eq!(
            chosen_ids(&view),
            ["c"],
            "the first press lands on the first row"
        );
        view.step(1, true);
        assert_eq!(chosen_ids(&view), ["c", "b"]);
        view.step(1, true);
        assert_eq!(chosen_ids(&view), ["c", "b", "a"]);
        view.step(-1, true);
        assert_eq!(
            chosen_ids(&view),
            ["c", "b"],
            "shrinks back towards the anchor"
        );
        view.step(1, false);
        assert_eq!(chosen_ids(&view), ["a"]);
        view.step(5, false);
        assert_eq!(chosen_ids(&view), ["a"], "stops at the last row");
        view.go_to(0, false);
        assert_eq!(chosen_ids(&view), ["c"]);
    }

    #[test]
    fn up_with_nothing_under_the_keyboard_lands_on_the_last_row() {
        let mut view = three();
        view.step(-1, false);
        assert_eq!(chosen_ids(&view), ["a"]);
    }

    #[test]
    fn choose_all_takes_every_row() {
        let mut view = three();
        view.choose_all();
        assert_eq!(chosen_ids(&view), ["c", "b", "a"]);
        assert!(view.summary().contains("3 chosen"), "{}", view.summary());
    }

    #[test]
    fn the_buttons_say_why_they_are_off() {
        let mut view = three();
        assert!(view.button_state(BinButton::Restore).is_disabled());
        assert!(view.button_state(BinButton::DeleteForever).is_disabled());
        assert!(view.button_state(BinButton::Empty).is_enabled());
        view.choose_only(0);
        assert!(view.button_state(BinButton::Restore).is_enabled());
        assert!(view.button_state(BinButton::DeleteForever).is_enabled());

        let mut damaged = view_of(vec![entry("d", None, 0)]);
        damaged.choose_only(0);
        let restore = damaged.button_state(BinButton::Restore);
        assert!(restore.is_disabled());
        assert!(
            restore.reason().is_some_and(|r| r.contains("damaged")),
            "{restore:?}"
        );
        assert!(damaged.button_state(BinButton::DeleteForever).is_enabled());

        let empty = view_of(Vec::new());
        assert!(empty.button_state(BinButton::Empty).is_disabled());
    }

    #[test]
    fn the_summary_counts_the_bin_and_the_choice() {
        let mut view = three();
        assert_eq!(view.summary(), "3 items in the recycle bin");
        view.choose_only(0);
        assert_eq!(view.summary(), "3 items in the recycle bin, 1 chosen");
        assert_eq!(view_of(Vec::new()).summary(), "The recycle bin is empty");
        assert_eq!(
            view_of(vec![entry("one", Some("/x/y"), 1)]).summary(),
            "1 item in the recycle bin"
        );
    }

    #[test]
    fn a_click_finds_the_row_drawn_under_it_after_a_scroll() {
        let entries = (0..50)
            .map(|i| entry(&format!("e{i:02}"), Some("/home/u/f.txt"), 1000 - i))
            .collect();
        let mut view = view_of(entries);
        let pane = Rect::new(200.0, 64.0, 700.0, 512.0);
        let layout = BinLayout::for_pane(pane);
        view.fit(layout.capacity());
        view.scroll_by(10);
        let y = layout.rows.y + 2.0 * ROW_H + ROW_H / 2.0;
        assert_eq!(view.hit(pane, 300.0, y), Some(BinHit::Row(12)));
        // Below the last row of a short bin is blank, not the last row.
        let short = three();
        let below = layout.rows.y + 5.0 * ROW_H;
        assert_eq!(short.hit(pane, 300.0, below), Some(BinHit::Blank));
        assert_eq!(short.hit(pane, 10.0, below), None, "outside the pane");
    }

    #[test]
    fn every_button_is_where_the_hit_test_finds_it() {
        let view = three();
        let pane = Rect::new(200.0, 64.0, 700.0, 512.0);
        let layout = BinLayout::for_pane(pane);
        for (button, rect) in layout.buttons {
            let hit = view.hit(pane, rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            assert_eq!(hit, Some(BinHit::Button(button)));
            assert!(
                rect.x + rect.w <= pane.x + pane.w,
                "{button:?} runs off the pane"
            );
        }
        let [(_, restore), (_, forever), (_, empty)] = layout.buttons;
        assert!(restore.x + restore.w < forever.x);
        assert!(
            forever.x + forever.w < empty.x,
            "Empty overlaps Delete permanently"
        );
    }

    #[test]
    fn fitting_the_same_height_again_leaves_the_scroll_alone() {
        let entries = (0..50)
            .map(|i| entry(&format!("e{i:02}"), Some("/home/u/f.txt"), 1000 - i))
            .collect();
        let mut view = view_of(entries);
        view.fit(10);
        view.choose_only(0);
        view.scroll_by(20);
        assert_eq!(view.first_visible(), 20);
        view.fit(10);
        assert_eq!(view.first_visible(), 20, "the wheel's scroll was undone");
    }
}
