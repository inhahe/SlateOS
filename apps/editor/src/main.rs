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
// The keyboard and mouse. Everything above this module can be *asked* to edit;
// `input` is the only place that decides that Ctrl+S means save.
mod input;
mod syntree;

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderTree, TextSpan};
use guitk::tabs::Tabs;
use guitk::text;
use highlight::{DEFAULT_THEME, HighlightState, StyledToken, Token};
use input::{EditorResponse, FindField};
use oswindow::{EventLoop, EventResponse, WindowBuilder};
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
    /// Horizontal scroll offset, in **pixels** from the left edge of the line.
    ///
    /// **It used to be a byte offset, and that was wrong at the root rather
    /// than in the arithmetic.** Scrolling by a byte offset means drawing
    /// `line[scroll..]` — and the visible part of a bidirectional line is not
    /// the shaping of a suffix of it. Cutting a line at byte *n* and shaping
    /// what is left re-orders the remainder against itself: the bidi algorithm
    /// resolves paragraph direction and embedding levels from the *whole* run,
    /// so a suffix can come out in a different visual order than those same
    /// characters have in the complete line. Scrolling would rearrange the
    /// text, not merely slide it.
    ///
    /// A pixel offset has no such failure mode, because it moves the drawn run
    /// instead of shortening it: the line is shaped once, whole, and then
    /// translated left by `scroll_px` with a clip rectangle hiding what falls
    /// outside the text area. What is on screen is then a *window onto* the
    /// correctly-ordered line rather than a separate, differently-ordered
    /// shaping of part of it.
    ///
    /// It also removes a whole class of bug outright: there is no longer any
    /// slice, so there is no character to land in the middle of, and no
    /// `snap_to_boundary` call standing between the scroll position and a
    /// panic.
    pub scroll_px: f32,
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
        col = col.saturating_sub(1);
    }
    col
}

/// A token's byte offset as a [`TextSpan`] end.
///
/// Saturating rather than panicking, because this feeds a span and a wrong
/// span mis-colours where a panic takes the editor down: a line longer than
/// 4 GiB clamps to `u32::MAX`, which is past every glyph and so colours the
/// rest of the line in that token's colour rather than losing the line.
///
/// It takes no scroll position because there is none to take. Spans are
/// offsets into the whole line, always — horizontal scrolling moves the drawn
/// run rather than slicing it, so there is nothing to rebase against. See
/// [`Document::scroll_px`].
fn span_end(offset: usize) -> u32 {
    u32::try_from(offset).unwrap_or(u32::MAX)
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
            scroll_px: 0.0,
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
            scroll_px: 0.0,
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
            let index = entries.len().saturating_sub(1);
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
                let spaces = " ".repeat(
                    doc.tab_width
                        .saturating_sub(col.checked_rem(doc.tab_width).unwrap_or(0)),
                );
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
            self.cursor_col = self.cursor_col.saturating_sub(ch.len_utf8());
        } else if self.cursor_line > 0 {
            self.cursor_line = self.cursor_line.saturating_sub(1);
            self.cursor_col = self.cursor_line_text().len();
        }
    }

    pub fn move_right(&mut self) {
        if let Some(ch) = self.char_at_cursor() {
            self.cursor_col = self.cursor_col.saturating_add(ch.len_utf8());
        } else if self.cursor_line.saturating_add(1) < self.lines.len() {
            self.cursor_line = self.cursor_line.saturating_add(1);
            self.cursor_col = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor_line > 0 {
            self.cursor_line = self.cursor_line.saturating_sub(1);
            self.cursor_col = snap_to_boundary(self.cursor_line_text(), self.cursor_col);
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_line.saturating_add(1) < self.lines.len() {
            self.cursor_line = self.cursor_line.saturating_add(1);
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
        } else if self.cursor_line >= self.scroll_line.saturating_add(visible_lines) {
            self.scroll_line = self
                .cursor_line
                .saturating_sub(visible_lines)
                .saturating_add(1);
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

    /// Whether anything is selected.
    ///
    /// An anchor equal to the cursor is *not* a selection: clicking sets an
    /// anchor before the drag is known to be a drag, and treating that as a
    /// zero-width selection would make a plain click delete-on-next-keystroke.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        let (start, end) = self.selection_range();
        start != end
    }

    /// The selected text, with `\n` between lines. Empty when nothing is
    /// selected.
    ///
    /// Offsets are snapped to character boundaries before slicing, because a
    /// selection endpoint is a byte offset that a buffer edit may have left
    /// mid-character — and slicing there panics rather than rounding.
    #[must_use]
    pub fn selected_text(&self) -> String {
        let (start, end) = self.selection_range();
        if start == end {
            return String::new();
        }
        let slice = |line: usize, from: usize, to: usize| -> String {
            let text = self.lines.get(line).map_or("", String::as_str);
            let from = snap_to_boundary(text, from);
            let to = snap_to_boundary(text, to.max(from));
            text.get(from..to).unwrap_or("").to_string()
        };
        if start.line == end.line {
            return slice(start.line, start.col, end.col);
        }
        let mut out = slice(start.line, start.col, usize::MAX);
        for line in start.line.saturating_add(1)..end.line {
            out.push('\n');
            out.push_str(self.lines.get(line).map_or("", String::as_str));
        }
        out.push('\n');
        out.push_str(&slice(end.line, 0, end.col));
        out
    }

    /// Delete the selection, leaving the cursor where it began. Returns whether
    /// there was anything to delete.
    ///
    /// One recorded edit, so undo takes the whole selection back in a single
    /// step rather than a character at a time — which is what a user means by
    /// "undo that deletion", and is also why this cannot be written as a loop
    /// over [`Self::backspace`].
    pub fn delete_selection(&mut self) -> bool {
        let (start, end) = self.selection_range();
        if start == end {
            return false;
        }
        // The edit replaces every line the selection touches with one joined
        // line, so that is the range to record.
        let span = end.line.saturating_sub(start.line).saturating_add(1);
        self.record_edit(start.line, span, |doc| {
            let head = {
                let text = doc.lines.get(start.line).map_or("", String::as_str);
                let at = snap_to_boundary(text, start.col);
                text.get(..at).unwrap_or("").to_string()
            };
            let tail = {
                let text = doc.lines.get(end.line).map_or("", String::as_str);
                let at = snap_to_boundary(text, end.col);
                text.get(at..).unwrap_or("").to_string()
            };
            let joined = format!("{head}{tail}");
            let from = start.line.min(doc.lines.len());
            let to = end.line.saturating_add(1).min(doc.lines.len());
            doc.lines
                .splice(from..to, std::iter::once(joined))
                .for_each(drop);
            doc.cursor_line = start.line;
            doc.cursor_col = head.len();
            doc.selection_anchor = None;
        });
        self.clamp_cursor();
        true
    }

    /// Select the word under the cursor, as a double-click does.
    ///
    /// "Word" is a run of alphanumerics and underscores, or — when the cursor
    /// is not on one — the run of identical-class characters it *is* on, so
    /// double-clicking whitespace or punctuation selects that run rather than
    /// nothing at all. Returns whether anything was selected.
    pub fn select_word_at_cursor(&mut self) -> bool {
        let line = self.cursor_line;
        let text = self.lines.get(line).map_or("", String::as_str).to_string();
        if text.is_empty() {
            return false;
        }
        let at = snap_to_boundary(&text, self.cursor_col);

        // The character to the right decides the class; at end of line, the one
        // to the left, so double-clicking past the last word still selects it.
        let class = word_class;
        let Some(here) = text
            .get(at..)
            .and_then(|s| s.chars().next())
            .or_else(|| text.get(..at).and_then(|s| s.chars().next_back()))
        else {
            return false;
        };
        let want = class(here);

        let mut start = at.min(text.len());
        while let Some(ch) = text.get(..start).and_then(|s| s.chars().next_back()) {
            if class(ch) != want {
                break;
            }
            start = start.saturating_sub(ch.len_utf8());
        }
        let mut end = at.min(text.len());
        while let Some(ch) = text.get(end..).and_then(|s| s.chars().next()) {
            if class(ch) != want {
                break;
            }
            end = end.saturating_add(ch.len_utf8());
        }
        if start == end {
            return false;
        }
        self.set_selection(Pos::new(line, start), Pos::new(line, end));
        true
    }

    /// Select the whole buffer.
    pub fn select_all(&mut self) {
        let last = self.lines.len().saturating_sub(1);
        let end = self.lines.get(last).map_or(0, String::len);
        self.set_selection(Pos::new(0, 0), Pos::new(last, end));
    }

    /// Move the caret to the start of the previous word.
    ///
    /// At column 0 this falls through to [`Self::move_left`], which is what
    /// crosses the line break — word motion is defined within a line, and a
    /// caret that stopped dead at the left margin could never leave it.
    pub fn move_word_left(&mut self) {
        if self.cursor_col == 0 {
            self.move_left();
            return;
        }
        let text = self.cursor_line_text().to_string();
        let mut at = snap_to_boundary(&text, self.cursor_col);
        // Whitespace behind the caret belongs to the gap, not to a word, so
        // skip it before deciding which run is being crossed. Without this,
        // pressing Ctrl+Left in the indentation of a line would land one space
        // to the left each time rather than at the previous word.
        while let Some(ch) = text.get(..at).and_then(|s| s.chars().next_back()) {
            if !ch.is_whitespace() {
                break;
            }
            at = at.saturating_sub(ch.len_utf8());
        }
        let Some(anchor) = text.get(..at).and_then(|s| s.chars().next_back()) else {
            self.cursor_col = at;
            return;
        };
        let want = word_class(anchor);
        while let Some(ch) = text.get(..at).and_then(|s| s.chars().next_back()) {
            if word_class(ch) != want {
                break;
            }
            at = at.saturating_sub(ch.len_utf8());
        }
        self.cursor_col = at;
    }

    /// Move the caret past the end of the next word.
    ///
    /// The mirror of [`Self::move_word_left`]: it crosses the run under the
    /// caret and then the whitespace after it, so repeated presses land on
    /// successive word *starts* — which is where the eye expects the caret when
    /// moving rightwards.
    pub fn move_word_right(&mut self) {
        let text = self.cursor_line_text().to_string();
        if self.cursor_col >= text.len() {
            self.move_right();
            return;
        }
        let mut at = snap_to_boundary(&text, self.cursor_col);
        if let Some(here) = text.get(at..).and_then(|s| s.chars().next()) {
            let want = word_class(here);
            while let Some(ch) = text.get(at..).and_then(|s| s.chars().next()) {
                if word_class(ch) != want {
                    break;
                }
                at = at.saturating_add(ch.len_utf8());
            }
        }
        while let Some(ch) = text.get(at..).and_then(|s| s.chars().next()) {
            if !ch.is_whitespace() {
                break;
            }
            at = at.saturating_add(ch.len_utf8());
        }
        self.cursor_col = at;
    }

    /// Insert text at the caret, splitting lines on every newline it contains.
    ///
    /// This is what paste needs and what a loop over [`Self::insert_char`]
    /// cannot give: the loop would record one undo entry per character, so
    /// undoing a paste would take the pasted text back one keystroke at a time,
    /// and every `'\n'` in it would run the auto-indent — silently adding
    /// leading whitespace to text the user copied from somewhere else.
    ///
    /// Carriage returns are folded into newlines first. A paste from a CRLF
    /// source would otherwise leave a stray `\r` *inside* a line, where it draws
    /// as a replacement box, never matches a search for the word beside it, and
    /// is written back out in the middle of the file on save.
    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let pieces: Vec<&str> = text.split('\n').collect();
        let line = self.cursor_line;
        let col = self.cursor_col;

        self.record_edit(line, 1, |doc| {
            let current = doc.lines.get(line).cloned().unwrap_or_default();
            let at = snap_to_boundary(&current, col);
            let (head, tail) = current.split_at(at);
            let first = pieces.first().copied().unwrap_or("");

            let mut replacement: Vec<String> = Vec::with_capacity(pieces.len());
            if pieces.len() == 1 {
                replacement.push(format!("{head}{first}{tail}"));
                doc.cursor_line = line;
                doc.cursor_col = head.len().saturating_add(first.len());
            } else {
                replacement.push(format!("{head}{first}"));
                let middle = pieces
                    .get(1..pieces.len().saturating_sub(1))
                    .unwrap_or_default();
                replacement.extend(middle.iter().map(|s| (*s).to_string()));
                let last = pieces.last().copied().unwrap_or("");
                replacement.push(format!("{last}{tail}"));
                doc.cursor_line = line.saturating_add(pieces.len()).saturating_sub(1);
                doc.cursor_col = last.len();
            }

            let from = line.min(doc.lines.len());
            let to = line.saturating_add(1).min(doc.lines.len());
            doc.lines.splice(from..to, replacement).for_each(drop);
            doc.selection_anchor = None;
        });
        self.clamp_cursor();
    }
}

/// Which of the three character classes word motion treats as one run:
/// identifier (0), whitespace (1), or punctuation (2).
///
/// Shared by [`Document::select_word_at_cursor`] and the word-motion pair so
/// that double-clicking a word selects exactly what Ctrl+Left/Ctrl+Right skip
/// over. Two separate definitions would drift, and the drift would show up as a
/// double-click selecting a different span than the keyboard does.
fn word_class(c: char) -> u8 {
    if c.is_alphanumeric() || c == '_' {
        0
    } else if c.is_whitespace() {
        1
    } else {
        2
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
                start = if end > at { end } else { at.saturating_add(1) };
            }
        }

        self.current_match = 0;
    }

    /// Go to next match.
    pub fn next_match(&mut self, doc: &mut Document) {
        if self.matches.is_empty() {
            return;
        }
        // Written as a comparison rather than `% len`: a remainder is a
        // division, and the compiler cannot see that the list is non-empty here.
        let ahead = self.current_match.saturating_add(1);
        self.current_match = if ahead >= self.matches.len() {
            0
        } else {
            ahead
        };
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
            self.current_match = self.current_match.saturating_sub(1);
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

/// Height of the tab strip along the top, in pixels.
///
/// A constant rather than a literal in each of the four places that used to
/// spell it, because the viewport height, the first text line's `y`, the find
/// panel's `y` and the strip itself must agree or text is drawn under
/// furniture.
pub const TAB_BAR_HEIGHT: f32 = 32.0;

/// Height of the status bar along the bottom, in pixels. See
/// [`TAB_BAR_HEIGHT`].
pub const STATUS_BAR_HEIGHT: f32 = 24.0;

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
    /// The last thing worth telling the user, shown in the status bar.
    ///
    /// Somewhere for a failure to go. Saving, closing a modified tab and
    /// opening a file can all be refused, and an editor driven by keystrokes
    /// has no return value to hand the refusal back through — without this the
    /// only options are to swallow the error or to print it to a console the
    /// user is not looking at.
    pub status: Option<String>,
    /// Whether the left button is down inside the text area, so that mouse
    /// movement extends the selection.
    ///
    /// Needed because `MouseEventKind::Move` does not say which buttons are
    /// held; the press and release are what bound a drag.
    pub dragging: bool,
    /// Modifiers as of the last key event.
    ///
    /// Mouse events do not carry them — [`guitk::event::MouseEvent`] is a
    /// position and a kind — so shift-click can only be recognised from what the
    /// keyboard last said. Tracked in `input`, read by the mouse handling there.
    pub modifiers: oswindow::Modifiers,
    /// The editor's own clipboard.
    ///
    /// Not the system clipboard: `gui/clipboard` is a *service binary*, not a
    /// library, and reaching it needs an IPC transport that does not exist yet
    /// (see `known-issues.md`, the transport step). Cut/copy/paste therefore
    /// work within this editor and nowhere else. When the clipboard protocol
    /// lands, this field becomes the local cache in front of it rather than the
    /// whole story.
    pub clipboard: String,
    /// Which of the find bar's two fields the keyboard is typing into.
    pub find_field: FindField,
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
            status: None,
            dragging: false,
            modifiers: oswindow::Modifiers::NONE,
            clipboard: String::new(),
            find_field: FindField::Query,
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
    ///
    /// Derived from the same two constants the renderer lays out with. It used
    /// to subtract 64 for a "toolbar" that is 32 pixels tall and is the tab bar,
    /// so it under-reported the viewport by about two lines: `render_editor`
    /// stopped drawing two lines above the status bar, leaving a strip of
    /// background, and `ensure_cursor_visible` scrolled two lines early. Two
    /// numbers that must agree are now one.
    pub fn visible_lines(&self) -> usize {
        let editor_height = self.window_height as f32 - TAB_BAR_HEIGHT - STATUS_BAR_HEIGHT;
        (editor_height / self.line_height).max(0.0) as usize
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
        let tab_h = TAB_BAR_HEIGHT;
        tree.fill_rect(
            0.0,
            0.0,
            self.window_width as f32,
            tab_h,
            Color::from_hex(0x181825),
        );

        let mut x = 0.0;
        for (i, doc) in self.tabs.iter().enumerate() {
            // The same width the hit test uses, so a click lands on the tab it
            // looks like it landed on.
            let tab_w = input::TAB_WIDTH;
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
            tree.text(x + 12.0, 9.0, &title, Color::from_hex(0xCDD6F4), 12.0);

            // Close button, drawn inside the box `tab_at` reports as the close
            // box so that the glyph and the clickable area coincide.
            tree.text(
                x + tab_w - input::TAB_CLOSE_WIDTH + 4.0,
                9.0,
                "x",
                Color::from_hex(0x6C7086),
                11.0,
            );

            x += tab_w + input::TAB_GAP;
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

    /// Draw one line as a single shaped run whose glyphs carry the colours of
    /// the tokens they came from.
    ///
    /// The whole line is drawn, shifted left by `scroll_px` and clipped to the
    /// band `left..right` at `y`, so a line thousands of columns wide costs one
    /// command rather than one per token — and the renderer still stops at
    /// `right`, so it rasterizes a screenful of glyphs rather than all of them.
    ///
    /// **This used to emit one `Text` command per token, each positioned at the
    /// sum of the previous tokens' widths, and that was wrong in a way no amount
    /// of care in the loop could fix.** Laying pieces out end to end *is* the
    /// assumption that screen order is byte order. Under a right-to-left run the
    /// glyphs of a token belong interleaved with those around them, so there is
    /// no single `x` at which the token can be drawn — the piece is not
    /// contiguous on the screen. Colour therefore has to be an attribute of a
    /// glyph rather than of a substring to draw, which is what
    /// `guitk::render::RenderCommand::RichText` makes it: the line is shaped
    /// once as a whole and each glyph takes the colour of the token containing
    /// the byte it came from.
    ///
    /// Two things fall out for free. Kerning across a token boundary is no
    /// longer lost — the comment that used to sit here accepted losing it as a
    /// small cost. And it is *faster*: each shaping carries a fixed cost of a
    /// few microseconds on top of the per-character cost, and cutting a line
    /// into *n* pieces paid it *n* times. An 80-character line of 40 tokens — an
    /// ordinary line of code — measured 2.3x the cost of shaping it whole. See
    /// `known-issues.md` → `TD-EDITOR-IS-NOT-BIDIRECTIONAL`.
    ///
    /// **Horizontal scrolling moves the run rather than shortening it**, which
    /// is the other half of the same argument. Drawing `line[scroll..]` would
    /// re-shape a *suffix*, and the bidi algorithm resolves visual order from
    /// the whole paragraph — so the suffix can come out ordered differently
    /// from the way those same characters sit in the complete line, and
    /// scrolling would rearrange the text instead of sliding it. Translating a
    /// single whole-line shaping and clipping it cannot do that. It also
    /// deletes the entire "scrolled into the middle of a character" family of
    /// bugs, because nothing is sliced any more.
    #[allow(clippy::too_many_arguments)]
    fn draw_tokens(
        tree: &mut RenderTree,
        line: &str,
        tokens: &[StyledToken],
        scroll_px: f32,
        left: f32,
        y: f32,
        right: f32,
        line_height: f32,
        font_size: f32,
    ) {
        if line.is_empty() || right <= left {
            return;
        }
        let plain = DEFAULT_THEME.color_for(Token::Plain);

        // Spans are cumulative — each runs from where the last ended — so a gap
        // the tokenizer left is not silently dropped but explicitly filled with
        // the plain colour. `every_byte_of_a_line_is_covered_by_some_token`
        // asserts there are no gaps; this is what keeps a future one from
        // mis-colouring rather than deleting the user's text off the screen.
        let mut spans: Vec<TextSpan> = Vec::with_capacity(tokens.len());
        let mut covered = 0usize;
        for token in tokens {
            if token.end <= covered {
                continue;
            }
            if token.start > covered {
                spans.push(TextSpan {
                    end: span_end(token.start),
                    color: plain,
                });
            }
            spans.push(TextSpan {
                end: span_end(token.end),
                color: DEFAULT_THEME.color_for(token.kind),
            });
            covered = token.end;
        }

        // The clip is what makes the leftward shift a scroll rather than a
        // spill: without it the glyphs scrolled off the left would paint over
        // the line-number gutter. `max_width` handles only the right edge —
        // the renderer computes `x + max_width`, so it has to be measured from
        // the shifted `x` to still land on `right`.
        // Clamped at zero so `right - x` — the width handed to the renderer —
        // is positive by construction rather than by an argument about who set
        // the scroll position. A negative scroll would mean the line starts
        // right of the text area, which is not a state worth representing.
        let x = left - scroll_px.max(0.0);
        tree.clip(left, y, right - left, line_height);
        // Bytes past the last span take the fallback colour, so the tail the
        // tokens did not reach needs no span of its own.
        tree.rich_text_clipped(x, y, right - x, line, spans, plain, font_size);
        tree.unclip();
    }

    fn render_editor(&self, tree: &mut RenderTree) {
        let doc = self.active_document();
        let editor_y = TAB_BAR_HEIGHT;
        let editor_h = self.window_height as f32 - TAB_BAR_HEIGHT - STATUS_BAR_HEIGHT;
        let w = self.window_width as f32;

        // Gutter (line numbers)
        tree.fill_rect(
            0.0,
            editor_y,
            self.gutter_width,
            editor_h,
            Color::from_hex(0x181825),
        );

        let visible_lines = self.visible_lines();
        let end_line = doc
            .scroll_line
            .saturating_add(visible_lines)
            .min(doc.lines.len());

        // The syntax state entering the first visible line. Everything above the
        // viewport has to be tokenized to know it — a block comment opened on
        // line 3 colours line 4000 — which is what `entry_state`'s memo is for.
        // From here the loop carries the state forward itself, since it is
        // tokenizing each visible line anyway.
        let mut state = doc.entry_state(doc.scroll_line);

        for i in doc.scroll_line..end_line {
            let y = editor_y + i.saturating_sub(doc.scroll_line) as f32 * self.line_height;

            // Line number
            let ln = format!("{:>4}", i.saturating_add(1));
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

            // Line text: one shaped run, coloured per glyph.
            let line = doc.lines.get(i).map_or("", String::as_str);

            // Selection band, drawn before the text so the glyphs sit on top
            // of it. Without this a selection is a state the editor knows
            // about and never shows, which reads as the selection keys doing
            // nothing at all.
            if let Some((from, to, trailing)) = Self::selection_on_line(doc, i) {
                let start_px = self.measure_prefix(line, from);
                let end_px = self.measure_prefix(line, to);
                let x = self.text_x() + start_px - doc.scroll_px.max(0.0);
                // A line whose selection continues onto the next one gets a
                // sliver past its last glyph, so the selected line break is
                // visible rather than the band appearing to stop early.
                let width = (end_px - start_px) + if trailing { self.font_size * 0.4 } else { 0.0 };
                let left = self.text_x();
                let clipped_x = x.max(left);
                let clipped_w = (x + width - clipped_x).min(w - clipped_x);
                if clipped_w > 0.0 {
                    tree.fill_rect(
                        clipped_x,
                        y,
                        clipped_w,
                        self.line_height,
                        Color::from_hex(0x45475A),
                    );
                }
            }

            let tokens = highlight::highlight_line(line, doc.language, &mut state);
            Self::draw_tokens(
                tree,
                line,
                &tokens,
                doc.scroll_px,
                self.text_x(),
                y + 3.0,
                w,
                self.line_height,
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
            let cursor_y = editor_y
                + doc.cursor_line.saturating_sub(doc.scroll_line) as f32 * self.line_height;
            // Measured from the start of the line and then shifted by the
            // scroll, rather than measured from the scroll position: the text
            // is one shaping of the whole line, so the caret has to be placed
            // in that same coordinate system or it disagrees with the glyphs
            // by whatever kerning crosses the scroll boundary.
            let cursor_x = self.text_x() - doc.scroll_px.max(0.0) + self.caret_offset_px(doc);
            // Clipped for the same reason the text is: a caret scrolled off the
            // left belongs behind the gutter, not drawn over the line numbers.
            tree.clip(self.text_x(), cursor_y, w - self.text_x(), self.line_height);
            tree.fill_rect(
                cursor_x,
                cursor_y + 2.0,
                2.0,
                self.line_height - 4.0,
                Color::from_hex(0x89B4FA),
            );
            tree.unclip();
        }
    }

    /// How far along its line the caret sits, in pixels from the line's start.
    ///
    /// Ignores the scroll position on purpose — this is the caret's place *in
    /// the line*, and where that lands on screen is the caller's business.
    ///
    /// Still a prefix measurement, and so still wrong for a bidirectional line:
    /// the caret between two characters of a right-to-left run is not at the
    /// summed width of the bytes before it. Fixing that needs the shaped run's
    /// cluster positions rather than a width, which is item 5 (step (e)) of
    /// `known-issues.md` → `TD-EDITOR-IS-NOT-BIDIRECTIONAL`. It is at least now
    /// wrong in one place instead of two, and consistently with nothing.
    fn caret_offset_px(&self, doc: &Document) -> f32 {
        let line = doc.lines.get(doc.cursor_line).map_or("", String::as_str);
        self.measure_prefix(line, doc.cursor_col)
    }

    /// Width of `line[..col]`, with `col` snapped to a character boundary.
    ///
    /// The one place a byte offset in a line becomes an x. Carries the same
    /// caveat as [`Self::caret_offset_px`]: a prefix width is not a position in
    /// a bidirectional line.
    fn measure_prefix(&self, line: &str, col: usize) -> f32 {
        // Snapped, not merely clamped: `col` is a byte offset and slicing
        // inside a character panics.
        let to = snap_to_boundary(line, col);
        let before = line.get(..to).unwrap_or("");
        text::measure(before, self.font_size, FontWeightHint::Regular)
    }

    /// The byte range of `line` covered by the selection, and whether the
    /// selection continues past the end of the line.
    ///
    /// `None` when the line is outside the selection or nothing is selected.
    fn selection_on_line(doc: &Document, line: usize) -> Option<(usize, usize, bool)> {
        let (start, end) = doc.selection_range();
        if start == end || line < start.line || line > end.line {
            return None;
        }
        let text_len = doc.lines.get(line).map_or(0, String::len);
        let from = if line == start.line { start.col } else { 0 };
        let to = if line == end.line { end.col } else { text_len };
        Some((from.min(text_len), to.min(text_len), line < end.line))
    }

    /// Which byte offset in which line a point in the window names.
    ///
    /// The inverse of [`Self::caret_offset_px`], and deliberately its
    /// neighbour: a click that does not land where the caret is then drawn is
    /// the most immediately visible bug an editor can have, and the two agree
    /// only if they measure the same way. `None` for a point outside the text
    /// area — the tab strip, the gutter or the status bar — because those are
    /// not places a caret can go.
    ///
    /// The column is the character boundary *nearest* the point rather than the
    /// one before it, so clicking the right half of a character puts the caret
    /// after it, which is what every other editor does and what the eye
    /// expects.
    #[must_use]
    pub fn caret_position_at(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let bottom = self.window_height as f32 - STATUS_BAR_HEIGHT;
        if y < TAB_BAR_HEIGHT || y >= bottom || x < self.text_x() {
            return None;
        }
        let doc = self.active_document();
        let row = ((y - TAB_BAR_HEIGHT) / self.line_height) as usize;
        let line = doc
            .scroll_line
            .saturating_add(row)
            .min(doc.lines.len().saturating_sub(1));
        let text = doc.lines.get(line).map_or("", String::as_str);

        // Where the click falls along the line, in the line's own coordinates.
        let target = x - self.text_x() + doc.scroll_px.max(0.0);

        // Walk the character boundaries, keeping the closest. Linear in the
        // line's length and quadratic in it overall, which is fine for a click
        // — it happens once, not once per frame — and is the only measurement
        // that stays correct when glyph widths differ.
        let mut best = 0usize;
        let mut best_distance = f32::INFINITY;
        for (offset, _) in text
            .char_indices()
            .chain(std::iter::once((text.len(), ' ')))
        {
            let distance = (self.measure_prefix(text, offset) - target).abs();
            if distance < best_distance {
                best_distance = distance;
                best = offset;
            }
        }
        Some((line, best))
    }

    /// Scroll horizontally so the caret is inside the text area.
    ///
    /// The companion to [`Document::ensure_cursor_visible`], and the reason
    /// [`Document::scroll_px`] is ever non-zero: without it a caret moved past
    /// the right edge of a long line simply vanishes, with no way to bring it
    /// back.
    ///
    /// Scrolls by whole *pixels* rather than to a character boundary, because
    /// there is no boundary to snap to any more — the run is translated, not
    /// sliced. A margin keeps the caret off the very edge, so typing at the end
    /// of a long line shows some of what comes before it rather than pinning the
    /// caret to the last column.
    pub fn ensure_caret_visible_horizontally(&mut self) {
        const MARGIN: f32 = 24.0;

        let width = self.window_width as f32 - self.text_x();
        if width <= 0.0 {
            return;
        }
        let caret = self.caret_offset_px(self.active_document());
        let doc = self.active_document_mut();
        let scroll = doc.scroll_px.max(0.0);

        // Left first, then right, so that on a viewport narrower than the
        // margins the caret ends up at the left edge — visible — rather than
        // oscillating between two unsatisfiable bounds.
        let scroll = if caret < scroll + MARGIN {
            (caret - MARGIN).max(0.0)
        } else if caret > scroll + width - MARGIN {
            (caret - width + MARGIN).max(0.0)
        } else {
            scroll
        };
        doc.scroll_px = scroll;
    }

    fn render_status_bar(&self, tree: &mut RenderTree) {
        let doc = self.active_document();
        let bar_y = self.window_height as f32 - STATUS_BAR_HEIGHT;
        let w = self.window_width as f32;

        tree.fill_rect(0.0, bar_y, w, STATUS_BAR_HEIGHT, Color::from_hex(0x181825));

        // A message takes the bar over. It is there because something the user
        // asked for did not happen, which matters more for the moment than the
        // line number they can also see in the caret's position.
        if let Some(message) = self.status.as_deref() {
            tree.text(8.0, bar_y + 5.0, message, Color::from_hex(0xF9E2AF), 11.0);
            return;
        }

        // Cursor position
        let pos_text = format!(
            "Ln {}, Col {}",
            doc.cursor_line.saturating_add(1),
            doc.cursor_col.saturating_add(1)
        );
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
        let panel_y = TAB_BAR_HEIGHT;
        let panel_w = 350.0;
        let panel_h = 80.0;
        let panel_x = self.window_width as f32 - panel_w - 16.0;

        tree.fill_rect(
            panel_x,
            panel_y,
            panel_w,
            panel_h,
            Color::from_hex(0x313244),
        );
        tree.stroke_rect(
            panel_x,
            panel_y,
            panel_w,
            panel_h,
            Color::from_hex(0x585B70),
            1.0,
        );

        // Find input
        tree.text(
            panel_x + 8.0,
            panel_y + 10.0,
            "Find:",
            Color::from_hex(0xA6ADC8),
            11.0,
        );
        tree.fill_rect(
            panel_x + 50.0,
            panel_y + 6.0,
            panel_w - 60.0,
            22.0,
            Color::from_hex(0x1E1E2E),
        );
        tree.text(
            panel_x + 54.0,
            panel_y + 10.0,
            &self.find.query,
            Color::from_hex(0xCDD6F4),
            12.0,
        );

        // Replace input
        tree.text(
            panel_x + 8.0,
            panel_y + 40.0,
            "Repl:",
            Color::from_hex(0xA6ADC8),
            11.0,
        );
        tree.fill_rect(
            panel_x + 50.0,
            panel_y + 36.0,
            panel_w - 60.0,
            22.0,
            Color::from_hex(0x1E1E2E),
        );
        tree.text(
            panel_x + 54.0,
            panel_y + 40.0,
            &self.find.replace_text,
            Color::from_hex(0xCDD6F4),
            12.0,
        );

        // Match count
        let match_info = format!("{} match(es)", self.find.matches.len());
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
                format!(
                    "\"{name}\" was deleted outside the editor while you have unsaved changes."
                ),
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
            tree.text(dx + 160.0, by + 8.0, hint, Color::from_hex(0x9399B2), 10.0);
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
        tree.text(
            dx + 12.0,
            dy + 9.0,
            &header,
            Color::from_hex(0xF9E2AF),
            13.0,
        );

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

            let label = format!("#{}", i.saturating_add(1));
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

/// Drive the editor on `window` until the user quits or the connection closes.
///
/// Everything the editor decides is in [`EditorState::handle_event`]; this is
/// only the strap between it and the loop. The one thing it adds is *when to
/// draw*: a frame is submitted for the initial state and thereafter exactly when
/// an event reported a visible change. Redrawing unconditionally would repaint
/// the whole document for every mouse move across the window.
fn run<T: oswindow::ConnectionTransport>(
    events: &mut EventLoop<T>,
    window: u64,
    editor: &mut EditorState,
) -> Result<(), oswindow::Error<T>> {
    // Nothing has happened yet, so no event is going to ask for the first frame.
    events.submit(window, &editor.render())?;

    // A failed submit has nowhere to be reported from inside the handler — its
    // return type is the loop's `EventResponse`, not a `Result` — so it is
    // carried out here and the loop stopped. Swallowing it would leave an
    // editor that runs on happily while the screen no longer changes.
    let mut failure = None;
    events.run(|events, id, event| {
        if id != window {
            return EventResponse::Continue;
        }
        match editor.handle_event(&event) {
            EditorResponse::Idle => EventResponse::Continue,
            EditorResponse::Redraw => {
                if let Err(e) = events.submit(id, &editor.render()) {
                    failure = Some(e);
                    return EventResponse::Exit;
                }
                EventResponse::Continue
            }
            EditorResponse::Exit => EventResponse::Exit,
        }
    })?;

    failure.map_or(Ok(()), Err)
}

/// Obtain a transport to the compositor.
///
/// There is nothing to connect to yet, so this always returns `None`. Both ends
/// of the display protocol are finished — the editor speaks it through
/// `oswindow`, and the compositor decodes it in `gui/compositor/src/wire.rs` —
/// but no channel exists between two processes to carry the bytes. That is a
/// tracked, separate piece of work (`known-issues.md`,
/// `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`).
///
/// It is a function returning `None` rather than an unimplemented `main`
/// precisely so the code above it is real: `run` is written against
/// [`oswindow::ConnectionTransport`] and compiles, and the day this returns a
/// socket the editor starts working with no other change here.
fn connect() -> Option<oswindow::Pipe> {
    None
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut editor = EditorState::new();

    for path_str in &args {
        let path = PathBuf::from(path_str);
        if let Err(e) = editor.open_file(&path) {
            eprintln!("editor: cannot open {}: {e}", path.display());
        }
    }

    let Some(transport) = connect() else {
        eprintln!("editor: no connection to the compositor.");
        eprintln!("  The editor and the compositor both speak the display protocol, but");
        eprintln!("  nothing carries it between processes yet. See known-issues.md,");
        eprintln!("  TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR.");
        std::process::exit(1);
    };

    let title = format!("{} — Editor", editor.active_document().name);
    let mut events = EventLoop::new(transport);
    let window = match WindowBuilder::new(title, editor.window_width, editor.window_height)
        .resizable(true)
        .build(&mut events)
    {
        Ok(id) => id,
        Err(e) => {
            eprintln!("editor: the compositor refused the window: {e:?}");
            std::process::exit(1);
        }
    };

    if let Err(e) = run(&mut events, window, &mut editor) {
        eprintln!("editor: the connection failed: {e:?}");
        std::process::exit(1);
    }
}

/// The editor as a compositor client, driven end to end.
///
/// These go through the real protocol — the requests are encoded, decoded by
/// `oswindow::testing`'s compositor, and answered, and the frames the editor
/// draws come back as decoded submissions. What they check is the one thing
/// [`run`] adds over [`EditorState::handle_event`]: *when* a frame is sent.
#[cfg(test)]
mod loop_tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::{EditorState, run};
    use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseEvent, MouseEventKind};
    use oswindow::testing;
    use oswindow::{InputEvent, WindowBuilder};

    fn typed(ch: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some(ch),
        })
    }

    #[test]
    fn the_editor_opens_a_window_draws_once_and_then_only_on_change() {
        let (mut events, desktop) = testing::desktop();
        let window = WindowBuilder::new("Editor", 900, 600)
            .resizable(true)
            .build(&mut events)
            .expect("the compositor should have created the window");

        // Each batch is delivered on one turn of the loop, so these arrive in
        // order with the editor processing each before the next appears.
        {
            let mut desk = desktop.borrow_mut();
            // A mouse move with no button held: nothing changes, nothing draws.
            desk.script.push_back(vec![InputEvent::new(
                window,
                Event::Mouse(MouseEvent {
                    x: 400.0,
                    y: 300.0,
                    kind: MouseEventKind::Move,
                }),
            )]);
            desk.script
                .push_back(vec![InputEvent::new(window, typed('a'))]);
            desk.script
                .push_back(vec![InputEvent::new(window, Event::CloseRequested)]);
        }

        let mut editor = EditorState::new();
        run(&mut events, window, &mut editor).expect("the loopback connection cannot fail");

        assert_eq!(editor.active_document().lines[0], "a", "the key arrived");

        let drawn = desktop.borrow_mut().drawn();
        assert_eq!(
            drawn.len(),
            2,
            "the initial frame and one for the keystroke, not one per event: {drawn:?}"
        );
        assert!(
            drawn.iter().all(|(w, count)| *w == window && *count > 0),
            "every frame is a real picture of this window: {drawn:?}"
        );
    }

    #[test]
    fn events_for_another_window_are_ignored_rather_than_applied() {
        let (mut events, desktop) = testing::desktop();
        let mine = WindowBuilder::new("Editor", 900, 600)
            .build(&mut events)
            .unwrap();
        let theirs = WindowBuilder::new("Other", 100, 100)
            .build(&mut events)
            .unwrap();
        assert_ne!(mine, theirs);

        {
            let mut desk = desktop.borrow_mut();
            desk.script
                .push_back(vec![InputEvent::new(theirs, typed('z'))]);
            desk.script
                .push_back(vec![InputEvent::new(mine, Event::CloseRequested)]);
        }

        let mut editor = EditorState::new();
        run(&mut events, mine, &mut editor).unwrap();

        assert_eq!(
            editor.active_document().lines[0],
            "",
            "a keystroke addressed to another window must not edit this document"
        );
        let drawn = desktop.borrow_mut().drawn();
        assert_eq!(drawn, vec![(mine, drawn[0].1)], "only the initial frame");
    }
}

/// A tour of the model's API, kept as a test.
///
/// This was the body of `main` before there was an event loop: the only way to
/// exercise the editor was to call its operations from a program and print what
/// came out. It is kept because it is a compact walk through the whole model —
/// open, render, type, undo, redo, outline, expand-selection — but the prints
/// are now assertions, so a regression in any of them fails a build instead of
/// changing a line of console output nobody reads.
#[cfg(test)]
mod api_tour {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::{Document, EditorState, Language};

    #[test]
    fn the_model_answers_every_operation_the_demo_used_to_print() {
        let mut editor = EditorState::new();

        // Rendering an empty editor still produces a frame: the chrome — tab
        // bar, gutter, status bar — is drawn whether or not there is text.
        let render = editor.render();
        assert!(
            render.len() > 3,
            "an empty editor still draws its chrome, got {} commands",
            render.len()
        );
        assert_eq!(editor.active_document().line_count(), 1);

        let doc = editor.active_document_mut();
        for ch in "Hello".chars() {
            doc.insert_char(ch);
        }
        assert_eq!(doc.line(0), "Hello");

        doc.undo();
        doc.undo();
        assert_eq!(doc.line(0), "Hel", "one character per undo step");

        doc.redo();
        assert_eq!(doc.line(0), "Hell");
    }

    #[test]
    fn expand_selection_walks_out_through_the_nesting() {
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
        assert_eq!(outline.len(), 2, "both functions are headers: {outline:?}");
        assert!(outline[1].0 > outline[0].0, "`inner` nests inside `outer`");

        sample.cursor_line = 2;
        sample.cursor_col = 12;
        sample.selection_anchor = None;

        // Each expansion must cover the previous one and reach strictly
        // further, which is the property that makes repeated presses useful.
        let mut previous: Option<(super::Pos, super::Pos)> = None;
        let mut steps = 0;
        while sample.expand_selection() && steps < 8 {
            let range = sample.selection_range();
            if let Some((was_start, was_end)) = previous {
                assert!(
                    range.0 <= was_start && range.1 >= was_end && range != (was_start, was_end),
                    "step {steps} did not grow: {range:?} vs {:?}",
                    (was_start, was_end)
                );
            }
            previous = Some(range);
            steps += 1;
        }
        assert!(steps >= 2, "expected several expansions, got {steps}");
    }
}

// ============================================================================
// Caret placement
// ============================================================================

#[cfg(test)]
mod caret_tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

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

    /// Whether the line is at its unscrolled resting position.
    ///
    /// Exact zero is what the code produces (`.max(0.0)` of a negative is
    /// exactly `0.0`), but comparing floats with `==` is the habit that makes
    /// the *next* such assertion wrong, so it is spelled as a tolerance.
    fn unscrolled(editor: &EditorState) -> bool {
        editor.active_document().scroll_px.abs() < f32::EPSILON
    }

    /// The x of the caret in a rendered frame.
    fn caret_x(editor: &EditorState) -> f32 {
        let tree = editor.render();
        tree.commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::FillRect {
                    x, width, color, ..
                } if *color == Color::from_hex(CARET) && (*width - 2.0).abs() < 0.01 => Some(*x),
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

    /// A horizontal scroll slides the caret by exactly the scrolled distance.
    ///
    /// The old model measured `line[scroll..cursor]` and so had to agree with a
    /// *re-shaping* of the visible suffix; this one measures the whole prefix
    /// once and subtracts, which is the same coordinate system the glyphs are
    /// drawn in.
    #[test]
    fn a_horizontal_scroll_slides_the_caret_by_the_scrolled_distance() {
        let unscrolled = caret_x(&editor_with("hello world", 8));

        let mut editor = editor_with("hello world", 8);
        editor.active_document_mut().scroll_px = 37.0;
        assert!(
            (caret_x(&editor) - (unscrolled - 37.0)).abs() < 0.01,
            "a 37px scroll moved the caret from {unscrolled} to {}, not by 37",
            caret_x(&editor)
        );
    }

    /// The caret is measured from the start of the line, not from the scroll
    /// position — so it keeps whatever kerning crosses the scroll boundary, and
    /// agrees with the single whole-line shaping the glyphs come from.
    #[test]
    fn the_caret_is_measured_over_the_whole_prefix_not_the_visible_part() {
        let mut editor = editor_with("hello world", 8);
        editor.active_document_mut().scroll_px = 20.0;
        let expected = editor.gutter_width + 8.0 - 20.0
            + text::measure("hello wo", editor.font_size, FontWeightHint::Regular);
        assert!(
            (caret_x(&editor) - expected).abs() < 0.01,
            "the caret is at {}, but the whole prefix ends at {expected}",
            caret_x(&editor)
        );
    }

    /// Horizontal auto-scroll: a caret past the right edge brings the view to
    /// it. Without this a long line's end is simply unreachable.
    #[test]
    fn a_caret_past_the_right_edge_scrolls_the_view_to_it() {
        let line = "x".repeat(4000);
        let mut editor = editor_with(&line, 4000);
        assert!(unscrolled(&editor), "a fresh document starts scrolled");

        editor.ensure_caret_visible_horizontally();
        let scroll = editor.active_document().scroll_px;
        assert!(scroll > 0.0, "a caret 4000 characters along did not scroll");

        let caret = caret_x(&editor);
        assert!(
            caret >= editor.text_x() && caret <= editor.window_width as f32,
            "after scrolling, the caret is at {caret}, outside the text area \
             {}..{}",
            editor.text_x(),
            editor.window_width
        );
    }

    /// And back again: the same call scrolls left when the caret is behind the
    /// view, and lands on exactly zero at the start of the line rather than a
    /// negative offset that would leave a gap at the left edge.
    #[test]
    fn a_caret_before_the_left_edge_scrolls_back_and_stops_at_zero() {
        let line = "x".repeat(4000);
        let mut editor = editor_with(&line, 4000);
        editor.ensure_caret_visible_horizontally();
        assert!(editor.active_document().scroll_px > 0.0);

        editor.active_document_mut().cursor_col = 0;
        editor.ensure_caret_visible_horizontally();
        assert!(
            unscrolled(&editor),
            "a caret at column 0 left the line scrolled by {}",
            editor.active_document().scroll_px,
        );
    }

    /// A caret already comfortably inside the view does not move the view at
    /// all — auto-scroll that fires on every keystroke would make the text
    /// crawl sideways as the user types.
    #[test]
    fn a_caret_already_in_view_does_not_scroll() {
        let mut editor = editor_with("hello world", 5);
        editor.ensure_caret_visible_horizontally();
        assert!(
            unscrolled(&editor),
            "a caret in plain view scrolled the line by {}",
            editor.active_document().scroll_px,
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
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::render::RenderCommand;

    /// Every coloured stretch of text in a rendered frame, as `(text, colour)`.
    ///
    /// A `RichText` command contributes one entry per span rather than one for
    /// the whole string, because a span is exactly "this much of the line, in
    /// this colour" — the thing these tests are about. No `x` accompanies it:
    /// a span *has* no independent x, which is the entire point of the command
    /// (a run is positioned once and shaped once; where within it a given byte
    /// lands is the renderer's answer, not the caller's).
    fn drawn(editor: &EditorState) -> Vec<(String, Color)> {
        let mut out = Vec::new();
        for c in &editor.render().commands {
            match c {
                RenderCommand::Text { text, color, .. } => out.push((text.clone(), *color)),
                RenderCommand::RichText {
                    text, spans, color, ..
                } => {
                    let mut at = 0usize;
                    for span in spans {
                        let end = (span.end as usize).min(text.len());
                        if let Some(s) = text.get(at..end) {
                            out.push((s.to_string(), span.color));
                        }
                        at = end;
                    }
                    // Bytes past the last span take the command's own colour.
                    if let Some(tail) = text.get(at..)
                        && !tail.is_empty()
                    {
                        out.push((tail.to_string(), *color));
                    }
                }
                _ => {}
            }
        }
        out
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

    /// The renderer colours each byte by the token claiming it, so a byte no
    /// token claims falls back to the plain colour. `draw_tokens` fills such a
    /// gap defensively rather than dropping it — this is the assertion that says
    /// the defence should never have to fire.
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
            runs.iter().any(|(t, c)| t == "fn" && *c == keyword),
            "`fn` was not drawn in the keyword colour; runs were {runs:?}"
        );
        assert!(
            runs.iter().any(|(t, c)| t == "main" && *c == function),
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
                .any(|(t, c)| t.contains("still a comment") && *c == comment),
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
                .any(|(t, c)| t.contains("body line 149") && *c == comment),
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

    /// A scroll position can no longer land inside a character, because it is
    /// no longer an offset into the text — but `cursor_col` still is, and the
    /// caret measurement slices the line at it. This walks both across a line
    /// with a multi-byte character in it.
    ///
    /// The old version of this test drove `scroll_col` through every byte to
    /// prove the *slice* did not panic. There is no slice now; what is left to
    /// prove is that an arbitrary pixel scroll and an arbitrary byte cursor
    /// still render.
    #[test]
    fn an_arbitrary_scroll_and_cursor_position_does_not_panic() {
        let mut editor = editor_showing("caf\u{e9} au lait", Language::Plain);
        for i in 0..16 {
            editor.active_document_mut().scroll_px = i as f32 * 7.5;
            editor.active_document_mut().cursor_col = i;
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
            doc.line(0),
            "ab",
            "undo removed the character after the one it inserted"
        );
    }

    // ---- syntax colouring is per glyph, not per drawn substring ------------

    /// The one `RichText` command `draw_tokens` emits, or a panic naming what
    /// it emitted instead.
    ///
    /// `left` is 20.0 rather than 0.0 so a scroll actually has somewhere to
    /// move the run *to*: at `left == 0` a correct shift and a clamped-to-zero
    /// one are indistinguishable.
    struct Drawn {
        x: f32,
        text: String,
        spans: Vec<TextSpan>,
        max_width: Option<f32>,
    }

    const TEST_LEFT: f32 = 20.0;

    fn tokens_drawn(line: &str, language: Language, scroll_px: f32, right: f32) -> Drawn {
        let mut tree = RenderTree::new();
        let mut state = HighlightState::Normal;
        let tokens = highlight::highlight_line(line, language, &mut state);
        EditorState::draw_tokens(
            &mut tree, line, &tokens, scroll_px, TEST_LEFT, 0.0, right, 18.0, 14.0,
        );
        first_rich(&tree)
    }

    /// The `RichText` command in a tree, wherever the clip put it.
    fn first_rich(tree: &RenderTree) -> Drawn {
        let found = tree.commands.iter().find_map(|c| match c {
            guitk::render::RenderCommand::RichText {
                x,
                text,
                spans,
                max_width,
                ..
            } => Some(Drawn {
                x: *x,
                text: text.clone(),
                spans: spans.clone(),
                max_width: *max_width,
            }),
            _ => None,
        });
        match found {
            Some(d) => d,
            None => panic!("expected a RichText command, got {:?}", tree.commands),
        }
    }

    /// **The regression this exists to prevent.** A highlighted line must be
    /// *one* text command: one command is one shaping, and one shaping is the
    /// only arrangement under which a right-to-left run can be ordered
    /// correctly. A command per token is the assumption that screen order is
    /// byte order, and no amount of care inside the loop repairs it.
    ///
    /// The clip around it is not a text command and does not count — but it is
    /// asserted here too, because a clip that went missing would turn the
    /// horizontal scroll into a spill over the line-number gutter.
    #[test]
    fn a_highlighted_line_is_drawn_as_one_shaped_run() {
        use guitk::render::RenderCommand as Rc;

        let mut tree = RenderTree::new();
        let line = "fn main() { let x: u32 = 0x1F; /* hi */ }";
        let mut state = HighlightState::Normal;
        let tokens = highlight::highlight_line(line, Language::Rust, &mut state);
        assert!(
            tokens.len() > 4,
            "this line should tokenize into several runs"
        );
        EditorState::draw_tokens(
            &mut tree, line, &tokens, 0.0, TEST_LEFT, 0.0, 800.0, 18.0, 14.0,
        );

        let texts = tree
            .commands
            .iter()
            .filter(|c| matches!(c, Rc::Text { .. } | Rc::RichText { .. }))
            .count();
        assert_eq!(
            texts,
            1,
            "a {}-token line produced {texts} text commands; it must produce one",
            tokens.len(),
        );
        assert!(
            matches!(tree.commands.first(), Some(Rc::PushClip { x, width, .. })
                if (*x - TEST_LEFT).abs() < 0.01 && (*width - (800.0 - TEST_LEFT)).abs() < 0.01),
            "the run is not clipped to the text area: {:?}",
            tree.commands.first(),
        );
        assert!(
            matches!(tree.commands.last(), Some(Rc::PopClip)),
            "the clip is not popped",
        );
    }

    /// Every byte of the drawn text must resolve to the colour of the token
    /// containing it — the property the old per-token loop had by construction
    /// and this one has to be shown to have.
    #[test]
    fn every_byte_resolves_to_its_own_token_colour() {
        let line = "fn main() { let x: u32 = 0x1F; /* hi */ }";
        let Drawn { text, spans, .. } = tokens_drawn(line, Language::Rust, 0.0, 800.0);
        assert_eq!(text, line);

        let mut state = HighlightState::Normal;
        let tokens = highlight::highlight_line(line, Language::Rust, &mut state);
        for token in &tokens {
            for byte in token.start..token.end {
                assert_eq!(
                    TextSpan::color_at(&spans, byte),
                    Some(DEFAULT_THEME.color_for(token.kind)),
                    "byte {byte} of {line:?} is in {token:?} but resolved elsewhere",
                );
            }
        }
    }

    /// **Spans are offsets into the whole line, at every scroll position.**
    ///
    /// This is the assertion that the scroll no longer slices: a scrolled line
    /// draws the *same string* with the *same spans* as an unscrolled one, and
    /// differs only in where it is put. The previous model drew `line[scroll..]`
    /// and rebased every span against it, which is what made a scrolled
    /// bidirectional line a differently-ordered shaping rather than the same
    /// line moved.
    #[test]
    fn a_scroll_changes_only_where_the_line_is_drawn_not_what() {
        let line = "let alpha = \"beta\";";
        let at_rest = tokens_drawn(line, Language::Rust, 0.0, 800.0);
        let scrolled = tokens_drawn(line, Language::Rust, 60.0, 800.0);

        assert_eq!(scrolled.text, line, "a scroll sliced the line");
        assert_eq!(scrolled.text, at_rest.text);
        assert_eq!(scrolled.spans, at_rest.spans, "a scroll rebased the spans");
        assert!(
            (scrolled.x - (at_rest.x - 60.0)).abs() < 0.01,
            "a 60px scroll moved the run from {} to {}, not by 60",
            at_rest.x,
            scrolled.x,
        );

        let mut state = HighlightState::Normal;
        let tokens = highlight::highlight_line(line, Language::Rust, &mut state);
        for token in &tokens {
            for byte in token.start..token.end {
                assert_eq!(
                    TextSpan::color_at(&scrolled.spans, byte),
                    Some(DEFAULT_THEME.color_for(token.kind)),
                    "byte {byte} of {line:?} resolved wrongly while scrolled",
                );
            }
        }
    }

    /// The bound is what lets the renderer stop: without it a line thousands of
    /// columns wide costs a glyph per column every frame, of which a screenful
    /// is visible.
    ///
    /// It is measured from the *shifted* x, because the renderer computes its
    /// stopping point as `x + max_width`. Getting this wrong is invisible until
    /// someone scrolls, and then truncates the line early by exactly the scroll
    /// distance.
    #[test]
    fn the_drawn_run_is_bounded_by_the_viewport() {
        let d = tokens_drawn("let x = 1;", Language::Rust, 0.0, 640.0);
        assert_eq!(d.max_width, Some(640.0 - TEST_LEFT));

        let s = tokens_drawn("let x = 1;", Language::Rust, 100.0, 640.0);
        assert_eq!(
            s.max_width,
            Some(640.0 - TEST_LEFT + 100.0),
            "a scrolled run stops short of the viewport's right edge",
        );
        // The bound is only correct if it *lands* on the viewport edge once the
        // shift is applied, which is the property the arithmetic exists for.
        let stop = s.x + 640.0 - TEST_LEFT + 100.0;
        assert!(
            (stop - 640.0).abs() < 0.01,
            "the scrolled run stops at {stop}, not at the viewport edge 640",
        );
    }

    /// A scroll past the end of the line still draws it — off to the left,
    /// where the clip hides it. There is nothing to slice and so nothing to
    /// clamp, which is the point: the failure mode the old `scroll_col` needed
    /// guarding against does not exist here.
    #[test]
    fn scrolling_past_the_end_of_a_line_draws_it_off_to_the_left() {
        let d = tokens_drawn("short", Language::Rust, 9_000.0, 800.0);
        assert_eq!(d.text, "short");
        assert!(d.x < 0.0, "a 9000px scroll left the run at x={}", d.x);
    }

    /// An empty line draws nothing at all — no clip, no run. A zero-width
    /// viewport likewise, which is what a window narrower than its own gutter
    /// produces.
    #[test]
    fn an_empty_line_or_viewport_draws_nothing() {
        let mut tree = RenderTree::new();
        EditorState::draw_tokens(&mut tree, "", &[], 0.0, TEST_LEFT, 0.0, 800.0, 18.0, 14.0);
        assert_eq!(tree.len(), 0, "an empty line drew {:?}", tree.commands);

        let mut tree = RenderTree::new();
        let mut state = HighlightState::Normal;
        let tokens = highlight::highlight_line("x", Language::Rust, &mut state);
        EditorState::draw_tokens(&mut tree, "x", &tokens, 0.0, 800.0, 0.0, 800.0, 18.0, 14.0);
        assert_eq!(
            tree.len(),
            0,
            "a zero-width viewport drew {:?}",
            tree.commands
        );
    }

    /// A multi-byte line is never sliced, at any scroll position — the whole
    /// class of "scrolled into the middle of a character" bug is gone, not
    /// merely guarded against.
    #[test]
    fn a_multi_byte_line_is_drawn_whole_at_every_scroll_position() {
        let line = "日本語 x";
        for i in 0..line.len() {
            let d = tokens_drawn(line, Language::Rust, i as f32 * 3.5, 800.0);
            assert_eq!(
                d.text, line,
                "scroll {i} drew {:?}, not the whole line",
                d.text
            );
        }
    }

    /// A gap the tokenizer leaves is filled with the plain colour rather than
    /// shifting every later span — the cumulative representation makes a gap
    /// impossible to express, so it has to be filled explicitly.
    #[test]
    fn a_gap_between_tokens_is_filled_with_the_plain_colour() {
        let line = "abcdef";
        // Hand-built tokens with a hole at bytes 2..4, which the tokenizer is
        // asserted never to produce but the renderer must survive.
        let tokens = vec![
            StyledToken {
                start: 0,
                end: 2,
                kind: Token::Keyword,
            },
            StyledToken {
                start: 4,
                end: 6,
                kind: Token::Number,
            },
        ];
        let mut tree = RenderTree::new();
        EditorState::draw_tokens(
            &mut tree, line, &tokens, 0.0, TEST_LEFT, 0.0, 800.0, 18.0, 14.0,
        );
        let spans = &first_rich(&tree).spans;
        let plain = DEFAULT_THEME.color_for(Token::Plain);
        assert_eq!(
            TextSpan::color_at(spans, 0),
            Some(DEFAULT_THEME.color_for(Token::Keyword))
        );
        assert_eq!(TextSpan::color_at(spans, 2), Some(plain));
        assert_eq!(TextSpan::color_at(spans, 3), Some(plain));
        assert_eq!(
            TextSpan::color_at(spans, 4),
            Some(DEFAULT_THEME.color_for(Token::Number))
        );
    }

    /// Bytes past the last token take the fallback colour, which is the plain
    /// one — so a tokenizer that stops short mis-colours a tail rather than
    /// losing it.
    #[test]
    fn a_tail_past_the_last_token_needs_no_span() {
        let line = "abcdef";
        let tokens = vec![StyledToken {
            start: 0,
            end: 2,
            kind: Token::Keyword,
        }];
        let mut tree = RenderTree::new();
        EditorState::draw_tokens(
            &mut tree, line, &tokens, 0.0, TEST_LEFT, 0.0, 800.0, 18.0, 14.0,
        );
        let d = first_rich(&tree);
        assert_eq!(d.text, line);
        assert_eq!(TextSpan::color_at(&d.spans, 2), None);
    }

    /// `span_end` is what keeps an out-of-range offset a colouring bug rather
    /// than a crash: it saturates instead of truncating to a wrapped-around
    /// value that would colour the wrong part of the line.
    #[test]
    fn a_span_end_past_the_u32_range_saturates() {
        assert_eq!(span_end(0), 0);
        assert_eq!(span_end(14), 14);
        assert_eq!(span_end(usize::MAX), u32::MAX);
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
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

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
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

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
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

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
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

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
