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
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::step;
use crate::style::CornerRadii;
use crate::text::TextCursor;

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
/// The height of a breadcrumb pill: one line of text with padding above and
/// below it.
///
/// Named because three places want it, and one of them had spelled it out
/// again inside a halving — `y_center - (FONT_SIZE + SEGMENT_PADDING_V * 2.0)
/// / 2.0` — where it read as the midpoint between a font size and a padding,
/// two quantities that have no midpoint.
const SEGMENT_HEIGHT: f32 = FONT_SIZE + SEGMENT_PADDING_V * 2.0;
/// Stands for the segments that did not fit and were dropped from the left.
const ELLIPSIS: &str = "...";
/// Room held back for [`ELLIPSIS`] when deciding how many segments fit.
///
/// Held back rather than measured, because whether the marker is drawn at all
/// depends on how many segments fit — which is the question being answered.
const ELLIPSIS_RESERVE: f32 = 20.0;

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
    /// Cursor position: a byte offset into `edit_text`, plus which side of a
    /// direction boundary the caret is drawn on.
    ///
    /// A plain byte offset was enough while every path ran left to right. It is
    /// not enough for a path holding a Hebrew or Arabic directory name: the
    /// offset where the two directions meet is drawn at two x coordinates, and
    /// which one the caret goes to depends on how it got there. See
    /// [`TextCursor`].
    cursor: TextCursor,
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
            cursor: TextCursor::default(),
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
        self.cursor = self.edit_text.len().into();
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
        self.cursor = TextCursor::default();
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
        if let Some(ch) = event.text
            && !ch.is_control()
        {
            self.enter_edit_mode();
            // Insert the typed character.
            self.edit_text.clear();
            self.edit_text.push(ch);
            self.cursor = self.edit_text.len().into();
            return EventResult::Consumed;
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
                self.cursor = self.edit_text.len().into();
                EventResult::Consumed
            }
            _ => {
                // Insert character.
                if let Some(ch) = event.text
                    && !ch.is_control()
                {
                    self.delete_selection();
                    self.edit_text.insert(self.cursor.byte, ch);
                    // The caret stop after the insertion point is the far side
                    // of the character just inserted, so there is always one.
                    self.cursor = self.cursor.next_in(&self.edit_text).unwrap_or(self.cursor);
                    self.selection_anchor = None;
                    self.on_text_changed();
                    return EventResult::Consumed;
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
                self.cursor = self.cursor.byte.min(anchor).into();
                self.selection_anchor = None;
                return;
            }
        } else if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor.byte);
        }

        // Leftwards on the screen, which on a path holding a right-to-left
        // directory name is not backwards through the string: the caret walks
        // through that name rather than jumping across it. The operator chose
        // visual motion; the reasoning is `design-decisions.md` §541.
        //
        // Measured at `FONT_SIZE`/`Regular` because that is what `edit_text` is
        // drawn at — the gaps between glyphs belong to the shaped run, so any
        // other size would put the caret where this bar never drew it. And the
        // returned cursor is assigned whole: its affinity is what tells apart
        // the two screen positions that share one byte offset where the two
        // directions meet.
        if let Some(prev) = crate::text::caret_left(
            &self.edit_text,
            self.cursor,
            FONT_SIZE,
            FontWeightHint::Regular,
        ) {
            self.cursor = prev;
        }
    }

    fn move_cursor_right(&mut self, extend_selection: bool) {
        if !extend_selection {
            if let Some(anchor) = self.selection_anchor {
                self.cursor = self.cursor.byte.max(anchor).into();
                self.selection_anchor = None;
                return;
            }
        } else if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor.byte);
        }

        // Visual, for the reason given in `move_cursor_left` above.
        if let Some(next) = crate::text::caret_right(
            &self.edit_text,
            self.cursor,
            FONT_SIZE,
            FontWeightHint::Regular,
        ) {
            self.cursor = next;
        }
    }

    fn move_cursor_home(&mut self, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor.byte);
        } else if !extend_selection {
            self.selection_anchor = None;
        }
        self.cursor = TextCursor::default();
    }

    fn move_cursor_end(&mut self, extend_selection: bool) {
        if extend_selection && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor.byte);
        } else if !extend_selection {
            self.selection_anchor = None;
        }
        self.cursor = self.edit_text.len().into();
    }

    fn handle_backspace(&mut self) {
        if self.delete_selection() {
            self.on_text_changed();
            return;
        }
        // `String::remove` takes the offset of the character to remove, which is
        // exactly the cursor's new home — so the two come out of one lookup
        // rather than a subtraction guarded further up.
        if let Some(prev) = self.cursor.prev_in(&self.edit_text) {
            self.edit_text.remove(prev.byte());
            self.cursor = prev;
            self.on_text_changed();
        }
    }

    fn handle_delete(&mut self) {
        if self.delete_selection() {
            self.on_text_changed();
            return;
        }
        if self.cursor.byte < self.edit_text.len() {
            self.edit_text.remove(self.cursor.byte);
            self.on_text_changed();
        }
    }

    /// Delete the current selection, returning true if something was deleted.
    fn delete_selection(&mut self) -> bool {
        if let Some(anchor) = self.selection_anchor.take() {
            let start = self.cursor.byte.min(anchor);
            let end = self.cursor.byte.max(anchor);
            if start != end {
                self.edit_text.drain(start..end);
                self.cursor = start.into();
                return true;
            }
        }
        false
    }

    /// Called whenever edit text changes — requests autocomplete.
    fn on_text_changed(&mut self) {
        // Determine the prefix for autocomplete: everything up to and including the last '/'.
        let prefix = autocomplete_prefix(&self.edit_text, self.cursor.byte);
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
        let len = self.completions.len();
        self.completion_index = Some(match self.completion_index {
            Some(i) => step::wrapping_before(len, i),
            // Nothing selected: Up enters the list from the bottom.
            None => len.saturating_sub(1),
        });
        self.ensure_completion_visible();
    }

    fn move_completion_down(&mut self) {
        if !self.dropdown_visible || self.completions.is_empty() {
            return;
        }
        let len = self.completions.len();
        self.completion_index = Some(match self.completion_index {
            Some(i) => step::wrapping_after(len, i),
            // Nothing selected: Down enters the list from the top.
            None => 0,
        });
        self.ensure_completion_visible();
    }

    /// Scroll the dropdown the least distance that brings the selected row into
    /// view.
    fn ensure_completion_visible(&mut self) {
        let Some(idx) = self.completion_index else {
            return;
        };
        // The topmost scroll position that still shows `idx`: far enough down
        // that `idx` is the last visible row. `saturating_sub` is what makes it
        // 0 for a row already within the first windowful, which is the same
        // answer the old `idx + 1 - DROPDOWN_MAX_VISIBLE` gave — except that
        // one computed a negative number first and relied on never reaching
        // this line unless it was positive.
        let lowest = idx.saturating_add(1).saturating_sub(DROPDOWN_MAX_VISIBLE);
        // `min` then `max` rather than `clamp`, which panics when its bounds
        // cross — a hidden precondition is what this whole pass is removing.
        self.dropdown_scroll = self.dropdown_scroll.min(idx).max(lowest);
    }

    fn accept_completion(&mut self) {
        if !self.dropdown_visible {
            return;
        }
        let Some(item) = self
            .completion_index
            .and_then(|idx| self.completions.get(idx))
            .cloned()
        else {
            return;
        };

        // Replace the partial name after the last '/' with the completion. The
        // prefix to keep is "everything but the trailing name", which measures
        // straight off the split — the old form added one to a byte position
        // found in a different expression, which is only a character boundary
        // because the separator it found is one byte wide.
        let typed = self.edit_text.get(..self.cursor.byte).unwrap_or_default();
        let prefix_end = typed
            .rsplit_once('/')
            .map_or(0, |(_, name)| typed.len().saturating_sub(name.len()));

        // Remove everything after the prefix up to cursor.
        self.edit_text.drain(prefix_end..self.cursor.byte);

        // Insert the completion name; the cursor lands at its far end.
        let insert = if item.is_directory {
            format!("{}/", item.name)
        } else {
            item.name.clone()
        };
        self.edit_text.insert_str(prefix_end, &insert);
        self.cursor = prefix_end.saturating_add(insert.len()).into();

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
                // Hit-tested against the drawn glyphs rather than a nominal
                // cell, so a click lands on the character under the pointer
                // instead of one several letters away. The affinity the click
                // carries is kept: clicking the left edge of a right-to-left
                // word and clicking the right edge of the left-to-right word
                // before it yield the same byte offset but different carets,
                // and only the affinity tells them apart.
                self.cursor = crate::text::cursor_at(
                    &self.edit_text,
                    x - text_x,
                    FONT_SIZE,
                    FontWeightHint::Regular,
                );
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

        // The whole trail: every pill, with a gap and a separator between each
        // neighbouring pair, inside the bar's padding.
        let mut total_width = BAR_PADDING * 2.0;
        for (i, seg) in self.segments.iter().enumerate() {
            if i > 0 {
                total_width += SEGMENT_GAP + SEPARATOR_WIDTH;
            }
            total_width += pill_width(seg);
        }

        // When the trail is too long the leading segments are dropped, so the
        // deepest — the one the user is actually in — always survives. Walk
        // back from the end taking segments while they fit; `first_visible`
        // ends at `len` if not even the last one does, and the "..." then
        // stands for the lot.
        let overflow = total_width > width;
        let first_visible = if overflow {
            let available = width - BAR_PADDING * 2.0 - SEPARATOR_WIDTH - ELLIPSIS_RESERVE;
            let mut accum = 0.0f32;
            let mut first = self.segments.len();
            for (i, seg) in self.segments.iter().enumerate().rev() {
                let seg_total = pill_width(seg) + SEGMENT_GAP + SEPARATOR_WIDTH;
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

        // A separator belongs between two pills, so it is drawn before a pill
        // that has one in front of it rather than after a pill that has one
        // behind it — which is the same set of separators without having to
        // ask whether an index is the last.
        let mut x = BAR_PADDING;
        let mut preceded = false;

        if overflow && first_visible > 0 {
            let (_, _, ellipsis_w, _) = push_pill(cmds, x, y_center, ELLIPSIS, COLOR_SUBTEXT0);
            x += ellipsis_w + SEGMENT_GAP;
            preceded = true;
        }

        for seg in self.segments.get(first_visible..).unwrap_or_default() {
            if preceded {
                push_separator(cmds, x, y_center);
                x += SEPARATOR_WIDTH;
            }
            let (rx, ry, rw, rh) = push_pill(cmds, x, y_center, seg, COLOR_TEXT);
            self.segment_rects.push((rx, ry, rw, rh));
            x += rw + SEGMENT_GAP;
            preceded = true;
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
            let sel_start = self.cursor.byte.min(anchor);
            let sel_end = self.cursor.byte.max(anchor);
            // A selection is a *set* of rectangles, not one rectangle. The
            // selected bytes are contiguous in the string but need not be
            // contiguous on screen: a range that starts in Latin text and ends
            // inside a Hebrew directory name is drawn as two separated runs,
            // and the gap between them holds characters the user did not
            // select. Painting `x(end) - x(start)` would highlight those too.
            for (sel_x, sel_w) in crate::text::selection_boxes(
                &self.edit_text,
                sel_start,
                sel_end,
                FONT_SIZE,
                FontWeightHint::Regular,
            ) {
                cmds.push(RenderCommand::FillRect {
                    x: text_x + sel_x,
                    y: text_y - 2.0,
                    width: sel_w,
                    height: FONT_SIZE + 4.0,
                    color: Color::rgba(COLOR_LAVENDER.r, COLOR_LAVENDER.g, COLOR_LAVENDER.b, 60),
                    corner_radii: CornerRadii::all(2.0),
                });
            }
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
            overflow: TextOverflow::Ellipsis,
        });

        // Cursor. Placed by the shaper rather than by measuring the logical
        // prefix: at a direction boundary the caret does not sit at the
        // prefix's width, and which of the two candidate positions is right
        // depends on the affinity the cursor is carrying.
        let cursor_x = text_x
            + crate::text::caret_x(
                &self.edit_text,
                self.cursor,
                FONT_SIZE,
                FontWeightHint::Regular,
            );
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
        let dropdown_h = visible_count as f32 * DROPDOWN_ITEM_HEIGHT + DROPDOWN_PADDING * 2.0;
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

        // Items. The window is taken as a slice from the scroll position, so
        // its end is the slice's own rather than a sum to be clamped back
        // against the length it was derived from.
        let window = self
            .completions
            .get(self.dropdown_scroll..)
            .unwrap_or_default();

        // Which *row* of the window is selected — the comparison the highlight
        // wants — rather than which entry of the whole list, which would have
        // to be added back up per row. An entry above the window subtracts to
        // `None`; one below matches no row that is drawn.
        let selected_row = self
            .completion_index
            .and_then(|idx| idx.checked_sub(self.dropdown_scroll));

        for (vi, item) in window.iter().take(visible_count).enumerate() {
            let item_y = dropdown_y + DROPDOWN_PADDING + vi as f32 * DROPDOWN_ITEM_HEIGHT;

            // Highlight selected item.
            if selected_row == Some(vi) {
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
                overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Breadcrumb geometry
// ---------------------------------------------------------------------------
//
// The breadcrumb draws one shape — a rounded box with a label in it — three
// times over: once notionally, to measure the trail; once for the "..." that
// stands for the segments scrolled off the left; and once per visible segment.
// Each had its own copy of the same four numbers, and the copies had drifted:
// the measuring pass summed a width the drawing pass then recomputed, so the
// two agreed only by both being edited together.
//
// These are free functions rather than methods because the drawing loop holds
// a shared borrow of `segments` while pushing to `segment_rects`, and only a
// disjoint field borrow may coexist with that.

/// The width of the rounded box drawn behind `label`.
fn pill_width(label: &str) -> f32 {
    crate::text::width(label, FONT_SIZE) + SEGMENT_PADDING_H * 2.0
}

/// Draw one pill — `label` in a rounded box, centred vertically on `y_center`
/// — and return the rectangle it occupies, for hit testing.
fn push_pill(
    cmds: &mut Vec<RenderCommand>,
    x: f32,
    y_center: f32,
    label: &str,
    text_color: Color,
) -> (f32, f32, f32, f32) {
    let width = pill_width(label);
    let y = y_center - SEGMENT_HEIGHT / 2.0;
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: SEGMENT_HEIGHT,
        color: COLOR_SURFACE0,
        corner_radii: CornerRadii::all(SEGMENT_RADIUS),
    });
    cmds.push(RenderCommand::Text {
        x: x + SEGMENT_PADDING_H,
        y: y_center - FONT_SIZE / 2.0,
        text: label.to_string(),
        color: text_color,
        font_size: FONT_SIZE,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    (x, y, width, SEGMENT_HEIGHT)
}

/// Draw the ">" that stands between two pills.
fn push_separator(cmds: &mut Vec<RenderCommand>, x: f32, y_center: f32) {
    cmds.push(RenderCommand::Text {
        x,
        y: y_center - FONT_SIZE / 2.0,
        text: ">".to_string(),
        color: COLOR_SUBTEXT0,
        font_size: FONT_SIZE,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
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
///
/// `"/"` becomes `["/"]`; `"/home/user"` becomes `["/", "home", "user"]`.
fn split_path(path: &str) -> Vec<String> {
    if path.is_empty() || path == "/" {
        return vec!["/".to_string()];
    }

    let mut segments = Vec::new();

    if let Some(rest) = path.strip_prefix('/') {
        segments.push("/".to_string());
        for part in rest.split('/') {
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
///
/// An index past the end names the whole trail rather than being an error:
/// `take` clamps by construction, so there is no prefix length to compute and
/// then clamp back against the slice it was derived from.
///
/// The two early returns this replaced — for an empty slice and for a prefix
/// that is exactly the root — both re-derived answers the loop below already
/// gives: no segments leaves `path` empty, and a lone `"/"` segment pushes a
/// single slash.
fn rebuild_path(segments: &[String], up_to_index: usize) -> String {
    let mut path = String::new();
    for (i, seg) in segments
        .iter()
        .take(up_to_index.saturating_add(1))
        .enumerate()
    {
        if i == 0 && seg == "/" {
            path.push('/');
        } else {
            if i > 0 && !path.ends_with('/') {
                path.push('/');
            }
            path.push_str(seg);
        }
    }

    // An empty trail, or one whose segments were all empty, is the root.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

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
        assert_eq!(bar.cursor.byte, 3);

        bar.handle_key_event(&key_press(Key::Left));
        assert_eq!(bar.cursor.byte, 2);

        bar.handle_key_event(&key_press(Key::Left));
        assert_eq!(bar.cursor.byte, 1);

        bar.handle_key_event(&key_press(Key::Right));
        assert_eq!(bar.cursor.byte, 2);

        bar.handle_key_event(&key_press(Key::Home));
        assert_eq!(bar.cursor.byte, 0);

        bar.handle_key_event(&key_press(Key::End));
        assert_eq!(bar.cursor.byte, 3);
    }

    #[test]
    fn test_select_all() {
        let mut bar = PathBar::new("/home");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        bar.handle_key_event(&key_press_ctrl(Key::A));
        assert_eq!(bar.selection_anchor, Some(0));
        assert_eq!(bar.cursor.byte, 5); // "/home" is 5 bytes.
    }

    // --- Autocomplete tests ---

    #[test]
    fn test_autocomplete_matching() {
        let mut bar = PathBar::new("/home");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        // Simulate typing "/home/" to trigger autocomplete.
        bar.edit_text = "/home/".to_string();
        bar.cursor = 6.into();
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
        bar.cursor = 6.into();

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
        bar.cursor = 6.into();

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
        bar.cursor = 1.into();
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

    /// The path bar's arrows walk the *screen*, not the string
    /// (`design-decisions.md` §541) — and they do it in the middle of a path,
    /// not just on a bare word.
    ///
    /// `/x/ab\u{05D0}\u{05D1}cd` draws as `/ x / a b <bet> <aleph> c d`: the two
    /// Hebrew letters run right-to-left inside a left-to-right line, so the one
    /// stored second is painted first. The `/x/` before them is unaffected, and
    /// that is half the claim — a directory whose name is in a right-to-left
    /// script must not change how the rest of the path is walked.
    ///
    /// The other half is the repeat. The two gaps where the directions meet —
    /// `b|<bet>` and `<aleph>|c` — each answer to *both* byte 5 and byte 9, and
    /// which one is reported depends on the side the caret is on. It keeps the
    /// side it is travelling towards, so leftwards reports 9 at both gaps and
    /// rightwards reports 5 at both.
    ///
    /// **A failure here showing a sequence without the repeat is §541's
    /// measured trap**: a bar that stored only the byte cannot tell the second
    /// 9 from the first, and jumps the whole directory name in one keypress —
    /// worse than the logical motion this replaced.
    #[test]
    fn the_path_bars_arrows_cross_a_right_to_left_directory_name_letter_by_letter() {
        let mut bar = PathBar::new("/x/ab\u{05D0}\u{05D1}cd");
        bar.handle_key_event(&key_press_ctrl(Key::L)); // edit mode copies `path`
        bar.drain_events();
        assert_eq!(bar.edit_text, "/x/ab\u{05D0}\u{05D1}cd");
        assert_eq!(bar.cursor.byte(), 11, "edit mode starts at the end");

        let mut leftwards = Vec::new();
        for _ in 0..9 {
            bar.handle_key_event(&key_press(Key::Left));
            leftwards.push(bar.cursor.byte());
        }
        assert_eq!(leftwards, vec![10, 9, 7, 9, 4, 3, 2, 1, 0]);

        let mut rightwards = Vec::new();
        for _ in 0..9 {
            bar.handle_key_event(&key_press(Key::Right));
            rightwards.push(bar.cursor.byte());
        }
        assert_eq!(rightwards, vec![1, 2, 3, 4, 5, 7, 5, 10, 11]);
    }

    #[test]
    fn test_navigate_via_enter() {
        let mut bar = PathBar::new("/home");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        bar.edit_text = "/usr/local/bin".to_string();
        bar.cursor = bar.edit_text.len().into();

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
        let mut bar = PathBar::new("/very/long/path/with/many/segments/that/will/overflow");
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

    // --- Breadcrumb geometry ---

    /// The rectangles the click handler tests against are the rectangles that
    /// were drawn. These used to be two separate calculations of the same
    /// four numbers — the measuring pass summed a width the drawing pass then
    /// recomputed — so they agreed only for as long as both were edited
    /// together, and nothing said so.
    #[test]
    fn every_segment_is_clickable_exactly_where_it_was_drawn() {
        let mut bar = PathBar::new("/home/user/Documents");
        let cmds = bar.render(800, 32);

        // The bar's own background is a fill too; the pills are the ones one
        // line of text tall.
        let pills: Vec<(f32, f32, f32, f32)> = cmds
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } if *height == SEGMENT_HEIGHT => Some((*x, *y, *width, *height)),
                _ => None,
            })
            .collect();

        assert_eq!(bar.segment_rects, pills, "hit boxes are the drawn boxes");
        assert_eq!(bar.segment_rects.len(), 4);
    }

    /// Consecutive pills are one gap and one separator apart — the spacing the
    /// measuring pass charges for each join. A separator is drawn *before* a
    /// pill that has one in front of it rather than after a pill that is not
    /// the last, and the two must lay out identically.
    #[test]
    fn neighbouring_segments_are_a_gap_and_a_separator_apart() {
        let mut bar = PathBar::new("/home/user/Documents");
        bar.render(800, 32);

        for pair in bar.segment_rects.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            let expected = left.0 + left.2 + SEGMENT_GAP + SEPARATOR_WIDTH;
            assert!(
                (right.0 - expected).abs() < 0.01,
                "segment at {} should follow the one ending at {left:?}",
                right.0
            );
        }
    }

    /// The trail is laid out inside the width it was measured against: when
    /// nothing overflows, the last pill ends within the bar's padding.
    #[test]
    fn a_trail_that_fits_stays_inside_the_bar() {
        let mut bar = PathBar::new("/home/user/Documents");
        let width = 800.0;
        bar.render(800, 32);

        let last = *bar.segment_rects.last().expect("four segments were drawn");
        assert!(last.0 + last.2 <= width - BAR_PADDING);
        assert_eq!(
            bar.segment_rects.first().map(|r| r.0),
            Some(BAR_PADDING),
            "with no overflow the first pill starts at the padding"
        );
    }

    /// When the trail overflows, the ellipsis is followed by one separator and
    /// then only the segments that fit — never a separator with nothing on one
    /// side of it.
    #[test]
    fn an_overflowing_trail_draws_one_separator_per_join() {
        let mut bar = PathBar::new("/very/long/path/with/many/segments/that/will/overflow");
        let cmds = bar.render(150, 32);

        let texts: Vec<&str> = cmds
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert!(texts.contains(&ELLIPSIS), "the dropped segments are marked");
        let separators = texts.iter().filter(|t| **t == ">").count();
        // One join per visible segment: each is preceded by the ellipsis or by
        // another segment.
        assert_eq!(separators, bar.segment_rects.len());
    }

    /// An index past the last segment names the whole trail. It reaches
    /// `rebuild_path` from a click test against a stale `segment_rects`, so it
    /// must not be a panic.
    #[test]
    fn rebuilding_past_the_end_yields_the_whole_path() {
        let segments = vec!["/".to_string(), "home".to_string(), "user".to_string()];
        assert_eq!(rebuild_path(&segments, 2), "/home/user");
        assert_eq!(rebuild_path(&segments, 99), "/home/user");
        assert_eq!(rebuild_path(&segments, usize::MAX), "/home/user");
        assert_eq!(rebuild_path(&[], 0), "/");
        assert_eq!(rebuild_path(&[], usize::MAX), "/");
    }

    // --- Editing text that is not one byte per character ---

    /// The caret moves by characters, not by bytes. Stepping onto a byte
    /// inside a character is the difference between a cursor and a panic, and
    /// the arrow keys used to reach the offset by subtraction.
    #[test]
    fn the_caret_steps_over_a_multibyte_character_whole() {
        // "é" is two bytes, "日" and "本" three each.
        let mut bar = PathBar::new("/é/日本");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();

        bar.handle_key_event(&key_press(Key::Home));
        assert_eq!(bar.cursor.byte, 0);

        // Every stop is a character boundary, and every character is stepped
        // over in one press.
        for expected in [1, 3, 4, 7, 10] {
            bar.handle_key_event(&key_press(Key::Right));
            assert_eq!(bar.cursor.byte, expected);
        }

        // At the end the caret stays put rather than stepping out of range.
        bar.handle_key_event(&key_press(Key::Right));
        assert_eq!(bar.cursor.byte, 10);

        // And back down the same offsets.
        for expected in [7, 4, 3, 1, 0] {
            bar.handle_key_event(&key_press(Key::Left));
            assert_eq!(bar.cursor.byte, expected);
        }
        bar.handle_key_event(&key_press(Key::Left));
        assert_eq!(bar.cursor.byte, 0);
    }

    /// Backspace removes one character, not one byte — and the offset it
    /// removes at is the offset the caret lands on, so the two cannot disagree.
    #[test]
    fn backspace_removes_a_whole_multibyte_character() {
        let mut bar = PathBar::new("/é");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();
        assert_eq!(bar.cursor.byte, 3);

        bar.handle_key_event(&key_press(Key::Backspace));
        assert_eq!(bar.edit_text, "/");
        assert_eq!(bar.cursor.byte, 1);

        bar.handle_key_event(&key_press(Key::Backspace));
        assert_eq!(bar.edit_text, "");
        assert_eq!(bar.cursor.byte, 0);

        // Nothing left to remove, and no offset to underflow.
        bar.handle_key_event(&key_press(Key::Backspace));
        assert_eq!(bar.edit_text, "");
        assert_eq!(bar.cursor.byte, 0);
    }

    /// Typing a character leaves the caret on its far side, whatever its
    /// width in bytes.
    #[test]
    fn typing_a_multibyte_character_leaves_the_caret_past_it() {
        let mut bar = PathBar::new("/");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.drain_events();
        bar.handle_key_event(&key_press(Key::Home));

        bar.handle_key_event(&key_press_with_text(Key::E, 'é'));
        assert_eq!(bar.edit_text, "é/");
        assert_eq!(bar.cursor.byte, 2);
    }
}
