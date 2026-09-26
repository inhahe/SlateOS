//! Slate OS Hex Editor
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

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
#[allow(unused_imports)]
use guitk::color::Color;
use guitk::dialog::{FilePicker, Picked};
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
use guitk::wheel;
#[allow(unused_imports)]
use guitk::widget::{Widget, WidgetId, WidgetTree};
use oswindow::app::{self, App, Response};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;
use unsaved::{Choice, Question};

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha color palette
// ============================================================================

/// Catppuccin Mocha theme colors used throughout the hex editor.
pub mod colors {}

// ============================================================================
// Configuration constants
// ============================================================================

/// Default number of bytes displayed per line (used by external consumers).
#[allow(dead_code)]
const DEFAULT_BYTES_PER_LINE: usize = 16;

/// Width of one cell of the hex dump's character grid, at `font_size`.
///
/// A hex dump is a genuine grid: the byte at column 7 of one line has to sit
/// directly above the byte at column 7 of the next, and the click handler
/// inverts this arithmetic to turn an x coordinate back into a byte offset.
/// So the grid stays — but the cell now comes from the face rather than from
/// `font_size * 0.6`, a guess that put the caret on the wrong byte as soon as
/// the face's digit advance was anything but exactly six tenths of an em.
///
/// It was then `text::digit_advance`, on the reasoning that every glyph the
/// grid is built for is a hex digit. That reasoning covered the hex column and
/// forgot the one beside it: the **ASCII column** draws whatever the bytes
/// happen to spell, and `'W'` in the proportional UI face is nearly twice a
/// digit. So `'W'` spilled into its neighbour's cell, `'i'` left a gap, and
/// `hit_test`'s `(ascii_x / char_w)` — this same arithmetic run backwards —
/// resolved a click to the wrong byte, further wrong the further right it fell.
///
/// A grid needs a face in which *every* glyph advances the same distance,
/// which is what `text::cell_advance` asks for.
fn cell_width(font_size: f32) -> f32 {
    text::cell_advance(font_size, FontWeightHint::Regular)
}

/// The text drawn on `doc`'s tab, modified marker included.
///
/// The marker is part of the label rather than something the renderer appends,
/// so the width the tab is given and the string it is given are always the same
/// string: a tab that gained a ` *` used to overflow by exactly two cells.
fn tab_label(doc: &HexDocument) -> String {
    let mut label = doc.display_name();
    if doc.modified {
        label.push_str(" *");
    }
    label
}

/// Width of the tab drawn for `label`, including the 8 px padding on each side.
fn tab_label_width(label: &str) -> f32 {
    text::measure(label, UI_FONT_SIZE, FontWeightHint::Regular) + 16.0
}

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OffsetDisplay {
    #[default]
    Hex,
    Decimal,
}

/// Hex view configuration.
///
/// One of these belongs to each document, because each tab keeps its own place
/// in its own file. That is also why the wheel accumulator lives here rather
/// than on the editor: two views sharing one accumulator would steal each
/// other's banked fractions, and switching tabs mid-flick would move the wrong
/// file.
///
/// There is deliberately no `show_inspector` field here. There used to be,
/// alongside the [`HexEditor::show_inspector`] that actually drives the
/// renderer, and a flag stored twice is a flag that can disagree with itself —
/// the dead copy defaulted to `true` and stayed `true` however the panel was
/// toggled.
#[derive(Clone, Debug)]
pub struct HexView {
    pub bytes_per_line: BytesPerLine,
    pub offset_display: OffsetDisplay,
    pub show_ascii: bool,
    pub scroll_offset: usize,
    /// Banks the fractions a high-resolution wheel or trackpad sends, so that a
    /// stream of tenth-of-a-notch events scrolls instead of rounding away to
    /// nothing. See [`guitk::wheel`].
    pub wheel: wheel::Accumulator,
}

impl Default for HexView {
    fn default() -> Self {
        Self {
            bytes_per_line: BytesPerLine::default(),
            offset_display: OffsetDisplay::default(),
            show_ascii: true,
            scroll_offset: 0,
            wheel: wheel::Accumulator::default(),
        }
    }
}

// ============================================================================
// Data model — Edit mode
// ============================================================================

/// Editing mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
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
#[derive(Clone, Debug, Default)]
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
    /// The file this document was read from or last saved to.
    ///
    /// A path, not the string a path displays as: that string had been
    /// through a lossy conversion, so a file whose name was not valid UTF-8
    /// would have been saved to some other name.
    pub path: Option<PathBuf>,
    /// The file's whole length, when only its first [`MAX_OPEN_BYTES`] were
    /// read. Such a document is never saved over its file -- that would cut
    /// off everything past what was read.
    pub whole_len: Option<usize>,
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
            path: None,
            whole_len: None,
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

    /// Create a document from a file path and data; `whole_len` when `data`
    /// is only the first part of the file.
    pub fn from_file(path: &Path, data: Vec<u8>, whole_len: Option<usize>) -> Self {
        Self {
            data,
            path: Some(path.to_path_buf()),
            whole_len,
            ..Self::new()
        }
    }

    /// Display name for tabs.
    pub fn display_name(&self) -> String {
        self.path.as_deref().and_then(Path::file_name).map_or_else(
            || String::from("Untitled"),
            |n| Path::new(n).display().to_string(),
        )
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
        } else if visible_lines > 0
            && cursor_line >= self.view.scroll_offset.saturating_add(visible_lines)
        {
            self.view.scroll_offset = cursor_line.saturating_sub(visible_lines).saturating_add(1);
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
        let old_bytes: Vec<u8> = self.data.get(start..=actual_end).unwrap_or(&[]).to_vec();
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
        let actual_end = end.min(if self.data.is_empty() {
            0
        } else {
            self.data.len().saturating_sub(1)
        });
        let old_bytes: Vec<u8> = if start < self.data.len() {
            self.data.get(start..=actual_end).unwrap_or(&[]).to_vec()
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
            SearchDirection::Forward => self.find_forward(
                &bytes_to_find,
                from_offset,
                query.case_sensitive,
                query.wrap_around,
            ),
            SearchDirection::Backward => self.find_backward(
                &bytes_to_find,
                from_offset,
                query.case_sensitive,
                query.wrap_around,
            ),
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
            let d = self
                .data
                .get(offset.saturating_add(i))
                .copied()
                .unwrap_or(0);
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
        let search_len = self
            .data
            .len()
            .saturating_sub(bytes_to_find.len())
            .saturating_add(1);
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
            // A bookmark's colour is *stored on the bookmark*, so it is the
            // user's data rather than chrome: nothing rewrites it when the
            // theme changes, and a themed value here would leave old bookmarks
            // in the old scheme and new ones in the new. Same rule as
            // `snippets`' folder colours and `tmux`'s parsed ANSI cells.
            self.add_bookmark(offset, "", Color::from_hex(0xF9E2AF));
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
            // `get_mut` although a `u8` cannot leave a 256-entry array: the
            // guarantee is in the types rather than in this expression, and
            // this is the form that stays correct if the array is ever sized
            // from something else.
            if let Some(slot) = freq.get_mut(b as usize) {
                *slot = slot.saturating_add(1);
            }
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
                    .get(check_start..check_start.saturating_add(pat_len))
                    == Some(hl.pattern.as_slice());
                if matches && offset >= check_start && offset < check_start.saturating_add(pat_len)
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
    /// Whether the shortcut list is up.
    pub show_help: bool,
    /// Recent file paths.
    pub recent_files: VecDeque<String>,
    /// The picker. Holds the dialog and the routing thirteen
    /// applications used to write out by hand.
    pub picker: FilePicker,
    /// What the last open attempt did, shown in the status bar.
    pub last_open: Option<String>,
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
    /// Bytes held by the editor's clipboard, in the order they were copied.
    ///
    /// Raw bytes rather than the hex text, because the text is a *rendering*
    /// of the bytes and not the other way round — a byte round-trips through
    /// hex exactly, so keeping one field and deriving the other
    /// ([`HexEditor::clipboard_hex`]) is the form that cannot drift out of
    /// sync with itself.
    pub clipboard: Vec<u8>,
    /// Transient message shown in the status bar, e.g. `Copied 16 bytes`.
    ///
    /// Empty when there is nothing to say. Replaced by the next copy or paste
    /// rather than expiring on a timer, since this app has no frame clock.
    pub status_message: String,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// What the file picker is choosing a path for, since one picker serves
    /// opening and saving.
    pub picker_purpose: PickerPurpose,
    /// "Unsaved changes -- save them?", while it is being asked, and what it
    /// would close: the toolkit's own dialog, asked the one way every editor
    /// here asks it (`apps/unsaved`).
    pub question: Option<Question<CloseScope>>,
    /// Set when the window may close. The next response is `Exit`.
    pub quit: bool,
}

/// What the file picker is choosing a path for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerPurpose {
    /// A file to read into a new tab.
    Open,
    /// Where to write the active document.
    SaveAs,
    /// Where to write tab `usize`, which then closes.
    SaveThenClose(usize),
    /// Where to write tab `usize`, and then carry on closing the window.
    SaveThenQuit(usize),
}

/// What a pending close would close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseScope {
    /// One document's tab.
    Tab(usize),
    /// The whole window.
    Window,
}

/// The toolbar's buttons: label, what it does, and where it is drawn -- the
/// one list the drawing and the click both read, so a button is clicked where
/// it is drawn. They were drawn and nothing answered them.
const TOOLBAR_BUTTONS: [(&str, ToolbarAction, f32); 7] = [
    ("New", ToolbarAction::New, 8.0),
    ("Open", ToolbarAction::Open, 58.0),
    ("Save", ToolbarAction::Save, 114.0),
    ("Undo", ToolbarAction::Undo, 174.0),
    ("Redo", ToolbarAction::Redo, 224.0),
    ("Find", ToolbarAction::Find, 284.0),
    ("GoTo", ToolbarAction::GoTo, 334.0),
];

/// A toolbar button's width and height, and its top.
const TOOLBAR_BUTTON: (f32, f32, f32) = (44.0, 28.0, 4.0);

/// What a toolbar button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    New,
    Open,
    Save,
    Undo,
    Redo,
    Find,
    GoTo,
}

/// Every key this program answers, and what it does.
///
/// Thirteen chords and a page of navigation, none of which appeared anywhere
/// on screen. `Ctrl+B`, `Ctrl+N` and `Ctrl+P` are the worst of them: bookmarks
/// are invisible until one is set, so the feature could not be found by
/// looking at the window in any state.
///
/// **Each row is a key this program actually answers**, checked by
/// `every_advertised_key_does_something`, which reads each label with
/// `guitk::shortcut` and presses every key it names.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Arrows", "Move the cursor"),
    ("PageUp / PageDown", "One screen up / down"),
    ("Home / End", "Start / end of the line"),
    ("Ctrl+Home / Ctrl+End", "Start / end of the file"),
    ("Tab", "Swap between the hex and text panes"),
    ("0-9, A-F", "Type a byte, in the hex pane"),
    (
        "Delete / Backspace",
        "Delete the byte at / before the cursor",
    ),
    ("Ctrl+O", "Open a file"),
    ("Ctrl+S", "Save"),
    ("Ctrl+Shift+S", "Save as a new file"),
    (
        "Ctrl+W",
        "Close the tab, asking first if it is not saved -- in the search bar, wrap round or not",
    ),
    ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
    ("Ctrl+C / Ctrl+V", "Copy / paste the selection"),
    ("Ctrl+F", "Find"),
    ("Ctrl+I", "Match case, while the search bar is up"),
    ("Ctrl+G", "Go to an offset"),
    ("Ctrl+B", "Set or clear a bookmark here"),
    ("Ctrl+D", "Show or hide the data inspector"),
    ("Ctrl+N / Ctrl+P", "Next / previous bookmark"),
    ("Ctrl+Tab", "Next tab"),
    ("F1", "This list"),
];

impl HexEditor {
    /// Create a new hex editor with one empty document.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            documents: vec![HexDocument::new()],
            active_tab: 0,
            search: SearchState::default(),
            show_inspector: true,
            show_help: false,
            recent_files: VecDeque::new(),
            picker: FilePicker::new(),
            last_open: None,
            window_width: width,
            window_height: height,
            focused_panel: FocusedPanel::HexView,
            goto_visible: false,
            goto_text: String::new(),
            show_frequency: false,
            show_file_info: false,
            clipboard: Vec::new(),
            status_message: String::new(),
            picker_purpose: PickerPurpose::Open,
            question: None,
            quit: false,
        }
    }

    // ========================================================================
    // Clipboard
    // ========================================================================

    /// Copy the selection — or the byte under the cursor when nothing is
    /// selected — into the editor clipboard.
    ///
    /// Copying nothing (an empty document) deliberately leaves the previous
    /// clipboard alone rather than clearing it: a stray Ctrl+C should not be
    /// able to destroy what the user copied a moment ago.
    pub fn copy_selection(&mut self) {
        let bytes = self.active_doc().copy_as_bytes();
        if bytes.is_empty() {
            self.status_message = "Nothing to copy".to_string();
            return;
        }
        self.status_message = format!("Copied {} byte(s)", bytes.len());
        self.clipboard = bytes;
    }

    /// Paste the editor clipboard at the cursor of the active document.
    ///
    /// Honours the document's edit mode: a read-only document is left alone
    /// and the user is told why, rather than the key silently doing nothing.
    pub fn paste_clipboard(&mut self) {
        if self.clipboard.is_empty() {
            self.status_message = "Clipboard is empty".to_string();
            return;
        }
        if self.active_doc().edit_mode == EditMode::ReadOnly {
            self.status_message = "Document is read-only".to_string();
            return;
        }
        let bytes = std::mem::take(&mut self.clipboard);
        self.active_doc_mut().paste_bytes(&bytes);
        self.status_message = format!("Pasted {} byte(s)", bytes.len());
        self.clipboard = bytes;
    }

    /// The clipboard rendered the way the hex column shows it: uppercase
    /// pairs separated by single spaces.
    ///
    /// This is the form that would go to the *system* clipboard, so that a
    /// paste into a text editor reads like a hex dump; [`HexDocument::paste_hex`]
    /// is its inverse, for text arriving from outside this app.
    #[must_use]
    pub fn clipboard_hex(&self) -> String {
        let mut s = String::with_capacity(self.clipboard.len().saturating_mul(3));
        for (i, b) in self.clipboard.iter().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(&format!("{b:02X}"));
        }
        s
    }

    /// Get a reference to the active document.
    ///
    /// # Panics
    ///
    /// Never in practice: `documents` starts with one entry, `close_tab`
    /// refuses to remove the last, and `active_tab` is only ever set to an
    /// index that exists — all three are enforced in this module. The previous
    /// spelling fell back to `self.documents[0]`, which is not a fallback at
    /// all: the only state in which the `get` fails is an empty `documents`,
    /// and indexing an empty `Vec` panics exactly as the `get` would have.
    #[allow(
        clippy::indexing_slicing,
        reason = "the invariant is stated above and enforced by this module"
    )]
    pub fn active_doc(&self) -> &HexDocument {
        &self.documents[self.active_tab.min(self.documents.len().saturating_sub(1))]
    }

    /// Get a mutable reference to the active document.
    ///
    /// # Panics
    ///
    /// Never, for the reason given on [`HexEditor::active_doc`].
    #[allow(
        clippy::indexing_slicing,
        reason = "the invariant is stated on `active_doc`"
    )]
    pub fn active_doc_mut(&mut self) -> &mut HexDocument {
        let idx = self.active_tab.min(self.documents.len().saturating_sub(1));
        &mut self.documents[idx]
    }

    /// Open a new tab with the given document.
    pub fn open_tab(&mut self, doc: HexDocument) {
        // Track in recent files.
        if let Some(path) = doc.path.as_deref() {
            self.add_recent_file(&path.display().to_string());
        }
        self.documents.push(doc);
        self.active_tab = self.documents.len().saturating_sub(1);
    }

    /// Close the tab at the given index.
    pub fn close_tab(&mut self, index: usize) {
        if self.documents.len() <= 1 {
            // Don't close the last tab; replace it with an empty document.
            // Written as first_mut/push rather than `documents[0] = …` because
            // the field is `pub`: the "never empty" invariant is one the app
            // maintains but callers can break, and a closing tab is the last
            // place that should panic.
            match self.documents.first_mut() {
                Some(slot) => *slot = HexDocument::new(),
                None => self.documents.push(HexDocument::new()),
            }
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
            // `checked_rem` although the emptiness test is a line up: the
            // guard and the modulo are separate statements that an edit can
            // separate further.
            self.active_tab = self
                .active_tab
                .saturating_add(1)
                .checked_rem(self.documents.len())
                .unwrap_or(0);
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

    // ========================================================================
    // Dump geometry
    //
    // Everything below is the *one* place each edge of the hex dump is
    // computed. It is one place because it used not to be: the top edge was
    // spelled out separately in `visible_lines`, in `handle_mouse_click`, in
    // `render_hex_view` and in `render_inspector`, while the bottom edge and
    // the right edge were spelled only in the renderer's clip. The click path
    // therefore had no bottom and no right at all — a click in the status bar
    // computed a line number below every line that had been drawn and jumped
    // the cursor to the end of the file, and a click on the data inspector,
    // which is painted *over* the dump's right-hand side, moved the cursor in
    // the document behind it.
    //
    // The rule that keeps them honest: the renderer emits its clip from these
    // helpers, and the hit test accepts exactly what these helpers describe,
    // so the region drawn *is* the region clicked.
    // ========================================================================

    /// Top edge of the hex dump, below the toolbar and the tab strip.
    const fn content_top() -> f32 {
        TOOLBAR_HEIGHT + TAB_BAR_HEIGHT
    }

    /// Height of the hex dump, above the status bar.
    ///
    /// Clamped at zero: a window shorter than its own chrome has a negative
    /// height, and every consumer wants "no room" rather than a negative one.
    fn content_height(&self) -> f32 {
        (self.window_height - Self::content_top() - STATUS_BAR_HEIGHT).max(0.0)
    }

    /// Width of the hex dump, left of the data inspector panel when it is open.
    fn content_width(&self) -> f32 {
        let full = if self.show_inspector {
            self.window_width - INSPECTOR_WIDTH
        } else {
            self.window_width
        };
        full.max(0.0)
    }

    /// Number of whole lines the hex dump has room for.
    ///
    /// Whole lines only. The renderer draws no partial line, so the strip of
    /// dump below the last full one belongs to no line and [`Self::line_at`]
    /// rejects it.
    pub fn visible_lines(&self) -> usize {
        if LINE_HEIGHT <= 0.0 {
            return 0;
        }
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let lines = (self.content_height() / LINE_HEIGHT) as usize;
        lines
    }

    /// The absolute line index the point `(x, y)` falls on, or `None` if the
    /// point is not on a line the renderer drew.
    ///
    /// `x` matters as well as `y` because the data inspector is drawn over the
    /// dump's right-hand side; without the check a click on an inspector field
    /// moves the cursor in the file underneath.
    ///
    /// A point past the end of the data still returns a line. That is not
    /// drift: clicking the blank space below the last line is the ordinary
    /// "put the cursor at the end" affordance every editor has, and the
    /// callers clamp the resulting byte offset to the last byte. What used to
    /// be wrong is that clicking the *status bar*, which is not blank dump at
    /// all, did the same thing.
    fn line_at(&self, x: f32, y: f32) -> Option<usize> {
        if !x.is_finite() || x < 0.0 || x >= self.content_width() {
            return None;
        }
        let from_top = y - Self::content_top();
        if !from_top.is_finite() || from_top < 0.0 || from_top >= self.content_height() {
            return None;
        }
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let slot = (from_top / LINE_HEIGHT) as usize;
        if slot >= self.visible_lines() {
            return None;
        }
        Some(self.active_doc().view.scroll_offset.saturating_add(slot))
    }

    /// The largest scroll offset that still shows something.
    ///
    /// The last line is allowed to sit at the top of the dump — that is what
    /// `Ctrl+End` lands on — so the bound is `total_lines - 1` rather than
    /// `total_lines - visible_lines`. Scrolling further would leave the dump
    /// blank with no indication of which way to scroll back.
    fn max_scroll(&self) -> usize {
        self.active_doc().total_lines().saturating_sub(1)
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    /// Handle a keyboard event.
    pub fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }

        // The shortcut list, before anything else.
        //
        // `F1` rather than `?`: the ASCII pane writes `key.typed()` straight
        // into the file, and the search and go-to boxes both take text, so `?`
        // is a character in three different places here. Ahead of every one of
        // them, because a key that is sometimes help and sometimes a byte
        // written into somebody's file is worse than no key at all.
        if key.key == Key::F1 && !key.modifiers.ctrl {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if key.key == Key::Escape && self.show_help {
            self.show_help = false;
            return EventResult::Consumed;
        }

        // Global shortcuts (regardless of focus).
        if key.modifiers.ctrl {
            match key.key {
                Key::O => {
                    self.open_file_dialog();
                    return EventResult::Consumed;
                }
                Key::S => {
                    if key.modifiers.shift {
                        self.save_active_as();
                    } else {
                        self.save_active();
                    }
                    return EventResult::Consumed;
                }
                // Not while the search bar has the keyboard, where Ctrl+W
                // says whether the search wraps round.
                Key::W if self.focused_panel != FocusedPanel::SearchBar => {
                    self.request_close_tab(self.active_tab);
                    return EventResult::Consumed;
                }
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
                    self.copy_selection();
                    return EventResult::Consumed;
                }
                Key::V => {
                    self.paste_clipboard();
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
                // `show_inspector` was `true` at construction with no writer
                // anywhere, so the panel was permanent and the bytes it sat
                // beside had that much less room. `Ctrl+D` for "data
                // inspector", which is what every other hex editor calls it.
                // Found by `scripts/frozen-flag-survey.py`.
                Key::D => {
                    self.show_inspector = !self.show_inspector;
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

        // Enter in search bar: perform search. Shift goes the other way.
        //
        // `SearchDirection::Backward` and `find_backward` were written and
        // tested, and `direction` was `Forward` at construction with nothing
        // to change it -- so every search this program ran went forwards and
        // the way back through a file was by starting again from the top.
        if key.key == Key::Enter && self.focused_panel == FocusedPanel::SearchBar {
            self.search.query.direction = if key.modifiers.shift {
                SearchDirection::Backward
            } else {
                SearchDirection::Forward
            };
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
            // Case sensitivity. Before the text branch, which would otherwise
            // take this and type an `i` into the query.
            //
            // `SearchQuery::case_sensitive` is honoured by `match_at` and had
            // no writer anywhere in production, so every search this program
            // ran was case-sensitive and there was no way to ask for anything
            // else -- in a tool whose whole job is finding a byte sequence
            // somebody half remembers.
            if key.key == Key::I && key.modifiers.ctrl {
                self.search.query.case_sensitive = !self.search.query.case_sensitive;
                self.perform_search();
                return EventResult::Consumed;
            }
            // Whether the search runs past the end and starts again.
            // `find_forward` and `find_backward` both take it and it was
            // `true` with no writer, so a search could not be asked to stop
            // at the end of the file -- which is the difference between "not
            // below here" and "not in the file".
            if key.key == Key::W && key.modifiers.ctrl {
                self.search.query.wrap_around = !self.search.query.wrap_around;
                self.perform_search();
                return EventResult::Consumed;
            }
            if key.types_text() {
                self.search.input_text.extend(key.typed());
                return EventResult::Consumed;
            }
            if key.key == Key::Backspace && !self.search.input_text.is_empty() {
                self.search.input_text.pop();
                return EventResult::Consumed;
            }
        }
        if self.focused_panel == FocusedPanel::GoToDialog {
            if key.types_text() {
                self.goto_text.extend(key.typed());
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
                let line_end =
                    line_start
                        .saturating_add(bpl)
                        .saturating_sub(1)
                        .min(if doc.data.is_empty() {
                            0
                        } else {
                            doc.data.len().saturating_sub(1)
                        });
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
            //
            // Every character the keystroke typed, not its first: a dead key
            // whose composition failed types two, and writing one byte of a
            // two-character keystroke into a file is a corruption the user
            // cannot see — the pane shows the byte they meant to type and says
            // nothing about the one that went missing.
            //
            // Non-ASCII is dropped per character rather than rejecting the run,
            // because the pane edits *bytes*: `é` names no single one, and
            // there is no honest byte to write for it.
            let mut wrote_any = false;
            for byte in key.typed().filter(char::is_ascii).map(|ch| ch as u8) {
                let cur = doc.cursor;
                if doc.edit_mode == EditMode::Overwrite && cur < doc.data.len() {
                    doc.overwrite_byte(cur, byte);
                    if cur.saturating_add(1) < doc.data.len() {
                        doc.cursor = cur.saturating_add(1);
                    }
                } else if doc.edit_mode == EditMode::Insert {
                    doc.insert_byte(cur, byte);
                    doc.cursor = cur.saturating_add(1);
                }
                wrote_any = true;
            }
            if wrote_any {
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
        // One step off the cursor, on the side the search is heading, or
        // every press finds the match already under it.
        let cursor = self.active_doc().cursor;
        let from = match self.search.query.direction {
            SearchDirection::Forward => cursor.saturating_add(1),
            SearchDirection::Backward => cursor.saturating_sub(1),
        };
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

        let offset =
            if let Some(stripped) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
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
        let Some(absolute_line) = self.line_at(x, y) else {
            return;
        };
        let bpl = self.active_doc().view.bytes_per_line.value();
        let char_w = cell_width(HEX_FONT_SIZE);

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
    ///
    /// `dy` is in wheel **notches**, positive away from the user, and a
    /// precision device sends fractions of one. This used to read only the
    /// sign of `dy` and move a flat three lines per event, which made a
    /// trackpad — which sends a fraction of a notch per frame — scroll three
    /// lines a frame, and made a fast wheel flick no faster than a slow one.
    /// [`wheel::Accumulator`] converts and banks the remainder instead.
    pub fn handle_scroll(&mut self, dy: f32) {
        let max_scroll = self.max_scroll();
        let doc = self.active_doc_mut();
        let delta = doc.view.wheel.rows(dy);
        doc.view.scroll_offset =
            guitk::scroll_window::shift(doc.view.scroll_offset, delta).min(max_scroll);
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Route a compositor event into the editor.
    ///
    /// The three handlers below it — keys, clicks and the wheel — already
    /// existed and were already tested; nothing dispatched to them, because
    /// nothing delivered an event. This is that dispatch.
    /// Put the file picker up, listing the directory it starts in.
    ///
    /// The widget does no I/O: the host reads the listing and hands it over,
    /// the convention `apps/fileassoc`, `apps/photomanager` and
    /// `apps/filesearch` all follow.
    pub fn open_file_dialog(&mut self) {
        self.picker_purpose = PickerPurpose::Open;
        self.picker.open_to_read();
    }

    /// A new empty document in a tab of its own.
    pub fn new_document(&mut self) {
        self.documents.push(HexDocument::new());
        self.active_tab = self.documents.len().saturating_sub(1);
    }

    /// Save the active document: to its file, or through the picker if it has
    /// none.
    ///
    /// It could not be saved at all. The toolbar drew a Save button that
    /// nothing answered and no key saved, so every edit this program made was
    /// lost when its window closed.
    pub fn save_active(&mut self) {
        let idx = self.active_tab;
        if self.documents.get(idx).is_some_and(|d| d.path.is_none()) {
            self.ask_where_to_save(PickerPurpose::SaveAs);
            return;
        }
        self.status_message = self.save_to_own_file(idx);
    }

    /// Save As: the picker, seeded with the document's name.
    pub fn save_active_as(&mut self) {
        self.ask_where_to_save(PickerPurpose::SaveAs);
    }

    /// Put up the picker to choose where to write.
    fn ask_where_to_save(&mut self, purpose: PickerPurpose) {
        let name = self.active_doc().display_name();
        self.picker_purpose = purpose;
        self.picker.open_to_write(&name);
    }

    /// Write tab `idx` over the file it came from. What to say about it.
    ///
    /// Refused for a document holding only part of its file: writing it back
    /// would cut the file off where the reading stopped.
    fn save_to_own_file(&mut self, idx: usize) -> String {
        let Some(doc) = self.documents.get_mut(idx) else {
            return String::from("Nothing to save");
        };
        let Some(path) = doc.path.clone() else {
            return String::from("It has no file yet");
        };
        if let Some(whole) = doc.whole_len {
            return format!(
                "Not saved: only the first {} of its {whole} bytes were read, and writing them over {} would cut it short -- save as a new file instead",
                doc.data.len(),
                path.display()
            );
        }
        match safeio::write_atomically(&path, &doc.data) {
            Ok(()) => {
                doc.modified = false;
                format!("Saved {} bytes to {}", doc.data.len(), path.display())
            }
            Err(err) => format!("Could not save {}: {err}", path.display()),
        }
    }

    /// Write tab `idx` to `path`, which becomes its file. What to say.
    fn save_to(&mut self, idx: usize, path: &Path) -> Result<String, String> {
        let Some(doc) = self.documents.get_mut(idx) else {
            return Err(String::from("Nothing to save"));
        };
        match safeio::write_atomically(path, &doc.data) {
            Ok(()) => {
                doc.path = Some(path.to_path_buf());
                // All of it now lives in the new file: nothing left to cut.
                doc.whole_len = None;
                doc.modified = false;
                Ok(format!(
                    "Saved {} bytes to {}",
                    doc.data.len(),
                    path.display()
                ))
            }
            Err(err) => Err(format!("Could not save to {}: {err}", path.display())),
        }
    }

    /// The picker chose `path`: do what it was put up for.
    fn picked(&mut self, path: &Path) {
        match self.picker_purpose {
            PickerPurpose::Open => self.last_open = Some(self.open_path(path)),
            PickerPurpose::SaveAs => {
                let idx = self.active_tab;
                self.status_message = match self.save_to(idx, path) {
                    Ok(said) | Err(said) => said,
                };
            }
            PickerPurpose::SaveThenClose(idx) => {
                self.status_message = match self.save_to(idx, path) {
                    Ok(said) => {
                        self.close_tab(idx);
                        said
                    }
                    Err(said) => said,
                };
            }
            PickerPurpose::SaveThenQuit(idx) => match self.save_to(idx, path) {
                Ok(_) => self.continue_quitting(),
                Err(said) => self.status_message = said,
            },
        }
    }

    /// Close tab `idx`, or ask first if it has unsaved changes.
    pub fn request_close_tab(&mut self, idx: usize) {
        match self.documents.get(idx).map(|d| d.modified) {
            Some(true) => {
                self.active_tab = idx;
                let name = self.active_doc().display_name();
                self.question = Some(Question::new(
                    &unsaved::message_for(&[&name]),
                    "Save them before the tab closes?",
                    CloseScope::Tab(idx),
                ));
            }
            Some(false) => self.close_tab(idx),
            None => {}
        }
    }

    /// The window has been asked to close. Whether it may go now; if not,
    /// the question is up.
    pub fn request_quit(&mut self) -> bool {
        if self.documents.iter().any(|d| d.modified) {
            let names: Vec<String> = self
                .documents
                .iter()
                .filter(|d| d.modified)
                .map(HexDocument::display_name)
                .collect();
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            // The question replaces whatever was up: a picker left open would
            // take the keys it needs, and be drawn over it.
            self.picker.close();
            self.show_help = false;
            self.question = Some(Question::new(
                &unsaved::message_for(&names),
                "Save them before closing?",
                CloseScope::Window,
            ));
            false
        } else {
            self.quit = true;
            true
        }
    }

    /// Answer the close question put before `scope`.
    pub fn answer_close(&mut self, scope: CloseScope, choice: Choice) {
        match (scope, choice) {
            (_, Choice::Cancel) => {}
            (CloseScope::Tab(idx), Choice::Discard) => self.close_tab(idx),
            (CloseScope::Tab(idx), Choice::Save) => {
                self.active_tab = idx;
                if self
                    .documents
                    .get(idx)
                    .is_some_and(|d| d.path.is_none() || d.whole_len.is_some())
                {
                    self.ask_where_to_save(PickerPurpose::SaveThenClose(idx));
                } else {
                    let said = self.save_to_own_file(idx);
                    if self.documents.get(idx).is_some_and(|d| !d.modified) {
                        self.close_tab(idx);
                    }
                    self.status_message = said;
                }
            }
            (CloseScope::Window, Choice::Discard) => self.quit = true,
            (CloseScope::Window, Choice::Save) => self.continue_quitting(),
        }
    }

    /// Carry on closing the window: save what can go back to its own file,
    /// ask where to put the first thing that cannot, and quit once nothing is
    /// left unsaved. Stops at the first failure.
    fn continue_quitting(&mut self) {
        for idx in 0..self.documents.len() {
            let own_file = self
                .documents
                .get(idx)
                .is_some_and(|d| d.modified && d.path.is_some() && d.whole_len.is_none());
            if own_file {
                let said = self.save_to_own_file(idx);
                if self.documents.get(idx).is_some_and(|d| d.modified) {
                    self.status_message = format!("{said} -- so the window stays open");
                    return;
                }
            }
        }
        match self.documents.iter().position(|d| d.modified) {
            Some(idx) => {
                self.active_tab = idx;
                self.ask_where_to_save(PickerPurpose::SaveThenQuit(idx));
            }
            None => self.quit = true,
        }
    }

    /// Where each tab is drawn: the one walk the drawing and the click share.
    fn tab_rects(&self) -> Vec<(usize, f32, f32)> {
        let mut x = 4.0_f32;
        self.documents
            .iter()
            .enumerate()
            .map(|(i, doc)| {
                let w = tab_label_width(&tab_label(doc));
                let at = (i, x, w);
                x += w + 2.0;
                at
            })
            .collect()
    }

    /// Do what a toolbar button does.
    pub fn toolbar(&mut self, action: ToolbarAction) {
        match action {
            ToolbarAction::New => self.new_document(),
            ToolbarAction::Open => self.open_file_dialog(),
            ToolbarAction::Save => self.save_active(),
            ToolbarAction::Undo => {
                self.active_doc_mut().undo();
            }
            ToolbarAction::Redo => {
                self.active_doc_mut().redo();
            }
            ToolbarAction::Find => {
                self.search.visible = true;
                self.focused_panel = FocusedPanel::SearchBar;
            }
            ToolbarAction::GoTo => {
                self.goto_visible = true;
                self.focused_panel = FocusedPanel::GoToDialog;
            }
        }
    }

    /// Read `path` into a document. Returns what to say about it.
    ///
    /// A failure is a message rather than a silent no-op: an editor whose
    /// window does not change and offers no reason cannot be told from one
    /// that was never asked.
    ///
    /// Bounded at [`MAX_OPEN_BYTES`], and it **says so when it truncates**. A
    /// hex editor showing the first sixteen mebibytes of a file without
    /// mentioning it is worse than one that refuses: every offset past the cut
    /// is a real offset in a file that has different bytes there.
    pub fn open_path(&mut self, path: &std::path::Path) -> String {
        let shown = path.display().to_string();
        // Read only as far as the cap. `std::fs::read` took the whole file and
        // cut it afterwards, so the four-gigabyte file the cap exists for was
        // read into memory in full before a byte of it was thrown away.
        let read = match safeio::read_capped(path, MAX_OPEN_BYTES) {
            Ok(read) => read,
            Err(err) => return format!("Could not read {shown}: {err}"),
        };
        let (whole, truncated, data) = (read.whole, read.truncated, read.bytes);
        let len = data.len();
        let doc = HexDocument::from_file(path, data, truncated.then_some(whole));

        // Replace an untouched empty document rather than opening a second
        // tab beside it: the editor starts with one, and leaving it there
        // would mean every session began with a stray "Untitled".
        let replace = self
            .documents
            .get(self.active_tab)
            .is_some_and(|d| d.data.is_empty() && !d.modified);
        if replace {
            if let Some(slot) = self.documents.get_mut(self.active_tab) {
                *slot = doc;
            }
        } else {
            self.documents.push(doc);
            self.active_tab = self.documents.len().saturating_sub(1);
        }

        if truncated {
            format!(
                "Opened the first {len} bytes of {shown} -- it is {whole} bytes and the rest is not shown"
            )
        } else {
            format!("Opened {shown} ({len} bytes)")
        }
    }

    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // The close question has every key and click while it is up: a byte
        // typed into the document under it would be a change nobody was asked
        // about. Each one it takes is a redraw -- focus and hover inside it
        // are its own business.
        if let Some(question) = self.question.as_mut()
            && matches!(event, Event::Key(_) | Event::Mouse(_))
        {
            if let Some(choice) = question.handle(event) {
                let scope = question.pending();
                self.question = None;
                self.answer_close(scope, choice);
            }
            return EventResult::Consumed;
        }
        // The picker takes input first while it is up. A tick or a
        // resize comes back as `Ignored` and falls through to its own arm
        // below -- the guard arms this replaced had that property by
        // construction, and it is why neither of these two ever stopped
        // its application's clock.
        match self
            .picker
            .handle(event, self.window_width, self.window_height)
        {
            Picked::Chose(path) => {
                self.picked(&path);
                return EventResult::Consumed;
            }
            // Cancelled grouped with Handled: this caller keeps no dialog
            // state of its own that could go stale.
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key_ev) => self.handle_key(key_ev),
            Event::Mouse(mouse_ev) => self.handle_mouse(mouse_ev),
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension is far below f32's integer-exact range"
                )]
                {
                    self.window_width = *width as f32;
                    self.window_height = *height as f32;
                }
                // Not `Consumed`: a resize is not by itself a reason to redraw.
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// Apply a mouse event.
    fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            let (bw, bh, by) = TOOLBAR_BUTTON;
            if let Some(&(_, action, _)) = TOOLBAR_BUTTONS
                .iter()
                .find(|(_, _, x)| ev.x >= *x && ev.x < x + bw && ev.y >= by && ev.y < by + bh)
            {
                self.toolbar(action);
                return EventResult::Consumed;
            }
            let strip = TOOLBAR_HEIGHT..TOOLBAR_HEIGHT + TAB_BAR_HEIGHT;
            if strip.contains(&ev.y) {
                if let Some(&(i, ..)) = self
                    .tab_rects()
                    .iter()
                    .find(|(_, x, w)| ev.x >= *x && ev.x < x + w)
                {
                    self.active_tab = i;
                    return EventResult::Consumed;
                }
                return EventResult::Ignored;
            }
        }
        match ev.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                // The compositor's mouse event carries no modifier state, so
                // a click here can only start a selection; shift-extending is
                // done from the keyboard, which does carry modifiers.
                self.handle_mouse_click(ev.x, ev.y, false);
                EventResult::Consumed
            }
            MouseEventKind::Scroll { dy, .. } => {
                // No `dy == 0.0` short-circuit: the comparison below already
                // answers `Ignored` for a wheel event that moved nothing, and a
                // guard whose removal changes no behaviour is a branch that
                // only looks like it is doing something. A mutation run is what
                // said so.
                let before = self.active_doc().view.scroll_offset;
                self.handle_scroll(dy);
                if self.active_doc().view.scroll_offset == before {
                    // Already at the end the wheel was pushing towards.
                    EventResult::Ignored
                } else {
                    EventResult::Consumed
                }
            }
            _ => EventResult::Ignored,
        }
    }

    /// Named `render_tree` and not `render`: at equal arity an inherent method
    /// silently wins method lookup over `oswindow::app::App::render`, so an app
    /// that keeps the name draws nothing and reports no error.
    ///
    /// Renders the entire hex editor UI.
    pub fn render_tree(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background.
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: self.palette.base,
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
        self.palette.push_surface(
            tree,
            0.0,
            0.0,
            self.window_width,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        // Toolbar buttons.
        let (bw, bh, by) = TOOLBAR_BUTTON;
        for &(label, _, x) in &TOOLBAR_BUTTONS {
            self.palette
                .push_surface(tree, x, by, bw, bh, 4.0, Surface::Card);
            tree.push(RenderCommand::Text {
                x: x + 6.0,
                y: 10.0,
                text: label.to_string(),
                color: self.palette.text,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(38.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Separator.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT - 1.0,
            x2: self.window_width,
            y2: TOOLBAR_HEIGHT - 1.0,
            color: self.palette.surface1,
            width: 1.0,
        });
    }

    /// Render the tab bar.
    fn render_tab_bar(&self, tree: &mut RenderTree) {
        let y = TOOLBAR_HEIGHT;

        // Tab bar background.
        self.palette.push_surface(
            tree,
            0.0,
            y,
            self.window_width,
            TAB_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        let mut tab_x: f32 = 4.0;

        for (i, doc) in self.documents.iter().enumerate() {
            // The tab strip is chrome, not grid: it holds filenames, which are
            // proportional text and may hold any byte but `/` and NUL. Sizing
            // it in nominal cells made an accented filename's tab half again
            // as wide as the name inside it.
            let label = tab_label(doc);
            let tab_width = tab_label_width(&label);

            let bg_color = if i == self.active_tab {
                self.palette.base
            } else {
                self.palette.surface0
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
                self.palette.text
            } else {
                self.palette.subtext0
            };

            tree.push(RenderCommand::Text {
                x: tab_x + 8.0,
                y: y + 8.0,
                text: label,
                color: text_color,
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(tab_width),
                overflow: TextOverflow::Ellipsis,
            });

            tab_x += tab_width + 2.0;
        }

        // Bottom separator.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TAB_BAR_HEIGHT - 1.0,
            x2: self.window_width,
            y2: y + TAB_BAR_HEIGHT - 1.0,
            color: self.palette.surface1,
            width: 1.0,
        });
    }

    /// Render the main hex view.
    fn render_hex_view(&self, tree: &mut RenderTree) {
        let doc = self.active_doc();
        let content_y = Self::content_top();
        let content_width = self.content_width();
        let vis = self.visible_lines();
        let bpl = doc.view.bytes_per_line.value();
        let char_w = cell_width(HEX_FONT_SIZE);

        // Clip to the hex view area. The rectangle comes from the same three
        // helpers the hit test consults, so the dump that gets drawn is
        // exactly the dump that answers a click.
        tree.push(RenderCommand::PushClip {
            x: 0.0,
            y: content_y,
            width: content_width,
            height: self.content_height(),
        });

        // The offset, hex and ASCII columns are all stepped by `char_w`, which
        // came from the mono face — so draw them in it. Laying a grid out on
        // one face's advances and filling it from another's is what put `'W'`
        // over its neighbour in the ASCII column.
        tree.push(RenderCommand::PushFont {
            family: FontFamily::Mono,
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
                color: self.palette.subtext0,
                font_size: HEX_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(char_w * 11.0),
                overflow: TextOverflow::Ellipsis,
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
                    Some(self.palette.surface1)
                } else if offset == doc.cursor {
                    Some(self.palette.surface0)
                } else if doc.bookmarks.iter().any(|b| b.offset == offset) {
                    Some(Color::rgba(250, 179, 135, 60))
                } else {
                    doc.highlight_color_at(offset)
                        .map(|hl| Color::rgba(hl.r, hl.g, hl.b, 40))
                };

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
                    self.palette.blue
                } else if byte == 0 {
                    self.palette.overlay0
                } else if byte.is_ascii_graphic() || byte == b' ' {
                    self.palette.text
                } else {
                    self.palette.peach
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
                    overflow: TextOverflow::Ellipsis,
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
                    color: self.palette.surface1,
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
                        self.palette.push_surface(
                            tree,
                            ax - 1.0,
                            y,
                            char_w + 1.0,
                            LINE_HEIGHT,
                            0.0,
                            Surface::Selected,
                        );
                    } else if offset == doc.cursor {
                        self.palette.push_surface(
                            tree,
                            ax - 1.0,
                            y,
                            char_w + 1.0,
                            LINE_HEIGHT,
                            0.0,
                            Surface::Card,
                        );
                    }

                    let ch = if byte.is_ascii_graphic() || byte == b' ' {
                        byte as char
                    } else {
                        '.'
                    };

                    let ascii_color = if offset == doc.cursor && !doc.cursor_in_hex {
                        self.palette.blue
                    } else if byte.is_ascii_graphic() || byte == b' ' {
                        self.palette.text
                    } else {
                        self.palette.overlay0
                    };

                    tree.push(RenderCommand::Text {
                        x: ax,
                        y,
                        text: String::from(ch),
                        color: ascii_color,
                        font_size: HEX_FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(char_w),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
        }

        tree.push(RenderCommand::PopFont);
        tree.push(RenderCommand::PopClip);
    }

    /// Render the data inspector panel.
    fn render_inspector(&self, tree: &mut RenderTree) {
        let doc = self.active_doc();
        // The panel starts exactly where the dump ends, by construction rather
        // than by two subtractions that have to agree.
        let panel_x = self.content_width();
        let panel_y = Self::content_top();
        let panel_height = self.content_height();

        // Panel background.
        self.palette.push_surface(
            tree,
            panel_x,
            panel_y,
            INSPECTOR_WIDTH,
            panel_height,
            0.0,
            Surface::Card,
        );

        // Left border.
        tree.push(RenderCommand::Line {
            x1: panel_x,
            y1: panel_y,
            x2: panel_x,
            y2: panel_y + panel_height,
            color: self.palette.surface1,
            width: 1.0,
        });

        // Title.
        tree.push(RenderCommand::Text {
            x: panel_x + 8.0,
            y: panel_y + 6.0,
            text: String::from("Data Inspector"),
            color: self.palette.ink(self.palette.lavender),
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(INSPECTOR_WIDTH - 16.0),
            overflow: TextOverflow::Ellipsis,
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
                color: self.palette.subtext0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(90.0),
                overflow: TextOverflow::Ellipsis,
            });

            let value_str = doc
                .inspect_at(doc.cursor, dtype)
                .unwrap_or_else(|| String::from("--"));
            tree.push(RenderCommand::Text {
                x: value_x,
                y,
                text: value_str,
                color: self.palette.text,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(INSPECTOR_WIDTH - 108.0),
                overflow: TextOverflow::Ellipsis,
            });

            y += 16.0;
        }
    }

    /// Render the status bar.
    fn render_status_bar(&self, tree: &mut RenderTree) {
        let doc = self.active_doc();
        let y = self.window_height - STATUS_BAR_HEIGHT;

        // Status bar background.
        self.palette.push_surface(
            tree,
            0.0,
            y,
            self.window_width,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );

        // Top separator.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.window_width,
            y2: y,
            color: self.palette.surface1,
            width: 1.0,
        });

        let text_y = y + 5.0;

        // Offset (hex + decimal).
        let offset_hex = format!("0x{:08X}", doc.cursor);
        let offset_dec = format!("({})", doc.cursor);
        let offset_text = format!("Offset: {offset_hex} {offset_dec}");
        tree.push(RenderCommand::Text {
            x: 8.0,
            y: text_y,
            text: offset_text.clone(),
            color: self.palette.text,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Selection size.
        let sel_x = 300.0;
        if let Some(sel) = &doc.selection {
            let sel_text = format!("Sel: {} bytes", sel.len());
            tree.push(RenderCommand::Text {
                x: sel_x,
                y: text_y,
                text: sel_text,
                color: self.palette.ink(self.palette.green),
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Transient message (copy/paste feedback). Sits after the selection
        // field, and is bounded so it can never run into the file size on the
        // right no matter how narrow the window is.
        if !self.status_message.is_empty() {
            let msg_x = sel_x + 130.0;
            let msg_width = (self.window_width - 400.0 - msg_x).max(0.0);
            if msg_width > 0.0 {
                tree.push(RenderCommand::Text {
                    x: msg_x,
                    y: text_y,
                    text: self.status_message.clone(),
                    color: self.palette.subtext0,
                    font_size: UI_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(msg_width),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        // File size.
        let size_text = format!("Size: {} bytes", doc.data.len());
        tree.push(RenderCommand::Text {
            x: self.window_width - 400.0,
            y: text_y,
            text: size_text,
            color: self.palette.subtext0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(140.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Edit mode.
        let mode_text = match doc.edit_mode {
            EditMode::ReadOnly => "RO",
            EditMode::Insert => "INS",
            EditMode::Overwrite => "OVR",
        };
        let mode_color = match doc.edit_mode {
            EditMode::ReadOnly => self.palette.red,
            EditMode::Insert => self.palette.green,
            EditMode::Overwrite => self.palette.blue,
        };
        tree.push(RenderCommand::Text {
            x: self.window_width - 240.0,
            y: text_y,
            text: mode_text.to_string(),
            color: mode_color,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(40.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Column indicator (hex/ASCII).
        let col_text = if doc.cursor_in_hex { "HEX" } else { "ASCII" };
        tree.push(RenderCommand::Text {
            x: self.window_width - 190.0,
            y: text_y,
            text: col_text.to_string(),
            color: self.palette.ink(self.palette.lavender),
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(50.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Modified indicator.
        if doc.modified {
            tree.push(RenderCommand::Text {
                x: self.window_width - 120.0,
                y: text_y,
                text: String::from("Modified"),
                color: self.palette.ink(self.palette.yellow),
                font_size: UI_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(70.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Encoding.
        tree.push(RenderCommand::Text {
            x: self.window_width - 50.0,
            y: text_y,
            text: String::from("Hex"),
            color: self.palette.subtext0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(40.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the search bar overlay.
    fn render_search_bar(&self, tree: &mut RenderTree) {
        let bar_width: f32 = 400.0;
        let bar_height: f32 = 40.0;
        let x = self.window_width - bar_width - 20.0;
        let y = Self::content_top() + 4.0;

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
            color: self.palette.surface0,
            corner_radii: CornerRadii::all(6.0),
        });

        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: bar_width,
            height: bar_height,
            color: self.palette.blue,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Search icon placeholder.
        tree.push(RenderCommand::Text {
            x: x + 10.0,
            y: y + 12.0,
            text: String::from("Find:"),
            color: self.palette.subtext0,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(40.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Search input text.
        let input_text = if self.search.input_text.is_empty() {
            String::from("hex bytes or text...")
        } else {
            self.search.input_text.clone()
        };
        let input_color = if self.search.input_text.is_empty() {
            self.palette.overlay0
        } else {
            self.palette.text
        };
        tree.push(RenderCommand::Text {
            x: x + 50.0,
            y: y + 12.0,
            text: input_text,
            color: input_color,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            // Stops where the options begin. It used to reach to
            // `bar_width - 120`, which was clear of one option and is not
            // clear of three.
            max_width: Some((bar_width - 400.0).max(60.0)),
            overflow: TextOverflow::Ellipsis,
        });

        // What the search will do, and the keys that change it. A search
        // box that silently ignores case -- or silently insists on it, or
        // silently stops at the end of the file -- turns a miss into "it is
        // not in the file", which is a claim about the file.
        let on_off = |on: bool| if on { "on" } else { "off" };
        tree.push(RenderCommand::Text {
            x: x + bar_width - 340.0,
            y: y + 12.0,
            text: format!(
                "Case: {} Ctrl+I   Wrap: {} Ctrl+W   Shift+Enter back",
                on_off(self.search.query.case_sensitive),
                on_off(self.search.query.wrap_around),
            ),
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(265.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Match count.
        if self.search.match_count > 0 {
            tree.push(RenderCommand::Text {
                x: x + bar_width - 70.0,
                y: y + 12.0,
                text: format!("{} found", self.search.match_count),
                color: self.palette.ink(self.palette.green),
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(65.0),
                overflow: TextOverflow::Ellipsis,
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
        self.palette
            .push_surface(tree, x, y, dialog_width, dialog_height, 8.0, Surface::Panel);

        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: dialog_width,
            height: dialog_height,
            color: self.palette.lavender,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title.
        tree.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 12.0,
            text: String::from("Go to Offset"),
            color: self.palette.ink(self.palette.lavender),
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Input field background.
        tree.push(RenderCommand::FillRect {
            x: x + 12.0,
            y: y + 36.0,
            width: dialog_width - 24.0,
            height: 28.0,
            color: self.palette.base,
            corner_radii: CornerRadii::all(4.0),
        });

        tree.push(RenderCommand::StrokeRect {
            x: x + 12.0,
            y: y + 36.0,
            width: dialog_width - 24.0,
            height: 28.0,
            color: self.palette.surface1,
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
            self.palette.overlay0
        } else {
            self.palette.text
        };
        tree.push(RenderCommand::Text {
            x: x + 20.0,
            y: y + 42.0,
            text: display_text,
            color: text_color,
            font_size: UI_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_width - 40.0),
            overflow: TextOverflow::Ellipsis,
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
        if (i.saturating_add(1)) % 8 == 0 && i.saturating_add(1) < bytes_per_line && i < data.len()
        {
            hex_part.push(' ');
        }
    }

    format!("{:08X}: {hex_part}  {ascii_part}", offset)
}

// ============================================================================
// Application entry point
// ============================================================================

impl App for HexEditor {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        let doc = self.active_doc();
        let name = doc.display_name();
        if doc.modified {
            // The marker goes in the title because a minimised window is a
            // taskbar entry and nothing else, and this program edits bytes in
            // place.
            format!("Hex Editor — {name} *")
        } else {
            format!("Hex Editor — {name}")
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (self.window_width as u32, self.window_height as u32)
        }
    }

    /// No clock.
    ///
    /// Nothing here advances on its own: bytes change when they are typed over
    /// and the view moves when it is scrolled. There is no animation and no
    /// data that ages, so a tick would redraw an identical frame.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        // It closed at once whatever it held -- and it could not have saved
        // what it held anyway. `KeepOpen` while it asks: any other answer to
        // a close request closes the window, question and all.
        if matches!(event, Event::CloseRequested) {
            return if self.request_quit() {
                Response::Exit
            } else {
                Response::KeepOpen
            };
        }
        let result = self.handle_event(event);
        if self.quit {
            return Response::Exit;
        }
        match result {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Reconciled with the size we are handed rather than trusted from the
        // last `Resize`: the compositor may grant a size that was never asked
        // for, and the first frame is drawn before any `Resize` arrives.
        self.window_width = width;
        self.window_height = height;
        let mut tree = self.render_tree();
        // The picker goes last so it sits over everything, which is the same
        // order in which `handle_event` gives it the click.
        //
        // This was a comment claiming the picker was drawn, above code that
        // did not draw it, for about four minutes. It would have shipped a
        // dialog that takes every keystroke and every click while being
        // invisible -- and the comment would have told the next reader to look
        // somewhere else. `the_picker_is_drawn_when_it_is_open` exists because
        // of it.
        tree.commands
            .extend(self.picker.render(&self.palette, width, height));

        // The close question over the document it is about. It and the picker
        // are never up together: raising it takes the picker down, and
        // answering it takes it down before any picker goes up.
        let palette = self.palette;
        if let Some(question) = self.question.as_mut() {
            question.render(&palette, width, height, &mut tree);
        }

        // And the shortcut list over even that, because it is the one thing a
        // reader asked for explicitly.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut tree,
                &self.palette,
                (width, height),
                0.0,
                SHORTCUTS,
                "F1 closes this",
            );
        }
        tree
    }
}

fn main() -> ExitCode {
    // Opens empty. It used to fill the first document with `(0..=255)` -- every
    // byte value once -- and label it `/demo/sample.bin`, a path that does not
    // exist, so the window named a file it was not showing you. The comment
    // above that said "until a file can be opened this is what there is to
    // edit", which was true of the program and not of the system: the file
    // picker and `std::fs` were both already here.
    //
    // A hex editor is for looking at a particular file's actual bytes. There
    // is no version of that a synthetic buffer satisfies, which is why this one
    // starts on an empty document and says how to open something instead.
    //
    // The arguments are parsed here rather than by `app::launch`, which
    // refuses every argument but `--display`: the file manager opens a file
    // in this program by naming it, and the refusal ended the program -- "exit
    // 2, unexpected argument" on a stderr nobody sees -- before its window
    // opened.
    let args = match app::Args::from_env() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("hexeditor: {e}");
            return ExitCode::from(2);
        }
    };
    let mut editor = HexEditor::new(1200.0, 800.0);
    editor.last_open = Some(open_arguments(&mut editor, &args.rest));
    app::launch_with("hexeditor", args.display.as_deref(), &mut editor)
}

/// Open each file named on the command line, a tab each, and say what
/// happened to every one; with none, how to open something.
///
/// Every message is kept: a file that could not be read is named even when
/// one after it opened, so the window does not report only the last success.
fn open_arguments(editor: &mut HexEditor, paths: &[String]) -> String {
    if paths.is_empty() {
        return String::from("Press Ctrl+O to open a file");
    }
    paths
        .iter()
        .map(|path| editor.open_path(std::path::Path::new(path)))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The most of a file one open will read.
///
/// A hex editor asked to open a four-gigabyte file would otherwise read all of
/// it into memory before drawing anything. The cap is reported when it bites,
/// because every offset past the cut is a real offset in a file that has
/// different bytes there -- a silently truncated view is a view that lies
/// about a specific address.
pub const MAX_OPEN_BYTES: usize = 16 * 1024 * 1024;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
// Panicking on bad data is what a test is *for*: an `unwrap` that fires is the
// failure report, and an index out of range is the assertion. The project
// convention (CLAUDE.md, "Coding Conventions") is to enforce these lints in
// production code and allow them under `#[cfg(test)]`, which is what this does.
//
// `float_cmp` is here for a sharper reason. Several of these tests compare a
// coordinate the renderer emitted against the helper that produced it, and
// exact equality *is* the assertion: the renderer passes those helpers' return
// values straight into the clip, so anything short of bit-for-bit identity
// means a second copy of the arithmetic has appeared somewhere. A tolerance
// would let exactly the drift these tests exist to catch slip through.
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]
mod tests {

    // ------------------------------------------------------------------
    // Compositor routing
    //
    // `handle_key`, `handle_mouse_click` and `handle_scroll` already existed
    // and were already tested. What was missing was anything that *called*
    // them, so these cover the dispatch rather than the handlers.
    // ------------------------------------------------------------------

    fn loaded() -> HexEditor {
        let mut editor = HexEditor::new(1200.0, 800.0);
        let sample: Vec<u8> = (0..=255).collect();
        let mut doc = HexDocument::from_data(sample);
        doc.path = Some(PathBuf::from("/demo/sample.bin"));
        if let Some(first) = editor.documents.first_mut() {
            *first = doc;
        }
        editor
    }

    /// **Every key the shortcut list advertises is one this program answers.**
    ///
    /// The label is read by `guitk::shortcut` rather than matched against a
    /// table written beside it here -- that table would be a third copy of the
    /// same fact, drifting from both the list and the handler.
    ///
    /// The property is "some reachable state answers this key", not "this key
    /// is taken right now". This program opens on an *empty document*, where
    /// almost nothing can act, which is why the states below load bytes first:
    /// a guard run against a fresh window would call most of this program dead.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = help_states()
                    .iter_mut()
                    .any(|ed| ed.handle_key(&stroke) == EventResult::Consumed);
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// Editors chosen so that between them every advertised key has work.
    fn help_states() -> Vec<HexEditor> {
        // 256 bytes, cursor at the start.
        let plain = loaded();

        // ...and away from the start, so `Left`, `Up`, `Backspace` and the
        // keys that go *back* have somewhere to go.
        let mut moved = loaded();
        for _ in 0..40 {
            moved.handle_key(&key_press(Key::Right, Modifiers::NONE));
        }
        for _ in 0..3 {
            moved.handle_key(&key_press(Key::Down, Modifiers::NONE));
        }

        // ...with a bookmark set, which is the only state `Ctrl+N` and
        // `Ctrl+P` can act in -- and bookmarks are invisible until one exists,
        // so this is also the state a user cannot discover by looking.
        let mut marked = loaded();
        for _ in 0..16 {
            marked.handle_key(&key_press(Key::Right, Modifiers::NONE));
        }
        marked.handle_key(&key_press(Key::B, Modifiers::ctrl()));
        for _ in 0..16 {
            marked.handle_key(&key_press(Key::Right, Modifiers::NONE));
        }

        // ...with the search bar up, which is the only state `Ctrl+I` means
        // anything in.
        let mut searching = loaded();
        searching.handle_key(&key_press(Key::F, Modifiers::ctrl()));

        vec![plain, moved, marked, searching]
    }

    /// **The shortcut list reaches the window.**
    ///
    /// The guard above reads the list against the handler; this reads it
    /// against the screen. `apps/rssreader`'s overlay drew twenty of its
    /// twenty-one rows for weeks -- the list and the handler agreed, and the
    /// box was a third quantity agreeing with neither.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut editor = loaded();
        assert!(
            !help_text(&mut editor).contains("F1 closes this"),
            "the list is up before anybody asked for it"
        );

        editor.handle_key(&key_press(Key::F1, Modifiers::NONE));
        let shown = help_text(&mut editor);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        editor.handle_key(&key_press(Key::Escape, Modifiers::NONE));
        assert!(
            !help_text(&mut editor).contains("F1 closes this"),
            "Escape did not close it"
        );
    }

    /// **The data inspector can be put away.**
    ///
    /// `show_inspector` was `true` at construction and written nowhere, so the
    /// panel was permanent and the bytes beside it had that much less room.
    /// `Ctrl+D` and not a bare `D`, which types the hex digit 0xD -- the
    /// chord is in the Ctrl block that runs first, and this asserts the byte
    /// under the cursor did not change, which is the only way to tell the two
    /// apart when both answer `Consumed`.
    #[test]
    fn the_data_inspector_can_be_hidden() {
        let mut editor = loaded();
        let before = editor.show_inspector;
        let byte = editor.active_doc().data.first().copied();

        editor.handle_key(&key_press(Key::D, Modifiers::ctrl()));

        assert_ne!(
            editor.show_inspector, before,
            "Ctrl+D did not move the panel"
        );
        assert_eq!(
            editor.active_doc().data.first().copied(),
            byte,
            "Ctrl+D was taken as hex-digit entry and wrote a byte"
        );
    }

    /// `?` stays a byte, because the ASCII pane has to be able to write one.
    #[test]
    fn a_question_mark_is_not_the_help_key_here() {
        let mut editor = loaded();
        let mut ask = key_press(Key::Slash, Modifiers::NONE);
        ask.modifiers.shift = true;
        editor.handle_key(&ask);
        assert!(
            !help_text(&mut editor).contains("F1 closes this"),
            "`?` opened the list in a program that writes it into a file"
        );
    }

    /// Every string the window is drawing, joined.
    fn help_text(editor: &mut HexEditor) -> String {
        editor
            .render(1200.0, 800.0)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn key(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn wheel(dy: f32) -> Event {
        Event::Mouse(MouseEvent {
            x: 400.0,
            y: 400.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        })
    }

    #[test]
    fn a_key_event_reaches_the_key_handler() {
        // The handler moves the cursor; the dispatch is what was missing.
        let mut editor = loaded();
        let before = editor.active_doc().cursor;
        assert_eq!(editor.handle_event(&key(Key::Right)), EventResult::Consumed);
        assert_ne!(
            editor.active_doc().cursor,
            before,
            "the arrow did not reach handle_key"
        );
    }

    #[test]
    fn the_wheel_scrolls_and_stops_at_the_top() {
        let mut editor = loaded();
        assert_eq!(
            editor.active_doc().view.scroll_offset,
            0,
            "it opens at the top"
        );
        // Towards the user scrolls down through the file.
        assert_eq!(editor.handle_event(&wheel(-1.0)), EventResult::Consumed);
        assert!(editor.active_doc().view.scroll_offset > 0);
        assert_eq!(editor.handle_event(&wheel(1.0)), EventResult::Consumed);
        assert_eq!(editor.active_doc().view.scroll_offset, 0);
        // At the top, another notch upwards changes nothing and must not
        // redraw: this is an editor, and a wheel resting against the end of a
        // file should not spin the compositor.
        assert_eq!(editor.handle_event(&wheel(1.0)), EventResult::Ignored);
    }

    #[test]
    fn a_zero_delta_wheel_event_is_ignored() {
        let mut editor = loaded();
        assert_eq!(editor.handle_event(&wheel(0.0)), EventResult::Ignored);
    }

    #[test]
    fn a_click_reaches_the_click_handler() {
        let mut editor = loaded();
        let before = editor.active_doc().cursor;
        // Somewhere inside the hex pane, below the header rows.
        let click = Event::Mouse(MouseEvent {
            x: 300.0,
            y: 300.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        assert_eq!(editor.handle_event(&click), EventResult::Consumed);
        assert_ne!(
            editor.active_doc().cursor,
            before,
            "the click did not move the cursor"
        );
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut editor = loaded();
        let before = editor.active_doc().cursor;
        let release = Event::Key(KeyEvent {
            key: Key::Right,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(editor.handle_event(&release), EventResult::Ignored);
        assert_eq!(editor.active_doc().cursor, before);
    }

    #[test]
    fn a_resize_is_taken_but_is_not_itself_a_redraw() {
        let mut editor = loaded();
        assert_eq!(
            editor.handle_event(&Event::Resize {
                width: 1600,
                height: 900
            }),
            EventResult::Ignored
        );
        assert!((editor.window_width - 1600.0).abs() < f32::EPSILON);
        assert!((editor.window_height - 900.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_title_names_the_file_and_marks_it_when_modified() {
        // A minimised window is a taskbar entry and nothing else, and this
        // program edits bytes in place.
        let mut editor = loaded();
        let title = editor.title();
        assert!(
            title.contains("sample.bin"),
            "title {title:?} omits the file"
        );
        assert!(
            !title.contains('*'),
            "an unmodified file should not be marked"
        );
        editor.active_doc_mut().modified = true;
        assert!(
            editor.title().contains('*'),
            "a modified file should be marked, got {:?}",
            editor.title()
        );
    }

    #[test]
    fn a_close_request_exits() {
        let mut editor = loaded();
        assert!(matches!(
            editor.on_event(&Event::CloseRequested),
            Response::Exit
        ));
    }

    #[test]
    fn rendering_at_a_new_size_adopts_it_and_draws_something() {
        let mut editor = loaded();
        for (w, h) in [(1.0, 1.0), (640.0, 480.0), (3840.0, 2160.0)] {
            let tree = editor.render(w, h);
            assert!((editor.window_width - w).abs() < f32::EPSILON);
            assert!(!tree.commands.is_empty(), "drew nothing at {w}x{h}");
        }
    }
    use super::*;

    // -- Opening a real file --

    /// A scratch file unique to one test, removed when it is done.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn with(tag: &str, bytes: &[u8]) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "slateos-hexeditor-{tag}-{}-{unique}.bin",
                std::process::id()
            ));
            std::fs::write(&path, bytes).expect("scratch file");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(std::fs::remove_file(&self.0));
        }
    }

    /// The bytes on screen are the bytes in the file.
    ///
    /// Before 2026-09-15 they could not be: `main` filled the first document
    /// with `(0..=255)` and there was no way to read anything else.
    #[test]
    fn opening_a_file_shows_that_files_bytes() {
        let scratch = Scratch::with("open", &[0xDE, 0xAD, 0xBE, 0xEF]);
        let mut editor = HexEditor::new(1200.0, 800.0);
        let said = editor.open_path(&scratch.0);

        let doc = editor.documents.get(editor.active_tab).expect("a document");
        assert_eq!(doc.data, vec![0xDE, 0xAD, 0xBE, 0xEF], "{said}");
        assert_eq!(
            doc.path.as_deref(),
            Some(scratch.0.as_path()),
            "the document does not name the file it came from"
        );
    }

    // -- saving, which it could not do at all --

    fn ctrl_key(key: Key, shift: bool) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers {
                ctrl: true,
                shift,
                ..Modifiers::NONE
            },
            text: String::new(),
        }
    }

    fn typed_key(c: char) -> KeyEvent {
        KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        }
    }

    #[test]
    fn an_edited_file_is_saved_back_to_itself() {
        let scratch = Scratch::with("save", &[0x00, 0x11, 0x22]);
        let mut editor = HexEditor::new(1200.0, 800.0);
        editor.open_path(&scratch.0);
        editor.handle_key(&key_press(Key::A, Modifiers::NONE));
        editor.handle_key(&key_press(Key::B, Modifiers::NONE));
        assert!(
            editor.active_doc().modified,
            "control: the typing edited it"
        );
        editor.handle_key(&ctrl_key(Key::S, false));
        assert_eq!(std::fs::read(&scratch.0).unwrap(), vec![0xAB, 0x11, 0x22]);
        assert!(!editor.active_doc().modified);
        assert!(
            editor.status_message.starts_with("Saved 3 bytes"),
            "{}",
            editor.status_message
        );
    }

    #[test]
    fn a_file_read_only_in_part_is_never_saved_over() {
        // Writing the first sixteen mebibytes back would cut the file there.
        let scratch = Scratch::with("partial", &[1, 2, 3, 4]);
        let mut editor = HexEditor::new(1200.0, 800.0);
        editor.documents[0] = HexDocument::from_file(&scratch.0, vec![1, 2], Some(4));
        editor.active_doc_mut().modified = true;
        editor.handle_key(&ctrl_key(Key::S, false));
        assert_eq!(
            std::fs::read(&scratch.0).unwrap(),
            vec![1, 2, 3, 4],
            "it was cut short"
        );
        assert!(
            editor.status_message.starts_with("Not saved"),
            "{}",
            editor.status_message
        );

        // Save As to a new file is how those bytes are kept.
        let out = Scratch::with("partial-out", &[]);
        editor.handle_key(&ctrl_key(Key::S, true));
        assert!(editor.picker.is_saving());
        editor.picked(&out.0);
        assert_eq!(std::fs::read(&out.0).unwrap(), vec![1, 2]);
        assert_eq!(
            editor.active_doc().whole_len,
            None,
            "the new file holds all of it"
        );
    }

    #[test]
    fn an_untitled_document_is_saved_where_the_picker_says() {
        let out = Scratch::with("untitled-out", &[]);
        let mut editor = make_test_editor(vec![9, 8, 7]);
        editor.handle_key(&ctrl_key(Key::S, false));
        assert!(editor.picker.is_saving(), "no file yet, so it asks where");
        editor.picked(&out.0);
        assert_eq!(std::fs::read(&out.0).unwrap(), vec![9, 8, 7]);
        assert_eq!(editor.active_doc().path.as_deref(), Some(out.0.as_path()));
        assert!(!editor.active_doc().modified);
    }

    // -- the chrome, which was drawn and answered nothing --

    fn click(editor: &mut HexEditor, x: f32, y: f32) -> EventResult {
        editor.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }))
    }

    fn click_button(editor: &mut HexEditor, action: ToolbarAction) {
        let (bw, bh, by) = TOOLBAR_BUTTON;
        let &(_, _, x) = TOOLBAR_BUTTONS
            .iter()
            .find(|b| b.1 == action)
            .expect("a button");
        assert_eq!(
            click(editor, x + bw / 2.0, by + bh / 2.0),
            EventResult::Consumed
        );
    }

    #[test]
    fn every_toolbar_button_does_what_it_says() {
        let mut editor = make_test_editor(vec![0; 16]);
        click_button(&mut editor, ToolbarAction::New);
        assert_eq!(editor.documents.len(), 2);
        assert_eq!(editor.active_tab, 1);

        click_button(&mut editor, ToolbarAction::Open);
        assert!(editor.picker.is_open() && !editor.picker.is_saving());
        editor.picker.close();

        click_button(&mut editor, ToolbarAction::Save);
        assert!(
            editor.picker.is_saving(),
            "an untitled document is saved as"
        );
        editor.picker.close();

        editor.active_tab = 0;
        editor.handle_key(&key_press(Key::F, Modifiers::NONE));
        editor.handle_key(&key_press(Key::F, Modifiers::NONE));
        assert_eq!(editor.active_doc().data[0], 0xFF);
        // A nibble is an edit of its own, so one Undo takes back the second F.
        click_button(&mut editor, ToolbarAction::Undo);
        assert_eq!(editor.active_doc().data[0], 0xF0);
        click_button(&mut editor, ToolbarAction::Redo);
        assert_eq!(editor.active_doc().data[0], 0xFF);

        click_button(&mut editor, ToolbarAction::Find);
        assert!(editor.search.visible);
        click_button(&mut editor, ToolbarAction::GoTo);
        assert!(editor.goto_visible);
    }

    #[test]
    fn a_tab_is_chosen_by_clicking_it() {
        let mut editor = make_test_editor(vec![1]);
        editor.new_document();
        assert_eq!(editor.active_tab, 1);
        let (_, x, w) = editor.tab_rects()[0];
        assert_eq!(
            click(
                &mut editor,
                x + w / 2.0,
                TOOLBAR_HEIGHT + TAB_BAR_HEIGHT / 2.0
            ),
            EventResult::Consumed
        );
        assert_eq!(editor.active_tab, 0);
    }

    // -- closing over unsaved work --

    /// What the close question is asking about, while it is up.
    fn asking(editor: &HexEditor) -> Option<CloseScope> {
        editor.question.as_ref().map(Question::pending)
    }

    #[test]
    fn closing_the_window_over_unsaved_work_asks_and_each_answer_is_kept() {
        let mut editor = make_test_editor(vec![0; 4]);
        assert!(matches!(
            editor.on_event(&Event::CloseRequested),
            Response::Exit
        ));

        let mut editor = make_test_editor(vec![0; 4]);
        editor.active_doc_mut().modified = true;
        assert!(matches!(
            editor.on_event(&Event::CloseRequested),
            Response::KeepOpen
        ));
        assert_eq!(asking(&editor), Some(CloseScope::Window));
        // Through `handle_event`, where the window's events arrive: the
        // question is what takes them there.
        editor.handle_event(&Event::Key(key_press(Key::F, Modifiers::NONE)));
        assert_eq!(
            editor.active_doc().data[0],
            0,
            "a key typed under the question edited the file"
        );
        editor.handle_event(&Event::Key(key_press(Key::Escape, Modifiers::NONE)));
        assert_eq!(asking(&editor), None);

        editor.on_event(&Event::CloseRequested);
        // Drawn first: a dialog's buttons are where it last drew them.
        let (w, h) = (editor.window_width, editor.window_height);
        let _ = editor.render(w, h);
        let (x, y) = editor
            .question
            .as_ref()
            .and_then(|q| q.button_centre(Choice::Discard))
            .expect("the question is drawn");
        let _ = click(&mut editor, x, y);
        assert!(editor.quit, "Don't save, clicked, lets it go");
    }

    #[test]
    fn saving_on_close_writes_the_files_and_asks_where_for_the_untitled() {
        let titled = Scratch::with("close-titled", &[0x10]);
        let out = Scratch::with("close-out", &[]);
        let mut editor = HexEditor::new(1200.0, 800.0);
        editor.open_path(&titled.0);
        editor.handle_key(&key_press(Key::Num2, Modifiers::NONE));
        editor.handle_key(&key_press(Key::Num2, Modifiers::NONE));
        editor.new_document();
        editor.active_doc_mut().data = vec![7];
        editor.active_doc_mut().modified = true;

        editor.on_event(&Event::CloseRequested);
        assert!(!matches!(
            editor.on_event(&Event::Key(key_press(Key::S, Modifiers::NONE))),
            Response::Exit
        ));
        assert_eq!(std::fs::read(&titled.0).unwrap(), vec![0x22]);
        assert!(
            editor.picker.is_saving(),
            "the untitled one needs somewhere to go"
        );
        editor.picked(&out.0);
        assert!(editor.quit);
        assert_eq!(std::fs::read(&out.0).unwrap(), vec![7]);
    }

    #[test]
    fn ctrl_w_asks_before_closing_a_modified_tab() {
        let mut editor = make_test_editor(vec![1]);
        editor.new_document();
        editor.active_doc_mut().modified = true;
        editor.handle_key(&ctrl_key(Key::W, false));
        assert_eq!(editor.documents.len(), 2, "closed unsaved work");
        assert_eq!(asking(&editor), Some(CloseScope::Tab(1)));
        editor.handle_event(&Event::Key(typed_key('d')));
        assert_eq!(editor.documents.len(), 1);
    }

    /// A file that cannot be read says so rather than doing nothing.
    #[test]
    fn an_unreadable_file_reports_instead_of_failing_quietly() {
        let mut editor = HexEditor::new(1200.0, 800.0);
        let missing = std::env::temp_dir().join("slateos-hexeditor-no-such-file.bin");
        let said = editor.open_path(&missing);
        assert!(said.contains("Could not read"), "said {said:?}");
        let doc = editor.documents.get(editor.active_tab).expect("a document");
        assert!(doc.data.is_empty(), "a failed read put bytes on screen");
    }

    /// A truncated view says it is truncated.
    ///
    /// Every offset past the cut is a real offset in a file that has different
    /// bytes there, so a silent truncation is a view that lies about a
    /// specific address -- worse than refusing the file outright.
    /// The file manager opens a file here by naming it on the command line:
    /// each one named opens in a tab of its own, and a file that cannot be read
    /// is said to be so beside the ones that opened.
    #[test]
    fn the_files_named_on_the_command_line_are_opened() {
        let one = Scratch::with("arg-one", b"one");
        let two = Scratch::with("arg-two", b"second");
        let missing = one.0.with_extension("not-there");
        let mut editor = HexEditor::new(1200.0, 800.0);
        let paths: Vec<String> = [&one.0, &missing, &two.0]
            .iter()
            .map(|p| p.to_str().expect("a text path").to_owned())
            .collect();
        let said = open_arguments(&mut editor, &paths);
        assert_eq!(editor.documents.len(), 2, "one tab per file that opened");
        assert_eq!(editor.documents[0].data, b"one");
        assert_eq!(editor.documents[1].data, b"second");
        assert!(said.contains("Could not read"), "{said}");
        assert!(said.contains("(6 bytes)"), "{said}");

        let mut empty = HexEditor::new(1200.0, 800.0);
        assert_eq!(
            open_arguments(&mut empty, &[]),
            "Press Ctrl+O to open a file"
        );
    }

    #[test]
    fn a_file_past_the_cap_says_it_was_cut() {
        let big = vec![0x41_u8; MAX_OPEN_BYTES + 32];
        let scratch = Scratch::with("big", &big);
        let mut editor = HexEditor::new(1200.0, 800.0);
        let said = editor.open_path(&scratch.0);

        let doc = editor.documents.get(editor.active_tab).expect("a document");
        assert_eq!(doc.data.len(), MAX_OPEN_BYTES);
        assert!(
            said.contains("not shown"),
            "a truncated open did not say so: {said:?}"
        );
        assert!(
            said.contains(&big.len().to_string()),
            "the message does not give the real size: {said:?}"
        );
    }

    /// The picker is drawn while it is open.
    ///
    /// This test exists because the first version of the change added a
    /// *comment* saying the picker was drawn, above a `render` that did not
    /// draw it. It compiled, every other test passed, and the result would
    /// have been a dialog that swallowed every keystroke and every click while
    /// being invisible. A comment is not a rendering.
    #[test]
    fn the_picker_is_drawn_when_it_is_open() {
        let mut editor = HexEditor::new(1200.0, 800.0);
        let before = editor.render(1200.0, 800.0).commands.len();
        editor.open_file_dialog();
        let after = editor.render(1200.0, 800.0).commands.len();
        assert!(
            after > before,
            "the picker is open and nothing more is drawn ({before} then {after})"
        );
    }

    /// Opening a second file does not lose the first.
    ///
    /// The editor has tabs, so a second file joins them rather than replacing
    /// what you were looking at. The empty document the window starts with is
    /// the one exception, or every session would begin with a stray
    /// "Untitled" beside the file you asked for.
    #[test]
    fn a_second_file_opens_beside_the_first() {
        let one = Scratch::with("one", &[1, 2, 3]);
        let two = Scratch::with("two", &[4, 5, 6, 7]);
        let mut editor = HexEditor::new(1200.0, 800.0);

        editor.open_path(&one.0);
        assert_eq!(
            editor.documents.len(),
            1,
            "the empty document was not reused"
        );

        editor.open_path(&two.0);
        assert_eq!(
            editor.documents.len(),
            2,
            "the second file replaced the first"
        );
        let active = editor.documents.get(editor.active_tab).expect("a document");
        assert_eq!(active.data, vec![4, 5, 6, 7], "the new tab is not focused");
    }

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
        assert_eq!(parse_hex_string("FF 00 AB"), Some(vec![0xFF, 0x00, 0xAB]));
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
        let doc = HexDocument::from_file(Path::new("/test/foo.bin"), vec![0xAA, 0xBB], None);
        assert_eq!(doc.path.as_deref(), Some(Path::new("/test/foo.bin")));
        assert_eq!(doc.display_name(), "foo.bin");
        assert_eq!(doc.data.len(), 2);
    }

    #[test]
    fn test_document_display_name_with_path() {
        let doc = HexDocument::from_file(Path::new("/some/path/file.bin"), vec![], None);
        assert_eq!(doc.display_name(), "file.bin");
    }

    #[test]
    fn test_document_display_name_untitled() {
        let doc = HexDocument::new();
        assert_eq!(doc.display_name(), "Untitled");
    }

    // ====================================================================
    // Text measurement — the hex grid and the proportional chrome
    // ====================================================================

    #[test]
    fn the_hex_cell_is_derived_from_the_face() {
        // Not a hardcoded fraction of the em: the grid has to match what the
        // face actually advances by, or the caret lands on the wrong byte.
        // All that is asserted here is the derivation, since the value is the
        // face's business.
        let cell = cell_width(HEX_FONT_SIZE);
        assert!(cell > 0.0, "a hex cell has to have a width");
        assert_eq!(
            cell,
            text::cell_advance(HEX_FONT_SIZE, FontWeightHint::Regular)
        );
        // And it scales with the size, because it is measured at the size.
        assert!(cell_width(HEX_FONT_SIZE * 2.0) > cell);
    }

    /// The ASCII column draws whatever the bytes spell, so the cell has to fit
    /// the *widest* printable glyph, not just a digit. A proportional face
    /// fails this by construction — which is how `'W'` came to spill into its
    /// neighbour's cell and how a click resolved to the wrong byte.
    #[test]
    fn every_byte_the_ascii_column_can_draw_fits_its_cell() {
        let cell = cell_width(HEX_FONT_SIZE);
        for b in 0u8..=255 {
            let ch = byte_to_ascii_char(b);
            let w = text::measure_in(
                &ch.to_string(),
                HEX_FONT_SIZE,
                FontWeightHint::Regular,
                FontFamily::Mono,
            );
            assert!(
                w <= cell + 0.01,
                "byte {b:#04x} draws as {ch:?}, which measures {w} in a {cell} cell",
            );
        }
    }

    /// The dump is placed on a mono cell, so it must be drawn in the mono
    /// face. Positions alone cannot catch the mismatch.
    #[test]
    fn the_dump_is_drawn_in_the_family_it_was_measured_in() {
        let mut app = HexEditor::new(1200.0, 800.0);
        app.documents[0] = HexDocument::from_data((0u8..=255).collect());

        let tree = app.render_tree();

        let mut depth = 0_i32;
        let mut deepest = 0_i32;
        let mut inside = 0_usize;
        for cmd in &tree.commands {
            match cmd {
                RenderCommand::PushFont { family } => {
                    assert_eq!(family, &FontFamily::Mono, "only the dump pushes a family");
                    depth += 1;
                    deepest = deepest.max(depth);
                }
                RenderCommand::PopFont => {
                    depth -= 1;
                    assert!(depth >= 0, "a PopFont without a matching PushFont");
                }
                RenderCommand::Text { .. } if depth > 0 => inside += 1,
                _ => {}
            }
        }
        assert_eq!(depth, 0, "the font scopes do not balance");
        assert_eq!(deepest, 1, "the dump's scope was never opened");
        assert!(inside > 0, "no dump glyph was drawn inside the mono scope");
    }

    #[test]
    fn clicking_a_hex_column_selects_that_byte() {
        // The render arithmetic and the click arithmetic have to agree. This
        // walks the columns of the first line, clicking the middle of each
        // byte's two hex digits, and checks the cursor lands on that byte.
        let mut app = HexEditor::new(1200.0, 800.0);
        app.documents[0] = HexDocument::from_data((0u8..=255).collect());

        let cell = cell_width(HEX_FONT_SIZE);
        let bpl = app.active_doc().view.bytes_per_line.value();
        let y = TOOLBAR_HEIGHT + TAB_BAR_HEIGHT + LINE_HEIGHT / 2.0;

        for col in 0..bpl {
            // Centre of the byte's "XX" pair, in the same units the renderer
            // lays the line out in.
            let x = cell * 10.0 + (col as f32) * cell * 3.0 + cell;
            app.handle_mouse_click(x, y, false);
            assert_eq!(
                app.active_doc().cursor,
                col,
                "a click in column {col} selected the wrong byte"
            );
            assert!(app.active_doc().cursor_in_hex);
        }
    }

    // ====================================================================
    // Where the dump is, and where a click on it lands
    //
    // The renderer and the hit test used to compute the dump's edges
    // separately, and only the renderer computed the bottom and right ones
    // at all. These tests read the rectangle out of the `PushClip` the
    // renderer actually emits rather than recomputing it, because a test
    // that recomputes the renderer's arithmetic and then checks the hit
    // test against *that* passes just as happily when the two drift.
    // ====================================================================

    /// A long enough file that the dump is nowhere near running out of lines.
    fn long_doc(app: &mut HexEditor) {
        app.documents[0] = HexDocument::from_data((0..4096u32).map(|b| b as u8).collect());
    }

    /// The rectangle the renderer clips the dump to: `(x, y, width, height)`.
    fn dump_clip(app: &HexEditor) -> (f32, f32, f32, f32) {
        app.render_tree()
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                RenderCommand::PushClip {
                    x,
                    y,
                    width,
                    height,
                } => Some((*x, *y, *width, *height)),
                _ => None,
            })
            .expect("the hex dump is drawn under a clip")
    }

    /// The `(top_y, absolute_line)` of every line the renderer actually drew,
    /// recovered from the offset column it prints at the left edge.
    fn painted_lines(app: &HexEditor) -> Vec<(f32, usize)> {
        let bpl = app.active_doc().view.bytes_per_line.value();
        app.render_tree()
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                // The offset column is the only text drawn at x == 4.0, and
                // it prints the byte offset of the line's first byte.
                RenderCommand::Text { x, y, text, .. } if *x == 4.0 => {
                    let digits = text.trim_end_matches(':').trim();
                    let offset = match app.active_doc().view.offset_display {
                        OffsetDisplay::Hex => usize::from_str_radix(digits, 16).ok()?,
                        OffsetDisplay::Decimal => digits.parse().ok()?,
                    };
                    Some((*y, offset / bpl))
                }
                _ => None,
            })
            .collect()
    }

    /// An x inside the hex column, where a click selects a byte.
    fn hex_column_x() -> f32 {
        cell_width(HEX_FONT_SIZE) * 11.0
    }

    #[test]
    fn clicking_the_status_bar_does_not_move_the_cursor() {
        // The bug this whole section exists for. The click path subtracted the
        // toolbar and tab strip and checked only that the result was not
        // negative, so a click on the status bar — a good forty lines below the
        // last line the renderer drew — computed a line number anyway, and the
        // cursor jumped to the end of the file.
        let mut app = HexEditor::new(1200.0, 800.0);
        long_doc(&mut app);
        app.active_doc_mut().cursor = 0;

        let status_bar_y = app.window_height - STATUS_BAR_HEIGHT / 2.0;
        app.handle_mouse_click(hex_column_x(), status_bar_y, false);
        assert_eq!(
            app.active_doc().cursor,
            0,
            "a click on the status bar moved the cursor"
        );
        assert_eq!(app.line_at(hex_column_x(), status_bar_y), None);
    }

    #[test]
    fn clicking_the_data_inspector_does_not_move_the_cursor_behind_it() {
        // The inspector is painted over the dump's right-hand side, and the
        // click path had no right edge at all: a click on an inspector field
        // ran off the end of the ASCII column, clamped to the last byte of the
        // line, and moved the cursor in the file underneath.
        let mut app = HexEditor::new(1200.0, 800.0);
        long_doc(&mut app);
        app.show_inspector = true;
        app.active_doc_mut().cursor = 0;

        let (_, _, clip_w, _) = dump_clip(&app);
        assert_eq!(
            clip_w,
            app.window_width - INSPECTOR_WIDTH,
            "the dump should stop where the inspector starts"
        );

        let inspector_x = app.window_width - INSPECTOR_WIDTH / 2.0;
        let first_line_y = HexEditor::content_top() + LINE_HEIGHT / 2.0;
        app.handle_mouse_click(inspector_x, first_line_y, false);
        assert_eq!(
            app.active_doc().cursor,
            0,
            "a click on the inspector moved the cursor in the document"
        );

        // With the panel closed the same point is dump, and does select.
        app.show_inspector = false;
        app.handle_mouse_click(inspector_x, first_line_y, false);
        assert!(
            app.active_doc().cursor > 0,
            "closing the inspector should hand that x back to the dump"
        );
    }

    #[test]
    fn the_clip_the_renderer_emits_is_the_region_a_click_lands_in() {
        let mut app = HexEditor::new(1200.0, 800.0);
        long_doc(&mut app);
        app.active_doc_mut().view.scroll_offset = 9;

        let (x, y, w, h) = dump_clip(&app);
        assert_eq!(x, 0.0);
        assert_eq!(y, HexEditor::content_top());
        assert_eq!(w, app.content_width());
        assert_eq!(h, app.content_height());

        // Top edge: inside is the first visible line, one pixel above is the
        // tab strip.
        assert_eq!(app.line_at(hex_column_x(), y), Some(9));
        assert_eq!(app.line_at(hex_column_x(), y - 0.5), None);

        // Bottom edge: past the clip nothing is accepted.
        assert_eq!(app.line_at(hex_column_x(), y + h), None);

        // Left and right edges.
        assert!(app.line_at(0.0, y).is_some());
        assert_eq!(app.line_at(-0.5, y), None);
        assert!(app.line_at(w - 0.5, y).is_some());
        assert_eq!(app.line_at(w, y), None);
    }

    #[test]
    fn every_line_the_renderer_drew_hit_tests_to_itself() {
        // Sweeps across each painted line rather than probing its top edge: a
        // top-edge probe returns the right index under several wrong
        // implementations, because the error in a mis-stepped grid is zero at
        // the origin and grows down the row.
        let mut app = HexEditor::new(1200.0, 800.0);
        long_doc(&mut app);

        for scroll in [0usize, 1, 7, 33] {
            app.active_doc_mut().view.scroll_offset = scroll;
            let drawn = painted_lines(&app);
            assert!(!drawn.is_empty(), "nothing was drawn at scroll {scroll}");
            assert_eq!(
                drawn.len(),
                app.visible_lines(),
                "the renderer and `visible_lines` disagree at scroll {scroll}"
            );

            for (top, line) in drawn {
                for step in 0..8 {
                    #[allow(clippy::cast_precision_loss)]
                    let y = top + LINE_HEIGHT * (step as f32) / 8.0;
                    assert_eq!(
                        app.line_at(hex_column_x(), y),
                        Some(line),
                        "the line painted at {top} hit-tests as something else at y={y}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_toolbar_and_tab_strip_are_not_part_of_the_dump() {
        let mut app = HexEditor::new(1200.0, 800.0);
        long_doc(&mut app);
        app.active_doc_mut().cursor = 100;

        for y in [0.0, TOOLBAR_HEIGHT / 2.0, HexEditor::content_top() - 0.5] {
            app.handle_mouse_click(hex_column_x(), y, false);
            assert_eq!(
                app.active_doc().cursor,
                100,
                "a click at y={y} moved the cursor"
            );
        }
    }

    #[test]
    fn a_window_shorter_than_its_own_chrome_has_no_lines() {
        // The toolbar, tab strip and status bar together are taller than this
        // window, so the dump's height is negative before it is clamped.
        let app = HexEditor::new(1200.0, 40.0);
        assert_eq!(app.content_height(), 0.0);
        assert_eq!(app.visible_lines(), 0);
        for y in [0.0, 20.0, 39.0, HexEditor::content_top()] {
            assert_eq!(app.line_at(hex_column_x(), y), None, "at y={y}");
        }
    }

    #[test]
    fn a_nonfinite_coordinate_selects_nothing() {
        // Pointer coordinates arrive from outside the process.
        let mut app = HexEditor::new(1200.0, 800.0);
        long_doc(&mut app);
        app.active_doc_mut().cursor = 42;

        let good_y = HexEditor::content_top() + LINE_HEIGHT / 2.0;
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(app.line_at(bad, good_y), None);
            assert_eq!(app.line_at(hex_column_x(), bad), None);
            app.handle_mouse_click(bad, good_y, false);
            app.handle_mouse_click(hex_column_x(), bad, false);
            assert_eq!(app.active_doc().cursor, 42, "{bad} moved the cursor");
        }
    }

    // ====================================================================
    // The wheel
    // ====================================================================

    #[test]
    fn one_notch_of_the_wheel_moves_three_lines() {
        let mut app = HexEditor::new(1200.0, 800.0);
        long_doc(&mut app);

        app.handle_scroll(-1.0);
        assert_eq!(app.active_doc().view.scroll_offset, 3);
        app.handle_scroll(-2.0);
        assert_eq!(
            app.active_doc().view.scroll_offset,
            9,
            "two notches should move twice as far as one — the old handler \
             read only the sign of dy and moved a flat three lines"
        );
        app.handle_scroll(1.0);
        assert_eq!(app.active_doc().view.scroll_offset, 6);
    }

    #[test]
    fn a_trackpads_fractions_add_up_instead_of_vanishing() {
        // A precision device sends a fraction of a notch per frame. Rounding
        // each event on its own would return zero every time and the dump
        // would never move.
        let mut app = HexEditor::new(1200.0, 800.0);
        long_doc(&mut app);

        for _ in 0..10 {
            app.handle_scroll(-0.1);
        }
        assert_eq!(
            app.active_doc().view.scroll_offset,
            3,
            "ten tenths of a notch is one notch"
        );
    }

    #[test]
    fn the_wheel_stops_at_both_ends_of_the_file() {
        let mut app = HexEditor::new(1200.0, 800.0);
        app.documents[0] = HexDocument::from_data((0u8..=255).collect());
        let last = app.active_doc().total_lines() - 1;

        for _ in 0..200 {
            app.handle_scroll(-1.0);
        }
        assert_eq!(
            app.active_doc().view.scroll_offset,
            last,
            "the last line should stay at the top of the dump"
        );

        for _ in 0..200 {
            app.handle_scroll(1.0);
        }
        assert_eq!(app.active_doc().view.scroll_offset, 0);
    }

    #[test]
    fn a_nonfinite_wheel_delta_does_not_poison_the_accumulator() {
        let mut app = HexEditor::new(1200.0, 800.0);
        long_doc(&mut app);

        app.handle_scroll(f32::NAN);
        assert_eq!(app.active_doc().view.scroll_offset, 0);
        app.handle_scroll(-1.0);
        assert_eq!(
            app.active_doc().view.scroll_offset,
            3,
            "a NaN in the residue would have stopped the wheel for good"
        );
    }

    #[test]
    fn each_tab_banks_its_own_wheel_fractions() {
        // Two documents sharing one accumulator would steal each other's
        // remainders, so a flick that switched tabs would move the wrong file.
        let mut app = HexEditor::new(1200.0, 800.0);
        long_doc(&mut app);
        app.documents.push(HexDocument::from_data(
            (0..4096u32).map(|b| b as u8).collect(),
        ));

        app.active_tab = 0;
        for _ in 0..4 {
            app.handle_scroll(-0.1);
        }
        app.active_tab = 1;
        for _ in 0..4 {
            app.handle_scroll(-0.1);
        }
        assert_eq!(app.documents[0].view.scroll_offset, 1);
        assert_eq!(app.documents[1].view.scroll_offset, 1);
    }

    #[test]
    fn a_tab_label_carries_its_modified_marker() {
        let mut doc = HexDocument::from_file(Path::new("/tmp/file.bin"), vec![1, 2, 3], None);
        assert_eq!(tab_label(&doc), "file.bin");
        doc.modified = true;
        assert_eq!(tab_label(&doc), "file.bin *");
        // And the marker widens the tab, so it cannot overflow it.
        assert!(tab_label_width("file.bin *") > tab_label_width("file.bin"));
    }

    #[test]
    fn tab_labels_fit_their_tabs() {
        for name in [
            "Untitled",
            "file.bin",
            "a-rather-long-firmware-image-name.rom *",
            // A filename is bytes, not ASCII: ours admit everything but `/`
            // and NUL. Sized by byte length this tab was half again too wide.
            "imagen-de-arranque-ñ.bin",
            "ファームウェア.bin",
        ] {
            let drawn = text::measure(name, UI_FONT_SIZE, FontWeightHint::Regular);
            let tab = tab_label_width(name);
            assert!(
                drawn + 16.0 <= tab + 0.01,
                "{name:?} does not fit its tab ({drawn} in {tab})"
            );
        }
    }

    #[test]
    fn tab_width_is_not_driven_by_byte_length() {
        // Same character count, very different byte counts. If these came out
        // proportional to `len()`, the second tab would be ~3x the first.
        let ascii = tab_label_width("aaa.bin");
        let wide = tab_label_width("ふふふ.bin");
        assert!(
            wide < ascii * 3.0,
            "tab width is tracking bytes, not glyphs ({ascii} vs {wide})"
        );
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(10, "test", pal.yellow);
        assert_eq!(doc.bookmarks.len(), 1);
        assert_eq!(doc.bookmarks[0].offset, 10);
        assert_eq!(doc.bookmarks[0].label, "test");
    }

    #[test]
    fn test_add_bookmark_no_duplicates() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(10, "first", pal.yellow);
        doc.add_bookmark(10, "second", pal.blue);
        assert_eq!(doc.bookmarks.len(), 1);
    }

    #[test]
    fn test_add_bookmark_sorted() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(50, "b", pal.yellow);
        doc.add_bookmark(10, "a", pal.blue);
        doc.add_bookmark(30, "c", pal.green);
        assert_eq!(doc.bookmarks[0].offset, 10);
        assert_eq!(doc.bookmarks[1].offset, 30);
        assert_eq!(doc.bookmarks[2].offset, 50);
    }

    #[test]
    fn test_remove_bookmark() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(10, "test", pal.yellow);
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(10, "a", pal.yellow);
        doc.add_bookmark(30, "b", pal.blue);
        doc.cursor = 0;
        assert_eq!(doc.next_bookmark(), Some(10));
        doc.cursor = 15;
        assert_eq!(doc.next_bookmark(), Some(30));
    }

    #[test]
    fn test_next_bookmark_wraps() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(10, "a", pal.yellow);
        doc.cursor = 50;
        assert_eq!(doc.next_bookmark(), Some(10));
    }

    #[test]
    fn test_prev_bookmark() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut doc = HexDocument::from_data(vec![0; 100]);
        doc.add_bookmark(10, "a", pal.yellow);
        doc.add_bookmark(30, "b", pal.blue);
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

    // ====================================================================
    // HexEditor — the clipboard the Ctrl+C/Ctrl+V keys drive
    //
    // The regression these guard: `Key::C` used to call `copy_as_hex()` and
    // drop the result on the floor, and `Key::V` was an empty arm, so the
    // "Copy/paste (hex string or raw bytes)" line in this module's docs
    // described a feature that did nothing at all. Every test here goes
    // through `handle_key`, not through the helpers, because the helpers were
    // never the broken part.
    // ====================================================================

    fn ctrl(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            text: String::new(),
        }
    }

    #[test]
    fn ctrl_c_puts_the_selection_on_the_clipboard() {
        let mut app = HexEditor::new(1200.0, 800.0);
        *app.active_doc_mut() = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC, 0xDD]);
        app.active_doc_mut().selection = Some(Selection::new(1, 2));

        assert_eq!(app.handle_key(&ctrl(Key::C)), EventResult::Consumed);

        assert_eq!(app.clipboard, vec![0xBB, 0xCC]);
        assert_eq!(app.clipboard_hex(), "BB CC");
        assert!(app.status_message.contains('2'), "{}", app.status_message);
    }

    #[test]
    fn ctrl_c_with_no_selection_takes_the_byte_under_the_cursor() {
        let mut app = HexEditor::new(1200.0, 800.0);
        *app.active_doc_mut() = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC]);
        app.active_doc_mut().cursor = 2;

        app.handle_key(&ctrl(Key::C));

        assert_eq!(app.clipboard, vec![0xCC]);
    }

    #[test]
    fn ctrl_v_inserts_the_clipboard_at_the_cursor() {
        let mut app = HexEditor::new(1200.0, 800.0);
        *app.active_doc_mut() = HexDocument::from_data(vec![0xAA, 0xDD]);
        app.active_doc_mut().edit_mode = EditMode::Insert;
        app.active_doc_mut().selection = Some(Selection::new(0, 0));

        app.handle_key(&ctrl(Key::C));
        app.active_doc_mut().selection = None;
        app.active_doc_mut().cursor = 1;
        assert_eq!(app.handle_key(&ctrl(Key::V)), EventResult::Consumed);

        assert_eq!(app.active_doc().data, vec![0xAA, 0xAA, 0xDD]);
    }

    #[test]
    fn a_paste_does_not_consume_the_clipboard() {
        // Pasting twice is a normal thing to do; `paste_clipboard` takes the
        // buffer out of `self` to satisfy the borrow checker, so this asserts
        // it puts it back.
        let mut app = HexEditor::new(1200.0, 800.0);
        *app.active_doc_mut() = HexDocument::from_data(vec![0xAA]);
        app.active_doc_mut().edit_mode = EditMode::Insert;

        app.handle_key(&ctrl(Key::C));
        app.handle_key(&ctrl(Key::V));
        app.handle_key(&ctrl(Key::V));

        assert_eq!(app.clipboard, vec![0xAA]);
        assert_eq!(app.active_doc().data, vec![0xAA, 0xAA, 0xAA]);
    }

    #[test]
    fn ctrl_v_on_a_read_only_document_says_so_instead_of_doing_nothing() {
        let mut app = HexEditor::new(1200.0, 800.0);
        *app.active_doc_mut() = HexDocument::from_data(vec![0xAA]);
        app.handle_key(&ctrl(Key::C));
        app.active_doc_mut().edit_mode = EditMode::ReadOnly;

        app.handle_key(&ctrl(Key::V));

        assert_eq!(app.active_doc().data, vec![0xAA], "read-only was edited");
        assert!(
            app.status_message.to_lowercase().contains("read-only"),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn copying_nothing_leaves_the_previous_clipboard_intact() {
        // A stray Ctrl+C in an empty tab must not destroy what was copied a
        // moment ago in another one.
        let mut app = HexEditor::new(1200.0, 800.0);
        *app.active_doc_mut() = HexDocument::from_data(vec![0xAA, 0xBB]);
        app.active_doc_mut().selection = Some(Selection::new(0, 1));
        app.handle_key(&ctrl(Key::C));

        *app.active_doc_mut() = HexDocument::new();
        app.handle_key(&ctrl(Key::C));

        assert_eq!(app.clipboard, vec![0xAA, 0xBB]);
    }

    #[test]
    fn clipboard_hex_is_the_inverse_of_paste_hex() {
        // The hex text is what a *system* clipboard would carry, so the two
        // directions have to agree or a copy out and paste back would corrupt.
        let mut app = HexEditor::new(1200.0, 800.0);
        let payload = vec![0x00, 0x0F, 0x7F, 0x80, 0xFF];
        *app.active_doc_mut() = HexDocument::from_data(payload.clone());
        app.active_doc_mut().selection = Some(Selection::new(0, payload.len() - 1));
        app.handle_key(&ctrl(Key::C));

        let text = app.clipboard_hex();
        assert_eq!(text, "00 0F 7F 80 FF");

        let mut target = HexDocument::new();
        target.edit_mode = EditMode::Insert;
        assert!(target.paste_hex(&text));
        assert_eq!(target.data, payload);
    }

    #[test]
    fn the_status_bar_shows_the_copy_message() {
        let mut app = HexEditor::new(1200.0, 800.0);
        *app.active_doc_mut() = HexDocument::from_data(vec![0xAA]);
        app.handle_key(&ctrl(Key::C));
        // Guard against the vacuous pass: an empty message would match any
        // other empty Text command in the bar.
        assert!(!app.status_message.is_empty());

        let mut tree = RenderTree::new();
        app.render_status_bar(&mut tree);
        let shown = tree
            .commands
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if *text == app.status_message));
        assert!(shown, "status message never reached the status bar");
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB, 0xCC, 0xDD]);
        doc.highlights.push(HighlightPattern {
            pattern: vec![0xBB, 0xCC],
            color: pal.red,
            label: String::from("test"),
            enabled: true,
        });
        assert_eq!(doc.highlight_color_at(1), Some(pal.red));
        assert_eq!(doc.highlight_color_at(2), Some(pal.red));
        assert_eq!(doc.highlight_color_at(0), None);
        assert_eq!(doc.highlight_color_at(3), None);
    }

    #[test]
    fn test_highlight_disabled() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut doc = HexDocument::from_data(vec![0xAA, 0xBB]);
        doc.highlights.push(HighlightPattern {
            pattern: vec![0xAA],
            color: pal.red,
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

    /// Shift+Enter walks back through the matches.
    ///
    /// `SearchDirection::Backward` and `find_backward` were written and
    /// tested, and `direction` was `Forward` at construction with nothing in
    /// the program to change it -- so every search went forwards, and the way
    /// back through a file was to start again from the top and count.
    #[test]
    fn shift_enter_walks_back_through_the_matches() {
        // "zz" at 0, 10 and 20, and nothing else that matches. Not "aa":
        // every character of that is a hex digit, so the search box reads it
        // as the byte 0xAA rather than as two letters, which is this
        // program's documented first guess at what was typed.
        let mut data = vec![0u8; 32];
        for at in [0usize, 10, 20] {
            data[at] = b'z';
            data[at.saturating_add(1)] = b'z';
        }
        let mut editor = make_test_editor(data);
        editor.focused_panel = FocusedPanel::SearchBar;
        editor.search.input_text = String::from("zz");

        // Forward to the second and third.
        editor.handle_key(&key_press(Key::Enter, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 10, "the first Enter");
        editor.handle_key(&key_press(Key::Enter, Modifiers::NONE));
        assert_eq!(editor.active_doc().cursor, 20, "the second Enter");

        // And back.
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        editor.handle_key(&key_press(Key::Enter, shift));
        assert_eq!(
            editor.active_doc().cursor,
            10,
            "Shift+Enter did not go back to the previous match"
        );
        editor.handle_key(&key_press(Key::Enter, shift));
        assert_eq!(
            editor.active_doc().cursor,
            0,
            "Shift+Enter did not keep going back"
        );
    }

    /// `Ctrl+W` decides whether a search runs past the end and starts again.
    ///
    /// Both `find_forward` and `find_backward` take `wrap_around` and it was
    /// `true` with no writer, so a search could not be asked to stop at the
    /// end -- which is the difference between "not below here" and "not in
    /// the file".
    #[test]
    fn ctrl_w_decides_whether_the_search_starts_again_at_the_top() {
        // Letters rather than hex digits, for the reason above.
        let mut data = vec![0u8; 32];
        data[0] = b'z';
        data[1] = b'z';
        let mut editor = make_test_editor(data);
        editor.focused_panel = FocusedPanel::SearchBar;
        editor.search.input_text = String::from("zz");

        // Past the only match, wrapping on: it comes back round to it.
        editor.active_doc_mut().cursor = 20;
        assert!(editor.search.query.wrap_around, "control: wrap starts on");
        editor.handle_key(&key_press(Key::Enter, Modifiers::NONE));
        assert_eq!(
            editor.active_doc().cursor,
            0,
            "with wrapping on, the search should have come back to the top"
        );

        // Wrapping off: it stays where it is rather than pretending.
        editor.active_doc_mut().cursor = 20;
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        assert_eq!(
            editor.handle_key(&key_press(Key::W, ctrl)),
            EventResult::Consumed,
            "Ctrl+W was ignored in the search bar"
        );
        assert!(
            !editor.search.query.wrap_around,
            "Ctrl+W did not turn it off"
        );
        editor.active_doc_mut().cursor = 20;
        editor.handle_key(&key_press(Key::Enter, Modifiers::NONE));
        assert_eq!(
            editor.active_doc().cursor,
            20,
            "with wrapping off, the search should not have started again"
        );
    }

    /// The search bar names every key that changes what the search does.
    #[test]
    fn the_search_bar_names_its_keys() {
        let mut editor = make_test_editor(vec![0; 64]);
        editor.focused_panel = FocusedPanel::SearchBar;
        editor.search.visible = true;
        let texts: Vec<String> = editor
            .render(1200.0, 800.0)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();

        for hint in ["Ctrl+I", "Ctrl+W", "Shift+Enter"] {
            assert!(
                texts.iter().any(|t| t.contains(hint)),
                "the search bar never mentions {hint}: {texts:?}"
            );
        }
    }

    fn key_press(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: String::new(),
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
    // HexEditor — search case sensitivity
    // ====================================================================

    /// An editor holding "Hello" with the search bar focused.
    fn editor_searching(text: &str) -> HexEditor {
        let mut editor = make_test_editor(text.as_bytes().to_vec());
        editor.focused_panel = FocusedPanel::SearchBar;
        // The bar is drawn on `search.visible`, not on focus; without this the
        // render assertions test a bar that is not on screen.
        editor.search.visible = true;
        editor
    }

    /// Ctrl+I turns case matching off and on.
    ///
    /// `SearchQuery::case_sensitive` is honoured by `match_at` and had no
    /// writer anywhere in production, so every search this program ran was
    /// case-sensitive and nothing could ask for anything else.
    #[test]
    fn ctrl_i_toggles_case_sensitivity() {
        let mut editor = editor_searching("Hello");
        let before = editor.search.query.case_sensitive;

        editor.handle_key(&key_press(Key::I, Modifiers::ctrl()));

        assert_eq!(
            editor.search.query.case_sensitive, !before,
            "Ctrl+I did not change the setting"
        );
    }

    /// Ctrl+I is not typed into the query.
    ///
    /// The text branch sits directly below and would otherwise take it.
    #[test]
    fn ctrl_i_does_not_type_into_the_query() {
        let mut editor = editor_searching("Hello");

        editor.handle_key(&key_press(Key::I, Modifiers::ctrl()));

        assert!(
            editor.search.input_text.is_empty(),
            "Ctrl+I typed into the box: {:?}",
            editor.search.input_text
        );
    }

    /// With case matching off, a search finds text that differs only in case.
    ///
    /// The behaviour, not the flag: a flag that flips and changes no result
    /// is the defect this replaces, one step along.
    #[test]
    fn a_case_insensitive_search_finds_what_a_sensitive_one_misses() {
        let mut editor = editor_searching("Hello");
        for c in "hello".chars() {
            editor.search.input_text.push(c);
        }

        editor.perform_search();
        assert_eq!(
            editor.search.match_count, 0,
            "control: 'hello' should not match 'Hello' while case matters"
        );

        editor.handle_key(&key_press(Key::I, Modifiers::ctrl()));

        assert!(
            editor.search.match_count > 0,
            "turning case matching off did not find 'Hello'"
        );
    }

    /// The search bar says which way it is set, and which key changes it.
    #[test]
    fn the_search_bar_says_whether_case_matters() {
        let mut editor = editor_searching("Hello");
        let drawn = |e: &mut HexEditor| -> Vec<String> {
            e.render(1200.0, 800.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect()
        };

        assert!(
            drawn(&mut editor).iter().any(|t| t.contains("Case: on")),
            "the bar does not say case matters"
        );

        editor.handle_key(&key_press(Key::I, Modifiers::ctrl()));

        assert!(
            drawn(&mut editor).iter().any(|t| t.contains("Case: off")),
            "the bar still says case matters after it stopped"
        );
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
        let tree = editor.render_tree();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_empty_document() {
        let editor = HexEditor::new(800.0, 600.0);
        let tree = editor.render_tree();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_search_bar() {
        let mut editor = make_test_editor(vec![0; 16]);
        editor.search.visible = true;
        let tree = editor.render_tree();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_goto_dialog() {
        let mut editor = make_test_editor(vec![0; 16]);
        editor.goto_visible = true;
        let tree = editor.render_tree();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_inspector() {
        let mut editor = make_test_editor(vec![0; 16]);
        editor.show_inspector = true;
        let tree = editor.render_tree();
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
        assert_eq!(view.scroll_offset, 0);
        // The inspector's visibility is `HexEditor`'s alone; the view used to
        // carry a second, dead copy of the flag.
        assert!(HexEditor::new(1200.0, 800.0).show_inspector);
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        assert_eq!(pal.base, Color::from_hex(0x1E1E2E));
        assert_eq!(pal.mantle, Color::from_hex(0x181825));
        assert_eq!(pal.surface0, Color::from_hex(0x313244));
        assert_eq!(pal.text, Color::from_hex(0xCDD6F4));
        assert_eq!(pal.blue, Color::from_hex(0x89B4FA));
        assert_eq!(pal.red, Color::from_hex(0xF38BA8));
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

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        // Named explicitly rather than relied on from the file's own imports.
        // The sixteen applications that declare their palette inside a
        // `mod mocha` block import `Color` *there*, so it is not in scope at
        // file level at all -- and once the module is emptied and removed, the
        // import goes with it.
        use guitk::Color;

        fn fills(app: &mut HexEditor) -> Vec<Color> {
            // Fully qualified. Several applications also have an *inherent*
            // `render`, with different arguments, and an inherent method wins
            // resolution over a trait one -- so `app.render(w, h)` calls the
            // wrong function and fails to compile in a way that looks like the
            // trait is missing.
            oswindow::app::App::render(app, 1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = HexEditor::new(1000.0, 700.0);

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }
}
