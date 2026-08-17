//! Slate OS Text Editor
//!
//! Graphical text editor with:
//! - Multi-file editing with tabs
//! - Syntax highlighting for common languages
//! - Line numbers
//! - Find & replace (with regex support)
//! - Undo/redo (unlimited history)
//! - Word wrap or horizontal scroll
//! - Status bar (line, column, encoding, line ending)
//! - Keyboard shortcuts (Ctrl+S save, Ctrl+Z undo, Ctrl+F find, etc.)
//! - Auto-indent
//! - Configurable tab width
//!
//! Uses the guitk library for UI rendering.

mod highlight;
mod syntree;

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderTree};
use guitk::tabs::Tabs;
use guitk::text;
use highlight::{DEFAULT_THEME, HighlightState, StyledToken, Token};
use syntree::{Pos, SyntaxTree};

use diffcore::{
    ConflictChoice, DiskChange, FileSync, MergeOutcome, MergeReview, ThreeWayMerge,
    normalize_content,
};

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;

// ============================================================================
// Document buffer
// ============================================================================

/// A single text document.
pub struct Document {
    /// Lines of text.
    pub lines: Vec<String>,
    /// File path (None for untitled).
    pub path: Option<PathBuf>,
    /// Display name (filename or "Untitled").
    pub name: String,
    /// Whether the document has unsaved changes.
    pub modified: bool,
    /// Cursor line (0-based).
    pub cursor_line: usize,
    /// Cursor column (0-based, byte offset in line).
    pub cursor_col: usize,
    /// Selection anchor (line, col) — None if no selection.
    pub selection_anchor: Option<(usize, usize)>,
    /// Scroll offset (first visible line).
    pub scroll_line: usize,
    /// Horizontal scroll offset — a **byte** offset into the line, the same
    /// units as [`Document::cursor_col`].
    ///
    /// It has to be, because the two are subtracted from one another to place
    /// the caret and to clip a highlighted token to the visible part of its
    /// line. A character count and a byte offset only agree on ASCII, and the
    /// disagreement is silent: the caret lands beside the character it is on,
    /// by one pixel per byte of accumulated difference.
    pub scroll_col: usize,
    /// Undo history.
    pub undo_stack: VecDeque<EditAction>,
    /// Redo history.
    pub redo_stack: VecDeque<EditAction>,
    /// Line ending style.
    pub line_ending: LineEnding,
    /// Tab width (spaces).
    pub tab_width: usize,
    /// Whether to use spaces for tabs.
    pub use_spaces: bool,
    /// Detected language for syntax highlighting.
    pub language: Language,
    /// External-change tracker: records the last loaded/saved content and mtime
    /// so edits made to the file by other programs can be detected and merged.
    pub sync: FileSync,
    /// Memoized syntax state, one entry per line: `hl_entry[i]` is the state the
    /// highlighter is in *entering* line `i`, so `hl_entry[0]` is always
    /// [`HighlightState::Normal`]. Only a prefix is stored; anything past
    /// `hl_entry.len()` has not been computed yet.
    ///
    /// **Why this cache exists at all.** A block comment opened on line 3 colours
    /// line 4000, so the state entering the first *visible* line is a function of
    /// every line above it. Without a memo, drawing a screen 4000 lines down a
    /// file would re-tokenize those 4000 lines on every frame — for a caret
    /// blink, for a mouse move, for nothing. With it, scrolling down one line
    /// tokenizes one line.
    ///
    /// **Why it is not `pub`, and why `RefCell`.** Rendering takes `&self` (it
    /// produces a command list and changes nothing the user can see), but it is
    /// the only place that knows how far down the file the memo needs to reach.
    /// The alternative — computing states eagerly on every edit — does work
    /// proportional to the whole file per keystroke, which is the cost this
    /// exists to avoid.
    ///
    /// **Invariant, and the one way to break it:** every mutation of `lines` or
    /// `language` must call [`Document::invalidate_highlight`] with the first
    /// line it touched. `lines` is `pub`, so code outside this module can in
    /// principle edit it without saying so; `set_lines_from_text` and the editing
    /// operations below all do say so, and
    /// `the_syntax_cache_agrees_with_a_recomputation_after_every_edit` walks each
    /// of them and checks the memo against a from-scratch answer.
    hl_entry: RefCell<Vec<HighlightState>>,
}

/// One undoable edit, recorded as *what the affected lines were* and *what they
/// became*.
///
/// This deliberately does not describe the edit — no "inserted this character
/// at this column", no per-operation variant. It stores the two states of a
/// contiguous run of lines, so undo is `after → before` and redo is
/// `before → after`, and both are exact by construction.
///
/// The previous design was a four-variant enum naming the operation, and it was
/// wrong in three separate ways at once, all of them the same mistake: an
/// operation's *description* has to be kept in agreement with what the
/// operation actually does, and nothing enforces that agreement.
///
/// - Pressing Enter recorded `Insert { text: "\n" }`, and undo reverted it by
///   deleting one byte from the line — but by then the newline was not in any
///   line, the split had already moved the tail into a new entry. Undo deleted
///   an unrelated character and left the file split.
/// - Enter's auto-indent copied the previous line's leading whitespace into the
///   new line and recorded nothing at all, so those bytes could not be undone.
/// - `InsertLine` was matched by both `undo` and `redo` and constructed by
///   nothing, which is exactly the state a describe-the-operation model rots
///   into.
///
/// Storing the text costs two copies of each touched line per edit. For a
/// 1000-entry stack of single-character edits on 80-column lines that is on the
/// order of 100 KiB — the price of an undo stack that cannot disagree with the
/// buffer, which is the only property that matters here.
#[derive(Clone, Debug)]
pub struct EditAction {
    /// Index of the first line the edit replaced.
    line: usize,
    /// The lines occupying `line..line + before.len()` before the edit.
    before: Vec<String>,
    /// The lines occupying `line..line + after.len()` after it.
    after: Vec<String>,
    /// Caret position before the edit; where undo puts it back.
    cursor_before: (usize, usize),
    /// Caret position after the edit; where redo puts it back.
    cursor_after: (usize, usize),
}

/// Line ending style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::CrLf => "CRLF",
        }
    }

    pub fn chars(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

/// Language detection for syntax highlighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Plain,
    Rust,
    C,
    Python,
    JavaScript,
    Html,
    Css,
    Shell,
    Toml,
    Yaml,
    Json,
    Markdown,
}

impl Language {
    /// Detect language from file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Self::Rust,
            "c" | "h" | "cpp" | "hpp" | "cc" => Self::C,
            "py" => Self::Python,
            "js" | "ts" | "jsx" | "tsx" => Self::JavaScript,
            "html" | "htm" => Self::Html,
            "css" | "scss" => Self::Css,
            "sh" | "bash" | "zsh" => Self::Shell,
            "toml" => Self::Toml,
            "yaml" | "yml" => Self::Yaml,
            "json" => Self::Json,
            "md" | "markdown" => Self::Markdown,
            _ => Self::Plain,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Plain => "Plain Text",
            Self::Rust => "Rust",
            Self::C => "C/C++",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::Shell => "Shell",
            Self::Toml => "TOML",
            Self::Yaml => "YAML",
            Self::Json => "JSON",
            Self::Markdown => "Markdown",
        }
    }
}

/// The nearest character boundary in `line` at or before byte offset `col`.
///
/// A column carried from one line to another — pressing Down, or re-clamping
/// after the document shrinks — is a byte offset that meant something on the
/// *old* line. On the new one it may land in the middle of a multi-byte
/// character, and every subsequent `String::insert`/`remove` at that offset
/// panics. Moving *back* to the boundary rather than forward keeps the cursor
/// on the character the user was over rather than skipping past it.
fn snap_to_boundary(line: &str, col: usize) -> usize {
    let mut col = col.min(line.len());
    // Terminates: offset 0 is always a boundary.
    while !line.is_char_boundary(col) {
        col -= 1;
    }
    col
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
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
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            line_ending: LineEnding::Lf,
            tab_width: 4,
            use_spaces: true,
            language: Language::Plain,
            sync: FileSync::new(),
            hl_entry: RefCell::new(Vec::new()),
        }
    }

    pub fn from_file(path: &std::path::Path) -> std::io::Result<Self> {
        let content = fs::read_to_string(path)?;

        let line_ending = if content.contains("\r\n") {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        };

        let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string());

        let language = highlight::language_of_path(path);

        // Record the load-time snapshot (LF-normalized, matching our in-memory
        // representation) and mtime so we can later detect external edits and
        // three-way merge against this common ancestor.
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
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            line_ending,
            tab_width: 4,
            use_spaces: true,
            language,
            sync,
            hl_entry: RefCell::new(Vec::new()),
        })
    }

    /// The LF-normalized text of the current buffer.
    ///
    /// This is the canonical form used for diffing/merging: it matches
    /// [`normalize_content`] applied to on-disk bytes, so a freshly loaded and
    /// unedited buffer compares equal to its file.
    #[must_use]
    pub fn buffer_text(&self) -> String {
        self.lines.join("\n")
    }

    /// Save the document to its file path.
    ///
    /// Written through [`safeio::write_str_atomically`] rather than
    /// `fs::write`. `fs::write` truncates the target *before* writing it, so a
    /// save interrupted part-way — a full disk, a removed drive, a killed
    /// process, a power loss — left the user's document on disk as a fragment
    /// or as nothing at all. For a text editor that is the worst possible
    /// failure, because the file *is* the user's only copy.
    pub fn save(&mut self) -> std::io::Result<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| std::io::Error::other("no file path"))?;

        let content: String = self.lines.join(self.line_ending.chars());
        safeio::write_str_atomically(path, &content)?;
        self.modified = false;
        // Refresh the merge ancestor and mtime so the file we just wrote is not
        // mistaken for an external change on the next check.
        let text = self.buffer_text();
        self.sync.record(path, text);
        Ok(())
    }

    /// Save to a new path.
    pub fn save_as(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        self.path = Some(path.to_path_buf());
        self.name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string());
        self.language = highlight::language_of_path(path);
        // The language decides the colours, so every memoized state is now an
        // answer to a different question.
        self.invalidate_highlight(0);
        self.save()
    }

    // ======================================================================
    // External-change detection & three-way merge
    // ======================================================================

    /// Check whether the file backing this document has changed on disk since
    /// it was last loaded or saved.
    ///
    /// Delegates to the shared [`FileSync`] tracker, which uses the recorded
    /// mtime as a cheap pre-filter and only re-reads content when it differs.
    /// Returns [`DiskChange::Unchanged`] for buffers with no backing file.
    #[must_use]
    pub fn disk_changed(&self) -> DiskChange {
        match self.path.as_ref() {
            Some(path) => self.sync.changed(path),
            None => DiskChange::Unchanged,
        }
    }

    /// Dismiss an external change, keeping the current buffer as-is.
    ///
    /// Records the disk's current mtime so the same external edit is not
    /// re-reported. The buffer stays modified and will overwrite the file on the
    /// next save. The merge ancestor is intentionally left unchanged so a later
    /// merge still diffs against the original common ancestor.
    pub fn keep_current(&mut self) {
        if let Some(path) = self.path.clone() {
            self.sync.touch(&path);
        }
    }

    /// Replace the buffer with the on-disk content, discarding local edits.
    ///
    /// `disk` is the LF-normalized disk content (as produced by
    /// [`DiskChange::Modified`]). Resets the modified flag, refreshes the merge
    /// ancestor/mtime, clears undo history (the reload is not itself undoable),
    /// and clamps the cursor into the new bounds.
    pub fn reload_from_disk(&mut self, disk: &str) {
        self.set_lines_from_text(disk);
        self.modified = false;
        if let Some(path) = self.path.clone() {
            self.sync.record(&path, disk.to_string());
        } else {
            self.sync.base = Some(disk.to_string());
        }
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Compute the three-way merge of the current buffer against `disk`.
    ///
    /// `base` = last loaded/saved content, `ours` = current buffer, `theirs` =
    /// `disk`. When the ancestor is unknown (never saved), the disk content is
    /// used as the ancestor, which degrades gracefully to a two-way merge.
    #[must_use]
    pub fn merge_preview(&self, disk: &str) -> ThreeWayMerge {
        self.sync.merge(&self.buffer_text(), disk)
    }

    /// Auto-merge the on-disk changes into the buffer.
    ///
    /// Non-conflicting changes from both sides are combined automatically. If
    /// the merge is clean the buffer becomes the merged result; if it conflicts,
    /// the buffer is filled with Git-style conflict markers for manual
    /// resolution. In both cases the buffer is marked modified (it now differs
    /// from disk and must be saved) and the merge ancestor advances to `disk`.
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

    /// Apply an already-resolved merge result to the buffer.
    ///
    /// Used by the review flow after the user has chosen per-conflict
    /// resolutions. `disk` becomes the new merge ancestor.
    pub fn apply_merged(&mut self, merged: &str, disk: &str) {
        self.set_lines_from_text(merged);
        self.modified = true;
        // Their changes are now incorporated, so the disk content is the new
        // common ancestor; the buffer is "ours" relative to it and needs saving.
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
        // Clamp the cursor into the new bounds.
        let last_line = self.lines.len().saturating_sub(1);
        if self.cursor_line > last_line {
            self.cursor_line = last_line;
        }
        // Snapped rather than merely clamped to the length: the line the cursor
        // has been moved to may hold a multi-byte character straddling the old
        // column, and a `cursor_col` inside one panics the next edit.
        self.cursor_col = self
            .lines
            .get(self.cursor_line)
            .map_or(0, |line| snap_to_boundary(line, self.cursor_col));
        if self.scroll_line > last_line {
            self.scroll_line = last_line;
        }
        self.invalidate_highlight(0);
    }

    // ======================================================================
    // Syntax state
    // ======================================================================

    /// Discard the memoized syntax state for `from_line` and everything below.
    ///
    /// Takes the *first* line whose text changed. The state entering that line
    /// is still whatever it was — the lines above it did not move — so the entry
    /// for `from_line` survives and only the ones after it are dropped. Passing a
    /// line earlier than the edit is merely wasteful; passing a later one is a
    /// bug, and shows up as text that keeps a colour it has stopped deserving.
    fn invalidate_highlight(&mut self, from_line: usize) {
        let keep = from_line.saturating_add(1);
        let entries = self.hl_entry.get_mut();
        if entries.len() > keep {
            entries.truncate(keep);
        }
    }

    /// The highlighter's state entering `line`, computing and memoizing whatever
    /// prefix of the file is still missing.
    ///
    /// Costs one tokenization per not-yet-computed line, so the first draw of a
    /// file pays for everything above the viewport once and scrolling pays one
    /// line at a time.
    fn entry_state(&self, line: usize) -> HighlightState {
        let mut entries = self.hl_entry.borrow_mut();
        if entries.is_empty() {
            entries.push(HighlightState::Normal);
        }
        while entries.len() <= line {
            let index = entries.len() - 1;
            // `last()` rather than an index: the vector is never empty here — it
            // was seeded just above and only grows in this loop.
            let mut state = entries.last().cloned().unwrap_or(HighlightState::Normal);
            let source = self.lines.get(index).map_or("", String::as_str);
            drop(highlight::highlight_line(source, self.language, &mut state));
            entries.push(state);
        }
        entries.get(line).cloned().unwrap_or(HighlightState::Normal)
    }

    // ======================================================================
    // Editing operations
    // ======================================================================

    /// Run `edit`, recording what it did to `lines[line..]` so it can be undone
    /// exactly.
    ///
    /// `before_count` is how many lines starting at `line` the edit may touch —
    /// 1 for an edit within a line, 2 for one that joins two. The count
    /// *afterwards* is not asked for, because it is not something a caller
    /// should have to get right: it is derived from how the buffer's total
    /// length changed, which is a fact rather than a claim. That is the whole
    /// discipline here — every call site states only what it is about to touch,
    /// and the recording is taken from the buffer itself, so an edit cannot
    /// describe itself incorrectly.
    ///
    /// This is also where the redo stack is cleared, so it happens for every
    /// edit rather than only the ones whose author remembered. Before, only
    /// `insert_char` cleared it, and a backspace after an undo left a redo
    /// entry that would re-apply an edit on top of a buffer that had moved on.
    fn record_edit<R>(
        &mut self,
        line: usize,
        before_count: usize,
        edit: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let cursor_before = (self.cursor_line, self.cursor_col);
        let before = Self::snapshot(&self.lines, line, before_count);
        let old_len = self.lines.len();

        let result = edit(self);

        // The edit replaced `before_count` lines with however many the buffer
        // grew or shrank by, relative to that.
        let after_count = before
            .len()
            .saturating_add(self.lines.len())
            .saturating_sub(old_len);
        let after = Self::snapshot(&self.lines, line, after_count);

        self.modified = true;
        self.invalidate_highlight(line);
        self.redo_stack.clear();
        self.push_undo(EditAction {
            line,
            before,
            after,
            cursor_before,
            cursor_after: (self.cursor_line, self.cursor_col),
        });
        result
    }

    /// `lines[at..at + count]`, clamped, cloned.
    fn snapshot(lines: &[String], at: usize, count: usize) -> Vec<String> {
        let start = at.min(lines.len());
        let end = start.saturating_add(count).min(lines.len());
        lines.get(start..end).unwrap_or_default().to_vec()
    }

    /// Replace `lines[at..at + remove]` with `insert`.
    fn splice_lines(&mut self, at: usize, remove: usize, insert: &[String]) {
        let start = at.min(self.lines.len());
        let end = start.saturating_add(remove).min(self.lines.len());
        self.lines
            .splice(start..end, insert.iter().cloned())
            .for_each(drop);
        self.invalidate_highlight(at);
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, ch: char) {
        let line = self.cursor_line;
        let col = self.cursor_col;

        self.record_edit(line, 1, |doc| {
            if ch == '\n' {
                // Split the line. `col` is a byte offset and is snapped first,
                // because `split_at` panics rather than rounds when it lands
                // inside a character.
                let current_line = doc.lines.get(line).cloned().unwrap_or_default();
                let at = snap_to_boundary(&current_line, col);
                let (head, tail) = current_line.split_at(at);

                // Auto-indent: the new line starts with the old line's leading
                // whitespace. This is part of the same recorded edit, so undo
                // takes it back with the split rather than leaving it behind.
                let indent: String = head.chars().take_while(|c| c.is_whitespace()).collect();
                doc.cursor_col = indent.len();
                let tail = format!("{indent}{tail}");

                if let Some(slot) = doc.lines.get_mut(line) {
                    slot.truncate(at);
                }
                doc.lines.insert(line.saturating_add(1), tail);
                doc.cursor_line = line.saturating_add(1);
            } else if ch == '\t' && doc.use_spaces {
                // Insert spaces instead of a tab.
                let spaces = " ".repeat(doc.tab_width.saturating_sub(col % doc.tab_width));
                if let Some(current_line) = doc.lines.get_mut(line) {
                    let at = snap_to_boundary(current_line, col);
                    current_line.insert_str(at, &spaces);
                    doc.cursor_col = at.saturating_add(spaces.len());
                }
            } else if let Some(current_line) = doc.lines.get_mut(line) {
                let at = snap_to_boundary(current_line, col);
                current_line.insert(at, ch);
                doc.cursor_col = at.saturating_add(ch.len_utf8());
            }
        });
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if let Some(ch) = self.char_before_cursor() {
            let line = self.cursor_line;
            // Step back by the character's width in bytes, not by one:
            // `cursor_col` is a byte offset, and `String::remove` panics on an
            // offset that is not a character boundary.
            let col = self.cursor_col.saturating_sub(ch.len_utf8());
            self.record_edit(line, 1, |doc| {
                if let Some(current_line) = doc.lines.get_mut(line)
                    && col < current_line.len()
                    && current_line.is_char_boundary(col)
                {
                    current_line.remove(col);
                }
                doc.cursor_col = col;
            });
        } else if self.cursor_line > 0 {
            // Join with the previous line. Both lines are in the recorded
            // range, because the join changes both of them.
            let line = self.cursor_line.saturating_sub(1);
            self.record_edit(line, 2, |doc| {
                let current_text = doc.lines.remove(doc.cursor_line);
                doc.cursor_line = line;
                doc.cursor_col = doc.lines.get(line).map_or(0, String::len);
                if let Some(previous) = doc.lines.get_mut(line) {
                    previous.push_str(&current_text);
                }
            });
        }
    }

    /// Delete the character at the cursor (delete key).
    pub fn delete_forward(&mut self) {
        let line = self.cursor_line;
        let col = self.cursor_col;
        let line_len = self.lines.get(line).map_or(0, String::len);

        if col < line_len {
            self.record_edit(line, 1, |doc| {
                if let Some(current_line) = doc.lines.get_mut(line)
                    && current_line.is_char_boundary(col)
                {
                    current_line.remove(col);
                }
            });
        } else if line.saturating_add(1) < self.lines.len() {
            // Join with the next line; again both lines are recorded.
            self.record_edit(line, 2, |doc| {
                let next_text = doc.lines.remove(line.saturating_add(1));
                if let Some(current_line) = doc.lines.get_mut(line) {
                    current_line.push_str(&next_text);
                }
            });
        }
    }

    /// Undo the last action.
    pub fn undo(&mut self) {
        if let Some(action) = self.undo_stack.pop_back() {
            self.splice_lines(action.line, action.after.len(), &action.before);
            (self.cursor_line, self.cursor_col) = action.cursor_before;
            self.clamp_cursor();
            self.redo_stack.push_back(action);
            self.modified = true;
        }
    }

    /// Redo the last undone action.
    pub fn redo(&mut self) {
        if let Some(action) = self.redo_stack.pop_back() {
            self.splice_lines(action.line, action.before.len(), &action.after);
            (self.cursor_line, self.cursor_col) = action.cursor_after;
            self.clamp_cursor();
            self.undo_stack.push_back(action);
            self.modified = true;
        }
    }

    /// Pull the caret back inside the buffer and onto a character boundary.
    ///
    /// A recorded caret position was valid against the buffer state that is
    /// being restored, so this should never have anything to do — but "should
    /// never" is not a guarantee, and every later edit indexes a `String` by
    /// `cursor_col`, where being wrong is a panic rather than a wrong answer.
    fn clamp_cursor(&mut self) {
        self.cursor_line = self.cursor_line.min(self.lines.len().saturating_sub(1));
        let col = self.cursor_col;
        self.cursor_col = self
            .lines
            .get(self.cursor_line)
            .map_or(0, |line| snap_to_boundary(line, col.min(line.len())));
    }

    fn push_undo(&mut self, action: EditAction) {
        self.undo_stack.push_back(action);
        if self.undo_stack.len() > 1000 {
            self.undo_stack.pop_front();
        }
    }

    // ======================================================================
    // Cursor movement
    // ======================================================================

    /// Replace bytes `start..end` of line `n` with `text`. Returns whether
    /// anything was replaced.
    ///
    /// Find/replace records its matches against whichever document was
    /// searched, and nothing stops a caller handing a *different* document
    /// to `replace_all`, or editing the buffer between the search and the
    /// replace. `String::replace_range` panics on both of those — a range
    /// past the end, or one that lands inside a character — so the check
    /// lives here, once, instead of being forgotten at one of the call
    /// sites.
    fn replace_in_line(&mut self, n: usize, start: usize, end: usize, text: &str) -> bool {
        let Some(line) = self.lines.get_mut(n) else {
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

    /// The text of line `n`, or `""` if the buffer has no such line.
    ///
    /// Every caller wants "the characters on that line", and the only honest
    /// answer for a line that is not there is "none". Indexing instead took
    /// the whole editor down — and with it every unsaved buffer in every
    /// other tab, not just the one whose index had gone stale.
    fn line(&self, n: usize) -> &str {
        self.lines.get(n).map_or("", String::as_str)
    }

    /// The text of the line the cursor is on.
    fn cursor_line_text(&self) -> &str {
        self.line(self.cursor_line)
    }

    /// The character immediately before the cursor, if the cursor is not at the
    /// start of its line.
    ///
    /// `cursor_col` is a *byte* offset — `String::insert` and `String::remove`
    /// index by bytes — so every backward step must be the width of a character
    /// in bytes and never one byte. A one-byte step lands inside a `é` and the
    /// next edit panics, because both of those methods reject an offset that is
    /// not on a character boundary.
    fn char_before_cursor(&self) -> Option<char> {
        self.lines
            .get(self.cursor_line)?
            .get(..self.cursor_col)?
            .chars()
            .next_back()
    }

    /// The character the cursor sits on, if it is not at the end of its line.
    /// The forward counterpart of [`Document::char_before_cursor`], and byte
    /// offsets for the same reason.
    fn char_at_cursor(&self) -> Option<char> {
        self.lines
            .get(self.cursor_line)?
            .get(self.cursor_col..)?
            .chars()
            .next()
    }

    pub fn move_left(&mut self) {
        if let Some(ch) = self.char_before_cursor() {
            self.cursor_col -= ch.len_utf8();
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.cursor_line_text().len();
        }
    }

    pub fn move_right(&mut self) {
        if let Some(ch) = self.char_at_cursor() {
            self.cursor_col += ch.len_utf8();
        } else if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = snap_to_boundary(self.cursor_line_text(), self.cursor_col);
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = snap_to_boundary(self.cursor_line_text(), self.cursor_col);
        }
    }

    pub fn move_home(&mut self) {
        self.cursor_col = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor_col = self.cursor_line_text().len();
    }

    pub fn move_to_start(&mut self) {
        self.cursor_line = 0;
        self.cursor_col = 0;
    }

    pub fn move_to_end(&mut self) {
        self.cursor_line = self.lines.len().saturating_sub(1);
        self.cursor_col = self.cursor_line_text().len();
    }

    /// Total line count.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Ensure cursor is visible in the viewport.
    pub fn ensure_cursor_visible(&mut self, visible_lines: usize) {
        if self.cursor_line < self.scroll_line {
            self.scroll_line = self.cursor_line;
        } else if self.cursor_line >= self.scroll_line + visible_lines {
            self.scroll_line = self.cursor_line - visible_lines + 1;
        }
    }

    // ======================================================================
    // Structural editing (syntree-backed)
    // ======================================================================

    /// Build a fresh syntactic structure tree for the current buffer state.
    ///
    /// The tree is rebuilt on demand rather than cached on the document —
    /// document edits would invalidate any cached tree, and a full rebuild
    /// of a typical source file is fast enough that caching is not yet
    /// worth the bookkeeping complexity. When edits become the bottleneck,
    /// switch to incremental re-parsing of the affected line range.
    pub fn build_syntax_tree(&self) -> SyntaxTree {
        SyntaxTree::build(&self.lines, self.language)
    }

    /// Returns the depth-first outline of multi-line syntactic scopes.
    ///
    /// Each entry is `(depth, header)` where `header` is the trimmed source
    /// of the line that opens the scope. Suitable for an outline / document-
    /// symbol panel.
    pub fn outline(&self) -> Vec<(usize, String)> {
        self.build_syntax_tree().outline()
    }

    /// Returns `(start_line, end_line)` pairs for foldable multi-line scopes.
    pub fn fold_ranges(&self) -> Vec<(usize, usize)> {
        self.build_syntax_tree().fold_ranges()
    }

    /// Expand the current selection to the smallest enclosing syntactic
    /// scope. With no selection, snap to the scope containing the cursor.
    /// With a selection that already equals an enclosing scope, expand
    /// outward to that scope's parent. Returns `true` if the selection
    /// changed.
    ///
    /// This is the editor's structural-selection primitive (the
    /// Ctrl+Shift+A / Alt+Up gesture in IDEs that integrate tree-sitter).
    pub fn expand_selection(&mut self) -> bool {
        let tree = self.build_syntax_tree();
        let (sel_start, sel_end) = self.selection_range();
        // Find the smallest node enclosing the current selection.
        let mut idx = tree.enclosing_range(sel_start, sel_end);
        // If the selection already equals this node's range, expand to its
        // parent (so repeated invocations grow outward through the tree).
        let Some(node) = tree.node(idx) else {
            return false;
        };
        let at_node_bounds = node.start == sel_start && node.end == sel_end;
        if at_node_bounds {
            if let Some(p) = node.parent {
                idx = p;
            } else {
                return false; // already at the root
            }
        }
        let Some(target) = tree.node(idx) else {
            return false;
        };
        // Don't snap to the synthetic root if there's nothing useful there.
        if target.kind == syntree::NodeKind::Root && target.children.is_empty() {
            return false;
        }
        let new_start = target.start;
        let new_end = target.end;
        if (new_start, new_end) == (sel_start, sel_end) {
            return false;
        }
        self.set_selection(new_start, new_end);
        true
    }

    /// Returns the current selection as a `(start, end)` byte-position pair,
    /// where `start <= end`. With no selection, both equal the cursor.
    fn selection_range(&self) -> (Pos, Pos) {
        let cursor = Pos::new(self.cursor_line, self.cursor_col);
        match self.selection_anchor {
            Some((al, ac)) => {
                let anchor = Pos::new(al, ac);
                if anchor <= cursor {
                    (anchor, cursor)
                } else {
                    (cursor, anchor)
                }
            }
            None => (cursor, cursor),
        }
    }

    /// Set the selection so the anchor is at `start` and the cursor at `end`.
    fn set_selection(&mut self, start: Pos, end: Pos) {
        self.selection_anchor = Some((start.line, start.col));
        self.cursor_line = end.line;
        self.cursor_col = end.col;
    }
}

// ============================================================================
// Find & Replace
// ============================================================================

pub struct FindState {
    pub query: String,
    pub replace_text: String,
    pub case_sensitive: bool,
    pub use_regex: bool,
    pub matches: Vec<(usize, usize, usize)>, // (line, start_col, end_col)
    pub current_match: usize,
}

impl Default for FindState {
    fn default() -> Self {
        Self::new()
    }
}

impl FindState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            replace_text: String::new(),
            case_sensitive: false,
            use_regex: false,
            matches: Vec::new(),
            current_match: 0,
        }
    }

    /// Find all occurrences in the document.
    pub fn find_all(&mut self, doc: &Document) {
        self.matches.clear();
        if self.query.is_empty() {
            return;
        }

        for (line_idx, line) in doc.lines.iter().enumerate() {
            let mut start = 0;
            while let Some(rest) = line.get(start..) {
                let hit = if self.case_sensitive {
                    rest.find(&self.query).map(|p| {
                        let at = start.saturating_add(p);
                        (at, at.saturating_add(self.query.len()))
                    })
                } else {
                    rest.char_indices().find_map(|(off, _)| {
                        let at = start.saturating_add(off);
                        folded_match_end(line, at, &self.query).map(|end| (at, end))
                    })
                };
                let Some((at, end)) = hit else { break };
                self.matches.push((line_idx, at, end));
                // Resume *past* the match, not one byte into it. Stepping one
                // byte reported overlapping occurrences -- three "aa" in
                // "aaaa" -- and `replace_all` then rewrote overlapping ranges
                // one after another, shredding the line. A one-byte step also
                // lands inside a multi-byte character, where the old
                // `search_line[start..]` panicked outright.
                start = if end > at {
                    end
                } else {
                    at.saturating_add(1)
                };
            }
        }

        self.current_match = 0;
    }

    /// Go to next match.
    pub fn next_match(&mut self, doc: &mut Document) {
        if self.matches.is_empty() {
            return;
        }
        self.current_match = (self.current_match + 1) % self.matches.len();
        let Some(&(line, col, _)) = self.matches.get(self.current_match) else {
            return;
        };
        doc.cursor_line = line;
        doc.cursor_col = col;
    }

    /// Go to previous match.
    pub fn prev_match(&mut self, doc: &mut Document) {
        if self.matches.is_empty() {
            return;
        }
        if self.current_match == 0 {
            self.current_match = self.matches.len().saturating_sub(1);
        } else {
            self.current_match -= 1;
        }
        let Some(&(line, col, _)) = self.matches.get(self.current_match) else {
            return;
        };
        doc.cursor_line = line;
        doc.cursor_col = col;
    }

    /// Replace current match.
    pub fn replace_current(&mut self, doc: &mut Document) {
        if self.matches.is_empty() {
            return;
        }
        let Some(&(line, start, end)) = self.matches.get(self.current_match) else {
            return;
        };
        if doc.replace_in_line(line, start, end, &self.replace_text) {
            doc.modified = true;
        }
        self.find_all(doc);
    }

    /// Replace all matches.
    pub fn replace_all(&mut self, doc: &mut Document) -> usize {
        if self.matches.is_empty() {
            return 0;
        }
        // Replace from end to start to preserve indices. The count is of
        // replacements actually made, not matches recorded: a match whose
        // range no longer fits the document is skipped, and reporting it as
        // replaced would be a lie the user can see in the buffer.
        let mut count = 0_usize;
        for &(line, start, end) in self.matches.iter().rev() {
            if doc.replace_in_line(line, start, end, &self.replace_text) {
                count = count.saturating_add(1);
            }
        }
        if count > 0 {
            doc.modified = true;
        }
        self.matches.clear();
        count
    }
}

// ============================================================================
// Editor state (multi-tab)
// ============================================================================

// The open documents, and which one is in front, live in
// `guitk::tabs::Tabs` — a `Vec<Document>` plus an `active: usize` left "at
// least one is open" and "the index names one" as conventions every call
// site had to keep, and both editors had broken them. The generic type is
// in the toolkit because the markdown editor needs exactly the same thing.


/// Byte offset just past a case-folded match of `needle` starting exactly at
/// byte offset `at` in `haystack`, or `None` if there is no match there.
///
/// The offsets are into `haystack` -- the *real* line -- which is the whole
/// point. Find used to search a `to_lowercase()` copy of the line instead, but
/// `to_lowercase` is not length-preserving (Turkish `I` with a dot above folds
/// to two characters, three bytes, from two), so an offset found in the folded
/// copy does not point at the same place in the line the user is looking at.
/// The editor then selected, or replaced, the wrong bytes -- or a span past
/// the end of the line, which `String::replace_range` turns into a panic.
///
/// The match length in `haystack` can differ from the needle's, for the same
/// reason, so the *end* is returned rather than assumed.
fn folded_match_end(haystack: &str, at: usize, needle: &str) -> Option<usize> {
    let rest = haystack.get(at..)?;
    let mut needle_folded = needle.chars().flat_map(char::to_lowercase);
    let mut want = needle_folded.next();
    let mut end = at;
    for (off, hc) in rest.char_indices() {
        if want.is_none() {
            break;
        }
        for hf in hc.to_lowercase() {
            match want {
                Some(nf) if nf == hf => want = needle_folded.next(),
                // A mismatch, or the needle running out partway through a
                // haystack character. Neither is a match we could select.
                _ => return None,
            }
        }
        end = at.saturating_add(off).saturating_add(hc.len_utf8());
    }
    want.is_none().then_some(end)
}

/// Complete editor application state.
pub struct EditorState {
    /// Open documents (tabs), and which is in front.
    pub tabs: Tabs<Document>,
    /// Find & replace state.
    pub find: FindState,
    /// Whether find panel is visible.
    pub find_visible: bool,
    /// Window dimensions.
    pub window_width: u32,
    pub window_height: u32,
    /// Line number gutter width.
    pub gutter_width: f32,
    /// Font size.
    pub font_size: f32,
    /// Line-to-line spacing of the text area.
    ///
    /// There is deliberately no matching `char_width`: horizontal positions
    /// come from measuring the text with `guitk::text`, because a nominal
    /// per-character width is only ever right for one font at one size, and
    /// wrong by a compounding amount along every line for all the others.
    pub line_height: f32,
    /// Pending external-change prompt (file edited/deleted outside the editor).
    pub external_prompt: Option<ExternalChangePrompt>,
}

/// A pending prompt shown when the active document's file changed on disk.
///
/// Presents the user with keep-current / reload / merge / review options
/// (see [`EditorState::resolve_external`]). When [`review`](Self::review) is
/// `Some`, the editor is in the side-by-side review sub-mode.
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

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorState {
    pub fn new() -> Self {
        let font_size = 14.0;
        Self {
            tabs: Tabs::new(),
            find: FindState::new(),
            find_visible: false,
            window_width: 900,
            window_height: 600,
            gutter_width: 50.0,
            font_size,
            line_height: font_size * 1.5,
            external_prompt: None,
        }
    }

    pub fn active_document(&self) -> &Document {
        self.tabs.active()
    }

    pub fn active_document_mut(&mut self) -> &mut Document {
        self.tabs.active_mut()
    }

    /// Open a file in a new tab.
    pub fn open_file(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        // Check if already open. The search is a separate statement so the
        // borrow of `tabs` ends before `set_active` takes a mutable one.
        let already_open = self
            .tabs
            .iter()
            .position(|doc| doc.path.as_deref() == Some(path));
        if let Some(i) = already_open {
            self.tabs.set_active(i);
            return Ok(());
        }

        let doc = Document::from_file(path)?;
        self.tabs.open(doc);
        Ok(())
    }

    /// Close the active tab.
    pub fn close_tab(&mut self) -> bool {
        if self.tabs.active().modified {
            // Would need to prompt user — return false to indicate unsaved
            return false;
        }
        self.tabs.close_active();
        true
    }

    /// Number of visible lines in the editor viewport.
    pub fn visible_lines(&self) -> usize {
        let editor_height = self.window_height as f32 - 64.0 - 24.0; // toolbar + status bar
        (editor_height / self.line_height) as usize
    }

    // ======================================================================
    // External-change handling
    // ======================================================================

    /// Check the active document's file for external modification and, if it
    /// changed in a way that needs the user's attention, raise a prompt.
    ///
    /// Policy: if the file changed on disk but the buffer has *no* unsaved edits,
    /// the disk version is loaded automatically (there is nothing to lose and
    /// nothing to decide). A prompt is raised only when there is a genuine
    /// choice to make — the buffer is modified and the file also changed, or the
    /// file was deleted. Returns `true` when a prompt was raised.
    pub fn check_external_change(&mut self) -> bool {
        // Don't stack prompts.
        if self.external_prompt.is_some() {
            return false;
        }
        let tab = self.tabs.active_index();
        let doc = self.tabs.active();
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
                    // No local edits at risk — just adopt the disk version.
                    if let Some(doc) = self.tabs.get_mut(tab) {
                        doc.reload_from_disk(&disk);
                    }
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
    /// [`ExternalChoice::Review`] transitions the prompt into review sub-mode
    /// (building a [`MergeReview`]); the other three choices resolve immediately
    /// and clear the prompt.
    pub fn resolve_external(&mut self, choice: ExternalChoice) {
        let Some(prompt) = self.external_prompt.as_ref() else {
            return;
        };
        let tab = prompt.tab;
        // "disk" is only meaningful for a Modified change.
        let disk = match &prompt.change {
            DiskChange::Modified { disk } => Some(disk.clone()),
            _ => None,
        };

        match choice {
            ExternalChoice::KeepCurrent => {
                if let Some(doc) = self.tabs.get_mut(tab) {
                    // For a deletion, there is no disk mtime to record; keep the
                    // buffer (marked modified) so a save recreates the file.
                    doc.keep_current();
                    doc.modified = true;
                }
                self.external_prompt = None;
            }
            ExternalChoice::Reload => {
                if let (Some(doc), Some(disk)) = (self.tabs.get_mut(tab), disk) {
                    doc.reload_from_disk(&disk);
                }
                self.external_prompt = None;
            }
            ExternalChoice::Merge => {
                if let (Some(doc), Some(disk)) = (self.tabs.get_mut(tab), disk) {
                    doc.merge_from_disk(&disk);
                }
                self.external_prompt = None;
            }
            ExternalChoice::Review => {
                if let (Some(doc), Some(disk)) = (self.tabs.get(tab), disk.as_ref()) {
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
        if let Some(doc) = self.tabs.get_mut(tab) {
            doc.apply_merged(&merged, &disk);
        }
        self.external_prompt = None;
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

    // ======================================================================
    // Rendering
    // ======================================================================

    /// Render the complete editor UI.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();
        let w = self.window_width as f32;
        let h = self.window_height as f32;

        // Background
        tree.fill_rect(0.0, 0.0, w, h, Color::from_hex(0x1E1E2E));

        // Tab bar
        self.render_tabs(&mut tree);

        // Editor area
        self.render_editor(&mut tree);

        // Status bar
        self.render_status_bar(&mut tree);

        // Find panel (if visible)
        if self.find_visible {
            self.render_find_panel(&mut tree);
        }

        // External-change prompt / merge review (modal overlay)
        if let Some(prompt) = self.external_prompt.as_ref() {
            self.render_external_prompt(&mut tree, prompt);
        }

        tree
    }

    fn render_tabs(&self, tree: &mut RenderTree) {
        let tab_h = 32.0;
        tree.fill_rect(0.0, 0.0, self.window_width as f32, tab_h, Color::from_hex(0x181825));

        let mut x = 0.0;
        for (i, doc) in self.tabs.iter().enumerate() {
            let tab_w = 160.0;
            let bg = if i == self.tabs.active_index() {
                Color::from_hex(0x1E1E2E)
            } else {
                Color::from_hex(0x11111B)
            };

            tree.fill_rect(x, 0.0, tab_w, tab_h, bg);

            // Tab title
            let title = if doc.modified {
                format!("\u{25CF} {}", doc.name) // bullet for modified
            } else {
                doc.name.clone()
            };
            tree.text(
                x + 12.0,
                9.0,
                &title,
                Color::from_hex(0xCDD6F4),
                12.0,
            );

            // Close button
            tree.text(x + tab_w - 20.0, 9.0, "x", Color::from_hex(0x6C7086), 11.0);

            x += tab_w + 1.0;
        }
    }

    /// The x at which a line's text starts: just right of the gutter.
    ///
    /// One definition rather than `gutter_width + 8.0` repeated, because the
    /// caret and the text it sits in must agree to the pixel — and they only
    /// agree by construction if they read the same number.
    fn text_x(&self) -> f32 {
        self.gutter_width + 8.0
    }

    /// Draw one line as a row of coloured runs, one per syntax token.
    ///
    /// `scroll_col` is a byte offset into `line`; tokens entirely left of it are
    /// skipped and the one straddling it is cut. Drawing stops once the pen has
    /// passed `right`, so a line thousands of columns wide costs a screenful of
    /// commands rather than one per token.
    ///
    /// Each run's x is the previous run's x plus that run's measured width,
    /// which is how the toolkit's own multi-span text (`guitk::textview`) is
    /// laid out. It forgoes kerning *across* a token boundary — but drawing the
    /// runs in separate commands already does, and a token boundary is almost
    /// always a change of character class, where there is no kern pair to lose.
    #[allow(clippy::too_many_arguments)]
    fn draw_tokens(
        tree: &mut RenderTree,
        line: &str,
        tokens: &[StyledToken],
        scroll_col: usize,
        left: f32,
        y: f32,
        right: f32,
        font_size: f32,
    ) {
        let start = snap_to_boundary(line, scroll_col);
        let mut x = left;
        // Where the last drawn run ended, so a token that begins after it — a
        // gap the tokenizer left — is still drawn rather than silently dropped.
        // `every_byte_of_a_line_is_covered_by_some_token` asserts there are no
        // gaps; this is what keeps a future one from deleting the user's text
        // off the screen instead of merely mis-colouring it.
        let mut covered = start;
        for token in tokens {
            if token.end <= covered {
                continue;
            }
            if token.start > covered
                && let Some(gap) = line.get(covered..token.start)
            {
                tree.text(x, y, gap, DEFAULT_THEME.color_for(Token::Plain), font_size);
                x += text::measure(gap, font_size, FontWeightHint::Regular);
            }
            let from = token.start.max(covered);
            let Some(piece) = line.get(from..token.end) else {
                continue;
            };
            if !piece.is_empty() {
                tree.text(x, y, piece, DEFAULT_THEME.color_for(token.kind), font_size);
                x += text::measure(piece, font_size, FontWeightHint::Regular);
            }
            covered = token.end;
            if x > right {
                return;
            }
        }
        // Anything the tokens did not reach — again, defence rather than an
        // expected path.
        if let Some(tail) = line.get(covered..)
            && !tail.is_empty()
        {
            tree.text(x, y, tail, DEFAULT_THEME.color_for(Token::Plain), font_size);
        }
    }

    fn render_editor(&self, tree: &mut RenderTree) {
        let doc = self.active_document();
        let editor_y = 32.0;
        let editor_h = self.window_height as f32 - 32.0 - 24.0;
        let w = self.window_width as f32;

        // Gutter (line numbers)
        tree.fill_rect(0.0, editor_y, self.gutter_width, editor_h, Color::from_hex(0x181825));

        let visible_lines = self.visible_lines();
        let end_line = (doc.scroll_line + visible_lines).min(doc.lines.len());

        // The syntax state entering the first visible line. Everything above the
        // viewport has to be tokenized to know it — a block comment opened on
        // line 3 colours line 4000 — which is what `entry_state`'s memo is for.
        // From here the loop carries the state forward itself, since it is
        // tokenizing each visible line anyway.
        let mut state = doc.entry_state(doc.scroll_line);

        for i in doc.scroll_line..end_line {
            let y = editor_y + (i - doc.scroll_line) as f32 * self.line_height;

            // Line number
            let ln = format!("{:>4}", i + 1);
            let ln_color = if i == doc.cursor_line {
                Color::from_hex(0xCDD6F4)
            } else {
                Color::from_hex(0x585B70)
            };
            tree.text(4.0, y + 3.0, &ln, ln_color, self.font_size - 2.0);

            // Current line highlight
            if i == doc.cursor_line {
                tree.fill_rect(
                    self.gutter_width,
                    y,
                    w - self.gutter_width,
                    self.line_height,
                    Color::from_hex(0x313244),
                );
            }

            // Line text, one drawn run per syntax token.
            let line = doc.lines.get(i).map_or("", String::as_str);
            let tokens = highlight::highlight_line(line, doc.language, &mut state);
            Self::draw_tokens(
                tree,
                line,
                &tokens,
                doc.scroll_col,
                self.text_x(),
                y + 3.0,
                w,
                self.font_size,
            );
        }

        // Cursor. Placed by measuring the text actually to its left, not by
        // multiplying a column count by a nominal character width. A column
        // count only lands on the right pixel if every glyph is exactly as
        // wide as the guess, and the error compounds along the line, so on a
        // long line the caret drifts visibly away from the character it is on
        // — and it drifts differently for every font the user picks.
        if doc.cursor_line >= doc.scroll_line && doc.cursor_line < end_line {
            let cursor_y =
                editor_y + (doc.cursor_line - doc.scroll_line) as f32 * self.line_height;
            let before_cursor = doc.lines.get(doc.cursor_line).map_or("", |line| {
                // Both offsets are bytes, and both are snapped, so the slice
                // cannot land inside a character even if the caller left the
                // scroll offset somewhere odd.
                let from = snap_to_boundary(line, doc.scroll_col);
                let to = snap_to_boundary(line, doc.cursor_col.max(from));
                line.get(from..to).unwrap_or("")
            });
            let cursor_x = self.text_x()
                + text::measure(before_cursor, self.font_size, FontWeightHint::Regular);
            tree.fill_rect(cursor_x, cursor_y + 2.0, 2.0, self.line_height - 4.0, Color::from_hex(0x89B4FA));
        }
    }

    fn render_status_bar(&self, tree: &mut RenderTree) {
        let doc = self.active_document();
        let bar_y = self.window_height as f32 - 24.0;
        let w = self.window_width as f32;

        tree.fill_rect(0.0, bar_y, w, 24.0, Color::from_hex(0x181825));

        // Cursor position
        let pos_text = format!("Ln {}, Col {}", doc.cursor_line + 1, doc.cursor_col + 1);
        tree.text(8.0, bar_y + 5.0, &pos_text, Color::from_hex(0x6C7086), 11.0);

        // Language
        tree.text(
            200.0,
            bar_y + 5.0,
            doc.language.name(),
            Color::from_hex(0x6C7086),
            11.0,
        );

        // Line ending
        tree.text(
            350.0,
            bar_y + 5.0,
            doc.line_ending.as_str(),
            Color::from_hex(0x6C7086),
            11.0,
        );

        // Line count
        let lc = format!("{} lines", doc.line_count());
        tree.text(w - 100.0, bar_y + 5.0, &lc, Color::from_hex(0x6C7086), 11.0);
    }

    fn render_find_panel(&self, tree: &mut RenderTree) {
        let panel_y = 32.0;
        let panel_w = 350.0;
        let panel_h = 80.0;
        let panel_x = self.window_width as f32 - panel_w - 16.0;

        tree.fill_rect(panel_x, panel_y, panel_w, panel_h, Color::from_hex(0x313244));
        tree.stroke_rect(panel_x, panel_y, panel_w, panel_h, Color::from_hex(0x585B70), 1.0);

        // Find input
        tree.text(panel_x + 8.0, panel_y + 10.0, "Find:", Color::from_hex(0xA6ADC8), 11.0);
        tree.fill_rect(panel_x + 50.0, panel_y + 6.0, panel_w - 60.0, 22.0, Color::from_hex(0x1E1E2E));
        tree.text(
            panel_x + 54.0,
            panel_y + 10.0,
            &self.find.query,
            Color::from_hex(0xCDD6F4),
            12.0,
        );

        // Replace input
        tree.text(panel_x + 8.0, panel_y + 40.0, "Repl:", Color::from_hex(0xA6ADC8), 11.0);
        tree.fill_rect(panel_x + 50.0, panel_y + 36.0, panel_w - 60.0, 22.0, Color::from_hex(0x1E1E2E));
        tree.text(
            panel_x + 54.0,
            panel_y + 40.0,
            &self.find.replace_text,
            Color::from_hex(0xCDD6F4),
            12.0,
        );

        // Match count
        let match_info = format!(
            "{} match(es)",
            self.find.matches.len()
        );
        tree.text(
            panel_x + 8.0,
            panel_y + 64.0,
            &match_info,
            Color::from_hex(0x6C7086),
            10.0,
        );
    }

    /// Render the external-change modal — either the four-option prompt or, when
    /// the user chose "review", the side-by-side merge review.
    fn render_external_prompt(&self, tree: &mut RenderTree, prompt: &ExternalChangePrompt) {
        let w = self.window_width as f32;
        let h = self.window_height as f32;

        // Dim the background.
        tree.fill_rect(0.0, 0.0, w, h, Color::rgba(0x11, 0x11, 0x1B, 0xB0));

        if let Some(review) = prompt.review.as_ref() {
            self.render_merge_review(tree, prompt, review);
            return;
        }

        // Centered dialog card.
        let dw = 480.0_f32.min(w - 40.0);
        let dh = 220.0_f32;
        let dx = (w - dw) / 2.0;
        let dy = (h - dh) / 2.0;
        tree.fill_rect(dx, dy, dw, dh, Color::from_hex(0x1E1E2E));
        tree.fill_rect(dx, dy, dw, 32.0, Color::from_hex(0x313244));

        let name = self
            .tabs
            .get(prompt.tab)
            .map_or("file", |d| d.name.as_str());

        let (title, body): (&str, String) = match &prompt.change {
            DiskChange::Deleted => (
                "File deleted on disk",
                format!("\"{name}\" was deleted outside the editor while you have unsaved changes."),
            ),
            _ => (
                "File changed on disk",
                format!("\"{name}\" was modified outside the editor and you have unsaved changes."),
            ),
        };

        tree.text(dx + 12.0, dy + 9.0, title, Color::from_hex(0xF9E2AF), 13.0);
        tree.text(dx + 12.0, dy + 44.0, &body, Color::from_hex(0xCDD6F4), 11.0);

        // Option buttons, stacked. For a deletion, merge/review don't apply.
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
            tree.fill_rect(dx + 12.0, by, dw - 24.0, 30.0, Color::from_hex(0x45475A));
            tree.text(dx + 20.0, by + 6.0, label, Color::from_hex(0xCDD6F4), 12.0);
            tree.text(
                dx + 160.0,
                by + 8.0,
                hint,
                Color::from_hex(0x9399B2),
                10.0,
            );
            by += 34.0;
        }
    }

    /// Render the side-by-side merge review (ours | theirs) with each conflict's
    /// current resolution. Mirrors orchestrator2's file-edit diff viewer layout.
    fn render_merge_review(
        &self,
        tree: &mut RenderTree,
        prompt: &ExternalChangePrompt,
        review: &MergeReview,
    ) {
        let w = self.window_width as f32;
        let h = self.window_height as f32;
        let margin = 24.0;
        let dx = margin;
        let dy = margin;
        let dw = w - margin * 2.0;
        let dh = h - margin * 2.0;

        tree.fill_rect(dx, dy, dw, dh, Color::from_hex(0x1E1E2E));
        tree.fill_rect(dx, dy, dw, 32.0, Color::from_hex(0x313244));

        let name = self
            .tabs
            .get(prompt.tab)
            .map_or("file", |d| d.name.as_str());
        let header = format!(
            "Review merge — {name}  ({} conflict(s))",
            review.conflict_count()
        );
        tree.text(dx + 12.0, dy + 9.0, &header, Color::from_hex(0xF9E2AF), 13.0);

        // Column headers.
        let col_w = (dw - 24.0) / 2.0;
        let ours_x = dx + 12.0;
        let theirs_x = dx + 12.0 + col_w;
        tree.text(ours_x, dy + 40.0, name, Color::from_hex(0xA6E3A1), 11.0);
        tree.text(theirs_x, dy + 40.0, "disk", Color::from_hex(0xF38BA8), 11.0);

        // Each conflict as a row block.
        let mut y = dy + 60.0;
        let line_h = self.line_height;
        for (i, (_base, ours, theirs)) in review.conflicts().iter().enumerate() {
            let choice = review.choice(i).unwrap_or(ConflictChoice::Theirs);
            let chosen_ours = matches!(choice, ConflictChoice::Ours | ConflictChoice::Both);
            let chosen_theirs = matches!(choice, ConflictChoice::Theirs | ConflictChoice::Both);

            let block_lines = ours.len().max(theirs.len()).max(1);
            let block_h = block_lines as f32 * line_h + 6.0;

            // Highlight the selected side(s).
            if chosen_ours {
                tree.fill_rect(ours_x - 4.0, y, col_w, block_h, Color::from_hex(0x2A3A2A));
            }
            if chosen_theirs {
                tree.fill_rect(theirs_x - 4.0, y, col_w, block_h, Color::from_hex(0x3A2A2A));
            }

            let label = format!("#{}", i + 1);
            tree.text(dx + 2.0, y, &label, Color::from_hex(0x6C7086), 9.0);

            for (li, line) in ours.iter().enumerate() {
                tree.text(
                    ours_x,
                    y + li as f32 * line_h,
                    line,
                    Color::from_hex(0xCDD6F4),
                    11.0,
                );
            }
            for (li, line) in theirs.iter().enumerate() {
                tree.text(
                    theirs_x,
                    y + li as f32 * line_h,
                    line,
                    Color::from_hex(0xCDD6F4),
                    11.0,
                );
            }
            y += block_h + 6.0;
        }

        // Footer actions.
        let fy = dy + dh - 30.0;
        tree.text(
            dx + 12.0,
            fy,
            "[Accept]  [Cancel]   per-conflict: take ours / take disk / keep both",
            Color::from_hex(0x9399B2),
            11.0,
        );
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut editor = EditorState::new();

    // Open files from command line
    for path_str in &args {
        let path = PathBuf::from(path_str);
        match editor.open_file(&path) {
            Ok(()) => println!("Opened: {}", path.display()),
            Err(e) => eprintln!("Error opening {}: {e}", path.display()),
        }
    }

    // Render initial frame
    let render = editor.render();
    let doc = editor.active_document();
    println!(
        "Text Editor: {} ({} lines, {})",
        doc.name,
        doc.line_count(),
        doc.language.name()
    );
    println!("  {} render commands", render.len());
    println!("  Cursor at Ln {}, Col {}", doc.cursor_line + 1, doc.cursor_col + 1);

    // Demonstrate editing
    let doc = editor.active_document_mut();
    doc.insert_char('H');
    doc.insert_char('e');
    doc.insert_char('l');
    doc.insert_char('l');
    doc.insert_char('o');
    println!(
        "  After typing 'Hello': \"{}\"",
        doc.line(0)
    );

    doc.undo();
    doc.undo();
    println!("  After 2x undo: \"{}\"", doc.line(0));

    doc.redo();
    println!("  After redo: \"{}\"", doc.line(0));

    // Demonstrate structural editing on a small Rust snippet.
    let mut sample = Document::new();
    sample.language = Language::Rust;
    sample.lines = vec![
        "fn outer() {".to_string(),
        "    fn inner() {".to_string(),
        "        let x = 1;".to_string(),
        "    }".to_string(),
        "}".to_string(),
    ];
    let outline = sample.outline();
    println!("\nOutline of sample snippet ({} entries):", outline.len());
    for (depth, header) in &outline {
        println!("  {}{}", "  ".repeat(*depth), header);
    }
    sample.cursor_line = 2;
    sample.cursor_col = 12;
    sample.selection_anchor = None;
    let mut steps = 0;
    while sample.expand_selection() && steps < 8 {
        let (s, e) = sample.selection_range();
        println!(
            "  expand-selection #{}: ({}:{}) -> ({}:{})",
            steps + 1,
            s.line + 1,
            s.col + 1,
            e.line + 1,
            e.col + 1
        );
        steps += 1;
    }

    println!("\nText editor ready.");
}

// ============================================================================
// Caret placement
// ============================================================================

#[cfg(test)]
mod caret_tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use guitk::render::RenderCommand;

    /// Colour of the caret, which is what identifies it in the command list.
    const CARET: u32 = 0x89B4FA;

    fn editor_with(line: &str, cursor_col: usize) -> EditorState {
        let mut editor = EditorState::new();
        editor.active_document_mut().lines = vec![line.to_string()];
        editor.active_document_mut().cursor_line = 0;
        editor.active_document_mut().cursor_col = cursor_col;
        editor
    }

    /// The x of the caret in a rendered frame.
    fn caret_x(editor: &EditorState) -> f32 {
        let tree = editor.render();
        tree.commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::FillRect {
                    x, width, color, ..
                } if *color == Color::from_hex(CARET) && (*width - 2.0).abs() < 0.01 => {
                    Some(*x)
                }
                _ => None,
            })
            .expect("the caret is drawn")
    }

    #[test]
    fn the_caret_sits_where_the_text_before_it_ends() {
        let editor = editor_with("hello world", 5);
        let expected = editor.gutter_width
            + 8.0
            + text::measure("hello", editor.font_size, FontWeightHint::Regular);
        assert!(
            (caret_x(&editor) - expected).abs() < 0.01,
            "the caret is at {}, but the text before it ends at {expected}",
            caret_x(&editor)
        );
    }

    #[test]
    fn the_caret_starts_at_the_left_edge_of_the_text() {
        let editor = editor_with("hello world", 0);
        assert!(
            (caret_x(&editor) - (editor.gutter_width + 8.0)).abs() < 0.01,
            "the caret at column 0 is not at the text's left edge"
        );
    }

    #[test]
    fn the_caret_tracks_the_font_rather_than_a_nominal_width() {
        // The regression this replaces: the caret's x was the column count
        // times `font_size * 0.6`, so it only landed on the right pixel for a
        // font whose glyphs happened to be exactly that wide, and the error
        // compounded along the line.
        //
        // The obvious test — that ten W's put the caret further right than ten
        // i's — cannot run here. With no system font installed, `osfont` falls
        // back to a built-in *monospace bitmap* face, so every glyph has the
        // same advance and the two are legitimately equal. What is checkable
        // in either backend is that the caret moves with what the font
        // reports, and not with the old constant.
        let editor = editor_with("xxxxxxxxxx", 10);
        let measured = text::measure("xxxxxxxxxx", editor.font_size, FontWeightHint::Regular);
        let old_guess = 10.0 * editor.font_size * 0.6;
        assert!(
            (measured - old_guess).abs() > 0.01,
            "the fallback font's advance now equals the constant this test \
             exists to rule out, so the assertion below proves nothing"
        );
        assert!(
            (caret_x(&editor) - (editor.gutter_width + 8.0 + measured)).abs() < 0.01,
            "the caret is not at the measured width of the text before it"
        );
    }

    #[test]
    fn a_horizontally_scrolled_line_measures_only_what_is_shown() {
        let mut editor = editor_with("hello world", 8);
        editor.active_document_mut().scroll_col = 6;
        // Columns 6..8 are "wo": the caret is two characters into the visible
        // text, not eight.
        let expected = editor.gutter_width
            + 8.0
            + text::measure("wo", editor.font_size, FontWeightHint::Regular);
        assert!(
            (caret_x(&editor) - expected).abs() < 0.01,
            "a scrolled line put the caret at {}, not {expected}",
            caret_x(&editor)
        );
    }

    /// `cursor_col` is a byte offset, and every move used to step by one byte.
    /// One step into a multi-byte character armed a crash that the *next* edit
    /// fired, because `String::insert`/`remove` panic on an offset that is not
    /// a character boundary. Any non-ASCII file — a comment in Russian, a
    /// string literal holding `é` — was one keystroke from taking the editor
    /// down with unsaved work in it.
    #[test]
    fn editing_around_a_multi_byte_character_does_not_panic() {
        let mut doc = Document::new();
        doc.lines = vec!["café au lait".to_string()];
        doc.cursor_line = 0;
        // Four characters in, five bytes in: `é` is two bytes.
        doc.cursor_col = 5;

        // One press crosses the whole character, in both directions.
        doc.move_left();
        assert_eq!(doc.cursor_col, 3);
        doc.move_right();
        assert_eq!(doc.cursor_col, 5);

        // Backspace removes the character, not one of its bytes.
        doc.backspace();
        assert_eq!(doc.line(0), "caf au lait");
        assert_eq!(doc.cursor_col, 3);

        // …and typing it back leaves the line as it was.
        doc.insert_char('é');
        assert_eq!(doc.line(0), "café au lait");
        assert_eq!(doc.cursor_col, 5);

        // Delete takes the whole character from in front of the cursor.
        doc.cursor_col = 3;
        doc.delete_forward();
        assert_eq!(doc.line(0), "caf au lait");
    }

    /// A column carried between lines is a byte offset that meant something on
    /// the *old* line. Moving down onto a line whose byte 3 is inside a
    /// character must snap back to a boundary, not sit inside it.
    #[test]
    fn a_column_carried_between_lines_lands_on_a_character() {
        let mut doc = Document::new();
        // Byte 3 of the second line is the middle of the three-byte `日`.
        doc.lines = vec!["abcdef".to_string(), "ab日本".to_string()];
        doc.cursor_line = 0;
        doc.cursor_col = 3;

        doc.move_down();
        assert_eq!(doc.cursor_line, 1);
        assert!(
            doc.lines[1].is_char_boundary(doc.cursor_col),
            "column {} is inside a character of {:?}",
            doc.cursor_col,
            doc.lines[1]
        );
        // Snapping goes back to the character the user was over, not past it.
        assert_eq!(doc.cursor_col, 2);

        // And the edit that used to panic now works.
        doc.insert_char('x');
        assert_eq!(doc.lines[1], "abx日本");
    }
}

// ============================================================================
// Syntax highlighting, as the user actually sees it
// ============================================================================

/// The highlighter has always worked; until now nothing called it outside its
/// own tests, so the editor drew every file in one colour while its module doc
/// advertised "syntax highlighting for common languages". These tests are about
/// the *connection* — that tokens reach the render tree, that a construct
/// opened above the viewport still colours what is on screen, and that the memo
/// which makes the second of those affordable cannot go stale.
#[cfg(test)]
mod highlight_render_tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use guitk::render::RenderCommand;

    /// Every `Text` command in a rendered frame, as `(x, text, colour)`.
    fn drawn(editor: &EditorState) -> Vec<(f32, String, Color)> {
        editor
            .render()
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x, text, color, .. } => Some((*x, text.clone(), *color)),
                _ => None,
            })
            .collect()
    }

    fn editor_showing(src: &str, language: Language) -> EditorState {
        let mut editor = EditorState::new();
        let doc = editor.active_document_mut();
        doc.lines = src.split('\n').map(str::to_string).collect();
        doc.language = language;
        doc.invalidate_highlight(0);
        editor
    }

    /// Recompute the state entering `line` from the top of the file, with no
    /// memo at all — the answer the cache has to keep agreeing with.
    fn state_from_scratch(doc: &Document, line: usize) -> HighlightState {
        let mut state = HighlightState::Normal;
        for i in 0..line {
            let src = doc.lines.get(i).map_or("", String::as_str);
            drop(highlight::highlight_line(src, doc.language, &mut state));
        }
        state
    }

    const LANGUAGES: [Language; 12] = [
        Language::Plain,
        Language::Rust,
        Language::C,
        Language::Python,
        Language::JavaScript,
        Language::Html,
        Language::Css,
        Language::Shell,
        Language::Toml,
        Language::Yaml,
        Language::Json,
        Language::Markdown,
    ];

    /// The renderer draws one run per token and nothing else, so a byte no token
    /// claims is a byte the user does not see. `draw_tokens` covers a gap
    /// defensively rather than dropping it — this is the assertion that says the
    /// defence should never have to fire.
    #[test]
    fn every_byte_of_a_line_is_covered_by_some_token() {
        let lines = [
            "fn main() { let x: u32 = 0x1F; /* hi */ }",
            "  # comment with  spaces\tand a tab",
            "s = \"a string\" + 'another' # tail",
            "### heading **bold** [link](http://x) `code`",
            "int *p = &q; // c",
            "key = [1, 2, 3]  # toml",
            "{\"a\": [1, null, true]}",
            "export PATH=$HOME/bin:$PATH  # sh",
            "",
            "   ",
        ];
        for language in LANGUAGES {
            for line in lines {
                let mut state = HighlightState::Normal;
                let tokens = highlight::highlight_line(line, language, &mut state);
                let mut at = 0usize;
                for token in &tokens {
                    assert_eq!(
                        token.start, at,
                        "{language:?} on {line:?}: token {token:?} does not start where the \
                         previous one ended ({at}), so those bytes would not be drawn"
                    );
                    assert!(
                        line.is_char_boundary(token.start) && line.is_char_boundary(token.end),
                        "{language:?} on {line:?}: token {token:?} splits a character"
                    );
                    at = token.end;
                }
                assert_eq!(
                    at,
                    line.len(),
                    "{language:?} on {line:?}: the trailing bytes belong to no token"
                );
            }
        }
    }

    #[test]
    fn a_rust_line_is_drawn_as_several_coloured_runs() {
        let editor = editor_showing("fn main() {}", Language::Rust);
        let runs = drawn(&editor);
        let keyword = DEFAULT_THEME.color_for(Token::Keyword);
        let function = DEFAULT_THEME.color_for(Token::Function);
        assert!(
            runs.iter().any(|(_, t, c)| t == "fn" && *c == keyword),
            "`fn` was not drawn in the keyword colour; runs were {runs:?}"
        );
        assert!(
            runs.iter().any(|(_, t, c)| t == "main" && *c == function),
            "`main` was not drawn in the function colour; runs were {runs:?}"
        );
    }

    /// The whole reason the state has to be carried between lines. Before this
    /// was wired up the editor drew every one of these lines in one colour.
    #[test]
    fn a_block_comment_keeps_its_colour_onto_the_next_line() {
        let editor = editor_showing("/* opens here\nstill a comment\n*/ code", Language::Rust);
        let comment = DEFAULT_THEME.color_for(Token::Comment);
        let runs = drawn(&editor);
        assert!(
            runs.iter()
                .any(|(_, t, c)| t.contains("still a comment") && *c == comment),
            "the second line of a block comment was not drawn as a comment; runs were {runs:?}"
        );
    }

    /// …and it must survive the viewport being scrolled past the line that
    /// opened it, which is the case the memo exists to make cheap.
    #[test]
    fn a_comment_opened_above_the_viewport_still_colours_what_is_on_screen() {
        let mut src = String::from("/* opens on line 1\n");
        for i in 0..200 {
            src.push_str(&format!("body line {i}\n"));
        }
        src.push_str("*/");
        let mut editor = editor_showing(&src, Language::Rust);
        editor.active_document_mut().scroll_line = 150;
        let comment = DEFAULT_THEME.color_for(Token::Comment);
        let runs = drawn(&editor);
        assert!(
            runs.iter()
                .any(|(_, t, c)| t.contains("body line 149") && *c == comment),
            "a line 150 rows below the `/*` lost the comment colour; runs were {runs:?}"
        );
    }

    /// The memo is only correct if every mutation says which line it touched.
    /// `lines` is public, so nothing in the type system enforces that — this
    /// walks each editing operation and checks the memo against a from-scratch
    /// recomputation, which is what would catch a future edit path that forgets.
    #[test]
    fn the_syntax_cache_agrees_with_a_recomputation_after_every_edit() {
        fn check(doc: &Document, what: &str) {
            let line = doc.lines.len().saturating_sub(1);
            assert_eq!(
                doc.entry_state(line),
                state_from_scratch(doc, line),
                "after {what}, the memoized state entering line {line} is stale"
            );
        }

        let mut doc = Document::new();
        doc.language = Language::Rust;
        doc.lines = vec![
            "let a = 1;".to_string(),
            "let b = 2;".to_string(),
            "let c = 3;".to_string(),
            "let d = 4;".to_string(),
        ];
        doc.invalidate_highlight(0);
        check(&doc, "the initial fill");

        // Typing `/*` on line 0 turns every line below it into a comment.
        doc.cursor_line = 0;
        doc.cursor_col = 0;
        doc.insert_char('/');
        doc.insert_char('*');
        check(&doc, "opening a block comment on line 0");

        doc.backspace();
        check(&doc, "backspacing the `*` back out");

        doc.cursor_col = 0;
        doc.delete_forward();
        check(&doc, "deleting the `/` forward");

        doc.cursor_line = 1;
        doc.cursor_col = 0;
        doc.insert_char('\n');
        check(&doc, "splitting a line");

        doc.backspace();
        check(&doc, "joining it again");

        doc.cursor_line = 2;
        doc.cursor_col = doc.lines[2].len();
        doc.delete_forward();
        check(&doc, "joining with the next line");

        doc.undo();
        check(&doc, "an undo");

        doc.redo();
        check(&doc, "a redo");

        doc.set_lines_from_text("/* everything\nis a comment\nnow */");
        check(&doc, "replacing the whole buffer");
    }

    /// `scroll_col` is a byte offset. Slicing a line at one that is not a
    /// character boundary panics, and a line scrolled into the middle of a
    /// multi-byte character is not hypothetical — a horizontal scroll lands
    /// wherever the arithmetic puts it.
    #[test]
    fn a_line_scrolled_into_the_middle_of_a_character_does_not_panic() {
        let mut editor = editor_showing("caf\u{e9} au lait", Language::Plain);
        for scroll in 0..16 {
            editor.active_document_mut().scroll_col = scroll;
            editor.active_document_mut().cursor_col = scroll;
            drop(editor.render());
        }
    }

    /// The undo stack records the *text* an edit inserted and reverts it by
    /// taking those bytes back out. Doing that a character at a time, once per
    /// byte, removed one character too many for every non-ASCII one.
    #[test]
    fn undoing_a_multi_byte_insertion_removes_only_that_character() {
        let mut doc = Document::new();
        doc.lines = vec!["ab".to_string()];
        doc.cursor_line = 0;
        doc.cursor_col = 1;
        doc.insert_char('\u{e9}');
        assert_eq!(doc.line(0), "a\u{e9}b");
        doc.undo();
        assert_eq!(
            doc.line(0), "ab",
            "undo removed the character after the one it inserted"
        );
    }
}

// ============================================================================
// Undo/redo
// ============================================================================

#[cfg(test)]
mod undo_tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn doc_with(lines: &[&str], line: usize, col: usize) -> Document {
        let mut doc = Document::new();
        doc.lines = lines.iter().map(|s| (*s).to_string()).collect();
        doc.cursor_line = line;
        doc.cursor_col = col;
        doc
    }

    /// The bug this module exists for. Enter used to be recorded as "inserted
    /// the text `\n` at this column", and undo reverted it by deleting one byte
    /// from that line — but the split had already moved the newline out of every
    /// line, so undo deleted an unrelated character and left the file split.
    #[test]
    fn undoing_enter_puts_the_line_back_together() {
        let mut doc = doc_with(&["abcd"], 0, 2);
        doc.insert_char('\n');
        assert_eq!(doc.lines, vec!["ab".to_string(), "cd".to_string()]);

        doc.undo();
        assert_eq!(
            doc.lines,
            vec!["abcd".to_string()],
            "undoing Enter must rejoin the line it split, not delete a character"
        );
        assert_eq!((doc.cursor_line, doc.cursor_col), (0, 2));
    }

    /// Enter's auto-indent is part of the same edit, so undo has to take it back
    /// with the split. Recording only the newline left the copied whitespace
    /// behind with nothing on the stack that could remove it.
    #[test]
    fn undoing_enter_also_takes_back_the_auto_indent() {
        let mut doc = doc_with(&["    body();"], 0, 11);
        doc.insert_char('\n');
        assert_eq!(
            doc.lines,
            vec!["    body();".to_string(), "    ".to_string()],
            "a new line starts at the previous line's indent"
        );
        assert_eq!(doc.cursor_col, 4);

        doc.undo();
        assert_eq!(doc.lines, vec!["    body();".to_string()]);
    }

    #[test]
    fn undoing_a_join_restores_both_lines() {
        let mut doc = doc_with(&["one", "two"], 1, 0);
        doc.backspace();
        assert_eq!(doc.lines, vec!["onetwo".to_string()]);

        doc.undo();
        assert_eq!(doc.lines, vec!["one".to_string(), "two".to_string()]);
        assert_eq!((doc.cursor_line, doc.cursor_col), (1, 0));
    }

    #[test]
    fn undoing_a_forward_join_restores_both_lines() {
        let mut doc = doc_with(&["one", "two"], 0, 3);
        doc.delete_forward();
        assert_eq!(doc.lines, vec!["onetwo".to_string()]);

        doc.undo();
        assert_eq!(doc.lines, vec!["one".to_string(), "two".to_string()]);
    }

    /// Only `insert_char` used to clear the redo stack, so a backspace after an
    /// undo left a redo entry describing an edit against a buffer that had since
    /// moved on — pressing redo then re-applied it at a stale position.
    #[test]
    fn a_new_edit_after_an_undo_clears_the_redo_stack() {
        let mut doc = doc_with(&["ab"], 0, 2);
        doc.insert_char('c');
        doc.undo();
        assert_eq!(doc.redo_stack.len(), 1);

        doc.backspace();
        assert!(
            doc.redo_stack.is_empty(),
            "an edit made after an undo invalidates the redo stack, whatever the edit was"
        );
    }

    /// Every editing operation, applied in sequence to a document with
    /// non-ASCII text and indentation, then undone one step at a time and
    /// redone one step at a time.
    ///
    /// The assertion is against a snapshot taken *between* every pair of steps,
    /// not just at the ends: an undo stack can arrive back at the original text
    /// while having been wrong at every intermediate point, and it is the
    /// intermediate points the user actually looks at.
    #[test]
    fn every_edit_operation_round_trips_one_step_at_a_time() {
        type Step = (&'static str, fn(&mut Document));
        const STEPS: &[Step] = &[
            ("insert ascii", |d| {
                d.cursor_line = 0;
                d.cursor_col = 3;
                d.insert_char('X');
            }),
            ("insert two-byte char", |d| {
                d.cursor_line = 0;
                d.cursor_col = 1;
                d.insert_char('\u{e9}');
            }),
            ("insert four-byte char", |d| {
                d.cursor_line = 1;
                d.cursor_col = 0;
                d.insert_char('\u{1f600}');
            }),
            ("split a line", |d| {
                d.cursor_line = 0;
                d.cursor_col = 2;
                d.insert_char('\n');
            }),
            ("split an indented line (auto-indent)", |d| {
                d.cursor_line = 2;
                d.cursor_col = d.lines[2].len();
                d.insert_char('\n');
            }),
            ("tab expanded to spaces", |d| {
                d.cursor_line = 0;
                d.cursor_col = 0;
                d.use_spaces = true;
                d.insert_char('\t');
            }),
            ("backspace over a character", |d| {
                d.cursor_line = 1;
                d.cursor_col = d.lines[1].len();
                d.backspace();
            }),
            ("backspace joining two lines", |d| {
                d.cursor_line = 1;
                d.cursor_col = 0;
                d.backspace();
            }),
            ("delete forward over a character", |d| {
                d.cursor_line = 0;
                d.cursor_col = 0;
                d.delete_forward();
            }),
            ("delete forward joining two lines", |d| {
                d.cursor_line = 0;
                d.cursor_col = d.lines[0].len();
                d.delete_forward();
            }),
        ];

        let mut doc = doc_with(
            &["  caf\u{e9} au lait", "\u{4e2d}\u{6587}", "    if x:"],
            0,
            0,
        );

        // `history[i]` is the buffer *before* step `i` ran.
        let mut history = vec![doc.lines.clone()];
        for (name, step) in STEPS {
            step(&mut doc);
            assert!(
                !doc.lines.iter().any(|l| l.contains('\n')),
                "{name} left a newline inside a line; the buffer is one string per line"
            );
            history.push(doc.lines.clone());
        }

        for (i, (name, _)) in STEPS.iter().enumerate().rev() {
            doc.undo();
            assert_eq!(
                doc.lines, history[i],
                "undoing {name:?} (step {i}) did not restore the buffer"
            );
        }

        for (i, (name, _)) in STEPS.iter().enumerate() {
            doc.redo();
            assert_eq!(
                doc.lines,
                history[i + 1],
                "redoing {name:?} (step {i}) did not re-apply it"
            );
        }
    }

    /// Undo must never leave the caret inside a character or past the end of the
    /// buffer, because every later edit indexes a `String` by `cursor_col` and
    /// being wrong there is a panic rather than a wrong answer.
    #[test]
    fn undo_leaves_the_caret_on_a_character_boundary() {
        let mut doc = doc_with(&["\u{4e2d}\u{6587}"], 0, 0);
        doc.cursor_col = 3;
        doc.insert_char('\u{e9}');
        doc.undo();
        doc.redo();
        for _ in 0..4 {
            doc.undo();
        }
        let line = &doc.lines[doc.cursor_line];
        assert!(
            line.is_char_boundary(doc.cursor_col),
            "caret at byte {} is inside a character of {line:?}",
            doc.cursor_col
        );
        // The caret being valid is only useful if editing there does not panic.
        doc.insert_char('z');
    }
}

// ============================================================================
// Integration tests for syntree-backed Document operations
// ============================================================================

#[cfg(test)]
mod doc_syntree_tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn rust_doc(src: &str) -> Document {
        let mut d = Document::new();
        d.language = Language::Rust;
        d.lines = src.lines().map(str::to_string).collect();
        if d.lines.is_empty() {
            d.lines.push(String::new());
        }
        d
    }

    #[test]
    fn outline_lists_top_level_functions() {
        let d = rust_doc("fn a() {\n    1\n}\n\nfn b() {\n    2\n}\n");
        let outline = d.outline();
        // Two multi-line blocks expected.
        assert!(outline.len() >= 2, "outline = {:?}", outline);
    }

    #[test]
    fn expand_selection_grows_to_enclosing_block() {
        let mut d = rust_doc("fn f() {\n    let x = 1;\n}\n");
        // Cursor inside the function body.
        d.cursor_line = 1;
        d.cursor_col = 8;
        d.selection_anchor = None;
        assert!(d.expand_selection());
        let (s, e) = d.selection_range();
        // Selection should now span the {...} block.
        assert_eq!(s.line, 0);
        assert_eq!(e.line, 2);
    }

    #[test]
    fn expand_selection_repeatedly_grows_outward() {
        let mut d = rust_doc("fn f() {\n    {\n        1\n    }\n}\n");
        d.cursor_line = 2;
        d.cursor_col = 8;
        d.selection_anchor = None;
        let mut last = d.selection_range();
        for _ in 0..4 {
            if !d.expand_selection() {
                break;
            }
            let cur = d.selection_range();
            // Each step must strictly grow the range.
            assert!(cur.0 <= last.0 && cur.1 >= last.1 && cur != last);
            last = cur;
        }
    }

    #[test]
    fn expand_selection_no_op_when_already_at_root() {
        // A buffer with no scopes: expansion should report no change.
        let mut d = rust_doc("plain text with no braces\n");
        d.cursor_line = 0;
        d.cursor_col = 4;
        d.selection_anchor = None;
        assert!(!d.expand_selection());
    }

    #[test]
    fn fold_ranges_returned_in_sorted_order() {
        let d = rust_doc("fn a() {\n    1\n}\nfn b() {\n    2\n}\n");
        let folds = d.fold_ranges();
        for w in folds.windows(2) {
            assert!(w[0] <= w[1]);
        }
        assert!(folds.len() >= 2);
    }
}

// ============================================================================
// External-change detection & three-way merge tests
// ============================================================================

#[cfg(test)]
mod external_merge_tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use std::io::Write;
    use std::time::SystemTime;

    /// Build a document as if it were loaded from disk with the given content
    /// (LF-normalized `base`), without touching the filesystem.
    fn loaded_doc(content: &str) -> Document {
        let mut d = Document::new();
        d.lines = content.split('\n').map(str::to_string).collect();
        if d.lines.is_empty() {
            d.lines.push(String::new());
        }
        d.sync.base = Some(normalize_content(content));
        d.modified = false;
        d
    }

    /// Create a unique temp file path under the OS temp dir.
    fn temp_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("slate_editor_test_{tag}_{nanos}.txt"));
        p
    }

    #[test]
    fn normalize_strips_crlf_and_trailing_newline() {
        assert_eq!(normalize_content("a\r\nb\r\n"), "a\nb");
        assert_eq!(normalize_content("a\nb"), "a\nb");
        assert_eq!(normalize_content(""), "");
    }

    #[test]
    fn buffer_text_is_lf_joined() {
        let d = loaded_doc("one\ntwo\nthree");
        assert_eq!(d.buffer_text(), "one\ntwo\nthree");
    }

    /// A save goes through `safeio`, not `std::fs::write`.
    ///
    /// The two are indistinguishable in the result — identical bytes at an
    /// identical path — and differ only when the write is interrupted, which
    /// no portable test can stage. So the routing itself is asserted, via
    /// `safeio`'s `audit` counters. Without this, restoring `fs::write` in
    /// `save` leaves every other test in this file green.
    ///
    /// This matters more for an editor than for most adopters: the file being
    /// overwritten is the user's only copy of their document, so a truncating
    /// write that dies part-way destroys the very thing the save was meant to
    /// preserve.
    ///
    /// The counters are process-global and tests run in parallel, so this
    /// compares a before and after reading rather than an absolute.
    #[test]
    fn a_save_goes_through_safeio() {
        let path = temp_path("routing");
        let mut d = loaded_doc("some text the user would hate to lose");
        d.path = Some(path.clone());

        let before = safeio::writes_performed();
        d.save().expect("save");
        let after = safeio::writes_performed();

        assert!(
            after > before,
            "the save did not go through safeio (writes_performed stayed at {before}) \
             -- Document::save must not use std::fs::write"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            "some text the user would hate to lose"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn disk_changed_unchanged_when_no_path() {
        let d = loaded_doc("hello");
        assert_eq!(d.disk_changed(), DiskChange::Unchanged);
    }

    #[test]
    fn disk_changed_detects_modification_and_deletion() {
        let path = temp_path("detect");
        {
            let mut f = std::fs::File::create(&path).expect("create temp");
            f.write_all(b"line1\nline2\n").expect("write");
        }
        let doc = Document::from_file(&path).expect("load");
        assert_eq!(doc.disk_changed(), DiskChange::Unchanged);

        // Modify the file externally.
        std::fs::write(&path, b"line1 CHANGED\nline2\n").expect("rewrite");
        match doc.disk_changed() {
            DiskChange::Modified { disk } => assert_eq!(disk, "line1 CHANGED\nline2"),
            other => panic!("expected Modified, got {other:?}"),
        }

        // Delete the file.
        std::fs::remove_file(&path).expect("remove");
        assert_eq!(doc.disk_changed(), DiskChange::Deleted);
    }

    #[test]
    fn reload_replaces_buffer_and_clears_modified() {
        let mut d = loaded_doc("old\ncontent");
        d.modified = true;
        d.cursor_line = 5; // out of new bounds
        d.reload_from_disk("new\ndisk\ncontent");
        assert_eq!(d.lines, vec!["new", "disk", "content"]);
        assert!(!d.modified);
        assert_eq!(d.sync.base.as_deref(), Some("new\ndisk\ncontent"));
        assert!(d.cursor_line <= 2); // clamped
    }

    #[test]
    fn merge_disjoint_changes_is_clean() {
        // base: ours edits the first line, theirs edits the last; the shared
        // "middle" line is the common context anchor that lets both apply
        // cleanly (matching `git merge-file` semantics — adjacent changes with
        // no unchanged context line between them would instead conflict).
        let mut d = loaded_doc("alpha\nmiddle\nbeta");
        d.lines = vec![
            "ALPHA".to_string(),
            "middle".to_string(),
            "beta".to_string(),
        ];
        d.modified = true;
        let disk = "alpha\nmiddle\nBETA";
        let outcome = d.merge_from_disk(disk);
        assert_eq!(outcome, MergeOutcome::Clean);
        assert_eq!(d.buffer_text(), "ALPHA\nmiddle\nBETA");
        assert!(d.modified);
    }

    #[test]
    fn merge_overlapping_changes_conflicts() {
        let mut d = loaded_doc("shared");
        d.lines = vec!["ours-version".to_string()];
        d.modified = true;
        let outcome = d.merge_from_disk("theirs-version");
        match outcome {
            MergeOutcome::Conflicted { conflicts } => assert_eq!(conflicts, 1),
            MergeOutcome::Clean => panic!("expected a conflict"),
        }
        // Buffer should contain conflict markers for manual resolution.
        assert!(d.buffer_text().contains("<<<<<<<"));
        assert!(d.buffer_text().contains(">>>>>>>"));
    }

    #[test]
    fn review_lets_user_pick_ours() {
        let d = {
            let mut d = loaded_doc("shared");
            d.lines = vec!["ours-version".to_string()];
            d.modified = true;
            d
        };
        let mut review = MergeReview::new(d.merge_preview("theirs-version"));
        assert_eq!(review.conflict_count(), 1);
        // Default is theirs (disk).
        assert_eq!(review.accepted_text(), "theirs-version");
        // Flip to ours.
        review.set_choice(0, ConflictChoice::Ours);
        assert_eq!(review.accepted_text(), "ours-version");
        // Keep both.
        review.set_choice(0, ConflictChoice::Both);
        assert_eq!(review.accepted_text(), "ours-version\ntheirs-version");
    }

    #[test]
    fn editor_auto_reloads_unmodified_buffer() {
        let path = temp_path("autoreload");
        std::fs::write(&path, b"first\n").expect("write");
        let mut editor = EditorState::new();
        editor.open_file(&path).expect("open");
        // Externally change the file; buffer is not modified.
        std::fs::write(&path, b"second\n").expect("rewrite");
        let raised = editor.check_external_change();
        assert!(!raised, "no prompt expected for unmodified buffer");
        assert_eq!(editor.active_document().buffer_text(), "second");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn editor_prompts_on_conflicting_change() {
        let path = temp_path("prompt");
        std::fs::write(&path, b"base\n").expect("write");
        let mut editor = EditorState::new();
        editor.open_file(&path).expect("open");
        // Local edit.
        editor.active_document_mut().lines = vec!["local".to_string()];
        editor.active_document_mut().modified = true;
        // External edit.
        std::fs::write(&path, b"remote\n").expect("rewrite");
        assert!(editor.check_external_change());
        assert!(editor.external_prompt.is_some());

        // Enter review, pick ours, accept.
        editor.resolve_external(ExternalChoice::Review);
        editor.review_set_choice(0, ConflictChoice::Ours);
        editor.review_accept();
        assert!(editor.external_prompt.is_none());
        assert_eq!(editor.active_document().buffer_text(), "local");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reload_choice_discards_local_edits() {
        let path = temp_path("reload");
        std::fs::write(&path, b"base\n").expect("write");
        let mut editor = EditorState::new();
        editor.open_file(&path).expect("open");
        editor.active_document_mut().lines = vec!["local".to_string()];
        editor.active_document_mut().modified = true;
        std::fs::write(&path, b"remote\n").expect("rewrite");
        assert!(editor.check_external_change());
        editor.resolve_external(ExternalChoice::Reload);
        assert_eq!(editor.active_document().buffer_text(), "remote");
        assert!(!editor.active_document().modified);
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod tab_tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)]

    use super::{Document, FindState, Tabs};

    fn named(name: &str) -> Document {
        let mut d = Document::new();
        d.name = name.to_string();
        d
    }

    fn names(tabs: &Tabs<Document>) -> Vec<String> {
        tabs.iter().map(|d| d.name.clone()).collect()
    }

    #[test]
    fn there_is_always_a_document_open() {
        // The editor draws *the* active document on every frame; there is no
        // "no document" screen. Closing the last tab therefore has to leave a
        // fresh empty one rather than an empty list.
        let mut tabs: Tabs<Document> = Tabs::new();
        assert_eq!(tabs.count(), 1);
        tabs.close_active();
        assert_eq!(tabs.count(), 1);
        assert_eq!(tabs.active_index(), 0);
        assert_eq!(tabs.active().buffer_text(), "");
    }

    #[test]
    fn closing_the_first_tab_promotes_the_next_one() {
        // The first tab is stored apart from the rest, so closing it is the
        // one case that has to move a document rather than remove one.
        let mut tabs: Tabs<Document> = Tabs::new();
        tabs.open(named("b"));
        tabs.open(named("c"));
        tabs.set_active(0);
        tabs.close_active();
        assert_eq!(names(&tabs), ["b", "c"]);
        assert_eq!(tabs.active().name, "b");
    }

    #[test]
    fn closing_the_last_tab_moves_the_selection_back_one() {
        let mut tabs: Tabs<Document> = Tabs::new();
        tabs.open(named("b"));
        tabs.open(named("c"));
        assert_eq!(tabs.active().name, "c");
        tabs.close_active();
        assert_eq!(names(&tabs), ["Untitled", "b"]);
        assert_eq!(tabs.active().name, "b");
    }

    #[test]
    fn closing_a_middle_tab_leaves_the_others_in_order() {
        let mut tabs: Tabs<Document> = Tabs::new();
        tabs.open(named("b"));
        tabs.open(named("c"));
        tabs.set_active(1);
        tabs.close_active();
        assert_eq!(names(&tabs), ["Untitled", "c"]);
        assert_eq!(tabs.active().name, "c");
    }

    #[test]
    fn a_tab_index_past_the_end_is_clamped_not_fatal() {
        // The index used to be a public field, so any stale value reached
        // `documents[active_tab]` and took the editor — and every unsaved
        // buffer in it — down. It can now only move through `set_active`.
        let mut tabs: Tabs<Document> = Tabs::new();
        tabs.open(named("b"));
        tabs.set_active(99);
        assert_eq!(tabs.active_index(), 1);
        assert_eq!(tabs.active().name, "b");
        assert!(tabs.get(99).is_none());
    }

    #[test]
    fn replacing_a_match_that_no_longer_fits_the_line_is_skipped() {
        // Find records byte ranges against the document it searched. Handing
        // a *different* (or since-shortened) document to `replace_all` used
        // to reach `String::replace_range` with an out-of-range span, which
        // panics. The count reports replacements made, not matches recorded.
        let mut doc = Document::new();
        doc.lines = vec!["aaaa".to_string(), "aaaa".to_string()];
        let mut find = FindState::new();
        find.query = "aa".to_string();
        find.find_all(&doc);
        assert_eq!(find.matches.len(), 4);

        // Shrink the buffer out from under the recorded matches.
        doc.lines = vec!["a".to_string()];
        assert_eq!(find.replace_all(&mut doc), 0);
        assert_eq!(doc.buffer_text(), "a");
        assert!(!doc.modified);
    }

    #[test]
    fn replacing_a_match_that_lands_inside_a_character_is_skipped() {
        // `replace_range` also panics on a boundary that is not a character
        // boundary, which a stale byte offset easily is once the line's
        // contents have changed.
        let mut doc = Document::new();
        doc.lines = vec!["xx".to_string()];
        let mut find = FindState::new();
        find.query = "x".to_string();
        find.find_all(&doc);
        doc.lines = vec!["é".to_string()]; // two bytes, one character
        assert_eq!(find.replace_all(&mut doc), 0);
        assert_eq!(doc.buffer_text(), "é");
    }

    #[test]
    fn cursor_movement_on_a_line_that_is_not_there_does_not_panic() {
        // `lines` is public, so a caller can shrink it without touching the
        // cursor. Every movement key then asks for a line that is gone.
        let mut doc = Document::new();
        doc.lines = vec!["one".to_string(), "two".to_string()];
        doc.cursor_line = 7;
        doc.cursor_col = 3;
        doc.move_end();
        assert_eq!(doc.cursor_col, 0);
        doc.move_up();
        doc.move_down();
        doc.move_left();
        doc.move_right();
        doc.move_to_end();
        assert_eq!(doc.cursor_line, 1);
        assert_eq!(doc.cursor_col, 3);
    }

    #[test]
    fn overlapping_occurrences_are_not_counted_or_replaced_twice() {
        // `find_all` used to resume one byte into the match it had just
        // recorded, so "aaaa" reported three occurrences of "aa" — and
        // `replace_all`, rewriting overlapping ranges back to front, shredded
        // the line.
        let mut doc = Document::new();
        doc.lines = vec!["aaaa".to_string()];
        let mut find = FindState::new();
        find.query = "aa".to_string();
        find.replace_text = "b".to_string();
        find.find_all(&doc);
        assert_eq!(find.matches, [(0, 0, 2), (0, 2, 4)]);
        assert_eq!(find.replace_all(&mut doc), 2);
        assert_eq!(doc.buffer_text(), "bb");
    }

    #[test]
    fn a_case_insensitive_match_is_found_at_its_offset_in_the_real_line() {
        // `İ` is two bytes but lowercases to three, so the offset of anything
        // after it differs between the line and a `to_lowercase()` copy of
        // it. Searching the copy — which is what find used to do — put the
        // match past the end of the real line, where the replace either hit
        // the wrong bytes or panicked.
        let mut doc = Document::new();
        doc.lines = vec!["İx".to_string()];
        let mut find = FindState::new();
        find.query = "X".to_string();
        find.replace_text = "y".to_string();
        find.find_all(&doc);
        assert_eq!(find.matches, [(0, 2, 3)]);
        assert_eq!(find.replace_all(&mut doc), 1);
        assert_eq!(doc.buffer_text(), "İy");
    }

    #[test]
    fn a_search_never_resumes_inside_a_character() {
        // The old one-byte resume step landed inside a multi-byte character,
        // where slicing the line panicked outright. Every occurrence here is
        // three bytes wide.
        let mut doc = Document::new();
        doc.lines = vec!["日本日本".to_string()];
        let mut find = FindState::new();
        find.query = "日本".to_string();
        find.find_all(&doc);
        assert_eq!(find.matches, [(0, 0, 6), (0, 6, 12)]);
    }
}
