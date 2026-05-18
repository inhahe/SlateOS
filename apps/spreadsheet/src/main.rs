//! OurOS Spreadsheet
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
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

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

const MAX_COLS: usize = 26;
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

// ============================================================================
// Cell address
// ============================================================================

/// A cell address (column, row) — zero-indexed internally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellAddr {
    pub col: usize,
    pub row: usize,
}

impl CellAddr {
    /// Create a new cell address from zero-indexed column and row.
    pub fn new(col: usize, row: usize) -> Self {
        Self { col, row }
    }

    /// Convert column index (0-based) to letter string (A, B, ..., Z).
    pub fn col_letter(col: usize) -> String {
        if col < 26 {
            let ch = b'A' + col as u8;
            String::from(ch as char)
        } else {
            String::from("?")
        }
    }

    /// Display string for this cell address, e.g. "A1", "B5".
    pub fn display(&self) -> String {
        let mut s = Self::col_letter(self.col);
        s.push_str(&(self.row + 1).to_string());
        s
    }

    /// Parse a cell address string like "A1", "Z999".
    /// Returns `None` if the string is not a valid cell reference.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let upper = s.to_ascii_uppercase();
        let bytes = upper.as_bytes();
        if bytes.is_empty() || !bytes[0].is_ascii_uppercase() {
            return None;
        }
        let col_char = bytes[0];
        if col_char < b'A' || col_char > b'Z' {
            return None;
        }
        let col = (col_char - b'A') as usize;
        let row_str = &upper[1..];
        if row_str.is_empty() {
            return None;
        }
        let row_num: usize = row_str.parse().ok()?;
        if row_num == 0 || row_num > MAX_ROWS {
            return None;
        }
        Some(Self { col, row: row_num - 1 })
    }
}

// ============================================================================
// Cell value types
// ============================================================================

/// The type of data stored in a cell.
#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    /// No data.
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
            Self::Boolean(b) => if *b { "TRUE".to_string() } else { "FALSE".to_string() },
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

impl Default for CellValue {
    fn default() -> Self {
        Self::Empty
    }
}

/// Cell error types.
#[derive(Clone, Debug, PartialEq)]
pub enum CellError {
    DivisionByZero,
    InvalidReference,
    InvalidFormula,
    CircularReference,
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
            Self::ValueError => "#VALUE!",
            Self::NameError => "#NAME?",
        }
    }
}

// ============================================================================
// Number formatting
// ============================================================================

/// How to format numeric values in a cell.
#[derive(Clone, Debug, PartialEq)]
pub enum NumberFormat {
    /// General — display as-is.
    General,
    /// Fixed decimal places.
    Decimal(u8),
    /// Percentage (multiply by 100, add %).
    Percentage(u8),
    /// Currency (prefix with $).
    Currency(u8),
}

impl NumberFormat {
    /// Format a number according to this format specification.
    pub fn format_number(&self, value: f64) -> String {
        match self {
            Self::General => {
                if value == value.floor() && value.abs() < 1e15 {
                    format!("{}", value as i64)
                } else {
                    // Remove trailing zeros from decimal representation
                    let s = format!("{:.10}", value);
                    let s = s.trim_end_matches('0');
                    let s = s.trim_end_matches('.');
                    s.to_string()
                }
            }
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

impl Default for NumberFormat {
    fn default() -> Self {
        Self::General
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
        Self { top: true, bottom: true, left: true, right: true }
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

/// A rectangular range of cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellRange {
    pub start: CellAddr,
    pub end: CellAddr,
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
        Self { start: addr, end: addr }
    }

    /// Check if a cell address is within this range.
    pub fn contains(&self, addr: CellAddr) -> bool {
        addr.col >= self.start.col && addr.col <= self.end.col
            && addr.row >= self.start.row && addr.row <= self.end.row
    }

    /// Number of columns in this range.
    pub fn col_count(&self) -> usize {
        self.end.col - self.start.col + 1
    }

    /// Number of rows in this range.
    pub fn row_count(&self) -> usize {
        self.end.row - self.start.row + 1
    }

    /// Total number of cells in this range.
    pub fn cell_count(&self) -> usize {
        self.col_count() * self.row_count()
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
        if let Some(idx) = s.find(':') {
            let left = CellAddr::parse(&s[..idx])?;
            let right = CellAddr::parse(&s[idx + 1..])?;
            Some(Self::new(left, right))
        } else {
            let addr = CellAddr::parse(s)?;
            Some(Self::single(addr))
        }
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
        self.col += 1;
        if self.col > self.range.end.col {
            self.col = self.range.start.col;
            self.row += 1;
        }
        Some(addr)
    }
}

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
        self.ranges.first().copied().unwrap_or_else(|| CellRange::single(self.active))
    }

    /// Collect all numeric values in the selection from a given sheet.
    pub fn numeric_values(&self, sheet: &Sheet) -> Vec<f64> {
        let mut vals = Vec::new();
        for range in &self.ranges {
            for addr in range.iter() {
                if let Some(cell) = sheet.cells.get(&addr) {
                    if let Some(n) = cell.value.as_number() {
                        vals.push(n);
                    }
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
    AddSheet { sheet_idx: usize },
    /// Sheet removed.
    RemoveSheet { sheet_idx: usize, sheet: Sheet },
}

/// Manages undo/redo stacks.
pub struct UndoManager {
    undo_stack: Vec<UndoAction>,
    redo_stack: Vec<UndoAction>,
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
            y += self.row_heights.get(r).copied().unwrap_or(DEFAULT_ROW_HEIGHT);
        }
        y
    }

    /// Get column width.
    pub fn col_width(&self, col: usize) -> f32 {
        self.col_widths.get(col).copied().unwrap_or(DEFAULT_COL_WIDTH)
    }

    /// Get row height.
    pub fn row_height(&self, row: usize) -> f32 {
        self.row_heights.get(row).copied().unwrap_or(DEFAULT_ROW_HEIGHT)
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
    pub fn sort_by_column(&mut self, col: usize, start_row: usize, end_row: usize, ascending: bool) -> Vec<(CellAddr, Cell, Cell)> {
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
            if ascending { ordering } else { ordering.reverse() }
        });

        let sorted_indices: Vec<usize> = rows.iter().map(|(r, _, _)| *r).collect();
        let mut changes = Vec::new();

        // Collect all row data before making changes
        let all_row_data: Vec<Vec<(CellAddr, Cell)>> = sorted_indices.iter().map(|&src_row| {
            (0..MAX_COLS).map(|c| {
                let addr = CellAddr::new(c, src_row);
                (addr, self.get_cell(addr))
            }).collect()
        }).collect();

        // Apply sorted data
        for (dest_offset, row_data) in all_row_data.iter().enumerate() {
            let dest_row = start_row + dest_offset;
            for &(_, ref src_cell) in row_data {
                let col_idx = row_data.iter().position(|(_, c)| std::ptr::eq(c, src_cell)).unwrap_or(0);
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
                let text = cell.display_text();
                if text.contains(',') || text.contains('"') || text.contains('\n') {
                    result.push('"');
                    result.push_str(&text.replace('"', "\"\""));
                    result.push('"');
                } else {
                    result.push_str(&text);
                }
            }
            result.push('\n');
        }
        result
    }

    /// Import CSV data into the sheet, returning batch changes for undo.
    pub fn import_csv(&mut self, csv: &str) -> Vec<(CellAddr, Cell, Cell)> {
        let mut changes = Vec::new();
        for (row_idx, line) in csv.lines().enumerate() {
            if row_idx >= MAX_ROWS {
                break;
            }
            let fields = parse_csv_line(line);
            for (col_idx, field) in fields.into_iter().enumerate() {
                if col_idx >= MAX_COLS {
                    break;
                }
                let addr = CellAddr::new(col_idx, row_idx);
                let old = self.set_cell_input(addr, &field);
                let new_cell = self.get_cell(addr);
                if old != new_cell {
                    changes.push((addr, old, new_cell));
                }
            }
        }
        changes
    }
}

/// Parse a single CSV line into fields, respecting quoted fields.
pub fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if in_quotes {
            if ch == '"' {
                if i + 1 < chars.len() && chars[i + 1] == '"' {
                    current.push('"');
                    i += 2;
                    continue;
                } else {
                    in_quotes = false;
                    i += 1;
                    continue;
                }
            } else {
                current.push(ch);
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == ',' {
            fields.push(current.clone());
            current.clear();
        } else {
            current.push(ch);
        }
        i += 1;
    }
    fields.push(current);
    fields
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

/// Tokenize a formula string (without the leading '=').
fn tokenize_formula(input: &str) -> Result<Vec<FormulaToken>, CellError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let length = chars.len();
    let mut pos = 0;

    while pos < length {
        let ch = chars[pos];
        match ch {
            ' ' | '\t' => { pos += 1; }
            '+' => { tokens.push(FormulaToken::Plus); pos += 1; }
            '-' => { tokens.push(FormulaToken::Minus); pos += 1; }
            '*' => { tokens.push(FormulaToken::Multiply); pos += 1; }
            '/' => { tokens.push(FormulaToken::Divide); pos += 1; }
            '(' => { tokens.push(FormulaToken::LeftParen); pos += 1; }
            ')' => { tokens.push(FormulaToken::RightParen); pos += 1; }
            ',' => { tokens.push(FormulaToken::Comma); pos += 1; }
            '&' => { tokens.push(FormulaToken::Ampersand); pos += 1; }
            '<' => {
                if pos + 1 < length && chars[pos + 1] == '=' {
                    tokens.push(FormulaToken::LessEq);
                    pos += 2;
                } else if pos + 1 < length && chars[pos + 1] == '>' {
                    tokens.push(FormulaToken::NotEquals);
                    pos += 2;
                } else {
                    tokens.push(FormulaToken::LessThan);
                    pos += 1;
                }
            }
            '>' => {
                if pos + 1 < length && chars[pos + 1] == '=' {
                    tokens.push(FormulaToken::GreaterEq);
                    pos += 2;
                } else {
                    tokens.push(FormulaToken::GreaterThan);
                    pos += 1;
                }
            }
            '=' => { tokens.push(FormulaToken::Equals); pos += 1; }
            '"' => {
                pos += 1;
                let mut s = String::new();
                while pos < length && chars[pos] != '"' {
                    s.push(chars[pos]);
                    pos += 1;
                }
                if pos < length { pos += 1; } // skip closing quote
                tokens.push(FormulaToken::StringLiteral(s));
            }
            _ if ch.is_ascii_digit() || ch == '.' => {
                let start = pos;
                while pos < length && (chars[pos].is_ascii_digit() || chars[pos] == '.') {
                    pos += 1;
                }
                let num_str: String = chars[start..pos].iter().collect();
                let val: f64 = num_str.parse().map_err(|_| CellError::InvalidFormula)?;
                tokens.push(FormulaToken::Number(val));
            }
            _ if ch.is_ascii_alphabetic() => {
                let start = pos;
                while pos < length && (chars[pos].is_ascii_alphanumeric() || chars[pos] == '_') {
                    pos += 1;
                }
                let word: String = chars[start..pos].iter().collect();
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
                    if pos < length && chars[pos] == ':' {
                        pos += 1;
                        let range_start = pos;
                        while pos < length && (chars[pos].is_ascii_alphanumeric()) {
                            pos += 1;
                        }
                        let end_word: String = chars[range_start..pos].iter().collect();
                        if let Some(end_addr) = CellAddr::parse(&end_word) {
                            tokens.push(FormulaToken::RangeRef(addr, end_addr));
                        } else {
                            return Err(CellError::InvalidReference);
                        }
                    } else {
                        tokens.push(FormulaToken::CellRef(addr));
                    }
                }
                // Check if followed by '(' — function call
                else if pos < length && chars[pos] == '(' {
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

    /// Parse comparison expressions (=, <>, <, >, <=, >=).
    fn parse_comparison(&mut self) -> Result<CellValue, CellError> {
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
        if self.eval_depth > 100 {
            return Err(CellError::CircularReference);
        }
        let cell = self.sheet.get_cell(addr);
        if cell.is_formula() {
            // Recursively evaluate
            self.visited.push(addr);
            self.eval_depth += 1;
            let formula_text = &cell.raw_input[1..];
            let sub_tokens = tokenize_formula(formula_text)?;
            let mut sub_eval = FormulaEvaluator {
                tokens: sub_tokens,
                pos: 0,
                sheet: self.sheet,
                eval_depth: self.eval_depth,
                visited: self.visited.clone(),
            };
            let result = sub_eval.evaluate();
            self.eval_depth -= 1;
            let _ = self.visited.pop();
            result
        } else {
            Ok(cell.value.clone())
        }
    }

    /// Collect numeric values from a range for aggregate functions.
    fn collect_range_numbers(&mut self, start: CellAddr, end: CellAddr) -> Result<Vec<f64>, CellError> {
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
    fn collect_range_values(&mut self, start: CellAddr, end: CellAddr) -> Result<Vec<CellValue>, CellError> {
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
        let result = match name {
            "SUM" => self.eval_aggregate_func(|nums| nums.iter().sum()),
            "AVG" | "AVERAGE" => self.eval_aggregate_func(|nums| {
                if nums.is_empty() { 0.0 } else { nums.iter().sum::<f64>() / nums.len() as f64 }
            }),
            "MIN" => self.eval_aggregate_func(|nums| {
                nums.iter().copied().fold(f64::INFINITY, f64::min)
            }),
            "MAX" => self.eval_aggregate_func(|nums| {
                nums.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            }),
            "COUNT" => self.eval_count_func(),
            "IF" => self.eval_if_func(),
            "ABS" => {
                let val = self.parse_comparison()?;
                self.expect_token(&FormulaToken::RightParen)?;
                let n = require_number(&val)?;
                return Ok(CellValue::Number(n.abs()));
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
                return Ok(CellValue::Number((n * factor).round() / factor));
            }
            "CONCATENATE" | "CONCAT" => self.eval_concatenate_func(),
            "LEN" => {
                let val = self.parse_comparison()?;
                self.expect_token(&FormulaToken::RightParen)?;
                let s = value_to_string(&val);
                return Ok(CellValue::Number(s.len() as f64));
            }
            "UPPER" => {
                let val = self.parse_comparison()?;
                self.expect_token(&FormulaToken::RightParen)?;
                let s = value_to_string(&val);
                return Ok(CellValue::Text(s.to_uppercase()));
            }
            "LOWER" => {
                let val = self.parse_comparison()?;
                self.expect_token(&FormulaToken::RightParen)?;
                let s = value_to_string(&val);
                return Ok(CellValue::Text(s.to_lowercase()));
            }
            _ => return Err(CellError::NameError),
        };
        result
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
                    count += vals.iter().filter(|v| !v.is_empty()).count();
                }
                Some(FormulaToken::RightParen) => break,
                _ => {
                    let val = self.parse_comparison()?;
                    if !val.is_empty() {
                        count += 1;
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

    /// Advance to the next token.
    fn advance(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
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
            if *n == n.floor() && n.abs() < 1e15 {
                format!("{}", *n as i64)
            } else {
                format!("{}", n)
            }
        }
        CellValue::Boolean(b) => if *b { "TRUE".to_string() } else { "FALSE".to_string() },
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
        (Some(na), Some(nb)) => {
            Ok(na.partial_cmp(&nb).map(|o| o as i32).unwrap_or(0))
        }
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
    let formula_addrs: Vec<CellAddr> = sheet.cells.iter()
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

/// Detect a numeric series and produce the next value.
pub fn auto_fill_next(values: &[CellValue], index: usize) -> CellValue {
    if values.is_empty() {
        return CellValue::Empty;
    }
    if values.len() == 1 {
        return values[0].clone();
    }

    // Try to detect a numeric series
    let nums: Vec<Option<f64>> = values.iter().map(|v| v.as_number()).collect();
    if nums.iter().all(|n| n.is_some()) {
        let numbers: Vec<f64> = nums.iter().map(|n| n.unwrap_or(0.0)).collect();
        if numbers.len() >= 2 {
            let diff = numbers[1] - numbers[0];
            let is_arithmetic = numbers.windows(2)
                .all(|w| (w[1] - w[0] - diff).abs() < 1e-10);
            if is_arithmetic {
                let last = numbers[numbers.len() - 1];
                return CellValue::Number(last + diff * (index as f64 + 1.0));
            }
        }
        // Default: repeat pattern
        let pattern_idx = index % values.len();
        return values[pattern_idx].clone();
    }

    // For text: repeat pattern
    let pattern_idx = index % values.len();
    values[pattern_idx].clone()
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
        let search = if self.case_sensitive {
            self.search_text.clone()
        } else {
            self.search_text.to_lowercase()
        };

        for (&addr, cell) in &sheet.cells {
            let text = cell.display_text();
            let compare = if self.case_sensitive { text.clone() } else { text.to_lowercase() };
            if compare.contains(&search) {
                self.results.push(addr);
            }
        }
    }

    /// Move to the next search result.
    pub fn next_result(&mut self) -> Option<CellAddr> {
        if self.results.is_empty() {
            return None;
        }
        self.current_result = (self.current_result + 1) % self.results.len();
        Some(self.results[self.current_result])
    }

    /// Move to the previous search result.
    pub fn prev_result(&mut self) -> Option<CellAddr> {
        if self.results.is_empty() {
            return None;
        }
        if self.current_result == 0 {
            self.current_result = self.results.len() - 1;
        } else {
            self.current_result -= 1;
        }
        Some(self.results[self.current_result])
    }

    /// Replace current match and advance.
    pub fn replace_current(&mut self, sheet: &mut Sheet) -> Option<(CellAddr, Cell, Cell)> {
        if self.results.is_empty() {
            return None;
        }
        let addr = self.results[self.current_result];
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
        // Remove this address from results
        self.results.remove(self.current_result);
        if !self.results.is_empty() && self.current_result >= self.results.len() {
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
fn case_insensitive_replace(text: &str, search: &str, replacement: &str) -> String {
    let lower_text = text.to_lowercase();
    let lower_search = search.to_lowercase();
    let mut result = String::new();
    let mut start = 0;

    while let Some(pos) = lower_text[start..].find(&lower_search) {
        let abs_pos = start + pos;
        result.push_str(&text[start..abs_pos]);
        result.push_str(replacement);
        start = abs_pos + search.len();
    }
    result.push_str(&text[start..]);
    result
}

// ============================================================================
// Interaction modes
// ============================================================================

/// Current interaction mode for the spreadsheet.
#[derive(Clone, Debug, PartialEq)]
pub enum InteractionMode {
    /// Normal cell navigation and selection.
    Normal,
    /// User is editing a cell (typing into formula bar or cell).
    Editing { text: String, cursor_pos: usize },
    /// User is dragging to select a range.
    RangeSelect { anchor: CellAddr },
    /// User is resizing a column.
    ColResize { col: usize, start_x: f32, original_width: f32 },
    /// User is resizing a row.
    RowResize { row: usize, start_y: f32, original_height: f32 },
    /// User is dragging the auto-fill handle.
    AutoFill { anchor_range: CellRange, current_end: CellAddr },
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
#[derive(Clone, Debug, Default)]
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
        if self.x < 0.0 { self.x = 0.0; }
        if self.y < 0.0 { self.y = 0.0; }
        if self.x > max_x { self.x = max_x; }
        if self.y > max_y { self.y = max_y; }
    }
}

// ============================================================================
// Spreadsheet application state
// ============================================================================

/// The main spreadsheet application state.
pub struct SpreadsheetApp {
    /// All worksheets.
    pub sheets: Vec<Sheet>,
    /// Index of the currently active sheet.
    pub active_sheet: usize,
    /// Current cell selection.
    pub selection: Selection,
    /// Current interaction mode.
    pub mode: InteractionMode,
    /// Clipboard contents.
    pub clipboard: Option<ClipboardData>,
    /// Undo/redo manager.
    pub undo_manager: UndoManager,
    /// Scroll position.
    pub scroll: ScrollPosition,
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
            sheets: vec![Sheet::new("Sheet1")],
            active_sheet: 0,
            selection: Selection::default(),
            mode: InteractionMode::Normal,
            clipboard: None,
            undo_manager: UndoManager::new(),
            scroll: ScrollPosition::new(),
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
        self.sheets.get(self.active_sheet).unwrap_or_else(|| {
            // This should never happen, but handle gracefully
            &self.sheets[0]
        })
    }

    /// Get a mutable reference to the currently active sheet.
    pub fn active_sheet_mut(&mut self) -> &mut Sheet {
        let idx = if self.active_sheet < self.sheets.len() {
            self.active_sheet
        } else {
            0
        };
        &mut self.sheets[idx]
    }

    /// Set the active cell input, recording undo, and recalculate.
    pub fn set_cell_input(&mut self, addr: CellAddr, input: &str) {
        let sheet_idx = self.active_sheet;
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
        let cell = self.active_sheet().get_cell(self.selection.active);
        let text = if cell.is_formula() {
            cell.raw_input.clone()
        } else {
            cell.display_text()
        };
        let cursor_pos = text.len();
        self.mode = InteractionMode::Editing { text, cursor_pos };
    }

    /// Confirm cell edit and return to normal mode.
    pub fn confirm_edit(&mut self) {
        if let InteractionMode::Editing { text, .. } = &self.mode {
            let text = text.clone();
            let addr = self.selection.active;
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
        let sheet_idx = self.active_sheet;
        let mut changes = Vec::new();
        let ranges = self.selection.ranges.clone();
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
            self.undo_manager.push_action(UndoAction::BatchEdit {
                sheet_idx,
                changes,
            });
            recalculate_sheet(self.active_sheet_mut());
        }
    }

    /// Copy selected cells to clipboard.
    pub fn copy_selection(&mut self) {
        let range = self.selection.primary_range();
        let mut cells = HashMap::new();
        for addr in range.iter() {
            let cell = self.active_sheet().get_cell(addr);
            let rel_col = addr.col - range.start.col;
            let rel_row = addr.row - range.start.row;
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
        let dest = self.selection.active;
        let sheet_idx = self.active_sheet;
        let mut changes = Vec::new();

        for (&(rel_col, rel_row), src_cell) in &clip.cells {
            let target_col = dest.col + rel_col;
            let target_row = dest.row + rel_row;
            if target_col >= MAX_COLS || target_row >= MAX_ROWS {
                continue;
            }
            let target_addr = CellAddr::new(target_col, target_row);
            let old = self.active_sheet().get_cell(target_addr);
            let new_cell = src_cell.clone();
            changes.push((target_addr, old, new_cell.clone()));
            self.active_sheet_mut().set_cell(target_addr, new_cell);
        }

        if !changes.is_empty() {
            self.undo_manager.push_action(UndoAction::BatchEdit {
                sheet_idx,
                changes,
            });
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
            UndoAction::CellEdit { sheet_idx, addr, old_cell, new_cell } => {
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
            UndoAction::ColResize { sheet_idx, col, old_width, new_width } => {
                if let Some(sheet) = self.sheets.get_mut(*sheet_idx) {
                    let width = if is_undo { *old_width } else { *new_width };
                    if let Some(w) = sheet.col_widths.get_mut(*col) {
                        *w = width;
                    }
                }
            }
            UndoAction::RowResize { sheet_idx, row, old_height, new_height } => {
                if let Some(sheet) = self.sheets.get_mut(*sheet_idx) {
                    let height = if is_undo { *old_height } else { *new_height };
                    if let Some(h) = sheet.row_heights.get_mut(*row) {
                        *h = height;
                    }
                }
            }
            UndoAction::AddSheet { sheet_idx } => {
                if is_undo && *sheet_idx < self.sheets.len() {
                    self.sheets.remove(*sheet_idx);
                    if self.active_sheet >= self.sheets.len() && !self.sheets.is_empty() {
                        self.active_sheet = self.sheets.len() - 1;
                    }
                }
            }
            UndoAction::RemoveSheet { sheet_idx, sheet } => {
                if is_undo {
                    let idx = (*sheet_idx).min(self.sheets.len());
                    self.sheets.insert(idx, sheet.clone());
                    self.active_sheet = idx;
                }
            }
        }
    }

    /// Add a new sheet.
    pub fn add_sheet(&mut self) {
        let idx = self.sheets.len();
        let name = format!("Sheet{}", idx + 1);
        self.sheets.push(Sheet::new(&name));
        self.active_sheet = idx;
        self.undo_manager.push_action(UndoAction::AddSheet { sheet_idx: idx });
    }

    /// Remove the active sheet (if more than one sheet exists).
    pub fn remove_active_sheet(&mut self) {
        if self.sheets.len() <= 1 {
            return;
        }
        let idx = self.active_sheet;
        let sheet = self.sheets.remove(idx);
        self.undo_manager.push_action(UndoAction::RemoveSheet { sheet_idx: idx, sheet });
        if self.active_sheet >= self.sheets.len() {
            self.active_sheet = self.sheets.len() - 1;
        }
    }

    /// Sort the active sheet by the selected column.
    pub fn sort_column(&mut self, direction: SortDirection) {
        let col = self.selection.active.col;
        let range = self.selection.primary_range();
        let start_row = range.start.row;
        let end_row = range.end.row;
        let ascending = direction == SortDirection::Ascending;
        let sheet_idx = self.active_sheet;

        let changes = self.active_sheet_mut().sort_by_column(col, start_row, end_row, ascending);
        if !changes.is_empty() {
            self.undo_manager.push_action(UndoAction::BatchEdit { sheet_idx, changes });
            recalculate_sheet(self.active_sheet_mut());
        }
    }

    /// Auto-fill from a source range to a target range.
    pub fn auto_fill(&mut self, source: CellRange, target_end: CellAddr) {
        let target = CellRange::new(source.start, target_end);
        let sheet_idx = self.active_sheet;
        let mut changes = Vec::new();

        // Collect source values per column
        for col in source.start.col..=source.end.col {
            let source_vals: Vec<CellValue> = (source.start.row..=source.end.row)
                .map(|r| self.active_sheet().get_cell(CellAddr::new(col, r)).value.clone())
                .collect();

            let fill_start = source.end.row + 1;
            let fill_end = target.end.row;
            for row in fill_start..=fill_end {
                let idx = row - fill_start;
                let new_val = auto_fill_next(&source_vals, idx);
                let addr = CellAddr::new(col, row);
                let input = value_to_string(&new_val);
                let old_cell = self.active_sheet_mut().set_cell_input(addr, &input);
                let new_cell = self.active_sheet().get_cell(addr);
                changes.push((addr, old_cell, new_cell));
            }
        }

        if !changes.is_empty() {
            self.undo_manager.push_action(UndoAction::BatchEdit { sheet_idx, changes });
            recalculate_sheet(self.active_sheet_mut());
        }
    }

    /// Toggle bold formatting for the selected cells.
    pub fn toggle_bold(&mut self) {
        let sheet_idx = self.active_sheet;
        let mut changes = Vec::new();
        let current_bold = self.active_sheet().get_cell(self.selection.active).format.bold;
        let new_bold = !current_bold;

        let ranges = self.selection.ranges.clone();
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
            self.undo_manager.push_action(UndoAction::BatchEdit { sheet_idx, changes });
        }
    }

    /// Toggle italic formatting for the selected cells.
    pub fn toggle_italic(&mut self) {
        let sheet_idx = self.active_sheet;
        let mut changes = Vec::new();
        let current_italic = self.active_sheet().get_cell(self.selection.active).format.italic;
        let new_italic = !current_italic;

        let ranges = self.selection.ranges.clone();
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
            self.undo_manager.push_action(UndoAction::BatchEdit { sheet_idx, changes });
        }
    }

    /// Set alignment for the selected cells.
    pub fn set_alignment(&mut self, alignment: Alignment) {
        let sheet_idx = self.active_sheet;
        let mut changes = Vec::new();

        let ranges = self.selection.ranges.clone();
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
            self.undo_manager.push_action(UndoAction::BatchEdit { sheet_idx, changes });
        }
    }

    /// Set number format for the selected cells.
    pub fn set_number_format(&mut self, format: NumberFormat) {
        let sheet_idx = self.active_sheet;
        let mut changes = Vec::new();

        let ranges = self.selection.ranges.clone();
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
            self.undo_manager.push_action(UndoAction::BatchEdit { sheet_idx, changes });
        }
    }

    /// Toggle borders on selected cells.
    pub fn toggle_borders(&mut self) {
        let sheet_idx = self.active_sheet;
        let mut changes = Vec::new();
        let current_borders = self.active_sheet().get_cell(self.selection.active).format.borders.has_any();
        let new_borders = if current_borders { CellBorders::none() } else { CellBorders::all() };

        let ranges = self.selection.ranges.clone();
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
            self.undo_manager.push_action(UndoAction::BatchEdit { sheet_idx, changes });
        }
    }

    /// Freeze rows/columns at the current selection.
    pub fn toggle_freeze_panes(&mut self) {
        let col = self.selection.active.col;
        let row = self.selection.active.row;
        let sheet = self.active_sheet_mut();
        if sheet.frozen_cols > 0 || sheet.frozen_rows > 0 {
            sheet.frozen_cols = 0;
            sheet.frozen_rows = 0;
        } else {
            sheet.frozen_cols = col;
            sheet.frozen_rows = row;
        }
    }

    /// Navigate the active cell in a given direction.
    pub fn navigate(&mut self, d_col: i32, d_row: i32) {
        let new_col = (self.selection.active.col as i32 + d_col).max(0).min(MAX_COLS as i32 - 1) as usize;
        let new_row = (self.selection.active.row as i32 + d_row).max(0).min(MAX_ROWS as i32 - 1) as usize;
        let new_addr = CellAddr::new(new_col, new_row);
        self.selection = Selection::single(new_addr);
        self.ensure_cell_visible(new_addr);
    }

    /// Ensure a cell is visible by scrolling if necessary.
    pub fn ensure_cell_visible(&mut self, addr: CellAddr) {
        let sheet = self.active_sheet();
        let cell_x = sheet.col_x_offset(addr.col);
        let cell_y = sheet.row_y_offset(addr.row);
        let cell_w = sheet.col_width(addr.col);
        let cell_h = sheet.row_height(addr.row);

        let grid_x = ROW_HEADER_WIDTH;
        let _grid_y = self.grid_top();
        let grid_w = self.window_width - grid_x - SCROLLBAR_WIDTH;
        let grid_h = self.grid_height();

        // Adjust for frozen panes
        let frozen_w = sheet.col_x_offset(sheet.frozen_cols);
        let frozen_h = sheet.row_y_offset(sheet.frozen_rows);

        let visible_x = self.scroll.x + frozen_w;
        let visible_y = self.scroll.y + frozen_h;

        if cell_x < visible_x {
            self.scroll.x = (cell_x - frozen_w).max(0.0);
        } else if cell_x + cell_w > visible_x + grid_w - frozen_w {
            self.scroll.x = (cell_x + cell_w - grid_w).max(0.0);
        }

        if cell_y < visible_y {
            self.scroll.y = (cell_y - frozen_h).max(0.0);
        } else if cell_y + cell_h > visible_y + grid_h - frozen_h {
            self.scroll.y = (cell_y + cell_h - grid_h).max(0.0);
        }
    }

    /// Calculate where the grid starts (Y coordinate), accounting for toolbar and formula bar.
    pub fn grid_top(&self) -> f32 {
        let mut y = 0.0;
        if self.show_toolbar { y += TOOLBAR_HEIGHT; }
        if self.show_formula_bar { y += FORMULA_BAR_HEIGHT; }
        y += COL_HEADER_HEIGHT;
        y
    }

    /// Calculate the grid viewport height.
    pub fn grid_height(&self) -> f32 {
        let mut bottom = self.window_height;
        if self.show_status_bar { bottom -= STATUS_BAR_HEIGHT; }
        bottom -= SHEET_TAB_HEIGHT;
        let top = self.grid_top();
        (bottom - top).max(0.0)
    }

    /// Calculate the grid viewport width.
    pub fn grid_width(&self) -> f32 {
        (self.window_width - ROW_HEADER_WIDTH - SCROLLBAR_WIDTH).max(0.0)
    }

    /// Get status bar text showing SUM/AVG/COUNT of selection.
    pub fn status_bar_text(&self) -> String {
        let nums = self.selection.numeric_values(self.active_sheet());
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
        if let InteractionMode::Editing { ref mut text, ref mut cursor_pos } = self.mode {
            return handle_editing_key(text, cursor_pos, event);
        }

        // Ctrl shortcuts
        if event.modifiers.ctrl {
            match event.key {
                Key::C => { self.copy_selection(); return EventResult::Consumed; }
                Key::X => { self.cut_selection(); return EventResult::Consumed; }
                Key::V => { self.paste(); return EventResult::Consumed; }
                Key::Z => { self.undo(); return EventResult::Consumed; }
                Key::Y => { self.redo(); return EventResult::Consumed; }
                Key::B => { self.toggle_bold(); return EventResult::Consumed; }
                Key::I => { self.toggle_italic(); return EventResult::Consumed; }
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
            Key::Left => { self.navigate(-1, 0); EventResult::Consumed }
            Key::Right => { self.navigate(1, 0); EventResult::Consumed }
            Key::Up => { self.navigate(0, -1); EventResult::Consumed }
            Key::Down => { self.navigate(0, 1); EventResult::Consumed }
            Key::Home => {
                self.selection = Selection::single(CellAddr::new(0, self.selection.active.row));
                self.ensure_cell_visible(self.selection.active);
                EventResult::Consumed
            }
            Key::End => {
                self.selection = Selection::single(CellAddr::new(MAX_COLS - 1, self.selection.active.row));
                self.ensure_cell_visible(self.selection.active);
                EventResult::Consumed
            }
            Key::PageUp => { self.navigate(0, -20); EventResult::Consumed }
            Key::PageDown => { self.navigate(0, 20); EventResult::Consumed }
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
                if let Some(ch) = event.text {
                    if !ch.is_control() {
                        self.mode = InteractionMode::Editing {
                            text: String::from(ch),
                            cursor_pos: 1,
                        };
                        return EventResult::Consumed;
                    }
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
                let idx = self.active_sheet;
                let sheet = &self.sheets[idx.min(self.sheets.len().saturating_sub(1))];
                self.find_replace.find_all(sheet);
                if let Some(addr) = self.find_replace.next_result() {
                    self.selection = Selection::single(addr);
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
                if let Some(ch) = event.text {
                    if !ch.is_control() {
                        self.find_replace.search_text.push(ch);
                    }
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
            MouseEventKind::Move => {
                self.handle_mouse_move(event.x, event.y)
            }
            MouseEventKind::Scroll { dx: _, dy } => {
                self.scroll.y -= dy * 40.0;
                self.scroll.y = self.scroll.y.max(0.0);
                EventResult::Consumed
            }
            MouseEventKind::DoubleClick(MouseButton::Left) => {
                // Double-click starts editing
                let (col, row) = self.cell_at_position(event.x, event.y);
                self.selection = Selection::single(CellAddr::new(col, row));
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
                self.active_sheet = tab_idx;
                self.selection = Selection::default();
            } else if tab_idx == self.sheets.len() {
                // "+" button to add sheet
                self.add_sheet();
            }
            return EventResult::Consumed;
        }

        // Check column header resize
        let header_y = self.grid_top() - COL_HEADER_HEIGHT;
        if y >= header_y && y < header_y + COL_HEADER_HEIGHT {
            // Check for resize handles
            let sheet = self.active_sheet();
            let mut cx = ROW_HEADER_WIDTH - self.scroll.x;
            for col in 0..MAX_COLS {
                cx += sheet.col_width(col);
                if (x - cx).abs() < RESIZE_HANDLE_SIZE {
                    self.mode = InteractionMode::ColResize {
                        col,
                        start_x: x,
                        original_width: sheet.col_width(col),
                    };
                    return EventResult::Consumed;
                }
            }
            return EventResult::Consumed;
        }

        // Check row header resize
        if x < ROW_HEADER_WIDTH {
            let sheet = self.active_sheet();
            let grid_top = self.grid_top();
            let mut cy = grid_top - self.scroll.y;
            for row in 0..MAX_ROWS {
                cy += sheet.row_height(row);
                if cy < grid_top { continue; }
                if cy > self.window_height { break; }
                if (y - cy).abs() < RESIZE_HANDLE_SIZE {
                    self.mode = InteractionMode::RowResize {
                        row,
                        start_y: y,
                        original_height: sheet.row_height(row),
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

        let (col, row) = self.cell_at_position(x, y);
        let addr = CellAddr::new(col, row);

        // Check autofill handle
        let active = self.selection.active;
        let handle_x = ROW_HEADER_WIDTH + self.active_sheet().col_x_offset(active.col)
            + self.active_sheet().col_width(active.col) - self.scroll.x;
        let handle_y = self.grid_top() + self.active_sheet().row_y_offset(active.row)
            + self.active_sheet().row_height(active.row) - self.scroll.y;
        if (x - handle_x).abs() < AUTOFILL_HANDLE_SIZE && (y - handle_y).abs() < AUTOFILL_HANDLE_SIZE {
            self.mode = InteractionMode::AutoFill {
                anchor_range: self.selection.primary_range(),
                current_end: active,
            };
            return EventResult::Consumed;
        }

        self.selection = Selection::single(addr);
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
            InteractionMode::ColResize { col, original_width, start_x, .. } => {
                let col = *col;
                let original_width = *original_width;
                let _ = *start_x;
                let new_width = self.active_sheet().col_width(col);
                if (new_width - original_width).abs() > 0.5 {
                    self.undo_manager.push_action(UndoAction::ColResize {
                        sheet_idx: self.active_sheet,
                        col,
                        old_width: original_width,
                        new_width,
                    });
                }
                self.mode = InteractionMode::Normal;
                EventResult::Consumed
            }
            InteractionMode::RowResize { row, original_height, .. } => {
                let row = *row;
                let original_height = *original_height;
                let new_height = self.active_sheet().row_height(row);
                if (new_height - original_height).abs() > 0.5 {
                    self.undo_manager.push_action(UndoAction::RowResize {
                        sheet_idx: self.active_sheet,
                        row,
                        old_height: original_height,
                        new_height,
                    });
                }
                self.mode = InteractionMode::Normal;
                EventResult::Consumed
            }
            InteractionMode::AutoFill { anchor_range, current_end } => {
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
                let (col, row) = self.cell_at_position(x, y);
                let end = CellAddr::new(col, row);
                self.selection.active = end;
                self.selection.ranges = vec![CellRange::new(anchor, end)];
                EventResult::Consumed
            }
            InteractionMode::ColResize { col, start_x, original_width } => {
                let delta = x - start_x;
                let new_width = (original_width + delta).max(MIN_COL_WIDTH);
                if let Some(w) = self.active_sheet_mut().col_widths.get_mut(col) {
                    *w = new_width;
                }
                EventResult::Consumed
            }
            InteractionMode::RowResize { row, start_y, original_height } => {
                let delta = y - start_y;
                let new_height = (original_height + delta).max(MIN_ROW_HEIGHT);
                if let Some(h) = self.active_sheet_mut().row_heights.get_mut(row) {
                    *h = new_height;
                }
                EventResult::Consumed
            }
            InteractionMode::AutoFill { anchor_range, .. } => {
                let (col, row) = self.cell_at_position(x, y);
                let end = CellAddr::new(col.max(anchor_range.end.col), row.max(anchor_range.end.row));
                self.mode = InteractionMode::AutoFill {
                    anchor_range,
                    current_end: end,
                };
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Convert a pixel position to a cell column and row.
    pub fn cell_at_position(&self, x: f32, y: f32) -> (usize, usize) {
        let grid_x = x - ROW_HEADER_WIDTH + self.scroll.x;
        let grid_y = y - self.grid_top() + self.scroll.y;
        let sheet = self.active_sheet();
        let col = sheet.col_at_x(grid_x.max(0.0));
        let row = sheet.row_at_y(grid_y.max(0.0));
        (col, row)
    }

    /// Handle a resize event.
    pub fn handle_resize(&mut self, width: u32, height: u32) {
        self.window_width = width as f32;
        self.window_height = height as f32;
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
        let tab_y = self.window_height - SHEET_TAB_HEIGHT - if self.show_status_bar { STATUS_BAR_HEIGHT } else { 0.0 };
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
        let bold_active = self.active_sheet().get_cell(self.selection.active).format.bold;
        self.render_toolbar_button(cmds, bx, btn_y, btn_w, btn_h, "B", bold_active, true);
        bx += btn_w + 4.0;

        // Italic button
        let italic_active = self.active_sheet().get_cell(self.selection.active).format.italic;
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
        let current_align = self.active_sheet().get_cell(self.selection.active).format.alignment;
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
        let has_borders = self.active_sheet().get_cell(self.selection.active).format.borders.has_any();
        self.render_toolbar_button(cmds, bx, btn_y, btn_w + 8.0, btn_h, "Bdr", has_borders, false);
        bx += btn_w + 16.0;

        // Freeze panes
        let frozen = self.active_sheet().frozen_cols > 0 || self.active_sheet().frozen_rows > 0;
        self.render_toolbar_button(cmds, bx, btn_y, btn_w + 16.0, btn_h, "Freeze", frozen, false);
        bx += btn_w + 24.0;

        // Sort buttons
        self.render_toolbar_button(cmds, bx, btn_y, btn_w + 4.0, btn_h, "A-Z", false, false);
        bx += btn_w + 8.0;
        self.render_toolbar_button(cmds, bx, btn_y, btn_w + 4.0, btn_h, "Z-A", false, false);
    }

    /// Render a single toolbar button.
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
        let bg = if active { COLOR_SURFACE1 } else { COLOR_SURFACE0 };
        let fg = if active { COLOR_BLUE } else { COLOR_TEXT };

        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });

        let font_weight = if bold { FontWeightHint::Bold } else { FontWeightHint::Regular };
        cmds.push(RenderCommand::Text {
            x: x + w / 2.0 - (label.len() as f32 * 3.5),
            y: y + h / 2.0 - 5.0,
            text: label.to_string(),
            font_size: SMALL_FONT,
            color: fg,
            font_weight,
            max_width: Some(w),
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
        let addr_text = self.selection.active.display();
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

        let formula_text = if let InteractionMode::Editing { ref text, .. } = self.mode {
            text.clone()
        } else {
            let cell = self.active_sheet().get_cell(self.selection.active);
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

        // Column labels
        let mut cx = ROW_HEADER_WIDTH;
        let frozen_cols = sheet.frozen_cols;
        for col in 0..MAX_COLS {
            let w = sheet.col_width(col);
            let header_x = if col < frozen_cols {
                cx
            } else {
                cx - self.scroll.x
            };

            if header_x + w < ROW_HEADER_WIDTH {
                cx += w;
                continue;
            }
            if header_x > self.window_width {
                break;
            }

            let is_selected = self.selection.ranges.iter().any(|r| {
                col >= r.start.col && col <= r.end.col
            });

            let bg = if is_selected { COLOR_SURFACE1 } else { COLOR_MANTLE };
            cmds.push(RenderCommand::FillRect {
                x: header_x,
                y,
                width: w,
                height: COL_HEADER_HEIGHT,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });

            let label = CellAddr::col_letter(col);
            let text_color = if is_selected { COLOR_BLUE } else { COLOR_SUBTEXT1 };
            cmds.push(RenderCommand::Text {
                x: header_x + w / 2.0 - 4.0,
                y: y + 5.0,
                text: label,
                font_size: HEADER_FONT,
                color: text_color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(w),
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

            cx += w;
        }

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

        // Calculate visible row range
        let first_visible_row = if frozen_rows > 0 { 0 } else { sheet.row_at_y(self.scroll.y) };
        let last_visible_row = sheet.row_at_y(self.scroll.y + grid_h).min(MAX_ROWS - 1);

        // Render rows
        for row in first_visible_row..=last_visible_row {
            let row_h = sheet.row_height(row);
            let row_y = if row < frozen_rows {
                y_start + sheet.row_y_offset(row)
            } else {
                y_start + sheet.row_y_offset(row) - self.scroll.y
            };

            if row_y + row_h < y_start {
                continue;
            }
            if row_y > y_start + grid_h {
                break;
            }

            // Row header
            let is_row_selected = self.selection.ranges.iter().any(|r| {
                row >= r.start.row && row <= r.end.row
            });
            let header_bg = if is_row_selected { COLOR_SURFACE1 } else { COLOR_MANTLE };
            cmds.push(RenderCommand::FillRect {
                x: 0.0,
                y: row_y,
                width: ROW_HEADER_WIDTH,
                height: row_h,
                color: header_bg,
                corner_radii: CornerRadii::ZERO,
            });

            let row_label = (row + 1).to_string();
            let text_color = if is_row_selected { COLOR_BLUE } else { COLOR_SUBTEXT1 };
            cmds.push(RenderCommand::Text {
                x: 4.0,
                y: row_y + row_h / 2.0 - 6.0,
                text: row_label,
                font_size: HEADER_FONT,
                color: text_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(ROW_HEADER_WIDTH - 8.0),
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

            // Draw cells in this row
            let mut cx = ROW_HEADER_WIDTH;
            for col in 0..MAX_COLS {
                let col_w = sheet.col_width(col);
                let cell_x = if col < frozen_cols {
                    cx
                } else {
                    cx - self.scroll.x
                };

                if cell_x + col_w < ROW_HEADER_WIDTH {
                    cx += col_w;
                    continue;
                }
                if cell_x > ROW_HEADER_WIDTH + grid_w {
                    break;
                }

                let addr = CellAddr::new(col, row);
                let cell = sheet.get_cell(addr);
                let is_selected = self.selection.contains(addr);
                let is_active = addr == self.selection.active;

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
                            x1: cell_x, y1: row_y,
                            x2: cell_x + col_w, y2: row_y,
                            color: border_color, width: 1.5,
                        });
                    }
                    if cell.format.borders.bottom {
                        cmds.push(RenderCommand::Line {
                            x1: cell_x, y1: row_y + row_h,
                            x2: cell_x + col_w, y2: row_y + row_h,
                            color: border_color, width: 1.5,
                        });
                    }
                    if cell.format.borders.left {
                        cmds.push(RenderCommand::Line {
                            x1: cell_x, y1: row_y,
                            x2: cell_x, y2: row_y + row_h,
                            color: border_color, width: 1.5,
                        });
                    }
                    if cell.format.borders.right {
                        cmds.push(RenderCommand::Line {
                            x1: cell_x + col_w, y1: row_y,
                            x2: cell_x + col_w, y2: row_y + row_h,
                            color: border_color, width: 1.5,
                        });
                    }
                }

                // Cell text
                let display_text = if is_active {
                    if let InteractionMode::Editing { ref text, .. } = self.mode {
                        text.clone()
                    } else {
                        cell.display_text()
                    }
                } else {
                    cell.display_text()
                };

                if !display_text.is_empty() {
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

                    let text_x = match cell.format.alignment {
                        Alignment::Left => cell_x + 4.0,
                        Alignment::Center => cell_x + col_w / 2.0 - (display_text.len() as f32 * 3.5),
                        Alignment::Right => cell_x + col_w - (display_text.len() as f32 * 7.0) - 4.0,
                    };

                    cmds.push(RenderCommand::Text {
                        x: text_x,
                        y: row_y + row_h / 2.0 - 6.0,
                        text: display_text,
                        font_size: FONT_SIZE,
                        color: text_color,
                        font_weight,
                        max_width: Some(col_w - 8.0),
                    });
                }

                cx += col_w;
            }
        }

        // Active cell outline
        let active = self.selection.active;
        let active_x = ROW_HEADER_WIDTH + sheet.col_x_offset(active.col)
            - if active.col >= frozen_cols { self.scroll.x } else { 0.0 };
        let active_y = y_start + sheet.row_y_offset(active.row)
            - if active.row >= frozen_rows { self.scroll.y } else { 0.0 };
        let active_w = sheet.col_width(active.col);
        let active_h = sheet.row_height(active.row);

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
        for range in &self.selection.ranges {
            if range.cell_count() > 1 {
                let rx = ROW_HEADER_WIDTH + sheet.col_x_offset(range.start.col)
                    - if range.start.col >= frozen_cols { self.scroll.x } else { 0.0 };
                let ry = y_start + sheet.row_y_offset(range.start.row)
                    - if range.start.row >= frozen_rows { self.scroll.y } else { 0.0 };
                let rw: f32 = (range.start.col..=range.end.col)
                    .map(|c| sheet.col_width(c))
                    .sum();
                let rh: f32 = (range.start.row..=range.end.row)
                    .map(|r| sheet.row_height(r))
                    .sum();

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
        if let InteractionMode::AutoFill { anchor_range, current_end } = &self.mode {
            let range = CellRange::new(anchor_range.start, *current_end);
            let rx = ROW_HEADER_WIDTH + sheet.col_x_offset(range.start.col)
                - if range.start.col >= frozen_cols { self.scroll.x } else { 0.0 };
            let ry = y_start + sheet.row_y_offset(range.start.row)
                - if range.start.row >= frozen_rows { self.scroll.y } else { 0.0 };
            let rw: f32 = (range.start.col..=range.end.col)
                .map(|c| sheet.col_width(c))
                .sum();
            let rh: f32 = (range.start.row..=range.end.row)
                .map(|r| sheet.row_height(r))
                .sum();

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
                x1: fx, y1: y_start,
                x2: fx, y2: y_start + grid_h,
                color: COLOR_LAVENDER, width: 2.0,
            });
        }
        if frozen_rows > 0 {
            let fy = y_start + sheet.row_y_offset(frozen_rows);
            cmds.push(RenderCommand::Line {
                x1: 0.0, y1: fy,
                x2: self.window_width, y2: fy,
                color: COLOR_LAVENDER, width: 2.0,
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
            let is_active = idx == self.active_sheet;
            let bg = if is_active { COLOR_BASE } else { COLOR_MANTLE };
            let fg = if is_active { COLOR_BLUE } else { COLOR_SUBTEXT0 };
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
                font_weight: if is_active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(SHEET_TAB_WIDTH - 16.0),
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
            x1: 0.0, y1: y,
            x2: self.window_width, y2: y,
            color: COLOR_SURFACE0, width: 1.0,
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
        });

        // Selection range display
        let range_text = self.selection.primary_range().display();
        cmds.push(RenderCommand::Text {
            x: 80.0,
            y: y + 5.0,
            text: range_text,
            font_size: SMALL_FONT,
            color: COLOR_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
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
            let scroll_ratio = self.scroll.y / (total_content_h - grid_h);
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
            let scroll_ratio = self.scroll.x / (total_content_w - hbar_w);
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
        });

        // Buttons
        let btn_y = dlg_y + dlg_h - 34.0;
        let buttons = ["Find Next", "Replace", "Replace All"];
        let mut bx = dlg_x + 12.0;
        for label in &buttons {
            let bw = label.len() as f32 * 7.0 + 16.0;
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
            });
            bx += bw + 8.0;
        }
    }
}

/// Handle keyboard input in editing mode. Returns Consumed if handled.
fn handle_editing_key(text: &mut String, cursor_pos: &mut usize, event: &KeyEvent) -> EventResult {
    match event.key {
        Key::Backspace => {
            if *cursor_pos > 0 {
                let remove_idx = *cursor_pos - 1;
                if remove_idx < text.len() {
                    text.remove(remove_idx);
                    *cursor_pos -= 1;
                }
            }
            EventResult::Consumed
        }
        Key::Delete => {
            if *cursor_pos < text.len() {
                text.remove(*cursor_pos);
            }
            EventResult::Consumed
        }
        Key::Left => {
            if *cursor_pos > 0 {
                *cursor_pos -= 1;
            }
            EventResult::Consumed
        }
        Key::Right => {
            if *cursor_pos < text.len() {
                *cursor_pos += 1;
            }
            EventResult::Consumed
        }
        Key::Home => {
            *cursor_pos = 0;
            EventResult::Consumed
        }
        Key::End => {
            *cursor_pos = text.len();
            EventResult::Consumed
        }
        Key::Enter | Key::Tab | Key::Escape => {
            // Let the caller handle these transitions
            EventResult::Consumed
        }
        _ => {
            if let Some(ch) = event.text {
                if !ch.is_control() {
                    let byte_pos = text.char_indices()
                        .nth(*cursor_pos)
                        .map(|(i, _)| i)
                        .unwrap_or(text.len());
                    text.insert(byte_pos, ch);
                    *cursor_pos += 1;
                    return EventResult::Consumed;
                }
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
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

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
        let v = CellValue::Text("3.14".to_string());
        assert_eq!(v.as_number(), Some(3.14));
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
        let v = CellValue::Number(3.14159);
        assert_eq!(v.display_string(&NumberFormat::Decimal(2)), "3.14");
    }

    #[test]
    fn test_cell_value_display_boolean() {
        assert_eq!(CellValue::Boolean(true).display_string(&NumberFormat::General), "TRUE");
        assert_eq!(CellValue::Boolean(false).display_string(&NumberFormat::General), "FALSE");
    }

    // -- NumberFormat tests --

    #[test]
    fn test_format_general_integer() {
        assert_eq!(NumberFormat::General.format_number(100.0), "100");
    }

    #[test]
    fn test_format_general_float() {
        let s = NumberFormat::General.format_number(3.14);
        assert!(s.starts_with("3.14"));
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
        assert_eq!(r.start, CellAddr::new(2, 3));
        assert_eq!(r.end, CellAddr::new(2, 3));
        assert_eq!(r.cell_count(), 1);
    }

    #[test]
    fn test_range_normalizes() {
        let r = CellRange::new(CellAddr::new(3, 5), CellAddr::new(1, 2));
        assert_eq!(r.start, CellAddr::new(1, 2));
        assert_eq!(r.end, CellAddr::new(3, 5));
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
        assert_eq!(r.start, CellAddr::new(1, 2));
        assert_eq!(r.end, CellAddr::new(1, 2));
    }

    #[test]
    fn test_range_parse_multi() {
        let r = CellRange::parse("A1:C5").unwrap();
        assert_eq!(r.start, CellAddr::new(0, 0));
        assert_eq!(r.end, CellAddr::new(2, 4));
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
        assert_eq!(s.get_cell(CellAddr::new(0, 0)).value, CellValue::Boolean(true));
        s.set_cell_input(CellAddr::new(0, 1), "false");
        assert_eq!(s.get_cell(CellAddr::new(0, 1)).value, CellValue::Boolean(false));
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
        assert_eq!(s.get_cell(CellAddr::new(0, 0)).value, CellValue::Text("Name".to_string()));
        assert_eq!(s.get_cell(CellAddr::new(1, 1)).value, CellValue::Number(42.0));
    }

    #[test]
    fn test_parse_csv_line_simple() {
        let fields = parse_csv_line("a,b,c");
        assert_eq!(fields, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_parse_csv_line_quoted() {
        let fields = parse_csv_line("\"hello, world\",b");
        assert_eq!(fields[0], "hello, world");
    }

    #[test]
    fn test_parse_csv_line_escaped_quotes() {
        let fields = parse_csv_line("\"he said \"\"hi\"\"\",b");
        assert_eq!(fields[0], "he said \"hi\"");
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
        assert!(matches!(&tokens[0], FormulaToken::CellRef(addr) if addr.col == 0 && addr.row == 0));
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
        let sheet = Sheet::new("Test");
        assert_eq!(evaluate_formula("=ROUND(3.14159,2)", &sheet), CellValue::Number(3.14));
    }

    #[test]
    fn test_eval_round_no_places() {
        let sheet = Sheet::new("Test");
        assert_eq!(evaluate_formula("=ROUND(3.7)", &sheet), CellValue::Number(4.0));
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
        assert_eq!(evaluate_formula("=LEN(\"hello\")", &sheet), CellValue::Number(5.0));
    }

    #[test]
    fn test_eval_upper() {
        let sheet = Sheet::new("Test");
        assert_eq!(evaluate_formula("=UPPER(\"hello\")", &sheet), CellValue::Text("HELLO".to_string()));
    }

    #[test]
    fn test_eval_lower() {
        let sheet = Sheet::new("Test");
        assert_eq!(evaluate_formula("=LOWER(\"HELLO\")", &sheet), CellValue::Text("hello".to_string()));
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
        assert_eq!(sheet.get_cell(CellAddr::new(1, 0)).value, CellValue::Number(20.0));
    }

    #[test]
    fn test_recalculate_chain() {
        let mut sheet = Sheet::new("Test");
        sheet.set_cell_input(CellAddr::new(0, 0), "5");
        sheet.set_cell_input(CellAddr::new(1, 0), "=A1+1");
        sheet.set_cell_input(CellAddr::new(2, 0), "=B1+1");
        recalculate_sheet(&mut sheet);
        assert_eq!(sheet.get_cell(CellAddr::new(2, 0)).value, CellValue::Number(7.0));
    }

    // -- Auto-fill tests --

    #[test]
    fn test_auto_fill_constant() {
        let vals = vec![CellValue::Number(5.0)];
        assert_eq!(auto_fill_next(&vals, 0), CellValue::Number(5.0));
    }

    #[test]
    fn test_auto_fill_arithmetic_series() {
        let vals = vec![CellValue::Number(1.0), CellValue::Number(2.0), CellValue::Number(3.0)];
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
        let vals = vec![CellValue::Text("a".to_string()), CellValue::Text("b".to_string())];
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
        assert_eq!(case_insensitive_replace("Hello World", "hello", "hi"), "hi World");
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
        assert_eq!(r.start, CellAddr::new(3, 4));
        assert_eq!(r.end, CellAddr::new(3, 4));
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
        assert_eq!(app.active_sheet, 0);
    }

    #[test]
    fn test_app_set_cell_input() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "42");
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(0, 0)).value, CellValue::Number(42.0));
    }

    #[test]
    fn test_app_undo_redo() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "hello");
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(0, 0)).value, CellValue::Text("hello".to_string()));
        app.undo();
        assert!(app.active_sheet().get_cell(CellAddr::new(0, 0)).value.is_empty());
        app.redo();
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(0, 0)).value, CellValue::Text("hello".to_string()));
    }

    #[test]
    fn test_app_copy_paste() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "source");
        app.selection = Selection::single(CellAddr::new(0, 0));
        app.copy_selection();
        app.selection = Selection::single(CellAddr::new(1, 1));
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
        app.selection = Selection::single(CellAddr::new(0, 0));
        app.cut_selection();
        assert!(app.active_sheet().get_cell(CellAddr::new(0, 0)).value.is_empty());
        app.selection = Selection::single(CellAddr::new(2, 2));
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
        app.selection = Selection::single(CellAddr::new(0, 0));
        app.delete_selection();
        assert!(app.active_sheet().get_cell(CellAddr::new(0, 0)).value.is_empty());
    }

    #[test]
    fn test_app_add_sheet() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.add_sheet();
        assert_eq!(app.sheets.len(), 2);
        assert_eq!(app.active_sheet, 1);
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
    fn test_app_toggle_bold() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "text");
        app.selection = Selection::single(CellAddr::new(0, 0));
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
        app.selection = Selection::single(CellAddr::new(0, 0));
        app.toggle_italic();
        assert!(app.active_sheet().get_cell(CellAddr::new(0, 0)).format.italic);
    }

    #[test]
    fn test_app_set_alignment() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "text");
        app.selection = Selection::single(CellAddr::new(0, 0));
        app.set_alignment(Alignment::Center);
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(0, 0)).format.alignment, Alignment::Center);
    }

    #[test]
    fn test_app_set_number_format() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "0.5");
        app.selection = Selection::single(CellAddr::new(0, 0));
        app.set_number_format(NumberFormat::Percentage(0));
        let cell = app.active_sheet().get_cell(CellAddr::new(0, 0));
        assert_eq!(cell.format.number_format, NumberFormat::Percentage(0));
    }

    #[test]
    fn test_app_toggle_borders() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "text");
        app.selection = Selection::single(CellAddr::new(0, 0));
        app.toggle_borders();
        assert!(app.active_sheet().get_cell(CellAddr::new(0, 0)).format.borders.has_any());
        app.toggle_borders();
        assert!(!app.active_sheet().get_cell(CellAddr::new(0, 0)).format.borders.has_any());
    }

    #[test]
    fn test_app_freeze_panes() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.selection = Selection::single(CellAddr::new(2, 3));
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
        assert_eq!(app.selection.active, CellAddr::new(1, 0));
        app.navigate(0, 1);
        assert_eq!(app.selection.active, CellAddr::new(1, 1));
        app.navigate(-1, -1);
        assert_eq!(app.selection.active, CellAddr::new(0, 0));
    }

    #[test]
    fn test_app_navigate_clamp() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.navigate(-10, -10);
        assert_eq!(app.selection.active, CellAddr::new(0, 0));
    }

    #[test]
    fn test_app_begin_editing() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "hello");
        app.selection = Selection::single(CellAddr::new(0, 0));
        app.begin_editing();
        assert!(matches!(app.mode, InteractionMode::Editing { .. }));
    }

    #[test]
    fn test_app_confirm_edit() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.mode = InteractionMode::Editing { text: "99".to_string(), cursor_pos: 2 };
        app.selection = Selection::single(CellAddr::new(0, 0));
        app.confirm_edit();
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(0, 0)).value, CellValue::Number(99.0));
        assert!(matches!(app.mode, InteractionMode::Normal));
    }

    #[test]
    fn test_app_cancel_edit() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.mode = InteractionMode::Editing { text: "99".to_string(), cursor_pos: 2 };
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
        app.selection = Selection {
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
        let has_text = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text == "Hello")
        });
        assert!(has_text);
    }

    #[test]
    fn test_render_active_cell_outline() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let cmds = app.render();
        let has_stroke = cmds.iter().any(|c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == COLOR_BLUE));
        assert!(has_stroke);
    }

    #[test]
    fn test_render_find_replace_overlay() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.find_replace.active = true;
        let cmds = app.render();
        let has_overlay = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text == "Find and Replace")
        });
        assert!(has_overlay);
    }

    #[test]
    fn test_render_sheet_tabs() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.add_sheet();
        let cmds = app.render();
        let tab1 = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text == "Sheet1")
        });
        let tab2 = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text == "Sheet2")
        });
        assert!(tab1);
        assert!(tab2);
    }

    #[test]
    fn test_render_formula_bar() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let cmds = app.render();
        let has_fx = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text == "fx")
        });
        assert!(has_fx);
    }

    #[test]
    fn test_render_col_headers() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let cmds = app.render();
        let has_a = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text == "A")
        });
        assert!(has_a);
    }

    #[test]
    fn test_render_row_headers() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let cmds = app.render();
        let has_1 = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text == "1")
        });
        assert!(has_1);
    }

    // -- handle_editing_key tests --

    #[test]
    fn test_editing_backspace() {
        let mut text = "abc".to_string();
        let mut cursor = 3;
        let ev = KeyEvent { key: Key::Backspace, pressed: true, modifiers: Modifiers::NONE, text: None };
        handle_editing_key(&mut text, &mut cursor, &ev);
        assert_eq!(text, "ab");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn test_editing_delete() {
        let mut text = "abc".to_string();
        let mut cursor = 0;
        let ev = KeyEvent { key: Key::Delete, pressed: true, modifiers: Modifiers::NONE, text: None };
        handle_editing_key(&mut text, &mut cursor, &ev);
        assert_eq!(text, "bc");
        assert_eq!(cursor, 0);
    }

    #[test]
    fn test_editing_type_char() {
        let mut text = "ab".to_string();
        let mut cursor = 2;
        let ev = KeyEvent { key: Key::C, pressed: true, modifiers: Modifiers::NONE, text: Some('c') };
        handle_editing_key(&mut text, &mut cursor, &ev);
        assert_eq!(text, "abc");
        assert_eq!(cursor, 3);
    }

    #[test]
    fn test_editing_left_arrow() {
        let mut text = "abc".to_string();
        let mut cursor = 2;
        let ev = KeyEvent { key: Key::Left, pressed: true, modifiers: Modifiers::NONE, text: None };
        handle_editing_key(&mut text, &mut cursor, &ev);
        assert_eq!(cursor, 1);
    }

    #[test]
    fn test_editing_right_arrow() {
        let mut text = "abc".to_string();
        let mut cursor = 1;
        let ev = KeyEvent { key: Key::Right, pressed: true, modifiers: Modifiers::NONE, text: None };
        handle_editing_key(&mut text, &mut cursor, &ev);
        assert_eq!(cursor, 2);
    }

    #[test]
    fn test_editing_home() {
        let mut text = "abc".to_string();
        let mut cursor = 2;
        let ev = KeyEvent { key: Key::Home, pressed: true, modifiers: Modifiers::NONE, text: None };
        handle_editing_key(&mut text, &mut cursor, &ev);
        assert_eq!(cursor, 0);
    }

    #[test]
    fn test_editing_end() {
        let mut text = "abc".to_string();
        let mut cursor = 0;
        let ev = KeyEvent { key: Key::End, pressed: true, modifiers: Modifiers::NONE, text: None };
        handle_editing_key(&mut text, &mut cursor, &ev);
        assert_eq!(cursor, 3);
    }

    // -- values_equal tests --

    #[test]
    fn test_values_equal_numbers() {
        assert!(values_equal(&CellValue::Number(1.0), &CellValue::Number(1.0)));
        assert!(!values_equal(&CellValue::Number(1.0), &CellValue::Number(2.0)));
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
        assert!(!values_equal(&CellValue::Number(1.0), &CellValue::Text("1".to_string())));
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
        assert_eq!(value_to_string(&CellValue::Number(3.14)), "3.14");
    }

    #[test]
    fn test_value_to_string_boolean() {
        assert_eq!(value_to_string(&CellValue::Boolean(true)), "TRUE");
        assert_eq!(value_to_string(&CellValue::Boolean(false)), "FALSE");
    }

    #[test]
    fn test_value_to_string_error() {
        assert_eq!(value_to_string(&CellValue::Error(CellError::DivisionByZero)), "#DIV/0!");
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
        app.selection = Selection {
            active: CellAddr::new(0, 0),
            ranges: vec![CellRange::new(CellAddr::new(0, 0), CellAddr::new(0, 2))],
        };
        app.sort_column(SortDirection::Ascending);
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(0, 0)).value, CellValue::Number(1.0));
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(0, 1)).value, CellValue::Number(2.0));
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(0, 2)).value, CellValue::Number(3.0));
    }

    // -- Integration: formula with cell references after recalc --

    #[test]
    fn test_integration_sum_formula() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "10");
        app.set_cell_input(CellAddr::new(0, 1), "20");
        app.set_cell_input(CellAddr::new(0, 2), "30");
        app.set_cell_input(CellAddr::new(0, 3), "=SUM(A1:A3)");
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(0, 3)).value, CellValue::Number(60.0));
    }

    #[test]
    fn test_integration_product_formula() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "5");
        app.set_cell_input(CellAddr::new(1, 0), "10");
        app.set_cell_input(CellAddr::new(2, 0), "=A1*B1");
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(2, 0)).value, CellValue::Number(50.0));
    }

    #[test]
    fn test_integration_nested_formula() {
        let mut app = SpreadsheetApp::new(1280.0, 800.0);
        app.set_cell_input(CellAddr::new(0, 0), "100");
        app.set_cell_input(CellAddr::new(1, 0), "=A1/2");
        app.set_cell_input(CellAddr::new(2, 0), "=B1+10");
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(2, 0)).value, CellValue::Number(60.0));
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
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(0, 3)).value, CellValue::Number(4.0));
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(0, 4)).value, CellValue::Number(5.0));
        assert_eq!(app.active_sheet().get_cell(CellAddr::new(0, 5)).value, CellValue::Number(6.0));
    }

    #[test]
    fn test_ensure_cell_visible_scrolls_right() {
        let mut app = SpreadsheetApp::new(800.0, 600.0);
        app.selection = Selection::single(CellAddr::new(20, 0));
        app.ensure_cell_visible(CellAddr::new(20, 0));
        assert!(app.scroll.x > 0.0);
    }

    #[test]
    fn test_ensure_cell_visible_scrolls_down() {
        let mut app = SpreadsheetApp::new(800.0, 600.0);
        app.selection = Selection::single(CellAddr::new(0, 100));
        app.ensure_cell_visible(CellAddr::new(0, 100));
        assert!(app.scroll.y > 0.0);
    }

    #[test]
    fn test_cell_at_position() {
        let app = SpreadsheetApp::new(1280.0, 800.0);
        let grid_top = app.grid_top();
        let (col, row) = app.cell_at_position(ROW_HEADER_WIDTH + 10.0, grid_top + 10.0);
        assert_eq!(col, 0);
        assert_eq!(row, 0);
    }
}
