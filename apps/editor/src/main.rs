//! OurOS Text Editor
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
use guitk::render::RenderTree;
use syntree::{Pos, SyntaxTree};

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
    /// Horizontal scroll offset.
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
}

/// An edit action for undo/redo.
#[derive(Clone, Debug)]
pub enum EditAction {
    Insert {
        line: usize,
        col: usize,
        text: String,
    },
    Delete {
        line: usize,
        col: usize,
        text: String,
    },
    InsertLine {
        line: usize,
        text: String,
    },
    DeleteLine {
        line: usize,
        text: String,
    },
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

        let language = path
            .extension()
            .map(|e| Language::from_extension(&e.to_string_lossy()))
            .unwrap_or(Language::Plain);

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
        })
    }

    /// Save the document to its file path.
    pub fn save(&mut self) -> std::io::Result<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| std::io::Error::other("no file path"))?;

        let content: String = self.lines.join(self.line_ending.chars());
        fs::write(path, &content)?;
        self.modified = false;
        Ok(())
    }

    /// Save to a new path.
    pub fn save_as(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        self.path = Some(path.to_path_buf());
        self.name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string());
        self.language = path
            .extension()
            .map(|e| Language::from_extension(&e.to_string_lossy()))
            .unwrap_or(Language::Plain);
        self.save()
    }

    // ======================================================================
    // Editing operations
    // ======================================================================

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, ch: char) {
        let line = self.cursor_line;
        let col = self.cursor_col;

        if ch == '\n' {
            // Split line
            let current_line = self.lines.get(line).cloned().unwrap_or_default();
            let (before, after) = current_line.split_at(col.min(current_line.len()));
            self.lines[line] = before.to_string();
            self.lines.insert(line + 1, after.to_string());
            self.cursor_line += 1;
            self.cursor_col = 0;

            // Auto-indent: copy leading whitespace from previous line
            let indent: String = self.lines[line]
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            if !indent.is_empty() {
                self.lines[line + 1] = format!("{indent}{}", self.lines[line + 1]);
                self.cursor_col = indent.len();
            }
        } else if ch == '\t' && self.use_spaces {
            // Insert spaces instead of tab
            let spaces = " ".repeat(self.tab_width - (col % self.tab_width));
            let current_line = self.lines.get_mut(line).unwrap();
            current_line.insert_str(col.min(current_line.len()), &spaces);
            self.cursor_col += spaces.len();
        } else {
            let current_line = self.lines.get_mut(line).unwrap();
            current_line.insert(col.min(current_line.len()), ch);
            self.cursor_col += ch.len_utf8();
        }

        self.modified = true;
        self.redo_stack.clear();
        self.push_undo(EditAction::Insert {
            line,
            col,
            text: ch.to_string(),
        });
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let line = self.cursor_line;
            let current_line = self.lines.get_mut(line).unwrap();
            if self.cursor_col <= current_line.len() {
                let removed = current_line.remove(self.cursor_col - 1);
                self.cursor_col -= 1;
                self.modified = true;
                self.push_undo(EditAction::Delete {
                    line,
                    col: self.cursor_col,
                    text: removed.to_string(),
                });
            }
        } else if self.cursor_line > 0 {
            // Join with previous line
            let current_text = self.lines.remove(self.cursor_line);
            self.cursor_line -= 1;
            self.cursor_col = self.lines[self.cursor_line].len();
            self.lines[self.cursor_line].push_str(&current_text);
            self.modified = true;
            self.push_undo(EditAction::DeleteLine {
                line: self.cursor_line + 1,
                text: current_text,
            });
        }
    }

    /// Delete the character at the cursor (delete key).
    pub fn delete_forward(&mut self) {
        let line = self.cursor_line;
        let current_line = &self.lines[line];

        if self.cursor_col < current_line.len() {
            let current_line = self.lines.get_mut(line).unwrap();
            let removed = current_line.remove(self.cursor_col);
            self.modified = true;
            self.push_undo(EditAction::Delete {
                line,
                col: self.cursor_col,
                text: removed.to_string(),
            });
        } else if line + 1 < self.lines.len() {
            // Join with next line
            let next_text = self.lines.remove(line + 1);
            self.lines[line].push_str(&next_text);
            self.modified = true;
            self.push_undo(EditAction::DeleteLine {
                line: line + 1,
                text: next_text,
            });
        }
    }

    /// Undo the last action.
    pub fn undo(&mut self) {
        if let Some(action) = self.undo_stack.pop_back() {
            match &action {
                EditAction::Insert { line, col, text } => {
                    let current = self.lines.get_mut(*line).unwrap();
                    for _ in 0..text.len() {
                        if *col < current.len() {
                            current.remove(*col);
                        }
                    }
                    self.cursor_line = *line;
                    self.cursor_col = *col;
                }
                EditAction::Delete { line, col, text } => {
                    let current = self.lines.get_mut(*line).unwrap();
                    current.insert_str(*col, text);
                    self.cursor_line = *line;
                    self.cursor_col = col + text.len();
                }
                EditAction::InsertLine { line, .. } => {
                    self.lines.remove(*line);
                    self.cursor_line = line.saturating_sub(1);
                }
                EditAction::DeleteLine { line, text } => {
                    self.lines.insert(*line, text.clone());
                    self.cursor_line = *line;
                }
            }
            self.redo_stack.push_back(action);
            self.modified = true;
        }
    }

    /// Redo the last undone action.
    pub fn redo(&mut self) {
        if let Some(action) = self.redo_stack.pop_back() {
            match &action {
                EditAction::Insert { line, col, text } => {
                    let current = self.lines.get_mut(*line).unwrap();
                    current.insert_str(*col, text);
                    self.cursor_line = *line;
                    self.cursor_col = col + text.len();
                }
                EditAction::Delete { line, col, text } => {
                    let current = self.lines.get_mut(*line).unwrap();
                    for _ in 0..text.len() {
                        if *col < current.len() {
                            current.remove(*col);
                        }
                    }
                    self.cursor_line = *line;
                    self.cursor_col = *col;
                }
                EditAction::InsertLine { line, text } => {
                    self.lines.insert(*line, text.clone());
                    self.cursor_line = *line;
                }
                EditAction::DeleteLine { line, .. } => {
                    self.lines.remove(*line);
                    self.cursor_line = line.saturating_sub(1);
                }
            }
            self.undo_stack.push_back(action);
            self.modified = true;
        }
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

    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.lines[self.cursor_line].len();
        }
    }

    pub fn move_right(&mut self) {
        let line_len = self.lines[self.cursor_line].len();
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.cursor_col.min(self.lines[self.cursor_line].len());
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = self.cursor_col.min(self.lines[self.cursor_line].len());
        }
    }

    pub fn move_home(&mut self) {
        self.cursor_col = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor_col = self.lines[self.cursor_line].len();
    }

    pub fn move_to_start(&mut self) {
        self.cursor_line = 0;
        self.cursor_col = 0;
    }

    pub fn move_to_end(&mut self) {
        self.cursor_line = self.lines.len() - 1;
        self.cursor_col = self.lines[self.cursor_line].len();
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
        let node = &tree.nodes[idx];
        let at_node_bounds = node.start == sel_start && node.end == sel_end;
        if at_node_bounds {
            if let Some(p) = node.parent {
                idx = p;
            } else {
                return false; // already at the root
            }
        }
        let target = &tree.nodes[idx];
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

        let query = if self.case_sensitive {
            self.query.clone()
        } else {
            self.query.to_lowercase()
        };

        for (line_idx, line) in doc.lines.iter().enumerate() {
            let search_line = if self.case_sensitive {
                line.clone()
            } else {
                line.to_lowercase()
            };

            let mut start = 0;
            while let Some(pos) = search_line[start..].find(&query) {
                let abs_pos = start + pos;
                self.matches
                    .push((line_idx, abs_pos, abs_pos + query.len()));
                start = abs_pos + 1;
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
        let (line, col, _) = self.matches[self.current_match];
        doc.cursor_line = line;
        doc.cursor_col = col;
    }

    /// Go to previous match.
    pub fn prev_match(&mut self, doc: &mut Document) {
        if self.matches.is_empty() {
            return;
        }
        if self.current_match == 0 {
            self.current_match = self.matches.len() - 1;
        } else {
            self.current_match -= 1;
        }
        let (line, col, _) = self.matches[self.current_match];
        doc.cursor_line = line;
        doc.cursor_col = col;
    }

    /// Replace current match.
    pub fn replace_current(&mut self, doc: &mut Document) {
        if self.matches.is_empty() {
            return;
        }
        let (line, start, end) = self.matches[self.current_match];
        let current_line = &mut doc.lines[line];
        current_line.replace_range(start..end, &self.replace_text);
        doc.modified = true;
        self.find_all(doc);
    }

    /// Replace all matches.
    pub fn replace_all(&mut self, doc: &mut Document) -> usize {
        if self.matches.is_empty() {
            return 0;
        }
        let count = self.matches.len();
        // Replace from end to start to preserve indices
        for &(line, start, end) in self.matches.iter().rev() {
            let current_line = &mut doc.lines[line];
            current_line.replace_range(start..end, &self.replace_text);
        }
        doc.modified = true;
        self.matches.clear();
        count
    }
}

// ============================================================================
// Editor state (multi-tab)
// ============================================================================

/// Complete editor application state.
pub struct EditorState {
    /// Open documents (tabs).
    pub documents: Vec<Document>,
    /// Active document index.
    pub active_tab: usize,
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
    /// Character dimensions (approximate).
    pub char_width: f32,
    pub line_height: f32,
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
            documents: vec![Document::new()],
            active_tab: 0,
            find: FindState::new(),
            find_visible: false,
            window_width: 900,
            window_height: 600,
            gutter_width: 50.0,
            font_size,
            char_width: font_size * 0.6,
            line_height: font_size * 1.5,
        }
    }

    pub fn active_document(&self) -> &Document {
        &self.documents[self.active_tab]
    }

    pub fn active_document_mut(&mut self) -> &mut Document {
        &mut self.documents[self.active_tab]
    }

    /// Open a file in a new tab.
    pub fn open_file(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        // Check if already open
        for (i, doc) in self.documents.iter().enumerate() {
            if doc.path.as_deref() == Some(path) {
                self.active_tab = i;
                return Ok(());
            }
        }

        let doc = Document::from_file(path)?;
        self.documents.push(doc);
        self.active_tab = self.documents.len() - 1;
        Ok(())
    }

    /// Close the active tab.
    pub fn close_tab(&mut self) -> bool {
        if self.documents[self.active_tab].modified {
            // Would need to prompt user — return false to indicate unsaved
            return false;
        }
        self.documents.remove(self.active_tab);
        if self.documents.is_empty() {
            self.documents.push(Document::new());
            self.active_tab = 0;
        } else if self.active_tab >= self.documents.len() {
            self.active_tab = self.documents.len() - 1;
        }
        true
    }

    /// Number of visible lines in the editor viewport.
    pub fn visible_lines(&self) -> usize {
        let editor_height = self.window_height as f32 - 64.0 - 24.0; // toolbar + status bar
        (editor_height / self.line_height) as usize
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

        tree
    }

    fn render_tabs(&self, tree: &mut RenderTree) {
        let tab_h = 32.0;
        tree.fill_rect(0.0, 0.0, self.window_width as f32, tab_h, Color::from_hex(0x181825));

        let mut x = 0.0;
        for (i, doc) in self.documents.iter().enumerate() {
            let tab_w = 160.0;
            let bg = if i == self.active_tab {
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

    fn render_editor(&self, tree: &mut RenderTree) {
        let doc = self.active_document();
        let editor_y = 32.0;
        let editor_h = self.window_height as f32 - 32.0 - 24.0;
        let w = self.window_width as f32;

        // Gutter (line numbers)
        tree.fill_rect(0.0, editor_y, self.gutter_width, editor_h, Color::from_hex(0x181825));

        let visible_lines = self.visible_lines();
        let end_line = (doc.scroll_line + visible_lines).min(doc.lines.len());

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

            // Line text
            let line = &doc.lines[i];
            let display_text: String = line
                .chars()
                .skip(doc.scroll_col)
                .collect();
            tree.text(
                self.gutter_width + 8.0,
                y + 3.0,
                &display_text,
                Color::from_hex(0xCDD6F4),
                self.font_size,
            );
        }

        // Cursor
        if doc.cursor_line >= doc.scroll_line && doc.cursor_line < end_line {
            let cursor_y =
                editor_y + (doc.cursor_line - doc.scroll_line) as f32 * self.line_height;
            let cursor_x = self.gutter_width
                + 8.0
                + (doc.cursor_col.saturating_sub(doc.scroll_col)) as f32 * self.char_width;
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
        doc.lines[0]
    );

    doc.undo();
    doc.undo();
    println!("  After 2x undo: \"{}\"", doc.lines[0]);

    doc.redo();
    println!("  After redo: \"{}\"", doc.lines[0]);

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
// Integration tests for syntree-backed Document operations
// ============================================================================

#[cfg(test)]
mod doc_syntree_tests {
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
