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

use crate::color::Color;
use crate::event::{Key, KeyEvent};
use crate::render::{FontWeightHint, RenderCommand};
use crate::style::CornerRadii;

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
        self.selected_index = None;
    }

    /// Navigate to the parent directory.
    pub fn navigate_up(&mut self) {
        let parent = parent_path(&self.current_path);
        if parent != self.current_path {
            self.navigate_to(&parent.to_string());
        }
    }

    /// Navigate backward in history (if available).
    pub fn navigate_back(&mut self) {
        if let Some(prev) = self.history_back.pop() {
            self.history_forward.push(self.current_path.clone());
            self.current_path = prev;
            self.selected_index = None;
        }
    }

    /// Navigate forward in history (if available).
    pub fn navigate_forward(&mut self) {
        if let Some(next) = self.history_forward.pop() {
            self.history_back.push(self.current_path.clone());
            self.current_path = next;
            self.selected_index = None;
        }
    }

    // --- Selection / Interaction ---

    /// Highlight the entry at `index` (single-click equivalent).
    pub fn select_entry(&mut self, index: usize) {
        if index < self.entries.len() {
            self.selected_index = Some(index);
            // In save mode, clicking a file fills the filename input.
            if self.mode == DialogMode::Save {
                if let Some(entry) = self.entries.get(index) {
                    if !entry.is_dir {
                        self.filename_input = entry.name.clone();
                    }
                }
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
                if let Some(idx) = self.selected_index {
                    if let Some(entry) = self.entries.get(idx) {
                        if entry.is_dir {
                            return Some(join_path(&self.current_path, &entry.name));
                        }
                    }
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
    pub fn handle_event(&mut self, event: &KeyEvent) -> DialogAction {
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
                    let entry_is_dir = self
                        .entries
                        .get(idx)
                        .map(|e| e.is_dir)
                        .unwrap_or(false);
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
                self.move_selection(-1);
                DialogAction::None
            }
            Key::Down => {
                self.move_selection(1);
                DialogAction::None
            }
            Key::Backspace if event.modifiers.alt => {
                self.navigate_back();
                if self.history_back.is_empty() {
                    DialogAction::None
                } else {
                    DialogAction::NavigatedTo(self.current_path.clone())
                }
            }
            Key::Backspace => {
                // Without modifiers in non-save mode: go to parent.
                if self.mode != DialogMode::Save || self.filename_input.is_empty() {
                    self.navigate_up();
                    DialogAction::NavigatedTo(self.current_path.clone())
                } else {
                    // In save mode with text: delete last char of filename input.
                    self.filename_input.pop();
                    DialogAction::None
                }
            }
            Key::Home => {
                if !self.entries.is_empty() {
                    self.selected_index = Some(0);
                }
                DialogAction::None
            }
            Key::End => {
                if !self.entries.is_empty() {
                    self.selected_index = Some(self.entries.len().saturating_sub(1));
                }
                DialogAction::None
            }
            _ => {
                // Text input for save-mode filename.
                if self.mode == DialogMode::Save {
                    if let Some(ch) = event.text {
                        if !ch.is_control() {
                            self.filename_input.push(ch);
                        }
                    }
                }
                DialogAction::None
            }
        }
    }

    // --- Rendering ---

    /// Produce render commands for the entire dialog at the given dimensions.
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Dialog background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Toolbar
        self.render_toolbar(&mut cmds, width);

        // Sidebar
        let content_top = TOOLBAR_HEIGHT;
        let content_height = height - TOOLBAR_HEIGHT - BOTTOM_BAR_HEIGHT;
        self.render_sidebar(&mut cmds, content_top, content_height);

        // File list
        let list_x = SIDEBAR_WIDTH;
        let list_width = width - SIDEBAR_WIDTH;
        self.render_file_list(&mut cmds, list_x, content_top, list_width, content_height);

        // Bottom bar (filename input for save, buttons)
        let bottom_y = height - BOTTOM_BAR_HEIGHT;
        self.render_bottom_bar(&mut cmds, bottom_y, width);

        cmds
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
                    let patterns: Vec<&str> =
                        filter.patterns.iter().map(|s| s.as_str()).collect();
                    entries.retain(|e| e.is_dir || matches_any_pattern(&e.name, &patterns));
                }
            }
        }

        // Sort: directories first, then by selected column.
        entries.sort_by(|a, b| {
            // Directories always come first.
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

        self.entries = entries;
        self.selected_index = None;
    }

    /// Toggle sort column. If already sorting by this column, flip direction.
    pub fn toggle_sort(&mut self, column: SortColumn) {
        if self.sort_by == column {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_by = column;
            self.sort_ascending = true;
        }
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
            if let Some(ext) = pattern.strip_prefix("*.") {
                if name.ends_with(&format!(".{ext}")) {
                    return name.clone();
                }
            }
        }

        // Append the first pattern's extension.
        if let Some(first) = filter.patterns.first() {
            if let Some(ext) = first.strip_prefix("*.") {
                return format!("{name}.{ext}");
            }
        }

        name.clone()
    }

    fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let len = self.entries.len();
        let current = self.selected_index.unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, (len as isize) - 1) as usize;
        self.selected_index = Some(next);
    }

    // --- Render sub-methods ---

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>, width: f32) {
        // Toolbar background
        cmds.push(RenderCommand::FillRect {
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

        // Back button
        let back_color = if self.history_back.is_empty() {
            COLOR_OVERLAY
        } else {
            COLOR_TEXT
        };
        cmds.push(RenderCommand::Text {
            x,
            y: btn_y + 4.0,
            text: String::from("<"),
            color: back_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        x += 24.0;

        // Forward button
        let fwd_color = if self.history_forward.is_empty() {
            COLOR_OVERLAY
        } else {
            COLOR_TEXT
        };
        cmds.push(RenderCommand::Text {
            x,
            y: btn_y + 4.0,
            text: String::from(">"),
            color: fwd_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        x += 24.0;

        // Up button
        cmds.push(RenderCommand::Text {
            x,
            y: btn_y + 4.0,
            text: String::from("^"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        x += 28.0;

        // Address bar
        let addr_width = width - x - PADDING;
        cmds.push(RenderCommand::FillRect {
            x,
            y: btn_y,
            width: addr_width,
            height: 24.0,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 6.0,
            y: btn_y + 5.0,
            text: self.current_path.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(addr_width - 12.0),
        });
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>, top: f32, height: f32) {
        // Sidebar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: SIDEBAR_WIDTH,
            height,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let mut y = top + PADDING;
        for qa in &self.quick_access {
            cmds.push(RenderCommand::Text {
                x: PADDING + 4.0,
                y,
                text: qa.label.clone(),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0 - 4.0),
            });
            y += ROW_HEIGHT;
        }
    }

    fn render_file_list(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        top: f32,
        width: f32,
        height: f32,
    ) {
        // Clip the file list area
        cmds.push(RenderCommand::PushClip {
            x,
            y: top,
            width,
            height,
        });

        // Column headers
        let header_y = top;
        cmds.push(RenderCommand::FillRect {
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

        // Header labels
        cmds.push(RenderCommand::Text {
            x: name_col_x,
            y: header_y + 6.0,
            text: String::from("Name"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: size_col_x,
            y: header_y + 6.0,
            text: String::from("Size"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: date_col_x,
            y: header_y + 6.0,
            text: String::from("Modified"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Sort indicator on active column
        let indicator_x = match self.sort_by {
            SortColumn::Name => name_col_x + 36.0,
            SortColumn::Size => size_col_x + 30.0,
            SortColumn::Modified => date_col_x + 54.0,
        };
        let indicator = if self.sort_ascending { "v" } else { "^" };
        cmds.push(RenderCommand::Text {
            x: indicator_x,
            y: header_y + 6.0,
            text: String::from(indicator),
            color: COLOR_OVERLAY,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // File entries
        let entries_top = top + ROW_HEIGHT;
        for (i, entry) in self.entries.iter().enumerate() {
            let row_y = entries_top + (i as f32) * ROW_HEIGHT;

            // Stop rendering if below visible area (simple culling).
            if row_y > top + height {
                break;
            }

            // Selection highlight
            if self.selected_index == Some(i) {
                cmds.push(RenderCommand::FillRect {
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
            cmds.push(RenderCommand::Text {
                x: x + PADDING,
                y: row_y + 6.0,
                text: String::from(icon_char),
                color: icon_color,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Name
            let name_color = if entry.is_dir {
                COLOR_YELLOW
            } else {
                COLOR_TEXT
            };
            let max_name_width = size_col_x - name_col_x - PADDING;
            cmds.push(RenderCommand::Text {
                x: name_col_x,
                y: row_y + 6.0,
                text: entry.name.clone(),
                color: name_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_name_width),
            });

            // Size (human-readable, only for files)
            if !entry.is_dir {
                cmds.push(RenderCommand::Text {
                    x: size_col_x,
                    y: row_y + 6.0,
                    text: format_size(entry.size),
                    color: COLOR_SUBTEXT,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            // Modified timestamp (simplified display)
            cmds.push(RenderCommand::Text {
                x: date_col_x,
                y: row_y + 6.0,
                text: format_timestamp(entry.modified_timestamp),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        cmds.push(RenderCommand::PopClip);
    }

    fn render_bottom_bar(&self, cmds: &mut Vec<RenderCommand>, y: f32, width: f32) {
        // Bottom bar background
        cmds.push(RenderCommand::FillRect {
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
            cmds.push(RenderCommand::FillRect {
                x: PADDING,
                y: input_y,
                width: input_width,
                height: 28.0,
                color: COLOR_SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::StrokeRect {
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
            cmds.push(RenderCommand::Text {
                x: PADDING + 6.0,
                y: input_y + 7.0,
                text: display_text,
                color: text_color,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(input_width - 12.0),
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
        cmds.push(RenderCommand::FillRect {
            x: confirm_x,
            y: input_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: confirm_bg,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
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
        });

        // Cancel button
        cmds.push(RenderCommand::FillRect {
            x: cancel_x,
            y: input_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        cmds.push(RenderCommand::Text {
            x: cancel_x + (BUTTON_WIDTH - 42.0) / 2.0,
            y: input_y + 8.0,
            text: String::from("Cancel"),
            color: COLOR_RED,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

// --- Free functions (utilities) ---

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
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;

    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Format a Unix timestamp into a simplified date string.
/// Full date formatting would depend on a time library; this provides
/// a basic representation suitable for display.
fn format_timestamp(epoch_secs: u64) -> String {
    if epoch_secs == 0 {
        return String::from("--");
    }
    // Simple epoch-days calculation (no timezone, no leap-second precision).
    let days = epoch_secs / 86400;
    let years_approx = days / 365;
    let year = 1970 + years_approx;
    let remaining_days = days - (years_approx * 365);
    let month = (remaining_days / 30).min(11) + 1;
    let day = (remaining_days % 30) + 1;
    format!("{year:04}-{month:02}-{day:02}")
}

/// Check whether a filename matches any of the given glob patterns.
/// Supports simple `*.ext` patterns only (not full glob).
fn matches_any_pattern(filename: &str, patterns: &[&str]) -> bool {
    for pattern in patterns {
        if *pattern == "*" || *pattern == "*.*" {
            return true;
        }
        if let Some(ext) = pattern.strip_prefix("*.") {
            if filename.ends_with(&format!(".{ext}")) {
                return true;
            }
        }
        // Exact match fallback
        if *pattern == filename {
            return true;
        }
    }
    false
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

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
        dialog.set_entries(vec![
            DirEntry {
                name: String::from("file.txt"),
                is_dir: false,
                size: 100,
                modified_timestamp: 1000,
                extension: String::from("txt"),
            },
        ]);

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
        assert_eq!(action, DialogAction::Selected(String::from("/docs/notes.txt")));
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
            text: None,
        };
        let action = dialog.handle_event(&event);
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
            text: None,
        };
        dialog.handle_event(&down);
        assert_eq!(dialog.selected_index(), Some(1));

        dialog.handle_event(&down);
        assert_eq!(dialog.selected_index(), Some(2));

        // Should clamp at the end.
        dialog.handle_event(&down);
        assert_eq!(dialog.selected_index(), Some(2));

        let up = KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: crate::event::Modifiers::NONE,
            text: None,
        };
        dialog.handle_event(&up);
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
}
