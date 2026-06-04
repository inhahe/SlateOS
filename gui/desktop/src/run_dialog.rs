//! Run Dialog — desktop shell component.
//!
//! A Windows-style "Run" dialog (typically invoked via Ctrl+R or Super+R)
//! that lets users type a command to execute. Supports text editing, command
//! history (with persistence), fuzzy autocomplete, and path resolution.
//!
//! # Usage from the desktop shell
//!
//! ```ignore
//! let mut run_dialog = RunDialog::new();
//!
//! // When Ctrl+R or Super+R is pressed:
//! run_dialog.show();
//!
//! // Forward key/mouse events while visible:
//! run_dialog.handle_key_event(&key_event);
//! run_dialog.handle_mouse_event(&mouse_event);
//!
//! // Each frame, if visible:
//! let commands = run_dialog.render();
//!
//! // Drain events to act on:
//! for event in run_dialog.drain_events() {
//!     match event {
//!         RunDialogEvent::Execute(cmd) => { /* spawn process */ }
//!         RunDialogEvent::Browse => { /* open file picker */ }
//!         RunDialogEvent::Cancel => { /* dismiss */ }
//!         RunDialogEvent::Closed => { /* cleanup */ }
//!     }
//! }
//! ```

use guitk::event::{EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Theme — Catppuccin Mocha palette
// ============================================================================

mod theme {
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
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const SHADOW: Color = Color::rgba(0, 0, 0, 120);
    pub const INPUT_BG: Color = Color::from_hex(0x11111B);
    pub const INPUT_BORDER: Color = Color::from_hex(0x585B70);
    pub const INPUT_BORDER_FOCUS: Color = Color::from_hex(0x89B4FA);
    pub const BUTTON_BG: Color = Color::from_hex(0x45475A);
    pub const BUTTON_HOVER: Color = Color::from_hex(0x585B70);
    pub const BUTTON_PRIMARY: Color = Color::from_hex(0x89B4FA);
    pub const BUTTON_PRIMARY_TEXT: Color = Color::from_hex(0x1E1E2E);
    pub const AUTOCOMPLETE_BG: Color = Color::from_hex(0x181825);
    pub const AUTOCOMPLETE_HOVER: Color = Color::from_hex(0x313244);
}

// ============================================================================
// Constants
// ============================================================================

const DIALOG_WIDTH: f32 = 450.0;
const DIALOG_HEIGHT: f32 = 180.0;
const DIALOG_RADIUS: f32 = 8.0;
const PADDING: f32 = 16.0;
const TITLE_HEIGHT: f32 = 32.0;
const INPUT_HEIGHT: f32 = 28.0;
const INPUT_Y_OFFSET: f32 = 100.0;
const BUTTON_HEIGHT: f32 = 28.0;
const BUTTON_WIDTH: f32 = 75.0;
const BUTTON_SPACING: f32 = 8.0;
const TITLE_FONT_SIZE: f32 = 14.0;
const BODY_FONT_SIZE: f32 = 12.0;
const INPUT_FONT_SIZE: f32 = 13.0;
const AUTOCOMPLETE_ROW_HEIGHT: f32 = 26.0;
const MAX_AUTOCOMPLETE: usize = 8;
const MAX_HISTORY: usize = 50;

// ============================================================================
// Events emitted by the dialog
// ============================================================================

/// Events produced by the Run dialog for the shell to act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunDialogEvent {
    /// User pressed OK or Enter — execute this command.
    Execute(String),
    /// User clicked Browse — open a file picker.
    Browse,
    /// User pressed Cancel or Escape.
    Cancel,
    /// Dialog was dismissed (after Cancel or Execute).
    Closed,
}

// ============================================================================
// Button identifiers
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ButtonId {
    Ok,
    Cancel,
    Browse,
}

// ============================================================================
// Text input state
// ============================================================================

/// Single-line text input state with cursor, selection, and clipboard.
#[derive(Clone, Debug)]
struct TextInput {
    /// The text content.
    text: String,
    /// Cursor position (byte offset, always at a char boundary).
    cursor: usize,
    /// Selection anchor (byte offset). If `Some`, selection spans anchor..cursor.
    selection_anchor: Option<usize>,
    /// Clipboard contents (internal; real clipboard would use IPC).
    clipboard: String,
}

impl TextInput {
    fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            selection_anchor: None,
            clipboard: String::new(),
        }
    }

    fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selection_anchor = None;
    }

    fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.text.len();
        self.selection_anchor = None;
    }

    /// Returns (start, end) byte offsets of the selection, or (cursor, cursor).
    fn selection_range(&self) -> (usize, usize) {
        match self.selection_anchor {
            Some(anchor) => {
                let start = anchor.min(self.cursor);
                let end = anchor.max(self.cursor);
                (start, end)
            }
            None => (self.cursor, self.cursor),
        }
    }

    fn has_selection(&self) -> bool {
        self.selection_anchor.is_some_and(|a| a != self.cursor)
    }

    fn selected_text(&self) -> &str {
        let (start, end) = self.selection_range();
        &self.text[start..end]
    }

    fn delete_selection(&mut self) {
        if !self.has_selection() {
            return;
        }
        let (start, end) = self.selection_range();
        self.text.drain(start..end);
        self.cursor = start;
        self.selection_anchor = None;
    }

    fn select_all(&mut self) {
        self.selection_anchor = Some(0);
        self.cursor = self.text.len();
    }

    fn move_cursor_left(&mut self, shift: bool) {
        if shift && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !shift {
            if self.has_selection() {
                let (start, _) = self.selection_range();
                self.cursor = start;
                self.selection_anchor = None;
                return;
            }
            self.selection_anchor = None;
        }

        if self.cursor > 0 {
            // Move to previous char boundary.
            let mut pos = self.cursor - 1;
            while pos > 0 && !self.text.is_char_boundary(pos) {
                pos -= 1;
            }
            self.cursor = pos;
        }
    }

    fn move_cursor_right(&mut self, shift: bool) {
        if shift && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !shift {
            if self.has_selection() {
                let (_, end) = self.selection_range();
                self.cursor = end;
                self.selection_anchor = None;
                return;
            }
            self.selection_anchor = None;
        }

        if self.cursor < self.text.len() {
            // Move to next char boundary.
            let mut pos = self.cursor + 1;
            while pos < self.text.len() && !self.text.is_char_boundary(pos) {
                pos += 1;
            }
            self.cursor = pos;
        }
    }

    fn move_home(&mut self, shift: bool) {
        if shift && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !shift {
            self.selection_anchor = None;
        }
        self.cursor = 0;
    }

    fn move_end(&mut self, shift: bool) {
        if shift && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !shift {
            self.selection_anchor = None;
        }
        self.cursor = self.text.len();
    }

    fn insert_char(&mut self, ch: char) {
        self.delete_selection();
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    fn backspace(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        if self.cursor > 0 {
            let mut pos = self.cursor - 1;
            while pos > 0 && !self.text.is_char_boundary(pos) {
                pos -= 1;
            }
            self.text.drain(pos..self.cursor);
            self.cursor = pos;
        }
    }

    fn delete(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        if self.cursor < self.text.len() {
            let mut end = self.cursor + 1;
            while end < self.text.len() && !self.text.is_char_boundary(end) {
                end += 1;
            }
            self.text.drain(self.cursor..end);
        }
    }

    fn cut(&mut self) {
        if self.has_selection() {
            self.clipboard = self.selected_text().to_string();
            self.delete_selection();
        }
    }

    fn copy(&mut self) {
        if self.has_selection() {
            self.clipboard = self.selected_text().to_string();
        }
    }

    fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.delete_selection();
        let clip = self.clipboard.clone();
        self.text.insert_str(self.cursor, &clip);
        self.cursor += clip.len();
    }
}

// ============================================================================
// Autocomplete
// ============================================================================

/// An autocomplete suggestion.
#[derive(Clone, Debug)]
struct Suggestion {
    /// Display text.
    text: String,
    /// Score for sorting (higher is better).
    score: u32,
}

// ============================================================================
// RunDialog
// ============================================================================

/// The Run dialog state and logic.
pub struct RunDialog {
    /// Whether the dialog is currently visible.
    visible: bool,
    /// Text input state.
    input: TextInput,
    /// Command history (most recent last).
    history: Vec<String>,
    /// Current position in history when cycling (-1 = not browsing history).
    history_index: Option<usize>,
    /// Text saved before entering history browse mode.
    pre_history_text: String,
    /// Known application names for autocomplete.
    known_apps: Vec<String>,
    /// Known PATH directories for resolution.
    path_dirs: Vec<String>,
    /// Autocomplete suggestions currently shown.
    suggestions: Vec<Suggestion>,
    /// Selected suggestion index.
    suggestion_index: Option<usize>,
    /// Whether to show autocomplete dropdown.
    show_autocomplete: bool,
    /// Error message to display (e.g., "not found").
    error_message: Option<String>,
    /// Pending events to drain.
    events: Vec<RunDialogEvent>,
    /// Path to persist history (if set).
    history_path: Option<String>,
    /// Which button is hovered.
    hovered_button: Option<ButtonId>,
    /// Dialog X position (centered on screen, set by caller or default).
    dialog_x: f32,
    /// Dialog Y position.
    dialog_y: f32,
}

impl RunDialog {
    /// Create a new Run dialog (initially hidden).
    pub fn new() -> Self {
        Self {
            visible: false,
            input: TextInput::new(),
            history: Vec::new(),
            history_index: None,
            pre_history_text: String::new(),
            known_apps: default_known_apps(),
            path_dirs: default_path_dirs(),
            suggestions: Vec::new(),
            suggestion_index: None,
            show_autocomplete: false,
            error_message: None,
            events: Vec::new(),
            history_path: None,
            hovered_button: None,
            // Default to centered-ish position; caller should reposition.
            dialog_x: 200.0,
            dialog_y: 150.0,
        }
    }

    /// Create a Run dialog with custom known apps and PATH dirs.
    pub fn with_config(
        known_apps: Vec<String>,
        path_dirs: Vec<String>,
        history_path: Option<String>,
    ) -> Self {
        let mut dialog = Self::new();
        dialog.known_apps = known_apps;
        dialog.path_dirs = path_dirs;
        dialog.history_path = history_path;
        dialog
    }

    /// Set the dialog position (e.g., center on screen).
    pub fn set_position(&mut self, x: f32, y: f32) {
        self.dialog_x = x;
        self.dialog_y = y;
    }

    /// Whether the dialog is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Show the dialog, resetting input state.
    pub fn show(&mut self) {
        self.visible = true;
        self.input.clear();
        self.history_index = None;
        self.pre_history_text.clear();
        self.suggestions.clear();
        self.suggestion_index = None;
        self.show_autocomplete = false;
        self.error_message = None;
        self.hovered_button = None;
    }

    /// Hide the dialog.
    pub fn hide(&mut self) {
        self.visible = false;
        self.events.push(RunDialogEvent::Closed);
    }

    /// Drain pending events.
    pub fn drain_events(&mut self) -> Vec<RunDialogEvent> {
        core::mem::take(&mut self.events)
    }

    /// Add a command to history (called after successful execution).
    pub fn add_to_history(&mut self, command: &str) {
        // Remove duplicate if present.
        self.history.retain(|h| h != command);
        self.history.push(command.to_string());
        // Cap at MAX_HISTORY.
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
    }

    /// Load history from a list of strings (e.g., read from file).
    pub fn load_history(&mut self, commands: Vec<String>) {
        self.history = commands;
        if self.history.len() > MAX_HISTORY {
            let excess = self.history.len() - MAX_HISTORY;
            self.history.drain(0..excess);
        }
    }

    /// Get current history for persistence.
    pub fn history(&self) -> &[String] {
        &self.history
    }

    // ========================================================================
    // Input handling
    // ========================================================================

    /// Handle a key event. Returns `EventResult::Consumed` if the dialog handled it.
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> EventResult {
        if !self.visible || !event.pressed {
            return EventResult::Ignored;
        }

        let ctrl = event.modifiers.ctrl;
        let shift = event.modifiers.shift;

        match event.key {
            // Escape → cancel
            Key::Escape => {
                self.events.push(RunDialogEvent::Cancel);
                self.hide();
            }

            // Enter → execute
            Key::Enter => {
                self.execute_current();
            }

            // Tab → accept autocomplete suggestion
            Key::Tab => {
                self.accept_suggestion();
            }

            // Ctrl+A → select all
            Key::A if ctrl => {
                self.input.select_all();
            }

            // Ctrl+X → cut
            Key::X if ctrl => {
                self.input.cut();
                self.update_suggestions();
            }

            // Ctrl+C → copy
            Key::C if ctrl => {
                self.input.copy();
            }

            // Ctrl+V → paste
            Key::V if ctrl => {
                self.input.paste();
                self.update_suggestions();
            }

            // Arrow keys — history cycling
            Key::Up => {
                if self.show_autocomplete && self.suggestion_index.is_some() {
                    // Navigate autocomplete up
                    if let Some(idx) = self.suggestion_index
                        && idx > 0 {
                            self.suggestion_index = Some(idx - 1);
                        }
                } else {
                    self.history_prev();
                }
            }

            Key::Down => {
                if self.show_autocomplete && self.suggestion_index.is_some() {
                    // Navigate autocomplete down
                    if let Some(idx) = self.suggestion_index
                        && idx + 1 < self.suggestions.len() {
                            self.suggestion_index = Some(idx + 1);
                        }
                } else {
                    self.history_next();
                }
            }

            // Cursor movement
            Key::Left => {
                self.input.move_cursor_left(shift);
            }

            Key::Right => {
                self.input.move_cursor_right(shift);
            }

            Key::Home => {
                self.input.move_home(shift);
            }

            Key::End => {
                self.input.move_end(shift);
            }

            // Editing
            Key::Backspace => {
                self.input.backspace();
                self.update_suggestions();
            }

            Key::Delete => {
                self.input.delete();
                self.update_suggestions();
            }

            // Text input (character typed)
            _ => {
                if let Some(ch) = event.text {
                    if !ch.is_control() {
                        self.input.insert_char(ch);
                        self.update_suggestions();
                        self.error_message = None;
                    }
                } else {
                    return EventResult::Ignored;
                }
            }
        }

        EventResult::Consumed
    }

    /// Handle a mouse event. Returns `EventResult::Consumed` if the dialog handled it.
    pub fn handle_mouse_event(&mut self, event: &MouseEvent) -> EventResult {
        if !self.visible {
            return EventResult::Ignored;
        }

        // Transform mouse coordinates to dialog-local space.
        let local_x = event.x - self.dialog_x;
        let local_y = event.y - self.dialog_y;

        // Check if click is outside dialog bounds — dismiss.
        if local_x < 0.0
            || local_y < 0.0
            || local_x > DIALOG_WIDTH
            || local_y > DIALOG_HEIGHT
        {
            if matches!(event.kind, MouseEventKind::Press(MouseButton::Left)) {
                self.events.push(RunDialogEvent::Cancel);
                self.hide();
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }

        // Button hit detection.
        let button_y = DIALOG_HEIGHT - PADDING - BUTTON_HEIGHT;
        let ok_x = DIALOG_WIDTH - PADDING - BUTTON_WIDTH;
        let cancel_x = ok_x - BUTTON_SPACING - BUTTON_WIDTH;
        let browse_x = cancel_x - BUTTON_SPACING - BUTTON_WIDTH;

        let hit_button = if local_y >= button_y && local_y <= button_y + BUTTON_HEIGHT {
            if local_x >= ok_x && local_x <= ok_x + BUTTON_WIDTH {
                Some(ButtonId::Ok)
            } else if local_x >= cancel_x && local_x <= cancel_x + BUTTON_WIDTH {
                Some(ButtonId::Cancel)
            } else if local_x >= browse_x && local_x <= browse_x + BUTTON_WIDTH {
                Some(ButtonId::Browse)
            } else {
                None
            }
        } else {
            None
        };

        match &event.kind {
            MouseEventKind::Move => {
                self.hovered_button = hit_button;
            }
            MouseEventKind::Press(MouseButton::Left) => {
                match hit_button {
                    Some(ButtonId::Ok) => self.execute_current(),
                    Some(ButtonId::Cancel) => {
                        self.events.push(RunDialogEvent::Cancel);
                        self.hide();
                    }
                    Some(ButtonId::Browse) => {
                        self.events.push(RunDialogEvent::Browse);
                    }
                    None => {
                        // Check autocomplete dropdown clicks.
                        if self.show_autocomplete {
                            let dropdown_y = INPUT_Y_OFFSET + INPUT_HEIGHT + 2.0;
                            let rel_y = local_y - dropdown_y;
                            if rel_y >= 0.0 && local_x >= PADDING + 40.0 {
                                let idx = (rel_y / AUTOCOMPLETE_ROW_HEIGHT) as usize;
                                if idx < self.suggestions.len() {
                                    self.suggestion_index = Some(idx);
                                    self.accept_suggestion();
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        EventResult::Consumed
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the dialog to a list of render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        let mut cmds = Vec::with_capacity(32);
        let x = self.dialog_x;
        let y = self.dialog_y;

        // Box shadow for elevation.
        cmds.push(RenderCommand::BoxShadow {
            x,
            y,
            width: DIALOG_WIDTH,
            height: DIALOG_HEIGHT,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 2.0,
            color: theme::SHADOW,
            corner_radii: CornerRadii::all(DIALOG_RADIUS),
        });

        // Dialog background.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: DIALOG_WIDTH,
            height: DIALOG_HEIGHT,
            color: theme::BASE,
            corner_radii: CornerRadii::all(DIALOG_RADIUS),
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x,
            y,
            width: DIALOG_WIDTH,
            height: DIALOG_HEIGHT,
            color: theme::SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(DIALOG_RADIUS),
        });

        // Title bar area.
        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: y + PADDING,
            text: "Run".to_string(),
            color: theme::TEXT,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Instruction text.
        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: y + TITLE_HEIGHT + PADDING + 4.0,
            text: "Type the name of a program, folder, or document, and the \
                   OS will open it for you."
                .to_string(),
            color: theme::SUBTEXT0,
            font_size: BODY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(DIALOG_WIDTH - PADDING * 2.0),
        });

        // "Open:" label.
        cmds.push(RenderCommand::Text {
            x: x + PADDING,
            y: y + INPUT_Y_OFFSET + 6.0,
            text: "Open:".to_string(),
            color: theme::TEXT,
            font_size: BODY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Input field background.
        let input_x = x + PADDING + 40.0;
        let input_w = DIALOG_WIDTH - PADDING * 2.0 - 40.0;

        cmds.push(RenderCommand::FillRect {
            x: input_x,
            y: y + INPUT_Y_OFFSET,
            width: input_w,
            height: INPUT_HEIGHT,
            color: theme::INPUT_BG,
            corner_radii: CornerRadii::all(4.0),
        });

        // Input field border.
        cmds.push(RenderCommand::StrokeRect {
            x: input_x,
            y: y + INPUT_Y_OFFSET,
            width: input_w,
            height: INPUT_HEIGHT,
            color: theme::INPUT_BORDER_FOCUS,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Selection highlight (if any).
        if self.input.has_selection() {
            let (start, end) = self.input.selection_range();
            let text_before_start = &self.input.text[..start];
            let text_selection = &self.input.text[start..end];
            let start_px = estimate_text_width(text_before_start, INPUT_FONT_SIZE);
            let sel_width = estimate_text_width(text_selection, INPUT_FONT_SIZE);
            cmds.push(RenderCommand::FillRect {
                x: input_x + 4.0 + start_px,
                y: y + INPUT_Y_OFFSET + 3.0,
                width: sel_width,
                height: INPUT_HEIGHT - 6.0,
                color: theme::BLUE,
                corner_radii: CornerRadii::all(2.0),
            });
        }

        // Input text.
        cmds.push(RenderCommand::Text {
            x: input_x + 4.0,
            y: y + INPUT_Y_OFFSET + 7.0,
            text: self.input.text.clone(),
            color: theme::TEXT,
            font_size: INPUT_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(input_w - 8.0),
        });

        // Cursor.
        let cursor_text = &self.input.text[..self.input.cursor];
        let cursor_px = estimate_text_width(cursor_text, INPUT_FONT_SIZE);
        cmds.push(RenderCommand::Line {
            x1: input_x + 4.0 + cursor_px,
            y1: y + INPUT_Y_OFFSET + 4.0,
            x2: input_x + 4.0 + cursor_px,
            y2: y + INPUT_Y_OFFSET + INPUT_HEIGHT - 4.0,
            color: theme::TEXT,
            width: 1.0,
        });

        // Error message.
        if let Some(ref err) = self.error_message {
            cmds.push(RenderCommand::Text {
                x: input_x,
                y: y + INPUT_Y_OFFSET + INPUT_HEIGHT + 2.0,
                text: err.clone(),
                color: theme::RED,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(input_w),
            });
        }

        // Autocomplete dropdown.
        if self.show_autocomplete && !self.suggestions.is_empty() {
            let dropdown_x = input_x;
            let dropdown_y = y + INPUT_Y_OFFSET + INPUT_HEIGHT + 2.0;
            let dropdown_h = self.suggestions.len() as f32 * AUTOCOMPLETE_ROW_HEIGHT;

            cmds.push(RenderCommand::FillRect {
                x: dropdown_x,
                y: dropdown_y,
                width: input_w,
                height: dropdown_h,
                color: theme::AUTOCOMPLETE_BG,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::StrokeRect {
                x: dropdown_x,
                y: dropdown_y,
                width: input_w,
                height: dropdown_h,
                color: theme::SURFACE1,
                line_width: 1.0,
                corner_radii: CornerRadii::all(4.0),
            });

            for (i, suggestion) in self.suggestions.iter().enumerate() {
                let row_y = dropdown_y + i as f32 * AUTOCOMPLETE_ROW_HEIGHT;
                let is_selected = self.suggestion_index == Some(i);

                if is_selected {
                    cmds.push(RenderCommand::FillRect {
                        x: dropdown_x + 1.0,
                        y: row_y,
                        width: input_w - 2.0,
                        height: AUTOCOMPLETE_ROW_HEIGHT,
                        color: theme::AUTOCOMPLETE_HOVER,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                cmds.push(RenderCommand::Text {
                    x: dropdown_x + 8.0,
                    y: row_y + 6.0,
                    text: suggestion.text.clone(),
                    color: if is_selected { theme::BLUE } else { theme::TEXT },
                    font_size: INPUT_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(input_w - 16.0),
                });
            }
        }

        // Buttons row.
        let button_y = y + DIALOG_HEIGHT - PADDING - BUTTON_HEIGHT;
        self.render_button(
            &mut cmds,
            "OK",
            x + DIALOG_WIDTH - PADDING - BUTTON_WIDTH,
            button_y,
            ButtonId::Ok,
            true,
        );
        self.render_button(
            &mut cmds,
            "Cancel",
            x + DIALOG_WIDTH - PADDING - BUTTON_WIDTH * 2.0 - BUTTON_SPACING,
            button_y,
            ButtonId::Cancel,
            false,
        );
        self.render_button(
            &mut cmds,
            "Browse...",
            x + DIALOG_WIDTH
                - PADDING
                - BUTTON_WIDTH * 3.0
                - BUTTON_SPACING * 2.0,
            button_y,
            ButtonId::Browse,
            false,
        );

        cmds
    }

    // ========================================================================
    // Private methods
    // ========================================================================

    fn render_button(
        &self,
        cmds: &mut Vec<RenderCommand>,
        label: &str,
        bx: f32,
        by: f32,
        id: ButtonId,
        primary: bool,
    ) {
        let hovered = self.hovered_button == Some(id);
        let bg = if primary {
            theme::BUTTON_PRIMARY
        } else if hovered {
            theme::BUTTON_HOVER
        } else {
            theme::BUTTON_BG
        };
        let fg = if primary {
            theme::BUTTON_PRIMARY_TEXT
        } else {
            theme::TEXT
        };

        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: by,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });

        cmds.push(RenderCommand::Text {
            x: bx + BUTTON_WIDTH / 2.0 - estimate_text_width(label, BODY_FONT_SIZE) / 2.0,
            y: by + 7.0,
            text: label.to_string(),
            color: fg,
            font_size: BODY_FONT_SIZE,
            font_weight: if primary {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: None,
        });
    }

    fn execute_current(&mut self) {
        let command = self.input.text.trim().to_string();
        if command.is_empty() {
            return;
        }

        // Resolve the command.
        if self.resolve_command(&command) {
            self.add_to_history(&command);
            self.events.push(RunDialogEvent::Execute(command));
            self.hide();
        } else {
            self.error_message = Some(format!(
                "\"{}\" is not recognized as an application or command.",
                command
            ));
        }
    }

    /// Resolve a command: check if it is an absolute path, a known app, or on PATH.
    fn resolve_command(&self, command: &str) -> bool {
        // Absolute paths pass through directly.
        if command.starts_with('/') {
            return true;
        }

        // Extract the program name (first word).
        let program = command.split_whitespace().next().unwrap_or(command);

        // Check known apps (case-insensitive).
        let program_lower = program.to_ascii_lowercase();
        for app in &self.known_apps {
            if app.to_ascii_lowercase() == program_lower {
                return true;
            }
        }

        // Check PATH directories (simulate: just check if program name is non-empty
        // and doesn't contain invalid chars — real resolution would stat files).
        if !program.is_empty() && !program.contains('\0') {
            for _dir in &self.path_dirs {
                // In a real implementation, we would check if dir/program exists.
                // For now, accept anything that looks like a valid command name.
                if program.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
                    return true;
                }
            }
        }

        false
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                // Enter history browse mode.
                self.pre_history_text = self.input.text.clone();
                let idx = self.history.len() - 1;
                self.history_index = Some(idx);
                self.input.set_text(&self.history[idx]);
            }
            Some(idx) => {
                if idx > 0 {
                    let new_idx = idx - 1;
                    self.history_index = Some(new_idx);
                    self.input.set_text(&self.history[new_idx]);
                }
            }
        }
        self.update_suggestions();
    }

    fn history_next(&mut self) {
        match self.history_index {
            None => {
                // Not in history mode — nothing to do.
            }
            Some(idx) => {
                if idx + 1 < self.history.len() {
                    let new_idx = idx + 1;
                    self.history_index = Some(new_idx);
                    self.input.set_text(&self.history[new_idx]);
                } else {
                    // Past the end → restore original text.
                    self.history_index = None;
                    let saved = self.pre_history_text.clone();
                    self.input.set_text(&saved);
                }
            }
        }
        self.update_suggestions();
    }

    fn accept_suggestion(&mut self) {
        if !self.show_autocomplete || self.suggestions.is_empty() {
            return;
        }
        let idx = self.suggestion_index.unwrap_or(0);
        if let Some(suggestion) = self.suggestions.get(idx) {
            self.input.set_text(&suggestion.text);
            self.show_autocomplete = false;
            self.suggestions.clear();
            self.suggestion_index = None;
        }
    }

    fn update_suggestions(&mut self) {
        let query = self.input.text.trim();
        if query.is_empty() {
            self.suggestions.clear();
            self.show_autocomplete = false;
            self.suggestion_index = None;
            return;
        }

        let mut results: Vec<Suggestion> = Vec::new();

        // Match against known apps.
        for app in &self.known_apps {
            if let Some(score) = fuzzy_score(query, app) {
                results.push(Suggestion {
                    text: app.clone(),
                    score,
                });
            }
        }

        // Match against history.
        for cmd in &self.history {
            if let Some(score) = fuzzy_score(query, cmd) {
                // Avoid duplicates.
                if !results.iter().any(|s| s.text == *cmd) {
                    results.push(Suggestion {
                        text: cmd.clone(),
                        score: score.saturating_add(5), // slight history bonus
                    });
                }
            }
        }

        // Sort by score descending.
        results.sort_by(|a, b| b.score.cmp(&a.score));
        results.truncate(MAX_AUTOCOMPLETE);

        self.show_autocomplete = !results.is_empty();
        self.suggestions = results;
        // Reset selection to first item if we have suggestions.
        self.suggestion_index = if self.show_autocomplete {
            Some(0)
        } else {
            None
        };
    }
}

// ============================================================================
// Fuzzy matching (same algorithm style as the launcher)
// ============================================================================

/// Score how well `query` fuzzy-matches `target`.
///
/// Returns `None` if the query does not match. Higher scores are better.
/// Uses the same algorithm as the application launcher for consistency.
fn fuzzy_score(query: &str, target: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }

    let query_lower: Vec<char> = query.chars().map(|c| c.to_ascii_lowercase()).collect();
    let target_lower: Vec<char> = target.chars().map(|c| c.to_ascii_lowercase()).collect();

    if query_lower.len() > target_lower.len() {
        return None;
    }

    // Check prefix match for bonus.
    let is_prefix = target_lower
        .iter()
        .zip(query_lower.iter())
        .all(|(t, q)| t == q);

    let mut score: u32 = 0;
    let mut qi = 0;
    let mut prev_match_idx: Option<usize> = None;
    let mut first_match_idx: Option<usize> = None;

    for (ti, &tc) in target_lower.iter().enumerate() {
        if qi >= query_lower.len() {
            break;
        }
        if tc == query_lower[qi] {
            if first_match_idx.is_none() {
                first_match_idx = Some(ti);
            }

            // Bonus for matching at word boundaries.
            let at_boundary = ti == 0
                || target_lower
                    .get(ti.saturating_sub(1))
                    .is_some_and(|&prev| prev == ' ' || prev == '-' || prev == '_');
            if at_boundary {
                score = score.saturating_add(10);
            }

            // Bonus for consecutive matches.
            if let Some(prev) = prev_match_idx
                && ti == prev + 1 {
                    score = score.saturating_add(5);
                }

            prev_match_idx = Some(ti);
            qi += 1;
        }
    }

    // All query chars must match.
    if qi < query_lower.len() {
        return None;
    }

    // Prefix bonus.
    if is_prefix {
        score = score.saturating_add(50);
    }

    // Early match bonus.
    if let Some(idx) = first_match_idx {
        let early_bonus = 20u32.saturating_sub(idx as u32);
        score = score.saturating_add(early_bonus);
    }

    // Shorter targets score higher (more specific match).
    let length_diff = target_lower.len().saturating_sub(query_lower.len());
    let length_bonus = 10u32.saturating_sub(length_diff.min(10) as u32);
    score = score.saturating_add(length_bonus);

    Some(score)
}

// ============================================================================
// Text width estimation
// ============================================================================

/// Rough text width estimation for cursor/selection positioning.
/// In a real system this would query the font rasterizer metrics.
fn estimate_text_width(text: &str, font_size: f32) -> f32 {
    // Approximate: average char width ~0.55 of font size for a monospace-ish font.
    text.len() as f32 * font_size * 0.55
}

// ============================================================================
// Default data
// ============================================================================

fn default_known_apps() -> Vec<String> {
    vec![
        "terminal".to_string(),
        "file-explorer".to_string(),
        "text-editor".to_string(),
        "settings".to_string(),
        "process-explorer".to_string(),
        "calculator".to_string(),
        "browser".to_string(),
        "image-viewer".to_string(),
        "music-player".to_string(),
        "video-player".to_string(),
        "package-manager".to_string(),
        "system-monitor".to_string(),
        "disk-utility".to_string(),
        "network-settings".to_string(),
        "display-settings".to_string(),
    ]
}

fn default_path_dirs() -> Vec<String> {
    vec![
        "/usr/bin".to_string(),
        "/usr/local/bin".to_string(),
        "/bin".to_string(),
        "/sbin".to_string(),
    ]
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(key: Key, ctrl: bool, shift: bool, text: Option<char>) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: guitk::event::Modifiers {
                shift,
                ctrl,
                alt: false,
                super_key: false,
            },
            text,
        }
    }

    // ====================================================================
    // Text input tests
    // ====================================================================

    #[test]
    fn test_text_input_insert() {
        let mut input = TextInput::new();
        input.insert_char('h');
        input.insert_char('e');
        input.insert_char('l');
        input.insert_char('l');
        input.insert_char('o');
        assert_eq!(input.text, "hello");
        assert_eq!(input.cursor, 5);
    }

    #[test]
    fn test_text_input_backspace() {
        let mut input = TextInput::new();
        input.set_text("hello");
        input.backspace();
        assert_eq!(input.text, "hell");
        assert_eq!(input.cursor, 4);
    }

    #[test]
    fn test_text_input_delete() {
        let mut input = TextInput::new();
        input.set_text("hello");
        input.cursor = 0;
        input.delete();
        assert_eq!(input.text, "ello");
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn test_text_input_cursor_movement() {
        let mut input = TextInput::new();
        input.set_text("hello");
        assert_eq!(input.cursor, 5);
        input.move_cursor_left(false);
        assert_eq!(input.cursor, 4);
        input.move_cursor_left(false);
        assert_eq!(input.cursor, 3);
        input.move_cursor_right(false);
        assert_eq!(input.cursor, 4);
        input.move_home(false);
        assert_eq!(input.cursor, 0);
        input.move_end(false);
        assert_eq!(input.cursor, 5);
    }

    #[test]
    fn test_text_input_selection() {
        let mut input = TextInput::new();
        input.set_text("hello world");
        input.move_home(false);
        // Select "hello" with shift+right x5
        for _ in 0..5 {
            input.move_cursor_right(true);
        }
        assert!(input.has_selection());
        assert_eq!(input.selected_text(), "hello");
        assert_eq!(input.selection_range(), (0, 5));
    }

    #[test]
    fn test_text_input_select_all() {
        let mut input = TextInput::new();
        input.set_text("hello world");
        input.select_all();
        assert_eq!(input.selected_text(), "hello world");
    }

    #[test]
    fn test_text_input_cut_paste() {
        let mut input = TextInput::new();
        input.set_text("hello world");
        input.select_all();
        input.cut();
        assert_eq!(input.text, "");
        assert_eq!(input.clipboard, "hello world");
        input.paste();
        assert_eq!(input.text, "hello world");
    }

    #[test]
    fn test_text_input_delete_selection() {
        let mut input = TextInput::new();
        input.set_text("hello world");
        input.selection_anchor = Some(0);
        input.cursor = 5;
        input.delete_selection();
        assert_eq!(input.text, " world");
        assert_eq!(input.cursor, 0);
    }

    // ====================================================================
    // History cycling tests
    // ====================================================================

    #[test]
    fn test_history_cycling() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.add_to_history("ls");
        dialog.add_to_history("pwd");
        dialog.add_to_history("cat file.txt");

        // Navigate up through history.
        dialog.history_prev();
        assert_eq!(dialog.input.text, "cat file.txt");
        dialog.history_prev();
        assert_eq!(dialog.input.text, "pwd");
        dialog.history_prev();
        assert_eq!(dialog.input.text, "ls");

        // Navigate back down.
        dialog.history_next();
        assert_eq!(dialog.input.text, "pwd");
        dialog.history_next();
        assert_eq!(dialog.input.text, "cat file.txt");

        // Past the end returns to original.
        dialog.history_next();
        assert_eq!(dialog.input.text, "");
    }

    #[test]
    fn test_history_preserves_current_text() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.add_to_history("old-command");

        // Type something.
        dialog.input.set_text("partial");

        // Go up into history.
        dialog.history_prev();
        assert_eq!(dialog.input.text, "old-command");

        // Come back down — original text restored.
        dialog.history_next();
        assert_eq!(dialog.input.text, "partial");
    }

    #[test]
    fn test_history_max_entries() {
        let mut dialog = RunDialog::new();
        for i in 0..60 {
            dialog.add_to_history(&format!("cmd{}", i));
        }
        assert_eq!(dialog.history.len(), MAX_HISTORY);
        // Oldest entries removed.
        assert_eq!(dialog.history[0], "cmd10");
    }

    #[test]
    fn test_history_dedup() {
        let mut dialog = RunDialog::new();
        dialog.add_to_history("ls");
        dialog.add_to_history("pwd");
        dialog.add_to_history("ls"); // duplicate
        assert_eq!(dialog.history.len(), 2);
        // "ls" should be at the end (most recent).
        assert_eq!(dialog.history[0], "pwd");
        assert_eq!(dialog.history[1], "ls");
    }

    // ====================================================================
    // Autocomplete / fuzzy matching tests
    // ====================================================================

    #[test]
    fn test_fuzzy_score_exact() {
        let score = fuzzy_score("terminal", "terminal");
        assert!(score.is_some());
        assert!(score.unwrap() > 50); // High score for exact match.
    }

    #[test]
    fn test_fuzzy_score_prefix() {
        let score = fuzzy_score("term", "terminal");
        assert!(score.is_some());
        assert!(score.unwrap() > 30); // Prefix matches score well.
    }

    #[test]
    fn test_fuzzy_score_no_match() {
        let score = fuzzy_score("xyz", "terminal");
        assert!(score.is_none());
    }

    #[test]
    fn test_fuzzy_score_boundary() {
        // "fe" should match "file-explorer" at word boundaries.
        let score = fuzzy_score("fe", "file-explorer");
        assert!(score.is_some());
    }

    #[test]
    fn test_fuzzy_score_case_insensitive() {
        let score = fuzzy_score("TERM", "terminal");
        assert!(score.is_some());
    }

    #[test]
    fn test_update_suggestions() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("term");
        dialog.update_suggestions();
        assert!(!dialog.suggestions.is_empty());
        // "terminal" should be in the suggestions.
        assert!(dialog.suggestions.iter().any(|s| s.text == "terminal"));
    }

    #[test]
    fn test_accept_suggestion() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("term");
        dialog.update_suggestions();
        assert!(dialog.show_autocomplete);

        dialog.accept_suggestion();
        assert_eq!(dialog.input.text, "terminal");
        assert!(!dialog.show_autocomplete);
    }

    // ====================================================================
    // Event generation tests
    // ====================================================================

    #[test]
    fn test_enter_executes() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("terminal");

        let event = make_key(Key::Enter, false, false, None);
        dialog.handle_key_event(&event);

        let events = dialog.drain_events();
        assert!(events.contains(&RunDialogEvent::Execute("terminal".to_string())));
        assert!(events.contains(&RunDialogEvent::Closed));
    }

    #[test]
    fn test_escape_cancels() {
        let mut dialog = RunDialog::new();
        dialog.show();

        let event = make_key(Key::Escape, false, false, None);
        dialog.handle_key_event(&event);

        let events = dialog.drain_events();
        assert!(events.contains(&RunDialogEvent::Cancel));
        assert!(events.contains(&RunDialogEvent::Closed));
    }

    #[test]
    fn test_empty_enter_does_nothing() {
        let mut dialog = RunDialog::new();
        dialog.show();

        let event = make_key(Key::Enter, false, false, None);
        dialog.handle_key_event(&event);

        let events = dialog.drain_events();
        assert!(events.is_empty());
        assert!(dialog.is_visible()); // Still visible.
    }

    #[test]
    fn test_not_found_error() {
        let mut dialog = RunDialog::new();
        dialog.known_apps.clear();
        dialog.path_dirs.clear();
        dialog.show();
        dialog.input.set_text("nonexistent!@#");

        let event = make_key(Key::Enter, false, false, None);
        dialog.handle_key_event(&event);

        // Should show error, not execute.
        assert!(dialog.error_message.is_some());
        assert!(dialog.is_visible());
        let events = dialog.drain_events();
        assert!(events.is_empty());
    }

    #[test]
    fn test_absolute_path_resolves() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("/usr/bin/something");

        let event = make_key(Key::Enter, false, false, None);
        dialog.handle_key_event(&event);

        let events = dialog.drain_events();
        assert!(events.contains(&RunDialogEvent::Execute("/usr/bin/something".to_string())));
    }

    #[test]
    fn test_show_hide_visibility() {
        let mut dialog = RunDialog::new();
        assert!(!dialog.is_visible());
        dialog.show();
        assert!(dialog.is_visible());
        dialog.hide();
        assert!(!dialog.is_visible());
    }

    #[test]
    fn test_render_empty_when_hidden() {
        let dialog = RunDialog::new();
        let cmds = dialog.render();
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_render_nonempty_when_visible() {
        let mut dialog = RunDialog::new();
        dialog.show();
        let cmds = dialog.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_key_event_ignored_when_hidden() {
        let mut dialog = RunDialog::new();
        let event = make_key(Key::A, false, false, Some('a'));
        let result = dialog.handle_key_event(&event);
        assert_eq!(result, EventResult::Ignored);
    }

    #[test]
    fn test_key_event_consumed_when_visible() {
        let mut dialog = RunDialog::new();
        dialog.show();
        let event = make_key(Key::A, false, false, Some('a'));
        let result = dialog.handle_key_event(&event);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(dialog.input.text, "a");
    }

    #[test]
    fn test_ctrl_a_selects_all() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("hello world");
        let event = make_key(Key::A, true, false, None);
        dialog.handle_key_event(&event);
        assert!(dialog.input.has_selection());
        assert_eq!(dialog.input.selected_text(), "hello world");
    }

    #[test]
    fn test_tab_accepts_autocomplete() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.input.set_text("calc");
        dialog.update_suggestions();
        assert!(dialog.show_autocomplete);

        let event = make_key(Key::Tab, false, false, None);
        dialog.handle_key_event(&event);
        assert_eq!(dialog.input.text, "calculator");
    }
}
