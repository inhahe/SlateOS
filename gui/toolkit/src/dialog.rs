//! File picker and save dialog component.
//!
//! Provides a reusable file open/save dialog that applications use to let users
//! browse the filesystem, select files or folders, and specify save locations.
//! Renders using `RenderCommand` primitives with a Catppuccin Mocha dark theme.
//!
//! # Usage
//!
//! ```no_run
//! use guitk::dialog::FileDialog;
//!
//! // Open dialog with Rust file filter
//! let mut dialog = FileDialog::open()
//!     .with_filter("Rust files", &["*.rs"])
//!     .with_filter("All files", &["*"])
//!     .with_initial_path("/home/user/projects");
//!
//! // Save dialog with a default filename
//! let mut dialog = FileDialog::save()
//!     .with_filter("Text files", &["*.txt"])
//!     .with_filename("untitled.txt");
//! ```
//!
//! # Driving it
//!
//! Forward keys to [`FileDialog::handle_event`] and pointer events to
//! [`FileDialog::handle_mouse`]; both answer with a [`DialogAction`]. Forward
//! *every* pointer event while the dialog is up, including ones landing outside
//! its edges — it swallows those deliberately, and that is what makes it modal.
//!
//! The dialog does no I/O of its own: it shows whatever listing it was handed,
//! so a [`DialogAction::NavigatedTo`] is a *request* for a fresh one, answered
//! with [`FileDialog::set_entries`]. Leaving one unanswered puts the previous
//! directory's files on screen under the new directory's name.
//!
//! ## The dialog stores no size
//!
//! [`render`](FileDialog::render) is *given* a width and height and cannot
//! write anything back, so a size kept on the widget would be a second answer
//! to how big the dialog is, free to disagree with the one it was last drawn
//! at. It keeps none — which is why the two handlers take the size too. Pass
//! them the size of the most recent `render`, or clicks will be tested against
//! a layout the user is not looking at.
//!
//! ## Hit-testing
//!
//! [`FileDialog::frame`] is the single walk that both draws the dialog and
//! records where each control landed, as a [`Frame<DialogTarget>`](crate::frame::Frame);
//! `render` is a thin wrapper over it. Hosts that only draw need nothing new,
//! and hosts that want to name a control themselves — a test, usually — can ask
//! the frame where it is rather than recomputing it. Recomputing row geometry
//! outside this module is the one thing not to do: it is a second copy of the
//! layout, and the bug then lives in whichever copy you are not reading.

use crate::color::Color;
use crate::date::Date;
use crate::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::{Frame, Rect};
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::scroll_window;
use crate::style::CornerRadii;
use crate::wheel;
use core::ops::Range;
pub use tzrules::Tz;

// --- Catppuccin Mocha palette ---

/// Base background (dialog body).
const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
/// Slightly raised surface (sidebar, toolbar).
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
/// Higher surface (selected items, input fields).
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
/// Overlay / hover highlights.
const COLOR_SURFACE2: Color = Color::from_hex(0x585B70);
/// Primary text.
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
/// Subdued text (secondary labels, sizes, dates).
const COLOR_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
/// Accent color (selection highlight, primary buttons).
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
/// Accent for folders.
const COLOR_YELLOW: Color = Color::from_hex(0xF9E2AF);
/// Disabled / muted elements.
const COLOR_OVERLAY: Color = Color::from_hex(0x6C7086);
/// Error / cancel accent.
const COLOR_RED: Color = Color::from_hex(0xF38BA8);

// --- Layout constants ---

const TOOLBAR_HEIGHT: f32 = 40.0;
const SIDEBAR_WIDTH: f32 = 160.0;
const BOTTOM_BAR_HEIGHT: f32 = 50.0;
const ROW_HEIGHT: f32 = 28.0;
const PADDING: f32 = 8.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 11.0;
const BUTTON_WIDTH: f32 = 80.0;
const BUTTON_HEIGHT: f32 = 30.0;
const CORNER_RADIUS: f32 = 4.0;
/// Width of the file list's scrollbar, when the list is long enough to have one.
const SCROLLBAR_WIDTH: f32 = 10.0;
/// Shortest the scrollbar thumb may get.
///
/// A thumb sized strictly in proportion to the visible fraction of a very long
/// listing shrinks to a couple of pixels, which is both invisible and too small
/// to grab. Every real scrollbar imposes a floor for the same reason; the cost
/// is that the thumb's *size* stops being a faithful proportion once the list is
/// long, which nobody reads it for, while its *position* stays exact.
const MIN_THUMB_HEIGHT: f32 = 20.0;

// --- Public types ---

/// Mode of operation for the file dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogMode {
    /// User is opening/selecting one or more files.
    Open,
    /// User is choosing where to save a file.
    Save,
    /// User is selecting a folder (not a file).
    SelectFolder,
}

/// One entry in the current directory listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    /// Display name (file or directory name, not full path).
    pub name: String,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// File size in bytes (0 for directories).
    pub size: u64,
    /// Last-modified timestamp (Unix epoch seconds).
    pub modified_timestamp: u64,
    /// File extension (without the dot), empty for dirs/extensionless.
    pub extension: String,
}

/// A file type filter (e.g. "Rust files" matching `*.rs`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFilter {
    /// Human-readable description shown in the filter dropdown.
    pub description: String,
    /// Glob patterns (e.g. `["*.rs", "*.toml"]`).
    pub patterns: Vec<String>,
}

/// Column used for sorting the file list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Size,
    Modified,
}

/// Quick-access sidebar location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickAccess {
    /// Display label (e.g. "Home").
    pub label: String,
    /// Absolute path this entry navigates to.
    pub path: String,
}

/// Result of an action on the dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogAction {
    /// Nothing happened (event was consumed but state unchanged meaningfully).
    None,
    /// Dialog navigated to a new directory.
    NavigatedTo(String),
    /// User confirmed a selection (path to the selected file/folder).
    Selected(String),
    /// User cancelled the dialog.
    Cancelled,
}

/// A part of a [`FileDialog`] that a click can land on.
///
/// Recorded by [`FileDialog::frame`] as it draws, so that what a click reaches
/// and what the user sees come out of the same walk. See [`crate::frame`] for
/// why that matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogTarget {
    /// The `<` button: back through the navigation history.
    Back,
    /// The `>` button: forward again.
    Forward,
    /// The `^` button: to the parent directory.
    Up,
    /// The path display in the toolbar.
    ///
    /// Recorded, but clicking it does nothing yet: editing a path by hand needs
    /// a text field with a caret, and the dialog has none. Recording it anyway
    /// means a click there is *swallowed* rather than falling through to the
    /// list behind, which is what the user expects of a control they can see.
    AddressBar,
    /// A quick-access shortcut in the sidebar, by position in the sidebar list.
    Shortcut(usize),
    /// A column header in the file list, which sorts by that column.
    Header(SortColumn),
    /// A row of the file list, by index into [`FileDialog::entries`].
    ///
    /// An index rather than a stable id — which is what [`crate::frame`]
    /// recommends for rows — because the frame a click is tested against is
    /// built from the listing as it stands at that moment, so there is no
    /// interval in which the two could disagree. A `DirEntry` has no id to use
    /// instead; its name would be one, at the cost of a `String` per row per
    /// frame.
    Entry(usize),
    /// The empty part of the file list, below the last row.
    List,
    /// The scrollbar's track.
    ScrollTrack,
    /// The scrollbar's thumb.
    ScrollThumb,
    /// The filename box in the bottom bar (Save mode only).
    ///
    /// Like [`AddressBar`](Self::AddressBar): recorded so the click stops here,
    /// not acted on. Typing goes to the box already, since Save mode has nowhere
    /// else for a keystroke to go.
    FilenameInput,
    /// The Open/Save/Select button.
    Confirm,
    /// The Cancel button.
    Cancel,
    /// Dialog background: no control, but inside the dialog.
    ///
    /// The distinction from "no target at all" is what tells a host whether the
    /// click was *outside* the dialog, which is the click a modal has to refuse
    /// to pass through.
    Chrome,
}

/// File open/save/folder-select dialog.
///
/// Maintains internal state for navigation, selection, and input. Call
/// [`handle_event`](Self::handle_event) to feed keyboard events and
/// [`render`](Self::render) to produce the draw commands each frame.
#[derive(Clone, Debug)]
pub struct FileDialog {
    mode: DialogMode,
    current_path: String,
    entries: Vec<DirEntry>,
    selected_index: Option<usize>,
    filename_input: String,
    filters: Vec<FileFilter>,
    active_filter_index: usize,
    show_hidden: bool,
    sort_by: SortColumn,
    sort_ascending: bool,
    history_back: Vec<String>,
    history_forward: Vec<String>,
    quick_access: Vec<QuickAccess>,
    cancelled: bool,
    /// Index of the first entry row drawn.
    ///
    /// A request rather than a fact: it is clamped against the height the
    /// dialog turns out to be drawn at, so a listing that shrank under a stale
    /// offset shows its last page rather than blank space. See
    /// [`scroll_window`], whose policy this is.
    ///
    /// The dialog stores no size of its own — [`render`](Self::render) is given
    /// one and cannot write anything back — which is why every method that has
    /// to move this takes the height as an argument.
    scroll_top: usize,
    /// Wheel fractions earned but not yet spent, so a high-resolution wheel or
    /// a trackpad scrolls smoothly instead of discarding everything under one
    /// notch.
    wheel: wheel::Accumulator,
    /// While the scrollbar thumb is being dragged, how far below the thumb's
    /// top edge the pointer grabbed it.
    ///
    /// Kept so the thumb does not jump under the pointer on the first drag
    /// event: the thumb follows the grab point, not the pointer.
    thumb_grab: Option<f32>,
    /// The zone the Modified column is rendered in.
    ///
    /// Defaults to UTC because a toolkit has no business reading `TZ` behind
    /// its embedder's back — the shell resolves the zone once (through
    /// zoneinfo, which a bare `TZ` string cannot express) and hands the same
    /// [`Tz`] to everything that shows a time, so the dialog and the taskbar
    /// clock cannot disagree.
    timezone: Tz,
}

impl FileDialog {
    // --- Constructors (builder pattern) ---

    /// Create a new file-open dialog.
    pub fn open() -> Self {
        Self::new(DialogMode::Open)
    }

    /// Create a new file-save dialog.
    pub fn save() -> Self {
        Self::new(DialogMode::Save)
    }

    /// Create a new folder-selection dialog.
    pub fn select_folder() -> Self {
        Self::new(DialogMode::SelectFolder)
    }

    /// Add a file type filter. The "All files (*)" filter is always appended
    /// automatically if not already present.
    #[must_use]
    pub fn with_filter(mut self, description: &str, patterns: &[&str]) -> Self {
        self.filters.push(FileFilter {
            description: description.to_string(),
            patterns: patterns.iter().map(|p| (*p).to_string()).collect(),
        });
        self
    }

    /// Set the initial directory the dialog opens to.
    #[must_use]
    pub fn with_initial_path(mut self, path: &str) -> Self {
        self.current_path = path.to_string();
        self
    }

    /// Pre-fill the filename input (useful for Save mode).
    #[must_use]
    pub fn with_filename(mut self, name: &str) -> Self {
        self.filename_input = name.to_string();
        self
    }

    /// Toggle display of hidden files (files starting with `.`).
    #[must_use]
    pub fn show_hidden(mut self, show: bool) -> Self {
        self.show_hidden = show;
        self
    }

    /// Render the Modified column in `zone` rather than UTC.
    #[must_use]
    pub fn with_timezone(mut self, zone: Tz) -> Self {
        self.timezone = zone;
        self
    }

    /// Change the zone the Modified column is rendered in.
    ///
    /// Separate from [`with_timezone`](Self::with_timezone) because the zone
    /// can change while a dialog is open — the user can edit it in Settings —
    /// and a dialog consumed by a builder cannot be told.
    pub fn set_timezone(&mut self, zone: Tz) {
        self.timezone = zone;
    }

    /// The zone the Modified column is rendered in.
    #[must_use]
    pub fn timezone(&self) -> Tz {
        self.timezone
    }

    // --- Navigation ---

    /// Navigate into the given directory path. Pushes the current path onto the
    /// back-history stack.
    pub fn navigate_to(&mut self, path: &str) {
        if path == self.current_path {
            return;
        }
        self.history_back.push(self.current_path.clone());
        self.history_forward.clear();
        self.current_path = path.to_string();
        self.rewind();
    }

    /// Navigate to the parent directory.
    pub fn navigate_up(&mut self) {
        let parent = parent_path(&self.current_path);
        if parent != self.current_path {
            self.navigate_to(&parent);
        }
    }

    /// Navigate backward in history (if available).
    pub fn navigate_back(&mut self) {
        if let Some(prev) = self.history_back.pop() {
            self.history_forward.push(self.current_path.clone());
            self.current_path = prev;
            self.rewind();
        }
    }

    /// Navigate forward in history (if available).
    pub fn navigate_forward(&mut self) {
        if let Some(next) = self.history_forward.pop() {
            self.history_back.push(self.current_path.clone());
            self.current_path = next;
            self.rewind();
        }
    }

    /// Forget where the *previous* listing was looking.
    ///
    /// Called by every navigation. A scroll position and a selected row are
    /// both indices into the listing that is about to be replaced, so carrying
    /// either into the new directory would pick an unrelated file, or open at
    /// row 40 of a directory with three files in it. The wheel's banked
    /// fraction goes too, so a fraction earned in one directory cannot deliver
    /// a row in the next.
    fn rewind(&mut self) {
        self.selected_index = None;
        self.scroll_top = 0;
        self.wheel.reset();
        self.thumb_grab = None;
    }

    // --- Selection / Interaction ---

    /// Highlight the entry at `index` (single-click equivalent).
    pub fn select_entry(&mut self, index: usize) {
        if index < self.entries.len() {
            self.selected_index = Some(index);
            // In save mode, clicking a file fills the filename input.
            if self.mode == DialogMode::Save
                && let Some(entry) = self.entries.get(index)
                && !entry.is_dir
            {
                self.filename_input = entry.name.clone();
            }
        }
    }

    /// Activate (double-click/Enter) the entry at `index`.
    ///
    /// - If it is a directory, navigates into it.
    /// - If it is a file (and mode is Open), returns `DialogAction::Selected`.
    /// - In `SelectFolder` mode, double-clicking a dir selects it.
    pub fn activate_entry(&mut self, index: usize) -> DialogAction {
        let entry = match self.entries.get(index) {
            Some(e) => e.clone(),
            None => return DialogAction::None,
        };

        if entry.is_dir {
            if self.mode == DialogMode::SelectFolder {
                let full = join_path(&self.current_path, &entry.name);
                return DialogAction::Selected(full);
            }
            let target = join_path(&self.current_path, &entry.name);
            self.navigate_to(&target);
            DialogAction::NavigatedTo(self.current_path.clone())
        } else {
            match self.mode {
                DialogMode::Open => {
                    let full = join_path(&self.current_path, &entry.name);
                    DialogAction::Selected(full)
                }
                DialogMode::Save => {
                    // Double-clicking a file in save mode fills the name input.
                    self.filename_input = entry.name.clone();
                    DialogAction::None
                }
                DialogMode::SelectFolder => {
                    // Cannot select a file in folder mode.
                    DialogAction::None
                }
            }
        }
    }

    /// Set the filename input text (Save mode).
    pub fn set_filename(&mut self, name: &str) {
        self.filename_input = name.to_string();
    }

    /// Change the active file type filter by index.
    pub fn set_filter_index(&mut self, index: usize) {
        let max_index = self.effective_filters().len().saturating_sub(1);
        if index <= max_index {
            self.active_filter_index = index;
        }
    }

    /// Attempt to confirm the current selection. Returns `Some(path)` if a
    /// valid selection exists, or `None` if confirmation is not possible.
    pub fn confirm(&self) -> Option<String> {
        match self.mode {
            DialogMode::Open => {
                let idx = self.selected_index?;
                let entry = self.entries.get(idx)?;
                if entry.is_dir {
                    return None;
                }
                Some(join_path(&self.current_path, &entry.name))
            }
            DialogMode::Save => {
                if self.filename_input.is_empty() {
                    return None;
                }
                let name = self.filename_with_extension();
                Some(join_path(&self.current_path, &name))
            }
            DialogMode::SelectFolder => {
                // In folder mode, confirming selects the current directory
                // or the highlighted directory entry.
                if let Some(idx) = self.selected_index
                    && let Some(entry) = self.entries.get(idx)
                    && entry.is_dir
                {
                    return Some(join_path(&self.current_path, &entry.name));
                }
                // Fall back to current directory itself.
                Some(self.current_path.clone())
            }
        }
    }

    /// Cancel the dialog.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Handle a keyboard event. Returns the resulting action.
    ///
    /// `height` is the height the dialog is being drawn at — the same number
    /// [`render`](Self::render) is given. Moving the selection has to scroll the
    /// list to keep the selection on screen, and how many rows are on screen
    /// depends on the height; the dialog deliberately stores no size of its own,
    /// because a stored size is a second answer to "how big is this dialog"
    /// that can disagree with the one the renderer is using.
    pub fn handle_event(&mut self, event: &KeyEvent, height: f32) -> DialogAction {
        if !event.pressed {
            return DialogAction::None;
        }

        match event.key {
            Key::Escape => {
                self.cancel();
                DialogAction::Cancelled
            }
            Key::Enter => {
                // If an entry is selected, activate it; otherwise confirm.
                if let Some(idx) = self.selected_index {
                    let entry_is_dir = self.entries.get(idx).map(|e| e.is_dir).unwrap_or(false);
                    if entry_is_dir || self.mode == DialogMode::Open {
                        return self.activate_entry(idx);
                    }
                }
                // Attempt confirm (primarily for Save mode with filename input).
                match self.confirm() {
                    Some(path) => DialogAction::Selected(path),
                    None => DialogAction::None,
                }
            }
            Key::Up => {
                self.move_selection(-1, height);
                DialogAction::None
            }
            Key::Down => {
                self.move_selection(1, height);
                DialogAction::None
            }
            Key::PageUp => {
                // Saturating rather than plain `-`: negating `isize::MIN` is
                // an overflow, and `page_step` is free to return whatever a
                // window height implies. Saturation is exact for every value
                // it can actually produce, so this is a guard, not a rounding.
                self.move_selection(page_step(height).saturating_neg(), height);
                DialogAction::None
            }
            Key::PageDown => {
                self.move_selection(page_step(height), height);
                DialogAction::None
            }
            Key::Backspace if event.modifiers.alt => self.navigated(Self::navigate_back),
            Key::Backspace => {
                // Without modifiers in non-save mode: go to parent.
                if self.mode != DialogMode::Save || self.filename_input.is_empty() {
                    self.navigated(Self::navigate_up)
                } else {
                    // In save mode with text: delete last char of filename input.
                    self.filename_input.pop();
                    DialogAction::None
                }
            }
            Key::Home => {
                if !self.entries.is_empty() {
                    self.selected_index = Some(0);
                    self.reveal(height);
                }
                DialogAction::None
            }
            Key::End => {
                if !self.entries.is_empty() {
                    self.selected_index = Some(self.entries.len().saturating_sub(1));
                    self.reveal(height);
                }
                DialogAction::None
            }
            _ => {
                // Text input for save-mode filename.
                if self.mode == DialogMode::Save {
                    self.filename_input.extend(event.typed());
                }
                DialogAction::None
            }
        }
    }

    // --- Rendering ---

    /// Produce render commands for the entire dialog at the given dimensions.
    ///
    /// A thin wrapper over [`frame`](Self::frame), which is the same walk with
    /// the click targets kept. Callers that only paint can keep using this.
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        self.frame(width, height).into_tree().commands
    }

    /// Draw the dialog at the given dimensions, recording what each part of it
    /// can be clicked to reach.
    ///
    /// One walk produces both the ink and the hit boxes, so a control cannot be
    /// drawn in one place and clicked in another — see [`crate::frame`]. This is
    /// also what [`handle_mouse`](Self::handle_mouse) tests a click against, so
    /// there is no second copy of the layout to keep in step with this one.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame<DialogTarget> {
        let mut frame = Frame::new(width, height);

        // Dialog background. Recorded as a target so that a host drawing the
        // dialog inset can tell a click on the dialog from a click past its
        // edge — the click a modal must not let through.
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        frame.hit(DialogTarget::Chrome, Rect::new(0.0, 0.0, width, height));

        // Toolbar
        self.draw_toolbar(&mut frame, width);

        // Sidebar. Clamped at zero because a dialog shorter than its own
        // furniture would otherwise hand a negative height to the clip stack,
        // and a negative-height clip is not a small one — it is one whose
        // bottom edge is above its top.
        let content_top = TOOLBAR_HEIGHT;
        let content_height = (height - TOOLBAR_HEIGHT - BOTTOM_BAR_HEIGHT).max(0.0);
        self.draw_sidebar(&mut frame, content_top, content_height);

        // File list
        let list_x = SIDEBAR_WIDTH;
        let list_width = (width - SIDEBAR_WIDTH).max(0.0);
        self.draw_file_list(
            &mut frame,
            list_x,
            content_top,
            list_width,
            content_height,
            height,
        );

        // Bottom bar (filename input for save, buttons)
        let bottom_y = height - BOTTOM_BAR_HEIGHT;
        self.draw_bottom_bar(&mut frame, bottom_y, width);

        frame
    }

    /// Handle a mouse event. Returns the resulting action.
    ///
    /// `width` and `height` are the dimensions the dialog is drawn at — the
    /// same numbers [`render`](Self::render) is given — because the click is
    /// tested against a frame laid out at that size. A host that passes a
    /// different size here than it draws at will find its clicks landing
    /// somewhere else, which is the one way this can go wrong and is why the
    /// dialog will not keep a second copy of the size for itself.
    ///
    /// Every mouse event should be forwarded while the dialog is up, not only
    /// presses: a drag of the scrollbar is a press, a run of moves and a
    /// release, and a dialog that only sees the press cannot follow the thumb.
    pub fn handle_mouse(&mut self, event: &MouseEvent, width: f32, height: f32) -> DialogAction {
        let frame = self.frame(width, height);
        let target = frame.hit_test(event.x, event.y);

        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => self.press(&frame, target, event.y, height),
            MouseEventKind::Release(MouseButton::Left) => {
                self.thumb_grab = None;
                DialogAction::None
            }
            MouseEventKind::Move => {
                self.drag_thumb(&frame, event.y, height);
                DialogAction::None
            }
            MouseEventKind::DoubleClick(MouseButton::Left) => match target {
                // Only rows act on the second click. Every other control has
                // already acted on the press that came before it, and a Cancel
                // button that cancelled twice would be no worse but a Back
                // button that went back twice would be wrong.
                Some(DialogTarget::Entry(index)) => {
                    self.select_entry(index);
                    self.activate_entry(index)
                }
                _ => DialogAction::None,
            },
            MouseEventKind::Scroll { dy, .. } => {
                let rows = self.wheel.rows(dy);
                // Start from where the list is actually looking, not from the
                // stored request: after the keyboard has scrolled by revealing a
                // selection, or after the list shrank, the two differ, and
                // scrolling from the stale one makes the first notch jump.
                self.scroll_top = self.visible_rows(height).start;
                self.scroll_top = scroll_window::shift(self.scroll_top, rows);
                DialogAction::None
            }
            _ => DialogAction::None,
        }
    }

    /// Act on a left-button press on `target`.
    fn press(
        &mut self,
        frame: &Frame<DialogTarget>,
        target: Option<DialogTarget>,
        y: f32,
        height: f32,
    ) -> DialogAction {
        match target {
            Some(DialogTarget::Back) => self.navigated(Self::navigate_back),
            Some(DialogTarget::Forward) => self.navigated(Self::navigate_forward),
            Some(DialogTarget::Up) => self.navigated(Self::navigate_up),
            Some(DialogTarget::Shortcut(index)) => {
                let Some(path) = self.quick_access.get(index).map(|qa| qa.path.clone()) else {
                    return DialogAction::None;
                };
                self.navigated(|dialog| dialog.navigate_to(&path))
            }
            Some(DialogTarget::Header(column)) => {
                self.toggle_sort(column);
                DialogAction::None
            }
            Some(DialogTarget::Entry(index)) => {
                self.select_entry(index);
                DialogAction::None
            }
            Some(DialogTarget::ScrollThumb) => {
                if let Some(thumb) = frame.rect_of(|t| *t == DialogTarget::ScrollThumb) {
                    self.thumb_grab = Some(y - thumb.y);
                }
                DialogAction::None
            }
            Some(DialogTarget::ScrollTrack) => {
                // A click on the track moves one windowful towards the click,
                // which is what every scrollbar does and is more predictable
                // than jumping to the exact spot: the thumb ends up under the
                // pointer either way if you keep clicking.
                let page = usize::try_from(page_step(height)).unwrap_or(1);
                let above = frame
                    .rect_of(|t| *t == DialogTarget::ScrollThumb)
                    .is_some_and(|thumb| y < thumb.y);
                self.scroll_top = self.visible_rows(height).start;
                self.scroll_top = if above {
                    self.scroll_top.saturating_sub(page)
                } else {
                    self.scroll_top.saturating_add(page)
                };
                DialogAction::None
            }
            Some(DialogTarget::Confirm) => match self.confirm() {
                Some(path) => DialogAction::Selected(path),
                None => DialogAction::None,
            },
            Some(DialogTarget::Cancel) => {
                self.cancel();
                DialogAction::Cancelled
            }
            // Inside the dialog but on nothing that acts: swallowed, because a
            // click the user aimed at the dialog must not reach whatever is
            // behind it. `None` — outside the dialog entirely — is swallowed
            // the same way, and for the same reason: a modal that let the
            // window behind it be clicked would only look modal.
            _ => DialogAction::None,
        }
    }

    /// Follow the scrollbar thumb while it is being dragged.
    ///
    /// Reads the track and thumb out of the frame that was just drawn rather
    /// than recomputing where they are. Two answers to "where is the
    /// scrollbar" is exactly the divergence [`crate::frame`] exists to prevent,
    /// and a drag handler is where it would show: the thumb would follow the
    /// pointer at an offset that grew with the window size.
    fn drag_thumb(&mut self, frame: &Frame<DialogTarget>, y: f32, height: f32) {
        let Some(grab) = self.thumb_grab else { return };
        let (Some(track), Some(thumb)) = (
            frame.rect_of(|t| *t == DialogTarget::ScrollTrack),
            frame.rect_of(|t| *t == DialogTarget::ScrollThumb),
        ) else {
            // The list got short enough to lose its scrollbar mid-drag.
            self.thumb_grab = None;
            return;
        };
        let span = track.h - thumb.h;
        let hidden = self
            .entries
            .len()
            .saturating_sub(Self::row_capacity(height));
        if span <= 0.0 || hidden == 0 {
            return;
        }
        let fraction = ((y - grab - track.y) / span).clamp(0.0, 1.0);
        self.scroll_top = (fraction * hidden as f32).round() as usize;
    }

    // --- Queries ---

    /// The current directory being displayed.
    pub fn current_path(&self) -> &str {
        &self.current_path
    }

    /// The dialog mode.
    pub fn mode(&self) -> DialogMode {
        self.mode
    }

    /// The currently selected index, if any.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Current entries in the directory listing.
    pub fn entries(&self) -> &[DirEntry] {
        &self.entries
    }

    /// Whether the dialog has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Set the directory entries (typically called after an async filesystem read
    /// populates the listing). Entries are sorted according to current sort settings.
    pub fn set_entries(&mut self, mut entries: Vec<DirEntry>) {
        // Filter hidden files unless show_hidden is set.
        if !self.show_hidden {
            entries.retain(|e| !e.name.starts_with('.'));
        }

        // Filter by extension in Open/Save modes (not folder mode).
        if self.mode != DialogMode::SelectFolder {
            let filters = self.effective_filters();
            if let Some(filter) = filters.get(self.active_filter_index) {
                let dominated_by_all = filter.patterns.iter().any(|p| p == "*" || p == "*.*");
                if !dominated_by_all {
                    let patterns: Vec<&str> = filter.patterns.iter().map(|s| s.as_str()).collect();
                    entries.retain(|e| e.is_dir || matches_any_pattern(&e.name, &patterns));
                }
            }
        }

        self.sort_entries(&mut entries);

        self.entries = entries;
        // A new listing is a new set of rows, so a scroll position or a
        // selection into the old one means nothing.
        self.rewind();
    }

    /// Toggle sort column. If already sorting by this column, flip direction.
    ///
    /// Re-orders the listing already on screen, rather than only changing the
    /// order the *next* listing would arrive in. This used to set the field and
    /// stop, which was invisible while nothing could reach it — and became a
    /// column header that moved its own little sort arrow and nothing else the
    /// moment the headers became clickable.
    pub fn toggle_sort(&mut self, column: SortColumn) {
        if self.sort_by == column {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_by = column;
            self.sort_ascending = true;
        }

        // Follow the picked *entry* through the reordering, not its row number.
        // The row number after a re-sort belongs to some other file, and the
        // confirm button reads the selection — so keeping the number would let
        // a click on "Size" change which file Open would open.
        let picked = self
            .selected_index
            .and_then(|index| self.entries.get(index))
            .map(|entry| entry.name.clone());
        let mut entries = core::mem::take(&mut self.entries);
        self.sort_entries(&mut entries);
        self.entries = entries;
        self.selected_index =
            picked.and_then(|name| self.entries.iter().position(|entry| entry.name == name));
    }

    /// Order a listing by the current sort column and direction.
    ///
    /// Directories always come first, whatever the column: a listing that
    /// interleaves them by size or date buries the way *out* of the directory
    /// among the files in it.
    fn sort_entries(&self, entries: &mut [DirEntry]) {
        entries.sort_by(|a, b| {
            match (a.is_dir, b.is_dir) {
                (true, false) => return core::cmp::Ordering::Less,
                (false, true) => return core::cmp::Ordering::Greater,
                _ => {}
            }
            let ordering = match self.sort_by {
                SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortColumn::Size => a.size.cmp(&b.size),
                SortColumn::Modified => a.modified_timestamp.cmp(&b.modified_timestamp),
            };
            if self.sort_ascending {
                ordering
            } else {
                ordering.reverse()
            }
        });
    }

    // --- Private helpers ---

    fn new(mode: DialogMode) -> Self {
        Self {
            mode,
            current_path: String::from("/"),
            entries: Vec::new(),
            selected_index: None,
            filename_input: String::new(),
            filters: Vec::new(),
            active_filter_index: 0,
            show_hidden: false,
            sort_by: SortColumn::Name,
            sort_ascending: true,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            quick_access: default_quick_access(),
            cancelled: false,
            scroll_top: 0,
            wheel: wheel::Accumulator::default(),
            thumb_grab: None,
            timezone: Tz::UTC,
        }
    }

    /// Returns the effective filter list (user-added filters + "All files").
    fn effective_filters(&self) -> Vec<FileFilter> {
        let mut filters = self.filters.clone();
        let has_all = filters
            .iter()
            .any(|f| f.patterns.iter().any(|p| p == "*" || p == "*.*"));
        if !has_all {
            filters.push(FileFilter {
                description: String::from("All files"),
                patterns: vec![String::from("*")],
            });
        }
        filters
    }

    /// In save mode, if the user's filename input lacks an extension matching the
    /// active filter, append the first extension from the filter.
    fn filename_with_extension(&self) -> String {
        let name = &self.filename_input;
        if name.is_empty() {
            return String::new();
        }

        let filters = self.effective_filters();
        let filter = match filters.get(self.active_filter_index) {
            Some(f) => f,
            None => return name.clone(),
        };

        // If filter is "all files", don't auto-append.
        if filter.patterns.iter().any(|p| p == "*" || p == "*.*") {
            return name.clone();
        }

        // Check if the filename already has a matching extension.
        for pattern in &filter.patterns {
            if let Some(ext) = pattern.strip_prefix("*.")
                && name.ends_with(&format!(".{ext}"))
            {
                return name.clone();
            }
        }

        // Append the first pattern's extension.
        if let Some(first) = filter.patterns.first()
            && let Some(ext) = first.strip_prefix("*.")
        {
            return format!("{name}.{ext}");
        }

        name.clone()
    }

    /// Move the selection `delta` rows, stopping at either end of the list, and
    /// scroll far enough that the row it lands on is on screen.
    ///
    /// Done entirely in `usize` with saturating steps. The old version cast
    /// the index to `isize` to add a signed delta and clamped against
    /// `len as isize - 1`, which is two conversions and a subtraction that are
    /// each only safe because of something proven elsewhere — a non-empty
    /// list, a delta small enough not to overflow. Saturating in the index's
    /// own type needs none of those proofs, and `checked_sub` on the length
    /// *is* the emptiness check.
    fn move_selection(&mut self, delta: isize, height: f32) {
        let Some(last) = self.entries.len().checked_sub(1) else {
            return;
        };
        let current = self.selected_index.unwrap_or(0);
        let next = if delta < 0 {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            current.saturating_add(delta.unsigned_abs())
        };
        self.selected_index = Some(next.min(last));
        self.reveal(height);
    }

    /// Scroll the least distance that puts the selected row on screen.
    ///
    /// Called by everything that moves the selection with a keystroke, and by
    /// nothing else: an explicit scroll — the wheel, a drag of the thumb — is
    /// allowed to leave the selection behind, which is what every file manager
    /// does and what makes it possible to look elsewhere without losing the row
    /// you had picked.
    fn reveal(&mut self, height: f32) {
        let Some(selected) = self.selected_index else {
            return;
        };
        let capacity = Self::row_capacity(height);
        // Above the window: pull the top down to the selection.
        self.scroll_top = self.scroll_top.min(selected);
        // Below it: push the top up until the selection is the last row drawn.
        // A window `capacity` rows tall ending at `selected` starts at
        // `selected + 1 - capacity`; `checked_sub` returning `None` means the
        // window already reaches row 0 from there, so there is nothing to push.
        if let Some(top) = selected
            .checked_add(1)
            .and_then(|past_end| past_end.checked_sub(capacity))
        {
            self.scroll_top = self.scroll_top.max(top);
        }
    }

    /// How many whole entry rows fit below the column headers at `height`.
    ///
    /// A partially-visible row does not count, so nothing is ever drawn across
    /// the bottom edge of the list — [`scroll_window`]'s rule, applied from
    /// [`scroll_window::capacity`] rather than restated here.
    fn row_capacity(height: f32) -> usize {
        scroll_window::capacity(
            ROW_HEIGHT,
            height - TOOLBAR_HEIGHT - BOTTOM_BAR_HEIGHT - ROW_HEIGHT,
        )
    }

    /// Which rows of the listing are on screen at `height`.
    ///
    /// Derived rather than stored, and re-derived on every call: the listing can
    /// be replaced between the keystroke that last moved [`Self::scroll_top`]
    /// and the frame that asks what to draw, and this is the last chance to
    /// notice it got shorter. [`scroll_window::visible_count`] is what turns
    /// "shrank underneath us" into the last page rather than an empty list.
    fn visible_rows(&self, height: f32) -> Range<usize> {
        let rows = scroll_window::visible_count(
            self.entries.len(),
            Self::row_capacity(height),
            self.scroll_top,
        );
        rows.start..rows.end()
    }

    /// Run `nav`, reporting a navigation only if the path actually moved.
    ///
    /// A host answers [`DialogAction::NavigatedTo`] by reading that directory
    /// and handing the listing back through [`set_entries`](Self::set_entries),
    /// so the answer has to be exact in both directions: claiming a move that
    /// did not happen costs a pointless directory read, and failing to report
    /// one that did leaves the previous directory's files on screen under the
    /// new directory's name.
    ///
    /// The Alt+Backspace arm used to decide by asking whether any history
    /// remained *afterwards*, which is a different question with a different
    /// answer in exactly one case: going back to the first directory of the
    /// session empties the history, so the one navigation that always happens
    /// was the one always reported as not having happened.
    fn navigated(&mut self, nav: impl FnOnce(&mut Self)) -> DialogAction {
        let before = self.current_path.clone();
        nav(self);
        if self.current_path == before {
            DialogAction::None
        } else {
            DialogAction::NavigatedTo(self.current_path.clone())
        }
    }

    // --- Render sub-methods ---

    fn draw_toolbar(&self, frame: &mut Frame<DialogTarget>, width: f32) {
        // Toolbar background
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: TOOLBAR_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii {
                top_left: CORNER_RADIUS,
                top_right: CORNER_RADIUS,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        let btn_y = (TOOLBAR_HEIGHT - 24.0) / 2.0;
        let mut x = PADDING;

        // Navigation buttons. Each is a bare glyph, so its click target is the
        // slot the glyph sits in rather than the glyph's own ink — a single
        // character is a few pixels wide and would be a target nobody could
        // hit. The whole toolbar's height is used, not the glyph's, for the
        // same reason.
        //
        // Back and Forward are drawn greyed when there is nowhere to go, and
        // are still recorded: the press is then swallowed by a control that
        // visibly does nothing, which is what a disabled button is. Dropping
        // the target instead would let the click fall through to the toolbar,
        // which is not different here but would be if anything were behind it.
        let back_color = if self.history_back.is_empty() {
            COLOR_OVERLAY
        } else {
            COLOR_TEXT
        };
        frame.push(RenderCommand::Text {
            x,
            y: btn_y + 4.0,
            text: String::from("<"),
            color: back_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        frame.hit(DialogTarget::Back, Rect::new(x, 0.0, 24.0, TOOLBAR_HEIGHT));
        x += 24.0;

        // Forward button
        let fwd_color = if self.history_forward.is_empty() {
            COLOR_OVERLAY
        } else {
            COLOR_TEXT
        };
        frame.push(RenderCommand::Text {
            x,
            y: btn_y + 4.0,
            text: String::from(">"),
            color: fwd_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        frame.hit(
            DialogTarget::Forward,
            Rect::new(x, 0.0, 24.0, TOOLBAR_HEIGHT),
        );
        x += 24.0;

        // Up button
        frame.push(RenderCommand::Text {
            x,
            y: btn_y + 4.0,
            text: String::from("^"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        frame.hit(DialogTarget::Up, Rect::new(x, 0.0, 28.0, TOOLBAR_HEIGHT));
        x += 28.0;

        // Address bar
        let addr_width = width - x - PADDING;
        frame.push(RenderCommand::FillRect {
            x,
            y: btn_y,
            width: addr_width,
            height: 24.0,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });
        frame.push(RenderCommand::Text {
            x: x + 6.0,
            y: btn_y + 5.0,
            text: self.current_path.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(addr_width - 12.0),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(
            DialogTarget::AddressBar,
            Rect::new(x, btn_y, addr_width, 24.0),
        );
    }

    fn draw_sidebar(&self, frame: &mut Frame<DialogTarget>, top: f32, height: f32) {
        // Sidebar background
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: SIDEBAR_WIDTH,
            height,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // The sidebar is clipped to its own background so that a shortcut list
        // longer than a short dialog cannot draw over the bottom bar, and — the
        // part that matters here — so that a shortcut scrolled out of sight
        // cannot still be clicked. `Frame::hit` trims to the clip in force.
        frame.clip(Rect::new(0.0, top, SIDEBAR_WIDTH, height));
        let mut y = top + PADDING;
        for (index, qa) in self.quick_access.iter().enumerate() {
            frame.push(RenderCommand::Text {
                x: PADDING + 4.0,
                y,
                text: qa.label.clone(),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0 - 4.0),
                overflow: TextOverflow::Ellipsis,
            });
            // The row, not the label: the gap between two labels belongs to the
            // one above it, so there is no dead strip between shortcuts.
            frame.hit(
                DialogTarget::Shortcut(index),
                Rect::new(0.0, y, SIDEBAR_WIDTH, ROW_HEIGHT),
            );
            y += ROW_HEIGHT;
        }
        frame.unclip();
    }

    fn draw_file_list(
        &self,
        frame: &mut Frame<DialogTarget>,
        x: f32,
        top: f32,
        width: f32,
        height: f32,
        dialog_height: f32,
    ) {
        // Clip the file list area
        frame.clip(Rect::new(x, top, width, height));
        // The list's own background, below the rows, so that a click in the
        // empty space under a short listing still counts as "in the list" — the
        // wheel needs to know that much to decide whether to scroll.
        frame.hit(DialogTarget::List, Rect::new(x, top, width, height));

        // Column headers
        let header_y = top;
        frame.push(RenderCommand::FillRect {
            x,
            y: header_y,
            width,
            height: ROW_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });

        let name_col_x = x + PADDING + 20.0; // leave space for icon placeholder
        let size_col_x = x + width - 200.0;
        let date_col_x = x + width - 100.0;

        // Each header is clickable across its whole column, not just under its
        // label: the label sits at the column's left edge, and a target the
        // width of the word "Size" leaves most of the column dead.
        frame.hit(
            DialogTarget::Header(SortColumn::Name),
            Rect::new(x, header_y, size_col_x - x, ROW_HEIGHT),
        );
        frame.hit(
            DialogTarget::Header(SortColumn::Size),
            Rect::new(size_col_x, header_y, date_col_x - size_col_x, ROW_HEIGHT),
        );
        frame.hit(
            DialogTarget::Header(SortColumn::Modified),
            Rect::new(date_col_x, header_y, x + width - date_col_x, ROW_HEIGHT),
        );

        // Header labels
        frame.push(RenderCommand::Text {
            x: name_col_x,
            y: header_y + 6.0,
            text: String::from("Name"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        frame.push(RenderCommand::Text {
            x: size_col_x,
            y: header_y + 6.0,
            text: String::from("Size"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        frame.push(RenderCommand::Text {
            x: date_col_x,
            y: header_y + 6.0,
            text: String::from("Modified"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Sort indicator on active column
        let indicator_x = match self.sort_by {
            SortColumn::Name => name_col_x + 36.0,
            SortColumn::Size => size_col_x + 30.0,
            SortColumn::Modified => date_col_x + 54.0,
        };
        let indicator = if self.sort_ascending { "v" } else { "^" };
        frame.push(RenderCommand::Text {
            x: indicator_x,
            y: header_y + 6.0,
            text: String::from(indicator),
            color: COLOR_OVERLAY,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // File entries. Only the rows on screen are built at all: the loop used
        // to walk the whole listing and break once a row fell past the bottom
        // edge, which drew the right thing but had no way to reach anything
        // below it — the tail of a long directory could not be got at by any
        // means, since the selection did not scroll the list either.
        let rows = self.visible_rows(dialog_height);
        let first = rows.start;
        let entries_top = top + ROW_HEIGHT;
        for (offset, entry) in self
            .entries
            .get(rows)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let index = first.saturating_add(offset);
            let row_y = entries_top + (offset as f32) * ROW_HEIGHT;

            // Selection highlight
            if self.selected_index == Some(index) {
                frame.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: ROW_HEIGHT,
                    color: COLOR_SURFACE2,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            // Icon placeholder (folder vs file indicator)
            let icon_char = if entry.is_dir { "D" } else { "F" };
            let icon_color = if entry.is_dir {
                COLOR_YELLOW
            } else {
                COLOR_SUBTEXT
            };
            frame.push(RenderCommand::Text {
                x: x + PADDING,
                y: row_y + 6.0,
                text: String::from(icon_char),
                color: icon_color,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Name
            let name_color = if entry.is_dir {
                COLOR_YELLOW
            } else {
                COLOR_TEXT
            };
            let max_name_width = size_col_x - name_col_x - PADDING;
            frame.push(RenderCommand::Text {
                x: name_col_x,
                y: row_y + 6.0,
                text: entry.name.clone(),
                color: name_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_name_width),
                overflow: TextOverflow::Ellipsis,
            });

            // Size (human-readable, only for files)
            if !entry.is_dir {
                frame.push(RenderCommand::Text {
                    x: size_col_x,
                    y: row_y + 6.0,
                    text: format_size(entry.size),
                    color: COLOR_SUBTEXT,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            // Modified timestamp (simplified display)
            frame.push(RenderCommand::Text {
                x: date_col_x,
                y: row_y + 6.0,
                text: format_timestamp(entry.modified_timestamp, &self.timezone),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // The whole row, so that a click anywhere along it picks the file —
            // aiming at the filename's ink would leave the size and date
            // columns dead, and the row is what the highlight covers.
            frame.hit(
                DialogTarget::Entry(index),
                Rect::new(x, row_y, width, ROW_HEIGHT),
            );
        }

        self.draw_scrollbar(frame, x, entries_top, width, height, dialog_height);

        frame.unclip();
    }

    /// Draw the file list's scrollbar, if the listing is longer than the space
    /// it is drawn into.
    ///
    /// Nothing is drawn when everything fits — a track with a full-length thumb
    /// says "there is more" as loudly as one with a short thumb, and there is
    /// not.
    fn draw_scrollbar(
        &self,
        frame: &mut Frame<DialogTarget>,
        x: f32,
        entries_top: f32,
        width: f32,
        height: f32,
        dialog_height: f32,
    ) {
        let capacity = Self::row_capacity(dialog_height);
        let total = self.entries.len();
        if capacity == 0 || total <= capacity {
            return;
        }

        let track = Rect::new(
            x + width - SCROLLBAR_WIDTH,
            entries_top,
            SCROLLBAR_WIDTH,
            (height - ROW_HEIGHT).max(0.0),
        );
        frame.push(RenderCommand::FillRect {
            x: track.x,
            y: track.y,
            width: track.w,
            height: track.h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        frame.hit(DialogTarget::ScrollTrack, track);

        let thumb = thumb_rect(
            track,
            total,
            capacity,
            self.visible_rows(dialog_height).start,
        );
        frame.push(RenderCommand::FillRect {
            x: thumb.x,
            y: thumb.y,
            width: thumb.w,
            height: thumb.h,
            color: COLOR_SURFACE2,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        // Recorded after the track, so a press on the overlap reaches the thumb
        // — `hit_test` answers with the last box drawn, which is the one on top.
        frame.hit(DialogTarget::ScrollThumb, thumb);
    }

    fn draw_bottom_bar(&self, frame: &mut Frame<DialogTarget>, y: f32, width: f32) {
        // Bottom bar background
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width,
            height: BOTTOM_BAR_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: CORNER_RADIUS,
                bottom_right: CORNER_RADIUS,
            },
        });

        let input_y = y + (BOTTOM_BAR_HEIGHT - 28.0) / 2.0;

        // Filename input (save mode only)
        if self.mode == DialogMode::Save {
            let input_width = width - BUTTON_WIDTH * 2.0 - PADDING * 5.0;
            frame.push(RenderCommand::FillRect {
                x: PADDING,
                y: input_y,
                width: input_width,
                height: 28.0,
                color: COLOR_SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });
            frame.hit(
                DialogTarget::FilenameInput,
                Rect::new(PADDING, input_y, input_width, 28.0),
            );
            frame.push(RenderCommand::StrokeRect {
                x: PADDING,
                y: input_y,
                width: input_width,
                height: 28.0,
                color: COLOR_BLUE,
                line_width: 1.0,
                corner_radii: CornerRadii::all(3.0),
            });

            let display_text = if self.filename_input.is_empty() {
                String::from("Enter filename...")
            } else {
                self.filename_input.clone()
            };
            let text_color = if self.filename_input.is_empty() {
                COLOR_OVERLAY
            } else {
                COLOR_TEXT
            };
            frame.push(RenderCommand::Text {
                x: PADDING + 6.0,
                y: input_y + 7.0,
                text: display_text,
                color: text_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(input_width - 12.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Buttons (right-aligned)
        let cancel_x = width - BUTTON_WIDTH - PADDING;
        let confirm_x = cancel_x - BUTTON_WIDTH - PADDING;

        // Confirm button
        let confirm_enabled = self.confirm().is_some();
        let confirm_bg = if confirm_enabled {
            COLOR_BLUE
        } else {
            COLOR_SURFACE2
        };
        let confirm_label = match self.mode {
            DialogMode::Open => "Open",
            DialogMode::Save => "Save",
            DialogMode::SelectFolder => "Select",
        };
        frame.push(RenderCommand::FillRect {
            x: confirm_x,
            y: input_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: confirm_bg,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        // Recorded whether or not it is enabled. A disabled Open button that let
        // the click through would be a hole in the dialog exactly where the user
        // aims most confidently; `confirm()` returning `None` is what makes the
        // press do nothing, and it is asked again at press time rather than
        // trusted from paint time.
        frame.hit(
            DialogTarget::Confirm,
            Rect::new(confirm_x, input_y, BUTTON_WIDTH, BUTTON_HEIGHT),
        );
        frame.push(RenderCommand::Text {
            x: confirm_x + (BUTTON_WIDTH - 30.0) / 2.0,
            y: input_y + 8.0,
            text: String::from(confirm_label),
            color: if confirm_enabled {
                COLOR_BASE
            } else {
                COLOR_OVERLAY
            },
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Cancel button
        frame.push(RenderCommand::FillRect {
            x: cancel_x,
            y: input_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        frame.hit(
            DialogTarget::Cancel,
            Rect::new(cancel_x, input_y, BUTTON_WIDTH, BUTTON_HEIGHT),
        );
        frame.push(RenderCommand::Text {
            x: cancel_x + (BUTTON_WIDTH - 42.0) / 2.0,
            y: input_y + 8.0,
            text: String::from("Cancel"),
            color: COLOR_RED,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

// --- Free functions (utilities) ---

/// How many rows Page Up or Page Down moves at `height`.
///
/// One windowful, and never zero: a page key that moved nothing in a dialog too
/// short to show a whole row would be a key that appears broken, and one row is
/// the smallest movement that is still movement.
fn page_step(height: f32) -> isize {
    isize::try_from(FileDialog::row_capacity(height).max(1)).unwrap_or(isize::MAX)
}

/// Where the scrollbar thumb sits in `track` for a listing of `total` rows
/// showing `capacity` of them starting at row `first`.
///
/// Size shows how much of the listing is on screen; position shows where in it.
/// The two are computed separately because [`MIN_THUMB_HEIGHT`] makes the size
/// stop being proportional for a long listing while the position must stay
/// exact — a thumb that reached the bottom of its track only when the last row
/// was reached, but sat at 90% when the listing was at its end, would be worse
/// than no thumb at all.
fn thumb_rect(track: Rect, total: usize, capacity: usize, first: usize) -> Rect {
    let shown = (capacity as f32 / total as f32).clamp(0.0, 1.0);
    let thumb_h = (track.h * shown).clamp(MIN_THUMB_HEIGHT.min(track.h), track.h);
    let hidden = total.saturating_sub(capacity);
    let position = if hidden == 0 {
        0.0
    } else {
        (first as f32 / hidden as f32).clamp(0.0, 1.0)
    };
    Rect::new(
        track.x,
        track.y + (track.h - thumb_h) * position,
        track.w,
        thumb_h,
    )
}

/// Default quick-access sidebar entries.
fn default_quick_access() -> Vec<QuickAccess> {
    vec![
        QuickAccess {
            label: String::from("Home"),
            path: String::from("/home/user"),
        },
        QuickAccess {
            label: String::from("Documents"),
            path: String::from("/home/user/documents"),
        },
        QuickAccess {
            label: String::from("Downloads"),
            path: String::from("/home/user/downloads"),
        },
        QuickAccess {
            label: String::from("Desktop"),
            path: String::from("/home/user/desktop"),
        },
        QuickAccess {
            label: String::from("Recent"),
            path: String::from("/recent"),
        },
    ]
}

/// Get the parent of a path (simple slash-based splitting).
fn parent_path(path: &str) -> String {
    if path == "/" || path.is_empty() {
        return String::from("/");
    }
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => String::from("/"),
        Some(idx) => trimmed[..idx].to_string(),
        None => String::from("/"),
    }
}

/// Join a directory path and a child name.
fn join_path(dir: &str, name: &str) -> String {
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Format a byte size into a human-readable string.
fn format_size(bytes: u64) -> String {
    crate::bytes::iec(bytes)
}

/// Format a Unix timestamp as a `YYYY-MM-DD` date in `zone`, or `--` if unset.
///
/// The old implementation divided the epoch day count by 365 and then by 30,
/// which is a calendar with no leap years and twelve 30-day months. It was
/// wrong in three compounding ways: the missing leap days put the date about
/// two weeks early by 2026, the 360-day year advanced the year number early
/// on top of that, and the 30-day months meant the day-of-month was very
/// nearly never right. A "simplified display" of a file's modification date
/// that names the wrong day is not simplified, it is false, and the user has
/// no way to tell — which is worse than showing nothing.
fn format_timestamp(epoch_secs: u64, zone: &Tz) -> String {
    if epoch_secs == 0 {
        return String::from("--");
    }
    let utc = i64::try_from(epoch_secs).unwrap_or(i64::MAX);
    // Timestamps are UTC; the column is read as local time. `lookup` picks the
    // offset in force *at that instant*, so a file written in summer keeps its
    // summer date when read in winter.
    let local = utc.saturating_add(i64::from(zone.lookup(utc).gmtoff));
    // `Date`'s `Display` is ISO 8601, which is what this column has always
    // shown; the day-from-instant conversion now happens in one place for the
    // whole toolkit rather than here.
    Date::from_unix_utc(local).to_string()
}

/// Check whether a filename matches any of the given glob patterns.
/// Supports simple `*.ext` patterns only (not full glob).
fn matches_any_pattern(filename: &str, patterns: &[&str]) -> bool {
    for pattern in patterns {
        if *pattern == "*" || *pattern == "*.*" {
            return true;
        }
        if let Some(ext) = pattern.strip_prefix("*.")
            && filename.ends_with(&format!(".{ext}"))
        {
            return true;
        }
        // Exact match fallback
        if *pattern == filename {
            return true;
        }
    }
    false
}

/// List `path` for a [`FileDialog`], in the shape [`FileDialog::set_entries`]
/// wants.
///
/// The dialog does no I/O of its own — it is a widget and holds whatever
/// listing it is handed — so every navigation has to be answered with a fresh
/// listing. That split is deliberate (a widget that read the filesystem could
/// not be driven by a test, and could not show a listing that came from
/// somewhere other than the local disk), but it left every caller writing the
/// same twenty lines: `apps/diskimager` had them, and `apps/archivemanager`
/// was about to. The reader belongs next to the widget that consumes it.
///
/// An unreadable directory yields an empty listing rather than an error: the
/// dialog is a place the user is *browsing*, and a permission-denied folder
/// they wandered into is a normal thing to find, not a failure of the program.
/// Entries whose metadata cannot be read are skipped for the same reason.
///
/// The name is taken from `file_name` as an `OsStr` and converted once. A path
/// component is a byte string on this OS, and a listing is the one place a
/// filename with no UTF-8 reading still has to be *shown*, so lossy conversion
/// is the right answer here and only here — what is opened is rebuilt from
/// `current_path` plus this name inside the dialog, so a substituted character
/// would open the wrong file. That is a real limitation, and it belongs to
/// [`DirEntry`]'s `String`-typed API rather than to this function; recorded in
/// `known-issues.md`.
#[must_use]
pub fn list_directory(path: &str) -> Vec<DirEntry> {
    let Ok(iter) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in iter.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        let extension = name
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .unwrap_or_default();
        out.push(DirEntry {
            is_dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified_timestamp: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs()),
            extension: if meta.is_dir() {
                String::new()
            } else {
                extension
            },
            name,
        });
    }
    out
}

// --- Tests ---

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

    /// The size the tests render and dispatch at. The dialog stores no size of
    /// its own — every handler is *given* one, exactly as a host gives it — so
    /// the tests have to name one too rather than read it back off the widget.
    /// 400 tall leaves room for ten rows, so a listing of a dozen has a tail
    /// below the fold to scroll to.
    const W: f32 = 600.0;
    const H: f32 = 400.0;

    fn file(name: &str) -> DirEntry {
        DirEntry {
            name: String::from(name),
            is_dir: false,
            size: 10,
            modified_timestamp: 1_000,
            extension: name
                .rsplit_once('.')
                .map_or(String::new(), |(_, e)| String::from(e)),
        }
    }

    fn dir(name: &str) -> DirEntry {
        DirEntry {
            name: String::from(name),
            is_dir: true,
            size: 0,
            modified_timestamp: 1_000,
            extension: String::new(),
        }
    }

    /// Aim a click at the middle of whatever the frame drew for `target`.
    /// Deliberately *not* a recomputed coordinate: the point of recording hit
    /// boxes during the draw is that no second copy of the geometry exists to
    /// disagree with the first.
    fn centre_of(dialog: &FileDialog, target: DialogTarget) -> (f32, f32) {
        let frame = dialog.frame(W, H);
        let rect = frame
            .rect_of(|t| *t == target)
            .unwrap_or_else(|| panic!("{target:?} should have been drawn"));
        rect.centre()
    }

    fn click_at(dialog: &mut FileDialog, x: f32, y: f32) -> DialogAction {
        dialog.handle_mouse(&press_at(x, y), W, H)
    }

    fn press_at(x: f32, y: f32) -> MouseEvent {
        MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }
    }

    fn click(dialog: &mut FileDialog, target: DialogTarget) -> DialogAction {
        let (x, y) = centre_of(dialog, target);
        click_at(dialog, x, y)
    }

    #[test]
    fn test_open_dialog_creation() {
        let dialog = FileDialog::open()
            .with_filter("Rust files", &["*.rs"])
            .with_initial_path("/home/user/projects");

        assert_eq!(dialog.mode(), DialogMode::Open);
        assert_eq!(dialog.current_path(), "/home/user/projects");
        assert_eq!(dialog.selected_index(), None);
        assert!(!dialog.is_cancelled());
    }

    #[test]
    fn test_save_dialog_with_filename() {
        let dialog = FileDialog::save()
            .with_filename("hello.rs")
            .with_initial_path("/tmp");

        assert_eq!(dialog.mode(), DialogMode::Save);
        assert_eq!(dialog.filename_input, "hello.rs");
    }

    #[test]
    fn test_navigate_to_pushes_history() {
        let mut dialog = FileDialog::open().with_initial_path("/home");
        dialog.navigate_to("/home/user");
        dialog.navigate_to("/home/user/docs");

        assert_eq!(dialog.current_path(), "/home/user/docs");
        assert_eq!(dialog.history_back.len(), 2);
        assert!(dialog.history_forward.is_empty());
    }

    #[test]
    fn test_navigate_back_and_forward() {
        let mut dialog = FileDialog::open().with_initial_path("/a");
        dialog.navigate_to("/b");
        dialog.navigate_to("/c");

        dialog.navigate_back();
        assert_eq!(dialog.current_path(), "/b");
        assert_eq!(dialog.history_forward.len(), 1);

        dialog.navigate_forward();
        assert_eq!(dialog.current_path(), "/c");
        assert!(dialog.history_forward.is_empty());
    }

    #[test]
    fn test_navigate_up() {
        let mut dialog = FileDialog::open().with_initial_path("/home/user/docs");
        dialog.navigate_up();
        assert_eq!(dialog.current_path(), "/home/user");

        dialog.navigate_up();
        assert_eq!(dialog.current_path(), "/home");

        dialog.navigate_up();
        assert_eq!(dialog.current_path(), "/");

        // At root, navigating up stays at root.
        dialog.navigate_up();
        assert_eq!(dialog.current_path(), "/");
    }

    #[test]
    fn test_set_entries_sorts_dirs_first() {
        let mut dialog = FileDialog::open();
        dialog.set_entries(vec![
            DirEntry {
                name: String::from("zebra.txt"),
                is_dir: false,
                size: 100,
                modified_timestamp: 1000,
                extension: String::from("txt"),
            },
            DirEntry {
                name: String::from("alpha"),
                is_dir: true,
                size: 0,
                modified_timestamp: 2000,
                extension: String::new(),
            },
            DirEntry {
                name: String::from("beta.rs"),
                is_dir: false,
                size: 200,
                modified_timestamp: 3000,
                extension: String::from("rs"),
            },
        ]);

        // Directory should be first.
        assert_eq!(dialog.entries()[0].name, "alpha");
        assert!(dialog.entries()[0].is_dir);
    }

    #[test]
    fn test_set_entries_filters_hidden() {
        let mut dialog = FileDialog::open();
        dialog.set_entries(vec![
            DirEntry {
                name: String::from(".hidden"),
                is_dir: false,
                size: 10,
                modified_timestamp: 100,
                extension: String::new(),
            },
            DirEntry {
                name: String::from("visible.txt"),
                is_dir: false,
                size: 20,
                modified_timestamp: 200,
                extension: String::from("txt"),
            },
        ]);

        assert_eq!(dialog.entries().len(), 1);
        assert_eq!(dialog.entries()[0].name, "visible.txt");
    }

    #[test]
    fn test_show_hidden_includes_dotfiles() {
        let mut dialog = FileDialog::open().show_hidden(true);
        dialog.set_entries(vec![
            DirEntry {
                name: String::from(".hidden"),
                is_dir: false,
                size: 10,
                modified_timestamp: 100,
                extension: String::new(),
            },
            DirEntry {
                name: String::from("visible.txt"),
                is_dir: false,
                size: 20,
                modified_timestamp: 200,
                extension: String::from("txt"),
            },
        ]);

        assert_eq!(dialog.entries().len(), 2);
    }

    #[test]
    fn test_filter_by_extension() {
        let mut dialog = FileDialog::open().with_filter("Rust files", &["*.rs"]);
        // Activate the Rust filter (index 0).
        dialog.set_filter_index(0);
        dialog.set_entries(vec![
            DirEntry {
                name: String::from("main.rs"),
                is_dir: false,
                size: 500,
                modified_timestamp: 100,
                extension: String::from("rs"),
            },
            DirEntry {
                name: String::from("readme.md"),
                is_dir: false,
                size: 300,
                modified_timestamp: 200,
                extension: String::from("md"),
            },
            DirEntry {
                name: String::from("src"),
                is_dir: true,
                size: 0,
                modified_timestamp: 300,
                extension: String::new(),
            },
        ]);

        // Should have: src (dir, always passes) + main.rs.
        assert_eq!(dialog.entries().len(), 2);
        assert_eq!(dialog.entries()[0].name, "src");
        assert_eq!(dialog.entries()[1].name, "main.rs");
    }

    #[test]
    fn test_confirm_open_requires_file_selection() {
        let mut dialog = FileDialog::open();
        dialog.set_entries(vec![DirEntry {
            name: String::from("file.txt"),
            is_dir: false,
            size: 100,
            modified_timestamp: 1000,
            extension: String::from("txt"),
        }]);

        // No selection yet.
        assert_eq!(dialog.confirm(), None);

        // Select the file.
        dialog.select_entry(0);
        assert_eq!(dialog.confirm(), Some(String::from("/file.txt")));
    }

    #[test]
    fn test_confirm_save_uses_filename_input() {
        let mut dialog = FileDialog::save().with_initial_path("/docs");
        assert_eq!(dialog.confirm(), None);

        dialog.set_filename("report.txt");
        assert_eq!(dialog.confirm(), Some(String::from("/docs/report.txt")));
    }

    #[test]
    fn test_save_auto_appends_extension() {
        let mut dialog = FileDialog::save()
            .with_filter("Rust files", &["*.rs"])
            .with_initial_path("/src");
        dialog.set_filter_index(0);
        dialog.set_filename("main");

        // confirm() should append .rs
        assert_eq!(dialog.confirm(), Some(String::from("/src/main.rs")));
    }

    #[test]
    fn test_save_no_double_extension() {
        let mut dialog = FileDialog::save()
            .with_filter("Rust files", &["*.rs"])
            .with_initial_path("/src");
        dialog.set_filter_index(0);
        dialog.set_filename("main.rs");

        // Should not double up the extension.
        assert_eq!(dialog.confirm(), Some(String::from("/src/main.rs")));
    }

    #[test]
    fn test_activate_entry_navigates_into_dir() {
        let mut dialog = FileDialog::open().with_initial_path("/home");
        dialog.set_entries(vec![DirEntry {
            name: String::from("projects"),
            is_dir: true,
            size: 0,
            modified_timestamp: 1000,
            extension: String::new(),
        }]);

        let action = dialog.activate_entry(0);
        assert_eq!(
            action,
            DialogAction::NavigatedTo(String::from("/home/projects"))
        );
        assert_eq!(dialog.current_path(), "/home/projects");
    }

    #[test]
    fn test_activate_file_in_open_mode_selects() {
        let mut dialog = FileDialog::open().with_initial_path("/docs");
        dialog.set_entries(vec![DirEntry {
            name: String::from("notes.txt"),
            is_dir: false,
            size: 50,
            modified_timestamp: 2000,
            extension: String::from("txt"),
        }]);

        let action = dialog.activate_entry(0);
        assert_eq!(
            action,
            DialogAction::Selected(String::from("/docs/notes.txt"))
        );
    }

    #[test]
    fn test_cancel() {
        let mut dialog = FileDialog::open();
        assert!(!dialog.is_cancelled());
        dialog.cancel();
        assert!(dialog.is_cancelled());
    }

    #[test]
    fn test_handle_escape_cancels() {
        let mut dialog = FileDialog::open();
        let event = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: crate::event::Modifiers::NONE,
            text: String::new(),
        };
        let action = dialog.handle_event(&event, H);
        assert_eq!(action, DialogAction::Cancelled);
        assert!(dialog.is_cancelled());
    }

    #[test]
    fn test_arrow_keys_move_selection() {
        let mut dialog = FileDialog::open();
        dialog.set_entries(vec![
            DirEntry {
                name: String::from("a"),
                is_dir: false,
                size: 10,
                modified_timestamp: 100,
                extension: String::new(),
            },
            DirEntry {
                name: String::from("b"),
                is_dir: false,
                size: 20,
                modified_timestamp: 200,
                extension: String::new(),
            },
            DirEntry {
                name: String::from("c"),
                is_dir: false,
                size: 30,
                modified_timestamp: 300,
                extension: String::new(),
            },
        ]);

        let down = KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: crate::event::Modifiers::NONE,
            text: String::new(),
        };
        dialog.handle_event(&down, H);
        assert_eq!(dialog.selected_index(), Some(1));

        dialog.handle_event(&down, H);
        assert_eq!(dialog.selected_index(), Some(2));

        // Should clamp at the end.
        dialog.handle_event(&down, H);
        assert_eq!(dialog.selected_index(), Some(2));

        let up = KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: crate::event::Modifiers::NONE,
            text: String::new(),
        };
        dialog.handle_event(&up, H);
        assert_eq!(dialog.selected_index(), Some(1));
    }

    #[test]
    fn test_render_produces_commands() {
        let dialog = FileDialog::open().with_initial_path("/test");
        let cmds = dialog.render(600.0, 400.0);
        // Should produce at least the background, toolbar, sidebar, bottom bar.
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(1048576), "1.0 MiB");
        assert_eq!(format_size(1073741824), "1.0 GiB");
    }

    #[test]
    fn a_timestamp_names_the_day_it_actually_falls_on() {
        // Dates picked because the arithmetic that used to be here got each of
        // them wrong: a leap day in a leap year, a leap day in a leap year the
        // "divisible by 4" rule alone gets right, the last day of a century
        // that is *not* a leap year, and a plain modern date.
        let utc = Tz::UTC;
        assert_eq!(format_timestamp(0, &utc), "--", "unset stays unset");
        assert_eq!(format_timestamp(1, &utc), "1970-01-01");
        assert_eq!(format_timestamp(86_399, &utc), "1970-01-01");
        assert_eq!(format_timestamp(86_400, &utc), "1970-01-02");
        assert_eq!(format_timestamp(951_782_400, &utc), "2000-02-29");
        assert_eq!(format_timestamp(1_709_164_800, &utc), "2024-02-29");
        assert_eq!(format_timestamp(4_102_444_800, &utc), "2100-01-01");
        assert_eq!(format_timestamp(1_700_000_000, &utc), "2023-11-14");
    }

    #[test]
    fn the_modified_column_agrees_with_the_libc_for_a_century_of_days() {
        // The column must name the same day the shell and `ls -l` would. Both
        // render through `tzrules::days_from_civil`, so walking every day from
        // 1970 to 2079 and requiring the dialog's rendering to invert back to
        // the same day number is the check that they cannot drift apart. A
        // leap year missed or a month length wrong anywhere in that range
        // shows up immediately — the implementation before `Date` would have
        // failed on its 59th day.
        for days in 0..40_000i64 {
            // Midday, not midnight: timestamp 0 is the column's "unset"
            // sentinel, and a mid-day instant also shows the time of day is
            // discarded rather than rounded.
            let rendered = format_timestamp(
                u64::try_from(days * 86_400 + 43_200).expect("non-negative"),
                &Tz::UTC,
            );
            let date = Date::from_days_since_epoch(i32::try_from(days).expect("in range"));
            assert_eq!(rendered, date.to_string(), "day {days}");
            let (year, month, day) = date.ymd();
            assert!((1..=12).contains(&month), "month {month} at day {days}");
            assert!((1..=31).contains(&day), "day {day} at day {days}");
            assert_eq!(
                tzrules::days_from_civil(i64::from(year), month, day),
                days,
                "{rendered} did not round-trip"
            );
        }
    }

    #[test]
    fn the_modified_column_is_read_in_the_dialogs_zone() {
        // Midnight UTC on 2023-11-14. Five hours west of Greenwich it is still
        // the evening of the 13th, and an hour east it is already the 14th.
        let midnight_utc = 19_675 * 86_400;
        let est = Tz::parse(b"EST5").expect("a POSIX TZ string");
        let cet = Tz::parse(b"CET-1").expect("a POSIX TZ string");
        assert_eq!(format_timestamp(midnight_utc, &Tz::UTC), "2023-11-14");
        assert_eq!(format_timestamp(midnight_utc, &est), "2023-11-13");
        assert_eq!(format_timestamp(midnight_utc, &cet), "2023-11-14");

        // And the dialog renders through its own zone, not a hard-wired one.
        let dialog = FileDialog::open().with_timezone(est);
        assert_eq!(dialog.timezone(), est);
    }

    #[test]
    fn a_dialog_renders_the_modified_column_in_its_own_zone() {
        let est = Tz::parse(b"EST5").expect("a POSIX TZ string");
        let entry = DirEntry {
            name: String::from("notes.txt"),
            is_dir: false,
            size: 10,
            modified_timestamp: 19_675 * 86_400,
            extension: String::from("txt"),
        };
        let dated = |zone: Option<Tz>| {
            let mut dialog = FileDialog::open();
            if let Some(zone) = zone {
                dialog.set_timezone(zone);
            }
            dialog.set_entries(vec![entry.clone()]);
            dialog
                .render(800.0, 600.0)
                .into_iter()
                .filter_map(|cmd| match cmd {
                    RenderCommand::Text { text, .. } => Some(text),
                    _ => None,
                })
                .find(|text| text.starts_with("2023-"))
                .expect("the Modified column should be drawn")
        };
        assert_eq!(dated(None), "2023-11-14");
        assert_eq!(dated(Some(est)), "2023-11-13");
    }

    #[test]
    fn moving_the_selection_stops_at_both_ends() {
        let mut dialog = FileDialog::open();
        dialog.set_entries(vec![
            DirEntry {
                name: String::from("a"),
                is_dir: false,
                size: 0,
                modified_timestamp: 1,
                extension: String::new(),
            },
            DirEntry {
                name: String::from("b"),
                is_dir: false,
                size: 0,
                modified_timestamp: 1,
                extension: String::new(),
            },
        ]);
        dialog.move_selection(1, H);
        assert_eq!(dialog.selected_index, Some(1));
        // Far past either end, including a delta that would overflow the
        // signed arithmetic the old implementation used.
        dialog.move_selection(isize::MAX, H);
        assert_eq!(dialog.selected_index, Some(1));
        dialog.move_selection(isize::MIN, H);
        assert_eq!(dialog.selected_index, Some(0));
        dialog.move_selection(-1, H);
        assert_eq!(dialog.selected_index, Some(0));
    }

    #[test]
    fn moving_the_selection_in_an_empty_list_selects_nothing() {
        let mut dialog = FileDialog::open();
        dialog.move_selection(1, H);
        assert_eq!(dialog.selected_index, None);
        dialog.move_selection(-1, H);
        assert_eq!(dialog.selected_index, None);
    }

    #[test]
    fn test_parent_path() {
        assert_eq!(parent_path("/"), "/");
        assert_eq!(parent_path("/home"), "/");
        assert_eq!(parent_path("/home/user"), "/home");
        assert_eq!(parent_path("/home/user/docs"), "/home/user");
        assert_eq!(parent_path("/a/b/c/d"), "/a/b/c");
    }

    #[test]
    fn test_join_path() {
        assert_eq!(join_path("/", "home"), "/home");
        assert_eq!(join_path("/home", "user"), "/home/user");
        assert_eq!(join_path("/a/b", "c"), "/a/b/c");
    }

    #[test]
    fn test_matches_any_pattern() {
        assert!(matches_any_pattern("main.rs", &["*.rs"]));
        assert!(!matches_any_pattern("main.rs", &["*.txt"]));
        assert!(matches_any_pattern("anything", &["*"]));
        assert!(matches_any_pattern("main.rs", &["*.txt", "*.rs"]));
        assert!(matches_any_pattern("exact_match", &["exact_match"]));
    }

    #[test]
    fn test_select_folder_mode() {
        let mut dialog = FileDialog::select_folder().with_initial_path("/home");
        dialog.set_entries(vec![DirEntry {
            name: String::from("projects"),
            is_dir: true,
            size: 0,
            modified_timestamp: 1000,
            extension: String::new(),
        }]);

        // Activating a dir in select-folder mode selects it.
        let action = dialog.activate_entry(0);
        assert_eq!(
            action,
            DialogAction::Selected(String::from("/home/projects"))
        );
    }

    #[test]
    fn test_toggle_sort() {
        let mut dialog = FileDialog::open();
        assert_eq!(dialog.sort_by, SortColumn::Name);
        assert!(dialog.sort_ascending);

        dialog.toggle_sort(SortColumn::Name);
        assert_eq!(dialog.sort_by, SortColumn::Name);
        assert!(!dialog.sort_ascending);

        dialog.toggle_sort(SortColumn::Size);
        assert_eq!(dialog.sort_by, SortColumn::Size);
        assert!(dialog.sort_ascending);
    }

    #[test]
    fn test_navigate_to_same_path_is_noop() {
        let mut dialog = FileDialog::open().with_initial_path("/home");
        dialog.navigate_to("/home");
        assert!(dialog.history_back.is_empty());
    }

    #[test]
    fn a_directory_that_is_not_there_lists_as_empty_rather_than_failing() {
        // A dialog is somewhere the user is browsing. A folder they cannot
        // read is a normal thing to walk into, and the dialog showing it empty
        // is a better answer than the program reporting an error at them.
        assert!(list_directory("/no/such/directory/anywhere").is_empty());
    }

    #[test]
    fn a_listing_names_its_own_files_and_folders_and_sizes_them() {
        // The one test that touches the real filesystem, because the whole
        // point of this function is that it does.
        let dir = std::env::temp_dir().join(format!(
            "guitk-list-directory-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).expect("create the fixture");
        std::fs::write(dir.join("notes.TXT"), b"hello").expect("write the fixture");

        let listing = list_directory(&dir.to_string_lossy());
        let mut names: Vec<&str> = listing.iter().map(|e| e.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["notes.TXT", "sub"]);

        let file = listing
            .iter()
            .find(|e| e.name == "notes.TXT")
            .expect("the file is in its own directory");
        assert!(!file.is_dir);
        assert_eq!(file.size, 5, "the size is the file's, not a guess");
        assert_eq!(
            file.extension, "txt",
            "lower-cased, because the filter patterns are"
        );

        let sub = listing
            .iter()
            .find(|e| e.name == "sub")
            .expect("the directory is in its parent");
        assert!(sub.is_dir);
        assert_eq!(sub.size, 0, "a directory has no size worth showing");
        assert!(
            sub.extension.is_empty(),
            "a directory has no extension to filter on"
        );

        std::fs::remove_dir_all(&dir).expect("remove the fixture");
    }

    // --- Mouse ---

    #[test]
    fn clicking_a_row_selects_it_without_opening_it() {
        let mut dialog = FileDialog::open().with_initial_path("/docs");
        dialog.set_entries(vec![file("a.txt"), file("b.txt"), file("c.txt")]);

        let action = click(&mut dialog, DialogTarget::Entry(1));

        assert_eq!(dialog.selected_index(), Some(1));
        assert_eq!(
            action,
            DialogAction::None,
            "one click picks; it takes a second one, or Open, to act"
        );
        assert_eq!(dialog.current_path(), "/docs");
    }

    #[test]
    fn double_clicking_a_directory_navigates_into_it() {
        let mut dialog = FileDialog::open().with_initial_path("/home");
        dialog.set_entries(vec![dir("projects"), file("notes.txt")]);

        let (x, y) = centre_of(&dialog, DialogTarget::Entry(0));
        let action = dialog.handle_mouse(
            &MouseEvent {
                x,
                y,
                kind: MouseEventKind::DoubleClick(MouseButton::Left),
            },
            W,
            H,
        );

        assert_eq!(
            action,
            DialogAction::NavigatedTo(String::from("/home/projects"))
        );
        assert_eq!(dialog.current_path(), "/home/projects");
    }

    #[test]
    fn double_clicking_a_file_opens_it() {
        let mut dialog = FileDialog::open().with_initial_path("/docs");
        dialog.set_entries(vec![file("notes.txt")]);

        let (x, y) = centre_of(&dialog, DialogTarget::Entry(0));
        let action = dialog.handle_mouse(
            &MouseEvent {
                x,
                y,
                kind: MouseEventKind::DoubleClick(MouseButton::Left),
            },
            W,
            H,
        );

        assert_eq!(
            action,
            DialogAction::Selected(String::from("/docs/notes.txt")),
            "a double-click has to open the row it landed on even if no press \
             selected it first — the host may send only the double-click"
        );
    }

    #[test]
    fn clicking_cancel_cancels() {
        let mut dialog = FileDialog::open();
        assert_eq!(
            click(&mut dialog, DialogTarget::Cancel),
            DialogAction::Cancelled
        );
        assert!(dialog.is_cancelled());
    }

    #[test]
    fn clicking_open_returns_the_selected_file() {
        let mut dialog = FileDialog::open().with_initial_path("/docs");
        dialog.set_entries(vec![file("a.txt"), file("b.txt")]);
        click(&mut dialog, DialogTarget::Entry(1));

        assert_eq!(
            click(&mut dialog, DialogTarget::Confirm),
            DialogAction::Selected(String::from("/docs/b.txt"))
        );
    }

    #[test]
    fn clicking_open_with_nothing_picked_does_nothing() {
        let mut dialog = FileDialog::open().with_initial_path("/docs");
        dialog.set_entries(vec![file("a.txt")]);

        assert_eq!(
            click(&mut dialog, DialogTarget::Confirm),
            DialogAction::None,
            "a disabled Open button must still swallow the click, but must not act"
        );
        assert!(!dialog.is_cancelled());
    }

    #[test]
    fn clicking_up_navigates_and_reports_it() {
        let mut dialog = FileDialog::open().with_initial_path("/home/user/docs");
        assert_eq!(
            click(&mut dialog, DialogTarget::Up),
            DialogAction::NavigatedTo(String::from("/home/user"))
        );
    }

    #[test]
    fn clicking_up_at_the_root_reports_no_move() {
        let mut dialog = FileDialog::open().with_initial_path("/");
        assert_eq!(
            click(&mut dialog, DialogTarget::Up),
            DialogAction::None,
            "claiming a move that did not happen costs the host a pointless read"
        );
    }

    #[test]
    fn clicking_back_at_the_start_of_the_session_still_reports_the_move() {
        // The history is emptied by this very step, so an implementation that
        // asks "is there history left?" afterwards concludes nothing happened
        // and leaves the previous directory's files under the new name.
        let mut dialog = FileDialog::open().with_initial_path("/home");
        click(&mut dialog, DialogTarget::Up);
        assert_eq!(dialog.current_path(), "/");

        assert_eq!(
            click(&mut dialog, DialogTarget::Back),
            DialogAction::NavigatedTo(String::from("/home"))
        );
    }

    #[test]
    fn clicking_a_sidebar_shortcut_navigates_there() {
        let mut dialog = FileDialog::open().with_initial_path("/");
        let expected = dialog.quick_access[1].path.clone();

        assert_eq!(
            click(&mut dialog, DialogTarget::Shortcut(1)),
            DialogAction::NavigatedTo(expected.clone())
        );
        assert_eq!(dialog.current_path(), expected);
    }

    #[test]
    fn clicking_a_header_re_sorts_and_keeps_the_picked_file() {
        let mut dialog = FileDialog::open().with_initial_path("/docs");
        dialog.set_entries(vec![
            DirEntry {
                size: 300,
                ..file("a.txt")
            },
            DirEntry {
                size: 100,
                ..file("b.txt")
            },
            DirEntry {
                size: 200,
                ..file("c.txt")
            },
        ]);
        // Picked by name order: a, b, c -> row 0 is "a.txt".
        click(&mut dialog, DialogTarget::Entry(0));

        click(&mut dialog, DialogTarget::Header(SortColumn::Size));

        let order: Vec<&str> = dialog.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            order,
            ["b.txt", "c.txt", "a.txt"],
            "the header has to reorder the listing, not just move its arrow"
        );
        assert_eq!(
            dialog.selected_index(),
            Some(2),
            "the selection follows the file, not the row number"
        );
        assert_eq!(
            dialog.confirm().as_deref(),
            Some("/docs/a.txt"),
            "so sorting cannot change which file Open opens"
        );
    }

    // --- Scrolling ---

    /// A listing long enough to have a tail below the fold at `H`.
    fn long_listing() -> Vec<DirEntry> {
        (0..30).map(|i| file(&format!("f{i:02}.txt"))).collect()
    }

    #[test]
    fn everything_that_fits_gets_no_scrollbar() {
        let mut dialog = FileDialog::open();
        dialog.set_entries(vec![file("a.txt"), file("b.txt")]);
        let frame = dialog.frame(W, H);

        assert!(
            frame.rect_of(|t| *t == DialogTarget::ScrollTrack).is_none(),
            "a scrollbar with nothing to scroll is a control that does nothing"
        );
    }

    #[test]
    fn the_wheel_reaches_the_rows_below_the_fold() {
        let mut dialog = FileDialog::open().with_initial_path("/docs");
        dialog.set_entries(long_listing());

        let before = dialog.frame(W, H);
        assert!(
            before.rect_of(|t| *t == DialogTarget::Entry(29)).is_none(),
            "the last row starts out below the fold, or this test proves nothing"
        );

        for _ in 0..10 {
            dialog.handle_mouse(
                &MouseEvent {
                    x: W / 2.0,
                    y: H / 2.0,
                    kind: MouseEventKind::Scroll { dx: 0.0, dy: -3.0 },
                },
                W,
                H,
            );
        }

        let after = dialog.frame(W, H);
        assert!(
            after.rect_of(|t| *t == DialogTarget::Entry(29)).is_some(),
            "scrolling to the end has to bring the last row on screen"
        );
        assert!(
            after.rect_of(|t| *t == DialogTarget::Entry(0)).is_none(),
            "and take the first one off it"
        );
    }

    #[test]
    fn the_wheel_stops_at_the_end_rather_than_scrolling_into_nothing() {
        let mut dialog = FileDialog::open();
        dialog.set_entries(long_listing());
        let capacity = FileDialog::row_capacity(H);

        for _ in 0..50 {
            dialog.handle_mouse(
                &MouseEvent {
                    x: W / 2.0,
                    y: H / 2.0,
                    kind: MouseEventKind::Scroll { dx: 0.0, dy: -3.0 },
                },
                W,
                H,
            );
        }

        let rows = dialog.visible_rows(H);
        assert_eq!(
            rows.start,
            30 - capacity,
            "the last page is the end of the list, not a screen of blank"
        );
        assert_eq!(rows.end, 30);
    }

    #[test]
    fn a_row_clicked_after_scrolling_is_the_row_that_was_drawn_there() {
        // The bug class this whole design exists to prevent: hit-testing
        // against geometry recomputed without the scroll offset picks the row
        // that *would* be there if the list had never moved.
        let mut dialog = FileDialog::open();
        dialog.set_entries(long_listing());
        for _ in 0..2 {
            dialog.handle_mouse(
                &MouseEvent {
                    x: W / 2.0,
                    y: H / 2.0,
                    kind: MouseEventKind::Scroll { dx: 0.0, dy: -3.0 },
                },
                W,
                H,
            );
        }
        let first = dialog.visible_rows(H).start;
        assert!(first > 0, "the list has to have actually scrolled");

        // Aim at the topmost drawn row by its pixels, and check the dialog
        // agrees about which entry lives there.
        let rect = dialog
            .frame(W, H)
            .rect_of(|t| *t == DialogTarget::Entry(first))
            .expect("the first visible row is drawn");
        let (x, y) = rect.centre();
        click_at(&mut dialog, x, y);

        assert_eq!(dialog.selected_index(), Some(first));
    }

    #[test]
    fn dragging_the_thumb_scrolls_the_list() {
        let mut dialog = FileDialog::open();
        dialog.set_entries(long_listing());

        let (x, y) = centre_of(&dialog, DialogTarget::ScrollThumb);
        click_at(&mut dialog, x, y);
        assert!(
            dialog.thumb_grab.is_some(),
            "the press has to take the grab"
        );

        // Drag far past the bottom of the track; the offset clamps to the end.
        dialog.handle_mouse(
            &MouseEvent {
                x,
                y: y + H,
                kind: MouseEventKind::Move,
            },
            W,
            H,
        );
        assert_eq!(dialog.visible_rows(H).end, 30);

        dialog.handle_mouse(
            &MouseEvent {
                x,
                y: y - H,
                kind: MouseEventKind::Move,
            },
            W,
            H,
        );
        assert_eq!(dialog.visible_rows(H).start, 0);

        dialog.handle_mouse(
            &MouseEvent {
                x,
                y,
                kind: MouseEventKind::Release(MouseButton::Left),
            },
            W,
            H,
        );
        assert!(
            dialog.thumb_grab.is_none(),
            "releasing has to drop the grab"
        );
    }

    #[test]
    fn a_move_without_a_grab_does_not_scroll() {
        let mut dialog = FileDialog::open();
        dialog.set_entries(long_listing());

        dialog.handle_mouse(
            &MouseEvent {
                x: W - 5.0,
                y: H - 60.0,
                kind: MouseEventKind::Move,
            },
            W,
            H,
        );

        assert_eq!(
            dialog.visible_rows(H).start,
            0,
            "the pointer merely passing over the scrollbar must not move it"
        );
    }

    #[test]
    fn clicking_the_track_pages_towards_the_click() {
        let mut dialog = FileDialog::open();
        dialog.set_entries(long_listing());
        let capacity = FileDialog::row_capacity(H);

        let track = dialog
            .frame(W, H)
            .rect_of(|t| *t == DialogTarget::ScrollTrack)
            .expect("a long listing has a scrollbar");
        // Below the thumb, which sits at the top.
        click_at(&mut dialog, track.centre().0, track.y + track.h - 1.0);

        assert_eq!(dialog.visible_rows(H).start, capacity);
    }

    #[test]
    fn the_thumb_sits_at_the_end_when_the_list_is_at_its_end() {
        let mut dialog = FileDialog::open();
        dialog.set_entries(long_listing());
        for _ in 0..50 {
            dialog.handle_mouse(
                &MouseEvent {
                    x: W / 2.0,
                    y: H / 2.0,
                    kind: MouseEventKind::Scroll { dx: 0.0, dy: -3.0 },
                },
                W,
                H,
            );
        }

        let frame = dialog.frame(W, H);
        let track = frame
            .rect_of(|t| *t == DialogTarget::ScrollTrack)
            .expect("track");
        let thumb = frame
            .rect_of(|t| *t == DialogTarget::ScrollThumb)
            .expect("thumb");
        assert!(
            (thumb.y + thumb.h - (track.y + track.h)).abs() < 0.5,
            "a thumb held to a minimum height still has to reach the bottom: \
             {thumb:?} in {track:?}"
        );
    }

    #[test]
    fn keyboard_selection_scrolls_the_list_to_follow_it() {
        let mut dialog = FileDialog::open();
        dialog.set_entries(long_listing());
        let down = KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: crate::event::Modifiers::NONE,
            text: String::new(),
        };
        for _ in 0..29 {
            dialog.handle_event(&down, H);
        }

        assert_eq!(dialog.selected_index(), Some(29));
        let rows = dialog.visible_rows(H);
        assert!(
            rows.contains(&29),
            "walking off the bottom edge has to bring the list with it: {rows:?}"
        );
    }

    #[test]
    fn the_wheel_may_scroll_away_from_the_selection() {
        // Reveal is a consequence of *moving* the selection, not something
        // render re-imposes: if it were the latter, every wheel notch would be
        // undone by the next frame and the wheel would do nothing at all.
        let mut dialog = FileDialog::open();
        dialog.set_entries(long_listing());
        dialog.handle_event(
            &KeyEvent {
                key: Key::End,
                pressed: true,
                modifiers: crate::event::Modifiers::NONE,
                text: String::new(),
            },
            H,
        );
        assert_eq!(dialog.selected_index(), Some(29));

        for _ in 0..50 {
            dialog.handle_mouse(
                &MouseEvent {
                    x: W / 2.0,
                    y: H / 2.0,
                    kind: MouseEventKind::Scroll { dx: 0.0, dy: 3.0 },
                },
                W,
                H,
            );
        }

        assert_eq!(dialog.visible_rows(H).start, 0);
        assert_eq!(
            dialog.selected_index(),
            Some(29),
            "scrolling away from the selection must not change it"
        );
    }

    #[test]
    fn navigating_rewinds_the_scroll() {
        let mut dialog = FileDialog::open().with_initial_path("/a");
        dialog.set_entries(long_listing());
        for _ in 0..5 {
            dialog.handle_mouse(
                &MouseEvent {
                    x: W / 2.0,
                    y: H / 2.0,
                    kind: MouseEventKind::Scroll { dx: 0.0, dy: -3.0 },
                },
                W,
                H,
            );
        }
        assert!(dialog.visible_rows(H).start > 0);

        click(&mut dialog, DialogTarget::Up);
        dialog.set_entries(vec![file("only.txt")]);

        assert_eq!(dialog.visible_rows(H).start, 0);
        assert_eq!(dialog.selected_index(), None);
    }

    // --- Modality ---

    #[test]
    fn a_click_outside_the_dialog_is_swallowed() {
        let mut dialog = FileDialog::open().with_initial_path("/docs");
        dialog.set_entries(vec![file("a.txt")]);

        assert_eq!(
            click_at(&mut dialog, W + 40.0, H + 40.0),
            DialogAction::None,
            "a modal that let the window behind it be clicked would only look modal"
        );
        assert!(!dialog.is_cancelled(), "and must not dismiss itself either");
    }

    #[test]
    fn a_click_on_dead_chrome_is_swallowed() {
        let mut dialog = FileDialog::open().with_initial_path("/docs");
        dialog.set_entries(vec![file("a.txt")]);
        // The toolbar strip to the right of the address bar acts on nothing.
        assert_eq!(click_at(&mut dialog, W - 2.0, 2.0), DialogAction::None);
        assert_eq!(dialog.selected_index(), None);
    }

    #[test]
    fn every_control_the_frame_draws_is_reachable() {
        // A hit box recorded but clipped away is a control the user can see and
        // cannot press. `Frame` drops those, so asking the frame is the check.
        let mut dialog = FileDialog::save().with_initial_path("/docs");
        dialog.set_entries(long_listing());
        let frame = dialog.frame(W, H);

        for target in [
            DialogTarget::Back,
            DialogTarget::Forward,
            DialogTarget::Up,
            DialogTarget::AddressBar,
            DialogTarget::Shortcut(0),
            DialogTarget::Header(SortColumn::Name),
            DialogTarget::Header(SortColumn::Size),
            DialogTarget::Header(SortColumn::Modified),
            DialogTarget::Entry(0),
            DialogTarget::List,
            DialogTarget::ScrollTrack,
            DialogTarget::ScrollThumb,
            DialogTarget::FilenameInput,
            DialogTarget::Confirm,
            DialogTarget::Cancel,
            DialogTarget::Chrome,
        ] {
            assert!(
                frame.rect_of(|t| *t == target).is_some(),
                "{target:?} should be reachable"
            );
        }
        assert!(frame.is_balanced(), "every clip has to be closed");
    }

    #[test]
    fn the_address_bar_swallows_its_click_without_acting() {
        let mut dialog = FileDialog::open().with_initial_path("/home/user");

        assert_eq!(
            click(&mut dialog, DialogTarget::AddressBar),
            DialogAction::None
        );
        assert_eq!(
            dialog.current_path(),
            "/home/user",
            "it is not editable yet, but it must not fall through to the list"
        );
    }
}
