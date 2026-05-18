//! OurOS Sticky Notes
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

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

// VecDeque used for potential future features (e.g., recent-notes list).
#[allow(unused_imports)]
use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

// Full palette is defined for completeness; not all colors are used yet.
#[allow(dead_code)]
const BASE: Color = Color::from_hex(0x1E1E2E);
#[allow(dead_code)]
const SURFACE0: Color = Color::from_hex(0x313244);
#[allow(dead_code)]
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
#[allow(dead_code)]
const BLUE: Color = Color::from_hex(0x89B4FA);
#[allow(dead_code)]
const GREEN: Color = Color::from_hex(0xA6E3A1);
#[allow(dead_code)]
const RED: Color = Color::from_hex(0xF38BA8);
#[allow(dead_code)]
const YELLOW: Color = Color::from_hex(0xF9E2AF);
#[allow(dead_code)]
const PEACH: Color = Color::from_hex(0xFAB387);
#[allow(dead_code)]
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
#[allow(dead_code)]
const OVERLAY0: Color = Color::from_hex(0x6C7086);
#[allow(dead_code)]
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
    NoteColorPalette { light: Color::from_hex(0xF9E2AF), dark: Color::from_hex(0x45420E) },
    // Pink
    NoteColorPalette { light: Color::from_hex(0xF5C2E7), dark: Color::from_hex(0x452535) },
    // Blue
    NoteColorPalette { light: Color::from_hex(0x89B4FA), dark: Color::from_hex(0x1E2D45) },
    // Green
    NoteColorPalette { light: Color::from_hex(0xA6E3A1), dark: Color::from_hex(0x1E3A1E) },
    // Purple
    NoteColorPalette { light: Color::from_hex(0xCBA6F7), dark: Color::from_hex(0x2E1E45) },
    // Orange
    NoteColorPalette { light: Color::from_hex(0xFAB387), dark: Color::from_hex(0x452A1E) },
    // Teal
    NoteColorPalette { light: Color::from_hex(0x94E2D5), dark: Color::from_hex(0x1E3A35) },
    // Gray
    NoteColorPalette { light: Color::from_hex(0xA6ADC8), dark: Color::from_hex(0x2A2A3A) },
];

pub fn note_palette(index: NoteColorIndex) -> NoteColorPalette {
    NOTE_COLORS[index.as_usize()]
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

    pub fn name(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
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
    InsertChar { line: usize, col: usize, ch: char },
    DeleteChar { line: usize, col: usize, ch: char },
    InsertLine { line: usize, content: RichLine },
    DeleteLine { line: usize, content: RichLine },
    SetTitle { old: String, new: String },
    ReplaceBody { old: Vec<RichLine>, new: Vec<RichLine> },
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

    /// Check if a point (px, py) is within this note's bounding rectangle.
    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }

    /// Check if a point is in the title bar area (top 30px).
    pub fn in_title_bar(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + 30.0
    }

    /// Check if a point is in the resize handle (bottom-right 16x16 corner).
    pub fn in_resize_handle(&self, px: f32, py: f32) -> bool {
        let rx = self.x + self.width - 16.0;
        let ry = self.y + self.height - 16.0;
        px >= rx && px <= rx + 16.0 && py >= ry && py <= ry + 16.0
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
        if let Some(line) = self.body.get_mut(line_index) {
            if let LineKind::Checkbox { checked } = &mut line.kind {
                *checked = !*checked;
                return true;
            }
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
                        if *checked { "[x] ".to_string() } else { "[ ] ".to_string() }
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

    /// Insert a character at a specific line/column position.
    pub fn insert_char(&mut self, line: usize, col: usize, ch: char) {
        if let Some(rich_line) = self.body.get_mut(line) {
            if let Some(span) = rich_line.spans.first_mut() {
                let col = col.min(span.text.len());
                span.text.insert(col, ch);
                self.undo_history
                    .push(EditAction::InsertChar { line, col, ch });
            }
        }
    }

    /// Delete a character at a specific line/column position.
    pub fn delete_char(&mut self, line: usize, col: usize) -> Option<char> {
        if let Some(rich_line) = self.body.get_mut(line) {
            if let Some(span) = rich_line.spans.first_mut() {
                if col < span.text.len() {
                    let ch = span.text.remove(col);
                    self.undo_history
                        .push(EditAction::DeleteChar { line, col, ch });
                    return Some(ch);
                }
            }
        }
        None
    }

    /// Insert a new line at the given index.
    pub fn insert_line(&mut self, index: usize, content: RichLine) {
        let idx = index.min(self.body.len());
        self.undo_history
            .push(EditAction::InsertLine { line: idx, content: content.clone() });
        self.body.insert(idx, content);
    }

    /// Delete a line at the given index.
    pub fn delete_line(&mut self, index: usize) -> Option<RichLine> {
        if index < self.body.len() && self.body.len() > 1 {
            let removed = self.body.remove(index);
            self.undo_history
                .push(EditAction::DeleteLine { line: index, content: removed.clone() });
            Some(removed)
        } else {
            None
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
        if let Some(rest) = trimmed.strip_prefix("[x] ").or_else(|| trimmed.strip_prefix("[X] ")) {
            lines.push(RichLine::checkbox(rest, true));
        } else if let Some(rest) = trimmed.strip_prefix("[ ] ") {
            lines.push(RichLine::checkbox(rest, false));
        } else if let Some(rest) = trimmed.strip_prefix("* ").or_else(|| trimmed.strip_prefix("- ")) {
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
    if snap_enabled { snap_to_grid(value) } else { value }
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
        let mut tags: Vec<String> = self
            .notes
            .iter()
            .flat_map(|n| n.tags.clone())
            .collect();
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

    /// Find which note is at a given point (topmost by z-order, non-archived).
    pub fn note_at_point(&self, x: f32, y: f32) -> Option<NoteId> {
        // Pinned notes are always on top, then check z-order descending.
        let mut candidates: Vec<&Note> = self.notes.iter().filter(|n| !n.archived && n.contains_point(x, y)).collect();
        candidates.sort_by(|a, b| {
            // Pinned notes first, then by z-order descending.
            b.pinned.cmp(&a.pinned).then(b.z_order.cmp(&a.z_order))
        });
        candidates.first().map(|n| n.id)
    }

    /// Get a compact sidebar list of all visible notes (title + first line preview).
    pub fn sidebar_items(&self) -> Vec<SidebarItem> {
        let mut items: Vec<SidebarItem> = self
            .notes
            .iter()
            .filter(|n| !n.archived && n.matches_search(&self.search_query))
            .map(|n| SidebarItem {
                id: n.id,
                title: n.title.clone(),
                preview: n.body.first().map_or(String::new(), |l| {
                    let text = l.plain_text();
                    if text.len() > 40 {
                        format!("{}...", &text[..37])
                    } else {
                        text
                    }
                }),
                color_index: n.color_index,
                pinned: n.pinned,
                tag_count: n.tags.len(),
            })
            .collect();
        items.sort_by(|a, b| b.pinned.cmp(&a.pinned));
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
        let title_str = note.title.replace('|', "\\p").replace('\\', "\\\\");
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
        if parts.len() < 13 {
            continue;
        }
        let id: NoteId = parts[0].parse().ok()?;
        let title = parts[1].replace("\\p", "|").replace("\\\\", "\\");
        let x: f32 = parts[2].parse().ok()?;
        let y: f32 = parts[3].parse().ok()?;
        let width: f32 = parts[4].parse().ok()?;
        let height: f32 = parts[5].parse().ok()?;
        let color_idx: usize = parts[6].parse().ok()?;
        let pinned: bool = parts[7].parse().ok()?;
        let archived: bool = parts[8].parse().ok()?;
        let z_order: u32 = parts[9].parse().ok()?;
        let font_size_str = parts[10];
        let tags_str = parts[11];
        let body_str = parts[12]
            .replace("\\n", "\n")
            .replace("\\p", "|")
            .replace("\\\\", "\\");

        let color_index = NoteColorIndex::from_usize(color_idx).unwrap_or(NoteColorIndex::Yellow);
        let font_size = FontSizePreset::from_str(font_size_str).unwrap_or(FontSizePreset::Medium);
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
// Render commands generation
// ============================================================================

/// Title bar height.
const TITLE_BAR_HEIGHT: f32 = 30.0;
/// Resize handle size.
const RESIZE_HANDLE: f32 = 16.0;
/// Sidebar width.
const SIDEBAR_WIDTH: f32 = 240.0;
/// Search bar height.
const SEARCH_BAR_HEIGHT: f32 = 36.0;

/// Generate render commands for a single sticky note.
pub fn render_note(note: &Note, is_active: bool) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();
    let palette = note.palette();
    let corner = CornerRadii::all(8.0);
    let font = note.font_size.size();
    let title_font = note.font_size.title_size();

    // Drop shadow for active note.
    if is_active {
        cmds.push(RenderCommand::BoxShadow {
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
    cmds.push(RenderCommand::FillRect {
        x: note.x,
        y: note.y,
        width: note.width,
        height: note.height,
        color: palette.dark,
        corner_radii: corner,
    });

    // Title bar.
    let title_corner = CornerRadii {
        top_left: 8.0,
        top_right: 8.0,
        bottom_left: 0.0,
        bottom_right: 0.0,
    };
    cmds.push(RenderCommand::FillRect {
        x: note.x,
        y: note.y,
        width: note.width,
        height: TITLE_BAR_HEIGHT,
        color: palette.light,
        corner_radii: title_corner,
    });

    // Title text.
    let title_display = if note.pinned {
        format!("[P] {}", note.title)
    } else {
        note.title.clone()
    };
    cmds.push(RenderCommand::Text {
        x: note.x + 8.0,
        y: note.y + 6.0,
        text: title_display,
        color: MANTLE,
        font_size: title_font,
        font_weight: FontWeightHint::Bold,
        max_width: Some(note.width - 40.0),
    });

    // Close button (X) in title bar.
    cmds.push(RenderCommand::Text {
        x: note.x + note.width - 22.0,
        y: note.y + 6.0,
        text: "X".to_string(),
        color: Color::rgba(0, 0, 0, 150),
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    // Body text — render each line.
    let body_top = note.y + TITLE_BAR_HEIGHT + 6.0;
    let line_height = font * 1.5;
    let max_body_h = note.height - TITLE_BAR_HEIGHT - RESIZE_HANDLE - 6.0;
    let max_lines = (max_body_h / line_height).floor() as usize;

    for (i, line) in note.body.iter().enumerate() {
        if i >= max_lines {
            break;
        }
        let ly = body_top + i as f32 * line_height;

        // Prefix for bullet/checkbox.
        let prefix = match &line.kind {
            LineKind::Plain => "",
            LineKind::Bullet => "  * ",
            LineKind::Checkbox { checked } => {
                if *checked {
                    "  [x] "
                } else {
                    "  [ ] "
                }
            }
        };

        let full_text = format!("{}{}", prefix, line.plain_text());
        let weight = if line.spans.first().is_some_and(|s| s.bold) {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        };

        cmds.push(RenderCommand::Text {
            x: note.x + 8.0,
            y: ly,
            text: full_text,
            color: TEXT_COLOR,
            font_size: font,
            font_weight: weight,
            max_width: Some(note.width - 16.0),
        });
    }

    // Resize handle indicator (small triangle in bottom-right).
    cmds.push(RenderCommand::Line {
        x1: note.x + note.width - 4.0,
        y1: note.y + note.height - RESIZE_HANDLE,
        x2: note.x + note.width - RESIZE_HANDLE,
        y2: note.y + note.height - 4.0,
        color: Color::rgba(palette.light.r, palette.light.g, palette.light.b, 120),
        width: 2.0,
    });

    // Active border.
    if is_active {
        cmds.push(RenderCommand::StrokeRect {
            x: note.x,
            y: note.y,
            width: note.width,
            height: note.height,
            color: BLUE,
            line_width: 2.0,
            corner_radii: corner,
        });
    }

    cmds
}

/// Generate render commands for the sidebar.
pub fn render_sidebar(store: &NoteStore, viewport_height: f32) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();
    if !store.sidebar_visible() {
        return cmds;
    }

    // Sidebar background.
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: SIDEBAR_WIDTH,
        height: viewport_height,
        color: SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    // Sidebar header.
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: SIDEBAR_WIDTH,
        height: SEARCH_BAR_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Search label.
    let search_text = if store.search_query().is_empty() {
        "Search notes...".to_string()
    } else {
        store.search_query().to_string()
    };
    let search_color = if store.search_query().is_empty() {
        OVERLAY0
    } else {
        TEXT_COLOR
    };
    cmds.push(RenderCommand::Text {
        x: 10.0,
        y: 10.0,
        text: search_text,
        color: search_color,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(SIDEBAR_WIDTH - 20.0),
    });

    // Note list.
    let items = store.sidebar_items();
    let item_height = 52.0;
    for (i, item) in items.iter().enumerate() {
        let iy = SEARCH_BAR_HEIGHT + i as f32 * item_height;
        let is_selected = store.active_note() == Some(item.id);

        // Item background.
        let bg = if is_selected { BASE } else { SURFACE0 };
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: iy,
            width: SIDEBAR_WIDTH,
            height: item_height,
            color: bg,
            corner_radii: CornerRadii::ZERO,
        });

        // Color indicator dot.
        let dot_color = note_palette(item.color_index).light;
        cmds.push(RenderCommand::FillRect {
            x: 8.0,
            y: iy + 8.0,
            width: 10.0,
            height: 10.0,
            color: dot_color,
            corner_radii: CornerRadii::all(5.0),
        });

        // Pin indicator.
        let title_prefix = if item.pinned { "[P] " } else { "" };

        // Title.
        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: iy + 6.0,
            text: format!("{}{}", title_prefix, item.title),
            color: TEXT_COLOR,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 32.0),
        });

        // Preview.
        cmds.push(RenderCommand::Text {
            x: 24.0,
            y: iy + 24.0,
            text: item.preview.clone(),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(SIDEBAR_WIDTH - 32.0),
        });

        // Separator line.
        cmds.push(RenderCommand::Line {
            x1: 8.0,
            y1: iy + item_height - 1.0,
            x2: SIDEBAR_WIDTH - 8.0,
            y2: iy + item_height - 1.0,
            color: OVERLAY0,
            width: 1.0,
        });
    }

    cmds
}

/// Generate all render commands for the full sticky notes desktop.
pub fn render_all(store: &NoteStore, viewport_width: f32, viewport_height: f32) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    // Desktop background.
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: viewport_width,
        height: viewport_height,
        color: BASE,
        corner_radii: CornerRadii::ZERO,
    });

    // Sidebar.
    cmds.extend(render_sidebar(store, viewport_height));

    // Notes — render in z-order (lowest first, so highest draws on top).
    let notes = store.visible_notes();
    let active = store.active_note();
    for note in &notes {
        if note.pinned {
            continue; // Render pinned notes last (on top).
        }
        let is_active = active == Some(note.id);
        cmds.extend(render_note(note, is_active));
    }
    // Pinned notes on top.
    for note in &notes {
        if note.pinned {
            let is_active = active == Some(note.id);
            cmds.extend(render_note(note, is_active));
        }
    }

    // Toolbar at bottom.
    cmds.extend(render_toolbar(store, viewport_width, viewport_height));

    cmds
}

/// Render a small toolbar at the bottom of the screen.
fn render_toolbar(store: &NoteStore, viewport_width: f32, viewport_height: f32) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();
    let bar_h = 32.0;
    let bar_y = viewport_height - bar_h;

    // Bar background.
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: bar_y,
        width: viewport_width,
        height: bar_h,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // "+ New" button.
    cmds.push(RenderCommand::FillRect {
        x: 8.0,
        y: bar_y + 4.0,
        width: 60.0,
        height: 24.0,
        color: GREEN,
        corner_radii: CornerRadii::all(4.0),
    });
    cmds.push(RenderCommand::Text {
        x: 16.0,
        y: bar_y + 8.0,
        text: "+ New".to_string(),
        color: MANTLE,
        font_size: 13.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    // Note count.
    let count_text = format!(
        "{} notes ({} archived)",
        store.visible_count(),
        store.archived_count()
    );
    cmds.push(RenderCommand::Text {
        x: 80.0,
        y: bar_y + 9.0,
        text: count_text,
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // Snap-to-grid indicator.
    let snap_text = if store.snap_to_grid_enabled() {
        "Grid: ON"
    } else {
        "Grid: OFF"
    };
    cmds.push(RenderCommand::Text {
        x: viewport_width - 160.0,
        y: bar_y + 9.0,
        text: snap_text.to_string(),
        color: if store.snap_to_grid_enabled() { GREEN } else { OVERLAY0 },
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // Sidebar toggle.
    let sidebar_text = if store.sidebar_visible() { "Sidebar: ON" } else { "Sidebar: OFF" };
    cmds.push(RenderCommand::Text {
        x: viewport_width - 80.0,
        y: bar_y + 9.0,
        text: sidebar_text.to_string(),
        color: if store.sidebar_visible() { BLUE } else { OVERLAY0 },
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    cmds
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
// Main (placeholder — real entry point is the OS window manager integration)
// ============================================================================

fn main() {
    // Placeholder: the actual app lifecycle is driven by the compositor/WM
    // event loop via guitk. In a real session the WM creates a window, feeds
    // events, and the app returns render commands each frame.
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
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

    #[test]
    fn test_note_contains_point() {
        let note = Note::new(1, 10.0, 20.0);
        assert!(note.contains_point(15.0, 25.0));
        assert!(note.contains_point(10.0, 20.0));
        assert!(note.contains_point(230.0, 220.0)); // edge
        assert!(!note.contains_point(5.0, 25.0));
        assert!(!note.contains_point(15.0, 5.0));
        assert!(!note.contains_point(300.0, 25.0));
    }

    #[test]
    fn test_note_title_bar_hit() {
        let note = Note::new(1, 0.0, 0.0);
        assert!(note.in_title_bar(10.0, 10.0));
        assert!(note.in_title_bar(100.0, 29.0));
        assert!(!note.in_title_bar(10.0, 35.0));
    }

    #[test]
    fn test_note_resize_handle_hit() {
        let note = Note::new(1, 0.0, 0.0);
        // Bottom-right 16x16.
        assert!(note.in_resize_handle(210.0, 190.0));
        assert!(!note.in_resize_handle(10.0, 10.0));
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
        note.body = vec![
            RichLine::plain("line 1"),
            RichLine::plain("line 2"),
        ];
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
        h.push(EditAction::InsertChar { line: 0, col: 0, ch: 'a' });
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
        h.push(EditAction::InsertChar { line: 0, col: 0, ch: 'a' });
        h.pop_undo();
        let action = h.pop_redo();
        assert!(action.is_some());
        assert!(h.can_undo());
    }

    #[test]
    fn test_undo_history_push_clears_redo() {
        let mut h = UndoHistory::new(10);
        h.push(EditAction::InsertChar { line: 0, col: 0, ch: 'a' });
        h.pop_undo();
        assert!(h.can_redo());
        h.push(EditAction::InsertChar { line: 0, col: 0, ch: 'b' });
        assert!(!h.can_redo());
    }

    #[test]
    fn test_undo_history_max_depth() {
        let mut h = UndoHistory::new(3);
        h.push(EditAction::InsertChar { line: 0, col: 0, ch: 'a' });
        h.push(EditAction::InsertChar { line: 0, col: 1, ch: 'b' });
        h.push(EditAction::InsertChar { line: 0, col: 2, ch: 'c' });
        h.push(EditAction::InsertChar { line: 0, col: 3, ch: 'd' });
        assert_eq!(h.undo_count(), 3);
    }

    #[test]
    fn test_undo_history_clear() {
        let mut h = UndoHistory::new(10);
        h.push(EditAction::InsertChar { line: 0, col: 0, ch: 'x' });
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
        assert_eq!(store.get_note(id).map(|n| n.color_index), Some(NoteColorIndex::Blue));
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
        assert!(store.get_note(id).map_or(false, |n| n.pinned));
    }

    #[test]
    fn test_unpin_note() {
        let mut store = NoteStore::new();
        let id = store.create_note(0.0, 0.0);
        store.pin_note(id);
        assert!(store.unpin_note(id));
        assert!(!store.get_note(id).map_or(true, |n| n.pinned));
    }

    #[test]
    fn test_toggle_pin() {
        let mut store = NoteStore::new();
        let id = store.create_note(0.0, 0.0);
        store.toggle_pin(id);
        assert!(store.get_note(id).map_or(false, |n| n.pinned));
        store.toggle_pin(id);
        assert!(!store.get_note(id).map_or(true, |n| n.pinned));
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

    // -- Note-at-point -------------------------------------------------------

    #[test]
    fn test_note_at_point_topmost() {
        let mut store = NoteStore::new();
        let id1 = store.create_note(0.0, 0.0); // z=1
        let id2 = store.create_note(10.0, 10.0); // z=2 (overlaps)
        // id2 is on top because higher z_order.
        assert_eq!(store.note_at_point(15.0, 15.0), Some(id2));
        // Point only in id1.
        assert_eq!(store.note_at_point(5.0, 5.0), Some(id1));
    }

    #[test]
    fn test_note_at_point_pinned_on_top() {
        let mut store = NoteStore::new();
        let id1 = store.create_note(0.0, 0.0);
        let _id2 = store.create_note(10.0, 10.0);
        store.pin_note(id1);
        // Pinned note is on top even with lower z.
        assert_eq!(store.note_at_point(15.0, 15.0), Some(id1));
    }

    #[test]
    fn test_note_at_point_no_hit() {
        let store = NoteStore::new();
        assert_eq!(store.note_at_point(500.0, 500.0), None);
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
        assert_eq!(FontSizePreset::from_str("Small"), Some(FontSizePreset::Small));
        assert_eq!(FontSizePreset::from_str("medium"), Some(FontSizePreset::Medium));
        assert_eq!(FontSizePreset::from_str("Large"), Some(FontSizePreset::Large));
        assert_eq!(FontSizePreset::from_str("???"), None);
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
            spans: vec![
                TextSpan::plain("abc"),
                TextSpan::plain("de"),
            ],
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

    // -- Render commands (smoke tests) ---------------------------------------

    #[test]
    fn test_render_note_produces_commands() {
        let note = Note::new(1, 50.0, 50.0);
        let cmds = render_note(&note, false);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_active_note_has_shadow() {
        let note = Note::new(1, 50.0, 50.0);
        let cmds = render_note(&note, true);
        let has_shadow = cmds.iter().any(|c| matches!(c, RenderCommand::BoxShadow { .. }));
        assert!(has_shadow);
    }

    #[test]
    fn test_render_active_note_has_border() {
        let note = Note::new(1, 50.0, 50.0);
        let cmds = render_note(&note, true);
        let has_stroke = cmds.iter().any(|c| matches!(c, RenderCommand::StrokeRect { .. }));
        assert!(has_stroke);
    }

    #[test]
    fn test_render_all_produces_commands() {
        let mut store = NoteStore::new();
        store.create_note(10.0, 10.0);
        let cmds = render_all(&store, 1280.0, 720.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_sidebar_hidden() {
        let store = NoteStore::new();
        let cmds = render_sidebar(&store, 720.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_render_sidebar_visible() {
        let mut store = NoteStore::new();
        store.set_sidebar_visible(true);
        store.create_note(0.0, 0.0);
        let cmds = render_sidebar(&store, 720.0);
        assert!(!cmds.is_empty());
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
        for i in 0..8 {
            for j in (i + 1)..8 {
                assert_ne!(NOTE_COLORS[i].light, NOTE_COLORS[j].light);
                assert_ne!(NOTE_COLORS[i].dark, NOTE_COLORS[j].dark);
            }
        }
    }
}
