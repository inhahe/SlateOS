//! Slate OS Sticky Notes
//!
//! Desktop sticky notes application with:
//! - Create, delete, edit, archive, and pin notes
//! - 8 note color palettes (yellow, pink, blue, green, purple, orange, teal, gray)
//! - Drag-to-move and drag-to-resize notes
//! - Z-order stacking with bring-to-front / send-to-back
//! - Rich text: bold, italic, bullet lists, checkboxes
//! - Note title + body text editing
//! - Search across all notes
//! - Categories/tags for organization
//! - Note list sidebar (compact view)
//! - Font size selection (small / medium / large)
//! - Undo/redo for text editing
//! - Config persistence (pipe-delimited text format)
//! - Auto-save
//! - Export all notes as text
//!
//! Uses the guitk library for UI rendering.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, App, Response};

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const MANTLE: Color = Color::from_hex(0x181825);

// ============================================================================
// Note color palettes — each has a light and dark variant
// ============================================================================

/// A note color palette with light (header/accent) and dark (body) variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NoteColorPalette {
    pub light: Color,
    pub dark: Color,
}

/// Index into the predefined note color array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoteColorIndex {
    Yellow = 0,
    Pink = 1,
    Blue = 2,
    Green = 3,
    Purple = 4,
    Orange = 5,
    Teal = 6,
    Gray = 7,
}

impl NoteColorIndex {
    pub fn from_usize(v: usize) -> Option<Self> {
        match v {
            0 => Some(Self::Yellow),
            1 => Some(Self::Pink),
            2 => Some(Self::Blue),
            3 => Some(Self::Green),
            4 => Some(Self::Purple),
            5 => Some(Self::Orange),
            6 => Some(Self::Teal),
            7 => Some(Self::Gray),
            _ => None,
        }
    }

    pub fn as_usize(self) -> usize {
        self as usize
    }

    /// The next colour round the ring.
    ///
    /// `from_usize` and a wrapping index rather than an eight-arm match, so a
    /// ninth colour added to the enum joins the cycle by existing rather than
    /// by being remembered here.
    #[must_use]
    pub fn next(self) -> Self {
        let count = NOTE_COLORS.len().max(1);
        Self::from_usize(
            self.as_usize()
                .saturating_add(1)
                .checked_rem(count)
                .unwrap_or(0),
        )
        .unwrap_or(Self::Yellow)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Yellow => "Yellow",
            Self::Pink => "Pink",
            Self::Blue => "Blue",
            Self::Green => "Green",
            Self::Purple => "Purple",
            Self::Orange => "Orange",
            Self::Teal => "Teal",
            Self::Gray => "Gray",
        }
    }
}

const NOTE_COLORS: [NoteColorPalette; 8] = [
    // Yellow
    NoteColorPalette {
        light: Color::from_hex(0xF9E2AF),
        dark: Color::from_hex(0x45420E),
    },
    // Pink
    NoteColorPalette {
        light: Color::from_hex(0xF5C2E7),
        dark: Color::from_hex(0x452535),
    },
    // Blue
    NoteColorPalette {
        light: Color::from_hex(0x89B4FA),
        dark: Color::from_hex(0x1E2D45),
    },
    // Green
    NoteColorPalette {
        light: Color::from_hex(0xA6E3A1),
        dark: Color::from_hex(0x1E3A1E),
    },
    // Purple
    NoteColorPalette {
        light: Color::from_hex(0xCBA6F7),
        dark: Color::from_hex(0x2E1E45),
    },
    // Orange
    NoteColorPalette {
        light: Color::from_hex(0xFAB387),
        dark: Color::from_hex(0x452A1E),
    },
    // Teal
    NoteColorPalette {
        light: Color::from_hex(0x94E2D5),
        dark: Color::from_hex(0x1E3A35),
    },
    // Gray
    NoteColorPalette {
        light: Color::from_hex(0xA6ADC8),
        dark: Color::from_hex(0x2A2A3A),
    },
];

pub fn note_palette(index: NoteColorIndex) -> NoteColorPalette {
    // `get` rather than `[]`: the index is provably in range today, but the
    // proof lives in `NoteColorIndex`'s discriminants rather than here, and a
    // ninth colour added to the enum and not to the table would otherwise be a
    // panic in the renderer rather than a wrong swatch.
    NOTE_COLORS
        .get(index.as_usize())
        .copied()
        .unwrap_or(NoteColorPalette {
            light: YELLOW,
            dark: SURFACE0,
        })
}

// ============================================================================
// Font size presets
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontSizePreset {
    Small,
    Medium,
    Large,
}

impl FontSizePreset {
    pub fn size(self) -> f32 {
        match self {
            Self::Small => 11.0,
            Self::Medium => 14.0,
            Self::Large => 18.0,
        }
    }

    pub fn title_size(self) -> f32 {
        match self {
            Self::Small => 13.0,
            Self::Medium => 16.0,
            Self::Large => 20.0,
        }
    }

    /// The next size up, wrapping back to `Small` past `Large`.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Small => Self::Medium,
            Self::Medium => Self::Large,
            Self::Large => Self::Small,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }

    // Named `parse_label` rather than `from_str` so it doesn't shadow
    // the standard `std::str::FromStr::from_str` trait method (which
    // would require returning `Result<Self, ...>`).
    pub fn parse_label(s: &str) -> Option<Self> {
        match s {
            "Small" | "small" => Some(Self::Small),
            "Medium" | "medium" => Some(Self::Medium),
            "Large" | "large" => Some(Self::Large),
            _ => None,
        }
    }
}

// ============================================================================
// Rich text model
// ============================================================================

/// A span of styled text within a line.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

impl TextSpan {
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
            bold: false,
            italic: false,
        }
    }

    pub fn styled(text: &str, bold: bool, italic: bool) -> Self {
        Self {
            text: text.to_string(),
            bold,
            italic,
        }
    }
}

/// A line of rich text, optionally a bullet or checkbox.
#[derive(Clone, Debug, PartialEq)]
pub struct RichLine {
    pub kind: LineKind,
    pub spans: Vec<TextSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineKind {
    Plain,
    Bullet,
    Checkbox { checked: bool },
}

impl RichLine {
    pub fn plain(text: &str) -> Self {
        Self {
            kind: LineKind::Plain,
            spans: vec![TextSpan::plain(text)],
        }
    }

    pub fn bullet(text: &str) -> Self {
        Self {
            kind: LineKind::Bullet,
            spans: vec![TextSpan::plain(text)],
        }
    }

    pub fn checkbox(text: &str, checked: bool) -> Self {
        Self {
            kind: LineKind::Checkbox { checked },
            spans: vec![TextSpan::plain(text)],
        }
    }

    /// Get the plain text content of the line (all spans concatenated).
    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    /// Total character count across all spans.
    pub fn char_count(&self) -> usize {
        self.spans.iter().map(|s| s.text.len()).sum()
    }
}

// ============================================================================
// Undo/redo
// ============================================================================

/// A text editing action that can be undone/redone.
#[derive(Clone, Debug)]
pub enum EditAction {
    InsertChar {
        line: usize,
        col: usize,
        ch: char,
    },
    DeleteChar {
        line: usize,
        col: usize,
        ch: char,
    },
    InsertLine {
        line: usize,
        content: RichLine,
    },
    DeleteLine {
        line: usize,
        content: RichLine,
    },
    SetTitle {
        old: String,
        new: String,
    },
    ReplaceBody {
        old: Vec<RichLine>,
        new: Vec<RichLine>,
    },
}

/// Undo/redo history for a note's text.
#[derive(Clone, Debug)]
pub struct UndoHistory {
    undo_stack: Vec<EditAction>,
    redo_stack: Vec<EditAction>,
    max_depth: usize,
}

impl UndoHistory {
    pub fn new(max_depth: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_depth,
        }
    }

    pub fn push(&mut self, action: EditAction) {
        self.redo_stack.clear();
        self.undo_stack.push(action);
        if self.undo_stack.len() > self.max_depth {
            self.undo_stack.remove(0);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn pop_undo(&mut self) -> Option<EditAction> {
        let action = self.undo_stack.pop()?;
        self.redo_stack.push(action.clone());
        Some(action)
    }

    pub fn pop_redo(&mut self) -> Option<EditAction> {
        let action = self.redo_stack.pop()?;
        self.undo_stack.push(action.clone());
        Some(action)
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }
}

// ============================================================================
// Note model
// ============================================================================

/// Unique identifier for a note.
pub type NoteId = u64;

/// A sticky note.
#[derive(Clone, Debug)]
pub struct Note {
    pub id: NoteId,
    pub title: String,
    pub body: Vec<RichLine>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color_index: NoteColorIndex,
    pub pinned: bool,
    pub archived: bool,
    pub z_order: u32,
    pub tags: Vec<String>,
    pub font_size: FontSizePreset,
    pub created_at: u64,
    pub modified_at: u64,
    pub undo_history: UndoHistory,
}

impl Note {
    pub fn new(id: NoteId, x: f32, y: f32) -> Self {
        Self {
            id,
            title: String::from("New Note"),
            body: vec![RichLine::plain("")],
            x,
            y,
            width: 220.0,
            height: 200.0,
            color_index: NoteColorIndex::Yellow,
            pinned: false,
            archived: false,
            z_order: 0,
            tags: Vec::new(),
            font_size: FontSizePreset::Medium,
            created_at: 0,
            modified_at: 0,
            undo_history: UndoHistory::new(100),
        }
    }

    /// Get the palette for this note's color.
    pub fn palette(&self) -> NoteColorPalette {
        note_palette(self.color_index)
    }

    /// Check if the note body or title contains a search query (case-insensitive).
    pub fn matches_search(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        // `tag:work` means the tag `work` and nothing else. A bare `work`
        // still matches a note titled "homework", which is the right answer
        // for typing and the wrong one for clicking a tag chip: the chip is a
        // promise that the list it produces is exactly the notes carrying that
        // tag, and a substring match quietly breaks that promise for any tag
        // that happens to be a word inside another.
        if let Some(tag) = query.strip_prefix(TAG_PREFIX) {
            let tag = tag.trim().to_lowercase();
            return tag.is_empty() || self.tags.iter().any(|t| t.to_lowercase() == tag);
        }
        let q = query.to_lowercase();
        if self.title.to_lowercase().contains(&q) {
            return true;
        }
        for line in &self.body {
            if line.plain_text().to_lowercase().contains(&q) {
                return true;
            }
        }
        for tag in &self.tags {
            if tag.to_lowercase().contains(&q) {
                return true;
            }
        }
        false
    }

    /// Set the note's position.
    pub fn set_position(&mut self, x: f32, y: f32) {
        if !self.pinned {
            self.x = x;
            self.y = y;
        }
    }

    /// Set the note's size, enforcing minimum dimensions.
    pub fn set_size(&mut self, width: f32, height: f32) {
        self.width = width.max(120.0);
        self.height = height.max(80.0);
    }

    /// Add a tag if not already present.
    pub fn add_tag(&mut self, tag: &str) {
        let tag_s = tag.to_string();
        if !self.tags.contains(&tag_s) {
            self.tags.push(tag_s);
        }
    }

    /// Remove a tag.
    pub fn remove_tag(&mut self, tag: &str) -> bool {
        if let Some(pos) = self.tags.iter().position(|t| t == tag) {
            self.tags.remove(pos);
            true
        } else {
            false
        }
    }

    /// Toggle a checkbox line's checked state.
    pub fn toggle_checkbox(&mut self, line_index: usize) -> bool {
        if let Some(line) = self.body.get_mut(line_index)
            && let LineKind::Checkbox { checked } = &mut line.kind
        {
            *checked = !*checked;
            return true;
        }
        false
    }

    /// Get the body as plain text (all lines joined by newlines).
    pub fn body_text(&self) -> String {
        self.body
            .iter()
            .map(|line| {
                let prefix = match &line.kind {
                    LineKind::Plain => String::new(),
                    LineKind::Bullet => "* ".to_string(),
                    LineKind::Checkbox { checked } => {
                        if *checked {
                            "[x] ".to_string()
                        } else {
                            "[ ] ".to_string()
                        }
                    }
                };
                format!("{}{}", prefix, line.plain_text())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Set the body from plain text, parsing bullet/checkbox markers.
    pub fn set_body_from_text(&mut self, text: &str) {
        self.body = parse_rich_text(text);
    }

    /// The byte offset at or before `col` in `text` that is a character
    /// boundary, and never past the end.
    ///
    /// `col` throughout this type is a **byte** offset, because `String::insert`
    /// and `String::remove` index by bytes — and both *panic* rather than
    /// rounding when the offset is inside a multi-byte character. Clamping to
    /// `len()`, which is what the two callers below used to do, stops the
    /// offset running off the end but says nothing about the boundary, so any
    /// note containing an accent, Cyrillic, CJK or an emoji had a crash one
    /// call away.
    ///
    /// Moving *back* to the boundary rather than forward keeps the edit on the
    /// character the caller was pointing into, which is what
    /// `apps/editor::snap_to_boundary` and `apps/markdowneditor::clamp_col`
    /// also chose. See `GUI-TEXT-INPUT-CURSORS-STEP-BY-BYTES` in
    /// `known-issues.md` for the widgets where this class of bug was live.
    fn snap_col(text: &str, col: usize) -> usize {
        let mut col = col.min(text.len());
        // Terminates: offset 0 is always a boundary.
        while !text.is_char_boundary(col) {
            col = col.saturating_sub(1);
        }
        col
    }

    /// Insert a character at a specific line/byte-column position.
    ///
    /// `col` is clamped to the line's length and snapped back to a character
    /// boundary, so no offset a caller can supply panics.
    pub fn insert_char(&mut self, line: usize, col: usize, ch: char) {
        if let Some(rich_line) = self.body.get_mut(line)
            && let Some(span) = rich_line.spans.first_mut()
        {
            let col = Self::snap_col(&span.text, col);
            span.text.insert(col, ch);
            self.undo_history
                .push(EditAction::InsertChar { line, col, ch });
        }
    }

    /// Delete the character at a specific line/byte-column position.
    ///
    /// Returns the whole character removed, not a byte of one: `col` is snapped
    /// back to a character boundary first, so pointing into the middle of a
    /// multi-byte character deletes that character rather than aborting.
    /// Returns `None` at or past the end of the line.
    pub fn delete_char(&mut self, line: usize, col: usize) -> Option<char> {
        if let Some(rich_line) = self.body.get_mut(line)
            && let Some(span) = rich_line.spans.first_mut()
            && col < span.text.len()
        {
            let col = Self::snap_col(&span.text, col);
            let ch = span.text.remove(col);
            self.undo_history
                .push(EditAction::DeleteChar { line, col, ch });
            return Some(ch);
        }
        None
    }

    /// Insert a new line at the given index.
    pub fn insert_line(&mut self, index: usize, content: RichLine) {
        let idx = index.min(self.body.len());
        self.undo_history.push(EditAction::InsertLine {
            line: idx,
            content: content.clone(),
        });
        self.body.insert(idx, content);
    }

    /// Delete a line at the given index.
    pub fn delete_line(&mut self, index: usize) -> Option<RichLine> {
        if index < self.body.len() && self.body.len() > 1 {
            let removed = self.body.remove(index);
            self.undo_history.push(EditAction::DeleteLine {
                line: index,
                content: removed.clone(),
            });
            Some(removed)
        } else {
            None
        }
    }

    /// Replace the note's title, recording the change so it can be undone.
    ///
    /// One action for the whole edit rather than one per keystroke: the caller
    /// hands over the title as it was when editing began, so a single undo
    /// restores the name the user started from instead of walking back through
    /// twenty half-typed prefixes.
    pub fn commit_title(&mut self, old: String) {
        if old == self.title {
            return;
        }
        let new = self.title.clone();
        self.undo_history.push(EditAction::SetTitle { old, new });
    }

    /// Undo the most recent recorded edit. Returns whether anything moved.
    pub fn undo(&mut self) -> bool {
        match self.undo_history.pop_undo() {
            Some(action) => {
                self.revert(&action);
                true
            }
            None => false,
        }
    }

    /// Redo the most recently undone edit. Returns whether anything moved.
    pub fn redo(&mut self) -> bool {
        match self.undo_history.pop_redo() {
            Some(action) => {
                self.replay(&action);
                true
            }
            None => false,
        }
    }

    /// Apply an action's inverse **without recording it**.
    ///
    /// The recording methods above (`insert_char` and friends) cannot be
    /// reused here: each of them pushes onto the undo stack, and
    /// [`UndoHistory::push`] clears the redo stack — so an undo written in
    /// terms of them would erase the redo it just created and then queue
    /// itself to be undone again. That is why this walks the enum by hand.
    fn revert(&mut self, action: &EditAction) {
        match action {
            EditAction::InsertChar { line, col, ch } => {
                Self::remove_at(self.span_mut(*line), *col, *ch);
            }
            EditAction::DeleteChar { line, col, ch } => {
                if let Some(text) = self.span_mut(*line) {
                    let col = Self::snap_col(text, *col);
                    text.insert(col, *ch);
                }
            }
            EditAction::InsertLine { line, .. } => {
                if *line < self.body.len() {
                    self.body.remove(*line);
                }
            }
            EditAction::DeleteLine { line, content } => {
                let at = (*line).min(self.body.len());
                self.body.insert(at, content.clone());
            }
            EditAction::SetTitle { old, .. } => self.title = old.clone(),
            EditAction::ReplaceBody { old, .. } => self.body = old.clone(),
        }
        self.ensure_one_line();
    }

    /// Apply an action as it was originally performed, without recording it.
    fn replay(&mut self, action: &EditAction) {
        match action {
            EditAction::InsertChar { line, col, ch } => {
                if let Some(text) = self.span_mut(*line) {
                    let col = Self::snap_col(text, *col);
                    text.insert(col, *ch);
                }
            }
            EditAction::DeleteChar { line, col, ch } => {
                Self::remove_at(self.span_mut(*line), *col, *ch);
            }
            EditAction::InsertLine { line, content } => {
                let at = (*line).min(self.body.len());
                self.body.insert(at, content.clone());
            }
            EditAction::DeleteLine { line, .. } => {
                if *line < self.body.len() {
                    self.body.remove(*line);
                }
            }
            EditAction::SetTitle { new, .. } => self.title = new.clone(),
            EditAction::ReplaceBody { new, .. } => self.body = new.clone(),
        }
        self.ensure_one_line();
    }

    /// Remove `ch` at `col`, and only if `ch` is really what is there.
    ///
    /// The guard matters because an undo stack can be replayed against a note
    /// that has since been edited by something the stack does not know about —
    /// a reload from disk, say. Removing whatever happens to sit at that offset
    /// would corrupt the note silently; removing nothing leaves it as it was.
    fn remove_at(text: Option<&mut String>, col: usize, ch: char) {
        if let Some(text) = text {
            let col = Self::snap_col(text, col);
            if text.get(col..).is_some_and(|rest| rest.starts_with(ch)) {
                text.remove(col);
            }
        }
    }

    /// The editable text of a body line, if it has one.
    fn span_mut(&mut self, line: usize) -> Option<&mut String> {
        self.body
            .get_mut(line)
            .and_then(|l| l.spans.first_mut())
            .map(|s| &mut s.text)
    }

    /// A note always has at least one line, so the caret always has somewhere
    /// to be. Reverting the insert of the second line must not leave zero.
    fn ensure_one_line(&mut self) {
        if self.body.is_empty() {
            self.body.push(RichLine::plain(""));
        }
    }
}

// ============================================================================
// Rich text parsing
// ============================================================================

/// Parse plain text into rich lines, recognizing bullet (`* `) and
/// checkbox (`[ ] ` / `[x] `) markers.
pub fn parse_rich_text(text: &str) -> Vec<RichLine> {
    let mut lines = Vec::new();
    for raw in text.split('\n') {
        let trimmed = raw.trim_start();
        if let Some(rest) = trimmed
            .strip_prefix("[x] ")
            .or_else(|| trimmed.strip_prefix("[X] "))
        {
            lines.push(RichLine::checkbox(rest, true));
        } else if let Some(rest) = trimmed.strip_prefix("[ ] ") {
            lines.push(RichLine::checkbox(rest, false));
        } else if let Some(rest) = trimmed
            .strip_prefix("* ")
            .or_else(|| trimmed.strip_prefix("- "))
        {
            lines.push(RichLine::bullet(rest));
        } else {
            lines.push(RichLine::plain(raw));
        }
    }
    if lines.is_empty() {
        lines.push(RichLine::plain(""));
    }
    lines
}

// ============================================================================
// Snap-to-grid
// ============================================================================

/// Grid size for snap-to-grid positioning.
const GRID_SIZE: f32 = 20.0;

/// Snap a coordinate to the nearest grid point.
pub fn snap_to_grid(value: f32) -> f32 {
    (value / GRID_SIZE).round() * GRID_SIZE
}

/// Optionally snap to grid if enabled.
pub fn maybe_snap(value: f32, snap_enabled: bool) -> f32 {
    if snap_enabled {
        snap_to_grid(value)
    } else {
        value
    }
}

// ============================================================================
// Drag state
// ============================================================================

/// Active drag interaction.
#[derive(Clone, Debug, PartialEq)]
pub enum DragState {
    /// Not dragging.
    None,
    /// Moving a note by its title bar.
    Moving {
        note_id: NoteId,
        offset_x: f32,
        offset_y: f32,
    },
    /// Resizing a note from the bottom-right corner.
    Resizing {
        note_id: NoteId,
        start_w: f32,
        start_h: f32,
        start_mx: f32,
        start_my: f32,
    },
}

// ============================================================================
// Note Store (all notes + metadata)
// ============================================================================

/// The core data store for all sticky notes.
#[derive(Clone, Debug)]
pub struct NoteStore {
    notes: Vec<Note>,
    next_id: NoteId,
    next_z: u32,
    search_query: String,
    snap_to_grid: bool,
    auto_save_dirty: bool,
    sidebar_visible: bool,
    active_note: Option<NoteId>,
    drag: DragState,
}

impl NoteStore {
    pub fn new() -> Self {
        Self {
            notes: Vec::new(),
            next_id: 1,
            next_z: 1,
            search_query: String::new(),
            snap_to_grid: false,
            auto_save_dirty: false,
            sidebar_visible: false,
            active_note: None,
            drag: DragState::None,
        }
    }

    /// Create a new note at the given position.
    pub fn create_note(&mut self, x: f32, y: f32) -> NoteId {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let mut note = Note::new(id, x, y);
        note.z_order = self.next_z;
        self.next_z = self.next_z.wrapping_add(1);
        self.notes.push(note);
        self.auto_save_dirty = true;
        id
    }

    /// Create a note with a specific color.
    pub fn create_colored_note(&mut self, x: f32, y: f32, color: NoteColorIndex) -> NoteId {
        let id = self.create_note(x, y);
        if let Some(note) = self.get_note_mut(id) {
            note.color_index = color;
        }
        id
    }

    /// Delete a note by ID.
    pub fn delete_note(&mut self, id: NoteId) -> bool {
        if let Some(pos) = self.notes.iter().position(|n| n.id == id) {
            self.notes.remove(pos);
            if self.active_note == Some(id) {
                self.active_note = None;
            }
            self.auto_save_dirty = true;
            true
        } else {
            false
        }
    }

    /// Get a note by ID (immutable).
    pub fn get_note(&self, id: NoteId) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    /// Get a note by ID (mutable).
    pub fn get_note_mut(&mut self, id: NoteId) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    /// Number of notes (including archived).
    pub fn total_count(&self) -> usize {
        self.notes.len()
    }

    /// Number of visible (non-archived) notes.
    pub fn visible_count(&self) -> usize {
        self.notes.iter().filter(|n| !n.archived).count()
    }

    /// Number of archived notes.
    pub fn archived_count(&self) -> usize {
        self.notes.iter().filter(|n| n.archived).count()
    }

    /// Get all visible (non-archived) notes, sorted by z-order ascending.
    pub fn visible_notes(&self) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self.notes.iter().filter(|n| !n.archived).collect();
        notes.sort_by_key(|n| n.z_order);
        notes
    }

    /// Get all archived notes.
    pub fn archived_notes(&self) -> Vec<&Note> {
        self.notes.iter().filter(|n| n.archived).collect()
    }

    /// Get visible notes matching the current search query.
    pub fn search_results(&self) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| !n.archived && n.matches_search(&self.search_query))
            .collect();
        notes.sort_by_key(|n| n.z_order);
        notes
    }

    /// Get notes by tag.
    pub fn notes_with_tag(&self, tag: &str) -> Vec<&Note> {
        self.notes
            .iter()
            .filter(|n| !n.archived && n.tags.iter().any(|t| t == tag))
            .collect()
    }

    /// Get all unique tags across all notes.
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self.notes.iter().flat_map(|n| n.tags.clone()).collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// Set the search query.
    pub fn set_search(&mut self, query: &str) {
        self.search_query = query.to_string();
    }

    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    /// Archive a note (hide it but keep it).
    pub fn archive_note(&mut self, id: NoteId) -> bool {
        if let Some(note) = self.get_note_mut(id) {
            note.archived = true;
            self.auto_save_dirty = true;
            true
        } else {
            false
        }
    }

    /// Unarchive a note.
    pub fn unarchive_note(&mut self, id: NoteId) -> bool {
        if let Some(note) = self.get_note_mut(id) {
            note.archived = false;
            self.auto_save_dirty = true;
            true
        } else {
            false
        }
    }

    /// Pin a note (always on top, immovable).
    pub fn pin_note(&mut self, id: NoteId) -> bool {
        if let Some(note) = self.get_note_mut(id) {
            note.pinned = true;
            self.auto_save_dirty = true;
            true
        } else {
            false
        }
    }

    /// Unpin a note.
    pub fn unpin_note(&mut self, id: NoteId) -> bool {
        if let Some(note) = self.get_note_mut(id) {
            note.pinned = false;
            self.auto_save_dirty = true;
            true
        } else {
            false
        }
    }

    /// Toggle pin state.
    pub fn toggle_pin(&mut self, id: NoteId) -> bool {
        if let Some(note) = self.get_note_mut(id) {
            note.pinned = !note.pinned;
            self.auto_save_dirty = true;
            true
        } else {
            false
        }
    }

    /// Bring a note to the front (highest z-order).
    pub fn bring_to_front(&mut self, id: NoteId) {
        let z = self.next_z;
        self.next_z = self.next_z.wrapping_add(1);
        if let Some(note) = self.get_note_mut(id) {
            note.z_order = z;
        }
    }

    /// Send a note to the back (lowest z-order).
    pub fn send_to_back(&mut self, id: NoteId) {
        // Find the current minimum z-order.
        let min_z = self.notes.iter().map(|n| n.z_order).min().unwrap_or(0);
        if let Some(note) = self.get_note_mut(id) {
            note.z_order = min_z.saturating_sub(1);
        }
    }

    /// Set the active (selected) note.
    pub fn set_active(&mut self, id: Option<NoteId>) {
        self.active_note = id;
    }

    pub fn active_note(&self) -> Option<NoteId> {
        self.active_note
    }

    /// Toggle sidebar visibility.
    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
    }

    pub fn sidebar_visible(&self) -> bool {
        self.sidebar_visible
    }

    pub fn set_sidebar_visible(&mut self, visible: bool) {
        self.sidebar_visible = visible;
    }

    pub fn snap_to_grid_enabled(&self) -> bool {
        self.snap_to_grid
    }

    pub fn set_snap_to_grid(&mut self, enabled: bool) {
        self.snap_to_grid = enabled;
    }

    pub fn is_dirty(&self) -> bool {
        self.auto_save_dirty
    }

    pub fn mark_clean(&mut self) {
        self.auto_save_dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.auto_save_dirty = true;
    }

    pub fn drag_state(&self) -> &DragState {
        &self.drag
    }

    pub fn set_drag(&mut self, drag: DragState) {
        self.drag = drag;
    }

    /// Begin dragging a note (move).
    pub fn begin_move(&mut self, id: NoteId, mouse_x: f32, mouse_y: f32) {
        if let Some(note) = self.get_note(id) {
            if note.pinned {
                return;
            }
            let offset_x = mouse_x - note.x;
            let offset_y = mouse_y - note.y;
            self.drag = DragState::Moving {
                note_id: id,
                offset_x,
                offset_y,
            };
        }
    }

    /// Begin resizing a note.
    pub fn begin_resize(&mut self, id: NoteId, mouse_x: f32, mouse_y: f32) {
        if let Some(note) = self.get_note(id) {
            self.drag = DragState::Resizing {
                note_id: id,
                start_w: note.width,
                start_h: note.height,
                start_mx: mouse_x,
                start_my: mouse_y,
            };
        }
    }

    /// Update drag (mouse moved).
    pub fn update_drag(&mut self, mouse_x: f32, mouse_y: f32) {
        let snap = self.snap_to_grid;
        match self.drag.clone() {
            DragState::Moving {
                note_id,
                offset_x,
                offset_y,
            } => {
                let x = maybe_snap(mouse_x - offset_x, snap);
                let y = maybe_snap(mouse_y - offset_y, snap);
                if let Some(note) = self.get_note_mut(note_id) {
                    note.set_position(x, y);
                    self.auto_save_dirty = true;
                }
            }
            DragState::Resizing {
                note_id,
                start_w,
                start_h,
                start_mx,
                start_my,
            } => {
                let dw = mouse_x - start_mx;
                let dh = mouse_y - start_my;
                let w = maybe_snap(start_w + dw, snap);
                let h = maybe_snap(start_h + dh, snap);
                if let Some(note) = self.get_note_mut(note_id) {
                    note.set_size(w, h);
                    self.auto_save_dirty = true;
                }
            }
            DragState::None => {}
        }
    }

    /// End the current drag operation.
    pub fn end_drag(&mut self) {
        self.drag = DragState::None;
    }

    /// Get a compact sidebar list of all visible notes (title + first line preview).
    pub fn sidebar_items(&self) -> Vec<SidebarItem> {
        let mut items: Vec<SidebarItem> = self
            .notes
            .iter()
            .filter(|n| !n.archived && n.matches_search(&self.search_query))
            .map(SidebarItem::from_note)
            .collect();
        items.sort_by_key(|item| std::cmp::Reverse(item.pinned));
        items
    }
}

impl Default for NoteStore {
    fn default() -> Self {
        Self::new()
    }
}

/// A compact sidebar entry.
#[derive(Clone, Debug)]
pub struct SidebarItem {
    pub id: NoteId,
    pub title: String,
    pub preview: String,
    pub color_index: NoteColorIndex,
    pub pinned: bool,
    pub tag_count: usize,
}

impl SidebarItem {
    /// Summarise one note for the list.
    ///
    /// Shared with the archive view, which lists notes `sidebar_items` filters
    /// out by definition — two copies of this would be two ways for the two
    /// lists to disagree about what a row says.
    #[must_use]
    pub fn from_note(note: &Note) -> Self {
        Self {
            id: note.id,
            title: note.title.clone(),
            // The first body line, in full. Truncation is the sidebar's job,
            // not the store's: `&text[..37]` aborts whenever byte 37 lands
            // inside a multi-byte character — and the `len() > 40` guard made
            // that *more* likely rather than less, since a 13-character
            // Japanese note is 39 bytes and so always took the truncating
            // branch. The 40-byte budget also bore no relation to the
            // sidebar's actual width, which the store has no way to know. The
            // renderer elides it against the real width instead.
            preview: note
                .body
                .first()
                .map_or(String::new(), RichLine::plain_text),
            color_index: note.color_index,
            pinned: note.pinned,
            tag_count: note.tags.len(),
        }
    }
}

// ============================================================================
// Persistence — pipe-delimited text format
// ============================================================================

/// Serialize all notes to a pipe-delimited text string.
///
/// Format per note line:
/// `id|title|x|y|width|height|color_idx|pinned|archived|z_order|font_size|tags(comma-sep)|body(\\n-escaped)`
pub fn serialize_notes(store: &NoteStore) -> String {
    let mut lines = Vec::new();
    // Header with store settings.
    lines.push(format!(
        "STICKYNOTES|1|snap={}|sidebar={}",
        store.snap_to_grid_enabled(),
        store.sidebar_visible()
    ));
    for note in &store.notes {
        let tags_str = note.tags.join(",");
        let body_str = note
            .body_text()
            .replace('\\', "\\\\")
            .replace('|', "\\p")
            .replace('\n', "\\n");
        let title_str = note.title.replace('\\', "\\\\").replace('|', "\\p");
        lines.push(format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            note.id,
            title_str,
            note.x,
            note.y,
            note.width,
            note.height,
            note.color_index.as_usize(),
            note.pinned,
            note.archived,
            note.z_order,
            note.font_size.name(),
            tags_str,
            body_str,
        ));
    }
    lines.join("\n")
}

/// Deserialize notes from the pipe-delimited text format.
pub fn deserialize_notes(data: &str) -> Option<NoteStore> {
    let mut store = NoteStore::new();
    let mut lines = data.lines();

    // Parse header.
    let header = lines.next()?;
    let header_parts: Vec<&str> = header.split('|').collect();
    if header_parts.first().copied() != Some("STICKYNOTES") {
        return None;
    }
    // Parse header settings.
    for part in header_parts.iter().skip(2) {
        if let Some(val) = part.strip_prefix("snap=") {
            store.set_snap_to_grid(val == "true");
        } else if let Some(val) = part.strip_prefix("sidebar=") {
            store.set_sidebar_visible(val == "true");
        }
    }

    let mut max_id: NoteId = 0;
    let mut max_z: u32 = 0;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(13, '|').collect();
        // A slice pattern rather than `parts.len() < 13` and thirteen indexes:
        // the two say the same thing, but only one of them says it to the
        // compiler, and the indexing version is a panic away from a note file
        // that crashes the program that is supposed to open it.
        let [
            id_str,
            title_str,
            x_str,
            y_str,
            width_str,
            height_str,
            color_str,
            pinned_str,
            archived_str,
            z_str,
            font_size_str,
            tags_str,
            body_field,
        ] = parts.as_slice()
        else {
            continue;
        };
        // A malformed field skips its own note and keeps the rest.
        //
        // This used to be `?`, which abandoned the whole file: one corrupt
        // line — a half-written save from before this app wrote atomically, a
        // hand-edit, a truncating filesystem — and `with_storage` came back
        // empty, at which point the next autosave wrote that emptiness over
        // the notes that were still there. Losing one note to a bad line is a
        // bug; losing all of them to it is unrecoverable.
        let Some(parsed) = (|| {
            Some((
                id_str.parse::<NoteId>().ok()?,
                x_str.parse::<f32>().ok()?,
                y_str.parse::<f32>().ok()?,
                width_str.parse::<f32>().ok()?,
                height_str.parse::<f32>().ok()?,
                color_str.parse::<usize>().ok()?,
                pinned_str.parse::<bool>().ok()?,
                archived_str.parse::<bool>().ok()?,
                z_str.parse::<u32>().ok()?,
            ))
        })() else {
            continue;
        };
        let (id, x, y, width, height, color_idx, pinned, archived, z_order) = parsed;
        let title = title_str
            .replace("\\\\", "\x00")
            .replace("\\p", "|")
            .replace('\x00', "\\");
        let body_str = body_field
            .replace("\\\\", "\x00")
            .replace("\\n", "\n")
            .replace("\\p", "|")
            .replace('\x00', "\\");

        let color_index = NoteColorIndex::from_usize(color_idx).unwrap_or(NoteColorIndex::Yellow);
        let font_size =
            FontSizePreset::parse_label(font_size_str).unwrap_or(FontSizePreset::Medium);
        let tags: Vec<String> = if tags_str.is_empty() {
            Vec::new()
        } else {
            tags_str.split(',').map(|s| s.to_string()).collect()
        };

        let mut note = Note::new(id, x, y);
        note.title = title;
        note.width = width;
        note.height = height;
        note.color_index = color_index;
        note.pinned = pinned;
        note.archived = archived;
        note.z_order = z_order;
        note.font_size = font_size;
        note.tags = tags;
        note.set_body_from_text(&body_str);

        if id > max_id {
            max_id = id;
        }
        if z_order > max_z {
            max_z = z_order;
        }
        store.notes.push(note);
    }
    store.next_id = max_id.wrapping_add(1);
    store.next_z = max_z.wrapping_add(1);
    Some(store)
}

// ============================================================================
// Export
// ============================================================================

/// Export all non-archived notes as human-readable text.
pub fn export_notes_as_text(store: &NoteStore) -> String {
    let mut out = String::from("=== Sticky Notes Export ===\n\n");
    for note in store.visible_notes() {
        out.push_str(&format!("--- {} ---\n", note.title));
        if !note.tags.is_empty() {
            out.push_str(&format!("Tags: {}\n", note.tags.join(", ")));
        }
        out.push_str(&format!("Color: {}\n", note.color_index.name()));
        if note.pinned {
            out.push_str("Pinned: yes\n");
        }
        out.push_str(&note.body_text());
        out.push_str("\n\n");
    }
    out
}

// ============================================================================
// Auto-save timer (tick-based)
// ============================================================================

/// Auto-save state: tracks elapsed time since last save.
pub struct AutoSave {
    elapsed_ms: u64,
    interval_ms: u64,
}

impl AutoSave {
    pub fn new(interval_ms: u64) -> Self {
        Self {
            elapsed_ms: 0,
            interval_ms,
        }
    }

    /// Tick the timer. Returns true if it is time to save.
    pub fn tick(&mut self, delta_ms: u64, dirty: bool) -> bool {
        if !dirty {
            self.elapsed_ms = 0;
            return false;
        }
        self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);
        if self.elapsed_ms >= self.interval_ms {
            self.elapsed_ms = 0;
            true
        } else {
            false
        }
    }

    pub fn interval_ms(&self) -> u64 {
        self.interval_ms
    }

    pub fn set_interval(&mut self, ms: u64) {
        self.interval_ms = ms;
    }

    pub fn reset(&mut self) {
        self.elapsed_ms = 0;
    }
}

// ============================================================================
// Window layout
// ============================================================================

/// The window's starting size, and the size the probe draws at.
const WINDOW_WIDTH: f32 = 940.0;
const WINDOW_HEIGHT: f32 = 640.0;
/// Below this the toolbar has nowhere to put a chip and the canvas no room for
/// a note. The frame is drawn at this size and clipped by the compositor rather
/// than collapsing every control on top of every other one.
const MIN_WIDTH: f32 = 480.0;
const MIN_HEIGHT: f32 = 320.0;

/// Title bar height.
const TITLE_BAR_HEIGHT: f32 = 30.0;
/// Resize handle size.
const RESIZE_HANDLE: f32 = 16.0;
/// Sidebar width.
const SIDEBAR_WIDTH: f32 = 240.0;
/// Search bar height.
const SEARCH_BAR_HEIGHT: f32 = 36.0;
/// The strip of tag chips under the search box, drawn only when tags exist.
const TAG_STRIP_H: f32 = 28.0;
/// Height of one sidebar row.
const SIDEBAR_ITEM_H: f32 = 52.0;
/// The bar along the bottom of the window.
const TOOLBAR_H: f32 = 32.0;
const CHIP_H: f32 = 24.0;
const CHIP_PAD: f32 = 8.0;
const CHIP_GAP: f32 = 6.0;
const TOOLBAR_FONT: f32 = 12.0;
/// Padding inside a note, left and right.
const NOTE_PAD: f32 = 8.0;
/// Marker left on sidebar text that did not fit. Without one, a title cut by
/// `max_width` simply looks like a shorter title.
const ELLIPSIS: &str = "…";
const SIDEBAR_TITLE_SIZE: f32 = 13.0;
const SIDEBAR_PREVIEW_SIZE: f32 = 11.0;
/// How much of a note must stay inside the canvas. A note dragged past the
/// edge is not merely off-centre: nothing else can bring it back, because the
/// only handle it has is the title bar the drag just pushed out of reach.
const KEEP_ON_SCREEN: f32 = 60.0;
/// Caps on what the keyboard alone can grow. A note is a note, not a document.
const MAX_TITLE_LEN: usize = 80;
const MAX_LINE_LEN: usize = 512;
/// How often the window ticks — but only while a change is waiting to be
/// written; see [`StickyNotesApp::tick_interval`].
const TICK: Duration = Duration::from_millis(500);
/// How long an edit waits before it is saved.
const AUTOSAVE_MS: u64 = 5_000;
/// The prefix that turns the search box into a tag filter.
const TAG_PREFIX: &str = "tag:";

/// Everything in the window a click can land on.
///
/// Recorded by the renderer as it paints and read back by the click router;
/// see [`guitk::frame`] for why the two are one walk rather than two
/// descriptions of the same geometry. Notes are named by **id and not by
/// position**: raising a note reorders every one below it, so an index
/// recorded here would address a different note than the one that was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The empty canvas behind the notes. Clicking it deselects and drops the
    /// caret.
    Desktop,
    /// A note's title bar: pressing it selects, raises and begins a move;
    /// double-clicking it puts the caret in the title.
    NoteTitle(NoteId),
    /// The button at the right of a title bar. It *archives* rather than
    /// deletes: a sticky note is the user's own writing and the gesture that
    /// gets rid of one is a single click on a small target with no
    /// confirmation, so it has to be a gesture that can be taken back. The
    /// archive view restores it; the toolbar's Delete is the one that does not
    /// come back.
    NoteArchive(NoteId),
    /// One body line, by note id and line index.
    NoteLine(NoteId, usize),
    /// The `[ ]` at the head of a checkbox line, recorded on top of the line
    /// so that clicking the box ticks it and clicking the words does not.
    NoteCheck(NoteId, usize),
    /// The grip in the bottom-right corner.
    NoteGrip(NoteId),
    /// The search box in the sidebar header.
    SearchBox,
    /// A tag chip under the search box; clicking it filters by that tag.
    /// Indexed into [`NoteStore::all_tags`], which the hit-test recomputes
    /// identically — the store the frame read and the store the click reads
    /// are the same store.
    TagChip(usize),
    /// A sidebar row.
    SidebarItem(NoteId),
    /// Toolbar: acts on no note in particular.
    NewNote,
    ToggleGrid,
    ToggleSidebar,
    ToggleArchiveView,
    /// Toolbar: acts on the active note.
    CycleColor,
    CycleFontSize,
    CycleLineKind,
    TogglePin,
    DeleteNote,
    Undo,
    Redo,
}

/// The text box holding the caret, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Search,
    Title(NoteId),
    Body(NoteId),
}

/// What the window should do after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing visible changed; do not spend a frame saying so.
    None,
    Redraw,
    Quit,
}

/// A frame of this program's controls.
pub type Frame = guitk::frame::Frame<Target>;

/// Where the caret sits inside the note currently being drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Caret {
    /// A byte offset into the title.
    Title(usize),
    /// A line index and a byte offset into that line.
    Body(usize, usize),
}

// ============================================================================
// Note geometry — one description, used by the renderer and the caret alike
// ============================================================================

/// The marker drawn at the head of a body line.
fn line_prefix(kind: &LineKind) -> &'static str {
    match kind {
        LineKind::Plain => "",
        LineKind::Bullet => "• ",
        LineKind::Checkbox { checked: true } => "[x] ",
        LineKind::Checkbox { checked: false } => "[ ] ",
    }
}

/// The next kind in the cycle, for the key and the chip that change a line.
fn next_line_kind(kind: &LineKind) -> LineKind {
    match kind {
        LineKind::Plain => LineKind::Bullet,
        LineKind::Bullet => LineKind::Checkbox { checked: false },
        LineKind::Checkbox { .. } => LineKind::Plain,
    }
}

fn body_line_height(font: f32) -> f32 {
    font * 1.5
}

/// The y of the note's first body line.
fn body_top(note: &Note) -> f32 {
    note.y + TITLE_BAR_HEIGHT + 6.0
}

/// How many body lines fit between the title bar and the resize grip.
///
/// Lines past this are not drawn, and — because a hit box is only recorded as
/// a line is painted — not clickable either, which is the property that keeps
/// a click from landing on text nobody can see.
fn body_visible_lines(note: &Note) -> usize {
    let lh = body_line_height(note.font_size.size());
    let room = note.height - TITLE_BAR_HEIGHT - RESIZE_HANDLE - 6.0;
    if lh <= 0.0 || room <= 0.0 {
        return 0;
    }
    (room / lh).floor().max(0.0) as usize
}

/// The x at which a body line's own text begins, after its marker.
fn body_text_x(note: &Note, kind: &LineKind) -> f32 {
    let prefix = line_prefix(kind);
    note.x + NOTE_PAD + text::measure(prefix, note.font_size.size(), FontWeightHint::Regular)
}

/// The byte offset in `text` nearest to `x`, given text drawn from `origin`.
///
/// [`text::fit`] breaks between glyphs, so the result is always a character
/// boundary — which is what `String::insert` needs and what makes clicking
/// into an accented or CJK note an edit rather than an abort.
fn column_at(text_str: &str, origin: f32, x: f32, size: f32, weight: FontWeightHint) -> usize {
    text::fit(text_str, x - origin, size, weight)
}

// ============================================================================
// Rendering
// ============================================================================

/// Draw one sticky note, recording every part of it a click can land on.
fn draw_note(frame: &mut Frame, note: &Note, is_active: bool, caret: Option<Caret>) {
    let palette = note.palette();
    let corner = CornerRadii::all(8.0);
    let font = note.font_size.size();
    let title_font = note.font_size.title_size();

    if is_active {
        frame.push(RenderCommand::BoxShadow {
            x: note.x,
            y: note.y,
            width: note.width,
            height: note.height,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 12.0,
            spread: 2.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: corner,
        });
    }

    // Body background.
    frame.push(RenderCommand::FillRect {
        x: note.x,
        y: note.y,
        width: note.width,
        height: note.height,
        color: palette.dark,
        corner_radii: corner,
    });

    // Title bar. Recorded before the controls drawn on it, so those win.
    frame.push(RenderCommand::FillRect {
        x: note.x,
        y: note.y,
        width: note.width,
        height: TITLE_BAR_HEIGHT,
        color: palette.light,
        corner_radii: CornerRadii {
            top_left: 8.0,
            top_right: 8.0,
            bottom_left: 0.0,
            bottom_right: 0.0,
        },
    });
    frame.hit(
        Target::NoteTitle(note.id),
        Rect::new(note.x, note.y, note.width, TITLE_BAR_HEIGHT),
    );

    let title_x = note.x + NOTE_PAD;
    let title_room = (note.width - 40.0).max(0.0);
    let title_display = if note.pinned {
        format!("[P] {}", note.title)
    } else {
        note.title.clone()
    };
    frame.push(RenderCommand::Text {
        x: title_x,
        y: note.y + 6.0,
        text: title_display,
        color: MANTLE,
        font_size: title_font,
        font_weight: FontWeightHint::Bold,
        max_width: Some(title_room),
        overflow: TextOverflow::Ellipsis,
    });

    if let Some(Caret::Title(col)) = caret {
        // Measured against the *unprefixed* title, then shifted by the pin
        // marker: the caret indexes what the user is editing, and the `[P] `
        // is drawn by the renderer rather than typed.
        let lead = if note.pinned {
            text::measure("[P] ", title_font, FontWeightHint::Bold)
        } else {
            0.0
        };
        let cut = Note::snap_col(&note.title, col);
        let cx = title_x
            + lead
            + text::measure(
                note.title.get(..cut).unwrap_or(""),
                title_font,
                FontWeightHint::Bold,
            );
        frame.push(RenderCommand::Line {
            x1: cx,
            y1: note.y + 5.0,
            x2: cx,
            y2: note.y + 5.0 + title_font * 1.2,
            color: MANTLE,
            width: 1.5,
        });
    }

    // Archive button.
    let btn = Rect::new(note.x + note.width - 24.0, note.y + 4.0, 20.0, 22.0);
    frame.push(RenderCommand::Text {
        x: btn.x + 5.0,
        y: btn.y + 2.0,
        text: String::from("X"),
        color: Color::rgba(0, 0, 0, 150),
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    frame.hit(Target::NoteArchive(note.id), btn);

    // Body.
    let lh = body_line_height(font);
    let top = body_top(note);
    let max_lines = body_visible_lines(note);
    for (i, line) in note.body.iter().enumerate() {
        if i >= max_lines {
            break;
        }
        let ly = top + i as f32 * lh;
        frame.hit(
            Target::NoteLine(note.id, i),
            Rect::new(note.x, ly, note.width, lh),
        );

        let prefix = line_prefix(&line.kind);
        let text_x = body_text_x(note, &line.kind);
        if !prefix.is_empty() {
            frame.push(RenderCommand::Text {
                x: note.x + NOTE_PAD,
                y: ly,
                text: prefix.to_string(),
                color: SUBTEXT0,
                font_size: font,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            if matches!(line.kind, LineKind::Checkbox { .. }) {
                // On top of the row, so ticking a box and editing its words
                // are different clicks rather than the same one.
                frame.hit(
                    Target::NoteCheck(note.id, i),
                    Rect::new(note.x + NOTE_PAD, ly, text_x - note.x - NOTE_PAD, lh),
                );
            }
        }

        let content = line.plain_text();
        let weight = if line.spans.first().is_some_and(|s| s.bold) {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        };
        frame.push(RenderCommand::Text {
            x: text_x,
            y: ly,
            text: content.clone(),
            color: TEXT_COLOR,
            font_size: font,
            font_weight: weight,
            max_width: Some((note.x + note.width - NOTE_PAD - text_x).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        if let Some(Caret::Body(cl, cc)) = caret
            && cl == i
        {
            let cut = Note::snap_col(&content, cc);
            let cx = text_x + text::measure(content.get(..cut).unwrap_or(""), font, weight);
            frame.push(RenderCommand::Line {
                x1: cx,
                y1: ly,
                x2: cx,
                y2: ly + font * 1.2,
                color: TEXT_COLOR,
                width: 1.0,
            });
        }
    }

    // Resize grip, recorded last so it wins over the body line behind it.
    frame.push(RenderCommand::Line {
        x1: note.x + note.width - 4.0,
        y1: note.y + note.height - RESIZE_HANDLE,
        x2: note.x + note.width - RESIZE_HANDLE,
        y2: note.y + note.height - 4.0,
        color: Color::rgba(palette.light.r, palette.light.g, palette.light.b, 120),
        width: 2.0,
    });
    frame.hit(
        Target::NoteGrip(note.id),
        Rect::new(
            note.x + note.width - RESIZE_HANDLE,
            note.y + note.height - RESIZE_HANDLE,
            RESIZE_HANDLE,
            RESIZE_HANDLE,
        ),
    );

    if is_active {
        frame.push(RenderCommand::StrokeRect {
            x: note.x,
            y: note.y,
            width: note.width,
            height: note.height,
            color: BLUE,
            line_width: 2.0,
            corner_radii: corner,
        });
    }
}

/// Draw one chip, and record it if it is something to click.
fn draw_chip(
    frame: &mut Frame,
    rect: Rect,
    label: &str,
    fg: Color,
    bg: Option<Color>,
    target: Option<Target>,
) {
    if let Some(bg) = bg {
        frame.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });
    }
    frame.push(RenderCommand::Text {
        x: rect.x + CHIP_PAD,
        y: rect.y + (rect.h - TOOLBAR_FONT * 1.2) / 2.0,
        text: label.to_string(),
        color: fg,
        font_size: TOOLBAR_FONT,
        font_weight: FontWeightHint::Regular,
        max_width: Some((rect.w - CHIP_PAD * 2.0).max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });
    if let Some(target) = target {
        frame.hit(target, rect);
    }
}

fn chip_width(label: &str) -> f32 {
    text::padded_width(label, CHIP_PAD, TOOLBAR_FONT, FontWeightHint::Regular)
}

// ============================================================================
// The application
// ============================================================================

/// The sticky-notes window: the store, the caret, and where the notes are kept.
pub struct StickyNotesApp {
    pub store: NoteStore,
    autosave: AutoSave,
    /// The size the compositor last drew at. A remembered size is only ever a
    /// starting guess — [`App::render`] writes the real one back.
    window_size: (f32, f32),
    focus: Option<Focus>,
    /// The caret, as a line index and a byte offset into that line. The line
    /// index is unused while the caret is in a title or in the search box.
    caret: (usize, usize),
    /// The title as it read when the caret entered it, so that leaving the
    /// title records **one** undoable change rather than one per keystroke.
    title_before: String,
    sidebar_scroll: f32,
    /// While set, the sidebar lists archived notes and clicking one restores
    /// it. This is the only route back from the title bar's X.
    archive_view: bool,
    /// Where the notes are written. `None` in tests, which must never write
    /// into the developer's own configuration directory — see the module docs
    /// on [`gui/settingsfile`](settingsfile) for why that matters.
    storage: Option<PathBuf>,
    /// What the last save did, shown on the toolbar. A save that fails
    /// silently is how a day of notes disappears.
    status: String,
    saves: u32,
}

impl Default for StickyNotesApp {
    fn default() -> Self {
        Self::new()
    }
}

impl StickyNotesApp {
    /// An empty desktop that writes nowhere.
    #[must_use]
    pub fn new() -> Self {
        Self {
            store: NoteStore::new(),
            autosave: AutoSave::new(AUTOSAVE_MS),
            window_size: (WINDOW_WIDTH, WINDOW_HEIGHT),
            focus: None,
            caret: (0, 0),
            title_before: String::new(),
            sidebar_scroll: 0.0,
            archive_view: false,
            storage: None,
            status: String::new(),
            saves: 0,
        }
    }

    /// The desktop as a real user's session starts it: whatever was saved last
    /// time, and somewhere to write it back to.
    ///
    /// A file that is missing or unreadable yields an empty desktop rather than
    /// an error — "has never run before" is the ordinary case, not a failure.
    #[must_use]
    pub fn with_storage(path: PathBuf) -> Self {
        let mut app = Self::new();
        if let Ok(text) = fs::read_to_string(&path)
            && let Some(store) = deserialize_notes(&text)
        {
            app.store = store;
            app.store.mark_clean();
        }
        app.storage = Some(path);
        app
    }

    /// How many saves have gone through since this window opened.
    #[must_use]
    pub fn saves(&self) -> u32 {
        self.saves
    }

    /// The toolbar's last word on saving.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    // -- Persistence -------------------------------------------------------

    /// Write the notes out now.
    ///
    /// The store is marked clean **only on success**, so a failed save is
    /// retried at the next autosave rather than being forgotten.
    pub fn persist(&mut self) -> Action {
        let Some(path) = self.storage.clone() else {
            return Action::None;
        };
        match write_store(&path, &self.store) {
            Ok(()) => {
                self.store.mark_clean();
                self.saves = self.saves.saturating_add(1);
                self.status = String::from("Saved");
            }
            Err(err) => self.status = format!("Save failed: {err}"),
        }
        Action::Redraw
    }

    /// Write a plain-text copy of every live note beside the notes file.
    fn export(&mut self) -> Action {
        let Some(path) = self
            .storage
            .as_ref()
            .map(|p| p.with_extension("export.txt"))
        else {
            return Action::None;
        };
        match write_bytes(&path, export_notes_as_text(&self.store).as_bytes()) {
            Ok(()) => self.status = format!("Exported to {}", path.display()),
            Err(err) => self.status = format!("Export failed: {err}"),
        }
        Action::Redraw
    }

    // -- Layout ------------------------------------------------------------

    /// The size the frame is really drawn at.
    fn clamped(size: (f32, f32)) -> (f32, f32) {
        (size.0.max(MIN_WIDTH), size.1.max(MIN_HEIGHT))
    }

    fn sidebar_width(&self) -> f32 {
        if self.store.sidebar_visible() {
            SIDEBAR_WIDTH
        } else {
            0.0
        }
    }

    /// The region the notes live in.
    ///
    /// Note coordinates are relative to this rather than to the window, so
    /// opening the sidebar reveals the notes beside it instead of covering
    /// them — and a note cannot end up under the sidebar, where it would still
    /// be clickable while being invisible.
    #[must_use]
    pub fn canvas_rect(&self, width: f32, height: f32) -> Rect {
        let (width, height) = Self::clamped((width, height));
        let left = self.sidebar_width();
        Rect::new(
            left,
            0.0,
            (width - left).max(0.0),
            (height - TOOLBAR_H).max(0.0),
        )
    }

    /// The height the sidebar list would take if nothing clipped it.
    fn sidebar_content_height(&self) -> f32 {
        self.sidebar_rows().len() as f32 * SIDEBAR_ITEM_H
    }

    fn sidebar_list_rect(&self, height: f32) -> Rect {
        let top = SEARCH_BAR_HEIGHT + self.tag_strip_height();
        Rect::new(0.0, top, SIDEBAR_WIDTH, (height - top).max(0.0))
    }

    fn tag_strip_height(&self) -> f32 {
        if self.store.all_tags().is_empty() {
            0.0
        } else {
            TAG_STRIP_H
        }
    }

    /// The rows the sidebar lists: the live notes, or the archived ones while
    /// the archive view is on.
    fn sidebar_rows(&self) -> Vec<SidebarItem> {
        if self.archive_view {
            self.store
                .archived_notes()
                .into_iter()
                .map(SidebarItem::from_note)
                .collect()
        } else {
            self.store.sidebar_items()
        }
    }

    /// Pull the sidebar back when the list it was scrolled through shrinks.
    fn clamp_scroll(&mut self, height: f32) {
        let (_, height) = Self::clamped((MIN_WIDTH, height));
        let viewport = self.sidebar_list_rect(height).h;
        let overflow = (self.sidebar_content_height() - viewport).max(0.0);
        self.sidebar_scroll = self.sidebar_scroll.max(0.0).min(overflow);
    }

    /// Keep a note's title bar reachable after a move or a resize.
    fn clamp_note(&mut self, id: NoteId, canvas: Rect) {
        let Some(note) = self.store.get_note_mut(id) else {
            return;
        };
        let min_x = KEEP_ON_SCREEN - note.width;
        let max_x = (canvas.w - KEEP_ON_SCREEN).max(min_x);
        let max_y = (canvas.h - TITLE_BAR_HEIGHT).max(0.0);
        note.x = note.x.max(min_x).min(max_x);
        note.y = note.y.max(0.0).min(max_y);
    }

    // -- Rendering ---------------------------------------------------------

    /// Draw the whole window at `width` x `height`, recording a rectangle for
    /// every control as it is painted.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let (width, height) = Self::clamped((width, height));
        let mut frame = Frame::new(width, height);

        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        let canvas = self.canvas_rect(width, height);
        // Recorded before the notes, so every note drawn onto it wins.
        frame.hit(Target::Desktop, canvas);

        frame.clip(canvas);
        frame.translate(canvas.x, canvas.y);
        let active = self.store.active_note();
        let notes = self.store.visible_notes();
        // Unpinned first, then pinned, so a pinned note is both drawn on top
        // and — being recorded later — clicked on top.
        for pinned in [false, true] {
            for note in &notes {
                if note.pinned != pinned {
                    continue;
                }
                draw_note(&mut frame, note, active == Some(note.id), {
                    match self.focus {
                        Some(Focus::Title(id)) if id == note.id => Some(Caret::Title(self.caret.1)),
                        Some(Focus::Body(id)) if id == note.id => {
                            Some(Caret::Body(self.caret.0, self.caret.1))
                        }
                        _ => None,
                    }
                });
            }
        }
        frame.untranslate();
        frame.unclip();

        self.draw_sidebar(&mut frame, height);
        self.draw_toolbar(&mut frame, width, height);
        frame
    }

    fn draw_sidebar(&self, frame: &mut Frame, height: f32) {
        if !self.store.sidebar_visible() {
            return;
        }

        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: SIDEBAR_WIDTH,
            height,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Search box.
        let searching = self.focus == Some(Focus::Search);
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: SIDEBAR_WIDTH,
            height: SEARCH_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        if searching {
            frame.push(RenderCommand::StrokeRect {
                x: 1.0,
                y: 1.0,
                width: SIDEBAR_WIDTH - 2.0,
                height: SEARCH_BAR_HEIGHT - 2.0,
                color: BLUE,
                line_width: 1.0,
                corner_radii: CornerRadii::ZERO,
            });
        }
        let query = self.store.search_query();
        let (search_text, search_color) = if query.is_empty() && !searching {
            (String::from("Search notes..."), OVERLAY0)
        } else {
            (query.to_string(), TEXT_COLOR)
        };
        frame.push(RenderCommand::Text {
            x: 10.0,
            y: 10.0,
            text: search_text,
            color: search_color,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(SIDEBAR_WIDTH - 20.0),
            overflow: TextOverflow::Ellipsis,
        });
        if searching {
            let cut = query.len().min(self.caret.1);
            let cx = 10.0
                + text::measure(
                    query.get(..cut).unwrap_or(query),
                    13.0,
                    FontWeightHint::Regular,
                );
            frame.push(RenderCommand::Line {
                x1: cx,
                y1: 9.0,
                x2: cx,
                y2: 27.0,
                color: TEXT_COLOR,
                width: 1.0,
            });
        }
        frame.hit(
            Target::SearchBox,
            Rect::new(0.0, 0.0, SIDEBAR_WIDTH, SEARCH_BAR_HEIGHT),
        );

        // Tag chips.
        let tags = self.store.all_tags();
        if !tags.is_empty() {
            frame.clip(Rect::new(
                0.0,
                SEARCH_BAR_HEIGHT,
                SIDEBAR_WIDTH,
                TAG_STRIP_H,
            ));
            let mut x = 6.0;
            for (i, tag) in tags.iter().enumerate() {
                let label = format!("{} ({})", tag, self.store.notes_with_tag(tag).len());
                let w = chip_width(&label);
                if x + w > SIDEBAR_WIDTH {
                    break;
                }
                let filter = format!("{TAG_PREFIX}{tag}");
                let selected = self.store.search_query() == filter;
                draw_chip(
                    frame,
                    Rect::new(x, SEARCH_BAR_HEIGHT + 3.0, w, TAG_STRIP_H - 6.0),
                    &label,
                    if selected { MANTLE } else { SUBTEXT0 },
                    Some(if selected { BLUE } else { SURFACE1 }),
                    Some(Target::TagChip(i)),
                );
                x += w + CHIP_GAP;
            }
            frame.unclip();
        }

        // The list itself, clipped and scrolled: a row scrolled out of the
        // pane is not drawn, and so is not there to be clicked.
        let list = self.sidebar_list_rect(height);
        frame.clip(list);
        frame.translate(list.x, list.y - self.sidebar_scroll);
        let rows = self.sidebar_rows();
        for (i, item) in rows.iter().enumerate() {
            let iy = i as f32 * SIDEBAR_ITEM_H;
            let selected = self.store.active_note() == Some(item.id);
            frame.push(RenderCommand::FillRect {
                x: 0.0,
                y: iy,
                width: SIDEBAR_WIDTH,
                height: SIDEBAR_ITEM_H,
                color: if selected { BASE } else { SURFACE0 },
                corner_radii: CornerRadii::ZERO,
            });
            frame.push(RenderCommand::FillRect {
                x: 8.0,
                y: iy + 8.0,
                width: 10.0,
                height: 10.0,
                color: note_palette(item.color_index).light,
                corner_radii: CornerRadii::all(5.0),
            });

            // Title and preview are elided rather than clipped by `max_width`,
            // which drops whole glyphs off the end with no marker — so a cut
            // title looks like a shorter title.
            let text_x = 24.0;
            let room = (SIDEBAR_WIDTH - 8.0 - text_x).max(0.0);
            let prefix = if item.pinned { "[P] " } else { "" };
            frame.push(RenderCommand::Text {
                x: text_x,
                y: iy + 6.0,
                text: text::elide(
                    &format!("{}{}", prefix, item.title),
                    room,
                    ELLIPSIS,
                    SIDEBAR_TITLE_SIZE,
                    FontWeightHint::Bold,
                ),
                color: TEXT_COLOR,
                font_size: SIDEBAR_TITLE_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(room),
                overflow: TextOverflow::Ellipsis,
            });
            let preview = if item.tag_count == 0 {
                item.preview.clone()
            } else {
                format!("{}  ({} tags)", item.preview, item.tag_count)
            };
            frame.push(RenderCommand::Text {
                x: text_x,
                y: iy + 24.0,
                text: text::elide(
                    &preview,
                    room,
                    ELLIPSIS,
                    SIDEBAR_PREVIEW_SIZE,
                    FontWeightHint::Regular,
                ),
                color: SUBTEXT0,
                font_size: SIDEBAR_PREVIEW_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(room),
                overflow: TextOverflow::Ellipsis,
            });
            frame.push(RenderCommand::Line {
                x1: 8.0,
                y1: iy + SIDEBAR_ITEM_H - 1.0,
                x2: SIDEBAR_WIDTH - 8.0,
                y2: iy + SIDEBAR_ITEM_H - 1.0,
                color: OVERLAY0,
                width: 1.0,
            });
            frame.hit(
                Target::SidebarItem(item.id),
                Rect::new(0.0, iy, SIDEBAR_WIDTH, SIDEBAR_ITEM_H),
            );
        }
        if rows.is_empty() {
            frame.push(RenderCommand::Text {
                x: 12.0,
                y: 12.0,
                text: String::from(if self.archive_view {
                    "Nothing archived"
                } else {
                    "No notes match"
                }),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        frame.untranslate();
        frame.unclip();
    }

    fn draw_toolbar(&self, frame: &mut Frame, width: f32, height: f32) {
        let bar_y = height - TOOLBAR_H;
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width,
            height: TOOLBAR_H,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        let chip_y = bar_y + (TOOLBAR_H - CHIP_H) / 2.0;

        // The right-hand toggles are laid out first, from the right edge, so
        // the left-hand actions know where they have to stop.
        let archived = self.store.archived_count();
        let right_chips = [
            (
                format!("Sidebar: {}", on_off(self.store.sidebar_visible())),
                Target::ToggleSidebar,
                self.store.sidebar_visible(),
            ),
            (
                format!("Grid: {}", on_off(self.store.snap_to_grid_enabled())),
                Target::ToggleGrid,
                self.store.snap_to_grid_enabled(),
            ),
            (
                format!("Archived ({archived})"),
                Target::ToggleArchiveView,
                self.archive_view,
            ),
        ];
        let mut right = width - CHIP_GAP;
        for (label, target, on) in &right_chips {
            let w = chip_width(label);
            let x = right - w;
            if x < 0.0 {
                break;
            }
            draw_chip(
                frame,
                Rect::new(x, chip_y, w, CHIP_H),
                label,
                if *on { MANTLE } else { OVERLAY0 },
                if *on { Some(BLUE) } else { None },
                Some(*target),
            );
            right = x - CHIP_GAP;
        }

        let mut cursor = CHIP_GAP;
        let mut place = |frame: &mut Frame, label: &str, target: Option<Target>, fg, bg| {
            let w = chip_width(label);
            if cursor + w > right {
                return;
            }
            draw_chip(
                frame,
                Rect::new(cursor, chip_y, w, CHIP_H),
                label,
                fg,
                bg,
                target,
            );
            cursor += w + CHIP_GAP;
        };

        place(frame, "+ New", Some(Target::NewNote), MANTLE, Some(GREEN));

        if let Some(note) = self
            .store
            .active_note()
            .and_then(|id| self.store.get_note(id))
        {
            let kind = note
                .body
                .get(self.caret.0)
                .map_or(LineKind::Plain, |l| l.kind.clone());
            place(
                frame,
                &format!("Color: {}", note.color_index.name()),
                Some(Target::CycleColor),
                MANTLE,
                Some(note.palette().light),
            );
            place(
                frame,
                &format!("Size: {}", note.font_size.name()),
                Some(Target::CycleFontSize),
                TEXT_COLOR,
                Some(SURFACE1),
            );
            place(
                frame,
                &format!("Line: {}", line_kind_name(&kind)),
                Some(Target::CycleLineKind),
                TEXT_COLOR,
                Some(SURFACE1),
            );
            place(
                frame,
                if note.pinned { "Unpin" } else { "Pin" },
                Some(Target::TogglePin),
                MANTLE,
                Some(YELLOW),
            );
            place(
                frame,
                "Undo",
                Some(Target::Undo),
                if note.undo_history.can_undo() {
                    TEXT_COLOR
                } else {
                    OVERLAY0
                },
                Some(SURFACE1),
            );
            place(
                frame,
                "Redo",
                Some(Target::Redo),
                if note.undo_history.can_redo() {
                    TEXT_COLOR
                } else {
                    OVERLAY0
                },
                Some(SURFACE1),
            );
            place(frame, "Delete", Some(Target::DeleteNote), MANTLE, Some(RED));
        }

        // Status last, and only if it fits: it is the least important thing on
        // the bar and the first that should give up its room.
        let status = if self.status.is_empty() {
            format!(
                "{} notes{}",
                self.store.visible_count(),
                if self.store.is_dirty() { " *" } else { "" }
            )
        } else {
            self.status.clone()
        };
        place(frame, &status, None, SUBTEXT0, None);
    }
}

fn on_off(flag: bool) -> &'static str {
    if flag { "ON" } else { "OFF" }
}

fn line_kind_name(kind: &LineKind) -> &'static str {
    match kind {
        LineKind::Plain => "Plain",
        LineKind::Bullet => "Bullet",
        LineKind::Checkbox { .. } => "Check",
    }
}

// ============================================================================
// Persistence
// ============================================================================

/// The file the notes live in, or `None` when the environment names no home —
/// an early-boot or stripped service environment, where there is no user to
/// have notes.
#[must_use]
pub fn notes_path() -> Option<PathBuf> {
    Some(settingsfile::config_dir()?.join("stickynotes.txt"))
}

/// Write the notes so that the file is never left half-written.
///
/// `fs::write` truncates before it writes, so an interrupted save destroys
/// every note rather than one; [`safeio::write_atomically`] writes beside the
/// target and renames over it, which is atomic within a directory.
fn write_store(path: &Path, store: &NoteStore) -> io::Result<()> {
    write_bytes(path, serialize_notes(store).as_bytes())
}

fn write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    safeio::write_atomically(path, bytes)
}

// ============================================================================
// Interaction
// ============================================================================

impl StickyNotesApp {
    // -- Small shared helpers ----------------------------------------------

    /// Make `id` the note the toolbar acts on, without moving the caret out of
    /// wherever it already is on that same note.
    fn select(&mut self, id: NoteId) {
        if self.store.active_note() != Some(id) {
            self.commit_focus();
            self.store.set_active(Some(id));
        }
    }

    /// Drop the caret if it is inside `id`.
    ///
    /// Archiving or deleting a note while its title is being edited would
    /// otherwise leave a `Focus::Title(id)` naming a note the store no longer
    /// hands out — every later keystroke would look up nothing and be silently
    /// swallowed, which reads exactly like a wedged keyboard.
    fn drop_focus_on(&mut self, id: NoteId) {
        if matches!(
            self.focus,
            Some(Focus::Title(f) | Focus::Body(f)) if f == id
        ) {
            self.focus = None;
        }
    }

    /// Finish a title edit, folding the whole edit into one undoable action and
    /// promoting any `#tag` words in it to real tags.
    ///
    /// Called whenever the caret leaves a title — by a click elsewhere, by
    /// Escape, by Enter, or by a save — rather than on every keystroke, so that
    /// one undo restores the name the user started from.
    fn commit_focus(&mut self) {
        let Some(Focus::Title(id)) = self.focus else {
            return;
        };
        let before = std::mem::take(&mut self.title_before);
        let Some(note) = self.store.get_note_mut(id) else {
            return;
        };
        if note.title == before {
            return;
        }
        // `#word` in a title becomes a tag and leaves the title. The store has
        // had `add_tag`/`notes_with_tag`/`all_tags` since it was written and no
        // way at all to reach them; typing is the way every other notes program
        // spells this.
        let mut kept: Vec<&str> = Vec::new();
        let mut found: Vec<String> = Vec::new();
        for word in note.title.split_whitespace() {
            match word.strip_prefix('#') {
                Some(tag) if !tag.is_empty() => found.push(tag.to_ascii_lowercase()),
                _ => kept.push(word),
            }
        }
        if !found.is_empty() {
            note.title = kept.join(" ");
        }
        note.commit_title(before);
        for tag in found {
            note.add_tag(&tag);
        }
        self.store.mark_dirty();
    }

    /// Put the caret in a body line at the column nearest `x`, in canvas
    /// coordinates.
    fn column_in_line(&self, id: NoteId, line: usize, x: f32) -> usize {
        let Some(note) = self.store.get_note(id) else {
            return 0;
        };
        let Some(row) = note.body.get(line) else {
            return 0;
        };
        column_at(
            &row.plain_text(),
            body_text_x(note, &row.kind),
            x,
            note.font_size.size(),
            FontWeightHint::Regular,
        )
    }

    /// Run `f` over the active note and report a redraw, or do nothing at all
    /// when no note is active.
    fn with_active(&mut self, f: impl FnOnce(&mut Note)) -> Action {
        let Some(id) = self.store.active_note() else {
            return Action::None;
        };
        let Some(note) = self.store.get_note_mut(id) else {
            return Action::None;
        };
        f(note);
        self.store.mark_dirty();
        Action::Redraw
    }

    /// Add a note near the middle of the canvas and start editing its title.
    ///
    /// Offset by the number of notes already there so a run of new notes
    /// cascades instead of stacking exactly on top of one another, which would
    /// look like a single note that would not respond.
    fn new_note(&mut self, canvas: Rect) -> Action {
        let step = (self.store.visible_count() % 8) as f32 * 24.0;
        let x = (canvas.w / 2.0 - 110.0 + step).max(8.0);
        let y = (canvas.h / 3.0 + step).max(8.0);
        let id = self.store.create_note(x, y);
        self.store.set_active(Some(id));
        self.store.bring_to_front(id);
        self.clamp_note(id, canvas);
        self.focus = Some(Focus::Title(id));
        self.title_before = self
            .store
            .get_note(id)
            .map_or_else(String::new, |n| n.title.clone());
        self.caret = (0, self.title_before.len());
        self.status.clear();
        Action::Redraw
    }

    fn undo_active(&mut self) -> Action {
        self.commit_focus();
        let Some(id) = self.store.active_note() else {
            return Action::None;
        };
        let undone = self.store.get_note_mut(id).is_some_and(Note::undo);
        if undone {
            self.clamp_caret();
            self.store.mark_dirty();
            Action::Redraw
        } else {
            Action::None
        }
    }

    fn redo_active(&mut self) -> Action {
        let Some(id) = self.store.active_note() else {
            return Action::None;
        };
        let redone = self.store.get_note_mut(id).is_some_and(Note::redo);
        if redone {
            self.clamp_caret();
            self.store.mark_dirty();
            Action::Redraw
        } else {
            Action::None
        }
    }

    /// Pull the caret back inside the text it points into.
    ///
    /// An undo can shorten the line the caret is sitting past the end of; the
    /// renderer would then measure a prefix that no longer exists.
    fn clamp_caret(&mut self) {
        let Some(focus) = self.focus else { return };
        let (line, col) = self.caret;
        let bound = match focus {
            Focus::Search => self.store.search_query().len(),
            Focus::Title(id) => self.store.get_note(id).map_or(0, |n| n.title.len()),
            Focus::Body(id) => match self.store.get_note(id) {
                Some(note) => {
                    let line = line.min(note.body.len().saturating_sub(1));
                    self.caret.0 = line;
                    note.body.get(line).map_or(0, RichLine::char_count)
                }
                None => 0,
            },
        };
        self.caret.1 = col.min(bound);
    }

    // -- Mouse -------------------------------------------------------------

    /// Route a mouse press at window coordinates `(x, y)`.
    pub fn handle_click(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        size: (f32, f32),
    ) -> Action {
        let size = Self::clamped(size);
        let Some(target) = self.frame(size.0, size.1).hit_test(x, y) else {
            return Action::None;
        };
        self.activate(target, x, y, button, size)
    }

    /// Act on a control the renderer named, having already decided which one
    /// the press landed on.
    pub fn activate(
        &mut self,
        target: Target,
        x: f32,
        y: f32,
        button: MouseButton,
        size: (f32, f32),
    ) -> Action {
        let size = Self::clamped(size);
        let canvas = self.canvas_rect(size.0, size.1);
        // Note geometry is canvas-relative; see `canvas_rect`.
        let (cx, cy) = (x - canvas.x, y - canvas.y);
        match target {
            Target::Desktop => {
                self.commit_focus();
                self.focus = None;
                self.store.set_active(None);
                Action::Redraw
            }
            Target::NoteTitle(id) => {
                self.select(id);
                self.store.bring_to_front(id);
                if button == MouseButton::Left {
                    self.store.begin_move(id, cx, cy);
                }
                Action::Redraw
            }
            Target::NoteArchive(id) => {
                self.commit_focus();
                self.drop_focus_on(id);
                if !self.store.archive_note(id) {
                    return Action::None;
                }
                if self.store.active_note() == Some(id) {
                    self.store.set_active(None);
                }
                self.status = String::from("Archived — reopen it from Archived");
                Action::Redraw
            }
            Target::NoteLine(id, line) => {
                self.select(id);
                self.store.bring_to_front(id);
                self.focus = Some(Focus::Body(id));
                self.caret = (line, self.column_in_line(id, line, cx));
                Action::Redraw
            }
            Target::NoteCheck(id, line) => {
                self.select(id);
                if self
                    .store
                    .get_note_mut(id)
                    .is_some_and(|note| note.toggle_checkbox(line))
                {
                    self.store.mark_dirty();
                    return Action::Redraw;
                }
                Action::None
            }
            Target::NoteGrip(id) => {
                self.select(id);
                self.store.bring_to_front(id);
                if button == MouseButton::Left {
                    self.store.begin_resize(id, cx, cy);
                }
                Action::Redraw
            }
            Target::SearchBox => {
                self.commit_focus();
                self.focus = Some(Focus::Search);
                self.caret = (0, self.store.search_query().len());
                Action::Redraw
            }
            Target::TagChip(index) => {
                let tags = self.store.all_tags();
                let Some(tag) = tags.get(index) else {
                    return Action::None;
                };
                // A second click on the chip already filtering clears it —
                // otherwise the only way back to the full list is to notice
                // that the search box now holds a word nobody typed.
                let filter = format!("{TAG_PREFIX}{tag}");
                if self.store.search_query() == filter {
                    self.store.set_search("");
                } else {
                    self.store.set_search(&filter);
                }
                self.sidebar_scroll = 0.0;
                Action::Redraw
            }
            Target::SidebarItem(id) => {
                self.commit_focus();
                if self.archive_view {
                    if !self.store.unarchive_note(id) {
                        return Action::None;
                    }
                    self.archive_view = false;
                    self.status = String::from("Restored");
                }
                self.store.set_active(Some(id));
                self.store.bring_to_front(id);
                self.clamp_note(id, canvas);
                self.focus = None;
                self.sidebar_scroll_to(id, size.1);
                Action::Redraw
            }
            Target::NewNote => {
                self.commit_focus();
                self.new_note(canvas)
            }
            Target::ToggleGrid => {
                let on = self.store.snap_to_grid_enabled();
                self.store.set_snap_to_grid(!on);
                Action::Redraw
            }
            Target::ToggleSidebar => {
                self.store.toggle_sidebar();
                if !self.store.sidebar_visible() && self.focus == Some(Focus::Search) {
                    self.focus = None;
                }
                // The canvas just changed width; a note that was beside the
                // sidebar can now be beyond the right edge.
                let canvas = self.canvas_rect(size.0, size.1);
                for id in self
                    .store
                    .visible_notes()
                    .iter()
                    .map(|n| n.id)
                    .collect::<Vec<_>>()
                {
                    self.clamp_note(id, canvas);
                }
                Action::Redraw
            }
            Target::ToggleArchiveView => {
                self.archive_view = !self.archive_view;
                self.sidebar_scroll = 0.0;
                if self.archive_view {
                    self.store.set_sidebar_visible(true);
                }
                Action::Redraw
            }
            Target::CycleColor => self.with_active(|note| {
                note.color_index = note.color_index.next();
            }),
            Target::CycleFontSize => self.with_active(|note| {
                note.font_size = note.font_size.next();
            }),
            Target::CycleLineKind => self.cycle_line_kind(),
            Target::TogglePin => {
                let Some(id) = self.store.active_note() else {
                    return Action::None;
                };
                self.store.toggle_pin(id);
                Action::Redraw
            }
            Target::DeleteNote => {
                let Some(id) = self.store.active_note() else {
                    return Action::None;
                };
                self.drop_focus_on(id);
                self.title_before.clear();
                if !self.store.delete_note(id) {
                    return Action::None;
                }
                self.status = String::from("Deleted");
                Action::Redraw
            }
            Target::Undo => self.undo_active(),
            Target::Redo => self.redo_active(),
        }
    }

    /// Turn the caret's line into the next kind — plain, bullet, checkbox.
    fn cycle_line_kind(&mut self) -> Action {
        let line = self.caret.0;
        self.with_active(|note| {
            if let Some(row) = note.body.get_mut(line) {
                row.kind = next_line_kind(&row.kind);
            }
        })
    }

    /// Scroll the sidebar so the row for `id` is inside the pane.
    ///
    /// Selecting a note from the canvas highlights its sidebar row, and a
    /// highlight on a row nobody can see is not a highlight.
    fn sidebar_scroll_to(&mut self, id: NoteId, height: f32) {
        let Some(index) = self.sidebar_rows().iter().position(|item| item.id == id) else {
            return;
        };
        let top = index as f32 * SIDEBAR_ITEM_H;
        let viewport = self
            .sidebar_list_rect(Self::clamped((MIN_WIDTH, height)).1)
            .h;
        if top < self.sidebar_scroll {
            self.sidebar_scroll = top;
        } else if top + SIDEBAR_ITEM_H > self.sidebar_scroll + viewport {
            self.sidebar_scroll = top + SIDEBAR_ITEM_H - viewport;
        }
        self.clamp_scroll(height);
    }

    /// End whatever the mouse was dragging.
    fn release(&mut self, size: (f32, f32)) -> Action {
        if matches!(self.store.drag_state(), DragState::None) {
            return Action::None;
        }
        let dragged = match self.store.drag_state() {
            DragState::Moving { note_id, .. } | DragState::Resizing { note_id, .. } => {
                Some(*note_id)
            }
            DragState::None => None,
        };
        self.store.end_drag();
        if let Some(id) = dragged {
            let canvas = self.canvas_rect(size.0, size.1);
            self.clamp_note(id, canvas);
        }
        Action::Redraw
    }

    /// Scroll the sidebar, if the pointer is over it.
    fn scroll(&mut self, x: f32, dy: f32, size: (f32, f32)) -> Action {
        if !self.store.sidebar_visible() || x > SIDEBAR_WIDTH {
            return Action::None;
        }
        // `wheel::pixels` already negates for the vertical axis, so this adds.
        self.sidebar_scroll += wheel::pixels(dy, SIDEBAR_ITEM_H);
        self.clamp_scroll(size.1);
        Action::Redraw
    }

    // -- Keyboard ----------------------------------------------------------

    /// Route a keystroke.
    pub fn handle_key(&mut self, event: &KeyEvent, size: (f32, f32)) -> Action {
        if !event.pressed {
            return Action::None;
        }
        let size = Self::clamped(size);
        let m = event.modifiers;

        if m.ctrl && !m.alt {
            let canvas = self.canvas_rect(size.0, size.1);
            return match event.key {
                Key::Q => Action::Quit,
                Key::N => {
                    self.commit_focus();
                    self.new_note(canvas)
                }
                Key::S => {
                    self.commit_focus();
                    self.persist()
                }
                Key::E => {
                    self.commit_focus();
                    self.export()
                }
                Key::Z => self.undo_active(),
                Key::Y => self.redo_active(),
                Key::F => {
                    self.commit_focus();
                    self.store.set_sidebar_visible(true);
                    self.focus = Some(Focus::Search);
                    self.caret = (0, self.store.search_query().len());
                    Action::Redraw
                }
                Key::B => self.activate(Target::ToggleSidebar, 0.0, 0.0, MouseButton::Left, size),
                Key::G => {
                    let on = self.store.snap_to_grid_enabled();
                    self.store.set_snap_to_grid(!on);
                    Action::Redraw
                }
                Key::L => self.cycle_line_kind(),
                _ => Action::None,
            };
        }

        // A bare key must stay bare: Alt-Tab and the Super menu belong to the
        // window manager, and a shortcut that fired under them would fire
        // while the window was being switched away from.
        if m.ctrl || m.alt || m.super_key {
            return Action::None;
        }

        if event.key == Key::Escape {
            self.commit_focus();
            if self.focus.is_some() {
                self.focus = None;
            } else if self.archive_view {
                self.archive_view = false;
            } else if !self.store.search_query().is_empty() {
                self.store.set_search("");
            } else if self.store.active_note().is_some() {
                self.store.set_active(None);
            } else {
                return Action::None;
            }
            return Action::Redraw;
        }

        match self.focus {
            Some(focus) => self.type_into(focus, event, size),
            // Nothing has the caret: Delete archives the selected note, which
            // is reversible, rather than deleting it, which is not.
            None => match event.key {
                Key::Delete | Key::Backspace => {
                    let Some(id) = self.store.active_note() else {
                        return Action::None;
                    };
                    self.activate(Target::NoteArchive(id), 0.0, 0.0, MouseButton::Left, size)
                }
                _ => Action::None,
            },
        }
    }

    /// Deliver a keystroke to whichever text box has the caret.
    fn type_into(&mut self, focus: Focus, event: &KeyEvent, size: (f32, f32)) -> Action {
        match focus {
            Focus::Search => self.type_into_search(event),
            Focus::Title(id) => self.type_into_title(id, event, size),
            Focus::Body(id) => self.type_into_body(id, event),
        }
    }

    fn type_into_search(&mut self, event: &KeyEvent) -> Action {
        let mut query = self.store.search_query().to_string();
        let mut col = Note::snap_col(&query, self.caret.1);
        match event.key {
            Key::Enter | Key::Tab => {
                self.focus = None;
                return Action::Redraw;
            }
            Key::Backspace if col > 0 => {
                let prev = query
                    .get(..col)
                    .and_then(|s| s.chars().next_back())
                    .map_or(0, char::len_utf8);
                col = col.saturating_sub(prev);
                query.replace_range(col..col.saturating_add(prev), "");
            }
            Key::Delete => {
                if let Some(ch) = query.get(col..).and_then(|s| s.chars().next()) {
                    query.replace_range(col..col.saturating_add(ch.len_utf8()), "");
                }
            }
            Key::Left => {
                col = col.saturating_sub(
                    query
                        .get(..col)
                        .and_then(|s| s.chars().next_back())
                        .map_or(0, char::len_utf8),
                );
            }
            Key::Right => {
                col = col.saturating_add(
                    query
                        .get(col..)
                        .and_then(|s| s.chars().next())
                        .map_or(0, char::len_utf8),
                );
            }
            Key::Home => col = 0,
            Key::End => col = query.len(),
            _ => {
                let before = query.len();
                for ch in event.typed() {
                    query.insert(col, ch);
                    col = col.saturating_add(ch.len_utf8());
                }
                if query.len() == before {
                    return Action::None;
                }
            }
        }
        self.store.set_search(&query);
        self.caret = (0, col);
        self.sidebar_scroll = 0.0;
        Action::Redraw
    }

    fn type_into_title(&mut self, id: NoteId, event: &KeyEvent, size: (f32, f32)) -> Action {
        // Editing the title in place and folding the whole edit into one undo
        // when the caret leaves; see `commit_focus`.
        let Some(note) = self.store.get_note_mut(id) else {
            self.focus = None;
            return Action::Redraw;
        };
        let mut col = Note::snap_col(&note.title, self.caret.1);
        match event.key {
            Key::Enter | Key::Tab => {
                // Enter moves on to the body, which is what a note is for.
                self.commit_focus();
                self.focus = Some(Focus::Body(id));
                self.caret = (0, 0);
                return Action::Redraw;
            }
            Key::Down => {
                self.commit_focus();
                self.focus = Some(Focus::Body(id));
                self.caret = (0, 0);
                return Action::Redraw;
            }
            Key::Backspace if col > 0 => {
                let prev = note
                    .title
                    .get(..col)
                    .and_then(|s| s.chars().next_back())
                    .map_or(0, char::len_utf8);
                col = col.saturating_sub(prev);
                note.title.replace_range(col..col.saturating_add(prev), "");
            }
            Key::Delete => {
                if let Some(ch) = note.title.get(col..).and_then(|s| s.chars().next()) {
                    note.title
                        .replace_range(col..col.saturating_add(ch.len_utf8()), "");
                }
            }
            Key::Left => {
                col = col.saturating_sub(
                    note.title
                        .get(..col)
                        .and_then(|s| s.chars().next_back())
                        .map_or(0, char::len_utf8),
                );
            }
            Key::Right => {
                col = col.saturating_add(
                    note.title
                        .get(col..)
                        .and_then(|s| s.chars().next())
                        .map_or(0, char::len_utf8),
                );
            }
            Key::Home => col = 0,
            Key::End => col = note.title.len(),
            _ => {
                let before = note.title.len();
                for ch in event.typed() {
                    if note.title.chars().count() >= MAX_TITLE_LEN {
                        break;
                    }
                    note.title.insert(col, ch);
                    col = col.saturating_add(ch.len_utf8());
                }
                if note.title.len() == before {
                    return Action::None;
                }
            }
        }
        self.caret = (0, col);
        self.store.mark_dirty();
        let canvas = self.canvas_rect(size.0, size.1);
        self.clamp_note(id, canvas);
        Action::Redraw
    }

    fn type_into_body(&mut self, id: NoteId, event: &KeyEvent) -> Action {
        let Some(note) = self.store.get_note_mut(id) else {
            self.focus = None;
            return Action::Redraw;
        };
        let mut line = self.caret.0.min(note.body.len().saturating_sub(1));
        let current = note
            .body
            .get(line)
            .map_or_else(String::new, RichLine::plain_text);
        let mut col = Note::snap_col(&current, self.caret.1);

        match event.key {
            Key::Enter => {
                // The tail of the line moves down with the caret, and the new
                // line inherits the kind: a list of checkboxes stays a list.
                let kind = note
                    .body
                    .get(line)
                    .map_or(LineKind::Plain, |l| match &l.kind {
                        LineKind::Checkbox { .. } => LineKind::Checkbox { checked: false },
                        other => other.clone(),
                    });
                let tail = current.get(col..).unwrap_or("").to_string();
                for _ in 0..tail.chars().count() {
                    note.delete_char(line, col);
                }
                let mut row = RichLine::plain(&tail);
                row.kind = kind;
                line = line.saturating_add(1);
                note.insert_line(line, row);
                col = 0;
            }
            Key::Backspace => {
                if col > 0 {
                    let prev = current
                        .get(..col)
                        .and_then(|s| s.chars().next_back())
                        .map_or(0, char::len_utf8);
                    col = col.saturating_sub(prev);
                    note.delete_char(line, col);
                } else if line > 0 {
                    // Joining onto the line above, one character at a time so
                    // that each step is an action the undo stack can reverse.
                    let above = line.saturating_sub(1);
                    let mut at = note.body.get(above).map_or(0, |l| l.plain_text().len());
                    col = at;
                    for ch in current.chars() {
                        note.insert_char(above, at, ch);
                        at = at.saturating_add(ch.len_utf8());
                    }
                    if note.delete_line(line).is_none() {
                        return Action::None;
                    }
                    line = above;
                } else {
                    return Action::None;
                }
            }
            Key::Delete => {
                if col < current.len() {
                    note.delete_char(line, col);
                } else {
                    return Action::None;
                }
            }
            Key::Left => {
                if col > 0 {
                    col = col.saturating_sub(
                        current
                            .get(..col)
                            .and_then(|s| s.chars().next_back())
                            .map_or(0, char::len_utf8),
                    );
                } else if line > 0 {
                    line = line.saturating_sub(1);
                    col = note.body.get(line).map_or(0, |l| l.plain_text().len());
                } else {
                    return Action::None;
                }
            }
            Key::Right => {
                if col < current.len() {
                    col = col.saturating_add(
                        current
                            .get(col..)
                            .and_then(|s| s.chars().next())
                            .map_or(0, char::len_utf8),
                    );
                } else if line.saturating_add(1) < note.body.len() {
                    line = line.saturating_add(1);
                    col = 0;
                } else {
                    return Action::None;
                }
            }
            Key::Up => {
                if line == 0 {
                    self.focus = Some(Focus::Title(id));
                    self.title_before = note.title.clone();
                    self.caret = (0, note.title.len());
                    return Action::Redraw;
                }
                line = line.saturating_sub(1);
                col = col.min(note.body.get(line).map_or(0, |l| l.plain_text().len()));
            }
            Key::Down => {
                if line.saturating_add(1) >= note.body.len() {
                    return Action::None;
                }
                line = line.saturating_add(1);
                col = col.min(note.body.get(line).map_or(0, |l| l.plain_text().len()));
            }
            Key::Home => col = 0,
            Key::End => col = current.len(),
            Key::Tab => {
                self.focus = None;
                return Action::Redraw;
            }
            _ => {
                let mut typed = false;
                for ch in event.typed() {
                    if current.len() >= MAX_LINE_LEN {
                        break;
                    }
                    note.insert_char(line, col, ch);
                    col = col.saturating_add(ch.len_utf8());
                    typed = true;
                }
                if !typed {
                    return Action::None;
                }
            }
        }

        self.caret = (line, col);
        self.store.mark_dirty();
        Action::Redraw
    }

    // -- Events ------------------------------------------------------------

    /// Route one window event.
    pub fn handle_event(&mut self, event: &Event, size: (f32, f32)) -> Action {
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Press(button) => self.handle_click(mouse.x, mouse.y, button, size),
                MouseEventKind::Move => {
                    if matches!(self.store.drag_state(), DragState::None) {
                        return Action::None;
                    }
                    let canvas = self.canvas_rect(size.0, size.1);
                    self.store
                        .update_drag(mouse.x - canvas.x, mouse.y - canvas.y);
                    Action::Redraw
                }
                MouseEventKind::Release(_) | MouseEventKind::Leave => self.release(size),
                MouseEventKind::Scroll { dy, .. } => self.scroll(mouse.x, dy, size),
                MouseEventKind::DoubleClick(_) => {
                    // The title bar's first job is dragging, so the caret goes
                    // into a title on the second click rather than the first.
                    let clamped = Self::clamped(size);
                    let hit = self.frame(clamped.0, clamped.1).hit_test(mouse.x, mouse.y);
                    match hit {
                        Some(Target::NoteTitle(id)) => {
                            self.store.end_drag();
                            self.select(id);
                            self.focus = Some(Focus::Title(id));
                            self.title_before = self
                                .store
                                .get_note(id)
                                .map_or_else(String::new, |n| n.title.clone());
                            let title_x = self.canvas_rect(clamped.0, clamped.1).x;
                            self.caret = (0, self.column_in_title(id, mouse.x - title_x));
                            Action::Redraw
                        }
                        _ => Action::None,
                    }
                }
                MouseEventKind::Enter => Action::None,
            },
            Event::Key(key) => self.handle_key(key, size),
            Event::Resize { width, height } => {
                self.window_size = (*width as f32, *height as f32);
                let canvas = self.canvas_rect(self.window_size.0, self.window_size.1);
                for id in self
                    .store
                    .visible_notes()
                    .iter()
                    .map(|n| n.id)
                    .collect::<Vec<_>>()
                {
                    self.clamp_note(id, canvas);
                }
                self.clamp_scroll(self.window_size.1);
                Action::Redraw
            }
            Event::Tick { elapsed_ms } => self.tick(*elapsed_ms),
            Event::FocusOut => {
                // Leaving the window is the same promise as closing it: what
                // was typed is on disk before anything else can happen to it.
                self.commit_focus();
                self.store.end_drag();
                if self.store.is_dirty() {
                    self.persist()
                } else {
                    Action::None
                }
            }
            Event::CloseRequested => {
                self.commit_focus();
                self.persist();
                Action::Quit
            }
            Event::FocusIn | Event::Moved { .. } | Event::ScaleChanged { .. } => Action::None,
        }
    }

    /// The byte offset in a note's title nearest `x`, in canvas coordinates.
    fn column_in_title(&self, id: NoteId, x: f32) -> usize {
        let Some(note) = self.store.get_note(id) else {
            return 0;
        };
        let weight = FontWeightHint::Bold;
        let size = note.font_size.title_size();
        let lead = if note.pinned {
            text::measure("[P] ", size, weight)
        } else {
            0.0
        };
        column_at(&note.title, note.x + NOTE_PAD + lead, x, size, weight)
    }

    /// Save, if enough time has passed since the last edit.
    pub fn tick(&mut self, elapsed_ms: u64) -> Action {
        if self.autosave.tick(elapsed_ms, self.store.is_dirty()) {
            self.commit_focus();
            return self.persist();
        }
        Action::None
    }
}

// ============================================================================
// Window plumbing
// ============================================================================

impl App for StickyNotesApp {
    fn title(&self) -> String {
        let dirty = if self.store.is_dirty() { " *" } else { "" };
        format!("Sticky Notes{dirty}")
    }

    fn app_id(&self) -> String {
        String::from("stickynotes")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Tick only while something is waiting to be written.
    ///
    /// A notes window with nothing unsaved has no reason to wake the
    /// compositor twice a second; consulted after every event, so the first
    /// keystroke starts the clock and the save that follows stops it.
    fn tick_interval(&self) -> Option<Duration> {
        (self.storage.is_some() && self.store.is_dirty()).then_some(TICK)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        let size = self.window_size;
        match self.handle_event(event, size) {
            Action::None => Response::Idle,
            Action::Redraw => Response::Redraw,
            Action::Quit => Response::Exit,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The remembered size is only ever a starting guess; this is the real
        // one, and the click router reads it back through `on_event`.
        self.window_size = (width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for StickyNotesApp {
    type Target = Target;
    type Outcome = Action;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    /// A click is a press *and* a release, so a click on a title bar does not
    /// leave a move running that the next mouse motion would act on.
    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Action {
        let pressed = self.handle_click(x, y, button, size);
        let released = self.release(size);
        if pressed == Action::None {
            released
        } else {
            pressed
        }
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Action {
        self.handle_key(key, size)
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() -> ExitCode {
    let mut state = match notes_path() {
        Some(path) => StickyNotesApp::with_storage(path),
        // No configuration directory means no user to have notes; the window
        // still opens, it just has nowhere to write them back to.
        None => StickyNotesApp::new(),
    };
    app::launch("stickynotes", &mut state)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
// A test that panics on bad data is a test reporting a fault; the production
// lints that forbid it are about inputs an attacker can shape, which a fixture
// is not.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    // Every float here is a layout constant this file itself produced — 40.0
    // from a 20-pixel grid, 50.0 from `set_position(50.0, …)`. Comparing those
    // exactly is the assertion; an epsilon would only hide an arithmetic slip.
    clippy::float_cmp
)]
mod tests {
    use guitk::event::Modifiers;
    use guitk::probe;
    use scratchdir::ScratchDir;

    use super::*;

    // -- Note construction & basic properties --------------------------------

    #[test]
    fn test_create_note_defaults() {
        let note = Note::new(1, 50.0, 100.0);
        assert_eq!(note.id, 1);
        assert_eq!(note.title, "New Note");
        assert_eq!(note.x, 50.0);
        assert_eq!(note.y, 100.0);
        assert_eq!(note.width, 220.0);
        assert_eq!(note.height, 200.0);
        assert!(!note.pinned);
        assert!(!note.archived);
        assert_eq!(note.color_index, NoteColorIndex::Yellow);
        assert_eq!(note.font_size, FontSizePreset::Medium);
        assert!(note.tags.is_empty());
    }

    #[test]
    fn test_note_palette_lookup() {
        let note = Note::new(1, 0.0, 0.0);
        let palette = note.palette();
        assert_eq!(palette.light, NOTE_COLORS[0].light);
        assert_eq!(palette.dark, NOTE_COLORS[0].dark);
    }

    /// A note's geometry now has exactly one description — the frame the
    /// renderer records as it paints — so these ask the renderer rather than a
    /// second copy of the arithmetic that could drift away from it.
    #[test]
    fn test_note_title_bar_and_grip_are_separate_targets() {
        let mut app = StickyNotesApp::new();
        let id = app.store.create_note(40.0, 40.0);
        let title = probe::rect_of(&app, Target::NoteTitle(id)).expect("a title bar");
        let grip = probe::rect_of(&app, Target::NoteGrip(id)).expect("a resize grip");
        assert!(
            title.intersect(grip).is_none(),
            "the drag handle and the resize handle must not overlap: {title:?} vs {grip:?}"
        );

        let frame = app.draw(StickyNotesApp::SIZE);
        let (tx, ty) = title.centre();
        assert_eq!(frame.hit_test(tx, ty), Some(Target::NoteTitle(id)));
        let (gx, gy) = grip.centre();
        assert_eq!(frame.hit_test(gx, gy), Some(Target::NoteGrip(id)));
    }

    #[test]
    fn test_click_outside_every_note_is_the_desktop() {
        let mut app = StickyNotesApp::new();
        let id = app.store.create_note(40.0, 40.0);
        let note = probe::rect_of(&app, Target::NoteTitle(id)).expect("a title bar");
        let frame = app.draw(StickyNotesApp::SIZE);
        assert_eq!(
            frame.hit_test(note.x - 12.0, note.y + 4.0),
            Some(Target::Desktop),
            "a point left of the note is the canvas, not the note"
        );
    }

    // -- Note positioning & sizing -------------------------------------------

    #[test]
    fn test_set_position_normal() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.set_position(100.0, 200.0);
        assert_eq!(note.x, 100.0);
        assert_eq!(note.y, 200.0);
    }

    #[test]
    fn test_set_position_pinned_does_not_move() {
        let mut note = Note::new(1, 50.0, 60.0);
        note.pinned = true;
        note.set_position(200.0, 300.0);
        assert_eq!(note.x, 50.0);
        assert_eq!(note.y, 60.0);
    }

    #[test]
    fn test_set_size_enforces_minimum() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.set_size(50.0, 30.0);
        assert_eq!(note.width, 120.0);
        assert_eq!(note.height, 80.0);
    }

    #[test]
    fn test_set_size_large() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.set_size(500.0, 400.0);
        assert_eq!(note.width, 500.0);
        assert_eq!(note.height, 400.0);
    }

    // -- Tags ----------------------------------------------------------------

    #[test]
    fn test_add_tag() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.add_tag("work");
        assert_eq!(note.tags, vec!["work"]);
    }

    #[test]
    fn test_add_duplicate_tag_ignored() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.add_tag("work");
        note.add_tag("work");
        assert_eq!(note.tags.len(), 1);
    }

    #[test]
    fn test_remove_tag() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.add_tag("work");
        note.add_tag("home");
        assert!(note.remove_tag("work"));
        assert_eq!(note.tags, vec!["home"]);
        assert!(!note.remove_tag("nonexistent"));
    }

    // -- Checkbox toggle -----------------------------------------------------

    #[test]
    fn test_toggle_checkbox() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![
            RichLine::checkbox("Buy milk", false),
            RichLine::plain("random"),
        ];
        assert!(note.toggle_checkbox(0));
        assert_eq!(note.body[0].kind, LineKind::Checkbox { checked: true });
        assert!(note.toggle_checkbox(0));
        assert_eq!(note.body[0].kind, LineKind::Checkbox { checked: false });
    }

    #[test]
    fn test_toggle_checkbox_on_plain_line_returns_false() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![RichLine::plain("hello")];
        assert!(!note.toggle_checkbox(0));
    }

    #[test]
    fn test_toggle_checkbox_out_of_bounds() {
        let mut note = Note::new(1, 0.0, 0.0);
        assert!(!note.toggle_checkbox(99));
    }

    // -- Search --------------------------------------------------------------

    #[test]
    fn test_matches_search_title() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.title = "Shopping List".to_string();
        assert!(note.matches_search("shop"));
        assert!(note.matches_search("SHOP"));
        assert!(!note.matches_search("work"));
    }

    #[test]
    fn test_matches_search_body() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![RichLine::plain("Buy groceries")];
        assert!(note.matches_search("grocer"));
    }

    #[test]
    fn test_matches_search_tag() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.tags = vec!["urgent".to_string()];
        assert!(note.matches_search("urgent"));
        assert!(note.matches_search("URG"));
    }

    #[test]
    fn test_matches_search_empty_matches_all() {
        let note = Note::new(1, 0.0, 0.0);
        assert!(note.matches_search(""));
    }

    // -- Body text -----------------------------------------------------------

    #[test]
    fn test_body_text_plain() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![RichLine::plain("line 1"), RichLine::plain("line 2")];
        assert_eq!(note.body_text(), "line 1\nline 2");
    }

    #[test]
    fn test_body_text_with_bullet_and_checkbox() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![
            RichLine::bullet("item"),
            RichLine::checkbox("task", true),
            RichLine::checkbox("task2", false),
        ];
        assert_eq!(note.body_text(), "* item\n[x] task\n[ ] task2");
    }

    #[test]
    fn test_set_body_from_text() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.set_body_from_text("hello\n* bullet\n[x] done\n[ ] todo");
        assert_eq!(note.body.len(), 4);
        assert_eq!(note.body[0].kind, LineKind::Plain);
        assert_eq!(note.body[1].kind, LineKind::Bullet);
        assert_eq!(note.body[2].kind, LineKind::Checkbox { checked: true });
        assert_eq!(note.body[3].kind, LineKind::Checkbox { checked: false });
    }

    // -- Char insert/delete/undo/redo ----------------------------------------

    #[test]
    fn test_insert_char() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![RichLine::plain("hllo")];
        note.insert_char(0, 1, 'e');
        assert_eq!(note.body[0].plain_text(), "hello");
    }

    #[test]
    fn test_delete_char() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![RichLine::plain("hello")];
        let ch = note.delete_char(0, 1);
        assert_eq!(ch, Some('e'));
        assert_eq!(note.body[0].plain_text(), "hllo");
    }

    /// `col` is a byte offset, so a note with an accent in it puts offsets
    /// inside characters within easy reach of any caller. Both methods used to
    /// clamp to `len()` and no further, and `String::insert`/`remove` panic
    /// rather than rounding on such an offset.
    #[test]
    fn a_column_inside_a_multibyte_character_edits_that_character() {
        // "café" — the é starts at byte 3 and is two bytes wide, so 4 is inside
        // it and 5 is the end of the string.
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![RichLine::plain("café")];
        assert_eq!(note.delete_char(0, 4), Some('é'));
        assert_eq!(note.body[0].plain_text(), "caf");

        // Inserting at an interior offset lands before the character, not
        // inside it, so the text stays valid UTF-8 and reads sensibly.
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![RichLine::plain("café")];
        note.insert_char(0, 4, 'x');
        assert_eq!(note.body[0].plain_text(), "cafxé");

        // Wider characters, and a column past the end of the line.
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![RichLine::plain("日本")];
        assert_eq!(note.delete_char(0, 5), Some('本'));
        note.insert_char(0, 99, '!');
        assert_eq!(note.body[0].plain_text(), "日!");
    }

    #[test]
    fn test_delete_char_out_of_range() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![RichLine::plain("hi")];
        assert_eq!(note.delete_char(0, 10), None);
    }

    #[test]
    fn test_insert_line() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![RichLine::plain("first"), RichLine::plain("third")];
        note.insert_line(1, RichLine::plain("second"));
        assert_eq!(note.body.len(), 3);
        assert_eq!(note.body[1].plain_text(), "second");
    }

    #[test]
    fn test_delete_line() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![RichLine::plain("a"), RichLine::plain("b")];
        let removed = note.delete_line(0);
        assert!(removed.is_some());
        assert_eq!(note.body.len(), 1);
        assert_eq!(note.body[0].plain_text(), "b");
    }

    #[test]
    fn test_delete_last_line_prevented() {
        let mut note = Note::new(1, 0.0, 0.0);
        note.body = vec![RichLine::plain("only")];
        assert!(note.delete_line(0).is_none());
        assert_eq!(note.body.len(), 1);
    }

    // -- Undo/redo system ----------------------------------------------------

    #[test]
    fn test_undo_history_push_and_pop() {
        let mut h = UndoHistory::new(10);
        h.push(EditAction::InsertChar {
            line: 0,
            col: 0,
            ch: 'a',
        });
        assert!(h.can_undo());
        assert!(!h.can_redo());
        let action = h.pop_undo();
        assert!(action.is_some());
        assert!(h.can_redo());
        assert!(!h.can_undo());
    }

    #[test]
    fn test_undo_history_redo() {
        let mut h = UndoHistory::new(10);
        h.push(EditAction::InsertChar {
            line: 0,
            col: 0,
            ch: 'a',
        });
        h.pop_undo();
        let action = h.pop_redo();
        assert!(action.is_some());
        assert!(h.can_undo());
    }

    #[test]
    fn test_undo_history_push_clears_redo() {
        let mut h = UndoHistory::new(10);
        h.push(EditAction::InsertChar {
            line: 0,
            col: 0,
            ch: 'a',
        });
        h.pop_undo();
        assert!(h.can_redo());
        h.push(EditAction::InsertChar {
            line: 0,
            col: 0,
            ch: 'b',
        });
        assert!(!h.can_redo());
    }

    #[test]
    fn test_undo_history_max_depth() {
        let mut h = UndoHistory::new(3);
        h.push(EditAction::InsertChar {
            line: 0,
            col: 0,
            ch: 'a',
        });
        h.push(EditAction::InsertChar {
            line: 0,
            col: 1,
            ch: 'b',
        });
        h.push(EditAction::InsertChar {
            line: 0,
            col: 2,
            ch: 'c',
        });
        h.push(EditAction::InsertChar {
            line: 0,
            col: 3,
            ch: 'd',
        });
        assert_eq!(h.undo_count(), 3);
    }

    #[test]
    fn test_undo_history_clear() {
        let mut h = UndoHistory::new(10);
        h.push(EditAction::InsertChar {
            line: 0,
            col: 0,
            ch: 'x',
        });
        h.clear();
        assert!(!h.can_undo());
        assert!(!h.can_redo());
    }

    // -- NoteStore CRUD ------------------------------------------------------

    #[test]
    fn test_store_create_note() {
        let mut store = NoteStore::new();
        let id = store.create_note(10.0, 20.0);
        assert_eq!(store.total_count(), 1);
        assert_eq!(store.get_note(id).map(|n| n.x), Some(10.0));
    }

    #[test]
    fn test_store_create_colored_note() {
        let mut store = NoteStore::new();
        let id = store.create_colored_note(0.0, 0.0, NoteColorIndex::Blue);
        assert_eq!(
            store.get_note(id).map(|n| n.color_index),
            Some(NoteColorIndex::Blue)
        );
    }

    #[test]
    fn test_store_delete_note() {
        let mut store = NoteStore::new();
        let id = store.create_note(0.0, 0.0);
        assert!(store.delete_note(id));
        assert_eq!(store.total_count(), 0);
        assert!(!store.delete_note(id)); // Already deleted.
    }

    #[test]
    fn test_store_delete_active_note_clears_active() {
        let mut store = NoteStore::new();
        let id = store.create_note(0.0, 0.0);
        store.set_active(Some(id));
        store.delete_note(id);
        assert_eq!(store.active_note(), None);
    }

    #[test]
    fn test_store_visible_count() {
        let mut store = NoteStore::new();
        let id1 = store.create_note(0.0, 0.0);
        let _id2 = store.create_note(100.0, 0.0);
        store.archive_note(id1);
        assert_eq!(store.visible_count(), 1);
        assert_eq!(store.archived_count(), 1);
    }

    // -- Archive/unarchive ---------------------------------------------------

    #[test]
    fn test_archive_and_unarchive() {
        let mut store = NoteStore::new();
        let id = store.create_note(0.0, 0.0);
        assert!(store.archive_note(id));
        assert_eq!(store.archived_count(), 1);
        assert!(store.unarchive_note(id));
        assert_eq!(store.archived_count(), 0);
    }

    #[test]
    fn test_archive_nonexistent_returns_false() {
        let mut store = NoteStore::new();
        assert!(!store.archive_note(999));
    }

    // -- Pin/unpin -----------------------------------------------------------

    #[test]
    fn test_pin_note() {
        let mut store = NoteStore::new();
        let id = store.create_note(0.0, 0.0);
        assert!(store.pin_note(id));
        assert!(store.get_note(id).is_some_and(|n| n.pinned));
    }

    #[test]
    fn test_unpin_note() {
        let mut store = NoteStore::new();
        let id = store.create_note(0.0, 0.0);
        store.pin_note(id);
        assert!(store.unpin_note(id));
        assert!(!store.get_note(id).is_none_or(|n| n.pinned));
    }

    #[test]
    fn test_toggle_pin() {
        let mut store = NoteStore::new();
        let id = store.create_note(0.0, 0.0);
        store.toggle_pin(id);
        assert!(store.get_note(id).is_some_and(|n| n.pinned));
        store.toggle_pin(id);
        assert!(!store.get_note(id).is_none_or(|n| n.pinned));
    }

    // -- Z-order -------------------------------------------------------------

    #[test]
    fn test_bring_to_front() {
        let mut store = NoteStore::new();
        let id1 = store.create_note(0.0, 0.0);
        let id2 = store.create_note(100.0, 0.0);
        store.bring_to_front(id1);
        let z1 = store.get_note(id1).map(|n| n.z_order).unwrap_or(0);
        let z2 = store.get_note(id2).map(|n| n.z_order).unwrap_or(0);
        assert!(z1 > z2);
    }

    #[test]
    fn test_send_to_back() {
        let mut store = NoteStore::new();
        let id1 = store.create_note(0.0, 0.0);
        let id2 = store.create_note(100.0, 0.0);
        store.send_to_back(id2);
        let z1 = store.get_note(id1).map(|n| n.z_order).unwrap_or(0);
        let z2 = store.get_note(id2).map(|n| n.z_order).unwrap_or(0);
        assert!(z2 < z1);
    }

    // -- Which note a point lands on -----------------------------------------

    /// The renderer answers this now: it paints the notes back-to-front and
    /// `hit_test` reads them front-to-back, so the note drawn on top is the
    /// note that is clicked. A separate `note_at_point` computing it from the
    /// store was a second answer that could disagree with the picture.
    #[test]
    fn test_overlapping_notes_click_the_one_on_top() {
        let mut app = StickyNotesApp::new();
        let lower = app.store.create_note(40.0, 40.0);
        let upper = app.store.create_note(60.0, 60.0);
        let frame = app.draw(StickyNotesApp::SIZE);
        let canvas = app.canvas_rect(StickyNotesApp::SIZE.0, StickyNotesApp::SIZE.1);
        // A point inside both notes' title-bar band belongs to the upper one.
        let hit = frame.hit_test(canvas.x + 70.0, canvas.y + 66.0);
        assert_eq!(hit, Some(Target::NoteTitle(upper)));
        // A point only the lower one covers still belongs to it.
        let hit = frame.hit_test(canvas.x + 45.0, canvas.y + 46.0);
        assert_eq!(hit, Some(Target::NoteTitle(lower)));
    }

    #[test]
    fn test_pinned_note_is_clicked_over_a_later_one() {
        let mut app = StickyNotesApp::new();
        let pinned = app.store.create_note(40.0, 40.0);
        let _later = app.store.create_note(60.0, 60.0);
        app.store.toggle_pin(pinned);
        let frame = app.draw(StickyNotesApp::SIZE);
        let canvas = app.canvas_rect(StickyNotesApp::SIZE.0, StickyNotesApp::SIZE.1);
        assert_eq!(
            frame.hit_test(canvas.x + 70.0, canvas.y + 66.0),
            Some(Target::NoteTitle(pinned)),
            "a pinned note is drawn on top, so it must be clicked on top too"
        );
    }

    // -- Drag ----------------------------------------------------------------

    #[test]
    fn test_begin_move_and_update() {
        let mut store = NoteStore::new();
        let id = store.create_note(100.0, 100.0);
        store.begin_move(id, 110.0, 110.0);
        assert!(matches!(store.drag_state(), DragState::Moving { .. }));
        store.update_drag(210.0, 210.0);
        let note = store.get_note(id).expect("note exists");
        assert_eq!(note.x, 200.0);
        assert_eq!(note.y, 200.0);
        store.end_drag();
        assert_eq!(*store.drag_state(), DragState::None);
    }

    #[test]
    fn test_begin_move_pinned_note_does_nothing() {
        let mut store = NoteStore::new();
        let id = store.create_note(100.0, 100.0);
        store.pin_note(id);
        store.begin_move(id, 110.0, 110.0);
        assert_eq!(*store.drag_state(), DragState::None);
    }

    #[test]
    fn test_begin_resize_and_update() {
        let mut store = NoteStore::new();
        let id = store.create_note(0.0, 0.0);
        let orig_w = store.get_note(id).map(|n| n.width).unwrap_or(0.0);
        let orig_h = store.get_note(id).map(|n| n.height).unwrap_or(0.0);
        store.begin_resize(id, 200.0, 180.0);
        store.update_drag(250.0, 230.0);
        let note = store.get_note(id).expect("note exists");
        assert_eq!(note.width, orig_w + 50.0);
        assert_eq!(note.height, orig_h + 50.0);
    }

    // -- Snap to grid --------------------------------------------------------

    #[test]
    fn test_snap_to_grid() {
        assert_eq!(snap_to_grid(0.0), 0.0);
        assert_eq!(snap_to_grid(10.0), 20.0);
        assert_eq!(snap_to_grid(25.0), 20.0);
        assert_eq!(snap_to_grid(31.0), 40.0);
    }

    #[test]
    fn test_maybe_snap_enabled() {
        assert_eq!(maybe_snap(15.0, true), 20.0);
    }

    #[test]
    fn test_maybe_snap_disabled() {
        assert_eq!(maybe_snap(15.0, false), 15.0);
    }

    // -- Sidebar items -------------------------------------------------------

    #[test]
    fn test_sidebar_items() {
        let mut store = NoteStore::new();
        let id1 = store.create_note(0.0, 0.0);
        let id2 = store.create_note(100.0, 0.0);
        if let Some(n) = store.get_note_mut(id1) {
            n.title = "First".to_string();
        }
        if let Some(n) = store.get_note_mut(id2) {
            n.title = "Second".to_string();
            n.pinned = true;
        }
        let items = store.sidebar_items();
        assert_eq!(items.len(), 2);
        // Pinned note should be first.
        assert!(items[0].pinned);
        assert_eq!(items[0].title, "Second");
    }

    #[test]
    fn test_sidebar_items_filters_archived() {
        let mut store = NoteStore::new();
        let id1 = store.create_note(0.0, 0.0);
        let _id2 = store.create_note(100.0, 0.0);
        store.archive_note(id1);
        let items = store.sidebar_items();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_sidebar_items_respects_search() {
        let mut store = NoteStore::new();
        let id1 = store.create_note(0.0, 0.0);
        let id2 = store.create_note(100.0, 0.0);
        if let Some(n) = store.get_note_mut(id1) {
            n.title = "Shopping".to_string();
        }
        if let Some(n) = store.get_note_mut(id2) {
            n.title = "Work".to_string();
        }
        store.set_search("shop");
        let items = store.sidebar_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Shopping");
    }

    // -- Search store-level --------------------------------------------------

    #[test]
    fn test_store_search_results() {
        let mut store = NoteStore::new();
        let id1 = store.create_note(0.0, 0.0);
        let _id2 = store.create_note(100.0, 0.0);
        if let Some(n) = store.get_note_mut(id1) {
            n.title = "Todo".to_string();
        }
        store.set_search("todo");
        let results = store.search_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Todo");
    }

    // -- Tags store-level ----------------------------------------------------

    #[test]
    fn test_all_tags() {
        let mut store = NoteStore::new();
        let id1 = store.create_note(0.0, 0.0);
        let id2 = store.create_note(100.0, 0.0);
        if let Some(n) = store.get_note_mut(id1) {
            n.add_tag("work");
            n.add_tag("urgent");
        }
        if let Some(n) = store.get_note_mut(id2) {
            n.add_tag("work");
            n.add_tag("home");
        }
        let tags = store.all_tags();
        assert_eq!(tags, vec!["home", "urgent", "work"]);
    }

    #[test]
    fn test_notes_with_tag() {
        let mut store = NoteStore::new();
        let id1 = store.create_note(0.0, 0.0);
        let _id2 = store.create_note(100.0, 0.0);
        if let Some(n) = store.get_note_mut(id1) {
            n.add_tag("urgent");
        }
        let tagged = store.notes_with_tag("urgent");
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].id, id1);
    }

    // -- Dirty flag ----------------------------------------------------------

    #[test]
    fn test_dirty_flag() {
        let mut store = NoteStore::new();
        assert!(!store.is_dirty());
        store.create_note(0.0, 0.0);
        assert!(store.is_dirty());
        store.mark_clean();
        assert!(!store.is_dirty());
        store.mark_dirty();
        assert!(store.is_dirty());
    }

    // -- Serialization -------------------------------------------------------

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let mut store = NoteStore::new();
        let id1 = store.create_note(50.0, 75.0);
        if let Some(n) = store.get_note_mut(id1) {
            n.title = "Test Note".to_string();
            n.set_body_from_text("line 1\n* bullet\n[x] done");
            n.color_index = NoteColorIndex::Blue;
            n.pinned = true;
            n.add_tag("work");
            n.add_tag("home");
            n.font_size = FontSizePreset::Large;
        }
        let id2 = store.create_note(200.0, 100.0);
        store.archive_note(id2);
        store.set_snap_to_grid(true);
        store.set_sidebar_visible(true);

        let data = serialize_notes(&store);
        let restored = deserialize_notes(&data).expect("deserialization should succeed");

        assert_eq!(restored.total_count(), 2);
        assert!(restored.snap_to_grid_enabled());
        assert!(restored.sidebar_visible());

        let n1 = restored.get_note(id1).expect("note 1");
        assert_eq!(n1.title, "Test Note");
        assert_eq!(n1.x, 50.0);
        assert_eq!(n1.y, 75.0);
        assert_eq!(n1.color_index, NoteColorIndex::Blue);
        assert!(n1.pinned);
        assert_eq!(n1.tags, vec!["work", "home"]);
        assert_eq!(n1.font_size, FontSizePreset::Large);
        assert_eq!(n1.body.len(), 3);
        assert_eq!(n1.body[1].kind, LineKind::Bullet);
        assert_eq!(n1.body[2].kind, LineKind::Checkbox { checked: true });

        let n2 = restored.get_note(id2).expect("note 2");
        assert!(n2.archived);
    }

    #[test]
    fn test_deserialize_invalid_header() {
        assert!(deserialize_notes("GARBAGE|1").is_none());
    }

    #[test]
    fn test_deserialize_empty() {
        assert!(deserialize_notes("").is_none());
    }

    #[test]
    fn test_serialize_pipe_in_title() {
        let mut store = NoteStore::new();
        let id = store.create_note(0.0, 0.0);
        if let Some(n) = store.get_note_mut(id) {
            n.title = "A|B".to_string();
        }
        let data = serialize_notes(&store);
        let restored = deserialize_notes(&data).expect("should roundtrip");
        let n = restored.get_note(id).expect("note");
        assert_eq!(n.title, "A|B");
    }

    // -- Export --------------------------------------------------------------

    #[test]
    fn test_export_notes_as_text() {
        let mut store = NoteStore::new();
        let id = store.create_note(0.0, 0.0);
        if let Some(n) = store.get_note_mut(id) {
            n.title = "My Note".to_string();
            n.set_body_from_text("Hello world");
            n.add_tag("test");
        }
        let text = export_notes_as_text(&store);
        assert!(text.contains("My Note"));
        assert!(text.contains("Hello world"));
        assert!(text.contains("Tags: test"));
    }

    // -- Rich text parsing ---------------------------------------------------

    #[test]
    fn test_parse_rich_text_plain() {
        let lines = parse_rich_text("hello\nworld");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].kind, LineKind::Plain);
        assert_eq!(lines[0].plain_text(), "hello");
    }

    #[test]
    fn test_parse_rich_text_bullets() {
        let lines = parse_rich_text("* item1\n- item2");
        assert_eq!(lines[0].kind, LineKind::Bullet);
        assert_eq!(lines[0].plain_text(), "item1");
        assert_eq!(lines[1].kind, LineKind::Bullet);
    }

    #[test]
    fn test_parse_rich_text_checkboxes() {
        let lines = parse_rich_text("[x] done\n[ ] todo");
        assert_eq!(lines[0].kind, LineKind::Checkbox { checked: true });
        assert_eq!(lines[0].plain_text(), "done");
        assert_eq!(lines[1].kind, LineKind::Checkbox { checked: false });
        assert_eq!(lines[1].plain_text(), "todo");
    }

    #[test]
    fn test_parse_rich_text_empty() {
        let lines = parse_rich_text("");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].plain_text(), "");
    }

    // -- NoteColorIndex -----------------------------------------------------

    #[test]
    fn test_note_color_index_from_usize() {
        assert_eq!(NoteColorIndex::from_usize(0), Some(NoteColorIndex::Yellow));
        assert_eq!(NoteColorIndex::from_usize(7), Some(NoteColorIndex::Gray));
        assert_eq!(NoteColorIndex::from_usize(8), None);
    }

    #[test]
    fn test_note_color_index_name() {
        assert_eq!(NoteColorIndex::Yellow.name(), "Yellow");
        assert_eq!(NoteColorIndex::Teal.name(), "Teal");
    }

    // -- FontSizePreset ------------------------------------------------------

    #[test]
    fn test_font_size_preset_sizes() {
        assert!(FontSizePreset::Small.size() < FontSizePreset::Medium.size());
        assert!(FontSizePreset::Medium.size() < FontSizePreset::Large.size());
        assert!(FontSizePreset::Small.title_size() > FontSizePreset::Small.size());
    }

    #[test]
    fn test_font_size_preset_from_str() {
        assert_eq!(
            FontSizePreset::parse_label("Small"),
            Some(FontSizePreset::Small)
        );
        assert_eq!(
            FontSizePreset::parse_label("medium"),
            Some(FontSizePreset::Medium)
        );
        assert_eq!(
            FontSizePreset::parse_label("Large"),
            Some(FontSizePreset::Large)
        );
        assert_eq!(FontSizePreset::parse_label("???"), None);
    }

    // -- TextSpan / RichLine -------------------------------------------------

    #[test]
    fn test_text_span_plain() {
        let span = TextSpan::plain("hello");
        assert_eq!(span.text, "hello");
        assert!(!span.bold);
        assert!(!span.italic);
    }

    #[test]
    fn test_text_span_styled() {
        let span = TextSpan::styled("bold", true, false);
        assert!(span.bold);
        assert!(!span.italic);
    }

    #[test]
    fn test_rich_line_char_count() {
        let line = RichLine {
            kind: LineKind::Plain,
            spans: vec![TextSpan::plain("abc"), TextSpan::plain("de")],
        };
        assert_eq!(line.char_count(), 5);
    }

    // -- AutoSave -----------------------------------------------------------

    #[test]
    fn test_autosave_not_dirty_no_trigger() {
        let mut auto = AutoSave::new(5000);
        assert!(!auto.tick(6000, false));
    }

    #[test]
    fn test_autosave_triggers_after_interval() {
        let mut auto = AutoSave::new(5000);
        assert!(!auto.tick(3000, true));
        assert!(auto.tick(3000, true)); // 3000 + 3000 >= 5000
    }

    #[test]
    fn test_autosave_resets_after_trigger() {
        let mut auto = AutoSave::new(5000);
        auto.tick(5000, true);
        assert!(!auto.tick(1000, true)); // Reset after trigger.
    }

    // -- Rendering -----------------------------------------------------------

    #[test]
    fn test_frame_is_balanced_and_draws_something() {
        let mut app = StickyNotesApp::new();
        app.store.set_sidebar_visible(true);
        app.store.create_note(30.0, 30.0);
        let frame = app.draw(StickyNotesApp::SIZE);
        assert!(
            frame.is_balanced(),
            "every clip and translate must be undone, or the next frame inherits it"
        );
        assert!(!frame.into_tree().commands.is_empty());
    }

    #[test]
    fn test_active_note_gets_a_shadow_and_a_border() {
        let mut app = StickyNotesApp::new();
        let id = app.store.create_note(50.0, 50.0);
        app.store.set_active(Some(id));
        let tree = app.draw(StickyNotesApp::SIZE).into_tree();
        assert!(
            tree.commands
                .iter()
                .any(|c| matches!(c, RenderCommand::BoxShadow { .. })),
            "the note being worked on should lift off the desktop"
        );
        assert!(
            tree.commands
                .iter()
                .any(|c| matches!(c, RenderCommand::StrokeRect { .. })),
            "and carry a border saying so"
        );
    }

    #[test]
    fn test_sidebar_controls_appear_only_when_the_sidebar_is_open() {
        let mut app = StickyNotesApp::new();
        app.store.create_note(0.0, 0.0);
        assert!(
            !probe::is_visible(&app, Target::SearchBox),
            "a closed sidebar draws no search box, so nothing can click one"
        );
        app.store.set_sidebar_visible(true);
        assert!(probe::is_visible(&app, Target::SearchBox));
    }

    #[test]
    fn test_toolbar_note_actions_appear_only_with_a_note_selected() {
        let mut app = StickyNotesApp::new();
        let id = app.store.create_note(30.0, 30.0);
        assert!(!probe::is_visible(&app, Target::DeleteNote));
        assert!(probe::is_visible(&app, Target::NewNote));
        app.store.set_active(Some(id));
        assert!(probe::is_visible(&app, Target::DeleteNote));
        assert!(probe::is_visible(&app, Target::CycleColor));
    }

    /// A window too small for its own layout still draws every control.
    #[test]
    fn test_tiny_window_still_draws_its_controls() {
        let mut app = StickyNotesApp::new();
        app.store.set_sidebar_visible(true);
        app.store.create_note(10.0, 10.0);
        let frame = app.draw((120.0, 90.0));
        assert!(frame.is_balanced());
        let chip = frame
            .rect_of(|t| *t == Target::NewNote)
            .expect("the one control that is always available must survive the smallest window");
        // "Draws its controls" is a claim about paint, and a hit box is not
        // paint (lesson 81): `draw_chip` pushes its background under an `if
        // let Some(bg)` and its label as a third statement, so a hit box can
        // outlive both. Name the two things a reader would actually see, and
        // require them *inside* the box rather than merely overlapping it, so
        // the window's own full-bleed background cannot stand in for the chip
        // (lesson 83).
        assert!(
            frame.commands().iter().any(|c| matches!(
                c,
                RenderCommand::FillRect { x, y, width, height, .. }
                    if *width > 0.0
                        && *height > 0.0
                        && *x >= chip.x - 0.01
                        && *y >= chip.y - 0.01
                        && x + width <= chip.right() + 0.01
                        && y + height <= chip.bottom() + 0.01
            )),
            "the New Note chip is clickable at 120x90 but its body was not painted"
        );
        assert!(
            frame.commands().iter().any(|c| matches!(
                c,
                RenderCommand::Text { x, y, text, .. }
                    if text == "+ New"
                        && *x >= chip.x - 0.01
                        && *y >= chip.y - 0.01
                        && *x <= chip.right() + 0.01
            )),
            "the New Note chip was painted at 120x90 with no label on it"
        );
    }

    // -- NoteStore default ---------------------------------------------------

    #[test]
    fn test_note_store_default() {
        let store = NoteStore::default();
        assert_eq!(store.total_count(), 0);
        assert!(!store.sidebar_visible());
        assert!(!store.snap_to_grid_enabled());
        assert_eq!(store.active_note(), None);
    }

    // -- Color palette array -------------------------------------------------

    #[test]
    fn test_all_8_palettes_distinct() {
        for (i, a) in NOTE_COLORS.iter().enumerate() {
            for b in NOTE_COLORS.iter().skip(i + 1) {
                assert_ne!(a.light, b.light);
                assert_ne!(a.dark, b.dark);
            }
        }
    }

    // -- Sidebar text is elided to the sidebar, not sliced at a byte budget --

    /// Titles/bodies chosen so a naive `&text[..37]` lands mid-character.
    ///
    /// The 40-byte guard that used to live in `NoteStore::sidebar_items` was
    /// anti-protective: it fired *only* for strings long enough in bytes, and
    /// non-Latin scripts reach 40 bytes at ~13 characters, so the guard
    /// selected for exactly the inputs whose byte 37 is a continuation byte.
    fn adversarial_bodies() -> Vec<(String, String)> {
        vec![
            (
                "\u{4ed8}\u{7b8b}\u{306e}\u{30bf}\u{30a4}\u{30c8}\u{30eb}".to_string(),
                "\u{3053}\u{308c}\u{306f}\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{9577}\u{3044}\u{30e1}\u{30e2}\u{3067}\u{3059}\u{304b}\u{3089}\u{3001}\u{30d0}\u{30a4}\u{30c8}\u{6570}\u{304c}\u{6587}\u{5b57}\u{6570}\u{3092}\u{5927}\u{304d}\u{304f}\u{4e0a}\u{56de}\u{308a}\u{307e}\u{3059}".to_string(),
            ),
            (
                "\u{395}\u{3bb}\u{3bb}\u{3b7}\u{3bd}\u{3b9}\u{3ba}\u{3cc} \u{3c3}\u{3b7}\u{3bc}\u{3b5}\u{3af}\u{3c9}\u{3bc}\u{3b1}".to_string(),
                "\u{391}\u{3c5}\u{3c4}\u{3cc} \u{3b5}\u{3af}\u{3bd}\u{3b1}\u{3b9} \u{3ad}\u{3bd}\u{3b1} \u{3c0}\u{3bf}\u{3bb}\u{3cd} \u{3bc}\u{3b1}\u{3ba}\u{3c1}\u{3cd} \u{3ba}\u{3b5}\u{3af}\u{3bc}\u{3b5}\u{3bd}\u{3bf} \u{3c3}\u{3b7}\u{3bc}\u{3b5}\u{3b9}\u{3ce}\u{3c3}\u{3b5}\u{3c9}\u{3bd}".to_string(),
            ),
            (
                "\u{417}\u{430}\u{43c}\u{435}\u{442}\u{43a}\u{430} \u{43e} \u{432}\u{441}\u{451}\u{43c}".to_string(),
                "\u{42d}\u{442}\u{43e} \u{43e}\u{447}\u{435}\u{43d}\u{44c} \u{434}\u{43b}\u{438}\u{43d}\u{43d}\u{430}\u{44f} \u{437}\u{430}\u{43c}\u{435}\u{442}\u{43a}\u{430} \u{43d}\u{430} \u{440}\u{443}\u{441}\u{441}\u{43a}\u{43e}\u{43c} \u{44f}\u{437}\u{44b}\u{43a}\u{435}".to_string(),
            ),
            (
                "\u{1f4cc}\u{1f4dd}\u{1f5d2}\u{fe0f} \u{1f4a1}\u{1f9e0}\u{1f4da}".to_string(),
                "\u{1f4cc}\u{1f4dd}\u{1f5d2}\u{fe0f}\u{1f4a1}\u{1f9e0}\u{1f4da}\u{1f4c8}\u{1f4c9}\u{1f4ca}\u{1f5c3}\u{fe0f}\u{1f4c1}\u{1f4c2}\u{1f5df}\u{fe0f}".to_string(),
            ),
            // Byte 37 is deliberately the middle of a two-byte U+00E9.
            (
                format!("{}\u{e9}{}", "T".repeat(36), "t".repeat(30)),
                format!("{}\u{e9}{}", "a".repeat(36), "b".repeat(30)),
            ),
            ("Short".to_string(), "brief".to_string()),
        ]
    }

    fn sidebar_with_adversarial_notes() -> Vec<RenderCommand> {
        let mut app = StickyNotesApp::new();
        for (title, body) in adversarial_bodies() {
            let id = app.store.create_note(0.0, 0.0);
            if let Some(n) = app.store.get_note_mut(id) {
                n.title = title;
                n.set_body_from_text(&body);
            }
        }
        app.store.set_sidebar_visible(true);
        app.draw((WINDOW_WIDTH, 800.0)).into_tree().commands
    }

    /// The sidebar rows only, told apart from the toolbar's chips and the
    /// search field by the two font sizes nothing else in the window uses.
    fn sidebar_row_text(cmds: &[RenderCommand]) -> Vec<(&str, f32, f32, FontWeightHint)> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    text,
                    font_size,
                    font_weight,
                    ..
                } if (*font_size - SIDEBAR_TITLE_SIZE).abs() < f32::EPSILON
                    || (*font_size - SIDEBAR_PREVIEW_SIZE).abs() < f32::EPSILON =>
                {
                    Some((text.as_str(), *x, *font_size, *font_weight))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_non_ascii_note_does_not_abort_the_sidebar() {
        let cmds = sidebar_with_adversarial_notes();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn no_sidebar_text_escapes_the_sidebar() {
        let cmds = sidebar_with_adversarial_notes();
        let right_edge = SIDEBAR_WIDTH - 8.0;
        let mut checked = 0usize;
        for (t, x, font_size, font_weight) in sidebar_row_text(&cmds) {
            let w = text::measure(t, font_size, font_weight);
            assert!(
                x + w <= right_edge + 0.5,
                "sidebar row {t:?} at {x} draws {w} wide, past the sidebar edge {right_edge}"
            );
            checked += 1;
        }
        // One title + one preview per note; a vacuous pass would report 0.
        assert!(
            checked >= adversarial_bodies().len() * 2,
            "only checked {checked} sidebar rows"
        );
    }

    // ========================================================================
    // Driving the window the way a user does
    // ========================================================================

    const SIZE: (f32, f32) = StickyNotesApp::SIZE;

    /// An app with one note and the sidebar open, which is the state most of
    /// these start from.
    fn app_with_note() -> (StickyNotesApp, NoteId) {
        let mut app = StickyNotesApp::new();
        app.store.set_sidebar_visible(true);
        let id = app.store.create_note(60.0, 60.0);
        (app, id)
    }

    // -- Creating, selecting, archiving --------------------------------------

    #[test]
    fn clicking_new_makes_a_note_and_puts_the_caret_in_its_title() {
        let mut app = StickyNotesApp::new();
        assert_eq!(probe::click(&mut app, Target::NewNote), Action::Redraw);
        assert_eq!(app.store.visible_count(), 1);
        let id = app.store.active_note().expect("the new note is selected");
        // The caret is in the title, so the first thing typed names the note
        // rather than vanishing.
        probe::type_str(&mut app, "Groceries");
        let title = app.store.get_note(id).map(|n| n.title.clone());
        assert!(
            title.as_deref().is_some_and(|t| t.contains("Groceries")),
            "typing after New must reach the title, got {title:?}"
        );
    }

    #[test]
    fn new_notes_cascade_instead_of_stacking_exactly() {
        let mut app = StickyNotesApp::new();
        probe::click(&mut app, Target::NewNote);
        let first = app.store.active_note().expect("a note");
        probe::click(&mut app, Target::NewNote);
        let second = app.store.active_note().expect("a second note");
        let a = app.store.get_note(first).map(|n| (n.x, n.y));
        let b = app.store.get_note(second).map(|n| (n.x, n.y));
        assert_ne!(
            a, b,
            "two notes at the same spot look like one note that will not respond"
        );
    }

    #[test]
    fn clicking_a_title_selects_and_raises_the_note() {
        let mut app = StickyNotesApp::new();
        let lower = app.store.create_note(40.0, 40.0);
        let upper = app.store.create_note(300.0, 40.0);
        assert_eq!(app.store.active_note(), None);
        probe::click(&mut app, Target::NoteTitle(lower));
        assert_eq!(app.store.active_note(), Some(lower));
        let lower_z = app.store.get_note(lower).map(|n| n.z_order);
        let upper_z = app.store.get_note(upper).map(|n| n.z_order);
        assert!(lower_z > upper_z, "the clicked note comes to the front");
    }

    #[test]
    fn the_title_bar_x_archives_rather_than_deletes() {
        let (mut app, id) = app_with_note();
        probe::click(&mut app, Target::NoteArchive(id));
        assert_eq!(app.store.visible_count(), 0);
        assert_eq!(
            app.store.total_count(),
            1,
            "a one-click gesture with no confirmation must be reversible"
        );
        // And the way back is on the toolbar.
        probe::click(&mut app, Target::ToggleArchiveView);
        probe::click(&mut app, Target::SidebarItem(id));
        assert_eq!(app.store.visible_count(), 1);
    }

    #[test]
    fn delete_is_a_separate_control_from_archive() {
        let (mut app, id) = app_with_note();
        app.store.set_active(Some(id));
        probe::click(&mut app, Target::DeleteNote);
        assert_eq!(app.store.total_count(), 0, "Delete really deletes");
    }

    #[test]
    fn archiving_the_note_being_edited_drops_the_caret() {
        let (mut app, id) = app_with_note();
        probe::click(&mut app, Target::NoteLine(id, 0));
        probe::type_str(&mut app, "milk");
        probe::click(&mut app, Target::NoteArchive(id));
        // A caret still naming an archived note would swallow every later
        // keystroke, which reads exactly like a wedged keyboard.
        assert_eq!(app.focus, None);
        probe::click(&mut app, Target::NewNote);
        assert_eq!(app.store.visible_count(), 1);
    }

    // -- Dragging and resizing -----------------------------------------------

    fn mouse(x: f32, y: f32, kind: MouseEventKind) -> Event {
        Event::Mouse(guitk::event::MouseEvent { x, y, kind })
    }

    #[test]
    fn dragging_a_title_bar_moves_the_note() {
        let (mut app, id) = app_with_note();
        let title = probe::rect_of(&app, Target::NoteTitle(id)).expect("a title bar");
        let (sx, sy) = title.centre();
        let before = app.store.get_note(id).map(|n| (n.x, n.y)).expect("a note");

        app.handle_event(
            &mouse(sx, sy, MouseEventKind::Press(MouseButton::Left)),
            SIZE,
        );
        app.handle_event(&mouse(sx + 90.0, sy + 40.0, MouseEventKind::Move), SIZE);
        app.handle_event(
            &mouse(
                sx + 90.0,
                sy + 40.0,
                MouseEventKind::Release(MouseButton::Left),
            ),
            SIZE,
        );

        let after = app.store.get_note(id).map(|n| (n.x, n.y)).expect("a note");
        assert!(
            (after.0 - before.0 - 90.0).abs() < 1.0 && (after.1 - before.1 - 40.0).abs() < 1.0,
            "the note should have followed the pointer: {before:?} -> {after:?}"
        );
        assert!(matches!(app.store.drag_state(), DragState::None));
    }

    #[test]
    fn dragging_the_grip_resizes_the_note() {
        let (mut app, id) = app_with_note();
        let grip = probe::rect_of(&app, Target::NoteGrip(id)).expect("a grip");
        let (sx, sy) = grip.centre();
        let before = app
            .store
            .get_note(id)
            .map(|n| (n.width, n.height))
            .expect("a note");

        app.handle_event(
            &mouse(sx, sy, MouseEventKind::Press(MouseButton::Left)),
            SIZE,
        );
        app.handle_event(&mouse(sx + 60.0, sy + 50.0, MouseEventKind::Move), SIZE);
        app.handle_event(
            &mouse(
                sx + 60.0,
                sy + 50.0,
                MouseEventKind::Release(MouseButton::Left),
            ),
            SIZE,
        );

        let after = app
            .store
            .get_note(id)
            .map(|n| (n.width, n.height))
            .expect("a note");
        assert!(
            after.0 > before.0 && after.1 > before.1,
            "the grip should have grown the note: {before:?} -> {after:?}"
        );
    }

    #[test]
    fn a_note_cannot_be_dragged_out_of_reach() {
        let (mut app, id) = app_with_note();
        let title = probe::rect_of(&app, Target::NoteTitle(id)).expect("a title bar");
        let (sx, sy) = title.centre();
        app.handle_event(
            &mouse(sx, sy, MouseEventKind::Press(MouseButton::Left)),
            SIZE,
        );
        app.handle_event(&mouse(sx + 4000.0, sy + 4000.0, MouseEventKind::Move), SIZE);
        app.handle_event(
            &mouse(
                sx + 4000.0,
                sy + 4000.0,
                MouseEventKind::Release(MouseButton::Left),
            ),
            SIZE,
        );
        // The only handle a note has is the title bar the drag just pushed
        // away, so if it leaves the canvas nothing can bring it back.
        assert!(
            probe::rect_of(&app, Target::NoteTitle(id)).is_some(),
            "the note's title bar must still be on screen after an extreme drag"
        );
    }

    #[test]
    fn a_pinned_note_does_not_move() {
        let (mut app, id) = app_with_note();
        app.store.toggle_pin(id);
        let title = probe::rect_of(&app, Target::NoteTitle(id)).expect("a title bar");
        let (sx, sy) = title.centre();
        let before = app.store.get_note(id).map(|n| (n.x, n.y));
        app.handle_event(
            &mouse(sx, sy, MouseEventKind::Press(MouseButton::Left)),
            SIZE,
        );
        app.handle_event(&mouse(sx + 120.0, sy, MouseEventKind::Move), SIZE);
        app.handle_event(
            &mouse(sx + 120.0, sy, MouseEventKind::Release(MouseButton::Left)),
            SIZE,
        );
        assert_eq!(app.store.get_note(id).map(|n| (n.x, n.y)), before);
    }

    // -- Editing -------------------------------------------------------------

    #[test]
    fn clicking_a_body_line_puts_the_caret_where_the_click_landed() {
        let (mut app, id) = app_with_note();
        if let Some(note) = app.store.get_note_mut(id) {
            note.set_body_from_text("hello world");
        }
        let line = probe::rect_of(&app, Target::NoteLine(id, 0)).expect("a body line");
        // Click well into the text rather than at its start.
        app.handle_click(line.x + 60.0, line.centre().1, MouseButton::Left, SIZE);
        assert_eq!(app.focus, Some(Focus::Body(id)));
        assert!(
            app.caret.1 > 0,
            "clicking into the middle of a line should not send the caret to column 0"
        );
        probe::type_str(&mut app, "X");
        let body = app.store.get_note(id).map(Note::body_text);
        assert_eq!(body.as_deref().map(str::len), Some("hello worldX".len()));
        assert_ne!(
            body.as_deref(),
            Some("Xhello world"),
            "the character went in at the caret, not at the start"
        );
    }

    #[test]
    fn typing_into_a_body_line_marks_the_store_dirty() {
        let (mut app, id) = app_with_note();
        app.store.mark_clean();
        probe::click(&mut app, Target::NoteLine(id, 0));
        probe::type_str(&mut app, "eggs");
        assert_eq!(
            app.store.get_note(id).map(Note::body_text).as_deref(),
            Some("eggs")
        );
        assert!(app.store.is_dirty());
    }

    #[test]
    fn enter_splits_a_body_line_and_keeps_the_tail() {
        let (mut app, id) = app_with_note();
        probe::click(&mut app, Target::NoteLine(id, 0));
        probe::type_str(&mut app, "milkbread");
        app.caret = (0, 4);
        probe::key(&mut app, &probe::press(Key::Enter));
        assert_eq!(
            app.store.get_note(id).map(Note::body_text).as_deref(),
            Some("milk\nbread")
        );
        assert_eq!(app.caret, (1, 0));
    }

    #[test]
    fn backspace_at_column_zero_joins_onto_the_line_above() {
        let (mut app, id) = app_with_note();
        if let Some(note) = app.store.get_note_mut(id) {
            note.set_body_from_text("milk\nbread");
        }
        probe::click(&mut app, Target::NoteLine(id, 1));
        app.caret = (1, 0);
        probe::key(&mut app, &probe::press(Key::Backspace));
        assert_eq!(
            app.store.get_note(id).map(Note::body_text).as_deref(),
            Some("milkbread")
        );
        assert_eq!(app.caret, (0, 4), "the caret sits at the join");
    }

    #[test]
    fn a_multibyte_note_survives_editing() {
        let (mut app, id) = app_with_note();
        probe::click(&mut app, Target::NoteLine(id, 0));
        // Byte offsets and character offsets disagree everywhere here; an
        // insert or a delete that stepped by bytes would abort.
        probe::type_str(&mut app, "caf\u{e9} \u{3c3}\u{3b7}\u{3bc} \u{1f4dd}");
        probe::key(&mut app, &probe::press(Key::Home));
        probe::key(&mut app, &probe::press(Key::Right));
        probe::key(&mut app, &probe::press(Key::Right));
        probe::key(&mut app, &probe::press(Key::Delete));
        let body = app.store.get_note(id).map(Note::body_text).expect("a body");
        assert!(body.starts_with("ca"), "unexpected body {body:?}");
        assert!(!body.contains('f'), "the third character went, not a byte");
        // And the frame can still measure the caret against it.
        assert!(app.draw(SIZE).is_balanced());
    }

    #[test]
    fn the_title_is_edited_by_double_clicking_it() {
        let (mut app, id) = app_with_note();
        let title = probe::rect_of(&app, Target::NoteTitle(id)).expect("a title bar");
        let (tx, ty) = title.centre();
        // A single click starts a drag; the caret needs the second one.
        app.handle_event(
            &mouse(tx, ty, MouseEventKind::Press(MouseButton::Left)),
            SIZE,
        );
        assert_ne!(app.focus, Some(Focus::Title(id)));
        app.handle_event(
            &mouse(tx, ty, MouseEventKind::DoubleClick(MouseButton::Left)),
            SIZE,
        );
        assert_eq!(app.focus, Some(Focus::Title(id)));
        assert!(matches!(app.store.drag_state(), DragState::None));
    }

    #[test]
    fn a_hash_word_in_a_title_becomes_a_tag() {
        let (mut app, id) = app_with_note();
        if let Some(note) = app.store.get_note_mut(id) {
            note.title.clear();
        }
        app.focus = Some(Focus::Title(id));
        app.title_before = String::new();
        app.caret = (0, 0);
        probe::type_str(&mut app, "Milk #shopping");
        // Committed when the caret leaves, not per keystroke.
        probe::key(&mut app, &probe::press(Key::Escape));
        assert_eq!(
            app.store.get_note(id).map(|n| n.title.clone()).as_deref(),
            Some("Milk")
        );
        assert_eq!(app.store.all_tags(), vec![String::from("shopping")]);
    }

    #[test]
    fn a_whole_title_edit_is_one_undo() {
        let (mut app, id) = app_with_note();
        app.focus = Some(Focus::Title(id));
        app.title_before = app
            .store
            .get_note(id)
            .map(|n| n.title.clone())
            .expect("a title");
        app.caret = (0, app.title_before.len());
        probe::type_str(&mut app, "abcdef");
        probe::key(&mut app, &probe::press(Key::Escape));
        app.store.set_active(Some(id));
        probe::click(&mut app, Target::Undo);
        assert_eq!(
            app.store.get_note(id).map(|n| n.title.clone()).as_deref(),
            Some("New Note"),
            "one undo should restore the name the edit started from, \
             not walk back through six half-typed prefixes"
        );
    }

    // -- Checkboxes, line kinds, colours -------------------------------------

    #[test]
    fn clicking_the_box_ticks_it_and_clicking_the_words_does_not() {
        let (mut app, id) = app_with_note();
        if let Some(note) = app.store.get_note_mut(id) {
            note.body = vec![RichLine::checkbox("buy milk", false)];
        }
        probe::click(&mut app, Target::NoteCheck(id, 0));
        assert_eq!(
            app.store
                .get_note(id)
                .and_then(|n| n.body.first().map(|l| l.kind.clone())),
            Some(LineKind::Checkbox { checked: true })
        );
        // The words are a different target, and put the caret there instead.
        probe::click(&mut app, Target::NoteLine(id, 0));
        assert_eq!(
            app.store
                .get_note(id)
                .and_then(|n| n.body.first().map(|l| l.kind.clone())),
            Some(LineKind::Checkbox { checked: true }),
            "clicking the text of a checkbox line must not toggle it"
        );
    }

    #[test]
    fn the_line_chip_cycles_plain_bullet_checkbox() {
        let (mut app, id) = app_with_note();
        app.store.set_active(Some(id));
        app.caret = (0, 0);
        for expected in [
            LineKind::Bullet,
            LineKind::Checkbox { checked: false },
            LineKind::Plain,
        ] {
            probe::click(&mut app, Target::CycleLineKind);
            assert_eq!(
                app.store
                    .get_note(id)
                    .and_then(|n| n.body.first().map(|l| l.kind.clone())),
                Some(expected)
            );
        }
    }

    #[test]
    fn the_colour_chip_cycles_all_the_way_round() {
        let (mut app, id) = app_with_note();
        app.store.set_active(Some(id));
        let start = app.store.get_note(id).map(|n| n.color_index);
        for _ in 0..NOTE_COLORS.len() {
            probe::click(&mut app, Target::CycleColor);
        }
        assert_eq!(
            app.store.get_note(id).map(|n| n.color_index),
            start,
            "eight clicks should return to the colour it started on"
        );
    }

    // -- Search and tags -----------------------------------------------------

    #[test]
    fn typing_in_the_search_box_filters_the_sidebar() {
        let mut app = StickyNotesApp::new();
        app.store.set_sidebar_visible(true);
        let milk = app.store.create_note(20.0, 20.0);
        let bread = app.store.create_note(300.0, 20.0);
        if let Some(note) = app.store.get_note_mut(milk) {
            note.title = String::from("Milk");
        }
        if let Some(note) = app.store.get_note_mut(bread) {
            note.title = String::from("Bread");
        }

        probe::click(&mut app, Target::SearchBox);
        probe::type_str(&mut app, "mil");
        assert!(probe::is_visible(&app, Target::SidebarItem(milk)));
        assert!(
            !probe::is_visible(&app, Target::SidebarItem(bread)),
            "a filtered-out row must not be drawn, or it can still be clicked"
        );
        // Escape clears the search rather than leaving a filter nobody can see.
        probe::key(&mut app, &probe::press(Key::Escape));
        probe::key(&mut app, &probe::press(Key::Escape));
        assert!(probe::is_visible(&app, Target::SidebarItem(bread)));
    }

    #[test]
    fn a_tag_chip_filters_by_that_tag_exactly() {
        let mut app = StickyNotesApp::new();
        app.store.set_sidebar_visible(true);
        let tagged = app.store.create_note(20.0, 20.0);
        let other = app.store.create_note(300.0, 20.0);
        if let Some(note) = app.store.get_note_mut(tagged) {
            note.add_tag("work");
        }
        if let Some(note) = app.store.get_note_mut(other) {
            // The word "work" appears inside this title, so a substring search
            // would match it and a tag filter must not.
            note.title = String::from("homework");
        }

        probe::click(&mut app, Target::TagChip(0));
        assert!(probe::is_visible(&app, Target::SidebarItem(tagged)));
        assert!(
            !probe::is_visible(&app, Target::SidebarItem(other)),
            "a tag chip promises the notes carrying that tag, not the ones \
             whose title happens to contain the word"
        );
        // A second click on the same chip clears it.
        probe::click(&mut app, Target::TagChip(0));
        assert!(probe::is_visible(&app, Target::SidebarItem(other)));
    }

    #[test]
    fn scrolling_the_sidebar_stops_at_the_ends() {
        let mut app = StickyNotesApp::new();
        app.store.set_sidebar_visible(true);
        for i in 0..40 {
            app.store.create_note(10.0 + i as f32, 10.0);
        }
        app.handle_event(
            &mouse(20.0, 300.0, MouseEventKind::Scroll { dx: 0.0, dy: -50.0 }),
            SIZE,
        );
        let far = app.sidebar_scroll;
        assert!(far > 0.0, "the list is longer than the pane, so it scrolls");
        app.handle_event(
            &mouse(20.0, 300.0, MouseEventKind::Scroll { dx: 0.0, dy: -50.0 }),
            SIZE,
        );
        assert!(
            (app.sidebar_scroll - far).abs() < f32::EPSILON,
            "scrolling past the end must stop rather than run off"
        );
        app.handle_event(
            &mouse(20.0, 300.0, MouseEventKind::Scroll { dx: 0.0, dy: 500.0 }),
            SIZE,
        );
        assert!(app.sidebar_scroll <= 0.0);
    }

    // -- Layout ---------------------------------------------------------------

    #[test]
    fn opening_the_sidebar_moves_the_notes_beside_it_not_under_it() {
        let mut app = StickyNotesApp::new();
        let id = app.store.create_note(10.0, 100.0);
        let closed = probe::rect_of(&app, Target::NoteTitle(id)).expect("a title bar");
        probe::click(&mut app, Target::ToggleSidebar);
        let opened = probe::rect_of(&app, Target::NoteTitle(id)).expect("a title bar");
        assert!(
            opened.x >= SIDEBAR_WIDTH - 0.5,
            "a note under the sidebar is invisible and still clickable: {opened:?}"
        );
        assert!(opened.x > closed.x);
    }

    #[test]
    fn shrinking_the_window_keeps_every_note_reachable() {
        let mut app = StickyNotesApp::new();
        let far = app.store.create_note(800.0, 500.0);
        app.handle_event(
            &Event::Resize {
                width: 520,
                height: 360,
            },
            SIZE,
        );
        assert!(
            probe::rect_of_sized(&app, Target::NoteTitle(far), (520.0, 360.0)).is_some(),
            "a note left outside a shrunken window can never be dragged back"
        );
    }

    // -- Keyboard ------------------------------------------------------------

    #[test]
    fn ctrl_q_quits_and_a_bare_q_does_not() {
        let mut app = StickyNotesApp::new();
        assert_eq!(probe::key(&mut app, &probe::press(Key::Q)), Action::None);
        assert_eq!(probe::key(&mut app, &probe::ctrl(Key::Q)), Action::Quit);
    }

    #[test]
    fn shortcuts_do_not_fire_while_alt_or_super_is_held() {
        let mut app = StickyNotesApp::new();
        for modifiers in [
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        ] {
            // Alt-Tab and the Super menu belong to the window manager; a
            // shortcut that fired under them would fire while the window was
            // being switched away from.
            let event = probe::press_with(Key::N, modifiers);
            assert_eq!(probe::key(&mut app, &event), Action::None);
            assert_eq!(app.store.total_count(), 0, "{modifiers:?} made a note");
        }
    }

    #[test]
    fn ctrl_n_makes_a_note_and_ctrl_b_toggles_the_sidebar() {
        let mut app = StickyNotesApp::new();
        probe::key(&mut app, &probe::ctrl(Key::N));
        assert_eq!(app.store.visible_count(), 1);
        assert!(!app.store.sidebar_visible());
        probe::key(&mut app, &probe::ctrl(Key::B));
        assert!(app.store.sidebar_visible());
        probe::key(&mut app, &probe::ctrl(Key::B));
        assert!(!app.store.sidebar_visible());
    }

    #[test]
    fn ctrl_z_and_ctrl_y_walk_the_undo_stack() {
        let (mut app, id) = app_with_note();
        probe::click(&mut app, Target::NoteLine(id, 0));
        probe::type_str(&mut app, "ab");
        assert_eq!(
            app.store.get_note(id).map(Note::body_text).as_deref(),
            Some("ab")
        );
        probe::key(&mut app, &probe::ctrl(Key::Z));
        assert_eq!(
            app.store.get_note(id).map(Note::body_text).as_deref(),
            Some("a"),
            "undo has to actually apply the recorded action, not just pop it"
        );
        probe::key(&mut app, &probe::ctrl(Key::Y));
        assert_eq!(
            app.store.get_note(id).map(Note::body_text).as_deref(),
            Some("ab")
        );
    }

    #[test]
    fn ctrl_f_opens_the_sidebar_and_focuses_the_search_box() {
        let mut app = StickyNotesApp::new();
        app.store.create_note(20.0, 20.0);
        probe::key(&mut app, &probe::ctrl(Key::F));
        assert!(app.store.sidebar_visible());
        probe::type_str(&mut app, "zz");
        assert_eq!(app.store.search_query(), "zz");
    }

    #[test]
    fn delete_with_nothing_focused_archives_rather_than_destroys() {
        let (mut app, id) = app_with_note();
        app.store.set_active(Some(id));
        app.focus = None;
        probe::key(&mut app, &probe::press(Key::Delete));
        assert_eq!(app.store.visible_count(), 0);
        assert_eq!(app.store.total_count(), 1);
    }

    // -- Saving --------------------------------------------------------------

    #[test]
    fn an_app_with_nowhere_to_write_does_not_ask_for_ticks() {
        let (mut app, id) = app_with_note();
        probe::click(&mut app, Target::NoteLine(id, 0));
        probe::type_str(&mut app, "x");
        assert!(app.store.is_dirty());
        assert_eq!(
            app.tick_interval(),
            None,
            "a window with nothing to save must not wake the compositor"
        );
    }

    #[test]
    fn a_dirty_note_ticks_and_a_saved_one_stops() {
        let scratch = ScratchDir::new("stickynotes-tick");
        let mut app = StickyNotesApp::with_storage(scratch.path("notes.txt"));
        assert_eq!(app.tick_interval(), None, "nothing typed yet");
        let id = app.store.create_note(20.0, 20.0);
        probe::click(&mut app, Target::NoteLine(id, 0));
        probe::type_str(&mut app, "x");
        assert_eq!(app.tick_interval(), Some(TICK));
        // Enough ticks to cross the autosave interval.
        for _ in 0..=(AUTOSAVE_MS / 500) {
            app.tick(500);
        }
        assert!(app.saves() >= 1, "the autosave should have fired");
        assert_eq!(app.tick_interval(), None, "and then stopped asking");
    }

    #[test]
    fn notes_survive_a_save_and_a_reopen() {
        let scratch = ScratchDir::new("stickynotes-roundtrip");
        let path = scratch.path("notes.txt");
        let id = {
            let mut app = StickyNotesApp::with_storage(path.clone());
            app.store.set_snap_to_grid(true);
            let id = app.store.create_note(120.0, 90.0);
            if let Some(note) = app.store.get_note_mut(id) {
                note.title = String::from("Caf\u{e9} \u{1f4dd}");
                note.set_body_from_text("* one\n[x] two\nthree");
                note.add_tag("work");
            }
            app.store.mark_dirty();
            assert_eq!(app.persist(), Action::Redraw);
            assert!(!app.store.is_dirty(), "a save marks the store clean");
            id
        };

        let reopened = StickyNotesApp::with_storage(path);
        assert_eq!(reopened.store.visible_count(), 1);
        let note = reopened.store.get_note(id).expect("the note came back");
        assert_eq!(note.title, "Caf\u{e9} \u{1f4dd}");
        assert_eq!(note.body_text(), "* one\n[x] two\nthree");
        assert_eq!(note.tags, vec![String::from("work")]);
        assert!(reopened.store.snap_to_grid_enabled());
        assert!(
            !reopened.store.is_dirty(),
            "a file just read is not a change waiting to be written back"
        );
    }

    /// The save must go through `safeio`, which writes beside the target and
    /// renames over it. `fs::write` truncates first, so an interrupted save
    /// destroys every note rather than one — and the two leave byte-identical
    /// files, so nothing except the counter can tell them apart.
    #[test]
    fn the_save_is_atomic() {
        let scratch = ScratchDir::new("stickynotes-atomic");
        let mut app = StickyNotesApp::with_storage(scratch.path("notes.txt"));
        app.store.create_note(10.0, 10.0);
        let before = safeio::writes_performed();
        app.persist();
        assert!(
            safeio::writes_performed() > before,
            "the notes file must be written through safeio::write_atomically"
        );
    }

    #[test]
    fn a_corrupt_line_costs_one_note_and_not_the_file() {
        let mut store = NoteStore::new();
        store.create_note(10.0, 10.0);
        store.create_note(20.0, 20.0);
        let mut text = serialize_notes(&store);
        text.push_str("this line is not a note\n");
        let reloaded = deserialize_notes(&text).expect("the file still parses");
        assert_eq!(
            reloaded.total_count(),
            2,
            "one bad line must not abandon the notes around it — an empty \
             store here would be overwritten onto the file by the next autosave"
        );
    }

    #[test]
    fn closing_the_window_saves_first() {
        let scratch = ScratchDir::new("stickynotes-close");
        let path = scratch.path("notes.txt");
        let mut app = StickyNotesApp::with_storage(path.clone());
        let id = app.store.create_note(30.0, 30.0);
        probe::click(&mut app, Target::NoteLine(id, 0));
        probe::type_str(&mut app, "remember this");
        assert_eq!(app.handle_event(&Event::CloseRequested, SIZE), Action::Quit);
        let reopened = StickyNotesApp::with_storage(path);
        assert_eq!(
            reopened.store.get_note(id).map(Note::body_text).as_deref(),
            Some("remember this"),
            "what was typed must be on disk before the window goes away"
        );
    }

    #[test]
    fn export_writes_a_readable_copy_beside_the_notes() {
        let scratch = ScratchDir::new("stickynotes-export");
        let mut app = StickyNotesApp::with_storage(scratch.path("notes.txt"));
        let id = app.store.create_note(10.0, 10.0);
        if let Some(note) = app.store.get_note_mut(id) {
            note.title = String::from("Shopping");
        }
        probe::key(&mut app, &probe::ctrl(Key::E));
        let exported = fs::read_to_string(scratch.path("notes.export.txt"))
            .expect("the export file was written");
        assert!(exported.contains("Shopping"), "got {exported:?}");
    }

    #[test]
    fn a_failed_save_is_reported_and_retried() {
        let scratch = ScratchDir::new("stickynotes-failure");
        // A directory where the file should be: the write cannot succeed, and
        // the store must stay dirty so the next autosave tries again rather
        // than believing the notes are safe.
        let path = scratch.path("notes.txt");
        fs::create_dir_all(&path).expect("a directory in the file's place");
        let mut app = StickyNotesApp::with_storage(path);
        app.store.create_note(10.0, 10.0);
        app.persist();
        assert_eq!(app.saves(), 0);
        assert!(
            app.store.is_dirty(),
            "a failed save must not look like a save"
        );
        assert!(
            app.status().starts_with("Save failed"),
            "and it must say so: {:?}",
            app.status()
        );
    }

    #[test]
    fn a_short_note_is_drawn_verbatim() {
        let cmds = sidebar_with_adversarial_notes();
        let drawn: Vec<&str> = sidebar_row_text(&cmds)
            .into_iter()
            .map(|(t, ..)| t)
            .collect();
        assert!(
            drawn.contains(&"Short"),
            "short title was altered: {drawn:?}"
        );
        assert!(
            drawn.contains(&"brief"),
            "short preview was altered: {drawn:?}"
        );
    }
}
