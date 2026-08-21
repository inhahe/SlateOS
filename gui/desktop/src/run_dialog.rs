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
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::text::TextCursor;
// The candidate ranking is shared with both launchers. It used to be a third
// copy of the same routine here, under a comment saying it "uses the same
// algorithm as the application launcher for consistency" — a promise with no
// mechanism behind it.
use guitk::textfind::fuzzy_score;

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
            Some(anchor) => (anchor.min(self.cursor), anchor.max(self.cursor)),
            None => (self.cursor, self.cursor),
        }
    }

    fn has_selection(&self) -> bool {
        self.selection_anchor.is_some_and(|a| a != self.cursor)
    }

    fn selected_text(&self) -> &str {
        let (start, end) = self.selection_range();
        self.text.get(start..end).unwrap_or("")
    }

    /// The largest character boundary at or before `at`, and never past the
    /// end of the text.
    ///
    /// Every offset this type holds is supposed to be on a boundary already.
    /// This is what makes that a *property* rather than an assumption: the
    /// primitives below all pass their offsets through here, so a stale or
    /// mid-character offset shortens an edit instead of panicking inside
    /// `String::replace_range`.
    /// The answer lives in the toolkit: one implementation of "where is the
    /// nearest caret stop" for every text field in the system, rather than one
    /// per field, each free to drift from the others.
    fn floor_boundary(&self, at: usize) -> usize {
        TextCursor::from(at).snapped_in(&self.text).byte()
    }

    /// The byte offset of the character before `at`, or `at` at the start.
    fn prev_boundary(&self, at: usize) -> usize {
        let at = TextCursor::from(at).snapped_in(&self.text);
        at.prev_in(&self.text).unwrap_or(at).byte()
    }

    /// The byte offset just past the character at `at`, or `at` at the end.
    fn next_boundary(&self, at: usize) -> usize {
        let at = TextCursor::from(at).snapped_in(&self.text);
        at.next_in(&self.text).unwrap_or(at).byte()
    }

    /// Replace the bytes in `start..end` with `with`, leaving the cursor just
    /// past the inserted text and nothing selected.
    ///
    /// The single place `text` is mutated. Insert, paste, delete, backspace
    /// and delete-selection are all this operation with different arguments,
    /// and each used to spell out its own `drain`/`insert` plus its own cursor
    /// adjustment — five chances to move the cursor to somewhere the text no
    /// longer has a character.
    fn replace_range(&mut self, start: usize, end: usize, with: &str) {
        let start = self.floor_boundary(start);
        let end = self.floor_boundary(end).max(start);
        self.text.replace_range(start..end, with);
        self.cursor = start.saturating_add(with.len());
        self.selection_anchor = None;
    }

    fn delete_selection(&mut self) {
        if !self.has_selection() {
            return;
        }
        let (start, end) = self.selection_range();
        self.replace_range(start, end, "");
    }

    fn select_all(&mut self) {
        self.selection_anchor = Some(0);
        self.cursor = self.text.len();
    }

    /// Update the selection anchor for a cursor move: holding shift starts (or
    /// keeps) a selection, releasing it drops one.
    fn anchor_for_move(&mut self, shift: bool) {
        if shift {
            self.selection_anchor.get_or_insert(self.cursor);
        } else {
            self.selection_anchor = None;
        }
    }

    fn move_cursor_left(&mut self, shift: bool) {
        // An unshifted arrow against a selection collapses it to that end
        // rather than moving — the cursor lands where the selection was, not
        // one character further.
        if !shift && self.has_selection() {
            let (start, _) = self.selection_range();
            self.cursor = start;
            self.selection_anchor = None;
            return;
        }
        self.anchor_for_move(shift);
        self.cursor = self.prev_boundary(self.cursor);
    }

    fn move_cursor_right(&mut self, shift: bool) {
        if !shift && self.has_selection() {
            let (_, end) = self.selection_range();
            self.cursor = end;
            self.selection_anchor = None;
            return;
        }
        self.anchor_for_move(shift);
        self.cursor = self.next_boundary(self.cursor);
    }

    fn move_home(&mut self, shift: bool) {
        self.anchor_for_move(shift);
        self.cursor = 0;
    }

    fn move_end(&mut self, shift: bool) {
        self.anchor_for_move(shift);
        self.cursor = self.text.len();
    }

    fn insert_char(&mut self, ch: char) {
        let (start, end) = self.selection_range();
        let mut buf = [0u8; 4];
        self.replace_range(start, end, ch.encode_utf8(&mut buf));
    }

    fn backspace(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        let start = self.prev_boundary(self.cursor);
        self.replace_range(start, self.cursor, "");
    }

    fn delete(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        let end = self.next_boundary(self.cursor);
        self.replace_range(self.cursor, end, "");
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
        let (start, end) = self.selection_range();
        let clip = core::mem::take(&mut self.clipboard);
        self.replace_range(start, end, &clip);
        self.clipboard = clip;
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
        // Remove duplicate if present, so re-running a command moves it to the
        // front rather than filling the list with one entry.
        self.history.retain(|h| h != command);
        self.history.push(command.to_string());
        self.trim_history();
    }

    /// Load history from a list of strings (e.g., read from file).
    pub fn load_history(&mut self, commands: Vec<String>) {
        self.history = commands;
        self.trim_history();
    }

    /// Drop the oldest entries until at most `MAX_HISTORY` remain.
    ///
    /// One rule in one place: `add_to_history` used to cap by removing a
    /// single entry (correct only because it is called after a single push)
    /// and `load_history` by draining a difference it computed itself.
    fn trim_history(&mut self) {
        let excess = self.history.len().saturating_sub(MAX_HISTORY);
        self.history.drain(0..excess);
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

            // Arrow keys steer whichever list is open: the autocomplete
            // popup if one is showing, otherwise the command history.
            Key::Up => {
                if self.browsing_suggestions() {
                    self.select_prev_suggestion();
                } else {
                    self.history_prev();
                }
            }

            Key::Down => {
                if self.browsing_suggestions() {
                    self.select_next_suggestion();
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
        if local_x < 0.0 || local_y < 0.0 || local_x > DIALOG_WIDTH || local_y > DIALOG_HEIGHT {
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
            overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Clip,
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
            let (start, _) = self.input.selection_range();
            // These are byte offsets that `floor_boundary` keeps on character
            // boundaries — but a render pass is the wrong place to find out
            // that one of them isn't, so it tolerates a bad offset the same
            // way `selected_text` already does rather than panicking mid-frame.
            let text_before_start = self.input.text.get(..start).unwrap_or("");
            let start_px = text::width(text_before_start, INPUT_FONT_SIZE);
            let sel_width = text::width(self.input.selected_text(), INPUT_FONT_SIZE);
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
            overflow: TextOverflow::Ellipsis,
        });

        // Cursor.
        let cursor_text = &self.input.text[..self.input.cursor];
        let cursor_px = text::width(cursor_text, INPUT_FONT_SIZE);
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
                overflow: TextOverflow::Ellipsis,
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
                    color: if is_selected {
                        theme::BLUE
                    } else {
                        theme::TEXT
                    },
                    font_size: INPUT_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(input_w - 16.0),
                    overflow: TextOverflow::Ellipsis,
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
            x + DIALOG_WIDTH - PADDING - BUTTON_WIDTH * 3.0 - BUTTON_SPACING * 2.0,
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
            x: text::center_x(
                label,
                bx + BUTTON_WIDTH / 2.0,
                BODY_FONT_SIZE,
                FontWeightHint::Regular,
            ),
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
            overflow: TextOverflow::Clip,
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
                if program
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
                {
                    return true;
                }
            }
        }

        false
    }

    /// Step one entry towards the *older* end of the history, entering browse
    /// mode from the newest entry if not already in it.
    ///
    /// Both directions read the entry through `get` and step with checked
    /// arithmetic. They used to index `self.history[idx]` after deciding the
    /// index was in range in the statement before — which held, but only
    /// because `history_index` was maintained correctly by the two methods
    /// that write it, at a distance of eighty lines from the index expression.
    fn history_prev(&mut self) {
        let entering = self.history_index.is_none();
        let target = match self.history_index {
            // Not browsing yet: start at the newest entry. `checked_sub` is
            // also the empty-history test — there is no newest entry.
            None => self.history.len().checked_sub(1),
            // Already at the oldest: stay there rather than wrapping.
            Some(idx) => Some(idx.saturating_sub(1)),
        };
        let Some(entry) = target.and_then(|idx| self.history.get(idx).cloned()) else {
            return;
        };
        if entering {
            self.pre_history_text = self.input.text.clone();
        }
        self.history_index = target;
        self.input.set_text(&entry);
        self.update_suggestions();
    }

    /// Step one entry towards the *newer* end, leaving browse mode and
    /// restoring the user's own text when stepping past the newest.
    fn history_next(&mut self) {
        if let Some(idx) = self.history_index {
            match idx
                .checked_add(1)
                .filter(|&newer| newer < self.history.len())
                .and_then(|newer| Some((newer, self.history.get(newer)?.clone())))
            {
                Some((newer, entry)) => {
                    self.history_index = Some(newer);
                    self.input.set_text(&entry);
                }
                None => {
                    // Past the newest entry: back to whatever was typed before
                    // browsing started.
                    self.history_index = None;
                    let saved = core::mem::take(&mut self.pre_history_text);
                    self.input.set_text(&saved);
                    self.pre_history_text = saved;
                }
            }
        }
        self.update_suggestions();
    }

    /// Whether the arrow keys are steering the autocomplete popup rather than
    /// the history.
    fn browsing_suggestions(&self) -> bool {
        self.show_autocomplete && self.suggestion_index.is_some()
    }

    /// Highlight the previous suggestion, stopping at the first.
    fn select_prev_suggestion(&mut self) {
        if let Some(idx) = self.suggestion_index {
            self.suggestion_index = Some(idx.saturating_sub(1));
        }
    }

    /// Highlight the next suggestion, stopping at the last.
    fn select_next_suggestion(&mut self) {
        if let Some(idx) = self.suggestion_index
            && let Some(next) = idx
                .checked_add(1)
                .filter(|&next| next < self.suggestions.len())
        {
            self.suggestion_index = Some(next);
        }
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
        results.sort_by_key(|r| std::cmp::Reverse(r.score));
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
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

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

    // ------------------------------------------------------------------
    // Multi-byte text, which is where the byte offsets are load-bearing
    // ------------------------------------------------------------------

    #[test]
    fn a_cursor_step_crosses_a_whole_character_not_a_byte() {
        // "é" is two bytes, "→" three, "😀" four: one of each, so a step that
        // moved by a fixed amount would land inside a character and the next
        // edit would panic.
        let mut input = TextInput::new();
        input.set_text("aé→😀b");
        input.move_home(false);
        let mut offsets = vec![input.cursor];
        for _ in 0..5 {
            input.move_cursor_right(false);
            offsets.push(input.cursor);
        }
        assert_eq!(offsets, vec![0, 1, 3, 6, 10, 11]);

        // And back, landing on the same boundaries in reverse.
        let mut back = vec![input.cursor];
        for _ in 0..5 {
            input.move_cursor_left(false);
            back.push(input.cursor);
        }
        back.reverse();
        assert_eq!(back, offsets);
    }

    #[test]
    fn backspace_and_delete_remove_one_whole_character() {
        let mut input = TextInput::new();
        input.set_text("a😀b");
        input.move_end(false);
        input.move_cursor_left(false); // before 'b'
        input.backspace();
        assert_eq!(input.text, "ab");
        assert_eq!(input.cursor, 1);

        let mut input = TextInput::new();
        input.set_text("a😀b");
        input.move_home(false);
        input.move_cursor_right(false); // after 'a'
        input.delete();
        assert_eq!(input.text, "ab");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn typing_or_pasting_over_a_selection_replaces_it() {
        let mut input = TextInput::new();
        input.set_text("hello world");
        input.selection_anchor = Some(0);
        input.cursor = 5;
        input.insert_char('X');
        assert_eq!(input.text, "X world");
        assert_eq!(input.cursor, 1);
        assert!(!input.has_selection());

        let mut input = TextInput::new();
        input.set_text("hello world");
        input.clipboard = "bye".to_string();
        input.selection_anchor = Some(0);
        input.cursor = 5;
        input.paste();
        assert_eq!(input.text, "bye world");
        assert_eq!(input.cursor, 3);
        // Pasting does not consume the clipboard.
        assert_eq!(input.clipboard, "bye");
    }

    #[test]
    fn an_offset_left_inside_a_character_shortens_the_edit_rather_than_panicking() {
        // Nothing in the type is supposed to produce a mid-character offset,
        // but "supposed to" is what a panic in `String::replace_range` is made
        // of. Every entry point clamps to a boundary instead.
        let mut input = TextInput::new();
        input.set_text("a😀b");
        input.cursor = 3; // inside the four-byte character, which spans 1..5
        input.selection_anchor = None;
        input.backspace();
        // The offset floors to the start of the character it was inside, so
        // backspace takes the `a` before that. *Which* character goes is not
        // the claim — the claim is that an offset the type cannot legitimately
        // hold produces a smaller edit and a cursor still on a boundary,
        // rather than a panic inside `String::replace_range`.
        assert_eq!(input.text, "😀b");
        assert!(input.text.is_char_boundary(input.cursor));

        let mut input = TextInput::new();
        input.set_text("a😀b");
        input.cursor = 99; // past the end
        input.delete();
        assert_eq!(input.text, "a😀b");
        assert!(input.text.is_char_boundary(input.cursor));
    }

    #[test]
    fn a_shifted_arrow_extends_the_selection_and_an_unshifted_one_collapses_it() {
        let mut input = TextInput::new();
        input.set_text("abcdef");
        input.move_home(false);
        input.move_cursor_right(true);
        input.move_cursor_right(true);
        assert_eq!(input.selected_text(), "ab");

        // Unshifted Left collapses to the near end without moving further.
        input.move_cursor_left(false);
        assert_eq!(input.cursor, 0);
        assert!(!input.has_selection());

        input.move_cursor_right(true);
        input.move_cursor_right(true);
        input.move_cursor_right(false);
        assert_eq!(input.cursor, 2);
        assert!(!input.has_selection());
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

    #[test]
    fn stepping_past_either_end_of_the_history_stops_rather_than_wrapping() {
        let mut dialog = RunDialog::new();
        dialog.show();
        dialog.add_to_history("one");
        dialog.add_to_history("two");

        // Older, older, and once more past the oldest.
        dialog.history_prev();
        assert_eq!(dialog.input.text, "two");
        dialog.history_prev();
        assert_eq!(dialog.input.text, "one");
        dialog.history_prev();
        assert_eq!(dialog.input.text, "one");

        // Back down, and one step past the newest returns the typed text.
        dialog.history_next();
        assert_eq!(dialog.input.text, "two");
        dialog.history_next();
        assert_eq!(dialog.input.text, "");
        assert!(dialog.history_index.is_none());
        // Already out of browse mode: another step changes nothing.
        dialog.history_next();
        assert_eq!(dialog.input.text, "");
    }

    #[test]
    fn browsing_an_empty_history_does_nothing() {
        let mut dialog = RunDialog::new();
        dialog.show();
        for ch in "typed".chars() {
            dialog.input.insert_char(ch);
        }
        dialog.history_prev();
        dialog.history_next();
        assert_eq!(dialog.input.text, "typed");
        assert!(dialog.history_index.is_none());
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
