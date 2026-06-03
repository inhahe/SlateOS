//! OurOS Hex Editor
//!
//! Binary file hex editor with:
//! - Multi-tab document support
//! - Hex dump display: offset | hex bytes (grouped) | ASCII column
//! - Cursor navigation (arrow keys, Page Up/Down, Home/End, Ctrl+Home/End)
//! - Byte editing in hex mode and ASCII mode
//! - Insert and overwrite modes
//! - Selection (Shift+arrow, Shift+Click)
//! - Copy/paste (hex string or raw bytes)
//! - Find & replace (hex pattern, ASCII text)
//! - Go to offset dialog
//! - Data inspector (value at cursor as all common data types)
//! - Bookmarks (add/remove/navigate, with labels)
//! - Unlimited undo/redo
//! - File info and byte frequency analysis
//! - Structure templates (named fields at offsets)
//! - Highlight patterns (color-code byte patterns)
//! - Status bar with cursor offset, selection size, file size, encoding, modified indicator
//!
//! Uses the guitk library for UI rendering with Catppuccin Mocha colors.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[allow(unused_imports)]
use guitk::layout::{FlexAlign, FlexDirection, FlexItem, FlexJustify, SizeConstraint};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::{Borders, CornerRadii, Edges, FontWeight, Style, TextAlign};
#[allow(unused_imports)]
use guitk::widget::{Widget, WidgetId, WidgetTree};

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha color palette
// ============================================================================

/// Catppuccin Mocha theme colors used throughout the hex editor.
pub mod colors {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
}

// ============================================================================
// Configuration constants
// ============================================================================

/// Default number of bytes displayed per line (used by external consumers).
#[allow(dead_code)]
const DEFAULT_BYTES_PER_LINE: usize = 16;

/// Character width approximation for monospace text at a given font size.
const CHAR_WIDTH_FACTOR: f32 = 0.6;

/// Font size for hex dump display.
const HEX_FONT_SIZE: f32 = 13.0;

/// Font size for UI elements (toolbar, status bar, panels).
const UI_FONT_SIZE: f32 = 12.0;

/// Height of each hex dump line in pixels.
const LINE_HEIGHT: f32 = 18.0;

/// Height of the toolbar area.
const TOOLBAR_HEIGHT: f32 = 36.0;

/// Height of the status bar.
const STATUS_BAR_HEIGHT: f32 = 24.0;

/// Height of the tab bar.
const TAB_BAR_HEIGHT: f32 = 28.0;

/// Width of the data inspector panel.
const INSPECTOR_WIDTH: f32 = 260.0;

/// Maximum number of recent files to remember.
const MAX_RECENT_FILES: usize = 20;

/// Maximum undo stack depth (unlimited in spirit, capped at a large value to
/// prevent unbounded memory growth).
const MAX_UNDO_DEPTH: usize = 10_000;

// ============================================================================
// Data model — View configuration
// ============================================================================

/// Number of bytes displayed per line in the hex dump.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub enum BytesPerLine {
    Eight = 8,
    #[default]
    Sixteen = 16,
    ThirtyTwo = 32,
}

impl BytesPerLine {
    /// Return the numeric value.
    pub const fn value(self) -> usize {
        self as usize
    }
}


/// How to display the offset column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub enum OffsetDisplay {
    #[default]
    Hex,
    Decimal,
}


/// Hex view configuration.
#[derive(Clone, Debug)]
pub struct HexView {
    pub bytes_per_line: BytesPerLine,
    pub offset_display: OffsetDisplay,
    pub show_ascii: bool,
    pub show_inspector: bool,
    pub scroll_offset: usize,
}

impl Default for HexView {
    fn default() -> Self {
        Self {
            bytes_per_line: BytesPerLine::default(),
            offset_display: OffsetDisplay::default(),
            show_ascii: true,
            show_inspector: true,
            scroll_offset: 0,
        }
    }
}

// ============================================================================
// Data model — Edit mode
// ============================================================================

/// Editing mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub enum EditMode {
    ReadOnly,
    Insert,
    #[default]
    Overwrite,
}


// ============================================================================
// Data model — Data types for inspector
// ============================================================================

/// Data types the inspector can display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataType {
    U8,
    I8,
    U16Le,
    U16Be,
    I16Le,
    I16Be,
    U32Le,
    U32Be,
    I32Le,
    I32Be,
    U64Le,
    U64Be,
    I64Le,
    I64Be,
    F32Le,
    F32Be,
    F64Le,
    F64Be,
    AsciiString,
    Utf8String,
}

impl DataType {
    /// All data types in display order.
    pub const ALL: &'static [DataType] = &[
        DataType::U8,
        DataType::I8,
        DataType::U16Le,
        DataType::U16Be,
        DataType::I16Le,
        DataType::I16Be,
        DataType::U32Le,
        DataType::U32Be,
        DataType::I32Le,
        DataType::I32Be,
        DataType::U64Le,
        DataType::U64Be,
        DataType::I64Le,
        DataType::I64Be,
        DataType::F32Le,
        DataType::F32Be,
        DataType::F64Le,
        DataType::F64Be,
        DataType::AsciiString,
        DataType::Utf8String,
    ];

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            DataType::U8 => "uint8",
            DataType::I8 => "int8",
            DataType::U16Le => "uint16 LE",
            DataType::U16Be => "uint16 BE",
            DataType::I16Le => "int16 LE",
            DataType::I16Be => "int16 BE",
            DataType::U32Le => "uint32 LE",
            DataType::U32Be => "uint32 BE",
            DataType::I32Le => "int32 LE",
            DataType::I32Be => "int32 BE",
            DataType::U64Le => "uint64 LE",
            DataType::U64Be => "uint64 BE",
            DataType::I64Le => "int64 LE",
            DataType::I64Be => "int64 BE",
            DataType::F32Le => "float32 LE",
            DataType::F32Be => "float32 BE",
            DataType::F64Le => "float64 LE",
            DataType::F64Be => "float64 BE",
            DataType::AsciiString => "ASCII",
            DataType::Utf8String => "UTF-8",
        }
    }

    /// Number of bytes this type requires (0 for variable-length strings).
    pub fn byte_count(self) -> usize {
        match self {
            DataType::U8 | DataType::I8 => 1,
            DataType::U16Le | DataType::U16Be | DataType::I16Le | DataType::I16Be => 2,
            DataType::U32Le | DataType::U32Be | DataType::I32Le | DataType::I32Be => 4,
            DataType::U64Le | DataType::U64Be | DataType::I64Le | DataType::I64Be => 8,
            DataType::F32Le | DataType::F32Be => 4,
            DataType::F64Le | DataType::F64Be => 8,
            DataType::AsciiString | DataType::Utf8String => 0,
        }
    }
}

// ============================================================================
// Data model — Selection
// ============================================================================

/// A byte range selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Start offset (inclusive).
    pub start: usize,
    /// End offset (inclusive).
    pub end: usize,
    /// Whether the cursor (active end) is at the start or end of the selection.
    pub active_at_end: bool,
}

impl Selection {
    /// Create a new selection with cursor at the end.
    pub fn new(start: usize, end: usize) -> Self {
        let (s, e) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        Self {
            start: s,
            end: e,
            active_at_end: true,
        }
    }

    /// Number of bytes in the selection.
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start).saturating_add(1)
    }

    /// Whether the selection is empty (zero-length).
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Check if an offset falls within the selection.
    pub fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset <= self.end
    }

    /// The active (cursor) end offset.
    pub fn active_offset(&self) -> usize {
        if self.active_at_end {
            self.end
        } else {
            self.start
        }
    }

    /// The anchor (non-cursor) end offset.
    pub fn anchor_offset(&self) -> usize {
        if self.active_at_end {
            self.start
        } else {
            self.end
        }
    }
}

// ============================================================================
// Data model — Search
// ============================================================================

/// Direction for search operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub enum SearchDirection {
    #[default]
    Forward,
    Backward,
}


/// What kind of pattern to search for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchPattern {
    /// Raw hex bytes (e.g., "FF 00 AB").
    HexBytes(Vec<u8>),
    /// ASCII text.
    AsciiText(String),
    /// Basic regex pattern — stored as the source string.
    Regex(String),
}

/// Complete search query.
#[derive(Clone, Debug)]
pub struct SearchQuery {
    pub pattern: SearchPattern,
    pub direction: SearchDirection,
    pub case_sensitive: bool,
    pub wrap_around: bool,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            pattern: SearchPattern::AsciiText(String::new()),
            direction: SearchDirection::Forward,
            case_sensitive: true,
            wrap_around: true,
        }
    }
}

/// State of an active search / replace dialog.
#[derive(Clone, Debug)]
#[derive(Default)]
pub struct SearchState {
    pub query: SearchQuery,
    pub replace_pattern: Option<Vec<u8>>,
    pub last_match: Option<usize>,
    pub match_count: usize,
    pub visible: bool,
    /// Text in the search input field.
    pub input_text: String,
    /// Text in the replace input field.
    pub replace_text: String,
}


// ============================================================================
// Data model — Bookmarks
// ============================================================================

/// A bookmark at a specific offset.
#[derive(Clone, Debug)]
pub struct Bookmark {
    pub offset: usize,
    pub label: String,
    pub color: Color,
}

impl Bookmark {
    pub fn new(offset: usize, label: &str, color: Color) -> Self {
        Self {
            offset,
            label: label.to_string(),
            color,
        }
    }
}

// ============================================================================
// Data model — Undo / Redo
// ============================================================================

/// A single undoable edit operation.
#[derive(Clone, Debug)]
pub struct UndoEntry {
    /// Offset where the edit occurred.
    pub offset: usize,
    /// The old bytes that were replaced / removed.
    pub old_bytes: Vec<u8>,
    /// The new bytes that were inserted / written.
    pub new_bytes: Vec<u8>,
    /// Cursor position before the edit.
    pub cursor_before: usize,
}

// ============================================================================
// Data model — Structure templates
// ============================================================================

/// A named field within a structure template.
#[derive(Clone, Debug)]
pub struct StructField {
    /// Offset relative to the structure's base offset.
    pub relative_offset: usize,
    /// Human-readable field name.
    pub name: String,
    /// Data type of this field.
    pub data_type: DataType,
}

/// A structure template that maps named fields at offsets.
#[derive(Clone, Debug)]
pub struct StructTemplate {
    pub name: String,
    pub fields: Vec<StructField>,
}

impl StructTemplate {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            fields: Vec::new(),
        }
    }

    /// Add a field to the template.
    pub fn add_field(&mut self, relative_offset: usize, name: &str, data_type: DataType) {
        self.fields.push(StructField {
            relative_offset,
            name: name.to_string(),
            data_type,
        });
    }

    /// Total size in bytes (up to the end of the last field).
    pub fn total_size(&self) -> usize {
        self.fields
            .iter()
            .map(|f| {
                let bc = f.data_type.byte_count();
                // Strings get a default length of 16.
                let size = if bc == 0 { 16 } else { bc };
                f.relative_offset.saturating_add(size)
            })
            .max()
            .unwrap_or(0)
    }
}

// ============================================================================
// Data model — Highlight patterns
// ============================================================================

/// A pattern to highlight in the hex view.
#[derive(Clone, Debug)]
pub struct HighlightPattern {
    /// The byte pattern to match.
    pub pattern: Vec<u8>,
    /// Color to use for highlighting.
    pub color: Color,
    /// Human-readable label.
    pub label: String,
    /// Whether this pattern is active.
    pub enabled: bool,
}

// ============================================================================
// Data model — HexDocument (one open file/buffer)
// ============================================================================

/// A single open document in the hex editor.
#[derive(Clone, Debug)]
pub struct HexDocument {
    /// The raw file data.
    pub data: Vec<u8>,
    /// File path (if loaded from / saved to disk).
    pub file_path: Option<String>,
    /// Whether the buffer has been modified since last save.
    pub modified: bool,
    /// Undo stack (most recent at the end).
    pub undo_stack: Vec<UndoEntry>,
    /// Redo stack (most recent at the end).
    pub redo_stack: Vec<UndoEntry>,
    /// Bookmarks.
    pub bookmarks: Vec<Bookmark>,
    /// Current cursor position (byte offset).
    pub cursor: usize,
    /// Current selection (if any).
    pub selection: Option<Selection>,
    /// View configuration for this document.
    pub view: HexView,
    /// Current edit mode.
    pub edit_mode: EditMode,
    /// Whether the cursor is in the hex column (true) or ASCII column (false).
    pub cursor_in_hex: bool,
    /// Nibble position within the current byte when editing hex (0 = high, 1 = low).
    pub hex_nibble: u8,
    /// Applied structure templates.
    pub templates: Vec<(usize, StructTemplate)>,
    /// Highlight patterns.
    pub highlights: Vec<HighlightPattern>,
}

impl HexDocument {
    /// Create a new empty document.
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            file_path: None,
            modified: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            bookmarks: Vec::new(),
            cursor: 0,
            selection: None,
            view: HexView::default(),
            edit_mode: EditMode::Overwrite,
            cursor_in_hex: true,
            hex_nibble: 0,
            templates: Vec::new(),
            highlights: Vec::new(),
        }
    }

    /// Create a document from existing data.
    pub fn from_data(data: Vec<u8>) -> Self {
        Self {
            data,
            ..Self::new()
        }
    }

    /// Create a document from a file path and data.
    pub fn from_file(path: &str, data: Vec<u8>) -> Self {
        Self {
            data,
            file_path: Some(path.to_string()),
            ..Self::new()
        }
    }

    /// Display name for tabs.
    pub fn display_name(&self) -> String {
        if let Some(ref path) = self.file_path {
            // Extract filename from path.
            path.rsplit('/')
                .next()
                .unwrap_or(path.as_str())
                .to_string()
        } else {
            String::from("Untitled")
        }
    }

    /// Total number of lines in the hex dump.
    pub fn total_lines(&self) -> usize {
        let bpl = self.view.bytes_per_line.value();
        if bpl == 0 {
            return 0;
        }
        if self.data.is_empty() {
            return 1;
        }
        self.data
            .len()
            .saturating_sub(1)
            .checked_div(bpl)
            .map_or(0, |d| d.saturating_add(1))
    }

    /// Line number for a given byte offset.
    pub fn line_for_offset(&self, offset: usize) -> usize {
        let bpl = self.view.bytes_per_line.value();
        if bpl == 0 {
            return 0;
        }
        offset.checked_div(bpl).unwrap_or(0)
    }

    /// Column number (within a line) for a given byte offset.
    pub fn column_for_offset(&self, offset: usize) -> usize {
        let bpl = self.view.bytes_per_line.value();
        if bpl == 0 {
            return 0;
        }
        offset.checked_rem(bpl).unwrap_or(0)
    }

    /// Byte offset for a given line and column.
    pub fn offset_for_line_col(&self, line: usize, col: usize) -> usize {
        let bpl = self.view.bytes_per_line.value();
        line.saturating_mul(bpl).saturating_add(col)
    }

    /// Clamp cursor to valid range.
    pub fn clamp_cursor(&mut self) {
        if self.data.is_empty() {
            self.cursor = 0;
        } else {
            let max = self.data.len().saturating_sub(1);
            if self.cursor > max {
                self.cursor = max;
            }
        }
    }

    /// Ensure the cursor is visible by adjusting scroll offset.
    pub fn ensure_cursor_visible(&mut self, visible_lines: usize) {
        let cursor_line = self.line_for_offset(self.cursor);
        if cursor_line < self.view.scroll_offset {
            self.view.scroll_offset = cursor_line;
        } else if visible_lines > 0 && cursor_line >= self.view.scroll_offset.saturating_add(visible_lines)
        {
            self.view.scroll_offset = cursor_line
                .saturating_sub(visible_lines)
                .saturating_add(1);
        }
    }

    // ========================================================================
    // Editing operations with undo support
    // ========================================================================

    /// Overwrite a byte at the given offset.
    pub fn overwrite_byte(&mut self, offset: usize, new_byte: u8) {
        if offset >= self.data.len() {
            return;
        }
        let old_byte = self.data.get(offset).copied().unwrap_or(0);
        if old_byte == new_byte {
            return;
        }

        let entry = UndoEntry {
            offset,
            old_bytes: vec![old_byte],
            new_bytes: vec![new_byte],
            cursor_before: self.cursor,
        };
        self.push_undo(entry);

        if let Some(b) = self.data.get_mut(offset) {
            *b = new_byte;
        }
        self.modified = true;
    }

    /// Insert a byte at the given offset.
    pub fn insert_byte(&mut self, offset: usize, byte: u8) {
        let insert_at = offset.min(self.data.len());
        let entry = UndoEntry {
            offset: insert_at,
            old_bytes: Vec::new(),
            new_bytes: vec![byte],
            cursor_before: self.cursor,
        };
        self.push_undo(entry);

        self.data.insert(insert_at, byte);
        self.modified = true;
    }

    /// Delete a byte at the given offset.
    pub fn delete_byte(&mut self, offset: usize) {
        if offset >= self.data.len() {
            return;
        }
        let old_byte = self.data.get(offset).copied().unwrap_or(0);
        let entry = UndoEntry {
            offset,
            old_bytes: vec![old_byte],
            new_bytes: Vec::new(),
            cursor_before: self.cursor,
        };
        self.push_undo(entry);

        self.data.remove(offset);
        self.modified = true;
    }

    /// Delete a range of bytes.
    pub fn delete_range(&mut self, start: usize, end: usize) {
        if start >= self.data.len() || start > end {
            return;
        }
        let actual_end = end.min(self.data.len().saturating_sub(1));
        let old_bytes: Vec<u8> = self
            .data
            .get(start..=actual_end)
            .unwrap_or(&[])
            .to_vec();
        if old_bytes.is_empty() {
            return;
        }

        let entry = UndoEntry {
            offset: start,
            old_bytes,
            new_bytes: Vec::new(),
            cursor_before: self.cursor,
        };
        self.push_undo(entry);

        let drain_end = actual_end.saturating_add(1).min(self.data.len());
        self.data.drain(start..drain_end);
        self.modified = true;
    }

    /// Replace a range of bytes with new data.
    pub fn replace_range(&mut self, start: usize, end: usize, new_bytes: &[u8]) {
        if start > end {
            return;
        }
        let actual_end = end.min(
            if self.data.is_empty() {
                0
            } else {
                self.data.len().saturating_sub(1)
            },
        );
        let old_bytes: Vec<u8> = if start < self.data.len() {
            self.data
                .get(start..=actual_end)
                .unwrap_or(&[])
                .to_vec()
        } else {
            Vec::new()
        };

        let entry = UndoEntry {
            offset: start,
            old_bytes: old_bytes.clone(),
            new_bytes: new_bytes.to_vec(),
            cursor_before: self.cursor,
        };
        self.push_undo(entry);

        // Remove old range.
        if !old_bytes.is_empty() && start < self.data.len() {
            let drain_end = actual_end.saturating_add(1).min(self.data.len());
            self.data.drain(start..drain_end);
        }
        // Insert new bytes.
        let insert_at = start.min(self.data.len());
        for (i, &b) in new_bytes.iter().enumerate() {
            self.data.insert(insert_at.saturating_add(i), b);
        }
        self.modified = true;
    }

    /// Push an undo entry, clearing redo and capping stack size.
    fn push_undo(&mut self, entry: UndoEntry) {
        self.redo_stack.clear();
        self.undo_stack.push(entry);
        if self.undo_stack.len() > MAX_UNDO_DEPTH {
            self.undo_stack.remove(0);
        }
    }

    /// Undo the most recent edit.
    pub fn undo(&mut self) -> bool {
        if let Some(entry) = self.undo_stack.pop() {
            // Reverse the operation: remove new_bytes, insert old_bytes.
            let start = entry.offset;
            let new_len = entry.new_bytes.len();
            let drain_end = start.saturating_add(new_len).min(self.data.len());
            if new_len > 0 && start < self.data.len() {
                self.data.drain(start..drain_end);
            }
            for (i, &b) in entry.old_bytes.iter().enumerate() {
                let pos = start.saturating_add(i).min(self.data.len());
                self.data.insert(pos, b);
            }
            self.cursor = entry.cursor_before;
            self.clamp_cursor();

            // Move to redo stack (with swapped old/new).
            self.redo_stack.push(UndoEntry {
                offset: entry.offset,
                old_bytes: entry.new_bytes,
                new_bytes: entry.old_bytes,
                cursor_before: entry.cursor_before,
            });
            self.modified = true;
            true
        } else {
            false
        }
    }

    /// Redo the most recently undone edit.
    pub fn redo(&mut self) -> bool {
        if let Some(entry) = self.redo_stack.pop() {
            let start = entry.offset;
            let new_len = entry.new_bytes.len();
            let drain_end = start.saturating_add(new_len).min(self.data.len());
            if new_len > 0 && start < self.data.len() {
                self.data.drain(start..drain_end);
            }
            for (i, &b) in entry.old_bytes.iter().enumerate() {
                let pos = start.saturating_add(i).min(self.data.len());
                self.data.insert(pos, b);
            }
            self.cursor = entry.cursor_before;
            self.clamp_cursor();

            self.undo_stack.push(UndoEntry {
                offset: entry.offset,
                old_bytes: entry.new_bytes,
                new_bytes: entry.old_bytes,
                cursor_before: entry.cursor_before,
            });
            self.modified = true;
            true
        } else {
            false
        }
    }

    // ========================================================================
    // Search
    // ========================================================================

    /// Find the next occurrence of the pattern starting from `from_offset`.
    pub fn find_next(&self, query: &SearchQuery, from_offset: usize) -> Option<usize> {
        let bytes_to_find = match &query.pattern {
            SearchPattern::HexBytes(b) => b.clone(),
            SearchPattern::AsciiText(s) => {
                if query.case_sensitive {
                    s.as_bytes().to_vec()
                } else {
                    s.to_lowercase().as_bytes().to_vec()
                }
            }
            SearchPattern::Regex(_) => {
                // Basic regex not implemented in no_std; treat as literal.
                return None;
            }
        };

        if bytes_to_find.is_empty() || bytes_to_find.len() > self.data.len() {
            return None;
        }

        match query.direction {
            SearchDirection::Forward => {
                self.find_forward(&bytes_to_find, from_offset, query.case_sensitive, query.wrap_around)
            }
            SearchDirection::Backward => {
                self.find_backward(&bytes_to_find, from_offset, query.case_sensitive, query.wrap_around)
            }
        }
    }

    fn find_forward(
        &self,
        pattern: &[u8],
        from: usize,
        case_sensitive: bool,
        wrap: bool,
    ) -> Option<usize> {
        let data_len = self.data.len();
        if pattern.is_empty() || data_len == 0 {
            return None;
        }
        let search_len = data_len.saturating_sub(pattern.len()).saturating_add(1);

        // Search from `from` to end.
        for i in from..search_len {
            if self.match_at(i, pattern, case_sensitive) {
                return Some(i);
            }
        }

        // Wrap around.
        if wrap && from > 0 {
            let limit = from.min(search_len);
            for i in 0..limit {
                if self.match_at(i, pattern, case_sensitive) {
                    return Some(i);
                }
            }
        }

        None
    }

    fn find_backward(
        &self,
        pattern: &[u8],
        from: usize,
        case_sensitive: bool,
        wrap: bool,
    ) -> Option<usize> {
        let data_len = self.data.len();
        if pattern.is_empty() || data_len == 0 {
            return None;
        }
        let max_start = data_len.saturating_sub(pattern.len());

        // Search from `from` backwards.
        let start_at = from.min(max_start);
        for i in (0..=start_at).rev() {
            if self.match_at(i, pattern, case_sensitive) {
                return Some(i);
            }
        }

        // Wrap around.
        if wrap {
            for i in (start_at.saturating_add(1)..=max_start).rev() {
                if self.match_at(i, pattern, case_sensitive) {
                    return Some(i);
                }
            }
        }

        None
    }

    fn match_at(&self, offset: usize, pattern: &[u8], case_sensitive: bool) -> bool {
        if offset.saturating_add(pattern.len()) > self.data.len() {
            return false;
        }
        for (i, &p) in pattern.iter().enumerate() {
            let d = self.data.get(offset.saturating_add(i)).copied().unwrap_or(0);
            if case_sensitive {
                if d != p {
                    return false;
                }
            } else {
                // Case-insensitive: compare lowercase ASCII.
                if !d.eq_ignore_ascii_case(&p) {
                    return false;
                }
            }
        }
        true
    }

    /// Count all occurrences of the pattern.
    pub fn count_matches(&self, query: &SearchQuery) -> usize {
        let bytes_to_find = match &query.pattern {
            SearchPattern::HexBytes(b) => b.clone(),
            SearchPattern::AsciiText(s) => {
                if query.case_sensitive {
                    s.as_bytes().to_vec()
                } else {
                    s.to_lowercase().as_bytes().to_vec()
                }
            }
            SearchPattern::Regex(_) => return 0,
        };

        if bytes_to_find.is_empty() || bytes_to_find.len() > self.data.len() {
            return 0;
        }

        let mut count = 0usize;
        let search_len = self.data.len().saturating_sub(bytes_to_find.len()).saturating_add(1);
        for i in 0..search_len {
            if self.match_at(i, &bytes_to_find, query.case_sensitive) {
                count = count.saturating_add(1);
            }
        }
        count
    }

    // ========================================================================
    // Bookmarks
    // ========================================================================

    /// Add a bookmark at the given offset.
    pub fn add_bookmark(&mut self, offset: usize, label: &str, color: Color) {
        // Don't add duplicate at same offset.
        if self.bookmarks.iter().any(|b| b.offset == offset) {
            return;
        }
        self.bookmarks.push(Bookmark::new(offset, label, color));
        self.bookmarks.sort_by_key(|b| b.offset);
    }

    /// Remove bookmark at the given offset.
    pub fn remove_bookmark(&mut self, offset: usize) {
        self.bookmarks.retain(|b| b.offset != offset);
    }

    /// Toggle bookmark at the given offset.
    pub fn toggle_bookmark(&mut self, offset: usize) {
        if self.bookmarks.iter().any(|b| b.offset == offset) {
            self.remove_bookmark(offset);
        } else {
            self.add_bookmark(offset, "", colors::YELLOW);
        }
    }

    /// Navigate to the next bookmark after the cursor.
    pub fn next_bookmark(&self) -> Option<usize> {
        self.bookmarks
            .iter()
            .find(|b| b.offset > self.cursor)
            .map(|b| b.offset)
            .or_else(|| self.bookmarks.first().map(|b| b.offset))
    }

    /// Navigate to the previous bookmark before the cursor.
    pub fn prev_bookmark(&self) -> Option<usize> {
        self.bookmarks
            .iter()
            .rev()
            .find(|b| b.offset < self.cursor)
            .map(|b| b.offset)
            .or_else(|| self.bookmarks.last().map(|b| b.offset))
    }

    // ========================================================================
    // Data inspector
    // ========================================================================

    /// Interpret bytes at the given offset as the specified data type.
    pub fn inspect_at(&self, offset: usize, dtype: DataType) -> Option<String> {
        let remaining = self.data.len().saturating_sub(offset);
        let _needed = dtype.byte_count();

        match dtype {
            DataType::U8 => {
                let b = self.data.get(offset).copied()?;
                Some(format!("{b}"))
            }
            DataType::I8 => {
                let b = self.data.get(offset).copied()?;
                Some(format!("{}", b as i8))
            }
            DataType::U16Le => {
                if remaining < 2 {
                    return None;
                }
                let bytes: [u8; 2] = [
                    self.data.get(offset).copied()?,
                    self.data.get(offset.saturating_add(1)).copied()?,
                ];
                Some(format!("{}", u16::from_le_bytes(bytes)))
            }
            DataType::U16Be => {
                if remaining < 2 {
                    return None;
                }
                let bytes: [u8; 2] = [
                    self.data.get(offset).copied()?,
                    self.data.get(offset.saturating_add(1)).copied()?,
                ];
                Some(format!("{}", u16::from_be_bytes(bytes)))
            }
            DataType::I16Le => {
                if remaining < 2 {
                    return None;
                }
                let bytes: [u8; 2] = [
                    self.data.get(offset).copied()?,
                    self.data.get(offset.saturating_add(1)).copied()?,
                ];
                Some(format!("{}", i16::from_le_bytes(bytes)))
            }
            DataType::I16Be => {
                if remaining < 2 {
                    return None;
                }
                let bytes: [u8; 2] = [
                    self.data.get(offset).copied()?,
                    self.data.get(offset.saturating_add(1)).copied()?,
                ];
                Some(format!("{}", i16::from_be_bytes(bytes)))
            }
            DataType::U32Le => {
                if remaining < 4 {
                    return None;
                }
                let bytes = self.read_4_bytes(offset)?;
                Some(format!("{}", u32::from_le_bytes(bytes)))
            }
            DataType::U32Be => {
                if remaining < 4 {
                    return None;
                }
                let bytes = self.read_4_bytes(offset)?;
                Some(format!("{}", u32::from_be_bytes(bytes)))
            }
            DataType::I32Le => {
                if remaining < 4 {
                    return None;
                }
                let bytes = self.read_4_bytes(offset)?;
                Some(format!("{}", i32::from_le_bytes(bytes)))
            }
            DataType::I32Be => {
                if remaining < 4 {
                    return None;
                }
                let bytes = self.read_4_bytes(offset)?;
                Some(format!("{}", i32::from_be_bytes(bytes)))
            }
            DataType::U64Le => {
                if remaining < 8 {
                    return None;
                }
                let bytes = self.read_8_bytes(offset)?;
                Some(format!("{}", u64::from_le_bytes(bytes)))
            }
            DataType::U64Be => {
                if remaining < 8 {
                    return None;
                }
                let bytes = self.read_8_bytes(offset)?;
                Some(format!("{}", u64::from_be_bytes(bytes)))
            }
            DataType::I64Le => {
                if remaining < 8 {
                    return None;
                }
                let bytes = self.read_8_bytes(offset)?;
                Some(format!("{}", i64::from_le_bytes(bytes)))
            }
            DataType::I64Be => {
                if remaining < 8 {
                    return None;
                }
                let bytes = self.read_8_bytes(offset)?;
                Some(format!("{}", i64::from_be_bytes(bytes)))
            }
            DataType::F32Le => {
                if remaining < 4 {
                    return None;
                }
                let bytes = self.read_4_bytes(offset)?;
                Some(format!("{}", f32::from_le_bytes(bytes)))
            }
            DataType::F32Be => {
                if remaining < 4 {
                    return None;
                }
                let bytes = self.read_4_bytes(offset)?;
                Some(format!("{}", f32::from_be_bytes(bytes)))
            }
            DataType::F64Le => {
                if remaining < 8 {
                    return None;
                }
                let bytes = self.read_8_bytes(offset)?;
                Some(format!("{}", f64::from_le_bytes(bytes)))
            }
            DataType::F64Be => {
                if remaining < 8 {
                    return None;
                }
                let bytes = self.read_8_bytes(offset)?;
                Some(format!("{}", f64::from_be_bytes(bytes)))
            }
            DataType::AsciiString => {
                let mut s = String::new();
                let max_len = 32.min(remaining);
                for i in 0..max_len {
                    let b = self.data.get(offset.saturating_add(i)).copied()?;
                    if b == 0 {
                        break;
                    }
                    if b.is_ascii_graphic() || b == b' ' {
                        s.push(b as char);
                    } else {
                        s.push('.');
                    }
                }
                Some(s)
            }
            DataType::Utf8String => {
                let max_len = 32.min(remaining);
                let slice = self.data.get(offset..offset.saturating_add(max_len))?;
                // Find the first null byte.
                let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
                match core::str::from_utf8(slice.get(..end).unwrap_or(&[])) {
                    Ok(s) => Some(s.to_string()),
                    Err(_) => Some(String::from("<invalid UTF-8>")),
                }
            }
        }
    }

    /// Read 4 bytes starting at offset.
    fn read_4_bytes(&self, offset: usize) -> Option<[u8; 4]> {
        Some([
            self.data.get(offset).copied()?,
            self.data.get(offset.saturating_add(1)).copied()?,
            self.data.get(offset.saturating_add(2)).copied()?,
            self.data.get(offset.saturating_add(3)).copied()?,
        ])
    }

    /// Read 8 bytes starting at offset.
    fn read_8_bytes(&self, offset: usize) -> Option<[u8; 8]> {
        Some([
            self.data.get(offset).copied()?,
            self.data.get(offset.saturating_add(1)).copied()?,
            self.data.get(offset.saturating_add(2)).copied()?,
            self.data.get(offset.saturating_add(3)).copied()?,
            self.data.get(offset.saturating_add(4)).copied()?,
            self.data.get(offset.saturating_add(5)).copied()?,
            self.data.get(offset.saturating_add(6)).copied()?,
            self.data.get(offset.saturating_add(7)).copied()?,
        ])
    }

    // ========================================================================
    // Byte frequency analysis
    // ========================================================================

    /// Count occurrences of each byte value (0..=255).
    pub fn byte_frequency(&self) -> [usize; 256] {
        let mut freq = [0usize; 256];
        for &b in &self.data {
            let idx = b as usize;
            freq[idx] = freq[idx].saturating_add(1);
        }
        freq
    }

    /// Return the byte value that appears most frequently.
    pub fn most_frequent_byte(&self) -> Option<(u8, usize)> {
        if self.data.is_empty() {
            return None;
        }
        let freq = self.byte_frequency();
        let mut max_byte = 0u8;
        let mut max_count = 0usize;
        for (i, &count) in freq.iter().enumerate() {
            if count > max_count {
                max_count = count;
                // Safe: i is in 0..256.
                max_byte = i as u8;
            }
        }
        Some((max_byte, max_count))
    }

    // ========================================================================
    // Copy / paste
    // ========================================================================

    /// Copy the selection (or byte at cursor) as hex string.
    pub fn copy_as_hex(&self) -> String {
        let (start, end) = self.selected_range();
        let mut s = String::new();
        for i in start..=end {
            if let Some(&b) = self.data.get(i) {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(&format!("{b:02X}"));
            }
        }
        s
    }

    /// Copy the selection (or byte at cursor) as raw bytes.
    pub fn copy_as_bytes(&self) -> Vec<u8> {
        let (start, end) = self.selected_range();
        let actual_end = end.min(self.data.len().saturating_sub(1));
        self.data.get(start..=actual_end).unwrap_or(&[]).to_vec()
    }

    /// Paste hex string at cursor position.
    pub fn paste_hex(&mut self, hex_str: &str) -> bool {
        if let Some(bytes) = parse_hex_string(hex_str) {
            self.paste_bytes(&bytes);
            true
        } else {
            false
        }
    }

    /// Paste raw bytes at cursor position.
    pub fn paste_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        match self.edit_mode {
            EditMode::ReadOnly => {}
            EditMode::Insert => {
                // Insert all bytes at cursor.
                for (i, &b) in bytes.iter().enumerate() {
                    self.insert_byte(self.cursor.saturating_add(i), b);
                }
                self.cursor = self.cursor.saturating_add(bytes.len());
                self.clamp_cursor();
            }
            EditMode::Overwrite => {
                // Overwrite starting at cursor, extending file if needed.
                for (i, &b) in bytes.iter().enumerate() {
                    let off = self.cursor.saturating_add(i);
                    if off < self.data.len() {
                        self.overwrite_byte(off, b);
                    } else {
                        self.insert_byte(off, b);
                    }
                }
                self.cursor = self.cursor.saturating_add(bytes.len());
                self.clamp_cursor();
            }
        }
    }

    /// Get the selected byte range (or single byte at cursor).
    fn selected_range(&self) -> (usize, usize) {
        if let Some(sel) = &self.selection {
            (sel.start, sel.end)
        } else {
            (self.cursor, self.cursor)
        }
    }

    // ========================================================================
    // Highlight pattern matching
    // ========================================================================

    /// Check if a byte at offset is part of any highlight pattern.
    pub fn highlight_color_at(&self, offset: usize) -> Option<Color> {
        for hl in &self.highlights {
            if !hl.enabled || hl.pattern.is_empty() {
                continue;
            }
            let pat_len = hl.pattern.len();
            // Check if offset falls within any match of this pattern.
            let start = offset.saturating_sub(pat_len.saturating_sub(1));
            let end = offset.saturating_add(1).min(self.data.len());
            for check_start in start..end {
                if check_start.saturating_add(pat_len) > self.data.len() {
                    continue;
                }
                let matches = self
                    .data
                    .get(check_start..check_start.saturating_add(pat_len)) == Some(hl.pattern.as_slice());
                if matches
                    && offset >= check_start
                    && offset < check_start.saturating_add(pat_len)
                {
                    return Some(hl.color);
                }
            }
        }
        None
    }
}

impl Default for HexDocument {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Data model — HexEditor (multi-tab, application state)
// ============================================================================

/// Which panel/dialog is currently focused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusedPanel {
    HexView,
    SearchBar,
    GoToDialog,
    Inspector,
}

/// Complete hex editor application state.
#[derive(Clone, Debug)]
pub struct HexEditor {
    /// Open documents (tabs).
    pub documents: Vec<HexDocument>,
    /// Index of the active tab.
    pub active_tab: usize,
    /// Search/replace state.
    pub search: SearchState,
    /// Whether the data inspector panel is visible.
    pub show_inspector: bool,
    /// Recent file paths.
    pub recent_files: VecDeque<String>,
    /// Window width.
    pub window_width: f32,
    /// Window height.
    pub window_height: f32,
    /// Which panel has focus.
    pub focused_panel: FocusedPanel,
    /// Go-to-offset dialog state.
    pub goto_visible: bool,
    /// Go-to-offset input text.
    pub goto_text: String,
    /// Whether to show byte frequency analysis.
    pub show_frequency: bool,
    /// Whether to show file info.
    pub show_file_info: bool,
}

impl HexEditor {
    /// Create a new hex editor with one empty document.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            documents: vec![HexDocument::new()],
            active_tab: 0,
            search: SearchState::default(),
            show_inspector: true,
            recent_files: VecDeque::new(),
            window_width: width,
            window_height: height,
            focused_panel: FocusedPanel::HexView,
            goto_visible: false,
            goto_text: String::new(),
            show_frequency: false,
            show_file_info: false,
        }
    }

    /// Get a reference to the active document.
    pub fn active_doc(&self) -> &HexDocument {
        self.documents.get(self.active_tab).unwrap_or_else(|| {
            // This should never happen if we maintain invariants, but return
            // first doc as fallback.
            &self.documents[0]
        })
    }

    /// Get a mutable reference to the active document.
    pub fn active_doc_mut(&mut self) -> &mut HexDocument {
        let idx = if self.active_tab < self.documents.len() {
            self.active_tab
        } else {
            0
        };
        &mut self.documents[idx]
    }

    /// Open a new tab with the given document.
    pub fn open_tab(&mut self, doc: HexDocument) {
        // Track in recent files.
        if let Some(ref path) = doc.file_path {
            self.add_recent_file(path);
        }
        self.documents.push(doc);
        self.active_tab = self.documents.len().saturating_sub(1);
    }

    /// Close the tab at the given index.
    pub fn close_tab(&mut self, index: usize) {
        if self.documents.len() <= 1 {
            // Don't close the last tab; replace with empty doc.
            self.documents[0] = HexDocument::new();
            self.active_tab = 0;
            return;
        }
        if index < self.documents.len() {
            self.documents.remove(index);
            if self.active_tab >= self.documents.len() {
                self.active_tab = self.documents.len().saturating_sub(1);
            }
        }
    }

    /// Switch to the next tab.
    pub fn next_tab(&mut self) {
        if !self.documents.is_empty() {
            self.active_tab = (self.active_tab.saturating_add(1)) % self.documents.len();
        }
    }

    /// Switch to the previous tab.
    pub fn prev_tab(&mut self) {
        if !self.documents.is_empty() {
            if self.active_tab == 0 {
                self.active_tab = self.documents.len().saturating_sub(1);
            } else {
                self.active_tab = self.active_tab.saturating_sub(1);
            }
        }
    }

    /// Add a file path to the recent files list.
    pub fn add_recent_file(&mut self, path: &str) {
        // Remove if already present.
        self.recent_files.retain(|p| p != path);
        self.recent_files.push_front(path.to_string());
        while self.recent_files.len() > MAX_RECENT_FILES {
            self.recent_files.pop_back();
        }
    }

    /// Number of visible lines in the hex view area.
    pub fn visible_lines(&self) -> usize {
        let content_height = self.window_height
            - TOOLBAR_HEIGHT
            - TAB_BAR_HEIGHT
            - STATUS_BAR_HEIGHT;
        if content_height <= 0.0 || LINE_HEIGHT <= 0.0 {
            return 0;
        }
        (content_height / LINE_HEIGHT) as usize
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    /// Handle a keyboard event.
    pub fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }

        // Global shortcuts (regardless of focus).
        if key.modifiers.ctrl {
            match key.key {
                Key::Z => {
                    self.active_doc_mut().undo();
                    return EventResult::Consumed;
                }
                Key::Y => {
                    self.active_doc_mut().redo();
                    return EventResult::Consumed;
                }
                Key::F => {
                    self.search.visible = !self.search.visible;
                    if self.search.visible {
                        self.focused_panel = FocusedPanel::SearchBar;
                    } else {
                        self.focused_panel = FocusedPanel::HexView;
                    }
                    return EventResult::Consumed;
                }
                Key::G => {
                    self.goto_visible = !self.goto_visible;
                    if self.goto_visible {
                        self.focused_panel = FocusedPanel::GoToDialog;
                    } else {
                        self.focused_panel = FocusedPanel::HexView;
                    }
                    return EventResult::Consumed;
                }
                Key::C => {
                    // Copy — hex string.
                    let _ = self.active_doc().copy_as_hex();
                    return EventResult::Consumed;
                }
                Key::V => {
                    // Paste handled elsewhere (needs clipboard data).
                    return EventResult::Consumed;
                }
                Key::Tab => {
                    if key.modifiers.shift {
                        self.prev_tab();
                    } else {
                        self.next_tab();
                    }
                    return EventResult::Consumed;
                }
                Key::Home => {
                    // Ctrl+Home: go to start.
                    let vis = self.visible_lines();
                    let doc = self.active_doc_mut();
                    doc.cursor = 0;
                    doc.hex_nibble = 0;
                    doc.ensure_cursor_visible(vis);
                    return EventResult::Consumed;
                }
                Key::End => {
                    // Ctrl+End: go to end.
                    let vis = self.visible_lines();
                    let doc = self.active_doc_mut();
                    if !doc.data.is_empty() {
                        doc.cursor = doc.data.len().saturating_sub(1);
                    }
                    doc.hex_nibble = 0;
                    doc.ensure_cursor_visible(vis);
                    return EventResult::Consumed;
                }
                Key::B => {
                    // Toggle bookmark at cursor.
                    let cursor = self.active_doc().cursor;
                    self.active_doc_mut().toggle_bookmark(cursor);
                    return EventResult::Consumed;
                }
                Key::N => {
                    // Next bookmark.
                    if let Some(off) = self.active_doc().next_bookmark() {
                        let vis = self.visible_lines();
                        let doc = self.active_doc_mut();
                        doc.cursor = off;
                        doc.ensure_cursor_visible(vis);
                    }
                    return EventResult::Consumed;
                }
                Key::P => {
                    // Previous bookmark.
                    if let Some(off) = self.active_doc().prev_bookmark() {
                        let vis = self.visible_lines();
                        let doc = self.active_doc_mut();
                        doc.cursor = off;
                        doc.ensure_cursor_visible(vis);
                    }
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }

        // Escape closes dialogs.
        if key.key == Key::Escape {
            if self.search.visible {
                self.search.visible = false;
                self.focused_panel = FocusedPanel::HexView;
                return EventResult::Consumed;
            }
            if self.goto_visible {
                self.goto_visible = false;
                self.focused_panel = FocusedPanel::HexView;
                return EventResult::Consumed;
            }
            // Clear selection.
            self.active_doc_mut().selection = None;
            return EventResult::Consumed;
        }

        // Enter in search bar: perform search.
        if key.key == Key::Enter && self.focused_panel == FocusedPanel::SearchBar {
            self.perform_search();
            return EventResult::Consumed;
        }

        // Enter in goto dialog: go to offset.
        if key.key == Key::Enter && self.focused_panel == FocusedPanel::GoToDialog {
            self.perform_goto();
            return EventResult::Consumed;
        }

        // Insert key toggles edit mode.
        if key.key == Key::Insert {
            let doc = self.active_doc_mut();
            doc.edit_mode = match doc.edit_mode {
                EditMode::ReadOnly => EditMode::Overwrite,
                EditMode::Insert => EditMode::Overwrite,
                EditMode::Overwrite => EditMode::Insert,
            };
            return EventResult::Consumed;
        }

        // Tab key toggles between hex and ASCII columns.
        if key.key == Key::Tab && !key.modifiers.ctrl {
            let doc = self.active_doc_mut();
            doc.cursor_in_hex = !doc.cursor_in_hex;
            doc.hex_nibble = 0;
            return EventResult::Consumed;
        }

        // Navigation keys.
        if self.focused_panel == FocusedPanel::HexView {
            if self.handle_navigation(key) {
                return EventResult::Consumed;
            }
            if self.handle_hex_input(key) {
                return EventResult::Consumed;
            }
        }

        // Text input for search/goto dialogs.
        if self.focused_panel == FocusedPanel::SearchBar {
            if let Some(ch) = key.text {
                self.search.input_text.push(ch);
                return EventResult::Consumed;
            }
            if key.key == Key::Backspace && !self.search.input_text.is_empty() {
                self.search.input_text.pop();
                return EventResult::Consumed;
            }
        }
        if self.focused_panel == FocusedPanel::GoToDialog {
            if let Some(ch) = key.text {
                self.goto_text.push(ch);
                return EventResult::Consumed;
            }
            if key.key == Key::Backspace && !self.goto_text.is_empty() {
                self.goto_text.pop();
                return EventResult::Consumed;
            }
        }

        EventResult::Ignored
    }

    /// Handle cursor navigation keys.
    fn handle_navigation(&mut self, key: &KeyEvent) -> bool {
        let vis = self.visible_lines();
        let bpl = self.active_doc().view.bytes_per_line.value();
        let extending = key.modifiers.shift;

        match key.key {
            Key::Left => {
                let doc = self.active_doc_mut();
                let old_cursor = doc.cursor;
                if doc.cursor > 0 {
                    doc.cursor = doc.cursor.saturating_sub(1);
                    doc.hex_nibble = 0;
                }
                Self::update_selection(doc, old_cursor, extending);
                doc.ensure_cursor_visible(vis);
                true
            }
            Key::Right => {
                let doc = self.active_doc_mut();
                let old_cursor = doc.cursor;
                let max = if doc.data.is_empty() {
                    0
                } else {
                    doc.data.len().saturating_sub(1)
                };
                if doc.cursor < max {
                    doc.cursor = doc.cursor.saturating_add(1);
                    doc.hex_nibble = 0;
                }
                Self::update_selection(doc, old_cursor, extending);
                doc.ensure_cursor_visible(vis);
                true
            }
            Key::Up => {
                let doc = self.active_doc_mut();
                let old_cursor = doc.cursor;
                if doc.cursor >= bpl {
                    doc.cursor = doc.cursor.saturating_sub(bpl);
                    doc.hex_nibble = 0;
                }
                Self::update_selection(doc, old_cursor, extending);
                doc.ensure_cursor_visible(vis);
                true
            }
            Key::Down => {
                let doc = self.active_doc_mut();
                let old_cursor = doc.cursor;
                let max = if doc.data.is_empty() {
                    0
                } else {
                    doc.data.len().saturating_sub(1)
                };
                let new_pos = doc.cursor.saturating_add(bpl);
                if new_pos <= max {
                    doc.cursor = new_pos;
                    doc.hex_nibble = 0;
                }
                Self::update_selection(doc, old_cursor, extending);
                doc.ensure_cursor_visible(vis);
                true
            }
            Key::Home => {
                let doc = self.active_doc_mut();
                let old_cursor = doc.cursor;
                let line_start = doc.line_for_offset(doc.cursor).saturating_mul(bpl);
                doc.cursor = line_start;
                doc.hex_nibble = 0;
                Self::update_selection(doc, old_cursor, extending);
                doc.ensure_cursor_visible(vis);
                true
            }
            Key::End => {
                let doc = self.active_doc_mut();
                let old_cursor = doc.cursor;
                let line_start = doc.line_for_offset(doc.cursor).saturating_mul(bpl);
                let line_end = line_start
                    .saturating_add(bpl)
                    .saturating_sub(1)
                    .min(if doc.data.is_empty() { 0 } else { doc.data.len().saturating_sub(1) });
                doc.cursor = line_end;
                doc.hex_nibble = 0;
                Self::update_selection(doc, old_cursor, extending);
                doc.ensure_cursor_visible(vis);
                true
            }
            Key::PageUp => {
                let doc = self.active_doc_mut();
                let old_cursor = doc.cursor;
                let page = vis.saturating_mul(bpl);
                doc.cursor = doc.cursor.saturating_sub(page);
                doc.hex_nibble = 0;
                Self::update_selection(doc, old_cursor, extending);
                doc.ensure_cursor_visible(vis);
                true
            }
            Key::PageDown => {
                let doc = self.active_doc_mut();
                let old_cursor = doc.cursor;
                let max = if doc.data.is_empty() {
                    0
                } else {
                    doc.data.len().saturating_sub(1)
                };
                let page = vis.saturating_mul(bpl);
                doc.cursor = doc.cursor.saturating_add(page).min(max);
                doc.hex_nibble = 0;
                Self::update_selection(doc, old_cursor, extending);
                doc.ensure_cursor_visible(vis);
                true
            }
            Key::Delete => {
                let doc = self.active_doc_mut();
                if doc.edit_mode != EditMode::ReadOnly {
                    if let Some(sel) = doc.selection.take() {
                        doc.delete_range(sel.start, sel.end);
                        doc.cursor = sel.start;
                    } else {
                        doc.delete_byte(doc.cursor);
                    }
                    doc.clamp_cursor();
                }
                true
            }
            Key::Backspace => {
                let doc = self.active_doc_mut();
                if doc.edit_mode != EditMode::ReadOnly && doc.cursor > 0 {
                    if let Some(sel) = doc.selection.take() {
                        doc.delete_range(sel.start, sel.end);
                        doc.cursor = sel.start;
                    } else {
                        doc.cursor = doc.cursor.saturating_sub(1);
                        doc.delete_byte(doc.cursor);
                    }
                    doc.clamp_cursor();
                }
                true
            }
            _ => false,
        }
    }

    /// Handle hex digit / ASCII input in the hex view.
    fn handle_hex_input(&mut self, key: &KeyEvent) -> bool {
        let doc = self.active_doc_mut();
        if doc.edit_mode == EditMode::ReadOnly {
            return false;
        }

        if doc.cursor_in_hex {
            // Hex editing: accept hex digits.
            let nibble_val = match key.key {
                Key::Num0 => Some(0u8),
                Key::Num1 => Some(1),
                Key::Num2 => Some(2),
                Key::Num3 => Some(3),
                Key::Num4 => Some(4),
                Key::Num5 => Some(5),
                Key::Num6 => Some(6),
                Key::Num7 => Some(7),
                Key::Num8 => Some(8),
                Key::Num9 => Some(9),
                Key::A => Some(0xA),
                Key::B => Some(0xB),
                Key::C => Some(0xC),
                Key::D => Some(0xD),
                Key::E => Some(0xE),
                Key::F => Some(0xF),
                _ => None,
            };

            if let Some(nib) = nibble_val {
                if key.modifiers.ctrl || key.modifiers.alt {
                    return false;
                }
                let cur = doc.cursor;
                if doc.edit_mode == EditMode::Overwrite && cur < doc.data.len() {
                    let old = doc.data.get(cur).copied().unwrap_or(0);
                    let new_byte = if doc.hex_nibble == 0 {
                        (nib << 4) | (old & 0x0F)
                    } else {
                        (old & 0xF0) | nib
                    };
                    doc.overwrite_byte(cur, new_byte);

                    if doc.hex_nibble == 0 {
                        doc.hex_nibble = 1;
                    } else {
                        doc.hex_nibble = 0;
                        if cur.saturating_add(1) < doc.data.len() {
                            doc.cursor = cur.saturating_add(1);
                        }
                    }
                } else if doc.edit_mode == EditMode::Insert {
                    if doc.hex_nibble == 0 {
                        doc.insert_byte(cur, nib << 4);
                        doc.hex_nibble = 1;
                    } else {
                        let old = doc.data.get(cur).copied().unwrap_or(0);
                        let new_byte = (old & 0xF0) | nib;
                        doc.overwrite_byte(cur, new_byte);
                        doc.hex_nibble = 0;
                        doc.cursor = cur.saturating_add(1);
                    }
                }
                let vis = self.visible_lines();
                self.active_doc_mut().ensure_cursor_visible(vis);
                return true;
            }
        } else {
            // ASCII editing: accept printable characters.
            if let Some(ch) = key.text
                && ch.is_ascii() && !ch.is_ascii_control() {
                    let cur = doc.cursor;
                    let byte = ch as u8;
                    if doc.edit_mode == EditMode::Overwrite && cur < doc.data.len() {
                        doc.overwrite_byte(cur, byte);
                        if cur.saturating_add(1) < doc.data.len() {
                            doc.cursor = cur.saturating_add(1);
                        }
                    } else if doc.edit_mode == EditMode::Insert {
                        doc.insert_byte(cur, byte);
                        doc.cursor = cur.saturating_add(1);
                    }
                    let vis = self.visible_lines();
                    self.active_doc_mut().ensure_cursor_visible(vis);
                    return true;
                }
        }

        false
    }

    /// Update selection based on cursor movement with shift held.
    fn update_selection(doc: &mut HexDocument, old_cursor: usize, extending: bool) {
        if extending {
            if let Some(ref mut sel) = doc.selection {
                // Extend from the anchor.
                let anchor = sel.anchor_offset();
                let new_cursor = doc.cursor;
                if new_cursor >= anchor {
                    sel.start = anchor;
                    sel.end = new_cursor;
                    sel.active_at_end = true;
                } else {
                    sel.start = new_cursor;
                    sel.end = anchor;
                    sel.active_at_end = false;
                }
            } else {
                // Start new selection.
                doc.selection = Some(Selection::new(old_cursor, doc.cursor));
            }
        } else {
            doc.selection = None;
        }
    }

    /// Perform search based on current search state.
    fn perform_search(&mut self) {
        let input = self.search.input_text.clone();
        if input.is_empty() {
            return;
        }

        // Try to parse as hex bytes first (if it looks like hex).
        let pattern = if input.chars().all(|c| c.is_ascii_hexdigit() || c == ' ') {
            if let Some(bytes) = parse_hex_string(&input) {
                SearchPattern::HexBytes(bytes)
            } else {
                SearchPattern::AsciiText(input.clone())
            }
        } else {
            SearchPattern::AsciiText(input.clone())
        };

        self.search.query.pattern = pattern;
        let from = self.active_doc().cursor.saturating_add(1);
        if let Some(offset) = self.active_doc().find_next(&self.search.query, from) {
            let vis = self.visible_lines();
            let doc = self.active_doc_mut();
            doc.cursor = offset;
            doc.ensure_cursor_visible(vis);
            self.search.last_match = Some(offset);
        }
        self.search.match_count = self.active_doc().count_matches(&self.search.query);
    }

    /// Perform go-to-offset.
    fn perform_goto(&mut self) {
        let text = self.goto_text.trim().to_string();
        if text.is_empty() {
            return;
        }

        let offset = if let Some(stripped) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"))
        {
            usize::from_str_radix(stripped, 16).ok()
        } else if text.starts_with('$') {
            usize::from_str_radix(text.get(1..).unwrap_or(""), 16).ok()
        } else {
            text.parse::<usize>().ok()
        };

        if let Some(off) = offset {
            let vis = self.visible_lines();
            let doc = self.active_doc_mut();
            doc.cursor = off.min(if doc.data.is_empty() {
                0
            } else {
                doc.data.len().saturating_sub(1)
            });
            doc.hex_nibble = 0;
            doc.ensure_cursor_visible(vis);
        }

        self.goto_visible = false;
        self.focused_panel = FocusedPanel::HexView;
    }

    /// Handle mouse click in the hex view area.
    pub fn handle_mouse_click(&mut self, x: f32, y: f32, shift_held: bool) {
        let bpl = self.active_doc().view.bytes_per_line.value();
        let content_y = y - TOOLBAR_HEIGHT - TAB_BAR_HEIGHT;
        if content_y < 0.0 {
            return;
        }

        let line = (content_y / LINE_HEIGHT) as usize;
        let absolute_line = line.saturating_add(self.active_doc().view.scroll_offset);
        let char_w = HEX_FONT_SIZE * CHAR_WIDTH_FACTOR;

        // Offset column width (10 chars for hex offset "00000000: ").
        let offset_col_width = char_w * 10.0;

        if x < offset_col_width {
            return; // Clicked on offset column.
        }

        let hex_x = x - offset_col_width;
        // Each byte in hex is "XX " = 3 chars, with extra space every 8 bytes.
        let hex_col_width = (bpl as f32) * char_w * 3.0 + ((bpl / 8) as f32) * char_w;

        if hex_x < hex_col_width {
            // Clicked in hex column.
            let col_approx = (hex_x / (char_w * 3.0)) as usize;
            let col = col_approx.min(bpl.saturating_sub(1));
            let offset = absolute_line.saturating_mul(bpl).saturating_add(col);

            let doc = self.active_doc_mut();
            let old_cursor = doc.cursor;
            let max = if doc.data.is_empty() {
                0
            } else {
                doc.data.len().saturating_sub(1)
            };
            doc.cursor = offset.min(max);
            doc.cursor_in_hex = true;
            doc.hex_nibble = 0;
            Self::update_selection(doc, old_cursor, shift_held);
        } else {
            // Clicked in ASCII column.
            let ascii_x = hex_x - hex_col_width - char_w * 2.0;
            if ascii_x >= 0.0 {
                let col = (ascii_x / char_w) as usize;
                let col = col.min(bpl.saturating_sub(1));
                let offset = absolute_line.saturating_mul(bpl).saturating_add(col);

                let doc = self.active_doc_mut();
                let old_cursor = doc.cursor;
                let max = if doc.data.is_empty() {
                    0
                } else {
                    doc.data.len().saturating_sub(1)
                };
                doc.cursor = offset.min(max);
                doc.cursor_in_hex = false;
                doc.hex_nibble = 0;
                Self::update_selection(doc, old_cursor, shift_held);
            }
        }
    }

    /// Handle scroll event.
    pub fn handle_scroll(&mut self, dy: f32) {
        let doc = self.active_doc_mut();
        let lines = if dy < 0.0 { 3usize } else { 0usize };
        let lines_down = if dy > 0.0 { 3usize } else { 0usize };

        if lines > 0 {
            doc.view.scroll_offset = doc.view.scroll_offset.saturating_sub(lines);
        }
        if lines_down > 0 {
            let max_scroll = doc.total_lines().saturating_sub(1);
            doc.view.scroll_offset = doc
                .view
                .scroll_offset
                .saturating_add(lines_down)
                .min(max_scroll);
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire hex editor UI.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background.
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: colors::BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut tree);
        self.render_tab_bar(&mut tree);
        self.render_hex_view(&mut tree);
        if self.show_inspector {
            self.render_inspector(&mut tree);
        }
        self.render_status_bar(&mut tree);

        if self.search.visible {
            self.render_search_bar(&mut tree);
        }
        if self.goto_visible {
            self.render_goto_dialog(&mut tree);
        }

        tree
    }

    /// Render the toolbar.
    fn render_toolbar(&self, tree: &mut RenderTree) {
        // Toolbar background.
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: TOOLBAR_HEIGHT,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Toolbar buttons.
        let buttons = [
            ("New", 8.0),
            ("Open", 58.0),
            ("Save", 114.0),
            ("Undo", 174.0),
            ("Redo", 224.0),
            ("Find", 284.0),
            ("GoTo", 334.0),
        ];

        for &(label, x) in &buttons {
            tree.push(RenderCommand::FillRect {
                x,
                y: 4.0,
                width: 44.0,
                height: 28.0,
                color: colors::SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            tree.push(RenderCommand::Text {
                x: x + 6.0,
                y: 10.0,
                text: label.to_string(),
                color: colors::TEXT,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(38.0),
            });
        }

        // Separator.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT - 1.0,
            x2: self.window_width,
            y2: TOOLBAR_HEIGHT - 1.0,
            color: colors::SURFACE1,
            width: 1.0,
        });
    }

    /// Render the tab bar.
    fn render_tab_bar(&self, tree: &mut RenderTree) {
        let y = TOOLBAR_HEIGHT;

        // Tab bar background.
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: TAB_BAR_HEIGHT,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let mut tab_x: f32 = 4.0;
        let char_w = UI_FONT_SIZE * CHAR_WIDTH_FACTOR;

        for (i, doc) in self.documents.iter().enumerate() {
            let name = doc.display_name();
            let modified_mark = if doc.modified { " *" } else { "" };
            let tab_label = format!("{name}{modified_mark}");
            let tab_width = (tab_label.len() as f32) * char_w + 16.0;

            let bg_color = if i == self.active_tab {
                colors::BASE
            } else {
                colors::SURFACE0
            };

            tree.push(RenderCommand::FillRect {
                x: tab_x,
                y: y + 2.0,
                width: tab_width,
                height: TAB_BAR_HEIGHT - 2.0,
                color: bg_color,
                corner_radii: CornerRadii {
                    top_left: 4.0,
                    top_right: 4.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });

            let text_color = if i == self.active_tab {
                colors::TEXT
            } else {
                colors::SUBTEXT0
            };

            tree.push(RenderCommand::Text {
                x: tab_x + 8.0,
                y: y + 8.0,
                text: tab_label,
                color: text_color,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(tab_width),
            });

            tab_x += tab_width + 2.0;
        }

        // Bottom separator.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TAB_BAR_HEIGHT - 1.0,
            x2: self.window_width,
            y2: y + TAB_BAR_HEIGHT - 1.0,
            color: colors::SURFACE1,
            width: 1.0,
        });
    }

    /// Render the main hex view.
    fn render_hex_view(&self, tree: &mut RenderTree) {
        let doc = self.active_doc();
        let content_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;
        let content_width = if self.show_inspector {
            self.window_width - INSPECTOR_WIDTH
        } else {
            self.window_width
        };
        let vis = self.visible_lines();
        let bpl = doc.view.bytes_per_line.value();
        let char_w = HEX_FONT_SIZE * CHAR_WIDTH_FACTOR;

        // Clip to hex view area.
        tree.push(RenderCommand::PushClip {
            x: 0.0,
            y: content_y,
            width: content_width,
            height: self.window_height - content_y - STATUS_BAR_HEIGHT,
        });

        for line_idx in 0..vis {
            let absolute_line = doc.view.scroll_offset.saturating_add(line_idx);
            let line_offset = absolute_line.saturating_mul(bpl);
            if line_offset >= doc.data.len() && !doc.data.is_empty() {
                break;
            }

            let y = content_y + (line_idx as f32) * LINE_HEIGHT;

            // Offset column.
            let offset_str = match doc.view.offset_display {
                OffsetDisplay::Hex => format!("{line_offset:08X}:"),
                OffsetDisplay::Decimal => format!("{line_offset:>10}:"),
            };
            tree.push(RenderCommand::Text {
                x: 4.0,
                y,
                text: offset_str,
                color: colors::OVERLAY0,
                font_size: HEX_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(char_w * 11.0),
            });

            // Hex bytes column.
            let hex_start_x = char_w * 11.0;
            let mut hex_x = hex_start_x;

            for col in 0..bpl {
                let offset = line_offset.saturating_add(col);
                if offset >= doc.data.len() {
                    // Pad with spaces for incomplete last line.
                    hex_x += char_w * 3.0;
                    continue;
                }

                let byte = doc.data.get(offset).copied().unwrap_or(0);

                // Determine background color for this byte.
                let bg_color = if doc.selection.as_ref().is_some_and(|s| s.contains(offset)) {
                    Some(colors::SURFACE1)
                } else if offset == doc.cursor {
                    Some(colors::SURFACE0)
                } else if doc.bookmarks.iter().any(|b| b.offset == offset) {
                    Some(Color::rgba(250, 179, 135, 60))
                } else { doc.highlight_color_at(offset).map(|hl| Color::rgba(hl.r, hl.g, hl.b, 40)) };

                if let Some(bg) = bg_color {
                    tree.push(RenderCommand::FillRect {
                        x: hex_x - 1.0,
                        y,
                        width: char_w * 2.5,
                        height: LINE_HEIGHT,
                        color: bg,
                        corner_radii: CornerRadii::all(2.0),
                    });
                }

                // Hex text color.
                let text_color = if offset == doc.cursor && doc.cursor_in_hex {
                    colors::BLUE
                } else if byte == 0 {
                    colors::OVERLAY0
                } else if byte.is_ascii_graphic() || byte == b' ' {
                    colors::TEXT
                } else {
                    colors::PEACH
                };

                tree.push(RenderCommand::Text {
                    x: hex_x,
                    y,
                    text: format!("{byte:02X}"),
                    color: text_color,
                    font_size: HEX_FONT_SIZE,
                    font_weight: if offset == doc.cursor {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(char_w * 3.0),
                });

                hex_x += char_w * 3.0;

                // Extra space every 8 bytes for readability.
                if (col.saturating_add(1)) % 8 == 0 && col.saturating_add(1) < bpl {
                    hex_x += char_w;
                }
            }

            // ASCII column.
            if doc.view.show_ascii {
                let ascii_start_x = hex_x + char_w * 2.0;

                // Separator line.
                tree.push(RenderCommand::Line {
                    x1: ascii_start_x - char_w,
                    y1: y,
                    x2: ascii_start_x - char_w,
                    y2: y + LINE_HEIGHT,
                    color: colors::SURFACE1,
                    width: 1.0,
                });

                for col in 0..bpl {
                    let offset = line_offset.saturating_add(col);
                    if offset >= doc.data.len() {
                        break;
                    }

                    let byte = doc.data.get(offset).copied().unwrap_or(0);
                    let ax = ascii_start_x + (col as f32) * char_w;

                    // Background highlight.
                    if doc.selection.as_ref().is_some_and(|s| s.contains(offset)) {
                        tree.push(RenderCommand::FillRect {
                            x: ax - 1.0,
                            y,
                            width: char_w + 1.0,
                            height: LINE_HEIGHT,
                            color: colors::SURFACE1,
                            corner_radii: CornerRadii::ZERO,
                        });
                    } else if offset == doc.cursor {
                        tree.push(RenderCommand::FillRect {
                            x: ax - 1.0,
                            y,
                            width: char_w + 1.0,
                            height: LINE_HEIGHT,
                            color: colors::SURFACE0,
                            corner_radii: CornerRadii::ZERO,
                        });
                    }

                    let ch = if byte.is_ascii_graphic() || byte == b' ' {
                        byte as char
                    } else {
                        '.'
                    };

                    let ascii_color = if offset == doc.cursor && !doc.cursor_in_hex {
                        colors::BLUE
                    } else if byte.is_ascii_graphic() || byte == b' ' {
                        colors::TEXT
                    } else {
                        colors::OVERLAY0
                    };

                    tree.push(RenderCommand::Text {
                        x: ax,
                        y,
                        text: String::from(ch),
                        color: ascii_color,
                        font_size: HEX_FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(char_w),
                    });
                }
            }
        }

        tree.push(RenderCommand::PopClip);
    }

    /// Render the data inspector panel.
    fn render_inspector(&self, tree: &mut RenderTree) {
        let doc = self.active_doc();
        let panel_x = self.window_width - INSPECTOR_WIDTH;
        let panel_y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;
        let panel_height = self.window_height - panel_y - STATUS_BAR_HEIGHT;

        // Panel background.
        tree.push(RenderCommand::FillRect {
            x: panel_x,
            y: panel_y,
            width: INSPECTOR_WIDTH,
            height: panel_height,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Left border.
        tree.push(RenderCommand::Line {
            x1: panel_x,
            y1: panel_y,
            x2: panel_x,
            y2: panel_y + panel_height,
            color: colors::SURFACE1,
            width: 1.0,
        });

        // Title.
        tree.push(RenderCommand::Text {
            x: panel_x + 8.0,
            y: panel_y + 6.0,
            text: String::from("Data Inspector"),
            color: colors::LAVENDER,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(INSPECTOR_WIDTH - 16.0),
        });

        // Data type values.
        let mut y = panel_y + 26.0;
        let label_x = panel_x + 8.0;
        let value_x = panel_x + 100.0;

        for &dtype in DataType::ALL {
            if y + LINE_HEIGHT > panel_y + panel_height {
                break;
            }

            tree.push(RenderCommand::Text {
                x: label_x,
                y,
                text: dtype.label().to_string(),
                color: colors::SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(90.0),
            });

            let value_str = doc
                .inspect_at(doc.cursor, dtype)
                .unwrap_or_else(|| String::from("--"));
            tree.push(RenderCommand::Text {
                x: value_x,
                y,
                text: value_str,
                color: colors::TEXT,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(INSPECTOR_WIDTH - 108.0),
            });

            y += 16.0;
        }
    }

    /// Render the status bar.
    fn render_status_bar(&self, tree: &mut RenderTree) {
        let doc = self.active_doc();
        let y = self.window_height - STATUS_BAR_HEIGHT;

        // Status bar background.
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: STATUS_BAR_HEIGHT,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top separator.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.window_width,
            y2: y,
            color: colors::SURFACE1,
            width: 1.0,
        });

        let text_y = y + 5.0;
        let _char_w = UI_FONT_SIZE * CHAR_WIDTH_FACTOR;

        // Offset (hex + decimal).
        let offset_hex = format!("0x{:08X}", doc.cursor);
        let offset_dec = format!("({})", doc.cursor);
        let offset_text = format!("Offset: {offset_hex} {offset_dec}");
        tree.push(RenderCommand::Text {
            x: 8.0,
            y: text_y,
            text: offset_text.clone(),
            color: colors::TEXT,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });

        // Selection size.
        let sel_x = 300.0;
        if let Some(sel) = &doc.selection {
            let sel_text = format!("Sel: {} bytes", sel.len());
            tree.push(RenderCommand::Text {
                x: sel_x,
                y: text_y,
                text: sel_text,
                color: colors::GREEN,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
            });
        }

        // File size.
        let size_text = format!("Size: {} bytes", doc.data.len());
        tree.push(RenderCommand::Text {
            x: self.window_width - 400.0,
            y: text_y,
            text: size_text,
            color: colors::SUBTEXT0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(140.0),
        });

        // Edit mode.
        let mode_text = match doc.edit_mode {
            EditMode::ReadOnly => "RO",
            EditMode::Insert => "INS",
            EditMode::Overwrite => "OVR",
        };
        let mode_color = match doc.edit_mode {
            EditMode::ReadOnly => colors::RED,
            EditMode::Insert => colors::GREEN,
            EditMode::Overwrite => colors::BLUE,
        };
        tree.push(RenderCommand::Text {
            x: self.window_width - 240.0,
            y: text_y,
            text: mode_text.to_string(),
            color: mode_color,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(40.0),
        });

        // Column indicator (hex/ASCII).
        let col_text = if doc.cursor_in_hex { "HEX" } else { "ASCII" };
        tree.push(RenderCommand::Text {
            x: self.window_width - 190.0,
            y: text_y,
            text: col_text.to_string(),
            color: colors::LAVENDER,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(50.0),
        });

        // Modified indicator.
        if doc.modified {
            tree.push(RenderCommand::Text {
                x: self.window_width - 120.0,
                y: text_y,
                text: String::from("Modified"),
                color: colors::YELLOW,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(70.0),
            });
        }

        // Encoding.
        tree.push(RenderCommand::Text {
            x: self.window_width - 50.0,
            y: text_y,
            text: String::from("Hex"),
            color: colors::SUBTEXT0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(40.0),
        });
    }

    /// Render the search bar overlay.
    fn render_search_bar(&self, tree: &mut RenderTree) {
        let bar_width: f32 = 400.0;
        let bar_height: f32 = 40.0;
        let x = self.window_width - bar_width - 20.0;
        let y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + 4.0;

        // Shadow.
        tree.push(RenderCommand::BoxShadow {
            x,
            y,
            width: bar_width,
            height: bar_height,
            offset_x: 0.0,
            offset_y: 2.0,
            blur: 8.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 80),
            corner_radii: CornerRadii::all(6.0),
        });

        // Background.
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: bar_width,
            height: bar_height,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: bar_width,
            height: bar_height,
            color: colors::BLUE,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Search icon placeholder.
        tree.push(RenderCommand::Text {
            x: x + 10.0,
            y: y + 12.0,
            text: String::from("Find:"),
            color: colors::SUBTEXT0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(40.0),
        });

        // Search input text.
        let input_text = if self.search.input_text.is_empty() {
            String::from("hex bytes or text...")
        } else {
            self.search.input_text.clone()
        };
        let input_color = if self.search.input_text.is_empty() {
            colors::OVERLAY0
        } else {
            colors::TEXT
        };
        tree.push(RenderCommand::Text {
            x: x + 50.0,
            y: y + 12.0,
            text: input_text,
            color: input_color,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(bar_width - 120.0),
        });

        // Match count.
        if self.search.match_count > 0 {
            tree.push(RenderCommand::Text {
                x: x + bar_width - 70.0,
                y: y + 12.0,
                text: format!("{} found", self.search.match_count),
                color: colors::GREEN,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(65.0),
            });
        }
    }

    /// Render the go-to-offset dialog overlay.
    fn render_goto_dialog(&self, tree: &mut RenderTree) {
        let dialog_width: f32 = 300.0;
        let dialog_height: f32 = 80.0;
        let x = (self.window_width - dialog_width) / 2.0;
        let y = (self.window_height - dialog_height) / 2.0;

        // Shadow.
        tree.push(RenderCommand::BoxShadow {
            x,
            y,
            width: dialog_width,
            height: dialog_height,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(8.0),
        });

        // Background.
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: dialog_width,
            height: dialog_height,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: dialog_width,
            height: dialog_height,
            color: colors::LAVENDER,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title.
        tree.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 12.0,
            text: String::from("Go to Offset"),
            color: colors::LAVENDER,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_width - 24.0),
        });

        // Input field background.
        tree.push(RenderCommand::FillRect {
            x: x + 12.0,
            y: y + 36.0,
            width: dialog_width - 24.0,
            height: 28.0,
            color: colors::BASE,
            corner_radii: CornerRadii::all(4.0),
        });

        tree.push(RenderCommand::StrokeRect {
            x: x + 12.0,
            y: y + 36.0,
            width: dialog_width - 24.0,
            height: 28.0,
            color: colors::SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Input text.
        let display_text = if self.goto_text.is_empty() {
            String::from("0x... or decimal")
        } else {
            self.goto_text.clone()
        };
        let text_color = if self.goto_text.is_empty() {
            colors::OVERLAY0
        } else {
            colors::TEXT
        };
        tree.push(RenderCommand::Text {
            x: x + 20.0,
            y: y + 42.0,
            text: display_text,
            color: text_color,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_width - 40.0),
        });
    }
}

// ============================================================================
// Hex string parsing utility
// ============================================================================

/// Parse a hex string like "FF 00 AB" or "FF00AB" into bytes.
pub fn parse_hex_string(s: &str) -> Option<Vec<u8>> {
    let cleaned: String = s.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if cleaned.is_empty() || !cleaned.len().is_multiple_of(2) {
        return None;
    }
    if !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = Vec::new();
    let chars: Vec<char> = cleaned.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let hi = chars.get(i)?;
        let lo = chars.get(i.saturating_add(1))?;
        let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16).ok()?;
        bytes.push(byte);
        i = i.saturating_add(2);
    }
    Some(bytes)
}

/// Format a byte as two hex characters.
pub fn byte_to_hex(b: u8) -> String {
    format!("{b:02X}")
}

/// Format an offset as an 8-character hex string.
pub fn format_offset_hex(offset: usize) -> String {
    format!("{offset:08X}")
}

/// Format an offset as a decimal string.
pub fn format_offset_dec(offset: usize) -> String {
    format!("{offset}")
}

/// Convert a byte to its ASCII display character (or '.' for non-printable).
pub fn byte_to_ascii_char(b: u8) -> char {
    if b.is_ascii_graphic() || b == b' ' {
        b as char
    } else {
        '.'
    }
}

/// Format a line of hex bytes for display.
pub fn format_hex_line(data: &[u8], offset: usize, bytes_per_line: usize) -> String {
    let mut hex_part = String::new();
    let mut ascii_part = String::new();

    for i in 0..bytes_per_line {
        if i < data.len() {
            let b = data.get(i).copied().unwrap_or(0);
            if !hex_part.is_empty() {
                hex_part.push(' ');
            }
            hex_part.push_str(&format!("{b:02X}"));
            ascii_part.push(byte_to_ascii_char(b));
        } else {
            if !hex_part.is_empty() {
                hex_part.push(' ');
            }
            hex_part.push_str("  ");
            ascii_part.push(' ');
        }

        // Extra space every 8 bytes.
        if (i.saturating_add(1)) % 8 == 0
            && i.saturating_add(1) < bytes_per_line
            && i < data.len()
        {
            hex_part.push(' ');
        }
    }

    format!("{:08X}: {hex_part}  {ascii_part}", offset)
}

// ============================================================================
// Application entry point
// ============================================================================

fn main() {
    // Create the hex editor with a sample document.
    let mut editor = HexEditor::new(1200.0, 800.0);

    // Open with sample data for demonstration.
    let sample: Vec<u8> = (0..=255).collect();
    let mut doc = HexDocument::from_data(sample);
    doc.file_path = Some(String::from("/demo/sample.bin"));
    editor.documents[0] = doc;

    // Render one frame.
    let _render_tree = editor.render();

    // Event loop placeholder (actual event loop is provided by the compositor).
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ====================================================================
    // Hex formatting utilities
    // ====================================================================

    #[test]
    fn test_byte_to_hex() {
        assert_eq!(byte_to_hex(0x00), "00");
        assert_eq!(byte_to_hex(0xFF), "FF");
        assert_eq!(byte_to_hex(0xAB), "AB");
        assert_eq!(byte_to_hex(0x0F), "0F");
        assert_eq!(byte_to_hex(0xF0), "F0");
    }

    #[test]
    fn test_format_offset_hex() {
        assert_eq!(format_offset_hex(0), "00000000");
        assert_eq!(format_offset_hex(256), "00000100");
        assert_eq!(format_offset_hex(0xDEADBEEF), "DEADBEEF");
    }

    #[test]
    fn test_format_offset_dec() {
        assert_eq!(format_offset_dec(0), "0");
        assert_eq!(format_offset_dec(1024), "1024");
    }

    #[test]
    fn test_byte_to_ascii_char() {
        assert_eq!(byte_to_ascii_char(b'A'), 'A');
        assert_eq!(byte_to_ascii_char(b' '), ' ');
        assert_eq!(byte_to_ascii_char(b'~'), '~');
        assert_eq!(byte_to_ascii_char(0x00), '.');
        assert_eq!(byte_to_ascii_char(0x7F), '.');
        assert_eq!(byte_to_ascii_char(0xFF), '.');
        assert_eq!(byte_to_ascii_char(0x01), '.');
    }

    #[test]
    fn test_parse_hex_string_spaced() {
        assert_eq!(
            parse_hex_string("FF 00 AB"),
            Some(vec![0xFF, 0x00, 0xAB])
        );
    }

    #[test]
    fn test_parse_hex_string_no_spaces() {
        assert_eq!(parse_hex_string("FF00AB"), Some(vec![0xFF, 0x00, 0xAB]));
    }

    #[test]
    fn test_parse_hex_string_empty() {
        assert_eq!(parse_hex_string(""), None);
    }

    #[test]
    fn test_parse_hex_string_odd_length() {
        assert_eq!(parse_hex_string("FFA"), None);
    }

    #[test]
    fn test_parse_hex_string_invalid_chars() {
        assert_eq!(parse_hex_string("GGFF"), None);
    }

    #[test]
    fn test_parse_hex_string_lowercase() {
        assert_eq!(parse_hex_string("ff00ab"), Some(vec![0xFF, 0x00, 0xAB]));
    }

    #[test]
    fn test_format_hex_line() {
        let data = vec![0x48, 0x65, 0x6C, 0x6C, 0x6F];
        let line = format_hex_line(&data, 0, 8);
        assert!(line.starts_with("00000000:"));
        assert!(line.contains("48"));
        assert!(line.contains("Hello"));
    }

    #[test]
    fn test_format_hex_line_full() {
        let data: Vec<u8> = (0..16).collect();
        let line = format_hex_line(&data, 0x100, 16);
        assert!(line.starts_with("00000100:"));
    }

    #[test]
    fn test_format_hex_line_partial() {
        let data = vec![0xAA, 0xBB];
        let line = format_hex_line(&data, 0, 8);
        assert!(line.contains("AA"));
        assert!(line.contains("BB"));
    }

    // ====================================================================
    // BytesPerLine
    // ====================================================================

    #[test]
    fn test_bytes_per_line_values() {
        assert_eq!(BytesPerLine::Eight.value(), 8);
        assert_eq!(BytesPerLine::Sixteen.value(), 16);
        assert_eq!(BytesPerLine::ThirtyTwo.value(), 32);
    }

    #[test]
    fn test_bytes_per_line_default() {
        assert_eq!(BytesPerLine::default(), BytesPerLine::Sixteen);
    }

    // ====================================================================
    // Selection
    // ====================================================================

    #[test]
    fn test_selection_new_ordered() {
        let sel = Selection::new(5, 10);
        assert_eq!(sel.start, 5);
        assert_eq!(sel.end, 10);
    }

    #[test]
    fn test_selection_new_reversed() {
        let sel = Selection::new(10, 5);
        assert_eq!(sel.start, 5);
        assert_eq!(sel.end, 10);
    }

    #[test]
    fn test_selection_len() {
        let sel = Selection::new(5, 10);
        assert_eq!(sel.len(), 6);
    }

    #[test]
    fn test_selection_len_single_byte() {
        let sel = Selection::new(5, 5);
        assert_eq!(sel.len(), 1);
    }

    #[test]
    fn test_selection_is_empty() {
        let sel = Selection::new(5, 5);
        assert!(sel.is_empty());
    }

    #[test]
    fn test_selection_not_empty() {
        let sel = Selection::new(5, 6);
        assert!(!sel.is_empty());
    }

    #[test]
    fn test_selection_contains() {
        let sel = Selection::new(5, 10);
        assert!(sel.contains(5));
        assert!(sel.contains(7));
        assert!(sel.contains(10));
        assert!(!sel.contains(4));
        assert!(!sel.contains(11));
    }

    #[test]
    fn test_selection_active_offset() {
        let sel = Selection::new(5, 10);
        assert_eq!(sel.active_offset(), 10);
    }

    #[test]
    fn test_selection_anchor_offset() {
        let sel = Selection::new(5, 10);
        assert_eq!(sel.anchor_offset(), 5);
    }

    // ====================================================================
    // DataType
    // ====================================================================

    #[test]
    fn test_data_type_all_count() {
        assert_eq!(DataType::ALL.len(), 20);
    }

    #[test]
    fn test_data_type_byte_count() {
        assert_eq!(DataType::U8.byte_count(), 1);
        assert_eq!(DataType::I8.byte_count(), 1);
        assert_eq!(DataType::U16Le.byte_count(), 2);
        assert_eq!(DataType::U32Le.byte_count(), 4);
        assert_eq!(DataType::U64Le.byte_count(), 8);
        assert_eq!(DataType::F32Le.byte_count(), 4);
        assert_eq!(DataType::F64Le.byte_count(), 8);
        assert_eq!(DataType::AsciiString.byte_count(), 0);
        assert_eq!(DataType::Utf8String.byte_count(), 0);
    }

    #[test]
    fn test_data_type_labels_unique() {
        let labels: Vec<&str> = DataType::ALL.iter().map(|d| d.label()).collect();
        for (i, label) in labels.iter().enumerate() {
            for (j, other) in labels.iter().enumerate() {
                if i != j {
                    assert_ne!(label, other, "Duplicate label");
                }
            }
        }
    }

    // ====================================================================
    // HexDocument — basic properties
    // ====================================================================

    #[test]
    fn test_document_new_empty() {
        let doc = HexDocument::new();
        assert!(doc.data.is_empty());
        assert!(!doc.modified);
        assert_eq!(doc.cursor, 0);
        assert!(doc.selection.is_none());
        assert!(doc.undo_stack.is_empty());
        assert!(doc.redo_stack.is_empty());
    }

    #[test]
    fn test_document_from_data() {
        let doc = HexDocument::from_data(vec![1, 2, 3]);
        assert_eq!(doc.data.len(), 3);
        assert!(!doc.modified);
    }

    #[test]
    fn test_document_from_file() {
        let doc = HexDocument::from_file("/test/foo.bin", vec![0xAA, 0xBB]);
        assert_eq!(doc.file_path, Some(String::from("/test/foo.bin")));
        assert_eq!(doc.data.len(), 2);
    }

    #[test]
    fn test_document_display_name_with_path() {
        let doc = HexDocument::from_file("/some/path/file.bin", vec![]);
        assert_eq!(doc.display_name(), "file.bin");
    }

    #[test]
    fn test_document_display_name_untitled() {
        let doc = HexDocument::new();
        assert_eq!(doc.display_name(), "Untitled");
    }

    // ====================================================================
    // HexDocument — line/column calculations
    // ====================================================================

    #[test]
    fn test_total_lines_empty() {
        let doc = HexDocument::new();
        assert_eq!(doc.total_lines(), 1);
    }

    #[test]
    fn test_total_lines() {
        let doc = HexDocument::from_data(vec![0; 48]);
        // 48 bytes / 16 bpl = 3 lines.
        assert_eq!(doc.total_lines(), 3);
    }

    #[test]
    fn test_total_lines_partial() {
        let doc = HexDocument::from_data(vec![0; 20]);
        // 20 bytes / 16 bpl = 2 lines (16 + 4).
        assert_eq!(doc.total_lines(), 2);
    }

    #[test]
    fn test_total_lines_exact() {
        let doc = HexDocument::from_data(vec![0; 16]);
        assert_eq!(doc.total_lines(), 1);
    }

    #[test]
    fn test_line_for_offset() {
        let doc = HexDocument::from_data(vec![0; 64]);
        assert_eq!(doc.line_for_offset(0), 0);
        assert_eq!(doc.line_for_offset(15), 0);
        assert_eq!(doc.line_for_offset(16), 1);
        assert_eq!(doc.line_for_offset(32), 2);
    }

    #[test]
    fn test_column_for_offset() {
        let doc = HexDocument::from_data(vec![0; 64]);
        assert_eq!(doc.column_for_offset(0), 0);
        assert_eq!(doc.column_for_offset(5), 5);
        assert_eq!(doc.column_for_offset(16), 0);
        assert_eq!(doc.column_for_offset(18), 2);
    }

    #[test]
    fn test_offset_for_line_col() {
        let doc = HexDocument::from_data(vec![0; 64]);
        assert_eq!(doc.offset_for_line_col(0, 0), 0);
        assert_eq!(doc.offset_for_line_col(1, 0), 16);
        assert_eq!(doc.offset_for_line_col(2, 5), 37);
    }

    // ====================================================================
    // HexDocument — editing: overwrite
    // ====================================================================

    #[test]
    fn test_overwrite_byte() {
        let mut doc = HexDocument::from_data(vec![0, 0, 0]);
        doc.overwrite_byte(1, 0xFF);
        assert_eq!(doc.data, vec![0, 0xFF, 0]);
        assert!(doc.modified);
    }

    #[test]
    fn test_overwrite_byte_out_of_range() {
        let mut doc = HexDocument::from_data(vec![0, 0]);
        doc.overwrite_byte(5, 0xFF);
        assert_eq!(doc.data, vec![0, 0]);
        assert!(!doc.modified);
    }

    #[test]
    fn test_overwrite_byte_same_value() {
        let mut doc = HexDocument::from_data(vec![0xAA]);
        doc.overwrite_byte(0, 0xAA);
        assert!(!doc.modified);
        assert!(doc.undo_stack.is_empty());
    }

    // ====================================================================
    // HexDocument — editing: insert
    // ====================================================================

    #[test]
    fn test_insert_byte() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xCC]);
        doc.insert_byte(1, 0xBB);
        assert_eq!(doc.data, vec![0xAA, 0xBB, 0xCC]);
        assert!(doc.modified);
    }

    #[test]
    fn test_insert_byte_at_end() {
        let mut doc = HexDocument::from_data(vec![0xAA]);
        doc.insert_byte(1, 0xBB);
        assert_eq!(doc.data, vec![0xAA, 0xBB]);
    }

    #[test]
    fn test_insert_byte_beyond_end() {
        let mut doc = HexDocument::from_data(vec![0xAA]);
        doc.insert_byte(100, 0xBB);
        // Inserting beyond end should insert at the end.
        assert_eq!(doc.data, vec![0xAA, 0xBB]);
    }

    // ====================================================================
    // HexDocument — editing: delete
    // ====================================================================

    #[test]
    fn test_delete_byte() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC]);
        doc.delete_byte(1);
        assert_eq!(doc.data, vec![0xAA, 0xCC]);
        assert!(doc.modified);
    }

    #[test]
    fn test_delete_byte_out_of_range() {
        let mut doc = HexDocument::from_data(vec![0xAA]);
        doc.delete_byte(5);
        assert_eq!(doc.data, vec![0xAA]);
        assert!(!doc.modified);
    }

    #[test]
    fn test_delete_range() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC, 0xDD]);
        doc.delete_range(1, 2);
        assert_eq!(doc.data, vec![0xAA, 0xDD]);
    }

    #[test]
    fn test_delete_range_entire() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC]);
        doc.delete_range(0, 2);
        assert!(doc.data.is_empty());
    }

    // ====================================================================
    // HexDocument — editing: replace range
    // ====================================================================

    #[test]
    fn test_replace_range() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC, 0xDD]);
        doc.replace_range(1, 2, &[0x11, 0x22, 0x33]);
        assert_eq!(doc.data, vec![0xAA, 0x11, 0x22, 0x33, 0xDD]);
    }

    #[test]
    fn test_replace_range_shrink() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC, 0xDD]);
        doc.replace_range(1, 2, &[0xFF]);
        assert_eq!(doc.data, vec![0xAA, 0xFF, 0xDD]);
    }

    // ====================================================================
    // HexDocument — undo / redo
    // ====================================================================

    #[test]
    fn test_undo_overwrite() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC]);
        doc.overwrite_byte(1, 0xFF);
        assert_eq!(doc.data[1], 0xFF);
        assert!(doc.undo());
        assert_eq!(doc.data[1], 0xBB);
    }

    #[test]
    fn test_redo_overwrite() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC]);
        doc.overwrite_byte(1, 0xFF);
        doc.undo();
        assert_eq!(doc.data[1], 0xBB);
        assert!(doc.redo());
        assert_eq!(doc.data[1], 0xFF);
    }

    #[test]
    fn test_undo_insert() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xCC]);
        doc.insert_byte(1, 0xBB);
        assert_eq!(doc.data.len(), 3);
        assert!(doc.undo());
        assert_eq!(doc.data, vec![0xAA, 0xCC]);
    }

    #[test]
    fn test_undo_delete() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC]);
        doc.delete_byte(1);
        assert_eq!(doc.data, vec![0xAA, 0xCC]);
        assert!(doc.undo());
        assert_eq!(doc.data, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn test_undo_empty_stack() {
        let mut doc = HexDocument::new();
        assert!(!doc.undo());
    }

    #[test]
    fn test_redo_empty_stack() {
        let mut doc = HexDocument::new();
        assert!(!doc.redo());
    }

    #[test]
    fn test_redo_cleared_on_new_edit() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB]);
        doc.overwrite_byte(0, 0x11);
        doc.undo();
        assert!(!doc.redo_stack.is_empty());
        doc.overwrite_byte(0, 0x22);
        assert!(doc.redo_stack.is_empty());
    }

    #[test]
    fn test_multiple_undo_redo() {
        let mut doc = HexDocument::from_data(vec![0x00]);
        doc.overwrite_byte(0, 0x11);
        doc.overwrite_byte(0, 0x22);
        doc.overwrite_byte(0, 0x33);
        assert_eq!(doc.data[0], 0x33);

        assert!(doc.undo());
        assert_eq!(doc.data[0], 0x22);
        assert!(doc.undo());
        assert_eq!(doc.data[0], 0x11);
        assert!(doc.undo());
        assert_eq!(doc.data[0], 0x00);

        assert!(doc.redo());
        assert_eq!(doc.data[0], 0x11);
    }

    // ====================================================================
    // HexDocument — search
    // ====================================================================

    #[test]
    fn test_find_hex_bytes() {
        let doc = HexDocument::from_data(vec![0x00, 0xFF, 0xAB, 0xCD, 0xFF, 0xAB]);
        let query = SearchQuery {
            pattern: SearchPattern::HexBytes(vec![0xFF, 0xAB]),
            direction: SearchDirection::Forward,
            case_sensitive: true,
            wrap_around: false,
        };
        assert_eq!(doc.find_next(&query, 0), Some(1));
    }

    #[test]
    fn test_find_hex_bytes_second() {
        let doc = HexDocument::from_data(vec![0x00, 0xFF, 0xAB, 0xCD, 0xFF, 0xAB]);
        let query = SearchQuery {
            pattern: SearchPattern::HexBytes(vec![0xFF, 0xAB]),
            direction: SearchDirection::Forward,
            case_sensitive: true,
            wrap_around: false,
        };
        assert_eq!(doc.find_next(&query, 2), Some(4));
    }

    #[test]
    fn test_find_ascii() {
        let data = b"Hello World".to_vec();
        let doc = HexDocument::from_data(data);
        let query = SearchQuery {
            pattern: SearchPattern::AsciiText(String::from("World")),
            direction: SearchDirection::Forward,
            case_sensitive: true,
            wrap_around: false,
        };
        assert_eq!(doc.find_next(&query, 0), Some(6));
    }

    #[test]
    fn test_find_ascii_case_insensitive() {
        let data = b"Hello WORLD".to_vec();
        let doc = HexDocument::from_data(data);
        let query = SearchQuery {
            pattern: SearchPattern::AsciiText(String::from("world")),
            direction: SearchDirection::Forward,
            case_sensitive: false,
            wrap_around: false,
        };
        assert_eq!(doc.find_next(&query, 0), Some(6));
    }

    #[test]
    fn test_find_not_found() {
        let doc = HexDocument::from_data(vec![0x00, 0x11, 0x22]);
        let query = SearchQuery {
            pattern: SearchPattern::HexBytes(vec![0xFF, 0xFE]),
            direction: SearchDirection::Forward,
            case_sensitive: true,
            wrap_around: false,
        };
        assert_eq!(doc.find_next(&query, 0), None);
    }

    #[test]
    fn test_find_wrap_around() {
        let doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC, 0xDD]);
        let query = SearchQuery {
            pattern: SearchPattern::HexBytes(vec![0xAA]),
            direction: SearchDirection::Forward,
            case_sensitive: true,
            wrap_around: true,
        };
        // Search from offset 2, should wrap and find at 0.
        assert_eq!(doc.find_next(&query, 2), Some(0));
    }

    #[test]
    fn test_find_backward() {
        let doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xAA, 0xCC]);
        let query = SearchQuery {
            pattern: SearchPattern::HexBytes(vec![0xAA]),
            direction: SearchDirection::Backward,
            case_sensitive: true,
            wrap_around: false,
        };
        assert_eq!(doc.find_next(&query, 3), Some(2));
    }

    #[test]
    fn test_count_matches() {
        let doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xAA, 0xCC, 0xAA]);
        let query = SearchQuery {
            pattern: SearchPattern::HexBytes(vec![0xAA]),
            direction: SearchDirection::Forward,
            case_sensitive: true,
            wrap_around: false,
        };
        assert_eq!(doc.count_matches(&query), 3);
    }

    #[test]
    fn test_count_matches_empty_pattern() {
        let doc = HexDocument::from_data(vec![0xAA]);
        let query = SearchQuery {
            pattern: SearchPattern::HexBytes(vec![]),
            direction: SearchDirection::Forward,
            case_sensitive: true,
            wrap_around: false,
        };
        assert_eq!(doc.count_matches(&query), 0);
    }

    // ====================================================================
    // HexDocument — bookmarks
    // ====================================================================

    #[test]
    fn test_add_bookmark() {
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(10, "test", colors::YELLOW);
        assert_eq!(doc.bookmarks.len(), 1);
        assert_eq!(doc.bookmarks[0].offset, 10);
        assert_eq!(doc.bookmarks[0].label, "test");
    }

    #[test]
    fn test_add_bookmark_no_duplicates() {
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(10, "first", colors::YELLOW);
        doc.add_bookmark(10, "second", colors::BLUE);
        assert_eq!(doc.bookmarks.len(), 1);
    }

    #[test]
    fn test_add_bookmark_sorted() {
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(50, "b", colors::YELLOW);
        doc.add_bookmark(10, "a", colors::BLUE);
        doc.add_bookmark(30, "c", colors::GREEN);
        assert_eq!(doc.bookmarks[0].offset, 10);
        assert_eq!(doc.bookmarks[1].offset, 30);
        assert_eq!(doc.bookmarks[2].offset, 50);
    }

    #[test]
    fn test_remove_bookmark() {
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(10, "test", colors::YELLOW);
        doc.remove_bookmark(10);
        assert!(doc.bookmarks.is_empty());
    }

    #[test]
    fn test_toggle_bookmark() {
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.toggle_bookmark(10);
        assert_eq!(doc.bookmarks.len(), 1);
        doc.toggle_bookmark(10);
        assert!(doc.bookmarks.is_empty());
    }

    #[test]
    fn test_next_bookmark() {
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(10, "a", colors::YELLOW);
        doc.add_bookmark(30, "b", colors::BLUE);
        doc.cursor = 0;
        assert_eq!(doc.next_bookmark(), Some(10));
        doc.cursor = 15;
        assert_eq!(doc.next_bookmark(), Some(30));
    }

    #[test]
    fn test_next_bookmark_wraps() {
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(10, "a", colors::YELLOW);
        doc.cursor = 50;
        assert_eq!(doc.next_bookmark(), Some(10));
    }

    #[test]
    fn test_prev_bookmark() {
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(10, "a", colors::YELLOW);
        doc.add_bookmark(30, "b", colors::BLUE);
        doc.cursor = 50;
        assert_eq!(doc.prev_bookmark(), Some(30));
    }

    // ====================================================================
    // HexDocument — data inspector
    // ====================================================================

    #[test]
    fn test_inspect_u8() {
        let doc = HexDocument::from_data(vec![42]);
        assert_eq!(doc.inspect_at(0, DataType::U8), Some(String::from("42")));
    }

    #[test]
    fn test_inspect_i8() {
        let doc = HexDocument::from_data(vec![0xFF]);
        assert_eq!(doc.inspect_at(0, DataType::I8), Some(String::from("-1")));
    }

    #[test]
    fn test_inspect_u16_le() {
        let doc = HexDocument::from_data(vec![0x01, 0x00]);
        assert_eq!(doc.inspect_at(0, DataType::U16Le), Some(String::from("1")));
    }

    #[test]
    fn test_inspect_u16_be() {
        let doc = HexDocument::from_data(vec![0x00, 0x01]);
        assert_eq!(doc.inspect_at(0, DataType::U16Be), Some(String::from("1")));
    }

    #[test]
    fn test_inspect_u32_le() {
        let doc = HexDocument::from_data(vec![0x78, 0x56, 0x34, 0x12]);
        assert_eq!(
            doc.inspect_at(0, DataType::U32Le),
            Some(String::from("305419896"))
        );
    }

    #[test]
    fn test_inspect_i32_le() {
        let doc = HexDocument::from_data(vec![0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(doc.inspect_at(0, DataType::I32Le), Some(String::from("-1")));
    }

    #[test]
    fn test_inspect_u64_le() {
        let doc = HexDocument::from_data(vec![0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        assert_eq!(doc.inspect_at(0, DataType::U64Le), Some(String::from("1")));
    }

    #[test]
    fn test_inspect_f32_le() {
        let val: f32 = 3.25;
        let bytes = val.to_le_bytes();
        let doc = HexDocument::from_data(bytes.to_vec());
        let result = doc.inspect_at(0, DataType::F32Le);
        assert!(result.is_some());
        let parsed: f32 = result.unwrap().parse().unwrap();
        assert!((parsed - 3.25).abs() < 0.01);
    }

    #[test]
    fn test_inspect_f64_le() {
        let val: f64 = 1.234_567_89;
        let bytes = val.to_le_bytes();
        let doc = HexDocument::from_data(bytes.to_vec());
        let result = doc.inspect_at(0, DataType::F64Le);
        assert!(result.is_some());
    }

    #[test]
    fn test_inspect_insufficient_bytes() {
        let doc = HexDocument::from_data(vec![0x01]);
        assert_eq!(doc.inspect_at(0, DataType::U32Le), None);
    }

    #[test]
    fn test_inspect_ascii_string() {
        let data = b"Hello\x00World".to_vec();
        let doc = HexDocument::from_data(data);
        assert_eq!(
            doc.inspect_at(0, DataType::AsciiString),
            Some(String::from("Hello"))
        );
    }

    #[test]
    fn test_inspect_utf8_string() {
        let data = b"Test\x00Rest".to_vec();
        let doc = HexDocument::from_data(data);
        assert_eq!(
            doc.inspect_at(0, DataType::Utf8String),
            Some(String::from("Test"))
        );
    }

    #[test]
    fn test_inspect_out_of_range() {
        let doc = HexDocument::from_data(vec![0x01]);
        assert_eq!(doc.inspect_at(5, DataType::U8), None);
    }

    // ====================================================================
    // HexDocument — byte frequency
    // ====================================================================

    #[test]
    fn test_byte_frequency_empty() {
        let doc = HexDocument::new();
        let freq = doc.byte_frequency();
        assert!(freq.iter().all(|&c| c == 0));
    }

    #[test]
    fn test_byte_frequency() {
        let doc = HexDocument::from_data(vec![0x00, 0x00, 0xFF, 0x00, 0xFF]);
        let freq = doc.byte_frequency();
        assert_eq!(freq[0x00], 3);
        assert_eq!(freq[0xFF], 2);
        assert_eq!(freq[0x01], 0);
    }

    #[test]
    fn test_most_frequent_byte_empty() {
        let doc = HexDocument::new();
        assert_eq!(doc.most_frequent_byte(), None);
    }

    #[test]
    fn test_most_frequent_byte() {
        let doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xAA, 0xCC, 0xAA]);
        let (byte, count) = doc.most_frequent_byte().unwrap();
        assert_eq!(byte, 0xAA);
        assert_eq!(count, 3);
    }

    // ====================================================================
    // HexDocument — copy/paste
    // ====================================================================

    #[test]
    fn test_copy_as_hex() {
        let doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC]);
        // No selection: copy byte at cursor.
        assert_eq!(doc.copy_as_hex(), "AA");
    }

    #[test]
    fn test_copy_as_hex_with_selection() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC]);
        doc.selection = Some(Selection::new(0, 2));
        assert_eq!(doc.copy_as_hex(), "AA BB CC");
    }

    #[test]
    fn test_copy_as_bytes() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC]);
        doc.selection = Some(Selection::new(1, 2));
        assert_eq!(doc.copy_as_bytes(), vec![0xBB, 0xCC]);
    }

    #[test]
    fn test_paste_hex() {
        let mut doc = HexDocument::from_data(vec![0x00, 0x00, 0x00]);
        doc.edit_mode = EditMode::Overwrite;
        doc.cursor = 0;
        assert!(doc.paste_hex("AABB"));
        assert_eq!(doc.data[0], 0xAA);
        assert_eq!(doc.data[1], 0xBB);
    }

    #[test]
    fn test_paste_hex_invalid() {
        let mut doc = HexDocument::from_data(vec![0x00]);
        assert!(!doc.paste_hex("GGXX"));
    }

    #[test]
    fn test_paste_bytes_insert_mode() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xCC]);
        doc.edit_mode = EditMode::Insert;
        doc.cursor = 1;
        doc.paste_bytes(&[0xBB]);
        assert_eq!(doc.data, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn test_paste_bytes_readonly() {
        let mut doc = HexDocument::from_data(vec![0xAA]);
        doc.edit_mode = EditMode::ReadOnly;
        doc.paste_bytes(&[0xFF]);
        assert_eq!(doc.data, vec![0xAA]);
    }

    // ====================================================================
    // HexDocument — highlight patterns
    // ====================================================================

    #[test]
    fn test_highlight_color_at_match() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC, 0xDD]);
        doc.highlights.push(HighlightPattern {
            pattern: vec![0xBB, 0xCC],
            color: colors::RED,
            label: String::from("test"),
            enabled: true,
        });
        assert_eq!(doc.highlight_color_at(1), Some(colors::RED));
        assert_eq!(doc.highlight_color_at(2), Some(colors::RED));
        assert_eq!(doc.highlight_color_at(0), None);
        assert_eq!(doc.highlight_color_at(3), None);
    }

    #[test]
    fn test_highlight_disabled() {
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB]);
        doc.highlights.push(HighlightPattern {
            pattern: vec![0xAA],
            color: colors::RED,
            label: String::from("off"),
            enabled: false,
        });
        assert_eq!(doc.highlight_color_at(0), None);
    }

    // ====================================================================
    // HexDocument — structure templates
    // ====================================================================

    #[test]
    fn test_struct_template_new() {
        let tmpl = StructTemplate::new("ELF Header");
        assert_eq!(tmpl.name, "ELF Header");
        assert!(tmpl.fields.is_empty());
    }

    #[test]
    fn test_struct_template_add_field() {
        let mut tmpl = StructTemplate::new("Test");
        tmpl.add_field(0, "magic", DataType::U32Le);
        tmpl.add_field(4, "version", DataType::U16Le);
        assert_eq!(tmpl.fields.len(), 2);
    }

    #[test]
    fn test_struct_template_total_size() {
        let mut tmpl = StructTemplate::new("Test");
        tmpl.add_field(0, "magic", DataType::U32Le);
        tmpl.add_field(4, "version", DataType::U16Le);
        assert_eq!(tmpl.total_size(), 6);
    }

    #[test]
    fn test_struct_template_total_size_empty() {
        let tmpl = StructTemplate::new("Empty");
        assert_eq!(tmpl.total_size(), 0);
    }

    // ====================================================================
    // HexEditor — multi-tab
    // ====================================================================

    #[test]
    fn test_editor_new_has_one_tab() {
        let editor = HexEditor::new(800.0, 600.0);
        assert_eq!(editor.documents.len(), 1);
        assert_eq!(editor.active_tab, 0);
    }

    #[test]
    fn test_open_tab() {
        let mut editor = HexEditor::new(800.0, 600.0);
        editor.open_tab(HexDocument::from_data(vec![0xFF]));
        assert_eq!(editor.documents.len(), 2);
        assert_eq!(editor.active_tab, 1);
    }

    #[test]
    fn test_close_tab_last_becomes_empty() {
        let mut editor = HexEditor::new(800.0, 600.0);
        editor.close_tab(0);
        assert_eq!(editor.documents.len(), 1);
        assert!(editor.documents[0].data.is_empty());
    }

    #[test]
    fn test_close_tab_multiple() {
        let mut editor = HexEditor::new(800.0, 600.0);
        editor.open_tab(HexDocument::from_data(vec![0xAA]));
        editor.open_tab(HexDocument::from_data(vec![0xBB]));
        assert_eq!(editor.documents.len(), 3);
        editor.close_tab(1);
        assert_eq!(editor.documents.len(), 2);
    }

    #[test]
    fn test_next_tab() {
        let mut editor = HexEditor::new(800.0, 600.0);
        editor.open_tab(HexDocument::from_data(vec![0xAA]));
        editor.open_tab(HexDocument::from_data(vec![0xBB]));
        editor.active_tab = 0;
        editor.next_tab();
        assert_eq!(editor.active_tab, 1);
        editor.next_tab();
        assert_eq!(editor.active_tab, 2);
        editor.next_tab();
        assert_eq!(editor.active_tab, 0); // Wrap around.
    }

    #[test]
    fn test_prev_tab() {
        let mut editor = HexEditor::new(800.0, 600.0);
        editor.open_tab(HexDocument::from_data(vec![0xAA]));
        editor.active_tab = 0;
        editor.prev_tab();
        assert_eq!(editor.active_tab, 1); // Wrap around.
    }

    // ====================================================================
    // HexEditor — recent files
    // ====================================================================

    #[test]
    fn test_add_recent_file() {
        let mut editor = HexEditor::new(800.0, 600.0);
        editor.add_recent_file("/path/a.bin");
        editor.add_recent_file("/path/b.bin");
        assert_eq!(editor.recent_files.len(), 2);
        assert_eq!(editor.recent_files[0], "/path/b.bin");
    }

    #[test]
    fn test_add_recent_file_no_duplicates() {
        let mut editor = HexEditor::new(800.0, 600.0);
        editor.add_recent_file("/path/a.bin");
        editor.add_recent_file("/path/b.bin");
        editor.add_recent_file("/path/a.bin");
        assert_eq!(editor.recent_files.len(), 2);
        assert_eq!(editor.recent_files[0], "/path/a.bin");
    }

    #[test]
    fn test_recent_files_max() {
        let mut editor = HexEditor::new(800.0, 600.0);
        for i in 0..30 {
            editor.add_recent_file(&format!("/path/{i}.bin"));
        }
        assert_eq!(editor.recent_files.len(), MAX_RECENT_FILES);
    }

    // ====================================================================
    // HexEditor — cursor navigation
    // ====================================================================

    fn make_test_editor(data: Vec<u8>) -> HexEditor {
        let mut editor = HexEditor::new(1200.0, 800.0);
        editor.documents[0] = HexDocument::from_data(data);
        editor
    }

    fn key_press(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: None,
        }
    }

    #[test]
    fn test_cursor_right() {
        let mut editor = make_test_editor(vec![0; 32]);
        assert_eq!(editor.active_doc().cursor, 0);
        editor.handle_key(&key_press(Key::Right, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 1);
    }

    #[test]
    fn test_cursor_left() {
        let mut editor = make_test_editor(vec![0; 32]);
        editor.active_doc_mut().cursor = 5;
        editor.handle_key(&key_press(Key::Left, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 4);
    }

    #[test]
    fn test_cursor_left_at_start() {
        let mut editor = make_test_editor(vec![0; 32]);
        editor.handle_key(&key_press(Key::Left, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 0);
    }

    #[test]
    fn test_cursor_down() {
        let mut editor = make_test_editor(vec![0; 64]);
        editor.handle_key(&key_press(Key::Down, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 16);
    }

    #[test]
    fn test_cursor_up() {
        let mut editor = make_test_editor(vec![0; 64]);
        editor.active_doc_mut().cursor = 20;
        editor.handle_key(&key_press(Key::Up, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 4);
    }

    #[test]
    fn test_cursor_home() {
        let mut editor = make_test_editor(vec![0; 64]);
        editor.active_doc_mut().cursor = 21; // Line 1, col 5.
        editor.handle_key(&key_press(Key::Home, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 16); // Start of line 1.
    }

    #[test]
    fn test_cursor_end() {
        let mut editor = make_test_editor(vec![0; 64]);
        editor.active_doc_mut().cursor = 0;
        editor.handle_key(&key_press(Key::End, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 15); // End of line 0.
    }

    #[test]
    fn test_cursor_ctrl_home() {
        let mut editor = make_test_editor(vec![0; 64]);
        editor.active_doc_mut().cursor = 50;
        editor.handle_key(&key_press(Key::Home, Modifiers::ctrl()));
        assert_eq!(editor.active_doc().cursor, 0);
    }

    #[test]
    fn test_cursor_ctrl_end() {
        let mut editor = make_test_editor(vec![0; 64]);
        editor.handle_key(&key_press(Key::End, Modifiers::ctrl()));
        assert_eq!(editor.active_doc().cursor, 63);
    }

    // ====================================================================
    // HexEditor — selection
    // ====================================================================

    #[test]
    fn test_shift_right_creates_selection() {
        let mut editor = make_test_editor(vec![0; 32]);
        editor.active_doc_mut().cursor = 5;
        editor.handle_key(&key_press(Key::Right, Modifiers::shift()));
        let sel = editor.active_doc().selection.as_ref().unwrap();
        assert_eq!(sel.start, 5);
        assert_eq!(sel.end, 6);
    }

    #[test]
    fn test_shift_extends_selection() {
        let mut editor = make_test_editor(vec![0; 32]);
        editor.active_doc_mut().cursor = 5;
        editor.handle_key(&key_press(Key::Right, Modifiers::shift()));
        editor.handle_key(&key_press(Key::Right, Modifiers::shift()));
        let sel = editor.active_doc().selection.as_ref().unwrap();
        assert_eq!(sel.start, 5);
        assert_eq!(sel.end, 7);
    }

    #[test]
    fn test_move_without_shift_clears_selection() {
        let mut editor = make_test_editor(vec![0; 32]);
        editor.active_doc_mut().cursor = 5;
        editor.handle_key(&key_press(Key::Right, Modifiers::shift()));
        editor.handle_key(&key_press(Key::Right, Modifiers::NONE));
        assert!(editor.active_doc().selection.is_none());
    }

    #[test]
    fn test_escape_clears_selection() {
        let mut editor = make_test_editor(vec![0; 32]);
        editor.active_doc_mut().selection = Some(Selection::new(0, 5));
        editor.handle_key(&key_press(Key::Escape, Modifiers::NONE));
        assert!(editor.active_doc().selection.is_none());
    }

    // ====================================================================
    // HexEditor — edit modes
    // ====================================================================

    #[test]
    fn test_toggle_insert_overwrite() {
        let mut editor = make_test_editor(vec![0; 16]);
        assert_eq!(editor.active_doc().edit_mode, EditMode::Overwrite);
        editor.handle_key(&key_press(Key::Insert, Modifiers::NONE));
        assert_eq!(editor.active_doc().edit_mode, EditMode::Insert);
        editor.handle_key(&key_press(Key::Insert, Modifiers::NONE));
        assert_eq!(editor.active_doc().edit_mode, EditMode::Overwrite);
    }

    #[test]
    fn test_toggle_hex_ascii() {
        let mut editor = make_test_editor(vec![0; 16]);
        assert!(editor.active_doc().cursor_in_hex);
        editor.handle_key(&key_press(Key::Tab, Modifiers::NONE));
        assert!(!editor.active_doc().cursor_in_hex);
        editor.handle_key(&key_press(Key::Tab, Modifiers::NONE));
        assert!(editor.active_doc().cursor_in_hex);
    }

    // ====================================================================
    // HexEditor — keyboard shortcuts
    // ====================================================================

    #[test]
    fn test_ctrl_z_undo() {
        let mut editor = make_test_editor(vec![0xAA]);
        editor.active_doc_mut().overwrite_byte(0, 0xFF);
        assert_eq!(editor.active_doc().data[0], 0xFF);
        editor.handle_key(&key_press(Key::Z, Modifiers::ctrl()));
        assert_eq!(editor.active_doc().data[0], 0xAA);
    }

    #[test]
    fn test_ctrl_y_redo() {
        let mut editor = make_test_editor(vec![0xAA]);
        editor.active_doc_mut().overwrite_byte(0, 0xFF);
        editor.active_doc_mut().undo();
        editor.handle_key(&key_press(Key::Y, Modifiers::ctrl()));
        assert_eq!(editor.active_doc().data[0], 0xFF);
    }

    #[test]
    fn test_ctrl_f_toggle_search() {
        let mut editor = make_test_editor(vec![0; 16]);
        assert!(!editor.search.visible);
        editor.handle_key(&key_press(Key::F, Modifiers::ctrl()));
        assert!(editor.search.visible);
        assert_eq!(editor.focused_panel, FocusedPanel::SearchBar);
        editor.handle_key(&key_press(Key::F, Modifiers::ctrl()));
        assert!(!editor.search.visible);
    }

    #[test]
    fn test_ctrl_g_toggle_goto() {
        let mut editor = make_test_editor(vec![0; 16]);
        assert!(!editor.goto_visible);
        editor.handle_key(&key_press(Key::G, Modifiers::ctrl()));
        assert!(editor.goto_visible);
        assert_eq!(editor.focused_panel, FocusedPanel::GoToDialog);
    }

    #[test]
    fn test_ctrl_b_toggle_bookmark() {
        let mut editor = make_test_editor(vec![0; 16]);
        editor.active_doc_mut().cursor = 5;
        editor.handle_key(&key_press(Key::B, Modifiers::ctrl()));
        assert_eq!(editor.active_doc().bookmarks.len(), 1);
        editor.handle_key(&key_press(Key::B, Modifiers::ctrl()));
        assert!(editor.active_doc().bookmarks.is_empty());
    }

    // ====================================================================
    // HexEditor — rendering
    // ====================================================================

    #[test]
    fn test_render_produces_commands() {
        let editor = make_test_editor(vec![0; 256]);
        let tree = editor.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_empty_document() {
        let editor = HexEditor::new(800.0, 600.0);
        let tree = editor.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_search_bar() {
        let mut editor = make_test_editor(vec![0; 16]);
        editor.search.visible = true;
        let tree = editor.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_goto_dialog() {
        let mut editor = make_test_editor(vec![0; 16]);
        editor.goto_visible = true;
        let tree = editor.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_inspector() {
        let mut editor = make_test_editor(vec![0; 16]);
        editor.show_inspector = true;
        let tree = editor.render();
        assert!(!tree.is_empty());
    }

    // ====================================================================
    // HexEditor — clamp and scroll
    // ====================================================================

    #[test]
    fn test_clamp_cursor_empty() {
        let mut doc = HexDocument::new();
        doc.cursor = 100;
        doc.clamp_cursor();
        assert_eq!(doc.cursor, 0);
    }

    #[test]
    fn test_clamp_cursor_end() {
        let mut doc = HexDocument::from_data(vec![0; 10]);
        doc.cursor = 20;
        doc.clamp_cursor();
        assert_eq!(doc.cursor, 9);
    }

    #[test]
    fn test_ensure_cursor_visible_scroll_down() {
        let mut doc = HexDocument::from_data(vec![0; 1024]);
        doc.cursor = 512; // Line 32.
        doc.view.scroll_offset = 0;
        doc.ensure_cursor_visible(20);
        assert!(doc.view.scroll_offset > 0);
    }

    #[test]
    fn test_ensure_cursor_visible_scroll_up() {
        let mut doc = HexDocument::from_data(vec![0; 1024]);
        doc.cursor = 0;
        doc.view.scroll_offset = 10;
        doc.ensure_cursor_visible(20);
        assert_eq!(doc.view.scroll_offset, 0);
    }

    // ====================================================================
    // HexDocument — HexView defaults
    // ====================================================================

    #[test]
    fn test_hex_view_defaults() {
        let view = HexView::default();
        assert_eq!(view.bytes_per_line, BytesPerLine::Sixteen);
        assert_eq!(view.offset_display, OffsetDisplay::Hex);
        assert!(view.show_ascii);
        assert!(view.show_inspector);
        assert_eq!(view.scroll_offset, 0);
    }

    // ====================================================================
    // SearchQuery defaults
    // ====================================================================

    #[test]
    fn test_search_query_defaults() {
        let q = SearchQuery::default();
        assert!(q.case_sensitive);
        assert!(q.wrap_around);
        assert_eq!(q.direction, SearchDirection::Forward);
    }

    // ====================================================================
    // EditMode defaults
    // ====================================================================

    #[test]
    fn test_edit_mode_default() {
        assert_eq!(EditMode::default(), EditMode::Overwrite);
    }

    // ====================================================================
    // Color constants
    // ====================================================================

    #[test]
    fn test_catppuccin_colors() {
        assert_eq!(colors::BASE, Color::from_hex(0x1E1E2E));
        assert_eq!(colors::MANTLE, Color::from_hex(0x181825));
        assert_eq!(colors::SURFACE0, Color::from_hex(0x313244));
        assert_eq!(colors::TEXT, Color::from_hex(0xCDD6F4));
        assert_eq!(colors::BLUE, Color::from_hex(0x89B4FA));
        assert_eq!(colors::RED, Color::from_hex(0xF38BA8));
    }

    // ====================================================================
    // HexEditor — go to offset
    // ====================================================================

    #[test]
    fn test_goto_hex_offset() {
        let mut editor = make_test_editor(vec![0; 1024]);
        editor.goto_visible = true;
        editor.focused_panel = FocusedPanel::GoToDialog;
        editor.goto_text = String::from("0x100");
        editor.handle_key(&key_press(Key::Enter, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 0x100);
        assert!(!editor.goto_visible);
    }

    #[test]
    fn test_goto_decimal_offset() {
        let mut editor = make_test_editor(vec![0; 1024]);
        editor.goto_visible = true;
        editor.focused_panel = FocusedPanel::GoToDialog;
        editor.goto_text = String::from("256");
        editor.handle_key(&key_press(Key::Enter, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 256);
    }

    #[test]
    fn test_goto_dollar_hex_offset() {
        let mut editor = make_test_editor(vec![0; 1024]);
        editor.goto_visible = true;
        editor.focused_panel = FocusedPanel::GoToDialog;
        editor.goto_text = String::from("$FF");
        editor.handle_key(&key_press(Key::Enter, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 0xFF);
    }

    #[test]
    fn test_goto_clamped() {
        let mut editor = make_test_editor(vec![0; 100]);
        editor.goto_visible = true;
        editor.focused_panel = FocusedPanel::GoToDialog;
        editor.goto_text = String::from("9999");
        editor.handle_key(&key_press(Key::Enter, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 99);
    }
}
