//! Path bar widget — combined breadcrumb display / text input with autocomplete.
//!
//! Operates in two modes:
//! - **Breadcrumb mode** (default): shows the path as clickable crumbs separated by chevrons
//!   -- the reference desktop's address bar (`aero-crumbs`): plain names in an input's
//!   well, the folder you are in set bold
//! - **Edit mode**: full text input with autocomplete dropdown for directory navigation
//!
//! The widget does not perform filesystem I/O. It emits `PathBarEvent::RequestAutoComplete`
//! and the host provides completions via `set_completions()`.

use crate::color::Color;
use crate::event::{EventResult, Key, KeyEvent, MouseEvent, MouseEventKind};
use crate::osbytes::split_on_slash;
use crate::palette::Palette;
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::step;
use crate::style::CornerRadii;
use crate::surface::Surface;
use crate::text::TextCursor;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Catppuccin Mocha palette
// ---------------------------------------------------------------------------

// The colours come from the user's palette, threaded in by the caller.
//
// Nine Catppuccin Mocha constants used to sit here, so a breadcrumb bar was dark on a
// light desktop. 838 moved `Palette` into this crate so a widget could name
// one; mapped by value, so a name that said what a colour was *for* becomes
// the role it always held.

/// Shadow color.
const COLOR_SHADOW: Color = Color::rgba(0, 0, 0, 100);

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The crumbs' and the typed path's size: the toolkit's body text. The
/// reference sets its crumbs at 12.5px beside 13px body text; one size keeps
/// the path from jumping when the bar turns into a text field.
const FONT_SIZE: f32 = 13.0;
/// Room either side of a crumb's name inside its click target: the
/// reference's `padding: 0 2px`, and a pixel more, since the target is what a
/// pointer has to land in.
const CRUMB_PADDING_H: f32 = 3.0;
/// The slot the chevron between two crumbs sits in: the reference's 14px
/// separator glyph.
const SEPARATOR_WIDTH: f32 = 14.0;
/// How far the chevron reaches above and below the bar's centre line, and how
/// far it points: the reference's `M6 4l4 4-4 4`, in a 16-unit box drawn 14
/// wide.
const CHEVRON_REACH: f32 = 3.5;
/// The chevron's stroke: the reference's 1.5, at the same scale.
const CHEVRON_STROKE: f32 = 1.3;
/// The field's inside margin before the first crumb: the reference's
/// `padding: 0 4px 0 9px`.
const FIELD_PADDING_LEFT: f32 = 9.0;
/// The field's inside margin after the last crumb.
const FIELD_PADDING_RIGHT: f32 = 4.0;
/// The field's corners, and the dropdown's: the reference's 3px.
const FIELD_RADIUS: f32 = 3.0;
/// Where typed text starts in edit mode: where the first crumb's name was
/// drawn, so turning the trail into text leaves the path's first character
/// where it was.
///
/// One constant for the drawing and for the click that places the caret. They
/// were two numbers before, four pixels apart -- the text drawn at 8, the click
/// measured from 4 -- so a click put the caret half a character to the right
/// of where it was aimed.
const EDIT_TEXT_X: f32 = FIELD_PADDING_LEFT + CRUMB_PADDING_H;
const DROPDOWN_ITEM_HEIGHT: f32 = 24.0;
const DROPDOWN_MAX_VISIBLE: usize = 8;
const DROPDOWN_PADDING: f32 = 4.0;
/// Stands for the segments that did not fit and were dropped from the left.
const ELLIPSIS: &str = "...";

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
    Navigate(PathBuf),
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
    path: PathBuf,
    /// Parsed segments of `path`.
    segments: Vec<Segment>,

    /// Current mode.
    mode: Mode,

    // --- Edit mode state ---
    /// The text being edited.
    edit_text: String,
    /// The bytes `edit_text` was rendered from, while editing.
    ///
    /// `edit_text` is a text field with a caret, so it must stay a `String`.
    /// That makes it a *rendering* of a path that need not be text -- each
    /// byte that is not text shown as an octal escape
    /// (`pathcodec::display_os`). Keeping the original here lets an unedited
    /// field navigate to the bytes it was opened with rather than to the
    /// escapes spelled out as characters -- exactly how
    /// `RunDialog::command_exact` keeps a command.
    edit_exact: Option<PathBuf>,
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
    /// Where each drawn crumb is, and which segment it stands for.
    crumb_hits: Vec<CrumbHit>,
}

/// Where one crumb was drawn, and which segment of the trail it stands for.
///
/// The segment is carried rather than implied by the crumb's position among
/// the drawn ones. When the trail overflows, its leading segments are not
/// drawn, so the first crumb on screen is not the first segment -- and while
/// the position stood in for the segment, clicking the first crumb after the
/// "..." went to the root, and every other crumb to the wrong folder.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CrumbHit {
    /// Index into [`PathBar::segments`].
    segment: usize,
    /// `(x, y, w, h)` in the bar's own space.
    rect: (f32, f32, f32, f32),
}

impl PathBar {
    /// Create a new path bar with the given initial path.
    pub fn new(initial_path: impl AsRef<Path>) -> Self {
        let path = normalize_path(initial_path.as_ref().as_os_str());
        let segments = split_path(path.as_os_str());
        Self {
            path,
            segments,
            mode: Mode::Breadcrumb,
            edit_text: String::new(),
            edit_exact: None,
            cursor: TextCursor::default(),
            selection_anchor: None,
            completions: Vec::new(),
            completion_index: None,
            dropdown_visible: false,
            dropdown_scroll: 0,
            path_invalid: false,
            pending_events: Vec::new(),
            crumb_hits: Vec::new(),
        }
    }

    /// Update the displayed path (resets to breadcrumb mode).
    pub fn set_path(&mut self, path: impl AsRef<Path>) {
        self.path = normalize_path(path.as_ref().as_os_str());
        self.segments = split_path(self.path.as_os_str());
        self.exit_edit_mode(false);
    }

    /// Current confirmed path.
    pub fn current_path(&self) -> &Path {
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
    pub fn render(&mut self, palette: &Palette, width: u32, height: u32) -> Vec<RenderCommand> {
        let w = width as f32;
        let h = height as f32;
        let mut cmds = Vec::new();

        // The field: the well every text input sinks into (`crust`), with a
        // quiet edge round it -- the reference's white field and its pale
        // line, in the palette's words. The same field in both modes, because
        // it is one field: clicking the trail turns it into the text it stands
        // for, in place, and a box that changed colour as it did so would read
        // as a second control appearing over the first.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: palette.crust,
            corner_radii: CornerRadii::all(FIELD_RADIUS),
        });

        // The edge turns red while a typed path is known not to exist.
        let border_color = if self.mode == Mode::Edit && self.path_invalid {
            palette.red
        } else {
            palette.surface1
        };
        cmds.push(RenderCommand::StrokeRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: border_color,
            line_width: 1.0,
            corner_radii: CornerRadii::all(FIELD_RADIUS),
        });

        match self.mode {
            Mode::Breadcrumb => self.render_breadcrumb(palette, &mut cmds, w, h),
            Mode::Edit => self.render_edit(palette, &mut cmds, w, h),
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
        self.edit_text = pathcodec::display_path(&self.path);
        self.edit_exact = Some(self.path.clone());
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
        self.edit_exact = None;
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
        if event.types_text() {
            self.enter_edit_mode();
            // Insert everything the keystroke typed.
            self.edit_text.clear();
            self.edit_text.extend(event.typed());
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
                if event.types_text() {
                    self.delete_selection();
                    for ch in event.typed() {
                        self.edit_text.insert(self.cursor.byte, ch);
                        // The caret stop after the insertion point is the far
                        // side of the character just inserted, so there is
                        // always one.
                        self.cursor = self.cursor.next_in(&self.edit_text).unwrap_or(self.cursor);
                    }
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
        // If the field still reads exactly as the path it was opened with,
        // the user did not edit it, so navigate to the bytes rather than to
        // their rendering. Typing anything makes the text the source of
        // truth, because then it is what the user actually asked for.
        let typed = match &self.edit_exact {
            Some(exact) if pathcodec::display_path(exact) == self.edit_text => {
                exact.clone().into_os_string()
            }
            _ => OsString::from(self.edit_text.clone()),
        };
        let new_path = normalize_path(&typed);
        self.path.clone_from(&new_path);
        self.segments = split_path(self.path.as_os_str());
        self.pending_events.push(PathBarEvent::Navigate(new_path));
        self.exit_edit_mode(false);
    }

    fn navigate_to_segment(&mut self, segment_index: usize) {
        // Build path from segments[0..=segment_index].
        let new_path = rebuild_path(&self.segments, segment_index);
        self.path.clone_from(&new_path);
        self.segments = split_path(self.path.as_os_str());
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
                // A crumb goes to the segment it stands for -- carried by the
                // hit, not read off its position among the drawn crumbs (see
                // `CrumbHit`).
                let crumb = self.crumb_hits.iter().find_map(|hit| {
                    let (sx, sy, sw, sh) = hit.rect;
                    (x >= sx && x <= sx + sw && y >= sy && y <= sy + sh).then_some(hit.segment)
                });
                if let Some(segment) = crumb {
                    self.navigate_to_segment(segment);
                    return EventResult::Consumed;
                }
                // Anywhere else in the field -- past the last crumb, on a
                // chevron -- turns the trail into text to type over.
                self.enter_edit_mode();
                EventResult::Consumed
            }
            Mode::Edit => {
                // Click in dropdown?
                // For now, position cursor based on x.
                let text_x = EDIT_TEXT_X;
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

    fn render_breadcrumb(
        &mut self,
        palette: &Palette,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        height: f32,
    ) {
        self.crumb_hits.clear();
        let y_center = height / 2.0;
        // The folder the bar is showing: the one crumb set bold.
        let current = self.segments.len().saturating_sub(1);

        // The whole trail: every crumb, with a chevron between each
        // neighbouring pair, inside the field's padding.
        let mut total_width = FIELD_PADDING_LEFT + FIELD_PADDING_RIGHT;
        for (i, seg) in self.segments.iter().enumerate() {
            if i > 0 {
                total_width += SEPARATOR_WIDTH;
            }
            total_width += crumb_width(&seg.label, crumb_weight(i == current));
        }

        // When the trail is too long the leading segments are dropped, so the
        // deepest -- the one the user is actually in -- always survives. Walk
        // back from the end taking segments while they fit beside the "..."
        // that will stand for the rest; each one taken brings the chevron in
        // front of it. The marker's room is its measured width rather than a
        // guess: it is drawn whenever a segment is dropped, and this is the
        // room it is drawn in.
        let overflow = total_width > width;
        let first_visible = if overflow {
            let available = width
                - FIELD_PADDING_LEFT
                - FIELD_PADDING_RIGHT
                - crumb_width(ELLIPSIS, FontWeightHint::Regular);
            let mut accum = 0.0f32;
            let mut first = self.segments.len();
            for (i, seg) in self.segments.iter().enumerate().rev() {
                let seg_total =
                    SEPARATOR_WIDTH + crumb_width(&seg.label, crumb_weight(i == current));
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

        // A chevron belongs between two crumbs, so it is drawn before a crumb
        // that has one in front of it rather than after a crumb that has one
        // behind it -- which is the same set of chevrons without having to ask
        // whether an index is the last.
        let mut x = FIELD_PADDING_LEFT;
        let mut preceded = false;

        // Never a trail with no folder in it: when not even the current
        // folder fits beside the marker it is drawn anyway, cut short to the
        // room there is. The folder you are in is the one crumb that must be
        // on screen. The marker is drawn only when something was actually
        // left out -- a lone "/" too wide for its field is cut short, not
        // announced as a longer path.
        let first_drawn = first_visible.min(current);
        if first_drawn > 0 {
            let (_, _, marker_w, _) = push_crumb(
                cmds,
                x,
                height,
                ELLIPSIS,
                palette.subtext0,
                FontWeightHint::Regular,
                f32::INFINITY,
            );
            x += marker_w;
            preceded = true;
        }

        for (i, seg) in self.segments.iter().enumerate().skip(first_drawn) {
            if preceded {
                push_chevron(palette, cmds, x, y_center);
                x += SEPARATOR_WIDTH;
            }
            let room = (width - FIELD_PADDING_RIGHT - x).max(0.0);
            let rect = push_crumb(
                cmds,
                x,
                height,
                &seg.label,
                palette.text,
                crumb_weight(i == current),
                room,
            );
            self.crumb_hits.push(CrumbHit { segment: i, rect });
            x += rect.2;
            preceded = true;
        }
    }

    // -----------------------------------------------------------------------
    // Rendering — Edit mode
    // -----------------------------------------------------------------------

    fn render_edit(
        &self,
        palette: &Palette,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        height: f32,
    ) {
        let y_center = height / 2.0;
        let text_y = y_center - FONT_SIZE / 2.0;
        // The field `render` drew is already the input's well; the text goes
        // where the first crumb's name was.
        let text_x = EDIT_TEXT_X;

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
                    color: Color::rgba(
                        palette.lavender.r,
                        palette.lavender.g,
                        palette.lavender.b,
                        60,
                    ),
                    corner_radii: CornerRadii::all(2.0),
                });
            }
        }

        // Text.
        cmds.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: self.edit_text.clone(),
            color: palette.text,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some((width - EDIT_TEXT_X - FIELD_PADDING_RIGHT).max(0.0)),
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
        // Through the shared helper rather than a rectangle of its own: one
        // place decides how wide a caret is, so an accessibility scale has
        // somewhere to apply. The geometry is unchanged -- it started two
        // pixels above the text and ran four taller, and still does.
        let mut caret = crate::render::RenderTree::new();
        crate::textedit::push_caret(
            &mut caret,
            cursor_x,
            text_y - 2.0,
            FONT_SIZE + 4.0,
            palette.lavender,
            crate::textedit::CARET_WIDTH,
        );
        cmds.extend(caret.commands);

        // Autocomplete dropdown.
        if self.dropdown_visible && !self.completions.is_empty() {
            self.render_dropdown(palette, cmds, width, height);
        }
    }

    fn render_dropdown(
        &self,
        palette: &Palette,
        cmds: &mut Vec<RenderCommand>,
        width: f32,
        bar_height: f32,
    ) {
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
            corner_radii: CornerRadii::all(FIELD_RADIUS),
        });

        // Background.
        palette.push_surface(
            cmds,
            0.0,
            dropdown_y,
            dropdown_w,
            dropdown_h,
            FIELD_RADIUS,
            Surface::Panel,
        );

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
                palette.push_surface(
                    cmds,
                    DROPDOWN_PADDING,
                    item_y,
                    dropdown_w - DROPDOWN_PADDING * 2.0,
                    DROPDOWN_ITEM_HEIGHT,
                    3.0,
                    Surface::Selected,
                );
            }

            // Directory indicator.
            let icon_color = if item.is_directory {
                palette.blue
            } else {
                palette.subtext0
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
                color: palette.text,
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
// The trail is measured once to decide what fits and drawn once, and the two
// passes must agree on every width or a crumb is drawn in one place and
// clicked in another. So both ask the same two functions.
//
// These are free functions rather than methods because the drawing loop holds
// a shared borrow of `segments` while pushing to `crumb_hits`, and only a
// disjoint field borrow may coexist with that.

/// The weight a crumb is set in: bold for the folder the bar is showing, as
/// the reference marks its current crumb (`is-current`).
fn crumb_weight(is_current: bool) -> FontWeightHint {
    if is_current {
        FontWeightHint::Bold
    } else {
        FontWeightHint::Regular
    }
}

/// How wide a crumb is: its name in `weight`, with its padding either side.
fn crumb_width(label: &str, weight: FontWeightHint) -> f32 {
    crate::text::padded_width(label, CRUMB_PADDING_H, FONT_SIZE, weight)
}

/// Draw one crumb -- `label` in `weight`, centred on the bar's height, cut
/// short with an ellipsis if it is wider than `room` -- and return the
/// rectangle a click on it lands in: the name and its padding, the bar's full
/// height.
///
/// The full height rather than the text's: a crumb is a target in a strip, and
/// a click just above or below a name is aimed at that name.
fn push_crumb(
    cmds: &mut Vec<RenderCommand>,
    x: f32,
    height: f32,
    label: &str,
    color: Color,
    weight: FontWeightHint,
    room: f32,
) -> (f32, f32, f32, f32) {
    let full = crumb_width(label, weight);
    let (width, max_width, overflow) = if full <= room {
        (full, None, TextOverflow::Clip)
    } else {
        let text_room = (room - CRUMB_PADDING_H * 2.0).max(0.0);
        (room, Some(text_room), TextOverflow::Ellipsis)
    };
    cmds.push(RenderCommand::Text {
        x: x + CRUMB_PADDING_H,
        y: height / 2.0 - FONT_SIZE / 2.0,
        text: label.to_string(),
        color,
        font_size: FONT_SIZE,
        font_weight: weight,
        max_width,
        overflow,
    });
    (x, 0.0, width, height)
}

/// Draw the chevron between two crumbs, centred in the slot that starts at
/// `x`.
///
/// Two strokes rather than a `>` glyph, as the tree view draws its disclosure
/// arrows (`treeview::push_arrow`): the reference's separator is a drawn
/// chevron, and a glyph's shape and weight depend on the font a user
/// installed. In `overlay0`, the palette's role for separators -- the faintest
/// mark that is still seen, as the reference's is a pale blue beside its ink.
fn push_chevron(palette: &Palette, cmds: &mut Vec<RenderCommand>, x: f32, y_center: f32) {
    let centre = x + SEPARATOR_WIDTH / 2.0;
    let open = centre - CHEVRON_REACH / 2.0;
    let tip = centre + CHEVRON_REACH / 2.0;
    for end in [y_center - CHEVRON_REACH, y_center + CHEVRON_REACH] {
        cmds.push(RenderCommand::Line {
            x1: open,
            y1: end,
            x2: tip,
            y2: y_center,
            color: palette.overlay0,
            width: CHEVRON_STROKE,
        });
    }
}

// ---------------------------------------------------------------------------
// Path utilities
// ---------------------------------------------------------------------------

/// Normalize a path: collapse repeated slashes, drop a trailing slash except
/// at the root.
///
/// # Why this works on bytes and not on `str`
///
/// A path here is bytes: `design.txt` allows every byte in a name except `/`
/// and NUL, so a path need not be text at all. Rendering it through `str`
/// would replace whatever is not UTF-8 with `U+FFFD` and then *navigate to the
/// replacement* -- a different path that looks right on screen.
///
/// The split is on `/` rather than through `Path::components()` on purpose.
/// `components()` is host-dependent: on the Windows machine these tests run on
/// it also treats `\` as a separator and `C:\` as a prefix, so the widget
/// would behave one way under test and another on the target. SlateOS has a
/// single separator, so splitting on it is the target-correct thing to do.
fn normalize_path(path: &OsStr) -> PathBuf {
    let mut out = OsString::new();

    if path.as_encoded_bytes().first() == Some(&b'/') {
        out.push("/");
    }

    let mut first = true;
    for part in split_on_slash(path) {
        if part.is_empty() {
            continue;
        }
        if !first {
            out.push("/");
        }
        out.push(part);
        first = false;
    }

    if out.is_empty() {
        return PathBuf::from("/");
    }
    PathBuf::from(out)
}

/// One breadcrumb: the bytes it navigates to, and the text drawn on it.
///
/// Two fields rather than one because they answer different questions and only
/// one of them may be a rendering. `label` is what the crumb shows, and a crumb is
/// text -- bytes that are not text cannot be drawn as themselves, so the label
/// spells them as `pathcodec::display_os` does, which is correct *for
/// drawing*. `exact` is what a click navigates to, and must be the original
/// bytes. The same split as `dialog.rs`'s `filename_input` /
/// `filename_exact` and `RunDialog`'s `command_exact`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    /// The exact bytes of this path component. Never derived from `label`.
    exact: OsString,
    /// What is drawn. Lossy, and never used to build a path.
    label: String,
}

impl Segment {
    /// A segment for one path component.
    fn new(exact: &OsStr) -> Self {
        Self {
            exact: exact.to_os_string(),
            label: pathcodec::display_os(exact),
        }
    }

    /// The leading `"/"` crumb of an absolute path.
    fn root() -> Self {
        Self::new(OsStr::new("/"))
    }

    /// Whether this is the root crumb.
    fn is_root(&self) -> bool {
        self.exact == OsStr::new("/")
    }
}

/// Split a normalized path into display segments.
///
/// `"/"` becomes `["/"]`; `"/home/user"` becomes `["/", "home", "user"]`.
fn split_path(path: &OsStr) -> Vec<Segment> {
    let mut segments = Vec::new();

    if path.as_encoded_bytes().first() == Some(&b'/') {
        segments.push(Segment::root());
    }

    for part in split_on_slash(path) {
        if part.is_empty() {
            continue;
        }
        segments.push(Segment::new(part));
    }

    if segments.is_empty() {
        segments.push(Segment::root());
    }

    segments
}

/// Rebuild a path from segments up to and including `up_to_index`.
///
/// An index past the end names the whole trail rather than being an error:
/// `take` clamps by construction, so there is no prefix length to compute and
/// then clamp back against the slice it was derived from.
///
/// The two early returns this replaced -- for an empty slice and for a prefix
/// that is exactly the root -- both re-derived answers the loop below already
/// gives: no segments leaves `path` empty, and a lone `"/"` segment pushes a
/// single slash.
fn rebuild_path(segments: &[Segment], up_to_index: usize) -> PathBuf {
    let mut path = OsString::new();
    for (i, seg) in segments
        .iter()
        .take(up_to_index.saturating_add(1))
        .enumerate()
    {
        if i == 0 && seg.is_root() {
            path.push("/");
        } else {
            if i > 0 && !path.as_encoded_bytes().ends_with(b"/") {
                path.push("/");
            }
            path.push(&seg.exact);
        }
    }

    // An empty trail, or one whose segments were all empty, is the root.
    if path.is_empty() {
        return PathBuf::from("/");
    }
    PathBuf::from(path)
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

    /// How many chevrons a breadcrumb render drew: each is two strokes, and
    /// nothing else in breadcrumb mode is a line.
    fn chevrons(cmds: &[RenderCommand]) -> usize {
        cmds.iter()
            .filter(|cmd| matches!(cmd, RenderCommand::Line { .. }))
            .count()
            / 2
    }

    /// The drawn labels of a trail, for tests about the split rather than
    /// about the bytes.
    fn labels(segments: &[Segment]) -> Vec<String> {
        segments.iter().map(|s| s.label.clone()).collect()
    }

    /// Build a trail from text components, for the rebuild tests.
    fn segs(parts: &[&str]) -> Vec<Segment> {
        parts.iter().map(|p| Segment::new(OsStr::new(p))).collect()
    }

    fn key_press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn key_press_with_text(key: Key, ch: char) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: ch.to_string(),
        }
    }

    /// **Shift-selection, which was implemented and never tested.**
    ///
    /// `key_press_shift` existed with no caller -- the crate-level
    /// `#![allow(dead_code)]` that hid the widget's unused-ness hid this too,
    /// and removing it when the file explorer became the widget's first real
    /// user is what surfaced it. Four call sites pass `event.modifiers.shift`
    /// into the cursor moves and nothing checked that any of them extend a
    /// selection.
    #[test]
    fn shift_and_a_cursor_key_select_what_it_moves_over() {
        let mut bar = PathBar::new("/home/user");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        assert!(bar.is_editing());

        // To the start, then select the lot on the way back.
        bar.handle_key_event(&key_press(Key::Home));
        assert_eq!(bar.selection_anchor, None, "Home alone selects nothing");
        bar.handle_key_event(&key_press_shift(Key::End));
        assert_eq!(
            bar.selection_anchor,
            Some(0),
            "Shift+End did not anchor a selection at the start"
        );

        // Typing replaces the selection rather than inserting into it.
        bar.handle_key_event(&key_press_with_text(Key::X, 'x'));
        assert_eq!(bar.edit_text, "x", "the selection was not replaced");
        assert_eq!(
            bar.selection_anchor, None,
            "the selection outlived the typing"
        );
    }

    /// And an unshifted move collapses a selection instead of extending it.
    #[test]
    fn a_cursor_key_without_shift_drops_the_selection() {
        let mut bar = PathBar::new("/home/user");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        bar.handle_key_event(&key_press(Key::Home));
        bar.handle_key_event(&key_press_shift(Key::End));
        assert!(bar.selection_anchor.is_some());

        bar.handle_key_event(&key_press(Key::Left));

        assert_eq!(
            bar.selection_anchor, None,
            "the selection survived a plain move"
        );
        bar.handle_key_event(&key_press_with_text(Key::X, 'x'));
        assert!(
            bar.edit_text.len() > 1,
            "typing after a collapsed selection replaced the text anyway: {:?}",
            bar.edit_text
        );
    }

    fn key_press_ctrl(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        }
    }

    fn key_press_shift(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::shift(),
            text: String::new(),
        }
    }

    // --- Path splitting tests ---

    #[test]
    fn test_split_path_root() {
        assert_eq!(labels(&split_path(OsStr::new("/"))), vec!["/"]);
    }

    #[test]
    fn test_split_path_simple() {
        assert_eq!(
            labels(&split_path(OsStr::new("/home/user/Documents"))),
            vec!["/", "home", "user", "Documents"]
        );
    }

    #[test]
    fn test_split_path_single_dir() {
        assert_eq!(labels(&split_path(OsStr::new("/usr"))), vec!["/", "usr"]);
    }

    #[test]
    fn test_split_path_empty() {
        assert_eq!(labels(&split_path(OsStr::new(""))), vec!["/"]);
    }

    #[test]
    fn test_split_path_relative() {
        assert_eq!(
            labels(&split_path(OsStr::new("home/user"))),
            vec!["home", "user"]
        );
    }

    // --- Path normalization tests ---

    #[test]
    fn test_normalize_double_slashes() {
        assert_eq!(
            normalize_path(OsStr::new("/home//user///docs")),
            Path::new("/home/user/docs")
        );
    }

    #[test]
    fn test_normalize_trailing_slash() {
        assert_eq!(
            normalize_path(OsStr::new("/home/user/")),
            Path::new("/home/user")
        );
    }

    #[test]
    fn test_normalize_root_trailing() {
        assert_eq!(normalize_path(OsStr::new("/")), Path::new("/"));
    }

    #[test]
    fn test_normalize_empty() {
        assert_eq!(normalize_path(OsStr::new("")), Path::new("/"));
    }

    #[test]
    fn test_normalize_multiple_trailing() {
        assert_eq!(
            normalize_path(OsStr::new("/home/user///")),
            Path::new("/home/user")
        );
    }

    // --- Breadcrumb rendering tests ---

    #[test]
    fn test_render_breadcrumb_segment_count() {
        let palette = Palette::for_mode(false);
        let mut bar = PathBar::new("/home/user/Documents");
        let cmds = bar.render(&palette, 800, 32);

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

        // Four crumbs: "/", "home", "user", "Documents", and a chevron
        // between each neighbouring pair.
        assert_eq!(text_cmds, vec!["/", "home", "user", "Documents"]);
        assert_eq!(chevrons(&cmds), 3);
    }

    #[test]
    fn test_render_breadcrumb_root_only() {
        let palette = Palette::for_mode(false);
        let mut bar = PathBar::new("/");
        let cmds = bar.render(&palette, 800, 32);

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
        // No chevrons for root-only.
        assert_eq!(chevrons(&cmds), 0);
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
        assert_eq!(bar.current_path(), Path::new("/home/user"));
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

        assert!(events.contains(&PathBarEvent::Navigate(PathBuf::from("/usr/local/bin"))));
        assert!(!bar.is_editing());
        assert_eq!(bar.current_path(), Path::new("/usr/local/bin"));
    }

    #[test]
    fn test_navigate_via_segment_click() {
        let palette = Palette::for_mode(false);
        let mut bar = PathBar::new("/home/user/Documents");
        // Render to lay the crumbs out.
        bar.render(&palette, 800, 32);

        // "home" is the second crumb, and stands for segment 1.
        let hit = bar.crumb_hits[1];
        assert_eq!(hit.segment, 1);
        let (sx, sy, sw, sh) = hit.rect;
        let click = MouseEvent {
            x: sx + sw / 2.0,
            y: sy + sh / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };

        let result = bar.handle_mouse_event(&click);
        assert_eq!(result, EventResult::Consumed);

        let events = bar.drain_events();
        assert!(events.contains(&PathBarEvent::Navigate(PathBuf::from("/home"))));
        assert_eq!(bar.current_path(), Path::new("/home"));
    }

    #[test]
    fn test_navigate_to_root_segment() {
        let palette = Palette::for_mode(false);
        let mut bar = PathBar::new("/home/user");
        bar.render(&palette, 800, 32);

        // Click on "/" (index 0).
        let (sx, sy, sw, sh) = bar.crumb_hits[0].rect;
        let click = MouseEvent {
            x: sx + sw / 2.0,
            y: sy + sh / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };

        bar.handle_mouse_event(&click);
        let events = bar.drain_events();
        assert!(events.contains(&PathBarEvent::Navigate(PathBuf::from("/"))));
        assert_eq!(bar.current_path(), Path::new("/"));
    }

    // --- Overflow tests ---

    #[test]
    fn test_overflow_rendering() {
        let palette = Palette::for_mode(false);
        let mut bar = PathBar::new("/very/long/path/with/many/segments/that/will/overflow");
        // Render at a narrow width to trigger overflow.
        let cmds = bar.render(&palette, 150, 32);

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
        let segments = segs(&["/", "home", "user", "Documents"]);

        assert_eq!(rebuild_path(&segments, 0), Path::new("/"));
        assert_eq!(rebuild_path(&segments, 1), Path::new("/home"));
        assert_eq!(rebuild_path(&segments, 2), Path::new("/home/user"));
        assert_eq!(
            rebuild_path(&segments, 3),
            Path::new("/home/user/Documents")
        );
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
        assert_eq!(bar.current_path(), Path::new("/new/path/here"));
        assert_eq!(labels(&bar.segments), vec!["/", "new", "path", "here"]);
    }

    // --- Click on empty area enters edit mode ---

    #[test]
    fn test_click_empty_area_enters_edit() {
        let palette = Palette::for_mode(false);
        let mut bar = PathBar::new("/home");
        bar.render(&palette, 800, 32);

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

    /// The rectangles the click handler tests against are the rectangles the
    /// names were drawn in: each crumb's name sits one padding inside its
    /// target, and the target spans the bar's height. The measuring pass and
    /// the drawing pass used to be two calculations of the same numbers,
    /// agreeing only for as long as both were edited together.
    #[test]
    fn every_segment_is_clickable_exactly_where_it_was_drawn() {
        let palette = Palette::for_mode(false);
        let mut bar = PathBar::new("/home/user/Documents");
        let cmds = bar.render(&palette, 800, 32);

        let names: Vec<(f32, &str, FontWeightHint)> = cmds
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text {
                    x,
                    text,
                    font_weight,
                    ..
                } => Some((*x, text.as_str(), *font_weight)),
                _ => None,
            })
            .collect();
        assert_eq!(names.len(), 4);
        assert_eq!(bar.crumb_hits.len(), 4);
        for (i, (hit, (text_x, label, weight))) in bar.crumb_hits.iter().zip(&names).enumerate() {
            assert_eq!(hit.segment, i, "crumb {i} stands for another segment");
            let (x, y, w, h) = hit.rect;
            assert!(
                (x + CRUMB_PADDING_H - text_x).abs() < 0.01,
                "{label} is not drawn where it is clicked"
            );
            assert!(
                (w - crumb_width(label, *weight)).abs() < 0.01,
                "{label}'s target is not its name and padding"
            );
            assert_eq!(
                (y, h),
                (0.0, 32.0),
                "{label}'s target is not the bar's height"
            );
        }
    }

    /// Consecutive crumbs are one chevron's slot apart -- the spacing the
    /// measuring pass charges for each join. A chevron is drawn *before* a
    /// crumb that has one in front of it rather than after a crumb that is not
    /// the last, and the two must lay out identically.
    #[test]
    fn neighbouring_segments_are_a_chevron_apart() {
        let palette = Palette::for_mode(false);
        let mut bar = PathBar::new("/home/user/Documents");
        bar.render(&palette, 800, 32);

        for pair in bar.crumb_hits.windows(2) {
            let (left, right) = (pair[0].rect, pair[1].rect);
            let expected = left.0 + left.2 + SEPARATOR_WIDTH;
            assert!(
                (right.0 - expected).abs() < 0.01,
                "crumb at {} should follow the one ending at {left:?}",
                right.0
            );
        }
    }

    /// The trail is laid out inside the width it was measured against: when
    /// nothing overflows, the first crumb starts at the field's left padding
    /// and the last ends within its right one.
    #[test]
    fn a_trail_that_fits_stays_inside_the_bar() {
        let palette = Palette::for_mode(false);
        let mut bar = PathBar::new("/home/user/Documents");
        let width = 800.0;
        bar.render(&palette, 800, 32);

        let last = bar
            .crumb_hits
            .last()
            .expect("four segments were drawn")
            .rect;
        assert!(last.0 + last.2 <= width - FIELD_PADDING_RIGHT);
        assert_eq!(
            bar.crumb_hits.first().map(|hit| hit.rect.0),
            Some(FIELD_PADDING_LEFT),
            "with no overflow the first crumb starts at the padding"
        );
    }

    /// When the trail overflows, the ellipsis is followed by one chevron and
    /// then only the segments that fit -- never a chevron with nothing on one
    /// side of it -- and the last of them still fits in the field.
    #[test]
    fn an_overflowing_trail_draws_one_separator_per_join() {
        let palette = Palette::for_mode(false);
        let mut bar = PathBar::new("/very/long/path/with/many/segments/that/will/overflow");
        let cmds = bar.render(&palette, 150, 32);

        let texts: Vec<&str> = cmds
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert!(texts.contains(&ELLIPSIS), "the dropped segments are marked");
        // One join per visible segment: each is preceded by the ellipsis or by
        // another segment.
        assert_eq!(chevrons(&cmds), bar.crumb_hits.len());
        let last = bar
            .crumb_hits
            .last()
            .expect("the current folder is drawn")
            .rect;
        assert!(
            last.0 + last.2 <= 150.0 - FIELD_PADDING_RIGHT,
            "the trail ran out of its field"
        );
        // And every crumb the measuring pass let in is drawn whole. The pass
        // charged each its chevron, so none has to be cut to fit; one that
        // under-charged would let in a crumb too many and squeeze the last --
        // which the cut above would otherwise quietly hide.
        assert!(
            cmds.iter().all(|cmd| !matches!(
                cmd,
                RenderCommand::Text {
                    max_width: Some(_),
                    ..
                }
            )),
            "a crumb was cut short: the measuring pass let in more than fits"
        );
    }

    /// **A crumb after the "..." goes to its own folder.** The drawn crumbs
    /// were numbered from the first one on screen, so once the trail
    /// overflowed, clicking the first crumb after the marker went to the root
    /// and every other crumb one folder too high.
    #[test]
    fn clicking_a_crumb_after_the_ellipsis_goes_to_that_folder() {
        let palette = Palette::for_mode(false);
        let long = "/very/long/path/with/many/segments/that/will/overflow";
        let mut bar = PathBar::new(long);
        bar.render(&palette, 150, 32);

        let first = bar.crumb_hits[0];
        assert!(
            first.segment > 0,
            "the trail did not overflow, so this proves nothing"
        );
        let expected = rebuild_path(&bar.segments, first.segment);
        let (x, y, w, h) = first.rect;
        bar.handle_mouse_event(&MouseEvent {
            x: x + w / 2.0,
            y: y + h / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        let events = bar.drain_events();
        assert_eq!(events, vec![PathBarEvent::Navigate(expected.clone())]);
        assert_ne!(expected, Path::new("/"));
        assert!(long.starts_with(expected.to_str().expect("a text path")));
    }

    /// **The folder you are in is on screen however narrow the bar**: a name
    /// too long for the field beside the marker is cut short with an ellipsis
    /// of its own, not dropped, and it is still the crumb a click reaches.
    #[test]
    fn a_folder_too_long_for_the_bar_is_cut_short_not_dropped() {
        let palette = Palette::for_mode(false);
        let leaf = "a-folder-whose-name-is-much-longer-than-the-bar-it-is-shown-in";
        let mut bar = PathBar::new(format!("/home/{leaf}"));
        let cmds = bar.render(&palette, 160, 32);

        let drawn = cmds.iter().find_map(|cmd| match cmd {
            RenderCommand::Text {
                text,
                max_width,
                overflow,
                ..
            } if text == leaf => Some((*max_width, *overflow)),
            _ => None,
        });
        let (max_width, overflow) = drawn.expect("the current folder was not drawn at all");
        assert!(max_width.is_some(), "the name was not cut to its room");
        assert_eq!(overflow, TextOverflow::Ellipsis, "the cut is not marked");
        let hit = bar
            .crumb_hits
            .last()
            .expect("the current folder has no target");
        assert_eq!(hit.segment, bar.segments.len() - 1);
        assert!(hit.rect.0 + hit.rect.2 <= 160.0 - FIELD_PADDING_RIGHT + 0.01);
    }

    /// A lone "/" in a field too narrow for it is cut short, not announced as
    /// a longer path with segments left out.
    #[test]
    fn a_lone_root_is_never_announced_as_a_longer_path() {
        let palette = Palette::for_mode(false);
        let mut bar = PathBar::new("/");
        let cmds = bar.render(&palette, 12, 32);
        let texts: Vec<&str> = cmds
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["/"]);
        assert_eq!(bar.crumb_hits.len(), 1);
    }

    /// **The folder you are in is set bold, the rest regular**, as the
    /// reference marks its current crumb; the names are ink and the chevrons
    /// between them the palette's quiet separator mark.
    #[test]
    fn the_current_folder_is_the_one_crumb_set_bold() {
        for light in [false, true] {
            let palette = Palette::for_mode(light);
            let mut bar = PathBar::new("/home/user/Documents");
            let cmds = bar.render(&palette, 800, 32);
            let weights: Vec<(&str, FontWeightHint, Color)> = cmds
                .iter()
                .filter_map(|cmd| match cmd {
                    RenderCommand::Text {
                        text,
                        font_weight,
                        color,
                        ..
                    } => Some((text.as_str(), *font_weight, *color)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                weights,
                vec![
                    ("/", FontWeightHint::Regular, palette.text),
                    ("home", FontWeightHint::Regular, palette.text),
                    ("user", FontWeightHint::Regular, palette.text),
                    ("Documents", FontWeightHint::Bold, palette.text),
                ],
                "light = {light}"
            );
            assert!(
                cmds.iter().all(|cmd| match cmd {
                    RenderCommand::Line { color, .. } => *color == palette.overlay0,
                    _ => true,
                }),
                "a chevron is not drawn in the separator colour, light = {light}"
            );
        }
    }

    /// **One field in both modes**: the input's well (`crust`) with a quiet
    /// edge, whether it shows the trail or the typed path -- and a red edge
    /// only while a typed path is known not to exist.
    #[test]
    fn the_field_is_an_inputs_well_in_both_modes() {
        let palette = Palette::for_mode(false);
        let field = |cmds: &[RenderCommand]| -> (Color, Color) {
            let fill = cmds.iter().find_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if (*x, *y, *width, *height) == (0.0, 0.0, 400.0, 28.0) => Some(*color),
                _ => None,
            });
            let edge = cmds.iter().find_map(|cmd| match cmd {
                RenderCommand::StrokeRect { color, .. } => Some(*color),
                _ => None,
            });
            (
                fill.expect("no field was drawn"),
                edge.expect("the field has no edge"),
            )
        };

        let mut bar = PathBar::new("/home/user");
        assert_eq!(
            field(&bar.render(&palette, 400, 28)),
            (palette.crust, palette.surface1)
        );
        bar.handle_key_event(&key_press_ctrl(Key::L));
        assert!(bar.is_editing());
        let editing = bar.render(&palette, 400, 28);
        assert_eq!(field(&editing), (palette.crust, palette.surface1));
        // Nothing but the field is a box the width of the bar -- its fill and
        // its edge. The inner card edit mode used to draw over the field,
        // outlined or filled depending on the theme, is gone.
        let boxes = editing
            .iter()
            .filter_map(crate::surface::logical_rect)
            .filter(|(_, _, width, _)| *width >= 390.0)
            .count();
        assert_eq!(
            boxes, 2,
            "a second box is drawn over the field while editing"
        );
        bar.set_path_valid(false);
        assert_eq!(field(&bar.render(&palette, 400, 28)).1, palette.red);
    }

    /// **A click in the typed path puts the caret where it was aimed.** The
    /// text was drawn four pixels further right than the click was measured
    /// from, so a click just past a narrow character landed after the next.
    #[test]
    fn a_click_in_the_typed_path_puts_the_caret_where_it_was_aimed() {
        let palette = Palette::for_mode(false);
        let mut bar = PathBar::new("/home/user/lib");
        bar.handle_key_event(&key_press_ctrl(Key::L));
        let cmds = bar.render(&palette, 400, 28);
        let drawn_at = cmds
            .iter()
            .find_map(|cmd| match cmd {
                RenderCommand::Text { x, text, .. } if text == "/home/user/lib" => Some(*x),
                _ => None,
            })
            .expect("the typed path was not drawn");
        assert_eq!(drawn_at, EDIT_TEXT_X);

        // Just past each boundary: before the narrow "/" and "l" and "i",
        // where half a character's error is enough to land on the next one.
        for boundary in [5, 10, 11, 12] {
            let at = crate::text::caret_x(
                &bar.edit_text,
                TextCursor::from(boundary),
                FONT_SIZE,
                FontWeightHint::Regular,
            );
            bar.handle_mouse_event(&MouseEvent {
                x: drawn_at + at + 0.5,
                y: 14.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            });
            assert_eq!(
                bar.cursor.byte, boundary,
                "a click just past byte {boundary}"
            );
        }
    }

    /// An index past the last segment names the whole trail. It reaches
    /// `rebuild_path` from a click test against stale `crumb_hits`, so it
    /// must not be a panic.
    #[test]
    fn rebuilding_past_the_end_yields_the_whole_path() {
        let segments = segs(&["/", "home", "user"]);
        assert_eq!(rebuild_path(&segments, 2), Path::new("/home/user"));
        assert_eq!(rebuild_path(&segments, 99), Path::new("/home/user"));
        assert_eq!(rebuild_path(&segments, usize::MAX), Path::new("/home/user"));
        assert_eq!(rebuild_path(&[], 0), Path::new("/"));
        assert_eq!(rebuild_path(&[], usize::MAX), Path::new("/"));
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

    // --- A path that is not text ---
    //
    // `design.txt` allows every byte in a name but `/` and NUL, so a folder
    // can have a name with no text form at all. These pin that such a name
    // survives the widget rather than being flattened to its replacement-
    // character rendering and navigated to as a different path.

    /// A name with no text form round-trips through the breadcrumb.
    #[cfg(windows)]
    #[test]
    fn a_path_that_is_not_text_survives_the_breadcrumb() {
        use std::os::windows::ffi::OsStringExt;

        let odd = PathBuf::from(OsString::from_wide(&[
            u16::from(b'/'),
            u16::from(b'z'),
            0xD800,
        ]));
        let bar = PathBar::new(&odd);

        assert_eq!(
            bar.current_path(),
            odd.as_path(),
            "the address bar changed a path it was only asked to display"
        );
    }

    /// The crumb is drawn escaped and navigated to exactly.
    ///
    /// Both halves matter: a crumb is text, so the label *must* be a rendering
    /// -- the lone surrogate as its three bytes in octal; the click target is a
    /// path, so `exact` must not be.
    #[cfg(windows)]
    #[test]
    fn a_crumb_is_drawn_escaped_and_navigated_to_exactly() {
        use std::os::windows::ffi::OsStringExt;

        let name = OsString::from_wide(&[u16::from(b'z'), 0xD800]);
        let odd = PathBuf::from(OsString::from_wide(&[
            u16::from(b'/'),
            u16::from(b'z'),
            0xD800,
        ]));
        let bar = PathBar::new(&odd);

        let last = bar.segments.last().expect("the trail has a leaf");
        assert_eq!(
            last.label, r"z\355\240\200",
            "the crumb is not drawn as the terminal would spell the name"
        );
        assert_eq!(
            last.exact, name,
            "the click target was rebuilt from the drawn text"
        );
    }

    /// Confirming an address the user never touched navigates to the bytes.
    ///
    /// The field itself has to be a `String` -- it is a text input with a
    /// caret. That makes it a rendering, so confirming it verbatim would
    /// navigate to a path that merely *looks* like the one displayed. The
    /// widget keeps the original beside it and prefers it while the text still
    /// reads as the rendering of those bytes, which is how
    /// `RunDialog::command_exact` already distinguishes "unedited" from
    /// "typed".
    #[cfg(windows)]
    #[test]
    fn confirming_an_untouched_address_navigates_to_the_bytes() {
        use std::os::windows::ffi::OsStringExt;

        let odd = PathBuf::from(OsString::from_wide(&[
            u16::from(b'/'),
            u16::from(b'z'),
            0xD800,
        ]));
        let mut bar = PathBar::new(&odd);
        bar.enter_edit_mode();

        assert_eq!(
            bar.edit_text, r"/z\355\240\200",
            "the field does not show the name as the terminal would spell it"
        );

        bar.navigate_to_edit_text();
        let events = bar.drain_events();
        assert!(
            events.contains(&PathBarEvent::Navigate(odd.clone())),
            "an address bar nobody typed into navigated somewhere else: {events:?}"
        );
    }

    /// Typing makes the text the source of truth again.
    ///
    /// The other side of the rule above: keeping the original bytes must not
    /// mean ignoring what the user actually asked for.
    #[test]
    fn typing_an_address_makes_the_text_the_source_of_truth() {
        let mut bar = PathBar::new("/home/user");
        bar.enter_edit_mode();
        bar.edit_text = "/etc".to_string();
        bar.navigate_to_edit_text();

        let events = bar.drain_events();
        assert!(
            events.contains(&PathBarEvent::Navigate(PathBuf::from("/etc"))),
            "the typed address was discarded: {events:?}"
        );
    }
}
