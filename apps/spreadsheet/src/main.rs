//! Slate OS Spreadsheet
//!
//! Full-featured spreadsheet application with:
//! - Cell grid with columns A-Z and rows 1-999
//! - Formula engine (SUM, AVG, MIN, MAX, COUNT, IF, ABS, ROUND, CONCATENATE, LEN, UPPER, LOWER)
//! - Cell formatting (bold, italic, alignment, number formats)
//! - Column/row resize, selection, clipboard, undo/redo
//! - Multiple sheets, sort, auto-fill, freeze panes
//! - Find and replace, CSV import/export
//! - Catppuccin Mocha theme
//!
//! Uses the guitk library for UI rendering.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use guitk::text;
use guitk::textfind;
use guitk::wheel;

use std::collections::{BTreeMap, HashMap};

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
const COLOR_MANTLE: Color = Color::from_hex(0x181825);
const COLOR_CRUST: Color = Color::from_hex(0x11111B);
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
const _COLOR_SURFACE2: Color = Color::from_hex(0x585B70);
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
const COLOR_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COLOR_SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
const COLOR_GREEN: Color = Color::from_hex(0xA6E3A1);
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
const _COLOR_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COLOR_PEACH: Color = Color::from_hex(0xFAB387);
const COLOR_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const _COLOR_OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Layout constants
// ============================================================================

const MAX_COLS: usize = COLUMN_LETTERS.len();
const MAX_ROWS: usize = 999;
const DEFAULT_COL_WIDTH: f32 = 100.0;
const DEFAULT_ROW_HEIGHT: f32 = 24.0;
const MIN_COL_WIDTH: f32 = 30.0;
const MIN_ROW_HEIGHT: f32 = 16.0;
const ROW_HEADER_WIDTH: f32 = 50.0;
const COL_HEADER_HEIGHT: f32 = 24.0;
const TOOLBAR_HEIGHT: f32 = 36.0;
const FORMULA_BAR_HEIGHT: f32 = 28.0;
const SHEET_TAB_HEIGHT: f32 = 28.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const FONT_SIZE: f32 = 13.0;
const SMALL_FONT: f32 = 11.0;
const HEADER_FONT: f32 = 12.0;
const RESIZE_HANDLE_SIZE: f32 = 5.0;
const AUTOFILL_HANDLE_SIZE: f32 = 7.0;
const UNDO_STACK_LIMIT: usize = 200;
const SCROLLBAR_WIDTH: f32 = 14.0;
const SHEET_TAB_WIDTH: f32 = 90.0;

/// Move a zero-based index by a signed delta, staying inside `0..limit`.
///
/// Written once, in `usize`, rather than twice as a round trip through `i32`.
/// The round trip stated the same bound three ways — `col as i32`, `.max(0)`,
/// `.min(MAX_COLS as i32 - 1)` — and each of the two axes wrote all three out,
/// which is six statements of one rule.
fn step_index(index: usize, delta: i32, limit: usize) -> usize {
    let last = limit.saturating_sub(1);
    // `unsigned_abs` rather than `-delta`, which panics in debug for
    // `i32::MIN`; the two directions then differ only in which saturating
    // operation they use, and neither can leave the range.
    let magnitude = usize::try_from(delta.unsigned_abs()).unwrap_or(usize::MAX);
    let moved = if delta >= 0 {
        index.saturating_add(magnitude)
    } else {
        index.saturating_sub(magnitude)
    };
    moved.min(last)
}

// ============================================================================
// Cell address
// ============================================================================

/// A cell address (column, row) — zero-indexed internally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellAddr {
    pub col: usize,
    pub row: usize,
}

/// The column headings, in order, and the definition of how many there are.
///
/// [`CellAddr::col_letter`] and [`CellAddr::parse`] are inverses of one another,
/// and they used to say so only by coincidence: one computed `b'A' + col` after
/// testing `col < 26`, the other computed `col_char - b'A'` after testing
/// `is_ascii_uppercase`, and `MAX_COLS` was a third statement of the same 26.
/// Reading both directions out of this table is what makes them agree by
/// construction, and it is why neither needs a bound of its own —
/// `get`/`position` are the bound.
const COLUMN_LETTERS: &[u8; 26] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";

impl CellAddr {
    /// Create a new cell address from zero-indexed column and row.
    pub fn new(col: usize, row: usize) -> Self {
        Self { col, row }
    }

    /// Convert column index (0-based) to letter string (A, B, ..., Z).
    ///
    /// Returns `"?"` for a column past the last one, which is what the headers
    /// and the address box display rather than nothing at all.
    pub fn col_letter(col: usize) -> String {
        COLUMN_LETTERS
            .get(col)
            .map_or_else(|| "?".to_owned(), |&byte| char::from(byte).to_string())
    }

    /// Convert row index (0-based) to the label the row header shows.
    ///
    /// Rows are stored zero-based and shown one-based, and that fact used to be
    /// written out at both places that show one — here and in the row-header
    /// renderer — as a bare `+ 1`. Saturating because [`CellAddr::new`] does not
    /// bound its arguments: a row of `usize::MAX` would otherwise wrap the label
    /// round to "0", which reads as a valid address that no cell has.
    pub fn row_label(row: usize) -> String {
        row.saturating_add(1).to_string()
    }

    /// Display string for this cell address, e.g. "A1", "B5".
    pub fn display(&self) -> String {
        let mut s = Self::col_letter(self.col);
        s.push_str(&Self::row_label(self.row));
        s
    }

    /// Parse a cell address string like "A1", "Z999".
    /// Returns `None` if the string is not a valid cell reference.
    pub fn parse(s: &str) -> Option<Self> {
        let upper = s.trim().to_ascii_uppercase();
        let mut chars = upper.chars();
        // `Chars::as_str` hands back the untaken remainder, so the row digits
        // are read without ever forming a byte offset into `upper`. The old
        // code sliced `&upper[1..]`, which is only correct because the first
        // character had already been established as ASCII -- a fact stated
        // five lines earlier and nowhere near the slice.
        let col_byte = u8::try_from(chars.next()?).ok()?;
        // Looking the letter up *is* the range check: anything that is not a
        // column heading is simply not in the table.
        let col = COLUMN_LETTERS
            .iter()
            .position(|&letter| letter == col_byte)?;

        let row_num: usize = chars.as_str().parse().ok()?;
        if row_num == 0 || row_num > MAX_ROWS {
            return None;
        }
        Some(Self {
            col,
            row: row_num.saturating_sub(1),
        })
    }
}

// ============================================================================
// Cell value types
// ============================================================================

/// The type of data stored in a cell.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum CellValue {
    /// No data.
    #[default]
    Empty,
    /// Plain text string.
    Text(String),
    /// Numeric value.
    Number(f64),
    /// Boolean value.
    Boolean(bool),
    /// Error value (e.g. #DIV/0!, #REF!).
    Error(CellError),
}

impl CellValue {
    /// Display this value as a string for rendering in the grid.
    pub fn display_string(&self, format: &NumberFormat) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Text(s) => s.clone(),
            Self::Number(n) => format.format_number(*n),
            Self::Boolean(b) => {
                if *b {
                    "TRUE".to_string()
                } else {
                    "FALSE".to_string()
                }
            }
            Self::Error(e) => e.display().to_string(),
        }
    }

    /// Try to interpret this value as a number.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
            Self::Text(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    }

    /// Check if this value is empty.
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }
}

/// Cell error types.
#[derive(Clone, Debug, PartialEq)]
pub enum CellError {
    DivisionByZero,
    InvalidReference,
    InvalidFormula,
    CircularReference,
    /// The formula nests deeper than the evaluator will recurse.
    ///
    /// Distinct from [`CellError::CircularReference`], which is what a genuine
    /// cycle produces: cycles are detected exactly, by the path of addresses
    /// being visited, so a depth failure is never one. It is a formula that is
    /// merely too deeply nested, and saying so is more use to whoever wrote it
    /// than pointing at a cycle that is not there.
    TooDeep,
    ValueError,
    NameError,
}

impl CellError {
    /// Display string for this error.
    pub fn display(&self) -> &str {
        match self {
            Self::DivisionByZero => "#DIV/0!",
            Self::InvalidReference => "#REF!",
            Self::InvalidFormula => "#ERROR!",
            Self::CircularReference => "#CIRC!",
            Self::TooDeep => "#DEPTH!",
            Self::ValueError => "#VALUE!",
            Self::NameError => "#NAME?",
        }
    }
}

// ============================================================================
// Number formatting
// ============================================================================

/// How to format numeric values in a cell.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum NumberFormat {
    /// General — display as-is.
    #[default]
    General,
    /// Fixed decimal places.
    Decimal(u8),
    /// Percentage (multiply by 100, add %).
    Percentage(u8),
    /// Currency (prefix with $).
    Currency(u8),
}

/// `n` as an integer, if it is one and the conversion loses nothing.
///
/// Two places asked this question, and both spelled it out as
/// `n == n.floor() && n.abs() < 1e15`. That is the same rule stated twice, and
/// the `1e15` was a hand-picked stand-in for "small enough that `as i64` will
/// not saturate" — a connection neither copy made, and one that is off by three
/// orders of magnitude from the real bound.
fn whole_number(n: f64) -> Option<i64> {
    // `f64 as i64` saturates rather than wrapping, and the saturated value at
    // the positive end converts *back* to the same `f64` — so 2^63 would pass a
    // round-trip test while printing as 2^63 - 1. The range is therefore tested
    // here rather than inferred. 2^63 is the first `f64` with no `i64`; -2^63 is
    // the last one that has. NaN and the infinities fail both comparisons.
    const TWO_POW_63: f64 = 9_223_372_036_854_775_808.0;
    if !(n >= -TWO_POW_63 && n < TWO_POW_63) {
        return None;
    }
    let candidate = n as i64;
    // Exact equality is the question being asked, not an approximation of it:
    // `n` either has a fractional part or it has not. An epsilon here would
    // print 0.5 as 0.
    #[expect(
        clippy::float_cmp,
        reason = "whether a value survives the round trip through i64 is an exact question"
    )]
    let is_whole = candidate as f64 == n;
    is_whole.then_some(candidate)
}

impl NumberFormat {
    /// Format a number according to this format specification.
    pub fn format_number(&self, value: f64) -> String {
        match self {
            Self::General => whole_number(value).map_or_else(
                || {
                    // Remove trailing zeros from decimal representation
                    let s = format!("{value:.10}");
                    let s = s.trim_end_matches('0');
                    let s = s.trim_end_matches('.');
                    s.to_string()
                },
                |int| int.to_string(),
            ),
            Self::Decimal(places) => {
                format!("{:.prec$}", value, prec = *places as usize)
            }
            Self::Percentage(places) => {
                format!("{:.prec$}%", value * 100.0, prec = *places as usize)
            }
            Self::Currency(places) => {
                format!("${:.prec$}", value, prec = *places as usize)
            }
        }
    }
}

// ============================================================================
// Text alignment
// ============================================================================

/// Horizontal text alignment within a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}

// ============================================================================
// Cell borders
// ============================================================================

/// Border configuration for a single cell.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CellBorders {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl CellBorders {
    /// Create borders on all sides.
    pub fn all() -> Self {
        Self {
            top: true,
            bottom: true,
            left: true,
            right: true,
        }
    }

    /// No borders.
    pub fn none() -> Self {
        Self::default()
    }

    /// Check if any border is active.
    pub fn has_any(&self) -> bool {
        self.top || self.bottom || self.left || self.right
    }
}

// ============================================================================
// Cell formatting
// ============================================================================

/// Complete formatting for a single cell.
#[derive(Clone, Debug, PartialEq)]
pub struct CellFormat {
    pub bold: bool,
    pub italic: bool,
    pub alignment: Alignment,
    pub number_format: NumberFormat,
    pub text_color: Option<Color>,
    pub bg_color: Option<Color>,
    pub borders: CellBorders,
}

impl Default for CellFormat {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            alignment: Alignment::Left,
            number_format: NumberFormat::General,
            text_color: None,
            bg_color: None,
            borders: CellBorders::none(),
        }
    }
}

// ============================================================================
// Cell data
// ============================================================================

/// All data associated with a single cell in the spreadsheet.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    /// The raw input (formula text or literal).
    pub raw_input: String,
    /// The computed value after evaluation.
    pub value: CellValue,
    /// Display/number formatting.
    pub format: CellFormat,
}

impl Cell {
    /// Create a new empty cell.
    pub fn empty() -> Self {
        Self {
            raw_input: String::new(),
            value: CellValue::Empty,
            format: CellFormat::default(),
        }
    }

    /// Check whether this cell holds a formula.
    pub fn is_formula(&self) -> bool {
        self.raw_input.starts_with('=')
    }

    /// Display string for the cell value.
    pub fn display_text(&self) -> String {
        self.value.display_string(&self.format.number_format)
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::empty()
    }
}

// ============================================================================
// Cell range
// ============================================================================

/// A module for one type, because privacy in Rust is per-module and this crate
/// is a single one.
///
/// [`CellRange`]'s two corners are ordered — `start` is never past `end` on
/// either axis — and that is what lets `col_count` and `row_count` be
/// subtractions at all. The fields used to be `pub`, which made the
/// normalization done by `CellRange::new` a piece of advice rather than a
/// guarantee: any of this file's six thousand lines could assign a corner
/// directly, and every count would then underflow to near `usize::MAX` — which
/// is one `Vec::with_capacity` away from an abort.
///
/// Making the fields private inside the same module would have changed nothing,
/// since a module can always see its own privates. The `mod` is the enforcement;
/// the `pub` on the fields' accessors is just the interface.
mod cell_range {
    use super::CellAddr;

    /// A rectangular range of cells, with `start` no greater than `end` on both
    /// axes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct CellRange {
        start: CellAddr,
        end: CellAddr,
    }

    impl CellRange {
        /// Create a new range, normalizing so start <= end.
        pub fn new(a: CellAddr, b: CellAddr) -> Self {
            let start = CellAddr::new(a.col.min(b.col), a.row.min(b.row));
            let end = CellAddr::new(a.col.max(b.col), a.row.max(b.row));
            Self { start, end }
        }

        /// Create a single-cell range.
        pub fn single(addr: CellAddr) -> Self {
            Self {
                start: addr,
                end: addr,
            }
        }

        /// The top-left corner. Never past [`CellRange::end`] on either axis.
        pub fn start(&self) -> CellAddr {
            self.start
        }

        /// The bottom-right corner. Never before [`CellRange::start`] on either axis.
        pub fn end(&self) -> CellAddr {
            self.end
        }

        /// Check if a cell address is within this range.
        pub fn contains(&self, addr: CellAddr) -> bool {
            addr.col >= self.start.col
                && addr.col <= self.end.col
                && addr.row >= self.start.row
                && addr.row <= self.end.row
        }

        /// Number of columns in this range. At least one.
        pub fn col_count(&self) -> usize {
            // Sound because the corners are ordered and private; see the type docs.
            self.end
                .col
                .saturating_sub(self.start.col)
                .saturating_add(1)
        }

        /// Number of rows in this range. At least one.
        pub fn row_count(&self) -> usize {
            self.end
                .row
                .saturating_sub(self.start.row)
                .saturating_add(1)
        }

        /// Total number of cells in this range.
        ///
        /// Saturating rather than wrapping: a range is at most `MAX_COLS` by
        /// `MAX_ROWS`, so the product is under 26,000 and the saturation is
        /// unreachable — but a count that silently wraps to a small number is a
        /// worse thing for a caller sizing a buffer to receive than one that is
        /// merely enormous.
        pub fn cell_count(&self) -> usize {
            self.col_count().saturating_mul(self.row_count())
        }

        /// Iterate over all cell addresses in this range (row-major order).
        pub fn iter(&self) -> CellRangeIter {
            CellRangeIter {
                range: *self,
                col: self.start.col,
                row: self.start.row,
            }
        }

        /// Display string like "A1:C5".
        pub fn display(&self) -> String {
            if self.start == self.end {
                self.start.display()
            } else {
                format!("{}:{}", self.start.display(), self.end.display())
            }
        }

        /// Parse a range string like "A1:C5" or a single cell "A1".
        pub fn parse(s: &str) -> Option<Self> {
            // `split_once` gives the two halves without either of them being a
            // byte offset the caller has to keep in step with the other. The old
            // form found `idx` and then sliced `&s[..idx]` and `&s[idx + 1..]`,
            // which is the same offset used three ways.
            match s.split_once(':') {
                Some((left, right)) => {
                    Some(Self::new(CellAddr::parse(left)?, CellAddr::parse(right)?))
                }
                None => Some(Self::single(CellAddr::parse(s)?)),
            }
        }
    }

    impl IntoIterator for &CellRange {
        type Item = CellAddr;
        type IntoIter = CellRangeIter;

        fn into_iter(self) -> Self::IntoIter {
            self.iter()
        }
    }

    /// Iterator over cell addresses in a range.
    pub struct CellRangeIter {
        range: CellRange,
        col: usize,
        row: usize,
    }

    impl Iterator for CellRangeIter {
        type Item = CellAddr;

        fn next(&mut self) -> Option<Self::Item> {
            if self.row > self.range.end.row {
                return None;
            }
            let addr = CellAddr::new(self.col, self.row);
            // Saturating: a cursor pinned at `usize::MAX` is past `end.col`, which
            // is what the wrap below is testing for, so the iterator still
            // terminates rather than cycling.
            self.col = self.col.saturating_add(1);
            if self.col > self.range.end.col {
                self.col = self.range.start.col;
                self.row = self.row.saturating_add(1);
            }
            Some(addr)
        }
    }
}

pub use cell_range::{CellRange, CellRangeIter};

// ============================================================================
// Selection state
// ============================================================================

/// Current selection in the spreadsheet (supports multi-range via Ctrl+click).
#[derive(Clone, Debug)]
pub struct Selection {
    /// Currently active cell (cursor).
    pub active: CellAddr,
    /// All selected ranges.
    pub ranges: Vec<CellRange>,
}

impl Selection {
    /// Create a new selection with a single cell selected.
    pub fn single(addr: CellAddr) -> Self {
        Self {
            active: addr,
            ranges: vec![CellRange::single(addr)],
        }
    }

    /// Check if a cell is within any selected range.
    pub fn contains(&self, addr: CellAddr) -> bool {
        self.ranges.iter().any(|r| r.contains(addr))
    }

    /// Get the primary (first) selected range.
    pub fn primary_range(&self) -> CellRange {
        self.ranges
            .first()
            .copied()
            .unwrap_or_else(|| CellRange::single(self.active))
    }

    /// Collect all numeric values in the selection from a given sheet.
    pub fn numeric_values(&self, sheet: &Sheet) -> Vec<f64> {
        let mut vals = Vec::new();
        for range in &self.ranges {
            for addr in range.iter() {
                if let Some(cell) = sheet.cells.get(&addr)
                    && let Some(n) = cell.value.as_number()
                {
                    vals.push(n);
                }
            }
        }
        vals
    }
}

impl Default for Selection {
    fn default() -> Self {
        Self::single(CellAddr::new(0, 0))
    }
}

// ============================================================================
// Clipboard
// ============================================================================

/// Data stored in the clipboard for copy/cut/paste operations.
#[derive(Clone, Debug)]
pub struct ClipboardData {
    /// The range that was copied.
    pub source_range: CellRange,
    /// Cell data indexed by relative offset from source_range.start.
    pub cells: HashMap<(usize, usize), Cell>,
    /// Whether this was a cut (vs copy) operation.
    pub is_cut: bool,
}

// ============================================================================
// Undo/Redo
// ============================================================================

/// A single undoable action.
#[derive(Clone, Debug)]
pub enum UndoAction {
    /// Cell content changed.
    CellEdit {
        sheet_idx: usize,
        addr: CellAddr,
        old_cell: Cell,
        new_cell: Cell,
    },
    /// Multiple cells changed at once (paste, fill, sort, etc.).
    BatchEdit {
        sheet_idx: usize,
        changes: Vec<(CellAddr, Cell, Cell)>,
    },
    /// Column width changed.
    ColResize {
        sheet_idx: usize,
        col: usize,
        old_width: f32,
        new_width: f32,
    },
    /// Row height changed.
    RowResize {
        sheet_idx: usize,
        row: usize,
        old_height: f32,
        new_height: f32,
    },
    /// Sheet added.
    ///
    /// Carries the sheet, like [`UndoAction::RemoveSheet`] does, so that redo
    /// can put it back. Recording it at *add* time is right rather than
    /// merely convenient: any edit made to the sheet afterwards is its own
    /// action, and the undo stack is unwound in reverse, so by the time this
    /// action's undo runs those edits have already been undone and the sheet is
    /// once again the empty one recorded here.
    AddSheet { sheet_idx: usize, sheet: Sheet },
    /// Sheet removed.
    RemoveSheet { sheet_idx: usize, sheet: Sheet },
}

/// Manages undo/redo stacks.
pub struct UndoManager {
    undo_stack: Vec<UndoAction>,
    redo_stack: Vec<UndoAction>,
}

impl Default for UndoManager {
    fn default() -> Self {
        Self::new()
    }
}

impl UndoManager {
    /// Create a new empty undo manager.
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Record an action for potential undo.
    pub fn push_action(&mut self, action: UndoAction) {
        if self.undo_stack.len() >= UNDO_STACK_LIMIT {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(action);
        self.redo_stack.clear();
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Pop the last undo action.
    pub fn pop_undo(&mut self) -> Option<UndoAction> {
        let action = self.undo_stack.pop()?;
        self.redo_stack.push(action.clone());
        Some(action)
    }

    /// Pop the last redo action.
    pub fn pop_redo(&mut self) -> Option<UndoAction> {
        let action = self.redo_stack.pop()?;
        self.undo_stack.push(action.clone());
        Some(action)
    }

    /// Count of undo actions available.
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Count of redo actions available.
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }
}

// ============================================================================
// Sheet
// ============================================================================

/// A single worksheet within the spreadsheet.
#[derive(Clone, Debug)]
pub struct Sheet {
    /// Sheet name displayed on the tab.
    pub name: String,
    /// Cell data, keyed by address. Only non-empty cells are stored.
    pub cells: BTreeMap<CellAddr, Cell>,
    /// Column widths (indexed by column number).
    pub col_widths: Vec<f32>,
    /// Row heights (indexed by row number).
    pub row_heights: Vec<f32>,
    /// Number of frozen columns (scroll-locked).
    pub frozen_cols: usize,
    /// Number of frozen rows (scroll-locked).
    pub frozen_rows: usize,
    /// What is selected on this sheet, and which cell is active.
    ///
    /// Per sheet for the same reason as `scroll`, and inseparably from it: a
    /// selection held per-window fights a scroll offset held per-sheet, because
    /// switching tabs would restore one sheet's view around another sheet's
    /// active cell — so whichever of the two you reconciled to, the other would
    /// jump. Kept together, a tab switch simply returns the sheet as you left
    /// it, and the "reveal the active cell" rule has nothing to fight.
    pub selection: Selection,
    /// How far this sheet is scrolled, in pixels from its top-left.
    ///
    /// Per sheet, not per window, because the offset only means anything
    /// against a particular sheet's column widths and row heights: 900 px
    /// across is column J on one sheet and column D on another, and past the
    /// end of a third. Held here — beside `frozen_cols`/`frozen_rows`, which
    /// are per-sheet view state for the same reason — the offset shown is
    /// always the one belonging to the sheet being shown, and switching tabs
    /// returns you to where you were rather than to A1.
    ///
    /// The single-offset version it replaces had to be reset on every path
    /// that changed the active sheet, and there are three of them (the tab
    /// bar, adding a sheet, removing one) plus the undo of each. Only the tab
    /// bar remembered.
    pub scroll: ScrollPosition,
}

impl Sheet {
    /// Create a new empty sheet with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            cells: BTreeMap::new(),
            col_widths: vec![DEFAULT_COL_WIDTH; MAX_COLS],
            row_heights: vec![DEFAULT_ROW_HEIGHT; MAX_ROWS],
            frozen_cols: 0,
            frozen_rows: 0,
            selection: Selection::default(),
            scroll: ScrollPosition::new(),
        }
    }

    /// Get a cell, returning a default empty cell if not present.
    pub fn get_cell(&self, addr: CellAddr) -> Cell {
        self.cells.get(&addr).cloned().unwrap_or_default()
    }

    /// Set a cell's raw input, returning the old cell for undo.
    pub fn set_cell_input(&mut self, addr: CellAddr, input: &str) -> Cell {
        let old = self.get_cell(addr);
        let mut cell = Cell::empty();
        cell.raw_input = input.to_string();
        cell.format = old.format.clone();

        if input.is_empty() {
            cell.value = CellValue::Empty;
        } else if input.starts_with('=') {
            // Formula — defer evaluation to the engine
            cell.value = CellValue::Empty;
        } else if let Ok(n) = input.parse::<f64>() {
            cell.value = CellValue::Number(n);
        } else if input.eq_ignore_ascii_case("true") {
            cell.value = CellValue::Boolean(true);
        } else if input.eq_ignore_ascii_case("false") {
            cell.value = CellValue::Boolean(false);
        } else {
            cell.value = CellValue::Text(input.to_string());
        }

        if cell.value.is_empty() && cell.raw_input.is_empty() {
            self.cells.remove(&addr);
        } else {
            self.cells.insert(addr, cell);
        }
        old
    }

    /// Set a cell directly (used by undo/redo).
    pub fn set_cell(&mut self, addr: CellAddr, cell: Cell) {
        if cell.value.is_empty() && cell.raw_input.is_empty() {
            self.cells.remove(&addr);
        } else {
            self.cells.insert(addr, cell);
        }
    }

    /// Get the X offset for a given column, accounting for widths.
    pub fn col_x_offset(&self, col: usize) -> f32 {
        let mut x = 0.0;
        for c in 0..col.min(MAX_COLS) {
            x += self.col_widths.get(c).copied().unwrap_or(DEFAULT_COL_WIDTH);
        }
        x
    }

    /// Get the Y offset for a given row, accounting for heights.
    pub fn row_y_offset(&self, row: usize) -> f32 {
        let mut y = 0.0;
        for r in 0..row.min(MAX_ROWS) {
            y += self
                .row_heights
                .get(r)
                .copied()
                .unwrap_or(DEFAULT_ROW_HEIGHT);
        }
        y
    }

    /// Get column width.
    pub fn col_width(&self, col: usize) -> f32 {
        self.col_widths
            .get(col)
            .copied()
            .unwrap_or(DEFAULT_COL_WIDTH)
    }

    /// Get row height.
    pub fn row_height(&self, row: usize) -> f32 {
        self.row_heights
            .get(row)
            .copied()
            .unwrap_or(DEFAULT_ROW_HEIGHT)
    }

    /// Find which column a given X position falls in.
    pub fn col_at_x(&self, x: f32) -> usize {
        let mut acc = 0.0;
        for c in 0..MAX_COLS {
            acc += self.col_width(c);
            if x < acc {
                return c;
            }
        }
        MAX_COLS.saturating_sub(1)
    }

    /// Find which row a given Y position falls in.
    pub fn row_at_y(&self, y: f32) -> usize {
        let mut acc = 0.0;
        for r in 0..MAX_ROWS {
            acc += self.row_height(r);
            if y < acc {
                return r;
            }
        }
        MAX_ROWS.saturating_sub(1)
    }

    /// Sort rows by a given column within a range.
    pub fn sort_by_column(
        &mut self,
        col: usize,
        start_row: usize,
        end_row: usize,
        ascending: bool,
    ) -> Vec<(CellAddr, Cell, Cell)> {
        if start_row >= end_row || end_row >= MAX_ROWS {
            return Vec::new();
        }

        // Collect row data
        let mut rows: Vec<(usize, Option<f64>, String)> = (start_row..=end_row)
            .map(|r| {
                let cell = self.get_cell(CellAddr::new(col, r));
                let num = cell.value.as_number();
                let text = cell.display_text();
                (r, num, text)
            })
            .collect();

        // Sort
        rows.sort_by(|a, b| {
            let ordering = match (&a.1, &b.1) {
                (Some(na), Some(nb)) => na.partial_cmp(nb).unwrap_or(std::cmp::Ordering::Equal),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.2.cmp(&b.2),
            };
            if ascending {
                ordering
            } else {
                ordering.reverse()
            }
        });

        let sorted_indices: Vec<usize> = rows.iter().map(|(r, _, _)| *r).collect();
        let mut changes = Vec::new();

        // Collect all row data before making changes
        let all_row_data: Vec<Vec<(CellAddr, Cell)>> = sorted_indices
            .iter()
            .map(|&src_row| {
                (0..MAX_COLS)
                    .map(|c| {
                        let addr = CellAddr::new(c, src_row);
                        (addr, self.get_cell(addr))
                    })
                    .collect()
            })
            .collect();

        // Apply sorted data.
        //
        // Zipping against `start_row..=end_row` rather than adding an enumerate
        // offset to `start_row`: the rows written are by definition the rows
        // read, and saying so with the same range object makes the two agree
        // without an addition to bound.
        for (dest_row, row_data) in (start_row..=end_row).zip(&all_row_data) {
            // `enumerate` rather than recovering the column by searching
            // `row_data` for a pointer equal to `src_cell`. The search returned
            // the same number the loop already had, at quadratic cost — and its
            // `.unwrap_or(0)` meant that a miss would silently write the cell to
            // column A instead of failing.
            for (col_idx, (_, src_cell)) in row_data.iter().enumerate() {
                let dest_addr = CellAddr::new(col_idx, dest_row);
                let old = self.get_cell(dest_addr);
                if *src_cell != old {
                    changes.push((dest_addr, old, src_cell.clone()));
                    self.set_cell(dest_addr, src_cell.clone());
                }
            }
        }

        changes
    }

    /// Export sheet data as CSV string.
    pub fn export_csv(&self) -> String {
        let mut result = String::new();
        let max_row = self.cells.keys().map(|a| a.row).max().unwrap_or(0);
        let max_col = self.cells.keys().map(|a| a.col).max().unwrap_or(0);

        for r in 0..=max_row {
            for c in 0..=max_col {
                if c > 0 {
                    result.push(',');
                }
                let cell = self.get_cell(CellAddr::new(c, r));
                // The hand-rolled trigger set this replaces omitted `\r`.
                // RFC 4180 records are CRLF-terminated, so a bare CR left in
                // an unquoted field splits the record for most readers.
                result.push_str(&guitk::csv::field(&cell.display_text()));
            }
            result.push('\n');
        }
        result
    }

    /// Import CSV data into the sheet, returning batch changes for undo.
    pub fn import_csv(&mut self, csv: &str) -> Vec<(CellAddr, Cell, Cell)> {
        let mut changes = Vec::new();
        // Record-oriented, not line-oriented: a quoted cell may contain a
        // newline, and splitting on `\n` first tore such a cell in half and
        // dropped the rest of its row -- so the sheet could not read back its
        // own export.
        for (row_idx, record) in guitk::csv::parse_records(csv).into_iter().enumerate() {
            if row_idx >= MAX_ROWS {
                break;
            }
            for (col_idx, field) in record.into_iter().enumerate() {
                if col_idx >= MAX_COLS {
                    break;
                }
                let addr = CellAddr::new(col_idx, row_idx);
                let old = self.set_cell_input(addr, &field.text);
                let new_cell = self.get_cell(addr);
                if old != new_cell {
                    changes.push((addr, old, new_cell));
                }
            }
        }
        changes
    }
}

// ============================================================================
// Formula engine — tokenizer
// ============================================================================

/// Token types for the formula parser.
#[derive(Clone, Debug, PartialEq)]
enum FormulaToken {
    Number(f64),
    StringLiteral(String),
    CellRef(CellAddr),
    RangeRef(CellAddr, CellAddr),
    Plus,
    Minus,
    Multiply,
    Divide,
    LeftParen,
    RightParen,
    Comma,
    Equals,
    NotEquals,
    LessThan,
    GreaterThan,
    LessEq,
    GreaterEq,
    Ampersand,
    FuncName(String),
    Boolean(bool),
}

/// A cursor over a formula's characters.
///
/// Every read of the input goes through one of the four methods below, and each
/// states its bound at the point of the read. The tokenizer used to carry a
/// bare `pos` and restate the bound at each use — `if pos + 1 < length &&
/// chars[pos + 1] == '='`, sixteen times — which is one bound written twice,
/// once as a guard and once as an index, and in four places the guard was for a
/// different offset than the index beside it.
///
/// Characters, not bytes: a formula is user-typed text and may contain any of
/// it, so `String` byte offsets would be the wrong unit even where they did not
/// panic.
struct FormulaCursor {
    chars: Vec<char>,
    pos: usize,
}

impl FormulaCursor {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    /// The character under the cursor, or `None` at the end of the input.
    fn peek(&self) -> Option<char> {
        self.peek_at(0)
    }

    /// The character `offset` positions past the cursor.
    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos.checked_add(offset)?).copied()
    }

    /// Move the cursor forward by `n`, stopping at the end of the input.
    fn skip(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n).min(self.chars.len());
    }

    /// Consume and return the run of characters satisfying `keep`.
    fn take_while(&mut self, keep: impl Fn(char) -> bool) -> String {
        let mut taken = String::new();
        while let Some(ch) = self.peek() {
            if !keep(ch) {
                break;
            }
            taken.push(ch);
            self.skip(1);
        }
        taken
    }
}

/// Tokenize a formula string (without the leading '=').
fn tokenize_formula(input: &str) -> Result<Vec<FormulaToken>, CellError> {
    let mut tokens = Vec::new();
    let mut cur = FormulaCursor::new(input);

    while let Some(ch) = cur.peek() {
        // Every arm is responsible for advancing past what it consumed. The
        // single-character arms are gathered here so that "one character, one
        // token" is stated once rather than as twelve `pos += 1`s.
        let single = match ch {
            ' ' | '\t' => Some(None),
            '+' => Some(Some(FormulaToken::Plus)),
            '-' => Some(Some(FormulaToken::Minus)),
            '*' => Some(Some(FormulaToken::Multiply)),
            '/' => Some(Some(FormulaToken::Divide)),
            '(' => Some(Some(FormulaToken::LeftParen)),
            ')' => Some(Some(FormulaToken::RightParen)),
            ',' => Some(Some(FormulaToken::Comma)),
            '&' => Some(Some(FormulaToken::Ampersand)),
            '=' => Some(Some(FormulaToken::Equals)),
            _ => None,
        };
        if let Some(token) = single {
            cur.skip(1);
            tokens.extend(token);
            continue;
        }

        match ch {
            '<' => match cur.peek_at(1) {
                Some('=') => {
                    tokens.push(FormulaToken::LessEq);
                    cur.skip(2);
                }
                Some('>') => {
                    tokens.push(FormulaToken::NotEquals);
                    cur.skip(2);
                }
                _ => {
                    tokens.push(FormulaToken::LessThan);
                    cur.skip(1);
                }
            },
            '>' => {
                if cur.peek_at(1) == Some('=') {
                    tokens.push(FormulaToken::GreaterEq);
                    cur.skip(2);
                } else {
                    tokens.push(FormulaToken::GreaterThan);
                    cur.skip(1);
                }
            }
            '"' => {
                cur.skip(1);
                let literal = cur.take_while(|c| c != '"');
                // An unterminated string literal ends at the end of the input
                // rather than being an error, which is what a spreadsheet does
                // while the user is still typing the closing quote.
                cur.skip(1);
                tokens.push(FormulaToken::StringLiteral(literal));
            }
            _ if ch.is_ascii_digit() || ch == '.' => {
                let num_str = cur.take_while(|c| c.is_ascii_digit() || c == '.');
                let val: f64 = num_str.parse().map_err(|_| CellError::InvalidFormula)?;
                tokens.push(FormulaToken::Number(val));
            }
            _ if ch.is_ascii_alphabetic() => {
                let word = cur.take_while(|c| c.is_ascii_alphanumeric() || c == '_');
                let upper = word.to_ascii_uppercase();

                // Check for boolean literals
                if upper == "TRUE" {
                    tokens.push(FormulaToken::Boolean(true));
                } else if upper == "FALSE" {
                    tokens.push(FormulaToken::Boolean(false));
                }
                // Check if this is a cell reference potentially followed by ':'
                else if let Some(addr) = CellAddr::parse(&upper) {
                    // Check for range reference
                    if cur.peek() == Some(':') {
                        cur.skip(1);
                        let end_word = cur.take_while(|c| c.is_ascii_alphanumeric());
                        let end_addr =
                            CellAddr::parse(&end_word).ok_or(CellError::InvalidReference)?;
                        tokens.push(FormulaToken::RangeRef(addr, end_addr));
                    } else {
                        tokens.push(FormulaToken::CellRef(addr));
                    }
                }
                // Check if followed by '(' — function call
                else if cur.peek() == Some('(') {
                    tokens.push(FormulaToken::FuncName(upper));
                } else {
                    return Err(CellError::NameError);
                }
            }
            _ => {
                return Err(CellError::InvalidFormula);
            }
        }
    }
    Ok(tokens)
}

// ============================================================================
// Formula engine — parser and evaluator
// ============================================================================

/// Recursive-descent parser context for formula evaluation.
/// How deep the formula evaluator will recurse before giving up.
///
/// One budget covers both parenthesis nesting and chains of referring cells —
/// see [`FormulaEvaluator::parse_comparison`]. A hundred is far more than any
/// document a person writes and far less than the stack can take, which is the
/// right side of both errors to be on.
const MAX_EVAL_DEPTH: usize = 100;

pub struct FormulaEvaluator<'a> {
    tokens: Vec<FormulaToken>,
    pos: usize,
    sheet: &'a Sheet,
    eval_depth: usize,
    visited: Vec<CellAddr>,
}

impl<'a> FormulaEvaluator<'a> {
    /// Create a new evaluator for the given tokens and sheet context.
    fn new(tokens: Vec<FormulaToken>, sheet: &'a Sheet) -> Self {
        Self {
            tokens,
            pos: 0,
            sheet,
            eval_depth: 0,
            visited: Vec::new(),
        }
    }

    /// Evaluate the formula and return the result.
    pub fn evaluate(&mut self) -> Result<CellValue, CellError> {
        if self.tokens.is_empty() {
            return Ok(CellValue::Empty);
        }
        let result = self.parse_comparison()?;
        if self.pos < self.tokens.len() {
            // Check for string concatenation with &
            if self.peek() == Some(&FormulaToken::Ampersand) {
                return self.parse_concatenation_from(result);
            }
        }
        Ok(result)
    }

    /// Parse concatenation expressions (using &).
    fn parse_concatenation_from(&mut self, left: CellValue) -> Result<CellValue, CellError> {
        let mut result = value_to_string(&left);
        while self.peek() == Some(&FormulaToken::Ampersand) {
            self.advance();
            let right = self.parse_comparison()?;
            result.push_str(&value_to_string(&right));
        }
        Ok(CellValue::Text(result))
    }

    /// Parse a sub-expression one level deeper, refusing to go past
    /// [`MAX_EVAL_DEPTH`].
    ///
    /// **Every** recursive step in this evaluator passes through here, because
    /// every one of them re-enters the grammar at `parse_comparison`: a
    /// parenthesised group, each argument of a function call, and following a
    /// cell reference into the formula of the cell it names.
    ///
    /// It did not used to. The counter was incremented only in
    /// [`FormulaEvaluator::resolve_cell`], so nothing at all bounded the
    /// parenthesis case — `=((((((…1…))))))` recursed once per parenthesis
    /// until the stack ran out. The tokens come from a cell's text, whose
    /// length is the document's to choose, and a stack overflow is not a
    /// `CellError` the caller can render in the cell: it takes the process out,
    /// losing the whole workbook.
    ///
    /// The budget is shared between the two kinds of nesting rather than
    /// counted separately, because it stands for one thing — the depth of the
    /// Rust stack — and it does not care which grammar production put a frame
    /// there.
    fn parse_comparison(&mut self) -> Result<CellValue, CellError> {
        let restore = self.eval_depth;
        let next = restore.checked_add(1).ok_or(CellError::TooDeep)?;
        if next > MAX_EVAL_DEPTH {
            return Err(CellError::TooDeep);
        }
        self.eval_depth = next;
        let result = self.parse_comparison_inner();
        // Not `-= 1`: restoring the saved value is correct even if the body
        // left the counter somewhere unexpected, and there is no subtraction to
        // justify.
        self.eval_depth = restore;
        result
    }

    /// Parse comparison expressions (=, <>, <, >, <=, >=).
    fn parse_comparison_inner(&mut self) -> Result<CellValue, CellError> {
        let left = self.parse_addition()?;
        match self.peek().cloned() {
            Some(FormulaToken::Equals) => {
                self.advance();
                let right = self.parse_addition()?;
                Ok(CellValue::Boolean(values_equal(&left, &right)))
            }
            Some(FormulaToken::NotEquals) => {
                self.advance();
                let right = self.parse_addition()?;
                Ok(CellValue::Boolean(!values_equal(&left, &right)))
            }
            Some(FormulaToken::LessThan) => {
                self.advance();
                let right = self.parse_addition()?;
                Ok(CellValue::Boolean(compare_values(&left, &right)? < 0))
            }
            Some(FormulaToken::GreaterThan) => {
                self.advance();
                let right = self.parse_addition()?;
                Ok(CellValue::Boolean(compare_values(&left, &right)? > 0))
            }
            Some(FormulaToken::LessEq) => {
                self.advance();
                let right = self.parse_addition()?;
                Ok(CellValue::Boolean(compare_values(&left, &right)? <= 0))
            }
            Some(FormulaToken::GreaterEq) => {
                self.advance();
                let right = self.parse_addition()?;
                Ok(CellValue::Boolean(compare_values(&left, &right)? >= 0))
            }
            _ => Ok(left),
        }
    }

    /// Parse addition/subtraction expressions.
    fn parse_addition(&mut self) -> Result<CellValue, CellError> {
        let mut left = self.parse_multiplication()?;
        loop {
            match self.peek().cloned() {
                Some(FormulaToken::Plus) => {
                    self.advance();
                    let right = self.parse_multiplication()?;
                    let a = require_number(&left)?;
                    let b = require_number(&right)?;
                    left = CellValue::Number(a + b);
                }
                Some(FormulaToken::Minus) => {
                    self.advance();
                    let right = self.parse_multiplication()?;
                    let a = require_number(&left)?;
                    let b = require_number(&right)?;
                    left = CellValue::Number(a - b);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    /// Parse multiplication/division expressions.
    fn parse_multiplication(&mut self) -> Result<CellValue, CellError> {
        let mut left = self.parse_unary()?;
        loop {
            match self.peek().cloned() {
                Some(FormulaToken::Multiply) => {
                    self.advance();
                    let right = self.parse_unary()?;
                    let a = require_number(&left)?;
                    let b = require_number(&right)?;
                    left = CellValue::Number(a * b);
                }
                Some(FormulaToken::Divide) => {
                    self.advance();
                    let right = self.parse_unary()?;
                    let a = require_number(&left)?;
                    let b = require_number(&right)?;
                    if b == 0.0 {
                        return Err(CellError::DivisionByZero);
                    }
                    left = CellValue::Number(a / b);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    /// Parse unary minus/plus.
    fn parse_unary(&mut self) -> Result<CellValue, CellError> {
        match self.peek().cloned() {
            Some(FormulaToken::Minus) => {
                self.advance();
                let val = self.parse_primary()?;
                let n = require_number(&val)?;
                Ok(CellValue::Number(-n))
            }
            Some(FormulaToken::Plus) => {
                self.advance();
                self.parse_primary()
            }
            _ => self.parse_primary(),
        }
    }

    /// Parse primary expressions (numbers, strings, cell refs, function calls, parens).
    fn parse_primary(&mut self) -> Result<CellValue, CellError> {
        match self.peek().cloned() {
            Some(FormulaToken::Number(n)) => {
                self.advance();
                Ok(CellValue::Number(n))
            }
            Some(FormulaToken::StringLiteral(s)) => {
                self.advance();
                Ok(CellValue::Text(s))
            }
            Some(FormulaToken::Boolean(b)) => {
                self.advance();
                Ok(CellValue::Boolean(b))
            }
            Some(FormulaToken::CellRef(addr)) => {
                self.advance();
                self.resolve_cell(addr)
            }
            Some(FormulaToken::LeftParen) => {
                self.advance();
                let val = self.parse_comparison()?;
                self.expect_token(&FormulaToken::RightParen)?;
                Ok(val)
            }
            Some(FormulaToken::FuncName(name)) => {
                self.advance();
                self.parse_function_call(&name)
            }
            _ => Err(CellError::InvalidFormula),
        }
    }

    /// Resolve a cell reference to its value.
    fn resolve_cell(&mut self, addr: CellAddr) -> Result<CellValue, CellError> {
        if self.visited.contains(&addr) {
            return Err(CellError::CircularReference);
        }
        let cell = self.sheet.get_cell(addr);
        // `strip_prefix` rather than `&raw_input[1..]`: the slice was only ever
        // correct because `is_formula` had just established that byte 0 is `=`,
        // which is the bound stated in one statement and used in another. This
        // states it once, and the `else` branch is then the same "not a
        // formula" case the `if` was already testing for.
        let Some(formula_text) = cell.raw_input.strip_prefix('=') else {
            return Ok(cell.value.clone());
        };
        // Depth is counted in `parse_comparison`, which the sub-evaluator will
        // enter; this seeds it with the depth already spent so that a chain of
        // referring cells and a nest of parentheses draw on one budget.
        // Tokenize before pushing: `?` here would otherwise leave `addr` on the
        // visited path forever, and every later reference to it in this
        // evaluation would report a cycle that does not exist.
        let sub_tokens = tokenize_formula(formula_text)?;
        self.visited.push(addr);
        let mut sub_eval = FormulaEvaluator {
            tokens: sub_tokens,
            pos: 0,
            sheet: self.sheet,
            eval_depth: self.eval_depth,
            visited: self.visited.clone(),
        };
        let result = sub_eval.evaluate();
        let _ = self.visited.pop();
        result
    }

    /// Collect numeric values from a range for aggregate functions.
    fn collect_range_numbers(
        &mut self,
        start: CellAddr,
        end: CellAddr,
    ) -> Result<Vec<f64>, CellError> {
        let range = CellRange::new(start, end);
        let mut values = Vec::new();
        for addr in range.iter() {
            let val = self.resolve_cell(addr)?;
            if let Some(n) = val.as_number() {
                values.push(n);
            }
        }
        Ok(values)
    }

    /// Collect all values from a range (for COUNT, etc.).
    fn collect_range_values(
        &mut self,
        start: CellAddr,
        end: CellAddr,
    ) -> Result<Vec<CellValue>, CellError> {
        let range = CellRange::new(start, end);
        let mut values = Vec::new();
        for addr in range.iter() {
            let val = self.resolve_cell(addr)?;
            values.push(val);
        }
        Ok(values)
    }

    /// Parse and evaluate a function call.
    fn parse_function_call(&mut self, name: &str) -> Result<CellValue, CellError> {
        self.expect_token(&FormulaToken::LeftParen)?;

        match name {
            "SUM" => self.eval_aggregate_func(|nums| nums.iter().sum()),
            "AVG" | "AVERAGE" => self.eval_aggregate_func(|nums| {
                if nums.is_empty() {
                    0.0
                } else {
                    nums.iter().sum::<f64>() / nums.len() as f64
                }
            }),
            "MIN" => {
                self.eval_aggregate_func(|nums| nums.iter().copied().fold(f64::INFINITY, f64::min))
            }
            "MAX" => self
                .eval_aggregate_func(|nums| nums.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
            "COUNT" => self.eval_count_func(),
            "IF" => self.eval_if_func(),
            "ABS" => {
                let val = self.parse_comparison()?;
                self.expect_token(&FormulaToken::RightParen)?;
                let n = require_number(&val)?;
                Ok(CellValue::Number(n.abs()))
            }
            "ROUND" => {
                let val = self.parse_comparison()?;
                let places = if self.peek() == Some(&FormulaToken::Comma) {
                    self.advance();
                    let p = self.parse_comparison()?;
                    require_number(&p)? as i32
                } else {
                    0
                };
                self.expect_token(&FormulaToken::RightParen)?;
                let n = require_number(&val)?;
                let factor = 10f64.powi(places);
                Ok(CellValue::Number((n * factor).round() / factor))
            }
            "CONCATENATE" | "CONCAT" => self.eval_concatenate_func(),
            "LEN" => {
                let val = self.parse_comparison()?;
                self.expect_token(&FormulaToken::RightParen)?;
                let s = value_to_string(&val);
                Ok(CellValue::Number(s.len() as f64))
            }
            "UPPER" => {
                let val = self.parse_comparison()?;
                self.expect_token(&FormulaToken::RightParen)?;
                let s = value_to_string(&val);
                Ok(CellValue::Text(s.to_uppercase()))
            }
            "LOWER" => {
                let val = self.parse_comparison()?;
                self.expect_token(&FormulaToken::RightParen)?;
                let s = value_to_string(&val);
                Ok(CellValue::Text(s.to_lowercase()))
            }
            _ => Err(CellError::NameError),
        }
    }

    /// Evaluate an aggregate function (SUM, AVG, MIN, MAX) that collects numbers.
    fn eval_aggregate_func<F>(&mut self, func: F) -> Result<CellValue, CellError>
    where
        F: FnOnce(&[f64]) -> f64,
    {
        let mut all_nums = Vec::new();
        loop {
            match self.peek().cloned() {
                Some(FormulaToken::RangeRef(start, end)) => {
                    self.advance();
                    let nums = self.collect_range_numbers(start, end)?;
                    all_nums.extend(nums);
                }
                Some(FormulaToken::RightParen) => break,
                _ => {
                    let val = self.parse_comparison()?;
                    if let Some(n) = val.as_number() {
                        all_nums.push(n);
                    }
                }
            }
            if self.peek() == Some(&FormulaToken::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect_token(&FormulaToken::RightParen)?;
        Ok(CellValue::Number(func(&all_nums)))
    }

    /// Evaluate the COUNT function.
    fn eval_count_func(&mut self) -> Result<CellValue, CellError> {
        let mut count: usize = 0;
        loop {
            match self.peek().cloned() {
                Some(FormulaToken::RangeRef(start, end)) => {
                    self.advance();
                    let vals = self.collect_range_values(start, end)?;
                    // Saturating: a range is at most `MAX_COLS * MAX_ROWS`
                    // cells, so the sum cannot approach `usize::MAX` — but a
                    // count that wraps to a small number is worse than one that
                    // stops climbing, because only the second is obviously wrong
                    // to whoever reads the cell.
                    count = count.saturating_add(vals.iter().filter(|v| !v.is_empty()).count());
                }
                Some(FormulaToken::RightParen) => break,
                _ => {
                    let val = self.parse_comparison()?;
                    if !val.is_empty() {
                        count = count.saturating_add(1);
                    }
                }
            }
            if self.peek() == Some(&FormulaToken::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect_token(&FormulaToken::RightParen)?;
        Ok(CellValue::Number(count as f64))
    }

    /// Evaluate the IF function: IF(condition, value_if_true, value_if_false).
    fn eval_if_func(&mut self) -> Result<CellValue, CellError> {
        let condition = self.parse_comparison()?;
        self.expect_comma()?;
        let true_val = self.parse_comparison()?;
        let false_val = if self.peek() == Some(&FormulaToken::Comma) {
            self.advance();
            self.parse_comparison()?
        } else {
            CellValue::Boolean(false)
        };
        self.expect_token(&FormulaToken::RightParen)?;

        let is_true = match &condition {
            CellValue::Boolean(b) => *b,
            CellValue::Number(n) => *n != 0.0,
            CellValue::Text(s) => !s.is_empty(),
            _ => false,
        };

        Ok(if is_true { true_val } else { false_val })
    }

    /// Evaluate the CONCATENATE function.
    fn eval_concatenate_func(&mut self) -> Result<CellValue, CellError> {
        let mut result = String::new();
        loop {
            if self.peek() == Some(&FormulaToken::RightParen) {
                break;
            }
            let val = self.parse_comparison()?;
            result.push_str(&value_to_string(&val));
            if self.peek() == Some(&FormulaToken::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect_token(&FormulaToken::RightParen)?;
        Ok(CellValue::Text(result))
    }

    /// Peek at the current token without consuming.
    fn peek(&self) -> Option<&FormulaToken> {
        self.tokens.get(self.pos)
    }

    /// Advance to the next token, stopping at the end.
    ///
    /// The clamp is what makes [`FormulaEvaluator::peek`] a plain `get`: the
    /// cursor is never further than one past the last token, so it is either a
    /// valid index or the end, and never some third thing a caller has to test
    /// for.
    fn advance(&mut self) {
        self.pos = self.pos.saturating_add(1).min(self.tokens.len());
    }

    /// Expect a specific token, consuming it if matched.
    fn expect_token(&mut self, expected: &FormulaToken) -> Result<(), CellError> {
        // Compare discriminants only for structural tokens
        match self.peek() {
            Some(tok) if std::mem::discriminant(tok) == std::mem::discriminant(expected) => {
                self.advance();
                Ok(())
            }
            _ => Err(CellError::InvalidFormula),
        }
    }

    /// Expect and consume a comma token.
    fn expect_comma(&mut self) -> Result<(), CellError> {
        self.expect_token(&FormulaToken::Comma)
    }
}

/// Convert a cell value to a display string for concatenation.
pub fn value_to_string(val: &CellValue) -> String {
    match val {
        CellValue::Empty => String::new(),
        CellValue::Text(s) => s.clone(),
        CellValue::Number(n) => {
            whole_number(*n).map_or_else(|| format!("{n}"), |int| int.to_string())
        }
        CellValue::Boolean(b) => {
            if *b {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        CellValue::Error(e) => e.display().to_string(),
    }
}

/// Require a cell value to be numeric, returning an error otherwise.
pub fn require_number(val: &CellValue) -> Result<f64, CellError> {
    val.as_number().ok_or(CellError::ValueError)
}

/// Compare two cell values for ordering, returning -1, 0, or 1.
pub fn compare_values(a: &CellValue, b: &CellValue) -> Result<i32, CellError> {
    match (a.as_number(), b.as_number()) {
        (Some(na), Some(nb)) => Ok(na.partial_cmp(&nb).map(|o| o as i32).unwrap_or(0)),
        _ => {
            let sa = value_to_string(a);
            let sb = value_to_string(b);
            Ok(sa.cmp(&sb) as i32)
        }
    }
}

/// Check if two cell values are equal.
pub fn values_equal(a: &CellValue, b: &CellValue) -> bool {
    match (a, b) {
        (CellValue::Number(na), CellValue::Number(nb)) => (na - nb).abs() < 1e-10,
        (CellValue::Text(sa), CellValue::Text(sb)) => sa.eq_ignore_ascii_case(sb),
        (CellValue::Boolean(ba), CellValue::Boolean(bb)) => ba == bb,
        (CellValue::Empty, CellValue::Empty) => true,
        _ => false,
    }
}

/// Evaluate a formula string in the context of a sheet.
pub fn evaluate_formula(formula: &str, sheet: &Sheet) -> CellValue {
    if !formula.starts_with('=') {
        return CellValue::Error(CellError::InvalidFormula);
    }
    let formula_body = &formula[1..];
    let tokens = match tokenize_formula(formula_body) {
        Ok(t) => t,
        Err(e) => return CellValue::Error(e),
    };
    let mut evaluator = FormulaEvaluator::new(tokens, sheet);
    match evaluator.evaluate() {
        Ok(val) => val,
        Err(e) => CellValue::Error(e),
    }
}

/// Recalculate all formula cells in a sheet.
pub fn recalculate_sheet(sheet: &mut Sheet) {
    // Collect all formula addresses first to avoid borrow issues
    let formula_addrs: Vec<CellAddr> = sheet
        .cells
        .iter()
        .filter(|(_, c)| c.is_formula())
        .map(|(a, _)| *a)
        .collect();

    // Create a snapshot for evaluation
    let snapshot = sheet.clone();

    for addr in formula_addrs {
        if let Some(cell) = sheet.cells.get_mut(&addr) {
            let val = evaluate_formula(&cell.raw_input, &snapshot);
            cell.value = val;
        }
    }
}

// ============================================================================
// Auto-fill logic
// ============================================================================

/// Fill position `index` by repeating `values` cyclically.
///
/// This is the answer whenever the source is not an arithmetic series — which
/// includes a single value and any text — and it used to be written out three
/// separate times, each as an `index % values.len()` followed by an index into
/// `values`. `checked_rem` folds the "there is something to repeat" test into
/// the operation that needs it, so the empty case cannot be forgotten at one of
/// the three sites.
fn repeat_pattern(values: &[CellValue], index: usize) -> CellValue {
    index
        .checked_rem(values.len())
        .and_then(|slot| values.get(slot))
        .cloned()
        .unwrap_or(CellValue::Empty)
}

/// Detect a numeric series and produce the next value.
pub fn auto_fill_next(values: &[CellValue], index: usize) -> CellValue {
    // A series needs every value to be a number, and needs at least two of them
    // to have a step at all. Collecting into `Option<Vec<_>>` makes "all of them
    // are numbers" the same statement as "here they are", rather than a
    // separate `all(is_some)` pass followed by an `unwrap_or(0.0)` that would
    // quietly substitute a zero if the two ever disagreed.
    let Some(numbers) = values
        .iter()
        .map(CellValue::as_number)
        .collect::<Option<Vec<f64>>>()
    else {
        return repeat_pattern(values, index);
    };
    let (Some(&first), Some(&second), Some(&last)) =
        (numbers.first(), numbers.get(1), numbers.last())
    else {
        return repeat_pattern(values, index);
    };

    let step = second - first;
    let is_arithmetic = numbers.windows(2).all(|pair| match pair {
        [a, b] => (b - a - step).abs() < 1e-10,
        // `windows(2)` yields nothing else; a series of one is not a series.
        _ => false,
    });
    if !is_arithmetic {
        return repeat_pattern(values, index);
    }

    let steps_ahead = index as f64 + 1.0;
    CellValue::Number(last + step * steps_ahead)
}

// ============================================================================
// Find and Replace
// ============================================================================

/// State for find-and-replace operations.
#[derive(Clone, Debug)]
pub struct FindReplace {
    pub search_text: String,
    pub replace_text: String,
    pub case_sensitive: bool,
    pub active: bool,
    pub results: Vec<CellAddr>,
    pub current_result: usize,
}

impl Default for FindReplace {
    fn default() -> Self {
        Self::new()
    }
}

impl FindReplace {
    /// Create a new, inactive find/replace state.
    pub fn new() -> Self {
        Self {
            search_text: String::new(),
            replace_text: String::new(),
            case_sensitive: false,
            active: false,
            results: Vec::new(),
            current_result: 0,
        }
    }

    /// Search for occurrences in a sheet.
    pub fn find_all(&mut self, sheet: &Sheet) {
        self.results.clear();
        self.current_result = 0;
        if self.search_text.is_empty() {
            return;
        }
        // Folding both sides into fresh `String`s allocated twice per cell and
        // got the answer wrong for any character whose folded form is a
        // different length; `textfind` compares the folded forms as it walks,
        // allocating neither.
        let case = textfind::Case::sensitive(self.case_sensitive);
        for (&addr, cell) in &sheet.cells {
            if textfind::contains(&cell.display_text(), &self.search_text, case) {
                self.results.push(addr);
            }
        }
    }

    /// The result the cursor is on, or `None` if there are no results.
    ///
    /// `current_result` is an index into `results`, and "it is always in range"
    /// used to be a convention kept by five methods rather than a property of
    /// the pair — each of them tested `results.is_empty()` and then indexed,
    /// which is only sound because of what the *other four* do. Reading through
    /// one `get` makes the out-of-range case an answer instead of a panic.
    pub fn current(&self) -> Option<CellAddr> {
        self.results.get(self.current_result).copied()
    }

    /// Move to the next search result.
    pub fn next_result(&mut self) -> Option<CellAddr> {
        let count = self.results.len();
        // `checked_rem` is the emptiness test: there is no next result in a
        // list of none, and no remainder modulo zero.
        self.current_result = self.current_result.saturating_add(1).checked_rem(count)?;
        self.current()
    }

    /// Move to the previous search result.
    pub fn prev_result(&mut self) -> Option<CellAddr> {
        let last = self.results.len().checked_sub(1)?;
        self.current_result = self
            .current_result
            .checked_sub(1)
            // Wrapping past the start lands on the last result, and clamping
            // to `last` also repairs a cursor that was somehow already past
            // the end rather than carrying it forward.
            .map_or(last, |prev| prev.min(last));
        self.current()
    }

    /// Replace current match and advance.
    pub fn replace_current(&mut self, sheet: &mut Sheet) -> Option<(CellAddr, Cell, Cell)> {
        let addr = self.current()?;
        let cell = sheet.get_cell(addr);
        let old_text = if cell.is_formula() {
            cell.raw_input.clone()
        } else {
            cell.display_text()
        };

        let new_text = if self.case_sensitive {
            old_text.replace(&self.search_text, &self.replace_text)
        } else {
            case_insensitive_replace(&old_text, &self.search_text, &self.replace_text)
        };

        let old = sheet.set_cell_input(addr, &new_text);
        let new_cell = sheet.get_cell(addr);
        // The cell no longer matches, so it leaves the result list. `current()`
        // above is what establishes that this index is in range.
        self.results.remove(self.current_result);
        // Keep the cursor on a real result: taking out the last one wraps to
        // the front. The old form only did this when the list was non-empty,
        // so emptying it left the cursor pointing past the end -- which the
        // next `results[current_result]` would have panicked on had any of the
        // other methods forgotten its own emptiness check.
        if self.current_result >= self.results.len() {
            self.current_result = 0;
        }
        Some((addr, old, new_cell))
    }

    /// Replace all matches.
    pub fn replace_all(&mut self, sheet: &mut Sheet) -> Vec<(CellAddr, Cell, Cell)> {
        let mut changes = Vec::new();
        // Clone results to avoid borrow issues
        let addrs: Vec<CellAddr> = self.results.clone();
        for addr in addrs {
            let cell = sheet.get_cell(addr);
            let old_text = if cell.is_formula() {
                cell.raw_input.clone()
            } else {
                cell.display_text()
            };
            let new_text = if self.case_sensitive {
                old_text.replace(&self.search_text, &self.replace_text)
            } else {
                case_insensitive_replace(&old_text, &self.search_text, &self.replace_text)
            };
            let old = sheet.set_cell_input(addr, &new_text);
            let new_cell = sheet.get_cell(addr);
            changes.push((addr, old, new_cell));
        }
        self.results.clear();
        self.current_result = 0;
        changes
    }

    /// Count of search results.
    pub fn result_count(&self) -> usize {
        self.results.len()
    }
}

/// Case-insensitive string replacement.
///
/// The offsets come from `textfind`, so they are offsets into `text` itself.
/// This used to search `text.to_lowercase()` and then slice `text` with the
/// copy's offsets, which is wrong three ways:
///
/// * folding is not length-preserving (`İ` U+0130 is two bytes and folds to
///   three), so `&text[start..abs_pos]` could slice inside a character and
///   panic;
/// * it advanced by the *unfolded* needle's byte length inside the folded
///   copy, which lands mid-character for a needle that grows when folded;
/// * an empty needle matched at every position without advancing `start`, so
///   the loop appended `replacement` forever.
fn case_insensitive_replace(text: &str, search: &str, replacement: &str) -> String {
    let mut result = String::new();
    let mut at = 0;
    for (start, end) in textfind::matches(text, search, textfind::Case::Insensitive) {
        result.push_str(text.get(at..start).unwrap_or(""));
        result.push_str(replacement);
        at = end;
    }
    result.push_str(text.get(at..).unwrap_or(""));
    result
}

// ============================================================================
// Interaction modes
// ============================================================================

/// A module for one type, because privacy in Rust is per-module and this crate
/// is a single one — the same reason [`cell_range`] is one.
mod edit_buffer {
    /// The text being typed into a cell, and where the caret sits in it.
    ///
    /// **The caret is counted in characters**, and having somewhere to write
    /// that down is the entire reason this type exists. It used to be a bare
    /// `usize` beside a `String` in an enum variant, and the code around it did
    /// not agree on what the number meant: insertion converted it through
    /// `char_indices().nth(n)`, which is characters, while `Backspace`,
    /// `Delete`, `Right` and `End` all used it as a byte offset into the same
    /// string. For ASCII those are the same number, which is why it worked at
    /// all, and why the disagreement stayed invisible until a cell held a
    /// character that is not one byte.
    ///
    /// The consequence was not cosmetic. `String::remove` takes a byte offset
    /// and *panics* if it is not on a character boundary. Typing `é` then `a`
    /// into a cell, pressing Home, Right, Delete took the whole application
    /// down — losing the workbook, not the cell. The two entry points into
    /// editing did not even agree with each other: one seeded the caret with
    /// `text.len()` (bytes) and the other with `1` (characters).
    ///
    /// Every method here keeps the caret in `0..=chars().count()`, and
    /// `byte_of` is the one place the conversion to bytes happens.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct EditBuffer {
        text: String,
        caret: usize,
    }

    impl EditBuffer {
        /// A buffer holding `text`, with the caret after its last character.
        pub fn at_end(text: String) -> Self {
            let caret = text.chars().count();
            Self { text, caret }
        }

        /// The text typed so far.
        pub fn text(&self) -> &str {
            &self.text
        }

        /// The caret position, in characters from the start.
        pub fn caret(&self) -> usize {
            self.caret
        }

        /// The byte offset of the `n`th character, or the end of the string if
        /// there is no such character.
        ///
        /// The single place a character index becomes a byte index, and so the
        /// single place that conversion can be got wrong.
        fn byte_of(&self, char_index: usize) -> usize {
            self.text
                .char_indices()
                .nth(char_index)
                .map_or(self.text.len(), |(byte, _)| byte)
        }

        fn char_count(&self) -> usize {
            self.text.chars().count()
        }

        /// Insert a character at the caret and step over it.
        pub fn insert(&mut self, ch: char) {
            let at = self.byte_of(self.caret);
            self.text.insert(at, ch);
            self.caret = self.caret.saturating_add(1);
        }

        /// Delete the character before the caret. Does nothing at the start.
        pub fn backspace(&mut self) {
            // `checked_sub` failing *is* the "caret is at the start" case, so
            // the guard and the arithmetic are one statement rather than two
            // that have to agree.
            let Some(previous) = self.caret.checked_sub(1) else {
                return;
            };
            let at = self.byte_of(previous);
            if at < self.text.len() {
                let _ = self.text.remove(at);
                self.caret = previous;
            }
        }

        /// Delete the character at the caret. Does nothing at the end.
        pub fn delete(&mut self) {
            let at = self.byte_of(self.caret);
            if at < self.text.len() {
                let _ = self.text.remove(at);
            }
        }

        /// Move the caret one character left, stopping at the start.
        pub fn move_left(&mut self) {
            self.caret = self.caret.saturating_sub(1);
        }

        /// Move the caret one character right, stopping at the end.
        pub fn move_right(&mut self) {
            self.caret = self.caret.saturating_add(1).min(self.char_count());
        }

        /// Move the caret before the first character.
        pub fn move_home(&mut self) {
            self.caret = 0;
        }

        /// Move the caret after the last character.
        pub fn move_end(&mut self) {
            self.caret = self.char_count();
        }
    }
}

pub use edit_buffer::EditBuffer;

/// Current interaction mode for the spreadsheet.
#[derive(Clone, Debug, PartialEq)]
pub enum InteractionMode {
    /// Normal cell navigation and selection.
    Normal,
    /// User is editing a cell (typing into formula bar or cell).
    Editing { buffer: EditBuffer },
    /// User is dragging to select a range.
    RangeSelect { anchor: CellAddr },
    /// User is resizing a column.
    ColResize {
        col: usize,
        start_x: f32,
        original_width: f32,
    },
    /// User is resizing a row.
    RowResize {
        row: usize,
        start_y: f32,
        original_height: f32,
    },
    /// User is dragging the auto-fill handle.
    AutoFill {
        anchor_range: CellRange,
        current_end: CellAddr,
    },
    /// Find/replace dialog is active.
    FindReplace,
}

// ============================================================================
// Sort direction
// ============================================================================

/// Sort direction for column sorting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

// ============================================================================
// Scroll position
// ============================================================================

/// Tracks the current scroll position of the grid.
///
/// `Copy`: it is two floats, and every reader wants the pair by value. Making
/// callers borrow a sheet to read an offset — and then hold that borrow across
/// the call that adjusts it — is the sort of thing that gets "fixed" with a
/// clone in one place and a stale copy in another.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrollPosition {
    pub x: f32,
    pub y: f32,
}

impl ScrollPosition {
    /// Create a new scroll position at origin.
    pub fn new() -> Self {
        Self { x: 0.0, y: 0.0 }
    }

    /// Clamp scroll to valid bounds given content size and viewport.
    pub fn clamp(&mut self, max_x: f32, max_y: f32) {
        if self.x < 0.0 {
            self.x = 0.0;
        }
        if self.y < 0.0 {
            self.y = 0.0;
        }
        if self.x > max_x {
            self.x = max_x;
        }
        if self.y > max_y {
            self.y = max_y;
        }
    }
}

// ============================================================================
// The workbook: a list of sheets that is never empty
// ============================================================================

/// The workbook's sheets, together with which one is active.
///
/// `active_sheet()` hands back a `&Sheet` rather than an `Option<&Sheet>`,
/// because all eighty of its callers want a sheet and there is nothing useful
/// for a renderer or an edit handler to do with `None`. That is only sound if
/// the list cannot be empty — and it used to be a `Vec` whose non-emptiness was
/// maintained by four separate methods and *asserted* by `&self.sheets[0]`,
/// under a comment reading "this should never happen, but handle gracefully".
/// Indexing a possibly-empty `Vec` is not handling anything gracefully; it is
/// the panic the fallback was written to avoid, one line further down.
///
/// Keeping the first sheet in a field of its own makes the guarantee
/// structural. There is no sequence of adds, removes, undos and redos that can
/// produce a workbook with no sheets, because nothing in this type's API can
/// take `head` away — and the "you may not remove the last sheet" rule is now
/// stated once, in [`SheetBook::remove`], rather than in `remove_active_sheet`
/// and *not* in the undo path, which was the actual state of affairs.
pub struct SheetBook {
    /// The sheet that always exists; sheet 0.
    head: Sheet,
    /// The remaining sheets, in order after `head`.
    tail: Vec<Sheet>,
    /// Index of the active sheet. Every mutator keeps this inside `0..len()`.
    active: usize,
}

impl SheetBook {
    /// A workbook holding a single sheet, which is the active one.
    pub fn new(first: Sheet) -> Self {
        Self {
            head: first,
            tail: Vec::new(),
            active: 0,
        }
    }

    /// Number of sheets. Never zero.
    pub fn len(&self) -> usize {
        self.tail.len().saturating_add(1)
    }

    /// Always `false` — see this type's documentation.
    ///
    /// Present because a `len` without an `is_empty` is a lint, and answering
    /// the question honestly says more than suppressing it would.
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The sheet at `index`, or `None` if there is none.
    ///
    /// `checked_sub(1)` failing *is* the "this is sheet zero" case, so the two
    /// branches are the two halves of the representation rather than a bound
    /// test followed by an index.
    pub fn get(&self, index: usize) -> Option<&Sheet> {
        match index.checked_sub(1) {
            None => Some(&self.head),
            Some(rest) => self.tail.get(rest),
        }
    }

    /// The sheet at `index`, mutably, or `None` if there is none.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Sheet> {
        match index.checked_sub(1) {
            None => Some(&mut self.head),
            Some(rest) => self.tail.get_mut(rest),
        }
    }

    /// Index of the active sheet.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Make `index` active. An index naming no sheet is ignored.
    pub fn set_active(&mut self, index: usize) {
        if index < self.len() {
            self.active = index;
        }
    }

    /// The active sheet.
    pub fn active(&self) -> &Sheet {
        self.get(self.active).unwrap_or(&self.head)
    }

    /// The active sheet, mutably.
    pub fn active_mut(&mut self) -> &mut Sheet {
        match self.active.checked_sub(1) {
            None => &mut self.head,
            // The fallback cannot be reached while `active` is in range, which
            // every mutator here keeps it. It is written out anyway so that a
            // future mutator which forgets shows the wrong sheet rather than
            // stopping the program.
            Some(rest) => self.tail.get_mut(rest).unwrap_or(&mut self.head),
        }
    }

    /// Append `sheet` and make it active, returning its index.
    pub fn push(&mut self, sheet: Sheet) -> usize {
        self.tail.push(sheet);
        self.active = self.tail.len();
        self.active
    }

    /// Remove the sheet at `index`.
    ///
    /// Returns `None` — changing nothing — if `index` names no sheet, or if
    /// removing it would leave the workbook with none. This is the only place
    /// that rule is enforced, and the only place it needs to be.
    pub fn remove(&mut self, index: usize) -> Option<Sheet> {
        if index >= self.len() || self.tail.is_empty() {
            return None;
        }
        let removed = match index.checked_sub(1) {
            // Removing sheet 0 promotes the next sheet into its place, which
            // is why this is only allowed when there *is* a next sheet.
            None => core::mem::replace(&mut self.head, self.tail.remove(0)),
            Some(rest) => self.tail.remove(rest),
        };
        self.active = self.active.min(self.len().saturating_sub(1));
        Some(removed)
    }

    /// Insert `sheet` at `index` (clamped to the end) and make it active.
    pub fn insert(&mut self, index: usize, sheet: Sheet) {
        let index = index.min(self.len());
        match index.checked_sub(1) {
            None => {
                let displaced = core::mem::replace(&mut self.head, sheet);
                self.tail.insert(0, displaced);
            }
            Some(rest) => self.tail.insert(rest.min(self.tail.len()), sheet),
        }
        self.active = index;
    }

    /// Every sheet, in order.
    pub fn iter(&self) -> impl Iterator<Item = &Sheet> {
        core::iter::once(&self.head).chain(self.tail.iter())
    }
}

// ============================================================================
// Spreadsheet application state
// ============================================================================

/// The main spreadsheet application state.
pub struct SpreadsheetApp {
    /// All worksheets, and which one is active.
    pub sheets: SheetBook,
    /// Current interaction mode.
    pub mode: InteractionMode,
    /// Clipboard contents.
    pub clipboard: Option<ClipboardData>,
    /// Undo/redo manager.
    pub undo_manager: UndoManager,
    /// Window width.
    pub window_width: f32,
    /// Window height.
    pub window_height: f32,
    /// Find and replace state.
    pub find_replace: FindReplace,
    /// Whether to show gridlines.
    pub show_gridlines: bool,
    /// Whether to show the formula bar.
    pub show_formula_bar: bool,
    /// Whether to show the toolbar.
    pub show_toolbar: bool,
    /// Whether to show the status bar.
    pub show_status_bar: bool,
}

impl SpreadsheetApp {
    /// Create a new spreadsheet application with a single sheet.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            sheets: SheetBook::new(Sheet::new("Sheet1")),
            mode: InteractionMode::Normal,
            clipboard: None,
            undo_manager: UndoManager::new(),
            window_width: width,
            window_height: height,
            find_replace: FindReplace::new(),
            show_gridlines: true,
            show_formula_bar: true,
            show_toolbar: true,
            show_status_bar: true,
        }
    }

    /// Get a reference to the currently active sheet.
    pub fn active_sheet(&self) -> &Sheet {
        self.sheets.active()
    }

    /// Get a mutable reference to the currently active sheet.
    pub fn active_sheet_mut(&mut self) -> &mut Sheet {
        self.sheets.active_mut()
    }

    /// How far the visible sheet is scrolled.
    ///
    /// An accessor rather than a field on the app, because the offset belongs
    /// to the sheet — see [`Sheet::scroll`]. Reading it through the active
    /// sheet is what makes it impossible for a tab switch to show one sheet at
    /// another's offset, however that switch came about.
    /// What is selected on the visible sheet.
    ///
    /// An accessor rather than a field on the app — see [`Sheet::selection`].
    pub fn selection(&self) -> &Selection {
        &self.active_sheet().selection
    }

    /// The visible sheet's selection, mutably.
    fn selection_mut(&mut self) -> &mut Selection {
        &mut self.active_sheet_mut().selection
    }

    /// How far the visible sheet is scrolled.
    ///
    /// An accessor rather than a field on the app, because the offset belongs
    /// to the sheet — see [`Sheet::scroll`]. Reading it through the active
    /// sheet is what makes it impossible for a tab switch to show one sheet at
    /// another's offset, however that switch came about.
    pub fn scroll(&self) -> ScrollPosition {
        self.active_sheet().scroll
    }

    /// The visible sheet's scroll offset, mutably.
    ///
    /// Prefer [`Self::scroll_by`] and [`Self::ensure_cell_visible`]: both end
    /// in [`Self::clamp_scroll`], whereas an offset written through here can be
    /// left outside its bounds.
    fn scroll_mut(&mut self) -> &mut ScrollPosition {
        &mut self.active_sheet_mut().scroll
    }

    /// Set the active cell input, recording undo, and recalculate.
    pub fn set_cell_input(&mut self, addr: CellAddr, input: &str) {
        let sheet_idx = self.sheets.active_index();
        let old_cell = self.active_sheet_mut().set_cell_input(addr, input);
        let new_cell = self.active_sheet().get_cell(addr);
        self.undo_manager.push_action(UndoAction::CellEdit {
            sheet_idx,
            addr,
            old_cell,
            new_cell,
        });
        recalculate_sheet(self.active_sheet_mut());
    }

    /// Begin editing the active cell.
    pub fn begin_editing(&mut self) {
        let cell = self.active_sheet().get_cell(self.selection().active);
        let text = if cell.is_formula() {
            cell.raw_input.clone()
        } else {
            cell.display_text()
        };
        self.mode = InteractionMode::Editing {
            buffer: EditBuffer::at_end(text),
        };
    }

    /// Confirm cell edit and return to normal mode.
    pub fn confirm_edit(&mut self) {
        if let InteractionMode::Editing { buffer } = &self.mode {
            let text = buffer.text().to_owned();
            let addr = self.selection().active;
            self.set_cell_input(addr, &text);
            self.mode = InteractionMode::Normal;
        }
    }

    /// Cancel cell edit, discarding changes.
    pub fn cancel_edit(&mut self) {
        self.mode = InteractionMode::Normal;
    }

    /// Delete the contents of all selected cells.
    pub fn delete_selection(&mut self) {
        let sheet_idx = self.sheets.active_index();
        let mut changes = Vec::new();
        let ranges = self.selection().ranges.clone();
        for range in &ranges {
            for addr in range.iter() {
                let old = self.active_sheet().get_cell(addr);
                if !old.value.is_empty() || !old.raw_input.is_empty() {
                    let new_cell = Cell::empty();
                    changes.push((addr, old, new_cell));
                }
            }
        }
        if !changes.is_empty() {
            for (addr, _, new_cell) in &changes {
                self.active_sheet_mut().set_cell(*addr, new_cell.clone());
            }
            self.undo_manager
                .push_action(UndoAction::BatchEdit { sheet_idx, changes });
            recalculate_sheet(self.active_sheet_mut());
        }
    }

    /// Copy selected cells to clipboard.
    pub fn copy_selection(&mut self) {
        let range = self.selection().primary_range();
        let origin = range.start();
        let mut cells = HashMap::new();
        for addr in range.iter() {
            let cell = self.active_sheet().get_cell(addr);
            // Saturating rather than `-`: the iterator only yields addresses at
            // or after `start`, so the difference cannot be negative -- but that
            // is a fact about `CellRangeIter`, stated here in the one form the
            // compiler will keep true if the iterator is ever changed.
            let rel_col = addr.col.saturating_sub(origin.col);
            let rel_row = addr.row.saturating_sub(origin.row);
            cells.insert((rel_col, rel_row), cell);
        }
        self.clipboard = Some(ClipboardData {
            source_range: range,
            cells,
            is_cut: false,
        });
    }

    /// Cut selected cells to clipboard.
    pub fn cut_selection(&mut self) {
        self.copy_selection();
        if let Some(ref mut clip) = self.clipboard {
            clip.is_cut = true;
        }
        self.delete_selection();
    }

    /// Paste clipboard contents at the active cell.
    pub fn paste(&mut self) {
        let clip = match self.clipboard.clone() {
            Some(c) => c,
            None => return,
        };
        let dest = self.selection().active;
        let sheet_idx = self.sheets.active_index();
        let mut changes = Vec::new();

        for (&(rel_col, rel_row), src_cell) in &clip.cells {
            // A destination off the sheet is skipped — and an offset that would
            // wrap is off the sheet too, so `checked_add` and the bound live in
            // one expression. Written as `dest.col + rel_col` followed by a
            // separate `>= MAX_COLS`, a wrap would land back near column A and
            // pass the very test meant to reject it.
            let Some(target_col) = dest.col.checked_add(rel_col).filter(|c| *c < MAX_COLS) else {
                continue;
            };
            let Some(target_row) = dest.row.checked_add(rel_row).filter(|r| *r < MAX_ROWS) else {
                continue;
            };
            let target_addr = CellAddr::new(target_col, target_row);
            let old = self.active_sheet().get_cell(target_addr);
            let new_cell = src_cell.clone();
            changes.push((target_addr, old, new_cell.clone()));
            self.active_sheet_mut().set_cell(target_addr, new_cell);
        }

        if !changes.is_empty() {
            self.undo_manager
                .push_action(UndoAction::BatchEdit { sheet_idx, changes });
            recalculate_sheet(self.active_sheet_mut());
        }
    }

    /// Undo the last action.
    pub fn undo(&mut self) {
        if let Some(action) = self.undo_manager.pop_undo() {
            self.apply_undo_action(&action, true);
        }
    }

    /// Redo the last undone action.
    pub fn redo(&mut self) {
        if let Some(action) = self.undo_manager.pop_redo() {
            self.apply_undo_action(&action, false);
        }
    }

    /// Apply an undo or redo action.
    fn apply_undo_action(&mut self, action: &UndoAction, is_undo: bool) {
        match action {
            UndoAction::CellEdit {
                sheet_idx,
                addr,
                old_cell,
                new_cell,
            } => {
                if let Some(sheet) = self.sheets.get_mut(*sheet_idx) {
                    let cell = if is_undo { old_cell } else { new_cell };
                    sheet.set_cell(*addr, cell.clone());
                    recalculate_sheet(sheet);
                }
            }
            UndoAction::BatchEdit { sheet_idx, changes } => {
                if let Some(sheet) = self.sheets.get_mut(*sheet_idx) {
                    for (addr, old, new_cell) in changes {
                        let cell = if is_undo { old } else { new_cell };
                        sheet.set_cell(*addr, cell.clone());
                    }
                    recalculate_sheet(sheet);
                }
            }
            UndoAction::ColResize {
                sheet_idx,
                col,
                old_width,
                new_width,
            } => {
                if let Some(sheet) = self.sheets.get_mut(*sheet_idx) {
                    let width = if is_undo { *old_width } else { *new_width };
                    if let Some(w) = sheet.col_widths.get_mut(*col) {
                        *w = width;
                    }
                }
            }
            UndoAction::RowResize {
                sheet_idx,
                row,
                old_height,
                new_height,
            } => {
                if let Some(sheet) = self.sheets.get_mut(*sheet_idx) {
                    let height = if is_undo { *old_height } else { *new_height };
                    if let Some(h) = sheet.row_heights.get_mut(*row) {
                        *h = height;
                    }
                }
            }
            // Both sheet actions are their own inverse in the other direction,
            // and both used to implement only the undo half -- `is_undo &&` on
            // one, `if is_undo` on the other -- so redoing either did nothing
            // at all. Adding a sheet, undoing, then redoing left the sheet
            // gone; removing one, undoing, then redoing left it present. Each
            // is now written as one `if`, so a direction cannot be dropped
            // without the other becoming visibly wrong.
            UndoAction::AddSheet { sheet_idx, sheet } => {
                if is_undo {
                    self.sheets.remove(*sheet_idx);
                } else {
                    self.sheets.insert(*sheet_idx, sheet.clone());
                }
            }
            UndoAction::RemoveSheet { sheet_idx, sheet } => {
                if is_undo {
                    self.sheets.insert(*sheet_idx, sheet.clone());
                } else {
                    self.sheets.remove(*sheet_idx);
                }
            }
        }
    }

    /// Add a new sheet.
    pub fn add_sheet(&mut self) {
        let idx = self.sheets.len();
        let sheet = Sheet::new(&format!("Sheet{}", idx.saturating_add(1)));
        self.sheets.push(sheet.clone());
        self.undo_manager.push_action(UndoAction::AddSheet {
            sheet_idx: idx,
            sheet,
        });
    }

    /// Remove the active sheet (if more than one sheet exists).
    ///
    /// The "more than one" test lives in [`SheetBook::remove`], which declines
    /// rather than emptying the workbook — so the `let ... else` here is the
    /// same rule, read from the one place that states it, instead of a second
    /// copy of it that the undo path did not have.
    pub fn remove_active_sheet(&mut self) {
        let idx = self.sheets.active_index();
        let Some(sheet) = self.sheets.remove(idx) else {
            return;
        };
        self.undo_manager.push_action(UndoAction::RemoveSheet {
            sheet_idx: idx,
            sheet,
        });
    }

    /// Sort the active sheet by the selected column.
    pub fn sort_column(&mut self, direction: SortDirection) {
        let col = self.selection().active.col;
        let range = self.selection().primary_range();
        let start_row = range.start().row;
        let end_row = range.end().row;
        let ascending = direction == SortDirection::Ascending;
        let sheet_idx = self.sheets.active_index();

        let changes = self
            .active_sheet_mut()
            .sort_by_column(col, start_row, end_row, ascending);
        if !changes.is_empty() {
            self.undo_manager
                .push_action(UndoAction::BatchEdit { sheet_idx, changes });
            recalculate_sheet(self.active_sheet_mut());
        }
    }

    /// Auto-fill from a source range to a target range.
    pub fn auto_fill(&mut self, source: CellRange, target_end: CellAddr) {
        let target = CellRange::new(source.start(), target_end);
        let sheet_idx = self.sheets.active_index();
        let mut changes = Vec::new();

        // Collect source values per column
        for col in source.start().col..=source.end().col {
            let source_vals: Vec<CellValue> = (source.start().row..=source.end().row)
                .map(|r| {
                    self.active_sheet()
                        .get_cell(CellAddr::new(col, r))
                        .value
                        .clone()
                })
                .collect();

            // The row below the source block. Saturating so that a source range
            // ending at `usize::MAX` yields an empty fill rather than wrapping to
            // row zero and overwriting the top of the sheet.
            let fill_start = source.end().row.saturating_add(1);
            let fill_end = target.end().row;
            // `enumerate` rather than `row - fill_start`: the pattern index and
            // the row it fills come off the same iterator, so they cannot drift
            // apart, and there is no subtraction to justify.
            for (idx, row) in (fill_start..=fill_end).enumerate() {
                let new_val = auto_fill_next(&source_vals, idx);
                let addr = CellAddr::new(col, row);
                let input = value_to_string(&new_val);
                let old_cell = self.active_sheet_mut().set_cell_input(addr, &input);
                let new_cell = self.active_sheet().get_cell(addr);
                changes.push((addr, old_cell, new_cell));
            }
        }

        if !changes.is_empty() {
            self.undo_manager
                .push_action(UndoAction::BatchEdit { sheet_idx, changes });
            recalculate_sheet(self.active_sheet_mut());
        }
    }

    /// Toggle bold formatting for the selected cells.
    pub fn toggle_bold(&mut self) {
        let sheet_idx = self.sheets.active_index();
        let mut changes = Vec::new();
        let current_bold = self
            .active_sheet()
            .get_cell(self.selection().active)
            .format
            .bold;
        let new_bold = !current_bold;

        let ranges = self.selection().ranges.clone();
        for range in &ranges {
            for addr in range.iter() {
                let old = self.active_sheet().get_cell(addr);
                let mut new_cell = old.clone();
                new_cell.format.bold = new_bold;
                changes.push((addr, old, new_cell.clone()));
                self.active_sheet_mut().set_cell(addr, new_cell);
            }
        }

        if !changes.is_empty() {
            self.undo_manager
                .push_action(UndoAction::BatchEdit { sheet_idx, changes });
        }
    }

    /// Toggle italic formatting for the selected cells.
    pub fn toggle_italic(&mut self) {
        let sheet_idx = self.sheets.active_index();
        let mut changes = Vec::new();
        let current_italic = self
            .active_sheet()
            .get_cell(self.selection().active)
            .format
            .italic;
        let new_italic = !current_italic;

        let ranges = self.selection().ranges.clone();
        for range in &ranges {
            for addr in range.iter() {
                let old = self.active_sheet().get_cell(addr);
                let mut new_cell = old.clone();
                new_cell.format.italic = new_italic;
                changes.push((addr, old, new_cell.clone()));
                self.active_sheet_mut().set_cell(addr, new_cell);
            }
        }

        if !changes.is_empty() {
            self.undo_manager
                .push_action(UndoAction::BatchEdit { sheet_idx, changes });
        }
    }

    /// Set alignment for the selected cells.
    pub fn set_alignment(&mut self, alignment: Alignment) {
        let sheet_idx = self.sheets.active_index();
        let mut changes = Vec::new();

        let ranges = self.selection().ranges.clone();
        for range in &ranges {
            for addr in range.iter() {
                let old = self.active_sheet().get_cell(addr);
                let mut new_cell = old.clone();
                new_cell.format.alignment = alignment;
                changes.push((addr, old, new_cell.clone()));
                self.active_sheet_mut().set_cell(addr, new_cell);
            }
        }

        if !changes.is_empty() {
            self.undo_manager
                .push_action(UndoAction::BatchEdit { sheet_idx, changes });
        }
    }

    /// Set number format for the selected cells.
    pub fn set_number_format(&mut self, format: NumberFormat) {
        let sheet_idx = self.sheets.active_index();
        let mut changes = Vec::new();

        let ranges = self.selection().ranges.clone();
        for range in &ranges {
            for addr in range.iter() {
                let old = self.active_sheet().get_cell(addr);
                let mut new_cell = old.clone();
                new_cell.format.number_format = format.clone();
                changes.push((addr, old, new_cell.clone()));
                self.active_sheet_mut().set_cell(addr, new_cell);
            }
        }

        if !changes.is_empty() {
            self.undo_manager
                .push_action(UndoAction::BatchEdit { sheet_idx, changes });
        }
    }

    /// Toggle borders on selected cells.
    pub fn toggle_borders(&mut self) {
        let sheet_idx = self.sheets.active_index();
        let mut changes = Vec::new();
        let current_borders = self
            .active_sheet()
            .get_cell(self.selection().active)
            .format
            .borders
            .has_any();
        let new_borders = if current_borders {
            CellBorders::none()
        } else {
            CellBorders::all()
        };

        let ranges = self.selection().ranges.clone();
        for range in &ranges {
            for addr in range.iter() {
                let old = self.active_sheet().get_cell(addr);
                let mut new_cell = old.clone();
                new_cell.format.borders = new_borders.clone();
                changes.push((addr, old, new_cell.clone()));
                self.active_sheet_mut().set_cell(addr, new_cell);
            }
        }

        if !changes.is_empty() {
            self.undo_manager
                .push_action(UndoAction::BatchEdit { sheet_idx, changes });
        }
    }

    /// Freeze rows/columns at the current selection.
    pub fn toggle_freeze_panes(&mut self) {
        let col = self.selection().active.col;
        let row = self.selection().active.row;
        let sheet = self.active_sheet_mut();
        if sheet.frozen_cols > 0 || sheet.frozen_rows > 0 {
            sheet.frozen_cols = 0;
            sheet.frozen_rows = 0;
        } else {
            sheet.frozen_cols = col;
            sheet.frozen_rows = row;
        }
        // Freezing does not change either limit, but it does change which
        // cells the current offset is showing, and `ensure_cell_visible` now
        // treats the frozen band differently. Re-running the bound keeps the
        // invariant "`scroll` is always inside its range" true after every
        // mutation rather than only after most of them.
        self.clamp_scroll();
        self.ensure_cell_visible(self.selection().active);
    }

    /// Navigate the active cell in a given direction.
    pub fn navigate(&mut self, d_col: i32, d_row: i32) {
        let new_col = step_index(self.selection().active.col, d_col, MAX_COLS);
        let new_row = step_index(self.selection().active.row, d_row, MAX_ROWS);
        let new_addr = CellAddr::new(new_col, new_row);
        *self.selection_mut() = Selection::single(new_addr);
        self.ensure_cell_visible(new_addr);
    }

    /// Scroll the least amount that brings `addr` fully on screen.
    ///
    /// A cell inside a frozen band needs no scrolling at all: it is pinned to
    /// the edge and therefore always visible, and moving the sheet to "reveal"
    /// it only displaces everything else. The version this replaces did exactly
    /// that — it compared a frozen cell's unscrolled offset against a scrolled
    /// one, always found it short, and so snapped the sheet back to column A
    /// (or row 1) every time the selection entered the frozen band. With panes
    /// frozen, the arrow keys could not be used to walk along the pinned
    /// header row without throwing away the scroll position.
    pub fn ensure_cell_visible(&mut self, addr: CellAddr) {
        let sheet = self.active_sheet();
        let (frozen_cols, frozen_rows) = (sheet.frozen_cols, sheet.frozen_rows);
        let frozen_w = sheet.col_x_offset(frozen_cols);
        let frozen_h = sheet.row_y_offset(frozen_rows);
        let (cell_x, cell_w) = (sheet.col_x_offset(addr.col), sheet.col_width(addr.col));
        let (cell_y, cell_h) = (sheet.row_y_offset(addr.row), sheet.row_height(addr.row));
        let (grid_w, grid_h) = (self.grid_width(), self.grid_height());

        // The window a *scrolling* column is seen through starts after the
        // frozen band, so it is off the left when it has slid under that band —
        // not when it has left the grid. Both edges are measured in the grid's
        // own coordinates, with `ROW_HEADER_WIDTH` cancelling out of each side.
        if addr.col >= frozen_cols {
            if cell_x - self.scroll().x < frozen_w {
                self.scroll_mut().x = cell_x - frozen_w;
            } else if cell_x + cell_w - self.scroll().x > grid_w {
                self.scroll_mut().x = cell_x + cell_w - grid_w;
            }
        }

        if addr.row >= frozen_rows {
            if cell_y - self.scroll().y < frozen_h {
                self.scroll_mut().y = cell_y - frozen_h;
            } else if cell_y + cell_h - self.scroll().y > grid_h {
                self.scroll_mut().y = cell_y + cell_h - grid_h;
            }
        }

        self.clamp_scroll();
    }

    /// Calculate where the grid starts (Y coordinate), accounting for toolbar and formula bar.
    pub fn grid_top(&self) -> f32 {
        let mut y = 0.0;
        if self.show_toolbar {
            y += TOOLBAR_HEIGHT;
        }
        if self.show_formula_bar {
            y += FORMULA_BAR_HEIGHT;
        }
        y += COL_HEADER_HEIGHT;
        y
    }

    /// Calculate the grid viewport height.
    pub fn grid_height(&self) -> f32 {
        let mut bottom = self.window_height;
        if self.show_status_bar {
            bottom -= STATUS_BAR_HEIGHT;
        }
        bottom -= SHEET_TAB_HEIGHT;
        let top = self.grid_top();
        (bottom - top).max(0.0)
    }

    /// Calculate the grid viewport width.
    pub fn grid_width(&self) -> f32 {
        (self.window_width - ROW_HEADER_WIDTH - SCROLLBAR_WIDTH).max(0.0)
    }

    /// The bottom edge of the grid viewport, in window coordinates.
    pub fn grid_bottom(&self) -> f32 {
        self.grid_top() + self.grid_height()
    }

    /// The right edge of the grid viewport, in window coordinates.
    pub fn grid_right(&self) -> f32 {
        ROW_HEADER_WIDTH + self.grid_width()
    }

    // ── Where the grid's contents land on screen ─────────────────────────
    //
    // One law, five callers. The renderer, the column headers, the hit test,
    // `ensure_cell_visible` and the two resize loops all have to agree about
    // where column `c` is drawn — and until this existed each derived it
    // separately, with the two derivations disagreeing.
    //
    // The renderer and the selection outlines subtract the scroll offset only
    // from the *unfrozen* side, which is what a frozen pane means: the pinned
    // columns stay put while the rest slide underneath. `cell_at_position` and
    // both resize loops subtracted it from every column and every row. So with
    // panes frozen and the sheet scrolled, clicking a frozen cell selected a
    // different one, the error being exactly the scroll offset — and it grew
    // as you scrolled, which is the signature of two derivations of one
    // number rather than a constant being wrong.

    /// The right edge of the frozen column band, in window coordinates.
    ///
    /// Equals [`ROW_HEADER_WIDTH`] when nothing is frozen, so the band is
    /// empty and every column is a scrolling one.
    pub fn frozen_right(&self) -> f32 {
        let sheet = self.active_sheet();
        ROW_HEADER_WIDTH + sheet.col_x_offset(sheet.frozen_cols)
    }

    /// The bottom edge of the frozen row band, in window coordinates.
    pub fn frozen_bottom(&self) -> f32 {
        let sheet = self.active_sheet();
        self.grid_top() + sheet.row_y_offset(sheet.frozen_rows)
    }

    /// Where column `col`'s left edge is drawn, in window coordinates.
    pub fn col_screen_x(&self, col: usize) -> f32 {
        let sheet = self.active_sheet();
        let unscrolled = ROW_HEADER_WIDTH + sheet.col_x_offset(col);
        if col < sheet.frozen_cols {
            unscrolled
        } else {
            unscrolled - self.scroll().x
        }
    }

    /// Where row `row`'s top edge is drawn, in window coordinates.
    pub fn row_screen_y(&self, row: usize) -> f32 {
        let sheet = self.active_sheet();
        let unscrolled = self.grid_top() + sheet.row_y_offset(row);
        if row < sheet.frozen_rows {
            unscrolled
        } else {
            unscrolled - self.scroll().y
        }
    }

    // ── How far the grid may be scrolled ─────────────────────────────────

    /// The largest horizontal offset that still shows content.
    ///
    /// At this offset the last column's right edge sits on the grid's right
    /// edge. Note that the frozen band does *not* enter into it: freezing
    /// columns narrows the window the scrolling ones are seen through, but the
    /// last column still arrives at the same place, because it is positioned
    /// against the grid's own right edge either way.
    pub fn max_scroll_x(&self) -> f32 {
        (self.active_sheet().col_x_offset(MAX_COLS) - self.grid_width()).max(0.0)
    }

    /// The largest vertical offset that still shows content.
    pub fn max_scroll_y(&self) -> f32 {
        (self.active_sheet().row_y_offset(MAX_ROWS) - self.grid_height()).max(0.0)
    }

    /// Move the grid by a pixel delta, bounded at both ends of both axes.
    ///
    /// A non-finite delta moves nothing rather than poisoning the offset: the
    /// deltas come from an input event, and a `NaN` that reached `scroll` would
    /// leave the sheet unscrollable for the rest of the session with no way
    /// back short of restarting.
    pub fn scroll_by(&mut self, dx: f32, dy: f32) {
        if dx.is_finite() {
            self.scroll_mut().x += dx;
        }
        if dy.is_finite() {
            self.scroll_mut().y += dy;
        }
        self.clamp_scroll();
    }

    /// How many rows a page key moves: the rows the pane is actually showing,
    /// less one.
    ///
    /// This was the constant `20`, which is correct for exactly one window
    /// height. In a taller window PageDown advanced less than a screen, so
    /// reading a sheet took two presses per page; in a shorter one it jumped
    /// over rows that were never displayed. It is measured from the selected
    /// row rather than from the top of the sheet because rows here are
    /// individually resizable — a page of tall rows is fewer of them.
    ///
    /// The frozen band is subtracted: those rows do not scroll, so they are not
    /// part of the page that turns. The one row of overlap is the usual pager
    /// convention — the row you were on stays visible, so you can find your
    /// place — and it also guarantees the key is never a no-op.
    pub fn page_rows(&self) -> i32 {
        let sheet = self.active_sheet();
        let pane = (self.grid_height() - sheet.row_y_offset(sheet.frozen_rows)).max(0.0);
        let first = self.selection().active.row;
        let last = sheet.row_at_y(sheet.row_y_offset(first) + pane);
        let span = last.saturating_sub(first).saturating_sub(1).max(1);
        i32::try_from(span).unwrap_or(i32::MAX)
    }

    /// Pull the scroll offset back inside its bounds.
    ///
    /// Must run after anything that can move either the offset or the bounds,
    /// which includes things that never touch `scroll` at all: *growing* the
    /// window lowers `max_scroll_y`, and widening a column raises
    /// `max_scroll_x`. An offset left past a shrunken limit shows blank space
    /// below the last row, which is the state the far end was never bounded
    /// against in the first place.
    pub fn clamp_scroll(&mut self) {
        let (max_x, max_y) = (self.max_scroll_x(), self.max_scroll_y());
        self.scroll_mut().clamp(max_x, max_y);
    }

    /// Get status bar text showing SUM/AVG/COUNT of selection.
    pub fn status_bar_text(&self) -> String {
        let nums = self.selection().numeric_values(self.active_sheet());
        if nums.is_empty() {
            return String::new();
        }
        let sum: f64 = nums.iter().sum();
        let count = nums.len();
        let avg = sum / count as f64;
        format!("SUM: {:.2}  AVG: {:.2}  COUNT: {}", sum, avg, count)
    }

    /// Handle keyboard events.
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Ignored;
        }

        // Handle find/replace mode
        if self.mode == InteractionMode::FindReplace {
            return self.handle_find_replace_key(event);
        }

        // Handle editing mode
        if let InteractionMode::Editing { ref mut buffer } = self.mode {
            return handle_editing_key(buffer, event);
        }

        // Ctrl shortcuts
        if event.modifiers.ctrl {
            match event.key {
                Key::C => {
                    self.copy_selection();
                    return EventResult::Consumed;
                }
                Key::X => {
                    self.cut_selection();
                    return EventResult::Consumed;
                }
                Key::V => {
                    self.paste();
                    return EventResult::Consumed;
                }
                Key::Z => {
                    self.undo();
                    return EventResult::Consumed;
                }
                Key::Y => {
                    self.redo();
                    return EventResult::Consumed;
                }
                Key::B => {
                    self.toggle_bold();
                    return EventResult::Consumed;
                }
                Key::I => {
                    self.toggle_italic();
                    return EventResult::Consumed;
                }
                Key::F => {
                    self.mode = InteractionMode::FindReplace;
                    self.find_replace.active = true;
                    return EventResult::Consumed;
                }
                Key::H => {
                    self.mode = InteractionMode::FindReplace;
                    self.find_replace.active = true;
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }

        // Normal mode navigation
        match event.key {
            Key::Left => {
                self.navigate(-1, 0);
                EventResult::Consumed
            }
            Key::Right => {
                self.navigate(1, 0);
                EventResult::Consumed
            }
            Key::Up => {
                self.navigate(0, -1);
                EventResult::Consumed
            }
            Key::Down => {
                self.navigate(0, 1);
                EventResult::Consumed
            }
            Key::Home => {
                *self.selection_mut() = Selection::single(CellAddr::new(0, self.selection().active.row));
                self.ensure_cell_visible(self.selection().active);
                EventResult::Consumed
            }
            Key::End => {
                *self.selection_mut() =
                    Selection::single(CellAddr::new(MAX_COLS - 1, self.selection().active.row));
                self.ensure_cell_visible(self.selection().active);
                EventResult::Consumed
            }
            Key::PageUp => {
                // `saturating_neg`, not `-`: `page_rows` is clamped to `i32::MAX`
                // rather than to `i32::MAX - 1`, and unary minus on `i32::MIN`
                // is the one negation that overflows.
                self.navigate(0, self.page_rows().saturating_neg());
                EventResult::Consumed
            }
            Key::PageDown => {
                self.navigate(0, self.page_rows());
                EventResult::Consumed
            }
            Key::Tab => {
                if event.modifiers.shift {
                    self.navigate(-1, 0);
                } else {
                    self.navigate(1, 0);
                }
                EventResult::Consumed
            }
            Key::Enter => {
                if event.modifiers.shift {
                    self.navigate(0, -1);
                } else {
                    // If we were in editing, confirm would already have been handled above
                    self.navigate(0, 1);
                }
                EventResult::Consumed
            }
            Key::F2 => {
                self.begin_editing();
                EventResult::Consumed
            }
            Key::Delete => {
                self.delete_selection();
                EventResult::Consumed
            }
            Key::Escape => {
                self.cancel_edit();
                EventResult::Consumed
            }
            _ => {
                // Start editing if a printable character is typed
                if let Some(ch) = event.text
                    && !ch.is_control()
                {
                    self.mode = InteractionMode::Editing {
                        buffer: EditBuffer::at_end(String::from(ch)),
                    };
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
        }
    }

    /// Handle keyboard events in find/replace mode.
    fn handle_find_replace_key(&mut self, event: &KeyEvent) -> EventResult {
        match event.key {
            Key::Escape => {
                self.mode = InteractionMode::Normal;
                self.find_replace.active = false;
                EventResult::Consumed
            }
            Key::Enter => {
                // The clamp that used to be written here -- `idx.min(len - 1)`
                // -- was guarding against an active index past the end, which
                // `SheetBook` no longer permits.
                self.find_replace.find_all(self.sheets.active());
                if let Some(addr) = self.find_replace.next_result() {
                    *self.selection_mut() = Selection::single(addr);
                    self.ensure_cell_visible(addr);
                }
                EventResult::Consumed
            }
            Key::Backspace => {
                if !self.find_replace.search_text.is_empty() {
                    self.find_replace.search_text.pop();
                }
                EventResult::Consumed
            }
            _ => {
                if let Some(ch) = event.text
                    && !ch.is_control()
                {
                    self.find_replace.search_text.push(ch);
                }
                EventResult::Consumed
            }
        }
    }

    /// Handle mouse events.
    pub fn handle_mouse_event(&mut self, event: &MouseEvent) -> EventResult {
        match &event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                self.handle_left_click(event.x, event.y, false)
            }
            MouseEventKind::Release(MouseButton::Left) => {
                self.handle_left_release(event.x, event.y)
            }
            MouseEventKind::Move => self.handle_mouse_move(event.x, event.y),
            // Both axes, and neither converted with the other's helper: `dy`
            // is positive away from the user (towards row 0) while `dx` is
            // positive to the right (towards the last column), so exactly one
            // of the two has its sign undone. See `wheel::pixels_x`.
            //
            // No accumulator here, unlike the row-indexed views: this offset is
            // already continuous, so a tenth of a notch is 7 px and can be
            // applied on the spot rather than banked until it makes a whole
            // row. An accumulator belongs to an integer offset and only to one.
            MouseEventKind::Scroll { dx, dy } => {
                self.scroll_by(
                    wheel::pixels_x(*dx, DEFAULT_COL_WIDTH),
                    wheel::pixels(*dy, DEFAULT_ROW_HEIGHT),
                );
                EventResult::Consumed
            }
            MouseEventKind::DoubleClick(MouseButton::Left) => {
                // Double-click starts editing
                let Some((col, row)) = self.cell_at_position(event.x, event.y) else {
                    return EventResult::Ignored;
                };
                *self.selection_mut() = Selection::single(CellAddr::new(col, row));
                self.begin_editing();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle left mouse click at a position.
    fn handle_left_click(&mut self, x: f32, y: f32, _ctrl_held: bool) -> EventResult {
        // Check for sheet tab clicks
        let tab_y = self.window_height - SHEET_TAB_HEIGHT - STATUS_BAR_HEIGHT;
        if y >= tab_y && y < tab_y + SHEET_TAB_HEIGHT {
            let tab_x = x;
            let tab_idx = (tab_x / SHEET_TAB_WIDTH) as usize;
            if tab_idx < self.sheets.len() {
                // Nothing to reset afterwards: the selection and the offset
                // both live on the sheet, so this returns it exactly as it was
                // left. This used to clear both — carrying the *old* sheet's
                // offset in and then snapping the selection to A1 to
                // compensate, which lost your place on both sheets at once.
                self.sheets.set_active(tab_idx);
            } else if tab_idx == self.sheets.len() {
                // "+" button to add sheet
                self.add_sheet();
            }
            return EventResult::Consumed;
        }

        // Check column header resize.
        //
        // The handle is a column's *right* edge, so the position wanted is
        // `col_screen_x(col + 1)` — which for the last column is
        // `col_screen_x(MAX_COLS)`, hence the inclusive-looking `..=`. Both
        // this loop and the row one below ran their own accumulator that
        // subtracted the scroll offset from every column, frozen or not, so
        // with panes frozen the handle for a pinned column sat one scroll
        // offset away from the divider the user was aiming at.
        let header_y = self.grid_top() - COL_HEADER_HEIGHT;
        if y >= header_y && y < header_y + COL_HEADER_HEIGHT {
            for col in 0..MAX_COLS {
                let edge = self.col_screen_x(col) + self.active_sheet().col_width(col);
                if edge < ROW_HEADER_WIDTH {
                    continue;
                }
                if edge > self.grid_right() {
                    break;
                }
                if (x - edge).abs() < RESIZE_HANDLE_SIZE {
                    self.mode = InteractionMode::ColResize {
                        col,
                        start_x: x,
                        original_width: self.active_sheet().col_width(col),
                    };
                    return EventResult::Consumed;
                }
            }
            return EventResult::Consumed;
        }

        // Check row header resize
        if x < ROW_HEADER_WIDTH {
            let grid_top = self.grid_top();
            for row in 0..MAX_ROWS {
                let edge = self.row_screen_y(row) + self.active_sheet().row_height(row);
                if edge < grid_top {
                    continue;
                }
                if edge > self.grid_bottom() {
                    break;
                }
                if (y - edge).abs() < RESIZE_HANDLE_SIZE {
                    self.mode = InteractionMode::RowResize {
                        row,
                        start_y: y,
                        original_height: self.active_sheet().row_height(row),
                    };
                    return EventResult::Consumed;
                }
            }
            return EventResult::Consumed;
        }

        // Cell click — begin selection
        if let InteractionMode::Editing { .. } = &self.mode {
            self.confirm_edit();
        }

        // Check the auto-fill handle first: it is the smaller, more specific
        // target, and it sits on the active cell's bottom-right corner, which
        // a plain cell hit test would read as one of the four cells that meet
        // there. Its position comes from the layout law rather than a fourth
        // hand-rolled copy of it — the copy that was here subtracted the scroll
        // offset from a frozen cell, putting the handle a screenful away from
        // the outline that marks it.
        let active = self.selection().active;
        let handle_x = self.col_screen_x(active.col) + self.active_sheet().col_width(active.col);
        let handle_y = self.row_screen_y(active.row) + self.active_sheet().row_height(active.row);
        if (x - handle_x).abs() < AUTOFILL_HANDLE_SIZE
            && (y - handle_y).abs() < AUTOFILL_HANDLE_SIZE
        {
            self.mode = InteractionMode::AutoFill {
                anchor_range: self.selection().primary_range(),
                current_end: active,
            };
            return EventResult::Consumed;
        }

        // Not a cell: the toolbar, the formula bar, the scrollbars. Left
        // unconsumed so that whatever eventually handles those can see it,
        // rather than silently selecting A1 as this did before.
        let Some((col, row)) = self.cell_at_position(x, y) else {
            return EventResult::Ignored;
        };
        let addr = CellAddr::new(col, row);

        *self.selection_mut() = Selection::single(addr);
        self.mode = InteractionMode::RangeSelect { anchor: addr };
        self.ensure_cell_visible(addr);
        EventResult::Consumed
    }

    /// Handle left mouse button release.
    fn handle_left_release(&mut self, _x: f32, _y: f32) -> EventResult {
        match &self.mode {
            InteractionMode::RangeSelect { .. } => {
                self.mode = InteractionMode::Normal;
                EventResult::Consumed
            }
            InteractionMode::ColResize {
                col,
                original_width,
                start_x,
                ..
            } => {
                let col = *col;
                let original_width = *original_width;
                let _ = *start_x;
                let new_width = self.active_sheet().col_width(col);
                if (new_width - original_width).abs() > 0.5 {
                    self.undo_manager.push_action(UndoAction::ColResize {
                        sheet_idx: self.sheets.active_index(),
                        col,
                        old_width: original_width,
                        new_width,
                    });
                }
                self.mode = InteractionMode::Normal;
                EventResult::Consumed
            }
            InteractionMode::RowResize {
                row,
                original_height,
                ..
            } => {
                let row = *row;
                let original_height = *original_height;
                let new_height = self.active_sheet().row_height(row);
                if (new_height - original_height).abs() > 0.5 {
                    self.undo_manager.push_action(UndoAction::RowResize {
                        sheet_idx: self.sheets.active_index(),
                        row,
                        old_height: original_height,
                        new_height,
                    });
                }
                self.mode = InteractionMode::Normal;
                EventResult::Consumed
            }
            InteractionMode::AutoFill {
                anchor_range,
                current_end,
            } => {
                let source = *anchor_range;
                let end = *current_end;
                self.mode = InteractionMode::Normal;
                self.auto_fill(source, end);
                EventResult::Consumed
            }
            _ => {
                self.mode = InteractionMode::Normal;
                EventResult::Consumed
            }
        }
    }

    /// Handle mouse move.
    fn handle_mouse_move(&mut self, x: f32, y: f32) -> EventResult {
        match self.mode.clone() {
            InteractionMode::RangeSelect { anchor } => {
                let Some((col, row)) = self.cell_nearest_position(x, y) else {
                    return EventResult::Consumed;
                };
                let end = CellAddr::new(col, row);
                self.selection_mut().active = end;
                self.selection_mut().ranges = vec![CellRange::new(anchor, end)];
                EventResult::Consumed
            }
            InteractionMode::ColResize {
                col,
                start_x,
                original_width,
            } => {
                let delta = x - start_x;
                let new_width = (original_width + delta).max(MIN_COL_WIDTH);
                if let Some(w) = self.active_sheet_mut().col_widths.get_mut(col) {
                    *w = new_width;
                }
                // Narrowing a column shrinks the sheet, which lowers
                // `max_scroll_x` — an offset left past the new limit would show
                // blank space to the right of the last column.
                self.clamp_scroll();
                EventResult::Consumed
            }
            InteractionMode::RowResize {
                row,
                start_y,
                original_height,
            } => {
                let delta = y - start_y;
                let new_height = (original_height + delta).max(MIN_ROW_HEIGHT);
                if let Some(h) = self.active_sheet_mut().row_heights.get_mut(row) {
                    *h = new_height;
                }
                self.clamp_scroll();
                EventResult::Consumed
            }
            InteractionMode::AutoFill { anchor_range, .. } => {
                let Some((col, row)) = self.cell_nearest_position(x, y) else {
                    return EventResult::Consumed;
                };
                let anchor_end = anchor_range.end();
                let end = CellAddr::new(col.max(anchor_end.col), row.max(anchor_end.row));
                self.mode = InteractionMode::AutoFill {
                    anchor_range,
                    current_end: end,
                };
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Which cell is drawn at a window position, or `None` if no cell is.
    ///
    /// The `Option` is the point. This used to return a cell for *any*
    /// coordinate — clamping a negative offset to zero and handing back A1 —
    /// so a click on the toolbar, the formula bar, the scrollbars or the empty
    /// space past the last column all selected A1, and dragging a selection up
    /// into the toolbar snapped its end to row 0. There is no cell there; the
    /// only honest answer is `None`, and the callers now each decide what that
    /// means for them.
    ///
    /// A position inside a frozen band is read against *unscrolled* content,
    /// because that is where the renderer draws it — see the layout law next
    /// to [`col_screen_x`](Self::col_screen_x). Reading it against the scroll
    /// offset, which is what this did before, returned a cell further down or
    /// to the right by exactly that offset.
    pub fn cell_at_position(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let grid_top = self.grid_top();
        if x < ROW_HEADER_WIDTH || x >= self.grid_right() {
            return None;
        }
        if y < grid_top || y >= self.grid_bottom() {
            return None;
        }

        // Inverse of `col_screen_x` / `row_screen_y`: inside a frozen band the
        // renderer did not subtract the offset, so neither does this.
        let content_x = if x < self.frozen_right() {
            x - ROW_HEADER_WIDTH
        } else {
            x - ROW_HEADER_WIDTH + self.scroll().x
        };
        let content_y = if y < self.frozen_bottom() {
            y - grid_top
        } else {
            y - grid_top + self.scroll().y
        };

        let sheet = self.active_sheet();
        Some((sheet.col_at_x(content_x), sheet.row_at_y(content_y)))
    }

    /// The cell nearest a window position — for a *drag*, which is allowed to
    /// leave the grid.
    ///
    /// A drag is not a click. Pulling the pointer past the edge while selecting
    /// a range is ordinary, and means "as far as I got", so the useful answer
    /// is the edge cell rather than the `None` a click deserves. Clamping into
    /// the viewport before hit testing is what makes the range *stop* at the
    /// edge; the unranged hit test this replaces snapped the range's end to A1
    /// the moment the pointer crossed into the toolbar, silently discarding
    /// the selection the user was halfway through making.
    fn cell_nearest_position(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        // `clamp` panics when its bounds are inverted, which a zero-width
        // window would do; the `max` keeps them ordered, and the hit test then
        // returns `None` for the degenerate viewport of its own accord.
        let right = (self.grid_right() - 1.0).max(ROW_HEADER_WIDTH);
        let bottom = (self.grid_bottom() - 1.0).max(self.grid_top());
        let x = x.clamp(ROW_HEADER_WIDTH, right);
        let y = y.clamp(self.grid_top(), bottom);
        self.cell_at_position(x, y)
    }

    /// Handle a resize event.
    pub fn handle_resize(&mut self, width: u32, height: u32) {
        self.window_width = width as f32;
        self.window_height = height as f32;
        // Growing the window lowers both limits — the viewport got bigger, so
        // there is less sheet left to reach. Without this, maximising a window
        // that was scrolled to the bottom leaves it showing blank space past
        // the last row, and the wheel cannot get back because the offset it is
        // stuck at is already the largest one it will accept.
        self.clamp_scroll();
    }

    /// Process a top-level event.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_event) => {
                let result = self.handle_key_event(key_event);
                if result == EventResult::Consumed {
                    // If we were editing and pressed Enter/Tab/Escape, confirm
                    if let InteractionMode::Editing { .. } = &self.mode {
                        match key_event.key {
                            Key::Enter | Key::Tab => self.confirm_edit(),
                            Key::Escape => self.cancel_edit(),
                            _ => {}
                        }
                    }
                }
                result
            }
            Event::Mouse(mouse_event) => self.handle_mouse_event(mouse_event),
            Event::Resize { width, height } => {
                self.handle_resize(*width, *height);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire spreadsheet UI to a list of render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(2000);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        let mut y_offset = 0.0;

        // Toolbar
        if self.show_toolbar {
            self.render_toolbar(&mut cmds, y_offset);
            y_offset += TOOLBAR_HEIGHT;
        }

        // Formula bar
        if self.show_formula_bar {
            self.render_formula_bar(&mut cmds, y_offset);
            y_offset += FORMULA_BAR_HEIGHT;
        }

        // Column headers
        self.render_col_headers(&mut cmds, y_offset);
        y_offset += COL_HEADER_HEIGHT;

        // Row headers + cell grid
        self.render_grid(&mut cmds, y_offset);

        // Sheet tabs
        let tab_y = self.window_height
            - SHEET_TAB_HEIGHT
            - if self.show_status_bar {
                STATUS_BAR_HEIGHT
            } else {
                0.0
            };
        self.render_sheet_tabs(&mut cmds, tab_y);

        // Status bar
        if self.show_status_bar {
            self.render_status_bar(&mut cmds, self.window_height - STATUS_BAR_HEIGHT);
        }

        // Scrollbars
        self.render_scrollbars(&mut cmds);

        // Find/replace overlay
        if self.find_replace.active {
            self.render_find_replace(&mut cmds);
        }

        cmds
    }

    /// Render the toolbar with formatting buttons.
    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>, y: f32) {
        // Toolbar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: TOOLBAR_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Toolbar separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TOOLBAR_HEIGHT - 1.0,
            x2: self.window_width,
            y2: y + TOOLBAR_HEIGHT - 1.0,
            color: COLOR_SURFACE0,
            width: 1.0,
        });

        let btn_y = y + 4.0;
        let btn_h = TOOLBAR_HEIGHT - 8.0;
        let btn_w = 32.0;
        let mut bx = 8.0;

        // Bold button
        let bold_active = self
            .active_sheet()
            .get_cell(self.selection().active)
            .format
            .bold;
        self.render_toolbar_button(cmds, bx, btn_y, btn_w, btn_h, "B", bold_active, true);
        bx += btn_w + 4.0;

        // Italic button
        let italic_active = self
            .active_sheet()
            .get_cell(self.selection().active)
            .format
            .italic;
        self.render_toolbar_button(cmds, bx, btn_y, btn_w, btn_h, "I", italic_active, false);
        bx += btn_w + 4.0;

        // Separator
        cmds.push(RenderCommand::Line {
            x1: bx,
            y1: btn_y + 2.0,
            x2: bx,
            y2: btn_y + btn_h - 2.0,
            color: COLOR_SURFACE1,
            width: 1.0,
        });
        bx += 8.0;

        // Alignment buttons
        let alignment_labels = ["L", "C", "R"];
        let alignments = [Alignment::Left, Alignment::Center, Alignment::Right];
        let current_align = self
            .active_sheet()
            .get_cell(self.selection().active)
            .format
            .alignment;
        for (label, align) in alignment_labels.iter().zip(alignments.iter()) {
            let active = current_align == *align;
            self.render_toolbar_button(cmds, bx, btn_y, btn_w, btn_h, label, active, false);
            bx += btn_w + 4.0;
        }

        // Separator
        cmds.push(RenderCommand::Line {
            x1: bx,
            y1: btn_y + 2.0,
            x2: bx,
            y2: btn_y + btn_h - 2.0,
            color: COLOR_SURFACE1,
            width: 1.0,
        });
        bx += 8.0;

        // Format buttons
        let format_labels = ["$", "%", ".0"];
        for label in &format_labels {
            self.render_toolbar_button(cmds, bx, btn_y, btn_w + 4.0, btn_h, label, false, false);
            bx += btn_w + 8.0;
        }

        // Separator
        cmds.push(RenderCommand::Line {
            x1: bx,
            y1: btn_y + 2.0,
            x2: bx,
            y2: btn_y + btn_h - 2.0,
            color: COLOR_SURFACE1,
            width: 1.0,
        });
        bx += 8.0;

        // Border toggle
        let has_borders = self
            .active_sheet()
            .get_cell(self.selection().active)
            .format
            .borders
            .has_any();
        self.render_toolbar_button(
            cmds,
            bx,
            btn_y,
            btn_w + 8.0,
            btn_h,
            "Bdr",
            has_borders,
            false,
        );
        bx += btn_w + 16.0;

        // Freeze panes
        let frozen = self.active_sheet().frozen_cols > 0 || self.active_sheet().frozen_rows > 0;
        self.render_toolbar_button(
            cmds,
            bx,
            btn_y,
            btn_w + 16.0,
            btn_h,
            "Freeze",
            frozen,
            false,
        );
        bx += btn_w + 24.0;

        // Sort buttons
        self.render_toolbar_button(cmds, bx, btn_y, btn_w + 4.0, btn_h, "A-Z", false, false);
        bx += btn_w + 8.0;
        self.render_toolbar_button(cmds, bx, btn_y, btn_w + 4.0, btn_h, "Z-A", false, false);
    }

    /// Render a single toolbar button. Nine parameters reflect the per-button
    /// rendering primitive (canvas, position, size, label, two state flags) —
    /// bundling them into a struct would just push the same data around.
    #[allow(clippy::too_many_arguments)]
    fn render_toolbar_button(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
        active: bool,
        bold: bool,
    ) {
        let bg = if active {
            COLOR_SURFACE1
        } else {
            COLOR_SURFACE0
        };
        let fg = if active { COLOR_BLUE } else { COLOR_TEXT };

        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });

        let font_weight = if bold {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        };
        cmds.push(RenderCommand::Text {
            x: text::center_x(label, x + w / 2.0, SMALL_FONT, font_weight),
            y: y + h / 2.0 - 5.0,
            text: label.to_string(),
            font_size: SMALL_FONT,
            color: fg,
            font_weight,
            max_width: Some(w),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the formula bar.
    fn render_formula_bar(&self, cmds: &mut Vec<RenderCommand>, y: f32) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: FORMULA_BAR_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Cell address label
        let addr_text = self.selection().active.display();
        cmds.push(RenderCommand::FillRect {
            x: 4.0,
            y: y + 3.0,
            width: 60.0,
            height: FORMULA_BAR_HEIGHT - 6.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: 10.0,
            y: y + 7.0,
            text: addr_text,
            font_size: FONT_SIZE,
            color: COLOR_BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(54.0),
            overflow: TextOverflow::Ellipsis,
        });

        // "fx" label
        cmds.push(RenderCommand::Text {
            x: 72.0,
            y: y + 7.0,
            text: "fx".to_string(),
            font_size: FONT_SIZE,
            color: COLOR_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Formula/value text area
        cmds.push(RenderCommand::FillRect {
            x: 96.0,
            y: y + 3.0,
            width: self.window_width - 100.0,
            height: FORMULA_BAR_HEIGHT - 6.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });

        let formula_text = if let InteractionMode::Editing { ref buffer } = self.mode {
            buffer.text().to_owned()
        } else {
            let cell = self.active_sheet().get_cell(self.selection().active);
            if cell.is_formula() {
                cell.raw_input.clone()
            } else {
                cell.display_text()
            }
        };

        cmds.push(RenderCommand::Text {
            x: 102.0,
            y: y + 7.0,
            text: formula_text,
            font_size: FONT_SIZE,
            color: COLOR_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.window_width - 112.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Bottom separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + FORMULA_BAR_HEIGHT - 1.0,
            x2: self.window_width,
            y2: y + FORMULA_BAR_HEIGHT - 1.0,
            color: COLOR_SURFACE0,
            width: 1.0,
        });
    }

    /// Render column headers (A, B, C, ...).
    fn render_col_headers(&self, cmds: &mut Vec<RenderCommand>, y: f32) {
        let sheet = self.active_sheet();

        // Header background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: COL_HEADER_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top-left corner cell
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: ROW_HEADER_WIDTH,
            height: COL_HEADER_HEIGHT,
            color: COLOR_CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Column labels, in two bands.
        //
        // The scrolling columns are drawn first and clipped to the region to
        // the right of the frozen band; the frozen ones are drawn second, over
        // the top. Order and clip are both load-bearing: a single loop in
        // column order draws column A (frozen, pinned at the left) *before*
        // column D (scrolled underneath it), so D lands on top of A and the
        // pinned header is replaced by the one it was pinned to outrank. The
        // clip stops the column straddling the boundary from doing the same
        // with its left half, which no draw order can fix.
        let frozen_cols = sheet.frozen_cols;
        let frozen_right = self.frozen_right();
        let grid_right = self.grid_right();

        self.push_clip_rect(cmds, frozen_right, y, grid_right - frozen_right, COL_HEADER_HEIGHT);
        for col in frozen_cols..MAX_COLS {
            self.render_col_header(cmds, col, y);
        }
        cmds.push(RenderCommand::PopClip);

        self.push_clip_rect(
            cmds,
            ROW_HEADER_WIDTH,
            y,
            frozen_right - ROW_HEADER_WIDTH,
            COL_HEADER_HEIGHT,
        );
        for col in 0..frozen_cols.min(MAX_COLS) {
            self.render_col_header(cmds, col, y);
        }
        cmds.push(RenderCommand::PopClip);

        // Bottom separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + COL_HEADER_HEIGHT - 1.0,
            x2: self.window_width,
            y2: y + COL_HEADER_HEIGHT - 1.0,
            color: COLOR_SURFACE0,
            width: 1.0,
        });
    }

    /// Push a clip rectangle, collapsing a negative extent to nothing.
    ///
    /// A window narrower than its own row header gives `grid_right() -
    /// frozen_right()` a negative width, and a clip with a negative extent is
    /// not a small clip — depending on the backend it is either an empty one or
    /// an unbounded one, and "unbounded" would silently undo the clipping the
    /// callers depend on for correctness rather than for looks.
    fn push_clip_rect(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        cmds.push(RenderCommand::PushClip {
            x,
            y,
            width: width.max(0.0),
            height: height.max(0.0),
        });
    }

    /// One column's header cell, positioned by the layout law.
    fn render_col_header(&self, cmds: &mut Vec<RenderCommand>, col: usize, y: f32) {
        let w = self.active_sheet().col_width(col);
        let header_x = self.col_screen_x(col);
        if header_x + w < ROW_HEADER_WIDTH || header_x > self.grid_right() {
            return;
        }

        let is_selected = self
            .selection()
            .ranges
            .iter()
            .any(|r| col >= r.start().col && col <= r.end().col);

        let bg = if is_selected {
            COLOR_SURFACE1
        } else {
            COLOR_MANTLE
        };
        cmds.push(RenderCommand::FillRect {
            x: header_x,
            y,
            width: w,
            height: COL_HEADER_HEIGHT,
            color: bg,
            corner_radii: CornerRadii::ZERO,
        });

        let text_color = if is_selected {
            COLOR_BLUE
        } else {
            COLOR_SUBTEXT1
        };
        cmds.push(RenderCommand::Text {
            x: header_x + w / 2.0 - 4.0,
            y: y + 5.0,
            text: CellAddr::col_letter(col),
            font_size: HEADER_FONT,
            color: text_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w),
            overflow: TextOverflow::Ellipsis,
        });

        // Vertical separator
        cmds.push(RenderCommand::Line {
            x1: header_x + w,
            y1: y,
            x2: header_x + w,
            y2: y + COL_HEADER_HEIGHT,
            color: COLOR_SURFACE0,
            width: 1.0,
        });
    }

    /// Draw the rows in `rows`, clipped vertically to `[clip_y, clip_y +
    /// clip_h)`.
    ///
    /// Two of these make the grid — the frozen row band and the rows that
    /// scroll under it — and each splits horizontally into a frozen column band
    /// and the columns that scroll under *that*. Between them the four calls
    /// cover the four quadrants of a frozen-pane grid, each with the clip that
    /// keeps its contents out of the others.
    fn render_row_band(
        &self,
        cmds: &mut Vec<RenderCommand>,
        rows: core::ops::Range<usize>,
        clip_y: f32,
        clip_h: f32,
    ) {
        if rows.is_empty() || clip_h <= 0.0 {
            return;
        }
        let frozen_cols = self.active_sheet().frozen_cols.min(MAX_COLS);
        let frozen_right = self.frozen_right();
        let grid_right = self.grid_right();

        // Row headers sit left of the grid and never scroll horizontally, so
        // they are their own band with only the vertical clip.
        self.push_clip_rect(cmds, 0.0, clip_y, ROW_HEADER_WIDTH, clip_h);
        for row in rows.clone() {
            self.render_row_header(cmds, row);
        }
        cmds.push(RenderCommand::PopClip);

        self.push_clip_rect(
            cmds,
            frozen_right,
            clip_y,
            grid_right - frozen_right,
            clip_h,
        );
        for row in rows.clone() {
            for col in frozen_cols..MAX_COLS {
                self.render_cell(cmds, col, row);
            }
        }
        cmds.push(RenderCommand::PopClip);

        if frozen_cols > 0 {
            self.push_clip_rect(
                cmds,
                ROW_HEADER_WIDTH,
                clip_y,
                frozen_right - ROW_HEADER_WIDTH,
                clip_h,
            );
            for row in rows {
                for col in 0..frozen_cols {
                    self.render_cell(cmds, col, row);
                }
            }
            cmds.push(RenderCommand::PopClip);
        }
    }

    /// One row's header cell, positioned by the layout law.
    fn render_row_header(&self, cmds: &mut Vec<RenderCommand>, row: usize) {
        let row_h = self.active_sheet().row_height(row);
        let row_y = self.row_screen_y(row);

        let is_row_selected = self
            .selection()
            .ranges
            .iter()
            .any(|r| row >= r.start().row && row <= r.end().row);
        let header_bg = if is_row_selected {
            COLOR_SURFACE1
        } else {
            COLOR_MANTLE
        };
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: row_y,
            width: ROW_HEADER_WIDTH,
            height: row_h,
            color: header_bg,
            corner_radii: CornerRadii::ZERO,
        });

        let text_color = if is_row_selected {
            COLOR_BLUE
        } else {
            COLOR_SUBTEXT1
        };
        cmds.push(RenderCommand::Text {
            x: 4.0,
            y: row_y + row_h / 2.0 - 6.0,
            text: CellAddr::row_label(row),
            font_size: HEADER_FONT,
            color: text_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(ROW_HEADER_WIDTH - 8.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Row header separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: row_y + row_h,
            x2: ROW_HEADER_WIDTH,
            y2: row_y + row_h,
            color: COLOR_SURFACE0,
            width: 1.0,
        });
    }

    /// One cell: background, gridlines, borders and text.
    ///
    /// Positioned entirely by the layout law, so this cannot disagree with the
    /// hit test about where it put itself. The caller's clip decides whether
    /// any of it survives; this only skips the cells that are nowhere near, to
    /// keep the command list from growing by the whole sheet.
    fn render_cell(&self, cmds: &mut Vec<RenderCommand>, col: usize, row: usize) {
        let sheet = self.active_sheet();
        let (col_w, row_h) = (sheet.col_width(col), sheet.row_height(row));
        let (cell_x, row_y) = (self.col_screen_x(col), self.row_screen_y(row));
        if cell_x + col_w < ROW_HEADER_WIDTH || cell_x > self.grid_right() {
            return;
        }
        if row_y + row_h < self.grid_top() || row_y > self.grid_bottom() {
            return;
        }

        let addr = CellAddr::new(col, row);
        let cell = sheet.get_cell(addr);
        let is_selected = self.selection().contains(addr);
        let is_active = addr == self.selection().active;

        // Cell background
        let bg_color = if let Some(bg) = cell.format.bg_color {
            bg
        } else if is_active {
            COLOR_SURFACE0
        } else if is_selected {
            Color::rgba(COLOR_BLUE.r, COLOR_BLUE.g, COLOR_BLUE.b, 30)
        } else {
            COLOR_BASE
        };

        cmds.push(RenderCommand::FillRect {
            x: cell_x,
            y: row_y,
            width: col_w,
            height: row_h,
            color: bg_color,
            corner_radii: CornerRadii::ZERO,
        });

        // Gridlines
        if self.show_gridlines {
            cmds.push(RenderCommand::Line {
                x1: cell_x + col_w,
                y1: row_y,
                x2: cell_x + col_w,
                y2: row_y + row_h,
                color: COLOR_SURFACE0,
                width: 1.0,
            });
            cmds.push(RenderCommand::Line {
                x1: cell_x,
                y1: row_y + row_h,
                x2: cell_x + col_w,
                y2: row_y + row_h,
                color: COLOR_SURFACE0,
                width: 1.0,
            });
        }

        // Cell borders
        if cell.format.borders.has_any() {
            let border_color = COLOR_TEXT;
            if cell.format.borders.top {
                cmds.push(RenderCommand::Line {
                    x1: cell_x,
                    y1: row_y,
                    x2: cell_x + col_w,
                    y2: row_y,
                    color: border_color,
                    width: 1.5,
                });
            }
            if cell.format.borders.bottom {
                cmds.push(RenderCommand::Line {
                    x1: cell_x,
                    y1: row_y + row_h,
                    x2: cell_x + col_w,
                    y2: row_y + row_h,
                    color: border_color,
                    width: 1.5,
                });
            }
            if cell.format.borders.left {
                cmds.push(RenderCommand::Line {
                    x1: cell_x,
                    y1: row_y,
                    x2: cell_x,
                    y2: row_y + row_h,
                    color: border_color,
                    width: 1.5,
                });
            }
            if cell.format.borders.right {
                cmds.push(RenderCommand::Line {
                    x1: cell_x + col_w,
                    y1: row_y,
                    x2: cell_x + col_w,
                    y2: row_y + row_h,
                    color: border_color,
                    width: 1.5,
                });
            }
        }

        // Cell text
        let display_text = if is_active {
            if let InteractionMode::Editing { ref buffer } = self.mode {
                buffer.text().to_owned()
            } else {
                cell.display_text()
            }
        } else {
            cell.display_text()
        };

        if display_text.is_empty() {
            return;
        }

        let text_color = cell.format.text_color.unwrap_or(match &cell.value {
            CellValue::Error(_) => COLOR_RED,
            CellValue::Boolean(_) => COLOR_PEACH,
            CellValue::Number(_) => COLOR_TEXT,
            _ => COLOR_TEXT,
        });

        let font_weight = if cell.format.bold {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        };

        // A spreadsheet's columns are pixel widths, not character cells, so
        // alignment is a measurement — and it has to be taken in the cell's own
        // weight, since a bold cell drawn with a regular-weight offset is the
        // one that drifts out of its column.
        let text_x = match cell.format.alignment {
            Alignment::Left => cell_x + 4.0,
            Alignment::Center => {
                text::center_x(&display_text, cell_x + col_w / 2.0, FONT_SIZE, font_weight)
            }
            Alignment::Right => {
                text::right_x(&display_text, cell_x + col_w - 4.0, FONT_SIZE, font_weight)
            }
        };

        cmds.push(RenderCommand::Text {
            x: text_x,
            y: row_y + row_h / 2.0 - 6.0,
            text: display_text,
            font_size: FONT_SIZE,
            color: text_color,
            font_weight,
            max_width: Some(col_w - 8.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the cell grid, row headers, and cell contents.
    fn render_grid(&self, cmds: &mut Vec<RenderCommand>, y_start: f32) {
        let sheet = self.active_sheet();
        let grid_w = self.grid_width();
        let grid_h = self.grid_height();
        let frozen_cols = sheet.frozen_cols;
        let frozen_rows = sheet.frozen_rows;

        // Clip grid area
        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: y_start,
            width: self.window_width,
            height: grid_h,
        });

        // Draw grid background
        cmds.push(RenderCommand::FillRect {
            x: ROW_HEADER_WIDTH,
            y: y_start,
            width: grid_w,
            height: grid_h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // The grid is drawn as four quadrants, which is what a frozen pane
        // means: rows and columns each split into a pinned band and the ones
        // that scroll under it.
        //
        // Two things make it correct, and the old single loop over
        // `0..MAX_COLS` had neither. **Order**: the scrolling band is drawn
        // first and the pinned one over the top, because a loop in index order
        // draws frozen column A before scrolled column D, and D — having slid
        // underneath A — then paints over the very thing that was pinned to
        // outrank it. **Clip**: the column straddling the boundary is half in
        // each band, and no draw order fixes a half. Both bands therefore get a
        // clip rectangle, and the clip is load-bearing for correctness here
        // rather than merely tidying the edges.
        let frozen_bottom = self.frozen_bottom();
        let grid_bottom = y_start + grid_h;

        // The scrolling rows: from the first one not hidden under the frozen
        // band down to the last one the pane can show.
        let first_scrolling = sheet
            .row_at_y(self.scroll().y + (frozen_bottom - y_start))
            .max(frozen_rows);
        let last_scrolling = sheet
            .row_at_y(self.scroll().y + grid_h)
            .min(MAX_ROWS.saturating_sub(1));
        self.render_row_band(
            cmds,
            first_scrolling..last_scrolling.saturating_add(1),
            frozen_bottom,
            grid_bottom - frozen_bottom,
        );

        // The frozen rows, over the top.
        self.render_row_band(
            cmds,
            0..frozen_rows.min(MAX_ROWS),
            y_start,
            frozen_bottom - y_start,
        );

        // Where a rectangle of cells lands on screen, in pixels.
        //
        // Written once rather than the three times it was: the active-cell
        // outline, each selection outline and the auto-fill preview all need
        // exactly this. The corner comes from the same `col_screen_x` /
        // `row_screen_y` the cells themselves were drawn by, so an outline
        // cannot land where its own cell is not — which is what this closure's
        // hand-rolled copy of the frozen-pane subtraction used to risk.
        let range_rect = |range: CellRange| -> (f32, f32, f32, f32) {
            let (start, end) = (range.start(), range.end());
            let x = self.col_screen_x(start.col);
            let y = self.row_screen_y(start.row);
            let w: f32 = (start.col..=end.col).map(|c| sheet.col_width(c)).sum();
            let h: f32 = (start.row..=end.row).map(|r| sheet.row_height(r)).sum();
            (x, y, w, h)
        };

        // Active cell outline
        let active = self.selection().active;
        let (active_x, active_y, active_w, active_h) = range_rect(CellRange::single(active));

        cmds.push(RenderCommand::StrokeRect {
            x: active_x,
            y: active_y,
            width: active_w,
            height: active_h,
            color: COLOR_BLUE,
            line_width: 2.0,
            corner_radii: CornerRadii::ZERO,
        });

        // Auto-fill handle (small square at bottom-right of active cell)
        let handle_size = AUTOFILL_HANDLE_SIZE;
        cmds.push(RenderCommand::FillRect {
            x: active_x + active_w - handle_size / 2.0,
            y: active_y + active_h - handle_size / 2.0,
            width: handle_size,
            height: handle_size,
            color: COLOR_BLUE,
            corner_radii: CornerRadii::ZERO,
        });

        // Selection range highlight outline (for multi-cell selection)
        for range in &self.selection().ranges {
            if range.cell_count() > 1 {
                let (rx, ry, rw, rh) = range_rect(*range);

                cmds.push(RenderCommand::StrokeRect {
                    x: rx,
                    y: ry,
                    width: rw,
                    height: rh,
                    color: COLOR_BLUE,
                    line_width: 1.5,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }

        // Auto-fill preview highlight
        if let InteractionMode::AutoFill {
            anchor_range,
            current_end,
        } = &self.mode
        {
            let range = CellRange::new(anchor_range.start(), *current_end);
            let (rx, ry, rw, rh) = range_rect(range);

            cmds.push(RenderCommand::StrokeRect {
                x: rx,
                y: ry,
                width: rw,
                height: rh,
                color: COLOR_GREEN,
                line_width: 2.0,
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Freeze pane dividers
        if frozen_cols > 0 {
            let fx = ROW_HEADER_WIDTH + sheet.col_x_offset(frozen_cols);
            cmds.push(RenderCommand::Line {
                x1: fx,
                y1: y_start,
                x2: fx,
                y2: y_start + grid_h,
                color: COLOR_LAVENDER,
                width: 2.0,
            });
        }
        if frozen_rows > 0 {
            let fy = y_start + sheet.row_y_offset(frozen_rows);
            cmds.push(RenderCommand::Line {
                x1: 0.0,
                y1: fy,
                x2: self.window_width,
                y2: fy,
                color: COLOR_LAVENDER,
                width: 2.0,
            });
        }

        // Pop grid clip
        cmds.push(RenderCommand::PopClip);
    }

    /// Render sheet tabs at the bottom.
    fn render_sheet_tabs(&self, cmds: &mut Vec<RenderCommand>, y: f32) {
        // Tab bar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: SHEET_TAB_HEIGHT,
            color: COLOR_CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let mut tx = 4.0;
        for (idx, sheet) in self.sheets.iter().enumerate() {
            let is_active = idx == self.sheets.active_index();
            let bg = if is_active { COLOR_BASE } else { COLOR_MANTLE };
            let fg = if is_active {
                COLOR_BLUE
            } else {
                COLOR_SUBTEXT0
            };
            let radii = CornerRadii {
                top_left: 4.0,
                top_right: 4.0,
                bottom_left: 0.0,
                bottom_right: 0.0,
            };

            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: y + 2.0,
                width: SHEET_TAB_WIDTH,
                height: SHEET_TAB_HEIGHT - 2.0,
                color: bg,
                corner_radii: radii,
            });

            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: y + 8.0,
                text: sheet.name.clone(),
                font_size: SMALL_FONT,
                color: fg,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SHEET_TAB_WIDTH - 16.0),
                overflow: TextOverflow::Ellipsis,
            });

            tx += SHEET_TAB_WIDTH + 2.0;
        }

        // "+" button for new sheet
        cmds.push(RenderCommand::FillRect {
            x: tx,
            y: y + 2.0,
            width: 28.0,
            height: SHEET_TAB_HEIGHT - 2.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: tx + 8.0,
            y: y + 7.0,
            text: "+".to_string(),
            font_size: FONT_SIZE,
            color: COLOR_SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render the status bar.
    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>, y: f32) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: STATUS_BAR_HEIGHT,
            color: COLOR_CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Top separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.window_width,
            y2: y,
            color: COLOR_SURFACE0,
            width: 1.0,
        });

        // Status text (SUM/AVG/COUNT of selection)
        let status = self.status_bar_text();
        if !status.is_empty() {
            cmds.push(RenderCommand::Text {
                x: self.window_width - 400.0,
                y: y + 5.0,
                text: status,
                font_size: SMALL_FONT,
                color: COLOR_SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(390.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Mode indicator
        let mode_text = match &self.mode {
            InteractionMode::Normal => "Ready",
            InteractionMode::Editing { .. } => "Edit",
            InteractionMode::RangeSelect { .. } => "Select",
            InteractionMode::ColResize { .. } | InteractionMode::RowResize { .. } => "Resize",
            InteractionMode::AutoFill { .. } => "Fill",
            InteractionMode::FindReplace => "Find",
        };
        cmds.push(RenderCommand::Text {
            x: 8.0,
            y: y + 5.0,
            text: mode_text.to_string(),
            font_size: SMALL_FONT,
            color: COLOR_GREEN,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Selection range display
        let range_text = self.selection().primary_range().display();
        cmds.push(RenderCommand::Text {
            x: 80.0,
            y: y + 5.0,
            text: range_text,
            font_size: SMALL_FONT,
            color: COLOR_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render scrollbars.
    fn render_scrollbars(&self, cmds: &mut Vec<RenderCommand>) {
        let sheet = self.active_sheet();
        let grid_top = self.grid_top();
        let grid_h = self.grid_height();
        let total_content_h = sheet.row_y_offset(MAX_ROWS);
        let total_content_w = sheet.col_x_offset(MAX_COLS);

        // Vertical scrollbar track
        let vbar_x = self.window_width - SCROLLBAR_WIDTH;
        cmds.push(RenderCommand::FillRect {
            x: vbar_x,
            y: grid_top,
            width: SCROLLBAR_WIDTH,
            height: grid_h,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Vertical scrollbar thumb
        if total_content_h > grid_h {
            let thumb_ratio = grid_h / total_content_h;
            let thumb_h = (thumb_ratio * grid_h).max(20.0);
            let scroll_ratio = self.scroll().y / (total_content_h - grid_h);
            let thumb_y = grid_top + scroll_ratio * (grid_h - thumb_h);

            cmds.push(RenderCommand::FillRect {
                x: vbar_x + 2.0,
                y: thumb_y,
                width: SCROLLBAR_WIDTH - 4.0,
                height: thumb_h,
                color: COLOR_SURFACE1,
                corner_radii: CornerRadii::all(4.0),
            });
        }

        // Horizontal scrollbar track
        let hbar_y = grid_top + grid_h;
        let hbar_w = self.window_width - SCROLLBAR_WIDTH - ROW_HEADER_WIDTH;
        cmds.push(RenderCommand::FillRect {
            x: ROW_HEADER_WIDTH,
            y: hbar_y,
            width: hbar_w,
            height: SCROLLBAR_WIDTH,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Horizontal scrollbar thumb
        if total_content_w > hbar_w {
            let thumb_ratio = hbar_w / total_content_w;
            let thumb_w = (thumb_ratio * hbar_w).max(20.0);
            let scroll_ratio = self.scroll().x / (total_content_w - hbar_w);
            let thumb_x = ROW_HEADER_WIDTH + scroll_ratio * (hbar_w - thumb_w);

            cmds.push(RenderCommand::FillRect {
                x: thumb_x,
                y: hbar_y + 2.0,
                width: thumb_w,
                height: SCROLLBAR_WIDTH - 4.0,
                color: COLOR_SURFACE1,
                corner_radii: CornerRadii::all(4.0),
            });
        }
    }

    /// Render the find/replace overlay dialog.
    fn render_find_replace(&self, cmds: &mut Vec<RenderCommand>) {
        let dlg_w = 360.0;
        let dlg_h = 140.0;
        let dlg_x = self.window_width - dlg_w - 20.0;
        let dlg_y = 60.0;

        // Shadow
        cmds.push(RenderCommand::BoxShadow {
            x: dlg_x,
            y: dlg_y,
            width: dlg_w,
            height: dlg_h,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(8.0),
        });

        // Background
        cmds.push(RenderCommand::FillRect {
            x: dlg_x,
            y: dlg_y,
            width: dlg_w,
            height: dlg_h,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Border
        cmds.push(RenderCommand::StrokeRect {
            x: dlg_x,
            y: dlg_y,
            width: dlg_w,
            height: dlg_h,
            color: COLOR_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: dlg_x + 12.0,
            y: dlg_y + 12.0,
            text: "Find and Replace".to_string(),
            font_size: FONT_SIZE,
            color: COLOR_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Search field label
        cmds.push(RenderCommand::Text {
            x: dlg_x + 12.0,
            y: dlg_y + 40.0,
            text: "Find:".to_string(),
            font_size: SMALL_FONT,
            color: COLOR_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Search field
        cmds.push(RenderCommand::FillRect {
            x: dlg_x + 70.0,
            y: dlg_y + 35.0,
            width: 200.0,
            height: 22.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });

        cmds.push(RenderCommand::Text {
            x: dlg_x + 74.0,
            y: dlg_y + 39.0,
            text: self.find_replace.search_text.clone(),
            font_size: SMALL_FONT,
            color: COLOR_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(192.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Replace field label
        cmds.push(RenderCommand::Text {
            x: dlg_x + 12.0,
            y: dlg_y + 70.0,
            text: "Replace:".to_string(),
            font_size: SMALL_FONT,
            color: COLOR_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Replace field
        cmds.push(RenderCommand::FillRect {
            x: dlg_x + 70.0,
            y: dlg_y + 65.0,
            width: 200.0,
            height: 22.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });

        cmds.push(RenderCommand::Text {
            x: dlg_x + 74.0,
            y: dlg_y + 69.0,
            text: self.find_replace.replace_text.clone(),
            font_size: SMALL_FONT,
            color: COLOR_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(192.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Result count
        let count_text = format!("{} found", self.find_replace.result_count());
        cmds.push(RenderCommand::Text {
            x: dlg_x + 280.0,
            y: dlg_y + 40.0,
            text: count_text,
            font_size: SMALL_FONT,
            color: COLOR_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Buttons
        let btn_y = dlg_y + dlg_h - 34.0;
        let buttons = ["Find Next", "Replace", "Replace All"];
        let mut bx = dlg_x + 12.0;
        for label in &buttons {
            let bw = text::padded_width(label, 8.0, SMALL_FONT, FontWeightHint::Regular);
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: btn_y,
                width: bw,
                height: 24.0,
                color: COLOR_SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: btn_y + 5.0,
                text: label.to_string(),
                font_size: SMALL_FONT,
                color: COLOR_TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(bw - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            bx += bw + 8.0;
        }
    }
}

/// Handle keyboard input in editing mode. Returns Consumed if handled.
fn handle_editing_key(buffer: &mut EditBuffer, event: &KeyEvent) -> EventResult {
    match event.key {
        Key::Backspace => {
            buffer.backspace();
            EventResult::Consumed
        }
        Key::Delete => {
            buffer.delete();
            EventResult::Consumed
        }
        Key::Left => {
            buffer.move_left();
            EventResult::Consumed
        }
        Key::Right => {
            buffer.move_right();
            EventResult::Consumed
        }
        Key::Home => {
            buffer.move_home();
            EventResult::Consumed
        }
        Key::End => {
            buffer.move_end();
            EventResult::Consumed
        }
        Key::Enter | Key::Tab | Key::Escape => {
            // Let the caller handle these transitions
            EventResult::Consumed
        }
        _ => {
            if let Some(ch) = event.text
                && !ch.is_control()
            {
                buffer.insert(ch);
                return EventResult::Consumed;
            }
            EventResult::Ignored
        }
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let mut app = SpreadsheetApp::new(1280.0, 800.0);

    // Set up initial demo data
    app.set_cell_input(CellAddr::new(0, 0), "Item");
    app.set_cell_input(CellAddr::new(1, 0), "Price");
    app.set_cell_input(CellAddr::new(2, 0), "Qty");
    app.set_cell_input(CellAddr::new(3, 0), "Total");

    app.set_cell_input(CellAddr::new(0, 1), "Widget A");
    app.set_cell_input(CellAddr::new(1, 1), "10.50");
    app.set_cell_input(CellAddr::new(2, 1), "5");
    app.set_cell_input(CellAddr::new(3, 1), "=B2*C2");

    app.set_cell_input(CellAddr::new(0, 2), "Widget B");
    app.set_cell_input(CellAddr::new(1, 2), "25.00");
    app.set_cell_input(CellAddr::new(2, 2), "3");
    app.set_cell_input(CellAddr::new(3, 2), "=B3*C3");

    app.set_cell_input(CellAddr::new(0, 3), "Widget C");
    app.set_cell_input(CellAddr::new(1, 3), "7.99");
    app.set_cell_input(CellAddr::new(2, 3), "12");
    app.set_cell_input(CellAddr::new(3, 3), "=B4*C4");

    app.set_cell_input(CellAddr::new(3, 5), "=SUM(D2:D4)");

    // Bold the header row
    for col in 0..4 {
        let addr = CellAddr::new(col, 0);
        let mut cell = app.active_sheet().get_cell(addr);
        cell.format.bold = true;
        app.active_sheet_mut().set_cell(addr, cell);
    }

    recalculate_sheet(app.active_sheet_mut());

    // Render one frame to verify
    let _commands = app.render();
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

    // -- text measurement --

    #[test]
    fn a_centred_cell_is_centred_in_its_column() {
        // In the cell's own weight: a bold cell placed with a regular-weight
        // offset is the one that drifts out of its column.
        let col_w = 90.0;
        for (value, weight) in [
            ("1234.50", FontWeightHint::Regular),
            ("Total", FontWeightHint::Bold),
            ("Übertrag", FontWeightHint::Bold),
        ] {
            let x = text::center_x(value, col_w / 2.0, FONT_SIZE, weight);
            let w = text::measure(value, FONT_SIZE, weight);
            assert!(
                (x + w / 2.0 - col_w / 2.0).abs() < 0.01,
                "{value:?} is not centred in its column"
            );
        }
    }

    #[test]
    fn a_right_aligned_cell_ends_at_the_column_edge() {
        let col_w: f32 = 90.0;
        for value in ["1", "1234.50", "-99 %"] {
            let right = col_w - 4.0;
            let x = text::right_x(value, right, FONT_SIZE, FontWeightHint::Regular);
            let w = text::measure(value, FONT_SIZE, FontWeightHint::Regular);
            assert!(
                (x + w - right).abs() < 0.01,
                "{value:?} does not end at the column edge"
            );
        }
    }

    #[test]
    fn a_column_of_numbers_lines_up_on_the_right() {
        // The point of right alignment in a spreadsheet: the decimal points of
        // a column of figures have to sit above one another.
        let right = 86.0;
        for value in ["1.50", "23.50", "456.50"] {
            let end = text::right_x(value, right, FONT_SIZE, FontWeightHint::Regular)
                + text::measure(value, FONT_SIZE, FontWeightHint::Regular);
            assert!(
                (end - right).abs() < 0.01,
                "{value:?} ends at {end}, not {right}"
            );
        }
    }

    // -- CellAddr tests --

    #[test]
    fn test_cell_addr_new() {
        let addr = CellAddr::new(0, 0);
        assert_eq!(addr.col, 0);
        assert_eq!(addr.row, 0);
    }

    #[test]
    fn test_cell_addr_display() {
        assert_eq!(CellAddr::new(0, 0).display(), "A1");
        assert_eq!(CellAddr::new(1, 4).display(), "B5");
        assert_eq!(CellAddr::new(25, 998).display(), "Z999");
    }

    #[test]
    fn test_cell_addr_parse_valid() {
        assert_eq!(CellAddr::parse("A1"), Some(CellAddr::new(0, 0)));
        assert_eq!(CellAddr::parse("B5"), Some(CellAddr::new(1, 4)));
        assert_eq!(CellAddr::parse("Z999"), Some(CellAddr::new(25, 998)));
    }

    #[test]
    fn test_cell_addr_parse_lowercase() {
        assert_eq!(CellAddr::parse("a1"), Some(CellAddr::new(0, 0)));
        assert_eq!(CellAddr::parse("z999"), Some(CellAddr::new(25, 998)));
    }

    #[test]
    fn test_cell_addr_parse_invalid_empty() {
        assert_eq!(CellAddr::parse(""), None);
    }

    #[test]
    fn test_cell_addr_parse_invalid_no_number() {
        assert_eq!(CellAddr::parse("A"), None);
    }

    #[test]
    fn test_cell_addr_parse_invalid_zero_row() {
        assert_eq!(CellAddr::parse("A0"), None);
    }

    #[test]
    fn test_cell_addr_parse_invalid_too_large_row() {
        assert_eq!(CellAddr::parse("A1000"), None);
    }

    #[test]
    fn test_cell_addr_col_letter() {
        assert_eq!(CellAddr::col_letter(0), "A");
        assert_eq!(CellAddr::col_letter(25), "Z");
        assert_eq!(CellAddr::col_letter(26), "?");
    }

    // -- CellValue tests --

    #[test]
    fn test_cell_value_empty() {
        let v = CellValue::Empty;
        assert!(v.is_empty());
        assert_eq!(v.as_number(), None);
    }

    #[test]
    fn test_cell_value_number() {
        let v = CellValue::Number(42.0);
        assert!(!v.is_empty());
        assert_eq!(v.as_number(), Some(42.0));
    }

    #[test]
    fn test_cell_value_text() {
        let v = CellValue::Text("hello".to_string());
        assert!(!v.is_empty());
        assert_eq!(v.as_number(), None);
    }

    #[test]
    fn test_cell_value_text_numeric() {
        // Using 3.25 (exactly representable) avoids clippy::approx_constant
        // which flags any literal close to PI regardless of context.
        let v = CellValue::Text("3.25".to_string());
        assert_eq!(v.as_number(), Some(3.25));
    }

    #[test]
    fn test_cell_value_boolean_as_number() {
        assert_eq!(CellValue::Boolean(true).as_number(), Some(1.0));
        assert_eq!(CellValue::Boolean(false).as_number(), Some(0.0));
    }

    #[test]
    fn test_cell_value_display_empty() {
        let v = CellValue::Empty;
        assert_eq!(v.display_string(&NumberFormat::General), "");
    }

    #[test]
    fn test_cell_value_display_number_general() {
        let v = CellValue::Number(42.0);
        assert_eq!(v.display_string(&NumberFormat::General), "42");
    }

    #[test]
    fn test_cell_value_display_number_decimal() {
        // 3.55 — exactly representable, dodges clippy::approx_constant.
        let v = CellValue::Number(3.55_f64);
        assert_eq!(v.display_string(&NumberFormat::Decimal(2)), "3.55");
    }

    #[test]
    fn test_cell_value_display_boolean() {
        assert_eq!(
            CellValue::Boolean(true).display_string(&NumberFormat::General),
            "TRUE"
        );
        assert_eq!(
            CellValue::Boolean(false).display_string(&NumberFormat::General),
            "FALSE"
        );
    }

    // -- NumberFormat tests --

    #[test]
    fn test_format_general_integer() {
        assert_eq!(NumberFormat::General.format_number(100.0), "100");
    }

    #[test]
    fn test_format_general_float() {
        // 3.25 dodges clippy::approx_constant (PI proximity flag).
        let s = NumberFormat::General.format_number(3.25);
        assert!(s.starts_with("3.25"));
    }

    #[test]
    fn test_format_percentage() {
        assert_eq!(NumberFormat::Percentage(1).format_number(0.75), "75.0%");
    }

    #[test]
    fn test_format_currency() {
        assert_eq!(NumberFormat::Currency(2).format_number(9.99), "$9.99");
    }

    #[test]
    fn test_format_decimal_zero_places() {
        assert_eq!(NumberFormat::Decimal(0).format_number(3.7), "4");
    }

    #[test]
    fn test_format_default_is_general() {
        let fmt = NumberFormat::default();
        assert_eq!(fmt, NumberFormat::General);
    }

    // -- CellBorders tests --

    #[test]
    fn test_borders_none() {
        let b = CellBorders::none();
        assert!(!b.has_any());
    }

    #[test]
    fn test_borders_all() {
        let b = CellBorders::all();
        assert!(b.has_any());
        assert!(b.top && b.bottom && b.left && b.right);
    }

    // -- CellRange tests --

    #[test]
    fn test_range_single() {
        let r = CellRange::single(CellAddr::new(2, 3));
        assert_eq!(r.start(), CellAddr::new(2, 3));
        assert_eq!(r.end(), CellAddr::new(2, 3));
        assert_eq!(r.cell_count(), 1);
    }

    #[test]
    fn test_range_normalizes() {
        let r = CellRange::new(CellAddr::new(3, 5), CellAddr::new(1, 2));
        assert_eq!(r.start(), CellAddr::new(1, 2));
        assert_eq!(r.end(), CellAddr::new(3, 5));
    }

    #[test]
    fn test_range_contains() {
        let r = CellRange::new(CellAddr::new(1, 1), CellAddr::new(3, 3));
        assert!(r.contains(CellAddr::new(2, 2)));
        assert!(r.contains(CellAddr::new(1, 1)));
        assert!(r.contains(CellAddr::new(3, 3)));
        assert!(!r.contains(CellAddr::new(0, 0)));
        assert!(!r.contains(CellAddr::new(4, 4)));
    }

    #[test]
    fn test_range_dimensions() {
        let r = CellRange::new(CellAddr::new(1, 2), CellAddr::new(4, 7));
        assert_eq!(r.col_count(), 4);
        assert_eq!(r.row_count(), 6);
        assert_eq!(r.cell_count(), 24);
    }

    #[test]
    fn test_range_iter_count() {
        let r = CellRange::new(CellAddr::new(0, 0), CellAddr::new(2, 2));
        let cells: Vec<_> = r.iter().collect();
        assert_eq!(cells.len(), 9);
    }

    #[test]
    fn test_range_iter_order() {
        let r = CellRange::new(CellAddr::new(0, 0), CellAddr::new(1, 1));
        let cells: Vec<_> = r.iter().collect();
        assert_eq!(cells[0], CellAddr::new(0, 0));
        assert_eq!(cells[1], CellAddr::new(1, 0));
        assert_eq!(cells[2], CellAddr::new(0, 1));
        assert_eq!(cells[3], CellAddr::new(1, 1));
    }

    #[test]
    fn test_range_display_single() {
        let r = CellRange::single(CellAddr::new(0, 0));
        assert_eq!(r.display(), "A1");
    }

    #[test]
    fn test_range_display_multi() {
        let r = CellRange::new(CellAddr::new(0, 0), CellAddr::new(2, 4));
        assert_eq!(r.display(), "A1:C5");
    }

    #[test]
    fn test_range_parse_single() {
        let r = CellRange::parse("B3").unwrap();
        assert_eq!(r.start(), CellAddr::new(1, 2));
        assert_eq!(r.end(), CellAddr::new(1, 2));
    }

    #[test]
    fn test_range_parse_multi() {
        let r = CellRange::parse("A1:C5").unwrap();
        assert_eq!(r.start(), CellAddr::new(0, 0));
        assert_eq!(r.end(), CellAddr::new(2, 4));
    }

    #[test]
    fn test_range_parse_invalid() {
        assert!(CellRange::parse("").is_none());
        assert!(CellRange::parse("::").is_none());
    }

    // -- Cell tests --

    #[test]
    fn test_cell_default_is_empty() {
        let c = Cell::default();
        assert!(c.value.is_empty());
        assert!(c.raw_input.is_empty());
        assert!(!c.is_formula());
    }

    #[test]
    fn test_cell_is_formula() {
        let mut c = Cell::empty();
        c.raw_input = "=A1+B1".to_string();
        assert!(c.is_formula());
    }

    #[test]
    fn test_cell_is_not_formula() {
        let mut c = Cell::empty();
        c.raw_input = "hello".to_string();
        assert!(!c.is_formula());
    }

    // -- Sheet tests --

    #[test]
    fn test_sheet_new() {
        let s = Sheet::new("Test");
        assert_eq!(s.name, "Test");
        assert!(s.cells.is_empty());
    }

    #[test]
    fn test_sheet_get_cell_empty() {
        let s = Sheet::new("Test");
        let c = s.get_cell(CellAddr::new(0, 0));
        assert!(c.value.is_empty());
    }

    #[test]
    fn test_sheet_set_cell_input_number() {
        let mut s = Sheet::new("Test");
        s.set_cell_input(CellAddr::new(0, 0), "42");
        let c = s.get_cell(CellAddr::new(0, 0));
        assert_eq!(c.value, CellValue::Number(42.0));
    }

    #[test]
    fn test_sheet_set_cell_input_text() {
        let mut s = Sheet::new("Test");
        s.set_cell_input(CellAddr::new(0, 0), "hello");
        let c = s.get_cell(CellAddr::new(0, 0));
        assert_eq!(c.value, CellValue::Text("hello".to_string()));
    }

    #[test]
    fn test_sheet_set_cell_input_boolean() {
        let mut s = Sheet::new("Test");
        s.set_cell_input(CellAddr::new(0, 0), "TRUE");
        assert_eq!(
            s.get_cell(CellAddr::new(0, 0)).value,
            CellValue::Boolean(true)
        );
        s.set_cell_input(CellAddr::new(0, 1), "false");
        assert_eq!(
            s.get_cell(CellAddr::new(0, 1)).value,
            CellValue::Boolean(false)
        );
    }

    #[test]
    fn test_sheet_set_cell_input_formula() {
        let mut s = Sheet::new("Test");
        s.set_cell_input(CellAddr::new(0, 0), "=1+2");
        let c = s.get_cell(CellAddr::new(0, 0));
        assert!(c.is_formula());
    }

    #[test]
    fn test_sheet_set_cell_input_empty_removes() {
        let mut s = Sheet::new("Test");
        s.set_cell_input(CellAddr::new(0, 0), "42");
        assert!(!s.cells.is_empty());
        s.set_cell_input(CellAddr::new(0, 0), "");
        assert!(s.cells.is_empty());
    }

    #[test]
    fn test_sheet_col_x_offset() {
        let s = Sheet::new("Test");
        assert_eq!(s.col_x_offset(0), 0.0);
        assert_eq!(s.col_x_offset(1), DEFAULT_COL_WIDTH);
        assert_eq!(s.col_x_offset(2), DEFAULT_COL_WIDTH * 2.0);
    }

    #[test]
    fn test_sheet_row_y_offset() {
        let s = Sheet::new("Test");
        assert_eq!(s.row_y_offset(0), 0.0);
        assert_eq!(s.row_y_offset(1), DEFAULT_ROW_HEIGHT);
    }

    #[test]
    fn test_sheet_col_at_x() {
        let s = Sheet::new("Test");
        assert_eq!(s.col_at_x(0.0), 0);
        assert_eq!(s.col_at_x(DEFAULT_COL_WIDTH + 1.0), 1);
    }

    #[test]
    fn test_sheet_row_at_y() {
        let s = Sheet::new("Test");
        assert_eq!(s.row_at_y(0.0), 0);
        assert_eq!(s.row_at_y(DEFAULT_ROW_HEIGHT + 1.0), 1);
    }

    // -- CSV tests --

    #[test]
    fn test_csv_export_basic() {
        let mut s = Sheet::new("Test");
        s.set_cell_input(CellAddr::new(0, 0), "Name");
        s.set_cell_input(CellAddr::new(1, 0), "Value");
        s.set_cell_input(CellAddr::new(0, 1), "A");
        s.set_cell_input(CellAddr::new(1, 1), "42");
        let csv = s.export_csv();
        assert!(csv.contains("Name,Value"));
        assert!(csv.contains("A,42"));
    }

    #[test]
    fn test_csv_export_with_commas() {
        let mut s = Sheet::new("Test");
        s.set_cell_input(CellAddr::new(0, 0), "Hello, world");
        let csv = s.export_csv();
        assert!(csv.contains("\"Hello, world\""));
    }

    #[test]
    fn test_csv_import_basic() {
        let mut s = Sheet::new("Test");
        let csv = "Name,Value\nA,42\nB,99";
        s.import_csv(csv);
        assert_eq!(
            s.get_cell(CellAddr::new(0, 0)).value,
            CellValue::Text("Name".to_string())
        );
        assert_eq!(
            s.get_cell(CellAddr::new(1, 1)).value,
            CellValue::Number(42.0)
        );
    }

    /// A sheet must be able to read back what it wrote. Every value here is
    /// one the old line-oriented importer mangled or the old exporter failed
    /// to quote.
    #[test]
    fn a_csv_export_can_be_imported_back() {
        let hostile = [
            "plain",
            "has, comma",
            "has\nnewline",
            "has \"quotes\"",
            "has\rcarriage return",
        ];
        let mut sheet = Sheet::new("src");
        for (i, v) in hostile.iter().enumerate() {
            let _ = sheet.set_cell_input(CellAddr::new(i, 0), v);
        }

        let csv = sheet.export_csv();
        let mut back = Sheet::new("dst");
        let _ = back.import_csv(&csv);

        for (i, want) in hostile.iter().enumerate() {
            assert_eq!(
                back.get_cell(CellAddr::new(i, 0)).display_text(),
                *want,
                "cell {i} did not survive the round trip; csv was {csv:?}"
            );
        }
    }

    #[test]
    fn a_newline_in_a_cell_does_not_forge_a_row() {
        let mut sheet = Sheet::new("src");
        let _ = sheet.set_cell_input(CellAddr::new(0, 0), "one\ntwo");
        let _ = sheet.set_cell_input(CellAddr::new(1, 0), "x");
        let csv = sheet.export_csv();

        let mut back = Sheet::new("dst");
        let _ = back.import_csv(&csv);
        // The second cell must still be on row 0, not pushed onto a row the
        // newline invented.
        assert_eq!(back.get_cell(CellAddr::new(1, 0)).display_text(), "x");
    }

    // -- Formula tokenizer tests --

    #[test]
    fn test_tokenize_number() {
        let tokens = tokenize_formula("42").unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0], FormulaToken::Number(n) if (*n - 42.0).abs() < 1e-10));
    }

    #[test]
    fn test_tokenize_cell_ref() {
        let tokens = tokenize_formula("A1").unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(
            matches!(&tokens[0], FormulaToken::CellRef(addr) if addr.col == 0 && addr.row == 0)
        );
    }

    #[test]
    fn test_tokenize_range_ref() {
        let tokens = tokenize_formula("A1:C3").unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0], FormulaToken::RangeRef(_, _)));
    }

    #[test]
    fn test_tokenize_operators() {
        let tokens = tokenize_formula("1+2*3-4/5").unwrap();
        assert_eq!(tokens.len(), 9);
    }

    #[test]
    fn test_tokenize_string_literal() {
        let tokens = tokenize_formula("\"hello\"").unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0], FormulaToken::StringLiteral(s) if s == "hello"));
    }

    #[test]
    fn test_tokenize_boolean() {
        let tokens = tokenize_formula("TRUE").unwrap();
        assert_eq!(tokens[0], FormulaToken::Boolean(true));
    }

    #[test]
    fn test_tokenize_function() {
        let tokens = tokenize_formula("SUM(A1:A5)").unwrap();
        assert!(matches!(&tokens[0], FormulaToken::FuncName(n) if n == "SUM"));
    }

    #[test]
    fn test_tokenize_comparison_operators() {
        let tokens = tokenize_formula("A1<>B1").unwrap();
        assert!(tokens.iter().any(|t| matches!(t, FormulaToken::NotEquals)));
    }

    #[test]
    fn test_tokenize_less_eq() {
        let tokens = tokenize_formula("A1<=B1").unwrap();
        assert!(tokens.iter().any(|t| matches!(t, FormulaToken::LessEq)));
    }

    #[test]
    fn test_tokenize_greater_eq() {
        let tokens = tokenize_formula("A1>=B1").unwrap();
        assert!(tokens.iter().any(|t| matches!(t, FormulaToken::GreaterEq)));
    }

    // -- Formula evaluator tests --

    #[test]
    fn test_eval_simple_number() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=42", &sheet);
        assert_eq!(val, CellValue::Number(42.0));
    }

    #[test]
    fn test_eval_addition() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=1+2", &sheet);
        assert_eq!(val, CellValue::Number(3.0));
    }

    #[test]
    fn test_eval_multiplication() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=3*4", &sheet);
        assert_eq!(val, CellValue::Number(12.0));
    }

    #[test]
    fn test_eval_operator_precedence() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=2+3*4", &sheet);
        assert_eq!(val, CellValue::Number(14.0));
    }

    #[test]
    fn test_eval_parentheses() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=(2+3)*4", &sheet);
        assert_eq!(val, CellValue::Number(20.0));
    }

    #[test]
    fn test_eval_division() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=10/4", &sheet);
        assert_eq!(val, CellValue::Number(2.5));
    }

    #[test]
    fn test_eval_division_by_zero() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=1/0", &sheet);
        assert!(matches!(val, CellValue::Error(CellError::DivisionByZero)));
    }

    #[test]
    fn test_eval_unary_minus() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=-5", &sheet);
        assert_eq!(val, CellValue::Number(-5.0));
    }

    #[test]
    fn test_eval_cell_reference() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "10");
        let val = evaluate_formula("=A1", &sheet);
        assert_eq!(val, CellValue::Number(10.0));
    }

    #[test]
    fn test_eval_cell_ref_formula() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "5");
        sheet.set_cell_input(CellAddr::new(1, 0), "=A1*2");
        let val = evaluate_formula("=B1", &sheet);
        assert_eq!(val, CellValue::Number(10.0));
    }

    #[test]
    fn test_eval_sum_range() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "1");
        sheet.set_cell_input(CellAddr::new(0, 1), "2");
        sheet.set_cell_input(CellAddr::new(0, 2), "3");
        let val = evaluate_formula("=SUM(A1:A3)", &sheet);
        assert_eq!(val, CellValue::Number(6.0));
    }

    #[test]
    fn test_eval_avg() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "10");
        sheet.set_cell_input(CellAddr::new(0, 1), "20");
        sheet.set_cell_input(CellAddr::new(0, 2), "30");
        let val = evaluate_formula("=AVG(A1:A3)", &sheet);
        assert_eq!(val, CellValue::Number(20.0));
    }

    #[test]
    fn test_eval_min() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "5");
        sheet.set_cell_input(CellAddr::new(0, 1), "3");
        sheet.set_cell_input(CellAddr::new(0, 2), "9");
        let val = evaluate_formula("=MIN(A1:A3)", &sheet);
        assert_eq!(val, CellValue::Number(3.0));
    }

    #[test]
    fn test_eval_max() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "5");
        sheet.set_cell_input(CellAddr::new(0, 1), "3");
        sheet.set_cell_input(CellAddr::new(0, 2), "9");
        let val = evaluate_formula("=MAX(A1:A3)", &sheet);
        assert_eq!(val, CellValue::Number(9.0));
    }

    #[test]
    fn test_eval_count() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "5");
        sheet.set_cell_input(CellAddr::new(0, 1), "hello");
        sheet.set_cell_input(CellAddr::new(0, 2), "9");
        let val = evaluate_formula("=COUNT(A1:A3)", &sheet);
        assert_eq!(val, CellValue::Number(3.0));
    }

    #[test]
    fn test_eval_if_true() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=IF(TRUE,1,2)", &sheet);
        assert_eq!(val, CellValue::Number(1.0));
    }

    #[test]
    fn test_eval_if_false() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=IF(FALSE,1,2)", &sheet);
        assert_eq!(val, CellValue::Number(2.0));
    }

    #[test]
    fn test_eval_if_comparison() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "10");
        let val = evaluate_formula("=IF(A1>5,\"big\",\"small\")", &sheet);
        assert_eq!(val, CellValue::Text("big".to_string()));
    }

    #[test]
    fn test_eval_abs() {
        let sheet = Sheet::new("Test");
        assert_eq!(evaluate_formula("=ABS(-5)", &sheet), CellValue::Number(5.0));
        assert_eq!(evaluate_formula("=ABS(5)", &sheet), CellValue::Number(5.0));
    }

    #[test]
    fn test_eval_round() {
        // 3.55678 rounds to 3.56 — both literals avoid the clippy::approx_constant
        // PI proximity flag while still exercising ROUND().
        let sheet = Sheet::new("Test");
        assert_eq!(
            evaluate_formula("=ROUND(3.55678,2)", &sheet),
            CellValue::Number(3.56)
        );
    }

    #[test]
    fn test_eval_round_no_places() {
        let sheet = Sheet::new("Test");
        assert_eq!(
            evaluate_formula("=ROUND(3.7)", &sheet),
            CellValue::Number(4.0)
        );
    }

    #[test]
    fn test_eval_concatenate() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=CONCATENATE(\"hello\",\" \",\"world\")", &sheet);
        assert_eq!(val, CellValue::Text("hello world".to_string()));
    }

    #[test]
    fn test_eval_len() {
        let sheet = Sheet::new("Test");
        assert_eq!(
            evaluate_formula("=LEN(\"hello\")", &sheet),
            CellValue::Number(5.0)
        );
    }

    #[test]
    fn test_eval_upper() {
        let sheet = Sheet::new("Test");
        assert_eq!(
            evaluate_formula("=UPPER(\"hello\")", &sheet),
            CellValue::Text("HELLO".to_string())
        );
    }

    #[test]
    fn test_eval_lower() {
        let sheet = Sheet::new("Test");
        assert_eq!(
            evaluate_formula("=LOWER(\"HELLO\")", &sheet),
            CellValue::Text("hello".to_string())
        );
    }

    #[test]
    fn test_eval_string_literal() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=\"test\"", &sheet);
        assert_eq!(val, CellValue::Text("test".to_string()));
    }

    #[test]
    fn test_eval_invalid_formula() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=", &sheet);
        // Empty formula body
        assert!(matches!(val, CellValue::Empty));
    }

    /// A formula nested deeper than the evaluator will go reports an error
    /// rather than overflowing the stack.
    ///
    /// Before the depth guard covered the grammar, only *cell-reference*
    /// recursion was counted, so this recursed once per parenthesis until the
    /// process died — and a stack overflow is not something the caller can
    /// render in a cell: it takes the workbook down with it. 50,000 is far past
    /// any real formula and far past the depth that actually overflows.
    #[test]
    fn a_deeply_nested_formula_reports_an_error_instead_of_overflowing() {
        let sheet = Sheet::new("Test");
        let depth = 50_000;
        let formula = format!("={}1{}", "(".repeat(depth), ")".repeat(depth));
        assert_eq!(
            evaluate_formula(&formula, &sheet),
            CellValue::Error(CellError::TooDeep)
        );
    }

    /// The same, reached through function arguments rather than parentheses.
    #[test]
    fn deeply_nested_function_calls_report_an_error() {
        let sheet = Sheet::new("Test");
        let depth = 20_000;
        let formula = format!("={}1{}", "ABS(".repeat(depth), ")".repeat(depth));
        assert_eq!(
            evaluate_formula(&formula, &sheet),
            CellValue::Error(CellError::TooDeep)
        );
    }

    /// Nesting a person would actually write still evaluates.
    ///
    /// The guard is worth nothing if it fires on real formulas, so this pins
    /// the other side of it.
    #[test]
    fn ordinary_nesting_still_evaluates() {
        let sheet = Sheet::new("Test");
        let formula = format!("={}1+1{}", "(".repeat(20), ")".repeat(20));
        assert_eq!(evaluate_formula(&formula, &sheet), CellValue::Number(2.0));
    }

    /// A chain of cells that refer to one another still resolves, and a chain
    /// longer than the budget reports depth rather than a cycle.
    ///
    /// Cycles are detected exactly, by the path of addresses being visited, so
    /// a depth failure is never one — which is why it gets its own error.
    #[test]
    fn a_chain_of_referring_cells_resolves_until_the_budget_runs_out() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "1");
        // A1 = 1, A2 = A1 + 1, A3 = A2 + 1, ...
        for row in 1..MAX_ROWS {
            let previous = CellAddr::new(0, row.saturating_sub(1)).display();
            sheet.set_cell_input(CellAddr::new(0, row), &format!("={previous}+1"));
        }
        recalculate_sheet(&mut sheet);
        assert_eq!(
            sheet.get_cell(CellAddr::new(0, 9)).value,
            CellValue::Number(10.0),
            "a ten-deep chain is well inside the budget"
        );
        assert_eq!(
            sheet
                .get_cell(CellAddr::new(0, MAX_ROWS.saturating_sub(1)))
                .value,
            CellValue::Error(CellError::TooDeep),
            "a 999-deep chain is not, and says so without claiming a cycle"
        );
    }

    /// A real cycle still reports as one.
    #[test]
    fn a_two_cell_cycle_is_reported_as_a_cycle_not_as_depth() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "=A2");
        sheet.set_cell_input(CellAddr::new(0, 1), "=A1");
        recalculate_sheet(&mut sheet);
        assert_eq!(
            sheet.get_cell(CellAddr::new(0, 0)).value,
            CellValue::Error(CellError::CircularReference)
        );
    }

    #[test]
    fn test_eval_unknown_function() {
        let sheet = Sheet::new("Test");
        let val = evaluate_formula("=FOOBAR(1)", &sheet);
        assert!(matches!(val, CellValue::Error(CellError::NameError)));
    }

    #[test]
    fn test_eval_comparison_equal() {
        let sheet = Sheet::new("Test");
        assert_eq!(evaluate_formula("=1=1", &sheet), CellValue::Boolean(true));
        assert_eq!(evaluate_formula("=1=2", &sheet), CellValue::Boolean(false));
    }

    #[test]
    fn test_eval_comparison_not_equal() {
        let sheet = Sheet::new("Test");
        assert_eq!(evaluate_formula("=1<>2", &sheet), CellValue::Boolean(true));
    }

    #[test]
    fn test_eval_comparison_less() {
        let sheet = Sheet::new("Test");
        assert_eq!(evaluate_formula("=1<2", &sheet), CellValue::Boolean(true));
        assert_eq!(evaluate_formula("=2<1", &sheet), CellValue::Boolean(false));
    }

    #[test]
    fn test_eval_comparison_greater() {
        let sheet = Sheet::new("Test");
        assert_eq!(evaluate_formula("=5>3", &sheet), CellValue::Boolean(true));
    }

    // -- Recalculate tests --

    #[test]
    fn test_recalculate_formulas() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "10");
        sheet.set_cell_input(CellAddr::new(1, 0), "=A1*2");
        recalculate_sheet(&mut sheet);
        assert_eq!(
            sheet.get_cell(CellAddr::new(1, 0)).value,
            CellValue::Number(20.0)
        );
    }

    #[test]
    fn test_recalculate_chain() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "5");
        sheet.set_cell_input(CellAddr::new(1, 0), "=A1+1");
        sheet.set_cell_input(CellAddr::new(2, 0), "=B1+1");
        recalculate_sheet(&mut sheet);
        assert_eq!(
            sheet.get_cell(CellAddr::new(2, 0)).value,
            CellValue::Number(7.0)
        );
    }

    // -- Auto-fill tests --

    #[test]
    fn test_auto_fill_constant() {
        let vals = vec![CellValue::Number(5.0)];
        assert_eq!(auto_fill_next(&vals, 0), CellValue::Number(5.0));
    }

    #[test]
    fn test_auto_fill_arithmetic_series() {
        let vals = vec![
            CellValue::Number(1.0),
            CellValue::Number(2.0),
            CellValue::Number(3.0),
        ];
        assert_eq!(auto_fill_next(&vals, 0), CellValue::Number(4.0));
        assert_eq!(auto_fill_next(&vals, 1), CellValue::Number(5.0));
    }

    #[test]
    fn test_auto_fill_arithmetic_step2() {
        let vals = vec![CellValue::Number(2.0), CellValue::Number(4.0)];
        assert_eq!(auto_fill_next(&vals, 0), CellValue::Number(6.0));
    }

    #[test]
    fn test_auto_fill_text_repeat() {
        let vals = vec![
            CellValue::Text("a".to_string()),
            CellValue::Text("b".to_string()),
        ];
        assert_eq!(auto_fill_next(&vals, 0), CellValue::Text("a".to_string()));
        assert_eq!(auto_fill_next(&vals, 1), CellValue::Text("b".to_string()));
    }

    #[test]
    fn test_auto_fill_empty() {
        let vals: Vec<CellValue> = vec![];
        assert_eq!(auto_fill_next(&vals, 0), CellValue::Empty);
    }

    // -- Find and Replace tests --

    #[test]
    fn test_find_basic() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "hello");
        sheet.set_cell_input(CellAddr::new(1, 0), "world");
        sheet.set_cell_input(CellAddr::new(0, 1), "hello world");

        let mut fr = FindReplace::new();
        fr.search_text = "hello".to_string();
        fr.find_all(&sheet);
        assert_eq!(fr.result_count(), 2);
    }

    #[test]
    fn test_find_case_insensitive() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "Hello");
        sheet.set_cell_input(CellAddr::new(1, 0), "HELLO");

        let mut fr = FindReplace::new();
        fr.search_text = "hello".to_string();
        fr.case_sensitive = false;
        fr.find_all(&sheet);
        assert_eq!(fr.result_count(), 2);
    }

    #[test]
    fn test_find_case_sensitive() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "Hello");
        sheet.set_cell_input(CellAddr::new(1, 0), "HELLO");

        let mut fr = FindReplace::new();
        fr.search_text = "Hello".to_string();
        fr.case_sensitive = true;
        fr.find_all(&sheet);
        assert_eq!(fr.result_count(), 1);
    }

    #[test]
    fn test_find_next_wraps() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "a");
        sheet.set_cell_input(CellAddr::new(1, 0), "a");

        let mut fr = FindReplace::new();
        fr.search_text = "a".to_string();
        fr.find_all(&sheet);
        let first = fr.next_result();
        let second = fr.next_result();
        assert!(first.is_some());
        assert!(second.is_some());
        assert_ne!(first, second);
    }

    #[test]
    fn test_replace_all() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "foo");
        sheet.set_cell_input(CellAddr::new(1, 0), "foobar");

        let mut fr = FindReplace::new();
        fr.search_text = "foo".to_string();
        fr.replace_text = "baz".to_string();
        fr.find_all(&sheet);
        let changes = fr.replace_all(&mut sheet);
        assert_eq!(changes.len(), 2);
        assert_eq!(sheet.get_cell(CellAddr::new(0, 0)).display_text(), "baz");
        assert_eq!(sheet.get_cell(CellAddr::new(1, 0)).display_text(), "bazbar");
    }

    // -- case_insensitive_replace tests --

    #[test]
    fn test_case_insensitive_replace() {
        assert_eq!(
            case_insensitive_replace("Hello World", "hello", "hi"),
            "hi World"
        );
    }

    /// Replacement slices the cell text, not a lowercased copy of it.
    ///
    /// Turkish `İ` (U+0130) is two bytes and folds to three, so the offsets
    /// from the folded copy pointed one byte past where they should — inside
    /// the following character in the real text, which panics in `&text[..]`.
    #[test]
    fn a_replacement_slices_the_text_it_was_given() {
        assert_eq!(
            case_insensitive_replace("\u{130}abc\u{130}", "ABC", "-"),
            "\u{130}-\u{130}"
        );
    }

    /// A needle that grows when folded is matched and skipped whole, rather
    /// than resumed inside.
    #[test]
    fn a_needle_that_grows_when_folded_is_skipped_whole() {
        assert_eq!(
            case_insensitive_replace("x\u{130}y", "i\u{307}", "I"),
            "xIy"
        );
    }

    /// Replacements do not overlap: `aa` occurs twice in `aaaa`.
    #[test]
    fn replacements_do_not_overlap() {
        assert_eq!(case_insensitive_replace("aaaa", "aa", "b"), "bb");
    }

    /// An empty needle replaces nothing. It used to match at every position
    /// without advancing, so the loop appended the replacement until the
    /// process ran out of memory.
    #[test]
    fn an_empty_needle_replaces_nothing() {
        assert_eq!(case_insensitive_replace("abc", "", "X"), "abc");
    }

    // -- Selection tests --

    #[test]
    fn test_selection_single() {
        let sel = Selection::single(CellAddr::new(1, 2));
        assert_eq!(sel.active, CellAddr::new(1, 2));
        assert!(sel.contains(CellAddr::new(1, 2)));
        assert!(!sel.contains(CellAddr::new(0, 0)));
    }

    #[test]
    fn test_selection_primary_range() {
        let sel = Selection::single(CellAddr::new(3, 4));
        let r = sel.primary_range();
        assert_eq!(r.start(), CellAddr::new(3, 4));
        assert_eq!(r.end(), CellAddr::new(3, 4));
    }

    // -- UndoManager tests --

    #[test]
    fn test_undo_manager_initially_empty() {
        let um = UndoManager::new();
        assert!(!um.can_undo());
        assert!(!um.can_redo());
    }

    #[test]
    fn test_undo_manager_push_and_undo() {
        let mut um = UndoManager::new();
        um.push_action(UndoAction::CellEdit {
            sheet_idx: 0,
            addr: CellAddr::new(0, 0),
            old_cell: Cell::empty(),
            new_cell: Cell::empty(),
        });
        assert!(um.can_undo());
        assert!(!um.can_redo());
        um.pop_undo();
        assert!(!um.can_undo());
        assert!(um.can_redo());
    }

    #[test]
    fn test_undo_manager_redo() {
        let mut um = UndoManager::new();
        um.push_action(UndoAction::CellEdit {
            sheet_idx: 0,
            addr: CellAddr::new(0, 0),
            old_cell: Cell::empty(),
            new_cell: Cell::empty(),
        });
        um.pop_undo();
        um.pop_redo();
        assert!(um.can_undo());
        assert!(!um.can_redo());
    }

    #[test]
    fn test_undo_manager_push_clears_redo() {
        let mut um = UndoManager::new();
        um.push_action(UndoAction::CellEdit {
            sheet_idx: 0,
            addr: CellAddr::new(0, 0),
            old_cell: Cell::empty(),
            new_cell: Cell::empty(),
        });
        um.pop_undo();
        assert!(um.can_redo());
        um.push_action(UndoAction::CellEdit {
            sheet_idx: 0,
            addr: CellAddr::new(1, 1),
            old_cell: Cell::empty(),
            new_cell: Cell::empty(),
        });
        assert!(!um.can_redo());
    }

    #[test]
    fn test_undo_manager_limit() {
        let mut um = UndoManager::new();
        for i in 0..UNDO_STACK_LIMIT + 50 {
            um.push_action(UndoAction::CellEdit {
                sheet_idx: 0,
                addr: CellAddr::new(i % 26, 0),
                old_cell: Cell::empty(),
                new_cell: Cell::empty(),
            });
        }
        assert_eq!(um.undo_count(), UNDO_STACK_LIMIT);
    }

    // -- SpreadsheetApp tests --

    #[test]
    fn test_app_new() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        assert_eq!(app.sheets.len(), 1);
        assert_eq!(app.sheets.active_index(), 0);
    }

    #[test]
    fn test_app_set_cell_input() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "42");
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(0, 0)).value,
            CellValue::Number(42.0)
        );
    }

    #[test]
    fn test_app_undo_redo() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "hello");
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(0, 0)).value,
            CellValue::Text("hello".to_string())
        );
        app.undo();
        assert!(
            app.active_sheet()
                .get_cell(CellAddr::new(0, 0))
                .value
                .is_empty()
        );
        app.redo();
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(0, 0)).value,
            CellValue::Text("hello".to_string())
        );
    }

    #[test]
    fn test_app_copy_paste() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "source");
        *app.selection_mut() = Selection::single(CellAddr::new(0, 0));
        app.copy_selection();
        *app.selection_mut() = Selection::single(CellAddr::new(1, 1));
        app.paste();
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(1, 1)).value,
            CellValue::Text("source".to_string())
        );
    }

    #[test]
    fn test_app_cut_paste() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "moveme");
        *app.selection_mut() = Selection::single(CellAddr::new(0, 0));
        app.cut_selection();
        assert!(
            app.active_sheet()
                .get_cell(CellAddr::new(0, 0))
                .value
                .is_empty()
        );
        *app.selection_mut() = Selection::single(CellAddr::new(2, 2));
        app.paste();
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(2, 2)).value,
            CellValue::Text("moveme".to_string())
        );
    }

    #[test]
    fn test_app_delete_selection() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "delete me");
        *app.selection_mut() = Selection::single(CellAddr::new(0, 0));
        app.delete_selection();
        assert!(
            app.active_sheet()
                .get_cell(CellAddr::new(0, 0))
                .value
                .is_empty()
        );
    }

    #[test]
    fn test_app_add_sheet() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.add_sheet();
        assert_eq!(app.sheets.len(), 2);
        assert_eq!(app.sheets.active_index(), 1);
    }

    #[test]
    fn test_app_remove_sheet() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.add_sheet();
        app.remove_active_sheet();
        assert_eq!(app.sheets.len(), 1);
    }

    #[test]
    fn test_app_remove_last_sheet_prevented() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.remove_active_sheet();
        assert_eq!(app.sheets.len(), 1);
    }

    #[test]
    fn redoing_an_added_sheet_brings_it_back() {
        // Both sheet actions used to implement only their undo half, so this
        // sequence lost the sheet: redo was a no-op and the user had no way to
        // get it back short of adding a fresh one.
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.add_sheet();
        assert_eq!(app.sheets.len(), 2);

        app.undo();
        assert_eq!(app.sheets.len(), 1, "undo must remove the added sheet");

        app.redo();
        assert_eq!(app.sheets.len(), 2, "redo must put the added sheet back");
        assert_eq!(
            app.sheets.get(1).map(|s| s.name.clone()).as_deref(),
            Some("Sheet2")
        );
    }

    #[test]
    fn redoing_a_removed_sheet_removes_it_again() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.add_sheet();
        app.remove_active_sheet();
        assert_eq!(app.sheets.len(), 1);

        app.undo();
        assert_eq!(app.sheets.len(), 2, "undo must restore the removed sheet");

        app.redo();
        assert_eq!(app.sheets.len(), 1, "redo must remove it again");
    }

    #[test]
    fn a_workbook_always_has_at_least_one_sheet() {
        // The rule used to live in `remove_active_sheet` only. The undo path
        // removed a sheet with nothing but a `sheet_idx < len` test, so it was
        // one code path away from a workbook with no sheets at all -- which
        // `active_sheet()` would then have answered by indexing `sheets[0]`,
        // in a branch commented "this should never happen".
        let mut book = SheetBook::new(Sheet::new("Only"));
        assert_eq!(book.len(), 1);
        assert!(
            book.remove(0).is_none(),
            "the last sheet may not be removed"
        );
        assert_eq!(book.len(), 1);
        assert_eq!(book.active().name, "Only");

        // And no sequence of removals can get past that floor.
        book.push(Sheet::new("Second"));
        book.push(Sheet::new("Third"));
        for index in [2, 1, 0, 0, 0, 5] {
            book.remove(index);
            // Not `len() >= 1`: `SheetBook::is_empty` is a constant `false`, so
            // the usual rewrite of that comparison would assert nothing at all.
            // Asking for sheet 0 is the same claim made in a form that can fail.
            assert!(book.get(0).is_some(), "a workbook always has a sheet 0");
            assert!(book.active_index() < book.len());
        }
        assert_eq!(book.len(), 1);
    }

    #[test]
    fn removing_the_first_sheet_promotes_the_next_one() {
        // Sheet 0 lives in its own field, so removing it is the one case that
        // is not a `Vec::remove` -- worth pinning separately.
        let mut book = SheetBook::new(Sheet::new("First"));
        book.push(Sheet::new("Second"));
        book.push(Sheet::new("Third"));

        assert_eq!(book.remove(0).map(|s| s.name), Some("First".to_owned()));
        assert_eq!(book.len(), 2);
        assert_eq!(book.get(0).map(|s| s.name.as_str()), Some("Second"));
        assert_eq!(book.get(1).map(|s| s.name.as_str()), Some("Third"));
        assert_eq!(book.get(2).map(|s| s.name.as_str()), None);

        // Inserting at 0 must displace the head rather than overwrite it.
        book.insert(0, Sheet::new("Zeroth"));
        let names: Vec<&str> = book.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Zeroth", "Second", "Third"]);
        assert_eq!(book.active_index(), 0);
    }

    #[test]
    fn test_app_toggle_bold() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "text");
        *app.selection_mut() = Selection::single(CellAddr::new(0, 0));
        assert!(!app.active_sheet().get_cell(CellAddr::new(0, 0)).format.bold);
        app.toggle_bold();
        assert!(app.active_sheet().get_cell(CellAddr::new(0, 0)).format.bold);
        app.toggle_bold();
        assert!(!app.active_sheet().get_cell(CellAddr::new(0, 0)).format.bold);
    }

    #[test]
    fn test_app_toggle_italic() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "text");
        *app.selection_mut() = Selection::single(CellAddr::new(0, 0));
        app.toggle_italic();
        assert!(
            app.active_sheet()
                .get_cell(CellAddr::new(0, 0))
                .format
                .italic
        );
    }

    #[test]
    fn test_app_set_alignment() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "text");
        *app.selection_mut() = Selection::single(CellAddr::new(0, 0));
        app.set_alignment(Alignment::Center);
        assert_eq!(
            app.active_sheet()
                .get_cell(CellAddr::new(0, 0))
                .format
                .alignment,
            Alignment::Center
        );
    }

    #[test]
    fn test_app_set_number_format() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "0.5");
        *app.selection_mut() = Selection::single(CellAddr::new(0, 0));
        app.set_number_format(NumberFormat::Percentage(0));
        let cell = app.active_sheet().get_cell(CellAddr::new(0, 0));
        assert_eq!(cell.format.number_format, NumberFormat::Percentage(0));
    }

    #[test]
    fn test_app_toggle_borders() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "text");
        *app.selection_mut() = Selection::single(CellAddr::new(0, 0));
        app.toggle_borders();
        assert!(
            app.active_sheet()
                .get_cell(CellAddr::new(0, 0))
                .format
                .borders
                .has_any()
        );
        app.toggle_borders();
        assert!(
            !app.active_sheet()
                .get_cell(CellAddr::new(0, 0))
                .format
                .borders
                .has_any()
        );
    }

    #[test]
    fn test_app_freeze_panes() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        *app.selection_mut() = Selection::single(CellAddr::new(2, 3));
        app.toggle_freeze_panes();
        assert_eq!(app.active_sheet().frozen_cols, 2);
        assert_eq!(app.active_sheet().frozen_rows, 3);
        app.toggle_freeze_panes();
        assert_eq!(app.active_sheet().frozen_cols, 0);
        assert_eq!(app.active_sheet().frozen_rows, 0);
    }

    #[test]
    fn test_app_navigate() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.navigate(1, 0);
        assert_eq!(app.selection().active, CellAddr::new(1, 0));
        app.navigate(0, 1);
        assert_eq!(app.selection().active, CellAddr::new(1, 1));
        app.navigate(-1, -1);
        assert_eq!(app.selection().active, CellAddr::new(0, 0));
    }

    #[test]
    fn test_app_navigate_clamp() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.navigate(-10, -10);
        assert_eq!(app.selection().active, CellAddr::new(0, 0));
    }

    #[test]
    fn test_app_begin_editing() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "hello");
        *app.selection_mut() = Selection::single(CellAddr::new(0, 0));
        app.begin_editing();
        assert!(matches!(app.mode, InteractionMode::Editing { .. }));
    }

    #[test]
    fn test_app_confirm_edit() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.mode = InteractionMode::Editing {
            buffer: EditBuffer::at_end("99".to_string()),
        };
        *app.selection_mut() = Selection::single(CellAddr::new(0, 0));
        app.confirm_edit();
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(0, 0)).value,
            CellValue::Number(99.0)
        );
        assert!(matches!(app.mode, InteractionMode::Normal));
    }

    #[test]
    fn test_app_cancel_edit() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.mode = InteractionMode::Editing {
            buffer: EditBuffer::at_end("99".to_string()),
        };
        app.cancel_edit();
        assert!(matches!(app.mode, InteractionMode::Normal));
    }

    #[test]
    fn test_app_status_bar_text_empty() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        assert!(app.status_bar_text().is_empty());
    }

    #[test]
    fn test_app_status_bar_text_with_data() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "10");
        app.set_cell_input(CellAddr::new(0, 1), "20");
        *app.selection_mut() = Selection {
            active: CellAddr::new(0, 0),
            ranges: vec![CellRange::new(CellAddr::new(0, 0), CellAddr::new(0, 1))],
        };
        let text = app.status_bar_text();
        assert!(text.contains("SUM: 30.00"));
        assert!(text.contains("AVG: 15.00"));
        assert!(text.contains("COUNT: 2"));
    }

    #[test]
    fn test_app_grid_top() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let expected = TOOLBAR_HEIGHT + FORMULA_BAR_HEIGHT + COL_HEADER_HEIGHT;
        assert_eq!(app.grid_top(), expected);
    }

    #[test]
    fn test_app_grid_height() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let h = app.grid_height();
        assert!(h > 0.0);
    }

    #[test]
    fn test_app_grid_width() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let w = app.grid_width();
        assert!(w > 0.0);
        assert_eq!(w, 1280.0 - ROW_HEADER_WIDTH - SCROLLBAR_WIDTH);
    }

    // -- Rendering tests --

    #[test]
    fn test_render_produces_commands() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_has_background_fill() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let cmds = app.render();
        let has_bg = cmds.iter().any(|c| {
            matches!(c, RenderCommand::FillRect { width, height, .. } if *width == 1280.0 && *height == 800.0)
        });
        assert!(has_bg);
    }

    #[test]
    fn test_render_has_text_commands() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "Hello");
        let cmds = app.render();
        let has_text = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Hello"));
        assert!(has_text);
    }

    #[test]
    fn test_render_active_cell_outline() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let cmds = app.render();
        let has_stroke = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == COLOR_BLUE));
        assert!(has_stroke);
    }

    #[test]
    fn test_render_find_replace_overlay() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.find_replace.active = true;
        let cmds = app.render();
        let has_overlay = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Find and Replace"));
        assert!(has_overlay);
    }

    #[test]
    fn test_render_sheet_tabs() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.add_sheet();
        let cmds = app.render();
        let tab1 = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Sheet1"));
        let tab2 = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Sheet2"));
        assert!(tab1);
        assert!(tab2);
    }

    #[test]
    fn test_render_formula_bar() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let cmds = app.render();
        let has_fx = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "fx"));
        assert!(has_fx);
    }

    #[test]
    fn test_render_col_headers() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let cmds = app.render();
        let has_a = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "A"));
        assert!(has_a);
    }

    #[test]
    fn test_render_row_headers() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let cmds = app.render();
        let has_1 = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "1"));
        assert!(has_1);
    }

    // -- handle_editing_key tests --

    /// A buffer holding `text` with the caret `caret` characters from the start.
    ///
    /// Built by walking the caret rather than by setting a field, because the
    /// field is private on purpose: a test that could place the caret at an
    /// arbitrary number would be able to construct the very state the type
    /// exists to rule out, and would then be testing something the program
    /// cannot reach.
    fn editing(text: &str, caret: usize) -> EditBuffer {
        let mut buffer = EditBuffer::at_end(text.to_owned());
        buffer.move_home();
        for _ in 0..caret {
            buffer.move_right();
        }
        buffer
    }

    fn key(key: Key, text: Option<char>) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text,
        }
    }

    #[test]
    fn test_editing_backspace() {
        let mut buffer = editing("abc", 3);
        handle_editing_key(&mut buffer, &key(Key::Backspace, None));
        assert_eq!(buffer.text(), "ab");
        assert_eq!(buffer.caret(), 2);
    }

    #[test]
    fn test_editing_delete() {
        let mut buffer = editing("abc", 0);
        handle_editing_key(&mut buffer, &key(Key::Delete, None));
        assert_eq!(buffer.text(), "bc");
        assert_eq!(buffer.caret(), 0);
    }

    #[test]
    fn test_editing_type_char() {
        let mut buffer = editing("ab", 2);
        handle_editing_key(&mut buffer, &key(Key::C, Some('c')));
        assert_eq!(buffer.text(), "abc");
        assert_eq!(buffer.caret(), 3);
    }

    #[test]
    fn test_editing_left_arrow() {
        let mut buffer = editing("abc", 2);
        handle_editing_key(&mut buffer, &key(Key::Left, None));
        assert_eq!(buffer.caret(), 1);
    }

    #[test]
    fn test_editing_right_arrow() {
        let mut buffer = editing("abc", 1);
        handle_editing_key(&mut buffer, &key(Key::Right, None));
        assert_eq!(buffer.caret(), 2);
    }

    #[test]
    fn test_editing_home() {
        let mut buffer = editing("abc", 2);
        handle_editing_key(&mut buffer, &key(Key::Home, None));
        assert_eq!(buffer.caret(), 0);
    }

    #[test]
    fn test_editing_end() {
        let mut buffer = editing("abc", 0);
        handle_editing_key(&mut buffer, &key(Key::End, None));
        assert_eq!(buffer.caret(), 3);
    }

    /// The exact keystrokes that used to abort the process.
    ///
    /// `String::remove` panics when handed a byte offset that is not on a
    /// character boundary. With the caret counted in characters and the removal
    /// done in bytes, `Home`, `Right`, `Delete` on a cell beginning with a
    /// two-byte character handed it byte 1, which is inside that character.
    #[test]
    fn deleting_after_a_multi_byte_character_does_not_panic() {
        // "\u{e9}a" -- e-acute is two bytes in UTF-8, so character 1 is byte 2.
        let mut buffer = editing("\u{e9}a", 0);
        handle_editing_key(&mut buffer, &key(Key::Right, None));
        handle_editing_key(&mut buffer, &key(Key::Delete, None));
        assert_eq!(buffer.text(), "\u{e9}");
        assert_eq!(buffer.caret(), 1);
    }

    /// The same defect reached through `Backspace` rather than `Delete`.
    #[test]
    fn backspacing_over_a_multi_byte_character_does_not_panic() {
        let mut buffer = EditBuffer::at_end("\u{e9}a".to_owned());
        handle_editing_key(&mut buffer, &key(Key::Backspace, None));
        assert_eq!(buffer.text(), "\u{e9}");
        handle_editing_key(&mut buffer, &key(Key::Backspace, None));
        assert_eq!(buffer.text(), "");
        assert_eq!(buffer.caret(), 0);
    }

    /// Typing before a multi-byte character inserts before it, not inside it.
    ///
    /// The caret is a character count, so `Home` puts it at character 0 whatever
    /// the first character's width; the byte offset is derived from it in one
    /// place rather than assumed to be the same number.
    #[test]
    fn typing_at_the_start_of_multi_byte_text_inserts_before_it() {
        let mut buffer = editing("\u{4e2d}\u{6587}", 0);
        handle_editing_key(&mut buffer, &key(Key::X, Some('x')));
        assert_eq!(buffer.text(), "x\u{4e2d}\u{6587}");
        assert_eq!(buffer.caret(), 1);
    }

    /// `End` lands after the last character, not after the last byte.
    ///
    /// The two entry points into editing used to disagree about this: one seeded
    /// the caret with `text.len()`, which is a byte count, and the other with
    /// `1` after a single character was typed.
    #[test]
    fn end_counts_characters_not_bytes() {
        let mut buffer = editing("\u{4e2d}\u{6587}", 0);
        handle_editing_key(&mut buffer, &key(Key::End, None));
        assert_eq!(buffer.caret(), 2, "two characters, six bytes");
        handle_editing_key(&mut buffer, &key(Key::Right, None));
        assert_eq!(buffer.caret(), 2, "the caret must not run past the text");
    }

    // -- values_equal tests --

    #[test]
    fn test_values_equal_numbers() {
        assert!(values_equal(
            &CellValue::Number(1.0),
            &CellValue::Number(1.0)
        ));
        assert!(!values_equal(
            &CellValue::Number(1.0),
            &CellValue::Number(2.0)
        ));
    }

    #[test]
    fn test_values_equal_text_case_insensitive() {
        assert!(values_equal(
            &CellValue::Text("hello".to_string()),
            &CellValue::Text("HELLO".to_string()),
        ));
    }

    #[test]
    fn test_values_equal_empty() {
        assert!(values_equal(&CellValue::Empty, &CellValue::Empty));
    }

    #[test]
    fn test_values_equal_different_types() {
        assert!(!values_equal(
            &CellValue::Number(1.0),
            &CellValue::Text("1".to_string())
        ));
    }

    // -- compare_values tests --

    #[test]
    fn test_compare_values_numbers() {
        let r = compare_values(&CellValue::Number(1.0), &CellValue::Number(2.0)).unwrap();
        assert!(r < 0);
    }

    #[test]
    fn test_compare_values_equal() {
        let r = compare_values(&CellValue::Number(5.0), &CellValue::Number(5.0)).unwrap();
        assert_eq!(r, 0);
    }

    // -- value_to_string tests --

    #[test]
    fn test_value_to_string_empty() {
        assert_eq!(value_to_string(&CellValue::Empty), "");
    }

    #[test]
    fn test_value_to_string_number_int() {
        assert_eq!(value_to_string(&CellValue::Number(42.0)), "42");
    }

    #[test]
    fn test_value_to_string_number_float() {
        // 3.25 — exactly representable, dodges clippy::approx_constant.
        assert_eq!(value_to_string(&CellValue::Number(3.25)), "3.25");
    }

    #[test]
    fn test_value_to_string_boolean() {
        assert_eq!(value_to_string(&CellValue::Boolean(true)), "TRUE");
        assert_eq!(value_to_string(&CellValue::Boolean(false)), "FALSE");
    }

    #[test]
    fn test_value_to_string_error() {
        assert_eq!(
            value_to_string(&CellValue::Error(CellError::DivisionByZero)),
            "#DIV/0!"
        );
    }

    // -- ScrollPosition tests --

    #[test]
    fn test_scroll_clamp() {
        let mut s = ScrollPosition { x: -10.0, y: 500.0 };
        s.clamp(100.0, 200.0);
        assert_eq!(s.x, 0.0);
        assert_eq!(s.y, 200.0);
    }

    #[test]
    fn test_scroll_new() {
        let s = ScrollPosition::new();
        assert_eq!(s.x, 0.0);
        assert_eq!(s.y, 0.0);
    }

    // -- CellError display tests --

    #[test]
    fn test_cell_error_display() {
        assert_eq!(CellError::DivisionByZero.display(), "#DIV/0!");
        assert_eq!(CellError::InvalidReference.display(), "#REF!");
        assert_eq!(CellError::InvalidFormula.display(), "#ERROR!");
        assert_eq!(CellError::CircularReference.display(), "#CIRC!");
        assert_eq!(CellError::ValueError.display(), "#VALUE!");
        assert_eq!(CellError::NameError.display(), "#NAME?");
    }

    // -- require_number tests --

    #[test]
    fn test_require_number_ok() {
        assert_eq!(require_number(&CellValue::Number(5.0)).unwrap(), 5.0);
    }

    #[test]
    fn test_require_number_err() {
        assert!(require_number(&CellValue::Text("abc".to_string())).is_err());
    }

    #[test]
    fn test_require_number_boolean() {
        assert_eq!(require_number(&CellValue::Boolean(true)).unwrap(), 1.0);
    }

    // -- Resize event tests --

    #[test]
    fn test_handle_resize() {
        let mut app = SpreadsheetApp::new(800.0, 600.0);
        app.handle_resize(1920, 1080);
        assert_eq!(app.window_width, 1920.0);
        assert_eq!(app.window_height, 1080.0);
    }

    // -- Sort tests --

    #[test]
    fn test_sort_column_ascending() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "3");
        app.set_cell_input(CellAddr::new(0, 1), "1");
        app.set_cell_input(CellAddr::new(0, 2), "2");
        *app.selection_mut() = Selection {
            active: CellAddr::new(0, 0),
            ranges: vec![CellRange::new(CellAddr::new(0, 0), CellAddr::new(0, 2))],
        };
        app.sort_column(SortDirection::Ascending);
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(0, 0)).value,
            CellValue::Number(1.0)
        );
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(0, 1)).value,
            CellValue::Number(2.0)
        );
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(0, 2)).value,
            CellValue::Number(3.0)
        );
    }

    // -- Integration: formula with cell references after recalc --

    #[test]
    fn test_integration_sum_formula() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "10");
        app.set_cell_input(CellAddr::new(0, 1), "20");
        app.set_cell_input(CellAddr::new(0, 2), "30");
        app.set_cell_input(CellAddr::new(0, 3), "=SUM(A1:A3)");
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(0, 3)).value,
            CellValue::Number(60.0)
        );
    }

    #[test]
    fn test_integration_product_formula() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "5");
        app.set_cell_input(CellAddr::new(1, 0), "10");
        app.set_cell_input(CellAddr::new(2, 0), "=A1*B1");
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(2, 0)).value,
            CellValue::Number(50.0)
        );
    }

    #[test]
    fn test_integration_nested_formula() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "100");
        app.set_cell_input(CellAddr::new(1, 0), "=A1/2");
        app.set_cell_input(CellAddr::new(2, 0), "=B1+10");
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(2, 0)).value,
            CellValue::Number(60.0)
        );
    }

    #[test]
    fn test_integration_if_with_sum() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "10");
        app.set_cell_input(CellAddr::new(0, 1), "20");
        app.set_cell_input(CellAddr::new(0, 2), "=IF(SUM(A1:A2)>25,\"high\",\"low\")");
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(0, 2)).value,
            CellValue::Text("high".to_string())
        );
    }

    #[test]
    fn test_integration_render_with_data() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        for i in 0..10 {
            app.set_cell_input(CellAddr::new(0, i), &format!("Row {}", i + 1));
            app.set_cell_input(CellAddr::new(1, i), &format!("{}", (i + 1) * 10));
        }
        app.set_cell_input(CellAddr::new(1, 10), "=SUM(B1:B10)");
        let cmds = app.render();
        // Should have many render commands for a populated spreadsheet
        assert!(cmds.len() > 100);
    }

    #[test]
    fn test_integration_auto_fill_numbers() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "1");
        app.set_cell_input(CellAddr::new(0, 1), "2");
        app.set_cell_input(CellAddr::new(0, 2), "3");
        let source = CellRange::new(CellAddr::new(0, 0), CellAddr::new(0, 2));
        let end = CellAddr::new(0, 5);
        app.auto_fill(source, end);
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(0, 3)).value,
            CellValue::Number(4.0)
        );
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(0, 4)).value,
            CellValue::Number(5.0)
        );
        assert_eq!(
            app.active_sheet().get_cell(CellAddr::new(0, 5)).value,
            CellValue::Number(6.0)
        );
    }

    #[test]
    fn test_ensure_cell_visible_scrolls_right() {
        let mut app = SpreadsheetApp::new(800.0, 600.0);
        *app.selection_mut() = Selection::single(CellAddr::new(20, 0));
        app.ensure_cell_visible(CellAddr::new(20, 0));
        assert!(app.scroll().x > 0.0);
    }

    #[test]
    fn test_ensure_cell_visible_scrolls_down() {
        let mut app = SpreadsheetApp::new(800.0, 600.0);
        *app.selection_mut() = Selection::single(CellAddr::new(0, 100));
        app.ensure_cell_visible(CellAddr::new(0, 100));
        assert!(app.scroll().y > 0.0);
    }

    #[test]
    fn test_cell_at_position() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let grid_top = app.grid_top();
        let hit = app.cell_at_position(ROW_HEADER_WIDTH + 10.0, grid_top + 10.0);
        assert_eq!(hit, Some((0, 0)));
    }

    // -- the layout law, and the frozen panes it now serves --------------
    //
    // Frozen panes were settable and undriven before this suite existed: the
    // renderer, the hit test and `ensure_cell_visible` each had their own idea
    // of where a pinned cell was, and none of the three was tested. These
    // assert the property that makes the four-quadrant grid correct — the
    // renderer, the hit test and the scroller agree on where a cell is — rather
    // than the pixel values, which are free to change.

    /// A sheet with `cols`/`rows` pinned and the view scrolled well past them.
    ///
    /// The offset is deliberately *not* a whole number of cells. An aligned one
    /// puts every band boundary on a cell edge, which is the one case where an
    /// off-by-one in the first/last visible index does not show — and it makes
    /// a scrolled column land on exactly the same x as a pinned one, so a test
    /// asking "which was drawn here" cannot tell the two apart.
    fn frozen_app(cols: usize, rows: usize) -> SpreadsheetApp {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        {
            let sheet = app.active_sheet_mut();
            sheet.frozen_cols = cols;
            sheet.frozen_rows = rows;
        }
        app.scroll_by(530.0, 310.0);
        app
    }

    #[test]
    fn a_frozen_column_does_not_move_when_the_sheet_scrolls() {
        let mut app = frozen_app(2, 0);
        let pinned = app.col_screen_x(1);
        let scrolled = app.col_screen_x(9);
        app.scroll_by(120.0, 0.0);
        assert_eq!(app.col_screen_x(1), pinned, "the pinned column moved");
        assert!(
            app.col_screen_x(9) < scrolled,
            "the scrolling column did not move"
        );
    }

    #[test]
    fn a_frozen_row_does_not_move_when_the_sheet_scrolls() {
        let mut app = frozen_app(0, 3);
        let pinned = app.row_screen_y(2);
        let scrolled = app.row_screen_y(40);
        app.scroll_by(0.0, 96.0);
        assert_eq!(app.row_screen_y(2), pinned, "the pinned row moved");
        assert!(
            app.row_screen_y(40) < scrolled,
            "the scrolling row did not move"
        );
    }

    /// The property the whole restructure exists for: whatever the renderer
    /// drew at a point, the hit test finds at that point. Before it, the two
    /// disagreed by exactly the scroll offset — so the error grew as you
    /// scrolled, and clicking a cell selected a different one further left.
    #[test]
    fn clicking_where_a_cell_was_drawn_selects_that_cell() {
        let app = frozen_app(2, 3);
        for (col, row) in [(0, 0), (1, 2), (0, 40), (9, 1), (12, 45), (20, 60)] {
            let x = app.col_screen_x(col) + 2.0;
            let y = app.row_screen_y(row) + 2.0;
            // Only assert about cells the pane can actually show; one scrolled
            // off the top has a screen position, but nothing is drawn there.
            if x < ROW_HEADER_WIDTH
                || x >= app.grid_right()
                || y < app.grid_top()
                || y >= app.grid_bottom()
            {
                continue;
            }
            assert_eq!(
                app.cell_at_position(x, y),
                Some((col, row)),
                "the hit test disagrees with the renderer about ({col}, {row})"
            );
        }
    }

    #[test]
    fn the_hit_test_misses_outside_the_grid() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let inside = (ROW_HEADER_WIDTH + 10.0, app.grid_top() + 10.0);
        assert!(app.cell_at_position(inside.0, inside.1).is_some());
        // Each of these used to saturate into cell (0, 0) — a click on the row
        // header selected A1, and so did one below the last row.
        assert_eq!(app.cell_at_position(ROW_HEADER_WIDTH - 1.0, inside.1), None);
        assert_eq!(app.cell_at_position(inside.0, app.grid_top() - 1.0), None);
        assert_eq!(app.cell_at_position(app.grid_right(), inside.1), None);
        assert_eq!(app.cell_at_position(inside.0, app.grid_bottom()), None);
        assert_eq!(app.cell_at_position(f32::NAN, inside.1), None);
        assert_eq!(app.cell_at_position(inside.0, f32::INFINITY), None);
    }

    /// A drag is allowed to leave the grid, so it clamps instead of missing.
    #[test]
    fn a_drag_outside_the_grid_lands_on_the_nearest_cell() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        assert!(app.cell_nearest_position(-500.0, -500.0).is_some());
        assert!(app.cell_nearest_position(9999.0, 9999.0).is_some());
        assert_eq!(app.cell_nearest_position(f32::NAN, 0.0), None);
    }

    /// `f32::clamp` panics when its bounds are inverted, and a window narrower
    /// than its own row header inverts them.
    #[test]
    fn a_window_smaller_than_its_headers_does_not_panic() {
        let app = SpreadsheetApp::new(1.0, 1.0);
        let _ = app.cell_nearest_position(10.0, 10.0);
        let _ = app.cell_at_position(10.0, 10.0);
        let _ = app.render();
    }

    /// The bug that made frozen panes unusable: selecting a pinned cell
    /// "revealed" it by scrolling the whole sheet back to A1, so the arrow keys
    /// could not walk along the pinned band without losing your place.
    #[test]
    fn selecting_a_frozen_cell_does_not_throw_away_the_scroll_position() {
        let mut app = frozen_app(2, 3);
        let (before_x, before_y) = (app.scroll().x, app.scroll().y);
        assert!(before_x > 0.0 && before_y > 0.0, "test scrolled nowhere");
        app.ensure_cell_visible(CellAddr::new(1, 2));
        assert_eq!(app.scroll().x, before_x);
        assert_eq!(app.scroll().y, before_y);
    }

    #[test]
    fn revealing_a_scrolling_cell_clears_the_frozen_band_rather_than_the_edge() {
        let mut app = frozen_app(2, 3);
        let target = CellAddr::new(3, 4);
        app.ensure_cell_visible(target);
        // Not merely on screen: clear of the band pinned over the top of it,
        // which is the part a frozen-pane-unaware version got wrong.
        assert!(app.col_screen_x(target.col) >= app.frozen_right());
        assert!(app.row_screen_y(target.row) >= app.frozen_bottom());
    }

    // -- scrolling -------------------------------------------------------

    #[test]
    fn the_wheel_scrolls_both_axes_in_the_directions_the_two_report() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.scroll_by(400.0, 400.0);
        let (x, y) = (app.scroll().x, app.scroll().y);

        // `dy` positive is away from the user: back towards row 1.
        app.handle_event(&scroll_event(0.0, 1.0));
        assert!(app.scroll().y < y, "wheel away from the user scrolled down");
        // `dx` positive is to the right: on towards the last column. The
        // opposite relation to the offset, from the same sign of delta.
        app.handle_event(&scroll_event(1.0, 0.0));
        assert!(app.scroll().x > x, "tilting right scrolled left");
    }

    fn scroll_event(dx: f32, dy: f32) -> Event {
        Event::Mouse(MouseEvent {
            x: 400.0,
            y: 400.0,
            kind: MouseEventKind::Scroll { dx, dy },
        })
    }

    /// A trackpad's fractional notch moves a fraction of the distance. The
    /// offset is continuous pixels, so there is nothing to bank — and nothing
    /// that should round a small movement away to nothing.
    #[test]
    fn a_fraction_of_a_notch_scrolls_a_fraction_of_the_distance() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.scroll_by(0.0, 400.0);
        let before = app.scroll().y;
        app.handle_event(&scroll_event(0.0, 0.2));
        let moved = before - app.scroll().y;
        assert!(moved > 0.0, "a fifth of a notch moved nothing");
        assert!(moved < wheel::pixels(-1.0, DEFAULT_ROW_HEIGHT));
    }

    /// Note the crossed signs: reaching the *end* of the sheet means tilting
    /// right (`dx` positive) but wheeling towards the user (`dy` negative).
    /// Writing this test with matching signs is how the first draft of it
    /// failed, which is a fair demonstration of why the two axes need separate
    /// converters rather than one applied twice.
    #[test]
    fn the_wheel_cannot_scroll_past_either_end() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        for _ in 0..2000 {
            app.handle_event(&scroll_event(1.0, -1.0));
        }
        assert_eq!(app.scroll().x, app.max_scroll_x());
        assert_eq!(app.scroll().y, app.max_scroll_y());
        for _ in 0..4000 {
            app.handle_event(&scroll_event(-1.0, 1.0));
        }
        assert_eq!(app.scroll().x, 0.0);
        assert_eq!(app.scroll().y, 0.0);
    }

    /// Growing the window can leave the offset past the end of the sheet, which
    /// used to strand the view on blank space with no way back but a scroll.
    #[test]
    fn growing_the_window_pulls_the_offset_back_inside_the_sheet() {
        let mut app = SpreadsheetApp::new(400.0, 300.0);
        app.scroll_by(1e6, 1e6);
        assert!(app.scroll().y > 0.0);
        app.handle_event(&Event::Resize {
            width: 4000,
            height: 3000,
        });
        assert!(app.scroll().x <= app.max_scroll_x());
        assert!(app.scroll().y <= app.max_scroll_y());
    }

    #[test]
    fn a_nonfinite_delta_cannot_poison_the_offset() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.scroll_by(200.0, 200.0);
        app.handle_event(&scroll_event(f32::NAN, f32::INFINITY));
        assert_eq!(app.scroll().x, 200.0);
        assert_eq!(app.scroll().y, 200.0);
    }

    /// Page Up/Down step by what the pane can show, not by a guessed constant,
    /// and the step excludes the frozen band — those rows never scroll, so
    /// counting them would page past a bandful of rows every time.
    #[test]
    fn a_page_is_measured_from_the_pane_and_excludes_the_frozen_band() {
        let tall = SpreadsheetApp::new(1280.0, 1600.0);
        let short = SpreadsheetApp::new(1280.0, 400.0);
        assert!(
            tall.page_rows() > short.page_rows(),
            "a taller window pages no further"
        );
        assert!(short.page_rows() >= 1, "a page must move at least one row");

        let frozen = frozen_app(0, 8);
        let plain = SpreadsheetApp::new(1280.0, 800.0);
        assert!(
            frozen.page_rows() < plain.page_rows(),
            "the page step counted the pinned rows"
        );
    }

    #[test]
    fn page_down_then_page_up_returns_to_the_starting_row() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        *app.selection_mut() = Selection::single(CellAddr::new(0, 200));
        app.handle_event(&key_event(Key::PageDown));
        let moved = app.selection().active.row;
        assert!(moved > 200, "page down did not move");
        app.handle_event(&key_event(Key::PageUp));
        assert_eq!(app.selection().active.row, 200);
    }

    fn key_event(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        })
    }

    // -- rendering -------------------------------------------------------

    /// The four quadrants each get a clip, and the clip is what keeps a
    /// scrolled column out of the frozen band. Without it the column
    /// straddling the boundary draws into both, and no draw order fixes a half.
    #[test]
    fn a_frozen_grid_clips_every_band_it_draws() {
        let app = frozen_app(2, 3);
        let cmds = app.render();
        let pushes = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::PushClip { .. }))
            .count();
        let pops = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::PopClip))
            .count();
        assert_eq!(pushes, pops, "a clip was pushed and never popped");
        assert!(pushes >= 8, "fewer clips than the grid has bands: {pushes}");
    }

    /// No clip may have a negative extent: depending on the backend that is
    /// either an empty clip or an unbounded one, and "unbounded" would silently
    /// undo the clipping the bands depend on for correctness.
    #[test]
    fn no_clip_has_a_negative_extent() {
        for app in [
            SpreadsheetApp::new(1280.0, 800.0),
            SpreadsheetApp::new(1.0, 1.0),
            frozen_app(2, 3),
        ] {
            for cmd in app.render() {
                if let RenderCommand::PushClip { width, height, .. } = cmd {
                    assert!(width >= 0.0 && height >= 0.0, "{width} x {height}");
                }
            }
        }
    }

    /// The pinned band is drawn *after* the band that scrolls under it. An
    /// index-order loop draws frozen column A before scrolled column D — and D,
    /// having slid underneath A, then paints over the very thing pinned to
    /// outrank it.
    #[test]
    fn the_frozen_band_is_drawn_over_the_one_that_scrolls_under_it() {
        let app = frozen_app(2, 3);
        let cmds = app.render();
        // Two cells in one row band: one in the pinned columns, one in the
        // columns that scroll under them. Both must be on screen, so the
        // scrolling one is chosen from what the 500 px offset actually shows —
        // and the row is a scrolling row for the same reason.
        let cell_at = |col: usize, row: usize| {
            let (want_x, want_y) = (app.col_screen_x(col), app.row_screen_y(row));
            cmds.iter().position(|c| {
                matches!(c, RenderCommand::FillRect { x, y, .. }
                         if (x - want_x).abs() < 0.5 && (y - want_y).abs() < 0.5)
            })
        };
        let (Some(pinned), Some(scrolled)) = (cell_at(1, 15), cell_at(7, 15)) else {
            panic!("expected both a pinned and a scrolled cell to be drawn");
        };
        assert!(
            scrolled < pinned,
            "the scrolling band was drawn over the pinned one"
        );
    }

    /// The selection outline is positioned by the same law as the cell it
    /// surrounds, so it cannot end up somewhere its own cell is not.
    #[test]
    fn the_active_cell_outline_sits_on_the_cell_it_outlines() {
        let mut app = frozen_app(2, 3);
        let addr = CellAddr::new(1, 2);
        *app.selection_mut() = Selection::single(addr);
        let (want_x, want_y) = (app.col_screen_x(addr.col), app.row_screen_y(addr.row));
        let found = app.render().into_iter().any(|c| {
            matches!(c, RenderCommand::StrokeRect { x, y, color, .. }
                     if color == COLOR_BLUE
                        && (x - want_x).abs() < 0.5
                        && (y - want_y).abs() < 0.5)
        });
        assert!(found, "no outline at ({want_x}, {want_y})");
    }

    #[test]
    fn switching_sheets_starts_the_new_one_at_its_own_top_left() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.scroll_by(500.0, 500.0);
        app.add_sheet();
        assert_eq!(app.scroll().x, 0.0);
        assert_eq!(app.scroll().y, 0.0);
    }
}
