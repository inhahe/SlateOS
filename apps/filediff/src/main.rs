//! `Slate OS` File Diff/Compare Tool
//!
//! A desktop application for comparing files and directories with:
//! - Myers diff algorithm for optimal edit scripts
//! - Side-by-side view with synchronized scrolling
//! - Unified diff view with +/- markers
//! - Inline diff view with character-level highlighting
//! - Color coding: green additions, red deletions, yellow modifications
//! - Navigation: jump to next/previous change
//! - File loading for two-file comparison
//! - Statistics: line counts, change counts, similarity percentage
//! - Merge support: accept left/right/both per hunk
//! - Directory comparison mode
//! - Ignore options: whitespace, case, blank lines
//! - Search within diff panels
//!
//! Uses the guitk library for UI rendering with Catppuccin Mocha colors.

#![deny(clippy::all, clippy::pedantic)]
#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::unreadable_literal,
    clippy::module_name_repetitions,
    clippy::struct_excessive_bools
)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[allow(unused_imports)]
use guitk::layout::{FlexAlign, FlexDirection, FlexItem, FlexJustify, SizeConstraint};
#[allow(unused_imports)]
use guitk::render::{FontFamily, FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[allow(unused_imports)]
use guitk::style::{Borders, CornerRadii, Edges, FontWeight, Style, TextAlign};
use guitk::text;
use guitk::textfind;
use guitk::wheel;
#[allow(unused_imports)]
use guitk::widget::{Widget, WidgetId, WidgetTree};

use std::fmt;

// The diff/merge engine lives in the shared `diffcore` crate so the text
// editors can reuse the exact same algorithms (Myers line diff, inline diff,
// per-hunk merge, and diff3 three-way merge). This tool only adds the UI:
// side-by-side/unified/inline views, search, and directory comparison.
use diffcore::{
    DiffEdit, DiffOp, DiffResult, DiffStats, IgnoreOptions, InlineEdit, MergeDecision, MergeState,
    compute_diff, inline_diff,
};

// ============================================================================
// Catppuccin Mocha color palette
// ============================================================================

/// Catppuccin Mocha theme colors used throughout the diff tool.
pub mod colors {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const TEAL: Color = Color::from_hex(0x94E2D5);

    // Diff-specific background colors (semi-transparent effect via muted shades)
    pub const ADD_BG: Color = Color::rgba(166, 227, 161, 30);
    pub const DEL_BG: Color = Color::rgba(243, 139, 168, 30);
    pub const ADD_LINE_BG: Color = Color::rgba(166, 227, 161, 50);
    pub const DEL_LINE_BG: Color = Color::rgba(243, 139, 168, 50);

    // Search highlights. The focused match is opaque and the rest are washes,
    // so "which one is 4 of 17" is answerable at a glance rather than by
    // counting down the panel.
    pub const SEARCH_BG: Color = Color::rgba(249, 226, 175, 60);
    pub const SEARCH_CURRENT_BG: Color = Color::rgba(250, 179, 135, 150);
}

// ============================================================================
// Configuration constants
// ============================================================================

/// Font size for diff content display.
const CONTENT_FONT_SIZE: f32 = 13.0;

/// Font size for UI elements (toolbar, status bar).
const UI_FONT_SIZE: f32 = 12.0;

/// Height of each diff line in pixels.
const LINE_HEIGHT: f32 = 20.0;

/// Width of the line number gutter in pixels.
const GUTTER_WIDTH: f32 = 55.0;

/// Height of the toolbar area.
const TOOLBAR_HEIGHT: f32 = 38.0;

/// Height of the status bar.
const STATUS_BAR_HEIGHT: f32 = 26.0;

/// Width of one cell of the diff content grid.
///
/// The two panels are a code view: every row is laid out against the same
/// column positions, so the content is a grid and the cell has to come from
/// the face it is drawn in rather than from a guess. This used to be a
/// hardcoded 7.8, which matched the built-in face at 13 px and nothing else —
/// with any other face the prefix column, the inline highlight rectangles and
/// the text they are meant to sit behind all drifted apart.
///
/// Only the *content* panels are a grid. Toolbar labels and status text are
/// proportional UI text and are measured with `text::width`.
///
/// The cell was then `text::digit_advance` — the right idea, the wrong face.
/// A digit's advance *in the proportional UI face* is a cell only digits fit,
/// and source code is not digits: `'W'` is nearly twice as wide, `'i'` barely
/// half. A grid needs a face where every glyph advances the same distance,
/// which is what `text::cell_advance` asks for.
///
/// **What this is still for, and what it is no longer for.** It reserves the
/// two-column prefix gutter that holds `'+'`, `'-'` or a space — a fixed
/// indent, of known ASCII characters, which is exactly the job a cell width
/// can do. It is *not* used to place anything against the line's own text any
/// more: a companion `columns()` used to count characters and multiply, which
/// is right only where every character advances one cell, and a tab does not —
/// it draws four cells wide and counts as one `char`, so on the tab-indented
/// source that most diffs are made of, every inline highlight and every search
/// box was placed three cells short per level of indentation. Anything
/// positioned against real text now measures it: see `render_inline_spans`
/// and `render_search_highlights`.
fn char_width() -> f32 {
    text::cell_advance(CONTENT_FONT_SIZE, FontWeightHint::Regular)
}

/// The next position after `index` in a list of `len` items, wrapping at the end.
///
/// `None` for an empty list, and that is the point: `checked_rem` returns `None`
/// exactly when `len == 0`, so the emptiness test *is* the wrap rather than a
/// separate `if !is_empty()` written above it. Three cursors in this program
/// cycle over a `Vec` -- the search matches, the change list and the merge
/// hunks -- and each of them used to spell the guard and the `%` out
/// separately, which is three chances for the two halves to stop agreeing.
fn wrap_next(index: usize, len: usize) -> Option<usize> {
    index.saturating_add(1).checked_rem(len)
}

/// The previous position before `index` in a list of `len` items, wrapping at
/// the start. `None` for an empty list.
fn wrap_prev(index: usize, len: usize) -> Option<usize> {
    // Two `checked_sub`s, each failing on exactly the case it stands for:
    // `len - 1` fails when the list is empty, and `index - 1` fails when the
    // cursor is already on the first item, which is precisely when it should
    // wrap round to the last.
    let last = len.checked_sub(1)?;
    Some(index.checked_sub(1).unwrap_or(last))
}

/// One extra row at each end of a viewport.
///
/// The row the viewport's top edge cuts through and the row its bottom edge
/// cuts through are both partly visible. A viewport that drew neither would
/// show a blank strip along both edges whenever the scroll offset was not a
/// whole number of rows, which is most of the time while scrolling.
const OVERSCAN_ROWS: usize = 2;

/// The rows of a list of `item_count` items that a viewport shows.
///
/// Four render paths ask exactly this and each wrote the answer out longhand --
/// the same `scroll as usize`, the same `(height / LINE_HEIGHT) as usize`, the
/// same `.saturating_add(..).min(len)`. Three of them added a bare `+ 2` with
/// no name on it and the fourth, the directory list, added nothing, so the
/// directory view dropped its bottom row while scrolling for no reason anybody
/// had decided on.
fn visible_range(scroll: f32, content_height: f32, item_count: usize) -> core::ops::Range<usize> {
    // A float-to-integer `as` cast saturates rather than wrapping, so a
    // negative scroll offset -- which a trackpad's overscroll can produce for a
    // frame before the clamp catches up -- starts at the top of the list
    // instead of at `usize::MAX`.
    let first = scroll as usize;
    let rows = (content_height / LINE_HEIGHT) as usize;
    let end = first
        .saturating_add(rows)
        .saturating_add(OVERSCAN_ROWS)
        .min(item_count);
    first.min(end)..end
}

/// Maximum number of search results to track.
const MAX_SEARCH_RESULTS: usize = 10_000;

/// Padding inside panels.
const PANEL_PADDING: f32 = 4.0;

/// Separator width between side-by-side panels.
const SEPARATOR_WIDTH: f32 = 2.0;

// ============================================================================
// Directory comparison
// ============================================================================

/// Status of a file in a directory comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileCompareStatus {
    /// File is identical in both directories.
    Same,
    /// File differs between directories.
    Different,
    /// File exists only in the left directory.
    OnlyLeft,
    /// File exists only in the right directory.
    OnlyRight,
}

impl fmt::Display for FileCompareStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Same => write!(f, "Same"),
            Self::Different => write!(f, "Different"),
            Self::OnlyLeft => write!(f, "Only in left"),
            Self::OnlyRight => write!(f, "Only in right"),
        }
    }
}

/// A single entry in a directory comparison result.
#[derive(Clone, Debug)]
pub struct DirCompareEntry {
    /// Relative path within the compared directories.
    pub path: String,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// Comparison status.
    pub status: FileCompareStatus,
}

/// Result of comparing two directories.
#[derive(Clone, Debug, Default)]
pub struct DirCompareResult {
    /// All entries found during comparison.
    pub entries: Vec<DirCompareEntry>,
    /// Count of identical files.
    pub same_count: usize,
    /// Count of differing files.
    pub different_count: usize,
    /// Count of files only in left.
    pub only_left_count: usize,
    /// Count of files only in right.
    pub only_right_count: usize,
}

/// Compare two lists of filenames (simulated directory comparison).
#[must_use]
pub fn compare_directories(
    left_files: &[(&str, &str)],
    right_files: &[(&str, &str)],
) -> DirCompareResult {
    let mut result = DirCompareResult::default();

    let mut left_map: Vec<(&str, &str)> = left_files.to_vec();
    left_map.sort_by_key(|(name, _)| *name);

    let mut right_map: Vec<(&str, &str)> = right_files.to_vec();
    right_map.sort_by_key(|(name, _)| *name);

    let mut li = 0;
    let mut ri = 0;

    while li < left_map.len() && ri < right_map.len() {
        let (lname, lcontent) = left_map.get(li).copied().unwrap_or(("", ""));
        let (rname, rcontent) = right_map.get(ri).copied().unwrap_or(("", ""));

        match lname.cmp(rname) {
            std::cmp::Ordering::Equal => {
                if lcontent == rcontent {
                    result.entries.push(DirCompareEntry {
                        path: lname.to_string(),
                        is_dir: false,
                        status: FileCompareStatus::Same,
                    });
                    result.same_count = result.same_count.saturating_add(1);
                } else {
                    result.entries.push(DirCompareEntry {
                        path: lname.to_string(),
                        is_dir: false,
                        status: FileCompareStatus::Different,
                    });
                    result.different_count = result.different_count.saturating_add(1);
                }
                li = li.saturating_add(1);
                ri = ri.saturating_add(1);
            }
            std::cmp::Ordering::Less => {
                result.entries.push(DirCompareEntry {
                    path: lname.to_string(),
                    is_dir: false,
                    status: FileCompareStatus::OnlyLeft,
                });
                result.only_left_count = result.only_left_count.saturating_add(1);
                li = li.saturating_add(1);
            }
            std::cmp::Ordering::Greater => {
                result.entries.push(DirCompareEntry {
                    path: rname.to_string(),
                    is_dir: false,
                    status: FileCompareStatus::OnlyRight,
                });
                result.only_right_count = result.only_right_count.saturating_add(1);
                ri = ri.saturating_add(1);
            }
        }
    }

    while li < left_map.len() {
        let (lname, _) = left_map.get(li).copied().unwrap_or(("", ""));
        result.entries.push(DirCompareEntry {
            path: lname.to_string(),
            is_dir: false,
            status: FileCompareStatus::OnlyLeft,
        });
        result.only_left_count = result.only_left_count.saturating_add(1);
        li = li.saturating_add(1);
    }

    while ri < right_map.len() {
        let (rname, _) = right_map.get(ri).copied().unwrap_or(("", ""));
        result.entries.push(DirCompareEntry {
            path: rname.to_string(),
            is_dir: false,
            status: FileCompareStatus::OnlyRight,
        });
        result.only_right_count = result.only_right_count.saturating_add(1);
        ri = ri.saturating_add(1);
    }

    result
}

// ============================================================================
// Search
// ============================================================================

/// A search match within the diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchMatch {
    /// Which panel the match is in (0 = left, 1 = right).
    pub panel: u8,
    /// Edit index in the diff result.
    pub edit_index: usize,
    /// Byte offset within the line text.
    pub byte_offset: usize,
    /// Length of the match in bytes.
    pub match_len: usize,
}

/// Search state for find-in-diff.
#[derive(Clone, Debug, Default)]
pub struct SearchState {
    /// Current search query.
    pub query: String,
    /// Whether search is case-sensitive.
    pub case_sensitive: bool,
    /// All matches found.
    pub matches: Vec<SearchMatch>,
    /// Index of the currently focused match.
    pub current_match: usize,
    /// Whether the search bar is visible.
    pub visible: bool,
}

impl SearchState {
    /// Perform a search across diff edits.
    pub fn search(&mut self, edits: &[DiffEdit]) {
        self.matches.clear();
        self.current_match = 0;

        if self.query.is_empty() {
            return;
        }

        // The offsets have to be offsets into `edit.text` — that is the string
        // the highlighter slices. Searching a `to_lowercase()` copy gave the
        // copy's offsets, which drift apart from the real ones at the first
        // character whose folded form is a different length (`İ` U+0130 is two
        // bytes and folds to three). The match length was likewise taken from
        // the query rather than from what matched, and the scan resumed one
        // byte past each match's *start*, so `aa` in `aaaa` was reported three
        // times, overlapping.
        let case = textfind::Case::sensitive(self.case_sensitive);
        // Cloned because `push_matches_for_edit` takes `&mut self`; the search
        // itself borrows nothing but the edit's own text.
        let query = self.query.clone();
        for (i, edit) in edits.iter().enumerate() {
            for (start, end) in textfind::matches(&edit.text, &query, case) {
                self.push_matches_for_edit(i, edit.op, start, end.saturating_sub(start));
                if self.matches.len() >= MAX_SEARCH_RESULTS {
                    return;
                }
            }
        }
    }

    /// Push search matches for a given edit based on its operation type.
    fn push_matches_for_edit(
        &mut self,
        edit_index: usize,
        op: DiffOp,
        byte_offset: usize,
        match_len: usize,
    ) {
        match op {
            DiffOp::Equal => {
                self.matches.push(SearchMatch {
                    panel: 0,
                    edit_index,
                    byte_offset,
                    match_len,
                });
                self.matches.push(SearchMatch {
                    panel: 1,
                    edit_index,
                    byte_offset,
                    match_len,
                });
            }
            DiffOp::Delete => {
                self.matches.push(SearchMatch {
                    panel: 0,
                    edit_index,
                    byte_offset,
                    match_len,
                });
            }
            DiffOp::Insert => {
                self.matches.push(SearchMatch {
                    panel: 1,
                    edit_index,
                    byte_offset,
                    match_len,
                });
            }
        }
    }

    /// Move to the next match.
    pub fn next_match(&mut self) {
        if let Some(next) = wrap_next(self.current_match, self.matches.len()) {
            self.current_match = next;
        }
    }

    /// Move to the previous match.
    pub fn prev_match(&mut self) {
        if let Some(prev) = wrap_prev(self.current_match, self.matches.len()) {
            self.current_match = prev;
        }
    }

    /// Get the current match if any.
    #[must_use]
    pub fn current(&self) -> Option<&SearchMatch> {
        self.matches.get(self.current_match)
    }

    /// The matches that fall on one edit, in the order they occur in the line.
    ///
    /// A binary search rather than a filter, because the highlighter asks this
    /// once per visible row per frame and `matches` runs to
    /// [`MAX_SEARCH_RESULTS`]. It is sound because [`SearchState::search`]
    /// walks the edits in order and pushes as it goes, so the list is
    /// non-decreasing in `edit_index` by construction -- there is no separate
    /// sort step that could be forgotten or reordered.
    fn matches_on_edit(&self, edit_index: usize) -> &[SearchMatch] {
        let start = self.matches.partition_point(|m| m.edit_index < edit_index);
        let end = self.matches.partition_point(|m| m.edit_index <= edit_index);
        self.matches.get(start..end).unwrap_or(&[])
    }
}

/// Which panel's copy of a match a single-column view should draw.
///
/// [`SearchState::push_matches_for_edit`] records an equal line's match twice,
/// once per side-by-side panel, because side-by-side draws that line twice. The
/// unified and inline views draw it once and so must take one copy -- and
/// taking it by a rule rather than by "whichever came first" means the two
/// single-column views agree with each other, and with side-by-side, about
/// which occurrence *is* the match.
const fn canonical_panel(op: DiffOp) -> u8 {
    match op {
        DiffOp::Equal | DiffOp::Delete => 0,
        DiffOp::Insert => 1,
    }
}

// ============================================================================
// View mode
// ============================================================================

/// Display mode for the diff viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// Side-by-side panels.
    SideBySide,
    /// Unified diff format.
    Unified,
    /// Inline with character-level highlighting.
    Inline,
}

impl fmt::Display for ViewMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SideBySide => write!(f, "Side-by-Side"),
            Self::Unified => write!(f, "Unified"),
            Self::Inline => write!(f, "Inline"),
        }
    }
}

// ============================================================================
// Side-by-side pairing
// ============================================================================

/// A paired row for side-by-side display.
#[derive(Clone, Debug)]
struct SideBySidePair {
    left_line: Option<usize>,
    left_text: Option<String>,
    left_op: Option<DiffOp>,
    /// The edit the left half was built from.
    ///
    /// A pair's two halves can come from two *different* edits -- that is
    /// exactly what a paired delete+insert is -- so anything that wants to say
    /// something about "the edit on this line" has to ask per side. The
    /// alternative, recovering it from `row_of_edit`, would be searching a map
    /// for a value the loop that built the row already had.
    left_edit: Option<usize>,
    right_line: Option<usize>,
    right_text: Option<String>,
    right_op: Option<DiffOp>,
    /// The edit the right half was built from.
    right_edit: Option<usize>,
}

/// The side-by-side layout of an edit list: the rows to draw, and the row each
/// edit landed on.
///
/// The two live in one value because they are one fact, produced by one loop.
/// Side-by-side is the only view whose row count differs from the edit count --
/// a delete immediately followed by an insert is one modified line and occupies
/// one row, so two edits share it -- and three separate places used to assume
/// the two numbers were the same: [`FileDiffApp::scroll_to_current_change`]
/// scrolled to *edit* N when asked for the change on row N,
/// [`FileDiffApp::max_scroll`] let the view scroll one row past the end per
/// modified line, and the search highlighter (which did not exist) would have
/// been the third. On a file with five hundred modified lines, "jump to the
/// last change" scrolled five hundred rows past the bottom and showed a blank
/// panel.
#[derive(Clone, Debug, Default)]
struct SideBySideRows {
    /// The rows, in display order.
    pairs: Vec<SideBySidePair>,
    /// Which row each edit is displayed on, parallel to the edit list.
    row_of_edit: Vec<usize>,
}

/// Build side-by-side pairs from an edit list.
///
/// Equal lines appear on both sides. Deletes appear on the left with a blank right.
/// Inserts appear on the right with a blank left. Consecutive delete+insert pairs
/// are aligned on the same row.
fn build_side_by_side_pairs(edits: &[DiffEdit]) -> SideBySideRows {
    let mut pairs: Vec<SideBySidePair> = Vec::new();
    // Filled in step with `pairs` rather than derived from it afterwards: the
    // loop below is the only place that knows an edit was folded into the row
    // before it, and a second pass would have to reconstruct that decision from
    // the output and could reconstruct it differently.
    let mut row_of_edit: Vec<usize> = Vec::with_capacity(edits.len());
    let mut i = 0;

    while i < edits.len() {
        let Some(edit) = edits.get(i) else { break };
        let row = pairs.len();

        match edit.op {
            DiffOp::Equal => {
                row_of_edit.push(row);
                pairs.push(SideBySidePair {
                    left_line: edit.left_line,
                    left_text: Some(edit.text.clone()),
                    left_op: Some(DiffOp::Equal),
                    left_edit: Some(i),
                    right_line: edit.right_line,
                    right_text: Some(edit.text.clone()),
                    right_op: Some(DiffOp::Equal),
                    right_edit: Some(i),
                });
                i = i.saturating_add(1);
            }
            DiffOp::Delete => {
                // Check if the next edit is an insert (paired modification)
                let next = edits.get(i.saturating_add(1));
                if let Some(next_edit) = next
                    && next_edit.op == DiffOp::Insert
                {
                    // Paired: show delete on left, insert on right. Both edits
                    // land on this one row, which is the whole reason the map
                    // exists.
                    row_of_edit.push(row);
                    row_of_edit.push(row);
                    pairs.push(SideBySidePair {
                        left_line: edit.left_line,
                        left_text: Some(edit.text.clone()),
                        left_op: Some(DiffOp::Delete),
                        left_edit: Some(i),
                        right_line: next_edit.right_line,
                        right_text: Some(next_edit.text.clone()),
                        right_op: Some(DiffOp::Insert),
                        right_edit: Some(i.saturating_add(1)),
                    });
                    i = i.saturating_add(2);
                    continue;
                }
                // Unpaired delete
                row_of_edit.push(row);
                pairs.push(SideBySidePair {
                    left_line: edit.left_line,
                    left_text: Some(edit.text.clone()),
                    left_op: Some(DiffOp::Delete),
                    left_edit: Some(i),
                    right_line: None,
                    right_text: None,
                    right_op: None,
                    right_edit: None,
                });
                i = i.saturating_add(1);
            }
            DiffOp::Insert => {
                row_of_edit.push(row);
                pairs.push(SideBySidePair {
                    left_line: None,
                    left_text: None,
                    left_op: None,
                    left_edit: None,
                    right_line: edit.right_line,
                    right_text: Some(edit.text.clone()),
                    right_op: Some(DiffOp::Insert),
                    right_edit: Some(i),
                });
                i = i.saturating_add(1);
            }
        }
    }

    SideBySideRows { pairs, row_of_edit }
}

// ============================================================================
// Inline rows
// ============================================================================

/// A row in the inline diff view.
#[derive(Clone, Debug)]
struct InlineRow {
    op: DiffOp,
    line_num: Option<usize>,
    text: String,
    spans: Vec<InlineEdit>,
}

/// Build inline rows from edits, computing character-level diffs for change pairs.
fn build_inline_rows(edits: &[DiffEdit]) -> Vec<InlineRow> {
    let mut rows = Vec::new();
    let mut i = 0;

    while i < edits.len() {
        let Some(edit) = edits.get(i) else { break };

        match edit.op {
            DiffOp::Equal => {
                rows.push(InlineRow {
                    op: DiffOp::Equal,
                    line_num: edit.left_line,
                    text: edit.text.clone(),
                    spans: Vec::new(),
                });
                i = i.saturating_add(1);
            }
            DiffOp::Delete => {
                let next = edits.get(i.saturating_add(1));
                if let Some(next_edit) = next
                    && next_edit.op == DiffOp::Insert
                {
                    let (left_spans, right_spans) = inline_diff(&edit.text, &next_edit.text);

                    rows.push(InlineRow {
                        op: DiffOp::Delete,
                        line_num: edit.left_line,
                        text: edit.text.clone(),
                        spans: left_spans,
                    });
                    rows.push(InlineRow {
                        op: DiffOp::Insert,
                        line_num: next_edit.right_line,
                        text: next_edit.text.clone(),
                        spans: right_spans,
                    });
                    i = i.saturating_add(2);
                    continue;
                }
                rows.push(InlineRow {
                    op: DiffOp::Delete,
                    line_num: edit.left_line,
                    text: edit.text.clone(),
                    spans: Vec::new(),
                });
                i = i.saturating_add(1);
            }
            DiffOp::Insert => {
                rows.push(InlineRow {
                    op: DiffOp::Insert,
                    line_num: edit.right_line,
                    text: edit.text.clone(),
                    spans: Vec::new(),
                });
                i = i.saturating_add(1);
            }
        }
    }

    rows
}

// ============================================================================
// Diff line rendering data (avoids too-many-arguments on render methods)
// ============================================================================

/// The search matches that could fall on one line, and which is focused.
///
/// Carried as one value rather than three loose arguments because the three are
/// only meaningful together: a match list without the panel it is being drawn
/// for would highlight an equal line twice, and without the focused match every
/// hit would look like every other. Passing them separately invites a call site
/// that has two of the three.
#[derive(Clone, Copy)]
struct SearchOverlay<'a> {
    /// Every match on the edit this line came from -- both panels' worth,
    /// because for an equal line the same occurrence is recorded twice.
    matches: &'a [SearchMatch],
    /// Which panel this line is, so the other panel's copies are skipped.
    panel: u8,
    /// The focused match, drawn differently from the rest.
    current: Option<SearchMatch>,
}

/// Parameters for rendering a single diff line in side-by-side mode.
struct DiffLineParams<'a> {
    x: f32,
    y: f32,
    width: f32,
    line_num: Option<usize>,
    text: Option<&'a str>,
    op: Option<DiffOp>,
    /// Drawn between the line background and the line text, so the boxes sit
    /// behind the characters they mark rather than over them.
    search: SearchOverlay<'a>,
}

// ============================================================================
// Application state
// ============================================================================

/// Main application state.
pub struct FileDiffApp {
    /// Width of the application window.
    pub width: f32,
    /// Height of the application window.
    pub height: f32,

    /// Left file path.
    pub left_path: String,
    /// Right file path.
    pub right_path: String,
    /// Left file content.
    pub left_content: String,
    /// Right file content.
    pub right_content: String,

    /// Current diff result.
    pub diff: Option<DiffResult>,
    /// Diff statistics.
    pub stats: DiffStats,
    /// Merge state.
    pub merge: Option<MergeState>,

    /// Side-by-side layout of `diff`, rebuilt whenever the diff is.
    ///
    /// Cached rather than rebuilt inside `render`, where it used to live, for
    /// two reasons. It clones every line of both files, so building it once a
    /// frame copied the whole document sixty times a second -- on a
    /// hundred-thousand-line comparison that is megabytes of `String`
    /// allocation per frame, against a two-millisecond budget. And it carries
    /// `row_of_edit`, which the scroll and search code need *between* frames
    /// and could not reach at all while it was a local variable.
    sbs: SideBySideRows,
    /// Inline layout of `diff`, rebuilt whenever the diff is. Cached for the
    /// same reason as `sbs`: it clones every line too.
    inline_rows: Vec<InlineRow>,

    /// Current view mode.
    pub view_mode: ViewMode,
    /// Scroll offset (in lines) for the left panel.
    pub scroll_left: f32,
    /// Scroll offset (in lines) for the right panel.
    pub scroll_right: f32,
    /// Whether scroll is synchronized between panels.
    pub sync_scroll: bool,
    /// Index of the current change being viewed.
    pub current_change_index: usize,
    /// Indices of change edits in the edit list (for navigation).
    pub change_indices: Vec<usize>,

    /// Ignore options.
    pub ignore_opts: IgnoreOptions,
    /// Search state.
    pub search: SearchState,

    /// Directory comparison result (when in directory mode).
    pub dir_compare: Option<DirCompareResult>,
    /// Whether we are in directory comparison mode.
    pub dir_mode: bool,

    /// Scroll offset for directory comparison view.
    pub dir_scroll: f32,

    /// Currently selected hunk index for merge operations.
    pub selected_hunk: usize,

    /// Whether the toolbar dropdown for view mode is open.
    pub view_mode_dropdown_open: bool,
}

impl Default for FileDiffApp {
    fn default() -> Self {
        Self::new()
    }
}

impl FileDiffApp {
    /// Create a new application instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            width: 1200.0,
            height: 800.0,
            left_path: String::new(),
            right_path: String::new(),
            left_content: String::new(),
            right_content: String::new(),
            diff: None,
            stats: DiffStats::default(),
            merge: None,
            sbs: SideBySideRows::default(),
            inline_rows: Vec::new(),
            view_mode: ViewMode::SideBySide,
            scroll_left: 0.0,
            scroll_right: 0.0,
            sync_scroll: true,
            current_change_index: 0,
            change_indices: Vec::new(),
            ignore_opts: IgnoreOptions::default(),
            search: SearchState::default(),
            dir_compare: None,
            dir_mode: false,
            dir_scroll: 0.0,
            selected_hunk: 0,
            view_mode_dropdown_open: false,
        }
    }

    /// Load two files for comparison.
    pub fn load_files(
        &mut self,
        left_path: &str,
        left_content: &str,
        right_path: &str,
        right_content: &str,
    ) {
        self.left_path = left_path.to_string();
        self.right_path = right_path.to_string();
        self.left_content = left_content.to_string();
        self.right_content = right_content.to_string();
        self.dir_mode = false;
        self.dir_compare = None;
        self.recompute_diff();
    }

    /// Recompute the diff with current options.
    pub fn recompute_diff(&mut self) {
        let diff = compute_diff(&self.left_content, &self.right_content, &self.ignore_opts);
        self.stats = DiffStats::from_diff(&diff);

        // Build change index list for navigation
        self.change_indices.clear();
        for (i, edit) in diff.edits.iter().enumerate() {
            if edit.op != DiffOp::Equal {
                self.change_indices.push(i);
            }
        }

        let hunk_count = diff.hunks.len();
        self.merge = Some(MergeState::new(hunk_count));
        // The two layouts are rebuilt here, with the diff they describe, so
        // there is no moment at which a row list refers to edits that no longer
        // exist.
        self.sbs = build_side_by_side_pairs(&diff.edits);
        self.inline_rows = build_inline_rows(&diff.edits);
        self.diff = Some(diff);
        self.current_change_index = 0;
        self.scroll_left = 0.0;
        self.scroll_right = 0.0;
        self.selected_hunk = 0;

        // Re-run the search against the new edit list.
        //
        // Not conditional on the search bar being open, which is what it used
        // to be. Escape hides the bar, not the highlights, so a match list left
        // over from the previous diff outlives the edits it indexes -- toggling
        // "ignore whitespace" with a search still showing would leave boxes
        // drawn on whatever lines those edit numbers now happen to name. That
        // was harmless while nothing drew them; it is not any more.
        self.refresh_search_matches();
    }

    /// Recompute the match list against the current diff, without moving the view.
    fn refresh_search_matches(&mut self) {
        if let Some(ref diff) = self.diff {
            self.search.search(&diff.edits);
        } else {
            // No diff means no edits to search, and stale matches would have
            // the highlighter drawing on a document that is not there.
            self.search.matches.clear();
            self.search.current_match = 0;
        }
    }

    /// Navigate to the next change.
    pub fn next_change(&mut self) {
        if let Some(next) = wrap_next(self.current_change_index, self.change_indices.len()) {
            self.current_change_index = next;
            self.scroll_to_current_change();
        }
    }

    /// Navigate to the previous change.
    pub fn prev_change(&mut self) {
        if let Some(prev) = wrap_prev(self.current_change_index, self.change_indices.len()) {
            self.current_change_index = prev;
            self.scroll_to_current_change();
        }
    }

    /// How many rows the current view displays.
    ///
    /// Not the edit count. Unified draws the edit list directly and
    /// [`build_inline_rows`] emits one row per edit, so for those two the row
    /// *is* the edit -- but side-by-side folds a delete and the insert that
    /// follows it into one row, and asking `diff.edits.len()` there overstates
    /// the list by one row per modified line.
    fn display_row_count(&self) -> usize {
        match self.view_mode {
            ViewMode::SideBySide => self.sbs.pairs.len(),
            ViewMode::Unified => self.diff.as_ref().map_or(0, |d| d.edits.len()),
            ViewMode::Inline => self.inline_rows.len(),
        }
    }

    /// The row the current view shows edit `edit_index` on, if it shows it.
    fn display_row_of_edit(&self, edit_index: usize) -> Option<usize> {
        match self.view_mode {
            ViewMode::SideBySide => self.sbs.row_of_edit.get(edit_index).copied(),
            ViewMode::Unified => self
                .diff
                .as_ref()
                .filter(|d| edit_index < d.edits.len())
                .map(|_| edit_index),
            ViewMode::Inline => (edit_index < self.inline_rows.len()).then_some(edit_index),
        }
    }

    /// Put display row `row` in the middle of the viewport.
    fn scroll_to_row(&mut self, row: usize) {
        let half_visible = self.visible_line_count() / 2.0;
        let target_scroll = (row as f32 - half_visible).max(0.0).min(self.max_scroll());
        self.scroll_left = target_scroll;
        if self.sync_scroll {
            self.scroll_right = target_scroll;
        }
    }

    /// Scroll to make the current change visible.
    fn scroll_to_current_change(&mut self) {
        // Two lookups, both of which can miss, and neither of which is the
        // other's business: the change list holds an *edit* index, and the row
        // that edit is drawn on is the view's to say.
        let Some(&edit_idx) = self.change_indices.get(self.current_change_index) else {
            return;
        };
        let Some(row) = self.display_row_of_edit(edit_idx) else {
            return;
        };
        self.scroll_to_row(row);
    }

    /// Number of lines visible in the content area.
    fn visible_line_count(&self) -> f32 {
        let content_height = self.height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;
        content_height / LINE_HEIGHT
    }

    /// Maximum scroll value.
    fn max_scroll(&self) -> f32 {
        (self.display_row_count() as f32 - self.visible_line_count()).max(0.0)
    }

    /// Toggle an ignore option and recompute.
    pub fn toggle_ignore_whitespace(&mut self) {
        self.ignore_opts.ignore_whitespace = !self.ignore_opts.ignore_whitespace;
        self.recompute_diff();
    }

    /// Toggle case ignore and recompute.
    pub fn toggle_ignore_case(&mut self) {
        self.ignore_opts.ignore_case = !self.ignore_opts.ignore_case;
        self.recompute_diff();
    }

    /// Toggle blank line ignore and recompute.
    pub fn toggle_ignore_blank_lines(&mut self) {
        self.ignore_opts.ignore_blank_lines = !self.ignore_opts.ignore_blank_lines;
        self.recompute_diff();
    }

    /// Accept left side for the selected hunk.
    pub fn accept_left(&mut self) {
        if let Some(ref mut merge) = self.merge {
            merge.set_decision(self.selected_hunk, MergeDecision::AcceptLeft);
        }
    }

    /// Accept right side for the selected hunk.
    pub fn accept_right(&mut self) {
        if let Some(ref mut merge) = self.merge {
            merge.set_decision(self.selected_hunk, MergeDecision::AcceptRight);
        }
    }

    /// Accept both sides for the selected hunk.
    pub fn accept_both(&mut self) {
        if let Some(ref mut merge) = self.merge {
            merge.set_decision(self.selected_hunk, MergeDecision::AcceptBoth);
        }
    }

    /// Get the merged output text.
    #[must_use]
    pub fn merged_text(&self) -> Option<String> {
        match (&self.merge, &self.diff) {
            (Some(merge), Some(diff)) => Some(merge.apply(diff)),
            _ => None,
        }
    }

    /// Handle events from the UI.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Resize { width, height } => {
                self.width = *width as f32;
                self.height = *height as f32;
                EventResult::Consumed
            }
            Event::Key(key_event) if key_event.pressed => self.handle_key(key_event),
            Event::Mouse(mouse_event) => self.handle_mouse(mouse_event),
            _ => EventResult::Ignored,
        }
    }

    /// Handle keyboard input.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if self.search.visible {
            return self.handle_search_key(key);
        }

        match key.key {
            // Navigation
            Key::Down | Key::J => {
                self.scroll_left = (self.scroll_left + 1.0).min(self.max_scroll());
                if self.sync_scroll {
                    self.scroll_right = self.scroll_left;
                }
                EventResult::Consumed
            }
            Key::Up | Key::K => {
                self.scroll_left = (self.scroll_left - 1.0).max(0.0);
                if self.sync_scroll {
                    self.scroll_right = self.scroll_left;
                }
                EventResult::Consumed
            }
            Key::PageDown => {
                let page = self.visible_line_count();
                self.scroll_left = (self.scroll_left + page).min(self.max_scroll());
                if self.sync_scroll {
                    self.scroll_right = self.scroll_left;
                }
                EventResult::Consumed
            }
            Key::PageUp => {
                let page = self.visible_line_count();
                self.scroll_left = (self.scroll_left - page).max(0.0);
                if self.sync_scroll {
                    self.scroll_right = self.scroll_left;
                }
                EventResult::Consumed
            }
            Key::Home if key.modifiers.ctrl => {
                self.scroll_left = 0.0;
                if self.sync_scroll {
                    self.scroll_right = 0.0;
                }
                EventResult::Consumed
            }
            Key::End if key.modifiers.ctrl => {
                self.scroll_left = self.max_scroll();
                if self.sync_scroll {
                    self.scroll_right = self.scroll_left;
                }
                EventResult::Consumed
            }

            // Change navigation (F7/F8 or Ctrl+N/P)
            Key::F7 => {
                self.prev_change();
                EventResult::Consumed
            }
            Key::F8 => {
                self.next_change();
                EventResult::Consumed
            }
            Key::N if key.modifiers.ctrl => {
                self.next_change();
                EventResult::Consumed
            }
            Key::P if key.modifiers.ctrl => {
                self.prev_change();
                EventResult::Consumed
            }

            // View mode toggle
            Key::Num1 if key.modifiers.ctrl => {
                self.view_mode = ViewMode::SideBySide;
                EventResult::Consumed
            }
            Key::Num2 if key.modifiers.ctrl => {
                self.view_mode = ViewMode::Unified;
                EventResult::Consumed
            }
            Key::Num3 if key.modifiers.ctrl => {
                self.view_mode = ViewMode::Inline;
                EventResult::Consumed
            }

            // Sync scroll toggle
            Key::S if key.modifiers.ctrl && key.modifiers.shift => {
                self.sync_scroll = !self.sync_scroll;
                EventResult::Consumed
            }

            // Search
            Key::F if key.modifiers.ctrl => {
                self.search.visible = true;
                EventResult::Consumed
            }

            // Merge actions
            Key::Left if key.modifiers.alt => {
                self.accept_left();
                EventResult::Consumed
            }
            Key::Right if key.modifiers.alt => {
                self.accept_right();
                EventResult::Consumed
            }
            Key::B if key.modifiers.alt => {
                self.accept_both();
                EventResult::Consumed
            }

            // Hunk navigation for merge
            Key::Tab => {
                if let Some(ref diff) = self.diff
                    && let Some(next) = wrap_next(self.selected_hunk, diff.hunks.len())
                {
                    self.selected_hunk = next;
                }
                EventResult::Consumed
            }

            // Ignore toggles
            Key::W if key.modifiers.alt => {
                self.toggle_ignore_whitespace();
                EventResult::Consumed
            }
            Key::C if key.modifiers.alt => {
                self.toggle_ignore_case();
                EventResult::Consumed
            }

            _ => EventResult::Ignored,
        }
    }

    /// Re-run the search against the current diff and show the first hit.
    ///
    /// The three places that edit the query used to run the search and stop
    /// there, which left the view wherever it happened to be. Typing a word
    /// that occurs once, four thousand lines down, changed the counter from
    /// "No matches" to "1/1" and nothing else.
    fn rerun_search(&mut self) {
        self.refresh_search_matches();
        self.scroll_to_current_match();
    }

    /// Scroll so the focused search match is on screen.
    fn scroll_to_current_match(&mut self) {
        // The edit index is copied out before anything mutable happens: the
        // match lives inside `self.search`, and `scroll_to_row` needs `self`.
        let Some(edit_index) = self.search.current().map(|m| m.edit_index) else {
            return;
        };
        let Some(row) = self.display_row_of_edit(edit_index) else {
            return;
        };
        self.scroll_to_row(row);
    }

    /// Handle keyboard input when search bar is active.
    fn handle_search_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape => {
                self.search.visible = false;
                EventResult::Consumed
            }
            Key::Enter => {
                self.search.next_match();
                self.scroll_to_current_match();
                EventResult::Consumed
            }
            Key::Backspace => {
                self.search.query.pop();
                self.rerun_search();
                EventResult::Consumed
            }
            Key::F3 => {
                if key.modifiers.shift {
                    self.search.prev_match();
                } else {
                    self.search.next_match();
                }
                self.scroll_to_current_match();
                EventResult::Consumed
            }
            _ => {
                if let Some(ch) = key.text {
                    self.search.query.push(ch);
                    self.rerun_search();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    /// Handle mouse input.
    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        match &mouse.kind {
            MouseEventKind::Scroll { dy, .. } => {
                // `wheel::rows_f`, not an `Accumulator`: these offsets are
                // fractional row counts, so a trackpad's 0.2 of a notch can be
                // shown as 0.6 of a row straight away rather than banked until
                // it rounds. It replaces a local `SCROLL_SPEED` that happened
                // to hold the same 3.0 -- one more private copy of a constant
                // the toolkit already owns, and the reason twelve handlers here
                // once disagreed about what a notch was worth.
                let delta = wheel::rows_f(*dy);
                let max = self.max_scroll();

                // Determine which panel was scrolled based on x position
                let mid_x = self.width / 2.0;
                if self.sync_scroll || self.view_mode != ViewMode::SideBySide {
                    self.scroll_left = (self.scroll_left + delta).clamp(0.0, max);
                    self.scroll_right = self.scroll_left;
                } else if mouse.x < mid_x {
                    self.scroll_left = (self.scroll_left + delta).clamp(0.0, max);
                } else {
                    self.scroll_right = (self.scroll_right + delta).clamp(0.0, max);
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Render the entire application to a render tree.
    #[must_use]
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: colors::BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut tree);

        let content_y = TOOLBAR_HEIGHT;
        let content_height = self.height - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        if self.dir_mode {
            self.render_dir_compare(&mut tree, content_y, content_height);
        } else if let Some(ref diff) = self.diff {
            tree.push(RenderCommand::PushClip {
                x: 0.0,
                y: content_y,
                width: self.width,
                height: content_height,
            });

            match self.view_mode {
                ViewMode::SideBySide => {
                    self.render_side_by_side(&mut tree, content_y, content_height);
                }
                ViewMode::Unified => {
                    self.render_unified(&mut tree, diff, content_y, content_height);
                }
                ViewMode::Inline => {
                    self.render_inline(&mut tree, content_y, content_height);
                }
            }

            tree.push(RenderCommand::PopClip);
        } else {
            // No diff loaded — show placeholder
            self.render_empty_state(&mut tree, content_y, content_height);
        }

        if self.search.visible {
            self.render_search_bar(&mut tree);
        }

        self.render_status_bar(&mut tree);

        tree
    }

    /// Render the toolbar area.
    fn render_toolbar(&self, tree: &mut RenderTree) {
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: TOOLBAR_HEIGHT,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let mut btn_x: f32 = 8.0;
        let btn_y: f32 = 6.0;
        let btn_h: f32 = 26.0;

        // View mode buttons
        self.render_view_mode_buttons(tree, &mut btn_x, btn_y, btn_h);

        // Separator
        btn_x += 8.0;
        tree.push(RenderCommand::Line {
            x1: btn_x,
            y1: 8.0,
            x2: btn_x,
            y2: TOOLBAR_HEIGHT - 8.0,
            color: colors::SURFACE1,
            width: 1.0,
        });
        btn_x += 14.0;

        // Navigation buttons
        self.render_nav_buttons(tree, &mut btn_x, btn_y, btn_h);

        // Separator
        btn_x += 8.0;
        tree.push(RenderCommand::Line {
            x1: btn_x,
            y1: 8.0,
            x2: btn_x,
            y2: TOOLBAR_HEIGHT - 8.0,
            color: colors::SURFACE1,
            width: 1.0,
        });
        btn_x += 14.0;

        // Ignore option toggles
        self.render_ignore_toggles(tree, &mut btn_x, btn_y, btn_h);

        // Sync scroll indicator (right-aligned)
        self.render_sync_indicator(tree, btn_y, btn_h);

        // Toolbar bottom border
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.width,
            y2: TOOLBAR_HEIGHT,
            color: colors::SURFACE0,
            width: 1.0,
        });
    }

    /// Render view mode toggle buttons.
    fn render_view_mode_buttons(
        &self,
        tree: &mut RenderTree,
        btn_x: &mut f32,
        btn_y: f32,
        btn_h: f32,
    ) {
        let modes = [
            (ViewMode::SideBySide, "Side-by-Side"),
            (ViewMode::Unified, "Unified"),
            (ViewMode::Inline, "Inline"),
        ];

        for (mode, label) in &modes {
            let btn_w = text::width(label, UI_FONT_SIZE) + 16.0;
            let is_active = self.view_mode == *mode;

            tree.push(RenderCommand::FillRect {
                x: *btn_x,
                y: btn_y,
                width: btn_w,
                height: btn_h,
                color: if is_active {
                    colors::SURFACE1
                } else {
                    colors::SURFACE0
                },
                corner_radii: CornerRadii::all(4.0),
            });

            tree.push(RenderCommand::Text {
                x: *btn_x + 8.0,
                y: btn_y + 7.0,
                text: (*label).to_string(),
                color: if is_active {
                    colors::BLUE
                } else {
                    colors::TEXT
                },
                font_size: UI_FONT_SIZE,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            *btn_x += btn_w + 6.0;
        }
    }

    /// Render navigation buttons.
    // Kept as a `&self` method for consistency with the rest of the
    // `render_*` toolbar family, several of which do read `self`.
    #[allow(clippy::unused_self)]
    fn render_nav_buttons(&self, tree: &mut RenderTree, btn_x: &mut f32, btn_y: f32, btn_h: f32) {
        let nav_buttons = [("Prev", "F7"), ("Next", "F8")];
        for (label, shortcut) in &nav_buttons {
            let full_label = format!("{label} ({shortcut})");
            let btn_w = text::width(&full_label, UI_FONT_SIZE) + 16.0;

            tree.push(RenderCommand::FillRect {
                x: *btn_x,
                y: btn_y,
                width: btn_w,
                height: btn_h,
                color: colors::SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            tree.push(RenderCommand::Text {
                x: *btn_x + 8.0,
                y: btn_y + 7.0,
                text: full_label,
                color: colors::TEXT,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            *btn_x += btn_w + 6.0;
        }
    }

    /// Render ignore option toggle buttons.
    fn render_ignore_toggles(
        &self,
        tree: &mut RenderTree,
        btn_x: &mut f32,
        btn_y: f32,
        btn_h: f32,
    ) {
        let ignore_toggles = [
            ("WS", self.ignore_opts.ignore_whitespace),
            ("Case", self.ignore_opts.ignore_case),
            ("Blank", self.ignore_opts.ignore_blank_lines),
        ];
        for (label, active) in &ignore_toggles {
            let btn_w = text::width(label, UI_FONT_SIZE) + 16.0;

            tree.push(RenderCommand::FillRect {
                x: *btn_x,
                y: btn_y,
                width: btn_w,
                height: btn_h,
                color: if *active {
                    colors::SURFACE1
                } else {
                    colors::SURFACE0
                },
                corner_radii: CornerRadii::all(4.0),
            });

            tree.push(RenderCommand::Text {
                x: *btn_x + 8.0,
                y: btn_y + 7.0,
                text: (*label).to_string(),
                color: if *active {
                    colors::TEAL
                } else {
                    colors::SUBTEXT0
                },
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            *btn_x += btn_w + 6.0;
        }
    }

    /// Render the scroll sync indicator button (right-aligned).
    fn render_sync_indicator(&self, tree: &mut RenderTree, btn_y: f32, btn_h: f32) {
        let sync_label = if self.sync_scroll {
            "Sync: ON"
        } else {
            "Sync: OFF"
        };
        let sync_w = text::width(sync_label, UI_FONT_SIZE) + 16.0;
        let sync_x = self.width - sync_w - 8.0;

        tree.push(RenderCommand::FillRect {
            x: sync_x,
            y: btn_y,
            width: sync_w,
            height: btn_h,
            color: if self.sync_scroll {
                colors::SURFACE1
            } else {
                colors::SURFACE0
            },
            corner_radii: CornerRadii::all(4.0),
        });

        tree.push(RenderCommand::Text {
            x: sync_x + 8.0,
            y: btn_y + 7.0,
            text: sync_label.to_string(),
            color: if self.sync_scroll {
                colors::GREEN
            } else {
                colors::OVERLAY0
            },
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render side-by-side diff view.
    fn render_side_by_side(&self, tree: &mut RenderTree, content_y: f32, content_height: f32) {
        let panel_width = (self.width - SEPARATOR_WIDTH) / 2.0;

        // Left panel header
        render_panel_header(tree, 0.0, content_y, panel_width, &self.left_path);
        // Right panel header
        render_panel_header(
            tree,
            panel_width + SEPARATOR_WIDTH,
            content_y,
            panel_width,
            &self.right_path,
        );

        let header_h = LINE_HEIGHT;
        let lines_y = content_y + header_h;

        // Separator line
        tree.push(RenderCommand::FillRect {
            x: panel_width,
            y: content_y,
            width: SEPARATOR_WIDTH,
            height: content_height,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Render visible lines
        let pairs = &self.sbs.pairs;

        // File content only. The panel headers above and the scrollbars below
        // are proportional chrome; the lines between them are a grid stepped
        // by `char_width()`, and have to be drawn in the face that measured it.
        tree.push(RenderCommand::PushFont {
            family: FontFamily::Mono,
        });

        let current = self.search.current().copied();
        for (vi, pair_idx) in
            visible_range(self.scroll_left, content_height, pairs.len()).enumerate()
        {
            let y = lines_y + vi as f32 * LINE_HEIGHT;
            let Some(pair) = pairs.get(pair_idx) else {
                continue;
            };
            // A pair's two halves can come from two different edits -- that is
            // what a paired delete+insert is -- so each side looks up its own.
            let left_matches = pair
                .left_edit
                .map_or(&[][..], |e| self.search.matches_on_edit(e));
            let right_matches = pair
                .right_edit
                .map_or(&[][..], |e| self.search.matches_on_edit(e));
            // Left side
            render_diff_line(
                tree,
                &DiffLineParams {
                    x: 0.0,
                    y,
                    width: panel_width,
                    line_num: pair.left_line,
                    text: pair.left_text.as_deref(),
                    op: pair.left_op,
                    search: SearchOverlay {
                        matches: left_matches,
                        panel: 0,
                        current,
                    },
                },
            );
            // Right side
            render_diff_line(
                tree,
                &DiffLineParams {
                    x: panel_width + SEPARATOR_WIDTH,
                    y,
                    width: panel_width,
                    line_num: pair.right_line,
                    text: pair.right_text.as_deref(),
                    op: pair.right_op,
                    search: SearchOverlay {
                        matches: right_matches,
                        panel: 1,
                        current,
                    },
                },
            );
        }

        tree.push(RenderCommand::PopFont);

        // Scrollbars
        self.render_scrollbar(
            tree,
            panel_width - 8.0,
            lines_y,
            content_height - header_h,
            self.scroll_left,
            pairs.len() as f32,
        );
        self.render_scrollbar(
            tree,
            self.width - 8.0,
            lines_y,
            content_height - header_h,
            self.scroll_right,
            pairs.len() as f32,
        );
    }

    /// Render unified diff view.
    fn render_unified(
        &self,
        tree: &mut RenderTree,
        diff: &DiffResult,
        content_y: f32,
        content_height: f32,
    ) {
        // File content is a grid stepped by `char_width()`; the scrollbar below
        // is not, so the scope closes before it.
        tree.push(RenderCommand::PushFont {
            family: FontFamily::Mono,
        });

        let current = self.search.current().copied();
        for (vi, edit_idx) in
            visible_range(self.scroll_left, content_height, diff.edits.len()).enumerate()
        {
            let y = content_y + vi as f32 * LINE_HEIGHT;
            if let Some(edit) = diff.edits.get(edit_idx) {
                let overlay = SearchOverlay {
                    matches: self.search.matches_on_edit(edit_idx),
                    panel: canonical_panel(edit.op),
                    current,
                };
                self.render_unified_line(tree, y, edit, overlay);
            }
        }

        tree.push(RenderCommand::PopFont);

        // Scrollbar
        self.render_scrollbar(
            tree,
            self.width - 8.0,
            content_y,
            content_height,
            self.scroll_left,
            diff.edits.len() as f32,
        );
    }

    /// Render a single unified diff line.
    fn render_unified_line(
        &self,
        tree: &mut RenderTree,
        y: f32,
        edit: &DiffEdit,
        search: SearchOverlay<'_>,
    ) {
        let (bg_color, prefix, text_color) = match edit.op {
            DiffOp::Equal => (colors::BASE, " ", colors::TEXT),
            DiffOp::Insert => (colors::ADD_BG, "+", colors::GREEN),
            DiffOp::Delete => (colors::DEL_BG, "-", colors::RED),
        };

        // Background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: LINE_HEIGHT,
            color: bg_color,
            corner_radii: CornerRadii::ZERO,
        });

        // Left line number
        if let Some(ln) = edit.left_line {
            let ln_text = format!("{}", ln.saturating_add(1));
            tree.push(RenderCommand::Text {
                x: PANEL_PADDING,
                y: y + 3.0,
                text: ln_text,
                color: colors::OVERLAY0,
                font_size: CONTENT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(GUTTER_WIDTH - 4.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Right line number
        if let Some(rn) = edit.right_line {
            let rn_text = format!("{}", rn.saturating_add(1));
            tree.push(RenderCommand::Text {
                x: GUTTER_WIDTH + PANEL_PADDING,
                y: y + 3.0,
                text: rn_text,
                color: colors::OVERLAY0,
                font_size: CONTENT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(GUTTER_WIDTH - 4.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Prefix
        let prefix_x = GUTTER_WIDTH * 2.0 + PANEL_PADDING;
        tree.push(RenderCommand::Text {
            x: prefix_x,
            y: y + 3.0,
            text: prefix.to_string(),
            color: text_color,
            font_size: CONTENT_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Text content
        let text_x = prefix_x + char_width() * 2.0;
        render_search_highlights(tree, text_x, y, &edit.text, search);
        tree.push(RenderCommand::Text {
            x: text_x,
            y: y + 3.0,
            text: edit.text.clone(),
            color: text_color,
            font_size: CONTENT_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - text_x - 12.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render inline diff view with character-level highlighting.
    fn render_inline(&self, tree: &mut RenderTree, content_y: f32, content_height: f32) {
        // `build_inline_rows` emits exactly one row per edit, which is why the
        // row index below doubles as the edit index the search matches are
        // filed under. Side-by-side is the view where that stops being true.
        let inline_rows = &self.inline_rows;

        // The character-level highlight rectangles are sized and placed by
        // measuring each span in `FontFamily::Mono`, so the runs they sit
        // behind have to be *drawn* in that family too, or the highlight slides
        // off the change it marks — the whole point of the inline view. This
        // push is what makes the measurement and the drawing the same question.
        tree.push(RenderCommand::PushFont {
            family: FontFamily::Mono,
        });

        let current = self.search.current().copied();
        for (vi, row_idx) in
            visible_range(self.scroll_left, content_height, inline_rows.len()).enumerate()
        {
            let y = content_y + vi as f32 * LINE_HEIGHT;
            if let Some(row) = inline_rows.get(row_idx) {
                let overlay = SearchOverlay {
                    matches: self.search.matches_on_edit(row_idx),
                    panel: canonical_panel(row.op),
                    current,
                };
                self.render_inline_row(tree, y, row, overlay);
            }
        }

        tree.push(RenderCommand::PopFont);

        // Scrollbar
        self.render_scrollbar(
            tree,
            self.width - 8.0,
            content_y,
            content_height,
            self.scroll_left,
            inline_rows.len() as f32,
        );
    }

    /// Render a single inline row with character-level highlights.
    fn render_inline_row(
        &self,
        tree: &mut RenderTree,
        y: f32,
        row: &InlineRow,
        search: SearchOverlay<'_>,
    ) {
        let bg_color = match row.op {
            DiffOp::Equal => colors::BASE,
            DiffOp::Insert => colors::ADD_BG,
            DiffOp::Delete => colors::DEL_BG,
        };

        // Background
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: LINE_HEIGHT,
            color: bg_color,
            corner_radii: CornerRadii::ZERO,
        });

        // Line number
        if let Some(ln) = row.line_num {
            let ln_text = format!("{}", ln.saturating_add(1));
            tree.push(RenderCommand::Text {
                x: PANEL_PADDING,
                y: y + 3.0,
                text: ln_text,
                color: colors::OVERLAY0,
                font_size: CONTENT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(GUTTER_WIDTH - 4.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Prefix
        let (prefix, prefix_color) = match row.op {
            DiffOp::Equal => (" ", colors::TEXT),
            DiffOp::Insert => ("+", colors::GREEN),
            DiffOp::Delete => ("-", colors::RED),
        };
        tree.push(RenderCommand::Text {
            x: GUTTER_WIDTH + PANEL_PADDING,
            y: y + 3.0,
            text: prefix.to_string(),
            color: prefix_color,
            font_size: CONTENT_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Render text with inline highlights.
        //
        // The search boxes go down first, under both the character-level change
        // spans and the text: a search hit inside a changed run has to leave
        // the run's own colour visible, or finding a word would erase the
        // reason it was interesting.
        let text_x = GUTTER_WIDTH + PANEL_PADDING + char_width() * 2.0;
        render_search_highlights(tree, text_x, y, &row.text, search);
        if row.spans.is_empty() {
            tree.push(RenderCommand::Text {
                x: text_x,
                y: y + 3.0,
                text: row.text.clone(),
                color: prefix_color,
                font_size: CONTENT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - text_x - 12.0),
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            self.render_inline_spans(tree, text_x, y, row);
        }
    }

    /// Render character-level spans for an inline row.
    // Kept as a `&self` method for consistency with the rest of the
    // `render_*` row family, several of which do read `self`.
    #[allow(clippy::unused_self)]
    fn render_inline_spans(&self, tree: &mut RenderTree, text_x: f32, y: f32, row: &InlineRow) {
        let mut char_offset: f32 = 0.0;
        for span in &row.spans {
            let span_text = row.text.get(span.start..span.end).unwrap_or("");
            if span_text.is_empty() {
                continue;
            }

            let weight = if span.changed {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            };
            // What this span will actually be drawn as, rather than a nominal
            // cell count. The two are equal only where every character advances
            // the cell width — true of Latin source in a mono face, and not
            // true of a tab (four cells, one `char`), a CJK ideograph, a
            // combining mark, or anything the face substitutes. Where they
            // disagreed the highlight rectangle was drawn a different width
            // from the text it marks and the pen stepped to the wrong place
            // for the next span, so on a tab-indented line — which is most
            // source — the highlight slid off the change it exists to point at.
            let span_w = text::measure_in(span_text, CONTENT_FONT_SIZE, weight, FontFamily::Mono);

            if span.changed {
                let highlight_color = match row.op {
                    DiffOp::Insert => colors::ADD_LINE_BG,
                    DiffOp::Delete => colors::DEL_LINE_BG,
                    DiffOp::Equal => colors::BASE,
                };
                tree.push(RenderCommand::FillRect {
                    x: text_x + char_offset,
                    y,
                    width: span_w,
                    height: LINE_HEIGHT,
                    color: highlight_color,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            tree.push(RenderCommand::Text {
                x: text_x + char_offset,
                y: y + 3.0,
                text: span_text.to_string(),
                color: if span.changed {
                    match row.op {
                        DiffOp::Insert => colors::GREEN,
                        DiffOp::Delete => colors::RED,
                        DiffOp::Equal => colors::TEXT,
                    }
                } else {
                    colors::TEXT
                },
                font_size: CONTENT_FONT_SIZE,
                font_weight: weight,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            char_offset += span_w;
        }
    }

    /// Render the empty state when no files are loaded.
    fn render_empty_state(&self, tree: &mut RenderTree, y: f32, height: f32) {
        let center_x = self.width / 2.0;
        let center_y = y + height / 2.0;

        tree.push(RenderCommand::Text {
            x: center_x - 120.0,
            y: center_y - 30.0,
            text: "File Diff/Compare Tool".to_string(),
            color: colors::TEXT,
            font_size: 20.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        tree.push(RenderCommand::Text {
            x: center_x - 140.0,
            y: center_y + 10.0,
            text: "Open two files to compare them".to_string(),
            color: colors::SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render directory comparison view.
    fn render_dir_compare(&self, tree: &mut RenderTree, y: f32, height: f32) {
        let Some(result) = &self.dir_compare else {
            return;
        };

        // Header
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: LINE_HEIGHT,
            color: colors::CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let summary = format!(
            "Directory Compare: {} same, {} different, {} left only, {} right only",
            result.same_count,
            result.different_count,
            result.only_left_count,
            result.only_right_count,
        );
        tree.push(RenderCommand::Text {
            x: PANEL_PADDING + 4.0,
            y: y + 3.0,
            text: summary,
            color: colors::SUBTEXT1,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Entries
        let list_y = y + LINE_HEIGHT;
        for (vi, entry_idx) in
            visible_range(self.dir_scroll, height, result.entries.len()).enumerate()
        {
            let ey = list_y + vi as f32 * LINE_HEIGHT;
            if let Some(entry) = result.entries.get(entry_idx) {
                render_dir_entry(tree, ey, entry);
            }
        }
    }

    /// Render the search bar overlay.
    fn render_search_bar(&self, tree: &mut RenderTree) {
        let bar_h: f32 = 36.0;
        let bar_y = TOOLBAR_HEIGHT;
        let bar_w = 400.0f32.min(self.width - 20.0);
        let bar_x = self.width - bar_w - 10.0;

        // Background
        tree.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Border
        tree.push(RenderCommand::StrokeRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: colors::BLUE,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Search label
        tree.push(RenderCommand::Text {
            x: bar_x + 8.0,
            y: bar_y + 10.0,
            text: "Find:".to_string(),
            color: colors::SUBTEXT0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Query text
        if !self.search.query.is_empty() {
            tree.push(RenderCommand::Text {
                x: bar_x + 48.0,
                y: bar_y + 10.0,
                text: self.search.query.clone(),
                color: colors::TEXT,
                font_size: CONTENT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(bar_w - 140.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Match count
        let match_info = if self.search.matches.is_empty() {
            "No matches".to_string()
        } else {
            format!(
                "{}/{}",
                self.search.current_match.saturating_add(1),
                self.search.matches.len()
            )
        };
        tree.push(RenderCommand::Text {
            x: bar_x + bar_w - 80.0,
            y: bar_y + 10.0,
            text: match_info,
            color: colors::SUBTEXT0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render the status bar at the bottom.
    fn render_status_bar(&self, tree: &mut RenderTree) {
        let y = self.height - STATUS_BAR_HEIGHT;

        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: STATUS_BAR_HEIGHT,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.width,
            y2: y,
            color: colors::SURFACE0,
            width: 1.0,
        });

        let mut text_x: f32 = 10.0;
        let text_y = y + 7.0;

        // View mode
        tree.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: format!("{}", self.view_mode),
            color: colors::BLUE,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        text_x += 100.0;

        if self.diff.is_some() {
            self.render_status_stats(tree, &mut text_x, text_y);
        }
    }

    /// Render statistics section of the status bar.
    fn render_status_stats(&self, tree: &mut RenderTree, text_x: &mut f32, text_y: f32) {
        // Change navigation position
        let change_info = if self.change_indices.is_empty() {
            "No changes".to_string()
        } else {
            format!(
                "Change {}/{} ",
                self.current_change_index.saturating_add(1),
                self.change_indices.len()
            )
        };
        tree.push(RenderCommand::Text {
            x: *text_x,
            y: text_y,
            text: change_info,
            color: colors::PEACH,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        *text_x += 130.0;

        // Line stats
        let stats_text = format!(
            "+{} -{} ~{:.0}%",
            self.stats.inserted_lines, self.stats.deleted_lines, self.stats.similarity,
        );
        tree.push(RenderCommand::Text {
            x: *text_x,
            y: text_y,
            text: stats_text,
            color: colors::TEXT,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        *text_x += 140.0;

        // Left/right totals
        let totals_text = format!("L:{} R:{}", self.stats.left_total, self.stats.right_total);
        tree.push(RenderCommand::Text {
            x: *text_x,
            y: text_y,
            text: totals_text,
            color: colors::SUBTEXT0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Merge status (right-aligned)
        if let Some(ref merge) = self.merge {
            let total_hunks = merge.decisions.len();
            if total_hunks > 0 {
                let decided = merge.decided_count();
                let merge_text = format!("Merge: {decided}/{total_hunks}");
                tree.push(RenderCommand::Text {
                    x: text::right_x(
                        &merge_text,
                        self.width - 16.0,
                        UI_FONT_SIZE,
                        FontWeightHint::Regular,
                    ),
                    y: text_y,
                    text: merge_text,
                    color: if decided == total_hunks {
                        colors::GREEN
                    } else {
                        colors::YELLOW
                    },
                    font_size: UI_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }
    }

    /// Render a scrollbar track and thumb.
    fn render_scrollbar(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        height: f32,
        scroll_pos: f32,
        total_lines: f32,
    ) {
        if total_lines <= 0.0 {
            return;
        }
        let visible = self.visible_line_count();
        if visible >= total_lines {
            return;
        }

        let track_w: f32 = 6.0;

        // Track
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: track_w,
            height,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });

        // Thumb
        let ratio = visible / total_lines;
        let thumb_h = (height * ratio).max(20.0);
        let scroll_ratio = if total_lines > visible {
            scroll_pos / (total_lines - visible)
        } else {
            0.0
        };
        let thumb_y = y + scroll_ratio * (height - thumb_h);

        tree.push(RenderCommand::FillRect {
            x,
            y: thumb_y,
            width: track_w,
            height: thumb_h,
            color: colors::SURFACE2,
            corner_radii: CornerRadii::all(3.0),
        });
    }
}

// ============================================================================
// Free functions for rendering (avoid unused_self and too_many_arguments)
// ============================================================================

/// Render a panel header with file path.
fn render_panel_header(tree: &mut RenderTree, x: f32, y: f32, width: f32, path: &str) {
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: LINE_HEIGHT,
        color: colors::CRUST,
        corner_radii: CornerRadii::ZERO,
    });

    let display_path = if path.is_empty() { "(no file)" } else { path };
    tree.push(RenderCommand::Text {
        x: x + PANEL_PADDING + 4.0,
        y: y + 3.0,
        text: display_path.to_string(),
        color: colors::SUBTEXT1,
        font_size: UI_FONT_SIZE,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 12.0),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Draw the search-match rectangles that fall on one line.
///
/// This is the half of find-in-diff that was missing. `SearchState` computed
/// matches, the status bar counted them ("4/17"), Enter and F3 advanced the
/// counter -- and nothing was ever drawn and the view never moved, so on any
/// file longer than one screen the whole feature was a number that changed. The
/// byte offsets had even been carefully corrected (see the comment in
/// [`SearchState::search`]) for the benefit of a highlighter that did not
/// exist.
///
/// `text_x` is where the line's first character is drawn and `text` is the
/// exact string drawn there, because a match's offsets are byte offsets into
/// *that* string and nothing else.
fn render_search_highlights(
    tree: &mut RenderTree,
    text_x: f32,
    y: f32,
    text: &str,
    overlay: SearchOverlay<'_>,
) {
    for m in overlay.matches {
        if m.panel != overlay.panel {
            continue;
        }
        // `get` rather than `&text[..n]`: an offset that is not on a character
        // boundary panics, and a stale match -- one computed against a diff
        // that has since been recomputed -- should draw nothing rather than
        // take the window down. `textfind` returns boundaries, but the
        // highlighter does not have to depend on knowing that.
        let Some(before) = text.get(..m.byte_offset) else {
            continue;
        };
        let Some(match_end) = m.byte_offset.checked_add(m.match_len) else {
            continue;
        };
        let Some(matched) = text.get(m.byte_offset..match_end) else {
            continue;
        };
        // Measured, not counted. Bytes were the first version of this bug and
        // characters were the second: `before.len()` put the box wrong on every
        // line holding a multi-byte character, and `before.chars().count()`
        // put it wrong on every line holding a character the face does not
        // advance one cell for -- a tab above all, which is how most source is
        // indented, and which the face draws four cells wide. The width the
        // renderer will use is the only width a highlight box can be right at.
        tree.push(RenderCommand::FillRect {
            x: text_x
                + text::measure_in(
                    before,
                    CONTENT_FONT_SIZE,
                    FontWeightHint::Regular,
                    FontFamily::Mono,
                ),
            y,
            width: text::measure_in(
                matched,
                CONTENT_FONT_SIZE,
                FontWeightHint::Regular,
                FontFamily::Mono,
            ),
            height: LINE_HEIGHT,
            color: if overlay.current == Some(*m) {
                colors::SEARCH_CURRENT_BG
            } else {
                colors::SEARCH_BG
            },
            corner_radii: CornerRadii::ZERO,
        });
    }
}

/// Render a single diff line (used in side-by-side mode).
fn render_diff_line(tree: &mut RenderTree, params: &DiffLineParams<'_>) {
    let bg_color = match params.op {
        Some(DiffOp::Insert) => colors::ADD_BG,
        Some(DiffOp::Delete) => colors::DEL_BG,
        Some(DiffOp::Equal) | None => colors::BASE,
    };

    // Background
    tree.push(RenderCommand::FillRect {
        x: params.x,
        y: params.y,
        width: params.width,
        height: LINE_HEIGHT,
        color: bg_color,
        corner_radii: CornerRadii::ZERO,
    });

    // Gutter separator
    tree.push(RenderCommand::Line {
        x1: params.x + GUTTER_WIDTH,
        y1: params.y,
        x2: params.x + GUTTER_WIDTH,
        y2: params.y + LINE_HEIGHT,
        color: colors::SURFACE0,
        width: 1.0,
    });

    // Line number
    if let Some(ln) = params.line_num {
        let ln_text = format!("{}", ln.saturating_add(1));
        tree.push(RenderCommand::Text {
            x: text::right_x(
                &ln_text,
                params.x + GUTTER_WIDTH - 4.0,
                CONTENT_FONT_SIZE,
                FontWeightHint::Regular,
            ),
            y: params.y + 3.0,
            text: ln_text,
            color: colors::OVERLAY0,
            font_size: CONTENT_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(GUTTER_WIDTH - 4.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    // Text
    if let Some(text) = params.text {
        let text_color = match params.op {
            Some(DiffOp::Insert) => colors::GREEN,
            Some(DiffOp::Delete) => colors::RED,
            _ => colors::TEXT,
        };

        // Before the text, so the boxes are behind it.
        render_search_highlights(
            tree,
            params.x + GUTTER_WIDTH + PANEL_PADDING,
            params.y,
            text,
            params.search,
        );

        tree.push(RenderCommand::Text {
            x: params.x + GUTTER_WIDTH + PANEL_PADDING,
            y: params.y + 3.0,
            text: text.to_string(),
            color: text_color,
            font_size: CONTENT_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(params.width - GUTTER_WIDTH - PANEL_PADDING - 12.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

/// Render a single directory comparison entry.
fn render_dir_entry(tree: &mut RenderTree, ey: f32, entry: &DirCompareEntry) {
    let (status_color, status_text) = match entry.status {
        FileCompareStatus::Same => (colors::GREEN, "Same"),
        FileCompareStatus::Different => (colors::YELLOW, "Diff"),
        FileCompareStatus::OnlyLeft => (colors::RED, "Left"),
        FileCompareStatus::OnlyRight => (colors::BLUE, "Right"),
    };

    // Status indicator
    tree.push(RenderCommand::FillRect {
        x: PANEL_PADDING,
        y: ey + 2.0,
        width: 4.0,
        height: LINE_HEIGHT - 4.0,
        color: status_color,
        corner_radii: CornerRadii::all(2.0),
    });

    // Status text
    tree.push(RenderCommand::Text {
        x: 14.0,
        y: ey + 3.0,
        text: status_text.to_string(),
        color: status_color,
        font_size: CONTENT_FONT_SIZE,
        font_weight: FontWeightHint::Bold,
        max_width: Some(50.0),
        overflow: TextOverflow::Ellipsis,
    });

    // File path
    tree.push(RenderCommand::Text {
        x: 70.0,
        y: ey + 3.0,
        text: entry.path.clone(),
        color: colors::TEXT,
        font_size: CONTENT_FONT_SIZE,
        font_weight: FontWeightHint::Regular,
        max_width: Some(1200.0),
        overflow: TextOverflow::Ellipsis,
    });
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let mut app = FileDiffApp::new();

    // Demo: load sample files for testing
    let left = "fn main() {\n    println!(\"Hello, world!\");\n    let x = 42;\n}\n";
    let right =
        "fn main() {\n    println!(\"Hello, Slate OS!\");\n    let x = 42;\n    let y = 100;\n}\n";
    app.load_files("left.rs", left, "right.rs", right);

    let _tree = app.render();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not. Nor is
    // exact float comparison a hazard in a test that asserts a *computed*
    // layout offset equals the constant it was built from: an epsilon there
    // would weaken the assertion rather than strengthen it.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;
    use diffcore::myers_diff;

    // --- Myers diff tests ---

    #[test]
    fn test_diff_empty_both() {
        let result = myers_diff(&[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_diff_empty_left() {
        let right = ["a", "b"];
        let result = myers_diff(&[], &right);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].op, DiffOp::Insert);
        assert_eq!(result[1].op, DiffOp::Insert);
    }

    #[test]
    fn test_diff_empty_right() {
        let left = ["a", "b"];
        let result = myers_diff(&left, &[]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].op, DiffOp::Delete);
        assert_eq!(result[1].op, DiffOp::Delete);
    }

    #[test]
    fn test_diff_identical() {
        let lines = ["hello", "world"];
        let result = myers_diff(&lines, &lines);
        assert_eq!(result.len(), 2);
        for edit in &result {
            assert_eq!(edit.op, DiffOp::Equal);
        }
    }

    #[test]
    fn test_diff_single_insert() {
        let left = ["a", "c"];
        let right = ["a", "b", "c"];
        let result = myers_diff(&left, &right);
        let ops: Vec<DiffOp> = result.iter().map(|e| e.op).collect();
        assert!(ops.contains(&DiffOp::Insert));
        assert!(ops.contains(&DiffOp::Equal));
    }

    #[test]
    fn test_diff_single_delete() {
        let left = ["a", "b", "c"];
        let right = ["a", "c"];
        let result = myers_diff(&left, &right);
        let ops: Vec<DiffOp> = result.iter().map(|e| e.op).collect();
        assert!(ops.contains(&DiffOp::Delete));
        assert!(ops.contains(&DiffOp::Equal));
    }

    #[test]
    fn test_diff_complete_replacement() {
        let left = ["a", "b"];
        let right = ["c", "d"];
        let result = myers_diff(&left, &right);
        let del_count = result.iter().filter(|e| e.op == DiffOp::Delete).count();
        let ins_count = result.iter().filter(|e| e.op == DiffOp::Insert).count();
        assert_eq!(del_count, 2);
        assert_eq!(ins_count, 2);
    }

    #[test]
    fn test_diff_line_numbers_correct() {
        let left = ["a", "b", "c"];
        let right = ["a", "c"];
        let result = myers_diff(&left, &right);
        for edit in &result {
            match edit.op {
                DiffOp::Equal => {
                    assert!(edit.left_line.is_some());
                    assert!(edit.right_line.is_some());
                }
                DiffOp::Delete => {
                    assert!(edit.left_line.is_some());
                    assert!(edit.right_line.is_none());
                }
                DiffOp::Insert => {
                    assert!(edit.left_line.is_none());
                    assert!(edit.right_line.is_some());
                }
            }
        }
    }

    #[test]
    fn test_diff_preserves_text() {
        let left = ["hello", "world"];
        let right = ["hello", "rust"];
        let result = myers_diff(&left, &right);
        let texts: Vec<&str> = result.iter().map(|e| e.text.as_str()).collect();
        assert!(texts.contains(&"hello"));
        assert!(texts.contains(&"world") || texts.contains(&"rust"));
    }

    #[test]
    fn test_diff_preserves_order() {
        let left = ["a", "b", "c", "d"];
        let right = ["a", "x", "c", "d"];
        let result = myers_diff(&left, &right);
        assert_eq!(result[0].op, DiffOp::Equal);
        assert_eq!(result[0].text, "a");
    }

    // --- Compute diff tests ---

    #[test]
    fn test_compute_diff_basic() {
        let left = "hello\nworld\n";
        let right = "hello\nrust\n";
        let opts = IgnoreOptions::default();
        let result = compute_diff(left, right, &opts);
        assert!(!result.edits.is_empty());
        assert_eq!(result.left_line_count, 2);
        assert_eq!(result.right_line_count, 2);
    }

    #[test]
    fn test_compute_diff_ignore_case() {
        let left = "Hello\nWorld";
        let right = "hello\nworld";
        let opts = IgnoreOptions {
            ignore_case: true,
            ..Default::default()
        };
        let result = compute_diff(left, right, &opts);
        let equal_count = result
            .edits
            .iter()
            .filter(|e| e.op == DiffOp::Equal)
            .count();
        assert_eq!(equal_count, 2);
    }

    #[test]
    fn test_compute_diff_ignore_whitespace() {
        let left = "hello   world\n";
        let right = "hello world\n";
        let opts = IgnoreOptions {
            ignore_whitespace: true,
            ..Default::default()
        };
        let result = compute_diff(left, right, &opts);
        let equal_count = result
            .edits
            .iter()
            .filter(|e| e.op == DiffOp::Equal)
            .count();
        assert_eq!(equal_count, 1);
    }

    #[test]
    fn test_compute_diff_ignore_blank_lines() {
        let left = "a\n\nb";
        let right = "a\nb";
        let opts = IgnoreOptions {
            ignore_blank_lines: true,
            ..Default::default()
        };
        let result = compute_diff(left, right, &opts);
        let change_count = result
            .edits
            .iter()
            .filter(|e| e.op != DiffOp::Equal)
            .count();
        assert!(change_count <= 1);
    }

    #[test]
    fn test_compute_diff_empty_both() {
        let opts = IgnoreOptions::default();
        let result = compute_diff("", "", &opts);
        assert!(result.edits.is_empty());
        assert_eq!(result.left_line_count, 0);
        assert_eq!(result.right_line_count, 0);
    }

    // --- Hunk grouping tests ---

    #[test]
    fn test_hunk_grouping_single_change() {
        let left = "a\nb\nc\nd\ne";
        let right = "a\nx\nc\nd\ne";
        let opts = IgnoreOptions::default();
        let result = compute_diff(left, right, &opts);
        assert!(!result.hunks.is_empty());
    }

    #[test]
    fn test_hunk_grouping_multiple_changes() {
        // The two changes (line 2 and line 13) are separated by 10 unchanged
        // lines, which exceeds 2*context (=6) so they form two distinct hunks.
        // (Changes closer than that are correctly merged into one hunk, matching
        // `diff -U3` semantics.)
        let left = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn";
        let right = "a\nB\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nM\nn";
        let opts = IgnoreOptions::default();
        let result = compute_diff(left, right, &opts);
        assert!(result.hunks.len() >= 2);
    }

    #[test]
    fn test_hunk_no_changes() {
        let text = "a\nb\nc";
        let opts = IgnoreOptions::default();
        let result = compute_diff(text, text, &opts);
        assert!(result.hunks.is_empty());
    }

    // --- Inline diff tests ---

    #[test]
    fn test_inline_diff_identical() {
        let (left, right) = inline_diff("hello", "hello");
        for span in &left {
            assert!(!span.changed);
        }
        for span in &right {
            assert!(!span.changed);
        }
    }

    #[test]
    fn test_inline_diff_single_char_change() {
        let (left, right) = inline_diff("abc", "axc");
        assert!(left.iter().any(|s| s.changed));
        assert!(right.iter().any(|s| s.changed));
    }

    #[test]
    fn test_inline_diff_prefix_preserved() {
        let (left, right) = inline_diff("hello world", "hello rust");
        if let Some(first) = left.first() {
            assert!(!first.changed);
            assert!(first.end > 0);
        }
        if let Some(first) = right.first() {
            assert!(!first.changed);
            assert!(first.end > 0);
        }
    }

    #[test]
    fn test_inline_diff_empty_both() {
        let (left, right) = inline_diff("", "");
        assert!(left.is_empty());
        assert!(right.is_empty());
    }

    #[test]
    fn test_inline_diff_one_empty() {
        let (left, right) = inline_diff("hello", "");
        assert!(left.iter().any(|s| s.changed));
        assert!(right.is_empty());
    }

    // --- DiffStats tests ---

    #[test]
    fn test_stats_from_identical_files() {
        let text = "a\nb\nc";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(text, text, &opts);
        let stats = DiffStats::from_diff(&diff);
        assert_eq!(stats.equal_lines, 3);
        assert_eq!(stats.inserted_lines, 0);
        assert_eq!(stats.deleted_lines, 0);
        assert!((stats.similarity - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_stats_completely_different() {
        let left = "a\nb";
        let right = "c\nd";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let stats = DiffStats::from_diff(&diff);
        assert_eq!(stats.inserted_lines, 2);
        assert_eq!(stats.deleted_lines, 2);
        assert!(stats.similarity < 50.0);
    }

    #[test]
    fn test_stats_change_count() {
        let left = "a\nb\nc";
        let right = "a\nx\nc";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let stats = DiffStats::from_diff(&diff);
        assert!(stats.change_count() > 0);
    }

    #[test]
    fn test_stats_empty_diff() {
        let opts = IgnoreOptions::default();
        let diff = compute_diff("", "", &opts);
        let stats = DiffStats::from_diff(&diff);
        assert!((stats.similarity - 100.0).abs() < f32::EPSILON);
    }

    // --- Search tests ---

    #[test]
    fn test_search_finds_matches() {
        let left = "hello world\nfoo bar";
        let right = "hello earth\nfoo baz";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut search = SearchState {
            query: "foo".to_string(),
            ..Default::default()
        };
        search.search(&diff.edits);
        assert!(!search.matches.is_empty());
    }

    #[test]
    fn test_search_case_insensitive() {
        let left = "Hello World";
        let right = "hello world";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut search = SearchState {
            query: "hello".to_string(),
            case_sensitive: false,
            ..Default::default()
        };
        search.search(&diff.edits);
        assert!(!search.matches.is_empty());
    }

    #[test]
    fn test_search_empty_query() {
        let left = "hello";
        let right = "world";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut search = SearchState::default();
        search.search(&diff.edits);
        assert!(search.matches.is_empty());
    }

    #[test]
    fn test_search_navigation() {
        let left = "aaa\naaa\naaa";
        let right = "aaa\naaa\naaa";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut search = SearchState {
            query: "aaa".to_string(),
            case_sensitive: true,
            ..Default::default()
        };
        search.search(&diff.edits);
        let count = search.matches.len();
        assert!(count > 0);
        search.next_match();
        assert_eq!(search.current_match, 1);
        search.prev_match();
        assert_eq!(search.current_match, 0);
    }

    #[test]
    fn test_search_wraps_around() {
        let left = "abc\ndef";
        let right = "abc\ndef";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut search = SearchState {
            query: "abc".to_string(),
            case_sensitive: true,
            ..Default::default()
        };
        search.search(&diff.edits);
        let count = search.matches.len();
        assert!(count > 0);
        for _ in 0..=count {
            search.next_match();
        }
        assert!(search.current_match < count);
    }

    /// One edit, so a match's offsets can be checked against a known string.
    fn edit(op: DiffOp, text: &str) -> Vec<DiffEdit> {
        vec![DiffEdit {
            op,
            left_line: Some(1),
            right_line: Some(1),
            text: text.to_string(),
        }]
    }

    #[test]
    fn a_match_offset_is_an_offset_into_the_line_the_highlighter_slices() {
        // `İ` (U+0130) is two bytes but folds to three (`i` + a combining dot),
        // so an offset measured in a lower-cased copy of this line is one byte
        // past the truth — enough to slice inside a character and panic the
        // highlighter.
        let mut search = SearchState {
            query: "ABC".to_string(),
            ..Default::default()
        };
        search.search(&edit(DiffOp::Delete, "\u{130}abc"));
        assert_eq!(search.matches.len(), 1);
        let m = &search.matches[0];
        assert_eq!(m.byte_offset, 2);
        assert_eq!(m.match_len, 3);
    }

    #[test]
    fn a_match_is_not_assumed_to_be_as_long_as_the_query() {
        // The query is three bytes (`i` + a combining dot); what it matches is
        // the two-byte `İ`. Taking the length from the query would over-run the
        // character by a byte.
        let mut search = SearchState {
            query: "i\u{307}".to_string(),
            ..Default::default()
        };
        search.search(&edit(DiffOp::Delete, "x\u{130}y"));
        assert_eq!(search.matches.len(), 1);
        let m = &search.matches[0];
        assert_eq!(m.byte_offset, 1);
        assert_eq!(m.match_len, 2);
    }

    #[test]
    fn matches_do_not_overlap_one_another() {
        // Resuming one byte past a match's *start* reported `aa` three times in
        // `aaaa`, so the highlighter painted overlapping runs.
        let mut search = SearchState {
            query: "aa".to_string(),
            case_sensitive: true,
            ..Default::default()
        };
        search.search(&edit(DiffOp::Delete, "aaaa"));
        let spans: Vec<(usize, usize)> = search
            .matches
            .iter()
            .map(|m| (m.byte_offset, m.match_len))
            .collect();
        assert_eq!(spans, vec![(0, 2), (2, 2)]);
    }

    #[test]
    fn an_equal_line_is_highlighted_in_both_panels() {
        let mut search = SearchState {
            query: "b".to_string(),
            case_sensitive: true,
            ..Default::default()
        };
        search.search(&edit(DiffOp::Equal, "abc"));
        assert_eq!(search.matches.len(), 2);
        assert_eq!(search.matches[0].panel, 0);
        assert_eq!(search.matches[1].panel, 1);
        assert!(search.matches.iter().all(|m| m.byte_offset == 1));
    }

    // --- Merge tests ---

    #[test]
    fn test_merge_state_new() {
        let merge = MergeState::new(3);
        assert_eq!(merge.decisions.len(), 3);
        for d in &merge.decisions {
            assert_eq!(*d, MergeDecision::Undecided);
        }
    }

    #[test]
    fn test_merge_set_get_decision() {
        let mut merge = MergeState::new(3);
        merge.set_decision(1, MergeDecision::AcceptLeft);
        assert_eq!(merge.get_decision(1), MergeDecision::AcceptLeft);
        assert_eq!(merge.get_decision(0), MergeDecision::Undecided);
    }

    #[test]
    fn test_merge_decided_count() {
        let mut merge = MergeState::new(4);
        assert_eq!(merge.decided_count(), 0);
        merge.set_decision(0, MergeDecision::AcceptLeft);
        merge.set_decision(2, MergeDecision::AcceptRight);
        assert_eq!(merge.decided_count(), 2);
    }

    #[test]
    fn test_merge_out_of_bounds() {
        let mut merge = MergeState::new(2);
        merge.set_decision(99, MergeDecision::AcceptLeft);
        assert_eq!(merge.get_decision(99), MergeDecision::Undecided);
    }

    #[test]
    fn test_merge_apply_accept_right() {
        let left = "a\nb\nc";
        let right = "a\nX\nc";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut merge = MergeState::new(diff.hunks.len());
        for i in 0..diff.hunks.len() {
            merge.set_decision(i, MergeDecision::AcceptRight);
        }
        let output = merge.apply(&diff);
        assert!(output.contains('X'));
    }

    #[test]
    fn test_merge_apply_accept_left() {
        let left = "a\nb\nc";
        let right = "a\nX\nc";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut merge = MergeState::new(diff.hunks.len());
        for i in 0..diff.hunks.len() {
            merge.set_decision(i, MergeDecision::AcceptLeft);
        }
        let output = merge.apply(&diff);
        assert!(output.contains('b'));
    }

    // --- Directory comparison tests ---

    #[test]
    fn test_dir_compare_identical() {
        let left = [("file.txt", "hello")];
        let right = [("file.txt", "hello")];
        let result = compare_directories(&left, &right);
        assert_eq!(result.same_count, 1);
        assert_eq!(result.different_count, 0);
    }

    #[test]
    fn test_dir_compare_different() {
        let left = [("file.txt", "hello")];
        let right = [("file.txt", "world")];
        let result = compare_directories(&left, &right);
        assert_eq!(result.different_count, 1);
    }

    #[test]
    fn test_dir_compare_only_left() {
        let left = [("a.txt", "x"), ("b.txt", "y")];
        let right = [("a.txt", "x")];
        let result = compare_directories(&left, &right);
        assert_eq!(result.only_left_count, 1);
        assert_eq!(result.same_count, 1);
    }

    #[test]
    fn test_dir_compare_only_right() {
        let left = [("a.txt", "x")];
        let right = [("a.txt", "x"), ("c.txt", "z")];
        let result = compare_directories(&left, &right);
        assert_eq!(result.only_right_count, 1);
    }

    #[test]
    fn test_dir_compare_empty() {
        let result = compare_directories(&[], &[]);
        assert_eq!(result.entries.len(), 0);
    }

    #[test]
    fn test_dir_compare_mixed() {
        let left = [("a.txt", "same"), ("b.txt", "old"), ("d.txt", "only_l")];
        let right = [("a.txt", "same"), ("b.txt", "new"), ("c.txt", "only_r")];
        let result = compare_directories(&left, &right);
        assert_eq!(result.same_count, 1);
        assert_eq!(result.different_count, 1);
        assert_eq!(result.only_left_count, 1);
        assert_eq!(result.only_right_count, 1);
    }

    // --- View mode tests ---

    #[test]
    fn test_view_mode_display() {
        assert_eq!(format!("{}", ViewMode::SideBySide), "Side-by-Side");
        assert_eq!(format!("{}", ViewMode::Unified), "Unified");
        assert_eq!(format!("{}", ViewMode::Inline), "Inline");
    }

    // --- App state tests ---

    #[test]
    fn test_app_new_defaults() {
        let app = FileDiffApp::new();
        assert!(app.diff.is_none());
        assert!(app.sync_scroll);
        assert_eq!(app.view_mode, ViewMode::SideBySide);
        assert!(app.scroll_left.abs() < f32::EPSILON);
    }

    #[test]
    fn test_app_load_files() {
        let mut app = FileDiffApp::new();
        app.load_files("left.txt", "hello\nworld", "right.txt", "hello\nrust");
        assert!(app.diff.is_some());
        assert_eq!(app.left_path, "left.txt");
        assert_eq!(app.right_path, "right.txt");
    }

    #[test]
    fn test_app_change_navigation() {
        let mut app = FileDiffApp::new();
        app.load_files("a", "a\nb\nc", "b", "a\nX\nc");
        assert!(!app.change_indices.is_empty());
        let initial = app.current_change_index;
        app.next_change();
        assert!(app.current_change_index != initial || app.change_indices.len() <= 1);
    }

    #[test]
    fn test_app_prev_change_wraps() {
        let mut app = FileDiffApp::new();
        app.load_files("a", "a\nb", "b", "a\nX");
        assert!(!app.change_indices.is_empty());
        app.prev_change();
        assert_eq!(
            app.current_change_index,
            app.change_indices.len().saturating_sub(1)
        );
    }

    #[test]
    fn test_app_toggle_ignore_options() {
        let mut app = FileDiffApp::new();
        app.load_files("a", "Hello", "b", "hello");
        assert!(!app.ignore_opts.ignore_case);
        app.toggle_ignore_case();
        assert!(app.ignore_opts.ignore_case);
    }

    #[test]
    fn test_app_merge_operations() {
        let mut app = FileDiffApp::new();
        app.load_files("a", "a\nb\nc", "b", "a\nX\nc");
        app.accept_left();
        if let Some(ref merge) = app.merge {
            assert_eq!(merge.get_decision(0), MergeDecision::AcceptLeft);
        }
    }

    // --- Render tests ---

    #[test]
    fn test_render_empty_state() {
        let app = FileDiffApp::new();
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_diff() {
        let mut app = FileDiffApp::new();
        app.load_files("a", "hello\nworld", "b", "hello\nrust");
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_unified_view() {
        let mut app = FileDiffApp::new();
        app.view_mode = ViewMode::Unified;
        app.load_files("a", "hello\nworld", "b", "hello\nrust");
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_inline_view() {
        let mut app = FileDiffApp::new();
        app.view_mode = ViewMode::Inline;
        app.load_files("a", "hello world", "b", "hello rust");
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    // --- DiffOp display tests ---

    #[test]
    fn test_diffop_display() {
        assert_eq!(format!("{}", DiffOp::Equal), " ");
        assert_eq!(format!("{}", DiffOp::Insert), "+");
        assert_eq!(format!("{}", DiffOp::Delete), "-");
    }

    // --- Side-by-side pairing tests ---

    #[test]
    fn test_side_by_side_pairs_equal() {
        let edits = vec![DiffEdit {
            op: DiffOp::Equal,
            left_line: Some(0),
            right_line: Some(0),
            text: "hello".to_string(),
        }];
        let pairs = build_side_by_side_pairs(&edits).pairs;
        assert_eq!(pairs.len(), 1);
        assert!(pairs[0].left_text.is_some());
        assert!(pairs[0].right_text.is_some());
    }

    #[test]
    fn test_side_by_side_pairs_delete_insert_paired() {
        let edits = vec![
            DiffEdit {
                op: DiffOp::Delete,
                left_line: Some(0),
                right_line: None,
                text: "old".to_string(),
            },
            DiffEdit {
                op: DiffOp::Insert,
                left_line: None,
                right_line: Some(0),
                text: "new".to_string(),
            },
        ];
        let pairs = build_side_by_side_pairs(&edits).pairs;
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].left_text.as_deref(), Some("old"));
        assert_eq!(pairs[0].right_text.as_deref(), Some("new"));
    }

    #[test]
    fn test_side_by_side_pairs_standalone_delete() {
        let edits = vec![DiffEdit {
            op: DiffOp::Delete,
            left_line: Some(0),
            right_line: None,
            text: "removed".to_string(),
        }];
        let pairs = build_side_by_side_pairs(&edits).pairs;
        assert_eq!(pairs.len(), 1);
        assert!(pairs[0].left_text.is_some());
        assert!(pairs[0].right_text.is_none());
    }

    // --- IgnoreOptions tests ---

    #[test]
    fn test_ignore_options_has_any() {
        let default_opts = IgnoreOptions::default();
        assert!(!default_opts.has_any());

        let ws_opts = IgnoreOptions {
            ignore_whitespace: true,
            ..Default::default()
        };
        assert!(ws_opts.has_any());
    }

    // `normalize_line` is now a private detail of the `diffcore` crate;
    // its behavior is covered there by the `compute_diff` ignore-option tests.

    // --- FileCompareStatus tests ---

    #[test]
    fn test_file_compare_status_display() {
        assert_eq!(format!("{}", FileCompareStatus::Same), "Same");
        assert_eq!(format!("{}", FileCompareStatus::Different), "Different");
        assert_eq!(format!("{}", FileCompareStatus::OnlyLeft), "Only in left");
        assert_eq!(format!("{}", FileCompareStatus::OnlyRight), "Only in right");
    }

    // --- Event handling tests ---

    #[test]
    fn test_handle_resize() {
        let mut app = FileDiffApp::new();
        let result = app.handle_event(&Event::Resize {
            width: 1920,
            height: 1080,
        });
        assert_eq!(result, EventResult::Consumed);
        assert!((app.width - 1920.0).abs() < f32::EPSILON);
        assert!((app.height - 1080.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_handle_scroll_down() {
        let mut app = FileDiffApp::new();
        // Use more lines than fit in the viewport so there is room to scroll
        // (an 8-line file fits entirely on screen and would never scroll).
        let left: String = (0..60)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut right_lines: Vec<String> = (0..60).map(|i| format!("line{i}")).collect();
        right_lines[59] = "changed".to_string();
        let right = right_lines.join("\n");
        app.load_files("a", &left, "b", &right);
        let key = KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let result = app.handle_event(&Event::Key(key));
        assert_eq!(result, EventResult::Consumed);
        assert!(app.scroll_left > 0.0);
    }

    // --- Wheel scrolling ---

    /// A diff long enough that both panels have room to scroll.
    fn long_diff() -> FileDiffApp {
        let mut app = FileDiffApp::new();
        let left: String = (0..400)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("
");
        let mut right_lines: Vec<String> = (0..400).map(|i| format!("line{i}")).collect();
        right_lines[399] = "changed".to_string();
        app.load_files("a", &left, "b", &right_lines.join("
"));
        app
    }

    fn wheel_at(app: &mut FileDiffApp, x: f32, dy: f32) {
        app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y: 200.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        }));
    }

    #[test]
    fn one_wheel_notch_moves_three_rows() {
        let mut app = long_diff();
        wheel_at(&mut app, 100.0, -1.0);
        assert_eq!(app.scroll_left, 3.0, "one detent is three rows");
        wheel_at(&mut app, 100.0, 1.0);
        assert_eq!(app.scroll_left, 0.0, "and back the other way");
    }

    #[test]
    fn a_fraction_of_a_notch_moves_now_rather_than_being_banked() {
        // The offset is an `f32`, so there is nothing an accumulator could buy
        // -- it would only sit on movement this view is able to show.
        let mut app = long_diff();
        wheel_at(&mut app, 100.0, -0.2);
        assert_eq!(app.scroll_left, 0.6);
    }

    #[test]
    fn scrolling_stops_at_the_last_page_and_at_the_top() {
        let mut app = long_diff();
        let max = app.max_scroll();
        assert!(max > 0.0, "the fixture must be longer than the viewport");
        for _ in 0..500 {
            wheel_at(&mut app, 100.0, -1.0);
        }
        assert_eq!(app.scroll_left, max);
        for _ in 0..500 {
            wheel_at(&mut app, 100.0, 1.0);
        }
        assert_eq!(app.scroll_left, 0.0);
    }

    #[test]
    fn unsynced_panels_scroll_independently() {
        let mut app = long_diff();
        app.view_mode = ViewMode::SideBySide;
        app.sync_scroll = false;
        wheel_at(&mut app, 10.0, -1.0);
        assert_eq!(app.scroll_left, 3.0);
        assert_eq!(app.scroll_right, 0.0, "the right panel was not under the pointer");
        let right_x = app.width - 10.0;
        wheel_at(&mut app, right_x, -2.0);
        assert_eq!(app.scroll_left, 3.0);
        assert_eq!(app.scroll_right, 6.0);
    }

    #[test]
    fn a_nonfinite_delta_does_not_freeze_the_view() {
        // An infinity stored into the offset would clamp to `max` and never
        // come back; `wheel::rows_f` turns one into no movement at all.
        let mut app = long_diff();
        wheel_at(&mut app, 100.0, f32::NAN);
        wheel_at(&mut app, 100.0, f32::INFINITY);
        assert_eq!(app.scroll_left, 0.0);
        wheel_at(&mut app, 100.0, -1.0);
        assert_eq!(app.scroll_left, 3.0, "and later scrolls still work");
    }

    #[test]
    fn test_handle_view_mode_switch() {
        let mut app = FileDiffApp::new();
        let key = KeyEvent {
            key: Key::Num2,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        };
        let result = app.handle_event(&Event::Key(key));
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(app.view_mode, ViewMode::Unified);
    }

    // --- Merged text tests ---

    #[test]
    fn test_merged_text_none_without_diff() {
        let app = FileDiffApp::new();
        assert!(app.merged_text().is_none());
    }

    #[test]
    fn test_merged_text_with_diff() {
        let mut app = FileDiffApp::new();
        app.load_files("a", "hello", "b", "world");
        assert!(app.merged_text().is_some());
    }

    // --- Default trait test ---

    #[test]
    fn test_default_app() {
        let app = FileDiffApp::default();
        assert!(app.diff.is_none());
        assert_eq!(app.view_mode, ViewMode::SideBySide);
    }

    // --- Additional search current tests ---

    #[test]
    fn test_search_current_empty() {
        let search = SearchState::default();
        assert!(search.current().is_none());
    }

    #[test]
    fn test_search_current_with_results() {
        let left = "hello";
        let right = "hello";
        let opts = IgnoreOptions::default();
        let diff = compute_diff(left, right, &opts);
        let mut search = SearchState {
            query: "hello".to_string(),
            case_sensitive: true,
            ..Default::default()
        };
        search.search(&diff.edits);
        assert!(search.current().is_some());
    }
    // --- Text measurement tests ---

    /// Every toolbar label has to fit the button drawn around it. The button
    /// is sized by measuring the label, so this only fails if someone puts an
    /// estimate back — which is how the old 7.8-px guess let "Side-by-Side"
    /// spill past its own background at any face wider than the built-in one.
    #[test]
    fn toolbar_labels_fit_inside_their_buttons() {
        let app = FileDiffApp::default();
        let mut tree = RenderTree::new();
        app.render_toolbar(&mut tree);

        let mut box_at: Option<(f32, f32)> = None;
        let mut checked = 0;
        for cmd in &tree.commands {
            match cmd {
                RenderCommand::FillRect { x, width, .. } => box_at = Some((*x, *width)),
                RenderCommand::Text {
                    x,
                    text,
                    font_size,
                    font_weight,
                    ..
                } => {
                    let Some((bx, bw)) = box_at else { continue };
                    let end = x + text::measure(text, *font_size, *font_weight);
                    assert!(
                        end <= bx + bw + 0.5,
                        "{text:?} ends at {end} but its button ends at {}",
                        bx + bw
                    );
                    checked += 1;
                }
                _ => {}
            }
        }
        assert!(
            checked > 3,
            "expected several toolbar labels, saw {checked}"
        );
    }

    /// A highlight rectangle is as wide as the run it marks, whatever that run
    /// is made of. Bytes were the first version of this bug — an accented word
    /// got a box two or three times too wide — and a character count was the
    /// second, which is right only where every character advances one cell.
    #[test]
    fn a_highlight_is_as_wide_as_the_run_it_marks() {
        let width = |s: &str| {
            text::measure_in(
                s,
                CONTENT_FONT_SIZE,
                FontWeightHint::Regular,
                FontFamily::Mono,
            )
        };
        // Bytes: three characters, six bytes, one width.
        assert!((width("abc") - width("áéí")).abs() < 0.01);
        // Cells: one character, and emphatically not one cell. This is the
        // case a character count still got wrong, and tabs are how source is
        // indented, so it was not a corner.
        assert!(
            width("\t") > width("x") * 1.5,
            "a tab measures {} against a {} character — if a tab really is one \
             cell on this face, this test has stopped testing anything",
            width("\t"),
            width("x")
        );
        // Additive, which is what lets the pen walk span by span.
        assert!((width("ab") + width("cd") - width("abcd")).abs() < 0.01);
        assert!(
            char_width() > 0.0,
            "a zero cell would collapse every column"
        );
    }

    /// Why a character count survived review: for Latin source in the mono
    /// face, a character does fit a cell. This is the premise that held for
    /// everything anyone looked at and failed for everything else.
    #[test]
    fn a_source_character_fits_a_content_cell() {
        let cell = char_width();
        for ch in ['0', 'W', 'i', '#', 'é', 'M', '@', '_', ' '] {
            let w = text::measure_in(
                &ch.to_string(),
                CONTENT_FONT_SIZE,
                FontWeightHint::Regular,
                FontFamily::Mono,
            );
            assert!(w <= cell + 0.01, "{ch:?} measures {w} in a {cell} cell");
        }
    }

    /// Bold marks a changed span in the inline view. It sits on the same grid
    /// as the regular text around it, so it has to fit the same cell.
    #[test]
    fn a_bold_source_character_fits_the_same_cell() {
        let cell = char_width();
        for ch in ['0', 'W', 'M', '@'] {
            let w = text::measure_in(
                &ch.to_string(),
                CONTENT_FONT_SIZE,
                FontWeightHint::Bold,
                FontFamily::Mono,
            );
            assert!(
                w <= cell + 0.01,
                "bold {ch:?} measures {w} in a {cell} cell"
            );
        }
    }

    /// Content is placed on a mono cell, so it must be drawn in the mono face.
    /// Chrome — toolbar, headers, status bar, scrollbars — must not be.
    #[test]
    fn content_is_drawn_in_the_family_it_was_measured_in() {
        for mode in [ViewMode::SideBySide, ViewMode::Unified, ViewMode::Inline] {
            let mut app = FileDiffApp::new();
            app.view_mode = mode;
            app.load_files(
                "a",
                "alpha\nbravo WWWW\ncharlie",
                "b",
                "alpha\nbravo iiii\ndelta",
            );
            let tree = app.render();

            let mut depth = 0_i32;
            let mut deepest = 0_i32;
            let mut inside = 0_usize;
            for cmd in &tree.commands {
                match cmd {
                    RenderCommand::PushFont { family } => {
                        assert_eq!(family, &FontFamily::Mono, "only content pushes a family");
                        depth += 1;
                        deepest = deepest.max(depth);
                    }
                    RenderCommand::PopFont => {
                        depth -= 1;
                        assert!(depth >= 0, "{mode:?}: a PopFont without a PushFont");
                    }
                    RenderCommand::Text { .. } if depth > 0 => inside += 1,
                    _ => {}
                }
            }
            assert_eq!(depth, 0, "{mode:?}: the font scopes do not balance");
            assert_eq!(deepest, 1, "{mode:?}: the content scope was never opened");
            assert!(
                inside > 0,
                "{mode:?}: no content drawn inside the mono scope"
            );
        }
    }

    /// The gutter right-aligns line numbers, so even the widest one still has
    /// to start inside the gutter rather than being pushed off its left edge.
    #[test]
    fn line_numbers_stay_inside_the_gutter() {
        for n in ["1", "42", "9999"] {
            let right = GUTTER_WIDTH - 4.0;
            let x = text::right_x(n, right, CONTENT_FONT_SIZE, FontWeightHint::Regular);
            assert!(
                x >= 0.0,
                "line number {n} starts at {x}, left of the gutter"
            );
            let end = x + text::measure(n, CONTENT_FONT_SIZE, FontWeightHint::Regular);
            assert!(
                (end - right).abs() < 0.01,
                "{n} ends at {end}, not at {right}"
            );
        }
    }

    // --- Cursor wrapping ---

    #[test]
    fn a_cursor_over_an_empty_list_has_nowhere_to_go() {
        assert_eq!(wrap_next(0, 0), None);
        assert_eq!(wrap_prev(0, 0), None);
        // The guard and the arithmetic being one statement is the point: there
        // is no `%` or `- 1` left that a missing `is_empty()` could reach.
        assert_eq!(wrap_next(usize::MAX, 0), None);
        assert_eq!(wrap_prev(usize::MAX, 0), None);
    }

    #[test]
    fn a_cursor_wraps_at_both_ends() {
        assert_eq!(wrap_next(0, 3), Some(1));
        assert_eq!(wrap_next(2, 3), Some(0));
        assert_eq!(wrap_prev(0, 3), Some(2));
        assert_eq!(wrap_prev(1, 3), Some(0));
        assert_eq!(wrap_next(0, 1), Some(0));
        assert_eq!(wrap_prev(0, 1), Some(0));
    }

    // --- Viewport ---

    #[test]
    fn a_viewport_overscans_by_one_row_at_each_end() {
        // Ten rows fit exactly, so twelve are drawn: the partial row at the top
        // and the partial row at the bottom.
        let r = visible_range(0.0, LINE_HEIGHT * 10.0, 1000);
        assert_eq!(r, 0..12);
        let r = visible_range(5.0, LINE_HEIGHT * 10.0, 1000);
        assert_eq!(r, 5..17);
    }

    #[test]
    fn a_viewport_stops_at_the_end_of_the_list() {
        assert_eq!(visible_range(0.0, LINE_HEIGHT * 10.0, 3), 0..3);
        // Scrolled past the end -- which `max_scroll` prevents, but a viewport
        // that panicked or wrapped if it ever happened would be the wrong shape.
        assert!(visible_range(500.0, LINE_HEIGHT * 10.0, 3).is_empty());
        assert!(visible_range(0.0, LINE_HEIGHT * 10.0, 0).is_empty());
    }

    #[test]
    fn a_negative_scroll_starts_at_the_top_not_at_the_end_of_the_list() {
        // A float-to-integer cast saturates. If it wrapped, an overscroll of
        // one frame would jump to `usize::MAX` and blank the panel.
        assert_eq!(visible_range(-3.0, LINE_HEIGHT * 10.0, 1000), 0..12);
    }

    // --- The row a view draws an edit on ---

    /// Side-by-side folds a delete and the insert after it into one row, so its
    /// row count is smaller than the edit count. Three separate places used to
    /// assume the two numbers were the same.
    #[test]
    fn a_paired_modification_puts_both_its_edits_on_one_row() {
        let edits = vec![
            DiffEdit {
                op: DiffOp::Equal,
                left_line: Some(0),
                right_line: Some(0),
                text: "same".to_string(),
            },
            DiffEdit {
                op: DiffOp::Delete,
                left_line: Some(1),
                right_line: None,
                text: "old".to_string(),
            },
            DiffEdit {
                op: DiffOp::Insert,
                left_line: None,
                right_line: Some(1),
                text: "new".to_string(),
            },
            DiffEdit {
                op: DiffOp::Equal,
                left_line: Some(2),
                right_line: Some(2),
                text: "tail".to_string(),
            },
        ];
        let rows = build_side_by_side_pairs(&edits);
        assert_eq!(rows.pairs.len(), 3, "four edits, one of them paired");
        assert_eq!(rows.row_of_edit, vec![0, 1, 1, 2]);
        assert_eq!(rows.pairs[1].left_edit, Some(1));
        assert_eq!(rows.pairs[1].right_edit, Some(2));
    }

    /// Every edit has a row, and no row is claimed out of order. Written as a
    /// property over a real diff rather than a hand-built edit list, because
    /// the map is only useful if it stays parallel to whatever `compute_diff`
    /// actually emits.
    #[test]
    fn every_edit_has_a_row_and_the_rows_only_move_forward() {
        let left = "a\nb\nc\nd\ne\nf\n";
        let right = "a\nB\nc\nD\nE\nf\nG\n";
        let diff = compute_diff(left, right, &IgnoreOptions::default());
        let rows = build_side_by_side_pairs(&diff.edits);

        assert_eq!(rows.row_of_edit.len(), diff.edits.len());
        let mut previous = 0;
        for (edit, &row) in rows.row_of_edit.iter().enumerate() {
            assert!(row < rows.pairs.len(), "edit {edit} claims row {row}");
            assert!(row >= previous, "edit {edit} goes backwards to row {row}");
            previous = row;
        }
        assert_eq!(rows.row_of_edit.first(), Some(&0));
        assert_eq!(rows.row_of_edit.last(), Some(&(rows.pairs.len() - 1)));
    }

    /// `count` numbered lines of `prefix`, each preceded by a shared line.
    ///
    /// The shared lines are the point. Two files with *nothing* in common diff
    /// as one delete block followed by one insert block, and the side-by-side
    /// pairing rule -- a delete folds in the insert immediately after it --
    /// then fires exactly once, at the seam between the blocks. Twenty changed
    /// lines with no context are 39 rows, not 20. Interleaving an unchanged
    /// line forces each change to be its own adjacent delete-then-insert, which
    /// is the shape where rows and edits actually diverge:
    /// `count` equal rows plus `count` paired rows against `3 * count` edits.
    fn interleaved(prefix: &str, count: usize) -> String {
        let mut out = String::new();
        for i in 0..count {
            out.push_str("same");
            out.push_str(&i.to_string());
            out.push('\n');
            out.push_str(prefix);
            out.push_str(&i.to_string());
            out.push('\n');
        }
        out
    }

    /// Jumping to a change scrolled to *edit* N when the change was drawn on
    /// row N-k, k being the number of paired modifications before it. With
    /// enough of them the target was off the bottom of the list entirely and
    /// "next change" showed a blank panel.
    #[test]
    fn jumping_to_a_change_scrolls_to_the_row_it_is_drawn_on() {
        let mut app = FileDiffApp::new();
        // Twenty modified lines with an unchanged line between each: sixty
        // edits, forty rows.
        app.load_files("a", &interleaved("old", 20), "b", &interleaved("new", 20));
        app.view_mode = ViewMode::SideBySide;
        app.height = TOOLBAR_HEIGHT + STATUS_BAR_HEIGHT + LINE_HEIGHT * 4.0;

        let rows = app.sbs.pairs.len();
        assert_eq!(rows, 40, "twenty equal rows and twenty modified ones");
        assert_eq!(
            app.diff.as_ref().unwrap().edits.len(),
            60,
            "but sixty edits, which is what made the two indices differ"
        );

        // Jump to the last change and require the row it is drawn on to be on
        // screen. Scrolling to the *edit* index put the view twenty rows past
        // the end of a forty-row list, which is a blank panel.
        app.current_change_index = app.change_indices.len() - 1;
        app.scroll_to_current_change();
        let last_edit = *app.change_indices.last().unwrap();
        let row = app.display_row_of_edit(last_edit).unwrap() as f32;
        let visible = app.visible_line_count();
        assert!(
            app.scroll_left <= row && row < app.scroll_left + visible,
            "row {row} is not on screen: scrolled to {} showing {visible} rows",
            app.scroll_left
        );
    }

    /// The scroll limit is a row count too. Using the edit count let the view
    /// scroll past the bottom by one row per unchanged line and two per
    /// modified one -- twenty blank rows on the file above.
    #[test]
    fn the_scroll_limit_counts_rows_not_edits() {
        let mut app = FileDiffApp::new();
        app.load_files("a", &interleaved("old", 20), "b", &interleaved("new", 20));
        app.height = TOOLBAR_HEIGHT + STATUS_BAR_HEIGHT + LINE_HEIGHT * 4.0;

        app.view_mode = ViewMode::SideBySide;
        let sbs_max = app.max_scroll();
        app.view_mode = ViewMode::Unified;
        let unified_max = app.max_scroll();

        assert_eq!(app.diff.as_ref().unwrap().edits.len(), 60);
        assert_eq!(app.sbs.pairs.len(), 40);
        assert!(
            sbs_max < unified_max,
            "side-by-side folds two edits into one row, so it scrolls less: \
             {sbs_max} vs {unified_max}"
        );
        assert_eq!(sbs_max, 40.0 - app.visible_line_count());
    }

    // --- Search highlighting ---

    /// Count the highlight rectangles a render produced, by colour.
    fn highlight_counts(tree: &RenderTree) -> (usize, usize) {
        let mut plain = 0;
        let mut current = 0;
        for cmd in &tree.commands {
            if let RenderCommand::FillRect { color, .. } = cmd {
                if *color == colors::SEARCH_BG {
                    plain += 1;
                } else if *color == colors::SEARCH_CURRENT_BG {
                    current += 1;
                }
            }
        }
        (plain, current)
    }

    fn searching_app_on(mode: ViewMode, left: &str, right: &str, query: &str) -> FileDiffApp {
        let mut app = FileDiffApp::new();
        app.view_mode = mode;
        app.load_files("a", left, "b", right);
        app.search.visible = true;
        app.search.query = query.to_string();
        app.refresh_search_matches();
        app
    }

    /// Two unchanged lines around one modified line -- the smallest file that
    /// has an equal row, a delete and an insert all at once.
    fn searching_app(mode: ViewMode, query: &str) -> FileDiffApp {
        searching_app_on(
            mode,
            "alpha\nbravo\ncharlie\n",
            "alpha\nzulu\ncharlie\n",
            query,
        )
    }

    /// The feature this whole section is about: matches were computed, counted
    /// in the status bar and cycled through with Enter, and never drawn. On any
    /// file longer than a screen, find-in-diff was a number that changed.
    #[test]
    fn a_search_match_is_drawn_as_a_highlight() {
        for mode in [ViewMode::SideBySide, ViewMode::Unified, ViewMode::Inline] {
            let app = searching_app(mode, "alpha");
            assert!(!app.search.matches.is_empty(), "{mode:?}: nothing matched");
            let (plain, current) = highlight_counts(&app.render());
            assert!(
                plain + current > 0,
                "{mode:?}: {} matches and no highlight drawn",
                app.search.matches.len()
            );
            assert_eq!(current, 1, "{mode:?}: exactly one match is the focused one");
        }
    }

    /// An equal line is recorded twice, once per side-by-side panel. The
    /// single-column views draw that line once, so they must take one copy --
    /// otherwise every hit on an unchanged line is painted twice over.
    #[test]
    fn an_equal_line_is_highlighted_once_in_a_single_column_view() {
        let app = searching_app(ViewMode::Unified, "alpha");
        assert_eq!(
            app.search.matches.len(),
            2,
            "an equal line records one match per panel"
        );
        let (plain, current) = highlight_counts(&app.render());
        assert_eq!(plain + current, 1, "but the unified view draws one line");

        // Side-by-side draws the line twice, so it draws both.
        let app = searching_app(ViewMode::SideBySide, "alpha");
        let (plain, current) = highlight_counts(&app.render());
        assert_eq!(plain + current, 2, "one per panel");
    }

    /// A delete and the insert replacing it are two different edits on one
    /// side-by-side row, and each panel must show only its own hit.
    ///
    /// `zulu` is on the new side only -- and searching is case-insensitive by
    /// default, so a word that differs from its counterpart merely in case
    /// would be found on *both* sides and prove nothing.
    #[test]
    fn each_panel_shows_only_its_own_side_of_a_modified_line() {
        let app = searching_app(ViewMode::SideBySide, "zulu");
        assert_eq!(app.search.matches.len(), 1, "only the right side has it");
        assert_eq!(app.search.matches[0].panel, 1);
        let (plain, current) = highlight_counts(&app.render());
        assert_eq!(plain + current, 1, "drawn once, on the right");
    }

    /// The offsets in a match are byte offsets; the grid is measured in cells.
    /// Confusing the two has been this sweep's most common defect, and here it
    /// would put the box several columns right of the word it marks.
    #[test]
    fn a_highlight_is_placed_in_cells_not_bytes() {
        let mut app = FileDiffApp::new();
        app.view_mode = ViewMode::Unified;
        // "ééé" is three characters and six bytes; the hit starts after them.
        app.load_files("a", "éééNEEDLE\n", "b", "éééNEEDLE\n");
        app.search.visible = true;
        app.search.query = "NEEDLE".to_string();
        app.refresh_search_matches();

        let m = app.search.matches.first().copied().expect("a match");
        assert_eq!(m.byte_offset, 6, "six bytes in, three cells in");

        let tree = app.render();
        // Where `render_unified_line` puts the line's first character: two
        // gutters, the panel padding, then the two-cell `+ ` / `- ` marker.
        let text_x = GUTTER_WIDTH * 2.0 + PANEL_PADDING + char_width() * 2.0;
        let rect = tree
            .commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::FillRect {
                    x, width, color, ..
                } if *color == colors::SEARCH_CURRENT_BG => Some((*x, *width)),
                _ => None,
            })
            .expect("the focused match is drawn");
        assert!(
            (rect.0 - (text_x + 3.0 * char_width())).abs() < 0.01,
            "highlight starts at {} but the fourth cell is at {}",
            rect.0,
            text_x + 3.0 * char_width()
        );
        assert!(
            (rect.1 - 6.0 * char_width()).abs() < 0.01,
            "NEEDLE is six cells wide, not {}",
            rect.1 / char_width()
        );
    }

    /// The common case: most lines hold no match, so the highlighter is called
    /// with an empty list far more often than not and must cost nothing there.
    #[test]
    fn an_overlay_with_nothing_in_it_draws_nothing() {
        let mut tree = RenderTree::new();
        let empty = SearchOverlay {
            matches: &[],
            panel: 0,
            current: None,
        };
        render_search_highlights(&mut tree, 0.0, 0.0, "anything", empty);
        assert!(tree.commands.is_empty());
    }

    /// A match whose offsets do not fit the line -- which a stale match list
    /// can produce -- draws nothing rather than panicking on a slice.
    #[test]
    fn a_match_that_does_not_fit_the_line_draws_nothing() {
        let bad = [
            SearchMatch {
                panel: 0,
                edit_index: 0,
                byte_offset: 500,
                match_len: 3,
            },
            SearchMatch {
                panel: 0,
                edit_index: 0,
                byte_offset: 0,
                match_len: 500,
            },
            SearchMatch {
                panel: 0,
                edit_index: 0,
                byte_offset: usize::MAX,
                match_len: 2,
            },
            // Mid-character: `String::get` refuses it, which is the whole
            // reason this uses `get` and not a slice.
            SearchMatch {
                panel: 0,
                edit_index: 0,
                byte_offset: 1,
                match_len: 1,
            },
        ];
        let mut tree = RenderTree::new();
        render_search_highlights(
            &mut tree,
            0.0,
            0.0,
            "é",
            SearchOverlay {
                matches: &bad,
                panel: 0,
                current: None,
            },
        );
        assert!(
            tree.commands.is_empty(),
            "{} boxes drawn",
            tree.commands.len()
        );
    }

    // --- Search navigation ---

    /// Advancing the match counter used to leave the view where it was, so on
    /// a file longer than a screen "2/17" and "3/17" looked identical.
    #[test]
    fn advancing_to_the_next_match_scrolls_to_it() {
        let mut app = FileDiffApp::new();
        let mut left = String::new();
        for i in 0..200 {
            left.push_str(if i == 5 || i == 150 {
                "needle\n"
            } else {
                "x\n"
            });
        }
        app.load_files("a", &left, "b", &left);
        app.height = TOOLBAR_HEIGHT + STATUS_BAR_HEIGHT + LINE_HEIGHT * 10.0;
        app.search.visible = true;
        app.search.query = "needle".to_string();
        app.rerun_search();

        let first = app.scroll_left;
        assert!(
            first < 20.0,
            "the first hit is near the top, not at {first}"
        );

        // Two presses: the first hit is recorded once per panel, so the second
        // entry is the same line seen from the other side.
        app.handle_search_key(&key(Key::Enter));
        app.handle_search_key(&key(Key::Enter));
        assert!(
            app.scroll_left > 100.0,
            "the second hit is at line 150 but the view sits at {}",
            app.scroll_left
        );
    }

    #[test]
    fn typing_a_query_scrolls_to_the_first_hit() {
        let mut app = FileDiffApp::new();
        let mut left = String::new();
        for i in 0..200 {
            left.push_str(if i == 150 { "needle\n" } else { "x\n" });
        }
        app.load_files("a", &left, "b", &left);
        app.height = TOOLBAR_HEIGHT + STATUS_BAR_HEIGHT + LINE_HEIGHT * 10.0;
        app.search.visible = true;
        for ch in "needle".chars() {
            app.handle_search_key(&KeyEvent {
                key: Key::A,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: Some(ch),
            });
        }
        assert!(
            app.scroll_left > 100.0,
            "typed a query whose only hit is at line 150, view sits at {}",
            app.scroll_left
        );
    }

    /// Escape hides the search bar but not the highlights, so the match list
    /// outlives the bar -- and must therefore be rebuilt when the diff is,
    /// or it names edits that no longer exist.
    #[test]
    fn recomputing_the_diff_does_not_leave_stale_matches() {
        let mut app = FileDiffApp::new();
        // Ten lines that differ only in leading space, then the hit. With
        // whitespace significant those ten lines are twenty edits and the
        // needle is edit twenty; ignoring whitespace collapses them to ten
        // equal edits and the needle moves to edit ten. A match list that
        // survives the recompute therefore names an edit that no longer
        // exists -- so the shrink has to be large enough to run off the end,
        // which a one-line file is not.
        let mut left = String::new();
        let mut right = String::new();
        for i in 0..10 {
            let n = i.to_string();
            left.push_str("  a");
            left.push_str(&n);
            left.push('\n');
            right.push('a');
            right.push_str(&n);
            right.push('\n');
        }
        left.push_str("needle\n");
        right.push_str("needle\n");
        app.load_files("a", &left, "b", &right);
        app.search.visible = true;
        app.search.query = "needle".to_string();
        app.refresh_search_matches();
        assert!(
            app.search.matches.iter().any(|m| m.edit_index >= 11),
            "the hit must start out past where the shrunk list ends"
        );

        // Escape hides the bar but not the highlights, so this is the state a
        // user is actually in when they toggle the option.
        app.search.visible = false;
        app.toggle_ignore_whitespace();

        let edits = app.diff.as_ref().unwrap().edits.len();
        for m in &app.search.matches {
            assert!(
                m.edit_index < edits,
                "match names edit {} of {edits}",
                m.edit_index
            );
        }
        // And the highlighter agrees with the new layout.
        let (plain, current) = highlight_counts(&app.render());
        assert!(
            plain + current > 0,
            "the hit is still there, and still drawn"
        );
    }

    fn key(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }
}
