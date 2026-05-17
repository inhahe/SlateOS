#![allow(dead_code)]
//! Path bar widget — combined breadcrumb display / text input with autocomplete.
//!
//! Operates in two modes:
//! - **Breadcrumb mode** (default): shows the path as clickable segments separated by ">"
//! - **Edit mode**: full text input with autocomplete dropdown for directory navigation
//!
//! The widget does not perform filesystem I/O. It emits `PathBarEvent::RequestAutoComplete`
//! and the host provides completions via `set_completions()`.

use crate::color::Color;
use crate::event::{EventResult, Key, KeyEvent, MouseEvent, MouseEventKind};
use crate::render::{FontWeightHint, RenderCommand};
use crate::style::CornerRadii;

// ---------------------------------------------------------------------------
// Catppuccin Mocha palette
// ---------------------------------------------------------------------------

/// Base background (dark).
const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
/// Slightly lighter surface for segments/inputs.
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
/// Overlay for dropdowns.
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
/// Highlighted item in dropdown.
const COLOR_SURFACE2: Color = Color::from_hex(0x585B70);
/// Primary text.
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
/// Subdued/dim text.
const COLOR_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
/// Accent (lavender) for cursor, selection.
const COLOR_LAVENDER: Color = Color::from_hex(0xB4BEFE);
/// Error/invalid (red).
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
/// Directory icon hint (blue).
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
/// Shadow color.
const COLOR_SHADOW: Color = Color::rgba(0, 0, 0, 100);

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const FONT_SIZE: f32 = 14.0;
const SEGMENT_PADDING_H: f32 = 8.0;
const SEGMENT_PADDING_V: f32 = 4.0;
const SEGMENT_GAP: f32 = 2.0;
const SEGMENT_RADIUS: f32 = 4.0;
const SEPARATOR_WIDTH: f32 = 16.0;
const BAR_PADDING: f32 = 4.0;
const DROPDOWN_ITEM_HEIGHT: f32 = 24.0;
const DROPDOWN_MAX_VISIBLE: usize = 8;
const DROPDOWN_PADDING: f32 = 4.0;
const CURSOR_WIDTH: f32 = 2.0;

// Rough character width for a 14px monospace/proportional font.
const CHAR_WIDTH: f32 = 8.0;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A completion item provided by the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionItem {
    pub name: String,
    pub is_directory: bool,
}

/// Events emitted by the path bar for the host to handle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathBarEvent {
    /// User navigated to a new path (clicked a breadcrumb or pressed Enter).
    Navigate(String),
    /// Widget requests autocomplete results for the given prefix.
    RequestAutoComplete { prefix: String },
    /// Edit mode was entered.
    EditModeEntered,
    /// Edit mode was exited.
    EditModeExited,
}

/// Current display mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Breadcrumb,
    Edit,
}

/// The path bar widget.
#[derive(Clone, Debug)]
pub struct PathBar {
    /// Current confirmed path (what breadcrumb mode displays).
    path: String,
    /// Parsed segments of `path`.
    segments: Vec<String>,

    /// Current mode.
    mode: Mode,

    // --- Edit mode state ---
    /// The text being edited.
    edit_text: String,
    /// Cursor position (byte offset into `edit_text`).
    cursor: usize,
    /// Selection anchor (byte offset), if any. Selection is anchor..cursor or cursor..anchor.
    selection_anchor: Option<usize>,

    // --- Autocomplete state ---
    /// Available completions from the host.
    completions: Vec<CompletionItem>,
    /// Index of highlighted completion (None = no highlight).
    completion_index: Option<usize>,
    /// Whether the dropdown is visible.
    dropdown_visible: bool,
    /// Scroll offset in dropdown (first visible item index).
    dropdown_scroll: usize,

    // --- Validation ---
    /// Whether the currently typed path is considered invalid.
    path_invalid: bool,

    // --- Pending events ---
    pending_events: Vec<PathBarEvent>,

    // --- Layout cache (computed during render) ---
    /// Cached segment x-positions for hit testing.
    segment_rects: Vec<(f32, f32, f32, f32)>, // (x, y, w, h)
}

impl PathBar {
    /// Create a new path bar with the given initial path.
    pub fn new(initial_path: &str) -> Self {
        let path = normalize_path(initial_path);
        let segments = split_path(&path);
        Self {
            path,
            segments,
            mode: Mode::Breadcrumb,
            edit_text: String::new(),
            cursor: 0,
            selection_anchor: None,
            completions: Vec::new(),
            completion_index: None,
            dropdown_visible: false,
            dropdown_scroll: 0,
            path_invalid: false,
            pending_events: Vec::new(),
            segment_rects: Vec::new(),
        }
    }

    /// Update the displayed path (resets to breadcrumb mode).
    pub fn set_path(&mut self, path: &str) {
        self.path = normalize_path(path);
        self.segments = split_path(&self.path);
        self.exit_edit_mode(false);
    }

    /// Current confirmed path.
    pub fn current_path(&self) -> &str {
        &self.path
    }

    /// Provide autocomplete results from the host.
    pub fn set_completions(&mut self, items: Vec<CompletionItem>) {
        self.completions = items;
        self.completion_index = if self.completions.is_empty() {
            None
        } else {
            Some(0)
        };
        self.dropdown_visible = !self.completions.is_empty();
        self.dropdown_scroll = 0;
    }

    /// Mark whether the current edit text represents a valid path.
    pub fn set_path_valid(&mut self, valid: bool) {
        self.path_invalid = !valid;
    }

    /// Drain all pending events.
    pub fn drain_events(&mut self) -> Vec<PathBarEvent> {
        core::mem::take(&mut self.pending_events)
    }

    /// Whether the widget is currently in edit mode.
    pub fn is_editing(&self) -> bool {
        self.mode == Mode::Edit
    }

    // -----------------------------------------------------------------------
    // Event handling
    // -----------------------------------------------------------------------

    /// Handle a key event. Returns `Consumed` if the widget used the event.
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Ignored;
        }

        // Ctrl+L always enters edit mode regardless of current mode.
        if event.modifiers.ctrl && event.key == Key::L {
            self.enter_edit_mode();
            return EventResult::Consumed;
        }

        match self.mode {
            Mode::Breadcrumb => self.handle_key_breadcrumb(event),
            Mode::Edit => self.handle_key_edit(event),
        }
    }

    /// Handle a mouse event. Returns `Consumed` if the widget used the event.
    pub fn handle_mouse_event(&mut self, event: &MouseEvent) -> EventResult {
        match &event.kind {
            MouseEventKind::Press(crate::event::MouseButton::Left) => {
                self.handle_click(event.x, event.y)
            }
            _ => EventResult::Ignored,
        }
    }

    /// Render the path bar into a list of render commands.
    pub fn render(&mut self, width: u32, height: u32) -> Vec<RenderCommand> {
        let w = width as f32;
        let h = height as f32;
        let mut cmds = Vec::new();

        // Background fill.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(SEGMENT_RADIUS),
        });

        // Border (red if invalid in edit mode).
        let border_color = if self.mode == Mode::Edit && self.path_invalid {
            COLOR_RED
        } else {
            COLOR_SURFACE1
        };
        cmds.push(RenderCommand::StrokeRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: border_color,
            line_width: 1.0,
            corner_radii: CornerRadii::all(SEGMENT_RADIUS),
        });

        match self.mode {
            Mode::Breadcrumb => self.render_breadcrumb(&mut cmds, w, h),
            Mode::Edit => self.render_edit(&mut cmds, w, h),
        }

        cmds
    }

    // -----------------------------------------------------------------------
    // Mode transitions
    // -----------------------------------------------------------------------

    fn enter_edit_mode(&mut self) {
        if self.mode == Mode::Edit {
            return;
        }
        self.mode = Mode::Edit;
        self.edit_text = self.path.clone();
        self.cursor = self.edit_text.len();
        self.selection_anchor = None;
        self.completions.clear();
        self.completion_index = None;
        self.dropdown_visible = false;
        self.path_invalid = false;
        self.pending_events.push(PathBarEvent::EditModeEntered);
    }

    fn exit_edit_mode(&mut self, revert: bool) {
        if self.mode != Mode::Edit {
            return;
        }
        self.mode = Mode::Breadcrumb;
        if !revert {
            // Path was already updated by the caller.
        }
        self.edit_text.clear();
        self.cursor = 0;
        self.selection_anchor = None;
        self.completions.clear();
        self.completion_index = None;
        self.dropdown_visible = false;
        self.path_invalid = false;
        self.pending_events.push(PathBarEvent::EditModeExited);
    }

    // -----------------------------------------------------------------------
    // Breadcrumb mode key handling
    // -----------------------------------------------------------------------

    fn handle_key_breadcrumb(&mut self, event: &KeyEvent) -> EventResult {
        // Any printable character enters edit mode.
        if let Some(ch) = event.text {
            if !ch.is_control() {
                self.enter_edit_mode();
                // Insert the typed character.
                self.edit_text.clear();
                self.edit_text.push(ch);
                self.cursor = self.edit_text.len();
                return EventResult::Consumed;
            }
        }
        EventResult::Ignored
    }

    // -----------------------------------------------------------------------
    // Edit mode key handling
    // -----------------------------------------------------------------------

    fn handle_key_edit(&mut self, event: &KeyEvent) -> EventResult {
        match event.key {
            Key::Escape => {
                self.exit_edit_mode(true);
                EventResult::Consumed
            }
            Key::Enter => {
                self.navigate_to_edit_text();
                EventResult::Consumed
            }
            Key::Tab => {
                self.accept_completion();
                EventResult::Consumed
            }
            Key::Up => {
                self.move_completion_up();
                EventResult::Consumed
            }
            Key::Down => {
                self.move_completion_down();
                EventResult::Consumed
            }
            Key::Left => {
                if self.dropdown_visible && self.completion_index.is_some() {
                    // Right accepts in some UIs, left does nothing special.
                    // But in edit mode Left moves cursor.
                }
                self.move_cursor_left(event.modifiers.shift);
                EventResult::Consumed
            }
            Key::Right => {
                if self.dropdown_visible && self.completion_index.is_some() {
                    self.accept_completion();
                } else {
                    self.move_cursor_right(event.modifiers.shift);
                }
                EventResult::Consumed
            }
            Key::Home => {
                self.move_cursor_home(event.modifiers.shift);
                EventResult::Consumed
            }
            Key::End => {
                self.move_cursor_end(event.modifiers.shift);
                EventResult::Consumed
            }
            Key::Backspace => {
                self.handle_backspace();
                EventResult::Consumed
            }
            Key::Delete => {
                self.handle_delete();
                EventResult::Consumed
            }
            Key::A if event.modifiers.ctrl => {
                // Select all.
                self.selection_anchor = Some(0);
                self.cursor = self.edit_text.len();
                EventResult::Consumed
            }
            _ => {
                // Insert character.
                if let Some(ch) = event.text {
                    if !ch.is_control() {
                        self.delete_selection();
                        self.edit_text.insert(self.cursor, ch);
                        self.cursor += ch.len_utf8();
                        self.selection_anchor = None;
                        self.on_text_changed();
                        return EventResult::Consumed;
                    }
                }
                EventResult::Ignored
            }
        }
    }

    // -----------------------------------------------------------------------
    // Text editing helpers
    // -----------------------------------------------------------------------

    fn move_cursor_left(&mut self, extend_selection: bool) {
        if !extend_selection {
            // If there's a selection, collapse to its start.
            if let Some(anchor) = self.selection_anchor {
                self.cursor = self.cursor.min(anchor);
                self.selection_anchor = None;
                return;
            }
        } else if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }

        if self.cursor > 0 {
            // Move back one character (handle UTF-8).
            let s = &self.edit_text[..self.cursor];
            if let Some(ch) = s.chars().next_back() {
                self.cursor -= ch.len_utf8();
            }
        }
    }

    fn move_cursor_right(&mut self, extend_selection: bool) {
        if !extend_selection {
            if let Some(anchor) = self.selection_anchor {
                self.cursor = self.cursor.max(anchor);
                self.selection_anchor = None;
                return;
            }
        } else if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }

        if self.cursor < self.edit_text.len() {
            let s = &self.edit_text[self.cursor..];
            if let Some(ch) = s.chars().next() {
                self.cursor += ch.len_utf8();
            }
        }
    }

    fn move_cursor_home(&mut self, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !extend_selection {
            self.selection_anchor = None;
        }
        self.cursor = 0;
    }

    fn move_cursor_end(&mut self, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !extend_selection {
            self.selection_anchor = None;
        }
        self.cursor = self.edit_text.len();
    }

    fn handle_backspace(&mut self) {
        if self.delete_selection() {
            self.on_text_changed();
            return;
        }
        if self.cursor > 0 {
            let s = &self.edit_text[..self.cursor];
            if let Some(ch) = s.chars().next_back() {
                let new_cursor = self.cursor - ch.len_utf8();
                self.edit_text.remove(new_cursor);
                self.cursor = new_cursor;
                self.on_text_changed();
            }
        }
    }

    fn handle_delete(&mut self) {
        if self.delete_selection() {
            self.on_text_changed();
            return;
        }
        if self.cursor < self.edit_text.len() {
            self.edit_text.remove(self.cursor);
            self.on_text_changed();
        }
    }

    /// Delete the current selection, returning true if something was deleted.
    fn delete_selection(&mut self) -> bool {
        if let Some(anchor) = self.selection_anchor.take() {
            let start = self.cursor.min(anchor);
            let end = self.cursor.max(anchor);
            if start != end {
                self.edit_text.drain(start..end);
                self.cursor = start;
                return true;
            }
        }
        false
    }

    /// Called whenever edit text changes — requests autocomplete.
    fn on_text_changed(&mut self) {
        // Determine the prefix for autocomplete: everything up to and including the last '/'.
        let prefix = autocomplete_prefix(&self.edit_text, self.cursor);
        self.pending_events.push(PathBarEvent::RequestAutoComplete {
            prefix: prefix.to_string(),
        });
    }

    // -----------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------

    fn navigate_to_edit_text(&mut self) {
        let new_path = normalize_path(&self.edit_text);
        self.path = new_path.clone();
        self.segments = split_path(&self.path);
        self.pending_events.push(PathBarEvent::Navigate(new_path));
        self.exit_edit_mode(false);
    }

    fn navigate_to_segment(&mut self, segment_index: usize) {
        // Build path from segments[0..=segment_index].
        let new_path = rebuild_path(&self.segments, segment_index);
        self.path = new_path.clone();
        self.segments = split_path(&self.path);
        self.pending_events.push(PathBarEvent::Navigate(new_path));
    }

    // -----------------------------------------------------------------------
    // Autocomplete
    // -----------------------------------------------------------------------

    fn move_completion_up(&mut self) {
        if !self.dropdown_visible || self.completions.is_empty() {
            return;
        }
        match self.completion_index {
            Some(0) | None => {
                self.completion_index = Some(self.completions.len().saturating_sub(1));
            }
            Some(i) => {
                self.completion_index = Some(i - 1);
            }
        }
        self.ensure_completion_visible();
    }

    fn move_completion_down(&mut self) {
        if !self.dropdown_visible || self.completions.is_empty() {
            return;
        }
        match self.completion_index {
            None => {
                self.completion_index = Some(0);
            }
            Some(i) => {
                if i + 1 >= self.completions.len() {
                    self.completion_index = Some(0);
                } else {
                    self.completion_index = Some(i + 1);
                }
            }
        }
        self.ensure_completion_visible();
    }

    fn ensure_completion_visible(&mut self) {
        if let Some(idx) = self.completion_index {
            if idx < self.dropdown_scroll {
                self.dropdown_scroll = idx;
            } else if idx >= self.dropdown_scroll + DROPDOWN_MAX_VISIBLE {
                self.dropdown_scroll = idx + 1 - DROPDOWN_MAX_VISIBLE;
            }
        }
    }

    fn accept_completion(&mut self) {
        if !self.dropdown_visible {
            return;
        }
        let idx = match self.completion_index {
            Some(i) if i < self.completions.len() => i,
            _ => return,
        };

        let item = self.completions[idx].clone();

        // Replace the partial name after the last '/' with the completion.
        let prefix_end = self.edit_text[..self.cursor]
            .rfind('/')
            .map_or(0, |pos| pos + 1);

        // Remove everything after the prefix up to cursor.
        self.edit_text.drain(prefix_end..self.cursor);
        self.cursor = prefix_end;

        // Insert the completion name.
        let insert = if item.is_directory {
            format!("{}/", item.name)
        } else {
            item.name.clone()
        };
        self.edit_text.insert_str(self.cursor, &insert);
        self.cursor += insert.len();

        self.selection_anchor = None;
        self.dropdown_visible = false;
        self.completions.clear();
        self.completion_index = None;

        // Request new completions if we just completed a directory.
        if item.is_directory {
            self.on_text_changed();
        }
    }

    // -----------------------------------------------------------------------
    // Mouse handling
    // -----------------------------------------------------------------------

    fn handle_click(&mut self, x: f32, y: f32) -> EventResult {
        match self.mode {
            Mode::Breadcrumb => {
                // Check if click is on a segment.
                for (i, &(sx, sy, sw, sh)) in self.segment_rects.iter().enumerate() {
                    if x >= sx && x <= sx + sw && y >= sy && y <= sy + sh {
                        self.navigate_to_segment(i);
                        return EventResult::Consumed;
                    }
                }
                // Click on empty area enters edit mode.
                self.enter_edit_mode();
                EventResult::Consumed
            }
            Mode::Edit => {
                // Click in dropdown?
                // For now, position cursor based on x.
                let text_x = BAR_PADDING;
                let char_offset = ((x - text_x) / CHAR_WIDTH).max(0.0) as usize;
                let new_cursor = char_to_byte_offset(&self.edit_text, char_offset);
                self.cursor = new_cursor;
                self.selection_anchor = None;
                EventResult::Consumed
            }
        }
    }

    // -----------------------------------------------------------------------
    // Rendering — Breadcrumb mode
    // -----------------------------------------------------------------------

    fn render_breadcrumb(&mut self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        self.segment_rects.clear();
        let y_center = height / 2.0;

        // Calculate total width needed for all segments.
        let mut total_width = BAR_PADDING;
        let mut seg_widths: Vec<f32> = Vec::new();
        for seg in &self.segments {
            let text_w = seg.len() as f32 * CHAR_WIDTH;
            let seg_w = text_w + SEGMENT_PADDING_H * 2.0;
            seg_widths.push(seg_w);
            total_width += seg_w + SEGMENT_GAP + SEPARATOR_WIDTH;
        }
        // Remove trailing gap+separator.
        if !self.segments.is_empty() {
            total_width -= SEGMENT_GAP + SEPARATOR_WIDTH;
        }
        total_width += BAR_PADDING;

        // Determine overflow: if total_width > width, hide leading segments.
        let overflow = total_width > width;
        let first_visible = if overflow {
            // Find the first segment that fits from the right.
            let available = width - BAR_PADDING * 2.0 - SEPARATOR_WIDTH - 20.0; // 20 for "..."
            let mut accum = 0.0f32;
            let mut first = self.segments.len();
            for i in (0..self.segments.len()).rev() {
                let seg_total = seg_widths[i] + SEGMENT_GAP + SEPARATOR_WIDTH;
                if accum + seg_total > available {
                    break;
                }
                accum += seg_total;
                first = i;
            }
            first
        } else {
            0
        };

        let mut x = BAR_PADDING;

        // Render overflow indicator.
        if overflow && first_visible > 0 {
            let ellipsis = "...";
            let ew = ellipsis.len() as f32 * CHAR_WIDTH;
            cmds.push(RenderCommand::FillRect {
                x,
                y: y_center - (FONT_SIZE + SEGMENT_PADDING_V * 2.0) / 2.0,
                width: ew + SEGMENT_PADDING_H * 2.0,
                height: FONT_SIZE + SEGMENT_PADDING_V * 2.0,
                color: COLOR_SURFACE0,
                corner_radii: CornerRadii::all(SEGMENT_RADIUS),
            });
            cmds.push(RenderCommand::Text {
                x: x + SEGMENT_PADDING_H,
                y: y_center - FONT_SIZE / 2.0,
                text: ellipsis.to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            x += ew + SEGMENT_PADDING_H * 2.0 + SEGMENT_GAP;

            // Separator after ellipsis.
            cmds.push(RenderCommand::Text {
                x,
                y: y_center - FONT_SIZE / 2.0,
                text: ">".to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            x += SEPARATOR_WIDTH;
        }

        // Render visible segments.
        for i in first_visible..self.segments.len() {
            let seg = &self.segments[i];
            let text_w = seg.len() as f32 * CHAR_WIDTH;
            let seg_w = text_w + SEGMENT_PADDING_H * 2.0;
            let seg_h = FONT_SIZE + SEGMENT_PADDING_V * 2.0;
            let seg_y = y_center - seg_h / 2.0;

            // Segment background.
            cmds.push(RenderCommand::FillRect {
                x,
                y: seg_y,
                width: seg_w,
                height: seg_h,
                color: COLOR_SURFACE0,
                corner_radii: CornerRadii::all(SEGMENT_RADIUS),
            });

            // Segment text.
            cmds.push(RenderCommand::Text {
                x: x + SEGMENT_PADDING_H,
                y: y_center - FONT_SIZE / 2.0,
                text: seg.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Store rect for hit testing.
            self.segment_rects.push((x, seg_y, seg_w, seg_h));

            x += seg_w + SEGMENT_GAP;

            // Separator (except after last).
            if i < self.segments.len() - 1 {
                cmds.push(RenderCommand::Text {
                    x,
                    y: y_center - FONT_SIZE / 2.0,
                    text: ">".to_string(),
                    color: COLOR_SUBTEXT0,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                x += SEPARATOR_WIDTH;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Rendering — Edit mode
    // -----------------------------------------------------------------------

    fn render_edit(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        let y_center = height / 2.0;
        let text_y = y_center - FONT_SIZE / 2.0;
        let text_x = BAR_PADDING + 4.0;

        // Inner background (slightly darker for input feel).
        cmds.push(RenderCommand::FillRect {
            x: 2.0,
            y: 2.0,
            width: width - 4.0,
            height: height - 4.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(SEGMENT_RADIUS - 1.0),
        });

        // Selection highlight.
        if let Some(anchor) = self.selection_anchor {
            let sel_start = self.cursor.min(anchor);
            let sel_end = self.cursor.max(anchor);
            let start_chars = byte_to_char_offset(&self.edit_text, sel_start);
            let end_chars = byte_to_char_offset(&self.edit_text, sel_end);
            let sel_x = text_x + start_chars as f32 * CHAR_WIDTH;
            let sel_w = (end_chars - start_chars) as f32 * CHAR_WIDTH;
            cmds.push(RenderCommand::FillRect {
                x: sel_x,
                y: text_y - 2.0,
                width: sel_w,
                height: FONT_SIZE + 4.0,
                color: Color::rgba(
                    COLOR_LAVENDER.r,
                    COLOR_LAVENDER.g,
                    COLOR_LAVENDER.b,
                    60,
                ),
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Text.
        cmds.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: self.edit_text.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - BAR_PADDING * 2.0 - 8.0),
        });

        // Cursor.
        let cursor_chars = byte_to_char_offset(&self.edit_text, self.cursor);
        let cursor_x = text_x + cursor_chars as f32 * CHAR_WIDTH;
        cmds.push(RenderCommand::FillRect {
            x: cursor_x,
            y: text_y - 2.0,
            width: CURSOR_WIDTH,
            height: FONT_SIZE + 4.0,
            color: COLOR_LAVENDER,
            corner_radii: CornerRadii::ZERO,
        });

        // Autocomplete dropdown.
        if self.dropdown_visible && !self.completions.is_empty() {
            self.render_dropdown(cmds, width, height);
        }
    }

    fn render_dropdown(&self, cmds: &mut Vec<RenderCommand>, width: f32, bar_height: f32) {
        let visible_count = self.completions.len().min(DROPDOWN_MAX_VISIBLE);
        let dropdown_h =
            visible_count as f32 * DROPDOWN_ITEM_HEIGHT + DROPDOWN_PADDING * 2.0;
        let dropdown_y = bar_height + 2.0;
        let dropdown_w = width;

        // Shadow.
        cmds.push(RenderCommand::BoxShadow {
            x: 0.0,
            y: dropdown_y,
            width: dropdown_w,
            height: dropdown_h,
            offset_x: 0.0,
            offset_y: 2.0,
            blur: 8.0,
            spread: 0.0,
            color: COLOR_SHADOW,
            corner_radii: CornerRadii::all(SEGMENT_RADIUS),
        });

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: dropdown_y,
            width: dropdown_w,
            height: dropdown_h,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(SEGMENT_RADIUS),
        });

        // Items.
        let end_idx = (self.dropdown_scroll + visible_count).min(self.completions.len());
        for (vi, idx) in (self.dropdown_scroll..end_idx).enumerate() {
            let item = &self.completions[idx];
            let item_y = dropdown_y + DROPDOWN_PADDING + vi as f32 * DROPDOWN_ITEM_HEIGHT;

            // Highlight selected item.
            if self.completion_index == Some(idx) {
                cmds.push(RenderCommand::FillRect {
                    x: DROPDOWN_PADDING,
                    y: item_y,
                    width: dropdown_w - DROPDOWN_PADDING * 2.0,
                    height: DROPDOWN_ITEM_HEIGHT,
                    color: COLOR_SURFACE2,
                    corner_radii: CornerRadii::all(3.0),
                });
            }

            // Directory indicator.
            let icon_color = if item.is_directory {
                COLOR_BLUE
            } else {
                COLOR_SUBTEXT0
            };
            let icon_text = if item.is_directory { "/" } else { " " };
            cmds.push(RenderCommand::Text {
                x: DROPDOWN_PADDING + 4.0,
                y: item_y + (DROPDOWN_ITEM_HEIGHT - FONT_SIZE) / 2.0,
                text: icon_text.to_string(),
                color: icon_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Item name.
            cmds.push(RenderCommand::Text {
                x: DROPDOWN_PADDING + 16.0,
                y: item_y + (DROPDOWN_ITEM_HEIGHT - FONT_SIZE) / 2.0,
                text: item.name.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dropdown_w - DROPDOWN_PADDING * 2.0 - 20.0),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Path utilities
// ---------------------------------------------------------------------------

/// Normalize a path: collapse double slashes, remove trailing slash (except root).
fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        return "/".to_string();
    }

    let mut result = String::with_capacity(path.len());
    let mut prev_slash = false;

    for ch in path.chars() {
        if ch == '/' {
            if !prev_slash {
                result.push('/');
            }
            prev_slash = true;
        } else {
            result.push(ch);
            prev_slash = false;
        }
    }

    // Remove trailing slash unless it's the root.
    if result.len() > 1 && result.ends_with('/') {
        result.pop();
    }

    if result.is_empty() {
        "/".to_string()
    } else {
        result
    }
}

/// Split a normalized path into display segments.
/// "/" -> ["/"]
/// "/home/user" -> ["/", "home", "user"]
fn split_path(path: &str) -> Vec<String> {
    if path.is_empty() || path == "/" {
        return vec!["/".to_string()];
    }

    let mut segments = Vec::new();

    if path.starts_with('/') {
        segments.push("/".to_string());
        for part in path[1..].split('/') {
            if !part.is_empty() {
                segments.push(part.to_string());
            }
        }
    } else {
        for part in path.split('/') {
            if !part.is_empty() {
                segments.push(part.to_string());
            }
        }
    }

    if segments.is_empty() {
        segments.push("/".to_string());
    }

    segments
}

/// Rebuild a path from segments up to and including `up_to_index`.
fn rebuild_path(segments: &[String], up_to_index: usize) -> String {
    if segments.is_empty() {
        return "/".to_string();
    }

    let end = (up_to_index + 1).min(segments.len());

    if end == 1 && segments[0] == "/" {
        return "/".to_string();
    }

    let mut path = String::new();
    for (i, seg) in segments[..end].iter().enumerate() {
        if i == 0 && seg == "/" {
            path.push('/');
        } else {
            if i > 0 && !path.ends_with('/') {
                path.push('/');
            }
            path.push_str(seg);
        }
    }

    if path.is_empty() {
        "/".to_string()
    } else {
        path
    }
}

/// Determine the prefix to use for autocomplete based on current edit text and cursor.
/// Returns everything from the start of the text up to and including the last '/' before cursor,
/// which represents the directory whose contents should be listed.
fn autocomplete_prefix(text: &str, cursor: usize) -> &str {
    let up_to_cursor = &text[..cursor.min(text.len())];
    // Find the last slash to determine the directory.
    match up_to_cursor.rfind('/') {
        Some(pos) => &text[..=pos],
        None => "",
    }
}

/// Convert a byte offset in a string to a character offset.
fn byte_to_char_offset(s: &str, byte_offset: usize) -> usize {
    s[..byte_offset.min(s.len())].chars().count()
}

/// Convert a character offset to a byte offset.
fn char_to_byte_offset(s: &str, char_offset: usize) -> usize {
    s.char_indices()
        .nth(char_offset)
        .map_or(s.len(), |(byte_idx, _)| byte_idx)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};

    fn key_press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

    fn key_press_with_text(key: Key, ch: char) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some(ch),
        }
    }

    fn key_press_ctrl(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        }
    }

    fn key_press_shift(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::shift(),
            text: None,
        }
    }

    // --- Path splitting tests ---

    #[test]
    fn test_split_path_root() {
        assert_eq!(split_path("/"), vec!["/"]);
    }

    #[test]
    fn test_split_path_simple() {
        assert_eq!(
            split_path("/home/user/Documents"),
            vec!["/", "home", "user", "Documents"]
        );
    }

    #[test]
    fn test_split_path_single_dir() {
        assert_eq!(split_path("/usr"), vec!["/", "usr"]);
    }

    #[test]
    fn test_split_path_empty() {
        assert_eq!(split_path(""), vec!["/"]);
    }

    #[test]
    fn test_split_path_relative() {
        assert_eq!(split_path("home/user"), vec!["home", "user"]);
    }

    // --- Path normalization tests ---

    #[test]
    fn test_normalize_double_slashes() {
        assert_eq!(normalize_path("/home//user///docs"), "/home/user/docs");
    }

    #[test]
    fn test_normalize_trailing_slash() {
        assert_eq!(normalize_path("/home/user/"), "/home/user");
    }

    #[test]
    fn test_normalize_root_trailing() {
        assert_eq!(normalize_path("/"), "/");
    }

    #[test]
    fn test_normalize_empty() {
        assert_eq!(normalize_path(""), "/");
    }

    #[test]
    fn test_normalize_multiple_trailing() {
        assert_eq!(normalize_path("/home/user///"), "/home/user");
    }

    // --- Breadcrumb rendering tests ---

    #[test]
    fn test_render_breadcrumb_segment_count() {
        let mut bar = PathBar::new("/home/user/Documents");
        let cmds = bar.render(800, 32);

        // Count Text commands that are segment names (not separators).
        let text_cmds: Vec<&str> = cmds
            .iter()
            .filter_map(|cmd| {
                if let RenderCommand::Text { text, .. } = cmd {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();

        // Should have 4 segment texts: "/", "home", "user", "Documents"
        // plus 3 separator ">" texts.
        assert!(text_cmds.contains(&"/"));
        assert!(text_cmds.contains(&"home"));
        assert!(text_cmds.contains(&"user"));
        assert!(text_cmds.contains(&"Documents"));
        let separator_count = text_cmds.iter().filter(|&&t| t == ">").count();
        assert_eq!(separator_count, 3);
    }

    #[test]
    fn test_render_breadcrumb_root_only() {
        let mut bar = PathBar::new("/");
        let cmds = bar.render(800, 32);

        let text_cmds: Vec<&str> = cmds
            .iter()
            .filter_map(|cmd| {
                if let RenderCommand::Text { text, .. } = cmd {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();

        assert!(text_cmds.contains(&"/"));
        // No separators for root-only.
        let separator_count = text_cmds.iter().filter(|&&t| t == ">").count();
        assert_eq!(separator_count, 0);
    }

    // --- Edit mode entry/exit tests ---

    #[test]
    fn test_enter_edit_mode_ctrl_l() {
        let mut bar = PathBar::new("/home/user");
        assert!(!bar.is_editing());

        let result = bar.handle_key_event(&key_press_ctrl(Key::L));
        assert_eq!(result, EventResult::Consumed);
        assert!(bar.is_editing());

        let events = bar.drain_events();
        assert!(events.contains(&PathBarEvent::EditModeEntered));
    }

    #[test]
    fn test_enter_edit_mode_typing() {
        let mut bar = PathBar::new("/home");
        let result = bar.handle_key_event(&key_press_with_text(Key::A, 'a'));
        assert_eq!(result, EventResult::Consumed);
        assert!(bar.is_editing());
        assert_eq!(bar.edit_text, "a");
    }

    #[test]
    fn test_exit_edit_mode_escape() {
        let mut bar = PathBar::new("/home/user");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        let result = bar.handle_key_event(&key_press(Key::Escape));
        assert_eq!(result, EventResult::Consumed);
        assert!(!bar.is_editing());

        let events = bar.drain_events();
        assert!(events.contains(&PathBarEvent::EditModeExited));
        // Path should not have changed (reverted).
        assert_eq!(bar.current_path(), "/home/user");
    }

    // --- Text editing tests ---

    #[test]
    fn test_insert_characters() {
        let mut bar = PathBar::new("/");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        bar.handle_key_event(&key_press_with_text(Key::Slash, '/'));
        bar.handle_key_event(&key_press_with_text(Key::H, 'h'));
        bar.handle_key_event(&key_press_with_text(Key::O, 'o'));
        bar.handle_key_event(&key_press_with_text(Key::M, 'm'));
        bar.handle_key_event(&key_press_with_text(Key::E, 'e'));

        assert_eq!(bar.edit_text, "//home");
    }

    #[test]
    fn test_backspace() {
        let mut bar = PathBar::new("/home/user");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        // Cursor is at end of "/home/user".
        bar.handle_key_event(&key_press(Key::Backspace));
        assert_eq!(bar.edit_text, "/home/use");
        bar.handle_key_event(&key_press(Key::Backspace));
        assert_eq!(bar.edit_text, "/home/us");
    }

    #[test]
    fn test_delete() {
        let mut bar = PathBar::new("/home/user");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        // Move cursor to start.
        bar.handle_key_event(&key_press(Key::Home));
        bar.handle_key_event(&key_press(Key::Delete));
        assert_eq!(bar.edit_text, "home/user");
    }

    #[test]
    fn test_cursor_movement() {
        let mut bar = PathBar::new("/ab");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        // Cursor at end (byte 3).
        assert_eq!(bar.cursor, 3);

        bar.handle_key_event(&key_press(Key::Left));
        assert_eq!(bar.cursor, 2);

        bar.handle_key_event(&key_press(Key::Left));
        assert_eq!(bar.cursor, 1);

        bar.handle_key_event(&key_press(Key::Right));
        assert_eq!(bar.cursor, 2);

        bar.handle_key_event(&key_press(Key::Home));
        assert_eq!(bar.cursor, 0);

        bar.handle_key_event(&key_press(Key::End));
        assert_eq!(bar.cursor, 3);
    }

    #[test]
    fn test_select_all() {
        let mut bar = PathBar::new("/home");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        bar.handle_key_event(&key_press_ctrl(Key::A));
        assert_eq!(bar.selection_anchor, Some(0));
        assert_eq!(bar.cursor, 5); // "/home" is 5 bytes.
    }

    // --- Autocomplete tests ---

    #[test]
    fn test_autocomplete_matching() {
        let mut bar = PathBar::new("/home");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        // Simulate typing "/home/" to trigger autocomplete.
        bar.edit_text = "/home/".to_string();
        bar.cursor = 6;
        bar.on_text_changed();

        let events = bar.drain_events();
        assert!(events.iter().any(|e| matches!(
            e,
            PathBarEvent::RequestAutoComplete { prefix } if prefix == "/home/"
        )));
    }

    #[test]
    fn test_autocomplete_selection() {
        let mut bar = PathBar::new("/home");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        bar.edit_text = "/home/".to_string();
        bar.cursor = 6;

        bar.set_completions(vec![
            CompletionItem {
                name: "Documents".to_string(),
                is_directory: true,
            },
            CompletionItem {
                name: "Downloads".to_string(),
                is_directory: true,
            },
            CompletionItem {
                name: ".bashrc".to_string(),
                is_directory: false,
            },
        ]);

        assert!(bar.dropdown_visible);
        assert_eq!(bar.completion_index, Some(0));

        // Move down.
        bar.handle_key_event(&key_press(Key::Down));
        assert_eq!(bar.completion_index, Some(1));

        // Accept with Tab.
        bar.handle_key_event(&key_press(Key::Tab));
        assert_eq!(bar.edit_text, "/home/Downloads/");
        assert!(!bar.dropdown_visible);
    }

    #[test]
    fn test_autocomplete_accept_file() {
        let mut bar = PathBar::new("/home");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        bar.edit_text = "/home/".to_string();
        bar.cursor = 6;

        bar.set_completions(vec![CompletionItem {
            name: "file.txt".to_string(),
            is_directory: false,
        }]);

        bar.handle_key_event(&key_press(Key::Tab));
        // File completions don't append '/'.
        assert_eq!(bar.edit_text, "/home/file.txt");
    }

    #[test]
    fn test_autocomplete_wraps_around() {
        let mut bar = PathBar::new("/");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        bar.edit_text = "/".to_string();
        bar.cursor = 1;
        bar.set_completions(vec![
            CompletionItem {
                name: "a".to_string(),
                is_directory: true,
            },
            CompletionItem {
                name: "b".to_string(),
                is_directory: true,
            },
        ]);

        assert_eq!(bar.completion_index, Some(0));
        bar.handle_key_event(&key_press(Key::Down));
        assert_eq!(bar.completion_index, Some(1));
        bar.handle_key_event(&key_press(Key::Down));
        assert_eq!(bar.completion_index, Some(0)); // wraps
        bar.handle_key_event(&key_press(Key::Up));
        assert_eq!(bar.completion_index, Some(1)); // wraps back
    }

    // --- Navigation tests ---

    #[test]
    fn test_navigate_via_enter() {
        let mut bar = PathBar::new("/home");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        bar.edit_text = "/usr/local/bin".to_string();
        bar.cursor = bar.edit_text.len();

        bar.handle_key_event(&key_press(Key::Enter));
        let events = bar.drain_events();

        assert!(events.contains(&PathBarEvent::Navigate("/usr/local/bin".to_string())));
        assert!(!bar.is_editing());
        assert_eq!(bar.current_path(), "/usr/local/bin");
    }

    #[test]
    fn test_navigate_via_segment_click() {
        let mut bar = PathBar::new("/home/user/Documents");
        // Render to populate segment_rects.
        bar.render(800, 32);

        // We need to find the rect for "home" (index 1).
        // The segment_rects are populated after render.
        assert!(bar.segment_rects.len() >= 2);

        let (sx, sy, sw, sh) = bar.segment_rects[1];
        let click = MouseEvent {
            x: sx + sw / 2.0,
            y: sy + sh / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };

        let result = bar.handle_mouse_event(&click);
        assert_eq!(result, EventResult::Consumed);

        let events = bar.drain_events();
        assert!(events.contains(&PathBarEvent::Navigate("/home".to_string())));
        assert_eq!(bar.current_path(), "/home");
    }

    #[test]
    fn test_navigate_to_root_segment() {
        let mut bar = PathBar::new("/home/user");
        bar.render(800, 32);

        // Click on "/" (index 0).
        let (sx, sy, sw, sh) = bar.segment_rects[0];
        let click = MouseEvent {
            x: sx + sw / 2.0,
            y: sy + sh / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };

        bar.handle_mouse_event(&click);
        let events = bar.drain_events();
        assert!(events.contains(&PathBarEvent::Navigate("/".to_string())));
        assert_eq!(bar.current_path(), "/");
    }

    // --- Overflow tests ---

    #[test]
    fn test_overflow_rendering() {
        let mut bar =
            PathBar::new("/very/long/path/with/many/segments/that/will/overflow");
        // Render at a narrow width to trigger overflow.
        let cmds = bar.render(150, 32);

        let text_cmds: Vec<&str> = cmds
            .iter()
            .filter_map(|cmd| {
                if let RenderCommand::Text { text, .. } = cmd {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();

        // Should have "..." indicating overflow.
        assert!(text_cmds.contains(&"..."));
        // The last segment should still be visible.
        assert!(text_cmds.contains(&"overflow"));
    }

    // --- Rebuild path tests ---

    #[test]
    fn test_rebuild_path_from_segments() {
        let segments = vec![
            "/".to_string(),
            "home".to_string(),
            "user".to_string(),
            "Documents".to_string(),
        ];

        assert_eq!(rebuild_path(&segments, 0), "/");
        assert_eq!(rebuild_path(&segments, 1), "/home");
        assert_eq!(rebuild_path(&segments, 2), "/home/user");
        assert_eq!(rebuild_path(&segments, 3), "/home/user/Documents");
    }

    // --- Autocomplete prefix tests ---

    #[test]
    fn test_autocomplete_prefix_after_slash() {
        assert_eq!(autocomplete_prefix("/home/", 6), "/home/");
    }

    #[test]
    fn test_autocomplete_prefix_partial() {
        assert_eq!(autocomplete_prefix("/home/Do", 8), "/home/");
    }

    #[test]
    fn test_autocomplete_prefix_no_slash() {
        assert_eq!(autocomplete_prefix("something", 9), "");
    }

    #[test]
    fn test_autocomplete_prefix_root() {
        assert_eq!(autocomplete_prefix("/", 1), "/");
    }

    // --- Byte/char offset conversion tests ---

    #[test]
    fn test_byte_to_char_offset_ascii() {
        assert_eq!(byte_to_char_offset("hello", 0), 0);
        assert_eq!(byte_to_char_offset("hello", 3), 3);
        assert_eq!(byte_to_char_offset("hello", 5), 5);
    }

    #[test]
    fn test_char_to_byte_offset_ascii() {
        assert_eq!(char_to_byte_offset("hello", 0), 0);
        assert_eq!(char_to_byte_offset("hello", 3), 3);
        assert_eq!(char_to_byte_offset("hello", 5), 5);
    }

    // --- Set path updates segments ---

    #[test]
    fn test_set_path_updates_segments() {
        let mut bar = PathBar::new("/old/path");
        bar.set_path("/new/path/here");
        assert_eq!(bar.current_path(), "/new/path/here");
        assert_eq!(bar.segments, vec!["/", "new", "path", "here"]);
    }

    // --- Click on empty area enters edit mode ---

    #[test]
    fn test_click_empty_area_enters_edit() {
        let mut bar = PathBar::new("/home");
        bar.render(800, 32);

        // Click far to the right where no segment is.
        let click = MouseEvent {
            x: 700.0,
            y: 16.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        bar.handle_mouse_event(&click);
        assert!(bar.is_editing());
    }
}
