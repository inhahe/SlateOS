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
//!   Ctrl+Shift+K code block, and the rest on the F1 card
//! - The desktop's theme throughout
//!
//! Uses the guitk library for UI rendering.
//!
//! # The pointer
//!
//! Every control is drawn and hit-tested by one walk, [`App::frame`], through
//! [`guitk::frame::Frame`]: the toolbar (with a `»` menu for the buttons a
//! narrow window has no room for), the tabs and their close buttons (with a
//! `»` menu listing every document once they overflow), the table of contents,
//! the find panel, the status bar's view and auto-save switches, and the three
//! dialogs. In the source pane a press places the caret, a drag or a
//! Shift+press selects, and the wheel scrolls whichever pane it is over.
//!
//! Until 2026-09-25 this app handled no pointer event of any kind, and five of
//! the things listed above -- the view mode, the contents, the templates, Save
//! As and the tabs -- had no key either, so nothing could reach them at all.
//! `known-issues.md` → `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`
//! has the whole list.

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::color::Color;
use guitk::event::{MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::menu::{ContextMenu, MenuAction, MenuItem};
use guitk::render::{FontFamily, FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::tabs::Tabs;
use guitk::text;
use guitk::textfind::{self, Case};
use guitk::wheel;

use diffcore::{
    ConflictChoice, DiskChange, FileSync, MergeOutcome, MergeReview, ThreeWayMerge,
    normalize_content,
};

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;

/// `path`'s file name as the window shows it: the name itself when it is
/// text, its bytes as escapes (`quoting::escape_unprintable`) when it is not
/// -- never a lossy decode, which shows two such names alike.
fn shown_file_name(path: &std::path::Path) -> Option<String> {
    let name = path.file_name()?;
    Some(name.to_str().map_or_else(
        || quoting::escape_unprintable(name.as_encoded_bytes()),
        str::to_owned,
    ))
}
use unsaved::{Choice, Question};

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

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
/// x of byte offset `col` within `line`, given the pane's left edge `text_x`.
///
/// The document stores cursor and selection positions as byte offsets on
/// character boundaries, which is the right model for a text buffer — but the
/// grid counts *cells*, and a multi-byte character occupies one cell, not two
/// or three. Multiplying the byte offset by the cell width put the caret
/// several columns right of the character it precedes on any line holding
/// non-ASCII text, and stretched the selection band with it.
///
/// Counting characters and multiplying by a cell was the next version of that
/// same mistake, one step further out. It assumes every character advances the
/// cell width, which is what "monospace" means for Latin text and is *not* true
/// of the text this pane can hold: a CJK ideograph is conventionally two cells
/// wide, a combining accent is zero, and a face missing a glyph substitutes one
/// of whatever width `.notdef` happens to be. The nominal count and the drawn
/// advance then disagree, and every mark this function places — the caret, the
/// selection band, the find highlights, and the left edge of every syntax span
/// after the first — lands beside the character it belongs to, drifting further
/// with each such character on the line.
///
/// So it measures instead, in the family the pane pushes and the caller draws
/// in. The answer is then the renderer's own answer by construction, which is
/// the only definition of "where is column `col`" that cannot be wrong. Weight
/// is not passed in even though spans are drawn bold: on a monospace face bold
/// advances the same distance as regular (asserted by
/// `a_bold_source_character_fits_the_same_cell`), and threading a per-span
/// weight through here would make the caret's position depend on which syntax
/// span it happens to fall in.
fn col_x(line: &str, col: usize, text_x: f32) -> f32 {
    // A byte offset off a character boundary is a bug elsewhere, but slicing a
    // `str` there aborts the process, so it is floored to the boundary below.
    // Falling back to the *whole line* — what this used to do — turned a caret
    // one byte out of place into a caret at the end of the line.
    let mut end = col.min(line.len());
    while end > 0 && !line.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let prefix = line.get(..end).unwrap_or("");
    text_x
        + text::measure_in(
            prefix,
            EDITOR_FONT_SIZE,
            FontWeightHint::Regular,
            FontFamily::Mono,
        )
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
        let name = shown_file_name(path).unwrap_or_else(|| "Untitled".to_string());
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
    ///
    /// Written through [`safeio::write_str_atomically`] rather than
    /// `fs::write`, which truncates the target before writing it and so left
    /// the user's document as a fragment if the save was interrupted. See the
    /// `safeio` crate docs for why that is the worst failure a document editor
    /// has available to it.
    pub fn save(&mut self) -> std::io::Result<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "No file path set"))?
            .clone();
        let content = self.lines.join("\n");
        safeio::write_str_atomically(&path, &content)?;
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
        safeio::write_str_atomically(path, &content)?;
        self.path = Some(path.to_path_buf());
        self.name = shown_file_name(path).unwrap_or_else(|| "Untitled".to_string());
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
            self.lines.len().saturating_sub(1)
        };
        line_chars.saturating_add(newlines)
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

    /// Remove `text` from the document at `(line, col)`, and put the cursor
    /// where it was.
    ///
    /// Undoing an insert and redoing a delete are the same operation, so they
    /// share it — as do the two below. An undo stack outlives the text it
    /// describes (a reload, or a merge from disk, replaces every line while
    /// leaving the stack alone), so each of these validates rather than
    /// trusting the recorded position.
    ///
    /// `text` may span lines. A selection deleted across three lines is
    /// recorded as one `Delete` whose text holds two newlines, and this used to
    /// erase `text.len()` bytes *of the first line* -- clamped at its end -- so
    /// redoing that delete removed the tail of one line and left the other two
    /// in place.
    fn erase_at(&mut self, line: usize, col: usize, text: &str) {
        let Some(first) = self.lines.get(line) else {
            return;
        };
        let start = clamp_col(first, col);
        let end = match text.rsplit_once('\n') {
            None => (line, start.saturating_add(text.len())),
            Some((_, last)) => (line.saturating_add(text.matches('\n').count()), last.len()),
        };
        self.remove_range((line, start), end);
        self.cursor_line = line;
        self.cursor_col = start;
    }

    /// Put `insert` back into the document at `(line, col)`, and put the
    /// cursor after it.
    ///
    /// `insert` may span lines, for the reason [`erase_at`](Self::erase_at)
    /// gives: this used to `insert_str` it into the one line whole, so undoing
    /// a multi-line delete left a line with newlines *inside* it -- drawn as
    /// one line, counted as one line, and stepped over by the cursor as one
    /// line, until the file was saved and reopened.
    fn restore_at(&mut self, line: usize, col: usize, insert: &str) {
        let Some(text) = self.lines.get_mut(line) else {
            return;
        };
        let col = clamp_col(text, col);
        let tail = text.split_off(col);
        let mut pieces = insert.split('\n');
        text.push_str(pieces.next().unwrap_or(""));
        let mut at = line;
        for piece in pieces {
            at = at.saturating_add(1);
            self.lines
                .insert(at.min(self.lines.len()), piece.to_string());
        }
        let Some(last) = self.lines.get_mut(at) else {
            return;
        };
        self.cursor_line = at;
        self.cursor_col = last.len();
        last.push_str(&tail);
    }

    /// Delete everything from `start` to `end`, `start` first, each a
    /// `(line, col)` clamped to the document.
    ///
    /// The one place text between two positions is removed: the selection's
    /// delete and the undo stack's erase both come here, so the two cannot
    /// disagree about what "the text from here to there" is.
    fn remove_range(&mut self, start: (usize, usize), end: (usize, usize)) {
        let last = self.lines.len().saturating_sub(1);
        let (start, end) = if end < start {
            (end, start)
        } else {
            (start, end)
        };
        let (sl, el) = (start.0.min(last), end.0.min(last));
        let s = clamp_col(self.line_text(sl), start.1);
        let e = clamp_col(self.line_text(el), end.1);
        if sl == el {
            if let Some(text) = self.lines.get_mut(sl) {
                let e = e.max(s);
                text.replace_range(s..e, "");
            }
            return;
        }
        let remaining = self.line_text(el).get(e..).unwrap_or("").to_string();
        // Lines strictly after the first, through the last, go.
        self.lines.drain(sl.saturating_add(1)..=el.min(last));
        if let Some(text) = self.lines.get_mut(sl) {
            text.truncate(s);
            text.push_str(&remaining);
        }
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
            EditAction::Insert { line, col, text } => self.erase_at(*line, *col, text),
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
            EditAction::Delete { line, col, text } => self.erase_at(*line, *col, text),
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
        let anchor = self.selection_anchor.take()?;
        let caret = (self.cursor_line, self.cursor_col);
        let (start, end) = if anchor < caret {
            (anchor, caret)
        } else {
            (caret, anchor)
        };
        // Clamped to the document as it stands: a selection can outlive the
        // text it was made on (a reload, a merge from disk), and the undo entry
        // has to describe what was actually removed -- including the column,
        // which used to be recorded unclamped, so undo put the text back one
        // byte off wherever the clamp had moved it.
        let start = self.clamp_position(start);
        let end = self.clamp_position(end);
        let deleted = self.text_between(start, end);
        self.remove_range(start, end);
        self.cursor_line = start.0;
        self.cursor_col = start.1;
        if !deleted.is_empty() {
            self.push_undo(EditAction::Delete {
                line: start.0,
                col: start.1,
                text: deleted.clone(),
            });
        }
        Some(deleted)
    }

    /// `(line, col)` moved onto the document: a line past the end becomes the
    /// end of the last line, and a column is floored to a character boundary.
    fn clamp_position(&self, (line, col): (usize, usize)) -> (usize, usize) {
        let last = self.lines.len().saturating_sub(1);
        if line > last {
            return (last, self.line_text(last).len());
        }
        (line, clamp_col(self.line_text(line), col))
    }

    /// Select the whole document, caret at the end.
    pub fn select_all(&mut self) {
        let last = self.lines.len().saturating_sub(1);
        self.selection_anchor = Some((0, 0));
        self.cursor_line = last;
        self.cursor_col = self.line_text(last).len();
    }

    /// Whether some text is selected (an anchor that is not the caret).
    pub fn has_selection(&self) -> bool {
        self.selection_anchor
            .is_some_and(|anchor| anchor != (self.cursor_line, self.cursor_col))
    }

    /// The selection's two ends, earlier first, if there is a selection.
    pub fn selection_bounds(&self) -> Option<((usize, usize), (usize, usize))> {
        let anchor = self.selection_anchor?;
        let caret = (self.cursor_line, self.cursor_col);
        (anchor != caret).then(|| {
            if anchor < caret {
                (anchor, caret)
            } else {
                (caret, anchor)
            }
        })
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
        } else if self.cursor_line >= self.scroll_line.saturating_add(visible_lines) {
            self.scroll_line = self
                .cursor_line
                .saturating_sub(visible_lines)
                .saturating_add(1);
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
            idx = idx.saturating_add(1);
            continue;
        }

        // Horizontal rule: ---, ***, ___ (3+ chars, optional spaces).
        if is_horizontal_rule(line) {
            blocks.push(MdBlock::HorizontalRule);
            idx = idx.saturating_add(1);
            continue;
        }

        // Heading: # through ######.
        if let Some(heading) = parse_heading(line) {
            blocks.push(heading);
            idx = idx.saturating_add(1);
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
            idx = idx.saturating_add(1);
            while let Some(&cl) = lines.get(idx) {
                if cl.trim_start().starts_with(&closing_fence)
                    && cl.trim().chars().all(|c| c == fence_char)
                {
                    idx = idx.saturating_add(1);
                    break;
                }
                code_lines.push(cl);
                idx = idx.saturating_add(1);
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
                idx = idx.saturating_add(1);
            }
            let inner_text = quote_lines.join("\n");
            let children = parse_markdown(&inner_text);
            blocks.push(MdBlock::BlockQuote { children });
            continue;
        }

        // Table: line contains | and the next line is a separator row.
        if let Some(&separator) = idx
            .checked_add(1)
            .and_then(|n| lines.get(n))
            .filter(|_| line.contains('|'))
            && is_table_separator(separator)
        {
            let headers = parse_table_row(line);
            let alignments = parse_table_alignments(separator);
            idx = idx.saturating_add(2);
            let mut rows = Vec::new();
            while let Some(&row) = lines
                .get(idx)
                .filter(|l| l.contains('|') && !l.trim().is_empty())
            {
                rows.push(parse_table_row(row));
                idx = idx.saturating_add(1);
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
                idx = idx.saturating_add(1);
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
                idx = idx.saturating_add(1);
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
            idx = idx.saturating_add(1);
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
    /// Whether typing goes to the replacement box rather than the query box.
    ///
    /// The panel has two text boxes and this app has no pointer, so there has
    /// to be something that says which one a keystroke belongs to. `Tab`
    /// moves between them.
    pub focus_replacement: bool,
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
            focus_replacement: false,
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
            self.current_match = self
                .current_match
                .saturating_add(1)
                .checked_rem(self.matches.len())
                .unwrap_or(0);
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
    if start > end
        || end > line.len()
        || !line.is_char_boundary(start)
        || !line.is_char_boundary(end)
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
pub fn highlight_line(line: &str, pal: &Palette) -> Vec<HighlightSpan> {
    let mut spans = Vec::new();
    let trimmed = line.trim_start();
    let indent_len = line.len().saturating_sub(trimmed.len());

    // Heading lines: color the whole line.
    if trimmed.starts_with('#') {
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if hashes <= 6 && (trimmed.len() == hashes || trimmed.as_bytes().get(hashes) == Some(&b' '))
        {
            let heading_color = match hashes {
                1 => pal.blue,
                2 => pal.lavender,
                3 => pal.green,
                4 => pal.yellow,
                5 => pal.peach,
                _ => pal.red,
            };
            // Hash marks get dimmed.
            spans.push(HighlightSpan {
                start: indent_len,
                end: indent_len.saturating_add(hashes),
                color: pal.overlay0,
                weight: FontWeightHint::Bold,
            });
            // The heading text.
            if line.len() > indent_len.saturating_add(hashes) {
                spans.push(HighlightSpan {
                    start: indent_len.saturating_add(hashes),
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
            color: pal.green,
            weight: FontWeightHint::Regular,
        });
        return spans;
    }

    // Blockquote prefix.
    if trimmed.starts_with('>') {
        spans.push(HighlightSpan {
            start: indent_len,
            end: indent_len.saturating_add(1),
            color: pal.blue,
            weight: FontWeightHint::Bold,
        });
        if line.len() > indent_len.saturating_add(1) {
            spans.push(HighlightSpan {
                start: indent_len.saturating_add(1),
                end: line.len(),
                color: pal.subtext0,
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
            color: pal.surface2,
            weight: FontWeightHint::Regular,
        });
        return spans;
    }

    // List items: color the bullet/number.
    if is_unordered_list_start(line) {
        let bullet_end = indent_len.saturating_add(2);
        spans.push(HighlightSpan {
            start: indent_len,
            end: bullet_end.min(line.len()),
            color: pal.blue,
            weight: FontWeightHint::Bold,
        });
        // Check for task list checkbox.
        let after_bullet = &line[bullet_end.min(line.len())..];
        if after_bullet.starts_with("[x] ") || after_bullet.starts_with("[X] ") {
            spans.push(HighlightSpan {
                start: bullet_end,
                end: bullet_end.saturating_add(4),
                color: pal.green,
                weight: FontWeightHint::Regular,
            });
            highlight_inline_spans(line, pal, bullet_end.saturating_add(4), &mut spans);
        } else if after_bullet.starts_with("[ ] ") {
            spans.push(HighlightSpan {
                start: bullet_end,
                end: bullet_end.saturating_add(4),
                color: pal.overlay0,
                weight: FontWeightHint::Regular,
            });
            highlight_inline_spans(line, pal, bullet_end.saturating_add(4), &mut spans);
        } else {
            highlight_inline_spans(line, pal, bullet_end, &mut spans);
        }
        return spans;
    }

    if is_ordered_list_start(line) {
        let num_end = trimmed
            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != ')')
            .unwrap_or(trimmed.len());
        let abs_end = indent_len.saturating_add(num_end);
        spans.push(HighlightSpan {
            start: indent_len,
            end: abs_end.min(line.len()),
            color: pal.blue,
            weight: FontWeightHint::Bold,
        });
        highlight_inline_spans(line, pal, abs_end, &mut spans);
        return spans;
    }

    // Table rows.
    if line.contains('|') && is_table_separator(line) {
        spans.push(HighlightSpan {
            start: 0,
            end: line.len(),
            color: pal.surface2,
            weight: FontWeightHint::Regular,
        });
        return spans;
    }

    // Default: apply inline highlighting.
    highlight_inline_spans(line, pal, 0, &mut spans);

    // If no spans were generated, use default text color.
    if spans.is_empty() {
        spans.push(HighlightSpan {
            start: 0,
            end: line.len(),
            color: pal.text,
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
fn highlight_inline_spans(
    line: &str,
    pal: &Palette,
    start_offset: usize,
    spans: &mut Vec<HighlightSpan>,
) {
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
                push(spans, text_start, pos, pal.text, FontWeightHint::Regular);
            }
            if let Some(code_end) = scan_to(bytes, pos.saturating_add(1), b'`') {
                push(
                    spans,
                    pos,
                    code_end.saturating_add(1),
                    pal.green,
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
                    push(spans, text_start, pos, pal.text, FontWeightHint::Regular);
                }
                // Dim the opening markers, bold the text, dim the closing ones.
                push(
                    spans,
                    pos,
                    inner_start,
                    pal.overlay0,
                    FontWeightHint::Regular,
                );
                push(
                    spans,
                    inner_start,
                    inner_end,
                    pal.text,
                    FontWeightHint::Bold,
                );
                push(
                    spans,
                    inner_end,
                    inner_end.saturating_add(2),
                    pal.overlay0,
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
                    push(spans, text_start, pos, pal.text, FontWeightHint::Regular);
                }
                push(
                    spans,
                    pos,
                    inner_end.saturating_add(2),
                    pal.overlay0,
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
                    push(spans, text_start, pos, pal.text, FontWeightHint::Regular);
                }
                push(
                    spans,
                    pos,
                    inner_start,
                    pal.overlay0,
                    FontWeightHint::Regular,
                );
                push(
                    spans,
                    inner_start,
                    inner_end,
                    pal.lavender,
                    FontWeightHint::Light,
                );
                push(
                    spans,
                    inner_end,
                    inner_end.saturating_add(1),
                    pal.overlay0,
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
            let bracket_start = if is_image { pos.saturating_add(1) } else { pos };
            if let Some(bracket_end) = scan_to(bytes, bracket_start.saturating_add(1), b']')
                && bytes.get(bracket_end.saturating_add(1)) == Some(&b'(')
                && let Some(paren_end) = scan_to(bytes, bracket_end.saturating_add(2), b')')
            {
                if pos > text_start {
                    push(spans, text_start, pos, pal.text, FontWeightHint::Regular);
                }
                if is_image {
                    // An image is drawn as one span: its alt text is not the
                    // link text a reader clicks, so colouring it as one would
                    // be a lie about what it is.
                    push(
                        spans,
                        pos,
                        paren_end.saturating_add(1),
                        pal.peach,
                        FontWeightHint::Regular,
                    );
                } else {
                    push(
                        spans,
                        bracket_start,
                        bracket_end.saturating_add(1),
                        pal.blue,
                        FontWeightHint::Regular,
                    );
                    push(
                        spans,
                        bracket_end.saturating_add(1),
                        paren_end.saturating_add(1),
                        pal.overlay0,
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
            pal.text,
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

/// Make the caret's line a heading of `level` (clamped to 1–6), or plain text
/// again if it already is a heading of that level.
///
/// Replaces a heading marker that is already there rather than stacking a
/// second one in front of it: this used to insert `## ` unconditionally, so
/// pressing the button twice made `## ## Title`, and changing a heading's level
/// meant deleting its old `#`s by hand first. The caret stays on the same
/// character of the heading's text.
///
/// One undo step either way: taking the old marker off and putting the new one
/// on are recorded as a single batch.
pub fn set_heading(doc: &mut Document, level: u8) {
    let level = usize::from(level.clamp(1, 6));
    let line = doc.cursor_line;
    let Some(text) = doc.lines.get(line) else {
        return;
    };
    let hashes = text.bytes().take_while(|&b| b == b'#').count();
    let is_heading = (1..=6).contains(&hashes) && text.as_bytes().get(hashes) == Some(&b' ');
    let old_prefix = if is_heading {
        text.get(..=hashes).unwrap_or("").to_string()
    } else {
        String::new()
    };
    let new_prefix = if is_heading && hashes == level {
        String::new()
    } else {
        format!("{} ", "#".repeat(level))
    };
    let body_col = doc.cursor_col.saturating_sub(old_prefix.len());

    let mut actions = Vec::new();
    if !old_prefix.is_empty() {
        doc.erase_at(line, 0, &old_prefix);
        actions.push(EditAction::Delete {
            line,
            col: 0,
            text: old_prefix,
        });
    }
    if !new_prefix.is_empty() {
        doc.restore_at(line, 0, &new_prefix);
        let caret = new_prefix.len().saturating_add(body_col);
        actions.push(EditAction::Insert {
            line,
            col: 0,
            text: new_prefix,
        });
        doc.cursor_col = caret;
    } else {
        doc.cursor_col = body_col;
    }
    doc.cursor_line = line;
    doc.cursor_col = clamp_col(doc.line_text(line), doc.cursor_col);
    match actions.len() {
        0 => {}
        1 => {
            if let Some(action) = actions.pop() {
                doc.push_undo(action);
            }
        }
        _ => doc.push_undo(EditAction::Batch { actions }),
    }
}

/// The name to offer when saving `doc` under a new one: its own file name if
/// it has one, otherwise its tab name with `.md` on the end.
///
/// An `OsString` and never text: a document opened from a file whose name is
/// not valid UTF-8 must be offered that name, byte for byte, and not a copy
/// with a replacement character in it.
fn save_as_name(doc: &Document) -> std::ffi::OsString {
    if let Some(name) = doc.path.as_ref().and_then(|p| p.file_name()) {
        return name.to_os_string();
    }
    let mut name = std::ffi::OsString::from(&doc.name);
    name.push(".md");
    name
}

/// The name to offer for `doc`'s HTML export: its file stem, or its tab name,
/// with `.html` on the end.
fn html_export_name(doc: &Document) -> std::ffi::OsString {
    let mut name = doc
        .path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map_or_else(|| std::ffi::OsString::from(&doc.name), |s| s.to_os_string());
    name.push(".html");
    name
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
    doc.cursor_line = line.saturating_add(1);
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
    pal: &Palette,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    find_state: &FindReplaceState,
) -> Vec<RenderCommand> {
    let mut cmds = vec![
        // `col_x` places the caret, the selection band, the find highlights
        // and every syntax span by *measuring* the line's prefix in the mono
        // face. Everything this function draws has to therefore be drawn in
        // that face, or the marks land beside the characters they mark — and
        // nothing that asserts on positions can see the mismatch, because both
        // sides of such an assertion would be measured the same wrong way.
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
            color: pal.base,
            corner_radii: CornerRadii::ZERO,
        },
        // Gutter background.
        RenderCommand::FillRect {
            x,
            y,
            width: GUTTER_WIDTH,
            height,
            color: pal.mantle,
            corner_radii: CornerRadii::ZERO,
        },
        // Gutter separator line.
        RenderCommand::Line {
            x1: x + GUTTER_WIDTH,
            y1: y,
            x2: x + GUTTER_WIDTH,
            y2: y + height,
            color: pal.surface0,
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
                color: Color::rgba(pal.surface0.r, pal.surface0.g, pal.surface0.b, 100),
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
                color: Color::rgba(pal.surface0.r, pal.surface0.g, pal.surface0.b, 60),
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
                    Color::rgba(pal.yellow.r, pal.yellow.g, pal.yellow.b, 120)
                } else {
                    Color::rgba(pal.yellow.r, pal.yellow.g, pal.yellow.b, 50)
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
        let line_num_text = format!("{}", line_num.saturating_add(1));
        let num_color = if line_num == doc.cursor_line {
            pal.text
        } else {
            pal.overlay0
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
                    color: pal.green,
                    weight: FontWeightHint::Regular,
                }]
            } else {
                highlight_line(line, pal)
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
                color: pal.text,
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
                    color: Color::rgba(pal.blue.r, pal.blue.g, pal.blue.b, 60),
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
    /// The desktop theme every element draws in.
    palette: Palette,
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
    fn new(palette: Palette, x: f32, y: f32, width: f32, height: f32, scroll_offset: f32) -> Self {
        Self {
            palette,
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
    pal: &Palette,
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
        color: pal.base,
        corner_radii: CornerRadii::ZERO,
    });

    let content_x = x + PREVIEW_PADDING;
    let content_width = width - PREVIEW_PADDING * 2.0;
    let mut ctx = PreviewContext::new(
        *pal,
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
                    color: ctx.palette.ink(ctx.palette.blue),
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
                    color: ctx.palette.surface1,
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
                ctx.palette.text,
                FontWeightHint::Regular,
            );
            ctx.add_spacing(8.0);
        }
        MdBlock::CodeBlock { language, code } => {
            let block_height = (code.lines().count() as f32 + 1.0) * LINE_HEIGHT + 16.0;
            if ctx.is_visible(block_height) {
                // Code block background.
                let pal = ctx.palette;
                let g0 = ctx.x;
                let g1 = ctx.render_y();
                let g2 = ctx.width;
                pal.push_surface(&mut ctx.cmds, g0, g1, g2, block_height, 6.0, Surface::Card);

                // Language label.
                if !language.is_empty() {
                    ctx.cmds.push(RenderCommand::Text {
                        x: ctx.x + 8.0,
                        y: ctx.render_y() + 4.0,
                        text: language.clone(),
                        font_size: 10.0,
                        color: ctx.palette.subtext0,
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
                        color: ctx.palette.ink(ctx.palette.green),
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
                    color: ctx.palette.blue,
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
                        let cb_color = if checked {
                            ctx.palette.green
                        } else {
                            ctx.palette.overlay0
                        };
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
                            color: ctx.palette.text,
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
                    ctx.palette.text,
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
                    let num_text = format!("{}.", start.saturating_add(i));
                    ctx.cmds.push(RenderCommand::Text {
                        x: ctx.x,
                        y: ctx.render_y(),
                        text: num_text,
                        font_size: EDITOR_FONT_SIZE,
                        color: ctx.palette.text,
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
                    ctx.palette.text,
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
                    color: ctx.palette.surface1,
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
                let pal = ctx.palette;
                let g0 = ctx.x;
                let g1 = ctx.render_y();
                let g2 = ctx.width;
                pal.push_surface(&mut ctx.cmds, g0, g1, g2, row_height, 0.0, Surface::Card);

                // Header cells.
                for (j, cell) in headers.iter().enumerate() {
                    let cell_x = ctx.x + (j as f32) * col_width + 8.0;
                    let text = inlines_to_plain_text(cell);
                    ctx.cmds.push(RenderCommand::Text {
                        x: cell_x,
                        y: ctx.render_y() + 4.0,
                        text,
                        font_size: EDITOR_FONT_SIZE,
                        color: ctx.palette.text,
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
                    color: ctx.palette.surface1,
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
                            color: Color::rgba(
                                ctx.palette.surface0.r,
                                ctx.palette.surface0.g,
                                ctx.palette.surface0.b,
                                80,
                            ),
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
                            color: ctx.palette.text,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(col_width - 16.0),
                            overflow: TextOverflow::Ellipsis,
                        });
                    }
                }
                ctx.y += row_height;
            }

            // Table border.
            let total_height = row_height * (rows.len().saturating_add(1)) as f32 + 2.0;
            let _ = alignments; // used for alignment but rendering simplified here
            ctx.cmds.push(RenderCommand::StrokeRect {
                x: ctx.x,
                y: ctx.render_y() - total_height,
                width: ctx.width,
                height: total_height,
                color: ctx.palette.surface1,
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
                    color: ctx.palette.surface1,
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
    let text = inlines_to_styled_text(inlines, &ctx.palette);
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
fn inlines_to_styled_text(inlines: &[MdInline], pal: &Palette) -> Vec<StyledSegment> {
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
                    color: Some(pal.lavender),
                    weight: Some(FontWeightHint::Light),
                    font_size: None,
                });
            }
            MdInline::Strikethrough(inner) => {
                let inner_text = inlines_to_plain_text(inner);
                segments.push(StyledSegment {
                    text: inner_text,
                    color: Some(pal.overlay0),
                    weight: None,
                    font_size: None,
                });
            }
            MdInline::InlineCode(code) => {
                segments.push(StyledSegment {
                    text: code.clone(),
                    color: Some(pal.green),
                    weight: None,
                    font_size: None,
                });
            }
            MdInline::Link { text, url: _ } => {
                let link_text = inlines_to_plain_text(text);
                segments.push(StyledSegment {
                    text: link_text,
                    color: Some(pal.blue),
                    weight: None,
                    font_size: None,
                });
            }
            MdInline::Image { alt, url: _ } => {
                segments.push(StyledSegment {
                    text: format!("[Image: {}]", alt),
                    color: Some(pal.peach),
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
// Pointer targets
// ============================================================================

/// Everything in the window a pointer can press, as the renderer records it.
///
/// This app drew a toolbar, a tab bar, a table of contents, a find panel and
/// three dialogs, and handled no pointer event of any kind (`known-issues.md`
/// → `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`). Every
/// variant here is recorded by the same walk that paints it ([`App::frame`],
/// through [`guitk::frame::Frame`]), so a control and the place a click finds
/// it cannot disagree.
///
/// Variants carry a tab index or a heading's source line rather than a stable
/// id. That is safe here because the frame is rebuilt from the current state
/// at the moment a press arrives, so an index names exactly what is on screen
/// then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A toolbar button.
    Toolbar(ToolbarAction),
    /// The toolbar's `»`, standing in for the buttons a narrow window has no
    /// room for.
    ToolbarMore,
    /// A document's tab.
    Tab(usize),
    /// A document tab's close button.
    CloseTab(usize),
    /// The tab bar's `»`, which lists every open document.
    TabsMore,
    /// The table of contents panel itself, which scrolls under the wheel.
    Toc,
    /// A heading in the table of contents, by its line in the source.
    Heading(usize),
    /// The find panel's background.
    FindPanel,
    /// The find box.
    FindQuery,
    /// The replace box.
    FindReplacement,
    /// Previous match.
    FindPrev,
    /// Next match.
    FindNext,
    /// The match-case switch.
    FindMatchCase,
    /// Replace the current match.
    FindReplace,
    /// Replace every match.
    FindReplaceAll,
    /// Close the find panel.
    FindClose,
    /// The source pane: gutter and text. A press places the caret, a drag
    /// selects, the wheel scrolls.
    Editor,
    /// The rendered preview, which scrolls under the wheel.
    Preview,
    /// The status bar's view-mode switch.
    ViewMode,
    /// The status bar's auto-save switch.
    Autosave,
    /// The dimmed window behind the template chooser; a press there closes it.
    TemplateBackdrop,
    /// A modal dialog's own body, between its buttons. Does nothing, and in
    /// doing nothing keeps the press off whatever the dialog covers.
    DialogBody,
    /// The dimmed window behind a dialog that must be answered. Does nothing,
    /// for the same reason.
    ModalBackdrop,
    /// A template in the chooser, by its index in [`Template::all`].
    Template(usize),
    /// The template chooser's Cancel.
    TemplateCancel,
    /// One of the answers to "the file changed on disk".
    External(ExternalChoice),
    /// One of a conflict's three resolutions in the merge review.
    ReviewPick(usize, ConflictChoice),
    /// Accept the reviewed merge.
    ReviewAccept,
    /// Back out of the review to the four answers.
    ReviewCancel,
    /// The shortcut card; a press anywhere while it is up puts it away.
    HelpCard,
}

// ============================================================================
// Rendering — toolbar
// ============================================================================

/// Toolbar button definition.
#[derive(Clone, Debug)]
pub struct ToolbarButton {
    /// Display label for the button.
    pub label: String,
    /// What the button does, in words, with its key if it has one.
    ///
    /// Shown in the status bar while the pointer is over the button, which is
    /// the only place it has ever been read: for as long as this app took no
    /// pointer input the field had no reader at all.
    pub tooltip: String,
    /// Button action identifier.
    pub action: ToolbarAction,
}

/// One slot in the toolbar: a button, or the rule between two groups.
///
/// A separator used to be a `ToolbarButton` labelled `"|"` whose action was
/// `NewFile` "because it is not clickable" -- true only while nothing in the
/// app could be clicked. The moment a pointer arrived it would have become a
/// second New button, twelve pixels wide, between Save and Bold.
#[derive(Clone, Debug)]
pub enum ToolbarItem {
    /// A button that does something.
    Button(ToolbarButton),
    /// A thin vertical rule, which does nothing and takes no hit box.
    Separator,
}

/// Actions that can be triggered from the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// Put the template chooser up.
    Templates,
    /// Apply a template.
    ApplyTemplate(usize),
}

/// The toolbar, left to right.
///
/// Every button names its key in its tooltip where it has one, and every
/// action here is reachable without the pointer too -- the buttons that could
/// not be reached any other way (Save As, the view, the contents, the
/// templates, the HTML export) were given keys in the same change that made
/// the toolbar clickable, and the rest are text a person can type.
pub fn default_toolbar() -> Vec<ToolbarItem> {
    let button = |label: &str, tooltip: &str, action| {
        ToolbarItem::Button(ToolbarButton {
            label: label.to_string(),
            tooltip: tooltip.to_string(),
            action,
        })
    };
    vec![
        button("New", "New document (Ctrl+N)", ToolbarAction::NewFile),
        button("Open", "Open a file (Ctrl+O)", ToolbarAction::OpenFile),
        button("Save", "Save (Ctrl+S)", ToolbarAction::Save),
        button(
            "Save As",
            "Save under a new name (Ctrl+Shift+S)",
            ToolbarAction::SaveAs,
        ),
        ToolbarItem::Separator,
        button("B", "Bold (Ctrl+B)", ToolbarAction::Bold),
        button("I", "Italic (Ctrl+I)", ToolbarAction::Italic),
        button(
            "H",
            "Heading, level 2; Ctrl+1 to Ctrl+6 for a level",
            ToolbarAction::Heading,
        ),
        button("Link", "Link (Ctrl+K)", ToolbarAction::Link),
        button("Img", "Image (Ctrl+Shift+I)", ToolbarAction::Image),
        button("<>", "Code block (Ctrl+Shift+K)", ToolbarAction::CodeBlock),
        button("UL", "Bulleted list item", ToolbarAction::UnorderedList),
        button("OL", "Numbered list item", ToolbarAction::OrderedList),
        button("Tbl", "Table", ToolbarAction::Table),
        button("---", "Horizontal rule", ToolbarAction::HRule),
        ToolbarItem::Separator,
        button("Undo", "Undo (Ctrl+Z)", ToolbarAction::Undo),
        button("Redo", "Redo (Ctrl+Y)", ToolbarAction::Redo),
        ToolbarItem::Separator,
        button(
            "Find",
            "Find and replace (Ctrl+H)",
            ToolbarAction::FindReplace,
        ),
        button(
            "View",
            "Editor, split or preview (Ctrl+E)",
            ToolbarAction::ToggleView,
        ),
        button(
            "ToC",
            "Table of contents (Ctrl+Shift+O)",
            ToolbarAction::ToggleToc,
        ),
        ToolbarItem::Separator,
        button(
            "Templates",
            "New document from a template (Ctrl+Shift+N)",
            ToolbarAction::Templates,
        ),
        button(
            "HTML",
            "Export as HTML (Ctrl+Shift+E)",
            ToolbarAction::ExportHtml,
        ),
    ]
}

/// Width of the `»` button that stands in for the buttons a narrow window has
/// no room for, including the gap before it.
const TOOLBAR_CHEVRON_W: f32 = 36.0;
/// Horizontal room a separator takes.
const TOOLBAR_SEPARATOR_W: f32 = 12.0;
/// Gap between two adjacent toolbar buttons.
const TOOLBAR_GAP: f32 = 4.0;

/// Where the toolbar's buttons go at a given width, and which did not fit.
///
/// One function, read by the drawing and by the overflow menu, so "the buttons
/// the window had no room for" and "the buttons the menu offers" are the same
/// list by construction. Before this there was no overflow at all: a window
/// narrower than the toolbar drew its last buttons past its own right edge,
/// where they could not be seen -- and, now that they can be clicked, could
/// not be clicked either.
#[derive(Debug, Default)]
struct ToolbarLayout {
    /// `(index into the toolbar, the box it is drawn in)` for every button
    /// that fits.
    buttons: Vec<(usize, Rect)>,
    /// The left edge of every separator that fits.
    separators: Vec<f32>,
    /// The chevron's box, and the index of the first item that did not fit.
    overflow: Option<(Rect, usize)>,
}

/// How wide a toolbar button is: its label and eight pixels either side.
fn toolbar_button_width(button: &ToolbarButton) -> f32 {
    text::width(&button.label, TOOLBAR_FONT_SIZE) + 16.0
}

fn toolbar_layout(items: &[ToolbarItem], x: f32, y: f32, width: f32) -> ToolbarLayout {
    let btn_y = y + 4.0;
    let btn_h = TOOLBAR_HEIGHT - 8.0;
    let advance = |item: &ToolbarItem| match item {
        ToolbarItem::Separator => TOOLBAR_SEPARATOR_W,
        ToolbarItem::Button(b) => toolbar_button_width(b) + TOOLBAR_GAP,
    };
    // The chevron's room is reserved only when something is going to need
    // it; a toolbar that fits whole should not lose its last button to a
    // chevron that would open an empty menu.
    let natural: f32 = 8.0 + items.iter().map(advance).sum::<f32>();
    let right = if natural <= width {
        x + width
    } else {
        x + width - TOOLBAR_CHEVRON_W
    };
    let mut layout = ToolbarLayout::default();
    let mut bx = x + 8.0;
    for (i, item) in items.iter().enumerate() {
        let w = match item {
            ToolbarItem::Separator => TOOLBAR_SEPARATOR_W,
            ToolbarItem::Button(b) => toolbar_button_width(b),
        };
        if bx + w > right {
            layout.overflow = Some((
                Rect::new(
                    x + width - TOOLBAR_CHEVRON_W + TOOLBAR_GAP,
                    btn_y,
                    TOOLBAR_CHEVRON_W - 2.0 * TOOLBAR_GAP,
                    btn_h,
                ),
                i,
            ));
            break;
        }
        match item {
            ToolbarItem::Separator => layout.separators.push(bx),
            ToolbarItem::Button(_) => layout.buttons.push((i, Rect::new(bx, btn_y, w, btn_h))),
        }
        bx += advance(item);
    }
    layout
}

/// The buttons a toolbar of `width` had no room for, in order.
fn toolbar_overflow(items: &[ToolbarItem], width: f32) -> Vec<(usize, &ToolbarButton)> {
    let Some((_, first)) = toolbar_layout(items, 0.0, 0.0, width).overflow else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .skip(first)
        .filter_map(|(i, item)| match item {
            ToolbarItem::Button(b) => Some((i, b)),
            ToolbarItem::Separator => None,
        })
        .collect()
}

/// Draw the toolbar, recording every button where it is drawn.
///
/// `hover` is what the pointer is over, so the button under it can be drawn
/// lifted: a control that does not change when the pointer reaches it gives
/// no sign that it can be pressed.
pub fn draw_toolbar(
    f: &mut Frame<Target>,
    items: &[ToolbarItem],
    pal: &Palette,
    hover: Option<Target>,
    x: f32,
    y: f32,
    width: f32,
) {
    // Toolbar background.
    pal.push_surface(
        f,
        x,
        y,
        width,
        TOOLBAR_HEIGHT,
        0.0,
        Surface::Strip(Edge::Bottom),
    );

    // Bottom border.
    f.push(RenderCommand::Line {
        x1: x,
        y1: y + TOOLBAR_HEIGHT,
        x2: x + width,
        y2: y + TOOLBAR_HEIGHT,
        color: pal.surface0,
        width: 1.0,
    });

    let layout = toolbar_layout(items, x, y, width);
    for sx in &layout.separators {
        f.push(RenderCommand::Line {
            x1: sx + 4.0,
            y1: y + 6.0,
            x2: sx + 4.0,
            y2: y + TOOLBAR_HEIGHT - 6.0,
            color: pal.surface1,
            width: 1.0,
        });
    }
    for &(i, rect) in &layout.buttons {
        let Some(ToolbarItem::Button(button)) = items.get(i) else {
            continue;
        };
        let target = Target::Toolbar(button.action);
        draw_toolbar_button(f, pal, rect, &button.label, hover == Some(target));
        f.hit(target, rect);
    }
    if let Some((rect, _)) = layout.overflow {
        draw_toolbar_button(f, pal, rect, "»", hover == Some(Target::ToolbarMore));
        f.hit(Target::ToolbarMore, rect);
    }
}

/// One toolbar button's face: a raised strip and its label.
fn draw_toolbar_button(f: &mut Frame<Target>, pal: &Palette, rect: Rect, label: &str, hot: bool) {
    let surface = if hot {
        Surface::Card
    } else {
        Surface::Strip(Edge::Bottom)
    };
    pal.push_surface(f, rect.x, rect.y, rect.w, rect.h, 4.0, surface);
    f.push(RenderCommand::Text {
        x: rect.x + 8.0,
        y: rect.y + 5.0,
        text: label.to_string(),
        font_size: TOOLBAR_FONT_SIZE,
        color: pal.text,
        font_weight: FontWeightHint::Regular,
        max_width: Some(rect.w - 16.0),
        overflow: TextOverflow::Ellipsis,
    });
}

// ============================================================================
// Rendering — tab bar
// ============================================================================

/// The narrowest a tab is squeezed to before the tab bar gives up on showing
/// every tab and offers the rest through its `»` menu.
const TAB_MIN_WIDTH: f32 = 64.0;
/// The widest a tab grows for a long name.
const TAB_MAX_WIDTH: f32 = 200.0;
/// The side of the square a tab's close button answers in.
const TAB_CLOSE_BOX: f32 = 18.0;
/// Gap between two adjacent tabs.
const TAB_GAP: f32 = 2.0;

/// One tab as the bar draws it.
#[derive(Debug)]
struct TabSlot {
    /// Which document.
    index: usize,
    /// The whole tab.
    rect: Rect,
    /// The close button inside it.
    close: Rect,
    /// The name, with ` *` when the document has unsaved changes.
    label: String,
}

/// The label a document's tab shows.
fn tab_label(doc: &Document) -> String {
    if doc.modified {
        format!("{} *", doc.name)
    } else {
        doc.name.clone()
    }
}

/// Where each tab goes, and the `»` button's box if some did not fit.
///
/// Tabs shrink before they overflow, down to [`TAB_MIN_WIDTH`], because a tab
/// bar that runs off the edge of the window hides documents that are open.
/// Past that floor the bar shows a run of tabs that always includes the one in
/// front -- a hidden active tab would leave the user unable to see which
/// document they are typing into -- and a `»` button whose menu lists every
/// open document, so each can still be brought forward with the pointer.
fn tab_layout(
    documents: &Tabs<Document>,
    x: f32,
    y: f32,
    width: f32,
) -> (Vec<TabSlot>, Option<Rect>) {
    let active = documents.active_index();
    let tab_y = y + 4.0;
    let tab_h = TAB_BAR_HEIGHT - 4.0;
    let natural: Vec<f32> = documents
        .iter()
        .enumerate()
        .map(|(i, doc)| {
            // The active tab is drawn bold, so it has to be *measured* bold —
            // sizing every tab as if it were regular made the active one the
            // one that overflowed.
            let weight = if i == active {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            };
            (text::measure(&tab_label(doc), TOOLBAR_FONT_SIZE, weight) + 24.0 + TAB_CLOSE_BOX)
                .clamp(80.0, TAB_MAX_WIDTH)
        })
        .collect();
    let room = width - 8.0;
    let span = |ws: &[f32]| ws.iter().map(|w| w + TAB_GAP).sum::<f32>();
    let mut widths = natural.clone();
    if span(&widths) > room {
        // Squeeze every tab by the same factor, but never below the floor.
        let scale = room / span(&natural);
        widths = natural
            .iter()
            .map(|w| (w * scale).max(TAB_MIN_WIDTH))
            .collect();
    }
    let overflowing = span(&widths) > room;
    let right = if overflowing {
        x + width - TOOLBAR_CHEVRON_W
    } else {
        x + width
    };
    // The first tab drawn: as far left as possible while the active one still
    // fits between it and `right`.
    let mut first = 0;
    while first < active {
        let through_active = widths.get(first..=active).map_or(0.0, span);
        if x + 4.0 + through_active <= right {
            break;
        }
        first = first.saturating_add(1);
    }
    let mut slots = Vec::new();
    let mut tab_x = x + 4.0;
    for ((index, doc), &w) in documents.iter().enumerate().zip(&widths).skip(first) {
        if tab_x + w > right {
            break;
        }
        let rect = Rect::new(tab_x, tab_y, w, tab_h);
        let close = Rect::new(
            rect.right() - TAB_CLOSE_BOX - 4.0,
            tab_y + (tab_h - TAB_CLOSE_BOX) / 2.0,
            TAB_CLOSE_BOX,
            TAB_CLOSE_BOX,
        );
        slots.push(TabSlot {
            index,
            rect,
            close,
            label: tab_label(doc),
        });
        tab_x += w + TAB_GAP;
    }
    let chevron = overflowing.then(|| {
        Rect::new(
            x + width - TOOLBAR_CHEVRON_W + TOOLBAR_GAP,
            tab_y,
            TOOLBAR_CHEVRON_W - 2.0 * TOOLBAR_GAP,
            tab_h,
        )
    });
    (slots, chevron)
}

/// Draw the tab bar, recording each tab and each close button.
pub fn draw_tab_bar(
    f: &mut Frame<Target>,
    documents: &Tabs<Document>,
    pal: &Palette,
    hover: Option<Target>,
    x: f32,
    y: f32,
    width: f32,
) {
    // Which tab is in front is the tab set's own business, so it is read
    // from it rather than passed alongside — the two could not disagree.
    let active_idx = documents.active_index();

    // Tab bar background.
    pal.push_surface(
        f,
        x,
        y,
        width,
        TAB_BAR_HEIGHT,
        0.0,
        Surface::Strip(Edge::Bottom),
    );

    let (slots, chevron) = tab_layout(documents, x, y, width);
    for slot in slots {
        let is_active = slot.index == active_idx;
        let label_weight = if is_active {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        };
        let bg_color = if is_active { pal.base } else { pal.mantle };
        let text_color = if is_active { pal.text } else { pal.subtext0 };
        let rect = slot.rect;

        // Tab background.
        f.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
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
            f.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: 2.0,
                color: pal.blue,
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Tab label, stopping short of the close button.
        f.push(RenderCommand::Text {
            x: rect.x + 8.0,
            y: rect.y + 6.0,
            text: slot.label,
            font_size: TOOLBAR_FONT_SIZE,
            color: text_color,
            font_weight: label_weight,
            max_width: Some((slot.close.x - rect.x - 12.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::Tab(slot.index), rect);

        // Close button, lit while the pointer is on it so it is plain which of
        // the two targets in the tab a press will reach.
        let close_target = Target::CloseTab(slot.index);
        if hover == Some(close_target) {
            f.push(RenderCommand::FillRect {
                x: slot.close.x,
                y: slot.close.y,
                width: slot.close.w,
                height: slot.close.h,
                color: pal.surface1,
                corner_radii: CornerRadii::all(4.0),
            });
        }
        f.push(RenderCommand::Text {
            x: slot.close.x + 5.0,
            y: slot.close.y + 2.0,
            text: "x".to_string(),
            font_size: 11.0,
            color: pal.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        f.hit(close_target, slot.close);
    }
    if let Some(rect) = chevron {
        draw_toolbar_button(f, pal, rect, "»", hover == Some(Target::TabsMore));
        f.hit(Target::TabsMore, rect);
    }
}

// ============================================================================
// Rendering — status bar
// ============================================================================

/// What the middle of the status bar is saying, most urgent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusNote<'a> {
    /// What the toolbar button under the pointer does. Transient: it lasts as
    /// long as the pointer stays, and is the only place a button's tooltip --
    /// and so its key -- is ever shown.
    Tip(&'a str),
    /// A save that failed. Sticky; see [`App::save_error`].
    Error(&'a str),
    /// What the last open, save or export did.
    Info(&'a str),
    /// Nothing to report: the word count and its companions.
    Stats,
}

/// Where the status bar's two switches are drawn, right-aligned, with the
/// saved/modified word to their right.
///
/// Returned as `(view, autosave, state)` text boxes; the drawing and the hit
/// boxes both come from here.
fn status_segments(
    view_mode: ViewMode,
    autosave_enabled: bool,
    modified: bool,
    x: f32,
    y: f32,
    width: f32,
) -> [(Rect, &'static str); 3] {
    let texts = [
        view_mode.label(),
        if autosave_enabled {
            "Auto-save ON"
        } else {
            "Auto-save OFF"
        },
        if modified { "Modified" } else { "Saved" },
    ];
    let mut right = x + width - 12.0;
    let mut out = [(Rect::EMPTY, ""); 3];
    for (slot, text) in out.iter_mut().zip(texts).rev() {
        let w = text::measure(text, STATUS_FONT_SIZE, FontWeightHint::Regular);
        // Four pixels of slack either side, so a switch is not a target only
        // as wide as its letters.
        *slot = (
            Rect::new(right - w - 4.0, y, w + 8.0, STATUS_BAR_HEIGHT),
            text,
        );
        right -= w + 16.0;
    }
    out
}

/// Draw the status bar, recording its two switches: the view mode, which
/// cycles, and auto-save, which turns on and off.
///
/// Auto-save had a field, a tick, a status-bar label and tests, and no way to
/// be turned off: `autosave_enabled` was written only by the constructor. The
/// label saying "Auto-save ON" is where a person would look to change it, so
/// it is where the change is made.
pub fn draw_status_bar(
    f: &mut Frame<Target>,
    doc: &Document,
    pal: &Palette,
    view_mode: ViewMode,
    x: f32,
    y: f32,
    width: f32,
    autosave_enabled: bool,
    note: StatusNote<'_>,
) {
    // Status bar background.
    pal.push_surface(
        f,
        x,
        y,
        width,
        STATUS_BAR_HEIGHT,
        0.0,
        Surface::Strip(Edge::Top),
    );

    // Top border.
    f.push(RenderCommand::Line {
        x1: x,
        y1: y,
        x2: x + width,
        y2: y,
        color: pal.surface0,
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
    let pos_text = format!(
        "Ln {}, Col {}",
        doc.cursor_line.saturating_add(1),
        cursor_column.saturating_add(1)
    );
    f.push(RenderCommand::Text {
        x: x + 12.0,
        y: y + 5.0,
        text: pos_text,
        font_size: STATUS_FONT_SIZE,
        color: pal.subtext0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Center: whatever `note` says, otherwise the statistics. A message
    // *displaces* the word count rather than sharing the row with it: the two
    // would collide on a narrow window, and of the two it is the word count
    // that can wait.
    let (text, color, weight) = match note {
        StatusNote::Tip(tip) => (tip.to_string(), pal.text, FontWeightHint::Regular),
        StatusNote::Error(err) => (err.to_string(), pal.ink(pal.red), FontWeightHint::Bold),
        StatusNote::Info(info) => (info.to_string(), pal.text, FontWeightHint::Regular),
        StatusNote::Stats => (
            format!(
                "{} words | {} chars | {} lines | ~{:.0} min read",
                doc.word_count(),
                doc.char_count(),
                doc.lines.len(),
                doc.reading_time_minutes()
            ),
            pal.subtext0,
            FontWeightHint::Regular,
        ),
    };
    f.push(RenderCommand::Text {
        x: text::center_x(&text, x + width / 2.0, STATUS_FONT_SIZE, weight),
        y: y + 5.0,
        text,
        font_size: STATUS_FONT_SIZE,
        color,
        font_weight: weight,
        max_width: Some(width * 0.5),
        overflow: TextOverflow::Ellipsis,
    });

    // Right side: the two switches and the saved/modified word.
    let [view, autosave, state] =
        status_segments(view_mode, autosave_enabled, doc.modified, x, y, width);
    for (rect, label) in [view, autosave, state] {
        f.push(RenderCommand::Text {
            x: rect.x + 4.0,
            y: y + 5.0,
            text: label.to_string(),
            font_size: STATUS_FONT_SIZE,
            color: pal.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
    f.hit(Target::ViewMode, view.0);
    f.hit(Target::Autosave, autosave.0);
}

// ============================================================================
// Rendering — table of contents sidebar
// ============================================================================

/// Height of one row in the table of contents.
const TOC_ROW_H: f32 = 20.0;
/// Space above the first row, where the panel's title is drawn.
const TOC_HEADER_H: f32 = 32.0;

/// How many contents rows fit in a panel `height` tall.
fn toc_rows_visible(height: f32) -> usize {
    ((height - TOC_HEADER_H) / TOC_ROW_H).max(0.0) as usize
}

/// Draw the table of contents, recording each heading as a target that jumps
/// to it.
///
/// The module doc has promised "generated from headings, clickable" since the
/// file was written. The panel had no way to be opened -- nothing wrote
/// `toc_visible` but its own toolbar button, which could not be clicked -- and
/// had it been opened, nothing on it would have answered a press. `first` is
/// the first row shown, so a document with more headings than the panel has
/// room for can be scrolled through; the rows used to stop at the panel's
/// bottom edge and the rest of the document's headings were simply not there.
///
/// `current` is the line of the heading the caret is under, which is drawn
/// marked, so the panel also says where in the document you are.
pub fn draw_toc_sidebar(
    f: &mut Frame<Target>,
    entries: &[TocEntry],
    pal: &Palette,
    first: usize,
    current: Option<usize>,
    hover: Option<Target>,
    x: f32,
    y: f32,
    height: f32,
) {
    let panel = Rect::new(x, y, TOC_SIDEBAR_WIDTH, height);
    // Sidebar background.
    f.push(RenderCommand::FillRect {
        x,
        y,
        width: TOC_SIDEBAR_WIDTH,
        height,
        color: pal.mantle,
        corner_radii: CornerRadii::ZERO,
    });
    // The whole panel answers the wheel; the rows below are recorded on top
    // of it and take presses.
    f.hit(Target::Toc, panel);

    // Right border.
    f.push(RenderCommand::Line {
        x1: x + TOC_SIDEBAR_WIDTH,
        y1: y,
        x2: x + TOC_SIDEBAR_WIDTH,
        y2: y + height,
        color: pal.surface0,
        width: 1.0,
    });

    // Title.
    f.push(RenderCommand::Text {
        x: x + 12.0,
        y: y + 8.0,
        text: "Table of Contents".to_string(),
        font_size: 12.0,
        color: pal.ink(pal.blue),
        font_weight: FontWeightHint::Bold,
        max_width: Some(TOC_SIDEBAR_WIDTH - 24.0),
        overflow: TextOverflow::Ellipsis,
    });

    // The rows are clipped to the panel below the title, so a row half past
    // the bottom neither paints over the status bar nor answers a press there.
    f.clip(Rect::new(
        x,
        y + TOC_HEADER_H,
        TOC_SIDEBAR_WIDTH,
        (height - TOC_HEADER_H).max(0.0),
    ));
    let mut entry_y = y + TOC_HEADER_H;
    for entry in entries.iter().skip(first) {
        if entry_y > y + height {
            break;
        }
        let row = Rect::new(x, entry_y - 2.0, TOC_SIDEBAR_WIDTH, TOC_ROW_H);
        let target = Target::Heading(entry.line);
        if current == Some(entry.line) || hover == Some(target) {
            f.push(RenderCommand::FillRect {
                x: row.x + 4.0,
                y: row.y,
                width: row.w - 8.0,
                height: row.h,
                color: if current == Some(entry.line) {
                    pal.surface0
                } else {
                    pal.surface1
                },
                corner_radii: CornerRadii::all(4.0),
            });
        }
        let indent = (entry.level.saturating_sub(1) as f32) * 12.0;
        let entry_color = match entry.level {
            1 => pal.blue,
            2 => pal.lavender,
            3 => pal.green,
            4 => pal.subtext1,
            _ => pal.subtext0,
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

        f.push(RenderCommand::Text {
            x: x + 12.0 + indent,
            y: entry_y,
            text: entry.text.clone(),
            font_size,
            color: entry_color,
            font_weight,
            max_width: Some(TOC_SIDEBAR_WIDTH - 24.0 - indent),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(target, row);

        entry_y += TOC_ROW_H;
    }
    f.unclip();
}

// ============================================================================
// Rendering — find/replace panel
// ============================================================================

/// Height of the find panel's two text boxes and its buttons.
const FIND_ROW_H: f32 = 22.0;

/// Draw a compact button at `(x, y)` and return the box it occupies.
///
/// `lit` is for a button that is also a switch -- match case -- and is drawn
/// in the accent while it is on, so the panel says what the search is doing
/// rather than leaving the user to remember which way they last pressed it.
fn draw_small_button(
    f: &mut Frame<Target>,
    pal: &Palette,
    x: f32,
    y: f32,
    label: &str,
    hot: bool,
    lit: bool,
) -> Rect {
    let w = text::width(label, SMALL_BUTTON_FONT_SIZE) + 16.0;
    let rect = Rect::new(x, y, w, FIND_ROW_H);
    let surface = if hot { Surface::Panel } else { Surface::Card };
    pal.push_surface(f, rect.x, rect.y, rect.w, rect.h, 4.0, surface);
    if lit {
        f.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: pal.blue,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });
    }
    f.push(RenderCommand::Text {
        x: rect.x + 8.0,
        y: rect.y + 4.0,
        text: label.to_string(),
        font_size: SMALL_BUTTON_FONT_SIZE,
        color: if lit { pal.ink(pal.blue) } else { pal.text },
        font_weight: FontWeightHint::Regular,
        max_width: Some(rect.w - 16.0),
        overflow: TextOverflow::Ellipsis,
    });
    rect
}

/// Draw one of the panel's two text boxes, with a caret when it has the
/// keyboard, and record it as a target that takes the keyboard.
fn draw_find_box(
    f: &mut Frame<Target>,
    pal: &Palette,
    rect: Rect,
    value: &str,
    focused: bool,
    target: Target,
) {
    pal.push_surface(f, rect.x, rect.y, rect.w, rect.h, 4.0, Surface::Card);
    if focused {
        f.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: pal.blue,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });
    }
    f.push(RenderCommand::Text {
        x: rect.x + 4.0,
        y: rect.y + 4.0,
        text: value.to_string(),
        font_size: 12.0,
        color: pal.text,
        font_weight: FontWeightHint::Regular,
        max_width: Some(rect.w - 8.0),
        overflow: TextOverflow::Ellipsis,
    });
    if focused {
        let caret_x = (rect.x + 4.0 + text::measure(value, 12.0, FontWeightHint::Regular))
            .min(rect.right() - 4.0);
        f.push(RenderCommand::FillRect {
            x: caret_x,
            y: rect.y + 4.0,
            width: 1.0,
            height: rect.h - 8.0,
            color: pal.text,
            corner_radii: CornerRadii::ZERO,
        });
    }
    f.hit(target, rect);
}

/// Draw the find and replace panel, recording both boxes and every button.
///
/// The boxes used to look identical whichever one the keyboard was typing
/// into, and the panel never said whether matching was case-sensitive; both
/// are drawn now. The buttons were drawn with a comment calling them "labels
/// for keys, not targets for a pointer", which was true only because nothing
/// in the app took a pointer. They keep their keys in their labels, because a
/// button that names its key teaches it.
pub fn draw_find_replace(
    f: &mut Frame<Target>,
    state: &FindReplaceState,
    pal: &Palette,
    hover: Option<Target>,
    x: f32,
    y: f32,
    width: f32,
) {
    if !state.visible {
        return;
    }

    // Panel background.
    pal.push_surface(f, x, y, width, FIND_PANEL_HEIGHT, 0.0, Surface::Card);
    // Clicks on the panel's bare background are the panel's, not the
    // document's under it.
    f.hit(Target::FindPanel, Rect::new(x, y, width, FIND_PANEL_HEIGHT));

    // Bottom border.
    f.push(RenderCommand::Line {
        x1: x,
        y1: y + FIND_PANEL_HEIGHT,
        x2: x + width,
        y2: y + FIND_PANEL_HEIGHT,
        color: pal.surface0,
        width: 1.0,
    });

    for (label, row_y) in [("Find:", y + 8.0), ("Replace:", y + 36.0)] {
        f.push(RenderCommand::Text {
            x: x + 12.0,
            y: row_y,
            text: label.to_string(),
            font_size: 12.0,
            color: pal.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    let box_w = width * 0.4;
    draw_find_box(
        f,
        pal,
        Rect::new(x + 70.0, y + 4.0, box_w, FIND_ROW_H),
        &state.query,
        !state.focus_replacement,
        Target::FindQuery,
    );
    draw_find_box(
        f,
        pal,
        Rect::new(x + 70.0, y + 32.0, box_w, FIND_ROW_H),
        &state.replacement,
        state.focus_replacement,
        Target::FindReplacement,
    );

    let buttons_x = x + 70.0 + box_w + 12.0;
    let rows: [(f32, &[(&str, Target)]); 2] = [
        (
            y + 4.0,
            &[
                ("Prev  Shift+Enter", Target::FindPrev),
                ("Next  Enter", Target::FindNext),
                ("Match case  Ctrl+I", Target::FindMatchCase),
            ],
        ),
        (
            y + 32.0,
            &[
                ("Replace  Ctrl+Enter", Target::FindReplace),
                ("Replace All  Ctrl+Shift+Enter", Target::FindReplaceAll),
                ("Close  Esc", Target::FindClose),
            ],
        ),
    ];
    let mut count_x = buttons_x;
    for (row_y, buttons) in rows {
        let mut bx = buttons_x;
        for &(label, target) in buttons {
            let lit = target == Target::FindMatchCase && state.case_sensitive;
            let rect = draw_small_button(f, pal, bx, row_y, label, hover == Some(target), lit);
            f.hit(target, rect);
            bx = rect.right() + 4.0;
        }
        if row_y < y + 20.0 {
            count_x = bx + 8.0;
        }
    }

    // Match count, after the first row's buttons.
    let match_text = if state.matches.is_empty() {
        "No matches".to_string()
    } else {
        format!(
            "{} of {} matches",
            state.current_match.saturating_add(1),
            state.matches.len()
        )
    };
    f.push(RenderCommand::Text {
        x: count_x,
        y: y + 8.0,
        text: match_text,
        font_size: 11.0,
        color: pal.subtext0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

// ============================================================================
// Rendering — template chooser dialog
// ============================================================================

/// The template chooser's box, for a window `width` by `height` at `(x, y)`.
fn template_dialog_rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    let w = 400.0;
    let h = 350.0;
    Rect::new(x + (width - w) / 2.0, y + (height - h) / 2.0, w, h)
}

/// Draw the template chooser over the window, recording each template and
/// the Cancel button.
///
/// Nothing could put this dialog up -- `template_chooser_open` was written
/// only by the key that closed it and by a test -- so five templates, their
/// content and a `ToolbarAction::ApplyTemplate` sat behind a door with no
/// handle. It is opened by the toolbar's Templates button and by
/// Ctrl+Shift+N now, and answers the pointer, the arrow keys and Enter.
///
/// `focus` is the row the keyboard is on.
pub fn draw_template_chooser(
    f: &mut Frame<Target>,
    pal: &Palette,
    focus: usize,
    hover: Option<Target>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) {
    // Overlay dimmer. It is a target too: a press on it closes the dialog,
    // and -- being recorded over everything drawn before it -- it keeps that
    // press off the document behind.
    f.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height,
        color: Color::rgba(0, 0, 0, 150),
        corner_radii: CornerRadii::ZERO,
    });
    f.hit(Target::TemplateBackdrop, Rect::new(x, y, width, height));

    let dialog = template_dialog_rect(x, y, width, height);

    // Dialog shadow.
    f.push(RenderCommand::BoxShadow {
        x: dialog.x,
        y: dialog.y,
        width: dialog.w,
        height: dialog.h,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 20.0,
        spread: 0.0,
        color: Color::rgba(0, 0, 0, 100),
        corner_radii: CornerRadii::all(8.0),
    });

    // Dialog background.
    pal.push_surface(
        f,
        dialog.x,
        dialog.y,
        dialog.w,
        dialog.h,
        8.0,
        Surface::Panel,
    );
    // The dialog's own body is not the backdrop: a press between two buttons
    // does nothing rather than closing the dialog it landed in.
    f.hit(Target::DialogBody, dialog);

    // Dialog border.
    f.push(RenderCommand::StrokeRect {
        x: dialog.x,
        y: dialog.y,
        width: dialog.w,
        height: dialog.h,
        color: pal.surface0,
        line_width: 1.0,
        corner_radii: CornerRadii::all(8.0),
    });

    // Title.
    f.push(RenderCommand::Text {
        x: dialog.x + 20.0,
        y: dialog.y + 20.0,
        text: "Choose a Template".to_string(),
        font_size: 18.0,
        color: pal.ink(pal.blue),
        font_weight: FontWeightHint::Bold,
        max_width: Some(dialog.w - 40.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Template buttons.
    let mut btn_y = dialog.y + 56.0;
    for (i, template) in Template::all().iter().enumerate() {
        let rect = Rect::new(dialog.x + 20.0, btn_y, dialog.w - 40.0, 40.0);
        let target = Target::Template(i);
        let surface = if hover == Some(target) {
            Surface::Card
        } else {
            Surface::Panel
        };
        pal.push_surface(f, rect.x, rect.y, rect.w, rect.h, 6.0, surface);
        if i == focus {
            f.push(RenderCommand::StrokeRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: pal.blue,
                line_width: 1.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }
        f.push(RenderCommand::Text {
            x: rect.x + 12.0,
            y: rect.y + 12.0,
            text: template.label().to_string(),
            font_size: 14.0,
            color: pal.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(rect.w - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(target, rect);
        btn_y += 48.0;
    }

    let cancel_w = text::width("Cancel  Esc", SMALL_BUTTON_FONT_SIZE) + 16.0;
    let cancel = draw_small_button(
        f,
        pal,
        dialog.right() - 20.0 - cancel_w,
        dialog.bottom() - 20.0 - FIND_ROW_H,
        "Cancel  Esc",
        hover == Some(Target::TemplateCancel),
        false,
    );
    f.hit(Target::TemplateCancel, cancel);
}

// ============================================================================
// Full application state
// ============================================================================

/// The full state of the markdown editor application.
pub struct App {
    /// The open/save dialog.
    ///
    /// **Both halves of this door already existed and nothing joined them.**
    /// `App::open_file` reads a file into a new tab and `Document::save_as`
    /// writes atomically and renames the tab; both have tests. What the
    /// toolbar did was `ToolbarAction::OpenFile => self.new_document()` --
    /// click Open, get a blank document -- and `ToolbarAction::SaveAs => {}`,
    /// an empty body under the comment "Would open a save dialog in a real
    /// app".
    ///
    /// The silent Save As was the dangerous one. A person who clicks it, sees
    /// no dialog and no error, and closes the editor has been told nothing and
    /// has every reason to believe the file was written under the new name.
    ///
    /// This is the shape `scripts/find-unpinned-picker-routing.py` was written
    /// for: a control, a writer, and no routing between them. Twenty apps were
    /// pinned against it; this one was not among them because it had no picker
    /// at all.
    pub picker: guitk::dialog::FilePicker,
    /// What the last open, save or export did, when it worked.
    ///
    /// Drawn in the middle of the status bar until the next key or press, so
    /// "Opened notes.md" is seen and then gets out of the way. For most of this
    /// app's life it was written and drawn nowhere -- and a Save As that
    /// *failed* reported itself only here, so it failed silently. Failures go
    /// to [`save_error`](Self::save_error) now, which is sticky and red.
    pub file_status: Option<FileNote>,
    /// What the file picker is up for, when it is up.
    ///
    /// The picker answers only "a path was chosen"; whether that path is a
    /// Save As, an HTML export or a save that should close its tab afterwards
    /// is the app's to remember.
    save_purpose: SavePurpose,
    /// Milliseconds seen since the last whole second was handed to autosave.
    ///
    /// `tick_autosave` counts in whole seconds and `Event::Tick` arrives in
    /// milliseconds, so the remainder has to live somewhere or the interval
    /// drifts long by up to a second per tick.
    tick_ms_carry: u64,
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
    /// The toolbar, left to right.
    pub toolbar: Vec<ToolbarItem>,
    /// Whether the shortcut card is up.
    pub show_help: bool,
    /// Whether auto-save is enabled.
    pub autosave_enabled: bool,
    /// Auto-save interval in seconds.
    pub autosave_interval: u64,
    /// Whether the template chooser dialog is open.
    pub template_chooser_open: bool,
    /// The template the chooser's keyboard highlight is on.
    pub template_focus: usize,
    /// The first row the table of contents shows.
    pub toc_scroll: usize,
    /// A close waiting on an answer, because what it would close has unsaved
    /// changes: the toolkit's own dialog, asked the one way every editor here
    /// asks it (`apps/unsaved`).
    pub question: Option<Question<CloseScope>>,
    /// The conflict the merge review's keys act on.
    pub review_focus: usize,
    /// A drop-down open over the window: the toolbar's or the tab bar's `»`.
    menu: Option<(MenuKind, ContextMenu)>,
    /// What the pointer is over, so it can be drawn lit and a toolbar button's
    /// tooltip can be shown.
    hover: Option<Target>,
    /// Whether a press in the source pane is being dragged, extending the
    /// selection as it goes.
    dragging: bool,
    /// Wheel remainders, one per scrolling surface, so a trackpad's fractions
    /// add up instead of vanishing (see [`guitk::wheel`]).
    editor_wheel: wheel::Accumulator,
    toc_wheel: wheel::Accumulator,
    /// The editor's own clipboard.
    ///
    /// Not the system clipboard, for the reason `apps/editor` gives: the
    /// clipboard service is reachable only over an IPC transport applications
    /// do not have yet, so cut, copy and paste work within this editor and
    /// nowhere else until it lands.
    pub clipboard: String,
    /// Whether Shift is held, as the keyboard last reported it.
    ///
    /// A pointer event carries a position and a kind and nothing else, so a
    /// Shift-press that extends the selection can only learn about Shift from
    /// the key events; `apps/editor` does the same.
    shift_held: bool,
    /// Every box the last paint recorded, for hover and the wheel; see
    /// [`App::target_at`].
    last_hits: Vec<(Target, Rect)>,
    /// Set when the window should close: every unsaved document has been
    /// saved or given up.
    quit: bool,
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
    /// The most recent save failure, or `None` if the last save worked.
    ///
    /// Sticky rather than transient. A failed write is the one message in this
    /// app the user must not miss — the buffer survives it (`Document::save`
    /// leaves `modified` set when the write fails), but only until the window
    /// closes — and there is no frame clock here to expire a toast on. It is
    /// cleared by the next save that succeeds, and by nothing else.
    pub save_error: Option<String>,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
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

/// The top-level responses to an [`ExternalChangePrompt`].
///
/// A file that was *modified* is offered the first four; a file that was
/// *deleted* is offered `KeepCurrent` and `Close`. It used to be offered
/// "Reload from disk" as well, which with no file to read cleared the prompt
/// and did nothing else -- an answer that silently was not one.
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
    /// The file is gone and the user does not want it back: close the tab.
    Close,
}

impl ExternalChoice {
    /// The answers a prompt about `change` offers, in the order drawn, with
    /// each one's label, key and consequence.
    pub fn offered(change: &DiskChange) -> &'static [(ExternalChoice, &'static str, &'static str)] {
        match change {
            DiskChange::Deleted => &[
                (
                    ExternalChoice::KeepCurrent,
                    "Keep editing  K",
                    "keep your buffer; saving writes the file again",
                ),
                (
                    ExternalChoice::Close,
                    "Close the document  C",
                    "discard the buffer; the file stays deleted",
                ),
            ],
            _ => &[
                (
                    ExternalChoice::KeepCurrent,
                    "Keep current  K",
                    "keep your buffer; overwrites disk on save",
                ),
                (
                    ExternalChoice::Reload,
                    "Reload from disk  R",
                    "discard local edits, load disk version",
                ),
                (
                    ExternalChoice::Merge,
                    "Merge  M",
                    "auto-combine both; mark conflicts inline",
                ),
                (
                    ExternalChoice::Review,
                    "Review merge…  V",
                    "resolve conflicts side-by-side",
                ),
            ],
        }
    }
}

/// What the last file operation said: done, or why not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileNote {
    /// It worked, and this is what it did.
    Done(String),
    /// It did not, and this is why.
    Failed(String),
}

/// What a pending close would close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseScope {
    /// One document's tab.
    Tab(usize),
    /// The whole window, with every document in it.
    Window,
}

/// What a path chosen in the save picker is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SavePurpose {
    /// Save the active document under the chosen name.
    Document,
    /// Save the given tab, then close it -- the "Save" answer to closing an
    /// untitled document.
    DocumentThenClose(usize),
    /// Save the given tab, then carry on closing the window.
    DocumentThenQuit(usize),
    /// Write the active document's HTML rendering.
    Html,
}

/// Which `»` a drop-down belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuKind {
    /// The toolbar's, listing the buttons that did not fit.
    Toolbar,
    /// The tab bar's, listing every open document.
    Tabs,
}

impl App {
    /// Create a new application with a single empty document.
    pub fn new(window_width: f32, window_height: f32) -> Self {
        let doc = Document::new();
        let text = doc.full_text();
        let blocks = parse_markdown(&text);
        let toc = extract_toc(&text);
        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            picker: guitk::dialog::FilePicker::new(),
            file_status: None,
            tick_ms_carry: 0,
            documents: Tabs::with(doc),
            view_mode: ViewMode::Split,
            toc_visible: false,
            find_state: FindReplaceState::new(),
            toolbar: default_toolbar(),
            show_help: false,
            autosave_enabled: true,
            autosave_interval: DEFAULT_AUTOSAVE_INTERVAL,
            template_chooser_open: false,
            template_focus: 0,
            toc_scroll: 0,
            question: None,
            review_focus: 0,
            menu: None,
            hover: None,
            dragging: false,
            editor_wheel: wheel::Accumulator::default(),
            toc_wheel: wheel::Accumulator::default(),
            clipboard: String::new(),
            shift_held: false,
            last_hits: Vec::new(),
            quit: false,
            save_purpose: SavePurpose::Document,
            cached_blocks: blocks,
            cached_toc: toc,
            window_width,
            window_height,
            external_prompt: None,
            save_error: None,
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
        // An edit that removed headings must not leave the contents scrolled
        // past its own end.
        self.toc_scroll = self.toc_scroll.min(self.cached_toc.len().saturating_sub(1));
    }

    /// A different document has come to the front: refresh what is drawn from
    /// it, and start its contents at the top rather than wherever the last
    /// document's were.
    fn document_changed(&mut self) {
        self.toc_scroll = 0;
        self.refresh_cache();
    }

    /// Create a new blank document and add it as a new tab.
    /// Route an event to the picker, and act on a chosen path.
    ///
    /// Returns `true` when the picker consumed the event, so the caller stops:
    /// an open dialog takes the keyboard, and a window that keeps typing into
    /// the document behind one is a modal that is not modal.
    pub fn picker_took(&mut self, event: &guitk::event::Event) -> bool {
        // Read before `handle`: choosing a path takes the dialog down, and a
        // flag read afterwards would be answering about a picker that is no
        // longer up.
        let saving = self.picker.is_saving();
        match self
            .picker
            .handle(event, self.window_width, self.window_height)
        {
            guitk::dialog::Picked::Chose(path) => {
                if saving {
                    self.write_chosen(&path);
                } else {
                    self.file_status = Some(match self.open_file(&path) {
                        Ok(()) => FileNote::Done(format!("Opened {}", path.display())),
                        Err(e) => {
                            FileNote::Failed(format!("Could not open {}: {e}", path.display()))
                        }
                    });
                }
                true
            }
            guitk::dialog::Picked::Cancelled => {
                // A close that was waiting on this save does not happen: the
                // user backed out of the save, so they have not agreed to lose
                // the document either.
                self.save_purpose = SavePurpose::Document;
                true
            }
            guitk::dialog::Picked::Handled => true,
            guitk::dialog::Picked::Ignored => false,
        }
    }

    /// Put the save picker up for `purpose`, suggesting `name`.
    fn ask_where_to_save(&mut self, purpose: SavePurpose, name: std::ffi::OsString) {
        self.save_purpose = purpose;
        self.picker.open_to_write(name);
    }

    /// Do whatever the save picker was put up for, at the path it chose.
    ///
    /// A failed write of the document goes to `save_error` like any other
    /// failed save -- sticky and red, because the buffer is unsaved and the
    /// user has just been told otherwise if they miss it.
    fn write_chosen(&mut self, path: &std::path::Path) {
        let purpose = std::mem::replace(&mut self.save_purpose, SavePurpose::Document);
        let tab = match purpose {
            SavePurpose::Html => {
                let html = export_html(&self.cached_blocks);
                match safeio::write_str_atomically(path, &html) {
                    Ok(()) => {
                        self.file_status =
                            Some(FileNote::Done(format!("Exported {}", path.display())));
                    }
                    Err(e) => {
                        self.file_status = Some(FileNote::Failed(format!(
                            "Could not export {}: {e}",
                            path.display()
                        )));
                    }
                }
                return;
            }
            SavePurpose::Document => self.active_doc(),
            SavePurpose::DocumentThenClose(tab) | SavePurpose::DocumentThenQuit(tab) => tab,
        };
        let Some(doc) = self.documents.get_mut(tab) else {
            return;
        };
        match doc.save_as(path) {
            Ok(()) => {
                self.save_error = None;
                self.file_status = Some(FileNote::Done(format!("Saved as {}", path.display())));
                match purpose {
                    SavePurpose::DocumentThenClose(tab) => self.close_document(tab),
                    SavePurpose::DocumentThenQuit(_) => self.continue_quitting(),
                    SavePurpose::Document | SavePurpose::Html => {}
                }
            }
            Err(e) => {
                self.save_error = Some(format!("Could not save {}: {e}", path.display()));
            }
        }
    }

    pub fn new_document(&mut self) {
        self.documents.open(Document::new());
        self.document_changed();
    }

    /// Create a new document from a template and add it as a new tab.
    pub fn new_from_template(&mut self, template: Template) {
        self.documents.open(Document::from_template(template));
        self.document_changed();
    }

    /// Open a file and add it as a new tab.
    pub fn open_file(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        let doc = Document::from_file(path)?;
        self.documents.open(doc);
        self.document_changed();
        Ok(())
    }

    /// Close the document at the given index. Closing the last one leaves a
    /// fresh empty document rather than no document at all.
    ///
    /// Closes without asking: [`request_close_tab`](Self::request_close_tab)
    /// is the door a person uses, and asks first when there is anything to
    /// lose.
    pub fn close_document(&mut self, idx: usize) {
        self.documents.close(idx);
        self.document_changed();
    }

    /// Switch to the document at the given index.
    ///
    /// Also asks whether its file changed on disk while it was behind another
    /// tab: the check reads only the document in front, so a document edited
    /// elsewhere while it was not would otherwise be shown -- and saved over --
    /// as it was.
    pub fn switch_tab(&mut self, idx: usize) {
        if idx < self.documents.count() {
            self.documents.set_active(idx);
            self.document_changed();
            self.check_external_change();
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
                    self.review_focus = 0;
                }
            }
            ExternalChoice::Close => {
                self.external_prompt = None;
                self.close_document(tab);
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
                self.picker.open_to_read();
            }
            ToolbarAction::Save => {
                self.save_active();
            }
            ToolbarAction::SaveAs => {
                let name = save_as_name(self.active_document());
                self.ask_where_to_save(SavePurpose::Document, name);
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
                set_heading(self.active_document_mut(), 2);
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
            // It rendered the document to HTML and dropped the result -- "In a
            // real app, write to a file. For now, store in memory.", above a
            // `let _ = html;`. A button that computes an answer and discards it
            // is a button that does nothing, and says nothing about it.
            ToolbarAction::ExportHtml => {
                let name = html_export_name(self.active_document());
                self.ask_where_to_save(SavePurpose::Html, name);
            }
            ToolbarAction::Templates => {
                self.template_chooser_open = true;
                self.template_focus = 0;
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

    /// Save the active document, recording any failure where the user can see
    /// it. Returns whether the write succeeded.
    ///
    /// Every user-initiated save goes through here. The write itself was
    /// always correct — `Document::save` uses `?`, so a failed write leaves
    /// `modified` set and the buffer intact — but until this existed all three
    /// call sites discarded the `Result`, so a save onto a full disk or a
    /// read-only file looked exactly like a save that worked.
    pub fn save_active(&mut self) -> bool {
        let name = self.active_document().name.clone();
        match self.active_document_mut().save() {
            Ok(()) => {
                self.save_error = None;
                true
            }
            Err(e) => {
                self.save_error = Some(format!("Could not save {name}: {e}"));
                false
            }
        }
    }

    /// Perform auto-save if conditions are met.
    ///
    /// A failure here is reported the same way a manual save's is: an autosave
    /// that fails every interval for a whole session, silently, is the worst
    /// version of this bug — the user has been told the file is being looked
    /// after and it is not.
    pub fn tick_autosave(&mut self, elapsed_seconds: u64) {
        if !self.autosave_enabled {
            return;
        }
        let mut attempted = false;
        let mut failure = None;
        for doc in self.documents.iter_mut() {
            doc.seconds_since_save = doc.seconds_since_save.saturating_add(elapsed_seconds);
            if doc.modified
                && doc.path.is_some()
                && doc.seconds_since_save >= self.autosave_interval
            {
                attempted = true;
                if let Err(e) = doc.save() {
                    // First failure wins: it is the one the user can act on,
                    // and a later tab's error would otherwise overwrite it.
                    if failure.is_none() {
                        failure = Some(format!("Auto-save failed for {}: {e}", doc.name));
                    }
                }
            }
        }
        // The notice describes the most recent save *attempt*, so a tick that
        // saved everything clears an older complaint — otherwise a transient
        // failure would leave a false alarm on screen until the next manual
        // save, which is its own way of being untrustworthy.
        if failure.is_some() {
            self.save_error = failure;
        } else if attempted {
            self.save_error = None;
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
    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`.
    pub fn render_commands(&self) -> Vec<RenderCommand> {
        self.frame(self.window_width, self.window_height)
            .into_tree()
            .commands
    }

    /// Where the source and preview panes go in the content area, for the
    /// current view mode: `(source, preview)`, either of which may be absent.
    fn content_panes(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> (Option<Rect>, Option<Rect>) {
        match self.view_mode {
            ViewMode::EditorOnly => (Some(Rect::new(x, y, width, height)), None),
            ViewMode::Split => {
                let half = width / 2.0;
                (
                    Some(Rect::new(x, y, half, height)),
                    Some(Rect::new(x + half + 1.0, y, half - 1.0, height)),
                )
            }
            ViewMode::PreviewOnly => (None, Some(Rect::new(x, y, width, height))),
        }
    }

    /// The line of the heading the caret is under, if it is under one.
    fn current_heading(&self) -> Option<usize> {
        let caret = self.active_document().cursor_line;
        self.cached_toc
            .iter()
            .take_while(|entry| entry.line <= caret)
            .last()
            .map(|entry| entry.line)
    }

    /// What the middle of the status bar should say right now.
    fn status_note(&self) -> StatusNote<'_> {
        let tip = match self.hover {
            Some(Target::Toolbar(action)) => self.toolbar.iter().find_map(|item| match item {
                ToolbarItem::Button(b) if b.action == action => Some(b.tooltip.as_str()),
                _ => None,
            }),
            Some(Target::ToolbarMore) => Some("More buttons"),
            Some(Target::TabsMore) => Some("Every open document"),
            Some(Target::ViewMode) => Some("Editor, split or preview (Ctrl+E)"),
            Some(Target::Autosave) => Some("Turn auto-save on or off"),
            _ => None,
        };
        if let Some(tip) = tip {
            return StatusNote::Tip(tip);
        }
        if let Some(err) = self.save_error.as_deref() {
            return StatusNote::Error(err);
        }
        match &self.file_status {
            Some(FileNote::Failed(msg)) => StatusNote::Error(msg),
            Some(FileNote::Done(msg)) => StatusNote::Info(msg),
            None => StatusNote::Stats,
        }
    }

    /// Draw the window at `width` by `height`, recording every control where
    /// it is drawn.
    ///
    /// This is both the renderer and the hit test: a press is resolved by
    /// drawing a frame and asking it what is at the point (see [`Target`]).
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let mut f = Frame::new(width, height);
        let pal = &self.palette;

        // Full window background.
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: pal.base,
            corner_radii: CornerRadii::ZERO,
        });

        let mut content_y: f32 = 0.0;
        draw_toolbar(
            &mut f,
            &self.toolbar,
            pal,
            self.hover,
            0.0,
            content_y,
            width,
        );
        content_y += TOOLBAR_HEIGHT;
        draw_tab_bar(
            &mut f,
            &self.documents,
            pal,
            self.hover,
            0.0,
            content_y,
            width,
        );
        content_y += TAB_BAR_HEIGHT;
        if self.find_state.visible {
            draw_find_replace(
                &mut f,
                &self.find_state,
                pal,
                self.hover,
                0.0,
                content_y,
                width,
            );
            content_y += FIND_PANEL_HEIGHT;
        }

        let status_y = height - STATUS_BAR_HEIGHT;
        let content_height = (status_y - content_y).max(0.0);
        let mut content_x = 0.0;
        let mut available_width = width;
        if self.toc_visible {
            draw_toc_sidebar(
                &mut f,
                &self.cached_toc,
                pal,
                self.toc_scroll,
                self.current_heading(),
                self.hover,
                0.0,
                content_y,
                content_height,
            );
            content_x += TOC_SIDEBAR_WIDTH;
            available_width -= TOC_SIDEBAR_WIDTH;
        }

        let doc = self.active_document();
        let (source, preview) =
            self.content_panes(content_x, content_y, available_width, content_height);
        if let Some(pane) = source {
            f.clip(pane);
            f.extend(render_editor(
                doc,
                pal,
                pane.x,
                pane.y,
                pane.w,
                pane.h,
                &self.find_state,
            ));
            f.hit(Target::Editor, pane);
            f.unclip();
        }
        if let (Some(pane), Some(_)) = (preview, source) {
            // Split divider.
            pal.push_surface(
                &mut f,
                pane.x - 2.0,
                pane.y,
                2.0,
                pane.h,
                0.0,
                Surface::Card,
            );
        }
        if let Some(pane) = preview {
            // Clipped, because a block the preview has scrolled half out of
            // view is drawn whole, and its top half would otherwise be painted
            // over the tab bar.
            f.clip(pane);
            f.extend(render_preview(
                &self.cached_blocks,
                pal,
                pane.x,
                pane.y,
                pane.w,
                pane.h,
                doc.preview_scroll,
            ));
            f.hit(Target::Preview, pane);
            f.unclip();
        }

        draw_status_bar(
            &mut f,
            doc,
            pal,
            self.view_mode,
            0.0,
            status_y,
            width,
            self.autosave_enabled,
            self.status_note(),
        );

        if self.template_chooser_open {
            draw_template_chooser(
                &mut f,
                pal,
                self.template_focus,
                self.hover,
                0.0,
                0.0,
                width,
                height,
            );
        }
        if let Some(prompt) = self.external_prompt.as_ref() {
            self.draw_external_prompt(&mut f, prompt, width, height);
        }
        if let Some((_, menu)) = self.menu.as_ref() {
            f.extend(menu.render(pal));
        }
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                pal,
                (width, height),
                0.0,
                SHORTCUTS,
                "F1 closes this",
            );
            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));
        }
        f
    }

    /// A dialog's frame: the dimmed window behind it, the box, and a title
    /// bar. Returns the box.
    ///
    /// The dimmed window is recorded as [`Target::ModalBackdrop`], which does
    /// nothing -- so a press that misses the dialog's buttons is not delivered
    /// to the document under it. These dialogs must be answered; a press
    /// outside one is not an answer.
    fn draw_dialog_frame(
        &self,
        f: &mut Frame<Target>,
        width: f32,
        height: f32,
        dialog: Rect,
        title: &str,
    ) {
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::rgba(0x11, 0x11, 0x1B, 0xB0),
            corner_radii: CornerRadii::ZERO,
        });
        f.hit(Target::ModalBackdrop, Rect::new(0.0, 0.0, width, height));
        f.push(RenderCommand::FillRect {
            x: dialog.x,
            y: dialog.y,
            width: dialog.w,
            height: dialog.h,
            color: self.palette.base,
            corner_radii: CornerRadii::all(6.0),
        });
        f.hit(Target::DialogBody, dialog);
        self.palette
            .push_surface(f, dialog.x, dialog.y, dialog.w, 32.0, 0.0, Surface::Card);
        f.push(RenderCommand::Text {
            x: dialog.x + 12.0,
            y: dialog.y + 9.0,
            text: title.to_string(),
            font_size: 14.0,
            color: self.palette.ink(self.palette.yellow),
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog.w - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// One answer in a dialog: a full-width row with a label and, to its
    /// right, what choosing it does, recorded as `target`.
    fn draw_answer_row(
        &self,
        f: &mut Frame<Target>,
        row: Rect,
        label: &str,
        hint: &str,
        target: Target,
    ) {
        let surface = if self.hover == Some(target) {
            Surface::Panel
        } else {
            Surface::Card
        };
        self.palette
            .push_surface(f, row.x, row.y, row.w, row.h, 4.0, surface);
        f.push(RenderCommand::Text {
            x: row.x + 8.0,
            y: row.y + 7.0,
            text: label.to_string(),
            font_size: 12.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(160.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.push(RenderCommand::Text {
            x: row.x + 176.0,
            y: row.y + 8.0,
            text: hint.to_string(),
            font_size: 10.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((row.w - 184.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(target, row);
    }

    /// Draw the external-change modal — either its answers or, when the user
    /// chose "review", the side-by-side merge review.
    ///
    /// Neither half could ever be answered. `check_external_change` had no
    /// caller, so the prompt was never raised; had it been, nothing would
    /// have reached `resolve_external`, and the review's footer read
    /// "[Accept]  [Cancel]" in brackets that were text. The prompt is raised
    /// when the window regains focus and when a tab comes to the front, and
    /// every answer is a key and a button now.
    fn draw_external_prompt(
        &self,
        f: &mut Frame<Target>,
        prompt: &ExternalChangePrompt,
        width: f32,
        height: f32,
    ) {
        if let Some(review) = prompt.review.as_ref() {
            self.draw_merge_review(f, prompt, review, width, height);
            return;
        }
        let offered = ExternalChoice::offered(&prompt.change);
        let dw = 480.0_f32.min(width - 40.0);
        #[allow(clippy::cast_precision_loss, reason = "at most four answers")]
        let dh = 86.0 + offered.len() as f32 * 34.0;
        let dialog = Rect::new((width - dw) / 2.0, (height - dh) / 2.0, dw, dh);

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
        self.draw_dialog_frame(f, width, height, dialog, title);
        f.push(RenderCommand::Text {
            x: dialog.x + 12.0,
            y: dialog.y + 44.0,
            text: body,
            font_size: 12.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dw - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        let mut by = dialog.y + 74.0;
        for &(choice, label, hint) in offered {
            let row = Rect::new(dialog.x + 12.0, by, dw - 24.0, 30.0);
            self.draw_answer_row(f, row, label, hint, Target::External(choice));
            by += 34.0;
        }
    }

    /// The side-by-side merge review (ours | disk), each conflict's current
    /// resolution highlighted, with three answers per conflict and Accept and
    /// Cancel. Mirrors orchestrator2's diff viewer.
    fn draw_merge_review(
        &self,
        f: &mut Frame<Target>,
        prompt: &ExternalChangePrompt,
        review: &MergeReview,
        width: f32,
        height: f32,
    ) {
        let margin = 24.0;
        let dialog = Rect::new(
            margin,
            margin,
            (width - margin * 2.0).max(0.0),
            (height - margin * 2.0).max(0.0),
        );
        let name = self
            .documents
            .get(prompt.tab)
            .map_or("file", |d| d.name.as_str());
        self.draw_dialog_frame(
            f,
            width,
            height,
            dialog,
            &format!(
                "Review merge — {name}  ({} conflict(s))",
                review.conflict_count()
            ),
        );
        let (dx, dy, dw, dh) = (dialog.x, dialog.y, dialog.w, dialog.h);

        // The conflicts scroll inside the dialog above its footer; the chips
        // on a conflict scrolled out of view are clipped away with it.
        let body = Rect::new(dx, dy + 60.0, dw, (dh - 60.0 - 44.0).max(0.0));
        let col_w = (dw - 24.0 - REVIEW_CHIPS_W) / 2.0;
        let ours_x = dx + 12.0 + REVIEW_CHIPS_W;
        let theirs_x = ours_x + col_w;
        for (x, label, color) in [
            (ours_x, name, self.palette.green),
            (theirs_x, "disk", self.palette.red),
        ] {
            f.push(RenderCommand::Text {
                x,
                y: dy + 40.0,
                text: label.to_string(),
                font_size: 11.0,
                color: self.palette.ink(color),
                font_weight: FontWeightHint::Bold,
                max_width: Some(col_w),
                overflow: TextOverflow::Ellipsis,
            });
        }

        f.clip(body);
        let mut y = body.y - review_scroll_px(review, self.review_focus, body.h);
        for (i, (_base, ours, theirs)) in review.conflicts().iter().enumerate() {
            let choice = review.choice(i).unwrap_or(ConflictChoice::Theirs);
            let chosen_ours = matches!(choice, ConflictChoice::Ours | ConflictChoice::Both);
            let chosen_theirs = matches!(choice, ConflictChoice::Theirs | ConflictChoice::Both);

            let block_h = review_block_h(ours.len(), theirs.len());

            if i == self.review_focus {
                f.push(RenderCommand::StrokeRect {
                    x: dx + 6.0,
                    y: y - 2.0,
                    width: dw - 12.0,
                    height: block_h + 4.0,
                    color: self.palette.blue,
                    line_width: 1.0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            if chosen_ours {
                f.push(RenderCommand::FillRect {
                    x: ours_x - 4.0,
                    y,
                    width: col_w,
                    height: block_h,
                    color: Color::from_hex(0x2A3A2A),
                    corner_radii: CornerRadii::ZERO,
                });
            }
            if chosen_theirs {
                f.push(RenderCommand::FillRect {
                    x: theirs_x - 4.0,
                    y,
                    width: col_w,
                    height: block_h,
                    color: Color::from_hex(0x3A2A2A),
                    corner_radii: CornerRadii::ZERO,
                });
            }
            // The three answers for this conflict, stacked at its left. Each
            // column is also a target for "take this side", since pressing
            // the text you want is what a person reaches for first.
            let mut chip_y = y;
            for (label, pick) in [
                ("Ours  O", ConflictChoice::Ours),
                ("Disk  D", ConflictChoice::Theirs),
                ("Both  B", ConflictChoice::Both),
            ] {
                let target = Target::ReviewPick(i, pick);
                let rect = draw_small_button(
                    f,
                    &self.palette,
                    dx + 10.0,
                    chip_y,
                    label,
                    self.hover == Some(target),
                    choice == pick,
                );
                f.hit(target, rect);
                chip_y += FIND_ROW_H + 2.0;
            }
            for (li, line) in ours.iter().enumerate() {
                f.push(RenderCommand::Text {
                    x: ours_x,
                    y: y + li as f32 * LINE_HEIGHT,
                    text: line.clone(),
                    font_size: 11.0,
                    color: self.palette.text,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(col_w),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            for (li, line) in theirs.iter().enumerate() {
                f.push(RenderCommand::Text {
                    x: theirs_x,
                    y: y + li as f32 * LINE_HEIGHT,
                    text: line.clone(),
                    font_size: 11.0,
                    color: self.palette.text,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(col_w),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            f.hit(
                Target::ReviewPick(i, ConflictChoice::Ours),
                Rect::new(ours_x - 4.0, y, col_w, block_h),
            );
            f.hit(
                Target::ReviewPick(i, ConflictChoice::Theirs),
                Rect::new(theirs_x - 4.0, y, col_w, block_h),
            );
            y += block_h + REVIEW_BLOCK_GAP;
        }
        f.unclip();

        f.push(RenderCommand::Text {
            x: dx + 12.0,
            y: dy + dh - 30.0,
            text: "Up and Down choose a conflict".to_string(),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((dw * 0.5).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        let mut bx = dx + dw - 12.0;
        for (label, target) in [
            ("Cancel  Esc", Target::ReviewCancel),
            ("Accept  Enter", Target::ReviewAccept),
        ] {
            bx -= text::width(label, SMALL_BUTTON_FONT_SIZE) + 16.0;
            let rect = draw_small_button(
                f,
                &self.palette,
                bx,
                dy + dh - 34.0,
                label,
                self.hover == Some(target),
                false,
            );
            f.hit(target, rect);
            bx -= 8.0;
        }
    }
}

/// Width of the column of three answer buttons at the left of each conflict
/// in the merge review.
const REVIEW_CHIPS_W: f32 = 80.0;
/// Vertical gap between two conflicts in the merge review.
const REVIEW_BLOCK_GAP: f32 = 6.0;

/// Height of one conflict in the merge review: its longer side, and never
/// less than the three answer buttons stacked beside it.
fn review_block_h(ours: usize, theirs: usize) -> f32 {
    let lines = ours.max(theirs).max(1) as f32;
    (lines * LINE_HEIGHT + 6.0).max(3.0 * (FIND_ROW_H + 2.0))
}

/// How far the merge review's conflicts are scrolled so the focused one is in
/// view, for a body `body_h` tall.
///
/// Derived from the focus rather than kept: the conflicts cannot be scrolled
/// any other way, and a position computed from the one thing that moves it
/// cannot drift from it.
fn review_scroll_px(review: &MergeReview, focus: usize, body_h: f32) -> f32 {
    let mut top = 0.0;
    let mut bottom = 0.0;
    for (i, (_base, ours, theirs)) in review.conflicts().iter().enumerate() {
        let h = review_block_h(ours.len(), theirs.len());
        if i == focus {
            bottom = top + h;
            break;
        }
        top += h + REVIEW_BLOCK_GAP;
    }
    (bottom - body_h).max(0.0).min(top)
}

// ============================================================================
// Closing, and the pointer
// ============================================================================

impl App {
    /// Close tab `idx`, or ask first if it has unsaved changes.
    ///
    /// The close button on every tab drew an `x` that nothing answered, and
    /// `close_document` -- written and tested -- had no caller at all, so a
    /// document once opened stayed open. Now it can be closed, and closing one
    /// with unsaved changes asks rather than losing them.
    pub fn request_close_tab(&mut self, idx: usize) {
        match self.documents.get(idx) {
            Some(doc) if doc.modified => {
                self.question = Some(Question::new(
                    &unsaved::message_for(&[&doc.name]),
                    "Save them before the tab closes?",
                    CloseScope::Tab(idx),
                ));
            }
            Some(_) => self.close_document(idx),
            None => {}
        }
    }

    /// The window has been asked to close. Returns whether it may go now.
    ///
    /// It used to go at once whatever it held: `CloseRequested` answered
    /// `Exit` unconditionally, so every unsaved change in every tab --
    /// including any untitled document, which auto-save never touches -- was
    /// lost without a word. Now unsaved work is asked about first.
    pub fn request_quit(&mut self) -> bool {
        // With auto-save on, every document that has a file is saved first,
        // without asking: auto-save is the user having said "save for me",
        // and closing the window is the last chance to.
        if self.autosave_enabled {
            self.save_every_titled_document();
        }
        if self.documents.iter().any(|d| d.modified) {
            let names: Vec<&str> = self
                .documents
                .iter()
                .filter(|d| d.modified)
                .map(|d| d.name.as_str())
                .collect();
            let question = Question::new(
                &unsaved::message_for(&names),
                "Save them before the window closes?",
                CloseScope::Window,
            );
            // The question replaces whatever was up: a picker or a menu left
            // open would take the keys it needs, and be drawn over it.
            self.picker.close();
            self.menu = None;
            self.show_help = false;
            self.question = Some(question);
            false
        } else {
            true
        }
    }

    /// Save every modified document that has a file, recording the first
    /// failure where the user will see it. Returns whether they all saved.
    fn save_every_titled_document(&mut self) -> bool {
        let mut failure = None;
        for doc in self.documents.iter_mut() {
            if doc.modified
                && doc.path.is_some()
                && let Err(e) = doc.save()
                && failure.is_none()
            {
                failure = Some(format!("Could not save {}: {e}", doc.name));
            }
        }
        match failure {
            Some(err) => {
                self.save_error = Some(err);
                false
            }
            None => true,
        }
    }

    /// Answer the close question put before `scope`.
    pub fn answer_close(&mut self, scope: CloseScope, choice: Choice) {
        match (scope, choice) {
            (_, Choice::Cancel) => {}
            (CloseScope::Tab(idx), Choice::Discard) => self.close_document(idx),
            (CloseScope::Tab(idx), Choice::Save) => self.save_then_close(idx),
            (CloseScope::Window, Choice::Discard) => self.quit = true,
            (CloseScope::Window, Choice::Save) => self.continue_quitting(),
        }
    }

    /// Save tab `idx` and close it -- through the picker first if it has never
    /// been saved. A save that fails leaves the tab open and says why.
    fn save_then_close(&mut self, idx: usize) {
        let Some(doc) = self.documents.get_mut(idx) else {
            return;
        };
        if doc.path.is_none() {
            let name = save_as_name(doc);
            self.ask_where_to_save(SavePurpose::DocumentThenClose(idx), name);
            return;
        }
        let name = doc.name.clone();
        match doc.save() {
            Ok(()) => {
                self.save_error = None;
                self.close_document(idx);
            }
            Err(e) => self.save_error = Some(format!("Could not save {name}: {e}")),
        }
    }

    /// Carry on closing the window: save everything that has somewhere to go,
    /// ask where to put the first thing that does not, and quit once nothing
    /// is left unsaved.
    ///
    /// Stops at the first failure. Quitting past a document whose save just
    /// failed is exactly the loss this exists to prevent.
    fn continue_quitting(&mut self) {
        if !self.save_every_titled_document() {
            return;
        }
        let unsaved = self.documents.iter().position(|d| d.modified);
        if let Some(idx) = unsaved {
            self.switch_tab(idx);
            let name = save_as_name(self.active_document());
            self.ask_where_to_save(SavePurpose::DocumentThenQuit(idx), name);
            return;
        }
        self.quit = true;
    }

    /// Whether the window should close now.
    pub fn wants_to_quit(&self) -> bool {
        self.quit
    }

    /// What is under `(x, y)` in the frame last shown.
    ///
    /// For the events that come in floods -- the pointer moving, the wheel
    /// turning -- the boxes recorded by the last paint are read rather than a
    /// frame drawn afresh: drawing one lays out the whole preview, and doing
    /// that for every pixel the pointer crosses would be most of this app's
    /// work. What the last paint recorded is also exactly what the person is
    /// looking at. A press, which is rare and must act on the current state,
    /// draws a fresh frame instead.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self
                .frame(self.window_width, self.window_height)
                .hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    /// The box the source pane was last drawn in, if it is showing.
    fn source_pane(&self) -> Option<Rect> {
        if let Some(&(_, rect)) = self
            .last_hits
            .iter()
            .rev()
            .find(|(t, _)| *t == Target::Editor)
        {
            return Some(rect);
        }
        self.frame(self.window_width, self.window_height)
            .rect_of(|t| *t == Target::Editor)
    }

    /// Route a pointer event. Returns whether anything changed.
    pub fn handle_mouse(&mut self, event: &MouseEvent) -> bool {
        // An open drop-down takes every press, and consumes it either way: a
        // press that dismisses a menu must not also land on what was behind
        // it.
        if let Some((kind, menu)) = self.menu.as_mut() {
            match event.kind {
                MouseEventKind::Press(_) => {
                    let kind = *kind;
                    let chosen = menu.handle_click(event.x, event.y);
                    self.menu = None;
                    if let Some(id) = chosen {
                        self.choose_from_menu(kind, id);
                    }
                    return true;
                }
                MouseEventKind::Move => {
                    menu.handle_mouse_move(event.x, event.y);
                    return true;
                }
                MouseEventKind::Scroll { dy, .. } => {
                    return menu.handle_scroll(event.x, event.y, dy);
                }
                _ => return false,
            }
        }
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                self.file_status = None;
                let frame = self.frame(self.window_width, self.window_height);
                let Some(target) = frame.hit_test(event.x, event.y) else {
                    return false;
                };
                if target == Target::Editor {
                    let pane = frame
                        .rect_of(|t| *t == Target::Editor)
                        .unwrap_or(Rect::EMPTY);
                    self.press_in_source(pane, event.x, event.y);
                    return true;
                }
                self.activate(target);
                true
            }
            MouseEventKind::DoubleClick(MouseButton::Left) => {
                let frame = self.frame(self.window_width, self.window_height);
                match frame.hit_test(event.x, event.y) {
                    Some(Target::Editor) => {
                        let pane = frame
                            .rect_of(|t| *t == Target::Editor)
                            .unwrap_or(Rect::EMPTY);
                        self.select_word_at(pane, event.x, event.y);
                        true
                    }
                    // Anywhere else a double press means nothing more than
                    // its first press, which has already been delivered. Not
                    // repeated: whether a host sends the second press as well
                    // as this is not settled (`guitk::dialog` notes a host may
                    // send only this), and a button that fired three times for
                    // two presses would be worse than one that fired once.
                    _ => false,
                }
            }
            MouseEventKind::Move => {
                if self.dragging {
                    let Some(pane) = self.source_pane() else {
                        self.dragging = false;
                        return false;
                    };
                    self.drag_in_source(pane, event.x, event.y);
                    return true;
                }
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return false;
                }
                self.hover = over;
                true
            }
            MouseEventKind::Release(MouseButton::Left) => {
                if !self.dragging {
                    return false;
                }
                self.dragging = false;
                // A press that never moved selected nothing; leaving an empty
                // selection behind would make the next Bold wrap nothing in
                // `****` rather than inserting its placeholder.
                let doc = self.active_document_mut();
                if doc.selection_anchor == Some((doc.cursor_line, doc.cursor_col)) {
                    doc.selection_anchor = None;
                }
                true
            }
            MouseEventKind::Leave => {
                let changed = self.hover.is_some();
                self.hover = None;
                changed
            }
            MouseEventKind::Scroll { dy, .. } => {
                let over = self.target_at(event.x, event.y);
                self.scroll(over, dy)
            }
            _ => false,
        }
    }

    /// Turn the wheel over `over` by `dy` notches. Returns whether anything
    /// moved.
    fn scroll(&mut self, over: Option<Target>, dy: f32) -> bool {
        match over {
            Some(Target::Editor) => {
                let rows = self.editor_wheel.rows(dy);
                let doc = self.active_document_mut();
                let last = doc.lines.len().saturating_sub(1);
                let before = doc.scroll_line;
                doc.scroll_line = doc.scroll_line.saturating_add_signed(rows).min(last);
                if doc.scroll_line == before {
                    return false;
                }
                // The preview follows the source, as it does for the keys.
                self.sync_scroll();
                true
            }
            Some(Target::Preview) => {
                let Some(pane) = self
                    .frame(self.window_width, self.window_height)
                    .rect_of(|t| *t == Target::Preview)
                else {
                    return false;
                };
                let limit = (preview_content_height(&self.cached_blocks, &self.palette, pane.w)
                    - pane.h)
                    .max(0.0);
                let doc = self.active_document_mut();
                let before = doc.preview_scroll;
                doc.preview_scroll =
                    (doc.preview_scroll + wheel::pixels(dy, LINE_HEIGHT)).clamp(0.0, limit);
                (doc.preview_scroll - before).abs() > f32::EPSILON
            }
            Some(Target::Toc | Target::Heading(_)) => {
                let rows = self.toc_wheel.rows(dy);
                let content_h = self.window_height
                    - TOOLBAR_HEIGHT
                    - TAB_BAR_HEIGHT
                    - STATUS_BAR_HEIGHT
                    - if self.find_state.visible {
                        FIND_PANEL_HEIGHT
                    } else {
                        0.0
                    };
                let last_first = self
                    .cached_toc
                    .len()
                    .saturating_sub(toc_rows_visible(content_h));
                let before = self.toc_scroll;
                self.toc_scroll = self.toc_scroll.saturating_add_signed(rows).min(last_first);
                self.toc_scroll != before
            }
            _ => false,
        }
    }

    /// Do what pressing `target` means.
    fn activate(&mut self, target: Target) {
        match target {
            Target::Toolbar(action) => self.handle_toolbar_action(&action),
            Target::ToolbarMore => self.open_toolbar_menu(),
            Target::Tab(idx) => self.switch_tab(idx),
            Target::CloseTab(idx) => self.request_close_tab(idx),
            Target::TabsMore => self.open_tabs_menu(),
            Target::Heading(line) => self.jump_to_heading(line),
            Target::FindQuery => self.find_state.focus_replacement = false,
            Target::FindReplacement => self.find_state.focus_replacement = true,
            Target::FindPrev => {
                self.find_state.prev_match();
                show_current_match(self);
            }
            Target::FindNext => {
                self.find_state.next_match();
                show_current_match(self);
            }
            Target::FindMatchCase => {
                self.find_state.case_sensitive = !self.find_state.case_sensitive;
                refresh_matches(self);
            }
            Target::FindReplace | Target::FindReplaceAll => {
                replace_in_active(self, target == Target::FindReplaceAll);
                refresh_matches(self);
                show_current_match(self);
            }
            Target::FindClose => self.find_state.visible = false,
            Target::ViewMode => self.view_mode = self.view_mode.next(),
            Target::Autosave => self.autosave_enabled = !self.autosave_enabled,
            Target::TemplateBackdrop | Target::TemplateCancel => {
                self.template_chooser_open = false;
            }
            Target::Template(idx) => {
                self.handle_toolbar_action(&ToolbarAction::ApplyTemplate(idx));
            }
            Target::External(choice) => self.resolve_external(choice),
            Target::ReviewPick(idx, choice) => {
                self.review_focus = idx;
                self.review_set_choice(idx, choice);
            }
            Target::ReviewAccept => self.review_accept(),
            Target::ReviewCancel => self.review_cancel(),
            Target::HelpCard => self.show_help = false,
            // Surfaces that answer the wheel or nothing at all; a press on
            // them is theirs, and stops there.
            Target::Toc
            | Target::FindPanel
            | Target::Preview
            | Target::DialogBody
            | Target::ModalBackdrop
            | Target::Editor => {}
        }
    }

    /// Bring heading `line` to the top of the source pane, caret on it.
    fn jump_to_heading(&mut self, line: usize) {
        let doc = self.active_document_mut();
        doc.selection_anchor = None;
        doc.go_to_line(line);
        doc.cursor_col = 0;
        doc.scroll_line = doc.cursor_line;
        self.sync_scroll();
    }

    /// Open the toolbar's drop-down under its `»`, listing the buttons the
    /// window had no room for.
    fn open_toolbar_menu(&mut self) {
        let items: Vec<MenuItem> = toolbar_overflow(&self.toolbar, self.window_width)
            .into_iter()
            .map(|(i, button)| MenuItem::Action {
                id: i as u64,
                label: button.tooltip.clone(),
                shortcut: None,
                icon: None,
                enabled: true,
                checked: None,
            })
            .collect();
        let at = self
            .frame(self.window_width, self.window_height)
            .rect_of(|t| *t == Target::ToolbarMore);
        self.show_menu(MenuKind::Toolbar, items, at);
    }

    /// Open the tab bar's drop-down under its `»`, listing every document.
    fn open_tabs_menu(&mut self) {
        let active = self.active_doc();
        let items: Vec<MenuItem> = self
            .documents
            .iter()
            .enumerate()
            .map(|(i, doc)| MenuItem::Action {
                id: i as u64,
                label: tab_label(doc),
                shortcut: None,
                icon: None,
                enabled: true,
                checked: Some(i == active),
            })
            .collect();
        let at = self
            .frame(self.window_width, self.window_height)
            .rect_of(|t| *t == Target::TabsMore);
        self.show_menu(MenuKind::Tabs, items, at);
    }

    /// Put a drop-down up below `under`, right-aligned to it.
    fn show_menu(&mut self, kind: MenuKind, items: Vec<MenuItem>, under: Option<Rect>) {
        if items.is_empty() {
            return;
        }
        let anchor = under.unwrap_or(Rect::new(self.window_width, 0.0, 0.0, 0.0));
        let mut menu = ContextMenu::new(items);
        // `show` keeps the menu inside the viewport, so asking for the right
        // edge of the `»` is safe even when the menu is wider than the space
        // to its right.
        menu.show(
            anchor.right(),
            anchor.bottom(),
            (self.window_width, self.window_height),
        );
        self.menu = Some((kind, menu));
    }

    /// Act on item `id` of a drop-down of `kind`.
    fn choose_from_menu(&mut self, kind: MenuKind, id: u64) {
        let Ok(idx) = usize::try_from(id) else {
            return;
        };
        match kind {
            MenuKind::Toolbar => {
                if let Some(ToolbarItem::Button(button)) = self.toolbar.get(idx) {
                    let action = button.action;
                    self.handle_toolbar_action(&action);
                }
            }
            MenuKind::Tabs => self.switch_tab(idx),
        }
    }

    /// A press in the source pane: put the caret there, or -- with Shift held
    /// -- extend the selection there, and start a drag either way.
    fn press_in_source(&mut self, pane: Rect, x: f32, y: f32) {
        let extend = self.shift_held;
        let doc = self.active_document_mut();
        let (line, col) = position_at(doc, pane, x, y);
        if extend {
            if doc.selection_anchor.is_none() {
                doc.selection_anchor = Some((doc.cursor_line, doc.cursor_col));
            }
        } else {
            // Anchored here, so the drag that may follow has somewhere to
            // extend from. Cleared again on release if nothing was dragged.
            doc.selection_anchor = Some((line, col));
        }
        doc.cursor_line = line;
        doc.cursor_col = col;
        self.dragging = true;
    }

    /// The pointer moved with the button held: extend the selection to it.
    ///
    /// Past the top or bottom of the pane the view scrolls one line per
    /// movement, so a selection can be dragged beyond what is on screen.
    fn drag_in_source(&mut self, pane: Rect, x: f32, y: f32) {
        let visible = (pane.h / LINE_HEIGHT) as usize;
        let doc = self.active_document_mut();
        if y < pane.y {
            doc.scroll_line = doc.scroll_line.saturating_sub(1);
        } else if y >= pane.bottom() {
            let last = doc.lines.len().saturating_sub(1);
            doc.scroll_line = doc.scroll_line.saturating_add(1).min(last);
        }
        let (line, col) = position_at(doc, pane, x, y);
        doc.cursor_line = line;
        doc.cursor_col = col;
        doc.ensure_cursor_visible(visible.max(1));
        self.sync_scroll();
    }

    /// Select the word under `(x, y)`: letters, digits and underscores, or the
    /// single character there if it is none of those.
    fn select_word_at(&mut self, pane: Rect, x: f32, y: f32) {
        self.dragging = false;
        let doc = self.active_document_mut();
        let (line, col) = position_at(doc, pane, x, y);
        let (start, end) = word_bounds(doc.line_text(line), col);
        doc.cursor_line = line;
        if start == end {
            doc.selection_anchor = None;
            doc.cursor_col = start;
        } else {
            doc.selection_anchor = Some((line, start));
            doc.cursor_col = end;
        }
    }
}

/// The byte range of the word around byte `col` of `line`.
///
/// A word is a run of letters, digits and underscores. Off a word, the range
/// is the single character at `col`, so a double press on punctuation still
/// selects what it landed on; at the end of a line it is empty.
fn word_bounds(line: &str, col: usize) -> (usize, usize) {
    let col = clamp_col(line, col);
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let Some(here) = line.get(col..).and_then(|tail| tail.chars().next()) else {
        return (col, col);
    };
    if !is_word(here) {
        return (col, col.saturating_add(here.len_utf8()));
    }
    let start = line
        .get(..col)
        .and_then(|head| {
            head.char_indices()
                .rev()
                .take_while(|&(_, c)| is_word(c))
                .last()
        })
        .map_or(col, |(i, _)| i);
    let end = line
        .get(col..)
        .and_then(|tail| tail.char_indices().find(|&(_, c)| !is_word(c)))
        .map_or(line.len(), |(i, _)| col.saturating_add(i));
    (start, end)
}

/// The column of `line` nearest to `x`, for a line whose text starts at
/// `text_x`.
///
/// The inverse of [`col_x`], and built on it rather than beside it: it asks
/// `col_x` where each character boundary is drawn and picks the nearest, so a
/// press lands on the boundary the caret would be *drawn* at, whatever the
/// characters' widths. A binary search, because `col_x` only grows with the
/// column.
fn col_at(line: &str, x: f32, text_x: f32) -> usize {
    let bounds: Vec<usize> = line
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(line.len()))
        .collect();
    let past = bounds.partition_point(|&b| col_x(line, b, text_x) <= x);
    let before = past.checked_sub(1).and_then(|i| bounds.get(i)).copied();
    let after = bounds.get(past).copied();
    match (before, after) {
        (Some(lo), Some(hi)) => {
            if x - col_x(line, lo, text_x) <= col_x(line, hi, text_x) - x {
                lo
            } else {
                hi
            }
        }
        (Some(lo), None) => lo,
        (None, Some(hi)) => hi,
        (None, None) => 0,
    }
}

/// The document position under `(x, y)` in a source pane drawn in `pane`.
///
/// Above or below the pane it answers the line just off that edge, clamped
/// to the document, which is what a drag past the edge needs.
fn position_at(doc: &Document, pane: Rect, x: f32, y: f32) -> (usize, usize) {
    let row = ((y - pane.y) / LINE_HEIGHT).floor() as isize;
    let last = doc.lines.len().saturating_sub(1);
    let line = doc.scroll_line.saturating_add_signed(row).min(last);
    let text_x = pane.x + GUTTER_WIDTH + EDITOR_PADDING;
    (line, col_at(doc.line_text(line), x, text_x))
}

/// How tall the preview of `blocks` is when laid out `width` wide: what the
/// wheel may scroll through.
fn preview_content_height(blocks: &[MdBlock], pal: &Palette, width: f32) -> f32 {
    // A zero-height viewport: every element is laid out and none is drawn,
    // so this measures without building a frame's worth of commands.
    let mut ctx = PreviewContext::new(
        *pal,
        PREVIEW_PADDING,
        0.0,
        width - PREVIEW_PADDING * 2.0,
        0.0,
        0.0,
    );
    for block in blocks {
        render_block_preview(block, &mut ctx);
        ctx.add_spacing(8.0);
    }
    ctx.y + PREVIEW_PADDING * 2.0
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
/// Recompute the matches against the document as it stands.
///
/// The lines are moved out and back rather than cloned: `find_all` needs them
/// by reference while `find_state` is borrowed mutably, and both live on
/// `App`.
fn refresh_matches(app: &mut App) {
    let lines = std::mem::take(&mut app.active_document_mut().lines);
    app.find_state.find_all(&lines);
    app.active_document_mut().lines = lines;
}

/// Put the cursor on the match the panel is pointing at.
///
/// Without this the panel counts matches it will not show you, which is the
/// half of "find" that is not searching. It is also the only caller
/// `go_to_line` has ever had.
fn show_current_match(app: &mut App) {
    let Some((line, _, _)) = app.find_state.current_match_info() else {
        return;
    };
    app.active_document_mut().go_to_line(line);
    let visible = compute_visible_lines(app);
    app.active_document_mut().ensure_cursor_visible(visible);
}

/// Replace the current match, or every match.
fn replace_in_active(app: &mut App, all: bool) {
    let mut lines = std::mem::take(&mut app.active_document_mut().lines);
    let changed = if all {
        app.find_state.replace_all(&mut lines) > 0
    } else {
        app.find_state.replace_current(&mut lines)
    };
    app.active_document_mut().lines = lines;
    if changed {
        app.active_document_mut().modified = true;
        app.refresh_cache();
    }
}

/// Keys while the find and replace panel is open.
///
/// The panel takes the keyboard for the reason every text mode in this tree
/// does: a query containing `s` must not save the document behind it. Returns
/// whether the key was used here.
///
/// Before this, **nothing in production wrote `query` or `replacement`**, so
/// the panel opened with two boxes that could never hold a character and three
/// buttons that could not be clicked -- this app handles no pointer events at
/// all. `replace_current`, `replace_all`, `next_match` and `prev_match` each
/// appeared exactly once in the crate: their own definition.
fn handle_find_key(app: &mut App, key: Key, modifiers: Modifiers) -> bool {
    if modifiers.ctrl {
        return match key {
            // The keys that opened it close it again.
            Key::Char('h' | 'H' | 'f' | 'F') => {
                app.find_state.visible = false;
                true
            }
            // Match case. `case_sensitive` was `false` at construction with no
            // writer anywhere, so `Case::sensitive(self.case_sensitive)` was
            // handed `false` for every search this program has ever run --
            // the find could not be made to match case. Found by
            // `scripts/frozen-flag-survey.py`.
            //
            // `Ctrl+I` inside the find bar, which is what `apps/hexeditor` and
            // `apps/jsonviewer` use for the same question. The global `Ctrl+I`
            // is italic and lives in a different handler, so the two do not
            // collide.
            Key::Char('i' | 'I') => {
                app.find_state.case_sensitive = !app.find_state.case_sensitive;
                refresh_matches(app);
                true
            }
            // Replace, and replace every match. On the buttons the panel
            // draws, which are labels for these keys rather than targets for
            // a pointer that does not exist.
            Key::Enter => {
                replace_in_active(app, modifiers.shift);
                refresh_matches(app);
                show_current_match(app);
                true
            }
            _ => false,
        };
    }
    match key {
        Key::Escape => {
            app.find_state.visible = false;
            true
        }
        Key::Tab => {
            app.find_state.focus_replacement = !app.find_state.focus_replacement;
            true
        }
        Key::Enter => {
            if modifiers.shift {
                app.find_state.prev_match();
            } else {
                app.find_state.next_match();
            }
            show_current_match(app);
            true
        }
        Key::Backspace => {
            if app.find_state.focus_replacement {
                app.find_state.replacement.pop();
            } else {
                app.find_state.query.pop();
                refresh_matches(app);
            }
            true
        }
        Key::Char(c) => {
            if app.find_state.focus_replacement {
                app.find_state.replacement.push(c);
            } else {
                app.find_state.query.push(c);
                // On every keystroke rather than on Enter: the panel reports a
                // match count, and a count that lags what is typed is worse
                // than none.
                refresh_matches(app);
                show_current_match(app);
            }
            true
        }
        _ => false,
    }
}

/// The keys this program answers, as a reader sees them.
///
/// Every one of these was already written down -- in `ToolbarButton::tooltip`,
/// which for most of this app's life nothing read and no pointer could
/// summon, because the app handled no mouse event of any kind. A shortcut
/// named only in a field with no reader is not named.
///
/// `Ctrl+O` was in that list too and was bound to nothing at all. It is bound
/// here, because the honest repair for an advertised key is to make it work.
/// The rows from `Ctrl+Shift+N` to `Ctrl+Shift+I`, and the selection and
/// clipboard rows, arrived with the pointer layer: each names something the
/// app offered and nothing reached.
///
/// **No `?` row.** `Key::Char(ch)` with no Ctrl inserts the character into the
/// document, so binding `?` would cost a question mark in a markdown file to
/// buy a list `F1` already opens. Same reason `apps/ebook` does not offer it.
const SHORTCUTS: &[(&str, &str)] = &[
    ("F1", "This list"),
    ("Ctrl+N", "New document"),
    ("Ctrl+Shift+N", "New document from a template"),
    ("Ctrl+O", "Open a file"),
    ("Ctrl+S", "Save"),
    ("Ctrl+Shift+S", "Save under a new name"),
    ("Ctrl+Shift+E", "Export as HTML"),
    ("Ctrl+W", "Close the document"),
    ("Ctrl+Tab", "Next document"),
    ("Ctrl+Shift+Tab", "Previous document"),
    ("Ctrl+E", "Editor, split or preview"),
    ("Ctrl+Shift+O", "Table of contents"),
    ("Ctrl+B", "Bold"),
    ("Ctrl+I", "Italic; match case in the find panel"),
    ("Ctrl+K", "Link"),
    ("Ctrl+Shift+K", "Code block"),
    ("Ctrl+Shift+I", "Image"),
    ("Ctrl+1", "Heading 1; Ctrl+2 to Ctrl+6 for the rest"),
    ("Ctrl+A", "Select everything"),
    (
        "Shift+Right",
        "Extend the selection; any arrow, Home or End",
    ),
    ("Ctrl+C", "Copy"),
    ("Ctrl+X", "Cut"),
    ("Ctrl+V", "Paste"),
    ("Ctrl+Z", "Undo"),
    ("Ctrl+Y", "Redo"),
    ("Ctrl+Shift+Z", "Redo"),
    ("Ctrl+F", "Find"),
    ("Ctrl+H", "Find and replace"),
    ("Tab", "Indent; switch boxes in the find panel"),
    ("Enter", "New line; next match in the find panel"),
    ("Shift+Enter", "Previous match, in the find panel"),
    ("Ctrl+Enter", "Replace, in the find panel"),
    ("Ctrl+Shift+Enter", "Replace all, in the find panel"),
    ("Esc", "Close the find panel"),
];

/// Keys while the "file changed on disk" dialog is up -- its answers, or the
/// merge review's. Modal for the same reason as the close dialog.
///
/// `Esc` on the answers puts the question off rather than answering it: the
/// dialog comes back the next time the window regains focus, because the file
/// on disk still differs from the buffer. On the review it goes back to the
/// answers, which is what the review's own Cancel does.
fn handle_external_prompt_key(app: &mut App, key: Key) -> bool {
    let Some(prompt) = app.external_prompt.as_ref() else {
        return false;
    };
    if let Some(review) = prompt.review.as_ref() {
        let last = review.conflicts().len().saturating_sub(1);
        match key {
            Key::Up => app.review_focus = app.review_focus.saturating_sub(1),
            Key::Down => app.review_focus = app.review_focus.saturating_add(1).min(last),
            Key::Char('o' | 'O') => app.review_set_choice(app.review_focus, ConflictChoice::Ours),
            Key::Char('d' | 'D') => {
                app.review_set_choice(app.review_focus, ConflictChoice::Theirs);
            }
            Key::Char('b' | 'B') => app.review_set_choice(app.review_focus, ConflictChoice::Both),
            Key::Enter => app.review_accept(),
            Key::Escape => app.review_cancel(),
            _ => {}
        }
        return true;
    }
    let choice = match key {
        Key::Char('k' | 'K') => ExternalChoice::KeepCurrent,
        Key::Char('r' | 'R') => ExternalChoice::Reload,
        Key::Char('m' | 'M') => ExternalChoice::Merge,
        Key::Char('v' | 'V') => ExternalChoice::Review,
        Key::Char('c' | 'C') => ExternalChoice::Close,
        Key::Escape => {
            app.dismiss_external();
            return true;
        }
        _ => return true,
    };
    // Only the answers this prompt offers: `R` on a deleted file has no disk
    // version to reload, and doing nothing silently is what that answer used
    // to do.
    if ExternalChoice::offered(&prompt.change)
        .iter()
        .any(|(offered, ..)| *offered == choice)
    {
        app.resolve_external(choice);
    }
    true
}

/// Keys while the template chooser is up: arrows and Enter, a digit for a
/// template by number, `Esc` to close. Modal, like the rest.
fn handle_template_key(app: &mut App, key: Key) -> bool {
    let last = Template::all().len().saturating_sub(1);
    match key {
        Key::Up => app.template_focus = app.template_focus.saturating_sub(1),
        Key::Down => app.template_focus = app.template_focus.saturating_add(1).min(last),
        Key::Enter => {
            let focus = app.template_focus;
            app.handle_toolbar_action(&ToolbarAction::ApplyTemplate(focus));
        }
        Key::Char(c @ '1'..='9') => {
            let chosen = c
                .to_digit(10)
                .and_then(|d| usize::try_from(d).ok())
                .and_then(|d| d.checked_sub(1));
            if let Some(idx) = chosen.filter(|&i| i <= last) {
                app.handle_toolbar_action(&ToolbarAction::ApplyTemplate(idx));
            }
        }
        Key::Escape => app.template_chooser_open = false,
        _ => {}
    }
    true
}

/// Bring the next or the previous tab to the front, wrapping at either end.
fn cycle_tab(app: &mut App, forward: bool) {
    let count = app.documents.count();
    if count < 2 {
        return;
    }
    let current = app.active_doc();
    let last = count.saturating_sub(1);
    let next = if forward {
        if current >= last {
            0
        } else {
            current.saturating_add(1)
        }
    } else {
        current.checked_sub(1).unwrap_or(last)
    };
    app.switch_tab(next);
}

/// Copy the selection to the editor's clipboard. Returns whether there was a
/// selection to copy -- Ctrl+C with nothing selected leaves the clipboard
/// alone rather than emptying it.
fn copy_selection(app: &mut App) -> bool {
    match app.active_document().selected_text() {
        Some(text) => {
            app.clipboard = text;
            true
        }
        None => false,
    }
}

/// Answer a Ctrl chord. `None` when it is not one of this app's, so the caller
/// can carry on looking.
fn handle_ctrl_key(app: &mut App, key: Key, modifiers: Modifiers) -> Option<bool> {
    let shift = modifiers.shift;
    match key {
        Key::Char('n' | 'N') if shift => {
            app.handle_toolbar_action(&ToolbarAction::Templates);
        }
        Key::Char('n' | 'N') => app.new_document(),
        Key::Char('o' | 'O') if shift => app.toc_visible = !app.toc_visible,
        // Advertised by the Open button's tooltip since that toolbar was
        // written, and answered by nothing until this was.
        Key::Char('o' | 'O') => app.picker.open_to_read(),
        Key::Char('s' | 'S') if shift => app.handle_toolbar_action(&ToolbarAction::SaveAs),
        Key::Char('s' | 'S') => {
            app.save_active();
        }
        Key::Char('e' | 'E') if shift => app.handle_toolbar_action(&ToolbarAction::ExportHtml),
        Key::Char('e' | 'E') => app.view_mode = app.view_mode.next(),
        Key::Char('w' | 'W') => app.request_close_tab(app.active_doc()),
        Key::Tab => cycle_tab(app, !shift),
        Key::PageDown => cycle_tab(app, true),
        Key::PageUp => cycle_tab(app, false),
        Key::Char('z' | 'Z') => {
            if shift {
                app.active_document_mut().redo();
            } else {
                app.active_document_mut().undo();
            }
            app.refresh_cache();
        }
        Key::Char('y' | 'Y') => {
            app.active_document_mut().redo();
            app.refresh_cache();
        }
        Key::Char('b' | 'B') => {
            insert_bold(app.active_document_mut());
            app.refresh_cache();
        }
        Key::Char('i' | 'I') if shift => {
            insert_image(app.active_document_mut());
            app.refresh_cache();
        }
        Key::Char('i' | 'I') => {
            insert_italic(app.active_document_mut());
            app.refresh_cache();
        }
        Key::Char('k' | 'K') => {
            if shift {
                insert_code_block(app.active_document_mut());
            } else {
                insert_link(app.active_document_mut());
            }
            app.refresh_cache();
        }
        Key::Char(c @ '1'..='6') => {
            let level = c
                .to_digit(10)
                .and_then(|d| u8::try_from(d).ok())
                .unwrap_or(1);
            set_heading(app.active_document_mut(), level);
            app.refresh_cache();
        }
        Key::Char('a' | 'A') => app.active_document_mut().select_all(),
        Key::Char('c' | 'C') => {
            copy_selection(app);
        }
        Key::Char('x' | 'X') => {
            if copy_selection(app) {
                app.active_document_mut().delete_selection();
                app.refresh_cache();
            }
        }
        Key::Char('v' | 'V') => {
            let text = app.clipboard.clone();
            let doc = app.active_document_mut();
            doc.delete_selection();
            doc.insert_text(&text);
            app.refresh_cache();
        }
        Key::Char('h' | 'H') => app.find_state.visible = !app.find_state.visible,
        Key::Char('f' | 'F') => app.find_state.visible = true,
        _ => return None,
    }
    Some(true)
}

/// Move the caret with `step`, extending the selection when Shift is held and
/// dropping it when not.
///
/// Shift+arrow is how a keyboard selects, and nothing here did it:
/// `selection_anchor` was cleared in four places and set in none, so every
/// branch of Bold, Italic and Link that wraps a selection was unreachable, and
/// they could only ever insert their placeholders.
fn move_caret(app: &mut App, extend: bool, step: impl FnOnce(&mut Document)) {
    let doc = app.active_document_mut();
    if extend {
        if doc.selection_anchor.is_none() {
            doc.selection_anchor = Some((doc.cursor_line, doc.cursor_col));
        }
    } else {
        doc.selection_anchor = None;
    }
    step(doc);
    if doc.selection_anchor == Some((doc.cursor_line, doc.cursor_col)) {
        doc.selection_anchor = None;
    }
    let visible = compute_visible_lines(app);
    app.active_document_mut().ensure_cursor_visible(visible);
}

/// Answer a keystroke. Reports whether this program did anything with it.
///
/// The return value is not decoration. Without it "did the app answer this
/// key?" can only be inferred from some field moving, and picking that field
/// is the mistake this tree has made six times: the observable whose name
/// matches the verb is usually not the one the action writes.
pub fn handle_key(app: &mut App, key: Key, modifiers: Modifiers) -> bool {
    // "Opened notes.md" has been seen once the person does something else.
    app.file_status = None;
    // Above the find panel's branch, which returns before everything below
    // it. Placed after, the card could be raised from the editor and then not
    // dismissed while the panel was open -- a list with no way out, which is
    // the third app in this tree where this placement was the difference.
    if key == Key::Function(1) {
        app.show_help = !app.show_help;
        return true;
    }
    if app.show_help {
        // Modal. Letting keys through would edit a document the reader cannot
        // see, and typing is what most keys do here.
        if matches!(key, Key::Escape | Key::Enter) {
            app.show_help = false;
        }
        return true;
    }
    // The dialogs, each modal, most urgent first.
    if app.external_prompt.is_some() {
        return handle_external_prompt_key(app, key);
    }
    if app.template_chooser_open {
        return handle_template_key(app, key);
    }

    // The find panel takes the keyboard while it is open.
    if app.find_state.visible && handle_find_key(app, key, modifiers) {
        return true;
    }

    // Global shortcuts.
    if modifiers.ctrl
        && let Some(answered) = handle_ctrl_key(app, key, modifiers)
    {
        app.sync_scroll();
        return answered;
    }

    // Escape closes the find panel.
    if key == Key::Escape && app.find_state.visible {
        app.find_state.visible = false;
        return true;
    }

    let extend = modifiers.shift;
    let answered = match key {
        // Left and Right on a selection without Shift land on its near and
        // far ends rather than moving one character from the caret.
        Key::Left | Key::Right if !extend && app.active_document().has_selection() => {
            if let Some((start, end)) = app.active_document().selection_bounds() {
                let doc = app.active_document_mut();
                let (line, col) = if key == Key::Left { start } else { end };
                doc.selection_anchor = None;
                doc.cursor_line = line;
                doc.cursor_col = col;
            }
            true
        }
        Key::Up => {
            move_caret(app, extend, Document::move_cursor_up);
            true
        }
        Key::Down => {
            move_caret(app, extend, Document::move_cursor_down);
            true
        }
        Key::Left => {
            move_caret(app, extend, Document::move_cursor_left);
            true
        }
        Key::Right => {
            move_caret(app, extend, Document::move_cursor_right);
            true
        }
        Key::Home => {
            move_caret(app, extend, Document::move_cursor_home);
            true
        }
        Key::End => {
            move_caret(app, extend, Document::move_cursor_end);
            true
        }
        Key::PageUp | Key::PageDown => {
            let visible = compute_visible_lines(app);
            let down = key == Key::PageDown;
            move_caret(app, extend, |doc| {
                for _ in 0..visible {
                    if down {
                        doc.move_cursor_down();
                    } else {
                        doc.move_cursor_up();
                    }
                }
            });
            true
        }
        // Everything that types replaces the selection, as it does in every
        // editor a person has used; an anchor left behind would have made the
        // next Bold wrap text the person had already typed over.
        Key::Enter => {
            let doc = app.active_document_mut();
            doc.delete_selection();
            doc.insert_newline();
            app.refresh_cache();
            true
        }
        Key::Backspace | Key::Delete => {
            let doc = app.active_document_mut();
            if doc.has_selection() {
                doc.delete_selection();
            } else if key == Key::Backspace {
                doc.selection_anchor = None;
                doc.delete_backward();
            } else {
                doc.selection_anchor = None;
                doc.delete_forward();
            }
            app.refresh_cache();
            true
        }
        Key::Tab => {
            let doc = app.active_document_mut();
            doc.delete_selection();
            doc.insert_text("    ");
            app.refresh_cache();
            true
        }
        Key::Char(ch) if !modifiers.ctrl && !modifiers.alt => {
            let doc = app.active_document_mut();
            doc.delete_selection();
            doc.insert_char(ch);
            app.refresh_cache();
            true
        }
        _ => false,
    };

    // Sync scroll after any edit.
    app.sync_scroll();
    answered
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

// ============================================================================
// Compositor wiring
// ============================================================================

/// Translate a toolkit key event into this app's own vocabulary.
///
/// **The app declares its own `Key` and `Modifiers`**, a complete parallel
/// input vocabulary beside `guitk::event`'s and the wire's — the same
/// three-enums-for-one-concept the compositor client was rewritten to remove
/// (`known-issues.md` → `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`, step (d½)).
/// They are not collapsed here because `handle_key` and its several hundred
/// lines of shortcut matching are the app's own logic and converting them is a
/// separate change; this is the one place the two vocabularies meet, which is
/// what makes that change possible later.
///
/// `text` is consulted before the key code for `Char`, so a shifted key and a
/// dead-key sequence arrive correctly spelled — asking the enum for a letter
/// cannot do that, and is what would make this the file to edit when
/// `TD-ONLY-ONE-KEYBOARD-LAYOUT` is closed.
fn translate_key(ev: &guitk::event::KeyEvent) -> Option<(Key, Modifiers)> {
    use guitk::event::Key as GKey;
    let key = match ev.key {
        GKey::Enter => Key::Enter,
        GKey::Backspace => Key::Backspace,
        GKey::Delete => Key::Delete,
        GKey::Up => Key::Up,
        GKey::Down => Key::Down,
        GKey::Left => Key::Left,
        GKey::Right => Key::Right,
        GKey::Home => Key::Home,
        GKey::End => Key::End,
        GKey::PageUp => Key::PageUp,
        GKey::PageDown => Key::PageDown,
        GKey::Tab => Key::Tab,
        GKey::Escape => Key::Escape,
        GKey::F1 => Key::Function(1),
        GKey::F2 => Key::Function(2),
        GKey::F3 => Key::Function(3),
        GKey::F4 => Key::Function(4),
        GKey::F5 => Key::Function(5),
        GKey::F6 => Key::Function(6),
        GKey::F7 => Key::Function(7),
        GKey::F8 => Key::Function(8),
        GKey::F9 => Key::Function(9),
        GKey::F10 => Key::Function(10),
        GKey::F11 => Key::Function(11),
        GKey::F12 => Key::Function(12),
        _ => Key::Char(printable(ev)?),
    };
    Some((
        key,
        Modifiers {
            ctrl: ev.modifiers.ctrl,
            shift: ev.modifiers.shift,
            alt: ev.modifiers.alt,
        },
    ))
}

/// The character a keystroke produced, falling back to the letter its key
/// names when a chord suppressed the text.
///
/// A chord produces no text on most layouts, so `Ctrl+S` would otherwise have
/// no character at all and every shortcut in the app would be unreachable.
fn printable(ev: &guitk::event::KeyEvent) -> Option<char> {
    use guitk::event::Key as GKey;
    if let Some(c) = ev.text.chars().next() {
        return Some(c);
    }
    let c = match ev.key {
        GKey::A => 'a',
        GKey::B => 'b',
        GKey::C => 'c',
        GKey::D => 'd',
        GKey::E => 'e',
        GKey::F => 'f',
        GKey::G => 'g',
        GKey::H => 'h',
        GKey::I => 'i',
        GKey::J => 'j',
        GKey::K => 'k',
        GKey::L => 'l',
        GKey::M => 'm',
        GKey::N => 'n',
        GKey::O => 'o',
        GKey::P => 'p',
        GKey::Q => 'q',
        GKey::R => 'r',
        GKey::S => 's',
        GKey::T => 't',
        GKey::U => 'u',
        GKey::V => 'v',
        GKey::W => 'w',
        GKey::X => 'x',
        GKey::Y => 'y',
        GKey::Z => 'z',
        GKey::Space => ' ',
        // Digits too, for Ctrl+1 to Ctrl+6: a chord that suppressed the text
        // would otherwise arrive with no character and the heading keys would
        // be dropped here, before anything could answer them.
        GKey::Num0 => '0',
        GKey::Num1 => '1',
        GKey::Num2 => '2',
        GKey::Num3 => '3',
        GKey::Num4 => '4',
        GKey::Num5 => '5',
        GKey::Num6 => '6',
        GKey::Num7 => '7',
        GKey::Num8 => '8',
        GKey::Num9 => '9',
        _ => return None,
    };
    Some(c)
}

impl oswindow::app::App for App {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        "Markdown Editor".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (self.window_width as u32, self.window_height as u32)
    }

    /// A clock only while autosave is on.
    ///
    /// `tick_autosave` returns immediately when it is off, so an unconditional
    /// interval would wake the machine once a second to do nothing for as long
    /// as the window is open. Returning `None` while it is *on* would ship an
    /// autosave that never fires, with its tests still green — which is the
    /// half `known-issues.md` lesson 47 is about.
    ///
    /// One second, because `tick_autosave` counts in whole seconds and a finer
    /// clock would deliver fractions it discards.
    fn tick_interval(&self) -> Option<std::time::Duration> {
        self.autosave_enabled
            .then(|| std::time::Duration::from_secs(1))
    }

    fn on_event(&mut self, event: &guitk::event::Event) -> oswindow::app::Response {
        use guitk::event::Event as GEvent;
        use oswindow::app::Response;

        // **Before everything else.** An open dialog takes the keyboard, and a
        // window that goes on typing into the document behind one is a modal
        // that is not modal -- the half of this that `is_open()` tests cannot
        // see, because the key handler is what opens the dialog.
        //
        // `Resize` is let through first so the picker is laid out against the
        // size the compositor actually gave us; everything else stops here
        // while a dialog is up.
        // The close question first of all, while it is up: every key and
        // click is its, since typing into a document being closed is typing
        // into one the user has already decided about. Each is a redraw --
        // focus and hover move inside it.
        if let Some(question) = self.question.as_mut()
            && matches!(event, GEvent::Key(_) | GEvent::Mouse(_))
        {
            if let Some(choice) = question.handle(event) {
                let scope = question.pending();
                self.question = None;
                self.answer_close(scope, choice);
            }
            return if self.quit {
                Response::Exit
            } else {
                Response::Redraw
            };
        }
        if !matches!(event, GEvent::Resize { .. }) && self.picker_took(event) {
            // The picker's last answer may have been the save that lets a
            // closing window go.
            if self.quit {
                return Response::Exit;
            }
            return Response::Redraw;
        }

        let response = match event {
            // Not `Exit` outright any more: see `App::request_quit`. And
            // `KeepOpen`, not `Redraw`, while it asks: any other answer to a
            // close request still closes the window, so the question would be
            // drawn into a window already gone.
            GEvent::CloseRequested => {
                if self.request_quit() {
                    Response::Exit
                } else {
                    Response::KeepOpen
                }
            }
            GEvent::Resize { width, height } => {
                #[allow(clippy::cast_precision_loss)]
                {
                    self.window_width = *width as f32;
                    self.window_height = *height as f32;
                }
                Response::Redraw
            }
            // Coming back to the window is when another program is most
            // likely to have written the file -- a build, a formatter, a
            // `git checkout` in the terminal the user has just left -- and
            // was, until this, never checked at all: `check_external_change`
            // and the whole prompt-and-merge path behind it had no caller.
            // Redraw whatever it finds, because an unmodified document is
            // reloaded without a prompt and that changes the text on screen.
            GEvent::FocusIn => {
                self.check_external_change();
                Response::Redraw
            }
            // A drag that ends outside the window never delivers its release,
            // so losing focus ends it; otherwise the next move over the window,
            // with no button held, would go on extending the selection.
            GEvent::FocusOut => {
                self.dragging = false;
                self.shift_held = false;
                self.hover = None;
                Response::Redraw
            }
            GEvent::Tick { elapsed_ms } => {
                // Whole seconds, and the remainder is *kept*: dropping it makes
                // the autosave interval drift long by up to a second per tick,
                // which over an editing session is the difference between
                // "every five minutes" and "eventually".
                self.tick_ms_carry = self.tick_ms_carry.saturating_add(*elapsed_ms);
                let secs = self.tick_ms_carry / 1000;
                if secs > 0 {
                    self.tick_ms_carry =
                        self.tick_ms_carry.saturating_sub(secs.saturating_mul(1000));
                    self.tick_autosave(secs);
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            GEvent::Mouse(mouse) => {
                if self.handle_mouse(mouse) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            GEvent::Key(key) => {
                // Pressed and released both, so Shift is known while it is
                // held and forgotten when it is not.
                self.shift_held = key.modifiers.shift;
                if !key.pressed {
                    return Response::Idle;
                }
                if let Some((kind, menu)) = self.menu.as_mut() {
                    // An open drop-down takes the keyboard: arrows, Enter,
                    // Escape. Everything else stops here with it.
                    let kind = *kind;
                    match menu.handle_key(key) {
                        Some(MenuAction::Selected(id)) => {
                            self.menu = None;
                            self.choose_from_menu(kind, id);
                        }
                        Some(MenuAction::Closed) => self.menu = None,
                        Some(MenuAction::None) | None => {}
                    }
                    return Response::Redraw;
                }
                let Some((k, m)) = translate_key(key) else {
                    return Response::Idle;
                };
                // Idle when nothing answered, so a stray key does not
                // repaint the whole document.
                if handle_key(self, k, m) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            _ => Response::Idle,
        };
        // Any event can be the one that finishes closing the window: the
        // last save of a "Save all" can arrive as a key in the picker, a
        // press on a dialog button, or a close request itself.
        if self.quit {
            return Response::Exit;
        }
        response
    }

    fn render(&mut self, width: f32, height: f32) -> guitk::render::RenderTree {
        self.window_width = width;
        self.window_height = height;
        let frame = self.frame(width, height);
        // Kept for the pointer's hover and the wheel, which read what was
        // last shown rather than drawing a frame of their own per event.
        self.last_hits = frame.hits().to_vec();
        let mut tree = frame.into_tree();
        // Over the document, and last, so nothing is drawn on top of the
        // dialog. A picker painted under the text it is asking about is the
        // same defect as one that never receives an event, arriving by a
        // different route.
        tree.commands
            .extend(self.picker.render(&self.palette, width, height));
        let palette = self.palette;
        if let Some(question) = self.question.as_mut() {
            question.render(&palette, width, height, &mut tree);
        }
        tree
    }
}

fn main() -> std::process::ExitCode {
    oswindow::app::launch("markdowneditor", &mut App::new(1280.0, 800.0))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that overflows or indexes out of range should fail loudly and
    // point at the line that did it — that is the diagnosis. The defensive
    // lints exist to keep panics out of code that runs on a user's data.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;
    use scratchdir::ScratchDir;

    // --- Document tests ---

    /// Open puts the picker up instead of quietly making a blank document.
    ///
    /// `ToolbarAction::OpenFile` called `self.new_document()`, under the
    /// comment "In a real app, this would open a file dialog. For now, we
    /// create a new document." Click Open, get a blank page -- and a tab count
    /// that went up, so it looked like something had happened.
    /// A file name that is text is shown as it is; one that is not, by its
    /// bytes -- two such names never look the same.
    #[test]
    fn a_file_name_that_is_not_text_is_shown_by_its_bytes() {
        use std::path::Path;
        assert_eq!(
            shown_file_name(Path::new("dir/notes.txt")).as_deref(),
            Some("notes.txt")
        );
        assert_eq!(shown_file_name(Path::new("/")), None);
        #[cfg(windows)]
        let (a, b) = {
            use std::os::windows::ffi::OsStringExt;
            (
                std::ffi::OsString::from_wide(&[0x0066, 0xD800]),
                std::ffi::OsString::from_wide(&[0x0066, 0xD801]),
            )
        };
        #[cfg(not(windows))]
        let (a, b) = {
            use std::os::unix::ffi::OsStringExt;
            (
                std::ffi::OsString::from_vec(vec![b'f', 0xFE]),
                std::ffi::OsString::from_vec(vec![b'f', 0xFF]),
            )
        };
        let shown_a = shown_file_name(Path::new(&a)).unwrap();
        let shown_b = shown_file_name(Path::new(&b)).unwrap();
        assert!(!shown_a.contains('\u{FFFD}'), "{shown_a:?}");
        assert_ne!(shown_a, shown_b, "two names became one");
    }

    #[test]
    fn open_puts_the_picker_up_rather_than_making_a_new_document() {
        let mut app = App::new(1280.0, 800.0);
        let before = app.documents.count();

        app.handle_toolbar_action(&ToolbarAction::OpenFile);

        assert!(app.picker.is_open(), "Open did not put a dialog up");
        assert_eq!(
            app.documents.count(),
            before,
            "Open created a document instead of asking for a file"
        );
    }

    /// Save As puts the picker up instead of doing nothing at all.
    ///
    /// **This is the one with a cost attached.** `ToolbarAction::SaveAs` was
    /// an empty body under the comment "Would open a save dialog in a real
    /// app". No dialog, no file, no message. A person who clicks it, sees no
    /// error and closes the editor has been told nothing and has every reason
    /// to believe the file was written under the new name.
    #[test]
    fn save_as_puts_the_picker_up_rather_than_doing_nothing() {
        let mut app = App::new(1280.0, 800.0);

        app.handle_toolbar_action(&ToolbarAction::SaveAs);

        assert!(app.picker.is_open(), "Save As did nothing at all");
        assert!(
            app.picker.is_saving(),
            "Save As put up a dialog that would have opened a file"
        );
    }

    /// An open dialog takes the keyboard from the document behind it.
    ///
    /// The other two tests pin that the toolbar *opens* the picker -- but the
    /// toolbar handler is what opens it, so cutting the picker's event routing
    /// entirely leaves both of them true. This is the half routing actually
    /// decides, and it is the half `scripts/find-unpinned-picker-routing.py`
    /// found missing in sixteen of twenty apps this morning. This app was not
    /// among the twenty because it had no picker at all -- **a scanner that
    /// looks for broken routing cannot see an app with no routing to break.**
    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_document() {
        let mut app = App::new(1280.0, 800.0);
        app.handle_toolbar_action(&ToolbarAction::OpenFile);
        assert!(app.picker.is_open(), "control: the picker must be up");
        let before = app.active_document().lines.join("\n");

        app.on_event(&guitk::event::Event::Key(guitk::event::KeyEvent {
            key: guitk::event::Key::X,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::from("x"),
        }));

        assert_eq!(
            app.active_document().lines.join("\n"),
            before,
            "a keystroke at the open dialog was typed into the document behind it"
        );
    }

    /// A chosen path is written, and the tab is renamed to it.
    ///
    /// Through `picker_took`, which is what the event loop calls -- not
    /// through `Document::save_as` directly. `save_as` already had a test and
    /// already worked; what was missing was anything that reached it.
    #[test]
    fn a_path_chosen_in_the_save_dialog_is_written() {
        let dir = std::env::temp_dir().join(format!(
            "markdowneditor-saveas-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).expect("fixture");
        let path = dir.join("chosen.md");
        let _ = std::fs::remove_file(&path);

        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().lines = vec![String::from("# written through the door")];
        // The dialog a Save As would put up, pointed at the fixture directory
        // -- driven by a real Enter, not by a test-only hook, so the path the
        // document is written to is the path the dialog computed.
        app.picker.put_up(
            guitk::dialog::FileDialog::save()
                .with_initial_path(&dir)
                .with_filename("chosen.md"),
            true,
        );
        assert!(app.picker.is_open(), "control: the save dialog must be up");

        let took = app.picker_took(&guitk::event::Event::Key(guitk::event::KeyEvent {
            key: guitk::event::Key::Enter,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        }));

        assert!(took, "the picker did not consume the choice");
        assert_eq!(
            std::fs::read_to_string(&path).expect("the file the dialog named"),
            "# written through the door"
        );
        assert_eq!(app.active_document().name, "chosen.md");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_document_new() {
        let doc = Document::new();
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0], "");
        assert!(!doc.modified);
        assert!(doc.path.is_none());
        assert_eq!(doc.name, "Untitled");
    }

    /// Both save paths go through `safeio`, not `std::fs::write`.
    ///
    /// A successful atomic write and a successful truncating write leave
    /// identical bytes at an identical path; they differ only when the write
    /// is interrupted, which no portable test can stage. So the routing itself
    /// is asserted, via `safeio`'s `audit` counters. Without this, restoring
    /// `fs::write` leaves every other test in this file green.
    ///
    /// `save` and `save_as` are checked separately because they are separate
    /// call sites: fixing one and missing the other is the likely regression,
    /// and a test that only covered `save` would not notice.
    ///
    /// The counters are process-global and tests run in parallel, so each
    /// check compares a before and after reading rather than an absolute.
    #[test]
    fn both_save_paths_go_through_safeio() {
        // save_as: writes to the path it is handed and adopts it.
        let (_scratch, as_path) = temp_path("routing_save_as");
        let mut doc = Document::new();
        doc.lines = vec!["# notes".to_string(), "body".to_string()];

        let before = safeio::writes_performed();
        doc.save_as(&as_path).expect("save_as");
        assert!(
            safeio::writes_performed() > before,
            "save_as did not go through safeio -- it must not use std::fs::write"
        );
        assert_eq!(
            std::fs::read_to_string(&as_path).expect("read back save_as"),
            "# notes\nbody"
        );

        // save: writes to the path adopted above.
        doc.lines.push("more".to_string());
        let before = safeio::writes_performed();
        doc.save().expect("save");
        assert!(
            safeio::writes_performed() > before,
            "save did not go through safeio -- it must not use std::fs::write"
        );
        assert_eq!(
            std::fs::read_to_string(&as_path).expect("read back save"),
            "# notes\nbody\nmore"
        );
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
        state.query = String::new();
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let spans = highlight_line("# Title", &pal);
        assert!(!spans.is_empty());
        // Should have hash mark span and text span.
        assert!(spans.len() >= 2);
    }

    #[test]
    fn test_highlight_code_fence() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let spans = highlight_line("```rust", &pal);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].color, pal.green);
    }

    #[test]
    fn test_highlight_blockquote() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let spans = highlight_line("> Quote text", &pal);
        assert!(spans.len() >= 2);
        assert_eq!(spans[0].color, pal.blue);
    }

    #[test]
    fn test_highlight_hr() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let spans = highlight_line("---", &pal);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].color, pal.surface2);
    }

    #[test]
    fn test_highlight_list_item() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let spans = highlight_line("- Item text", &pal);
        assert!(!spans.is_empty());
        assert_eq!(spans[0].color, pal.blue);
    }

    #[test]
    fn test_highlight_inline_code() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let spans = highlight_line("Use `code` here", &pal);
        let code_span = spans.iter().find(|s| s.color == pal.green);
        assert!(code_span.is_some());
    }

    #[test]
    fn test_highlight_bold_markers() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let spans = highlight_line("**bold**", &pal);
        // Should have dimmed markers and bold text.
        assert!(spans.len() >= 3);
    }

    #[test]
    fn test_highlight_link() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let spans = highlight_line("[text](url)", &pal);
        let link_span = spans.iter().find(|s| s.color == pal.blue);
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
        assert!(text.contains('|'));
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
        set_heading(&mut doc, 1);
        assert!(doc.lines[0].starts_with("# "));
    }

    #[test]
    fn test_insert_heading_level_3() {
        let mut doc = Document::new();
        doc.lines = vec!["Section".to_string()];
        set_heading(&mut doc, 3);
        assert!(doc.lines[0].starts_with("### "));
    }

    /// Undoing a heading removes the `#` prefix — it does not paste the old
    /// line in front of itself.
    ///
    /// `set_heading` (once `insert_heading`) was the one edit that hand-rolled its own undo entry,
    /// and it recorded the wrong *kind*: an `EditAction::Delete` naming the
    /// whole previous line, whose undo is "put that text back at column 0".
    /// Since nothing had been deleted, undo appended rather than removed, so
    /// Ctrl+H then Ctrl+Z turned `Title` into `Title# Title`.
    #[test]
    fn undoing_a_heading_removes_the_prefix_rather_than_duplicating_the_line() {
        let mut doc = Document::new();
        doc.lines = vec!["Title".to_string()];
        doc.cursor_line = 0;

        set_heading(&mut doc, 2);
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
        set_heading(&mut doc, 1);
        assert_eq!(doc.lines, vec!["only".to_string()]);
    }

    // --- Rendering tests ---

    #[test]
    fn test_render_editor_produces_commands() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let doc = Document::new();
        let find = FindReplaceState::new();
        let cmds = render_editor(&doc, &pal, 0.0, 0.0, 800.0, 600.0, &find);
        assert!(!cmds.is_empty());
    }

    /// Drawing a document whose `scroll_line` is past its last line draws no
    /// lines, rather than indexing off the end. A reload from disk or an
    /// adopted external change can shrink the document under a scroll
    /// position that was valid a frame ago.
    #[test]
    fn rendering_past_the_end_of_a_shrunken_document_draws_nothing() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut doc = Document::new();
        doc.lines = vec!["one".to_string()];
        doc.scroll_line = 500;
        doc.cursor_line = 0;
        let find = FindReplaceState::new();
        // The frame furniture (background, gutter) is still drawn; the point
        // is that it returns at all.
        let cmds = render_editor(&doc, &pal, 0.0, 0.0, 800.0, 600.0, &find);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_preview_produces_commands() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let blocks = parse_markdown("# Hello\n\nWorld");
        let cmds = render_preview(&blocks, &pal, 0.0, 0.0, 800.0, 600.0, 0.0);
        assert!(!cmds.is_empty());
    }

    /// What a drawing function puts in a fresh frame.
    fn drawn(draw: impl FnOnce(&mut Frame<Target>)) -> Frame<Target> {
        let mut frame = Frame::new(1200.0, 800.0);
        draw(&mut frame);
        assert!(frame.is_balanced(), "a clip or translation was left pushed");
        frame
    }

    #[test]
    fn test_render_toolbar_produces_commands() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let toolbar = default_toolbar();
        let frame = drawn(|f| draw_toolbar(f, &toolbar, &pal, None, 0.0, 0.0, 1200.0));
        assert!(!frame.commands().is_empty());
    }

    #[test]
    fn test_render_tab_bar_produces_commands() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let docs = Tabs::with(Document::new());
        let frame = drawn(|f| draw_tab_bar(f, &docs, &pal, None, 0.0, 0.0, 1200.0));
        assert!(!frame.commands().is_empty());
    }

    #[test]
    fn test_render_status_bar_produces_commands() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let doc = Document::new();
        let frame = drawn(|f| {
            draw_status_bar(
                f,
                &doc,
                &pal,
                ViewMode::Split,
                0.0,
                0.0,
                1200.0,
                true,
                StatusNote::Stats,
            );
        });
        assert!(!frame.commands().is_empty());
    }

    #[test]
    fn test_render_toc_sidebar_produces_commands() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let entries = vec![TocEntry {
            level: 1,
            text: "Title".to_string(),
            line: 0,
        }];
        let frame = drawn(|f| draw_toc_sidebar(f, &entries, &pal, 0, None, None, 0.0, 0.0, 600.0));
        assert!(!frame.commands().is_empty());
    }

    #[test]
    fn test_render_find_replace_hidden() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let state = FindReplaceState::new();
        let frame = drawn(|f| draw_find_replace(f, &state, &pal, None, 0.0, 0.0, 1200.0));
        assert!(frame.commands().is_empty()); // hidden by default
        assert!(
            frame.hits().is_empty(),
            "a hidden panel has nothing to press"
        );
    }

    #[test]
    fn test_render_find_replace_visible() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut state = FindReplaceState::new();
        state.visible = true;
        let frame = drawn(|f| draw_find_replace(f, &state, &pal, None, 0.0, 0.0, 1200.0));
        assert!(!frame.commands().is_empty());
    }

    #[test]
    fn test_render_template_chooser() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let frame = drawn(|f| draw_template_chooser(f, &pal, 0, None, 0.0, 0.0, 1200.0, 800.0));
        assert!(!frame.commands().is_empty());
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
        let cmds = app.render_commands();
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

    // --- Find and replace ---

    fn ctrl(shift: bool) -> Modifiers {
        Modifiers {
            ctrl: true,
            shift,
            alt: false,
        }
    }

    /// **The find can be made to match case.**
    ///
    /// `case_sensitive` was `false` at construction with no writer anywhere,
    /// so `Case::sensitive(self.case_sensitive)` received `false` for every
    /// search this program has ever run. The setting existed, was read, and
    /// could not be changed. Found by `scripts/frozen-flag-survey.py`.
    ///
    /// Asserts the *match count*, not the flag: a key that flips a boolean the
    /// searcher ignores would pass the weaker version, which is the defect
    /// `apps/regextester`'s `multiline` had.
    #[test]
    fn the_find_can_be_made_to_match_case() {
        let mut app = App::new(1280.0, 800.0);
        {
            let doc = app.active_document_mut();
            doc.lines = vec!["Beta".to_string(), "beta".to_string()];
        }
        app.refresh_cache();

        app.find_state.visible = true;
        app.find_state.query = "beta".to_string();
        refresh_matches(&mut app);
        let insensitive = app.find_state.matches.len();
        assert_eq!(insensitive, 2, "the fixture should match both spellings");

        assert!(
            handle_find_key(&mut app, Key::Char('i'), ctrl(false)),
            "Ctrl+I in the find bar was not answered"
        );
        assert!(app.find_state.case_sensitive, "the flag did not move");
        assert!(
            app.find_state.matches.len() < insensitive,
            "matching case returned the same {insensitive} matches"
        );
    }

    /// An app holding one document with three lines of known text.
    fn app_with_text() -> App {
        let mut app = App::new(1280.0, 800.0);
        {
            let doc = app.active_document_mut();
            doc.lines = vec![
                "alpha beta".to_string(),
                "beta gamma".to_string(),
                "delta".to_string(),
            ];
        }
        app.refresh_cache();
        app
    }

    /// Typing reaches the find box, and the matches follow what is typed.
    ///
    /// Nothing in production wrote `query` or `replacement`, so the panel
    /// opened with two boxes that could never hold a character.
    #[test]
    fn typing_reaches_the_find_box() {
        let mut app = app_with_text();
        handle_key(&mut app, Key::Char('h'), ctrl(false));
        assert!(app.find_state.visible, "control: Ctrl+H opens the panel");

        for c in "beta".chars() {
            handle_key(&mut app, Key::Char(c), Modifiers::default());
        }

        assert_eq!(app.find_state.query, "beta", "the letters never arrived");
        assert_eq!(
            app.find_state.matches.len(),
            2,
            "the matches did not follow"
        );
    }

    /// While the panel is open, typing does not reach the document behind it.
    #[test]
    fn typing_into_the_panel_does_not_edit_the_document() {
        let mut app = app_with_text();
        let before = app.active_document().full_text();
        handle_key(&mut app, Key::Char('h'), ctrl(false));

        for c in "beta".chars() {
            handle_key(&mut app, Key::Char(c), Modifiers::default());
        }

        assert_eq!(
            app.active_document().full_text(),
            before,
            "the query was typed into the document"
        );
    }

    /// Tab moves to the replacement box.
    #[test]
    fn tab_moves_between_the_two_boxes() {
        let mut app = app_with_text();
        handle_key(&mut app, Key::Char('h'), ctrl(false));
        handle_key(&mut app, Key::Char('x'), Modifiers::default());

        handle_key(&mut app, Key::Tab, Modifiers::default());
        handle_key(&mut app, Key::Char('y'), Modifiers::default());

        assert_eq!(app.find_state.query, "x", "the query changed after Tab");
        assert_eq!(
            app.find_state.replacement, "y",
            "Tab did not reach the second box"
        );
    }

    /// Enter puts the cursor on the match, rather than only counting it.
    ///
    /// This is also the only caller `go_to_line` has ever had.
    #[test]
    fn enter_moves_the_cursor_to_the_match() {
        let mut app = app_with_text();
        handle_key(&mut app, Key::Char('h'), ctrl(false));
        for c in "gamma".chars() {
            handle_key(&mut app, Key::Char(c), Modifiers::default());
        }

        handle_key(&mut app, Key::Enter, Modifiers::default());

        let (line, _, _) = app
            .find_state
            .current_match_info()
            .expect("there is a match to be on");
        assert_eq!(line, 1, "gamma is on the second line");
        assert_eq!(
            app.active_document().cursor_line,
            line,
            "the cursor stayed where it was"
        );
    }

    /// Ctrl+Enter replaces the current match in the document.
    /// **Every key the card advertises is answered by this program.**
    ///
    /// Two panel states, because `Esc`, `Enter`, `Tab` and `Ctrl+I` are
    /// claimed by the find panel when it is open and by the editor when it is
    /// not -- and `Esc` is answered *only* with a panel open. A single state
    /// would have hidden that in whichever direction it was written.
    ///
    /// This runs the real bridge: `keystrokes` builds a toolkit `KeyEvent`
    /// with no text, exactly as a chord arrives, and `translate_key` is what
    /// turns it into the key this app matches on. A guard that skipped it
    /// would pass on a list whose keys the bridge drops.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for ev in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = [false, true].into_iter().any(|panel_open| {
                    let Some((k, m)) = translate_key(&ev) else {
                        return false;
                    };
                    let mut app = App::new(1280.0, 800.0);
                    app.find_state.visible = panel_open;
                    handle_key(&mut app, k, m)
                });
                assert!(
                    answered,
                    "the card advertises {label:?} for {what:?}, and nothing answers {:?}",
                    ev.key
                );
            }
        }
    }

    /// **The card reaches the window, and nothing acts behind it.**
    ///
    /// The control is the last third. `Ctrl+N` behind the card must not open a
    /// document -- but asserting only that would pass just as well on an app
    /// that had lost `Ctrl+N` altogether, so the same keystroke is then run
    /// with the card down and required to work.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        fn drawn(app: &App) -> String {
            app.render_commands()
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" | ")
        }

        let mut app = App::new(1280.0, 800.0);
        assert!(
            !drawn(&app).contains("F1 closes this"),
            "the card is up before anybody asked for it"
        );

        handle_key(&mut app, Key::Function(1), Modifiers::default());
        let shown = drawn(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        let before = app.documents.count();
        handle_key(&mut app, Key::Char('n'), ctrl(false));
        assert_eq!(
            app.documents.count(),
            before,
            "Ctrl+N opened a document through the shortcut card"
        );

        handle_key(&mut app, Key::Escape, Modifiers::default());
        handle_key(&mut app, Key::Char('n'), ctrl(false));
        assert!(
            app.documents.count() > before,
            "control: Ctrl+N does nothing even with the card down"
        );
    }

    #[test]
    fn ctrl_enter_replaces_the_current_match() {
        let mut app = app_with_text();
        handle_key(&mut app, Key::Char('h'), ctrl(false));
        for c in "beta".chars() {
            handle_key(&mut app, Key::Char(c), Modifiers::default());
        }
        handle_key(&mut app, Key::Tab, Modifiers::default());
        for c in "ZZ".chars() {
            handle_key(&mut app, Key::Char(c), Modifiers::default());
        }

        handle_key(&mut app, Key::Enter, ctrl(false));

        let text = app.active_document().full_text();
        assert!(text.contains("ZZ"), "nothing was replaced: {text}");
        assert_eq!(text.matches("beta").count(), 1, "it replaced more than one");
        assert!(
            app.active_document().modified,
            "the document is not marked modified"
        );
    }

    /// Ctrl+Shift+Enter replaces every match.
    #[test]
    fn ctrl_shift_enter_replaces_every_match() {
        let mut app = app_with_text();
        handle_key(&mut app, Key::Char('h'), ctrl(false));
        for c in "beta".chars() {
            handle_key(&mut app, Key::Char(c), Modifiers::default());
        }
        handle_key(&mut app, Key::Tab, Modifiers::default());
        for c in "ZZ".chars() {
            handle_key(&mut app, Key::Char(c), Modifiers::default());
        }

        handle_key(&mut app, Key::Enter, ctrl(true));

        let text = app.active_document().full_text();
        assert_eq!(text.matches("beta").count(), 0, "a match survived: {text}");
        assert_eq!(
            text.matches("ZZ").count(),
            2,
            "not every match was replaced"
        );
    }

    /// Backspace deletes from the focused box.
    #[test]
    fn backspace_deletes_from_the_focused_box() {
        let mut app = app_with_text();
        handle_key(&mut app, Key::Char('h'), ctrl(false));
        for c in "beta".chars() {
            handle_key(&mut app, Key::Char(c), Modifiers::default());
        }

        handle_key(&mut app, Key::Backspace, Modifiers::default());

        assert_eq!(
            app.find_state.query, "bet",
            "backspace did not reach the box"
        );
    }

    /// Escape closes the panel and the keyboard goes back to the document.
    #[test]
    fn escape_closes_the_panel_and_returns_the_keyboard() {
        let mut app = app_with_text();
        handle_key(&mut app, Key::Char('h'), ctrl(false));
        handle_key(&mut app, Key::Char('q'), Modifiers::default());

        handle_key(&mut app, Key::Escape, Modifiers::default());
        assert!(!app.find_state.visible, "Escape did not close the panel");

        handle_key(&mut app, Key::Char('q'), Modifiers::default());
        assert!(
            app.active_document().full_text().contains('q'),
            "the document did not get the keyboard back"
        );
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        assert_eq!(pal.base.r, 0x1E);
        assert_eq!(pal.base.g, 0x1E);
        assert_eq!(pal.base.b, 0x2E);
        assert_eq!(pal.blue.r, 0x89);
        assert_eq!(pal.text.r, 0xCD);
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
        let buttons = default_toolbar()
            .into_iter()
            .filter(|item| matches!(item, ToolbarItem::Button(_)))
            .count();
        assert!(buttons > 10);
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

    /// A scratch file path for one test, together with the guard that owns the
    /// directory it lives in. The file and its directory are removed when the
    /// guard drops -- including during a panic unwind, which the manual
    /// `let _ = fs::remove_file(...)` trailers this replaces did not do.
    ///
    /// The name used to carry the system clock in nanoseconds, which is not
    /// unique. `cargo test` runs a binary's tests as threads of one process,
    /// and the clock a thread reads is only refreshed on a timer interrupt, so
    /// every test that starts within the same tick draws the same tag and they
    /// fight over one file. `ScratchDir` names itself from the process id and a
    /// per-process atomic counter, which is unique by construction.
    ///
    /// Bind the guard to a named local -- `let (_scratch, path) = ...` -- never
    /// to a bare `_`, which drops it before the test's first line.
    fn temp_path(tag: &str) -> (ScratchDir, PathBuf) {
        let scratch = ScratchDir::new(&format!("slate_md_test_{tag}"));
        let p = scratch.path("doc.md");
        (scratch, p)
    }

    // --- A save that fails must say so -------------------------------------
    //
    // The regression these guard: all three save call sites (the toolbar
    // button, Ctrl+S and the autosave tick) discarded `Document::save`'s
    // `Result`, so a write onto a read-only file or a full disk was
    // indistinguishable from one that worked. The buffer was never lost —
    // `save()` uses `?`, so `modified` stays set — but only until the window
    // closed, and nothing on screen said anything was wrong.

    /// A path whose *parent directory does not exist*, so `fs::write` to it
    /// fails on every platform without needing permissions to be manipulated.
    ///
    /// The guard comes back with it: the missing parent is missing *inside* a
    /// real scratch directory, and that directory still has to be cleaned up.
    fn unwritable_path(tag: &str) -> (ScratchDir, PathBuf) {
        let (scratch, p) = temp_path(tag);
        let p = p.join("no_such_dir").join("doc.md");
        (scratch, p)
    }

    #[test]
    fn a_failed_ctrl_s_leaves_a_message_and_keeps_the_buffer() {
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().lines = vec!["precious".to_string()];
        app.active_document_mut().modified = true;
        let (_scratch, bad_path) = unwritable_path("ctrl_s");
        app.active_document_mut().path = Some(bad_path);

        handle_key(
            &mut app,
            Key::Char('s'),
            Modifiers {
                ctrl: true,
                shift: false,
                alt: false,
            },
        );

        assert!(app.save_error.is_some(), "a failed save said nothing");
        assert!(
            app.active_document().modified,
            "document was marked clean by a write that never happened"
        );
        assert_eq!(app.active_document().lines, vec!["precious".to_string()]);
    }

    #[test]
    fn a_successful_save_clears_the_message() {
        let (_scratch, path) = temp_path("md_clears");
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().lines = vec!["ok".to_string()];
        app.active_document_mut().modified = true;
        let (_scratch, bad_path) = unwritable_path("clears");
        app.active_document_mut().path = Some(bad_path);
        assert!(!app.save_active());
        assert!(app.save_error.is_some());

        app.active_document_mut().path = Some(path.clone());
        assert!(app.save_active());

        assert!(app.save_error.is_none(), "stale complaint left on screen");
        assert!(!app.active_document().modified);
    }

    #[test]
    fn a_failing_autosave_is_not_silent() {
        // The worst version of this bug: the user has been told the file is
        // being looked after every interval, and it is not.
        let mut app = App::new(1280.0, 800.0);
        app.autosave_enabled = true;
        app.autosave_interval = 1;
        app.active_document_mut().lines = vec!["work".to_string()];
        app.active_document_mut().modified = true;
        let (_scratch, bad_path) = unwritable_path("autosave");
        app.active_document_mut().path = Some(bad_path);

        app.tick_autosave(60);

        let msg = app.save_error.clone().unwrap_or_default();
        assert!(msg.to_lowercase().contains("auto-save"), "{msg}");
        assert!(app.active_document().modified);
    }

    #[test]
    fn an_autosave_that_works_clears_an_earlier_complaint() {
        let (_scratch, path) = temp_path("md_autoclear");
        let mut app = App::new(1280.0, 800.0);
        app.autosave_enabled = true;
        app.autosave_interval = 1;
        app.save_error = Some("stale".to_string());
        app.active_document_mut().lines = vec!["work".to_string()];
        app.active_document_mut().modified = true;
        app.active_document_mut().path = Some(path.clone());

        app.tick_autosave(60);

        assert!(app.save_error.is_none());
    }

    #[test]
    fn an_idle_autosave_tick_does_not_clear_a_real_complaint() {
        // Nothing was attempted, so nothing was learned; the outstanding
        // failure must survive a tick that saved no document at all.
        let mut app = App::new(1280.0, 800.0);
        app.autosave_enabled = true;
        app.save_error = Some("disk full".to_string());
        app.active_document_mut().modified = false;

        app.tick_autosave(60);

        assert_eq!(app.save_error.as_deref(), Some("disk full"));
    }

    #[test]
    fn the_status_bar_shows_the_save_error_instead_of_the_word_count() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let doc = Document::new();
        let msg = "Could not save notes.md: disk full";
        let frame = drawn(|f| {
            draw_status_bar(
                f,
                &doc,
                &pal,
                ViewMode::Split,
                0.0,
                0.0,
                1200.0,
                true,
                StatusNote::Error(msg),
            );
        });

        let texts: Vec<&str> = frame
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&msg), "{texts:?}");
        assert!(
            !texts.iter().any(|t| t.contains("min read")),
            "the stats and the error would collide: {texts:?}"
        );
    }

    #[test]
    fn test_disk_changed_detects_modification() {
        let (_scratch, path) = temp_path("md_detect");
        std::fs::write(&path, b"# Title\n\nbody\n").unwrap();
        let doc = Document::from_file(&path).unwrap();
        assert_eq!(doc.disk_changed(), DiskChange::Unchanged);
        std::fs::write(&path, b"# Title\n\nCHANGED body\n").unwrap();
        match doc.disk_changed() {
            DiskChange::Modified { disk } => assert_eq!(disk, "# Title\n\nCHANGED body"),
            other => panic!("expected Modified, got {other:?}"),
        }
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
        let (_scratch, path) = temp_path("md_review");
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
    }

    #[test]
    fn test_auto_reload_unmodified() {
        let (_scratch, path) = temp_path("md_autoreload");
        std::fs::write(&path, b"first\n").unwrap();
        let mut app = App::new(1280.0, 800.0);
        app.open_file(&path).unwrap();
        std::fs::write(&path, b"second\n").unwrap();
        assert!(!app.check_external_change());
        assert_eq!(app.active_document().full_text(), "second");
    }
    // --- Text measurement ---

    /// The width of one mono cell, which several of these tests compare against
    /// to state what "a grid" means. Production code no longer has this idea:
    /// `col_x` measures, so it is right whether or not the face is a grid.
    fn cell() -> f32 {
        text::cell_advance(EDITOR_FONT_SIZE, FontWeightHint::Regular)
    }

    /// The document counts columns in bytes; the pane advances by glyphs. A
    /// multi-byte character advances once, not two or three times — get this
    /// wrong and the caret sits several columns right of the character it
    /// precedes on any line holding an accent, and the selection band
    /// stretches with it.
    #[test]
    fn column_x_advances_by_glyph_not_by_byte() {
        let cell = cell();
        // "é" is two bytes, so the byte offset after it is 2 — but it is one
        // glyph, so the caret belongs one advance in.
        assert!((col_x("éx", 2, 0.0) - cell).abs() < 0.01);
        assert!((col_x("ax", 1, 0.0) - cell).abs() < 0.01);
        // Same text, same glyphs, whatever the encoding costs.
        assert!((col_x("ééé", 6, 0.0) - col_x("aaa", 3, 0.0)).abs() < 0.01);
    }

    /// The real claim, and the one a cell count could not make: `col_x` is the
    /// renderer's own answer. Whatever the face does with a character — a wide
    /// ideograph, a zero-width combining mark, a `.notdef` box for a glyph it
    /// does not have — the caret goes exactly where the drawn text ends,
    /// because it is the same measurement.
    #[test]
    fn column_x_is_where_the_drawn_prefix_actually_ends() {
        for line in [
            "abc",
            "a#*_W",
            "héllo",
            "日本語です",
            "e\u{0301}tude",
            "\t x",
        ] {
            for (byte, _) in line
                .char_indices()
                .chain(std::iter::once((line.len(), ' ')))
            {
                let drawn = text::measure_in(
                    &line[..byte],
                    EDITOR_FONT_SIZE,
                    FontWeightHint::Regular,
                    FontFamily::Mono,
                );
                let placed = col_x(line, byte, 0.0);
                assert!(
                    (placed - drawn).abs() < 0.01,
                    "{line:?}[..{byte}] draws to {drawn} but the caret goes to {placed}"
                );
            }
        }
    }

    /// A cell count would have been right only if every character advanced the
    /// same distance, and this is the test that says so out loud: it passes for
    /// Latin text in the mono face, which is why the old code looked correct,
    /// and there is no reason it should hold for the whole of Unicode.
    #[test]
    fn a_source_character_fits_a_cell() {
        let cell = cell();
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

    /// Syntax highlighting draws headings and emphasis bold, positioned by a
    /// `col_x` that measures at *regular* weight. That is only sound because
    /// bold advances the same distance on a monospace face — so this test is
    /// the premise `col_x`'s doc comment cites, not a style check.
    #[test]
    fn a_bold_source_character_fits_the_same_cell() {
        let cell = cell();
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

    /// The source pane is measured in the mono face, so it must be drawn in the
    /// mono face — a mismatch is invisible in any assertion about positions.
    #[test]
    fn the_source_pane_is_drawn_in_the_family_it_was_measured_in() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut doc = Document::new();
        doc.insert_text("# Heading WWWW\n\nBody text with iiii and *emphasis*.\n");
        let cmds = render_editor(
            &doc,
            &pal,
            0.0,
            0.0,
            600.0,
            400.0,
            &FindReplaceState::default(),
        );

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
    ///
    /// It must also not be *silently wrong*, which the previous version was:
    /// its fallback for a non-boundary offset was the whole line, so a caret
    /// one byte out of place jumped to the end of the line rather than landing
    /// one character to its left. An offset inside `é` now floors to the
    /// boundary below it, i.e. to the start of that character.
    #[test]
    fn column_x_survives_a_bad_offset_without_lying_about_it() {
        assert!((col_x("é", 1, 0.0) - col_x("é", 0, 0.0)).abs() < f32::EPSILON);
        assert!((col_x("xéy", 2, 0.0) - col_x("xéy", 1, 0.0)).abs() < f32::EPSILON);
        assert!((col_x("abc", 99, 0.0) - col_x("abc", 3, 0.0)).abs() < f32::EPSILON);
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

    // --- Compositor wiring ---

    use oswindow::app::App as _;

    fn tick(ms: u64) -> guitk::event::Event {
        guitk::event::Event::Tick { elapsed_ms: ms }
    }

    /// The autosave clock keeps its remainder between ticks.
    ///
    /// `tick_autosave` counts whole seconds and `Event::Tick` arrives in
    /// milliseconds. Discarding the remainder makes the interval drift *long*
    /// by up to a second per tick — over an editing session that is the
    /// difference between "every five minutes" and "eventually", and it would
    /// never show up as a failure, only as an autosave that feels unreliable.
    #[test]
    fn the_autosave_clock_keeps_its_remainder() {
        let mut app = App::new(1280.0, 800.0);
        app.autosave_enabled = true;

        // Ten 100ms ticks are one second, not zero.
        for _ in 0..10 {
            app.on_event(&tick(100));
        }
        assert_eq!(
            app.active_document().seconds_since_save,
            1,
            "ten 100ms ticks did not add up to a second"
        );

        // And the remainder carries: 1500ms then 600ms is two seconds, not one.
        let mut app = App::new(1280.0, 800.0);
        app.autosave_enabled = true;
        app.on_event(&tick(1500));
        app.on_event(&tick(600));
        assert_eq!(
            app.active_document().seconds_since_save,
            2,
            "the 500ms remainder was dropped"
        );
    }

    /// A sub-second tick is not a repaint.
    ///
    /// Answering `Redraw` to every tick would redraw the whole editor ten times
    /// a second to show a second counter that has not changed.
    #[test]
    fn a_tick_that_completes_no_second_asks_for_no_repaint() {
        let mut app = App::new(1280.0, 800.0);
        app.autosave_enabled = true;
        assert_eq!(app.on_event(&tick(100)), oswindow::app::Response::Idle);
        assert_eq!(app.on_event(&tick(900)), oswindow::app::Response::Redraw);
    }

    /// The clock is asked for only when autosave is on.
    #[test]
    fn the_clock_is_asked_for_only_when_autosave_is_on() {
        let mut app = App::new(1280.0, 800.0);
        app.autosave_enabled = false;
        assert_eq!(app.tick_interval(), None);
        app.autosave_enabled = true;
        assert_eq!(app.tick_interval(), Some(std::time::Duration::from_secs(1)));
    }

    /// Every key this app's handlers understand has a toolkit key that reaches
    /// it.
    ///
    /// The app declares its own `Key` enum — a parallel vocabulary — and a
    /// variant with no translation is silently unreachable: the app looks like
    /// it has no Home key rather than like it has a gap in a match.
    #[test]
    fn every_named_key_has_a_translation() {
        use guitk::event::Key as GKey;
        for (g, want) in [
            (GKey::Enter, Key::Enter),
            (GKey::Backspace, Key::Backspace),
            (GKey::Delete, Key::Delete),
            (GKey::Up, Key::Up),
            (GKey::Down, Key::Down),
            (GKey::Left, Key::Left),
            (GKey::Right, Key::Right),
            (GKey::Home, Key::Home),
            (GKey::End, Key::End),
            (GKey::PageUp, Key::PageUp),
            (GKey::PageDown, Key::PageDown),
            (GKey::Tab, Key::Tab),
            (GKey::Escape, Key::Escape),
            (GKey::F1, Key::Function(1)),
            (GKey::F12, Key::Function(12)),
        ] {
            let ev = guitk::event::KeyEvent {
                key: g,
                pressed: true,
                modifiers: guitk::event::Modifiers::NONE,
                text: String::new(),
            };
            assert_eq!(
                translate_key(&ev).map(|(k, _)| k),
                Some(want),
                "{g:?} did not translate to {want:?}"
            );
        }
    }

    /// A chord still carries its letter, though it produces no text.
    ///
    /// On most layouts `Ctrl+S` delivers an empty `text`, so a translation that
    /// only read `text` would make every shortcut in the app unreachable.
    #[test]
    fn a_chord_carries_its_letter_despite_producing_no_text() {
        let ev = guitk::event::KeyEvent {
            key: guitk::event::Key::S,
            pressed: true,
            modifiers: guitk::event::Modifiers::ctrl(),
            text: String::new(),
        };
        let (k, m) = translate_key(&ev).expect("Ctrl+S translates");
        assert_eq!(k, Key::Char('s'));
        assert!(m.ctrl, "the chord lost its modifier");
    }

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        fn fills(app: &mut App) -> Vec<Color> {
            app.render(1200.0, 800.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = App::new(1200.0, 800.0);

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // --- The pointer ---------------------------------------------------------
    //
    // Everything below drives the window the way a person does: through
    // `on_event`, at the point the renderer says it drew the control. A test
    // that called `activate` or `handle_toolbar_action` directly would have
    // supplied the answer to the question the pointer layer exists to answer.

    use guitk::probe::{self, Probe};
    use oswindow::app::Response;

    impl Probe for App {
        type Target = Target;
        type Outcome = Response;
        const SIZE: (f32, f32) = (1280.0, 800.0);

        fn draw(&self, size: (f32, f32)) -> Frame<Target> {
            self.frame(size.0, size.1)
        }

        /// A click is a press and a release; the release is what ends a drag
        /// the press began in the source pane.
        fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Response {
            self.window_width = size.0;
            self.window_height = size.1;
            let press = self.on_event(&mouse(x, y, MouseEventKind::Press(button)));
            let release = self.on_event(&mouse(x, y, MouseEventKind::Release(button)));
            if release == Response::Exit {
                release
            } else {
                press
            }
        }

        fn key_at(&mut self, key: &guitk::event::KeyEvent, size: (f32, f32)) -> Response {
            self.window_width = size.0;
            self.window_height = size.1;
            self.on_event(&guitk::event::Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, size: (f32, f32)) -> Option<Response> {
            self.window_width = size.0;
            self.window_height = size.1;
            Some(self.on_event(&mouse(x, y, MouseEventKind::Scroll { dx: 0.0, dy })))
        }
    }

    /// What the close question is asking about, while it is up.
    fn asking(app: &App) -> Option<CloseScope> {
        app.question.as_ref().map(Question::pending)
    }

    /// Click the close question's `choice` where it is drawn. A dialog's
    /// buttons are where it last drew them, so it is drawn first.
    fn answer(app: &mut App, choice: Choice) -> Response {
        let (w, h) = App::SIZE;
        let _ = oswindow::app::App::render(app, w, h);
        let (x, y) = app
            .question
            .as_ref()
            .and_then(|q| q.button_centre(choice))
            .expect("the question is drawn");
        app.click_at(x, y, MouseButton::Left, App::SIZE)
    }

    fn mouse(x: f32, y: f32, kind: MouseEventKind) -> guitk::event::Event {
        guitk::event::Event::Mouse(MouseEvent { x, y, kind })
    }

    fn ctrl_shift(key: guitk::event::Key) -> guitk::event::KeyEvent {
        probe::press_with(
            key,
            guitk::event::Modifiers {
                ctrl: true,
                shift: true,
                ..guitk::event::Modifiers::NONE
            },
        )
    }

    /// Every text the frame draws, joined, for "is this on screen" checks.
    fn screen_text(app: &App) -> String {
        app.frame(1280.0, 800.0)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Where the renderer draws `(line, col)` of the active document, as a
    /// point a press can aim at: the character boundary's own x, and the
    /// middle of the line's row.
    fn point_of(app: &App, line: usize, col: usize) -> (f32, f32) {
        let pane = probe::rect_of(app, Target::Editor).expect("the source pane is drawn");
        let doc = app.active_document();
        let row = line - doc.scroll_line;
        let text_x = pane.x + GUTTER_WIDTH + EDITOR_PADDING;
        (
            col_x(doc.line_text(line), col, text_x) + 0.5,
            pane.y + row as f32 * LINE_HEIGHT + LINE_HEIGHT / 2.0,
        )
    }

    /// **Every toolbar button the window draws answers a press, and does what
    /// its action does.** The control is the same action through
    /// `handle_toolbar_action` on a twin app: a button wired to the wrong
    /// action, or to none, leaves the two apps different.
    #[test]
    fn every_toolbar_button_does_what_its_action_says() {
        let app = app_with_text();
        let drawn: Vec<ToolbarAction> = app
            .frame(1280.0, 800.0)
            .hits()
            .iter()
            .filter_map(|(t, _)| match t {
                Target::Toolbar(action) => Some(*action),
                _ => None,
            })
            .collect();
        let offered: Vec<ToolbarAction> = default_toolbar()
            .iter()
            .filter_map(|item| match item {
                ToolbarItem::Button(b) => Some(b.action),
                ToolbarItem::Separator => None,
            })
            .collect();
        assert_eq!(
            drawn, offered,
            "at 1280 wide every button is drawn, in order"
        );

        for action in drawn {
            let mut clicked = app_with_text();
            probe::click(&mut clicked, Target::Toolbar(action));
            let mut direct = app_with_text();
            direct.handle_toolbar_action(&action);
            assert_eq!(
                (
                    clicked.active_document().full_text(),
                    clicked.documents.count(),
                    clicked.view_mode,
                    clicked.toc_visible,
                    clicked.find_state.visible,
                    clicked.template_chooser_open,
                    clicked.picker.is_open(),
                    clicked.picker.is_saving(),
                ),
                (
                    direct.active_document().full_text(),
                    direct.documents.count(),
                    direct.view_mode,
                    direct.toc_visible,
                    direct.find_state.visible,
                    direct.template_chooser_open,
                    direct.picker.is_open(),
                    direct.picker.is_saving(),
                ),
                "pressing {action:?} did not do what {action:?} does"
            );
        }
    }

    /// A separator is a rule, not a button. It used to be a `ToolbarButton`
    /// whose action was `NewFile`, so the first press anyone made on the gap
    /// between Save and Bold would have opened a document.
    #[test]
    fn the_gap_between_toolbar_groups_is_not_a_button() {
        let app = App::new(1280.0, 800.0);
        let layout = toolbar_layout(&app.toolbar, 0.0, 0.0, 1280.0);
        assert!(!layout.separators.is_empty(), "the toolbar has separators");
        let frame = app.frame(1280.0, 800.0);
        for sx in layout.separators {
            let at = frame.hit_test(sx + 4.0, TOOLBAR_HEIGHT / 2.0);
            assert_eq!(at, None, "the separator at {sx} answers as {at:?}");
        }
    }

    /// **A window too narrow for the toolbar offers the rest through `»`.**
    /// Before this, the last buttons were drawn past the window's right edge:
    /// invisible, and so unpressable. The menu offers exactly the buttons that
    /// did not fit, and choosing one does what the button would have.
    #[test]
    fn a_narrow_window_offers_its_missing_buttons_through_the_chevron() {
        let size = (640.0, 600.0);
        let mut app = app_with_text();
        app.window_width = size.0;
        app.window_height = size.1;
        let frame = app.frame(size.0, size.1);
        let chevron = frame
            .rect_of(|t| *t == Target::ToolbarMore)
            .expect("a narrow toolbar draws a chevron");
        assert!(
            chevron.right() <= size.0,
            "the chevron is inside the window"
        );
        for (target, rect) in frame.hits() {
            if matches!(target, Target::Toolbar(_)) {
                assert!(rect.right() <= chevron.x, "{target:?} overlaps the chevron");
            }
        }
        let hidden: Vec<(usize, ToolbarAction)> = toolbar_overflow(&app.toolbar, size.0)
            .into_iter()
            .map(|(i, b)| (i, b.action))
            .collect();
        assert!(!hidden.is_empty());
        assert!(
            hidden
                .iter()
                .all(|(_, action)| frame.rect_of(|t| *t == Target::Toolbar(*action)).is_none()),
            "the menu offers a button that is already on the toolbar"
        );

        probe::click_sized(&mut app, Target::ToolbarMore, MouseButton::Left, size);
        assert!(app.menu.is_some(), "the chevron opened nothing");
        // The last hidden button is "HTML", which puts the save picker up.
        let (last_index, last) = *hidden.last().expect("something is hidden");
        assert_eq!(last, ToolbarAction::ExportHtml);
        app.menu = None;
        app.choose_from_menu(MenuKind::Toolbar, last_index as u64);
        assert!(
            app.picker.is_saving(),
            "choosing HTML from the menu did not export"
        );

        // And the keyboard can drive the menu: Down onto the first row, Enter.
        let mut app = app_with_text();
        app.window_width = size.0;
        probe::click_sized(&mut app, Target::ToolbarMore, MouseButton::Left, size);
        let (_, expected) = hidden[0];
        probe::key(&mut app, &probe::press(guitk::event::Key::Down));
        probe::key(&mut app, &probe::press(guitk::event::Key::Enter));
        assert!(app.menu.is_none(), "Enter did not close the menu");
        let mut direct = app_with_text();
        direct.handle_toolbar_action(&expected);
        assert_eq!(app.picker.is_saving(), direct.picker.is_saving());
        assert_eq!(app.view_mode, direct.view_mode);
        assert_eq!(app.toc_visible, direct.toc_visible);
        assert_eq!(
            app.active_document().full_text(),
            direct.active_document().full_text(),
            "the menu's first row did not do what {expected:?} does"
        );
    }

    /// **Hovering a button says what it does, key included.** The tooltips
    /// were written for every button and read by nothing.
    #[test]
    fn hovering_a_button_names_it_in_the_status_bar() {
        let mut app = App::new(1280.0, 800.0);
        let bold = probe::rect_of(&app, Target::Toolbar(ToolbarAction::Bold)).unwrap();
        let (x, y) = bold.centre();
        let redraw = app.on_event(&mouse(x, y, MouseEventKind::Move));
        assert_eq!(redraw, Response::Redraw);
        assert!(screen_text(&app).contains("Bold (Ctrl+B)"));
        // Moving within the same button changes nothing and asks for nothing.
        assert_eq!(
            app.on_event(&mouse(x + 1.0, y, MouseEventKind::Move)),
            Response::Idle
        );
        app.on_event(&mouse(x, 400.0, MouseEventKind::Move));
        assert!(
            !screen_text(&app).contains("Bold (Ctrl+B)"),
            "the tip outlived the hover"
        );
    }

    /// **The tabs switch and close.** `switch_tab` and `close_document` were
    /// written and tested and had no caller: a second document, once open,
    /// could never be brought back to the front or put away.
    #[test]
    fn a_tab_comes_forward_and_its_close_button_closes_it() {
        let mut app = App::new(1280.0, 800.0);
        app.new_document();
        app.active_document_mut().lines = vec!["second".to_string()];
        assert_eq!(app.active_doc(), 1);

        probe::click(&mut app, Target::Tab(0));
        assert_eq!(
            app.active_doc(),
            0,
            "pressing the first tab did not bring it forward"
        );

        probe::click(&mut app, Target::CloseTab(0));
        assert_eq!(app.documents.count(), 1, "the close button closed nothing");
        assert_eq!(
            app.active_document().full_text(),
            "second",
            "the wrong tab closed"
        );
    }

    /// **Closing a document with unsaved changes asks first**, and each answer
    /// does what it says. The control for "Cancel" is that the same press does
    /// close a document with nothing to lose (the test above).
    #[test]
    fn closing_an_unsaved_document_asks_and_each_answer_is_kept() {
        let (_scratch, path) = temp_path("md_close_ask");
        std::fs::write(&path, "on disk").unwrap();

        // Cancel keeps it.
        let mut app = App::new(1280.0, 800.0);
        app.open_file(&path).unwrap();
        app.active_document_mut().insert_char('!');
        probe::click(&mut app, Target::CloseTab(1));
        assert_eq!(asking(&app), Some(CloseScope::Tab(1)));
        assert_eq!(app.documents.count(), 2, "it closed without asking");
        answer(&mut app, Choice::Cancel);
        assert_eq!(app.documents.count(), 2);
        assert_eq!(asking(&app), None);

        // Don't save closes it and leaves the file alone.
        probe::click(&mut app, Target::CloseTab(1));
        answer(&mut app, Choice::Discard);
        assert_eq!(app.documents.count(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk");

        // Save writes it, then closes it.
        app.open_file(&path).unwrap();
        app.active_document_mut().insert_char('!');
        probe::click(&mut app, Target::CloseTab(1));
        probe::key(&mut app, &probe::press(guitk::event::Key::S));
        assert_eq!(app.documents.count(), 1, "Save did not close the tab");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "!on disk");
    }

    /// An untitled document has nowhere to be saved, so "Save" asks where --
    /// and the tab closes only once it has been written there.
    #[test]
    fn saving_an_untitled_document_on_close_asks_where_then_closes_it() {
        let (_scratch, path) = temp_path("md_close_untitled");
        let mut app = App::new(1280.0, 800.0);
        app.new_document();
        app.active_document_mut().insert_char('x');
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::W));
        assert_eq!(asking(&app), Some(CloseScope::Tab(1)));
        answer(&mut app, Choice::Save);
        assert!(app.picker.is_saving(), "no picker to say where");
        assert_eq!(app.documents.count(), 2, "closed before it was saved");

        app.write_chosen(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x");
        assert_eq!(app.documents.count(), 1, "saved but not closed");
    }

    /// **Closing the window with unsaved work asks, instead of losing it.**
    /// `CloseRequested` answered `Exit` whatever the window held.
    #[test]
    fn closing_the_window_with_unsaved_work_asks_first() {
        let (_scratch, path) = temp_path("md_quit");
        std::fs::write(&path, "kept").unwrap();
        let close = guitk::event::Event::CloseRequested;

        // Nothing to lose: it goes.
        let mut app = App::new(1280.0, 800.0);
        assert_eq!(app.on_event(&close), Response::Exit);

        // Something to lose: it asks, and Cancel keeps the window. Auto-save
        // is off here, so the document with a file is not saved on the way
        // out and the dialog has something to ask about.
        let mut app = App::new(1280.0, 800.0);
        app.autosave_enabled = false;
        app.open_file(&path).unwrap();
        app.active_document_mut().insert_char('!');
        assert_eq!(
            app.on_event(&close),
            Response::KeepOpen,
            "unsaved work was thrown away: any answer but KeepOpen closes the window"
        );
        assert_eq!(asking(&app), Some(CloseScope::Window));
        assert_ne!(answer(&mut app, Choice::Cancel), Response::Exit);

        // Save all writes it and then goes.
        app.on_event(&close);
        assert_eq!(
            answer(&mut app, Choice::Save),
            Response::Exit,
            "saved everything and stayed open"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "!kept");

        // Don't save goes at once and writes nothing.
        let mut app = App::new(1280.0, 800.0);
        app.autosave_enabled = false;
        app.open_file(&path).unwrap();
        app.active_document_mut().insert_char('?');
        app.on_event(&close);
        assert_eq!(answer(&mut app, Choice::Discard), Response::Exit);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "!kept");
    }

    /// **With auto-save on, closing the window saves what has a file** -- and
    /// asks only about what does not. Auto-save is the user having said "save
    /// for me", and the close is the last chance to.
    #[test]
    fn closing_with_auto_save_on_saves_titled_documents_and_asks_about_the_rest() {
        let (_scratch, path) = temp_path("md_quit_autosave");
        std::fs::write(&path, "kept").unwrap();
        let close = guitk::event::Event::CloseRequested;

        let mut app = App::new(1280.0, 800.0);
        assert!(app.autosave_enabled, "auto-save is on by default");
        app.open_file(&path).unwrap();
        app.active_document_mut().insert_char('!');
        assert_eq!(
            app.on_event(&close),
            Response::Exit,
            "it asked about a saved document"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "!kept");

        // An untitled document has nowhere to go, so it is still asked about,
        // and the titled one is saved all the same.
        let mut app = App::new(1280.0, 800.0);
        app.open_file(&path).unwrap();
        app.active_document_mut().insert_char('?');
        app.new_document();
        app.active_document_mut().insert_char('u');
        assert_ne!(
            app.on_event(&close),
            Response::Exit,
            "the untitled work was dropped"
        );
        assert_eq!(asking(&app), Some(CloseScope::Window));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "?!kept");
    }

    /// A dialog that must be answered keeps the press off the document behind
    /// it: the dimmed window is a target that does nothing.
    #[test]
    fn a_press_outside_a_dialog_does_not_reach_the_document() {
        let mut app = App::new(1280.0, 800.0);
        app.new_document();
        app.active_document_mut().insert_char('x');
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::W));
        assert!(app.question.is_some());
        // Where the first tab is drawn, under the dimmed window.
        let first_tab = {
            let mut plain = App::new(1280.0, 800.0);
            plain.new_document();
            probe::rect_of(&plain, Target::Tab(0)).unwrap()
        };
        let (x, y) = first_tab.centre();
        app.click_at(x, y, MouseButton::Left, App::SIZE);
        assert_eq!(
            app.active_doc(),
            1,
            "the press went through the dialog to a tab"
        );
        assert!(
            app.question.is_some(),
            "a press beside the dialog answered it"
        );
    }

    /// **Many tabs shrink, then overflow into a menu, and the tab in front is
    /// always one of those drawn.**
    #[test]
    fn many_tabs_keep_the_front_one_visible_and_list_the_rest() {
        let mut app = App::new(640.0, 600.0);
        for i in 0..20 {
            app.new_document();
            app.active_document_mut().name = format!("a-long-document-name-{i}.md");
        }
        let size = (640.0, 600.0);
        let front = app.active_doc();
        assert!(
            probe::is_visible_sized(&app, Target::Tab(front), size),
            "the document in front has no tab on screen"
        );
        assert!(probe::is_visible_sized(&app, Target::TabsMore, size));
        let frame = app.frame(size.0, size.1);
        for (target, rect) in frame.hits() {
            if matches!(target, Target::Tab(_)) {
                assert!(rect.right() <= size.0, "{target:?} runs off the window");
                assert!(
                    rect.w >= TAB_MIN_WIDTH - 0.01,
                    "{target:?} squeezed past the floor"
                );
            }
        }
        probe::click_sized(&mut app, Target::TabsMore, MouseButton::Left, size);
        app.menu = None;
        app.choose_from_menu(MenuKind::Tabs, 0);
        assert_eq!(app.active_doc(), 0);
        assert!(
            probe::is_visible_sized(&app, Target::Tab(0), size),
            "brought the first document forward and did not show its tab"
        );
    }

    /// **The contents panel opens, and a heading in it jumps to that heading.**
    /// The module doc promised "clickable" for the whole life of the file.
    #[test]
    fn a_heading_in_the_contents_jumps_to_it() {
        let mut app = App::new(1280.0, 800.0);
        let mut lines = vec!["# Top".to_string()];
        lines.extend((0..100).map(|i| format!("line {i}")));
        lines.push("## Far down".to_string());
        app.active_document_mut().lines = lines;
        app.refresh_cache();

        assert!(!probe::is_visible(&app, Target::Toc));
        probe::key(&mut app, &ctrl_shift(guitk::event::Key::O));
        assert!(app.toc_visible, "Ctrl+Shift+O did not open the contents");

        probe::click(&mut app, Target::Heading(101));
        let doc = app.active_document();
        assert_eq!((doc.cursor_line, doc.cursor_col), (101, 0));
        assert_eq!(
            doc.scroll_line, 101,
            "the heading was not brought into view"
        );
        assert!(
            screen_text(&app).contains("Far down"),
            "the source pane does not show the heading jumped to"
        );
    }

    /// The contents scroll under the wheel when they are longer than the
    /// panel, and a heading scrolled out of the panel cannot be pressed.
    #[test]
    fn a_long_contents_scrolls_and_hides_what_scrolled_away() {
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().lines = (0..80).map(|i| format!("# Heading {i}")).collect();
        app.refresh_cache();
        app.toc_visible = true;
        assert!(probe::is_visible(&app, Target::Heading(0)));
        assert!(
            !probe::is_visible(&app, Target::Heading(79)),
            "80 rows fit a 800px panel?"
        );

        probe::scroll_at_point(&mut app, Target::Toc, -10.0);
        assert!(app.toc_scroll > 0, "the wheel did not scroll the contents");
        assert!(
            !probe::is_visible(&app, Target::Heading(0)),
            "row 0 is still pressable"
        );

        for _ in 0..20 {
            probe::scroll_at_point(&mut app, Target::Toc, -10.0);
        }
        assert!(
            probe::is_visible(&app, Target::Heading(79)),
            "the last heading is unreachable"
        );
    }

    /// **The find panel's boxes, arrows, switch and buttons all answer.**
    #[test]
    fn the_find_panel_answers_the_pointer() {
        let mut app = app_with_text();
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::H));
        probe::type_str(&mut app, "beta");
        assert_eq!(app.find_state.match_count(), 2);

        probe::click(&mut app, Target::FindReplacement);
        assert!(
            app.find_state.focus_replacement,
            "the replace box did not take the keyboard"
        );
        probe::type_str(&mut app, "B");
        assert_eq!(app.find_state.replacement, "B");
        probe::click(&mut app, Target::FindQuery);
        assert!(!app.find_state.focus_replacement);

        let first = app.find_state.current_match;
        probe::click(&mut app, Target::FindNext);
        assert_ne!(app.find_state.current_match, first, "Next did not move");
        probe::click(&mut app, Target::FindPrev);
        assert_eq!(
            app.find_state.current_match, first,
            "Prev did not come back"
        );

        probe::click(&mut app, Target::FindMatchCase);
        assert!(app.find_state.case_sensitive);
        probe::click(&mut app, Target::FindReplace);
        assert_eq!(app.find_state.match_count(), 1, "Replace replaced nothing");
        probe::click(&mut app, Target::FindReplaceAll);
        assert_eq!(app.find_state.match_count(), 0);
        assert_eq!(app.active_document().full_text(), "alpha B\nB gamma\ndelta");

        probe::click(&mut app, Target::FindClose);
        assert!(!app.find_state.visible);
    }

    /// The find panel says which box is typing and whether case matters.
    #[test]
    fn the_find_panel_draws_its_focus_and_its_case_switch() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let outlines = |state: &FindReplaceState| {
            drawn(|f| draw_find_replace(f, state, &pal, None, 0.0, 0.0, 1200.0))
                .commands()
                .iter()
                .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
                .count()
        };
        let mut state = FindReplaceState::new();
        state.visible = true;
        let plain = outlines(&state);
        state.case_sensitive = true;
        assert_eq!(
            outlines(&state),
            plain + 1,
            "match case is on and nothing shows it"
        );
    }

    /// **A press puts the caret on the character under the pointer.** Aimed
    /// with `col_x`, the function that draws the caret, on a line with wide
    /// characters, so a press model that counted bytes or cells would land
    /// somewhere else.
    #[test]
    fn a_press_in_the_source_puts_the_caret_under_it() {
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().lines = vec![
            "plain".to_string(),
            "日本語 text".to_string(),
            "x".to_string(),
        ];
        app.refresh_cache();
        let col = "日本".len();
        let (x, y) = point_of(&app, 1, col);
        app.click_at(x, y, MouseButton::Left, App::SIZE);
        let doc = app.active_document();
        assert_eq!((doc.cursor_line, doc.cursor_col), (1, col));
        assert!(
            !doc.has_selection(),
            "a click without a drag selected something"
        );

        // Past the end of a line lands at its end; below the last line, on
        // the last line.
        // (In the split view the source pane is the left half, so "past the
        // end" is aimed inside it.)
        let (_, y) = point_of(&app, 0, 0);
        app.click_at(600.0, y, MouseButton::Left, App::SIZE);
        assert_eq!(app.active_document().cursor_col, "plain".len());
        app.click_at(300.0, 700.0, MouseButton::Left, App::SIZE);
        assert_eq!(app.active_document().cursor_line, 2);
    }

    /// **A drag selects, and Shift+press extends.** The selection anchor had
    /// four writers that cleared it and none that set it.
    #[test]
    fn a_drag_selects_and_shift_press_extends() {
        let mut app = app_with_text();
        let (x0, y0) = point_of(&app, 0, 0);
        let (x1, y1) = point_of(&app, 1, 4);
        app.on_event(&mouse(x0, y0, MouseEventKind::Press(MouseButton::Left)));
        app.on_event(&mouse(x1, y1, MouseEventKind::Move));
        app.on_event(&mouse(x1, y1, MouseEventKind::Release(MouseButton::Left)));
        assert_eq!(
            app.active_document().selected_text().as_deref(),
            Some("alpha beta\nbeta")
        );
        // Bold wraps it, which it could never do before.
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::B));
        assert_eq!(
            app.active_document().full_text(),
            "**alpha beta\nbeta** gamma\ndelta"
        );

        let mut app = app_with_text();
        let (x, y) = point_of(&app, 0, 6);
        app.click_at(x, y, MouseButton::Left, App::SIZE);
        app.on_event(&guitk::event::Event::Key(probe::press_with(
            guitk::event::Key::LeftShift,
            guitk::event::Modifiers {
                shift: true,
                ..guitk::event::Modifiers::NONE
            },
        )));
        let (x, y) = point_of(&app, 2, 5);
        app.click_at(x, y, MouseButton::Left, App::SIZE);
        assert_eq!(
            app.active_document().selected_text().as_deref(),
            Some("beta\nbeta gamma\ndelta")
        );
    }

    /// A double press selects the word under it.
    #[test]
    fn a_double_press_selects_a_word() {
        let mut app = app_with_text();
        let (x, y) = point_of(&app, 1, 7);
        app.on_event(&mouse(x, y, MouseEventKind::DoubleClick(MouseButton::Left)));
        assert_eq!(
            app.active_document().selected_text().as_deref(),
            Some("gamma")
        );
    }

    /// The wheel scrolls whichever pane it is over, and the preview follows
    /// the source as it does for the keys.
    #[test]
    fn the_wheel_scrolls_the_pane_under_it() {
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().lines = (0..300)
            .flat_map(|i| [format!("para {i}"), String::new()])
            .collect();
        app.refresh_cache();

        probe::scroll_at_point(&mut app, Target::Editor, -2.0);
        let doc = app.active_document();
        assert!(
            doc.scroll_line > 0,
            "the wheel over the source scrolled nothing"
        );
        assert!(
            doc.preview_scroll > 0.0,
            "the preview did not follow the source"
        );

        let before = app.active_document().preview_scroll;
        probe::scroll_at_point(&mut app, Target::Preview, -2.0);
        assert!(
            app.active_document().preview_scroll > before,
            "the preview did not scroll"
        );
        // Up far past the top stops at the top.
        for _ in 0..50 {
            probe::scroll_at_point(&mut app, Target::Preview, 5.0);
        }
        assert_eq!(app.active_document().preview_scroll, 0.0);
    }

    /// **The status bar's two labels are switches.** Auto-save had no off
    /// switch at all: `autosave_enabled` was written only by the constructor.
    #[test]
    fn the_status_bar_switches_the_view_and_auto_save() {
        let mut app = App::new(1280.0, 800.0);
        let before = app.view_mode;
        probe::click(&mut app, Target::ViewMode);
        assert_eq!(app.view_mode, before.next());
        assert!(app.autosave_enabled);
        probe::click(&mut app, Target::Autosave);
        assert!(!app.autosave_enabled, "auto-save cannot be turned off");
        assert!(screen_text(&app).contains("Auto-save OFF"));
        assert!(
            app.tick_interval().is_none(),
            "the clock kept running for nothing"
        );
    }

    /// **The template chooser can be opened, and answers the pointer and the
    /// keys.** Nothing could open it: `template_chooser_open` was written only
    /// by the key that closed it.
    #[test]
    fn a_template_is_chosen_with_the_pointer_or_the_keys() {
        let mut app = App::new(1280.0, 800.0);
        probe::click(&mut app, Target::Toolbar(ToolbarAction::Templates));
        assert!(app.template_chooser_open);
        probe::click(&mut app, Target::Template(1));
        assert_eq!(app.documents.count(), 2);
        assert_eq!(
            app.active_document().full_text(),
            Template::MeetingNotes.content().trim_end_matches('\n')
        );
        assert!(!app.template_chooser_open);

        probe::key(&mut app, &ctrl_shift(guitk::event::Key::N));
        assert!(
            app.template_chooser_open,
            "Ctrl+Shift+N did not open the chooser"
        );
        // Typing while it is up does not reach the document behind it.
        let text = app.active_document().full_text();
        probe::type_str(&mut app, "q");
        assert_eq!(app.active_document().full_text(), text);
        probe::key(&mut app, &probe::press(guitk::event::Key::Down));
        probe::key(&mut app, &probe::press(guitk::event::Key::Down));
        probe::key(&mut app, &probe::press(guitk::event::Key::Enter));
        assert_eq!(
            app.active_document().full_text(),
            Template::ProjectReadme.content().trim_end_matches('\n')
        );

        // A press on the dimmed window closes it; one on its body does not.
        // Aimed at points, not at the targets' centres: the centre of the
        // dialog's body and of the dimmed window are both on a template row,
        // and a press there would choose that template -- which would also
        // close the chooser, and pass this for the wrong reason.
        probe::click(&mut app, Target::Toolbar(ToolbarAction::Templates));
        let dialog = probe::rect_of(&app, Target::DialogBody).unwrap();
        let docs = app.documents.count();
        app.click_at(dialog.x + 8.0, dialog.y + 8.0, MouseButton::Left, App::SIZE);
        assert!(
            app.template_chooser_open,
            "a press on the dialog's title closed it"
        );
        app.click_at(5.0, 5.0, MouseButton::Left, App::SIZE);
        assert!(
            !app.template_chooser_open,
            "a press outside the dialog left it up"
        );
        assert_eq!(
            app.documents.count(),
            docs,
            "closing the chooser made a document"
        );
    }

    /// **The export writes the HTML it computes.** It used to compute it and
    /// drop it on the floor.
    #[test]
    fn the_html_export_writes_a_file() {
        let (_scratch, path) = temp_path("md_export");
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().lines = vec!["# Title".to_string(), "body".to_string()];
        app.refresh_cache();
        probe::key(&mut app, &ctrl_shift(guitk::event::Key::E));
        assert!(
            app.picker.is_saving(),
            "Ctrl+Shift+E asked for no file name"
        );
        app.write_chosen(&path);
        let html = std::fs::read_to_string(&path).unwrap();
        assert!(html.contains("<h1>Title</h1>"), "{html}");
        assert!(
            screen_text(&app).contains("Exported"),
            "the export said nothing about having worked"
        );
        // And the document itself was not renamed or saved by it.
        assert!(app.active_document().path.is_none());
    }

    /// **A Save As that fails says so, in red, and stays said.** It reported
    /// only to `file_status`, which nothing drew.
    #[test]
    fn a_failed_save_as_is_shown() {
        let (_scratch, path) = unwritable_path("md_saveas_fail");
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().insert_char('x');
        probe::key(&mut app, &ctrl_shift(guitk::event::Key::S));
        assert!(
            app.picker.is_saving(),
            "Ctrl+Shift+S asked for no file name"
        );
        app.write_chosen(&path);
        let err = app
            .save_error
            .clone()
            .expect("the failure was not recorded");
        assert!(
            screen_text(&app).contains(&err),
            "the failure is not on screen"
        );
        probe::type_str(&mut app, "y");
        assert!(
            screen_text(&app).contains(&err),
            "a keystroke hid an unsaved-work error"
        );
    }

    /// **A file changed on disk is noticed when the window comes back**, and
    /// the prompt's answers are buttons and keys. The check had no caller.
    #[test]
    fn a_file_changed_elsewhere_is_noticed_on_focus_and_answered() {
        let (_scratch, path) = temp_path("md_focus_changed");
        std::fs::write(&path, "one\ntwo\n").unwrap();
        let mut app = App::new(1280.0, 800.0);
        app.open_file(&path).unwrap();
        app.active_document_mut().insert_char('!');
        // Rewritten elsewhere, with a different mtime.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();

        app.on_event(&guitk::event::Event::FocusIn);
        assert!(
            app.external_prompt.is_some(),
            "coming back to the window noticed nothing"
        );
        // Typing does not reach the document behind the prompt.
        probe::type_str(&mut app, "z");
        assert_eq!(app.active_document().full_text(), "!one\ntwo");

        probe::click(&mut app, Target::External(ExternalChoice::Reload));
        assert!(app.external_prompt.is_none());
        // The disk version as `normalize_content` gives it: LF, and no
        // trailing newline to become an empty last line.
        assert_eq!(app.active_document().full_text(), "one\ntwo\nthree");
    }

    /// A deleted file is offered what makes sense for one: keep editing, or
    /// close. "Reload from disk" had nothing to reload and did nothing.
    #[test]
    fn a_deleted_file_offers_keep_or_close() {
        let (_scratch, path) = temp_path("md_focus_deleted");
        std::fs::write(&path, "gone soon").unwrap();
        let mut app = App::new(1280.0, 800.0);
        app.open_file(&path).unwrap();
        app.active_document_mut().insert_char('!');
        std::fs::remove_file(&path).unwrap();
        app.on_event(&guitk::event::Event::FocusIn);
        assert!(app.external_prompt.is_some());
        assert!(!probe::is_visible(
            &app,
            Target::External(ExternalChoice::Reload)
        ));
        // R is not an answer here, and does nothing.
        probe::key(&mut app, &probe::press(guitk::event::Key::R));
        assert!(app.external_prompt.is_some());
        probe::click(&mut app, Target::External(ExternalChoice::Close));
        assert!(app.external_prompt.is_none());
        assert_eq!(app.documents.count(), 1);
        assert!(
            app.active_document().path.is_none(),
            "the deleted file's tab is still open"
        );
    }

    /// **The merge review's answers are buttons and keys.** Its footer read
    /// "[Accept]  [Cancel]" in brackets that were text.
    #[test]
    fn the_merge_review_is_answered_by_pointer_and_keys() {
        let (_scratch, path) = temp_path("md_review_ptr");
        std::fs::write(&path, "a\nb\nc\n").unwrap();
        let mut app = App::new(1280.0, 800.0);
        app.open_file(&path).unwrap();
        app.active_document_mut().lines = vec!["a".into(), "OURS".into(), "c".into()];
        app.active_document_mut().modified = true;
        std::fs::write(&path, "a\nDISK\nc\n").unwrap();
        app.on_event(&guitk::event::Event::FocusIn);

        probe::key(&mut app, &probe::press(guitk::event::Key::V));
        assert!(
            app.external_prompt
                .as_ref()
                .is_some_and(|p| p.review.is_some()),
            "V did not open the review"
        );
        probe::click(&mut app, Target::ReviewPick(0, ConflictChoice::Ours));
        probe::click(&mut app, Target::ReviewAccept);
        assert!(app.external_prompt.is_none(), "Accept left the review up");
        assert!(app.active_document().full_text().contains("OURS"));
        assert!(!app.active_document().full_text().contains("DISK"));
    }

    /// The help card goes away on a press, as well as on F1 and Esc.
    #[test]
    fn a_press_puts_the_shortcut_card_away() {
        let mut app = App::new(1280.0, 800.0);
        probe::key(&mut app, &probe::press(guitk::event::Key::F1));
        assert!(app.show_help);
        probe::click(&mut app, Target::HelpCard);
        assert!(!app.show_help);
    }

    // --- Keys that reach what the pointer reaches ---------------------------

    #[test]
    fn ctrl_tab_and_ctrl_w_reach_the_tabs() {
        let mut app = App::new(1280.0, 800.0);
        app.new_document();
        app.new_document();
        assert_eq!(app.active_doc(), 2);
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::Tab));
        assert_eq!(
            app.active_doc(),
            0,
            "Ctrl+Tab does not wrap to the first document"
        );
        probe::key(&mut app, &ctrl_shift(guitk::event::Key::Tab));
        assert_eq!(app.active_doc(), 2, "Ctrl+Shift+Tab does not go back");
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::W));
        assert_eq!(app.documents.count(), 2);
    }

    #[test]
    fn ctrl_e_cycles_the_view() {
        let mut app = App::new(1280.0, 800.0);
        let before = app.view_mode;
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::E));
        assert_eq!(app.view_mode, before.next());
        // Preview only: the source pane is not drawn, so it cannot be pressed.
        while app.view_mode != ViewMode::PreviewOnly {
            probe::key(&mut app, &probe::ctrl(guitk::event::Key::E));
        }
        assert!(!probe::is_visible(&app, Target::Editor));
        assert!(probe::is_visible(&app, Target::Preview));
    }

    #[test]
    fn ctrl_digits_set_a_heading_level() {
        let mut app = App::new(1280.0, 800.0);
        app.active_document_mut().lines = vec!["Title".to_string()];
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::Num3));
        assert_eq!(app.active_document().line_text(0), "### Title");
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::Num1));
        assert_eq!(
            app.active_document().line_text(0),
            "# Title",
            "re-levelled by stacking"
        );
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::Num1));
        assert_eq!(
            app.active_document().line_text(0),
            "Title",
            "the same level again is off"
        );
    }

    #[test]
    fn shift_arrows_select_and_typing_replaces_the_selection() {
        let mut app = app_with_text();
        for _ in 0..5 {
            probe::key(&mut app, &probe::shift(guitk::event::Key::Right));
        }
        assert_eq!(
            app.active_document().selected_text().as_deref(),
            Some("alpha")
        );
        probe::type_str(&mut app, "A");
        assert_eq!(app.active_document().line_text(0), "A beta");
        assert!(!app.active_document().has_selection());

        // Left without Shift on a selection lands on its start.
        probe::key(&mut app, &probe::press(guitk::event::Key::End));
        probe::key(&mut app, &probe::shift(guitk::event::Key::Left));
        probe::key(&mut app, &probe::shift(guitk::event::Key::Left));
        probe::key(&mut app, &probe::press(guitk::event::Key::Left));
        assert_eq!(app.active_document().cursor_col, "A be".len());
        assert!(!app.active_document().has_selection());
    }

    #[test]
    fn cut_copy_and_paste_move_text() {
        let mut app = app_with_text();
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::A));
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::C));
        assert_eq!(app.clipboard, "alpha beta\nbeta gamma\ndelta");
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::X));
        assert_eq!(app.active_document().full_text(), "");
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::V));
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::V));
        assert_eq!(
            app.active_document().full_text(),
            "alpha beta\nbeta gamma\ndeltaalpha beta\nbeta gamma\ndelta"
        );
        // Copying nothing keeps what was copied.
        probe::key(&mut app, &probe::ctrl(guitk::event::Key::C));
        assert_eq!(app.clipboard, "alpha beta\nbeta gamma\ndelta");
    }

    // --- The document model under a real selection -------------------------

    /// **Undoing a multi-line delete restores the lines as lines.** The undo
    /// inserted the deleted text, newlines and all, into one line -- which
    /// then drew, counted and moved as one line.
    #[test]
    fn a_multi_line_delete_undoes_and_redoes_as_lines() {
        let mut doc = Document::new();
        doc.lines = vec!["one".into(), "two".into(), "three".into(), "four".into()];
        doc.selection_anchor = Some((0, 1));
        doc.cursor_line = 2;
        doc.cursor_col = 2;
        assert_eq!(doc.delete_selection().as_deref(), Some("ne\ntwo\nth"));
        assert_eq!(doc.lines, vec!["oree".to_string(), "four".to_string()]);

        doc.undo();
        assert_eq!(doc.lines, vec!["one", "two", "three", "four"]);
        assert!(doc.lines.iter().all(|l| !l.contains('\n')));
        assert_eq!((doc.cursor_line, doc.cursor_col), (2, 2));

        doc.redo();
        assert_eq!(doc.lines, vec!["oree".to_string(), "four".to_string()]);
        doc.undo();
        assert_eq!(doc.lines, vec!["one", "two", "three", "four"]);
    }

    /// The selection's delete records the column it actually deleted at: a
    /// column inside a character is floored, and undo puts the text back
    /// where it came from.
    #[test]
    fn a_selection_off_a_boundary_undoes_in_place() {
        let mut doc = Document::new();
        doc.lines = vec!["é-x".into()];
        doc.selection_anchor = Some((0, 1)); // inside the é
        doc.cursor_line = 0;
        doc.cursor_col = 2;
        assert_eq!(doc.delete_selection().as_deref(), Some("é"));
        assert_eq!(doc.lines, vec!["-x".to_string()]);
        doc.undo();
        assert_eq!(doc.lines, vec!["é-x".to_string()]);
    }

    #[test]
    fn a_heading_level_changes_in_one_undo_step() {
        let mut doc = Document::new();
        doc.lines = vec!["## Title".into()];
        doc.cursor_col = 5;
        set_heading(&mut doc, 4);
        assert_eq!(doc.line_text(0), "#### Title");
        assert_eq!(doc.cursor_col, 7, "the caret left the character it was on");
        doc.undo();
        assert_eq!(doc.line_text(0), "## Title");
    }

    #[test]
    fn word_bounds_find_words_and_single_characters() {
        assert_eq!(word_bounds("alpha beta", 2), (0, 5));
        assert_eq!(word_bounds("alpha beta", 5), (5, 6), "the space alone");
        assert_eq!(
            word_bounds("alpha beta", 10),
            (10, 10),
            "nothing past the end"
        );
        assert_eq!(word_bounds("日本語 x", 3), (0, "日本語".len()));
        assert_eq!(word_bounds("snake_case!", 0), (0, 10));
    }

    /// `col_at` is `col_x` inverted: every boundary of a mixed-width line
    /// comes back from its own drawn position.
    #[test]
    fn the_column_under_a_point_is_the_one_drawn_there() {
        let line = "a日b本c";
        let text_x = 100.0;
        for (col, _) in line
            .char_indices()
            .chain(std::iter::once((line.len(), ' ')))
        {
            let x = col_x(line, col, text_x);
            assert_eq!(col_at(line, x + 0.25, text_x), col, "boundary {col}");
        }
        assert_eq!(col_at(line, 0.0, text_x), 0, "left of the text is column 0");
        assert_eq!(col_at(line, 1.0e6, text_x), line.len());
    }
}
