//! Slate OS Markdown Editor
//!
//! A split-view markdown editor with live preview, syntax highlighting,
//! and a full-featured markdown parser. Features include:
//!
//! - Split view: source editor (left), rendered preview (right), toggleable
//! - Markdown parser: headings, bold/italic/strikethrough, links, images,
//!   ordered/unordered lists, code blocks, inline code, blockquotes,
//!   horizontal rules, tables (with alignment), task lists
//! - Syntax highlighting in the source editor
//! - Live preview re-renders on every keystroke
//! - Multi-tab document editing
//! - File operations: new, open, save, save as
//! - Word/character/line count and reading time estimate in status bar
//! - Table of contents sidebar (generated from headings, clickable)
//! - Find and replace in source
//! - Export to HTML
//! - Insert helpers: toolbar buttons for common markdown constructs
//! - Template system: blank, meeting notes, project README, blog post, changelog
//! - Auto-save with configurable interval
//! - Undo/redo
//! - Line numbers in editor
//! - Scroll sync between editor and preview
//! - Outline/structure view showing document hierarchy
//! - Keyboard shortcuts: Ctrl+B bold, Ctrl+I italic, Ctrl+K link,
//!   Ctrl+Shift+K code block
//! - Dark theme (Catppuccin Mocha) throughout
//!
//! Uses the guitk library for UI rendering.

use guitk::color::Color;
use guitk::render::{FontFamily, FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::tabs::Tabs;
use guitk::text;
use guitk::textfind::{self, Case};

use diffcore::{
    ConflictChoice, DiskChange, FileSync, MergeOutcome, MergeReview, ThreeWayMerge,
    normalize_content,
};

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

/// Catppuccin Mocha base background color.
const BASE: Color = Color::from_hex(0x1E1E2E);
/// Catppuccin Mocha mantle (darker background).
const MANTLE: Color = Color::from_hex(0x181825);
/// Catppuccin Mocha crust (darkest background).
const CRUST: Color = Color::from_hex(0x11111B);
/// Catppuccin Mocha surface level 0.
const SURFACE0: Color = Color::from_hex(0x313244);
/// Catppuccin Mocha surface level 1.
const SURFACE1: Color = Color::from_hex(0x45475A);
/// Catppuccin Mocha surface level 2.
const SURFACE2: Color = Color::from_hex(0x585B70);
/// Catppuccin Mocha primary text color.
const TEXT: Color = Color::from_hex(0xCDD6F4);
/// Catppuccin Mocha subtext0 (dimmer text).
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
/// Catppuccin Mocha subtext1 (slightly dimmer text).
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
/// Catppuccin Mocha blue accent.
const BLUE: Color = Color::from_hex(0x89B4FA);
/// Catppuccin Mocha green accent.
const GREEN: Color = Color::from_hex(0xA6E3A1);
/// Catppuccin Mocha red accent.
const RED: Color = Color::from_hex(0xF38BA8);
/// Catppuccin Mocha yellow accent.
const YELLOW: Color = Color::from_hex(0xF9E2AF);
/// Catppuccin Mocha peach accent.
const PEACH: Color = Color::from_hex(0xFAB387);
/// Catppuccin Mocha lavender accent.
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
/// Catppuccin Mocha overlay0 (muted foreground).
const OVERLAY0: Color = Color::from_hex(0x6C7086);

/// Default font size for the editor.
const EDITOR_FONT_SIZE: f32 = 14.0;
/// Font size for line numbers.
const LINE_NUMBER_FONT_SIZE: f32 = 12.0;
/// Width of the line number gutter in pixels.
const GUTTER_WIDTH: f32 = 50.0;
/// Height of each editor line in pixels.
const LINE_HEIGHT: f32 = 20.0;
/// Height of the toolbar in pixels.
const TOOLBAR_HEIGHT: f32 = 36.0;
/// Height of the tab bar in pixels.
const TAB_BAR_HEIGHT: f32 = 32.0;
/// Height of the status bar in pixels.
const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Font size of the status bar at the bottom of the window.
const STATUS_FONT_SIZE: f32 = 11.0;
/// Font size of toolbar buttons and document tabs.
const TOOLBAR_FONT_SIZE: f32 = 12.0;
/// Font size of the compact buttons in the find/replace panel.
const SMALL_BUTTON_FONT_SIZE: f32 = 11.0;
/// Width of the table of contents sidebar.
const TOC_SIDEBAR_WIDTH: f32 = 200.0;
/// Padding inside the editor area.
const EDITOR_PADDING: f32 = 8.0;
/// Padding inside the preview area.
const PREVIEW_PADDING: f32 = 16.0;
/// Width of one cell of the source pane's character grid.
///
/// The source pane is a code view: it is laid out on columns, so it keeps the
/// grid. But the cell has to come from the face — this was a hardcoded 8.4,
/// which matched the built-in face at 14 px and nothing else, so with any
/// other face the cursor, the selection band and the find highlights all
/// drifted away from the text they mark.
///
/// The *preview* pane is prose, not a grid, and measures each run instead.
///
/// The cell was then `text::digit_advance` — right that the face should decide
/// it, wrong about which face. A digit's advance in the proportional UI face is
/// a cell only digits fit, and the source pane holds prose and Markdown syntax:
/// at 14 px a digit is 8.1 px while `'W'` is 14.1 px, so the caret placed by
/// `col_x` drifted further left of its character with every wide glyph on the
/// line, and the selection band drifted with it. A grid needs a face where
/// every glyph advances the same distance — `text::cell_advance`.
fn char_width() -> f32 {
    text::cell_advance(EDITOR_FONT_SIZE, FontWeightHint::Regular)
}

/// x of byte offset `col` within `line`, given the pane's left edge `text_x`.
///
/// The document stores cursor and selection positions as byte offsets on
/// character boundaries, which is the right model for a text buffer — but the
/// grid counts *cells*, and a multi-byte character occupies one cell, not two
/// or three. Multiplying the byte offset by the cell width put the caret
/// several columns right of the character it precedes on any line holding
/// non-ASCII text, and stretched the selection band with it.
fn col_x(line: &str, col: usize, text_x: f32) -> f32 {
    let prefix = line.get(..col.min(line.len())).unwrap_or(line);
    text_x + prefix.chars().count() as f32 * char_width()
}
/// Minimum width for the find/replace panel.
const FIND_PANEL_HEIGHT: f32 = 64.0;
/// Maximum number of undo steps.
const MAX_UNDO_HISTORY: usize = 500;
/// Words per minute for reading time estimate.
const READING_WPM: f32 = 238.0;
/// Auto-save interval in seconds (default).
const DEFAULT_AUTOSAVE_INTERVAL: u64 = 60;

// ============================================================================
// View mode
// ============================================================================

/// Which panels are visible in the main content area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// Only the source editor is shown.
    EditorOnly,
    /// Both source editor and rendered preview side-by-side.
    Split,
    /// Only the rendered preview is shown.
    PreviewOnly,
}

impl ViewMode {
    /// Cycle to the next view mode.
    pub fn next(self) -> Self {
        match self {
            Self::EditorOnly => Self::Split,
            Self::Split => Self::PreviewOnly,
            Self::PreviewOnly => Self::EditorOnly,
        }
    }

    /// Display name for the view mode.
    pub fn label(self) -> &'static str {
        match self {
            Self::EditorOnly => "Editor",
            Self::Split => "Split",
            Self::PreviewOnly => "Preview",
        }
    }
}

// ============================================================================
// Template system
// ============================================================================

/// Available document templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    /// Empty document.
    Blank,
    /// Meeting notes with sections for attendees, agenda, notes, actions.
    MeetingNotes,
    /// Project README with standard sections.
    ProjectReadme,
    /// Blog post with frontmatter-style header.
    BlogPost,
    /// Changelog in Keep-a-Changelog format.
    Changelog,
}

impl Template {
    /// Return the template content string.
    pub fn content(self) -> &'static str {
        match self {
            Self::Blank => "",
            Self::MeetingNotes => concat!(
                "# Meeting Notes\n\n",
                "**Date:** YYYY-MM-DD\n",
                "**Time:** HH:MM\n",
                "**Location:** \n\n",
                "## Attendees\n\n",
                "- [ ] Person 1\n",
                "- [ ] Person 2\n",
                "- [ ] Person 3\n\n",
                "## Agenda\n\n",
                "1. Topic 1\n",
                "2. Topic 2\n",
                "3. Topic 3\n\n",
                "## Notes\n\n",
                "\n\n",
                "## Action Items\n\n",
                "- [ ] Action 1 — **Owner:** \n",
                "- [ ] Action 2 — **Owner:** \n\n",
                "## Next Meeting\n\n",
                "**Date:** YYYY-MM-DD\n",
            ),
            Self::ProjectReadme => concat!(
                "# Project Name\n\n",
                "Brief description of the project.\n\n",
                "## Features\n\n",
                "- Feature 1\n",
                "- Feature 2\n",
                "- Feature 3\n\n",
                "## Installation\n\n",
                "```bash\n",
                "# Clone the repository\n",
                "git clone https://example.com/project.git\n",
                "cd project\n\n",
                "# Install dependencies\n",
                "make install\n",
                "```\n\n",
                "## Usage\n\n",
                "```bash\n",
                "project --help\n",
                "```\n\n",
                "## Configuration\n\n",
                "| Option | Default | Description |\n",
                "|--------|---------|-------------|\n",
                "| `port` | `8080` | Server port |\n",
                "| `host` | `localhost` | Bind address |\n\n",
                "## Contributing\n\n",
                "1. Fork the repository\n",
                "2. Create a feature branch\n",
                "3. Submit a pull request\n\n",
                "## License\n\n",
                "MIT License\n",
            ),
            Self::BlogPost => concat!(
                "# Blog Post Title\n\n",
                "*Published: YYYY-MM-DD*\n",
                "*Author: Your Name*\n",
                "*Tags: tag1, tag2, tag3*\n\n",
                "---\n\n",
                "## Introduction\n\n",
                "Opening paragraph that hooks the reader.\n\n",
                "## Main Content\n\n",
                "### Section 1\n\n",
                "Content here.\n\n",
                "### Section 2\n\n",
                "Content here.\n\n",
                "> A notable quote or callout.\n\n",
                "### Section 3\n\n",
                "Content here.\n\n",
                "## Conclusion\n\n",
                "Summary and closing thoughts.\n\n",
                "---\n\n",
                "*Thanks for reading!*\n",
            ),
            Self::Changelog => concat!(
                "# Changelog\n\n",
                "All notable changes to this project will be documented in this file.\n\n",
                "The format is based on [Keep a Changelog](https://keepachangelog.com/).\n\n",
                "## [Unreleased]\n\n",
                "### Added\n\n",
                "- New feature 1\n\n",
                "### Changed\n\n",
                "- Updated behavior\n\n",
                "### Fixed\n\n",
                "- Bug fix\n\n",
                "## [1.0.0] - YYYY-MM-DD\n\n",
                "### Added\n\n",
                "- Initial release\n",
            ),
        }
    }

    /// Display name for the template.
    pub fn label(self) -> &'static str {
        match self {
            Self::Blank => "Blank",
            Self::MeetingNotes => "Meeting Notes",
            Self::ProjectReadme => "Project README",
            Self::BlogPost => "Blog Post",
            Self::Changelog => "Changelog",
        }
    }

    /// Return all available templates.
    pub fn all() -> &'static [Template] {
        &[
            Template::Blank,
            Template::MeetingNotes,
            Template::ProjectReadme,
            Template::BlogPost,
            Template::Changelog,
        ]
    }
}

// ============================================================================
// Edit actions for undo/redo
// ============================================================================

/// A reversible edit action for the undo/redo system.
#[derive(Clone, Debug)]
pub enum EditAction {
    /// Text was inserted at a position.
    Insert {
        /// Line index (0-based).
        line: usize,
        /// Column index (0-based byte offset).
        col: usize,
        /// The inserted text.
        text: String,
    },
    /// Text was deleted from a position.
    Delete {
        /// Line index (0-based).
        line: usize,
        /// Column index (0-based byte offset).
        col: usize,
        /// The deleted text.
        text: String,
    },
    /// A line was inserted at the given index.
    InsertLine {
        /// Line index where the new line was inserted.
        line: usize,
        /// Content of the inserted line.
        text: String,
    },
    /// A line was removed at the given index.
    DeleteLine {
        /// Line index that was deleted.
        line: usize,
        /// Content of the deleted line.
        text: String,
    },
    /// Multiple actions grouped as one undo step.
    Batch {
        /// The individual actions in order.
        actions: Vec<EditAction>,
    },
}

// ============================================================================
// Document
// ============================================================================

/// A single markdown document with editing state.
pub struct Document {
    /// Lines of text in the document.
    pub lines: Vec<String>,
    /// File path (None for untitled documents).
    pub path: Option<PathBuf>,
    /// Display name (filename or "Untitled").
    pub name: String,
    /// Whether the document has unsaved changes.
    pub modified: bool,
    /// Cursor line (0-based).
    pub cursor_line: usize,
    /// Cursor column (0-based byte offset).
    pub cursor_col: usize,
    /// Selection anchor (line, col) for text selection.
    pub selection_anchor: Option<(usize, usize)>,
    /// First visible line (vertical scroll offset).
    pub scroll_line: usize,
    /// Horizontal scroll offset in characters.
    pub scroll_col: usize,
    /// Preview scroll offset in pixels.
    pub preview_scroll: f32,
    /// Undo history stack.
    pub undo_stack: VecDeque<EditAction>,
    /// Redo history stack.
    pub redo_stack: VecDeque<EditAction>,
    /// Seconds since the last save (for auto-save tracking).
    pub seconds_since_save: u64,
    /// External-change tracker: records the last loaded/saved content and mtime
    /// so edits made to the file by other programs can be detected and merged.
    pub sync: FileSync,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

/// Clamp a byte offset into `line` to the nearest character boundary at or
/// before it.
///
/// Every column in this editor -- `cursor_col`, the selection anchor, the
/// columns recorded in undo actions -- is a **byte** offset into a line. The
/// clamp that kept such an offset in range was `.min(line.len())`, and a byte
/// length is the wrong bound for it: it keeps the offset inside the line but
/// says nothing about whether it lands *on* a character.
///
/// That gap is reachable by pressing Down. `move_cursor_down` carries the
/// column across to a line that may be shorter in bytes and differently shaped:
/// from column 1 of `"abc"` onto `"\u{65e5}x"`, `.min(4)` leaves 1, which is
/// inside the kanji. The next Backspace, Delete, insert or selection then
/// sliced there and aborted the editor -- with the user's unsaved document in
/// it, which is the worst place in this app to panic.
///
/// Rounding *down* is the right direction: it puts the cursor at the start of
/// the character it landed in, which is where a user who pressed Down onto a
/// wide character expects to be, and it can never run past the end of a line.
/// For an all-ASCII document it returns exactly what `.min(line.len())` did.
fn clamp_col(line: &str, byte: usize) -> usize {
    if byte >= line.len() {
        return line.len();
    }
    let mut i = byte;
    // Byte 0 is always a character boundary, so this terminates.
    while !line.is_char_boundary(i) {
        i = i.saturating_sub(1);
    }
    i
}

impl Document {
    /// Create a new empty document.
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            path: None,
            name: "Untitled".to_string(),
            modified: false,
            cursor_line: 0,
            cursor_col: 0,
            selection_anchor: None,
            scroll_line: 0,
            scroll_col: 0,
            preview_scroll: 0.0,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            seconds_since_save: 0,
            sync: FileSync::new(),
        }
    }

    /// Create a document from a template.
    pub fn from_template(template: Template) -> Self {
        let content = template.content();
        let lines: Vec<String> = if content.is_empty() {
            vec![String::new()]
        } else {
            content.lines().map(|l| l.to_string()).collect()
        };
        Self {
            lines,
            path: None,
            name: format!("{} (new)", template.label()),
            modified: false,
            cursor_line: 0,
            cursor_col: 0,
            selection_anchor: None,
            scroll_line: 0,
            scroll_col: 0,
            preview_scroll: 0.0,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            seconds_since_save: 0,
            sync: FileSync::new(),
        }
    }

    /// Load a document from a file path.
    pub fn from_file(path: &std::path::Path) -> std::io::Result<Self> {
        let content = fs::read_to_string(path)?;
        let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());
        // Record the load-time snapshot (LF-normalized) and mtime so external
        // edits can be detected and three-way merged against this ancestor.
        let mut sync = FileSync::new();
        sync.record(path, normalize_content(&content));
        Ok(Self {
            lines,
            path: Some(path.to_path_buf()),
            name,
            modified: false,
            cursor_line: 0,
            cursor_col: 0,
            selection_anchor: None,
            scroll_line: 0,
            scroll_col: 0,
            preview_scroll: 0.0,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            seconds_since_save: 0,
            sync,
        })
    }

    /// Save the document to its current path. Returns an error if no path is set.
    pub fn save(&mut self) -> std::io::Result<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "No file path set"))?
            .clone();
        let content = self.lines.join("\n");
        fs::write(&path, &content)?;
        self.modified = false;
        self.seconds_since_save = 0;
        // Refresh the merge ancestor/mtime so our own write is not mistaken for
        // an external change on the next check.
        self.sync.record(&path, content);
        Ok(())
    }

    /// Save the document to a specific path.
    pub fn save_as(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        let content = self.lines.join("\n");
        fs::write(path, &content)?;
        self.path = Some(path.to_path_buf());
        self.name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());
        self.modified = false;
        self.seconds_since_save = 0;
        self.sync.record(path, content);
        Ok(())
    }

    /// Get the full text content of the document.
    pub fn full_text(&self) -> String {
        self.lines.join("\n")
    }

    // ======================================================================
    // External-change detection & three-way merge
    // ======================================================================

    /// Check whether the file backing this document changed on disk since it was
    /// last loaded or saved. Delegates to the shared [`FileSync`] tracker.
    #[must_use]
    pub fn disk_changed(&self) -> DiskChange {
        match self.path.as_ref() {
            Some(path) => self.sync.changed(path),
            None => DiskChange::Unchanged,
        }
    }

    /// Dismiss an external change, keeping the current buffer as-is. Records the
    /// disk's current mtime so the same edit is not re-reported; the merge
    /// ancestor is left intact for a later merge.
    pub fn keep_current(&mut self) {
        if let Some(path) = self.path.clone() {
            self.sync.touch(&path);
        }
    }

    /// Replace the buffer with the on-disk content, discarding local edits.
    pub fn reload_from_disk(&mut self, disk: &str) {
        self.set_lines_from_text(disk);
        self.modified = false;
        self.seconds_since_save = 0;
        if let Some(path) = self.path.clone() {
            self.sync.record(&path, disk.to_string());
        } else {
            self.sync.base = Some(disk.to_string());
        }
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Compute the three-way merge of the current buffer against `disk`.
    #[must_use]
    pub fn merge_preview(&self, disk: &str) -> ThreeWayMerge {
        self.sync.merge(&self.full_text(), disk)
    }

    /// Auto-merge the on-disk changes into the buffer. Clean merges become the
    /// merged text; conflicts fill the buffer with Git-style markers for manual
    /// resolution. The buffer is marked modified and the ancestor advances to
    /// `disk` in both cases.
    pub fn merge_from_disk(&mut self, disk: &str) -> MergeOutcome {
        let merge = self.merge_preview(disk);
        let (text, outcome) = match merge.clean_merge() {
            Some(clean) => (clean, MergeOutcome::Clean),
            None => (
                merge.text_with_markers(&self.name, "disk"),
                MergeOutcome::Conflicted {
                    conflicts: merge.conflict_count(),
                },
            ),
        };
        self.apply_merged(&text, disk);
        outcome
    }

    /// Apply an already-resolved merge result to the buffer. `disk` becomes the
    /// new merge ancestor.
    pub fn apply_merged(&mut self, merged: &str, disk: &str) {
        self.set_lines_from_text(merged);
        self.modified = true;
        if let Some(path) = self.path.clone() {
            self.sync.record(&path, disk.to_string());
        } else {
            self.sync.base = Some(disk.to_string());
        }
    }

    /// Replace the buffer's lines from LF-normalized `text`, clamping the cursor.
    fn set_lines_from_text(&mut self, text: &str) {
        let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        self.lines = lines;
        self.selection_anchor = None;
        let last_line = self.lines.len().saturating_sub(1);
        if self.cursor_line > last_line {
            self.cursor_line = last_line;
        }
        self.cursor_col = self
            .lines
            .get(self.cursor_line)
            .map_or(0, |line| clamp_col(line, self.cursor_col));
        if self.scroll_line > last_line {
            self.scroll_line = last_line;
        }
    }

    /// Count the number of words in the document.
    pub fn word_count(&self) -> usize {
        self.lines
            .iter()
            .map(|line| line.split_whitespace().count())
            .sum()
    }

    /// Count the total number of characters in the document.
    pub fn char_count(&self) -> usize {
        let line_chars: usize = self.lines.iter().map(|line| line.len()).sum();
        // Add newlines between lines.
        let newlines = if self.lines.is_empty() {
            0
        } else {
            self.lines.len() - 1
        };
        line_chars + newlines
    }

    /// Estimate reading time in minutes based on word count.
    pub fn reading_time_minutes(&self) -> f32 {
        let words = self.word_count() as f32;
        words / READING_WPM
    }

    /// Push an edit action onto the undo stack and clear the redo stack.
    pub fn push_undo(&mut self, action: EditAction) {
        if self.undo_stack.len() >= MAX_UNDO_HISTORY {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(action);
        self.redo_stack.clear();
        self.modified = true;
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, ch: char) {
        let line = self.cursor_line;
        let Some(text) = self.lines.get_mut(line) else {
            return;
        };
        let col = clamp_col(text, self.cursor_col);
        text.insert(col, ch);
        self.cursor_col = col.saturating_add(ch.len_utf8());
        self.push_undo(EditAction::Insert {
            line,
            col,
            text: ch.to_string(),
        });
    }

    /// Insert a string at the cursor position (may contain newlines).
    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let mut actions = Vec::new();

        for ch in text.chars() {
            if ch == '\n' {
                self.insert_newline_internal(&mut actions);
            } else {
                let line = self.cursor_line;
                let Some(line_text) = self.lines.get_mut(line) else {
                    continue;
                };
                let col = clamp_col(line_text, self.cursor_col);
                line_text.insert(col, ch);
                self.cursor_col = col.saturating_add(ch.len_utf8());
                actions.push(EditAction::Insert {
                    line,
                    col,
                    text: ch.to_string(),
                });
            }
        }

        // One action undoes on its own; several must undo together, or a
        // pasted paragraph would take one Ctrl+Z per character.
        match actions.len() {
            0 => {}
            1 => {
                if let Some(action) = actions.into_iter().next() {
                    self.push_undo(action);
                }
            }
            _ => self.push_undo(EditAction::Batch { actions }),
        }
    }

    /// Split the current line at the cursor, recording the action.
    ///
    /// The split point is taken from the line's own text rather than trusted
    /// from `cursor_col`, which can name a byte inside a character after an
    /// edit that shortened the line.
    fn split_line_at_cursor(&mut self) -> Option<EditAction> {
        let text = self.lines.get_mut(self.cursor_line)?;
        let col = clamp_col(text, self.cursor_col);
        let remainder = text.get(col..).unwrap_or("").to_string();
        text.truncate(col);
        self.cursor_line = self.cursor_line.saturating_add(1);
        self.cursor_col = 0;
        // `insert` at `len` is a push; the index cannot exceed it, because the
        // line it came from is in the vector.
        self.lines
            .insert(self.cursor_line.min(self.lines.len()), remainder.clone());
        Some(EditAction::InsertLine {
            line: self.cursor_line,
            text: remainder,
        })
    }

    /// Insert a newline at the cursor, splitting the current line.
    fn insert_newline_internal(&mut self, actions: &mut Vec<EditAction>) {
        if let Some(action) = self.split_line_at_cursor() {
            actions.push(action);
        }
    }

    /// Insert a newline at the cursor position (public, single-action undo).
    pub fn insert_newline(&mut self) {
        if let Some(action) = self.split_line_at_cursor() {
            self.push_undo(action);
        }
    }

    /// Delete the character before the cursor (backspace).
    pub fn delete_backward(&mut self) {
        let line = self.cursor_line;
        let col = self.cursor_col;

        if col > 0 {
            let Some(text) = self.lines.get_mut(line) else {
                return;
            };
            let clamped_col = clamp_col(text, col);
            // Find the previous character boundary. `..clamped_col` is in
            // range and on a boundary because `clamp_col` guarantees both.
            let Some(prev) = text
                .get(..clamped_col)
                .and_then(|head| head.char_indices().next_back())
            else {
                return;
            };
            let (prev_boundary, _) = prev;
            let deleted = text
                .get(prev_boundary..clamped_col)
                .unwrap_or("")
                .to_string();
            text.replace_range(prev_boundary..clamped_col, "");
            self.cursor_col = prev_boundary;
            self.push_undo(EditAction::Delete {
                line,
                col: prev_boundary,
                text: deleted,
            });
        } else if let Some(prev_line) = line.checked_sub(1)
            && line < self.lines.len()
        {
            // Merge with previous line.
            let current_text = self.lines.remove(line);
            self.cursor_line = prev_line;
            if let Some(target) = self.lines.get_mut(prev_line) {
                self.cursor_col = target.len();
                target.push_str(&current_text);
            }
            self.push_undo(EditAction::DeleteLine {
                line,
                text: current_text,
            });
        }
    }

    /// Delete the character at the cursor (delete key).
    pub fn delete_forward(&mut self) {
        let line = self.cursor_line;
        let col = self.cursor_col;

        let Some(text) = self.lines.get_mut(line) else {
            return;
        };
        let current_len = text.len();
        let clamped_col = clamp_col(text, col);

        if clamped_col < current_len {
            // Delete the character at the cursor. `nth(1)` is the boundary
            // *after* it; running out means the cursor is on the last one.
            let next_boundary = text
                .get(clamped_col..)
                .and_then(|tail| tail.char_indices().nth(1))
                .map_or(current_len, |(i, _)| clamped_col.saturating_add(i));
            let deleted = text
                .get(clamped_col..next_boundary)
                .unwrap_or("")
                .to_string();
            text.replace_range(clamped_col..next_boundary, "");
            self.push_undo(EditAction::Delete {
                line,
                col: clamped_col,
                text: deleted,
            });
        } else {
            // Merge with the next line, if there is one.
            let next = line.saturating_add(1);
            if next < self.lines.len() {
                let next_text = self.lines.remove(next);
                if let Some(target) = self.lines.get_mut(line) {
                    target.push_str(&next_text);
                }
                self.push_undo(EditAction::DeleteLine {
                    line: next,
                    text: next_text,
                });
            }
        }
    }

    /// Undo the most recent edit action.
    pub fn undo(&mut self) {
        if let Some(action) = self.undo_stack.pop_back() {
            self.apply_undo(&action);
            self.redo_stack.push_back(action);
            self.modified = true;
        }
    }

    /// Redo the most recently undone action.
    pub fn redo(&mut self) {
        if let Some(action) = self.redo_stack.pop_back() {
            self.apply_redo(&action);
            self.undo_stack.push_back(action);
            self.modified = true;
        }
    }

    /// Remove `text.len()` bytes from line `line` starting at `col`, and put
    /// the cursor where they were.
    ///
    /// Undoing an insert and redoing a delete are the same operation, so they
    /// share it — as do the two below. An undo stack outlives the text it
    /// describes (a reload, or a merge from disk, replaces every line while
    /// leaving the stack alone), so each of these validates rather than
    /// trusting the recorded position.
    fn erase_at(&mut self, line: usize, col: usize, len: usize) {
        let Some(text) = self.lines.get_mut(line) else {
            return;
        };
        let start = clamp_col(text, col);
        let end = clamp_col(text, start.saturating_add(len));
        text.replace_range(start..end, "");
        self.cursor_line = line;
        self.cursor_col = start;
    }

    /// Put `insert` back into line `line` at `col`, and put the cursor after it.
    fn restore_at(&mut self, line: usize, col: usize, insert: &str) {
        let Some(text) = self.lines.get_mut(line) else {
            return;
        };
        let col = clamp_col(text, col);
        text.insert_str(col, insert);
        self.cursor_line = line;
        self.cursor_col = col.saturating_add(insert.len());
    }

    /// Join line `line` back onto the one above it, undoing a line split.
    fn join_onto_previous(&mut self, line: usize) {
        let Some(prev) = line.checked_sub(1) else {
            return;
        };
        if line >= self.lines.len() {
            return;
        }
        let removed = self.lines.remove(line);
        self.cursor_line = prev;
        if let Some(target) = self.lines.get_mut(prev) {
            self.cursor_col = target.len();
            target.push_str(&removed);
        }
    }

    /// Apply an undo action (reverse the edit).
    fn apply_undo(&mut self, action: &EditAction) {
        match action {
            EditAction::Insert { line, col, text } => self.erase_at(*line, *col, text.len()),
            EditAction::Delete { line, col, text } => self.restore_at(*line, *col, text),
            EditAction::InsertLine { line, text: _ } => self.join_onto_previous(*line),
            EditAction::DeleteLine { line, text } => {
                if *line <= self.lines.len() {
                    self.lines.insert(*line, text.clone());
                    self.cursor_line = *line;
                    self.cursor_col = 0;
                }
            }
            EditAction::Batch { actions } => {
                // Undo in reverse order.
                for a in actions.iter().rev() {
                    self.apply_undo(a);
                }
            }
        }
    }

    /// Apply a redo action (re-apply the edit).
    fn apply_redo(&mut self, action: &EditAction) {
        match action {
            EditAction::Insert { line, col, text } => self.restore_at(*line, *col, text),
            EditAction::Delete { line, col, text } => self.erase_at(*line, *col, text.len()),
            EditAction::InsertLine { line, text } => {
                if *line <= self.lines.len() {
                    self.lines.insert(*line, text.clone());
                    self.cursor_line = *line;
                    self.cursor_col = 0;
                }
            }
            EditAction::DeleteLine { line, text: _ } => {
                if *line < self.lines.len() {
                    self.lines.remove(*line);
                    self.cursor_line = (*line).min(self.lines.len().saturating_sub(1));
                    self.cursor_col = 0;
                }
            }
            EditAction::Batch { actions } => {
                for a in actions {
                    self.apply_redo(a);
                }
            }
        }
    }

    /// The text of line `n`, or `""` if the document has no such line.
    ///
    /// The cursor, the selection anchor, the scroll position and every entry
    /// on the undo stack hold line numbers, and all of them outlive the lines
    /// they refer to: a reload from disk, a three-way merge or an undo replaces
    /// the whole buffer. Reading a line that is no longer there is a
    /// misdrawn frame; indexing one is the loss of every unsaved document in
    /// the editor.
    fn line_text(&self, n: usize) -> &str {
        self.lines.get(n).map_or("", String::as_str)
    }

    /// Move cursor up one line.
    pub fn move_cursor_up(&mut self) {
        if let Some(prev) = self.cursor_line.checked_sub(1) {
            self.cursor_line = prev;
            self.cursor_col = clamp_col(self.line_text(prev), self.cursor_col);
        }
    }

    /// Move cursor down one line.
    pub fn move_cursor_down(&mut self) {
        let next = self.cursor_line.saturating_add(1);
        if next < self.lines.len() {
            self.cursor_line = next;
            self.cursor_col = clamp_col(self.line_text(next), self.cursor_col);
        }
    }

    /// Move cursor left one character.
    pub fn move_cursor_left(&mut self) {
        if self.cursor_col > 0 {
            // Move to previous character boundary.
            let line = self.line_text(self.cursor_line);
            let clamped = clamp_col(line, self.cursor_col);
            self.cursor_col = line
                .get(..clamped)
                .and_then(|head| head.char_indices().next_back())
                .map_or(0, |(i, _)| i);
        } else if let Some(prev) = self.cursor_line.checked_sub(1) {
            self.cursor_line = prev;
            self.cursor_col = self.line_text(prev).len();
        }
    }

    /// Move cursor right one character.
    pub fn move_cursor_right(&mut self) {
        let line = self.line_text(self.cursor_line);
        let line_len = line.len();
        if self.cursor_col < line_len {
            let clamped = clamp_col(line, self.cursor_col);
            self.cursor_col = line
                .get(clamped..)
                .and_then(|tail| tail.char_indices().nth(1))
                .map_or(line_len, |(i, _)| clamped.saturating_add(i));
        } else {
            let next = self.cursor_line.saturating_add(1);
            if next < self.lines.len() {
                self.cursor_line = next;
                self.cursor_col = 0;
            }
        }
    }

    /// Move cursor to the beginning of the current line.
    pub fn move_cursor_home(&mut self) {
        self.cursor_col = 0;
    }

    /// Move cursor to the end of the current line.
    pub fn move_cursor_end(&mut self) {
        self.cursor_col = self.line_text(self.cursor_line).len();
    }

    /// Move cursor to a specific line (0-based), clamping to valid range.
    pub fn go_to_line(&mut self, line: usize) {
        self.cursor_line = line.min(self.lines.len().saturating_sub(1));
        self.cursor_col = self
            .lines
            .get(self.cursor_line)
            .map_or(0, |line| clamp_col(line, self.cursor_col));
    }

    /// Delete the currently selected text (if any).
    pub fn delete_selection(&mut self) -> Option<String> {
        let anchor = self.selection_anchor?;
        let (start, end) = if anchor < (self.cursor_line, self.cursor_col) {
            (anchor, (self.cursor_line, self.cursor_col))
        } else {
            ((self.cursor_line, self.cursor_col), anchor)
        };

        // The deleted text goes on the undo stack, so it is read from the
        // buffer as it stands — and only in the branch that actually deletes
        // it, or an undo would restore text that was never removed.
        let mut deleted = String::new();
        if start.0 == end.0 {
            // Selection within a single line.
            if let Some(text) = self.lines.get_mut(start.0) {
                let s = clamp_col(text, start.1);
                let e = clamp_col(text, end.1).max(s);
                deleted = text.get(s..e).unwrap_or("").to_string();
                text.replace_range(s..e, "");
            }
        } else if end.0 < self.lines.len() {
            deleted = self.text_between(start, end);
            // Multi-line selection: keep the tail of the last line, drop the
            // lines in between, and graft the tail onto the head of the first.
            let remaining = self
                .lines
                .get(end.0)
                .map(|text| {
                    let end_col = clamp_col(text, end.1);
                    text.get(end_col..).unwrap_or("").to_string()
                })
                .unwrap_or_default();
            let start_col = clamp_col(self.line_text(start.0), start.1);

            let after_start = start.0.saturating_add(1);
            let remove_count = end.0.saturating_sub(start.0);
            for _ in 0..remove_count {
                if after_start < self.lines.len() {
                    self.lines.remove(after_start);
                }
            }
            if let Some(text) = self.lines.get_mut(start.0) {
                text.truncate(start_col);
                text.push_str(&remaining);
            }
        }

        self.cursor_line = start.0;
        self.cursor_col = self
            .lines
            .get(start.0)
            .map_or(0, |line| clamp_col(line, start.1));
        self.selection_anchor = None;

        if !deleted.is_empty() {
            self.push_undo(EditAction::Delete {
                line: start.0,
                col: start.1,
                text: deleted.clone(),
            });
        }

        Some(deleted)
    }

    /// The text between two `(line, col)` positions, `start` first.
    ///
    /// Positions that name lines the document no longer has contribute
    /// nothing, and a column inside a character is moved to the boundary
    /// before it by `clamp_col` — so a selection recorded before an edit
    /// yields a shorter string rather than a panic.
    fn text_between(&self, start: (usize, usize), end: (usize, usize)) -> String {
        if start.0 == end.0 {
            let line = self.line_text(start.0);
            let s = clamp_col(line, start.1);
            let e = clamp_col(line, end.1).max(s);
            return line.get(s..e).unwrap_or("").to_string();
        }

        let mut result = String::new();
        if start.0 < self.lines.len() {
            let first = self.line_text(start.0);
            result.push_str(first.get(clamp_col(first, start.1)..).unwrap_or(""));
        }
        for i in start.0.saturating_add(1)..end.0 {
            result.push('\n');
            result.push_str(self.line_text(i));
        }
        if end.0 < self.lines.len() {
            result.push('\n');
            let last = self.line_text(end.0);
            result.push_str(last.get(..clamp_col(last, end.1)).unwrap_or(""));
        }
        result
    }

    /// Get the selected text, if any.
    pub fn selected_text(&self) -> Option<String> {
        let anchor = self.selection_anchor?;
        let (start, end) = if anchor < (self.cursor_line, self.cursor_col) {
            (anchor, (self.cursor_line, self.cursor_col))
        } else {
            ((self.cursor_line, self.cursor_col), anchor)
        };

        let result = self.text_between(start, end);
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Ensure the cursor is visible by adjusting the scroll offset.
    pub fn ensure_cursor_visible(&mut self, visible_lines: usize) {
        if self.cursor_line < self.scroll_line {
            self.scroll_line = self.cursor_line;
        } else if self.cursor_line >= self.scroll_line + visible_lines {
            self.scroll_line = self.cursor_line - visible_lines + 1;
        }
    }
}

// ============================================================================
// Markdown parser — AST types
// ============================================================================

/// A parsed markdown block element.
#[derive(Clone, Debug, PartialEq)]
pub enum MdBlock {
    /// A heading (level 1-6) with inline content.
    Heading {
        /// Heading level (1-6).
        level: u8,
        /// Inline elements within the heading.
        inlines: Vec<MdInline>,
    },
    /// A paragraph of inline content.
    Paragraph {
        /// Inline elements within the paragraph.
        inlines: Vec<MdInline>,
    },
    /// A fenced code block.
    CodeBlock {
        /// Language identifier (may be empty).
        language: String,
        /// The raw code content.
        code: String,
    },
    /// A blockquote containing nested blocks.
    BlockQuote {
        /// The blocks inside the blockquote.
        children: Vec<MdBlock>,
    },
    /// An unordered list.
    UnorderedList {
        /// List items, each containing inline elements.
        items: Vec<ListItem>,
    },
    /// An ordered list.
    OrderedList {
        /// Starting number.
        start: usize,
        /// List items, each containing inline elements.
        items: Vec<ListItem>,
    },
    /// A horizontal rule.
    HorizontalRule,
    /// A table with headers and rows.
    Table {
        /// Column alignment specifications.
        alignments: Vec<TableAlign>,
        /// Header row cells.
        headers: Vec<Vec<MdInline>>,
        /// Data rows.
        rows: Vec<Vec<Vec<MdInline>>>,
    },
}

/// Alignment for a table column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableAlign {
    /// Left-aligned (default).
    Left,
    /// Center-aligned.
    Center,
    /// Right-aligned.
    Right,
}

/// A list item, which may be a task item.
#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    /// Inline content of the list item.
    pub inlines: Vec<MdInline>,
    /// Whether this is a task list item and its checked state.
    pub task: Option<bool>,
}

/// A parsed markdown inline element.
#[derive(Clone, Debug, PartialEq)]
pub enum MdInline {
    /// Plain text.
    Text(String),
    /// Bold text.
    Bold(Vec<MdInline>),
    /// Italic text.
    Italic(Vec<MdInline>),
    /// Strikethrough text.
    Strikethrough(Vec<MdInline>),
    /// Inline code.
    InlineCode(String),
    /// A hyperlink.
    Link {
        /// Link display text.
        text: Vec<MdInline>,
        /// URL target.
        url: String,
    },
    /// An image reference.
    Image {
        /// Alt text.
        alt: String,
        /// Image URL.
        url: String,
    },
    /// A line break.
    LineBreak,
}

// ============================================================================
// Markdown parser — implementation
// ============================================================================

/// Parse a markdown document into a list of block elements.
pub fn parse_markdown(input: &str) -> Vec<MdBlock> {
    let lines: Vec<&str> = input.lines().collect();
    let mut blocks = Vec::new();
    let mut idx = 0;

    // Every loop below advances `idx` through `lines`, and each used to state
    // its bound twice — once in `idx < lines.len()` and again in `lines[idx]`.
    // Reading through `get` states it once, so a future edit cannot move one
    // and leave the other behind.
    while let Some(&line) = lines.get(idx) {
        // Blank line — skip.
        if line.trim().is_empty() {
            idx += 1;
            continue;
        }

        // Horizontal rule: ---, ***, ___ (3+ chars, optional spaces).
        if is_horizontal_rule(line) {
            blocks.push(MdBlock::HorizontalRule);
            idx += 1;
            continue;
        }

        // Heading: # through ######.
        if let Some(heading) = parse_heading(line) {
            blocks.push(heading);
            idx += 1;
            continue;
        }

        // Fenced code block: ``` or ~~~.
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            let fence_char = if line.trim_start().starts_with("```") {
                '`'
            } else {
                '~'
            };
            let language = line
                .trim_start()
                .trim_start_matches(fence_char)
                .trim()
                .to_string();
            let mut code_lines = Vec::new();
            let closing_fence: String = core::iter::repeat_n(fence_char, 3).collect();
            idx += 1;
            while let Some(&cl) = lines.get(idx) {
                if cl.trim_start().starts_with(&closing_fence)
                    && cl.trim().chars().all(|c| c == fence_char)
                {
                    idx += 1;
                    break;
                }
                code_lines.push(cl);
                idx += 1;
            }
            blocks.push(MdBlock::CodeBlock {
                language,
                code: code_lines.join("\n"),
            });
            continue;
        }

        // Blockquote: > prefix.
        if line.trim_start().starts_with('>') {
            let mut quote_lines = Vec::new();
            while let Some(ql) = lines
                .get(idx)
                .map(|l| l.trim_start())
                .filter(|l| l.starts_with('>'))
            {
                let stripped = ql
                    .strip_prefix("> ")
                    .or_else(|| ql.strip_prefix('>'))
                    .unwrap_or(ql);
                quote_lines.push(stripped);
                idx += 1;
            }
            let inner_text = quote_lines.join("\n");
            let children = parse_markdown(&inner_text);
            blocks.push(MdBlock::BlockQuote { children });
            continue;
        }

        // Table: line contains | and the next line is a separator row.
        if let Some(&separator) = lines.get(idx + 1).filter(|_| line.contains('|'))
            && is_table_separator(separator)
        {
            let headers = parse_table_row(line);
            let alignments = parse_table_alignments(separator);
            idx += 2;
            let mut rows = Vec::new();
            while let Some(&row) = lines
                .get(idx)
                .filter(|l| l.contains('|') && !l.trim().is_empty())
            {
                rows.push(parse_table_row(row));
                idx += 1;
            }
            blocks.push(MdBlock::Table {
                alignments,
                headers,
                rows,
            });
            continue;
        }

        // Unordered list: starts with -, *, +.
        if is_unordered_list_start(line) {
            let mut items = Vec::new();
            while let Some(&item) = lines.get(idx).filter(|l| is_unordered_list_start(l)) {
                items.push(parse_list_item(item, false));
                idx += 1;
            }
            blocks.push(MdBlock::UnorderedList { items });
            continue;
        }

        // Ordered list: starts with number followed by . or ).
        if is_ordered_list_start(line) {
            let start_num = parse_ordered_list_number(line).unwrap_or(1);
            let mut items = Vec::new();
            while let Some(&item) = lines.get(idx).filter(|l| is_ordered_list_start(l)) {
                items.push(parse_list_item(item, true));
                idx += 1;
            }
            blocks.push(MdBlock::OrderedList {
                start: start_num,
                items,
            });
            continue;
        }

        // Paragraph: everything else until a blank line or block-level element.
        let mut para_lines = Vec::new();
        while let Some(&para) = lines
            .get(idx)
            .filter(|l| !l.trim().is_empty() && !is_block_start(l))
        {
            para_lines.push(para);
            idx += 1;
        }
        if !para_lines.is_empty() {
            let text = para_lines.join(" ");
            let inlines = parse_inlines(&text);
            blocks.push(MdBlock::Paragraph { inlines });
        }
    }

    blocks
}

/// Check if a line is a horizontal rule (---, ***, ___ with optional spaces).
fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let first = trimmed.chars().next().unwrap_or(' ');
    if first != '-' && first != '*' && first != '_' {
        return false;
    }
    trimmed.chars().all(|c| c == first || c == ' ')
        && trimmed.chars().filter(|c| *c == first).count() >= 3
}

/// Parse a heading line (# through ######).
fn parse_heading(line: &str) -> Option<MdBlock> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    let content = rest.trim();
    // Remove optional trailing #s.
    let content = content.trim_end_matches('#').trim_end();
    let inlines = parse_inlines(content);
    Some(MdBlock::Heading {
        level: level as u8,
        inlines,
    })
}

/// Check if a line looks like the start of a block element.
fn is_block_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('#')
        || trimmed.starts_with("```")
        || trimmed.starts_with("~~~")
        || trimmed.starts_with('>')
        || is_horizontal_rule(line)
        || is_unordered_list_start(line)
        || is_ordered_list_start(line)
}

/// Check if a line starts an unordered list item.
fn is_unordered_list_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    (trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || trimmed.starts_with("- [")
        || trimmed.starts_with("* [")
        || trimmed.starts_with("+ ["))
        && trimmed.len() > 2
}

/// Check if a line starts an ordered list item.
fn is_ordered_list_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    // Must start with a digit.
    let first = chars.next();
    if !first.is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    // Skip remaining digits.
    let mut found_separator = false;
    for c in chars {
        if c.is_ascii_digit() {
            continue;
        }
        if (c == '.' || c == ')') && !found_separator {
            found_separator = true;
            continue;
        }
        if c == ' ' && found_separator {
            return true;
        }
        return false;
    }
    false
}

/// Parse the starting number from an ordered list item.
fn parse_ordered_list_number(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    let num_str: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

/// Parse a list item (unordered or ordered) into a ListItem.
fn parse_list_item(line: &str, ordered: bool) -> ListItem {
    let trimmed = line.trim_start();

    let content = if ordered {
        // Skip the number and separator (e.g., "1. " or "1) ").
        let after_num: String = trimmed.chars().skip_while(|c| c.is_ascii_digit()).collect();
        after_num
            .strip_prefix(". ")
            .or_else(|| after_num.strip_prefix(") "))
            .map_or_else(|| after_num.clone(), str::to_string)
    } else {
        // Skip the bullet character and space (e.g., "- ").
        if trimmed.len() > 2 {
            trimmed[2..].to_string()
        } else {
            String::new()
        }
    };

    // Check for task list syntax: [x] or [ ].
    let (task, final_content) = if let Some(rest) = content
        .strip_prefix("[x] ")
        .or_else(|| content.strip_prefix("[X] "))
    {
        (Some(true), rest.to_string())
    } else if let Some(rest) = content.strip_prefix("[ ] ") {
        (Some(false), rest.to_string())
    } else {
        (None, content)
    };

    ListItem {
        inlines: parse_inlines(&final_content),
        task,
    }
}

/// Check if a line is a table separator row (e.g., |---|:---:|---:|).
fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return false;
    }
    // Remove leading/trailing pipes and split.
    let stripped = trimmed.trim_start_matches('|').trim_end_matches('|');
    let cells: Vec<&str> = stripped.split('|').collect();
    cells.iter().all(|cell| {
        let c = cell.trim();
        if c.is_empty() {
            return true;
        }
        let c = c.trim_start_matches(':').trim_end_matches(':');
        !c.is_empty() && c.chars().all(|ch| ch == '-')
    })
}

/// Parse table column alignments from the separator row.
fn parse_table_alignments(line: &str) -> Vec<TableAlign> {
    let trimmed = line.trim().trim_start_matches('|').trim_end_matches('|');
    trimmed
        .split('|')
        .map(|cell| {
            let c = cell.trim();
            let left = c.starts_with(':');
            let right = c.ends_with(':');
            match (left, right) {
                (true, true) => TableAlign::Center,
                (false, true) => TableAlign::Right,
                _ => TableAlign::Left,
            }
        })
        .collect()
}

/// Parse a table row into cells of inline elements.
fn parse_table_row(line: &str) -> Vec<Vec<MdInline>> {
    let trimmed = line.trim().trim_start_matches('|').trim_end_matches('|');
    trimmed
        .split('|')
        .map(|cell| parse_inlines(cell.trim()))
        .collect()
}

/// The characters of `chars[start..end]`, or `""` if that is not a range.
///
/// Every span in `parse_inlines` runs from a marker the scanner found to a
/// marker one of the `find_*` helpers found, so in a correct scanner the range
/// is always valid — which is exactly why an incorrect one used to be a panic
/// rather than a wrong rendering. An empty string is what an unmatched marker
/// should produce.
fn span(chars: &[char], start: usize, end: usize) -> String {
    chars.get(start..end).unwrap_or(&[]).iter().collect()
}

/// Emit the plain text accumulated so far, if any, and clear it.
///
/// A construct that begins here ends the run of plain text before it, whether
/// or not the construct turns out to be closed — which is why this is called
/// before the closing marker is looked for, not after.
fn flush_text(text: &mut String, out: &mut Vec<MdInline>) {
    if !text.is_empty() {
        out.push(MdInline::Text(std::mem::take(text)));
    }
}

/// Parse inline markdown elements from a text string.
pub fn parse_inlines(input: &str) -> Vec<MdInline> {
    let chars: Vec<char> = input.chars().collect();
    let mut result = Vec::new();
    let mut pos = 0;
    let mut current_text = String::new();

    // Each construct reads the character at `pos` and usually the one after
    // it, and `pos + 1` is past the end on the last character of the input —
    // the ordinary case, not an exceptional one. Reading both through `get`
    // states the bound once, in the loop header, instead of once per branch.
    while let Some(&ch) = chars.get(pos) {
        let next = chars.get(pos.saturating_add(1)).copied();

        // Strikethrough: ~~text~~
        if ch == '~' && next == Some('~') {
            flush_text(&mut current_text, &mut result);
            let start = pos.saturating_add(2);
            if let Some(end) = find_closing_marker(&chars, start, &['~', '~']) {
                result.push(MdInline::Strikethrough(parse_inlines(&span(
                    &chars, start, end,
                ))));
                pos = end.saturating_add(2);
                continue;
            }
        }

        // Bold: **text** or __text__
        if (ch == '*' || ch == '_') && next == Some(ch) {
            flush_text(&mut current_text, &mut result);
            let start = pos.saturating_add(2);
            if let Some(end) = find_closing_marker(&chars, start, &[ch, ch]) {
                result.push(MdInline::Bold(parse_inlines(&span(&chars, start, end))));
                pos = end.saturating_add(2);
                continue;
            }
        }

        // Italic: *text* or _text_
        if (ch == '*' || ch == '_') && next.is_some_and(|n| n != ch) {
            flush_text(&mut current_text, &mut result);
            let start = pos.saturating_add(1);
            if let Some(end) = find_single_closing(&chars, start, ch) {
                result.push(MdInline::Italic(parse_inlines(&span(&chars, start, end))));
                pos = end.saturating_add(1);
                continue;
            }
        }

        // Inline code: `code`
        if ch == '`' {
            flush_text(&mut current_text, &mut result);
            let start = pos.saturating_add(1);
            if let Some(end) = find_single_closing(&chars, start, '`') {
                result.push(MdInline::InlineCode(span(&chars, start, end)));
                pos = end.saturating_add(1);
                continue;
            }
        }

        // Image: ![alt](url)
        if ch == '!' && next == Some('[') {
            flush_text(&mut current_text, &mut result);
            let alt_start = pos.saturating_add(2);
            if let Some(alt_end) = find_single_closing(&chars, alt_start, ']')
                && chars.get(alt_end.saturating_add(1)) == Some(&'(')
            {
                let url_start = alt_end.saturating_add(2);
                if let Some(url_end) = find_single_closing(&chars, url_start, ')') {
                    result.push(MdInline::Image {
                        alt: span(&chars, alt_start, alt_end),
                        url: span(&chars, url_start, url_end),
                    });
                    pos = url_end.saturating_add(1);
                    continue;
                }
            }
        }

        // Link: [text](url)
        if ch == '[' {
            flush_text(&mut current_text, &mut result);
            let text_start = pos.saturating_add(1);
            if let Some(text_end) = find_single_closing(&chars, text_start, ']')
                && chars.get(text_end.saturating_add(1)) == Some(&'(')
            {
                let url_start = text_end.saturating_add(2);
                if let Some(url_end) = find_single_closing(&chars, url_start, ')') {
                    result.push(MdInline::Link {
                        text: parse_inlines(&span(&chars, text_start, text_end)),
                        url: span(&chars, url_start, url_end),
                    });
                    pos = url_end.saturating_add(1);
                    continue;
                }
            }
        }

        // Line break: two or more spaces at the very end of the text.
        if ch == ' ' && next == Some(' ') {
            let space_end = chars
                .iter()
                .skip(pos)
                .position(|&c| c != ' ')
                .map_or(chars.len(), |off| pos.saturating_add(off));
            if space_end == chars.len() {
                flush_text(&mut current_text, &mut result);
                result.push(MdInline::LineBreak);
                pos = space_end;
                continue;
            }
        }

        current_text.push(ch);
        pos = pos.saturating_add(1);
    }

    flush_text(&mut current_text, &mut result);
    result
}

/// Find the position of a two-character closing marker (like ** or ~~).
fn find_closing_marker(chars: &[char], start: usize, marker: &[char; 2]) -> Option<usize> {
    let [first, second] = *marker;
    chars
        .get(start..)?
        .windows(2)
        .position(|w| matches!(w, [a, b] if *a == first && *b == second))
        .map(|p| start.saturating_add(p))
}

/// Find the position of a single-character closing marker.
fn find_single_closing(chars: &[char], start: usize, marker: char) -> Option<usize> {
    chars
        .get(start..)?
        .iter()
        .position(|&c| c == marker)
        .map(|p| start.saturating_add(p))
}

// ============================================================================
// Table of contents extraction
// ============================================================================

/// A heading entry for the table of contents.
#[derive(Clone, Debug)]
pub struct TocEntry {
    /// Heading level (1-6).
    pub level: u8,
    /// Plain text content of the heading (no formatting).
    pub text: String,
    /// Line number in the source document (0-based).
    pub line: usize,
}

/// Extract table of contents entries from the raw markdown source.
pub fn extract_toc(source: &str) -> Vec<TocEntry> {
    let mut entries = Vec::new();
    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) {
            let rest = &trimmed[hashes..];
            if rest.is_empty() || rest.starts_with(' ') {
                let text = rest.trim().trim_end_matches('#').trim().to_string();
                entries.push(TocEntry {
                    level: hashes as u8,
                    text,
                    line: line_idx,
                });
            }
        }
    }
    entries
}

// ============================================================================
// Find and replace
// ============================================================================

/// State for the find and replace panel.
pub struct FindReplaceState {
    /// The search query string.
    pub query: String,
    /// The replacement string.
    pub replacement: String,
    /// Whether the find panel is visible.
    pub visible: bool,
    /// Whether to match case.
    pub case_sensitive: bool,
    /// Current match positions: (line, start_col, end_col).
    pub matches: Vec<(usize, usize, usize)>,
    /// Index of the currently highlighted match.
    pub current_match: usize,
}

impl Default for FindReplaceState {
    fn default() -> Self {
        Self::new()
    }
}

impl FindReplaceState {
    /// Create a new find/replace state.
    pub fn new() -> Self {
        Self {
            query: String::new(),
            replacement: String::new(),
            visible: false,
            case_sensitive: false,
            matches: Vec::new(),
            current_match: 0,
        }
    }

    /// Find all occurrences of the query in the given document lines.
    ///
    /// The ranges recorded are byte ranges *in the lines themselves*, so they
    /// can be handed straight to `replace_range` or to the highlighter. This
    /// used to search a `to_lowercase()` copy of each line and use the offsets
    /// it found on the real line, which is only sound when lowercasing
    /// preserves length — it does not. See `textfind`'s documentation.
    pub fn find_all(&mut self, lines: &[String]) {
        self.matches.clear();
        self.current_match = 0;
        let case = Case::sensitive(self.case_sensitive);
        for (line_idx, line) in lines.iter().enumerate() {
            self.matches.extend(
                textfind::matches(line, &self.query, case)
                    .map(|(start, end)| (line_idx, start, end)),
            );
        }
    }

    /// Move to the next match.
    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = (self.current_match + 1) % self.matches.len();
        }
    }

    /// Move to the previous match.
    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_match = match self.current_match.checked_sub(1) {
                Some(prev) => prev,
                None => self.matches.len().saturating_sub(1),
            };
        }
    }

    /// Replace the current match. Returns whether anything was replaced.
    pub fn replace_current(&mut self, lines: &mut [String]) -> bool {
        let Some(&(line_idx, start, end)) = self.current_match_info_ref() else {
            return false;
        };
        if !replace_in_line(lines, line_idx, start, end, &self.replacement) {
            return false;
        }
        // Refresh matches after replacement.
        self.find_all(lines);
        true
    }

    /// Replace all matches. Returns how many were replaced.
    pub fn replace_all(&mut self, lines: &mut [String]) -> usize {
        if self.query.is_empty() {
            return 0;
        }
        let mut count = 0_usize;
        // Replace from end to start, so that a replacement of a different
        // length than the text it replaces does not move the matches that have
        // not been applied yet. This is only sound because `find_all` reports
        // non-overlapping matches — with overlapping ones, a later replacement
        // would rewrite text an earlier one had already changed.
        for &(line_idx, start, end) in self.matches.iter().rev() {
            if replace_in_line(lines, line_idx, start, end, &self.replacement) {
                count = count.saturating_add(1);
            }
        }
        self.find_all(lines);
        count
    }

    /// Get the current match info, if any.
    pub fn current_match_info(&self) -> Option<(usize, usize, usize)> {
        self.current_match_info_ref().copied()
    }

    /// The current match, clamped into range.
    ///
    /// `current_match` is an index into `matches`, and `matches` is rebuilt
    /// whenever the query or the document changes — so an index held across
    /// either is stale. Clamping keeps a stale index pointing at *a* match
    /// rather than off the end.
    fn current_match_info_ref(&self) -> Option<&(usize, usize, usize)> {
        self.matches
            .get(self.current_match)
            .or_else(|| self.matches.last())
    }

    /// Return the total number of matches.
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }
}

/// Replace bytes `start..end` of `lines[n]` with `text`. Returns whether
/// anything was replaced.
///
/// Every argument here can be stale: the match list is recorded against
/// whatever the document held when the search ran, and the user can edit it,
/// or switch documents, before pressing Replace. `String::replace_range` panics
/// on an out-of-range span *and* on one that splits a character, so all four
/// conditions are checked rather than the two the previous version checked.
fn replace_in_line(lines: &mut [String], n: usize, start: usize, end: usize, text: &str) -> bool {
    let Some(line) = lines.get_mut(n) else {
        return false;
    };
    if start > end || end > line.len() || !line.is_char_boundary(start) || !line.is_char_boundary(end)
    {
        return false;
    }
    line.replace_range(start..end, text);
    true
}

// ============================================================================
// HTML export
// ============================================================================

/// Export a parsed markdown document to an HTML string.
pub fn export_html(blocks: &[MdBlock]) -> String {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html>\n<head>\n");
    html.push_str("<meta charset=\"utf-8\">\n");
    html.push_str("<title>Markdown Export</title>\n");
    html.push_str("<style>\n");
    html.push_str(
        "body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; ",
    );
    html.push_str("max-width: 800px; margin: 0 auto; padding: 20px; ");
    html.push_str("background: #1e1e2e; color: #cdd6f4; }\n");
    html.push_str("h1, h2, h3, h4, h5, h6 { color: #89b4fa; }\n");
    html.push_str("a { color: #89b4fa; }\n");
    html.push_str("code { background: #313244; padding: 2px 6px; border-radius: 4px; }\n");
    html.push_str(
        "pre { background: #313244; padding: 16px; border-radius: 8px; overflow-x: auto; }\n",
    );
    html.push_str("pre code { padding: 0; }\n");
    html.push_str("blockquote { border-left: 4px solid #89b4fa; margin-left: 0; padding-left: 16px; color: #a6adc8; }\n");
    html.push_str("table { border-collapse: collapse; width: 100%; }\n");
    html.push_str("th, td { border: 1px solid #45475a; padding: 8px; text-align: left; }\n");
    html.push_str("th { background: #313244; }\n");
    html.push_str("hr { border: none; border-top: 2px solid #45475a; margin: 24px 0; }\n");
    html.push_str("img { max-width: 100%; }\n");
    html.push_str("</style>\n");
    html.push_str("</head>\n<body>\n");

    for block in blocks {
        render_block_html(block, &mut html);
    }

    html.push_str("</body>\n</html>\n");
    html
}

/// Render a single block element to HTML.
fn render_block_html(block: &MdBlock, html: &mut String) {
    match block {
        MdBlock::Heading { level, inlines } => {
            html.push_str(&format!("<h{}>", level));
            render_inlines_html(inlines, html);
            html.push_str(&format!("</h{}>\n", level));
        }
        MdBlock::Paragraph { inlines } => {
            html.push_str("<p>");
            render_inlines_html(inlines, html);
            html.push_str("</p>\n");
        }
        MdBlock::CodeBlock { language, code } => {
            if language.is_empty() {
                html.push_str("<pre><code>");
            } else {
                html.push_str(&format!(
                    "<pre><code class=\"language-{}\">",
                    escape_html(language)
                ));
            }
            html.push_str(&escape_html(code));
            html.push_str("</code></pre>\n");
        }
        MdBlock::BlockQuote { children } => {
            html.push_str("<blockquote>\n");
            for child in children {
                render_block_html(child, html);
            }
            html.push_str("</blockquote>\n");
        }
        MdBlock::UnorderedList { items } => {
            html.push_str("<ul>\n");
            for item in items {
                html.push_str("<li>");
                if let Some(checked) = item.task {
                    if checked {
                        html.push_str("<input type=\"checkbox\" checked disabled> ");
                    } else {
                        html.push_str("<input type=\"checkbox\" disabled> ");
                    }
                }
                render_inlines_html(&item.inlines, html);
                html.push_str("</li>\n");
            }
            html.push_str("</ul>\n");
        }
        MdBlock::OrderedList { start, items } => {
            if *start != 1 {
                html.push_str(&format!("<ol start=\"{}\">\n", start));
            } else {
                html.push_str("<ol>\n");
            }
            for item in items {
                html.push_str("<li>");
                if let Some(checked) = item.task {
                    if checked {
                        html.push_str("<input type=\"checkbox\" checked disabled> ");
                    } else {
                        html.push_str("<input type=\"checkbox\" disabled> ");
                    }
                }
                render_inlines_html(&item.inlines, html);
                html.push_str("</li>\n");
            }
            html.push_str("</ol>\n");
        }
        MdBlock::HorizontalRule => {
            html.push_str("<hr>\n");
        }
        MdBlock::Table {
            alignments,
            headers,
            rows,
        } => {
            html.push_str("<table>\n<thead>\n<tr>\n");
            for (i, header) in headers.iter().enumerate() {
                let align = alignments.get(i).copied().unwrap_or(TableAlign::Left);
                let align_attr = match align {
                    TableAlign::Left => "",
                    TableAlign::Center => " style=\"text-align:center\"",
                    TableAlign::Right => " style=\"text-align:right\"",
                };
                html.push_str(&format!("<th{}>", align_attr));
                render_inlines_html(header, html);
                html.push_str("</th>\n");
            }
            html.push_str("</tr>\n</thead>\n<tbody>\n");
            for row in rows {
                html.push_str("<tr>\n");
                for (i, cell) in row.iter().enumerate() {
                    let align = alignments.get(i).copied().unwrap_or(TableAlign::Left);
                    let align_attr = match align {
                        TableAlign::Left => "",
                        TableAlign::Center => " style=\"text-align:center\"",
                        TableAlign::Right => " style=\"text-align:right\"",
                    };
                    html.push_str(&format!("<td{}>", align_attr));
                    render_inlines_html(cell, html);
                    html.push_str("</td>\n");
                }
                html.push_str("</tr>\n");
            }
            html.push_str("</tbody>\n</table>\n");
        }
    }
}

/// Render inline elements to HTML.
fn render_inlines_html(inlines: &[MdInline], html: &mut String) {
    for inline in inlines {
        match inline {
            MdInline::Text(t) => html.push_str(&escape_html(t)),
            MdInline::Bold(inner) => {
                html.push_str("<strong>");
                render_inlines_html(inner, html);
                html.push_str("</strong>");
            }
            MdInline::Italic(inner) => {
                html.push_str("<em>");
                render_inlines_html(inner, html);
                html.push_str("</em>");
            }
            MdInline::Strikethrough(inner) => {
                html.push_str("<del>");
                render_inlines_html(inner, html);
                html.push_str("</del>");
            }
            MdInline::InlineCode(code) => {
                html.push_str("<code>");
                html.push_str(&escape_html(code));
                html.push_str("</code>");
            }
            MdInline::Link { text, url } => {
                html.push_str(&format!("<a href=\"{}\">", escape_html(url)));
                render_inlines_html(text, html);
                html.push_str("</a>");
            }
            MdInline::Image { alt, url } => {
                html.push_str(&format!(
                    "<img src=\"{}\" alt=\"{}\">",
                    escape_html(url),
                    escape_html(alt)
                ));
            }
            MdInline::LineBreak => {
                html.push_str("<br>\n");
            }
        }
    }
}

/// Escape HTML special characters.
fn escape_html(input: &str) -> String {
    guitk::escape::xml(input)
}

// ============================================================================
// Syntax highlighting for the editor source view
// ============================================================================

/// A span of text with a specific color for syntax highlighting.
#[derive(Clone, Debug)]
pub struct HighlightSpan {
    /// Start byte offset within the line.
    pub start: usize,
    /// End byte offset (exclusive) within the line.
    pub end: usize,
    /// Color to render this span.
    pub color: Color,
    /// Font weight for this span.
    pub weight: FontWeightHint,
}

/// Produce syntax highlighting spans for a single line of markdown source.
pub fn highlight_line(line: &str) -> Vec<HighlightSpan> {
    let mut spans = Vec::new();
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();

    // Heading lines: color the whole line.
    if trimmed.starts_with('#') {
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if hashes <= 6 && (trimmed.len() == hashes || trimmed.as_bytes().get(hashes) == Some(&b' '))
        {
            let heading_color = match hashes {
                1 => BLUE,
                2 => LAVENDER,
                3 => GREEN,
                4 => YELLOW,
                5 => PEACH,
                _ => RED,
            };
            // Hash marks get dimmed.
            spans.push(HighlightSpan {
                start: indent_len,
                end: indent_len + hashes,
                color: OVERLAY0,
                weight: FontWeightHint::Bold,
            });
            // The heading text.
            if line.len() > indent_len + hashes {
                spans.push(HighlightSpan {
                    start: indent_len + hashes,
                    end: line.len(),
                    color: heading_color,
                    weight: FontWeightHint::Bold,
                });
            }
            return spans;
        }
    }

    // Fenced code block markers.
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        spans.push(HighlightSpan {
            start: 0,
            end: line.len(),
            color: GREEN,
            weight: FontWeightHint::Regular,
        });
        return spans;
    }

    // Blockquote prefix.
    if trimmed.starts_with('>') {
        spans.push(HighlightSpan {
            start: indent_len,
            end: indent_len + 1,
            color: BLUE,
            weight: FontWeightHint::Bold,
        });
        if line.len() > indent_len + 1 {
            spans.push(HighlightSpan {
                start: indent_len + 1,
                end: line.len(),
                color: SUBTEXT0,
                weight: FontWeightHint::Regular,
            });
        }
        return spans;
    }

    // Horizontal rule.
    if is_horizontal_rule(line) {
        spans.push(HighlightSpan {
            start: 0,
            end: line.len(),
            color: SURFACE2,
            weight: FontWeightHint::Regular,
        });
        return spans;
    }

    // List items: color the bullet/number.
    if is_unordered_list_start(line) {
        let bullet_end = indent_len + 2;
        spans.push(HighlightSpan {
            start: indent_len,
            end: bullet_end.min(line.len()),
            color: BLUE,
            weight: FontWeightHint::Bold,
        });
        // Check for task list checkbox.
        let after_bullet = &line[bullet_end.min(line.len())..];
        if after_bullet.starts_with("[x] ") || after_bullet.starts_with("[X] ") {
            spans.push(HighlightSpan {
                start: bullet_end,
                end: bullet_end + 4,
                color: GREEN,
                weight: FontWeightHint::Regular,
            });
            highlight_inline_spans(line, bullet_end + 4, &mut spans);
        } else if after_bullet.starts_with("[ ] ") {
            spans.push(HighlightSpan {
                start: bullet_end,
                end: bullet_end + 4,
                color: OVERLAY0,
                weight: FontWeightHint::Regular,
            });
            highlight_inline_spans(line, bullet_end + 4, &mut spans);
        } else {
            highlight_inline_spans(line, bullet_end, &mut spans);
        }
        return spans;
    }

    if is_ordered_list_start(line) {
        let num_end = trimmed
            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != ')')
            .unwrap_or(trimmed.len());
        let abs_end = indent_len + num_end;
        spans.push(HighlightSpan {
            start: indent_len,
            end: abs_end.min(line.len()),
            color: BLUE,
            weight: FontWeightHint::Bold,
        });
        highlight_inline_spans(line, abs_end, &mut spans);
        return spans;
    }

    // Table rows.
    if line.contains('|') && is_table_separator(line) {
        spans.push(HighlightSpan {
            start: 0,
            end: line.len(),
            color: SURFACE2,
            weight: FontWeightHint::Regular,
        });
        return spans;
    }

    // Default: apply inline highlighting.
    highlight_inline_spans(line, 0, &mut spans);

    // If no spans were generated, use default text color.
    if spans.is_empty() {
        spans.push(HighlightSpan {
            start: 0,
            end: line.len(),
            color: TEXT,
            weight: FontWeightHint::Regular,
        });
    }

    spans
}

/// Offset of the next `needle` byte at or after `from`, or `None` if there is
/// none.
///
/// Scanning bytes for an ASCII delimiter is sound on UTF-8 text: a continuation
/// byte is never equal to an ASCII one, so a hit is always on a character
/// boundary — which is what the renderer needs, since it slices the line by the
/// offsets these scans produce.
fn scan_to(bytes: &[u8], from: usize, needle: u8) -> Option<usize> {
    bytes
        .get(from..)?
        .iter()
        .position(|&b| b == needle)
        .map(|p| from.saturating_add(p))
}

/// Offset of the next doubled `needle` byte (`**`, `__`, `~~`) at or after
/// `from`, or `None` if there is none.
fn scan_to_pair(bytes: &[u8], from: usize, needle: u8) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(2)
        .position(|w| matches!(w, [a, b] if *a == needle && *b == needle))
        .map(|p| from.saturating_add(p))
}

/// Highlight inline markdown elements within a line starting at a byte offset.
fn highlight_inline_spans(line: &str, start_offset: usize, spans: &mut Vec<HighlightSpan>) {
    // `start_offset` is computed by the caller from a prefix it has already
    // matched, so it is a boundary in practice — but it is a *computed* offset
    // into a line the user is still typing into, and the failure mode of
    // getting that wrong is the whole editor going down rather than one line
    // drawing oddly.
    let segment = line.get(start_offset..).unwrap_or("");
    let bytes = segment.as_bytes();
    let mut pos = 0;
    let mut text_start = 0;

    // The six constructs below share two moves: emit the plain text that ran
    // up to the construct, then emit the construct's own spans. Both are
    // stated in terms of offsets within `segment`, which these close over and
    // shift by `start_offset`.
    let push = |spans: &mut Vec<HighlightSpan>, from: usize, to: usize, color, weight| {
        spans.push(HighlightSpan {
            start: start_offset.saturating_add(from),
            end: start_offset.saturating_add(to),
            color,
            weight,
        });
    };

    while let Some(&b) = bytes.get(pos) {
        let next = bytes.get(pos.saturating_add(1)).copied();

        // Inline code: `code`.
        if b == b'`' {
            if pos > text_start {
                push(spans, text_start, pos, TEXT, FontWeightHint::Regular);
            }
            if let Some(code_end) = scan_to(bytes, pos.saturating_add(1), b'`') {
                push(
                    spans,
                    pos,
                    code_end.saturating_add(1),
                    GREEN,
                    FontWeightHint::Regular,
                );
                pos = code_end.saturating_add(1);
                text_start = pos;
                continue;
            }
        }

        // Bold markers: ** or __.
        if (b == b'*' || b == b'_') && next == Some(b) {
            let inner_start = pos.saturating_add(2);
            if let Some(inner_end) = scan_to_pair(bytes, inner_start, b) {
                if pos > text_start {
                    push(spans, text_start, pos, TEXT, FontWeightHint::Regular);
                }
                // Dim the opening markers, bold the text, dim the closing ones.
                push(spans, pos, inner_start, OVERLAY0, FontWeightHint::Regular);
                push(spans, inner_start, inner_end, TEXT, FontWeightHint::Bold);
                push(
                    spans,
                    inner_end,
                    inner_end.saturating_add(2),
                    OVERLAY0,
                    FontWeightHint::Regular,
                );
                pos = inner_end.saturating_add(2);
                text_start = pos;
                continue;
            }
        }

        // Strikethrough markers: ~~.
        if b == b'~' && next == Some(b'~') {
            if let Some(inner_end) = scan_to_pair(bytes, pos.saturating_add(2), b'~') {
                if pos > text_start {
                    push(spans, text_start, pos, TEXT, FontWeightHint::Regular);
                }
                push(
                    spans,
                    pos,
                    inner_end.saturating_add(2),
                    OVERLAY0,
                    FontWeightHint::Regular,
                );
                pos = inner_end.saturating_add(2);
                text_start = pos;
                continue;
            }
        }

        // Italic markers: single * or _.
        if (b == b'*' || b == b'_') && next != Some(b) {
            let inner_start = pos.saturating_add(1);
            if let Some(inner_end) = scan_to(bytes, inner_start, b) {
                if pos > text_start {
                    push(spans, text_start, pos, TEXT, FontWeightHint::Regular);
                }
                push(spans, pos, inner_start, OVERLAY0, FontWeightHint::Regular);
                push(
                    spans,
                    inner_start,
                    inner_end,
                    LAVENDER,
                    FontWeightHint::Light,
                );
                push(
                    spans,
                    inner_end,
                    inner_end.saturating_add(1),
                    OVERLAY0,
                    FontWeightHint::Regular,
                );
                pos = inner_end.saturating_add(1);
                text_start = pos;
                continue;
            }
        }

        // Links: [text](url), and images, which are a link with a `!` in front.
        if b == b'[' || (b == b'!' && next == Some(b'[')) {
            let is_image = b == b'!';
            let bracket_start = if is_image {
                pos.saturating_add(1)
            } else {
                pos
            };
            if let Some(bracket_end) = scan_to(bytes, bracket_start.saturating_add(1), b']')
                && bytes.get(bracket_end.saturating_add(1)) == Some(&b'(')
                && let Some(paren_end) = scan_to(bytes, bracket_end.saturating_add(2), b')')
            {
                if pos > text_start {
                    push(spans, text_start, pos, TEXT, FontWeightHint::Regular);
                }
                if is_image {
                    // An image is drawn as one span: its alt text is not the
                    // link text a reader clicks, so colouring it as one would
                    // be a lie about what it is.
                    push(
                        spans,
                        pos,
                        paren_end.saturating_add(1),
                        PEACH,
                        FontWeightHint::Regular,
                    );
                } else {
                    push(
                        spans,
                        bracket_start,
                        bracket_end.saturating_add(1),
                        BLUE,
                        FontWeightHint::Regular,
                    );
                    push(
                        spans,
                        bracket_end.saturating_add(1),
                        paren_end.saturating_add(1),
                        OVERLAY0,
                        FontWeightHint::Regular,
                    );
                }
                pos = paren_end.saturating_add(1);
                text_start = pos;
                continue;
            }
        }

        pos = pos.saturating_add(1);
    }

    // Remaining text.
    if text_start < bytes.len() {
        push(
            spans,
            text_start,
            bytes.len(),
            TEXT,
            FontWeightHint::Regular,
        );
    }
}

// ============================================================================
// Insert helpers
// ============================================================================

/// Insert a bold wrapper around the current selection or at cursor.
pub fn insert_bold(doc: &mut Document) {
    if doc.selection_anchor.is_some() {
        wrap_selection(doc, "**", "**");
    } else {
        insert_snippet(doc, "**bold**");
    }
}

/// Insert an italic wrapper around the current selection or at cursor.
pub fn insert_italic(doc: &mut Document) {
    if doc.selection_anchor.is_some() {
        wrap_selection(doc, "*", "*");
    } else {
        insert_snippet(doc, "*italic*");
    }
}

/// Insert a strikethrough wrapper.
pub fn insert_strikethrough(doc: &mut Document) {
    if doc.selection_anchor.is_some() {
        wrap_selection(doc, "~~", "~~");
    } else {
        insert_snippet(doc, "~~strikethrough~~");
    }
}

/// Insert a heading at the current line.
pub fn insert_heading(doc: &mut Document, level: u8) {
    let prefix = format!("{} ", "#".repeat(level as usize));
    let line = doc.cursor_line;
    let Some(text) = doc.lines.get_mut(line) else {
        return;
    };
    text.insert_str(0, &prefix);
    doc.cursor_col = doc.line_text(line).len();
    // What happened here is an *insertion* of the prefix, so its undo is an
    // erase of the prefix. Recording it as a `Delete` of the old line — which
    // is what this used to do — made undo re-insert the whole line in front of
    // itself, so `# Hello` undid to `Hello# Hello` instead of `Hello`.
    doc.push_undo(EditAction::Insert {
        line,
        col: 0,
        text: prefix,
    });
}

/// Insert a link template.
pub fn insert_link(doc: &mut Document) {
    if doc.selection_anchor.is_some()
        && let Some(selected) = doc.selected_text()
    {
        doc.delete_selection();
        insert_snippet(doc, &format!("[{}](url)", selected));
        return;
    }
    insert_snippet(doc, "[link text](url)");
}

/// Insert an image template.
pub fn insert_image(doc: &mut Document) {
    insert_snippet(doc, "![alt text](image_url)");
}

/// Insert an inline code wrapper.
pub fn insert_inline_code(doc: &mut Document) {
    if doc.selection_anchor.is_some() {
        wrap_selection(doc, "`", "`");
    } else {
        insert_snippet(doc, "`code`");
    }
}

/// Insert a fenced code block.
pub fn insert_code_block(doc: &mut Document) {
    let line = doc.cursor_line;
    let text = "```\n\n```";
    doc.insert_text(text);
    // Position cursor inside the code block.
    doc.cursor_line = line + 1;
    doc.cursor_col = 0;
}

/// Insert an unordered list item.
pub fn insert_unordered_list(doc: &mut Document) {
    insert_snippet(doc, "- ");
}

/// Insert an ordered list item.
pub fn insert_ordered_list(doc: &mut Document) {
    insert_snippet(doc, "1. ");
}

/// Insert a task list item.
pub fn insert_task_list(doc: &mut Document) {
    insert_snippet(doc, "- [ ] ");
}

/// Insert a table template.
pub fn insert_table(doc: &mut Document) {
    let table = "| Column 1 | Column 2 | Column 3 |\n|----------|----------|----------|\n| Cell 1   | Cell 2   | Cell 3   |";
    doc.insert_text(table);
}

/// Insert a horizontal rule.
pub fn insert_horizontal_rule(doc: &mut Document) {
    insert_snippet(doc, "\n---\n");
}

/// Insert a text snippet at the cursor.
fn insert_snippet(doc: &mut Document, text: &str) {
    doc.insert_text(text);
}

/// Wrap the current selection with prefix and suffix strings.
fn wrap_selection(doc: &mut Document, prefix: &str, suffix: &str) {
    if let Some(selected) = doc.selected_text() {
        doc.delete_selection();
        let wrapped = format!("{}{}{}", prefix, selected, suffix);
        doc.insert_text(&wrapped);
    }
}

// ============================================================================
// Rendering — editor source view
// ============================================================================

/// Render the editor source view (line numbers + syntax-highlighted text).
pub fn render_editor(
    doc: &Document,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    find_state: &FindReplaceState,
) -> Vec<RenderCommand> {
    let mut cmds = vec![
        // The source pane is a grid: `col_x` places the caret, the selection
        // band and the find highlights at `chars * char_width()`, which came
        // from the mono face. Everything this function draws has to be drawn
        // in that face, or the marks land beside the characters they mark.
        // The preview pane — rendered separately, as prose — is deliberately
        // outside this scope.
        RenderCommand::PushFont {
            family: FontFamily::Mono,
        },
        // Background.
        RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        },
        // Gutter background.
        RenderCommand::FillRect {
            x,
            y,
            width: GUTTER_WIDTH,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        },
        // Gutter separator line.
        RenderCommand::Line {
            x1: x + GUTTER_WIDTH,
            y1: y,
            x2: x + GUTTER_WIDTH,
            y2: y + height,
            color: SURFACE0,
            width: 1.0,
        },
    ];

    let visible_lines = (height / LINE_HEIGHT) as usize;
    let text_x = x + GUTTER_WIDTH + EDITOR_PADDING;
    let text_width = width - GUTTER_WIDTH - EDITOR_PADDING * 2.0;

    // Determine which lines are inside code blocks (for code block background).
    let code_block_start_lines: Vec<usize> = doc
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim_start();
            t.starts_with("```") || t.starts_with("~~~")
        })
        .map(|(i, _)| i)
        .collect();

    // Track which lines are inside code blocks.
    let mut code_block_ranges: Vec<(usize, usize)> = Vec::new();
    let mut start_idx = None;
    for &line_idx in &code_block_start_lines {
        if start_idx.is_none() {
            start_idx = Some(line_idx);
        } else {
            code_block_ranges.push((start_idx.unwrap_or(0), line_idx));
            start_idx = None;
        }
    }

    let is_in_code_block = |line_num: usize| -> bool {
        code_block_ranges
            .iter()
            .any(|&(start, end)| line_num > start && line_num < end)
    };

    // `skip`/`take` say the same thing the old index-and-break said — draw at
    // most `visible_lines` lines starting at the scroll position — but say it
    // once, so a `scroll_line` past the end of a document that shrank under
    // the viewport draws nothing rather than indexing off the end.
    for (i, (line_num, line)) in doc
        .lines
        .iter()
        .enumerate()
        .skip(doc.scroll_line)
        .take(visible_lines)
        .enumerate()
    {
        let line_y = y + (i as f32) * LINE_HEIGHT;

        // Current line highlight.
        if line_num == doc.cursor_line {
            cmds.push(RenderCommand::FillRect {
                x: x + GUTTER_WIDTH,
                y: line_y,
                width: width - GUTTER_WIDTH,
                height: LINE_HEIGHT,
                color: Color::rgba(SURFACE0.r, SURFACE0.g, SURFACE0.b, 100),
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Code block background.
        if is_in_code_block(line_num) {
            cmds.push(RenderCommand::FillRect {
                x: x + GUTTER_WIDTH,
                y: line_y,
                width: width - GUTTER_WIDTH,
                height: LINE_HEIGHT,
                color: Color::rgba(SURFACE0.r, SURFACE0.g, SURFACE0.b, 60),
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Highlight find matches on this line.
        for (match_line, match_start, match_end) in &find_state.matches {
            if *match_line == line_num {
                let match_x = col_x(line, *match_start, text_x);
                let match_w = col_x(line, *match_end, text_x) - match_x;
                let is_current = find_state
                    .current_match_info()
                    .map(|(l, s, _)| l == line_num && s == *match_start)
                    .unwrap_or(false);
                let highlight_color = if is_current {
                    Color::rgba(YELLOW.r, YELLOW.g, YELLOW.b, 120)
                } else {
                    Color::rgba(YELLOW.r, YELLOW.g, YELLOW.b, 50)
                };
                cmds.push(RenderCommand::FillRect {
                    x: match_x,
                    y: line_y,
                    width: match_w,
                    height: LINE_HEIGHT,
                    color: highlight_color,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }

        // Line number.
        let line_num_text = format!("{}", line_num + 1);
        let num_color = if line_num == doc.cursor_line {
            TEXT
        } else {
            OVERLAY0
        };
        cmds.push(RenderCommand::Text {
            x: text::right_x(
                &line_num_text,
                x + GUTTER_WIDTH - EDITOR_PADDING,
                LINE_NUMBER_FONT_SIZE,
                FontWeightHint::Regular,
            ),
            y: line_y + 3.0,
            text: line_num_text,
            font_size: LINE_NUMBER_FONT_SIZE,
            color: num_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(GUTTER_WIDTH - EDITOR_PADDING),
            overflow: TextOverflow::Ellipsis,
        });

        // Syntax-highlighted line content.
        if !line.is_empty() {
            let spans = if is_in_code_block(line_num) {
                // Inside code block: use code color for everything.
                vec![HighlightSpan {
                    start: 0,
                    end: line.len(),
                    color: GREEN,
                    weight: FontWeightHint::Regular,
                }]
            } else {
                highlight_line(line)
            };

            for span in &spans {
                if span.start >= line.len() {
                    continue;
                }
                let end = span.end.min(line.len());
                let span_text = &line[span.start..end];
                let span_x = col_x(line, span.start, text_x);
                cmds.push(RenderCommand::Text {
                    x: span_x,
                    y: line_y + 3.0,
                    text: span_text.to_string(),
                    font_size: EDITOR_FONT_SIZE,
                    color: span.color,
                    font_weight: span.weight,
                    max_width: Some(text_width),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        // Cursor.
        if line_num == doc.cursor_line {
            let cursor_x = col_x(line, doc.cursor_col, text_x);
            cmds.push(RenderCommand::FillRect {
                x: cursor_x,
                y: line_y,
                width: 2.0,
                height: LINE_HEIGHT,
                color: TEXT,
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Selection highlight.
        if let Some(anchor) = doc.selection_anchor {
            let (sel_start, sel_end) = if anchor < (doc.cursor_line, doc.cursor_col) {
                (anchor, (doc.cursor_line, doc.cursor_col))
            } else {
                ((doc.cursor_line, doc.cursor_col), anchor)
            };

            if line_num >= sel_start.0 && line_num <= sel_end.0 {
                let start_col = if line_num == sel_start.0 {
                    sel_start.1
                } else {
                    0
                };
                let end_col = if line_num == sel_end.0 {
                    sel_end.1
                } else {
                    line.len()
                };
                let sel_x = col_x(line, start_col, text_x);
                let sel_w = col_x(line, end_col, text_x) - sel_x;
                cmds.push(RenderCommand::FillRect {
                    x: sel_x,
                    y: line_y,
                    width: sel_w.max(0.0),
                    height: LINE_HEIGHT,
                    color: Color::rgba(BLUE.r, BLUE.g, BLUE.b, 60),
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
    }

    cmds.push(RenderCommand::PopFont);
    cmds
}

// ============================================================================
// Rendering — preview
// ============================================================================

/// Context for tracking vertical position during preview rendering.
struct PreviewContext {
    /// Current Y position for the next element.
    y: f32,
    /// Left edge X position.
    x: f32,
    /// Available width.
    width: f32,
    /// Collected render commands.
    cmds: Vec<RenderCommand>,
    /// Scroll offset in pixels.
    scroll_offset: f32,
    /// Base Y (top of viewport).
    base_y: f32,
    /// Viewport height.
    viewport_height: f32,
}

impl PreviewContext {
    /// Create a new preview rendering context.
    fn new(x: f32, y: f32, width: f32, height: f32, scroll_offset: f32) -> Self {
        Self {
            y,
            x,
            width,
            cmds: Vec::new(),
            scroll_offset,
            base_y: y,
            viewport_height: height,
        }
    }

    /// Check if the current Y position is within the visible viewport.
    fn is_visible(&self, element_height: f32) -> bool {
        let screen_y = self.y - self.scroll_offset;
        screen_y + element_height >= self.base_y && screen_y < self.base_y + self.viewport_height
    }

    /// Add vertical spacing.
    fn add_spacing(&mut self, pixels: f32) {
        self.y += pixels;
    }

    /// Get the adjusted Y for rendering (accounting for scroll offset).
    fn render_y(&self) -> f32 {
        self.y - self.scroll_offset
    }
}

/// Render the preview panel for a list of parsed markdown blocks.
pub fn render_preview(
    blocks: &[MdBlock],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    scroll_offset: f32,
) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    // Background.
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height,
        color: BASE,
        corner_radii: CornerRadii::ZERO,
    });

    let content_x = x + PREVIEW_PADDING;
    let content_width = width - PREVIEW_PADDING * 2.0;
    let mut ctx = PreviewContext::new(
        content_x,
        y + PREVIEW_PADDING,
        content_width,
        height,
        scroll_offset,
    );

    for block in blocks {
        render_block_preview(block, &mut ctx);
        ctx.add_spacing(8.0);
    }

    cmds.extend(ctx.cmds);
    cmds
}

/// Render a single block element in the preview.
fn render_block_preview(block: &MdBlock, ctx: &mut PreviewContext) {
    match block {
        MdBlock::Heading { level, inlines } => {
            let (font_size, spacing) = match level {
                1 => (28.0, 16.0),
                2 => (24.0, 14.0),
                3 => (20.0, 12.0),
                4 => (18.0, 10.0),
                5 => (16.0, 8.0),
                _ => (14.0, 8.0),
            };
            ctx.add_spacing(spacing);
            if ctx.is_visible(font_size + 4.0) {
                let text = inlines_to_plain_text(inlines);
                ctx.cmds.push(RenderCommand::Text {
                    x: ctx.x,
                    y: ctx.render_y(),
                    text,
                    font_size,
                    color: BLUE,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(ctx.width),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            ctx.y += font_size + 4.0;
            // Underline for h1 and h2.
            if *level <= 2 && ctx.is_visible(2.0) {
                ctx.cmds.push(RenderCommand::Line {
                    x1: ctx.x,
                    y1: ctx.render_y(),
                    x2: ctx.x + ctx.width,
                    y2: ctx.render_y(),
                    color: SURFACE1,
                    width: 1.0,
                });
                ctx.y += 4.0;
            }
            ctx.add_spacing(spacing);
        }
        MdBlock::Paragraph { inlines } => {
            render_inlines_preview(
                inlines,
                ctx,
                EDITOR_FONT_SIZE,
                TEXT,
                FontWeightHint::Regular,
            );
            ctx.add_spacing(8.0);
        }
        MdBlock::CodeBlock { language, code } => {
            let block_height = (code.lines().count() as f32 + 1.0) * LINE_HEIGHT + 16.0;
            if ctx.is_visible(block_height) {
                // Code block background.
                ctx.cmds.push(RenderCommand::FillRect {
                    x: ctx.x,
                    y: ctx.render_y(),
                    width: ctx.width,
                    height: block_height,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });

                // Language label.
                if !language.is_empty() {
                    ctx.cmds.push(RenderCommand::Text {
                        x: ctx.x + 8.0,
                        y: ctx.render_y() + 4.0,
                        text: language.clone(),
                        font_size: 10.0,
                        color: SUBTEXT0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(ctx.width - 16.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }

                // Code content.
                let code_y_start = if language.is_empty() { 8.0 } else { 20.0 };
                for (i, code_line) in code.lines().enumerate() {
                    let line_y = ctx.render_y() + code_y_start + (i as f32) * LINE_HEIGHT;
                    ctx.cmds.push(RenderCommand::Text {
                        x: ctx.x + 12.0,
                        y: line_y,
                        text: code_line.to_string(),
                        font_size: 13.0,
                        color: GREEN,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(ctx.width - 24.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
            ctx.y += block_height;
        }
        MdBlock::BlockQuote { children } => {
            let saved_x = ctx.x;
            let saved_width = ctx.width;

            // Quote bar.
            let quote_start_y = ctx.render_y();

            ctx.x += 16.0;
            ctx.width -= 16.0;

            let y_before = ctx.y;
            for child in children {
                render_block_preview(child, ctx);
            }
            let quote_height = ctx.y - y_before;

            if ctx.is_visible(quote_height) {
                ctx.cmds.push(RenderCommand::FillRect {
                    x: saved_x,
                    y: quote_start_y,
                    width: 4.0,
                    height: quote_height,
                    color: BLUE,
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            ctx.x = saved_x;
            ctx.width = saved_width;
        }
        MdBlock::UnorderedList { items } => {
            for item in items {
                if ctx.is_visible(LINE_HEIGHT) {
                    // Bullet or checkbox.
                    if let Some(checked) = item.task {
                        let checkbox_text = if checked { "[x]" } else { "[ ]" };
                        let cb_color = if checked { GREEN } else { OVERLAY0 };
                        ctx.cmds.push(RenderCommand::Text {
                            x: ctx.x,
                            y: ctx.render_y(),
                            text: checkbox_text.to_string(),
                            font_size: EDITOR_FONT_SIZE,
                            color: cb_color,
                            font_weight: FontWeightHint::Regular,
                            max_width: None,
                            overflow: TextOverflow::Clip,
                        });
                    } else {
                        ctx.cmds.push(RenderCommand::FillRect {
                            x: ctx.x + 4.0,
                            y: ctx.render_y() + 7.0,
                            width: 6.0,
                            height: 6.0,
                            color: TEXT,
                            corner_radii: CornerRadii::all(3.0),
                        });
                    }
                }
                let saved_x = ctx.x;
                let saved_w = ctx.width;
                ctx.x += 24.0;
                ctx.width -= 24.0;
                render_inlines_preview(
                    &item.inlines,
                    ctx,
                    EDITOR_FONT_SIZE,
                    TEXT,
                    FontWeightHint::Regular,
                );
                ctx.x = saved_x;
                ctx.width = saved_w;
                ctx.add_spacing(4.0);
            }
        }
        MdBlock::OrderedList { start, items } => {
            for (i, item) in items.iter().enumerate() {
                if ctx.is_visible(LINE_HEIGHT) {
                    let num_text = format!("{}.", start + i);
                    ctx.cmds.push(RenderCommand::Text {
                        x: ctx.x,
                        y: ctx.render_y(),
                        text: num_text,
                        font_size: EDITOR_FONT_SIZE,
                        color: TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }
                let saved_x = ctx.x;
                let saved_w = ctx.width;
                ctx.x += 24.0;
                ctx.width -= 24.0;
                render_inlines_preview(
                    &item.inlines,
                    ctx,
                    EDITOR_FONT_SIZE,
                    TEXT,
                    FontWeightHint::Regular,
                );
                ctx.x = saved_x;
                ctx.width = saved_w;
                ctx.add_spacing(4.0);
            }
        }
        MdBlock::HorizontalRule => {
            ctx.add_spacing(12.0);
            if ctx.is_visible(2.0) {
                ctx.cmds.push(RenderCommand::Line {
                    x1: ctx.x,
                    y1: ctx.render_y(),
                    x2: ctx.x + ctx.width,
                    y2: ctx.render_y(),
                    color: SURFACE1,
                    width: 2.0,
                });
            }
            ctx.y += 2.0;
            ctx.add_spacing(12.0);
        }
        MdBlock::Table {
            alignments,
            headers,
            rows,
        } => {
            let col_count = headers.len().max(1);
            let col_width = ctx.width / col_count as f32;
            let row_height = LINE_HEIGHT + 8.0;

            // Header row background.
            if ctx.is_visible(row_height) {
                ctx.cmds.push(RenderCommand::FillRect {
                    x: ctx.x,
                    y: ctx.render_y(),
                    width: ctx.width,
                    height: row_height,
                    color: SURFACE0,
                    corner_radii: CornerRadii::ZERO,
                });

                // Header cells.
                for (j, cell) in headers.iter().enumerate() {
                    let cell_x = ctx.x + (j as f32) * col_width + 8.0;
                    let text = inlines_to_plain_text(cell);
                    ctx.cmds.push(RenderCommand::Text {
                        x: cell_x,
                        y: ctx.render_y() + 4.0,
                        text,
                        font_size: EDITOR_FONT_SIZE,
                        color: TEXT,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(col_width - 16.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
            }
            ctx.y += row_height;

            // Header separator line.
            if ctx.is_visible(1.0) {
                ctx.cmds.push(RenderCommand::Line {
                    x1: ctx.x,
                    y1: ctx.render_y(),
                    x2: ctx.x + ctx.width,
                    y2: ctx.render_y(),
                    color: SURFACE1,
                    width: 2.0,
                });
            }
            ctx.y += 2.0;

            // Data rows.
            for (row_idx, row) in rows.iter().enumerate() {
                if ctx.is_visible(row_height) {
                    // Alternating row background.
                    if row_idx % 2 == 1 {
                        ctx.cmds.push(RenderCommand::FillRect {
                            x: ctx.x,
                            y: ctx.render_y(),
                            width: ctx.width,
                            height: row_height,
                            color: Color::rgba(SURFACE0.r, SURFACE0.g, SURFACE0.b, 80),
                            corner_radii: CornerRadii::ZERO,
                        });
                    }

                    for (j, cell) in row.iter().enumerate() {
                        let cell_x = ctx.x + (j as f32) * col_width + 8.0;
                        let text = inlines_to_plain_text(cell);
                        ctx.cmds.push(RenderCommand::Text {
                            x: cell_x,
                            y: ctx.render_y() + 4.0,
                            text,
                            font_size: EDITOR_FONT_SIZE,
                            color: TEXT,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(col_width - 16.0),
                            overflow: TextOverflow::Ellipsis,
                        });
                    }
                }
                ctx.y += row_height;
            }

            // Table border.
            let total_height = row_height * (1 + rows.len()) as f32 + 2.0;
            let _ = alignments; // used for alignment but rendering simplified here
            ctx.cmds.push(RenderCommand::StrokeRect {
                x: ctx.x,
                y: ctx.render_y() - total_height,
                width: ctx.width,
                height: total_height,
                color: SURFACE1,
                line_width: 1.0,
                corner_radii: CornerRadii::ZERO,
            });

            // Vertical column separators.
            for j in 1..col_count {
                let sep_x = ctx.x + (j as f32) * col_width;
                ctx.cmds.push(RenderCommand::Line {
                    x1: sep_x,
                    y1: ctx.render_y() - total_height,
                    x2: sep_x,
                    y2: ctx.render_y(),
                    color: SURFACE1,
                    width: 1.0,
                });
            }
        }
    }
}

/// Render inline elements in the preview pane.
fn render_inlines_preview(
    inlines: &[MdInline],
    ctx: &mut PreviewContext,
    font_size: f32,
    color: Color,
    weight: FontWeightHint,
) {
    let text = inlines_to_styled_text(inlines);
    if text.is_empty() {
        ctx.y += LINE_HEIGHT;
        return;
    }

    // Runs of a paragraph sit side by side, so each one has to advance the pen
    // by its own width — measured in the size and weight it is actually drawn
    // in. Bold `code` at 13 px is not the same width as the regular 14 px text
    // beside it, and neither is a byte count.
    let mut offset_x = 0.0;
    for segment in &text {
        let seg_size = segment.font_size.unwrap_or(font_size);
        let seg_weight = segment.weight.unwrap_or(weight);
        let seg_width = text::measure(&segment.text, seg_size, seg_weight);

        // Wrap *before* drawing. Deciding on the previous run's overflow left
        // this one already painted past the right edge of the pane.
        if offset_x > 0.0 && offset_x + seg_width > ctx.width {
            offset_x = 0.0;
            ctx.y += LINE_HEIGHT;
        }

        if ctx.is_visible(seg_size + 4.0) {
            ctx.cmds.push(RenderCommand::Text {
                x: ctx.x + offset_x,
                y: ctx.render_y(),
                text: segment.text.clone(),
                font_size: seg_size,
                color: segment.color.unwrap_or(color),
                font_weight: seg_weight,
                max_width: Some(ctx.width - offset_x),
                overflow: TextOverflow::Ellipsis,
            });
        }
        offset_x += seg_width;
    }
    ctx.y += LINE_HEIGHT;
}

/// A styled text segment for preview rendering.
struct StyledSegment {
    /// The text content.
    text: String,
    /// Optional override color.
    color: Option<Color>,
    /// Optional override font weight.
    weight: Option<FontWeightHint>,
    /// Optional override font size.
    font_size: Option<f32>,
}

/// Convert inline elements to styled text segments for preview rendering.
fn inlines_to_styled_text(inlines: &[MdInline]) -> Vec<StyledSegment> {
    let mut segments = Vec::new();
    for inline in inlines {
        match inline {
            MdInline::Text(t) => {
                segments.push(StyledSegment {
                    text: t.clone(),
                    color: None,
                    weight: None,
                    font_size: None,
                });
            }
            MdInline::Bold(inner) => {
                let inner_text = inlines_to_plain_text(inner);
                segments.push(StyledSegment {
                    text: inner_text,
                    color: None,
                    weight: Some(FontWeightHint::Bold),
                    font_size: None,
                });
            }
            MdInline::Italic(inner) => {
                let inner_text = inlines_to_plain_text(inner);
                segments.push(StyledSegment {
                    text: inner_text,
                    color: Some(LAVENDER),
                    weight: Some(FontWeightHint::Light),
                    font_size: None,
                });
            }
            MdInline::Strikethrough(inner) => {
                let inner_text = inlines_to_plain_text(inner);
                segments.push(StyledSegment {
                    text: inner_text,
                    color: Some(OVERLAY0),
                    weight: None,
                    font_size: None,
                });
            }
            MdInline::InlineCode(code) => {
                segments.push(StyledSegment {
                    text: code.clone(),
                    color: Some(GREEN),
                    weight: None,
                    font_size: None,
                });
            }
            MdInline::Link { text, url: _ } => {
                let link_text = inlines_to_plain_text(text);
                segments.push(StyledSegment {
                    text: link_text,
                    color: Some(BLUE),
                    weight: None,
                    font_size: None,
                });
            }
            MdInline::Image { alt, url: _ } => {
                segments.push(StyledSegment {
                    text: format!("[Image: {}]", alt),
                    color: Some(PEACH),
                    weight: None,
                    font_size: None,
                });
            }
            MdInline::LineBreak => {
                segments.push(StyledSegment {
                    text: " ".to_string(),
                    color: None,
                    weight: None,
                    font_size: None,
                });
            }
        }
    }
    segments
}

/// Convert inline elements to plain text (no formatting).
pub fn inlines_to_plain_text(inlines: &[MdInline]) -> String {
    let mut result = String::new();
    for inline in inlines {
        match inline {
            MdInline::Text(t) => result.push_str(t),
            MdInline::Bold(inner) | MdInline::Italic(inner) | MdInline::Strikethrough(inner) => {
                result.push_str(&inlines_to_plain_text(inner));
            }
            MdInline::InlineCode(code) => result.push_str(code),
            MdInline::Link { text, .. } => {
                result.push_str(&inlines_to_plain_text(text));
            }
            MdInline::Image { alt, .. } => result.push_str(alt),
            MdInline::LineBreak => result.push(' '),
        }
    }
    result
}

// ============================================================================
// Rendering — toolbar
// ============================================================================

/// Toolbar button definition.
#[derive(Clone, Debug)]
pub struct ToolbarButton {
    /// Display label for the button.
    pub label: String,
    /// Tooltip text.
    pub tooltip: String,
    /// Button action identifier.
    pub action: ToolbarAction,
}

/// Actions that can be triggered from the toolbar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    /// Create a new document.
    NewFile,
    /// Open a file.
    OpenFile,
    /// Save the current file.
    Save,
    /// Save with a new name.
    SaveAs,
    /// Toggle bold.
    Bold,
    /// Toggle italic.
    Italic,
    /// Insert heading.
    Heading,
    /// Insert link.
    Link,
    /// Insert image.
    Image,
    /// Insert code block.
    CodeBlock,
    /// Insert unordered list.
    UnorderedList,
    /// Insert ordered list.
    OrderedList,
    /// Insert table.
    Table,
    /// Insert horizontal rule.
    HRule,
    /// Toggle view mode.
    ToggleView,
    /// Toggle table of contents.
    ToggleToc,
    /// Export to HTML.
    ExportHtml,
    /// Open find/replace.
    FindReplace,
    /// Undo.
    Undo,
    /// Redo.
    Redo,
    /// Apply a template.
    ApplyTemplate(usize),
}

/// Get the default toolbar buttons.
pub fn default_toolbar_buttons() -> Vec<ToolbarButton> {
    vec![
        ToolbarButton {
            label: "New".to_string(),
            tooltip: "New file (Ctrl+N)".to_string(),
            action: ToolbarAction::NewFile,
        },
        ToolbarButton {
            label: "Open".to_string(),
            tooltip: "Open file (Ctrl+O)".to_string(),
            action: ToolbarAction::OpenFile,
        },
        ToolbarButton {
            label: "Save".to_string(),
            tooltip: "Save file (Ctrl+S)".to_string(),
            action: ToolbarAction::Save,
        },
        ToolbarButton {
            label: "|".to_string(),
            tooltip: String::new(),
            action: ToolbarAction::NewFile, // separator, not clickable
        },
        ToolbarButton {
            label: "B".to_string(),
            tooltip: "Bold (Ctrl+B)".to_string(),
            action: ToolbarAction::Bold,
        },
        ToolbarButton {
            label: "I".to_string(),
            tooltip: "Italic (Ctrl+I)".to_string(),
            action: ToolbarAction::Italic,
        },
        ToolbarButton {
            label: "H".to_string(),
            tooltip: "Heading".to_string(),
            action: ToolbarAction::Heading,
        },
        ToolbarButton {
            label: "Link".to_string(),
            tooltip: "Link (Ctrl+K)".to_string(),
            action: ToolbarAction::Link,
        },
        ToolbarButton {
            label: "Img".to_string(),
            tooltip: "Image".to_string(),
            action: ToolbarAction::Image,
        },
        ToolbarButton {
            label: "<>".to_string(),
            tooltip: "Code block (Ctrl+Shift+K)".to_string(),
            action: ToolbarAction::CodeBlock,
        },
        ToolbarButton {
            label: "UL".to_string(),
            tooltip: "Unordered list".to_string(),
            action: ToolbarAction::UnorderedList,
        },
        ToolbarButton {
            label: "OL".to_string(),
            tooltip: "Ordered list".to_string(),
            action: ToolbarAction::OrderedList,
        },
        ToolbarButton {
            label: "Tbl".to_string(),
            tooltip: "Table".to_string(),
            action: ToolbarAction::Table,
        },
        ToolbarButton {
            label: "---".to_string(),
            tooltip: "Horizontal rule".to_string(),
            action: ToolbarAction::HRule,
        },
        ToolbarButton {
            label: "|".to_string(),
            tooltip: String::new(),
            action: ToolbarAction::NewFile,
        },
        ToolbarButton {
            label: "Undo".to_string(),
            tooltip: "Undo (Ctrl+Z)".to_string(),
            action: ToolbarAction::Undo,
        },
        ToolbarButton {
            label: "Redo".to_string(),
            tooltip: "Redo (Ctrl+Y)".to_string(),
            action: ToolbarAction::Redo,
        },
        ToolbarButton {
            label: "|".to_string(),
            tooltip: String::new(),
            action: ToolbarAction::NewFile,
        },
        ToolbarButton {
            label: "Find".to_string(),
            tooltip: "Find & Replace (Ctrl+H)".to_string(),
            action: ToolbarAction::FindReplace,
        },
        ToolbarButton {
            label: "View".to_string(),
            tooltip: "Toggle view mode".to_string(),
            action: ToolbarAction::ToggleView,
        },
        ToolbarButton {
            label: "ToC".to_string(),
            tooltip: "Toggle table of contents".to_string(),
            action: ToolbarAction::ToggleToc,
        },
        ToolbarButton {
            label: "HTML".to_string(),
            tooltip: "Export to HTML".to_string(),
            action: ToolbarAction::ExportHtml,
        },
    ]
}

/// Render the toolbar.
pub fn render_toolbar(buttons: &[ToolbarButton], x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    // Toolbar background.
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: TOOLBAR_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Bottom border.
    cmds.push(RenderCommand::Line {
        x1: x,
        y1: y + TOOLBAR_HEIGHT,
        x2: x + width,
        y2: y + TOOLBAR_HEIGHT,
        color: SURFACE0,
        width: 1.0,
    });

    let mut btn_x = x + 8.0;
    let btn_y = y + 4.0;
    let btn_height = TOOLBAR_HEIGHT - 8.0;

    for button in buttons {
        if button.label == "|" {
            // Separator.
            cmds.push(RenderCommand::Line {
                x1: btn_x + 4.0,
                y1: btn_y + 2.0,
                x2: btn_x + 4.0,
                y2: btn_y + btn_height - 2.0,
                color: SURFACE1,
                width: 1.0,
            });
            btn_x += 12.0;
            continue;
        }

        let btn_width = text::width(&button.label, TOOLBAR_FONT_SIZE) + 16.0;

        // Button background.
        cmds.push(RenderCommand::FillRect {
            x: btn_x,
            y: btn_y,
            width: btn_width,
            height: btn_height,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Button label.
        cmds.push(RenderCommand::Text {
            x: btn_x + 8.0,
            y: btn_y + 5.0,
            text: button.label.clone(),
            font_size: TOOLBAR_FONT_SIZE,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(btn_width - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        btn_x += btn_width + 4.0;
    }

    cmds
}

// ============================================================================
// Rendering — tab bar
// ============================================================================

/// Render the tab bar for multi-document editing.
pub fn render_tab_bar(
    documents: &Tabs<Document>,
    x: f32,
    y: f32,
    width: f32,
) -> Vec<RenderCommand> {
    // Which tab is in front is the tab set's own business, so it is read
    // from it rather than passed alongside — the two could not disagree.
    let active_idx = documents.active_index();
    let mut cmds = Vec::new();

    // Tab bar background.
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: TAB_BAR_HEIGHT,
        color: CRUST,
        corner_radii: CornerRadii::ZERO,
    });

    let mut tab_x = x + 4.0;
    let tab_y = y + 4.0;
    let tab_height = TAB_BAR_HEIGHT - 4.0;

    for (i, doc) in documents.iter().enumerate() {
        let is_active = i == active_idx;
        let label = if doc.modified {
            format!("{} *", doc.name)
        } else {
            doc.name.clone()
        };
        // The active tab is drawn bold, so it has to be *measured* bold —
        // sizing every tab as if it were regular made the active one the one
        // that overflowed.
        let label_weight = if is_active {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        };
        let tab_width =
            (text::measure(&label, TOOLBAR_FONT_SIZE, label_weight) + 24.0).clamp(80.0, 200.0);

        let bg_color = if is_active { BASE } else { MANTLE };
        let text_color = if is_active { TEXT } else { SUBTEXT0 };

        // Tab background.
        cmds.push(RenderCommand::FillRect {
            x: tab_x,
            y: tab_y,
            width: tab_width,
            height: tab_height,
            color: bg_color,
            corner_radii: CornerRadii {
                top_left: 6.0,
                top_right: 6.0,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        // Active tab indicator.
        if is_active {
            cmds.push(RenderCommand::FillRect {
                x: tab_x,
                y: tab_y,
                width: tab_width,
                height: 2.0,
                color: BLUE,
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Tab label.
        cmds.push(RenderCommand::Text {
            x: tab_x + 8.0,
            y: tab_y + 6.0,
            text: label,
            font_size: TOOLBAR_FONT_SIZE,
            color: text_color,
            font_weight: label_weight,
            max_width: Some(tab_width - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Close button (X).
        cmds.push(RenderCommand::Text {
            x: tab_x + tab_width - 18.0,
            y: tab_y + 6.0,
            text: "x".to_string(),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        tab_x += tab_width + 2.0;
    }

    cmds
}

// ============================================================================
// Rendering — status bar
// ============================================================================

/// Render the status bar with document statistics.
pub fn render_status_bar(
    doc: &Document,
    view_mode: ViewMode,
    x: f32,
    y: f32,
    width: f32,
    autosave_enabled: bool,
) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    // Status bar background.
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: STATUS_BAR_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Top border.
    cmds.push(RenderCommand::Line {
        x1: x,
        y1: y,
        x2: x + width,
        y2: y,
        color: SURFACE0,
        width: 1.0,
    });

    // Left side: cursor position.
    // `cursor_col` is a byte offset; the column the user counts is a
    // character, so a line with an accent must not jump the reading by one.
    let cursor_column = doc
        .lines
        .get(doc.cursor_line)
        .and_then(|line| line.get(..doc.cursor_col.min(line.len())))
        .map_or(doc.cursor_col, |prefix| prefix.chars().count());
    let pos_text = format!("Ln {}, Col {}", doc.cursor_line + 1, cursor_column + 1);
    cmds.push(RenderCommand::Text {
        x: x + 12.0,
        y: y + 5.0,
        text: pos_text,
        font_size: STATUS_FONT_SIZE,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Center: statistics.
    let words = doc.word_count();
    let chars = doc.char_count();
    let lines = doc.lines.len();
    let reading_mins = doc.reading_time_minutes();
    let stats_text = format!(
        "{} words | {} chars | {} lines | ~{:.0} min read",
        words, chars, lines, reading_mins
    );
    cmds.push(RenderCommand::Text {
        x: text::center_x(
            &stats_text,
            x + width / 2.0,
            STATUS_FONT_SIZE,
            FontWeightHint::Regular,
        ),
        y: y + 5.0,
        text: stats_text,
        font_size: STATUS_FONT_SIZE,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width * 0.6),
        overflow: TextOverflow::Ellipsis,
    });

    // Right side: view mode and auto-save status.
    let right_items = format!(
        "{}  {}  {}",
        view_mode.label(),
        if autosave_enabled {
            "Auto-save ON"
        } else {
            "Auto-save OFF"
        },
        if doc.modified { "Modified" } else { "Saved" }
    );
    cmds.push(RenderCommand::Text {
        x: text::right_x(
            &right_items,
            x + width - 12.0,
            STATUS_FONT_SIZE,
            FontWeightHint::Regular,
        ),
        y: y + 5.0,
        text: right_items,
        font_size: STATUS_FONT_SIZE,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    cmds
}

// ============================================================================
// Rendering — table of contents sidebar
// ============================================================================

/// Render the table of contents sidebar.
pub fn render_toc_sidebar(entries: &[TocEntry], x: f32, y: f32, height: f32) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    // Sidebar background.
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width: TOC_SIDEBAR_WIDTH,
        height,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Right border.
    cmds.push(RenderCommand::Line {
        x1: x + TOC_SIDEBAR_WIDTH,
        y1: y,
        x2: x + TOC_SIDEBAR_WIDTH,
        y2: y + height,
        color: SURFACE0,
        width: 1.0,
    });

    // Title.
    cmds.push(RenderCommand::Text {
        x: x + 12.0,
        y: y + 8.0,
        text: "Table of Contents".to_string(),
        font_size: 12.0,
        color: BLUE,
        font_weight: FontWeightHint::Bold,
        max_width: Some(TOC_SIDEBAR_WIDTH - 24.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Entries.
    let mut entry_y = y + 32.0;
    for entry in entries {
        if entry_y > y + height {
            break;
        }
        let indent = (entry.level.saturating_sub(1) as f32) * 12.0;
        let entry_color = match entry.level {
            1 => BLUE,
            2 => LAVENDER,
            3 => GREEN,
            4 => SUBTEXT1,
            _ => SUBTEXT0,
        };
        let font_weight = if entry.level <= 2 {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        };
        let font_size = match entry.level {
            1 => 12.0,
            2 => 11.5,
            _ => 11.0,
        };

        cmds.push(RenderCommand::Text {
            x: x + 12.0 + indent,
            y: entry_y,
            text: entry.text.clone(),
            font_size,
            color: entry_color,
            font_weight,
            max_width: Some(TOC_SIDEBAR_WIDTH - 24.0 - indent),
            overflow: TextOverflow::Ellipsis,
        });

        entry_y += 20.0;
    }

    cmds
}

// ============================================================================
// Rendering — find/replace panel
// ============================================================================

/// Render the find and replace panel.
pub fn render_find_replace(
    state: &FindReplaceState,
    x: f32,
    y: f32,
    width: f32,
) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    if !state.visible {
        return cmds;
    }

    // Panel background.
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: FIND_PANEL_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Bottom border.
    cmds.push(RenderCommand::Line {
        x1: x,
        y1: y + FIND_PANEL_HEIGHT,
        x2: x + width,
        y2: y + FIND_PANEL_HEIGHT,
        color: SURFACE0,
        width: 1.0,
    });

    // Find row.
    cmds.push(RenderCommand::Text {
        x: x + 12.0,
        y: y + 8.0,
        text: "Find:".to_string(),
        font_size: 12.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Find input box.
    cmds.push(RenderCommand::FillRect {
        x: x + 70.0,
        y: y + 4.0,
        width: width * 0.4,
        height: 22.0,
        color: SURFACE0,
        corner_radii: CornerRadii::all(4.0),
    });

    cmds.push(RenderCommand::Text {
        x: x + 74.0,
        y: y + 8.0,
        text: state.query.clone(),
        font_size: 12.0,
        color: TEXT,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width * 0.4 - 8.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Match count.
    let match_text = if state.matches.is_empty() {
        "No matches".to_string()
    } else {
        format!(
            "{} of {} matches",
            state.current_match + 1,
            state.matches.len()
        )
    };
    cmds.push(RenderCommand::Text {
        x: x + 70.0 + width * 0.4 + 12.0,
        y: y + 8.0,
        text: match_text,
        font_size: 11.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Replace row.
    cmds.push(RenderCommand::Text {
        x: x + 12.0,
        y: y + 36.0,
        text: "Replace:".to_string(),
        font_size: 12.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Replace input box.
    cmds.push(RenderCommand::FillRect {
        x: x + 70.0,
        y: y + 32.0,
        width: width * 0.4,
        height: 22.0,
        color: SURFACE0,
        corner_radii: CornerRadii::all(4.0),
    });

    cmds.push(RenderCommand::Text {
        x: x + 74.0,
        y: y + 36.0,
        text: state.replacement.clone(),
        font_size: 12.0,
        color: TEXT,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width * 0.4 - 8.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Action buttons.
    let btn_x = x + 70.0 + width * 0.4 + 12.0;
    let btn_labels = ["Replace", "Replace All", "Close"];
    let mut bx = btn_x;
    for label in &btn_labels {
        let bw = text::width(label, SMALL_BUTTON_FONT_SIZE) + 16.0;
        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: y + 32.0,
            width: bw,
            height: 22.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: bx + 8.0,
            y: y + 36.0,
            text: label.to_string(),
            font_size: SMALL_BUTTON_FONT_SIZE,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(bw - 16.0),
            overflow: TextOverflow::Ellipsis,
        });
        bx += bw + 4.0;
    }

    cmds
}

// ============================================================================
// Rendering — template chooser dialog
// ============================================================================

/// Render a template chooser dialog overlay.
pub fn render_template_chooser(x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    // Overlay dimmer.
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height,
        color: Color::rgba(0, 0, 0, 150),
        corner_radii: CornerRadii::ZERO,
    });

    let dialog_width = 400.0;
    let dialog_height = 300.0;
    let dialog_x = x + (width - dialog_width) / 2.0;
    let dialog_y = y + (height - dialog_height) / 2.0;

    // Dialog shadow.
    cmds.push(RenderCommand::BoxShadow {
        x: dialog_x,
        y: dialog_y,
        width: dialog_width,
        height: dialog_height,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 20.0,
        spread: 0.0,
        color: Color::rgba(0, 0, 0, 100),
        corner_radii: CornerRadii::all(8.0),
    });

    // Dialog background.
    cmds.push(RenderCommand::FillRect {
        x: dialog_x,
        y: dialog_y,
        width: dialog_width,
        height: dialog_height,
        color: MANTLE,
        corner_radii: CornerRadii::all(8.0),
    });

    // Dialog border.
    cmds.push(RenderCommand::StrokeRect {
        x: dialog_x,
        y: dialog_y,
        width: dialog_width,
        height: dialog_height,
        color: SURFACE0,
        line_width: 1.0,
        corner_radii: CornerRadii::all(8.0),
    });

    // Title.
    cmds.push(RenderCommand::Text {
        x: dialog_x + 20.0,
        y: dialog_y + 20.0,
        text: "Choose a Template".to_string(),
        font_size: 18.0,
        color: BLUE,
        font_weight: FontWeightHint::Bold,
        max_width: Some(dialog_width - 40.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Template buttons.
    let templates = Template::all();
    let mut btn_y = dialog_y + 56.0;
    for template in templates {
        let btn_height = 40.0;
        let btn_width = dialog_width - 40.0;

        cmds.push(RenderCommand::FillRect {
            x: dialog_x + 20.0,
            y: btn_y,
            width: btn_width,
            height: btn_height,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: dialog_x + 32.0,
            y: btn_y + 12.0,
            text: template.label().to_string(),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(btn_width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        btn_y += btn_height + 8.0;
    }

    cmds
}

// ============================================================================
// Full application state
// ============================================================================

/// The full state of the markdown editor application.
pub struct App {
    /// All open documents, and which one is in front.
    ///
    /// A `Vec<Document>` plus an `active_doc: usize` said the same thing but
    /// left "there is always a document open" and "the index names one" as
    /// conventions the call sites had to keep. `Tabs` makes both facts the
    /// type's job — see [`guitk::tabs::Tabs`].
    pub documents: Tabs<Document>,
    /// Current view mode.
    pub view_mode: ViewMode,
    /// Whether the table of contents sidebar is visible.
    pub toc_visible: bool,
    /// Find and replace state.
    pub find_state: FindReplaceState,
    /// Toolbar button definitions.
    pub toolbar_buttons: Vec<ToolbarButton>,
    /// Whether auto-save is enabled.
    pub autosave_enabled: bool,
    /// Auto-save interval in seconds.
    pub autosave_interval: u64,
    /// Whether the template chooser dialog is open.
    pub template_chooser_open: bool,
    /// Cached parsed blocks for the active document.
    pub cached_blocks: Vec<MdBlock>,
    /// Cached table of contents for the active document.
    pub cached_toc: Vec<TocEntry>,
    /// Window width.
    pub window_width: f32,
    /// Window height.
    pub window_height: f32,
    /// Pending external-change prompt (file edited/deleted outside the editor).
    pub external_prompt: Option<ExternalChangePrompt>,
}

/// A pending prompt shown when the active document's file changed on disk.
///
/// Presents the user with keep-current / reload / merge / review options (see
/// [`App::resolve_external`]). When [`review`](Self::review) is `Some`, the
/// editor is in the side-by-side review sub-mode.
pub struct ExternalChangePrompt {
    /// Index of the document (tab) the prompt concerns.
    pub tab: usize,
    /// What changed on disk.
    pub change: DiskChange,
    /// Active review state when the user chose "review the merge".
    pub review: Option<MergeReview>,
}

/// The four top-level responses to an [`ExternalChangePrompt`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalChoice {
    /// Keep the current buffer, ignoring the disk change.
    KeepCurrent,
    /// Discard local edits and reload the file from disk.
    Reload,
    /// Auto-merge disk changes into the buffer.
    Merge,
    /// Open the side-by-side review to resolve conflicts manually.
    Review,
}

impl App {
    /// Create a new application with a single empty document.
    pub fn new(window_width: f32, window_height: f32) -> Self {
        let doc = Document::new();
        let text = doc.full_text();
        let blocks = parse_markdown(&text);
        let toc = extract_toc(&text);
        Self {
            documents: Tabs::with(doc),
            view_mode: ViewMode::Split,
            toc_visible: false,
            find_state: FindReplaceState::new(),
            toolbar_buttons: default_toolbar_buttons(),
            autosave_enabled: true,
            autosave_interval: DEFAULT_AUTOSAVE_INTERVAL,
            template_chooser_open: false,
            cached_blocks: blocks,
            cached_toc: toc,
            window_width,
            window_height,
            external_prompt: None,
        }
    }

    /// Get a reference to the currently active document.
    pub fn active_document(&self) -> &Document {
        self.documents.active()
    }

    /// Get a mutable reference to the currently active document.
    pub fn active_document_mut(&mut self) -> &mut Document {
        self.documents.active_mut()
    }

    /// Index of the tab in front.
    pub fn active_doc(&self) -> usize {
        self.documents.active_index()
    }

    /// Refresh the cached parsed markdown and TOC for the active document.
    pub fn refresh_cache(&mut self) {
        let text = self.documents.active().full_text();
        self.cached_blocks = parse_markdown(&text);
        self.cached_toc = extract_toc(&text);
    }

    /// Create a new blank document and add it as a new tab.
    pub fn new_document(&mut self) {
        self.documents.open(Document::new());
        self.refresh_cache();
    }

    /// Create a new document from a template and add it as a new tab.
    pub fn new_from_template(&mut self, template: Template) {
        self.documents.open(Document::from_template(template));
        self.refresh_cache();
    }

    /// Open a file and add it as a new tab.
    pub fn open_file(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        let doc = Document::from_file(path)?;
        self.documents.open(doc);
        self.refresh_cache();
        Ok(())
    }

    /// Close the document at the given index. Closing the last one leaves a
    /// fresh empty document rather than no document at all.
    pub fn close_document(&mut self, idx: usize) {
        self.documents.close(idx);
        self.refresh_cache();
    }

    /// Switch to the document at the given index.
    pub fn switch_tab(&mut self, idx: usize) {
        if idx < self.documents.count() {
            self.documents.set_active(idx);
            self.refresh_cache();
        }
    }

    // ======================================================================
    // External-change handling
    // ======================================================================

    /// Check the active document's file for external modification and, if it
    /// changed in a way that needs the user's attention, raise a prompt.
    ///
    /// Policy: if the file changed but the buffer has no unsaved edits, the disk
    /// version is adopted automatically. A prompt is raised only when there is a
    /// genuine choice — the buffer is modified and the file also changed, or the
    /// file was deleted. Returns `true` when a prompt was raised.
    pub fn check_external_change(&mut self) -> bool {
        if self.external_prompt.is_some() {
            return false;
        }
        let tab = self.active_doc();
        let Some(doc) = self.documents.get(tab) else {
            return false;
        };
        match doc.disk_changed() {
            DiskChange::Unchanged => false,
            DiskChange::Modified { disk } => {
                if doc.modified {
                    self.external_prompt = Some(ExternalChangePrompt {
                        tab,
                        change: DiskChange::Modified { disk },
                        review: None,
                    });
                    true
                } else {
                    if let Some(doc) = self.documents.get_mut(tab) {
                        doc.reload_from_disk(&disk);
                    }
                    self.refresh_cache();
                    false
                }
            }
            DiskChange::Deleted => {
                self.external_prompt = Some(ExternalChangePrompt {
                    tab,
                    change: DiskChange::Deleted,
                    review: None,
                });
                true
            }
        }
    }

    /// Respond to the pending external-change prompt.
    ///
    /// [`ExternalChoice::Review`] transitions into review sub-mode; the other
    /// three resolve immediately and clear the prompt.
    pub fn resolve_external(&mut self, choice: ExternalChoice) {
        let Some(prompt) = self.external_prompt.as_ref() else {
            return;
        };
        let tab = prompt.tab;
        let disk = match &prompt.change {
            DiskChange::Modified { disk } => Some(disk.clone()),
            _ => None,
        };

        match choice {
            ExternalChoice::KeepCurrent => {
                if let Some(doc) = self.documents.get_mut(tab) {
                    doc.keep_current();
                    doc.modified = true;
                }
                self.external_prompt = None;
            }
            ExternalChoice::Reload => {
                if let (Some(doc), Some(disk)) = (self.documents.get_mut(tab), disk) {
                    doc.reload_from_disk(&disk);
                }
                self.external_prompt = None;
                self.refresh_cache();
            }
            ExternalChoice::Merge => {
                if let (Some(doc), Some(disk)) = (self.documents.get_mut(tab), disk) {
                    doc.merge_from_disk(&disk);
                }
                self.external_prompt = None;
                self.refresh_cache();
            }
            ExternalChoice::Review => {
                if let (Some(doc), Some(disk)) = (self.documents.get(tab), disk.as_ref()) {
                    let review = MergeReview::new(doc.merge_preview(disk));
                    if let Some(prompt) = self.external_prompt.as_mut() {
                        prompt.review = Some(review);
                    }
                }
            }
        }
    }

    /// Change the resolution of conflict `index` in the active review.
    pub fn review_set_choice(&mut self, index: usize, choice: ConflictChoice) {
        if let Some(review) = self
            .external_prompt
            .as_mut()
            .and_then(|p| p.review.as_mut())
        {
            review.set_choice(index, choice);
        }
    }

    /// Accept the reviewed merge, applying the chosen resolutions to the buffer.
    pub fn review_accept(&mut self) {
        let Some(prompt) = self.external_prompt.as_ref() else {
            return;
        };
        let tab = prompt.tab;
        let (Some(review), DiskChange::Modified { disk }) = (&prompt.review, &prompt.change) else {
            return;
        };
        let merged = review.accepted_text();
        let disk = disk.clone();
        if let Some(doc) = self.documents.get_mut(tab) {
            doc.apply_merged(&merged, &disk);
        }
        self.external_prompt = None;
        self.refresh_cache();
    }

    /// Cancel the review, returning to the top-level prompt options.
    pub fn review_cancel(&mut self) {
        if let Some(prompt) = self.external_prompt.as_mut() {
            prompt.review = None;
        }
    }

    /// Dismiss the external-change prompt without taking any action.
    pub fn dismiss_external(&mut self) {
        self.external_prompt = None;
    }

    /// Handle a toolbar action.
    pub fn handle_toolbar_action(&mut self, action: &ToolbarAction) {
        match action {
            ToolbarAction::NewFile => self.new_document(),
            ToolbarAction::OpenFile => {
                // In a real app, this would open a file dialog.
                // For now, we create a new document.
                self.new_document();
            }
            ToolbarAction::Save => {
                let _ = self.active_document_mut().save();
            }
            ToolbarAction::SaveAs => {
                // Would open a save dialog in a real app.
            }
            ToolbarAction::Bold => {
                insert_bold(self.active_document_mut());
                self.refresh_cache();
            }
            ToolbarAction::Italic => {
                insert_italic(self.active_document_mut());
                self.refresh_cache();
            }
            ToolbarAction::Heading => {
                insert_heading(self.active_document_mut(), 2);
                self.refresh_cache();
            }
            ToolbarAction::Link => {
                insert_link(self.active_document_mut());
                self.refresh_cache();
            }
            ToolbarAction::Image => {
                insert_image(self.active_document_mut());
                self.refresh_cache();
            }
            ToolbarAction::CodeBlock => {
                insert_code_block(self.active_document_mut());
                self.refresh_cache();
            }
            ToolbarAction::UnorderedList => {
                insert_unordered_list(self.active_document_mut());
                self.refresh_cache();
            }
            ToolbarAction::OrderedList => {
                insert_ordered_list(self.active_document_mut());
                self.refresh_cache();
            }
            ToolbarAction::Table => {
                insert_table(self.active_document_mut());
                self.refresh_cache();
            }
            ToolbarAction::HRule => {
                insert_horizontal_rule(self.active_document_mut());
                self.refresh_cache();
            }
            ToolbarAction::ToggleView => {
                self.view_mode = self.view_mode.next();
            }
            ToolbarAction::ToggleToc => {
                self.toc_visible = !self.toc_visible;
            }
            ToolbarAction::ExportHtml => {
                let html = export_html(&self.cached_blocks);
                // In a real app, write to a file. For now, store in memory.
                let _ = html;
            }
            ToolbarAction::FindReplace => {
                self.find_state.visible = !self.find_state.visible;
            }
            ToolbarAction::Undo => {
                self.active_document_mut().undo();
                self.refresh_cache();
            }
            ToolbarAction::Redo => {
                self.active_document_mut().redo();
                self.refresh_cache();
            }
            ToolbarAction::ApplyTemplate(idx) => {
                let templates = Template::all();
                if let Some(&template) = templates.get(*idx) {
                    self.new_from_template(template);
                }
                self.template_chooser_open = false;
            }
        }
    }

    /// Perform auto-save if conditions are met.
    pub fn tick_autosave(&mut self, elapsed_seconds: u64) {
        if !self.autosave_enabled {
            return;
        }
        for doc in self.documents.iter_mut() {
            doc.seconds_since_save += elapsed_seconds;
            if doc.modified
                && doc.path.is_some()
                && doc.seconds_since_save >= self.autosave_interval
            {
                let _ = doc.save();
            }
        }
    }

    /// Compute scroll sync: map editor scroll position to preview scroll position.
    pub fn sync_scroll(&mut self) {
        let doc = self.documents.active();
        let total_lines = doc.lines.len().max(1) as f32;
        let scroll_fraction = doc.scroll_line as f32 / total_lines;
        // Estimate total preview height (rough approximation).
        let estimated_preview_height = total_lines * LINE_HEIGHT * 1.5;
        self.documents.active_mut().preview_scroll = scroll_fraction * estimated_preview_height;
    }

    /// Render the full application frame.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Full window background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        let mut content_y: f32 = 0.0;

        // Toolbar.
        cmds.extend(render_toolbar(
            &self.toolbar_buttons,
            0.0,
            content_y,
            self.window_width,
        ));
        content_y += TOOLBAR_HEIGHT;

        // Tab bar.
        cmds.extend(render_tab_bar(
            &self.documents,
            0.0,
            content_y,
            self.window_width,
        ));
        content_y += TAB_BAR_HEIGHT;

        // Find/replace panel.
        let find_panel_offset = if self.find_state.visible {
            cmds.extend(render_find_replace(
                &self.find_state,
                0.0,
                content_y,
                self.window_width,
            ));
            FIND_PANEL_HEIGHT
        } else {
            0.0
        };
        content_y += find_panel_offset;

        // Content area dimensions.
        let status_y = self.window_height - STATUS_BAR_HEIGHT;
        let content_height = status_y - content_y;
        let mut content_x = 0.0;
        let mut available_width = self.window_width;

        // Table of contents sidebar.
        if self.toc_visible {
            cmds.extend(render_toc_sidebar(
                &self.cached_toc,
                0.0,
                content_y,
                content_height,
            ));
            content_x += TOC_SIDEBAR_WIDTH;
            available_width -= TOC_SIDEBAR_WIDTH;
        }

        // Main content: editor and/or preview.
        let doc = self.active_document();
        match self.view_mode {
            ViewMode::EditorOnly => {
                cmds.extend(render_editor(
                    doc,
                    content_x,
                    content_y,
                    available_width,
                    content_height,
                    &self.find_state,
                ));
            }
            ViewMode::Split => {
                let half_width = available_width / 2.0;
                cmds.extend(render_editor(
                    doc,
                    content_x,
                    content_y,
                    half_width,
                    content_height,
                    &self.find_state,
                ));
                // Split divider.
                cmds.push(RenderCommand::FillRect {
                    x: content_x + half_width - 1.0,
                    y: content_y,
                    width: 2.0,
                    height: content_height,
                    color: SURFACE0,
                    corner_radii: CornerRadii::ZERO,
                });
                cmds.extend(render_preview(
                    &self.cached_blocks,
                    content_x + half_width + 1.0,
                    content_y,
                    half_width - 1.0,
                    content_height,
                    doc.preview_scroll,
                ));
            }
            ViewMode::PreviewOnly => {
                cmds.extend(render_preview(
                    &self.cached_blocks,
                    content_x,
                    content_y,
                    available_width,
                    content_height,
                    doc.preview_scroll,
                ));
            }
        }

        // Status bar.
        cmds.extend(render_status_bar(
            doc,
            self.view_mode,
            0.0,
            status_y,
            self.window_width,
            self.autosave_enabled,
        ));

        // Template chooser overlay.
        if self.template_chooser_open {
            cmds.extend(render_template_chooser(
                0.0,
                0.0,
                self.window_width,
                self.window_height,
            ));
        }

        // External-change prompt / merge review (modal overlay).
        if let Some(prompt) = self.external_prompt.as_ref() {
            cmds.extend(self.render_external_prompt(prompt));
        }

        cmds
    }

    /// Render the external-change modal — either the four-option prompt or, when
    /// the user chose "review", the side-by-side merge review.
    fn render_external_prompt(&self, prompt: &ExternalChangePrompt) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let w = self.window_width;
        let h = self.window_height;

        // Dim the background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: Color::rgba(0x11, 0x11, 0x1B, 0xB0),
            corner_radii: CornerRadii::ZERO,
        });

        if let Some(review) = prompt.review.as_ref() {
            self.render_merge_review(&mut cmds, prompt, review);
            return cmds;
        }

        let dw = 480.0_f32.min(w - 40.0);
        let dh = 220.0_f32;
        let dx = (w - dw) / 2.0;
        let dy = (h - dh) / 2.0;
        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dw,
            height: dh,
            color: BASE,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dw,
            height: 32.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let name = self
            .documents
            .get(prompt.tab)
            .map_or("file", |d| d.name.as_str());
        let (title, body): (&str, String) = match &prompt.change {
            DiskChange::Deleted => (
                "File deleted on disk",
                format!(
                    "\"{name}\" was deleted outside the editor while you have unsaved changes."
                ),
            ),
            _ => (
                "File changed on disk",
                format!("\"{name}\" was modified outside the editor and you have unsaved changes."),
            ),
        };
        cmds.push(RenderCommand::Text {
            x: dx + 12.0,
            y: dy + 9.0,
            text: title.to_string(),
            font_size: 14.0,
            color: YELLOW,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dw - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: dx + 12.0,
            y: dy + 44.0,
            text: body,
            font_size: 12.0,
            color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dw - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        let deleted = matches!(prompt.change, DiskChange::Deleted);
        let mut options: Vec<(&str, &str)> = vec![
            ("Keep current", "keep your buffer; overwrites disk on save"),
            ("Reload from disk", "discard local edits, load disk version"),
        ];
        if !deleted {
            options.push(("Merge", "auto-combine both; mark conflicts inline"));
            options.push(("Review merge…", "resolve conflicts side-by-side"));
        }

        let mut by = dy + 74.0;
        for (label, hint) in options {
            cmds.push(RenderCommand::FillRect {
                x: dx + 12.0,
                y: by,
                width: dw - 24.0,
                height: 30.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: dx + 20.0,
                y: by + 7.0,
                text: label.to_string(),
                font_size: 12.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(140.0),
                overflow: TextOverflow::Ellipsis,
            });
            cmds.push(RenderCommand::Text {
                x: dx + 160.0,
                y: by + 8.0,
                text: hint.to_string(),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dw - 176.0),
                overflow: TextOverflow::Ellipsis,
            });
            by += 34.0;
        }
        cmds
    }

    /// Render the side-by-side merge review (ours | disk) with each conflict's
    /// current resolution highlighted. Mirrors orchestrator2's diff viewer.
    fn render_merge_review(
        &self,
        cmds: &mut Vec<RenderCommand>,
        prompt: &ExternalChangePrompt,
        review: &MergeReview,
    ) {
        let w = self.window_width;
        let h = self.window_height;
        let margin = 24.0;
        let dx = margin;
        let dy = margin;
        let dw = w - margin * 2.0;
        let dh = h - margin * 2.0;

        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dw,
            height: dh,
            color: BASE,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dw,
            height: 32.0,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let name = self
            .documents
            .get(prompt.tab)
            .map_or("file", |d| d.name.as_str());
        cmds.push(RenderCommand::Text {
            x: dx + 12.0,
            y: dy + 9.0,
            text: format!(
                "Review merge — {name}  ({} conflict(s))",
                review.conflict_count()
            ),
            font_size: 14.0,
            color: YELLOW,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dw - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        let col_w = (dw - 24.0) / 2.0;
        let ours_x = dx + 12.0;
        let theirs_x = dx + 12.0 + col_w;
        cmds.push(RenderCommand::Text {
            x: ours_x,
            y: dy + 40.0,
            text: name.to_string(),
            font_size: 11.0,
            color: GREEN,
            font_weight: FontWeightHint::Bold,
            max_width: Some(col_w),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: theirs_x,
            y: dy + 40.0,
            text: "disk".to_string(),
            font_size: 11.0,
            color: RED,
            font_weight: FontWeightHint::Bold,
            max_width: Some(col_w),
            overflow: TextOverflow::Ellipsis,
        });

        let mut y = dy + 60.0;
        for (i, (_base, ours, theirs)) in review.conflicts().iter().enumerate() {
            let choice = review.choice(i).unwrap_or(ConflictChoice::Theirs);
            let chosen_ours = matches!(choice, ConflictChoice::Ours | ConflictChoice::Both);
            let chosen_theirs = matches!(choice, ConflictChoice::Theirs | ConflictChoice::Both);

            let block_lines = ours.len().max(theirs.len()).max(1);
            let block_h = block_lines as f32 * LINE_HEIGHT + 6.0;

            if chosen_ours {
                cmds.push(RenderCommand::FillRect {
                    x: ours_x - 4.0,
                    y,
                    width: col_w,
                    height: block_h,
                    color: Color::from_hex(0x2A3A2A),
                    corner_radii: CornerRadii::ZERO,
                });
            }
            if chosen_theirs {
                cmds.push(RenderCommand::FillRect {
                    x: theirs_x - 4.0,
                    y,
                    width: col_w,
                    height: block_h,
                    color: Color::from_hex(0x3A2A2A),
                    corner_radii: CornerRadii::ZERO,
                });
            }
            cmds.push(RenderCommand::Text {
                x: dx + 2.0,
                y,
                text: format!("#{}", i + 1),
                font_size: 9.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(10.0),
                overflow: TextOverflow::Ellipsis,
            });
            for (li, line) in ours.iter().enumerate() {
                cmds.push(RenderCommand::Text {
                    x: ours_x,
                    y: y + li as f32 * LINE_HEIGHT,
                    text: line.clone(),
                    font_size: 11.0,
                    color: TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(col_w),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            for (li, line) in theirs.iter().enumerate() {
                cmds.push(RenderCommand::Text {
                    x: theirs_x,
                    y: y + li as f32 * LINE_HEIGHT,
                    text: line.clone(),
                    font_size: 11.0,
                    color: TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(col_w),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            y += block_h + 6.0;
        }

        cmds.push(RenderCommand::Text {
            x: dx + 12.0,
            y: dy + dh - 24.0,
            text: "[Accept]  [Cancel]   per-conflict: take ours / take disk / keep both"
                .to_string(),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dw - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

// ============================================================================
// Keyboard shortcut handling
// ============================================================================

/// Keyboard modifier flags.
#[derive(Clone, Copy, Debug, Default)]
pub struct Modifiers {
    /// Whether Ctrl is held.
    pub ctrl: bool,
    /// Whether Shift is held.
    pub shift: bool,
    /// Whether Alt is held.
    pub alt: bool,
}

/// Key identifiers for keyboard shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A regular character key.
    Char(char),
    /// Enter/Return.
    Enter,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Home key.
    Home,
    /// End key.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Tab key.
    Tab,
    /// Escape key.
    Escape,
    /// F1 through F12.
    Function(u8),
}

/// Handle a key press event in the application.
pub fn handle_key(app: &mut App, key: Key, modifiers: Modifiers) {
    // Global shortcuts.
    if modifiers.ctrl {
        match key {
            Key::Char('n') | Key::Char('N') => {
                app.new_document();
                return;
            }
            Key::Char('s') | Key::Char('S') => {
                let _ = app.active_document_mut().save();
                return;
            }
            Key::Char('z') | Key::Char('Z') => {
                if modifiers.shift {
                    app.active_document_mut().redo();
                } else {
                    app.active_document_mut().undo();
                }
                app.refresh_cache();
                return;
            }
            Key::Char('y') | Key::Char('Y') => {
                app.active_document_mut().redo();
                app.refresh_cache();
                return;
            }
            Key::Char('b') | Key::Char('B') => {
                insert_bold(app.active_document_mut());
                app.refresh_cache();
                return;
            }
            Key::Char('i') | Key::Char('I') => {
                insert_italic(app.active_document_mut());
                app.refresh_cache();
                return;
            }
            Key::Char('k') | Key::Char('K') => {
                if modifiers.shift {
                    insert_code_block(app.active_document_mut());
                } else {
                    insert_link(app.active_document_mut());
                }
                app.refresh_cache();
                return;
            }
            Key::Char('h') | Key::Char('H') => {
                app.find_state.visible = !app.find_state.visible;
                return;
            }
            Key::Char('f') | Key::Char('F') => {
                app.find_state.visible = true;
                return;
            }
            _ => {}
        }
    }

    // Escape closes dialogs/panels.
    if key == Key::Escape {
        if app.template_chooser_open {
            app.template_chooser_open = false;
            return;
        }
        if app.find_state.visible {
            app.find_state.visible = false;
            return;
        }
    }

    // Navigation keys.
    match key {
        Key::Up => {
            app.active_document_mut().move_cursor_up();
            let visible = compute_visible_lines(app);
            app.active_document_mut().ensure_cursor_visible(visible);
        }
        Key::Down => {
            app.active_document_mut().move_cursor_down();
            let visible = compute_visible_lines(app);
            app.active_document_mut().ensure_cursor_visible(visible);
        }
        Key::Left => {
            app.active_document_mut().move_cursor_left();
        }
        Key::Right => {
            app.active_document_mut().move_cursor_right();
        }
        Key::Home => {
            app.active_document_mut().move_cursor_home();
        }
        Key::End => {
            app.active_document_mut().move_cursor_end();
        }
        Key::PageUp => {
            let visible = compute_visible_lines(app);
            let doc = app.active_document_mut();
            for _ in 0..visible {
                doc.move_cursor_up();
            }
            doc.ensure_cursor_visible(visible);
        }
        Key::PageDown => {
            let visible = compute_visible_lines(app);
            let doc = app.active_document_mut();
            for _ in 0..visible {
                doc.move_cursor_down();
            }
            doc.ensure_cursor_visible(visible);
        }
        Key::Enter => {
            app.active_document_mut().insert_newline();
            app.refresh_cache();
        }
        Key::Backspace => {
            app.active_document_mut().delete_backward();
            app.refresh_cache();
        }
        Key::Delete => {
            app.active_document_mut().delete_forward();
            app.refresh_cache();
        }
        Key::Tab => {
            app.active_document_mut().insert_text("    ");
            app.refresh_cache();
        }
        Key::Char(ch) if !modifiers.ctrl && !modifiers.alt => {
            app.active_document_mut().insert_char(ch);
            app.refresh_cache();
        }
        _ => {}
    }

    // Sync scroll after any edit.
    app.sync_scroll();
}

/// Compute the number of visible lines in the editor area.
fn compute_visible_lines(app: &App) -> usize {
    let content_height = app.window_height
        - TOOLBAR_HEIGHT
        - TAB_BAR_HEIGHT
        - STATUS_BAR_HEIGHT
        - if app.find_state.visible {
            FIND_PANEL_HEIGHT
        } else {
            0.0
        };
    (content_height / LINE_HEIGHT) as usize
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let _app = App::new(1280.0, 800.0);
    // In a real application, this would enter the event loop
    // provided by the OS compositor/window system. The render()
    // method produces RenderCommands for the compositor to draw.
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    // --- Document tests ---

    #[test]
    fn test_document_new() {
        let doc = Document::new();
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0], "");
        assert!(!doc.modified);
        assert!(doc.path.is_none());
        assert_eq!(doc.name, "Untitled");
    }

    #[test]
    fn test_document_from_template_blank() {
        let doc = Document::from_template(Template::Blank);
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0], "");
    }

    #[test]
    fn test_document_from_template_meeting() {
        let doc = Document::from_template(Template::MeetingNotes);
        assert!(doc.lines.len() > 5);
        assert!(doc.lines[0].contains("Meeting Notes"));
    }

    #[test]
    fn test_document_from_template_readme() {
        let doc = Document::from_template(Template::ProjectReadme);
        assert!(doc.lines.len() > 5);
        assert!(doc.lines[0].contains("Project Name"));
    }

    #[test]
    fn test_document_from_template_blog() {
        let doc = Document::from_template(Template::BlogPost);
        assert!(doc.lines.len() > 3);
        assert!(doc.lines[0].contains("Blog Post"));
    }

    #[test]
    fn test_document_from_template_changelog() {
        let doc = Document::from_template(Template::Changelog);
        assert!(doc.lines.len() > 3);
        assert!(doc.lines[0].contains("Changelog"));
    }

    #[test]
    fn test_insert_char() {
        let mut doc = Document::new();
        doc.insert_char('H');
        doc.insert_char('i');
        assert_eq!(doc.lines[0], "Hi");
        assert_eq!(doc.cursor_col, 2);
    }

    #[test]
    fn test_insert_newline() {
        let mut doc = Document::new();
        doc.insert_char('A');
        doc.insert_newline();
        doc.insert_char('B');
        assert_eq!(doc.lines.len(), 2);
        assert_eq!(doc.lines[0], "A");
        assert_eq!(doc.lines[1], "B");
    }

    #[test]
    fn test_delete_backward_char() {
        let mut doc = Document::new();
        doc.insert_char('A');
        doc.insert_char('B');
        doc.delete_backward();
        assert_eq!(doc.lines[0], "A");
        assert_eq!(doc.cursor_col, 1);
    }

    #[test]
    fn test_delete_backward_merge_lines() {
        let mut doc = Document::new();
        doc.insert_char('A');
        doc.insert_newline();
        doc.insert_char('B');
        doc.cursor_col = 0;
        doc.delete_backward();
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0], "AB");
    }

    #[test]
    fn test_delete_forward_char() {
        let mut doc = Document::new();
        doc.insert_char('A');
        doc.insert_char('B');
        doc.cursor_col = 0;
        doc.delete_forward();
        assert_eq!(doc.lines[0], "B");
    }

    #[test]
    fn test_delete_forward_merge_lines() {
        let mut doc = Document::new();
        doc.insert_char('A');
        doc.insert_newline();
        doc.insert_char('B');
        doc.cursor_line = 0;
        doc.cursor_col = 1;
        doc.delete_forward();
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0], "AB");
    }

    #[test]
    fn test_undo_insert() {
        let mut doc = Document::new();
        doc.insert_char('A');
        doc.insert_char('B');
        doc.undo();
        assert_eq!(doc.lines[0], "A");
    }

    #[test]
    fn test_redo_insert() {
        let mut doc = Document::new();
        doc.insert_char('A');
        doc.insert_char('B');
        doc.undo();
        doc.redo();
        assert_eq!(doc.lines[0], "AB");
    }

    #[test]
    fn test_undo_delete() {
        let mut doc = Document::new();
        doc.insert_char('A');
        doc.insert_char('B');
        doc.delete_backward();
        doc.undo();
        assert_eq!(doc.lines[0], "AB");
    }

    #[test]
    fn test_multiple_undo() {
        let mut doc = Document::new();
        doc.insert_char('A');
        doc.insert_char('B');
        doc.insert_char('C');
        doc.undo();
        doc.undo();
        assert_eq!(doc.lines[0], "A");
    }

    #[test]
    fn test_cursor_movement_up_down() {
        let mut doc = Document::new();
        doc.insert_char('A');
        doc.insert_newline();
        doc.insert_char('B');
        doc.insert_newline();
        doc.insert_char('C');
        assert_eq!(doc.cursor_line, 2);
        doc.move_cursor_up();
        assert_eq!(doc.cursor_line, 1);
        doc.move_cursor_down();
        assert_eq!(doc.cursor_line, 2);
    }

    #[test]
    fn test_cursor_movement_left_right() {
        let mut doc = Document::new();
        doc.insert_text("ABC");
        doc.cursor_col = 1;
        doc.move_cursor_right();
        assert_eq!(doc.cursor_col, 2);
        doc.move_cursor_left();
        assert_eq!(doc.cursor_col, 1);
    }

    #[test]
    fn test_cursor_home_end() {
        let mut doc = Document::new();
        doc.insert_text("Hello World");
        doc.move_cursor_home();
        assert_eq!(doc.cursor_col, 0);
        doc.move_cursor_end();
        assert_eq!(doc.cursor_col, 11);
    }

    #[test]
    fn test_cursor_left_wraps_to_previous_line() {
        let mut doc = Document::new();
        doc.insert_text("AB");
        doc.insert_newline();
        doc.insert_text("CD");
        doc.cursor_line = 1;
        doc.cursor_col = 0;
        doc.move_cursor_left();
        assert_eq!(doc.cursor_line, 0);
        assert_eq!(doc.cursor_col, 2);
    }

    #[test]
    fn test_cursor_right_wraps_to_next_line() {
        let mut doc = Document::new();
        doc.insert_text("AB");
        doc.insert_newline();
        doc.insert_text("CD");
        doc.cursor_line = 0;
        doc.cursor_col = 2;
        doc.move_cursor_right();
        assert_eq!(doc.cursor_line, 1);
        assert_eq!(doc.cursor_col, 0);
    }

    #[test]
    fn test_word_count_empty() {
        let doc = Document::new();
        assert_eq!(doc.word_count(), 0);
    }

    #[test]
    fn test_word_count_simple() {
        let mut doc = Document::new();
        doc.lines = vec!["Hello World".to_string()];
        assert_eq!(doc.word_count(), 2);
    }

    #[test]
    fn test_word_count_multiline() {
        let mut doc = Document::new();
        doc.lines = vec!["Hello World".to_string(), "foo bar baz".to_string()];
        assert_eq!(doc.word_count(), 5);
    }

    #[test]
    fn test_char_count_empty() {
        let doc = Document::new();
        assert_eq!(doc.char_count(), 0);
    }

    #[test]
    fn test_char_count_simple() {
        let mut doc = Document::new();
        doc.lines = vec!["Hello".to_string()];
        assert_eq!(doc.char_count(), 5);
    }

    #[test]
    fn test_char_count_multiline() {
        let mut doc = Document::new();
        doc.lines = vec!["AB".to_string(), "CD".to_string()];
        // 2 + 2 + 1 newline = 5
        assert_eq!(doc.char_count(), 5);
    }

    #[test]
    fn test_reading_time() {
        let mut doc = Document::new();
        doc.lines = vec!["word ".repeat(238).trim().to_string()];
        let time = doc.reading_time_minutes();
        assert!((time - 1.0).abs() < 0.1);
    }

    #[test]
    fn test_go_to_line() {
        let mut doc = Document::new();
        doc.lines = vec![
            "Line 1".to_string(),
            "Line 2".to_string(),
            "Line 3".to_string(),
        ];
        doc.go_to_line(2);
        assert_eq!(doc.cursor_line, 2);
    }

    #[test]
    fn test_go_to_line_clamped() {
        let mut doc = Document::new();
        doc.lines = vec!["Line 1".to_string()];
        doc.go_to_line(100);
        assert_eq!(doc.cursor_line, 0);
    }

    #[test]
    fn test_full_text() {
        let mut doc = Document::new();
        doc.lines = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        assert_eq!(doc.full_text(), "A\nB\nC");
    }

    #[test]
    fn test_ensure_cursor_visible_scrolls_down() {
        let mut doc = Document::new();
        for i in 0..50 {
            doc.lines.push(format!("Line {}", i));
        }
        doc.cursor_line = 45;
        doc.scroll_line = 0;
        doc.ensure_cursor_visible(20);
        assert!(doc.scroll_line > 0);
    }

    #[test]
    fn test_ensure_cursor_visible_scrolls_up() {
        let mut doc = Document::new();
        for i in 0..50 {
            doc.lines.push(format!("Line {}", i));
        }
        doc.cursor_line = 5;
        doc.scroll_line = 20;
        doc.ensure_cursor_visible(20);
        assert_eq!(doc.scroll_line, 5);
    }

    // --- Markdown parser tests ---

    #[test]
    fn test_parse_heading_h1() {
        let blocks = parse_markdown("# Hello");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::Heading { level, inlines } => {
                assert_eq!(*level, 1);
                assert_eq!(inlines_to_plain_text(inlines), "Hello");
            }
            _ => panic!("Expected heading"),
        }
    }

    #[test]
    fn test_parse_heading_h3() {
        let blocks = parse_markdown("### Third Level");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::Heading { level, inlines } => {
                assert_eq!(*level, 3);
                assert_eq!(inlines_to_plain_text(inlines), "Third Level");
            }
            _ => panic!("Expected heading"),
        }
    }

    #[test]
    fn test_parse_heading_h6() {
        let blocks = parse_markdown("###### Smallest");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::Heading { level, .. } => {
                assert_eq!(*level, 6);
            }
            _ => panic!("Expected heading"),
        }
    }

    #[test]
    fn test_parse_paragraph() {
        let blocks = parse_markdown("Hello world");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::Paragraph { inlines } => {
                assert_eq!(inlines_to_plain_text(inlines), "Hello world");
            }
            _ => panic!("Expected paragraph"),
        }
    }

    #[test]
    fn test_parse_bold() {
        let inlines = parse_inlines("**bold text**");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            MdInline::Bold(inner) => {
                assert_eq!(inlines_to_plain_text(inner), "bold text");
            }
            _ => panic!("Expected bold"),
        }
    }

    #[test]
    fn test_parse_italic() {
        let inlines = parse_inlines("*italic text*");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            MdInline::Italic(inner) => {
                assert_eq!(inlines_to_plain_text(inner), "italic text");
            }
            _ => panic!("Expected italic"),
        }
    }

    #[test]
    fn test_parse_strikethrough() {
        let inlines = parse_inlines("~~deleted~~");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            MdInline::Strikethrough(inner) => {
                assert_eq!(inlines_to_plain_text(inner), "deleted");
            }
            _ => panic!("Expected strikethrough"),
        }
    }

    #[test]
    fn test_parse_inline_code() {
        let inlines = parse_inlines("`code`");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            MdInline::InlineCode(c) => {
                assert_eq!(c, "code");
            }
            _ => panic!("Expected inline code"),
        }
    }

    #[test]
    fn test_parse_link() {
        let inlines = parse_inlines("[click here](https://example.com)");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            MdInline::Link { text, url } => {
                assert_eq!(inlines_to_plain_text(text), "click here");
                assert_eq!(url, "https://example.com");
            }
            _ => panic!("Expected link"),
        }
    }

    #[test]
    fn test_parse_image() {
        let inlines = parse_inlines("![alt text](image.png)");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            MdInline::Image { alt, url } => {
                assert_eq!(alt, "alt text");
                assert_eq!(url, "image.png");
            }
            _ => panic!("Expected image"),
        }
    }

    #[test]
    fn test_parse_code_block() {
        let input = "```rust\nfn main() {}\n```";
        let blocks = parse_markdown(input);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::CodeBlock { language, code } => {
                assert_eq!(language, "rust");
                assert_eq!(code, "fn main() {}");
            }
            _ => panic!("Expected code block"),
        }
    }

    #[test]
    fn test_parse_code_block_no_language() {
        let input = "```\nhello\n```";
        let blocks = parse_markdown(input);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::CodeBlock { language, code } => {
                assert_eq!(language, "");
                assert_eq!(code, "hello");
            }
            _ => panic!("Expected code block"),
        }
    }

    #[test]
    fn test_parse_blockquote() {
        let input = "> This is a quote";
        let blocks = parse_markdown(input);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::BlockQuote { children } => {
                assert!(!children.is_empty());
            }
            _ => panic!("Expected blockquote"),
        }
    }

    #[test]
    fn test_parse_horizontal_rule_dashes() {
        let blocks = parse_markdown("---");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], MdBlock::HorizontalRule);
    }

    #[test]
    fn test_parse_horizontal_rule_asterisks() {
        let blocks = parse_markdown("***");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], MdBlock::HorizontalRule);
    }

    #[test]
    fn test_parse_horizontal_rule_underscores() {
        let blocks = parse_markdown("___");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], MdBlock::HorizontalRule);
    }

    #[test]
    fn test_parse_unordered_list() {
        let input = "- Item 1\n- Item 2\n- Item 3";
        let blocks = parse_markdown(input);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::UnorderedList { items } => {
                assert_eq!(items.len(), 3);
                assert_eq!(inlines_to_plain_text(&items[0].inlines), "Item 1");
                assert_eq!(inlines_to_plain_text(&items[1].inlines), "Item 2");
            }
            _ => panic!("Expected unordered list"),
        }
    }

    #[test]
    fn test_parse_ordered_list() {
        let input = "1. First\n2. Second\n3. Third";
        let blocks = parse_markdown(input);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::OrderedList { start, items } => {
                assert_eq!(*start, 1);
                assert_eq!(items.len(), 3);
            }
            _ => panic!("Expected ordered list"),
        }
    }

    #[test]
    fn test_parse_task_list_checked() {
        let input = "- [x] Done task";
        let blocks = parse_markdown(input);
        match &blocks[0] {
            MdBlock::UnorderedList { items } => {
                assert_eq!(items[0].task, Some(true));
            }
            _ => panic!("Expected unordered list"),
        }
    }

    #[test]
    fn test_parse_task_list_unchecked() {
        let input = "- [ ] Not done";
        let blocks = parse_markdown(input);
        match &blocks[0] {
            MdBlock::UnorderedList { items } => {
                assert_eq!(items[0].task, Some(false));
            }
            _ => panic!("Expected unordered list"),
        }
    }

    #[test]
    fn test_parse_table() {
        let input = "| A | B |\n|---|---|\n| 1 | 2 |";
        let blocks = parse_markdown(input);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::Table {
                headers,
                rows,
                alignments,
            } => {
                assert_eq!(headers.len(), 2);
                assert_eq!(rows.len(), 1);
                assert_eq!(alignments.len(), 2);
            }
            _ => panic!("Expected table"),
        }
    }

    #[test]
    fn test_parse_table_alignments() {
        let input = "| L | C | R |\n|:---|:---:|---:|\n| a | b | c |";
        let blocks = parse_markdown(input);
        match &blocks[0] {
            MdBlock::Table { alignments, .. } => {
                assert_eq!(alignments[0], TableAlign::Left);
                assert_eq!(alignments[1], TableAlign::Center);
                assert_eq!(alignments[2], TableAlign::Right);
            }
            _ => panic!("Expected table"),
        }
    }

    #[test]
    fn test_parse_mixed_content() {
        let input = "# Title\n\nSome text.\n\n- Item 1\n- Item 2\n\n---\n\n> Quote";
        let blocks = parse_markdown(input);
        assert!(blocks.len() >= 4);
    }

    #[test]
    fn test_parse_bold_underscore() {
        let inlines = parse_inlines("__bold__");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            MdInline::Bold(inner) => {
                assert_eq!(inlines_to_plain_text(inner), "bold");
            }
            _ => panic!("Expected bold"),
        }
    }

    #[test]
    fn test_parse_italic_underscore() {
        let inlines = parse_inlines("_italic_");
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
            MdInline::Italic(inner) => {
                assert_eq!(inlines_to_plain_text(inner), "italic");
            }
            _ => panic!("Expected italic"),
        }
    }

    #[test]
    fn test_parse_mixed_inline() {
        let inlines = parse_inlines("Hello **bold** and *italic*");
        assert!(inlines.len() >= 3);
    }

    #[test]
    fn test_parse_empty_input() {
        let blocks = parse_markdown("");
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_parse_only_blank_lines() {
        let blocks = parse_markdown("\n\n\n");
        assert!(blocks.is_empty());
    }

    // --- TOC tests ---

    #[test]
    fn test_extract_toc_simple() {
        let source = "# Title\n\n## Section 1\n\n### Subsection\n\n## Section 2";
        let toc = extract_toc(source);
        assert_eq!(toc.len(), 4);
        assert_eq!(toc[0].level, 1);
        assert_eq!(toc[0].text, "Title");
        assert_eq!(toc[1].level, 2);
        assert_eq!(toc[1].text, "Section 1");
    }

    #[test]
    fn test_extract_toc_preserves_line_numbers() {
        let source = "# Title\n\nSome text\n\n## Section";
        let toc = extract_toc(source);
        assert_eq!(toc[0].line, 0);
        assert_eq!(toc[1].line, 4);
    }

    #[test]
    fn test_extract_toc_empty() {
        let toc = extract_toc("No headings here");
        assert!(toc.is_empty());
    }

    // --- Find/replace tests ---

    #[test]
    fn test_find_simple() {
        let mut state = FindReplaceState::new();
        state.query = "hello".to_string();
        let lines = vec!["hello world".to_string(), "say hello".to_string()];
        state.find_all(&lines);
        assert_eq!(state.matches.len(), 2);
    }

    #[test]
    fn test_find_case_insensitive() {
        let mut state = FindReplaceState::new();
        state.query = "Hello".to_string();
        state.case_sensitive = false;
        let lines = vec!["hello HELLO Hello".to_string()];
        state.find_all(&lines);
        assert_eq!(state.matches.len(), 3);
    }

    #[test]
    fn test_find_case_sensitive() {
        let mut state = FindReplaceState::new();
        state.query = "Hello".to_string();
        state.case_sensitive = true;
        let lines = vec!["hello HELLO Hello".to_string()];
        state.find_all(&lines);
        assert_eq!(state.matches.len(), 1);
    }

    #[test]
    fn test_find_no_matches() {
        let mut state = FindReplaceState::new();
        state.query = "xyz".to_string();
        let lines = vec!["hello world".to_string()];
        state.find_all(&lines);
        assert!(state.matches.is_empty());
    }

    #[test]
    fn test_find_next_match() {
        let mut state = FindReplaceState::new();
        state.query = "a".to_string();
        let lines = vec!["a b a c a".to_string()];
        state.find_all(&lines);
        assert_eq!(state.current_match, 0);
        state.next_match();
        assert_eq!(state.current_match, 1);
        state.next_match();
        assert_eq!(state.current_match, 2);
        state.next_match();
        assert_eq!(state.current_match, 0); // wraps around
    }

    #[test]
    fn test_find_prev_match() {
        let mut state = FindReplaceState::new();
        state.query = "a".to_string();
        let lines = vec!["a b a".to_string()];
        state.find_all(&lines);
        state.prev_match();
        assert_eq!(state.current_match, 1); // wraps around from 0
    }

    #[test]
    fn test_replace_current() {
        let mut state = FindReplaceState::new();
        state.query = "foo".to_string();
        state.replacement = "bar".to_string();
        let mut lines = vec!["foo baz foo".to_string()];
        state.find_all(&lines);
        let replaced = state.replace_current(&mut lines);
        assert!(replaced);
        assert!(lines[0].contains("bar"));
    }

    #[test]
    fn test_replace_all() {
        let mut state = FindReplaceState::new();
        state.query = "old".to_string();
        state.replacement = "new".to_string();
        let mut lines = vec!["old and old".to_string(), "old again".to_string()];
        state.find_all(&lines);
        let count = state.replace_all(&mut lines);
        assert_eq!(count, 3);
        assert!(!lines[0].contains("old"));
        assert!(!lines[1].contains("old"));
    }

    #[test]
    fn test_find_empty_query() {
        let mut state = FindReplaceState::new();
        state.query = "".to_string();
        let lines = vec!["some text".to_string()];
        state.find_all(&lines);
        assert!(state.matches.is_empty());
    }

    /// A match range is an offset into the line the user is editing, not into
    /// a lowercased copy of it.
    ///
    /// Find used to search `line.to_lowercase()` and hand the offsets it got
    /// there straight to the highlighter and to `replace_range`. Folding is
    /// not length-preserving: Turkish `İ` (U+0130) is two bytes and folds to
    /// three, so every offset after one is shifted. Here the real match
    /// `abc` starts at byte 2; in the folded copy it starts at byte 3.
    #[test]
    fn a_match_offset_is_an_offset_into_the_line_the_user_is_editing() {
        let mut state = FindReplaceState::new();
        state.query = "ABC".to_string();
        state.case_sensitive = false;
        let lines = vec!["\u{130}abc".to_string()];
        state.find_all(&lines);
        assert_eq!(state.matches, vec![(0, 2, 5)]);
        // The offsets slice the line the user can see.
        assert_eq!(&lines[0][2..5], "abc");
    }

    /// Matches do not overlap: `aa` occurs twice in `aaaa`, not three times.
    /// The old scan resumed one byte past the *start* of the match it had
    /// just found, so replace-all could rewrite text it had already
    /// rewritten.
    #[test]
    fn matches_do_not_overlap_one_another() {
        let mut state = FindReplaceState::new();
        state.query = "aa".to_string();
        let lines = vec!["aaaa".to_string()];
        state.find_all(&lines);
        assert_eq!(state.matches, vec![(0, 0, 2), (0, 2, 4)]);

        let mut lines = lines;
        assert_eq!(state.replace_all(&mut lines), 2);
        assert_eq!(lines[0], "");
    }

    /// A case-insensitive match may be longer or shorter than the needle, so
    /// the end offset has to come from the search rather than from
    /// `start + query.len()`.
    #[test]
    fn a_match_is_not_assumed_to_be_as_long_as_the_query() {
        let mut state = FindReplaceState::new();
        state.query = "i\u{307}".to_string(); // `i` + combining dot: 3 bytes.
        state.case_sensitive = false;
        let lines = vec!["x\u{130}y".to_string()]; // `İ`: 2 bytes, folds to 3.
        state.find_all(&lines);
        assert_eq!(state.matches, vec![(0, 1, 3)]);

        state.replacement = "I".to_string();
        let mut lines = lines;
        assert_eq!(state.replace_all(&mut lines), 1);
        assert_eq!(lines[0], "xIy");
    }

    /// Replace refuses a range recorded against a document that has since
    /// changed, rather than slicing a line at a byte that is now inside a
    /// character.
    ///
    /// The window stays open while the document is editable, so the user can
    /// find, then type or switch tabs, then press Replace. `replace_range`
    /// panics on a non-boundary or out-of-range index — which took the whole
    /// editor down, losing every unsaved tab.
    #[test]
    fn a_stale_replace_range_is_refused_rather_than_panicking() {
        let mut state = FindReplaceState::new();
        state.query = "needle".to_string();
        state.replacement = "x".to_string();
        let mut lines = vec!["a needle here".to_string()];
        state.find_all(&lines);
        assert_eq!(state.matches.len(), 1);

        // The user edits: the line is now shorter than the recorded range,
        // and byte 8 is inside a character.
        lines[0] = "\u{65e5}\u{672c}".to_string();
        assert!(!state.replace_current(&mut lines));
        assert_eq!(lines[0], "\u{65e5}\u{672c}");

        // A line that no longer exists at all is refused the same way.
        let mut empty: Vec<String> = Vec::new();
        state.find_all(&["a needle here".to_string()]);
        assert!(!state.replace_current(&mut empty));
    }

    // --- HTML export tests ---

    #[test]
    fn test_export_html_basic() {
        let blocks = parse_markdown("# Hello\n\nWorld");
        let html = export_html(&blocks);
        assert!(html.contains("<h1>"));
        assert!(html.contains("Hello"));
        assert!(html.contains("<p>"));
        assert!(html.contains("World"));
    }

    #[test]
    fn test_export_html_code_block() {
        let blocks = parse_markdown("```rust\nfn main() {}\n```");
        let html = export_html(&blocks);
        assert!(html.contains("<pre>"));
        assert!(html.contains("<code"));
        assert!(html.contains("language-rust"));
    }

    #[test]
    fn test_export_html_bold_italic() {
        let blocks = parse_markdown("**bold** and *italic*");
        let html = export_html(&blocks);
        assert!(html.contains("<strong>"));
        assert!(html.contains("<em>"));
    }

    #[test]
    fn test_export_html_link() {
        let blocks = parse_markdown("[Example](https://example.com)");
        let html = export_html(&blocks);
        assert!(html.contains("<a href="));
        assert!(html.contains("example.com"));
    }

    #[test]
    fn test_export_html_image() {
        let blocks = parse_markdown("![Alt](image.png)");
        let html = export_html(&blocks);
        assert!(html.contains("<img"));
        assert!(html.contains("image.png"));
    }

    #[test]
    fn test_export_html_list() {
        let blocks = parse_markdown("- A\n- B");
        let html = export_html(&blocks);
        assert!(html.contains("<ul>"));
        assert!(html.contains("<li>"));
    }

    #[test]
    fn test_export_html_ordered_list() {
        let blocks = parse_markdown("1. First\n2. Second");
        let html = export_html(&blocks);
        assert!(html.contains("<ol>"));
    }

    #[test]
    fn test_export_html_horizontal_rule() {
        let blocks = parse_markdown("---");
        let html = export_html(&blocks);
        assert!(html.contains("<hr>"));
    }

    #[test]
    fn test_export_html_blockquote() {
        let blocks = parse_markdown("> Quoted text");
        let html = export_html(&blocks);
        assert!(html.contains("<blockquote>"));
    }

    #[test]
    fn test_export_html_table() {
        let blocks = parse_markdown("| A | B |\n|---|---|\n| 1 | 2 |");
        let html = export_html(&blocks);
        assert!(html.contains("<table>"));
        assert!(html.contains("<th>"));
        assert!(html.contains("<td>"));
    }

    #[test]
    fn test_export_html_task_list() {
        let blocks = parse_markdown("- [x] Done\n- [ ] Todo");
        let html = export_html(&blocks);
        assert!(html.contains("checkbox"));
        assert!(html.contains("checked"));
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("a & b"), "a &amp; b");
        assert_eq!(escape_html("\"quoted\""), "&quot;quoted&quot;");
    }

    #[test]
    fn test_export_html_strikethrough() {
        let blocks = parse_markdown("~~deleted~~");
        let html = export_html(&blocks);
        assert!(html.contains("<del>"));
    }

    #[test]
    fn test_export_html_inline_code() {
        let blocks = parse_markdown("Use `code` here");
        let html = export_html(&blocks);
        assert!(html.contains("<code>code</code>"));
    }

    // --- Syntax highlighting tests ---

    #[test]
    fn test_highlight_heading() {
        let spans = highlight_line("# Title");
        assert!(!spans.is_empty());
        // Should have hash mark span and text span.
        assert!(spans.len() >= 2);
    }

    #[test]
    fn test_highlight_code_fence() {
        let spans = highlight_line("```rust");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].color, GREEN);
    }

    #[test]
    fn test_highlight_blockquote() {
        let spans = highlight_line("> Quote text");
        assert!(spans.len() >= 2);
        assert_eq!(spans[0].color, BLUE);
    }

    #[test]
    fn test_highlight_hr() {
        let spans = highlight_line("---");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].color, SURFACE2);
    }

    #[test]
    fn test_highlight_list_item() {
        let spans = highlight_line("- Item text");
        assert!(!spans.is_empty());
        assert_eq!(spans[0].color, BLUE);
    }

    #[test]
    fn test_highlight_inline_code() {
        let spans = highlight_line("Use `code` here");
        let code_span = spans.iter().find(|s| s.color == GREEN);
        assert!(code_span.is_some());
    }

    #[test]
    fn test_highlight_bold_markers() {
        let spans = highlight_line("**bold**");
        // Should have dimmed markers and bold text.
        assert!(spans.len() >= 3);
    }

    #[test]
    fn test_highlight_link() {
        let spans = highlight_line("[text](url)");
        let link_span = spans.iter().find(|s| s.color == BLUE);
        assert!(link_span.is_some());
    }

    // --- Insert helper tests ---

    #[test]
    fn test_insert_bold() {
        let mut doc = Document::new();
        insert_bold(&mut doc);
        assert!(doc.full_text().contains("**bold**"));
    }

    #[test]
    fn test_insert_italic() {
        let mut doc = Document::new();
        insert_italic(&mut doc);
        assert!(doc.full_text().contains("*italic*"));
    }

    #[test]
    fn test_insert_strikethrough() {
        let mut doc = Document::new();
        insert_strikethrough(&mut doc);
        assert!(doc.full_text().contains("~~strikethrough~~"));
    }

    #[test]
    fn test_insert_link() {
        let mut doc = Document::new();
        insert_link(&mut doc);
        assert!(doc.full_text().contains("[link text](url)"));
    }

    #[test]
    fn test_insert_image() {
        let mut doc = Document::new();
        insert_image(&mut doc);
        assert!(doc.full_text().contains("![alt text](image_url)"));
    }

    #[test]
    fn test_insert_inline_code() {
        let mut doc = Document::new();
        insert_inline_code(&mut doc);
        assert!(doc.full_text().contains("`code`"));
    }

    #[test]
    fn test_insert_code_block() {
        let mut doc = Document::new();
        insert_code_block(&mut doc);
        let text = doc.full_text();
        assert!(text.contains("```"));
    }

    #[test]
    fn test_insert_unordered_list() {
        let mut doc = Document::new();
        insert_unordered_list(&mut doc);
        assert!(doc.full_text().contains("- "));
    }

    #[test]
    fn test_insert_ordered_list() {
        let mut doc = Document::new();
        insert_ordered_list(&mut doc);
        assert!(doc.full_text().contains("1. "));
    }

    #[test]
    fn test_insert_task_list() {
        let mut doc = Document::new();
        insert_task_list(&mut doc);
        assert!(doc.full_text().contains("- [ ] "));
    }

    #[test]
    fn test_insert_table() {
        let mut doc = Document::new();
        insert_table(&mut doc);
        let text = doc.full_text();
        assert!(text.contains("|"));
        assert!(text.contains("Column 1"));
    }

    #[test]
    fn test_insert_horizontal_rule() {
        let mut doc = Document::new();
        insert_horizontal_rule(&mut doc);
        assert!(doc.full_text().contains("---"));
    }

    #[test]
    fn test_insert_heading_level_1() {
        let mut doc = Document::new();
        doc.lines = vec!["Title".to_string()];
        insert_heading(&mut doc, 1);
        assert!(doc.lines[0].starts_with("# "));
    }

    #[test]
    fn test_insert_heading_level_3() {
        let mut doc = Document::new();
        doc.lines = vec!["Section".to_string()];
        insert_heading(&mut doc, 3);
        assert!(doc.lines[0].starts_with("### "));
    }

    /// Undoing a heading removes the `#` prefix — it does not paste the old
    /// line in front of itself.
    ///
    /// `insert_heading` was the one edit that hand-rolled its own undo entry,
    /// and it recorded the wrong *kind*: an `EditAction::Delete` naming the
    /// whole previous line, whose undo is "put that text back at column 0".
    /// Since nothing had been deleted, undo appended rather than removed, so
    /// Ctrl+H then Ctrl+Z turned `Title` into `Title# Title`.
    #[test]
    fn undoing_a_heading_removes_the_prefix_rather_than_duplicating_the_line() {
        let mut doc = Document::new();
        doc.lines = vec!["Title".to_string()];
        doc.cursor_line = 0;

        insert_heading(&mut doc, 2);
        assert_eq!(doc.line_text(0), "## Title");

        doc.undo();
        assert_eq!(doc.line_text(0), "Title");

        // And the redo of that undo puts the heading back, once.
        doc.redo();
        assert_eq!(doc.line_text(0), "## Title");
    }

    /// A heading on a cursor line the document no longer has is a no-op, not
    /// a panic. The toolbar button reads `cursor_line`, which a reload or an
    /// external-change adopt can leave past the end.
    #[test]
    fn a_heading_on_a_line_that_is_gone_does_nothing() {
        let mut doc = Document::new();
        doc.lines = vec!["only".to_string()];
        doc.cursor_line = 40;
        insert_heading(&mut doc, 1);
        assert_eq!(doc.lines, vec!["only".to_string()]);
    }

    // --- Rendering tests ---

    #[test]
    fn test_render_editor_produces_commands() {
        let doc = Document::new();
        let find = FindReplaceState::new();
        let cmds = render_editor(&doc, 0.0, 0.0, 800.0, 600.0, &find);
        assert!(!cmds.is_empty());
    }

    /// Drawing a document whose `scroll_line` is past its last line draws no
    /// lines, rather than indexing off the end. A reload from disk or an
    /// adopted external change can shrink the document under a scroll
    /// position that was valid a frame ago.
    #[test]
    fn rendering_past_the_end_of_a_shrunken_document_draws_nothing() {
        let mut doc = Document::new();
        doc.lines = vec!["one".to_string()];
        doc.scroll_line = 500;
        doc.cursor_line = 0;
        let find = FindReplaceState::new();
        // The frame furniture (background, gutter) is still drawn; the point
        // is that it returns at all.
        let cmds = render_editor(&doc, 0.0, 0.0, 800.0, 600.0, &find);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_preview_produces_commands() {
        let blocks = parse_markdown("# Hello\n\nWorld");
        let cmds = render_preview(&blocks, 0.0, 0.0, 800.0, 600.0, 0.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_toolbar_produces_commands() {
        let buttons = default_toolbar_buttons();
        let cmds = render_toolbar(&buttons, 0.0, 0.0, 1200.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_tab_bar_produces_commands() {
        let docs = Tabs::with(Document::new());
        let cmds = render_tab_bar(&docs, 0.0, 0.0, 1200.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_status_bar_produces_commands() {
        let doc = Document::new();
        let cmds = render_status_bar(&doc, ViewMode::Split, 0.0, 0.0, 1200.0, true);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_toc_sidebar_produces_commands() {
        let entries = vec![TocEntry {
            level: 1,
            text: "Title".to_string(),
            line: 0,
        }];
        let cmds = render_toc_sidebar(&entries, 0.0, 0.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_find_replace_hidden() {
        let state = FindReplaceState::new();
        let cmds = render_find_replace(&state, 0.0, 0.0, 1200.0);
        assert!(cmds.is_empty()); // hidden by default
    }

    #[test]
    fn test_render_find_replace_visible() {
        let mut state = FindReplaceState::new();
        state.visible = true;
        let cmds = render_find_replace(&state, 0.0, 0.0, 1200.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_template_chooser() {
        let cmds = render_template_chooser(0.0, 0.0, 1200.0, 800.0);
        assert!(!cmds.is_empty());
    }

    // --- App tests ---

    #[test]
    fn test_app_new() {
        let app = App::new(1280.0, 800.0);
        assert_eq!(app.documents.count(), 1);
        assert_eq!(app.active_doc(), 0);
        assert_eq!(app.view_mode, ViewMode::Split);
    }

    #[test]
    fn test_app_new_document() {
        let mut app = App::new(1280.0, 800.0);
        app.new_document();
        assert_eq!(app.documents.count(), 2);
        assert_eq!(app.active_doc(), 1);
    }

    #[test]
    fn test_app_close_document() {
        let mut app = App::new(1280.0, 800.0);
        app.new_document();
        app.close_document(0);
        assert_eq!(app.documents.count(), 1);
    }

    #[test]
    fn test_app_close_last_document() {
        let mut app = App::new(1280.0, 800.0);
        app.close_document(0);
        assert_eq!(app.documents.count(), 1); // always keeps one
    }

    #[test]
    fn test_app_switch_tab() {
        let mut app = App::new(1280.0, 800.0);
        app.new_document();
        app.switch_tab(0);
        assert_eq!(app.active_doc(), 0);
    }

    #[test]
    fn test_app_view_mode_cycle() {
        assert_eq!(ViewMode::EditorOnly.next(), ViewMode::Split);
        assert_eq!(ViewMode::Split.next(), ViewMode::PreviewOnly);
        assert_eq!(ViewMode::PreviewOnly.next(), ViewMode::EditorOnly);
    }

    #[test]
    fn test_app_view_mode_labels() {
        assert_eq!(ViewMode::EditorOnly.label(), "Editor");
        assert_eq!(ViewMode::Split.label(), "Split");
        assert_eq!(ViewMode::PreviewOnly.label(), "Preview");
    }

    #[test]
    fn test_app_render_produces_commands() {
        let app = App::new(1280.0, 800.0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_handle_toolbar_bold() {
        let mut app = App::new(1280.0, 800.0);
        app.handle_toolbar_action(&ToolbarAction::Bold);
        assert!(app.active_document().full_text().contains("**bold**"));
    }

    #[test]
    fn test_app_handle_toolbar_toggle_view() {
        let mut app = App::new(1280.0, 800.0);
        assert_eq!(app.view_mode, ViewMode::Split);
        app.handle_toolbar_action(&ToolbarAction::ToggleView);
        assert_eq!(app.view_mode, ViewMode::PreviewOnly);
    }

    #[test]
    fn test_app_handle_toolbar_toggle_toc() {
        let mut app = App::new(1280.0, 800.0);
        assert!(!app.toc_visible);
        app.handle_toolbar_action(&ToolbarAction::ToggleToc);
        assert!(app.toc_visible);
    }

    #[test]
    fn test_app_new_from_template() {
        let mut app = App::new(1280.0, 800.0);
        app.new_from_template(Template::MeetingNotes);
        assert_eq!(app.documents.count(), 2);
        assert!(app.active_document().full_text().contains("Meeting Notes"));
    }

    #[test]
    fn test_app_refresh_cache() {
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut()
            .insert_text("# Heading\n\nParagraph");
        app.refresh_cache();
        assert!(!app.cached_blocks.is_empty());
        assert!(!app.cached_toc.is_empty());
    }

    // --- Keyboard shortcut tests ---

    #[test]
    fn test_handle_key_char() {
        let mut app = App::new(1280.0, 800.0);
        handle_key(&mut app, Key::Char('H'), Modifiers::default());
        handle_key(&mut app, Key::Char('i'), Modifiers::default());
        assert_eq!(app.active_document().lines[0], "Hi");
    }

    #[test]
    fn test_handle_key_enter() {
        let mut app = App::new(1280.0, 800.0);
        handle_key(&mut app, Key::Char('A'), Modifiers::default());
        handle_key(&mut app, Key::Enter, Modifiers::default());
        handle_key(&mut app, Key::Char('B'), Modifiers::default());
        assert_eq!(app.active_document().lines.len(), 2);
    }

    #[test]
    fn test_handle_key_backspace() {
        let mut app = App::new(1280.0, 800.0);
        handle_key(&mut app, Key::Char('A'), Modifiers::default());
        handle_key(&mut app, Key::Char('B'), Modifiers::default());
        handle_key(&mut app, Key::Backspace, Modifiers::default());
        assert_eq!(app.active_document().lines[0], "A");
    }

    #[test]
    fn test_handle_key_ctrl_b_bold() {
        let mut app = App::new(1280.0, 800.0);
        handle_key(
            &mut app,
            Key::Char('b'),
            Modifiers {
                ctrl: true,
                shift: false,
                alt: false,
            },
        );
        assert!(app.active_document().full_text().contains("**bold**"));
    }

    #[test]
    fn test_handle_key_ctrl_i_italic() {
        let mut app = App::new(1280.0, 800.0);
        handle_key(
            &mut app,
            Key::Char('i'),
            Modifiers {
                ctrl: true,
                shift: false,
                alt: false,
            },
        );
        assert!(app.active_document().full_text().contains("*italic*"));
    }

    #[test]
    fn test_handle_key_ctrl_k_link() {
        let mut app = App::new(1280.0, 800.0);
        handle_key(
            &mut app,
            Key::Char('k'),
            Modifiers {
                ctrl: true,
                shift: false,
                alt: false,
            },
        );
        assert!(
            app.active_document()
                .full_text()
                .contains("[link text](url)")
        );
    }

    #[test]
    fn test_handle_key_ctrl_shift_k_code_block() {
        let mut app = App::new(1280.0, 800.0);
        handle_key(
            &mut app,
            Key::Char('k'),
            Modifiers {
                ctrl: true,
                shift: true,
                alt: false,
            },
        );
        assert!(app.active_document().full_text().contains("```"));
    }

    #[test]
    fn test_handle_key_escape_closes_find() {
        let mut app = App::new(1280.0, 800.0);
        app.find_state.visible = true;
        handle_key(&mut app, Key::Escape, Modifiers::default());
        assert!(!app.find_state.visible);
    }

    #[test]
    fn test_handle_key_escape_closes_template() {
        let mut app = App::new(1280.0, 800.0);
        app.template_chooser_open = true;
        handle_key(&mut app, Key::Escape, Modifiers::default());
        assert!(!app.template_chooser_open);
    }

    #[test]
    fn test_handle_key_arrow_up() {
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().lines = vec!["A".to_string(), "B".to_string()];
        app.active_document_mut().cursor_line = 1;
        handle_key(&mut app, Key::Up, Modifiers::default());
        assert_eq!(app.active_document().cursor_line, 0);
    }

    #[test]
    fn test_handle_key_arrow_down() {
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().lines = vec!["A".to_string(), "B".to_string()];
        app.active_document_mut().cursor_line = 0;
        handle_key(&mut app, Key::Down, Modifiers::default());
        assert_eq!(app.active_document().cursor_line, 1);
    }

    #[test]
    fn test_handle_key_home_end() {
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().lines = vec!["Hello".to_string()];
        app.active_document_mut().cursor_col = 2;
        handle_key(&mut app, Key::Home, Modifiers::default());
        assert_eq!(app.active_document().cursor_col, 0);
        handle_key(&mut app, Key::End, Modifiers::default());
        assert_eq!(app.active_document().cursor_col, 5);
    }

    // --- Template tests ---

    #[test]
    fn test_template_all() {
        let all = Template::all();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn test_template_labels() {
        assert_eq!(Template::Blank.label(), "Blank");
        assert_eq!(Template::MeetingNotes.label(), "Meeting Notes");
        assert_eq!(Template::ProjectReadme.label(), "Project README");
        assert_eq!(Template::BlogPost.label(), "Blog Post");
        assert_eq!(Template::Changelog.label(), "Changelog");
    }

    #[test]
    fn test_template_blank_content() {
        assert_eq!(Template::Blank.content(), "");
    }

    #[test]
    fn test_template_meeting_notes_content() {
        let content = Template::MeetingNotes.content();
        assert!(content.contains("Meeting Notes"));
        assert!(content.contains("Attendees"));
        assert!(content.contains("Action Items"));
    }

    #[test]
    fn test_template_changelog_content() {
        let content = Template::Changelog.content();
        assert!(content.contains("Changelog"));
        assert!(content.contains("Unreleased"));
    }

    // --- Selection tests ---

    #[test]
    fn test_selected_text_none() {
        let doc = Document::new();
        assert!(doc.selected_text().is_none());
    }

    #[test]
    fn test_selected_text_single_line() {
        let mut doc = Document::new();
        doc.lines = vec!["Hello World".to_string()];
        doc.selection_anchor = Some((0, 0));
        doc.cursor_line = 0;
        doc.cursor_col = 5;
        let text = doc.selected_text().unwrap();
        assert_eq!(text, "Hello");
    }

    #[test]
    fn test_delete_selection_single_line() {
        let mut doc = Document::new();
        doc.lines = vec!["Hello World".to_string()];
        doc.selection_anchor = Some((0, 0));
        doc.cursor_line = 0;
        doc.cursor_col = 5;
        let deleted = doc.delete_selection();
        assert_eq!(deleted, Some("Hello".to_string()));
        assert_eq!(doc.lines[0], " World");
    }

    // --- Color constant tests ---

    #[test]
    fn test_color_constants() {
        assert_eq!(BASE.r, 0x1E);
        assert_eq!(BASE.g, 0x1E);
        assert_eq!(BASE.b, 0x2E);
        assert_eq!(BLUE.r, 0x89);
        assert_eq!(TEXT.r, 0xCD);
    }

    #[test]
    fn test_color_from_hex() {
        let c = Color::from_hex(0xFF0000);
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 255);
    }

    // --- Horizontal rule detection tests ---

    #[test]
    fn test_is_horizontal_rule_valid() {
        assert!(is_horizontal_rule("---"));
        assert!(is_horizontal_rule("***"));
        assert!(is_horizontal_rule("___"));
        assert!(is_horizontal_rule("- - -"));
        assert!(is_horizontal_rule("----"));
    }

    #[test]
    fn test_is_horizontal_rule_invalid() {
        assert!(!is_horizontal_rule("--"));
        assert!(!is_horizontal_rule("abc"));
        assert!(!is_horizontal_rule("-"));
    }

    // --- Table separator detection ---

    #[test]
    fn test_is_table_separator_valid() {
        assert!(is_table_separator("|---|---|"));
        assert!(is_table_separator("|:---|:---:|---:|"));
        assert!(is_table_separator("| --- | --- |"));
    }

    #[test]
    fn test_is_table_separator_invalid() {
        assert!(!is_table_separator("| text | text |"));
        assert!(!is_table_separator("not a table"));
    }

    // --- List detection tests ---

    #[test]
    fn test_is_unordered_list_start() {
        assert!(is_unordered_list_start("- Item"));
        assert!(is_unordered_list_start("* Item"));
        assert!(is_unordered_list_start("+ Item"));
        assert!(is_unordered_list_start("  - Indented"));
    }

    #[test]
    fn test_is_ordered_list_start() {
        assert!(is_ordered_list_start("1. First"));
        assert!(is_ordered_list_start("10. Tenth"));
        assert!(is_ordered_list_start("1) Alt"));
    }

    #[test]
    fn test_is_not_ordered_list() {
        assert!(!is_ordered_list_start("abc"));
        assert!(!is_ordered_list_start("1word"));
    }

    // --- Autosave tests ---

    #[test]
    fn test_autosave_tick() {
        let mut app = App::new(1280.0, 800.0);
        app.autosave_enabled = true;
        app.autosave_interval = 60;
        app.active_document_mut().modified = true;
        app.tick_autosave(30);
        assert_eq!(app.active_document().seconds_since_save, 30);
    }

    #[test]
    fn test_autosave_disabled() {
        let mut app = App::new(1280.0, 800.0);
        app.autosave_enabled = false;
        app.tick_autosave(100);
        assert_eq!(app.active_document().seconds_since_save, 0);
    }

    // --- Scroll sync tests ---

    #[test]
    fn test_sync_scroll() {
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().lines = (0..100).map(|i| format!("Line {}", i)).collect();
        app.active_document_mut().scroll_line = 50;
        app.sync_scroll();
        assert!(app.active_document().preview_scroll > 0.0);
    }

    // --- inlines_to_plain_text tests ---

    #[test]
    fn test_inlines_to_plain_text_simple() {
        let inlines = vec![MdInline::Text("Hello".to_string())];
        assert_eq!(inlines_to_plain_text(&inlines), "Hello");
    }

    #[test]
    fn test_inlines_to_plain_text_bold() {
        let inlines = vec![MdInline::Bold(vec![MdInline::Text("bold".to_string())])];
        assert_eq!(inlines_to_plain_text(&inlines), "bold");
    }

    #[test]
    fn test_inlines_to_plain_text_link() {
        let inlines = vec![MdInline::Link {
            text: vec![MdInline::Text("click".to_string())],
            url: "https://example.com".to_string(),
        }];
        assert_eq!(inlines_to_plain_text(&inlines), "click");
    }

    #[test]
    fn test_inlines_to_plain_text_image() {
        let inlines = vec![MdInline::Image {
            alt: "photo".to_string(),
            url: "img.png".to_string(),
        }];
        assert_eq!(inlines_to_plain_text(&inlines), "photo");
    }

    // --- Toolbar button tests ---

    #[test]
    fn test_default_toolbar_buttons_count() {
        let buttons = default_toolbar_buttons();
        assert!(buttons.len() > 10);
    }

    // --- ToolbarAction tests ---

    #[test]
    fn test_toolbar_action_apply_template() {
        let mut app = App::new(1280.0, 800.0);
        app.handle_toolbar_action(&ToolbarAction::ApplyTemplate(1));
        assert_eq!(app.documents.count(), 2);
    }

    #[test]
    fn test_toolbar_action_find_replace_toggle() {
        let mut app = App::new(1280.0, 800.0);
        assert!(!app.find_state.visible);
        app.handle_toolbar_action(&ToolbarAction::FindReplace);
        assert!(app.find_state.visible);
        app.handle_toolbar_action(&ToolbarAction::FindReplace);
        assert!(!app.find_state.visible);
    }

    #[test]
    fn test_toolbar_action_undo_redo() {
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().insert_char('A');
        app.handle_toolbar_action(&ToolbarAction::Undo);
        assert_eq!(app.active_document().lines[0], "");
        app.handle_toolbar_action(&ToolbarAction::Redo);
        assert_eq!(app.active_document().lines[0], "A");
    }

    // --- External-change / three-way merge tests ---

    fn temp_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("slate_md_test_{tag}_{nanos}.md"));
        p
    }

    #[test]
    fn test_disk_changed_detects_modification() {
        let path = temp_path("md_detect");
        std::fs::write(&path, b"# Title\n\nbody\n").unwrap();
        let doc = Document::from_file(&path).unwrap();
        assert_eq!(doc.disk_changed(), DiskChange::Unchanged);
        std::fs::write(&path, b"# Title\n\nCHANGED body\n").unwrap();
        match doc.disk_changed() {
            DiskChange::Modified { disk } => assert_eq!(disk, "# Title\n\nCHANGED body"),
            other => panic!("expected Modified, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_merge_clean_with_context_anchor() {
        let mut d = Document::new();
        d.lines = vec!["a".into(), "mid".into(), "b".into()];
        d.sync.base = Some("a\nmid\nb".to_string());
        d.lines = vec!["A".into(), "mid".into(), "b".into()];
        d.modified = true;
        let outcome = d.merge_from_disk("a\nmid\nB");
        assert_eq!(outcome, MergeOutcome::Clean);
        assert_eq!(d.full_text(), "A\nmid\nB");
    }

    #[test]
    fn test_review_and_accept_ours() {
        let path = temp_path("md_review");
        std::fs::write(&path, b"shared\n").unwrap();
        let mut app = App::new(1280.0, 800.0);
        app.open_file(&path).unwrap();
        app.active_document_mut().lines = vec!["local".to_string()];
        app.active_document_mut().modified = true;
        std::fs::write(&path, b"remote\n").unwrap();
        assert!(app.check_external_change());
        app.resolve_external(ExternalChoice::Review);
        app.review_set_choice(0, ConflictChoice::Ours);
        app.review_accept();
        assert!(app.external_prompt.is_none());
        assert_eq!(app.active_document().full_text(), "local");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_auto_reload_unmodified() {
        let path = temp_path("md_autoreload");
        std::fs::write(&path, b"first\n").unwrap();
        let mut app = App::new(1280.0, 800.0);
        app.open_file(&path).unwrap();
        std::fs::write(&path, b"second\n").unwrap();
        assert!(!app.check_external_change());
        assert_eq!(app.active_document().full_text(), "second");
        let _ = std::fs::remove_file(&path);
    }
    // --- Text measurement ---

    /// The document counts columns in bytes; the grid counts them in cells.
    /// A multi-byte character is one cell, not two or three — get this wrong
    /// and the caret sits several columns right of the character it precedes
    /// on any line holding an accent, and the selection band stretches with it.
    #[test]
    fn column_x_counts_cells_not_bytes() {
        let cell = char_width();
        // "é" is two bytes, so the byte offset after it is 2 — but it is one
        // cell, so the caret belongs one cell in.
        assert!((col_x("éx", 2, 0.0) - cell).abs() < 0.01);
        assert!((col_x("ax", 1, 0.0) - cell).abs() < 0.01);
        // Same text, same cell count, whatever the encoding costs.
        assert!((col_x("ééé", 6, 0.0) - col_x("aaa", 3, 0.0)).abs() < 0.01);
    }

    /// Counting cells is only right if a character actually fits one. The
    /// source pane holds prose and Markdown syntax, not digits, so the cell
    /// has to hold the widest glyph a line can contain — otherwise `col_x`
    /// under-counts and the caret drifts left of its character.
    #[test]
    fn a_source_character_fits_a_cell() {
        let cell = char_width();
        for ch in ['0', 'W', 'i', '#', 'é', 'M', '@', '*', '_', ' '] {
            let w = text::measure_in(
                &ch.to_string(),
                EDITOR_FONT_SIZE,
                FontWeightHint::Regular,
                FontFamily::Mono,
            );
            assert!(w <= cell + 0.01, "{ch:?} measures {w} in a {cell} cell");
        }
    }

    /// Syntax highlighting draws headings and emphasis bold on the same grid,
    /// so bold has to fit the same cell as regular.
    #[test]
    fn a_bold_source_character_fits_the_same_cell() {
        let cell = char_width();
        for ch in ['0', 'W', 'M', '#', '*'] {
            let w = text::measure_in(
                &ch.to_string(),
                EDITOR_FONT_SIZE,
                FontWeightHint::Bold,
                FontFamily::Mono,
            );
            assert!(
                w <= cell + 0.01,
                "bold {ch:?} measures {w} in a {cell} cell"
            );
        }
    }

    /// The source pane is placed on a mono cell, so it must be drawn in the
    /// mono face — a mismatch is invisible in any assertion about positions.
    #[test]
    fn the_source_pane_is_drawn_in_the_family_it_was_measured_in() {
        let mut doc = Document::new();
        doc.insert_text("# Heading WWWW\n\nBody text with iiii and *emphasis*.\n");
        let cmds = render_editor(&doc, 0.0, 0.0, 600.0, 400.0, &FindReplaceState::default());

        let mut depth = 0_i32;
        let mut deepest = 0_i32;
        let mut inside = 0_usize;
        for cmd in &cmds {
            match cmd {
                RenderCommand::PushFont { family } => {
                    assert_eq!(family, &FontFamily::Mono, "only the source pane pushes");
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
        assert_eq!(deepest, 1, "the source pane's scope was never opened");
        assert!(
            inside > 0,
            "no source glyph was drawn inside the mono scope"
        );
    }

    /// A byte offset past the end (or off a character boundary, which the
    /// document should never produce but rendering must survive) must not
    /// panic — slicing a `str` at a non-boundary is an outright abort.
    #[test]
    fn column_x_survives_a_bad_offset() {
        assert!(col_x("é", 1, 0.0) >= 0.0, "mid-character offset");
        assert!(col_x("abc", 99, 0.0) >= 0.0, "past the end");
        assert!((col_x("", 0, 5.0) - 5.0).abs() < f32::EPSILON);
    }

    /// The active document tab is drawn bold, so it has to be measured bold.
    /// Sizing every tab as if it were regular made the active one — the only
    /// tab the user is looking at — the one whose name overflowed.
    #[test]
    fn active_tab_is_measured_in_the_weight_it_is_drawn_in() {
        let name = "a-fairly-long-document-name.md";
        let bold = text::measure(name, TOOLBAR_FONT_SIZE, FontWeightHint::Bold);
        let regular = text::measure(name, TOOLBAR_FONT_SIZE, FontWeightHint::Regular);
        assert!(bold >= regular, "bold is never narrower than regular");
    }

    /// Toolbar labels have to fit the buttons drawn around them; the button
    /// reserves 8 px of padding on each side.
    #[test]
    fn toolbar_labels_fit_their_buttons() {
        for label in ["B", "Italic", "Heading 1", "Numbered List"] {
            let w = text::width(label, TOOLBAR_FONT_SIZE) + 16.0;
            assert!(
                text::width(label, TOOLBAR_FONT_SIZE) <= w - 16.0 + 0.01,
                "{label:?} does not fit its button"
            );
            assert!(w > 16.0, "{label:?} produced a zero-width button");
        }
    }
    // --- Columns are byte offsets, and must stay on character boundaries ---

    /// A document whose lines are the same *character* length but wildly
    /// different byte lengths, so carrying a column from one to another lands
    /// mid-character unless it is rounded to a boundary.
    fn wide_document() -> Vec<String> {
        [
            "abcdefgh",                         // 8 chars, 8 bytes
            "\u{65e5}x\u{672c}y\u{8a9e}",       // mixed widths
            "\u{03b1}\u{03b2}\u{03b3}\u{03b4}", // 2 bytes/char
            "\u{1f600}\u{1f601}",               // 4 bytes/char
            "caf\u{e9}",                        // ASCII then not
            "",                                 // empty line
            "\u{0440}\u{0435}\u{0437}\u{0443}\u{043c}\u{0435}",
        ]
        .iter()
        .map(|l| (*l).to_string())
        .collect()
    }

    fn wide_doc() -> Document {
        let mut doc = Document::new();
        doc.lines = wide_document();
        doc
    }

    #[test]
    fn moving_down_onto_a_wide_line_leaves_the_cursor_on_a_boundary() {
        // The concrete case: column 1 of an ASCII line is a valid boundary, and
        // the same byte offset on the line below is inside a kanji.
        let mut doc = Document::new();
        doc.lines = vec!["abc".to_string(), "\u{65e5}x".to_string()];
        doc.cursor_col = 1;
        doc.move_cursor_down();
        assert_eq!(doc.cursor_line, 1);
        assert!(
            doc.lines[1].is_char_boundary(doc.cursor_col),
            "cursor landed at byte {} inside a character of {:?}",
            doc.cursor_col,
            doc.lines[1]
        );
        // Rounding down means the start of the character it landed in.
        assert_eq!(doc.cursor_col, 0);
    }

    #[test]
    fn no_vertical_move_strands_the_cursor_mid_character() {
        let mut checked = 0usize;
        for start_line in 0..wide_document().len() {
            for start_col in 0..=24 {
                for down in [true, false] {
                    let mut doc = wide_doc();
                    doc.cursor_line = start_line;
                    doc.cursor_col = clamp_col(&doc.lines[start_line], start_col);
                    if down {
                        doc.move_cursor_down();
                    } else {
                        doc.move_cursor_up();
                    }
                    assert!(
                        doc.lines[doc.cursor_line].is_char_boundary(doc.cursor_col),
                        "byte {} is inside a character of {:?}",
                        doc.cursor_col,
                        doc.lines[doc.cursor_line]
                    );
                    checked += 1;
                }
            }
        }
        assert!(
            checked >= 300,
            "expected the sweep to cover every line and column, got {checked}"
        );
    }

    #[test]
    fn an_edit_after_a_vertical_move_does_not_abort_the_editor() {
        // The panic did not happen on the move -- it happened on the next edit,
        // by which point the document is unsaved and the user has typed.
        let mut checked = 0usize;
        for start_line in 0..wide_document().len() {
            for start_col in 0..=24 {
                for edit in 0..6 {
                    let mut doc = wide_doc();
                    doc.cursor_line = start_line;
                    doc.cursor_col = clamp_col(&doc.lines[start_line], start_col);
                    doc.move_cursor_down();
                    match edit {
                        0 => doc.delete_backward(),
                        1 => doc.delete_forward(),
                        2 => doc.insert_char('\u{e9}'),
                        3 => doc.insert_text("\u{65e5}\u{672c}"),
                        4 => doc.insert_newline(),
                        _ => doc.move_cursor_left(),
                    }
                    // Whatever the edit did, every line is still valid text and
                    // the cursor is still somewhere we can slice.
                    assert!(
                        doc.lines[doc.cursor_line].is_char_boundary(doc.cursor_col),
                        "edit {edit} left the cursor at byte {} inside a character of {:?}",
                        doc.cursor_col,
                        doc.lines[doc.cursor_line]
                    );
                    checked += 1;
                }
            }
        }
        assert!(
            checked >= 900,
            "expected the sweep to cover every line, column and edit, got {checked}"
        );
    }

    #[test]
    fn every_edit_survives_a_column_left_mid_character() {
        // The vertical move is how the cursor *got* off-boundary, but it is not
        // the only way: an undo replays a recorded column against a line that
        // has since changed, and a click maps an x to a byte. So each edit path
        // is checked here against a column stranded directly, rather than
        // through `move_cursor_down` -- otherwise the first edit to abort masks
        // every site after it and the sweep proves nothing about them.
        let mut checked = 0usize;
        for line_idx in 0..wide_document().len() {
            let line_len = wide_document()[line_idx].len();
            for col in 0..=line_len {
                for edit in 0..7 {
                    let mut doc = wide_doc();
                    doc.cursor_line = line_idx;
                    // Deliberately *not* clamped: this is the stranded state.
                    doc.cursor_col = col;
                    match edit {
                        0 => doc.delete_backward(),
                        1 => doc.delete_forward(),
                        2 => doc.insert_char('\u{e9}'),
                        3 => doc.insert_text("\u{65e5}\u{672c}"),
                        4 => doc.insert_newline(),
                        5 => doc.move_cursor_left(),
                        _ => doc.move_cursor_right(),
                    }
                    checked += 1;
                }
                // Reading and deleting a selection anchored off-boundary too.
                let mut doc = wide_doc();
                doc.cursor_line = line_idx;
                doc.cursor_col = col;
                doc.selection_anchor = Some((line_idx, 1));
                let _ = doc.selected_text();
                let _ = doc.delete_selection();
                checked += 1;
            }
        }
        assert!(
            checked >= 300,
            "expected the sweep to cover every column of every line, got {checked}"
        );
    }

    #[test]
    fn a_selection_spanning_wide_lines_can_be_read_and_deleted() {
        let mut checked = 0usize;
        let n = wide_document().len();
        for anchor_line in 0..n {
            for anchor_col in [0usize, 1, 3, 5] {
                for cursor_line in 0..n {
                    let mut doc = wide_doc();
                    // Anchors are set from a click or a shift-drag, so they can
                    // name any offset the caller likes -- including one that is
                    // not a boundary on the line it ends up compared against.
                    doc.selection_anchor = Some((anchor_line, anchor_col));
                    doc.cursor_line = cursor_line;
                    doc.cursor_col = 3;
                    let read = doc.selected_text();
                    let deleted = doc.delete_selection();
                    if let (Some(r), Some(d)) = (read, deleted) {
                        assert_eq!(r, d, "selected_text and delete_selection disagree");
                    }
                    checked += 1;
                }
            }
        }
        assert!(
            checked >= 150,
            "expected the sweep to cover every anchor/cursor pair, got {checked}"
        );
    }

    #[test]
    fn a_reload_from_disk_lands_the_cursor_on_a_boundary() {
        // The file changing outside the editor replaces every line under a
        // cursor that was a valid boundary in the *old* text. This is the one
        // clamp the user never triggers themselves.
        let mut checked = 0usize;
        for col in 0..=8 {
            let mut doc = Document::new();
            doc.lines = vec!["abcdefgh".to_string()];
            doc.cursor_col = col;
            let disk = "\u{65e5}x\u{672c}y\u{8a9e}";
            doc.apply_merged(disk, disk);
            assert!(
                doc.lines[doc.cursor_line].is_char_boundary(doc.cursor_col),
                "a reload left the cursor at byte {} inside a character of {:?}",
                doc.cursor_col,
                doc.lines[doc.cursor_line]
            );
            checked += 1;
        }
        assert!(checked >= 9, "expected every column, got {checked}");
    }

    #[test]
    fn jumping_to_a_line_lands_on_a_boundary() {
        // Go-to-line keeps the column, so it carries the same hazard as Down.
        let mut checked = 0usize;
        for target in 0..wide_document().len() {
            for col in 0..=12 {
                let mut doc = wide_doc();
                doc.cursor_col = col;
                doc.go_to_line(target);
                assert!(
                    doc.lines[doc.cursor_line].is_char_boundary(doc.cursor_col),
                    "go_to_line({target}) with column {col} landed at byte {} inside a \
                     character of {:?}",
                    doc.cursor_col,
                    doc.lines[doc.cursor_line]
                );
                checked += 1;
            }
        }
        assert!(
            checked >= 80,
            "expected every line/column pair, got {checked}"
        );
    }

    #[test]
    fn an_undo_replayed_against_a_changed_line_does_not_abort() {
        // A recorded column is a boundary *of the line as it was*. Anything that
        // rewrites the line underneath -- a later edit, or a reload from disk
        // when the file changed outside the editor -- can leave that column
        // pointing inside a character by the time the undo replays it.
        let mut checked = 0usize;
        for col in 0..=8 {
            for text in ["x", "\u{65e5}", "ab"] {
                let mut doc = Document::new();
                doc.lines = vec!["abcdefgh".to_string()];
                doc.cursor_col = col.min(8);
                doc.insert_text(text);
                // The document is replaced under the recorded action, the way a
                // reload does when the file changed outside the editor.
                let disk = "\u{65e5}x\u{672c}y\u{8a9e}";
                doc.apply_merged(disk, disk);
                doc.undo();
                doc.redo();
                doc.undo();
                assert!(
                    doc.lines[doc.cursor_line].is_char_boundary(doc.cursor_col),
                    "replay left the cursor at byte {} inside a character of {:?}",
                    doc.cursor_col,
                    doc.lines[doc.cursor_line]
                );
                checked += 1;
            }
        }
        assert!(
            checked >= 24,
            "expected every column/text pair, got {checked}"
        );
    }

    #[test]
    fn undo_and_redo_replay_a_non_ascii_edit() {
        let mut doc = Document::new();
        doc.insert_text("\u{65e5}\u{672c}\u{8a9e}");
        let typed = doc.lines[0].clone();
        doc.undo();
        doc.redo();
        assert_eq!(doc.lines[0], typed);
        assert!(doc.lines[0].is_char_boundary(doc.cursor_col));
    }

    #[test]
    fn an_ascii_document_clamps_exactly_as_before() {
        // `clamp_col` must be indistinguishable from `.min(line.len())` when
        // every character is one byte, or it has changed behaviour for the
        // common case.
        for line in ["", "a", "hello world"] {
            for byte in 0..=15 {
                assert_eq!(
                    clamp_col(line, byte),
                    byte.min(line.len()),
                    "{line:?} {byte}"
                );
            }
        }
    }
}
